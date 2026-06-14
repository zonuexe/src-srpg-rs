//! モーダル進行制御の型付きビュー / Typed view of script-blocking modal state.
//!
//! 元来 `App` は `pending_dialog: Option<PendingDialog>` と
//! `pending_timer: Option<f64>` を独立した 2 つの Option として保持しており、
//! 「両方立っている時の振る舞い」「片方だけのときの再開タイミング」が
//! `tick()` / `respond_dialog()` / `event_runtime::resume()` の各所に
//! 暗黙に散らばっていた。
//!
//! 本モジュールはストレージは変えず、(`pending_dialog`, `pending_timer`)
//! の組み合わせを `ModalGate` 1 つの enum で型化し、不変条件 (誰が誰を
//! ブロックするか、tick がいつ resume を呼ぶか) を 1 箇所に集約する。
//!
//! 新たなモーダル種を追加する場合は `ModalGate` に variant を増やすこと
//! で `App::modal_gate()` の caller (tick / respond_dialog / render /
//! test_harness::snapshot) で漏れが compile error になる。配線不足の
//! 早期検出に使う。

use crate::dialog::PendingDialog;

/// スクリプト進行を阻害している「ゲート」。`App::modal_gate()` で取得する。
///
/// 不変条件:
/// - `Open` は dialog/timer 両方 `None` の時のみ。
/// - `Dialog(_)` は dialog 有り / timer 無し。
/// - `Timer(_)` は dialog 無し / timer 有り。
/// - `DialogOverTimer { .. }` は両方有り。dialog が先に応答され
///   `Timer(_)` に降格し、timer が満了したら `Open` に遷移する。
/// - `pending_audio` はスクリプトをブロックしない出力キューなのでここには含めない。
#[derive(Debug, Clone, PartialEq)]
pub enum ModalGate {
    /// スクリプト実行可。
    Open,
    /// 対話 UI 応答待ち (タイマ無し)。
    Dialog(PendingDialog),
    /// タイマ消費待ち (対話無し)。秒数 > 0。
    Timer(f64),
    /// 対話 UI + タイマが同時に立っている。応答後に Timer のみ残る。
    DialogOverTimer { dialog: PendingDialog, timer: f64 },
}

impl ModalGate {
    /// スクリプトが現在ブロックされているか。`Open` 以外で `true`。
    pub fn is_blocked(&self) -> bool {
        !matches!(self, Self::Open)
    }

    /// このゲート種別の短い文字列ラベル (debug / snapshot 用)。
    pub fn kind_label(&self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::Dialog(_) => "Dialog",
            Self::Timer(_) => "Timer",
            Self::DialogOverTimer { .. } => "DialogOverTimer",
        }
    }

    /// 「ユーザの dialog 応答を待っている」か (timer のみではない)。
    /// `respond_dialog` / `respond_dialog_text` が意味を持つ条件。
    pub fn awaits_dialog_response(&self) -> bool {
        matches!(self, Self::Dialog(_) | Self::DialogOverTimer { .. })
    }

    /// 「tick で timer を減らすべき」か。
    pub fn awaits_timer_tick(&self) -> bool {
        matches!(self, Self::Timer(_) | Self::DialogOverTimer { .. })
    }
}

/// `(dialog, timer)` から `ModalGate` を組み立てる純粋関数。
///
/// `App::modal_gate()` 経由で使う。ストレージを直接読まないので、テストや
/// 別経路から `(Option<PendingDialog>, Option<f64>)` を直接渡しても使える。
pub fn classify(dialog: Option<&PendingDialog>, timer: Option<f64>) -> ModalGate {
    match (dialog, timer) {
        (None, None) => ModalGate::Open,
        (Some(d), None) => ModalGate::Dialog(d.clone()),
        (None, Some(t)) => ModalGate::Timer(t),
        (Some(d), Some(t)) => ModalGate::DialogOverTimer {
            dialog: d.clone(),
            timer: t,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dialog::PendingDialog;

    fn talk() -> PendingDialog {
        PendingDialog::Talk {
            speaker: "A".into(),
            body: "B".into(),
        }
    }

    #[test]
    fn classify_covers_four_states() {
        assert_eq!(classify(None, None), ModalGate::Open);
        assert!(matches!(
            classify(Some(&talk()), None),
            ModalGate::Dialog(_)
        ));
        assert!(matches!(classify(None, Some(0.5)), ModalGate::Timer(_)));
        assert!(matches!(
            classify(Some(&talk()), Some(0.5)),
            ModalGate::DialogOverTimer { .. }
        ));
    }

    #[test]
    fn predicates_match_state() {
        let g = ModalGate::Open;
        assert!(!g.is_blocked());
        assert!(!g.awaits_dialog_response());
        assert!(!g.awaits_timer_tick());

        let g = ModalGate::Dialog(talk());
        assert!(g.is_blocked());
        assert!(g.awaits_dialog_response());
        assert!(!g.awaits_timer_tick());

        let g = ModalGate::Timer(1.0);
        assert!(g.is_blocked());
        assert!(!g.awaits_dialog_response());
        assert!(g.awaits_timer_tick());

        let g = ModalGate::DialogOverTimer {
            dialog: talk(),
            timer: 1.0,
        };
        assert!(g.is_blocked());
        assert!(g.awaits_dialog_response());
        assert!(g.awaits_timer_tick());
    }
}
