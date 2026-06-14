//! Save → 別 App で Load → Continue chain 続行 / Persistence across the gap.
//!
//! `Continue <次>` で次ステージが予約された状態 (エピローグの Wait Click で
//! suspend 中 = `FlowCont::LoadNextStage` が flow に積まれた状態) で
//! save_json → fresh App に復元できることと、復元後の resume 完了と同時に
//! エンジンが次章へ自動チェインすることを検証する (FLOW_REDESIGN Phase 2:
//! 手動 `advance_to_next_stage` は不要)。
//!
//! `script_library` / `script_vars` / `次ステージ` / `flow` / 既存
//! unit_instances が #[serde(skip)] で落ちないかを実地で検出する。

use src_core::data::event;
use src_core::event_runtime;
use src_core::stage::StageState;
use src_core::test_harness::{DriveOutcome, Harness, Step};
use src_core::App;

const TWO_CHAPTER_EVE: &str = r#"
Goto ch1

ch1:
Stage chapter1
Incr scene_visited
# プレイヤーが ch1 で 1 機残る (save 後の load で復元されるべき)
Pilot "テスト1" t1 男性 一般 BBBC 50 100 120 110 110 100 100
Unit "戦闘機" リアル系 1 4 陸 5 M 2000 300 2200 100 800 80 BBBC
Weapon "戦闘機" "機関砲" 1500 1 4 12 -1
Place "戦闘機" "テスト1" Player 2 2
Continue ch2

ch2:
Stage chapter2
Incr scene_visited
GameClear
Exit

エピローグ:
Incr epilogue_runs
Wait Click
"#;

#[test]
fn save_then_chain_to_next_chapter_in_fresh_app() {
    // ----- Phase 1: ch1 を実行。Continue ch2 が LoadNextStage を積み、
    // エピローグの Wait Click で suspend = 「チェイン途中」のセーブポイント -----
    let h = Harness::from_eve_source(TWO_CHAPTER_EVE).expect("initial execute");

    assert_eq!(h.app().script_var("scene_visited"), "1");
    assert_eq!(h.app().script_var("epilogue_runs"), "1");
    // suspend 中なので予約はまだ消費されていない
    assert_eq!(h.app().script_var("次ステージ"), "ch2");
    assert_eq!(h.app().stage(), "chapter1");
    // ch1 で配置したユニットが 1 機存在する
    assert_eq!(h.app().database().unit_instances.len(), 1);

    // ----- Phase 2: save → 別 App に load -----
    let json = h.app().to_save_json().expect("save_json");
    drop(h); // 元 Harness を破棄して fresh から再開する状況を再現

    let restored = App::from_save_json(&json).expect("from_save_json");

    // 復元後の状態が完全一致しているか
    assert_eq!(restored.script_var("scene_visited"), "1");
    assert_eq!(restored.script_var("epilogue_runs"), "1");
    assert_eq!(restored.script_var("次ステージ"), "ch2");
    assert_eq!(restored.stage(), "chapter1");
    assert_eq!(restored.database().unit_instances.len(), 1);
    assert!(
        restored.script_library().label_pc("ch2").is_some(),
        "ch2 ラベルが script_library に復元されていない"
    );

    // ----- Phase 3: fresh App で resume → エンジンが自動チェイン -----
    // Wait Click 応答でエピローグが完了すると、復元された
    // FlowCont::LoadNextStage が ch2 を自動起動する (手動 advance 不要)。
    let mut h2 = Harness::from_app(restored);
    let outcome = h2.drive(&[Step::Drain(50)]).expect("drain ch2");
    assert_eq!(outcome, DriveOutcome::Finished);

    // 最終確認: 2 章分の進行 / GameClear
    assert_eq!(h2.app().script_var("scene_visited"), "2");
    assert_eq!(h2.app().stage(), "chapter2");
    assert_eq!(h2.app().stage_state(), StageState::Victory);
    assert_eq!(h2.app().script_var("次ステージ"), "");
    // unit はそのまま生存
    assert_eq!(h2.app().database().unit_instances.len(), 1);
}

