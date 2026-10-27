//! アビリティ (回復・補給・変身・召喚・マップ型 等) のテスト。

use super::*;

/// 射程0 アビリティ (回復) は自分に即適用し、回数を消費し行動終了する。
#[test]
fn ability_self_heal_applies_consumes_and_ends_action() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Healer", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 50;
    give_ability(
        &mut app,
        "Healer",
        &uid,
        mk_ability("自己回復", "回復Lv1", 0, Some(2)),
    );

    app.open_unit_menu((2, 6));
    let has = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Ability)
    );
    assert!(has, "アビリティを持つユニットに『アビリティ』が表示される");

    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Ability));
    assert!(app.respond_dialog(1)); // 1 番目のアビリティを選択

    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(u.damage, 0, "回復Lv1 = 500 回復で全快 (最大HP100)");
    assert!(u.has_acted, "アビリティ発動で行動終了");
    assert_eq!(
        u.abilities[0].stock_remaining,
        Some(1),
        "回数が 2→1 に消費される"
    );
}

/// 射程≥1 アビリティは対象選択へ遷移し、クリックした射程内の味方に適用される。
#[test]
fn ability_targeted_heals_ally_in_range() {
    use crate::command_menu::{ActionMode, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Healer", 2, 6);
    place_player_unit(&mut app, "Ally", 4, 6);
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Healer",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let caster = first_player_uid(&app);
    let ally = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 4 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 90;
    give_ability(
        &mut app,
        "Healer",
        &caster,
        mk_ability("治療", "回復Lv2", 3, None),
    );

    app.open_unit_menu((2, 6));
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Ability));
    assert!(app.respond_dialog(1));
    // 射程≥1 → 対象選択モードへ。
    assert!(matches!(
        app.action_mode(),
        ActionMode::AbilityTarget { .. }
    ));

    // 射程内 (距離2) の味方 (4,6) をクリック。
    app.handle_input(Input::ClickAt {
        x: 4 * 32 + 16,
        y: 12 + 6 * 32 + 16,
    });
    assert_eq!(
        app.database().unit_by_uid(&ally).unwrap().damage,
        0,
        "回復Lv2 = 1000 回復で全快"
    );
    assert!(
        app.database().unit_by_uid(&caster).unwrap().has_acted,
        "アビリティ発動で発動主体は行動終了"
    );
}

/// 回数切れアビリティは `×` 付きで表示され、選択しても発動しない。
#[test]
fn ability_out_of_uses_shown_with_x_and_rejected() {
    use crate::command_menu::{MenuActionId, UnitMenuItem};
    use crate::dialog::PendingDialog;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Healer", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 50;
    give_ability(
        &mut app,
        "Healer",
        &uid,
        mk_ability("自己回復", "回復Lv1", 0, Some(0)),
    );

    app.open_unit_menu((2, 6));
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Ability));
    // サブメニューの選択肢に × が付く。
    let x_shown = matches!(
        app.pending_dialog(),
        Some(PendingDialog::Menu { options, .. }) if options[0].starts_with('×')
    );
    assert!(x_shown, "回数切れアビリティは × 付きで表示");
    // 選択しても回復しない (発動拒否)。
    assert!(app.respond_dialog(1));
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().damage,
        50,
        "回数切れは発動しない"
    );
    assert!(
        !app.database().unit_by_uid(&uid).unwrap().has_acted,
        "回数切れは行動を消費しない"
    );
}

/// アビリティを持たないユニットには『アビリティ』を出さない。
#[test]
fn ability_menu_hidden_without_abilities() {
    use crate::command_menu::{CommandMenu, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Plain", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    app.open_unit_menu((2, 6));
    let shown = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Ability)
    );
    assert!(!shown, "アビリティ無しユニットに『アビリティ』は出ない");
}

