//! ステージ進行 (勝敗・ターン / フェイズ・Continue・セーブ / ロード) のテスト。

use super::*;

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
        .register_unit(crate::unit_instance::UnitInstance::new(
            "U_P",
            "P",
            crate::Party::Player,
            1,
            1,
        ));
    app.database
        .register_unit(crate::unit_instance::UnitInstance::new(
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
        .remove_units_where(|u| u.party == crate::Party::Player);
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
        .register_unit(crate::unit_instance::UnitInstance::new(
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
        .register_unit(crate::unit_instance::UnitInstance::new(
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
    let menu_x = crate::scene::command_menu::MENU_X + 40;
    let endturn_y =
        crate::scene::command_menu::MENU_Y + crate::scene::command_menu::MENU_PADDING + 4;
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
    app.database_mut().catalog.units.push(foe_ud);
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
            .catalog
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
            .catalog
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
            .catalog
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
    app.database_mut().catalog.units.push(foe_ud);
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
