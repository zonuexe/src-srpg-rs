//! 単一 `.eve` ファイルを App で実行して結果を出力するデバッグ用 CLI。
//!
//! 使い方: `cargo run -p verify-archive --bin run_eve -- <path.eve>`

use std::env;
use std::fs;
use std::process::ExitCode;
use std::time::Instant;

use src_core::data::event;
use src_core::data::loader;
use src_core::event_runtime;
use src_core::App;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: run_eve <path.eve>");
        return ExitCode::FAILURE;
    }
    let path = &args[1];
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("read failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let text = loader::decode_text(&bytes);
    let stmts = match event::parse(&text) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("parse failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!("parsed: {} statements", stmts.len());
    let mut app = App::new();
    let t0 = Instant::now();
    match event_runtime::execute(&mut app, &stmts) {
        Ok(()) => println!(
            "executed OK in {:.3}s, vars={} messages={}",
            t0.elapsed().as_secs_f64(),
            app.script_vars().len(),
            app.messages().len()
        ),
        Err(e) => println!(
            "execute error: {e} (after {:.3}s)",
            t0.elapsed().as_secs_f64()
        ),
    }
    println!(
        "pending_dialog: {:?}",
        app.pending_dialog().map(|d| d.kind())
    );
    if let Some(d) = app.pending_dialog() {
        println!("dialog details: {:?}", d);
    }
    if !app.hotpoints().is_empty() {
        println!("hotpoints: {}", app.hotpoints().len());
        for h in app.hotpoints() {
            println!("  - {} @ ({}, {}, {}, {})", h.name, h.x, h.y, h.w, h.h);
        }
    }
    for u in &app.database().unit_instances {
        println!(
            "  [unit] uid={} party={:?} data={:?} pilot={:?} ({},{})",
            u.uid, u.party, u.unit_data_name, u.pilot_name, u.x, u.y,
        );
    }
    for (k, v) in app.script_vars() {
        let vshow = if v.chars().count() > 80 {
            format!("{}…", v.chars().take(80).collect::<String>())
        } else {
            v.clone()
        };
        println!("  {k} = {vshow}");
    }
    ExitCode::SUCCESS
}
