//! 被攻撃時の反撃/回避/防御の選択 (リアクション) と武器選択ウィンドウ。

use super::*;

impl App {
    /// 反撃モードのプロンプトを出すべき防御側か (味方ユニットのみ)。
    pub(super) fn reaction_applies(&self, def_uid: &str) -> bool {
        self.database
            .unit_by_uid(def_uid)
            .map(|u| u.party == crate::Party::Player)
            .unwrap_or(false)
    }

    /// 防御側が攻撃側まで射程内の武器を持ち反撃できるか (read-only)。
    pub(super) fn defender_can_counter(&self, def_uid: &str, atk_uid: &str) -> bool {
        let (Some(def), Some(atk)) = (
            self.database.unit_by_uid(def_uid),
            self.database.unit_by_uid(atk_uid),
        ) else {
            return false;
        };
        // 行動不能のユニットは反撃できない。
        if def.attack_disabled() {
            return false;
        }
        let dist = combat::manhattan((def.x, def.y), (atk.x, atk.y));
        self.database
            .unit_by_name(&def.unit_data_name)
            .map(|du| du.weapons.iter().any(|w| combat::weapon_in_range(w, dist)))
            .unwrap_or(false)
    }

    /// 反撃手段の選択メニュー (反撃/回避/防御) を提示する。チャージ中の防御側は
    /// 強制「防御」で即解決 (SRC 準拠)。`PendingDialog::Menu` を流用して UI を出す。
    pub(super) fn begin_reaction_prompt(
        &mut self,
        atk_uid: String,
        def_uid: String,
        tile: (u32, u32),
    ) {
        // 行動不能 (麻痺/混乱/睡眠/捕縛 等) は反撃も能動的な回避/防御も選べない
        // (SRC `MaxAction()==0`: 反撃不可・選択不可)。プロンプト無しで素通り解決する
        // (def_mode 空 = 命中率/ダメージ補正なし、反撃は try_counterattack 側で抑止)。
        let disabled = self
            .database
            .unit_by_uid(&def_uid)
            .map(|u| u.attack_disabled())
            .unwrap_or(false);
        if disabled {
            self.set_cursor_to_uid(&atk_uid);
            self.attack_resolve_and_run(Some(tile), false, "");
            return;
        }
        // チャージ中は強制防御 (プロンプト無し)。
        let charging = self
            .database
            .unit_by_uid(&def_uid)
            .map(|u| u.charged)
            .unwrap_or(false);
        if charging {
            self.set_cursor_to_uid(&atk_uid);
            self.attack_resolve_and_run(Some(tile), false, "防御");
            return;
        }
        let can_counter = self.defender_can_counter(&def_uid, &atk_uid);
        let mut options: Vec<String> = Vec::new();
        let mut modes: Vec<String> = Vec::new();
        if can_counter {
            options.push("反撃".to_string());
            modes.push("反撃".to_string());
        }
        options.push("回避".to_string());
        modes.push("回避".to_string());
        options.push("防御".to_string());
        modes.push("防御".to_string());
        // 隣接に援護防御可能な味方がいれば「援護防御」を選べる (SRC 反撃モード)。
        if self.support_guard_available(&def_uid) {
            options.push("援護防御".to_string());
            modes.push("援護防御".to_string());
        }
        let nick = self
            .database
            .unit_by_uid(&def_uid)
            .map(|u| u.pilot_name.clone())
            .unwrap_or_default();
        self.pending_reaction = Some(PendingReaction {
            atk_uid,
            target_tile: tile,
            modes,
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: format!("{nick} 反撃手段を選択"),
            options,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            // 反撃メニューは pending_reaction が別途処理する (キャンセルは右クリック)。
            non_cancellable: false,
        });
    }

    /// カーソルを uid のユニット位置へ移す内部ヘルパ。
    pub(super) fn set_cursor_to_uid(&mut self, uid: &str) {
        if let Some(u) = self.database.unit_by_uid(uid) {
            self.map_cursor = Some((u.x, u.y));
        }
    }

