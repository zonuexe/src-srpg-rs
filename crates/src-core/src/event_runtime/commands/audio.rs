//! `.eve` コマンド: BGM・効果音・音声。

use super::*;

pub(super) fn exec(app: &mut App, c: &Cmd) -> Option<Result<usize, ScriptError>> {
    Some(match c.lname.as_str() {
        "startbgm" => exec_startbgm(app, c),
        "stopbgm" => exec_stopbgm(app, c),
        "keepbgm" => exec_keepbgm(app, c),
        "playsound" => exec_playsound(app, c),
        "playvoice" => exec_playvoice(app, c),
        "playmidi" => exec_playmidi(app, c),
        _ => return None,
    })
}

fn exec_startbgm(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    if let Some(name) = xargs.first() {
        app.push_audio_request(crate::audio::AudioRequest::StartBgm { name: name.clone() });
    }
    Ok(pc + 1)
}

fn exec_stopbgm(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    app.push_audio_request(crate::audio::AudioRequest::StopBgm);
    Ok(pc + 1)
}

fn exec_keepbgm(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    app.push_audio_request(crate::audio::AudioRequest::KeepBgm);
    Ok(pc + 1)
}

fn exec_playsound(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    if let Some(name) = xargs.first() {
        app.push_audio_request(crate::audio::AudioRequest::PlaySound { name: name.clone() });
    }
    Ok(pc + 1)
}

fn exec_playvoice(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    if let Some(name) = xargs.first() {
        app.push_audio_request(crate::audio::AudioRequest::PlayVoice { name: name.clone() });
    }
    Ok(pc + 1)
}

fn exec_playmidi(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `PlayMIDI name [volume]` (`PlayMIDIコマンド.md`):
    // Midi/<name>.mid を 1 回再生 (BGM とは別チャネル)。
    // volume は本実装で未対応 (フロントエンド側で固定値)。
    if let Some(name) = xargs.first() {
        app.push_audio_request(crate::audio::AudioRequest::PlayMidi { name: name.clone() });
    }
    Ok(pc + 1)
}
