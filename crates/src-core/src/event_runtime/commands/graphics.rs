//! `.eve` コマンド: 画面描画・画像・エフェクト・ウィンドウ設定。

use super::*;

pub(super) fn exec(app: &mut App, c: &Cmd) -> Option<Result<usize, ScriptError>> {
    Some(match c.lname.as_str() {
        "effect" => exec_effect(app, c),
        "sepia" | "monotone" | "colorfilter" => exec_sepia(app, c),
        "whiteout" => exec_whiteout(app, c),
        "whitein" => exec_whitein(app, c),
        "showimage" => exec_showimage(app, c),
        "show" => exec_show(app, c),
        "hide" => exec_hide(app, c),
        "refresh" => exec_refresh(app, c),
        "hotpointstring" => exec_hotpointstring(app, c),
        "hotpoint" => exec_hotpoint(app, c),
        "clearpicture" => exec_clearpicture(app, c),
        "clearobj" => exec_clearobj(app, c),
        "font" => exec_font(app, c),
        "paintstring" | "paintstringr" | "paintsysstring" => exec_paintstring(app, c),
        "paintpicture" => exec_paintpicture(app, c),
        "color" => exec_color(app, c),
        "drawwidth" => exec_drawwidth(app, c),
        "cls" => exec_cls(app, c),
        "savescreen" => exec_savescreen(app, c),
        "loadscreen" => exec_loadscreen(app, c),
        "playflash" => exec_playflash(app, c),
        "stopflash" | "clearflash" => exec_stopflash(app, c),
        "sunset" => exec_sunset(app, c),
        "noon" => exec_noon(app, c),
        "night" => exec_night(app, c),
        "changeunitbitmap" => exec_changeunitbitmap(app, c),
        "changepilotbitmap" => exec_changepilotbitmap(app, c),
        "renameterm" => exec_renameterm(app, c),
        "setwindowframewidth" => exec_setwindowframewidth(app, c),
        "setwindowcolor" => exec_setwindowcolor(app, c),
        "setstatusstringcolor" => exec_setstatusstringcolor(app, c),
        "fillstyle" => exec_fillstyle(app, c),
        "background" | "drawoption" | "renametitle" | "renamebgm" | "freememory" | "exec"
        | "make" | "playmovie" | "redraw" | "setbackground" => exec_background(app, c),
        "line" => exec_line(app, c),
        "pset" => exec_pset(app, c),
        "circle" => exec_circle(app, c),
        "oval" => exec_oval(app, c),
        "arc" => exec_arc(app, c),
        "polygon" => exec_polygon(app, c),
        "fillcolor" => exec_fillcolor(app, c),
        "fadeout" => exec_fadeout(app, c),
        "fadein" => exec_fadein(app, c),
        _ => return None,
    })
}

