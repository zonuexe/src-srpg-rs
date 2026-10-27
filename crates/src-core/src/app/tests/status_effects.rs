//! 状態異常・回復系特殊能力・霊力・ボスランク等、フェイズをまたぐ状態のテスト。

use super::*;

/// 回復系特殊能力 ＨＰ回復Lv*/ＥＮ回復Lv* は当該陣営フェイズ開始時に
/// 実効最大値の 10×Lv% を回復する。
#[test]
fn regen_features_heal_hp_and_en_at_player_phase_start() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Regenner", 2, 6);
    // 勝敗即決回避の敵 (遠方)。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Regenner",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.active_features = vec![
            crate::feature::ActiveFeature::new("ＨＰ回復Lv2", ""),
            crate::feature::ActiveFeature::new("ＥＮ回復Lv2", ""),
        ];
        u.damage = 50; // 最大HP=100
        u.en_consumed = 20; // 最大EN=50
    }
    // 1 周回って味方フェイズ T2 開始 → 回復適用。
    app.handle_input(Input::EndPhase);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(u.damage, 30, "ＨＰ回復Lv2 = 最大HP 20% (20) 回復");
    assert_eq!(
        u.en_consumed, 5,
        "ＥＮ回復Lv2 = 最大EN 20% (10) 回復 + 基礎EN自然回復 (5) = 15 回復 (20 -> 5)"
    );
}

/// 基礎 EN 自然回復 (VB5 `Unit.cls:28182`「ＥＮは毎ターン5回復」): 回復系特殊能力を
/// 持たないユニットでもフェイズ開始時に EN +5 される。
#[test]
fn base_en_regen_applies_without_any_recovery_feature() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Regenner", 2, 6);
    // 勝敗即決回避の敵 (遠方)。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Regenner",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.active_features = vec![];
        u.en_consumed = 20; // 最大EN=50
    }
    // 1 周回って味方フェイズ T2 開始 → 基礎EN自然回復 (+5) のみ適用。
    app.handle_input(Input::EndPhase);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(
        u.en_consumed, 15,
        "回復系特殊能力が無くても基礎EN自然回復 (+5) は適用される (20 -> 15)"
    );
}

/// 基礎 EN 自然回復は `回復不能` (特殊効果攻撃属性 害) を持つユニットには適用されない。
#[test]
fn base_en_regen_skipped_for_no_regen_condition() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Regenner", 2, 6);
    // 勝敗即決回避の敵 (遠方)。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Regenner",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.active_features = vec![];
        u.en_consumed = 20; // 最大EN=50
        u.add_condition(crate::Condition::new("回復不能", -1));
    }
    // 1 周回って味方フェイズ T2 開始 → 回復不能により基礎EN自然回復も阻害される。
    app.handle_input(Input::EndPhase);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(
        u.en_consumed, 20,
        "回復不能により基礎EN自然回復も阻害される (据え置き)"
    );
}

/// `effective_max_plana`: 霊力Lv20・パイロットLv10・霊力成長なし →
/// `20 + round_ties_even(1.5*10) = 20 + 15 = 35`。VB5 `Pilot.cls:1423`。
#[test]
fn effective_max_plana_basic_formula() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    let uid = place_player_unit_with_plana_pilot(
        &mut app,
        "Reiryoku",
        "REI",
        vec![("霊力Lv20".into(), "1".into())],
        10,
        2,
        6,
    );
    let inst = app.database().unit_by_uid(&uid).unwrap().clone();
    assert_eq!(
        app.database().effective_max_plana(&inst),
        35,
        "20 + round_ties_even(1.5*10) = 20 + 15 = 35"
    );
}

/// `霊力` 技能を持たないパイロットは MaxPlana = 0 (プラーナ非搭載)。
#[test]
fn effective_max_plana_zero_without_reiryoku_skill() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    let uid =
        place_player_unit_with_plana_pilot(&mut app, "NoReiryoku", "PLAIN2", Vec::new(), 50, 2, 6);
    let inst = app.database().unit_by_uid(&uid).unwrap().clone();
    assert_eq!(
        app.database().effective_max_plana(&inst),
        0,
        "霊力技能なしは MaxPlana=0"
    );
}

