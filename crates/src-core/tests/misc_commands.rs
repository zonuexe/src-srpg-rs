//! Tests for commands that were previously no-ops:
//! `SetStock`, `Sunset`/`Noon`/`Night`, `ChangeUnitBitmap`, `ChangeUnitClass`,
//! `ChangePilotBitmap`, `Debug`, `Quit`, `Pause`.

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

/// フィクスチャ: パイロット + ユニット + 配置。
const SETUP: &str = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Feature "ブレイバー" "ヒーリング" 1
Weapon "ブレイバー" "ライフル" 2500 2 5 15 -1
Place "ブレイバー" "リオ" Player 0 0
"#;

fn run(extra: &str) -> App {
    let mut app = App::new();
    let src = format!("{SETUP}{extra}");
    let stmts = event::parse(&src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

// ============================================================
//  SetStock
// ============================================================

fn ability_stock(app: &App, unit_name: &str, ability_name: &str) -> Option<i32> {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == unit_name)
        .and_then(|u| u.ability_stocks.get(ability_name).copied())
}

#[test]
fn setstock_sets_ability_remaining_by_name() {
    let app = run("SetStock リオ ヒーリング 4\n");
    assert_eq!(ability_stock(&app, "ブレイバー", "ヒーリング"), Some(4));
}

#[test]
fn setstock_overwrites_existing_stock() {
    // 同じアビリティに複数回 SetStock → 最新値が残る
    let app = run("SetStock リオ ヒーリング 4\nSetStock リオ ヒーリング 2\n");
    assert_eq!(ability_stock(&app, "ブレイバー", "ヒーリング"), Some(2));
}

#[test]
fn setstock_arbitrary_name_is_stored() {
    // アビリティ名はフリーキー — 存在しない feature 名でも格納できる
    let app = run("SetStock リオ 存在しない 5\n");
    assert_eq!(ability_stock(&app, "ブレイバー", "存在しない"), Some(5));
}

// ============================================================
//  Sunset / Noon / Night (時間帯)
// ============================================================

#[test]
fn noon_sets_time_of_day_to_hiruyu() {
    let app = run("Noon\n");
    assert_eq!(app.time_of_day(), "昼");
}

#[test]
fn sunset_sets_time_of_day_to_yuu() {
    let app = run("Sunset\n");
    assert_eq!(app.time_of_day(), "夕");
}

#[test]
fn night_sets_time_of_day_to_yoru() {
    let app = run("Night\n");
    assert_eq!(app.time_of_day(), "夜");
}

#[test]
fn time_of_day_reflected_in_info_function() {
    // Info(マップ, 時間帯) が time_of_day を返す
    let app = run("Sunset\nSet tod $(Info(マップ, 時間帯))\n");
    assert_eq!(app.script_var("tod"), "夕");
}

#[test]
fn time_of_day_can_cycle() {
    let app = run("Night\nNoon\nSunset\n");
    assert_eq!(app.time_of_day(), "夕");
}

// ============================================================
//  ChangeUnitBitmap
// ============================================================

fn unit_bitmap_override(app: &App, unit_name: &str) -> Option<String> {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == unit_name)
        .and_then(|u| u.bitmap_override.clone())
}

fn unit_bitmap_hidden(app: &App, unit_name: &str) -> bool {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == unit_name)
        .map(|u| u.is_bitmap_hidden)
        .unwrap_or(false)
}

#[test]
fn change_unit_bitmap_sets_override() {
    let app = run("ChangeUnitBitmap リオ braver_alt\n");
    assert_eq!(
        unit_bitmap_override(&app, "ブレイバー"),
        Some("braver_alt".to_string())
    );
}

#[test]
fn change_unit_bitmap_dash_clears_override() {
    let app = run("ChangeUnitBitmap リオ braver_alt\nChangeUnitBitmap リオ -\n");
    assert_eq!(unit_bitmap_override(&app, "ブレイバー"), None);
}

#[test]
fn change_unit_bitmap_hide_sets_hidden() {
    let app = run("ChangeUnitBitmap リオ 非表示\n");
    assert!(unit_bitmap_hidden(&app, "ブレイバー"));
}

#[test]
fn change_unit_bitmap_unhide_clears_hidden() {
    let app = run("ChangeUnitBitmap リオ 非表示\nChangeUnitBitmap リオ 非表示解除\n");
    assert!(!unit_bitmap_hidden(&app, "ブレイバー"));
}

// ============================================================
//  ChangeUnitClass
// ============================================================

fn unit_class_override(app: &App, unit_name: &str) -> Option<String> {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == unit_name)
        .and_then(|u| u.class_override.clone())
}

#[test]
fn change_unit_class_sets_override() {
    let app = run("ChangeUnitClass リオ スーパー系\n");
    assert_eq!(
        unit_class_override(&app, "ブレイバー"),
        Some("スーパー系".to_string())
    );
}

