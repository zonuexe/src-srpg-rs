//! Math 関数 (`Abs` / `Int` / `Round` / `RoundUp` / `RoundDown` / `Sqr` /
//! `Min` / `Max`) のエッジケース。
//!
//! SRC.Sharp `SRCCoreTests/Expressions/MathFunctionTests.cs` 等を参考に、
//! VB6 原典 + .NET Math semantics と一致しているかを検証する。
//!
//! 著作権配慮: SRC オリジナルコードは含まない。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn run(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

fn parse_f64(s: &str) -> f64 {
    s.parse().unwrap_or(f64::NAN)
}

// ============================================================
//  Abs
// ============================================================

#[test]
fn abs_positive() {
    let app = run("Set v Abs(42)");
    assert_eq!(app.script_var("v"), "42");
}

#[test]
fn abs_negative() {
    let app = run("Set v Abs(-42)");
    assert_eq!(app.script_var("v"), "42");
}

#[test]
fn abs_zero() {
    let app = run("Set v Abs(0)");
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn abs_decimal() {
    let app = run("Set v Abs(-2.5)");
    let v = parse_f64(app.script_var("v"));
    assert!((v - 2.5).abs() < 1e-9);
}

// ============================================================
//  Int (Floor semantics, VB6 / SRC.Sharp 共通)
// ============================================================

#[test]
fn int_positive_decimal_floors() {
    let app = run("Set a Int(3.9)\nSet b Int(3.1)");
    assert_eq!(app.script_var("a"), "3");
    assert_eq!(app.script_var("b"), "3");
}

#[test]
fn int_negative_decimal_floors() {
    // SRC.Sharp Math.Floor(-3.1) = -4 (not truncate-to-zero)
    let app = run("Set v Int(-3.1)");
    assert_eq!(app.script_var("v"), "-4");
}

#[test]
fn int_whole_number_unchanged() {
    let app = run("Set v Int(5)");
    assert_eq!(app.script_var("v"), "5");
}

#[test]
fn int_negative_half() {
    // Floor(-0.5) = -1
    let app = run("Set v Int(-0.5)");
    assert_eq!(app.script_var("v"), "-1");
}

// ============================================================
//  Round (+∞ 方向への半数切り上げ; SRC.NET 準拠)
// ============================================================

#[test]
fn round_half_up_toward_positive_infinity() {
    // SRC.NET Expression.cs: floor(scaled) 後、小数部 >= 0.5 なら +1。
    // 正数では away-from-zero と一致するが、負数では +∞ 方向に丸める。
    let app = run("Set a Round(2.5,0)\nSet b Round(3.5,0)\nSet c Round(-2.5,0)");
    assert_eq!(app.script_var("a"), "3");
    assert_eq!(app.script_var("b"), "4");
    // 負数の .5 は +∞ 方向 (-2.5 → -2、away-from-zero の -3 ではない)
    assert_eq!(app.script_var("c"), "-2");
}

#[test]
fn round_negative_non_tie_uses_nearest() {
    // タイでない負数は最近接 (-2.4 → -2、-2.6 → -3)
    let app = run("Set a Round(-2.4,0)\nSet b Round(-2.6,0)");
    assert_eq!(app.script_var("a"), "-2");
    assert_eq!(app.script_var("b"), "-3");
}

#[test]
fn round_negative_half_with_decimals() {
    // Round(-0.5) = 0 (SRC: floor(-0.5)=-1, 小数部 0.5 → 0)
    let app = run("Set v Round(-0.5,0)");
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn round_with_decimals() {
    let app = run("Set v Round(3.14159,2)");
    assert_eq!(app.script_var("v"), "3.14");
}

#[test]
fn round_one_half() {
    let app = run("Set v Round(1.5,0)");
    assert_eq!(app.script_var("v"), "2");
}

// ============================================================
//  RoundUp / RoundDown
// ============================================================

#[test]
fn roundup_ceiling_at_decimal_position() {
    let app = run("Set v RoundUp(3.14,1)");
    let v = parse_f64(app.script_var("v"));
    assert!((v - 3.2).abs() < 1e-9, "RoundUp(3.14,1) = {v}");
}

#[test]
fn roundup_negative_digits_tens_place() {
    // RoundUp(31.4, -1) → 40 (10 の位で切り上げ)
    let app = run("Set v RoundUp(31.4,-1)");
    let v = parse_f64(app.script_var("v"));
    assert!((v - 40.0).abs() < 1e-9, "RoundUp(31.4,-1) = {v}");
}

#[test]
fn rounddown_floor_at_decimal_position() {
    let app = run("Set v RoundDown(3.19,1)");
    let v = parse_f64(app.script_var("v"));
    assert!((v - 3.1).abs() < 1e-9, "RoundDown(3.19,1) = {v}");
}

#[test]
fn rounddown_negative_digits_tens_place() {
    let app = run("Set v RoundDown(39.9,-1)");
    let v = parse_f64(app.script_var("v"));
    assert!((v - 30.0).abs() < 1e-9, "RoundDown(39.9,-1) = {v}");
}

// ============================================================
//  Sqr (Square root)
// ============================================================

#[test]
fn sqr_perfect_square() {
    let app = run("Set v Sqr(16)");
    let v = parse_f64(app.script_var("v"));
    assert!((v - 4.0).abs() < 1e-9, "Sqr(16) = {v}");
}

#[test]
fn sqr_two_returns_approximation() {
    let app = run("Set v Sqr(2)");
    let v = parse_f64(app.script_var("v"));
    assert!((v - 2_f64.sqrt()).abs() < 1e-9, "Sqr(2) = {v}");
}

#[test]
fn sqr_zero_returns_zero() {
    let app = run("Set v Sqr(0)");
    assert_eq!(app.script_var("v"), "0");
}

// ============================================================
//  Min / Max
// ============================================================

#[test]
fn min_two_args() {
    let app = run("Set v Min(3,7)");
    assert_eq!(app.script_var("v"), "3");
}

#[test]
fn min_negative_values() {
    let app = run("Set v Min(-5,-3)");
    assert_eq!(app.script_var("v"), "-5");
}

#[test]
fn max_two_args() {
    let app = run("Set v Max(3,7)");
    assert_eq!(app.script_var("v"), "7");
}

#[test]
fn max_negative_values() {
    let app = run("Set v Max(-5,-3)");
    assert_eq!(app.script_var("v"), "-3");
}

#[test]
fn max_with_arithmetic_args() {
    // Max(3 + 4, 5 + 7) = Max(7, 12) = 12
    let app = run("Set v Max(3 + 4, 5 + 7)");
    assert_eq!(app.script_var("v"), "12");
}

#[test]
fn min_three_args() {
    let app = run("Set v Min(10,1,50)");
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn max_three_args() {
    let app = run("Set v Max(10,100,50)");
    assert_eq!(app.script_var("v"), "100");
}

// ============================================================
//  Sin / Cos / Tan / Atn
// ============================================================

#[test]
fn sin_zero_returns_zero() {
    let app = run("Set v Sin(0)");
    let v = parse_f64(app.script_var("v"));
    assert!(v.abs() < 1e-9, "Sin(0) = {v}");
}

#[test]
fn cos_zero_returns_one() {
    let app = run("Set v Cos(0)");
    let v = parse_f64(app.script_var("v"));
    assert!((v - 1.0).abs() < 1e-9, "Cos(0) = {v}");
}

#[test]
fn tan_zero_returns_zero() {
    let app = run("Set v Tan(0)");
    let v = parse_f64(app.script_var("v"));
    assert!(v.abs() < 1e-9, "Tan(0) = {v}");
}

#[test]
fn atn_zero_returns_zero() {
    let app = run("Set v Atn(0)");
    let v = parse_f64(app.script_var("v"));
    assert!(v.abs() < 1e-9, "Atn(0) = {v}");
}

#[test]
fn atn_one_returns_pi_over_4() {
    // Atn(1) = π/4 ≈ 0.7853981...
    let app = run("Set v Atn(1)");
    let v = parse_f64(app.script_var("v"));
    assert!(
        (v - std::f64::consts::FRAC_PI_4).abs() < 1e-9,
        "Atn(1) = {v}"
    );
}

#[test]
fn sin_and_cos_squared_sum_to_one() {
    // 基本恒等式: sin²(x) + cos²(x) = 1
    // x = π/6 ≈ 0.5235987756
    let app = run(r#"
Set x 0.5235987756
Set s Sin($(x))
Set c Cos($(x))
Set v Round($(s) * $(s) + $(c) * $(c), 10)
"#);
    let v = parse_f64(app.script_var("v"));
    assert!((v - 1.0).abs() < 1e-9, "sin²+cos² = {v}");
}

#[test]
fn sin_pi_over_2_returns_one() {
    // Sin(π/2) = 1
    let pi_over_2 = std::f64::consts::FRAC_PI_2;
    let app = run(&format!("Set v Sin({pi_over_2})"));
    let v = parse_f64(app.script_var("v"));
    assert!((v - 1.0).abs() < 1e-9, "Sin(π/2) = {v}");
}

#[test]
fn cos_pi_returns_minus_one() {
    // Cos(π) = -1
    let pi = std::f64::consts::PI;
    let app = run(&format!("Set v Cos({pi})"));
    let v = parse_f64(app.script_var("v"));
    assert!((v + 1.0).abs() < 1e-9, "Cos(π) = {v}");
}

#[test]
fn tan_pi_over_4_returns_approx_one() {
    // Tan(π/4) ≈ 1
    let pi_over_4 = std::f64::consts::FRAC_PI_4;
    let app = run(&format!("Set v Tan({pi_over_4})"));
    let v = parse_f64(app.script_var("v"));
    assert!((v - 1.0).abs() < 1e-9, "Tan(π/4) = {v}");
}

#[test]
fn atn_minus_one_returns_minus_pi_over_4() {
    // Atn(-1) = -π/4
    let app = run("Set v Atn(-1)");
    let v = parse_f64(app.script_var("v"));
    assert!(
        (v + std::f64::consts::FRAC_PI_4).abs() < 1e-9,
        "Atn(-1) = {v}"
    );
}

// ============================================================
//  Max / Min — 5 引数
// ============================================================

#[test]
fn max_five_args_returns_largest() {
    let app = run("Set v Max(3,1,5,2,4)");
    assert_eq!(app.script_var("v"), "5");
}

#[test]
fn min_five_args_returns_smallest() {
    let app = run("Set v Min(3,1,5,2,4)");
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn max_single_arg_returns_arg() {
    let app = run("Set v Max(42)");
    assert_eq!(app.script_var("v"), "42");
}

#[test]
fn min_single_arg_returns_arg() {
    let app = run("Set v Min(7)");
    assert_eq!(app.script_var("v"), "7");
}
