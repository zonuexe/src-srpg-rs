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

/// ステータス詳細画面のビューモデル組み立て (盤面の実効値を表示用に整形する)。
impl crate::App {
    /// `Scene::UnitDetail` で表示する単機ステータスのビューモデルを構築する。
    /// `roster_pos` は味方ロスター (`status_roster`) 内の 0 始まり位置。範囲外 /
    /// ロスター空なら `None`。改造段階・装備・レベル成長・状態異常込みの実効値を
    /// 反映する (`GameDatabase::effective_*` / `effective_pilot_data`)。
    pub fn build_status_detail(
        &self,
        roster_pos: usize,
    ) -> Option<crate::scene::unit_detail::StatusDetail> {
        let roster = self.status_roster();
        let total = roster.len();
        let &unit_idx = roster.get(roster_pos)?;
        let u = self.database().unit_instances.get(unit_idx)?;
        let unit_data = self.database().unit_by_name(&u.unit_data_name)?;

        let hp_max = self.database().effective_max_hp(u);
        let hp_cur = (hp_max - u.damage).max(0);
        let en_max = self.database().effective_max_en(u);
        let en_cur = (en_max - u.en_consumed).max(0);

        // 搭乗パイロット (メイン)。無人なら has_pilot=false。
        let main_pilot = u.main_pilot_name();
        let pilot_data = if main_pilot.is_empty() {
            None
        } else {
            self.database().effective_pilot_data(main_pilot)
        };
        let pilot_inst = u
            .pilot_ids
            .first()
            .and_then(|id| self.database().pilot_instance_by_id(id));

        // 武器一覧 (runtime 状態を反映: 残弾 / EN)。
        let weapons: Vec<WeaponRow> = u
            .weapons
            .iter()
            .filter_map(|uw| {
                let wd = unit_data.weapons.get(uw.weapon_index)?;
                let range = if wd.min_range == wd.max_range {
                    wd.max_range.to_string()
                } else {
                    format!("{}-{}", wd.min_range, wd.max_range)
                };
                let ammo = if wd.bullet > 0 {
                    format!("残{}/{}", uw.bullet_remaining.max(0), wd.bullet)
                } else if wd.en_consumption > 0 {
                    format!("EN{}", wd.en_consumption)
                } else {
                    "-".to_string()
                };
                // 必要技能 / 必要条件のゲートのみで使用可否を判定する (has_acted / EN / 残弾
                // 等の一時的状態は閲覧画面では除外し、技能による恒久的な解禁状況のみ示す)。
                let ns = wd.necessary_skill();
                let nc = wd.necessary_condition();
                let usable = (ns.is_empty()
                    || crate::necessary_skill::is_satisfied(ns, u, self.database()))
                    && (nc.is_empty()
                        || crate::necessary_skill::is_satisfied(nc, u, self.database()));
                Some(WeaponRow {
                    name: wd.name.clone(),
                    power: wd.power,
                    range,
                    ammo,
                    usable,
                })
            })
            .collect();

        let conditions: Vec<String> = u.conditions.iter().map(|c| c.name.clone()).collect();

        // パイロット項目。
        let (
            has_pilot,
            pilot_name,
            pilot_nickname,
            level,
            exp,
            sp_cur,
            sp_max,
            infight,
            shooting,
            hit,
            dodge,
            intuition,
            technique,
            pilot_adaption,
            spirit_commands,
            skills,
        ) = if let Some(pd) = pilot_data.as_ref() {
            let level = pilot_inst.map(|p| p.level).unwrap_or(1);
            let exp = pilot_inst.map(|p| p.total_exp).unwrap_or(u.total_exp);
            let sp_max = pd.sp.unwrap_or(0);
            let sp_cur = pilot_inst.map(|p| p.sp_remaining).unwrap_or(sp_max);
            let spirit_commands: Vec<String> = pd
                .spirit_commands
                .iter()
                .map(|sc| format!("{} Lv{}", sc.name, sc.level))
                .collect();
            // 特殊技能: runtime crate::PilotInstance.skills を優先し、無ければ静的特殊能力名。
            let skills: Vec<String> = if let Some(pi) = pilot_inst {
                if !pi.skills.is_empty() {
                    pi.skills.clone()
                } else {
                    pd.features.iter().map(|(n, _)| n.clone()).collect()
                }
            } else {
                pd.features.iter().map(|(n, _)| n.clone()).collect()
            };
            (
                true,
                pd.name.clone(),
                pd.nickname.clone(),
                level,
                exp,
                sp_cur,
                sp_max,
                pd.infight,
                pd.shooting,
                pd.hit,
                pd.dodge,
                pd.intuition,
                pd.technique,
                pd.adaption.as_str().to_string(),
                spirit_commands,
                skills,
            )
        } else {
            (
                false,
                String::new(),
                String::new(),
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                String::new(),
                Vec::new(),
                Vec::new(),
            )
        };

        Some(StatusDetail {
            index: roster_pos,
            total,
            unit_name: unit_data.name.clone(),
            unit_nickname: unit_data.nickname.clone(),
            party_label: u.party.short_label().to_string(),
            class: u
                .class_override
                .clone()
                .unwrap_or_else(|| unit_data.class.clone()),
            size: unit_data.size.label().to_string(),
            unit_adaption: unit_data.adaption.as_str().to_string(),
            hp_cur,
            hp_max,
            en_cur,
            en_max,
            armor: self.database().effective_armor(u),
            mobility: self.database().effective_mobility(u),
            speed: self.database().effective_speed(u),
            upgrade_level: u.upgrade_level,
            unit_morale: u.morale,
            conditions,
            has_pilot,
            pilot_name,
            pilot_nickname,
            level,
            exp,
            sp_cur,
            sp_max,
            infight,
            shooting,
            hit,
            dodge,
            intuition,
            technique,
            pilot_adaption,
            spirit_commands,
            skills,
            weapons,
        })
    }
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
