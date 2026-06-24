//! 武器選択ウィンドウの表示用ビューモデル / Weapon select window.
//!
//! 攻撃側が目標を確定した後・解決前に、使用武器を選ぶ (オリジナル SRC 武器選択窓)。
//! `animate_battle`(UI モード) かつ「武器選択ウィンドウ」設定が ON のときに
//! [`crate::App::weapon_select_window_data`] で構築する。front (src-web) は
//! `attacker` / `defender` のタイル位置から戦闘 HUD を解決し、各武器を表に描く。
//!
//! クリック当たり判定は [`crate::dialog::weapon_select_choice_at`] と描画ジオメトリを
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
