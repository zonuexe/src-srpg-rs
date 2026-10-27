//! 差分オラクルの評価経路モード (Rust 側)。`oracle_scenario` と同じ入力形式で、上段の
//! コマンド列を実行したあと、下段の probe 式を本実装の **各評価経路** で評価して
//! タブ区切りで出力する (C# `tools/oracle-diff paths` と突合する)。
//!
//! 出力列: `Eval  代入  If  メニュー条件  新評価器(値)  新評価器(If)`
//! - Eval: `Set z Eval(<式>)` の値 (C# `GetValueAsString` と比較)
//! - 代入: `z = <式>` (`=` 代入形。値は式評価) の値 (同上)
//! - If: `If <式> Then` の真偽 1/0 (C# `AIfCmd.Evaluate()` と比較)
//! - メニュー条件: `evaluate_command_condition` の真偽 1/0 (同上)
//! - 新評価器: `event_runtime::eval_expression` / `eval_if_condition` (統一評価器)
//!
//! 使い方:
//!   cargo run -q -p verify-archive --bin oracle_paths < paths.txt

use src_core::data::event;
use src_core::{event_runtime, App};
use std::io::{self, BufRead, Write};

fn run(app: &mut App, src: &str) {
    if let Ok(stmts) = event::parse(src) {
        let _ = event_runtime::execute(app, &stmts);
    }
}

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
    run(&mut app, &format!("{}\n", cmds.join("\n")));

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for p in &probes {
        run(&mut app, &format!("Set __e Eval({p})\n"));
        run(&mut app, &format!("__s = {p}\n"));
        run(
            &mut app,
            &format!("If {p} Then\nSet __a 1\nElse\nSet __a 0\nEndIf\n"),
        );
        let e = app.script_var("__e").to_string();
        let s = app.script_var("__s").to_string();
        let a = app.script_var("__a").to_string();
        let c = u8::from(event_runtime::evaluate_command_condition(&mut app, Some(p)));
        let v = event_runtime::eval_expression(&app, p);
        let w = u8::from(event_runtime::eval_if_condition(&app, p));
        let _ = writeln!(out, "{e}\t{s}\t{a}\t{c}\t{v}\t{w}");
        // 次の probe に前の値が残らないよう消す。
        run(&mut app, "UnSet __e\nUnSet __s\nUnSet __a\n");
    }
}
