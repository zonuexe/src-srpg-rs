//! オーディオ要求キュー / Audio request queue.
//!
//! `event_runtime` の `Startbgm` / `Stopbgm` / `Playsound` / `Keepbgm`
//! 命令は副作用として `App.pending_audio` にリクエストを積む。
//! フロントエンド (src-web) は毎フレーム `take_pending_audio()` で
//! 取り出して実際の HtmlAudioElement / PicoAudio.js を駆動する。
//!
//! 純 Rust ロジック側に Web API を直接持ち込まないためのバッファ。

use serde::{Deserialize, Serialize};

/// 1 件のオーディオ要求 / One audio playback request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum AudioRequest {
    /// `Startbgm name [volume]` — BGM 再生。同名 BGM が再生中なら継続。
    /// 通常は loop=true で繰り返す。`name` は basename (拡張子付き / 無し
    /// どちらでも src-web 側で解決する)。
    StartBgm { name: String },
    /// `Stopbgm` — BGM 停止。
    StopBgm,
    /// `Keepbgm` — 次のシーン遷移 / Stopbgm を 1 回だけ無視する hint。
    /// 本実装ではフロントエンドに伝達するのみ。
    KeepBgm,
    /// `Playsound name [volume]` — 一回限りの SE 再生。
    PlaySound { name: String },
    /// `PlayVoice name` — ボイス再生 (PlaySound と同義扱い)。
    PlayVoice { name: String },
    /// `PlayMIDI name [volume]` — MIDI 単発再生 (BGM とは別チャネル)。
    /// 元 SRC は `Midi\<name>.mid` を MIDI シーケンサで再生。`StartBgm` と
    /// 違いループ無しで 1 回。フロントエンドは PicoAudio.js 等で再生する。
    PlayMidi { name: String },
}
