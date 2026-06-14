//! Runtime pilot instances — mutable state tied to a specific unit deployment.
//!
//! `PilotData` is static, parsed once from `pilot.txt`.  `PilotInstance` tracks
//! the runtime state of a pilot assigned to a unit: level, EXP, SP, morale,
//! current combat stats, skills, etc.

use serde::{Deserialize, Serialize};

/// Runtime state for a pilot assigned to a unit.
/// References static `PilotData` from `GameDatabase::pilots` but tracks mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PilotInstance {
    /// References `PilotData.name` in `GameDatabase::pilots`.
    pub pilot_data_name: String,
    /// Unique ID for this pilot instance (like `UnitInstance.uid`).
    #[serde(default)]
    pub id: String,
    /// Current level. Starts at 1.
    #[serde(default = "default_level")]
    pub level: i32,
    /// Total accumulated experience.
    #[serde(default)]
    pub total_exp: i32,
    /// Remaining SP (spirit points). Copied from `PilotData.sp` on creation.
    #[serde(default)]
    pub sp_remaining: i32,
    /// Current morale (0..=150). Starts at 100.
    #[serde(default = "default_morale")]
    pub morale: i32,
    /// Plana (extra resource for some scenarios).
    #[serde(default)]
    pub plana: i32,
    /// Combat stats modified by level, skills, items, conditions.
    /// Base values come from `PilotData`. These are recalculated by `update()`.
    #[serde(default)]
    pub infight: i32,
    #[serde(default)]
    pub shooting: i32,
    #[serde(default)]
    pub hit: i32,
    #[serde(default)]
    pub dodge: i32,
    #[serde(default)]
    pub intuition: i32,
    #[serde(default)]
    pub technique: i32,
    /// Active skill names (e.g., "格闘L3", "射撃L2").
    #[serde(default)]
    pub skills: Vec<String>,
    /// This is the main pilot of the unit (vs sub-pilot or support).
    #[serde(default = "default_true")]
    pub is_main_pilot: bool,
    /// This pilot is a support pilot (not main/sub).
    #[serde(default)]
    pub is_support: bool,
    /// Position among pilots on this unit (0 = main, 1+ = sub/support).
    #[serde(default)]
    pub pilot_index: i32,
    /// Fixed by `Fix` command — cannot be swapped in the intermission.
    /// Cleared by `Release` command.
    #[serde(default)]
    pub is_fixed: bool,
    /// `ChangePilotBitmap` コマンドで一時上書きされたビットマップ名。
    /// `None` はデフォルト画像。`Some("-")` は元の画像に戻した状態。
    #[serde(default)]
    pub bitmap_override: Option<String>,
}

fn default_level() -> i32 {
    1
}
fn default_morale() -> i32 {
    100
}
fn default_true() -> bool {
    true
}

impl PilotInstance {
    /// Create a new pilot instance from static data.
    pub fn from_data(
        pilot_data_name: impl Into<String>,
        id: impl Into<String>,
        pilot_data: &crate::data::pilot::PilotData,
    ) -> Self {
        // Parse skills from pilot data features
        // Skills are in features like: "格闘L3", "射撃L2", "命中L1", "回避L2", "技量L1", "反応L1"
        // Also: "SP消費減少", "SPアップ", "水上移動", "空中移動", "援護", etc.
        let mut skills = Vec::new();
        for (feat_name, _) in &pilot_data.features {
            let is_skill = feat_name.contains("L")
                || feat_name.contains("SP消費")
                || feat_name.contains("SPアップ")
                || feat_name.contains("水上移動")
                || feat_name.contains("空中移動")
                || feat_name.contains("援護")
                || feat_name.contains("底力")
                || feat_name.contains("エース")
                || feat_name.contains("見切り")
                || feat_name.contains("NEWTYPE")
                || feat_name.contains("強化人間");
            if is_skill && !skills.contains(feat_name) {
                skills.push(feat_name.clone());
            }
        }

        let mut inst = Self {
            pilot_data_name: pilot_data_name.into(),
            id: id.into(),
            level: 1,
            total_exp: 0,
            sp_remaining: pilot_data.sp.unwrap_or(0),
            morale: 100,
            plana: 0,
            infight: pilot_data.infight,
            shooting: pilot_data.shooting,
            hit: pilot_data.hit,
            dodge: pilot_data.dodge,
            intuition: pilot_data.intuition,
            technique: pilot_data.technique,
            skills,
            is_main_pilot: true,
            is_support: false,
            pilot_index: 0,
            is_fixed: false,
            bitmap_override: None,
        };
        inst.apply_stat_growth(pilot_data);
        inst
    }

