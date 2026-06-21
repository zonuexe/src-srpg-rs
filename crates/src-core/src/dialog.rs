//! ペンディング対話 UI / Pending dialog presented to the player.
//!
//! 元 SRC では `Talk` / `Confirm` / `Input` 等の対話命令は
//! `Event.bas::HandleEvent` 内で実行スレッドをモーダル待ちにする。
//! Web 移植では非同期化を避けるため、対話命令にぶつかった時点で
//! インタプリタを「サスペンド状態」にし、ユーザ入力後に
//! `event_runtime::resume` で再開する。
//!
//! `App.pending_dialog` がこの enum を持っている間は MapView 上に
//! オーバーレイを表示し、Enter/Y/N キーなどで応答する。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingDialog {
    /// `Talk speaker` … `End` ブロック。ナレーションは speaker 空。
    Talk { speaker: String, body: String },
    /// `Wait Click/Press/Key/Input` で Hotpoint が無いケース。
    /// 原典 SRC.NET (`CmdData.cs::ExecWaitCmd "click"`) は何も描画せず
    /// クリック/キー押下まで spin-wait するだけ。直前の `PaintString` /
    /// `PaintPicture` 描画がそのまま画面に残るのが本来の挙動。
    /// レンダラは本変種を **何も描かない** で、ClickAt/Enter を `respond_dialog(0)`
    /// で resume 再開する仕組みだけ流用する。
    WaitClick,
    /// `Confirm 質問文` → Yes/No 応答。答えを `var_name` に "0"/"1" で格納。
    Confirm { question: String, var_name: String },
    /// `Menu 質問文` … 各行が選択肢 … `End`。
    /// 1-indexed の選択番号 (default) または対応する `options[i-1]` の
    /// 文字列値 (`store_value = true`) を `var_name` に格納。
    /// 0 (キャンセル/Esc) は常に "0" を格納。
    Menu {
        prompt: String,
        options: Vec<String>,
        var_name: String,
        /// true なら選択された option の文字列値を var に書き込む。
        /// 元 SRC の `Hotpoint` 経由の選択（クリックされた hotpoint の name を
        /// 変数に格納）と一致させるためのモード。
        #[serde(default)]
        store_value: bool,
        /// SRC `Ask` Format 2 用。`options[i]` に対応する配列の添字 (subscript)。
        /// 非空なら `respond_dialog` は文字列値ではなく **添字** を var に格納する。
        /// 元 SRC の `Ask 配列変数 ...` は選んだ要素の添字を `選択` に返す仕様。
        #[serde(default)]
        option_keys: Vec<String>,
        /// `true` ならキャンセル (Esc / 右クリック / choice 0) を受け付けない。
        /// `キャンセル可` オプションの無い `Ask` は SRC では選択必須で、キャンセル
        /// すると `選択 = 0` になり「キャラ未選択 → 味方0体 → 即敗北」を招くため、
        /// 必須選択を強制する。`#[serde(default)]` = `false` (従来どおりキャンセル可)。
        #[serde(default)]
        non_cancellable: bool,
    },
    /// `Input var prompt default` — テキスト入力モーダル。
    /// ユーザ入力（Enter で確定）を `var_name` に格納。Esc キャンセル時は
    /// `default` を保持。
    Input {
        prompt: String,
        var_name: String,
        default: String,
    },
}

impl PendingDialog {
    /// 簡易ラベル（デバッグ表示用）。
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Talk { .. } => "Talk",
            Self::WaitClick => "WaitClick",
            Self::Confirm { .. } => "Confirm",
            Self::Menu { .. } => "Menu",
            Self::Input { .. } => "Input",
        }
    }
}

/// プレーンな `Menu`(Ask) 選択肢のクリック当たり判定。canvas ピクセル座標
/// `(x, y)` がどの選択肢行に当たるかを **1-indexed** で返す (枠外/選択肢外は
/// `None`)。
///
/// **重要**: src-web `render::draw_dialog_overlay` の `D::Menu` 描画ジオメトリと
/// 一致させること (両者がずれるとクリック位置と選択肢が食い違う):
/// - ウィンドウ上端 `win_top = 高さ * 0.55`、下 6px マージン。
/// - 内側パディング 12px。prompt は最大 2 行、各 24px、その後 +4px。
/// - 選択肢は 1 行 20px、最大 9 件。下端 (`win_top + win_h - 24`) を越えたら打切り。
///
/// プレーン Ask は元来「クリック = Advance(0) = キャンセル」で、選択肢を
/// クリックしても `選択 = 0` になり選べなかった (東方夢想伝: キャラ選択が 0 →
/// 味方 0 体で即敗北)。本判定で行クリック確定を可能にする。
pub(crate) fn menu_choice_at(prompt: &str, num_options: usize, x: i32, y: i32) -> Option<u32> {
    let cw = f64::from(crate::CANVAS_WIDTH);
    let ch = f64::from(crate::CANVAS_HEIGHT);
    let win_top = ch * 0.55;
    let win_h = ch - win_top - 6.0;
    let pad = 12.0;
    let (fx, fy) = (f64::from(x), f64::from(y));
    if fx < 6.0 || fx > cw - 6.0 || fy < win_top || fy > win_top + win_h {
        return None;
    }
    let prompt_lines = wrapped_line_count(prompt, 36).min(2);
    let mut oy = win_top + pad + (prompt_lines as f64) * 24.0 + 4.0;
    for i in 0..num_options.min(9) {
        if fy >= oy && fy < oy + 20.0 {
            return Some((i + 1) as u32);
        }
        oy += 20.0;
        if oy > win_top + win_h - 24.0 {
            break;
        }
    }
    None
}

