//! どのトピックにも入らないテスト。

use super::*;

#[test]
fn moving_one_of_duplicate_units_leaves_twin_in_place() {
    // 同型 (同 unit_data_name) のユニットが複数いても uid で区別され、
    // 移動したユニットだけが動き、もう一方は出撃地点に残る。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Twin", 2, 6); // UnitData Twin + instance #1
    let twin2 = app.database_mut().register_unit(crate::UnitInstance::new(
        "Twin",
        "PILOT",
        crate::Party::Player,
        5,
        6,
    ));
    let twin1 = app.database().uid_at(2, 6).unwrap().to_string();
    app.set_stage_state(crate::stage::StageState::Battle);

    // (2,6) の Twin を選択して (3,6) へ移動
    click_tile(&mut app, 2, 6);
    click_menu_item(&mut app, 0); // Move
    click_tile(&mut app, 3, 6);

    assert_eq!(
        app.database().unit_by_uid(&twin1).map(|u| (u.x, u.y)),
        Some((3, 6)),
        "選択した Twin が移動していない"
    );
    assert_eq!(
        app.database().unit_by_uid(&twin2).map(|u| (u.x, u.y)),
        Some((5, 6)),
        "もう一方の Twin が巻き込まれて動いている"
    );
    assert!(app.database().pos_index_is_consistent());
}

#[test]
fn call_condition_with_multibyte_neighbors_does_not_panic() {
    // 回帰テスト: 条件式中の `Call(...)` スキャンが `src[i..i+5]` の文字列
    // スライスでマルチバイト文字の途中を踏んで panic していた
    // (実例: game_4011 の `Call(版権ＢＧＭ確認) = あり`)。バイト列比較に
    // 修正後は正常に評価できる。
    let src = "\
メイン:
If Call(版権確認) = あり Then
Set 結果 当たり
EndIf
If (0 >= 1 And 通常ルート) Or (Call(版権確認) = あり And 分岐) Then
Set 結果2 1
EndIf
Exit
版権確認:
Return あり
";
    let stmts = crate::data::event::parse(src).unwrap();
    let mut app = App::new();
    app.script_library_mut().append(&stmts);
    crate::event_runtime::trigger_label(&mut app, "メイン");
    assert_eq!(app.script_var("結果"), "当たり");
}