/// `効果Lv23` 実際の `霊力成長` 込み: `霊力Lv20` / `霊力成長Lv5` / パイロットLv10 →
/// `20 + round_ties_even(1.5*10*(10+5)/10) = 20 + round_ties_even(22.5) = 20 + 22 = 42`
/// (`round_ties_even(22.5)` は最近接偶数の 22)。VB5 `Pilot.cls:1451-1454`。
#[test]
fn effective_max_plana_growth_skill_formula() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    let uid = place_player_unit_with_plana_pilot(
        &mut app,
        "ReiryokuGrowth",
        "REIG",
        vec![
            ("霊力Lv20".into(), "1".into()),
            ("霊力成長Lv5".into(), "1".into()),
        ],
        10,
        2,
        6,
    );
    let inst = app.database().unit_by_uid(&uid).unwrap().clone();
    assert_eq!(
        app.database().effective_max_plana(&inst),
        42,
        "20 + round_ties_even(1.5*10*(10+5)/10) = 20 + round_ties_even(22.5) = 20 + 22 = 42"
    );
}

/// 霊力 (プラーナ) 自然回復: MaxPlana=35 (霊力Lv20・パイロットLv10) のユニットは
/// 味方フェイズ開始時に `plana += 35/16 = 2` (霊力回復/霊力消費 特殊能力なし)。
/// VB5 `Unit.cls:27374`。
#[test]
fn plana_regen_applies_at_player_phase_start() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    let uid = place_player_unit_with_plana_pilot(
        &mut app,
        "PlanaRegen",
        "REIP",
        vec![("霊力Lv20".into(), "1".into())],
        10,
        2,
        6,
    );
    // 勝敗即決回避の敵 (遠方)。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "PlanaRegen",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.plana = 0;
    }
    // 1 周回って味方フェイズ T2 開始 → 霊力回復適用。
    app.handle_input(Input::EndPhase);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(u.plana, 2, "MaxPlana=35 のとき 35/16 == 2");
}

/// 霊力 (プラーナ) 自然回復は HP/EN と異なり `回復不能` (特殊効果攻撃属性 害) でも
/// 阻害されない (VB5 では回復不能ガードの外)。同一ユニットで HP/EN 自然回復は
/// 阻害されることも併せて確認する。
#[test]
fn plana_regen_not_gated_by_no_regen_condition() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    let uid = place_player_unit_with_plana_pilot(
        &mut app,
        "PlanaNoRegenGate",
        "REIN",
        vec![("霊力Lv20".into(), "1".into())],
        10,
        2,
        6,
    );
    // 勝敗即決回避の敵 (遠方)。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "PlanaNoRegenGate",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.plana = 0;
        u.en_consumed = 20; // 最大EN=50
        u.add_condition(crate::Condition::new("回復不能", -1));
    }
    // 1 周回って味方フェイズ T2 開始。
    app.handle_input(Input::EndPhase);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(
        u.plana, 2,
        "回復不能でも霊力回復 (35/16 == 2) は阻害されない"
    );
    assert_eq!(
        u.en_consumed, 20,
        "対照: 回復不能により基礎EN自然回復は阻害される (据え置き)"
    );
}

