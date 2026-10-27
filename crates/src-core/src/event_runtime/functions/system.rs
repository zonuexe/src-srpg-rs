//! 組み込み関数: ゲーム進行・時刻・画面・入力・ファイル・変数の問い合わせ。

use super::*;

pub(super) fn eval(app: &App, c: &FnCall) -> Option<Option<String>> {
    Some(match c.canon.as_str() {
        "Args" => fn_args(app, c),
        "Money" => fn_money(app, c),
        "Turn" => fn_turn(app, c),
        "Phase" => fn_phase(app, c),
        "Stage" => fn_stage(app, c),
        "Now" => fn_now(app, c),
        "GetTime" => fn_get_time(app, c),
        "Year" | "Month" | "Day" | "Hour" | "Minute" | "Second" | "Weekday" => fn_year(app, c),
        "DiffTime" => fn_diff_time(app, c),
        "IsVarDefined" => fn_is_var_defined(app, c),
        "Term" => fn_term(app, c),
        "TextWidth" => fn_text_width(app, c),
        "TextHeight" => fn_text_height(app, c),
        "KeyState" => fn_key_state(app, c),
        "PlayingMidi" | "PlayingSound" => fn_playing_midi(app, c),
        "WindowWidth" => fn_window_width(app, c),
        "WindowHeight" => fn_window_height(app, c),
        "Font" => fn_font(app, c),
        "PointX" | "BaseX" => fn_point_x(app, c),
        "PointY" | "BaseY" => fn_point_y(app, c),
        "Dir" => fn_dir(app, c),
        "FileExists" => fn_file_exists(app, c),
        "FolderExists" => fn_folder_exists(app, c),
        "FileLen" => fn_file_len(app, c),
        "Loc" => fn_loc(app, c),
        "EOF" => fn_eof(app, c),
        "LOF" => fn_lof(app, c),
        "Info" => fn_info(app, c),
        "LoadFileDialog" | "SaveFileDialog" => fn_load_file_dialog(app, c),
        _ => return None,
    })
}

fn fn_args(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // Args(N) — 値は Set 時に Args(N) というキーで script_var に格納済み。
    // N は数値リテラルだけでなく変数でも良い (`Args(配置用乱数)` 等) ため、
    // まず `fn_arg_value` で解決してから数値判定する。
    let n = fn_arg_value(app, args.first()?);
    if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
        Some(app.script_var(&format!("Args({n})")).to_string())
    } else {
        None
    }
}

fn fn_money(app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    Some(app.money().to_string())
}

fn fn_turn(app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    Some(app.turn().number.to_string())
}

fn fn_phase(app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    Some(match app.turn().phase {
        crate::Phase::Player => "Player".to_string(),
        crate::Phase::Enemy => "Enemy".to_string(),
        crate::Phase::Neutral => "Neutral".to_string(),
        crate::Phase::Npc => "NPC".to_string(),
    })
}

fn fn_stage(app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    Some(app.stage().to_string())
}

fn fn_now(app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    // SRC `Now` システム変数相当の関数アクセス: `YYYY/MM/DD HH:MM:SS`。
    // src-web が `Date::now()` で `App.wall_clock_ms` を更新する前提。
    // src-core 単独 (テスト) では `1970/01/01 00:00:00` 固定。
    Some(crate::time_util::format_now(app.wall_clock_ms()))
}

fn fn_get_time(app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    // SRC `GetTime()` — システム起動からのミリ秒。本実装はサンプル時刻
    // からの差分が分からないため、Unix epoch ミリ秒をそのまま返す。
    // SRC.NET `GeneralLib.timeGetTime()` の用途は主に乱数シード等で、
    // 単調増加していれば仕様充足する。
    Some(format_num(app.wall_clock_ms()))
}

fn fn_year(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { name, args, .. } = *c;
    // `Year([time_str])` 等。引数省略時は `Now()` の値を使う。
    // SRC.NET 仕様: 引数が IsDate で解釈できない場合は 0 を返す。
    let epoch_ms = if let Some(arg) = args.first() {
        let s = fn_arg_value(app, arg);
        if s.trim().is_empty() {
            app.wall_clock_ms()
        } else {
            match crate::time_util::parse_datetime(&s) {
                Some(v) => v,
                None => return Some("0".to_string()),
            }
        }
    } else {
        app.wall_clock_ms()
    };
    let b = crate::time_util::breakdown(epoch_ms);
    Some(match name {
        "Year" => b.year.to_string(),
        "Month" => b.month.to_string(),
        "Day" => b.day.to_string(),
        "Hour" => b.hour.to_string(),
        "Minute" => b.minute.to_string(),
        "Second" => b.second.to_string(),
        "Weekday" => crate::time_util::weekday_name(b.weekday).to_string(),
        _ => unreachable!(),
    })
}

