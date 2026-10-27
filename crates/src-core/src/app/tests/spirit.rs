//! 精神コマンド (プレイヤー操作・効果・SP 消費) のテスト。

use super::*;

#[test]
fn spirit_command_menu_applies_condition_and_consumes_sp() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    use crate::data::pilot::SpiritCommand;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    // 勝敗判定回避のため敵を 1 体配置 (味方 0 / 敵 0 での即決着を防ぐ)。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);

    let uid = give_spirit_commands(
        &mut app,
        vec![
            SpiritCommand {
                name: "集中".into(),
                cost: Some(20),
                level: 1,
            },
            // level 99: 未習得 (パイロットは level 1)。メニューに出ない。
            SpiritCommand {
                name: "熱血".into(),
                cost: None,
                level: 99,
            },
        ],
        47,
    );

    // 発動可能なのは 集中 のみ (熱血 は未習得)。
    assert_eq!(
        app.spirit_command_options(&uid),
        vec![("集中".to_string(), 20)]
    );

    // ユニットメニューを開く → 精神コマンド項目が並ぶ。
    let pos = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        (u.x, u.y)
    };
    app.open_unit_menu(pos);
    let has_spirit = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Spirit)
    );
    assert!(has_spirit, "精神コマンド項目が表示される");

    // 精神コマンドを選択 → サブメニュー (pending_dialog Menu) が出る。
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Spirit));
    assert!(matches!(
        app.pending_dialog(),
        Some(crate::dialog::PendingDialog::Menu { .. })
    ));

    // 1 番目 (集中) を選択 → condition 付与 + SP 20 消費。
    assert!(app.respond_dialog(1));
    {
        let u = app.database().unit_by_uid(&uid).unwrap();
        assert!(u.has_condition("集中"), "集中 condition が付与される");
        assert_eq!(u.sp_consumed, 20, "SP 20 消費");
    }

    // 次の味方フェイズ開始 (T2) で 集中 (lifetime 1) が解除される。
    app.handle_input(Input::EndPhase);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert!(!u.has_condition("集中"), "集中 は次ターン開始で解除");
}

#[test]
fn spirit_command_hidden_when_insufficient_sp() {
    use crate::data::pilot::SpiritCommand;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // 最大 SP 10 では 熱血(30) は買えない。集中(20) も買えない。
    let uid = give_spirit_commands(
        &mut app,
        vec![
            SpiritCommand {
                name: "集中".into(),
                cost: Some(20),
                level: 1,
            },
            SpiritCommand {
                name: "熱血".into(),
                cost: Some(30),
                level: 1,
            },
        ],
        10,
    );
    assert!(
        app.spirit_command_options(&uid).is_empty(),
        "SP 不足なら発動可能コマンドは空"
    );
    // メニューにも精神コマンド項目は出ない。
    let pos = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        (u.x, u.y)
    };
    app.open_unit_menu(pos);
    let has_spirit = matches!(
        app.command_menu(),
        Some(crate::command_menu::CommandMenu::Unit { items, .. })
            if items.contains(&crate::command_menu::UnitMenuItem::Spirit)
    );
    assert!(!has_spirit, "SP 不足では精神コマンド項目は非表示");
}

#[test]
fn spirit_kasoku_shinsoku_add_move_points() {
    use crate::Condition;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = first_player_uid(&app);
    let base = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        app.database().effective_speed(u)
    };
    // 加速: 移動力 +2
    let mut u = app.database().unit_by_uid(&uid).unwrap().clone();
    u.add_condition(Condition::new("加速", 1));
    assert_eq!(app.database().effective_speed(&u), base + 2);
    // 神速: 移動力 +3
    let mut u = app.database().unit_by_uid(&uid).unwrap().clone();
    u.add_condition(Condition::new("神速", 1));
    assert_eq!(app.database().effective_speed(&u), base + 3);
}

#[test]
fn spirit_heal_effects_reduce_damage() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = first_player_uid(&app);
    let max_hp = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        app.database().effective_max_hp(u)
    }; // 100
       // 信頼: 最大HP 1/3 回復
    app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 90;
    app.apply_spirit_effect(&uid, "信頼");
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().damage,
        90 - max_hp / 3
    );
    // 愛: 全快
    app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 90;
    app.apply_spirit_effect(&uid, "愛");
    assert_eq!(app.database().unit_by_uid(&uid).unwrap().damage, 0);
}

#[test]
fn spirit_kakusei_resets_action_flags() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.has_acted = true;
        u.has_moved = true;
    }
    app.apply_spirit_effect(&uid, "覚醒");
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert!(!u.has_acted && !u.has_moved, "覚醒 で再行動可能になる");
}

