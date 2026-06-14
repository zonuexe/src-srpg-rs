//! 音声再生 / Audio playback.
//!
//! 元 SRC は MP3 を VBMP3 / Susie プラグイン経由、MIDI を DirectMusic 経由で
//! 鳴らしていた。ブラウザでは HtmlAudioElement / Web Audio で MP3 を、
//! MIDI は PicoAudio.js (JS ライブラリ; src-web/midi.rs) 経由で鳴らす。

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Blob, BlobPropertyBag, HtmlAudioElement, Url};

/// 与えられた MP3 / OGG / WAV のバイト列を再生。`volume` は 0..=100。
/// `loop_playback=true` のとき終了せずループ再生する (BGM 用)。
/// 返り値の `HtmlAudioElement` を保持し続けないと GC でリソースが解放され
/// 再生が止まる点に注意（呼び出し側で適切に保管すること）。
pub fn play_audio(
    bytes: &[u8],
    mime: &str,
    volume: u8,
    loop_playback: bool,
) -> Result<HtmlAudioElement, JsValue> {
    let array = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    array.copy_from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let bag = BlobPropertyBag::new();
    bag.set_type(mime);
    let blob = Blob::new_with_buffer_source_sequence_and_options(&parts, &bag)?;
    let url = Url::create_object_url_with_blob(&blob)?;

    let audio = HtmlAudioElement::new()?;
    audio.set_src(&url);
    audio.set_loop(loop_playback);
    let v = (f64::from(volume) / 100.0).clamp(0.0, 1.0);
    audio.set_volume(v);
    let _ = audio.play()?;

    // ループしない場合のみ、再生終了で ObjectURL を解放
    if !loop_playback {
        let url_for_cleanup = url.clone();
        let onended = Closure::<dyn FnMut(web_sys::Event)>::new(move |_| {
            let _ = Url::revoke_object_url(&url_for_cleanup);
        });
        audio.set_onended(Some(onended.as_ref().unchecked_ref()));
        onended.forget();
    }
    Ok(audio)
}

/// 既に再生中の `HtmlAudioElement` を停止し、リソースを解放する。
pub fn stop_audio(audio: &HtmlAudioElement) {
    audio.pause().ok();
    audio.set_current_time(0.0);
    let src = audio.src();
    if !src.is_empty() {
        let _ = Url::revoke_object_url(&src);
        audio.set_src("");
    }
}

/// 拡張子で MIME を判定。
pub fn audio_mime_from_name(name: &str) -> Option<&'static str> {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".mp3") {
        Some("audio/mpeg")
    } else if lower.ends_with(".ogg") {
        Some("audio/ogg")
    } else if lower.ends_with(".wav") {
        Some("audio/wav")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mime_detection_by_name() {
        assert_eq!(audio_mime_from_name("battle.mp3"), Some("audio/mpeg"));
        assert_eq!(audio_mime_from_name("bgm.OGG"), Some("audio/ogg"));
        assert_eq!(audio_mime_from_name("voice.WAV"), Some("audio/wav"));
        assert_eq!(audio_mime_from_name("foo.mid"), None);
    }
}