/// 射程0 の `再行動` アビリティは行動を消費しない (発動後もメニュー再表示)。
#[test]
fn ability_saidoudou_does_not_consume_action() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Mover", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    // 行動済みにしておき、再行動で戻ることを確認。
    app.database_mut().unit_by_uid_mut(&uid).unwrap().has_acted = false;
    give_ability(
        &mut app,
        "Mover",
        &uid,
        mk_ability("再動", "再行動", 0, None),
    );

    app.open_unit_menu((2, 6));
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Ability));
    assert!(app.respond_dialog(1));
    // 再行動で has_acted=false のまま → メニューが再表示される。
    assert!(
        !app.database().unit_by_uid(&uid).unwrap().has_acted,
        "再行動アビリティは行動を消費しない"
    );
    assert!(
        matches!(app.command_menu(), Some(CommandMenu::Unit { .. })),
        "発動後にユニットメニューを再表示する"
    );
}

/// アビリティ効果 霊力回復 / ＳＰ回復 は SP を 10×Lv 回復する。
#[test]
fn ability_sp_restore_effect_reduces_consumed() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Caster", 2, 6);
    let uid = first_player_uid(&app);
    // PILOT に最大SP 50 を与え 30 消費した状態に (PilotInstance 無し → sp_consumed 経路)。
    app.database_mut()
        .catalog
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
        .unwrap()
        .sp = Some(50);
    app.database_mut()
        .unit_by_uid_mut(&uid)
        .unwrap()
        .sp_consumed = 30;
    app.apply_ability_effects(&uid, &uid, "霊力回復Lv2"); // 10×2 = 20 回復
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().sp_consumed,
        10,
        "SP 20 回復で消費 30→10"
    );
}

/// アビリティ効果 変身 は対象を別フォームへ差し替える (set_unit_form を共有)。
#[test]
fn ability_henshin_effect_changes_form() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Human", 2, 6);
    // 変身先 Dragon の UnitData を用意。
    let mut d = app.database().unit_by_name("Human").cloned().unwrap();
    d.name = "Dragon".into();
    app.database_mut().catalog.units.push(d);
    let uid = first_player_uid(&app);
    app.apply_ability_effects(&uid, &uid, "変身Lv3=Dragon");
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().unit_data_name,
        "Dragon",
        "変身で形態が変わる"
    );
}

/// アビリティ効果 召喚Lv* は指定ユニットを Lv 体、隣接マスへ同陣営生成する。
#[test]
fn ability_summon_effect_creates_units() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Summoner", 5, 5);
    // 召喚先 Drone の UnitData (Summoner を複製)。
    let mut d = app.database().unit_by_name("Summoner").cloned().unwrap();
    d.name = "Drone".into();
    app.database_mut().catalog.units.push(d);
    let uid = first_player_uid(&app);
    app.apply_ability_effects(&uid, &uid, "召喚Lv2=Drone");

    let drones: Vec<_> = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| u.unit_data_name == "Drone")
        .collect();
    assert_eq!(drones.len(), 2, "召喚Lv2 で 2 体生成");
    assert!(
        drones
            .iter()
            .all(|d| d.summoned_by.as_deref() == Some(uid.as_str())),
        "summoned_by に親 uid"
    );
    assert!(
        drones.iter().all(|d| d.party == crate::Party::Player),
        "同陣営で生成"
    );
    // 親 (5,5) の隣接に配置される。
    assert!(drones
        .iter()
        .all(|d| (d.x as i32 - 5).abs() <= 1 && (d.y as i32 - 5).abs() <= 1));
}

/// アビリティ効果 強化 は指定特殊能力を一時状態 (condition) として対象へ付与する。
#[test]
fn ability_kyouka_adds_condition() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Booster", 2, 6);
    let uid = first_player_uid(&app);
    app.apply_ability_effects(&uid, &uid, "強化Lv2=切り払い");
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert!(
        u.conditions.iter().any(|c| c.name == "切り払い"),
        "強化で指定能力が状態として付与される"
    );
}