fn fn_diff_time(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `DiffTime(t1, t2)` — t1 から t2 までの秒数 (t2 - t1)。
    // どちらも `Now` (=リテラル文字列) または datetime 文字列を許可。
    if args.len() < 2 {
        return None;
    }
    let resolve = |arg: &str| -> Option<f64> {
        let s = fn_arg_value(app, arg);
        let t = s.trim();
        if t.is_empty() || t.eq_ignore_ascii_case("now") {
            return Some(app.wall_clock_ms());
        }
        crate::time_util::parse_datetime(t)
    };
    let t1 = resolve(args[0])?;
    let t2 = resolve(args[1])?;
    Some(format_num((t2 - t1) / 1000.0))
}

fn fn_is_var_defined(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `IsVarDefined(name)` — 変数 / 配列要素が定義されていれば 1。
    // SRC.Sharp `Expression.IsVariableDefined` 同等: 空文字代入でも
    // **定義済 (1)** を返す (`Set var ""` 後の判定が 1)。
    //
    // 元 SRC は名称を文字通り (式評価せず) 使うが、本実装では
    // expand_vars 経由で既に解決済の文字列を script_var キーとして引く。
    // 数値リテラルが来た場合は (= 既に値で置換された) 「定義済み」と
    // 判定する。
    // 引数が無い / 空 (`IsVarDefined()`) は「未定義」= 0。
    // `args.first()` が None のときも literal 落ちさせず 0 を返す。
    let raw_orig = args.first().map_or("", |s| s.trim());
    // SRC.Sharp 準拠: `$` プレフィックスは除去してから参照する。
    let raw = raw_orig.strip_prefix('$').unwrap_or(raw_orig);
    if raw.is_empty() {
        return Some("0".to_string());
    }
    if raw.parse::<f64>().is_ok() {
        return Some("1".to_string());
    }
    // 直接 / 解決済の indexed key — `contains_key` で空代入も拾う
    if app.is_script_var_defined(raw) {
        return Some("1".to_string());
    }
    let resolved = resolve_lhs_name(app, raw);
    Some(
        if app.is_script_var_defined(&resolved) {
            "1"
        } else {
            "0"
        }
        .to_string(),
    )
}

fn fn_term(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Term(用語[, unitId])` — RenameTerm で設定された用語別名を引く。
    // 別名未設定なら用語名そのものを返す (SRC.Sharp 準拠)。
    // `unitId` は「用語名」特殊能力オーバーライド用だが本実装は無視。
    let term = fn_arg_value(app, args.first()?);
    let key = format!("__term_{term}");
    let alias = app.script_var(&key).to_string();
    if alias.is_empty() {
        Some(term)
    } else {
        Some(alias)
    }
}

fn fn_text_width(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `TextWidth(text)` — 描画幅 (ピクセル) の近似値。フォントサイズが
    // ランタイムに保持されていないため、char_count × 14 (14pt 等幅相当) で
    // 概算する。
    let s = fn_arg_value(app, args.first()?);
    let s = s.trim_matches('"');
    Some((s.chars().count() as i32 * 14).to_string())
}

fn fn_text_height(_app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    // `TextHeight(text)` — 1 行高さ。フォントサイズ 14pt 想定で 20 を返す。
    Some("20".to_string())
}

fn fn_key_state(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `KeyState(2)` — 右マウスボタン。SRC `Wait Click` の「右ボタン = キャンセル」
    // を実現するため、直近の Wait Click を右クリックで解除していれば 1 を返す。
    // これでステータス画面の `Case "" → If KeyState(2) Then Break` が機能する。
    // 重要: 右クリックでない場合は **下のビジーループ脱出ロジックへフォール
    // スルー** する (early-return しない)。`Do While (KeyState(2)=0)` 形式の
    // ビジーループが脱出できず STEP_LIMIT までフリーズするのを防ぐため。
    if let Some(a) = args.first() {
        if fn_arg_value(app, a).trim() == "2" && app.take_wait_click_right() {
            // ワンショット消費: 直後の `Do While KeyState(2) Loop` (解放待ち) は
            // 0 を見て即脱出する。これをしないと無限ループで STEP_LIMIT に達する。
            return Some("1".to_string());
        }
    }
    // `KeyState(n)` — 押されているキー番号を判定。Web 移植ではリアルタイムの
    // キー押下状態を取得できないため、通常は 0 を返す。
    //
    // ただし `Do While (KeyState()=0) ... Loop` パターン（ユーザ入力待ちの
    // ビジーループ）が STEP_LIMIT まで走り続けてブラウザをフリーズさせるのを
    // 防ぐため、同一スクリプト実行内で一定回数呼ばれた後は "1"（押された）を
    // 返してループを強制終了させる。
    // しきい値 4: Do While (KeyState=0) ループの初回評価は 3 回以内に
    // KeyState を呼ぶことが多く、4 回目以降で "1" を返すとループが脱出する。
    const KEYSTATE_AUTO_BREAK_THRESHOLD: usize = 4;
    let count = app.increment_keystate_call_count();
    if count >= KEYSTATE_AUTO_BREAK_THRESHOLD {
        Some("1".to_string())
    } else {
        Some("0".to_string())
    }
}

fn fn_playing_midi(_app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    Some("0".to_string())
}

fn fn_window_width(_app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    // メインウィンドウの横幅 (ピクセル)。WASM 環境では 800 を返す。
    Some("800".to_string())
}

fn fn_window_height(_app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    // メインウィンドウの縦幅 (ピクセル)。WASM 環境では 600 を返す。
    Some("600".to_string())
}

fn fn_font(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Font(属性名)` — 現在のフォント設定を返す。
    // WASM 環境ではフォントをここから取得できないため固定値を返す。
    let attr = fn_arg_value(app, args.first()?);
    match attr.trim().trim_matches('"') {
        "フォント名" => Some("MS ゴシック".to_string()),
        "サイズ" => Some("12".to_string()),
        "太字" => Some("0".to_string()),
        "斜体" => Some("0".to_string()),
        "色" => Some("#000000".to_string()),
        "書き込み" => Some("通常".to_string()),
        _ => Some("".to_string()),
    }
}

