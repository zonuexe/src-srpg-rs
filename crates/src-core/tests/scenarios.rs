//! E2E シナリオテスト driver / End-to-end scenario test driver.
//!
//! `tests/fixtures/scenarios/<name>.eve` を実行し、`.expected` と比較する。
//! 横に `.steps` があればドライバ入力として食わせる。
//!
//! 環境変数:
//!
//! - `SRC_UPDATE_SNAPSHOTS=1` … `.expected` を actual で上書き
//! - `SRC_SCENARIO=<name>` … 指定したシナリオだけ実行
//!
//! ## 新規シナリオの追加手順
//!
//! 1. `tests/fixtures/scenarios/<name>.eve` を作成
//! 2. (任意) `.steps` にドライバ入力を 1 行 1 ステップで列挙
//! 3. `SRC_UPDATE_SNAPSHOTS=1 cargo test -p src-core --test scenarios <name>` で
//!    `.expected` を自動生成
//! 4. 内容を目視確認してコミット

use std::fs;
use std::path::{Path, PathBuf};

use src_core::test_harness::{parse_steps, Harness};

fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("scenarios")
}

fn collect_scenarios() -> Vec<String> {
    let dir = fixtures_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let only = std::env::var("SRC_SCENARIO").ok();
    let mut names: Vec<String> = entries
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let p = e.path();
            if p.extension().and_then(|s| s.to_str()) != Some("eve") {
                return None;
            }
            let stem = p.file_stem()?.to_str()?.to_string();
            if let Some(needle) = &only {
                if !stem.contains(needle) {
                    return None;
                }
            }
            Some(stem)
        })
        .collect();
    names.sort();
    names
}

fn run_scenario(name: &str) -> Result<(), String> {
    let dir = fixtures_dir();
    let eve_path = dir.join(format!("{name}.eve"));
    let steps_path = dir.join(format!("{name}.steps"));
    let expected_path = dir.join(format!("{name}.expected"));

    let eve_src = fs::read_to_string(&eve_path)
        .map_err(|e| format!("{name}: read {} failed: {e}", eve_path.display()))?;
    let steps = if steps_path.exists() {
        let txt = fs::read_to_string(&steps_path)
            .map_err(|e| format!("{name}: read {} failed: {e}", steps_path.display()))?;
        parse_steps(&txt).map_err(|m| format!("{name}: steps parse failed: {m}"))?
    } else {
        Vec::new()
    };

    let mut h = Harness::from_eve_source(&eve_src)
        .map_err(|e| format!("{name}: eve execute failed: {e}"))?;
    let outcome = h
        .drive(&steps)
        .map_err(|e| format!("{name}: drive failed: {e}"))?;

    let mut snapshot = h.snapshot();
    // outcome をスナップショット末尾に追記して「途中で詰まった」を可視化する。
    snapshot.push_str("\n== outcome ==\n");
    snapshot.push_str(&format!("{outcome:?}\n"));

    let update = std::env::var("SRC_UPDATE_SNAPSHOTS")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false);

    if update || !expected_path.exists() {
        fs::write(&expected_path, &snapshot)
            .map_err(|e| format!("{name}: write expected failed: {e}"))?;
        eprintln!("[snapshot updated] {}", expected_path.display());
        return Ok(());
    }

    let expected = fs::read_to_string(&expected_path)
        .map_err(|e| format!("{name}: read expected failed: {e}"))?;

    if expected == snapshot {
        return Ok(());
    }

    Err(format!(
        "{name}: snapshot mismatch\n--- expected ({}) ---\n{}\n--- actual ---\n{}\n--- diff (first 10 differing lines) ---\n{}",
        expected_path.display(),
        expected,
        snapshot,
        first_diff_lines(&expected, &snapshot, 10),
    ))
}

fn first_diff_lines(a: &str, b: &str, limit: usize) -> String {
    let mut out = String::new();
    let mut shown = 0;
    let av: Vec<&str> = a.lines().collect();
    let bv: Vec<&str> = b.lines().collect();
    let max = av.len().max(bv.len());
    for i in 0..max {
        let ai = av.get(i).copied().unwrap_or("<EOF>");
        let bi = bv.get(i).copied().unwrap_or("<EOF>");
        if ai != bi {
            out.push_str(&format!("@{}\n  -{}\n  +{}\n", i + 1, ai, bi));
            shown += 1;
            if shown >= limit {
                break;
            }
        }
    }
    out
}

#[test]
fn all_scenarios() {
    let names = collect_scenarios();
    if names.is_empty() {
        panic!(
            "シナリオ fixture が見つかりません: {}",
            fixtures_dir().display()
        );
    }
    let mut failures: Vec<String> = Vec::new();
    for name in &names {
        if let Err(msg) = run_scenario(name) {
            failures.push(msg);
        }
    }
    if !failures.is_empty() {
        panic!(
            "{} / {} シナリオが失敗:\n\n{}",
            failures.len(),
            names.len(),
            failures.join("\n\n========\n\n")
        );
    }
}
