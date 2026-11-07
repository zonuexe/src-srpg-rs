//! 合体技 (武器属性 `合`)。原典 `Unit.CombinationPartner` / `IsCombinationAttackAvailable`
//! と、`Unit.Attack` / `Command.bas` のパートナーの扱い。
//!
//! 仕様: `SRC.Sharp/SRC.Sharp.Help/src/合体技に関する属性.md` と
//! `武器関連特殊能力.md` の `合体技Lv*=合体技名 パートナー…`。
//! - パートナーは発動地点 (射程 1 の技は攻撃目標、射程 2 以上は使うユニット) に隣接して
//!   いなければならない。人数が多い技は斜め・2 マス先まで探す。
//! - パートナーが同名の武器を持っていればその武器を使える必要があり、持っていなければ
//!   使うユニットの技の消費 EN・必要気力で判定する。行動済みでもよい。
//! - 使うとパートナーも EN・弾数を消費し、行動済みになる。反撃では使えない。

use super::App;
use crate::combat;
use crate::data::unit::WeaponData;

impl App {
    /// パイロット `a` から `b` への信頼度 (`SetRelation` の値。未設定は 0)。
    pub(crate) fn relation(&self, a: &str, b: &str) -> i64 {
        self.script_var(&format!("__rel_{a}_{b}"))
            .parse()
            .unwrap_or(0)
    }

    /// 原典 `Unit.CombinationPartner "武装"`: `idx` が `weapon` を `origin` (発動地点) で
    /// 使うときのパートナー (`unit_instances` の添字、ユニット特殊能力 `合体技` の並び順)。
    /// 揃わなければ `None`。マップに出ていないユニットは、パートナーが仲間にいればよい
    /// (ステータス表示用。そのときは空の一覧を返す)。
    pub(crate) fn combination_partners(
        &self,
        idx: usize,
        weapon: &WeaponData,
        origin: (u32, u32),
    ) -> Option<Vec<usize>> {
        let db = &self.database;
        let me = &db.unit_instances[idx];
        // 正常な判断ができない。
        if me.has_condition("混乱") {
            return None;
        }
        // ユニット特殊能力「合体技」から、この技のパートナー一覧と必要な信頼度を探す。
        let (level, names) = me
            .active_features
            .iter()
            .filter(|f| f.is_active && crate::feature::feature_name_matches_base(&f.name, "合体技"))
            .find_map(|f| {
                let mut it = f.value.split_whitespace();
                (it.next() == Some(weapon.name.as_str())).then(|| {
                    let level = crate::feature::feature_level(std::slice::from_ref(f), "合体技")
                        .filter(|_| f.name != "合体技")
                        .unwrap_or(0);
                    (level, it.map(str::to_string).collect::<Vec<_>>())
                })
            })?;

        // 出撃していなければ、パートナーが仲間にいるだけでよい。
        if me.off_map || db.map.is_none() {
            let found = names.iter().all(|n| {
                db.unit_instances.iter().enumerate().any(|(i, u)| {
                    i != idx
                        && !u.party.is_hostile_to(me.party)
                        && (&u.unit_data_name == n || &u.pilot_name == n)
                })
            });
            return found.then(Vec::new);
        }

        // パートナーを探す範囲 (人数が多い技ほど広い)。
        let n = names.len();
        let mut limit = match (weapon.max_range <= 1, n) {
            (true, 8..) | (false, 9..) => 12,
            (true, 4..) | (false, 5..) => 8,
            _ => 4,
        };
        if limit == 4 && self.is_script_var_defined("Option(合体技斜め隣接可)") {
            limit = 8;
        }
        const AROUND: [(i32, i32); 12] = [
            (-1, 0),
            (1, 0),
            (0, -1),
            (0, 1),
            (-1, -1),
            (1, 1),
            (-1, 1),
            (1, -1),
            (-2, 0),
            (2, 0),
            (0, -2),
            (0, 2),
        ];
        let mut partners: Vec<usize> = Vec::new();
        for name in &names {
            let found = AROUND[..limit].iter().find_map(|&(dx, dy)| {
                let x = origin.0.checked_add_signed(dx)?;
                let y = origin.1.checked_add_signed(dy)?;
                let i = db.uid_at(x, y).and_then(|uid| db.idx_by_uid(uid))?;
                let u = &db.unit_instances[i];
                let ok = (&u.unit_data_name == name || &u.pilot_name == name)
                    && i != idx
                    && !partners.contains(&i)
                    && !me.party.is_hostile_to(u.party)
                    // 行動できなければだめ。
                    && !u.attack_disabled()
                    && !["混乱", "恐怖", "憑依"].iter().any(|c| u.has_condition(c))
                    // 合体技にレベルがあればパイロット間の信頼度を調べる。
                    && (level <= 0
                        || (self.relation(&me.pilot_name, &u.pilot_name) >= i64::from(level)
                            && self.relation(&u.pilot_name, &me.pilot_name) >= i64::from(level)))
                    && self.partner_can_join(i, weapon);
                ok.then_some(i)
            });
            partners.push(found?);
        }
        Some(partners)
    }

