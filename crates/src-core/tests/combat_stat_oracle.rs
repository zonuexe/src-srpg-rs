//! 戦闘 stat 計算のオラクル準拠テスト。
//!
//! 原典 SRC.Sharp `SRCCoreTests/Units/UnitAdaptionArmorTests.cs` 他で確認した
//! 地形適応倍率テーブルを、本実装 `combat::adaptation_mult` に突き合わせる。
//!
//! SRC `戦闘システム詳細.md` / `Unit.props.cs` の既定テーブル:
//!   S=1.4 / A=1.2 / B=1.0 / C=0.8 / D=0.6 / `-`=0.0
//! 本実装はこのテーブルと一致することを確認済 (この pin で回帰を防ぐ)。
//!
//! 注: 改造 (HP/装甲) と exp→level スケールは原典と乖離する (意図的再スケールの
//! 可能性。docs/CURRENT_WORK.md の差異レポート参照)。本ファイルでは一致が確認できた
//! 適応倍率のみを pin する。
//!
//! 著作権配慮: SRC オリジナルコードは含まない。値のみ移植。

use src_core::combat::adaptation_mult;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn adaptation_multiplier_default_table() {
    assert!(approx(adaptation_mult(b'S'), 1.4));
    assert!(approx(adaptation_mult(b'A'), 1.2));
    assert!(approx(adaptation_mult(b'B'), 1.0));
    assert!(approx(adaptation_mult(b'C'), 0.8));
    assert!(approx(adaptation_mult(b'D'), 0.6));
}

#[test]
fn adaptation_multiplier_dash_is_zero() {
    // `-` (適応なし) は 0.0 倍 = その地形で戦闘不可。
    assert!(approx(adaptation_mult(b'-'), 0.0));
}

#[test]
fn adaptation_multiplier_lowercase_matches_uppercase() {
    // 小文字も同値 (SRC データは大文字だが堅牢性のため)。
    assert!(approx(adaptation_mult(b's'), 1.4));
    assert!(approx(adaptation_mult(b'd'), 0.6));
}
