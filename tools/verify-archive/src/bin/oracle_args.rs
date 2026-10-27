//! 差分オラクルのコマンド引数モード (Rust 側)。`oracle_scenario` と同じ入力形式で、
//! 上段のコマンド列を実行したあと、下段の各行をコマンドとして構文解析し、引数ごとの
//! `文字列値|整数値|実数値` をタブ区切りで出力する (C# `tools/oracle-diff args` と突合する。
//! 原典 `CmdData.GetArgAsString` / `GetArgAsLong` / `GetArgAsDouble`)。
//!
//! 使い方:
//!   cargo run -q -p verify-archive --bin oracle_args < args.txt

use src_core::data::event;
use src_core::{event_runtime, App};
use std::io::{self, BufRead, Write};

fn main() {
    let stdin = io::stdin();
    let mut cmds: Vec<String> = Vec::new();
    let mut probes: Vec<String> = Vec::new();
    let mut in_probes = false;
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line == "===PROBES===" {
            in_probes = true;
            continue;
        }
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if in_probes {
            probes.push(line);
        } else {
            cmds.push(line);
        }
    }
    let mut app = App::new();
    if let Ok(stmts) = event::parse(&format!("{}\n", cmds.join("\n"))) {
        let _ = event_runtime::execute(&mut app, &stmts);
    }
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for p in &probes {
        let cols: Vec<String> = event_runtime::eval_command_args(&app, p)
            .into_iter()
            .map(|(s, n, d)| format!("{s}|{n}|{d}"))
            .collect();
        let _ = writeln!(out, "{}", cols.join("\t"));
    }
}
