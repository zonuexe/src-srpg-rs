//! 設定変更ダイアログのレイアウト / Configuration dialog layout.
//!
//! 元実装: `SRC_20121125/Configuration.frm` (`frmConfiguration`)。
//! 元 .frm の座標値は twips（1/20 pt）。ScaleMode 未指定なので default (1=Twip)。
//! ピクセル換算は `/ 15`（96 dpi 想定）。

use super::title::Rect;
use crate::Settings;

/// 元 `frmConfiguration` のクライアント領域（pixel）。
/// 5190 / 15 = 346, 6075 / 15 = 405
pub const CONFIG_WIDTH: u32 = 346;
pub const CONFIG_HEIGHT: u32 = 405;

/// ダイアログ上部に描画する VB6 風タイトルバーの高さ。
/// レイアウト座標はクライアント領域基準なので、ヒット判定時にこの分 Y を引く
/// （render 側も同値で描画する）。
pub const TITLE_BAR_HEIGHT: i32 = 18;

/// ラベル付き UI コントロール（チェックボックス・ボタン用）。
/// Single labelled control (checkbox / button).
#[derive(Debug, Clone, Copy)]
pub struct LabelledControl {
    pub bounds: Rect,
    pub caption: &'static str,
}

impl LabelledControl {
    pub const fn new(x: i32, y: i32, w: u32, h: u32, caption: &'static str) -> Self {
        Self {
            bounds: Rect::new(x, y, w, h),
            caption,
        }
    }
}

/// `frmConfiguration` の全コントロール配置。
/// Layout for every control on the original Configuration dialog.
#[derive(Debug, Clone, Copy)]
pub struct ConfigurationLayout {
    // CheckBoxes — フィールド名は元 VB6 識別子に対応。
    pub message_speed_label: LabelledControl,
    pub message_speed_combo: Rect,
    pub battle_animation: LabelledControl,
    pub extended_animation: LabelledControl,
    pub weapon_animation: LabelledControl,
    pub special_power_animation: LabelledControl,
    pub move_animation: LabelledControl,
    pub auto_move_cursor: LabelledControl,
    pub show_square_line: LabelledControl,
    pub show_turn: LabelledControl,
    pub keep_enemy_bgm: LabelledControl,
    pub use_direct_music: LabelledControl,
    pub midi_reset_label: LabelledControl,
    pub midi_reset_combo: Rect,
    pub mp3_volume_label: LabelledControl,
    pub mp3_volume_text: Rect,
    pub mp3_volume_scroll: Rect,
    pub ok: LabelledControl,
    pub cancel: LabelledControl,
}

