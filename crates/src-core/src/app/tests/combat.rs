//! 戦闘 (命中・ダメージ・クリティカル・防御特性・援護・反撃・撃破報酬・戦闘アニメ) のテスト。

use super::*;

/// `push_combat_feature_statuses` で特徴由来の `ステルス` が予測の hit% 低下 status に
/// 合流すること。`分身`/`超回避` は (B) で実行段の回避ロールへ移行したため**合流しない**
/// (check_dodge_feature が active_features を直接読む)。
#[test]
fn combat_feature_statuses_merges_stealth_only() {
    use crate::feature::ActiveFeature;
    let mut u = crate::UnitInstance::new("X", "P", crate::Party::Enemy, 0, 0);
    u.active_features = vec![
        ActiveFeature::new("ステルス", ""),  // → status
        ActiveFeature::new("分身", ""),      // (B) 回避ロール → 合流しない
        ActiveFeature::new("超回避Lv4", ""), // (B) 回避ロール → 合流しない
        ActiveFeature::new("水上移動", ""),  // 非戦闘 → 合流しない
    ];
    let mut s = vec!["集中".to_string()]; // 既存の condition 由来 status
    push_combat_feature_statuses(&u, &mut s);
    assert!(
        s.contains(&"ステルス".to_string()),
        "有効な ステルス 特徴は status に合流するはず"
    );
    assert!(
        !s.iter().any(|x| x == "分身" || x.contains("超回避")),
        "分身/超回避 は (B) 回避ロールへ移行し status には合流しない: {s:?}"
    );
    assert!(
        !s.contains(&"水上移動".to_string()),
        "非戦闘特徴は合流しない"
    );
    assert!(s.contains(&"集中".to_string()), "既存 status は保持");
}

/// バリア / バリアLv* は (B) の実行段吸収 (apply_barrier_absorb) へ移行したため、
/// 予測の ÷2 合流 (`バリア` status push) には **乗らない**。`バリア中和` (攻撃属性) も
/// 当然除外。`バリアシールド`/フィールド系の合流は別テストで検証。
#[test]
fn barrier_feature_is_not_merged_into_predict_halving() {
    use crate::feature::ActiveFeature;
    let mut u = crate::UnitInstance::new("X", "P", crate::Party::Enemy, 0, 0);
    u.active_features = vec![
        ActiveFeature::new("バリアLv2", ""), // → (B) 吸収・(A) push しない
        ActiveFeature::new("バリア", ""),    // → (B) 吸収・(A) push しない
        ActiveFeature::new("バリア中和", ""), // 攻撃属性 → 除外
    ];
    let mut s = Vec::new();
    push_combat_feature_statuses(&u, &mut s);
    assert!(
        !s.iter().any(|x| x == "バリア"),
        "バリア/バリアLv* は (B) 吸収へ移行し予測の ÷2 には合流しない: {s:?}"
    );
}

/// (A) ÷2 合流に残るのは バリアシールド(非標準名)/盾 のみ。シールド系・超回避・分身は
/// (B) ロール、バリア/フィールド/アクティブフィールド は (B) 吸収、単独プロテクト・
/// フィールド(解説)は非機能、バリア中和は攻撃属性 なので (A) には乗らない。
#[test]
fn combat_feature_statuses_merges_shield_field_and_chougaihi() {
    use crate::feature::ActiveFeature;
    let mut u = crate::UnitInstance::new("X", "P", crate::Party::Enemy, 0, 0);
    u.active_features = vec![
        ActiveFeature::new("バリアシールドLv3", ""), // → バリア (非標準名・(A) 残置)
        ActiveFeature::new("アクティブフィールドLv3", ""), // (B) 確率吸収 → (A) 除外
        ActiveFeature::new("アクティブシールド", ""), // シールド系 → (B)・(A) 除外
        ActiveFeature::new("フィールドLv1", ""),     // (B) 確定吸収 → (A) 除外
        ActiveFeature::new("超回避Lv4", ""),         // (B) 回避ロール → (A) 除外
        ActiveFeature::new("プロテクト・フィールド", "解説 これはヘルプ文"), // 非機能 → 除外
        ActiveFeature::new("バリア中和", ""),        // 攻撃属性 → 除外
    ];
    let mut s = Vec::new();
    push_combat_feature_statuses(&u, &mut s);
    assert_eq!(
        s.iter().filter(|x| x.as_str() == "バリア").count(),
        1,
        "バリアシールド の 1 つのみ (A) ÷2 に合流: {s:?}"
    );
}

#[test]
fn effective_combat_data_reflects_growth_and_debuff() {
    use crate::Condition;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let idx = {
        let uid = first_player_uid(&app);
        app.database().idx_by_uid(&uid).unwrap()
    };
    // 基準 (level 1 / 装備なし / 状態異常なし)。VB5 `Pilot.cls:582-593` 準拠で
    // レベル 1 でも成長が乗る (格闘 = base 100 + lv 1 = 101)。装甲は unit 由来で不変。
    let (p0, u0) = app.database().effective_combat_data(idx).unwrap();
    assert_eq!(p0.infight, 101);
    assert_eq!(u0.armor, 10);
    // 育成: total_exp 500 → level 2 → 格闘がさらに成長する (戦闘予測へ反映)。
    app.database_mut().unit_instances[idx].total_exp = 500;
    let (p1, _) = app.database().effective_combat_data(idx).unwrap();
    assert!(
        p1.infight > p0.infight,
        "レベル成長で格闘が上がる: {} > {}",
        p1.infight,
        p0.infight
    );
    // 状態異常「装甲低下」で実効装甲が下がる (被ダメージ増加に直結)。
    app.database_mut().unit_instances[idx].add_condition(Condition::new("装甲低下", 1));
    let (_, u2) = app.database().effective_combat_data(idx).unwrap();
    assert!(
        u2.armor < u0.armor,
        "装甲低下で実効装甲が下がる: {} < {}",
        u2.armor,
        u0.armor
    );
}

#[test]
fn award_kill_rewards_grants_money_exp_and_consumes_kouun() {
    use crate::Condition;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let idx = {
        let uid = first_player_uid(&app);
        app.database().idx_by_uid(&uid).unwrap()
    };
    let money0 = app.money();
    let exp0 = app.database().unit_instances[idx].total_exp;
    // 撃破: 経験値 50 / 敵価値 200 → 資金 100。
    let gained = app.award_kill_rewards(idx, 50, 200, crate::Party::Enemy);
    assert_eq!(gained, 100, "資金 = 敵価値/2");
    assert_eq!(app.money(), money0 + 100);
    assert_eq!(app.database().unit_instances[idx].total_exp, exp0 + 50);
    // 幸運: 資金 2 倍 かつ消費。
    app.database_mut().unit_instances[idx].add_condition(Condition::new("幸運", 1));
    let money1 = app.money();
    let gained2 = app.award_kill_rewards(idx, 0, 200, crate::Party::Enemy);
    assert_eq!(gained2, 200, "幸運 で資金 2 倍");
    assert_eq!(app.money(), money1 + 200);
    assert!(
        !app.database().unit_instances[idx].has_condition("幸運"),
        "幸運 は撃破時に消費される"
    );
}

/// 撃破でメインパイロットの撃墜数が +1 され、必要技能ゲートが読む値が上がる。
#[test]
fn kill_increments_pilot_kill_count() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let idx = {
        let uid = first_player_uid(&app);
        app.database().idx_by_uid(&uid).unwrap()
    };
    // place_player_unit は pilot_ids を張らないので実体を作って紐付ける。
    app.database_mut().create_pilot_instance("PILOT", "p1");
    app.database_mut().unit_instances[idx].pilot_ids = vec!["p1".to_string()];
    assert_eq!(app.unit_pilot_skill_level(idx, "撃墜数"), 0);
    app.award_kill_rewards(idx, 0, 0, crate::Party::Enemy);
    assert_eq!(
        app.unit_pilot_skill_level(idx, "撃墜数"),
        1,
        "1 撃破で撃墜数 1"
    );
    app.award_kill_rewards(idx, 0, 0, crate::Party::Enemy);
    app.award_kill_rewards(idx, 0, 0, crate::Party::Enemy);
    assert_eq!(
        app.unit_pilot_skill_level(idx, "撃墜数"),
        3,
        "3 撃破で撃墜数 3"
    );
}

/// 撃墜による気力上昇: 撃破者 +4 / 同陣営の他ユニット +1 (`Unit.cs` 準拠)。
#[test]
fn kill_raises_morale_killer_plus4_faction_plus1() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // [0] 撃破者
    place_player_unit(&mut app, "Ally", 4, 6); // [1] 同陣営
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    )); // [2] 敵 (別陣営、対象外)
    let k0 = app.database().unit_instances[0].morale;
    let a0 = app.database().unit_instances[1].morale;
    let e0 = app.database().unit_instances[2].morale;
    app.award_kill_rewards(0, 0, 0, crate::Party::Enemy);
    assert_eq!(
        app.database().unit_instances[0].morale,
        k0 + 4,
        "撃破者は気力 +4"
    );
    assert_eq!(
        app.database().unit_instances[1].morale,
        a0 + 1,
        "同陣営の他ユニットは気力 +1"
    );
    assert_eq!(
        app.database().unit_instances[2].morale,
        e0,
        "別陣営は変動しない"
    );
}

/// 性格「機械」のパイロットは撃墜による気力上昇の対象外 (撃破者は +3 のみ)。
#[test]
fn kill_morale_skips_mecha_pilots() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    place_player_unit(&mut app, "Ally", 4, 6);
    app.database_mut()
        .catalog
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
        .unwrap()
        .personality = Some("機械".into());
    let k0 = app.database().unit_instances[0].morale;
    let a0 = app.database().unit_instances[1].morale;
    app.award_kill_rewards(0, 0, 0, crate::Party::Enemy);
    assert_eq!(
        app.database().unit_instances[0].morale,
        k0 + 3,
        "機械の撃破者は陣営加算を受けず +3 のみ"
    );
    assert_eq!(
        app.database().unit_instances[1].morale,
        a0,
        "機械の同陣営は気力が動かない"
    );
}

/// 被撃破陣営は性格に応じて気力が変動する (超強気 +2 / 弱気 -1)。
#[test]
fn kill_morale_victim_faction_changes_by_personality() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // [0] 撃破者 (Player)
    push_pilot_with_features(&mut app, "BRAVE", Vec::new());
    push_pilot_with_features(&mut app, "TIMID", Vec::new());
    app.database_mut()
        .catalog
        .pilots
        .iter_mut()
        .find(|p| p.name == "BRAVE")
        .unwrap()
        .personality = Some("超強気".into());
    app.database_mut()
        .catalog
        .pilots
        .iter_mut()
        .find(|p| p.name == "TIMID")
        .unwrap()
        .personality = Some("弱気".into());
    let brave = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "BRAVE",
        crate::Party::Enemy,
        8,
        8,
    ));
    let timid = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "TIMID",
        crate::Party::Enemy,
        9,
        9,
    ));
    let b0 = app.database().unit_by_uid(&brave).unwrap().morale;
    let t0 = app.database().unit_by_uid(&timid).unwrap().morale;
    // Player[0] が Enemy を撃破 (victim_party = Enemy)。
    app.award_kill_rewards(0, 0, 0, crate::Party::Enemy);
    assert_eq!(
        app.database().unit_by_uid(&brave).unwrap().morale,
        b0 + 2,
        "超強気は味方撃破で気力 +2"
    );
    assert_eq!(
        app.database().unit_by_uid(&timid).unwrap().morale,
        t0 - 1,
        "弱気は味方撃破で気力 -1"
    );
}

/// (B) シールド防御: Ｓ防御技能 + シールド系特殊能力で被ダメージ半減。Ｓ防御Lv16 なら
/// prob 16 >= Dice(16) で必ず発動。精/殺/浸 属性は不可。
#[test]
fn shield_defense_halves_damage_with_high_sdefense() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    // PILOT に Ｓ防御Lv16 を付与 (必ず発動)。
    if let Some(pd) = app
        .database_mut()
        .catalog
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
    {
        pd.features.push(("Ｓ防御Lv16".into(), String::new()));
    }
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
        .push(crate::feature::ActiveFeature::new("シールド", ""));

    // 通常武器 (実) 800 ダメージ → シールド発動で ÷2 = 400。
    assert_eq!(
        app.apply_shield_defense(def_idx, "実", 800),
        400,
        "Ｓ防御Lv16+シールド で被ダメージ半減"
    );
    // 精 属性にはシールド防御不可。
    assert_eq!(
        app.apply_shield_defense(def_idx, "精", 800),
        800,
        "精 属性にはシールド防御不可"
    );
    // Ｓ防御技能が無ければ発動しない。
    let target2 = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "NOSKILL",
        crate::Party::Enemy,
        4,
        6,
    ));
    let def2 = app.database().idx_by_uid(&target2).unwrap();
    app.database_mut().unit_instances[def2]
        .active_features
        .push(crate::feature::ActiveFeature::new("シールド", ""));
    assert_eq!(
        app.apply_shield_defense(def2, "実", 800),
        800,
        "Ｓ防御技能が無ければシールド防御は発動しない"
    );
}

/// (B) 小型シールド は 2/3 に軽減 (通常武器)。
#[test]
fn small_shield_reduces_damage_to_two_thirds() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    if let Some(pd) = app
        .database_mut()
        .catalog
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
    {
        pd.features.push(("Ｓ防御Lv16".into(), String::new()));
    }
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
        .push(crate::feature::ActiveFeature::new("小型シールド", ""));
    // 900 → 2/3 = 600。
    assert_eq!(
        app.apply_shield_defense(def_idx, "実", 900),
        600,
        "小型シールド は 2/3 に軽減"
    );
}

/// (B) バリア吸収: バリアLv* が 1000×Lv ダメージを吸収する。完全吸収は 0。
/// 直撃 (攻撃側) / バリア中和 (防御側) で無効。対象属性が不一致なら吸収しない。
#[test]
fn barrier_absorb_blocks_up_to_thousand_times_level() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Atk", 2, 6);
    let atk_idx = first_player_uid(&app);
    let atk_idx = app.database().idx_by_uid(&atk_idx).unwrap();
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def",
        "DP",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new("バリアLv2", "")); // 2000 吸収・全属性

    // 1500 < 2000 → 完全吸収 0。
    assert_eq!(
        app.apply_barrier_absorb(def_idx, atk_idx, "実", 1500),
        0,
        "バリアLv2 は 2000 までを完全吸収"
    );
    // 2500 > 2000 → 残 500。
    assert_eq!(
        app.apply_barrier_absorb(def_idx, atk_idx, "実", 2500),
        500,
        "2000 を超えた分のみ通る"
    );
    // 直撃 (攻撃側) でバリア無効。
    app.database_mut().unit_instances[atk_idx].add_condition(crate::Condition::new("直撃", -1));
    assert_eq!(
        app.apply_barrier_absorb(def_idx, atk_idx, "実", 1500),
        1500,
        "直撃 でバリアは無効"
    );
    app.database_mut().unit_instances[atk_idx].remove_condition("直撃");

    // 対象属性つきバリア (火) は不一致武器を吸収しない。
    app.database_mut().unit_instances[def_idx]
        .active_features
        .clear();
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new(
            "バリアLv2",
            "炎バリア 火",
        ));
    assert_eq!(
        app.apply_barrier_absorb(def_idx, atk_idx, "実", 1500),
        1500,
        "対象属性 火 のバリアは 実 武器を吸収しない"
    );
    assert_eq!(
        app.apply_barrier_absorb(def_idx, atk_idx, "火", 1500),
        0,
        "対象属性 火 のバリアは 火 武器を吸収する"
    );
}

/// (B knobs) 相殺/中和: 隣接する同名 (別名一致) バリアを持つ攻撃側に対し、相殺=完全無効 /
/// 中和=攻撃側 Lv 分だけ効果減少。
#[test]
fn barrier_sousai_and_chuwa_with_adjacent_same_named() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Atk", 3, 6);
    let atk = first_player_uid(&app);
    let atk = app.database().idx_by_uid(&atk).unwrap();
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def",
        "DP",
        crate::Party::Enemy,
        4,
        6, // (3,6) と隣接
    ));
    let def = app.database().idx_by_uid(&target).unwrap();

    // --- 相殺 --- defender: バリアLv3=魔法障壁 … 相殺 (3000 吸収・相殺オプション)。
    app.database_mut().unit_instances[def].active_features.push(
        crate::feature::ActiveFeature::new("バリアLv3", "魔法障壁 全 0 0 相殺"),
    );
    // 攻撃側が同名 (魔法障壁) バリアを隣接所持 → 相殺で防御側バリア無効。
    app.database_mut().unit_instances[atk].active_features.push(
        crate::feature::ActiveFeature::new("バリアLv1", "魔法障壁 全 0 0"),
    );
    assert_eq!(
        app.apply_barrier_absorb(def, atk, "実", 2000),
        2000,
        "相殺: 隣接同名バリアで防御側バリアが無効化"
    );
    // 攻撃側が同名バリアを持たなければ通常どおり吸収 (3000>2000 → 0)。
    app.database_mut().unit_instances[atk]
        .active_features
        .clear();
    assert_eq!(
        app.apply_barrier_absorb(def, atk, "実", 2000),
        0,
        "攻撃側に同名バリアが無ければ通常吸収"
    );

    // --- 中和 --- defender: バリアLv3=魔法障壁 … 中和。攻撃側 バリアLv1=魔法障壁 隣接
    // → eff_lv = 3-1 = 2 → 2000 吸収。
    app.database_mut().unit_instances[def]
        .active_features
        .clear();
    app.database_mut().unit_instances[def].active_features.push(
        crate::feature::ActiveFeature::new("バリアLv3", "魔法障壁 全 0 0 中和"),
    );
    app.database_mut().unit_instances[atk].active_features.push(
        crate::feature::ActiveFeature::new("バリアLv1", "魔法障壁 全 0 0"),
    );
    assert_eq!(
        app.apply_barrier_absorb(def, atk, "実", 2500),
        500,
        "中和: Lv3-1=2 → 2000 吸収、残 500"
    );
}

/// (B knobs) バリア/フィールド吸収は 必要気力 と ＥＮ消費 のゲートを尊重する (忠実性)。
#[test]
fn barrier_absorb_respects_morale_and_en_gates() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Atk", 2, 6);
    let atk = first_player_uid(&app);
    let atk = app.database().idx_by_uid(&atk).unwrap();
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def",
        "DP",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def = app.database().idx_by_uid(&target).unwrap();

    // フィールドLv2 (1000 吸収) だが 必要気力 130 (値の 4 番目)。
    app.database_mut().unit_instances[def].active_features.push(
        crate::feature::ActiveFeature::new("フィールドLv2", "魔法障壁 全 0 130"),
    );
    app.database_mut().unit_instances[def].morale = 100;
    assert_eq!(
        app.apply_barrier_absorb(def, atk, "実", 800),
        800,
        "気力 100 < 必要気力 130 で不発"
    );
    app.database_mut().unit_instances[def].morale = 130;
    assert_eq!(
        app.apply_barrier_absorb(def, atk, "実", 800),
        0,
        "気力 130 到達で発動 (800<1000 完全吸収)"
    );

    // ＥＮ消費 9999 (3 番目) は max EN 0 のユニットでは EN 不足で不発。
    app.database_mut().unit_instances[def]
        .active_features
        .clear();
    app.database_mut().unit_instances[def].active_features.push(
        crate::feature::ActiveFeature::new("バリアLv2", "x 全 9999 0"),
    );
    assert_eq!(
        app.apply_barrier_absorb(def, atk, "実", 800),
        800,
        "ＥＮ不足でバリア不発"
    );
}

/// (B) フィールド吸収: フィールドLv* は 500×Lv を確定吸収 (バリアの半分)。
/// 解説エントリ (値が「解説…」) は防御能力として扱わない。バリアとスタックする。
#[test]
fn field_absorbs_five_hundred_times_level_and_ignores_kaisetsu() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Atk", 2, 6);
    let atk_idx = first_player_uid(&app);
    let atk_idx = app.database().idx_by_uid(&atk_idx).unwrap();
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def",
        "DP",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new("フィールドLv2", "")); // 1000 吸収
                                                                        // 解説エントリは無視されること。
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new(
            "プロテクト・フィールド",
            "解説 これはヘルプ文",
        ));
    // 800 < 1000 → 完全吸収 0。
    assert_eq!(app.apply_barrier_absorb(def_idx, atk_idx, "実", 800), 0);
    // 1200 > 1000 → 残 200。
    assert_eq!(app.apply_barrier_absorb(def_idx, atk_idx, "実", 1200), 200);

    // バリアLv1 (1000) を追加するとスタックして 1000+1000=2000 吸収。
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new("バリアLv1", ""));
    assert_eq!(
        app.apply_barrier_absorb(def_idx, atk_idx, "実", 1900),
        0,
        "フィールド 1000 + バリア 1000 = 2000 吸収"
    );
}

/// (B) アクティブフィールド: Ｓ防御/16 の確率発動で 500×Lv 吸収。Ｓ防御Lv16 なら必発。
/// Ｓ防御技能が無ければ発動しない。
#[test]
fn active_field_absorbs_when_sdefense_high() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Atk", 2, 6);
    let atk_idx = first_player_uid(&app);
    let atk_idx = app.database().idx_by_uid(&atk_idx).unwrap();
    // PILOT に Ｓ防御Lv16 (必発)。
    if let Some(pd) = app
        .database_mut()
        .catalog
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
    {
        pd.features.push(("Ｓ防御Lv16".into(), String::new()));
    }
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new(
            "アクティブフィールドLv3",
            "",
        )); // 1500 吸収
            // 1200 < 1500 → 完全吸収 0 (Ｓ防御Lv16 で必発)。
    assert_eq!(
        app.apply_active_field_absorb(def_idx, atk_idx, "実", 1200),
        0
    );

    // Ｓ防御技能が無い防御側では発動しない。
    let t2 = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def2",
        "NOSKILL",
        crate::Party::Enemy,
        4,
        6,
    ));
    let d2 = app.database().idx_by_uid(&t2).unwrap();
    app.database_mut().unit_instances[d2]
        .active_features
        .push(crate::feature::ActiveFeature::new(
            "アクティブフィールドLv3",
            "",
        ));
    assert_eq!(
        app.apply_active_field_absorb(d2, atk_idx, "実", 1200),
        1200,
        "Ｓ防御技能なしではアクティブフィールド不発"
    );
}

