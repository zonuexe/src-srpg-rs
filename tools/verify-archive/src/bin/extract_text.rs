//! シナリオアーカイブ群からテキスト系エントリ (.eve/.txt/.map/.ini) を
//! デコードしてディレクトリツリーに書き出す解析補助 CLI。
//!
//! 使い方:
//!   cargo run -p verify-archive --bin extract_text -- <out_dir> <archive...>
//!
//! 各アーカイブごとに `<out_dir>/<archive_basename>/<entry_path>` を作る。

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use src_core::data::loader;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("usage: extract_text <out_dir> <archive...>");
        return ExitCode::FAILURE;
    }
    let out_dir = PathBuf::from(&args[1]);
    let mut written = 0usize;
    for path in &args[2..] {
        let bytes = match fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("read failed {path}: {e}");
                continue;
            }
        };
        let lower = path.to_ascii_lowercase();
        let entries = if lower.ends_with(".zip") {
            src_core::archive::unzip(&bytes)
        } else if lower.ends_with(".lzh") || lower.ends_with(".lha") {
            src_core::archive::unlzh(&bytes)
        } else {
            continue;
        };
        let entries = match entries {
            Ok(v) => v,
            Err(e) => {
                eprintln!("extract failed {path}: {e}");
                continue;
            }
        };
        let stem = Path::new(path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("archive");
        for (name, data) in &entries {
            let lname = name.to_ascii_lowercase();
            let is_text = lname.ends_with(".eve")
                || lname.ends_with(".txt")
                || lname.ends_with(".map")
                || lname.ends_with(".ini")
                || lname.ends_with(".dat");
            if !is_text {
                continue;
            }
            let safe: String = name.replace('\\', "/");
            let rel = safe.trim_start_matches('/');
            // パストラバーサル回避
            if rel.split('/').any(|c| c == "..") {
                continue;
            }
            let dest = out_dir.join(stem).join(rel);
            if let Some(parent) = dest.parent() {
                let _ = fs::create_dir_all(parent);
            }
            let txt = loader::decode_text(data);
            if fs::write(&dest, txt.as_bytes()).is_ok() {
                written += 1;
            }
        }
    }
    eprintln!("wrote {written} text files under {}", out_dir.display());
    ExitCode::SUCCESS
}