    /// Add experience. Returns true if this caused a level-up (every 100 exp = 1 level).
    /// Level is capped at 99 (SRC.Sharp 準拠)。
    pub fn add_exp(&mut self, amount: i32) -> bool {
        const MAX_LEVEL: i32 = 99;
        let old_level = self.level;
        self.total_exp += amount;
        self.level = (1 + self.total_exp / 100).min(MAX_LEVEL);
        self.level > old_level
    }

    /// Apply stat growth on level up. Each level increases stats based on
    /// a simple growth formula: base_stat + level * growth_rate.
    /// Growth rates are derived from the pilot's class/personality.
    pub fn apply_stat_growth(&mut self, pilot_data: &crate::data::pilot::PilotData) {
        // Simple growth: each level adds a small amount to each stat
        // The growth rate varies by pilot class
        let growth_rate = match pilot_data.class.as_str() {
            "スーパー系" => 15, // super robot: higher growth
            "リアル系" => 12,   // real robot: moderate growth
            _ => 10,            // default
        };

        // Recalculate stats from base + growth
        self.infight = pilot_data.infight + (self.level - 1) * growth_rate;
        self.shooting = pilot_data.shooting + (self.level - 1) * growth_rate;
        self.hit = pilot_data.hit + (self.level - 1) * growth_rate / 2;
        self.dodge = pilot_data.dodge + (self.level - 1) * growth_rate / 2;
        self.intuition = pilot_data.intuition + (self.level - 1) * growth_rate / 3;
        self.technique = pilot_data.technique + (self.level - 1) * growth_rate / 3;
    }

    /// Consume SP for a special power. Returns false if insufficient SP.
    pub fn consume_sp(&mut self, amount: i32) -> bool {
        if self.sp_remaining < amount {
            return false;
        }
        self.sp_remaining -= amount;
        true
    }

    /// Recover SP.
    pub fn recover_sp(&mut self, amount: i32) {
        self.sp_remaining += amount;
    }

    /// Check if this pilot has a skill containing the given substring.
    pub fn has_skill(&self, skill_name: &str) -> bool {
        self.skills.iter().any(|s| s.contains(skill_name))
    }

    /// Get the level of a skill by name substring. Returns 0 if not found.
    pub fn skill_level(&self, skill_name: &str) -> i32 {
        for s in &self.skills {
            if s.contains(skill_name) {
                // Extract trailing number from skill name (e.g., "格闘L3" → 3)
                if let Some(pos) = s.rfind(|c: char| c.is_ascii_digit()) {
                    let num_start = s[..=pos]
                        .rfind(|c: char| !c.is_ascii_digit())
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    if let Ok(n) = s[num_start..=pos].parse::<i32>() {
                        return n;
                    }
                }
                return 1; // skill present but no level number
            }
        }
        0
    }

    /// Calculate the actual SP cost for a special power, considering skill reductions.
    /// "SP消費減少" skill reduces cost by 10 per level.
    pub fn sp_cost_for_power(&self, base_cost: i32) -> i32 {
        let reduction = self.skill_level("SP消費減少") * 10;
        base_cost.saturating_sub(reduction).max(0)
    }

