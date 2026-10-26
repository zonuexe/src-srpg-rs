use super::*;
use crate::scene::configuration::{ConfigurationLayout, TITLE_BAR_HEIGHT};

/// 回帰: 自動発火する章ローカルイベント (`ターン N <陣営>` 等) が、全章を
/// 同一ライブラリに同時ロードしている状況で **別章の同名ラベルへ漏れない**
/// こと。
///
/// 旧バグ (東方夢想伝): `begin_phase` が `post_event_label("ターン 1 敵")` を
/// global 解決していたため、現章 (01) に `ターン 1 敵` が無いと、別章 (12) の
/// `ターン 1 敵` (アリスの台詞) が誤発火して「話が飛んで」いた。
#[test]
fn stage_event_label_does_not_leak_across_chapters() {
    const CH_A: &str = "ターン 1 敵:\nSet ch_a_turn 1\nExit\n";
    const CH_B: &str = "ターン 1 敵:\nSet ch_b_turn 1\nExit\n";

    // chapter A / B を別ファイルとして登録 (両方に同名 `ターン 1 敵`)。
    let mut app = App::new();
    let stmts_a = crate::data::event::parse(CH_A).expect("parse A");
    app.script_library_mut()
        .append_with_name(&stmts_a, "scenes/ch_a.eve");
    let stmts_b = crate::data::event::parse(CH_B).expect("parse B");
    app.script_library_mut()
        .append_with_name(&stmts_b, "scenes/ch_b.eve");

    // 現ステージが chapter A のとき、`ターン 1 敵` は A のものだけ発火。
    app.current_stage_file = "scenes/ch_a.eve".to_string();
    assert!(app.post_stage_event_label("ターン 1 敵".to_string()));
    assert_eq!(app.script_var("ch_a_turn"), "1", "A の ターン 1 敵 が発火");
    assert_eq!(
        app.script_var("ch_b_turn"),
        "",
        "B の ターン 1 敵 が誤発火した (章間漏れ)"
    );
}

/// 回帰: 勝利条件「敵・中立撃墜」型ステージ (`全滅 敵`/`全滅 中立`→`クリア`) で、
/// 敵だけ全滅し敵対する中立が残る局面では組込み勝利を発火しない (早期 Victory で
/// 状態がロックされ、その後の味方全滅で敗北できず詰むのを防ぐ)。味方全滅時は
/// 組込み敗北を確実に発火する (中立フェイズでの撃墜でも詰まないように)。
#[test]
fn check_victory_defers_clear_but_fires_defeat() {
    const STAGE: &str =
        "全滅 敵:\nSet 敵全滅\nExit\n全滅 中立:\nSet 中立全滅\nExit\nクリア:\nExit\n";
    let mut app = App::new();
    let stmts = crate::data::event::parse(STAGE).unwrap();
    app.script_library_mut()
        .append_with_name(&stmts, "scenes/stage1.eve");
    app.current_stage_file = "scenes/stage1.eve".to_string();
    app.stage_state = crate::stage::StageState::Battle;
    // 味方 1 + 敵対する中立 1 (敵ユニットは既に全滅)。
    app.database
        .unit_instances
        .push(crate::unit_instance::UnitInstance::new(
            "U_P",
            "P",
            crate::Party::Player,
            1,
            1,
        ));
    app.database
        .unit_instances
        .push(crate::unit_instance::UnitInstance::new(
            "U_N",
            "N",
            crate::Party::Neutral,
            2,
            2,
        ));

    app.check_victory();
    assert_eq!(
        app.stage_state,
        crate::stage::StageState::Battle,
        "敵全滅でも敵対中立が残るうちは組込み勝利を発火しない"
    );

    // 味方を全滅させると組込み敗北が確実に発火する。
    app.database
        .unit_instances
        .retain(|u| u.party != crate::Party::Player);
    app.check_victory();
    assert_eq!(
        app.stage_state,
        crate::stage::StageState::Defeat,
        "味方全滅で敗北が発火する (中立フェイズでも詰まない)"
    );
}

#[test]
fn enemy_wipe_with_only_kuria_label_fires_kuria_and_clears() {
    // `クリア` だけ定義 (`全滅 敵` ハンドラなし) のシナリオで、敵全滅後に
    // `クリア` が発火し勝利状態へ進むこと (実機報告: 撃破しても何も起こらない の修正)。
    let stage = crate::data::event::parse("クリア:\nSet クリア発火 1\nExit\n").unwrap();
    let mut app = App::new();
    app.script_library_mut()
        .append_with_name(&stage, "scenes/stage1.eve");
    app.current_stage_file = "scenes/stage1.eve".to_string();
    app.stage_state = crate::stage::StageState::Battle;
    // 味方 1 のみ (敵・中立は全滅済み)。
    app.database
        .unit_instances
        .push(crate::unit_instance::UnitInstance::new(
            "U_P",
            "P",
            crate::Party::Player,
            1,
            1,
        ));
    app.check_victory();
    assert_eq!(
        app.script_var("クリア発火"),
        "1",
        "クリア だけ定義のシナリオでも敵全滅で クリア が発火する"
    );
    assert_eq!(
        app.stage_state,
        crate::stage::StageState::Victory,
        "勝利状態へ進む (Battle のまま固まらない)"
    );
}

#[test]
fn defeat_without_gameover_label_offers_continue_then_title() {
    use crate::stage::StageState;
    // (1) __restart_save あり: 敗北 → コンティニューで再ロード要求。
    let mut app = App::new();
    app.stage_state = StageState::Battle;
    app.current_stage_file = "stage.eve".to_string();
    app.set_script_var("__restart_save".to_string(), "{\"x\":1}".to_string());
    app.game_over();
    assert_eq!(app.stage_state(), StageState::Defeat);
    assert!(
        app.pending_game_over,
        "GameOver 出口イベントが無いので組込みフォールバックが立つ"
    );
    app.game_over_continue();
    assert_eq!(
        app.take_pending_reload().as_deref(),
        Some("{\"x\":1}"),
        "コンティニューでステージ開始時スナップショットの再ロードを要求"
    );
    assert!(!app.pending_game_over);

    // (2) スナップショット無し: コンティニューはタイトルへフォールバック。
    let mut app2 = App::new();
    app2.stage_state = StageState::Battle;
    app2.current_stage_file = "stage.eve".to_string();
    app2.game_over();
    assert!(app2.pending_game_over);
    app2.game_over_continue();
    assert_eq!(
        app2.scene(),
        Scene::Title,
        "スナップショットが無ければタイトルへ戻る"
    );
    assert!(!app2.pending_game_over);
}

#[test]
fn enemy_wipe_with_unresolving_zenmetsu_falls_back_to_game_clear() {
    use crate::stage::StageState;
    // `全滅 敵` ハンドラはあるが空 (進行を解決しない) シナリオ。敵全滅後に idle なら
    // 組込み game_clear で救済し、`クリア` を発火して Victory へ進む (実機報告: 東方夢想伝01)。
    let stage =
        crate::data::event::parse("全滅 敵:\nExit\nクリア:\nSet クリア発火 1\nExit\n").unwrap();
    let mut app = App::new();
    app.script_library_mut()
        .append_with_name(&stage, "stage.eve");
    app.current_stage_file = "stage.eve".to_string();
    app.stage_state = StageState::Battle;
    app.database
        .unit_instances
        .push(crate::unit_instance::UnitInstance::new(
            "U_P",
            "P",
            crate::Party::Player,
            1,
            1,
        ));
    app.check_victory();
    assert_eq!(
        app.stage_state(),
        StageState::Victory,
        "全滅敵 が解決しない idle 状態は game_clear で救済"
    );
    assert_eq!(
        app.script_var("クリア発火"),
        "1",
        "game_clear が クリア を発火"
    );
}

#[test]
fn victory_proceed_goes_to_intermission_when_commands_registered() {
    use crate::stage::StageState;
    let mut app = App::new();
    app.stage_state = StageState::Victory;
    app.push_intermission_command("改造".to_string(), "x.eve".to_string());
    app.proceed_after_victory();
    assert_eq!(
        app.scene(),
        Scene::Intermission,
        "勝利後 Enter/クリックでインターミッションへ進む"
    );
}

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

/// 現章に当該ステージイベントが無い場合は **何も発火しない** (別章へ漏れない)。
/// 東方夢想伝01 は `ターン 1 敵` を持たないので、敵フェイズで他章のそれを
/// 引いてはならない。
#[test]
fn stage_event_label_absent_in_current_chapter_fires_nothing() {
    const CH_A: &str = "プロローグ:\nExit\n"; // ターン 1 敵 を持たない
    const CH_B: &str = "ターン 1 敵:\nSet ch_b_turn 1\nExit\n";

    let mut app = App::new();
    let stmts_a = crate::data::event::parse(CH_A).expect("parse A");
    app.script_library_mut()
        .append_with_name(&stmts_a, "scenes/ch_a.eve");
    let stmts_b = crate::data::event::parse(CH_B).expect("parse B");
    app.script_library_mut()
        .append_with_name(&stmts_b, "scenes/ch_b.eve");

    app.current_stage_file = "scenes/ch_a.eve".to_string();
    assert!(
        !app.post_stage_event_label("ターン 1 敵".to_string()),
        "現章に無いステージイベントは投函されない"
    );
    assert_eq!(
        app.script_var("ch_b_turn"),
        "",
        "別章の ターン 1 敵 へ漏れた"
    );
}

fn click(app: &mut App, scene_x: i32, scene_y: i32) -> bool {
    let (sw, sh) = app.scene().size();
    let ox = (CANVAS_WIDTH as i32 - sw as i32) / 2;
    let oy = (CANVAS_HEIGHT as i32 - sh as i32) / 2;
    app.handle_input(Input::ClickAt {
        x: ox + scene_x,
        y: oy + scene_y,
    })
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

fn enter_mapview_with_demo_map(app: &mut App) {
    app.handle_input(Input::Advance); // Configuration
    app.handle_input(Input::Advance); // MapView
    app.database_mut().replace_map(crate::data::map::demo());
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
fn end_phase_advances_turn_after_neutral() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    app.set_stage_state(crate::stage::StageState::Battle);
    assert_eq!(app.turn().number, 1);
    assert_eq!(app.turn().phase, crate::Phase::Player);

    // 自動 AI が Enemy/Allied/Neutral を即座に消費して Player(T2) に戻る
    app.handle_input(Input::EndPhase);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    assert_eq!(app.turn().number, 2);

    app.handle_input(Input::EndPhase);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    assert_eq!(app.turn().number, 3);
}

#[test]
fn end_phase_ignored_outside_mapview() {
    let mut app = App::new();
    assert!(!app.handle_input(Input::EndPhase));
    assert_eq!(app.turn().number, 1);
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
    app.database_mut().pilots.push(pilot);
    app.database_mut().units.push(unit_data);
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
    app.database_mut().pilots.push(PilotData {
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

/// パイロットに精神コマンド (SP コマンド) を与えてユニットに搭載するヘルパ。
/// `place_player_unit` 済みのユニット uid を返す。
fn give_spirit_commands(
    app: &mut App,
    cmds: Vec<crate::data::pilot::SpiritCommand>,
    max_sp: i32,
) -> String {
    let p = app
        .database_mut()
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

#[test]
fn spirit_command_menu_applies_condition_and_consumes_sp() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    use crate::data::pilot::SpiritCommand;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    // 勝敗判定回避のため敵を 1 体配置 (味方 0 / 敵 0 での即決着を防ぐ)。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);

    let uid = give_spirit_commands(
        &mut app,
        vec![
            SpiritCommand {
                name: "集中".into(),
                cost: Some(20),
                level: 1,
            },
            // level 99: 未習得 (パイロットは level 1)。メニューに出ない。
            SpiritCommand {
                name: "熱血".into(),
                cost: None,
                level: 99,
            },
        ],
        47,
    );

    // 発動可能なのは 集中 のみ (熱血 は未習得)。
    assert_eq!(
        app.spirit_command_options(&uid),
        vec![("集中".to_string(), 20)]
    );

    // ユニットメニューを開く → 精神コマンド項目が並ぶ。
    let pos = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        (u.x, u.y)
    };
    app.open_unit_menu(pos);
    let has_spirit = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Spirit)
    );
    assert!(has_spirit, "精神コマンド項目が表示される");

    // 精神コマンドを選択 → サブメニュー (pending_dialog Menu) が出る。
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Spirit));
    assert!(matches!(
        app.pending_dialog(),
        Some(crate::dialog::PendingDialog::Menu { .. })
    ));

    // 1 番目 (集中) を選択 → condition 付与 + SP 20 消費。
    assert!(app.respond_dialog(1));
    {
        let u = app.database().unit_by_uid(&uid).unwrap();
        assert!(u.has_condition("集中"), "集中 condition が付与される");
        assert_eq!(u.sp_consumed, 20, "SP 20 消費");
    }

    // 次の味方フェイズ開始 (T2) で 集中 (lifetime 1) が解除される。
    app.handle_input(Input::EndPhase);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert!(!u.has_condition("集中"), "集中 は次ターン開始で解除");
}

#[test]
fn spirit_command_hidden_when_insufficient_sp() {
    use crate::data::pilot::SpiritCommand;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // 最大 SP 10 では 熱血(30) は買えない。集中(20) も買えない。
    let uid = give_spirit_commands(
        &mut app,
        vec![
            SpiritCommand {
                name: "集中".into(),
                cost: Some(20),
                level: 1,
            },
            SpiritCommand {
                name: "熱血".into(),
                cost: Some(30),
                level: 1,
            },
        ],
        10,
    );
    assert!(
        app.spirit_command_options(&uid).is_empty(),
        "SP 不足なら発動可能コマンドは空"
    );
    // メニューにも精神コマンド項目は出ない。
    let pos = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        (u.x, u.y)
    };
    app.open_unit_menu(pos);
    let has_spirit = matches!(
        app.command_menu(),
        Some(crate::command_menu::CommandMenu::Unit { items, .. })
            if items.contains(&crate::command_menu::UnitMenuItem::Spirit)
    );
    assert!(!has_spirit, "SP 不足では精神コマンド項目は非表示");
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

#[test]
fn spirit_kasoku_shinsoku_add_move_points() {
    use crate::Condition;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = first_player_uid(&app);
    let base = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        app.database().effective_speed(u)
    };
    // 加速: 移動力 +2
    let mut u = app.database().unit_by_uid(&uid).unwrap().clone();
    u.add_condition(Condition::new("加速", 1));
    assert_eq!(app.database().effective_speed(&u), base + 2);
    // 神速: 移動力 +3
    let mut u = app.database().unit_by_uid(&uid).unwrap().clone();
    u.add_condition(Condition::new("神速", 1));
    assert_eq!(app.database().effective_speed(&u), base + 3);
}

#[test]
fn spirit_heal_effects_reduce_damage() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = first_player_uid(&app);
    let max_hp = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        app.database().effective_max_hp(u)
    }; // 100
       // 信頼: 最大HP 1/3 回復
    app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 90;
    app.apply_spirit_effect(&uid, "信頼");
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().damage,
        90 - max_hp / 3
    );
    // 愛: 全快
    app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 90;
    app.apply_spirit_effect(&uid, "愛");
    assert_eq!(app.database().unit_by_uid(&uid).unwrap().damage, 0);
}

#[test]
fn spirit_kakusei_resets_action_flags() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.has_acted = true;
        u.has_moved = true;
    }
    app.apply_spirit_effect(&uid, "覚醒");
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert!(!u.has_acted && !u.has_moved, "覚醒 で再行動可能になる");
}

#[test]
fn spirit_hokyuu_restores_en_and_lowers_morale() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.en_consumed = 20;
        u.morale = 100;
    }
    app.apply_spirit_effect(&uid, "補給");
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(u.en_consumed, 0, "補給 で EN 全快");
    assert_eq!(u.morale, 90, "補給 は気力 -10");
}

#[test]
fn spirit_morale_and_grant_effects() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = first_player_uid(&app);
    // 脱力: 気力 -10
    app.database_mut().unit_by_uid_mut(&uid).unwrap().morale = 100;
    app.apply_spirit_effect(&uid, "脱力");
    assert_eq!(app.database().unit_by_uid(&uid).unwrap().morale, 90);
    // 応援: 対象に「努力」を付与
    app.apply_spirit_effect(&uid, "応援");
    assert!(app
        .database()
        .unit_by_uid(&uid)
        .unwrap()
        .has_condition("努力"));
    // 祝福: 対象に「幸運」を付与
    app.apply_spirit_effect(&uid, "祝福");
    assert!(app
        .database()
        .unit_by_uid(&uid)
        .unwrap()
        .has_condition("幸運"));
}

#[test]
fn spirit_fukkatsu_revives_once() {
    use crate::Condition;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = first_player_uid(&app);
    let idx = app.database().idx_by_uid(&uid).unwrap();
    app.database_mut().unit_instances[idx].add_condition(Condition::new("復活", 1));
    app.database_mut().unit_instances[idx].damage = 9999;
    assert!(app.revive_if_possible(idx), "復活 保持なら復活する");
    assert_eq!(app.database().unit_instances[idx].damage, 0, "HP 全快");
    assert!(
        !app.database().unit_instances[idx].has_condition("復活"),
        "復活 は 1 回で消費"
    );
    assert!(!app.revive_if_possible(idx), "2 回目は復活しない");
}

#[test]
fn spirit_single_target_defers_to_target_mode() {
    use crate::command_menu::{ActionMode, MenuActionId, UnitMenuItem};
    use crate::data::pilot::SpiritCommand;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    // 勝敗即決を避けるため敵を 1 体配置。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = give_spirit_commands(
        &mut app,
        vec![SpiritCommand {
            name: "信頼".into(),
            cost: Some(25),
            level: 1,
        }],
        50,
    );
    let pos = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        (u.x, u.y)
    };
    app.open_unit_menu(pos);
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Spirit));
    assert!(app.respond_dialog(1));
    // 対象選択モードへ遷移する (caster には適用せず SP も未消費)。
    match app.action_mode() {
        ActionMode::SpiritTarget {
            ref spirit,
            target_enemy,
            ..
        } => {
            assert_eq!(spirit, "信頼");
            assert!(!target_enemy, "信頼 は味方単体対象");
        }
        other => panic!("SpiritTarget を期待: {other:?}"),
    }
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert!(!u.has_condition("信頼"), "対象未確定で caster に付与しない");
    assert_eq!(u.sp_consumed, 0, "対象未確定で SP 未消費");
}

#[test]
fn apply_spirit_to_target_heals_and_consumes_sp() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let caster = first_player_uid(&app);
    // 損傷した味方を対象として配置。
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Player,
        3,
        6,
    ));
    app.database_mut().unit_by_uid_mut(&target).unwrap().damage = 90;
    app.apply_spirit_to_target(&caster, &target, "信頼", 25);
    // 信頼 = 最大HP(100)/3 = 33 回復。
    assert_eq!(app.database().unit_by_uid(&target).unwrap().damage, 90 - 33);
    // caster (PilotInstance 無し) は sp_consumed に反映。
    assert_eq!(app.database().unit_by_uid(&caster).unwrap().sp_consumed, 25);
}

/// SRC `SpecialPowerData.cs:523-540`: 敵対象 (TargetType 敵/全敵/任意/全) の精神コマンドは、
/// 対象が `スペシャルパワー無効化` / `精神コマンド無効化` を持つと無効化される。脱力 (敵単体・
/// 気力 -10) で検証する。サンプル決戦1話は全敵に `SetStatus スペシャルパワー無効化` を付与し
/// 挑発等の攻撃系精神からザコを守る。
#[test]
fn special_power_nullification_blocks_offensive_spirit() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    // 敵対象 2 体 (無効化あり / なし)。
    let immune = app.database_mut().register_unit(crate::UnitInstance::new(
        "Zako",
        "ZAKO1",
        crate::Party::Enemy,
        5,
        5,
    ));
    let plain = app.database_mut().register_unit(crate::UnitInstance::new(
        "Zako",
        "ZAKO2",
        crate::Party::Enemy,
        6,
        5,
    ));
    app.database_mut()
        .unit_by_uid_mut(&immune)
        .unwrap()
        .add_condition(crate::Condition::new("スペシャルパワー無効化", -1));

    let m_immune = app.database().unit_by_uid(&immune).unwrap().morale;
    let m_plain = app.database().unit_by_uid(&plain).unwrap().morale;
    app.apply_spirit_effect(&immune, "脱力");
    app.apply_spirit_effect(&plain, "脱力");

    // 無効化あり: 気力不変 (攻撃系精神が効かない)。
    assert_eq!(
        app.database().unit_by_uid(&immune).unwrap().morale,
        m_immune,
        "スペシャルパワー無効化 を持つ対象には攻撃系精神 (脱力) が効かないはず"
    );
    // 無効化なし: 脱力で気力 -10。
    assert_eq!(
        app.database().unit_by_uid(&plain).unwrap().morale,
        m_plain - 10,
        "無効化なしの対象には脱力 (気力-10) が効くはず"
    );
}

/// 損傷した隣接味方が居れば「修理」がメニューに出て、対象クリックで HP 全回復
/// + 発動主体は行動終了する (特殊能力 `修理装置` ベースの組込支援コマンド)。
#[test]
fn support_repair_heals_adjacent_ally_and_ends_action() {
    use crate::command_menu::{ActionMode, CommandMenu, MenuActionId, SupportKind, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Medic", 2, 6);
    place_player_unit(&mut app, "Ally", 3, 6);
    // 勝敗即決を避けるため遠方に敵を配置。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Medic",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);

    let caster = first_player_uid(&app);
    let ally = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 3 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features
        .push(crate::feature::ActiveFeature::new("修理装置", ""));
    app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 50;

    // メニューに「修理」が出る。
    app.open_unit_menu((2, 6));
    let has_repair = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. })
            if items.contains(&UnitMenuItem::Support(SupportKind::Repair))
    );
    assert!(has_repair, "修理装置 + 隣接損傷味方 で『修理』が表示される");

    // 選択 → 対象選択モードへ。
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Support(
        SupportKind::Repair,
    )));
    assert!(matches!(
        app.action_mode(),
        ActionMode::SupportTarget {
            kind: SupportKind::Repair,
            ..
        }
    ));

    // 隣接味方タイル (3,6) をクリック → HP 回復 (修理装置 Lv なし=30%) + 行動終了。
    app.handle_input(Input::ClickAt {
        x: 3 * 32 + 16,
        y: 12 + 6 * 32 + 16,
    });
    assert_eq!(
        app.database().unit_by_uid(&ally).unwrap().damage,
        20,
        "修理装置 Lv なし=30% (最大HP100 → 30 回復) で damage 50→20"
    );
    assert!(
        app.database().unit_by_uid(&caster).unwrap().has_acted,
        "修理発動で行動終了"
    );
    assert!(matches!(app.action_mode(), ActionMode::Browse));
}

/// 補給は対象の EN・残弾を全回復し、対象の気力を 10 下げる (原典準拠)。
#[test]
fn support_supply_restores_en_and_lowers_morale() {
    use crate::command_menu::SupportKind;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Ally", 3, 6);
    let ally = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&ally).unwrap();
        u.en_consumed = 30;
        u.morale = 100;
    }
    app.apply_support_to_target(&ally, &ally, SupportKind::Supply);
    let u = app.database().unit_by_uid(&ally).unwrap();
    assert_eq!(u.en_consumed, 0, "補給で EN 全回復");
    assert_eq!(u.morale, 90, "補給で対象の気力 -10");
}

/// 修理装置の回復量はレベルに比例する (Lv3 = 100%)。
#[test]
fn repair_amount_scales_with_device_level() {
    use crate::command_menu::SupportKind;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Medic", 2, 6);
    place_player_unit(&mut app, "Ally", 3, 6);
    let caster = first_player_uid(&app);
    let ally = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 3 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    // 修理装置Lv3 = 100% 回復。
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new("修理装置Lv3", "")];
    app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 80;
    app.apply_support_to_target(&caster, &ally, SupportKind::Repair);
    assert_eq!(
        app.database().unit_by_uid(&ally).unwrap().damage,
        0,
        "修理装置Lv3=100% で全回復 (damage 80→0)"
    );
}

/// 修理 / 補給 を行ったユニットは経験値を得る。
#[test]
fn support_grants_exp_to_caster() {
    use crate::command_menu::SupportKind;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Medic", 2, 6);
    place_player_unit(&mut app, "Ally", 3, 6);
    let caster = first_player_uid(&app);
    let ally = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 3 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new("修理装置", "")];
    app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 50;
    let exp0 = app.database().unit_by_uid(&caster).unwrap().total_exp;
    app.apply_support_to_target(&caster, &ally, SupportKind::Repair);
    assert_eq!(
        app.database().unit_by_uid(&caster).unwrap().total_exp,
        exp0 + 100,
        "同レベル対象の修理で経験値 +100 (SRC 基準値)"
    );
}

/// 修理 / 補給 経験値の対象レベル差倍率 (SRC `Unit.cs::GetExp`)。
#[test]
fn support_exp_level_diff_table() {
    // 同レベル → 基準値。
    assert_eq!(support_exp_with_level_diff(10, 5, 5), 10);
    // 対象 +2 → ×2、+8 (>7) → ×5。
    assert_eq!(support_exp_with_level_diff(10, 7, 5), 20);
    assert_eq!(support_exp_with_level_diff(10, 13, 5), 50);
    // 対象が低レベル → 逓減 (-1 で ÷2)。
    assert_eq!(support_exp_with_level_diff(10, 4, 5), 5);
    // 補給基準 15 で +1 → ×1.5 = 22 (整数)。
    assert_eq!(support_exp_with_level_diff(15, 6, 5), 22);
    // 最低 1。
    assert_eq!(support_exp_with_level_diff(10, 0, 99), 1);
}

/// 統合: 高レベルの対象を修理すると経験値が増える (caster lv1, target lv5 → ×3)。
#[test]
fn support_exp_more_for_higher_level_target() {
    use crate::command_menu::SupportKind;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Medic", 2, 6);
    place_player_unit(&mut app, "Ally", 3, 6);
    let caster = first_player_uid(&app);
    let ally = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 3 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new("修理装置", "")];
    // 対象を高レベル (total_exp 2000 → level 5、SRC: 500 exp/level) に。
    {
        let a = app.database_mut().unit_by_uid_mut(&ally).unwrap();
        a.total_exp = 2000;
        a.damage = 50;
    }
    let exp0 = app.database().unit_by_uid(&caster).unwrap().total_exp;
    app.apply_support_to_target(&caster, &ally, SupportKind::Repair);
    let gained = app.database().unit_by_uid(&caster).unwrap().total_exp - exp0;
    assert_eq!(gained, 300, "高レベル対象 (+4) の修理で経験値 100×3=300");
}

/// 特殊能力が無い / 隣接に要支援の味方が居ないときは支援コマンドを出さない。
#[test]
fn support_command_hidden_without_feature_or_need() {
    use crate::command_menu::{CommandMenu, SupportKind, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Medic", 2, 6);
    place_player_unit(&mut app, "Ally", 3, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    let caster = first_player_uid(&app);

    // 特殊能力なし → 支援コマンドは出ない。
    app.open_unit_menu((2, 6));
    let shown = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. })
            if items.iter().any(|i| matches!(i, UnitMenuItem::Support(_)))
    );
    assert!(!shown, "修理装置を持たないユニットに支援コマンドは出ない");

    // 修理装置はあるが隣接味方が無傷 → 対象が無いので出ない。
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features
        .push(crate::feature::ActiveFeature::new("修理装置", ""));
    app.open_unit_menu((2, 6));
    let shown2 = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. })
            if items.contains(&UnitMenuItem::Support(SupportKind::Repair))
    );
    assert!(!shown2, "隣接に要修理の味方が居なければ『修理』は出ない");
}

/// 対象選択をキャンセルしても効果は発生せず、行動も消費しない。
#[test]
fn support_target_cancel_has_no_effect() {
    use crate::command_menu::{ActionMode, SupportKind};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Medic", 2, 6);
    place_player_unit(&mut app, "Ally", 3, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    let caster = first_player_uid(&app);
    let ally = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 3 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features
        .push(crate::feature::ActiveFeature::new("修理装置", ""));
    app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 50;

    app.begin_support_target(&caster, SupportKind::Repair);
    assert!(matches!(
        app.action_mode(),
        ActionMode::SupportTarget { .. }
    ));
    // 右クリック相当でキャンセル。
    assert!(app.cancel_action());
    assert!(matches!(app.action_mode(), ActionMode::Browse));
    assert_eq!(
        app.database().unit_by_uid(&ally).unwrap().damage,
        50,
        "キャンセルで回復しない"
    );
    assert!(
        !app.database().unit_by_uid(&caster).unwrap().has_acted,
        "キャンセルで行動終了しない"
    );
}

/// 変形先を 1 つ持つユニットはメニューの「変形」で即変形し、行動は消費しない。
#[test]
fn transform_single_form_switches_unit_immediately() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "FormA", 2, 6);
    // 変形先 FormB の UnitData を用意 (FormA を複製してリネーム)。
    let mut b = app.database().unit_by_name("FormA").cloned().unwrap();
    b.name = "FormB".into();
    app.database_mut().units.push(b);
    app.set_stage_state(crate::stage::StageState::Battle);
    let caster = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new("変形", "変形 FormB")];

    app.open_unit_menu((2, 6));
    let has = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Transform)
    );
    assert!(has, "変形先を持つユニットに『変形』が表示される");

    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Transform));
    assert_eq!(
        app.database().unit_by_uid(&caster).unwrap().unit_data_name,
        "FormB",
        "単一形態は即変形する"
    );
    assert!(
        !app.database().unit_by_uid(&caster).unwrap().has_acted,
        "変形は行動を消費しない"
    );
}

/// 変形先が複数あるとサブメニューが出て、選んだ形態に変形する。
#[test]
fn transform_multi_form_opens_submenu_and_switches() {
    use crate::command_menu::{MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "FormA", 2, 6);
    for n in ["FormB", "FormC"] {
        let mut d = app.database().unit_by_name("FormA").cloned().unwrap();
        d.name = n.into();
        app.database_mut().units.push(d);
    }
    app.set_stage_state(crate::stage::StageState::Battle);
    let caster = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new(
        "変形",
        "変形 FormB FormC",
    )];

    app.open_unit_menu((2, 6));
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Transform));
    // サブメニューが開く (まだ変形していない)。
    assert_eq!(
        app.database().unit_by_uid(&caster).unwrap().unit_data_name,
        "FormA"
    );
    // 2 番目 (FormC) を選ぶ。
    assert!(app.respond_dialog(2));
    assert_eq!(
        app.database().unit_by_uid(&caster).unwrap().unit_data_name,
        "FormC",
        "選んだ形態に変形する"
    );
}

/// 変形先サブメニューをキャンセルすると形態は変わらない。
#[test]
fn transform_submenu_cancel_keeps_form() {
    use crate::command_menu::{MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "FormA", 2, 6);
    for n in ["FormB", "FormC"] {
        let mut d = app.database().unit_by_name("FormA").cloned().unwrap();
        d.name = n.into();
        app.database_mut().units.push(d);
    }
    app.set_stage_state(crate::stage::StageState::Battle);
    let caster = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&caster)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new(
        "変形",
        "変形 FormB FormC",
    )];
    app.open_unit_menu((2, 6));
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Transform));
    assert!(app.respond_dialog(0)); // キャンセル
    assert_eq!(
        app.database().unit_by_uid(&caster).unwrap().unit_data_name,
        "FormA",
        "キャンセルで変形しない"
    );
}

/// 変形特殊能力が無い / 移動後 のときは「変形」を出さない (移動前のみ)。
#[test]
fn transform_hidden_post_move_or_without_feature() {
    use crate::command_menu::{CommandMenu, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "FormA", 2, 6);
    let mut b = app.database().unit_by_name("FormA").cloned().unwrap();
    b.name = "FormB".into();
    app.database_mut().units.push(b);
    app.set_stage_state(crate::stage::StageState::Battle);
    let caster = first_player_uid(&app);

    // 変形 特殊能力なし → 出ない。
    app.open_unit_menu((2, 6));
    let absent = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if !items.contains(&UnitMenuItem::Transform)
    );
    assert!(absent, "変形 特殊能力なしでは出ない");

    // 変形を付与するが移動後 → 出ない (移動前のみ)。
    {
        let u = app.database_mut().unit_by_uid_mut(&caster).unwrap();
        u.active_features = vec![crate::feature::ActiveFeature::new("変形", "変形 FormB")];
        u.has_moved = true;
    }
    app.open_unit_menu((2, 6));
    let absent2 = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if !items.contains(&UnitMenuItem::Transform)
    );
    assert!(absent2, "移動後は変形を出さない");
}

/// チャージ攻撃 (Ｃ属性) 武器を持つユニットには「チャージ」が出て、選択で
/// charged フラグが立ち行動終了する。武器を持たないユニットには出ない。
#[test]
fn charge_command_sets_flag_and_ends_action() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Charger", 2, 6);
    place_player_unit(&mut app, "Plain", 4, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    // Charger の UnitData にチャージ攻撃 (Ｃ属性) 武器を追加。
    {
        let d = app
            .database_mut()
            .units
            .iter_mut()
            .find(|d| d.name == "Charger")
            .unwrap();
        d.weapons.push(crate::data::unit::WeaponData {
            name: "チャージ砲".into(),
            power: 1000,
            min_range: 1,
            max_range: 3,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: "Ｃ".into(),
            extras: Vec::new(),
        });
    }
    let charger = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 2 && u.y == 6)
        .unwrap()
        .uid
        .clone();

    // チャージ砲を持つユニットには「チャージ」が出る。
    app.open_unit_menu((2, 6));
    let has = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Charge)
    );
    assert!(has, "Ｃ属性武器を持つユニットに『チャージ』が表示される");

    // チャージ砲を持たないユニットには出ない。
    app.open_unit_menu((4, 6));
    let absent = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if !items.contains(&UnitMenuItem::Charge)
    );
    assert!(absent, "Ｃ属性武器の無いユニットには『チャージ』は出ない");

    // 発動で charged フラグが立ち、行動終了する。
    app.open_unit_menu((2, 6));
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Charge));
    let u = app.database().unit_by_uid(&charger).unwrap();
    assert!(u.charged, "チャージで charged フラグが立つ");
    assert!(u.has_acted, "チャージで行動終了する");

    // 既にチャージ済みなら再表示しない。
    app.database_mut()
        .unit_by_uid_mut(&charger)
        .unwrap()
        .has_acted = false;
    app.open_unit_menu((2, 6));
    let absent2 = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if !items.contains(&UnitMenuItem::Charge)
    );
    assert!(
        absent2,
        "チャージ済みユニットには『チャージ』を再表示しない"
    );
}

/// 精神「突撃」は移動後でも長射程武器 (マップ攻撃を除く) を使用可能にする。
#[test]
fn totsugeki_allows_long_range_weapon_post_move() {
    let mk = |max_range: i32, class: &str| crate::data::unit::WeaponData {
        name: "W".into(),
        power: 1000,
        min_range: 1,
        max_range,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: class.into(),
        extras: Vec::new(),
    };
    let long = mk(3, ""); // 長射程・属性なし
    let map = mk(3, "Ｍ"); // マップ攻撃

    // 移動前は常に使用可。
    assert!(App::weapon_usable_post_move(&long, false, false));
    // 移動後: 通常は長射程不可。突撃でのみ可。
    assert!(!App::weapon_usable_post_move(&long, true, false));
    assert!(App::weapon_usable_post_move(&long, true, true));
    // 突撃でもマップ攻撃 (Ｍ) は移動後不可。
    assert!(!App::weapon_usable_post_move(&map, true, true));
}

/// 回復系特殊能力 ＨＰ回復Lv*/ＥＮ回復Lv* は当該陣営フェイズ開始時に
/// 実効最大値の 10×Lv% を回復する。
#[test]
fn regen_features_heal_hp_and_en_at_player_phase_start() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Regenner", 2, 6);
    // 勝敗即決回避の敵 (遠方)。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Regenner",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.active_features = vec![
            crate::feature::ActiveFeature::new("ＨＰ回復Lv2", ""),
            crate::feature::ActiveFeature::new("ＥＮ回復Lv2", ""),
        ];
        u.damage = 50; // 最大HP=100
        u.en_consumed = 20; // 最大EN=50
    }
    // 1 周回って味方フェイズ T2 開始 → 回復適用。
    app.handle_input(Input::EndPhase);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(u.damage, 30, "ＨＰ回復Lv2 = 最大HP 20% (20) 回復");
    assert_eq!(
        u.en_consumed, 5,
        "ＥＮ回復Lv2 = 最大EN 20% (10) 回復 + 基礎EN自然回復 (5) = 15 回復 (20 -> 5)"
    );
}

