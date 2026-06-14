//! `Data/system/terrain.txt` のパース / Parser for `terrain.txt`.
//!
//! 元実装: `TerrainDataList.Load`。実 SRC シナリオでは以下のレコード形式:
//!
//! ```text
//! {id}
//! {name},{english_alias}
//! {class},{move_cost},{hit_mod},{damage_mod}
//! [features... 0..N lines]
//! [blank]
//! ```
//!
//! 例:
//! ```text
//! 0
//! 平地, plain
//! 陸, 1, 0, 0
//!
//! 32
//! 低木, bush
//! 陸, 1.5, 5, 5
//! ```
//!
//! `move_cost` は浮動小数（"1.5" 等）で表現される。本実装では整数に丸める。
//! features は今は文字列リストとして保持。

use serde::{Deserialize, Serialize};

use super::loader::{read_data_lines, split_records, SourceLine};
use super::pilot::ParseError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TerrainEntry {
    /// 元: `.ID`
    pub id: u32,
    /// 元: `.Name`（日本語）
    pub name: String,
    /// 移植版独自: 英語別名（画像ファイル名検索用）
    pub english: String,
    /// 元: `.Class`（"陸"/"空"/"海"/"宇" 等）
    pub class: String,
    /// 元: `.MoveCost`（小数 → 切上げ整数）
    pub move_cost: i32,
    /// 元: `.HitMod` (回避修正)
    pub hit_mod: i32,
    /// 元: `.DamageMod`
    pub damage_mod: i32,
    /// 元: `.colFeature` 由来の追加属性文字列
    pub features: Vec<String>,
}

pub fn parse(src: &str) -> Result<Vec<TerrainEntry>, ParseError> {
    let lines = read_data_lines(src);
    let records = split_records(&lines);
    records
        .iter()
        .filter_map(|r| parse_record(r).transpose())
        .collect()
}

/// レコード単位で寛容に解析する。不正な 1 レコードはスキップし、解析できた
/// 地形エントリだけを返す (`unit::parse_lenient` 等と同方針)。空レコードは
/// `None` として無視する。
pub fn parse_lenient(src: &str) -> (Vec<TerrainEntry>, Vec<ParseError>) {
    let lines = read_data_lines(src);
    let records = split_records(&lines);
    let mut entries = Vec::new();
    let mut errors = Vec::new();
    for r in &records {
        match parse_record(r) {
            Ok(Some(e)) => entries.push(e),
            Ok(None) => {}
            Err(e) => errors.push(e),
        }
    }
    (entries, errors)
}

/// 1 レコードを解釈。空レコードは `None`。
fn parse_record(record: &[SourceLine]) -> Result<Option<TerrainEntry>, ParseError> {
    if record.is_empty() {
        return Ok(None);
    }
    let mut it = record.iter();
    let id_line = it.next().unwrap();
    let id: u32 = id_line
        .text
        .parse()
        .map_err(|_| err(id_line.line_num, "地形 id が整数ではありません。"))?;

    let name_line = it
        .next()
        .ok_or_else(|| err(id_line.line_num, "地形名行がありません。"))?;
    let (name, english) = match name_line.text.split_once(',') {
        Some((n, e)) => (n.trim().to_string(), e.trim().to_string()),
        None => (name_line.text.clone(), String::new()),
    };

    let stat_line = it
        .next()
        .ok_or_else(|| err(name_line.line_num, "地形ステータス行がありません。"))?;
    let toks: Vec<&str> = stat_line.text.split(',').map(str::trim).collect();
    if toks.len() < 4 {
        return Err(err(stat_line.line_num, "ステータスは 4 項目必要。"));
    }
    let class = toks[0].to_string();
    let move_cost = parse_float_ceiling(toks[1])
        .ok_or_else(|| err(stat_line.line_num, "move_cost が数値ではありません。"))?;
    let hit_mod: i32 = toks[2]
        .parse()
        .map_err(|_| err(stat_line.line_num, "hit_mod が数値ではありません。"))?;
    let damage_mod: i32 = toks[3]
        .parse()
        .map_err(|_| err(stat_line.line_num, "damage_mod が数値ではありません。"))?;

    let features: Vec<String> = it
        .filter(|l| !l.text.is_empty())
        .map(|l| l.text.clone())
        .collect();

    Ok(Some(TerrainEntry {
        id,
        name,
        english,
        class,
        move_cost,
        hit_mod,
        damage_mod,
        features,
    }))
}

fn parse_float_ceiling(s: &str) -> Option<i32> {
    // 元 SRC では "-" を「通行不能」のマーカとして使う。Dijkstra でカットされる
    // よう極大値 (= 9999) を返す。
    if s == "-" {
        return Some(9999);
    }
    if let Ok(f) = s.parse::<f64>() {
        return Some(f.ceil() as i32);
    }
    None
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
    fn parses_real_format() {
        let src = "\
0
平地, plain
陸, 1, 0, 0

32
低木, bush
陸, 1.5, 5, 5

15
山, mountain
陸, 2.5, 30, 30
衝突
";
        let v = parse(src).unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].id, 0);
        assert_eq!(v[0].name, "平地");
        assert_eq!(v[0].english, "plain");
        assert_eq!(v[0].class, "陸");
        assert_eq!(v[0].move_cost, 1);
        assert_eq!(v[1].move_cost, 2); // 1.5 → 切上げ 2
        assert_eq!(v[2].id, 15);
        assert_eq!(v[2].name, "山");
        assert_eq!(v[2].move_cost, 3); // 2.5 → 3
        assert_eq!(v[2].hit_mod, 30);
        assert_eq!(v[2].features, vec!["衝突".to_string()]);
    }
}