/// アビリティ効果 能力コピー は発動者を対象 (射程内味方) の形態へ変化させる。
/// サイズ 2 段階以上差のユニットへは変化できない (サイズ制限)。
#[test]
fn ability_nouryoku_copy_transforms_caster_with_size_limit() {
    use crate::data::unit::Size;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Copier", 2, 6); // Size::M
    place_player_unit(&mut app, "Model", 4, 6); // Size::M (同サイズ)
    let caster = first_player_uid(&app);
    let model = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 4 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    give_ability(
        &mut app,
        "Copier",
        &caster,
        mk_ability("コピー", "能力コピーLv3", 3, None),
    );
    // 同サイズ・射程内 → 有効な対象。
    assert!(app.ability_target_valid(&caster, 0, &model));
    app.apply_ability(&caster, 0, &model);
    assert_eq!(
        app.database().unit_by_uid(&caster).unwrap().unit_data_name,
        "Model",
        "能力コピーで発動者が対象の形態へ変化"
    );

    // XL サイズの対象 (M とは 3 段階差) は対象不可。
    let mut big = app.database().unit_by_name("Model").cloned().unwrap();
    big.name = "BigModel".into();
    big.size = Size::XL;
    app.database_mut().catalog.units.push(big);
    app.database_mut()
        .unit_by_uid_mut(&model)
        .unwrap()
        .unit_data_name = "BigModel".into();
    assert!(
        !app.ability_target_valid(&caster, 0, &model),
        "サイズ 2 段階以上差の対象には能力コピー不可"
    );
}

/// マップ型アビリティ (Ｍ全) は射程内 (Ｍ全 は盤上全体) の全味方へ効果を及ぼす。
#[test]
fn ability_map_type_heals_all_allies() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Caster", 5, 5);
    place_player_unit(&mut app, "AllyA", 5, 6);
    place_player_unit(&mut app, "AllyB", 6, 5);
    let caster = first_player_uid(&app);
    for pos in [(5u32, 6u32), (6, 5)] {
        let uid = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.x == pos.0 && u.y == pos.1)
            .unwrap()
            .uid
            .clone();
        app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 50;
    }
    let mut ab = mk_ability("全体回復", "回復Lv1", 3, None);
    ab.attributes = "Ｍ全".into();
    give_ability(&mut app, "Caster", &caster, ab);
    app.apply_ability(&caster, 0, &caster);
    for pos in [(5u32, 6u32), (6, 5)] {
        let dmg = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.x == pos.0 && u.y == pos.1)
            .unwrap()
            .damage;
        assert_eq!(dmg, 0, "Ｍ全 回復で全味方が回復する");
    }
}

/// 敵対象アビリティ (脱=気力低下 / 除=特殊効果解除) は味方ではなく敵を対象に取る。
#[test]
fn ability_enemy_target_datsu_and_jo() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Caster", 2, 6);
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Caster",
        "FOE",
        crate::Party::Enemy,
        4,
        6,
    ));
    let caster = first_player_uid(&app);
    let enemy = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Enemy)
        .unwrap()
        .uid
        .clone();
    // 敵に強化系の状態を付けておく (除 で解除される)。
    app.database_mut()
        .unit_by_uid_mut(&enemy)
        .unwrap()
        .add_condition(crate::Condition::new("攻撃力ＵＰ", -1));
    let mut ab = mk_ability("脱力除去弾", "解説", 3, None);
    ab.attributes = "脱 除".into();
    give_ability(&mut app, "Caster", &caster, ab);

    // 脱/除 アビリティは敵が有効対象、味方 (自分) は無効。
    assert!(app.ability_target_valid(&caster, 0, &enemy));
    assert!(!app.ability_target_valid(&caster, 0, &caster));

    let m0 = app.database().unit_by_uid(&enemy).unwrap().morale;
    app.apply_ability(&caster, 0, &enemy);
    let enemy_u = app.database().unit_by_uid(&enemy).unwrap();
    assert_eq!(enemy_u.morale, m0 - 10, "脱で敵気力 -10");
    assert!(
        enemy_u.conditions.is_empty(),
        "除で敵のアビリティ特殊効果を解除"
    );
}
