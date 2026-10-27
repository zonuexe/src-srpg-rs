//! ユニットコマンド: 援護・変形・アビリティ・発進/着艦・合体/分離・搭乗。

use super::*;

impl App {
    /// 修理 / 補給 (特殊能力 `修理装置` / `補給装置`) の対象になり得る隣接味方
    /// ユニットの uid 一覧を返す。原典 SRC: 対象は隣接 (距離1) の味方/ＮＰＣで、
    /// 修理は HP 減、補給は EN または残弾が減っているユニットに限る。
    pub(super) fn support_target_uids(
        &self,
        caster_uid: &str,
        kind: crate::command_menu::SupportKind,
    ) -> Vec<String> {
        let Some(c) = self.database.unit_by_uid(caster_uid) else {
            return Vec::new();
        };
        if c.off_map {
            return Vec::new();
        }
        let cpos = (c.x, c.y);
        let cparty = c.party;
        self.database
            .unit_instances
            .iter()
            .filter(|t| {
                !t.off_map
                    && t.uid != caster_uid
                    && cparty.is_ally_of(t.party)
                    && combat::manhattan(cpos, (t.x, t.y)) == 1
                    && self.support_needs(t, kind)
            })
            .map(|t| t.uid.clone())
            .collect()
    }

    /// `t` が `kind` の支援を必要としているか (修理=HP 減 / 補給=EN または残弾減)。
    pub(super) fn support_needs(
        &self,
        t: &crate::UnitInstance,
        kind: crate::command_menu::SupportKind,
    ) -> bool {
        use crate::command_menu::SupportKind;
        match kind {
            SupportKind::Repair => t.damage > 0,
            SupportKind::Supply => {
                if t.en_consumed > 0 {
                    return true;
                }
                let Some(d) = self.database.unit_by_name(&t.unit_data_name) else {
                    return false;
                };
                t.weapons.iter().any(|w| {
                    d.weapons
                        .get(w.weapon_index)
                        .is_some_and(|wd| wd.bullet >= 0 && w.bullet_remaining < wd.bullet)
                })
            }
        }
    }

    /// 修理 / 補給 コマンドの対象選択モードを開始する。メニューを閉じ
    /// `ActionMode::SupportTarget` へ遷移する。効果適用は対象確定時。
    pub(super) fn begin_support_target(
        &mut self,
        caster: &str,
        kind: crate::command_menu::SupportKind,
    ) {
        use crate::command_menu::ActionMode;
        self.pending_dialog = None;
        self.command_menu = None;
        self.action_mode = ActionMode::SupportTarget {
            caster: caster.to_string(),
            kind,
        };
        self.push_message(format!(
            "{} 対象の隣接ユニットを選択 (右クリックでキャンセル)",
            kind.label()
        ));
    }

    /// 修理 / 補給 の対象が確定したときに効果を適用する。
    /// 修理: HP 全回復 / 補給: EN・残弾 全回復 + 対象の気力 -10 (原典準拠)。
    pub(super) fn apply_support_to_target(
        &mut self,
        caster: &str,
        target: &str,
        kind: crate::command_menu::SupportKind,
    ) {
        use crate::command_menu::SupportKind;
        let tnick = self
            .database
            .unit_by_uid(target)
            .map(|u| u.pilot_name.clone())
            .unwrap_or_default();
        match kind {
            SupportKind::Repair => {
                // 修理装置 Lv 別回復率 (回復系特殊能力.md): なし/1=30% / 2=50% / 3+=100%。
                let lv = self
                    .database
                    .unit_by_uid(caster)
                    .and_then(|u| crate::feature::feature_level(&u.active_features, "修理装置"))
                    .unwrap_or(1);
                let pct: i64 = match lv {
                    ..=1 => 30,
                    2 => 50,
                    _ => 100,
                };
                // ゾンビ状態の対象は能動的な HP 回復を受けられない。
                let blocked = self.recovery_blocked(target);
                if let Some(idx) = self.database.idx_by_uid(target).filter(|_| !blocked) {
                    let max_hp = self
                        .database
                        .effective_max_hp(&self.database.unit_instances[idx]);
                    let heal = max_hp * pct / 100;
                    let u = &mut self.database.unit_instances[idx];
                    u.damage = (u.damage - heal).max(0);
                }
                self.push_message(format!("修理 → {tnick} (HP +{pct}%)"));
            }
            SupportKind::Supply => {
                self.spirit_resupply(target);
                self.add_unit_morale(target, -10);
                self.push_message(format!("補給 → {tnick} (EN・残弾 補給 / 気力 -10)"));
            }
        }
        // 修理 / 補給 を行った側は経験値を得る。獲得量は対象パイロットのレベルが
        // 高いほど多い (SRC: 相手のレベルが高いほど多い)。基準値は SRC 原典準拠で
        // 修理=100 / 補給=150 (exp スケールが 500/level になったため原典値へ復帰)。
        if let (Some(ci), Some(ti)) = (
            self.database.idx_by_uid(caster),
            self.database.idx_by_uid(target),
        ) {
            let base = match kind {
                SupportKind::Repair => 100,
                SupportKind::Supply => 150,
            };
            let actor_lv = self.unit_pilot_level(&self.database.unit_instances[ci]);
            let target_lv = self.unit_pilot_level(&self.database.unit_instances[ti]);
            let xp = support_exp_with_level_diff(base, target_lv, actor_lv);
            self.award_support_exp(ci, xp);
        }
    }

    /// ユニット `u` の特殊能力 `変形` から (表示ラベル, 変形先フォーム列) を返す。
    /// 原典書式: `変形=<変形の名称> <変形先1> <変形先2> …`。第 1 トークンが
    /// コマンド表示名、以降が変形先。DB に存在する形態のみ採用し、1 つも無ければ
    /// `None`（メニューに出さない）。
    pub(super) fn transform_forms(&self, u: &crate::UnitInstance) -> Option<(String, Vec<String>)> {
        let val = crate::feature::feature_value(&u.active_features, "変形")?;
        let mut toks = val.split_whitespace();
        let label = toks.next()?.to_string();
        let forms: Vec<String> = toks
            .filter(|f| self.database.unit_by_name(f).is_some())
            .map(|f| f.to_string())
            .collect();
        if forms.is_empty() {
            return None;
        }
        Some((label, forms))
    }

    /// 変形コマンドを開く。変形先が 1 つなら即変形、複数ならサブメニューを出す。
    pub(super) fn open_transform_menu(&mut self, uid: &str) {
        let info = self.database.unit_by_uid(uid).and_then(|u| {
            self.transform_forms(u)
                .map(|(_label, forms)| (u.pilot_name.clone(), forms))
        });
        let Some((nick, forms)) = info else {
            self.push_message("変形できる形態がありません".to_string());
            self.reopen_unit_menu_for(uid);
            return;
        };
        if forms.len() == 1 {
            // 変形先が 1 つ → 即変形。発動後もユニットメニューへ戻る（行動非消費）。
            let form = forms[0].clone();
            self.transform_unit_by_uid(uid, &form);
            self.reopen_unit_menu_for(uid);
            return;
        }
        // 複数形態はサブメニューで選択させる。
        self.pending_transform = Some(PendingTransform {
            uid: uid.to_string(),
            forms: forms.clone(),
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: format!("{nick} 変形先を選択"),
            options: forms,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            non_cancellable: false,
        });
    }

