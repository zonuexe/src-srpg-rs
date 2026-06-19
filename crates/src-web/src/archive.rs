//! ファイル選択 / アーカイブ展開 / Archive extraction.
//!
//! ブラウザの `<input type=file>` から受け取った 1 ファイルを処理する。
//! 対応:
//!
//! - 単独テキスト (`*.eve` / `*.txt`)
//! - ZIP アーカイブ (`*.zip`) — `zip` クレートで展開
//! - LZH アーカイブ (`*.lzh`) — 「無圧縮 -lh0-」のみ最小対応
//!
//! 展開後の各エントリは拡張子で判定し、`*.eve` を見つけたらシナリオとして
//! 実行、`pilot.txt` / `unit.txt` 等はそれぞれのパーサにかける。

use std::collections::HashMap;
use std::io::{Cursor, Read};
use std::rc::Rc;

use src_core::data::{
    event, item, loader, map as mapdata, pilot, special_power, terrain_file, unit,
};
use src_core::App;
use src_core::{entrypoint, event_runtime};

use crate::assets::{detect_image_mime, Assets};
use crate::audio;

/// 展開済みアーカイブ全体を `App` と `Assets` に反映。
///
/// `on_image_loaded` は取り込んだ各画像の非同期デコード完了時に呼ばれる。
/// 静止画面でも後からデコードされた画像を反映させる再描画トリガ。
///
/// `full_load=false` は「データ専用の再適用」モード。画像/音声/MIDI の登録も
/// `.eve`/`.ini` の登録・実行も行わず、データ (pilot/unit/item/sp/terrain/map)
/// だけを `database` に取り込む。シナリオ読込で `App` をリセットした後に汎用素材
/// パックの terrain/sp 等を下地として再適用する用途。画像/音声は `Assets` 側に
/// 残るため再デコード不要。ライブラリ `.eve` は top-level 実行すると Wait/Goto で
/// script context を suspend させ後続シナリオを壊すため、このモードでは実行しない。
pub fn load_into_app(
    app: &mut App,
    assets: &mut Assets,
    file_name: &str,
    bytes: &[u8],
    on_image_loaded: &Rc<dyn Fn()>,
    full_load: bool,
) -> Result<String, String> {
    let lower = file_name.to_ascii_lowercase();
    let entries: Vec<(String, Vec<u8>)> = if lower.ends_with(".zip") {
        unzip(bytes).map_err(|e| format!("zip 展開失敗: {e}"))?
    } else if lower.ends_with(".lzh") || lower.ends_with(".lha") {
        unlzh(bytes).map_err(|e| format!("lzh 展開失敗: {e}"))?
    } else {
        // 単独テキストとして扱う
        vec![(file_name.to_string(), bytes.to_vec())]
    };

    if entries.is_empty() {
        return Err("アーカイブにエントリがありません。".to_string());
    }

    let mut log = String::new();
    log.push_str(&format!(
        "読み込み: {file_name} ({} entries)\n",
        entries.len()
    ));

    // 数千件規模アーカイブ対策: バイナリは text 化せずスキップ。
    let is_text_kind = |base: &str, lname: &str| -> bool {
        lname.ends_with(".eve")
            || lname.ends_with(".ini")
            || lname.ends_with(".map")
            || base == "pilot.txt"
            || base == "non_pilot.txt"
            || base == "unit.txt"
            || base == "robot.txt"
            || base == "item.txt"
            || base == "sp.txt"
            || base == "mind.txt"
            || base == "terrain.txt"
            || base == "animation.txt"
            || base == "ext_animation.txt"
    };

    // 2 パス化: 第 1 パスはデータ系 (.txt / .map / 音声 / 画像) のみ取り込み、
    // 第 2 パスで .eve を実行する。これをやらないと .eve の `ChangeMap`
    // / `Playsound` / 画像描画が、まだ取り込まれていない後段のリソースを
    // 探して失敗する (zip 内の順序は不定)。
    let mut deferred_eves: Vec<(String, String)> = Vec::new();
    // `.ini` (Alpha2ndStatus.ini 等の設定ファイル) は `Require` で参照される。
    // script_library に登録だけして自動実行はしない。
    let mut deferred_inis: Vec<(String, String)> = Vec::new();
    // 画像は一旦集めてから「Unit / Pilot フォルダ優先」で登録する。
    // zip エントリ順 (アルファベット順) のままだと戦闘エフェクトの
    // アニメフレームが上限を食い潰し、肝心のユニット / 顔グラが登録
    // されない (スパロボ戦記: Bitmap/Anime/Animation 等が先, Bitmap/Unit が後)。
    let mut deferred_images: Vec<(&str, &[u8])> = Vec::new();
    for (name, data) in &entries {
        let lname = name.to_ascii_lowercase();
        // basename (区切り `/` / `\` 両対応) を取り出し、`non_pilot.txt`/`pilot.txt`
        // の取り違えを防ぐ。
        let base = basename(&lname);
        let txt = if is_text_kind(base, &lname) {
            loader::decode_text(data)
        } else {
            String::new()
        };
        if lname.ends_with(".eve") {
            // 第 2 パスに回す。ここではテキストだけ確定。
            deferred_eves.push((name.clone(), txt));
            continue;
        }
        if lname.ends_with(".ini") {
            // `Require` 専用。script_library に登録するが自動実行しない。
            deferred_inis.push((name.clone(), txt));
            continue;
        }
        if base == "pilot.txt" {
            // §5: データファイル 1 件のパース失敗でシナリオ全体を中断しない。
            // SRC.NET は DataErrorMessage でダイアログを出しつつ続行する。
            // さらにレコード単位で寛容に解析し、壊れた 1 レコードがあっても
            // 残りのパイロットを取り込む (README/図鑑/テンプレ pilot.txt 対策)。
            let (pilots, errors) = pilot::parse_lenient(&txt);
            let count = pilots.len();
            app.database_mut().extend_pilots(pilots);
            if errors.is_empty() {
                log.push_str(&format!(
                    "  ✓ {name} を pilot として取り込み ({count} 件)\n"
                ));
            } else {
                log.push_str(&format!(
                    "  ⚠ {name} を pilot として取り込み ({count} 件, {} レコードをスキップ): {}\n",
                    errors.len(),
                    errors[0]
                ));
            }
        } else if base == "non_pilot.txt" {
            // 元 SRC `NonPilotDataList.Load` 相当の最小読込。
            // 2 行 1 組: 名前 / ニックネーム,Bitmap
            let count = parse_non_pilots_into(app, &txt);
            log.push_str(&format!(
                "  ✓ {name} を non_pilot として取り込み ({count} 件)\n"
            ));
        } else if base == "unit.txt" || base == "robot.txt" {
            // レコード単位で寛容に解析。壊れた 1 ユニットがあっても残りを取り込む。
            let (units, errors) = unit::parse_lenient(&txt);
            let count = units.len();
            app.database_mut().extend_units(units);
            if errors.is_empty() {
                log.push_str(&format!("  ✓ {name} を unit として取り込み ({count} 件)\n"));
            } else {
                log.push_str(&format!(
                    "  ⚠ {name} を unit として取り込み ({count} 件, {} レコードをスキップ): {}\n",
                    errors.len(),
                    errors[0]
                ));
            }
        } else if base == "item.txt" {
            let (items, errors) = item::parse_lenient(&txt);
            let count = items.len();
            app.database_mut().extend_items(items);
            if errors.is_empty() {
                log.push_str(&format!("  ✓ {name} を item として取り込み ({count} 件)\n"));
            } else {
                log.push_str(&format!(
                    "  ⚠ {name} を item として取り込み ({count} 件, {} レコードをスキップ): {}\n",
                    errors.len(),
                    errors[0]
                ));
            }
        } else if base == "sp.txt" || base == "mind.txt" {
            let (sps, errors) = special_power::parse_lenient(&txt);
            let count = sps.len();
            app.database_mut().extend_special_powers(sps);
            if errors.is_empty() {
                log.push_str(&format!(
                    "  ✓ {name} を special_power として取り込み ({count} 件)\n"
                ));
            } else {
                log.push_str(&format!(
                    "  ⚠ {name} を special_power として取り込み ({count} 件, {} レコードをスキップ): {}\n",
                    errors.len(),
                    errors[0]
                ));
            }
        } else if base == "terrain.txt" {
            let (terrains, errors) = terrain_file::parse_lenient(&txt);
            let count = terrains.len();
            app.database_mut().extend_terrains(terrains);
            if errors.is_empty() {
                log.push_str(&format!(
                    "  ✓ {name} を terrain として取り込み ({count} 件)\n"
                ));
            } else {
                log.push_str(&format!(
                    "  ⚠ {name} を terrain として取り込み ({count} 件, {} レコードをスキップ): {}\n",
                    errors.len(),
                    errors[0]
                ));
            }
        } else if base == "animation.txt" || base == "ext_animation.txt" {
            // 戦闘アニメデータ。武器(状況)→表示用サブルーチンの対応表。
            let before = app.database().animation.entries.len();
            app.database_mut().merge_animation_data(&txt);
            let after = app.database().animation.entries.len();
            log.push_str(&format!(
                "  ✓ {name} を戦闘アニメデータとして取り込み (対象 {} → {} 件)\n",
                before, after
            ));
        } else if lname.ends_with(".map") {
            // `.map` ファイル: パースしてキャッシュ。ChangeMap で参照される。
            match mapdata::parse(&txt) {
                Ok(m) => {
                    app.database_mut().store_map(base.to_string(), m);
                }
                Err(e) => {
                    log.push_str(&format!("  ⚠ {name} map parse: {e}\n"));
                }
            }
        } else if full_load && audio::audio_mime_from_name(&lname).is_some() {
            // MP3 / OGG / WAV は Assets に登録（自動再生はしない）。
            // シナリオ側の `Startbgm` / `Playsound` 命令で名前を引いて再生する。
            let mime = audio::audio_mime_from_name(&lname).unwrap();
            assets.add_audio(name, data, mime);
        } else if full_load && (lname.ends_with(".mid") || lname.ends_with(".midi")) {
            // MIDI も Assets に登録。
            assets.add_midi(name, data);
        } else if full_load && detect_image_mime(data).is_some() {
            // 画像 (BMP / PNG / ICO / JPG / GIF) は後でまとめて登録する。
            // バイト列が大きすぎる (>2MB) ものはスキップしてメモリ節約。
            if data.len() <= 2 * 1024 * 1024 {
                deferred_images.push((name.as_str(), data.as_slice()));
            }
        }
        // 残りはその他 — .map / .ini / 拡張子なし等。今は無視。
    }
    // 画像登録: Unit / Pilot フォルダ (タイトルアイコン / マップスプライト /
    // 顔グラ / タイトル機体全身画像) はゲーム必須なので **上限に関わらず全て
    // 登録** し、メモリ抑制は tier 2 (背景 / エフェクト 等) の枚数だけを絞る。
    {
        // tier 0/1 (event UI アイコン / Unit・Pilot スプライト / Anime/Unit 全身
        // 画像) はゲーム必須なので **上限に関わらず全登録** する。tier 判定は
        // 849e460 で抽出した純関数 `image_priority_tier` を共有する。
        deferred_images.sort_by_key(|(name, _)| image_priority_tier(name));
        // メモリ抑制用の総枚数上限。tier 0/1 は必須なのでこの上限を超えても
        // 登録し続け、tier 2 (背景 / エフェクト) だけがこの上限の影響を受ける。
        // これにより大規模アーカイブ (例: 84MB 版スパロボ戦記、画像 6500+ 枚)
        // でもタイトルの機体整列演出 / ユニットアイコン / 顔グラ / ステータス
        // 画面の UI アイコンが欠落しない (bd20b44 / 849e460 で直した「画像が
        // 描画されない」不具合の再発防止。tier 0+1 が上限ギリギリ・超過の構成
        // でも安全)。
        const TOTAL_BUDGET: usize = 5000;
        let mut registered = 0usize;
        let mut tier2_dropped = 0usize;
        for (name, data) in deferred_images.into_iter() {
            if image_priority_tier(name) == 2 && registered >= TOTAL_BUDGET {
                tier2_dropped += 1;
                continue;
            }
            match assets.add_image(name, data, on_image_loaded) {
                Ok(()) => registered += 1,
                Err(e) => log.push_str(&format!(
                    "  ⚠ {name} 画像登録失敗: {}\n",
                    e.as_string().unwrap_or_default()
                )),
            }
        }
        if tier2_dropped > 0 {
            log.push_str(&format!(
                "  ℹ️ 背景/エフェクト画像 {tier2_dropped} 件を上限超過のため省略 \
                 (ユニット/機体画像は全登録)\n"
            ));
        }
    }
    if !assets.images.is_empty() {
        log.push_str(&format!(
            "  📷 画像 {} 件を Assets に登録\n",
            assets.images.len() / 2 // basename + stem の 2 キーで張っているので半分
        ));
    }
    if !assets.audio_clips.is_empty() {
        log.push_str(&format!(
            "  🔊 音声 {} 件を Assets に登録\n",
            assets.audio_clips.len() / 2
        ));
    }
    if !assets.midi_clips.is_empty() {
        log.push_str(&format!(
            "  ♪ MIDI {} 件を Assets に登録\n",
            assets.midi_clips.len() / 2
        ));
    }
    // 再適用モード (full_load=false) でも .eve の **ラベル登録 (2a) は行う**。
    // 共有 Lib (スペシャルパワー.eve の Mindanime、汎用戦闘アニメ GBA_*.eve 等) の
    // サブルーチンは、シナリオ読込で App をリセットした後も下地として Call できる
    // 必要があるため。**top-level 実行 (2b) と bootstrap は full_load 時のみ**行う
    // (ライブラリ .eve の top-level 実行は script context を suspend させ後続シナリオを
    // 壊すため)。terrain/sp 等は pass 1 で取り込み済み。

    // 第 2 パス: 全データ取り込み後に .eve スクリプトを実行する。
    // この順序にすることで .eve 内の `ChangeMap` / `Startbgm` /
    // `Playsound` / `PaintPicture` が、第 1 パスで登録された
    // .map / audio / image を確実に参照できる。
    //
    // `.eve` の **2-phase load**:
    //   2a. 全 .eve を parse して script_library に登録 (基本名も記録)。
    //   2b. エントリポイント .eve のみ run_from_pc で実行する。
    // 2a でラベル定義を含む全文が登録されるので `Call SubTitle` の
    // ようなファイル跨ぎ参照は 2b 実行前に解決済み。サブ .eve
    // (章ファイル・ライブラリ・GameOver.eve 等) は登録のみで十分で、
    // Continue チェインが必要なタイミングで実行される。全ファイルを
    // 一括実行すると多章シナリオの後続章が走りゲーム状態を破壊する。
    //
    // entrypoint 検出 (entrypoint-pattern.md §7) で最高スコアの .eve を選ぶ。
    // README 起動指示 / 開始系命名 / 階層位置 / `@識別子` を加味したスコア降順で
    // 並べ、同点はパス区切りの少ない = 浅い .eve を先頭へ寄せる。
    let analysis = entrypoint::analyze(&entries);
    if let Some(best) = analysis.best() {
        log.push_str(&format!(
            "  → エントリーポイント検出: {best} (候補 {} 件)\n",
            analysis.candidates.len()
        ));
    } else if analysis.data_only {
        log.push_str("  → データ専用アーカイブ (実行可能ファイルなし)\n");
    }
    let ep_score: HashMap<&str, i32> = analysis
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

    // `.ini` を script_library に登録 (`.eve` 実行前)。`Require` から
    // basename で引けるようにする。自動実行はしない。
    for (name, txt) in &deferred_inis {
        match event::parse(txt) {
            Ok(stmts) => {
                app.script_library_mut().append_with_name(&stmts, name);
                log.push_str(&format!("  ✓ {name} を設定ファイルとして登録\n"));
            }
            Err(e) => {
                log.push_str(&format!("  ⚠ {name} パースエラー: {e}\n"));
            }
        }
    }

    let mut eve_entries: Vec<(String, usize, usize)> = Vec::new();
    for (name, txt) in &deferred_eves {
        match event::parse(txt) {
            Ok(stmts) => {
                let len = stmts.len();
                let pc = app.script_library().statements.len();
                app.script_library_mut().append_with_name(&stmts, name);
                eve_entries.push((name.clone(), pc, len));
            }
            Err(e) => {
                log.push_str(&format!("  ⚠ {name} パースエラー: {e}\n"));
            }
        }
    }
    // 再適用モードはここまで (.eve のラベル登録のみ)。共有 Lib のサブルーチンが
    // 下地として登録されたので、この後に読み込むシナリオから Call できる。
    // top-level 実行・bootstrap は行わない (後続シナリオ破壊を防ぐ)。
    if !full_load {
        return Ok(log);
    }

    // エントリポイントの .eve のみ run_from_pc で実行する。
    // サブ .eve (章ファイル・ライブラリ・GameOver.eve 等) は script_library への
    // 登録済みで十分。Continue チェインが必要なタイミングで実行される。
    // 全ファイルを一括実行すると多章シナリオの後続章・GameOver.eve が
    // ロード時に走り、ゲーム状態を破壊する。
    let entry_name = analysis.best();
    for (name, pc, len) in &eve_entries {
        if entry_name == Some(name.as_str()) {
            match event_runtime::run_from_pc(app, *pc) {
                Ok(()) => {
                    log.push_str(&format!(
                        "  ✓ {name} を .eve として実行 ({len} statements)\n"
                    ));
                }
                Err(e) => {
                    log.push_str(&format!("  ⚠ {name} 実行エラー: {e}\n"));
                }
            }
        } else {
            log.push_str(&format!(
                "  ✓ {name} を .eve として登録 ({len} statements)\n"
            ));
        }
    }

    // シナリオが `IntermissionCommand` で戦闘外メニューを登録している場合
    // (スパロボ戦記 等のインターミッション制シナリオ) は、`Continue` チェインを
    // 自動進行させずに `Scene::Intermission` で停止する。ここで自動チェインを
    // 走らせると、キャラメイキングを通る前に本編 (`*スタート`) が走ってしまい
    // 「出撃可能な味方がいません」で詰む。
    //
    // 予約された `次ステージ` は消費せず、インターミッション画面の
    // 「次のステージへ」項目としてユーザに選ばせる。
    if !app.intermission_commands().is_empty() {
        app.set_scene(src_core::Scene::Intermission);
        app.set_intermission_cursor(0);
        log.push_str(&format!(
            "  → IntermissionCommand {} 件登録: インターミッション画面で停止\n",
            app.intermission_commands().len()
        ));
        return Ok(log);
    }

    // 主シナリオ `.eve` が `Continue eve\次.eve` で次ステージを予約した
    // 場合、ロード末尾でチェインを進める。`スタート.eve` のような薄い
    // entry (Continue だけ書いてある) を実際の本編ファイルへ繋ぐ用途。
    //
    // modal が立っているうちは何も進めない (フロントエンドの応答後に
    // `App::advance_to_next_stage` を再度呼ぶことを期待)。
    // ループは無限再帰防止のため 8 段で打ち切り。
    let mut chain_steps = 0;
    while chain_steps < 8 && !app.modal_gate().is_blocked() && app.advance_to_next_stage() {
        chain_steps += 1;
    }
    if chain_steps > 0 {
        log.push_str(&format!("  → Continue チェインで {chain_steps} 段進行\n"));
    }

    // `Stage` も `Continue` も使わないシナリオ (エントリ .eve のプロローグ
    // だけで終わる型。例: 東中無双２) のブートストラップ。エントリ .eve を
    // ステージファイルとして登録し、原典 `StartScenario` 後半の
    // 「プロローグ完了 → `スタート` 発火 → 味方フェイズ」を flow 継続で
    // 駆動する (プロローグが対話で suspend 中なら完了時に自動進行)。
    if let Some(best) = analysis.best() {
        app.bootstrap_stage_after_load(best);
        log.push_str(&format!("  → ステージブートストラップ: {best}\n"));
    }
    // Continue チェイン / bootstrap が flow に積んだ継続を今すぐドレインする。
    // bootstrap が early return した場合 (flow が空でない) にも対応する。
    // スクリプト中断中 (Talk 等) なら no-op のまま返り、resume 後にドレインされる。
    app.on_script_completed();

    // シナリオが動き始めていれば (PaintPicture / Hotpoint で描画した、または
    // プロローグが Talk 等の対話で中断している)、タイトル画面を抜けて MapView を
    // 表示する。これをしないとロード後も SRC スプラッシュロゴが残り、プロローグの
    // Talk がその上に重なって表示されてしまう (musou 系: 最初の Talk は PaintPicture
    // を伴わないため script_overlay 空のまま Title に留まっていた)。
    let has_scenario_visuals = !app.script_overlay().cmds.is_empty()
        || !app.hotpoints().is_empty()
        || app.pending_dialog().is_some();
    if has_scenario_visuals && app.scene() != src_core::Scene::MapView {
        app.set_scene(src_core::Scene::MapView);
    }
    Ok(log)
}

