//! 対話オーバーレイ (`PendingDialog::Menu` 等) のクリック当たり判定。描画は src-web
//! `render::draw_dialog_overlay` と同じジオメトリを使う。

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
pub fn menu_choice_at(prompt: &str, num_options: usize, x: i32, y: i32) -> Option<u32> {
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
