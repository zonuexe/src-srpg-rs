//! `Like` 演算子のテスト。
//!
//! SRC.Sharp `AdditionalFunctionTests` の Like ケースから移植。
//! VB6 の Like 演算子仕様: *, ?, #, [charlist], [!charlist]
//!
//! 主要な使用場面: `If $(var) Like "パターン" Then ...`

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn run(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

// ──────────────────────────────────────────────
// 基本マッチ (リテラル)
// ──────────────────────────────────────────────

#[test]
fn like_exact_match_true() {
    let app = run(r#"
Set r 0
If "hello" Like "hello" Then
    Set r 1
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

#[test]
fn like_exact_match_false() {
    let app = run(r#"
Set r 1
If "hello" Like "world" Then
    Set r 0
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

#[test]
fn like_case_sensitive() {
    // Like は大文字小文字を区別する
    let app = run(r#"
Set r 1
If "Hello" Like "hello" Then
    Set r 0
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

// ──────────────────────────────────────────────
// * ワイルドカード (任意の 0 文字以上)
// ──────────────────────────────────────────────

#[test]
fn like_asterisk_matches_any_sequence() {
    // "abcda" Like "a*a" → 1 (ヘルプ例)
    let app = run(r#"
Set r 0
If "abcda" Like "a*a" Then
    Set r 1
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

#[test]
fn like_asterisk_no_match() {
    // "abcde" Like "a*a" → 0 (ヘルプ例)
    let app = run(r#"
Set r 1
If "abcde" Like "a*a" Then
    Set r 0
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

#[test]
fn like_asterisk_suffix_match() {
    let app = run(r#"
Set r 0
If "abc123" Like "abc*" Then
    Set r 1
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

#[test]
fn like_asterisk_prefix_match() {
    let app = run(r#"
Set r 0
If "abc123" Like "*123" Then
    Set r 1
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

#[test]
fn like_asterisk_contains() {
    let app = run(r#"
Set r 0
If "abc123def" Like "*123*" Then
    Set r 1
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

#[test]
fn like_asterisk_only_matches_everything() {
    let app = run(r#"
Set r 0
If "anything here" Like "*" Then
    Set r 1
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

// ──────────────────────────────────────────────
// ? ワイルドカード (任意の 1 文字)
// ──────────────────────────────────────────────

#[test]
fn like_question_mark_matches_single_char() {
    let app = run(r#"
Set r 0
If "abc" Like "a?c" Then
    Set r 1
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

#[test]
fn like_question_mark_fails_on_multiple_chars() {
    let app = run(r#"
Set r 1
If "abbc" Like "a?c" Then
    Set r 0
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

// ──────────────────────────────────────────────
// # ワイルドカード (任意の 1 桁数字)
// ──────────────────────────────────────────────

#[test]
fn like_hash_matches_single_digit() {
    // "a2b" Like "a#b" → 1 (ヘルプ例)
    let app = run(r#"
Set r 0
If "a2b" Like "a#b" Then
    Set r 1
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

#[test]
fn like_hash_fails_on_non_digit() {
    let app = run(r#"
Set r 1
If "axb" Like "a#b" Then
    Set r 0
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

// ──────────────────────────────────────────────
// [charlist] 文字クラス
// ──────────────────────────────────────────────

#[test]
fn like_char_class_range_match() {
    // "D" Like "[A-Z]" → 1 (ヘルプ例)
    let app = run(r#"
Set r 0
If "D" Like "[A-Z]" Then
    Set r 1
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

#[test]
fn like_char_class_negation_outside_range() {
    // "D" Like "[!A-Z]" → 0 (ヘルプ例)
    let app = run(r#"
Set r 1
If "D" Like "[!A-Z]" Then
    Set r 0
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

#[test]
fn like_char_class_list_match() {
    let app = run(r#"
Set r 0
If "b" Like "[abc]" Then
    Set r 1
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

#[test]
fn like_char_class_list_no_match() {
    let app = run(r#"
Set r 1
If "z" Like "[abc]" Then
    Set r 0
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

// ──────────────────────────────────────────────
// 変数との組み合わせ
// ──────────────────────────────────────────────

#[test]
fn like_with_variable_in_condition() {
    let app = run(r#"
Set target "ブレイバーMkII"
Set r 0
If $(target) Like "ブレイバー*" Then
    Set r 1
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

#[test]
fn like_with_variable_no_match() {
    let app = run(r#"
Set target "イーグル1"
Set r 1
If $(target) Like "ブレイバー*" Then
    Set r 0
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}

// ──────────────────────────────────────────────
// ElseIf での使用
// ──────────────────────────────────────────────

#[test]
fn like_in_elseif() {
    let app = run(r#"
Set name "νブレイバー"
Set r 0
If $(name) Like "Ζ*" Then
    Set r 1
ElseIf $(name) Like "ν*" Then
    Set r 2
Else
    Set r 3
EndIf
"#);
    assert_eq!(app.script_var("r"), "2");
}

// ──────────────────────────────────────────────
// And / Or との組み合わせ
// ──────────────────────────────────────────────

#[test]
fn like_and_another_condition() {
    let app = run(r#"
Set name "ブレイバー改"
Set r 0
If $(name) Like "ブレイバー*" And Len($(name)) > 4 Then
    Set r 1
EndIf
"#);
    assert_eq!(app.script_var("r"), "1");
}
