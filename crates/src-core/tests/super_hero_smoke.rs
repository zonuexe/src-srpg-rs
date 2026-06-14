//! `スーパーヒーロー伝説.zip` のシナリオ起動スモークテスト。
//!
//! 著作権配慮: fixture を **参照のみ** で embed しない。zip 内のデータを
//! デコード → DB 構築 → entry-point .eve 実行までネイティブ側で通し、
//! 「App::Scene が MapView に遷移」「`.eve` パース 0 失敗」「DB に pilot/unit
//! が登録されている」までを assert する。
//!
//! 同等の検証は `tools/verify-archive` の `VERIFY_SMOKE=1` でも手動実行可能。
//! こちらは CI でも回るよう cargo test に組み込む。

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use src_core::data::{
    event, item, loader, map as mapdata, pilot, special_power, terrain_file, unit,
};
use src_core::{entrypoint, event_runtime, App};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("src-web/tests/fixtures/スーパーヒーロー伝説.zip")
}

fn unzip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = loader::decode_text(entry.name_raw());
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        out.push((name, buf));
    }
    Ok(out)
}

fn basename(lname: &str) -> String {
    lname
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(lname)
        .to_string()
}

#[test]
fn super_hero_zip_opens_and_starts_scenario() {
    let path = fixture_path();
    let Ok(bytes) = std::fs::read(&path) else {
        eprintln!("[skip] fixture not found: {}", path.display());
        return;
    };

    let entries = unzip(&bytes).expect("unzip failed");
    assert!(!entries.is_empty(), "アーカイブが空");

    let mut app = App::with_rng_seed(0xCAFE_BABE);
    let mut deferred_eves: Vec<(String, String)> = Vec::new();

    for (name, data) in &entries {
        let lname = name.to_ascii_lowercase();
        let base = basename(&lname);
        let txt = loader::decode_text(data);
        if lname.ends_with(".eve") {
            deferred_eves.push((name.clone(), txt));
        } else if base == "pilot.txt" {
            let v = pilot::parse(&txt).unwrap_or_else(|e| panic!("{name}: {e}"));
            app.database_mut().extend_pilots(v);
        } else if base == "unit.txt" || base == "robot.txt" {
            let v = unit::parse(&txt).unwrap_or_else(|e| panic!("{name}: {e}"));
            app.database_mut().extend_units(v);
        } else if base == "item.txt" {
            let v = item::parse(&txt).unwrap_or_else(|e| panic!("{name}: {e}"));
            app.database_mut().extend_items(v);
        } else if base == "sp.txt" || base == "mind.txt" {
            let v = special_power::parse(&txt).unwrap_or_else(|e| panic!("{name}: {e}"));
            app.database_mut().extend_special_powers(v);
        } else if base == "terrain.txt" {
            let v = terrain_file::parse(&txt).unwrap_or_else(|e| panic!("{name}: {e}"));
            app.database_mut().extend_terrains(v);
        } else if lname.ends_with(".map") {
            if let Ok(m) = mapdata::parse(&txt) {
                app.database_mut().store_map(base, m);
            }
        }
    }

    // entry-point スコア順に並べ替えてから .eve をロード + 実行。
    let analysis = entrypoint::analyze(&entries);
    let best = analysis
        .best()
        .expect("entry-point が見つからない")
        .to_string();
    let ep_score: std::collections::HashMap<&str, i32> = analysis
        .candidates
        .iter()
        .map(|c| (c.name.as_str(), c.score))
        .collect();
    deferred_eves.sort_by(|a, b| {
        let sa = ep_score.get(a.0.as_str()).copied().unwrap_or(i32::MIN);
        let sb = ep_score.get(b.0.as_str()).copied().unwrap_or(i32::MIN);
        sb.cmp(&sa).then_with(|| {
            a.0.matches(['/', '\\'])
                .count()
                .cmp(&b.0.matches(['/', '\\']).count())
        })
    });

    let mut eve_entries: Vec<(String, usize)> = Vec::new();
    let mut parse_errors = Vec::<String>::new();
    for (name, txt) in &deferred_eves {
        match event::parse(txt) {
            Ok(stmts) => {
                let pc = app.script_library().statements.len();
                app.script_library_mut().append_with_name(&stmts, name);
                eve_entries.push((name.clone(), pc));
            }
            Err(e) => parse_errors.push(format!("{name}: {e}")),
        }
    }
    assert!(
        parse_errors.is_empty(),
        ".eve パースエラー: {parse_errors:#?}"
    );

    let mut runtime_errors = Vec::<String>::new();
    for (name, pc) in &eve_entries {
        if let Err(e) = event_runtime::run_from_pc(&mut app, *pc) {
            runtime_errors.push(format!("{name}: {e}"));
        }
    }
    assert!(
        runtime_errors.is_empty(),
        ".eve 実行エラー: {runtime_errors:#?}"
    );

    // ---- ここから最終状態の検証 ----
    // entry-point は Chapter0a.eve であることを保証 (entrypoint::analyze の
    // 結果がこの fixture でブレるとシナリオ起動順が変わる)。
    assert!(
        best.ends_with("Chapter0a.eve"),
        "entry-point が想定外: {best}"
    );

    // .eve 実行後、Title から MapView に遷移していること。
    // (スーパーヒーロー伝説 の opening .eve が Briefing 状態を確立する)
    assert_eq!(
        app.scene(),
        src_core::Scene::MapView,
        "scene が MapView に遷移していない (現在: {:?})",
        app.scene()
    );

    // データベースに pilot/unit/item が取り込まれていること。
    assert!(
        app.database().pilots.len() >= 200,
        "pilots が少なすぎ ({})",
        app.database().pilots.len()
    );
    assert!(
        app.database().units.len() >= 180,
        "units が少なすぎ ({})",
        app.database().units.len()
    );

    eprintln!(
        "OK: entries={}, pilots={}, units={}, .eve={}, scene={:?}, stage_state={:?}",
        entries.len(),
        app.database().pilots.len(),
        app.database().units.len(),
        eve_entries.len(),
        app.scene(),
        app.stage_state()
    );
}
