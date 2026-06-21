//! 反撃手段選択ウィンドウの表示用ビューモデル / Reaction (counter/defend/evade) window.
//!
//! 敵フェイズで味方が攻撃され「自動反撃モード」がオフのとき、`App` は
//! [`crate::App::reaction_window_data`] で本データを構築する。front (src-web) は
//! `attacker` / `defender` のタイル位置から戦闘 HUD (顔・Lv・気力・HP/EN) を解決し、
//! 各選択肢に命中率を添えてオリジナル SRC 風の戦闘窓として描画する。
//!
//! クリック当たり判定は [`crate::dialog::reaction_choice_at`] と描画ジオメトリを
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