/// `ScriptLibrary.files` (basename → PC 区間) が save/load で復元されるか
/// を検証。新規 save では `Continue <filename>` の解決に必須なので、
/// `#[serde(default)]` だけでなく実際に round-trip することを確認する。
///
/// stage_a は `エピローグ` の Wait Click で suspend させ、「予約未消費 +
/// LoadNextStage 積載」のまま save する (suspend が無いと完了通知で即
/// チェインが走り stage_b まで進んでしまうため)。
///
/// 著作権配慮: 合成 2 ファイルを inline で。
#[test]
fn script_library_file_entries_survive_save_load() {
    const STAGE_A: &str = r#"
Set saw_a 1
Continue scenes\stage_b.eve
エピローグ:
Wait Click
"#;
    const STAGE_B: &str = r#"
Set saw_b 1
"#;

    let mut app = App::new();
    let stmts_a = event::parse(STAGE_A).expect("parse a");
    let start_a = app.script_library().statements.len();
    app.script_library_mut()
        .append_with_name(&stmts_a, "scenes/stage_a.eve");
    let stmts_b = event::parse(STAGE_B).expect("parse b");
    app.script_library_mut()
        .append_with_name(&stmts_b, "scenes/stage_b.eve");

    // 登録済みファイル数を保存前に控える
    let pre_file_count = app.script_library().files.len();
    let pre_stage_b = app
        .script_library()
        .find_file("stage_b.eve")
        .map(|e| (e.start_pc, e.end_pc, e.basename.clone()));
    assert_eq!(pre_file_count, 2);
    assert!(pre_stage_b.is_some());

    // stage_a を走らせ、エピローグの Wait Click で suspend したところで save
    event_runtime::run_from_pc(&mut app, start_a).expect("run a");
    assert_eq!(app.script_var("saw_a"), "1");
    assert_eq!(app.script_var("次ステージ"), "scenes\\stage_b.eve");
    assert_eq!(
        app.script_var("saw_b"),
        "",
        "suspend 中はまだチェインしない"
    );

    let json = app.to_save_json().expect("save");
    let mut restored = App::from_save_json(&json).expect("load");

    // file_entries が完全に復元されていること
    let post_file_count = restored.script_library().files.len();
    let post_stage_b = restored
        .script_library()
        .find_file("stage_b.eve")
        .map(|e| (e.start_pc, e.end_pc, e.basename.clone()));
    assert_eq!(pre_file_count, post_file_count);
    assert_eq!(pre_stage_b, post_stage_b);

    // 復元された App で resume 完了 → LoadNextStage が basename 解決して
    // stage_b を自動起動する
    assert!(restored.respond_dialog(0));
    assert_eq!(restored.script_var("saw_b"), "1");
    assert_eq!(restored.script_var("次ステージ"), "");
}

/// `pending_timer` が save/load を生き残ること。
///
/// Wait 中に save → 別 App に load → tick で resume するシナリオで、
/// 以前は `#[serde(skip)]` のため timer が失われて script が永久停止
/// していた。`#[serde(default)]` に修正後は round-trip 可能。
#[test]
fn pending_timer_survives_save_load_during_wait() {
    const EVE: &str = r#"
Set before 1
Wait 1.5
Set after 1
"#;
    let stmts = event::parse(EVE).expect("parse");
    let mut app = App::with_rng_seed(0);
    event_runtime::execute(&mut app, &stmts).expect("execute");

    // Wait で suspend している
    assert_eq!(app.script_var("before"), "1");
    assert_eq!(app.script_var("after"), "");
    let timer = app.pending_timer().expect("timer should be set");
    assert!(timer > 0.0);

    // save → 別 App に load → 復元後 timer がそのまま残っているか
    let json = app.to_save_json().expect("save");
    let mut restored = App::from_save_json(&json).expect("load");
    let restored_timer = restored
        .pending_timer()
        .expect("pending_timer が save/load を生き残るべき");
    assert!((restored_timer - timer).abs() < 1e-9);

    // tick で timer 消化 → after が走る
    restored.tick(2.0);
    assert!(restored.pending_timer().is_none());
    assert_eq!(restored.script_var("after"), "1");
}
