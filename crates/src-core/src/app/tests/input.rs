//! 入力 (クリック・キー・メニュー・移動 / キャンセル・設定画面) のテスト。

use super::*;

/// 回帰: プレーンな Ask(Menu) の選択肢を **クリックして選べる** こと。
/// 旧実装は Menu への任意クリックを `respond_dialog(0)` (キャンセル) にして
/// いたため、難易度/キャラ選択がクリックで確定できず `選択 = 0` になり、
/// 東方夢想伝ではキャラ未選択 → 味方 0 体 → 即敗北していた。
#[test]
fn clicking_plain_ask_menu_option_selects_it() {
    let mut app = App::new();
    app.set_pending_dialog(crate::dialog::PendingDialog::Menu {
        prompt: "どの難易度？".to_string(),
        options: vec!["Easy".into(), "Normal".into(), "Hard".into()],
        var_name: "選択".to_string(),
        store_value: false,
        option_keys: Vec::new(),
        non_cancellable: false,
    });
    // CANVAS 480: 選択肢 2 行目は y∈[324,344)。x は枠内。
    assert!(app.handle_input(Input::ClickAt { x: 120, y: 330 }));
    assert_eq!(app.script_var("選択"), "2", "2 番目の選択肢が確定する");
    assert!(app.pending_dialog().is_none(), "確定で dialog は閉じる");
}

/// プレーン Ask の選択肢**外**クリックはキャンセルせず無反応 (dialog 継続)。
#[test]
fn clicking_outside_plain_ask_options_is_noop() {
    let mut app = App::new();
    app.set_pending_dialog(crate::dialog::PendingDialog::Menu {
        prompt: "どの難易度？".to_string(),
        options: vec!["Easy".into(), "Normal".into(), "Hard".into()],
        var_name: "選択".to_string(),
        store_value: false,
        option_keys: Vec::new(),
        non_cancellable: false,
    });
    // 選択肢列より上 (prompt 付近) をクリック。
    let _ = app.handle_input(Input::ClickAt { x: 120, y: 270 });
    assert_eq!(app.script_var("選択"), "", "未選択のまま");
    assert!(
        app.pending_dialog().is_some(),
        "枠内でも選択肢外なら dialog は閉じない (キャンセルしない)"
    );
}

/// 回帰: キャンセル不可の Ask は choice 0 (Esc / 任意進行) を拒否し、選択を強制する。
/// `キャンセル可` のない `Ask` (キャラ選択等) をキャンセルできると `選択 = 0` で
/// 味方 0 体 → 即敗北になるため。
#[test]
fn non_cancellable_ask_rejects_cancel() {
    let mut app = App::new();
    app.set_pending_dialog(crate::dialog::PendingDialog::Menu {
        prompt: "霊夢と魔理沙、どっちが好き？".to_string(),
        options: vec!["もちろん霊夢".into(), "当然、魔理沙".into()],
        var_name: "選択".to_string(),
        store_value: false,
        option_keys: Vec::new(),
        non_cancellable: true,
    });
    // Esc (Cancel) / 任意進行 (Advance=choice 0) は拒否され dialog 継続。
    let _ = app.handle_input(Input::Cancel);
    assert!(
        app.pending_dialog().is_some(),
        "キャンセル不可の Ask は Esc で閉じない"
    );
    let _ = app.respond_dialog(0);
    assert!(
        app.pending_dialog().is_some(),
        "キャンセル不可の Ask は choice 0 で閉じない"
    );
    assert_eq!(app.script_var("選択"), "", "選択は確定していない");
    // 正規の選択 (choice 1) は通る。
    assert!(app.respond_dialog(1));
    assert_eq!(app.script_var("選択"), "1");
    assert!(app.pending_dialog().is_none());
}

