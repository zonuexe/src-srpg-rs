//! ネイティブ戦闘演出。SRC_BA の `.eve`（汎用戦闘アニメ ＝ `GBA_*.eve` 群）を
//! 解釈せず、エンジン側で攻撃 1 回の結果を短いオーバーレイ演出として可視化する。
//!
//! 状態だけを src-core が持ち、実描画はフロントエンド (src-web) が
//! [`crate::App::battle_anim`] を読んで Canvas に重ねて行う。テストでは
//! `set_animate_battle(true)` を呼ぶと攻撃解決時に [`BattleAnim`] が積まれる。
//!
//! SRC 原典の戦闘アニメ（`animation.txt` ＋ `戦闘アニメ_*` サブルーチン群を
//! `PaintPicture`/`Wait`/`Redraw` で再生する仕組み）の完全移植は別タスク。本
//! モジュールは「チップ再配置＋結果メッセージ」だけだった演出を、命中フラッシュ・
//! 着弾・ダメージ数字のポップアップで補強する最小実装である。`kind` を記録して
//! おき、後続でスプライトフレーム再生を載せられるようにしている。

use serde::{Deserialize, Serialize};

/// 攻撃演出の種別。武器属性（[`crate::data::unit::WeaponData::class`] や射程）から
/// 決定し、フロントエンドがフラッシュ色や（将来的に）スプライト選択に使う。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttackKind {
    /// 格闘・白兵（近接、射程 1）。
    Melee,
    /// 射撃・実弾（遠距離）。
    Shoot,
    /// ビーム・エネルギー兵器。
    Beam,
    /// 属性不明・その他。
    Generic,
}

impl AttackKind {
    /// 武器データから演出種別を推定する。
    /// - 属性 `B`（ビーム）を含む → [`AttackKind::Beam`]
    /// - 最大射程 1 以下 → [`AttackKind::Melee`]
    /// - それ以外（遠距離） → [`AttackKind::Shoot`]
    pub fn from_weapon(weapon: &crate::data::unit::WeaponData) -> Self {
        let class = weapon.class.as_str();
        if class.contains('B') || class.contains("ビーム") {
            AttackKind::Beam
        } else if weapon.max_range <= 1 {
            AttackKind::Melee
        } else {
            AttackKind::Shoot
        }
    }
}

/// 攻撃 1 回ぶんの演出。`attack_resolve_and_run` が攻撃を解決した直後に積み、
/// `tick` が `elapsed` を進める。`elapsed >= total` で `App` 側が破棄する。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BattleAnim {
    /// 経過秒。
    pub elapsed: f64,
    /// 全体長（秒）。`elapsed >= total` で終了。
    pub total: f64,
    /// 攻撃側タイル（演出の始点 ＝ lunge / 弾道の起点）。
    pub attacker: (u32, u32),
    /// 防御側タイル（演出の中心 ＝ 着弾・ダメージ表示位置）。
    pub defender: (u32, u32),
    /// 命中したか。
    pub hit: bool,
    /// 防御側が実際に受けたダメージ（回避/防御/援護防御を反映した実値）。
    /// ミス時・肩代わり時は 0。
    pub damage: i64,
    /// この攻撃で防御側が撃破されたか。
    pub killed: bool,
    /// 演出種別。
    pub kind: AttackKind,
}

impl BattleAnim {
    /// 命中可否から演出の長さ（秒）を決める。命中はやや長く、ミスは短く。
    pub fn duration_for(hit: bool) -> f64 {
        if hit {
            0.55
        } else {
            0.40
        }
    }

    /// 新規演出を生成する（`elapsed = 0`）。
    pub fn new(
        attacker: (u32, u32),
        defender: (u32, u32),
        hit: bool,
        damage: i64,
        killed: bool,
        kind: AttackKind,
    ) -> Self {
        BattleAnim {
            elapsed: 0.0,
            total: Self::duration_for(hit),
            attacker,
            defender,
            hit,
            damage,
            killed,
            kind,
        }
    }

    /// 0..1 に正規化した進捗。
    pub fn progress(&self) -> f64 {
        if self.total <= 0.0 {
            1.0
        } else {
            (self.elapsed / self.total).clamp(0.0, 1.0)
        }
    }

    /// 演出が完了したか。
    pub fn finished(&self) -> bool {
        self.elapsed >= self.total
    }
}

