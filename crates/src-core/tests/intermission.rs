//! Intermission シーン + `IntermissionCommand` コマンドの edge cases。
//!
//! SRC.Sharp `Intermission.cs` / `IntermissionCommandCmd.cs` のセマンティクス:
//! - `IntermissionCommand <name> <file>` で項目登録 (既存上書き)
//! - `IntermissionCommand <name> 削除` で削除
//! - シーンには `Continue <file>` で `次ステージ` がセットされている場合に限り
//!   末尾に「次のステージへ」項目が出る
//! - 項目選択時の動作:
//!   - ユーザ定義: 該当 .eve の `プロローグ` ラベルを起動 (実行後はシーンに留まる)
//!   - 次のステージへ: `advance_to_next_stage` 経由で MapView へ遷移

use src_core::data::event;
use src_core::event_runtime;
use src_core::{App, Direction, Input, Scene};

fn make_app_with_script(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

// ──────────────────────────────────────────────
// IntermissionCommand 登録
// ──────────────────────────────────────────────

#[test]
fn intermission_command_registers_entry() {
    let app = make_app_with_script(r#"IntermissionCommand "キャラメイキング" "Lib\CMaking.eve""#);
    let cmds = app.intermission_commands();
    assert_eq!(cmds.len(), 1);
    assert_eq!(cmds[0].name, "キャラメイキング");
    assert_eq!(cmds[0].file, "Lib\\CMaking.eve");
}

#[test]
fn intermission_command_registers_in_order() {
    let app = make_app_with_script(
        r#"
IntermissionCommand 改造 "Lib\Remodeling.eve"
IntermissionCommand キャラメイキング "Lib\CMaking.eve"
IntermissionCommand ショップ "Lib\Shop.eve"
"#,
    );
    let names: Vec<&str> = app
        .intermission_commands()
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert_eq!(names, vec!["改造", "キャラメイキング", "ショップ"]);
}

#[test]
fn intermission_command_overwrite_existing() {
    // 同名を再登録するとファイルパスが更新され、件数は増えない。
    let app = make_app_with_script(
        r#"
IntermissionCommand 改造 "Lib\Old.eve"
IntermissionCommand 改造 "Lib\New.eve"
"#,
    );
    assert_eq!(app.intermission_commands().len(), 1);
    assert_eq!(app.intermission_commands()[0].file, "Lib\\New.eve");
}

#[test]
fn intermission_command_delete_removes_entry() {
    let app = make_app_with_script(
        r#"
IntermissionCommand 改造 "Lib\Remodeling.eve"
IntermissionCommand 改造 "削除"
"#,
    );
    assert_eq!(app.intermission_commands().len(), 0);
}

#[test]
fn intermission_command_delete_unknown_is_noop() {
    let app = make_app_with_script(
        r#"
IntermissionCommand 改造 "Lib\Remodeling.eve"
IntermissionCommand 存在しない "削除"
"#,
    );
    assert_eq!(app.intermission_commands().len(), 1);
}

#[test]
fn intermission_command_too_few_args_is_error() {
    // SRC.Sharp 準拠: 引数 1 個 (name のみ) はエラー。
    // IntermissionCommandCmd_WrongArgCount_ReturnsError 準拠
    let mut app = src_core::App::new();
    let stmts = src_core::data::event::parse("IntermissionCommand 改造\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(
        result.is_err(),
        "IntermissionCommand with 1 arg should error"
    );
}

// ──────────────────────────────────────────────
// intermission_item_count / labels
// ──────────────────────────────────────────────

#[test]
fn item_count_without_next_stage() {
    let app = make_app_with_script(
        r#"
IntermissionCommand A "a.eve"
IntermissionCommand B "b.eve"
"#,
    );
    // ユーザ項目 A,B + 組込み (機体改造 / データセーブ)。
    assert_eq!(app.intermission_item_count(), 4);
    assert_eq!(app.intermission_item_label(0).as_deref(), Some("A"));
    assert_eq!(app.intermission_item_label(1).as_deref(), Some("B"));
    assert_eq!(app.intermission_item_label(2).as_deref(), Some("機体改造"));
    assert_eq!(
        app.intermission_item_label(3).as_deref(),
        Some("データセーブ")
    );
    assert!(app.intermission_item_label(4).is_none());
}

#[test]
fn item_count_includes_next_stage_when_set() {
    let mut app = make_app_with_script(r#"IntermissionCommand A "a.eve""#);
    app.set_script_var("次ステージ".to_string(), "Main.eve".to_string());
    // ユーザ A + 組込み 2 つ + 次のステージへ = 4 項目。次のステージは末尾 (index 3)。
    assert_eq!(app.intermission_item_count(), 4);
    assert_eq!(
        app.intermission_item_label(3).as_deref(),
        Some("次のステージへ")
    );
}

#[test]
fn item_count_zero_when_no_entries_no_next() {
    let app = App::new();
    assert_eq!(app.intermission_item_count(), 0);
    assert!(app.intermission_item_label(0).is_none());
}

// ──────────────────────────────────────────────
// シーン遷移
// ──────────────────────────────────────────────

fn advance(app: &mut App) {
    app.handle_input(Input::Advance);
}

#[test]
fn configuration_to_intermission_when_commands_present() {
    let mut app = make_app_with_script(r#"IntermissionCommand A "a.eve""#);
    assert_eq!(app.scene(), Scene::Title);
    advance(&mut app); // Title → Configuration
    assert_eq!(app.scene(), Scene::Configuration);
    advance(&mut app); // Configuration → Intermission (登録があるので)
    assert_eq!(app.scene(), Scene::Intermission);
}

#[test]
fn configuration_to_mapview_when_no_commands() {
    let mut app = App::new();
    advance(&mut app); // Title → Configuration
    advance(&mut app); // Configuration → MapView (登録無しなので直行)
    assert_eq!(app.scene(), Scene::MapView);
}

#[test]
fn cursor_moves_with_up_down_in_intermission() {
    let mut app = make_app_with_script(
        r#"
IntermissionCommand A "a.eve"
IntermissionCommand B "b.eve"
IntermissionCommand C "c.eve"
"#,
    );
    advance(&mut app);
    advance(&mut app);
    assert_eq!(app.scene(), Scene::Intermission);
    // 項目 = [A, B, C, 機体改造, データセーブ] = 5 個。
    assert_eq!(app.intermission_cursor(), 0);
    app.handle_input(Input::MoveCursor(Direction::Down));
    assert_eq!(app.intermission_cursor(), 1);
    app.handle_input(Input::MoveCursor(Direction::Down));
    assert_eq!(app.intermission_cursor(), 2);
    app.handle_input(Input::MoveCursor(Direction::Down));
    assert_eq!(app.intermission_cursor(), 3);
    app.handle_input(Input::MoveCursor(Direction::Down));
    assert_eq!(app.intermission_cursor(), 4);
    app.handle_input(Input::MoveCursor(Direction::Down));
    // 末尾 (index 4) で 0 にラップ
    assert_eq!(app.intermission_cursor(), 0);
    app.handle_input(Input::MoveCursor(Direction::Up));
    assert_eq!(app.intermission_cursor(), 4);
}

// ──────────────────────────────────────────────
// 「次のステージへ」選択でシーン遷移
// ──────────────────────────────────────────────

// ──────────────────────────────────────────────
// Continue → Intermission シーン遷移
// ──────────────────────────────────────────────

#[test]
fn continue_with_intermission_commands_switches_to_intermission_scene() {
    // IntermissionCommand 登録済みなら、Continue は次ステージへ自動遷移せず
    // Scene::Intermission で停止する (スパロボ戦記 のインターミッション制)。
    let app = make_app_with_script(
        r#"
IntermissionCommand キャラメイク "Lib\CMaking.eve"
Continue "eve\Main.eve"
"#,
    );
    assert_eq!(app.scene(), Scene::Intermission);
    // 次ステージ は消費されず保持される (「次のステージへ」項目用)
    assert_eq!(app.script_var("次ステージ"), "eve\\Main.eve");
}

#[test]
fn continue_without_intermission_commands_keeps_scene() {
    // IntermissionCommand 登録が無ければ、Continue は従来通り scene を変えない。
    let app = make_app_with_script(r#"Continue "eve\Main.eve""#);
    assert_ne!(app.scene(), Scene::Intermission);
}

// ──────────────────────────────────────────────
// ユーザ定義項目の起動 → サブコマンド実行 → 復帰
// ──────────────────────────────────────────────

#[test]
fn user_command_runs_subcommand_and_returns_to_intermission() {
    let mut app = App::new();
    // サブコマンド .eve を名前付きで登録 (即完了する単純な内容)。
    let sub = event::parse("プロローグ:\nSet テスト 実行された\nExit\n").expect("parse");
    app.script_library_mut().append_with_name(&sub, "sub.eve");
    app.push_intermission_command("テスト項目".to_string(), "sub.eve".to_string());
    app.set_scene(Scene::Intermission);
    app.set_intermission_cursor(0);
    // Enter で項目確定
    app.handle_input(Input::Advance);
    // サブコマンドのスクリプトが実行された
    assert_eq!(app.script_var("テスト"), "実行された");
    // pause しない単純コマンドなので即 Intermission に復帰
    assert_eq!(app.scene(), Scene::Intermission);
}

#[test]
fn user_command_with_pause_stays_in_mapview_until_resolved() {
    let mut app = App::new();
    // Talk で pause するサブコマンド。
    let sub = event::parse("プロローグ:\nTalk システム\nこんにちは\nEnd\nExit\n").expect("parse");
    app.script_library_mut().append_with_name(&sub, "talk.eve");
    app.push_intermission_command("会話".to_string(), "talk.eve".to_string());
    app.set_scene(Scene::Intermission);
    app.set_intermission_cursor(0);
    app.handle_input(Input::Advance); // 確定 → サブコマンド起動
                                      // Talk で pause している間は MapView (サブコマンド描画用シーン)
    assert_eq!(app.scene(), Scene::MapView);
    assert!(app.pending_dialog().is_some());
    // ダイアログ応答 → サブコマンド完了 → Intermission に復帰
    app.respond_dialog(0);
    assert_eq!(app.scene(), Scene::Intermission);
    assert!(app.pending_dialog().is_none());
}

#[test]
fn next_stage_item_advances_to_mapview() {
    // 「次のステージへ」を Enter で確定 → advance_to_next_stage → MapView。
    let mut app = make_app_with_script(
        r#"
IntermissionCommand A "a.eve"

エピローグ:
Exit
"#,
    );
    // 次ステージをセット (本来は Continue が行う)。ラベル "エピローグ" を起動できる。
    app.set_script_var("次ステージ".to_string(), "エピローグ".to_string());
    advance(&mut app); // Title → Configuration
    advance(&mut app); // Configuration → Intermission
                       // [A, 機体改造, データセーブ, 次のステージへ] → 末尾 (index 3) へ移動。
    app.handle_input(Input::MoveCursor(Direction::Down));
    app.handle_input(Input::MoveCursor(Direction::Down));
    app.handle_input(Input::MoveCursor(Direction::Down));
    assert_eq!(app.intermission_cursor(), 3);
    advance(&mut app); // 確定 → advance_to_next_stage で エピローグ 発火 + MapView へ
    assert_eq!(app.scene(), Scene::MapView);
    // 次ステージ は使用後にクリアされる
    assert!(app.script_var("次ステージ").is_empty());
}
