//! 単機ステータス詳細画面 / Single-unit status detail screen.
//!
//! インターミッション「ステータス」から開く、味方ロスター 1 機ぶんの
//! 詳細ステータス画面。元 SRC の `パイロットステータス` / `ユニットステータス`
//! コマンド (`パイロットステータス.md` / `ユニットステータス.md`) に相当する。
//!
//! 従来の `PilotList` / `UnitList` は **静的データ** (`GameDatabase::pilots`/`units`)
//! を一覧する MVP だったが、本画面は **実体** (`UnitInstance` + 搭乗パイロット) の
//! 実効値 (改造段階・装備・レベル成長・状態異常込み) を 1 機単位で表示し、
//! `◀ / ▶` でロスターを巡回する。
//!
//! ロジック層 (本モジュール) はレイアウト定数と表示用ビューモデル
//! [`StatusDetail`] のみを持ち、ビューモデルの構築は `App::build_status_detail`
//! (app.rs、`GameDatabase` の実効値ヘルパを使うため) が担う。描画は
//! `src-web::render::draw_unit_detail` がレイアウトする。

use super::title::Rect;

// 全画面モーダル。キャンバス幅に合わせ ox=0 を保つ (描画とクリック判定の中央寄せ
// オフセットずれを避ける)。内容レイアウトは 640 幅基準で右側に余白が出るが破綻しない。
pub const UNIT_DETAIL_WIDTH: u32 = crate::CANVAS_WIDTH;
pub const UNIT_DETAIL_HEIGHT: u32 = crate::CANVAS_HEIGHT;

/// フッタのナビゲーションボタン。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailButton {
    /// 前の機体へ (◀)。
    Prev,
    /// 次の機体へ (▶)。
    Next,
    /// インターミッションへ戻る。
    Close,
}

/// 「前へ」ボタンの矩形。
pub const fn prev_button() -> Rect {
    Rect::new(16, 444, 96, 24)
}

/// 「次へ」ボタンの矩形。
pub const fn next_button() -> Rect {
    Rect::new(120, 444, 96, 24)
}

/// 「閉じる」ボタンの矩形。
pub const fn close_button() -> Rect {
    Rect::new(528, 444, 96, 24)
}

fn rect_contains(r: Rect, x: i32, y: i32) -> bool {
    x >= r.x && x < r.x + r.w as i32 && y >= r.y && y < r.y + r.h as i32
}

/// シーンローカル座標 `(x, y)` がどのボタンに当たるか。
pub fn hit_button(x: i32, y: i32) -> Option<DetailButton> {
    if rect_contains(prev_button(), x, y) {
        Some(DetailButton::Prev)
    } else if rect_contains(next_button(), x, y) {
        Some(DetailButton::Next)
    } else if rect_contains(close_button(), x, y) {
        Some(DetailButton::Close)
    } else {
        None
    }
}

/// 1 武器ぶんの表示行。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WeaponRow {
    /// 武器名。
    pub name: String,
    /// 攻撃力 (`WeaponData.power`)。
    pub power: i64,
    /// 射程表記 (`"1"` / `"1-4"`)。
    pub range: String,
    /// 弾数 / EN 表記 (`"残3/5"` / `"EN10"` / `"-"`)。
    pub ammo: String,
    /// 必要技能 / 必要条件を満たし使用可能か。`false` なら技能不足でグレー表示する。
    pub usable: bool,
}

/// 単機詳細画面の表示用ビューモデル。表示文字列と数値のみを持つ純データで、
/// `GameDatabase` への参照は持たない (構築は `App::build_status_detail`)。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusDetail {
    /// ロスター内の 0 始まり位置。
    pub index: usize,
    /// ロスター総数。
    pub total: usize,

    // --- 機体 ---
    pub unit_name: String,
    pub unit_nickname: String,
    pub party_label: String,
    pub class: String,
    pub size: String,
    pub unit_adaption: String,
    pub hp_cur: i64,
    pub hp_max: i64,
    pub en_cur: i32,
    pub en_max: i32,
    pub armor: i64,
    pub mobility: i32,
    pub speed: i32,
    pub upgrade_level: i32,
    pub unit_morale: i32,
    /// 状態異常 / バフ名。空ならクリーン。
    pub conditions: Vec<String>,

    // --- パイロット ---
    /// 搭乗パイロットが居るか (無人なら以降のパイロット項目は表示しない)。
    pub has_pilot: bool,
    pub pilot_name: String,
    pub pilot_nickname: String,
    pub level: i32,
    pub exp: i32,
    pub sp_cur: i32,
    pub sp_max: i32,
    pub infight: i32,
    pub shooting: i32,
    pub hit: i32,
    pub dodge: i32,
    pub intuition: i32,
    pub technique: i32,
    pub pilot_adaption: String,
    /// 精神コマンド表記 (`"ひらめき Lv1"` 等)。
    pub spirit_commands: Vec<String>,
    /// 特殊技能 / 特殊能力名。
    pub skills: Vec<String>,

    // --- 武器 ---
    pub weapons: Vec<WeaponRow>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buttons_fit_inside_form() {
        for r in [prev_button(), next_button(), close_button()] {
            assert!(r.x >= 0);
            assert!(r.y >= 0);
            assert!((r.x as u32) + r.w <= UNIT_DETAIL_WIDTH);
            assert!((r.y as u32) + r.h <= UNIT_DETAIL_HEIGHT);
        }
    }

    #[test]
    fn buttons_do_not_overlap() {
        let p = prev_button();
        let n = next_button();
        let c = close_button();
        // prev と next は横並びで重ならない。
        assert!(n.x >= p.x + p.w as i32);
        // close は next より右。
        assert!(c.x >= n.x + n.w as i32);
    }

    #[test]
    fn hit_button_resolves_each_button() {
        let p = prev_button();
        assert_eq!(hit_button(p.x + 2, p.y + 2), Some(DetailButton::Prev));
        let n = next_button();
        assert_eq!(hit_button(n.x + 2, n.y + 2), Some(DetailButton::Next));
        let c = close_button();
        assert_eq!(hit_button(c.x + 2, c.y + 2), Some(DetailButton::Close));
        // ボタン外。
        assert_eq!(hit_button(0, 0), None);
        assert_eq!(hit_button(300, 100), None);
    }
}
