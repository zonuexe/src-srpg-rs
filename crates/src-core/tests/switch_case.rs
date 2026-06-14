//! Switch / Case のエッジケース。
//!
//! 実シナリオで頻出するパターン:
//! - 単一値 Case (`Case ユニット再表示`)
//! - 空白区切り複数値 (`Case 【START】 【EXハード】`)
//! - 範囲 Case (`Case 1 To 10`)
//! - 比較演算子 Case (`Case Is < 5`)
//! - CaseElse (default)
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
fn switch_range_case_to_keyword() {
    // SRC: `Case N To M` で範囲マッチ
    let app = run(r#"
Set x 5
Switch $(x)
Case 1 To 10
  Set v in_range
Case 11 To 20
  Set v out_range
EndSw
"#);
    assert_eq!(app.script_var("v"), "in_range");
}

#[test]
fn switch_case_is_comparison() {
    // SRC: `Case Is < N` で比較
    let app = run(r#"
Set x 3
Switch $(x)
Case Is < 5
  Set v less
CaseElse
  Set v other
EndSw
"#);
    assert_eq!(app.script_var("v"), "less");
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