/// 末尾コンポーネント（`\` または `/` 区切り）を取り出す。
/// すでに小文字化されている前提で呼ぶ。
fn basename(lname: &str) -> &str {
    let slash = lname.rfind('/').map(|i| i + 1).unwrap_or(0);
    let bslash = lname.rfind('\\').map(|i| i + 1).unwrap_or(0);
    &lname[slash.max(bslash)..]
}

/// 画像登録の優先度 tier (小さいほど優先)。総画像数が `IMAGE_LIMIT` を超える
/// シナリオ (スパロボ戦記 = 6500+ 枚 > 4500) では tier の大きいものが切り捨て
/// られるため、UI に必須の小アイコンを確実に残すための分類。
///
/// - 0: `event/` の UI アイコン (武器属性/種別/射撃/格闘/複合/次のページ 等。
///   ステータス画面で必須。点数が少なく小さいので最優先で確保) +
///   `Bitmap/Unit` `Bitmap/Pilot` (マップスプライト / 機体アイコン / 顔グラ)。
/// - 1: `Anime/Unit` (タイトル全身画像 + 戦闘アニメ)。`/unit/` を含むが
///   `Bitmap/Unit` とは別物なので分離する。
/// - 2: それ以外 (背景 / エフェクト 等)。超過時に最初に切られる。
fn image_priority_tier(name: &str) -> u8 {
    // 先頭に `/` を補い、ルート直下の `Unit/` `Pilot/` `event/` もマッチさせる。
    let l = format!("/{}", name.to_ascii_lowercase().replace('\\', "/"));
    if l.contains("/event/") {
        return 0;
    }
    let unit_or_pilot = l.contains("/unit/") || l.contains("/pilot/");
    match (unit_or_pilot, l.contains("/anime/")) {
        (true, false) => 0,
        (true, true) => 1,
        _ => 2,
    }
}

