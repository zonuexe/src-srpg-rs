//! パイロット情報関数 (Level / SP / Plana / Relation) の edge cases。
//!
//! SRC.Sharp `Expressions/PilotFunctionTests.cs` から移植。
//! 本実装の制約:
//! - Level は LevelUp で加算される total_exp / 100 + 1 から導出
//! - SP は PilotData.sp(最大値) − UnitInstance.sp_consumed
//! - Plana / Relation は modeling していないため常に 0

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

const PRELUDE: &str = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Pilot "ガロ" ガロ 男性 超能力者 AAAA 100 160 200 180 200 220 180
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
"#;

fn run(extra: &str) -> App {
    let mut app = App::new();
    let src = format!("{PRELUDE}{extra}");
    let stmts = event::parse(&src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

// ──────────────────────────────────────────────
// Level(pilot)
// ──────────────────────────────────────────────

#[test]
fn level_undefined_pilot_returns_zero() {
    let app = run("Set lv Level(存在しない)\n");
    assert_eq!(app.script_var("lv"), "0");
}

#[test]
fn level_defined_pilot_without_unit_returns_one() {
    // Pilot 定義は済んでいるが Place されていないユニット → level 1。
    let app = run("Set lv Level(リオ)\n");
    assert_eq!(app.script_var("lv"), "1");
}

#[test]
fn level_pilot_on_unit_uses_total_exp() {
    // LevelUp で total_exp が 200 (= 2 レベル分) になり level = 2 + 1 = 3。
    // 実装は floor(total_exp / 100) + 1。
    let app = run(r#"
Place "ブレイバー" "リオ" Player 0 0
LevelUp リオ 2
Set lv Level(リオ)
"#);
    assert_eq!(app.script_var("lv"), "3");
}

#[test]
fn level_via_unit_data_name_lookup() {
    let app = run(r#"
Place "ブレイバー" "リオ" Player 0 0
LevelUp リオ 1
Set lv Level(ブレイバー)
"#);
    assert_eq!(app.script_var("lv"), "2");
}

#[test]
fn level_zero_exp_returns_one() {
    let app = run(r#"
Place "ブレイバー" "リオ" Player 0 0
Set lv Level(リオ)
"#);
    assert_eq!(app.script_var("lv"), "1");
}

// ──────────────────────────────────────────────
// SP(pilot)
// ──────────────────────────────────────────────

#[test]
fn sp_unknown_pilot_returns_zero() {
    let app = run("Set s SP(存在しない)\n");
    assert_eq!(app.script_var("s"), "0");
}

#[test]
fn sp_pilot_without_sp_returns_zero() {
    // Pilot 命令の 12-arg 形式は sp フィールドを設定しない → 0。
    let app = run(r#"
Place "ブレイバー" "リオ" Player 0 0
Set s SP(リオ)
"#);
    assert_eq!(app.script_var("s"), "0");
}

// ──────────────────────────────────────────────
// Plana / Relation
// ──────────────────────────────────────────────
// Plana: UnitInstance.plana フィールドに格納。未配置ユニットは 0。
// Relation: SetRelation で __rel_a_b に保存し Relation() で読み出す。

#[test]
fn plana_returns_zero_when_unit_not_placed() {
    // リオは定義済みだが Place されていない → UnitInstance なし → 0
    let app = run("Set p Plana(リオ)\n");
    assert_eq!(app.script_var("p"), "0");
}

#[test]
fn relation_without_setrelation_returns_zero() {
    // SetRelation 未設定の場合は 0。
    let app = run("Set r Relation(リオ, ガロ)\n");
    assert_eq!(app.script_var("r"), "0");
}

#[test]
fn setrelation_and_relation_roundtrip() {
    // SetRelation → Relation() で読み戻せること
    let app = run("SetRelation リオ ガロ 50\nSet r Relation(リオ, ガロ)\n");
    assert_eq!(app.script_var("r"), "50");
}

#[test]
fn setrelation_is_symmetric() {
    // SetRelation は双方向に設定される
    let app = run("SetRelation リオ ガロ 30\nSet r Relation(ガロ, リオ)\n");
    assert_eq!(app.script_var("r"), "30");
}

#[test]
fn relation_unknown_pilots_return_zero() {
    let app = run("Set r Relation(未登録1, 未登録2)\n");
    assert_eq!(app.script_var("r"), "0");
}

// ──────────────────────────────────────────────
// 既存 Morale / Exp の edge cases (再確認)
// ──────────────────────────────────────────────

#[test]
fn morale_unknown_pilot_returns_zero() {
    let app = run("Set m Morale(存在しない)\n");
    assert_eq!(app.script_var("m"), "0");
}

#[test]
fn morale_default_value_is_100() {
    let app = run(r#"
Place "ブレイバー" "リオ" Player 0 0
Set m Morale(リオ)
"#);
    assert_eq!(app.script_var("m"), "100");
}

#[test]
fn exp_unknown_pilot_returns_zero() {
    let app = run("Set e Exp(存在しない)\n");
    assert_eq!(app.script_var("e"), "0");
}

#[test]
fn exp_after_levelup_reflects_total_exp() {
    let app = run(r#"
Place "ブレイバー" "リオ" Player 0 0
LevelUp リオ 3
Set e Exp(リオ)
"#);
    // LevelUp 3 → total_exp += 300
    assert_eq!(app.script_var("e"), "300");
}
