//! 文字列関数の edge cases。
//!
//! SRC.Sharp `SRCCoreTests/Expressions/StringFunctionTests.cs` から
//! ポート。新規実装: String / Wide / LCase / UCase / Trim / Asc / Chr / InStrRev。
//! 既存実装: Len / Left / Right / Mid / InStr / StrCmp / Replace の追加検証。

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
// String(count, s)
// ──────────────────────────────────────────────

#[test]
fn string_repeats_token() {
    // VB6 `String(3, "0 ")` → "0 0 0 " (スパロボ戦記 で頻出パターン)
    assert_eq!(eval(r#"String(3, "0 ")"#), "0 0 0 ");
}

#[test]
fn string_zero_count_returns_empty() {
    assert_eq!(eval(r#"String(0, "abc")"#), "");
}

#[test]
fn string_single_char_repeats() {
    assert_eq!(eval(r#"String(5, "a")"#), "aaaaa");
}

#[test]
fn string_japanese_token_repeats() {
    assert_eq!(eval(r#"String(2, "あ")"#), "ああ");
}

// ──────────────────────────────────────────────
// Wide(s)
// ──────────────────────────────────────────────

#[test]
fn wide_ascii_to_fullwidth() {
    // 'A' (0x41) → 'Ａ' (U+FF21)
    assert_eq!(eval(r#"Wide("ABC")"#), "ＡＢＣ");
}

#[test]
fn wide_digits_to_fullwidth() {
    // '0' (0x30) → '０' (U+FF10)
    assert_eq!(eval(r#"Wide("123")"#), "１２３");
}

#[test]
fn wide_space_becomes_fullwidth_space() {
    // ASCII space (0x20) → '\u{3000}' 全角空白
    assert_eq!(eval(r#"Wide(" ")"#), "\u{3000}");
}

#[test]
fn wide_already_fullwidth_unchanged() {
    // 既に全角の文字は触らない
    assert_eq!(eval(r#"Wide("あいう")"#), "あいう");
}

#[test]
fn wide_halfwidth_katakana_to_fullwidth() {
    // 半角カタカナ → 全角カタカナ (StrConv vbWide 準拠)
    assert_eq!(eval("Wide(\"\u{FF76}\u{FF85}\")"), "カナ");
}

#[test]
fn wide_halfwidth_katakana_with_dakuten() {
    // ｶﾞ (FF76 FF9E) → ガ、ﾊﾟ (FF8A FF9F) → パ
    assert_eq!(eval("Wide(\"\u{FF76}\u{FF9E}\")"), "ガ");
    assert_eq!(eval("Wide(\"\u{FF8A}\u{FF9F}\")"), "パ");
}

#[test]
fn wide_halfwidth_u_with_dakuten_is_vu() {
    // ｳﾞ (FF73 FF9E) → ヴ
    assert_eq!(eval("Wide(\"\u{FF73}\u{FF9E}\")"), "ヴ");
}

#[test]
fn wide_standalone_dakuten_is_fullwidth_mark() {
    // 濁点不可な仮名 (ﾝ) + ﾞ は合成せず単独の全角濁点を出力
    assert_eq!(eval("Wide(\"\u{FF9D}\u{FF9E}\")"), "ン\u{309B}");
}

#[test]
fn wide_halfwidth_punctuation() {
    // ｡ (FF61) → 。、ｰ (FF70) → ー
    assert_eq!(eval("Wide(\"\u{FF61}\u{FF70}\")"), "。ー");
}

// ──────────────────────────────────────────────
// LCase / UCase
// ──────────────────────────────────────────────

#[test]
fn lcase_converts_to_lower() {
    assert_eq!(eval(r#"LCase("HELLO")"#), "hello");
}

#[test]
fn lcase_japanese_unchanged() {
    // SRC.Sharp StringFunctionMoreTests と同じ期待値
    assert_eq!(eval(r#"LCase("てすと")"#), "てすと");
}

#[test]
fn lcase_mixed_case_with_digits() {
    assert_eq!(eval(r#"LCase("ABC123")"#), "abc123");
}

#[test]
fn ucase_converts_to_upper() {
    assert_eq!(eval(r#"UCase("hello")"#), "HELLO");
}

#[test]
fn ucase_mixed_case() {
    assert_eq!(eval(r#"UCase("Hello World")"#), "HELLO WORLD");
}

// ──────────────────────────────────────────────
// Trim(s)
// ──────────────────────────────────────────────

#[test]
fn trim_removes_leading_and_trailing() {
    assert_eq!(eval(r#"Trim("  hello  ")"#), "hello");
}

#[test]
fn trim_leading_only() {
    assert_eq!(eval(r#"Trim("  hello")"#), "hello");
}

#[test]
fn trim_trailing_only() {
    assert_eq!(eval(r#"Trim("hello  ")"#), "hello");
}

#[test]
fn trim_internal_spaces_preserved() {
    assert_eq!(eval(r#"Trim("  hello world  ")"#), "hello world");
}

#[test]
fn trim_only_spaces_returns_empty() {
    assert_eq!(eval(r#"Trim("   ")"#), "");
}

// ──────────────────────────────────────────────
// Asc / Chr
// ──────────────────────────────────────────────

#[test]
fn asc_ascii_uppercase_returns_byte_value() {
    // ASCII (0..=0x7F) は SJIS と Unicode で同値。
    assert_eq!(eval(r#"Asc("A")"#), "65");
}

#[test]
fn asc_lowercase() {
    assert_eq!(eval(r#"Asc("a")"#), "97");
}

#[test]
fn asc_digit_zero() {
    assert_eq!(eval(r#"Asc("0")"#), "48");
}

#[test]
fn chr_from_code() {
    assert_eq!(eval("Chr(65)"), "A");
}

#[test]
fn chr_space() {
    assert_eq!(eval("Chr(32)"), " ");
}

#[test]
fn asc_japanese_returns_sjis_code() {
    // VB6 互換: "あ" は SJIS で 0x82A0 = 33440。
    // (Unicode codepoint U+3042 = 12354 ではない)
    assert_eq!(eval(r#"Asc("あ")"#), "33440");
}

#[test]
fn chr_sjis_double_byte_decodes_japanese() {
    // VB6 互換: Chr(0x82A0) (= 33440) は SJIS 2 バイト ("あ") として decode。
    assert_eq!(eval("Chr(33440)"), "あ");
}

#[test]
fn asc_chr_round_trip_japanese() {
    // Chr(Asc("あ")) は "あ" に戻る (VB6 SRC 互換性)。
    // SRC.Sharp は Chr が Unicode キャストなので round-trip しないが、
    // 本実装は SJIS 一貫性を維持。
    assert_eq!(eval(r#"Chr(Asc("あ"))"#), "あ");
}

#[test]
fn asc_halfwidth_katakana_returns_single_byte() {
    // SJIS 半角カナ "ｱ" は 0xB1 (177)。
    assert_eq!(eval(r#"Asc("ｱ")"#), "177");
}

#[test]
fn chr_halfwidth_katakana_byte_decodes_to_kana() {
    // Chr(0xB1) (= 177) は SJIS の半角 "ｱ"。
    assert_eq!(eval("Chr(177)"), "ｱ");
}

// ──────────────────────────────────────────────
// InStrRev(s1, s2 [, start])
// ──────────────────────────────────────────────

#[test]
fn instrrev_finds_last_occurrence() {
    // "hello" の最後の 'l' は 4 番目 (1-indexed)
    assert_eq!(eval(r#"InStrRev("hello", "l")"#), "4");
}

#[test]
fn instrrev_not_found_returns_zero() {
    assert_eq!(eval(r#"InStrRev("hello", "xyz")"#), "0");
}

#[test]
fn instrrev_multiple_occurrences() {
    // "ababab" で 'a' の最後は 5 番目
    assert_eq!(eval(r#"InStrRev("ababab", "a")"#), "5");
}

#[test]
fn instrrev_substring_match() {
    // "hellohello" で "lo" の最後は 9 番目
    assert_eq!(eval(r#"InStrRev("hellohello", "lo")"#), "9");
}

// ──────────────────────────────────────────────
// Existing functions: edge cases not yet exercised
// ──────────────────────────────────────────────

#[test]
fn left_zero_chars_returns_empty() {
    assert_eq!(eval(r#"Left("hello", 0)"#), "");
}

#[test]
fn right_zero_chars_returns_empty() {
    assert_eq!(eval(r#"Right("hello", 0)"#), "");
}

#[test]
fn left_longer_than_string_returns_full() {
    assert_eq!(eval(r#"Left("hello", 10)"#), "hello");
}

#[test]
fn right_longer_than_string_returns_full() {
    assert_eq!(eval(r#"Right("hello", 10)"#), "hello");
}

#[test]
fn mid_japanese_with_length() {
    assert_eq!(eval(r#"Mid("あいうえお", 2, 2)"#), "いう");
}

#[test]
fn mid_zero_length_returns_empty() {
    assert_eq!(eval(r#"Mid("hello", 2, 0)"#), "");
}

#[test]
fn len_japanese_string_returns_char_count() {
    assert_eq!(eval(r#"Len("あいう")"#), "3");
}

#[test]
fn len_empty_string_returns_zero() {
    assert_eq!(eval(r#"Len("")"#), "0");
}

#[test]
fn instr_at_start_returns_one() {
    assert_eq!(eval(r#"InStr("hello", "h")"#), "1");
}

#[test]
fn instr_at_end_returns_last_position() {
    assert_eq!(eval(r#"InStr("hello", "o")"#), "5");
}

#[test]
fn strcmp_both_empty_returns_zero() {
    assert_eq!(eval(r#"StrCmp("", "")"#), "0");
}

#[test]
fn strcmp_first_empty_returns_negative() {
    assert_eq!(eval(r#"StrCmp("", "a")"#), "-1");
}

#[test]
fn replace_no_match_returns_original() {
    assert_eq!(eval(r#"Replace("hello", "z", "X")"#), "hello");
}

#[test]
fn replace_all_occurrences() {
    assert_eq!(eval(r#"Replace("ababa", "a", "x")"#), "xbxbx");
}

// ============================================================
//  Replace with start / count (SRC.Sharp 準拠)
// ============================================================

#[test]
fn replace_with_start_replaces_from_position() {
    // Replace("abcabc","a","X",4) → "abcXbc"
    assert_eq!(eval(r#"Replace("abcabc","a","X",4)"#), "abcXbc");
}

#[test]
fn replace_with_start_at_beginning_replaces_all() {
    // Replace("aaa","a","b",1) → "bbb"
    assert_eq!(eval(r#"Replace("aaa","a","b",1)"#), "bbb");
}

#[test]
fn replace_with_start_and_count() {
    // Replace("abcabc","a","X",1,3) → "Xbcbc"
    assert_eq!(eval(r#"Replace("abcabc","a","X",1,3)"#), "Xbcbc");
}

#[test]
fn replace_with_count_zero() {
    // Replace("abc","a","X",1,0) → "bc"
    assert_eq!(eval(r#"Replace("abc","a","X",1,0)"#), "bc");
}

// ============================================================
//  IsNumeric — SRC.Sharp decimal.TryParse 互換エッジケース
// ============================================================

#[test]
fn isnumeric_scientific_notation_returns_zero() {
    // SRC.Sharp は decimal.TryParse 準拠 → 科学的表記法は false
    assert_eq!(eval(r#"IsNumeric("1e5")"#), "0");
    assert_eq!(eval(r#"IsNumeric("1E10")"#), "0");
}

#[test]
fn isnumeric_positive_sign_prefix_returns_one() {
    // "+5" は有効な数値
    assert_eq!(eval(r#"IsNumeric("+5")"#), "1");
}

#[test]
fn isnumeric_sign_only_returns_zero() {
    assert_eq!(eval(r#"IsNumeric("+")"#), "0");
    assert_eq!(eval(r#"IsNumeric("-")"#), "0");
}

#[test]
fn isnumeric_whitespace_only_returns_zero() {
    assert_eq!(eval(r#"IsNumeric("   ")"#), "0");
}

#[test]
fn isnumeric_with_surrounding_whitespace_returns_one() {
    // 前後の空白は trim されて有効な数値として処理
    assert_eq!(eval(r#"IsNumeric("  42  ")"#), "1");
}

#[test]
fn isnumeric_single_dot_returns_zero() {
    assert_eq!(eval(r#"IsNumeric(".")"#), "0");
}

#[test]
fn isnumeric_multiple_dots_returns_zero() {
    assert_eq!(eval(r#"IsNumeric("1.2.3")"#), "0");
}

#[test]
fn isnumeric_negative_float_returns_one() {
    assert_eq!(eval(r#"IsNumeric("-3.14")"#), "1");
}

// StrComp / StrCmp
#[test]
fn strcomp_equal_strings_returns_zero() {
    assert_eq!(eval(r#"StrComp("abc","abc")"#), "0");
}

#[test]
fn strcomp_less_than_returns_minus_one() {
    assert_eq!(eval(r#"StrComp("abc","abd")"#), "-1");
}

#[test]
fn strcomp_greater_than_returns_one() {
    assert_eq!(eval(r#"StrComp("abd","abc")"#), "1");
}

#[test]
fn strcomp_japanese_strings() {
    // "き" < "ま" (Unicode 順)
    assert_eq!(eval(r#"StrComp("きさらぎ","まい")"#), "-1");
}
