//! ステージ進行状態 / Stage flow state.
//!
//! 元 SRC `SRC.play.cs::StartScenario` / `StartTurn` / `GameOver` /
//! `GameClear` のステージ進行を以下の状態機械にまとめる:
//!
//! ```text
//!  ┌── App::start_scenario(name) ──┐
//!  ▼                               │  (Prologue / プロローグ 発火)
//!  Briefing ──advance──▶ Sortie ──advance──▶ Battle ──check_victory─┬─▶ Victory
//!                                       │   (Start / スタート 発火)  │  (Victory / 勝利 発火)
//!                                       │                            │
//!                                       │                            ╰─▶ Defeat
//!                                       │                                (GameOver / ゲームオーバー 発火)
//!                                       │
//!                                       └── 各 end_phase で
//!                                           App::begin_phase(party) を呼び
//!                                           "ターン N <陣営>" を発火。
//! ```
//!
//! 各状態と元 SRC `Stage` 文字列の対応:
//! - `Briefing` ↔ `Stage = "プロローグ"` （`Event.HandleEvent("プロローグ")` 後の待機）
//! - `Sortie`   ↔ 出撃可能ユニット配置確認（移植版独自の中間ステップ。元 SRC は
//!   `スタート` イベント内で `Place` 命令を流すので独立した状態を持たない）
//! - `Battle`   ↔ `Stage = "味方"/"敵"/"友軍"/"中立"/"ＮＰＣ"` の循環
//! - `Victory`  ↔ `GameClear` 後の終了オーバーレイ
//! - `Defeat`   ↔ `GameOver` 後の終了オーバーレイ
//!
//! 状態遷移は基本的に `App` のメソッドで実行する（直接 `set_stage_state` で
//! 飛ばすことも可能だがラベル自動発火を伴わない）。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum StageState {
    #[default]
    Briefing,
    Sortie,
    Battle,
    Victory,
    Defeat,
}

impl StageState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Briefing => "ブリーフィング",
            Self::Sortie => "出撃準備",
            Self::Battle => "戦闘中",
            Self::Victory => "勝利",
            Self::Defeat => "敗北",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels_present() {
        assert!(!StageState::Briefing.label().is_empty());
        assert!(!StageState::Victory.label().is_empty());
    }
}
