//! App のテスト。共通のヘルパはここ、テスト本体はトピック別のファイルに置く。

use super::*;
use crate::scene::configuration::{ConfigurationLayout, TITLE_BAR_HEIGHT};

fn click(app: &mut App, scene_x: i32, scene_y: i32) -> bool {
    let (sw, sh) = app.scene().size();
    let ox = (CANVAS_WIDTH as i32 - sw as i32) / 2;
    let oy = (CANVAS_HEIGHT as i32 - sh as i32) / 2;
    app.handle_input(Input::ClickAt {
        x: ox + scene_x,
        y: oy + scene_y,
    })
}

fn enter_mapview_with_demo_map(app: &mut App) {
    app.handle_input(Input::Advance); // Configuration
    app.handle_input(Input::Advance); // MapView
    app.database_mut().replace_map(crate::data::map::demo());
}

fn place_player_unit(app: &mut App, name: &str, x: u32, y: u32) {
    use crate::data::pilot::{Adaption, PilotData, Sex};
    use crate::data::unit::{Size, UnitData};
    use crate::{Party, UnitInstance};

    let pilot = PilotData {
        spirit_commands: Vec::new(),
        name: "PILOT".into(),
        nickname: "P".into(),
        kana_name: "P".into(),
        sex: Sex::Unspecified,
        class: String::new(),
        adaption: Adaption::parse("AAAA").unwrap(),
        exp_value: 0,
        // SRC のデフォルト値に近い数値。0 だと乗算式でダメージが 0 になる。
        infight: 100,
        shooting: 100,
        hit: 10,
        dodge: 10,
        intuition: 10,
        technique: 10,
        personality: None,
        sp: None,
        bgm: None,
        bitmap: None,
        features: Vec::new(),
    };
    let unit_data = UnitData {
        abilities: Vec::new(),
        name: name.into(),
        kana_name: String::new(),
        nickname: name.into(),
        class: String::new(),
        pilot_num: 1,
        item_num: 0,
        transportation: "陸".into(),
        speed: 3,
        size: Size::M,
        value: 0,
        exp_value: 0,
        hp: 100,
        en: 50,
        armor: 10,
        mobility: 10,
        adaption: Adaption::parse("AAAA").unwrap(),
        bitmap: String::new(),
        weapons: Vec::new(),
        features: Vec::new(),
    };
    app.database_mut().catalog.pilots.push(pilot);
    app.database_mut().catalog.units.push(unit_data);
    app.database_mut()
        .register_unit(UnitInstance::new(name, "PILOT", Party::Player, x, y));
}

/// 指定名・指定 features のパイロットを DB に追加する (技能テスト用)。
/// 同名が既にあれば何もしない。ユニットデータは別途 (`place_player_unit` 等) で用意する。
fn push_pilot_with_features(app: &mut App, name: &str, features: Vec<(String, String)>) {
    use crate::data::pilot::{Adaption, PilotData, Sex};
    if app.database().pilot_by_name(name).is_some() {
        return;
    }
    app.database_mut().catalog.pilots.push(PilotData {
        spirit_commands: Vec::new(),
        name: name.into(),
        nickname: name.into(),
        kana_name: name.into(),
        sex: Sex::Unspecified,
        class: String::new(),
        adaption: Adaption::parse("AAAA").unwrap(),
        exp_value: 0,
        infight: 100,
        shooting: 100,
        hit: 10,
        dodge: 10,
        intuition: 10,
        technique: 10,
        personality: None,
        sp: None,
        bgm: None,
        bitmap: None,
        features,
    });
}

/// パイロットに精神コマンド (SP コマンド) を与えてユニットに搭載するヘルパ。
/// `place_player_unit` 済みのユニット uid を返す。
fn give_spirit_commands(
    app: &mut App,
    cmds: Vec<crate::data::pilot::SpiritCommand>,
    max_sp: i32,
) -> String {
    let p = app
        .database_mut()
        .catalog
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
        .unwrap();
    p.sp = Some(max_sp);
    p.spirit_commands = cmds;
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Player)
        .unwrap()
        .uid
        .clone()
}