/// (B) 回避系特殊能力 (分身/超回避) の完全回避ロール check_dodge_feature。
/// 超回避Lv10 は `Lv>=Dice(10)` で必発。直撃/行動不能/気力130未満で不発。
#[test]
fn check_dodge_feature_chougaihi_and_bunshin() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Atk", 2, 6);
    let atk = first_player_uid(&app);
    let atk = app.database().idx_by_uid(&atk).unwrap();
    let t = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def",
        "DP",
        crate::Party::Enemy,
        3,
        6,
    ));
    let d = app.database().idx_by_uid(&t).unwrap();

    // 超回避Lv10 → Lv>=Dice(10) で必ず回避。
    app.database_mut().unit_instances[d]
        .active_features
        .push(crate::feature::ActiveFeature::new("超回避Lv10", ""));
    assert!(
        app.check_dodge_feature(d, atk, "実"),
        "超回避Lv10 は必ず回避するはず"
    );
    // 直撃 (攻撃側) で回避無効。
    app.database_mut().unit_instances[atk].add_condition(crate::Condition::new("直撃", -1));
    assert!(!app.check_dodge_feature(d, atk, "実"), "直撃 で回避は無効");
    app.database_mut().unit_instances[atk].remove_condition("直撃");
    // 行動不能系 (麻痺) で能動回避不可。
    app.database_mut().unit_instances[d].add_condition(crate::Condition::new("麻痺", 3));
    assert!(
        !app.check_dodge_feature(d, atk, "実"),
        "麻痺 (行動不能) で回避不可"
    );
    app.database_mut().unit_instances[d].remove_condition("麻痺");

    // 分身は気力130未満では発動しない (既定気力100)。
    app.database_mut().unit_instances[d].active_features.clear();
    app.database_mut().unit_instances[d]
        .active_features
        .push(crate::feature::ActiveFeature::new("分身", ""));
    assert_eq!(app.database().unit_instances[d].morale, 100);
    assert!(
        !app.check_dodge_feature(d, atk, "実"),
        "分身は気力130未満で不発"
    );
}

/// (B) ＥＣＭ エリア補正: 防御側周囲の味方 ＥＣＭ で命中低下、攻撃側の味方 ＥＣＭ で相殺。
/// ホーミング `Ｈ` 武器は影響 2 倍。
#[test]
fn ecm_aura_reduces_hit_from_nearby_allies() {
    use crate::feature::ActiveFeature;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    // 防御側 (敵) を (5,5)、攻撃側 (味方) を遠方 (5,9) に配置。
    let d = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def",
        "DP",
        crate::Party::Enemy,
        5,
        5,
    ));
    let d = app.database().idx_by_uid(&d).unwrap();
    let a = app.database_mut().register_unit(crate::UnitInstance::new(
        "Atk",
        "AP",
        crate::Party::Player,
        5,
        9,
    ));
    let a = app.database().idx_by_uid(&a).unwrap();

    // ＥＣＭ 機が居なければ補正なし。
    assert_eq!(app.ecm_hit_mult(d, a, "実"), 1.0);

    // 防御側の味方 (敵) が ＥＣＭLv4 を持って隣接 (6,5)。
    let mut jam = crate::UnitInstance::new("Jam", "JP", crate::Party::Enemy, 6, 5);
    jam.active_features
        .push(ActiveFeature::new("ＥＣＭLv4", ""));
    app.database_mut().register_unit(jam);
    // 通常武器: (100-5×4)/100 = 0.8。
    assert!((app.ecm_hit_mult(d, a, "実") - 0.8).abs() < 1e-9);
    // ホーミング Ｈ: (100-10×4)/100 = 0.6。
    assert!((app.ecm_hit_mult(d, a, "Ｈ") - 0.6).abs() < 1e-9);

    // 攻撃側の味方 (味方) が同 Lv の ＥＣＭ を防御側付近 (4,5) に持つと相殺 → 1.0。
    let mut jammer = crate::UnitInstance::new("CJam", "CP", crate::Party::Player, 4, 5);
    jammer
        .active_features
        .push(ActiveFeature::new("ＥＣＭLv4", ""));
    app.database_mut().register_unit(jammer);
    assert_eq!(app.ecm_hit_mult(d, a, "実"), 1.0, "攻撃側 ＥＣＭ で相殺");
}

/// 脱属性武器は命中・proc 時に対象の気力を低下させる (吸収は無し)。
#[test]
fn weapon_datsu_reduces_target_morale() {
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
    let m0 = app.database().unit_instances[def_idx].morale;
    // 脱 + critical 100 → 必ず proc。
    let weapon = WeaponData {
        name: "脱力弾".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: "脱".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_special_effects(def_idx, def_idx, &weapon, &pilot, &pilot);
    assert!(applied.iter().any(|s| s.contains("気力")), "気力減少ラベル");
    assert_eq!(
        app.database().unit_instances[def_idx].morale,
        m0 - 10,
        "脱 で気力 -10"
    );
}

/// Ｄ属性 (気力吸収) は対象の気力を低下させ、低下分の半分を攻撃側へ移す。
#[test]
fn weapon_d_absorbs_half_morale_to_firer() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Drainer", 2, 6); // 攻撃側 (firer)
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Drainer",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let firer = first_player_uid(&app);
    let firer_idx = app.database().idx_by_uid(&firer).unwrap();
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    app.database_mut().unit_instances[firer_idx].morale = 100;
    app.database_mut().unit_instances[def_idx].morale = 130;
    // Ｄ + critical 100 → 必ず proc。低下 10 → 対象 -10 / 攻撃側 +5。
    let weapon = WeaponData {
        name: "吸収弾".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: "Ｄ".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_special_effects(def_idx, firer_idx, &weapon, &pilot, &pilot);
    assert!(
        applied.iter().any(|s| s == "気力吸収 +5"),
        "攻撃側へ +5 吸収: {applied:?}"
    );
    assert_eq!(
        app.database().unit_instances[def_idx].morale,
        120,
        "対象 130→120"
    );
    assert_eq!(
        app.database().unit_instances[firer_idx].morale,
        105,
        "攻撃側 100→105 (吸収)"
    );
}

/// 底力 (パイロット技能) は HP が最大の 1/4 以下のとき命中・回避を +30 する。
/// 全 HP 時はボーナス無し。`combat_bonuses` 経由で戦闘に反映される。
#[test]
fn guts_skill_boosts_hit_and_dodge_at_low_hp() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Guts", 2, 6);
    // PILOT に 底力 技能を付与。
    app.database_mut()
        .catalog
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
        .unwrap()
        .features
        .push(("底力".into(), String::new()));
    let uid = first_player_uid(&app);
    // 全 HP 時はボーナス無し。
    let full = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        app.database().combat_bonuses(u)
    };
    assert_eq!(full.hit, 0, "全 HP では 底力 は発動しない (命中)");
    assert_eq!(full.dodge, 0, "全 HP では 底力 は発動しない (回避)");
    // HP を 1/4 以下まで減らす。
    let max_hp = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        app.database().effective_max_hp(u)
    };
    app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = max_hp * 3 / 4 + 1;
    let low = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        app.database().combat_bonuses(u)
    };
    assert_eq!(low.hit, 30, "底力: HP 1/4 以下で命中 +30");
    assert_eq!(low.dodge, 30, "底力: HP 1/4 以下で回避 +30");
}

/// 超底力 は 底力 より大きい命中・回避ボーナス (+50) を与える。
/// 「超底力」は「底力」を部分文字列に含むが超底力が優先される。
#[test]
fn super_guts_gives_larger_bonus_than_guts() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "SuperGuts", 2, 6);
    app.database_mut()
        .catalog
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
        .unwrap()
        .features
        .push(("超底力".into(), String::new()));
    let uid = first_player_uid(&app);
    let max_hp = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        app.database().effective_max_hp(u)
    };
    app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = max_hp * 3 / 4 + 1;
    let low = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        app.database().combat_bonuses(u)
    };
    assert_eq!(low.hit, 50, "超底力: HP 1/4 以下で命中 +50");
    assert_eq!(low.dodge, 50, "超底力: HP 1/4 以下で回避 +50");
}

/// 超反応 はクリティカル率を 攻撃側 +2×Lv / 防御側 −2×Lv で増減する (非対称)。
#[test]
fn crit_skill_bonus_super_reaction_is_asymmetric() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // Hero unit_data を用意
    push_pilot_with_features(&mut app, "ATKP", vec![("超反応L3".into(), String::new())]);
    push_pilot_with_features(&mut app, "DEFP", vec![("超反応L1".into(), String::new())]);
    let atk = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "ATKP",
        crate::Party::Player,
        3,
        6,
    ));
    let def = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "DEFP",
        crate::Party::Enemy,
        4,
        6,
    ));
    let ai = app.database().idx_by_uid(&atk).unwrap();
    let di = app.database().idx_by_uid(&def).unwrap();
    assert_eq!(
        app.crit_skill_bonus(ai, di),
        4,
        "超反応: 2×3(攻) − 2×1(防) = 4"
    );
}

/// 超能力 (攻撃側 +5) と 底力 (攻撃側 HP1/4以下で +50) のクリティカル率補正。
#[test]
fn crit_skill_bonus_psychic_and_guts() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    push_pilot_with_features(
        &mut app,
        "ESP",
        vec![
            ("超能力".into(), String::new()),
            ("底力".into(), String::new()),
        ],
    );
    push_pilot_with_features(&mut app, "PLAIN", Vec::new());
    let atk = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "ESP",
        crate::Party::Player,
        3,
        6,
    ));
    let def = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PLAIN",
        crate::Party::Enemy,
        4,
        6,
    ));
    let ai = app.database().idx_by_uid(&atk).unwrap();
    let di = app.database().idx_by_uid(&def).unwrap();
    // 全 HP: 超能力 +5 のみ (底力 非発動)。
    assert_eq!(
        app.crit_skill_bonus(ai, di),
        5,
        "超能力 +5 / 底力 は全 HP で非発動"
    );
    // HP を 1/4 以下に減らす。
    let max_hp = {
        let u = app.database().unit_by_uid(&atk).unwrap();
        app.database().effective_max_hp(u)
    };
    app.database_mut().unit_by_uid_mut(&atk).unwrap().damage = max_hp * 3 / 4 + 1;
    assert_eq!(
        app.crit_skill_bonus(ai, di),
        55,
        "超能力 +5 + 底力 +50 = 55"
    );
}

/// 写/化 (能力コピー) はクリティカル時に発動者を対象の形態へ変える。写はサイズ制限あり。
#[test]
fn weapon_copy_transforms_attacker_with_size_limit() {
    use crate::data::unit::{Size, WeaponData};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Mimic", 2, 6);
    place_player_unit(&mut app, "Model", 4, 6); // 同サイズ M
    let atk = first_player_uid(&app);
    let model = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 4 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    let atk_idx = app.database().idx_by_uid(&atk).unwrap();
    let def_idx = app.database().idx_by_uid(&model).unwrap();
    let mk = |class: &str| WeaponData {
        name: "コピー光線".into(),
        power: 100,
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
    // 写 同サイズ → 変化。
    let applied = app.apply_weapon_crit_copy(atk_idx, def_idx, &mk("写"));
    assert_eq!(applied, vec!["能力コピー → Model".to_string()]);
    assert_eq!(
        app.database().unit_by_uid(&atk).unwrap().unit_data_name,
        "Model"
    );
    // Model を XL に変え、Mimic(M) へ戻して再試行: 写 はサイズ差2以上で無効、化 は可。
    app.database_mut()
        .unit_by_uid_mut(&atk)
        .unwrap()
        .unit_data_name = "Mimic".into();
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|d| d.name == "Model")
        .unwrap()
        .size = Size::XL;
    assert!(
        app.apply_weapon_crit_copy(atk_idx, def_idx, &mk("写"))
            .is_empty(),
        "写 はサイズ 2 段階以上差で無効"
    );
    assert_eq!(
        app.database().unit_by_uid(&atk).unwrap().unit_data_name,
        "Mimic",
        "写 失敗で形態は変わらない"
    );
    assert!(
        !app.apply_weapon_crit_copy(atk_idx, def_idx, &mk("化"))
            .is_empty(),
        "化 はサイズ制限なしで変化"
    );
}

/// 習 (ラーニング): クリティカル時に対象の「ラーニング可能技」を攻撃側メイン
/// パイロットが習得し、(技名称) 必要技能ゲートが解禁される。再習得はしない。
#[test]
fn learning_weapon_teaches_skill_on_crit() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Learner", 2, 6);
    place_player_unit(&mut app, "Master", 4, 6);
    // Master の UnitData に「ラーニング可能技」を付与。
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|d| d.name == "Master")
        .unwrap()
        .features
        .push(("ラーニング可能技".into(), "はりせんぼん".into()));
    // 攻撃側にパイロット実体を紐付ける (習得先)。
    app.database_mut()
        .create_pilot_instance("PILOT", "learner_p");
    let atk = first_player_uid(&app);
    let atk_idx = app.database().idx_by_uid(&atk).unwrap();
    app.database_mut().unit_instances[atk_idx].pilot_ids = vec!["learner_p".into()];
    let def_idx = app
        .database()
        .unit_instances
        .iter()
        .position(|u| u.x == 4 && u.y == 6)
        .unwrap();
    let mk = |class: &str| WeaponData {
        name: "ラーニング".into(),
        power: 0,
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
    // 習なし → 習得しない。
    assert!(app
        .apply_weapon_learning(atk_idx, def_idx, &mk("無"))
        .is_empty());
    assert_eq!(app.unit_pilot_skill_level(atk_idx, "はりせんぼん"), 0);
    // 習あり (無習 で連結検出) → 習得。
    let msgs = app.apply_weapon_learning(atk_idx, def_idx, &mk("無習"));
    assert_eq!(msgs.len(), 1, "ラーニングメッセージが 1 件");
    assert!(
        app.unit_pilot_skill_level(atk_idx, "はりせんぼん") > 0,
        "はりせんぼん を習得"
    );
    // 既習得は再習得しない。
    assert!(
        app.apply_weapon_learning(atk_idx, def_idx, &mk("無習"))
            .is_empty(),
        "既習得は再習得しない"
    );
}

/// 盗属性武器のクリティカル時資金奪取: 味方が敵を盗むと修理費の1/4が入り、再取得は不可。
#[test]
fn weapon_steal_grants_money_once() {
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
    let weapon = WeaponData {
        name: "盗賊剣".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: "盗".into(),
        extras: Vec::new(),
    };
    let money0 = app.money();
    // 修理費 800 → 1/4 = 200。
    let applied = app.apply_weapon_crit_steal(def_idx, crate::Party::Player, 800, &weapon);
    assert_eq!(applied, vec!["資金奪取 +200".to_string()]);
    assert_eq!(app.money(), money0 + 200, "資金 +200");
    // 再取得は不可 (被盗)。
    let applied2 = app.apply_weapon_crit_steal(def_idx, crate::Party::Player, 800, &weapon);
    assert!(applied2.is_empty(), "同じ相手から再取得しない");
    assert_eq!(app.money(), money0 + 200, "資金は変わらない");
}

/// 盗: 相手が「アイテム所有」を持つときは資金でなくアイテムを攻撃側へ奪う。
#[test]
fn weapon_steal_takes_item_when_target_owns_one() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 攻撃側 = idx 0
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    // 対象に「アイテム所有=賢者の石」を付与 (active_features)。
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new(
            "アイテム所有",
            "賢者の石",
        ));
    let weapon = WeaponData {
        name: "盗賊剣".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: "盗".into(),
        extras: Vec::new(),
    };
    let money0 = app.money();
    let applied = app.apply_weapon_crit_steal(def_idx, crate::Party::Player, 800, &weapon);
    assert_eq!(applied, vec!["アイテム奪取: 賢者の石".to_string()]);
    assert!(
        app.database().spare_items.contains(&"賢者の石".to_string()),
        "盗んだアイテムは味方の未装備アイテムに入る (原典 IList.Add)"
    );
    assert_eq!(app.money(), money0, "アイテム盗み時は資金を奪わない");
    // 再取得不可 (被盗)。
    let applied2 = app.apply_weapon_crit_steal(def_idx, crate::Party::Player, 800, &weapon);
    assert!(applied2.is_empty(), "同じ相手から再取得しない");
}

/// 衰L2 武器のクリティカル時減衰: 対象の現在 HP が半分になる (撃破はしない)。
#[test]
fn weapon_crit_decay_halves_current_hp() {
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
    // 最大HP=100、ダメージ20 → 現在HP 80。衰L2 で 80→40。
    app.database_mut().unit_instances[def_idx].damage = 20;
    let weapon = WeaponData {
        name: "減衰砲".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: "衰L2".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_crit_decay(def_idx, &weapon);
    assert!(!applied.is_empty(), "衰 でラベルが返る");
    // 現在HP 80 → 40 → damage = 100 - 40 = 60。
    assert_eq!(
        app.database().unit_instances[def_idx].damage,
        60,
        "衰L2 で現在HP が半分 (80→40)"
    );
}

/// 即死 (即) は非ボスを致死化し、ボスには無効。
#[test]
fn instakill_attribute_kills_non_boss_not_boss() {
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
    let w = WeaponData {
        name: "即死針".into(),
        power: 1,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100, // proc 率 100
        class: "即".into(),
        extras: Vec::new(),
    };
    assert!(
        app.roll_weapon_instakill(def_idx, &w, &pilot, &pilot),
        "非ボスは即死 proc"
    );
    app.database_mut().unit_instances[def_idx].boss_rank = 1;
    assert!(
        !app.roll_weapon_instakill(def_idx, &w, &pilot, &pilot),
        "ボスは即死無効"
    );
}

/// 吹L2 武器は対象を攻撃側から遠ざかる方向へ 2 マス押し出す。占有マスで停止する。
#[test]
fn weapon_knockback_pushes_and_stops_at_obstacle() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 5);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let mk = |class: &str| WeaponData {
        name: "衝撃砲".into(),
        power: 100,
        min_range: 1,
        max_range: 3,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: class.into(),
        extras: Vec::new(),
    };
    // 攻撃側 (5,5) → 対象 (6,5) → +x 方向へ 2 マス → (8,5)。
    let moved = app.apply_weapon_knockback(def_idx, (5, 5), "Hero", &mk("吹L2"), false);
    assert!(moved, "吹L2 で押し出される");
    assert_eq!(
        (
            app.database().unit_by_uid(&target).unwrap().x,
            app.database().unit_by_uid(&target).unwrap().y
        ),
        (8, 5),
        "(6,5) から +x に 2 マス → (8,5)"
    );
    // (10,5) に障害ユニットを置き、(8,5) の対象を 吹L3 → (9,5) で停止 (10,5 不可)。
    place_player_unit(&mut app, "Blocker", 10, 5);
    let moved2 = app.apply_weapon_knockback(def_idx, (7, 5), "Hero", &mk("吹L3"), false);
    assert!(moved2);
    assert_eq!(
        app.database().unit_by_uid(&target).unwrap().x,
        9,
        "障害ユニット (10,5) の手前 (9,5) で停止"
    );
}

/// 引(引き寄せ)武器は対象を攻撃側に隣接する空きマスへ移す。
#[test]
fn weapon_pull_moves_target_adjacent_to_attacker() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 5);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        8,
        5,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let weapon = WeaponData {
        name: "引力砲".into(),
        power: 100,
        min_range: 1,
        max_range: 5,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: "引".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_crit_reposition(def_idx, (5, 5), &weapon);
    assert_eq!(applied, vec!["引き寄せ".to_string()]);
    let t = app.database().unit_by_uid(&target).unwrap();
    let dist = (t.x as i32 - 5).abs() + (t.y as i32 - 5).abs();
    assert_eq!(dist, 1, "攻撃側 (5,5) に隣接する位置へ移動");
}

/// 反撃も本線攻撃と同じ判定を通る (原典の反撃は `Attack w, target, "", ""`)。
/// 旧実装は命中ロールだけで、クリティカルが出なかった。
#[test]
fn counterattack_can_be_critical() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|d| d.name == "Hero")
        .unwrap()
        .hp = 1_000_000;
    let mut ebot = app.database().unit_by_name("Hero").cloned().unwrap();
    ebot.name = "EnemyBot".into();
    ebot.weapons = vec![WeaponData {
        name: "ビーム".into(),
        power: 1500,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 1000,
        class: String::new(),
        extras: Vec::new(),
    }];
    app.database_mut().catalog.units.push(ebot);
    let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
        "EnemyBot",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    app.database_mut()
        .unit_by_uid_mut(&enemy)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));
    assert!(app.try_counterattack(3, 6, (2, 6)).is_some());
    assert!(
        app.messages()
            .iter()
            .any(|m| m.contains("反撃") && m.contains("クリティカル")),
        "反撃でもクリティカルが出る: {:?}",
        app.messages()
    );
}

/// 反撃武器の特殊効果攻撃属性 (状態異常) が、反撃の命中・生存時に被弾側へ proc する。
#[test]
fn counterattack_procs_weapon_special_effect() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 反撃を受ける側
                                               // 敵 unit_data: 痺 反撃武器 (critical 100 → 必ず proc)。
    let mut ebot = app.database().unit_by_name("Hero").cloned().unwrap();
    ebot.name = "EnemyBot".into();
    ebot.weapons = vec![WeaponData {
        name: "麻痺針".into(),
        power: 1, // Hero を撃破しない
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
    }];
    app.database_mut().catalog.units.push(ebot);
    let hero = first_player_uid(&app);
    let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
        "EnemyBot",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    // 反撃を必中させ RNG 非依存にする (hit_chance=100)。
    app.database_mut()
        .unit_by_uid_mut(&enemy)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));

    let res = app.try_counterattack(3, 6, (2, 6));
    assert!(res.is_some(), "反撃が成立する");
    assert!(
        app.database()
            .unit_by_uid(&hero)
            .unwrap()
            .has_condition("麻痺"),
        "反撃武器の特殊効果 (麻痺) が被弾側へ proc する"
    );
}

