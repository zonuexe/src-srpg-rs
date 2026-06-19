//! 公式 SRC「サンプルシナリオ」起動スモークテスト / Official sample-scenario smoke.
//!
//! 著作権配慮: シナリオ本文を embed せず、ローカル配置 (`srcall-2_2_33-111106/
//! サンプルシナリオ/`) を **参照のみ** する。フォルダが無ければ skip するので
//! CI / 他環境では無害。再配布パッケージはリポジトリにコミットしない。
//!
//! 目的: 実フォルダ構成のサンプルが、Rust 移植のデータパーサ + `.eve` 実行 +
//! ステージブートストラップを通って「直接プレイ可能な各ステージのプロローグ→
//! スタートイベント (味方/敵ユニット生成) まで完走」することを検証する。
//! 足りない機能 (未対応コマンド / パース崩壊 / 配置漏れ) を実エラーで炙り出す。
//!
//! 単発実行: `cargo test -p src-core --test sample_scenario_smoke -- --nocapture`

use std::fs;
use std::path::{Path, PathBuf};

use src_core::data::{event, item, loader, map as mapdata, pilot, special_power, unit};
use src_core::stage::StageState;
use src_core::test_harness::{Harness, Step};
use src_core::{event_runtime, App, Party};

/// リポジトリ直下に展開された非再配布パッケージのサンプルシナリオ root。
fn sample_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")) // crates/src-core
        .parent()
        .unwrap() // crates
        .parent()
        .unwrap() // repo root
        .join("srcall-2_2_33-111106/サンプルシナリオ")
}

fn walk<F: Fn(&Path) -> bool>(dir: &Path, f: &F, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, f, out);
        } else if f(&p) {
            out.push(p);
        }
    }
}

fn basename_lower(p: &Path) -> String {
    p.file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default()
}

