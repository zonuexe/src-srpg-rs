//! Switch / Case のエッジケース。
//!
//! 実シナリオで頻出するパターン:
//! - 単一値 Case (`Case ユニット再表示`)
//! - 空白区切り複数値 (`Case 【START】 【EXハード】`)
//! - `Case Else` / `CaseElse` (default)
//! - Case の値は 1 つずつ式の項として評価して、文字列として比べる (原典 `ExecSwitchCmd`)
//! - ネスト Switch

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn run(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

#[test]
fn switch_single_case_match() {
    let app = run(r#"
Set x 2
Switch $(x)
Case 1
  Set v one
Case 2
  Set v two
Case 3
  Set v three
EndSw
"#);
    assert_eq!(app.script_var("v"), "two");
}

#[test]
fn switch_case_else_default() {
    let app = run(r#"
Set x 99
Switch $(x)
Case 1
  Set v one
Case 2
  Set v two
CaseElse
  Set v other
EndSw
"#);
    assert_eq!(app.script_var("v"), "other");
}

#[test]
fn switch_multiple_values_in_case() {
    // SRC: 空白区切りで複数値マッチ
    let app = run(r#"
Set x ハード
Switch $(x)
Case イージー ノーマル
  Set v easy_or_normal
Case ハード 超ハード
  Set v hard
EndSw
"#);
    assert_eq!(app.script_var("v"), "hard");
}

#[test]
fn switch_case_else_two_words() {
    // 原典は `Case Else` (引数 1 個が else) を CaseElse として扱う。
    let app = run(r#"
Set x 99
Switch x
Case 1
  Set v one
Case Else
  Set v other
EndSw
"#);
    assert_eq!(app.script_var("v"), "other");
}

#[test]
fn switch_case_values_are_evaluated_terms() {
    // Case の各値は変数・関数・括弧式として評価してから比べる。
    let app = run(r#"
Set x 6
Set 候補 6
Switch (x Mod 4)
Case 0 1
  Set v low
Case (1 + 1) 3
  Set v high
EndSw
Switch x
Case "5" 候補
  Set w var
EndSw
"#);
    assert_eq!(app.script_var("v"), "high");
    assert_eq!(app.script_var("w"), "var");
}

#[test]
fn switch_case_compares_as_strings() {
    // `Case 1 To 10` / `Case Is < 5` のような範囲・比較は原典に無い。各語が値として
    // 比べられるだけなので、`To` という値には一致する。
    let app = run(r#"
Set x To
Switch x
Case 1 To 10
  Set v word
CaseElse
  Set v other
EndSw
"#);
    assert_eq!(app.script_var("v"), "word");
}

#[test]
fn switch_case_number_literal_compares_as_written() {
    // 数値リテラルの Case 値は評価せず書かれたまま比べる (原典 `GetArg`)。
    let app = run(r#"
Set x 5
Set s "05"
Switch x
Case 05 5.0
  Set v padded
Case 5
  Set v plain
EndSw
Switch s
Case 5
  Set w plain
Case 05
  Set w padded
EndSw
"#);
    assert_eq!(app.script_var("v"), "plain");
    assert_eq!(app.script_var("w"), "padded");
}

#[test]
fn switch_string_value_case_match() {
    let app = run(r#"
Set name リオ
Switch $(name)
Case リオ
  Set v braver
Case ガロ
  Set v zolda
EndSw
"#);
    assert_eq!(app.script_var("v"), "braver");
}

#[test]
fn switch_no_match_no_assignment() {
    // どの case にもマッチしないなら body 実行されない
    let app = run(r#"
Set x 99
Switch $(x)
Case 1
  Set v one
Case 2
  Set v two
EndSw
"#);
    assert_eq!(app.script_var("v"), "");
}

#[test]
fn nested_switch() {
    let app = run(r#"
Set a 1
Set b 2
Switch $(a)
Case 1
  Switch $(b)
  Case 1
    Set v "1-1"
  Case 2
    Set v "1-2"
  EndSw
Case 2
  Set v "outer-2"
EndSw
"#);
    assert_eq!(app.script_var("v"), "1-2");
}