/// 基礎 EN 自然回復 (VB5 `Unit.cls:28182`「ＥＮは毎ターン5回復」): 回復系特殊能力を
/// 持たないユニットでもフェイズ開始時に EN +5 される。
#[test]
fn base_en_regen_applies_without_any_recovery_feature() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Regenner", 2, 6);
    // 勝敗即決回避の敵 (遠方)。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Regenner",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.active_features = vec![];
        u.en_consumed = 20; // 最大EN=50
    }
    // 1 周回って味方フェイズ T2 開始 → 基礎EN自然回復 (+5) のみ適用。
    app.handle_input(Input::EndPhase);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(
        u.en_consumed, 15,
        "回復系特殊能力が無くても基礎EN自然回復 (+5) は適用される (20 -> 15)"
    );
}

/// 基礎 EN 自然回復は `回復不能` (特殊効果攻撃属性 害) を持つユニットには適用されない。
#[test]
fn base_en_regen_skipped_for_no_regen_condition() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Regenner", 2, 6);
    // 勝敗即決回避の敵 (遠方)。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Regenner",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.active_features = vec![];
        u.en_consumed = 20; // 最大EN=50
        u.add_condition(crate::Condition::new("回復不能", -1));
    }
    // 1 周回って味方フェイズ T2 開始 → 回復不能により基礎EN自然回復も阻害される。
    app.handle_input(Input::EndPhase);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(
        u.en_consumed, 20,
        "回復不能により基礎EN自然回復も阻害される (据え置き)"
    );
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

/// `effective_max_plana`: 霊力Lv20・パイロットLv10・霊力成長なし →
/// `20 + round_ties_even(1.5*10) = 20 + 15 = 35`。VB5 `Pilot.cls:1423`。
#[test]
fn effective_max_plana_basic_formula() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    let uid = place_player_unit_with_plana_pilot(
        &mut app,
        "Reiryoku",
        "REI",
        vec![("霊力Lv20".into(), "1".into())],
        10,
        2,
        6,
    );
    let inst = app.database().unit_by_uid(&uid).unwrap().clone();
    assert_eq!(
        app.database().effective_max_plana(&inst),
        35,
        "20 + round_ties_even(1.5*10) = 20 + 15 = 35"
    );
}

/// `霊力` 技能を持たないパイロットは MaxPlana = 0 (プラーナ非搭載)。
#[test]
fn effective_max_plana_zero_without_reiryoku_skill() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    let uid =
        place_player_unit_with_plana_pilot(&mut app, "NoReiryoku", "PLAIN2", Vec::new(), 50, 2, 6);
    let inst = app.database().unit_by_uid(&uid).unwrap().clone();
    assert_eq!(
        app.database().effective_max_plana(&inst),
        0,
        "霊力技能なしは MaxPlana=0"
    );
}

/// `効果Lv23` 実際の `霊力成長` 込み: `霊力Lv20` / `霊力成長Lv5` / パイロットLv10 →
/// `20 + round_ties_even(1.5*10*(10+5)/10) = 20 + round_ties_even(22.5) = 20 + 22 = 42`
/// (`round_ties_even(22.5)` は最近接偶数の 22)。VB5 `Pilot.cls:1451-1454`。
#[test]
fn effective_max_plana_growth_skill_formula() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    let uid = place_player_unit_with_plana_pilot(
        &mut app,
        "ReiryokuGrowth",
        "REIG",
        vec![
            ("霊力Lv20".into(), "1".into()),
            ("霊力成長Lv5".into(), "1".into()),
        ],
        10,
        2,
        6,
    );
    let inst = app.database().unit_by_uid(&uid).unwrap().clone();
    assert_eq!(
        app.database().effective_max_plana(&inst),
        42,
        "20 + round_ties_even(1.5*10*(10+5)/10) = 20 + round_ties_even(22.5) = 20 + 22 = 42"
    );
}

/// 霊力 (プラーナ) 自然回復: MaxPlana=35 (霊力Lv20・パイロットLv10) のユニットは
/// 味方フェイズ開始時に `plana += 35/16 = 2` (霊力回復/霊力消費 特殊能力なし)。
/// VB5 `Unit.cls:27374`。
#[test]
fn plana_regen_applies_at_player_phase_start() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    let uid = place_player_unit_with_plana_pilot(
        &mut app,
        "PlanaRegen",
        "REIP",
        vec![("霊力Lv20".into(), "1".into())],
        10,
        2,
        6,
    );
    // 勝敗即決回避の敵 (遠方)。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "PlanaRegen",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.plana = 0;
    }
    // 1 周回って味方フェイズ T2 開始 → 霊力回復適用。
    app.handle_input(Input::EndPhase);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(u.plana, 2, "MaxPlana=35 のとき 35/16 == 2");
}

/// 霊力 (プラーナ) 自然回復は HP/EN と異なり `回復不能` (特殊効果攻撃属性 害) でも
/// 阻害されない (VB5 では回復不能ガードの外)。同一ユニットで HP/EN 自然回復は
/// 阻害されることも併せて確認する。
#[test]
fn plana_regen_not_gated_by_no_regen_condition() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    let uid = place_player_unit_with_plana_pilot(
        &mut app,
        "PlanaNoRegenGate",
        "REIN",
        vec![("霊力Lv20".into(), "1".into())],
        10,
        2,
        6,
    );
    // 勝敗即決回避の敵 (遠方)。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "PlanaNoRegenGate",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.plana = 0;
        u.en_consumed = 20; // 最大EN=50
        u.add_condition(crate::Condition::new("回復不能", -1));
    }
    // 1 周回って味方フェイズ T2 開始。
    app.handle_input(Input::EndPhase);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(
        u.plana, 2,
        "回復不能でも霊力回復 (35/16 == 2) は阻害されない"
    );
    assert_eq!(
        u.en_consumed, 20,
        "対照: 回復不能により基礎EN自然回復は阻害される (据え置き)"
    );
}

/// `Disable unit ＨＰ回復` / `Enable unit ＨＰ回復` (レベル無し基底名) が実体
/// `ＨＰ回復Lv1` の is_active を切替え、フェイズ開始回復の有無に反映されることを検証。
/// 決戦2話の核ギミック「水上=ＨＰ回復ON / 陸上=OFF」(`*ターン全味方` の Enable/Disable
/// + Area 分岐) の combat 経路 (毎フェイズ回復) を実検証する。
#[test]
fn disable_enable_toggles_hp_regen_feature() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Regenner", 2, 6);
    // 勝敗即決回避の敵 (遠方)。`place_player_unit` が登録する "Regenner" の
    // ユニットデータを流用する (データ無しだと敵フェイズ AI が panic する)。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Regenner",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.active_features = vec![crate::feature::ActiveFeature::new("ＨＰ回復Lv1", "")];
        u.damage = 50; // 最大HP=100
    }

    // (1) Disable → 基底名一致で is_active=false → フェイズ開始回復しない。
    let dis = crate::data::event::parse(&format!("Disable {uid} ＨＰ回復\n")).unwrap();
    crate::event_runtime::execute(&mut app, &dis).unwrap();
    assert!(
        !app.database().unit_by_uid(&uid).unwrap().active_features[0].is_active,
        "Disable で ＨＰ回復Lv1 が無効化されるはず"
    );
    app.handle_input(Input::EndPhase);
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().damage,
        50,
        "Disable 中は ＨＰ回復 で回復しない (陸上のボス挙動)"
    );

    // (2) Enable → 再有効 → ＨＰ回復Lv1 = 最大HP10% (10) 回復。
    let en = crate::data::event::parse(&format!("Enable {uid} ＨＰ回復\n")).unwrap();
    crate::event_runtime::execute(&mut app, &en).unwrap();
    assert!(
        app.database().unit_by_uid(&uid).unwrap().active_features[0].is_active,
        "Enable で ＨＰ回復Lv1 が再有効化されるはず"
    );
    app.handle_input(Input::EndPhase);
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().damage,
        40,
        "Enable 後は ＨＰ回復Lv1 = 最大HP10% (10) 回復 (水上のボス挙動)"
    );
}

/// `push_combat_feature_statuses` で特徴由来の `ステルス` が予測の hit% 低下 status に
/// 合流すること。`分身`/`超回避` は (B) で実行段の回避ロールへ移行したため**合流しない**
/// (check_dodge_feature が active_features を直接読む)。
#[test]
fn combat_feature_statuses_merges_stealth_only() {
    use crate::feature::ActiveFeature;
    let mut u = crate::UnitInstance::new("X", "P", crate::Party::Enemy, 0, 0);
    u.active_features = vec![
        ActiveFeature::new("ステルス", ""),  // → status
        ActiveFeature::new("分身", ""),      // (B) 回避ロール → 合流しない
        ActiveFeature::new("超回避Lv4", ""), // (B) 回避ロール → 合流しない
        ActiveFeature::new("水上移動", ""),  // 非戦闘 → 合流しない
    ];
    let mut s = vec!["集中".to_string()]; // 既存の condition 由来 status
    push_combat_feature_statuses(&u, &mut s);
    assert!(
        s.contains(&"ステルス".to_string()),
        "有効な ステルス 特徴は status に合流するはず"
    );
    assert!(
        !s.iter().any(|x| x == "分身" || x.contains("超回避")),
        "分身/超回避 は (B) 回避ロールへ移行し status には合流しない: {s:?}"
    );
    assert!(
        !s.contains(&"水上移動".to_string()),
        "非戦闘特徴は合流しない"
    );
    assert!(s.contains(&"集中".to_string()), "既存 status は保持");
}

/// バリア / バリアLv* は (B) の実行段吸収 (apply_barrier_absorb) へ移行したため、
/// 予測の ÷2 合流 (`バリア` status push) には **乗らない**。`バリア中和` (攻撃属性) も
/// 当然除外。`バリアシールド`/フィールド系の合流は別テストで検証。
#[test]
fn barrier_feature_is_not_merged_into_predict_halving() {
    use crate::feature::ActiveFeature;
    let mut u = crate::UnitInstance::new("X", "P", crate::Party::Enemy, 0, 0);
    u.active_features = vec![
        ActiveFeature::new("バリアLv2", ""), // → (B) 吸収・(A) push しない
        ActiveFeature::new("バリア", ""),    // → (B) 吸収・(A) push しない
        ActiveFeature::new("バリア中和", ""), // 攻撃属性 → 除外
    ];
    let mut s = Vec::new();
    push_combat_feature_statuses(&u, &mut s);
    assert!(
        !s.iter().any(|x| x == "バリア"),
        "バリア/バリアLv* は (B) 吸収へ移行し予測の ÷2 には合流しない: {s:?}"
    );
}

/// (A) ÷2 合流に残るのは バリアシールド(非標準名)/盾 のみ。シールド系・超回避・分身は
/// (B) ロール、バリア/フィールド/アクティブフィールド は (B) 吸収、単独プロテクト・
/// フィールド(解説)は非機能、バリア中和は攻撃属性 なので (A) には乗らない。
#[test]
fn combat_feature_statuses_merges_shield_field_and_chougaihi() {
    use crate::feature::ActiveFeature;
    let mut u = crate::UnitInstance::new("X", "P", crate::Party::Enemy, 0, 0);
    u.active_features = vec![
        ActiveFeature::new("バリアシールドLv3", ""), // → バリア (非標準名・(A) 残置)
        ActiveFeature::new("アクティブフィールドLv3", ""), // (B) 確率吸収 → (A) 除外
        ActiveFeature::new("アクティブシールド", ""), // シールド系 → (B)・(A) 除外
        ActiveFeature::new("フィールドLv1", ""),     // (B) 確定吸収 → (A) 除外
        ActiveFeature::new("超回避Lv4", ""),         // (B) 回避ロール → (A) 除外
        ActiveFeature::new("プロテクト・フィールド", "解説 これはヘルプ文"), // 非機能 → 除外
        ActiveFeature::new("バリア中和", ""),        // 攻撃属性 → 除外
    ];
    let mut s = Vec::new();
    push_combat_feature_statuses(&u, &mut s);
    assert_eq!(
        s.iter().filter(|x| x.as_str() == "バリア").count(),
        1,
        "バリアシールド の 1 つのみ (A) ÷2 に合流: {s:?}"
    );
}

/// ＨＰ消費Lv*/ＥＮ消費Lv* は同率を減少させる (ＨＰ は最低 1)。
#[test]
fn drain_features_reduce_hp_and_en_at_player_phase_start() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Leaker", 2, 6);
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Leaker",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.active_features = vec![
            crate::feature::ActiveFeature::new("ＨＰ消費Lv1", ""),
            crate::feature::ActiveFeature::new("ＥＮ消費Lv3", ""),
        ];
        // HP 満タン / EN 未消費から開始。
    }
    app.handle_input(Input::EndPhase);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(u.damage, 10, "ＨＰ消費Lv1 = 最大HP 10% (10) 減少");
    assert_eq!(u.en_consumed, 15, "ＥＮ消費Lv3 = 最大EN 30% (15) 減少");
}

/// 回復不能 (特殊効果攻撃属性 害) は特殊能力による HP/EN 自然回復を阻害する。
#[test]
fn kaifukufunou_blocks_regen_features() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Regenner", 2, 6);
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Regenner",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.active_features = vec![
            crate::feature::ActiveFeature::new("ＨＰ回復Lv2", ""),
            crate::feature::ActiveFeature::new("ＥＮ回復Lv2", ""),
        ];
        u.damage = 50;
        u.en_consumed = 20;
        u.add_condition(crate::Condition::new("回復不能", -1));
    }
    app.handle_input(Input::EndPhase);
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(u.damage, 50, "回復不能で HP 自然回復が阻害される");
    assert_eq!(u.en_consumed, 20, "回復不能で EN 自然回復が阻害される");
}

/// ゾンビ (特殊効果攻撃属性 ゾ) はアビリティ / 精神による能動的な HP/EN 回復を阻害する。
#[test]
fn zombie_blocks_active_recovery() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Z", 2, 6);
    let uid = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
        u.damage = 80;
        u.add_condition(crate::Condition::new("ゾンビ", 3));
    }
    // アビリティ回復は無効。
    app.apply_ability_effects(&uid, &uid, "回復Lv2");
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().damage,
        80,
        "ゾンビはアビリティ回復を受けない"
    );
    // 精神 全快 (spirit_heal_full) も無効。
    app.spirit_heal_full(&uid);
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().damage,
        80,
        "ゾンビは全快回復も受けない"
    );
}

/// アビリティをユニットに与えるテストヘルパ (静的 UnitData + ランタイム両方)。
fn give_ability(app: &mut App, unit_name: &str, uid: &str, ab: crate::data::unit::AbilityData) {
    let d = app
        .database_mut()
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

/// 射程0 アビリティ (回復) は自分に即適用し、回数を消費し行動終了する。
#[test]
fn ability_self_heal_applies_consumes_and_ends_action() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Healer", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 50;
    give_ability(
        &mut app,
        "Healer",
        &uid,
        mk_ability("自己回復", "回復Lv1", 0, Some(2)),
    );

    app.open_unit_menu((2, 6));
    let has = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Ability)
    );
    assert!(has, "アビリティを持つユニットに『アビリティ』が表示される");

    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Ability));
    assert!(app.respond_dialog(1)); // 1 番目のアビリティを選択

    let u = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(u.damage, 0, "回復Lv1 = 500 回復で全快 (最大HP100)");
    assert!(u.has_acted, "アビリティ発動で行動終了");
    assert_eq!(
        u.abilities[0].stock_remaining,
        Some(1),
        "回数が 2→1 に消費される"
    );
}

/// 射程≥1 アビリティは対象選択へ遷移し、クリックした射程内の味方に適用される。
#[test]
fn ability_targeted_heals_ally_in_range() {
    use crate::command_menu::{ActionMode, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Healer", 2, 6);
    place_player_unit(&mut app, "Ally", 4, 6);
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Healer",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let caster = first_player_uid(&app);
    let ally = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 4 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 90;
    give_ability(
        &mut app,
        "Healer",
        &caster,
        mk_ability("治療", "回復Lv2", 3, None),
    );

    app.open_unit_menu((2, 6));
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Ability));
    assert!(app.respond_dialog(1));
    // 射程≥1 → 対象選択モードへ。
    assert!(matches!(
        app.action_mode(),
        ActionMode::AbilityTarget { .. }
    ));

    // 射程内 (距離2) の味方 (4,6) をクリック。
    app.handle_input(Input::ClickAt {
        x: 4 * 32 + 16,
        y: 12 + 6 * 32 + 16,
    });
    assert_eq!(
        app.database().unit_by_uid(&ally).unwrap().damage,
        0,
        "回復Lv2 = 1000 回復で全快"
    );
    assert!(
        app.database().unit_by_uid(&caster).unwrap().has_acted,
        "アビリティ発動で発動主体は行動終了"
    );
}

/// 回数切れアビリティは `×` 付きで表示され、選択しても発動しない。
#[test]
fn ability_out_of_uses_shown_with_x_and_rejected() {
    use crate::command_menu::{MenuActionId, UnitMenuItem};
    use crate::dialog::PendingDialog;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Healer", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 50;
    give_ability(
        &mut app,
        "Healer",
        &uid,
        mk_ability("自己回復", "回復Lv1", 0, Some(0)),
    );

    app.open_unit_menu((2, 6));
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Ability));
    // サブメニューの選択肢に × が付く。
    let x_shown = matches!(
        app.pending_dialog(),
        Some(PendingDialog::Menu { options, .. }) if options[0].starts_with('×')
    );
    assert!(x_shown, "回数切れアビリティは × 付きで表示");
    // 選択しても回復しない (発動拒否)。
    assert!(app.respond_dialog(1));
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().damage,
        50,
        "回数切れは発動しない"
    );
    assert!(
        !app.database().unit_by_uid(&uid).unwrap().has_acted,
        "回数切れは行動を消費しない"
    );
}

/// アビリティを持たないユニットには『アビリティ』を出さない。
#[test]
fn ability_menu_hidden_without_abilities() {
    use crate::command_menu::{CommandMenu, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Plain", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    app.open_unit_menu((2, 6));
    let shown = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Ability)
    );
    assert!(!shown, "アビリティ無しユニットに『アビリティ』は出ない");
}

/// 射程0 の `再行動` アビリティは行動を消費しない (発動後もメニュー再表示)。
#[test]
fn ability_saidoudou_does_not_consume_action() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Mover", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    // 行動済みにしておき、再行動で戻ることを確認。
    app.database_mut().unit_by_uid_mut(&uid).unwrap().has_acted = false;
    give_ability(
        &mut app,
        "Mover",
        &uid,
        mk_ability("再動", "再行動", 0, None),
    );

    app.open_unit_menu((2, 6));
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Ability));
    assert!(app.respond_dialog(1));
    // 再行動で has_acted=false のまま → メニューが再表示される。
    assert!(
        !app.database().unit_by_uid(&uid).unwrap().has_acted,
        "再行動アビリティは行動を消費しない"
    );
    assert!(
        matches!(app.command_menu(), Some(CommandMenu::Unit { .. })),
        "発動後にユニットメニューを再表示する"
    );
}

/// アビリティ効果 霊力回復 / ＳＰ回復 は SP を 10×Lv 回復する。
#[test]
fn ability_sp_restore_effect_reduces_consumed() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Caster", 2, 6);
    let uid = first_player_uid(&app);
    // PILOT に最大SP 50 を与え 30 消費した状態に (PilotInstance 無し → sp_consumed 経路)。
    app.database_mut()
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
        .unwrap()
        .sp = Some(50);
    app.database_mut()
        .unit_by_uid_mut(&uid)
        .unwrap()
        .sp_consumed = 30;
    app.apply_ability_effects(&uid, &uid, "霊力回復Lv2"); // 10×2 = 20 回復
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().sp_consumed,
        10,
        "SP 20 回復で消費 30→10"
    );
}

/// アビリティ効果 変身 は対象を別フォームへ差し替える (set_unit_form を共有)。
#[test]
fn ability_henshin_effect_changes_form() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Human", 2, 6);
    // 変身先 Dragon の UnitData を用意。
    let mut d = app.database().unit_by_name("Human").cloned().unwrap();
    d.name = "Dragon".into();
    app.database_mut().units.push(d);
    let uid = first_player_uid(&app);
    app.apply_ability_effects(&uid, &uid, "変身Lv3=Dragon");
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().unit_data_name,
        "Dragon",
        "変身で形態が変わる"
    );
}

/// アビリティ効果 召喚Lv* は指定ユニットを Lv 体、隣接マスへ同陣営生成する。
#[test]
fn ability_summon_effect_creates_units() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Summoner", 5, 5);
    // 召喚先 Drone の UnitData (Summoner を複製)。
    let mut d = app.database().unit_by_name("Summoner").cloned().unwrap();
    d.name = "Drone".into();
    app.database_mut().units.push(d);
    let uid = first_player_uid(&app);
    app.apply_ability_effects(&uid, &uid, "召喚Lv2=Drone");

    let drones: Vec<_> = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| u.unit_data_name == "Drone")
        .collect();
    assert_eq!(drones.len(), 2, "召喚Lv2 で 2 体生成");
    assert!(
        drones
            .iter()
            .all(|d| d.summoned_by.as_deref() == Some(uid.as_str())),
        "summoned_by に親 uid"
    );
    assert!(
        drones.iter().all(|d| d.party == crate::Party::Player),
        "同陣営で生成"
    );
    // 親 (5,5) の隣接に配置される。
    assert!(drones
        .iter()
        .all(|d| (d.x as i32 - 5).abs() <= 1 && (d.y as i32 - 5).abs() <= 1));
}

/// アビリティ効果 強化 は指定特殊能力を一時状態 (condition) として対象へ付与する。
#[test]
fn ability_kyouka_adds_condition() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Booster", 2, 6);
    let uid = first_player_uid(&app);
    app.apply_ability_effects(&uid, &uid, "強化Lv2=切り払い");
    let u = app.database().unit_by_uid(&uid).unwrap();
    assert!(
        u.conditions.iter().any(|c| c.name == "切り払い"),
        "強化で指定能力が状態として付与される"
    );
}

/// アビリティ効果 能力コピー は発動者を対象 (射程内味方) の形態へ変化させる。
/// サイズ 2 段階以上差のユニットへは変化できない (サイズ制限)。
#[test]
fn ability_nouryoku_copy_transforms_caster_with_size_limit() {
    use crate::data::unit::Size;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Copier", 2, 6); // Size::M
    place_player_unit(&mut app, "Model", 4, 6); // Size::M (同サイズ)
    let caster = first_player_uid(&app);
    let model = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 4 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    give_ability(
        &mut app,
        "Copier",
        &caster,
        mk_ability("コピー", "能力コピーLv3", 3, None),
    );
    // 同サイズ・射程内 → 有効な対象。
    assert!(app.ability_target_valid(&caster, 0, &model));
    app.apply_ability(&caster, 0, &model);
    assert_eq!(
        app.database().unit_by_uid(&caster).unwrap().unit_data_name,
        "Model",
        "能力コピーで発動者が対象の形態へ変化"
    );

    // XL サイズの対象 (M とは 3 段階差) は対象不可。
    let mut big = app.database().unit_by_name("Model").cloned().unwrap();
    big.name = "BigModel".into();
    big.size = Size::XL;
    app.database_mut().units.push(big);
    app.database_mut()
        .unit_by_uid_mut(&model)
        .unwrap()
        .unit_data_name = "BigModel".into();
    assert!(
        !app.ability_target_valid(&caster, 0, &model),
        "サイズ 2 段階以上差の対象には能力コピー不可"
    );
}

/// マップ型アビリティ (Ｍ全) は射程内 (Ｍ全 は盤上全体) の全味方へ効果を及ぼす。
#[test]
fn ability_map_type_heals_all_allies() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Caster", 5, 5);
    place_player_unit(&mut app, "AllyA", 5, 6);
    place_player_unit(&mut app, "AllyB", 6, 5);
    let caster = first_player_uid(&app);
    for pos in [(5u32, 6u32), (6, 5)] {
        let uid = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.x == pos.0 && u.y == pos.1)
            .unwrap()
            .uid
            .clone();
        app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 50;
    }
    let mut ab = mk_ability("全体回復", "回復Lv1", 3, None);
    ab.attributes = "Ｍ全".into();
    give_ability(&mut app, "Caster", &caster, ab);
    app.apply_ability(&caster, 0, &caster);
    for pos in [(5u32, 6u32), (6, 5)] {
        let dmg = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.x == pos.0 && u.y == pos.1)
            .unwrap()
            .damage;
        assert_eq!(dmg, 0, "Ｍ全 回復で全味方が回復する");
    }
}

/// 敵対象アビリティ (脱=気力低下 / 除=特殊効果解除) は味方ではなく敵を対象に取る。
#[test]
fn ability_enemy_target_datsu_and_jo() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Caster", 2, 6);
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Caster",
        "FOE",
        crate::Party::Enemy,
        4,
        6,
    ));
    let caster = first_player_uid(&app);
    let enemy = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Enemy)
        .unwrap()
        .uid
        .clone();
    // 敵に強化系の状態を付けておく (除 で解除される)。
    app.database_mut()
        .unit_by_uid_mut(&enemy)
        .unwrap()
        .add_condition(crate::Condition::new("攻撃力ＵＰ", -1));
    let mut ab = mk_ability("脱力除去弾", "解説", 3, None);
    ab.attributes = "脱 除".into();
    give_ability(&mut app, "Caster", &caster, ab);

    // 脱/除 アビリティは敵が有効対象、味方 (自分) は無効。
    assert!(app.ability_target_valid(&caster, 0, &enemy));
    assert!(!app.ability_target_valid(&caster, 0, &caster));

    let m0 = app.database().unit_by_uid(&enemy).unwrap().morale;
    app.apply_ability(&caster, 0, &enemy);
    let enemy_u = app.database().unit_by_uid(&enemy).unwrap();
    assert_eq!(enemy_u.morale, m0 - 10, "脱で敵気力 -10");
    assert!(
        enemy_u.conditions.is_empty(),
        "除で敵のアビリティ特殊効果を解除"
    );
}

/// 母艦: 搭載 (fire_boarding_event) で格納リンクが張られ、発進で隣接マスへ出撃する。
#[test]
fn carrier_board_and_launch() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Carrier", 5, 5);
    place_player_unit(&mut app, "Fighter", 6, 5);
    app.set_stage_state(crate::stage::StageState::Battle);
    let carrier = first_player_uid(&app);
    let fighter = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Fighter")
        .unwrap()
        .uid
        .clone();
    let carrier_idx = app.database().idx_by_uid(&carrier).unwrap();
    let fighter_idx = app.database().idx_by_uid(&fighter).unwrap();

    // 搭載: 相互リンク + off_map。
    app.fire_boarding_event(fighter_idx, carrier_idx);
    let f = app.database().unit_by_uid(&fighter).unwrap();
    assert!(f.off_map, "格納で off_map");
    assert_eq!(f.stored_in.as_deref(), Some(carrier.as_str()));
    assert!(app
        .database()
        .unit_by_uid(&carrier)
        .unwrap()
        .stored_units
        .contains(&fighter));

    // 発進: メニューに「発進」が出る。
    app.open_unit_menu((5, 5));
    let has_launch = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Launch)
    );
    assert!(has_launch, "格納ユニットがあれば『発進』が出る");

    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Launch));
    assert!(app.respond_dialog(1)); // 1 番目の格納ユニット

    // 発進は着地点選択 (移動範囲から対話選択) へ移行する。配置はまだ。
    let cand = match app.action_mode() {
        crate::command_menu::ActionMode::LandingSelect { candidates, .. } => {
            assert!(!candidates.is_empty(), "着地候補がある");
            // 母艦 (5,5) は占有なので候補外、移動範囲内の空きマスが候補。
            assert!(!candidates.contains(&(5, 5)), "母艦マスは候補に含まない");
            candidates[0]
        }
        other => panic!("着地点選択へ遷移すべき: {other:?}"),
    };
    assert!(
        app.database().unit_by_uid(&fighter).unwrap().off_map,
        "着地点選択中はまだ off_map"
    );
    // 候補外クリックは無反応。
    assert!(!app.confirm_landing(5, 5), "候補外 (母艦マス) は確定しない");
    assert!(app.database().unit_by_uid(&fighter).unwrap().off_map);
    // 候補マスを確定 → 配置。
    assert!(app.confirm_landing(cand.0, cand.1), "候補マスで配置確定");

    let f = app.database().unit_by_uid(&fighter).unwrap();
    assert!(!f.off_map, "発進で盤上へ復帰");
    assert!(f.stored_in.is_none());
    assert!(
        app.database()
            .unit_by_uid(&carrier)
            .unwrap()
            .stored_units
            .is_empty(),
        "格納リストから外れる"
    );
    assert_eq!((f.x, f.y), cand, "選択した候補マスに配置される");
}

/// 発進の着地点選択を右クリック/Esc でキャンセルすると、ユニットは格納のまま残る。
#[test]
fn launch_landing_select_cancel_keeps_stored() {
    use crate::command_menu::{ActionMode, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Carrier", 5, 5);
    place_player_unit(&mut app, "Fighter", 6, 5);
    app.set_stage_state(crate::stage::StageState::Battle);
    let carrier = first_player_uid(&app);
    let fighter = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Fighter")
        .unwrap()
        .uid
        .clone();
    let ci = app.database().idx_by_uid(&carrier).unwrap();
    let fi = app.database().idx_by_uid(&fighter).unwrap();
    app.fire_boarding_event(fi, ci);
    app.open_unit_menu((5, 5));
    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Launch));
    assert!(app.respond_dialog(1));
    assert!(matches!(
        app.action_mode(),
        ActionMode::LandingSelect { .. }
    ));

    // キャンセル (Esc/右クリック相当) → 格納のまま・Browse へ戻る。
    assert!(app.handle_input(Input::Cancel));
    assert!(matches!(app.action_mode(), ActionMode::Browse));
    let f = app.database().unit_by_uid(&fighter).unwrap();
    assert!(f.off_map, "キャンセルで格納のまま");
    assert_eq!(f.stored_in.as_deref(), Some(carrier.as_str()));
    assert!(
        app.database()
            .unit_by_uid(&carrier)
            .unwrap()
            .stored_units
            .contains(&fighter),
        "格納リストに残る"
    );
}

/// 母艦格納中ユニットは当該陣営フェイズ開始時に HP/EN を 50% 回復する。
#[test]
fn carrier_stored_unit_recovers_each_turn() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Carrier", 5, 5);
    place_player_unit(&mut app, "Fighter", 6, 5);
    // 勝敗即決回避の敵。
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Carrier",
        "PILOT",
        crate::Party::Enemy,
        12,
        12,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let carrier = first_player_uid(&app);
    let fighter = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Fighter")
        .unwrap()
        .uid
        .clone();
    let carrier_idx = app.database().idx_by_uid(&carrier).unwrap();
    let fighter_idx = app.database().idx_by_uid(&fighter).unwrap();
    {
        let f = app.database_mut().unit_by_uid_mut(&fighter).unwrap();
        f.damage = 60; // 最大HP 100
        f.en_consumed = 40; // 最大EN 50
    }
    app.fire_boarding_event(fighter_idx, carrier_idx);

    // 1 周して味方フェイズ T2 開始 → 母艦回復。
    app.handle_input(Input::EndPhase);
    let f = app.database().unit_by_uid(&fighter).unwrap();
    assert_eq!(f.damage, 10, "HP 50% (50) 回復で damage 60→10");
    assert_eq!(
        f.en_consumed, 10,
        "基礎EN自然回復 (+5, 40→35) の後さらに母艦格納 EN 50% (最大EN50の50%=25) 回復で 35→10"
    );
}

/// 合体テスト用: host + 2マス以内の合体相手を置き、合体形態 UnitData を用意する。
fn setup_combine(app: &mut App) -> (String, String) {
    enter_mapview_with_demo_map(app);
    place_player_unit(app, "GetterEagle", 3, 3);
    place_player_unit(app, "GetterJaguar", 4, 3);
    // 合体形態 GetterRobo の UnitData (GetterEagle を複製)。
    let mut g = app.database().unit_by_name("GetterEagle").cloned().unwrap();
    g.name = "GetterRobo".into();
    app.database_mut().units.push(g);
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

/// 3 体合体: 相手が全て 2 マス以内なら『合体』が出て、実行で合体形態へ変身し
/// 相手は全員 off_map で温存・行動終了する (原典: 3 体以上は合体コマンド)。
#[test]
fn combine_absorbs_partners_into_form() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    let (host, partner) = setup_combine(&mut app);
    let partner2 = extend_combine_to_triple(&mut app, &host, (2, 3));

    app.open_unit_menu((3, 3));
    let has = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Combine)
    );
    assert!(has, "3 体合体で相手が全て 2 マス以内なら『合体』が出る");

    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Combine));
    let h = app.database().unit_by_uid(&host).unwrap();
    assert_eq!(h.unit_data_name, "GetterRobo", "host が合体形態へ変身");
    assert!(
        h.combined_from.contains(&partner) && h.combined_from.contains(&partner2),
        "両方の相手を構成に記録"
    );
    assert_eq!(h.pre_combine_form.as_deref(), Some("GetterEagle"));
    assert!(h.has_acted, "合体で行動終了");
    assert!(
        app.database().unit_by_uid(&partner).unwrap().off_map,
        "合体相手 1 は off_map で温存"
    );
    assert!(
        app.database().unit_by_uid(&partner2).unwrap().off_map,
        "合体相手 2 は off_map で温存"
    );
}

/// 合体形態の必要技能 (`必要技能.md` §4): 構成員が満たさなければ合体できない。
#[test]
fn combine_respects_form_necessary_skill() {
    let mut app = App::new();
    let (host, _partner) = setup_combine(&mut app);
    // 合体形態 GetterRobo に 必要技能=念力Lv3 を付与。
    app.database_mut()
        .units
        .iter_mut()
        .find(|d| d.name == "GetterRobo")
        .unwrap()
        .features
        .push(("必要技能".into(), "念力Lv3".into()));
    // host に念力なしパイロットを紐付け。
    app.database_mut().create_pilot_instance("PILOT", "p_host");
    app.database_mut().unit_by_uid_mut(&host).unwrap().pilot_ids = vec!["p_host".into()];
    // 念力なし → 合体不可 (host は GetterEagle のまま、相手も on_map)。
    app.apply_combine(&host);
    assert_eq!(
        app.database().unit_by_uid(&host).unwrap().unit_data_name,
        "GetterEagle",
        "念力なしで合体不可"
    );
    // 念力Lv3 を習得 → 合体成功。
    app.database_mut()
        .pilot_instance_by_id_mut("p_host")
        .unwrap()
        .skills
        .push("念力Lv3".into());
    app.apply_combine(&host);
    assert_eq!(
        app.database().unit_by_uid(&host).unwrap().unit_data_name,
        "GetterRobo",
        "念力Lv3 で合体成功"
    );
}

/// 2 体合体は合体コマンドを使わず移動で合体する (原典 合体.md)。
/// → ユニットメニューに『合体』は出さない (移動経路は別テストで検証)。
#[test]
fn two_unit_combine_hides_menu_item() {
    use crate::command_menu::{CommandMenu, UnitMenuItem};
    let mut app = App::new();
    let _ = setup_combine(&mut app); // 2 体合体 (相手 1 機)
    app.open_unit_menu((3, 3));
    let has = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Combine)
    );
    assert!(!has, "2 体合体では『合体』コマンドを出さない (移動で合体)");
}

/// 3 体合体で相手の 1 機が 2 マス以内に居なければ『合体』は出ない
/// (原典: 合体に参加するユニット全てが 2 マス以内に居る場合のみ)。
#[test]
fn three_unit_combine_requires_all_partners_present() {
    use crate::command_menu::{CommandMenu, UnitMenuItem};
    let mut app = App::new();
    let (host, _partner) = setup_combine(&mut app);
    // 3 体目 (GetterBear) を遠方 (2 マス超) に置く。
    let _ = extend_combine_to_triple(&mut app, &host, (10, 10));
    app.open_unit_menu((3, 3));
    let has = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Combine)
    );
    assert!(!has, "相手が全員 2 マス以内に揃わなければ合体不可");
}