/// `Disable unit ＨＰ回復` / `Enable unit ＨＰ回復` (レベル無し基底名) が実体
/// `ＨＰ回復Lv1` の is_active を切替え、フェイズ開始回復の有無に反映されることを検証。
/// 決戦2話の核ギミック「水上=ＨＰ回復ON / 陸上=OFF」(`*ターン全味方` の Enable/Disable
/// + Area 分岐) の combat 経路 (毎フェイズ回復) を実検証する。
#[test]
fn disable_enable_toggles_hp_regen_feature() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Regenner", 2, 6);
    // 勝敗即決回避の敵 (遠方)。`place_player_unit` が登録する "Regenner" の
    // ユニットデータを流用する (データ無しだと敵フェイズ AI が panic する)。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Regenner",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.active_features = vec![crate::feature::ActiveFeature::new("ＨＰ回復Lv1", "")];
        u.damage = 50; // 最大HP=100
    }

    // (1) Disable → 基底名一致で is_active=false → フェイズ開始回復しない。
    let dis = crate::data::event::parse(&format!("Disable {uid} ＨＰ回復\n")).unwrap();
    crate::event_runtime::execute(&mut app, &dis).unwrap();
    assert!(
        !app.database().unit_by_uid(&uid).unwrap().active_features[0].is_active,
        "Disable で ＨＰ回復Lv1 が無効化されるはず"
    );
    app.handle_input(Input::EndPhase);
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().damage,
        50,
        "Disable 中は ＨＰ回復 で回復しない (陸上のボス挙動)"
    );

    // (2) Enable → 再有効 → ＨＰ回復Lv1 = 最大HP10% (10) 回復。
    let en = crate::data::event::parse(&format!("Enable {uid} ＨＰ回復\n")).unwrap();
    crate::event_runtime::execute(&mut app, &en).unwrap();
    assert!(
        app.database().unit_by_uid(&uid).unwrap().active_features[0].is_active,
        "Enable で ＨＰ回復Lv1 が再有効化されるはず"
    );
    app.handle_input(Input::EndPhase);
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().damage,
        40,
        "Enable 後は ＨＰ回復Lv1 = 最大HP10% (10) 回復 (水上のボス挙動)"
    );
}

/// ＨＰ消費Lv*/ＥＮ消費Lv* は同率を減少させる (ＨＰ は最低 1)。
#[test]
fn drain_features_reduce_hp_and_en_at_player_phase_start() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Leaker", 2, 6);
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Leaker",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.active_features = vec![
            crate::feature::ActiveFeature::new("ＨＰ消費Lv1", ""),
            crate::feature::ActiveFeature::new("ＥＮ消費Lv3", ""),
        ];
        // HP 満タン / EN 未消費から開始。
    }
    app.handle_input(Input::EndPhase);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(u.damage, 10, "ＨＰ消費Lv1 = 最大HP 10% (10) 減少");
    assert_eq!(u.en_consumed, 15, "ＥＮ消費Lv3 = 最大EN 30% (15) 減少");
}

/// 回復不能 (特殊効果攻撃属性 害) は特殊能力による HP/EN 自然回復を阻害する。
#[test]
fn kaifukufunou_blocks_regen_features() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Regenner", 2, 6);
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Regenner",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.active_features = vec![
            crate::feature::ActiveFeature::new("ＨＰ回復Lv2", ""),
            crate::feature::ActiveFeature::new("ＥＮ回復Lv2", ""),
        ];
        u.damage = 50;
        u.en_consumed = 20;
        u.add_condition(crate::Condition::new("回復不能", -1));
    }
    app.handle_input(Input::EndPhase);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(u.damage, 50, "回復不能で HP 自然回復が阻害される");
    assert_eq!(u.en_consumed, 20, "回復不能で EN 自然回復が阻害される");
}

/// ゾンビ (特殊効果攻撃属性 ゾ) はアビリティ / 精神による能動的な HP/EN 回復を阻害する。
#[test]
fn zombie_blocks_active_recovery() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Z", 2, 6);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.damage = 80;
        u.add_condition(crate::Condition::new("ゾンビ", 3));
    }
    // アビリティ回復は無効。
    app.apply_ability_effects(&uid, &uid, "回復Lv2");
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().damage,
        80,
        "ゾンビはアビリティ回復を受けない"
    );
    // 精神 全快 (spirit_heal_full) も無効。
    app.spirit_heal_full(&uid);
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().damage,
        80,
        "ゾンビは全快回復も受けない"
    );
}

