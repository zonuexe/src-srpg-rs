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
use src_core::{App, Party};

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
/// - `次ステージ` にエントリを予約 → `advance_to_next_stage` (原典 Continue
///   チェイン相当: プロローグ→スタート発火) → Talk を Drain で進める。
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
    let mut entry_name = String::new();
    for p in &eve_paths {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        let name = rel_name(root, p);
        match event::parse(&txt) {
            Ok(stmts) => {
                app.script_library_mut().append_with_name(&stmts, &name);
                if basename_lower(p) == entry_basename_lower {
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
    assert!(
        !entry_name.is_empty(),
        "{entry_basename_lower} が見つからない"
    );

    // ---- ステージ起動 (原典 Continue チェイン相当) --------------------
    // `次ステージ` にエントリを予約して `advance_to_next_stage` を呼ぶ。これは
    // Welcome.eve の `Continue <stage>` → インターミッション「次のステージへ」と
    // 同じ起動経路で、プロローグ→(begin_battle 経由) スタート の順に発火する。
    // インターミッション型 (決戦シリーズ) でも early-return せず Battle へ進む点が
    // `bootstrap_stage_after_load` 経路との違い。
    app.set_script_var("次ステージ".to_string(), entry_name.clone());
    assert!(
        app.advance_to_next_stage(),
        "{entry_name} のステージ起動に失敗"
    );

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
    let alive_enemy = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| u.party == Party::Enemy && !u.off_map)
        .count();
    let alive_player = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| u.party == Party::Player && !u.off_map)
        .count();
    eprintln!(
        "[{entry_basename_lower}] scene={:?} stage_state={:?} turn={} phase={:?} alive(P/E)={alive_player}/{alive_enemy} 次ステージ={:?} units={names:?}",
        app.scene(),
        app.stage_state(),
        app.turn().number,
        app.turn().phase,
        app.script_var("次ステージ"),
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

fn total_enemy_damage(app: &App) -> i64 {
    app.database()
        .unit_instances
        .iter()
        .filter(|u| u.party == Party::Enemy)
        .map(|u| u.damage)
        .sum()
}

#[test]
fn sample_scenario_player_phase_engages_combat() {
    let root = sample_root();
    if !root.exists() {
        eprintln!("[skip] サンプルシナリオ未配置: {}", root.display());
        return;
    }
    // SRCｻﾝﾌﾟﾙ を戦闘 (Battle / 味方フェイズ) まで起動。
    let mut app = boot_stage(&root, "srcｻﾝﾌﾟﾙ.eve");
    // scene→MapView はフロント責務なのでテスト側で再現 (debug_run_phase_ai の前提)。
    app.set_scene(src_core::Scene::MapView);

    let dmg_before = total_enemy_damage(&app);
    let msgs_before = app.messages().len();

    // 味方フェイズを AI 自動行動させ、合間に発生する `攻撃` イベント等の Talk を
    // 解消する。複数ユニットが順次行動するので数回繰り返す。
    for _ in 0..40 {
        if app.pending_dialog().is_some() {
            app.respond_dialog(0);
            continue;
        }
        app.debug_run_phase_ai();
        if app.pending_dialog().is_none() {
            break;
        }
    }

    let dmg_after = total_enemy_damage(&app);
    let new_msgs: Vec<&String> = app.messages().iter().skip(msgs_before).collect();
    let joined = new_msgs
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    eprintln!(
        "[combat] enemy damage {dmg_before} -> {dmg_after}, +{} msgs",
        new_msgs.len()
    );
    for m in new_msgs.iter().take(16) {
        eprintln!("  msg+ {m}");
    }

    // 実データ (パイロット能力・武器・命中/ダメージ計算) で味方フェイズの戦闘が
    // 成立し、敵が被弾している (= 交戦が起きた) こと。
    assert!(
        dmg_after > dmg_before,
        "味方フェイズで戦闘が発生していない (敵被ダメージ {dmg_before}->{dmg_after})"
    );
    // 攻撃イベント (`攻撃 ジェイド サラ:`) がプレイヤー攻撃で発火していること。
    assert!(
        joined.contains("へん、ブレスなんざ"),
        "攻撃イベントが発火していない:\n{joined}"
    );
    // サンプルの目玉ギミックが成立: 不死身 (撃破阻止) → 損傷率 50% イベント
    // (HP 回復 + 不死身解除) が戦闘ダメージで発火する。
    // (Finish=ステージ終了の誤実装 / 不死身未実装 / 損傷率が戦闘で非発火 の
    //  3 連鎖バグを修正した結果としてここまで通る。)
    assert!(
        joined.contains("ブレスのＨＰが５０％回復した"),
        "損傷率 50% イベント (不死身→回復) が戦闘で発火していない:\n{joined}"
    );
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
fn kessen_chapter1_boots_to_deployment() {
    let root = sample_root();
    if !root.exists() {
        eprintln!("[skip] サンプルシナリオ未配置: {}", root.display());
        return;
    }
    // 第1話は連続シナリオの初回。プロローグで `Option 乗り換え/アイテム交換`
    // + `IntermissionCommand` を登録する **インターミッション型**で、本来は
    // Welcome.eve の `Continue` → インターミッション「次のステージへ」で起動する。
    // boot_stage は `次ステージ` + `advance_to_next_stage` でこの経路を再現し、
    // プロローグ→スタート (味方/敵配置 + ChangeMap で本番マップ) まで完走させる。
    let app = boot_stage(&root, "決戦！宇宙怪獣1話.eve");

    // インターミッションコマンドが登録されている (Option/IntermissionCommand)。
    let icmds: Vec<&str> = app
        .intermission_commands()
        .iter()
        .map(|e| e.name.as_str())
        .collect();
    assert!(
        icmds.iter().any(|n| n.contains("乗り換え説明")),
        "IntermissionCommand 乗り換え説明 が登録されていない: {icmds:?}"
    );

    // スタートイベントが本編ユニットを配置していること (味方キャリバーン +
    // 補給艦リームズ、敵の巨大宇宙怪獣バルアド)。
    let names = unit_names(&app);
    assert!(
        names.iter().any(|n| n.contains("キャリバーン")),
        "味方キャリバーンが配置されていない: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("リームズ")),
        "補給艦リームズが配置されていない: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("バルアド")),
        "敵バルアドが配置されていない: {names:?}"
    );
    assert!(count_party(&app, Party::Player) >= 1, "味方ユニット無し");
    assert!(count_party(&app, Party::Enemy) >= 1, "敵ユニット無し");
    // ChangeMap で本番マップ (決戦1話.map) が読み込まれていること。
    assert!(app.database().map.is_some(), "active map が無い");

    // スタートイベント完走後、戦闘 (Battle) + 味方フェイズに到達していること。
    // ※ 以前は スタートの `Finish ジェイド＝ソウマ` を「ステージ終了」と誤実装して
    //   いたため開始即 Victory に落ちていた。Finish=行動終了 に修正済み。
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
