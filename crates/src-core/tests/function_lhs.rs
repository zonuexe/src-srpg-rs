//! 関数左辺値代入のテスト。
//! SRC では `HP(unit) = n` / `EN(unit) = n` / `Action(unit) = n` /
//! `Morale(unit) = n` / `SP(pilot) = n` / `Plana(pilot) = n` が
//! それぞれの runtime フィールドを直接更新する。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

/// パイロット + ユニット + 配置フィクスチャ。
/// Unit コマンドの引数順:
///   Name Class PilotNum ItemNum Trans Speed Size Value ExpValue HP EN Armor Mob Adaption
/// → ブレイバーの MaxHP = 3500, MaxEN = 120
///
/// Pilot コマンドの 12-arg 形式は SP を設定しない (sp = None → max_sp = 0)。
const SETUP: &str = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
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

// helpers -------------------------------------------------------------------

fn unit_damage(app: &App, unit_name: &str) -> i64 {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == unit_name)
        .map(|u| u.damage)
        .unwrap_or(-1)
}

fn unit_en_consumed(app: &App, unit_name: &str) -> i32 {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == unit_name)
        .map(|u| u.en_consumed)
        .unwrap_or(-1)
}

fn unit_morale(app: &App, unit_name: &str) -> i32 {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == unit_name)
        .map(|u| u.morale)
        .unwrap_or(-1)
}

fn unit_has_acted(app: &App, unit_name: &str) -> bool {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == unit_name)
        .map(|u| u.has_acted)
        .unwrap_or(false)
}

fn unit_sp_consumed(app: &App, unit_name: &str) -> i32 {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == unit_name)
        .map(|u| u.sp_consumed)
        .unwrap_or(-1)
}

fn unit_plana(app: &App, unit_name: &str) -> i32 {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == unit_name)
        .map(|u| u.plana)
        .unwrap_or(-1)
}

// ============================================================
//  HP(unit) = n
// ============================================================

#[test]
fn hp_lhs_set_directly() {
    // MaxHP = 3500。HP(リオ) = 1000 → damage = 3500 - 1000 = 2500
    let app = run("HP(リオ) = 1000\n");
    assert_eq!(unit_damage(&app, "ブレイバー"), 2500);
}

#[test]
fn hp_lhs_via_expr() {
    // HP(リオ) = HP(リオ) - 500
    // 初期 HP = MaxHP = 3500 → HP(リオ) - 500 = 3000 → damage = 500
    let app = run("HP(リオ) = HP(リオ) - 500\n");
    assert_eq!(unit_damage(&app, "ブレイバー"), 500);
}

#[test]
fn hp_lhs_clamps_at_max() {
    // MaxHP = 3500 なので 9999 は 3500 に clamp → damage = 0
    let app = run("HP(リオ) = 9999\n");
    assert_eq!(unit_damage(&app, "ブレイバー"), 0);
}

#[test]
fn hp_lhs_zero_is_valid() {
    // C# Unit.HP setter: max(0, min(MaxHP, value)) → HP=0 は有効 (撃墜状態)。
    // HP=0 → damage = MaxHP = 3500
    let app = run("HP(リオ) = 0\n");
    assert_eq!(unit_damage(&app, "ブレイバー"), 3500);
}

#[test]
fn hp_lhs_negative_clamps_to_zero() {
    // 負値は 0 に clamp → damage = MaxHP = 3500
    let app = run("HP(リオ) = -100\n");
    assert_eq!(unit_damage(&app, "ブレイバー"), 3500);
}

#[test]
fn hp_lhs_does_not_create_script_var() {
    // 関数左辺値代入は script_var を汚染しないこと
    let app = run("HP(リオ) = 1000\n");
    assert_eq!(app.script_var("HP(リオ)"), "");
}

// ============================================================
//  EN(unit) = n
// ============================================================

#[test]
fn en_lhs_set_directly() {
    // MaxEN = 120。EN(リオ) = 60 → en_consumed = 120 - 60 = 60
    let app = run("EN(リオ) = 60\n");
    assert_eq!(unit_en_consumed(&app, "ブレイバー"), 60);
}

#[test]
fn en_lhs_clamps_at_zero() {
    // EN(リオ) = -100 → 0 に clamp → en_consumed = MaxEN = 120
    let app = run("EN(リオ) = -100\n");
    assert_eq!(unit_en_consumed(&app, "ブレイバー"), 120);
}

#[test]
fn en_lhs_clamps_at_max() {
    // EN(リオ) = 9999 → MaxEN = 120 → en_consumed = 0
    let app = run("EN(リオ) = 9999\n");
    assert_eq!(unit_en_consumed(&app, "ブレイバー"), 0);
}

#[test]
fn en_lhs_read_back_via_function() {
    // EN(リオ) = 80 後に EN(リオ) で読み戻せること
    let app = run("EN(リオ) = 80\nSet e $(EN(リオ))\n");
    assert_eq!(app.script_var("e"), "80");
}