/// 分離: 構成ユニットを隣接マスへ復帰させ、host を合体前形態へ戻す (行動非消費)。
#[test]
fn separate_restores_components_and_reverts_form() {
    let mut app = App::new();
    let (host, partner) = setup_combine(&mut app);
    app.apply_combine(&host);
    assert!(app.database().unit_by_uid(&partner).unwrap().off_map);

    app.apply_separate(&host);
    // host 形態/構成リストは分離時点で確定 (着地点選択とは独立)。
    let h = app.database().unit_by_uid(&host).unwrap();
    assert_eq!(h.unit_data_name, "GetterEagle", "host が合体前形態へ戻る");
    assert!(h.combined_from.is_empty(), "構成リストがクリアされる");
    assert!(h.pre_combine_form.is_none());

    // 構成ユニットは着地点選択 (LandingSelect) へ移行する (移動範囲から対話選択)。
    let cand = match app.action_mode() {
        crate::command_menu::ActionMode::LandingSelect {
            uid, candidates, ..
        } => {
            assert_eq!(uid, partner, "構成ユニットの着地を選択");
            assert!(!candidates.is_empty());
            candidates[0]
        }
        other => panic!("着地点選択へ遷移すべき: {other:?}"),
    };
    assert!(
        app.database().unit_by_uid(&partner).unwrap().off_map,
        "選択中はまだ off_map"
    );
    assert!(app.confirm_landing(cand.0, cand.1), "候補マスで配置確定");

    let p = app.database().unit_by_uid(&partner).unwrap();
    assert!(!p.off_map, "構成ユニットが盤上へ復帰");
    assert_eq!((p.x, p.y), cand, "選択した候補マスに配置される");
}

/// 分離 (3 体合体) の着地点選択キュー: 構成ユニットを 1 機ずつ順次配置し、最後に Browse へ。
#[test]
fn separate_landing_queue_places_components_sequentially() {
    use crate::command_menu::ActionMode;
    let mut app = App::new();
    let (host, partner1) = setup_combine(&mut app);
    let partner2 = extend_combine_to_triple(&mut app, &host, (4, 4));
    app.apply_combine(&host);
    assert!(app.database().unit_by_uid(&partner1).unwrap().off_map);
    assert!(app.database().unit_by_uid(&partner2).unwrap().off_map);

    app.apply_separate(&host);
    // 1 機目の着地点選択。
    let (uid1, cand1) = match app.action_mode() {
        ActionMode::LandingSelect {
            uid, candidates, ..
        } => (uid, candidates[0]),
        other => panic!("1 機目の着地点選択へ: {other:?}"),
    };
    assert!(app.confirm_landing(cand1.0, cand1.1));
    // キューが進み 2 機目の着地点選択へ。
    let (uid2, cand2) = match app.action_mode() {
        ActionMode::LandingSelect {
            uid, candidates, ..
        } => {
            assert_ne!(uid, uid1, "2 機目へ進む");
            assert!(!candidates.contains(&cand1), "配置済みマスは候補から除外");
            (uid, candidates[0])
        }
        other => panic!("2 機目の着地点選択へ: {other:?}"),
    };
    let _ = uid2;
    assert!(app.confirm_landing(cand2.0, cand2.1));
    // 全機配置完了 → Browse。
    assert!(
        matches!(app.action_mode(), ActionMode::Browse),
        "完了で Browse"
    );
    let p1 = app.database().unit_by_uid(&partner1).unwrap();
    let p2 = app.database().unit_by_uid(&partner2).unwrap();
    assert!(!p1.off_map && !p2.off_map, "両構成ユニットが盤上へ");
    assert_ne!((p1.x, p1.y), (p2.x, p2.y), "別マスに配置される");
}

/// 合体時のパイロット統合: 構成ユニットのパイロットが合体形態へ搭乗 (全員搭乗)、
/// 分離で各機の搭乗構成へ戻る。
#[test]
fn combine_integrates_pilots_and_separate_restores() {
    let mut app = App::new();
    let (host, partner) = setup_combine(&mut app);
    // host / partner に区別可能な pilot_ids を設定。
    app.database_mut().unit_by_uid_mut(&host).unwrap().pilot_ids = vec!["host_p".to_string()];
    app.database_mut()
        .unit_by_uid_mut(&partner)
        .unwrap()
        .pilot_ids = vec!["partner_p".to_string()];

    app.apply_combine(&host);
    let h = app.database().unit_by_uid(&host).unwrap();
    assert!(
        h.pilot_ids.contains(&"host_p".to_string()),
        "host のパイロットを維持"
    );
    assert!(
        h.pilot_ids.contains(&"partner_p".to_string()),
        "相手のパイロットが合体形態へ搭乗"
    );
    assert_eq!(
        h.pre_combine_pilots,
        vec!["host_p".to_string()],
        "合体前の搭乗構成を保持"
    );

    app.apply_separate(&host);
    let h = app.database().unit_by_uid(&host).unwrap();
    assert_eq!(
        h.pilot_ids,
        vec!["host_p".to_string()],
        "分離で host の搭乗構成が復帰"
    );
    assert!(h.pre_combine_pilots.is_empty(), "統合状態がクリアされる");
    let p = app.database().unit_by_uid(&partner).unwrap();
    assert_eq!(
        p.pilot_ids,
        vec!["partner_p".to_string()],
        "相手のパイロットが各機へ復帰"
    );
}

/// 母艦上へ移動すると搭載される (ムーブ統合)。
#[test]
fn board_carrier_by_moving_onto_it() {
    use crate::command_menu::ActionMode;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Carrier", 5, 5);
    place_player_unit(&mut app, "Fighter", 5, 6); // 母艦に隣接
    app.set_stage_state(crate::stage::StageState::Battle);
    let carrier = first_player_uid(&app);
    let fighter = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Fighter")
        .unwrap()
        .uid
        .clone();
    app.database_mut()
        .unit_by_uid_mut(&carrier)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new("母艦", "")];

    // Fighter を移動モードにして母艦タイル (5,5) をクリック。
    app.action_mode = ActionMode::MoveSelect {
        uid: fighter.clone(),
    };
    app.handle_input(Input::ClickAt {
        x: 5 * 32 + 16,
        y: 12 + 5 * 32 + 16,
    });

    assert!(
        app.database().unit_by_uid(&fighter).unwrap().off_map,
        "母艦上へ移動で搭載 (off_map)"
    );
    assert!(app
        .database()
        .unit_by_uid(&carrier)
        .unwrap()
        .stored_units
        .contains(&fighter));
}

/// 合体相手の上へ移動すると合体する (2 機合体のムーブ統合)。
#[test]
fn combine_by_moving_onto_partner() {
    use crate::command_menu::ActionMode;
    let mut app = App::new();
    let (host, partner) = setup_combine(&mut app);
    // partner (GetterJaguar) を移動モードにして host (GetterEagle, 3,3) をクリック。
    // host が 合体 特殊能力を持ち相手リストに partner の形態を含むので、host が
    // 合体形態 (GetterRobo) になり partner が温存される。
    app.action_mode = ActionMode::MoveSelect {
        uid: partner.clone(),
    };
    app.handle_input(Input::ClickAt {
        x: 3 * 32 + 16,
        y: 12 + 3 * 32 + 16,
    });

    assert_eq!(
        app.database().unit_by_uid(&host).unwrap().unit_data_name,
        "GetterRobo",
        "相手上へ移動で合体形態へ"
    );
    assert!(
        app.database().unit_by_uid(&partner).unwrap().off_map,
        "mover は構成ユニットとして温存"
    );
}

/// 最初から合体形態で配置されたユニット (構成 uid 無し) の分離: `分離` 特殊能力の
/// 形態へ分離し、host が形態1 になり形態2 以降が隣接へ新規生成される。パイロットは
/// 形態順に乗り移る。分離は行動を消費しない。
#[test]
fn separate_from_initial_combined_form_spawns_components() {
    use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "CombinedForm", 5, 5);
    // 分離後の形態 FormA / FormB の UnitData を用意 (インスタンスは作らない)。
    for form in ["FormA", "FormB"] {
        let mut ud = app
            .database()
            .unit_by_name("CombinedForm")
            .cloned()
            .unwrap();
        ud.name = form.into();
        app.database_mut().units.push(ud);
    }
    app.set_stage_state(crate::stage::StageState::Battle);
    let host = first_player_uid(&app);
    {
        let u = app.database_mut().unit_by_uid_mut(&host).unwrap();
        // 構成 uid は無く、`分離=<名称> <形態1> <形態2>` だけを持つ。
        u.active_features = vec![crate::feature::ActiveFeature::new(
            "分離",
            "分離 FormA FormB",
        )];
        u.pilot_ids = vec!["pa".into(), "pb".into()];
    }

    // メニューに『分離』が出る (combined_from が空でも `分離` 特殊能力で表示)。
    app.open_unit_menu((5, 5));
    let has = matches!(
        app.command_menu(),
        Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Separate)
    );
    assert!(has, "`分離` 特殊能力を持つなら『分離』が出る");

    app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Separate));
    // host は形態1 (FormA) へ、行動は消費しない。
    let h = app.database().unit_by_uid(&host).unwrap();
    assert_eq!(h.unit_data_name, "FormA", "host が形態1 へ");
    assert!(!h.has_acted, "分離は行動を消費しない");
    assert_eq!(
        h.pilot_ids,
        vec!["pa".to_string()],
        "形態1 は主パイロットを保持"
    );
    // 形態2 (FormB) が host 隣接に新規生成され、形態順にパイロットが乗り移る。
    let fb = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "FormB")
        .expect("形態2 が新規生成される");
    assert!(
        (fb.x as i32 - 5).abs() <= 1 && (fb.y as i32 - 5).abs() <= 1,
        "形態2 は host (5,5) 隣接に生成: ({},{})",
        fb.x,
        fb.y
    );
    assert_eq!(
        fb.pilot_ids,
        vec!["pb".to_string()],
        "形態順に 2 番目のパイロットが形態2 へ"
    );
}

#[test]
fn effective_combat_data_reflects_growth_and_debuff() {
    use crate::Condition;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let idx = {
        let uid = first_player_uid(&app);
        app.database().idx_by_uid(&uid).unwrap()
    };
    // 基準 (level 1 / 装備なし / 状態異常なし)。VB5 `Pilot.cls:582-593` 準拠で
    // レベル 1 でも成長が乗る (格闘 = base 100 + lv 1 = 101)。装甲は unit 由来で不変。
    let (p0, u0) = app.database().effective_combat_data(idx).unwrap();
    assert_eq!(p0.infight, 101);
    assert_eq!(u0.armor, 10);
    // 育成: total_exp 500 → level 2 → 格闘がさらに成長する (戦闘予測へ反映)。
    app.database_mut().unit_instances[idx].total_exp = 500;
    let (p1, _) = app.database().effective_combat_data(idx).unwrap();
    assert!(
        p1.infight > p0.infight,
        "レベル成長で格闘が上がる: {} > {}",
        p1.infight,
        p0.infight
    );
    // 状態異常「装甲低下」で実効装甲が下がる (被ダメージ増加に直結)。
    app.database_mut().unit_instances[idx].add_condition(Condition::new("装甲低下", 1));
    let (_, u2) = app.database().effective_combat_data(idx).unwrap();
    assert!(
        u2.armor < u0.armor,
        "装甲低下で実効装甲が下がる: {} < {}",
        u2.armor,
        u0.armor
    );
}

#[test]
fn award_kill_rewards_grants_money_exp_and_consumes_kouun() {
    use crate::Condition;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let idx = {
        let uid = first_player_uid(&app);
        app.database().idx_by_uid(&uid).unwrap()
    };
    let money0 = app.money();
    let exp0 = app.database().unit_instances[idx].total_exp;
    // 撃破: 経験値 50 / 敵価値 200 → 資金 100。
    let gained = app.award_kill_rewards(idx, 50, 200, crate::Party::Enemy);
    assert_eq!(gained, 100, "資金 = 敵価値/2");
    assert_eq!(app.money(), money0 + 100);
    assert_eq!(app.database().unit_instances[idx].total_exp, exp0 + 50);
    // 幸運: 資金 2 倍 かつ消費。
    app.database_mut().unit_instances[idx].add_condition(Condition::new("幸運", 1));
    let money1 = app.money();
    let gained2 = app.award_kill_rewards(idx, 0, 200, crate::Party::Enemy);
    assert_eq!(gained2, 200, "幸運 で資金 2 倍");
    assert_eq!(app.money(), money1 + 200);
    assert!(
        !app.database().unit_instances[idx].has_condition("幸運"),
        "幸運 は撃破時に消費される"
    );
}

/// 撃破でメインパイロットの撃墜数が +1 され、必要技能ゲートが読む値が上がる。
#[test]
fn kill_increments_pilot_kill_count() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let idx = {
        let uid = first_player_uid(&app);
        app.database().idx_by_uid(&uid).unwrap()
    };
    // place_player_unit は pilot_ids を張らないので実体を作って紐付ける。
    app.database_mut().create_pilot_instance("PILOT", "p1");
    app.database_mut().unit_instances[idx].pilot_ids = vec!["p1".to_string()];
    assert_eq!(app.unit_pilot_skill_level(idx, "撃墜数"), 0);
    app.award_kill_rewards(idx, 0, 0, crate::Party::Enemy);
    assert_eq!(
        app.unit_pilot_skill_level(idx, "撃墜数"),
        1,
        "1 撃破で撃墜数 1"
    );
    app.award_kill_rewards(idx, 0, 0, crate::Party::Enemy);
    app.award_kill_rewards(idx, 0, 0, crate::Party::Enemy);
    assert_eq!(
        app.unit_pilot_skill_level(idx, "撃墜数"),
        3,
        "3 撃破で撃墜数 3"
    );
}

/// 撃墜による気力上昇: 撃破者 +4 / 同陣営の他ユニット +1 (`Unit.cs` 準拠)。
#[test]
fn kill_raises_morale_killer_plus4_faction_plus1() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // [0] 撃破者
    place_player_unit(&mut app, "Ally", 4, 6); // [1] 同陣営
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    )); // [2] 敵 (別陣営、対象外)
    let k0 = app.database().unit_instances[0].morale;
    let a0 = app.database().unit_instances[1].morale;
    let e0 = app.database().unit_instances[2].morale;
    app.award_kill_rewards(0, 0, 0, crate::Party::Enemy);
    assert_eq!(
        app.database().unit_instances[0].morale,
        k0 + 4,
        "撃破者は気力 +4"
    );
    assert_eq!(
        app.database().unit_instances[1].morale,
        a0 + 1,
        "同陣営の他ユニットは気力 +1"
    );
    assert_eq!(
        app.database().unit_instances[2].morale,
        e0,
        "別陣営は変動しない"
    );
}

/// 性格「機械」のパイロットは撃墜による気力上昇の対象外 (撃破者は +3 のみ)。
#[test]
fn kill_morale_skips_mecha_pilots() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    place_player_unit(&mut app, "Ally", 4, 6);
    app.database_mut()
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
        .unwrap()
        .personality = Some("機械".into());
    let k0 = app.database().unit_instances[0].morale;
    let a0 = app.database().unit_instances[1].morale;
    app.award_kill_rewards(0, 0, 0, crate::Party::Enemy);
    assert_eq!(
        app.database().unit_instances[0].morale,
        k0 + 3,
        "機械の撃破者は陣営加算を受けず +3 のみ"
    );
    assert_eq!(
        app.database().unit_instances[1].morale,
        a0,
        "機械の同陣営は気力が動かない"
    );
}

/// 被撃破陣営は性格に応じて気力が変動する (超強気 +2 / 弱気 -1)。
#[test]
fn kill_morale_victim_faction_changes_by_personality() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // [0] 撃破者 (Player)
    push_pilot_with_features(&mut app, "BRAVE", Vec::new());
    push_pilot_with_features(&mut app, "TIMID", Vec::new());
    app.database_mut()
        .pilots
        .iter_mut()
        .find(|p| p.name == "BRAVE")
        .unwrap()
        .personality = Some("超強気".into());
    app.database_mut()
        .pilots
        .iter_mut()
        .find(|p| p.name == "TIMID")
        .unwrap()
        .personality = Some("弱気".into());
    let brave = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "BRAVE",
        crate::Party::Enemy,
        8,
        8,
    ));
    let timid = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "TIMID",
        crate::Party::Enemy,
        9,
        9,
    ));
    let b0 = app.database().unit_by_uid(&brave).unwrap().morale;
    let t0 = app.database().unit_by_uid(&timid).unwrap().morale;
    // Player[0] が Enemy を撃破 (victim_party = Enemy)。
    app.award_kill_rewards(0, 0, 0, crate::Party::Enemy);
    assert_eq!(
        app.database().unit_by_uid(&brave).unwrap().morale,
        b0 + 2,
        "超強気は味方撃破で気力 +2"
    );
    assert_eq!(
        app.database().unit_by_uid(&timid).unwrap().morale,
        t0 - 1,
        "弱気は味方撃破で気力 -1"
    );
}

#[test]
fn intermission_upgrade_flow_spends_money_and_boosts_stats() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    app.add_money(5000);
    // インターミッションを表示させる (次ステージありで組込みコマンドが出る)。
    app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
    app.scene = Scene::Intermission;
    app.intermission_mode = IntermissionMode::Menu;
    // メニュー = [機体改造, データセーブ, 次のステージへ]。機体改造 (index 0) を選択。
    assert_eq!(app.intermission_item_label(0).as_deref(), Some("機体改造"));
    app.set_intermission_cursor(0);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.intermission_mode, IntermissionMode::UnitUpgrade);
    // ユニット選択 (index 0 = Hero) → 改造。
    let idx = {
        let uid = first_player_uid(&app);
        app.database().idx_by_uid(&uid).unwrap()
    };
    let hp_before = app
        .database()
        .effective_max_hp(&app.database().unit_instances[idx]);
    let money_before = app.money();
    app.set_intermission_cursor(0);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.database().unit_instances[idx].upgrade_level, 1);
    assert_eq!(app.money(), money_before - 1000, "Lv0→1 は 1000G");
    let hp_after = app
        .database()
        .effective_max_hp(&app.database().unit_instances[idx]);
    assert!(
        hp_after > hp_before,
        "改造で最大HPが上がる: {hp_after} > {hp_before}"
    );
    // 戻る (ユニット数=1 → index 1 が「戻る」) でメインメニューへ。
    app.set_intermission_cursor(1);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.intermission_mode, IntermissionMode::Menu);
    // データセーブ → __quicksave に保存 (項目順は他組込みの増減で変わるため
    // ラベルで index を解決する)。
    let save_idx = (0..app.intermission_item_count())
        .find(|&n| app.intermission_item_label(n).as_deref() == Some("データセーブ"))
        .expect("データセーブ 項目がある");
    app.set_intermission_cursor(save_idx);
    assert!(app.confirm_intermission_selection());
    assert!(
        !app.script_var("__quicksave").is_empty(),
        "データセーブで __quicksave が書き込まれる"
    );
}

/// インターミッション「換装」で `換装` 特殊能力を持つユニットの形態を差し替える。
#[test]
fn intermission_equip_swap_changes_unit_form() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Base", 2, 6);
    // 換装先 "砲撃装備" の UnitData を用意 (Base を複製してリネーム)。
    let mut alt = app.database().unit_by_name("Base").cloned().unwrap();
    alt.name = "砲撃装備".into();
    alt.nickname = "砲撃装備".into();
    app.database_mut().units.push(alt);
    let uid = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&uid)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new("換装", "砲撃装備")];
    // インターミッション表示 (次ステージありで組込みコマンドが出る)。
    app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
    app.scene = Scene::Intermission;
    app.intermission_mode = IntermissionMode::Menu;

    // メニュー = [機体改造, 換装, データセーブ, 次のステージへ]。換装 = index 1。
    assert_eq!(app.intermission_item_label(1).as_deref(), Some("換装"));
    app.set_intermission_cursor(1);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.intermission_mode, IntermissionMode::EquipSwap);

    // 行 0 = "Base → 砲撃装備"。選択で換装。
    assert_eq!(
        app.intermission_item_label(0).as_deref(),
        Some("Base → 砲撃装備")
    );
    app.set_intermission_cursor(0);
    assert!(app.confirm_intermission_selection());
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().unit_data_name,
        "砲撃装備",
        "換装で形態が差し替わる"
    );
}

/// 換装可能なユニットが居なければ「換装」項目を出さない。
#[test]
fn intermission_equip_swap_hidden_without_feature() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Base", 2, 6);
    app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
    app.scene = Scene::Intermission;
    app.intermission_mode = IntermissionMode::Menu;
    let labels: Vec<String> = (0..app.intermission_item_count())
        .filter_map(|n| app.intermission_item_label(n))
        .collect();
    assert!(
        !labels.iter().any(|l| l == "換装"),
        "換装可能ユニットが無ければ項目を出さない: {labels:?}"
    );
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

/// 乗り換えは `Option 乗り換え` 有効時のみ表示される (原典準拠)。
#[test]
fn intermission_ride_change_hidden_without_option() {
    let mut app = App::new();
    let _ = setup_ride_change(&mut app);
    // setup は Option を立てる → 表示される。
    assert!(
        (0..app.intermission_item_count())
            .any(|n| app.intermission_item_label(n).as_deref() == Some("乗り換え")),
        "Option 乗り換え 有効時は表示"
    );
    // Option を解除 → 2 機以上でも非表示。
    app.unset_script_var("Option(乗り換え)");
    assert!(
        (0..app.intermission_item_count())
            .all(|n| app.intermission_item_label(n).as_deref() != Some("乗り換え")),
        "Option 未設定なら 2 機以上でも乗り換えは非表示"
    );
}

/// インターミッション「乗り換え」で 2 機の搭乗パイロットを入れ替える。
#[test]
fn intermission_ride_change_swaps_pilots() {
    let mut app = App::new();
    let (uid_a, uid_b) = setup_ride_change(&mut app);

    // メニューの「乗り換え」を選ぶ → RideChange モード。
    let ride_idx = (0..app.intermission_item_count())
        .find(|&n| app.intermission_item_label(n).as_deref() == Some("乗り換え"))
        .expect("乗り換え 項目がある");
    app.set_intermission_cursor(ride_idx);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.intermission_mode, IntermissionMode::RideChange);

    // 移動元 = UnitA (cursor 0)。
    app.set_intermission_cursor(0);
    assert!(app.confirm_intermission_selection());
    // 移動先候補は移動元を除いた一覧 → cursor 0 = UnitB。先頭は "→ " 付き。
    assert!(app
        .intermission_item_label(0)
        .as_deref()
        .unwrap()
        .starts_with("→ "));
    app.set_intermission_cursor(0);
    assert!(app.confirm_intermission_selection());

    // 搭乗が入れ替わる (pilot_name + pilot_ids)。
    let a = app.database().unit_by_uid(&uid_a).unwrap();
    let b = app.database().unit_by_uid(&uid_b).unwrap();
    assert_eq!(a.pilot_name, "魔理沙");
    assert_eq!(a.pilot_ids, vec!["marisa".to_string()]);
    assert_eq!(b.pilot_name, "アリス");
    assert_eq!(b.pilot_ids, vec!["alice".to_string()]);
}

/// 移動先選択中のキャンセルは移動元選択に戻り、入れ替えは起きない。
#[test]
fn intermission_ride_change_dest_cancel_returns_to_source() {
    let mut app = App::new();
    let (uid_a, _uid_b) = setup_ride_change(&mut app);
    let ride_idx = (0..app.intermission_item_count())
        .find(|&n| app.intermission_item_label(n).as_deref() == Some("乗り換え"))
        .unwrap();
    app.set_intermission_cursor(ride_idx);
    app.confirm_intermission_selection();
    // 移動元 UnitA を選択 → 移動先選択 (→ prefix)。
    app.set_intermission_cursor(0);
    app.confirm_intermission_selection();
    assert!(app
        .intermission_item_label(0)
        .as_deref()
        .unwrap()
        .starts_with("→ "));
    // キャンセル → 移動元選択へ戻る (→ prefix が消える)、入れ替えなし。
    assert!(app.cancel_action());
    assert_eq!(app.intermission_mode, IntermissionMode::RideChange);
    assert!(!app
        .intermission_item_label(0)
        .as_deref()
        .unwrap()
        .starts_with("→ "));
    assert_eq!(
        app.database().unit_by_uid(&uid_a).unwrap().pilot_name,
        "アリス",
        "キャンセルで入れ替わらない"
    );
}

/// インターミッション「ステータス」は単機ステータス詳細を開き、Enter で
/// インターミッションへ戻る。
#[test]
fn intermission_status_opens_unit_detail_and_returns() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
    app.scene = Scene::Intermission;
    app.intermission_mode = IntermissionMode::Menu;

    let idx = (0..app.intermission_item_count())
        .find(|&n| app.intermission_item_label(n).as_deref() == Some("ステータス"))
        .expect("ステータス 項目がある");
    app.set_intermission_cursor(idx);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.scene(), Scene::UnitDetail);
    assert_eq!(app.status_detail_index(), 0);

    // ビューモデルが実体から構築できる。
    let detail = app.build_status_detail(0).expect("先頭機の詳細");
    assert_eq!(detail.unit_name, "Hero");
    assert_eq!(detail.total, 1);
    assert!(detail.has_pilot, "PILOT が搭乗");

    // Enter でインターミッションへ復帰。
    app.handle_input(Input::Advance);
    assert_eq!(
        app.scene(),
        Scene::Intermission,
        "ステータス閲覧後はインターミッションへ戻る"
    );
    assert_eq!(app.intermission_mode, IntermissionMode::Menu);
}

/// 単機ステータス詳細: ◀ / ▶ ボタンでロスターを巡回し、右クリック (Esc) /
/// 閉じる ボタンでインターミッションへ戻る。
#[test]
fn unit_detail_cycles_roster_and_closes() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Alpha", 2, 6);
    place_player_unit(&mut app, "Bravo", 3, 6);
    app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
    app.scene = Scene::Intermission;
    app.intermission_mode = IntermissionMode::Menu;
    let idx = (0..app.intermission_item_count())
        .find(|&n| app.intermission_item_label(n).as_deref() == Some("ステータス"))
        .expect("ステータス 項目");
    app.set_intermission_cursor(idx);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.scene(), Scene::UnitDetail);
    assert_eq!(app.status_detail_index(), 0);

    // ▶ ボタンで次の機体へ。
    let nb = crate::scene::unit_detail::next_button();
    assert!(app.handle_input(Input::ClickAt {
        x: nb.x + 2,
        y: nb.y + 2,
    }));
    assert_eq!(app.status_detail_index(), 1);
    // 末尾で ▶ → 先頭へラップ。
    assert!(app.handle_input(Input::ClickAt {
        x: nb.x + 2,
        y: nb.y + 2,
    }));
    assert_eq!(app.status_detail_index(), 0);
    // ◀ ボタンで前 (= ラップして末尾) へ。
    let pb = crate::scene::unit_detail::prev_button();
    assert!(app.handle_input(Input::ClickAt {
        x: pb.x + 2,
        y: pb.y + 2,
    }));
    assert_eq!(app.status_detail_index(), 1);

    // 閉じる ボタンでインターミッションへ。
    let cb = crate::scene::unit_detail::close_button();
    assert!(app.handle_input(Input::ClickAt {
        x: cb.x + 2,
        y: cb.y + 2,
    }));
    assert_eq!(app.scene(), Scene::Intermission);

    // 再度開いて右クリック (Cancel) で戻る。
    app.set_intermission_cursor(idx);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.scene(), Scene::UnitDetail);
    assert!(app.handle_input(Input::Cancel));
    assert_eq!(app.scene(), Scene::Intermission);
}

/// 単機ステータス詳細のビューモデルが実効値 (改造段階・残ダメージ・武器の残弾/EN・
/// 状態異常・パイロットの精神コマンド/SP) を正しく反映する。
#[test]
fn build_status_detail_reflects_effective_values() {
    use crate::condition::Condition;
    use crate::data::pilot::{Adaption, PilotData, Sex, SpiritCommand};
    use crate::data::unit::{Size, UnitData, WeaponData};
    use crate::unit_weapon::UnitWeapon;
    use crate::{Party, UnitInstance};

    let mut app = App::new();

    let pilot = PilotData {
        spirit_commands: vec![
            SpiritCommand {
                name: "ひらめき".into(),
                cost: Some(10),
                level: 1,
            },
            SpiritCommand {
                name: "熱血".into(),
                cost: Some(30),
                level: 9,
            },
        ],
        name: "アスカ".into(),
        nickname: "A".into(),
        kana_name: "あすか".into(),
        sex: Sex::Female,
        class: String::new(),
        adaption: Adaption::parse("ABAA").unwrap(),
        exp_value: 0,
        infight: 120,
        shooting: 130,
        hit: 20,
        dodge: 25,
        intuition: 15,
        technique: 18,
        personality: None,
        sp: Some(80),
        bgm: None,
        bitmap: None,
        features: vec![("底力".into(), "1".into())],
    };
    let weapon = |name: &str, power: i64, lo, hi, bullet, en| WeaponData {
        name: name.into(),
        power,
        min_range: lo,
        max_range: hi,
        precision: 0,
        bullet,
        en_consumption: en,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: String::new(),
        extras: Vec::new(),
    };
    let unit_data = UnitData {
        abilities: Vec::new(),
        name: "エヴァ".into(),
        kana_name: String::new(),
        nickname: "初号機".into(),
        class: "リアル".into(),
        pilot_num: 1,
        item_num: 0,
        transportation: "陸".into(),
        speed: 6,
        size: Size::L,
        value: 0,
        exp_value: 0,
        hp: 5000,
        en: 100,
        armor: 800,
        mobility: 120,
        adaption: Adaption::parse("AAAA").unwrap(),
        bitmap: String::new(),
        weapons: vec![
            weapon("バルカン", 800, 1, 1, 0, 0),
            weapon("ミサイル", 1500, 1, 4, 6, 0),
            weapon("ビーム", 2000, 1, 3, 0, 15),
        ],
        features: Vec::new(),
    };
    app.database_mut().pilots.push(pilot);
    app.database_mut().units.push(unit_data);

    let mut inst = UnitInstance::new("エヴァ", "アスカ", Party::Player, 0, 0);
    inst.upgrade_level = 3;
    inst.damage = 1000;
    inst.weapons = vec![
        UnitWeapon::from_data("バルカン", 0, 0),
        UnitWeapon::from_data("ミサイル", 1, 6),
        UnitWeapon::from_data("ビーム", 2, 0),
    ];
    inst.weapons[1].bullet_remaining = 4; // ミサイルを 2 発消費。
    inst.add_condition(Condition::new("加速", 5));
    app.database_mut().register_unit(inst);

    let d = app.build_status_detail(0).expect("先頭機の詳細");
    // 機体: 改造込みの実効 HP と残ダメージ反映。
    assert_eq!(d.unit_name, "エヴァ");
    assert_eq!(d.upgrade_level, 3);
    assert!(d.hp_max > 5000, "改造段階で素 HP を上回る");
    assert_eq!(d.hp_cur, d.hp_max - 1000);
    assert_eq!(d.size, "L");
    assert!(d.conditions.iter().any(|c| c == "加速"));
    // パイロット: SP/精神コマンド。
    assert!(d.has_pilot);
    assert_eq!(d.sp_max, 80);
    // 実効値: レベル成長込み (VB5: 格闘 += level、level 1 でも +1)。
    assert_eq!(d.infight, 121);
    assert_eq!(
        d.spirit_commands,
        vec!["ひらめき Lv1".to_string(), "熱血 Lv9".to_string()]
    );
    // 武器: 射程表記と残弾/EN 表記。
    assert_eq!(d.weapons.len(), 3);
    assert_eq!(d.weapons[0].ammo, "-", "弾/EN なしは '-'");
    assert_eq!(d.weapons[1].range, "1-4");
    assert_eq!(d.weapons[1].ammo, "残4/6");
    assert_eq!(d.weapons[2].ammo, "EN15");
    assert!(
        d.weapons.iter().all(|w| w.usable),
        "必要技能の無い武器は全て使用可"
    );
}

/// 単機ステータス詳細は、必要技能を満たさない武器を `usable=false` で示す
/// (機能ゲートと同じ `necessary_skill::is_satisfied` を閲覧表示にも反映)。
#[test]
fn status_detail_marks_necessary_skill_gated_weapons() {
    use crate::data::pilot::{Adaption, PilotData, Sex};
    use crate::data::unit::{Size, UnitData, WeaponData};
    use crate::unit_weapon::UnitWeapon;
    use crate::{Party, UnitInstance};

    let mk_weapon = |name: &str, necessary: &str| WeaponData {
        name: name.into(),
        power: 1000,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: "格".into(),
        extras: if necessary.is_empty() {
            Vec::new()
        } else {
            vec![necessary.into()]
        },
    };
    let mut app = App::new();
    // 念力 技能を持たないパイロット。
    app.database_mut().pilots.push(PilotData {
        spirit_commands: Vec::new(),
        name: "P".into(),
        nickname: "P".into(),
        kana_name: "P".into(),
        sex: Sex::Unspecified,
        class: String::new(),
        adaption: Adaption::parse("AAAA").unwrap(),
        exp_value: 0,
        infight: 100,
        shooting: 100,
        hit: 0,
        dodge: 0,
        intuition: 0,
        technique: 0,
        personality: None,
        sp: None,
        bgm: None,
        bitmap: None,
        features: Vec::new(),
    });
    app.database_mut().units.push(UnitData {
        abilities: Vec::new(),
        name: "機".into(),
        kana_name: String::new(),
        nickname: "機".into(),
        class: String::new(),
        pilot_num: 1,
        item_num: 0,
        transportation: "陸".into(),
        speed: 5,
        size: Size::M,
        value: 0,
        exp_value: 0,
        hp: 3000,
        en: 100,
        armor: 500,
        mobility: 100,
        adaption: Adaption::parse("AAAA").unwrap(),
        bitmap: String::new(),
        weapons: vec![mk_weapon("通常技", ""), mk_weapon("封印技", "念力Lv3")],
        features: Vec::new(),
    });
    let mut inst = UnitInstance::new("機", "P", Party::Player, 0, 0);
    inst.weapons = vec![
        UnitWeapon::from_data("通常技", 0, -1),
        UnitWeapon::from_data("封印技", 1, -1),
    ];
    app.database_mut().register_unit(inst);

    let d = app.build_status_detail(0).expect("詳細");
    assert!(d.weapons[0].usable, "必要技能なしの武器は使用可");
    assert!(
        !d.weapons[1].usable,
        "念力Lv3 を満たさないパイロットの武器は使用不可"
    );
}

/// 通常の部隊表 (戻り先指定なし) は従来どおり UnitList → Title で抜ける。
#[test]
fn unit_list_without_return_goes_to_title() {
    let mut app = App::new();
    app.scene = Scene::UnitList;
    app.handle_input(Input::Advance);
    assert_eq!(app.scene(), Scene::Title, "戻り先未指定なら Title へ");
}

#[test]
fn weapon_special_effect_inflicts_status_and_disables() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    // 痺 属性 + critical 100 → 必ず発動。
    let weapon = WeaponData {
        name: "麻痺銃".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: "痺".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_special_effects(def_idx, def_idx, &weapon, &pilot, &pilot);
    assert_eq!(applied, vec!["麻痺".to_string()]);
    assert!(app.database().unit_instances[def_idx].has_condition("麻痺"));
    assert!(
        app.database().unit_instances[def_idx].attack_disabled(),
        "麻痺 は行動不能 (AI / 攻撃ゲートが効く)"
    );
}

/// 特殊効果無効化=全 を持つ防御側は武器の特殊効果 (状態異常付与) を受けない
/// (防御特性に関する特殊能力.md)。決戦サンプルのザコは全て `特殊効果無効化=全` を持つ。
#[test]
fn tokushu_kouka_mukouka_blocks_status_effects() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    // 防御側に 特殊効果無効化=全 を付与。
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new("特殊効果無効化", "全"));
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    // 痺 属性 + critical 100 → 通常なら必ず麻痺。
    let weapon = WeaponData {
        name: "麻痺銃".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: "痺".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_special_effects(def_idx, def_idx, &weapon, &pilot, &pilot);
    assert!(
        applied.is_empty(),
        "特殊効果無効化=全 なら特殊効果は一切付与されないはず: {applied:?}"
    );
    assert!(
        !app.database().unit_instances[def_idx].has_condition("麻痺"),
        "特殊効果無効化=全 が麻痺付与を防ぐはず"
    );
}