fn exec_effect(_app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Effect <name> [target] [params...]` — visual effect
    log::info!(
        "Effect: {} (stub)",
        xargs.first().map(|s| s.as_str()).unwrap_or("")
    );
    Ok(pc + 1)
}

fn exec_sepia(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        lname, xargs, pc, ..
    } = *c;
    // SRC ビジュアルフィルタ: `Sepia` / `Monotone` / `ColorFilter` —
    // 画面全体に色フィルタを適用する。本実装は `DrawCmd::Fade` で擬似的に
    // 半透明のオーバーレイ色を被せる。
    // Sepia → セピア (薄茶)、Monotone → グレー、ColorFilter → 引数指定。
    let color = match lname.as_str() {
        "sepia" => "rgba(112,66,20,0.35)".to_string(),
        "monotone" => "rgba(128,128,128,0.35)".to_string(),
        "colorfilter" => xargs
            .first()
            .map(|c| resolve_color(app, c))
            .unwrap_or_else(|| "rgba(0,0,0,0.2)".to_string()),
        _ => "rgba(0,0,0,0.2)".to_string(),
    };
    app.script_overlay_mut()
        .push(crate::script_overlay::DrawCmd::Fade { color, alpha: 1.0 });
    Ok(pc + 1)
}

fn exec_whiteout(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `WhiteOut n`: 画面が白へフェードアウト (終状態: 白で覆う)。
    // 引数 n は 0..255 程度の段階値。n が大きいほど不透明 (= より白い)。
    // アニメーションせず終状態のみ描くので白の全画面 Fade を 1 枚積む。
    let n = c.long(app, 0).map_or(255.0, |n| n as f64);
    let alpha = (n / 255.0).clamp(0.0, 1.0);
    app.script_overlay_mut()
        .push(crate::script_overlay::DrawCmd::Fade {
            color: "#ffffff".to_string(),
            alpha,
        });
    Ok(pc + 1)
}

fn exec_whitein(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `WhiteIn n`: 白から通常画面へフェードイン。終状態は **通常画面**
    // (白を除去)。旧実装は WhiteOut と同一視して白を積んでおり、引数なし
    // (`WhiteIn` → n=255 → alpha=1.0) で全画面が白のまま残り「白いマップ」で
    // 操作不能になっていた (東方夢想伝: タイトルテロップ末尾の `WhiteIn`)。
    // フェードインなので白の全画面 Fade を取り除いて画面を露出させる。
    app.script_overlay_mut()
        .remove_fades_of(is_white_fade_color);
    Ok(pc + 1)
}

fn exec_showimage(_app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // `ShowImage bitmap x y [z]` — 画像を指定座標に表示。
    // 本実装はスクリプトオーバーレイに PaintPicture 相当で委譲。
    // 当面 no-op で安全スルー。
    Ok(pc + 1)
}

fn exec_show(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // `Show` — マップウィンドウを表示する SRC コマンド。
    // 本実装ではタイトル / Configuration から MapView に遷移する。
    // Intermission 中はエピローグで `Show` が呼ばれても上書きしない。
    if matches!(
        app.scene(),
        crate::Scene::Title | crate::Scene::Configuration
    ) {
        app.set_scene(crate::Scene::MapView);
    }
    Ok(pc + 1)
}

fn exec_hide(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `Hide` (`Hideコマンド.md`) ─ メインウィンドウを隠す。
    // 本実装はウィンドウ可視フラグを持たないため、`script_overlay` を
    // クリアして「メインウィンドウが空」相当の状態を作る (Cls 相当)。
    // SRC.NET の挙動 (背景画像のみ残す) とは厳密一致しないが、
    // プロローグ風カット導入では十分。
    app.script_overlay_mut().clear();
    Ok(pc + 1)
}

fn exec_refresh(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // 元 SRC `Refresh` は **画面の強制再描画 (present) のみ**で、
    // 蓄積した描画内容はクリアしない (クリアは `Cls` / `ClearPicture` /
    // `ClearObj` の役割)。本実装のレンダラは `script_overlay` を毎フレーム
    // 描き直すので、`Refresh` は実質 no-op。
    //
    // 以前は `script_overlay.clear()` していたが、これは誤りで、
    // `draw / Refresh / draw` で背景フレームが消えてしまう。スパロボ戦記の
    // タイトル系 (`PaintPicture メッセージウィンドウ / Refresh / Wait` の
    // フレームアニメ、`HotpointString ... / Refresh / Wait Click` 等) は
    // Refresh の後も描画が残ることを前提にしており、クリアすると
    // 「画面が一瞬見えてすぐ消える」状態になっていた。
    //
    // present(): 保留中の ClearPicture があればここで適用する
    // (`ClearPicture; Refresh` でバックバッファの空を表示するケース)。
    // 通常パターン `Paint; Refresh; ClearPicture` では Refresh 時点で
    // 保留は無いので従来どおり no-op。
    app.script_overlay_mut().present();
    Ok(pc + 1)
}

fn exec_hotpointstring(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `HotpointString x y text...` — 文字列を描画しつつ、その矩形領域を
    // クリック領域として登録する。x が "-" の場合は水平方向中央寄せ
    // (簡易実装)。クリック名はテキストそのもの。
    let (x, y, text) = parse_paintstring_args(xargs);
    if !text.is_empty() {
        app.script_overlay_mut()
            .push(crate::script_overlay::DrawCmd::PaintString {
                x,
                y,
                text: text.clone(),
            });
        // Hotpoint 領域の幅は文字数 × ピクセル係数 (概算)。
        // フォントサイズが取れないので 14pt 相当 (高さ ~20px) を仮定。
        let char_count = text.chars().count() as i32;
        let w = (char_count * 14).max(40);
        let h = 22;
        app.push_hotpoint(crate::event_runtime::HotpointEntry {
            name: text,
            x: x as i32,
            y: y as i32 - h,
            w,
            h,
            invisible: false,
        });
    }
    Ok(pc + 1)
}

fn exec_hotpoint(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Hotpoint name x y w h [非表示]`
    // name = クリック時に `選択` に格納される文字列 (unit 名など)
    // 引数は算術式 `(x - 25)` 形式が含まれるので eval する。
    if xargs.is_empty() {
        return Ok(pc + 1);
    }
    let name = xargs[0].clone();
    // 座標 / サイズは原典どおり `GetArgAsLong`。
    let num = |i: usize| c.long(app, i).unwrap_or(0) as i32;
    let (x, y, w, h) = (num(1), num(2), num(3), num(4));
    let invisible = xargs
        .get(5)
        .map(|s| s == "非表示" || s.eq_ignore_ascii_case("invisible"))
        .unwrap_or(false);
    app.push_hotpoint(crate::event_runtime::HotpointEntry {
        name,
        x,
        y,
        w,
        h,
        invisible,
    });
    Ok(pc + 1)
}