#[test]
fn change_unit_class_dash_clears_override() {
    let app = run("ChangeUnitClass リオ スーパー系\nChangeUnitClass リオ -\n");
    assert_eq!(unit_class_override(&app, "ブレイバー"), None);
}

// ============================================================
//  ChangePilotBitmap
// ============================================================
// PilotInstance は通常空なので script_var `__pilot_bitmap_<name>` で確認。

#[test]
fn change_pilot_bitmap_sets_script_var() {
    let app = run("ChangePilotBitmap リオ amuro_alt\n");
    assert_eq!(app.script_var("__pilot_bitmap_リオ"), "amuro_alt");
}

#[test]
fn change_pilot_bitmap_dash_clears_script_var() {
    let app = run("ChangePilotBitmap リオ amuro_alt\nChangePilotBitmap リオ -\n");
    assert_eq!(app.script_var("__pilot_bitmap_リオ"), "");
}

// ============================================================
//  Debug コマンド
// ============================================================

#[test]
fn debug_pushes_to_messages() {
    let app = run("Debug テストメッセージ\n");
    assert!(
        app.messages()
            .iter()
            .any(|m| m.contains("テストメッセージ")),
        "messages = {:?}",
        app.messages()
    );
}

// ============================================================
//  Quit コマンド (Suspend 相当)
// ============================================================