    /// 変形先サブメニューの選択を解決する。`choice` は 1-based。0 / 範囲外は
    /// キャンセルでユニットメニューへ戻る。
    /// 形態 (`form_name`) の必要技能 / 不必要技能 (`UnitData` の `必要技能=` / `不必要技能=`
    /// 特殊能力、`必要技能.md` §4) を `unit_idx` の搭乗員が満たすか。変形 / 換装 のゲート。
    /// 形態側にこれらの宣言が無ければ常に許可 (= 制限なし)。`必要技能` は満たさないと不可、
    /// `不必要技能` は満たすと不可 (原典: ちょうど逆)。乗り換え / 合体 のクロス搭乗評価は別途。
    pub(super) fn form_skill_ok(&self, unit_idx: usize, form_name: &str) -> bool {
        let Some(ud) = self.database.unit_by_name(form_name) else {
            return true;
        };
        let reqs: Vec<(bool, String)> = ud
            .features
            .iter()
            .filter_map(|(n, v)| match n.as_str() {
                "必要技能" => Some((true, v.clone())),
                "不必要技能" => Some((false, v.clone())),
                _ => None,
            })
            .collect();
        if reqs.is_empty() {
            return true;
        }
        let u = &self.database.unit_instances[unit_idx];
        for (require, expr) in &reqs {
            let satisfied = crate::necessary_skill::is_satisfied(expr, u, &self.database);
            if *require != satisfied {
                return false;
            }
        }
        true
    }

    pub(super) fn resolve_transform(&mut self, choice: u32) -> bool {
        let Some(pt) = self.pending_transform.take() else {
            return false;
        };
        self.pending_dialog = None;
        let idx = choice as usize;
        if idx == 0 || idx > pt.forms.len() {
            self.reopen_unit_menu_for(&pt.uid);
            return true;
        }
        let form = pt.forms[idx - 1].clone();
        // 形態の必要技能 (搭乗員が満たさないと変形不可)。
        if let Some(uidx) = self.database.idx_by_uid(&pt.uid) {
            if !self.form_skill_ok(uidx, &form) {
                self.push_message(format!("必要技能を満たさないため {form} に変形できません"));
                self.reopen_unit_menu_for(&pt.uid);
                return true;
            }
        }
        self.transform_unit_by_uid(&pt.uid, &form);
        self.reopen_unit_menu_for(&pt.uid);
        true
    }

    /// ユニット `uid` の形態 (`unit_data_name`) を `new_form` に差し替え、新形態の
    /// `active_features` / `abilities` を反映する（イベント発火・メッセージ無し）。
    /// 変形 / 換装 の共通コア。形態が存在し差し替えたら `true`。
    pub(crate) fn set_unit_form(&mut self, uid: &str, new_form: &str) -> bool {
        let Some(idx) = self.database.idx_by_uid(uid) else {
            return false;
        };
        let (raw_feats, abils) = self
            .database
            .unit_by_name(new_form)
            .map(|ud| {
                let feats = ud.features.clone();
                let abils = ud
                    .abilities
                    .iter()
                    .map(|a| {
                        let mut ua =
                            crate::unit_ability::UnitAbility::new(a.name.clone(), a.effect.clone());
                        ua.stock_remaining = a.uses;
                        ua
                    })
                    .collect::<Vec<_>>();
                (feats, abils)
            })
            .unwrap_or_default();
        self.database.unit_instances[idx].unit_data_name = new_form.to_string();
        // §3 必要技能ゲート込みで新形態の特殊能力を有効化する。
        let feats = crate::necessary_skill::gated_active_features(
            &raw_feats,
            &self.database.unit_instances[idx],
            &self.database,
        );
        let u = &mut self.database.unit_instances[idx];
        u.active_features = feats;
        u.abilities = abils;
        true
    }

    /// ユニット `uid` を `new_form` へ変形させる (プレイヤーの変形コマンド)。
    /// `変形`/`Transform` イベントを発火する。行動は消費しない。形態の差し替え自体は
    /// `.eve Transform` と共通の `set_unit_form`。
    pub(super) fn transform_unit_by_uid(&mut self, uid: &str, new_form: &str) {
        if !self.set_unit_form(uid, new_form) {
            return;
        }
        let Some(idx) = self.database.idx_by_uid(uid) else {
            return;
        };
        let (pn, un, party) = {
            let u = &self.database.unit_instances[idx];
            (u.pilot_name.clone(), u.unit_data_name.clone(), u.party)
        };
        self.push_message(format!("{pn} 変形 → {new_form}"));
        crate::event_runtime::fire_unit_event_labels_public(
            self,
            &["変形", "Transform"],
            &pn,
            &un,
            party,
        );
    }

    /// `caster` の `idx` 番目のアビリティ静的データ (`UnitData.abilities`) を返す。
    pub(super) fn ability_at(
        &self,
        caster_uid: &str,
        idx: usize,
    ) -> Option<crate::data::unit::AbilityData> {
        let u = self.database.unit_by_uid(caster_uid)?;
        let d = self.database.unit_by_name(&u.unit_data_name)?;
        d.abilities.get(idx).cloned()
    }

    /// アビリティが使用可能か (回数 / EN / 気力)。`アビリティ.md` の × 判定。
    pub(super) fn ability_usable(&self, caster_uid: &str, idx: usize) -> bool {
        let Some(u) = self.database.unit_by_uid(caster_uid) else {
            return false;
        };
        let Some(ab) = self.ability_at(caster_uid, idx) else {
            return false;
        };
        // 回数切れ (stock_remaining=Some(0))。無制限 (None) は常に可。
        if let Some(stock) = u.abilities.get(idx).and_then(|ua| ua.stock_remaining) {
            if stock <= 0 {
                return false;
            }
        }
        // EN 不足。
        if let Some(en) = ab.en_cost {
            let cur_en = self.database.effective_max_en(u) - u.en_consumed;
            if cur_en < en {
                return false;
            }
        }
        // 気力不足。
        if let Some(m) = ab.morale {
            if u.morale < m {
                return false;
            }
        }
        // 沈黙 (特殊効果攻撃属性 黙): 術 / 音 属性のアビリティは使用不能。
        if u.has_condition("沈黙") && (ab.attributes.contains('術') || ab.attributes.contains('音'))
        {
            return false;
        }
        // 必要技能 / 必要条件 (属性末尾の括弧条件)。武器同様、満たさないと使用不可。
        let (_class, ns, nc) = crate::necessary_skill::split_necessary(&ab.attributes);
        if !ns.is_empty() && !crate::necessary_skill::is_satisfied(&ns, u, &self.database) {
            return false;
        }
        if !nc.is_empty() && !crate::necessary_skill::is_satisfied(&nc, u, &self.database) {
            return false;
        }
        true
    }

    /// アビリティ一覧サブメニューを開く。使用不可なものは先頭に `×` を付ける。
    pub(super) fn open_ability_menu(&mut self, uid: &str) {
        let info: Option<(String, Vec<(usize, String)>)> =
            self.database.unit_by_uid(uid).and_then(|u| {
                let d = self.database.unit_by_name(&u.unit_data_name)?;
                if d.abilities.is_empty() {
                    return None;
                }
                let list = d
                    .abilities
                    .iter()
                    .enumerate()
                    .map(|(i, a)| (i, a.display_name().to_string()))
                    .collect();
                Some((u.pilot_name.clone(), list))
            });
        let Some((nick, list)) = info else {
            self.push_message("アビリティがありません".to_string());
            self.reopen_unit_menu_for(uid);
            return;
        };
        let mut entries = Vec::new();
        let mut options = Vec::new();
        for (idx, name) in list {
            let usable = self.ability_usable(uid, idx);
            options.push(format!("{}{name}", if usable { "" } else { "×" }));
            entries.push((idx, usable));
        }
        self.pending_ability = Some(PendingAbility {
            uid: uid.to_string(),
            entries,
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: format!("{nick} アビリティ"),
            options,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            non_cancellable: false,
        });
    }

