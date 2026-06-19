//! 公式 SRC「サンプルシナリオ」起動スモークテスト / Official sample-scenario smoke.
//!
//! 著作権配慮: シナリオ本文を embed せず、ローカル配置 (`srcall-2_2_33-111106/
//! サンプルシナリオ/`) を **参照のみ** する。フォルダが無ければ skip するので
//! CI / 他環境では無害。再配布パッケージはリポジトリにコミットしない。
//!
//! 目的: 実フォルダ構成のサンプルが、Rust 移植のデータパーサ + `.eve` 実行 +
//! ステージブートストラップを通って「本命ステージ `SRCｻﾝﾌﾟﾙ.eve` の
//! プロローグ→スタートイベント (味方/敵ユニット生成) まで完走」することを
//! 検証する。足りない機能 (未対応コマンド / パース崩壊 / 配置漏れ) を
//! 実エラーで炙り出す最短経路。
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

#[test]
fn sample_scenario_boots_to_start_event() {
    let root = sample_root();
    if !root.exists() {
        eprintln!("[skip] サンプルシナリオ未配置: {}", root.display());
        return;
    }

    let mut app = App::with_rng_seed(0x5A_4D_50_4C_u64);

    // ---- pass 1: データファイル -> DB ----------------------------------
    let mut data_paths = Vec::new();
    walk(
        &root,
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
        let base = basename_lower(p);
        let name = rel_name(&root, p);
        let r: Result<(), String> = match base.as_str() {
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
    eprintln!(
        "[data] pilots={} units={} items={} sp={}",
        app.database().pilots.len(),
        app.database().units.len(),
        app.database().items.len(),
        app.database().special_powers.len(),
    );

    // ---- 本命マップを active map として読み込む ------------------------
    let map_path = root.join("SRCｻﾝﾌﾟﾙ.map");
    let map_txt = loader::decode_text(&fs::read(&map_path).expect("SRCｻﾝﾌﾟﾙ.map 読込"));
    let map = mapdata::parse(&map_txt).expect("SRCｻﾝﾌﾟﾙ.map パース");
    assert_eq!((map.width, map.height), (20, 20), "サンプルマップは 20x20");
    app.database_mut().replace_map(map);

    // ---- pass 2: 全 .eve を登録 (前方参照解決のため先に全登録) ----------
    let mut eve_paths = Vec::new();
    walk(
        &root,
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
        let name = rel_name(&root, p);
        match event::parse(&txt) {
            Ok(stmts) => {
                let pc = app.script_library().statements.len();
                app.script_library_mut().append_with_name(&stmts, &name);
                if basename_lower(p) == "srcｻﾝﾌﾟﾙ.eve" {
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
    let entry_pc = entry_pc.expect("SRCｻﾝﾌﾟﾙ.eve が見つからない");

    // ---- 本命ステージを起動 (verify-archive と同じ順序) ----------------
    // エントリ .eve の top-level (@データロード + コメント) を実行してから
    // ロード末尾ブートストラップで プロローグ→スタート ライフサイクルへ。
    event_runtime::run_from_pc(&mut app, entry_pc).expect("SRCｻﾝﾌﾟﾙ.eve top-level 実行");
    app.bootstrap_stage_after_load(&entry_name);

    // プロローグ / スタートイベントの Talk を順に流して進める。
    let mut h = Harness::from_app(app);
    let outcome = h.drive(&[Step::Drain(600)]).expect("drive prologue+start");
    eprintln!("[drive] outcome={outcome:?}");

    let app = h.app();
    eprintln!(
        "[state] scene={:?} stage={:?} stage_state={:?} turn={} phase={:?}",
        app.scene(),
        app.stage(),
        app.stage_state(),
        app.turn().number,
        app.turn().phase,
    );

    // ---- 検証: スタートイベントが配置したユニットが存在する -------------
    let names: Vec<&str> = app
        .database()
        .unit_instances
        .iter()
        .map(|u| u.unit_data_name.as_str())
        .collect();
    eprintln!("[units] {names:?}");

    let player = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| matches!(u.party, Party::Player))
        .count();
    let enemy = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| matches!(u.party, Party::Enemy))
        .count();

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
    assert!(player >= 1, "味方ユニットが配置されていない ({player})");
    assert!(enemy >= 1, "敵ユニットが配置されていない ({enemy})");

    // スタートイベントまで通れば戦闘 (Battle) + 味方フェイズに到達する。
    // ※ Title→MapView の scene 遷移は現状フロントエンド (src-web archive.rs)
    //   の責務で、core の bootstrap 経路は scene を据え置く。ここでは core が
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
        "味方フェイズに到達していない"
    );
}