/// 元 SRC `NonPilotDataList.Load` (`NonPilotDataList.cls:120`) の最小読込。
///
/// 1 レコード = 2 行（空行区切り）:
///
/// ```text
/// 名前
/// ニックネーム,Bitmap.bmp
/// ```
///
/// `App` 側にはまだ NonPilot 専用テーブルが無いので、当面はカウントだけ返し、
/// 各レコードを最低限の `PilotData` として `database.pilots` に push する。
/// Talk 系イベントから "名前" で検索される時にヒットさせるための仮置きで、
/// 戦闘ステータスは全て 0 / 空。
fn parse_non_pilots_into(app: &mut src_core::App, txt: &str) -> usize {
    use src_core::data::pilot::{Adaption, PilotData, Sex};
    let mut count = 0;
    let mut lines = txt.lines().map(str::trim);
    while let Some(name_line) = lines.next() {
        if name_line.is_empty() || name_line.starts_with(';') {
            continue;
        }
        // 次の非空行を bitmap 行として消費
        let Some(meta_line) = lines.find(|l| !l.is_empty()) else {
            break;
        };
        let (nick, bitmap) = match meta_line.split_once(',') {
            Some((n, b)) => (n.trim(), b.trim()),
            None => (meta_line, ""),
        };
        let pilot = PilotData {
            spirit_commands: Vec::new(),
            name: name_line.to_string(),
            nickname: nick.to_string(),
            kana_name: nick.to_string(),
            sex: Sex::Unspecified,
            class: String::new(),
            adaption: Adaption::parse("----").unwrap_or(Adaption([b'-'; 4])),
            exp_value: 0,
            infight: 0,
            shooting: 0,
            hit: 0,
            dodge: 0,
            intuition: 0,
            technique: 0,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: if bitmap.is_empty() {
                None
            } else {
                Some(bitmap.to_string())
            },
            features: Vec::new(),
        };
        app.database_mut().pilots.push(pilot);
        count += 1;
    }
    count
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
        // 元 SRC 配布 zip はファイル名が Shift_JIS で UTF-8 フラグなし。
        // zip クレートはこの場合 CP437 として誤デコードするので、
        // 生バイトを取り出して loader::decode_text (SJIS 自動判定) で UTF-8 化。
        let name = loader::decode_text(entry.name_raw());
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        out.push((name, buf));
    }
    Ok(out)
}

