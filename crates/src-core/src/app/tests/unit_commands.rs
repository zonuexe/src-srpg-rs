//! ユニットコマンド (修理・補給・変形・合体 / 分離・発進 / 格納・カスタムコマンド) のテスト。

use super::*;

/// 損傷した隣接味方が居れば「修理」がメニューに出て、対象クリックで HP 全回復
/// + 発動主体は行動終了する (特殊能力 `修理装置` ベースの組込支援コマンド)。
#[test]
fn support_repair_heals_adjacent_ally_and_ends_action() {
    use crate::command_menu::{ActionMode, CommandMenu, MenuActionId, SupportKind, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Medic", 2, 6);
    place_player_unit(&mut app, "Ally", 3, 6);
    // 勝敗即決を避けるため遠方に敵を配置。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Medic",
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
        .find(|u| u.x == 3 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features
        .push(crate::feature::ActiveFeature::new("修理装置", ""));
    app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 50;

    // メニューに「修理」が出る。
    app.open_unit_menu((2, 6));
    let has_repair = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. })
            if items.contains(&UnitMenuItem::Support(SupportKind::Repair))
    );
    assert!(has_repair, "修理装置 + 隣接損傷味方 で『修理』が表示される");

    // 選択 → 対象選択モードへ。
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Support(
        SupportKind::Repair,
    )));
    assert!(matches!(
        app.action_mode(),
        ActionMode::SupportTarget {
            kind: SupportKind::Repair,
            ..
        }
    ));

    // 隣接味方タイル (3,6) をクリック → HP 回復 (修理装置 Lv なし=30%) + 行動終了。
    app.handle_input(Input::ClickAt {
        x: 3 * 32 + 16,
        y: 12 + 6 * 32 + 16,
    });
    assert_eq!(
        app.database().unit_by_uid(&ally).unwrap().damage,
        20,
        "修理装置 Lv なし=30% (最大HP100 → 30 回復) で damage 50→20"
    );
    assert!(
        app.database().unit_by_uid(&caster).unwrap().has_acted,
        "修理発動で行動終了"
    );
    assert!(matches!(app.action_mode(), ActionMode::Browse));
}

/// 補給は対象の EN・残弾を全回復し、対象の気力を 10 下げる (原典準拠)。
#[test]
fn support_supply_restores_en_and_lowers_morale() {
    use crate::command_menu::SupportKind;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Ally", 3, 6);
    let ally = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&ally).unwrap();
        u.en_consumed = 30;
        u.morale = 100;
    }
    app.apply_support_to_target(&ally, &ally, SupportKind::Supply);
    let u = app.database().unit_by_uid(&ally).unwrap();
    assert_eq!(u.en_consumed, 0, "補給で EN 全回復");
    assert_eq!(u.morale, 90, "補給で対象の気力 -10");
}

/// 修理装置の回復量はレベルに比例する (Lv3 = 100%)。
#[test]
fn repair_amount_scales_with_device_level() {
    use crate::command_menu::SupportKind;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Medic", 2, 6);
    place_player_unit(&mut app, "Ally", 3, 6);
    let caster = first_player_uid(&app);
    let ally = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 3 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    // 修理装置Lv3 = 100% 回復。
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new("修理装置Lv3", "")];
    app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 80;
    app.apply_support_to_target(&caster, &ally, SupportKind::Repair);
    assert_eq!(
        app.database().unit_by_uid(&ally).unwrap().damage,
        0,
        "修理装置Lv3=100% で全回復 (damage 80→0)"
    );
}

/// 修理 / 補給 を行ったユニットは経験値を得る。
#[test]
fn support_grants_exp_to_caster() {
    use crate::command_menu::SupportKind;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Medic", 2, 6);
    place_player_unit(&mut app, "Ally", 3, 6);
    let caster = first_player_uid(&app);
    let ally = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 3 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new("修理装置", "")];
    app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 50;
    let exp0 = app.database().unit_by_uid(&caster).unwrap().total_exp;
    app.apply_support_to_target(&caster, &ally, SupportKind::Repair);
    assert_eq!(
        app.database().unit_by_uid(&caster).unwrap().total_exp,
        exp0 + 100,
        "同レベル対象の修理で経験値 +100 (SRC 基準値)"
    );
}

/// 修理 / 補給 経験値の対象レベル差倍率 (SRC `Unit.cs::GetExp`)。
#[test]
fn support_exp_level_diff_table() {
    // 同レベル → 基準値。
    assert_eq!(support_exp_with_level_diff(10, 5, 5), 10);
    // 対象 +2 → ×2、+8 (>7) → ×5。
    assert_eq!(support_exp_with_level_diff(10, 7, 5), 20);
    assert_eq!(support_exp_with_level_diff(10, 13, 5), 50);
    // 対象が低レベル → 逓減 (-1 で ÷2)。
    assert_eq!(support_exp_with_level_diff(10, 4, 5), 5);
    // 補給基準 15 で +1 → ×1.5 = 22 (整数)。
    assert_eq!(support_exp_with_level_diff(15, 6, 5), 22);
    // 最低 1。
    assert_eq!(support_exp_with_level_diff(10, 0, 99), 1);
}

