//! 武器選択ウィンドウの表示用ビューモデル / Weapon select window.
//!
//! 攻撃側が目標を確定した後・解決前に、使用武器を選ぶ (オリジナル SRC 武器選択窓)。
//! `animate_battle`(UI モード) かつ「武器選択ウィンドウ」設定が ON のときに
//! [`crate::App::weapon_select_window_data`] で構築する。front (src-web) は
//! `attacker` / `defender` のタイル位置から戦闘 HUD を解決し、各武器を表に描く。
//!
//! クリック当たり判定は [`weapon_select_choice_at`] と描画ジオメトリを
//! 共有する。選択 (1-based) → `rows[choice-1]` の武器 (使用不可行は選べない)。

/// 武器選択ウィンドウ 1 件分の表示データ。`GameDatabase` 参照は持たない純データ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaponSelectWindowData {
    /// 攻撃側ユニットのタイル位置 (HUD 解決用)。
    pub attacker: (u32, u32),
    /// 防御側 (目標) ユニットのタイル位置 (HUD 解決用)。
    pub defender: (u32, u32),
    /// 武器行 (機体の武装順 = 選択 index 順)。
    pub rows: Vec<WeaponSelectRow>,
    /// 反撃武器選択 (反撃手段「反撃」選択後) なら true。タイトル表示の切替に使う。
    pub is_counter: bool,
}

/// 武器選択ウィンドウの 1 行 (1 武器)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaponSelectRow {
    /// 武器名。
    pub name: String,
    /// 攻撃力。
    pub power: i64,
    /// この目標への命中率 (% / 使用不可は 0)。
    pub hit_pct: i32,
    /// クリティカル率 (CT, %)。
    pub critical: i32,
    /// 弾 / EN 表記 (`"残3/5"` / `"EN10"` / `"-"`)。
    pub ammo: String,
    /// 武器の地形適応 (4 文字 / 空)。
    pub adaption: String,
    /// 分類 (武器 class の表示文字列)。
    pub class: String,
    /// この目標へ使用可能か (射程 / 資源 / 必要技能)。不可なら × 表示・選択不可。
    pub usable: bool,
}

// ── 武器選択ウィンドウのレイアウト ────────────────────────────────────────
// `render::draw_weapon_select_window` の描画ジオメトリと **完全に一致** させること。
// 中央寄せの明色ウィンドウ (横長の武器表)。

/// 武器選択ウィンドウ幅 (px)。
pub const WEAPON_WIN_W: f64 = 600.0;
/// 武器選択ウィンドウ高さ (px)。
pub const WEAPON_WIN_H: f64 = 262.0;
/// 武器選択ウィンドウ上端 Y (px)。
pub const WEAPON_WIN_Y: f64 = 176.0;
/// ウィンドウ上端からの武器行先頭 Y オフセット (px)。上にタイトル+2機HUD+表ヘッダ。
pub const WEAPON_ROW_TOP: f64 = 96.0;
/// 武器行 1 行の高さ (px)。
pub const WEAPON_ROW_H: f64 = 18.0;
/// ウィンドウ内側パディング (px)。
pub const WEAPON_PAD: f64 = 10.0;

/// 武器選択ウィンドウの左端 X (中央寄せ)。
pub fn weapon_win_x() -> f64 {
    (f64::from(crate::CANVAS_WIDTH) - WEAPON_WIN_W) / 2.0
}

/// 武器選択ウィンドウの武器行クリック判定。`(x, y)` が武器 `i` の行内なら
/// `Some(i+1)` (1-based)。行外 / ウィンドウ外は `None`。描画ジオメトリと共有。
pub fn weapon_select_choice_at(num_rows: usize, x: i32, y: i32) -> Option<u32> {
    let wx = weapon_win_x();
    let row_top = WEAPON_WIN_Y + WEAPON_ROW_TOP;
    let (fx, fy) = (f64::from(x), f64::from(y));
    if fx < wx + WEAPON_PAD || fx > wx + WEAPON_WIN_W - WEAPON_PAD {
        return None;
    }
    for i in 0..num_rows.min(9) {
        let top = row_top + (i as f64) * WEAPON_ROW_H;
        if fy >= top && fy < top + WEAPON_ROW_H {
            return Some((i + 1) as u32);
        }
    }
    None
}

impl crate::App {
    /// 武器選択中なら、ウィンドウ表示用データを構築する (front が描画)。
    pub fn weapon_select_window_data(
        &self,
    ) -> Option<crate::scene::weapon_select::WeaponSelectWindowData> {
        let pws = self.pending_weapon_select()?;
        let atk_idx = self.database().idx_by_uid(&pws.atk_uid)?;
        let atk_party = self.database().unit_instances[atk_idx].party;
        let def_idx = self.database().unit_instances.iter().position(|u| {
            !u.off_map && (u.x, u.y) == pws.target && u.party.is_hostile_to(atk_party)
        })?;
        let attacker = {
            let a = &self.database().unit_instances[atk_idx];
            (a.x, a.y)
        };
        let rows = self.build_weapon_rows(atk_idx, def_idx, pws.post_move);
        Some(WeaponSelectWindowData {
            attacker,
            defender: pws.target,
            rows,
            is_counter: pws.is_counter(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // 武器選択窓: WIN_Y=176, ROW_TOP=96 → row_top=272。各 18px:
    // [272,290) [290,308) [308,326)。中央寄せ wx=(640-600)/2=20, pad=10。
    #[test]
    fn weapon_select_choice_at_maps_rows() {
        let x = weapon_win_x() as i32 + 100; // 枠内
        assert_eq!(weapon_select_choice_at(3, x, 272), Some(1));
        assert_eq!(weapon_select_choice_at(3, x, 289), Some(1));
        assert_eq!(weapon_select_choice_at(3, x, 290), Some(2));
        assert_eq!(weapon_select_choice_at(3, x, 308), Some(3));
        assert_eq!(weapon_select_choice_at(3, x, 260), None);
        assert_eq!(weapon_select_choice_at(3, x, 330), None);
        assert_eq!(weapon_select_choice_at(3, 5, 272), None);
    }
}
