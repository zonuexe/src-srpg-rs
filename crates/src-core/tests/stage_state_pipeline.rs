//! stage_state パイプライン全段テスト / Full stage_state state-machine.
//!
//! `App::start_scenario` → `begin_sortie` → `begin_battle` → `game_clear`
//! が順に Briefing / Sortie / Battle / Victory を遷移し、それぞれの段で
//! 自動発火ラベル (`プロローグ` / `スタート` / `Turn 1` / `勝利`) を実際に
//! 走らせていることを検証する。
//!
//! 既存 E2E fixture は `Goto` / `Start` 直接実行で stage_state を飛ばして
//! 進めるため、ここで初めて UI 駆動の state pipeline が通る。
//!
//! 著作権配慮: 合成 fixture (.eve は test 内 inline)。

use src_core::data::event;
use src_core::event_runtime;
use src_core::stage::StageState;
use src_core::App;

/// 各 phase で自動発火するラベルを定義し、それぞれが実行されたかを
/// シナリオ変数 `prologue_run` / `start_run` / `turn1_run` / `victory_run`
/// で記録する。
const STAGE_LIFECYCLE_EVE: &str = r#"
プロローグ:
Set prologue_run 1
Return

スタート:
Set start_run 1
Return

ターン 1:
Set turn1_run 1
Return

勝利:
Set victory_run 1
Return

GameOver:
Set gameover_run 1
Return
"#;

fn load(app: &mut App) {
    let stmts = event::parse(STAGE_LIFECYCLE_EVE).expect("parse");
    event_runtime::execute(app, &stmts).expect("library register");
}

#[test]
fn briefing_to_sortie_to_battle_to_victory_fires_each_label() {
    let mut app = App::new();
    load(&mut app);

    // ----- start_scenario: 原典 SRC 通り Prologue → Start → Battle まで自動進行 -----
    // 原典 SRC では Prologue 終了 → メインウィンドウ表示 → Start イベント発生 →
    // 戦闘開始、までを連続して行う。ユーザの「Enter で出撃準備へ」入力は無い。
    app.start_scenario("テスト面");
    assert_eq!(app.stage(), "テスト面");
    assert_eq!(
        app.stage_state(),
        StageState::Battle,
        "Battle まで自動進行する"
    );
    assert_eq!(app.script_var("prologue_run"), "1", "プロローグ が走らない");
    assert_eq!(app.script_var("start_run"), "1", "スタート が走らない");
    assert_eq!(app.script_var("turn1_run"), "1", "ターン 1 が走らない");
    assert_eq!(app.script_var("victory_run"), "");

    // ----- Victory 段: game_clear -----
    app.game_clear();
    assert_eq!(app.stage_state(), StageState::Victory);
    assert_eq!(app.script_var("victory_run"), "1", "勝利 が走らない");
    assert_eq!(app.script_var("gameover_run"), "", "敗北 ラベルは走らない");
}

#[test]
fn battle_then_game_over_fires_defeat_label() {
    let mut app = App::new();
    load(&mut app);
    // start_scenario の auto-progress で Battle まで進む。
    app.start_scenario("敗北面");
    app.game_over();
    assert_eq!(app.stage_state(), StageState::Defeat);
    assert_eq!(app.script_var("gameover_run"), "1");
    assert_eq!(app.script_var("victory_run"), "");
}

#[test]
fn out_of_order_transitions_are_ignored() {
    // begin_battle / game_clear のゲート (前提 state でないと no-op) を
    // 手動で確認する。start_scenario 経由だと auto-progress で一気に Battle まで
    // 行くので、ここでは App::new() 直後の Briefing 状態から手動で順に呼ぶ。
    let mut app = App::new();
    load(&mut app);
    assert_eq!(app.stage_state(), StageState::Briefing);

    // Briefing で begin_battle → no-op (Sortie じゃない)
    app.begin_battle();
    assert_ne!(app.stage_state(), StageState::Battle);
    assert_eq!(app.script_var("start_run"), "");

    // Briefing で game_clear → no-op (Battle じゃない)
    app.game_clear();
    assert_ne!(app.stage_state(), StageState::Victory);
    assert_eq!(app.script_var("victory_run"), "");

    // Sortie で game_clear → no-op (まだ Battle に入ってない)
    app.begin_sortie();
    assert_eq!(app.stage_state(), StageState::Sortie);
    app.game_clear();
    assert_ne!(app.stage_state(), StageState::Victory);
    assert_eq!(app.script_var("victory_run"), "");
}