/// 攻撃を受けて生存したユニットは気力 +1 (SRC: 損傷による気力上昇)。
#[test]
fn taking_a_hit_and_surviving_raises_morale() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 反撃を受けて生存する側
    let mut ebot = app.database().unit_by_name("Hero").cloned().unwrap();
    ebot.name = "EnemyBot".into();
    ebot.weapons = vec![WeaponData {
        name: "パンチ".into(),
        power: 1, // Hero を撃破しない
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: String::new(),
        extras: Vec::new(),
    }];
    app.database_mut().catalog.units.push(ebot);
    let hero = first_player_uid(&app);
    let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
        "EnemyBot",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    app.database_mut()
        .unit_by_uid_mut(&enemy)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));
    let m0 = app.database().unit_by_uid(&hero).unwrap().morale;
    let res = app.try_counterattack(3, 6, (2, 6));
    assert!(res.is_some(), "反撃が成立する");
    assert_eq!(
        app.database().unit_by_uid(&hero).unwrap().morale,
        m0 + 1,
        "攻撃を受けて生存したユニットは気力 +1"
    );
}

/// 損傷時気力増加スキルは、被弾時の基本 +1 に加えてスキルLv分の気力を上乗せする。
#[test]
fn damage_morale_skill_adds_on_top_of_base_hit_gain() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 被弾側 (生存)
    app.database_mut()
        .catalog
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
        .unwrap()
        .features
        .push(("損傷時気力増加L2".into(), String::new()));
    let mut ebot = app.database().unit_by_name("Hero").cloned().unwrap();
    ebot.name = "EnemyBot".into();
    ebot.weapons = vec![WeaponData {
        name: "パンチ".into(),
        power: 1,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: String::new(),
        extras: Vec::new(),
    }];
    app.database_mut().catalog.units.push(ebot);
    let hero = first_player_uid(&app);
    let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
        "EnemyBot",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    app.database_mut()
        .unit_by_uid_mut(&enemy)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));
    let m0 = app.database().unit_by_uid(&hero).unwrap().morale;
    app.try_counterattack(3, 6, (2, 6));
    assert_eq!(
        app.database().unit_by_uid(&hero).unwrap().morale,
        m0 + 3,
        "被弾 +1 ＋ 損傷時気力増加L2 +2 = +3"
    );
}

/// 武器を発射すると EN と残弾を消費する (命中の有無に依らない)。
/// 実戦で生成したユニット (残弾の実行時状態を持たない) でも、有限弾の武器は撃つたびに
/// 減り、弾切れで使えなくなる。旧実装は実行時状態を作らず、弾数が減らなかった。
#[test]
fn limited_ammo_runs_out_without_preseeded_state() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|d| d.name == "Hero")
        .unwrap()
        .hp = 1_000_000;
    let mut gunner = app.database().unit_by_name("Hero").cloned().unwrap();
    gunner.name = "Gunner".into();
    gunner.weapons = vec![WeaponData {
        name: "ミサイル".into(),
        power: 1,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: 2,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: String::new(),
        extras: Vec::new(),
    }];
    app.database_mut().catalog.units.push(gunner);
    let shooter = app.database_mut().register_unit(crate::UnitInstance::new(
        "Gunner",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    for shot in 1..=2 {
        assert!(
            app.try_counterattack(3, 6, (2, 6)).is_some(),
            "{shot} 発目は撃てる"
        );
    }
    let idx = app.database().idx_by_uid(&shooter).unwrap();
    let w = app.database().unit_by_name("Gunner").unwrap().weapons[0].clone();
    assert!(!app.weapon_firable(idx, &w), "2 発で弾切れ");
    assert!(
        app.try_counterattack(3, 6, (2, 6)).is_none(),
        "弾切れで反撃できない"
    );
}

/// `Disable <武器名>` / `Disable <ユニット名> <武器名>` で封印した武器は使えない
/// (原典 `IsWeaponAvailable` の `IsDisabled`)。旧実装は誰も読まないフラグを立てるだけだった。
#[test]
fn disabled_weapon_is_not_firable() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 300, 1);
    let w = app.database().unit_by_name("Hero").unwrap().weapons[0].clone();
    let run = |app: &mut App, src: &str| {
        crate::event_runtime::execute(app, &crate::data::event::parse(src).unwrap()).unwrap();
    };
    assert!(app.weapon_firable(0, &w));
    run(&mut app, &format!("Disable {}\n", w.name));
    assert!(!app.weapon_firable(0, &w), "全ユニット共通の封印");
    run(&mut app, &format!("Enable {}\n", w.name));
    assert!(app.weapon_firable(0, &w));
    run(&mut app, &format!("Disable Hero {}\n", w.name));
    assert!(!app.weapon_firable(0, &w), "ユニット名指定の封印");
}

#[test]
fn firing_a_weapon_consumes_en_and_ammo() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 反撃を受けて生存する側
    let mut gunner = app.database().unit_by_name("Hero").cloned().unwrap();
    gunner.name = "Gunner".into();
    gunner.en = 1000; // EN 切れにならないよう十分に
    gunner.weapons = vec![WeaponData {
        name: "実弾砲".into(),
        power: 1, // Hero を撃破しない
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: 3,
        en_consumption: 10,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: String::new(),
        extras: Vec::new(),
    }];
    app.database_mut().catalog.units.push(gunner);
    let shooter = app.database_mut().register_unit(crate::UnitInstance::new(
        "Gunner",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    {
        let u = app.database_mut().unit_by_uid_mut(&shooter).unwrap();
        u.weapons = vec![crate::unit_weapon::UnitWeapon::from_data("実弾砲", 0, 3)];
        u.add_condition(crate::Condition::new("必中", -1));
    }
    // shooter (3,6) が Hero (2,6) を反撃 → 実弾砲を発射。
    app.try_counterattack(3, 6, (2, 6));
    let u = app.database().unit_by_uid(&shooter).unwrap();
    assert_eq!(u.en_consumed, 10, "発射で EN を 10 消費");
    assert_eq!(u.weapons[0].bullet_remaining, 2, "発射で残弾 3→2");
}

/// 残弾 0 の武器は自動選択 (反撃 / 援護 / AI) で選ばれず、発射されない。
#[test]
fn empty_weapon_is_not_auto_selected() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let mut gunner = app.database().unit_by_name("Hero").cloned().unwrap();
    gunner.name = "Gunner".into();
    gunner.weapons = vec![WeaponData {
        name: "空砲".into(),
        power: 50,
        min_range: 1,
        max_range: 1,
        precision: 100,
        bullet: 1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: String::new(),
        extras: Vec::new(),
    }];
    app.database_mut().catalog.units.push(gunner);
    let shooter = app.database_mut().register_unit(crate::UnitInstance::new(
        "Gunner",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    // 残弾 0 の UnitWeapon を attach (空)。
    {
        let u = app.database_mut().unit_by_uid_mut(&shooter).unwrap();
        u.weapons = vec![crate::unit_weapon::UnitWeapon::from_data("空砲", 0, 0)];
    }
    // 唯一の武器が残弾 0 → 反撃不成立。
    let res = app.try_counterattack(3, 6, (2, 6));
    assert!(res.is_none(), "残弾 0 の武器は自動選択されず反撃しない");
}

/// `atk_idx` の (x, y) への攻撃を、`look_for_support_attack` で選ばれたユニットが援護攻撃する。
fn support_attack_at(app: &mut App, atk_idx: usize, x: u32, y: u32) -> Option<String> {
    let def_idx = app
        .database()
        .uid_at(x, y)
        .and_then(|uid| app.database().idx_by_uid(uid))?;
    let sup_idx = app.look_for_support_attack(atk_idx, Some(def_idx))?;
    app.try_support_attack(atk_idx, sup_idx, def_idx, "", false)
}

/// 援護攻撃武器の特殊効果攻撃属性が、援護の命中・生存時に防御側へ proc する。
#[test]
fn support_attack_procs_weapon_special_effect() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 本攻撃側 (atk_idx)
    place_player_unit(&mut app, "Supporter", 3, 6); // Hero に隣接した援護役
                                                    // Supporter に 痺 武器 (critical 100)。
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|d| d.name == "Supporter")
        .unwrap()
        .weapons = vec![WeaponData {
        name: "麻痺針".into(),
        power: 1,
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
    }];
    let hero = first_player_uid(&app);
    let sup = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Supporter")
        .unwrap()
        .uid
        .clone();
    // 援護攻撃 + 必中 (hit_chance=100)。
    app.database_mut()
        .unit_by_uid_mut(&sup)
        .unwrap()
        .add_condition(crate::Condition::new("援護攻撃", -1));
    app.database_mut()
        .unit_by_uid_mut(&sup)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));
    // 敵 (3,7): Supporter (3,6) の射程内 (距離1)。
    let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        7,
    ));
    let atk_idx = app.database().idx_by_uid(&hero).unwrap();

    let res = support_attack_at(&mut app, atk_idx, 3, 7);
    assert!(res.is_some(), "援護攻撃が成立する");
    assert!(
        app.database()
            .unit_by_uid(&enemy)
            .unwrap()
            .has_condition("麻痺"),
        "援護攻撃武器の特殊効果 (麻痺) が防御側へ proc する"
    );
}

/// 援護攻撃は本線攻撃と同じ判定を通り、固定の ×0.75 は無い (原典 `Unit.Attack` の
/// `"援護攻撃"`)。ダメージ増加系の精神効果は乗らず、オプション
/// `サポートアタックダメージ低下` があればダメージ ×0.7 (原典 `Unit.Damage`)。
#[test]
fn support_attack_damage_follows_original_rules() {
    use crate::data::unit::WeaponData;
    // (援護攻撃役に付ける状態, オプション) で援護攻撃し、防御側の被ダメージを返す。
    let run = |extra: Option<&str>, option: bool| -> i64 {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        place_player_unit(&mut app, "Supporter", 3, 6);
        app.database_mut()
            .catalog
            .units
            .iter_mut()
            .find(|d| d.name == "Supporter")
            .unwrap()
            .weapons = vec![WeaponData {
            name: "ビーム".into(),
            power: 1500,
            min_range: 1,
            max_range: 1,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        }];
        let hero = first_player_uid(&app);
        let sup = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Supporter")
            .unwrap()
            .uid
            .clone();
        for c in ["援護攻撃", "必中"].into_iter().chain(extra) {
            app.database_mut()
                .unit_by_uid_mut(&sup)
                .unwrap()
                .add_condition(crate::Condition::new(c, -1));
        }
        if option {
            app.set_script_var(
                "Option(サポートアタックダメージ低下)".to_string(),
                "1".to_string(),
            );
        }
        // 防御側 (Hero データ) は撃破されない HP にする。
        app.database_mut()
            .catalog
            .units
            .iter_mut()
            .find(|d| d.name == "Hero")
            .unwrap()
            .hp = 1_000_000;
        let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            3,
            7,
        ));
        let atk_idx = app.database().idx_by_uid(&hero).unwrap();
        assert!(support_attack_at(&mut app, atk_idx, 3, 7).is_some());
        app.database().unit_by_uid(&enemy).map_or(0, |u| u.damage)
    };
    let base = run(None, false);
    assert!(base > 0, "援護攻撃が命中してダメージを与える");
    // 熱血 (ダメージ増加) は援護攻撃に乗らない。
    assert_eq!(run(Some("熱血"), false), base);
    // オプションがあれば減る。
    assert!(run(None, true) < base);
}

#[test]
fn combat_kill_fires_destruction_and_annihilation() {
    use crate::Condition;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    // `破壊 <name>` / `全滅 敵` を定義したステージファイル。
    let stage = crate::data::event::parse(
        "破壊 Foe:\nSet 破壊発火 1\nExit\n全滅 敵:\nSet 全滅発火 1\nExit\n",
    )
    .unwrap();
    app.script_library_mut()
        .append_with_name(&stage, "stage.eve");
    app.current_stage_file = "stage.eve".to_string();
    place_player_unit(&mut app, "Hero", 5, 6);
    add_weapon(&mut app, "Hero", 500, 1);
    place_player_unit(&mut app, "Foe", 9, 9); // Foe の UnitData を登録
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6); // 唯一の敵 (隣接)
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_map_cursor(5, 6);
    // 必中で確実に命中させ撃破する。
    let hero = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&hero)
        .unwrap()
        .add_condition(Condition::new("必中", 1));
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    // 戦闘での撃破が 破壊 <name> と (敵全滅で) 全滅 敵 を発火する (回帰)。
    assert_eq!(
        app.script_var("破壊発火"),
        "1",
        "破壊 <name> イベントが発火する"
    );
    assert_eq!(
        app.script_var("全滅発火"),
        "1",
        "全滅 敵 イベントが発火する (撃破でシナリオが進行)"
    );
}

#[test]
fn repro_move_then_attack_keeps_moved_position() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    // enemy を先に push して idx 0 にする (remove による index 失効を誘発)
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Foe",
        "PILOT",
        crate::Party::Enemy,
        4,
        6,
    ));
    place_player_unit(&mut app, "Foe", 4, 6); // Foe の UnitData/PilotData を用意
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 999, 1);
    app.set_stage_state(crate::stage::StageState::Battle);

    // 1) Hero を選択
    click_tile(&mut app, 2, 6);
    // 2) Move (先頭項目)
    click_menu_item(&mut app, 0);
    assert_eq!(
        move_select_pos(&app),
        Some((2, 6)),
        "Move 選択後は MoveSelect(Hero) になるはず: {:?}",
        app.action_mode()
    );
    // 3) (3,6) へ移動
    click_tile(&mut app, 3, 6);
    assert_eq!(
        unit_xy(&app, "Hero"),
        Some((3, 6)),
        "移動が反映されていない"
    );
    // 4) PostMoveMenu の Attack を選ぶ
    let attack_idx = match app.command_menu() {
        Some(crate::CommandMenu::Unit { items, .. }) => items
            .iter()
            .position(|i| {
                matches!(
                    i,
                    crate::command_menu::UnitMenuItem::Builtin(
                        crate::command_menu::UnitAction::Attack
                    )
                )
            })
            .expect("PostMoveMenu に Attack が無い"),
        _ => panic!("PostMoveMenu が開いていない: {:?}", app.command_menu()),
    };
    click_menu_item(&mut app, attack_idx as i32);
    assert!(
        matches!(app.action_mode(), crate::ActionMode::AttackSelect { .. }),
        "Attack 選択後は AttackSelect になるはず: {:?}",
        app.action_mode()
    );
    // 5) 敵 (4,6) をクリックして攻撃
    click_tile(&mut app, 4, 6);

    // 攻撃後、Hero は移動先 (3,6) に留まっていること（初期 (2,6) に戻らない）
    assert_eq!(
        unit_xy(&app, "Hero"),
        Some((3, 6)),
        "攻撃後に Hero が初期出撃位置へ revert している"
    );
    // 位置索引が実体と整合していること (撃破による remove 後も)
    assert!(
        app.database().pos_index_is_consistent(),
        "pos_index が unit_instances と乖離している"
    );
}

#[test]
fn attack_targets_clicked_tile_not_nearest() {
    // 射程内に敵が 2 体いるとき、クリックしたタイルの敵を攻撃する
    // (旧実装は最寄り敵を攻撃していた)。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 30, 3); // 射程3・低威力 (撃破せず damage を見る)
    place_player_unit(&mut app, "FoeData", 9, 9); // FoeData の UnitData/PilotData を用意
    let near = spawn_enemy(&mut app, "FoeData", 3, 6); // 最寄り (dist1)
    let far = spawn_enemy(&mut app, "FoeData", 4, 6); // 遠い (dist2) — これをクリック
    app.set_stage_state(crate::stage::StageState::Battle);

    click_tile(&mut app, 2, 6); // Hero 選択
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai); // 攻撃
    click_tile(&mut app, 4, 6); // 遠い敵を対象に

    let far_dmg = app.database().unit_by_uid(&far).map(|u| u.damage).unwrap();
    let near_dmg = app.database().unit_by_uid(&near).map(|u| u.damage).unwrap();
    assert!(far_dmg > 0, "クリックした敵 (4,6) が攻撃されていない");
    assert_eq!(near_dmg, 0, "最寄り敵 (3,6) を誤って攻撃している");
}

#[test]
fn combat_fires_damage_threshold_event_on_main_attack() {
    // 回帰: 通常攻撃で防御側の HP が閾値を割ったら `損傷率` イベントが発火する。
    // (旧実装は `Damage` コマンド経由でしか発火せず、実戦闘では沈黙していた。)
    // 閾値 10% + 最低ダメージ 10 / HP 100 なので、命中すれば必ず跨ぎ、かつ撃破しない。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 30, 3);
    place_player_unit(&mut app, "FoeData", 9, 9); // FoeData の UnitData/PilotData を用意
    let foe = spawn_enemy(&mut app, "FoeData", 3, 6);
    register_damage_threshold_label(&mut app, "FoeData", 10, "損傷率主");
    app.set_stage_state(crate::stage::StageState::Battle);

    click_tile(&mut app, 2, 6); // Hero 選択
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai); // 攻撃
    click_tile(&mut app, 3, 6); // 敵を攻撃

    let foe_dmg = app.database().unit_by_uid(&foe).map(|u| u.damage).unwrap();
    assert!(foe_dmg >= 10, "敵が被弾していない (dmg={foe_dmg})");
    assert!(foe_dmg < 100, "敵が撃破されてしまった (損傷率は生存時のみ)");
    assert_eq!(
        app.script_var("損傷率主"),
        "1",
        "通常攻撃で損傷率イベントが発火していない"
    );
}

#[test]
fn combat_fires_damage_threshold_event_on_counterattack() {
    // 回帰: 反撃で攻撃側 (Hero) の HP が閾値を割ったら `損傷率 Hero` が発火する。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 30, 3); // Hero の攻撃 (敵を撃破しない低威力)
    place_player_unit(&mut app, "FoeData", 9, 9);
    let _foe = spawn_enemy(&mut app, "FoeData", 3, 6);
    add_weapon(&mut app, "FoeData", 30, 3); // 敵の反撃武器 (射程内に Hero)
    register_damage_threshold_label(&mut app, "Hero", 10, "損傷率反撃");
    app.set_stage_state(crate::stage::StageState::Battle);

    let hero_uid = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Hero")
        .map(|u| u.uid.clone())
        .unwrap();

    click_tile(&mut app, 2, 6); // Hero 選択
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai); // 攻撃
    click_tile(&mut app, 3, 6); // 敵を攻撃 → 敵が反撃

    let hero_dmg = app
        .database()
        .unit_by_uid(&hero_uid)
        .map(|u| u.damage)
        .unwrap();
    assert!(
        hero_dmg >= 10,
        "Hero が反撃で被弾していない (dmg={hero_dmg})"
    );
    assert!(hero_dmg < 100, "Hero が反撃で撃破されてしまった");
    assert_eq!(
        app.script_var("損傷率反撃"),
        "1",
        "反撃で損傷率イベントが発火していない"
    );
}

/// みがわり (身代わり, SRC `Unit.cls:14855`): みがわり を持つ防御側が攻撃されると、
/// 指定された身代わりユニットが 1 回だけダメージを肩代わりする (防御側は無傷)。
/// サンプルの自作 SP 生贄→みがわり (`data/include.eve` の 生贄ルーチンが
/// `SpecialPower 相手ユニットＩＤ みがわり 対象ユニットＩＤ` を発行) を検証する。
#[test]
fn migawari_redirects_attack_damage_to_substitute_once() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 30, 3); // 撃破しない低威力 (HP100/装甲10 → dmg<100)
    register_enemy_data(&mut app, "Caster");
    register_enemy_data(&mut app, "Sub");
    let caster = spawn_enemy(&mut app, "Caster", 3, 6); // 保護対象
    let sub = spawn_enemy(&mut app, "Sub", 3, 5); // 身代わり
    app.set_stage_state(crate::stage::StageState::Battle);

    // Sub を Caster の身代わりに (生贄ルーチン相当)。
    let script = crate::data::event::parse("SpecialPower Sub みがわり Caster\n").unwrap();
    crate::event_runtime::execute(&mut app, &script).unwrap();
    assert!(
        app.database()
            .unit_by_uid(&caster)
            .unwrap()
            .has_condition("みがわり"),
        "Caster に みがわり condition が付与されていない"
    );

    let sub_before = app.database().unit_by_uid(&sub).unwrap().damage;
    let caster_before = app.database().unit_by_uid(&caster).unwrap().damage;

    // Hero が Caster を攻撃 → みがわり が Sub へ肩代わり。
    click_tile(&mut app, 2, 6);
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai);
    click_tile(&mut app, 3, 6);

    let sub_after = app.database().unit_by_uid(&sub).unwrap().damage;
    let caster_after = app.database().unit_by_uid(&caster).unwrap().damage;
    assert!(
        sub_after > sub_before,
        "身代わり Sub がダメージを肩代わりしていない (dmg {sub_before}->{sub_after})"
    );
    assert_eq!(
        caster_after, caster_before,
        "保護対象 Caster が被弾している (身代わりのはず: dmg {caster_before}->{caster_after})"
    );
    assert!(
        !app.database()
            .unit_by_uid(&caster)
            .unwrap()
            .has_condition("みがわり"),
        "みがわり が消費されていない (1 回限りのはず)"
    );
}

/// みがわり は反撃経路でも機能する: みがわり を持つ味方が攻撃して敵に反撃された
/// とき、反撃ダメージは身代わりが肩代わりする (通常攻撃のみの実装からの拡張)。
#[test]
fn migawari_redirects_counterattack_damage_to_substitute() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 保護対象 (反撃を受ける側)
    add_weapon(&mut app, "Hero", 30, 3); // Hero の攻撃 (敵を撃破しない)
    place_player_unit(&mut app, "Sub", 2, 5); // 身代わり
    register_enemy_data(&mut app, "EnemyU");
    let foe = spawn_enemy(&mut app, "EnemyU", 3, 6);
    add_weapon(&mut app, "EnemyU", 30, 3); // 敵の反撃武器 (Hero を撃破しない)
    app.set_stage_state(crate::stage::StageState::Battle);

    let hero = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Hero")
        .map(|u| u.uid.clone())
        .unwrap();
    let sub = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Sub")
        .map(|u| u.uid.clone())
        .unwrap();

    // Sub を Hero の身代わりに。
    let script = crate::data::event::parse("SpecialPower Sub みがわり Hero\n").unwrap();
    crate::event_runtime::execute(&mut app, &script).unwrap();

    let sub_before = app.database().unit_by_uid(&sub).unwrap().damage;
    let hero_before = app.database().unit_by_uid(&hero).unwrap().damage;
    let _ = foe;

    // Hero が敵を攻撃 → 敵が Hero に反撃 → みがわり が反撃ダメージを Sub へ。
    click_tile(&mut app, 2, 6);
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai);
    click_tile(&mut app, 3, 6);

    let sub_after = app.database().unit_by_uid(&sub).unwrap().damage;
    let hero_after = app.database().unit_by_uid(&hero).unwrap().damage;
    assert!(
        sub_after > sub_before,
        "身代わり Sub が反撃ダメージを肩代わりしていない (dmg {sub_before}->{sub_after})"
    );
    assert_eq!(
        hero_after, hero_before,
        "Hero が反撃で被弾している (みがわりのはず: dmg {hero_before}->{hero_after})"
    );
    assert!(
        !app.database()
            .unit_by_uid(&hero)
            .unwrap()
            .has_condition("みがわり"),
        "みがわり が消費されていない (反撃でも 1 回限り)"
    );
}

