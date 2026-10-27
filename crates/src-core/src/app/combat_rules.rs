//! 戦闘の付随ルール: 武器特殊効果・防御属性・必要技能/発射可否・撃破報酬・経験値。

use super::*;

impl App {
    /// 精神コマンド「復活」判定: `idx` のユニットが「復活」を保持していれば HP を
    /// 全快させ condition を消費して `true` を返す (= 撃破は成立しない)。保持して
    /// いなければ `false` (= 撃破成立) を返す。各撃破サイトで `remove_unit_at` の
    /// 直前に呼ぶ。
    pub(crate) fn revive_if_possible(&mut self, idx: usize) -> bool {
        if self.database.unit_instances[idx].has_condition("復活") {
            let u = &mut self.database.unit_instances[idx];
            u.remove_condition("復活");
            u.damage = 0;
            true
        } else {
            false
        }
    }

    /// 特殊効果攻撃属性 (`特殊効果攻撃属性.md`): 命中時に確率で防御側へ状態異常を
    /// 付与する。発生確率はクリティカルと同様 (CT率 + (技量差)/2)。付与した状態異常名の
    /// 列を返す (メッセージ用)。武器に該当属性が無ければ何もしない。
    pub(super) fn apply_weapon_special_effects(
        &mut self,
        def_idx: usize,
        firer_idx: usize,
        weapon: &crate::data::unit::WeaponData,
        atk_pilot: &crate::data::pilot::PilotData,
        def_pilot: &crate::data::pilot::PilotData,
    ) -> Vec<String> {
        let effects = crate::combat::weapon_special_effects(&weapon.class);
        let morale_down = crate::combat::weapon_morale_reduction(&weapon.class);
        let possession = crate::combat::weapon_possession(&weapon.class);
        if effects.is_empty() && morale_down.is_none() && possession.is_none() {
            return Vec::new();
        }
        // 特殊効果無効化 (防御特性に関する特殊能力.md): 防御側が指定特殊効果を無効化する。
        // `特殊効果無効化=全` (または値省略) は全特殊効果を無効化 (サンプルは全て `=全`)。
        // 個別指定は値に一致する効果のみ無効化する (下のループで部分一致判定)。
        let nullify_vals: Vec<String> = self.database.unit_instances[def_idx]
            .active_features
            .iter()
            .filter(|f| f.is_active && f.name == "特殊効果無効化")
            .map(|f| f.value.trim().to_string())
            .collect();
        let nullify_all = nullify_vals
            .iter()
            .any(|v| v.is_empty() || v.split_whitespace().any(|t| t == "全"));
        if nullify_all {
            // 全特殊効果 (状態異常・気力低下・支配) を無効化。ダメージ自体は別経路。
            return Vec::new();
        }
        let prob =
            (weapon.critical + (atk_pilot.technique - def_pilot.technique) / 2).clamp(1, 100);
        // 耐性 / 弱点: 武器の属性に対し対象が耐性を持てば発動率半減、弱点を持てば倍。
        let prob = self.adjust_proc_for_resistance(def_idx, &weapon.class, prob);
        // 抵抗力Lv* (回避系特殊能力.md): 特殊効果発生確率を 10×Lv% 減少
        // (C# Unit.cs:12275 `prob - 10 * FeatureLevel("抵抗力")`)。
        let resist_lv = crate::feature::feature_level(
            &self.database.unit_instances[def_idx].active_features,
            "抵抗力",
        )
        .unwrap_or(0);
        let prob = prob - 10 * resist_lv;
        if prob <= 0 || (self.next_u32() % 100) as i32 >= prob {
            return Vec::new();
        }
        let is_boss = self.database.unit_instances[def_idx].is_boss();
        let mut applied = Vec::new();
        for (name, lifetime) in effects {
            // ボスランク適用ユニットは石化 / 死の宣告 を無効化 (BossRankコマンド.md)。
            if is_boss && (name == "石化" || name == "死の宣告") {
                continue;
            }
            // 特殊効果無効化 (個別指定): 値が状態異常名に部分一致するものは無効化。
            if nullify_vals
                .iter()
                .any(|v| !v.is_empty() && (name.contains(v.as_str()) || v.contains(name.as_str())))
            {
                continue;
            }
            self.database.unit_instances[def_idx]
                .add_condition(crate::Condition::new(&name, lifetime));
            applied.push(name);
        }
        // 脱 / Ｄ: 対象の気力を低下。Ｄ は低下分の半分を攻撃側 (firer) へ吸収する。
        if let Some(amount) = morale_down {
            let uid = self.database.unit_instances[def_idx].uid.clone();
            self.add_unit_morale(&uid, -amount);
            applied.push(format!("気力 -{amount}"));
            if crate::combat::weapon_morale_absorbs(&weapon.class) {
                let gain = amount / 2;
                if gain > 0 {
                    let fuid = self.database.unit_instances[firer_idx].uid.clone();
                    self.add_unit_morale(&fuid, gain);
                    applied.push(format!("気力吸収 +{gain}"));
                }
            }
        }
        // 憑 (憑依) / 魅 (魅了): 相手の勢力を攻撃側へ奪う支配系属性 (特殊効果攻撃属性.md)。
        // BossRank 適用ユニットには無効。攻撃側自身 (firer_idx==def_idx の自爆等) は対象外。
        if let Some(kind) = possession {
            if !is_boss && firer_idx != def_idx {
                let firer_party = self.database.unit_instances[firer_idx].party;
                let firer_uid = self.database.unit_instances[firer_idx].uid.clone();
                let def = &mut self.database.unit_instances[def_idx];
                match kind {
                    // 憑依: 相手を乗っ取り**恒久支配**。勢力を攻撃側へ移し、SpecialPower の
                    // 使用を封じる condition `憑依` (永続=-1) を付与する。復帰しない
                    // (charm_revert_party は使わない)。
                    "憑依" => {
                        def.party = firer_party;
                        def.add_condition(crate::Condition::new("憑依", -1));
                        applied.push("憑依".to_string());
                    }
                    // 魅了: 相手を **3 ターン魅了**。元勢力を charm_revert_party に退避し
                    // 勢力を攻撃側へ移して「魅了主を護衛する味方」として行動させる
                    // (ai_mode=護衛 <firer_uid>)。condition `魅了` lifetime=4 (3 ターン +
                    // フェイズ開始 tick 吸収) が期限切れになると begin_phase が元勢力へ復帰。
                    "魅了" => {
                        if def.charm_revert_party.is_none() {
                            def.charm_revert_party = Some(def.party);
                        }
                        def.party = firer_party;
                        def.ai_mode = format!("護衛 {firer_uid}");
                        def.add_condition(crate::Condition::new("魅了", 4));
                        applied.push("魅了".to_string());
                    }
                    _ => {}
                }
            }
        }
        applied
    }

