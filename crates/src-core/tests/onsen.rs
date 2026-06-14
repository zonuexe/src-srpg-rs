//! 温泉旅館 シナリオの E2E 走破テスト / Full-scenario E2E.
//!
//! 著作権配慮: 本テストは `crates/src-web/tests/fixtures/温泉旅館/` を
//! **参照のみ** で、コードを embed しない。
//!
//! 2 通りのフローを検証:
//!
//! - `onsen_authentic_continue_chain_to_confirm`: SRC 原典に近い経路。
//!   library-only load → `スタート.eve` を main entry として起動 →
//!   `Continue Eve\onsen.eve` で 次ステージ予約 → `advance_to_next_stage`
//!   が basename `onsen.eve` を解決して run_from_pc → onsen.eve の
//!   `Confirm プロローグを見ますか？` まで自走。
//!
//! - `onsen_prologue_no_path_completes`: 旧ロジック (multi-プロローグの
//!   重複問題を回避するため `開始地点:` から trigger_label する代替経路)。
//!   `Continue` 解決と独立に確認したい場合の保険。

use std::fs;
use std::path::{Path, PathBuf};

use src_core::data::{event, loader};
use src_core::event_runtime;
use src_core::test_harness::{DriveOutcome, Harness, Step};
use src_core::App;

fn scenario_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("src-web/tests/fixtures/温泉旅館")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else if p.extension().and_then(|s| s.to_str()) == Some("eve") {
            out.push(p);
        }
    }
}

/// 全 .eve を **library-only** で集約し、basename を紐付ける。
fn load_library_only(app: &mut App, root: &Path) -> Vec<(PathBuf, String)> {
    let mut eves: Vec<PathBuf> = Vec::new();
    walk(root, &mut eves);
    eves.sort();
    let mut loaded: Vec<(PathBuf, String)> = Vec::new();
    for p in eves {
        let Ok(bytes) = fs::read(&p) else { continue };
        let text = loader::decode_text(&bytes);
        let stmts = event::parse(&text).expect("parse");
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        app.script_library_mut().append_with_name(&stmts, &name);
        loaded.push((p, name));
    }
    loaded
}

/// SRC 原典挙動: スタート.eve を main entry として起動し、その先の
/// `Continue Eve\onsen.eve` を `advance_to_next_stage` で受けて onsen.eve に
/// チェインし、`Confirm プロローグを見ますか？` で No → 戦闘開始 → Exit
/// まで完走することを検証する。
#[test]
fn onsen_authentic_continue_chain_to_confirm() {
    let root = scenario_root();
    if !root.exists() {
        eprintln!("[skip] 温泉旅館 fixture が無い: {}", root.display());
        return;
    }

    let mut app = App::with_rng_seed(0xDEADBEEF_u64);
    let loaded = load_library_only(&mut app, &root);
    // ルート直下の `スタート.eve` を main entry とする
    let main_name = loaded
        .iter()
        .find(|(p, _)| p.parent().and_then(|d| d.file_name()) == root.file_name())
        .map(|(_, n)| n.clone())
        .expect("ルート直下に entry .eve が無い");
    let main_pc = app
        .script_library()
        .find_file(&main_name)
        .map(|e| e.start_pc)
        .expect("main entry の FileEntry");

    // main entry の top-level (= スタート.eve) を実行。
    // `スタート.eve` の中身は `プロローグ:` ラベル + `Continue Eve\onsen.eve` のみ。
    // Continue が FlowCont::LoadNextStage を積み、script 完了と同時にエンジンが
    // basename `onsen.eve` を解決して自動チェインする (FLOW_REDESIGN Phase 2、
    // 手動 advance_to_next_stage 不要)。
    event_runtime::run_from_pc(&mut app, main_pc).expect("run main");
    assert_eq!(
        app.current_stage_file(),
        "Eve\\onsen.eve",
        "Continue Eve\\onsen.eve が自動チェインしていない"
    );
    assert_eq!(app.script_var("次ステージ"), "", "予約は消費済みのはず");

    // onsen.eve の上半は Set / StopBGM (Stub) で進み、`Confirm
    // プロローグを見ますか？` で停止しているはず。
    assert_eq!(
        app.pending_dialog().map(|d| d.kind()),
        Some("Confirm"),
        "Confirm で停止していない"
    );
    // onsen.eve の Set が反映されている (これが set 宿名 ...)
    assert_eq!(app.script_var("宿名"), "女神の居眠り亭");
    assert_eq!(app.script_var("新エンド"), "0");

    // No 応答 → `Goto 戦闘開始` → `exit` でプロローグ終了。
    // 原典 SRC (`SRC.cs::StartScenario`) と同じく、プロローグ終了後はエンジンが
    // `スタート` を自動発火する (旧実装は手動 Enter 待ちに依存しており、撤去後は
    // auto_progress が担う)。onsen の `スタート` は `changemap Map\map1.map` で
    // マップを読み込むため、Battle 状態 + map ロード済みになる。
    let mut h = Harness::from_app(app);
    let _ = h.drive(&[Step::No]).expect("drive");
    assert_eq!(h.app().script_var("選択"), "0");
    assert_eq!(
        h.app().stage_state(),
        src_core::StageState::Battle,
        "プロローグ後に スタート が自動発火して Battle へ進むはず"
    );
    assert!(
        h.app().database().map.is_some(),
        "スタート の changemap でマップが読み込まれるはず"
    );
}

/// 旧経路: archive.rs と同じく全 .eve を execute → GameOver.eve の prelude
/// を Drain で消化 → `開始地点:` ラベルを直接 trigger → No 応答 → 完走。
///
/// new continue-chain 経路と独立に通る regression セーフティ。
#[test]
fn onsen_prologue_no_path_completes() {
    let root = scenario_root();
    if !root.exists() {
        eprintln!("[skip] 温泉旅館 fixture が無い: {}", root.display());
        return;
    }

    let mut app = App::with_rng_seed(0xDEADBEEF_u64);
    let mut eves: Vec<PathBuf> = Vec::new();
    walk(&root, &mut eves);
    eves.sort();
    for p in &eves {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        let stmts = event::parse(&txt).expect("parse");
        let _ = event_runtime::execute(&mut app, &stmts);
    }
    assert!(
        app.script_library().label_pc("開始地点").is_some(),
        "開始地点 ラベルが script_library に存在しない"
    );

    // GameOver.eve 由来の Talk / Wait を Drain で全消化
    let mut harness = Harness::from_app(app);
    harness
        .drive(&[Step::Drain(128)])
        .expect("drain GameOver prelude");
    assert!(
        matches!(harness.app().modal_gate(), src_core::modal::ModalGate::Open),
        "GameOver.eve の prelude を抜けられない"
    );
    let mut app = harness.app().clone();

    // 初期化値を手動セット (Old test compat — `set 宿名 …` を bypassed)
    app.set_script_var("宿名".to_string(), "女神の居眠り亭".to_string());
    app.set_script_var("新エンド".to_string(), "0".to_string());

    let fired = event_runtime::trigger_label(&mut app, "開始地点");
    assert!(fired, "trigger_label(開始地点) が発火しなかった");
    assert_eq!(app.pending_dialog().map(|d| d.kind()), Some("Confirm"));

    let mut harness = Harness::from_app(app);
    let outcome = harness.drive(&[Step::No]).expect("drive");
    assert_eq!(outcome, DriveOutcome::Finished);
    let app = harness.app();
    assert_eq!(app.script_var("宿名"), "女神の居眠り亭");
    assert_eq!(app.script_var("選択"), "0");
}