    /// 反撃モードのメニュー応答を処理する。`respond_dialog` から委譲される。
    /// `choice` は 1-based のメニュー選択 (0/範囲外は先頭=反撃/回避にフォールバック)。
    pub(super) fn resolve_reaction(&mut self, choice: u32) -> bool {
        let Some(pr) = self.pending_reaction.take() else {
            return false;
        };
        self.pending_dialog = None; // 反撃メニューを閉じる
        let idx = choice as usize;
        let mode = if idx >= 1 && idx <= pr.modes.len() {
            pr.modes[idx - 1].clone()
        } else {
            pr.modes.first().cloned().unwrap_or_default()
        };
        self.set_cursor_to_uid(&pr.atk_uid);
        // 「反撃」選択時、反撃武器が複数あり武器選択ウィンドウが有効なら、どの反撃
        // 武器で反撃するかを選ばせる (使用可能が 1 本以下なら自動選定のまま解決)。
        if mode == "反撃" && self.weapon_window_enabled {
            if let (Some(atk_idx), Some(def_idx)) = (
                self.database.idx_by_uid(&pr.atk_uid),
                self.database
                    .unit_instances
                    .iter()
                    .position(|u| !u.off_map && (u.x, u.y) == pr.target_tile),
            ) {
                let usable_cnt = self
                    .build_weapon_rows(def_idx, atk_idx, false)
                    .iter()
                    .filter(|r| r.usable)
                    .count();
                if usable_cnt >= 2 {
                    let def_uid = self.database.unit_instances[def_idx].uid.clone();
                    let atk_tile = {
                        let a = &self.database.unit_instances[atk_idx];
                        (a.x, a.y)
                    };
                    self.begin_counter_weapon_select(
                        def_uid,
                        atk_tile,
                        pr.atk_uid.clone(),
                        pr.target_tile,
                    );
                    return true;
                }
            }
        }
        self.attack_resolve_and_run(Some(pr.target_tile), false, &mode);
        // ランナーは pending_dialog が消えたので次 tick で継続する。
        true
    }

    /// `atk_idx` が `def_idx` を攻撃するときの使用武器・攻撃力・基準命中率 (%) を
    /// 予測する。`attack_resolve_and_run` と同じ武器選定 (`pick_attack_weapon`) と
    /// 予測 (`forecast_strike`) を使い、反撃ウィンドウの
    /// 命中率表示に用いる (読み取り専用、盤面は変更しない)。射程内に使用可能武器が
    /// 無ければ `None`。
    pub(crate) fn attack_hit_forecast(
        &self,
        atk_idx: usize,
        def_idx: usize,
    ) -> Option<(String, i64, i32)> {
        let (_, atk_unit) = self.database.effective_combat_data(atk_idx)?;
        let a = &self.database.unit_instances[atk_idx];
        let d = &self.database.unit_instances[def_idx];
        let dist = combat::manhattan((a.x, a.y), (d.x, d.y));
        let weapon = Self::pick_attack_weapon(
            &atk_unit,
            dist,
            None,
            a.charged,
            false,
            a.has_condition("突撃"),
            |w| self.weapon_firable(atk_idx, w),
            |w| self.weapon_skill_ok(atk_idx, w),
        )?;
        let preview = self.forecast_strike(atk_idx, def_idx, &weapon)?;
        Some((
            weapon.name.clone(),
            weapon.power,
            preview.hit_chance.clamp(0, 100),
        ))
    }

    /// `weapon` を `dist` の目標へ `post_move` 状況で使用可能か (射程 / チャージ /
    /// 移動後 / 突撃 / 資源 / 必要技能)。武器選択窓の行表示・選択可否に使う。
    pub(super) fn weapon_usable_vs(
        &self,
        atk_idx: usize,
        weapon: &crate::data::unit::WeaponData,
        dist: u32,
        post_move: bool,
    ) -> bool {
        let u = &self.database.unit_instances[atk_idx];
        let totsugeki = u.has_condition("突撃");
        !combat::is_map_weapon(weapon)
            && combat::weapon_in_range(weapon, dist)
            && (u.charged || !combat::is_charge_weapon(weapon))
            && Self::weapon_usable_post_move(weapon, post_move, totsugeki)
            && self.weapon_firable(atk_idx, weapon)
            && self.weapon_skill_ok(atk_idx, weapon)
    }

