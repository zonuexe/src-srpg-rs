//! ネストされた関数呼び出しのテスト / Nested function call edge cases.
//!
//! SRC.Sharp `SRCCoreTests/Expressions/ExpressionNestedFunctionTests.cs` を
//! 参考に、関数を関数の引数に渡すケースを網羅する。最近の `expand_vars`
//! nested-paren 修正の後段検証も兼ねる。
//!
//! 著作権配慮: SRC オリジナルコードを含まない。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn run(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

// ============================================================
//  ネストされた数学関数
// ============================================================

#[test]
fn abs_of_int_negative_decimal() {
    // Int(-3.7) = -3 or -4 (実装依存), Abs(...) はいずれも 3 or 4
    let app = run(r#"Set v Abs(Int(-3.7))"#);
    let v = app.script_var("v");
    assert!(v == "3" || v == "4", "Abs(Int(-3.7)) = {v}");
}

#[test]
fn int_of_sqr_returns_floor_of_root() {
    // Sqr(10) ≈ 3.162, Int(3.162) = 3
    let app = run(r#"Set v Int(Sqr(10))"#);
    assert_eq!(app.script_var("v"), "3");
}

#[test]
fn max_of_abs_returns_larger_absolute() {
    let app = run(r#"Set v Max(Abs(-10),Abs(5))"#);
    assert_eq!(app.script_var("v"), "10");
}

#[test]
fn min_of_abs_returns_smaller_absolute() {
    let app = run(r#"Set v Min(Abs(-10),Abs(5))"#);
    assert_eq!(app.script_var("v"), "5");
}

#[test]
fn round_of_sqr_two_digits() {
    let app = run(r#"Set v Round(Sqr(2),2)"#);
    assert_eq!(app.script_var("v"), "1.41");
}

// ============================================================
//  ネストされた文字列関数
// ============================================================

#[test]
fn len_of_left_returns_substring_length() {
    let app = run(r#"Set v Len(Left("hello",3))"#);
    assert_eq!(app.script_var("v"), "3");
}

#[test]
fn left_of_right_extracts_mid_section() {
    // Right("hello", 4) = "ello", Left("ello", 2) = "el"
    let app = run(r#"Set v Left(Right("hello",4),2)"#);
    assert_eq!(app.script_var("v"), "el");
}

#[test]
fn mid_of_replace_processes_correctly() {
    // Replace("abcde", "c", "X") = "abXde", Mid(...,2,3) = "bXd"
    let app = run(r#"Set v Mid(Replace("abcde","c","X"),2,3)"#);
    assert_eq!(app.script_var("v"), "bXd");
}

// ============================================================
//  関数 + 算術
// ============================================================

#[test]
fn abs_of_difference_returns_absolute_difference() {
    let app = run(r#"Set v Abs(3 - 10)"#);
    assert_eq!(app.script_var("v"), "7");
}

// ============================================================
//  変数を含むネスト
// ============================================================

#[test]
fn abs_of_variable() {
    let app = run(r#"
Set x -42
Set v Abs($(x))
"#);
    assert_eq!(app.script_var("v"), "42");
}

#[test]
fn nested_args_via_implicit_call() {
    // implicit Call + $(Args(N)) で `Lindex(List(a,b,c), N)` を渡しても
    // 正しく解決できる
    let app = run(r#"
Goto run
helper:
  Set result Lindex($(Args(1)),$(Args(2)))
  Return
run:
helper "List(alpha,beta,gamma)" 2
"#);
    // Lindex は 1-indexed なので 2 つ目 = beta
    assert_eq!(app.script_var("result"), "beta");
}

#[test]
fn deeply_nested_paren_in_dollar_expansion() {
    // `$(...)` の中身に関数呼出がネストされても `expand_vars` の
    // paren-balanced scan が正しく動くこと
    let app = run(r#"
Set base 10
Set v $(Min(20,$(base)))
"#);
    // 内側 $(base) → "10"、外側 $(Min(20,10)) は中身全体が関数呼出なので
    // `expand_dollar_paren` が式として評価する → Min(20,10) = 10。
    assert_eq!(app.script_var("v"), "10");
}

#[test]
fn nested_info_call_in_set() {
    // Info の結果を別の関数に渡す典型パターン
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon "ブレイバー" "ビームライフル" 2500 2 5 15 -1
Place "ブレイバー" "リオ" Player 0 0
Set v Int(Info("ユニットデータ","ブレイバー","最大ＨＰ"))
"#);
    assert_eq!(app.script_var("v"), "3500");
}