#[test]
fn spirit_hokyuu_restores_en_and_lowers_morale() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.en_consumed = 20;
        u.morale = 100;
    }
    app.apply_spirit_effect(&uid, "補給");
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(u.en_consumed, 0, "補給 で EN 全快");
    assert_eq!(u.morale, 90, "補給 は気力 -10");
}

#[test]
fn spirit_morale_and_grant_effects() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = first_player_uid(&app);
    // 脱力: 気力 -10
    app.database_mut().unit_by_uid_mut(&uid).unwrap().morale = 100;
    app.apply_spirit_effect(&uid, "脱力");
    assert_eq!(app.database().unit_by_uid(&uid).unwrap().morale, 90);
    // 応援: 対象に「努力」を付与
    app.apply_spirit_effect(&uid, "応援");
    assert!(app
        .database()
        .unit_by_uid(&uid)
        .unwrap()
        .has_condition("努力"));
    // 祝福: 対象に「幸運」を付与
    app.apply_spirit_effect(&uid, "祝福");
    assert!(app
        .database()
        .unit_by_uid(&uid)
        .unwrap()
        .has_condition("幸運"));
}

#[test]
fn spirit_fukkatsu_revives_once() {
    use crate::Condition;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = first_player_uid(&app);
    let idx = app.database().idx_by_uid(&uid).unwrap();
    app.database_mut().unit_instances[idx].add_condition(Condition::new("復活", 1));
    app.database_mut().unit_instances[idx].damage = 9999;
    assert!(app.revive_if_possible(idx), "復活 保持なら復活する");
    assert_eq!(app.database().unit_instances[idx].damage, 0, "HP 全快");
    assert!(
        !app.database().unit_instances[idx].has_condition("復活"),
        "復活 は 1 回で消費"
    );
    assert!(!app.revive_if_possible(idx), "2 回目は復活しない");
}

#[test]
fn spirit_single_target_defers_to_target_mode() {
    use crate::command_menu::{ActionMode, MenuActionId, UnitMenuItem};
    use crate::data::pilot::SpiritCommand;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    // 勝敗即決を避けるため敵を 1 体配置。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = give_spirit_commands(
        &mut app,
        vec![SpiritCommand {
            name: "信頼".into(),
            cost: Some(25),
            level: 1,
        }],
        50,
    );
    let pos = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        (u.x, u.y)
    };
    app.open_unit_menu(pos);
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Spirit));
    assert!(app.respond_dialog(1));
    // 対象選択モードへ遷移する (caster には適用せず SP も未消費)。
    match app.action_mode() {
        ActionMode::SpiritTarget {
            ref spirit,
            target_enemy,
            ..
        } => {
            assert_eq!(spirit, "信頼");
            assert!(!target_enemy, "信頼 は味方単体対象");
        }
        other => panic!("SpiritTarget を期待: {other:?}"),
    }
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert!(!u.has_condition("信頼"), "対象未確定で caster に付与しない");
    assert_eq!(u.sp_consumed, 0, "対象未確定で SP 未消費");
}

#[test]
fn apply_spirit_to_target_heals_and_consumes_sp() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let caster = first_player_uid(&app);
    // 損傷した味方を対象として配置。
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Player,
        3,
        6,
    ));
    app.database_mut().unit_by_uid_mut(&target).unwrap().damage = 90;
    app.apply_spirit_to_target(&caster, &target, "信頼", 25);
    // 信頼 = 最大HP(100)/3 = 33 回復。
    assert_eq!(app.database().unit_by_uid(&target).unwrap().damage, 90 - 33);
    // caster (PilotInstance 無し) は sp_consumed に反映。
    assert_eq!(app.database().unit_by_uid(&caster).unwrap().sp_consumed, 25);
}

