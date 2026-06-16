//! 実 SRC シナリオ (.zip / .lzh) を unzip して、各エントリを内容種別に
//! 振り分けて報告するネイティブ CLI。ブラウザに乗せる前に「展開できるか」
//! 「Shift_JIS 名・SJIS テキストが UTF-8 化できるか」を検証するための補助。
//!
//! 使い方:
//!     cargo run -p verify-archive -- <path/to/scenario.zip>
//!
//! 環境変数:
//!     VERIFY_DUMP_NAMES=1     全エントリ名を列挙
//!     VERIFY_DUMP_PATH=<sub>  指定文字列を含むエントリ本文を出力
//!     VERIFY_SMOKE=1          GameDatabase 構築 + entry-point .eve 実行まで
//!                             シナリオ起動スモークテスト (assets/audio はスキップ)

use std::env;
use std::fs;
use std::io::{Cursor, Read};
use std::path::Path;
use std::process::ExitCode;

use src_core::data::{
    event, item, loader, map as mapdata, pilot, special_power, terrain_file, unit,
};
use src_core::{entrypoint, event_runtime, App};

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: verify-archive <path.zip|.lzh>");
        return ExitCode::FAILURE;
    }
    let path = &args[1];
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("read failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let lower = path.to_ascii_lowercase();

    let entries: Vec<(String, Vec<u8>)> = if lower.ends_with(".zip") {
        match unzip(&bytes) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("zip 展開失敗: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else if lower.ends_with(".lzh") || lower.ends_with(".lha") {
        match unlzh(&bytes) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("lzh 展開失敗: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        eprintln!("対応拡張子: .zip, .lzh, .lha");
        return ExitCode::FAILURE;
    };

    println!("--- archive: {} ({} entries) ---", path, entries.len());

    // VERIFY_DUMP_NAMES=1 で全エントリ名と元サイズを列挙する（デバッグ用）。
    let dump_names = env::var("VERIFY_DUMP_NAMES").ok().as_deref() == Some("1");
    // VERIFY_DUMP_PATH=<部分一致> でエントリ本文(SJIS→UTF-8)を出力する（デバッグ用）。
    let dump_path = env::var("VERIFY_DUMP_PATH").ok().unwrap_or_default();

    let mut counts = std::collections::BTreeMap::<&str, usize>::new();
    let mut text_samples = Vec::<(String, String)>::new(); // (kind, name)

    for (name, data) in &entries {
        if dump_names {
            println!("    NAME[{}B]: {}", data.len(), name);
        }
        if !dump_path.is_empty() && name.contains(&dump_path) {
            let text = loader::decode_text(data);
            println!("===== FILE: {} =====", name);
            for (i, line) in text.lines().enumerate() {
                println!("{:4}: {}", i + 1, line);
            }
            println!("===== END {} =====", name);
        }
        let ext = Path::new(name)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let lname = name.to_ascii_lowercase();
        let kind = match (ext.as_str(), lname.as_str()) {
            ("eve", _) => "eve",
            // non_pilot.txt は pilot.txt の prefix で先にマッチさせる必要がある。
            // パイロットスキーマと異なり (Nickname, Bitmap) の 2 フィールド形式のため
            // pilot::parse で「設定に抜けがあります」エラーになる。
            (_, n) if n.ends_with("non_pilot.txt") => "non_pilot.txt",
            (_, n) if n.ends_with("pilot.txt") => "pilot.txt",
            (_, n) if n.ends_with("unit.txt") || n.ends_with("robot.txt") => "unit.txt",
            (_, n) if n.ends_with("item.txt") => "item.txt",
            (_, n) if n.ends_with("sp.txt") || n.ends_with("mind.txt") => "sp.txt",
            (_, n) if n.ends_with("terrain.txt") => "terrain.txt",
            ("txt", _) => "txt",
            ("bmp", _) => "bmp",
            ("png", _) => "png",
            ("mid", _) | ("midi", _) => "mid",
            ("mp3", _) => "mp3",
            ("wav", _) => "wav",
            ("map", _) => "map",
            ("ini", _) => "ini",
            ("", _) => "noext",
            _ => "other",
        };
        *counts
            .entry(Box::leak(kind.to_string().into_boxed_str()))
            .or_default() += 1;

        // 主要テキストはサンプリングしてパース通過率を見る
        match kind {
            "eve" if text_samples.iter().filter(|(k, _)| k == "eve").count() < 3 => {
                let txt = loader::decode_text(data);
                match event::parse(&txt) {
                    Ok(stmts) => println!("  ✓ {name} (.eve, {} statements)", stmts.len()),
                    Err(e) => println!("  ✗ {name} (.eve parse error: {e})"),
                }
                text_samples.push(("eve".into(), name.clone()));
            }
            "pilot.txt" => {
                let txt = loader::decode_text(data);
                let (pilots, errors) = pilot::parse_lenient(&txt);
                if let Some(e) = errors.first() {
                    println!(
                        "  ⚠ {name} (pilots: {}, {} レコードをスキップ: {e})",
                        pilots.len(),
                        errors.len()
                    );
                } else {
                    println!("  ✓ {name} (pilots: {})", pilots.len());
                }
            }
            "unit.txt" => {
                let txt = loader::decode_text(data);
                let (units, errors) = unit::parse_lenient(&txt);
                if let Some(e) = errors.first() {
                    println!(
                        "  ⚠ {name} (units: {}, {} レコードをスキップ: {e})",
                        units.len(),
                        errors.len()
                    );
                } else {
                    println!("  ✓ {name} (units: {})", units.len());
                }
            }
            "item.txt" => {
                let txt = loader::decode_text(data);
                let (items, errors) = item::parse_lenient(&txt);
                if let Some(e) = errors.first() {
                    println!(
                        "  ⚠ {name} (items: {}, {} レコードをスキップ: {e})",
                        items.len(),
                        errors.len()
                    );
                } else {
                    println!("  ✓ {name} (items: {})", items.len());
                }
            }
            "sp.txt" => {
                let txt = loader::decode_text(data);
                let (sps, errors) = special_power::parse_lenient(&txt);
                if let Some(e) = errors.first() {
                    println!(
                        "  ⚠ {name} (special_powers: {}, {} レコードをスキップ: {e})",
                        sps.len(),
                        errors.len()
                    );
                } else {
                    println!("  ✓ {name} (special_powers: {})", sps.len());
                }
            }
            "terrain.txt" => {
                let txt = loader::decode_text(data);
                let (terrains, errors) = terrain_file::parse_lenient(&txt);
                if let Some(e) = errors.first() {
                    println!(
                        "  ⚠ {name} (terrains: {}, {} レコードをスキップ: {e})",
                        terrains.len(),
                        errors.len()
                    );
                } else {
                    println!("  ✓ {name} (terrains: {})", terrains.len());
                }
            }
            _ => {}
        }
    }

    println!("--- summary by kind ---");
    for (k, n) in &counts {
        println!("  {k:12} {n:>6}");
    }

    if env::var("VERIFY_SMOKE").ok().as_deref() == Some("1") {
        println!("--- scenario startup smoke ---");
        if let Err(e) = smoke_test(&entries) {
            eprintln!("smoke FAIL: {e}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}

/// シナリオ起動スモークテスト。`load_into_app` の src-web/wasm 非依存部分を
/// 抜粋・再現する: DB に pilot/unit/item/sp/terrain/map を取り込み、`.eve` を
/// script_library に登録 → entry-point スコア順に `run_from_pc` で実行する。
/// 画像 / 音声 / wasm-bindgen 依存部分はスキップ。
fn smoke_test(entries: &[(String, Vec<u8>)]) -> Result<(), String> {
    let mut app = App::new();
    // VERIFY_ANIMATE=1 でブラウザと同じ animate_ai / animate_battle を有効化し、
    // tick 駆動の AI / 戦闘演出経路 (= ブラウザの進行) を再現する。
    if env::var("VERIFY_ANIMATE").is_ok() {
        app.set_animate_ai(true);
        app.set_animate_battle(true);
    }
    let mut deferred_eves: Vec<(String, String)> = Vec::new();
    let mut deferred_inis: Vec<(String, String)> = Vec::new();
    let mut counts = std::collections::BTreeMap::<&str, usize>::new();

    let is_text = |base: &str, lname: &str| -> bool {
        lname.ends_with(".eve")
            || lname.ends_with(".ini")
            || lname.ends_with(".map")
            || matches!(
                base,
                "pilot.txt"
                    | "non_pilot.txt"
                    | "unit.txt"
                    | "robot.txt"
                    | "item.txt"
                    | "sp.txt"
                    | "mind.txt"
                    | "terrain.txt"
            )
    };
    let basename = |lname: &str| -> String {
        lname
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or(lname)
            .to_string()
    };

    for (name, data) in entries {
        let lname = name.to_ascii_lowercase();
        let base = basename(&lname);
        if !is_text(&base, &lname) {
            continue;
        }
        let txt = loader::decode_text(data);
        if lname.ends_with(".eve") {
            deferred_eves.push((name.clone(), txt));
        } else if lname.ends_with(".ini") {
            deferred_inis.push((name.clone(), txt));
        } else if base == "pilot.txt" {
            let (pilots, errors) = pilot::parse_lenient(&txt);
            *counts.entry("pilot").or_default() += pilots.len();
            app.database_mut().extend_pilots(pilots);
            if let Some(e) = errors.first() {
                eprintln!(
                    "  ⚠ {name} pilot parse: {} レコードをスキップ: {e}",
                    errors.len()
                );
            }
        } else if base == "unit.txt" || base == "robot.txt" {
            let (units, errors) = unit::parse_lenient(&txt);
            *counts.entry("unit").or_default() += units.len();
            app.database_mut().extend_units(units);
            if let Some(e) = errors.first() {
                eprintln!(
                    "  ⚠ {name} unit parse: {} レコードをスキップ: {e}",
                    errors.len()
                );
            }
        } else if base == "item.txt" {
            let (items, errors) = item::parse_lenient(&txt);
            *counts.entry("item").or_default() += items.len();
            app.database_mut().extend_items(items);
            if let Some(e) = errors.first() {
                eprintln!(
                    "  ⚠ {name} item parse: {} レコードをスキップ: {e}",
                    errors.len()
                );
            }
        } else if base == "sp.txt" || base == "mind.txt" {
            let (sps, errors) = special_power::parse_lenient(&txt);
            *counts.entry("sp").or_default() += sps.len();
            app.database_mut().extend_special_powers(sps);
            if let Some(e) = errors.first() {
                eprintln!(
                    "  ⚠ {name} sp parse: {} レコードをスキップ: {e}",
                    errors.len()
                );
            }
        } else if base == "terrain.txt" {
            let (terrains, errors) = terrain_file::parse_lenient(&txt);
            *counts.entry("terrain").or_default() += terrains.len();
            app.database_mut().extend_terrains(terrains);
            if let Some(e) = errors.first() {
                eprintln!(
                    "  ⚠ {name} terrain parse: {} レコードをスキップ: {e}",
                    errors.len()
                );
            }
        } else if lname.ends_with(".map") {
            if let Ok(m) = mapdata::parse(&txt) {
                *counts.entry("map").or_default() += 1;
                app.database_mut().store_map(base.clone(), m);
            }
        }
    }
    for (k, n) in &counts {
        println!("  ✓ DB: {k:8} {n:>6}");
    }

    // .ini を script_library に登録（自動実行はしない、Require 用の登録のみ）。
    for (name, txt) in &deferred_inis {
        if let Ok(stmts) = event::parse(txt) {
            app.script_library_mut().append_with_name(&stmts, name);
        }
    }

    let analysis = entrypoint::analyze(entries);
    if let Some(best) = analysis.best() {
        println!(
            "  → entry-point: {} (候補 {} 件, スコア降順)",
            best,
            analysis.candidates.len()
        );
    } else {
        println!("  → entry-point なし (データ専用?)");
    }
    let ep_score: std::collections::HashMap<&str, i32> = analysis
        .candidates
        .iter()
        .map(|c| (c.name.as_str(), c.score))
        .collect();
    deferred_eves.sort_by(|a, b| {
        let sa = ep_score.get(a.0.as_str()).copied().unwrap_or(i32::MIN);
        let sb = ep_score.get(b.0.as_str()).copied().unwrap_or(i32::MIN);
        sb.cmp(&sa).then_with(|| {
            a.0.matches(['/', '\\'])
                .count()
                .cmp(&b.0.matches(['/', '\\']).count())
        })
    });

    let mut eve_entries: Vec<(String, usize, usize)> = Vec::new();
    let mut eve_parse_errors = 0;
    for (name, txt) in &deferred_eves {
        match event::parse(txt) {
            Ok(stmts) => {
                let len = stmts.len();
                let pc = app.script_library().statements.len();
                app.script_library_mut().append_with_name(&stmts, name);
                eve_entries.push((name.clone(), pc, len));
            }
            Err(e) => {
                eve_parse_errors += 1;
                eprintln!("  ⚠ {name} parse: {e}");
            }
        }
    }
    println!(
        "  ✓ .eve 登録 {} 本 (parse errors: {})",
        eve_entries.len(),
        eve_parse_errors
    );

    // エントリポイントのみ run_from_pc で実行。サブ .eve はロード時に実行しない。
    let entry_name = analysis.best();
    let mut run_errors = 0;
    let mut run_count = 0;
    for (name, pc, _len) in &eve_entries {
        if entry_name == Some(name.as_str()) {
            if let Err(e) = event_runtime::run_from_pc(&mut app, *pc) {
                run_errors += 1;
                eprintln!("  ⚠ {name} runtime: {e}");
            } else {
                run_count += 1;
            }
        }
    }
    println!(
        "  ✓ .eve 実行 {} 本 (runtime errors: {})",
        run_count, run_errors
    );

    // src-web (archive.rs) と同じロード末尾ブートストラップ:
    // `Stage` / `Continue` を使わないシナリオでもエントリ .eve をステージ
    // ファイルとして `スタート` 発火 → 味方フェイズ開始まで進める。
    if let Some(best) = analysis.best() {
        app.bootstrap_stage_after_load(best);
        println!("  → ステージブートストラップ: {best}");
    }
    app.on_script_completed();

    println!("--- final app state ---");
    println!("  scene:           {:?}", app.scene());
    println!("  stage:           {:?}", app.stage());
    println!("  stage_state:     {:?}", app.stage_state());
    println!("  turn.number:     {}", app.turn().number);
    println!("  turn.phase:      {:?}", app.turn().phase);
    println!("  pilots in DB:    {}", app.database().pilots.len());
    println!("  units in DB:     {}", app.database().units.len());
    println!("  messages:        {}", app.messages().len());
    for m in app.messages().iter().take(8) {
        println!("    | {m}");
    }
    if app.messages().len() > 8 {
        println!("    | ... ({} more)", app.messages().len() - 8);
    }

    // VERIFY_DRIVE=1: ブラウザの「クリック連打 + tick」相当でシナリオを駆動し、
    // 各 dialog 種別・選択・ユニット生成 (Party 別) を観測する。
    // Ask (Menu) には最初の選択肢 (1) を選ぶ — キャラ選択で味方を 1 体作る想定。
    if env::var("VERIFY_DRIVE").is_ok() {
        use src_core::dialog::PendingDialog;
        // VERIFY_ASK=<n> で Menu/Ask の選択肢を変更 (既定 1)。0 = キャンセル相当。
        let ask_choice: u32 = env::var("VERIFY_ASK")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1);
        println!("--- drive (click-through, Ask→{ask_choice}) ---");
        let mut last_stage_file = app.current_stage_file().to_string();
        println!("  current_stage_file(start)={last_stage_file:?}");
        let mut last_units = app.database().unit_instances.len();
        // VERIFY_VAR=a,b,c: 指定した script_var を各ステップでダンプする (ブラウザ `__srcVar`
        // のヘッドレス相当)。D スパロボ戦記の 敵配置数/敵候補/配置場所[7]/味方平均レベル 等の triage 用。
        let verify_vars: Vec<String> = env::var("VERIFY_VAR")
            .ok()
            .map(|s| {
                s.split(',')
                    .map(|n| n.trim().to_string())
                    .filter(|n| !n.is_empty())
                    .collect()
            })
            .unwrap_or_default();
        let dump_vars = |app: &src_core::App, step: usize| {
            if verify_vars.is_empty() {
                return;
            }
            let parts: Vec<String> = verify_vars
                .iter()
                .map(|n| format!("{n}={:?}", app.script_var(n)))
                .collect();
            println!("  [{step}] vars: {}", parts.join(" "));
        };
        for step in 0..400 {
            let state = app.stage_state();
            dump_vars(&app, step);
            if matches!(state, src_core::stage::StageState::Defeat) {
                println!("  [{step}] stage_state={state:?} → 停止");
                break;
            }
            if let Some(d) = app.pending_dialog().cloned() {
                let (kind, snippet, choice) = match &d {
                    PendingDialog::Talk { speaker, body } => {
                        ("Talk", format!("{speaker}: {}", trunc(body, 40)), 0)
                    }
                    PendingDialog::WaitClick => ("WaitClick", String::new(), 0),
                    PendingDialog::Confirm { question, .. } => ("Confirm", trunc(question, 40), 0),
                    PendingDialog::Menu {
                        prompt, options, ..
                    } => (
                        "Menu/Ask",
                        format!("{} [{}]", trunc(prompt, 30), options.join("|")),
                        ask_choice,
                    ),
                    PendingDialog::Input { prompt, .. } => ("Input", trunc(prompt, 40), 0),
                };
                // Menu(Ask) はブラウザの「選択肢をクリック」経路を模して
                // handle_input(ClickAt) で確定する (プレーン Menu のクリック選択
                // 回帰を実シナリオで確認)。1 行 prompt 前提で選択肢行の y を算出。
                // クリックが外れた場合 (2 行 prompt 等) は respond で確定にフォール。
                if matches!(d, PendingDialog::Menu { .. }) && choice >= 1 {
                    // CANVAS 480: 選択肢開始 y=304、行高 20px、行中央 +10。
                    let cy = 304 + (choice as i32 - 1) * 20 + 10;
                    println!("  [{step}] {kind} {snippet} → click(120,{cy})→choice {choice}");
                    app.handle_input(src_core::Input::ClickAt { x: 120, y: cy });
                    if app.pending_dialog().is_some() {
                        // クリックが外れた: 確実に確定させる。
                        app.respond_dialog(choice);
                    }
                } else {
                    println!("  [{step}] {kind} {snippet} → respond({choice})");
                    app.respond_dialog(choice);
                }
            } else if let Some(t) = app.pending_timer() {
                println!("  [{step}] Timer({t:.1}) → tick(big)");
                app.tick(100.0);
            } else if matches!(app.scene(), src_core::Scene::Intermission) {
                let count = app.intermission_item_count();
                let last = count.saturating_sub(1);
                println!(
                    "  [{step}] Intermission (items={count}) → 次のステージへ (cursor={last})"
                );
                app.set_intermission_cursor(last);
                app.handle_input(src_core::Input::Advance);
            } else if matches!(state, src_core::stage::StageState::Battle) {
                // 味方フェーズで idle = プレイヤー操作待ち。ブラウザの「画面を
                // 進めるだけ」を模して EndPhase でターンを送り、tick で敵 AI を
                // 走らせる。チャプター cascade を観測する。
                let map_info = match app.database().map.as_ref() {
                    Some(m) => format!("map {}x{}", m.width, m.height),
                    None => "map=None(gray!)".to_string(),
                };
                let on_map = app
                    .database()
                    .unit_instances
                    .iter()
                    .filter(|u| !u.off_map)
                    .count();
                println!(
                    "  [{step}] Battle idle (turn {} {:?}) {map_info} on_map={on_map}/{} cursor={:?} scene={:?} → EndPhase",
                    app.turn().number,
                    app.turn().phase,
                    app.database().unit_instances.len(),
                    app.map_cursor(),
                    app.scene(),
                );
                app.handle_input(src_core::Input::EndPhase);
                for _ in 0..50 {
                    app.tick(1.0);
                }
            } else {
                println!(
                    "  [{step}] idle (no dialog/timer), scene={:?} stage_state={state:?}",
                    app.scene()
                );
                break;
            }
            let n = app.database().unit_instances.len();
            if n != last_units {
                println!("    units {last_units}→{n}:");
                for u in &app.database().unit_instances {
                    println!(
                        "      {} [{:?}] @({},{})",
                        u.unit_data_name, u.party, u.x, u.y
                    );
                }
                last_units = n;
            }
            let csf = app.current_stage_file().to_string();
            if csf != last_stage_file {
                println!("    current_stage_file: {last_stage_file:?} → {csf:?}");
                last_stage_file = csf;
            }
        }
        println!("--- drive final ---");
        dump_vars(&app, 999);
        println!("  stage_state: {:?}", app.stage_state());
        println!("  units: {}", app.database().unit_instances.len());
        for u in &app.database().unit_instances {
            println!("    {} [{:?}]", u.unit_data_name, u.party);
        }
        for m in app.messages().iter().rev().take(6) {
            println!("    msg| {m}");
        }
    }

    Ok(())
}

/// 先頭 `max` 文字 (char 単位) に丸めて改行を空白化する (drive ログ用)。
fn trunc(s: &str, max: usize) -> String {
    let one_line: String = s.chars().map(|c| if c == '\n' { ' ' } else { c }).collect();
    if one_line.chars().count() <= max {
        one_line
    } else {
        let head: String = one_line.chars().take(max).collect();
        format!("{head}…")
    }
}

fn unzip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = loader::decode_text(entry.name_raw());
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        out.push((name, buf));
    }
    Ok(out)
}

/// LZH / LHA アーカイブを展開。delharc クレートに丸投げ。
fn unlzh(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    use delharc::LhaDecodeReader;
    let mut reader = LhaDecodeReader::new(Cursor::new(bytes))
        .map_err(|e| format!("LZH ヘッダ読み取り失敗: {e}"))?;
    let mut out = Vec::new();
    loop {
        let header = reader.header();
        let is_dir = header.parse_pathname().to_string_lossy().ends_with('/');
        if !is_dir && reader.is_decoder_supported() {
            let name = decode_lzh_filename(header);
            let mut buf = Vec::with_capacity(header.original_size as usize);
            reader
                .read_to_end(&mut buf)
                .map_err(|e| format!("LZH デコード失敗: {e}"))?;
            out.push((name, buf));
        } else if !is_dir && !reader.is_decoder_supported() {
            let _ = reader.read_to_end(&mut Vec::new()).map_err(|_| ());
        }
        if !reader
            .next_file()
            .map_err(|e| format!("LZH 次エントリ取得失敗: {e}"))?
        {
            break;
        }
    }
    Ok(out)
}

const EXT_HEADER_FILENAME: u8 = 0x01;
const EXT_HEADER_PATH: u8 = 0x02;

// NOTE: 同等の実装が `crates/src-web/src/archive.rs` にも存在する。
// src-web は wasm32 専用 (cdylib)、verify-archive はネイティブ CLI のため
// 共通化先 (src-core) に置けず重複を維持している。仕様変更時は両方を更新。
fn decode_lzh_filename(header: &delharc::LhaHeader) -> String {
    let mut path_bytes: Vec<u8> = Vec::new();
    let mut filename_bytes: Option<&[u8]> = None;

    for hdr in header.iter_extra() {
        match hdr {
            [EXT_HEADER_PATH, data @ ..] => {
                path_bytes.extend(data.iter().map(|&b| if b == 0xFF { b'/' } else { b }));
                let needs_sep = !path_bytes.is_empty()
                    && !path_bytes.ends_with(b"/")
                    && !path_bytes.ends_with(b"\\");
                if needs_sep {
                    path_bytes.push(b'/');
                }
            }
            [EXT_HEADER_FILENAME, data @ ..] => {
                filename_bytes = Some(data);
            }
            _ => {}
        }
    }

    // EXT_HEADER_FILENAME がなければ Level 0/1 の header.filename をベース名として採用。
    // EXT_HEADER_PATH (ディレクトリ) と header.filename (ベース名) の両方を持つ
    // アーカイブが実在するため、両者を必ず連結する。
    let basename = filename_bytes.unwrap_or(header.filename.as_ref());
    path_bytes.extend_from_slice(basename);

    if path_bytes.is_empty() {
        return header.parse_pathname_to_str();
    }

    let sanitized: Vec<u8> = path_bytes
        .iter()
        .map(|&b| if b == 0xFF { b'/' } else { b })
        .collect();
    let s = loader::decode_text(&sanitized);
    s.replace('\\', "/")
}
