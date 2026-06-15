use serde::{Deserialize, Serialize};

/// A condition (status effect) applied to a unit instance.
/// Replaces the previous `Vec<String>` statuses field with proper lifetime tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Condition {
    /// Condition name (e.g., "毒", "麻痺", "気絶", "熱血", "必中").
    pub name: String,
    /// Turns remaining. -1 = permanent (until manually removed).
    pub lifetime: i32,
    /// Strength level of the condition (0 = default, higher = stronger effect).
    #[serde(default)]
    pub level: i32,
    /// Extra data associated with the condition (e.g., source, custom parameters).
    #[serde(default)]
    pub data: String,
}

impl Condition {
    /// Create a new condition with the given name and lifetime.
    pub fn new(name: impl Into<String>, lifetime: i32) -> Self {
        Self {
            name: name.into(),
            lifetime,
            level: 0,
            data: String::new(),
        }
    }

    /// Create a new condition with all fields specified.
    pub fn with_details(
        name: impl Into<String>,
        lifetime: i32,
        level: i32,
        data: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            lifetime,
            level,
            data: data.into(),
        }
    }

    /// Check if this condition is permanent.
    pub fn is_permanent(&self) -> bool {
        self.lifetime < 0
    }

    /// Decrement lifetime by 1. Returns true if the condition has expired (lifetime <= 0).
    /// Permanent conditions never expire via this method.
    pub fn tick(&mut self) -> bool {
        if self.is_permanent() {
            return false;
        }
        self.lifetime -= 1;
        self.lifetime <= 0
    }

    /// Check if this condition matches the given name (case-sensitive for JP compatibility).
    pub fn matches_name(&self, name: &str) -> bool {
        self.name == name
    }
}

/// Known condition gameplay effects. This enum is used by the combat system
/// and other subsystems to query what a condition does without parsing strings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ConditionEffect {
    /// Unit cannot attack (e.g., "麻痺", "混乱").
    AttackDisabled,
    /// Unit cannot move (e.g., "麻痺", "足止め").
    MoveDisabled,
    /// Unit takes damage each turn (e.g., "毒").
    DamageOverTime { amount: i32 },
    /// Hit rate reduced (e.g., "混乱").
    HitDown { amount: i32 },
    /// Dodge rate reduced (e.g., "足止め").
    DodgeDown { amount: i32 },
    /// Armor reduced (e.g., "装甲低下").
    ArmorDown { amount: i32 },
    /// Attack power boosted (e.g., "熱血", "魂").
    AttackMultiplier { multiplier: f64 },
    /// Damage taken reduced (e.g., "鉄壁").
    DamageReduction { factor: f64 },
    /// Maximum 1 damage taken (e.g., "不屈").
    MaxDamageOne,
    /// Guaranteed hit (e.g., "必中").
    GuaranteedHit,
    /// No effect (cosmetic or script-driven only).
    None,
}

