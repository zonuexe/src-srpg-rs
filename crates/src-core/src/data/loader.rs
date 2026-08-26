//! `Data/*.txt` 共通の行リーダと文字コード変換 / Shared line reader & encoding helpers.
//!
//! 元 `Event.bas` の `GetLine` 相当: 行頭 `#` のコメント行を空行に、
//! 行中の `//` をコメントとして切り捨てる。`'` (シングルクオート) と
//! `"` (ダブルクオート) のクオート状態を保持し、その中の `//` は通常文字扱い。
//!
//! 戻り値は (行番号, 行本体) のリスト。空行は除去せずに保持する
//! （上位のパーサがレコード境界を判定する場合があるため）。
//!
//! Roughly equivalent to `Event.bas`'s `GetLine` helper.

/// 元 SRC は Shift_JIS でテキストを保存している。受け取った生バイト列を UTF-8
/// 文字列に変換するヘルパ。BOM や UTF-8 で始まる場合は素通し、それ以外は
/// Shift_JIS としてデコードする（不正バイトは `U+FFFD` 置換）。
pub fn decode_text(bytes: &[u8]) -> String {
    // UTF-8 BOM
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8_lossy(&bytes[3..]).into_owned();
    }
    // 妥当な UTF-8 ならそのまま
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_string();
    }
    // それ以外は Shift_JIS とみなす
    let (cow, _, _) = encoding_rs::SHIFT_JIS.decode(bytes);
    cow.into_owned()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceLine {
    /// 1 始まりの行番号（エラー報告用）
    pub line_num: usize,
    /// コメントを取り除き、両端をトリムした本文
    pub text: String,
}

/// テキスト全体を行配列に分解し、コメントを除去。
/// Tokenize a whole source file: strip comments, trim whitespace, keep line numbers.
pub fn read_lines(src: &str) -> Vec<SourceLine> {
    src.split('\n')
        .enumerate()
        .map(|(idx, raw)| SourceLine {
            line_num: idx + 1,
            text: strip_comments(raw).trim().to_string(),
        })
        .collect()
}

/// 1 行分のコメントを取り除く。
/// Strip `#`-prefix line comments and `//` trailing comments while respecting quotes.
fn strip_comments(line: &str) -> String {
    let trimmed = line.trim_start();
    if trimmed.starts_with('#') {
        return String::new();
    }

    let mut in_single = false;
    let mut in_double = false;
    let bytes = line.as_bytes();
    let mut cut = bytes.len();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'`' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'/' if !in_single && !in_double && i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                cut = i;
                break;
            }
            _ => {}
        }
        i += 1;
    }
    line[..cut].to_string()
}

/// データファイル (`pilot.txt` / `unit.txt` / `item.txt` / `sp.txt`) 用の
/// 行リーダ。`.eve` と違って行頭 `#` は **コメントではなく内容の一部**
/// （`#武器名` 等の sigil として使われる）。`//` インラインコメントだけ除去する。
pub fn read_data_lines(src: &str) -> Vec<SourceLine> {
    src.split('\n')
        .enumerate()
        .map(|(idx, raw)| SourceLine {
            line_num: idx + 1,
            // 元 SRC `GeneralLib.GetLine` は全データ行で全角コンマ `，`(U+FF0C) を
            // 半角 `, ` に正規化してからパースする（フィールド区切りは半角 `,`)。
            // 全角・半角混在のデータ (例: `光頼，男性，魔術機, AABB, 190`) を
            // 取りこぼさないため、同じ正規化を行う。
            text: strip_slash_comment(raw)
                .replace('，', ", ")
                .trim()
                .to_string(),
        })
        .collect()
}

/// `//` 以降のみ除去（行頭 `#` には触れない）。
fn strip_slash_comment(line: &str) -> String {
    let mut in_single = false;
    let mut in_double = false;
    let bytes = line.as_bytes();
    let mut cut = bytes.len();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'`' if !in_double => in_single = !in_single,
            b'"' if !in_single => in_double = !in_double,
            b'/' if !in_single && !in_double && i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                cut = i;
                break;
            }
            _ => {}
        }
        i += 1;
    }
    line[..cut].to_string()
}

