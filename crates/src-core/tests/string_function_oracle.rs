//! String / Format 関数のオラクル準拠テスト。
//!
//! 原典 SRC.Sharp `SRCCoreTests/Expressions/StringFunction* / FormatFunction* /
//! ExpressionReplace*` の input→expected を実行時評価器に突き合わせる。
//! 突合の結果、本実装の String/Format サブシステムは全面的に忠実 (バグなし) で
//! あることを確認済。本ファイルはその忠実性を将来の回帰から守る pin。
//!
//! 重要な仕様 (VB6 由来):
//! - 索引は **1-based** (Mid/Left/Right/InStr)。
//! - `Len` = 文字数 / `LenB` = Shift-JIS バイト数 (全角=2)。
//! - InStr: 見つからない→0、空needle→1、3引数目は開始位置。
//! - Format = .NET `ToString(fmt)` 相当。`0` は trailing-zero 維持・`#` は除去・
//!   丸めは banker's (VB6 Format と一致)。
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
//  Len (文字数) vs LenB (Shift-JIS バイト数)
// ============================================================

#[test]
fn len_is_char_count() {
    assert_eq!(f(r#"Len("hello")"#), "5");
    assert_eq!(f(r#"Len("")"#), "0");
    assert_eq!(f(r#"Len("あいう")"#), "3"); // 全角でも文字数 = 3
    assert_eq!(f(r#"Len("   ")"#), "3");
}

#[test]
fn lenb_is_sjis_byte_count() {
    assert_eq!(f(r#"LenB("hello")"#), "5");
    assert_eq!(f(r#"LenB("あいう")"#), "6"); // 全角 = 2 バイト
    assert_eq!(f(r#"LenB("Aあ")"#), "3"); // 1 + 2
    assert_eq!(f(r#"LenB("")"#), "0");
}

// ============================================================
//  Left / Right — 1-based、超過はクランプ、0 は空
// ============================================================

#[test]
fn left_and_right() {
    assert_eq!(f(r#"Left("hello",3)"#), "hel");
    assert_eq!(f(r#"Left("hello",10)"#), "hello"); // 超過クランプ
    assert_eq!(f(r#"Left("hello",0)"#), "");
    assert_eq!(f(r#"Left("あいうえお",2)"#), "あい");
    assert_eq!(f(r#"Right("hello",3)"#), "llo");
    assert_eq!(f(r#"Right("hello",10)"#), "hello");
    assert_eq!(f(r#"Right("hello",1)"#), "o");
    assert_eq!(f(r#"Right("あいうえお",2)"#), "えお");
}

// ============================================================
//  Mid — 1-based、2/3 引数、クランプ
// ============================================================

#[test]
fn mid_one_based_and_clamping() {
    assert_eq!(f(r#"Mid("hello",2,3)"#), "ell"); // start=2 = 2文字目
    assert_eq!(f(r#"Mid("hello",1)"#), "hello"); // start=1 = 全体
    assert_eq!(f(r#"Mid("hello",4)"#), "lo"); // 2 引数 = 末尾まで
    assert_eq!(f(r#"Mid("hello",10)"#), ""); // start > 長さ → ""
    assert_eq!(f(r#"Mid("hello",4,100)"#), "lo"); // len 超過 → 末尾までクランプ
    assert_eq!(f(r#"Mid("hello",2,0)"#), ""); // len 0 → ""
    assert_eq!(f(r#"Mid("あいうえお",2,2)"#), "いう"); // 文字基準
}

// ============================================================
//  InStr — 1-based、not-found→0、3 引数開始、空needle→1
// ============================================================

#[test]
fn instr_semantics() {
    assert_eq!(f(r#"InStr("hello","el")"#), "2");
    assert_eq!(f(r#"InStr("hello","h")"#), "1");
    assert_eq!(f(r#"InStr("hello","o")"#), "5");
    assert_eq!(f(r#"InStr("hello","xyz")"#), "0"); // not-found → 0
    assert_eq!(f(r#"InStr("hello","l",3)"#), "3"); // 3 引数目 = 開始位置
    assert_eq!(f(r#"InStr("hello","h",2)"#), "0"); // 開始位置が唯一の一致を過ぎる → 0
    assert_eq!(f(r#"InStr("あいうえお","う")"#), "3"); // 文字索引
    assert_eq!(f(r#"InStr("hello","")"#), "1"); // 空 needle → 1
}

// ============================================================
//  Replace — 3 引数は全置換
// ============================================================

#[test]
fn replace_all_occurrences() {
    assert_eq!(f(r#"Replace("hello","e","X")"#), "hXllo");
    assert_eq!(f(r#"Replace("hello","z","X")"#), "hello"); // 一致なし
    assert_eq!(f(r#"Replace("ababa","a","x")"#), "xbxbx"); // 全置換
    assert_eq!(f(r#"Replace("hello","l","X")"#), "heXXo");
    assert_eq!(f(r#"Replace("hello","e","")"#), "hllo"); // 削除
    assert_eq!(f(r#"Replace("hello","ll","")"#), "heo");
}

// ============================================================
//  Format — .NET ToString(fmt) 相当
// ============================================================

#[test]
fn format_zero_pad_and_decimals() {
    assert_eq!(f(r#"Format(7,"000")"#), "007");
    assert_eq!(f(r#"Format(100,"000")"#), "100");
    assert_eq!(f(r#"Format(0,"000")"#), "000");
    assert_eq!(f(r#"Format(42,"0")"#), "42");
    assert_eq!(f(r#"Format(-5,"00")"#), "-05"); // 符号は 0 埋めの前
    assert_eq!(f(r#"Format(1000000,"0")"#), "1000000");
}

#[test]
fn format_fraction_zero_keeps_hash_drops() {
    assert_eq!(f(r#"Format(3.14,"0.00")"#), "3.14");
    assert_eq!(f(r#"Format(3.1,"0.00")"#), "3.10"); // `0` は trailing-zero 維持
    assert_eq!(f(r#"Format(3.14,"0.##")"#), "3.14");
    assert_eq!(f(r#"Format(3.0,"0.##")"#), "3"); // `#` は trailing-zero 除去
}

// ============================================================
//  String(count, s) — count が第1引数、文字列全体を反復
// ============================================================

#[test]
fn string_repeat() {
    assert_eq!(f(r#"String(3,"a")"#), "aaa");
    assert_eq!(f(r#"String(0,"a")"#), "");
    assert_eq!(f(r#"String(5,"a")"#), "aaaaa");
    assert_eq!(f(r#"String(3,"ab")"#), "ababab"); // 文字列全体を反復
    assert_eq!(f(r#"String(3,"あ")"#), "あああ");
}

// ============================================================
//  Asc / Chr (ASCII) / LCase / UCase / Trim / Wide
// ============================================================

#[test]
fn asc_and_chr_ascii() {
    assert_eq!(f(r#"Asc("A")"#), "65");
    assert_eq!(f(r#"Asc("a")"#), "97");
    assert_eq!(f(r#"Asc("0")"#), "48");
    assert_eq!(f(r#"Chr(65)"#), "A");
    assert_eq!(f(r#"Chr(97)"#), "a");
    assert_eq!(f(r#"Chr(90)"#), "Z");
}

#[test]
fn case_and_trim() {
    assert_eq!(f(r#"LCase("HELLO")"#), "hello");
    assert_eq!(f(r#"LCase("ABC123")"#), "abc123");
    assert_eq!(f(r#"UCase("hello")"#), "HELLO");
    assert_eq!(f(r#"Trim("  hello  ")"#), "hello");
    assert_eq!(f(r#"Trim("  hello world  ")"#), "hello world"); // 内部空白は保持
    assert_eq!(f(r#"Trim("   ")"#), "");
}

#[test]
fn wide_half_to_fullwidth() {
    assert_eq!(f(r#"Wide("ABC")"#), "ＡＢＣ");
    assert_eq!(f(r#"Wide("123")"#), "１２３");
    assert_eq!(f(r#"Wide("abc")"#), "ａｂｃ");
    assert_eq!(f(r#"Wide("")"#), "");
}

// ============================================================
//  *B 関数 (Shift-JIS バイト基準, 1-based)
// ============================================================

#[test]
fn byte_functions_sjis() {
    assert_eq!(f(r#"LeftB("あいう",2)"#), "あ"); // 2 バイト = 全角1文字
    assert_eq!(f(r#"RightB("あいう",2)"#), "う");
    assert_eq!(f(r#"MidB("あいう",3,2)"#), "い"); // バイト位置 3 から 2 バイト
    assert_eq!(f(r#"InStrB("あいう","い")"#), "3"); // い はバイト位置 3
    assert_eq!(f(r#"LeftB("hello",3)"#), "hel");
    assert_eq!(f(r#"InStrB("hello","l")"#), "3");
}