#[test]
fn quit_sets_scene_to_title() {
    use src_core::Scene;
    let mut app = App::new();
    let src = format!("{SETUP}Quit\n");
    let stmts = event::parse(&src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    assert_eq!(app.scene(), Scene::Title);
}

// ============================================================
//  Option コマンド
// ============================================================

#[test]
fn option_sets_global_variable() {
    // `Option 乗り換え` → `Option(乗り換え)` = 1
    let app = run("Option 乗り換え\n");
    assert_eq!(app.script_var("Option(乗り換え)"), "1");
}

#[test]
fn option_release_undefines_variable() {
    // `Option 乗り換え 解除` → `Option(乗り換え)` が削除される (SRC.Sharp: UndefineVariable)
    let app = run("Option 乗り換え\nOption 乗り換え 解除\n");
    // 削除後は未定義 → script_var は "" を返す
    assert_eq!(app.script_var("Option(乗り換え)"), "");
}

#[test]
fn option_multiple_options_independent() {
    let app = run("Option 乗り換え\nOption アイテム交換\n");
    assert_eq!(app.script_var("Option(乗り換え)"), "1");
    assert_eq!(app.script_var("Option(アイテム交換)"), "1");
    // 設定していないオプションは空 (= 未定義)
    assert_eq!(app.script_var("Option(デバッグ)"), "");
}

#[test]
fn option_can_be_read_with_isvardefined() {
    let app = run("Option 乗り換え\nSet v IsVarDefined(Option(乗り換え))\n");
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn option_release_isvardefined_returns_zero() {
    // 解除後は変数が削除される → IsVarDefined は 0 を返す
    let app = run("Option 乗り換え\nOption 乗り換え 解除\nSet v IsVarDefined(Option(乗り換え))\n");
    assert_eq!(app.script_var("v"), "0");
}

// ============================================================
//  SetWindowFrameWidth
// ============================================================

#[test]
fn setwindowframewidth_stores_script_var() {
    // SRC.Sharp `SetWindowFrameWidthCmd.cs`: `StatusWindow(FrameWidth)` にフレーム幅を格納。
    let app = run("SetWindowFrameWidth 3\n");
    assert_eq!(app.script_var("StatusWindow(FrameWidth)"), "3");
}

#[test]
fn setwindowframewidth_overwrite() {
    let app = run("SetWindowFrameWidth 5\nSetWindowFrameWidth 1\n");
    assert_eq!(app.script_var("StatusWindow(FrameWidth)"), "1");
}

// ============================================================
//  SetWindowColor
// ============================================================

#[test]
fn setwindowcolor_no_target_sets_both_vars() {
    // `SetWindowColor #ff0000` → FrameColor と BackBolor (typo 踏襲) 両方を設定。
    let app = run("SetWindowColor #ff0000\n");
    // #ff0000 (R=255,G=0,B=0) → COLORREF = (B<<16)|(G<<8)|R = 0x0000FF = 255
    assert_eq!(app.script_var("StatusWindow(FrameColor)"), "255");
    assert_eq!(app.script_var("StatusWindow(BackBolor)"), "255");
}

#[test]
fn setwindowcolor_waku_target_sets_frame_only() {
    let app = run("SetWindowColor #00ff00 枠\n");
    // #00ff00 (R=0,G=255,B=0) → COLORREF = (0<<16)|(255<<8)|0 = 65280
    assert_eq!(app.script_var("StatusWindow(FrameColor)"), "65280");
    assert_eq!(app.script_var("StatusWindow(BackBolor)"), ""); // not set
}

#[test]
fn setwindowcolor_background_target_sets_bg_only() {
    let app = run("SetWindowColor #0000ff 背景\n");
    // #0000ff (R=0,G=0,B=255) → COLORREF = (255<<16)|(0<<8)|0 = 16711680
    assert_eq!(app.script_var("StatusWindow(BackBolor)"), "16711680");
    assert_eq!(app.script_var("StatusWindow(FrameColor)"), ""); // not set
}

#[test]
fn setwindowcolor_blue_colorref_conversion() {
    // COLORREF は Windows BGR 形式: #0000ff → 16711680 (0xFF0000)
    let app = run("SetWindowColor #0000ff\n");
    assert_eq!(app.script_var("StatusWindow(FrameColor)"), "16711680");
}

// ============================================================
//  SetStatusStringColor
// ============================================================

#[test]
fn setstatusstringcolor_normal_sets_string_color() {
    // `SetStatusStringColor #FF0000 通常` → `StatusWindow(StringColor)` = 255 (COLORREF)
    // #FF0000 (R=255,G=0,B=0) → COLORREF=(B<<16)|(G<<8)|R = 0x0000FF = 255
    let app = run("SetStatusStringColor #FF0000 通常\n");
    assert_eq!(app.script_var("StatusWindow(StringColor)"), "255");
}

#[test]
fn setstatusstringcolor_ability_name_sets_aname_color() {
    // `SetStatusStringColor #00FF00 能力名` → `StatusWindow(ANameColor)` = 65280
    // #00FF00 (R=0,G=255,B=0) → COLORREF=(0<<16)|(255<<8)|0 = 65280
    let app = run("SetStatusStringColor #00FF00 能力名\n");
    assert_eq!(app.script_var("StatusWindow(ANameColor)"), "65280");
}

#[test]
fn setstatusstringcolor_enable_sets_enable_color() {
    // `SetStatusStringColor #0000FF 有効` → `StatusWindow(EnableColor)` = 16711680
    // #0000FF (R=0,G=0,B=255) → COLORREF=(255<<16)|(0<<8)|0 = 16711680
    let app = run("SetStatusStringColor #0000FF 有効\n");
    assert_eq!(app.script_var("StatusWindow(EnableColor)"), "16711680");
}

#[test]
fn setstatusstringcolor_disable_sets_disable_color() {
    // `SetStatusStringColor #FFFFFF 無効` → `StatusWindow(DisableColor)` = 16777215
    // #FFFFFF → COLORREF = (255<<16)|(255<<8)|255 = 16777215
    let app = run("SetStatusStringColor #FFFFFF 無効\n");
    assert_eq!(app.script_var("StatusWindow(DisableColor)"), "16777215");
}

#[test]
fn setstatusstringcolor_does_not_affect_other_vars() {
    // 通常 を設定しても ANameColor 等には影響しない
    let app = run("SetStatusStringColor #FF0000 通常\n");
    assert_eq!(app.script_var("StatusWindow(ANameColor)"), "");
    assert_eq!(app.script_var("StatusWindow(EnableColor)"), "");
    assert_eq!(app.script_var("StatusWindow(DisableColor)"), "");
}

// ============================================================
//  Talk ダッシュ正規化 (FormatMessage)
// ============================================================

fn first_message(app: &App) -> String {
    app.messages().first().cloned().unwrap_or_default()
}

#[test]
fn talk_em_dash_pair_normalized() {
    // `――` (em-dash × 2) → `──` (box-drawing horizontal × 2)
    // SRC.Sharp `Expression.replace.cs` `FormatMessage` 準拠
    let app = run("Talk テスト\nテスト――テスト\nEnd\n");
    let msg = first_message(&app);
    assert!(msg.contains("──"), "msg = {msg:?}");
    assert!(
        !msg.contains("――"),
        "EM ダッシュが残っていない: msg = {msg:?}"
    );
}

#[test]
fn talk_katakana_choonpu_pair_normalized() {
    // `ーー` (katakana prolonged sound mark × 2) → `──`
    let app = run("Talk テスト\nテストーーテスト\nEnd\n");
    let msg = first_message(&app);
    assert!(msg.contains("──"), "msg = {msg:?}");
}

#[test]
fn talk_mixed_dash_pair_normalized() {
    // `─―` (U+2500 + U+2015) → `──`
    let app = run("Talk テスト\nテスト─―テスト\nEnd\n");
    let msg = first_message(&app);
    assert!(msg.contains("──"), "msg = {msg:?}");
}

#[test]
fn talk_single_dash_not_normalized() {
    // 単独のダッシュは変換しない
    let app = run("Talk テスト\nテスト―テスト\nEnd\n");
    let msg = first_message(&app);
    // 「─テスト」でも「──テスト」でも「―テスト」でもなく、
    // 単独ダッシュ (U+2015 → U+2500 に正規化されるが 2 文字ではないので ── にはならない)
    assert!(
        !msg.contains("──"),
        "単独ダッシュが変換された: msg = {msg:?}"
    );
}

// ============================================================
//  Question コマンド: 選択肢なし → 選択 = "0"
// ============================================================

#[test]
fn question_no_choices_sets_sentaku_to_zero() {
    // `Question 5` に選択肢がないとき、選択="0" に設定して続行
    // SRC.Sharp 準拠: QuestionCmdTests.QuestionNoChoicesTest
    let app = run_raw("Question 5\nEnd\n");
    assert_eq!(app.script_var("選択"), "0");
}

// ============================================================
//  Ask コマンド: 選択肢なし・デフォルトプロンプト・終了
// ============================================================

fn run_raw(src: &str) -> App {
    let mut app = App::new();
    let stmts = src_core::data::event::parse(src).expect("parse");
    src_core::event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

#[test]
fn ask_no_choices_sets_sentaku_to_zero() {
    // `Ask` の後に `End` だけ → 選択肢なし → 選択 = "0"
    // SRC.Sharp 準拠: AskCmd_NoChoices_SetsSelectedAlternativeToZero
    let app = run_raw("Ask メッセージ\nEnd\n");
    assert_eq!(app.script_var("選択"), "0");
}

#[test]
fn ask_owari_returns_immediately() {
    // `Ask 終了` は特殊ターミネータ: 後続行を読まずに進む
    // SRC.Sharp 準拠: AskCmd_Owari_ClosesListBox
    // 選択肢なし終了と同じく選択="0" は設定されない (ターミネータなのでスキップ)
    let app = run_raw("Set x before\nAsk 終了\nSet x after\n");
    // Ask 終了 は後続 Set x after まで実行する (間にブロックがないので続行)
    assert_eq!(app.script_var("x"), "after");
}

// ============================================================
//  Cancel: ダイアログをキャンセル
// ============================================================

#[test]
fn cancel_sets_sentaku_to_zero() {
    // Cancel は 選択 = "0" にする。
    // (pending_dialog は既にクリア済みの状態でも動作する)
    let app = run_raw("Set 選択 5\nCancel\n");
    assert_eq!(app.script_var("選択"), "0", "Cancel で選択 = 0");
}

// ============================================================
//  Confirm: 引数数バリデーション
// ============================================================

#[test]
fn confirm_zero_args_is_error() {
    // SRC.Sharp 準拠: ConfirmCmd_WrongArgCount_ReturnsError
    let mut app = App::new();
    let stmts = src_core::data::event::parse("Confirm\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "Confirm with 0 args should error");
}

#[test]
fn confirm_two_args_is_error() {
    // SRC.Sharp 準拠: ConfirmCmd_TooManyArgs_ReturnsError
    let mut app = App::new();
    let stmts = src_core::data::event::parse("Confirm メッセージ 余分な引数\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "Confirm with 2 args should error");
}

// ============================================================
//  Input: 引数数バリデーション
// ============================================================

#[test]
fn input_one_arg_is_error() {
    // SRC.Sharp 準拠: InputCmd_WrongArgCount_ReturnsError
    let mut app = App::new();
    let stmts = src_core::data::event::parse("Input myVar\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "Input with 1 arg should error");
}

#[test]
fn input_four_args_is_error() {
    // SRC.Sharp 準拠: InputCmd_TooManyArgs_ReturnsError
    let mut app = App::new();
    let stmts = src_core::data::event::parse("Input myVar msg default extra\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "Input with 4 args should error");
}

// ============================================================
//  Switch: 引数なし → エラー
// ============================================================

#[test]
fn switch_no_args_is_error() {
    // SRC.Sharp 準拠: SwitchCmd_WrongArgCount_ReturnsError
    let mut app = App::new();
    let stmts = src_core::data::event::parse("Switch\nCaseElse\nEndSw\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "Switch with no args should error");
}

// ============================================================
//  Next: 対応する For なし → エラー
// ============================================================

#[test]
fn next_without_for_is_error() {
    // SRC.Sharp 準拠: NextCmd_MissingFor_ReturnsError
    let mut app = App::new();
    let stmts = src_core::data::event::parse("Next\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "Next without For should error");
}

// ============================================================
//  Wait: 引数なし / 2 引数 → エラー
// ============================================================

#[test]
fn wait_no_args_is_error() {
    // SRC.Sharp 準拠: WaitCmd_WrongArgCount_ReturnsError
    let mut app = App::new();
    let stmts = src_core::data::event::parse("Wait\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "Wait with no args should error");
}

#[test]
fn wait_two_args_is_error() {
    // SRC.Sharp 準拠: WaitCmd_FourArgs_ReturnsError (Wait Until 1 2)
    let mut app = App::new();
    let stmts = src_core::data::event::parse("Wait 1 2\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "Wait with 2 args should error");
}

// ============================================================
//  Win/GameClear + Lose/GameOver: 引数ありはエラー
// ============================================================

#[test]
fn gameclear_with_arg_is_error() {
    // SRC.Sharp 準拠: GameClearCmd_WrongArgCount_ReturnsError
    let mut app = App::new();
    let stmts = src_core::data::event::parse("GameClear extra\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "GameClear with arg should error");
}

#[test]
fn gameover_with_arg_is_error() {
    // SRC.Sharp 準拠: GameOverCmd_WrongArgCount_ReturnsError
    let mut app = App::new();
    let stmts = src_core::data::event::parse("GameOver extra\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "GameOver with arg should error");
}

// ============================================================
//  Swap: 2 引数超はエラー
// ============================================================

#[test]
fn swap_three_args_is_error() {
    // SRC.Sharp 準拠: SwapCmd_TooManyArgs_ReturnsError
    let mut app = App::new();
    let stmts = src_core::data::event::parse("Set a 1\nSet b 2\nSwap a b c\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "Swap with 3 args should error");
}

// ============================================================
//  Unset: 引数なし → エラー
// ============================================================

#[test]
fn unset_no_args_is_error() {
    // SRC.Sharp 準拠: UnSetCmd_WrongArgCount_ReturnsError
    let mut app = App::new();
    let stmts = src_core::data::event::parse("Unset\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "Unset with no args should error");
}

// ============================================================
//  RenameTerm: 2 引数以外はエラー
// ============================================================

#[test]
fn renameterm_one_arg_is_error() {
    // SRC.Sharp 準拠: RenameTermCmd_WrongArgCount_ReturnsError
    let mut app = App::new();
    let stmts = src_core::data::event::parse("RenameTerm スペシャルパワー\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "RenameTerm with 1 arg should error");
}

// ============================================================
//  Do/Loop 無効キーワード → エラー
// ============================================================

#[test]
fn do_invalid_keyword_is_error() {
    // SRC.Sharp 準拠: DoCmd_WrongArgCount_ReturnsError
    let mut app = App::new();
    let stmts = src_core::data::event::parse("Do Invalid 1\nLoop\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "Do with invalid keyword should error");
}

#[test]
fn loop_invalid_keyword_is_error() {
    // SRC.Sharp 準拠: LoopCmd_WrongArgCount_ReturnsError
    let mut app = App::new();
    let stmts = src_core::data::event::parse("Do\nLoop Invalid 1\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "Loop with invalid keyword should error");
}

// ============================================================
//  Select — 選択ユニット設定 (C# SelectCmd.cs)
// ============================================================

#[test]
fn select_sets_selected_unit_for_event() {
    // C# Select unit → Event.SelectedUnitForEvent = unit。
    // 引数 1: unit 識別子 (pilot 名 / unit_data 名)。
    let app = run(r#"Select リオ
Set v $(相手パイロット)
"#);
    // selected_unit_for_event は script_var ではなく内部フィールドなので
    // 直接確認する。
    assert_eq!(app.selected_unit_for_event(), "リオ");
}

#[test]
fn select_wrong_arg_count_is_error() {
    // C# SelectCmd.cs: ArgNum != 2 → EventErrorException。
    let mut app = App::new();
    let result = src_core::event_runtime::execute(
        &mut app,
        &src_core::data::event::parse("Select\n").unwrap(),
    );
    assert!(result.is_err(), "Select with 0 args should error");
    let result2 = src_core::event_runtime::execute(
        &mut app,
        &src_core::data::event::parse("Select リオ ブレイバー\n").unwrap(),
    );
    assert!(result2.is_err(), "Select with 2 args should error");
}

// ============================================================
//  SelectTarget — 相手ターゲット設定 (C# SelectTargetCmd.cs)
// ============================================================

#[test]
fn selecttarget_sets_opponent_vars() {
    // SelectTarget unit → `相手パイロット` / `相手ユニットＩＤ` をセット。
    let app = run("SelectTarget リオ\n");
    assert_eq!(app.script_var("相手パイロット"), "リオ");
}

#[test]
fn selecttarget_wrong_arg_count_is_error() {
    // C# SelectTargetCmd.cs: ArgNum != 2 → EventErrorException。
    let mut app = App::new();
    let result = src_core::event_runtime::execute(
        &mut app,
        &src_core::data::event::parse("SelectTarget\n").unwrap(),
    );
    assert!(result.is_err(), "SelectTarget with 0 args should error");
    let result2 = src_core::event_runtime::execute(
        &mut app,
        &src_core::data::event::parse("SelectTarget リオ ブレイバー\n").unwrap(),
    );
    assert!(result2.is_err(), "SelectTarget with 2 args should error");
}

// ============================================================
//  Money arg count validation (C# MoneyCmd.cs)
// ============================================================

#[test]
fn money_zero_args_is_error() {
    // C# MoneyCmd.cs: ArgNum != 2 → EventErrorException。
    let mut app = App::new();
    let result = src_core::event_runtime::execute(
        &mut app,
        &src_core::data::event::parse("Money\n").unwrap(),
    );
    assert!(result.is_err(), "Money with 0 args should error");
}

// ============================================================
//  RecoverHP / RecoverEN 3+ args → error (C# arg count check)
// ============================================================

#[test]
fn recoverhp_too_many_args_is_error() {
    // C# RecoverHPCmd.cs: ArgNum != 2 && ArgNum != 3 → EventErrorException。
    let mut app = App::new();
    let result = src_core::event_runtime::execute(
        &mut app,
        &src_core::data::event::parse("RecoverHP リオ 50 extra\n").unwrap(),
    );
    assert!(result.is_err(), "RecoverHP with 3 user args should error");
}

#[test]
fn recoveren_too_many_args_is_error() {
    let mut app = App::new();
    let result = src_core::event_runtime::execute(
        &mut app,
        &src_core::data::event::parse("RecoverEN リオ 50 extra\n").unwrap(),
    );
    assert!(result.is_err(), "RecoverEN with 3 user args should error");
}

// ============================================================
//  ExpUp / LevelUp イベント発火
// ============================================================

#[test]
fn expup_increases_total_exp() {
    let app = run("ExpUp ブレイバー 150\n");
    assert_eq!(
        app.database().unit_instances[0].total_exp,
        150,
        "ExpUp 150 → total_exp=150"
    );
}

#[test]
fn expup_causes_levelup_event_to_fire() {
    // total_exp 0→100 で level 1→2 に上がる → `レベルアップ リオ:` が発火。
    let app = run("ExpUp ブレイバー 100\n\
         Exit\n\
         レベルアップ リオ:\n\
         Set leveled_up 1\n\
         Return\n");
    assert_eq!(
        app.script_var("leveled_up"),
        "1",
        "ExpUp でレベルアップ時に レベルアップ ラベルが発火するはず"
    );
}

#[test]
fn expup_does_not_fire_levelup_event_when_no_level_change() {
    // total_exp 0→50 では level が変わらない → イベント未発火。
    let app = run("ExpUp ブレイバー 50\n\
         Exit\n\
         レベルアップ リオ:\n\
         Set leveled_up 1\n\
         Return\n");
    assert_eq!(
        app.script_var("leveled_up"),
        "",
        "レベルアップしていないのにイベントが発火してはいけない"
    );
}

#[test]
fn level_function_reflects_expup() {
    // Level(pilot) が ExpUp 後のレベルを正しく返す。
    // total_exp=200 → level = 200/100 + 1 = 3。
    let app = run("ExpUp ブレイバー 200\nSet lv Level(リオ)\n");
    assert_eq!(app.script_var("lv"), "3");
}

// ============================================================
//  PilotInstance レベルアップ → 戦闘計算への反映
// ============================================================

#[test]
fn expup_leveled_pilot_has_higher_effective_stats() {
    // ExpUp でレベルアップしたパイロットの実効スタットが
    // effective_pilot_data() から返されることを確認。
    // リオ (infight=100) が level 1 → 4 になると infight が上がる。
    let app = run("ExpUp ブレイバー 300\n"); // level = 300/100 + 1 = 4
    let data = app
        .database()
        .effective_pilot_data("リオ")
        .expect("effective_pilot_data for リオ");
    // level 4 (= 3 レベルアップ) で infight が増加しているはず。
    // pilot_instance.rs の apply_stat_growth: infight += (level-1) * growth_rate。
    // リアル系 (growth_rate=12): infight = 100 + 3*12 = 136
    assert!(
        data.infight > 100,
        "ExpUp後の effective_pilot_data infight={} > 100 であるべき",
        data.infight
    );
}

// ============================================================
//  SetSkill / ClearSkill / Skill()
// ============================================================

#[allow(dead_code)]
fn pilot_skill_level(app: &App, _pilot: &str, skill: &str) -> String {
    app.script_var(&format!("sk_{skill}")).to_string()
}

#[test]
fn setskill_adds_skill_to_pilot_instance() {
    // SetSkill でパイロットにスキルを追加し、Skill() 関数で参照できることを確認。
    let app = run("SetSkill リオ 格闘 3\nSet sk_格闘 Skill(リオ, 格闘)\n");
    assert_eq!(app.script_var("sk_格闘"), "3");
}

#[test]
fn setskill_level_minus1_means_no_level() {
    // level=-1 はレベル表示なし → Skill() は 1 を返す。
    let app = run("SetSkill リオ 格闘 -1\nSet sk_格闘 Skill(リオ, 格闘)\n");
    assert_eq!(app.script_var("sk_格闘"), "1");
}

#[test]
fn clearskill_removes_skill() {
    let app = run("SetSkill リオ 格闘 3\nClearSkill リオ 格闘\nSet sk_格闘 Skill(リオ, 格闘)\n");
    assert_eq!(app.script_var("sk_格闘"), "0");
}

#[test]
fn setskill_overrides_static_pilot_data() {
    // SetSkill でスキルを動的付与し、static PilotData.features より優先されることを確認。
    // SETUP の リオ は features 未登録なので Skill() は 0 → SetSkill 後は指定レベル。
    let app = run("SetSkill リオ 格闘 5\nSet sk_格闘 Skill(リオ, 格闘)\n");
    assert_eq!(app.script_var("sk_格闘"), "5");
}

// ============================================================
//  SetBullet
// ============================================================

fn bullet_in_unit_data(app: &App, unit_name: &str, weapon_name: &str) -> Option<i32> {
    app.database()
        .units
        .iter()
        .find(|u| u.name == unit_name)?
        .weapons
        .iter()
        .find(|w| w.name == weapon_name)
        .map(|w| w.bullet)
}

#[test]
fn setbullet_changes_remaining_bullet_count() {
    // SetBullet は UnitData.weapons[i].bullet を直接書き換える。
    let mut app = App::new();
    let src = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon "ブレイバー" "ライフル" 2500 2 5 15 5
Place "ブレイバー" "リオ" Player 0 0
SetBullet ブレイバー ライフル 2
"#;
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    assert_eq!(bullet_in_unit_data(&app, "ブレイバー", "ライフル"), Some(2));
}

#[test]
fn setbullet_read_back_via_bullet_function() {
    // SetBullet 後に Bullet() 関数で読み戻せること。
    let mut app = App::new();
    let src = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon "ブレイバー" "ライフル" 2500 2 5 15 5
Place "ブレイバー" "リオ" Player 0 0
SetBullet ブレイバー ライフル 3
Set b Bullet(リオ,ライフル)
"#;
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    assert_eq!(app.script_var("b"), "3");
}

// ============================================================
//  Upgrade
// ============================================================

fn unit_max_hp(app: &App, unit_name: &str) -> i64 {
    app.database()
        .units
        .iter()
        .find(|u| u.name == unit_name)
        .map(|u| u.hp)
        .unwrap_or(0)
}

#[test]
fn upgrade_increases_unit_hp() {
    // SETUP: Unit リアル系 ... 3000 400 3500 ... → value=3000, hp=3500。
    // Upgrade unit hp 500 → UnitData の HP が 3500+500=4000 になる。
    let app = run("Upgrade ブレイバー hp 500\n");
    assert_eq!(unit_max_hp(&app, "ブレイバー"), 3500 + 500);
}

#[test]
fn upgrade_hp_read_back_via_maxhp_function() {
    // Upgrade 後、MaxHP() 関数がアップグレード後の値を返すこと。
    let app = run("Upgrade ブレイバー hp 500\nSet m MaxHP(リオ)\n");
    assert_eq!(app.script_var("m"), "4000");
}

#[test]
fn upgrade_unknown_attr_is_noop() {
    // 未知の属性は無視され、HP が変わらない。
    let app = run("Upgrade ブレイバー 存在しない属性 999\n");
    assert_eq!(unit_max_hp(&app, "ブレイバー"), 3500);
}

#[test]
fn upgrade_en_increases_max_en() {
    // Unit "ブレイバー" ... 3500 120 1200 110 → hp=3500, en=120, armor=1200, mob=110
    let app = run("Upgrade ブレイバー en 100\n");
    let en = app
        .database()
        .units
        .iter()
        .find(|u| u.name == "ブレイバー")
        .map(|u| u.en)
        .unwrap_or(0);
    assert_eq!(en, 120 + 100);
}

#[test]
fn upgrade_armor_increases_armor() {
    // Unit "ブレイバー" ... 3500 120 1200 110 → armor=1200。
    let app = run("Upgrade ブレイバー armor 50\n");
    let armor = app
        .database()
        .units
        .iter()
        .find(|u| u.name == "ブレイバー")
        .map(|u| u.armor)
        .unwrap_or(0);
    assert_eq!(armor, 1200 + 50);
}

// ============================================================
//  Fix / Release
// ============================================================

#[test]
fn fix_sets_pilot_is_fixed() {
    // SetSkill で PilotInstance を生成してから Fix する。
    let app = run("SetSkill リオ 底力 3\nFix リオ\n");
    let fixed = app
        .database()
        .pilot_instances
        .iter()
        .any(|p| p.pilot_data_name == "リオ" && p.is_fixed);
    assert!(fixed, "Fix リオ → is_fixed = true");
}

#[test]
fn release_clears_pilot_is_fixed() {
    let app = run("SetSkill リオ 底力 3\nFix リオ\nRelease リオ\n");
    let fixed = app
        .database()
        .pilot_instances
        .iter()
        .any(|p| p.pilot_data_name == "リオ" && p.is_fixed);
    assert!(!fixed, "Release リオ → is_fixed = false");
}

#[test]
fn release_no_arg_clears_all_fixed() {
    // 引数なし Release は全パイロットの固定を解除する。
    let app = run("SetSkill リオ 底力 3\nFix リオ\nRelease\n");
    let any_fixed = app.database().pilot_instances.iter().any(|p| p.is_fixed);
    assert!(
        !any_fixed,
        "Release 引数なし → 全パイロットの is_fixed = false"
    );
}

// ============================================================
//  RankUp
// ============================================================

#[test]
fn rankup_increments_rank_var() {
    let app = run("RankUp リオ\n");
    assert_eq!(app.script_var("__rank_リオ"), "1");
}

#[test]
fn rankup_with_n_adds_n() {
    let app = run("RankUp リオ 3\n");
    assert_eq!(app.script_var("__rank_リオ"), "3");
}

#[test]
fn rankup_accumulates_on_repeated_calls() {
    let app = run("RankUp リオ\nRankUp リオ\nRankUp リオ 2\n");
    assert_eq!(app.script_var("__rank_リオ"), "4");
}

// ============================================================
//  Supply (HP/EN full recovery)
// ============================================================

#[test]
fn supply_recovers_hp_and_en() {
    // Unit hp=3500, en=120。先にダメージを与えてから Supply で完全回復。
    let app = run("Damage ブレイバー 500\nSupply ブレイバー\n");
    let inst = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "ブレイバー")
        .expect("unit exists");
    assert_eq!(inst.damage, 0, "Supply で HP 完全回復");
    assert_eq!(inst.en_consumed, 0, "Supply で EN 完全回復");
}

// ============================================================
//  UseAbility
// ============================================================

#[test]
fn useability_repair_recovers_target_hp() {
    let app = run("Damage リオ 1000\nUseAbility リオ 修理装置 リオ\n");
    let inst = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "ブレイバー")
        .expect("unit exists");
    assert_eq!(inst.damage, 0, "修理装置 で HP 全回復");
}

#[test]
fn useability_supply_recovers_target_en() {
    // EN を消費させた後 補給装置 で回復
    let app = run("UseAbility リオ 補給装置 リオ\n");
    let inst = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "ブレイバー")
        .expect("unit exists");
    assert_eq!(inst.en_consumed, 0, "補給装置 で EN 全回復");
}

#[test]
fn useability_sets_last_used_ability_var() {
    let app = run("UseAbility リオ カスタムアビリティ\n");
    assert_eq!(app.script_var("直前使用アビリティ"), "カスタムアビリティ");
}

// ============================================================
//  ClearSpecialPower
// ============================================================

#[test]
fn clearspecialpower_removes_named_condition() {
    // SetStatus で付与した精神コマンド効果を ClearSpecialPower で解除。
    let app = run("SetStatus リオ 熱血\nClearSpecialPower リオ 熱血\n");
    let inst = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "ブレイバー")
        .expect("unit exists");
    assert!(
        !inst.conditions.iter().any(|c| c.name == "熱血"),
        "熱血 が解除される"
    );
}

#[test]
fn clearspecialpower_no_sp_name_clears_all() {
    // sp_name 省略時は全 condition を解除する。
    let app = run("SetStatus リオ 熱血\nSetStatus リオ 必中\nClearSpecialPower リオ\n");
    let inst = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "ブレイバー")
        .expect("unit exists");
    assert!(inst.conditions.is_empty(), "全 condition が解除される");
}

// ============================================================
//  活動領域切替コマンド (Land / 空中 / 地上 / 水中 / 水上 / 宇宙 / 地中)
// ============================================================

fn area_of(app: &App, unit: &str) -> String {
    // Area() 関数で current_area を読み出す簡易ヘルパ。
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == unit)
        .map(|u| u.current_area.clone())
        .unwrap_or_default()
}

#[test]
fn land_command_sets_area_to_ground() {
    // `Land unit` (英語別名) は活動領域を「地上」に設定する。
    let app = run("Land ブレイバー\n");
    assert_eq!(area_of(&app, "ブレイバー"), "地上");
}

#[test]
fn air_command_sets_area_to_sky() {
    // 日本語コマンド「空中」で活動領域を「空中」に設定。
    let app = run("空中 ブレイバー\n");
    assert_eq!(area_of(&app, "ブレイバー"), "空中");
}

#[test]
fn area_command_readable_via_area_function() {
    // 設定後に Area() 関数で読み戻せる。
    let app = run("空中 ブレイバー\nSet v Area(ブレイバー)\n");
    assert_eq!(app.script_var("v"), "空中");
}

#[test]
fn area_command_all_english_aliases() {
    // 英語別名 → 日本語ラベルの対応を網羅。
    let cases = [
        ("Land", "地上"),
        ("Air", "空中"),
        ("Water", "水中"),
        ("Sea", "水上"),
        ("Cosmos", "宇宙"),
        ("Diving", "地中"),
    ];
    for (cmd, expected) in cases {
        let app = run(&format!("{cmd} ブレイバー\n"));
        assert_eq!(area_of(&app, "ブレイバー"), expected, "{cmd} → {expected}");
    }
}

#[test]
fn area_command_overwrites_previous_area() {
    // 連続実行で上書きされる (空中 → 地上)。
    let app = run("空中 ブレイバー\n地上 ブレイバー\n");
    assert_eq!(area_of(&app, "ブレイバー"), "地上");
}

#[test]
fn area_command_nonexistent_unit_is_noop() {
    // 存在しないユニット指定は no-op (パニックしない)。
    let app = run("空中 存在しないユニット\n");
    assert_eq!(
        area_of(&app, "ブレイバー"),
        "",
        "対象ユニットの area は未変更"
    );
}
