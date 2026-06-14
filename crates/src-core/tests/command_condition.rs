//! `evaluate_command_condition` の `Call(<label>)` 形式テスト。
//!
//! `*ユニットコマンド name party [condition]:` の condition に
//! `Call(label)` を書いた場合、サブルーチンを同期実行して
//! `Return <value>` の値でコマンドの表示可否を判定することを確認する。
//!
//! SRC 原典の対応:
//!   - `Command.cs` L1113: `Expression.GetValueAsLong(ref argexpr)` が
//!     `Call(label)` 形式の評価を担う。
//!   - `parse_custom_command` は条件式をそのまま保存する (`Call(X)` ラッパー込み)。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

/// スクリプトライブラリにステートメントを登録した App を返すヘルパー。
fn app_with_lib(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    app.script_library_mut().append(&stmts);
    app
}

// ============================================================
//  Call(<label>) 形式の条件評価
// ============================================================

/// Return 1 で返すサブルーチン → 表示 (true)
#[test]
fn condition_call_returns_one_is_visible() {
    let mut app = app_with_lib(
        r#"
check_visible:
Return 1
"#,
    );
    assert!(event_runtime::evaluate_command_condition(
        &mut app,
        Some("Call(check_visible)")
    ));
}

/// Return 0 で返すサブルーチン → 非表示 (false)
#[test]
fn condition_call_returns_zero_is_hidden() {
    let mut app = app_with_lib(
        r#"
check_hidden:
Return 0
"#,
    );
    assert!(!event_runtime::evaluate_command_condition(
        &mut app,
        Some("Call(check_hidden)")
    ));
}

/// Return なし (Exit 扱い) → 返り値は空文字 → 非表示
#[test]
fn condition_call_no_return_is_hidden() {
    let mut app = app_with_lib(
        r#"
check_noop:
Set x 1
"#,
    );
    assert!(!event_runtime::evaluate_command_condition(
        &mut app,
        Some("Call(check_noop)")
    ));
}

/// 条件サブルーチンが変数を参照して Return する
#[test]
fn condition_call_uses_script_vars() {
    let mut app = app_with_lib(
        r#"
check_with_var:
Set result $(enabled)
Return $(result)
"#,
    );
    // enabled = "1" → 表示
    app.set_script_var("enabled".to_string(), "1".to_string());
    assert!(event_runtime::evaluate_command_condition(
        &mut app,
        Some("Call(check_with_var)")
    ));

    // enabled = "0" → 非表示
    app.set_script_var("enabled".to_string(), "0".to_string());
    assert!(!event_runtime::evaluate_command_condition(
        &mut app,
        Some("Call(check_with_var)")
    ));
}

/// 条件サブルーチンが条件分岐 (If) を使う
#[test]
fn condition_call_with_if_branch() {
    let mut app = app_with_lib(
        r#"
check_if:
If $(flag) = yes Then
  Return 1
EndIf
Return 0
"#,
    );
    app.set_script_var("flag".to_string(), "yes".to_string());
    assert!(event_runtime::evaluate_command_condition(
        &mut app,
        Some("Call(check_if)")
    ));

    app.set_script_var("flag".to_string(), "no".to_string());
    assert!(!event_runtime::evaluate_command_condition(
        &mut app,
        Some("Call(check_if)")
    ));
}

/// 未定義ラベルを `Call()` で呼んだ場合 → false (表示しない)
#[test]
fn condition_call_unknown_label_is_hidden() {
    let mut app = App::new();
    // スクリプトライブラリに何もない状態で Call(label) 呼び出し
    assert!(!event_runtime::evaluate_command_condition(
        &mut app,
        Some("Call(nonexistent_label)")
    ));
}

/// None 条件 → 常に表示
#[test]
fn condition_none_is_always_visible() {
    let mut app = App::new();
    assert!(event_runtime::evaluate_command_condition(&mut app, None));
}

/// 空条件 → 常に表示
#[test]
fn condition_empty_is_always_visible() {
    let mut app = App::new();
    assert!(event_runtime::evaluate_command_condition(
        &mut app,
        Some("")
    ));
}

/// 数値式 "1" → 表示
#[test]
fn condition_numeric_one_is_visible() {
    let mut app = App::new();
    assert!(event_runtime::evaluate_command_condition(
        &mut app,
        Some("1")
    ));
}

/// 数値式 "0" → 非表示
#[test]
fn condition_numeric_zero_is_hidden() {
    let mut app = App::new();
    assert!(!event_runtime::evaluate_command_condition(
        &mut app,
        Some("0")
    ));
}

// ============================================================
//  コールスタック・スコープの保全
// ============================================================

/// 条件評価後に script_vars が汚染されないことを確認
/// (ArgNum / Args(N) は Call フレーム内で変更されるが Return 後に復元される)
#[test]
fn condition_call_does_not_pollute_argnum() {
    let mut app = app_with_lib(
        r#"
check_argnum:
Return 1
"#,
    );
    // 事前に ArgNum を設定
    app.set_script_var("ArgNum".to_string(), "5".to_string());
    app.set_script_var("Args(1)".to_string(), "hello".to_string());

    event_runtime::evaluate_command_condition(&mut app, Some("Call(check_argnum)"));

    // 条件評価後、外側の Args/ArgNum が復元されていること
    assert_eq!(app.script_var("ArgNum"), "5");
    assert_eq!(app.script_var("Args(1)"), "hello");
}

/// 条件サブルーチン内での Set がスクリプト変数を変更することを確認
/// (サブルーチンのスコープは呼び出し元と共有)
#[test]
fn condition_call_can_set_vars() {
    let mut app = app_with_lib(
        r#"
set_side_effect:
Set side_effect_var done
Return 1
"#,
    );
    event_runtime::evaluate_command_condition(&mut app, Some("Call(set_side_effect)"));
    // script_vars は共有スコープ
    assert_eq!(app.script_var("side_effect_var"), "done");
}

/// 条件サブルーチンが別サブルーチンを Call してネストする
#[test]
fn condition_call_nested_subroutine() {
    let mut app = app_with_lib(
        r#"
outer_check:
Call inner_check
Return $(inner_result)

inner_check:
Set inner_result 1
Return
"#,
    );
    assert!(event_runtime::evaluate_command_condition(
        &mut app,
        Some("Call(outer_check)")
    ));
}

// ============================================================
//  .eve からの end-to-end: カスタムコマンド登録 + 条件評価
// ============================================================

/// `*ユニットコマンド` パース時に `Call(label)` が条件式ごと格納される
#[test]
fn parse_custom_command_preserves_call_condition() {
    use src_core::event_runtime::CustomCommandDef;
    let stmts = event::parse(
        r#"
*ユニットコマンド 乗せ換え 味方 Call(乗せ換え確認):
Set x 1
Exit
"#,
    )
    .expect("parse");
    let mut lib = src_core::event_runtime::ScriptLibrary::default();
    lib.append(&stmts);
    let cmds: Vec<&CustomCommandDef> = lib.custom_commands.iter().collect();
    assert_eq!(cmds.len(), 1);
    assert_eq!(
        cmds[0].condition.as_deref(),
        Some("Call(乗せ換え確認)"),
        "条件式は Call() ラッパーごと保存される"
    );
}