    /// Try to consume SP for a special power. Returns false if insufficient SP.
    pub fn try_consume_sp(&mut self, base_cost: i32) -> bool {
        let actual_cost = self.sp_cost_for_power(base_cost);
        if self.sp_remaining < actual_cost {
            return false;
        }
        self.sp_remaining -= actual_cost;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_pilot_data() -> crate::data::pilot::PilotData {
        crate::data::pilot::PilotData {
            spirit_commands: Vec::new(),
            name: "テストパイロット".to_string(),
            nickname: "テスト".to_string(),
            kana_name: "てすと".to_string(),
            sex: crate::data::pilot::Sex::Male,
            class: "格闘家".to_string(),
            adaption: crate::data::pilot::Adaption([b'A', b'A', b'A', b'A']),
            exp_value: 100,
            infight: 150,
            shooting: 120,
            hit: 130,
            dodge: 110,
            intuition: 140,
            technique: 160,
            personality: Some("冷静".to_string()),
            sp: Some(50),
            bgm: None,
            bitmap: None,
            features: Vec::new(),
        }
    }

    #[test]
    fn pilot_instance_from_data_copies_base_stats() {
        let pdata = make_pilot_data();
        let inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);

        assert_eq!(inst.pilot_data_name, "テストパイロット");
        assert_eq!(inst.id, "p1");
        assert_eq!(inst.level, 1);
        assert_eq!(inst.total_exp, 0);
        assert_eq!(inst.sp_remaining, 50);
        assert_eq!(inst.morale, 100);
        assert_eq!(inst.infight, 150);
        assert_eq!(inst.shooting, 120);
        assert_eq!(inst.hit, 130);
        assert_eq!(inst.dodge, 110);
        assert_eq!(inst.intuition, 140);
        assert_eq!(inst.technique, 160);
        assert!(inst.is_main_pilot);
        assert!(!inst.is_support);
        assert_eq!(inst.pilot_index, 0);
    }

    #[test]
    fn pilot_instance_add_exp_triggers_level_up() {
        let pdata = make_pilot_data();
        let mut inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);

        assert_eq!(inst.level, 1);
        assert!(!inst.add_exp(50)); // not enough for level up
        assert_eq!(inst.level, 1);
        assert_eq!(inst.total_exp, 50);

        assert!(inst.add_exp(50)); // 100 total = level 2
        assert_eq!(inst.level, 2);
        assert_eq!(inst.total_exp, 100);

