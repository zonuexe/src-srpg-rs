//! ステータス詳細画面のビューモデル組み立て。

use super::*;

impl App {
    /// `Scene::UnitDetail` で表示する単機ステータスのビューモデルを構築する。
    /// `roster_pos` は味方ロスター (`status_roster`) 内の 0 始まり位置。範囲外 /
    /// ロスター空なら `None`。改造段階・装備・レベル成長・状態異常込みの実効値を
    /// 反映する (`GameDatabase::effective_*` / `effective_pilot_data`)。
    pub fn build_status_detail(
        &self,
        roster_pos: usize,
    ) -> Option<crate::scene::unit_detail::StatusDetail> {
        use crate::scene::unit_detail::{StatusDetail, WeaponRow};

        let roster = self.status_roster();
        let total = roster.len();
        let &unit_idx = roster.get(roster_pos)?;
        let u = self.database.unit_instances.get(unit_idx)?;
        let unit_data = self.database.unit_by_name(&u.unit_data_name)?;

        let hp_max = self.database.effective_max_hp(u);
        let hp_cur = (hp_max - u.damage).max(0);
        let en_max = self.database.effective_max_en(u);
        let en_cur = (en_max - u.en_consumed).max(0);

        // 搭乗パイロット (メイン)。無人なら has_pilot=false。
        let main_pilot = u.main_pilot_name();
        let pilot_data = if main_pilot.is_empty() {
            None
        } else {
            self.database.effective_pilot_data(main_pilot)
        };
        let pilot_inst = u
            .pilot_ids
            .first()
            .and_then(|id| self.database.pilot_instance_by_id(id));

        // 武器一覧 (runtime 状態を反映: 残弾 / EN)。
        let weapons: Vec<WeaponRow> = u
            .weapons
            .iter()
            .filter_map(|uw| {
                let wd = unit_data.weapons.get(uw.weapon_index)?;
                let range = if wd.min_range == wd.max_range {
                    wd.max_range.to_string()
                } else {
                    format!("{}-{}", wd.min_range, wd.max_range)
                };
                let ammo = if wd.bullet > 0 {
                    format!("残{}/{}", uw.bullet_remaining.max(0), wd.bullet)
                } else if wd.en_consumption > 0 {
                    format!("EN{}", wd.en_consumption)
                } else {
                    "-".to_string()
                };
                // 必要技能 / 必要条件のゲートのみで使用可否を判定する (has_acted / EN / 残弾
                // 等の一時的状態は閲覧画面では除外し、技能による恒久的な解禁状況のみ示す)。
                let ns = wd.necessary_skill();
                let nc = wd.necessary_condition();
                let usable = (ns.is_empty()
                    || crate::necessary_skill::is_satisfied(ns, u, &self.database))
                    && (nc.is_empty()
                        || crate::necessary_skill::is_satisfied(nc, u, &self.database));
                Some(WeaponRow {
                    name: wd.name.clone(),
                    power: wd.power,
                    range,
                    ammo,
                    usable,
                })
            })
            .collect();

        let conditions: Vec<String> = u.conditions.iter().map(|c| c.name.clone()).collect();

        // パイロット項目。
        let (
            has_pilot,
            pilot_name,
            pilot_nickname,
            level,
            exp,
            sp_cur,
            sp_max,
            infight,
            shooting,
            hit,
            dodge,
            intuition,
            technique,
            pilot_adaption,
            spirit_commands,
            skills,
        ) = if let Some(pd) = pilot_data.as_ref() {
            let level = pilot_inst.map(|p| p.level).unwrap_or(1);
            let exp = pilot_inst.map(|p| p.total_exp).unwrap_or(u.total_exp);
            let sp_max = pd.sp.unwrap_or(0);
            let sp_cur = pilot_inst.map(|p| p.sp_remaining).unwrap_or(sp_max);
            let spirit_commands: Vec<String> = pd
                .spirit_commands
                .iter()
                .map(|sc| format!("{} Lv{}", sc.name, sc.level))
                .collect();
            // 特殊技能: runtime PilotInstance.skills を優先し、無ければ静的特殊能力名。
            let skills: Vec<String> = if let Some(pi) = pilot_inst {
                if !pi.skills.is_empty() {
                    pi.skills.clone()
                } else {
                    pd.features.iter().map(|(n, _)| n.clone()).collect()
                }
            } else {
                pd.features.iter().map(|(n, _)| n.clone()).collect()
            };
            (
                true,
                pd.name.clone(),
                pd.nickname.clone(),
                level,
                exp,
                sp_cur,
                sp_max,
                pd.infight,
                pd.shooting,
                pd.hit,
                pd.dodge,
                pd.intuition,
                pd.technique,
                pd.adaption.as_str().to_string(),
                spirit_commands,
                skills,
            )
        } else {
            (
                false,
                String::new(),
                String::new(),
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                String::new(),
                Vec::new(),
                Vec::new(),
            )
        };

        Some(StatusDetail {
            index: roster_pos,
            total,
            unit_name: unit_data.name.clone(),
            unit_nickname: unit_data.nickname.clone(),
            party_label: u.party.short_label().to_string(),
            class: u
                .class_override
                .clone()
                .unwrap_or_else(|| unit_data.class.clone()),
            size: unit_data.size.label().to_string(),
            unit_adaption: unit_data.adaption.as_str().to_string(),
            hp_cur,
            hp_max,
            en_cur,
            en_max,
            armor: self.database.effective_armor(u),
            mobility: self.database.effective_mobility(u),
            speed: self.database.effective_speed(u),
            upgrade_level: u.upgrade_level,
            unit_morale: u.morale,
            conditions,
            has_pilot,
            pilot_name,
            pilot_nickname,
            level,
            exp,
            sp_cur,
            sp_max,
            infight,
            shooting,
            hit,
            dodge,
            intuition,
            technique,
            pilot_adaption,
            spirit_commands,
            skills,
            weapons,
        })
    }
}