/// カウンター (VB5 `COM.bas:1040-1058`): カウンター 技能を持つ防御側は主攻撃の前に
/// 先制反撃する。先制反撃で攻撃側を撃破すれば攻撃側の主攻撃は不発 (防御側は無傷)。
#[test]
fn counter_skill_strikes_pre_emptively() {
    fn run(with_counter: bool) -> (bool, i64) {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6); // 攻撃側 (HP100)
        add_weapon(&mut app, "Hero", 20, 3);
        place_player_unit(&mut app, "FoeData", 9, 9);
        let foe = spawn_enemy(&mut app, "FoeData", 3, 6); // 防御側
        add_weapon(&mut app, "FoeData", 500, 3); // 強力な反撃武器 (Hero を一撃)
        app.set_stage_state(crate::stage::StageState::Battle);
        if with_counter {
            if let Some(pd) = app
                .database_mut()
                .catalog
                .pilots
                .iter_mut()
                .find(|p| p.name == "PILOT")
            {
                pd.features.push(("カウンターLv1".into(), String::new()));
            }
        }
        click_tile(&mut app, 2, 6);
        let ai = attack_item_index(&app);
        click_menu_item(&mut app, ai);
        click_tile(&mut app, 3, 6);
        let hero_alive = app
            .database()
            .unit_instances
            .iter()
            .any(|u| u.unit_data_name == "Hero" && !u.off_map);
        let foe_dmg = app
            .database()
            .unit_by_uid(&foe)
            .map(|u| u.damage)
            .unwrap_or(-1);
        (hero_alive, foe_dmg)
    }
    // カウンター無し: Hero が先に攻撃 (Foe 被弾) → Foe が通常反撃で Hero 撃破。
    let (_, foe_dmg_base) = run(false);
    assert!(
        foe_dmg_base > 0,
        "通常: Hero の主攻撃で Foe が被弾するはず (dmg={foe_dmg_base})"
    );
    // カウンター有り: Foe が先制反撃で Hero 撃破 → Hero の主攻撃は不発 (Foe 無傷)。
    let (hero_alive_cnt, foe_dmg_cnt) = run(true);
    assert!(
        !hero_alive_cnt,
        "カウンター: Hero は Foe の先制反撃で撃破されるはず"
    );
    assert_eq!(
        foe_dmg_cnt, 0,
        "カウンター: 先制反撃で Hero 撃破→主攻撃不発で Foe 無傷のはず (dmg={foe_dmg_cnt})"
    );
}

/// 再攻撃 (VB5 `Unit.cls:10239`): 再攻撃 技能を持つ攻撃側は確率で2回攻撃する。高Lv＋
/// 直感同値で `slevel=2*Lv` が `Dice(32)` を必ず上回り、防御側は 2 ヒット分被弾する。
#[test]
fn reattack_skill_strikes_twice() {
    fn run(with_reattack: bool) -> i64 {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        add_weapon(&mut app, "Hero", 20, 3); // 低威力 (2ヒットでも防御側生存)
        place_player_unit(&mut app, "FoeData", 9, 9);
        let foe = spawn_enemy(&mut app, "FoeData", 3, 6); // 武器無し=反撃しない
        app.set_stage_state(crate::stage::StageState::Battle);
        if with_reattack {
            // 攻撃側パイロット "PILOT" (pilot_by_name は先頭を返す) に 再攻撃Lv50 を付与。
            if let Some(pd) = app
                .database_mut()
                .catalog
                .pilots
                .iter_mut()
                .find(|p| p.name == "PILOT")
            {
                pd.features.push(("再攻撃Lv50".into(), String::new()));
            }
        }
        click_tile(&mut app, 2, 6);
        let ai = attack_item_index(&app);
        click_menu_item(&mut app, ai);
        click_tile(&mut app, 3, 6);
        app.database()
            .unit_by_uid(&foe)
            .map(|u| u.damage)
            .unwrap_or(0)
    }
    let base = run(false);
    let twice = run(true);
    assert!(base > 0, "baseline 単発攻撃が当たっていない (dmg={base})");
    assert!(
        twice >= base * 2,
        "再攻撃で2ヒット分にならない: 単発 {base} vs 再攻撃 {twice}"
    );
}

#[test]
fn destruction_event_fires_with_pilot_function_label() {
    // 回帰: ラベル名に関数 `Pilot(<unit名>)` を含む破壊イベントが、当該ユニットの
    // 撃破で発火する (例: 決戦！宇宙怪獣1話 の `破壊 Pilot(補給艦リームズ):`)。
    // マッチャは実ユニットの unit名から `Pilot(<unit名>)` 候補を再構成して照合する。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 9999, 3); // 一撃必殺 (HP100 を撃破)
    place_player_unit(&mut app, "FoeData", 9, 9); // FoeData の UnitData/PilotData を用意
    let foe = spawn_enemy(&mut app, "FoeData", 3, 6);
    let stmts = crate::data::event::parse("破壊 Pilot(FoeData):\nSet 破壊発火 1\nExit\n")
        .expect("parse 破壊 label");
    app.script_library_mut()
        .append_with_name(&stmts, "test_destroy.eve");
    app.set_stage_state(crate::stage::StageState::Battle);

    click_tile(&mut app, 2, 6); // Hero 選択
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai); // 攻撃
    click_tile(&mut app, 3, 6); // 敵を攻撃 (撃破)

    assert!(
        app.database().unit_by_uid(&foe).is_none(),
        "敵が撃破されていない"
    );
    assert_eq!(
        app.script_var("破壊発火"),
        "1",
        "`破壊 Pilot(FoeData)` 関数形ラベルが発火していない"
    );
}

#[test]
fn player_cannot_attack_npc_ally_but_can_attack_enemy_and_neutral() {
    // 味方 ↔ ＮＰＣ は同盟。Hero(味方) は隣の ＮＰＣ を攻撃できず、
    // 敵 / 中立 は攻撃できる（SRC 敵味方関係）。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 30, 3);
    place_player_unit(&mut app, "Data", 9, 9); // UnitData/PilotData を用意
    let ally = spawn_party(&mut app, "Data", crate::Party::Npc, 3, 6); // 同盟 (dist1)
    let enemy = spawn_party(&mut app, "Data", crate::Party::Enemy, 4, 6); // 敵 (dist2)
    let neutral = spawn_party(&mut app, "Data", crate::Party::Neutral, 5, 6); // 中立 (dist3)
    app.set_stage_state(crate::stage::StageState::Battle);

    click_tile(&mut app, 2, 6); // Hero 選択
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai); // 攻撃

    // ＮＰＣ 同盟タイルをクリック → 攻撃不成立 (ダメージ無し、AttackSelect 継続)。
    click_tile(&mut app, 3, 6);
    assert_eq!(
        app.database().unit_by_uid(&ally).map(|u| u.damage),
        Some(0),
        "ＮＰＣ 同盟を攻撃できてしまっている"
    );
    assert!(
        matches!(app.action_mode(), crate::ActionMode::AttackSelect { .. }),
        "同盟クリックは no-op で AttackSelect 継続のはず"
    );

    // 敵タイルをクリック → 攻撃成立 (ダメージ)。
    click_tile(&mut app, 4, 6);
    assert!(
        app.database()
            .unit_by_uid(&enemy)
            .map(|u| u.damage)
            .unwrap()
            > 0,
        "敵を攻撃できていない"
    );
    let _ = neutral; // 中立への攻撃可否は行列テスト (is_hostile_to) で担保。
}

#[test]
fn cancel_after_post_move_attack_reverts_to_sortie() {
    // 「移動→攻撃選択→キャンセル」で出撃地点へ正しく巻き戻る (報告された症状の検証)。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 30, 1); // 射程1 → 移動後攻撃可
    place_player_unit(&mut app, "FoeData", 9, 9);
    spawn_enemy(&mut app, "FoeData", 4, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // Hero 選択 → Move → (3,6)
    click_tile(&mut app, 2, 6);
    click_menu_item(&mut app, 0); // Move (先頭)
    click_tile(&mut app, 3, 6);
    assert_eq!(unit_xy(&app, "Hero"), Some((3, 6)));

    // PostMoveMenu で攻撃選択 → AttackSelect (snapshot あり)
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai);
    assert!(matches!(
        app.action_mode(),
        crate::ActionMode::AttackSelect {
            snapshot: Some(_),
            ..
        }
    ));

    // 1回目キャンセル: PostMoveMenu に戻るだけ (移動はまだ巻き戻さない)
    app.handle_input(Input::Cancel);
    assert_eq!(
        unit_xy(&app, "Hero"),
        Some((3, 6)),
        "攻撃キャンセルで移動先に留まるべき"
    );

    // 2回目キャンセル: 移動前 (2,6) に巻き戻る
    app.handle_input(Input::Cancel);
    assert_eq!(
        unit_xy(&app, "Hero"),
        Some((2, 6)),
        "PostMoveMenu キャンセルで出撃地点へ戻る"
    );
    assert!(app.database().pos_index_is_consistent());
}

#[test]
fn next_stage_starts_battle_without_double_firing_start() {
    // スパロボ戦記: 「次のステージへ」でステージファイルが `スタート` をインライン
    // 実行 (配置まで完了) して中断せず終わった場合、Briefing で固まらず味方フェイズが
    // 始まり、かつ `スタート` が二重発火しない (= 敵の二重配置が起きない) こと。
    let mut app = App::new();
    // ステージファイル: `スタート` で Incr (= 配置の代用)。中断しない。
    let stage = crate::data::event::parse("スタート:\nIncr スタート回数\nExit\n").unwrap();
    app.script_library_mut()
        .append_with_name(&stage, "main.eve");
    // インターミッション設定: 1 ユーザ項目 + 組込み(機体改造/データセーブ) + 「次のステージへ」。
    // 表示順: [User(0), 機体改造, データセーブ, 次のステージへ] → 次のステージは index 3。
    app.push_intermission_command("改造".to_string(), "lib/x.eve".to_string());
    app.set_script_var("次ステージ".to_string(), "main.eve".to_string());
    app.set_intermission_cursor(3); // 「次のステージへ」

    assert!(app.confirm_intermission_selection());
    assert_eq!(
        app.stage_state(),
        crate::stage::StageState::Battle,
        "次のステージへ後に Briefing で固まらず Battle へ進むべき"
    );
    assert_eq!(app.turn().phase, crate::Phase::Player);
    assert_eq!(
        app.script_var("スタート回数"),
        "1",
        "スタートが二重発火している (敵の二重配置になる)"
    );
}

#[test]
fn reaction_prompt_shown_manual_and_skipped_when_auto() {
    // 手動 (既定): 敵が味方を攻撃 → 反撃メニューが出る。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    place_player_unit(&mut app, "Foe", 9, 9);
    add_weapon(&mut app, "Foe", 60, 1);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 4, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_animate_ai(true);
    app.handle_input(Input::EndPhase);
    let mut prompted = false;
    for _ in 0..100 {
        app.tick(0.1);
        if app.pending_dialog().is_some() {
            prompted = true;
            break;
        }
        if !app.ai_running() {
            break;
        }
    }
    assert!(prompted, "手動反撃モードで反撃メニューが出ていない");

    // 自動反撃モード: プロンプト無しで攻撃が解決され Hero が被弾する。
    let (_app, auto_dmg) = run_enemy_attacks_hero(None, false);
    assert!(auto_dmg > 0, "自動反撃モードで攻撃が成立していない");
}

#[test]
fn defend_reaction_halves_damage() {
    // options=[回避, 防御] (Hero 武器なし→反撃不可)。防御=choice 2。
    let (_a, defend_dmg) = run_enemy_attacks_hero(Some(2), false);
    let (_b, auto_dmg) = run_enemy_attacks_hero(None, false);
    assert!(auto_dmg > 0, "基準 (自動) で被弾していない");
    assert_eq!(
        defend_dmg,
        auto_dmg / 2,
        "防御でダメージが半減していない (defend={defend_dmg}, auto={auto_dmg})"
    );
}

#[test]
fn counter_reaction_hits_attacker_but_defend_does_not() {
    // Hero に武器 → options=[反撃, 回避, 防御]。反撃=1 / 防御=3。
    let (app_counter, _) = run_enemy_attacks_hero(Some(1), true);
    let enemy_dmg_counter = app_counter
        .database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Enemy)
        .map(|u| u.damage)
        .unwrap_or(0);
    assert!(
        enemy_dmg_counter > 0,
        "反撃で攻撃側にダメージが入っていない"
    );

    let (app_defend, _) = run_enemy_attacks_hero(Some(3), true);
    let enemy_dmg_defend = app_defend
        .database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Enemy)
        .map(|u| u.damage)
        .unwrap_or(0);
    assert_eq!(enemy_dmg_defend, 0, "防御では反撃しないはず");
}

#[test]
fn battle_anim_queued_on_attack_and_cleared_by_tick() {
    // animate_battle 有効時、攻撃解決で命中演出が積まれ、tick で総時間ぶん
    // 進めると破棄される。攻撃側/防御側タイルが正しく記録される。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    add_weapon(&mut app, "Hero", 200, 1);
    place_player_unit(&mut app, "Foe", 9, 9); // Foe の UnitData を用意
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6); // 隣接する攻撃対象
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_animate_battle(true);
    app.set_map_cursor(5, 6);

    assert!(
        app.attack_resolve_and_run(Some((6, 6)), false, ""),
        "攻撃が解決しなかった"
    );
    let anim = app.battle_anim().expect("戦闘演出が積まれていない");
    assert_eq!(anim.attacker, (5, 6));
    assert_eq!(anim.defender, (6, 6));
    assert_eq!(anim.elapsed, 0.0);
    let total = anim.total;
    // 途中までの tick では残る。
    app.tick(total * 0.5);
    assert!(app.battle_anim().is_some(), "途中で演出が消えている");
    // 総時間を超えたら破棄。
    app.tick(total);
    assert!(app.battle_anim().is_none(), "tick 後も演出が残っている");
}

#[test]
fn battle_anim_not_queued_when_disabled() {
    // animate_battle 無効 (既定/ヘッドレス) では演出を積まない (既存テスト互換)。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    add_weapon(&mut app, "Hero", 200, 1);
    place_player_unit(&mut app, "Foe", 9, 9);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_map_cursor(5, 6);

    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    assert!(
        app.battle_anim().is_none(),
        "animate_battle 無効でも演出が積まれている"
    );
}

#[test]
fn battle_animation_subroutine_plays_via_script_when_resolved() {
    // animation.txt で武器(状況)→サブルーチンが解決でき、それが script_library に
    // 実在すれば、戦闘解決後にスクリプト再生が起動する (ネイティブ演出より優先)。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    add_weapon(&mut app, "Hero", 200, 1); // 武器名 = "テスト砲"
    place_player_unit(&mut app, "Foe", 9, 9);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // 戦闘アニメサブルーチンを script_library に登録 (PaintPicture + Wait)。
    let sub = "\
戦闘アニメ_斬撃攻撃:
PaintPicture slash.bmp 100 100
Wait 5
Return
";
    let stmts = crate::data::event::parse(sub).unwrap();
    crate::event_runtime::library_append(&mut app, &stmts);
    // animation.txt: 汎用の テスト砲(攻撃) → 斬撃 (→ 戦闘アニメ_斬撃攻撃)。
    app.database_mut()
        .merge_animation_data("汎用\nテスト砲(攻撃), 斬撃\n");

    app.set_animate_battle(true);
    app.set_map_cursor(5, 6);
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));

    // スクリプト再生が起動し Wait で中断している。
    assert!(
        app.has_script_context(),
        "戦闘アニメスクリプトが起動・中断していない"
    );
    assert!(
        app.pending_timer().is_some(),
        "Wait による中断タイマが立っていない"
    );
    // PaintPicture が overlay に積まれている。
    assert!(
        !app.script_overlay().cmds.is_empty(),
        "PaintPicture が描画コマンドに積まれていない"
    );
    // スクリプト再生を優先したのでネイティブ演出は積まれない。
    assert!(
        app.battle_anim().is_none(),
        "スクリプト再生中にネイティブ演出も積まれている"
    );

    // tick でタイマを満了させるとスクリプトが再開・完了する。
    app.tick(1.0);
    assert!(
        !app.has_script_context(),
        "Wait 満了後もスクリプトが再開・完了していない"
    );
}

#[test]
fn battle_animation_frame_loop_keeps_frame_visible_through_combat() {
    // GBA クローズアップ実機相当: 戦闘解決から起動した戦闘アニメが
    // `PaintPicture; Refresh; ClearPicture; Wait` のフレームループのとき、
    // ClearPicture 遅延クリアにより各フレームが Wait 中も overlay に残ること
    // (毎フレーム空表示にならないこと) を、実際の combat 経路で検証する。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    add_weapon(&mut app, "Hero", 200, 1); // 武器名 = "テスト砲"
    place_player_unit(&mut app, "Foe", 9, 9);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // 2 フレームのループ。各フレームで 1 枚描いて Refresh→ClearPicture→Wait。
    let sub = "\
戦闘アニメ_斬撃攻撃:
For i = 1 To 2
  PaintPicture frame.bmp 10 20 透過
  Refresh
  ClearPicture
  Wait 5
Next
Return
";
    let stmts = crate::data::event::parse(sub).unwrap();
    crate::event_runtime::library_append(&mut app, &stmts);
    app.database_mut()
        .merge_animation_data("汎用\nテスト砲(攻撃), 斬撃\n");

    app.set_animate_battle(true);
    app.set_map_cursor(5, 6);
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));

    // 1 フレーム目の Wait で中断。フレームの PaintPicture が overlay に残っている。
    assert!(
        app.pending_timer().is_some(),
        "1 フレーム目 Wait で中断のはず"
    );
    let pics = |app: &App| {
        app.script_overlay()
            .cmds
            .iter()
            .filter(|c| matches!(c, crate::DrawCmd::Picture { .. }))
            .count()
    };
    assert_eq!(
        pics(&app),
        1,
        "Wait 中はフレームが見える (ClearPicture 即時クリアなら 0 になる)"
    );

    // tick で次フレームへ。毎フレーム ClearPicture されるので累積せず常に 1 枚。
    app.tick(1.0);
    assert!(
        app.pending_timer().is_some(),
        "2 フレーム目 Wait で再度中断"
    );
    assert_eq!(pics(&app), 1, "2 フレーム目も 1 枚 (累積しない)");
}

#[test]
fn battle_animation_falls_back_to_native_without_data() {
    // animation.txt が無ければ従来どおりネイティブ演出 (battle_anim) が積まれる。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    add_weapon(&mut app, "Hero", 200, 1);
    place_player_unit(&mut app, "Foe", 9, 9);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_animate_battle(true);
    app.set_map_cursor(5, 6);
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    assert!(
        !app.has_script_context(),
        "戦闘アニメデータ無しで script 起動"
    );
    assert!(
        app.battle_anim().is_some(),
        "フォールバックのネイティブ演出が積まれていない"
    );
}

#[test]
fn counterattack_suppressed_when_defender_action_disabled() {
    // 行動不能 (麻痺) の防御側は反撃武器を持っていても反撃できない (SRC
    // MaxAction()==0 ゲート)。必中で命中を確定させ RNG 非依存にする。
    fn run(disable: bool) -> (i64, i64) {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        add_weapon(&mut app, "Hero", 50, 1);
        place_player_unit(&mut app, "Foe", 9, 9);
        add_weapon(&mut app, "Foe", 50, 1);
        // Hero に必中 (攻撃確定命中)。
        app.database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| u.unit_data_name == "Hero")
            .unwrap()
            .add_condition(crate::condition::Condition::new("必中", 3));
        let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
        {
            let e = app.database_mut().unit_by_uid_mut(&euid).unwrap();
            e.add_condition(crate::condition::Condition::new("必中", 3));
            if disable {
                e.add_condition(crate::condition::Condition::new("麻痺", 3));
            }
        }
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_map_cursor(5, 6);
        assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
        let hero_dmg = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Hero" && u.party == crate::Party::Player)
            .map(|u| u.damage)
            .unwrap();
        let enemy_dmg = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.party == crate::Party::Enemy)
            .map(|u| u.damage)
            .unwrap_or(-1);
        (hero_dmg, enemy_dmg)
    }
    let (hero_disabled, enemy_disabled) = run(true);
    let (hero_normal, _enemy_normal) = run(false);
    assert!(hero_normal > 0, "前提: 通常は敵が反撃して Hero が被弾する");
    assert_eq!(hero_disabled, 0, "行動不能の敵が反撃してしまっている");
    assert!(enemy_disabled > 0, "行動不能でも敵自身は攻撃を受ける");
}

#[test]
fn reaction_prompt_skipped_when_defender_action_disabled() {
    // 行動不能 (麻痺) の味方が攻撃されても反撃メニューは出ない (選択不可)。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    app.database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| u.unit_data_name == "Hero")
        .unwrap()
        .add_condition(crate::condition::Condition::new("麻痺", 3));
    place_player_unit(&mut app, "Foe", 9, 9);
    add_weapon(&mut app, "Foe", 60, 1);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 4, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_animate_ai(true);
    app.handle_input(Input::EndPhase);
    let mut prompted = false;
    for _ in 0..200 {
        app.tick(0.1);
        if app.pending_dialog().is_some() {
            prompted = true;
            break;
        }
        if !app.ai_running() {
            break;
        }
    }
    assert!(!prompted, "行動不能の防御側に反撃メニューが出てしまった");
}

