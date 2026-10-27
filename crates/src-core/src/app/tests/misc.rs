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

/// `Talk` の中央表示 (原典 `ExecTalkCmd` / `CenterUnit`) と `Center` のスクロール。
/// 話者だけなら話者のユニット (プロローグ中は行わない)、`中央` は常に話者、`固定` は
/// 行わない、`X Y` は座標。中央表示はマップ端でスクロールが範囲内に収まる。
#[test]
fn talk_and_center_scroll_map_to_center() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    app.database_mut()
        .replace_map(crate::data::map::MapData::new(40, 40));
    place_player_unit(&mut app, "Hero", 30, 25); // パイロット PILOT (愛称 P)
    let run = |app: &mut App, src: &str| {
        let stmts = crate::data::event::parse(&format!("{src}\n")).unwrap();
        crate::event_runtime::execute(app, &stmts).unwrap();
        if app.pending_dialog().is_some() {
            app.handle_input(Input::Advance);
        }
    };
    let talk = |app: &mut App, head: &str| run(app, &format!("{head}\nメッセージ\nEnd"));
    let reset = |app: &mut App| app.center_map_on(0, 0);

    app.set_stage_state(crate::stage::StageState::Battle);
    talk(&mut app, "Talk P");
    assert_eq!(app.map_cursor(), Some((30, 25)));
    assert_eq!(app.map_scroll(), (23, 18), "話者のユニットが中央に来る");

    reset(&mut app);
    talk(&mut app, "Talk P 固定");
    assert_eq!(app.map_scroll(), (0, 0), "固定 は中央表示しない");

    talk(&mut app, "Talk システム 10 12");
    assert_eq!(app.map_scroll(), (3, 5), "座標指定");

    talk(&mut app, "Talk システム 39 39");
    assert_eq!(app.map_scroll(), (25, 25), "マップ端では範囲内に収まる");

    reset(&mut app);
    app.set_stage_state(crate::stage::StageState::Briefing);
    talk(&mut app, "Talk P");
    assert_eq!(
        app.map_scroll(),
        (0, 0),
        "プロローグ中は話者を中央表示しない"
    );
    talk(&mut app, "Talk P 中央");
    assert_eq!(
        app.map_scroll(),
        (23, 18),
        "中央 はプロローグ中でも中央表示する"
    );

    run(&mut app, "Center 35 2");
    assert_eq!(app.map_cursor(), Some((34, 1)));
    assert_eq!(app.map_scroll(), (25, 0), "Center も画面をスクロールする");
}
