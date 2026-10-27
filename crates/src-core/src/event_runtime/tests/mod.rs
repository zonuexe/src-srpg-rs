//! `.eve` ランタイムのテスト。共通のヘルパはここ、テスト本体はトピック別のファイルに置く。

use super::*;
use crate::combat::map_attack_area;
use crate::data::event;

/// テストヘルパ: src を実行して App を返す。
fn run_script(src: &str) -> App {
    let stmts = event::parse(src).expect("parse");
    let mut app = App::new();
    execute(&mut app, &stmts).expect("execute");
    app
}

fn setup_two_units(app: &mut App) {
    let src = "\
MapSize 6 5
Pilot \"リオ\" リオ 男性 c AAAA 100 100 100 100 100 100 100
Unit \"ブレイバー\" リアル系 1 0 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Place \"ブレイバー\" \"リオ\" Player 1 1
Pilot \"ガロ\" ガロ 男性 c AAAA 100 100 100 100 100 100 100
Unit \"ゾルダII\" リアル系 1 0 陸 5 M 800 200 2400 80 900 80 AAAA
Place \"ゾルダII\" \"ガロ\" Enemy 4 3
";
    let stmts = event::parse(src).unwrap();
    execute(app, &stmts).unwrap();
}

fn setup_units_with_item(app: &mut App) {
    setup_two_units(app);
    // Item を定義に追加 (Item 命令から参照される ItemData)
    app.database_mut()
        .catalog
        .items
        .push(crate::data::item::ItemData {
            name: "ハイブリッドアーマー".to_string(),
            class: "armor".to_string(),
            part: "本体".to_string(),
            hp_mod: 500,
            en_mod: 50,
            armor_mod: 100,
            mobility_mod: 10,
            speed_mod: 1,
            comment: String::new(),
            features: Vec::new(),
        });
}

/// テスト用に `database().catalog.items` (アイテム DB) へ名称のみのアイテムを登録する。
/// `ItemData` は Default を持たないため全フィールドを明示する。
fn push_item_named(app: &mut App, name: &str) {
    app.database_mut()
        .catalog
        .items
        .push(crate::data::item::ItemData {
            name: name.to_string(),
            class: String::new(),
            part: String::new(),
            hp_mod: 0,
            en_mod: 0,
            armor_mod: 0,
            mobility_mod: 0,
            speed_mod: 0,
            comment: String::new(),
            features: Vec::new(),
        });
}

const ZOLDA_SETUP: &str = "\
MapSize 4 4
Pilot \"P\" P 男性 c AAAA 10 100 100 100 100 100 100
Unit \"ゾルダII\" リアル系 1 0 陸 5 M 100 50 2400 80 900 80 AAAA
Place \"ゾルダII\" \"P\" Enemy 1 1
";

mod control_flow;
mod dialog;
mod events;
mod expressions;
mod files;
mod graphics;
mod units;
