//! `Data/sp.txt` (`Data/mind.txt`) のパース / Parser for special powers.
//!
//! 元実装: `SpecialPowerDataList.Load` (`SpecialPowerDataList.cls`)。
//! 1 レコード形式（v1 で取り込む範囲）:
//!
//! ```text
//! {Name}
//! {ShortName},{KanaName},{SPConsumption},{TargetType},{Duration}
//! [追加行...]
//! ```

use serde::{Deserialize, Serialize};

use super::loader::{read_data_lines, split_records, SourceLine};
use super::pilot::ParseError;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpecialPowerData {
    pub name: String,
    pub short_name: String,
    pub kana_name: String,
    pub sp_consumption: i32,
    /// 元: `.TargetType`（"自分"/"単体"/"全体"/etc）
    pub target_type: String,
    /// 元: `.Duration`（"瞬間"/"発動ターン"/etc）
    pub duration: String,
}

pub fn parse(src: &str) -> Result<Vec<SpecialPowerData>, ParseError> {
    let lines = read_data_lines(src);
    let records = split_records(&lines);
    records.iter().map(|r| parse_record(r)).collect()
}

/// レコード単位で寛容に解析する。不正な 1 レコードはスキップし、解析できた
/// 精神コマンドだけを返す (`unit::parse_lenient` 等と同方針)。
pub fn parse_lenient(src: &str) -> (Vec<SpecialPowerData>, Vec<ParseError>) {
    let lines = read_data_lines(src);
    let records = split_records(&lines);
    let mut powers = Vec::new();
    let mut errors = Vec::new();
    for r in &records {
        match parse_record(r) {
            Ok(p) => powers.push(p),
            Err(e) => errors.push(e),
        }
    }
    (powers, errors)
}

fn parse_record(record: &[SourceLine]) -> Result<SpecialPowerData, ParseError> {
    // 実 SRC sp.txt の形式:
    //   {Name}[, KanaName]
    //   {ShortName}, {SPCost}, {Target}, {Duration}, {Cond1?}, {Cond2?}, {Anim?}
    //   [習得スキル / 説明文 / ...]
    let mut it = record.iter();
    let name_line = it.next().ok_or_else(|| err(0, "空のレコード"))?;

    // L1: name + 任意の kana
    let (name, mut kana) = match name_line.text.split_once(',') {
        Some((n, k)) => (n.trim().to_string(), k.trim().to_string()),
        None => (name_line.text.clone(), String::new()),
    };

    // 次の非空行を属性行として読む。SP 消費量 (整数) を見つけられない時は
    // SPConsumption=0 にフォールバック。
    let attrs = it.find(|l| !l.text.is_empty());
    let (short_name, sp_consumption, target_type, duration) = match attrs {
        Some(a) => {
            let toks: Vec<&str> = a.text.split(',').map(str::trim).collect();
            let short_name = toks.first().map(|s| s.to_string()).unwrap_or_default();
            let sp = toks.get(1).and_then(|s| s.parse::<i32>().ok()).unwrap_or(0);
            let target = toks.get(2).map(|s| s.to_string()).unwrap_or_default();
            let duration = toks.get(3).map(|s| s.to_string()).unwrap_or_default();
            (short_name, sp, target, duration)
        }
        None => (String::new(), 0, String::new(), String::new()),
    };

    if kana.is_empty() {
        kana = name.clone();
    }

    Ok(SpecialPowerData {
        name,
        short_name,
        kana_name: kana,
        sp_consumption,
        target_type,
        duration,
    })
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
    fn parses_two_powers() {
        // 実 SRC sp.txt 形式: L1=name[,kana], L2=short,sp,target,duration,...
        let src = "\
熱血, ねっけつ
ネツ, 30, 自分, 瞬間, -, -, 熱血

魂, たましい
タマ, 55, 自分, 瞬間, -, -, 魂
";
        let v = parse(src).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].name, "熱血");
        assert_eq!(v[0].kana_name, "ねっけつ");
        assert_eq!(v[0].short_name, "ネツ");
        assert_eq!(v[0].sp_consumption, 30);
        assert_eq!(v[1].sp_consumption, 55);
    }

    #[test]
    fn missing_attrs_fallback_to_defaults() {
        // 互換性を優先し、欠損時はエラーではなくフォールバック (sp=0)
        let src = "熱血\nネツ,ねっけつ\n";
        let v = parse(src).unwrap();
        assert_eq!(v[0].name, "熱血");
        assert_eq!(v[0].short_name, "ネツ");
        assert_eq!(v[0].sp_consumption, 0);
    }
}
