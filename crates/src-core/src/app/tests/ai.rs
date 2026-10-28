//! AI の思考 (攻撃目標・移動・精神コマンド / アビリティの使用・ChangeMode) のテスト。

use super::*;

/// 恐怖 (特殊効果攻撃属性 恐) の敵 AI は味方から遠ざかり、攻撃しない。
#[test]
fn fear_makes_ai_flee_from_enemies() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 5);
    let coward = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    app.database_mut()
        .unit_by_uid_mut(&coward)
        .unwrap()
        .add_condition(crate::Condition::new("恐怖", 3));
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&coward).unwrap();
    app.ai_act_unit(idx);
    let c = app.database().unit_by_uid(&coward).unwrap();
    let dist_after = (c.x as i32 - 5).abs() + (c.y as i32 - 5).abs();
    assert!(
        dist_after > 1,
        "恐怖の敵は味方 (5,5) から遠ざかる (移動後距離={dist_after})"
    );
    assert!(c.has_acted, "逃走後は行動終了");
}

/// 敵 AI は射程内に対象が居るとき、攻撃前に攻撃補助の精神 (熱血) を使う。
#[test]
fn ai_uses_offensive_spirit_before_attacking() {
    use crate::data::pilot::SpiritCommand;
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 5); // 攻撃対象 (味方)
                                               // Hero unit_data に隣接武器を持たせる (敵もこの機体を使う)。
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|d| d.name == "Hero")
        .unwrap()
        .weapons
        .push(WeaponData {
            name: "パンチ".into(),
            power: 50,
            min_range: 1,
            max_range: 1,
            precision: 50,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        });
    let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    // PILOT に 熱血 を習得させ SP を与える。
    {
        let p = app
            .database_mut()
            .catalog
            .pilots
            .iter_mut()
            .find(|p| p.name == "PILOT")
            .unwrap();
        p.sp = Some(100);
        p.spirit_commands = vec![SpiritCommand {
            name: "熱血".into(),
            cost: Some(20),
            level: 1,
        }];
    }
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&enemy).unwrap();
    app.ai_act_unit(idx);
    assert!(
        app.database()
            .unit_by_uid(&enemy)
            .unwrap()
            .has_condition("熱血"),
        "AI が攻撃前に精神コマンド 熱血 を使う"
    );
}

/// 回復系精神 (信頼) を持つ敵 AI は、半分以下まで負傷した味方を回復する。
/// 精神は行動を消費しないので、その後も通常の行動を続ける。
#[test]
fn ai_uses_support_spirit_on_wounded_ally() {
    use crate::data::pilot::SpiritCommand;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 9, 9); // Hero unit_data + PILOT + 攻撃候補 (敵対)
    let caster = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        5,
        5,
    ));
    let ally = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    // 味方を半分以下 (最大HP100 に対し damage 60) まで負傷させる。
    app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 60;
    {
        let p = app
            .database_mut()
            .catalog
            .pilots
            .iter_mut()
            .find(|p| p.name == "PILOT")
            .unwrap();
        p.sp = Some(50);
        p.spirit_commands = vec![SpiritCommand {
            name: "信頼".into(),
            cost: Some(30),
            level: 1,
        }];
    }
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&caster).unwrap();
    app.ai_act_unit(idx);
    let healed = app.database().unit_by_uid(&ally).unwrap().damage;
    assert!(
        healed < 60,
        "AI が負傷した味方を信頼で回復する (信頼=最大HPの1/3、damage {healed} < 60)"
    );
    assert_eq!(
        app.database().unit_by_uid(&caster).unwrap().sp_consumed,
        30,
        "信頼の SP を消費する"
    );
}

/// ChangeMode「逃亡」の敵 AI も恐怖と同様に味方から遠ざかる。
#[test]
fn change_mode_escape_makes_ai_flee() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 5);
    let runner = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    app.database_mut().unit_by_uid_mut(&runner).unwrap().ai_mode = "逃亡".into();
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&runner).unwrap();
    app.ai_act_unit(idx);
    let c = app.database().unit_by_uid(&runner).unwrap();
    let dist = (c.x as i32 - 5).abs() + (c.y as i32 - 5).abs();
    assert!(dist > 1, "逃亡モードの敵は味方から遠ざかる (距離={dist})");
}

