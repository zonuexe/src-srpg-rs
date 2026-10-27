//! 反撃手段選択ウィンドウの表示用ビューモデル / Reaction (counter/defend/evade) window.
//!
//! 敵フェイズで味方が攻撃され「自動反撃モード」がオフのとき、`App` は
//! [`crate::App::reaction_window_data`] で本データを構築する。front (src-web) は
//! `attacker` / `defender` のタイル位置から戦闘 HUD (顔・Lv・気力・HP/EN) を解決し、
//! 各選択肢に命中率を添えてオリジナル SRC 風の戦闘窓として描画する。
//!
//! クリック当たり判定は [`reaction_choice_at`] と描画ジオメトリを
//! 共有する (両者がずれると選択肢とクリック位置が食い違う)。

/// 反撃ウィンドウ 1 件分の表示データ。`GameDatabase` 参照は持たない純データ。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionWindowData {
    /// 攻撃側ユニットのタイル位置 (HUD 解決用)。
    pub attacker: (u32, u32),
    /// 防御側 (味方) ユニットのタイル位置 (HUD 解決用)。
    pub defender: (u32, u32),
    /// 攻撃側が使う武器名 (タイトル "反撃：<武器> …")。
    pub weapon: String,
    /// 攻撃側の攻撃力 (タイトル "… 攻撃力=<power>")。
    pub power: i64,
    /// 攻撃側の基準命中率 (% / タイトル "… 命中率=<base_hit>%")。
    pub base_hit: i32,
    /// 各選択肢 (反撃 / 回避 / 防御 / 援護防御) と、その防御モードでの攻撃側命中率。
    pub options: Vec<ReactionOption>,
}

/// 反撃ウィンドウの 1 選択肢。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReactionOption {
    /// 選択肢ラベル (反撃 / 回避 / 防御 / 援護防御)。
    pub label: String,
    /// この防御モードを選んだ場合の攻撃側命中率 (%)。回避は半減。
    pub hit_pct: i32,
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

impl crate::App {
    /// 反撃手段選択中なら、ウィンドウ表示用データを構築する (front が描画)。
    /// `pending_reaction` が無い / 攻防ユニットを解決できない場合は `None`。
    pub fn reaction_window_data(&self) -> Option<crate::scene::reaction::ReactionWindowData> {
        let pr = self.pending_reaction()?;
        let atk_idx = self.database().idx_by_uid(&pr.atk_uid)?;
        let def_idx = self
            .database()
            .unit_instances
            .iter()
            .position(|u| !u.off_map && (u.x, u.y) == pr.target_tile)?;
        let (weapon, power, base_hit) = self.attack_hit_forecast(atk_idx, def_idx)?;
        let attacker = {
            let a = &self.database().unit_instances[atk_idx];
            (a.x, a.y)
        };
        let options = pr
            .modes
            .iter()
            .map(|m| {
                // 回避は命中半減 (SRC 反撃モード / attack_resolve_and_run の def_mode="回避")。
                let hit = if m == "回避" {
                    (base_hit / 2).max(0)
                } else {
                    base_hit
                };
                ReactionOption {
                    label: m.clone(),
                    hit_pct: hit,
                }
            })
            .collect();
        Some(ReactionWindowData {
            attacker,
            defender: pr.target_tile,
            weapon,
            power,
            base_hit,
            options,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
