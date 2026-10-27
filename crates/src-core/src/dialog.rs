//! ペンディング対話 UI / Pending dialog presented to the player.
//!
//! 元 SRC では `Talk` / `Confirm` / `Input` 等の対話命令は
//! `Event.bas::HandleEvent` 内で実行スレッドをモーダル待ちにする。
//! Web 移植では非同期化を避けるため、対話命令にぶつかった時点で
//! インタプリタを「サスペンド状態」にし、ユーザ入力後に
//! `event_runtime::resume` で再開する。
//!
//! `App.pending_dialog` がこの enum を持っている間は MapView 上に
//! オーバーレイを表示し、Enter/Y/N キーなどで応答する。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingDialog {
    /// `Talk speaker` … `End` ブロック。ナレーションは speaker 空。
    Talk { speaker: String, body: String },
    /// `Wait Click/Press/Key/Input` で Hotpoint が無いケース。
    /// 原典 SRC.NET (`CmdData.cs::ExecWaitCmd "click"`) は何も描画せず
    /// クリック/キー押下まで spin-wait するだけ。直前の `PaintString` /
    /// `PaintPicture` 描画がそのまま画面に残るのが本来の挙動。
    /// レンダラは本変種を **何も描かない** で、ClickAt/Enter を `respond_dialog(0)`
    /// で resume 再開する仕組みだけ流用する。
    WaitClick,
    /// `Confirm 質問文` → Yes/No 応答。答えを `var_name` に "0"/"1" で格納。
    Confirm { question: String, var_name: String },
    /// `Menu 質問文` … 各行が選択肢 … `End`。
    /// 1-indexed の選択番号 (default) または対応する `options[i-1]` の
    /// 文字列値 (`store_value = true`) を `var_name` に格納。
    /// 0 (キャンセル/Esc) は常に "0" を格納。
    Menu {
        prompt: String,
        options: Vec<String>,
        var_name: String,
        /// true なら選択された option の文字列値を var に書き込む。
        /// 元 SRC の `Hotpoint` 経由の選択（クリックされた hotpoint の name を
        /// 変数に格納）と一致させるためのモード。
        #[serde(default)]
        store_value: bool,
        /// SRC `Ask` Format 2 用。`options[i]` に対応する配列の添字 (subscript)。
        /// 非空なら `respond_dialog` は文字列値ではなく **添字** を var に格納する。
        /// 元 SRC の `Ask 配列変数 ...` は選んだ要素の添字を `選択` に返す仕様。
        #[serde(default)]
        option_keys: Vec<String>,
        /// `true` ならキャンセル (Esc / 右クリック / choice 0) を受け付けない。
        /// `キャンセル可` オプションの無い `Ask` は SRC では選択必須で、キャンセル
        /// すると `選択 = 0` になり「キャラ未選択 → 味方0体 → 即敗北」を招くため、
        /// 必須選択を強制する。`#[serde(default)]` = `false` (従来どおりキャンセル可)。
        #[serde(default)]
        non_cancellable: bool,
    },
    /// `Input var prompt default` — テキスト入力モーダル。
    /// ユーザ入力（Enter で確定）を `var_name` に格納。Esc キャンセル時は
    /// `default` を保持。
    Input {
        prompt: String,
        var_name: String,
        default: String,
    },
}

impl PendingDialog {
    /// 簡易ラベル（デバッグ表示用）。
    pub const fn kind(&self) -> &'static str {
        match self {
            Self::Talk { .. } => "Talk",
            Self::WaitClick => "WaitClick",
            Self::Confirm { .. } => "Confirm",
            Self::Menu { .. } => "Menu",
            Self::Input { .. } => "Input",
        }
    }
}