#[test]
fn weapon_special_effect_inflicts_status_and_disables() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    // 痺 属性 + critical 100 → 必ず発動。
    let weapon = WeaponData {
        name: "麻痺銃".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: "痺".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_special_effects(def_idx, def_idx, &weapon, &pilot, &pilot);
    assert_eq!(applied, vec!["麻痺".to_string()]);
    assert!(app.database().unit_instances[def_idx].has_condition("麻痺"));
    assert!(
        app.database().unit_instances[def_idx].attack_disabled(),
        "麻痺 は行動不能 (AI / 攻撃ゲートが効く)"
    );
}

/// 特殊効果無効化=全 を持つ防御側は武器の特殊効果 (状態異常付与) を受けない
/// (防御特性に関する特殊能力.md)。決戦サンプルのザコは全て `特殊効果無効化=全` を持つ。
#[test]
fn tokushu_kouka_mukouka_blocks_status_effects() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    // 防御側に 特殊効果無効化=全 を付与。
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new("特殊効果無効化", "全"));
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    // 痺 属性 + critical 100 → 通常なら必ず麻痺。
    let weapon = WeaponData {
        name: "麻痺銃".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: "痺".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_special_effects(def_idx, def_idx, &weapon, &pilot, &pilot);
    assert!(
        applied.is_empty(),
        "特殊効果無効化=全 なら特殊効果は一切付与されないはず: {applied:?}"
    );
    assert!(
        !app.database().unit_instances[def_idx].has_condition("麻痺"),
        "特殊効果無効化=全 が麻痺付与を防ぐはず"
    );
}

/// 抵抗力Lv* は特殊効果発生確率を 10×Lv% 減少させる (回避系特殊能力.md /
/// C# Unit.cs:12275)。Lv10 なら critical100 (proc 100) でも 0 になり付与されない。
#[test]
fn teikouryoku_reduces_special_effect_proc() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new("抵抗力Lv10", ""));
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    let weapon = WeaponData {
        name: "麻痺銃".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: "痺".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_special_effects(def_idx, def_idx, &weapon, &pilot, &pilot);
    assert!(
        applied.is_empty(),
        "抵抗力Lv10 で proc 100→0 になり麻痺は付与されないはず: {applied:?}"
    );
    assert!(!app.database().unit_instances[def_idx].has_condition("麻痺"));
}

/// 憑 (憑依) 属性は相手を攻撃側の勢力へ**恒久的に**移し、`憑依` condition (永続) を
/// 付与する。復帰用フィールド (charm_revert_party) は使わない。
#[test]
fn weapon_possession_permanently_takes_over_party() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 攻撃側 (firer, Player)
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let firer = first_player_uid(&app);
    let firer_idx = app.database().idx_by_uid(&firer).unwrap();
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    let weapon = possession_weapon("憑");
    let applied = app.apply_weapon_special_effects(def_idx, firer_idx, &weapon, &pilot, &pilot);
    assert_eq!(applied, vec!["憑依".to_string()]);
    assert_eq!(
        app.database().unit_instances[def_idx].party,
        crate::Party::Player,
        "憑依で対象は攻撃側 (味方) 勢力になる"
    );
    assert!(app.database().unit_instances[def_idx].has_condition("憑依"));
    assert!(
        app.database().unit_instances[def_idx]
            .charm_revert_party
            .is_none(),
        "憑依は恒久支配で復帰用フィールドを使わない"
    );
}

/// 憑 (憑依) は BossRank 適用ユニットには無効 (勢力を奪わず condition も付かない)。
#[test]
fn weapon_possession_is_blocked_by_boss_rank() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let firer = first_player_uid(&app);
    let firer_idx = app.database().idx_by_uid(&firer).unwrap();
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    app.database_mut().unit_instances[def_idx].boss_rank = 3;
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    let weapon = possession_weapon("憑");
    let applied = app.apply_weapon_special_effects(def_idx, firer_idx, &weapon, &pilot, &pilot);
    assert!(applied.is_empty(), "ボスには憑依が効かない: {applied:?}");
    assert_eq!(
        app.database().unit_instances[def_idx].party,
        crate::Party::Enemy,
        "勢力は変わらない"
    );
    assert!(!app.database().unit_instances[def_idx].has_condition("憑依"));
}

