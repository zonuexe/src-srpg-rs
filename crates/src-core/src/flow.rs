//! 進行フローの明示的継続 (continuation)。
//!
//! VB6 原典 (`SRC_20121125/SRC.bas`) では `StartScenario` / `StartTurn` が
//! 線形手続きで、「`HandleEvent "スタート"` から戻ったら次は `StartTurn "味方"`」
//! のように **「次にやること」は VB6 のコールスタックが暗黙に保持** していた。
//! 非ブロッキング化した本実装ではスクリプトが suspend/resume するため、その
//! 継続情報を serde 可能なデータ ([`FlowCont`]) として `App::flow` スタックに
//! 明示的に積む。
//!
//! 動作規則 (docs/FLOW_REDESIGN.md §2.2):
//!
//! 1. イベントを起動する側は、起動前に「完了後にやること」を `App::flow` に
//!    push する。
//! 2. スクリプトが **完了** したら (インライン完了でも suspend 後の resume
//!    完了でも) `event_runtime::run_loop` が `App::on_script_completed()` を
//!    呼び、idle な間 `flow` を pop して継続を実行する。
//! 3. 継続の実行がさらにイベントを起動して suspend したら、そこで drain は
//!    止まり、次の完了時に再開する。
//!
//! これにより「スクリプトがインラインで完了したか suspend したか」を呼び出し
//! 側が区別する必要がなくなる (旧 `start_battle_phase_after_inline_load` の
//! 廃止理由)。

/// スクリプト完了後に実行する継続 1 件。
///
/// 1 バリアント = VB6 原典の「`HandleEvent` 呼び出し直後のコード」に相当する。
/// `App::flow: Vec<FlowCont>` にスタックとして積まれ、
/// `App::on_script_completed()` が idle 時に pop して実行する。
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FlowCont {
    /// `スタート` / `Start` イベント完了後: 味方フェイズを開始する。
    /// 原典 `StartScenario` 末尾の `StartTurn "味方"` に相当
    /// (SRC.bas L1262)。`begin_battle` が `スタート` 発火前に push する。
    AfterStartEvent,
    /// インターミッションのサブコマンド `.eve` 完了後: メニュー
    /// (`Scene::Intermission`) へ復帰する。
    ReturnToIntermissionMenu,
    /// `Continue` チェインのステージファイル実行完了後:
    ///
    /// - `次ステージ` が再予約されていれば何もしない (ループバック)。
    /// - ステージファイルが `スタート` ラベルを **通過実行** していれば
    ///   (`App::start_passed_pcs` のファイル範囲判定)、`スタート` を再発火せずに Battle へ入る
    ///   (再発火すると敵が二重配置される)。
    /// - 通過していなければ通常経路 (auto_progress → `begin_battle` が
    ///   ファイルスコープで `スタート` を発火) に任せる。
    AfterStageFileRun,
    /// `Continue <file>` (非インターミッション) によるシナリオ終了後:
    /// `次ステージ` の予約を消費して次ステージを起動する
    /// (`advance_to_next_stage`)。原典の `IsScenarioFinished` →
    /// `StartScenario(次ステージ)` 相当。`Continue` コマンドが flow を
    /// 一括差し替え (旧ステージの継続を破棄) した上で積む。
    LoadNextStage,
}