/// 連続する空行を 1 レコード境界として、空行で区切られたレコード単位に分割。
/// Group consecutive non-empty lines into records, separated by blank lines.
///
/// 先頭行が `#` で始まるレコードはコメント (`###見出し`, `# section header`,
/// `# 開始時設定...` 等) としてスキップする。pilot / unit / item / sp / terrain の
/// 全データファイルで、本来のエンティティ名は `#` で始まらないため安全。
/// 内部行 (例: bitmap 行の `#BGlxy_AI.bmp` sigil) は引き続きレコードに含まれる。
pub fn split_records(lines: &[SourceLine]) -> Vec<Vec<SourceLine>> {
    let mut records = Vec::new();
    let mut current: Vec<SourceLine> = Vec::new();
    for line in lines {
        if line.text.is_empty() {
            if !current.is_empty() {
                records.push(std::mem::take(&mut current));
            }
        } else {
            current.push(line.clone());
        }
    }
    if !current.is_empty() {
        records.push(current);
    }
    records.retain(|r| {
        r.first()
            .map(|line| !line.text.starts_with('#'))
            .unwrap_or(false)
    });
    records
}

/// SRC がシナリオデータとして読み込むファイルの basename 一覧。
///
/// 原典 VB6 `SRC.bas::IncludeData` / `LoadData` が `Data\` 配下で
/// `FileExists` を確かめて読むファイル名と一致させる。
pub const DATA_FILE_BASENAMES: &[&str] = &[
    "alias.txt",
    "sp.txt",
    "mind.txt",
    "pilot.txt",
    "non_pilot.txt",
    "robot.txt",
    "unit.txt",
    "pilot_message.txt",
    "pilot_dialog.txt",
    "effect.txt",
    "animation.txt",
    "ext_animation.txt",
    "item.txt",
    "terrain.txt",
];

/// 与えられた名前 (アーカイブ内パス) が SRC データファイルの basename か。
pub fn is_data_file_name(name: &str) -> bool {
    let base = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .to_ascii_lowercase();
    DATA_FILE_BASENAMES.contains(&base.as_str())
}

/// パスの途中に `Data` ディレクトリ成分を含むか (大文字小文字無視)。
///
/// 原典 SRC はシナリオデータを `<ScenarioPath>\Data\...` からのみ読む
/// (`SRC.bas::SearchDataFolder` は `Data\<作品名>` を探す)。
pub fn is_under_data_dir(name: &str) -> bool {
    let mut parts: Vec<&str> = name.split(['/', '\\']).collect();
    parts.pop(); // ファイル名自身は除く
    parts.iter().any(|p| p.eq_ignore_ascii_case("data"))
}

/// アーカイブのデータファイル探索を `Data/` 配下に限定すべきか。
///
/// `Data/` 配下にデータファイルが 1 つでもあるアーカイブでは、その外側に
/// ある同名ファイルは SRC データではない: 実コーパスでは
/// `Lib/Library/Unit.txt` (別ツールの `[Setting]` 形式プロファイル) や
/// `Lib/_encyclopedia/pilot.txt` (`en_パイロット[1] = "…"` 形式の
/// 図鑑ライブラリ) が該当し、パーサに渡すと大量の「設定に抜けがあります」
/// を生む。
///
/// 一方 `Data/` を持たないデータ集アーカイブ (`作品名/item.txt` を
/// SRC の `Data\` へ配置して使う形式) も実在するため、その場合は
/// 限定せず従来どおり basename 一致で拾う。
pub fn scope_to_data_dir<'a, I>(entry_names: I) -> bool
where
    I: IntoIterator<Item = &'a str>,
{
    entry_names
        .into_iter()
        .any(|n| is_data_file_name(n) && is_under_data_dir(n))
}

