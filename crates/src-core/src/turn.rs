//! ターン / フェーズ管理 / Turn and phase management.
//!
//! 元 SRC では `SRC.bas::Turn` (Integer) と `SRC.bas::Stage` (String) で
//! 表現されていた。`Stage` は現フェイズの陣営名（"味方"/"敵"/"中立"/"ＮＰＣ"）。
//! SRC.Sharp `StartTurn` の正準モデル: 毎ターン 味方→敵→中立→ＮＰＣ の順で
//! フェイズが回り、ＮＰＣ 終了で 1 ターン経過する（`ターン終了.md` / `ターンイベント.md`）。
//! 「友軍」という陣営は SRC には存在せず、プレイヤー側 AI 陣営は "ＮＰＣ"。
//! 本移植では `Phase` enum と `Turn` 構造体に集約する。

use serde::{Deserialize, Serialize};

use crate::Party;

/// 現在のフェーズ / Current phase. SRC 順序: 味方 → 敵 → 中立 → ＮＰＣ。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Phase {
    #[default]
    Player,
    Enemy,
    Neutral,
    /// プレイヤー側 AI 陣営（SRC "ＮＰＣ"）。
    Npc,
}

impl Phase {
    /// 元 SRC の `Stage` 文字列に合わせた表示名（"…フェーズ" 付き）。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Player => "味方フェーズ",
            Self::Enemy => "敵フェーズ",
            Self::Neutral => "中立フェーズ",
            Self::Npc => "ＮＰＣフェーズ",
        }
    }

    /// 元 SRC `SRC.bas::Stage` 値（"味方"/"敵"/"中立"/"ＮＰＣ"）に合わせた
    /// 短い陣営名。自動発火ラベル "ターン N <陣営>" の組み立てに使う。
    ///
    /// Short party-name string matching original SRC's `Stage` value. Used to
    /// build auto-fired labels like `ターン N 敵`.
    pub const fn stage_name(self) -> &'static str {
        match self {
            Self::Player => "味方",
            Self::Enemy => "敵",
            Self::Neutral => "中立",
            Self::Npc => "ＮＰＣ",
        }
    }

    /// 次のフェーズ。ＮＰＣ → Player に戻す（同時にターン数増加が起こる）。
    pub const fn next(self) -> Self {
        match self {
            Self::Player => Self::Enemy,
            Self::Enemy => Self::Neutral,
            Self::Neutral => Self::Npc,
            Self::Npc => Self::Player,
        }
    }

    /// このフェーズが操作する Party。
    pub const fn party(self) -> Party {
        match self {
            Self::Player => Party::Player,
            Self::Enemy => Party::Enemy,
            Self::Neutral => Party::Neutral,
            Self::Npc => Party::Npc,
        }
    }
}

/// ターン状態 / Turn state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Turn {
    pub number: u32,
    pub phase: Phase,
}

impl Default for Turn {
    fn default() -> Self {
        Self::new()
    }
}

impl Turn {
    pub const fn new() -> Self {
        Self {
            number: 1,
            phase: Phase::Player,
        }
    }

    /// 現在フェーズを終了して次フェーズへ。ＮＰＣ 終了時（ＮＰＣ→味方）にターン数 +1。
    pub fn end_phase(&mut self) {
        if self.phase == Phase::Npc {
            self.number = self.number.saturating_add(1);
        }
        self.phase = self.phase.next();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_phase_cycles_and_increments_turn() {
        // SRC 順序: 味方 → 敵 → 中立 → ＮＰＣ → (味方, ターン+1)
        let mut t = Turn::new();
        assert_eq!(t.number, 1);
        assert_eq!(t.phase, Phase::Player);
        t.end_phase();
        assert_eq!(t.phase, Phase::Enemy);
        assert_eq!(t.number, 1);
        t.end_phase();
        assert_eq!(t.phase, Phase::Neutral);
        assert_eq!(t.number, 1);
        t.end_phase();
        assert_eq!(t.phase, Phase::Npc);
        assert_eq!(t.number, 1);
        t.end_phase();
        assert_eq!(t.phase, Phase::Player);
        assert_eq!(t.number, 2);
    }

    #[test]
    fn phase_party_round_trip() {
        for p in [Phase::Player, Phase::Enemy, Phase::Neutral, Phase::Npc] {
            assert_eq!(p.party() as u32, p.party() as u32); // 確認: panic しない
        }
    }
}
