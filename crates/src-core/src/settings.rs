//! 設定変更ダイアログ (`frmConfiguration`) で扱う設定値。
//! Settings exposed via the original `frmConfiguration` map command dialog.
//!
//! 元 `Configuration.frm` の各コントロールが書き込む設定項目を Rust 側で集約する。
//! 持続化 / 読み書きの実装は今のところ未定。デフォルト値は VB6 原典の挙動に
//! 寄せている。

use serde::{Deserialize, Serialize};

/// メッセージ送り速度 / Message scroll speed.
///
/// 元 `cboMessageSpeed` の選択肢。VB6 では文字列として保存されるが、ここでは
/// 型安全な enum で表現する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MessageSpeed {
    VeryFast,
    Fast,
    #[default]
    Normal,
    Slow,
    VerySlow,
}

impl MessageSpeed {
    pub fn label(self) -> &'static str {
        match self {
            Self::VeryFast => "最速",
            Self::Fast => "速い",
            Self::Normal => "普通",
            Self::Slow => "遅い",
            Self::VerySlow => "最遅",
        }
    }

    /// 次の選択肢へ循環。元コンボボックス上で下キーを押した動作に相当。
    pub const fn next(self) -> Self {
        match self {
            Self::VeryFast => Self::Fast,
            Self::Fast => Self::Normal,
            Self::Normal => Self::Slow,
            Self::Slow => Self::VerySlow,
            Self::VerySlow => Self::VeryFast,
        }
    }
}

/// MIDI リセットモード / MIDI reset mode.
///
/// 元 `cboMidiReset` の選択肢。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum MidiResetMode {
    None,
    #[default]
    Gm,
    Gs,
    Xg,
}

impl MidiResetMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "なし",
            Self::Gm => "GM",
            Self::Gs => "GS",
            Self::Xg => "XG",
        }
    }

    pub const fn next(self) -> Self {
        match self {
            Self::None => Self::Gm,
            Self::Gm => Self::Gs,
            Self::Gs => Self::Xg,
            Self::Xg => Self::None,
        }
    }
}

/// 設定変更ダイアログで保持する値の集合。
/// All values displayed/edited by the Configuration dialog.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    /// 元: `chkBattleAnimation`
    pub battle_animation: bool,
    /// 元: `chkExtendedAnimation`
    pub extended_animation: bool,
    /// 元: `chkWeaponAnimation`
    pub weapon_animation: bool,
    /// 元: `chkSpecialPowerAnimation`
    pub special_power_animation: bool,
    /// 元: `chkMoveAnimation`
    pub move_animation: bool,
    /// 元: `chkAutoMoveCursor`
    pub auto_move_cursor: bool,
    /// 元: `chkShowSquareLine`
    pub show_square_line: bool,
    /// 元: `chkShowTurn`
    pub show_turn: bool,
    /// 元: `chkKeepEnemyBGM`
    pub keep_enemy_bgm: bool,
    /// 元: `chkUseDirectMusic`
    pub use_direct_music: bool,
    /// 元: `cboMessageSpeed`
    pub message_speed: MessageSpeed,
    /// 元: `cboMidiReset`
    pub midi_reset: MidiResetMode,
    /// 元: `hscMP3Volume` (0..=100)
    pub mp3_volume: u8,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            battle_animation: true,
            extended_animation: true,
            weapon_animation: true,
            special_power_animation: true,
            move_animation: true,
            auto_move_cursor: true,
            show_square_line: false,
            show_turn: true,
            keep_enemy_bgm: false,
            use_direct_music: false,
            message_speed: MessageSpeed::default(),
            midi_reset: MidiResetMode::default(),
            mp3_volume: 50,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_vb6_initial_values() {
        let s = Settings::default();
        assert!(s.battle_animation);
        assert!(!s.show_square_line);
        assert_eq!(s.mp3_volume, 50);
        assert_eq!(s.message_speed.label(), "普通");
        assert_eq!(s.midi_reset.label(), "GM");
    }
}