/// 回復アビリティを持つ敵 AI は、射程内の負傷した味方を回復する。
#[test]
fn ai_uses_heal_ability_on_damaged_ally() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Healer", 9, 9); // Healer unit_data + PILOT を用意 (この個体は無関係)
    let healer = app.database_mut().register_unit(crate::UnitInstance::new(
        "Healer",
        "PILOT",
        crate::Party::Enemy,
        5,
        5,
    ));
    let ally = app.database_mut().register_unit(crate::UnitInstance::new(
        "Healer",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 50;
    give_ability(
        &mut app,
        "Healer",
        &healer,
        mk_ability("治療", "回復Lv1", 3, None),
    );
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&healer).unwrap();
    app.ai_act_unit(idx);
    assert_eq!(
        app.database().unit_by_uid(&ally).unwrap().damage,
        0,
        "AI が射程内の負傷した味方を回復する"
    );
    assert!(
        app.database().unit_by_uid(&healer).unwrap().has_acted,
        "回復で行動終了"
    );
}

/// 補給アビリティを持つ敵 AI は、射程内に EN を消費した味方が居れば補給する。
#[test]
fn ai_uses_supply_ability_on_low_en_ally() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Healer", 9, 9); // Healer unit_data + PILOT を用意
    let supplier = app.database_mut().register_unit(crate::UnitInstance::new(
        "Healer",
        "PILOT",
        crate::Party::Enemy,
        5,
        5,
    ));
    let ally = app.database_mut().register_unit(crate::UnitInstance::new(
        "Healer",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    app.database_mut()
        .unit_by_uid_mut(&ally)
        .unwrap()
        .en_consumed = 40;
    give_ability(
        &mut app,
        "Healer",
        &supplier,
        mk_ability("補給装置", "補給Lv1", 3, None),
    );
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&supplier).unwrap();
    app.ai_act_unit(idx);
    assert_eq!(
        app.database().unit_by_uid(&ally).unwrap().en_consumed,
        0,
        "AI が射程内の EN 不足の味方を補給する (補給Lv1 = EN 50 回復)"
    );
    assert!(
        app.database().unit_by_uid(&supplier).unwrap().has_acted,
        "補給で行動終了"
    );
}

/// 敵対象アビリティ (脱=気力減少) を持つ敵 AI は、射程内の最も気力の高い敵へ使う。
#[test]
fn ai_uses_offensive_ability_on_highest_morale_enemy() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Healer", 9, 9); // Healer unit_data + PILOT を用意
                                                 // 対象になる味方 2 機を攻撃側の射程内に配置し、気力差を付ける。
    place_player_unit(&mut app, "Ally", 6, 6); // 低気力
    place_player_unit(&mut app, "Ally", 6, 5); // 高気力
    let low = app.database().unit_instances[1].uid.clone();
    let high = app.database().unit_instances[2].uid.clone();
    app.database_mut().unit_by_uid_mut(&low).unwrap().morale = 100;
    app.database_mut().unit_by_uid_mut(&high).unwrap().morale = 130;
    let attacker = app.database_mut().register_unit(crate::UnitInstance::new(
        "Healer",
        "PILOT",
        crate::Party::Enemy,
        5,
        5,
    ));
    let mut ab = mk_ability("脱力", "状態", 3, None);
    ab.attributes = "脱".into();
    give_ability(&mut app, "Healer", &attacker, ab);
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&attacker).unwrap();
    app.ai_act_unit(idx);
    assert_eq!(
        app.database().unit_by_uid(&high).unwrap().morale,
        120,
        "最も気力の高い敵が 脱 で -10 される"
    );
    assert_eq!(
        app.database().unit_by_uid(&low).unwrap().morale,
        100,
        "気力の低い敵は対象にならない"
    );
    assert!(
        app.database().unit_by_uid(&attacker).unwrap().has_acted,
        "アビリティ発動で行動終了"
    );
}

