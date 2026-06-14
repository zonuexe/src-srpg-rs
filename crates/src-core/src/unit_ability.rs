//! Unit ability runtime state.
//!
//! Each `UnitInstance` holds a `Vec<UnitAbility>` that mirrors the
//! `UnitData.features` entries but tracks per-instance mutable state
//! (availability, EN consumption, etc.).

use serde::{Deserialize, Serialize};

/// Runtime ability state for a unit instance.
/// References a static ability/feature from `UnitData.features` but tracks per-instance mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitAbility {
    /// The ability/feature name (matches `UnitData.features[].0`).
    pub name: String,
    /// The ability's value/effect string (matches `UnitData.features[].1`).
    pub value: String,
    /// Whether this ability is currently available (may be disabled by conditions, EN shortage, etc.).
    #[serde(default = "default_true")]
    pub is_available: bool,
    /// EN consumed by this ability this battle.
    #[serde(default)]
    pub en_consumed: i32,
    /// 残り使用回数 (`SetStock` コマンドで設定)。`None` は無制限扱い。
    /// 元 SRC `Ability.Stock` に対応。
    #[serde(default)]
    pub stock_remaining: Option<i32>,
}

fn default_true() -> bool {
    true
}

impl UnitAbility {
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            is_available: true,
            en_consumed: 0,
            stock_remaining: None,
        }
    }

    /// Check if this ability can be used (available and not disabled).
    pub fn can_use(&self) -> bool {
        self.is_available
    }

    /// Reset ability state (e.g., between battles).
    pub fn reset(&mut self) {
        self.is_available = true;
        self.en_consumed = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_ability_default_available() {
        let ability = UnitAbility::new("回避", "30");
        assert!(ability.is_available);
        assert!(ability.can_use());
    }

    #[test]
    fn unit_ability_can_use_checks_available() {
        let mut ability = UnitAbility::new("援護", "防御力+20%");
        assert!(ability.can_use());
        ability.is_available = false;
        assert!(!ability.can_use());
    }

    #[test]
    fn unit_ability_reset_restores_availability() {
        let mut ability = UnitAbility::new("集中力", "命中率+15%");
        ability.is_available = false;
        ability.en_consumed = 5;
        assert!(!ability.can_use());
        ability.reset();
        assert!(ability.is_available);
        assert_eq!(ability.en_consumed, 0);
        assert!(ability.can_use());
    }
}