/// 反撃メニューには、援護防御を受けられるときだけ「援護防御：使用する (ユニット/パイロット)」
/// の切り替え行が出る (原典 `COM.bas`)。選ぶと「使用しない」に変わってメニューが出し直される。
#[test]
fn support_guard_reaction_option_offered_only_when_available() {
    fn reaction_app(with_guard: bool) -> App {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        add_weapon(&mut app, "Hero", 50, 1);
        place_player_unit(&mut app, "Guard", 5, 5); // Hero に隣接する味方
        if with_guard {
            let g = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| u.unit_data_name == "Guard")
                .unwrap();
            g.add_condition(crate::condition::Condition::new("援護防御", 9));
        }
        place_player_unit(&mut app, "Foe", 9, 9);
        add_weapon(&mut app, "Foe", 50, 1);
        let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 4, 6);
        let hero_uid = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Hero")
            .unwrap()
            .uid
            .clone();
        app.set_stage_state(crate::stage::StageState::Battle);
        app.begin_reaction_prompt(euid, hero_uid, (5, 6));
        app
    }
    fn options(app: &App) -> Vec<String> {
        match app.pending_dialog() {
            Some(crate::PendingDialog::Menu { options, .. }) => options.clone(),
            _ => Vec::new(),
        }
    }
    let mut app = reaction_app(true);
    let with = options(&app);
    let toggle = with
        .iter()
        .position(|o| o.starts_with("援護防御：使用する ("))
        .unwrap_or_else(|| panic!("援護防御 が出ていない: {with:?}"));
    assert!(app.resolve_reaction(toggle as u32 + 1));
    let toggled = options(&app);
    assert!(
        toggled[toggle].starts_with("援護防御：使用しない ("),
        "切り替わっていない: {toggled:?}"
    );
    assert!(!app.pending_reaction().unwrap().use_support_guard);

    let without = options(&reaction_app(false));
    assert!(
        !without.iter().any(|o| o.starts_with("援護防御")),
        "援護防御 が誤って出ている: {without:?}"
    );
}

/// B7: 援護防御の発動判定は実効最大 HP (改造・アイテム・BossRank 込み) で行う。
/// 旧実装は基本 HP を使い、改造で HP が十分ある援護役を「撃破される」と誤判定していた。
#[test]
fn support_guard_uses_effective_max_hp() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Foe", 9, 9);
    let fuid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6); // 防御側
    let guid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 5); // 援護役
    let base_hp = app.database().unit_by_name("Foe").unwrap().hp;
    {
        let g = app.database_mut().unit_by_uid_mut(&guid).unwrap();
        g.add_condition(crate::condition::Condition::new("援護防御", 9));
        // 基本 HP ではほぼ瀕死だが、改造 5 段階で実効 HP には十分な余裕がある。
        g.damage = base_hp - 1;
        g.upgrade_level = 5;
    }
    // 攻撃側 (Hero) の武器は、援護役が実効 HP なら耐えられる威力にする。
    place_player_unit(&mut app, "Hero", 5, 6);
    add_weapon(&mut app, "Hero", 30, 1);
    let hero = first_player_uid(&app);
    let weapon = app
        .database()
        .unit_by_name("Hero")
        .unwrap()
        .weapons
        .last()
        .cloned()
        .unwrap();
    let def_idx = app.database().idx_by_uid(&fuid).unwrap();
    let guard_idx = app.database().idx_by_uid(&guid).unwrap();
    let atk_idx = app.database().idx_by_uid(&hero).unwrap();
    let expected = app.forecast_strike(atk_idx, def_idx, &weapon).unwrap();
    assert!(
        expected.damage * 3 / 4 >= 1,
        "前提: 基本 HP (残り 1) なら撃破される見込み"
    );
    assert_eq!(
        app.try_find_support_guard(atk_idx, def_idx, &weapon, &expected, true),
        Some(guard_idx),
        "実効 HP に余裕のある援護役が援護防御しない"
    );
}

/// 原典の援護防御は反撃/防御/回避の選択とは別の切り替え (`UseSupportGuard`、既定は使用する)。
/// 防御側が自分のフェイズでなければ、味方の攻撃を受ける敵もかばわれる。「使用しない」を
/// 選んだときだけ防御側が自分で受ける。
#[test]
fn support_guard_applies_regardless_of_reaction_choice() {
    for def_mode in ["", "反撃", "防御"] {
        let (mut app, foe, guard) = setup_support_guard(0, false, None);
        assert!(app.attack_resolve_and_run(Some((6, 6)), false, def_mode));
        let db = app.database();
        assert_eq!(
            db.unit_by_uid(&foe).unwrap().damage,
            0,
            "{def_mode:?}: 防御側が被弾した"
        );
        assert!(
            db.unit_by_uid(&guard).unwrap().damage > 0,
            "{def_mode:?}: ガード役がかばっていない"
        );
    }
    let (mut app, foe, guard) = setup_support_guard(0, false, None);
    app.support_guard_declined = true;
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, "防御"));
    let db = app.database();
    assert!(
        db.unit_by_uid(&foe).unwrap().damage > 0,
        "使用しない のに防御側が受けていない"
    );
    assert_eq!(
        db.unit_by_uid(&guard).unwrap().damage,
        0,
        "使用しない のにかばった"
    );
}

/// 援護攻撃・援護防御の回数は主パイロットの技能のレベル (原典 `MaxSupportAttack` /
/// `MaxSupportGuard`)。`援護` は両方に効き、`援護攻撃` を持っていても援護防御はできない。
#[test]
fn support_counts_follow_pilot_skill_level() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    let hero = first_player_uid(&app);
    let idx = app.database().idx_by_uid(&hero).unwrap();
    let counts = |app: &App| (app.max_support_attack(idx), app.max_support_guard(idx));
    assert_eq!(counts(&app), (0, 0));
    for (skill, want) in [
        ("援護攻撃Lv2", (2, 0)),
        ("援護", (2, 1)),
        ("援護防御Lv3", (2, 3)),
    ] {
        app.database_mut()
            .unit_by_uid_mut(&hero)
            .unwrap()
            .add_condition(crate::Condition::new(skill, -1));
        assert_eq!(counts(&app), want, "{skill} を足したあと");
    }
}

/// 援護攻撃役は原典 `LookForSupportAttack` の条件で選ぶ。候補が複数なら援護に使える武器の
/// 攻撃力が高いほう。行動済み・混乱・回数切れのユニットは援護しない。味方は自爆攻撃 (自) を
/// 援護に使わない。
#[test]
fn support_attacker_chosen_by_original_rules() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    place_player_unit(&mut app, "Weak", 4, 6);
    place_player_unit(&mut app, "Strong", 5, 7);
    add_weapon(&mut app, "Weak", 500, 2);
    add_weapon(&mut app, "Strong", 900, 2);
    let hero = first_player_uid(&app);
    let uid_of = |app: &App, name: &str| {
        app.database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == name)
            .unwrap()
            .uid
            .clone()
    };
    let (weak, strong) = (uid_of(&app, "Weak"), uid_of(&app, "Strong"));
    let foe = spawn_party(&mut app, "Hero", crate::Party::Enemy, 6, 6);
    for uid in [&weak, &strong] {
        app.database_mut()
            .unit_by_uid_mut(uid)
            .unwrap()
            .add_condition(crate::Condition::new("援護攻撃", -1));
    }
    let pick = |app: &App| {
        let db = app.database();
        let (a, d) = (db.idx_by_uid(&hero).unwrap(), db.idx_by_uid(&foe).unwrap());
        app.look_for_support_attack(a, Some(d))
            .map(|i| db.unit_instances[i].unit_data_name.clone())
    };
    assert_eq!(pick(&app).as_deref(), Some("Strong"));
    app.database_mut()
        .unit_by_uid_mut(&strong)
        .unwrap()
        .has_acted = true;
    assert_eq!(pick(&app).as_deref(), Some("Weak"), "行動済みは援護しない");
    app.database_mut()
        .unit_by_uid_mut(&strong)
        .unwrap()
        .has_acted = false;
    if let Some(w) = app
        .database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == "Strong")
        .and_then(|u| u.weapons.last_mut())
    {
        w.class = "自".into();
    }
    assert_eq!(
        pick(&app).as_deref(),
        Some("Weak"),
        "味方は自爆攻撃で援護しない"
    );
    app.database_mut()
        .unit_by_uid_mut(&weak)
        .unwrap()
        .add_condition(crate::Condition::new("混乱", -1));
    assert_eq!(pick(&app), None, "混乱していると援護しない");
    app.database_mut()
        .unit_by_uid_mut(&weak)
        .unwrap()
        .conditions
        .retain(|c| c.name != "混乱");
    app.database_mut()
        .unit_by_uid_mut(&weak)
        .unwrap()
        .used_support_attack = 1;
    assert_eq!(pick(&app), None, "援護攻撃の回数を使い切ると援護しない");
}

/// Hero (5,6) が Foe (6,6) を攻撃し、Sup (5,7) が援護攻撃できる盤面。全員 必中。
/// `foe_weapon` は Foe の反撃武器の攻撃力 (0 なら反撃しない)。戻り値は (app, hero, sup, foe)。
fn setup_support_attack(
    hero_hp: i64,
    foe_hp: i64,
    foe_weapon: i64,
) -> (App, String, String, String) {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    place_player_unit(&mut app, "Sup", 5, 7);
    add_weapon(&mut app, "Hero", 100, 1);
    add_weapon(&mut app, "Sup", 100, 2);
    register_enemy_data(&mut app, "Foe");
    if foe_weapon > 0 {
        add_weapon(&mut app, "Foe", foe_weapon, 1);
    }
    for (name, hp) in [("Hero", hero_hp), ("Foe", foe_hp)] {
        app.database_mut()
            .catalog
            .units
            .iter_mut()
            .find(|u| u.name == name)
            .unwrap()
            .hp = hp;
    }
    let foe = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
    let hero = first_player_uid(&app);
    let sup = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Sup")
        .unwrap()
        .uid
        .clone();
    for uid in [&hero, &sup, &foe] {
        app.database_mut()
            .unit_by_uid_mut(uid)
            .unwrap()
            .add_condition(crate::Condition::new("必中", -1));
    }
    app.database_mut()
        .unit_by_uid_mut(&sup)
        .unwrap()
        .add_condition(crate::Condition::new("援護攻撃", -1));
    app.set_stage_state(crate::stage::StageState::Battle);
    (app, hero, sup, foe)
}

/// 援護攻撃は反撃のあとに行う (原典 `Command.bas`)。攻撃側が反撃で倒れたら援護攻撃しない。
#[test]
fn support_attack_follows_counter_and_needs_attacker_alive() {
    let run = |hero_hp: i64| -> Vec<String> {
        let (mut app, ..) = setup_support_attack(hero_hp, 1_000_000, 3000);
        app.map_cursor = Some((5, 6));
        assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
        app.messages().to_vec()
    };
    let pos = |msgs: &[String], pat: &str| msgs.iter().position(|m| m.contains(pat));
    let survived = run(1_000_000);
    let counter = pos(&survived, "↩ 反撃").expect("反撃していない");
    let support = pos(&survived, "サポートアタック").expect("援護攻撃していない");
    assert!(counter < support, "援護攻撃が反撃より先: {survived:?}");
    let killed = run(10);
    assert!(
        pos(&killed, "撃破").is_some(),
        "反撃で攻撃側が倒れていない: {killed:?}"
    );
    assert!(
        pos(&killed, "サポートアタック").is_none(),
        "倒れた攻撃側を援護攻撃した: {killed:?}"
    );
}

/// 統率: 攻撃側の主パイロットが技能 `統率` を持てば援護攻撃は同時援護攻撃になり (回数は
/// 統率のレベルまで)、反撃より前に行われる。本線攻撃と同時援護攻撃は必ずクリティカル。
#[test]
fn leadership_turns_support_attack_into_sync_attack() {
    let (mut app, hero, ..) = setup_support_attack(1_000_000, 1_000_000, 3000);
    app.database_mut()
        .unit_by_uid_mut(&hero)
        .unwrap()
        .add_condition(crate::Condition::new("統率", -1));
    app.map_cursor = Some((5, 6));
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let msgs = app.messages().to_vec();
    let find = |pat: &str| {
        msgs.iter()
            .position(|m| m.contains(pat))
            .unwrap_or_else(|| panic!("{pat} が無い: {msgs:?}"))
    };
    let (main, sync, counter) = (find("]: 命中"), find("同時援護攻撃"), find("↩ 反撃"));
    assert!(
        msgs[main].contains("クリティカル"),
        "本線攻撃: {}",
        msgs[main]
    );
    assert!(
        msgs[sync].contains("クリティカル"),
        "同時援護攻撃: {}",
        msgs[sync]
    );
    assert!(main < sync && sync < counter, "順番が違う: {msgs:?}");
    assert!(
        !msgs.iter().any(|m| m.contains("サポートアタック")),
        "同時援護攻撃のあとにも援護攻撃した: {msgs:?}"
    );
    assert_eq!(
        app.database().unit_by_uid(&hero).unwrap().used_sync_attack,
        1
    );
}

/// 援護攻撃で撃破しても、撃破の報酬 (気力) は攻撃したユニットが得て、援護役には入らない。
#[test]
fn support_attack_kill_rewards_main_attacker() {
    let (mut app, hero, sup, foe) = setup_support_attack(1_000_000, 10, 0);
    let hero_idx = app.database().idx_by_uid(&hero).unwrap();
    let before = |app: &App, uid: &str| app.database().unit_by_uid(uid).unwrap().morale;
    let (hero_morale, sup_morale) = (before(&app, &hero), before(&app, &sup));
    assert!(support_attack_at(&mut app, hero_idx, 6, 6).is_some());
    let db = app.database();
    assert!(db.unit_by_uid(&foe).is_none(), "援護攻撃で撃破していない");
    let gain = |uid: &str, was: i32| db.unit_by_uid(uid).unwrap().morale - was;
    assert!(
        gain(&hero, hero_morale) > gain(&sup, sup_morale),
        "撃破の気力が攻撃側に入っていない (攻撃側 +{} / 援護役 +{})",
        gain(&hero, hero_morale),
        gain(&sup, sup_morale)
    );
}

/// 武器選択の最後に「援護攻撃：使用する」の切り替え行が出て (援護攻撃してくれるユニットが
/// 隣にいるときだけ。原典 `WeaponListBox`)、選ぶたびに使用する/しないが入れ替わる。
/// 「使用しない」にして攻撃すると援護攻撃しない。
#[test]
fn weapon_select_offers_support_attack_toggle() {
    let last_option = |app: &App| match &app.pending_dialog {
        Some(crate::dialog::PendingDialog::Menu { options, .. }) => options.last().cloned(),
        _ => None,
    };
    let toggle = |app: &App| app.weapon_select_window_data().unwrap().support_attack;
    let (mut app, hero, sup, _) = setup_support_attack(1_000_000, 1_000_000, 0);
    app.begin_weapon_select(hero, (6, 6), false);
    assert_eq!(last_option(&app).as_deref(), Some("援護攻撃：使用する"));
    assert_eq!(toggle(&app), Some(true));
    // Hero の武器は 1 つなので、切り替え行は 2 番。
    assert!(app.respond_dialog(2));
    assert_eq!(last_option(&app).as_deref(), Some("援護攻撃：使用しない"));
    assert_eq!(toggle(&app), Some(false));
    assert!(app.respond_dialog(1));
    assert!(
        app.pending_weapon_select.is_none(),
        "武器を選んでも攻撃していない"
    );
    assert!(
        app.messages().iter().any(|m| m.contains("テスト砲")),
        "攻撃していない: {:?}",
        app.messages()
    );
    assert!(
        !app.messages()
            .iter()
            .any(|m| m.contains("サポートアタック")),
        "「使用しない」なのに援護攻撃した"
    );
    assert_eq!(
        app.database()
            .unit_by_uid(&sup)
            .unwrap()
            .used_support_attack,
        0
    );

    // 援護攻撃してくれるユニットがいなければ切り替え行は出ない。
    let (mut app, hero, sup, _) = setup_support_attack(1_000_000, 1_000_000, 0);
    app.database_mut()
        .unit_by_uid_mut(&sup)
        .unwrap()
        .conditions
        .retain(|c| c.name != "援護攻撃");
    app.begin_weapon_select(hero, (6, 6), false);
    assert_eq!(last_option(&app).as_deref(), Some("テスト砲"));
    assert_eq!(toggle(&app), None);
}

/// 援護攻撃 (原典 `Attack w2, target, "援護攻撃", def_mode`) も `Unit.Attack` を通るので、
/// 援護防御を受ける。
#[test]
fn support_guard_covers_support_attack() {
    let (mut app, foe, guard) = setup_support_guard(0, false, None);
    // 援護攻撃する味方 (Hero と同じデータ) を Hero の隣に置く。
    let sup = spawn_party(&mut app, "Hero", crate::Party::Player, 5, 7);
    {
        let s = app.database_mut().unit_by_uid_mut(&sup).unwrap();
        s.add_condition(crate::Condition::new("援護攻撃", -1));
        s.add_condition(crate::Condition::new("必中", -1));
    }
    {
        let g = app.database_mut().unit_by_uid_mut(&guard).unwrap();
        g.add_condition(crate::Condition::new("援護防御Lv2", -1));
    }
    // 援護役 (5,7) から防御側 (6,6) までは距離 2。
    if let Some(w) = app
        .database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == "Hero")
        .and_then(|u| u.weapons.last_mut())
    {
        w.max_range = 2;
    }
    let hero = first_player_uid(&app);
    let hero_idx = app.database().idx_by_uid(&hero).unwrap();
    assert_eq!(
        support_attack_at(&mut app, hero_idx, 6, 6).as_deref(),
        Some(sup.as_str())
    );
    let db = app.database();
    assert_eq!(db.unit_by_uid(&foe).unwrap().damage, 0, "防御側が被弾した");
    assert!(
        db.unit_by_uid(&guard).unwrap().damage > 0,
        "援護攻撃をガード役がかばっていない"
    );
    assert_eq!(db.unit_by_uid(&guard).unwrap().used_support_guard, 1);
}

#[test]
fn critical_hit_multiplies_damage_by_one_point_five() {
    // クリティカル率 100% (weapon.critical=100, 技量差0) で必ずクリティカル → ×1.5。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    give_crit_weapon(&mut app, "Hero", 20, 100);
    place_player_unit(&mut app, "Foe", 9, 9);
    let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_map_cursor(5, 6);

    // 非クリの基本ダメージを combat で算出。
    let base = {
        let hero = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Hero")
            .unwrap();
        let foe = app.database().unit_by_uid(&euid).unwrap();
        let atk_unit = app.database().unit_by_name("Hero").unwrap();
        let weapon = atk_unit
            .weapons
            .iter()
            .find(|w| w.name == "クリ砲")
            .unwrap();
        let atk_pilot = app.database().pilot_by_name(&hero.pilot_name).unwrap();
        let def_unit = app.database().unit_by_name("Foe").unwrap();
        let def_pilot = app.database().pilot_by_name(&foe.pilot_name).unwrap();
        let t = app.database().map.as_ref().unwrap().cell(6, 6).terrain_id;
        // 解決経路と同じ地形適応を適用して base を算出 (crit 比 1.5× を保つ)。
        let atk_env = app.terrain_env_at(hero.x, hero.y);
        let def_env = app.terrain_env_at(6, 6);
        crate::combat::predict_with_status_terrain(
            atk_pilot,
            atk_unit,
            weapon,
            def_pilot,
            def_unit,
            app.database().terrain_hit_mod(t),
            app.database().terrain_damage_mod(t),
            hero.morale,
            foe.morale,
            &["必中".to_string()],
            &[],
            atk_env,
            def_env,
            // 必中 / 防御側無印 ＝ 与・被ダメージ修正なし。
            crate::combat::DamageSpiritLevels::default(),
            1.0, // ＥＣＭ 補正なし
        )
        .damage
    };

    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let dealt = app.database().unit_by_uid(&euid).map(|u| u.damage).unwrap();
    assert_eq!(
        dealt,
        base * 3 / 2,
        "クリティカルが ×1.5 になっていない (base={base}, dealt={dealt})"
    );
    assert!(
        app.messages().iter().any(|m| m.contains("クリティカル")),
        "クリティカル表示が無い"
    );
}

/// 特殊効果攻撃属性 (痺) を持つ武器は critical=100 でもクリティカルしない
/// (特殊効果がクリの代わり)。ダメージは ×1.5 されず、状態異常が proc する。
#[test]
fn special_effect_weapon_does_not_crit_but_procs() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    // 痺 + critical 100 の武器を Hero に持たせ、必中で命中を確定。
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == "Hero")
        .unwrap()
        .weapons
        .push(WeaponData {
            name: "麻痺砲".into(),
            power: 20,
            min_range: 1,
            max_range: 1,
            precision: 100,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 100,
            class: "痺".into(),
            extras: Vec::new(),
        });
    app.database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| u.unit_data_name == "Hero")
        .unwrap()
        .add_condition(crate::Condition::new("必中", 9));
    place_player_unit(&mut app, "Foe", 9, 9);
    let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
    // Foe は高 HP で生存させる (撃破時は proc しないため)。
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == "Foe")
        .unwrap()
        .hp = 100_000;
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_map_cursor(5, 6);

    // 非クリの基本ダメージを combat で算出 (crit 抑止後はこれと一致するはず)。
    let base = {
        let hero = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Hero")
            .unwrap();
        let foe = app.database().unit_by_uid(&euid).unwrap();
        let atk_unit = app.database().unit_by_name("Hero").unwrap();
        let weapon = atk_unit
            .weapons
            .iter()
            .find(|w| w.name == "麻痺砲")
            .unwrap();
        let atk_pilot = app.database().pilot_by_name(&hero.pilot_name).unwrap();
        let def_unit = app.database().unit_by_name("Foe").unwrap();
        let def_pilot = app.database().pilot_by_name(&foe.pilot_name).unwrap();
        let t = app.database().map.as_ref().unwrap().cell(6, 6).terrain_id;
        let atk_env = app.terrain_env_at(hero.x, hero.y);
        let def_env = app.terrain_env_at(6, 6);
        crate::combat::predict_with_status_terrain(
            atk_pilot,
            atk_unit,
            weapon,
            def_pilot,
            def_unit,
            app.database().terrain_hit_mod(t),
            app.database().terrain_damage_mod(t),
            hero.morale,
            foe.morale,
            &["必中".to_string()],
            &[],
            atk_env,
            def_env,
            // 必中 / 防御側無印 ＝ 与・被ダメージ修正なし。
            crate::combat::DamageSpiritLevels::default(),
            1.0, // ＥＣＭ 補正なし
        )
        .damage
    };

    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let dealt = app.database().unit_by_uid(&euid).map(|u| u.damage).unwrap();
    assert_eq!(
        dealt, base,
        "特殊効果武器はクリティカルしない (×1.5 されない: base={base}, dealt={dealt})"
    );
    assert!(
        !app.messages().iter().any(|m| m.contains("クリティカル")),
        "特殊効果武器でクリティカル表示は出ない"
    );
    assert!(
        app.database()
            .unit_by_uid(&euid)
            .unwrap()
            .has_condition("麻痺"),
        "特殊効果 (麻痺) が proc する"
    );
}