#[test]
fn advance_walks_scene_loop() {
    let mut app = App::new();
    // Title → Configuration → MapView。MapView 以降の Advance は no-op で、
    // ステージ進行 (Briefing → Sortie → Battle) は Enter でゲートしない
    // （auto_progress / ラベル発火が担う）。PilotList / UnitList は専用 Input で。
    assert_eq!(app.scene(), Scene::Title);
    app.handle_input(Input::Advance);
    assert_eq!(app.scene(), Scene::Configuration);
    app.handle_input(Input::Advance);
    assert_eq!(app.scene(), Scene::MapView);
    // start_scenario を経ていないため stage は空で auto_progress は走らず
    // Briefing のまま。Advance は何もしない（独自の入力待機を撤去済み）。
    assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);
    assert!(!app.handle_input(Input::Advance));
    assert_eq!(app.scene(), Scene::MapView);
    assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);
    // 明示的に PilotList へ
    app.handle_input(Input::GotoPilotList);
    assert_eq!(app.scene(), Scene::PilotList);
    app.handle_input(Input::GotoUnitList);
    assert_eq!(app.scene(), Scene::UnitList);
    app.handle_input(Input::GotoMapView);
    assert_eq!(app.scene(), Scene::MapView);
}

#[test]
fn click_on_title_moves_to_configuration() {
    let mut app = App::new();
    assert!(app.handle_input(Input::ClickAt { x: 100, y: 100 }));
    assert_eq!(app.scene(), Scene::Configuration);
}

#[test]
fn click_on_checkbox_toggles_setting() {
    let mut app = App::new();
    app.handle_input(Input::Advance);
    assert_eq!(app.scene(), Scene::Configuration);

    let l = ConfigurationLayout::original();
    let r = l.battle_animation.bounds;
    assert!(app.settings().battle_animation);
    assert!(click(&mut app, r.x + 4, TITLE_BAR_HEIGHT + r.y + 5));
    assert!(!app.settings().battle_animation);
}

#[test]
fn click_ok_goes_to_main_and_keeps_settings() {
    let mut app = App::new();
    app.handle_input(Input::Advance);

    // checkbox を切り替える
    let l = ConfigurationLayout::original();
    let r = l.move_animation.bounds;
    click(&mut app, r.x + 4, TITLE_BAR_HEIGHT + r.y + 5);
    assert!(!app.settings().move_animation);

    // OK
    let ok = l.ok.bounds;
    assert!(click(&mut app, ok.x + 10, TITLE_BAR_HEIGHT + ok.y + 5));
    assert_eq!(app.scene(), Scene::MapView);
    assert!(!app.settings().move_animation); // 維持
}

#[test]
fn click_cancel_reverts_changes() {
    let mut app = App::new();
    app.handle_input(Input::Advance);

    let l = ConfigurationLayout::original();
    let r = l.move_animation.bounds;
    click(&mut app, r.x + 4, TITLE_BAR_HEIGHT + r.y + 5);
    assert!(!app.settings().move_animation);

    // Cancel
    let c = l.cancel.bounds;
    assert!(click(&mut app, c.x + 10, TITLE_BAR_HEIGHT + c.y + 5));
    assert_eq!(app.scene(), Scene::MapView);
    assert!(app.settings().move_animation); // 巻き戻し
}

#[test]
fn click_message_speed_cycles() {
    let mut app = App::new();
    app.handle_input(Input::Advance);

    let l = ConfigurationLayout::original();
    let r = l.message_speed_combo;
    let start = app.settings().message_speed;
    click(&mut app, r.x + 4, TITLE_BAR_HEIGHT + r.y + 5);
    assert_ne!(app.settings().message_speed, start);
}

#[test]
fn click_mp3_bar_sets_volume() {
    let mut app = App::new();
    app.handle_input(Input::Advance);

    let l = ConfigurationLayout::original();
    let r = l.mp3_volume_scroll;
    // 右端付近 → 高音量
    click(&mut app, r.x + r.w as i32 - 1, TITLE_BAR_HEIGHT + r.y + 5);
    assert!(app.settings().mp3_volume >= 90);
    // 左端付近 → 低音量
    click(&mut app, r.x + 1, TITLE_BAR_HEIGHT + r.y + 5);
    assert!(app.settings().mp3_volume <= 10);
}

