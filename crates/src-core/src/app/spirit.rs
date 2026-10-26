//! 精神コマンド (スペシャルパワー) の選択・対象指定・効果適用。

use super::*;

impl App {
    /// ユニットの主パイロットのレベルを返す。`PilotInstance` があればその level、
    /// 無ければ `UnitInstance.total_exp` から算出 (`db::effective_pilot_data` と同式)。
    pub(super) fn unit_pilot_level(&self, u: &crate::UnitInstance) -> i32 {
        let pilot_id = u
            .pilot_ids
            .first()
            .map(|s| s.as_str())
            .unwrap_or(u.pilot_name.as_str());
        if let Some(pi) = self
            .database
            .pilot_instances
            .iter()
            .find(|p| p.id == pilot_id || p.pilot_data_name == pilot_id)
        {
            return pi.level;
        }
        crate::pilot_instance::level_from_exp(u.total_exp)
    }

    /// ユニットの主パイロットの (最大 SP, 残り SP) を返す。`PilotInstance` があれば
    /// その `sp_remaining`、無ければ `PilotData.sp - UnitInstance.sp_consumed`。
    pub(super) fn unit_sp(&self, u: &crate::UnitInstance) -> (i32, i32) {
        let pilot_id = u
            .pilot_ids
            .first()
            .map(|s| s.as_str())
            .unwrap_or(u.pilot_name.as_str());
        if let Some(pi) = self
            .database
            .pilot_instances
            .iter()
            .find(|p| p.id == pilot_id || p.pilot_data_name == pilot_id)
        {
            let max = self
                .database
                .pilot_by_name(&pi.pilot_data_name)
                .and_then(|p| p.sp)
                .unwrap_or(0);
            return (max, pi.sp_remaining.max(0));
        }
        let max = self
            .database
            .pilot_by_name(&u.pilot_name)
            .and_then(|p| p.sp)
            .unwrap_or(0);
        (max, (max - u.sp_consumed).max(0))
    }

    /// 精神コマンド `name` の消費 SP を解決する。コマンド定義のコスト指定 (`override_cost`)
    /// を最優先、次に sp.txt (`special_powers`) の既定、最後に組込みテーブル。
    pub(super) fn spirit_cost(&self, name: &str, override_cost: Option<i32>) -> i32 {
        if let Some(c) = override_cost {
            return c;
        }
        if let Some(sp) = self
            .database
            .special_powers
            .iter()
            .find(|sp| sp.name == name)
        {
            if sp.sp_consumption > 0 {
                return sp.sp_consumption;
            }
        }
        crate::event_runtime::sp_cost_for(name)
    }

    /// 当該ユニットが現在発動可能な精神コマンドの (名前, 消費SP) 列を返す。
    /// 条件: パイロットが習得済み (`level <= パイロットレベル`) かつ 消費 SP が残り SP 以内。
    pub(super) fn spirit_command_options(&self, uid: &str) -> Vec<(String, i32)> {
        let Some(u) = self.database.unit_by_uid(uid) else {
            return Vec::new();
        };
        // 憑依 (特殊効果攻撃属性 憑): 支配されたユニットはスペシャルパワーを使用できない。
        // プレイヤー UI (`open_spirit_menu`) と AI 精神 (`ai_use_*_spirit`) はいずれも
        // ここを唯一の関門にしているため、空を返せば全経路で封じられる。
        if u.has_condition("憑依") {
            return Vec::new();
        }
        let Some(pdata) = self.database.pilot_by_name(&u.pilot_name) else {
            return Vec::new();
        };
        if pdata.spirit_commands.is_empty() {
            return Vec::new();
        }
        let level = self.unit_pilot_level(u);
        let (_max, remaining) = self.unit_sp(u);
        let mut out = Vec::new();
        for sc in &pdata.spirit_commands {
            if sc.level > level {
                continue;
            }
            let cost = self.spirit_cost(&sc.name, sc.cost);
            if cost > remaining {
                continue;
            }
            // 同名コマンドの重複や既発動中 (condition 保持) は除外。
            if out.iter().any(|(n, _): &(String, i32)| n == &sc.name) {
                continue;
            }
            out.push((sc.name.clone(), cost));
        }
        out
    }