impl ConfigurationLayout {
    /// 元 Configuration.frm から抽出した配置 (pixel 単位)。
    ///
    /// 縦並びの順序は TabIndex に倣う:
    /// 1. labMessageSpeed / cboMessageSpeed
    /// 2. chkBattleAnimation
    /// 3. chkExtendedAnimation
    /// 4. chkWeaponAnimation
    /// 5. chkSpecialPowerAnimation
    /// 6. chkMoveAnimation
    /// 7. chkAutoMoveCursor
    /// 8. chkShowSquareLine
    /// 9. chkShowTurn
    /// 10. chkKeepEnemyBGM
    /// 11. chkUseDirectMusic
    /// 12. labMidiReset / cboMidiReset
    /// 13. labMP3Volume / txtMP3Volume / hscMP3Volume
    /// 14. cmdOK / cmdCancel
    pub const fn original() -> Self {
        // すべて元 .frm の Left/Top/Width/Height (twips) / 15 でピクセル化。
        Self {
            // labMessageSpeed: L=490, T=295, W=1935, H=255  → 33, 20, 129, 17
            message_speed_label: LabelledControl::new(33, 20, 129, 17, "メッセージスピード"),
            // cboMessageSpeed: L=2160, T=240, W=2055, H=300 → 144, 16, 137, 20
            message_speed_combo: Rect::new(144, 16, 137, 20),

            // chkBattleAnimation: L=480, T=600, W=3735, H=375 → 32, 40, 249, 25
            battle_animation: LabelledControl::new(32, 40, 249, 25, "戦闘アニメを表示する"),
            // chkExtendedAnimation: L=720, T=960, W=3495, H=495 → 48, 64, 233, 33
            extended_animation: LabelledControl::new(
                48,
                64,
                233,
                33,
                "戦闘アニメの拡張機能を使用する",
            ),
            // chkWeaponAnimation: L=720, T=1440, W=3495, H=495 → 48, 96, 233, 33
            weapon_animation: LabelledControl::new(
                48,
                96,
                233,
                33,
                "武器準備アニメを自動選択表示する",
            ),
            // chkSpecialPowerAnimation: L=480, T=1920, W=3735, H=375 → 32, 128, 249, 25
            special_power_animation: LabelledControl::new(
                32,
                128,
                249,
                25,
                "スペシャルパワーアニメを表示する",
            ),
            // chkMoveAnimation: L=480, T=2280, W=3735, H=375 → 32, 152, 249, 25
            move_animation: LabelledControl::new(32, 152, 249, 25, "移動アニメを表示する"),
            // chkAutoMoveCursor: L=480, T=2640, W=3735, H=375 → 32, 176, 249, 25
            auto_move_cursor: LabelledControl::new(
                32,
                176,
                249,
                25,
                "マウスカーソルを自動的に移動する",
            ),
            // chkShowSquareLine: L=480, T=3000, W=3975, H=375 → 32, 200, 265, 25
            show_square_line: LabelledControl::new(32, 200, 265, 25, "マス目を表示する (要再起動)"),
            // chkShowTurn: L=480, T=3360, W=3735, H=375 → 32, 224, 249, 25
            show_turn: LabelledControl::new(
                32,
                224,
                249,
                25,
                "味方フェイズ開始時にターン表示を行う",
            ),
            // chkKeepEnemyBGM: L=480, T=3720, W=3735, H=375 → 32, 248, 249, 25
            keep_enemy_bgm: LabelledControl::new(
                32,
                248,
                249,
                25,
                "敵フェイズ中にＢＧＭを変更しない",
            ),
            // chkUseDirectMusic: L=480, T=4080, W=4215, H=375 → 32, 272, 281, 25
            use_direct_music: LabelledControl::new(
                32,
                272,
                281,
                25,
                "MIDI演奏にDirectMusicを使用する (要再起動)",
            ),

            // labMidiReset: L=495, T=4515, W=2880, H=255 → 33, 301, 192, 17
            midi_reset_label: LabelledControl::new(33, 301, 192, 17, "MIDI音源リセットの種類"),
            // cboMidiReset: L=2520, T=4440, W=1725, H=300 → 168, 296, 115, 20
            midi_reset_combo: Rect::new(168, 296, 115, 20),

            // labMP3Volume: L=495, T=4950, W=735, H=255 → 33, 330, 49, 17
            mp3_volume_label: LabelledControl::new(33, 330, 49, 17, "MP3音量"),
            // txtMP3Volume: L=1305, T=4905, W=495, H=285 → 87, 327, 33, 19
            mp3_volume_text: Rect::new(87, 327, 33, 19),
            // hscMP3Volume: L=1920, T=4920, W=2295, H=255 → 128, 328, 153, 17
            mp3_volume_scroll: Rect::new(128, 328, 153, 17),

            // cmdOK: L=1680, T=5400, W=1455, H=375 → 112, 360, 97, 25
            ok: LabelledControl::new(112, 360, 97, 25, "OK"),
            // cmdCancel: L=3240, T=5400, W=1455, H=375 → 216, 360, 97, 25
            cancel: LabelledControl::new(216, 360, 97, 25, "キャンセル"),
        }
    }

