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

/// 陣営ごとのユニット枠・塗りの色 (CSS カラー)。
pub const fn party_color(party: crate::Party) -> &'static str {
    match party {
        crate::Party::Player => "#1e88e5",
        // ＮＰＣ はプレイヤー側陣営。緑系で味方と区別。
        crate::Party::Npc => "#43a047",
        crate::Party::Enemy => "#e53935",
        crate::Party::Neutral => "#fdd835",
    }
}

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

/// マップカーソル上のユニットについて、ステータスパネルが表示する判定結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusPanelView {
    /// 武器ごとの現在使用可否 (ユニットデータの武装順)。必要技能・気力・EN・残弾。
    pub weapon_firable: Vec<bool>,
    /// 末尾の戦闘予測行。攻撃できる敵対ユニットが居なければ `None`。
    pub forecast: Option<PanelForecast>,
}

/// ステータスパネル末尾の戦闘予測行 (`▶相手 命中x% 与y`) の表示データ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PanelForecast {
    /// 予測相手の愛称。
    pub target: String,
    /// 命中率 (%)。
    pub hit_pct: i32,
    /// 与ダメージ。
    pub damage: i64,
}

impl crate::App {
    /// マップカーソル上のユニットのステータスパネル用判定 (武器の使用可否と戦闘予測)。
    /// カーソル上にユニットが居なければ `None`。
    pub fn status_panel_view(&self) -> Option<StatusPanelView> {
        let (cx, cy) = self.map_cursor()?;
        let db = self.database();
        let idx = db
            .unit_instances
            .iter()
            .position(|u| !u.off_map && (u.x, u.y) == (cx, cy))?;
        let weapon_firable = db
            .unit_by_name(&db.unit_instances[idx].unit_data_name)
            .map(|d| {
                d.weapons
                    .iter()
                    .map(|w| self.weapon_firable(idx, w))
                    .collect()
            })
            .unwrap_or_default();
        Some(StatusPanelView {
            weapon_firable,
            forecast: self.panel_forecast(idx),
        })
    }

    /// `atk_idx` が、いまの位置から攻撃できる最も近い敵対ユニットへの戦闘予測 (同距離なら
    /// 威力の高い武器)。武器の選び方と予測は実際の攻撃と同じ
    /// (`pick_attack_weapon` / `forecast_strike`)。
    fn panel_forecast(&self, atk_idx: usize) -> Option<PanelForecast> {
        let db = self.database();
        let (_, atk_unit) = db.effective_combat_data(atk_idx)?;
        let a = &db.unit_instances[atk_idx];
        let mut best: Option<(u32, usize, crate::data::unit::WeaponData)> = None;
        for (def_idx, d) in db.unit_instances.iter().enumerate() {
            if d.off_map || !d.party.is_hostile_to(a.party) {
                continue;
            }
            let dist = crate::combat::manhattan((a.x, a.y), (d.x, d.y));
            let Some(w) = Self::pick_attack_weapon(
                &atk_unit,
                dist,
                None,
                a.charged,
                false,
                a.has_condition("突撃"),
                |w| self.weapon_firable(atk_idx, w),
                |w| self.weapon_skill_ok(atk_idx, w),
            ) else {
                continue;
            };
            let take = match &best {
                None => true,
                Some((bd, _, bw)) => dist < *bd || (dist == *bd && w.power > bw.power),
            };
            if take {
                best = Some((dist, def_idx, w));
            }
        }
        let (_, def_idx, weapon) = best?;
        let p = self.forecast_strike(atk_idx, def_idx, &weapon)?;
        let d = &db.unit_instances[def_idx];
        let target = db
            .pilot_by_name(&d.pilot_name)
            .map_or_else(|| d.pilot_name.clone(), |p| p.nickname.clone());
        Some(PanelForecast {
            target,
            hit_pct: p.hit_chance.clamp(0, 100),
            damage: p.damage,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn party_color_is_distinct_per_party() {
        let colors = [
            party_color(crate::Party::Player),
            party_color(crate::Party::Npc),
            party_color(crate::Party::Enemy),
            party_color(crate::Party::Neutral),
        ];
        for (i, a) in colors.iter().enumerate() {
            for (j, b) in colors.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
            }
        }
    }

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

/// マップ攻撃の目標地点選択中に塗るマス (`App::map_attack_select_tiles`)。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MapAttackTiles {
    /// 目標地点に選べるマス。
    pub targets: Vec<(u32, u32)>,
    /// 仮決めした目標地点 (無ければカーソル位置が選べるマスならそこ) を狙ったときの効果範囲。
    pub area: Vec<(u32, u32)>,
}