    /// `atk_uid` がタイル `target` の敵対ユニットを `post_move` 状況で攻撃可能か
    /// (射程内に使用可能な武器が 1 つでもあるか)。武器選択窓を出すか即解決するかの判定。
    pub(super) fn can_attack_target(
        &self,
        atk_uid: &str,
        target: (u32, u32),
        post_move: bool,
    ) -> bool {
        let Some(atk_idx) = self.database.idx_by_uid(atk_uid) else {
            return false;
        };
        let atk = &self.database.unit_instances[atk_idx];
        let (atk_party, ax, ay) = (atk.party, atk.x, atk.y);
        let hostile = self
            .database
            .unit_instances
            .iter()
            .any(|u| !u.off_map && (u.x, u.y) == target && u.party.is_hostile_to(atk_party));
        if !hostile {
            return false;
        }
        let Some((_, atk_unit)) = self.database.effective_combat_data(atk_idx) else {
            return false;
        };
        let dist = combat::manhattan((ax, ay), target);
        atk_unit
            .weapons
            .iter()
            .any(|w| self.weapon_usable_vs(atk_idx, w, dist, post_move))
    }

    /// プレイヤー攻撃の後始末 (行動終了 / 行動終了・接触イベント / Browse 復帰)。
    /// 即解決経路と武器選択経路の両方から呼ぶ。
    pub(super) fn finish_player_attack(&mut self, uid: &str) {
        if let Some(u) = self.database.unit_by_uid_mut(uid) {
            u.has_acted = true;
        }
        if let Some(i) = self.database.idx_by_uid(uid) {
            crate::event_runtime::fire_action_end_labels(self, i);
            crate::event_runtime::fire_contact_event_labels(self, i);
        }
        self.action_mode = crate::command_menu::ActionMode::Browse;
        self.command_menu = None;
    }

    /// 武器選択ウィンドウを開始する。武器名一覧を `PendingDialog::Menu` に積み
    /// (番号キー / クリックのルーティング用)、`pending_weapon_select` を保持する。
    /// 武装が無ければ即解決へフォールバック。
    pub(super) fn begin_weapon_select(
        &mut self,
        atk_uid: String,
        target: (u32, u32),
        post_move: bool,
    ) {
        let names: Vec<String> = self
            .database
            .idx_by_uid(&atk_uid)
            .and_then(|i| self.database.unit_instances.get(i))
            .and_then(|u| self.database.unit_by_name(&u.unit_data_name))
            .map(|d| d.weapons.iter().map(|w| w.name.clone()).collect())
            .unwrap_or_default();
        if names.is_empty() {
            if self.attack_unit_at(&atk_uid, target, post_move) {
                self.finish_player_attack(&atk_uid);
            }
            return;
        }
        self.pending_weapon_select = Some(PendingWeaponSelect {
            atk_uid,
            target,
            post_move,
            counter: None,
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: "武器選択".to_string(),
            options: names,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            non_cancellable: false,
        });
    }

    /// 反撃武器選択を開始する。`def_uid` (反撃する側) の反撃武器一覧を Menu に積み、
    /// 解決時に選んだ武器を `forced_counter_weapon` に設定して主攻撃を `反撃` で解決する。
    /// `main_atk_uid` / `main_target` は主攻撃 (元の攻撃) の文脈。
    pub(super) fn begin_counter_weapon_select(
        &mut self,
        def_uid: String,
        atk_tile: (u32, u32),
        main_atk_uid: String,
        main_target: (u32, u32),
    ) {
        let names: Vec<String> = self
            .database
            .unit_by_uid(&def_uid)
            .and_then(|u| self.database.unit_by_name(&u.unit_data_name))
            .map(|d| d.weapons.iter().map(|w| w.name.clone()).collect())
            .unwrap_or_default();
        self.pending_weapon_select = Some(PendingWeaponSelect {
            atk_uid: def_uid,
            target: atk_tile,
            post_move: false,
            counter: Some(CounterCtx {
                main_atk_uid,
                main_target,
            }),
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: "反撃武器選択".to_string(),
            options: names,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            non_cancellable: false,
        });
    }

