//! 差分オラクルの Switch モード (Rust 側)。`oracle_scenario` と同じ入力形式で、上段の
//! コマンド列を実行したあと、下段の `Switch` ～ `EndSw` の各ブロックを実行し、飛び込んだ
//! `Case` の行 (ブロック内 0 始まりの行番号。どれにも入らなければ `end`) を出力する
//! (C# `tools/oracle-diff switch` と突合する。原典 `ExecSwitchCmd`)。
//!
//! 各 `Case` 行の直後に目印の `Set __hit <行番号>` を差し込んで実行し、その値を読む。
//!
//! 使い方:
//!   cargo run -q -p verify-archive --bin oracle_switch < switch.txt

use src_core::data::event;
use src_core::{event_runtime, App};
use std::io::{self, BufRead, Write};

fn run(app: &mut App, src: &str) -> bool {
    match event::parse(src) {
        Ok(stmts) => event_runtime::execute(app, &stmts).is_ok(),
        Err(_) => false,
    }
}

fn is_case_line(line: &str) -> bool {
    let head = line.split_whitespace().next().unwrap_or("");
    head.eq_ignore_ascii_case("case") || head.eq_ignore_ascii_case("caseelse")
}

fn main() {
    let stdin = io::stdin();
    let mut cmds: Vec<String> = Vec::new();
    let mut blocks: Vec<Vec<String>> = Vec::new();
    let mut block: Vec<String> = Vec::new();
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
        if !in_probes {
            cmds.push(line);
            continue;
        }
        let end = line.trim().eq_ignore_ascii_case("endsw");
        block.push(line);
        if end {
            blocks.push(std::mem::take(&mut block));
        }
    }

    let mut app = App::new();
    run(&mut app, &format!("{}\n", cmds.join("\n")));

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for b in &blocks {
        let mut src = String::from("Set __hit end\n");
        for (k, line) in b.iter().enumerate() {
            src.push_str(line);
            src.push('\n');
            if is_case_line(line) {
                src.push_str(&format!("Set __hit {k}\n"));
            }
        }
        let v = if run(&mut app, &src) {
            app.script_var("__hit").to_string()
        } else {
            "<ERR>".to_string()
        };
        let _ = writeln!(out, "{v}");
    }
}