#[test]
fn click_on_map_view_sets_cursor() {
    let mut app = App::new();
    app.handle_input(Input::Advance); // Configuration
    app.handle_input(Input::Advance); // MapView
    app.database_mut().replace_map(crate::data::map::demo());
    assert_eq!(app.scene(), Scene::MapView);
    assert!(app.map_cursor().is_none());

    // MapView: 640x456 (LeftMap 448 + RightPanel 192 / Bottom Msg 104)。CANVAS=640x480。
    // offset = (0, 12) (中央配置)。STATUS_BAR_HEIGHT=0, TILE_SIZE=32
    // タイル (1, 0) を狙う: canvas x = 0 + 1*32 + 16 = 48, y = 12 + 0 + 16 = 28
    assert!(app.handle_input(Input::ClickAt { x: 48, y: 28 }));
    assert_eq!(app.map_cursor(), Some((1, 0)));

    // タイル (2, 1)
    assert!(app.handle_input(Input::ClickAt { x: 80, y: 60 }));
    assert_eq!(app.map_cursor(), Some((2, 1)));
}

#[test]
fn click_on_pilotlist_unitlist_loops_back() {
    let mut app = App::new();
    app.handle_input(Input::Advance); // Configuration
    app.handle_input(Input::Advance); // MapView (Briefing)
    app.handle_input(Input::Advance); // MapView (Sortie)
    app.handle_input(Input::Advance); // MapView (Battle)
    app.handle_input(Input::GotoPilotList); // PilotList
    assert!(app.handle_input(Input::ClickAt { x: 50, y: 50 }));
    assert_eq!(app.scene(), Scene::UnitList);
    assert!(app.handle_input(Input::ClickAt { x: 50, y: 50 }));
    assert_eq!(app.scene(), Scene::Title);
}

#[test]
fn arrow_keys_init_cursor_to_origin() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    assert!(app.map_cursor().is_none());
    // 未配置からの Right → (0,0) スタート、その後 Right で (1,0)... ではない。
    // 実装は (0,0) を基準に dir を適用 → (1,0)
    assert!(app.handle_input(Input::MoveCursor(Direction::Right)));
    assert_eq!(app.map_cursor(), Some((1, 0)));
}

#[test]
fn arrow_keys_clamp_to_map_bounds() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    // 左端で Left → 動かない
    app.handle_input(Input::ClickAt { x: 16, y: 28 }); // (0,0)
    assert_eq!(app.map_cursor(), Some((0, 0)));
    assert!(!app.handle_input(Input::MoveCursor(Direction::Left)));
    assert_eq!(app.map_cursor(), Some((0, 0)));
    // 上端で Up
    assert!(!app.handle_input(Input::MoveCursor(Direction::Up)));
}

#[test]
fn battle_click_opens_unit_menu() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "TestUnit", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    let ox = 2 * 32 + 16;
    let oy = 12 + 6 * 32 + 16;
    app.handle_input(Input::ClickAt { x: ox, y: oy });
    assert_eq!(menu_unit_pos(&app), Some((2, 6)));
}

#[test]
fn menu_move_then_select_destination_moves_unit() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "TestUnit", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    let ox = 2 * 32 + 16;
    let oy = 12 + 6 * 32 + 16;
    app.handle_input(Input::ClickAt { x: ox, y: oy });
    // メニューの "移動" 項目位置をクリック
    let menu_x = crate::scene::command_menu::MENU_X + 40;
    let move_y = crate::scene::command_menu::MENU_Y + crate::scene::command_menu::MENU_PADDING + 4;
    app.handle_input(Input::ClickAt {
        x: menu_x,
        y: move_y,
    });
    assert_eq!(move_select_pos(&app), Some((2, 6)));

    // (3, 6) へクリック
    let dx = 3 * 32 + 16;
    let dy = 12 + 6 * 32 + 16;
    app.handle_input(Input::ClickAt { x: dx, y: dy });
    assert_eq!(app.database().unit_instances[0].x, 3);
    // 移動後はメニューが再表示
    assert_eq!(menu_unit_pos(&app), Some((3, 6)));
}

#[test]
fn cancel_closes_menu_and_action_mode() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "TestUnit", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // クリック→メニュー表示→Cancel で閉じる
    app.handle_input(Input::ClickAt {
        x: 2 * 32 + 16,
        y: 12 + 6 * 32 + 16,
    });
    assert!(app.command_menu().is_some());
    app.handle_input(Input::Cancel);
    assert!(app.command_menu().is_none());
}

