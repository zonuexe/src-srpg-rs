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

/// サンプルシナリオ全体 (data/map/eve) を 1 つの `App` にロードして返す。
/// ステージ起動はしない (テストが party 注入後に `advance_to_next_stage` する)。
///
/// - 全データファイル (pilot/robot/item/sp) を DB に取込む (パースエラーは panic)。
/// - 全 `.map` を basename で `store_map` する (`ChangeMap` が解決できるように)。
///   `active_map_base` と一致する map は active map にしておく。
/// - 全 `.eve` を `append_with_name` で登録 (ファイル間前方参照の解決)。
fn load_sample(root: &Path, active_map_base: &str) -> App {
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
    for p in &map_paths {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        let base = basename_lower(p);
        if let Ok(m) = mapdata::parse(&txt) {
            // 指定 basename の map は active map にしておく。
            if base == active_map_base {
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
    for p in &eve_paths {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        let name = rel_name(root, p);
        match event::parse(&txt) {
            Ok(stmts) => app.script_library_mut().append_with_name(&stmts, &name),
            Err(e) => eve_parse_errors.push(format!("{name}: {e}")),
        }
    }
    assert!(
        eve_parse_errors.is_empty(),
        ".eve パース失敗:\n{}",
        eve_parse_errors.join("\n")
    );
    app
}

/// ロード済み `app` で、登録済みエントリ .eve のステージを `次ステージ` +
/// `advance_to_next_stage` (原典 Continue チェイン相当: プロローグ→スタート発火)
/// で起動し、Talk を Drain で進めて駆動後の `App` を返す。
fn drive_stage(mut app: App, entry_name: &str) -> App {
    app.set_script_var("次ステージ".to_string(), entry_name.to_string());
    assert!(
        app.advance_to_next_stage(),
        "{entry_name} のステージ起動に失敗"
    );
    let mut h = Harness::from_app(app);
    let outcome = h
        .drive(&[Step::Drain(800)])
        .unwrap_or_else(|e| panic!("{entry_name} drive: {e}"));

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
        "[{entry_name}] drive={outcome:?} scene={:?} stage_state={:?} turn={} phase={:?} alive(P/E)={alive_player}/{alive_enemy} units={names:?}",
        app.scene(),
        app.stage_state(),
        app.turn().number,
        app.turn().phase,
    );
    h.app().clone()
}

/// basename (小文字) から、登録名に使う原ケースの相対パスを解決する。
/// 例: "srcｻﾝﾌﾟﾙ.eve" → "SRCｻﾝﾌﾟﾙ.eve" (登録時の append_with_name と一致させる)。
fn entry_rel_name(root: &Path, entry_basename_lower: &str) -> String {
    let mut eves = Vec::new();
    walk(
        root,
        &|p| p.extension().and_then(|s| s.to_str()) == Some("eve"),
        &mut eves,
    );
    eves.iter()
        .find(|p| basename_lower(p) == entry_basename_lower)
        .map(|p| rel_name(root, p))
        .unwrap_or_else(|| panic!("{entry_basename_lower} が見つからない"))
}

/// `load_sample` + `drive_stage` の定番コンボ。エントリと同名 (basename) の
/// `.map` を active map に採用する (ChangeMap を持たないステージ用)。
fn boot_stage(root: &Path, entry_basename_lower: &str) -> App {
    let map_base = format!("{}.map", entry_basename_lower.trim_end_matches(".eve"));
    let app = load_sample(root, &map_base);
    let entry_name = entry_rel_name(root, entry_basename_lower);
    drive_stage(app, &entry_name)
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

/// 全種別のダイアログを進める汎用ドライバ。Menu(Ask) は第1選択肢、Confirm は はい、
/// Talk/WaitClick は次へ、Input はダミー応答、Timer は経過させる。idle で停止。
/// (Harness の Drain は non-cancellable な Menu を進められないため、明示的に駆動する。)
fn drive_all(app: &mut App, max: usize) {
    for _ in 0..max {
        if let Some(kind) = app.pending_dialog().map(|d| d.kind()) {
            match kind {
                "Menu" => app.respond_dialog(1),
                "Input" => app.respond_dialog_text("テスト".to_string()),
                _ => app.respond_dialog(0),
            };
        } else if let Some(t) = app.pending_timer() {
            app.tick(t + 1.0);
        } else {
            break;
        }
    }
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

/// 連続シナリオ (決戦2話/3話) は前話からの持ち越し (セーブ) を前提に味方を Create
/// しない。headless では最小の持ち越し party (キャリバーン=ジェイド) を注入してから
/// 起動し、プロローグ/スタートの命令 (NPC 配置 / Upgrade / Unit / 必敗 設定 等) が
/// runtime error なく実行され、敵/NPC が配置されることを確認する (コマンド網羅の煙試験)。
fn boot_chapter_with_party(root: &Path, entry_basename_lower: &str, map_base: &str) -> App {
    let mut app = load_sample(root, map_base);
    // 持ち越し主力: キャリバーン (ジェイド＝ソウマ)。3話の `Unit ... Rank(ジェイド)` 等が
    // ジェイドを参照するため最低限これを配置しておく (本来はセーブからの引き継ぎ)。
    let setup = event::parse("Create 味方 キャリバーン 0 ジェイド＝ソウマ 23 3 3\n")
        .expect("party setup parse");
    event_runtime::execute(&mut app, &setup).expect("party setup exec");
    let entry = entry_rel_name(root, entry_basename_lower);
    drive_stage(app, &entry)
}

#[test]
fn kessen_chapter2_boots_with_npc_and_enemies() {
    let root = sample_root();
    if !root.exists() {
        eprintln!("[skip] サンプルシナリオ未配置: {}", root.display());
        return;
    }
    // 第2話「沿岸都市を守れ」: テーマは NPC(友軍) 制御 + 必敗シナリオ。
    // スタートで NPC(クルーワッハ) と 敵(宇宙怪獣ギルガス) を配置する。
    let app = boot_chapter_with_party(&root, "決戦！宇宙怪獣2話.eve", "決戦！宇宙怪獣2話.map");
    let names = unit_names(&app);
    assert!(
        names.iter().any(|n| n.contains("クルーワッハ")),
        "NPC(クルーワッハ) が配置されていない: {names:?}"
    );
    assert!(
        names.iter().any(|n| n.contains("ギルガス")),
        "敵(宇宙怪獣ギルガス) が配置されていない: {names:?}"
    );
}

#[test]
fn kessen_chapter3_boots_with_upgrade_and_unit() {
    let root = sample_root();
    if !root.exists() {
        eprintln!("[skip] サンプルシナリオ未配置: {}", root.display());
        return;
    }
    // 第3話「決戦、宇宙怪獣！」: テーマは強制乗り換え/Upgrade/Unit による新型機導入。
    // スタートで Upgrade(ギルガス第1→第2)・Unit(未完成エクスカリバー)・敵配置を行う。
    let app = boot_chapter_with_party(&root, "決戦！宇宙怪獣3話.eve", "決戦！宇宙怪獣3話.map");
    let names = unit_names(&app);
    assert!(
        names
            .iter()
            .any(|n| n.contains("ギルガス") || n.contains("ユニガル")),
        "敵(宇宙怪獣) が配置されていない: {names:?}"
    );
}

#[test]
fn campaign_chain_chapter1_to_chapter2_carries_party() {
    let root = sample_root();
    if !root.exists() {
        eprintln!("[skip] サンプルシナリオ未配置: {}", root.display());
        return;
    }
    // 第1話を起動 (Continue 経路で全配置→Battle)。
    let mut app = boot_stage(&root, "決戦！宇宙怪獣1話.eve");
    assert!(
        unit_names(&app).iter().any(|n| n.contains("キャリバーン")),
        "1話開始時に味方キャリバーンが居ない"
    );

    // 1話の勝利条件 = `接触 Pilot(補給艦リームズ) 敵:` (リームズが宇宙怪獣バルアドに
    // 接触)。headless では リームズを バルアド隣接へ移動して接触イベントを発火させる。
    // (ラベル名の関数 `Pilot(補給艦リームズ)` も解決されるようになった。)
    let (bx, by) = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name.contains("バルアド"))
        .map(|u| (u.x, u.y))
        .expect("バルアドが居ない");
    let riims_uid = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name.contains("リームズ"))
        .map(|u| u.uid.clone())
        .expect("補給艦リームズが居ない");
    // バルアドの左隣 (x-1, y) へ移動 (バルアドは x>=18 なので x-1 は空き)。
    assert!(
        app.database_mut().move_unit(&riims_uid, bx - 1, by),
        "リームズの移動に失敗"
    );
    let riims_idx = app
        .database()
        .unit_instances
        .iter()
        .position(|u| u.uid == riims_uid)
        .unwrap();
    event_runtime::fire_contact_event_labels(&mut app, riims_idx);

    // ステージクリア (Ask クリア/やり直し → Confirm → Ask 終了) → Continue 2話 →
    // エピローグ を全種別ダイアログ駆動で進める。Ask は第1選択肢 (=クリア) を選ぶ。
    drive_all(&mut app, 800);

    // インターミッションで停止していれば「次のステージへ」相当で 2話 を起動。
    if !app.script_var("次ステージ").is_empty() {
        assert!(app.advance_to_next_stage(), "2話への遷移に失敗");
        drive_all(&mut app, 800);
    }

    let names = unit_names(&app);
    // 1話のバルアド (宇宙怪獣) は勝利時の `バルアド撃破` 末尾 `ForEach 敵 Escape` で
    // 全員 off_map (退避) になる。2話に on_map で残っていないことを確認する
    // (= シナリオ側クリーンアップ + ForEach 敵 / Escape が機能している)。
    let balad_on_map = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| u.unit_data_name.contains("バルアド") && !u.off_map)
        .count();
    let alive_enemy = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| u.party == Party::Enemy && !u.off_map)
        .map(|u| u.unit_data_name.as_str())
        .collect::<Vec<_>>();
    eprintln!(
        "[chain] scene={:?} stage_state={:?} stage={:?} balad_on_map={balad_on_map} alive_enemy={alive_enemy:?}",
        app.scene(),
        app.stage_state(),
        app.stage(),
    );

    // 2話に到達: NPC(クルーワッハ) と 敵(ギルガス) が配置されている。
    assert!(
        names.iter().any(|n| n.contains("クルーワッハ")),
        "2話の NPC が配置されていない (チェーン未到達?): {names:?}"
    );
    // 持ち越し検証: 1話の主力キャリバーンが 2話にも引き継がれている。
    assert!(
        names.iter().any(|n| n.contains("キャリバーン")),
        "キャリバーンが2話に持ち越されていない: {names:?}"
    );
    // 遷移時クリーンアップ: 1話のバルアドが 2話に on_map で残っていない
    // (`ForEach 敵 / Escape` による退避が機能している)。
    assert_eq!(
        balad_on_map, 0,
        "1話のバルアドが 2話に on_map で残存している (ForEach 敵 Escape が未機能?)"
    );
    // 2話の本来の敵 (宇宙怪獣ギルガス) が出撃している。
    assert!(
        alive_enemy.iter().any(|n| n.contains("ギルガス")),
        "2話の敵ギルガスが出撃していない: {alive_enemy:?}"
    );
}