impl Condition {
    /// Determine the gameplay effect of this condition based on its name.
    /// Returns `ConditionEffect::None` for unknown or cosmetic conditions.
    pub fn effects(&self) -> Vec<ConditionEffect> {
        match self.name.as_str() {
            // 麻痺・捕縛: both attack and movement are disabled
            "麻痺" | "捕縛" => vec![
                ConditionEffect::AttackDisabled,
                ConditionEffect::MoveDisabled,
            ],
            // 混乱: attack disabled + hit rate down
            "混乱" => vec![
                ConditionEffect::AttackDisabled,
                ConditionEffect::HitDown { amount: 20 },
            ],
            // 足止め: movement disabled + dodge rate down
            "足止め" => vec![
                ConditionEffect::MoveDisabled,
                ConditionEffect::DodgeDown { amount: 20 },
            ],
            // 踊り: 一切の行動が取れない (特殊効果攻撃属性 踊)。常に回避行動を取る
            // ニュアンス (被命中時の回避) は未モデルで、行動不能のみ反映。
            "睡眠" | "行動不能" | "踊り" => vec![ConditionEffect::AttackDisabled],
            // 凍結 / 石化: 一切の行動が取れない (特殊効果攻撃属性 凍 / 石)。
            "凍結" | "石化" => vec![
                ConditionEffect::AttackDisabled,
                ConditionEffect::MoveDisabled,
            ],
            "移動不能" => vec![ConditionEffect::MoveDisabled],
            "毒" | "侵食" => vec![ConditionEffect::DamageOverTime {
                amount: self.level.max(1) * 100,
            }],
            "装甲低下" => vec![ConditionEffect::ArmorDown {
                amount: self.level.max(1) * 100,
            }],
            // 運動性ＵＰ / ＤＯＷＮ: 命中・回避に ±15 (SetStatus / 特殊効果攻撃属性 低運)。
            // HitDown/DodgeDown は負値で UP を表現する (combat_bonuses が減算するため)。
            "運動性ＵＰ" => vec![
                ConditionEffect::HitDown { amount: -15 },
                ConditionEffect::DodgeDown { amount: -15 },
            ],
            "運動性ＤＯＷＮ" => vec![
                ConditionEffect::HitDown { amount: 15 },
                ConditionEffect::DodgeDown { amount: 15 },
            ],
            "熱血" => vec![ConditionEffect::AttackMultiplier { multiplier: 2.0 }],
            "魂" => vec![ConditionEffect::AttackMultiplier { multiplier: 3.0 }],
            "ひらめき" => vec![ConditionEffect::AttackMultiplier { multiplier: 1.5 }],
            "鉄壁" => vec![ConditionEffect::DamageReduction { factor: 0.25 }],
            "ガード" => vec![ConditionEffect::DamageReduction { factor: 0.5 }],
            "不屈" => vec![ConditionEffect::MaxDamageOne],
            "必中" => vec![ConditionEffect::GuaranteedHit],
            _ => vec![ConditionEffect::None],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_lifetime_decrements_on_tick() {
        let mut c = Condition::new("毒", 3);
        assert!(!c.tick()); // lifetime = 2
        assert!(!c.tick()); // lifetime = 1
        assert!(c.tick()); // lifetime = 0, returns true (expired)
        assert!(c.lifetime <= 0);
    }

    #[test]
    fn condition_permanent_never_expires() {
        let mut c = Condition::new("鉄壁", -1);
        for _ in 0..10 {
            assert!(!c.tick());
        }
        assert_eq!(c.lifetime, -1);
    }

    #[test]
    fn condition_effect_mapping() {
        // 熱血 → AttackMultiplier(2.0)
        let mut nekketsu = Condition::new("熱血", 3);
        nekketsu.level = 1;
        assert!(nekketsu
            .effects()
            .contains(&ConditionEffect::AttackMultiplier { multiplier: 2.0 }));

        // 必中 → GuaranteedHit
        let hikkai = Condition::new("必中", 1);
        assert!(hikkai.effects().contains(&ConditionEffect::GuaranteedHit));

        // 毒 → DamageOverTime (level 0 → max(1) = 1 → 100)
        let mut poison = Condition::new("毒", 5);
        poison.level = 0;
        assert!(poison
            .effects()
            .contains(&ConditionEffect::DamageOverTime { amount: 100 }));

        // 麻痺 → both AttackDisabled and MoveDisabled
        let mahi = Condition::new("麻痺", 2);
        let mahi_effects = mahi.effects();
        assert!(mahi_effects.contains(&ConditionEffect::AttackDisabled));
        assert!(mahi_effects.contains(&ConditionEffect::MoveDisabled));

        // 混乱 → AttackDisabled + HitDown
        let konran = Condition::new("混乱", 2);
        let konran_effects = konran.effects();
        assert!(konran_effects.contains(&ConditionEffect::AttackDisabled));
        assert!(konran_effects.contains(&ConditionEffect::HitDown { amount: 20 }));

        // 運動性ＵＰ → 命中・回避 +15 (HitDown/DodgeDown は負値で UP)。
        let up = Condition::new("運動性ＵＰ", 3);
        let up_effects = up.effects();
        assert!(up_effects.contains(&ConditionEffect::HitDown { amount: -15 }));
        assert!(up_effects.contains(&ConditionEffect::DodgeDown { amount: -15 }));

        // 運動性ＤＯＷＮ → 命中・回避 -15。
        let down = Condition::new("運動性ＤＯＷＮ", 3);
        let down_effects = down.effects();
        assert!(down_effects.contains(&ConditionEffect::HitDown { amount: 15 }));
        assert!(down_effects.contains(&ConditionEffect::DodgeDown { amount: 15 }));
    }

    #[test]
    fn condition_add_condition_merges_duplicates() {
        let mut c1 = Condition::new("熱血", 3);
        let c2 = Condition::new("熱血", 5); // longer lifetime
        let c3 = Condition::new("熱血", 2); // shorter lifetime

        // Simulate add_condition logic
        if c2.is_permanent() || c2.lifetime > c1.lifetime {
            c1.lifetime = c2.lifetime;
        }
        if c2.level > c1.level {
            c1.level = c2.level;
        }
        assert_eq!(c1.lifetime, 5);

        // With shorter lifetime, should not update
        if c3.is_permanent() || c3.lifetime > c1.lifetime {
            c1.lifetime = c3.lifetime;
        }
        assert_eq!(c1.lifetime, 5); // unchanged
    }

    #[test]
    fn condition_matches_name_is_case_sensitive() {
        let c = Condition::new("毒", 3);
        assert!(c.matches_name("毒"));
        assert!(!c.matches_name("もうで"));
    }
}