#[test]
fn wait_marks_unit_acted() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "TestUnit", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    app.handle_input(Input::ClickAt {
        x: 2 * 32 + 16,
        y: 12 + 6 * 32 + 16,
    });
    // ユニットメニュー: items = [Move, WeaponList, Wait]
    // 最後の項目 (Wait) をクリック
    let menu = app.command_menu().cloned().unwrap();
    let item_count = match &menu {
        crate::CommandMenu::Unit { items, .. } => items.len(),
        _ => panic!("expected Unit menu"),
    };
    let menu_x = crate::scene::command_menu::MENU_X + 40;
    let wait_y = crate::scene::command_menu::MENU_Y
        + crate::scene::command_menu::MENU_PADDING
        + crate::scene::command_menu::MENU_ITEM_HEIGHT * (item_count as i32 - 1)
        + 4;
    app.handle_input(Input::ClickAt {
        x: menu_x,
        y: wait_y,
    });
    assert!(app.database().unit_instances[0].has_acted);
    assert!(app.command_menu().is_none());
}

#[test]
fn post_move_menu_excludes_move_command() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "TestUnit", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // 移動コマンドを選択
    let ox = 2 * 32 + 16;
    let oy = 12 + 6 * 32 + 16;
    app.handle_input(Input::ClickAt { x: ox, y: oy });
    let menu_x = crate::scene::command_menu::MENU_X + 40;
    let move_y = crate::scene::command_menu::MENU_Y + crate::scene::command_menu::MENU_PADDING + 4;
    app.handle_input(Input::ClickAt {
        x: menu_x,
        y: move_y,
    });
    // 移動先をクリック
    app.handle_input(Input::ClickAt {
        x: 3 * 32 + 16,
        y: 12 + 6 * 32 + 16,
    });

    // 移動後メニューに Move が含まれていないこと
    let menu = app.command_menu().unwrap();
    let has_move = match menu {
        crate::CommandMenu::Unit { items, .. } => items.iter().any(|i| {
            matches!(
                i,
                crate::command_menu::UnitMenuItem::Builtin(crate::command_menu::UnitAction::Move)
            )
        }),
        _ => false,
    };
    assert!(!has_move, "post-move menu should not contain Move");
}

// ===== REPRO: 移動→攻撃後にユニットが初期位置に戻る不具合の調査 =====
/// ステータスパネルの判定 (武器の使用可否・戦闘予測) は src-core が実際の攻撃と同じ
/// 経路で求める。EN 不足の武器は使用不可、予測は射程内の敵へ。
#[test]
fn status_panel_view_uses_core_rules() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 300, 1);
    add_weapon(&mut app, "Hero", 300, 1);
    // 2 本目は EN 9999 を要する (使用不可)。
    app.database_mut()
        .catalog
        .units
        .iter_mut()
        .find(|u| u.name == "Hero")
        .unwrap()
        .weapons[1]
        .en_consumption = 9999;
    spawn_enemy(&mut app, "Hero", 3, 6);
    app.set_map_cursor(2, 6);
    let view = app.status_panel_view().expect("カーソル上にユニットが居る");
    assert_eq!(view.weapon_firable, vec![true, false]);
    let f = view.forecast.expect("射程内の敵への予測");
    assert_eq!(f.target, "P");
    assert!(f.hit_pct > 0 && f.damage > 0);
    // カーソル上にユニットが居なければ None。
    app.set_map_cursor(9, 9);
    assert!(app.status_panel_view().is_none());
}

