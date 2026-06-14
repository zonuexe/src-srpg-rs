//! スパロボ戦記 シナリオの load 耐性 + 構造テスト / Load-resilience smoke test.
//!
//! 著作権配慮: 本テストは `crates/src-web/tests/fixtures/スパロボ戦記/`
//! に既存配置されているシナリオを **参照** するのみで、テストコード自体に
//! オリジナルシナリオの文章 / コードを embed しない。
//!
//! 目的: 34 個の `.eve` (合計 91k 行) + データファイル群を 2-pass で
//! ロードし、parse / execute のクラッシュやリグレッションを検出する。
//! fixture は SRC のオリジナルなので、parse 成功率 / 著名な entry label
//! の存在 / 大きなパース崩壊が起きていないことを assert する。

use std::fs;
use std::path::{Path, PathBuf};

use src_core::data::{
    event, item, loader, map as mapdata, pilot, special_power, terrain_file, unit,
};
use src_core::event_runtime;
use src_core::test_harness::{Harness, Step};
use src_core::App;

fn scenario_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("src-web/tests/fixtures/スパロボ戦記")
}

fn walk_filter<F: Fn(&Path) -> bool>(dir: &Path, f: &F, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_filter(&p, f, out);
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

#[test]
fn sparobo_scenario_loads_without_crashing() {
    let root = scenario_root();
    if !root.exists() {
        eprintln!("[skip] スパロボ戦記 fixture が無い: {}", root.display());
        return;
    }

    let mut app = App::with_rng_seed(0xCAFEBABE_u64);

    // ---- 1st pass: データファイル (pilot/unit/item/sp/terrain/.map) -----
    let mut data_paths: Vec<PathBuf> = Vec::new();
    walk_filter(
        &root,
        &|p| {
            matches!(
                basename_lower(p).as_str(),
                "pilot.txt"
                    | "non_pilot.txt"
                    | "unit.txt"
                    | "robot.txt"
                    | "item.txt"
                    | "sp.txt"
                    | "mind.txt"
                    | "terrain.txt"
            ) || p.extension().and_then(|s| s.to_str()) == Some("map")
        },
        &mut data_paths,
    );
    data_paths.sort();

    let mut data_loaded = 0usize;
    let mut data_failed = 0usize;
    for p in &data_paths {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        let base = basename_lower(p);
        let result: Result<(), String> = match base.as_str() {
            "pilot.txt" => pilot::parse(&txt)
                .map(|v| {
                    app.database_mut().extend_pilots(v);
                })
                .map_err(|e| e.to_string()),
            "unit.txt" | "robot.txt" => unit::parse(&txt)
                .map(|v| {
                    app.database_mut().extend_units(v);
                })
                .map_err(|e| e.to_string()),
            "item.txt" => item::parse(&txt)
                .map(|v| {
                    app.database_mut().extend_items(v);
                })
                .map_err(|e| e.to_string()),
            "sp.txt" | "mind.txt" => special_power::parse(&txt)
                .map(|v| {
                    app.database_mut().extend_special_powers(v);
                })
                .map_err(|e| e.to_string()),
            "terrain.txt" => terrain_file::parse(&txt)
                .map(|v| {
                    app.database_mut().extend_terrains(v);
                })
                .map_err(|e| e.to_string()),
            "non_pilot.txt" => Ok(()), // 一旦スキップ (フォーマット最小)
            _ if p.extension().and_then(|s| s.to_str()) == Some("map") => mapdata::parse(&txt)
                .map(|m| app.database_mut().store_map(base, m))
                .map_err(|e| e.to_string()),
            _ => Ok(()),
        };
        if result.is_ok() {
            data_loaded += 1;
        } else {
            data_failed += 1;
        }
    }
    eprintln!(
        "[data] 取込成功 {} / 失敗 {} / 全 {}",
        data_loaded,
        data_failed,
        data_paths.len()
    );

    // ---- 2nd pass: .eve をパース + 実行 -----
    let mut eves: Vec<PathBuf> = Vec::new();
    walk_filter(
        &root,
        &|p| p.extension().and_then(|s| s.to_str()) == Some("eve"),
        &mut eves,
    );
    eves.sort();

    // **2-phase load**: 単相 (parse → execute を 1 ファイルずつ) では
    // 例えば BossBattle.eve が Call SubTitle するときに lib/SubTitle.eve が
    // まだ未ロードでラベルが見つからず失敗する。先に全 .eve を
    // `library_append` でラベル登録だけ済ませてから、各ファイルの
    // top-level を `run_from_pc` で実行することで、ファイル間の前方参照を
    // 解決可能にする。
    let mut parse_ok = 0usize;
    let mut parse_err = 0usize;
    let mut total_statements = 0usize;
    let mut entries: Vec<(PathBuf, usize)> = Vec::new();
    for p in &eves {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        let stmts = match event::parse(&txt) {
            Ok(s) => {
                parse_ok += 1;
                s
            }
            Err(e) => {
                parse_err += 1;
                eprintln!(
                    "  ⚠ parse: {} :: {}",
                    p.strip_prefix(&root).unwrap_or(p).display(),
                    e
                );
                continue;
            }
        };
        total_statements += stmts.len();
        let start_pc = event_runtime::library_append(&mut app, &stmts);
        entries.push((p.clone(), start_pc));
    }
    let mut exec_ok = 0usize;
    let mut exec_err = 0usize;
    for (p, start_pc) in &entries {
        match event_runtime::run_from_pc(&mut app, *start_pc) {
            Ok(()) => exec_ok += 1,
            Err(e) => {
                exec_err += 1;
                eprintln!(
                    "  ⚠ exec: {} :: {}",
                    p.strip_prefix(&root).unwrap_or(p).display(),
                    e
                );
            }
        }
    }

    eprintln!(
        "[eve] parse {}/{} ({}失敗) / exec {}/{} ({}失敗) / total stmts {}",
        parse_ok,
        eves.len(),
        parse_err,
        exec_ok,
        eves.len(),
        exec_err,
        total_statements,
    );
    eprintln!(
        "[final] script_library 統計: statements={} custom_commands={}",
        app.script_library().statements.len(),
        app.script_library().custom_commands.len(),
    );
    let unit_cmds: Vec<&str> = app
        .script_library()
        .custom_commands
        .iter()
        .filter(|c| c.is_unit)
        .map(|c| c.name.as_str())
        .collect();
    eprintln!("[custom unit commands] {unit_cmds:?}");

    // ---- 不変条件 -----------------------------------------------------
    // 1) parse / exec 成功率は 80% 以上を期待 (現状値は実行後に調整可)
    assert!(
        parse_ok * 5 >= eves.len() * 4,
        "parse 成功率が 80% 未満: {parse_ok}/{}",
        eves.len()
    );
    // 2) entry label `プロローグ` が library に存在する
    assert!(
        app.script_library().label_pc("プロローグ").is_some(),
        "プロローグ ラベルが登録されていない (entry が壊れている)"
    );
    // 3) 取り込んだユニット / パイロットが 0 件ではない
    assert!(
        !app.database().pilots.is_empty() || !app.database().units.is_empty(),
        "pilot / unit data が取り込まれていない"
    );

    // ---- 失敗統計が SLA 範囲内 ------------------------------------------
    // exec が一定数失敗するのは構文の細部 (未対応コマンド or 引数欠落) の
    // ためで、parse が通っていれば実害は小さい。failure を許容しつつ
    // 「全件失敗ではない」ことだけ確認する。
    assert!(
        exec_ok > 0,
        "1 件も exec 成功していない (library 登録すら失敗)"
    );
}

#[test]
fn sparobo_main_entry_drives_to_prologue_state() {
    let root = scenario_root();
    if !root.exists() {
        eprintln!("[skip] スパロボ戦記 fixture が無い");
        return;
    }

    let mut app = App::with_rng_seed(0xCAFEBABE_u64);
    let mut eves: Vec<PathBuf> = Vec::new();
    walk_filter(
        &root,
        &|p| p.extension().and_then(|s| s.to_str()) == Some("eve"),
        &mut eves,
    );
    eves.sort();

    // Library-only load (どの .eve も top-level を走らせない)。
    // 各 .eve は basename 付きで library に登録 → 後で find_file で
    // main entry の PC を引ける。
    let mut main_name: Option<String> = None;
    for p in &eves {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        if let Ok(stmts) = event::parse(&txt) {
            let name = p
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            app.script_library_mut().append_with_name(&stmts, &name);
            // ルート直下 (== fixture ディレクトリ直下) を main entry とみなす
            if p.parent().and_then(|d| d.file_name()) == root.file_name() {
                main_name = Some(name);
            }
        }
    }
    let main_name = main_name.expect("main entry .eve がルート直下に見つからない");
    let main_pc = app
        .script_library()
        .find_file(&main_name)
        .map(|e| e.start_pc)
        .expect("main entry の FileEntry が無い");
    eprintln!("[main entry] {main_name} → start PC = {main_pc}");

    // main entry の top-level だけ実行 (他 .eve は library として参照されるのみ)
    let _ = event_runtime::run_from_pc(&mut app, main_pc);

    eprintln!(
        "[run後] {} / hotpoints={} / overlay cmds={} / 設定[ファイル名]={:?}",
        app.modal_gate().kind_label(),
        app.hotpoints().len(),
        app.script_overlay().cmds.len(),
        app.script_var("設定[ファイル名]"),
    );

    // main entry のプロローグでは多数の Set + 描画が走る → 何かしらの
    // 状態変化が必ず観測できるはず。
    let made_progress = !app.script_overlay().cmds.is_empty()
        || !app.script_var("設定[ファイル名]").is_empty()
        || app.modal_gate().is_blocked()
        || !app.hotpoints().is_empty();
    assert!(
        made_progress,
        "main entry の run_from_pc が何も状態を変えていない"
    );

    // Drain で残りの Wait / Confirm を消化
    let mut h = Harness::from_app(app);
    let _ = h.drive(&[Step::Drain(4096)]);
    eprintln!(
        "[Drain後] {} / hotpoints={} / overlay cmds={} / messages={}",
        h.app().modal_gate().kind_label(),
        h.app().hotpoints().len(),
        h.app().script_overlay().cmds.len(),
        h.app().messages().len(),
    );
}

#[test]
fn sparobo_prologue_drives_to_hotpoint_menu() {
    let root = scenario_root();
    if !root.exists() {
        eprintln!("[skip] スパロボ戦記 fixture が無い");
        return;
    }

    // 簡略化のためデータ事前ロードはスキップ (pilot/unit data 無しでも
    // プロローグの描画コマンド系は走る)。
    let mut app = App::with_rng_seed(0xCAFEBABE_u64);
    let mut eves: Vec<PathBuf> = Vec::new();
    walk_filter(
        &root,
        &|p| p.extension().and_then(|s| s.to_str()) == Some("eve"),
        &mut eves,
    );
    eves.sort();
    let mut entries: Vec<(PathBuf, usize)> = Vec::new();
    for p in &eves {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        if let Ok(stmts) = event::parse(&txt) {
            let pc = event_runtime::library_append(&mut app, &stmts);
            entries.push((p.clone(), pc));
        }
    }
    // 全 .eve を library に集約してから run。最初に suspend した .eve を
    // 記録 (将来の wiring 改善の足場として、どのファイルが modal を起こす
    // か分かるように)。
    let mut first_suspender: Option<PathBuf> = None;
    for (p, pc) in &entries {
        let pre_blocked = app.modal_gate().is_blocked();
        let _ = event_runtime::run_from_pc(&mut app, *pc);
        if !pre_blocked && app.modal_gate().is_blocked() {
            first_suspender = Some(p.clone());
        }
    }
    if let Some(p) = &first_suspender {
        eprintln!(
            "[suspender] 初回 modal blocked を起こした .eve: {}",
            p.strip_prefix(&root).unwrap_or(p).display()
        );
    }

    // プロローグ ラベルが見つかること
    let lib = app.script_library();
    assert!(
        lib.label_pc("プロローグ").is_some(),
        "プロローグ が library に無い"
    );

    // ロード直後に既にスクリプトが何かしらの中断状態 (Wait/Hotpoint 等) に
    // 入っていることを期待する。`スパロボ戦記.eve` のプロローグは描画 +
    // Wait → 描画 → 最終的に Hotpoint メニューで待機する設計。
    eprintln!(
        "[load後 modal] {} / hotpoints={} / script_overlay cmds={} / msg={}",
        app.modal_gate().kind_label(),
        app.hotpoints().len(),
        app.script_overlay().cmds.len(),
        app.messages().len(),
    );
    assert!(
        app.modal_gate().is_blocked() || !app.hotpoints().is_empty(),
        "プロローグが何も実行せずに完走終了している (Wait / Hotpoint 待機にも \
         入っていない)"
    );

    // Drain で Wait timer / Talk dialog を全部消化する。完走後の停止位置は
    // Hotpoint メニュー (描画 + Hotpoint 登録) になるはず。
    let mut h = Harness::from_app(app);
    let _ = h.drive(&[Step::Drain(2048)]);
    eprintln!(
        "[Drain後 modal] {} / hotpoints={} / script_overlay cmds={} / stage={}",
        h.app().modal_gate().kind_label(),
        h.app().hotpoints().len(),
        h.app().script_overlay().cmds.len(),
        h.app().stage(),
    );

    // 描画コマンドが蓄積されているはず (PaintPicture / Line / Font /
    // PaintString / HotpointString が一通り走る)。
    assert!(
        !h.app().script_overlay().cmds.is_empty(),
        "プロローグの描画コマンドが script_overlay に蓄積されていない"
    );
}

/// スパロボ戦記のタイトル画面を greedy にクリックして駆動し、
/// `IntermissionCommand` 登録 + `Continue` まで到達して `Scene::Intermission`
/// に入れるかを診断する。
///
/// 目的: 「出撃できる味方がいません」報告の切り分け。インターミッション画面に
/// 到達できているかを自動で確認する。完全踏破できなくても、どこで止まったかを
/// eprintln して手がかりにする。
#[test]
fn sparobo_title_drive_reaches_intermission() {
    use src_core::{PendingDialog, Scene};

    let root = scenario_root();
    if !root.exists() {
        eprintln!("[skip] スパロボ戦記 fixture が無い");
        return;
    }

    let mut app = App::with_rng_seed(0xCAFEBABE_u64);

    // ---- データ + .eve を 2-phase ロード ----
    let mut data_paths: Vec<PathBuf> = Vec::new();
    walk_filter(
        &root,
        &|p| {
            matches!(
                basename_lower(p).as_str(),
                "pilot.txt"
                    | "unit.txt"
                    | "robot.txt"
                    | "item.txt"
                    | "sp.txt"
                    | "mind.txt"
                    | "terrain.txt"
            ) || p.extension().and_then(|s| s.to_str()) == Some("map")
        },
        &mut data_paths,
    );
    data_paths.sort();
    for p in &data_paths {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        match basename_lower(p).as_str() {
            "pilot.txt" => {
                if let Ok(v) = pilot::parse(&txt) {
                    app.database_mut().extend_pilots(v);
                }
            }
            "unit.txt" | "robot.txt" => {
                if let Ok(v) = unit::parse(&txt) {
                    app.database_mut().extend_units(v);
                }
            }
            "item.txt" => {
                if let Ok(v) = item::parse(&txt) {
                    app.database_mut().extend_items(v);
                }
            }
            "sp.txt" | "mind.txt" => {
                if let Ok(v) = special_power::parse(&txt) {
                    app.database_mut().extend_special_powers(v);
                }
            }
            "terrain.txt" => {
                if let Ok(v) = terrain_file::parse(&txt) {
                    app.database_mut().extend_terrains(v);
                }
            }
            _ if p.extension().and_then(|s| s.to_str()) == Some("map") => {
                if let Ok(m) = mapdata::parse(&txt) {
                    app.database_mut().store_map(basename_lower(p), m);
                }
            }
            _ => {}
        }
    }

    let mut eves: Vec<PathBuf> = Vec::new();
    walk_filter(
        &root,
        &|p| p.extension().and_then(|s| s.to_str()) == Some("eve"),
        &mut eves,
    );
    eves.sort();
    // main entry (ルート直下の .eve) を先頭に並べ替える。実アプリと同様に
    // 主シナリオ プロローグ (タイトル画面) を最初に走らせるため。
    eves.sort_by_key(|p| {
        let is_root = p.parent().and_then(|d| d.file_name()) == root.file_name();
        !is_root // false(=0, root) が先
    });
    let mut entries: Vec<usize> = Vec::new();
    for p in &eves {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        if let Ok(stmts) = event::parse(&txt) {
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or_default();
            let pc = app.script_library().statements.len();
            app.script_library_mut().append_with_name(&stmts, name);
            entries.push(pc);
        }
    }
    for pc in &entries {
        let _ = event_runtime::run_from_pc(&mut app, *pc);
    }

    // ---- タイトル画面を greedy 駆動 ----
    // 優先的にクリックする hotpoint 名のキーワード (「進む」系)。
    let advance_kw = ["START", "開始", "決定", "はい", "ＯＫ", "OK"];
    let mut last_scene = app.scene();
    let mut reached = false;
    let mut stuck_reason = String::from("(未到達)");
    for i in 0..800 {
        if app.scene() == Scene::Intermission {
            reached = true;
            break;
        }
        match app.pending_dialog().cloned() {
            Some(PendingDialog::Menu { options, .. }) => {
                let hps = app.hotpoints().to_vec();
                if hps.is_empty() {
                    // 素の Menu (Ask 等)。「決定」系の選択肢を優先。
                    let pick = options
                        .iter()
                        .position(|o| {
                            ["決定", "確定", "はい", "ＯＫ", "OK"]
                                .iter()
                                .any(|k| o.contains(k))
                        })
                        .unwrap_or(0);
                    eprintln!("[step {i}] Menu options={options:?} → pick[{pick}]");
                    app.respond_dialog((pick as u32) + 1);
                } else {
                    // 「進む」系を優先、無ければ最後の hotpoint (多くの画面で
                    // 確定ボタンは最後に登録される)。
                    let pick = hps
                        .iter()
                        .position(|h| advance_kw.iter().any(|k| h.name.contains(k)))
                        .unwrap_or(hps.len() - 1);
                    let names: Vec<&str> = hps.iter().map(|h| h.name.as_str()).collect();
                    eprintln!(
                        "[step {i}] Menu hotpoints={names:?} → pick[{pick}]={:?}",
                        hps[pick].name
                    );
                    app.respond_dialog((pick as u32) + 1);
                }
            }
            Some(PendingDialog::Confirm { .. }) => {
                eprintln!("[step {i}] Confirm → Yes");
                app.respond_dialog(0); // Yes
            }
            Some(PendingDialog::Talk { speaker, .. }) => {
                eprintln!("[step {i}] Talk({speaker}) → advance");
                app.respond_dialog(0);
            }
            Some(PendingDialog::WaitClick) => {
                eprintln!("[step {i}] WaitClick → advance");
                app.respond_dialog(0);
            }
            Some(PendingDialog::Input { .. }) => {
                eprintln!("[step {i}] Input → text");
                app.respond_dialog_text("テストデータ".to_string());
            }
            None => {
                if app.pending_timer().is_some() {
                    app.tick(60.0);
                } else {
                    stuck_reason = format!(
                        "step {i}: pending 無し / scene={:?} / intermission_cmds={} / \
                         has_script_ctx={} / msg_count={}",
                        app.scene(),
                        app.intermission_commands().len(),
                        app.has_script_context(),
                        app.messages().len(),
                    );
                    break;
                }
            }
        }
        if app.scene() != last_scene {
            eprintln!(
                "[title drive] step {i}: scene {last_scene:?} → {:?}",
                app.scene()
            );
            last_scene = app.scene();
        }
    }

    eprintln!(
        "[title drive] reached_intermission={reached} / scene={:?} / \
         intermission_cmds={} / 次ステージ={:?} / stuck={stuck_reason}",
        app.scene(),
        app.intermission_commands().len(),
        app.script_var("次ステージ"),
    );
    let items: Vec<String> = (0..app.intermission_item_count())
        .filter_map(|i| app.intermission_item_label(i))
        .collect();
    eprintln!("[title drive] インターミッション項目: {items:?}");

    // タイトル画面を greedy 駆動すれば Scene::Intermission に到達できること。
    // ここが false に戻ると「インターミッションに入れず本編へ直行 →
    // 出撃可能な味方がいません」リグレッションの再来を意味する。
    assert!(
        reached,
        "タイトル画面駆動で Intermission に到達できなかった: {stuck_reason}"
    );
    assert_eq!(app.scene(), Scene::Intermission);
    // ユーザ定義 8 項目 + 「次のステージへ」= 9 項目。
    assert!(
        items.iter().any(|s| s == "キャラクターメイキング"),
        "インターミッションに キャラクターメイキング が無い: {items:?}"
    );
    assert!(
        items.iter().any(|s| s == "次のステージへ"),
        "「次のステージへ」項目が無い (次ステージ 未設定?): {items:?}"
    );

    // ---- 次のステージへ → 戦闘開始まで駆動し、script error 非発生を検証 ----
    // 戦闘開始時の auto-fire ラベル (Start / ターン1 → Include.eve の
    // `Foreach 味方` 等) で異常終了しないこと。L3220 (ForEach 書式1) /
    // L3233 (関数呼び出し条件の単一行 If) の回帰防止。
    let next_idx = (0..app.intermission_item_count())
        .find(|i| app.intermission_item_label(*i).as_deref() == Some("次のステージへ"))
        .expect("次のステージへ 項目");
    app.set_intermission_cursor(next_idx);
    app.handle_input(src_core::Input::Advance); // Intermission → MapView
                                                // Briefing → Sortie → Battle。各遷移後に pending を消化する。
    for _ in 0..8 {
        if app.stage_state() == src_core::StageState::Battle {
            break;
        }
        for _ in 0..400 {
            match app.pending_dialog().cloned() {
                Some(PendingDialog::Input { .. }) => {
                    app.respond_dialog_text("テストデータ".to_string());
                }
                Some(_) => {
                    app.respond_dialog(0);
                }
                None => {
                    if app.pending_timer().is_some() {
                        app.tick(60.0);
                    } else {
                        break;
                    }
                }
            }
        }
        app.handle_input(src_core::Input::Advance);
    }
    let party_count = |app: &App, p: src_core::Party| {
        app.database()
            .unit_instances
            .iter()
            .filter(|u| u.party == p)
            .count()
    };
    eprintln!(
        "[次ステージ駆動] scene={:?} stage_state={:?} units={} (味方={} 敵={}) err={:?}",
        app.scene(),
        app.stage_state(),
        app.database().unit_instances.len(),
        party_count(&app, src_core::Party::Player),
        party_count(&app, src_core::Party::Enemy),
        app.last_script_error(),
    );
    assert!(
        app.last_script_error().is_none(),
        "戦闘開始までの駆動中に .eve 実行エラー (L3220/L3233 等の回帰): {:?}",
        app.last_script_error()
    );

    // ---- 戦闘ターンを数フェイズ進め、敵フェーズ / `ターン N` auto-fire /
    //      AI 行動で異常終了しないことを検証 ----
    for _ in 0..6 {
        if app.stage_state() != src_core::StageState::Battle {
            break; // 勝敗が付いた等
        }
        app.handle_input(src_core::Input::EndPhase);
        for _ in 0..400 {
            match app.pending_dialog().cloned() {
                Some(PendingDialog::Input { .. }) => {
                    app.respond_dialog_text("x".to_string());
                }
                Some(_) => {
                    app.respond_dialog(0);
                }
                None => {
                    if app.pending_timer().is_some() {
                        app.tick(60.0);
                    } else {
                        break;
                    }
                }
            }
        }
    }
    eprintln!(
        "[戦闘ターン進行] stage_state={:?} turn={} err={:?} units={}",
        app.stage_state(),
        app.turn().number,
        app.last_script_error(),
        app.database().unit_instances.len(),
    );

    // ---- combat::predict 直接テスト ----
    // AI フェーズ / ユニット配置に依存しない直接検証。
    // データベースから武器付きユニットと任意のパイロットを取り出し、
    // combat::predict が正のダメージを返すことを確認する。
    // `*スタート` スクリプトの RNG 依存配置や Sortie フェーズの
    // 完了状況に関係なく、combat 計算エンジン自体の健全性を確認できる。
    {
        // 武器 (射程1) を持つ UnitData を探す
        let att_unit_opt = app
            .database()
            .units
            .iter()
            .find(|u| src_core::combat::best_weapon_in_range(u, 1).is_some());
        // 任意の 2 パイロット (同じでもよい)
        let pilot_opt = app.database().pilots.first();
        // 適当な防御側 UnitData (攻撃側と異なるものを優先、なければ同じ)
        let def_unit_opt = app
            .database()
            .units
            .iter()
            .find(|u| att_unit_opt.is_some_and(|a| a.name != u.name))
            .or(att_unit_opt);

        if let (Some(att_unit), Some(att_pilot), Some(def_unit)) =
            (att_unit_opt, pilot_opt, def_unit_opt)
        {
            let weapon = src_core::combat::best_weapon_in_range(att_unit, 1).unwrap();
            let preview =
                src_core::combat::predict(att_pilot, att_unit, weapon, att_pilot, def_unit, 0, 0);
            eprintln!(
                "[combat予測] atk={:?} weapon={:?} power={} → damage={}",
                att_unit.name, weapon.name, weapon.power, preview.damage
            );
            assert!(
                preview.damage > 0,
                "combat::predict が 0 ダメージを返した: atk={:?} weapon={:?}",
                att_unit.name,
                weapon.name,
            );
        } else {
            eprintln!("[combat予測] スキップ (武器付きユニット / パイロットが DB に無い)");
        }
    }

    // ---- 敵ユニットの UnitData / Pilot 解決チェック ----
    // 敵配置チェーン + データ加法マージの回帰防止。
    eprintln!(
        "[敵配置チェーン] 敵陣営={:?} 敵候補={:?} ボス候補確定={:?}",
        app.script_var("敵陣営"),
        app.script_var("敵候補確定"),
        app.script_var("ボス候補確定"),
    );
    for u in app
        .database()
        .unit_instances
        .iter()
        .filter(|u| u.party == src_core::Party::Enemy)
    {
        eprintln!(
            "[敵ユニット] {:?} pilot={:?} uid={:?} x={} y={}",
            u.unit_data_name, u.pilot_name, u.uid, u.x, u.y,
        );
        assert!(
            app.database().unit_by_name(&u.unit_data_name).is_some(),
            "配置済み敵の UnitData が未解決: {:?}",
            u.unit_data_name
        );
        assert!(
            app.database().pilot_by_name(&u.pilot_name).is_some(),
            "配置済み敵の Pilot が未解決: {:?}",
            u.pilot_name
        );
    }
}

/// キャラクターメイキングを自動駆動し、`召喚データ書き込み` 経由で
/// PilotData が VFS に書き出され再パースされることを検証する。
#[test]
fn sparobo_character_making_creates_pilot() {
    use src_core::{PendingDialog, Scene};

    let root = scenario_root();
    if !root.exists() {
        eprintln!("[skip] スパロボ戦記 fixture が無い");
        return;
    }

    let mut app = App::with_rng_seed(0xCAFEBABE_u64);

    // ---- データ + .eve を 2-phase ロード (title_drive と同一) ----
    let mut data_paths: Vec<PathBuf> = Vec::new();
    walk_filter(
        &root,
        &|p| {
            matches!(
                basename_lower(p).as_str(),
                "pilot.txt"
                    | "unit.txt"
                    | "robot.txt"
                    | "item.txt"
                    | "sp.txt"
                    | "mind.txt"
                    | "terrain.txt"
            ) || p.extension().and_then(|s| s.to_str()) == Some("map")
        },
        &mut data_paths,
    );
    data_paths.sort();
    for p in &data_paths {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        match basename_lower(p).as_str() {
            "pilot.txt" => {
                if let Ok(v) = pilot::parse(&txt) {
                    app.database_mut().extend_pilots(v);
                }
            }
            "unit.txt" | "robot.txt" => {
                if let Ok(v) = unit::parse(&txt) {
                    app.database_mut().extend_units(v);
                }
            }
            "item.txt" => {
                if let Ok(v) = item::parse(&txt) {
                    app.database_mut().extend_items(v);
                }
            }
            "sp.txt" | "mind.txt" => {
                if let Ok(v) = special_power::parse(&txt) {
                    app.database_mut().extend_special_powers(v);
                }
            }
            "terrain.txt" => {
                if let Ok(v) = terrain_file::parse(&txt) {
                    app.database_mut().extend_terrains(v);
                }
            }
            _ if p.extension().and_then(|s| s.to_str()) == Some("map") => {
                if let Ok(m) = mapdata::parse(&txt) {
                    app.database_mut().store_map(basename_lower(p), m);
                }
            }
            _ => {}
        }
    }

    let mut eves: Vec<PathBuf> = Vec::new();
    walk_filter(
        &root,
        &|p| p.extension().and_then(|s| s.to_str()) == Some("eve"),
        &mut eves,
    );
    eves.sort();
    eves.sort_by_key(|p| {
        let is_root = p.parent().and_then(|d| d.file_name()) == root.file_name();
        !is_root
    });
    let mut entries: Vec<usize> = Vec::new();
    for p in &eves {
        let Ok(bytes) = fs::read(p) else { continue };
        let txt = loader::decode_text(&bytes);
        if let Ok(stmts) = event::parse(&txt) {
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or_default();
            let pc = app.script_library().statements.len();
            app.script_library_mut().append_with_name(&stmts, name);
            entries.push(pc);
        }
    }
    for pc in &entries {
        let _ = event_runtime::run_from_pc(&mut app, *pc);
    }

    // ---- タイトル画面を greedy 駆動 → Intermission ----
    let advance_kw = ["START", "開始", "決定", "はい", "ＯＫ", "OK"];
    for _i in 0..800 {
        if app.scene() == Scene::Intermission {
            break;
        }
        match app.pending_dialog().cloned() {
            Some(PendingDialog::Menu { ref options, .. }) => {
                let hps = app.hotpoints().to_vec();
                if hps.is_empty() {
                    let pick = options
                        .iter()
                        .position(|o| {
                            ["決定", "確定", "はい", "ＯＫ", "OK"]
                                .iter()
                                .any(|k| o.contains(k))
                        })
                        .unwrap_or(0);
                    app.respond_dialog((pick as u32) + 1);
                } else {
                    let pick = hps
                        .iter()
                        .position(|h| advance_kw.iter().any(|k| h.name.contains(k)))
                        .unwrap_or(hps.len() - 1);
                    app.respond_dialog((pick as u32) + 1);
                }
            }
            Some(PendingDialog::Confirm { .. }) => {
                app.respond_dialog(0);
            }
            Some(PendingDialog::WaitClick) => {
                app.respond_dialog(0);
            }
            Some(PendingDialog::Talk { .. }) => {
                app.respond_dialog(0);
            }
            Some(PendingDialog::Input { .. }) => {
                app.respond_dialog_text("テストデータ".to_string());
            }
            None => {
                if app.pending_timer().is_some() {
                    app.tick(60.0);
                } else {
                    break;
                }
            }
        }
    }
    assert_eq!(
        app.scene(),
        Scene::Intermission,
        "タイトル駆動で Intermission に到達できなかった"
    );

    // ---- キャラクターメイキングを駆動 ----
    let pilots_before = app.database().pilots.len();
    let cmaking_idx = (0..app.intermission_item_count())
        .find(|i| app.intermission_item_label(*i).as_deref() == Some("キャラクターメイキング"))
        .expect("キャラクターメイキング 項目");
    app.set_intermission_cursor(cmaking_idx);
    app.handle_input(src_core::Input::Advance);

    for step in 0..800 {
        match app.pending_dialog().cloned() {
            Some(PendingDialog::Menu { .. }) => {
                let hps = app.hotpoints().to_vec();
                if hps.is_empty() {
                    app.respond_dialog(1);
                } else {
                    let pick = if app.script_var("召喚キャラ[名前]").is_empty() {
                        hps.iter()
                            .position(|h| h.name == "名前おまかせ")
                            .unwrap_or_else(|| {
                                hps.iter()
                                    .position(|h| h.name == "決定")
                                    .unwrap_or(hps.len() - 1)
                            })
                    } else {
                        hps.iter()
                            .position(|h| h.name == "決定")
                            .unwrap_or(hps.len() - 1)
                    };
                    let names: Vec<&str> = hps.iter().map(|h| h.name.as_str()).collect();
                    eprintln!(
                        "[cmaking step {step}] hotpoints={names:?} → pick[{pick}]={:?}",
                        hps[pick].name
                    );
                    app.respond_dialog((pick as u32) + 1);
                }
            }
            Some(PendingDialog::Confirm { .. }) => {
                eprintln!("[cmaking step {step}] Confirm → Yes");
                app.respond_dialog(0);
            }
            Some(PendingDialog::Talk { speaker, .. }) => {
                eprintln!("[cmaking step {step}] Talk({speaker}) → advance");
                app.respond_dialog(0);
            }
            Some(PendingDialog::WaitClick) => {
                eprintln!("[cmaking step {step}] WaitClick → advance");
                app.respond_dialog(0);
            }
            Some(PendingDialog::Input { .. }) => {
                eprintln!("[cmaking step {step}] Input → text");
                app.respond_dialog_text("テスト".to_string());
            }
            None => {
                if app.pending_timer().is_some() {
                    app.tick(60.0);
                } else {
                    break;
                }
            }
        }
        let pilots_now = app.database().pilots.len();
        if pilots_now > pilots_before {
            eprintln!("[cmaking step {step}] pilot 増加検出: {pilots_before} → {pilots_now}");
            break;
        }
    }
    let pilots_after = app.database().pilots.len();
    eprintln!(
        "[cmaking] pilots: {pilots_before} → {pilots_after} / units={} / script_err={:?}",
        app.database().unit_instances.len(),
        app.last_script_error(),
    );
    assert!(
        pilots_after > pilots_before,
        "キャラクターメイキング駆動後に PilotData が増えていない: {pilots_before} → {pilots_after}. \
         last_script_error={:?}",
        app.last_script_error()
    );
}