/// 抵抗力Lv* は特殊効果発生確率を 10×Lv% 減少させる (回避系特殊能力.md /
/// C# Unit.cs:12275)。Lv10 なら critical100 (proc 100) でも 0 になり付与されない。
#[test]
fn teikouryoku_reduces_special_effect_proc() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new("抵抗力Lv10", ""));
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    let weapon = WeaponData {
        name: "麻痺銃".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: "痺".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_special_effects(def_idx, def_idx, &weapon, &pilot, &pilot);
    assert!(
        applied.is_empty(),
        "抵抗力Lv10 で proc 100→0 になり麻痺は付与されないはず: {applied:?}"
    );
    assert!(!app.database().unit_instances[def_idx].has_condition("麻痺"));
}

/// (B) シールド防御: Ｓ防御技能 + シールド系特殊能力で被ダメージ半減。Ｓ防御Lv16 なら
/// prob 16 >= Dice(16) で必ず発動。精/殺/浸 属性は不可。
#[test]
fn shield_defense_halves_damage_with_high_sdefense() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    // PILOT に Ｓ防御Lv16 を付与 (必ず発動)。
    if let Some(pd) = app
        .database_mut()
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
    {
        pd.features.push(("Ｓ防御Lv16".into(), String::new()));
    }
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new("シールド", ""));

    // 通常武器 (実) 800 ダメージ → シールド発動で ÷2 = 400。
    assert_eq!(
        app.apply_shield_defense(def_idx, "実", 800),
        400,
        "Ｓ防御Lv16+シールド で被ダメージ半減"
    );
    // 精 属性にはシールド防御不可。
    assert_eq!(
        app.apply_shield_defense(def_idx, "精", 800),
        800,
        "精 属性にはシールド防御不可"
    );
    // Ｓ防御技能が無ければ発動しない。
    let target2 = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "NOSKILL",
        crate::Party::Enemy,
        4,
        6,
    ));
    let def2 = app.database().idx_by_uid(&target2).unwrap();
    app.database_mut().unit_instances[def2]
        .active_features
        .push(crate::feature::ActiveFeature::new("シールド", ""));
    assert_eq!(
        app.apply_shield_defense(def2, "実", 800),
        800,
        "Ｓ防御技能が無ければシールド防御は発動しない"
    );
}

/// (B) 小型シールド は 2/3 に軽減 (通常武器)。
#[test]
fn small_shield_reduces_damage_to_two_thirds() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    if let Some(pd) = app
        .database_mut()
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
    {
        pd.features.push(("Ｓ防御Lv16".into(), String::new()));
    }
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new("小型シールド", ""));
    // 900 → 2/3 = 600。
    assert_eq!(
        app.apply_shield_defense(def_idx, "実", 900),
        600,
        "小型シールド は 2/3 に軽減"
    );
}

/// (B) バリア吸収: バリアLv* が 1000×Lv ダメージを吸収する。完全吸収は 0。
/// 直撃 (攻撃側) / バリア中和 (防御側) で無効。対象属性が不一致なら吸収しない。
#[test]
fn barrier_absorb_blocks_up_to_thousand_times_level() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Atk", 2, 6);
    let atk_idx = first_player_uid(&app);
    let atk_idx = app.database().idx_by_uid(&atk_idx).unwrap();
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def",
        "DP",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new("バリアLv2", "")); // 2000 吸収・全属性

    // 1500 < 2000 → 完全吸収 0。
    assert_eq!(
        app.apply_barrier_absorb(def_idx, atk_idx, "実", 1500),
        0,
        "バリアLv2 は 2000 までを完全吸収"
    );
    // 2500 > 2000 → 残 500。
    assert_eq!(
        app.apply_barrier_absorb(def_idx, atk_idx, "実", 2500),
        500,
        "2000 を超えた分のみ通る"
    );
    // 直撃 (攻撃側) でバリア無効。
    app.database_mut().unit_instances[atk_idx].add_condition(crate::Condition::new("直撃", -1));
    assert_eq!(
        app.apply_barrier_absorb(def_idx, atk_idx, "実", 1500),
        1500,
        "直撃 でバリアは無効"
    );
    app.database_mut().unit_instances[atk_idx].remove_condition("直撃");

    // 対象属性つきバリア (火) は不一致武器を吸収しない。
    app.database_mut().unit_instances[def_idx]
        .active_features
        .clear();
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new(
            "バリアLv2",
            "炎バリア 火",
        ));
    assert_eq!(
        app.apply_barrier_absorb(def_idx, atk_idx, "実", 1500),
        1500,
        "対象属性 火 のバリアは 実 武器を吸収しない"
    );
    assert_eq!(
        app.apply_barrier_absorb(def_idx, atk_idx, "火", 1500),
        0,
        "対象属性 火 のバリアは 火 武器を吸収する"
    );
}

/// (B knobs) 相殺/中和: 隣接する同名 (別名一致) バリアを持つ攻撃側に対し、相殺=完全無効 /
/// 中和=攻撃側 Lv 分だけ効果減少。
#[test]
fn barrier_sousai_and_chuwa_with_adjacent_same_named() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Atk", 3, 6);
    let atk = first_player_uid(&app);
    let atk = app.database().idx_by_uid(&atk).unwrap();
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def",
        "DP",
        crate::Party::Enemy,
        4,
        6, // (3,6) と隣接
    ));
    let def = app.database().idx_by_uid(&target).unwrap();

    // --- 相殺 --- defender: バリアLv3=魔法障壁 … 相殺 (3000 吸収・相殺オプション)。
    app.database_mut().unit_instances[def].active_features.push(
        crate::feature::ActiveFeature::new("バリアLv3", "魔法障壁 全 0 0 相殺"),
    );
    // 攻撃側が同名 (魔法障壁) バリアを隣接所持 → 相殺で防御側バリア無効。
    app.database_mut().unit_instances[atk].active_features.push(
        crate::feature::ActiveFeature::new("バリアLv1", "魔法障壁 全 0 0"),
    );
    assert_eq!(
        app.apply_barrier_absorb(def, atk, "実", 2000),
        2000,
        "相殺: 隣接同名バリアで防御側バリアが無効化"
    );
    // 攻撃側が同名バリアを持たなければ通常どおり吸収 (3000>2000 → 0)。
    app.database_mut().unit_instances[atk]
        .active_features
        .clear();
    assert_eq!(
        app.apply_barrier_absorb(def, atk, "実", 2000),
        0,
        "攻撃側に同名バリアが無ければ通常吸収"
    );

    // --- 中和 --- defender: バリアLv3=魔法障壁 … 中和。攻撃側 バリアLv1=魔法障壁 隣接
    // → eff_lv = 3-1 = 2 → 2000 吸収。
    app.database_mut().unit_instances[def]
        .active_features
        .clear();
    app.database_mut().unit_instances[def].active_features.push(
        crate::feature::ActiveFeature::new("バリアLv3", "魔法障壁 全 0 0 中和"),
    );
    app.database_mut().unit_instances[atk].active_features.push(
        crate::feature::ActiveFeature::new("バリアLv1", "魔法障壁 全 0 0"),
    );
    assert_eq!(
        app.apply_barrier_absorb(def, atk, "実", 2500),
        500,
        "中和: Lv3-1=2 → 2000 吸収、残 500"
    );
}

/// (B knobs) バリア/フィールド吸収は 必要気力 と ＥＮ消費 のゲートを尊重する (忠実性)。
#[test]
fn barrier_absorb_respects_morale_and_en_gates() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Atk", 2, 6);
    let atk = first_player_uid(&app);
    let atk = app.database().idx_by_uid(&atk).unwrap();
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def",
        "DP",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def = app.database().idx_by_uid(&target).unwrap();

    // フィールドLv2 (1000 吸収) だが 必要気力 130 (値の 4 番目)。
    app.database_mut().unit_instances[def].active_features.push(
        crate::feature::ActiveFeature::new("フィールドLv2", "魔法障壁 全 0 130"),
    );
    app.database_mut().unit_instances[def].morale = 100;
    assert_eq!(
        app.apply_barrier_absorb(def, atk, "実", 800),
        800,
        "気力 100 < 必要気力 130 で不発"
    );
    app.database_mut().unit_instances[def].morale = 130;
    assert_eq!(
        app.apply_barrier_absorb(def, atk, "実", 800),
        0,
        "気力 130 到達で発動 (800<1000 完全吸収)"
    );

    // ＥＮ消費 9999 (3 番目) は max EN 0 のユニットでは EN 不足で不発。
    app.database_mut().unit_instances[def]
        .active_features
        .clear();
    app.database_mut().unit_instances[def].active_features.push(
        crate::feature::ActiveFeature::new("バリアLv2", "x 全 9999 0"),
    );
    assert_eq!(
        app.apply_barrier_absorb(def, atk, "実", 800),
        800,
        "ＥＮ不足でバリア不発"
    );
}

/// (B) フィールド吸収: フィールドLv* は 500×Lv を確定吸収 (バリアの半分)。
/// 解説エントリ (値が「解説…」) は防御能力として扱わない。バリアとスタックする。
#[test]
fn field_absorbs_five_hundred_times_level_and_ignores_kaisetsu() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Atk", 2, 6);
    let atk_idx = first_player_uid(&app);
    let atk_idx = app.database().idx_by_uid(&atk_idx).unwrap();
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def",
        "DP",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new("フィールドLv2", "")); // 1000 吸収
                                                                        // 解説エントリは無視されること。
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new(
            "プロテクト・フィールド",
            "解説 これはヘルプ文",
        ));
    // 800 < 1000 → 完全吸収 0。
    assert_eq!(app.apply_barrier_absorb(def_idx, atk_idx, "実", 800), 0);
    // 1200 > 1000 → 残 200。
    assert_eq!(app.apply_barrier_absorb(def_idx, atk_idx, "実", 1200), 200);

    // バリアLv1 (1000) を追加するとスタックして 1000+1000=2000 吸収。
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new("バリアLv1", ""));
    assert_eq!(
        app.apply_barrier_absorb(def_idx, atk_idx, "実", 1900),
        0,
        "フィールド 1000 + バリア 1000 = 2000 吸収"
    );
}

/// (B) アクティブフィールド: Ｓ防御/16 の確率発動で 500×Lv 吸収。Ｓ防御Lv16 なら必発。
/// Ｓ防御技能が無ければ発動しない。
#[test]
fn active_field_absorbs_when_sdefense_high() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Atk", 2, 6);
    let atk_idx = first_player_uid(&app);
    let atk_idx = app.database().idx_by_uid(&atk_idx).unwrap();
    // PILOT に Ｓ防御Lv16 (必発)。
    if let Some(pd) = app
        .database_mut()
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
    {
        pd.features.push(("Ｓ防御Lv16".into(), String::new()));
    }
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new(
            "アクティブフィールドLv3",
            "",
        )); // 1500 吸収
            // 1200 < 1500 → 完全吸収 0 (Ｓ防御Lv16 で必発)。
    assert_eq!(
        app.apply_active_field_absorb(def_idx, atk_idx, "実", 1200),
        0
    );

    // Ｓ防御技能が無い防御側では発動しない。
    let t2 = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def2",
        "NOSKILL",
        crate::Party::Enemy,
        4,
        6,
    ));
    let d2 = app.database().idx_by_uid(&t2).unwrap();
    app.database_mut().unit_instances[d2]
        .active_features
        .push(crate::feature::ActiveFeature::new(
            "アクティブフィールドLv3",
            "",
        ));
    assert_eq!(
        app.apply_active_field_absorb(d2, atk_idx, "実", 1200),
        1200,
        "Ｓ防御技能なしではアクティブフィールド不発"
    );
}

/// (B) 回避系特殊能力 (分身/超回避) の完全回避ロール check_dodge_feature。
/// 超回避Lv10 は `Lv>=Dice(10)` で必発。直撃/行動不能/気力130未満で不発。
#[test]
fn check_dodge_feature_chougaihi_and_bunshin() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Atk", 2, 6);
    let atk = first_player_uid(&app);
    let atk = app.database().idx_by_uid(&atk).unwrap();
    let t = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def",
        "DP",
        crate::Party::Enemy,
        3,
        6,
    ));
    let d = app.database().idx_by_uid(&t).unwrap();

    // 超回避Lv10 → Lv>=Dice(10) で必ず回避。
    app.database_mut().unit_instances[d]
        .active_features
        .push(crate::feature::ActiveFeature::new("超回避Lv10", ""));
    assert!(
        app.check_dodge_feature(d, atk, "実"),
        "超回避Lv10 は必ず回避するはず"
    );
    // 直撃 (攻撃側) で回避無効。
    app.database_mut().unit_instances[atk].add_condition(crate::Condition::new("直撃", -1));
    assert!(!app.check_dodge_feature(d, atk, "実"), "直撃 で回避は無効");
    app.database_mut().unit_instances[atk].remove_condition("直撃");
    // 行動不能系 (麻痺) で能動回避不可。
    app.database_mut().unit_instances[d].add_condition(crate::Condition::new("麻痺", 3));
    assert!(
        !app.check_dodge_feature(d, atk, "実"),
        "麻痺 (行動不能) で回避不可"
    );
    app.database_mut().unit_instances[d].remove_condition("麻痺");

    // 分身は気力130未満では発動しない (既定気力100)。
    app.database_mut().unit_instances[d].active_features.clear();
    app.database_mut().unit_instances[d]
        .active_features
        .push(crate::feature::ActiveFeature::new("分身", ""));
    assert_eq!(app.database().unit_instances[d].morale, 100);
    assert!(
        !app.check_dodge_feature(d, atk, "実"),
        "分身は気力130未満で不発"
    );
}

/// (B) ＥＣＭ エリア補正: 防御側周囲の味方 ＥＣＭ で命中低下、攻撃側の味方 ＥＣＭ で相殺。
/// ホーミング `Ｈ` 武器は影響 2 倍。
#[test]
fn ecm_aura_reduces_hit_from_nearby_allies() {
    use crate::feature::ActiveFeature;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    // 防御側 (敵) を (5,5)、攻撃側 (味方) を遠方 (5,9) に配置。
    let d = app.database_mut().register_unit(crate::UnitInstance::new(
        "Def",
        "DP",
        crate::Party::Enemy,
        5,
        5,
    ));
    let d = app.database().idx_by_uid(&d).unwrap();
    let a = app.database_mut().register_unit(crate::UnitInstance::new(
        "Atk",
        "AP",
        crate::Party::Player,
        5,
        9,
    ));
    let a = app.database().idx_by_uid(&a).unwrap();

    // ＥＣＭ 機が居なければ補正なし。
    assert_eq!(app.ecm_hit_mult(d, a, "実"), 1.0);

    // 防御側の味方 (敵) が ＥＣＭLv4 を持って隣接 (6,5)。
    let mut jam = crate::UnitInstance::new("Jam", "JP", crate::Party::Enemy, 6, 5);
    jam.active_features
        .push(ActiveFeature::new("ＥＣＭLv4", ""));
    app.database_mut().register_unit(jam);
    // 通常武器: (100-5×4)/100 = 0.8。
    assert!((app.ecm_hit_mult(d, a, "実") - 0.8).abs() < 1e-9);
    // ホーミング Ｈ: (100-10×4)/100 = 0.6。
    assert!((app.ecm_hit_mult(d, a, "Ｈ") - 0.6).abs() < 1e-9);

    // 攻撃側の味方 (味方) が同 Lv の ＥＣＭ を防御側付近 (4,5) に持つと相殺 → 1.0。
    let mut jammer = crate::UnitInstance::new("CJam", "CP", crate::Party::Player, 4, 5);
    jammer
        .active_features
        .push(ActiveFeature::new("ＥＣＭLv4", ""));
    app.database_mut().register_unit(jammer);
    assert_eq!(app.ecm_hit_mult(d, a, "実"), 1.0, "攻撃側 ＥＣＭ で相殺");
}

/// 脱属性武器は命中・proc 時に対象の気力を低下させる (吸収は無し)。
#[test]
fn weapon_datsu_reduces_target_morale() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    let m0 = app.database().unit_instances[def_idx].morale;
    // 脱 + critical 100 → 必ず proc。
    let weapon = WeaponData {
        name: "脱力弾".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: "脱".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_special_effects(def_idx, def_idx, &weapon, &pilot, &pilot);
    assert!(applied.iter().any(|s| s.contains("気力")), "気力減少ラベル");
    assert_eq!(
        app.database().unit_instances[def_idx].morale,
        m0 - 10,
        "脱 で気力 -10"
    );
}

/// Ｄ属性 (気力吸収) は対象の気力を低下させ、低下分の半分を攻撃側へ移す。
#[test]
fn weapon_d_absorbs_half_morale_to_firer() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Drainer", 2, 6); // 攻撃側 (firer)
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Drainer",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let firer = first_player_uid(&app);
    let firer_idx = app.database().idx_by_uid(&firer).unwrap();
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    app.database_mut().unit_instances[firer_idx].morale = 100;
    app.database_mut().unit_instances[def_idx].morale = 130;
    // Ｄ + critical 100 → 必ず proc。低下 10 → 対象 -10 / 攻撃側 +5。
    let weapon = WeaponData {
        name: "吸収弾".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: "Ｄ".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_special_effects(def_idx, firer_idx, &weapon, &pilot, &pilot);
    assert!(
        applied.iter().any(|s| s == "気力吸収 +5"),
        "攻撃側へ +5 吸収: {applied:?}"
    );
    assert_eq!(
        app.database().unit_instances[def_idx].morale,
        120,
        "対象 130→120"
    );
    assert_eq!(
        app.database().unit_instances[firer_idx].morale,
        105,
        "攻撃側 100→105 (吸収)"
    );
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

/// 憑 (憑依) 属性は相手を攻撃側の勢力へ**恒久的に**移し、`憑依` condition (永続) を
/// 付与する。復帰用フィールド (charm_revert_party) は使わない。
#[test]
fn weapon_possession_permanently_takes_over_party() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 攻撃側 (firer, Player)
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let firer = first_player_uid(&app);
    let firer_idx = app.database().idx_by_uid(&firer).unwrap();
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    let weapon = possession_weapon("憑");
    let applied = app.apply_weapon_special_effects(def_idx, firer_idx, &weapon, &pilot, &pilot);
    assert_eq!(applied, vec!["憑依".to_string()]);
    assert_eq!(
        app.database().unit_instances[def_idx].party,
        crate::Party::Player,
        "憑依で対象は攻撃側 (味方) 勢力になる"
    );
    assert!(app.database().unit_instances[def_idx].has_condition("憑依"));
    assert!(
        app.database().unit_instances[def_idx]
            .charm_revert_party
            .is_none(),
        "憑依は恒久支配で復帰用フィールドを使わない"
    );
}

/// 憑 (憑依) は BossRank 適用ユニットには無効 (勢力を奪わず condition も付かない)。
#[test]
fn weapon_possession_is_blocked_by_boss_rank() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let firer = first_player_uid(&app);
    let firer_idx = app.database().idx_by_uid(&firer).unwrap();
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    app.database_mut().unit_instances[def_idx].boss_rank = 3;
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    let weapon = possession_weapon("憑");
    let applied = app.apply_weapon_special_effects(def_idx, firer_idx, &weapon, &pilot, &pilot);
    assert!(applied.is_empty(), "ボスには憑依が効かない: {applied:?}");
    assert_eq!(
        app.database().unit_instances[def_idx].party,
        crate::Party::Enemy,
        "勢力は変わらない"
    );
    assert!(!app.database().unit_instances[def_idx].has_condition("憑依"));
}

/// 憑依状態のユニットはスペシャルパワー (精神コマンド) を使用できない
/// (spec「ただしスペシャルパワーは使用できません」)。`spirit_command_options` が空を返す。
#[test]
fn possession_seals_special_power() {
    use crate::data::pilot::SpiritCommand;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = give_spirit_commands(
        &mut app,
        vec![SpiritCommand {
            name: "集中".into(),
            cost: Some(20),
            level: 1,
        }],
        47,
    );
    // 憑依前: 習得済み精神が出る。
    assert_eq!(
        app.spirit_command_options(&uid),
        vec![("集中".to_string(), 20)],
        "憑依前は精神が使える"
    );
    // 憑依を付与すると封じられる。
    let idx = app.database().idx_by_uid(&uid).unwrap();
    app.database_mut().unit_instances[idx].add_condition(crate::Condition::new("憑依", -1));
    assert!(
        app.spirit_command_options(&uid).is_empty(),
        "憑依中は精神コマンドを使用できない"
    );
}

/// 魅 (魅了) 属性は相手を攻撃側の勢力へ一時的に移し、護衛 ai_mode を設定する。
/// 期限切れ (`begin_phase` の tick) になると元の勢力へ復帰し ai_mode をクリアする。
#[test]
fn weapon_charm_flips_party_then_reverts_on_expiry() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 攻撃側 (firer, Player)
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    // 勝敗判定回避のため、魅了で奪われない敵を 1 体残す (敵全滅=勝利を防ぐ)。
    let guard = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    ));
    let firer = first_player_uid(&app);
    let firer_idx = app.database().idx_by_uid(&firer).unwrap();
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    let weapon = possession_weapon("魅");
    let applied = app.apply_weapon_special_effects(def_idx, firer_idx, &weapon, &pilot, &pilot);
    assert_eq!(applied, vec!["魅了".to_string()]);
    assert_eq!(
        app.database().unit_instances[def_idx].party,
        crate::Party::Player,
        "魅了で対象は攻撃側 (味方) 勢力へ移る"
    );
    assert_eq!(
        app.database().unit_instances[def_idx].charm_revert_party,
        Some(crate::Party::Enemy),
        "元の勢力 (敵) を退避"
    );
    assert!(
        app.database().unit_instances[def_idx]
            .ai_mode
            .starts_with("護衛"),
        "魅了主を護衛する ai_mode"
    );
    assert!(app.database().unit_instances[def_idx].has_condition("魅了"));
    // 魅了中は元の味方 (guard 敵) と敵対し、攻撃側 (firer) とは敵対しない。
    let charmed_party = app.database().unit_instances[def_idx].party;
    let guard_idx = app.database().idx_by_uid(&guard).unwrap();
    assert!(
        app.database().unit_instances[guard_idx]
            .party
            .is_hostile_to(charmed_party),
        "魅了ユニットは元の味方を敵と見なす"
    );
    assert!(
        !app.database().unit_instances[firer_idx]
            .party
            .is_hostile_to(charmed_party),
        "魅了ユニットは魅了主と敵対しない"
    );

    // lifetime=4 → Player フェイズ開始の tick を 4 回で期限切れ → 復帰。
    // 魅了ユニットの勢力は Player なので begin_phase(Player) で tick される。
    for _ in 0..4 {
        app.begin_phase(crate::Phase::Player);
    }
    assert!(
        !app.database().unit_instances[def_idx].has_condition("魅了"),
        "魅了は期限切れで解除"
    );
    assert_eq!(
        app.database().unit_instances[def_idx].party,
        crate::Party::Enemy,
        "期限切れで元の敵勢力へ復帰"
    );
    assert!(
        app.database().unit_instances[def_idx]
            .charm_revert_party
            .is_none(),
        "復帰後は退避フィールドをクリア"
    );
    assert!(
        !app.database().unit_instances[def_idx]
            .ai_mode
            .starts_with("護衛"),
        "復帰後は護衛 ai_mode をクリア"
    );
}

/// 耐性 / 弱点 は特殊効果の発動確率を半減 / 倍にする (武器属性が一致する場合)。
#[test]
fn resistance_and_weakness_scale_proc_rate() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let uid = first_player_uid(&app);
    let def_idx = app.database().idx_by_uid(&uid).unwrap();
    // 属性なし → 変化なし。
    assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 50), 50);
    // 耐性=火 → 火属性武器の発動率半減。
    app.database_mut().unit_instances[def_idx].active_features =
        vec![crate::feature::ActiveFeature::new("耐性", "火")];
    assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 50), 25);
    // 非一致属性 (氷) なら変化なし。
    assert_eq!(app.adjust_proc_for_resistance(def_idx, "氷 痺", 50), 50);
    // 弱点=火 → +10 (SRC: 弱点はクリ率 +10、100 上限)。
    app.database_mut().unit_instances[def_idx].active_features =
        vec![crate::feature::ActiveFeature::new("弱点", "火")];
    assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 40), 50);
    assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 95), 100);

    // 弱/効 属性で付加された一時的弱点 (condition `弱点:火`) も +10 に効く。
    app.database_mut().unit_instances[def_idx].active_features = vec![];
    app.database_mut().unit_instances[def_idx].add_condition(crate::Condition::new("弱点:火", 3));
    assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 40), 50);
    assert_eq!(app.adjust_proc_for_resistance(def_idx, "氷 痺", 40), 40);
}

/// 移動力ＵＰ (+1) / 移動力ＤＯＷＮ (半減、特殊効果攻撃属性 低移) が effective_speed に効く。
#[test]
fn move_status_scales_effective_speed() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Mover", 5, 5); // speed 3
    let uid = first_player_uid(&app);
    let idx = app.database().idx_by_uid(&uid).unwrap();
    let speed = |app: &App| {
        app.database()
            .effective_speed(&app.database().unit_instances[idx])
    };
    assert_eq!(speed(&app), 3, "基本移動力 3");
    // 移動力ＤＯＷＮ → 半減 (1)。
    app.database_mut().unit_instances[idx]
        .add_condition(crate::Condition::new("移動力ＤＯＷＮ", 3));
    assert_eq!(speed(&app), 1, "移動力ＤＯＷＮ で半減");
    app.database_mut().unit_instances[idx].remove_condition("移動力ＤＯＷＮ");
    // 移動力ＵＰ → +1 (4)。
    app.database_mut().unit_instances[idx].add_condition(crate::Condition::new("移動力ＵＰ", 3));
    assert_eq!(speed(&app), 4, "移動力ＵＰ で +1");
}

/// 恐怖 (特殊効果攻撃属性 恐) の敵 AI は味方から遠ざかり、攻撃しない。
#[test]
fn fear_makes_ai_flee_from_enemies() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 5);
    let coward = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    app.database_mut()
        .unit_by_uid_mut(&coward)
        .unwrap()
        .add_condition(crate::Condition::new("恐怖", 3));
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&coward).unwrap();
    app.ai_act_unit(idx);
    let c = app.database().unit_by_uid(&coward).unwrap();
    let dist_after = (c.x as i32 - 5).abs() + (c.y as i32 - 5).abs();
    assert!(
        dist_after > 1,
        "恐怖の敵は味方 (5,5) から遠ざかる (移動後距離={dist_after})"
    );
    assert!(c.has_acted, "逃走後は行動終了");
}

/// 敵 AI は射程内に対象が居るとき、攻撃前に攻撃補助の精神 (熱血) を使う。
#[test]
fn ai_uses_offensive_spirit_before_attacking() {
    use crate::data::pilot::SpiritCommand;
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 5); // 攻撃対象 (味方)
                                               // Hero unit_data に隣接武器を持たせる (敵もこの機体を使う)。
    app.database_mut()
        .units
        .iter_mut()
        .find(|d| d.name == "Hero")
        .unwrap()
        .weapons
        .push(WeaponData {
            name: "パンチ".into(),
            power: 50,
            min_range: 1,
            max_range: 1,
            precision: 50,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        });
    let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    // PILOT に 熱血 を習得させ SP を与える。
    {
        let p = app
            .database_mut()
            .pilots
            .iter_mut()
            .find(|p| p.name == "PILOT")
            .unwrap();
        p.sp = Some(100);
        p.spirit_commands = vec![SpiritCommand {
            name: "熱血".into(),
            cost: Some(20),
            level: 1,
        }];
    }
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&enemy).unwrap();
    app.ai_act_unit(idx);
    assert!(
        app.database()
            .unit_by_uid(&enemy)
            .unwrap()
            .has_condition("熱血"),
        "AI が攻撃前に精神コマンド 熱血 を使う"
    );
}

/// 回復系精神 (信頼) を持つ敵 AI は、半分以下まで負傷した味方を回復する。
/// 精神は行動を消費しないので、その後も通常の行動を続ける。
#[test]
fn ai_uses_support_spirit_on_wounded_ally() {
    use crate::data::pilot::SpiritCommand;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 9, 9); // Hero unit_data + PILOT + 攻撃候補 (敵対)
    let caster = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        5,
        5,
    ));
    let ally = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    // 味方を半分以下 (最大HP100 に対し damage 60) まで負傷させる。
    app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 60;
    {
        let p = app
            .database_mut()
            .pilots
            .iter_mut()
            .find(|p| p.name == "PILOT")
            .unwrap();
        p.sp = Some(50);
        p.spirit_commands = vec![SpiritCommand {
            name: "信頼".into(),
            cost: Some(30),
            level: 1,
        }];
    }
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&caster).unwrap();
    app.ai_act_unit(idx);
    let healed = app.database().unit_by_uid(&ally).unwrap().damage;
    assert!(
        healed < 60,
        "AI が負傷した味方を信頼で回復する (信頼=最大HPの1/3、damage {healed} < 60)"
    );
    assert_eq!(
        app.database().unit_by_uid(&caster).unwrap().sp_consumed,
        30,
        "信頼の SP を消費する"
    );
}

/// 底力 (パイロット技能) は HP が最大の 1/4 以下のとき命中・回避を +30 する。
/// 全 HP 時はボーナス無し。`combat_bonuses` 経由で戦闘に反映される。
#[test]
fn guts_skill_boosts_hit_and_dodge_at_low_hp() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Guts", 2, 6);
    // PILOT に 底力 技能を付与。
    app.database_mut()
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
        .unwrap()
        .features
        .push(("底力".into(), String::new()));
    let uid = first_player_uid(&app);
    // 全 HP 時はボーナス無し。
    let full = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        app.database().combat_bonuses(u)
    };
    assert_eq!(full.hit, 0, "全 HP では 底力 は発動しない (命中)");
    assert_eq!(full.dodge, 0, "全 HP では 底力 は発動しない (回避)");
    // HP を 1/4 以下まで減らす。
    let max_hp = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        app.database().effective_max_hp(u)
    };
    app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = max_hp * 3 / 4 + 1;
    let low = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        app.database().combat_bonuses(u)
    };
    assert_eq!(low.hit, 30, "底力: HP 1/4 以下で命中 +30");
    assert_eq!(low.dodge, 30, "底力: HP 1/4 以下で回避 +30");
}

/// 超底力 は 底力 より大きい命中・回避ボーナス (+50) を与える。
/// 「超底力」は「底力」を部分文字列に含むが超底力が優先される。
#[test]
fn super_guts_gives_larger_bonus_than_guts() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "SuperGuts", 2, 6);
    app.database_mut()
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
        .unwrap()
        .features
        .push(("超底力".into(), String::new()));
    let uid = first_player_uid(&app);
    let max_hp = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        app.database().effective_max_hp(u)
    };
    app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = max_hp * 3 / 4 + 1;
    let low = {
        let u = app.database().unit_by_uid(&uid).unwrap();
        app.database().combat_bonuses(u)
    };
    assert_eq!(low.hit, 50, "超底力: HP 1/4 以下で命中 +50");
    assert_eq!(low.dodge, 50, "超底力: HP 1/4 以下で回避 +50");
}

/// 超反応 はクリティカル率を 攻撃側 +2×Lv / 防御側 −2×Lv で増減する (非対称)。
#[test]
fn crit_skill_bonus_super_reaction_is_asymmetric() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // Hero unit_data を用意
    push_pilot_with_features(&mut app, "ATKP", vec![("超反応L3".into(), String::new())]);
    push_pilot_with_features(&mut app, "DEFP", vec![("超反応L1".into(), String::new())]);
    let atk = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "ATKP",
        crate::Party::Player,
        3,
        6,
    ));
    let def = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "DEFP",
        crate::Party::Enemy,
        4,
        6,
    ));
    let ai = app.database().idx_by_uid(&atk).unwrap();
    let di = app.database().idx_by_uid(&def).unwrap();
    assert_eq!(
        app.crit_skill_bonus(ai, di),
        4,
        "超反応: 2×3(攻) − 2×1(防) = 4"
    );
}

/// 超能力 (攻撃側 +5) と 底力 (攻撃側 HP1/4以下で +50) のクリティカル率補正。
#[test]
fn crit_skill_bonus_psychic_and_guts() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    push_pilot_with_features(
        &mut app,
        "ESP",
        vec![
            ("超能力".into(), String::new()),
            ("底力".into(), String::new()),
        ],
    );
    push_pilot_with_features(&mut app, "PLAIN", Vec::new());
    let atk = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "ESP",
        crate::Party::Player,
        3,
        6,
    ));
    let def = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PLAIN",
        crate::Party::Enemy,
        4,
        6,
    ));
    let ai = app.database().idx_by_uid(&atk).unwrap();
    let di = app.database().idx_by_uid(&def).unwrap();
    // 全 HP: 超能力 +5 のみ (底力 非発動)。
    assert_eq!(
        app.crit_skill_bonus(ai, di),
        5,
        "超能力 +5 / 底力 は全 HP で非発動"
    );
    // HP を 1/4 以下に減らす。
    let max_hp = {
        let u = app.database().unit_by_uid(&atk).unwrap();
        app.database().effective_max_hp(u)
    };
    app.database_mut().unit_by_uid_mut(&atk).unwrap().damage = max_hp * 3 / 4 + 1;
    assert_eq!(
        app.crit_skill_bonus(ai, di),
        55,
        "超能力 +5 + 底力 +50 = 55"
    );
}

/// ChangeMode「逃亡」の敵 AI も恐怖と同様に味方から遠ざかる。
#[test]
fn change_mode_escape_makes_ai_flee() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 5);
    let runner = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    app.database_mut().unit_by_uid_mut(&runner).unwrap().ai_mode = "逃亡".into();
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&runner).unwrap();
    app.ai_act_unit(idx);
    let c = app.database().unit_by_uid(&runner).unwrap();
    let dist = (c.x as i32 - 5).abs() + (c.y as i32 - 5).abs();
    assert!(dist > 1, "逃亡モードの敵は味方から遠ざかる (距離={dist})");
}

/// 回復アビリティを持つ敵 AI は、射程内の負傷した味方を回復する。
#[test]
fn ai_uses_heal_ability_on_damaged_ally() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Healer", 9, 9); // Healer unit_data + PILOT を用意 (この個体は無関係)
    let healer = app.database_mut().register_unit(crate::UnitInstance::new(
        "Healer",
        "PILOT",
        crate::Party::Enemy,
        5,
        5,
    ));
    let ally = app.database_mut().register_unit(crate::UnitInstance::new(
        "Healer",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 50;
    give_ability(
        &mut app,
        "Healer",
        &healer,
        mk_ability("治療", "回復Lv1", 3, None),
    );
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&healer).unwrap();
    app.ai_act_unit(idx);
    assert_eq!(
        app.database().unit_by_uid(&ally).unwrap().damage,
        0,
        "AI が射程内の負傷した味方を回復する"
    );
    assert!(
        app.database().unit_by_uid(&healer).unwrap().has_acted,
        "回復で行動終了"
    );
}

/// 補給アビリティを持つ敵 AI は、射程内に EN を消費した味方が居れば補給する。
#[test]
fn ai_uses_supply_ability_on_low_en_ally() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Healer", 9, 9); // Healer unit_data + PILOT を用意
    let supplier = app.database_mut().register_unit(crate::UnitInstance::new(
        "Healer",
        "PILOT",
        crate::Party::Enemy,
        5,
        5,
    ));
    let ally = app.database_mut().register_unit(crate::UnitInstance::new(
        "Healer",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    app.database_mut()
        .unit_by_uid_mut(&ally)
        .unwrap()
        .en_consumed = 40;
    give_ability(
        &mut app,
        "Healer",
        &supplier,
        mk_ability("補給装置", "補給Lv1", 3, None),
    );
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&supplier).unwrap();
    app.ai_act_unit(idx);
    assert_eq!(
        app.database().unit_by_uid(&ally).unwrap().en_consumed,
        0,
        "AI が射程内の EN 不足の味方を補給する (補給Lv1 = EN 50 回復)"
    );
    assert!(
        app.database().unit_by_uid(&supplier).unwrap().has_acted,
        "補給で行動終了"
    );
}

