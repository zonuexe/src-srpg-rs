//! `.eve` のマップ座標は原典 SRC どおり 1 始まり (左上隅が (1,1))。
//! 内部の盤面は 0 始まりなので、スクリプト境界で ±1 されることを確かめる。
//! (`MapSize` / `SetTile` / `Place` は本移植独自のデータ定義で内部座標のまま。)

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn run(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

fn pos_of(app: &App, pilot: &str) -> (u32, u32) {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.pilot_name == pilot)
        .map(|u| (u.x, u.y))
        .expect("unit")
}

#[test]
fn create_places_unit_at_one_based_tile() {
    // 公式サンプルの `Create 味方 <unit> 0 <pilot> 20 3 3` は SRC タイル (3,3)
    // = 内部 (2,2) に置かれる。
    let app = run("MapSize 10 10\nCreate 味方 試作機 0 テスト 20 3 3\n");
    assert_eq!(pos_of(&app, "テスト"), (2, 2));
}

#[test]
fn create_at_map_size_lands_on_last_tile() {
    // 座標 = 幅・高さ は盤内の右下隅 (SRC は 1..=MapWidth)。
    let app = run("MapSize 15 12\nCreate 味方 試作機 0 テスト 1 15 12\n");
    assert_eq!(pos_of(&app, "テスト"), (14, 11));
}

#[test]
fn create_clamps_out_of_range_coordinates_into_map() {
    // 原典 `ExecCreateCmd`: 1 未満は 1、MapWidth 超は MapWidth に丸める。
    // 15x15 マップへの (19,19) 配置 (TukabarkSampleScenario01) は右下隅、
    // 0 は左上隅。
    let app = run("MapSize 15 15\n\
         Create 味方 試作機 0 遠方 1 19 19\n\
         Create 味方 試作機 0 原点 1 0 0\n");
    assert_eq!(pos_of(&app, "遠方"), (14, 14));
    assert_eq!(pos_of(&app, "原点"), (0, 0));
}

#[test]
fn move_and_launch_use_one_based_coordinates() {
    let app = run("MapSize 8 8\n\
         Create 味方 試作機 0 移動 1 1 1\n\
         Create 味方 試作機 0 出撃 1 2 1\n\
         Move 移動 4 5\n\
         Escape 出撃\n\
         Launch 出撃 8 8\n");
    assert_eq!(pos_of(&app, "移動"), (3, 4));
    assert_eq!(pos_of(&app, "出撃"), (7, 7));
}

#[test]
fn x_y_functions_return_one_based_coordinates() {
    let app = run("MapSize 10 10\n\
         Create 味方 試作機 0 テスト 1 3 7\n\
         Set gx X(テスト)\n\
         Set gy Y(テスト)\n\
         Set wx WX(テスト)\n\
         Set wx1 WX(1)\n");
    assert_eq!(app.script_var("gx"), "3");
    assert_eq!(app.script_var("gy"), "7");
    // WX は内部タイル左上 (1 タイル 32px)。SRC 3 列目 = 内部 2 → 64。
    assert_eq!(app.script_var("wx"), "64");
    assert_eq!(app.script_var("wx1"), "0");
}

#[test]
fn create_then_x_round_trips() {
    // `Create … (X(a) + 1) Y(a)` で隣に置くイディオムが原典どおり動く。
    let app = run("MapSize 10 10\n\
         Create 味方 試作機 0 甲 1 4 4\n\
         Create 味方 試作機 0 乙 1 (X(甲) + 1) Y(甲)\n");
    assert_eq!(pos_of(&app, "甲"), (3, 3));
    assert_eq!(pos_of(&app, "乙"), (4, 3));
}

#[test]
fn info_map_reads_one_based_cell() {
    let app = run("MapSize 4 3\n\
         SetTile 0 0 7\n\
         SetTile 3 2 9\n\
         Set first Info(マップ, 1, 1, 地形タイプ)\n\
         Set last Info(マップ, 4, 3, 地形タイプ)\n\
         Set zero Info(マップ, 0, 0, 地形タイプ)\n\
         Set over Info(マップ, 5, 3, 地形タイプ)\n");
    assert_eq!(app.script_var("first"), "7");
    assert_eq!(app.script_var("last"), "9");
    // 盤外 (0 や幅超) は空 (原典 `EvalInfoFunc` の範囲チェック)。
    assert_eq!(app.script_var("zero"), "");
    assert_eq!(app.script_var("over"), "");
}

#[test]
fn change_terrain_and_center_use_one_based_coordinates() {
    let app = run("MapSize 6 6\nChangeTerrain 6 6 海 0\nCenter 6 6\n");
    let map = app.database().map.as_ref().unwrap();
    // 海 = 4 (組込テーブル DEFAULT_TERRAINS)
    assert_eq!(map.cell(5, 5).terrain_id, 4);
    assert_eq!(app.map_cursor(), Some((5, 5)));
}

#[test]
fn entry_label_fires_with_one_based_coordinates() {
    // `進入 <unit> <x> <y>:` の座標は左上隅が (1,1) (`進入イベント.md`)。
    let src = "\
MapSize 5 5
Create 味方 試作機 0 テスト 1 3 3
Exit

進入 テスト 2 2:
Set wrong 1
Return

進入 テスト 3 3:
Set entered 1
Return
";
    let mut app = App::new();
    let stmts = event::parse(src).unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    let idx = app
        .database()
        .unit_instances
        .iter()
        .position(|u| u.pilot_name == "テスト")
        .unwrap();
    event_runtime::fire_entry_event_labels(&mut app, idx);
    assert_eq!(app.script_var("entered"), "1");
    assert_eq!(app.script_var("wrong"), "");
}