fn exec_clearpicture(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // `ClearPicture` — Paint / PaintPicture で積んだ表示を消去。
    // ただし SRC immediate-mode ではバックバッファのみを消し、画面は直前
    // Refresh が表示したフレームを次の present まで保持する。本実装は
    // 遅延クリア (defer_clear) で、次の描画 push か Refresh まで cmds を残す。
    // → 汎用戦闘アニメの `Paint; Refresh; ClearPicture; Wait` 各フレームが
    //   Wait 中も正しく表示される (即 clear すると毎フレーム空になっていた)。
    // Hotpoint は別管理なのでそのまま。
    //
    // `ClearPicture x1 y1 x2 y2` (原典 `ExecClearPictureCmd` の 4 引数形。座標は
    // `GetArgAsLong`) は矩形の内側だけを消す。
    if c.xargs.len() == 4 {
        let n = |i: usize| c.long(app, i).unwrap_or(0) as f64;
        let (x1, y1, x2, y2) = (n(0), n(1), n(2), n(3));
        app.script_overlay_mut()
            .push(crate::script_overlay::DrawCmd::ClearRect { x1, y1, x2, y2 });
        return Ok(pc + 1);
    }
    app.script_overlay_mut().defer_clear();
    Ok(pc + 1)
}

fn exec_clearobj(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // `ClearObj` — オブジェクト消去 = Hotpoint と画像オブジェクトを
    // 一括クリア。Refresh とは別に明示的に呼ばれる。
    app.script_overlay_mut().clear();
    app.clear_hotpoints();
    Ok(pc + 1)
}

fn exec_font(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Font [family] [Npt] [Bold|Italic|Underline] [#color | カラー名]`
    // 引数順は任意 (実シナリオは "14pt Bold" / "50pt Italic Bold" /
    // "Ｐゴシック 14pt" / "#5E5E5E" 等の様々な並びを使う)。
    // 引数 0 個なら既定値にリセット。
    let mut family = String::from("sans-serif");
    let mut size: u32 = 14;
    let mut color = String::from("#ffffff");
    let mut style_bold = false;
    let mut style_italic = false;
    // 各トークンを走査する前に、定義済みスクリプト変数を値へ展開する。
    // `Font LetterShadow` / `Font LetterColor1` のように変数でフォント
    // 指定を切り替える実シナリオ記法 (Alpha2ndStatus.ini 由来のテーマ
    // 設定) に対応するため。変数値は "10pt #000000" のように複数トークン
    // でありうるので空白で再分割する。未定義変数はリテラル扱い。
    let mut tokens: Vec<String> = Vec::new();
    for tok in xargs.iter() {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }
        let v = app.script_var(t);
        if !v.is_empty() {
            tokens.extend(v.split_whitespace().map(str::to_string));
        } else {
            tokens.push(t.to_string());
        }
    }
    for tok in &tokens {
        let t = tok.trim();
        if t.is_empty() {
            continue;
        }
        // サイズ: "14pt" / "14"
        let size_candidate = t.strip_suffix("pt").or_else(|| t.strip_suffix("PT"));
        if let Some(sn) = size_candidate {
            if let Ok(n) = sn.parse::<u32>() {
                size = n;
                continue;
            }
        }
        if let Ok(n) = t.parse::<u32>() {
            if (6..=144).contains(&n) {
                size = n;
                continue;
            }
        }
        // 色: "#RRGGBB" or 色名 (canonical_color が日本語色名にも対応)
        let lc = t.to_ascii_lowercase();
        if t.starts_with('#') || lc.starts_with("rgb(") {
            color = canonical_color(t);
            continue;
        }
        match t {
            "Bold" | "bold" => {
                style_bold = true;
                continue;
            }
            "Italic" | "italic" => {
                style_italic = true;
                continue;
            }
            "Underline" | "underline" => continue,
            "黒" | "白" | "赤" | "緑" | "青" | "黄" | "灰" => {
                color = canonical_color(t);
                continue;
            }
            _ => {}
        }
        // 残りは family 名として扱う
        family = t.to_string();
    }
    // style はフロントエンドが family 名から拾えるよう接頭辞で表現する。
    // canvas2d は font 文字列 "italic bold 14pt Family" を受け付ける。
    let prefix = match (style_italic, style_bold) {
        (true, true) => "italic bold ",
        (true, false) => "italic ",
        (false, true) => "bold ",
        (false, false) => "",
    };
    let family = format!("{prefix}{family}");
    app.script_overlay_mut()
        .push(crate::script_overlay::DrawCmd::SetFont {
            family,
            size_pt: size,
            color,
        });
    Ok(pc + 1)
}

fn exec_paintstring(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `PaintString x y text...` または `PaintString text x y` の二形式に
    // 対応するため、整数が見つかる位置で分岐。
    // 簡略実装: 引数の最初の 2 整数を x,y、残りを連結して text。
    let (x, y, text) = parse_paintstring_args(xargs);
    app.script_overlay_mut()
        .push(crate::script_overlay::DrawCmd::PaintString { x, y, text });
    Ok(pc + 1)
}