/// 統合: 高レベルの対象を修理すると経験値が増える (caster lv1, target lv5 → ×3)。
#[test]
fn support_exp_more_for_higher_level_target() {
    use crate::command_menu::SupportKind;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Medic", 2, 6);
    place_player_unit(&mut app, "Ally", 3, 6);
    let caster = first_player_uid(&app);
    let ally = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 3 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new("修理装置", "")];
    // 対象を高レベル (total_exp 2000 → level 5、SRC: 500 exp/level) に。
    {
        let a = app.database_mut().unit_by_uid_mut(&ally).unwrap();
        a.total_exp = 2000;
        a.damage = 50;
    }
    let exp0 = app.database().unit_by_uid(&caster).unwrap().total_exp;
    app.apply_support_to_target(&caster, &ally, SupportKind::Repair);
    let gained = app.database().unit_by_uid(&caster).unwrap().total_exp - exp0;
    assert_eq!(gained, 300, "高レベル対象 (+4) の修理で経験値 100×3=300");
}

/// 特殊能力が無い / 隣接に要支援の味方が居ないときは支援コマンドを出さない。
#[test]
fn support_command_hidden_without_feature_or_need() {
    use crate::command_menu::{CommandMenu, SupportKind, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Medic", 2, 6);
    place_player_unit(&mut app, "Ally", 3, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    let caster = first_player_uid(&app);

    // 特殊能力なし → 支援コマンドは出ない。
    app.open_unit_menu((2, 6));
    let shown = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. })
            if items.iter().any(|i| matches!(i, UnitMenuItem::Support(_)))
    );
    assert!(!shown, "修理装置を持たないユニットに支援コマンドは出ない");

    // 修理装置はあるが隣接味方が無傷 → 対象が無いので出ない。
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features
        .push(crate::feature::ActiveFeature::new("修理装置", ""));
    app.open_unit_menu((2, 6));
    let shown2 = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. })
            if items.contains(&UnitMenuItem::Support(SupportKind::Repair))
    );
    assert!(!shown2, "隣接に要修理の味方が居なければ『修理』は出ない");
}

/// 対象選択をキャンセルしても効果は発生せず、行動も消費しない。
#[test]
fn support_target_cancel_has_no_effect() {
    use crate::command_menu::{ActionMode, SupportKind};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Medic", 2, 6);
    place_player_unit(&mut app, "Ally", 3, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    let caster = first_player_uid(&app);
    let ally = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 3 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features
        .push(crate::feature::ActiveFeature::new("修理装置", ""));
    app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 50;

    app.begin_support_target(&caster, SupportKind::Repair);
    assert!(matches!(
        app.action_mode(),
        ActionMode::SupportTarget { .. }
    ));
    // 右クリック相当でキャンセル。
    assert!(app.cancel_action());
    assert!(matches!(app.action_mode(), ActionMode::Browse));
    assert_eq!(
        app.database().unit_by_uid(&ally).unwrap().damage,
        50,
        "キャンセルで回復しない"
    );
    assert!(
        !app.database().unit_by_uid(&caster).unwrap().has_acted,
        "キャンセルで行動終了しない"
    );
}

/// 変形先を 1 つ持つユニットはメニューの「変形」で即変形し、行動は消費しない。
#[test]
fn transform_single_form_switches_unit_immediately() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "FormA", 2, 6);
    // 変形先 FormB の UnitData を用意 (FormA を複製してリネーム)。
    let mut b = app.database().unit_by_name("FormA").cloned().unwrap();
    b.name = "FormB".into();
    app.database_mut().catalog.units.push(b);
    app.set_stage_state(crate::stage::StageState::Battle);
    let caster = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new("変形", "変形 FormB")];

    app.open_unit_menu((2, 6));
    let has = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Transform)
    );
    assert!(has, "変形先を持つユニットに『変形』が表示される");

    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Transform));
    assert_eq!(
        app.database().unit_by_uid(&caster).unwrap().unit_data_name,
        "FormB",
        "単一形態は即変形する"
    );
    assert!(
        !app.database().unit_by_uid(&caster).unwrap().has_acted,
        "変形は行動を消費しない"
    );
}

/// 変形先が複数あるとサブメニューが出て、選んだ形態に変形する。
#[test]
fn transform_multi_form_opens_submenu_and_switches() {
    use crate::command_menu::{MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "FormA", 2, 6);
    for n in ["FormB", "FormC"] {
        let mut d = app.database().unit_by_name("FormA").cloned().unwrap();
        d.name = n.into();
        app.database_mut().catalog.units.push(d);
    }
    app.set_stage_state(crate::stage::StageState::Battle);
    let caster = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new(
        "変形",
        "変形 FormB FormC",
    )];

    app.open_unit_menu((2, 6));
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Transform));
    // サブメニューが開く (まだ変形していない)。
    assert_eq!(
        app.database().unit_by_uid(&caster).unwrap().unit_data_name,
        "FormA"
    );
    // 2 番目 (FormC) を選ぶ。
    assert!(app.respond_dialog(2));
    assert_eq!(
        app.database().unit_by_uid(&caster).unwrap().unit_data_name,
        "FormC",
        "選んだ形態に変形する"
    );
}

