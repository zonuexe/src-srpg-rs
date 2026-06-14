//! Call / Return / Goto / Exit の edge cases。

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
//  Goto / Label
// ============================================================

#[test]
fn goto_jumps_forward_skipping_intervening() {
    let app = run(r#"
Set step start
Goto end
Set step skipped
end:
Set step arrived
"#);
    assert_eq!(app.script_var("step"), "arrived");
}

#[test]
fn goto_backwards_creates_infinite_loop_guard() {
    // STEP_LIMIT (2,000,000) で打ち切られるべき (panic しない)
    let app = run(r#"
Set i 0
loop_start:
Incr i
If $(i) < 10 Then
  Goto loop_start
EndIf
"#);
    assert_eq!(app.script_var("i"), "10");
}

#[test]
fn goto_at_label_anchor_form() {
    // `@anchor` 形式
    let app = run(r#"
Goto onsen
Set unreachable yes
@onsen
Set reached yes
"#);
    assert_eq!(app.script_var("reached"), "yes");
    assert_eq!(app.script_var("unreachable"), "");
}

// ============================================================
//  Call / Return
// ============================================================

#[test]
fn call_executes_subroutine_and_returns() {
    let app = run(r#"
Set step before
Call greet
Set step after

Exit

greet:
Set step in_sub
Return
"#);
    assert_eq!(app.script_var("step"), "after");
}

#[test]
fn call_passes_args_via_args_var() {
    let app = run(r#"
Call setvars hello world

Exit

setvars:
Set a $(Args(1))
Set b $(Args(2))
Return
"#);
    assert_eq!(app.script_var("a"), "hello");
    assert_eq!(app.script_var("b"), "world");
}

#[test]
fn call_evaluates_parenthesized_arithmetic_args() {
    // SRC の `Call` は引数を式評価して渡す。単一の括弧式 (`(500 - 経験値)`)
    // は数値化される。非数値の括弧は文字列のまま。
    let app = run(r#"
Set 経験値 123
Call sub (500 - 経験値) (テキスト)
Exit

sub:
Set num $(Args(1))
Set txt $(Args(2))
Return
"#);
    assert_eq!(app.script_var("num"), "377");
    assert_eq!(app.script_var("txt"), "(テキスト)");
}

#[test]
fn nested_call() {
    let app = run(r#"
Call outer

Exit

outer:
Set step outer_in
Call inner
Set step outer_out
Return

inner:
Set step inner_only
Return
"#);
    assert_eq!(app.script_var("step"), "outer_out");
}

#[test]
fn return_without_call_terminates_script() {
    // Call スタックが空のとき Return は exit と同等
    let app = run(r#"
Set before 1
Return
Set after 1
"#);
    assert_eq!(app.script_var("before"), "1");
    assert_eq!(app.script_var("after"), "");
}

// ============================================================
//  Exit
// ============================================================

#[test]
fn exit_terminates_immediately() {
    let app = run(r#"
Set a 1
Exit
Set b 2
"#);
    assert_eq!(app.script_var("a"), "1");
    assert_eq!(app.script_var("b"), "");
}

#[test]
fn exit_from_inside_if_works() {
    let app = run(r#"
Set a 1
If 1 = 1 Then
  Exit
EndIf
Set b 2
"#);
    assert_eq!(app.script_var("a"), "1");
    assert_eq!(app.script_var("b"), "");
}

// ============================================================
//  Implicit Call (label name without `Call` keyword)
// ============================================================

#[test]
fn implicit_call_to_label_works() {
    let app = run(r#"
Set step before
greet
Set step after
Exit

greet:
Set step in_sub
Return
"#);
    assert_eq!(app.script_var("step"), "after");
}

#[test]
fn implicit_call_unknown_label_warns_silent_ok() {
    // typo / 未定義は silent OK (VB6 と同じ挙動)
    let app = run(r#"
Set a 1
Retunr
Set b 2
"#);
    // Set a と Set b 両方走る (Retunr は silent no-op)
    assert_eq!(app.script_var("a"), "1");
    assert_eq!(app.script_var("b"), "2");
}

// ============================================================
//  ArgNum システム変数
// ============================================================

#[test]
fn argnum_reflects_call_arg_count() {
    // Call サブルーチン内で ArgNum = 渡した引数の数
    let app = run(r#"
Call sub hello world foo

Exit

sub:
Set n $(ArgNum)
Return
"#);
    assert_eq!(app.script_var("n"), "3");
}

#[test]
fn argnum_zero_when_no_args() {
    // 引数なしで Call した場合は 0
    let app = run(r#"
Call sub

Exit

sub:
Set n $(ArgNum)
Return
"#);
    assert_eq!(app.script_var("n"), "0");
}

#[test]
fn argnum_restored_after_return() {
    // ネストした Call から Return 後、外側の ArgNum が復元される
    let app = run(r#"
Call outer a b

Exit

outer:
Set outer_n $(ArgNum)
Call inner x y z
Set outer_n_after $(ArgNum)
Return

inner:
Set inner_n $(ArgNum)
Return
"#);
    assert_eq!(app.script_var("outer_n"), "2");
    assert_eq!(app.script_var("inner_n"), "3");
    assert_eq!(app.script_var("outer_n_after"), "2");
}

#[test]
fn argnum_readonly_set_is_ignored() {
    // ArgNum は読み取り専用 — Set しても値が変わらない
    let app = run(r#"
Call sub x y

Exit

sub:
Set ArgNum 999
Set n $(ArgNum)
Return
"#);
    assert_eq!(app.script_var("n"), "2");
}

#[test]
fn argnum_one_arg() {
    let app = run(r#"
Call sub only_one

Exit

sub:
Set n $(ArgNum)
Return
"#);
    assert_eq!(app.script_var("n"), "1");
}

// ============================================================
//  UpVar
// ============================================================

/// UpVar: 引数なしサブルーチンが呼び出し元の引数を参照できる。
#[test]
fn upvar_zero_arg_sub_inherits_parent_args() {
    let app = run(r#"
Call Outer hello world

Exit

Outer:
Call GetFirst
Return

GetFirst:
UpVar
Set result $(Args(1))
Set num $(ArgNum)
Return
"#);
    assert_eq!(
        app.script_var("result"),
        "hello",
        "UpVar で親フレームの Args(1) を参照"
    );
    assert_eq!(app.script_var("num"), "2", "ArgNum = 親フレームの引数数");
}

/// UpVar: 引数ありサブルーチンは自分の引数を先頭に保持し、親の引数を後に追加する。
#[test]
fn upvar_with_own_args_appends_parent_args() {
    let app = run(r#"
Call Outer p1 p2

Exit

Outer:
Call GetWithOwn own_arg
Return

GetWithOwn:
UpVar
Set a1 $(Args(1))
Set a2 $(Args(2))
Set a3 $(Args(3))
Set num $(ArgNum)
Return
"#);
    assert_eq!(app.script_var("a1"), "own_arg", "Args(1) は自分の引数");
    assert_eq!(app.script_var("a2"), "p1", "Args(2) は親の Args(1)");
    assert_eq!(app.script_var("a3"), "p2", "Args(3) は親の Args(2)");
    assert_eq!(app.script_var("num"), "3", "ArgNum = 1(自分) + 2(親)");
}

/// UpVar: 呼び出し元が UpVar 済みの場合、その拡張済み引数が引き継がれる。
#[test]
fn upvar_chained_inherits_ancestor_through_parent() {
    // Sub1(a1, a2) → Sub2(b1)[UpVar] → Sub3(c1)[UpVar]
    // Sub2 は UpVar で [b1, a1, a2] に拡張される。
    // Sub3 は UpVar で [c1, b1, a1, a2] に拡張される。
    let app = run(r#"
Call Sub1 a1 a2

Exit

Sub1:
Call Sub2 b1
Return

Sub2:
UpVar
Call Sub3 c1
Return

Sub3:
UpVar
Set r1 $(Args(1))
Set r2 $(Args(2))
Set r3 $(Args(3))
Set r4 $(Args(4))
Set rn $(ArgNum)
Return
"#);
    assert_eq!(app.script_var("r1"), "c1", "Sub3 自身の引数");
    assert_eq!(app.script_var("r2"), "b1", "Sub2 の引数 (UpVar で拡張済み)");
    assert_eq!(app.script_var("r3"), "a1", "Sub1 の引数 1");
    assert_eq!(app.script_var("r4"), "a2", "Sub1 の引数 2");
    assert_eq!(app.script_var("rn"), "4", "ArgNum = 1+1+2");
}

/// UpVar: Return 後に呼び出し元の ArgNum が変化していないこと。
#[test]
fn upvar_does_not_affect_caller_after_return() {
    let app = run(r#"
Call Outer x y z

Exit

Outer:
Set before $(ArgNum)
Call Inner
Set after $(ArgNum)
Return

Inner:
UpVar
Return
"#);
    assert_eq!(app.script_var("before"), "3", "Outer のコール前 ArgNum=3");
    assert_eq!(
        app.script_var("after"),
        "3",
        "UpVar は呼び出し元の ArgNum に影響しない"
    );
}

// ============================================================
//  Call() in If conditions
// ============================================================

#[test]
fn call_in_if_condition_return_one_is_truthy() {
    let app = run(r#"
Set flag 1
If Call(check_flag) Then
  Set r yes
Else
  Set r no
EndIf
Exit

@check_flag:
If $(flag) = 1 Then
  Return 1
EndIf
Return 0
"#);
    assert_eq!(app.script_var("r"), "yes");
}

#[test]
fn call_in_if_condition_return_zero_is_falsy() {
    let app = run(r#"
Set flag 0
If Call(check_flag) Then
  Set r yes
Else
  Set r no
EndIf
Exit

@check_flag:
If $(flag) = 1 Then
  Return 1
EndIf
Return 0
"#);
    assert_eq!(app.script_var("r"), "no");
}

#[test]
fn call_in_if_condition_with_comparison() {
    let app = run(r#"
If Call(get_value) > 3 Then
  Set r big
Else
  Set r small
EndIf
Exit

@get_value:
Return 5
"#);
    assert_eq!(app.script_var("r"), "big");
}

/// UpVar: トップレベル (Call 外) での呼び出しは no-op。
#[test]
fn upvar_at_top_level_is_noop() {
    let app = run(r#"
UpVar
Set n $(ArgNum)
"#);
    // ArgNum はトップレベルでは "0" (N4 fix で対応済み)
    assert_eq!(app.script_var("n"), "0");
}
