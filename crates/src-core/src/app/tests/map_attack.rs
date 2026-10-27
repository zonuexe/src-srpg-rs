//! プレイヤーのマップ攻撃 (ユニットコマンド「マップ攻撃」) と目標地点。

use super::*;

/// `unit_name` にマップ兵器を持たせる。
fn give_map_weapon(app: &mut App, unit_name: &str, class: &str, max_range: i32) {
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == unit_name)
        .unwrap()
        .weapons
        .push(crate::data::unit::WeaponData {
            name: "マップ砲".to_string(),
            power: 1500,
            min_range: 1,
            max_range,
            precision: 100,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: class.to_string(),
            extras: Vec::new(),
        });
}

fn menu_index(app: &App, item: &crate::command_menu::UnitMenuItem) -> Option<i32> {
    match app.command_menu() {
        Some(crate::CommandMenu::Unit { items, .. }) => {
            items.iter().position(|i| i == item).map(|i| i as i32)
        }
        _ => None,
    }
}

/// 味方 Hero (2,6) が `Ｍ投L1` (射程 4) を持ち、敵 3 体を置いた盤面。
/// A (5,6) と B (5,7) は (5,6) を狙った半径 1 の範囲内、C (8,6) は範囲外。
fn setup() -> (App, String, [String; 3]) {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    give_map_weapon(&mut app, "Hero", "Ｍ投L1", 4);
    register_enemy_data(&mut app, "Foe");
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == "Foe")
        .unwrap()
        .hp = 100_000;
    let foes = [
        spawn_enemy(&mut app, "Foe", 5, 6),
        spawn_enemy(&mut app, "Foe", 5, 7),
        spawn_enemy(&mut app, "Foe", 8, 6),
    ];
    let hero = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&hero)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));
    app.set_stage_state(crate::stage::StageState::Battle);
    (app, hero, foes)
}

fn damage(app: &App, uid: &str) -> i64 {
    app.database().unit_by_uid(uid).unwrap().damage
}

/// メニューの「マップ攻撃」→ 兵器を選ぶ → 目標地点を選ぶ (効果範囲を表示) → 同じ地点を
/// もう一度選ぶと発射し、効果範囲内の敵だけが被弾して行動終了する。目標地点は
/// `X(目標地点)` / `Y(目標地点)` で 1 始まりの座標として読める。
#[test]
fn player_map_attack_hits_units_in_area() {
    use crate::command_menu::{ActionMode, UnitMenuItem};
    let (mut app, hero, [a, b, c]) = setup();

    click_tile(&mut app, 2, 6);
    let mi = menu_index(&app, &UnitMenuItem::MapAttack).expect("マップ攻撃がメニューに無い");
    click_menu_item(&mut app, mi);
    assert!(
        app.pending_dialog().is_some(),
        "マップ兵器の一覧が出ていない"
    );
    app.respond_dialog(1);
    assert!(matches!(
        app.action_mode(),
        ActionMode::MapAttackSelect { point: None, .. }
    ));

    // 射程外 (距離 6) は選べない。
    click_tile(&mut app, 8, 6);
    assert!(matches!(
        app.action_mode(),
        ActionMode::MapAttackSelect { point: None, .. }
    ));

    // 1 回目: 目標地点を仮決めして効果範囲を表示する (まだ撃たない)。
    click_tile(&mut app, 5, 6);
    let crate::scene::map_view::MapAttackTiles { targets, area } =
        app.map_attack_select_tiles().unwrap();
    assert!(targets.contains(&(5, 6)) && !targets.contains(&(8, 6)));
    assert!(area.contains(&(5, 6)) && area.contains(&(5, 7)) && !area.contains(&(8, 6)));
    assert_eq!(damage(&app, &a), 0, "仮決めの段階で撃っている");

    // 2 回目: 発射。
    click_tile(&mut app, 5, 6);
    assert!(damage(&app, &a) > 0, "範囲内の A が被弾していない");
    assert!(damage(&app, &b) > 0, "範囲内の B が被弾していない");
    assert_eq!(damage(&app, &c), 0, "範囲外の C が被弾している");
    assert!(app.database().unit_by_uid(&hero).unwrap().has_acted);
    assert_eq!(app.action_mode(), ActionMode::Browse);

    assert_eq!(app.selected_point(), Some((5, 6)));
    let stmts =
        crate::data::event::parse("Set px X(目標地点)\nSet py Y(目標地点)\nSet wx WX(目標地点)\n")
            .unwrap();
    crate::event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("px"), "6");
    assert_eq!(app.script_var("py"), "7");
    assert_eq!(app.script_var("wx"), (5 * 32).to_string());
}

/// マップ兵器は単体攻撃には使わない: マップ兵器しか持たないユニットのメニューには
/// 「攻撃」が出ず、「マップ攻撃」だけが出る。
#[test]
fn map_weapon_is_not_used_for_single_target_attack() {
    use crate::command_menu::{UnitAction, UnitMenuItem};
    let (mut app, _, _) = setup();
    click_tile(&mut app, 2, 6);
    assert!(menu_index(&app, &UnitMenuItem::Builtin(UnitAction::Attack)).is_none());
    assert!(menu_index(&app, &UnitMenuItem::MapAttack).is_some());
}