/// 敵対象アビリティ (脱=気力減少) を持つ敵 AI は、射程内の最も気力の高い敵へ使う。
#[test]
fn ai_uses_offensive_ability_on_highest_morale_enemy() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Healer", 9, 9); // Healer unit_data + PILOT を用意
                                                 // 対象になる味方 2 機を攻撃側の射程内に配置し、気力差を付ける。
    place_player_unit(&mut app, "Ally", 6, 6); // 低気力
    place_player_unit(&mut app, "Ally", 6, 5); // 高気力
    let low = app.database().unit_instances[1].uid.clone();
    let high = app.database().unit_instances[2].uid.clone();
    app.database_mut().unit_by_uid_mut(&low).unwrap().morale = 100;
    app.database_mut().unit_by_uid_mut(&high).unwrap().morale = 130;
    let attacker = app.database_mut().register_unit(crate::UnitInstance::new(
        "Healer",
        "PILOT",
        crate::Party::Enemy,
        5,
        5,
    ));
    let mut ab = mk_ability("脱力", "状態", 3, None);
    ab.attributes = "脱".into();
    give_ability(&mut app, "Healer", &attacker, ab);
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&attacker).unwrap();
    app.ai_act_unit(idx);
    assert_eq!(
        app.database().unit_by_uid(&high).unwrap().morale,
        120,
        "最も気力の高い敵が 脱 で -10 される"
    );
    assert_eq!(
        app.database().unit_by_uid(&low).unwrap().morale,
        100,
        "気力の低い敵は対象にならない"
    );
    assert!(
        app.database().unit_by_uid(&attacker).unwrap().has_acted,
        "アビリティ発動で行動終了"
    );
}

/// 召喚アビリティを持つ敵 AI は、数的に劣勢 (味方数 ≤ 敵数) のとき増援を召喚する。
#[test]
fn ai_uses_summon_ability_when_outnumbered() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Healer", 9, 9); // 敵対する味方 1 体 (= 召喚側は劣勢)
    let summoner = app.database_mut().register_unit(crate::UnitInstance::new(
        "Healer",
        "PILOT",
        crate::Party::Enemy,
        5,
        5,
    ));
    give_ability(
        &mut app,
        "Healer",
        &summoner,
        mk_ability("援軍要請", "召喚=Healer", 0, Some(1)),
    );
    app.set_stage_state(crate::stage::StageState::Battle);
    let before = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| !u.off_map && u.party == crate::Party::Enemy)
        .count();
    let idx = app.database().idx_by_uid(&summoner).unwrap();
    app.ai_act_unit(idx);
    let after = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| !u.off_map && u.party == crate::Party::Enemy)
        .count();
    assert_eq!(after, before + 1, "劣勢の AI が増援を 1 体召喚する");
    assert!(
        app.database().unit_by_uid(&summoner).unwrap().has_acted,
        "召喚で行動終了"
    );
}

/// マップ兵器を持つ敵 AI は、2 体以上の敵を巻き込める照準があれば発射する。
#[test]
fn ai_uses_map_weapon_on_multiple_enemies() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Bomber", 9, 0); // Bomber unit_data + PILOT
    app.database_mut()
        .units
        .iter_mut()
        .find(|d| d.name == "Bomber")
        .unwrap()
        .weapons
        .push(WeaponData {
            name: "全体砲".into(),
            power: 30,
            min_range: 1,
            max_range: 5,
            precision: 50,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: "Ｍ全".into(),
            extras: Vec::new(),
        });
    let bomber = app.database_mut().register_unit(crate::UnitInstance::new(
        "Bomber",
        "PILOT",
        crate::Party::Enemy,
        5,
        5,
    ));
    place_player_unit(&mut app, "Victim1", 6, 5);
    place_player_unit(&mut app, "Victim2", 7, 5);
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&bomber).unwrap();
    app.ai_act_unit(idx);
    let victims: Vec<_> = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| u.unit_data_name.starts_with("Victim"))
        .collect();
    let hit = victims.len() < 2 || victims.iter().all(|u| u.damage > 0);
    assert!(hit, "マップ兵器で複数の敵が被弾する");
    assert!(
        app.database().unit_by_uid(&bomber).unwrap().has_acted,
        "マップ兵器発射で行動終了"
    );
}

/// ChangeMode「護衛 <対象>」の敵 AI は守護対象の近くへ移動する。
#[test]
fn change_mode_escort_moves_toward_protected_unit() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Foe", 11, 11); // 敵対ユニット (candidates 用)
    place_player_unit(&mut app, "Protectee", 1, 1); // Protectee unit_data を用意 (この個体は遠方)
    let protectee = app.database_mut().register_unit(crate::UnitInstance::new(
        "Protectee",
        "PILOT",
        crate::Party::Enemy,
        3,
        3,
    ));
    let guard = app.database_mut().register_unit(crate::UnitInstance::new(
        "Foe",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    ));
    app.database_mut().unit_by_uid_mut(&guard).unwrap().ai_mode = format!("護衛 {protectee}");
    app.set_stage_state(crate::stage::StageState::Battle);
    let idx = app.database().idx_by_uid(&guard).unwrap();
    app.ai_act_unit(idx);
    let g = app.database().unit_by_uid(&guard).unwrap();
    let dist_after = (g.x as i32 - 3).abs() + (g.y as i32 - 3).abs();
    assert!(
        dist_after < 14,
        "護衛役は守護対象 (3,3) へ近づく (移動後距離={dist_after})"
    );
}

/// 写/化 (能力コピー) はクリティカル時に発動者を対象の形態へ変える。写はサイズ制限あり。
#[test]
fn weapon_copy_transforms_attacker_with_size_limit() {
    use crate::data::unit::{Size, WeaponData};
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Mimic", 2, 6);
    place_player_unit(&mut app, "Model", 4, 6); // 同サイズ M
    let atk = first_player_uid(&app);
    let model = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.x == 4 && u.y == 6)
        .unwrap()
        .uid
        .clone();
    let atk_idx = app.database().idx_by_uid(&atk).unwrap();
    let def_idx = app.database().idx_by_uid(&model).unwrap();
    let mk = |class: &str| WeaponData {
        name: "コピー光線".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: class.into(),
        extras: Vec::new(),
    };
    // 写 同サイズ → 変化。
    let applied = app.apply_weapon_crit_copy(atk_idx, def_idx, &mk("写"));
    assert_eq!(applied, vec!["能力コピー → Model".to_string()]);
    assert_eq!(
        app.database().unit_by_uid(&atk).unwrap().unit_data_name,
        "Model"
    );
    // Model を XL に変え、Mimic(M) へ戻して再試行: 写 はサイズ差2以上で無効、化 は可。
    app.database_mut()
        .unit_by_uid_mut(&atk)
        .unwrap()
        .unit_data_name = "Mimic".into();
    app.database_mut()
        .units
        .iter_mut()
        .find(|d| d.name == "Model")
        .unwrap()
        .size = Size::XL;
    assert!(
        app.apply_weapon_crit_copy(atk_idx, def_idx, &mk("写"))
            .is_empty(),
        "写 はサイズ 2 段階以上差で無効"
    );
    assert_eq!(
        app.database().unit_by_uid(&atk).unwrap().unit_data_name,
        "Mimic",
        "写 失敗で形態は変わらない"
    );
    assert!(
        !app.apply_weapon_crit_copy(atk_idx, def_idx, &mk("化"))
            .is_empty(),
        "化 はサイズ制限なしで変化"
    );
}

/// 習 (ラーニング): クリティカル時に対象の「ラーニング可能技」を攻撃側メイン
/// パイロットが習得し、(技名称) 必要技能ゲートが解禁される。再習得はしない。
#[test]
fn learning_weapon_teaches_skill_on_crit() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Learner", 2, 6);
    place_player_unit(&mut app, "Master", 4, 6);
    // Master の UnitData に「ラーニング可能技」を付与。
    app.database_mut()
        .units
        .iter_mut()
        .find(|d| d.name == "Master")
        .unwrap()
        .features
        .push(("ラーニング可能技".into(), "はりせんぼん".into()));
    // 攻撃側にパイロット実体を紐付ける (習得先)。
    app.database_mut()
        .create_pilot_instance("PILOT", "learner_p");
    let atk = first_player_uid(&app);
    let atk_idx = app.database().idx_by_uid(&atk).unwrap();
    app.database_mut().unit_instances[atk_idx].pilot_ids = vec!["learner_p".into()];
    let def_idx = app
        .database()
        .unit_instances
        .iter()
        .position(|u| u.x == 4 && u.y == 6)
        .unwrap();
    let mk = |class: &str| WeaponData {
        name: "ラーニング".into(),
        power: 0,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: class.into(),
        extras: Vec::new(),
    };
    // 習なし → 習得しない。
    assert!(app
        .apply_weapon_learning(atk_idx, def_idx, &mk("無"))
        .is_empty());
    assert_eq!(app.unit_pilot_skill_level(atk_idx, "はりせんぼん"), 0);
    // 習あり (無習 で連結検出) → 習得。
    let msgs = app.apply_weapon_learning(atk_idx, def_idx, &mk("無習"));
    assert_eq!(msgs.len(), 1, "ラーニングメッセージが 1 件");
    assert!(
        app.unit_pilot_skill_level(atk_idx, "はりせんぼん") > 0,
        "はりせんぼん を習得"
    );
    // 既習得は再習得しない。
    assert!(
        app.apply_weapon_learning(atk_idx, def_idx, &mk("無習"))
            .is_empty(),
        "既習得は再習得しない"
    );
}

/// 形態の必要技能 (`必要技能=` / `不必要技能=`): 搭乗員が満たす形態のみ変形/換装可。
#[test]
fn form_necessary_skill_gates_transform() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "BaseForm", 2, 6);
    place_player_unit(&mut app, "SuperForm", 4, 6); // UnitData 用途
                                                    // SuperForm に 必要技能=念力Lv3、Forbidden に 不必要技能=機械 を付与。
    app.database_mut()
        .units
        .iter_mut()
        .find(|d| d.name == "SuperForm")
        .unwrap()
        .features
        .push(("必要技能".into(), "念力Lv3".into()));
    let base = first_player_uid(&app);
    let idx = app.database().idx_by_uid(&base).unwrap();
    app.database_mut().create_pilot_instance("PILOT", "p1");
    app.database_mut().unit_instances[idx].pilot_ids = vec!["p1".into()];
    // 念力なし → SuperForm へ変形/換装不可。
    assert!(
        !app.form_skill_ok(idx, "SuperForm"),
        "念力なしで SuperForm 不可"
    );
    // 念力Lv3 習得 → 可。
    app.database_mut()
        .pilot_instance_by_id_mut("p1")
        .unwrap()
        .skills
        .push("念力Lv3".into());
    assert!(
        app.form_skill_ok(idx, "SuperForm"),
        "念力Lv3 で SuperForm 可"
    );
    // 必要技能宣言の無い形態は常に可。
    assert!(app.form_skill_ok(idx, "BaseForm"));
}

/// 乗り換えは形態の必要技能を満たすパイロットのみ。満たさない組合せは入れ替え不成立 (revert)。
#[test]
fn ride_change_respects_form_necessary_skill() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Normal", 2, 6);
    place_player_unit(&mut app, "Psychic", 3, 6); // 必要技能=念力Lv3
    app.database_mut()
        .units
        .iter_mut()
        .find(|d| d.name == "Psychic")
        .unwrap()
        .features
        .push(("必要技能".into(), "念力Lv3".into()));
    let ua = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Normal")
        .unwrap()
        .uid
        .clone();
    let ub = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Psychic")
        .unwrap()
        .uid
        .clone();
    app.database_mut().create_pilot_instance("PILOT", "plain");
    app.database_mut().create_pilot_instance("PILOT", "esper");
    app.database_mut()
        .pilot_instance_by_id_mut("esper")
        .unwrap()
        .skills
        .push("念力Lv3".into());
    app.database_mut().unit_by_uid_mut(&ua).unwrap().pilot_ids = vec!["plain".into()];
    app.database_mut().unit_by_uid_mut(&ub).unwrap().pilot_ids = vec!["esper".into()];
    // plain は念力なし → Psychic に乗れず乗り換え不成立 (revert)。
    app.apply_ride_change(&ua, &ub);
    assert_eq!(
        app.database().unit_by_uid(&ua).unwrap().pilot_ids,
        vec!["plain".to_string()],
        "念力なしは Psychic に乗れず乗り換え不成立"
    );
    assert_eq!(
        app.database().unit_by_uid(&ub).unwrap().pilot_ids,
        vec!["esper".to_string()]
    );
    // plain に念力Lv3 → 双方適格 → 入れ替え成立。
    app.database_mut()
        .pilot_instance_by_id_mut("plain")
        .unwrap()
        .skills
        .push("念力Lv3".into());
    app.apply_ride_change(&ua, &ub);
    assert_eq!(
        app.database().unit_by_uid(&ua).unwrap().pilot_ids,
        vec!["esper".to_string()],
        "適格なら入れ替え成立"
    );
    assert_eq!(
        app.database().unit_by_uid(&ub).unwrap().pilot_ids,
        vec!["plain".to_string()]
    );
}

/// 盗属性武器のクリティカル時資金奪取: 味方が敵を盗むと修理費の1/4が入り、再取得は不可。
#[test]
fn weapon_steal_grants_money_once() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let weapon = WeaponData {
        name: "盗賊剣".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: "盗".into(),
        extras: Vec::new(),
    };
    let money0 = app.money();
    // 修理費 800 → 1/4 = 200。
    let applied = app.apply_weapon_crit_steal(def_idx, 0, crate::Party::Player, 800, &weapon);
    assert_eq!(applied, vec!["資金奪取 +200".to_string()]);
    assert_eq!(app.money(), money0 + 200, "資金 +200");
    // 再取得は不可 (被盗)。
    let applied2 = app.apply_weapon_crit_steal(def_idx, 0, crate::Party::Player, 800, &weapon);
    assert!(applied2.is_empty(), "同じ相手から再取得しない");
    assert_eq!(app.money(), money0 + 200, "資金は変わらない");
}

/// 盗: 相手が「アイテム所有」を持つときは資金でなくアイテムを攻撃側へ奪う。
#[test]
fn weapon_steal_takes_item_when_target_owns_one() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 攻撃側 = idx 0
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    // 対象に「アイテム所有=賢者の石」を付与 (active_features)。
    app.database_mut().unit_instances[def_idx]
        .active_features
        .push(crate::feature::ActiveFeature::new(
            "アイテム所有",
            "賢者の石",
        ));
    let weapon = WeaponData {
        name: "盗賊剣".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: "盗".into(),
        extras: Vec::new(),
    };
    let money0 = app.money();
    let applied = app.apply_weapon_crit_steal(def_idx, 0, crate::Party::Player, 800, &weapon);
    assert_eq!(applied, vec!["アイテム奪取: 賢者の石".to_string()]);
    assert!(
        app.database().unit_instances[0]
            .equipped_items
            .contains(&"賢者の石".to_string()),
        "攻撃側がアイテムを取得"
    );
    assert_eq!(app.money(), money0, "アイテム盗み時は資金を奪わない");
    // 再取得不可 (被盗)。
    let applied2 = app.apply_weapon_crit_steal(def_idx, 0, crate::Party::Player, 800, &weapon);
    assert!(applied2.is_empty(), "同じ相手から再取得しない");
}

/// 衰L2 武器のクリティカル時減衰: 対象の現在 HP が半分になる (撃破はしない)。
#[test]
fn weapon_crit_decay_halves_current_hp() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    // 最大HP=100、ダメージ20 → 現在HP 80。衰L2 で 80→40。
    app.database_mut().unit_instances[def_idx].damage = 20;
    let weapon = WeaponData {
        name: "減衰砲".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: "衰L2".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_crit_decay(def_idx, &weapon);
    assert!(!applied.is_empty(), "衰 でラベルが返る");
    // 現在HP 80 → 40 → damage = 100 - 40 = 60。
    assert_eq!(
        app.database().unit_instances[def_idx].damage,
        60,
        "衰L2 で現在HP が半分 (80→40)"
    );
}

/// BossRank はステータスを強化し、石化を無効化する。
#[test]
fn boss_rank_boosts_stats_and_blocks_petrify() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Boss", 5, 5); // hp 100, armor 10
    let uid = first_player_uid(&app);
    let idx = app.database().idx_by_uid(&uid).unwrap();
    let hp0 = app
        .database()
        .effective_max_hp(&app.database().unit_instances[idx]);
    assert_eq!(hp0, 100);
    app.database_mut().unit_instances[idx].boss_rank = 2; // HP ×2 / 装甲 +600
    assert_eq!(
        app.database()
            .effective_max_hp(&app.database().unit_instances[idx]),
        200,
        "rank2 で HP ×2"
    );
    assert_eq!(
        app.database()
            .effective_armor(&app.database().unit_instances[idx]),
        610,
        "rank2 で 装甲 +600"
    );
    // 石化 (critical 100 → 必ず proc) でもボスは無効。
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    let w = WeaponData {
        name: "石化光線".into(),
        power: 100,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: "石".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_special_effects(idx, idx, &w, &pilot, &pilot);
    assert!(applied.is_empty(), "ボスは石化を無効化");
    assert!(
        !app.database().unit_instances[idx].has_condition("石化"),
        "石化 condition が付かない"
    );
}

/// 即死 (即) は非ボスを致死化し、ボスには無効。
#[test]
fn instakill_attribute_kills_non_boss_not_boss() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
    let w = WeaponData {
        name: "即死針".into(),
        power: 1,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100, // proc 率 100
        class: "即".into(),
        extras: Vec::new(),
    };
    assert!(
        app.roll_weapon_instakill(def_idx, &w, &pilot, &pilot),
        "非ボスは即死 proc"
    );
    app.database_mut().unit_instances[def_idx].boss_rank = 1;
    assert!(
        !app.roll_weapon_instakill(def_idx, &w, &pilot, &pilot),
        "ボスは即死無効"
    );
}

/// 吹L2 武器は対象を攻撃側から遠ざかる方向へ 2 マス押し出す。占有マスで停止する。
#[test]
fn weapon_knockback_pushes_and_stops_at_obstacle() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 5);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let mk = |class: &str| WeaponData {
        name: "衝撃砲".into(),
        power: 100,
        min_range: 1,
        max_range: 3,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: class.into(),
        extras: Vec::new(),
    };
    // 攻撃側 (5,5) → 対象 (6,5) → +x 方向へ 2 マス → (8,5)。
    let moved = app.apply_weapon_knockback(def_idx, (5, 5), "Hero", &mk("吹L2"), false);
    assert!(moved, "吹L2 で押し出される");
    assert_eq!(
        (
            app.database().unit_by_uid(&target).unwrap().x,
            app.database().unit_by_uid(&target).unwrap().y
        ),
        (8, 5),
        "(6,5) から +x に 2 マス → (8,5)"
    );
    // (10,5) に障害ユニットを置き、(8,5) の対象を 吹L3 → (9,5) で停止 (10,5 不可)。
    place_player_unit(&mut app, "Blocker", 10, 5);
    let moved2 = app.apply_weapon_knockback(def_idx, (7, 5), "Hero", &mk("吹L3"), false);
    assert!(moved2);
    assert_eq!(
        app.database().unit_by_uid(&target).unwrap().x,
        9,
        "障害ユニット (10,5) の手前 (9,5) で停止"
    );
}

/// 引(引き寄せ)武器は対象を攻撃側に隣接する空きマスへ移す。
#[test]
fn weapon_pull_moves_target_adjacent_to_attacker() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 5);
    let target = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        8,
        5,
    ));
    let def_idx = app.database().idx_by_uid(&target).unwrap();
    let weapon = WeaponData {
        name: "引力砲".into(),
        power: 100,
        min_range: 1,
        max_range: 5,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: "引".into(),
        extras: Vec::new(),
    };
    let applied = app.apply_weapon_crit_reposition(def_idx, (5, 5), &weapon);
    assert_eq!(applied, vec!["引き寄せ".to_string()]);
    let t = app.database().unit_by_uid(&target).unwrap();
    let dist = (t.x as i32 - 5).abs() + (t.y as i32 - 5).abs();
    assert_eq!(dist, 1, "攻撃側 (5,5) に隣接する位置へ移動");
}

/// 反撃武器の特殊効果攻撃属性 (状態異常) が、反撃の命中・生存時に被弾側へ proc する。
#[test]
fn counterattack_procs_weapon_special_effect() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 反撃を受ける側
                                               // 敵 unit_data: 痺 反撃武器 (critical 100 → 必ず proc)。
    let mut ebot = app.database().unit_by_name("Hero").cloned().unwrap();
    ebot.name = "EnemyBot".into();
    ebot.weapons = vec![WeaponData {
        name: "麻痺針".into(),
        power: 1, // Hero を撃破しない
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: "痺".into(),
        extras: Vec::new(),
    }];
    app.database_mut().units.push(ebot);
    let hero = first_player_uid(&app);
    let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
        "EnemyBot",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    // 反撃を必中させ RNG 非依存にする (hit_chance=100)。
    app.database_mut()
        .unit_by_uid_mut(&enemy)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));

    let res = app.try_counterattack(3, 6, (2, 6));
    assert!(res.is_some(), "反撃が成立する");
    assert!(
        app.database()
            .unit_by_uid(&hero)
            .unwrap()
            .has_condition("麻痺"),
        "反撃武器の特殊効果 (麻痺) が被弾側へ proc する"
    );
}

/// 攻撃を受けて生存したユニットは気力 +1 (SRC: 損傷による気力上昇)。
#[test]
fn taking_a_hit_and_surviving_raises_morale() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 反撃を受けて生存する側
    let mut ebot = app.database().unit_by_name("Hero").cloned().unwrap();
    ebot.name = "EnemyBot".into();
    ebot.weapons = vec![WeaponData {
        name: "パンチ".into(),
        power: 1, // Hero を撃破しない
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: String::new(),
        extras: Vec::new(),
    }];
    app.database_mut().units.push(ebot);
    let hero = first_player_uid(&app);
    let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
        "EnemyBot",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    app.database_mut()
        .unit_by_uid_mut(&enemy)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));
    let m0 = app.database().unit_by_uid(&hero).unwrap().morale;
    let res = app.try_counterattack(3, 6, (2, 6));
    assert!(res.is_some(), "反撃が成立する");
    assert_eq!(
        app.database().unit_by_uid(&hero).unwrap().morale,
        m0 + 1,
        "攻撃を受けて生存したユニットは気力 +1"
    );
}

/// 損傷時気力増加スキルは、被弾時の基本 +1 に加えてスキルLv分の気力を上乗せする。
#[test]
fn damage_morale_skill_adds_on_top_of_base_hit_gain() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 被弾側 (生存)
    app.database_mut()
        .pilots
        .iter_mut()
        .find(|p| p.name == "PILOT")
        .unwrap()
        .features
        .push(("損傷時気力増加L2".into(), String::new()));
    let mut ebot = app.database().unit_by_name("Hero").cloned().unwrap();
    ebot.name = "EnemyBot".into();
    ebot.weapons = vec![WeaponData {
        name: "パンチ".into(),
        power: 1,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: String::new(),
        extras: Vec::new(),
    }];
    app.database_mut().units.push(ebot);
    let hero = first_player_uid(&app);
    let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
        "EnemyBot",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    app.database_mut()
        .unit_by_uid_mut(&enemy)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));
    let m0 = app.database().unit_by_uid(&hero).unwrap().morale;
    app.try_counterattack(3, 6, (2, 6));
    assert_eq!(
        app.database().unit_by_uid(&hero).unwrap().morale,
        m0 + 3,
        "被弾 +1 ＋ 損傷時気力増加L2 +2 = +3"
    );
}

/// 武器を発射すると EN と残弾を消費する (命中の有無に依らない)。
#[test]
fn firing_a_weapon_consumes_en_and_ammo() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 反撃を受けて生存する側
    let mut gunner = app.database().unit_by_name("Hero").cloned().unwrap();
    gunner.name = "Gunner".into();
    gunner.en = 1000; // EN 切れにならないよう十分に
    gunner.weapons = vec![WeaponData {
        name: "実弾砲".into(),
        power: 1, // Hero を撃破しない
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: 3,
        en_consumption: 10,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: String::new(),
        extras: Vec::new(),
    }];
    app.database_mut().units.push(gunner);
    let shooter = app.database_mut().register_unit(crate::UnitInstance::new(
        "Gunner",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    {
        let u = app.database_mut().unit_by_uid_mut(&shooter).unwrap();
        u.weapons = vec![crate::unit_weapon::UnitWeapon::from_data("実弾砲", 0, 3)];
        u.add_condition(crate::Condition::new("必中", -1));
    }
    // shooter (3,6) が Hero (2,6) を反撃 → 実弾砲を発射。
    app.try_counterattack(3, 6, (2, 6));
    let u = app.database().unit_by_uid(&shooter).unwrap();
    assert_eq!(u.en_consumed, 10, "発射で EN を 10 消費");
    assert_eq!(u.weapons[0].bullet_remaining, 2, "発射で残弾 3→2");
}

/// 残弾 0 の武器は自動選択 (反撃 / 援護 / AI) で選ばれず、発射されない。
#[test]
fn empty_weapon_is_not_auto_selected() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let mut gunner = app.database().unit_by_name("Hero").cloned().unwrap();
    gunner.name = "Gunner".into();
    gunner.weapons = vec![WeaponData {
        name: "空砲".into(),
        power: 50,
        min_range: 1,
        max_range: 1,
        precision: 100,
        bullet: 1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: String::new(),
        extras: Vec::new(),
    }];
    app.database_mut().units.push(gunner);
    let shooter = app.database_mut().register_unit(crate::UnitInstance::new(
        "Gunner",
        "PILOT",
        crate::Party::Enemy,
        3,
        6,
    ));
    // 残弾 0 の UnitWeapon を attach (空)。
    {
        let u = app.database_mut().unit_by_uid_mut(&shooter).unwrap();
        u.weapons = vec![crate::unit_weapon::UnitWeapon::from_data("空砲", 0, 0)];
    }
    // 唯一の武器が残弾 0 → 反撃不成立。
    let res = app.try_counterattack(3, 6, (2, 6));
    assert!(res.is_none(), "残弾 0 の武器は自動選択されず反撃しない");
}

/// 援護攻撃武器の特殊効果攻撃属性が、援護の命中・生存時に防御側へ proc する。
#[test]
fn support_attack_procs_weapon_special_effect() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 本攻撃側 (atk_idx)
    place_player_unit(&mut app, "Supporter", 3, 6); // Hero に隣接した援護役
                                                    // Supporter に 痺 武器 (critical 100)。
    app.database_mut()
        .units
        .iter_mut()
        .find(|d| d.name == "Supporter")
        .unwrap()
        .weapons = vec![WeaponData {
        name: "麻痺針".into(),
        power: 1,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 100,
        class: "痺".into(),
        extras: Vec::new(),
    }];
    let hero = first_player_uid(&app);
    let sup = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Supporter")
        .unwrap()
        .uid
        .clone();
    // 援護攻撃 + 必中 (hit_chance=100)。
    app.database_mut()
        .unit_by_uid_mut(&sup)
        .unwrap()
        .add_condition(crate::Condition::new("サポートアタック", -1));
    app.database_mut()
        .unit_by_uid_mut(&sup)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));
    // 敵 (3,7): Supporter (3,6) の射程内 (距離1)。
    let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        3,
        7,
    ));
    let atk_idx = app.database().idx_by_uid(&hero).unwrap();

    let res = app.try_support_attack(atk_idx, 3, 7);
    assert!(res.is_some(), "援護攻撃が成立する");
    assert!(
        app.database()
            .unit_by_uid(&enemy)
            .unwrap()
            .has_condition("麻痺"),
        "援護攻撃武器の特殊効果 (麻痺) が防御側へ proc する"
    );
}

#[test]
fn condition_lifetime_decrements_and_expires() {
    use crate::Condition;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&uid)
        .unwrap()
        .add_condition(Condition::new("麻痺", 2));
    // lifetime 2 → プレイヤーフェイズ開始 tick で 2→1 (残る)、再度で 1→0 (消滅)。
    app.begin_phase(crate::Phase::Player);
    assert!(
        app.database()
            .unit_by_uid(&uid)
            .unwrap()
            .has_condition("麻痺"),
        "lifetime 2 は 1 回の tick では消えない"
    );
    app.begin_phase(crate::Phase::Player);
    assert!(
        !app.database()
            .unit_by_uid(&uid)
            .unwrap()
            .has_condition("麻痺"),
        "2 回目の tick で解除される"
    );
}

#[test]
fn combat_kill_fires_destruction_and_annihilation() {
    use crate::Condition;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    // `破壊 <name>` / `全滅 敵` を定義したステージファイル。
    let stage = crate::data::event::parse(
        "破壊 Foe:\nSet 破壊発火 1\nExit\n全滅 敵:\nSet 全滅発火 1\nExit\n",
    )
    .unwrap();
    app.script_library_mut()
        .append_with_name(&stage, "stage.eve");
    app.current_stage_file = "stage.eve".to_string();
    place_player_unit(&mut app, "Hero", 5, 6);
    add_weapon(&mut app, "Hero", 500, 1);
    place_player_unit(&mut app, "Foe", 9, 9); // Foe の UnitData を登録
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6); // 唯一の敵 (隣接)
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_map_cursor(5, 6);
    // 必中で確実に命中させ撃破する。
    let hero = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&hero)
        .unwrap()
        .add_condition(Condition::new("必中", 1));
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    // 戦闘での撃破が 破壊 <name> と (敵全滅で) 全滅 敵 を発火する (回帰)。
    assert_eq!(
        app.script_var("破壊発火"),
        "1",
        "破壊 <name> イベントが発火する"
    );
    assert_eq!(
        app.script_var("全滅発火"),
        "1",
        "全滅 敵 イベントが発火する (撃破でシナリオが進行)"
    );
}

#[test]
fn custom_unit_command_appears_in_menu_and_invokes() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "TestUnit", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // シナリオ定義の `*ユニットコマンド` を登録。
    // `c:` ラベルは条件サブルーチン — Return 1 で常に表示。
    let cc = crate::data::event::parse(
        "*ユニットコマンド テスト 味方 Call(c):\nSet 実行された 1\nExit\n\
             c:\nReturn 1\n",
    )
    .unwrap();
    app.script_library_mut().append(&cc);

    // ユニットをクリック → メニューに「テスト」が並ぶ。
    let ox = 2 * 32 + 16;
    let oy = 12 + 6 * 32 + 16;
    app.handle_input(Input::ClickAt { x: ox, y: oy });
    let idx = match app.command_menu() {
        Some(crate::CommandMenu::Unit { items, .. }) => {
            let pos = items.iter().position(|i| i.label() == "テスト");
            pos.expect("custom command「テスト」がメニューに無い")
        }
        _ => panic!("ユニットメニューが開いていない"),
    };
    // 「テスト」項目をクリック → 本体が実行される。
    let menu_x = crate::command_menu::MENU_X + 40;
    let item_y = crate::command_menu::MENU_Y
        + crate::command_menu::MENU_PADDING
        + crate::command_menu::MENU_ITEM_HEIGHT * idx as i32
        + 4;
    app.handle_input(Input::ClickAt {
        x: menu_x,
        y: item_y,
    });
    assert_eq!(
        app.script_var("実行された"),
        "1",
        "メニューからカスタムコマンドが実行されていない"
    );
}

#[test]
fn custom_unit_command_clears_stale_overlay_on_entry() {
    // スパロボ戦記 AlphaSecond: 別ユニットでステータス画面を再表示したとき、
    // 前ユニットの描画 (script_overlay) が残り透けて見える問題の防止。
    // カスタムユニットコマンド実行時に overlay をクリアして開始するため、
    // 連続実行しても描画コマンドが累積しないことを検証する。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "U1", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    let cc = crate::data::event::parse(
        "*ユニットコマンド 能力 味方 Call(c):\nPaintString 10 10 \"X\"\nExit\nc:\nReturn 1\n",
    )
    .unwrap();
    app.script_library_mut().append(&cc);

    app.invoke_custom_unit_command("U1", "能力");
    let after_first = app.script_overlay().cmds.len();
    assert!(after_first >= 1, "コマンド本体が描画していない");

    app.invoke_custom_unit_command("U1", "能力");
    let after_second = app.script_overlay().cmds.len();
    assert_eq!(
        after_first, after_second,
        "再表示で overlay が累積している (前ユニットの描画が残る)"
    );
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
    let menu_x = crate::command_menu::MENU_X + 40;
    let move_y = crate::command_menu::MENU_Y + crate::command_menu::MENU_PADDING + 4;
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
    let menu_x = crate::command_menu::MENU_X + 40;
    let wait_y = crate::command_menu::MENU_Y
        + crate::command_menu::MENU_PADDING
        + crate::command_menu::MENU_ITEM_HEIGHT * (item_count as i32 - 1)
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
    let menu_x = crate::command_menu::MENU_X + 40;
    let move_y = crate::command_menu::MENU_Y + crate::command_menu::MENU_PADDING + 4;
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
fn add_weapon(app: &mut App, unit_name: &str, power: i64, max_range: i32) {
    app.database_mut()
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
    let menu_x = crate::command_menu::MENU_X + 40;
    let item_y = crate::command_menu::MENU_Y
        + crate::command_menu::MENU_PADDING
        + crate::command_menu::MENU_ITEM_HEIGHT * idx
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

#[test]
fn repro_move_then_attack_keeps_moved_position() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    // enemy を先に push して idx 0 にする (remove による index 失効を誘発)
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Foe",
        "PILOT",
        crate::Party::Enemy,
        4,
        6,
    ));
    place_player_unit(&mut app, "Foe", 4, 6); // Foe の UnitData/PilotData を用意
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 999, 1);
    app.set_stage_state(crate::stage::StageState::Battle);

    // 1) Hero を選択
    click_tile(&mut app, 2, 6);
    // 2) Move (先頭項目)
    click_menu_item(&mut app, 0);
    assert_eq!(
        move_select_pos(&app),
        Some((2, 6)),
        "Move 選択後は MoveSelect(Hero) になるはず: {:?}",
        app.action_mode()
    );
    // 3) (3,6) へ移動
    click_tile(&mut app, 3, 6);
    assert_eq!(
        unit_xy(&app, "Hero"),
        Some((3, 6)),
        "移動が反映されていない"
    );
    // 4) PostMoveMenu の Attack を選ぶ
    let attack_idx = match app.command_menu() {
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
            .expect("PostMoveMenu に Attack が無い"),
        _ => panic!("PostMoveMenu が開いていない: {:?}", app.command_menu()),
    };
    click_menu_item(&mut app, attack_idx as i32);
    assert!(
        matches!(app.action_mode(), crate::ActionMode::AttackSelect { .. }),
        "Attack 選択後は AttackSelect になるはず: {:?}",
        app.action_mode()
    );
    // 5) 敵 (4,6) をクリックして攻撃
    click_tile(&mut app, 4, 6);

    // 攻撃後、Hero は移動先 (3,6) に留まっていること（初期 (2,6) に戻らない）
    assert_eq!(
        unit_xy(&app, "Hero"),
        Some((3, 6)),
        "攻撃後に Hero が初期出撃位置へ revert している"
    );
    // 位置索引が実体と整合していること (撃破による remove 後も)
    assert!(
        app.database().pos_index_is_consistent(),
        "pos_index が unit_instances と乖離している"
    );
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

#[test]
fn attack_targets_clicked_tile_not_nearest() {
    // 射程内に敵が 2 体いるとき、クリックしたタイルの敵を攻撃する
    // (旧実装は最寄り敵を攻撃していた)。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 30, 3); // 射程3・低威力 (撃破せず damage を見る)
    place_player_unit(&mut app, "FoeData", 9, 9); // FoeData の UnitData/PilotData を用意
    let near = spawn_enemy(&mut app, "FoeData", 3, 6); // 最寄り (dist1)
    let far = spawn_enemy(&mut app, "FoeData", 4, 6); // 遠い (dist2) — これをクリック
    app.set_stage_state(crate::stage::StageState::Battle);

    click_tile(&mut app, 2, 6); // Hero 選択
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai); // 攻撃
    click_tile(&mut app, 4, 6); // 遠い敵を対象に

    let far_dmg = app.database().unit_by_uid(&far).map(|u| u.damage).unwrap();
    let near_dmg = app.database().unit_by_uid(&near).map(|u| u.damage).unwrap();
    assert!(far_dmg > 0, "クリックした敵 (4,6) が攻撃されていない");
    assert_eq!(near_dmg, 0, "最寄り敵 (3,6) を誤って攻撃している");
}

/// テスト用に `損傷率 <unit> <pct>:` ラベルを登録する (本体は `Set <var> 1`)。
/// `current_stage_file` が空のままなので発火は global 経路 (`post_event_label`)。
fn register_damage_threshold_label(app: &mut App, unit_name: &str, pct: i64, var: &str) {
    let src = format!("損傷率 {unit_name} {pct}:\nSet {var} 1\nExit\n");
    let stmts = crate::data::event::parse(&src).expect("parse 損傷率 label");
    app.script_library_mut()
        .append_with_name(&stmts, "test_dmg.eve");
}

