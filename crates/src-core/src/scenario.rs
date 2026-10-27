//! シナリオの取り込みと起動 (原典 `SRC.bas` の `LoadData` / `StartScenario` 相当)。
//!
//! 展開済みのエントリ群 (アーカイブ内パス, 中身) から、データファイル (`pilot.txt` 等) を
//! DB に、`.ini` / `.eve` をスクリプトライブラリに取り込み、エントリポイントの `.eve` を
//! 実行してステージを起動する。画像・音声の登録はフロントエンドの責務。
//!
//! 入口は [`load_scenario`]。検証ツールのようにエントリポイントを差し替えたい呼び出し側は
//! [`register_scenario`] → [`run_entry`] → [`boot_after_load`] を順に呼ぶ。

use std::collections::HashMap;

use crate::data::{event, item, loader, map as mapdata, pilot, special_power, terrain_file, unit};
use crate::{entrypoint, event_runtime, App};

/// データファイルの種類。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DataKind {
    Pilot,
    NonPilot,
    Unit,
    Item,
    SpecialPower,
    Terrain,
    Animation,
    Map,
}

impl DataKind {
    /// basename (小文字) から種類を決める。データファイルでなければ `None`。
    pub fn of(lower_basename: &str) -> Option<Self> {
        Some(match lower_basename {
            "pilot.txt" => Self::Pilot,
            "non_pilot.txt" => Self::NonPilot,
            "unit.txt" | "robot.txt" => Self::Unit,
            "item.txt" => Self::Item,
            "sp.txt" | "mind.txt" => Self::SpecialPower,
            "terrain.txt" => Self::Terrain,
            "animation.txt" | "ext_animation.txt" => Self::Animation,
            n if n.ends_with(".map") => Self::Map,
            _ => return None,
        })
    }

    /// ログ表示用の名前。
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pilot => "pilot",
            Self::NonPilot => "non_pilot",
            Self::Unit => "unit",
            Self::Item => "item",
            Self::SpecialPower => "special_power",
            Self::Terrain => "terrain",
            Self::Animation => "戦闘アニメデータ",
            Self::Map => "map",
        }
    }
}

/// データファイル 1 件の取り込み結果。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataIngest {
    pub kind: DataKind,
    /// 取り込んだレコード数 (戦闘アニメは取り込み後の対象数、マップは 1)。
    pub count: usize,
    /// 読み飛ばしたレコードのエラー (レコード単位で寛容に解析する)。
    pub errors: Vec<String>,
}

impl DataIngest {
    /// 取り込みログの 1 行。
    pub fn log_line(&self, name: &str) -> String {
        let label = self.kind.label();
        match self.errors.first() {
            None => format!("  ✓ {name} を {label} として取り込み ({} 件)", self.count),
            Some(e) => format!(
                "  ⚠ {name} を {label} として取り込み ({} 件, {} 件をスキップ): {e}",
                self.count,
                self.errors.len()
            ),
        }
    }
}

/// `name` の basename がデータファイルなら `text` を解析して DB に加法マージする。
/// データファイルでなければ `None`。1 ファイルの解析失敗でシナリオ全体を止めない
/// (原典 SRC.NET も DataErrorMessage を出して続行する)。
pub fn ingest_data_file(app: &mut App, name: &str, text: &str) -> Option<DataIngest> {
    let lower = name.to_ascii_lowercase();
    let base = basename(&lower);
    let kind = DataKind::of(base)?;
    fn errs<E: std::fmt::Display>(errors: Vec<E>) -> Vec<String> {
        errors.iter().map(ToString::to_string).collect()
    }
    let db = app.database_mut();
    let (count, errors) = match kind {
        DataKind::Pilot => {
            let (v, e) = pilot::parse_lenient(text);
            let n = v.len();
            db.extend_pilots(v);
            (n, errs(e))
        }
        DataKind::NonPilot => {
            // 原典 `NonPilotDataList.Load` の最小読込 (Talk の話者名として引けるように
            // 戦闘ステータス 0 のパイロットとして登録する)。
            let v = crate::data::non_pilot::parse(text);
            let n = v.len();
            db.extend_pilots(v);
            (n, Vec::new())
        }
        DataKind::Unit => {
            let (v, e) = unit::parse_lenient(text);
            let n = v.len();
            db.extend_units(v);
            (n, errs(e))
        }
        DataKind::Item => {
            let (v, e) = item::parse_lenient(text);
            let n = v.len();
            db.extend_items(v);
            (n, errs(e))
        }
        DataKind::SpecialPower => {
            let (v, e) = special_power::parse_lenient(text);
            let n = v.len();
            db.extend_special_powers(v);
            (n, errs(e))
        }
        DataKind::Terrain => {
            let (v, e) = terrain_file::parse_lenient(text);
            let n = v.len();
            db.extend_terrains(v);
            (n, errs(e))
        }
        DataKind::Animation => {
            db.merge_animation_data(text);
            (db.catalog.animation.entries.len(), Vec::new())
        }
        DataKind::Map => match mapdata::parse(text) {
            // `ChangeMap` で参照されるので basename で保存する。
            Ok(m) => {
                db.store_map(base.to_string(), m);
                (1, Vec::new())
            }
            Err(e) => (0, vec![e.to_string()]),
        },
    };
    Some(DataIngest {
        kind,
        count,
        errors,
    })
}