/// 個々のエントリをデータファイルとして採用するか。
/// `scope` は [`scope_to_data_dir`] の判定結果。
pub fn accept_data_entry(name: &str, scope: bool) -> bool {
    is_data_file_name(name) && (!scope || is_under_data_dir(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_lines_become_empty() {
        let lines = read_lines("# comment\nfoo");
        assert_eq!(lines[0].text, "");
        assert_eq!(lines[1].text, "foo");
    }

    #[test]
    fn slash_slash_strips_trailing_comment() {
        let lines = read_lines("foo // trailing\nbar");
        assert_eq!(lines[0].text, "foo");
        assert_eq!(lines[1].text, "bar");
    }

    #[test]
    fn slash_slash_inside_string_is_kept() {
        let lines = read_lines(r#""hello // world""#);
        assert_eq!(lines[0].text, r#""hello // world""#);
    }

    #[test]
    fn split_records_skips_comment_records() {
        // 先頭行 `#`/`###` で始まるレコードはコメントとしてスキップ。
        // 内部行の `#BGlxy.bmp` sigil はレコード末尾に残るので維持される。
        let lines = read_data_lines(
            "### 見出し\n\n\
             # 単発コメント\n\n\
             リオ\n\
             リオ,男性,リアル,SSSS,100\n\
             #BGlxy.bmp\n",
        );
        let records = split_records(&lines);
        assert_eq!(records.len(), 1, "コメントレコードは除外される");
        assert_eq!(records[0][0].text, "リオ");
        assert_eq!(records[0].len(), 3, "内部の #sigil 行は残る");
    }

    #[test]
    fn data_lines_normalize_fullwidth_comma() {
        // 元 SRC GetLine 互換: データ行の全角コンマ `，` は `, ` に正規化。
        // フィールド分割は半角 `,` なので、全角混在でも欠落しない。
        let lines = read_data_lines("光頼，男性，魔術機, AABB, 190\n");
        let fields: Vec<&str> = lines[0].text.split(',').map(|s| s.trim()).collect();
        assert_eq!(fields, vec!["光頼", "男性", "魔術機", "AABB", "190"]);
    }

    #[test]
    fn split_records_splits_on_blank_line() {
        let lines = read_lines("a\nb\n\nc\nd");
        let records = split_records(&lines);
        assert_eq!(records.len(), 2);
        assert_eq!(records[0].len(), 2);
        assert_eq!(records[1].len(), 2);
    }

    #[test]
    fn decode_text_utf8_passthrough() {
        let s = "あいうえお";
        assert_eq!(decode_text(s.as_bytes()), s);
    }

    #[test]
    fn decode_text_utf8_bom_stripped() {
        let mut b = vec![0xEF, 0xBB, 0xBF];
        b.extend_from_slice("hello".as_bytes());
        assert_eq!(decode_text(&b), "hello");
    }

    #[test]
    fn decode_text_shift_jis() {
        // "あ" in Shift_JIS = 0x82 0xA0
        let sjis = [0x82, 0xA0];
        assert_eq!(decode_text(&sjis), "あ");
    }

    #[test]
    fn line_numbers_preserved() {
        let lines = read_lines("# c\nbody\n# c2\nbody2");
        assert_eq!(lines[1].line_num, 2);
        assert_eq!(lines[3].line_num, 4);
    }

    #[test]
    fn data_dir_detection() {
        assert!(is_under_data_dir("scn/Data/unit.txt"));
        assert!(is_under_data_dir("scn/data/作品/unit.txt"));
        assert!(is_under_data_dir("scn\\DATA\\unit.txt"));
        assert!(!is_under_data_dir("scn/Lib/Library/Unit.txt"));
        assert!(!is_under_data_dir("unit.txt"));
        // `data` はディレクトリ成分でなければならない。
        assert!(!is_under_data_dir("scn/mydata/unit.txt"));
    }

    #[test]
    fn data_file_name_detection() {
        assert!(is_data_file_name("a/b/pilot.txt"));
        assert!(is_data_file_name("a\\b\\Unit.TXT"));
        assert!(!is_data_file_name("a/b/readme.txt"));
    }

    /// `Data/` 配下があるアーカイブでは、外側の同名ファイル
    /// (別ツールの `Lib/Library/Unit.txt` 等) を採用しない。
    #[test]
    fn scoped_archive_rejects_files_outside_data_dir() {
        let names = ["scn/Data/unit.txt", "scn/Lib/Library/Unit.txt"];
        let scope = scope_to_data_dir(names.iter().copied());
        assert!(scope);
        assert!(accept_data_entry("scn/Data/unit.txt", scope));
        assert!(!accept_data_entry("scn/Lib/Library/Unit.txt", scope));
    }

    /// `Data/` を持たないデータ集アーカイブは従来どおり拾う。
    #[test]
    fn unscoped_archive_keeps_basename_match() {
        let names = ["作品名/item.txt", "作品名/non_pilot.txt"];
        let scope = scope_to_data_dir(names.iter().copied());
        assert!(!scope);
        assert!(accept_data_entry("作品名/item.txt", scope));
    }
}