    /// 精神コマンドのサブメニュー (SP コマンド一覧) を開く。
    pub(super) fn open_spirit_menu(&mut self, uid: &str) {
        let commands = self.spirit_command_options(uid);
        if commands.is_empty() {
            self.push_message("発動できる精神コマンドがありません".to_string());
            return;
        }
        let (nick, remaining) = self
            .database
            .unit_by_uid(uid)
            .map(|u| (u.pilot_name.clone(), self.unit_sp(u).1))
            .unwrap_or_default();
        let options: Vec<String> = commands
            .iter()
            .map(|(name, cost)| format!("{name} ({cost})"))
            .collect();
        self.pending_spirit = Some(PendingSpirit {
            uid: uid.to_string(),
            commands,
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: format!("{nick} 精神コマンド (SP {remaining})"),
            options,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            // キャンセルは Esc / choice 0 (pending_spirit が別途処理する)。
            non_cancellable: false,
        });
    }

    /// 精神コマンドのサブメニュー応答を処理する。`respond_dialog` から委譲される。
    /// `choice` は 1-based (0 / 範囲外はキャンセル)。発動後はユニットメニューを再表示し、
    /// 続けて別の精神コマンドや移動 / 攻撃を選べるようにする (SRC: 精神は行動を消費しない)。
    pub(super) fn resolve_spirit(&mut self, choice: u32) -> bool {
        let Some(ps) = self.pending_spirit.take() else {
            return false;
        };
        self.pending_dialog = None; // サブメニューを閉じる
        let idx = choice as usize;
        if idx == 0 || idx > ps.commands.len() {
            // キャンセル: ユニットメニューを再表示。
            self.reopen_unit_menu_for(&ps.uid);
            return true;
        }
        let (name, cost) = ps.commands[idx - 1].clone();
        // 念のため残り SP を再確認 (連続発動で枯渇している場合)。
        let remaining = self
            .database
            .unit_by_uid(&ps.uid)
            .map(|u| self.unit_sp(u).1)
            .unwrap_or(0);
        if cost > remaining {
            self.push_message(format!("SP が足りません ({name}: {cost})"));
            self.reopen_unit_menu_for(&ps.uid);
            return true;
        }
        match self.spirit_target_kind(&name) {
            // 対象選択が必要なコマンドは AttackSelect と同様の選択モードへ遷移し、
            // 対象確定時 (`apply_spirit_to_target`) に SP を消費する。キャンセル
            // (右クリック / Esc) では SP を消費しないため、ここでは消費しない。
            kind @ (SpiritTargetKind::SingleAlly | SpiritTargetKind::SingleEnemy) => {
                self.begin_spirit_target(&ps.uid, &name, cost, kind);
                return true;
            }
            SpiritTargetKind::SelfOnly => {
                self.consume_unit_sp(&ps.uid, cost);
                let caster = ps.uid.clone();
                self.apply_spirit_effect(&caster, &name);
            }
            SpiritTargetKind::AllAllies => {
                self.consume_unit_sp(&ps.uid, cost);
                let party = self.database.unit_by_uid(&ps.uid).map(|u| u.party);
                if let Some(party) = party {
                    let targets: Vec<String> = self
                        .database
                        .unit_instances
                        .iter()
                        .filter(|u| u.party == party)
                        .map(|u| u.uid.clone())
                        .collect();
                    for t in &targets {
                        self.apply_spirit_effect(t, &name);
                    }
                }
            }
        }
        self.push_message(format!("精神コマンド【{name}】発動 (SP -{cost})"));
        // 続けて発動 / 行動できるようユニットメニューを再表示。
        self.reopen_unit_menu_for(&ps.uid);
        true
    }

