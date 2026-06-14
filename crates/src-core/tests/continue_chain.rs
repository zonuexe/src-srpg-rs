//! `Continue <next>` で予約された次ステージを順に走らせるチェインのテスト /
//! Multi-stage chaining via `次ステージ`.
//!
//! VB6 / SRC.Sharp の `ContinueCmd` は次に読むべきシナリオファイル名を
//! システム変数「次ステージ」にセットしてエピローグへジャンプする。
//! 原典では `SRC.StartScenario(次ステージ)` がエピローグ後に呼ばれて
//! 新しいシナリオに切替わる。
//!
//! 本実装 (FLOW_REDESIGN Phase 2) では `Continue` コマンド自身が
//! `FlowCont::LoadNextStage` 継続を積み、現スクリプト (エピローグ含む)
//! 完了後に `advance_to_next_stage` がエンジン内で自動実行される。
//! フロントエンドや test 側のオーケストレーションは不要。
//!
//! 著作権配慮: SRC オリジナルシナリオの内容は一切含まない合成 fixture。
//! ラベル名は 1〜3 章を ascii 短名で識別する。

use src_core::data::event;
use src_core::event_runtime;
use src_core::stage::StageState;
use src_core::test_harness::Harness;
use src_core::App;

/// 3 章連作。各章は `Stage` + `Incr` で進行記録 + `Continue 次の章ラベル` で
/// 終わり、最終章は `GameClear` で締める。`エピローグ` は共通で 1 つだけ
/// 定義し、毎章 Continue で呼ばれて `epilogue_runs` を加算する。
const CHAINED_EVE: &str = r#"
Goto ch1

ch1:
Stage chapter1
Incr scene_visited
Continue ch2

ch2:
Stage chapter2
Incr scene_visited
Continue ch3

ch3:
Stage chapter3
Incr scene_visited
GameClear
Exit

エピローグ:
Incr epilogue_runs
"#;

#[test]
fn continue_chains_through_three_stages_to_game_clear() {
    // 初回 execute: `Goto ch1` → Continue ch2 → エピローグ → 完了。
    // 完了通知 → FlowCont::LoadNextStage → ch2 → (同様に) ch3 → GameClear。
    // suspend する対話が無いため、チェイン全体がこの 1 回で完走する。
    let h = Harness::from_eve_source(CHAINED_EVE).expect("parse + initial execute");

    assert_eq!(h.app().script_var("scene_visited"), "3", "3 章すべて訪問");
    // エピローグ は ch1 と ch2 の Continue で計 2 回。ch3 は GameClear なので呼ばれない。
    assert_eq!(h.app().script_var("epilogue_runs"), "2");
    assert_eq!(h.app().stage(), "chapter3");
    assert_eq!(h.app().stage_state(), StageState::Victory);
    // 次ステージ は最終的にクリアされている
    assert_eq!(h.app().script_var("次ステージ"), "");
}

#[test]
fn continue_chain_needs_no_manual_orchestration() {
    // チェイン完走後に手動の advance_to_next_stage を呼んでも、予約が無いので
    // false (旧実装はフロントエンド/テスト側のループが必須だった)。
    let mut h = Harness::from_eve_source(CHAINED_EVE).expect("parse + initial execute");
    assert!(!h.app_mut().advance_to_next_stage());
}

/// `Continue <filename>` のファイル名指定経路を、合成 2 ファイルで検証する。
/// 著作権配慮: 全 inline、SRC オリジナルコードを含まない。
///
/// シナリオ:
///   stage_a.eve: top-level `Continue scenes\stage_b.eve` → 次ステージ予約
///   stage_b.eve: top-level で `Set stage_b_visited 1` を実行
///
/// stage_a 完了時に `FlowCont::LoadNextStage` が basename `stage_b.eve` から
/// file_entry を引いて start_pc から自動実行する。
const STAGE_A_EVE: &str = r#"
Set stage_a_visited 1
Continue scenes\stage_b.eve
"#;

const STAGE_B_EVE: &str = r#"
Set stage_b_visited 1
Message stage_b_ran
"#;