/// 変形先サブメニューをキャンセルすると形態は変わらない。
#[test]
fn transform_submenu_cancel_keeps_form() {
    use crate::command_menu::{MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "FormA", 2, 6);
    for n in ["FormB", "FormC"] {
        let mut d = app.database().unit_by_name("FormA").cloned().unwrap();
        d.name = n.into();
        app.database_mut().catalog.units.push(d);
    }
    app.set_stage_state(crate::stage::StageState::Battle);
    let caster = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new(
        "変形",
        "変形 FormB FormC",
    )];
    app.open_unit_menu((2, 6));
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Transform));
    assert!(app.respond_dialog(0)); // キャンセル
    assert_eq!(
        app.database().unit_by_uid(&caster).unwrap().unit_data_name,
        "FormA",
        "キャンセルで変形しない"
    );
}

/// 変形特殊能力が無い / 移動後 のときは「変形」を出さない (移動前のみ)。
#[test]
fn transform_hidden_post_move_or_without_feature() {
    use crate::command_menu::{CommandMenu, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "FormA", 2, 6);
    let mut b = app.database().unit_by_name("FormA").cloned().unwrap();
    b.name = "FormB".into();
    app.database_mut().catalog.units.push(b);
    app.set_stage_state(crate::stage::StageState::Battle);
    let caster = first_player_uid(&app);

    // 変形 特殊能力なし → 出ない。
    app.open_unit_menu((2, 6));
    let absent = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if !items.contains(&UnitMenuItem::Transform)
    );
    assert!(absent, "変形 特殊能力なしでは出ない");

    // 変形を付与するが移動後 → 出ない (移動前のみ)。
    {
        let u = app.database_mut().unit_by_uid_mut(&caster).unwrap();
        u.active_features = vec![crate::feature::ActiveFeature::new("変形", "変形 FormB")];
        u.has_moved = true;
    }
    app.open_unit_menu((2, 6));
    let absent2 = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if !items.contains(&UnitMenuItem::Transform)
    );
    assert!(absent2, "移動後は変形を出さない");
}

/// チャージ攻撃 (Ｃ属性) 武器を持つユニットには「チャージ」が出て、選択で
/// charged フラグが立ち行動終了する。武器を持たないユニットには出ない。
#[test]
fn charge_command_sets_flag_and_ends_action() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Charger", 2, 6);
    place_player_unit(&mut app, "Plain", 4, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    // Charger の UnitData にチャージ攻撃 (Ｃ属性) 武器を追加。
    {
        let d = app
            .database_mut()
            .catalog
            .units
            .iter_mut()
            .find(|d| d.name == "Charger")
            .unwrap();
        d.weapons.push(crate::data::unit::WeaponData {
            name: "チャージ砲".into(),
            power: 1000,
            min_range: 1,
            max_range: 3,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: "Ｃ".into(),
            extras: Vec::new(),
        });
    }
    let charger = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 2 && u.y == 6)
        .unwrap()
        .uid
        .clone();

    // チャージ砲を持つユニットには「チャージ」が出る。
    app.open_unit_menu((2, 6));
    let has = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Charge)
    );
    assert!(has, "Ｃ属性武器を持つユニットに『チャージ』が表示される");

    // チャージ砲を持たないユニットには出ない。
    app.open_unit_menu((4, 6));
    let absent = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if !items.contains(&UnitMenuItem::Charge)
    );
    assert!(absent, "Ｃ属性武器の無いユニットには『チャージ』は出ない");

    // 発動で charged フラグが立ち、行動終了する。
    app.open_unit_menu((2, 6));
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Charge));
    let u = app.database().unit_by_uid(&charger).unwrap();
    assert!(u.charged, "チャージで charged フラグが立つ");
    assert!(u.has_acted, "チャージで行動終了する");

    // 既にチャージ済みなら再表示しない。
    app.database_mut()
        .unit_by_uid_mut(&charger)
        .unwrap()
        .has_acted = false;
    app.open_unit_menu((2, 6));
    let absent2 = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if !items.contains(&UnitMenuItem::Charge)
    );
    assert!(
        absent2,
        "チャージ済みユニットには『チャージ』を再表示しない"
    );
}

/// 精神「突撃」は移動後でも長射程武器 (マップ攻撃を除く) を使用可能にする。
#[test]
fn totsugeki_allows_long_range_weapon_post_move() {
    let mk = |max_range: i32, class: &str| crate::data::unit::WeaponData {
        name: "W".into(),
        power: 1000,
        min_range: 1,
        max_range,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: class.into(),
        extras: Vec::new(),
    };
    let long = mk(3, ""); // 長射程・属性なし
    let map = mk(3, "Ｍ"); // マップ攻撃

    // 移動前は常に使用可。
    assert!(App::weapon_usable_post_move(&long, false, false));
    // 移動後: 通常は長射程不可。突撃でのみ可。
    assert!(!App::weapon_usable_post_move(&long, true, false));
    assert!(App::weapon_usable_post_move(&long, true, true));
    // 突撃でもマップ攻撃 (Ｍ) は移動後不可。
    assert!(!App::weapon_usable_post_move(&map, true, true));
}

