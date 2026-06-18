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

use src_core::data::{item, loader, pilot, special_power, terrain_file, unit};
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
    // predicts は (predict 引数, 地形 id, 気力スナップショット, 精神名スナップショット) の組で保持する。
    // `@terrain <id>` 以降の `@predict` に id が付き、評価時に防御側地形として用いる
    // (id<0 は中立: hit_mod=0/damage_mod=0/env=1)。C# `placeattack` の地形紐づけと対応。
    // 気力/精神も corpus 順に累積し、各 `@predict` を読んだ時点でスナップショットして紐づける
    // (C# 側が予測をまとめて末尾評価するため state を凍結する遅延適用方式と一致させる)。
    type MoraleMap = std::collections::BTreeMap<String, i32>;
    type SpiritMap = std::collections::BTreeMap<String, Vec<String>>;
    let mut predicts: Vec<(String, i64, MoraleMap, SpiritMap)> = Vec::new();
    let mut cur_terrain: i64 = -1; // -1 = 中立
                                   // ユニットデータ名 → 気力 (既定 100) / アクティブ精神名リスト。corpus 順に更新する。
    let mut morale_map: MoraleMap = MoraleMap::new();
    let mut spirit_map: SpiritMap = SpiritMap::new();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("@terrain ") {
            // `@terrain <id>` → 以降の `@predict` で防御側に敷く地形 id を設定。
            // 解析失敗時は中立 (-1) に戻す。
            cur_terrain = rest.trim().parse::<i64>().unwrap_or(-1);
            continue;
        }
        if let Some(rest) = line.strip_prefix("@morale ") {
            // `@morale <unitName> <value>` → 以降の `@predict` でそのユニットの気力を value にする
            // (corpus 順に累積)。C# は SetMorale で [50,150] にクランプするが、Rust 側はクランプ
            // しない (combat::predict は受け取った気力をそのまま乗じる) — 50..150 の範囲で使えば一致。
            let f: Vec<&str> = rest.split_whitespace().collect();
            if f.len() >= 2 {
                if let Ok(mv) = f[1].parse::<i32>() {
                    morale_map.insert(f[0].to_string(), mv);
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("@spirit ") {
            // `@spirit <unitName> <spiritName>` → 以降の `@predict` でそのユニットに精神コマンドを
            // アクティブにする (status slice へ名前を追加)。1 ユニットに複数指定可。同名は冪等。
            // 与・被ダメージ修正 (ダメージ増加/被ダメージ増加/ダメージ低下/被ダメージ低下) は
            // `eval_predict` が sp.txt から `db::damage_spirit_levels` (= `sp_effect_level` ×4) で
            // 解決する (C# `SpecialPowerEffectLevel(...)` 準拠: 最大値勝ち)。鉄壁/不屈 もこの
            // データ駆動 down-mod 経由 (鉄壁=被ダメージ低下Lv7.5→÷4 / 不屈=Lv9→×0.1)。
            // その他 (集中/ひらめき 等) は combat 側が名前ベースで解釈する。
            let f: Vec<&str> = rest.split_whitespace().collect();
            if f.len() >= 2 {
                let lst = spirit_map.entry(f[0].to_string()).or_default();
                if !lst.iter().any(|s| s == f[1]) {
                    lst.push(f[1].to_string());
                }
            }
            continue;
        }
        if let Some(rest) = line.strip_prefix("@predict ") {
            // `@predict <attacker> <defender> <weapon_index(1-based)> <field>`
            // C# `placeattack` と対応。生成後にまとめて評価する。地形 id と、その時点の
            // 気力/精神スナップショットを紐づける (後続の @morale/@spirit に影響されないよう複製)。
            predicts.push((
                rest.to_string(),
                cur_terrain,
                morale_map.clone(),
                spirit_map.clone(),
            ));
            continue;
        }
        if line.starts_with("@option ") {
            // `@option <name>` は C# `placeattack` でグローバルオプションを立てる指令
            // (例: `地形適応命中率修正` で C# 側の地形適応を ×1.0 へ強制)。Rust 側は
            // ダメージ予測で env=-1 (適応 ×1.0) を常に用い既に適応中立なので無視する
            // (Create にも probe にも変換しない)。
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
    // (C# `placeattack` モードと対応)。各 predict に紐づく地形 id・気力・精神を渡す。
    for (pr, terrain, morale, spirits) in &predicts {
        let _ = writeln!(out, "{}", eval_predict(&app, pr, *terrain, morale, spirits));
    }
}

/// `@predict <attacker> <defender> <weapon_index(1-based)> <field>` を 1 件評価する。
/// 攻撃側/防御側はユニットデータ名で `unit_instances` を引き、effective なコンバットデータ
/// (レベル成長 + 改造 + ボーナス込み) で `predict_with_status_terrain` を呼ぶ。
/// `terrain_id` >= 0 のとき防御側地形としてその地形の hit_mod/damage_mod/env を用いる
/// (C# 側で防御側セルに敷く地形と対応)。terrain_id < 0 は中立 (hit_mod=0/damage_mod=0/env=1)。
/// field: 命中率 → hit_chance / ダメージ → damage / クリティカル率 → critical_chance。
/// 引き当て失敗時は `<ERR:lookup>` (武器インデックス不正は `<ERR:weapon>`)。
fn eval_predict(
    app: &App,
    pr: &str,
    terrain_id: i64,
    morale: &std::collections::BTreeMap<String, i32>,
    spirits: &std::collections::BTreeMap<String, Vec<String>>,
) -> String {
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
    // 防御側地形の解決: terrain_id>=0 なら DB から hit_mod/damage_mod/env を引く。
    // env は地形クラスから算出 (combat::terrain_env; 陸=1)。terrain_id<0 (= @terrain 未指定) は
    // 中立 (hit_mod=0/damage_mod=0/env=1) で従来挙動を維持する。
    // C# は防御側セルに該当地形を敷き HitProbability/Damage が Map.Terrain(def) の HitMod/DamageMod を
    // live 参照する。攻撃側は常に中立 (env=1=陸) で双方の地形適応 (S=1.4/A=1.2/B=1.0/C=0.8/D=0.6) を揃える。
    let (def_hit_mod, def_damage_mod, def_env) = if terrain_id >= 0 {
        let id = terrain_id as u32;
        let env = match db.terrain_by_id(id) {
            Some(t) => src_core::combat::terrain_env(&t.class),
            None => 1,
        };
        (db.terrain_hit_mod(id), db.terrain_damage_mod(id), env)
    } else {
        (0, 0, 1)
    };
    // 気力 (未指定は 100) と精神名スナップショットを攻撃側/防御側それぞれに渡す。
    // C# は SetMorale で [50,150] にクランプするため corpus 側で範囲を守る。
    let atk_morale = morale.get(aname).copied().unwrap_or(100);
    let def_morale = morale.get(dname).copied().unwrap_or(100);
    let empty: Vec<String> = Vec::new();
    let atk_statuses = spirits.get(aname).unwrap_or(&empty).as_slice();
    let def_statuses = spirits.get(dname).unwrap_or(&empty).as_slice();
    // 与・被ダメージ修正 (ダメージ増加/被ダメージ増加/ダメージ低下/被ダメージ低下) を
    // シナリオ sp.txt から解決 (C# `Unit.SpecialPowerEffectLevel(...)` 相当)。攻撃側 4 種・
    // 防御側 4 種のうち該当タイプを攻撃側/防御側それぞれから引く。
    let dmg_levels = db.damage_spirit_levels(atk_statuses, def_statuses);
    let preview = src_core::combat::predict_with_status_terrain(
        &atk_pilot,
        &atk_unit,
        weapon,
        &def_pilot,
        &def_unit,
        def_hit_mod,
        def_damage_mod,
        atk_morale,
        def_morale,
        atk_statuses,
        def_statuses,
        1,
        def_env,
        dmg_levels,
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
    // sp.txt / mind.txt (C# は mind.txt 優先)。terrain.txt 同様、本フィクスチャでは
    // `<dir>/../system/sp.txt` に置かれるため (C# oracle `Program.cs` も同経路を明示ロード)、
    // シナリオ dir に無ければ system dir を見る。これを populate しないと `@spirit` 付与の
    // ダメージ増加効果レベルが解決できず既定テーブルへフォールバックしてしまう。
    let system_dir = dir.join("..").join("system");
    if let Some(txt) = read_data(dir, "mind.txt")
        .or_else(|| read_data(dir, "sp.txt"))
        .or_else(|| read_data(&system_dir, "mind.txt"))
        .or_else(|| read_data(&system_dir, "sp.txt"))
    {
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
    // terrain.txt は C# 同様シナリオ data dir には無く `<dir>/../system/terrain.txt` に置かれる。
    // C# は LoadDataDirectory では読まず `TDList.Load(<System>/terrain.txt)` で別途ロードする。
    // Rust も同パスを直接読み `extend_terrains` でデータベースへ取り込む。
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