    /// レイアウト内の全コントロール矩形を順序付きで列挙（境界テスト用）。
    pub fn all_bounds(&self) -> impl Iterator<Item = Rect> + '_ {
        [
            self.message_speed_label.bounds,
            self.message_speed_combo,
            self.battle_animation.bounds,
            self.extended_animation.bounds,
            self.weapon_animation.bounds,
            self.special_power_animation.bounds,
            self.move_animation.bounds,
            self.auto_move_cursor.bounds,
            self.show_square_line.bounds,
            self.show_turn.bounds,
            self.keep_enemy_bgm.bounds,
            self.use_direct_music.bounds,
            self.midi_reset_label.bounds,
            self.midi_reset_combo,
            self.mp3_volume_label.bounds,
            self.mp3_volume_text,
            self.mp3_volume_scroll,
            self.ok.bounds,
            self.cancel.bounds,
        ]
        .into_iter()
    }
}

/// フォームのタイトルバーに表示するテキスト。
/// 元: `frmConfiguration.Caption`。
pub const CAPTION: &str = "設定変更";

// ===== ヒット判定 / Hit-testing =====

/// チェックボックスで操作する `Settings` のフィールド識別子。
/// Identifier for each checkbox-backed field of `Settings`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckboxField {
    BattleAnimation,
    ExtendedAnimation,
    WeaponAnimation,
    SpecialPowerAnimation,
    MoveAnimation,
    AutoMoveCursor,
    ShowSquareLine,
    ShowTurn,
    KeepEnemyBgm,
    UseDirectMusic,
}

impl CheckboxField {
    /// 対応する Settings のフィールドを反転する。
    /// Toggle the corresponding bool field in `Settings`.
    pub fn toggle(self, s: &mut Settings) {
        let field = match self {
            Self::BattleAnimation => &mut s.battle_animation,
            Self::ExtendedAnimation => &mut s.extended_animation,
            Self::WeaponAnimation => &mut s.weapon_animation,
            Self::SpecialPowerAnimation => &mut s.special_power_animation,
            Self::MoveAnimation => &mut s.move_animation,
            Self::AutoMoveCursor => &mut s.auto_move_cursor,
            Self::ShowSquareLine => &mut s.show_square_line,
            Self::ShowTurn => &mut s.show_turn,
            Self::KeepEnemyBgm => &mut s.keep_enemy_bgm,
            Self::UseDirectMusic => &mut s.use_direct_music,
        };
        *field = !*field;
    }
}

/// クリックでヒットした UI 要素 / What the user clicked on.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HitTarget {
    Checkbox(CheckboxField),
    /// メッセージスピードのコンボ — 次の選択肢へ循環。
    MessageSpeedCombo,
    /// MIDI リセット種別のコンボ — 次の選択肢へ循環。
    MidiResetCombo,
    /// MP3 音量スクロールバーの相対位置（0.0..=1.0）。
    Mp3VolumeBar {
        ratio: f64,
    },
    OkButton,
    CancelButton,
}