#[test]
fn selecting_another_unit_after_move_commits_not_reverts() {
    // 移動後メニュー表示中に別ユニットをクリックしたら、移動済みユニットは
    // その場に留まり (巻き戻らず)、別ユニットのメニューが開く。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Alpha", 2, 6);
    place_player_unit(&mut app, "Bravo", 6, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // Alpha 選択 → Move → (3,6)
    click_tile(&mut app, 2, 6);
    click_menu_item(&mut app, 0); // Move
    click_tile(&mut app, 3, 6);
    assert_eq!(unit_xy(&app, "Alpha"), Some((3, 6)));

    // 移動後メニュー表示中に別ユニット Bravo(6,6) をクリック
    click_tile(&mut app, 6, 6);
    assert_eq!(
        unit_xy(&app, "Alpha"),
        Some((3, 6)),
        "別ユニット選択で前に移動したユニットが巻き戻ってはいけない"
    );
    assert_eq!(
        menu_unit_pos(&app),
        Some((6, 6)),
        "クリックした別ユニットのメニューが開くべき"
    );
    assert!(app.database().pos_index_is_consistent());
}

#[test]
fn wait_after_move_commits_and_unsticks_action_mode() {
    // 移動 → 待機 で確定したあと action_mode が Browse に戻り、別ユニットを
    // 選べること、さらに右クリックしても確定済みユニットが巻き戻らないこと。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Alpha", 2, 6);
    place_player_unit(&mut app, "Bravo", 9, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // Alpha: 選択 → Move → (3,6)
    click_tile(&mut app, 2, 6);
    click_menu_item(&mut app, 0); // Move
    click_tile(&mut app, 3, 6);
    // PostMoveMenu の待機を選択 (武器・敵なし → items=[待機] の先頭)
    click_menu_item(&mut app, 0);
    assert_eq!(unit_xy(&app, "Alpha"), Some((3, 6)));
    assert!(
        matches!(app.action_mode(), crate::ActionMode::Browse),
        "待機確定後は Browse に戻るべき: {:?}",
        app.action_mode()
    );

    // 続けて別ユニット Bravo を選択できる
    click_tile(&mut app, 9, 6);
    assert_eq!(
        menu_unit_pos(&app),
        Some((9, 6)),
        "別ユニットを選択できるべき"
    );

    // 右クリックしても Alpha は確定位置 (3,6) のまま
    app.handle_input(Input::Cancel);
    assert_eq!(
        unit_xy(&app, "Alpha"),
        Some((3, 6)),
        "待機確定後に巻き戻ってはいけない"
    );
    assert!(app.database().pos_index_is_consistent());
}

#[test]
fn cancel_post_move_restores_unit_position() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "TestUnit", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // 移動
    let ox = 2 * 32 + 16;
    let oy = 12 + 6 * 32 + 16;
    app.handle_input(Input::ClickAt { x: ox, y: oy });
    let menu_x = crate::scene::command_menu::MENU_X + 40;
    let move_y = crate::scene::command_menu::MENU_Y + crate::scene::command_menu::MENU_PADDING + 4;
    app.handle_input(Input::ClickAt {
        x: menu_x,
        y: move_y,
    });
    app.handle_input(Input::ClickAt {
        x: 3 * 32 + 16,
        y: 12 + 6 * 32 + 16,
    });
    assert_eq!(app.database().unit_instances[0].x, 3);

    // 移動後メニューでキャンセル → 元の位置に戻る
    app.handle_input(Input::Cancel);
    assert_eq!(
        app.database().unit_instances[0].x,
        2,
        "unit should be restored to original x"
    );
    assert!(
        !app.database().unit_instances[0].has_moved,
        "has_moved should be cleared"
    );
    assert!(app.command_menu().is_none());
}

#[test]
fn click_in_range_moves_unit() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    // ブレイバー相当ユニットを (2, 6) に置く。speed=3 で (3, 6) は移動範囲内（平地コスト1）
    place_player_unit(&mut app, "TestUnit", 2, 6);

    // カーソルをユニット上に
    let click_unit = (2u32, 6u32);
    let ox = (click_unit.0 as i32) * 32 + 16;
    let oy = 12 + (click_unit.1 as i32) * 32 + 16;
    app.handle_input(Input::ClickAt { x: ox, y: oy });
    assert_eq!(app.map_cursor(), Some(click_unit));

    // (3, 6) へクリック → 移動
    let dest = (3u32, 6u32);
    let dx = (dest.0 as i32) * 32 + 16;
    let dy = 12 + (dest.1 as i32) * 32 + 16;
    app.handle_input(Input::ClickAt { x: dx, y: dy });

    let u = &app.database().unit_instances[0];
    assert_eq!((u.x, u.y), dest);
    assert_eq!(app.map_cursor(), Some(dest));
}

#[test]
fn click_out_of_range_just_moves_cursor() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "TestUnit", 2, 6); // speed=3

    // カーソルをユニット上に
    let ox = 2 * 32 + 16;
    let oy = 12 + 6 * 32 + 16;
    app.handle_input(Input::ClickAt { x: ox, y: oy });

    // (10, 6) は遠すぎる → カーソル移動のみ
    let dx = 10 * 32 + 16;
    let dy = 12 + 6 * 32 + 16;
    app.handle_input(Input::ClickAt { x: dx, y: dy });

    let u = &app.database().unit_instances[0];
    assert_eq!((u.x, u.y), (2, 6));
    assert_eq!(app.map_cursor(), Some((10, 6)));
}

