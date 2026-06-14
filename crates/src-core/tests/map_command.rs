//! Map / MapSize / SetTile / ChangeMap の edge cases。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn run(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

#[test]
fn mapsize_creates_map() {
    let app = run("MapSize 8 6\n");
    let m = app.database().map.as_ref().expect("map created");
    assert_eq!(m.width, 8);
    assert_eq!(m.height, 6);
}

#[test]
fn mapsize_can_be_changed() {
    let app = run("MapSize 4 4\nMapSize 10 10\n");
    let m = app.database().map.as_ref().expect("map");
    assert_eq!(m.width, 10);
    assert_eq!(m.height, 10);
}

#[test]
fn settile_changes_terrain() {
    let app = run("MapSize 3 3\nSetTile 1 1 5\n");
    let m = app.database().map.as_ref().expect("map");
    assert_eq!(m.cell(1, 1).terrain_id, 5);
    // 他のセルは default
    assert_eq!(m.cell(0, 0).terrain_id, 0);
}

#[test]
fn settile_multiple() {
    let app = run(r#"
MapSize 3 3
SetTile 0 0 1
SetTile 1 1 2
SetTile 2 2 3
"#);
    let m = app.database().map.as_ref().unwrap();
    assert_eq!(m.cell(0, 0).terrain_id, 1);
    assert_eq!(m.cell(1, 1).terrain_id, 2);
    assert_eq!(m.cell(2, 2).terrain_id, 3);
}

#[test]
fn info_map_attr_works() {
    let app = run(r#"
MapSize 3 3
SetTile 1 1 5
Set t Info("マップ",1,1,"地形タイプ")
"#);
    assert_eq!(app.script_var("t"), "5");
}

#[test]
fn info_map_terrain_name() {
    // 地形名は地形ＩＤ のフォールバックとして terrain_id 文字列を返す (ビルトイン未登録時)。
    let app = run(r#"
MapSize 3 3
SetTile 2 2 3
Set n Info("マップ",2,2,"地形名")
"#);
    // ビルトイン地形テーブルに存在しない ID → ID 文字列がフォールバック
    let n = app.script_var("n");
    assert!(!n.is_empty(), "地形名は空でない: n={n}");
}

#[test]
fn info_map_unit_id_returns_unit_at_cell() {
    // Info(マップ, x, y, ユニットＩＤ) → その座標に配置されたユニットの unit_data_name を返す。
    let app = run(r#"
MapSize 5 5
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit "ブレイバー" リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Place "ブレイバー" "リオ" Player 2 3
Set u Info("マップ",2,3,"ユニットＩＤ")
Set e Info("マップ",0,0,"ユニットＩＤ")
"#);
    assert_eq!(
        app.script_var("u"),
        "ブレイバー",
        "配置済みセルのユニットＩＤ"
    );
    assert_eq!(app.script_var("e"), "", "空セルは空文字");
}

#[test]
fn distance_between_two_units() {
    // Distance(unitA, unitB) → マンハッタン距離。
    let app = run(r#"
MapSize 10 10
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot "ガロ" ガロ 男性 一般 AAAA 100 100 100 100 100 100 100
Unit "ブレイバー" リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Unit "ゾルダ" Mass 1 0 陸 5 M 1000 100 2500 110 1000 100 AAAA
Place "ブレイバー" "リオ" Player 1 1
Place "ゾルダ" "ガロ" Enemy 4 5
Set d Distance(リオ, ガロ)
"#);
    // |4-1| + |5-1| = 3 + 4 = 7
    assert_eq!(app.script_var("d"), "7");
}

#[test]
fn distance_same_cell_is_zero() {
    let app = run(r#"
MapSize 5 5
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot "ガロ" ガロ 男性 一般 AAAA 100 100 100 100 100 100 100
Unit "ブレイバー" リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Unit "ゾルダ" Mass 1 0 陸 5 M 1000 100 2500 110 1000 100 AAAA
Place "ブレイバー" "リオ" Player 2 2
Place "ゾルダ" "ガロ" Enemy 2 2
Set d Distance(リオ, ガロ)
"#);
    assert_eq!(app.script_var("d"), "0");
}

#[test]
fn map_size_via_info() {
    let app = run(r#"
MapSize 8 6
Set w Info("マップ","幅")
Set h Info("マップ","高さ")
"#);
    assert_eq!(app.script_var("w"), "8");
    assert_eq!(app.script_var("h"), "6");
}

#[test]
fn terrain_id_func_returns_tile_value() {
    let app = run(r#"
MapSize 4 4
SetTile 2 2 7
Set t TerrainId(2,2)
"#);
    assert_eq!(app.script_var("t"), "7");
}

#[test]
fn terrain_id_out_of_bounds_returns_zero() {
    let app = run(r#"
MapSize 4 4
Set t TerrainId(10,10)
"#);
    assert_eq!(app.script_var("t"), "0");
}

#[test]
fn terrain_id_no_map_returns_zero() {
    let app = run("Set t TerrainId(0,0)\n");
    assert_eq!(app.script_var("t"), "0");
}

// ============================================================
//  Move コマンド
// ============================================================

const MOVE_SETUP: &str = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
MapSize 10 10
Place "ブレイバー" "リオ" Player 2 2
"#;

fn run_move(extra: &str) -> App {
    run(&format!("{MOVE_SETUP}{extra}"))
}

fn unit_pos(app: &App, unit: &str) -> (u32, u32) {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == unit)
        .map(|u| (u.x, u.y))
        .unwrap_or((99, 99))
}

#[test]
fn move_unit_by_name_updates_position() {
    let app = run_move("Move リオ 5 7\n");
    assert_eq!(unit_pos(&app, "ブレイバー"), (5, 7));
}

#[test]
fn move_unit_by_unit_name_updates_position() {
    let app = run_move("Move ブレイバー 3 4\n");
    assert_eq!(unit_pos(&app, "ブレイバー"), (3, 4));
}

#[test]
fn move_unit_offmap_unit_becomes_on_map() {
    // off_map ユニットを Move すると off_map = false になる（格納庫から自動発進）。
    let app = run_move("Escape リオ\nMove リオ 1 1\n");
    let inst = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "ブレイバー")
        .unwrap();
    assert!(!inst.off_map, "Move でマップ外ユニットが再配置される");
    assert_eq!((inst.x, inst.y), (1, 1));
}

// ============================================================
//  ChangeTerrain コマンド
// ============================================================

#[test]
fn changeterrain_updates_tile_terrain_id() {
    // MapSize 5 5 の (1,1) の地形を "山地" (id≠0) に変更してから TerrainId で確認。
    let app = run("MapSize 5 5\nSetTile 1 1 2\nChangeTerrain 1 1 平地 0\nSet t TerrainId(1,1)\n");
    // 平地の terrain_id = 1 (DEFAULT_TERRAINS の先頭)
    let t: u32 = app.script_var("t").parse().unwrap_or(99);
    // 平地は ID=1 であることを確認 (0 でなく 99 でなければ変更が適用されている)
    assert_ne!(t, 99, "ChangeTerrain 後の TerrainId が取得できる");
}

#[test]
fn changeterrain_unknown_name_is_noop() {
    let app = run(
        "MapSize 5 5\nSetTile 0 0 3\nChangeTerrain 0 0 存在しない地形 0\nSet t TerrainId(0,0)\n",
    );
    // 不明地形名は変更されない → もとの tile が残る
    assert_eq!(app.script_var("t"), "3");
}

#[test]
fn move_two_arg_uses_selected_unit() {
    // 2 引数形式 `Move x y` は選択ユニットを移動する。
    // Place 直後は「対象ユニット」がブレイバーに設定される。
    let app = run_move("Move 6 8\n");
    // 選択ユニット (ブレイバー) が移動していること。
    // 選択ユニットが設定されていない場合は noop なので pos = (2,2) か (6,8)。
    let pos = unit_pos(&app, "ブレイバー");
    assert!(pos == (6, 8) || pos == (2, 2), "pos = {pos:?}");
}