fn exec_paintpicture(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `PaintPicture path x y [w h] [透過] [左右反転]`
    // 算術式 `(x - 5)` を含むのでそれぞれ eval_int_expr で評価。
    // 座標 / サイズに "-" が指定された場合は「中央寄せ」「自然サイズ」を意味する。
    if xargs.is_empty() {
        return Ok(pc + 1);
    }
    let path = xargs[0].clone();
    let parse_int_or_dash = |s: &str| -> Option<i32> {
        if s.trim() == "-" {
            None
        } else {
            // 原典 `ExecPaintPictureCmd` は `StrToLng(GetArgAsString(i))`。
            Some(eval::str_to_lng(s) as i32)
        }
    };
    // x/y: "-" は MAP_AREA 中央寄せ。サイズは後で w/h と組み合わせて確定する。
    let raw_x = xargs.get(1).and_then(|s| parse_int_or_dash(s));
    let raw_y = xargs.get(2).and_then(|s| parse_int_or_dash(s));
    // オプション (透過 / 反転 / 白黒 等) は w/h を省略すると index 3 以降に
    // 直接現れる (`PaintPicture img - - 透過`)。最初のオプションキーワード
    // 位置 `opts_start` を求め、それより前の数値引数だけを w/h として解釈
    // する。これが無いと `透過` が w として食われ (eval で 0 → None)、
    // 透過フラグが index 5 以降からしか拾われず効かないため、不透明描画
    // となって機体能力.png が画面全体を覆い隠していた。
    let opts_start = (3..xargs.len())
        .find(|&i| is_paint_option(&xargs[i]))
        .unwrap_or(xargs.len());
    // w/h: `0` 以下は「画像の自然サイズを使う」とみなして None にする。
    // スパロボ戦記タイトルは `PaintPicture 画像 x y Lindex(Info(...全身画像),3) ...`
    // のようにサイズを Info() から取るが、未対応データだと空文字 → 0 と
    // 評価され、0×0 で何も描画されなくなるため (機体アイコンが出ない)。
    let w = if opts_start > 3 {
        xargs
            .get(3)
            .and_then(|s| parse_int_or_dash(s))
            .map(f64::from)
            .filter(|v| *v > 0.0)
    } else {
        None
    };
    let h = if opts_start > 4 {
        xargs
            .get(4)
            .and_then(|s| parse_int_or_dash(s))
            .map(f64::from)
            .filter(|v| *v > 0.0)
    } else {
        None
    };
    const MAP_CENTER_X: f64 = 240.0;
    const MAP_CENTER_Y: f64 = 240.0;
    // 座標 `-` は中央寄せ。幅が明示されていれば src-core で確定できるが、
    // `PaintPicture img - -`（幅省略 = 画像実寸）の場合は実寸が分かる
    // フロントエンドで確定させる必要があるため center フラグを立てる。
    // (src-core の x/y はプレースホルダ。w 明示時も render が同値に再計算する)
    let center_x = raw_x.is_none();
    let center_y = raw_y.is_none();
    let x = match raw_x {
        Some(v) => f64::from(v),
        None => MAP_CENTER_X - w.unwrap_or(0.0) / 2.0,
    };
    let y = match raw_y {
        Some(v) => f64::from(v),
        None => MAP_CENTER_Y - h.unwrap_or(0.0) / 2.0,
    };
    let mut transparent = false;
    let mut flip_x = false;
    let mut flip_y = false;
    let mut monochrome = false;
    let mut sepia = false;
    let mut half_mode = String::new();
    let mut rotation_deg: f64 = 0.0;
    let mut as_background = false;
    let mut persist = false;
    let mut i_flag = opts_start;
    while i_flag < xargs.len() {
        let flag = &xargs[i_flag];
        let l = flag.to_ascii_lowercase();
        if flag == "透過" || l == "transparent" {
            transparent = true;
        } else if flag == "左右反転" || l == "flip" || l == "flipx" || l == "fliph" {
            flip_x = true;
        } else if flag == "上下反転" || l == "flipy" || l == "flipv" {
            flip_y = true;
        } else if flag == "白黒" || l == "monochrome" || l == "grayscale" {
            monochrome = true;
        } else if flag == "セピア" || l == "sepia" {
            sepia = true;
        } else if flag == "背景" || l == "background" {
            as_background = true;
        } else if flag == "保持" || l == "persist" || l == "keep" {
            persist = true;
        } else if flag == "右回転" || l == "rotate" || l == "rotateright" {
            // 次の引数を角度として消費
            if let Some(angle_arg) = xargs.get(i_flag + 1) {
                if let Ok(a) = angle_arg.trim().parse::<f64>() {
                    rotation_deg = a;
                    i_flag += 1;
                }
            }
        } else if flag == "左回転" || l == "rotateleft" {
            if let Some(angle_arg) = xargs.get(i_flag + 1) {
                if let Ok(a) = angle_arg.trim().parse::<f64>() {
                    rotation_deg = -a;
                    i_flag += 1;
                }
            }
        } else if matches!(
            flag.as_str(),
            "上半分" | "下半分" | "左半分" | "右半分" | "右上" | "左上" | "右下" | "左下"
        ) {
            half_mode = flag.clone();
        }
        i_flag += 1;
    }
    app.script_overlay_mut()
        .push(crate::script_overlay::DrawCmd::Picture {
            path,
            x,
            y,
            w,
            h,
            transparent,
            flip_x,
            flip_y,
            monochrome,
            sepia,
            half_mode,
            rotation_deg,
            as_background,
            persist,
            center_x,
            center_y,
        });
    Ok(pc + 1)
}