/// LZH / LHA アーカイブ (lh0 / lh1 / lh4 / lh5 / lh6 / lh7 等) を展開。
/// `delharc` クレートに丸投げし、各エントリ名を `loader::decode_text` で
/// UTF-8 化する。
pub fn unlzh(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    use std::io::{Cursor, Read as _};
    let mut reader = delharc::LhaDecodeReader::new(Cursor::new(bytes))
        .map_err(|e| format!("LZH ヘッダの読み取り失敗: {e}"))?;
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

// NOTE: 同等の実装が `tools/verify-archive/src/main.rs` にも存在する。
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
    // EXT_HEADER_FILENAME がなければ Level 0/1 ヘッダーの header.filename を採用。
    // EXT_HEADER_PATH(ディレクトリ)+ header.filename(ベース名)で構成される LZH が
    // 実在するため、両者を必ず連結する。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lone_eve_text_is_executed_as_event() {
        let mut app = App::new();
        let mut assets = Assets::default();
        let src = "Stage \"テスト\"\nMapSize 2 2\n";
        let noop: Rc<dyn Fn()> = Rc::new(|| {});
        let log = load_into_app(
            &mut app,
            &mut assets,
            "demo.eve",
            src.as_bytes(),
            &noop,
            true,
        )
        .unwrap();
        assert!(log.contains("demo.eve"));
        assert_eq!(app.stage(), "テスト");
        assert!(app.database().map.is_some());
    }

    #[test]
    fn image_priority_event_icons_are_top_tier() {
        // event/ の UI アイコンは最優先 (tier 0) — 画像数超過時も切り捨てない。
        // スパロボ戦記 のステータス画面の属性/種別/射撃/格闘アイコンが該当。
        assert_eq!(image_priority_tier("Bitmap\\event\\属性.png"), 0);
        assert_eq!(image_priority_tier("Bitmap/event/射撃.png"), 0);
        assert_eq!(image_priority_tier("bitmap\\EVENT\\次のページへ.png"), 0);
        // Unit / Pilot スプライトも tier 0。
        assert_eq!(image_priority_tier("Bitmap\\Unit\\foo.bmp"), 0);
        assert_eq!(image_priority_tier("Bitmap\\Pilot\\bar.bmp"), 0);
        // Anime/Unit は tier 1。
        assert_eq!(image_priority_tier("Anime\\Unit\\EC_X.bmp"), 1);
        // 背景 / エフェクトは tier 2 (超過時に最初に切られる)。
        assert_eq!(image_priority_tier("Back\\EFFECT_Window.gif"), 2);
        assert_eq!(image_priority_tier("Bitmap\\Anime\\Effect\\boom.bmp"), 2);
    }
}