    /// アビリティ一覧サブメニューの選択を解決する。`choice` は 1-based。
    pub(super) fn resolve_ability(&mut self, choice: u32) -> bool {
        let Some(pa) = self.pending_ability.take() else {
            return false;
        };
        self.pending_dialog = None;
        let i = choice as usize;
        if i == 0 || i > pa.entries.len() {
            self.reopen_unit_menu_for(&pa.uid);
            return true;
        }
        let (ability_idx, usable) = pa.entries[i - 1];
        if !usable {
            self.push_message("そのアビリティは使用できません (回数 / EN / 気力)".to_string());
            self.reopen_unit_menu_for(&pa.uid);
            return true;
        }
        let Some(ab) = self.ability_at(&pa.uid, ability_idx) else {
            self.reopen_unit_menu_for(&pa.uid);
            return true;
        };
        if ab.is_map_type() {
            // マップ型 (Ｍ全/Ｍ投/…): 座標選択を省略し射程内の全有効対象へ即適用。
            let caster = pa.uid.clone();
            self.apply_ability(&caster, ability_idx, &caster);
            self.finish_ability(&caster);
        } else if ab.is_self_only() {
            // 射程0: 自分に即適用 (召喚も射程0)。
            let caster = pa.uid.clone();
            self.apply_ability(&caster, ability_idx, &caster);
            self.finish_ability(&caster);
        } else {
            self.begin_ability_target(&pa.uid, ability_idx);
        }
        true
    }

    /// 射程≥1 アビリティの対象選択モードを開始する。
    pub(super) fn begin_ability_target(&mut self, caster: &str, ability_idx: usize) {
        use crate::command_menu::ActionMode;
        self.pending_dialog = None;
        self.command_menu = None;
        self.action_mode = ActionMode::AbilityTarget {
            caster: caster.to_string(),
            ability_idx,
        };
        let name = self
            .ability_at(caster, ability_idx)
            .map(|a| a.display_name().to_string())
            .unwrap_or_default();
        self.push_message(format!(
            "アビリティ【{name}】対象を選択 (右クリックでキャンセル)"
        ));
    }

    /// アビリティの対象が有効か (射程内・盤上、対象勢力、`援`/サイズ制限)。
    /// 対象勢力は属性で決まる: `脱`/`除` (敵対象) は敵、それ以外は味方。
    pub(super) fn ability_target_valid(&self, caster: &str, idx: usize, target: &str) -> bool {
        let Some(ab) = self.ability_at(caster, idx) else {
            return false;
        };
        let (Some(c), Some(t)) = (
            self.database.unit_by_uid(caster),
            self.database.unit_by_uid(target),
        ) else {
            return false;
        };
        if t.off_map {
            return false;
        }
        if ab.attributes.contains('援') && target == caster {
            return false; // 援: 自分に使用不可
        }
        // 対象勢力: 脱/除 アビリティは敵、それ以外は味方。
        if ab.targets_enemy() {
            if !c.party.is_hostile_to(t.party) {
                return false;
            }
        } else if !c.party.is_ally_of(t.party) {
            return false;
        }
        // 能力コピー: 自分以外の味方が対象。サイズ制限 (既定で 2 段階以上差は不可、
        // 追加設定 サイズ制限無し/強 で変化) を満たす相手のみ。
        if ab.has_copy_effect() {
            if target == caster {
                return false;
            }
            if !self.copy_size_ok(c, t, &ab) {
                return false;
            }
        }
        combat::manhattan((c.x, c.y), (t.x, t.y)) <= ab.range.max(0) as u32
    }

    /// `能力コピー` のサイズ制限判定。既定は 2 段階以上のサイズ差を禁止
    /// (`アビリティ効果.md`)。`サイズ制限無し` で無制限、`サイズ制限強` で同サイズのみ。
    pub(super) fn copy_size_ok(
        &self,
        caster: &crate::UnitInstance,
        target: &crate::UnitInstance,
        ab: &crate::data::unit::AbilityData,
    ) -> bool {
        if ab.effect.contains("サイズ制限無し") {
            return true;
        }
        let (Some(cd), Some(td)) = (
            self.database.unit_by_name(&caster.unit_data_name),
            self.database.unit_by_name(&target.unit_data_name),
        ) else {
            return true;
        };
        if ab.effect.contains("サイズ制限強") {
            cd.size == td.size
        } else {
            cd.size.step_diff(td.size) < 2
        }
    }

    /// アビリティを発動: 回数 / EN を消費し効果を `target` に適用する。
    /// 行動消費 (has_acted) は適用前に true をセットする (`再行動` 効果が解除しうる)。
    pub(super) fn apply_ability(&mut self, caster: &str, ability_idx: usize, target: &str) {
        let Some(ab) = self.ability_at(caster, ability_idx) else {
            return;
        };
        // 回数を消費 (有限のみ)。
        if let Some(ci) = self.database.idx_by_uid(caster) {
            if let Some(ua) = self.database.unit_instances[ci]
                .abilities
                .get_mut(ability_idx)
            {
                if let Some(s) = ua.stock_remaining.as_mut() {
                    *s = (*s - 1).max(0);
                }
            }
        }
        // EN を消費。
        if let Some(en) = ab.en_cost {
            if let Some(u) = self.database.unit_by_uid_mut(caster) {
                u.en_consumed += en;
            }
        }
        // 行動消費を先に確定 (`再行動` 効果が後で解除しうる)。
        if let Some(u) = self.database.unit_by_uid_mut(caster) {
            u.has_acted = true;
        }
        if ab.is_map_type() {
            // マップ型: 射程内の全有効対象へ効果と属性を適用する。
            self.apply_ability_area(caster, &ab);
        } else {
            self.apply_ability_effects(caster, target, &ab.effect);
            self.apply_ability_attributes(target, &ab);
            let tnick = self
                .database
                .unit_by_uid(target)
                .map(|u| u.pilot_name.clone())
                .unwrap_or_default();
            self.push_message(format!("アビリティ【{}】→ {tnick}", ab.display_name()));
        }
    }

    /// マップ型アビリティの効果を、射程内 (`Ｍ全` は盤上全体) の全有効対象へ
    /// 適用する。対象勢力は属性で決まる (脱/除=敵、それ以外=味方)。
    pub(super) fn apply_ability_area(&mut self, caster: &str, ab: &crate::data::unit::AbilityData) {
        let Some(c) = self.database.unit_by_uid(caster) else {
            return;
        };
        let (cx, cy, cparty) = (c.x, c.y, c.party);
        let range = ab.range.max(0) as u32;
        let targets: Vec<String> = self
            .database
            .unit_instances
            .iter()
            .filter(|u| !u.off_map)
            .filter(|u| {
                if ab.targets_enemy() {
                    cparty.is_hostile_to(u.party)
                } else {
                    cparty.is_ally_of(u.party)
                }
            })
            // 援: 自分自身は対象外。
            .filter(|u| !(ab.attributes.contains('援') && u.uid == caster))
            // Ｍ全 は射程・座標を無視。それ以外は発動者からの射程内。
            .filter(|u| ab.is_map_all() || combat::manhattan((cx, cy), (u.x, u.y)) <= range)
            .map(|u| u.uid.clone())
            .collect();
        for t in &targets {
            self.apply_ability_effects(caster, t, &ab.effect);
            self.apply_ability_attributes(t, ab);
        }
        self.push_message(format!(
            "アビリティ【{}】→ {} 体",
            ab.display_name(),
            targets.len()
        ));
    }