fn exec_color(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Color RGB(r,g,b)` / `Color "Red"` / `Color rgb(...)` /
    // `Color FrameColor1` (.ini 由来のテーマ色変数) → 描画色を更新
    let color = xargs.join(" ").trim().trim_matches('"').to_string();
    if !color.is_empty() {
        let c = resolve_color(app, &color);
        app.script_overlay_mut()
            .push(crate::script_overlay::DrawCmd::SetColor { color: c });
    }
    Ok(pc + 1)
}

fn exec_drawwidth(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    let n = c.long(app, 0).map_or(1.0, |n| n as f64);
    app.script_overlay_mut()
        .push(crate::script_overlay::DrawCmd::SetLineWidth(n.max(0.5)));
    Ok(pc + 1)
}

fn exec_cls(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // ClearPicture と同義 (画面消去)
    app.script_overlay_mut().clear();
    Ok(pc + 1)
}

fn exec_savescreen(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `SaveScreen` (`SaveScreenコマンド.md`):
    // 現在の script_overlay をスナップショットとして保存。LoadScreen で復元。
    // serde 経由で JSON 化して script_var に格納する。
    if let Ok(json) = serde_json::to_string(app.script_overlay()) {
        app.set_script_var("__screen_snapshot".to_string(), json);
    }
    Ok(pc + 1)
}

fn exec_loadscreen(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `LoadScreen` (`LoadScreenコマンド.md`): SaveScreen で保存した
    // スナップショットを復元。script_overlay 全体を置換。
    let snapshot = app.script_var("__screen_snapshot").to_string();
    if !snapshot.is_empty() {
        if let Ok(restored) =
            serde_json::from_str::<crate::script_overlay::ScriptOverlay>(&snapshot)
        {
            *app.script_overlay_mut() = restored;
        }
    }
    Ok(pc + 1)
}

fn exec_playflash(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `PlayFlash file` (Flash 演出再生): フロントエンド側で対応する
    // 演出が無いため、`__playing_flash` フラグを script_var に立てる
    // (シナリオ側の `If 演出中 ...` 風の分岐用)。
    if let Some(name) = xargs.first() {
        app.set_script_var("__playing_flash".to_string(), name.clone());
    }
    Ok(pc + 1)
}

fn exec_stopflash(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `StopFlash` / `ClearFlash`: Flash 演出停止。
    app.set_script_var("__playing_flash".to_string(), String::new());
    Ok(pc + 1)
}

fn exec_sunset(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `Sunset` コマンド: マップを夕方状態にする。
    app.set_time_of_day("夕");
    Ok(pc + 1)
}

fn exec_noon(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `Noon` コマンド: マップを昼状態にする。
    app.set_time_of_day("昼");
    Ok(pc + 1)
}

fn exec_night(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `Night` コマンド: マップを夜状態にする。
    app.set_time_of_day("夜");
    Ok(pc + 1)
}

fn exec_changeunitbitmap(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `ChangeUnitBitmap [unit] bitmap` (`ChangeUnitBitmapコマンド.md`):
    // ユニットのビットマップを一時的に変更する。`-` で元に戻す。
    // `非表示`/`非表示解除` でユニットを非表示化/解除。
    if xargs.is_empty() {
        return Ok(pc + 1);
    }
    let (unit_key, bitmap) = if xargs.len() >= 2 {
        (xargs[0].clone(), xargs[1].clone())
    } else {
        (app.selected_unit_for_event().to_string(), xargs[0].clone())
    };
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &unit_key))
    {
        match bitmap.as_str() {
            "非表示" => u.is_bitmap_hidden = true,
            "非表示解除" => u.is_bitmap_hidden = false,
            "-" => {
                u.bitmap_override = None;
                u.is_bitmap_hidden = false;
            }
            _ => u.bitmap_override = Some(bitmap),
        }
    }
    Ok(pc + 1)
}

fn exec_changepilotbitmap(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `ChangePilotBitmap [pilot] bitmap` (`ChangeUnitBitmapコマンド.md` 参照):
    // パイロットのビットマップを一時的に変更する。`-` で元に戻す。
    // PilotInstance は通常空なので script_var `__pilot_bitmap_<name>` に格納。
    if xargs.is_empty() {
        return Ok(pc + 1);
    }
    let (pilot_key, bitmap) = if xargs.len() >= 2 {
        (xargs[0].clone(), xargs[1].clone())
    } else {
        (app.selected_unit_for_event().to_string(), xargs[0].clone())
    };
    // PilotInstance が存在すればそちらを優先更新。
    if let Some(p) = app
        .database_mut()
        .pilot_instances
        .iter_mut()
        .find(|p| p.pilot_data_name == pilot_key)
    {
        p.bitmap_override = if bitmap == "-" {
            None
        } else {
            Some(bitmap.clone())
        };
    }
    // script_vars にも記録 (PilotInstance 不在のケースに備えるフォールバック)。
    let key = format!("__pilot_bitmap_{pilot_key}");
    if bitmap == "-" {
        app.unset_script_var(&key);
    } else {
        app.set_script_var(key, bitmap);
    }
    Ok(pc + 1)
}

fn exec_renameterm(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // RenameTerm term name — 用語の表示名を変更する。
    // `Term(term)` 関数で読み戻せるよう `__term_<term>` に保存。
    // 例: `RenameTerm スペシャルパワー ヒーローアクション`
    // SRC.Sharp 準拠: 正確に 2 引数必要。
    if xargs.len() != 2 {
        return Err(err(line, "RenameTermコマンドの引数の数が違います"));
    }
    let term = &xargs[0];
    let new_name = &xargs[1];
    let key = format!("__term_{term}");
    app.set_script_var(key, new_name.clone());
    Ok(pc + 1)
}

fn exec_setwindowframewidth(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `SetWindowFrameWidth n` (SRC.Sharp `SetWindowFrameWidthCmd.cs`):
    // ステータスウィンドウの枠幅を n に設定し、グローバル変数
    // `StatusWindow(FrameWidth)` に n を格納する。
    if let Some(arg) = xargs.first() {
        let n = arg.to_string();
        app.set_script_var("StatusWindow(FrameWidth)".to_string(), n);
    }
    Ok(pc + 1)
}

fn exec_setwindowcolor(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `SetWindowColor #RRGGBB [枠|背景]` (SRC.Sharp `SetWindowColorCmd.cs`):
    // ウィンドウ色を設定。色は `#rrggbb` (7 文字) で指定。
    // Windows COLORREF 形式 `(B<<16)|(G<<8)|R` に変換して格納。
    // 対象: `枠`=FrameColor のみ / `背景`=BackBolor のみ / なし=両方。
    // ※ "BackBolor" は SRC.Sharp の typo (BackColor の誤記) をそのまま踏襲。
    if xargs.is_empty() {
        return Err(err(line, "SetWindowColor には色指定が必要。"));
    }
    let color_str = xargs[0].to_string();
    // `#RRGGBB` 形式チェック
    if color_str.len() != 7 || !color_str.starts_with('#') {
        return Err(err(line, "SetWindowColor: 色指定が不正 (#rrggbb 形式)。"));
    }
    let hex = &color_str[1..];
    let rgb = match u32::from_str_radix(hex, 16) {
        Ok(v) => v,
        Err(_) => return Err(err(line, "SetWindowColor: 色指定が不正 (16進数不正)。")),
    };
    // COLORREF = (B<<16)|(G<<8)|R
    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;
    let colorref = (b << 16) | (g << 8) | r;

    let target = xargs.get(1).map(|s| s.as_str()).unwrap_or("");
    let set_frame = target == "枠" || target.is_empty();
    let set_bg = target == "背景" || target.is_empty();
    if !set_frame && !set_bg {
        return Err(err(
            line,
            "SetWindowColor: 対象は「枠」または「背景」のみ。",
        ));
    }
    if set_frame {
        app.set_script_var("StatusWindow(FrameColor)".to_string(), colorref.to_string());
    }
    if set_bg {
        app.set_script_var("StatusWindow(BackBolor)".to_string(), colorref.to_string());
    }
    Ok(pc + 1)
}