// ============================================================
//  Morale(unit) = n
// ============================================================

#[test]
fn morale_lhs_set() {
    let app = run("Morale(リオ) = 130\n");
    assert_eq!(unit_morale(&app, "ブレイバー"), 130);
}

#[test]
fn morale_lhs_clamps_at_150() {
    let app = run("Morale(リオ) = 200\n");
    assert_eq!(unit_morale(&app, "ブレイバー"), 150);
}

#[test]
fn morale_lhs_clamps_at_min_morale_50() {
    // C# Pilot.SetMorale: MinMorale デフォルト 50 が下限。
    let app = run("Morale(リオ) = -50\n");
    assert_eq!(unit_morale(&app, "ブレイバー"), 50);
}

#[test]
fn morale_lhs_read_back_via_function() {
    let app = run("Morale(リオ) = 120\nSet m $(Morale(リオ))\n");
    assert_eq!(app.script_var("m"), "120");
}

// ============================================================
//  Action(unit) = n
// ============================================================

#[test]
fn action_lhs_zero_marks_acted() {
    // Action(リオ) = 0 → has_acted = true
    let app = run("Action(リオ) = 0\n");
    assert!(unit_has_acted(&app, "ブレイバー"));
}

#[test]
fn action_lhs_positive_restores() {
    // has_acted を先に true にしてから Action = 1 で復元
    let app = run("Action(リオ) = 0\nAction(リオ) = 1\n");
    assert!(!unit_has_acted(&app, "ブレイバー"));
}

#[test]
fn action_lhs_increment_example() {
    // `Action() = Action() + 1` のドキュメント例相当
    // 初期 has_acted = false (action = 1)、+1 = 2 → has_acted = false
    let app = run("Action(リオ) = Action(リオ) + 1\n");
    assert!(!unit_has_acted(&app, "ブレイバー"));
}

#[test]
fn action_lhs_negative_marks_acted() {
    // 負値も 0 以下扱い → has_acted = true
    let app = run("Action(リオ) = -1\n");
    assert!(unit_has_acted(&app, "ブレイバー"));
}

// ============================================================
//  SP(pilot) = n
//  NOTE: inline Pilot コマンドは SP フィールドを設定しない (max_sp = 0)。
//        そのため代入は max_sp = 0 で clamp され sp_consumed は 0 に留まる。
//        「代入が script_var を汚染しない」点を主に検証する。
// ============================================================

#[test]
fn sp_lhs_does_not_create_script_var() {
    // SP(リオ) = 150 は script_var を作らず、sp_consumed に反映される
    let app = run("SP(リオ) = 150\n");
    assert_eq!(app.script_var("SP(リオ)"), "");
    // max_sp = 0 のため sp_consumed = 0 に留まる
    assert_eq!(unit_sp_consumed(&app, "ブレイバー"), 0);
}

#[test]
fn sp_lhs_no_script_var_for_negative() {
    // 負値でも script_var を汚染しない
    let app = run("SP(リオ) = -1\n");
    assert_eq!(app.script_var("SP(リオ)"), "");
}

// ============================================================
//  Plana(pilot) = n
// ============================================================

#[test]
fn plana_lhs_set() {
    let app = run("Plana(リオ) = 10\n");
    assert_eq!(unit_plana(&app, "ブレイバー"), 10);
}

#[test]
fn plana_lhs_read_back() {
    // `Plana()` 関数で読み戻せること
    let app = run("Plana(リオ) = 25\nSet p $(Plana(リオ))\n");
    assert_eq!(app.script_var("p"), "25");
}

#[test]
fn plana_lhs_overwrite() {
    let app = run("Plana(リオ) = 10\nPlana(リオ) = 30\n");
    assert_eq!(unit_plana(&app, "ブレイバー"), 30);
}

#[test]
fn plana_lhs_does_not_create_script_var() {
    let app = run("Plana(リオ) = 10\n");
    assert_eq!(app.script_var("Plana(リオ)"), "");
}

// ============================================================
//  unknown unit は no-op
// ============================================================

#[test]
fn hp_lhs_unknown_unit_is_noop() {
    let app = run("HP(存在しない) = 100\n");
    // ブレイバーのダメージはゼロのまま
    assert_eq!(unit_damage(&app, "ブレイバー"), 0);
}

#[test]
fn morale_lhs_unknown_unit_is_noop() {
    let app = run("Morale(存在しない) = 130\n");
    // ブレイバーの士気はデフォルト 100 のまま
    assert_eq!(unit_morale(&app, "ブレイバー"), 100);
}

// ============================================================
//  既存の pilot_function テストとの整合性
//  - Plana(pilot) はユニットが配置されていない場合に 0 を返す
// ============================================================

#[test]
fn plana_returns_zero_when_unit_not_placed() {
    // Unit を Place しない場合 → find_unit で見つからず 0
    let mut app = App::new();
    let src = "Pilot \"リオ\" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200\nSet p $(Plana(リオ))\n";
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    assert_eq!(app.script_var("p"), "0");
}