/// 召喚アビリティを持つ敵 AI は、数的に劣勢 (味方数 ≤ 敵数) のとき増援を召喚する。
#[test]
fn ai_uses_summon_ability_when_outnumbered() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Healer", 9, 9); // 敵対する味方 1 体 (= 召喚側は劣勢)
    let summoner = app.database_mut().register_unit(crate::UnitInstance::new(
        "Healer",
        "PILOT",
        crate::Party::Enemy,
        5,
        5,
    ));
    give_ability(
        &mut app,
        "Healer",
        &summoner,
        mk_ability("援軍要請", "召喚=Healer", 0, Some(1)),
    );
    app.set_stage_state(crate::stage::StageState::Battle);
    let before = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| !u.off_map && u.party == crate::Party::Enemy)
        .count();
    let idx = app.database().idx_by_uid(&summoner).unwrap();
    app.ai_act_unit(idx);
    let after = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| !u.off_map && u.party == crate::Party::Enemy)
        .count();
    assert_eq!(after, before + 1, "劣勢の AI が増援を 1 体召喚する");
    assert!(
        app.database().unit_by_uid(&summoner).unwrap().has_acted,
        "召喚で行動終了"
    );
}

/// マップ兵器を持つ敵 AI は、2 体以上の敵を巻き込める照準があれば発射する。
#[test]
fn ai_uses_map_weapon_on_multiple_enemies() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Bomber", 9, 0); // Bomber unit_data + PILOT
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|d| d.name == "Bomber")
        .unwrap()
        .weapons
        .push(WeaponData {
            name: "全体砲".into(),
            power: 30,
            min_range: 1,
            max_range: 5,
            precision: 50,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: "Ｍ全".into(),
            extras: Vec::new(),
        });
    let bomber = app.database_mut().register_unit(crate::UnitInstance::new(
        "Bomber",
        "PILOT",
        crate::Party::Enemy,
        5,
        5,
    ));
    place_player_unit(&mut app, "Victim1", 6, 5);
    place_player_unit(&mut app, "Victim2", 7, 5);
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&bomber).unwrap();
    app.ai_act_unit(idx);
    let victims: Vec<_> = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| u.unit_data_name.starts_with("Victim"))
        .collect();
    let hit = victims.len() < 2 || victims.iter().all(|u| u.damage > 0);
    assert!(hit, "マップ兵器で複数の敵が被弾する");
    assert!(
        app.database().unit_by_uid(&bomber).unwrap().has_acted,
        "マップ兵器発射で行動終了"
    );
}

/// 識別型でないマップ兵器は味方も巻き込むので、敵 AI は味方が効果範囲に入る照準では
/// 撃たない (`Ｍ全` は自分中心なので、隣に味方が居れば撃たない)。
#[test]
fn ai_map_weapon_avoids_hitting_allies() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Bomber", 9, 0);
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|d| d.name == "Bomber")
        .unwrap()
        .weapons
        .push(WeaponData {
            name: "全体砲".into(),
            power: 30,
            min_range: 1,
            max_range: 5,
            precision: 50,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: "Ｍ全".into(),
            extras: Vec::new(),
        });
    let bomber = app.database_mut().register_unit(crate::UnitInstance::new(
        "Bomber",
        "PILOT",
        crate::Party::Enemy,
        5,
        5,
    ));
    let buddy = app.database_mut().register_unit(crate::UnitInstance::new(
        "Bomber",
        "PILOT",
        crate::Party::Enemy,
        5,
        6,
    ));
    place_player_unit(&mut app, "Victim1", 6, 5);
    place_player_unit(&mut app, "Victim2", 7, 5);
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&bomber).unwrap();
    app.ai_act_unit(idx);
    assert_eq!(app.database().unit_by_uid(&buddy).unwrap().damage, 0);
    assert!(
        app.database()
            .unit_instances
            .iter()
            .filter(|u| u.unit_data_name.starts_with("Victim"))
            .all(|u| u.damage == 0),
        "味方を巻き込む照準で撃っている"
    );
}

