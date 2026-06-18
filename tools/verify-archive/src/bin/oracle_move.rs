//! 差分オラクルの移動範囲モード (Rust 側)。コマンドライン引数で渡したデータ
//! ディレクトリ (`pilot.txt` / `unit.txt` / `system/../terrain.txt` 等) をロードし、
//! 標準入力の指令言語でマップ・ユニットを組み立て、各ユニットの到達可能マス集合を
//! 標準出力へ書き出す (C# `tools/oracle-diff moverange <dir>` と同形式)。
//!
//! 原典 C# は `Map.AreaInSpeed(u)` が `Map.TotalMoveCost[x,y]` (2× game scale,
//! start=0) を埋め、`TotalMoveCost <= 2*Speed` のマスが到達可能。Rust は
//! `movement::compute_range_with(map, start, speed, cost_fn)` が
//! `cell -> 残り MP` を返す (start=max_mp、不到達は欠落)。両者を共通正規化
//! `<x> <y> <cost2x>` (0-indexed, 2× scale) へ落とし込み cross-engine で diff する。
//!
//! ## 指令言語 (stdin)
//! - `@map <w> <h>`                     — w×h の平地マップ (terrain id 0) を生成。
//! - `@cell <x> <y> <terrain_id>`       — セル (0-indexed) の地形 id を上書き。
//! - `@unit <name> <rank> <party> <pilot> <level> <x> <y>` — ユニットを (x,y) 0-indexed に配置。
//! - `@move <name>`                     — そのユニットの到達マスを出力 (ヘッダ `=== move <name> ===`)。
//!
//! `@map` → `@cell` → `@unit` → `@move` の順に処理する。空行・`#` 始まりはスキップ。
//!
//! ## 正規化 (FLAT 検証で要確認)
//! 各到達マスを `<x> <y> <cost2x>` (x,y 昇順) で出力する:
//! - 座標は 0-indexed。
//! - `cost2x` (Rust) = `(max_mp - remaining_mp) * 2` (start=0)。
//! - 到達不能マスは出力しない (両エンジン共通)。
//!
//! 使い方:
//!   cargo run -q -p verify-archive --bin oracle_move -- <data_dir> < move_corpus.txt

use src_core::data::map::{MapCell, MapData};
use src_core::data::{item, loader, pilot, special_power, terrain_file, unit};
use src_core::App;
use std::io::{self, BufRead, Write};
use std::path::Path;

/// 配置済みユニットの最小情報 (移動範囲計算に必要な属性)。
struct PlacedUnit {
    name: String,
    /// 配置セル (0-indexed)。
    pos: (u32, u32),
}

