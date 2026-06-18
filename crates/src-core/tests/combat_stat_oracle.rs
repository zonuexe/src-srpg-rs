//! 戦闘 stat 計算のオラクル準拠テスト。
//!
//! 原典 SRC.Sharp `SRCCoreTests/Units/UnitAdaptionArmorTests.cs` 他で確認した
//! 地形適応倍率テーブルを、本実装 `combat::adaptation_mult` に突き合わせる。
//!
//! SRC `戦闘システム詳細.md` / `Unit.props.cs` の既定テーブル:
//!   S=1.4 / A=1.2 / B=1.0 / C=0.8 / D=0.6 / `-`=0.0
//! 本実装はこのテーブルと一致することを確認済 (この pin で回帰を防ぐ)。
//!
//! 本ファイルは適応倍率・改造増分 (HP+200/装甲+100/EN+10)・exp→level (500/level) を
//! pin する。改造 HP/装甲と exp→level は当初原典と乖離していたが SRC 原典値へ是正済
//! (docs/SRC_SHARP_DIVERGENCE.md §4 参照)。
//!
//! 著作権配慮: SRC オリジナルコードは含まない。値のみ移植。

use src_core::combat::adaptation_mult;
use src_core::data::event;
use src_core::event_runtime;
use src_core::pilot_instance::level_from_exp;
use src_core::App;

fn approx(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-9
}

#[test]
fn adaptation_multiplier_default_table() {
    assert!(approx(adaptation_mult(b'S'), 1.4));
    assert!(approx(adaptation_mult(b'A'), 1.2));
    assert!(approx(adaptation_mult(b'B'), 1.0));
    assert!(approx(adaptation_mult(b'C'), 0.8));
    assert!(approx(adaptation_mult(b'D'), 0.6));
}

#[test]
fn adaptation_multiplier_dash_is_zero() {
    // `-` (適応なし) は 0.0 倍 = その地形で戦闘不可。
    assert!(approx(adaptation_mult(b'-'), 0.0));
}

#[test]
fn adaptation_multiplier_lowercase_matches_uppercase() {
    // 小文字も同値 (SRC データは大文字だが堅牢性のため)。
    assert!(approx(adaptation_mult(b's'), 1.4));
    assert!(approx(adaptation_mult(b'd'), 0.6));
}

// ============================================================
//  機体改造 (Rank) による増分 — SRC 原典 `Unit.cls:1719-1722` 準拠
//  HP +200 / EN +10 / 装甲 +100 / 運動性 +5 (各段)。
//  base 値非依存にするため upgrade_level=0 との delta で検証する。
// ============================================================