/// 母艦: 搭載 (fire_boarding_event) で格納リンクが張られ、発進で隣接マスへ出撃する。
#[test]
fn carrier_board_and_launch() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Carrier", 5, 5);
    place_player_unit(&mut app, "Fighter", 6, 5);
    app.set_stage_state(crate::stage::StageState::Battle);
    let carrier = first_player_uid(&app);
    let fighter = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Fighter")
        .unwrap()
        .uid
        .clone();
    let carrier_idx = app.database().idx_by_uid(&carrier).unwrap();
    let fighter_idx = app.database().idx_by_uid(&fighter).unwrap();

    // 搭載: 相互リンク + off_map。
    app.fire_boarding_event(fighter_idx, carrier_idx);
    let f = app.database().unit_by_uid(&fighter).unwrap();
    assert!(f.off_map, "格納で off_map");
    assert_eq!(f.stored_in.as_deref(), Some(carrier.as_str()));
    assert!(app
        .database()
        .unit_by_uid(&carrier)
        .unwrap()
        .stored_units
        .contains(&fighter));

    // 発進: メニューに「発進」が出る。
    app.open_unit_menu((5, 5));
    let has_launch = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Launch)
    );
    assert!(has_launch, "格納ユニットがあれば『発進』が出る");

    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Launch));
    assert!(app.respond_dialog(1)); // 1 番目の格納ユニット

    // 発進は着地点選択 (移動範囲から対話選択) へ移行する。配置はまだ。
    let cand = match app.action_mode() {
        crate::command_menu::ActionMode::LandingSelect { candidates, .. } => {
            assert!(!candidates.is_empty(), "着地候補がある");
            // 母艦 (5,5) は占有なので候補外、移動範囲内の空きマスが候補。
            assert!(!candidates.contains(&(5, 5)), "母艦マスは候補に含まない");
            candidates[0]
        }
        other => panic!("着地点選択へ遷移すべき: {other:?}"),
    };
    assert!(
        app.database().unit_by_uid(&fighter).unwrap().off_map,
        "着地点選択中はまだ off_map"
    );
    // 候補外クリックは無反応。
    assert!(!app.confirm_landing(5, 5), "候補外 (母艦マス) は確定しない");
    assert!(app.database().unit_by_uid(&fighter).unwrap().off_map);
    // 候補マスを確定 → 配置。
    assert!(app.confirm_landing(cand.0, cand.1), "候補マスで配置確定");

    let f = app.database().unit_by_uid(&fighter).unwrap();
    assert!(!f.off_map, "発進で盤上へ復帰");
    assert!(f.stored_in.is_none());
    assert!(
        app.database()
            .unit_by_uid(&carrier)
            .unwrap()
            .stored_units
            .is_empty(),
        "格納リストから外れる"
    );
    assert_eq!((f.x, f.y), cand, "選択した候補マスに配置される");
}

/// 発進の着地点選択を右クリック/Esc でキャンセルすると、ユニットは格納のまま残る。
#[test]
fn launch_landing_select_cancel_keeps_stored() {
    use crate::command_menu::{ActionMode, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Carrier", 5, 5);
    place_player_unit(&mut app, "Fighter", 6, 5);
    app.set_stage_state(crate::stage::StageState::Battle);
    let carrier = first_player_uid(&app);
    let fighter = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Fighter")
        .unwrap()
        .uid
        .clone();
    let ci = app.database().idx_by_uid(&carrier).unwrap();
    let fi = app.database().idx_by_uid(&fighter).unwrap();
    app.fire_boarding_event(fi, ci);
    app.open_unit_menu((5, 5));
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Launch));
    assert!(app.respond_dialog(1));
    assert!(matches!(
        app.action_mode(),
        ActionMode::LandingSelect { .. }
    ));

    // キャンセル (Esc/右クリック相当) → 格納のまま・Browse へ戻る。
    assert!(app.handle_input(Input::Cancel));
    assert!(matches!(app.action_mode(), ActionMode::Browse));
    let f = app.database().unit_by_uid(&fighter).unwrap();
    assert!(f.off_map, "キャンセルで格納のまま");
    assert_eq!(f.stored_in.as_deref(), Some(carrier.as_str()));
    assert!(
        app.database()
            .unit_by_uid(&carrier)
            .unwrap()
            .stored_units
            .contains(&fighter),
        "格納リストに残る"
    );
}

/// 母艦格納中ユニットは当該陣営フェイズ開始時に HP/EN を 50% 回復する。
#[test]
fn carrier_stored_unit_recovers_each_turn() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Carrier", 5, 5);
    place_player_unit(&mut app, "Fighter", 6, 5);
    // 勝敗即決回避の敵。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Carrier",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let carrier = first_player_uid(&app);
    let fighter = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Fighter")
        .unwrap()
        .uid
        .clone();
    let carrier_idx = app.database().idx_by_uid(&carrier).unwrap();
    let fighter_idx = app.database().idx_by_uid(&fighter).unwrap();
    {
        let f = app.database_mut().unit_by_uid_mut(&fighter).unwrap();
        f.damage = 60; // 最大HP 100
        f.en_consumed = 40; // 最大EN 50
    }
    app.fire_boarding_event(fighter_idx, carrier_idx);

    // 1 周して味方フェイズ T2 開始 → 母艦回復。
    app.handle_input(Input::EndPhase);
    let f = app.database().unit_by_uid(&fighter).unwrap();
    assert_eq!(f.damage, 10, "HP 50% (50) 回復で damage 60→10");
    assert_eq!(
        f.en_consumed, 10,
        "基礎EN自然回復 (+5, 40→35) の後さらに母艦格納 EN 50% (最大EN50の50%=25) 回復で 35→10"
    );
}