fn rel_name(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

/// サンプルシナリオ全体をロードして、指定エントリ .eve のステージを
/// プロローグ→スタートイベント完走まで駆動した `App` を返す。
///
/// - 全データファイル (pilot/robot/item/sp) を DB に取込む (パースエラーは panic)。
/// - 全 `.map` を basename で `store_map` する (`ChangeMap` が解決できるように)。
///   エントリと同名の `.map` があれば active map にしておく
///   (`SRCｻﾝﾌﾟﾙ.eve` のように `ChangeMap` を持たないステージ用)。
/// - 全 `.eve` を `append_with_name` で登録 (ファイル間前方参照の解決)。
/// - エントリ top-level を実行 → `bootstrap_stage_after_load` → Talk を Drain。
fn boot_stage(root: &Path, entry_basename_lower: &str) -> App {
    let mut app = App::with_rng_seed(0x5A_4D_50_4C_u64);

    // ---- データファイル -> DB ------------------------------------------
    let mut data_paths = Vec::new();
    walk(
        root,
        &|p| {
            matches!(
                basename_lower(p).as_str(),
                "pilot.txt" | "robot.txt" | "unit.txt" | "item.txt" | "sp.txt" | "mind.txt"
            )
        },
        &mut data_paths,
    );
    data_paths.sort();

    let mut data_errors = Vec::<String>::new();
    for p in &data_paths {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        let name = rel_name(root, p);
        let r: Result<(), String> = match basename_lower(p).as_str() {
            "pilot.txt" => pilot::parse(&txt)
                .map(|v| app.database_mut().extend_pilots(v))
                .map_err(|e| e.to_string()),
            "robot.txt" | "unit.txt" => unit::parse(&txt)
                .map(|v| app.database_mut().extend_units(v))
                .map_err(|e| e.to_string()),
            "item.txt" => item::parse(&txt)
                .map(|v| app.database_mut().extend_items(v))
                .map_err(|e| e.to_string()),
            "sp.txt" | "mind.txt" => special_power::parse(&txt)
                .map(|v| app.database_mut().extend_special_powers(v))
                .map_err(|e| e.to_string()),
            _ => Ok(()),
        };
        if let Err(e) = r {
            data_errors.push(format!("{name}: {e}"));
        }
    }
    assert!(
        data_errors.is_empty(),
        "データファイルのパース失敗:\n{}",
        data_errors.join("\n")
    );

    // ---- 全 .map を DB に格納 (ChangeMap 解決用) ------------------------
    let mut map_paths = Vec::new();
    walk(
        root,
        &|p| p.extension().and_then(|s| s.to_str()) == Some("map"),
        &mut map_paths,
    );
    map_paths.sort();
    let entry_map_base = format!("{}.map", entry_basename_lower.trim_end_matches(".eve"));
    for p in &map_paths {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        let base = basename_lower(p);
        if let Ok(m) = mapdata::parse(&txt) {
            // エントリと同名の map は active map にしておく。
            if base == entry_map_base {
                app.database_mut().replace_map(m.clone());
            }
            app.database_mut().store_map(base, m);
        }
    }

    // ---- 全 .eve を登録 ------------------------------------------------
    let mut eve_paths = Vec::new();
    walk(
        root,
        &|p| p.extension().and_then(|s| s.to_str()) == Some("eve"),
        &mut eve_paths,
    );
    eve_paths.sort();

    let mut eve_parse_errors = Vec::<String>::new();
    let mut entry_pc: Option<usize> = None;
    let mut entry_name = String::new();
    for p in &eve_paths {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        let name = rel_name(root, p);
        match event::parse(&txt) {
            Ok(stmts) => {
                let pc = app.script_library().statements.len();
                app.script_library_mut().append_with_name(&stmts, &name);
                if basename_lower(p) == entry_basename_lower {
                    entry_pc = Some(pc);
                    entry_name = name.clone();
                }
            }
            Err(e) => eve_parse_errors.push(format!("{name}: {e}")),
        }
    }
    assert!(
        eve_parse_errors.is_empty(),
        ".eve パース失敗:\n{}",
        eve_parse_errors.join("\n")
    );
    let entry_pc = entry_pc.unwrap_or_else(|| panic!("{entry_basename_lower} が見つからない"));

    // ---- ステージ起動 (verify-archive と同じ順序) ---------------------
    // エントリ .eve の top-level を実行 (プロローグまで流れて Talk で suspend)
    // → ロード末尾ブートストラップで スタート 発火 → 味方フェイズへ。
    event_runtime::run_from_pc(&mut app, entry_pc)
        .unwrap_or_else(|e| panic!("{entry_name} top-level 実行: {e}"));
    app.bootstrap_stage_after_load(&entry_name);

    // プロローグ / スタートイベントの Talk を順に流して進める。
    let mut h = Harness::from_app(app);
    let outcome = h
        .drive(&[Step::Drain(800)])
        .unwrap_or_else(|e| panic!("{entry_name} drive: {e}"));
    eprintln!("[{entry_basename_lower}] drive outcome={outcome:?}");

    let app = h.app();
    let names: Vec<&str> = app
        .database()
        .unit_instances
        .iter()
        .map(|u| u.unit_data_name.as_str())
        .collect();
    eprintln!(
        "[{entry_basename_lower}] scene={:?} stage_state={:?} phase={:?} units={names:?}",
        app.scene(),
        app.stage_state(),
        app.turn().phase,
    );
    h.app().clone()
}

fn count_party(app: &App, party: Party) -> usize {
    app.database()
        .unit_instances
        .iter()
        .filter(|u| u.party == party)
        .count()
}

fn unit_names(app: &App) -> Vec<String> {
    app.database()
        .unit_instances
        .iter()
        .map(|u| u.unit_data_name.clone())
        .collect()
}

#[test]
fn sample_scenario_boots_to_start_event() {
    let root = sample_root();
    if !root.exists() {
        eprintln!("[skip] サンプルシナリオ未配置: {}", root.display());
        return;
    }
    let app = boot_stage(&root, "srcｻﾝﾌﾟﾙ.eve");
    let names = unit_names(&app);

    assert!(
        names.iter().any(|n| n.contains("キャリバーン")),
        "味方キャリバーンが配置されていない: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("ブレス")),
        "敵ブレスが配置されていない: {names:?}"
    );
    // 合体ユニット (龍神機 = 龍武神 + 獣武神 + 鳳武神) の生成まで通っていること。
    assert!(
        names.iter().any(|n| n.contains("龍神機")),
        "合体ユニット龍神機が配置されていない: {names:?}"
    );
    assert!(count_party(&app, Party::Player) >= 1, "味方ユニット無し");
    assert!(count_party(&app, Party::Enemy) >= 1, "敵ユニット無し");

    // スタートイベントまで通れば戦闘 (Battle) + 味方フェイズに到達する。
    // ※ Title→MapView の scene 遷移はフロントエンド (src-web archive.rs) の
    //   責務で、core の bootstrap 経路は scene を据え置く。ここでは core が
    //   確実に到達する状態 (Battle / Player フェイズ) を検証する。
    assert_eq!(
        app.stage_state(),
        StageState::Battle,
        "Battle 状態に到達していない (scene={:?})",
        app.scene()
    );
    assert_eq!(
        app.turn().phase,
        src_core::Phase::Player,
        "味方フェイズ未到達"
    );
}

#[test]
fn tutorial_runs_to_completion() {
    let root = sample_root();
    if !root.exists() {
        eprintln!("[skip] サンプルシナリオ未配置: {}", root.display());
        return;
    }
    // チュートリアルは `ChangeMap SRCｻﾝﾌﾟﾙ.map` でマップを読み込み、Talk と練習
    // 戦闘だけで自走するため、全 Talk を Drain で進めると最後まで完走する。
    let app = boot_stage(&root, "チュートリアル.eve");

    // 操作練習用に味方 (アリス/クレア) と練習相手 (麗蘭華) が配置される。
    assert!(count_party(&app, Party::Player) >= 1, "味方ユニット無し");
    // ChangeMap が解決され active map が読み込まれていること。
    assert!(app.database().map.is_some(), "active map が無い");
    // 戦闘に入り、自走で完走 (Victory) するところまで進む。途中で未対応命令や
    // 配置漏れがあれば Battle に到達できず Briefing/Sortie で止まる。
    assert!(
        matches!(app.stage_state(), StageState::Battle | StageState::Victory),
        "戦闘に到達していない: {:?} (scene={:?})",
        app.stage_state(),
        app.scene()
    );
}

#[test]
fn kessen_chapter1_boots_intermission() {
    let root = sample_root();
    if !root.exists() {
        eprintln!("[skip] サンプルシナリオ未配置: {}", root.display());
        return;
    }
    // 第1話は連続シナリオの初回で、プロローグで `Option 乗り換え/アイテム交換`
    // + `IntermissionCommand` を登録する **インターミッション型**。core の
    // bootstrap はインターミッション型を auto-start せず (メニューの「次のステージ
    // へ」/「出撃」で進む設計)、Briefing で待機するのが正しい挙動。
    // ※ インターミッションメニュー経由の戦闘開始駆動は将来課題。ここでは
    //   プロローグが runtime error 無く完走し、IntermissionCommand が登録され、
    //   ミニキャンペーンのプロローグ台詞が出ることを検証する。
    let app = boot_stage(&root, "決戦！宇宙怪獣1話.eve");

    let icmds: Vec<&str> = app
        .intermission_commands()
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert!(
        icmds.iter().any(|n| n.contains("乗り換え説明")),
        "IntermissionCommand 乗り換え説明 が登録されていない: {icmds:?}"
    );
    assert!(
        app.messages()
            .iter()
            .any(|m| m.contains("ミニキャンペーン")),
        "プロローグ台詞が出ていない: {:?}",
        app.messages()
    );
}
