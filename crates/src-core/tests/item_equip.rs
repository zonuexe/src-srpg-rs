//! Item / Equip / RemoveItem / ExchangeItem の edge cases。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

const SETUP: &str = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon "ブレイバー" "ライフル" 2500 2 5 15 -1
Place "ブレイバー" "リオ" Player 0 0
"#;

fn run_setup(extra: &str) -> App {
    let mut app = App::new();
    let src = format!("{SETUP}{extra}");
    let stmts = event::parse(&src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

fn equipped_items(app: &App, unit_name: &str) -> Vec<String> {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == unit_name)
        .map(|u| {
            u.equipped_item_names()
                .into_iter()
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn item_equips_to_unit() {
    let app = run_setup(r#"Item "リオ" "ハイパーバズーカ""#);
    let items = equipped_items(&app, "ブレイバー");
    assert!(items.iter().any(|i| i == "ハイパーバズーカ"), "{items:?}");
}

#[test]
fn equip_is_alias_of_item() {
    let app = run_setup(r#"Equip "リオ" "ハイパーバズーカ""#);
    let items = equipped_items(&app, "ブレイバー");
    assert!(items.iter().any(|i| i == "ハイパーバズーカ"));
}

#[test]
fn multiple_items_equipped_in_order() {
    let app = run_setup(
        r#"
Item "リオ" "ハイパーバズーカ"
Item "リオ" "ビームサーベル"
Item "リオ" "シールド"
"#,
    );
    let items = equipped_items(&app, "ブレイバー");
    assert_eq!(items.len(), 3);
}

#[test]
fn remove_item_drops_specific() {
    let app = run_setup(
        r#"
Item "リオ" "A"
Item "リオ" "B"
Item "リオ" "C"
RemoveItem "リオ" "B"
"#,
    );
    let items = equipped_items(&app, "ブレイバー");
    assert_eq!(items, vec!["A".to_string(), "C".to_string()]);
}

#[test]
fn remove_item_unknown_is_noop() {
    let app = run_setup(
        r#"
Item "リオ" "A"
RemoveItem "リオ" "存在しない"
"#,
    );
    let items = equipped_items(&app, "ブレイバー");
    assert_eq!(items, vec!["A".to_string()]);
}

#[test]
fn item_on_unknown_unit_is_noop() {
    let app = run_setup(r#"Item "存在しない" "X""#);
    let items = equipped_items(&app, "ブレイバー");
    assert!(items.is_empty());
}

#[test]
fn item_func_returns_nth_equipment() {
    let app = run_setup(
        r#"
Item "リオ" "A"
Item "リオ" "B"
Set v1 Item("リオ",1)
Set v2 Item("リオ",2)
Set v3 Item("リオ",3)
"#,
    );
    assert_eq!(app.script_var("v1"), "A");
    assert_eq!(app.script_var("v2"), "B");
    assert_eq!(app.script_var("v3"), ""); // 範囲外
}

#[test]
fn has_item_returns_1_or_0() {
    let app = run_setup(
        r#"
Item "リオ" "X"
Set yes HasItem("リオ","X")
Set no HasItem("リオ","Y")
"#,
    );
    assert_eq!(app.script_var("yes"), "1");
    assert_eq!(app.script_var("no"), "0");
}

#[test]
fn exchange_item_replaces_one_item_with_another() {
    let app = run_setup(
        r#"
Item "リオ" "A"
Item "リオ" "B"
Item "リオ" "C"
ExchangeItem "リオ" "B" "X"
"#,
    );
    let items = equipped_items(&app, "ブレイバー");
    assert_eq!(
        items,
        vec!["A".to_string(), "X".to_string(), "C".to_string()]
    );
}

#[test]
fn exchange_item_unknown_target_is_noop() {
    let app = run_setup(
        r#"
Item "リオ" "A"
ExchangeItem "存在しない" "A" "B"
"#,
    );
    let items = equipped_items(&app, "ブレイバー");
    assert_eq!(items, vec!["A".to_string()]);
}

#[test]
fn countitem_returns_equipped_count() {
    let app = run_setup(
        r#"
Item "リオ" "A"
Item "リオ" "B"
Set v CountItem("リオ")
"#,
    );
    assert_eq!(app.script_var("v"), "2");
}

#[test]
fn countitem_zero_when_no_items() {
    let app = run_setup(r#"Set v CountItem("リオ")"#);
    assert_eq!(app.script_var("v"), "0");
}

// ============================================================
//  未装備在庫 (spare_items) — RemoveItem (取り外し) / Item(未装備,n)
// ============================================================

#[test]
fn removeitem_without_item_moves_all_to_spare() {
    // RemoveItem unit (item 省略) は全アイテムを取り外して未装備在庫へ。
    let app = run_setup(
        r#"
Item "リオ" "A"
Item "リオ" "B"
RemoveItem "リオ"
"#,
    );
    assert!(
        equipped_items(&app, "ブレイバー").is_empty(),
        "ユニットから外れる"
    );
    assert_eq!(
        app.database().spare_items,
        vec!["A".to_string(), "B".to_string()]
    );
}

#[test]
fn removeitem_with_item_deletes_not_pooled() {
    // RemoveItem unit item (item 指定) は削除 = 在庫には残さない。
    let app = run_setup(
        r#"
Item "リオ" "A"
Item "リオ" "B"
RemoveItem "リオ" "A"
"#,
    );
    assert_eq!(equipped_items(&app, "ブレイバー"), vec!["B".to_string()]);
    assert!(
        app.database().spare_items.is_empty(),
        "削除分は在庫に入らない"
    );
}

#[test]
fn countitem_spare_returns_pool_size() {
    let app = run_setup(
        r#"
Item "リオ" "A"
Item "リオ" "B"
RemoveItem "リオ"
Set v CountItem("未装備")
"#,
    );
    assert_eq!(app.script_var("v"), "2");
}

#[test]
fn item_func_unequipped_returns_nth_spare() {
    let app = run_setup(
        r#"
Item "リオ" "A"
Item "リオ" "B"
RemoveItem "リオ"
Set v1 Item("未装備",1)
Set v2 Item("未装備",2)
Set v3 Item("未装備",3)
"#,
    );
    assert_eq!(app.script_var("v1"), "A");
    assert_eq!(app.script_var("v2"), "B");
    assert_eq!(app.script_var("v3"), "", "範囲外は空文字");
}

#[test]
fn itemid_unequipped_returns_nth_spare() {
    let app = run_setup(
        r#"
Item "リオ" "A"
RemoveItem "リオ"
Set v ItemID("未装備",1)
"#,
    );
    assert_eq!(app.script_var("v"), "A");
}

#[test]
fn equip_from_spare_removes_from_pool() {
    // 在庫のアイテムを別ユニットへ装備すると在庫から取り出される。
    let app = run_setup(
        r#"
Item "リオ" "A"
Item "リオ" "B"
RemoveItem "リオ"
Item "リオ" "A"
Set v CountItem("未装備")
"#,
    );
    // A は在庫から取り出され、B のみ残る
    assert_eq!(app.script_var("v"), "1");
    assert_eq!(app.database().spare_items, vec!["B".to_string()]);
    assert!(equipped_items(&app, "ブレイバー").iter().any(|i| i == "A"));
}
