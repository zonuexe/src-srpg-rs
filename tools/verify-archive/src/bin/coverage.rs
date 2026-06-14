//! VB6 / SRC.Sharp 原典との対応カバレッジレポート / Command coverage report.
//!
//! `src_core::command_catalog::COMMAND_CATALOG` を、SRC.Sharp の
//! `SRCCore/CmdDatas/Commands/**/*Cmd.cs` ファイル名から抽出した
//! コマンド名集合と比較し、双方の差分を標準出力に出す。
//!
//! 使い方:
//!
//! ```sh
//! cargo run -p verify-archive --bin coverage
//! cargo run -p verify-archive --bin coverage --release  # 軽い処理なので release 不要
//! ```
//!
//! 出力フォーマット (例):
//!
//! ```text
//! == SRC.Sharp 側 (188 commands) ==
//! == 自カタログ (130 commands) ==
//! == 共通 (Implemented なし) ==
//! ...
//! == SRC.Sharp にあって自カタログに無い ==
//!   Arc, Array, Attack, ...
//! == 自カタログにあって SRC.Sharp に無い ==
//!   Damage, Heal, Restore, ...
//! ```
//!
//! VB6 ソース (`SRC.Sharp/SRC/SRC_20121125/`) の `CmdType` enum も併せて
//! スキャンするので、SRC.Sharp 未実装でも VB6 にだけある命令も把握できる。

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use src_core::command_catalog::{CommandKind, COMMAND_CATALOG};

const SRC_SHARP_CMD_DIR: &str = "SRC.Sharp/SRC.Sharp/SRCCore/CmdDatas/Commands";
const VB6_CMDTYPE_FILES: &[&str] = &[
    "SRC_20121125/Event.bas",
    "SRC.Sharp/SRC/SRC_20121125/Event.bas",
];

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR は tools/verify-archive を指す。
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.ancestors()
        .find(|p| p.join("Cargo.lock").exists())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| here.to_path_buf())
}

fn scan_src_sharp(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let dir = root.join(SRC_SHARP_CMD_DIR);
    if !dir.exists() {
        eprintln!(
            "[warning] SRC.Sharp 配下が見つかりません: {}\n  → submodule を update してください。\n  → ` git submodule update --init --recursive`",
            dir.display()
        );
        return out;
    }
    walk(&dir, &mut out);
    out
}

fn walk(dir: &Path, out: &mut BTreeSet<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
            if let Some(stem) = name.strip_suffix("Cmd.cs") {
                if !stem.is_empty() {
                    out.insert(stem.to_string());
                }
            }
        }
    }
}

fn scan_vb6_cmdtype(root: &Path) -> BTreeSet<String> {
    // CmdType enum の各 ident を抽出 (best-effort).
    let mut out = BTreeSet::new();
    for rel in VB6_CMDTYPE_FILES {
        let path = root.join(rel);
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        // 形式: `cmd_xxx = ...` (VB6) 又は CmdType_Xxx
        // Event.bas には `Select Case cmd.CmdName` の case 列がある可能性も。
        // ここでは "Case cmd_<Name>" の <Name> を拾うだけの簡易抽出。
        for line in text.lines() {
            let trimmed = line.trim();
            if let Some(rest) = trimmed.strip_prefix("Case ") {
                for tok in rest.split(',') {
                    let t = tok.trim();
                    let t = t.strip_prefix("cmd_").unwrap_or(t);
                    // 先頭が ASCII alpha で全部 [A-Za-z0-9_] のものだけ採用。
                    if t.is_empty()
                        || !t
                            .bytes()
                            .next()
                            .map(|b| b.is_ascii_alphabetic())
                            .unwrap_or(false)
                    {
                        continue;
                    }
                    if t.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
                        // 末尾 `Cmd` は enum サフィックスなので剥がす。
                        let trimmed = t.strip_suffix("Cmd").unwrap_or(t);
                        if !trimmed.is_empty() {
                            out.insert(trimmed.to_string());
                        }
                    }
                }
            }
        }
    }
    out
}

fn catalog_names() -> BTreeSet<String> {
    COMMAND_CATALOG.iter().map(|s| s.name.to_string()).collect()
}

/// case-insensitive 比較のため小文字に正規化したキーを返す。
fn lc(s: &str) -> String {
    s.to_ascii_lowercase()
}

