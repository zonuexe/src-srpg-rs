//! 必要技能 / 必要条件ゲートの end-to-end テスト。
//!
//! `(念力Lv3)` 形式の括弧条件をパーサ→評価器→`necessary_skill::weapon_usable_by` (App の武器ゲートと同じ経路) で検証する。
//! パイロットは静的 `PilotData.features` 経由 (pilot_ids 空時のフォールバック) で解決。

use src_core::data::pilot::{Adaption, PilotData, Sex};
use src_core::data::unit::{Size, UnitData, WeaponData};
use src_core::db::GameDatabase;
use src_core::necessary_skill;
use src_core::unit_instance::{Party, UnitInstance};

fn pilot_with_features(name: &str, features: Vec<(&str, &str)>) -> PilotData {
    PilotData {
        spirit_commands: Vec::new(),
        name: name.into(),
        nickname: name.into(),
        kana_name: name.into(),
        sex: Sex::Male,
        class: String::new(),
        adaption: Adaption::parse("AAAA").unwrap(),
        exp_value: 0,
        infight: 100,
        shooting: 100,
        hit: 100,
        dodge: 100,
        intuition: 100,
        technique: 100,
        personality: None,
        sp: None,
        bgm: None,
        bitmap: None,
        features: features
            .into_iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect(),
    }
}

/// 必要技能 `skill` を末尾 (`extras[0]`) に持つ武器を 1 つ積んだユニットデータ。
fn unit_data_with_gated_weapon(necessary_skill: &str) -> UnitData {
    let weapon = WeaponData {
        name: "ゲート武器".to_string(),
        power: 1000,
        min_range: 1,
        max_range: 5,
        precision: 10,
        bullet: -1, // 無限弾
        en_consumption: 0,
        necessary_morale: 0,
        adaption: String::new(),
        critical: 0,
        class: "武".to_string(),
        extras: vec![necessary_skill.to_string(), String::new()],
    };
    UnitData {
        abilities: Vec::new(),
        name: "テスト機".to_string(),
        kana_name: "てすとき".to_string(),
        nickname: "テスト".to_string(),
        class: "ロボ".to_string(),
        pilot_num: 1,
        item_num: 0,
        transportation: "陸".to_string(),
        speed: 100,
        size: Size::M,
        value: 0,
        exp_value: 0,
        hp: 5000,
        en: 100,
        armor: 500,
        mobility: 120,
        adaption: Adaption::parse("AAAA").unwrap(),
        bitmap: String::new(),
        weapons: vec![weapon],
        features: Vec::new(),
    }
}

/// App の武器使用資格ゲート (`App::weapon_skill_ok`) と同じ判定を 0 番武器に適用する。
fn gate_ok(unit: &UnitInstance, db: &GameDatabase) -> bool {
    let wd = &db
        .unit_by_name(&unit.unit_data_name)
        .expect("unit data")
        .weapons[0];
    necessary_skill::weapon_usable_by(wd, unit, db)
}

fn setup(necessary_skill: &str, pilot: PilotData) -> (GameDatabase, UnitInstance) {
    let mut db = GameDatabase::new();
    db.extend_units(vec![unit_data_with_gated_weapon(necessary_skill)]);
    let pilot_name = pilot.name.clone();
    db.extend_pilots(vec![pilot]);
    let mut unit = UnitInstance::new("テスト機", &pilot_name, Party::Player, 0, 0);
    unit.weapons
        .push(src_core::unit_weapon::UnitWeapon::from_data(
            "ゲート武器",
            0,
            -1,
        ));
    (db, unit)
}

#[test]
fn weapon_sealed_when_pilot_lacks_required_skill() {
    // 念力Lv3 を要求する武器。パイロットは念力を持たない → 封印。
    let (db, unit) = setup("念力Lv3", pilot_with_features("無能力者", vec![]));
    assert!(
        !gate_ok(&unit, &db),
        "念力を持たないパイロットの武器が使用可能になっている"
    );
}

#[test]
fn weapon_available_when_pilot_has_required_skill() {
    // 念力Lv3 を持つ → 使用可能。
    let (db, unit) = setup(
        "念力Lv3",
        pilot_with_features("超能力者", vec![("念力Lv3", "1")]),
    );
    assert!(
        gate_ok(&unit, &db),
        "念力Lv3 を持つパイロットの武器が封印されている"
    );
}

#[test]
fn weapon_sealed_when_skill_level_insufficient() {
    // 念力Lv3 要求に対しパイロットは念力Lv2 のみ → 不足で封印。
    let (db, unit) = setup(
        "念力Lv3",
        pilot_with_features("見習い", vec![("念力Lv2", "1")]),
    );
    assert!(!gate_ok(&unit, &db));
}

