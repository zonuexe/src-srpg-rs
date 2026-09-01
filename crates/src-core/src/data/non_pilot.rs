//! 非パイロットデータ (`non_pilot.txt`) パーサ / Non-pilot data parser.
//!
//! 原典 SRC の `NonPilotDataList.Load` に相当する。無人ユニットや会話専用の
//! キャラクタなど、戦闘能力を持たない登場人物を定義する。書式は 2 行 1 組:
//!
//! ```text
//! 名前
//! ニックネーム,Bitmap.bmp
//! ```
//!
//! 本実装にはまだ非パイロット専用テーブルが無いため、最低限の [`PilotData`]
//! として取り込む。`Talk` 等のイベントから名前で引かれたときにヒットさせる
//! ための仮置きで、戦闘ステータスはすべて 0 / 空。

use super::pilot::{Adaption, PilotData, Sex};

/// `non_pilot.txt` の本文をパースして `PilotData` の並びを返す。
pub fn parse(txt: &str) -> Vec<PilotData> {
    let mut out = Vec::new();
    let mut lines = txt.lines().map(str::trim);
    while let Some(name_line) = lines.next() {
        if name_line.is_empty() || name_line.starts_with(';') {
            continue;
        }
        // 次の非空行を「ニックネーム,Bitmap」行として消費する。
        let Some(meta_line) = lines.find(|l| !l.is_empty()) else {
            break;
        };
        let (nick, bitmap) = match meta_line.split_once(',') {
            Some((n, b)) => (n.trim(), b.trim()),
            None => (meta_line, ""),
        };
        out.push(PilotData {
            spirit_commands: Vec::new(),
            name: name_line.to_string(),
            nickname: nick.to_string(),
            kana_name: nick.to_string(),
            sex: Sex::Unspecified,
            class: String::new(),
            adaption: Adaption::parse("----").unwrap_or(Adaption([b'-'; 4])),
            exp_value: 0,
            infight: 0,
            shooting: 0,
            hit: 0,
            dodge: 0,
            intuition: 0,
            technique: 0,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: if bitmap.is_empty() {
                None
            } else {
                Some(bitmap.to_string())
            },
            features: Vec::new(),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_two_line_records() {
        let src = "アリス\nありす,alice.bmp\n\nボブ\nぼぶ\n";
        let v = parse(src);
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "アリス");
        assert_eq!(v[0].nickname, "ありす");
        assert_eq!(v[0].bitmap.as_deref(), Some("alice.bmp"));
        assert_eq!(v[1].name, "ボブ");
        assert_eq!(v[1].bitmap, None, "Bitmap 省略時は None");
    }

    #[test]
    fn skips_comment_lines() {
        let v = parse(";コメント\nアリス\nありす,a.bmp\n");
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].name, "アリス");
    }

    #[test]
    fn ignores_trailing_name_without_meta_line() {
        let v = parse("アリス\nありす,a.bmp\nボブ\n");
        assert_eq!(v.len(), 1, "対になる行が無い末尾レコードは捨てる");
    }
}
