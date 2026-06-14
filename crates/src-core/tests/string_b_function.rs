//! Shift-JIS バイト系文字列関数のテスト。
//!
//! SRC.Sharp `StringBFunctionEdgeCaseTests` から移植。
//! 実装: ASCII=1バイト、非ASCII=2バイトの Shift-JIS 近似。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn eval(expr: &str) -> String {
    let mut app = App::new();
    let src = format!("Set out {expr}\n");
    let stmts = event::parse(&src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app.script_var("out").to_string()
}

// ──────────────────────────────────────────────
// LenB
// ──────────────────────────────────────────────

#[test]
fn lenb_ascii_string_returns_char_count() {
    // ASCII: 5文字 = 5バイト
    assert_eq!(eval(r#"LenB("hello")"#), "5");
}

#[test]
fn lenb_empty_string_returns_zero() {
    assert_eq!(eval(r#"LenB("")"#), "0");
}

#[test]
fn lenb_mixed_width_returns_byte_count() {
    // "aあ" = 1 + 2 = 3バイト
    assert_eq!(eval(r#"LenB("aあ")"#), "3");
}

#[test]
fn lenb_japanese_only_returns_double_count() {
    // "あいう" = 2 * 3 = 6バイト
    assert_eq!(eval(r#"LenB("あいう")"#), "6");
}

// ──────────────────────────────────────────────
// LeftB
// ──────────────────────────────────────────────

#[test]
fn leftb_ascii_returns_prefix_bytes() {
    assert_eq!(eval(r#"LeftB("hello",3)"#), "hel");
}

#[test]
fn leftb_zero_returns_empty() {
    assert_eq!(eval(r#"LeftB("hello",0)"#), "");
}

#[test]
fn leftb_beyond_length_returns_all() {
    assert_eq!(eval(r#"LeftB("hello",100)"#), "hello");
}

#[test]
fn leftb_japanese_splits_at_char_boundary() {
    // "あいう" の2バイトを取ると "あ" (1文字=2バイト)
    assert_eq!(eval(r#"LeftB("あいう",2)"#), "あ");
}

// ──────────────────────────────────────────────
// RightB
// ──────────────────────────────────────────────

#[test]
fn rightb_ascii_returns_suffix_bytes() {
    assert_eq!(eval(r#"RightB("hello",3)"#), "llo");
}

#[test]
fn rightb_zero_returns_empty() {
    assert_eq!(eval(r#"RightB("hello",0)"#), "");
}

#[test]
fn rightb_japanese_splits_at_char_boundary() {
    // "あいう" の末尾2バイト = "う"
    assert_eq!(eval(r#"RightB("あいう",2)"#), "う");
}

// ──────────────────────────────────────────────
// MidB
// ──────────────────────────────────────────────

#[test]
fn midb_two_args_ascii_from_byte_position() {
    // "hello" の3バイト目以降 = "llo"
    assert_eq!(eval(r#"MidB("hello",3)"#), "llo");
}

#[test]
fn midb_three_args_ascii() {
    // "hello" の2バイト目から3バイト = "ell"
    assert_eq!(eval(r#"MidB("hello",2,3)"#), "ell");
}

#[test]
fn midb_japanese_from_third_byte() {
    // "あいう" の3バイト目以降 = "いう"
    assert_eq!(eval(r#"MidB("あいう",3)"#), "いう");
}

// ──────────────────────────────────────────────
// InStrB
// ──────────────────────────────────────────────

#[test]
fn instrb_ascii_returns_byte_position() {
    // "hello" で "l" の最初の位置 = 3バイト目
    assert_eq!(eval(r#"InStrB("hello","l")"#), "3");
}

#[test]
fn instrb_not_found_returns_zero() {
    assert_eq!(eval(r#"InStrB("hello","z")"#), "0");
}

#[test]
fn instrb_with_start_position_searches_from_position() {
    // "hello" で "l" を4バイト目から検索 → 4バイト目
    assert_eq!(eval(r#"InStrB("hello","l",4)"#), "4");
}

// ──────────────────────────────────────────────
// InStrRevB
// ──────────────────────────────────────────────

#[test]
fn instrrevb_ascii_returns_last_position() {
    // "hello" で "l" の最後のバイト位置 = 4
    assert_eq!(eval(r#"InStrRevB("hello","l")"#), "4");
}

#[test]
fn instrrevb_not_found_returns_zero() {
    assert_eq!(eval(r#"InStrRevB("hello","z")"#), "0");
}

#[test]
fn instrrevb_with_end_position_limits_search() {
    // "hello" で "l" を3バイト目までで検索 → 3バイト目
    assert_eq!(eval(r#"InStrRevB("hello","l",3)"#), "3");
}

// ──────────────────────────────────────────────
// WindowWidth / WindowHeight
// ──────────────────────────────────────────────

#[test]
fn window_width_returns_numeric() {
    let v: i64 = eval("WindowWidth()").parse().expect("numeric");
    assert!(v >= 0);
}

#[test]
fn window_height_returns_numeric() {
    let v: i64 = eval("WindowHeight()").parse().expect("numeric");
    assert!(v >= 0);
}