fn fn_point_x(app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    // 描画カーソル X 座標 (`Line` / `PSet` / `PaintString` の終端座標)。
    // SRC.NET `Event.BaseX` (`picMain.CurrentX`) 同等。
    Some(format_num(app.script_overlay().cursor_x))
}

fn fn_point_y(app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    Some(format_num(app.script_overlay().cursor_y))
}

fn fn_dir(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Dir(path, kind)` — 仮想ファイルシステムに該当ファイルが
    // あれば basename を返す ("見つかった")。無ければ空文字列。
    // 実ファイルシステムへのアクセスは行わない。
    let path = args
        .first()
        .map(|p| fn_arg_value(app, p))
        .unwrap_or_default();
    Some(
        app.virtual_file_basename_if_exists(&path)
            .unwrap_or_default(),
    )
}

fn fn_file_exists(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `FileExists(path)` — VFS に該当ファイルが存在するなら 1、無ければ 0。
    let path = args
        .first()
        .map(|p| fn_arg_value(app, p))
        .unwrap_or_default();
    Some(
        if app.virtual_file_exists(&path) {
            "1"
        } else {
            "0"
        }
        .to_string(),
    )
}

fn fn_folder_exists(_app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    // `FolderExists(path)` — 本実装は VFS にフォルダの概念がない
    // ため常に 0。SRC.Sharp との乖離だが、シナリオの大半は使わない。
    Some("0".to_string())
}

fn fn_file_len(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `FileLen(path)` — VFS 上のファイルサイズ (UTF-8 バイト数)。
    // 未登録なら 0。
    let path = args
        .first()
        .map(|p| fn_arg_value(app, p))
        .unwrap_or_default();
    Some(app.virtual_file_len(&path).to_string())
}

fn fn_loc(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Loc(handle)` — ファイルハンドルの読取カーソル (行番号)。
    let handle = args
        .first()
        .map(|p| fn_arg_value(app, p))
        .unwrap_or_default();
    Some(app.vfs_loc(&handle).to_string())
}

fn fn_eof(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `EOF(handle)` — 末尾に到達していれば 1、まだ読めるなら 0。
    let handle = args
        .first()
        .map(|p| fn_arg_value(app, p))
        .unwrap_or_default();
    Some(if app.vfs_eof(&handle) { "1" } else { "0" }.to_string())
}

fn fn_lof(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `LOF(handle)` — 開いているファイルの総行数。
    let handle = args
        .first()
        .map(|p| fn_arg_value(app, p))
        .unwrap_or_default();
    Some(app.vfs_lof(&handle).to_string())
}

fn fn_info(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // 元 SRC の Info(): ユニット / パイロット / アイテム / マップ等の
    // 動的データ照会。`info_query` ヘルパに委譲。
    Some(info_query(app, args))
}

fn fn_load_file_dialog(app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    // 実機はファイル選択ダイアログを開く。ヘッドレス (WASM/検証) には GUI が無いので、
    // script_var `__verify_loadfile` に設定されたパスを返す。未設定なら "" =「キャンセル
    // された」相当 (通常プレイでも "" のままなので副作用なし)。検証ドライバが
    // `__verify_loadfile` をセットすると `データロード` 経路を駆動できる。
    Some(app.script_var("__verify_loadfile").to_string())
}
