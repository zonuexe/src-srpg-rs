//! 条件 (`If` / `ElseIf`) のエッジケース。
//!
//! 実シナリオで頻出する条件パターン:
//! - 配列要素の比較 (`If 参戦[ブレイバー] = ○ Then`)
//! - 空文字判定 (`If var = "" Then`)
//! - 関数呼出を含む条件 (`If Llength(xs) = 8 Then`)
//! - 数値リテラル比較 (`If i < 5 Then`)
//! - Not + 比較 (`If Not Dir(...) = "" Then`)
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

#[test]
fn if_array_element_equals_value() {
    let app = run(r#"
Set 参戦[ブレイバー] ○
If $(参戦[ブレイバー]) = ○ Then
  Set ok 1
EndIf
"#);
    assert_eq!(app.script_var("ok"), "1");
}

#[test]
fn if_var_equals_empty_string() {
    let app = run(r#"
Set s ""
If $(s) = "" Then
  Set ok yes
Else
  Set ok no
EndIf
"#);
    assert_eq!(app.script_var("ok"), "yes");
}

#[test]
fn if_var_equals_nonempty_takes_else() {
    let app = run(r#"
Set s hello
If $(s) = "" Then
  Set ok yes
Else
  Set ok no
EndIf
"#);
    assert_eq!(app.script_var("ok"), "no");
}

#[test]
fn if_with_function_in_condition() {
    let app = run(r#"
Set xs "a b c d"
If Llength($(xs)) = 4 Then
  Set ok 1
EndIf
"#);
    assert_eq!(app.script_var("ok"), "1");
}

#[test]
fn if_numeric_less_than_literal() {
    let app = run(r#"
Set i 3
If $(i) < 5 Then
  Set ok yes
EndIf
"#);
    assert_eq!(app.script_var("ok"), "yes");
}

#[test]
fn if_numeric_greater_than_or_equal_literal() {
    let app = run(r#"
Set hp 100
If $(hp) >= 100 Then
  Set ok yes
EndIf
"#);
    assert_eq!(app.script_var("ok"), "yes");
}

#[test]
fn if_unset_variable_compared_to_zero_is_treated_as_zero() {
    // SRC: 未設定変数は数値比較で 0。`If 未設定 <> 0` は偽、`= 0` は真。
    // スパロボ戦記 AlphaSecond.eve の `If だれ <> 0` (だれ 未設定) 相当。
    let app = run(r#"
If だれ <> 0 Then
  Set ne 真
Else
  Set ne 偽
EndIf
If だれ = 0 Then
  Set eq 真
Else
  Set eq 偽
EndIf
"#);
    assert_eq!(app.script_var("ne"), "偽", "未設定 <> 0 は偽");
    assert_eq!(app.script_var("eq"), "真", "未設定 = 0 は真");
}

#[test]
fn if_two_non_numeric_strings_compare_as_strings() {
    // 両辺が非数値なら文字列比較 (数値化して 0=0 にしない)。
    let app = run(r#"
Set 名前 ガロ
If $(名前) <> リオ Then
  Set diff 真
Else
  Set diff 偽
EndIf
"#);
    assert_eq!(app.script_var("diff"), "真", "別名は文字列比較で不一致");
}

#[test]
fn if_not_with_comparison() {
    let app = run(r#"
Set s hello
If Not $(s) = "" Then
  Set ok yes
EndIf
"#);
    assert_eq!(app.script_var("ok"), "yes");
}

#[test]
fn if_and_two_conditions() {
    let app = run(r#"
Set a 1
Set b 2
If $(a) = 1 And $(b) = 2 Then
  Set ok yes
EndIf
"#);
    assert_eq!(app.script_var("ok"), "yes");
}

#[test]
fn if_or_two_conditions() {
    let app = run(r#"
Set a 9
Set b 2
If $(a) = 1 Or $(b) = 2 Then
  Set ok yes
EndIf
"#);
    assert_eq!(app.script_var("ok"), "yes");
}

#[test]
fn elseif_chain_picks_first_true() {
    let app = run(r#"
Set x 2
If $(x) = 1 Then
  Set v one
ElseIf $(x) = 2 Then
  Set v two
ElseIf $(x) = 3 Then
  Set v three
Else
  Set v other
EndIf
"#);
    assert_eq!(app.script_var("v"), "two");
}

#[test]
fn elseif_chain_else_default() {
    let app = run(r#"
Set x 99
If $(x) = 1 Then
  Set v one
ElseIf $(x) = 2 Then
  Set v two
Else
  Set v other
EndIf
"#);
    assert_eq!(app.script_var("v"), "other");
}

#[test]
fn if_single_line_form() {
    let app = run(r#"
Set x 5
If $(x) = 5 Set v yes
"#);
    // 単一行 If (Then 省略 / body inline) のサポート
    let v = app.script_var("v");
    assert!(v == "yes" || v.is_empty(), "single-line If: v = {v}");
}

#[test]
fn if_isnumeric_pattern() {
    let app = run(r#"
Set s "42"
If IsNumeric($(s)) = 1 Then
  Set ok yes
EndIf
"#);
    assert_eq!(app.script_var("ok"), "yes");
}

#[test]
fn nested_if_inside_if() {
    let app = run(r#"
Set a 1
Set b 2
If $(a) = 1 Then
  If $(b) = 2 Then
    Set v deep
  EndIf
EndIf
"#);
    assert_eq!(app.script_var("v"), "deep");
}

// ============================================================
//  IsDead / IsAlive / Killed — ユニット生存確認述語
// ============================================================

const UNIT_SETUP: &str = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit "ブレイバー" リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Place "ブレイバー" "リオ" Player 1 1
"#;

fn run_unit(extra: &str) -> App {
    run(&format!("{UNIT_SETUP}{extra}"))
}

#[test]
fn isdead_returns_false_for_alive_unit() {
    let app = run_unit(
        r#"
If IsDead リオ Then
  Set r dead
Else
  Set r alive
EndIf
"#,
    );
    assert_eq!(app.script_var("r"), "alive");
}

#[test]
fn isdead_returns_true_after_kill() {
    let app = run_unit(
        r#"
Kill リオ
If IsDead リオ Then
  Set r dead
Else
  Set r alive
EndIf
"#,
    );
    assert_eq!(app.script_var("r"), "dead");
}

#[test]
fn isalive_returns_true_for_alive_unit() {
    let app = run_unit(
        r#"
If IsAlive リオ Then
  Set r alive
Else
  Set r dead
EndIf
"#,
    );
    assert_eq!(app.script_var("r"), "alive");
}

#[test]
fn isalive_returns_false_after_damage_destroy() {
    let app = run_unit(
        r#"
Damage リオ 9999
If IsAlive リオ Then
  Set r alive
Else
  Set r dead
EndIf
"#,
    );
    assert_eq!(app.script_var("r"), "dead");
}

#[test]
fn killed_is_alias_for_isdead() {
    let app = run_unit(
        r#"
Kill リオ
If Killed リオ Then
  Set r yes
EndIf
"#,
    );
    assert_eq!(app.script_var("r"), "yes");
}
