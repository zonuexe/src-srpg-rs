//! SetStatus / UnsetStatus / IncreaseMorale / ExpUp / LevelUp の edge cases。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

const SETUP: &str = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon "ブレイバー" "ライフル" 2500 2 5 15 -1
Place "ブレイバー" "リオ" Player 0 0
"#;

fn run_setup(extra: &str) -> App {
    let mut app = App::new();
    let src = format!("{SETUP}{extra}");
    let stmts = event::parse(&src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

fn unit_statuses(app: &App, name: &str) -> Vec<String> {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == name)
        .map(|u| u.conditions.iter().map(|c| c.name.clone()).collect())
        .unwrap_or_default()
}

fn unit_morale(app: &App, name: &str) -> i32 {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == name)
        .map(|u| u.morale)
        .unwrap_or(-1)
}

fn unit_total_exp(app: &App, name: &str) -> i32 {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == name)
        .map(|u| u.total_exp)
        .unwrap_or(-1)
}

// ============================================================
//  SetStatus
// ============================================================

#[test]
fn set_status_adds_to_list() {
    let app = run_setup(r#"SetStatus リオ "毒""#);
    let st = unit_statuses(&app, "ブレイバー");
    assert!(st.iter().any(|s| s == "毒"), "statuses = {st:?}");
}

#[test]
fn set_status_multiple() {
    let app = run_setup(
        r#"
SetStatus リオ "毒"
SetStatus リオ "麻痺"
"#,
    );
    let st = unit_statuses(&app, "ブレイバー");
    assert!(st.iter().any(|s| s == "毒"));
    assert!(st.iter().any(|s| s == "麻痺"));
}

#[test]
fn unset_status_removes() {
    let app = run_setup(
        r#"
SetStatus リオ "毒"
SetStatus リオ "麻痺"
UnsetStatus リオ "毒"
"#,
    );
    let st = unit_statuses(&app, "ブレイバー");
    assert!(!st.iter().any(|s| s == "毒"));
    assert!(st.iter().any(|s| s == "麻痺"));
}

// ============================================================
//  IncreaseMorale
// ============================================================

#[test]
fn increase_morale_default_step() {
    // 初期 morale = 100
    let app = run_setup("IncreaseMorale リオ 10\n");
    assert_eq!(unit_morale(&app, "ブレイバー"), 110);
}

#[test]
fn increase_morale_negative_delta_decreases() {
    let app = run_setup("IncreaseMorale リオ -20\n");
    assert_eq!(unit_morale(&app, "ブレイバー"), 80);
}

// ============================================================
//  ExpUp / LevelUp
// ============================================================

#[test]
fn exp_up_accumulates() {
    let app = run_setup("ExpUp リオ 50\nExpUp リオ 30\n");
    assert_eq!(unit_total_exp(&app, "ブレイバー"), 80);
}

#[test]
fn level_up_adds_100_exp_per_level() {
    // LevelUp unit [n] → n * 100 exp 加算
    let app = run_setup("LevelUp リオ 3\n");
    assert_eq!(unit_total_exp(&app, "ブレイバー"), 300);
}

#[test]
fn level_up_default_one_level() {
    let app = run_setup("LevelUp リオ\n");
    assert_eq!(unit_total_exp(&app, "ブレイバー"), 100);
}

// ============================================================
//  DecreaseMorale
// ============================================================

#[test]
fn decrease_morale_reduces_by_delta() {
    // 初期 morale = 100
    let app = run_setup("DecreaseMorale リオ 20\n");
    assert_eq!(unit_morale(&app, "ブレイバー"), 80);
}

#[test]
fn decrease_morale_clamps_to_min_morale_50() {
    // C# では DecreaseMorale は存在せず IncreaseMorale の逆相当。
    // Pilot.SetMorale が [MinMorale, MaxMorale] = [50, 150] にクランプする。
    let app = run_setup("DecreaseMorale リオ 999\n");
    assert_eq!(unit_morale(&app, "ブレイバー"), 50);
}

#[test]
fn increase_then_decrease_morale() {
    let app = run_setup("IncreaseMorale リオ 30\nDecreaseMorale リオ 10\n");
    assert_eq!(unit_morale(&app, "ブレイバー"), 120);
}

// ============================================================
//  IncreaseMorale — 下限は 50 (C# IncreaseMoraleCmd.cs)
// ============================================================

#[test]
fn increase_morale_cannot_go_below_50() {
    // 初期 morale=100、大きな負 delta を与えても 50 より下にならない。
    let app = run_setup("IncreaseMorale リオ -999\n");
    assert_eq!(unit_morale(&app, "ブレイバー"), 50);
}

#[test]
fn increase_morale_at_boundary_stays_50() {
    // delta = -50 → 100 - 50 = 50 (境界値)
    let app = run_setup("IncreaseMorale リオ -50\n");
    assert_eq!(unit_morale(&app, "ブレイバー"), 50);
}

#[test]
fn increase_morale_upper_clamps_at_150() {
    // delta = +999 → 150 が上限
    let app = run_setup("IncreaseMorale リオ 999\n");
    assert_eq!(unit_morale(&app, "ブレイバー"), 150);
}

// ============================================================
//  Level() — 上限は 99 (C# LevelUpCmd.cs)
// ============================================================

#[test]
fn level_function_caps_at_99() {
    // total_exp = 9900 → (9900/100) + 1 = 100 だが上限 99。
    // LevelUp 99 → total_exp = 9900
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
LevelUp リオ 99
Set lv Level(リオ)
"#,
    );
    assert_eq!(app.script_var("lv"), "99", "Level() should cap at 99");
}

#[test]
fn level_function_at_98_is_not_capped() {
    // total_exp = 9700 → (9700/100) + 1 = 98 (上限 99 未満)
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
LevelUp リオ 97
Set lv Level(リオ)
"#,
    );
    assert_eq!(app.script_var("lv"), "98");
}

// ============================================================
//  unknown 対象は no-op
// ============================================================

#[test]
fn set_status_unknown_unit_is_noop() {
    let app = run_setup(r#"SetStatus 存在しない "毒""#);
    let st = unit_statuses(&app, "ブレイバー");
    assert!(st.is_empty());
}

#[test]
fn increase_morale_unknown_unit_is_noop() {
    let app = run_setup("IncreaseMorale 存在しない 10\n");
    assert_eq!(unit_morale(&app, "ブレイバー"), 100);
}
