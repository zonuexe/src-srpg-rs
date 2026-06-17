//! ゲーム状態クエリ関数のテスト。
//!
//! `Money()` / `Turn()` / `Phase()` / `Stage()` / `Rank()` / `Exists()` など、
//! コマンドとしても使われるが関数として式中で読み取れる関数群を検証する。
//!
//! 各関数は対応するコマンド (`Money 1000`, `Turn 5` 等) で値を設定してから
//! `Set v FuncName()` で読み出すパターンが基本形。

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
//  Money()
// ============================================================

#[test]
fn money_query_returns_zero_initially() {
    let app = run("Set v Money()");
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn money_query_reflects_money_command() {
    let app = run(r#"
Money 3000
Set v Money()
"#);
    assert_eq!(app.script_var("v"), "3000");
}

#[test]
fn money_query_reflects_incremental_money() {
    let app = run(r#"
Money 500
Money +200
Set v Money()
"#);
    assert_eq!(app.script_var("v"), "700");
}

// ============================================================
//  Turn()
// ============================================================

#[test]
fn turn_query_returns_one_initially() {
    // デフォルトターン番号は 1
    let app = run("Set v Turn()");
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn turn_query_reflects_turn_command() {
    let app = run(r#"
Turn 7
Set v Turn()
"#);
    assert_eq!(app.script_var("v"), "7");
}

#[test]
fn turn_query_reflects_last_turn_command() {
    let app = run(r#"
Turn 3
Turn 10
Set v Turn()
"#);
    assert_eq!(app.script_var("v"), "10");
}

// ============================================================
//  Phase()
// ============================================================

#[test]
fn phase_query_returns_player_initially() {
    // 初期フェーズは Player
    let app = run("Set v Phase()");
    assert_eq!(app.script_var("v"), "Player");
}

// ============================================================
//  Stage()
// ============================================================

#[test]
fn stage_query_returns_empty_initially() {
    let app = run("Set v Stage()");
    assert_eq!(app.script_var("v"), "");
}

#[test]
fn stage_query_reflects_stage_command() {
    let app = run(r#"
Stage "第一話"
Set v Stage()
"#);
    assert_eq!(app.script_var("v"), "第一話");
}

#[test]
fn stage_query_reflects_last_stage_command() {
    let app = run(r#"
Stage "第一話"
Stage "第二話"
Set v Stage()
"#);
    assert_eq!(app.script_var("v"), "第二話");
}

// ============================================================
//  Rank() / BossRank
// ============================================================

#[test]
fn rank_returns_zero_before_bossrank() {
    let app = run("Set v Rank(ゾルダ)");
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn rank_reflects_bossrank_command() {
    let app = run(r#"
BossRank ゾルダ 3
Set v Rank(ゾルダ)
"#);
    assert_eq!(app.script_var("v"), "3");
}

#[test]
fn rank_different_units_independent() {
    let app = run(r#"
BossRank ゾルダ 5
BossRank ゲルグ 2
Set a Rank(ゾルダ)
Set b Rank(ゲルグ)
Set c Rank(ドム)
"#);
    assert_eq!(app.script_var("a"), "5");
    assert_eq!(app.script_var("b"), "2");
    // 設定していないユニットは 0
    assert_eq!(app.script_var("c"), "0");
}

// ============================================================
//  Exists()
// ============================================================

#[test]
fn exists_returns_zero_for_unplaced_unit() {
    // 配置前はユニットが存在しないので 0
    let app = run("Set v Exists(ブレイバー)");
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn exists_returns_one_for_placed_unit() {
    let app = run(r#"
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Place "ブレイバー" "リオ" Player 1 1
Set v Exists(リオ)
"#);
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn exists_returns_zero_after_unit_not_placed() {
    // ユニットデータ登録だけで Place しなければ 0
    let app = run(r#"
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Set v Exists(リオ)
"#);
    assert_eq!(app.script_var("v"), "0");
}

// ============================================================
//  ターン数 システム変数 (読み書き)
// ============================================================

#[test]
fn turn_number_system_var_readable() {
    // `ターン数` は `Turn()` 関数と同じ値を返す。
    let app = run(r#"
Turn 5
Set v $(ターン数)
"#);
    assert_eq!(app.script_var("v"), "5");
}

#[test]
fn set_turn_number_system_var_updates_turn() {
    // `Set ターン数 N` でターン数が変わる。SRC.Sharp `Variable.cs` 準拠。
    let app = run(r#"
Set ターン数 8
Set v Turn()
"#);
    assert_eq!(app.script_var("v"), "8");
}

#[test]
fn incr_turn_number_system_var() {
    // `Incr ターン数` でターン数が加算される。
    let app = run(r#"
Turn 3
Incr ターン数 2
Set v $(ターン数)
"#);
    assert_eq!(app.script_var("v"), "5");
}

// ============================================================
//  総ターン数 システム変数 (読み書き)
// ============================================================

#[test]
fn total_turn_starts_at_zero() {
    let app = run("Set v $(総ターン数)");
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn set_total_turn_system_var() {
    let app = run(r#"
Set 総ターン数 42
Set v $(総ターン数)
"#);
    assert_eq!(app.script_var("v"), "42");
}

#[test]
fn incr_total_turn_system_var() {
    let app = run(r#"
Set 総ターン数 10
Incr 総ターン数 5
Set v $(総ターン数)
"#);
    assert_eq!(app.script_var("v"), "15");
}

// ============================================================
//  フェイズ システム変数 (読み取り専用)
// ============================================================

#[test]
fn phase_system_var_returns_player_initially() {
    // `フェイズ` は現在のフェーズの陣営名を返す。初期値は "味方"。
    let app = run("Set v $(フェイズ)");
    assert_eq!(app.script_var("v"), "味方");
}

// ============================================================
//  資金 システム変数 (読み書き)
// ============================================================

#[test]
fn money_system_var_readable() {
    let app = run(r#"
Money 2500
Set v $(資金)
"#);
    assert_eq!(app.script_var("v"), "2500");
}

#[test]
fn set_money_system_var() {
    // `Set 資金 N` で所持金が N になる。
    let app = run(r#"
Set 資金 9999
Set v $(資金)
"#);
    assert_eq!(app.script_var("v"), "9999");
}

#[test]
fn incr_money_system_var() {
    let app = run(r#"
Set 資金 1000
Incr 資金 500
Set v $(資金)
"#);
    assert_eq!(app.script_var("v"), "1500");
}

// ============================================================
//  ＮＰＣ数 (= 友軍数 エイリアス)
// ============================================================

#[test]
fn npc_count_is_alias_for_allied_count() {
    // `ＮＰＣ数` は `友軍数` と同じ値 (Allied ユニット数) を返す。
    let src = r#"
Unit "友軍機" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Pilot "友軍員" 友軍員 男性 一般 AAAA 100 160 220 200 220 240 200
Place "友軍機" "友軍員" Allied 2 2
Set a $(ＮＰＣ数)
Set b $(友軍数)
"#;
    let app = run(src);
    assert_eq!(app.script_var("a"), "1");
    assert_eq!(app.script_var("b"), "1");
    assert_eq!(app.script_var("a"), app.script_var("b"));
}

// ============================================================
//  Info(オプション, ...) — Optionコマンド連動
// ============================================================

#[test]
fn info_option_returns_off_by_default() {
    let app = run("Set v Info(オプション, NewGUI)\n");
    assert_eq!(app.script_var("v"), "Off");
}

#[test]
fn info_option_returns_on_after_option_command() {
    let app = run("Option NewGUI\nSet v Info(オプション, NewGUI)\n");
    assert_eq!(app.script_var("v"), "On");
}

#[test]
fn info_option_returns_off_after_option_cleared() {
    let app = run("Option NewGUI\nOption NewGUI 解除\nSet v Info(オプション, NewGUI)\n");
    assert_eq!(app.script_var("v"), "Off");
}

// ============================================================
//  Info(ユニット, ...) — ユニットインスタンス情報
// ============================================================

const UNIT_SETUP: &str = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon "ブレイバー" "ライフル" 2500 2 5 15 -1
Place "ブレイバー" "リオ" Player 1 1
"#;

fn run_unit(extra: &str) -> App {
    run(&format!("{UNIT_SETUP}{extra}"))
}

#[test]
fn info_unit_hp_returns_current_hp() {
    let app = run_unit("Set v Info(ユニット, リオ, HP)\n");
    assert_eq!(app.script_var("v"), "3500");
}

#[test]
fn info_unit_max_hp_returns_max() {
    let app = run_unit("Set v Info(ユニット, リオ, 最大ＨＰ)\n");
    assert_eq!(app.script_var("v"), "3500");
}

#[test]
fn info_unit_hp_after_damage() {
    let app = run_unit("Damage リオ 500\nSet v Info(ユニット, リオ, HP)\n");
    assert_eq!(app.script_var("v"), "3000");
}

#[test]
fn info_unit_en_returns_current_en() {
    let app = run_unit("Set v Info(ユニット, リオ, EN)\n");
    assert_eq!(app.script_var("v"), "120");
}

#[test]
fn info_unit_armor() {
    let app = run_unit("Set v Info(ユニット, リオ, 装甲)\n");
    assert_eq!(app.script_var("v"), "1200");
}

#[test]
fn info_unit_mobility() {
    let app = run_unit("Set v Info(ユニット, リオ, 運動性)\n");
    assert_eq!(app.script_var("v"), "110");
}

#[test]
fn info_unit_morale() {
    let app = run_unit("Set v Info(ユニット, リオ, 気力)\n");
    assert_eq!(app.script_var("v"), "100");
}

#[test]
fn info_unit_data_name() {
    let app = run_unit("Set v Info(ユニットデータ, ブレイバー, 名称)\n");
    assert_eq!(app.script_var("v"), "ブレイバー");
}

#[test]
fn info_unit_data_max_hp() {
    let app = run_unit("Set v Info(ユニットデータ, ブレイバー, 最大ＨＰ)\n");
    assert_eq!(app.script_var("v"), "3500");
}

#[test]
fn info_unit_data_weapon_power() {
    let app = run_unit("Set v Info(ユニットデータ, ブレイバー, 武器, ライフル, 攻撃力)\n");
    assert_eq!(app.script_var("v"), "2500");
}

#[test]
fn info_unit_data_weapon_by_number() {
    let app = run_unit("Set v Info(ユニットデータ, ブレイバー, 武器, 1)\n");
    assert_eq!(app.script_var("v"), "ライフル");
}

// ============================================================
//  Info(パイロット, ...) — パイロット情報
// ============================================================

#[test]
fn info_pilot_name() {
    let app = run_unit("Set v Info(パイロット, リオ, 名称)\n");
    assert_eq!(app.script_var("v"), "リオ");
}

#[test]
fn info_pilot_sex() {
    let app = run_unit("Set v Info(パイロット, リオ, 性別)\n");
    assert_eq!(app.script_var("v"), "男性");
}

#[test]
fn info_pilot_infight() {
    // パイロット (実体) は level 成長後。リオ は level 1 ＝ base 160 + lv 1 = 161
    // (VB6 `Pilot.cls:582-593`、格闘 += lv)。パイロットデータ (静的) は素の 160。
    let app = run_unit("Set v Info(パイロット, リオ, 格闘)\n");
    assert_eq!(app.script_var("v"), "161");
}

#[test]
fn info_pilot_shooting() {
    // 射撃 += lv。base 220 + lv 1 = 221。
    let app = run_unit("Set v Info(パイロット, リオ, 射撃)\n");
    assert_eq!(app.script_var("v"), "221");
}

#[test]
fn info_pilot_morale() {
    let app = run_unit("Set v Info(パイロット, リオ, 気力)\n");
    assert_eq!(app.script_var("v"), "100");
}

#[test]
fn info_pilot_level() {
    let app = run_unit("Set v Info(パイロット, リオ, レベル)\n");
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn info_pilot_data_infight() {
    let app = run_unit("Set v Info(パイロットデータ, リオ, 格闘)\n");
    assert_eq!(app.script_var("v"), "160");
}

#[test]
fn info_nonexistent_unit_returns_empty() {
    let app = run_unit("Set v Info(ユニット, 存在しない, HP)\n");
    assert_eq!(app.script_var("v"), "");
}

#[test]
fn info_nonexistent_pilot_returns_empty() {
    let app = run_unit("Set v Info(パイロット, 存在しない, 名称)\n");
    assert_eq!(app.script_var("v"), "");
}

#[test]
fn exists_returns_one_after_escape() {
    // Escape はマップから退避するが UnitInstance は残る → Exists = 1。
    let app = run(r#"
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Place "ブレイバー" "リオ" Player 1 1
Escape リオ
Set v Exists(リオ)
"#);
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn exists_returns_zero_after_kill() {
    // Kill は UnitInstance を削除 → Exists = 0。
    let app = run(r#"
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Place "ブレイバー" "リオ" Player 1 1
Kill リオ
Set v Exists(リオ)
"#);
    assert_eq!(app.script_var("v"), "0");
}

// ============================================================
//  Status() 関数
// ============================================================

#[test]
fn status_returns_sortie_for_placed_unit() {
    // 配置直後は "出撃" 状態。
    let app = run(r#"
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Place "ブレイバー" "リオ" Player 1 1
Set v Status(リオ)
"#);
    assert_eq!(app.script_var("v"), "出撃");
}

#[test]
fn status_returns_rittai_after_leave() {
    // Leave 後は "離脱" 状態。
    let app = run(r#"
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Place "ブレイバー" "リオ" Player 1 1
Leave リオ
Set v Status(リオ)
"#);
    assert_eq!(app.script_var("v"), "離脱");
}