/// ChangeMode「護衛 <対象>」の敵 AI は守護対象の近くへ移動する。
#[test]
fn change_mode_escort_moves_toward_protected_unit() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Foe", 11, 11); // 敵対ユニット (candidates 用)
    place_player_unit(&mut app, "Protectee", 1, 1); // Protectee unit_data を用意 (この個体は遠方)
    let protectee = app.database_mut().register_unit(crate::UnitInstance::new(
        "Protectee",
        "PILOT",
        crate::Party::Enemy,
        3,
        3,
    ));
    let guard = app.database_mut().register_unit(crate::UnitInstance::new(
        "Foe",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    ));
    app.database_mut().unit_by_uid_mut(&guard).unwrap().ai_mode = format!("護衛 {protectee}");
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&guard).unwrap();
    app.ai_act_unit(idx);
    let g = app.database().unit_by_uid(&guard).unwrap();
    let dist_after = (g.x as i32 - 3).abs() + (g.y as i32 - 3).abs();
    assert!(
        dist_after < 14,
        "護衛役は守護対象 (3,3) へ近づく (移動後距離={dist_after})"
    );
}

#[test]
fn ai_changemode_coord_lures_unit_toward_coordinate() {
    // ChangeMode <unit> <x> <y> (座標モード): 近い敵ではなく指定座標へ向かう。
    // 決戦！宇宙怪獣2話 の `ChangeMode 宇宙怪獣ギルガス 17 1` (ボスを釣る) 相当。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app); // 24x16
    place_player_unit(&mut app, "FoeData", 23, 15); // FoeData の UnitData/Pilot 登録用
    add_weapon(&mut app, "FoeData", 100, 1);
    place_player_unit(&mut app, "Hero", 0, 14); // 敵から見て近い標的 (下方向, dist14)
    let foe = spawn_enemy(&mut app, "FoeData", 0, 0); // AI 敵ユニット
    app.set_stage_state(crate::stage::StageState::Battle);
    let fidx = app.database().idx_by_uid(&foe).unwrap();
    app.database_mut().unit_instances[fidx].ai_mode = "21 1".to_string(); // 右方向へ誘導 (SRC 座標 = 内部 (20, 0))

    app.ai_act_unit(fidx);

    let u = app.database().unit_by_uid(&foe).unwrap();
    assert!(u.x > 0, "座標誘導で右へ動いていない (x={}, y={})", u.x, u.y);
    assert!(
        u.x > u.y,
        "近い Hero(下) へ動いてしまっている (x={}, y={})",
        u.x,
        u.y
    );
}

#[test]
fn ai_changemode_target_focuses_named_unit() {
    // ChangeMode <unit> <対象名> (ユニット狙い): 近い敵ではなく指定ユニットを
    // 優先ターゲットにする。決戦！宇宙怪獣2話 `ChangeMode ＮＰＣ 宇宙怪獣ギルガス`
    // (NPC をボスへ集中) 相当。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "NearData", 0, 3); // 近い標的 (下, dist3)
    place_player_unit(&mut app, "FarData", 20, 0); // 指定する遠い標的 (右, dist20)
    add_weapon(&mut app, "FarData", 100, 1);
    let foe = spawn_enemy(&mut app, "FarData", 0, 0); // AI 敵 (FarData 流用)
                                                      // 武器を持たせる (spawn_enemy は UnitData を共有するので FarData に付与済み)
    app.set_stage_state(crate::stage::StageState::Battle);
    let fidx = app.database().idx_by_uid(&foe).unwrap();
    app.database_mut().unit_instances[fidx].ai_mode = "FarData".to_string(); // FarData を狙う

    app.ai_act_unit(fidx);

    let u = app.database().unit_by_uid(&foe).unwrap();
    // 指定した FarData(右) 方向へ動く。近い NearData(下) ではない。
    assert!(
        u.x > u.y,
        "指定ユニット(右)でなく近い標的(下)へ動いている (x={}, y={})",
        u.x,
        u.y
    );
}