/// `(x, y)` はクライアント領域 (タイトルバー含む) 基準のシーン座標。
/// `(x, y)` is in scene coords relative to the dialog's outer top-left.
pub fn hit_test(layout: &ConfigurationLayout, x: i32, y: i32) -> Option<HitTarget> {
    // クライアント領域内 Y。タイトルバー上は捨てる。
    let cy = y - TITLE_BAR_HEIGHT;
    if cy < 0 {
        return None;
    }

    // チェックボックスは矩形全体（ボックス + ラベル両方）でヒット扱い。
    let checkboxes = [
        (
            layout.battle_animation.bounds,
            CheckboxField::BattleAnimation,
        ),
        (
            layout.extended_animation.bounds,
            CheckboxField::ExtendedAnimation,
        ),
        (
            layout.weapon_animation.bounds,
            CheckboxField::WeaponAnimation,
        ),
        (
            layout.special_power_animation.bounds,
            CheckboxField::SpecialPowerAnimation,
        ),
        (layout.move_animation.bounds, CheckboxField::MoveAnimation),
        (
            layout.auto_move_cursor.bounds,
            CheckboxField::AutoMoveCursor,
        ),
        (
            layout.show_square_line.bounds,
            CheckboxField::ShowSquareLine,
        ),
        (layout.show_turn.bounds, CheckboxField::ShowTurn),
        (layout.keep_enemy_bgm.bounds, CheckboxField::KeepEnemyBgm),
        (
            layout.use_direct_music.bounds,
            CheckboxField::UseDirectMusic,
        ),
    ];
    for (rect, field) in checkboxes {
        if rect_contains(rect, x, cy) {
            return Some(HitTarget::Checkbox(field));
        }
    }

    if rect_contains(layout.message_speed_combo, x, cy) {
        return Some(HitTarget::MessageSpeedCombo);
    }
    if rect_contains(layout.midi_reset_combo, x, cy) {
        return Some(HitTarget::MidiResetCombo);
    }
    if rect_contains(layout.mp3_volume_scroll, x, cy) {
        // 横スクロールバー内の x 比率を 0..=1 で返す
        let bar = layout.mp3_volume_scroll;
        let rel = (x - bar.x).clamp(0, bar.w as i32) as f64 / bar.w as f64;
        return Some(HitTarget::Mp3VolumeBar { ratio: rel });
    }
    if rect_contains(layout.ok.bounds, x, cy) {
        return Some(HitTarget::OkButton);
    }
    if rect_contains(layout.cancel.bounds, x, cy) {
        return Some(HitTarget::CancelButton);
    }
    None
}

fn rect_contains(r: Rect, x: i32, y: i32) -> bool {
    x >= r.x && x < r.x + r.w as i32 && y >= r.y && y < r.y + r.h as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_fits_in_form_bounds() {
        let l = ConfigurationLayout::original();
        for r in l.all_bounds() {
            assert!(r.x >= 0, "{:?}", r);
            assert!(r.y >= 0, "{:?}", r);
            assert!((r.x as u32) + r.w <= CONFIG_WIDTH, "{:?}", r);
            assert!((r.y as u32) + r.h <= CONFIG_HEIGHT, "{:?}", r);
        }
    }

    #[test]
    fn hit_test_finds_battle_animation_checkbox() {
        let l = ConfigurationLayout::original();
        let r = l.battle_animation.bounds;
        // タイトルバー分 Y をずらす
        let y = TITLE_BAR_HEIGHT + r.y + 5;
        let hit = hit_test(&l, r.x + 4, y);
        assert_eq!(
            hit,
            Some(HitTarget::Checkbox(CheckboxField::BattleAnimation))
        );
    }

    #[test]
    fn hit_test_outside_returns_none() {
        let l = ConfigurationLayout::original();
        assert_eq!(hit_test(&l, -1, -1), None);
        assert_eq!(hit_test(&l, 1000, 1000), None);
    }

    #[test]
    fn hit_test_ok_button() {
        let l = ConfigurationLayout::original();
        let r = l.ok.bounds;
        let hit = hit_test(&l, r.x + 10, TITLE_BAR_HEIGHT + r.y + 5);
        assert_eq!(hit, Some(HitTarget::OkButton));
    }

    #[test]
    fn checkbox_toggle_flips_settings_field() {
        let mut s = Settings::default();
        assert!(s.battle_animation);
        CheckboxField::BattleAnimation.toggle(&mut s);
        assert!(!s.battle_animation);
        CheckboxField::BattleAnimation.toggle(&mut s);
        assert!(s.battle_animation);
    }

    #[test]
    fn mp3_bar_returns_ratio() {
        let l = ConfigurationLayout::original();
        let r = l.mp3_volume_scroll;
        let hit = hit_test(&l, r.x + (r.w as i32) / 2, TITLE_BAR_HEIGHT + r.y + 2);
        match hit {
            Some(HitTarget::Mp3VolumeBar { ratio }) => {
                assert!((0.45..=0.55).contains(&ratio), "ratio={ratio}");
            }
            other => panic!("expected Mp3VolumeBar, got {:?}", other),
        }
    }
}