/// 3 体合体: 相手が全て 2 マス以内なら『合体』が出て、実行で合体形態へ変身し
/// 相手は全員 off_map で温存・行動終了する (原典: 3 体以上は合体コマンド)。
#[test]
fn combine_absorbs_partners_into_form() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    let (host, partner) = setup_combine(&mut app);
    let partner2 = extend_combine_to_triple(&mut app, &host, (2, 3));

    app.open_unit_menu((3, 3));
    let has = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Combine)
    );
    assert!(has, "3 体合体で相手が全て 2 マス以内なら『合体』が出る");

    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Combine));
    let h = app.database().unit_by_uid(&host).unwrap();
    assert_eq!(h.unit_data_name, "GetterRobo", "host が合体形態へ変身");
    assert!(
        h.combined_from.contains(&partner) && h.combined_from.contains(&partner2),
        "両方の相手を構成に記録"
    );
    assert_eq!(h.pre_combine_form.as_deref(), Some("GetterEagle"));
    assert!(h.has_acted, "合体で行動終了");
    assert!(
        app.database().unit_by_uid(&partner).unwrap().off_map,
        "合体相手 1 は off_map で温存"
    );
    assert!(
        app.database().unit_by_uid(&partner2).unwrap().off_map,
        "合体相手 2 は off_map で温存"
    );
}

/// 合体形態の必要技能 (`必要技能.md` §4): 構成員が満たさなければ合体できない。
#[test]
fn combine_respects_form_necessary_skill() {
    let mut app = App::new();
    let (host, _partner) = setup_combine(&mut app);
    // 合体形態 GetterRobo に 必要技能=念力Lv3 を付与。
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|d| d.name == "GetterRobo")
        .unwrap()
        .features
        .push(("必要技能".into(), "念力Lv3".into()));
    // host に念力なしパイロットを紐付け。
    app.database_mut().create_pilot_instance("PILOT", "p_host");
    app.database_mut().unit_by_uid_mut(&host).unwrap().pilot_ids = vec!["p_host".into()];
    // 念力なし → 合体不可 (host は GetterEagle のまま、相手も on_map)。
    app.apply_combine(&host);
    assert_eq!(
        app.database().unit_by_uid(&host).unwrap().unit_data_name,
        "GetterEagle",
        "念力なしで合体不可"
    );
    // 念力Lv3 を習得 → 合体成功。
    app.database_mut()
        .pilot_instance_by_id_mut("p_host")
        .unwrap()
        .skills
        .push("念力Lv3".into());
    app.apply_combine(&host);
    assert_eq!(
        app.database().unit_by_uid(&host).unwrap().unit_data_name,
        "GetterRobo",
        "念力Lv3 で合体成功"
    );
}

/// 2 体合体は合体コマンドを使わず移動で合体する (原典 合体.md)。
/// → ユニットメニューに『合体』は出さない (移動経路は別テストで検証)。
#[test]
fn two_unit_combine_hides_menu_item() {
    use crate::command_menu::{CommandMenu, UnitMenuItem};
    let mut app = App::new();
    let _ = setup_combine(&mut app); // 2 体合体 (相手 1 機)
    app.open_unit_menu((3, 3));
    let has = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Combine)
    );
    assert!(!has, "2 体合体では『合体』コマンドを出さない (移動で合体)");
}

/// 3 体合体で相手の 1 機が 2 マス以内に居なければ『合体』は出ない
/// (原典: 合体に参加するユニット全てが 2 マス以内に居る場合のみ)。
#[test]
fn three_unit_combine_requires_all_partners_present() {
    use crate::command_menu::{CommandMenu, UnitMenuItem};
    let mut app = App::new();
    let (host, _partner) = setup_combine(&mut app);
    // 3 体目 (GetterBear) を遠方 (2 マス超) に置く。
    let _ = extend_combine_to_triple(&mut app, &host, (10, 10));
    app.open_unit_menu((3, 3));
    let has = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Combine)
    );
    assert!(!has, "相手が全員 2 マス以内に揃わなければ合体不可");
}

