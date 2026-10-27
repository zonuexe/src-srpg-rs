//! シナリオアーカイブの展開 (`.zip` / `.lzh` / 単独ファイル)。
//!
//! 元 SRC の配布アーカイブはファイル名が Shift_JIS (UTF-8 フラグ無し) なので、名前は
//! 生バイトから [`loader::decode_text`] で UTF-8 化する。フロントエンド (src-web)・
//! 検証ツール (verify-archive)・統合テストが共有する。

use std::io::{Cursor, Read};

use crate::data::loader;

/// 展開済みのエントリ (アーカイブ内パス, 中身)。
pub type Entries = Vec<(String, Vec<u8>)>;

/// `file_name` の拡張子で展開方法を選ぶ。`.zip` / `.lzh` / `.lha` 以外は単独ファイルとして
/// 1 エントリにする。
pub fn extract(file_name: &str, bytes: &[u8]) -> Result<Entries, String> {
    let lower = file_name.to_ascii_lowercase();
    if lower.ends_with(".zip") {
        unzip(bytes).map_err(|e| format!("zip 展開失敗: {e}"))
    } else if lower.ends_with(".lzh") || lower.ends_with(".lha") {
        unlzh(bytes).map_err(|e| format!("lzh 展開失敗: {e}"))
    } else {
        Ok(vec![(file_name.to_string(), bytes.to_vec())])
    }
}

/// ZIP アーカイブを展開する (ディレクトリエントリは除く)。
pub fn unzip(bytes: &[u8]) -> Result<Entries, String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let mut out = Vec::with_capacity(archive.len());
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        if entry.is_dir() {
            continue;
        }
        // zip クレートは UTF-8 フラグの無い名前を CP437 として誤デコードするので、
        // 生バイトを SJIS 自動判定で UTF-8 化する。
        let name = loader::decode_text(entry.name_raw());
        let mut buf = Vec::with_capacity(entry.size() as usize);
        entry.read_to_end(&mut buf).map_err(|e| e.to_string())?;
        out.push((name, buf));
    }
    Ok(out)
}

/// LZH / LHA アーカイブ (lh0 / lh1 / lh4 / lh5 / lh6 / lh7 等) を展開する。未対応の
/// 圧縮方式のエントリは読み飛ばす。
pub fn unlzh(bytes: &[u8]) -> Result<Entries, String> {
    let mut reader = delharc::LhaDecodeReader::new(Cursor::new(bytes))
        .map_err(|e| format!("LZH ヘッダの読み取り失敗: {e}"))?;
    let mut out = Vec::new();
    loop {
        let header = reader.header();
        let is_dir = header.parse_pathname().to_string_lossy().ends_with('/');
        if !is_dir {
            if reader.is_decoder_supported() {
                let name = decode_lzh_filename(header);
                let mut buf = Vec::with_capacity(header.original_size as usize);
                reader
                    .read_to_end(&mut buf)
                    .map_err(|e| format!("LZH デコード失敗: {e}"))?;
                out.push((name, buf));
            } else {
                let _ = reader.read_to_end(&mut Vec::new());
            }
        }
        if !reader
            .next_file()
            .map_err(|e| format!("LZH 次エントリ取得失敗: {e}"))?
        {
            break;
        }
    }
    Ok(out)
}

const EXT_HEADER_FILENAME: u8 = 0x01;
const EXT_HEADER_PATH: u8 = 0x02;

/// LZH エントリのパス。拡張ヘッダのディレクトリ (`0x02`) とファイル名 (`0x01`、無ければ
/// Level 0/1 の `header.filename`) を連結し、区切り `0xFF` / `\` を `/` にする。
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
    // ディレクトリ (拡張ヘッダ) とベース名 (header.filename) を両方持つアーカイブが
    // 実在するため、両者を必ず連結する。
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