/// 魅 (魅了) 属性は相手を攻撃側の勢力へ一時的に移し、護衛 ai_mode を設定する。
/// 期限切れ (`begin_phase` の tick) になると元の勢力へ復帰し ai_mode をクリアする。
#[test]
fn weapon_charm_flips_party_then_reverts_on_expiry() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 攻撃側 (firer, Player)
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    // 勝敗判定回避のため、魅了で奪われない敵を 1 体残す (敵全滅=勝利を防ぐ)。
    let guard = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    ));
    let firer = first_player_uid(&app);
    let firer_idx = app.database().idx_by_uid(&firer).unwrap();
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    let weapon = possession_weapon("魅");
    let applied = app.apply_weapon_special_effects(def_idx, firer_idx, &weapon, &pilot, &pilot);
    assert_eq!(applied, vec!["魅了".to_string()]);
    assert_eq!(
        app.database().unit_instances[def_idx].party,
        crate::Party::Player,
        "魅了で対象は攻撃側 (味方) 勢力へ移る"
    );
    assert_eq!(
        app.database().unit_instances[def_idx].charm_revert_party,
        Some(crate::Party::Enemy),
        "元の勢力 (敵) を退避"
    );
    assert!(
        app.database().unit_instances[def_idx]
            .ai_mode
            .starts_with("護衛"),
        "魅了主を護衛する ai_mode"
    );
    assert!(app.database().unit_instances[def_idx].has_condition("魅了"));
    // 魅了中は元の味方 (guard 敵) と敵対し、攻撃側 (firer) とは敵対しない。
    let charmed_party = app.database().unit_instances[def_idx].party;
    let guard_idx = app.database().idx_by_uid(&guard).unwrap();
    assert!(
        app.database().unit_instances[guard_idx]
            .party
            .is_hostile_to(charmed_party),
        "魅了ユニットは元の味方を敵と見なす"
    );
    assert!(
        !app.database().unit_instances[firer_idx]
            .party
            .is_hostile_to(charmed_party),
        "魅了ユニットは魅了主と敵対しない"
    );

    // lifetime=4 → Player フェイズ開始の tick を 4 回で期限切れ → 復帰。
    // 魅了ユニットの勢力は Player なので begin_phase(Player) で tick される。
    for _ in 0..4 {
        app.begin_phase(crate::Phase::Player);
    }
    assert!(
        !app.database().unit_instances[def_idx].has_condition("魅了"),
        "魅了は期限切れで解除"
    );
    assert_eq!(
        app.database().unit_instances[def_idx].party,
        crate::Party::Enemy,
        "期限切れで元の敵勢力へ復帰"
    );
    assert!(
        app.database().unit_instances[def_idx]
            .charm_revert_party
            .is_none(),
        "復帰後は退避フィールドをクリア"
    );
    assert!(
        !app.database().unit_instances[def_idx]
            .ai_mode
            .starts_with("護衛"),
        "復帰後は護衛 ai_mode をクリア"
    );
}

/// 耐性 / 弱点 は特殊効果の発動確率を半減 / 倍にする (武器属性が一致する場合)。
#[test]
fn resistance_and_weakness_scale_proc_rate() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = first_player_uid(&app);
    let def_idx = app.database().idx_by_uid(&uid).unwrap();
    // 属性なし → 変化なし。
    assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 50), 50);
    // 耐性=火 → 火属性武器の発動率半減。
    app.database_mut().unit_instances[def_idx].active_features =
        vec![crate::feature::ActiveFeature::new("耐性", "火")];
    assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 50), 25);
    // 非一致属性 (氷) なら変化なし。
    assert_eq!(app.adjust_proc_for_resistance(def_idx, "氷 痺", 50), 50);
    // 弱点=火 → +10 (SRC: 弱点はクリ率 +10、100 上限)。
    app.database_mut().unit_instances[def_idx].active_features =
        vec![crate::feature::ActiveFeature::new("弱点", "火")];
    assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 40), 50);
    assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 95), 100);

    // 弱/効 属性で付加された一時的弱点 (condition `弱点:火`) も +10 に効く。
    app.database_mut().unit_instances[def_idx].active_features = vec![];
    app.database_mut().unit_instances[def_idx].add_condition(crate::Condition::new("弱点:火", 3));
    assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 40), 50);
    assert_eq!(app.adjust_proc_for_resistance(def_idx, "氷 痺", 40), 40);
}

