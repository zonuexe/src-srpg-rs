//! Damage / Heal / RecoverHP / RecoverEN / Supply / Fix のエッジケース。

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

fn unit_damage(app: &App, name: &str) -> i64 {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == name)
        .map(|u| u.damage)
        .unwrap_or(-1)
}

fn unit_en_consumed(app: &App, name: &str) -> i32 {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == name)
        .map(|u| u.en_consumed)
        .unwrap_or(-1)
}

// ============================================================
//  Damage
// ============================================================

#[test]
fn damage_subtracts_hp() {
    let app = run_setup("Damage リオ 500\n");
    assert_eq!(unit_damage(&app, "ブレイバー"), 500);
}

#[test]
fn damage_destroys_unit_at_zero_hp() {
    let app = run_setup("Damage リオ 9999\n");
    // Damage で HP <= 0 → unit_instances から除去
    let count = app.database().unit_instances.len();
    assert_eq!(count, 0);
}

#[test]
fn damage_cumulative() {
    let app = run_setup("Damage リオ 300\nDamage リオ 200\n");
    assert_eq!(unit_damage(&app, "ブレイバー"), 500);
}

// ============================================================
//  Heal
// ============================================================

#[test]
fn heal_after_damage() {
    let app = run_setup("Damage リオ 500\nHeal リオ 200\n");
    assert_eq!(unit_damage(&app, "ブレイバー"), 300);
}

#[test]
fn heal_cannot_go_below_zero() {
    let app = run_setup("Damage リオ 100\nHeal リオ 9999\n");
    assert_eq!(unit_damage(&app, "ブレイバー"), 0);
}

// ============================================================
//  RecoverHP
// ============================================================

#[test]
fn recover_hp_full_keyword() {
    let app = run_setup(
        r#"
Damage リオ 500
RecoverHP リオ Full
"#,
    );
    // "Full" / "全" spec は 100% 回復
    assert_eq!(unit_damage(&app, "ブレイバー"), 0);
}

#[test]
fn recover_hp_100_percent() {
    // 100 = 100% recovery = full
    let app = run_setup(
        r#"
Damage リオ 500
RecoverHP リオ 100
"#,
    );
    assert_eq!(unit_damage(&app, "ブレイバー"), 0);
}

#[test]
fn recover_hp_percentage() {
    let app = run_setup(
        r#"
Damage リオ 1000
RecoverHP リオ 50
"#,
    );
    // 50% of max_hp (3500) = 1750 restored
    // damage = 1000 → 1000 - 1750 = clamp to 0 (HP ≥ 0)
    assert_eq!(unit_damage(&app, "ブレイバー"), 0);
}

#[test]
fn recover_hp_over_100_percent_caps_at_full() {
    // RecoverHP rate は常にパーセント (SRC 仕様)。
    // 300% 指定 → 完全回復 (damage = 0)
    let app = run_setup(
        r#"
Damage リオ 1000
RecoverHP リオ 300
"#,
    );
    assert_eq!(unit_damage(&app, "ブレイバー"), 0);
}

#[test]
fn recover_hp_negative_rate_reduces_hp_but_minimum_1() {
    // RecoverHP -100 → HP を 100% 減少させようとするが最低値は 1 (damage = max_hp - 1)
    // max_hp = 3500 → damage = 3499
    let app = run_setup(
        r#"
RecoverHP リオ -100
"#,
    );
    assert_eq!(unit_damage(&app, "ブレイバー"), 3499);
}

// ============================================================
//  RecoverEN
// ============================================================

#[test]
fn recover_en_full_after_consume() {
    let app = run_setup(
        r#"
Damage リオ 0
RecoverEN リオ Full
"#,
    );
    assert_eq!(unit_en_consumed(&app, "ブレイバー"), 0);
}

// ============================================================
//  Supply (HP/EN 完全回復)
// ============================================================

#[test]
fn supply_restores_both_hp_and_en() {
    let app = run_setup(
        r#"
Damage リオ 500
Supply リオ
"#,
    );
    assert_eq!(unit_damage(&app, "ブレイバー"), 0);
    assert_eq!(unit_en_consumed(&app, "ブレイバー"), 0);
}

// ============================================================
//  Kill
// ============================================================

#[test]
fn kill_removes_unit_immediately() {
    let app = run_setup("Kill リオ\n");
    assert_eq!(app.database().unit_instances.len(), 0);
}

// ============================================================
//  Unknown target は no-op (crash しない)
// ============================================================

#[test]
fn damage_unknown_unit_is_noop() {
    let app = run_setup("Damage 存在しない 1000\n");
    // ブレイバーは無傷
    assert_eq!(unit_damage(&app, "ブレイバー"), 0);
}

#[test]
fn heal_unknown_unit_is_noop() {
    let app = run_setup("Heal 存在しない 1000\n");
    assert_eq!(app.database().unit_instances.len(), 1);
}

// ============================================================
//  RecoverSP — 引数は常にパーセント (C# RecoverSPCmd.cs)
// ============================================================

fn sp_consumed(app: &App) -> i32 {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "ブレイバー")
        .map(|u| u.sp_consumed)
        .unwrap_or(-1)
}