    /// 武器選択窓の行データを構築する (機体の武装順)。命中率は `forecast_strike`。
    pub(crate) fn build_weapon_rows(
        &self,
        atk_idx: usize,
        def_idx: usize,
        post_move: bool,
    ) -> Vec<crate::scene::weapon_select::WeaponSelectRow> {
        use crate::scene::weapon_select::WeaponSelectRow;
        let Some((_, atk_unit)) = self.database.effective_combat_data(atk_idx) else {
            return Vec::new();
        };
        let (a, d) = (
            &self.database.unit_instances[atk_idx],
            &self.database.unit_instances[def_idx],
        );
        let dist = combat::manhattan((a.x, a.y), (d.x, d.y));
        let atk_inst = &self.database.unit_instances[atk_idx];
        atk_unit
            .weapons
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let usable = self.weapon_usable_vs(atk_idx, w, dist, post_move);
                let hit_pct = if usable {
                    self.forecast_strike(atk_idx, def_idx, w)
                        .map_or(0, |p| p.hit_chance.clamp(0, 100))
                } else {
                    0
                };
                let uw = atk_inst.weapons.iter().find(|uw| uw.weapon_index == i);
                let ammo = if w.bullet > 0 {
                    let rem = uw.map_or(w.bullet, |x| x.bullet_remaining.max(0));
                    format!("残{}/{}", rem, w.bullet)
                } else if w.en_consumption > 0 {
                    format!("EN{}", w.en_consumption)
                } else {
                    "-".to_string()
                };
                WeaponSelectRow {
                    name: w.name.clone(),
                    power: w.power,
                    hit_pct,
                    critical: w.critical,
                    ammo,
                    adaption: w.adaption.clone(),
                    class: w.class.clone(),
                    usable,
                }
            })
            .collect()
    }

    /// 武器選択メニュー応答を処理する (`respond_dialog` から委譲)。`choice` は
    /// 1-based。選んだ武器が使用不可なら無視 (窓継続)。使用可能なら固定して攻撃・解決。
    pub(super) fn resolve_weapon_select(&mut self, choice: u32) -> bool {
        let Some(pws) = self.pending_weapon_select.clone() else {
            return false;
        };
        let Some(atk_idx) = self.database.idx_by_uid(&pws.atk_uid) else {
            self.pending_weapon_select = None;
            self.pending_dialog = None;
            return false;
        };
        let widx = choice as usize; // 1-based 武器番号
                                    // 選んだ武器の使用可否を検証 (× 行は無視)。
        let usable = self
            .database
            .effective_combat_data(atk_idx)
            .and_then(|(_, u)| u.weapons.get(widx.wrapping_sub(1)).cloned())
            .map(|w| {
                let dist = {
                    let a = &self.database.unit_instances[atk_idx];
                    combat::manhattan((a.x, a.y), pws.target)
                };
                self.weapon_usable_vs(atk_idx, &w, dist, pws.post_move)
            })
            .unwrap_or(false);
        if widx == 0 || !usable {
            return true; // 無効選択 → 窓を継続
        }
        self.pending_weapon_select = None;
        self.pending_dialog = None;
        if let Some(cc) = pws.counter {
            // 反撃武器選択: 選んだ武器を反撃武器に固定し、主攻撃を「反撃」で解決する
            // (try_counterattack が forced_counter_weapon を take して使う)。
            self.forced_counter_weapon = Some(widx - 1);
            self.set_cursor_to_uid(&cc.main_atk_uid);
            self.attack_resolve_and_run(Some(cc.main_target), false, "反撃");
            self.forced_counter_weapon = None;
            return true;
        }
        // 通常攻撃: 選んだ武器を固定して攻撃 (1回限り; 終了後に W キー設定へ戻す)。
        let prev = self.selected_weapon_idx;
        self.selected_weapon_idx = widx;
        let attacked = self.attack_unit_at(&pws.atk_uid, pws.target, pws.post_move);
        self.selected_weapon_idx = prev;
        if attacked {
            self.finish_player_attack(&pws.atk_uid);
        }
        true
    }

    /// 武器選択をキャンセルする (右クリック / Esc)。AttackSelect 状態は維持し、
    /// 目標を選び直せるようにする。
    pub(super) fn cancel_weapon_select(&mut self) -> bool {
        if self.pending_weapon_select.take().is_some() {
            self.pending_dialog = None;
            true
        } else {
            false
        }
    }
}
