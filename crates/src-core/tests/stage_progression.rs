//! Stage / Briefing / Money / Turn / Win / Lose / Finish のステージ進行系。

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
//  Stage / Briefing
// ============================================================

#[test]
fn stage_sets_name() {
    let app = run(r#"Stage "第一話 — 序章""#);
    assert_eq!(app.stage(), "第一話 — 序章");
}

#[test]
fn briefing_sets_text() {
    let app = run(r#"Briefing "テストブリーフィング""#);
    assert_eq!(app.briefing(), "テストブリーフィング");
}

#[test]
fn stage_can_be_changed() {
    let app = run(r#"
Stage "第一話"
Stage "第二話"
"#);
    assert_eq!(app.stage(), "第二話");
}

// ============================================================
//  Turn
// ============================================================

#[test]
fn turn_sets_number() {
    let app = run("Turn 5\n");
    assert_eq!(app.turn().number, 5);
}

#[test]
fn turn_overwrites() {
    let app = run("Turn 1\nTurn 10\n");
    assert_eq!(app.turn().number, 10);
}

// ============================================================
//  Money
// ============================================================

#[test]
fn money_delta_from_zero_gives_value() {
    // Money n は常にデルタ (差分)。0 から 10000 加算 → 10000
    let app = run("Money 10000\n");
    assert_eq!(app.money(), 10000);
}

#[test]
fn money_sequential_plain_values_accumulate() {
    // SRC.Sharp: Money は常に IncrMoney(n) — 2 回呼べば累積される
    let app = run("Money 10000\nMoney 5000\n");
    assert_eq!(app.money(), 15000);
}

#[test]
fn money_plus_delta_adds() {
    let app = run(r#"
Money 500
Money +200
"#);
    assert_eq!(app.money(), 700);
}

#[test]
fn money_minus_delta_subtracts() {
    let app = run(r#"
Money 500
Money -200
"#);
    assert_eq!(app.money(), 300);
}

#[test]
fn money_clamps_to_zero_when_goes_negative() {
    // SRC.Sharp: `Money -1000` で 100 → 0 (負にならない)
    let app = run(r#"
Money 100
Money -1000
"#);
    assert_eq!(app.money(), 0);
}

#[test]
fn money_clamps_to_max_999999999() {
    // SRC.Sharp: 999,999,999 が上限
    let app = run(r#"
Money 999999000
Money +9999
"#);
    assert_eq!(app.money(), 999_999_999);
}

// ============================================================
//  Win / Lose / GameClear / GameOver
// ============================================================

#[test]
fn win_sets_victory_state() {
    let app = run("Win\n");
    assert_eq!(app.stage_state(), src_core::StageState::Victory);
}

#[test]
fn game_clear_sets_victory() {
    let app = run("GameClear\n");
    assert_eq!(app.stage_state(), src_core::StageState::Victory);
}

#[test]
fn lose_sets_defeat_state() {
    let app = run("Lose\n");
    assert_eq!(app.stage_state(), src_core::StageState::Defeat);
}

#[test]
fn game_over_sets_defeat() {
    let app = run("GameOver\n");
    assert_eq!(app.stage_state(), src_core::StageState::Defeat);
}

// ============================================================
//  Telop / Message
// ============================================================

#[test]
fn message_appends_to_log() {
    let app = run(r#"
Message "メッセージ1"
Message "メッセージ2"
"#);
    let msgs = app.messages();
    assert_eq!(msgs.len(), 2);
    assert_eq!(msgs[0], "メッセージ1");
    assert_eq!(msgs[1], "メッセージ2");
}

#[test]
fn telop_appends_to_messages() {
    // Telop は messages にも push される (= DisplayMessage 相当)
    let app = run(r#"Telop "テロップ表示""#);
    let msgs = app.messages();
    assert!(msgs.iter().any(|m| m.contains("テロップ表示")));
}

// ============================================================
//  Start (Battle 状態へ即遷移)
// ============================================================

#[test]
fn start_sets_battle_state() {
    let app = run("Start\n");
    assert_eq!(app.stage_state(), src_core::StageState::Battle);
}

// ============================================================
//  Finish
// ============================================================

#[test]
fn finish_ends_unit_action_not_stage() {
    // SRC `Finish [unit]` (Finishコマンド.md): 指定ユニットの行動を 1 回分終了
    // させるユニットコマンド。ステージ終了 (Victory) ではない。
    let app = run("\
Pilot \"リオ\" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit \"ブレイバー\" Real 1 0 陸 5 M 1000 100 5000 100 1500 100 AAAA
Create 味方 ブレイバー 0 リオ 10 3 3
Finish リオ
");
    assert_ne!(app.stage_state(), src_core::StageState::Victory);
    let acted = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.pilot_name == "リオ")
        .map(|u| u.has_acted)
        .unwrap_or(false);
    assert!(acted, "Finish 後に has_acted=true であるべき");
}