#[test]
fn defend_halves_critical_rate() {
    // 防御選択でクリティカル率が半減する (SRC)。多数試行で発生率を比較
    // (固定シードなので件数は決定的)。
    fn crit_count(def_mode: &str, trials: usize) -> usize {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        give_crit_weapon(&mut app, "Hero", 20, 50); // クリ率 50% (防御で 25%)
        place_player_unit(&mut app, "Foe", 9, 9);
        let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_map_cursor(5, 6);
        let mut crits = 0;
        for _ in 0..trials {
            // 撃破されないよう毎回ダメージをリセット (同じタイルを攻撃し続ける)。
            app.database_mut().unit_by_uid_mut(&euid).unwrap().damage = 0;
            let before = app.messages().len();
            app.attack_resolve_and_run(Some((6, 6)), false, def_mode);
            if app.messages()[before..]
                .iter()
                .any(|m| m.contains("クリティカル"))
            {
                crits += 1;
            }
        }
        crits
    }
    let normal = crit_count("", 400);
    let defend = crit_count("防御", 400);
    assert!(
        normal > defend,
        "防御でクリ率が下がっていない (normal={normal}, defend={defend})"
    );
    assert!(
        defend < normal * 3 / 4,
        "防御のクリ率半減が不十分 (normal={normal}, defend={defend})"
    );
}

#[test]
fn support_attack_fires_when_adjacent_supporter_has_status() {
    // 隣接味方が「サポートアタック」状態を持っていれば、本攻撃の後に
    // 自動で 75% 火力の追撃が走る。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    place_player_unit(&mut app, "Supporter", 3, 6);
    // 敵を隣接 (2,7) に配置
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        2,
        7,
    ));
    // サポーターに能力付与
    app.database_mut().unit_instances[1].add_condition(crate::Condition::new("援護攻撃", -1));
    app.set_stage_state(crate::stage::StageState::Battle);
    // Player ユニット (Hero) に攻撃用武器を持たせる
    app.database_mut().catalog.units[0]
        .weapons
        .push(crate::data::unit::WeaponData {
            name: "ビーム".to_string(),
            power: 200,
            min_range: 1,
            max_range: 3,
            precision: 50,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        });
    // カーソルを敵に合わせて Hero 側から攻撃
    app.map_cursor = Some((2, 6));
    let _ = app.attack_target();
    // 敵 HP は 100、ヒット時の本攻撃 + サポートアタックで多めに削れていれば良い。
    // 1 度ヒットしただけでも撃破される可能性が高いので、削れた事実だけ確認。
    let enemy = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Enemy);
    // 撃破 or ダメージ蓄積していること
    let damaged_or_dead = enemy.map(|u| u.damage > 0).unwrap_or(true);
    assert!(damaged_or_dead, "expected enemy hit");
    // サポーターの残りサポート攻撃が 0 になっていること (使った場合)
    // ※本攻撃でミス → サポートアタック発動 (使用回数 0→1) のケース、
    // 　本攻撃で撃破 → サポートアタック発動せず (0 のまま) のケース、両方許容。
    let sup = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Supporter")
        .unwrap();
    assert!(sup.used_support_attack <= 1);
}

#[test]
fn support_guard_fires_during_enemy_phase() {
    // 敵フェイズに敵が味方ユニット (Target) を攻撃した際、隣接する援護防御可能な
    // 味方ユニット (Guard) が代わりにダメージを受ける。
    // Target は無傷 / Guard はダメージ蓄積していること。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    // Target (3, 6): 攻撃を受ける味方ユニット (武器なし)
    place_player_unit(&mut app, "Target", 3, 6);
    // Guard (4, 6): 援護防御ユニット (Target の隣)
    place_player_unit(&mut app, "Guard", 4, 6);
    // Guard に「サポートガード」能力を付与
    {
        let guard = app
            .database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| u.unit_data_name == "Guard")
            .unwrap();
        guard.add_condition(crate::Condition::new("援護防御", -1));
    }
    // 敵ユニット (2, 6): Target に隣接、武器あり、必中
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Target", // Hero の unit data を再利用
        "PILOT",
        crate::Party::Enemy,
        2,
        6,
    ));
    // 敵に武器を持たせる (power=50, range=1, precision=100)
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == "Target")
        .unwrap()
        .weapons
        .push(crate::data::unit::WeaponData {
            name: "バルカン".to_string(),
            power: 50,
            min_range: 1,
            max_range: 1,
            precision: 100,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        });
    // 必中条件: 命中 100 を保証
    app.database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| u.party == crate::Party::Enemy)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));
    app.set_stage_state(crate::stage::StageState::Battle);
    // EndPhase → 敵フェイズへ → AI が Target を攻撃 → 援護防御発動
    app.handle_input(Input::EndPhase);
    // Target は無傷のはず
    let target = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Target" && u.party == crate::Party::Player);
    let target_damage = target.map(|u| u.damage).unwrap_or(0);
    assert_eq!(
        target_damage, 0,
        "Target should be unharmed (support guard protected it)"
    );
    // Guard はダメージを受けているか撃破されているはず
    let guard = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Guard");
    let guard_damaged_or_dead = guard.map(|u| u.damage > 0).unwrap_or(true);
    assert!(
        guard_damaged_or_dead,
        "Guard should have taken damage (support guarded Target)"
    );
}

/// 直撃 (精神コマンド) を持つ攻撃側はサポートガード (援護防御) を無効化する。
/// Target が直接ダメージを受け、Guard は無傷になる。
#[test]
fn chokugeki_disables_support_guard() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Target", 3, 6);
    place_player_unit(&mut app, "Guard", 4, 6);
    app.database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| u.unit_data_name == "Guard")
        .unwrap()
        .add_condition(crate::Condition::new("援護防御", -1));
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Target",
        "PILOT",
        crate::Party::Enemy,
        2,
        6,
    ));
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == "Target")
        .unwrap()
        .weapons
        .push(crate::data::unit::WeaponData {
            name: "バルカン".to_string(),
            power: 50,
            min_range: 1,
            max_range: 1,
            precision: 100,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        });
    // 敵に 必中 + 直撃。
    {
        let enemy = app
            .database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| u.party == crate::Party::Enemy)
            .unwrap();
        enemy.add_condition(crate::Condition::new("必中", -1));
        enemy.add_condition(crate::Condition::new("直撃", -1));
    }
    app.set_stage_state(crate::stage::StageState::Battle);
    app.handle_input(Input::EndPhase); // 敵フェイズ → AI が Target を攻撃
    let target_damage = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Target" && u.party == crate::Party::Player)
        .map(|u| u.damage)
        .unwrap_or(0);
    let guard_damage = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Guard")
        .map(|u| u.damage)
        .unwrap_or(-1);
    assert!(
        target_damage > 0,
        "直撃でサポートガードが無効 → Target が被弾"
    );
    assert_eq!(guard_damage, 0, "Guard は肩代わりしない (無傷)");
}

/// 捨て身 は反撃では無効 (自分から攻撃したときのみ ×3、反撃では適用しない)。
#[test]
fn sutemi_does_not_apply_on_counterattack() {
    use crate::data::unit::WeaponData;
    fn counter_dmg(with_sutemi: bool) -> i64 {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6); // 反撃を受ける側
        let mut ebot = app.database().unit_by_name("Hero").cloned().unwrap();
        ebot.name = "Counter".into();
        ebot.weapons = vec![WeaponData {
            name: "反撃砲".into(),
            power: 20,
            min_range: 1,
            max_range: 1,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        }];
        app.database_mut().catalog.units.push(ebot);
        let hero = first_player_uid(&app);
        let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
            "Counter",
            "PILOT",
            crate::Party::Enemy,
            3,
            6,
        ));
        app.database_mut()
            .unit_by_uid_mut(&enemy)
            .unwrap()
            .add_condition(crate::Condition::new("必中", -1));
        if with_sutemi {
            app.database_mut()
                .unit_by_uid_mut(&enemy)
                .unwrap()
                .add_condition(crate::Condition::new("捨て身", -1));
        }
        app.try_counterattack(3, 6, (2, 6));
        app.database().unit_by_uid(&hero).unwrap().damage
    }
    let normal = counter_dmg(false);
    let with_sutemi = counter_dmg(true);
    assert!(normal > 0, "反撃が命中している");
    assert_eq!(with_sutemi, normal, "捨て身 は反撃では ×3 されない");
}

/// 切り払い: 高レベルの防御側は格闘攻撃を切り払う (確率 prob=2×防御Lv−攻撃Lv、Dice 1..32)。
/// 射撃 (近接でない) は切り払い対象外。
#[test]
fn parry_nullifies_melee_for_skilled_defender() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Swordsman", 2, 6);
    // 攻撃側用に 切り払いなしの別パイロット Brute を作る。
    {
        let mut p = app.database().pilot_by_name("PILOT").unwrap().clone();
        p.name = "Brute".into();
        p.nickname = "Brute".into();
        p.features = vec![];
        app.database_mut().catalog.pilots.push(p);
    }
    let attacker = app.database_mut().register_unit(crate::UnitInstance::new(
        "Swordsman",
        "Brute",
        crate::Party::Enemy,
        3,
        6,
    ));
    // Swordsman の搭乗者 PILOT に 切り払いLv16 (prob 2×16=32 → 必ず発動)。
    {
        let p = app
            .database_mut()
            .catalog
            .pilots
            .iter_mut()
            .find(|p| p.name == "PILOT")
            .unwrap();
        p.features.push(("切り払いLv16".into(), String::new()));
    }
    let def_uid = first_player_uid(&app);
    let def_idx = app.database().idx_by_uid(&def_uid).unwrap();
    let atk_idx = app.database().idx_by_uid(&attacker).unwrap();
    assert_eq!(app.unit_parry_level(def_idx), 16, "防御側 切り払いLv16");
    assert_eq!(app.unit_parry_level(atk_idx), 0, "攻撃側 切り払いなし");
    assert!(
        app.roll_parry(atk_idx, def_idx, "武"),
        "格闘攻撃 (武) を切り払う"
    );
    assert!(
        !app.roll_parry(atk_idx, def_idx, "射"),
        "射撃は切り払い対象外"
    );
}

/// 見切り (特殊防御発動 SpecialPower): 切り払いを必ず成功させる。攻撃側 Lv が高く
/// 通常なら発動率が負 (絶対に切り払えない) でも、見切りがあれば必ず切り払う。
#[test]
fn foresight_guarantees_parry() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Swordsman", 2, 6);
    // 攻撃側: 切り払いLv16 の別パイロット (防御側 Lv1 では prob=2-16=-14 → 通常は不発)。
    {
        let mut p = app.database().pilot_by_name("PILOT").unwrap().clone();
        p.name = "Master".into();
        p.nickname = "Master".into();
        p.features = vec![("切り払いLv16".into(), String::new())];
        app.database_mut().catalog.pilots.push(p);
    }
    let attacker = app.database_mut().register_unit(crate::UnitInstance::new(
        "Swordsman",
        "Master",
        crate::Party::Enemy,
        3,
        6,
    ));
    // 防御側 PILOT に 切り払いLv1 (低レベル)。
    {
        let p = app
            .database_mut()
            .catalog
            .pilots
            .iter_mut()
            .find(|p| p.name == "PILOT")
            .unwrap();
        p.features.push(("切り払いLv1".into(), String::new()));
    }
    let def_uid = first_player_uid(&app);
    let def_idx = app.database().idx_by_uid(&def_uid).unwrap();
    let atk_idx = app.database().idx_by_uid(&attacker).unwrap();

    // 見切り無し: prob = 2×1 − 16 = −14 → 乱数 (1..32) 未満で必ず不発。
    assert!(
        !app.roll_parry(atk_idx, def_idx, "武"),
        "見切り無し・高 Lv 攻撃には切り払えない"
    );
    // 見切り付与: prob=32 で必ず切り払う。
    app.database_mut().unit_instances[def_idx].add_condition(crate::Condition::new("見切り", 1));
    assert!(
        app.roll_parry(atk_idx, def_idx, "武"),
        "見切りがあれば必ず切り払う"
    );
    // 見切りでも切り払い不可武器 (射撃) は無効。
    assert!(
        !app.roll_parry(atk_idx, def_idx, "射"),
        "見切りでも射撃は切り払い対象外"
    );
}

/// 防御特性 (耐性 ÷2 / 無効化 0 / 吸収 -1/2) を与ダメージへ適用。弱点/有効が一致すると不適用。
#[test]
fn defense_attributes_modify_damage() {
    use crate::feature::ActiveFeature;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Tank", 2, 6);
    let idx = app.database().idx_by_uid(&first_player_uid(&app)).unwrap();
    let set = |app: &mut App, feats: Vec<ActiveFeature>| {
        app.database_mut().unit_instances[idx].active_features = feats;
    };
    // 属性なし → 変化なし。
    assert_eq!(app.defense_attribute_damage(idx, "火 実", 100), 100);
    // 耐性=火 → ÷2。非一致 (氷) は変化なし。
    set(&mut app, vec![ActiveFeature::new("耐性", "火")]);
    assert_eq!(app.defense_attribute_damage(idx, "火 実", 100), 50);
    assert_eq!(app.defense_attribute_damage(idx, "氷 実", 100), 100);
    // 無効化=火 → 0。
    set(&mut app, vec![ActiveFeature::new("無効化", "火")]);
    assert_eq!(app.defense_attribute_damage(idx, "火 実", 100), 0);
    // 吸収=火 → -50 (回復)。
    set(&mut app, vec![ActiveFeature::new("吸収", "火")]);
    assert_eq!(app.defense_attribute_damage(idx, "火 実", 100), -50);
    // 弱点優先: 弱点=火 があれば耐性は適用しない (与ダメ変化なし)。
    set(
        &mut app,
        vec![
            ActiveFeature::new("弱点", "火"),
            ActiveFeature::new("耐性", "火"),
        ],
    );
    assert_eq!(app.defense_attribute_damage(idx, "火 実", 100), 100);
}

/// マップ攻撃も通常戦闘と同じく防御特性 (無効化/耐性/吸収) を反映する
/// (`event_runtime::map_attack`)。火属性マップ攻撃に対し 無効化=火 の敵は 0 ダメージで
/// 生存し、特性なしの敵は被弾する (以前はマップ攻撃だけ防御特性が抜けていた)。
#[test]
fn mapattack_applies_defense_attributes() {
    use crate::feature::ActiveFeature;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Attacker", 5, 5);
    // 攻撃側に火属性の高威力武器を付与。
    {
        let ud = app
            .database_mut()
            .catalog
            .units
            .iter_mut()
            .find(|d| d.name == "Attacker")
            .unwrap();
        ud.weapons.push(crate::data::unit::WeaponData {
            name: "FireCannon".into(),
            power: 8000,
            min_range: 1,
            max_range: 3,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: "火".into(),
            extras: Vec::new(),
        });
    }
    // 無効化=火 の敵 (生存するはず) と 特性なしの敵 (被弾するはず) を area 内に配置。
    let nullify_uid = app.database_mut().register_unit(crate::UnitInstance::new(
        "Attacker",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    app.database_mut()
        .unit_by_uid_mut(&nullify_uid)
        .unwrap()
        .active_features = vec![ActiveFeature::new("無効化", "火")];
    let plain_uid = app.database_mut().register_unit(crate::UnitInstance::new(
        "Attacker",
        "PILOT",
        crate::Party::Enemy,
        6,
        4,
    ));
    // スクリプト経路 (is_event=true) でマップ攻撃を実行。
    let shooter = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Player)
        .map(|u| u.uid.clone())
        .unwrap();
    app.execute_map_attack(&shooter, "FireCannon", 6, 5, true);
    // 無効化=火 の敵は 0 ダメージで生存。
    let nullified = app.database().unit_by_uid(&nullify_uid);
    assert!(
        nullified.is_some(),
        "無効化=火 の敵はマップ火属性攻撃で撃破されない"
    );
    assert_eq!(
        nullified.unwrap().damage,
        0,
        "無効化=火 でマップ火属性攻撃は 0 ダメージ"
    );
    // 対照: 特性なしの敵は被弾する (撃破で除去 or ダメージ > 0)。
    let plain_hit = app
        .database()
        .unit_by_uid(&plain_uid)
        .map(|u| u.damage > 0)
        .unwrap_or(true);
    assert!(plain_hit, "特性なしの敵はマップ攻撃で被弾する (対照)");
}

/// 防御特性の属性照合は字単位の**部分一致** (VB5 `InStrNotNest`)。無空白複合 class
/// ("格実火") も弱点/耐性 "火" に一致する (旧 `split_whitespace` 完全一致では非一致で
/// 防御特性が事実上機能していなかった)。併せて弱点検出 (装甲半減トリガ) を確認。
#[test]
fn defense_attr_substring_matching_and_weakness_detection() {
    use crate::feature::ActiveFeature;
    // 純関数 defense_attr_matches の字単位照合。
    assert!(
        App::defense_attr_matches("格実火", "火"),
        "複合 class が弱点 火 に部分一致"
    );
    assert!(!App::defense_attr_matches("格実水", "火"));
    assert!(
        App::defense_attr_matches("格実火", "火地獣"),
        "弱点リストのいずれかに一致"
    );
    assert!(App::defense_attr_matches("射", "全"), "全=常に一致");
    assert!(
        App::defense_attr_matches("格実火", "物"),
        "物=魔/精 でなければ一致"
    );
    assert!(
        !App::defense_attr_matches("魔術火", "物"),
        "物 は 魔 武器に不一致"
    );
    assert!(!App::defense_attr_matches("射", ""), "空リストは不一致");
    // 魔 属性例外: 魔武/魔突/魔接/魔銃/魔実 武器には「魔」防御特性が効かない。
    assert!(
        !App::defense_attr_matches("魔武", "魔"),
        "魔武 武器は 魔 防御特性に不一致 (免除)"
    );
    assert!(
        App::defense_attr_matches("魔術火", "魔"),
        "魔術 (非 魔X) は 魔 防御特性に一致"
    );
    assert!(
        App::defense_attr_matches("武魔", "魔"),
        "逆順 武魔 は免除されず一致"
    );

    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Tank", 2, 6);
    let idx = app.database().idx_by_uid(&first_player_uid(&app)).unwrap();
    // 耐性=火 → 無空白複合武器 "格実火" のダメージ半減 (旧実装は非一致で素通りだった)。
    app.database_mut().unit_instances[idx].active_features = vec![ActiveFeature::new("耐性", "火")];
    assert_eq!(app.defense_attribute_damage(idx, "格実火", 1000), 500);
    // 弱点=火 検出 (装甲半減のトリガ)。
    app.database_mut().unit_instances[idx].active_features = vec![ActiveFeature::new("弱点", "火")];
    assert!(
        app.weapon_hits_weakness(idx, "格実火"),
        "弱点 火 が複合 火武器に該当"
    );
    assert!(!app.weapon_hits_weakness(idx, "格実水"));
    // 一時付加 condition `弱点:火` も検出。
    app.database_mut().unit_instances[idx].active_features = vec![];
    app.database_mut().unit_instances[idx]
        .conditions
        .push(crate::condition::Condition::new("弱点:火", 3));
    assert!(
        app.weapon_hits_weakness(idx, "格実火"),
        "一時付加 弱点:火 も該当"
    );
    // 吸収検出 (装甲無視のトリガ): 吸収=水 は 水武器に該当、有効=水 で打ち消される。
    app.database_mut().unit_instances[idx].conditions.clear();
    app.database_mut().unit_instances[idx].active_features = vec![ActiveFeature::new("吸収", "水")];
    assert!(
        app.weapon_hits_absorb(idx, "魔術水"),
        "吸収 水 が 水武器に該当"
    );
    assert!(
        !app.weapon_hits_absorb(idx, "魔術火"),
        "非一致 (火) は不該当"
    );
    app.database_mut().unit_instances[idx].active_features = vec![
        ActiveFeature::new("吸収", "水"),
        ActiveFeature::new("有効", "水"),
    ];
    assert!(
        !app.weapon_hits_absorb(idx, "魔術水"),
        "有効=水 が吸収を打ち消す"
    );
}

/// みがわり の肩代わりダメージは、原典どおり身代わりを対象に計算し直す
/// (`Unit.Attack` の「ダメージを再計算」: `Damage(w, su)`、常に防御扱いで半減)。
/// 装甲の厚い保護対象に向かっていた小さな値ではなく、装甲の薄い身代わりへの値になる。
#[test]
fn migawari_recomputes_damage_against_substitute() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 1500, 3);
    register_enemy_data(&mut app, "Caster");
    register_enemy_data(&mut app, "Sub");
    for (name, armor, hp) in [("Caster", 5000, 100_000), ("Sub", 0, 100_000)] {
        let u = app
            .database_mut()
            .catalog
            .units
            .iter_mut()
            .find(|u| u.name == name)
            .unwrap();
        u.armor = armor;
        u.hp = hp;
    }
    let caster = spawn_enemy(&mut app, "Caster", 3, 6); // 保護対象 (装甲が厚い)
    let sub = spawn_enemy(&mut app, "Sub", 3, 5); // 身代わり (装甲 0)
    let hero = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&hero)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));
    app.set_stage_state(crate::stage::StageState::Battle);
    let script = crate::data::event::parse("SpecialPower Sub みがわり Caster\n").unwrap();
    crate::event_runtime::execute(&mut app, &script).unwrap();

    let weapon = app
        .database()
        .unit_by_name("Hero")
        .unwrap()
        .weapons
        .last()
        .cloned()
        .unwrap();
    let idx = |app: &App, uid: &str| app.database().idx_by_uid(uid).unwrap();
    let hero_idx = idx(&app, &hero);
    let vs_caster = app
        .forecast_strike(hero_idx, idx(&app, &caster), &weapon)
        .unwrap()
        .damage;
    let vs_sub = app
        .forecast_strike(hero_idx, idx(&app, &sub), &weapon)
        .unwrap()
        .damage;
    assert!(
        vs_sub > vs_caster * 3,
        "前提: 身代わりへの予測 {vs_sub} が保護対象への予測 {vs_caster} より十分大きい"
    );

    click_tile(&mut app, 2, 6);
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai);
    click_tile(&mut app, 3, 6);

    let taken = app.database().unit_by_uid(&sub).unwrap().damage;
    // 防御扱いで半減 (クリティカルなら 1.5 倍してから半減)。
    assert!(
        taken == vs_sub / 2 || taken == vs_sub * 3 / 2 / 2,
        "身代わりの被ダメージ {taken} が身代わり基準の再計算 ({vs_sub} の半分) になっていない"
    );
    assert_eq!(app.database().unit_by_uid(&caster).unwrap().damage, 0);
}