fn exec_setstatusstringcolor(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `SetStatusStringColor #RRGGBB <対象>` (SRC.Sharp `SetStatusStringColorCmd.cs`):
    // ステータスウィンドウの文字色を設定。色は `#rrggbb` 形式。
    // 対象: `通常`=StringColor / `能力名`=ANameColor / `有効`=EnableColor / `無効`=DisableColor
    if xargs.len() < 2 {
        return Err(err(line, "SetStatusStringColor には色指定と対象が必要。"));
    }
    let color_str = xargs[0].to_string();
    if color_str.len() != 7 || !color_str.starts_with('#') {
        return Err(err(
            line,
            "SetStatusStringColor: 色指定が不正 (#rrggbb 形式)。",
        ));
    }
    let hex = &color_str[1..];
    let rgb = match u32::from_str_radix(hex, 16) {
        Ok(v) => v,
        Err(_) => {
            return Err(err(
                line,
                "SetStatusStringColor: 色指定が不正 (16進数不正)。",
            ))
        }
    };
    // COLORREF = (B<<16)|(G<<8)|R
    let r = (rgb >> 16) & 0xFF;
    let g = (rgb >> 8) & 0xFF;
    let b = rgb & 0xFF;
    let colorref = (b << 16) | (g << 8) | r;

    let target = xargs[1].to_string();
    let vname = match target.as_str() {
        "通常" => "StatusWindow(StringColor)",
        "能力名" => "StatusWindow(ANameColor)",
        "有効" => "StatusWindow(EnableColor)",
        "無効" => "StatusWindow(DisableColor)",
        _ => {
            return Err(err(
                line,
                "SetStatusStringColor: 対象は「通常」「能力名」「有効」「無効」のみ。",
            ))
        }
    };
    app.set_script_var(vname.to_string(), colorref.to_string());
    Ok(pc + 1)
}

