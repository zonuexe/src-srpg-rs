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
//! `@unit <name> <rank> <party>` 行はユニット実体の生成指令 (C# `placeunit` モードの
//! `UList.Add(name, rank, party)` と対応)。本実装の `Place` は rank を無視する
//! (改造段階が UnitInstance に未配線＝既知の差) ため rank は捨て、無人 (`-`) ・衝突回避の
//! 座標で `Place` する。生成後 `Info(ユニット, <name>, …)` を probe できる。
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

    // stdin を読み、`@unit` 生成指令と probe に分ける。
    let stdin = io::stdin();
    let mut probes: Vec<String> = Vec::new();
    let mut creates: Vec<String> = Vec::new();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("@unit ") {
            // `@unit <name> <rank> <party>` → C# `UList.Add(name, rank, party)` と対応。
            // Rust は `Create <party> <name> <rank> - 0 <x> 1` で生成する (Create は rank=改造段階
            // を反映する。Place は rank 引数を持たないため Create を使う)。無人 (`-`)・座標は
            // 指令順 (衝突回避)。
            let f: Vec<&str> = rest.split_whitespace().collect();
            if f.len() >= 3 {
                let (name, rank, party) = (f[0], f[1], f[2]);
                let x = creates.len() + 1;
                creates.push(format!("Create {party} {name} {rank} - 0 {x} 1"));
            }
            continue;
        }
        probes.push(line);
    }

    // データロード後にユニットを生成し、C# `GetValueAsString(probe)` に対応する
    // `Set __probe_N $(<probe>)` で probe を解決する。
    let mut script = String::new();
    for c in &creates {
        script.push_str(c);
        script.push('\n');
    }
    for (i, p) in probes.iter().enumerate() {
        script.push_str(&format!("Set __probe_{i} $({p})\n"));
    }
    if let Ok(stmts) = src_core::data::event::parse(&script) {
        let _ = event_runtime::execute(&mut app, &stmts);
    }
    eprintln!(
        "created={} unit_instances={}",
        creates.len(),
        app.database().unit_instances.len()
    );

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