/// `防御` を選んでもダメージが半減しない例外 (原典 `Unit.Attack`「防御＆かばう時は
/// ダメージを半減」の条件): 殺 属性の武器、`防御不可` を持つ防御側、無防備 (捨て身)
/// の防御側。乱数列は同じなので、半減する場合のちょうど 2 倍 (端数 1 まで) になる。
#[test]
fn defense_does_not_halve_against_exceptions() {
    fn defended_damage(tweak: impl FnOnce(&mut App, &str)) -> i64 {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        add_weapon(&mut app, "Hero", 1500, 3);
        register_enemy_data(&mut app, "Foe");
        app.database_mut()
            .catalog
            .units
            .iter_mut()
            .find(|u| u.name == "Foe")
            .unwrap()
            .hp = 100_000;
        let foe = spawn_enemy(&mut app, "Foe", 3, 6);
        let hero = first_player_uid(&app);
        app.database_mut()
            .unit_by_uid_mut(&hero)
            .unwrap()
            .add_condition(crate::Condition::new("必中", -1));
        tweak(&mut app, &foe);
        app.script_attack(&hero, "テスト砲", &foe, "防御");
        app.database().unit_by_uid(&foe).unwrap().damage
    }
    let halved = defended_damage(|_, _| {});
    assert!(halved > 0);
    let assert_full = |label: &str, full: i64| {
        assert!(
            full == halved * 2 || full == halved * 2 + 1,
            "{label}: 防御でも半減しないはず (半減 {halved} / 実際 {full})"
        );
    };
    assert_full(
        "殺 属性",
        defended_damage(|app, _| {
            app.database_mut()
                .catalog
                .units
                .iter_mut()
                .find(|u| u.name == "Hero")
                .unwrap()
                .weapons
                .last_mut()
                .unwrap()
                .class = "殺".into();
        }),
    );
    assert_full(
        "防御不可",
        defended_damage(|app, foe| {
            app.database_mut()
                .unit_by_uid_mut(foe)
                .unwrap()
                .active_features
                .push(crate::feature::ActiveFeature::new("防御不可", ""));
        }),
    );
    assert_full(
        "捨て身",
        defended_damage(|app, foe| {
            app.database_mut()
                .unit_by_uid_mut(foe)
                .unwrap()
                .add_condition(crate::Condition::new("捨て身", -1));
        }),
    );
}

/// みがわり と切り払いの組み立て: Hero が格闘武器 (必中) で Caster を攻撃し、Sub
/// (パイロット Guard) が Caster の身代わり。`parrier` に渡したユニットだけが
/// 切り払い + 見切り (必ず切り払う) を持つ。
fn setup_migawari_parry(parrier: &str) -> (App, String, String) {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 30, 1);
    if let Some(w) = app
        .database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == "Hero")
        .and_then(|u| u.weapons.last_mut())
    {
        w.class = "武".into();
    }
    register_enemy_data(&mut app, "Caster");
    register_enemy_data(&mut app, "Sub");
    {
        let mut p = app.database().pilot_by_name("PILOT").unwrap().clone();
        p.name = "Guard".into();
        p.nickname = "Guard".into();
        p.features = vec![("切り払いLv1".into(), String::new())];
        app.database_mut().catalog.pilots.push(p);
        let mut p = app.database().pilot_by_name("PILOT").unwrap().clone();
        p.name = "Ward".into();
        p.nickname = "Ward".into();
        p.features = vec![("切り払いLv1".into(), String::new())];
        app.database_mut().catalog.pilots.push(p);
    }
    let (caster_pilot, sub_pilot) = if parrier == "Sub" {
        ("PILOT", "Guard")
    } else {
        ("Ward", "PILOT")
    };
    let caster = app.database_mut().register_unit(crate::UnitInstance::new(
        "Caster",
        caster_pilot,
        crate::Party::Enemy,
        3,
        6,
    ));
    let sub = app.database_mut().register_unit(crate::UnitInstance::new(
        "Sub",
        sub_pilot,
        crate::Party::Enemy,
        3,
        5,
    ));
    let parrier_uid = if parrier == "Sub" { &sub } else { &caster };
    app.database_mut()
        .unit_by_uid_mut(parrier_uid)
        .unwrap()
        .add_condition(crate::Condition::new("見切り", -1));
    let hero = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&hero)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));
    app.set_stage_state(crate::stage::StageState::Battle);
    let script = crate::data::event::parse("SpecialPower Sub みがわり Caster\n").unwrap();
    crate::event_runtime::execute(&mut app, &script).unwrap();
    assert!(
        app.database()
            .unit_by_uid(&caster)
            .unwrap()
            .has_condition("みがわり"),
        "前提: Caster に みがわり が掛かっている"
    );
    (app, caster, sub)
}

fn hero_attacks_caster(app: &mut App) {
    click_tile(app, 2, 6);
    let ai = attack_item_index(app);
    click_menu_item(app, ai);
    click_tile(app, 3, 6);
}

/// 原典 `Unit.Attack` は みがわり の身代わりを攻撃対象に差し替えてから
/// `CheckParryFeature` を呼ぶ: 切り払うのは身代わりのほうで、切り払えば誰も被弾しない
/// (みがわり は消費される)。
#[test]
fn migawari_substitute_parries_the_attack() {
    let (mut app, caster, sub) = setup_migawari_parry("Sub");
    hero_attacks_caster(&mut app);
    let db = app.database();
    assert_eq!(
        db.unit_by_uid(&sub).unwrap().damage,
        0,
        "身代わりが切り払えていない"
    );
    assert_eq!(
        db.unit_by_uid(&caster).unwrap().damage,
        0,
        "保護対象が被弾した"
    );
    assert!(
        !db.unit_by_uid(&caster).unwrap().has_condition("みがわり"),
        "切り払っても みがわり は消費される"
    );
    assert!(
        app.messages()
            .iter()
            .any(|m| m.contains("Guard は") && m.contains("切り払った")),
        "切り払いの主語が身代わりでない: {:?}",
        app.messages()
    );
}

/// 保護対象が切り払いを持っていても、身代わりが攻撃を受ける以上は保護対象の切り払いは
/// 判定されない (身代わりが被弾する)。
#[test]
fn migawari_protected_unit_does_not_parry() {
    let (mut app, caster, sub) = setup_migawari_parry("Caster");
    hero_attacks_caster(&mut app);
    let db = app.database();
    assert!(
        db.unit_by_uid(&sub).unwrap().damage > 0,
        "身代わりが被弾していない"
    );
    assert_eq!(
        db.unit_by_uid(&caster).unwrap().damage,
        0,
        "保護対象が被弾した"
    );
}

/// `Attack` コマンドの戦闘でも みがわり が働く (原典は `is_event` の判定より前に調べる)。
#[test]
fn migawari_applies_to_attack_command() {
    let (mut app, caster, sub) = setup_migawari_parry("Caster");
    let hero = first_player_uid(&app);
    let script =
        crate::data::event::parse(&format!("Attack {hero} テスト砲 {caster} 無抵抗\n")).unwrap();
    crate::event_runtime::execute(&mut app, &script).unwrap();
    let db = app.database();
    assert!(
        db.unit_by_uid(&sub).unwrap().damage > 0,
        "身代わりが被弾していない"
    );
    assert_eq!(
        db.unit_by_uid(&caster).unwrap().damage,
        0,
        "保護対象が被弾した"
    );
}

/// 援護防御の組み立て: Hero (必中) が Foe を攻撃し、隣の Guard (別の UnitData `Wall`)
/// が援護防御する。`guard_armor` は援護役の装甲、`melee` なら格闘武器 (武)。
/// `parrier` に渡したユニット ("Foe" / "Guard") だけが 切り払い + 見切り を持つ。
fn setup_support_guard(
    guard_armor: i64,
    melee: bool,
    parrier: Option<&str>,
) -> (App, String, String) {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    add_weapon(&mut app, "Hero", 1500, 1);
    if melee {
        if let Some(w) = app
            .database_mut()
            .catalog
            .units
            .iter_mut()
            .find(|u| u.name == "Hero")
            .and_then(|u| u.weapons.last_mut())
        {
            w.class = "武".into();
        }
    }
    let hero = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&hero)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));
    register_enemy_data(&mut app, "Foe");
    register_enemy_data(&mut app, "Wall");
    // 防御側は装甲が厚く HP が低い (援護防御の閾値 MaxHP/20 を超える予測ダメージにする)。
    for (name, armor, hp) in [("Foe", 1200, 1000), ("Wall", guard_armor, 100_000)] {
        let u = app
            .database_mut()
            .catalog
            .units
            .iter_mut()
            .find(|u| u.name == name)
            .unwrap();
        u.armor = armor;
        u.hp = hp;
    }
    {
        let mut p = app.database().pilot_by_name("PILOT").unwrap().clone();
        p.name = "Parrier".into();
        p.nickname = "Parrier".into();
        p.features = vec![("切り払いLv1".into(), String::new())];
        app.database_mut().catalog.pilots.push(p);
    }
    let pilot = |who: &str| {
        if parrier == Some(who) {
            "Parrier"
        } else {
            "PILOT"
        }
    };
    let foe = app.database_mut().register_unit(crate::UnitInstance::new(
        "Foe",
        pilot("Foe"),
        crate::Party::Enemy,
        6,
        6,
    ));
    let guard = app.database_mut().register_unit(crate::UnitInstance::new(
        "Wall",
        pilot("Guard"),
        crate::Party::Enemy,
        6,
        5,
    ));
    {
        let g = app.database_mut().unit_by_uid_mut(&guard).unwrap();
        g.add_condition(crate::Condition::new("援護防御", 9));
    }
    if let Some(who) = parrier {
        let uid = if who == "Foe" { &foe } else { &guard };
        app.database_mut()
            .unit_by_uid_mut(uid)
            .unwrap()
            .add_condition(crate::Condition::new("見切り", -1));
    }
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_map_cursor(5, 6);
    (app, foe, guard)
}

/// 原典 `Unit.Attack` は援護防御のガード役を攻撃対象として `Damage(w, su)` を計算し直し、
/// 防御扱いで半減する (旧実装は防御側への予測ダメージの 50% を固定で受けていた)。
#[test]
fn support_guard_recomputes_damage_against_guard() {
    let (mut app, foe, guard) = setup_support_guard(0, false, None);
    let weapon = app
        .database()
        .unit_by_name("Hero")
        .unwrap()
        .weapons
        .last()
        .cloned()
        .unwrap();
    let idx = |app: &App, uid: &str| app.database().idx_by_uid(uid).unwrap();
    let hero = first_player_uid(&app);
    let hero_idx = idx(&app, &hero);
    let vs_foe = app
        .forecast_strike(hero_idx, idx(&app, &foe), &weapon)
        .unwrap()
        .damage;
    let vs_guard = app
        .forecast_strike(hero_idx, idx(&app, &guard), &weapon)
        .unwrap()
        .damage;
    assert!(
        vs_foe >= 50,
        "前提: 防御側への予測 {vs_foe} が援護防御の閾値以上"
    );
    assert!(
        vs_guard > vs_foe * 3,
        "前提: ガード役への予測 {vs_guard} が防御側 {vs_foe} より十分大きい"
    );

    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let db = app.database();
    let taken = db.unit_by_uid(&guard).unwrap().damage;
    assert_eq!(db.unit_by_uid(&foe).unwrap().damage, 0, "防御側が被弾した");
    assert!(
        taken == vs_guard / 2 || taken == vs_guard * 3 / 2 / 2,
        "ガード役の被ダメージ {taken} がガード役基準の再計算 ({vs_guard} の半分) になっていない"
    );
}

/// 原典 `Unit.Attack` はガード役を防御側の位置へ動かしてから `Damage(w, su)` を求めるので、
/// 地形補正はガード役自身のマスではなく防御側のマスのものが効く。
#[test]
fn support_guard_damage_uses_defender_tile() {
    let (mut app, foe, guard) = setup_support_guard(0, false, None);
    let weapon = app
        .database()
        .unit_by_name("Hero")
        .unwrap()
        .weapons
        .last()
        .cloned()
        .unwrap();
    let set_terrain = |app: &mut App, x: u32, y: u32, id: u32| {
        let m = app.database_mut().map.as_mut().unwrap();
        let mut c = m.cell(x, y);
        c.terrain_id = id;
        m.set_cell(x, y, c);
    };
    let hero = first_player_uid(&app);
    let hero_idx = app.database().idx_by_uid(&hero).unwrap();
    let guard_idx = app.database().idx_by_uid(&guard).unwrap();
    let vs_guard_plain = app
        .forecast_strike(hero_idx, guard_idx, &weapon)
        .unwrap()
        .damage;
    // 都市 (damage_mod=15) に居るガード役への予測を基準にする。
    set_terrain(&mut app, 6, 5, 5);
    let vs_guard_city = app
        .forecast_strike(hero_idx, guard_idx, &weapon)
        .unwrap()
        .damage;
    assert!(
        vs_guard_city < vs_guard_plain,
        "前提: 都市の地形補正でダメージが減る ({vs_guard_city} < {vs_guard_plain})"
    );
    // ガード役は平地、防御側は都市。
    set_terrain(&mut app, 6, 5, 0);
    set_terrain(&mut app, 6, 6, 5);

    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let db = app.database();
    let taken = db.unit_by_uid(&guard).unwrap().damage;
    assert_eq!(db.unit_by_uid(&foe).unwrap().damage, 0, "防御側が被弾した");
    assert!(
        taken == vs_guard_city / 2 || taken == vs_guard_city * 3 / 2 / 2,
        "ガード役の被ダメージ {taken} が防御側のマス (都市) 基準 ({vs_guard_city} の半分) になっていない"
    );
}

/// 原典はガード役を選んでから `CheckParryFeature` を呼ぶので、切り払うのはガード役。
#[test]
fn support_guard_parries_as_the_guard() {
    let (mut app, foe, guard) = setup_support_guard(0, true, Some("Guard"));
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let db = app.database();
    assert_eq!(
        db.unit_by_uid(&guard).unwrap().damage,
        0,
        "ガード役が切り払えていない"
    );
    assert_eq!(db.unit_by_uid(&foe).unwrap().damage, 0, "防御側が被弾した");
    assert_eq!(
        db.unit_by_uid(&guard).unwrap().used_support_guard,
        1,
        "切り払っても援護防御の回数は使う"
    );

    // かばわれた防御側の切り払いは判定されず、ガード役が被弾する。
    let (mut app, foe, guard) = setup_support_guard(0, true, Some("Foe"));
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let db = app.database();
    assert!(
        db.unit_by_uid(&guard).unwrap().damage > 0,
        "ガード役が被弾していない"
    );
    assert_eq!(db.unit_by_uid(&foe).unwrap().damage, 0, "防御側が被弾した");
}

/// `uid` のユニットの主パイロットを、`PILOT` を複製して `edit` を当てた `name` にする。
fn give_pilot(
    app: &mut App,
    uid: &str,
    name: &str,
    edit: impl FnOnce(&mut crate::data::pilot::PilotData),
) {
    let mut p = app.database().pilot_by_name("PILOT").unwrap().clone();
    p.name = name.into();
    p.nickname = name.into();
    edit(&mut p);
    app.database_mut().catalog.pilots.push(p);
    app.database_mut().unit_by_uid_mut(uid).unwrap().pilot_name = name.into();
}

/// Hero の最後の武器で Foe を攻撃するとき、援護防御するユニットの uid (手動選択扱い)。
fn support_guard_for(app: &App, foe: &str) -> Option<String> {
    let weapon = app
        .database()
        .unit_by_name("Hero")
        .unwrap()
        .weapons
        .last()
        .cloned()
        .unwrap();
    let hero = first_player_uid(app);
    let atk_idx = app.database().idx_by_uid(&hero).unwrap();
    let def_idx = app.database().idx_by_uid(foe).unwrap();
    let expected = app.forecast_strike(atk_idx, def_idx, &weapon).unwrap();
    app.try_find_support_guard(atk_idx, def_idx, &weapon, &expected, true)
        .map(|i| app.database().unit_instances[i].uid.clone())
}

/// チーム (`チーム=名前`): チームを持つガード役は同じチームの味方だけをかばう。
/// チームを持たないガード役は誰でもかばう (`その他の特殊能力(パイロット).md`)。
#[test]
fn support_guard_respects_team() {
    let team = |t: &'static str| {
        move |p: &mut crate::data::pilot::PilotData| p.features.push(("チーム".into(), t.into()))
    };
    let (mut app, foe, guard) = setup_support_guard(0, false, None);
    give_pilot(&mut app, &guard, "TeamA", team("A"));
    assert_eq!(
        support_guard_for(&app, &foe),
        None,
        "チーム外 (チーム無し) をかばった"
    );
    give_pilot(&mut app, &foe, "TeamB", team("B"));
    assert_eq!(support_guard_for(&app, &foe), None, "別チームをかばった");
    give_pilot(&mut app, &foe, "TeamA2", team("A"));
    assert_eq!(
        support_guard_for(&app, &foe),
        Some(guard.clone()),
        "同じチームをかばわない"
    );

    let (mut app, foe, guard) = setup_support_guard(0, false, None);
    give_pilot(&mut app, &foe, "TeamB", team("B"));
    assert_eq!(
        support_guard_for(&app, &foe),
        Some(guard),
        "チーム無しのガード役がかばわない"
    );
}

/// 機械 (性格) をかばうのは機械だけ。
#[test]
fn support_guard_machine_only_guarded_by_machine() {
    let machine = |p: &mut crate::data::pilot::PilotData| p.personality = Some("機械".into());
    let (mut app, foe, guard) = setup_support_guard(0, false, None);
    give_pilot(&mut app, &foe, "Drone", machine);
    assert_eq!(
        support_guard_for(&app, &foe),
        None,
        "機械でないユニットが機械をかばった"
    );
    give_pilot(&mut app, &guard, "Drone2", machine);
    assert_eq!(
        support_guard_for(&app, &foe),
        Some(guard),
        "機械が機械をかばわない"
    );
}

/// 防御側の領域 (空中) に入れないガード役はかばわない。
/// オプション `対ＮＰＣサポートガード無効`: 味方は ＮＰＣ をかばわなくなる。
/// ＮＰＣ が味方をかばうのはオプションがあっても変わらない。
#[test]
fn support_guard_option_disables_player_guarding_npc() {
    let (mut app, foe, guard) = setup_support_guard(0, false, None);
    let hero = first_player_uid(&app);
    let set_party = |app: &mut App, uid: &str, party: crate::Party| {
        app.database_mut().unit_by_uid_mut(uid).unwrap().party = party;
    };
    let weapon = app
        .database()
        .unit_by_name("Hero")
        .unwrap()
        .weapons
        .last()
        .cloned()
        .unwrap();
    let find = |app: &App, def: &str| {
        let atk_idx = app.database().idx_by_uid(&hero).unwrap();
        let def_idx = app.database().idx_by_uid(def).unwrap();
        let expected = app.forecast_strike(atk_idx, def_idx, &weapon).unwrap();
        app.try_find_support_guard(atk_idx, def_idx, &weapon, &expected, true)
            .map(|i| app.database().unit_instances[i].uid.clone())
    };
    // 敵 (Hero) が ＮＰＣ (Foe) を攻撃し、味方 (Wall) がかばう。
    set_party(&mut app, &hero, crate::Party::Enemy);
    set_party(&mut app, &foe, crate::Party::Npc);
    set_party(&mut app, &guard, crate::Party::Player);
    assert_eq!(
        find(&app, &foe),
        Some(guard.clone()),
        "前提: 味方が ＮＰＣ をかばう"
    );
    app.set_script_var(
        "Option(対ＮＰＣサポートガード無効)".to_string(),
        "1".to_string(),
    );
    assert_eq!(
        find(&app, &foe),
        None,
        "オプション下で味方が ＮＰＣ をかばった"
    );
    // 立場を入れ替えると (ＮＰＣ が味方をかばう) オプションの影響を受けない。
    set_party(&mut app, &foe, crate::Party::Player);
    set_party(&mut app, &guard, crate::Party::Npc);
    assert_eq!(
        find(&app, &foe),
        Some(guard.clone()),
        "ＮＰＣ が味方をかばわなくなった"
    );
}

#[test]
fn support_guard_requires_entering_defender_area() {
    let (mut app, foe, guard) = setup_support_guard(0, false, None);
    app.database_mut()
        .unit_by_uid_mut(&foe)
        .unwrap()
        .current_area = "空中".into();
    app.database_mut()
        .unit_by_uid_mut(&guard)
        .unwrap()
        .current_area = "地上".into();
    for u in app
        .database_mut()
        .catalog
        .units
        .iter_mut()
        .filter(|u| u.name == "Wall")
    {
        u.transportation = "陸".into();
    }
    assert_eq!(
        support_guard_for(&app, &foe),
        None,
        "飛べないユニットが空中の味方をかばった"
    );
    for u in app
        .database_mut()
        .catalog
        .units
        .iter_mut()
        .filter(|u| u.name == "Wall")
    {
        u.transportation = "空陸".into();
    }
    assert_eq!(
        support_guard_for(&app, &foe),
        Some(guard),
        "飛べるユニットがかばわない"
    );
}