fn exec_fillstyle(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `FillStyle <style>` — 図形 (Circle/Oval/Polygon/Arc) の塗り方式。
    // `透明`=塗りなし、それ以外 (`塗りつぶし`/`横線`/`縦線`/`斜線`/`クロス`/
    // `網かけ` 等) は本実装では solid 塗りで近似 (C# は != Transparent で塗る)。
    let style = xargs.first().map(|s| s.trim()).unwrap_or("");
    let solid = !style.is_empty() && style != "透明";
    app.script_overlay_mut()
        .push(crate::script_overlay::DrawCmd::SetFillSolid(solid));
    Ok(pc + 1)
}

fn exec_background(_app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // 表示 / システム / 拡張系: 現段階では no-op (将来拡張)
    Ok(pc + 1)
}

fn exec_line(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Line x1 y1 x2 y2 [color] [B|BF]`
    //   B  = box outline (4 lines)
    //   BF = box filled (FillRect)
    // 色はトークン中 `B`/`BF` 以外を吸収するため、色判定を先に行う。
    if xargs.len() >= 4 {
        // 座標は `(307 + i * 30)` のような変数入り算術式でありうるため
        // app-aware に評価する (生 parse だと式が 0 に潰れて線がずれる)。
        let x1 = c.long(app, 0).unwrap_or(0) as f64;
        let y1 = c.long(app, 1).unwrap_or(0) as f64;
        let x2 = c.long(app, 2).unwrap_or(0) as f64;
        let y2 = c.long(app, 3).unwrap_or(0) as f64;
        let mut color: Option<String> = None;
        let mut box_mode: Option<&'static str> = None;
        for tok in xargs.iter().skip(4) {
            let t = tok.trim();
            if t.eq_ignore_ascii_case("b") {
                box_mode = Some("B");
            } else if t.eq_ignore_ascii_case("bf") {
                box_mode = Some("BF");
            } else if !t.is_empty() {
                // 色トークン。`FrameColor1` のような変数指定 (.ini 由来の
                // テーマ色) も解決する。
                color = Some(resolve_color(app, t));
            }
        }
        if let Some(c) = color {
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::SetColor { color: c });
        }
        let (lx, ly, lw, lh) = (x1.min(x2), y1.min(y2), (x2 - x1).abs(), (y2 - y1).abs());
        match box_mode {
            Some("BF") => {
                app.script_overlay_mut()
                    .push(crate::script_overlay::DrawCmd::FillRect {
                        x: lx,
                        y: ly,
                        w: lw,
                        h: lh,
                    });
            }
            Some("B") => {
                // 4 辺
                for (px1, py1, px2, py2) in [
                    (lx, ly, lx + lw, ly),
                    (lx + lw, ly, lx + lw, ly + lh),
                    (lx + lw, ly + lh, lx, ly + lh),
                    (lx, ly + lh, lx, ly),
                ] {
                    app.script_overlay_mut()
                        .push(crate::script_overlay::DrawCmd::Line {
                            x1: px1,
                            y1: py1,
                            x2: px2,
                            y2: py2,
                        });
                }
            }
            _ => {
                app.script_overlay_mut()
                    .push(crate::script_overlay::DrawCmd::Line { x1, y1, x2, y2 });
            }
        }
    }
    Ok(pc + 1)
}

fn exec_pset(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    if xargs.len() >= 2 {
        // `PSet x, y` の SRC 構文は tokenizer 上で trailing `,` が
        // 付くため strip してから parse する。
        let x = c.long(app, 0).unwrap_or(0) as f64;
        let y = c.long(app, 1).unwrap_or(0) as f64;
        app.script_overlay_mut()
            .push(crate::script_overlay::DrawCmd::PSet { x, y });
    }
    Ok(pc + 1)
}

fn exec_circle(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Circle x y r [color]` — 中心 (x,y)・半径 r の真円 (SRC `Circleコマンド`)。
    // 色省略時は現在の線色。座標は変数入り算術式でありうるため app-aware に評価。
    if xargs.len() >= 3 {
        let cx = c.long(app, 0).unwrap_or(0) as f64;
        let cy = c.long(app, 1).unwrap_or(0) as f64;
        let r = c.long(app, 2).unwrap_or(0) as f64;
        if let Some(c) = optional_draw_color(app, &xargs[3..]) {
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::SetColor { color: c });
        }
        app.script_overlay_mut()
            .push(crate::script_overlay::DrawCmd::Circle { cx, cy, r });
    }
    Ok(pc + 1)
}

