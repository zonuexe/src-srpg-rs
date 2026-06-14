//! シナリオアーカイブ群からテキスト系エントリ (.eve/.txt/.map/.ini) を
//! デコードしてディレクトリツリーに書き出す解析補助 CLI。
//!
//! 使い方:
//!   cargo run -p verify-archive --bin extract_text -- <out_dir> <archive...>
//!
//! 各アーカイブごとに `<out_dir>/<archive_basename>/<entry_path>` を作る。

use std::env;
use std::fs;
use std::io::{Cursor, Read};
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
            unzip(&bytes)
        } else if lower.ends_with(".lzh") || lower.ends_with(".lha") {
            unlzh(&bytes)
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

fn unzip(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    let cursor = Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(cursor).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        let name = loader::decode_text(entry.name_raw());
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        out.push((name, buf));
    }
    Ok(out)
}

fn unlzh(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, String> {
    use delharc::LhaDecodeReader;
    let mut reader =
        LhaDecodeReader::new(Cursor::new(bytes)).map_err(|e| format!("LZH header: {e}"))?;
    let mut out = Vec::new();
    loop {
        let header = reader.header();
        let is_dir = header.parse_pathname().to_string_lossy().ends_with('/');
        if !is_dir && reader.is_decoder_supported() {
            let name = decode_lzh_filename(header);
            let mut buf = Vec::new();
            reader
                .read_to_end(&mut buf)
                .map_err(|e| format!("LZH decode: {e}"))?;
            out.push((name, buf));
        } else if !is_dir {
            let _ = reader.read_to_end(&mut Vec::new());
        }
        if !reader.next_file().map_err(|e| format!("LZH next: {e}"))? {
            break;
        }
    }
    Ok(out)
}

const EXT_HEADER_FILENAME: u8 = 0x01;
const EXT_HEADER_PATH: u8 = 0x02;

fn decode_lzh_filename(header: &delharc::LhaHeader) -> String {
    let mut path_bytes: Vec<u8> = Vec::new();
    let mut filename_bytes: Option<&[u8]> = None;
    for hdr in header.iter_extra() {
        match hdr {
            [EXT_HEADER_PATH, data @ ..] => {
                path_bytes.extend(data.iter().map(|&b| if b == 0xFF { b'/' } else { b }));
                let needs_sep = !path_bytes.is_empty()
                    && !path_bytes.ends_with(b"/")
                    && !path_bytes.ends_with(b"\\");
                if needs_sep {
                    path_bytes.push(b'/');
                }
            }
            [EXT_HEADER_FILENAME, data @ ..] => {
                filename_bytes = Some(data);
            }
            _ => {}
        }
    }
    let basename = filename_bytes.unwrap_or(header.filename.as_ref());
    path_bytes.extend_from_slice(basename);
    if path_bytes.is_empty() {
        return header.parse_pathname_to_str();
    }
    let sanitized: Vec<u8> = path_bytes
        .iter()
        .map(|&b| if b == 0xFF { b'/' } else { b })
        .collect();
    loader::decode_text(&sanitized).replace('\\', "/")
}