fn main() {
    let dir = match std::env::args().nth(1) {
        Some(d) => d,
        None => {
            eprintln!("usage: oracle_move <data_dir> < move_corpus.txt");
            std::process::exit(2);
        }
    };

    let mut app = App::new();
    load_data_directory(&mut app, Path::new(&dir));

    // 指令を順に処理する。map → cell → unit → move の順で来る想定だが、行を読みながら
    // 即時に状態を更新する (map を最初に作り、cell で上書き、unit で配置、move で計算)。
    let stdin = io::stdin();
    let mut map: Option<MapData> = None;
    let mut placed: Vec<PlacedUnit> = Vec::new();
    // 出力 (@move ごとのブロック) をバッファに溜め、最後にまとめて書く。
    let mut out_blocks: Vec<String> = Vec::new();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim_end().to_string();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // 行末コメント (`# ...`) を剥がす (corpus 可読性のため)。先頭が `@cell` 等で
        // トークン途中に `#` は来ない想定なので、空白に続く `#` 以降を捨てる。
        let line = match line.find(" #") {
            Some(i) => line[..i].trim_end().to_string(),
            None => line,
        };

        if let Some(rest) = line.strip_prefix("@map ") {
            let f: Vec<&str> = rest.split_whitespace().collect();
            if f.len() >= 2 {
                if let (Ok(w), Ok(h)) = (f[0].parse::<u32>(), f[1].parse::<u32>()) {
                    map = Some(MapData::new(w, h)); // 全セル terrain_id 0 (平地)
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("@cell ") {
            let f: Vec<&str> = rest.split_whitespace().collect();
            if f.len() >= 3 {
                if let (Ok(x), Ok(y), Ok(tid)) = (
                    f[0].parse::<u32>(),
                    f[1].parse::<u32>(),
                    f[2].parse::<u32>(),
                ) {
                    if let Some(m) = map.as_mut() {
                        m.set_cell(
                            x,
                            y,
                            MapCell {
                                terrain_id: tid,
                                bitmap_no: 0,
                            },
                        );
                    }
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("@unit ") {
            // `@unit <name> <rank> <party> <pilot> <level> <x> <y>`
            let f: Vec<&str> = rest.split_whitespace().collect();
            if f.len() >= 7 {
                let name = f[0].to_string();
                let x = f[5].parse::<u32>().unwrap_or(0);
                let y = f[6].parse::<u32>().unwrap_or(0);
                placed.push(PlacedUnit { name, pos: (x, y) });
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("@move ") {
            let name = rest.trim();
            out_blocks.push(eval_move(&app, map.as_ref(), &placed, name));
            continue;
        }
        // それ以外の行は無視。
    }

    eprintln!(
        "map={} placed={}",
        map.as_ref()
            .map(|m| format!("{}x{}", m.width, m.height))
            .unwrap_or_else(|| "none".to_string()),
        placed.len()
    );

    let stdout = io::stdout();
    let mut out = stdout.lock();
    for b in &out_blocks {
        let _ = write!(out, "{b}");
    }
}

/// `@move <name>` を 1 件評価し、ヘッダ + 到達マス行のブロック文字列を返す。
fn eval_move(app: &App, map: Option<&MapData>, placed: &[PlacedUnit], name: &str) -> String {
    let mut s = format!("=== move {name} ===\n");
    let Some(map) = map else {
        s.push_str("<ERR:nomap>\n");
        return s;
    };
    let Some(pu) = placed.iter().find(|p| p.name == name) else {
        s.push_str("<ERR:nounit>\n");
        return s;
    };
    let db = app.database();
    let Some(unit_data) = db.unit_by_name(name) else {
        s.push_str("<ERR:nodata>\n");
        return s;
    };

    // 移動範囲計算用のコストクロージャ。新規ユニット (実体未生成) なので
    // current_area=空 (transportation から推定)、active_features=空、地形適応=空。
    // app.rs:4496-4528 の組み立てを fresh-unit 向けに簡約したもの。
    let cost_fn = src_core::movement::make_unit_cost_fn(
        db.terrains.clone(),
        unit_data.transportation.clone(),
        unit_data.adaption.0,
        String::new(),
        Vec::new(),
        Vec::new(),
    );
    let speed = unit_data.speed;

    // 診断: 速度・移動属性を stderr へ。
    eprintln!(
        "move {name}: speed={speed} transportation={} adaption={:?} start=({},{})",
        unit_data.transportation,
        String::from_utf8_lossy(&unit_data.adaption.0),
        pu.pos.0,
        pu.pos.1,
    );

    let reachable = src_core::movement::compute_range_with(map, pu.pos, speed, cost_fn);

    // 共通正規化: `<x> <y> <cost2x>` を (x,y) 昇順で。cost2x = (max_mp - remaining) * 2。
    let mut rows: Vec<(u32, u32, i32)> = reachable
        .iter()
        .map(|(&(x, y), &rem)| (x, y, (speed - rem) * 2))
        .collect();
    rows.sort_by_key(|a| (a.0, a.1));
    for (x, y, c) in rows {
        s.push_str(&format!("{x} {y} {c}\n"));
    }
    s
}

/// C# `SRC.LoadDataDirectory` と同じファイル群を同順でロードする
/// (oracle_loaddata.rs と同一。terrain.txt は `<dir>/../system/terrain.txt`)。
fn load_data_directory(app: &mut App, dir: &Path) {
    if let Some(txt) = read_data(dir, "mind.txt").or_else(|| read_data(dir, "sp.txt")) {
        let (sps, _) = special_power::parse_lenient(&txt);
        app.database_mut().extend_special_powers(sps);
    }
    if let Some(txt) = read_data(dir, "pilot.txt") {
        let (pilots, _) = pilot::parse_lenient(&txt);
        app.database_mut().extend_pilots(pilots);
    }
    if let Some(txt) = read_data(dir, "robot.txt") {
        let (units, _) = unit::parse_lenient(&txt);
        app.database_mut().extend_units(units);
    }
    if let Some(txt) = read_data(dir, "unit.txt") {
        let (units, _) = unit::parse_lenient(&txt);
        app.database_mut().extend_units(units);
    }
    if let Some(txt) = read_data(dir, "item.txt") {
        let (items, _) = item::parse_lenient(&txt);
        app.database_mut().extend_items(items);
    }
    if let Some(txt) = read_data(&dir.join("..").join("system"), "terrain.txt") {
        let (terrains, _) = terrain_file::parse_lenient(&txt);
        app.database_mut().extend_terrains(terrains);
    }

    eprintln!(
        "loaded: pilots={} units={} items={} sp={} terrains={}",
        app.database().pilots.len(),
        app.database().units.len(),
        app.database().items.len(),
        app.database().special_powers.len(),
        app.database().terrains.len(),
    );
}

fn read_data(dir: &Path, name: &str) -> Option<String> {
    let path = dir.join(name);
    match std::fs::read(&path) {
        Ok(bytes) => Some(loader::decode_text(&bytes)),
        Err(_) => None,
    }
}