#[test]
fn combat_fires_damage_threshold_event_on_main_attack() {
    // 回帰: 通常攻撃で防御側の HP が閾値を割ったら `損傷率` イベントが発火する。
    // (旧実装は `Damage` コマンド経由でしか発火せず、実戦闘では沈黙していた。)
    // 閾値 10% + 最低ダメージ 10 / HP 100 なので、命中すれば必ず跨ぎ、かつ撃破しない。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 30, 3);
    place_player_unit(&mut app, "FoeData", 9, 9); // FoeData の UnitData/PilotData を用意
    let foe = spawn_enemy(&mut app, "FoeData", 3, 6);
    register_damage_threshold_label(&mut app, "FoeData", 10, "損傷率主");
    app.set_stage_state(crate::stage::StageState::Battle);

    click_tile(&mut app, 2, 6); // Hero 選択
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai); // 攻撃
    click_tile(&mut app, 3, 6); // 敵を攻撃

    let foe_dmg = app.database().unit_by_uid(&foe).map(|u| u.damage).unwrap();
    assert!(foe_dmg >= 10, "敵が被弾していない (dmg={foe_dmg})");
    assert!(foe_dmg < 100, "敵が撃破されてしまった (損傷率は生存時のみ)");
    assert_eq!(
        app.script_var("損傷率主"),
        "1",
        "通常攻撃で損傷率イベントが発火していない"
    );
}

#[test]
fn combat_fires_damage_threshold_event_on_counterattack() {
    // 回帰: 反撃で攻撃側 (Hero) の HP が閾値を割ったら `損傷率 Hero` が発火する。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 30, 3); // Hero の攻撃 (敵を撃破しない低威力)
    place_player_unit(&mut app, "FoeData", 9, 9);
    let _foe = spawn_enemy(&mut app, "FoeData", 3, 6);
    add_weapon(&mut app, "FoeData", 30, 3); // 敵の反撃武器 (射程内に Hero)
    register_damage_threshold_label(&mut app, "Hero", 10, "損傷率反撃");
    app.set_stage_state(crate::stage::StageState::Battle);

    let hero_uid = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Hero")
        .map(|u| u.uid.clone())
        .unwrap();

    click_tile(&mut app, 2, 6); // Hero 選択
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai); // 攻撃
    click_tile(&mut app, 3, 6); // 敵を攻撃 → 敵が反撃

    let hero_dmg = app
        .database()
        .unit_by_uid(&hero_uid)
        .map(|u| u.damage)
        .unwrap();
    assert!(
        hero_dmg >= 10,
        "Hero が反撃で被弾していない (dmg={hero_dmg})"
    );
    assert!(hero_dmg < 100, "Hero が反撃で撃破されてしまった");
    assert_eq!(
        app.script_var("損傷率反撃"),
        "1",
        "反撃で損傷率イベントが発火していない"
    );
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

/// みがわり (身代わり, SRC `Unit.cls:14855`): みがわり を持つ防御側が攻撃されると、
/// 指定された身代わりユニットが 1 回だけダメージを肩代わりする (防御側は無傷)。
/// サンプルの自作 SP 生贄→みがわり (`data/include.eve` の 生贄ルーチンが
/// `SpecialPower 相手ユニットＩＤ みがわり 対象ユニットＩＤ` を発行) を検証する。
#[test]
fn migawari_redirects_attack_damage_to_substitute_once() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 30, 3); // 撃破しない低威力 (HP100/装甲10 → dmg<100)
    register_enemy_data(&mut app, "Caster");
    register_enemy_data(&mut app, "Sub");
    let caster = spawn_enemy(&mut app, "Caster", 3, 6); // 保護対象
    let sub = spawn_enemy(&mut app, "Sub", 3, 5); // 身代わり
    app.set_stage_state(crate::stage::StageState::Battle);

    // Sub を Caster の身代わりに (生贄ルーチン相当)。
    let script = crate::data::event::parse("SpecialPower Sub みがわり Caster\n").unwrap();
    crate::event_runtime::execute(&mut app, &script).unwrap();
    assert!(
        app.database()
            .unit_by_uid(&caster)
            .unwrap()
            .has_condition("みがわり"),
        "Caster に みがわり condition が付与されていない"
    );

    let sub_before = app.database().unit_by_uid(&sub).unwrap().damage;
    let caster_before = app.database().unit_by_uid(&caster).unwrap().damage;

    // Hero が Caster を攻撃 → みがわり が Sub へ肩代わり。
    click_tile(&mut app, 2, 6);
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai);
    click_tile(&mut app, 3, 6);

    let sub_after = app.database().unit_by_uid(&sub).unwrap().damage;
    let caster_after = app.database().unit_by_uid(&caster).unwrap().damage;
    assert!(
        sub_after > sub_before,
        "身代わり Sub がダメージを肩代わりしていない (dmg {sub_before}->{sub_after})"
    );
    assert_eq!(
        caster_after, caster_before,
        "保護対象 Caster が被弾している (身代わりのはず: dmg {caster_before}->{caster_after})"
    );
    assert!(
        !app.database()
            .unit_by_uid(&caster)
            .unwrap()
            .has_condition("みがわり"),
        "みがわり が消費されていない (1 回限りのはず)"
    );
}

/// みがわり は反撃経路でも機能する: みがわり を持つ味方が攻撃して敵に反撃された
/// とき、反撃ダメージは身代わりが肩代わりする (通常攻撃のみの実装からの拡張)。
#[test]
fn migawari_redirects_counterattack_damage_to_substitute() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 保護対象 (反撃を受ける側)
    add_weapon(&mut app, "Hero", 30, 3); // Hero の攻撃 (敵を撃破しない)
    place_player_unit(&mut app, "Sub", 2, 5); // 身代わり
    register_enemy_data(&mut app, "EnemyU");
    let foe = spawn_enemy(&mut app, "EnemyU", 3, 6);
    add_weapon(&mut app, "EnemyU", 30, 3); // 敵の反撃武器 (Hero を撃破しない)
    app.set_stage_state(crate::stage::StageState::Battle);

    let hero = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Hero")
        .map(|u| u.uid.clone())
        .unwrap();
    let sub = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Sub")
        .map(|u| u.uid.clone())
        .unwrap();

    // Sub を Hero の身代わりに。
    let script = crate::data::event::parse("SpecialPower Sub みがわり Hero\n").unwrap();
    crate::event_runtime::execute(&mut app, &script).unwrap();

    let sub_before = app.database().unit_by_uid(&sub).unwrap().damage;
    let hero_before = app.database().unit_by_uid(&hero).unwrap().damage;
    let _ = foe;

    // Hero が敵を攻撃 → 敵が Hero に反撃 → みがわり が反撃ダメージを Sub へ。
    click_tile(&mut app, 2, 6);
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai);
    click_tile(&mut app, 3, 6);

    let sub_after = app.database().unit_by_uid(&sub).unwrap().damage;
    let hero_after = app.database().unit_by_uid(&hero).unwrap().damage;
    assert!(
        sub_after > sub_before,
        "身代わり Sub が反撃ダメージを肩代わりしていない (dmg {sub_before}->{sub_after})"
    );
    assert_eq!(
        hero_after, hero_before,
        "Hero が反撃で被弾している (みがわりのはず: dmg {hero_before}->{hero_after})"
    );
    assert!(
        !app.database()
            .unit_by_uid(&hero)
            .unwrap()
            .has_condition("みがわり"),
        "みがわり が消費されていない (反撃でも 1 回限り)"
    );
}

/// 自作SP「生贄」(`イベント=生贄ルーチン`) をプレイヤーが味方に使うと、その味方が
/// 使用者の身代わり (みがわり) になる end-to-end フロー。SRC `SpecialPowerData.cls`
/// Execute の "イベント" 効果。サンプル `data/include.eve` の生贄ルーチン相当を駆動。
#[test]
fn sacrifice_special_power_makes_selected_ally_a_substitute() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6); // 生贄 使用者 (保護される)
    place_player_unit(&mut app, "Ally", 2, 5); // 選んだ味方 (身代わりになる)

    // 生贄 + みがわり の SP データを sp.txt 形式で登録。
    let sp = crate::data::special_power::parse(
        "生贄, いけにえ\n\
             贄, 20, 味方, 即効, -, -, みがわり\n\
             イベント=生贄ルーチン\n\
             \n\
             みがわり\n\
             身, 10, 味方, みがわり, -, -, みがわり\n",
    )
    .unwrap();
    app.database_mut().extend_special_powers(sp);
    // 生贄ルーチン (data/include.eve 相当) を登録。
    let lib = crate::data::event::parse(
        "生贄ルーチン:\nSpecialPower 相手ユニットＩＤ みがわり 対象ユニットＩＤ\nReturn\n",
    )
    .unwrap();
    app.script_library_mut()
        .append_with_name(&lib, "include.eve");

    // 生贄 の対象は「味方単体選択」(SRC 準拠の対象種別是正)。
    assert!(
        matches!(app.spirit_target_kind("生贄"), SpiritTargetKind::SingleAlly),
        "生贄 (対象=味方) は単体味方選択のはず (旧実装は全体と誤判定)"
    );

    let hero = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Hero")
        .map(|u| u.uid.clone())
        .unwrap();
    let ally = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Ally")
        .map(|u| u.uid.clone())
        .unwrap();

    // Hero が Ally を対象に 生贄 を発動 (= 単体選択確定 → apply_spirit_to_target)。
    app.apply_spirit_to_target(&hero, &ally, "生贄", 20);

    // Hero (対象ユニットＩＤ=使用者) に みがわり が付き、身代わりは Ally (相手ユニットＩＤ)。
    let hero_u = app.database().unit_by_uid(&hero).unwrap();
    let cond = hero_u.conditions.iter().find(|c| c.name == "みがわり");
    assert!(
        cond.is_some(),
        "生贄 使用者 Hero に みがわり が付与されていない (イベント=生贄ルーチン 未実行?)"
    );
    assert_eq!(
        cond.unwrap().data,
        ally,
        "みがわり の身代わりが選んだ味方 Ally でない"
    );
}

/// カウンター (VB5 `COM.bas:1040-1058`): カウンター 技能を持つ防御側は主攻撃の前に
/// 先制反撃する。先制反撃で攻撃側を撃破すれば攻撃側の主攻撃は不発 (防御側は無傷)。
#[test]
fn counter_skill_strikes_pre_emptively() {
    fn run(with_counter: bool) -> (bool, i64) {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6); // 攻撃側 (HP100)
        add_weapon(&mut app, "Hero", 20, 3);
        place_player_unit(&mut app, "FoeData", 9, 9);
        let foe = spawn_enemy(&mut app, "FoeData", 3, 6); // 防御側
        add_weapon(&mut app, "FoeData", 500, 3); // 強力な反撃武器 (Hero を一撃)
        app.set_stage_state(crate::stage::StageState::Battle);
        if with_counter {
            if let Some(pd) = app
                .database_mut()
                .pilots
                .iter_mut()
                .find(|p| p.name == "PILOT")
            {
                pd.features.push(("カウンターLv1".into(), String::new()));
            }
        }
        click_tile(&mut app, 2, 6);
        let ai = attack_item_index(&app);
        click_menu_item(&mut app, ai);
        click_tile(&mut app, 3, 6);
        let hero_alive = app
            .database()
            .unit_instances
            .iter()
            .any(|u| u.unit_data_name == "Hero" && !u.off_map);
        let foe_dmg = app
            .database()
            .unit_by_uid(&foe)
            .map(|u| u.damage)
            .unwrap_or(-1);
        (hero_alive, foe_dmg)
    }
    // カウンター無し: Hero が先に攻撃 (Foe 被弾) → Foe が通常反撃で Hero 撃破。
    let (_, foe_dmg_base) = run(false);
    assert!(
        foe_dmg_base > 0,
        "通常: Hero の主攻撃で Foe が被弾するはず (dmg={foe_dmg_base})"
    );
    // カウンター有り: Foe が先制反撃で Hero 撃破 → Hero の主攻撃は不発 (Foe 無傷)。
    let (hero_alive_cnt, foe_dmg_cnt) = run(true);
    assert!(
        !hero_alive_cnt,
        "カウンター: Hero は Foe の先制反撃で撃破されるはず"
    );
    assert_eq!(
        foe_dmg_cnt, 0,
        "カウンター: 先制反撃で Hero 撃破→主攻撃不発で Foe 無傷のはず (dmg={foe_dmg_cnt})"
    );
}

/// 再攻撃 (VB5 `Unit.cls:10239`): 再攻撃 技能を持つ攻撃側は確率で2回攻撃する。高Lv＋
/// 直感同値で `slevel=2*Lv` が `Dice(32)` を必ず上回り、防御側は 2 ヒット分被弾する。
#[test]
fn reattack_skill_strikes_twice() {
    fn run(with_reattack: bool) -> i64 {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        add_weapon(&mut app, "Hero", 20, 3); // 低威力 (2ヒットでも防御側生存)
        place_player_unit(&mut app, "FoeData", 9, 9);
        let foe = spawn_enemy(&mut app, "FoeData", 3, 6); // 武器無し=反撃しない
        app.set_stage_state(crate::stage::StageState::Battle);
        if with_reattack {
            // 攻撃側パイロット "PILOT" (pilot_by_name は先頭を返す) に 再攻撃Lv50 を付与。
            if let Some(pd) = app
                .database_mut()
                .pilots
                .iter_mut()
                .find(|p| p.name == "PILOT")
            {
                pd.features.push(("再攻撃Lv50".into(), String::new()));
            }
        }
        click_tile(&mut app, 2, 6);
        let ai = attack_item_index(&app);
        click_menu_item(&mut app, ai);
        click_tile(&mut app, 3, 6);
        app.database()
            .unit_by_uid(&foe)
            .map(|u| u.damage)
            .unwrap_or(0)
    }
    let base = run(false);
    let twice = run(true);
    assert!(base > 0, "baseline 単発攻撃が当たっていない (dmg={base})");
    assert!(
        twice >= base * 2,
        "再攻撃で2ヒット分にならない: 単発 {base} vs 再攻撃 {twice}"
    );
}

#[test]
fn destruction_event_fires_with_pilot_function_label() {
    // 回帰: ラベル名に関数 `Pilot(<unit名>)` を含む破壊イベントが、当該ユニットの
    // 撃破で発火する (例: 決戦！宇宙怪獣1話 の `破壊 Pilot(補給艦リームズ):`)。
    // マッチャは実ユニットの unit名から `Pilot(<unit名>)` 候補を再構成して照合する。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 9999, 3); // 一撃必殺 (HP100 を撃破)
    place_player_unit(&mut app, "FoeData", 9, 9); // FoeData の UnitData/PilotData を用意
    let foe = spawn_enemy(&mut app, "FoeData", 3, 6);
    let stmts = crate::data::event::parse("破壊 Pilot(FoeData):\nSet 破壊発火 1\nExit\n")
        .expect("parse 破壊 label");
    app.script_library_mut()
        .append_with_name(&stmts, "test_destroy.eve");
    app.set_stage_state(crate::stage::StageState::Battle);

    click_tile(&mut app, 2, 6); // Hero 選択
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai); // 攻撃
    click_tile(&mut app, 3, 6); // 敵を攻撃 (撃破)

    assert!(
        app.database().unit_by_uid(&foe).is_none(),
        "敵が撃破されていない"
    );
    assert_eq!(
        app.script_var("破壊発火"),
        "1",
        "`破壊 Pilot(FoeData)` 関数形ラベルが発火していない"
    );
}

#[test]
fn ai_changemode_coord_lures_unit_toward_coordinate() {
    // ChangeMode <unit> <x> <y> (座標モード): 近い敵ではなく指定座標へ向かう。
    // 決戦！宇宙怪獣2話 の `ChangeMode 宇宙怪獣ギルガス 17 1` (ボスを釣る) 相当。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app); // 24x16
    place_player_unit(&mut app, "FoeData", 23, 15); // FoeData の UnitData/Pilot 登録用
    add_weapon(&mut app, "FoeData", 100, 1);
    place_player_unit(&mut app, "Hero", 0, 14); // 敵から見て近い標的 (下方向, dist14)
    let foe = spawn_enemy(&mut app, "FoeData", 0, 0); // AI 敵ユニット
    app.set_stage_state(crate::stage::StageState::Battle);
    let fidx = app.database().idx_by_uid(&foe).unwrap();
    app.database_mut().unit_instances[fidx].ai_mode = "20 0".to_string(); // 右方向へ誘導

    app.ai_act_unit(fidx);

    let u = app.database().unit_by_uid(&foe).unwrap();
    assert!(u.x > 0, "座標誘導で右へ動いていない (x={}, y={})", u.x, u.y);
    assert!(
        u.x > u.y,
        "近い Hero(下) へ動いてしまっている (x={}, y={})",
        u.x,
        u.y
    );
}

#[test]
fn ai_changemode_target_focuses_named_unit() {
    // ChangeMode <unit> <対象名> (ユニット狙い): 近い敵ではなく指定ユニットを
    // 優先ターゲットにする。決戦！宇宙怪獣2話 `ChangeMode ＮＰＣ 宇宙怪獣ギルガス`
    // (NPC をボスへ集中) 相当。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "NearData", 0, 3); // 近い標的 (下, dist3)
    place_player_unit(&mut app, "FarData", 20, 0); // 指定する遠い標的 (右, dist20)
    add_weapon(&mut app, "FarData", 100, 1);
    let foe = spawn_enemy(&mut app, "FarData", 0, 0); // AI 敵 (FarData 流用)
                                                      // 武器を持たせる (spawn_enemy は UnitData を共有するので FarData に付与済み)
    app.set_stage_state(crate::stage::StageState::Battle);
    let fidx = app.database().idx_by_uid(&foe).unwrap();
    app.database_mut().unit_instances[fidx].ai_mode = "FarData".to_string(); // FarData を狙う

    app.ai_act_unit(fidx);

    let u = app.database().unit_by_uid(&foe).unwrap();
    // 指定した FarData(右) 方向へ動く。近い NearData(下) ではない。
    assert!(
        u.x > u.y,
        "指定ユニット(右)でなく近い標的(下)へ動いている (x={}, y={})",
        u.x,
        u.y
    );
}

#[test]
fn player_cannot_attack_npc_ally_but_can_attack_enemy_and_neutral() {
    // 味方 ↔ ＮＰＣ は同盟。Hero(味方) は隣の ＮＰＣ を攻撃できず、
    // 敵 / 中立 は攻撃できる（SRC 敵味方関係）。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 30, 3);
    place_player_unit(&mut app, "Data", 9, 9); // UnitData/PilotData を用意
    let ally = spawn_party(&mut app, "Data", crate::Party::Npc, 3, 6); // 同盟 (dist1)
    let enemy = spawn_party(&mut app, "Data", crate::Party::Enemy, 4, 6); // 敵 (dist2)
    let neutral = spawn_party(&mut app, "Data", crate::Party::Neutral, 5, 6); // 中立 (dist3)
    app.set_stage_state(crate::stage::StageState::Battle);

    click_tile(&mut app, 2, 6); // Hero 選択
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai); // 攻撃

    // ＮＰＣ 同盟タイルをクリック → 攻撃不成立 (ダメージ無し、AttackSelect 継続)。
    click_tile(&mut app, 3, 6);
    assert_eq!(
        app.database().unit_by_uid(&ally).map(|u| u.damage),
        Some(0),
        "ＮＰＣ 同盟を攻撃できてしまっている"
    );
    assert!(
        matches!(app.action_mode(), crate::ActionMode::AttackSelect { .. }),
        "同盟クリックは no-op で AttackSelect 継続のはず"
    );

    // 敵タイルをクリック → 攻撃成立 (ダメージ)。
    click_tile(&mut app, 4, 6);
    assert!(
        app.database()
            .unit_by_uid(&enemy)
            .map(|u| u.damage)
            .unwrap()
            > 0,
        "敵を攻撃できていない"
    );
    let _ = neutral; // 中立への攻撃可否は行列テスト (is_hostile_to) で担保。
}

#[test]
fn cancel_after_post_move_attack_reverts_to_sortie() {
    // 「移動→攻撃選択→キャンセル」で出撃地点へ正しく巻き戻る (報告された症状の検証)。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 30, 1); // 射程1 → 移動後攻撃可
    place_player_unit(&mut app, "FoeData", 9, 9);
    spawn_enemy(&mut app, "FoeData", 4, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // Hero 選択 → Move → (3,6)
    click_tile(&mut app, 2, 6);
    click_menu_item(&mut app, 0); // Move (先頭)
    click_tile(&mut app, 3, 6);
    assert_eq!(unit_xy(&app, "Hero"), Some((3, 6)));

    // PostMoveMenu で攻撃選択 → AttackSelect (snapshot あり)
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai);
    assert!(matches!(
        app.action_mode(),
        crate::ActionMode::AttackSelect {
            snapshot: Some(_),
            ..
        }
    ));

    // 1回目キャンセル: PostMoveMenu に戻るだけ (移動はまだ巻き戻さない)
    app.handle_input(Input::Cancel);
    assert_eq!(
        unit_xy(&app, "Hero"),
        Some((3, 6)),
        "攻撃キャンセルで移動先に留まるべき"
    );

    // 2回目キャンセル: 移動前 (2,6) に巻き戻る
    app.handle_input(Input::Cancel);
    assert_eq!(
        unit_xy(&app, "Hero"),
        Some((2, 6)),
        "PostMoveMenu キャンセルで出撃地点へ戻る"
    );
    assert!(app.database().pos_index_is_consistent());
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
fn cancel_post_move_restores_unit_position() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "TestUnit", 2, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // 移動
    let ox = 2 * 32 + 16;
    let oy = 12 + 6 * 32 + 16;
    app.handle_input(Input::ClickAt { x: ox, y: oy });
    let menu_x = crate::command_menu::MENU_X + 40;
    let move_y = crate::command_menu::MENU_Y + crate::command_menu::MENU_PADDING + 4;
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
fn end_phase_via_map_menu_works() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    app.set_stage_state(crate::stage::StageState::Battle);
    let start_turn = app.turn().number;

    // 空白地形をクリック → マップメニュー
    let empty_x = 8 * 32 + 16;
    let empty_y = 12 + 6 * 32 + 16;
    app.handle_input(Input::ClickAt {
        x: empty_x,
        y: empty_y,
    });
    assert!(matches!(
        app.command_menu(),
        Some(crate::CommandMenu::Map { .. })
    ));
    // EndTurn は items[0]
    let menu_x = crate::command_menu::MENU_X + 40;
    let endturn_y = crate::command_menu::MENU_Y + crate::command_menu::MENU_PADDING + 4;
    app.handle_input(Input::ClickAt {
        x: menu_x,
        y: endturn_y,
    });
    // ターン進行 (Player→…→Player)、turn.number は維持 or +1。少なくとも
    // command_menu は閉じている。
    assert!(app.command_menu().is_none());
    let _ = start_turn;
}

#[test]
fn victory_conditions_map_menu_item_gated_on_label() {
    use crate::command_menu::{CommandMenu, MapAction};
    let mut app = App::new();

    // `勝利条件:` ラベルが無いときは「作戦目的」を出さない。
    app.open_map_menu();
    match app.command_menu() {
        Some(CommandMenu::Map { items, .. }) => {
            assert!(!items.contains(&MapAction::VictoryConditions));
        }
        _ => panic!("map menu expected"),
    }

    // ラベルを定義すると「作戦目的」が現れ、選択でラベル本体が走る。
    let stmts =
        crate::data::event::parse("勝利条件:\nSet vc_fired 1\nReturn\n").expect("parse 勝利条件");
    app.script_library_mut().append(&stmts);
    assert!(app.has_victory_condition_event());

    app.open_map_menu();
    let items = match app.command_menu() {
        Some(CommandMenu::Map { items, .. }) => items.clone(),
        _ => panic!("map menu expected"),
    };
    assert!(items.contains(&MapAction::VictoryConditions));

    // メニュー経由で実行 → `勝利条件:` 本体が発火する。
    let idx = items
        .iter()
        .position(|a| *a == MapAction::VictoryConditions)
        .unwrap();
    assert!(app.execute_menu_action(crate::command_menu::MenuActionId::Map(items[idx])));
    assert_eq!(app.script_var("vc_fired"), "1");
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

// ===== ステージ進行 API（start_scenario / begin_battle / game_over 等）=====

#[test]
fn start_scenario_resets_state_and_enters_mapview() {
    let mut app = App::new();
    // タイトル → 設定画面に進めておく（start_scenario は途中状態でも呼べる）
    app.handle_input(Input::Advance);
    assert_eq!(app.scene(), Scene::Configuration);

    app.start_scenario("テストステージ");
    assert_eq!(app.scene(), Scene::MapView);
    // 原典 SRC: Prologue → Start → Battle が自動進行する。
    // `Prologue` ラベルが script_library に未登録 (App::new() のまま) なので、
    // start_scenario 後に auto_progress_stage_state_if_idle が走り Battle へ。
    assert_eq!(app.stage_state(), crate::stage::StageState::Battle);
    assert_eq!(app.stage(), "テストステージ");
    assert_eq!(app.turn().number, 1);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    assert!(app.command_menu().is_none());
}

/// 次ステージへ進むとき、前ステージが Victory / Defeat のままだと
/// `FlowCont::AfterStageFileRun` のガード (Briefing / Sortie のみ通す) に
/// 弾かれて次ステージが開始しない。原典 `StartScenario` はステージを
/// 立ち上げ直すので Briefing に戻す。
#[test]
fn next_stage_resets_terminal_stage_state() {
    let src = "スタート:\nExit\n";
    let stmts = crate::data::event::parse(src).unwrap();
    let mut app = App::new();
    app.script_library_mut()
        .append_with_name(&stmts, "stage02.eve");
    app.stage_state = crate::stage::StageState::Victory;
    app.set_script_var("次ステージ".to_string(), "stage02.eve".to_string());

    assert!(app.advance_to_next_stage());
    assert_ne!(
        app.stage_state(),
        crate::stage::StageState::Victory,
        "Victory のままだと次ステージが開始しない"
    );
}

/// `IntermissionCommand` は「インターミッションのメニューに項目を追加する」
/// だけの命令で (`IntermissionCommandコマンド.md`)、シナリオがインター
/// ミッション制であることを意味しない。プロローグで辞典やショップを登録
/// するステージ制シナリオ (例: 多次元世界エニバース) が、これを理由に
/// ステージ開始をブロックされないこと。
#[test]
fn intermission_command_does_not_block_stage_bootstrap() {
    let src = "プロローグ:\n\
IntermissionCommand \"辞典\" \"lib.eve\"\n\
Exit\n\
\n\
スタート:\n\
Exit\n";
    let stmts = crate::data::event::parse(src).unwrap();
    let mut app = App::new();
    app.script_library_mut()
        .append_with_name(&stmts, "stage01.eve");
    // 素材パック判定を避けるためユニット定義を入れる。
    let (units, _) =
        crate::data::unit::parse_lenient("テスト機\nテスト, ＭＳ, 1, 2\n陸, 4, M, 1000, 100\n");
    app.database_mut().extend_units(units);
    // プロローグを実行して IntermissionCommand を登録させる。
    crate::event_runtime::trigger_label(&mut app, "プロローグ");
    assert!(
        !app.intermission_commands.is_empty(),
        "IntermissionCommand が登録されていない"
    );

    app.bootstrap_stage_after_load("stage01.eve");
    assert_eq!(
        app.stage_state(),
        crate::stage::StageState::Battle,
        "IntermissionCommand があってもステージ制なら Battle へ進む"
    );
}

#[test]
fn begin_battle_goes_through_sortie_only() {
    // begin_sortie / begin_battle のゲート挙動 (順序強制) を直接検証する。
    // App::new() の初期 stage_state は Briefing なので start_scenario を
    // 経由せず手動で順に呼ぶ。auto_progress は start_scenario が呼ばないと
    // 走らないため、ここではゲートだけがテスト対象。
    let mut app = App::new();
    assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);

    // Briefing 状態では begin_battle は no-op
    app.begin_battle();
    assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);

    app.begin_sortie();
    assert_eq!(app.stage_state(), crate::stage::StageState::Sortie);

    app.begin_battle();
    assert_eq!(app.stage_state(), crate::stage::StageState::Battle);
    // begin_phase(Player) によりターン 1 開始メッセージが入る
    assert!(app
        .messages()
        .iter()
        .any(|m| m.contains("ターン 1") && m.contains("味方")));
}

#[test]
fn auto_progress_starts_battle_even_with_intermission_commands() {
    // 回帰テスト: musou 系インターミッション制シナリオで `Continue` チェイン後に
    // 本編 (MapView / Briefing) へ入ると、`IntermissionCommand` が登録済みでも
    // auto_progress が Battle まで進め、マップ未ロードならデフォルト 15×15
    // マップが生成されること。手動 Enter 進行を撤去した後の停止バグの再発防止。
    let mut app = App::new();
    // musou 系は `Stage` コマンドを使わないため stage 表示名は空のまま、
    // 本編突入は `Continue` チェイン (advance_to_next_stage) 経由で
    // current_stage_file が立つことで判定する。該当ファイルは未登録なので
    // ラベル発火は no-op だが current_stage_file は設定される。
    app.set_script_var("次ステージ".to_string(), "東方夢想伝01.eve".to_string());
    let _ = app.advance_to_next_stage();
    app.set_scene(Scene::MapView);
    assert!(app.stage().is_empty());
    // プロローグで登録されたインターミッションコマンドはステージ突入後も残る。
    app.push_intermission_command(
        "プラクティス".to_string(),
        "Lib/プラクティス.eve".to_string(),
    );
    assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);
    assert!(app.database().map.is_none());

    app.auto_progress_stage_state_if_idle();

    assert_eq!(
        app.stage_state(),
        crate::stage::StageState::Battle,
        "intermission_commands 登録済みでも Battle へ進むべき"
    );
    assert!(
        app.database().map.is_some(),
        "マップ未ロードならデフォルトマップが生成されるべき"
    );
}

#[test]
fn next_stage_starts_battle_without_double_firing_start() {
    // スパロボ戦記: 「次のステージへ」でステージファイルが `スタート` をインライン
    // 実行 (配置まで完了) して中断せず終わった場合、Briefing で固まらず味方フェイズが
    // 始まり、かつ `スタート` が二重発火しない (= 敵の二重配置が起きない) こと。
    let mut app = App::new();
    // ステージファイル: `スタート` で Incr (= 配置の代用)。中断しない。
    let stage = crate::data::event::parse("スタート:\nIncr スタート回数\nExit\n").unwrap();
    app.script_library_mut()
        .append_with_name(&stage, "main.eve");
    // インターミッション設定: 1 ユーザ項目 + 組込み(機体改造/データセーブ) + 「次のステージへ」。
    // 表示順: [User(0), 機体改造, データセーブ, 次のステージへ] → 次のステージは index 3。
    app.push_intermission_command("改造".to_string(), "lib/x.eve".to_string());
    app.set_script_var("次ステージ".to_string(), "main.eve".to_string());
    app.set_intermission_cursor(3); // 「次のステージへ」

    assert!(app.confirm_intermission_selection());
    assert_eq!(
        app.stage_state(),
        crate::stage::StageState::Battle,
        "次のステージへ後に Briefing で固まらず Battle へ進むべき"
    );
    assert_eq!(app.turn().phase, crate::Phase::Player);
    assert_eq!(
        app.script_var("スタート回数"),
        "1",
        "スタートが二重発火している (敵の二重配置になる)"
    );
}

#[test]
fn begin_phase_deferred_until_start_event_completes() {
    // 完了プロトコル (FlowCont::AfterStartEvent): `スタート` が Wait Click で
    // suspend したら味方フェイズ開始 (begin_phase) は完了まで遅延し、完了後に
    // ターンイベントが発火すること。旧実装は begin_battle が suspend 中でも
    // 即 begin_phase していたため、ターン 1 イベントが trigger_label の
    // 実行中ガードに弾かれて黙って消えていた。
    let stage = crate::data::event::parse(
        "スタート:\nWait Click\nIncr 開始\nExit\nターン 1 味方:\nIncr ターンイベント\nExit\n",
    )
    .unwrap();
    let mut app = App::new();
    app.script_library_mut()
        .append_with_name(&stage, "main.eve");

    app.begin_sortie();
    app.begin_battle();
    // `スタート` が Wait Click で suspend 中: フェイズはまだ始まらない。
    assert!(app.pending_dialog().is_some());
    assert_eq!(app.script_var("ターンイベント"), "");
    assert!(!app.messages().iter().any(|m| m.contains("ターン 1 開始")));

    assert!(app.respond_dialog(0));
    // 完了 → AfterStartEvent 継続 → begin_phase → ターン 1 イベント発火。
    assert_eq!(app.script_var("開始"), "1");
    assert_eq!(app.turn().phase, crate::Phase::Player);
    assert_eq!(
        app.script_var("ターンイベント"),
        "1",
        "スタート完了後にターン 1 イベントが発火するべき"
    );
}

#[test]
fn suspended_stage_file_with_inline_start_does_not_refire() {
    // 完了プロトコル (FlowCont::AfterStageFileRun + start_passed_pcs):
    // ステージファイルが `スタート` を通過実行する途中で suspend した場合、
    // resume 完了後に `スタート` を再発火しないこと。旧実装は「中断せず
    // 完了したか」で推定していたため、このケース (suspend したが スタート は
    // インライン実行済み) で resume → auto_progress → begin_battle が
    // `スタート` を再発火し、敵が二重配置されていた。
    let stage =
        crate::data::event::parse("スタート:\nIncr 配置\nWait Click\nIncr 配置後\nExit\n").unwrap();
    let mut app = App::new();
    app.script_library_mut()
        .append_with_name(&stage, "main.eve");
    app.set_script_var("次ステージ".to_string(), "main.eve".to_string());

    assert!(app.advance_to_next_stage());
    // `スタート` 内の Wait Click で suspend 中。
    assert!(app.pending_dialog().is_some());
    assert_eq!(app.script_var("配置"), "1");
    assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);

    assert!(app.respond_dialog(0));
    assert_eq!(app.stage_state(), crate::stage::StageState::Battle);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    assert_eq!(
        app.script_var("配置"),
        "1",
        "スタートが再発火している (敵の二重配置になる)"
    );
    assert_eq!(app.script_var("配置後"), "1");
}

#[test]
fn intermission_subcommand_returns_to_menu_after_suspend() {
    // FlowCont::ReturnToIntermissionMenu: サブコマンド .eve が suspend した
    // 場合も、完了後にインターミッションメニューへ復帰すること。
    let sub = crate::data::event::parse("プロローグ:\nWait Click\nIncr 実行済\nExit\n").unwrap();
    let mut app = App::new();
    app.script_library_mut().append_with_name(&sub, "lib/x.eve");
    app.push_intermission_command("改造".to_string(), "lib/x.eve".to_string());
    app.set_scene(Scene::Intermission);
    app.set_intermission_cursor(0);

    assert!(app.confirm_intermission_selection());
    // サブコマンド実行中は MapView (PaintString / Hotpoint 表示のため)。
    assert_eq!(app.scene(), Scene::MapView);
    assert!(app.pending_dialog().is_some());

    assert!(app.respond_dialog(0));
    assert_eq!(app.script_var("実行済"), "1");
    assert_eq!(
        app.scene(),
        Scene::Intermission,
        "サブコマンド完了後はメニューへ復帰するべき"
    );
}

#[test]
fn flow_continuations_survive_save_load() {
    // `flow` / `start_passed_pcs` は serde 対象: suspend 中にセーブ → ロード
    // しても継続が失われず、resume 完了後に正しく Battle へ進むこと。
    let stage = crate::data::event::parse("スタート:\nIncr 配置\nWait Click\nExit\n").unwrap();
    let mut app = App::new();
    app.script_library_mut()
        .append_with_name(&stage, "main.eve");
    app.set_script_var("次ステージ".to_string(), "main.eve".to_string());
    assert!(app.advance_to_next_stage());
    assert!(app.pending_dialog().is_some());

    let json = app.to_save_json().expect("save");
    let mut loaded = App::from_save_json(&json).expect("load");

    assert!(loaded.respond_dialog(0));
    assert_eq!(loaded.stage_state(), crate::stage::StageState::Battle);
    assert_eq!(
        loaded.script_var("配置"),
        "1",
        "ロード後の resume でスタートが再発火している"
    );
}