    /// パートナー `i` が合体技 `weapon` に加われるか。同名の武器を持っていればその武器を
    /// 使える (必要技能・気力・EN・弾数) こと、持っていなければ技の必要気力と消費 EN を
    /// 満たすこと。
    fn partner_can_join(&self, i: usize, weapon: &WeaponData) -> bool {
        let u = &self.database.unit_instances[i];
        let own = self
            .database
            .effective_combat_data(i)
            .and_then(|(_, d)| d.weapons.into_iter().find(|w| w.name == weapon.name));
        match own {
            Some(w) => self.weapon_firable_base(i, &w),
            None => {
                let en = self.database.effective_max_en(u) - u.en_consumed;
                (weapon.necessary_morale <= 0 || u.morale >= weapon.necessary_morale)
                    && (weapon.en_consumption <= 0 || en >= weapon.en_consumption)
            }
        }
    }

    /// 原典 `IsCombinationAttackAvailable`: パートナーの揃う地点があるか。射程 1 の技は
    /// 隣の敵のどれかを囲めればよく、射程 2 以上は自分の周りにパートナーがいればよい。
    pub(crate) fn combination_attack_available(&self, idx: usize, weapon: &WeaponData) -> bool {
        let u = &self.database.unit_instances[idx];
        if u.off_map || self.database.map.is_none() {
            return self.combination_partners(idx, weapon, (u.x, u.y)).is_some();
        }
        if weapon.max_range <= 1 && !combat::is_map_weapon(weapon) {
            let party = u.party;
            [(-1, 0), (1, 0), (0, -1), (0, 1)].iter().any(|&(dx, dy)| {
                let (Some(x), Some(y)) = (u.x.checked_add_signed(dx), u.y.checked_add_signed(dy))
                else {
                    return false;
                };
                self.database
                    .uid_at(x, y)
                    .and_then(|uid| self.database.unit_by_uid(uid))
                    .is_some_and(|t| t.party.is_hostile_to(party))
                    && self.combination_partners(idx, weapon, (x, y)).is_some()
            })
        } else {
            self.combination_partners(idx, weapon, (u.x, u.y)).is_some()
        }
    }

    /// `weapon` で `target` を攻撃するのに合体技のパートナーが揃っているか (原典
    /// `IsTargetWithinRange`: 射程 1 の合体技は目標を囲んでいる必要がある)。合体技でなければ
    /// 常に true。
    pub(crate) fn combination_ok_vs(
        &self,
        idx: usize,
        weapon: &WeaponData,
        target: (u32, u32),
    ) -> bool {
        !weapon.class.contains('合')
            || combat::is_map_weapon(weapon)
            || weapon.max_range > 1
            || self.combination_partners(idx, weapon, target).is_some()
    }

    /// 攻撃に使う合体技のパートナーを決めて記録する (原典 `SelectedPartners`)。発動地点は
    /// 射程 1 の技なら目標、そうでなければ自分。合体技でなければ記録を消す。
    pub(super) fn select_partners(&mut self, idx: usize, weapon: &WeaponData, target: (u32, u32)) {
        self.selected_partners.clear();
        if !weapon.class.contains('合') {
            return;
        }
        let u = &self.database.unit_instances[idx];
        let origin = if weapon.max_range <= 1 {
            target
        } else {
            (u.x, u.y)
        };
        let partners = self
            .combination_partners(idx, weapon, origin)
            .unwrap_or_default();
        self.selected_partners = partners
            .into_iter()
            .map(|i| self.database.unit_instances[i].uid.clone())
            .collect();
    }