/// 移動力ＵＰ (+1) / 移動力ＤＯＷＮ (半減、特殊効果攻撃属性 低移) が effective_speed に効く。
#[test]
fn move_status_scales_effective_speed() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Mover", 5, 5); // speed 3
    let uid = first_player_uid(&app);
    let idx = app.database().idx_by_uid(&uid).unwrap();
    let speed = |app: &App| {
        app.database()
            .effective_speed(&app.database().unit_instances[idx])
    };
    assert_eq!(speed(&app), 3, "基本移動力 3");
    // 移動力ＤＯＷＮ → 半減 (1)。
    app.database_mut().unit_instances[idx]
        .add_condition(crate::Condition::new("移動力ＤＯＷＮ", 3));
    assert_eq!(speed(&app), 1, "移動力ＤＯＷＮ で半減");
    app.database_mut().unit_instances[idx].remove_condition("移動力ＤＯＷＮ");
    // 移動力ＵＰ → +1 (4)。
    app.database_mut().unit_instances[idx].add_condition(crate::Condition::new("移動力ＵＰ", 3));
    assert_eq!(speed(&app), 4, "移動力ＵＰ で +1");
}

/// BossRank はステータスを強化し、石化を無効化する。
#[test]
fn boss_rank_boosts_stats_and_blocks_petrify() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Boss", 5, 5); // hp 100, armor 10
    let uid = first_player_uid(&app);
    let idx = app.database().idx_by_uid(&uid).unwrap();
    let hp0 = app
        .database()
        .effective_max_hp(&app.database().unit_instances[idx]);
    assert_eq!(hp0, 100);
    app.database_mut().unit_instances[idx].boss_rank = 2; // HP ×2 / 装甲 +600
    assert_eq!(
        app.database()
            .effective_max_hp(&app.database().unit_instances[idx]),
        200,
        "rank2 で HP ×2"
    );
    assert_eq!(
        app.database()
            .effective_armor(&app.database().unit_instances[idx]),
        610,
        "rank2 で 装甲 +600"
    );
    // 石化 (critical 100 → 必ず proc) でもボスは無効。
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    let w = WeaponData {
        name: "石化光線".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: "石".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_special_effects(idx, idx, &w, &pilot, &pilot);
    assert!(applied.is_empty(), "ボスは石化を無効化");
    assert!(
        !app.database().unit_instances[idx].has_condition("石化"),
        "石化 condition が付かない"
    );
}

#[test]
fn condition_lifetime_decrements_and_expires() {
    use crate::Condition;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&uid)
        .unwrap()
        .add_condition(Condition::new("麻痺", 2));
    // lifetime 2 → プレイヤーフェイズ開始 tick で 2→1 (残る)、再度で 1→0 (消滅)。
    app.begin_phase(crate::Phase::Player);
    assert!(
        app.database()
            .unit_by_uid(&uid)
            .unwrap()
            .has_condition("麻痺"),
        "lifetime 2 は 1 回の tick では消えない"
    );
    app.begin_phase(crate::Phase::Player);
    assert!(
        !app.database()
            .unit_by_uid(&uid)
            .unwrap()
            .has_condition("麻痺"),
        "2 回目の tick で解除される"
    );
}