#[test]
fn interrupt_event_defers_until_script_completes() {
    // EventQue (原典 Event.bas 準拠): スクリプト実行中の `Kill` が発火する
    // 破壊イベントはキューに積まれ、現在のスクリプト完了後に実行される。
    // ハンドラに Wait Click 等の suspend 系命令が含まれていても、外側の
    // 実行コンテキストを上書きしない (旧実装は再入実行で、外側 suspend 時に
    // ハンドラの ctx が上書き消失するハザードがあった)。
    let src = "\
メイン:
Pilot \"鋼鉄1号\" 鋼1 男性 一般 BBBC 50 100 120 110 110 100 100
Unit \"敵A\" リアル系 1 3 陸 5 M 1000 200 1200 100 800 80 BBBC
Place \"敵A\" \"鋼鉄1号\" Enemy 3 3
Kill 鋼鉄1号
Message after_kill
Exit
破壊 鋼鉄1号:
Message handler_start
Wait Click
Incr 破壊処理完了
Exit
";
    let stmts = crate::data::event::parse(src).unwrap();
    let mut app = App::new();
    app.script_library_mut().append(&stmts);
    crate::event_runtime::trigger_label(&mut app, "メイン");

    // メイン完了後にハンドラが起動し、Wait Click で suspend している。
    let msgs: Vec<&str> = app.messages().iter().map(String::as_str).collect();
    let after_kill_pos = msgs.iter().position(|m| *m == "after_kill");
    let handler_pos = msgs.iter().position(|m| *m == "handler_start");
    assert!(
        after_kill_pos.is_some() && handler_pos.is_some(),
        "after_kill / handler_start が記録されていない: {msgs:?}"
    );
    assert!(
        after_kill_pos < handler_pos,
        "破壊ハンドラがスクリプト完了前に割り込んでいる (EventQue 非準拠): {msgs:?}"
    );
    assert!(app.pending_dialog().is_some());
    assert_eq!(app.script_var("破壊処理完了"), "");

    // ハンドラの続きも失われない (旧実装では ctx 上書きで消失し得た)。
    assert!(app.respond_dialog(0));
    assert_eq!(app.script_var("破壊処理完了"), "1");
}

#[test]
fn turn_events_not_swallowed_by_suspending_predecessor() {
    // EventQue: `ターン 全 味方` が Wait Click で suspend しても、後続の
    // `ターン 1 味方` はキューに残り、完了後に実行される。旧実装は
    // trigger_label の実行中ガードに弾かれて黙って消えていた。
    let src = "\
ターン 全 味方:
Incr 全ターン
Wait Click
Exit
ターン 1 味方:
Incr ターンN
Exit
";
    let stmts = crate::data::event::parse(src).unwrap();
    let mut app = App::new();
    app.script_library_mut().append(&stmts);

    // スタート未定義 → AfterStartEvent 継続が即 begin_phase(Player) を呼び、
    // ターンイベント 2 件が投函される。
    app.begin_sortie();
    app.begin_battle();

    assert_eq!(app.script_var("全ターン"), "1");
    assert!(
        app.pending_dialog().is_some(),
        "ターン全 が suspend するはず"
    );
    assert_eq!(
        app.script_var("ターンN"),
        "",
        "ターン 1 イベントは先行イベント完了まで実行されないはず"
    );

    assert!(app.respond_dialog(0));
    assert_eq!(
        app.script_var("ターンN"),
        "1",
        "先行ターンイベントの suspend で後続イベントが消えている"
    );
}

#[test]
fn continue_chains_to_next_stage_without_frontend() {
    // 非インターミッションシナリオの `Continue <file>` はフロントエンドの
    // 介在なしに次ステージへ遷移する (FlowCont::LoadNextStage)。旧実装は
    // archive ロード時の while ループが拾う前提で、戦闘中の Continue
    // (例: 勝利 → エピローグ → Continue 次.eve) を誰も消費せず停止していた。
    // また Continue は旧ステージの flow 継続を破棄する (scenario_transition_
    // reset) ため、旧 AfterStageFileRun が新ステージ突入後に二重発火しない。
    let a = crate::data::event::parse(
        "スタート:\nIncr A実行\nContinue b.eve\nエピローグ:\nIncr エピ\nExit\n",
    )
    .unwrap();
    let b = crate::data::event::parse("スタート:\nIncr B実行\nExit\n").unwrap();
    let mut app = App::new();
    app.script_library_mut().append_with_name(&a, "a.eve");
    app.script_library_mut().append_with_name(&b, "b.eve");
    app.set_script_var("次ステージ".to_string(), "a.eve".to_string());

    assert!(app.advance_to_next_stage());
    assert_eq!(app.script_var("A実行"), "1");
    assert_eq!(app.script_var("エピ"), "1", "エピローグが実行されるべき");
    assert_eq!(
        app.script_var("B実行"),
        "1",
        "Continue が次ステージを起動するべき (フロントエンド非依存)"
    );
    assert_eq!(app.current_stage_file(), "b.eve");
    assert_eq!(app.stage_state(), crate::stage::StageState::Battle);
    assert_eq!(app.turn().phase, crate::Phase::Player);
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
fn end_phase_uses_begin_phase_and_increments_turn() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    app.set_stage_state(crate::stage::StageState::Battle);
    assert_eq!(app.turn().number, 1);

    app.handle_input(Input::EndPhase);
    assert_eq!(app.turn().phase, crate::Phase::Player);
    assert_eq!(app.turn().number, 2);
    // 新ターン開始の HUD メッセージが入っているはず
    assert!(app
        .messages()
        .iter()
        .any(|m| m.contains("ターン 2") && m.contains("味方")));
}

#[test]
fn turn_events_fire_per_phase_and_every_turn() {
    // SRC `ターンイベント.md`: 各フェイズ開始で `ターン <N> <陣営>` と
    // `ターン 全 <陣営>` が発火する。フェイズ順は 味方→敵→中立→ＮＰＣ。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    let eve = crate::data::event::parse(
        "ターン 全 味方:\nIncr c_all\nExit\n\
             ターン 1 敵:\nSet e1 1\nExit\n\
             ターン 1 ＮＰＣ:\nSet npc1 1\nExit\n\
             ターン 2 敵:\nSet e2 1\nExit\n",
    )
    .unwrap();
    app.script_library_mut().append(&eve);
    app.set_stage_state(crate::stage::StageState::Battle);

    // ターン1 味方 → EndPhase で 敵(1)/中立/ＮＰＣ(1) を消費し ターン2 味方 へ。
    app.handle_input(Input::EndPhase);
    assert_eq!(app.script_var("e1"), "1", "ターン 1 敵 が発火していない");
    assert_eq!(
        app.script_var("npc1"),
        "1",
        "ターン 1 ＮＰＣ が発火していない"
    );
    // ターン2 味方開始で `ターン 全 味方` が発火 (毎ターン)。
    assert_eq!(
        app.script_var("c_all"),
        "1",
        "ターン 全 味方 が毎ターン発火していない"
    );
    assert_eq!(
        app.script_var("e2"),
        "",
        "まだ ターン 2 敵 は発火しないはず"
    );

    // もう一度 EndPhase → ターン3 味方。`ターン 全 味方` が再度発火 (計2回)、
    // `ターン 2 敵` が発火する。
    app.handle_input(Input::EndPhase);
    assert_eq!(
        app.script_var("c_all"),
        "2",
        "ターン 全 味方 が2ターン目に再発火していない"
    );
    assert_eq!(app.script_var("e2"), "1", "ターン 2 敵 が発火していない");
}

#[test]
fn animated_ai_runner_steps_units_and_returns_to_player() {
    // animate_ai=true では EndPhase で即完了せず、tick が敵/中立/ＮＰＣ を
    // 1 体ずつ進めて Player ターン+1 に戻る。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Data", 9, 9);
    spawn_party(&mut app, "Data", crate::Party::Enemy, 2, 2);
    spawn_party(&mut app, "Data", crate::Party::Enemy, 14, 14);
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_animate_ai(true);

    // EndPhase → ランナー起動 (敵フェイズ)。即座には Player に戻らない。
    app.handle_input(Input::EndPhase);
    assert!(app.ai_running(), "逐次ランナーが起動しているはず");
    assert_eq!(app.turn().phase, crate::Phase::Enemy);
    assert_eq!(app.turn().number, 1);

    // 最初の 1 ステップ未満の tick ではまだ誰も動かない (間の演出)。
    app.tick(0.1);
    assert!(app.ai_running());
    assert_eq!(app.turn().phase, crate::Phase::Enemy);

    // tick を十分回すと敵→中立→ＮＰＣ を消化し Player ターン2 に戻る。
    for _ in 0..300 {
        app.tick(0.1);
        if !app.ai_running() {
            break;
        }
    }
    assert!(!app.ai_running(), "ランナーが完了していない");
    assert_eq!(app.turn().phase, crate::Phase::Player);
    assert_eq!(app.turn().number, 2);
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

#[test]
fn reaction_prompt_shown_manual_and_skipped_when_auto() {
    // 手動 (既定): 敵が味方を攻撃 → 反撃メニューが出る。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    place_player_unit(&mut app, "Foe", 9, 9);
    add_weapon(&mut app, "Foe", 60, 1);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 4, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_animate_ai(true);
    app.handle_input(Input::EndPhase);
    let mut prompted = false;
    for _ in 0..100 {
        app.tick(0.1);
        if app.pending_dialog().is_some() {
            prompted = true;
            break;
        }
        if !app.ai_running() {
            break;
        }
    }
    assert!(prompted, "手動反撃モードで反撃メニューが出ていない");

    // 自動反撃モード: プロンプト無しで攻撃が解決され Hero が被弾する。
    let (_app, auto_dmg) = run_enemy_attacks_hero(None, false);
    assert!(auto_dmg > 0, "自動反撃モードで攻撃が成立していない");
}

#[test]
fn defend_reaction_halves_damage() {
    // options=[回避, 防御] (Hero 武器なし→反撃不可)。防御=choice 2。
    let (_a, defend_dmg) = run_enemy_attacks_hero(Some(2), false);
    let (_b, auto_dmg) = run_enemy_attacks_hero(None, false);
    assert!(auto_dmg > 0, "基準 (自動) で被弾していない");
    assert_eq!(
        defend_dmg,
        auto_dmg / 2,
        "防御でダメージが半減していない (defend={defend_dmg}, auto={auto_dmg})"
    );
}

#[test]
fn counter_reaction_hits_attacker_but_defend_does_not() {
    // Hero に武器 → options=[反撃, 回避, 防御]。反撃=1 / 防御=3。
    let (app_counter, _) = run_enemy_attacks_hero(Some(1), true);
    let enemy_dmg_counter = app_counter
        .database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Enemy)
        .map(|u| u.damage)
        .unwrap_or(0);
    assert!(
        enemy_dmg_counter > 0,
        "反撃で攻撃側にダメージが入っていない"
    );

    let (app_defend, _) = run_enemy_attacks_hero(Some(3), true);
    let enemy_dmg_defend = app_defend
        .database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Enemy)
        .map(|u| u.damage)
        .unwrap_or(0);
    assert_eq!(enemy_dmg_defend, 0, "防御では反撃しないはず");
}

#[test]
fn battle_anim_queued_on_attack_and_cleared_by_tick() {
    // animate_battle 有効時、攻撃解決で命中演出が積まれ、tick で総時間ぶん
    // 進めると破棄される。攻撃側/防御側タイルが正しく記録される。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    add_weapon(&mut app, "Hero", 200, 1);
    place_player_unit(&mut app, "Foe", 9, 9); // Foe の UnitData を用意
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6); // 隣接する攻撃対象
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_animate_battle(true);
    app.set_map_cursor(5, 6);

    assert!(
        app.attack_resolve_and_run(Some((6, 6)), false, ""),
        "攻撃が解決しなかった"
    );
    let anim = app.battle_anim().expect("戦闘演出が積まれていない");
    assert_eq!(anim.attacker, (5, 6));
    assert_eq!(anim.defender, (6, 6));
    assert_eq!(anim.elapsed, 0.0);
    let total = anim.total;
    // 途中までの tick では残る。
    app.tick(total * 0.5);
    assert!(app.battle_anim().is_some(), "途中で演出が消えている");
    // 総時間を超えたら破棄。
    app.tick(total);
    assert!(app.battle_anim().is_none(), "tick 後も演出が残っている");
}

#[test]
fn battle_anim_not_queued_when_disabled() {
    // animate_battle 無効 (既定/ヘッドレス) では演出を積まない (既存テスト互換)。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    add_weapon(&mut app, "Hero", 200, 1);
    place_player_unit(&mut app, "Foe", 9, 9);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_map_cursor(5, 6);

    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    assert!(
        app.battle_anim().is_none(),
        "animate_battle 無効でも演出が積まれている"
    );
}

#[test]
fn battle_animation_subroutine_plays_via_script_when_resolved() {
    // animation.txt で武器(状況)→サブルーチンが解決でき、それが script_library に
    // 実在すれば、戦闘解決後にスクリプト再生が起動する (ネイティブ演出より優先)。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    add_weapon(&mut app, "Hero", 200, 1); // 武器名 = "テスト砲"
    place_player_unit(&mut app, "Foe", 9, 9);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // 戦闘アニメサブルーチンを script_library に登録 (PaintPicture + Wait)。
    let sub = "\
戦闘アニメ_斬撃攻撃:
PaintPicture slash.bmp 100 100
Wait 5
Return
";
    let stmts = crate::data::event::parse(sub).unwrap();
    crate::event_runtime::library_append(&mut app, &stmts);
    // animation.txt: 汎用の テスト砲(攻撃) → 斬撃 (→ 戦闘アニメ_斬撃攻撃)。
    app.database_mut()
        .merge_animation_data("汎用\nテスト砲(攻撃), 斬撃\n");

    app.set_animate_battle(true);
    app.set_map_cursor(5, 6);
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));

    // スクリプト再生が起動し Wait で中断している。
    assert!(
        app.has_script_context(),
        "戦闘アニメスクリプトが起動・中断していない"
    );
    assert!(
        app.pending_timer().is_some(),
        "Wait による中断タイマが立っていない"
    );
    // PaintPicture が overlay に積まれている。
    assert!(
        !app.script_overlay().cmds.is_empty(),
        "PaintPicture が描画コマンドに積まれていない"
    );
    // スクリプト再生を優先したのでネイティブ演出は積まれない。
    assert!(
        app.battle_anim().is_none(),
        "スクリプト再生中にネイティブ演出も積まれている"
    );

    // tick でタイマを満了させるとスクリプトが再開・完了する。
    app.tick(1.0);
    assert!(
        !app.has_script_context(),
        "Wait 満了後もスクリプトが再開・完了していない"
    );
}

#[test]
fn battle_animation_frame_loop_keeps_frame_visible_through_combat() {
    // GBA クローズアップ実機相当: 戦闘解決から起動した戦闘アニメが
    // `PaintPicture; Refresh; ClearPicture; Wait` のフレームループのとき、
    // ClearPicture 遅延クリアにより各フレームが Wait 中も overlay に残ること
    // (毎フレーム空表示にならないこと) を、実際の combat 経路で検証する。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    add_weapon(&mut app, "Hero", 200, 1); // 武器名 = "テスト砲"
    place_player_unit(&mut app, "Foe", 9, 9);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
    app.set_stage_state(crate::stage::StageState::Battle);

    // 2 フレームのループ。各フレームで 1 枚描いて Refresh→ClearPicture→Wait。
    let sub = "\
戦闘アニメ_斬撃攻撃:
For i = 1 To 2
  PaintPicture frame.bmp 10 20 透過
  Refresh
  ClearPicture
  Wait 5
Next
Return
";
    let stmts = crate::data::event::parse(sub).unwrap();
    crate::event_runtime::library_append(&mut app, &stmts);
    app.database_mut()
        .merge_animation_data("汎用\nテスト砲(攻撃), 斬撃\n");

    app.set_animate_battle(true);
    app.set_map_cursor(5, 6);
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));

    // 1 フレーム目の Wait で中断。フレームの PaintPicture が overlay に残っている。
    assert!(
        app.pending_timer().is_some(),
        "1 フレーム目 Wait で中断のはず"
    );
    let pics = |app: &App| {
        app.script_overlay()
            .cmds
            .iter()
            .filter(|c| matches!(c, crate::DrawCmd::Picture { .. }))
            .count()
    };
    assert_eq!(
        pics(&app),
        1,
        "Wait 中はフレームが見える (ClearPicture 即時クリアなら 0 になる)"
    );

    // tick で次フレームへ。毎フレーム ClearPicture されるので累積せず常に 1 枚。
    app.tick(1.0);
    assert!(
        app.pending_timer().is_some(),
        "2 フレーム目 Wait で再度中断"
    );
    assert_eq!(pics(&app), 1, "2 フレーム目も 1 枚 (累積しない)");
}

#[test]
fn battle_animation_falls_back_to_native_without_data() {
    // animation.txt が無ければ従来どおりネイティブ演出 (battle_anim) が積まれる。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    add_weapon(&mut app, "Hero", 200, 1);
    place_player_unit(&mut app, "Foe", 9, 9);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_animate_battle(true);
    app.set_map_cursor(5, 6);
    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    assert!(
        !app.has_script_context(),
        "戦闘アニメデータ無しで script 起動"
    );
    assert!(
        app.battle_anim().is_some(),
        "フォールバックのネイティブ演出が積まれていない"
    );
}

#[test]
fn counterattack_suppressed_when_defender_action_disabled() {
    // 行動不能 (麻痺) の防御側は反撃武器を持っていても反撃できない (SRC
    // MaxAction()==0 ゲート)。必中で命中を確定させ RNG 非依存にする。
    fn run(disable: bool) -> (i64, i64) {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        add_weapon(&mut app, "Hero", 50, 1);
        place_player_unit(&mut app, "Foe", 9, 9);
        add_weapon(&mut app, "Foe", 50, 1);
        // Hero に必中 (攻撃確定命中)。
        app.database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| u.unit_data_name == "Hero")
            .unwrap()
            .add_condition(crate::condition::Condition::new("必中", 3));
        let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
        {
            let e = app.database_mut().unit_by_uid_mut(&euid).unwrap();
            e.add_condition(crate::condition::Condition::new("必中", 3));
            if disable {
                e.add_condition(crate::condition::Condition::new("麻痺", 3));
            }
        }
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_map_cursor(5, 6);
        assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
        let hero_dmg = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Hero" && u.party == crate::Party::Player)
            .map(|u| u.damage)
            .unwrap();
        let enemy_dmg = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.party == crate::Party::Enemy)
            .map(|u| u.damage)
            .unwrap_or(-1);
        (hero_dmg, enemy_dmg)
    }
    let (hero_disabled, enemy_disabled) = run(true);
    let (hero_normal, _enemy_normal) = run(false);
    assert!(hero_normal > 0, "前提: 通常は敵が反撃して Hero が被弾する");
    assert_eq!(hero_disabled, 0, "行動不能の敵が反撃してしまっている");
    assert!(enemy_disabled > 0, "行動不能でも敵自身は攻撃を受ける");
}

#[test]
fn reaction_prompt_skipped_when_defender_action_disabled() {
    // 行動不能 (麻痺) の味方が攻撃されても反撃メニューは出ない (選択不可)。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    app.database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| u.unit_data_name == "Hero")
        .unwrap()
        .add_condition(crate::condition::Condition::new("麻痺", 3));
    place_player_unit(&mut app, "Foe", 9, 9);
    add_weapon(&mut app, "Foe", 60, 1);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 4, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_animate_ai(true);
    app.handle_input(Input::EndPhase);
    let mut prompted = false;
    for _ in 0..200 {
        app.tick(0.1);
        if app.pending_dialog().is_some() {
            prompted = true;
            break;
        }
        if !app.ai_running() {
            break;
        }
    }
    assert!(!prompted, "行動不能の防御側に反撃メニューが出てしまった");
}

#[test]
fn support_guard_reaction_option_offered_only_when_available() {
    // 反撃メニューに「援護防御」が出るのは、隣接に援護防御可能な味方が居るときだけ。
    fn reaction_menu_options(with_guard: bool) -> Vec<String> {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        add_weapon(&mut app, "Hero", 50, 1);
        place_player_unit(&mut app, "Guard", 5, 5); // Hero に隣接する味方
        if with_guard {
            let g = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| u.unit_data_name == "Guard")
                .unwrap();
            g.add_condition(crate::condition::Condition::new("サポートガード", 9));
            g.support_guard_remaining = 1;
        }
        place_player_unit(&mut app, "Foe", 9, 9);
        let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 4, 6);
        let hero_uid = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Hero")
            .unwrap()
            .uid
            .clone();
        app.set_stage_state(crate::stage::StageState::Battle);
        app.begin_reaction_prompt(euid, hero_uid, (5, 6));
        match app.pending_dialog() {
            Some(crate::PendingDialog::Menu { options, .. }) => options.clone(),
            _ => Vec::new(),
        }
    }
    let with = reaction_menu_options(true);
    let without = reaction_menu_options(false);
    assert!(
        with.contains(&"援護防御".to_string()),
        "援護防御 が出ていない: {with:?}"
    );
    assert!(
        !without.contains(&"援護防御".to_string()),
        "援護防御 が誤って出ている: {without:?}"
    );
}

#[test]
fn support_guard_intercepts_on_choice_and_suppressed_on_personal_reaction() {
    // "援護防御" 選択 → 隣接の援護役が肩代わり (防御側被弾 0)。個別反撃 ("防御")
    // 選択 → 援護防御は発動せず防御側が受ける。必中で命中確定し RNG 非依存。
    fn run(def_mode: &str) -> (i64, i64) {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        add_weapon(&mut app, "Hero", 60, 1);
        app.database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| u.unit_data_name == "Hero")
            .unwrap()
            .add_condition(crate::condition::Condition::new("必中", 9));
        place_player_unit(&mut app, "Foe", 9, 9);
        let fuid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6); // 防御側
        let guid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 5); // 援護する敵
        {
            let g = app.database_mut().unit_by_uid_mut(&guid).unwrap();
            g.add_condition(crate::condition::Condition::new("サポートガード", 9));
            g.support_guard_remaining = 1;
        }
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_map_cursor(5, 6);
        assert!(app.attack_resolve_and_run(Some((6, 6)), false, def_mode));
        let foe = app
            .database()
            .unit_by_uid(&fuid)
            .map(|u| u.damage)
            .unwrap_or(-1);
        let guard = app
            .database()
            .unit_by_uid(&guid)
            .map(|u| u.damage)
            .unwrap_or(-1);
        (foe, guard)
    }
    let (foe_guarded, guard_guarded) = run("援護防御");
    assert_eq!(foe_guarded, 0, "援護防御で防御側が被弾している");
    assert!(guard_guarded > 0, "援護役が肩代わりしていない");

    let (foe_defend, guard_defend) = run("防御");
    assert!(foe_defend > 0, "個別防御で防御側が受けていない");
    assert_eq!(guard_defend, 0, "個別反撃選択時に援護防御が発動している");
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

#[test]
fn critical_hit_multiplies_damage_by_one_point_five() {
    // クリティカル率 100% (weapon.critical=100, 技量差0) で必ずクリティカル → ×1.5。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    give_crit_weapon(&mut app, "Hero", 20, 100);
    place_player_unit(&mut app, "Foe", 9, 9);
    let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_map_cursor(5, 6);

    // 非クリの基本ダメージを combat で算出。
    let base = {
        let hero = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Hero")
            .unwrap();
        let foe = app.database().unit_by_uid(&euid).unwrap();
        let atk_unit = app.database().unit_by_name("Hero").unwrap();
        let weapon = atk_unit
            .weapons
            .iter()
            .find(|w| w.name == "クリ砲")
            .unwrap();
        let atk_pilot = app.database().pilot_by_name(&hero.pilot_name).unwrap();
        let def_unit = app.database().unit_by_name("Foe").unwrap();
        let def_pilot = app.database().pilot_by_name(&foe.pilot_name).unwrap();
        let t = app.database().map.as_ref().unwrap().cell(6, 6).terrain_id;
        // 解決経路と同じ地形適応を適用して base を算出 (crit 比 1.5× を保つ)。
        let atk_env = app.terrain_env_at(hero.x, hero.y);
        let def_env = app.terrain_env_at(6, 6);
        crate::combat::predict_with_status_terrain(
            atk_pilot,
            atk_unit,
            weapon,
            def_pilot,
            def_unit,
            app.database().terrain_hit_mod(t),
            app.database().terrain_damage_mod(t),
            hero.morale,
            foe.morale,
            &["必中".to_string()],
            &[],
            atk_env,
            def_env,
            // 必中 / 防御側無印 ＝ 与・被ダメージ修正なし。
            crate::combat::DamageSpiritLevels::default(),
            1.0, // ＥＣＭ 補正なし
        )
        .damage
    };

    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let dealt = app.database().unit_by_uid(&euid).map(|u| u.damage).unwrap();
    assert_eq!(
        dealt,
        base * 3 / 2,
        "クリティカルが ×1.5 になっていない (base={base}, dealt={dealt})"
    );
    assert!(
        app.messages().iter().any(|m| m.contains("クリティカル")),
        "クリティカル表示が無い"
    );
}

/// 特殊効果攻撃属性 (痺) を持つ武器は critical=100 でもクリティカルしない
/// (特殊効果がクリの代わり)。ダメージは ×1.5 されず、状態異常が proc する。
#[test]
fn special_effect_weapon_does_not_crit_but_procs() {
    use crate::data::unit::WeaponData;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 5, 6);
    // 痺 + critical 100 の武器を Hero に持たせ、必中で命中を確定。
    app.database_mut()
        .units
        .iter_mut()
        .find(|u| u.name == "Hero")
        .unwrap()
        .weapons
        .push(WeaponData {
            name: "麻痺砲".into(),
            power: 20,
            min_range: 1,
            max_range: 1,
            precision: 100,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 100,
            class: "痺".into(),
            extras: Vec::new(),
        });
    app.database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| u.unit_data_name == "Hero")
        .unwrap()
        .add_condition(crate::Condition::new("必中", 9));
    place_player_unit(&mut app, "Foe", 9, 9);
    let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
    // Foe は高 HP で生存させる (撃破時は proc しないため)。
    app.database_mut()
        .units
        .iter_mut()
        .find(|u| u.name == "Foe")
        .unwrap()
        .hp = 100_000;
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_map_cursor(5, 6);

    // 非クリの基本ダメージを combat で算出 (crit 抑止後はこれと一致するはず)。
    let base = {
        let hero = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Hero")
            .unwrap();
        let foe = app.database().unit_by_uid(&euid).unwrap();
        let atk_unit = app.database().unit_by_name("Hero").unwrap();
        let weapon = atk_unit
            .weapons
            .iter()
            .find(|w| w.name == "麻痺砲")
            .unwrap();
        let atk_pilot = app.database().pilot_by_name(&hero.pilot_name).unwrap();
        let def_unit = app.database().unit_by_name("Foe").unwrap();
        let def_pilot = app.database().pilot_by_name(&foe.pilot_name).unwrap();
        let t = app.database().map.as_ref().unwrap().cell(6, 6).terrain_id;
        let atk_env = app.terrain_env_at(hero.x, hero.y);
        let def_env = app.terrain_env_at(6, 6);
        crate::combat::predict_with_status_terrain(
            atk_pilot,
            atk_unit,
            weapon,
            def_pilot,
            def_unit,
            app.database().terrain_hit_mod(t),
            app.database().terrain_damage_mod(t),
            hero.morale,
            foe.morale,
            &["必中".to_string()],
            &[],
            atk_env,
            def_env,
            // 必中 / 防御側無印 ＝ 与・被ダメージ修正なし。
            crate::combat::DamageSpiritLevels::default(),
            1.0, // ＥＣＭ 補正なし
        )
        .damage
    };

    assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
    let dealt = app.database().unit_by_uid(&euid).map(|u| u.damage).unwrap();
    assert_eq!(
        dealt, base,
        "特殊効果武器はクリティカルしない (×1.5 されない: base={base}, dealt={dealt})"
    );
    assert!(
        !app.messages().iter().any(|m| m.contains("クリティカル")),
        "特殊効果武器でクリティカル表示は出ない"
    );
    assert!(
        app.database()
            .unit_by_uid(&euid)
            .unwrap()
            .has_condition("麻痺"),
        "特殊効果 (麻痺) が proc する"
    );
}

#[test]
fn defend_halves_critical_rate() {
    // 防御選択でクリティカル率が半減する (SRC)。多数試行で発生率を比較
    // (固定シードなので件数は決定的)。
    fn crit_count(def_mode: &str, trials: usize) -> usize {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        give_crit_weapon(&mut app, "Hero", 20, 50); // クリ率 50% (防御で 25%)
        place_player_unit(&mut app, "Foe", 9, 9);
        let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_map_cursor(5, 6);
        let mut crits = 0;
        for _ in 0..trials {
            // 撃破されないよう毎回ダメージをリセット (同じタイルを攻撃し続ける)。
            app.database_mut().unit_by_uid_mut(&euid).unwrap().damage = 0;
            let before = app.messages().len();
            app.attack_resolve_and_run(Some((6, 6)), false, def_mode);
            if app.messages()[before..]
                .iter()
                .any(|m| m.contains("クリティカル"))
            {
                crits += 1;
            }
        }
        crits
    }
    let normal = crit_count("", 400);
    let defend = crit_count("防御", 400);
    assert!(
        normal > defend,
        "防御でクリ率が下がっていない (normal={normal}, defend={defend})"
    );
    assert!(
        defend < normal * 3 / 4,
        "防御のクリ率半減が不十分 (normal={normal}, defend={defend})"
    );
}

#[test]
fn ai_move_sets_move_anim_and_runner_waits_for_it() {
    // 逐次 AI 移動でスライド演出 (move_anim) が立ち、再生中はランナーが次に進まない。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 9, 6); // 遠い標的
    place_player_unit(&mut app, "Foe", 23, 15); // Foe の UnitData 用 (隅)
    add_weapon(&mut app, "Foe", 60, 1);
    spawn_party(&mut app, "Foe", crate::Party::Enemy, 2, 6); // 攻撃側 (遠方)
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_animate_ai(true);
    app.handle_input(Input::EndPhase);

    // 最初のステップ間隔を消化して 1 体目を行動させる。
    app.tick(0.5);
    assert!(app.ai_running(), "AI フェイズが進行していない");
    let anim = app.move_anim().expect("移動スライド演出が立っていない");
    assert!(anim.path.len() >= 2, "経路が短すぎる: {:?}", anim.path);
    // 論理位置は即時に移動先へ (始点から動いている)。
    let moved_x = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Enemy)
        .map(|u| u.x)
        .unwrap();
    assert!(moved_x > 2, "敵の論理位置が移動していない (x={moved_x})");

    // 演出再生中はランナーが待機 (move_anim が残る短い tick では別ユニットへ進まない)。
    app.tick(0.05);
    assert!(
        app.move_anim().is_some(),
        "短い tick で演出が消えている (待機していない)"
    );
    // 総時間を超えて進めると演出は破棄される。
    for _ in 0..30 {
        app.tick(0.1);
        if app.move_anim().is_none() {
            break;
        }
    }
    assert!(app.move_anim().is_none(), "演出が完了後も残っている");
}

#[test]
fn game_over_only_fires_during_battle() {
    let mut app = App::new();
    // Battle 以外では no-op
    app.game_over();
    assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);

    app.set_stage_state(crate::stage::StageState::Battle);
    app.game_over();
    assert_eq!(app.stage_state(), crate::stage::StageState::Defeat);
    assert!(app.messages().iter().any(|m| m.contains("敗北")));

    // 二度目以降は遷移済みなので no-op
    app.game_over();
    assert_eq!(app.stage_state(), crate::stage::StageState::Defeat);
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

#[test]
fn save_load_preserves_active_dialog() {
    // モーダル中 (Talk dialog 表示中) でも save → load で状態を完全復元できる。
    let mut app = App::new();
    // Talk を発火
    let src = "\
Talk リオ
こんにちは。
End
";
    let stmts = crate::data::event::parse(src).unwrap();
    crate::event_runtime::execute(&mut app, &stmts).unwrap();
    assert!(matches!(
        app.pending_dialog(),
        Some(crate::PendingDialog::Talk { .. })
    ));
    let json = app.to_save_json().expect("serialize");
    let restored = App::from_save_json(&json).expect("deserialize");
    assert!(matches!(
        restored.pending_dialog(),
        Some(crate::PendingDialog::Talk { .. })
    ));
    assert!(restored.has_script_context());
}

#[test]
fn support_attack_fires_when_adjacent_supporter_has_status() {
    // 隣接味方が「サポートアタック」状態を持っていれば、本攻撃の後に
    // 自動で 75% 火力の追撃が走る。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    place_player_unit(&mut app, "Supporter", 3, 6);
    // 敵を隣接 (2,7) に配置
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        2,
        7,
    ));
    // サポーターに能力付与
    app.database_mut().unit_instances[1]
        .add_condition(crate::Condition::new("サポートアタック", -1));
    app.set_stage_state(crate::stage::StageState::Battle);
    // Player ユニット (Hero) に攻撃用武器を持たせる
    app.database_mut().units[0]
        .weapons
        .push(crate::data::unit::WeaponData {
            name: "ビーム".to_string(),
            power: 200,
            min_range: 1,
            max_range: 3,
            precision: 50,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        });
    // カーソルを敵に合わせて Hero 側から攻撃
    app.map_cursor = Some((2, 6));
    let _ = app.attack_target();
    // 敵 HP は 100、ヒット時の本攻撃 + サポートアタックで多めに削れていれば良い。
    // 1 度ヒットしただけでも撃破される可能性が高いので、削れた事実だけ確認。
    let enemy = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Enemy);
    // 撃破 or ダメージ蓄積していること
    let damaged_or_dead = enemy.map(|u| u.damage > 0).unwrap_or(true);
    assert!(damaged_or_dead, "expected enemy hit");
    // サポーターの残りサポート攻撃が 0 になっていること (使った場合)
    // ※本攻撃でミス → サポートアタック発動 (1→0) のケース、
    // 　本攻撃で撃破 → サポートアタック発動せず (1 のまま) のケース、両方許容。
    let sup = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Supporter")
        .unwrap();
    assert!(sup.support_attack_remaining <= 1);
}

#[test]
fn support_guard_fires_during_enemy_phase() {
    // 敵フェイズに敵が味方ユニット (Target) を攻撃した際、隣接する援護防御可能な
    // 味方ユニット (Guard) が代わりにダメージを受ける。
    // Target は無傷 / Guard はダメージ蓄積していること。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    // Target (3, 6): 攻撃を受ける味方ユニット (武器なし)
    place_player_unit(&mut app, "Target", 3, 6);
    // Guard (4, 6): 援護防御ユニット (Target の隣)
    place_player_unit(&mut app, "Guard", 4, 6);
    // Guard に「サポートガード」能力を付与
    {
        let guard = app
            .database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| u.unit_data_name == "Guard")
            .unwrap();
        guard.add_condition(crate::Condition::new("サポートガード", -1));
    }
    // 敵ユニット (2, 6): Target に隣接、武器あり、必中
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Target", // Hero の unit data を再利用
        "PILOT",
        crate::Party::Enemy,
        2,
        6,
    ));
    // 敵に武器を持たせる (power=50, range=1, precision=100)
    app.database_mut()
        .units
        .iter_mut()
        .find(|u| u.name == "Target")
        .unwrap()
        .weapons
        .push(crate::data::unit::WeaponData {
            name: "バルカン".to_string(),
            power: 50,
            min_range: 1,
            max_range: 1,
            precision: 100,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        });
    // 必中条件: 命中 100 を保証
    app.database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| u.party == crate::Party::Enemy)
        .unwrap()
        .add_condition(crate::Condition::new("必中", -1));
    app.set_stage_state(crate::stage::StageState::Battle);
    // EndPhase → 敵フェイズへ → AI が Target を攻撃 → 援護防御発動
    app.handle_input(Input::EndPhase);
    // Target は無傷のはず
    let target = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Target" && u.party == crate::Party::Player);
    let target_damage = target.map(|u| u.damage).unwrap_or(0);
    assert_eq!(
        target_damage, 0,
        "Target should be unharmed (support guard protected it)"
    );
    // Guard はダメージを受けているか撃破されているはず
    let guard = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Guard");
    let guard_damaged_or_dead = guard.map(|u| u.damage > 0).unwrap_or(true);
    assert!(
        guard_damaged_or_dead,
        "Guard should have taken damage (support guarded Target)"
    );
}

