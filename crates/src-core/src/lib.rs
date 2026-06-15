//! src-core — platform-agnostic engine core for the SRC SRPG port.
//!
//! VB6 製の元コード（`SRC_20121125/` 配下）から段階的に移植する予定。
//! Windows GUI 非依存のロジックをこのクレートに集約し、`src-web` から呼び出す。
//!
//! Originally a VB6 program (see `SRC_20121125/`). This crate will hold the
//! platform-independent engine logic so that `src-web` (or future native
//! frontends) can drive it.

#![forbid(unsafe_code)]

pub mod app;
pub mod asset;
pub mod audio;
pub mod battle_anim;
pub mod combat;
pub mod command_catalog;
pub mod command_menu;
pub mod condition;
pub mod data;
pub mod db;
pub mod dialog;
pub mod entrypoint;
pub mod event_runtime;
pub mod expression;
pub mod feature;
pub mod flow;
pub mod item_slot;
pub mod modal;
pub mod movement;
pub mod necessary_skill;
pub mod pilot_instance;
pub mod scene;
pub mod script_overlay;
pub mod settings;
pub mod stage;
pub mod test_harness;
pub mod time_util;
pub mod turn;
pub mod ui;
pub mod unit_ability;
pub mod unit_instance;
pub mod unit_weapon;

pub use app::{App, Direction, Input, IntermissionCommandEntry};
pub use audio::AudioRequest;
pub use battle_anim::{AttackKind, BattleAnim, MoveAnim};
pub use command_menu::{ActionMode, CommandMenu, MapAction, UnitAction, UnitMenuItem};
pub use condition::{Condition, ConditionEffect};
pub use db::GameDatabase;
pub use dialog::PendingDialog;
pub use feature::{feature_value, has_feature, ActiveFeature};
pub use item_slot::{ItemSlot, SlotType};
pub use pilot_instance::PilotInstance;
pub use scene::Scene;
pub use script_overlay::{DrawCmd, ScriptOverlay};
pub use settings::Settings;
pub use stage::StageState;
pub use turn::{Phase, Turn};
pub use unit_ability::UnitAbility;
pub use unit_instance::{Party, UnitInstance};
pub use unit_weapon::UnitWeapon;

/// 描画キャンバスの論理サイズ。元 SRC の主要フォームを全て収めるため少し大きめ
/// に取る（Main.frm = 508x294, Configuration.frm = 346x405）。
/// Logical canvas size large enough to letterbox every VB6 form we port.
pub const CANVAS_WIDTH: u32 = 640;
pub const CANVAS_HEIGHT: u32 = 480;

/// 現在の移植進捗を識別する文字列。フロントエンドのスプラッシュに表示する。
/// Build banner shown by the frontend splash.
pub fn banner() -> &'static str {
    concat!("SRC (Rust port) v", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banner_includes_version() {
        assert!(banner().contains(env!("CARGO_PKG_VERSION")));
    }
}
