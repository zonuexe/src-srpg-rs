//! 差分オラクルのデータロードモード (Rust 側)。コマンドライン引数で渡した
//! データディレクトリ (`pilot.txt` / `unit.txt` / `item.txt` / `sp.txt` 等) を
//! ロードし、標準入力の probe 式を評価して結果を標準出力へ
//! (C# `tools/oracle-diff loaddata <dir>` と同形式)。
//!
//! 原典 C# は `SRC.LoadDataDirectory(dir)` が同じファイル群を `PDList`/`UDList`/…
//! へロードする。両エンジンに同一の `Info(ユニットデータ, …)` / `Info(パイロットデータ, …)`
//! probe を通すと、パーサと Info 照会の fidelity を cross-engine で diff できる
//! (ユニット/combat 状態 diff の foundation = 静的データ層)。
//!
//! probe は C# 側の `GetValueAsString(probe)` に対応させ、`Set __probe_N $(<probe>)`
//! で解決して読む (oracle_scenario と同形式)。空行・`#` 始まりはスキップ。
//!
//! 使い方:
//!   cargo run -q -p verify-archive --bin oracle_loaddata -- <data_dir> < probes.txt

use src_core::data::{item, loader, pilot, special_power, unit};
use src_core::{event_runtime, App};
use std::io::{self, BufRead, Write};
use std::path::Path;

fn main() {
    let dir = match std::env::args().nth(1) {
        Some(d) => d,
        None => {
            eprintln!("usage: oracle_loaddata <data_dir> < probes.txt");
            std::process::exit(2);
        }
    };

    let mut app = App::new();
    load_data_directory(&mut app, Path::new(&dir));

    // probe をすべて読み込む
    let stdin = io::stdin();
    let mut probes: Vec<String> = Vec::new();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        probes.push(line);
    }

    // C# `GetValueAsString(probe)` に対応: `Set __probe_N $(<probe>)` で解決。
    let mut script = String::new();
    for (i, p) in probes.iter().enumerate() {
        script.push_str(&format!("Set __probe_{i} $({p})\n"));
    }
    if let Ok(stmts) = src_core::data::event::parse(&script) {
        let _ = event_runtime::execute(&mut app, &stmts);
    }

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for i in 0..probes.len() {
        let _ = writeln!(out, "{}", app.script_var(&format!("__probe_{i}")));
    }
}

/// C# `SRC.LoadDataDirectory` と同じファイル群を同順でロードする。
/// 物理ファイルは Shift-JIS なので `loader::decode_text` で UTF-8 化する。
fn load_data_directory(app: &mut App, dir: &Path) {
    // sp.txt / mind.txt (C# は mind.txt 優先)
    if let Some(txt) = read_data(dir, "mind.txt").or_else(|| read_data(dir, "sp.txt")) {
        let (sps, _) = special_power::parse_lenient(&txt);
        app.database_mut().extend_special_powers(sps);
    }
    // pilot.txt
    if let Some(txt) = read_data(dir, "pilot.txt") {
        let (pilots, _) = pilot::parse_lenient(&txt);
        app.database_mut().extend_pilots(pilots);
    }
    // robot.txt / unit.txt (どちらも UDList へ。両方あれば両方ロード)
    if let Some(txt) = read_data(dir, "robot.txt") {
        let (units, _) = unit::parse_lenient(&txt);
        app.database_mut().extend_units(units);
    }
    if let Some(txt) = read_data(dir, "unit.txt") {
        let (units, _) = unit::parse_lenient(&txt);
        app.database_mut().extend_units(units);
    }
    // item.txt
    if let Some(txt) = read_data(dir, "item.txt") {
        let (items, _) = item::parse_lenient(&txt);
        app.database_mut().extend_items(items);
    }

    eprintln!(
        "loaded: pilots={} units={} items={} sp={}",
        app.database().pilots.len(),
        app.database().units.len(),
        app.database().items.len(),
        app.database().special_powers.len(),
    );
}

fn read_data(dir: &Path, name: &str) -> Option<String> {
    let path = dir.join(name);
    match std::fs::read(&path) {
        Ok(bytes) => Some(loader::decode_text(&bytes)),
        Err(_) => None,
    }
}