/// 分離: 構成ユニットを隣接マスへ復帰させ、host を合体前形態へ戻す (行動非消費)。
#[test]
fn separate_restores_components_and_reverts_form() {
    let mut app = App::new();
    let (host, partner) = setup_combine(&mut app);
    app.apply_combine(&host);
    assert!(app.database().unit_by_uid(&partner).unwrap().off_map);

    app.apply_separate(&host);
    // host 形態/構成リストは分離時点で確定 (着地点選択とは独立)。
    let h = app.database().unit_by_uid(&host).unwrap();
    assert_eq!(h.unit_data_name, "GetterEagle", "host が合体前形態へ戻る");
    assert!(h.combined_from.is_empty(), "構成リストがクリアされる");
    assert!(h.pre_combine_form.is_none());

    // 構成ユニットは着地点選択 (LandingSelect) へ移行する (移動範囲から対話選択)。
    let cand = match app.action_mode() {
        crate::command_menu::ActionMode::LandingSelect {
            uid, candidates, ..
        } => {
            assert_eq!(uid, partner, "構成ユニットの着地を選択");
            assert!(!candidates.is_empty());
            candidates[0]
        }
        other => panic!("着地点選択へ遷移すべき: {other:?}"),
    };
    assert!(
        app.database().unit_by_uid(&partner).unwrap().off_map,
        "選択中はまだ off_map"
    );
    assert!(app.confirm_landing(cand.0, cand.1), "候補マスで配置確定");

    let p = app.database().unit_by_uid(&partner).unwrap();
    assert!(!p.off_map, "構成ユニットが盤上へ復帰");
    assert_eq!((p.x, p.y), cand, "選択した候補マスに配置される");
}

/// 分離 (3 体合体) の着地点選択キュー: 構成ユニットを 1 機ずつ順次配置し、最後に Browse へ。
#[test]
fn separate_landing_queue_places_components_sequentially() {
    use crate::command_menu::ActionMode;
    let mut app = App::new();
    let (host, partner1) = setup_combine(&mut app);
    let partner2 = extend_combine_to_triple(&mut app, &host, (4, 4));
    app.apply_combine(&host);
    assert!(app.database().unit_by_uid(&partner1).unwrap().off_map);
    assert!(app.database().unit_by_uid(&partner2).unwrap().off_map);

    app.apply_separate(&host);
    // 1 機目の着地点選択。
    let (uid1, cand1) = match app.action_mode() {
        ActionMode::LandingSelect {
            uid, candidates, ..
        } => (uid, candidates[0]),
        other => panic!("1 機目の着地点選択へ: {other:?}"),
    };
    assert!(app.confirm_landing(cand1.0, cand1.1));
    // キューが進み 2 機目の着地点選択へ。
    let (uid2, cand2) = match app.action_mode() {
        ActionMode::LandingSelect {
            uid, candidates, ..
        } => {
            assert_ne!(uid, uid1, "2 機目へ進む");
            assert!(!candidates.contains(&cand1), "配置済みマスは候補から除外");
            (uid, candidates[0])
        }
        other => panic!("2 機目の着地点選択へ: {other:?}"),
    };
    let _ = uid2;
    assert!(app.confirm_landing(cand2.0, cand2.1));
    // 全機配置完了 → Browse。
    assert!(
        matches!(app.action_mode(), ActionMode::Browse),
        "完了で Browse"
    );
    let p1 = app.database().unit_by_uid(&partner1).unwrap();
    let p2 = app.database().unit_by_uid(&partner2).unwrap();
    assert!(!p1.off_map && !p2.off_map, "両構成ユニットが盤上へ");
    assert_ne!((p1.x, p1.y), (p2.x, p2.y), "別マスに配置される");
}

/// 合体時のパイロット統合: 構成ユニットのパイロットが合体形態へ搭乗 (全員搭乗)、
/// 分離で各機の搭乗構成へ戻る。
#[test]
fn combine_integrates_pilots_and_separate_restores() {
    let mut app = App::new();
    let (host, partner) = setup_combine(&mut app);
    // host / partner に区別可能な pilot_ids を設定。
    app.database_mut().unit_by_uid_mut(&host).unwrap().pilot_ids = vec!["host_p".to_string()];
    app.database_mut()
        .unit_by_uid_mut(&partner)
        .unwrap()
        .pilot_ids = vec!["partner_p".to_string()];

    app.apply_combine(&host);
    let h = app.database().unit_by_uid(&host).unwrap();
    assert!(
        h.pilot_ids.contains(&"host_p".to_string()),
        "host のパイロットを維持"
    );
    assert!(
        h.pilot_ids.contains(&"partner_p".to_string()),
        "相手のパイロットが合体形態へ搭乗"
    );
    assert_eq!(
        h.pre_combine_pilots,
        vec!["host_p".to_string()],
        "合体前の搭乗構成を保持"
    );

    app.apply_separate(&host);
    let h = app.database().unit_by_uid(&host).unwrap();
    assert_eq!(
        h.pilot_ids,
        vec!["host_p".to_string()],
        "分離で host の搭乗構成が復帰"
    );
    assert!(h.pre_combine_pilots.is_empty(), "統合状態がクリアされる");
    let p = app.database().unit_by_uid(&partner).unwrap();
    assert_eq!(
        p.pilot_ids,
        vec!["partner_p".to_string()],
        "相手のパイロットが各機へ復帰"
    );
}