    /// アビリティ属性 (`脱`=気力低下 / `除`=特殊効果解除) を対象へ適用する。
    /// いずれも発動確率 100% (`ユニットデータ.md` アビリティ属性)。
    pub(super) fn apply_ability_attributes(
        &mut self,
        target: &str,
        ab: &crate::data::unit::AbilityData,
    ) {
        if ab.attributes.contains('脱') {
            self.add_unit_morale(target, -10);
        }
        if ab.attributes.contains('除') {
            // 相手にかかっているアビリティ由来の特殊効果を解除。MVP: 状態異常を
            // 全消去 (アビリティ付与分とそれ以外を区別しない簡易版)。
            if let Some(u) = self.database.unit_by_uid_mut(target) {
                u.conditions.clear();
            }
        }
    }

    /// アビリティ発動後の後処理: 行動が残っていればメニュー再表示、消費済みなら
    /// 行動終了イベントを発火してメニューを閉じる。
    pub(super) fn finish_ability(&mut self, caster: &str) {
        use crate::command_menu::ActionMode;
        self.action_mode = ActionMode::Browse;
        let acted = self
            .database
            .unit_by_uid(caster)
            .map(|u| u.has_acted)
            .unwrap_or(false);
        if acted {
            if let Some(i) = self.database.idx_by_uid(caster) {
                crate::event_runtime::fire_action_end_labels(self, i);
                crate::event_runtime::fire_contact_event_labels(self, i);
            }
            self.command_menu = None;
        } else {
            // 再行動等で行動が戻った → 連続使用できるようメニューを再表示。
            self.reopen_unit_menu_for(caster);
        }
    }

    /// アビリティ効果文字列 (`回復Lv2 治癒` 等、半角スペース区切り) を適用する。
    /// `アビリティ効果.md` の主要効果に対応。未対応効果は無視 (無害)。
    /// `caster` は発動者 (`能力コピー` で発動者自身が変化するため必要)。
    pub(super) fn apply_ability_effects(&mut self, caster: &str, target: &str, effect: &str) {
        // ゾンビ状態の対象は能動的な HP/EN 回復 (回復/補給) を受けられない。
        let recovery_blocked = self.recovery_blocked(target);
        for tok in effect.split_whitespace() {
            let (head, arg) = match tok.split_once('=') {
                Some((h, a)) => (h, Some(a.trim_matches('"'))),
                None => (tok, None),
            };
            let (base, lv) = split_effect_level(head);
            match base {
                // 回復: HP 500×Lv。ゾンビ対象は回復不可。
                "回復" => {
                    if let Some(ci) = self
                        .database
                        .idx_by_uid(target)
                        .filter(|_| !recovery_blocked)
                    {
                        let heal = i64::from((500 * lv).max(0));
                        let u = &mut self.database.unit_instances[ci];
                        u.damage = (u.damage - heal).max(0);
                    }
                }
                // 補給: EN 50×Lv。ゾンビ対象は回復不可。
                "補給" => {
                    if let Some(ci) = self
                        .database
                        .idx_by_uid(target)
                        .filter(|_| !recovery_blocked)
                    {
                        let u = &mut self.database.unit_instances[ci];
                        u.en_consumed = (u.en_consumed - (50 * lv).max(0)).max(0);
                    }
                }
                // 気力増加: 10×Lv。
                "気力増加" => self.add_unit_morale(target, 10 * lv),
                // 治癒: 状態異常回復 (引数指定なら該当のみ)。
                "治癒" => {
                    if let Some(u) = self.database.unit_by_uid_mut(target) {
                        match arg {
                            Some(names) => {
                                for n in names.split_whitespace() {
                                    u.remove_condition(n);
                                }
                            }
                            None => u.conditions.clear(),
                        }
                    }
                }
                // 装填: 残弾を最大まで回復 (EN は戻さない)。
                "装填" => self.ability_reload(target),
                // 再行動: 行動済みを解除。
                "再行動" => self.reset_unit_action(target),
                // 霊力回復 / ＳＰ回復: メインパイロットの SP を 10×Lv 回復。
                "霊力回復" | "ＳＰ回復" => self.restore_unit_sp(target, 10 * lv),
                // 変身: 対象を別フォームへ (set_unit_form を変形/換装と共有)。
                // 注: 持続ターン (Lv) の自動解除は未モデル (永続変身扱い)。
                "変身" => {
                    if let Some(form) = arg {
                        self.set_unit_form(target, form);
                    }
                }
                // 状態 / 付加: 特殊状態・特殊能力を付与。Lv 指定で持続ターン、無指定で永続。
                "状態" | "付加" => {
                    if let Some(name) = arg {
                        let lifetime = if head.contains("Lv") { lv } else { -1 };
                        self.add_unit_condition(target, name, lifetime);
                    }
                }
                // 召喚: 指定ユニットを Lv 体、対象 (=発動者) の隣接空きマスへ生成する。
                "召喚" => {
                    if let Some(unit_name) = arg {
                        for _ in 0..lv.max(1) {
                            if !self.summon_unit_adjacent(target, unit_name) {
                                break; // 空きマスが尽きたら打ち切り
                            }
                        }
                    }
                }
                // 強化: メインパイロットの特殊能力レベルを一定時間増加。MVP では
                // 指定能力を一時的な状態 (condition) として対象へ付与する (付加と同じ
                // 機構で名前ベースに参照可能化。既存能力へのレベル加算は未モデル)。
                "強化" => {
                    if let Some(name) = arg {
                        let lifetime = if head.contains("Lv") { lv } else { -1 };
                        self.add_unit_condition(target, name, lifetime);
                    }
                }
                // 能力コピー: 発動者自身を対象 (射程内味方) のユニットへ変化させる。
                // 対象のユニットデータをコピー (サイズ制限は target 選択時に担保)。
                // パイロット能力は変化しない (set_unit_form は pilot を保持)。
                "能力コピー" => {
                    let form = self
                        .database
                        .unit_by_uid(target)
                        .map(|u| u.unit_data_name.clone());
                    if let Some(form) = form {
                        if caster != target {
                            self.set_unit_form(caster, &form);
                        }
                    }
                }
                // 解説: 効果なし (表示用)。
                "解説" => {}
                _ => {}
            }
        }
    }

    /// 装填アビリティ: 対象の全武器の残弾を最大値へ戻す (EN は変えない)。
    pub(super) fn ability_reload(&mut self, target: &str) {
        let Some(idx) = self.database.idx_by_uid(target) else {
            return;
        };
        let unit_name = self.database.unit_instances[idx].unit_data_name.clone();
        let max_bullets: Vec<i32> = self
            .database
            .unit_by_name(&unit_name)
            .map(|d| d.weapons.iter().map(|w| w.bullet).collect())
            .unwrap_or_default();
        let u = &mut self.database.unit_instances[idx];
        for w in &mut u.weapons {
            if let Some(b) = max_bullets.get(w.weapon_index) {
                w.bullet_remaining = *b;
            }
        }
    }

    /// 召喚アビリティ: `parent` の隣接空きマスに `unit_name` のユニットを生成する。
    /// 同陣営・無人で生成し、`summoned_by=parent` を記録 (親破壊で消滅させるため)。
    /// 空きマスが無い / ユニット定義が無いときは `false`。
    pub(super) fn summon_unit_adjacent(&mut self, parent_uid: &str, unit_name: &str) -> bool {
        if self.database.unit_by_name(unit_name).is_none() {
            return false;
        }
        let Some((ppos, party)) = self
            .database
            .unit_by_uid(parent_uid)
            .map(|u| ((u.x, u.y), u.party))
        else {
            return false;
        };
        let Some((tx, ty)) = self.find_empty_adjacent_tile(ppos) else {
            return false;
        };
        let mut inst = crate::UnitInstance::new(unit_name, String::new(), party, tx, ty);
        inst.summoned_by = Some(parent_uid.to_string());
        let uid = self.database.register_unit(inst);
        // 召喚ユニットの active_features / abilities を UnitData から初期化。
        self.set_unit_form(&uid, unit_name);
        self.push_message(format!("{unit_name} を召喚！"));
        true
    }