#[test]
fn kill_count_gate_via_static_feature() {
    // 撃墜数Lv20 ゲート。エース (撃墜数Lv100) は解禁、ザコ (撃墜数なし) は封印。
    let (db_ace, ace) = setup(
        "撃墜数Lv20",
        pilot_with_features("エース", vec![("撃墜数Lv100", "1")]),
    );
    assert!(gate_ok(&ace, &db_ace), "エースの撃墜数武器が封印されている");

    let (db_mob, mob) = setup("撃墜数Lv20", pilot_with_features("ザコ", vec![]));
    assert!(
        !gate_ok(&mob, &db_mob),
        "撃墜数 0 のザコが撃墜数Lv20 武器を使えてしまう"
    );
}

#[test]
fn morale_gate() {
    // 気力Lv3 = 気力 130 以上で使用可能。
    let (db, mut unit) = setup("気力Lv3", pilot_with_features("熱血漢", vec![]));
    unit.morale = 120;
    assert!(!gate_ok(&unit, &db), "気力120 で気力Lv3 武器が使える");
    unit.morale = 130;
    assert!(gate_ok(&unit, &db), "気力130 で気力Lv3 武器が封印される");
}

#[test]
fn negation_gate() {
    // !瀕死 = 瀕死でないとき使用可能。
    let (db, mut unit) = setup("!瀕死", pilot_with_features("剣士", vec![]));
    // 満タン (瀕死でない) → 使用可能。
    assert!(gate_ok(&unit, &db));
    // 瀕死 (HP <= 1/4) にする。max_hp=5000 → 1/4=1250。damage=4000 → hp=1000。
    unit.damage = 4000;
    assert!(!gate_ok(&unit, &db), "瀕死なのに !瀕死 武器が使用可能");
}

#[test]
fn or_grouping() {
    // (念力Lv3 or 剣術Lv1): どちらかを満たせば良い。
    let (db_a, ua) = setup(
        "念力Lv3 or 剣術Lv1",
        pilot_with_features("剣士", vec![("剣術Lv1", "1")]),
    );
    assert!(gate_ok(&ua, &db_a), "剣術を持つのに OR 条件で封印");

    let (db_b, ub) = setup(
        "念力Lv3 or 剣術Lv1",
        pilot_with_features("無能力者", vec![]),
    );
    assert!(!gate_ok(&ub, &db_b), "どちらも持たないのに OR 条件で解禁");
}

#[test]
fn and_grouping() {
    // 念力Lv3 剣術Lv1: 両方必要。
    let pilot_both = pilot_with_features("達人", vec![("念力Lv3", "1"), ("剣術Lv1", "1")]);
    let (db, unit) = setup("念力Lv3 剣術Lv1", pilot_both);
    assert!(gate_ok(&unit, &db));

    let (db2, unit2) = setup(
        "念力Lv3 剣術Lv1",
        pilot_with_features("片手落ち", vec![("念力Lv3", "1")]),
    );
    assert!(!gate_ok(&unit2, &db2), "剣術が無いのに AND 条件で解禁");
}

#[test]
fn unmodeled_condition_fails_open() {
    // 同調率系はモデル化していない → fail-open (誤封印しない)。
    let (db, unit) = setup("同調率Lv5", pilot_with_features("操縦士", vec![]));
    assert!(
        gate_ok(&unit, &db),
        "未モデル条件で武器が誤封印されている (fail-open であるべき)"
    );
}

#[test]
fn empty_requirement_is_noop() {
    assert!(necessary_skill::is_satisfied(
        "",
        &dummy_unit(),
        &GameDatabase::new()
    ));
}

fn dummy_unit() -> UnitInstance {
    UnitInstance::new("x", "", Party::Player, 0, 0)
}

#[test]
fn weapon_parser_strips_necessary_skill_from_class() {
    // 武器行をパースし class クリーン化 + extras=[skill,cond] を確認。
    let src = "\
テスト機
テスト機,てすとき,リアル系,1,4
陸宇,5,M,3000,400
特殊能力なし
3500,120,1200,110
AAAA,Test.bmp
念動斬り,1350,1,1,19,-1,0,0,AAAA,5,武 (念力Lv3)
===
";
    let units = src_core::data::unit::parse(src).expect("parse ok");
    let w = &units[0].weapons[0];
    assert_eq!(w.class, "武", "class から必要技能が剥がれていない");
    assert_eq!(w.necessary_skill(), "念力Lv3");
    assert_eq!(w.necessary_condition(), "");
}

#[test]
fn weapon_parser_splits_condition_and_skill() {
    let src = "\
テスト機
テスト機,てすとき,リアル系,1,4
陸宇,5,M,3000,400
特殊能力なし
3500,120,1200,110
AAAA,Test.bmp
森のカーニバル,4500,3,8,5,-1,60,120,AAAA,10,格 <@森 or @林> (術Lv4)
===
";
    let units = src_core::data::unit::parse(src).expect("parse ok");
    let w = &units[0].weapons[0];
    assert_eq!(w.class, "格");
    assert_eq!(w.necessary_skill(), "術Lv4");
    assert_eq!(w.necessary_condition(), "@森 or @林");
}