    /// 武器 class が防御特性の属性リスト `attr_list` に該当するか
    /// (VB5 `Unit.cls::Weakness/Resist/Absorb` 準拠の字単位部分文字列照合)。
    /// `attr_list` の各属性 (空白は無視) が weapon_class に**部分文字列**として含まれれば該当する
    /// (例: 弱点 "火" は複合 class "格実火" に該当)。`全`=常に該当 / `物`=武器に 魔・精 が無ければ該当。
    /// 旧実装は `split_whitespace` の完全トークン一致で、無空白複合 class ("格実火" 等) が
    /// 弱点 "火" に一致せず防御特性が事実上機能していなかった (VB5 は `InStrNotNest`=部分一致)。
    pub(super) fn defense_attr_matches(weapon_class: &str, attr_list: &str) -> bool {
        for c in attr_list.chars() {
            if c.is_whitespace() {
                continue;
            }
            match c {
                '全' => return true,
                '物' => {
                    if !weapon_class.contains('魔') && !weapon_class.contains('精') {
                        return true;
                    }
                }
                '魔' => {
                    // 魔武/魔突/魔接/魔銃/魔実 武器には「魔」属性への防御特性が効かない
                    // (spec `防御特性に関する特殊能力.md`「魔法武器には魔法への防御能力が効かない」)。
                    // 逆順 (武魔 等) は対象 = `魔X` 並びのときだけ免除する。
                    const MAGIC_COMBAT: [&str; 5] = ["魔武", "魔突", "魔接", "魔銃", "魔実"];
                    if weapon_class.contains('魔')
                        && !MAGIC_COMBAT.iter().any(|m| weapon_class.contains(m))
                    {
                        return true;
                    }
                }
                _ => {
                    if weapon_class.contains(c) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// 武器属性が防御側の `弱点` (静的特殊能力 + 一時付加 condition `弱点:<属性>`) に一致するか。
    /// 一致時は VB5 `Unit.cls:6949` 準拠で被ダメージ計算の装甲を半減させる
    /// (`attack_resolve_and_run` が予測前に `def_unit.armor` を ÷2 する)。
    pub(super) fn weapon_hits_weakness(&self, def_idx: usize, weapon_class: &str) -> bool {
        let inst = &self.database.unit_instances[def_idx];
        let weak = crate::feature::feature_value(&inst.active_features, "弱点").unwrap_or("");
        if Self::defense_attr_matches(weapon_class, weak) {
            return true;
        }
        inst.conditions
            .iter()
            .filter_map(|c| c.name.strip_prefix("弱点:"))
            .any(|el| weapon_class.contains(el))
    }

    /// 武器属性が防御側の `吸収` に一致し、かつ `有効` で打ち消されていないか。
    /// 一致時は VB5 `Unit.cls:6951` 準拠で被ダメージ計算の装甲を無視 (arm=0) する
    /// (`attack_resolve_and_run` が予測前に `def_unit.armor=0` にする)。これにより吸収量が
    /// spec「装甲値を無視して計算されたダメージを半減させた値」基準になり、
    /// `defense_attribute_damage` の吸収分岐 `-(damage/2)` が正しい回復量へ反転する。
    pub(super) fn weapon_hits_absorb(&self, def_idx: usize, weapon_class: &str) -> bool {
        let inst = &self.database.unit_instances[def_idx];
        let absorb = crate::feature::feature_value(&inst.active_features, "吸収").unwrap_or("");
        let effective = crate::feature::feature_value(&inst.active_features, "有効").unwrap_or("");
        Self::defense_attr_matches(weapon_class, absorb)
            && !Self::defense_attr_matches(weapon_class, effective)
    }

    /// 防御特性による装甲調整を予測前に `def_unit.armor` へ適用する (VB5 `Unit.cls:6949-6951`)。
    /// 弱点 → 装甲半減 (被ダメージ増) / 吸収 (かつ非有効) → 装甲無視 (arm=0)。弱点 > 吸収 の優先。
    /// 通常攻撃 / 援護 / 反撃の全経路で一貫適用するための共有ヘルパ。
    pub(crate) fn apply_defense_armor_mod(
        &self,
        def_idx: usize,
        weapon_class: &str,
        def_unit: &mut crate::data::unit::UnitData,
    ) {
        if self.weapon_hits_weakness(def_idx, weapon_class) {
            def_unit.armor /= 2;
        } else if self.weapon_hits_absorb(def_idx, weapon_class) {
            def_unit.armor = 0;
        }
    }

    /// 特殊効果の発動確率を、対象の `耐性` / `弱点` 特殊能力で調整する
    /// (VB5 `Unit.cls::CriticalProbability`: 弱点属性に対しては発動確率 +10、耐性属性には半減)。
    pub(super) fn adjust_proc_for_resistance(
        &self,
        def_idx: usize,
        weapon_class: &str,
        prob: i32,
    ) -> i32 {
        let inst = &self.database.unit_instances[def_idx];
        let weak = crate::feature::feature_value(&inst.active_features, "弱点").unwrap_or("");
        let resist = crate::feature::feature_value(&inst.active_features, "耐性").unwrap_or("");
        // 弱/効 属性で付加された一時的弱点 (condition `弱点:<属性>`)。
        let added_weak_match = inst
            .conditions
            .iter()
            .filter_map(|c| c.name.strip_prefix("弱点:"))
            .any(|el| weapon_class.contains(el));
        if weak.is_empty() && resist.is_empty() && !added_weak_match {
            return prob;
        }
        let mut p = prob;
        if Self::defense_attr_matches(weapon_class, weak) || added_weak_match {
            p += 10;
        } else if Self::defense_attr_matches(weapon_class, resist) {
            p /= 2;
        }
        p.clamp(1, 100)
    }

    /// 防御特性 (`吸収` / `無効化` / `耐性`) を与ダメージへ適用する (VB5 `Unit.cls::Damage`)。
    /// 武器属性が一致するとき: 吸収→`-damage/2`(回復)、無効化→0、耐性→÷2。優先順は
    /// spec `防御特性に関する特殊能力.md`「弱点 > 有効 > 吸収 > 無効化 > 耐性」。
    /// `弱点` の装甲半減は `weapon_hits_weakness` で予測前に適用済みのためここでは無処理
    /// (`有効` は防御特性を打ち消すため、`弱点`/`有効` 一致時は吸収/無効化/耐性を適用しない)。
    /// 対象が該当特性を持たなければ `damage` をそのまま返す。
    /// 注: 吸収は spec では「装甲無視ダメージ÷2」だが、本実装は最終ダメージ基準の近似 (要精緻化)。
    pub(crate) fn defense_attribute_damage(
        &self,
        def_idx: usize,
        weapon_class: &str,
        damage: i64,
    ) -> i64 {
        let inst = &self.database.unit_instances[def_idx];
        let m = |name: &str| -> bool {
            let list = crate::feature::feature_value(&inst.active_features, name).unwrap_or("");
            Self::defense_attr_matches(weapon_class, list)
        };
        // 弱点 / 有効 が一致するときは後段の防御特性 (吸収/無効化/耐性) を適用しない。
        if m("弱点") || m("有効") {
            return damage;
        }
        if damage > 0 && m("吸収") {
            return -(damage / 2);
        }
        if m("無効化") {
            return 0;
        }
        if m("耐性") {
            return damage / 2;
        }
        damage
    }

    /// ユニットの主パイロットの「切り払い」技能レベル (PilotInstance 優先、無ければ
    /// `PilotData.features` の「切り払いLv<n>」から)。0 = 切り払い不可。
    pub(super) fn unit_parry_level(&self, idx: usize) -> i32 {
        self.unit_pilot_skill_level(idx, "切り払い")
    }

    /// 主パイロット (`pilot_ids` 先頭、無ければ `pilot_name`) の指定技能レベルを返す。
    /// `PilotInstance` を優先し、無ければ静的 `PilotData.features` から末尾の数字を抽出する
    /// (レベル表記が無ければ 1、技能が無ければ 0)。技能名は部分一致 (`skill_level` 同様)。
    pub(super) fn unit_pilot_skill_level(&self, idx: usize, skill: &str) -> i32 {
        let u = &self.database.unit_instances[idx];
        let from_inst = u
            .pilot_ids
            .first()
            .and_then(|id| self.database.pilot_instance_by_id(id))
            .map(|p| p.skill_level(skill))
            .unwrap_or(0);
        if from_inst > 0 {
            return from_inst;
        }
        self.database
            .pilot_by_name(&u.pilot_name)
            .and_then(|pd| pd.features.iter().find(|(n, _)| n.contains(skill)))
            .map(|(n, _)| {
                n.rfind(|c: char| c.is_ascii_digit())
                    .and_then(|pos| {
                        let start = n[..=pos]
                            .rfind(|c: char| !c.is_ascii_digit())
                            .map(|i| i + 1)
                            .unwrap_or(0);
                        n[start..=pos].parse::<i32>().ok()
                    })
                    .unwrap_or(1)
            })
            .unwrap_or(0)
    }

    /// 撃破者のメインパイロットの撃墜数を +1 する (SRC `Pilot.Kills`)。撃墜数は
    /// パイロット用特殊能力として扱われ (`撃墜数LvN`)、`(撃墜数Lv20)` 等の必要技能
    /// ゲートが読む。これにより「規定数撃墜でエース専用武器が解禁される」原典の挙動を再現。
    /// 現値 (静的 features ＋ 実体 skills の最大) を基準に +1 し `PilotInstance.skills` へ
    /// `撃墜数 N` 形式で書き戻す (SetSkill と同形式)。
    pub(super) fn increment_kill_count(&mut self, killer_idx: usize) {
        let cur = self.unit_pilot_skill_level(killer_idx, "撃墜数");
        let Some(pid) = self.database.unit_instances[killer_idx]
            .pilot_ids
            .first()
            .cloned()
        else {
            return;
        };
        if let Some(pi) = self.database.pilot_instance_by_id_mut(&pid) {
            let entry = format!("撃墜数 {}", cur + 1);
            if let Some(pos) = pi.skills.iter().position(|s| s.contains("撃墜数")) {
                pi.skills[pos] = entry;
            } else {
                pi.skills.push(entry);
            }
        }
    }

    /// クリティカル発生率へのパイロット技能補正 (`Unit.cs` クリティカル発生率計算準拠)。
    /// 超反応 (攻撃側 +2×Lv / 防御側 −2×Lv)・超能力 (攻撃側 +5)・底力/超底力/覚悟
    /// (攻撃側 HP が最大の 1/4 以下で +50) の合計を返す。これらの補正は `weapon.critical`
    /// が特殊効果攻撃属性の発動率計算と共有されるため `combat::critical_probability` には
    /// 含めず、命中/クリティカルロール直前に `crit_chance` へ加算する (proc 率への漏れ防止)。
    pub(super) fn crit_skill_bonus(&self, atk_idx: usize, def_idx: usize) -> i32 {
        let mut bonus = 0;
        // 超反応: 攻撃側 +2×Lv / 防御側 −2×Lv。
        bonus += 2 * self.unit_pilot_skill_level(atk_idx, "超反応");
        bonus -= 2 * self.unit_pilot_skill_level(def_idx, "超反応");
        // 超能力: 攻撃側が持てば +5。
        if self.unit_pilot_skill_level(atk_idx, "超能力") > 0 {
            bonus += 5;
        }
        // 底力 / 超底力 / 覚悟: 攻撃側 HP が最大の 1/4 以下なら +50。
        let atk = &self.database.unit_instances[atk_idx];
        let max_hp = self.database.effective_max_hp(atk);
        if max_hp > 0
            && (max_hp - atk.damage) * 4 <= max_hp
            && (self.unit_pilot_skill_level(atk_idx, "底力") > 0
                || self.unit_pilot_skill_level(atk_idx, "覚悟") > 0)
        {
            bonus += 50;
        }
        bonus
    }

    /// 切り払い判定 (SRC `Unit.cs::CheckParryFeature`)。格闘系 (武/格/突/接) の攻撃を、
    /// 防御側が確率で無効化する。攻撃側が `直撃` を持つときは無効化されない。
    /// 確率: `2×防御側Lv − 攻撃側Lv` が `Dice(1..32)` 以上で発動。防御側が切り払いを
    /// 持たない (Lv0) ときは乱数を消費しない (既存 RNG 列を保つ)。
    /// 防御側が `見切り` (特殊防御発動の SpecialPower) を持つときは確率を 32 (必ず成功) に
    /// 上書きする (`Unit.cs::CheckParryFeature`「見切りがあれば必ず発動」、prob=32)。
    pub(super) fn roll_parry(
        &mut self,
        atk_idx: usize,
        def_idx: usize,
        weapon_class: &str,
    ) -> bool {
        let is_melee = weapon_class.contains('武')
            || weapon_class.contains('格')
            || weapon_class.contains('突')
            || weapon_class.contains('接');
        if !is_melee {
            return false;
        }
        let def_parry = self.unit_parry_level(def_idx);
        if def_parry <= 0 {
            return false;
        }
        let atk_parry = self.unit_parry_level(atk_idx);
        // 見切り: 切り払いを必ず成功させる (prob=32)。乱数は通常どおり消費して RNG 列を保つ。
        let prob = if self.database.unit_instances[def_idx].has_condition("見切り") {
            32
        } else {
            2 * def_parry - atk_parry
        };
        let dice = (self.next_u32() % 32) as i32 + 1; // 1..=32
        prob >= dice
    }

    /// 即死 (`即`) の発動判定。武器が `即` 属性を持ち対象がボスでないとき、特殊効果
    /// 発動率 (CT率 + 技量差/2、耐性/弱点補正込み) で proc すれば `true` (= 致死化)。
    /// `即` を持たない武器では乱数を消費しない (既存の RNG 列を保つため)。
    pub(super) fn roll_weapon_instakill(
        &mut self,
        def_idx: usize,
        weapon: &crate::data::unit::WeaponData,
        atk_pilot: &crate::data::pilot::PilotData,
        def_pilot: &crate::data::pilot::PilotData,
    ) -> bool {
        if !weapon.class.split_whitespace().any(|t| t == "即") {
            return false;
        }
        // ボスランク適用ユニットには無効 (BossRankコマンド.md)。
        if self.database.unit_instances[def_idx].is_boss() {
            return false;
        }
        let prob =
            (weapon.critical + (atk_pilot.technique - def_pilot.technique) / 2).clamp(1, 100);
        let prob = self.adjust_proc_for_resistance(def_idx, &weapon.class, prob);
        ((self.next_u32() % 100) as i32) < prob
    }

    /// 減衰系属性 (`衰`=HP / `滅`=EN) をクリティカル時に適用する。対象の現在 HP / EN を
    /// 属性レベルに応じた割合 (Lv1=3/4・Lv2=1/2・Lv3=1/4) に減らす (`特殊効果攻撃属性.md`)。
    /// 減衰は撃破せず (常に 1 以上残す)。適用したラベル列を返す (メッセージ用)。
    pub(super) fn apply_weapon_crit_decay(
        &mut self,
        def_idx: usize,
        weapon: &crate::data::unit::WeaponData,
    ) -> Vec<String> {
        let (hp_lv, en_lv) = crate::combat::weapon_crit_decay_levels(&weapon.class);
        let is_boss = self.database.unit_instances[def_idx].is_boss();
        // 残す割合 (分子, 分母)。ボスは減少率が半減するため残す割合が増える ((4+keep)/8)。
        let keep_frac = |lv: i32| -> (i64, i64) {
            let keep = crate::combat::crit_decay_keep_numer(lv);
            if is_boss {
                (4 + keep, 8)
            } else {
                (keep, 4)
            }
        };
        let mut applied = Vec::new();
        if let Some(lv) = hp_lv {
            let (num, den) = keep_frac(lv);
            let max_hp = self
                .database
                .effective_max_hp(&self.database.unit_instances[def_idx]);
            let cur_hp = (max_hp - self.database.unit_instances[def_idx].damage).max(1);
            let new_hp = (cur_hp * num / den).max(1);
            self.database.unit_instances[def_idx].damage = max_hp - new_hp;
            applied.push(format!("ＨＰ減衰 (残{new_hp})"));
        }
        if let Some(lv) = en_lv {
            let (num, den) = keep_frac(lv);
            let max_en = self
                .database
                .effective_max_en(&self.database.unit_instances[def_idx]);
            let cur_en = (max_en - self.database.unit_instances[def_idx].en_consumed).max(0);
            let new_en = ((i64::from(cur_en) * num / den) as i32).max(0);
            self.database.unit_instances[def_idx].en_consumed = max_en - new_en;
            applied.push(format!("ＥＮ減衰 (残{new_en})"));
        }
        applied
    }

    /// 吹き飛ばし / ノックバック (`吹` / `Ｋ`): 命中時に対象を攻撃側から見て遠ざかる
    /// 方向へ最大 `マス数` (`critical` 時 +1) だけ押し出す。盤外・占有マスで停止する。
    /// 対象サイズ XL / 移動力 0 では不発 (`特殊効果攻撃属性.md`)。Ｋ は攻撃側サイズが
    /// 標的より 2 段階以上小さいと不発。衝突ダメージは未モデル。押し出したら `true`。
    pub(super) fn apply_weapon_knockback(
        &mut self,
        def_idx: usize,
        atk_pos: (u32, u32),
        atk_unit_name: &str,
        weapon: &crate::data::unit::WeaponData,
        critical: bool,
    ) -> bool {
        use crate::data::unit::Size;
        let Some((mut tiles, is_k)) = crate::combat::weapon_knockback(&weapon.class) else {
            return false;
        };
        if critical {
            tiles += 1;
        }
        let (tx, ty, def_name) = {
            let u = &self.database.unit_instances[def_idx];
            (u.x, u.y, u.unit_data_name.clone())
        };
        // 対象サイズ XL / 移動力 0 は固定扱いで不発。
        let (def_size, def_speed) = self
            .database
            .unit_by_name(&def_name)
            .map(|d| (d.size, d.speed))
            .unwrap_or((Size::M, 1));
        if def_size == Size::XL || def_speed <= 0 {
            return false;
        }
        // Ｋ: 攻撃側サイズが標的より 2 段階以上小さいと不発。
        if is_k {
            let atk_size = self
                .database
                .unit_by_name(atk_unit_name)
                .map(|d| d.size)
                .unwrap_or(Size::M);
            // rank が大きいほど小さいサイズ。攻撃側 rank - 標的 rank >= 2 で「2段階以上小さい」。
            if atk_size.rank() - def_size.rank() >= 2 {
                return false;
            }
        }
        // 押し出し方向 (攻撃側→対象の優勢軸)。
        let ddx = tx as i32 - atk_pos.0 as i32;
        let ddy = ty as i32 - atk_pos.1 as i32;
        let (sx, sy) = if ddx.abs() >= ddy.abs() {
            (ddx.signum(), 0)
        } else {
            (0, ddy.signum())
        };
        if sx == 0 && sy == 0 {
            return false;
        }
        let (mw, mh) = match self.database.map.as_ref() {
            Some(m) => (m.width as i32, m.height as i32),
            None => return false,
        };
        let uid = self.database.unit_instances[def_idx].uid.clone();
        let (mut cx, mut cy) = (tx as i32, ty as i32);
        let mut moved = false;
        for _ in 0..tiles {
            let (nx, ny) = (cx + sx, cy + sy);
            if nx < 0 || ny < 0 || nx >= mw || ny >= mh {
                break; // 盤外で停止
            }
            if self.database.uid_at(nx as u32, ny as u32).is_some() {
                break; // 他ユニットで停止
            }
            self.database.move_unit(&uid, nx as u32, ny as u32);
            cx = nx;
            cy = ny;
            moved = true;
        }
        moved
    }

    /// 盗 (`盗`): クリティカル時に相手から資金またはアイテムを奪う (`特殊効果攻撃属性.md`)。
    /// 相手が `アイテム所有=<item>` を持てばそのアイテムを (`レアアイテム所有` があれば 1/8 で
    /// 代わりにレアアイテムを) 味方の未装備アイテムに加える (原典 `IList.Add`)。持たなければ修理費の 1/4 の資金。攻撃側が味方
    /// (Player) のときのみ。同じ相手からは 1 度だけ (`被盗` 状態で再取得を抑止)。
    pub(super) fn apply_weapon_crit_steal(
        &mut self,
        def_idx: usize,
        atk_party: crate::Party,
        victim_value: i64,
        weapon: &crate::data::unit::WeaponData,
    ) -> Vec<String> {
        if !weapon.class.split_whitespace().any(|t| t == "盗") {
            return Vec::new();
        }
        // 撃破側が味方 (Player) のときのみ。既に盗んだ相手からは再取得しない。
        if atk_party != crate::Party::Player
            || self.database.unit_instances[def_idx].has_condition("被盗")
        {
            return Vec::new();
        }
        // アイテム所有 / レアアイテム所有 (相手が持つ盗めるアイテム)。
        let feats = &self.database.unit_instances[def_idx].active_features;
        let rare = crate::feature::feature_value(feats, "レアアイテム所有")
            .map(str::to_string)
            .filter(|s| !s.is_empty());
        let normal = crate::feature::feature_value(feats, "アイテム所有")
            .map(str::to_string)
            .filter(|s| !s.is_empty());
        // レアアイテム所有があれば 1/8 で代わりにレアを盗む (`&&` 短絡で非保有時は RNG 不消費)。
        let stolen_item = if rare.is_some() && self.next_u32() % 8 == 0 {
            rare
        } else {
            normal
        };

        if let Some(item) = stolen_item {
            self.database.spare_items.push(item.clone());
            self.database.unit_instances[def_idx].add_condition(crate::Condition::new("被盗", -1));
            return vec![format!("アイテム奪取: {item}")];
        }
        let amount = (victim_value / 4).max(0);
        if amount <= 0 {
            return Vec::new();
        }
        self.add_money(amount);
        self.database.unit_instances[def_idx].add_condition(crate::Condition::new("被盗", -1));
        vec![format!("資金奪取 +{amount}")]
    }

    /// 写 / 化 (能力コピー): クリティカル時に発動者を対象のユニットへ変化させる
    /// (`特殊効果攻撃属性.md`)。`写` はサイズ 2 段階以上差で無効、`化` は制限なし。
    /// パイロットは `set_unit_form` が保持する。適用したラベル列を返す。
    pub(super) fn apply_weapon_crit_copy(
        &mut self,
        atk_idx: usize,
        def_idx: usize,
        weapon: &crate::data::unit::WeaponData,
    ) -> Vec<String> {
        use crate::data::unit::Size;
        let has_sha = weapon.class.split_whitespace().any(|t| t == "写");
        let has_ka = weapon.class.split_whitespace().any(|t| t == "化");
        if !has_sha && !has_ka {
            return Vec::new();
        }
        let atk_name = self.database.unit_instances[atk_idx].unit_data_name.clone();
        let def_name = self.database.unit_instances[def_idx].unit_data_name.clone();
        if def_name == atk_name {
            return Vec::new();
        }
        // 写 (化 でない) はサイズ 2 段階以上差で無効。
        if has_sha && !has_ka {
            let asize = self
                .database
                .unit_by_name(&atk_name)
                .map(|d| d.size)
                .unwrap_or(Size::M);
            let dsize = self
                .database
                .unit_by_name(&def_name)
                .map(|d| d.size)
                .unwrap_or(Size::M);
            if asize.step_diff(dsize) >= 2 {
                return Vec::new();
            }
        }
        let atk_uid = self.database.unit_instances[atk_idx].uid.clone();
        if self.set_unit_form(&atk_uid, &def_name) {
            vec![format!("能力コピー → {def_name}")]
        } else {
            Vec::new()
        }
    }

    /// ラーニング技 (`習`): クリティカル時に対象 (`def_idx`) が持つユニット用特殊能力
    /// `ラーニング可能技=<技名称> [表示]` を攻撃側 (`atk_idx`) のメインパイロットが習得する
    /// (`特殊効果攻撃属性.md` 習 / `その他の特殊能力 ラーニング可能技`)。習得した技は
    /// レベル無しのパイロット用特殊能力として `PilotInstance.skills` に追加され、`(技名称)`
    /// を必要技能とする武器/アビリティのゲートが解禁される。
    ///
    /// `習` は class 内で他属性と連結されうる (`無習` 等) ため文字単位で検出する。
    /// 既に保有 (静的/習得済) の技は再習得しない。適用メッセージを返す。
    pub(super) fn apply_weapon_learning(
        &mut self,
        atk_idx: usize,
        def_idx: usize,
        weapon: &crate::data::unit::WeaponData,
    ) -> Vec<String> {
        if !weapon.class.contains('習') {
            return Vec::new();
        }
        // 対象ユニットの「ラーニング可能技」を列挙 (value の先頭トークンが技名称、
        // 続く "表示" は表示フラグ。本実装は技名のみ使用)。
        let def_name = self.database.unit_instances[def_idx].unit_data_name.clone();
        let learnable: Vec<String> = self
            .database
            .unit_by_name(&def_name)
            .map(|d| {
                d.features
                    .iter()
                    .filter(|(n, _)| n == "ラーニング可能技")
                    .filter_map(|(_, v)| v.split_whitespace().next())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if learnable.is_empty() {
            return Vec::new();
        }
        let Some(pid) = self.database.unit_instances[atk_idx]
            .pilot_ids
            .first()
            .cloned()
        else {
            return Vec::new();
        };
        let mut msgs = Vec::new();
        for skill in learnable {
            // 既に保有 (静的 features / 習得済 skills) していれば再習得しない。
            if self.unit_pilot_skill_level(atk_idx, &skill) > 0 {
                continue;
            }
            if let Some(pi) = self.database.pilot_instance_by_id_mut(&pid) {
                pi.skills.push(skill.clone());
                msgs.push(format!("ラーニング！ {skill} を習得"));
            }
        }
        msgs
    }

    /// 引き寄せ / 強制転移 (`引` / `転`): クリティカル時に対象の位置を移す
    /// (`特殊効果攻撃属性.md`)。`引`=攻撃側に隣接する空きマスへ、`転`=現在地から
    /// 属性レベル距離内のランダムな空きマスへ。対象 XL / 移動力 0 では不発。
    /// 適用したラベル列を返す (メッセージ用)。
    pub(super) fn apply_weapon_crit_reposition(
        &mut self,
        def_idx: usize,
        atk_pos: (u32, u32),
        weapon: &crate::data::unit::WeaponData,
    ) -> Vec<String> {
        use crate::data::unit::Size;
        let (pull, teleport) = crate::combat::weapon_crit_reposition(&weapon.class);
        if !pull && teleport.is_none() {
            return Vec::new();
        }
        let def_name = self.database.unit_instances[def_idx].unit_data_name.clone();
        let (def_size, def_speed) = self
            .database
            .unit_by_name(&def_name)
            .map(|d| (d.size, d.speed))
            .unwrap_or((Size::M, 1));
        if def_size == Size::XL || def_speed <= 0 {
            return Vec::new();
        }
        let uid = self.database.unit_instances[def_idx].uid.clone();
        let mut applied = Vec::new();
        // 引き寄せ: 攻撃側の隣接空きマスへ。
        if pull {
            if let Some((nx, ny)) = self.find_empty_adjacent_tile(atk_pos) {
                self.database.move_unit(&uid, nx, ny);
                applied.push("引き寄せ".to_string());
            }
        }
        // 強制転移: 現在地から dist マス内のランダムな空きマスへ。
        if let Some(dist) = teleport {
            let (px, py) = {
                let u = &self.database.unit_instances[def_idx];
                (u.x as i32, u.y as i32)
            };
            let (mw, mh) = match self.database.map.as_ref() {
                Some(m) => (m.width as i32, m.height as i32),
                None => return applied,
            };
            let mut candidates: Vec<(u32, u32)> = Vec::new();
            for dx in -dist..=dist {
                for dy in -dist..=dist {
                    if dx == 0 && dy == 0 || dx.abs() + dy.abs() > dist {
                        continue;
                    }
                    let (nx, ny) = (px + dx, py + dy);
                    if nx < 0 || ny < 0 || nx >= mw || ny >= mh {
                        continue;
                    }
                    if self.database.uid_at(nx as u32, ny as u32).is_none() {
                        candidates.push((nx as u32, ny as u32));
                    }
                }
            }
            if !candidates.is_empty() {
                let pick = (self.next_u32() as usize) % candidates.len();
                let (nx, ny) = candidates[pick];
                self.database.move_unit(&uid, nx, ny);
                applied.push("強制転移".to_string());
            }
        }
        applied
    }

    /// 撃破報酬を撃破側ユニット (`killer_idx`) に付与する。`exp` は 努力 反映済みの最終
    /// 経験値。`UnitInstance.total_exp` への加算に加え、メインパイロットの `PilotInstance`
    /// を成長させ、レベルが上がれば `レベルアップ / LevelUp` イベントを発火する。資金は
    /// `victim_value / 2` を基準に、撃破側の「獲得資金増加」技能で +10%/Lv、「幸運」で
    /// 2 倍 (消費) して `App.money` に加算する。獲得資金額を返す (0 なら無し)。
    /// `idx` のユニットの主パイロット (静的 `PilotData`) の性格を返す。
    pub(super) fn main_pilot_personality(&self, idx: usize) -> Option<String> {
        self.database
            .unit_instances
            .get(idx)
            .and_then(|u| self.database.pilot_by_name(&u.pilot_name))
            .and_then(|p| p.personality.clone())
    }

    /// 主パイロットの性格が `機械` か (気力が変動しない)。
    pub(super) fn main_pilot_is_mecha(&self, idx: usize) -> bool {
        self.main_pilot_personality(idx).as_deref() == Some("機械")
    }

    /// 撃墜による気力変動 (`Unit.cs`)。撃破陣営: 撃破者トータル +4 (機械は +3)、同陣営の他
    /// 出撃中ユニット +1 (機械は不動)。被撃破陣営: 性格別 (超強気 +2 / 強気 +1 / 弱気 -1 /
    /// その他・機械 0)。マップ攻撃の撃破は `award_kill_rewards` を経由しないため対象外
    /// (原典どおり)。`victim_party` は撃破された側の陣営。
    pub(super) fn apply_kill_morale(&mut self, killer_idx: usize, victim_party: crate::Party) {
        let killer_party = self.database.unit_instances[killer_idx].party;
        for i in 0..self.database.unit_instances.len() {
            if self.database.unit_instances[i].off_map {
                continue;
            }
            let party = self.database.unit_instances[i].party;
            let delta = if party == killer_party {
                // 撃破者本人: +3 (常時) ＋ 陣営加算 +1 (機械以外) = 機械 +3 / それ以外 +4。
                // 同陣営の他ユニット: +1 (機械以外)。
                let mecha = self.main_pilot_is_mecha(i);
                if i == killer_idx {
                    if mecha {
                        3
                    } else {
                        4
                    }
                } else if mecha {
                    0
                } else {
                    1
                }
            } else if party == victim_party {
                // 被撃破陣営: 性格別 (機械・その他は default 0)。
                match self.main_pilot_personality(i).as_deref() {
                    Some("超強気") => 2,
                    Some("強気") => 1,
                    Some("弱気") => -1,
                    _ => 0,
                }
            } else {
                0
            };
            if delta != 0 {
                let m = &mut self.database.unit_instances[i];
                m.morale = (m.morale + delta).clamp(0, 150);
            }
        }
    }

    /// `unit_idx` が `wd` (武器データ) を今発射できるか (残弾・EN・必要気力)。実行時資源を見る。
    /// 該当 UnitWeapon が無い (未 populate) 場合は弾無制限とみなす (= 既存テストへ無影響)。
    /// 武器の必要技能 / 必要条件 ((念力Lv3) 形式の括弧条件) を満たすか。資源 (EN/弾/
    /// 気力) は判定しない使用「資格」のみのゲート。強制武器選択時にも必須。
    pub(crate) fn weapon_skill_ok(
        &self,
        unit_idx: usize,
        wd: &crate::data::unit::WeaponData,
    ) -> bool {
        let u = &self.database.unit_instances[unit_idx];
        crate::necessary_skill::weapon_usable_by(wd, u, &self.database)
    }

    pub(crate) fn weapon_firable(
        &self,
        unit_idx: usize,
        wd: &crate::data::unit::WeaponData,
    ) -> bool {
        // イベントコマンド `Disable` で封印されている。
        if self.is_disabled(&self.database.unit_instances[unit_idx], &wd.name) {
            return false;
        }
        // 必要技能 / 必要条件 (使用資格)。
        if !self.weapon_skill_ok(unit_idx, wd) {
            return false;
        }
        let u = &self.database.unit_instances[unit_idx];
        // 必要気力。
        if wd.necessary_morale > 0 && u.morale < wd.necessary_morale {
            return false;
        }
        // EN。
        let max_en = self.database.effective_max_en(u);
        if max_en - u.en_consumed < wd.en_consumption {
            return false;
        }
        // 残弾 (有限弾のみ)。武器を名前で unit_data → index 解決し、該当 UnitWeapon を見る。
        if let Some(di) = self
            .database
            .unit_by_name(&u.unit_data_name)
            .and_then(|d| d.weapons.iter().position(|w| w.name == wd.name))
        {
            if let Some(uw) = u.weapons.iter().find(|w| w.weapon_index == di) {
                if !uw.has_ammo() {
                    return false;
                }
            }
        }
        true
    }

    /// 能力 (武器・アビリティ) `name` が `Disable` で封印されているか (原典 `IsDisabled`)。
    /// 全ユニット共通の `Disable(<name>)` と、ユニット名 (ユニットデータ名) 指定の
    /// `Disable(<ユニット名>,<name>)` を見る。
    pub(crate) fn is_disabled(&self, unit: &crate::UnitInstance, name: &str) -> bool {
        !name.is_empty()
            && (self.is_script_var_defined(&format!("Disable({name})"))
                || self.is_script_var_defined(&format!("Disable({},{name})", unit.unit_data_name)))
    }

    /// 射程内かつ発射可能 (残弾/EN/気力) な最高威力武器を選ぶ。`combat::best_weapon_in_range`
    /// の実行時資源を考慮する版 (反撃 / 援護 / AI 自動選択用)。チャージ武器は除外。
    pub(super) fn best_firable_weapon_in_range(
        &self,
        unit_idx: usize,
        unit: &crate::data::unit::UnitData,
        dist: u32,
    ) -> Option<crate::data::unit::WeaponData> {
        unit.weapons
            .iter()
            .filter(|w| combat::weapon_in_range(w, dist))
            .filter(|w| !combat::is_charge_weapon(w))
            .filter(|w| self.weapon_firable(unit_idx, w))
            .max_by_key(|w| w.power)
            .cloned()
    }

    /// 武器発射時の資源消費 (EN・残弾)。`Unit.cs` 同様、命中の有無に関わらず発射で消費する。
    /// `weapon_di` は `unit_data.weapons` でのインデックス (= `UnitWeapon.weapon_index`)。
    /// EN コスト 0・無限弾 (bullet ≤ 0) の武器は実質無消費 (= テスト用武器へ無影響)。
    pub(crate) fn consume_weapon_resources(&mut self, unit_idx: usize, weapon_di: usize) {
        let (en_cost, bullet, name) = {
            let u = &self.database.unit_instances[unit_idx];
            match self
                .database
                .unit_by_name(&u.unit_data_name)
                .and_then(|d| d.weapons.get(weapon_di))
            {
                Some(wd) => (wd.en_consumption, wd.bullet, wd.name.clone()),
                None => return,
            }
        };
        let u = &mut self.database.unit_instances[unit_idx];
        if en_cost > 0 {
            u.en_consumed += en_cost;
        }
        if bullet > 0 {
            // 残弾の実行時状態は初めて撃ったときに満タンで作る (原典はユニット生成時に
            // 全武器の残弾を最大弾数で持つ)。
            let pos = match u.weapons.iter().position(|w| w.weapon_index == weapon_di) {
                Some(p) => p,
                None => {
                    u.weapons
                        .push(crate::UnitWeapon::from_data(name, weapon_di, bullet));
                    u.weapons.len() - 1
                }
            };
            u.weapons[pos].consume_bullet();
        }
    }

    /// 命中/損傷/失敗/回避 時の気力増加スキルを適用する (`Unit.cs` 17078-17138)。
    /// `hit=true`: 攻撃側に 命中時気力増加 / 被弾側に 損傷時気力増加 をスキルLv分加算。
    /// `hit=false`: 攻撃側に 失敗時気力増加 / 回避側に 回避時気力増加 を加算。テスト用
    /// ユニットは該当スキルを持たないため無影響 (= 既存テストへ影響なし)。
    pub(super) fn apply_combat_morale_skills(&mut self, atk_idx: usize, def_idx: usize, hit: bool) {
        let (atk_skill, def_skill) = if hit {
            ("命中時気力増加", "損傷時気力増加")
        } else {
            ("失敗時気力増加", "回避時気力増加")
        };
        let atk_lv = self.unit_pilot_skill_level(atk_idx, atk_skill);
        if atk_lv > 0 {
            let m = &mut self.database.unit_instances[atk_idx];
            m.morale = (m.morale + atk_lv).clamp(0, 150);
        }
        let def_lv = self.unit_pilot_skill_level(def_idx, def_skill);
        if def_lv > 0 {
            let m = &mut self.database.unit_instances[def_idx];
            m.morale = (m.morale + def_lv).clamp(0, 150);
        }
    }

    pub(super) fn award_kill_rewards(
        &mut self,
        killer_idx: usize,
        exp: i32,
        victim_value: i64,
        victim_party: crate::Party,
    ) -> i64 {
        // 撃墜による気力変動 (撃破者 +4 / 同陣営 +1、被撃破陣営は性格別、機械は不動)。
        self.apply_kill_morale(killer_idx, victim_party);
        // 撃墜数 +1 (撃破者メインパイロット)。撃墜数Lv* の必要技能ゲートが読む。
        self.increment_kill_count(killer_idx);
        // 経験値: UnitInstance.total_exp + メインパイロットの PilotInstance 成長。
        let old_level = crate::pilot_instance::level_from_exp(
            self.database.unit_instances[killer_idx].total_exp,
        );
        self.database.unit_instances[killer_idx].total_exp += exp;
        let new_level = crate::pilot_instance::level_from_exp(
            self.database.unit_instances[killer_idx].total_exp,
        );
        self.database.unit_instances[killer_idx].remove_condition("努力");
        let pilot_ids = self.database.unit_instances[killer_idx].pilot_ids.clone();
        for pilot_id in &pilot_ids {
            let pdata = self
                .database
                .pilot_instance_by_id(pilot_id)
                .map(|p| p.pilot_data_name.clone())
                .and_then(|n| self.database.pilot_by_name(&n).cloned());
            if let Some(pdata) = pdata {
                if let Some(pi) = self.database.pilot_instance_by_id_mut(pilot_id) {
                    if pi.add_exp(exp) {
                        pi.apply_stat_growth(&pdata);
                    }
                }
            }
        }
        // 資金: 撃破側が味方 (Player) のときのみ獲得する。敵/中立 の撃破で味方の
        // 資金プールが増えるのは誤り (実機報告: 敵が味方を撃破して「資金 +N」)。
        // victim_value/2 を基準に 獲得資金増加 (+10%/Lv) と 幸運 (×2, 消費) を適用。
        let mut money = if self.database.unit_instances[killer_idx].party == crate::Party::Player {
            (victim_value / 2).max(0)
        } else {
            0
        };
        let gain_lv = pilot_ids
            .iter()
            .filter_map(|id| self.database.pilot_instance_by_id(id))
            .map(|p| p.skill_level("獲得資金増加"))
            .max()
            .unwrap_or(0);
        if gain_lv > 0 {
            money += money * i64::from(gain_lv) / 10;
        }
        if self.database.unit_instances[killer_idx].has_condition("幸運") {
            money *= 2;
            self.database.unit_instances[killer_idx].remove_condition("幸運");
        }
        if money > 0 {
            self.add_money(money);
        }
        // レベルアップイベント (メインパイロットのレベルが上がった場合のみ発火)。
        if new_level > old_level {
            let (pn, un, party) = {
                let u = &self.database.unit_instances[killer_idx];
                (u.pilot_name.clone(), u.unit_data_name.clone(), u.party)
            };
            crate::event_runtime::fire_unit_event_labels_public(
                self,
                &["レベルアップ", "LevelUp"],
                &pn,
                &un,
                party,
            );
        }
        money
    }

    /// 撃破以外の行動 (修理 / 補給 等) の経験値付与。`total_exp` 加算 + メイン
    /// パイロット成長 + レベルアップイベント (資金は無し)。
    pub(super) fn award_support_exp(&mut self, unit_idx: usize, exp: i32) {
        if exp <= 0 {
            return;
        }
        let old_level =
            crate::pilot_instance::level_from_exp(self.database.unit_instances[unit_idx].total_exp);
        self.database.unit_instances[unit_idx].total_exp += exp;
        let new_level =
            crate::pilot_instance::level_from_exp(self.database.unit_instances[unit_idx].total_exp);
        let pilot_ids = self.database.unit_instances[unit_idx].pilot_ids.clone();
        for pilot_id in &pilot_ids {
            let pdata = self
                .database
                .pilot_instance_by_id(pilot_id)
                .map(|p| p.pilot_data_name.clone())
                .and_then(|n| self.database.pilot_by_name(&n).cloned());
            if let Some(pdata) = pdata {
                if let Some(pi) = self.database.pilot_instance_by_id_mut(pilot_id) {
                    if pi.add_exp(exp) {
                        pi.apply_stat_growth(&pdata);
                    }
                }
            }
        }
        if new_level > old_level {
            let (pn, un, party) = {
                let u = &self.database.unit_instances[unit_idx];
                (u.pilot_name.clone(), u.unit_data_name.clone(), u.party)
            };
            crate::event_runtime::fire_unit_event_labels_public(
                self,
                &["レベルアップ", "LevelUp"],
                &pn,
                &un,
                party,
            );
        }
    }
}