// ── 反撃ウィンドウのレイアウト ────────────────────────────────────────────
// `render::draw_reaction_window` の描画ジオメトリと **完全に一致** させること
// (両者がずれると選択肢とクリック位置が食い違う)。中央寄せの明色ウィンドウ。

/// 反撃ウィンドウ幅 (px)。
pub const REACTION_WIN_W: f64 = 470.0;
/// 反撃ウィンドウ高さ (px)。
pub const REACTION_WIN_H: f64 = 236.0;
/// 反撃ウィンドウ上端 Y (px)。下寄りに置きマップ上の戦闘を隠しすぎない。
pub const REACTION_WIN_Y: f64 = 224.0;
/// ウィンドウ上端からの選択肢先頭 Y オフセット (px)。上にタイトル + 2 機ヘッダ。
pub const REACTION_OPT_TOP: f64 = 98.0;
/// 選択肢 1 行の高さ (px)。
pub const REACTION_OPT_H: f64 = 22.0;
/// ウィンドウ内側パディング (px)。
pub const REACTION_PAD: f64 = 10.0;

/// 反撃ウィンドウの左端 X (中央寄せ)。
pub fn reaction_win_x() -> f64 {
    (f64::from(crate::CANVAS_WIDTH) - REACTION_WIN_W) / 2.0
}

/// 反撃ウィンドウの選択肢行クリック判定。`(x, y)` が選択肢 `i` の行内なら
/// `Some(i+1)` (1-based)。行外 / ウィンドウ外は `None`。描画ジオメトリと共有。
pub fn reaction_choice_at(num_options: usize, x: i32, y: i32) -> Option<u32> {
    let wx = reaction_win_x();
    let opt_top = REACTION_WIN_Y + REACTION_OPT_TOP;
    let (fx, fy) = (f64::from(x), f64::from(y));
    if fx < wx + REACTION_PAD || fx > wx + REACTION_WIN_W - REACTION_PAD {
        return None;
    }
    for i in 0..num_options.min(6) {
        let top = opt_top + (i as f64) * REACTION_OPT_H;
        if fy >= top && fy < top + REACTION_OPT_H {
            return Some((i + 1) as u32);
        }
    }
    None
}

/// src-web `render::wrap_text` と同じ折返し規則 (全角=2幅 / ascii=1幅 / 累積幅が
/// `max_width` 以上で改行 / `\n` で改行) での行数。Menu prompt の表示行数算出用。
fn wrapped_line_count(s: &str, max_width: usize) -> usize {
    let mut lines = 0;
    let mut n = 0;
    let mut pending = false; // 未確定の行内容があるか
    for ch in s.chars() {
        if ch == '\n' {
            lines += 1; // wrap_text は空行でも push する
            n = 0;
            pending = false;
            continue;
        }
        pending = true;
        n += if ch.is_ascii() { 1 } else { 2 };
        if n >= max_width {
            lines += 1;
            n = 0;
            pending = false;
        }
    }
    if pending {
        lines += 1;
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    // CANVAS 480 高: win_top=264, win_h=210, pad=12。1 行 prompt → 選択肢開始
    // oy = 264 + 12 + 24 + 4 = 304。各 20px: [304,324) [324,344) [344,364)。
    #[test]
    fn menu_choice_at_maps_rows_to_1indexed_choices() {
        let opts = 3;
        // 枠内 x。
        assert_eq!(menu_choice_at("どの難易度？", opts, 100, 310), Some(1));
        assert_eq!(menu_choice_at("どの難易度？", opts, 100, 330), Some(2));
        assert_eq!(menu_choice_at("どの難易度？", opts, 100, 350), Some(3));
    }

    // 反撃ウィンドウ: WIN_Y=224, OPT_TOP=98 → opt_top=322。各 22px:
    // [322,344) [344,366) [366,388)。中央寄せ wx=(640-470)/2=85, pad=10。
    #[test]
    fn reaction_choice_at_maps_option_rows() {
        let x = reaction_win_x() as i32 + 50; // 枠内
        assert_eq!(reaction_choice_at(3, x, 322), Some(1));
        assert_eq!(reaction_choice_at(3, x, 343), Some(1));
        assert_eq!(reaction_choice_at(3, x, 344), Some(2));
        assert_eq!(reaction_choice_at(3, x, 366), Some(3));
        // 選択肢の上 / 件数を越えた下 / 横枠外。
        assert_eq!(reaction_choice_at(3, x, 300), None);
        assert_eq!(reaction_choice_at(3, x, 390), None);
        assert_eq!(reaction_choice_at(3, 10, 322), None);
    }

    #[test]
    fn menu_choice_at_misses_return_none() {
        // prompt 行や選択肢の上 (y < 304)。
        assert_eq!(menu_choice_at("どの難易度？", 3, 100, 270), None);
        // 選択肢 3 件の下 (y >= 364)。
        assert_eq!(menu_choice_at("どの難易度？", 3, 100, 400), None);
        // ウィンドウ枠外 (上端 264 より上)。
        assert_eq!(menu_choice_at("どの難易度？", 3, 100, 100), None);
        // 横枠外。
        assert_eq!(menu_choice_at("どの難易度？", 3, 2, 310), None);
    }

    #[test]
    fn wrapped_line_count_matches_widths() {
        assert_eq!(wrapped_line_count("", 36), 0);
        assert_eq!(wrapped_line_count("短い", 36), 1);
        // 全角 18 文字 = 36 幅 → 1 行で確定 (>=36 で改行)。
        assert_eq!(wrapped_line_count(&"あ".repeat(18), 36), 1);
        // 全角 19 文字 = 38 幅 → 2 行。
        assert_eq!(wrapped_line_count(&"あ".repeat(19), 36), 2);
        assert_eq!(wrapped_line_count("a\nb", 36), 2);
    }
}
