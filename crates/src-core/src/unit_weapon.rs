use serde::{Deserialize, Serialize};

/// Runtime weapon state for a unit instance.
/// References a static `WeaponData` from `GameDatabase` but tracks per-instance mutable state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitWeapon {
    /// References `WeaponData.name` in `GameDatabase::units[].weapons[]`.
    pub weapon_data_name: String,
    /// Index into the parent unit's `UnitData.weapons` vec for fast lookup.
    pub weapon_index: usize,
    /// Remaining bullets. -1 = unlimited. Copied from `WeaponData.bullet` on creation.
    pub bullet_remaining: i32,
    /// EN consumed by this weapon this battle (tracked separately from unit's total EN).
    pub en_consumed_this_battle: i32,
    /// Weapon is disabled by a condition or feature.
    #[serde(default)]
    pub is_disabled: bool,
}

impl UnitWeapon {
    /// Create a new runtime weapon from static data.
    pub fn from_data(
        weapon_data_name: impl Into<String>,
        weapon_index: usize,
        initial_bullet: i32,
    ) -> Self {
        Self {
            weapon_data_name: weapon_data_name.into(),
            weapon_index,
            bullet_remaining: initial_bullet,
            en_consumed_this_battle: 0,
            is_disabled: false,
        }
    }

    /// Check if this weapon has ammo remaining.
    pub fn has_ammo(&self) -> bool {
        self.bullet_remaining != 0
    }

    /// Consume one bullet. Returns false if no ammo.
    pub fn consume_bullet(&mut self) -> bool {
        if self.bullet_remaining < 0 {
            return true; // unlimited
        }
        if self.bullet_remaining <= 0 {
            return false;
        }
        self.bullet_remaining -= 1;
        true
    }

    /// Reset ammo to initial value (e.g., between battles).
    pub fn reset_ammo(&mut self, initial_bullet: i32) {
        self.bullet_remaining = initial_bullet;
        self.en_consumed_this_battle = 0;
        self.is_disabled = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unit_weapon_unlimited_ammo_never_runs_out() {
        let mut weapon = UnitWeapon::from_data("Laser", 0, -1);
        for _ in 0..100 {
            assert!(weapon.has_ammo());
            assert!(weapon.consume_bullet());
        }
    }

    #[test]
    fn unit_weapon_limited_ammo_runs_out() {
        let mut weapon = UnitWeapon::from_data("MachineGun", 0, 3);
        assert!(weapon.has_ammo());
        assert!(weapon.consume_bullet());
        assert!(weapon.has_ammo());
        assert!(weapon.consume_bullet());
        assert!(weapon.has_ammo());
        assert!(weapon.consume_bullet());
        assert!(!weapon.has_ammo());
        assert!(!weapon.consume_bullet());
    }

    #[test]
    fn unit_weapon_reset_restores_ammo() {
        let mut weapon = UnitWeapon::from_data("Cannon", 0, 5);
        assert!(weapon.has_ammo());
        weapon.consume_bullet();
        weapon.consume_bullet();
        weapon.consume_bullet();
        assert!(weapon.has_ammo()); // 2 remain after consuming 3 of 5
        assert_eq!(weapon.bullet_remaining, 2);
        weapon.reset_ammo(5);
        assert!(weapon.has_ammo());
        assert_eq!(weapon.bullet_remaining, 5);
        assert_eq!(weapon.en_consumed_this_battle, 0);
        assert!(!weapon.is_disabled);
    }
}
