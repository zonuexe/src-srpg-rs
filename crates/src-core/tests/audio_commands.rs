//! 音声系コマンド (Startbgm / Stopbgm / Keepbgm / Playsound / PlayVoice)
//! の edge cases。

use src_core::audio::AudioRequest;
use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn run(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

fn audio_names(app: &App) -> Vec<String> {
    app.pending_audio()
        .iter()
        .map(|a| match a {
            AudioRequest::StartBgm { name } => format!("StartBgm:{name}"),
            AudioRequest::StopBgm => "StopBgm".to_string(),
            AudioRequest::KeepBgm => "KeepBgm".to_string(),
            AudioRequest::PlaySound { name } => format!("PlaySound:{name}"),
            AudioRequest::PlayVoice { name } => format!("PlayVoice:{name}"),
            AudioRequest::PlayMidi { name } => format!("PlayMidi:{name}"),
        })
        .collect()
}

#[test]
fn startbgm_pushes_request() {
    let app = run(r#"Startbgm "battle.mid""#);
    let names = audio_names(&app);
    assert_eq!(names, vec!["StartBgm:battle.mid".to_string()]);
}

#[test]
fn stopbgm_pushes_request() {
    let app = run("Stopbgm\n");
    let names = audio_names(&app);
    assert_eq!(names, vec!["StopBgm".to_string()]);
}

#[test]
fn keepbgm_pushes_request() {
    let app = run("Keepbgm\n");
    let names = audio_names(&app);
    assert_eq!(names, vec!["KeepBgm".to_string()]);
}

#[test]
fn playsound_pushes_request() {
    let app = run(r#"Playsound "hit.wav""#);
    let names = audio_names(&app);
    assert_eq!(names, vec!["PlaySound:hit.wav".to_string()]);
}

#[test]
fn audio_queue_preserves_order() {
    let app = run(r#"
Startbgm "a.mid"
Playsound "b.wav"
Stopbgm
Startbgm "c.mid"
"#);
    let names = audio_names(&app);
    assert_eq!(
        names,
        vec![
            "StartBgm:a.mid",
            "PlaySound:b.wav",
            "StopBgm",
            "StartBgm:c.mid"
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>()
    );
}

#[test]
fn take_pending_audio_drains_queue() {
    let mut app = run(r#"
Startbgm "x.mid"
Playsound "y.wav"
"#);
    assert_eq!(app.pending_audio().len(), 2);
    let drained = app.take_pending_audio();
    assert_eq!(drained.len(), 2);
    assert_eq!(app.pending_audio().len(), 0);
}