/// 母艦上へ移動すると搭載される (ムーブ統合)。
#[test]
fn board_carrier_by_moving_onto_it() {
    use crate::command_menu::ActionMode;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Carrier", 5, 5);
    place_player_unit(&mut app, "Fighter", 5, 6); // 母艦に隣接
    app.set_stage_state(crate::stage::StageState::Battle);
    let carrier = first_player_uid(&app);
    let fighter = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Fighter")
        .unwrap()
        .uid
        .clone();
    app.database_mut()
        .unit_by_uid_mut(&carrier)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new("母艦", "")];

    // Fighter を移動モードにして母艦タイル (5,5) をクリック。
    app.action_mode = ActionMode::MoveSelect {
        uid: fighter.clone(),
    };
    app.handle_input(Input::ClickAt {
        x: 5 * 32 + 16,
        y: 12 + 5 * 32 + 16,
    });

    assert!(
        app.database().unit_by_uid(&fighter).unwrap().off_map,
        "母艦上へ移動で搭載 (off_map)"
    );
    assert!(app
        .database()
        .unit_by_uid(&carrier)
        .unwrap()
        .stored_units
        .contains(&fighter));
}

/// 合体相手の上へ移動すると合体する (2 機合体のムーブ統合)。
#[test]
fn combine_by_moving_onto_partner() {
    use crate::command_menu::ActionMode;
    let mut app = App::new();
    let (host, partner) = setup_combine(&mut app);
    // partner (GetterJaguar) を移動モードにして host (GetterEagle, 3,3) をクリック。
    // host が 合体 特殊能力を持ち相手リストに partner の形態を含むので、host が
    // 合体形態 (GetterRobo) になり partner が温存される。
    app.action_mode = ActionMode::MoveSelect {
        uid: partner.clone(),
    };
    app.handle_input(Input::ClickAt {
        x: 3 * 32 + 16,
        y: 12 + 3 * 32 + 16,
    });

    assert_eq!(
        app.database().unit_by_uid(&host).unwrap().unit_data_name,
        "GetterRobo",
        "相手上へ移動で合体形態へ"
    );
    assert!(
        app.database().unit_by_uid(&partner).unwrap().off_map,
        "mover は構成ユニットとして温存"
    );
}

/// 最初から合体形態で配置されたユニット (構成 uid 無し) の分離: `分離` 特殊能力の
/// 形態へ分離し、host が形態1 になり形態2 以降が隣接へ新規生成される。パイロットは
/// 形態順に乗り移る。分離は行動を消費しない。
#[test]
fn separate_from_initial_combined_form_spawns_components() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "CombinedForm", 5, 5);
    // 分離後の形態 FormA / FormB の UnitData を用意 (インスタンスは作らない)。
    for form in ["FormA", "FormB"] {
        let mut ud = app
            .database()
            .unit_by_name("CombinedForm")
            .cloned()
            .unwrap();
        ud.name = form.into();
        app.database_mut().catalog.units.push(ud);
    }
    app.set_stage_state(crate::stage::StageState::Battle);
    let host = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&host).unwrap();
        // 構成 uid は無く、`分離=<名称> <形態1> <形態2>` だけを持つ。
        u.active_features = vec![crate::feature::ActiveFeature::new(
            "分離",
            "分離 FormA FormB",
        )];
        u.pilot_ids = vec!["pa".into(), "pb".into()];
    }

    // メニューに『分離』が出る (combined_from が空でも `分離` 特殊能力で表示)。
    app.open_unit_menu((5, 5));
    let has = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Separate)
    );
    assert!(has, "`分離` 特殊能力を持つなら『分離』が出る");

    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Separate));
    // host は形態1 (FormA) へ、行動は消費しない。
    let h = app.database().unit_by_uid(&host).unwrap();
    assert_eq!(h.unit_data_name, "FormA", "host が形態1 へ");
    assert!(!h.has_acted, "分離は行動を消費しない");
    assert_eq!(
        h.pilot_ids,
        vec!["pa".to_string()],
        "形態1 は主パイロットを保持"
    );
    // 形態2 (FormB) が host 隣接に新規生成され、形態順にパイロットが乗り移る。
    let fb = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "FormB")
        .expect("形態2 が新規生成される");
    assert!(
        (fb.x as i32 - 5).abs() <= 1 && (fb.y as i32 - 5).abs() <= 1,
        "形態2 は host (5,5) 隣接に生成: ({},{})",
        fb.x,
        fb.y
    );
    assert_eq!(
        fb.pilot_ids,
        vec!["pb".to_string()],
        "形態順に 2 番目のパイロットが形態2 へ"
    );
}

/// 形態の必要技能 (`必要技能=` / `不必要技能=`): 搭乗員が満たす形態のみ変形/換装可。
#[test]
fn form_necessary_skill_gates_transform() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "BaseForm", 2, 6);
    place_player_unit(&mut app, "SuperForm", 4, 6); // UnitData 用途
                                                    // SuperForm に 必要技能=念力Lv3、Forbidden に 不必要技能=機械 を付与。
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|d| d.name == "SuperForm")
        .unwrap()
        .features
        .push(("必要技能".into(), "念力Lv3".into()));
    let base = first_player_uid(&app);
    let idx = app.database().idx_by_uid(&base).unwrap();
    app.database_mut().create_pilot_instance("PILOT", "p1");
    app.database_mut().unit_instances[idx].pilot_ids = vec!["p1".into()];
    // 念力なし → SuperForm へ変形/換装不可。
    assert!(
        !app.form_skill_ok(idx, "SuperForm"),
        "念力なしで SuperForm 不可"
    );
    // 念力Lv3 習得 → 可。
    app.database_mut()
        .pilot_instance_by_id_mut("p1")
        .unwrap()
        .skills
        .push("念力Lv3".into());
    assert!(
        app.form_skill_ok(idx, "SuperForm"),
        "念力Lv3 で SuperForm 可"
    );
    // 必要技能宣言の無い形態は常に可。
    assert!(app.form_skill_ok(idx, "BaseForm"));
}