/// SRC `SpecialPowerData.cs:523-540`: 敵対象 (TargetType 敵/全敵/任意/全) の精神コマンドは、
/// 対象が `スペシャルパワー無効化` / `精神コマンド無効化` を持つと無効化される。脱力 (敵単体・
/// 気力 -10) で検証する。サンプル決戦1話は全敵に `SetStatus スペシャルパワー無効化` を付与し
/// 挑発等の攻撃系精神からザコを守る。
#[test]
fn special_power_nullification_blocks_offensive_spirit() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    // 敵対象 2 体 (無効化あり / なし)。
    let immune = app.database_mut().register_unit(crate::UnitInstance::new(
        "Zako",
        "ZAKO1",
        crate::Party::Enemy,
        5,
        5,
    ));
    let plain = app.database_mut().register_unit(crate::UnitInstance::new(
        "Zako",
        "ZAKO2",
        crate::Party::Enemy,
        6,
        5,
    ));
    app.database_mut()
        .unit_by_uid_mut(&immune)
        .unwrap()
        .add_condition(crate::Condition::new("スペシャルパワー無効化", -1));

    let m_immune = app.database().unit_by_uid(&immune).unwrap().morale;
    let m_plain = app.database().unit_by_uid(&plain).unwrap().morale;
    app.apply_spirit_effect(&immune, "脱力");
    app.apply_spirit_effect(&plain, "脱力");

    // 無効化あり: 気力不変 (攻撃系精神が効かない)。
    assert_eq!(
        app.database().unit_by_uid(&immune).unwrap().morale,
        m_immune,
        "スペシャルパワー無効化 を持つ対象には攻撃系精神 (脱力) が効かないはず"
    );
    // 無効化なし: 脱力で気力 -10。
    assert_eq!(
        app.database().unit_by_uid(&plain).unwrap().morale,
        m_plain - 10,
        "無効化なしの対象には脱力 (気力-10) が効くはず"
    );
}

/// 憑依状態のユニットはスペシャルパワー (精神コマンド) を使用できない
/// (spec「ただしスペシャルパワーは使用できません」)。`spirit_command_options` が空を返す。
#[test]
fn possession_seals_special_power() {
    use crate::data::pilot::SpiritCommand;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = give_spirit_commands(
        &mut app,
        vec![SpiritCommand {
            name: "集中".into(),
            cost: Some(20),
            level: 1,
        }],
        47,
    );
    // 憑依前: 習得済み精神が出る。
    assert_eq!(
        app.spirit_command_options(&uid),
        vec![("集中".to_string(), 20)],
        "憑依前は精神が使える"
    );
    // 憑依を付与すると封じられる。
    let idx = app.database().idx_by_uid(&uid).unwrap();
    app.database_mut().unit_instances[idx].add_condition(crate::Condition::new("憑依", -1));
    assert!(
        app.spirit_command_options(&uid).is_empty(),
        "憑依中は精神コマンドを使用できない"
    );
}

/// 自作SP「生贄」(`イベント=生贄ルーチン`) をプレイヤーが味方に使うと、その味方が
/// 使用者の身代わり (みがわり) になる end-to-end フロー。SRC `SpecialPowerData.cls`
/// Execute の "イベント" 効果。サンプル `data/include.eve` の生贄ルーチン相当を駆動。
#[test]
fn sacrifice_special_power_makes_selected_ally_a_substitute() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 生贄 使用者 (保護される)
    place_player_unit(&mut app, "Ally", 2, 5); // 選んだ味方 (身代わりになる)

    // 生贄 + みがわり の SP データを sp.txt 形式で登録。
    let sp = crate::data::special_power::parse(
        "生贄, いけにえ\n\
             贄, 20, 味方, 即効, -, -, みがわり\n\
             イベント=生贄ルーチン\n\
             \n\
             みがわり\n\
             身, 10, 味方, みがわり, -, -, みがわり\n\
             みがわり\n",
    )
    .unwrap();
    app.database_mut().extend_special_powers(sp);
    // 生贄ルーチン (data/include.eve 相当) を登録。
    let lib = crate::data::event::parse(
        "生贄ルーチン:\nSpecialPower 相手ユニットＩＤ みがわり 対象ユニットＩＤ\nReturn\n",
    )
    .unwrap();
    app.script_library_mut()
        .append_with_name(&lib, "include.eve");

    // 生贄 の対象は「味方単体選択」(SRC 準拠の対象種別是正)。
    assert!(
        matches!(app.spirit_target_kind("生贄"), SpiritTargetKind::SingleAlly),
        "生贄 (対象=味方) は単体味方選択のはず (旧実装は全体と誤判定)"
    );

    let hero = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Hero")
        .map(|u| u.uid.clone())
        .unwrap();
    let ally = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Ally")
        .map(|u| u.uid.clone())
        .unwrap();

    // Hero が Ally を対象に 生贄 を発動 (= 単体選択確定 → apply_spirit_to_target)。
    app.apply_spirit_to_target(&hero, &ally, "生贄", 20);

    // Hero (対象ユニットＩＤ=使用者) に みがわり が付き、身代わりは Ally (相手ユニットＩＤ)。
    let hero_u = app.database().unit_by_uid(&hero).unwrap();
    let cond = hero_u.conditions.iter().find(|c| c.name == "みがわり");
    assert!(
        cond.is_some(),
        "生贄 使用者 Hero に みがわり が付与されていない (イベント=生贄ルーチン 未実行?)"
    );
    assert_eq!(
        cond.unwrap().data,
        ally,
        "みがわり の身代わりが選んだ味方 Ally でない"
    );
}