#[test]
fn arrow_keys_ignored_outside_mapview() {
    let mut app = App::new();
    // Title 上で Right を送っても何も起きない
    assert!(!app.handle_input(Input::MoveCursor(Direction::Right)));
    assert_eq!(app.scene(), Scene::Title);
}

#[test]
fn escape_cancels_hotpoint_wait_click_screen() {
    // Esc (Input::Cancel) で Hotpoint Wait Click 画面を右クリック相当で「戻る」。
    // トラックパッドの副ボタンが効かない環境でも確実に抜けられることの保証。
    let src = "\
状態:
Hotpoint タブ 10 10 50 50
Do
  Wait Click
  Switch 選択
  Case \"\"
    If KeyState(2) Then
      Set 結果 終了
      Break
    EndIf
  EndSw
Loop While (1)
Exit
";
    let stmts = crate::data::event::parse(src).unwrap();
    let mut app = App::new();
    app.script_library_mut().append(&stmts);
    crate::event_runtime::trigger_label(&mut app, "状態");
    assert!(
        app.pending_dialog().is_some(),
        "Wait Click で中断していない"
    );
    // Esc → Input::Cancel → 右クリック相当でループ脱出
    app.handle_input(Input::Cancel);
    assert!(
        app.pending_dialog().is_none(),
        "Esc で Wait Click を抜けられていない"
    );
    assert_eq!(app.script_var("結果"), "終了");
}

#[test]
fn keystate2_one_shot_no_infinite_release_loop() {
    // スパロボ戦記 AlphaSecond L65: ステータス画面を右クリックで抜けた直後の
    // `Do While KeyState(2) Loop` (ボタン解放待ち) が無限ループしないこと。
    // KeyState(2) はワンショット消費なので、解放待ちループは即座に 0 を見て抜ける。
    let src = "\
状態:
Hotpoint タブ 10 10 50 50
Do
  Wait Click
  Switch 選択
  Case \"\"
    If KeyState(2) Then
      Break
    EndIf
  EndSw
Loop While (1)
Do While KeyState(2)
Loop
Set 完了 1
Exit
";
    let stmts = crate::data::event::parse(src).unwrap();
    let mut app = App::new();
    app.script_library_mut().append(&stmts);
    crate::event_runtime::trigger_label(&mut app, "状態");
    assert!(app.pending_dialog().is_some());
    // 右クリック相当で抜ける → 解放待ち `Do While KeyState(2)` を無限ループせず通過
    app.handle_input(Input::Cancel);
    assert!(app.pending_dialog().is_none());
    assert_eq!(
        app.script_var("完了"),
        "1",
        "解放待ちループを抜けて最後まで到達するはず"
    );
    assert!(
        app.last_script_error().is_none(),
        "STEP_LIMIT (無限ループ) が発生した: {:?}",
        app.last_script_error()
    );
}

#[test]
fn click_advances_talk_dialog() {
    // Talk モーダル中はクリックでも Advance できる (旧実装は無反応)。
    let mut app = App::new();
    app.set_pending_dialog(crate::dialog::PendingDialog::Talk {
        speaker: String::new(),
        body: "セリフ".to_string(),
    });
    // クリック → respond_dialog(0) 相当
    assert!(app.handle_input(Input::ClickAt { x: 100, y: 100 }));
    assert!(app.pending_dialog().is_none());
}

#[test]
fn click_advances_wait_click_dialog() {
    // `Wait Click` の合成ダイアログ (WaitClick variant) もクリック / Enter で
    // resume できる。レンダリングは行わず、原典 SRC のように画面はそのまま。
    let mut app = App::new();
    app.set_pending_dialog(crate::dialog::PendingDialog::WaitClick);
    assert!(app.handle_input(Input::ClickAt { x: 50, y: 50 }));
    assert!(app.pending_dialog().is_none());

    let mut app = App::new();
    app.set_pending_dialog(crate::dialog::PendingDialog::WaitClick);
    assert!(app.handle_input(Input::Advance));
    assert!(app.pending_dialog().is_none());
}