/// スクリプトライブラリに登録した `.eve` 1 本。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EveEntry {
    pub name: String,
    /// ライブラリ内の先頭 pc。
    pub pc: usize,
    /// 文の数。
    pub len: usize,
}

/// [`register_scenario`] の結果。
#[derive(Debug, Clone, Default)]
pub struct Registered {
    /// 取り込みログ (1 行ずつ改行区切り)。
    pub log: String,
    /// エントリポイントと判定した `.eve` (実行可能なものが無ければ `None`)。
    pub entry: Option<String>,
    /// 登録した `.eve` (エントリポイントのスコア順)。
    pub eves: Vec<EveEntry>,
    /// 解析に失敗した `.eve` の数。
    pub eve_parse_errors: usize,
}

/// データファイルを DB に、`.ini` / `.eve` をスクリプトライブラリに取り込む (実行はしない)。
///
/// - 原典はシナリオデータを `Data\` 配下からのみ読む (`SRC.bas::SearchDataFolder`)。
///   `Data/` を持つアーカイブでは、その外側の同名ファイルをデータとして読まない。
/// - `.ini` (`Require` で参照する設定ファイル) は登録だけする。
/// - `.eve` は全文を登録してからエントリポイントだけを実行する (ファイルを跨ぐラベル参照を
///   先に解決するため。サブ `.eve` は `Continue` 等で必要になったときに実行される)。
pub fn register_scenario(app: &mut App, entries: &[(String, Vec<u8>)]) -> Registered {
    let mut reg = Registered::default();
    let mut log = |line: String| {
        reg.log.push_str(&line);
        reg.log.push('\n');
    };
    let data_scope = loader::scope_to_data_dir(entries.iter().map(|(n, _)| n.as_str()));
    let mut eves: Vec<(&str, String)> = Vec::new();
    let mut inis: Vec<(&str, String)> = Vec::new();
    for (name, data) in entries {
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".eve") {
            eves.push((name, loader::decode_text(data)));
        } else if lower.ends_with(".ini") {
            inis.push((name, loader::decode_text(data)));
        } else if DataKind::of(basename(&lower)).is_some() {
            if loader::is_data_file_name(&lower) && !loader::accept_data_entry(name, data_scope) {
                continue;
            }
            if let Some(r) = ingest_data_file(app, name, &loader::decode_text(data)) {
                log(r.log_line(name));
            }
        }
    }

    // エントリポイント (README の起動指示 / 開始系の命名 / 階層 / `@識別子` のスコア) の
    // 高い順、同点は浅いパスを先に並べる。
    let analysis = entrypoint::analyze(entries);
    if let Some(best) = analysis.best_runnable() {
        log(format!(
            "  → エントリーポイント検出: {best} (候補 {} 件)",
            analysis.candidates.len()
        ));
    } else if analysis.data_only {
        log("  → データ専用アーカイブ (実行可能ファイルなし)".to_string());
    }
    let score: HashMap<&str, i32> = analysis
        .candidates
        .iter()
        .map(|c| (c.name.as_str(), c.score))
        .collect();
    let depth = |n: &str| n.matches(['/', '\\']).count();
    eves.sort_by(|a, b| {
        let sa = score.get(a.0).copied().unwrap_or(i32::MIN);
        let sb = score.get(b.0).copied().unwrap_or(i32::MIN);
        sb.cmp(&sa).then_with(|| depth(a.0).cmp(&depth(b.0)))
    });

    for (name, txt) in &inis {
        match event::parse(txt) {
            Ok(stmts) => {
                app.script_library_mut().append_with_name(&stmts, name);
                log(format!("  ✓ {name} を設定ファイルとして登録"));
            }
            Err(e) => log(format!("  ⚠ {name} パースエラー: {e}")),
        }
    }
    let mut registered = Vec::new();
    let mut parse_errors = 0;
    for (name, txt) in &eves {
        match event::parse(txt) {
            Ok(stmts) => {
                let pc = app.script_library().statements.len();
                app.script_library_mut().append_with_name(&stmts, name);
                registered.push(EveEntry {
                    name: name.to_string(),
                    pc,
                    len: stmts.len(),
                });
            }
            Err(e) => {
                parse_errors += 1;
                log(format!("  ⚠ {name} パースエラー: {e}"));
            }
        }
    }
    reg.entry = analysis.best_runnable().map(String::from);
    reg.eves = registered;
    reg.eve_parse_errors = parse_errors;
    reg
}

