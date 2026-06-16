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
    // VERIFY_ENTRY で entry-point を上書きできる (D triage: root シナリオ起点で駆動)。
    // 形式: 部分一致文字列、または `@N`(登録一覧の 1 始まり index)。
    // 日本語 env 値はシェル/nix 経由で文字化けしマッチしないことがあるため index 形式を併用。
    let effective_entry: Option<String> = match env::var("VERIFY_ENTRY") {
        Ok(sub) if !sub.trim().is_empty() => {
            let sub = sub.trim();
            let found = match sub.strip_prefix('@').and_then(|n| n.parse::<usize>().ok()) {
                Some(idx) if idx >= 1 => eve_entries.get(idx - 1).map(|(n, _, _)| n.clone()),
                _ => eve_entries
                    .iter()
                    .map(|(n, _, _)| n.clone())
                    .find(|n| n.contains(sub)),
            };
            match &found {
                Some(f) => println!("  → entry-point 上書き (VERIFY_ENTRY={sub}): {f}"),
                None => {
                    println!("  → VERIFY_ENTRY={sub} に一致する .eve なし (既定を使用)。登録 .eve 一覧 (1始まり):");
                    for (i, (n, _, _)) in eve_entries.iter().enumerate() {
                        println!("      @{} {n}", i + 1);
                    }
                }
            }
            found.or_else(|| analysis.best().map(String::from))
        }
        _ => analysis.best().map(String::from),
    };
    let entry_name = effective_entry.as_deref();
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
    if let Some(best) = effective_entry.as_deref() {
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
        // VERIFY_AUTOSTART=1: メニューに 【開始】/【START】/【決定】 等の "進める" 選択肢が
        // あればそれを選ぶ (タイトル/難易度設定メニューを抜けてゲームを進める。既定は ask_choice
        // で従来挙動を維持＝他シナリオの smoke に非影響)。
        let autostart = env::var("VERIFY_AUTOSTART").is_ok();
        // VERIFY_AUTOPLAY=1: Battle の味方フェイズで味方を AI で自動行動させてから
        // EndPhase する (前進・攻撃して戦闘を勝敗まで通す)。敵が待機配置で動かない
        // シナリオ (D スパロボ戦記) でも戦闘を実際に走らせて検証できる。
        let autoplay = env::var("VERIFY_AUTOPLAY").is_ok();
        // VERIFY_SEAT_DEBUG=1: キャラメイキングを経ず (出撃導線をヘッドレス完走できない D 用)、
        // パイロット不在のまま出撃した味方機に DB パイロットを検証用に乗せて戦闘を成立させる。
        // キャラメイキングはスキップし、Battle 開始時に debug_seat_db_pilot を一度呼ぶ。
        let seat_debug = env::var("VERIFY_SEAT_DEBUG").is_ok();
        // VERIFY_CMAKING_EXIT=1: キャラメイキングを目標人数まで進めた後、`データロード`
        // 経路（唯一の Break）で CMaking を正規に抜ける。VFS に最小 .src を置き
        // `__verify_loadfile` をセットして LoadFileDialog に返させ、パイロットリスト Ask を
        // キャンセル→RemovePilot→Break で抜ける。
        let cmaking_exit_drive = env::var("VERIFY_CMAKING_EXIT").is_ok();
        let menu_choice = move |options: &[String]| -> u32 {
            if autostart {
                // ① 【開始】/【START】 等の括弧付き進行アクション (タイトル/難易度設定を抜ける)。
                if let Some(i) = options.iter().position(|o| {
                    o.contains('【')
                        && ["開始", "START", "実行", "はい"]
                            .iter()
                            .any(|k| o.contains(k))
                }) {
                    return i as u32 + 1;
                }
                // ② 決定/確定 (機体確定の `決定する`)。閲覧系 `確認する` は除外。
                if let Some(i) = options
                    .iter()
                    .position(|o| (o.contains("決定") || o.contains("確定")) && !o.contains("確認"))
                {
                    return i as u32 + 1;
                }
                if let Some(i) = options.iter().position(|o| o.contains('【')) {
                    return i as u32 + 1;
                }
            }
            ask_choice
        };
        println!("--- drive (click-through, Ask→{ask_choice}, autostart={autostart}) ---");
        let mut last_stage_file = app.current_stage_file().to_string();
        println!("  current_stage_file(start)={last_stage_file:?}");
        let mut last_units = app.database().unit_instances.len();
        let mut firable_reported = false;
        // キャラメイキング (召喚画面) で「名前入力」→ 一意な名前 → 「決定」の順に進めるための状態。
        // autostart が空のまま「決定」すると パイロット名が空 → 戦闘で combat_data=None になる。
        // また「ランダム」はヘッドレス決定論 RNG が同名 (例 ガガガガガ) を生むため重複登録で詰まる。
        // よって「名前入力」で `テストパイロットN` と一意名を与えてから「決定」する。
        let mut cmaking_named = false; // 現キャラの名前入力済みか (キャラごとに false に戻す)。
        let mut cmaking_char = 0u32; // 一意名のための連番。
        let mut cmaking_pilots = 0u32; // 部隊ロスターに加えたパイロット数。
                                       // キャラメイキングは内部ループで何人でも作れる (手動 exit を別途要実装)。
                                       // D は機体 2 機なので 2 人作ったら drive を終了して状態を報告する
                                       // (キャラメイキングの exit→搭乗→出撃 の drive はこれから/次セッション)。
        const CMAKING_TARGET: u32 = 2;
        // インターミッションで「キャラクターメイキング」を一度実行したか (未実行なら次ステージへ
        // 進む前にキャラメイキングを選び、パイロットを作って味方機に乗せる)。
        let mut cmaking_intermission_done = false;
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
        // VERIFY_CMAKING_EXIT: データロード経路の exit 用に最小 .src を VFS に用意する。
        // 書式は読込側 `Right(行, Len-16)` (= "Set 設定[パイロット一覧] " 16文字を剥がす) ＋
        // `Left(.., Len-1)` (末尾1文字を剥がす) に合わせ、行末に捨て1文字 (空白) を付ける。
        if cmaking_exit_drive {
            let pilot = app
                .database()
                .pilots
                .first()
                .map(|p| p.name.clone())
                .unwrap_or_default();
            let path = "cmexit.src";
            let h = app.vfs_open(path, "出力");
            app.vfs_print(&h, format!("Set 設定[パイロット一覧] {pilot} "));
            app.vfs_close(&h);
            app.set_script_var("__verify_loadfile".to_string(), path.to_string());
            let rh = app.vfs_open(path, "入力");
            let readback = app.vfs_read_line(&rh);
            app.vfs_close(&rh);
            println!(
                "  cmaking_exit setup: .src='{path}' load_pilot='{pilot}' readback={readback:?}"
            );
        }
        for step in 0..400 {
            let state = app.stage_state();
            dump_vars(&app, step);
            if matches!(state, src_core::stage::StageState::Defeat) {
                println!("  [{step}] stage_state={state:?} → 停止");
                break;
            }
            if let Some(d) = app.pending_dialog().cloned() {
                // 召喚キャラが部隊に加わったら、次キャラの名前入力に備えて状態をリセットする。
                if matches!(&d, PendingDialog::Confirm { question, .. } if question.contains("部隊に加え"))
                    || matches!(&d, PendingDialog::Talk { body, .. } if body.contains("部隊に加え"))
                {
                    cmaking_named = false;
                }
                if matches!(&d, PendingDialog::Talk { body, .. } if body.contains("部隊に加えた"))
                {
                    cmaking_pilots += 1;
                }
                let (kind, snippet, choice) = match &d {
                    PendingDialog::Talk { speaker, body } => {
                        ("Talk", format!("{speaker}: {}", trunc(body, 40)), 0)
                    }
                    PendingDialog::WaitClick => ("WaitClick", String::new(), 0),
                    // Confirm は choice 0 = はい (respond_dialog で 0→選択=1=Yes に反転される)。
                    // 機体確定「…でいいですか？」も難易度「開始しますか？」も 0(=Yes) で進む。
                    PendingDialog::Confirm { question, .. } => ("Confirm", trunc(question, 40), 0),
                    PendingDialog::Menu {
                        prompt, options, ..
                    } => {
                        // 召喚画面 (キャラメイキング) の HotPoint メニューは「ランダム」と
                        // 「決定」を含む。autostart 時はまず「ランダム」でキャラ生成→次に「決定」。
                        let is_cmaking = options.iter().any(|o| o == "名前入力")
                            && options.iter().any(|o| o == "決定");
                        let c = if autostart && is_cmaking {
                            if cmaking_exit_drive
                                && cmaking_pilots >= CMAKING_TARGET
                                && options.iter().any(|o| o == "データロード")
                            {
                                // 目標人数作成後はデータロードを選んで Break 経路で抜ける。
                                options.iter().position(|o| o == "データロード").unwrap() as u32 + 1
                            } else if !cmaking_named {
                                // まず「名前入力」を開き、続く Input で一意名を与える。
                                options.iter().position(|o| o == "名前入力").unwrap() as u32 + 1
                            } else {
                                // 名前入力後は「決定」で 姓/性別/愛称 を順に促し最後に確定する。
                                // named は部隊加入確認まで保持 (途中で false に戻すとループする)。
                                options.iter().position(|o| o == "決定").unwrap() as u32 + 1
                            }
                        } else {
                            menu_choice(options)
                        };
                        (
                            "Menu/Ask",
                            format!("{} [{}]", trunc(prompt, 30), options.join("|")),
                            c,
                        )
                    }
                    PendingDialog::Input { prompt, .. } => ("Input", trunc(prompt, 40), 0),
                };
                // 対話の発生元を exec_pc の逆引きで特定 (動的構築メニューの源 triage)。
                // ラベル名はエンコーディング差で grep 不能なことがあるため、pc が属する
                // 登録 .eve ファイル名も併記する (eve_entries の [pc, pc+len) レンジ)。
                let src = {
                    let pc = app.current_exec_pc();
                    let lib = app.script_library();
                    let lbl = lib
                        .labels
                        .iter()
                        .filter(|(_, &p)| p <= pc)
                        .max_by_key(|(_, &p)| p)
                        .map(|(n, p)| format!("{n}@{p}"))
                        .unwrap_or_else(|| "?".to_string());
                    let file = eve_entries
                        .iter()
                        .find(|(_, p, l)| *p <= pc && pc < p + l)
                        .map(|(n, _, _)| n.as_str())
                        .unwrap_or("?");
                    format!(" {{src pc={pc} {lbl} file={file}}}")
                };
                // パイロット/ユニット能力の「閲覧画面」(タブ [ユニット|機体|レーダー|武器|
                // パイロット] のみ) は進行肢が無く、選択し続けるとループする。右クリックで
                // キャンセルして抜ける (キャラメイキング確定後にこの画面へ入るため)。
                let cancel_menu = matches!(&d, PendingDialog::Menu { options, .. }
                    if !options.is_empty()
                        && options.iter().all(|o| matches!(o.as_str(),
                            "ユニット" | "機体" | "レーダー" | "武器" | "パイロット")))
                    // データロードの パイロットリスト Ask (「キャンセルで作成終了」) も右クリックで
                    // キャンセルし、RemovePilot→パイロットロード終了→Break で CMaking を抜ける。
                    || matches!(&d, PendingDialog::Menu { prompt, .. } if prompt.contains("作成終了"));
                // 目標人数を作ったらキャラメイキングの 召喚画面 メニューをキャンセルして抜ける。
                let cmaking_exit = autostart
                    && !cmaking_exit_drive
                    && cmaking_pilots >= CMAKING_TARGET
                    && matches!(&d, PendingDialog::Menu { options, .. }
                        if options.iter().any(|o| o == "名前入力")
                            && options.iter().any(|o| o == "決定"));
                // Menu(Ask) はブラウザの「選択肢をクリック」経路を模して
                // handle_input(ClickAt) で確定する (プレーン Menu のクリック選択
                // 回帰を実シナリオで確認)。1 行 prompt 前提で選択肢行の y を算出。
                // クリックが外れた場合 (2 行 prompt 等) は respond で確定にフォール。
                // キャラメイキングの名前入力 (Input "名前を入力してください…") には一意名を与える。
                // 姓/ミドルネームの任意入力はキャンセル (右クリック=既定/空のまま) で抜ける。
                let cmaking_name_input = matches!(&d, PendingDialog::Input { prompt, .. }
                    if prompt.contains("名前を入力"));
                let cmaking_skip_input = matches!(&d, PendingDialog::Input { prompt, .. }
                    if prompt.contains("姓を入力") || prompt.contains("ミドルネームを入力"));
                if cmaking_name_input && autostart {
                    // 名前は「全角カタカナのみ」。キャラごとに一意なカタカナ名にする。
                    let kata = [
                        'ア', 'イ', 'ウ', 'エ', 'オ', 'カ', 'キ', 'ク', 'ケ', 'コ', 'サ', 'シ',
                    ];
                    let suffix = kata[(cmaking_char as usize) % kata.len()];
                    cmaking_char += 1;
                    // 9 文字以内・全角カタカナのみ。短い一意名にする。
                    let name = format!("パイロ{suffix}");
                    cmaking_named = true;
                    println!("  [{step}] {kind} {snippet}{src} → input(\"{name}\")");
                    app.respond_dialog_text(name);
                } else if cmaking_skip_input && autostart {
                    // 姓/ミドルネームは任意だが全角カタカナのみ。キャンセルだと再提示で
                    // ループするため固定のカタカナ値を与えて進める。
                    println!("  [{step}] {kind} {snippet}{src} → input(\"テスト\")");
                    app.respond_dialog_text("テスト".to_string());
                } else if cmaking_exit {
                    // 目標人数 (D は機体 2 機ぶん) を作成済み。キャラメイキングの exit→搭乗→出撃 の
                    // drive はまだ無いため、ここで作成済みパイロットを報告して drive を終了する。
                    // (注: パイロットはロスターに追加されるだけで、機体への搭乗は後段の別工程。
                    //  ※ 2 人目以降のカタカナ名入力が固まる不具合は src-core 側で修正済 = commit 865844c。)
                    println!(
                        "  [{step}] {kind} {snippet}{src} → 目標 {CMAKING_TARGET} 人作成済 → drive 終了 (exit/搭乗/出撃は未実装)"
                    );
                    break;
                } else if cancel_menu {
                    println!(
                        "  [{step}] {kind} {snippet}{src} → cancel(閲覧画面を右クリックで抜ける)"
                    );
                    app.respond_dialog_right_click();
                } else if matches!(d, PendingDialog::Menu { .. }) && choice >= 1 && !autostart {
                    // CANVAS 480: 選択肢開始 y=304、行高 20px、行中央 +10。
                    // (非 autostart のみ: クリック選択経路の回帰を実シナリオで確認する。
                    //  autostart 時はクリック座標ズレで意図しない選択肢に当たるのを避け、
                    //  下の respond_dialog(choice) で確実に意図した選択肢を選ぶ。)
                    let cy = 304 + (choice as i32 - 1) * 20 + 10;
                    println!("  [{step}] {kind} {snippet}{src} → click(120,{cy})→choice {choice}");
                    app.handle_input(src_core::Input::ClickAt { x: 120, y: cy });
                    if app.pending_dialog().is_some() {
                        // クリックが外れた: 確実に確定させる。
                        app.respond_dialog(choice);
                    }
                } else {
                    println!("  [{step}] {kind} {snippet}{src} → respond({choice})");
                    app.respond_dialog(choice);
                }
            } else if let Some(t) = app.pending_timer() {
                println!("  [{step}] Timer({t:.1}) → tick(big)");
                app.tick(100.0);
            } else if matches!(app.scene(), src_core::Scene::Intermission) {
                let count = app.intermission_item_count();
                // autostart 時、未実行なら「キャラクターメイキング」項目を選んでパイロットを作る
                // (味方機は機体選択時 `パイロット不在` で生成されるため、これを経ないと無人で
                // 出撃し combat_data=None になり戦闘が成立しない)。実行後は次ステージへ。
                let cmaking_idx = if autostart && !cmaking_intermission_done && !seat_debug {
                    (0..count).find(|&i| {
                        app.intermission_item_label(i)
                            .map(|l| l.contains("キャラクター") || l.contains("メイキング"))
                            .unwrap_or(false)
                    })
                } else {
                    None
                };
                if let Some(i) = cmaking_idx {
                    cmaking_intermission_done = true;
                    let label = app.intermission_item_label(i).unwrap_or_default();
                    println!(
                        "  [{step}] Intermission (items={count}) → キャラクターメイキング (cursor={i} {label:?})"
                    );
                    app.set_intermission_cursor(i);
                    app.handle_input(src_core::Input::Advance);
                } else {
                    let last = count.saturating_sub(1);
                    println!(
                        "  [{step}] Intermission (items={count}) → 次のステージへ (cursor={last})"
                    );
                    app.set_intermission_cursor(last);
                    app.handle_input(src_core::Input::Advance);
                }
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
                // VERIFY_AUTOPLAY: 味方を AI で前進・攻撃させてから EndPhase する。
                // 戦闘開始時に各ユニットの武器発射可否レポートを一度だけ出す (交戦不成立の
                // 原因 — パイロット欠落/必要技能/気力/EN — を切り分ける)。
                let action = if autoplay {
                    if seat_debug && !firable_reported {
                        let n = app.debug_seat_db_pilot();
                        if n > 0 {
                            println!("      seat_debug: パイロット不在の味方 {n} 機に DB パイロットを搭乗");
                        }
                    }
                    if !firable_reported {
                        firable_reported = true;
                        println!("      --- firable report (戦闘開始時) ---");
                        for line in app.debug_firable_report() {
                            println!("      {line}");
                        }
                    }
                    let msgs_before = app.messages().len();
                    app.debug_run_phase_ai();
                    // 最初の数ステップだけ、味方フェイズで戦闘 (新規メッセージ) が発生したか報告する。
                    if step < 52 {
                        let new_msgs: Vec<&String> =
                            app.messages().iter().skip(msgs_before).collect();
                        if new_msgs.is_empty() {
                            println!("      autoplay: 攻撃が発生していない (交戦不成立)");
                        } else {
                            for m in new_msgs.iter().take(8) {
                                println!("      msg+ {m}");
                            }
                        }
                    }
                    "autoplay→EndPhase"
                } else {
                    "EndPhase"
                };
                println!(
                    "  [{step}] Battle idle (turn {} {:?}) {map_info} on_map={on_map}/{} cursor={:?} scene={:?} → {action}",
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
            println!(
                "    {} [{:?}] pilot={:?}",
                u.unit_data_name, u.party, u.pilot_name
            );
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
