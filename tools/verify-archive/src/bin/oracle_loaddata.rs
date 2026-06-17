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
//! `@unit <name> <rank> <party> [<pilot> <level>]` 行はユニット実体の生成指令
//! (C# `placeunit` モードの `UList.Add(name,rank,party)` (+ `PList.Add(pilot,level,party)`+Ride)
//! と対応)。Rust は `Create <party> <name> <rank> <pilot> <level> <x> 1` で生成する
//! (Create は rank=改造段階・level=初期レベルを反映。pilot/level 省略時は無人 `-`・level 0)。
//! 生成後 `Info(ユニット|パイロット, <name>, …)` を probe できる。座標は衝突回避のため指令順。
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

    // stdin を読み、`@unit` 生成指令・`@predict` 戦闘予測指令・probe に分ける。
    let stdin = io::stdin();
    let mut probes: Vec<String> = Vec::new();
    let mut creates: Vec<String> = Vec::new();
    let mut predicts: Vec<String> = Vec::new();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("@predict ") {
            // `@predict <attacker> <defender> <weapon_index(1-based)> <field>`
            // C# `placeattack` と対応。生成後にまとめて評価する。
            predicts.push(rest.to_string());
            continue;
        }
        if let Some(rest) = line.strip_prefix("@unit ") {
            // `@unit <name> <rank> <party>` (無人) / `@unit <name> <rank> <party> <pilot> <level>`
            // (有人) → C# `UList.Add(name,rank,party)` (+ `PList.Add(pilot,level,party)`+Ride) と対応。
            // Rust は `Create <party> <name> <rank> <pilot> <level> <x> 1` で生成する
            // (Create は rank=改造段階・level=初期レベルを反映する。Place は rank/level 引数を
            // 持たないため Create を使う)。座標は指令順 (衝突回避)。
            let f: Vec<&str> = rest.split_whitespace().collect();
            if f.len() >= 3 {
                let (name, rank, party) = (f[0], f[1], f[2]);
                let (pilot, level) = if f.len() >= 5 {
                    (f[3], f[4])
                } else {
                    ("-", "0")
                };
                let x = creates.len() + 1;
                creates.push(format!(
                    "Create {party} {name} {rank} {pilot} {level} {x} 1"
                ));
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
    // 戦闘予測 `@predict` を入力順に評価して 1 行ずつ出力する
    // (C# `placeattack` モードと対応)。
    for pr in &predicts {
        let _ = writeln!(out, "{}", eval_predict(&app, pr));
    }
}

/// `@predict <attacker> <defender> <weapon_index(1-based)> <field>` を 1 件評価する。
/// 攻撃側/防御側はユニットデータ名で `unit_instances` を引き、effective なコンバットデータ
/// (レベル成長 + 改造 + ボーナス込み) で `predict_with_status_terrain` を中立条件で呼ぶ。
/// field: 命中率 → hit_chance / ダメージ → damage / クリティカル率 → critical_chance。
/// 引き当て失敗時は `<ERR:lookup>` (武器インデックス不正は `<ERR:weapon>`)。
fn eval_predict(app: &App, pr: &str) -> String {
    let f: Vec<&str> = pr.split_whitespace().collect();
    if f.len() < 4 {
        return "<ERR:args>".to_string();
    }
    let (aname, dname, widx_s, field) = (f[0], f[1], f[2], f[3]);
    let Ok(widx) = widx_s.parse::<usize>() else {
        return "<ERR:args>".to_string();
    };
    if widx == 0 {
        return "<ERR:weapon>".to_string();
    }
    let db = app.database();
    let Some(atk_idx) = db
        .unit_instances
        .iter()
        .position(|u| u.unit_data_name == aname)
    else {
        return "<ERR:lookup>".to_string();
    };
    let Some(def_idx) = db
        .unit_instances
        .iter()
        .position(|u| u.unit_data_name == dname)
    else {
        return "<ERR:lookup>".to_string();
    };
    let Some((atk_pilot, atk_unit)) = db.effective_combat_data(atk_idx) else {
        return "<ERR:lookup>".to_string();
    };
    let Some((def_pilot, def_unit)) = db.effective_combat_data(def_idx) else {
        return "<ERR:lookup>".to_string();
    };
    let Some(weapon) = atk_unit.weapons.get(widx - 1) else {
        return "<ERR:weapon>".to_string();
    };
    // 中立条件: 地形 hit_mod=0 / damage_mod=0、士気 100/100、状態異常なし、env -1/-1 (適応 ×1.0)。
    let preview = src_core::combat::predict_with_status_terrain(
        &atk_pilot,
        &atk_unit,
        weapon,
        &def_pilot,
        &def_unit,
        0,
        0,
        100,
        100,
        &[],
        &[],
        -1,
        -1,
    );
    match field {
        "命中率" => preview.hit_chance.to_string(),
        "ダメージ" => preview.damage.to_string(),
        "クリティカル率" => preview.critical_chance.to_string(),
        _ => "<ERR:field>".to_string(),
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
