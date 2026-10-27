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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SpecialPowerData {
    pub name: String,
    pub short_name: String,
    pub kana_name: String,
    pub sp_consumption: i32,
    /// 元: `.TargetType`（"自分"/"単体"/"全体"/etc）
    pub target_type: String,
    /// 元: `.Duration`（"瞬間"/"発動ターン"/etc）
    pub duration: String,
    /// 効果リスト `(effect_type, level)`。元: `SpecialPowerData.SetEffect`
    /// (`SpecialPowerData.cs`)。効果行 (レコード 3 行目) を `,` 区切りで分解し、
    /// 各要素を `Lv` で `type` と `level` に分ける (例: `ダメージ増加Lv10` →
    /// `("ダメージ増加", 10.0)`)。`Lv` なしの効果はレベル既定値 0.0 とする
    /// (本移植が必要とする数値効果 (ダメージ増加 等) は常に `Lv` を伴うため十分)。
    #[serde(default)]
    pub effects: Vec<(String, f64)>,
}

impl SpecialPowerData {
    /// `イベント=<routine>` 効果が指定するサブルーチン名。SRC では「システム側で
    /// 用意されていない効果のスペシャルパワーを自作する」ために、効果行に
    /// `イベント=<ラベル>` と書くと発動時にそのサブルーチンが呼ばれる
    /// (`SpecialPowerData.cls` Execute の "イベント" 効果種別)。サンプルの `生贄`
    /// (`イベント=生贄ルーチン`) が代表例。`parse_effects` は `イベント=生贄ルーチン` を
    /// `("イベント=生贄ルーチン", 0.0)` として保持するので、ここで接頭辞を剥がす。
    pub fn event_routine(&self) -> Option<&str> {
        self.effects.iter().find_map(|(e, _)| {
            e.strip_prefix("イベント=")
                .map(str::trim)
                .filter(|s| !s.is_empty())
        })
    }
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

    // 属性行の次の非空行を効果行として読む (`SpecialPowerDataList.Load`: 効果は属性行直後)。
    let effects = it
        .find(|l| !l.text.is_empty())
        .map(|l| parse_effects(&l.text))
        .unwrap_or_default();

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
        effects,
    })
}

/// 効果行を `(effect_type, level)` のリストへ分解する。
///
/// 元: `SpecialPowerData.SetEffect` (`SpecialPowerData.cs`)。効果は
/// `GeneralLib.ToList` と同じく空白区切り (例: 挑発 の `挑発 命中率低下Lv5`)。
/// 旧形式との互換で `,` も区切りとして扱い、`"` で囲んだ部分は分割しない。
/// 各効果は `Lv` の左を type、右の数値を level とする
/// (例: `ダメージ増加Lv10` → `("ダメージ増加", 10.0)`)。`Lv` を含まない効果は
/// レベルを既定 0.0 とする (本移植が数値で参照するのは `Lv` 付き効果のみ)。
/// 値指定 (`=...`) を伴う効果は `イベント=生贄ルーチン` のように 1 要素のまま保持する。
fn parse_effects(line: &str) -> Vec<(String, f64)> {
    split_effect_list(line)
        .into_iter()
        .map(|item| match item.find("Lv") {
            Some(idx) => {
                let etype = item[..idx].trim().to_string();
                let level = item[idx + "Lv".len()..]
                    .trim()
                    .parse::<f64>()
                    .unwrap_or(0.0);
                (etype, level)
            }
            None => (item, 0.0),
        })
        .collect()
}

/// 空白 / `,` 区切りで効果リストを分割する (`"` 内は区切らない)。
fn split_effect_list(line: &str) -> Vec<String> {
    let mut items = Vec::new();
    let mut cur = String::new();
    let mut quoted = false;
    for c in line.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                cur.push(c);
            }
            c if !quoted && (c.is_whitespace() || c == ',') => {
                if !cur.is_empty() {
                    items.push(std::mem::take(&mut cur));
                }
            }
            c => cur.push(c),
        }
    }
    if !cur.is_empty() {
        items.push(cur);
    }
    items
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
        // 実 SRC sp.txt 形式: L1=name[,kana], L2=short,sp,target,duration,...,
        // L3=効果, L4=解説
        let src = "\
熱血, ねっけつ
ネツ, 30, 自分, 瞬間, -, -, 熱血
ダメージ増加Lv10
次の戦闘で敵に与えるダメージを2倍にする

魂, たましい
タマ, 55, 自分, 瞬間, -, -, 魂
ダメージ増加Lv15
次の戦闘で敵に与えるダメージを2.5倍にする
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
    fn parses_damage_increase_effects() {
        // 実フィクスチャ (スパロボ戦記/data/system/sp.txt) 準拠の効果行:
        //   熱血 → ダメージ増加Lv10、魂 → ダメージ増加Lv15、気合 → 気力増加Lv1。
        // 気合 は気力 (morale) 効果でありダメージ増加効果を持たない。
        let src = "\
気合, きあい
気, 30, 自分, 即効, -, -, 気合
気力増加Lv1
自分の気力を+10

熱血, ねっけつ
熱, 40, 自分, 攻撃, -, -, 熱血
ダメージ増加Lv10
次の戦闘で敵に与えるダメージを2倍にする

魂, たましい
魂, 60, 自分, 攻撃, -, -, 魂
ダメージ増加Lv15
次の戦闘で敵に与えるダメージを2.5倍にする
";
        let v = parse(src).unwrap();
        assert_eq!(v.len(), 3);
        assert_eq!(v[0].name, "気合");
        assert_eq!(v[0].effects, vec![("気力増加".to_string(), 1.0)]);
        assert_eq!(v[1].name, "熱血");
        assert_eq!(v[1].effects, vec![("ダメージ増加".to_string(), 10.0)]);
        assert_eq!(v[2].name, "魂");
        assert_eq!(v[2].effects, vec![("ダメージ増加".to_string(), 15.0)]);
    }

    #[test]
    fn effect_line_is_whitespace_separated() {
        // 実 sp.txt (srcall Data/System/sp.txt) の 挑発 / みがわり。
        let src = "\
挑発, ちょうはつ
挑, 35, 敵, 攻撃, -, -, 挑発
挑発 命中率低下Lv5
指定したユニットの攻撃を自分に引き付ける

みがわり
身, 10, 味方, みがわり, -, -, みがわり
みがわり
指定したユニットが受けた攻撃を一度だけ代わりに受ける
";
        let v = parse(src).unwrap();
        assert_eq!(
            v[0].effects,
            vec![("挑発".to_string(), 0.0), ("命中率低下".to_string(), 5.0)]
        );
        assert_eq!(v[1].effects, vec![("みがわり".to_string(), 0.0)]);
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