/// 乗り換えは形態の必要技能を満たすパイロットのみ。満たさない組合せは入れ替え不成立 (revert)。
#[test]
fn ride_change_respects_form_necessary_skill() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Normal", 2, 6);
    place_player_unit(&mut app, "Psychic", 3, 6); // 必要技能=念力Lv3
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|d| d.name == "Psychic")
        .unwrap()
        .features
        .push(("必要技能".into(), "念力Lv3".into()));
    let ua = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Normal")
        .unwrap()
        .uid
        .clone();
    let ub = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Psychic")
        .unwrap()
        .uid
        .clone();
    app.database_mut().create_pilot_instance("PILOT", "plain");
    app.database_mut().create_pilot_instance("PILOT", "esper");
    app.database_mut()
        .pilot_instance_by_id_mut("esper")
        .unwrap()
        .skills
        .push("念力Lv3".into());
    app.database_mut().unit_by_uid_mut(&ua).unwrap().pilot_ids = vec!["plain".into()];
    app.database_mut().unit_by_uid_mut(&ub).unwrap().pilot_ids = vec!["esper".into()];
    // plain は念力なし → Psychic に乗れず乗り換え不成立 (revert)。
    app.apply_ride_change(&ua, &ub);
    assert_eq!(
        app.database().unit_by_uid(&ua).unwrap().pilot_ids,
        vec!["plain".to_string()],
        "念力なしは Psychic に乗れず乗り換え不成立"
    );
    assert_eq!(
        app.database().unit_by_uid(&ub).unwrap().pilot_ids,
        vec!["esper".to_string()]
    );
    // plain に念力Lv3 → 双方適格 → 入れ替え成立。
    app.database_mut()
        .pilot_instance_by_id_mut("plain")
        .unwrap()
        .skills
        .push("念力Lv3".into());
    app.apply_ride_change(&ua, &ub);
    assert_eq!(
        app.database().unit_by_uid(&ua).unwrap().pilot_ids,
        vec!["esper".to_string()],
        "適格なら入れ替え成立"
    );
    assert_eq!(
        app.database().unit_by_uid(&ub).unwrap().pilot_ids,
        vec!["plain".to_string()]
    );
}

#[test]
fn custom_unit_command_appears_in_menu_and_invokes() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "TestUnit", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // シナリオ定義の `*ユニットコマンド` を登録。
    // `c:` ラベルは条件サブルーチン — Return 1 で常に表示。
    let cc = crate::data::event::parse(
        "*ユニットコマンド テスト 味方 Call(c):\nSet 実行された 1\nExit\n\
             c:\nReturn 1\n",
    )
    .unwrap();
    app.script_library_mut().append(&cc);

    // ユニットをクリック → メニューに「テスト」が並ぶ。
    let ox = 2 * 32 + 16;
    let oy = 12 + 6 * 32 + 16;
    app.handle_input(Input::ClickAt { x: ox, y: oy });
    let idx = match app.command_menu() {
        Some(crate::CommandMenu::Unit { items, .. }) => {
            let pos = items.iter().position(|i| i.label() == "テスト");
            pos.expect("custom command「テスト」がメニューに無い")
        }
        _ => panic!("ユニットメニューが開いていない"),
    };
    // 「テスト」項目をクリック → 本体が実行される。
    let menu_x = crate::scene::command_menu::MENU_X + 40;
    let item_y = crate::scene::command_menu::MENU_Y
        + crate::scene::command_menu::MENU_PADDING
        + crate::scene::command_menu::MENU_ITEM_HEIGHT * idx as i32
        + 4;
    app.handle_input(Input::ClickAt {
        x: menu_x,
        y: item_y,
    });
    assert_eq!(
        app.script_var("実行された"),
        "1",
        "メニューからカスタムコマンドが実行されていない"
    );
}

#[test]
fn custom_unit_command_clears_stale_overlay_on_entry() {
    // スパロボ戦記 AlphaSecond: 別ユニットでステータス画面を再表示したとき、
    // 前ユニットの描画 (script_overlay) が残り透けて見える問題の防止。
    // カスタムユニットコマンド実行時に overlay をクリアして開始するため、
    // 連続実行しても描画コマンドが累積しないことを検証する。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "U1", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    let cc = crate::data::event::parse(
        "*ユニットコマンド 能力 味方 Call(c):\nPaintString 10 10 \"X\"\nExit\nc:\nReturn 1\n",
    )
    .unwrap();
    app.script_library_mut().append(&cc);

    app.invoke_custom_unit_command("U1", "能力");
    let after_first = app.script_overlay().cmds.len();
    assert!(after_first >= 1, "コマンド本体が描画していない");

    app.invoke_custom_unit_command("U1", "能力");
    let after_second = app.script_overlay().cmds.len();
    assert_eq!(
        after_first, after_second,
        "再表示で overlay が累積している (前ユニットの描画が残る)"
    );
}