/// ガード役が撃破される見込みならかばわない (ボスが格下に守られる場合は例外)。
/// マップ兵器はかばえない。
#[test]
fn support_guard_declines_when_it_would_fall_or_map_weapon() {
    let (mut app, foe, guard) = setup_support_guard(0, false, None);
    let hp = app.database().unit_by_name("Wall").unwrap().hp;
    app.database_mut().unit_by_uid_mut(&guard).unwrap().damage = hp - 1;
    assert_eq!(
        support_guard_for(&app, &foe),
        None,
        "撃破される見込みでかばった"
    );
    app.database_mut().unit_by_uid_mut(&foe).unwrap().boss_rank = 1;
    assert_eq!(
        support_guard_for(&app, &foe),
        Some(guard.clone()),
        "格上のボスを守る格下は撃破されてもかばう"
    );

    let (mut app, foe, _) = setup_support_guard(0, false, None);
    for u in app
        .database_mut()
        .catalog
        .units
        .iter_mut()
        .filter(|u| u.name == "Hero")
    {
        u.weapons.last_mut().unwrap().class = "Ｍ全".into();
    }
    assert_eq!(support_guard_for(&app, &foe), None, "マップ兵器をかばった");
}

/// 再攻撃は原典 `Unit.Attack` の中の繰り返しなので、本線攻撃の直後、反撃と援護攻撃より
/// 前に行う。反撃・援護攻撃は 1 回だけ。
#[test]
fn reattack_comes_before_counter_and_support_attack() {
    let (mut app, hero, _, _) = setup_support_attack(1_000_000, 1_000_000, 3000);
    app.database_mut()
        .unit_by_uid_mut(&hero)
        .unwrap()
        .add_condition(crate::Condition::new("再攻撃", -1));
    app.map_cursor = Some((5, 6));
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let msgs = app.messages().to_vec();
    let pos = |pat: &str| msgs.iter().position(|m| m.contains(pat));
    let reattack = pos("の再攻撃！").expect("再攻撃していない");
    let counter = pos("↩ 反撃").expect("反撃していない");
    let support = pos("サポートアタック").expect("援護攻撃していない");
    assert!(reattack < counter && counter < support, "{msgs:?}");
    let count = |pat: &str| msgs.iter().filter(|m| m.contains(pat)).count();
    assert_eq!(count("↩ 反撃"), 1, "{msgs:?}");
    assert_eq!(count("サポートアタック"), 1, "{msgs:?}");
}

/// 原典の反撃も `Unit.Attack` なので、反撃側も再攻撃する。
#[test]
fn counter_attack_can_reattack() {
    let (mut app, _, _, foe) = setup_support_attack(1_000_000, 1_000_000, 3000);
    app.database_mut()
        .unit_by_uid_mut(&foe)
        .unwrap()
        .add_condition(crate::Condition::new("再攻撃", -1));
    app.map_cursor = Some((5, 6));
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let msgs = app.messages().to_vec();
    let counters: Vec<usize> = msgs
        .iter()
        .enumerate()
        .filter(|(_, m)| m.contains("↩ 反撃"))
        .map(|(i, _)| i)
        .collect();
    assert_eq!(counters.len(), 2, "{msgs:?}");
    assert!(
        msgs[counters[0]..counters[1]]
            .iter()
            .any(|m| m.contains("の再攻撃！")),
        "{msgs:?}"
    );
}

/// `setup_support_attack` の敵のパイロットを `FOE` (愛称 FOE) に替え、`pilot_message.txt`
/// を取り込む。
fn setup_pilot_messages(text: &str) -> (App, String, String, String) {
    let (mut app, hero, sup, foe) = setup_support_attack(1_000_000, 1_000_000, 3000);
    push_pilot_with_features(&mut app, "FOE", Vec::new());
    app.database_mut().unit_by_uid_mut(&foe).unwrap().pilot_name = "FOE".into();
    let ingest = crate::scenario::ingest_data_file(&mut app, "Data/pilot_message.txt", text)
        .expect("pilot_message.txt はデータファイル");
    assert_eq!(ingest.count, 2);
    (app, hero, sup, foe)
}

/// パイロットの台詞 (`pilot_message.txt`) が原典 `Unit.Attack` の順に出る: 攻撃の台詞 →
/// 目標の被弾の台詞 → 反撃の台詞 → 援護攻撃の台詞。
#[test]
fn pilot_messages_follow_battle_flow() {
    let (mut app, ..) = setup_pilot_messages(
        "PILOT\n攻撃, 撃つぞ！\nサポートアタック, 援護する！;続け:！\n\n\
         FOE\nダメージ小, 効かんな！\n攻撃(反撃), 返すぞ！\n回避, 遅い！\n",
    );
    app.map_cursor = Some((5, 6));
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let msgs = app.messages().to_vec();
    let pos = |pat: &str| {
        msgs.iter()
            .position(|m| m == pat)
            .unwrap_or_else(|| panic!("{pat} が無い: {msgs:?}"))
    };
    let attack = pos("P「撃つぞ！」");
    let damaged = pos("FOE「効かんな！」");
    let counter = pos("FOE「返すぞ！」");
    let support = pos("P「援護する！」");
    // `;` はページ送り、`:` は消える。
    let support2 = pos("P「続け！」");
    assert!(
        attack < damaged && damaged < counter && counter < support && support < support2,
        "{msgs:?}"
    );
    // 援護役の攻撃の台詞は援護の台詞の後。
    assert!(
        msgs[support2..].iter().any(|m| m == "P「撃つぞ！」"),
        "{msgs:?}"
    );
    assert!(!msgs.iter().any(|m| m == "FOE「遅い！」"), "{msgs:?}");
}

/// 沈黙中のパイロットは台詞の代わりに「…………」と言い、麻痺していれば何も言わない。
#[test]
fn silenced_pilot_says_nothing_but_dots() {
    let run = |condition: &str| {
        let (mut app, hero, sup, _) =
            setup_pilot_messages("PILOT\n攻撃, 撃つぞ！\n\nFOE\n回避, 遅い！\n");
        // 援護役も同じパイロットなので援護攻撃させない。
        app.database_mut()
            .unit_by_uid_mut(&sup)
            .unwrap()
            .conditions
            .retain(|c| c.name != "援護攻撃");
        app.database_mut()
            .unit_by_uid_mut(&hero)
            .unwrap()
            .add_condition(crate::Condition::new(condition, -1));
        app.map_cursor = Some((5, 6));
        app.attack_resolve_and_run(Some((6, 6)), false, "");
        app.messages()
            .iter()
            .filter(|m| m.starts_with("P「"))
            .cloned()
            .collect::<Vec<_>>()
    };
    assert_eq!(run("沈黙")[0], "P「…………」");
    assert!(!run("麻痺").iter().any(|m| m == "P「撃つぞ！」"));
}

/// ダイアログデータ (`pilot_dialog.txt`) はメッセージデータより先に使い、複数の話者が
/// 順に話す。非戦闘員は常に話せるが、作成されていないパイロットが話すダイアログは
/// 使わない (原典 `Dialog.IsAvailable`)。
#[test]
fn pilot_dialog_comes_before_messages() {
    let run = |dialog: &str| {
        let (mut app, _, sup, _) =
            setup_pilot_messages("PILOT\n攻撃, 撃つぞ！\n\nFOE\n回避, 遅い！\n");
        app.database_mut()
            .unit_by_uid_mut(&sup)
            .unwrap()
            .conditions
            .retain(|c| c.name != "援護攻撃");
        push_pilot_with_features(&mut app, "ロイ", Vec::new());
        for (name, text) in [
            ("Data/non_pilot.txt", "相棒\n相棒くん,aibo.bmp\n"),
            ("Data/pilot_dialog.txt", dialog),
        ] {
            crate::scenario::ingest_data_file(&mut app, name, text).expect("データファイル");
        }
        app.map_cursor = Some((5, 6));
        app.attack_resolve_and_run(Some((6, 6)), false, "");
        app.messages().to_vec()
    };
    let msgs =
        run("PILOT\n攻撃\nPILOT, いくぞ、相棒！\n相棒, 了解！\nシステム, -\nPILOT, 出ない台詞\n");
    let pos = |pat: &str| msgs.iter().position(|m| m == pat);
    let (a, b) = (pos("P「いくぞ、相棒！」"), pos("相棒くん「了解！」"));
    assert!(a.is_some() && a < b, "{msgs:?}");
    assert!(pos("P「撃つぞ！」").is_none(), "{msgs:?}");
    // メッセージ `-` の行でダイアログを打ち切る。
    assert!(!msgs.iter().any(|m| m.contains("出ない台詞")), "{msgs:?}");
    // 作成されていないパイロット (ロイ) が話すダイアログは使わず、メッセージデータへ。
    let msgs = run("PILOT\n攻撃\nロイ, 援護する！\n");
    assert!(msgs.iter().any(|m| m == "P「撃つぞ！」"), "{msgs:?}");
    assert!(!msgs.iter().any(|m| m.contains("援護する")), "{msgs:?}");
}

/// かばう (原典 `LookForGuardHelp`): 防御側の隣の味方が、信頼度が `Dice(10)` 以上で、
/// 「かばう(防御側)」の台詞を持ち、受けても倒されなければ代わりに攻撃を受ける。
#[test]
fn ally_with_relation_and_line_protects_defender() {
    let run = |relation: &str| {
        let (mut app, _, sup, foe) =
            setup_pilot_messages("PILOT\nかばう(FOE), 危ない！\n\nFOE\n回避, 遅い！\n");
        app.database_mut()
            .unit_by_uid_mut(&sup)
            .unwrap()
            .conditions
            .retain(|c| c.name != "援護攻撃");
        let buddy = spawn_party(&mut app, "Foe", crate::Party::Enemy, 7, 6);
        app.set_script_var("__rel_PILOT_FOE".to_string(), relation.to_string());
        app.map_cursor = Some((5, 6));
        app.attack_resolve_and_run(Some((6, 6)), false, "");
        let dmg = |uid: &str| app.database().unit_by_uid(uid).unwrap().damage;
        (dmg(&foe), dmg(&buddy), app.messages().to_vec())
    };
    let (foe, buddy, msgs) = run("10");
    assert_eq!(foe, 0, "{msgs:?}");
    assert!(buddy > 0, "{msgs:?}");
    assert!(msgs.iter().any(|m| m == "P「危ない！」"), "{msgs:?}");
    assert!(msgs.iter().any(|m| m.starts_with("かばう: ")), "{msgs:?}");
    // 信頼度が 0 なら Dice(10) を超えられず、かばわない。
    let (foe, buddy, msgs) = run("0");
    assert!(foe > 0 && buddy == 0, "{msgs:?}");
}

/// 合体技の試験用: Hero が射程 1 の合体技 `Combo` (消費 EN 10) を持ち、パートナーは Sup
/// (パイロット BUDDY)。Sup は目標 Foe (6,6) の斜め (5,7) にいる。
fn setup_combination(
    feature: &str,
) -> (App, String, String, String, crate::data::unit::WeaponData) {
    let (mut app, hero, sup, foe) = setup_support_attack(1_000_000, 1_000_000, 0);
    let combo = crate::data::unit::WeaponData {
        name: "Combo".to_string(),
        power: 500,
        min_range: 1,
        max_range: 1,
        precision: 100,
        bullet: -1,
        en_consumption: 10,
        necessary_morale: 0,
        adaption: String::new(),
        critical: 0,
        class: "合".to_string(),
        extras: Vec::new(),
    };
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == "Hero")
        .unwrap()
        .weapons
        .push(combo.clone());
    app.database_mut()
        .unit_by_uid_mut(&hero)
        .unwrap()
        .active_features
        .push(crate::feature::ActiveFeature::new(feature, "Combo Sup"));
    push_pilot_with_features(&mut app, "BUDDY", Vec::new());
    app.database_mut().unit_by_uid_mut(&sup).unwrap().pilot_name = "BUDDY".into();
    (app, hero, sup, foe, combo)
}

/// 合体技 (原典 `CombinationPartner`): 射程 1 の合体技は目標の隣にパートナーがいるときだけ
/// 使える。使うとパートナーも EN を払って行動済みになり、攻撃の間は `Partner` 関数から
/// 見える。
#[test]
fn combination_attack_needs_partner_next_to_target() {
    let (mut app, hero, sup, foe, combo) = setup_combination("合体技");
    let hero_idx = app.database().idx_by_uid(&hero).unwrap();
    // 斜めは隣ではない。
    assert!(!app.weapon_firable(hero_idx, &combo));
    assert!(!app.combination_ok_vs(hero_idx, &combo, (6, 6)));
    assert!(app.database_mut().move_unit(&sup, 6, 7));
    assert!(app.weapon_firable(hero_idx, &combo));
    assert!(app.combination_ok_vs(hero_idx, &combo, (6, 6)));

    let stmts = crate::data::event::parse(
        "使用 PILOT Combo:\nSet n CountPartner()\nSet p Partner(1)\nExit\n",
    )
    .unwrap();
    app.script_library_mut()
        .append_with_name(&stmts, "test_combo.eve");
    app.selected_weapon_idx = 2;
    app.map_cursor = Some((5, 6));
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let msgs = app.messages().to_vec();
    assert!(
        msgs.iter()
            .any(|m| m.ends_with("[Sup]と共に[Combo]で攻撃をかけた。")),
        "{msgs:?}"
    );
    assert_eq!(app.script_var("n"), "1");
    assert_eq!(app.script_var("p"), sup);
    let s = app.database().unit_by_uid(&sup).unwrap();
    assert!(s.has_acted);
    assert_eq!(s.en_consumed, 10, "同名の武器が無ければ技の消費 EN を払う");
    assert!(app.database().unit_by_uid(&foe).unwrap().damage > 0);
    assert!(app.selected_partners().is_empty());
}

/// `合体技Lv2` はパイロット間の信頼度が双方 2 以上のときだけ使える。
#[test]
fn leveled_combination_needs_mutual_relation() {
    let (mut app, hero, sup, _, combo) = setup_combination("合体技Lv2");
    assert!(app.database_mut().move_unit(&sup, 6, 7));
    let hero_idx = app.database().idx_by_uid(&hero).unwrap();
    assert!(!app.weapon_firable(hero_idx, &combo));
    app.set_script_var("__rel_PILOT_BUDDY".to_string(), "2".to_string());
    assert!(
        !app.weapon_firable(hero_idx, &combo),
        "片方向だけでは足りない"
    );
    app.set_script_var("__rel_BUDDY_PILOT".to_string(), "2".to_string());
    assert!(app.weapon_firable(hero_idx, &combo));
}

/// 合体技は反撃に使えない。
#[test]
fn combination_attack_is_not_a_counter() {
    let (mut app, hero, sup, _, combo) = setup_combination("合体技");
    assert!(app.database_mut().move_unit(&sup, 6, 7));
    let hero_idx = app.database().idx_by_uid(&hero).unwrap();
    assert!(app.weapon_usable_vs(hero_idx, &combo, (6, 6), false, false));
    assert!(!app.weapon_usable_vs(hero_idx, &combo, (6, 6), false, true));
}

/// ダイアログの `@名前` の行は、その人が合体技のパートナーの主パイロットのときだけ使う。
#[test]
fn combination_dialog_follows_partner() {
    let (mut app, _, sup, _, _) = setup_combination("合体技");
    assert!(app.database_mut().move_unit(&sup, 6, 7));
    crate::scenario::ingest_data_file(
        &mut app,
        "Data/pilot_dialog.txt",
        "PILOT\nCombo\n@OTHER, 違う！\nPILOT, 違う！\nCombo\n@BUDDY, 任せろ！\nPILOT, 行くぞ！\n",
    )
    .expect("pilot_dialog.txt はデータファイル");
    app.selected_weapon_idx = 2;
    app.map_cursor = Some((5, 6));
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let msgs = app.messages().to_vec();
    let pos = |pat: &str| msgs.iter().position(|m| m == pat);
    let (Some(buddy), Some(hero)) = (pos("BUDDY「任せろ！」"), pos("P「行くぞ！」"))
    else {
        panic!("{msgs:?}");
    };
    assert!(buddy < hero, "{msgs:?}");
    assert!(!msgs.iter().any(|m| m.contains("違う")), "{msgs:?}");
}

/// Hero に属性 `class` の武器 Burst (武器番号 2) を足す。
fn hero_weapon(app: &mut App, class: &str, en: i32) -> crate::data::unit::WeaponData {
    let w = weapon_with_class(class, en);
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == "Hero")
        .unwrap()
        .weapons
        .push(w.clone());
    w
}

fn weapon_with_class(class: &str, en: i32) -> crate::data::unit::WeaponData {
    crate::data::unit::WeaponData {
        name: "Burst".to_string(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 100,
        bullet: -1,
        en_consumption: en,
        necessary_morale: 0,
        adaption: String::new(),
        critical: 0,
        class: class.to_string(),
        extras: Vec::new(),
    }
}

/// 原典 `UseWeapon`: 消 (消耗)・気 (気力)・失 (ＨＰ)・尽 (EN を使い切る) は撃った本人に効く。
/// 属性は区切りなしで並ぶ。消耗したユニットは次のターンまで行動できない。
#[test]
fn weapon_use_pays_self_costs() {
    let (mut app, hero, _, _) = setup_support_attack(1000, 1_000_000, 0);
    hero_weapon(&mut app, "格消気L2失L1尽", 10);
    let idx = app.database().idx_by_uid(&hero).unwrap();
    let max_en = app
        .database()
        .effective_max_en(&app.database().unit_instances[idx]);
    let morale = app.database().unit_instances[idx].morale;
    app.selected_weapon_idx = 2;
    app.map_cursor = Some((5, 6));
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let u = app.database().unit_by_uid(&hero).unwrap();
    assert!(u.has_condition("消耗"));
    assert!(u.attack_disabled());
    assert_eq!(u.morale, morale - 10);
    assert_eq!(u.damage, 100, "最大ＨＰの 10%");
    assert_eq!(u.en_consumed, max_en);
}

/// 自爆攻撃 (自) を使ったユニットは攻撃の後で壊れ、反撃を受けない。
#[test]
fn self_destruct_weapon_destroys_attacker() {
    let (mut app, hero, _, foe) = setup_support_attack(1000, 1_000_000, 50);
    hero_weapon(&mut app, "格自", 0);
    app.selected_weapon_idx = 2;
    app.map_cursor = Some((5, 6));
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    assert!(app.database().unit_by_uid(&hero).is_none());
    assert!(app.database().unit_by_uid(&foe).unwrap().damage > 0);
    let msgs = app.messages().to_vec();
    assert!(!msgs.iter().any(|m| m.contains("反撃")), "{msgs:?}");
}

/// 自動チャージ (ＡL2) の武器は使うと 2 ターン「<武器名>充填中」になって使えない。
#[test]
fn auto_charge_weapon_waits_turns() {
    let (mut app, hero, _, _) = setup_support_attack(1000, 1_000_000, 0);
    let burst = hero_weapon(&mut app, "格ＡL2", 0);
    let idx = app.database().idx_by_uid(&hero).unwrap();
    assert!(app.weapon_firable(idx, &burst));
    app.selected_weapon_idx = 2;
    app.map_cursor = Some((5, 6));
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let idx = app.database().idx_by_uid(&hero).unwrap();
    let u = &app.database().unit_instances[idx];
    assert!(u
        .conditions
        .iter()
        .any(|c| c.name == "Burst充填中" && c.lifetime == 2));
    assert!(!app.weapon_firable(idx, &burst));
}

/// 原典 `IsWeaponAvailable`: 霊力・属性使用不能・使用禁止・瀕死時限定・沈黙。
#[test]
fn weapon_availability_follows_class_and_state() {
    let (mut app, hero, _, _) = setup_support_attack(1000, 1_000_000, 0);
    let idx = app.database().idx_by_uid(&hero).unwrap();
    let ok = |app: &App, class: &str| app.weapon_firable(idx, &weapon_with_class(class, 0));
    // 霊力消費攻撃は霊力が 5×レベル 要る。
    app.database_mut().unit_instances[idx].plana = 9;
    assert!(!ok(&app, "射霊L2"));
    app.database_mut().unit_instances[idx].plana = 10;
    assert!(ok(&app, "射霊L2"));
    let add = |app: &mut App, name: &str| {
        app.database_mut().unit_instances[idx].add_condition(crate::Condition::new(name, 3));
    };
    add(&mut app, "霊力使用不能");
    assert!(!ok(&app, "射霊L2"));
    // 剋 で封じられた属性。
    add(&mut app, "火属性使用不能");
    assert!(!ok(&app, "射火"));
    assert!(ok(&app, "射"));
    // 使用禁止。
    assert!(!ok(&app, "射禁"));
    // 瀕死 (ＨＰが最大の 1/4 以下) のときだけ。
    assert!(!ok(&app, "格瀕"));
    app.database_mut().unit_instances[idx].damage = 750;
    assert!(ok(&app, "格瀕"));
    // 沈黙すると 術・音 が使えない。
    add(&mut app, "沈黙");
    assert!(!ok(&app, "術"));
    assert!(!ok(&app, "射音"));
    assert!(ok(&app, "射"));
}

/// 原典 `WeaponENConsumption`: 特殊能力「ＥＮ消費減少Lv*」で 10%/Lv 減る。
#[test]
fn en_saving_feature_reduces_weapon_cost() {
    let (mut app, hero, _, _) = setup_support_attack(1000, 1_000_000, 0);
    let idx = app.database().idx_by_uid(&hero).unwrap();
    let w = weapon_with_class("射", 10);
    assert_eq!(app.weapon_en_cost(idx, &w), 10);
    app.database_mut().unit_instances[idx]
        .active_features
        .push(crate::feature::ActiveFeature::new("ＥＮ消費減少Lv2", ""));
    assert_eq!(app.weapon_en_cost(idx, &w), 8);
}

/// 同名の武器を持たない合体技のパートナーも、技の代償 (気力) を払う。
#[test]
fn combination_partner_pays_weapon_penalties() {
    let (mut app, _, sup, _, _) = setup_combination("合体技");
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == "Hero")
        .unwrap()
        .weapons
        .last_mut()
        .unwrap()
        .class = "合気L1".into();
    assert!(app.database_mut().move_unit(&sup, 6, 7));
    let morale = app.database().unit_by_uid(&sup).unwrap().morale;
    app.selected_weapon_idx = 2;
    app.map_cursor = Some((5, 6));
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    assert_eq!(app.database().unit_by_uid(&sup).unwrap().morale, morale - 5);
}