/// ユニット移動のスライド演出。AI 移動が瞬間再配置だったのを、経路に沿って
/// チップを滑らせる視覚演出にする。論理位置は移動先 (即時) のまま、表示位置だけを
/// `position()` で補間する。`tick` が `elapsed` を進め、完了で `App` 側が破棄する。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveAnim {
    /// 移動中のユニット uid。
    pub uid: String,
    /// 経路 (start..=dest のタイル列)。
    pub path: Vec<(u32, u32)>,
    /// 経過秒。
    pub elapsed: f64,
    /// 1 タイル進むのにかける秒。
    pub seg_secs: f64,
}

impl MoveAnim {
    /// 1 タイルあたりの既定スライド時間 (秒)。
    pub const DEFAULT_SEG_SECS: f64 = 0.12;

    /// 新規生成。
    pub fn new(uid: String, path: Vec<(u32, u32)>) -> Self {
        MoveAnim {
            uid,
            path,
            elapsed: 0.0,
            seg_secs: Self::DEFAULT_SEG_SECS,
        }
    }

    /// 全体長 (秒)。
    pub fn total(&self) -> f64 {
        self.path.len().saturating_sub(1) as f64 * self.seg_secs
    }

    /// 完了したか。
    pub fn finished(&self) -> bool {
        self.elapsed >= self.total()
    }

    /// 現在の補間タイル座標 (経路に沿った位置、タイル単位の浮動小数)。
    pub fn position(&self) -> (f64, f64) {
        let last = self.path.last().copied().unwrap_or((0, 0));
        if self.path.len() <= 1 || self.seg_secs <= 0.0 {
            return (last.0 as f64, last.1 as f64);
        }
        let seg = (self.elapsed / self.seg_secs).floor() as usize;
        if seg >= self.path.len() - 1 {
            return (last.0 as f64, last.1 as f64);
        }
        let t = (self.elapsed / self.seg_secs) - seg as f64;
        let a = self.path[seg];
        let b = self.path[seg + 1];
        (
            a.0 as f64 + (b.0 as f64 - a.0 as f64) * t,
            a.1 as f64 + (b.1 as f64 - a.1 as f64) * t,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::unit::WeaponData;

    fn weapon(class: &str, max_range: i32) -> WeaponData {
        WeaponData {
            name: "テスト武器".into(),
            power: 1000,
            min_range: 1,
            max_range,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: class.into(),
            extras: Vec::new(),
        }
    }

    #[test]
    fn kind_from_weapon_classifies_beam_melee_shoot() {
        assert_eq!(AttackKind::from_weapon(&weapon("B", 4)), AttackKind::Beam);
        assert_eq!(AttackKind::from_weapon(&weapon("", 1)), AttackKind::Melee);
        assert_eq!(AttackKind::from_weapon(&weapon("実", 5)), AttackKind::Shoot);
    }

    #[test]
    fn progress_and_finished_track_elapsed() {
        let mut a = BattleAnim::new((0, 0), (1, 0), true, 500, false, AttackKind::Melee);
        assert_eq!(a.progress(), 0.0);
        assert!(!a.finished());
        a.elapsed = a.total;
        assert_eq!(a.progress(), 1.0);
        assert!(a.finished());
    }

    #[test]
    fn move_anim_interpolates_along_path() {
        // 経路 (0,0)->(1,0)->(2,0)、1 タイル 0.1s。
        let mut m = MoveAnim::new("U1".into(), vec![(0, 0), (1, 0), (2, 0)]);
        m.seg_secs = 0.1;
        assert!((m.total() - 0.2).abs() < 1e-9);
        // 開始は始点。
        assert_eq!(m.position(), (0.0, 0.0));
        // 1 セグメントの中央 (0.05s) → (0.5, 0)。
        m.elapsed = 0.05;
        let (x, y) = m.position();
        assert!((x - 0.5).abs() < 1e-9 && y == 0.0);
        // 2 セグメント目途中 (0.15s) → (1.5, 0)。
        m.elapsed = 0.15;
        let (x, _) = m.position();
        assert!((x - 1.5).abs() < 1e-9);
        // 終了後は終点 (2,0) に張り付き。
        m.elapsed = 1.0;
        assert!(m.finished());
        assert_eq!(m.position(), (2.0, 0.0));
    }
}