    /// 発進サブメニューを開く。母艦が格納しているユニットを一覧表示する。
    pub(super) fn open_launch_menu(&mut self, carrier_uid: &str) {
        let stored: Vec<String> = self
            .database
            .unit_by_uid(carrier_uid)
            .map(|u| u.stored_units.clone())
            .unwrap_or_default();
        if stored.is_empty() {
            self.push_message("格納しているユニットがありません".to_string());
            self.reopen_unit_menu_for(carrier_uid);
            return;
        }
        let options: Vec<String> = stored
            .iter()
            .map(|uid| {
                self.database
                    .unit_by_uid(uid)
                    .map(|u| {
                        let unit = self.unit_display_name(u);
                        let pilot = if u.pilot_name.is_empty() {
                            "無人".to_string()
                        } else {
                            u.pilot_name.clone()
                        };
                        format!("{unit} ({pilot})")
                    })
                    .unwrap_or_else(|| uid.clone())
            })
            .collect();
        self.pending_launch = Some(PendingLaunch {
            carrier: carrier_uid.to_string(),
            stored,
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: "発進させるユニットを選択".to_string(),
            options,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            non_cancellable: false,
        });
    }

    /// 発進サブメニューの選択を解決する。`choice` は 1-based。発進は行動を消費しない。
    pub(super) fn resolve_launch(&mut self, choice: u32) -> bool {
        let Some(pl) = self.pending_launch.take() else {
            return false;
        };
        self.pending_dialog = None;
        let i = choice as usize;
        if i == 0 || i > pl.stored.len() {
            self.reopen_unit_menu_for(&pl.carrier);
            return true;
        }
        let stored = pl.stored[i - 1].clone();
        // 着地点選択 (原典): 母艦位置を起点とする移動範囲の空きマスから対話選択する。
        // 候補があればクリック選択へ移行し、無ければ従来の隣接自動配置にフォールバック。
        if let Some(cpos) = self.database.unit_by_uid(&pl.carrier).map(|u| (u.x, u.y)) {
            let candidates = self.landing_candidates(&stored, cpos);
            if !candidates.is_empty() {
                self.map_cursor = Some(cpos);
                self.action_mode = crate::command_menu::ActionMode::LandingSelect {
                    uid: stored,
                    candidates,
                    carrier: Some(pl.carrier.clone()),
                };
                return true;
            }
        }
        if self.launch_unit_from_carrier(&pl.carrier, &stored) {
            let nick = self
                .database
                .unit_by_uid(&stored)
                .map(|u| u.pilot_name.clone())
                .unwrap_or_default();
            self.push_message(format!("{nick} 発進！"));
        }
        // 発進は母艦の行動を消費しないのでメニューを再表示。
        self.reopen_unit_menu_for(&pl.carrier);
        true
    }

    /// 着地点選択 (`LandingSelect`) で候補マス `(tx, ty)` を確定する。ユニットを配置し、
    /// 母艦経路なら母艦メニューを再表示する。候補外なら何もしない (`false`)。
    pub(super) fn confirm_landing(&mut self, tx: u32, ty: u32) -> bool {
        let crate::command_menu::ActionMode::LandingSelect {
            uid,
            candidates,
            carrier,
        } = self.action_mode.clone()
        else {
            return false;
        };
        if !candidates.contains(&(tx, ty)) {
            return false; // 候補外クリックは無反応 (キャンセルは右クリック)。
        }
        if let Some(carrier) = carrier {
            self.place_launched_unit_at(&carrier, &uid, tx, ty);
            let nick = self
                .database
                .unit_by_uid(&uid)
                .map(|u| u.pilot_name.clone())
                .unwrap_or_default();
            self.push_message(format!("{nick} 発進！"));
            self.action_mode = crate::command_menu::ActionMode::Browse;
            self.reopen_unit_menu_for(&carrier);
        } else {
            // 分離経路 (carrier なし): 構成ユニットを盤上へ配置し、キューの次へ進む
            // (残りが無ければ Browse へ戻し host メニューを再表示)。
            self.place_separated_unit_at(&uid, tx, ty);
            self.advance_landing_queue();
        }
        true
    }

    /// 母艦 `carrier` から格納ユニット `stored` を出撃させる。母艦の隣接空きマスに
    /// 配置し、格納リンクを解除する。空きマスが無ければ `false`。
    /// (スクリプト/AI 経路・着地点選択候補が無いときのフォールバック。プレイヤーの
    /// メニュー経路は `resolve_launch` が移動範囲からの着地点選択 (`LandingSelect`) を挟む。)
    pub(super) fn launch_unit_from_carrier(&mut self, carrier: &str, stored: &str) -> bool {
        let Some(cpos) = self.database.unit_by_uid(carrier).map(|u| (u.x, u.y)) else {
            return false;
        };
        let Some((tx, ty)) = self.find_empty_adjacent_tile(cpos) else {
            self.push_message("発進できる空きマスがありません".to_string());
            return false;
        };
        self.place_launched_unit_at(carrier, stored, tx, ty);
        true
    }

    /// 格納ユニット `stored` を `(tx, ty)` へ配置し、格納リンクを解除する。
    /// `launch_unit_from_carrier` (隣接自動) と `LandingSelect` (移動範囲から選択) の共通配置。
    pub(super) fn place_launched_unit_at(&mut self, carrier: &str, stored: &str, tx: u32, ty: u32) {
        // off_map ユニットは move_unit で座標だけ更新し、set_off_map(false) で
        // pos_index に載せる (`.eve Launch` と同順)。
        self.database.move_unit(stored, tx, ty);
        self.database.set_off_map(stored, false);
        if let Some(u) = self.database.unit_by_uid_mut(stored) {
            u.life_state = String::new();
            u.stored_in = None;
            u.has_acted = false;
            u.has_moved = false;
        }
        if let Some(c) = self.database.unit_by_uid_mut(carrier) {
            c.stored_units.retain(|s| s != stored);
        }
    }

    /// 発進/分離ユニット `uid` が `source` (母艦/合体元の位置) から着地可能なマス一覧。
    /// ユニットの移動範囲内 (Dijkstra) かつ空き (他ユニット不在) のマス。`source` が母艦で
    /// 占有されていても起点としては有効 (そこから移動して空きマスへ着地する)。
    pub(super) fn landing_candidates(&self, uid: &str, source: (u32, u32)) -> Vec<(u32, u32)> {
        self.database
            .unit_move_range_from(uid, source)
            .into_keys()
            .filter(|&(x, y)| self.database.uid_at(x, y).is_none())
            .collect()
    }

    /// 分離の構成ユニット `uid` を `(tx, ty)` へ盤上配置する (off_map 解除 + 状態リセット)。
    pub(super) fn place_separated_unit_at(&mut self, uid: &str, tx: u32, ty: u32) {
        self.database.move_unit(uid, tx, ty);
        self.database.set_off_map(uid, false);
        if let Some(u) = self.database.unit_by_uid_mut(uid) {
            u.life_state = String::new();
            u.has_acted = false;
            u.has_moved = false;
        }
    }

