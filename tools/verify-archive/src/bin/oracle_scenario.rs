//! 差分オラクルのコマンド列モード (Rust 側)。標準入力を `===PROBES===` で分け、
//! 上段の .eve コマンドを順に実行し、下段の probe 式を評価して結果を標準出力へ
//! (C# `tools/oracle-diff scenario` と同形式)。
//!
//! probe は変数/配列/関数の読み出しを想定し、`Set __probe_N $(<probe>)` で解決して
//! 読む (C# 側の `GetValueAsString(probe)` に対応)。
//!
//! 使い方:
//!   cargo run -q -p verify-archive --bin oracle_scenario < scenario.txt

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

    let mut script = cmds.join("\n");
    script.push('\n');
    for (i, p) in probes.iter().enumerate() {
        script.push_str(&format!("Set __probe_{i} $({p})\n"));
    }

    let mut app = App::new();
    if let Ok(stmts) = event::parse(&script) {
        let _ = event_runtime::execute(&mut app, &stmts);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for i in 0..probes.len() {
        let _ = writeln!(out, "{}", app.script_var(&format!("__probe_{i}")));
    }
}
