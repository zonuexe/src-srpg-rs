//! Status display UI operations.

/// Status UI interface for showing unit/pilot details.
pub trait IGUIStatus: Send + Sync {
    /// Show unit status panel.
    #[allow(clippy::too_many_arguments)]
    fn show_unit_status(
        &self,
        unit_name: &str,
        pilot_name: &str,
        hp: i64,
        max_hp: i64,
        en: i32,
        max_en: i32,
        armor: i64,
        mobility: i32,
    );

    /// Hide unit status panel.
    fn hide_unit_status(&self);

    /// Show pilot status panel.
    fn show_pilot_status(&self, pilot_name: &str, level: i32, exp: i32, sp: i32, morale: i32);

    /// Hide pilot status panel.
    fn hide_pilot_status(&self);

    /// Show weapon list for a unit.
    fn show_weapon_list(&self, unit_name: &str, weapons: &[(&str, i64, i32, i32)]);

    /// Hide weapon list.
    fn hide_weapon_list(&self);
}