    /// 分離の構成ユニット列 `units` を `source` (host 位置) 起点で順次着地選択させる。
    /// 最初に着地候補のあるユニットで `LandingSelect` へ移行し残りを `landing_queue` へ積む。
    /// 候補の無いユニットは隣接自動配置でスキップ。全機 auto-place / 候補皆無なら `false`
    /// (対話に入らない＝呼出側が従来どおりメニュー再表示する)。
    pub(super) fn begin_landing_queue(&mut self, units: Vec<String>, source: (u32, u32)) -> bool {
        self.landing_source = Some(source);
        let mut remaining: std::collections::VecDeque<String> = units.into();
        while let Some(uid) = remaining.pop_front() {
            let candidates = self.landing_candidates(&uid, source);
            if candidates.is_empty() {
                if let Some((tx, ty)) = self.find_empty_adjacent_tile(source) {
                    self.place_separated_unit_at(&uid, tx, ty);
                }
                continue;
            }
            self.landing_queue = remaining.into_iter().collect();
            self.action_mode = crate::command_menu::ActionMode::LandingSelect {
                uid,
                candidates,
                carrier: None,
            };
            return true;
        }
        self.landing_source = None;
        false
    }

    /// 分離キューの次のユニットへ進む。候補があれば `LandingSelect`、無ければ隣接自動で
    /// スキップ。キューが尽きたら `finish_landing_queue` で Browse へ戻し host メニューを再表示。
    pub(super) fn advance_landing_queue(&mut self) {
        let source = self.landing_source.unwrap_or((0, 0));
        while !self.landing_queue.is_empty() {
            let uid = self.landing_queue.remove(0);
            let candidates = self.landing_candidates(&uid, source);
            if candidates.is_empty() {
                if let Some((tx, ty)) = self.find_empty_adjacent_tile(source) {
                    self.place_separated_unit_at(&uid, tx, ty);
                }
                continue;
            }
            self.action_mode = crate::command_menu::ActionMode::LandingSelect {
                uid,
                candidates,
                carrier: None,
            };
            return;
        }
        self.finish_landing_queue();
    }

    /// 分離キュー完了: Browse へ戻し、host のユニットメニューを再表示する。
    pub(super) fn finish_landing_queue(&mut self) {
        self.action_mode = crate::command_menu::ActionMode::Browse;
        self.landing_source = None;
        if let Some(host) = self.landing_menu_uid.take() {
            self.reopen_unit_menu_for(&host);
        }
    }

    /// `pos` の 8 近傍からユニットの居ない盤内マスを 1 つ返す。発進の着地点用。
    pub(super) fn find_empty_adjacent_tile(&self, pos: (u32, u32)) -> Option<(u32, u32)> {
        let (w, h) = match self.database.map.as_ref() {
            Some(m) => (m.width, m.height),
            None => return None,
        };
        const DELTAS: [(i32, i32); 8] = [
            (0, -1),
            (0, 1),
            (-1, 0),
            (1, 0),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ];
        for (dx, dy) in DELTAS {
            let nx = pos.0 as i32 + dx;
            let ny = pos.1 as i32 + dy;
            if nx < 0 || ny < 0 || nx as u32 >= w || ny as u32 >= h {
                continue;
            }
            let (tx, ty) = (nx as u32, ny as u32);
            if self.database.uid_at(tx, ty).is_none() {
                return Some((tx, ty));
            }
        }
        None
    }

    /// `host` の `合体` 特殊能力 (`合体=<名称> <合体形態> <相手1> <相手2> …`) を解析し、
    /// 合体可能なら (合体形態, 合体相手 uid 列) を返す。原典 (合体.md): 合体に参加する
    /// ユニット全てが 2 マス以内に居る場合にのみ合体可能なので、列挙された各パートナー
    /// 形態がそれぞれ 2 マス以内に揃っていなければ `None` を返す。
    pub(super) fn combine_partners(&self, host_uid: &str) -> Option<(String, Vec<String>)> {
        let host = self.database.unit_by_uid(host_uid)?;
        let val = crate::feature::feature_value(&host.active_features, "合体")?;
        let toks: Vec<&str> = val.split_whitespace().collect();
        if toks.len() < 3 {
            return None; // 名称 + 合体形態 + 相手 が最低限必要
        }
        let combined_form = toks[1].to_string();
        // 合体形態が DB に存在しなければ不可。
        self.database.unit_by_name(&combined_form)?;
        let hpos = (host.x, host.y);
        let hparty = host.party;
        // 列挙されたパートナー形態ごとに、2 マス以内・同陣営・盤上の味方を 1 機ずつ探す。
        // 1 形態でも揃わなければ合体不可。
        let mut partners: Vec<String> = Vec::new();
        for pform in &toks[2..] {
            let found = self.database.unit_instances.iter().find(|u| {
                !u.off_map
                    && u.uid != host_uid
                    && u.party == hparty
                    && u.unit_data_name.as_str() == *pform
                    && !partners.contains(&u.uid)
                    && combat::manhattan(hpos, (u.x, u.y)) <= 2
            });
            partners.push(found?.uid.clone());
        }
        Some((combined_form, partners))
    }

    /// `host` の `合体` 特殊能力に列挙されたパートナー形態数を返す (合体能力が無ければ 0)。
    /// 1 = 2 体合体 (相手ユニット上への移動で合体)、2 以上 = 3 体以上合体 (合体コマンドで合体)。
    pub(super) fn combine_partner_count(&self, host_uid: &str) -> usize {
        self.database
            .unit_by_uid(host_uid)
            .and_then(|h| crate::feature::feature_value(&h.active_features, "合体"))
            .map(|v| v.split_whitespace().skip(2).count())
            .unwrap_or(0)
    }

    /// 合体を実行: 2 マス以内の合体相手を温存 (off_map) して合体形態へ変身する。
    /// 発動で行動終了 (原典準拠)。構成ユニットは `分離` で復帰できる。
    pub(super) fn apply_combine(&mut self, host_uid: &str) {
        let Some((combined_form, partners)) = self.combine_partners(host_uid) else {
            return;
        };
        // 合体形態の必要技能 (`必要技能.md` §4) を構成全員のパイロットで判定する。満たさなければ
        // 合体しない。判定は merged パイロットを host へ一時設定して評価し、即座に復元する
        // (以降の本処理が merged を再計算するため、ここでは破壊しない非破壊チェック)。
        if let Some(host_idx) = self.database.idx_by_uid(host_uid) {
            let mut merged = self.database.unit_instances[host_idx].pilot_ids.clone();
            for p in &partners {
                if let Some(pu) = self.database.unit_by_uid(p) {
                    for pid in &pu.pilot_ids {
                        if !merged.contains(pid) {
                            merged.push(pid.clone());
                        }
                    }
                }
            }
            let saved = std::mem::replace(
                &mut self.database.unit_instances[host_idx].pilot_ids,
                merged,
            );
            let ok = self.form_skill_ok(host_idx, &combined_form);
            self.database.unit_instances[host_idx].pilot_ids = saved;
            if !ok {
                self.push_message(format!(
                    "必要技能を満たさないため {combined_form} に合体できません"
                ));
                return;
            }
        }
        // 構成ユニット (相手) を温存: off_map + life_state="合体"。
        for p in &partners {
            self.database.set_off_map(p, true);
            if let Some(u) = self.database.unit_by_uid_mut(p) {
                u.life_state = "合体".to_string();
            }
        }
        // host の合体前形態を記録し、合体形態へ変身。
        let pre_form = self
            .database
            .unit_by_uid(host_uid)
            .map(|u| u.unit_data_name.clone());
        // パイロット統合 (全員搭乗): host の元搭乗者を温存しつつ、構成ユニットの
        // パイロットを合体形態へ集約する。構成ユニット側の pilot_ids は温存され
        // (off_map のまま)、分離時に各自へ戻る。
        let host_pilots = self
            .database
            .unit_by_uid(host_uid)
            .map(|u| u.pilot_ids.clone())
            .unwrap_or_default();
        let mut merged_pilots = host_pilots.clone();
        for p in &partners {
            if let Some(pu) = self.database.unit_by_uid(p) {
                for pid in &pu.pilot_ids {
                    if !merged_pilots.contains(pid) {
                        merged_pilots.push(pid.clone());
                    }
                }
            }
        }
        self.set_unit_form(host_uid, &combined_form);
        if let Some(u) = self.database.unit_by_uid_mut(host_uid) {
            u.pre_combine_form = pre_form;
            u.pre_combine_pilots = host_pilots;
            u.pilot_ids = merged_pilots;
            u.combined_from = partners.clone();
            u.has_acted = true;
        }
        self.push_message(format!(
            "合体！ → {combined_form} (構成 {} 機)",
            partners.len() + 1
        ));
        if let Some(i) = self.database.idx_by_uid(host_uid) {
            let (pn, un, party) = {
                let u = &self.database.unit_instances[i];
                (u.pilot_name.clone(), u.unit_data_name.clone(), u.party)
            };
            crate::event_runtime::fire_unit_event_labels_public(
                self,
                &["合体", "Combine"],
                &pn,
                &un,
                party,
            );
            if let Some(i2) = self.database.idx_by_uid(host_uid) {
                crate::event_runtime::fire_action_end_labels(self, i2);
            }
        }
        self.action_mode = crate::command_menu::ActionMode::Browse;
        self.command_menu = None;
    }

