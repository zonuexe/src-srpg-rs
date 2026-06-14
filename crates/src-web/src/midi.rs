//! MIDI 再生 / MIDI playback via PicoAudio.js.
//!
//! 元 SRC は DirectMusic / MCI 経由で `*.mid` を鳴らしていた。ブラウザでは
//! [PicoAudio.js](https://github.com/cagpie/PicoAudio.js) を `<script>` で
//! 読み込み、`window.srcPlayMidi(bytes, volume)` JS グルー関数を経由して
//! 再生する。

use wasm_bindgen::prelude::*;

#[wasm_bindgen]
extern "C" {
    /// `window.srcPlayMidi(bytes, volume, loop) -> boolean`
    #[wasm_bindgen(js_namespace = window, js_name = srcPlayMidi)]
    fn js_play_midi(bytes: js_sys::Uint8Array, volume: f64, loop_play: bool) -> bool;
    /// `window.srcStopMidi()`
    #[wasm_bindgen(js_namespace = window, js_name = srcStopMidi)]
    fn js_stop_midi();
}

/// MIDI バイト列を再生。`volume` は 0..=100。`loop_play=true` で BGM ループ。
/// 成功なら `Ok(())`。
pub fn play_midi(bytes: &[u8], volume: u8, loop_play: bool) -> Result<(), JsValue> {
    let array = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    array.copy_from(bytes);
    let v = (f64::from(volume) / 100.0).clamp(0.0, 1.0);
    let ok = js_play_midi(array, v, loop_play);
    if ok {
        Ok(())
    } else {
        Err(JsValue::from_str(
            "srcPlayMidi が失敗（PicoAudio.js 未ロードまたは MIDI 不正）",
        ))
    }
}

/// 再生中の MIDI を停止。
pub fn stop_midi() {
    js_stop_midi();
}
