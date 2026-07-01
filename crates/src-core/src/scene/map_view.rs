//! マップ表示画面 / Map view.
//!
//! SRC 互換の 480×480 マップウィンドウに合わせる:
//! ```text
//! ┌────────────────────────┬──────────┐
//! │                        │          │
//! │  Map (15 × 15 タイル)  │  Status  │
//! │  = 480 × 480 px        │  Panel   │
//! │                        │  160px幅 │
//! │                        ├──────────┤
//! │                        │ Message  │
//! │                        │ Box      │
//! └────────────────────────┴──────────┘
//! ```
//!
//! - タイルは 32 px 固定 (元 SRC の標準サイズ)
//! - 15 × 15 = 480 × 480 px のビューポート
//! - 右パネルにユニット詳細（HP/EN/能力値/武器一覧）+ メッセージボックスを縦並び
//! - PaintString / Hotpoint の y=420 等 SRC 標準 480x480 想定座標が
//!   マップ領域内に収まる

pub const TILE_SIZE: u32 = 32;

/// ステータスバーは廃止し、ターン/ステージ情報は右パネル先頭に表示する。
/// 後方互換のため定数だけ残し、0 を割り当てる（render 側でこの値を加算しても
/// 影響しない）。
pub const STATUS_BAR_HEIGHT: u32 = 0;

/// ビューポートに収めるタイル数 (SRC 互換 480×480)。
pub const VIEW_TILES_X: u32 = 15;
pub const VIEW_TILES_Y: u32 = 15;

/// 右側のステータスパネル幅 (768 - 480 = 288)。オリジナル SRC のステータス窓幅に
/// 寄せ、武器名・能力名を切り詰めずに表示できる幅を確保する。
pub const STATUS_PANEL_WIDTH: u32 = 288;

/// 下部メッセージボックスの高さ (右パネル下部分を割く)。
pub const MESSAGE_BOX_HEIGHT: u32 = 144;
/// メッセージボックス内の顔グラ枠（左側）。
pub const PORTRAIT_SIZE: u32 = 64;

pub const MAP_VIEW_WIDTH: u32 = TILE_SIZE * VIEW_TILES_X + STATUS_PANEL_WIDTH;
pub const MAP_VIEW_HEIGHT: u32 = TILE_SIZE * VIEW_TILES_Y;

// 下部互換: 旧 INFO_BAR は MessageBox に統合済み。
pub const INFO_BAR_HEIGHT: u32 = MESSAGE_BOX_HEIGHT;

/// マップビュー左上を基準にしたタイル領域の矩形（pixel）。
pub const MAP_AREA_X: u32 = 0;
pub const MAP_AREA_Y: u32 = STATUS_BAR_HEIGHT;
pub const MAP_AREA_W: u32 = TILE_SIZE * VIEW_TILES_X;
pub const MAP_AREA_H: u32 = TILE_SIZE * VIEW_TILES_Y;

/// ステータスパネル領域 (右側全高)。旧実装は下部にメッセージボックスを割いていたが、
/// サイドバーのメッセージ表示は廃止し、右パネルは全高をステータスに使う。
pub const STATUS_PANEL_X: u32 = MAP_AREA_W;
pub const STATUS_PANEL_Y: u32 = STATUS_BAR_HEIGHT;
pub const STATUS_PANEL_H: u32 = MAP_AREA_H;

/// メッセージボックス領域 (廃止済み・後方互換のため定数のみ残す)。
pub const MESSAGE_BOX_X: u32 = MAP_AREA_W;
pub const MESSAGE_BOX_Y: u32 = STATUS_PANEL_Y + STATUS_PANEL_H;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_match_constants() {
        assert_eq!(MAP_VIEW_WIDTH, 15 * 32 + 288);
        assert_eq!(MAP_VIEW_HEIGHT, 15 * 32);
        assert_eq!(MAP_VIEW_WIDTH, 768);
        assert_eq!(MAP_VIEW_HEIGHT, 480);
        assert_eq!(MAP_AREA_W, 480);
        assert_eq!(MAP_AREA_H, 480);
        // キャンバス幅とマップビュー幅は一致 (ox=0 で中央寄せ)。
        assert_eq!(MAP_VIEW_WIDTH, crate::CANVAS_WIDTH);
    }
}

/// シーン座標 (MapView ローカル) からタイル座標を計算。マップ領域外なら `None`。
pub fn pixel_to_tile(scene_x: i32, scene_y: i32) -> Option<(u32, u32)> {
    let tile_size = i32::try_from(TILE_SIZE).unwrap_or(1);
    let area_w = i32::try_from(MAP_AREA_W).unwrap_or(1);
    let area_h = i32::try_from(MAP_AREA_H).unwrap_or(1);

    if scene_x < 0 || scene_y < 0 || scene_x >= area_w || scene_y >= area_h {
        return None;
    }
    let tx = scene_x / tile_size;
    let ty = scene_y / tile_size;
    if tx < 0 || ty < 0 {
        return None;
    }
    let (tx, ty) = (tx as u32, ty as u32);
    if tx >= VIEW_TILES_X || ty >= VIEW_TILES_Y {
        return None;
    }
    Some((tx, ty))
}

#[cfg(test)]
mod px_tests {
    use super::*;

    #[test]
    fn pixel_to_tile_corners() {
        assert_eq!(pixel_to_tile(0, 0), Some((0, 0)));
        assert_eq!(
            pixel_to_tile(
                (TILE_SIZE * VIEW_TILES_X - 1) as i32,
                (TILE_SIZE * VIEW_TILES_Y - 1) as i32
            ),
            Some((VIEW_TILES_X - 1, VIEW_TILES_Y - 1))
        );
    }

    #[test]
    fn pixel_to_tile_outside_returns_none() {
        // 右パネル領域
        assert_eq!(pixel_to_tile((MAP_AREA_W + 1) as i32, 10), None);
        // 下メッセージ領域
        assert_eq!(pixel_to_tile(10, (MAP_AREA_H + 1) as i32), None);
        // マイナス
        assert_eq!(pixel_to_tile(-1, -1), None);
    }
}