/// 登録済みの `.eve` のうち `entry` だけを実行する。ログを返す。
pub fn run_entry(app: &mut App, reg: &Registered, entry: &str) -> String {
    let mut log = String::new();
    for e in &reg.eves {
        if e.name == entry {
            match event_runtime::run_from_pc(app, e.pc) {
                Ok(()) => log.push_str(&format!(
                    "  ✓ {} を .eve として実行 ({} statements)\n",
                    e.name, e.len
                )),
                Err(err) => log.push_str(&format!("  ⚠ {} 実行エラー: {err}\n", e.name)),
            }
        } else {
            log.push_str(&format!(
                "  ✓ {} を .eve として登録 ({} statements)\n",
                e.name, e.len
            ));
        }
    }
    log
}

/// エントリポイント実行後の起動処理 (原典 `StartScenario` の後半)。ログを返す。
///
/// 1. `IntermissionCommand` を登録したシナリオは、`Continue` チェインを進めずに
///    インターミッション画面で止める (本編より先にキャラメイキング等を通すため)。
/// 2. `Continue` で予約された次ステージへ進める (対話等で止まったら打ち切り、最大 8 段)。
/// 3. `Stage` も `Continue` も使わないシナリオは、エントリ `.eve` をステージとして
///    `スタート` 発火 → 味方フェイズまで進める。
/// 4. シナリオが描画・対話を始めていればマップ画面に切り替える。
pub fn boot_after_load(app: &mut App, entry: Option<&str>) -> String {
    let mut log = String::new();
    if !app.intermission_commands().is_empty() {
        app.set_scene(crate::Scene::Intermission);
        app.set_intermission_cursor(0);
        log.push_str(&format!(
            "  → IntermissionCommand {} 件登録: インターミッション画面で停止\n",
            app.intermission_commands().len()
        ));
        return log;
    }
    let mut steps = 0;
    while steps < 8 && !app.modal_gate().is_blocked() && app.advance_to_next_stage() {
        steps += 1;
    }
    if steps > 0 {
        log.push_str(&format!("  → Continue チェインで {steps} 段進行\n"));
    }
    if let Some(entry) = entry {
        app.bootstrap_stage_after_load(entry);
        log.push_str(&format!("  → ステージブートストラップ: {entry}\n"));
    }
    // チェイン / ブートストラップが積んだ継続を今すぐ流す (対話で中断中なら再開後に流れる)。
    app.on_script_completed();
    let has_visuals = !app.script_overlay().cmds.is_empty()
        || !app.hotpoints().is_empty()
        || app.pending_dialog().is_some();
    if has_visuals && app.scene() != crate::Scene::MapView {
        app.set_scene(crate::Scene::MapView);
    }
    log
}

/// 展開済みのエントリ群をシナリオとして取り込む。`full_load` が偽なら登録だけ行う
/// (素材パックを下地として再適用する用途。ライブラリ `.eve` を実行すると後続シナリオの
/// スクリプト文脈を壊すため)。ログを返す。
pub fn load_scenario(app: &mut App, entries: &[(String, Vec<u8>)], full_load: bool) -> String {
    let reg = register_scenario(app, entries);
    let mut log = reg.log.clone();
    if !full_load {
        return log;
    }
    if let Some(entry) = reg.entry.as_deref() {
        log.push_str(&run_entry(app, &reg, entry));
    }
    log.push_str(&boot_after_load(app, reg.entry.as_deref()));
    log
}

/// 末尾のパス要素 (`/` / `\` 区切り)。
fn basename(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lone_eve_is_run_and_data_files_are_ingested() {
        let mut app = App::new();
        let entries = vec![
            (
                "Data/pilot.txt".to_string(),
                "テスト\nテスト, てすと, 男性, 汎用, AAAA, 100\n特殊能力なし\n100, 100, 100, 100, 100, 100, 強気\nSP, 50\n-, -, -\n".as_bytes().to_vec(),
            ),
            (
                "demo.eve".to_string(),
                "Stage \"テスト\"\nMapSize 2 2\n".as_bytes().to_vec(),
            ),
        ];
        let log = load_scenario(&mut app, &entries, true);
        assert!(log.contains("demo.eve"), "{log}");
        assert_eq!(app.stage(), "テスト");
        assert!(app.database().map.is_some());
    }

    #[test]
    fn data_outside_data_dir_is_ignored() {
        let mut app = App::new();
        let entries = vec![
            ("Data/item.txt".to_string(), Vec::new()),
            ("Lib/pilot.txt".to_string(), b"broken".to_vec()),
        ];
        let reg = register_scenario(&mut app, &entries);
        assert!(!reg.log.contains("Lib/pilot.txt"), "{}", reg.log);
    }
}
