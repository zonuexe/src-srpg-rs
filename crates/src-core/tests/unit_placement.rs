//! ユニットを置くコマンド (`Create` / `Move` / `Launch`) は、指定マスが埋まっていれば
//! 原典 `Unit.StandBy` どおり近くの空きマスに置く (同じマスに 2 機を重ねない)。

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

fn uid_at(app: &App, pos: (u32, u32)) -> Option<String> {
    app.database().uid_at(pos.0, pos.1).map(str::to_string)
}

#[test]
fn create_on_occupied_tile_uses_nearest_free_tile() {
    // 公式サンプルは キャリバーン と 龍神機 を同じ (3,3) に Create する。原典の探索順
    // (距離 1 で x の小さい順 → y の小さい順) では 2 機目は左隣 (2,3) = 内部 (1,2)。
    let app = run("MapSize 10 10\n\
         Create 味方 試作機 0 一号 1 3 3\n\
         Create 味方 試作機 0 二号 1 3 3\n\
         Create 味方 試作機 0 三号 1 3 3\n");
    assert_eq!(pos_of(&app, "一号"), (2, 2));
    assert_eq!(pos_of(&app, "二号"), (1, 2));
    assert_eq!(pos_of(&app, "三号"), (2, 1));
    // 位置索引もそれぞれを指す (重なって片方が選べなくなることが無い)。
    for p in ["一号", "二号", "三号"] {
        let uid = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.pilot_name == p)
            .map(|u| u.uid.clone());
        assert_eq!(uid_at(&app, pos_of(&app, p)), uid, "{p}");
    }
}

#[test]
fn create_with_formation_option_skips_adjacent_tiles() {
    // `部隊配置` は奇数距離を飛ばし、ユニット同士を隣接させない。
    let app = run("MapSize 10 10\n\
         Create 味方 試作機 0 一号 1 5 5\n\
         Create 味方 試作機 0 二号 1 5 5 \"\" 部隊配置\n");
    let (a, b) = (pos_of(&app, "一号"), pos_of(&app, "二号"));
    let d = a.0.abs_diff(b.0) + a.1.abs_diff(b.1);
    assert_eq!(d, 2, "部隊配置なのに距離 {d}");
}

#[test]
fn move_and_launch_onto_occupied_tile_shift_aside() {
    let app = run("MapSize 10 10\n\
         Create 味方 試作機 0 先客 1 4 4\n\
         Create 味方 試作機 0 移動 1 1 1\n\
         Create 味方 試作機 0 出撃 1 8 8\n\
         Move 移動 4 4\n\
         Escape 出撃\n\
         Launch 出撃 4 4\n");
    assert_eq!(pos_of(&app, "先客"), (3, 3));
    assert_eq!(pos_of(&app, "移動"), (2, 3));
    assert_eq!(pos_of(&app, "出撃"), (3, 2));
}

#[test]
fn move_onto_own_tile_stays() {
    // 自分の居るマスは空きとみなす。
    let app = run("MapSize 10 10\n\
         Create 味方 試作機 0 一号 1 4 4\n\
         Move 一号 4 4\n");
    assert_eq!(pos_of(&app, "一号"), (3, 3));
}