    /// 精神コマンド `name` の対象種別を解決する。sp.txt の `TargetType` を優先。
    pub(super) fn spirit_target_kind(&self, name: &str) -> SpiritTargetKind {
        if let Some(sp) = self.database.special_powers.iter().find(|s| s.name == name) {
            match sp.target_type.as_str() {
                "自分" | "自分のみ" => return SpiritTargetKind::SelfOnly,
                // SRC `SpecialPowerData.cls` Execute 準拠: `全味方`/`全体` のみ全体、
                // `味方`/`単体`/`指定` は単体選択 (旧実装は `味方` を全体と誤判定していた)。
                "全体" | "味方全体" | "全味方" => return SpiritTargetKind::AllAllies,
                "単体" | "味方単体" | "指定" | "味方" => {
                    return SpiritTargetKind::SingleAlly
                }
                "敵単体" | "敵" | "敵指定" => return SpiritTargetKind::SingleEnemy,
                _ => {}
            }
        }
        // 組込み既定 (sp.txt 未定義時)。
        match name {
            "友情" | "愛" | "鼓舞" => SpiritTargetKind::AllAllies,
            "信頼" | "補給" | "祝福" | "応援" | "激励" | "再動" | "癒し" => {
                SpiritTargetKind::SingleAlly
            }
            "脱力" | "戦慄" | "かく乱" | "威圧" | "魅惑" | "足かせ" => {
                SpiritTargetKind::SingleEnemy
            }
            _ => SpiritTargetKind::SelfOnly,
        }
    }

    /// 精神コマンドの効果を `target` ユニットへ適用する。継続効果は `lifetime=1`
    /// の condition として付与し (戦闘 / 移動 / 撃破報酬 / 復活判定が名前で参照)、
    /// 瞬間効果 (HP/EN/弾/気力/再行動) は即時に反映する。
    pub(super) fn apply_spirit_effect(&mut self, target: &str, name: &str) {
        // 敵対象 (攻撃系) の精神コマンドは、対象が `スペシャルパワー無効化` /
        // `精神コマンド無効化` を持つと効果が無効化される (SRC `SpecialPowerData.cs:523-540`:
        // TargetType 敵/全敵/任意/全 の SP は当該 condition を持つユニットに効果なし)。
        // SP コストは発動側で既に消費済みで、対象への効果だけを無効化する。
        if matches!(self.spirit_target_kind(name), SpiritTargetKind::SingleEnemy) {
            let immune = self.database.unit_by_uid(target).map(|u| {
                (
                    u.has_condition("スペシャルパワー無効化")
                        || u.has_condition("精神コマンド無効化"),
                    u.unit_data_name.clone(),
                )
            });
            if let Some((true, nm)) = immune {
                self.push_message(format!("{nm} には精神コマンド【{name}】が効かない！"));
                return;
            }
        }
        match name {
            // ── 継続効果: combat.rs / movement / 撃破報酬 / 復活判定が名前で参照 ──
            // (必中/集中/ひらめき/熱血/魂/気合/不屈/鉄壁 は既に combat.rs が解釈。
            //  加速/神速→movement, 復活→撃破時, 幸運/努力→撃破報酬, 突撃→攻撃判定。)
            "必中" | "集中" | "ひらめき" | "熱血" | "魂" | "気合" | "不屈" | "鉄壁" | "加速"
            | "神速" | "復活" | "幸運" | "努力" | "突撃" | "捨て身" | "直撃" => {
                self.add_unit_condition(target, name, 1);
            }
            // ── 瞬間: 再行動 (行動済みフラグを戻す) ──
            "覚醒" | "再動" => self.reset_unit_action(target),
            // ── 瞬間: HP 回復 ──
            "信頼" => self.spirit_heal_fraction(target, 3), // 最大HPの 1/3
            "友情" => self.spirit_heal_fraction(target, 2), // 最大HPの 1/2
            "根性" => self.spirit_heal_fraction(target, 3),
            "愛" | "ド根性" => self.spirit_heal_full(target),
            // ── 瞬間: EN・弾数全快 (補給は気力 -10) ──
            "補給" => {
                self.spirit_resupply(target);
                self.add_unit_morale(target, -10);
            }
            "瞑想" => self.spirit_resupply(target),
            // ── 瞬間: 気力増減 ──
            "脱力" => self.add_unit_morale(target, -10),
            "激励" => self.add_unit_morale(target, 10),
            "鼓舞" => self.add_unit_morale(target, 5),
            // ── 効果付与: 対象に別コマンドの継続効果を与える ──
            "応援" => self.add_unit_condition(target, "努力", 1),
            "祝福" => self.add_unit_condition(target, "幸運", 1),
            // ── 複合 (奇跡 = 加速+魂+必中+ひらめき+幸運+気力+全快) ──
            "奇跡" => {
                for n in ["加速", "魂", "必中", "ひらめき", "幸運"] {
                    self.add_unit_condition(target, n, 1);
                }
                self.add_unit_morale(target, 30);
                self.spirit_heal_full(target);
            }
            "奇襲" => {
                for n in ["加速", "熱血", "必中", "ひらめき"] {
                    self.add_unit_condition(target, n, 1);
                }
            }
            // ── 既定: SRC 標準以外 (シナリオ独自定義) の精神コマンドは継続 condition として
            //   名前のまま付与するに留める。効果はシナリオ側 (sp.txt の効果種別 / イベント) で
            //   解決すべきであり、原典 (SRC_20121125 / SRC.Sharp) に定義の無いコマンドの効果を
            //   エンジンへ独自実装 (推測) しない。例: 東方夢想伝の 決意/気迫/希望 は原典に無く、
            //   ここでは無害な named condition となる (combat.rs は解釈しない)。 ──
            _ => self.add_unit_condition(target, name, 1),
        }
    }