/// place_player_unit 済みの先頭の味方ユニット uid を返すヘルパ。
fn first_player_uid(app: &App) -> String {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Player)
        .unwrap()
        .uid
        .clone()
}

/// `霊力`/`霊力成長` 技能を持つパイロットデータを DB へ追加し、それを主パイロットに
/// 持つプレイヤーユニットを配置する。`push_pilot_with_features` でパイロット静的
/// データ (`霊力Lv<n>` 等) を登録した上で `create_pilot_instance` して
/// `PilotInstance.level` を設定し、`main_pilot_name()` (= `pilot_name` フォールバック)
/// と `PilotInstance.id` を一致させて `effective_max_plana` の実解決経路に乗せる。
/// 返り値は配置したユニットの uid。
fn place_player_unit_with_plana_pilot(
    app: &mut App,
    unit_name: &str,
    pilot_name: &str,
    pilot_features: Vec<(String, String)>,
    pilot_level: i32,
    x: u32,
    y: u32,
) -> String {
    place_player_unit(app, unit_name, x, y); // UnitData 用途 (features は空で登録済み)
    push_pilot_with_features(app, pilot_name, pilot_features);
    let uid = app.database_mut().register_unit(crate::UnitInstance::new(
        unit_name,
        pilot_name,
        crate::Party::Player,
        x + 1,
        y,
    ));
    app.database_mut()
        .create_pilot_instance(pilot_name, pilot_name);
    app.database_mut()
        .pilot_instance_by_id_mut(pilot_name)
        .unwrap()
        .level = pilot_level;
    uid
}

/// アビリティをユニットに与えるテストヘルパ (静的 UnitData + ランタイム両方)。
fn give_ability(app: &mut App, unit_name: &str, uid: &str, ab: crate::data::unit::AbilityData) {
    let d = app
        .database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|d| d.name == unit_name)
        .unwrap();
    d.abilities.push(ab);
    let abilities = app
        .database()
        .unit_by_name(unit_name)
        .unwrap()
        .abilities
        .clone();
    let u = app.database_mut().unit_by_uid_mut(uid).unwrap();
    u.abilities = abilities
        .iter()
        .map(|a| {
            let mut ua = crate::unit_ability::UnitAbility::new(a.name.clone(), a.effect.clone());
            ua.stock_remaining = a.uses;
            ua
        })
        .collect();
}

fn mk_ability(
    name: &str,
    effect: &str,
    range: i32,
    uses: Option<i32>,
) -> crate::data::unit::AbilityData {
    crate::data::unit::AbilityData {
        name: name.into(),
        effect: effect.into(),
        range,
        uses,
        en_cost: None,
        morale: None,
        attributes: String::new(),
    }
}

/// 合体テスト用: host + 2マス以内の合体相手を置き、合体形態 UnitData を用意する。
fn setup_combine(app: &mut App) -> (String, String) {
    enter_mapview_with_demo_map(app);
    place_player_unit(app, "GetterEagle", 3, 3);
    place_player_unit(app, "GetterJaguar", 4, 3);
    // 合体形態 GetterRobo の UnitData (GetterEagle を複製)。
    let mut g = app.database().unit_by_name("GetterEagle").cloned().unwrap();
    g.name = "GetterRobo".into();
    app.database_mut().catalog.units.push(g);
    app.set_stage_state(crate::stage::StageState::Battle);
    let host = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "GetterEagle")
        .unwrap()
        .uid
        .clone();
    let partner = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "GetterJaguar")
        .unwrap()
        .uid
        .clone();
    // host に 合体 特殊能力 (名称, 合体形態, 相手)。
    app.database_mut()
        .unit_by_uid_mut(&host)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new(
        "合体",
        "合体 GetterRobo GetterJaguar",
    )];
    (host, partner)
}