fn place_unit() -> App {
    let src = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Place "ブレイバー" "リオ" Player 0 0
"#;
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

#[test]
fn upgrade_level_increments_match_src() {
    let mut app = place_unit();
    let (base_hp, base_en, base_armor) = {
        let db = app.database();
        let u = &db.unit_instances[0];
        (
            db.effective_max_hp(u),
            db.effective_max_en(u),
            db.effective_armor(u),
        )
    };

    app.database_mut().unit_instances[0].upgrade_level = 3;

    let db = app.database();
    let u = &db.unit_instances[0];
    assert_eq!(db.effective_max_hp(u) - base_hp, 600, "HP +200/段 × 3");
    assert_eq!(db.effective_max_en(u) - base_en, 30, "EN +10/段 × 3");
    assert_eq!(db.effective_armor(u) - base_armor, 300, "装甲 +100/段 × 3");
}

// ============================================================
//  パイロットレベル算出 — SRC 原典 `Pilot.cls:1183` 準拠で 500 exp = 1 level
//  (本実装は以前 100 exp/level で 5 倍速成長していた乖離を是正)。
// ============================================================

#[test]
fn level_from_exp_is_500_per_level() {
    assert_eq!(level_from_exp(0), 1);
    assert_eq!(level_from_exp(499), 1);
    assert_eq!(level_from_exp(500), 2); // 境界
    assert_eq!(level_from_exp(999), 2);
    assert_eq!(level_from_exp(1000), 3);
    assert_eq!(level_from_exp(2000), 5); // (Level-1)*500 = total_exp
    assert_eq!(level_from_exp(49000), 99); // 上限直前
    assert_eq!(level_from_exp(1_000_000), 99); // 上限 99 クランプ
    assert_eq!(level_from_exp(-100), 1); // 負は 0 扱い
}

// ============================================================
//  機体改造 (Rank) による武器攻撃力加算 — SRC.NET `Unit.cs` UpdateWeaponPower 準拠。
//  通常武器 +100×Rank / 固 据え置き / Ｒ・改 (レベル無) +50×Rank /
//  Ｒ・改 (ＲL<n>/改L<n>) +10×n×Rank / 攻撃力 0 据え置き。
//  旧実装は `+base×10%×Rank` の乗算で C# の **加算** と乖離していた
//  (差分オラクル combat_rank_level.txt で発覚: 改造ユニットの攻撃ダメージが過大)。
// ============================================================
#[test]
fn upgrade_weapon_power_matches_src_additive_rule() {
    use src_core::data::pilot::{Adaption, PilotData, Sex};
    use src_core::data::unit::{Size, UnitData, WeaponData};
    use src_core::{Party, UnitInstance};

    let mk_weapon = |name: &str, power: i64, class: &str| WeaponData {
        name: name.into(),
        power,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: class.into(),
        extras: Vec::new(),
    };

    let mut app = App::new();
    app.database_mut().pilots.push(PilotData {
        spirit_commands: Vec::new(),
        name: "P".into(),
        nickname: "P".into(),
        kana_name: "P".into(),
        sex: Sex::Unspecified,
        class: String::new(),
        adaption: Adaption::parse("AAAA").unwrap(),
        exp_value: 0,
        infight: 100,
        shooting: 100,
        hit: 0,
        dodge: 0,
        intuition: 0,
        technique: 0,
        personality: None,
        sp: None,
        bgm: None,
        bitmap: None,
        features: Vec::new(),
    });
    app.database_mut().units.push(UnitData {
        abilities: Vec::new(),
        name: "改造機".into(),
        kana_name: String::new(),
        nickname: "改造機".into(),
        class: String::new(),
        pilot_num: 1,
        item_num: 0,
        transportation: "陸".into(),
        speed: 5,
        size: Size::M,
        value: 0,
        exp_value: 0,
        hp: 3000,
        en: 100,
        armor: 500,
        mobility: 100,
        adaption: Adaption::parse("AAAA").unwrap(),
        bitmap: String::new(),
        weapons: vec![
            mk_weapon("通常", 1500, "実"),     // 通常 → +100×Rank
            mk_weapon("固定", 1000, "固"),     // 固 → 据え置き
            mk_weapon("Ｒ無", 1200, "実Ｒ"),   // Ｒ(レベル無) → +50×Rank
            mk_weapon("Ｒ有", 1200, "実ＲL3"), // ＲL3 → +10×3×Rank = +30×Rank
            mk_weapon("ゼロ", 0, "実"),        // 攻撃力 0 → 据え置き
        ],
        features: Vec::new(),
    });
    app.database_mut()
        .register_unit(UnitInstance::new("改造機", "P", Party::Player, 0, 0));
    app.database_mut().unit_instances[0].upgrade_level = 5;

    let (_pilot, unit) = app
        .database()
        .effective_combat_data(0)
        .expect("combat data");
    assert_eq!(unit.weapons[0].power, 1500 + 100 * 5, "通常武器 +100×Rank");
    assert_eq!(unit.weapons[1].power, 1000, "固 は据え置き");
    assert_eq!(
        unit.weapons[2].power,
        1200 + 50 * 5,
        "Ｒ(レベル無) +50×Rank"
    );
    assert_eq!(unit.weapons[3].power, 1200 + 30 * 5, "ＲL3 +10×3×Rank");
    assert_eq!(unit.weapons[4].power, 0, "攻撃力 0 は据え置き");
}