    /// `target` に lifetime=1 の condition を付与する (発動陣営の次フェイズ開始で解除)。
    pub(super) fn add_unit_condition(&mut self, uid: &str, name: &str, lifetime: i32) {
        if let Some(u) = self.database.unit_by_uid_mut(uid) {
            u.add_condition(crate::Condition::new(name, lifetime));
        }
    }

    /// `target` の気力を `delta` 増減する (0..=150 にクランプ)。
    pub(super) fn add_unit_morale(&mut self, uid: &str, delta: i32) {
        if let Some(u) = self.database.unit_by_uid_mut(uid) {
            u.morale = (u.morale + delta).clamp(0, 150);
        }
    }

    /// `target` の行動済み / 移動済みフラグを解除し、再行動可能にする。
    pub(super) fn reset_unit_action(&mut self, uid: &str) {
        if let Some(u) = self.database.unit_by_uid_mut(uid) {
            u.has_acted = false;
            u.has_moved = false;
        }
    }

    /// `target` の HP を 最大HP/`denom` だけ回復する (撃破はしない / 既に最大なら無効)。
    /// `uid` がゾンビ状態 (特殊効果攻撃属性 ゾ) で、アビリティ / 精神 / 修理補給 等の
    /// 能動的な HP/EN 回復を受けられないか。地形・特殊能力による自然回復は別 (回復可)。
    pub(super) fn recovery_blocked(&self, uid: &str) -> bool {
        self.database
            .unit_by_uid(uid)
            .is_some_and(|u| u.has_condition("ゾンビ"))
    }

    pub(super) fn spirit_heal_fraction(&mut self, uid: &str, denom: i64) {
        if self.recovery_blocked(uid) {
            return;
        }
        let Some(idx) = self.database.idx_by_uid(uid) else {
            return;
        };
        let max_hp = self
            .database
            .effective_max_hp(&self.database.unit_instances[idx]);
        let heal = (max_hp / denom).max(1);
        let u = &mut self.database.unit_instances[idx];
        u.damage = (u.damage - heal).max(0);
    }

    /// `target` の HP を全快する。
    pub(super) fn spirit_heal_full(&mut self, uid: &str) {
        if self.recovery_blocked(uid) {
            return;
        }
        if let Some(u) = self.database.unit_by_uid_mut(uid) {
            u.damage = 0;
        }
    }