    /// 合体技の攻撃メッセージの主語 (原典 `Unit.Attack` の「攻撃を行ったことについての
    /// システムメッセージ」)。`idx` は合体技を使うユニット。
    pub(super) fn combination_subject(&self, idx: usize) -> String {
        let db = &self.database;
        let unit_nick = |u: &crate::UnitInstance| {
            db.unit_by_name(&u.unit_data_name)
                .map_or_else(|| u.unit_data_name.clone(), |d| d.nickname.clone())
        };
        let pilot_nick = |u: &crate::UnitInstance| {
            db.pilot_by_name(&u.pilot_name)
                .map_or_else(|| u.pilot_name.clone(), |p| p.nickname.clone())
        };
        let me = &db.unit_instances[idx];
        let partners: Vec<&crate::UnitInstance> = self
            .selected_partners
            .iter()
            .filter_map(|uid| db.unit_by_uid(uid))
            .collect();
        let (nick, pilot) = (unit_nick(me), pilot_nick(me));
        match partners.as_slice() {
            [] => format!("{nick}は"),
            [p] if unit_nick(p) != nick => format!("{nick}は[{}]と共に", unit_nick(p)),
            [p] if pilot_nick(p) != pilot => format!("{pilot}と[{}]は", pilot_nick(p)),
            [p, q] if unit_nick(p) != nick => {
                format!("{nick}は[{}]、[{}]と共に", unit_nick(p), unit_nick(q))
            }
            [p, q] if pilot_nick(p) != pilot => {
                format!("{pilot}は[{}]、[{}]と共に", pilot_nick(p), pilot_nick(q))
            }
            _ => format!("{nick}達は"),
        }
    }

    /// 合体技のパートナーが EN・弾数を消費する (原典 `Unit.Attack` の「合体技のパートナーの
    /// 弾数＆ＥＮの消費」)。同名の武器があればその武器を使ったものとし、なければ使う
    /// ユニット (`atk_uid`) の技の消費 EN と代償 (消・気・霊・失) を払う。自爆技・ＨＰ消費で
    /// 壊れたパートナーはここで破壊する。
    pub(super) fn consume_partner_resources(&mut self, atk_uid: &str, weapon: &WeaponData) {
        let en_cost = self
            .database
            .idx_by_uid(atk_uid)
            .map_or(weapon.en_consumption, |i| self.weapon_en_cost(i, weapon));
        for uid in self.selected_partners.clone() {
            let Some(i) = self.database.idx_by_uid(&uid) else {
                continue;
            };
            let own = self
                .database
                .unit_by_name(&self.database.unit_instances[i].unit_data_name)
                .and_then(|d| {
                    let di = d.weapons.iter().position(|w| w.name == weapon.name)?;
                    Some((di, d.weapons[di].clone()))
                });
            let used = match own {
                Some((di, w)) => {
                    self.use_weapon(i, di);
                    w
                }
                None => {
                    if weapon.en_consumption > 0 {
                        self.database.unit_instances[i].en_consumed += en_cost;
                    }
                    self.pay_weapon_penalties(i, weapon);
                    weapon.clone()
                }
            };
            self.self_destruct_after_attack(&uid, &used);
        }
    }

    /// 攻撃の後、合体技のパートナーを行動済みにし、敵を倒していれば経験値を渡して
    /// (原典 `Command.bas` の `GetExp …, "パートナー"` / `UseAction`)、記録を消す。
    pub(super) fn finish_partners(&mut self, kill_exp: Option<i32>) {
        let partners = std::mem::take(&mut self.selected_partners);
        let gain_exp = !self.is_script_var_defined("Option(合体技パートナー経験値無効)");
        let use_action = !self.is_script_var_defined("Option(合体技パートナー行動数無消費)");
        for uid in partners {
            let Some(i) = self.database.idx_by_uid(&uid) else {
                continue;
            };
            if let (Some(exp), true) = (kill_exp, gain_exp) {
                self.award_support_exp(i, exp);
            }
            if use_action {
                self.database.unit_instances[i].has_acted = true;
            }
        }
    }
}