        // 200 total = level 3
        inst.add_exp(100);
        assert_eq!(inst.level, 3);
        assert_eq!(inst.total_exp, 200);
    }

    #[test]
    fn pilot_instance_consume_sp_checks_sufficient() {
        let pdata = make_pilot_data();
        let mut inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);

        assert_eq!(inst.sp_remaining, 50);
        assert!(inst.consume_sp(30)); // 50 >= 30, ok
        assert_eq!(inst.sp_remaining, 20);
        assert!(!inst.consume_sp(30)); // 20 < 30, fails
        assert_eq!(inst.sp_remaining, 20); // unchanged
    }

    #[test]
    fn pilot_instance_skill_level_parses_number() {
        let pdata = make_pilot_data();
        let mut inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);

        // No skills yet
        assert_eq!(inst.skill_level("格闘"), 0);

        inst.skills.push("格闘L3".to_string());
        assert_eq!(inst.skill_level("格闘"), 3);

        inst.skills.push("射撃L2".to_string());
        assert_eq!(inst.skill_level("射撃"), 2);
        assert_eq!(inst.skill_level("格闘"), 3); // still works

        // Skill without number returns 1
        inst.skills.push("底力".to_string());
        assert_eq!(inst.skill_level("底力"), 1);
    }

    #[test]
    fn pilot_instance_recover_sp() {
        let pdata = make_pilot_data();
        let mut inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);

        inst.consume_sp(30);
        assert_eq!(inst.sp_remaining, 20);
        inst.recover_sp(10);
        assert_eq!(inst.sp_remaining, 30);
    }

    #[test]
    fn pilot_instance_has_skill() {
        let pdata = make_pilot_data();
        let mut inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);

        assert!(!inst.has_skill("格闘"));
        inst.skills.push("格闘L3".to_string());
        assert!(inst.has_skill("格闘"));
        assert!(inst.has_skill("格闘L3"));
        assert!(!inst.has_skill("射撃"));
    }

    #[test]
    fn pilot_instance_from_data_parses_skills_from_features() {
        let mut pdata = make_pilot_data();
        pdata.features.push(("格闘L3".to_string(), String::new()));
        pdata.features.push(("射撃L2".to_string(), String::new()));
        pdata.features.push(("水上移動".to_string(), String::new()));

        let inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);

        assert!(inst.skills.contains(&"格闘L3".to_string()));
        assert!(inst.skills.contains(&"射撃L2".to_string()));
        assert!(inst.skills.contains(&"水上移動".to_string()));
        assert_eq!(inst.skills.len(), 3);
    }

    #[test]
    fn pilot_instance_skill_level_parses_number_from_name() {
        let mut pdata = make_pilot_data();
        pdata.features.push(("格闘L3".to_string(), String::new()));

        let inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);

        assert_eq!(inst.skill_level("格闘"), 3);
    }

    #[test]
    fn pilot_instance_has_skill_matches_substring() {
        let mut pdata = make_pilot_data();
        pdata.features.push(("格闘L3".to_string(), String::new()));

        let inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);

        assert!(inst.has_skill("格闘"));
    }

    #[test]
    fn sp_cost_reduced_by_skill() {
        let pdata = make_pilot_data();
        let mut inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);
        inst.skills.push("SP消費減少L2".to_string());
        assert_eq!(inst.sp_cost_for_power(50), 30);
    }

    #[test]
    fn sp_cost_cannot_go_negative() {
        let pdata = make_pilot_data();
        let mut inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);
        inst.skills.push("SP消費減少L10".to_string());
        assert_eq!(inst.sp_cost_for_power(50), 0);
    }

    #[test]
    fn try_consume_sp_fails_when_insufficient() {
        let pdata = make_pilot_data();
        let mut inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);
        inst.sp_remaining = 10;
        assert!(!inst.try_consume_sp(30));
        assert_eq!(inst.sp_remaining, 10);
    }

    #[test]
    fn level_up_increases_level() {
        let mut pdata = make_pilot_data();
        pdata.class = "格闘家".to_string();
        let mut inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);
        assert_eq!(inst.level, 1);
        inst.add_exp(100);
        assert_eq!(inst.level, 2);
    }

    #[test]
    fn exp_up_accumulates() {
        let mut pdata = make_pilot_data();
        pdata.class = "格闘家".to_string();
        let mut inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);
        inst.add_exp(50);
        inst.add_exp(50);
        assert_eq!(inst.total_exp, 100);
        assert_eq!(inst.level, 2);
    }

    #[test]
    fn stat_growth_increases_on_level_up() {
        let mut pdata = make_pilot_data();
        pdata.class = "格闘家".to_string();
        pdata.infight = 100;
        let inst = PilotInstance::from_data("テストパイロット", "p1", &pdata);
        assert_eq!(inst.infight, 100);
        let mut inst2 = inst.clone();
        inst2.level = 2;
        inst2.apply_stat_growth(&pdata);
        assert!(inst2.infight > inst.infight);
    }

    #[test]
    fn stat_growth_varies_by_class() {
        let mut pdata_super = make_pilot_data();
        pdata_super.class = "スーパー系".to_string();
        pdata_super.infight = 100;
        let mut inst_super = PilotInstance::from_data("テストパイロット", "p1", &pdata_super);
        inst_super.level = 2;
        inst_super.apply_stat_growth(&pdata_super);

        let mut pdata_real = make_pilot_data();
        pdata_real.class = "リアル系".to_string();
        pdata_real.infight = 100;
        let mut inst_real = PilotInstance::from_data("テストパイロット", "p2", &pdata_real);
        inst_real.level = 2;
        inst_real.apply_stat_growth(&pdata_real);

        assert!(inst_super.infight > inst_real.infight);
    }
}