    /// `target` の EN を全快し、全武器の残弾を初期値へ戻す (補給 / 瞑想)。
    pub(super) fn spirit_resupply(&mut self, uid: &str) {
        let Some(idx) = self.database.idx_by_uid(uid) else {
            return;
        };
        // ゾンビは EN 回復を受けられない (弾薬補給は可)。
        let en_blocked = self.recovery_blocked(uid);
        let unit_name = self.database.unit_instances[idx].unit_data_name.clone();
        let max_bullets: Vec<i32> = self
            .database
            .unit_by_name(&unit_name)
            .map(|d| d.weapons.iter().map(|w| w.bullet).collect())
            .unwrap_or_default();
        let u = &mut self.database.unit_instances[idx];
        if !en_blocked {
            u.en_consumed = 0;
        }
        for w in &mut u.weapons {
            if let Some(b) = max_bullets.get(w.weapon_index) {
                w.reset_ammo(*b);
            }
        }
    }

    /// 対象選択が要る精神コマンドの対象選択モードを開始する。メニュー / サブ
    /// メニューを閉じ、`ActionMode::SpiritTarget` へ遷移する。SP の消費は対象
    /// 確定時 (`apply_spirit_to_target`) に行う。
    pub(super) fn begin_spirit_target(
        &mut self,
        caster: &str,
        spirit: &str,
        cost: i32,
        kind: SpiritTargetKind,
    ) {
        use crate::command_menu::ActionMode;
        self.pending_dialog = None;
        self.command_menu = None;
        let target_enemy = kind == SpiritTargetKind::SingleEnemy;
        self.action_mode = ActionMode::SpiritTarget {
            caster: caster.to_string(),
            spirit: spirit.to_string(),
            cost,
            target_enemy,
        };
        let who = if target_enemy { "敵" } else { "味方" };
        self.push_message(format!(
            "精神【{spirit}】対象の{who}ユニットを選択 (右クリックでキャンセル)"
        ));
    }

    /// 精神コマンドの対象が確定したときに SP を消費し効果を適用する。
    /// 精神コマンド `name` が `イベント=<routine>` 効果を持つならサブルーチン名を返す
    /// (自作SP `生贄`→`生贄ルーチン`)。SRC `SpecialPowerData.cls` Execute の "イベント"。
    pub(super) fn spirit_event_routine(&self, name: &str) -> Option<String> {
        self.database
            .special_powers
            .iter()
            .find(|s| s.name == name)
            .and_then(|s| s.event_routine())
            .map(|s| s.to_string())
    }

    pub(super) fn apply_spirit_to_target(
        &mut self,
        caster: &str,
        target: &str,
        spirit: &str,
        cost: i32,
    ) {
        self.consume_unit_sp(caster, cost);
        self.apply_spirit_effect(target, spirit);
        // イベント= SP効果種別 (自作SP): 対象/相手ユニットＩＤ を束縛してサブルーチンを実行
        // (SRC `SpecialPowerData.cls` Execute の "イベント" 効果)。`生贄`→`生贄ルーチン` が
        // `SpecialPower 相手ユニットＩＤ みがわり 対象ユニットＩＤ` で選んだ味方を身代わりにする。
        // 対象ユニットＩＤ = 使用者, 相手ユニットＩＤ = 選んだ対象 (combat の束縛と同じ向き)。
        if let Some(routine) = self.spirit_event_routine(spirit) {
            self.set_script_var("対象ユニットＩＤ".to_string(), caster.to_string());
            self.set_script_var("相手ユニットＩＤ".to_string(), target.to_string());
            crate::event_runtime::trigger_label(self, &routine);
        }
        let target_nick = self
            .database
            .unit_by_uid(target)
            .map(|u| u.pilot_name.clone())
            .unwrap_or_default();
        self.push_message(format!(
            "精神コマンド【{spirit}】→ {target_nick} (SP -{cost})"
        ));
    }
}