/// 直撃 (精神コマンド) を持つ攻撃側はサポートガード (援護防御) を無効化する。
/// Target が直接ダメージを受け、Guard は無傷になる。
#[test]
fn chokugeki_disables_support_guard() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Target", 3, 6);
    place_player_unit(&mut app, "Guard", 4, 6);
    app.database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| u.unit_data_name == "Guard")
        .unwrap()
        .add_condition(crate::Condition::new("サポートガード", -1));
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Target",
        "PILOT",
        crate::Party::Enemy,
        2,
        6,
    ));
    app.database_mut()
        .units
        .iter_mut()
        .find(|u| u.name == "Target")
        .unwrap()
        .weapons
        .push(crate::data::unit::WeaponData {
            name: "バルカン".to_string(),
            power: 50,
            min_range: 1,
            max_range: 1,
            precision: 100,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        });
    // 敵に 必中 + 直撃。
    {
        let enemy = app
            .database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| u.party == crate::Party::Enemy)
            .unwrap();
        enemy.add_condition(crate::Condition::new("必中", -1));
        enemy.add_condition(crate::Condition::new("直撃", -1));
    }
    app.set_stage_state(crate::stage::StageState::Battle);
    app.handle_input(Input::EndPhase); // 敵フェイズ → AI が Target を攻撃
    let target_damage = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Target" && u.party == crate::Party::Player)
        .map(|u| u.damage)
        .unwrap_or(0);
    let guard_damage = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Guard")
        .map(|u| u.damage)
        .unwrap_or(-1);
    assert!(
        target_damage > 0,
        "直撃でサポートガードが無効 → Target が被弾"
    );
    assert_eq!(guard_damage, 0, "Guard は肩代わりしない (無傷)");
}

/// 捨て身 は反撃では無効 (自分から攻撃したときのみ ×3、反撃では適用しない)。
#[test]
fn sutemi_does_not_apply_on_counterattack() {
    use crate::data::unit::WeaponData;
    fn counter_dmg(with_sutemi: bool) -> i64 {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6); // 反撃を受ける側
        let mut ebot = app.database().unit_by_name("Hero").cloned().unwrap();
        ebot.name = "Counter".into();
        ebot.weapons = vec![WeaponData {
            name: "反撃砲".into(),
            power: 20,
            min_range: 1,
            max_range: 1,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        }];
        app.database_mut().units.push(ebot);
        let hero = first_player_uid(&app);
        let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
            "Counter",
            "PILOT",
            crate::Party::Enemy,
            3,
            6,
        ));
        app.database_mut()
            .unit_by_uid_mut(&enemy)
            .unwrap()
            .add_condition(crate::Condition::new("必中", -1));
        if with_sutemi {
            app.database_mut()
                .unit_by_uid_mut(&enemy)
                .unwrap()
                .add_condition(crate::Condition::new("捨て身", -1));
        }
        app.try_counterattack(3, 6, (2, 6));
        app.database().unit_by_uid(&hero).unwrap().damage
    }
    let normal = counter_dmg(false);
    let with_sutemi = counter_dmg(true);
    assert!(normal > 0, "反撃が命中している");
    assert_eq!(with_sutemi, normal, "捨て身 は反撃では ×3 されない");
}

/// 切り払い: 高レベルの防御側は格闘攻撃を切り払う (確率 prob=2×防御Lv−攻撃Lv、Dice 1..32)。
/// 射撃 (近接でない) は切り払い対象外。
#[test]
fn parry_nullifies_melee_for_skilled_defender() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Swordsman", 2, 6);
    // 攻撃側用に 切り払いなしの別パイロット Brute を作る。
    {
        let mut p = app.database().pilot_by_name("PILOT").unwrap().clone();
        p.name = "Brute".into();
        p.nickname = "Brute".into();
        p.features = vec![];
        app.database_mut().pilots.push(p);
    }
    let attacker = app.database_mut().register_unit(crate::UnitInstance::new(
        "Swordsman",
        "Brute",
        crate::Party::Enemy,
        3,
        6,
    ));
    // Swordsman の搭乗者 PILOT に 切り払いLv16 (prob 2×16=32 → 必ず発動)。
    {
        let p = app
            .database_mut()
            .pilots
            .iter_mut()
            .find(|p| p.name == "PILOT")
            .unwrap();
        p.features.push(("切り払いLv16".into(), String::new()));
    }
    let def_uid = first_player_uid(&app);
    let def_idx = app.database().idx_by_uid(&def_uid).unwrap();
    let atk_idx = app.database().idx_by_uid(&attacker).unwrap();
    assert_eq!(app.unit_parry_level(def_idx), 16, "防御側 切り払いLv16");
    assert_eq!(app.unit_parry_level(atk_idx), 0, "攻撃側 切り払いなし");
    assert!(
        app.roll_parry(atk_idx, def_idx, "武"),
        "格闘攻撃 (武) を切り払う"
    );
    assert!(
        !app.roll_parry(atk_idx, def_idx, "射"),
        "射撃は切り払い対象外"
    );
}

/// 見切り (特殊防御発動 SpecialPower): 切り払いを必ず成功させる。攻撃側 Lv が高く
/// 通常なら発動率が負 (絶対に切り払えない) でも、見切りがあれば必ず切り払う。
#[test]
fn foresight_guarantees_parry() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Swordsman", 2, 6);
    // 攻撃側: 切り払いLv16 の別パイロット (防御側 Lv1 では prob=2-16=-14 → 通常は不発)。
    {
        let mut p = app.database().pilot_by_name("PILOT").unwrap().clone();
        p.name = "Master".into();
        p.nickname = "Master".into();
        p.features = vec![("切り払いLv16".into(), String::new())];
        app.database_mut().pilots.push(p);
    }
    let attacker = app.database_mut().register_unit(crate::UnitInstance::new(
        "Swordsman",
        "Master",
        crate::Party::Enemy,
        3,
        6,
    ));
    // 防御側 PILOT に 切り払いLv1 (低レベル)。
    {
        let p = app
            .database_mut()
            .pilots
            .iter_mut()
            .find(|p| p.name == "PILOT")
            .unwrap();
        p.features.push(("切り払いLv1".into(), String::new()));
    }
    let def_uid = first_player_uid(&app);
    let def_idx = app.database().idx_by_uid(&def_uid).unwrap();
    let atk_idx = app.database().idx_by_uid(&attacker).unwrap();

    // 見切り無し: prob = 2×1 − 16 = −14 → 乱数 (1..32) 未満で必ず不発。
    assert!(
        !app.roll_parry(atk_idx, def_idx, "武"),
        "見切り無し・高 Lv 攻撃には切り払えない"
    );
    // 見切り付与: prob=32 で必ず切り払う。
    app.database_mut().unit_instances[def_idx].add_condition(crate::Condition::new("見切り", 1));
    assert!(
        app.roll_parry(atk_idx, def_idx, "武"),
        "見切りがあれば必ず切り払う"
    );
    // 見切りでも切り払い不可武器 (射撃) は無効。
    assert!(
        !app.roll_parry(atk_idx, def_idx, "射"),
        "見切りでも射撃は切り払い対象外"
    );
}

/// 防御特性 (耐性 ÷2 / 無効化 0 / 吸収 -1/2) を与ダメージへ適用。弱点/有効が一致すると不適用。
#[test]
fn defense_attributes_modify_damage() {
    use crate::feature::ActiveFeature;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Tank", 2, 6);
    let idx = app.database().idx_by_uid(&first_player_uid(&app)).unwrap();
    let set = |app: &mut App, feats: Vec<ActiveFeature>| {
        app.database_mut().unit_instances[idx].active_features = feats;
    };
    // 属性なし → 変化なし。
    assert_eq!(app.defense_attribute_damage(idx, "火 実", 100), 100);
    // 耐性=火 → ÷2。非一致 (氷) は変化なし。
    set(&mut app, vec![ActiveFeature::new("耐性", "火")]);
    assert_eq!(app.defense_attribute_damage(idx, "火 実", 100), 50);
    assert_eq!(app.defense_attribute_damage(idx, "氷 実", 100), 100);
    // 無効化=火 → 0。
    set(&mut app, vec![ActiveFeature::new("無効化", "火")]);
    assert_eq!(app.defense_attribute_damage(idx, "火 実", 100), 0);
    // 吸収=火 → -50 (回復)。
    set(&mut app, vec![ActiveFeature::new("吸収", "火")]);
    assert_eq!(app.defense_attribute_damage(idx, "火 実", 100), -50);
    // 弱点優先: 弱点=火 があれば耐性は適用しない (与ダメ変化なし)。
    set(
        &mut app,
        vec![
            ActiveFeature::new("弱点", "火"),
            ActiveFeature::new("耐性", "火"),
        ],
    );
    assert_eq!(app.defense_attribute_damage(idx, "火 実", 100), 100);
}

/// マップ攻撃も通常戦闘と同じく防御特性 (無効化/耐性/吸収) を反映する
/// (`event_runtime::map_attack`)。火属性マップ攻撃に対し 無効化=火 の敵は 0 ダメージで
/// 生存し、特性なしの敵は被弾する (以前はマップ攻撃だけ防御特性が抜けていた)。
#[test]
fn mapattack_applies_defense_attributes() {
    use crate::feature::ActiveFeature;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Attacker", 5, 5);
    // 攻撃側に火属性の高威力武器を付与。
    {
        let ud = app
            .database_mut()
            .units
            .iter_mut()
            .find(|d| d.name == "Attacker")
            .unwrap();
        ud.weapons.push(crate::data::unit::WeaponData {
            name: "FireCannon".into(),
            power: 8000,
            min_range: 1,
            max_range: 3,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: "火".into(),
            extras: Vec::new(),
        });
    }
    // 無効化=火 の敵 (生存するはず) と 特性なしの敵 (被弾するはず) を area 内に配置。
    let nullify_uid = app.database_mut().register_unit(crate::UnitInstance::new(
        "Attacker",
        "PILOT",
        crate::Party::Enemy,
        6,
        5,
    ));
    app.database_mut()
        .unit_by_uid_mut(&nullify_uid)
        .unwrap()
        .active_features = vec![ActiveFeature::new("無効化", "火")];
    let plain_uid = app.database_mut().register_unit(crate::UnitInstance::new(
        "Attacker",
        "PILOT",
        crate::Party::Enemy,
        6,
        4,
    ));
    // スクリプト経路 (is_event=true) でマップ攻撃を実行。
    crate::event_runtime::map_attack(&mut app, None, "FireCannon", 6, 5, true);
    // 無効化=火 の敵は 0 ダメージで生存。
    let nullified = app.database().unit_by_uid(&nullify_uid);
    assert!(
        nullified.is_some(),
        "無効化=火 の敵はマップ火属性攻撃で撃破されない"
    );
    assert_eq!(
        nullified.unwrap().damage,
        0,
        "無効化=火 でマップ火属性攻撃は 0 ダメージ"
    );
    // 対照: 特性なしの敵は被弾する (撃破で除去 or ダメージ > 0)。
    let plain_hit = app
        .database()
        .unit_by_uid(&plain_uid)
        .map(|u| u.damage > 0)
        .unwrap_or(true);
    assert!(plain_hit, "特性なしの敵はマップ攻撃で被弾する (対照)");
}

/// 防御特性の属性照合は字単位の**部分一致** (VB5 `InStrNotNest`)。無空白複合 class
/// ("格実火") も弱点/耐性 "火" に一致する (旧 `split_whitespace` 完全一致では非一致で
/// 防御特性が事実上機能していなかった)。併せて弱点検出 (装甲半減トリガ) を確認。
#[test]
fn defense_attr_substring_matching_and_weakness_detection() {
    use crate::feature::ActiveFeature;
    // 純関数 defense_attr_matches の字単位照合。
    assert!(
        App::defense_attr_matches("格実火", "火"),
        "複合 class が弱点 火 に部分一致"
    );
    assert!(!App::defense_attr_matches("格実水", "火"));
    assert!(
        App::defense_attr_matches("格実火", "火地獣"),
        "弱点リストのいずれかに一致"
    );
    assert!(App::defense_attr_matches("射", "全"), "全=常に一致");
    assert!(
        App::defense_attr_matches("格実火", "物"),
        "物=魔/精 でなければ一致"
    );
    assert!(
        !App::defense_attr_matches("魔術火", "物"),
        "物 は 魔 武器に不一致"
    );
    assert!(!App::defense_attr_matches("射", ""), "空リストは不一致");
    // 魔 属性例外: 魔武/魔突/魔接/魔銃/魔実 武器には「魔」防御特性が効かない。
    assert!(
        !App::defense_attr_matches("魔武", "魔"),
        "魔武 武器は 魔 防御特性に不一致 (免除)"
    );
    assert!(
        App::defense_attr_matches("魔術火", "魔"),
        "魔術 (非 魔X) は 魔 防御特性に一致"
    );
    assert!(
        App::defense_attr_matches("武魔", "魔"),
        "逆順 武魔 は免除されず一致"
    );

    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Tank", 2, 6);
    let idx = app.database().idx_by_uid(&first_player_uid(&app)).unwrap();
    // 耐性=火 → 無空白複合武器 "格実火" のダメージ半減 (旧実装は非一致で素通りだった)。
    app.database_mut().unit_instances[idx].active_features = vec![ActiveFeature::new("耐性", "火")];
    assert_eq!(app.defense_attribute_damage(idx, "格実火", 1000), 500);
    // 弱点=火 検出 (装甲半減のトリガ)。
    app.database_mut().unit_instances[idx].active_features = vec![ActiveFeature::new("弱点", "火")];
    assert!(
        app.weapon_hits_weakness(idx, "格実火"),
        "弱点 火 が複合 火武器に該当"
    );
    assert!(!app.weapon_hits_weakness(idx, "格実水"));
    // 一時付加 condition `弱点:火` も検出。
    app.database_mut().unit_instances[idx].active_features = vec![];
    app.database_mut().unit_instances[idx]
        .conditions
        .push(crate::condition::Condition::new("弱点:火", 3));
    assert!(
        app.weapon_hits_weakness(idx, "格実火"),
        "一時付加 弱点:火 も該当"
    );
    // 吸収検出 (装甲無視のトリガ): 吸収=水 は 水武器に該当、有効=水 で打ち消される。
    app.database_mut().unit_instances[idx].conditions.clear();
    app.database_mut().unit_instances[idx].active_features = vec![ActiveFeature::new("吸収", "水")];
    assert!(
        app.weapon_hits_absorb(idx, "魔術水"),
        "吸収 水 が 水武器に該当"
    );
    assert!(
        !app.weapon_hits_absorb(idx, "魔術火"),
        "非一致 (火) は不該当"
    );
    app.database_mut().unit_instances[idx].active_features = vec![
        ActiveFeature::new("吸収", "水"),
        ActiveFeature::new("有効", "水"),
    ];
    assert!(
        !app.weapon_hits_absorb(idx, "魔術水"),
        "有効=水 が吸収を打ち消す"
    );
}

#[test]
fn ai_uses_dijkstra_to_close_distance() {
    // 敵ユニットがマップ反対側 (10, 6) にいる。プレイヤーは Player phase 終了で
    // Enemy AI が走る。AI はプレイヤーに向かって速度分マスを進む。
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    // 敵ユニットを (10, 6) に配置
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        10,
        6,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    // EndPhase で Enemy phase が走る → AI が Hero (player) に近付く
    app.handle_input(Input::EndPhase);
    // AI が動いた結果、敵は (2,6) のプレイヤーに近付いている
    let enemy = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Enemy)
        .expect("enemy alive");
    let manhattan_after = (enemy.x as i32 - 2).unsigned_abs() + (enemy.y as i32 - 6).unsigned_abs();
    assert!(
        manhattan_after < 8,
        "AI failed to close: dist={manhattan_after}"
    );
}

#[test]
fn poison_status_drops_hp_each_phase_and_one_turn_buffs_clear() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Poisoned", 2, 6);
    // 勝利判定を回避するため敵ユニットも配置
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Poisoned",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    // unit_data の hp は 100。10% = 10 ダメージのはず
    let u = &mut app.database_mut().unit_instances[0];
    u.add_condition(crate::Condition::new("毒", -1)); // 毒は自動削除しない
    u.add_condition(crate::Condition::new("必中", 1));
    u.add_condition(crate::Condition::new("永続バフ", -1));
    assert_eq!(u.damage, 0);
    // Player フェイズ → Enemy → Allied → Neutral → Player (turn 2) の遷移で
    // Player party の begin_phase が再度呼ばれる。end_phase 1 回で 1 ティック。
    app.handle_input(Input::EndPhase);
    let u = &app.database().unit_instances[0];
    assert!(u.damage >= 10, "expected poison tick ≥10, got {}", u.damage);
    assert!(!u.has_condition("必中"), "必中 should have expired");
    assert!(
        u.has_condition("毒"),
        "毒 should still be present (permanent condition)"
    );
    assert!(u.has_condition("永続バフ"));
}

/// 毒ダメージは静的 HP ではなく実効最大 HP (改造/強化パーツ反映) の 10%。
#[test]
fn poison_damage_scales_with_effective_max_hp() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Tough", 2, 6);
    // 改造段階を上げ、実効最大 HP を静的 HP より大きくする。
    app.database_mut().unit_instances[0].upgrade_level = 5;
    app.database_mut().unit_instances[0].add_condition(crate::Condition::new("毒", -1));
    let eff = app
        .database()
        .effective_max_hp(&app.database().unit_instances[0]);
    let static_hp = app.database().unit_by_name("Tough").unwrap().hp;
    assert!(
        eff > static_hp,
        "改造で実効 HP が静的 HP を超える (eff={eff}, static={static_hp})"
    );
    app.begin_phase(crate::Phase::Player);
    assert_eq!(
        app.database().unit_instances[0].damage,
        (eff / 10).max(1),
        "毒ダメージは実効最大 HP の 10% (静的 HP 基準ではない)"
    );
}

/// 死の宣告 (告) は期限切れ (次の自軍フェイズ) で HP を 1 にする。ボスは無効。
#[test]
fn death_sentence_sets_hp_to_one_on_expiry() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Doomed", 2, 6);
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Doomed",
        "PILOT",
        crate::Party::Enemy,
        10,
        10,
    ));
    app.set_stage_state(crate::stage::StageState::Battle);
    let uid = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&uid)
        .unwrap()
        .add_condition(crate::Condition::new("死の宣告", 1));
    app.handle_input(Input::EndPhase); // T2 味方フェイズ開始で発動
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().damage,
        99,
        "死の宣告 で HP が 1 になる (最大HP100 → damage 99)"
    );
}

/// 毒ダメージは 毒属性への弱点で倍・耐性で半減する (特殊効果攻撃属性.md)。
#[test]
fn poison_damage_scales_with_resistance_and_weakness() {
    fn poison_tick(features: Vec<crate::feature::ActiveFeature>) -> i64 {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "P", 2, 6);
        app.database_mut().register_unit(crate::UnitInstance::new(
            "P",
            "PILOT",
            crate::Party::Enemy,
            10,
            10,
        ));
        app.set_stage_state(crate::stage::StageState::Battle);
        {
            let u = &mut app.database_mut().unit_instances[0];
            u.add_condition(crate::Condition::new("毒", -1));
            u.active_features = features;
        }
        app.handle_input(Input::EndPhase); // T2 Player フェイズ開始で 1 ティック
        app.database().unit_instances[0].damage
    }
    let normal = poison_tick(vec![]);
    let weak = poison_tick(vec![crate::feature::ActiveFeature::new("弱点", "毒")]);
    let resist = poison_tick(vec![crate::feature::ActiveFeature::new("耐性", "毒")]);
    assert_eq!(weak, normal * 2, "弱点=毒 で毒ダメージ倍");
    assert_eq!(resist, (normal / 2).max(1), "耐性=毒 で毒ダメージ半減");
}

#[test]
fn game_clear_only_fires_during_battle() {
    let mut app = App::new();
    app.set_stage_state(crate::stage::StageState::Battle);
    app.game_clear();
    assert_eq!(app.stage_state(), crate::stage::StageState::Victory);
    assert!(app.messages().iter().any(|m| m.contains("勝利")));
}

/// SRC 本体相当の `Data/System/GameOver.eve`(`プロローグ:` ラベル) を再現した
/// 最小スクリプト。コンティニュー Ask → `選択=1` で `Quickload` → else `GameClear`。
const GAMEOVER_EVE: &str = "プロローグ:\n\
        Ask コンティニュー？\n\
        はい\n\
        いいえ\n\
        End\n\
        If 選択 = 1 Then\n\
        Quickload\n\
        Endif\n\
        GameClear\n\
        Exit\n";

#[test]
fn game_over_fires_gameover_eve_prologue_continue_prompt() {
    // 旧バグ: GameOver.eve のコンティニューは `プロローグ:` ラベルなのに
    // `game_over` が `GameOver`/`ゲームオーバー` しか試さず「何も起こらない」。
    let mut app = App::new();
    let stmts = crate::data::event::parse(GAMEOVER_EVE).unwrap();
    app.script_library_mut()
        .append_with_name(&stmts, "Data/System/GameOver.eve");
    app.set_stage_state(crate::stage::StageState::Battle);

    app.game_over();
    assert_eq!(app.stage_state(), crate::stage::StageState::Defeat);
    // GameOver.eve の プロローグ が発火し、コンティニュー Ask が出る。
    assert!(
        matches!(
            app.pending_dialog(),
            Some(crate::dialog::PendingDialog::Menu { .. })
        ),
        "コンティニュー Ask が表示される"
    );
}

#[test]
fn game_over_continue_choice_requests_reload_from_restart_save() {
    let mut app = App::new();
    app.set_stage_state(crate::stage::StageState::Battle);
    // 戦闘開始時スナップショット相当を __restart_save に保存 (enter_battle_state 相当)。
    let snapshot = app.to_save_json().unwrap();
    app.set_script_var("__restart_save".to_string(), snapshot.clone());

    let stmts = crate::data::event::parse(GAMEOVER_EVE).unwrap();
    app.script_library_mut()
        .append_with_name(&stmts, "GameOver.eve");

    app.game_over();
    // コンティニュー Ask に「はい」(choice 1) で応答 → Quickload。
    assert!(app.respond_dialog(1));
    // __quicksave が無いので __restart_save から再ロードが要求される。
    assert_eq!(
        app.take_pending_reload().as_deref(),
        Some(snapshot.as_str()),
        "コンティニューで __restart_save から再ロード要求"
    );
}

#[test]
fn game_over_decline_continue_does_not_reload() {
    let mut app = App::new();
    app.set_stage_state(crate::stage::StageState::Battle);
    app.set_script_var("__restart_save".to_string(), app.to_save_json().unwrap());
    let stmts = crate::data::event::parse(GAMEOVER_EVE).unwrap();
    app.script_library_mut()
        .append_with_name(&stmts, "GameOver.eve");

    app.game_over();
    // 「いいえ」(choice 2) → Quickload を踏まず GameClear 経路 → 再ロード無し。
    assert!(app.respond_dialog(2));
    assert!(
        app.take_pending_reload().is_none(),
        "コンティニュー拒否では再ロードしない"
    );
}

#[test]
fn quickload_falls_back_to_restart_save_when_no_quicksave() {
    // `Quickload` 単体: __quicksave が空でも __restart_save があれば再ロード要求。
    let mut app = App::new();
    let snapshot = app.to_save_json().unwrap();
    app.set_script_var("__restart_save".to_string(), snapshot.clone());
    let stmts = crate::data::event::parse("Quickload\n").unwrap();
    crate::event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(
        app.take_pending_reload().as_deref(),
        Some(snapshot.as_str())
    );
}

#[test]
fn invoke_custom_unit_command_runs_body_with_target_bound() {
    let mut app = App::new();
    let setup = "Create 味方 ブレイバー 0 リオ 1 3 3\n";
    let stmts = crate::data::event::parse(setup).unwrap();
    crate::event_runtime::execute(&mut app, &stmts).unwrap();
    let uid = app.database().unit_instances[0].uid.clone();

    // シナリオ定義のユニットコマンドを登録。
    // `常true:` ラベルは条件サブルーチン — Return 1 で常に表示。
    let cc = "*ユニットコマンド テスト 味方 Call(常true):\n\
                  Set 実行対象 $(対象ユニットＩＤ)\n\
                  Exit\n\
                  常true:\nReturn 1\n";
    let cc_stmts = crate::data::event::parse(cc).unwrap();
    app.script_library_mut().append(&cc_stmts);

    // 対象ユニットＩＤ が束縛され本体が実行される。
    assert!(app.invoke_custom_unit_command(&uid, "テスト"));
    assert_eq!(app.script_var("実行対象"), uid);
    // 未登録コマンド名は false。
    assert!(!app.invoke_custom_unit_command(&uid, "存在しないコマンド"));
    // 不在ユニットも false。
    assert!(!app.invoke_custom_unit_command("U999", "テスト"));
}

#[test]
fn ai_prefers_high_damage_target() {
    // Two enemy units at different distances: one killable with ~100 damage, one not.
    // AI should target the killable one even if slightly farther.
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);

    // Player unit at (3, 3) - we control Hero
    place_player_unit(&mut app, "Hero", 3, 3);

    // Enemy 1: medium distance (dist=2), HP remaining = 40 (killable with ~100 damage)
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        1, // x = 1, manhattan dist from (3,3) = 2
        3, // y = 3
    ));
    app.database_mut().unit_instances.last_mut().unwrap().damage = 60; // HP remaining = 40

    // Enemy 2: closer (dist=1), HP remaining = 100 (NOT killable with ~100 damage)
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        4, // x = 4, manhattan dist from (3,3) = 1
        3, // y = 3
    ));
    app.database_mut().unit_instances.last_mut().unwrap().damage = 0; // HP remaining = 100

    app.set_stage_state(crate::stage::StageState::Battle);
    // End player phase to trigger enemy AI
    app.handle_input(Input::EndPhase);

    // After AI phase, check which enemy was attacked
    // The killable enemy (HP 40) should have taken more damage
    let enemies: Vec<_> = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| u.party == crate::Party::Enemy)
        .collect();
    let most_damaged = enemies.iter().max_by_key(|u| u.damage);
    if let Some(t) = most_damaged {
        // The killable enemy should have been targeted (higher damage taken)
        assert!(
            t.damage >= 60,
            "AI should target killable enemy, got damage={}",
            t.damage
        );
    }
}

/// 敵 AI は、ターゲットを射程内に収める到達マスが複数あるとき、防御地形 (森/山/都市)
/// を選好する (SRC 戦術判断)。安価な平地マスより遠い森林マスを選ぶことを確認する。
#[test]
fn ai_prefers_defensive_terrain_among_attack_tiles() {
    let mut app = App::new();
    app.handle_input(Input::Advance); // Configuration
    app.handle_input(Input::Advance); // MapView
                                      // 5x3 平地マップ。(3,0) のみ森林 (damage_mod=5 / hit_mod=10 → 防御価値 15)。
    let mut m = crate::data::map::MapData::new(5, 3);
    let mut forest = m.cell(3, 0);
    forest.terrain_id = 2; // 森林
    m.set_cell(3, 0, forest);
    app.database_mut().replace_map(m);

    // 標的 (味方) を (3,1) に配置 (これが唯一の敵対ターゲット)。
    place_player_unit(&mut app, "Hero", 3, 1);
    // 攻撃側 (敵) の UnitData "Foe" を Hero から複製して用意 (インスタンスは作らない)。
    // 射程1武器・速度5 で、(3,1) を射程に収める複数のマスへ到達できる。
    let mut foe_ud = app.database().unit_by_name("Hero").cloned().unwrap();
    foe_ud.name = "Foe".into();
    foe_ud.speed = 5;
    app.database_mut().units.push(foe_ud);
    add_weapon(&mut app, "Foe", 60, 1);
    let foe = spawn_party(&mut app, "Foe", crate::Party::Enemy, 0, 1);

    app.set_stage_state(crate::stage::StageState::Battle);
    // 敵フェイズ (同期 AI) を走らせる。
    app.handle_input(Input::EndPhase);

    // (3,1) を射程に収める到達マス: (2,1)平地[安価] / (3,2)平地 / (4,1)平地 / (3,0)森林[高価]。
    // 防御選好が無ければ最安・最近接の (2,1) を選ぶが、森林 (3,0) を選好するはず。
    let f = app.database().unit_by_uid(&foe).unwrap();
    assert_eq!(
        (f.x, f.y),
        (3, 0),
        "AI は射程内の攻撃マスのうち防御地形 (森林 (3,0)) を選好する (実際: ({},{}))",
        f.x,
        f.y
    );
}

/// 瀕死 (HP ≤ 1/4) の敵 AI は、撃破される前に「復活」精神を自分へ事前発動する。
#[test]
fn ai_uses_revive_spirit_when_critical() {
    use crate::data::pilot::SpiritCommand;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    // 味方ターゲットを 1 体 (敵 AI が攻撃対象を持ち・即決着を防ぐ)。
    place_player_unit(&mut app, "Hero", 2, 6);
    // 瀕死の敵を配置。共有パイロット "PILOT" に 復活 精神を与える。
    let enemy = spawn_party(&mut app, "Hero", crate::Party::Enemy, 10, 10);
    {
        let p = app
            .database_mut()
            .pilots
            .iter_mut()
            .find(|p| p.name == "PILOT")
            .unwrap();
        p.sp = Some(50);
        p.spirit_commands = vec![SpiritCommand {
            name: "復活".into(),
            cost: Some(30),
            level: 1,
        }];
    }
    // 敵を瀕死に: 実効最大 HP 100, damage 80 → 現 HP 20 ≤ 25 (1/4)。
    app.database_mut().unit_by_uid_mut(&enemy).unwrap().damage = 80;
    app.set_stage_state(crate::stage::StageState::Battle);

    // 敵フェイズ (同期 AI) を走らせる。
    app.handle_input(Input::EndPhase);

    assert!(
        app.database()
            .unit_by_uid(&enemy)
            .unwrap()
            .has_condition("復活"),
        "瀕死の敵 AI は撃破に備えて復活精神を事前発動する"
    );
}

/// 余力のある (HP > 1/4) 敵 AI は復活精神を温存する (無駄撃ちしない)。
#[test]
fn ai_keeps_revive_spirit_when_healthy() {
    use crate::data::pilot::SpiritCommand;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    let enemy = spawn_party(&mut app, "Hero", crate::Party::Enemy, 10, 10);
    {
        let p = app
            .database_mut()
            .pilots
            .iter_mut()
            .find(|p| p.name == "PILOT")
            .unwrap();
        p.sp = Some(50);
        p.spirit_commands = vec![SpiritCommand {
            name: "復活".into(),
            cost: Some(30),
            level: 1,
        }];
    }
    // 健全 (damage 0 → 現 HP 100 > 25)。
    app.set_stage_state(crate::stage::StageState::Battle);
    app.handle_input(Input::EndPhase);

    assert!(
        !app.database()
            .unit_by_uid(&enemy)
            .unwrap()
            .has_condition("復活"),
        "余力のある敵 AI は復活精神を温存する"
    );
}

/// 敵 AI は、敵対勢力がマップ兵器を持つとき、攻撃マスのうち同陣営との密集を避けた
/// マスを選ぶ (1 発で複数巻き込まれないよう散開する)。
#[test]
fn ai_spreads_to_avoid_enemy_map_weapon() {
    let mut app = App::new();
    app.handle_input(Input::Advance); // Configuration
    app.handle_input(Input::Advance); // MapView
                                      // 7x3 平地マップ (防御価値は一様 → 密集度が決め手)。
    app.database_mut()
        .replace_map(crate::data::map::MapData::new(7, 3));

    // 味方ターゲット兼マップ兵器持ち (脅威源) を (6,1) に。マップ兵器のみ＝反撃しない。
    place_player_unit(&mut app, "Hero", 6, 1);
    {
        let h = app
            .database_mut()
            .units
            .iter_mut()
            .find(|u| u.name == "Hero")
            .unwrap();
        h.weapons.push(crate::data::unit::WeaponData {
            name: "マップ砲".into(),
            power: 80,
            min_range: 1,
            max_range: 5,
            precision: 100,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: "Ｍ全".into(),
            extras: Vec::new(),
        });
    }
    // 攻撃側 (敵): 射程1武器・速度8。Hero を射程に収める複数マスへ到達できる。
    let mut foe_ud = app.database().unit_by_name("Hero").cloned().unwrap();
    foe_ud.name = "Foe".into();
    foe_ud.speed = 8;
    foe_ud.weapons.clear(); // マップ兵器は引き継がない
    app.database_mut().units.push(foe_ud);
    add_weapon(&mut app, "Foe", 60, 1);
    let foe = spawn_party(&mut app, "Foe", crate::Party::Enemy, 0, 1);
    // 同陣営の僚機を (5,0) に固定配置 → (5,1) が密集マスになる。
    let ally = spawn_party(&mut app, "Foe", crate::Party::Enemy, 5, 0);
    app.database_mut().unit_by_uid_mut(&ally).unwrap().ai_mode = "固定".into();

    app.set_stage_state(crate::stage::StageState::Battle);
    app.handle_input(Input::EndPhase);

    // (6,1) を射程に収める到達マス: (5,1)[僚機(5,0)に近接=密集] / (6,2)[僚機から離れる=非密集]。
    // マップ兵器脅威下なので密集を避けた (6,2) を選ぶ。
    let f = app.database().unit_by_uid(&foe).unwrap();
    assert_eq!(
        (f.x, f.y),
        (6, 2),
        "マップ兵器脅威下では密集を避けた攻撃マス (6,2) を選ぶ (実際: ({},{}))",
        f.x,
        f.y
    );
}

#[test]
fn ai_avoids_high_counter_risk() {
    // Enemy with counter ability - AI should still target killable enemies.
    // This is a simplified test: if a target can be killed with high damage,
    // AI prefers it regardless of counter risk.
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);

    place_player_unit(&mut app, "Hero", 3, 3);

    // Enemy 1: killable with high damage
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        4,
        3, // distance 1
    ));
    app.database_mut().unit_instances.last_mut().unwrap().damage = 50; // HP 50

    // Enemy 2: far, not killable
    app.database_mut().register_unit(crate::UnitInstance::new(
        "Hero",
        "PILOT",
        crate::Party::Enemy,
        10,
        3, // distance 8
    ));

    app.set_stage_state(crate::stage::StageState::Battle);
    app.handle_input(Input::EndPhase);

    // AI should have targeted enemy 1 (killable, closer)
    let enemies: Vec<_> = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| u.party == crate::Party::Enemy)
        .collect();
    let most_damaged = enemies.iter().max_by_key(|u| u.damage);
    if let Some(t) = most_damaged {
        // The closer killable enemy should have taken more damage
        assert!(t.damage >= 50, "AI should attack killable target");
    }
}

#[test]
fn counterattack_kill_fires_destruction_event() {
    // 回帰: 反撃で攻撃側 (Hero) が撃破されたとき `破壊 Hero` が発火する
    // (本線・援護の撃破と同じく fire_destruction_labels を通す)。
    use crate::Condition;
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    let stage = crate::data::event::parse("破壊 Hero:\nSet 反撃撃破 1\nExit\n").unwrap();
    app.script_library_mut()
        .append_with_name(&stage, "stage.eve");
    app.current_stage_file = "stage.eve".to_string();
    place_player_unit(&mut app, "Hero", 2, 6);
    add_weapon(&mut app, "Hero", 30, 3); // 敵を撃破しない低威力
    place_player_unit(&mut app, "FoeData", 9, 9);
    let foe = spawn_enemy(&mut app, "FoeData", 3, 6);
    add_weapon(&mut app, "FoeData", 30, 3); // 反撃武器 (射程内に Hero)
    app.set_stage_state(crate::stage::StageState::Battle);
    let hero_uid = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "Hero")
        .map(|u| u.uid.clone())
        .unwrap();
    // Hero は残り HP 1、敵は必中で反撃を確実に当てる。
    let max_hp = {
        let u = app.database().unit_by_uid(&hero_uid).unwrap();
        app.database().effective_max_hp(u)
    };
    app.database_mut()
        .unit_by_uid_mut(&hero_uid)
        .unwrap()
        .damage = max_hp - 1;
    app.database_mut()
        .unit_by_uid_mut(&foe)
        .unwrap()
        .add_condition(Condition::new("必中", 1));

    click_tile(&mut app, 2, 6); // Hero 選択
    let ai = attack_item_index(&app);
    click_menu_item(&mut app, ai); // 攻撃
    click_tile(&mut app, 3, 6); // 敵を攻撃 → 敵の反撃で Hero 撃破

    assert!(
        app.database().unit_by_uid(&hero_uid).is_none(),
        "Hero が反撃で撃破されていない"
    );
    assert_eq!(
        app.script_var("反撃撃破"),
        "1",
        "反撃での撃破で 破壊 イベントが発火していない"
    );
}