fn make_app_with_sp(max_sp: i32, extra_script: &str) -> App {
    use src_core::data::event;
    use src_core::event_runtime;
    let mut app = App::new();
    let src = format!("{SETUP}{extra_script}");
    let stmts = event::parse(&src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    // PilotData.sp は Pilot 命令では設定されないため直接注入する。
    if let Some(p) = app
        .database_mut()
        .pilots
        .iter_mut()
        .find(|p| p.name == "リオ")
    {
        p.sp = Some(max_sp);
    }
    app
}

#[test]
fn recoversp_full_restores_all_sp() {
    // SRC仕様: RecoverSP pilot rate の2引数。100% で全回復。
    let mut app = make_app_with_sp(60, "");
    app.database_mut().unit_instances[0].sp_consumed = 40;
    let stmts = src_core::data::event::parse("RecoverSP リオ 100\n").unwrap();
    src_core::event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(sp_consumed(&app), 0);
}

#[test]
fn recoversp_plain_number_is_percentage_not_raw() {
    // C# RecoverSPCmd.cs: 引数はパーセント。
    // max_sp=100 のとき `RecoverSP リオ 50` → 50% = 50 ポイント回復。
    // バグ修正前は raw 50 ポイントと同じだが、max_sp=100 なら偶然同値。
    // max_sp=200 のとき `RecoverSP リオ 50` → 100 ポイント回復 (sp_consumed 100→0)。
    let mut app = make_app_with_sp(200, "");
    app.database_mut().unit_instances[0].sp_consumed = 100; // 消費 100
    let stmts = src_core::data::event::parse("RecoverSP リオ 50\n").unwrap();
    src_core::event_runtime::execute(&mut app, &stmts).unwrap();
    // 50% of 200 = 100 → sp_consumed 100 - 100 = 0
    assert_eq!(
        sp_consumed(&app),
        0,
        "50% of MaxSP=200 should recover 100 points"
    );
}

#[test]
fn recoversp_percent_suffix_is_also_percentage() {
    // "50%" と "50" は同じ計算になる。
    let mut app = make_app_with_sp(200, "");
    app.database_mut().unit_instances[0].sp_consumed = 100;
    let stmts = src_core::data::event::parse("RecoverSP リオ 50%\n").unwrap();
    src_core::event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(sp_consumed(&app), 0);
}

#[test]
fn recoversp_partial_recovery_does_not_go_below_zero() {
    // max_sp=100, sp_consumed=20, recover 50% = 50 → sp_consumed max(20-50,0) = 0
    let mut app = make_app_with_sp(100, "");
    app.database_mut().unit_instances[0].sp_consumed = 20;
    let stmts = src_core::data::event::parse("RecoverSP リオ 50\n").unwrap();
    src_core::event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(sp_consumed(&app), 0);
}

#[test]
fn recoversp_plain_number_distinguishable_from_raw_when_max_sp_differs() {
    // max_sp=100, sp_consumed=80, recover 25% = 25 → sp_consumed = 55。
    // もし raw 25 ポイントなら 55 と同じだが、max_sp=80 の場合:
    // 25% of 80 = 20 → sp_consumed 80-20=60。raw 25 ならば 55。
    let mut app = make_app_with_sp(80, "");
    app.database_mut().unit_instances[0].sp_consumed = 80;
    let stmts = src_core::data::event::parse("RecoverSP リオ 25\n").unwrap();
    src_core::event_runtime::execute(&mut app, &stmts).unwrap();
    // 25% of 80 = 20 → sp_consumed = 80 - 20 = 60
    assert_eq!(
        sp_consumed(&app),
        60,
        "25% of MaxSP=80 should recover 20 points"
    );
}

#[test]
fn recoversp_by_pilot_name_variable() {
    // RecoverSP 対象パイロット 100 パターン (スパロボ戦記実使用例)
    let mut app = make_app_with_sp(100, "");
    app.database_mut().unit_instances[0].sp_consumed = 60;
    // 対象パイロット変数にパイロット名をセット
    app.set_script_var("対象パイロット".to_string(), "リオ".to_string());
    let stmts = src_core::data::event::parse("RecoverSP $(対象パイロット) 100\n").unwrap();
    src_core::event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(sp_consumed(&app), 0);
}

// ============================================================
//  RecoverPlana
// ============================================================

#[test]
fn recoverplana_is_alias_for_recoversp() {
    // RecoverPlana は RecoverSP のエイリアス。100% で完全回復。
    let mut app = make_app_with_sp(100, "");
    app.database_mut().unit_instances[0].sp_consumed = 50;
    let stmts = src_core::data::event::parse("RecoverPlana リオ 100\n").unwrap();
    src_core::event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(sp_consumed(&app), 0);
}

#[test]
fn recoverplana_partial_recovery() {
    // RecoverPlana pilot 50 → 50% 回復。max_sp=100 なら 50 ポイント回復。
    let mut app = make_app_with_sp(100, "");
    app.database_mut().unit_instances[0].sp_consumed = 80;
    let stmts = src_core::data::event::parse("RecoverPlana リオ 50\n").unwrap();
    src_core::event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(sp_consumed(&app), 30);
}