#[test]
fn poison_status_drops_hp_each_phase_and_one_turn_buffs_clear() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Poisoned", 2, 6);
    // 勝利判定を回避するため敵ユニットも配置
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Poisoned",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    // unit_data の hp は 100。10% = 10 ダメージのはず
    let u = &mut app.database_mut().unit_instances[0];
    u.add_condition(crate::Condition::new("毒", -1)); // 毒は自動削除しない
    u.add_condition(crate::Condition::new("必中", 1));
    u.add_condition(crate::Condition::new("永続バフ", -1));
    assert_eq!(u.damage, 0);
    // Player フェイズ → Enemy → Allied → Neutral → Player (turn 2) の遷移で
    // Player party の begin_phase が再度呼ばれる。end_phase 1 回で 1 ティック。
    app.handle_input(Input::EndPhase);
    let u = &app.database().unit_instances[0];
    assert!(u.damage >= 10, "expected poison tick ≥10, got {}", u.damage);
    assert!(!u.has_condition("必中"), "必中 should have expired");
    assert!(
        u.has_condition("毒"),
        "毒 should still be present (permanent condition)"
    );
    assert!(u.has_condition("永続バフ"));
}

/// 毒ダメージは静的 HP ではなく実効最大 HP (改造/強化パーツ反映) の 10%。
#[test]
fn poison_damage_scales_with_effective_max_hp() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Tough", 2, 6);
    // 改造段階を上げ、実効最大 HP を静的 HP より大きくする。
    app.database_mut().unit_instances[0].upgrade_level = 5;
    app.database_mut().unit_instances[0].add_condition(crate::Condition::new("毒", -1));
    let eff = app
        .database()
        .effective_max_hp(&app.database().unit_instances[0]);
    let static_hp = app.database().unit_by_name("Tough").unwrap().hp;
    assert!(
        eff > static_hp,
        "改造で実効 HP が静的 HP を超える (eff={eff}, static={static_hp})"
    );
    app.begin_phase(crate::Phase::Player);
    assert_eq!(
        app.database().unit_instances[0].damage,
        (eff / 10).max(1),
        "毒ダメージは実効最大 HP の 10% (静的 HP 基準ではない)"
    );
}

/// 死の宣告 (告) は期限切れ (次の自軍フェイズ) で HP を 1 にする。ボスは無効。
#[test]
fn death_sentence_sets_hp_to_one_on_expiry() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Doomed", 2, 6);
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Doomed",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&uid)
        .unwrap()
        .add_condition(crate::Condition::new("死の宣告", 1));
    app.handle_input(Input::EndPhase); // T2 味方フェイズ開始で発動
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().damage,
        99,
        "死の宣告 で HP が 1 になる (最大HP100 → damage 99)"
    );
}

/// 毒ダメージは 毒属性への弱点で倍・耐性で半減する (特殊効果攻撃属性.md)。
#[test]
fn poison_damage_scales_with_resistance_and_weakness() {
    fn poison_tick(features: Vec<crate::feature::ActiveFeature>) -> i64 {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "P", 2, 6);
        app.database_mut().register_unit(crate::UnitInstance::new(
            "P",
            "PILOT",
            crate::Party::Enemy,
            10,
            10,
        ));
        app.set_stage_state(crate::stage::StageState::Battle);
        {
            let u = &mut app.database_mut().unit_instances[0];
            u.add_condition(crate::Condition::new("毒", -1));
            u.active_features = features;
        }
        app.handle_input(Input::EndPhase); // T2 Player フェイズ開始で 1 ティック
        app.database().unit_instances[0].damage
    }
    let normal = poison_tick(vec![]);
    let weak = poison_tick(vec![crate::feature::ActiveFeature::new("弱点", "毒")]);
    let resist = poison_tick(vec![crate::feature::ActiveFeature::new("耐性", "毒")]);
    assert_eq!(weak, normal * 2, "弱点=毒 で毒ダメージ倍");
    assert_eq!(resist, (normal / 2).max(1), "耐性=毒 で毒ダメージ半減");
}
