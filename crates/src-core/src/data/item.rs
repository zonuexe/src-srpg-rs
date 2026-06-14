//! `Data/item.txt` のパース / Parser for `Data/item.txt`.
//!
//! 元実装: `ItemDataList.Load` (`ItemDataList.cls`)。
//! 1 レコードのおおまかな構造（v1 で対応する範囲）:
//!
//! ```text
//! {Name}
//! {Class},{Part},{HP},{EN},{Armor},{Mobility},{Speed}
//! [説明テキスト行 0..N (空行で終端)]
//! ```
//!
//! 元には武器・特殊能力・アビリティ等の追加行もあるが、本パーサは基本
//! ステータス行までを厳密に解釈し、残りはコメントとして取り込む。

use serde::{Deserialize, Serialize};

use super::loader::{read_data_lines, split_records, SourceLine};
use super::pilot::ParseError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ItemData {
    pub name: String,
    pub class: String,
    /// 元: `.Part`（装備個所: "本体"/"両手"等）
    pub part: String,
    pub hp_mod: i64,
    pub en_mod: i32,
    pub armor_mod: i64,
    pub mobility_mod: i32,
    pub speed_mod: i32,
    pub comment: String,
    /// 「特殊能力」セクションの `名前=値` 行（順序保持）。
    #[serde(default)]
    pub features: Vec<(String, String)>,
}

pub fn parse(src: &str) -> Result<Vec<ItemData>, ParseError> {
    let lines = read_data_lines(src);
    let records = split_records(&lines);
    records.iter().map(|r| parse_record(r)).collect()
}

/// レコード単位で寛容に解析する。不正な 1 レコードはスキップし、解析できた
/// アイテムだけを返す (`unit::parse_lenient` / `pilot::parse_lenient` と同方針)。
pub fn parse_lenient(src: &str) -> (Vec<ItemData>, Vec<ParseError>) {
    let lines = read_data_lines(src);
    let records = split_records(&lines);
    let mut items = Vec::new();
    let mut errors = Vec::new();
    for r in &records {
        match parse_record(r) {
            Ok(it) => items.push(it),
            Err(e) => errors.push(e),
        }
    }
    (items, errors)
}

fn parse_record(record: &[SourceLine]) -> Result<ItemData, ParseError> {
    // 実 SRC item.txt は柔軟な multi-line 形式:
    //   {Name}
    //   {Nickname or Class},...
    //   [特殊能力 マーカ + フィーチャー行群]
    //   {HP},{EN},{Armor},{Mob},{Speed}   (5 整数の line)
    //   ...
    // ここでは name と「5 整数カンマ区切り」行だけ厳密に解釈し、
    // それ以外は class / part / comment にざっくり詰め込む。
    let name_line = record.first().ok_or_else(|| err(0, "空のレコード"))?;
    let mut class = String::new();
    let mut part = String::new();
    let mut stat_line: Option<&SourceLine> = None;
    let mut comment_lines: Vec<String> = Vec::new();
    let mut features: Vec<(String, String)> = Vec::new();

    for (idx, line) in record.iter().enumerate().skip(1) {
        if line.text.is_empty() {
            continue;
        }
        // 5 整数カンマ区切りなら stat 行
        if let Some(parsed) = try_item_stat(&line.text) {
            stat_line = Some(line);
            // 残りはコメント
            for extra in record.iter().skip(idx + 1) {
                if !extra.text.is_empty() {
                    comment_lines.push(extra.text.clone());
                }
            }
            let _ = parsed;
            break;
        }
        // 「特殊能力」セクション内の `名前=値` 行を捕捉。
        if let Some(feat) = parse_feature_line(&line.text) {
            features.push(feat);
            continue;
        }
        // class/part 候補: 2 〜 3 カンマフィールドで全部非数値の行
        if class.is_empty() {
            let toks: Vec<&str> = line.text.split(',').map(str::trim).collect();
            if toks.len() >= 2 && toks.iter().all(|t| t.parse::<i32>().is_err()) {
                class = toks.first().map(|s| s.to_string()).unwrap_or_default();
                part = toks.get(1).map(|s| s.to_string()).unwrap_or_default();
                continue;
            }
        }
        comment_lines.push(line.text.clone());
    }

    let (hp_mod, en_mod, armor_mod, mobility_mod, speed_mod) = match stat_line {
        Some(line) => try_item_stat(&line.text)
            .ok_or_else(|| err(line.line_num, "ステータス行の再解析に失敗しました。"))?,
        // stat 行未検出時は 0 埋め（コメントだけのアイテム）
        None => (0, 0, 0, 0, 0),
    };

    Ok(ItemData {
        name: name_line.text.clone(),
        class,
        part,
        hp_mod,
        en_mod,
        armor_mod,
        mobility_mod,
        speed_mod,
        comment: comment_lines.join("\n"),
        features,
    })
}

/// 「特殊能力」セクション内の `名前=値` 行を `(name, value)` として返す。
fn parse_feature_line(text: &str) -> Option<(String, String)> {
    let (name, value) = text.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some((name.to_string(), value.trim().to_string()))
}

/// 5 整数カンマ区切りなら `(hp, en, armor, mob, speed)` を返す。
fn try_item_stat(text: &str) -> Option<(i64, i32, i64, i32, i32)> {
    let toks: Vec<&str> = text.split(',').map(str::trim).collect();
    if toks.len() != 5 {
        return None;
    }
    let hp = toks[0].parse().ok()?;
    let en = toks[1].parse().ok()?;
    let armor = toks[2].parse().ok()?;
    let mob = toks[3].parse().ok()?;
    let speed = toks[4].parse().ok()?;
    Some((hp, en, armor, mob, speed))
}

fn err(line_num: usize, message: &str) -> ParseError {
    ParseError {
        line_num,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_simple_item() {
        // 実 SRC item.txt 形式 (Name / Nickname,Class,Part / 特殊能力 行群 / 5 整数の stat 行)
        let src = "\
ハイパーバズーカ
武器,両手
強力な実弾兵装。
弾薬限定。
0,0,0,0,0
";
        let items = parse(src).unwrap();
        assert_eq!(items.len(), 1);
        let it = &items[0];
        assert_eq!(it.name, "ハイパーバズーカ");
        assert_eq!(it.class, "武器");
        assert_eq!(it.part, "両手");
        assert_eq!(it.hp_mod, 0);
        assert!(it.comment.contains("強力な実弾兵装"));
    }

    #[test]
    fn parses_stat_modifier_item() {
        let src = "\
強化装甲
装備,本体
装甲を増強する。
500,20,300,5,0
";
        let it = &parse(src).unwrap()[0];
        assert_eq!(it.armor_mod, 300);
        assert_eq!(it.hp_mod, 500);
        assert_eq!(it.en_mod, 20);
        assert_eq!(it.mobility_mod, 5);
    }
}