/// 目標地点選択のキャンセル: 仮決めした地点があれば取り消し、無ければユニットメニューへ戻る。
#[test]
fn map_attack_select_cancel_steps_back() {
    use crate::command_menu::{ActionMode, UnitMenuItem};
    let (mut app, _, [a, _, _]) = setup();
    click_tile(&mut app, 2, 6);
    let mi = menu_index(&app, &UnitMenuItem::MapAttack).unwrap();
    click_menu_item(&mut app, mi);
    app.respond_dialog(1);
    click_tile(&mut app, 5, 6);
    app.handle_input(Input::Cancel);
    assert!(matches!(
        app.action_mode(),
        ActionMode::MapAttackSelect { point: None, .. }
    ));
    app.handle_input(Input::Cancel);
    assert_eq!(app.action_mode(), ActionMode::Browse);
    assert!(
        menu_index(&app, &UnitMenuItem::MapAttack).is_some(),
        "メニューに戻っていない"
    );
    assert_eq!(damage(&app, &a), 0);
}

/// 方向型 (`Ｍ直`) は攻撃側から十字方向のマスだけを目標地点に選べる。
#[test]
fn directional_map_weapon_targets_cross_only() {
    use crate::command_menu::UnitMenuItem;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 4, 6);
    give_map_weapon(&mut app, "Hero", "Ｍ直", 3);
    app.set_stage_state(crate::stage::StageState::Battle);
    click_tile(&mut app, 4, 6);
    let mi = menu_index(&app, &UnitMenuItem::MapAttack).unwrap();
    click_menu_item(&mut app, mi);
    app.respond_dialog(1);
    let targets = app.map_attack_select_tiles().unwrap().targets;
    assert!(targets.contains(&(7, 6)) && targets.contains(&(4, 3)));
    assert!(!targets.contains(&(5, 7)), "斜めは選べない");
    assert!(!targets.contains(&(8, 6)), "射程外は選べない");
}

/// 使えないマップ兵器 (気力不足) も「マップ攻撃」は表示し、一覧で `×` を付けて選べなくする。
#[test]
fn unusable_map_weapon_is_listed_but_not_selectable() {
    use crate::command_menu::{ActionMode, UnitMenuItem};
    let (mut app, _, _) = setup();
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == "Hero")
        .unwrap()
        .weapons
        .last_mut()
        .unwrap()
        .necessary_morale = 150;
    click_tile(&mut app, 2, 6);
    let mi = menu_index(&app, &UnitMenuItem::MapAttack).expect("マップ攻撃がメニューに無い");
    click_menu_item(&mut app, mi);
    match app.pending_dialog() {
        Some(crate::PendingDialog::Menu { options, .. }) => {
            assert_eq!(options, &vec!["×マップ砲".to_string()]);
        }
        other => panic!("マップ兵器の一覧が出ていない: {other:?}"),
    }
    app.respond_dialog(1);
    assert!(!matches!(
        app.action_mode(),
        ActionMode::MapAttackSelect { .. }
    ));
    assert!(
        menu_index(&app, &UnitMenuItem::MapAttack).is_some(),
        "メニューに戻っていない"
    );
}

/// 移動型マップ攻撃 (`Ｍ移`、公式サンプルの鳳翼天翔): 目標地点は武器の最大射程を移動力と
/// した移動範囲の空きマスで、発射すると攻撃側がそこへ移動し、通り道の敵を攻撃する
/// (原典 `Unit.MapAttack` の `Jump(tx, ty)`)。
#[test]
fn move_type_map_attack_moves_attacker_through_enemies() {
    use crate::command_menu::UnitMenuItem;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    // 地形コストで移動範囲が削れないよう一様なマップにする。
    app.database_mut()
        .replace_map(crate::data::map::MapData::new(12, 12));
    place_player_unit(&mut app, "Hero", 2, 6);
    give_map_weapon(&mut app, "Hero", "Ｍ移", 3);
    register_enemy_data(&mut app, "Foe");
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == "Foe")
        .unwrap()
        .hp = 100_000;
    let a = spawn_enemy(&mut app, "Foe", 3, 6);
    let b = spawn_enemy(&mut app, "Foe", 4, 6);
    let hero = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&hero)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));
    app.set_stage_state(crate::stage::StageState::Battle);

    click_tile(&mut app, 2, 6);
    let mi = menu_index(&app, &UnitMenuItem::MapAttack).unwrap();
    click_menu_item(&mut app, mi);
    app.respond_dialog(1);
    let targets = app.map_attack_select_tiles().unwrap().targets;
    assert!(targets.contains(&(5, 6)), "移動範囲の空きマスが選べない");
    assert!(!targets.contains(&(3, 6)), "敵の居るマスは選べない");
    assert!(!targets.contains(&(2, 6)), "自分のマスは選べない");

    click_tile(&mut app, 5, 6);
    click_tile(&mut app, 5, 6);
    let h = app.database().unit_by_uid(&hero).unwrap();
    assert_eq!((h.x, h.y), (5, 6), "攻撃側が目標地点へ移動していない");
    assert_eq!(app.database().uid_at(5, 6), Some(hero.as_str()));
    assert_eq!(app.database().uid_at(2, 6), None);
    assert!(
        damage(&app, &a) > 0 && damage(&app, &b) > 0,
        "通り道の敵が被弾していない"
    );
}