fn main() -> ExitCode {
    let root = workspace_root();
    let src_sharp = scan_src_sharp(&root);
    let vb6 = scan_vb6_cmdtype(&root);
    let ours = catalog_names();

    println!("# .eve コマンドカバレッジレポート\n");
    println!(
        "- 自カタログ ({}): src_core::command_catalog::COMMAND_CATALOG",
        ours.len()
    );
    println!("- SRC.Sharp ({}): {}", src_sharp.len(), SRC_SHARP_CMD_DIR);
    println!(
        "- VB6 CmdType ({} entries; best-effort): {:?}",
        vb6.len(),
        VB6_CMDTYPE_FILES
    );
    println!();

    // 内訳: 自カタログの Implemented / Stub / ControlFlow ごとの数
    let mut implemented = 0usize;
    let mut stub = 0usize;
    let mut control = 0usize;
    for s in COMMAND_CATALOG {
        match s.kind {
            CommandKind::Implemented => implemented += 1,
            CommandKind::Stub => stub += 1,
            CommandKind::ControlFlow => control += 1,
        }
    }
    println!("## 自カタログ内訳\n");
    println!(
        "- Implemented: {}  / Stub: {}  / ControlFlow: {}",
        implemented, stub, control
    );
    println!();

    // ---- SRC.Sharp ↔ 自カタログ ---------------------------------------
    // alias も含めた lc 一致候補
    let ours_aliased_lc: BTreeSet<String> = COMMAND_CATALOG
        .iter()
        .flat_map(|s| std::iter::once(lc(s.name)).chain(s.aliases.iter().map(|a| lc(a))))
        .collect();

    let src_sharp_lc: BTreeSet<String> = src_sharp.iter().map(|s| lc(s)).collect();

    let in_sharp_not_us: Vec<String> = src_sharp
        .iter()
        .filter(|n| !ours_aliased_lc.contains(&lc(n)))
        .cloned()
        .collect();
    let in_us_not_sharp: Vec<String> = ours
        .iter()
        .filter(|n| !src_sharp_lc.contains(&lc(n)))
        .cloned()
        .collect();

    println!(
        "## SRC.Sharp にあって自カタログに無い ({}件)\n",
        in_sharp_not_us.len()
    );
    if in_sharp_not_us.is_empty() {
        println!("  (なし)");
    } else {
        for chunk in in_sharp_not_us.chunks(6) {
            println!("  - {}", chunk.join(", "));
        }
    }
    println!();

    println!(
        "## 自カタログにあって SRC.Sharp に無い ({}件)\n",
        in_us_not_sharp.len()
    );
    if in_us_not_sharp.is_empty() {
        println!("  (なし)");
    } else {
        for n in &in_us_not_sharp {
            let spec = COMMAND_CATALOG.iter().find(|s| s.name == *n).unwrap();
            println!("  - {} ({:?}) — {}", spec.name, spec.kind, spec.summary);
        }
    }
    println!();

    // ---- VB6 のみ (差分) ----------------------------------------------
    if !vb6.is_empty() {
        let vb6_lc: BTreeSet<String> = vb6.iter().map(|s| lc(s)).collect();
        let in_vb6_not_us: Vec<String> = vb6
            .iter()
            .filter(|n| !ours_aliased_lc.contains(&lc(n)))
            .cloned()
            .collect();
        let in_vb6_not_sharp: Vec<String> = vb6
            .iter()
            .filter(|n| !src_sharp_lc.contains(&lc(n)))
            .cloned()
            .collect();
        println!(
            "## VB6 CmdType にあって自カタログに無い ({}件)\n",
            in_vb6_not_us.len()
        );
        for chunk in in_vb6_not_us.chunks(8) {
            println!("  - {}", chunk.join(", "));
        }
        println!();
        println!(
            "## VB6 CmdType にあって SRC.Sharp にも無い ({}件)\n",
            in_vb6_not_sharp.len()
        );
        for chunk in in_vb6_not_sharp.chunks(8) {
            println!("  - {}", chunk.join(", "));
        }
        println!();
        // unused-warn 抑止
        let _ = vb6_lc;
    }

    println!("---");
    println!("## アクションアイテム候補\n");
    println!("- SRC.Sharp にあって自カタログに無いコマンドは、シナリオ実行時に");
    println!("  `[command-catalog] … 未登録コマンド` 警告として現れる可能性があるため、");
    println!("  Stub として追記するか、実装する。");
    println!("- 自カタログにあって SRC.Sharp に無いコマンドは、VB6 由来 or 我々独自。");
    println!("  実装側 (event_runtime) との整合を確認する。");

    ExitCode::SUCCESS
}