/// 回帰: エピローグを **持たない** 薄い entry ファイルの `Continue <file>` が、
/// global labels に登録された **別ファイルの** `エピローグ:` へ誤ジャンプしない
/// こと。
///
/// 旧バグ (東方夢想伝): entry (`*スタート.eve`) は `Continue 01.eve` だけ書いた
/// 薄いファイルで自前のエピローグを持たない。`Continue` の旧実装は global
/// `labels.contains_key("エピローグ")` で判定し、`label_pc_scoped` の global
/// フォールバックで **01.eve のエピローグ** へ飛んでいた。結果、本編プロローグ
/// /スタートを飛ばしてエピローグが開幕再生され、味方 0 体で即敗北していた。
///
/// 修正後は `label_pc_within_file` で現ファイル内のみ探し、無ければジャンプ
/// しない。entry → (LoadNextStage) → chapter の プロローグ → スタート が走る。
const ENTRY_NO_EPILOGUE_EVE: &str = r#"
Set entry_ran 1
Continue chapter01.eve
"#;

const CHAPTER_WITH_EPILOGUE_EVE: &str = r#"
マップコマンド ダミー:
Set mapcmd_ran 1
Exit

プロローグ:
Set prologue_ran 1
Exit

スタート:
Set start_ran 1
Exit

エピローグ:
Set epilogue_ran 1
Exit
"#;

#[test]
fn continue_from_epilogue_less_entry_does_not_jump_into_other_files_epilogue() {
    let mut app = App::new();

    let stmts_entry = event::parse(ENTRY_NO_EPILOGUE_EVE).expect("parse entry");
    let start_entry = app.script_library().statements.len();
    app.script_library_mut()
        .append_with_name(&stmts_entry, "scenes/start.eve");
    let stmts_ch = event::parse(CHAPTER_WITH_EPILOGUE_EVE).expect("parse chapter");
    app.script_library_mut()
        .append_with_name(&stmts_ch, "scenes/chapter01.eve");

    event_runtime::run_from_pc(&mut app, start_entry).expect("run entry");

    // entry 本体は走った。
    assert_eq!(app.script_var("entry_ran"), "1");
    // 別ファイル (chapter01) のエピローグへ誤飛びしていない。
    assert_eq!(
        app.script_var("epilogue_ran"),
        "",
        "entry の Continue が chapter01 の エピローグ を誤実行した"
    );
    // 先頭ハンドラ (マップコマンド) の本体も開幕実行されていない。
    assert_eq!(
        app.script_var("mapcmd_ran"),
        "",
        "ステージ起動時に先頭の マップコマンド 本体がインライン実行された"
    );
    // chapter01 の プロローグ は走った (プロローグ優先起動)。
    assert_eq!(
        app.script_var("prologue_ran"),
        "1",
        "chapter01 の プロローグ が起動されていない"
    );
}

#[test]
fn continue_resolves_next_stage_via_file_basename() {
    let mut app = App::new();

    // stage_a → stage_b の順に basename 付き登録。
    let stmts_a = event::parse(STAGE_A_EVE).expect("parse a");
    let start_a = app.script_library().statements.len();
    app.script_library_mut()
        .append_with_name(&stmts_a, "scenes/stage_a.eve");
    let stmts_b = event::parse(STAGE_B_EVE).expect("parse b");
    app.script_library_mut()
        .append_with_name(&stmts_b, "scenes/stage_b.eve");

    // stage_a を top-level 実行。完了通知 → LoadNextStage がファイル名
    // basename で stage_b を解決して自動起動する (手動 advance 不要)。
    event_runtime::run_from_pc(&mut app, start_a).expect("run stage_a");
    assert_eq!(app.script_var("stage_a_visited"), "1");
    assert_eq!(app.script_var("stage_b_visited"), "1");
    assert!(app.messages().iter().any(|m| m == "stage_b_ran"));
    // 予約は消費済み + 現ステージファイルが記録されている。
    assert_eq!(app.script_var("次ステージ"), "");
    assert_eq!(app.current_stage_file(), "scenes\\stage_b.eve");
}