fn exec_oval(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Oval x y r ratio [color]` — 中心 (x,y)・横半径 r・縦横比 ratio の楕円。
    if xargs.len() >= 4 {
        let cx = c.long(app, 0).unwrap_or(0) as f64;
        let cy = c.long(app, 1).unwrap_or(0) as f64;
        let r = c.long(app, 2).unwrap_or(0) as f64;
        let ratio = c.double(app, 3).unwrap_or(1.0);
        if let Some(c) = optional_draw_color(app, &xargs[4..]) {
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::SetColor { color: c });
        }
        app.script_overlay_mut()
            .push(crate::script_overlay::DrawCmd::Oval { cx, cy, r, ratio });
    }
    Ok(pc + 1)
}

fn exec_arc(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Arc x y r start end [color]` — 中心 (x,y)・半径 r の円弧。
    // 角度は度数法・右向き=0・反時計回り増加 (SRC `Arcコマンド`)。
    if xargs.len() >= 5 {
        let cx = c.long(app, 0).unwrap_or(0) as f64;
        let cy = c.long(app, 1).unwrap_or(0) as f64;
        let r = c.long(app, 2).unwrap_or(0) as f64;
        let start_deg = c.double(app, 3).unwrap_or(0.0) % 360.0;
        let end_deg = c.double(app, 4).unwrap_or(0.0) % 360.0;
        if let Some(c) = optional_draw_color(app, &xargs[5..]) {
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::SetColor { color: c });
        }
        app.script_overlay_mut()
            .push(crate::script_overlay::DrawCmd::Arc {
                cx,
                cy,
                r,
                start_deg,
                end_deg,
            });
    }
    Ok(pc + 1)
}

fn exec_polygon(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Polygon x1 y1 x2 y2 …` — 頂点列を結ぶ多角形 (SRC `Polygonコマンド`)。
    // 色オプションは無く、現在の線色のみ。頂点は座標ペアで任意個数。
    // 余った単独座標は無視する (原典 `2 * pnum < ArgNum`)。
    let points: Vec<(f64, f64)> = (0..xargs.len() / 2)
        .map(|k| {
            let px = c.long(app, 2 * k).unwrap_or(0) as f64;
            let py = c.long(app, 2 * k + 1).unwrap_or(0) as f64;
            (px, py)
        })
        .collect();
    if points.len() >= 2 {
        app.script_overlay_mut()
            .push(crate::script_overlay::DrawCmd::Polygon { points });
    }
    Ok(pc + 1)
}

fn exec_fillcolor(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `FillColor <color>` — 図形の内部塗り色。線色 (Color) とは独立。
    // 旧実装は SetColor (線色) に潰していたが、塗り色は SetFillColor で別管理。
    let raw = xargs.join(" ");
    let raw = raw.trim();
    let color = if raw.is_empty() {
        "#000000".to_string()
    } else {
        resolve_color(app, raw)
    };
    app.script_overlay_mut()
        .push(crate::script_overlay::DrawCmd::SetFillColor { color });
    Ok(pc + 1)
}

fn exec_fadeout(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // `FadeOut n` — 画面が黒へフェードアウト (終状態: 黒で覆う)。
    // n は段階値。終状態のみ描くので黒の全画面 Fade を 1 枚積む。
    let n = c.long(app, 0).unwrap_or(0);
    let alpha = (n as f64 / 60.0).clamp(0.0, 1.0);
    app.script_overlay_mut()
        .push(crate::script_overlay::DrawCmd::Fade {
            color: "#000000".to_string(),
            alpha,
        });
    Ok(pc + 1)
}

fn exec_fadein(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // `FadeIn n` — 黒から通常画面へフェードイン。終状態は通常画面なので
    // 黒の全画面 Fade を除去して露出する (WhiteIn と同様、残すと黒画面で
    // 操作不能になる)。
    app.script_overlay_mut()
        .remove_fades_of(is_black_fade_color);
    Ok(pc + 1)
}
