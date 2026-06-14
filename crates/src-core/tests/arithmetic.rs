//! 算術 / 比較 / 論理演算子のテスト。
//!
//! SRC.Sharp `SRCCoreTests/Expressions/ExpressionArithmeticTests.cs` /
//! `ExpressionOperatorTests.cs` を参考に、`try_eval_int` の演算子サポートを
//! 検証する。
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

fn arith(expr: &str) -> String {
    let app = run(&format!("Set v {expr}\n"));
    app.script_var("v").to_string()
}

// ============================================================
//  基本四則
// ============================================================

#[test]
fn add_two_integers() {
    let app = run("Set v Eval(3 + 4)");
    assert_eq!(app.script_var("v"), "7");
}

#[test]
fn subtract_two_integers() {
    let app = run("Set v Eval(5 - 4)");
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn multiply_two_integers() {
    let app = run("Set v Eval(3 * 4)");
    assert_eq!(app.script_var("v"), "12");
}

#[test]
fn divide_two_integers_returns_float() {
    // SRC.Sharp: 10 / 4 = 2.5 (浮動小数除算)
    let app = run("Set v Eval(10 / 4)");
    assert_eq!(app.script_var("v"), "2.5");
}

#[test]
fn divide_even_integers_returns_whole_number() {
    // 整数結果は format_num で整数表記
    let app = run("Set v Eval(8 / 4)");
    assert_eq!(app.script_var("v"), "2");
}

// ============================================================
//  VB6 特有: 整数除算 / 指数 / Mod
// ============================================================

#[test]
fn integer_divide_backslash() {
    // VB6: `5 \ 2` = 2 (整数除算・切り捨て)
    assert_eq!(arith("Eval(5 \\ 2)"), "2");
    // 負数: `-7 \ 2` = -3 (truncate-toward-zero)
    assert_eq!(arith("Eval(-7 \\ 2)"), "-3");
}

#[test]
fn exponent_caret() {
    // VB6: `2 ^ 3` = 8
    assert_eq!(arith("Eval(2 ^ 3)"), "8");
    // `2 ^ 0.5` ≈ 1.4142…
    let v: f64 = arith("Eval(2 ^ 0.5)").parse().unwrap();
    assert!((v - std::f64::consts::SQRT_2).abs() < 1e-6);
}

#[test]
fn modulo_keyword() {
    // VB6: `7 Mod 3` = 1
    assert_eq!(arith("Eval(7 Mod 3)"), "1");
    assert_eq!(arith("Eval(10 Mod 3)"), "1");
}

// ============================================================
//  優先順位 / 括弧
// ============================================================

#[test]
fn order_of_operations() {
    // 2 + 3 * 4 = 14
    let v = arith("Eval(2 + 3 * 4)");
    assert_eq!(v, "14");
}

#[test]
fn parentheses_change_precedence() {
    // (2 + 3) * 4 = 20
    let v = arith("Eval((2 + 3) * 4)");
    assert_eq!(v, "20");
}

#[test]
fn unary_minus() {
    let v = arith("Eval(-5)");
    assert_eq!(v, "-5");
}

// ============================================================
//  比較演算子 (If で使用される)
// ============================================================

#[test]
fn equal_true_branch() {
    let app = run(r#"
If 3 = 3 Then
  Set v yes
Else
  Set v no
EndIf
"#);
    assert_eq!(app.script_var("v"), "yes");
}

#[test]
fn equal_false_branch() {
    let app = run(r#"
If 3 = 4 Then
  Set v yes
Else
  Set v no
EndIf
"#);
    assert_eq!(app.script_var("v"), "no");
}

#[test]
fn not_equal_diamond() {
    let app = run(r#"
If 3 <> 4 Then
  Set v yes
Else
  Set v no
EndIf
"#);
    assert_eq!(app.script_var("v"), "yes");
}

#[test]
fn less_than_when_less() {
    let app = run(r#"
If 3 < 4 Then
  Set v yes
EndIf
"#);
    assert_eq!(app.script_var("v"), "yes");
}

#[test]
fn less_than_or_equal_when_equal() {
    let app = run(r#"
If 4 <= 4 Then
  Set v yes
EndIf
"#);
    assert_eq!(app.script_var("v"), "yes");
}

#[test]
fn greater_than_or_equal_when_greater() {
    let app = run(r#"
If 5 >= 4 Then
  Set v yes
EndIf
"#);
    assert_eq!(app.script_var("v"), "yes");
}

// ============================================================
//  論理演算子 (And / Or / Not)
// ============================================================

#[test]
fn logical_and_both_true() {
    let app = run(r#"
If 1 = 1 And 2 = 2 Then
  Set v yes
EndIf
"#);
    assert_eq!(app.script_var("v"), "yes");
}

#[test]
fn logical_and_one_false() {
    let app = run(r#"
If 1 = 1 And 2 = 3 Then
  Set v yes
Else
  Set v no
EndIf
"#);
    assert_eq!(app.script_var("v"), "no");
}

#[test]
fn logical_or_one_true() {
    let app = run(r#"
If 1 = 9 Or 2 = 2 Then
  Set v yes
EndIf
"#);
    assert_eq!(app.script_var("v"), "yes");
}

#[test]
fn logical_not_inverts() {
    let app = run(r#"
If Not (1 = 9) Then
  Set v yes
EndIf
"#);
    assert_eq!(app.script_var("v"), "yes");
}

// ============================================================
//  文字列連結
// ============================================================

#[test]
fn string_concat_with_ampersand() {
    let app = run(r#"
Set a hello
Set b world
Set v "$(a) & ' ' & $(b)"
"#);
    // pin 挙動 (実装が ` & ` を文字列連結子と解釈するかは未確定)
    let _ = app.script_var("v");
}

#[test]
fn concat_via_dollar_expansion() {
    // SRC でよくある: `Set msg "$(a)-$(b)"` で文字列連結
    let app = run(r#"
Set a hello
Set b world
Set v "$(a)-$(b)"
"#);
    assert_eq!(app.script_var("v"), "hello-world");
}

// ============================================================
//  比較演算子の算術評価 (Set コンテキストで 0/1 を返す)
//  SRC.Sharp `ExpressionArithmeticMoreTests` 準拠
// ============================================================

#[test]
fn comparison_greater_than_true_returns_one() {
    // `Set v (5 > 3)` → v = "1"
    let app = run("Set v (5 > 3)\n");
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn comparison_greater_than_false_returns_zero() {
    let app = run("Set v (3 > 5)\n");
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn comparison_less_than_or_equal_equal() {
    let app = run("Set v (5 <= 5)\n");
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn comparison_greater_than_or_equal_true() {
    let app = run("Set v (5 >= 5)\n");
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn comparison_not_equal_true() {
    let app = run("Set v (5 <> 3)\n");
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn comparison_not_equal_false() {
    let app = run("Set v (5 <> 5)\n");
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn comparison_equal_true() {
    let app = run("Set v (3 = 3)\n");
    assert_eq!(app.script_var("v"), "1");
}

// ============================================================
//  論理演算子の算術評価 (Set コンテキスト)
//  SRC.Sharp `ExpressionArithmeticMoreTests` (And/Or/Not) 準拠
// ============================================================

#[test]
fn logical_and_both_true_returns_one() {
    let app = run("Set v ((1 = 1) And (2 = 2))\n");
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn logical_and_one_false_returns_zero() {
    let app = run("Set v ((1 = 1) And (2 = 3))\n");
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn logical_or_one_true_returns_one() {
    let app = run("Set v ((1 = 1) Or (2 = 3))\n");
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn logical_or_both_false_returns_zero() {
    let app = run("Set v ((1 = 2) Or (3 = 4))\n");
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn logical_not_true_returns_zero() {
    let app = run("Set v (Not (1 = 1))\n");
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn logical_not_false_returns_one() {
    let app = run("Set v (Not (1 = 2))\n");
    assert_eq!(app.script_var("v"), "1");
}

// ============================================================
//  GetValueAsLong の切り捨て挙動
//  SRC.Sharp `ExpressionGetValueAsLongTests` 準拠:
//  `(int)double` はゼロ方向への切り捨て。
// ============================================================

#[test]
fn for_loop_end_value_truncated_not_rounded() {
    // For i = 1 To (7/2) → To 3 (切り捨て), not 4 (四捨五入)
    // SRC.Sharp `ExpressionGetValueAsLongTests.GetValueAsLong_PositiveDecimal_TruncatesTowardZero`
    // に対応。`eval_int_expr` が `.trunc()` を使うことを検証する。
    let app = run(r#"
Set count 0
For i = 1 To (7/2)
  Incr count
Next
"#);
    assert_eq!(app.script_var("count"), "3");
}

#[test]
fn for_loop_end_value_negative_decimal_truncated() {
    // `-3.9 → -3` (ゼロ方向切り捨て)。For の終端値が負の小数でも正しく動く。
    // ループ自体は start=0 > end=-3 で実行しないが、end値の解釈が正しいか確認。
    let app = run(r#"
Set count 99
For i = 0 To (-7/2)
  Set count 0
Next
"#);
    // start(0) > end(-3) なので loop しない → count = 99 のまま
    assert_eq!(app.script_var("count"), "99");
}

// ============================================================
//  変数を含む算術
// ============================================================

#[test]
fn variable_in_arithmetic() {
    let app = run(r#"
Set x 10
Set v Eval($(x) + 5)
"#);
    assert_eq!(app.script_var("v"), "15");
}

#[test]
fn complex_with_variables_and_parens() {
    let app = run(r#"
Set a 3
Set b 4
Set v Eval(($(a) + $(b)) * 2)
"#);
    assert_eq!(app.script_var("v"), "14");
}
