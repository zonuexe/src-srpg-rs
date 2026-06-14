//! ファイル別 scoped label resolution / Per-file scoped label triggering.
//!
//! 多くのシナリオは複数 .eve に同名 `プロローグ:` ラベルを定義する
//! (主シナリオ + intermission cmd 用各種 lib)。グローバル `trigger_label`
//! は first-wins で alphabetically 早い側を返してしまうため、main entry の
//! プロローグを狙いたい場合は `trigger_label_in_file` を使う。
//!
//! 著作権配慮: SRC 原典コードは inline せず、合成 fixture のみ。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

/// 2 つのファイルが同名 `プロローグ:` を持つ場合、scoped trigger で
/// 意図したファイル側の body を実行できることを確認する。
#[test]
fn trigger_label_in_file_picks_specific_file() {
    const LIB: &str = r#"
プロローグ:
Set lib_prologue_ran 1
"#;
    const MAIN: &str = r#"
プロローグ:
Set main_prologue_ran 1
"#;

    let mut app = App::new();
    let lib_stmts = event::parse(LIB).expect("parse lib");
    app.script_library_mut()
        .append_with_name(&lib_stmts, "lib/intermission.eve");
    let main_stmts = event::parse(MAIN).expect("parse main");
    app.script_library_mut()
        .append_with_name(&main_stmts, "scenario.eve");

    // 通常 `trigger_label("プロローグ")` は最初に登録された側を選ぶ
    // (= lib/intermission.eve)。これは intended behavior。
    let lib_pc = app.script_library().label_pc("プロローグ");
    let lib_pc_specific = app
        .script_library()
        .label_pc_in_file("lib/intermission.eve", "プロローグ");
    assert_eq!(lib_pc, lib_pc_specific);

    // main scenario 側 `プロローグ` の PC は別個に取れる
    let main_pc = app
        .script_library()
        .label_pc_in_file("scenario.eve", "プロローグ");
    assert!(main_pc.is_some());
    assert_ne!(main_pc, lib_pc);

    // 個別 trigger で main 側だけ実行
    let fired = event_runtime::trigger_label_in_file(&mut app, "scenario.eve", "プロローグ");
    assert!(
        fired,
        "trigger_label_in_file(scenario.eve, プロローグ) 失敗"
    );
    assert_eq!(app.script_var("main_prologue_ran"), "1");
    assert_eq!(app.script_var("lib_prologue_ran"), "");
}

/// 同名ラベルが複数 .eve にある場合、`Call` は実行中ファイル内の定義を
/// 優先解決する。スパロボ戦記の `敵配置` が Main.eve と EventBattle.eve の
/// 双方にあり、Main.eve の `Call 敵配置` が EventBattle 側へ誤飛びしていた
/// 不具合の回帰防止。
#[test]
fn call_resolves_label_within_executing_file() {
    const FILE_A: &str = r#"
スタート:
Call 配置
Exit

配置:
Set a_placement_ran 1
Return
"#;
    const FILE_B: &str = r#"
配置:
Set b_placement_ran 1
Return
"#;

    let mut app = App::new();
    // B を先に登録 → フラットな labels は first-wins で B 側 `配置` を持つ。
    let b = event::parse(FILE_B).expect("parse B");
    app.script_library_mut()
        .append_with_name(&b, "battle_b.eve");
    let a = event::parse(FILE_A).expect("parse A");
    app.script_library_mut().append_with_name(&a, "main_a.eve");

    let fired = event_runtime::trigger_label_in_file(&mut app, "main_a.eve", "スタート");
    assert!(fired, "trigger_label_in_file(main_a.eve, スタート) 失敗");
    // 現ファイル (A) の `配置` が実行され、B 側は実行されない。
    assert_eq!(app.script_var("a_placement_ran"), "1");
    assert_eq!(app.script_var("b_placement_ran"), "");
}

#[test]
fn trigger_label_in_file_returns_false_for_unknown_file_or_label() {
    let mut app = App::new();
    let stmts = event::parse("プロローグ:\nSet ran 1\n").expect("parse");
    app.script_library_mut()
        .append_with_name(&stmts, "scenario.eve");

    // 未登録ファイル
    assert!(!event_runtime::trigger_label_in_file(
        &mut app,
        "nonexistent.eve",
        "プロローグ"
    ));
    assert_eq!(app.script_var("ran"), "");

    // 未定義ラベル
    assert!(!event_runtime::trigger_label_in_file(
        &mut app,
        "scenario.eve",
        "存在しないラベル"
    ));
    assert_eq!(app.script_var("ran"), "");

    // 正しい組合せ
    assert!(event_runtime::trigger_label_in_file(
        &mut app,
        "scenario.eve",
        "プロローグ"
    ));
    assert_eq!(app.script_var("ran"), "1");
}

/// basename 一致 (`scenario.eve` と `path/to/scenario.eve` が同等扱い)
#[test]
fn trigger_label_in_file_matches_by_basename() {
    let mut app = App::new();
    let stmts = event::parse("プロローグ:\nSet ran 1\n").expect("parse");
    app.script_library_mut()
        .append_with_name(&stmts, "path/to/scenario.eve");

    // basename だけで引ける
    assert!(event_runtime::trigger_label_in_file(
        &mut app,
        "scenario.eve",
        "プロローグ"
    ));
    assert_eq!(app.script_var("ran"), "1");
}