/// host を 3 体合体能力に拡張し、3 体目の合体相手 (GetterBear) を `pos` に置く。
/// 拡張後の 3 体目 uid を返す。
fn extend_combine_to_triple(app: &mut App, host: &str, pos: (u32, u32)) -> String {
    place_player_unit(app, "GetterBear", pos.0, pos.1);
    let partner2 = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "GetterBear")
        .unwrap()
        .uid
        .clone();
    app.database_mut()
        .unit_by_uid_mut(host)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new(
        "合体",
        "合体 GetterRobo GetterJaguar GetterBear",
    )];
    partner2
}

/// テスト用: 区別できる 2 機の味方ユニットを置きインターミッションに入る。
fn setup_ride_change(app: &mut App) -> (String, String) {
    enter_mapview_with_demo_map(app);
    place_player_unit(app, "UnitA", 2, 6);
    place_player_unit(app, "UnitB", 3, 6);
    let uid_a = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "UnitA")
        .unwrap()
        .uid
        .clone();
    let uid_b = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "UnitB")
        .unwrap()
        .uid
        .clone();
    {
        let a = app.database_mut().unit_by_uid_mut(&uid_a).unwrap();
        a.pilot_name = "アリス".into();
        a.pilot_ids = vec!["alice".into()];
    }
    {
        let b = app.database_mut().unit_by_uid_mut(&uid_b).unwrap();
        b.pilot_name = "魔理沙".into();
        b.pilot_ids = vec!["marisa".into()];
    }
    app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
    // 乗り換えは Option コマンドで有効化されたときのみ表示される (原典準拠)。
    app.set_script_var("Option(乗り換え)".to_string(), "1".to_string());
    app.scene = Scene::Intermission;
    app.intermission_mode = IntermissionMode::Menu;
    (uid_a, uid_b)
}

/// 支配系属性 (`憑`/`魅`) 武器を作る (critical 100 → 必ず proc)。
fn possession_weapon(attr: &str) -> crate::data::unit::WeaponData {
    crate::data::unit::WeaponData {
        name: format!("{attr}武器"),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: attr.into(),
        extras: Vec::new(),
    }
}

fn add_weapon(app: &mut App, unit_name: &str, power: i64, max_range: i32) {
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == unit_name)
        .unwrap()
        .weapons
        .push(crate::data::unit::WeaponData {
            name: "テスト砲".to_string(),
            power,
            min_range: 1,
            max_range,
            precision: 100,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        });
}

fn click_tile(app: &mut App, c: u32, r: u32) {
    app.handle_input(Input::ClickAt {
        x: (c as i32) * 32 + 16,
        y: 12 + (r as i32) * 32 + 16,
    });
}

fn click_menu_item(app: &mut App, idx: i32) {
    let menu_x = crate::scene::command_menu::MENU_X + 40;
    let item_y = crate::scene::command_menu::MENU_Y
        + crate::scene::command_menu::MENU_PADDING
        + crate::scene::command_menu::MENU_ITEM_HEIGHT * idx
        + 4;
    app.handle_input(Input::ClickAt {
        x: menu_x,
        y: item_y,
    });
}

fn unit_xy(app: &App, name: &str) -> Option<(u32, u32)> {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == name)
        .map(|u| (u.x, u.y))
}

/// 表示中のユニットメニューが指す uid を現在位置に解決する (uid キー化後の assert 用)。
fn menu_unit_pos(app: &App) -> Option<(u32, u32)> {
    match app.command_menu() {
        Some(crate::CommandMenu::Unit { uid, .. }) => {
            app.database().unit_by_uid(uid).map(|u| (u.x, u.y))
        }
        _ => None,
    }
}

/// MoveSelect 中のユニット uid を現在位置に解決する。
fn move_select_pos(app: &App) -> Option<(u32, u32)> {
    match app.action_mode() {
        crate::ActionMode::MoveSelect { uid } => {
            app.database().unit_by_uid(&uid).map(|u| (u.x, u.y))
        }
        _ => None,
    }
}

/// 既存 UnitData を流用して敵ユニットを 1 体登録し uid を返す。
fn spawn_enemy(app: &mut App, unit_data_name: &str, x: u32, y: u32) -> String {
    spawn_party(app, unit_data_name, crate::Party::Enemy, x, y)
}

