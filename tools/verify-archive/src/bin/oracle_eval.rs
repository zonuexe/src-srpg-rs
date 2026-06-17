//! 差分オラクルの Rust 側評価器。標準入力から式を 1 行ずつ読み、本実装の
//! 実行時評価器で評価して結果を標準出力に返す (C# `tools/oracle-diff` と同形式)。
//!
//! C# 側の `Expression.GetValueAsString(expr)` に対応させるため、各式を
//! `Set z Eval(<式>)` として実行し `z` の値を読む。`Eval` は引数を式評価し、
//! 算術/比較/論理/連結/関数のいずれも文字列結果へ正規化する (本実装には
//! GetValueAsString 相当の単一入口が無く、Eval が最も近い万能評価)。
//! 空行・`#` 始まりはスキップ。
//!
//! 使い方:
//!   cargo run -q -p verify-archive --bin oracle_eval < corpus.txt
//! 差分:
//!   diff <(C# 出力) <(Rust 出力)

use src_core::data::event;
use src_core::{event_runtime, App};
use std::io::{self, BufRead, Write};

fn eval_line(line: &str) -> String {
    let src = format!("Set z Eval({line})\n");
    let mut app = App::new();
    match event::parse(&src) {
        Ok(stmts) => {
            let _ = event_runtime::execute(&mut app, &stmts);
            app.script_var("z").to_string()
        }
        Err(_) => "<PARSE_ERR>".to_string(),
    }
}

fn main() {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let result = eval_line(&line);
        let _ = writeln!(out, "{result}");
    }
}