#[test]
fn animated_ai_runner_steps_units_and_returns_to_player() {
    // animate_ai=true では EndPhase で即完了せず、tick が敵/中立/ＮＰＣ を
    // 1 体ずつ進めて Player ターン+1 に戻る。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Data", 9, 9);
    spawn_party(&mut app, "Data", crate::Party::Enemy, 2, 2);
    spawn_party(&mut app, "Data", crate::Party::Enemy, 14, 14);
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_animate_ai(true);

    // EndPhase → ランナー起動 (敵フェイズ)。即座には Player に戻らない。
    app.handle_input(Input::EndPhase);
    assert!(app.ai_running(), "逐次ランナーが起動しているはず");
    assert_eq!(app.turn().phase, crate::Phase::Enemy);
    assert_eq!(app.turn().number, 1);

    // 最初の 1 ステップ未満の tick ではまだ誰も動かない (間の演出)。
    app.tick(0.1);
    assert!(app.ai_running());
    assert_eq!(app.turn().phase, crate::Phase::Enemy);

    // tick を十分回すと敵→中立→ＮＰＣ を消化し Player ターン2 に戻る。
    for _ in 0..300 {
        app.tick(0.1);
        if !app.ai_running() {
            break;
        }
    }
    assert!(!app.ai_running(), "ランナーが完了していない");
    assert_eq!(app.turn().phase, crate::Phase::Player);
    assert_eq!(app.turn().number, 2);
}

#[test]
fn ai_move_sets_move_anim_and_runner_waits_for_it() {
    // 逐次 AI 移動でスライド演出 (move_anim) が立ち、再生中はランナーが次に進まない。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 9, 6); // 遠い標的
    place_player_unit(&mut app, "Foe", 23, 15); // Foe の UnitData 用 (隅)
    add_weapon(&mut app, "Foe", 60, 1);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 2, 6); // 攻撃側 (遠方)
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_animate_ai(true);
    app.handle_input(Input::EndPhase);

    // 最初のステップ間隔を消化して 1 体目を行動させる。
    app.tick(0.5);
    assert!(app.ai_running(), "AI フェイズが進行していない");
    let anim = app.move_anim().expect("移動スライド演出が立っていない");
    assert!(anim.path.len() >= 2, "経路が短すぎる: {:?}", anim.path);
    // 論理位置は即時に移動先へ (始点から動いている)。
    let moved_x = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Enemy)
        .map(|u| u.x)
        .unwrap();
    assert!(moved_x > 2, "敵の論理位置が移動していない (x={moved_x})");

    // 演出再生中はランナーが待機 (move_anim が残る短い tick では別ユニットへ進まない)。
    app.tick(0.05);
    assert!(
        app.move_anim().is_some(),
        "短い tick で演出が消えている (待機していない)"
    );
    // 総時間を超えて進めると演出は破棄される。
    for _ in 0..30 {
        app.tick(0.1);
        if app.move_anim().is_none() {
            break;
        }
    }
    assert!(app.move_anim().is_none(), "演出が完了後も残っている");
}

#[test]
fn ai_uses_dijkstra_to_close_distance() {
    // 敵ユニットがマップ反対側 (10, 6) にいる。プレイヤーは Player phase 終了で
    // Enemy AI が走る。AI はプレイヤーに向かって速度分マスを進む。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    // 敵ユニットを (10, 6) に配置
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        10,
        6,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    // EndPhase で Enemy phase が走る → AI が Hero (player) に近付く
    app.handle_input(Input::EndPhase);
    // AI が動いた結果、敵は (2,6) のプレイヤーに近付いている
    let enemy = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Enemy)
        .expect("enemy alive");
    let manhattan_after = (enemy.x as i32 - 2).unsigned_abs() + (enemy.y as i32 - 6).unsigned_abs();
    assert!(
        manhattan_after < 8,
        "AI failed to close: dist={manhattan_after}"
    );
}