/// 既存 UnitData を流用して指定陣営のユニットを登録し uid を返す。
fn spawn_party(app: &mut App, unit_data_name: &str, party: crate::Party, x: u32, y: u32) -> String {
    app.database_mut().register_unit(crate::UnitInstance::new(
        unit_data_name,
        "PILOT",
        party,
        x,
        y,
    ))
}

/// 表示中ユニットメニューの「攻撃」項目の index。
fn attack_item_index(app: &App) -> i32 {
    match app.command_menu() {
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
            .expect("メニューに Attack が無い")
            as i32,
        _ => panic!("ユニットメニューが開いていない: {:?}", app.command_menu()),
    }
}

/// テスト用に `損傷率 <unit> <pct>:` ラベルを登録する (本体は `Set <var> 1`)。
/// `current_stage_file` が空のままなので発火は global 経路 (`post_event_label`)。
fn register_damage_threshold_label(app: &mut App, unit_name: &str, pct: i64, var: &str) {
    let src = format!("損傷率 {unit_name} {pct}:\nSet {var} 1\nExit\n");
    let stmts = crate::data::event::parse(&src).expect("parse 損傷率 label");
    app.script_library_mut()
        .append_with_name(&stmts, "test_dmg.eve");
}

/// `place_player_unit` でユニットデータ (UnitData/PilotData) を登録しつつ、配置された
/// player プレースホルダ実体は除去する。これにより同名の敵を `spawn_enemy` したとき、
/// 名前によるユニット解決 (`SpecialPower` 等) が敵側を指す。
fn register_enemy_data(app: &mut App, name: &str) {
    place_player_unit(app, name, 9, 9);
    let idx = app
        .database()
        .unit_instances
        .iter()
        .position(|u| u.unit_data_name == name)
        .unwrap();
    app.database_mut().remove_unit_at(idx);
}

/// 敵が隣の Hero を攻撃する逐次フェイズを走らせ、反撃選択 `reaction`
/// (Some=手動応答 / None=自動反撃モード) のときの Hero 被ダメージを返す。
/// `hero_weapon` を与えると Hero は反撃可能になる。
fn run_enemy_attacks_hero(reaction: Option<u32>, hero_weapon: bool) -> (App, i64) {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6); // 防御側
    if hero_weapon {
        add_weapon(&mut app, "Hero", 50, 1);
    }
    place_player_unit(&mut app, "Foe", 9, 9); // Foe の UnitData を用意 (隅の味方)
    add_weapon(&mut app, "Foe", 60, 1);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 4, 6); // 攻撃側 (隣接)
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_animate_ai(true);
    if reaction.is_none() {
        app.toggle_auto_counter(); // 自動反撃モード ON
    }
    app.handle_input(Input::EndPhase);
    for _ in 0..300 {
        app.tick(0.1);
        if let Some(c) = reaction {
            if app.pending_dialog().is_some() {
                app.respond_dialog(c);
            }
        }
        if !app.ai_running() {
            break;
        }
    }
    let dmg = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Hero")
        .map(|u| u.damage)
        .unwrap();
    (app, dmg)
}

/// テスト用: `unit_data_name` のユニットに `critical` 率の武器「クリ砲」(射程1) と
/// 必中を付与する。
fn give_crit_weapon(app: &mut App, unit_name: &str, power: i64, critical: i32) {
    let weapon = crate::data::unit::WeaponData {
        name: "クリ砲".into(),
        power,
        min_range: 1,
        max_range: 1,
        precision: 100,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: String::new(),
        critical,
        class: String::new(),
        extras: Vec::new(),
    };
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == unit_name)
        .unwrap()
        .weapons
        .push(weapon);
    app.database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| u.unit_data_name == unit_name)
        .unwrap()
        .add_condition(crate::condition::Condition::new("必中", 9));
}

mod ability;
mod ai;
mod combat;
mod input;
mod intermission;
mod misc;
mod spirit;
mod stage_flow;
mod status_effects;
mod unit_commands;
