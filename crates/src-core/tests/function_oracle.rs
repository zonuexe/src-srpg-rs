//! 時刻 / 正規表現 / 判定系の純粋関数のオラクル準拠テスト。
//!
//! 原典 SRC.Sharp `SRCCoreTests/Expressions/{Time,Regex,Utility,Other}*` で確認した
//! 純粋・決定論的関数の input→expected を実行時評価器に突き合わせる。
//! 時刻関数は **日付文字列引数あり** の形のみ (引数省略は現在時刻依存で非決定論)。
//!
//! 注: `Val/CInt/CStr/Str/Hex/Oct` は SRC 原典に存在しない (関数表に無い) ため対象外。
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
//  時刻関数 (日付文字列引数) — パース失敗は 0、曜日は日本語名
// ============================================================

#[test]
fn time_functions_with_date_string() {
    assert_eq!(f(r#"Year("2024/03/15")"#), "2024");
    assert_eq!(f(r#"Month("2024/03/15")"#), "3"); // 先頭ゼロ無し
    assert_eq!(f(r#"Day("2024/03/15")"#), "15");
    assert_eq!(f(r#"Hour("2024/03/15 14:30:45")"#), "14");
    assert_eq!(f(r#"Minute("2024/03/15 14:30:45")"#), "30");
    assert_eq!(f(r#"Second("2024/03/15 14:30:45")"#), "45");
}

#[test]
fn time_functions_invalid_date_returns_zero() {
    assert_eq!(f(r#"Year("notadate")"#), "0");
    assert_eq!(f(r#"Month("notadate")"#), "0");
}

#[test]
fn weekday_returns_japanese_name() {
    // 2024/03/15 は金曜。
    assert_eq!(f(r#"Weekday("2024/03/15")"#), "金曜");
    assert_eq!(f(r#"Weekday("2024/03/16")"#), "土曜");
    assert_eq!(f(r#"Weekday("2024/03/17")"#), "日曜");
}

#[test]
fn difftime_is_d2_minus_d1_seconds() {
    assert_eq!(
        f(r#"DiffTime("2024/01/01 10:00:00","2024/01/01 11:00:00")"#),
        "3600"
    );
    assert_eq!(
        f(r#"DiffTime("2024/01/01 10:00:00","2024/01/01 10:01:00")"#),
        "60"
    );
    assert_eq!(
        f(r#"DiffTime("2024/01/01 10:00:05","2024/01/01 10:00:05")"#),
        "0"
    );
    // 逆順は負。
    assert_eq!(
        f(r#"DiffTime("2024/01/01 11:00:00","2024/01/01 10:00:00")"#),
        "-3600"
    );
}

// ============================================================
//  正規表現 (.NET regex 相当、既定は大小区別あり)
// ============================================================

#[test]
fn regexp_returns_first_match_or_empty() {
    assert_eq!(f(r#"RegExp("hello world","[a-z]+")"#), "hello");
    assert_eq!(f(r#"RegExp("abc123","[0-9]+")"#), "123");
    assert_eq!(f(r#"RegExp("price: 42","[0-9]+")"#), "42");
    assert_eq!(f(r#"RegExp("hello","[0-9]+")"#), ""); // 不一致 → 空
}

#[test]
fn regexp_replace_replaces_all_matches() {
    assert_eq!(
        f(r#"RegExpReplace("hello world","world","SRC")"#),
        "hello SRC"
    );
    assert_eq!(
        f(r##"RegExpReplace("abc123def456","[0-9]+","#")"##),
        "abc#def#"
    );
    assert_eq!(f(r#"RegExpReplace("a1b2c3","[0-9]","")"#), "abc");
}

// ============================================================
//  IsNumeric — 数値判定 (1/0)
// ============================================================

#[test]
fn is_numeric_basic() {
    assert_eq!(f(r#"IsNumeric("42")"#), "1");
    assert_eq!(f(r#"IsNumeric("3.14")"#), "1");
    assert_eq!(f(r#"IsNumeric("-100")"#), "1");
    assert_eq!(f(r#"IsNumeric("abc")"#), "0");
    assert_eq!(f(r#"IsNumeric("")"#), "0");
}

// ============================================================
//  Eval — 式文字列を評価
// ============================================================

#[test]
fn eval_evaluates_expression_string() {
    assert_eq!(f(r#"Eval("42")"#), "42");
    let app = {
        let src = "Set x 55\nSet z Eval(x)\n";
        let mut app = App::new();
        let stmts = event::parse(src).expect("parse");
        event_runtime::execute(&mut app, &stmts).expect("execute");
        app
    };
    assert_eq!(app.script_var("z"), "55");
}
