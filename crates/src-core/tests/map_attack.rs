//! MapAttack / MapAbility コマンドの edge cases。
//!
//! SRC.Sharp の MapAttackCmd は「攻撃結果以外のイベントを発生させない」と
//! 文書化されている (経験値 / 資金 / 損傷率 / 破壊イベント無効)。本テストは
//! Rust 実装の動作 — 攻撃元と同陣営は除外、area 内の敵対勢力にのみダメージ、
//! HP <= 0 でユニット除去、未知の unit / weapon は no-op — を確認する。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

const PRELUDE: &str = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot "ガロ" ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot "ノヴァ" ノヴァ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit "ブレイバー" Real 1 0 陸 5 M 1000 100 5000 100 1500 100 AAAA
Unit "ゾルダII" Mass 1 0 陸 5 M 1000 100 2000 80 800 80 BBBB
Unit "アークシップ" 母艦 1 0 空 4 L 5000 200 8000 200 2000 50 ACBA
Weapon "ブレイバー" "メガキャノン" 8000 1 3 +0 0
Weapon "ブレイバー" "ビームライフル" 2500 2 5 +0 -1
"#;

fn run(extra: &str) -> App {
    let mut app = App::new();
    let src = format!("{PRELUDE}{extra}");
    let stmts = event::parse(&src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

fn alive_count(app: &App, party: src_core::Party) -> usize {
    app.database()
        .unit_instances
        .iter()
        .filter(|u| u.party == party)
        .count()
}

fn damage_of(app: &App, pilot_name: &str) -> i64 {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.pilot_name == pilot_name)
        .map(|u| u.damage)
        .unwrap_or(0)
}

#[test]
fn mapattack_destroys_enemy_in_fallback_area() {
    // class が空の Weapon は fallback 経路 (target 中心の菱形 radius=max_range)。
    // メガキャノン (power 8000) はゾルダ (armor 800, hp 1000) を即死させる。
    let app = run(r#"
Place "ブレイバー" "リオ" 味方 5 5
Place "ゾルダII" "ガロ" 敵 6 5
MapAttack リオ メガキャノン 6 5
"#);
    assert_eq!(alive_count(&app, src_core::Party::Enemy), 0);
}

#[test]
fn mapattack_respects_revive_spirit() {
    // 精神コマンド「復活」を持つ敵はマップ兵器で撃破されても立ち上がる (通常戦闘と同様)。
    let app = run(r#"
Place "ブレイバー" "リオ" 味方 5 5
Place "ゾルダII" "ガロ" 敵 6 5
SetStatus ガロ 復活
MapAttack リオ メガキャノン 6 5
"#);
    assert_eq!(
        alive_count(&app, src_core::Party::Enemy),
        1,
        "復活で生存 (除去されない)"
    );
    let revived = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.pilot_name == "ガロ")
        .expect("ガロ は復活で残存");
    assert!(!revived.has_condition("復活"), "復活 は 1 回で消費される");
    assert_eq!(revived.damage, 0, "復活で HP 全快 (damage=0)");
}

#[test]
fn mapattack_spares_friendly_in_area() {
    // 同じマスに味方が居ても、攻撃側陣営 (味方) は対象外。
    let app = run(r#"
Place "ブレイバー" "リオ" 味方 5 5
Place "アークシップ" "ノヴァ" 味方 6 5
Place "ゾルダII" "ガロ" 敵 7 5
MapAttack リオ メガキャノン 6 5
"#);
    // アークシップ(味方) 健在, ゾルダ(敵) 撃破
    assert_eq!(alive_count(&app, src_core::Party::Player), 2);
    assert_eq!(alive_count(&app, src_core::Party::Enemy), 0);
}

#[test]
fn mapattack_skips_targets_outside_range() {
    // メガキャノン max_range=3 → target (6,5) を中心とした菱形半径 3。
    // (10, 5) はマンハッタン距離 4 で範囲外。
    let app = run(r#"
Place "ブレイバー" "リオ" 味方 5 5
Place "ゾルダII" "ガロ" 敵 10 5
MapAttack リオ メガキャノン 6 5
"#);
    // 範囲外の敵は無傷で残る
    assert_eq!(alive_count(&app, src_core::Party::Enemy), 1);
    assert_eq!(damage_of(&app, "ガロ"), 0);
}

#[test]
fn mapattack_unknown_weapon_is_noop() {
    // 武器名が間違っていれば、何のダメージも与えない。
    let app = run(r#"
Place "ブレイバー" "リオ" 味方 5 5
Place "ゾルダII" "ガロ" 敵 6 5
MapAttack リオ 存在しない武器 6 5
"#);
    assert_eq!(alive_count(&app, src_core::Party::Enemy), 1);
    assert_eq!(damage_of(&app, "ガロ"), 0);
}

#[test]
fn mapattack_unknown_attacker_is_noop() {
    // 攻撃元ユニットが居なければ何も起こらない。
    let app = run(r#"
Place "ゾルダII" "ガロ" 敵 6 5
MapAttack 存在しない メガキャノン 6 5
"#);
    assert_eq!(damage_of(&app, "ガロ"), 0);
}

#[test]
fn mapattack_no_attacker_no_targets() {
    // 味方が居らず attacker unit_key も省略すると、Player の候補が無いため no-op。
    let app = run(r#"
Place "ゾルダII" "ガロ" 敵 6 5
MapAttack メガキャノン 6 5
"#);
    // attacker 不在で全 unit 健在
    assert_eq!(alive_count(&app, src_core::Party::Enemy), 1);
}

#[test]
fn mapattack_player_default_attacker_when_unit_key_omitted() {
    // unit_key 省略 (`MapAttack weapon X Y`) → Player の最初のユニットを attacker とみなす。
    let app = run(r#"
Place "ブレイバー" "リオ" 味方 5 5
Place "ゾルダII" "ガロ" 敵 6 5
MapAttack メガキャノン 6 5
"#);
    // 攻撃側がリオ (味方) と認識され、敵ゾルダが範囲内 → 撃破。
    assert_eq!(alive_count(&app, src_core::Party::Enemy), 0);
}

#[test]
fn mapability_alias_works_like_mapattack() {
    // MapAbility は MapAttack の汎用版 (同じ処理に分岐)。
    let app = run(r#"
Place "ブレイバー" "リオ" 味方 5 5
Place "ゾルダII" "ガロ" 敵 6 5
MapAbility リオ メガキャノン 6 5
"#);
    assert_eq!(alive_count(&app, src_core::Party::Enemy), 0);
}

#[test]
fn mapattack_multiple_enemies_in_range_all_damaged() {
    // 範囲内の複数敵が同時にダメージを受ける。
    let app = run(r#"
Place "ブレイバー" "リオ" 味方 5 5
Place "ゾルダII" "ガロ" 敵 6 5
Place "ゾルダII" "ノヴァ" 敵 7 5
ChangeParty ノヴァ 敵
MapAttack リオ メガキャノン 6 5
"#);
    // 同じ場所が target なら半径 3 内に両方含まれる → 両方撃破。
    assert_eq!(alive_count(&app, src_core::Party::Enemy), 0);
}

#[test]
fn mapattack_does_not_award_money_or_exp() {
    // SRC.Sharp 仕様: マップ攻撃で標的を破壊しても経験値 / 資金は入らない。
    let app = run(r#"
Money 10000
Place "ブレイバー" "リオ" 味方 5 5
Place "ゾルダII" "ガロ" 敵 6 5
MapAttack リオ メガキャノン 6 5
"#);
    // money 不変
    assert_eq!(app.money(), 10000);
    // attacker の total_exp 不変
    let attacker = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.pilot_name == "リオ")
        .unwrap();
    assert_eq!(attacker.total_exp, 0);
}
