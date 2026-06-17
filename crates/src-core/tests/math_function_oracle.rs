//! Math 関数のオラクル準拠テスト (Int / Round / RoundUp / RoundDown / Sqr /
//! Abs / Min / Max / 三角関数)。
//!
//! 突合の基準は **VB6 原典 `Expression.bas` (SRC_20121125)**。C# 移植
//! SRC.Sharp `SRCCoreTests/Expressions/MathFunction*` も参照したが、Round の
//! 負の半数で SRC.Sharp は `MidpointRounding.AwayFromZero` を使い VB6 と乖離する
//! (`Round(-2.5)`: VB6=-2 / SRC.Sharp=-3)。本実装は VB6 準拠 (-2) が正しいため、
//! ここでは **VB6 原典の値** を pin する。
//!
//! VB6 Round 系の実装 (Expression.bas:2991):
//!   num = Int(ldbl * 10 ^ digits)         ' Int = floor (−∞方向)
//!   round    : if frac >= 0.5 then num+1   ' floor-then-+1 = +∞方向の半数丸め
//!   roundup  : if frac >  0   then num+1   ' = ceil
//!   rounddown: (なし)                       ' = floor
//!
//! 著作権配慮: SRC オリジナルコードは含まない。input→expected のみ移植。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn f(call: &str) -> String {
    let src = format!("Set z {call}\n");
    let mut app = App::new();
    let stmts = event::parse(&src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app.script_var("z").to_string()
}

// ============================================================
//  Int — floor (−∞方向)。ゼロ方向切り捨て (trunc) ではない PITFALL
// ============================================================

#[test]
fn int_is_floor_not_truncate() {
    assert_eq!(f("Int(3.9)"), "3");
    assert_eq!(f("Int(3.1)"), "3");
    assert_eq!(f("Int(5)"), "5");
    assert_eq!(f("Int(0)"), "0");
    assert_eq!(f("Int(0.999)"), "0");
    // 負数: floor なので trunc より 1 小さい。
    assert_eq!(f("Int(-3.1)"), "-4"); // floor(-3.1) = -4 (trunc なら -3)
    assert_eq!(f("Int(-2.5)"), "-3");
    assert_eq!(f("Int(-1.1)"), "-2");
    assert_eq!(f("Int(-1)"), "-1"); // 厳密な整数は不変
}

// ============================================================
//  Round — VB6: floor(x*10^d) して frac>=0.5 で +1 (= +∞方向の半数丸め)
// ============================================================

#[test]
fn round_positive_half_rounds_up() {
    assert_eq!(f("Round(2.5,0)"), "3");
    assert_eq!(f("Round(3.5,0)"), "4");
    assert_eq!(f("Round(1.5,0)"), "2");
    assert_eq!(f("Round(2.7,0)"), "3");
    assert_eq!(f("Round(0,0)"), "0");
    assert_eq!(f("Round(2.75,1)"), "2.8");
    // 1 引数形 (digits 省略 = 0)。
    assert_eq!(f("Round(2.5)"), "3");
}

#[test]
fn round_negative_half_goes_toward_positive_infinity() {
    // VB6 原典: Int(-2.5)=-3, frac=0.5>=0.5 → -3+1 = -2。
    // ※ SRC.Sharp オラクルは AwayFromZero で -3 を返す (原典から乖離)。
    //    本実装は VB6 準拠の -2 が正しい。
    assert_eq!(f("Round(-2.5,0)"), "-2");
}

// ============================================================
//  RoundUp (= ceil) / RoundDown (= floor)
// ============================================================

#[test]
fn roundup_is_ceil() {
    assert_eq!(f("RoundUp(3.01,0)"), "4");
    assert_eq!(f("RoundUp(2.1,0)"), "3");
    assert_eq!(f("RoundUp(5,0)"), "5");
    assert_eq!(f("RoundUp(3.0,0)"), "3");
    assert_eq!(f("RoundUp(3.14,1)"), "3.2");
    // 負数は 0 方向へ (ceil)。
    assert_eq!(f("RoundUp(-3.1,0)"), "-3");
}

#[test]
fn rounddown_is_floor() {
    assert_eq!(f("RoundDown(3.99,0)"), "3");
    assert_eq!(f("RoundDown(2.9,0)"), "2");
    assert_eq!(f("RoundDown(5,0)"), "5");
    assert_eq!(f("RoundDown(3.19,1)"), "3.1");
    // 負数は −∞方向へ (floor)。
    assert_eq!(f("RoundDown(-3.1,0)"), "-4");
}

// ============================================================
//  Sqr = 平方根 (square root)、二乗ではない PITFALL
// ============================================================

#[test]
fn sqr_is_square_root() {
    assert_eq!(f("Sqr(9)"), "3");
    assert_eq!(f("Sqr(25)"), "5");
    assert_eq!(f("Sqr(100)"), "10");
    assert_eq!(f("Sqr(144)"), "12");
    assert_eq!(f("Sqr(2.25)"), "1.5");
    assert_eq!(f("Sqr(0)"), "0");
    assert_eq!(f("Sqr(1)"), "1");
}

// ============================================================
//  Abs
// ============================================================

#[test]
fn abs_basic() {
    assert_eq!(f("Abs(5)"), "5");
    assert_eq!(f("Abs(-5)"), "5");
    assert_eq!(f("Abs(0)"), "0");
    assert_eq!(f("Abs(-3.14)"), "3.14");
    assert_eq!(f("Abs(-1000000)"), "1000000");
}

// ============================================================
//  Min / Max — 可変長 (1 引数以上)
// ============================================================

#[test]
fn max_variadic() {
    assert_eq!(f("Max(3,5)"), "5");
    assert_eq!(f("Max(5,3)"), "5");
    assert_eq!(f("Max(-10,-1)"), "-1");
    assert_eq!(f("Max(1,2,99,3,4)"), "99"); // 5 引数
    assert_eq!(f("Max(42)"), "42"); // 単一引数も合法
    assert_eq!(f("Max(3.5,2.1)"), "3.5");
}

#[test]
fn min_variadic() {
    assert_eq!(f("Min(3,5)"), "3");
    assert_eq!(f("Min(0,5)"), "0");
    assert_eq!(f("Min(-5,-1)"), "-5");
    assert_eq!(f("Min(0,-10,5,-3,2)"), "-10"); // 5 引数
    assert_eq!(f("Min(42)"), "42"); // 単一引数も合法
}

// ============================================================
//  三角関数 (radians) — 厳密値のみ
// ============================================================

#[test]
fn trig_exact_values() {
    assert_eq!(f("Sin(0)"), "0");
    assert_eq!(f("Cos(0)"), "1");
    assert_eq!(f("Tan(0)"), "0");
    assert_eq!(f("Atn(0)"), "0");
}