    /// 分離を実行する。行動は消費しない (原典準拠)。2 つの経路がある:
    /// - **実行時合体** (`combined_from` 非空): 温存していた構成ユニットを隣接へ復帰し、
    ///   host を合体前形態へ戻す。
    /// - **最初から合体形態** (`combined_from` 空で `分離` 特殊能力あり): `分離` 特殊能力の
    ///   形態へ分離し、形態 2 以降を新規生成する。
    pub(super) fn apply_separate(&mut self, host_uid: &str) {
        let components = self
            .database
            .unit_by_uid(host_uid)
            .map(|u| u.combined_from.clone())
            .unwrap_or_default();
        let split_forms: Vec<String> = self
            .database
            .unit_by_uid(host_uid)
            .and_then(|u| crate::feature::feature_value(&u.active_features, "分離"))
            // `分離=<名称> <形態1> <形態2> …` の先頭 (名称) を除いた形態列。
            .map(|v| {
                v.split_whitespace()
                    .skip(1)
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let staged = if !components.is_empty() {
            self.separate_preserved_components(host_uid, &components)
        } else if !split_forms.is_empty() {
            self.separate_into_forms(host_uid, &split_forms)
        } else {
            return; // 分離できない
        };

        self.push_message("分離！".to_string());
        if let Some(i) = self.database.idx_by_uid(host_uid) {
            let (pn, un, party) = {
                let u = &self.database.unit_instances[i];
                (u.pilot_name.clone(), u.unit_data_name.clone(), u.party)
            };
            crate::event_runtime::fire_unit_event_labels_public(
                self,
                &["分離", "Split"],
                &pn,
                &un,
                party,
            );
        }
        // 構成ユニットの着地点を移動範囲から対話選択させる (host 位置を起点)。
        // 候補があればキュー化し、完了後に host メニューを再表示。候補が無い (狭所/map 無し)
        // ときは begin_landing_queue 内で隣接自動配置し false を返すので従来どおりメニュー再表示。
        if let Some(hpos) = self.database.unit_by_uid(host_uid).map(|u| (u.x, u.y)) {
            self.landing_menu_uid = Some(host_uid.to_string());
            if self.begin_landing_queue(staged, hpos) {
                return; // 対話配置中 (完了で finish_landing_queue がメニュー再表示)。
            }
        }
        self.landing_menu_uid = None;
        // 分離は行動を消費しないのでメニューを再表示。
        self.reopen_unit_menu_for(host_uid);
    }

    /// 実行時合体の分離: 温存していた構成ユニット (off_map) の着地点を呼出側が選択させる。
    /// host を合体前形態・合体前の搭乗構成へ戻し、構成 uid 列を返す (placement は
    /// `apply_separate` が移動範囲から対話選択させる。候補無し時は隣接自動)。
    pub(super) fn separate_preserved_components(
        &mut self,
        host_uid: &str,
        components: &[String],
    ) -> Vec<String> {
        // 構成ユニットは合体中ずっと off_map で温存されているため、ここでは盤上配置せず
        // uid 列だけ返す (着地は landing queue が担う)。
        let staged: Vec<String> = components.to_vec();
        // host を合体前形態へ戻す。
        let pre = self
            .database
            .unit_by_uid(host_uid)
            .and_then(|u| u.pre_combine_form.clone());
        if let Some(form) = pre {
            self.set_unit_form(host_uid, &form);
        }
        // host の搭乗者を合体前の構成へ戻す (構成ユニットのパイロットは各機へ復帰)。
        let pre_pilots = self
            .database
            .unit_by_uid(host_uid)
            .map(|u| u.pre_combine_pilots.clone())
            .unwrap_or_default();
        if let Some(u) = self.database.unit_by_uid_mut(host_uid) {
            u.pilot_ids = pre_pilots;
            u.combined_from.clear();
            u.pre_combine_form = None;
            u.pre_combine_pilots.clear();
        }
        staged
    }

    /// 最初から合体形態で配置されたユニットの分離: `分離` 特殊能力
    /// (`分離=<名称> <形態1> <形態2> …`) の形態へ分離する。host は形態 1 になり、
    /// 形態 2 以降は隣接空きマスへ新規生成する。原典 (分離.md / 変形系特殊能力.md):
    /// パイロットは形態の順に乗り移り、サポートパイロット (形態数を超える分) は全て
    /// 1 号機 (形態 1) に乗せられる。MVP: 形態順の主パイロット割当のみ、複数形態への
    /// 主/副の細かい配分は形態 1 へ集約する単純化。
    pub(super) fn separate_into_forms(&mut self, host_uid: &str, forms: &[String]) -> Vec<String> {
        let Some((hpos, party, pilots)) = self
            .database
            .unit_by_uid(host_uid)
            .map(|u| ((u.x, u.y), u.party, u.pilot_ids.clone()))
        else {
            return Vec::new();
        };
        // 形態 1 = host。形態 2 以降を host 位置に off_map で生成し (staging)、着地点は
        // 呼出側 (`apply_separate`) が移動範囲から対話選択させる (候補無し時は隣接自動)。
        // 形態 i (0 基点) の主パイロット = pilots[i] (あれば)。形態数を超えるパイロットは
        // サポートとして形態 1 (host) に集約する。
        let mut staged = Vec::new();
        for (i, form) in forms.iter().enumerate().skip(1) {
            // 生成先の形態が DB に無ければスキップ。
            if self.database.unit_by_name(form).is_none() {
                continue;
            }
            let pilot_id = pilots.get(i).cloned();
            let pilot_name = pilot_id
                .as_deref()
                .map(|id| self.pilot_name_for_id(id))
                .unwrap_or_default();
            let mut inst = crate::UnitInstance::new(form, pilot_name, party, hpos.0, hpos.1);
            if let Some(pid) = pilot_id {
                inst.pilot_ids = vec![pid];
            }
            inst.off_map = true; // staging (pos_index に載せない)
            let uid = self.database.register_unit(inst);
            self.set_unit_form(&uid, form);
            staged.push(uid);
        }
        // host を形態 1 へ変身させ、形態 1 のパイロット (主 pilots[0] + サポート余り) を残す。
        self.set_unit_form(host_uid, &forms[0]);
        let mut host_pilots: Vec<String> = Vec::new();
        if let Some(p0) = pilots.first() {
            host_pilots.push(p0.clone());
        }
        for p in pilots.iter().skip(forms.len()) {
            host_pilots.push(p.clone());
        }
        if let Some(u) = self.database.unit_by_uid_mut(host_uid) {
            if !host_pilots.is_empty() {
                u.pilot_ids = host_pilots;
            }
        }
        staged
    }

    /// パイロット ID (PilotInstance.id または pilot_data_name) から表示用のパイロット
    /// データ名を解決する。見つからなければ ID をそのまま返す。
    pub(super) fn pilot_name_for_id(&self, pilot_id: &str) -> String {
        self.database
            .pilot_instances
            .iter()
            .find(|p| p.id == pilot_id || p.pilot_data_name == pilot_id)
            .map(|p| p.pilot_data_name.clone())
            .unwrap_or_else(|| pilot_id.to_string())
    }

    /// `host` の `合体` 特殊能力が **2 体合体** (相手 1 機) で、合体形態 (DB に存在) と
    /// 相手 `partner_form` を持つか。相手ユニット上への移動による合体の判定に使う。
    /// 原典 (合体.md): 3 体以上合体は合体コマンド専用で、移動では合体しない。
    pub(super) fn combine_lists_partner(&self, host_uid: &str, partner_form: &str) -> bool {
        let Some(host) = self.database.unit_by_uid(host_uid) else {
            return false;
        };
        let Some(val) = crate::feature::feature_value(&host.active_features, "合体") else {
            return false;
        };
        let toks: Vec<&str> = val.split_whitespace().collect();
        // 名称 + 合体形態 + 相手 1 機 = ちょうど 3 トークン (2 体合体) のみ。
        toks.len() == 3 && self.database.unit_by_name(toks[1]).is_some() && toks[2] == partner_form
    }

    /// `mover` が `target_pos` の隣接マスへ到達できるなら、その (空き) マスを返す。
    /// 既に隣接していれば現在位置を返す。搭載 / 合体の到達判定に使う。
    pub(super) fn reachable_adjacent_to(
        &self,
        mover_uid: &str,
        target_pos: (u32, u32),
    ) -> Option<(u32, u32)> {
        if let Some(mp) = self.database.unit_by_uid(mover_uid).map(|u| (u.x, u.y)) {
            if combat::manhattan(mp, target_pos) == 1 {
                return Some(mp);
            }
        }
        let range = self.database.unit_move_range(mover_uid);
        for (dx, dy) in [(0i32, -1), (0, 1), (-1, 0), (1, 0)] {
            let nx = target_pos.0 as i32 + dx;
            let ny = target_pos.1 as i32 + dy;
            if nx < 0 || ny < 0 {
                continue;
            }
            let adj = (nx as u32, ny as u32);
            if range.contains_key(&adj) {
                return Some(adj);
            }
        }
        None
    }

    /// 占有マス (味方の母艦 or 合体相手) へ移動しようとしたときの 搭載 / 合体 統合。
    /// `mover` が `occupant` に隣接到達でき、母艦搭載 or 合体が成立すれば実行して
    /// `true`。該当しなければ `false` (通常移動の占有判定に委ねる = 移動不可)。
    pub(super) fn try_board_or_combine_by_move(
        &mut self,
        mover_uid: &str,
        occupant_uid: &str,
    ) -> bool {
        let (Some(m), Some(o)) = (
            self.database.unit_by_uid(mover_uid),
            self.database.unit_by_uid(occupant_uid),
        ) else {
            return false;
        };
        if m.party != o.party || o.off_map {
            return false;
        }
        let opos = (o.x, o.y);
        let mover_form = m.unit_data_name.clone();
        let occ_form = o.unit_data_name.clone();
        let occ_is_carrier = crate::feature::has_feature(&o.active_features, "母艦");
        let mover_no_store = crate::feature::has_feature(&m.active_features, "格納不可");
        let combine_into_occ = self.combine_lists_partner(occupant_uid, &mover_form);
        let combine_into_mover = self.combine_lists_partner(mover_uid, &occ_form);
        if !occ_is_carrier && !combine_into_occ && !combine_into_mover {
            return false; // 母艦でも合体相手でもない
        }
        // 到達判定 (隣接マスへ行けるか)。
        let Some(adj) = self.reachable_adjacent_to(mover_uid, opos) else {
            return false;
        };
        if occ_is_carrier && !mover_no_store {
            // 搭載: mover は off_map になるので位置は問わない。
            let (Some(mi), Some(ci)) = (
                self.database.idx_by_uid(mover_uid),
                self.database.idx_by_uid(occupant_uid),
            ) else {
                return false;
            };
            self.fire_boarding_event(mi, ci);
            return true;
        }
        // 合体: mover を隣接マスへ動かしてから host を合体させる。
        if combine_into_occ {
            self.database.move_unit(mover_uid, adj.0, adj.1);
            self.apply_combine(occupant_uid);
            return true;
        }
        if combine_into_mover {
            self.database.move_unit(mover_uid, adj.0, adj.1);
            self.apply_combine(mover_uid);
            return true;
        }
        false
    }

    /// 精神コマンドの SP 消費を反映する。`PilotInstance` があれば `sp_remaining` を、
    /// 無ければ `UnitInstance.sp_consumed` を更新する (`SpecialPower` 命令と同方針)。
    pub(super) fn consume_unit_sp(&mut self, uid: &str, cost: i32) {
        let Some(unit_idx) = self.database.idx_by_uid(uid) else {
            return;
        };
        let pilot_id = {
            let u = &self.database.unit_instances[unit_idx];
            u.pilot_ids
                .first()
                .cloned()
                .unwrap_or_else(|| u.pilot_name.clone())
        };
        let pilot_idx = self
            .database
            .pilot_instances
            .iter()
            .position(|p| p.id == pilot_id || p.pilot_data_name == pilot_id);
        if let Some(pi) = pilot_idx {
            let p = &mut self.database.pilot_instances[pi];
            p.sp_remaining = (p.sp_remaining - cost).max(0);
        } else {
            self.database.unit_instances[unit_idx].sp_consumed += cost;
        }
    }

    /// SP を `amount` 回復する (最大値でクランプ)。`consume_unit_sp` の逆。
    /// アビリティ効果 霊力回復 / ＳＰ回復 用。
    pub(super) fn restore_unit_sp(&mut self, uid: &str, amount: i32) {
        let Some(unit_idx) = self.database.idx_by_uid(uid) else {
            return;
        };
        let max = self.unit_sp(&self.database.unit_instances[unit_idx]).0;
        let pilot_id = {
            let u = &self.database.unit_instances[unit_idx];
            u.pilot_ids
                .first()
                .cloned()
                .unwrap_or_else(|| u.pilot_name.clone())
        };
        let pilot_idx = self
            .database
            .pilot_instances
            .iter()
            .position(|p| p.id == pilot_id || p.pilot_data_name == pilot_id);
        if let Some(pi) = pilot_idx {
            let p = &mut self.database.pilot_instances[pi];
            p.sp_remaining = (p.sp_remaining + amount).min(max);
        } else {
            let u = &mut self.database.unit_instances[unit_idx];
            u.sp_consumed = (u.sp_consumed - amount).max(0);
        }
    }

    /// uid のユニットの居る位置でユニットコマンドメニューを再表示する。
    pub(super) fn reopen_unit_menu_for(&mut self, uid: &str) {
        if let Some(u) = self.database.unit_by_uid(uid) {
            let pos = (u.x, u.y);
            self.open_unit_menu(pos);
        }
    }
}
