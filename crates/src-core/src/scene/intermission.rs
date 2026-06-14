//! インターミッション画面のロジック層 / Intermission screen logic layer.
//!
//! 元実装: `SRC.Sharp/SRCCore/Intermissions/Intermission.cs` の
//! `InterMissionCommand()`。VB6 原典では `frmIntermission` フォームに相当。
//!
//! 戦闘外で表示され、シナリオが `IntermissionCommand <name> <file>` で
//! 登録したメニュー項目 (キャラクターメイキング / 改造 / ショップ / ...) と、
//! 「次のステージへ」(`次ステージ` システム変数経由) を一覧表示する。
//!
//! 移植版は SRC.Sharp の組み込みコマンド (機体改造 / 乗り換え / 乗り換え / ...)
//! は未実装で、ユーザ定義の `IntermissionCommand` 項目と「次のステージへ」のみ。

use super::title::Rect;

pub const INTERMISSION_WIDTH: u32 = 640;
pub const INTERMISSION_HEIGHT: u32 = 480;

/// 各メニュー項目の表示矩形の縦サイズ。
pub const ITEM_HEIGHT: u32 = 32;
/// メニュー項目間のギャップ。
pub const ITEM_GAP: u32 = 4;
/// メニューリスト左マージン。
pub const LIST_LEFT: i32 = 80;
/// メニュー先頭の Y。
pub const LIST_TOP: i32 = 84;
/// メニュー幅。
pub const LIST_WIDTH: u32 = 480;

/// シーン全体のレイアウト定数を返す。
pub fn item_rect(index: usize) -> Rect {
    let y = LIST_TOP + (ITEM_HEIGHT + ITEM_GAP) as i32 * index as i32;
    Rect::new(LIST_LEFT, y, LIST_WIDTH, ITEM_HEIGHT)
}

/// クリック座標が n 番目の項目に当たれば `Some(n)` を返す。
/// `item_count` は表示中の合計項目数 (ユーザ定義 + 末尾の「次のステージへ」)。
pub fn hit_item(x: i32, y: i32, item_count: usize) -> Option<usize> {
    for i in 0..item_count {
        let r = item_rect(i);
        if x >= r.x && x < r.x + r.w as i32 && y >= r.y && y < r.y + r.h as i32 {
            return Some(i);
        }
    }
    None
}

/// 「次のステージへ」項目のラベル。SRC.Sharp と同じ。
pub const NEXT_STAGE_LABEL: &str = "次のステージへ";

/// タイトル文字列。SRC.Sharp で `SRC.Stage = "インターミッション"` に相当。
pub const TITLE_LABEL: &str = "インターミッション";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_rects_dont_overlap() {
        let r0 = item_rect(0);
        let r1 = item_rect(1);
        assert!(r1.y >= r0.y + r0.h as i32, "items should not overlap");
    }

    #[test]
    fn hit_item_returns_correct_index() {
        // 0 番目の項目の中心 (LIST_LEFT + 1, LIST_TOP + 1) は 0 を返す。
        assert_eq!(hit_item(LIST_LEFT + 1, LIST_TOP + 1, 3), Some(0));
        // 1 番目項目の中央
        let r1 = item_rect(1);
        assert_eq!(hit_item(r1.x + 5, r1.y + 5, 3), Some(1));
        // リスト範囲外
        assert_eq!(hit_item(0, 0, 3), None);
        // 領域外 (count を超える index)
        assert_eq!(hit_item(LIST_LEFT + 1, LIST_TOP + 1, 0), None);
    }

    #[test]
    fn layout_fits_in_form_bounds() {
        let r = item_rect(0);
        assert!(r.x >= 0);
        assert!(r.y >= 0);
        assert!((r.x as u32) + r.w <= INTERMISSION_WIDTH);
        // 最大 8 項目想定 (sparobo は 8 個登録)
        let last = item_rect(7);
        assert!((last.y as u32) + last.h <= INTERMISSION_HEIGHT);
    }
}
