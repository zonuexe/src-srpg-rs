//! `Data/pilot.txt` のパース / Parser for `Data/pilot.txt`.
//!
//! 元実装: `PilotDataList.Load` (`PilotDataList.cls:121`)。
//! 1 レコードは少なくとも 3 行から成る（多くは数行〜十数行）:
//!
//! ```text
//! {Name}                                            // パイロット識別子
//! {Nickname},[{KanaName},][{Sex},]{Class},{Adaption},{ExpValue}
//! {Infight} {Shooting} {Hit} {Dodge} {Intuition} {Technique} [{Personality} [{SP}]]
//! [{BGM}]
//! [SpecialPower / Skill / Weapon ... 任意行]
//! ```
//!
//! 本パーサでは現状、先頭 3 行（名称・基本属性・能力値）と任意の BGM 行だけを
//! 取り込む。スペシャルパワー・特殊能力・武器の解析は後続フェーズで追加する。

use serde::{Deserialize, Serialize};

use super::loader::{read_data_lines, split_records, SourceLine};
use crate::settings::Settings;

/// 性別 / Pilot sex. 元 `.Sex` フィールド。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Sex {
    Male,
    Female,
    #[default]
    Unspecified,
}

impl Sex {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "男性" => Some(Sex::Male),
            "女性" => Some(Sex::Female),
            "-" => Some(Sex::Unspecified),
            _ => None,
        }
    }
}

/// 地形適応 (空, 陸, 海, 宇宙 の 4 文字) / Terrain adaption (e.g. "AAAA").
///
/// 各文字は `A`/`B`/`C`/`D`/`E` または `-`。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Adaption(pub [u8; 4]);

impl Adaption {
    /// 4 文字 ASCII を期待。失敗時は `None`。
    pub fn parse(s: &str) -> Option<Self> {
        if s.len() != 4 || !s.is_ascii() {
            return None;
        }
        let bytes = s.as_bytes();
        Some(Adaption([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    pub fn as_str(&self) -> &str {
        std::str::from_utf8(&self.0).unwrap_or("????")
    }
}

/// 1 つの精神コマンド（SP コマンド）の習得情報 / One learned spirit command.
///
/// `pilot.txt` の `ＳＰ` 行（`ＳＰ, <最大SP>, <cmd>[=<cost>], <level>, ...`）から
/// パースする。`cost` 省略時は `sp.txt`（`special_powers`）の既定値 or 組込み既定値を
/// 使用する（`App` 側で解決）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpiritCommand {
    /// コマンド名（例: "集中" / "ひらめき" / "熱血" / "必中"）。
    /// `combat.rs` / `condition.rs` の condition 名と一致させること。
    pub name: String,
    /// 消費 SP。`None` なら既定コスト（sp.txt or 組込みテーブル）を使う。
    pub cost: Option<i32>,
    /// 習得レベル。`level <= パイロットのレベル` で使用可能になる。
    pub level: i32,
}

/// 元 `PilotData` クラス / Pilot static data record.
///
/// 元クラスのフィールドのうち、現状で取り込むものだけを Rust 化している。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PilotData {
    /// 元: `.Name`
    pub name: String,
    /// 元: `.Nickname`
    pub nickname: String,
    /// 元: `.KanaName` (省略時は Nickname のひらがな化 — ここでは Nickname を流用)
    pub kana_name: String,
    /// 元: `.Sex`
    pub sex: Sex,
    /// 元: `.Class`
    pub class: String,
    /// 元: `.Adaption`
    pub adaption: Adaption,
    /// 元: `.ExpValue`
    pub exp_value: i32,
    /// 元: `.Infight`
    pub infight: i32,
    /// 元: `.Shooting`
    pub shooting: i32,
    /// 元: `.Hit`
    pub hit: i32,
    /// 元: `.Dodge`
    pub dodge: i32,
    /// 元: `.Intuition`
    pub intuition: i32,
    /// 元: `.Technique`
    pub technique: i32,
    /// 元: `.Personality` (省略可)
    pub personality: Option<String>,
    /// 元: `.SP` (省略可)
    pub sp: Option<i32>,
    /// 元: `.BGM` (省略可)
    pub bgm: Option<String>,
    /// 元: `.Bitmap` (顔グラ画像ファイル名)。実 SRC では `pilot.txt` 内に
    /// 直接指定されないため、`Nickname` をフォールバックとして利用する。
    pub bitmap: Option<String>,
    /// 「特殊能力」セクションの `名前=値` 行（順序保持）。
    #[serde(default)]
    pub features: Vec<(String, String)>,
    /// `ＳＰ` 行から取り込んだ精神コマンド列（習得順）。空ならコマンド無し。
    #[serde(default)]
    pub spirit_commands: Vec<SpiritCommand>,
}

/// パイロットデータパースエラー / Pilot data parse error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError {
    pub line_num: usize,
    pub message: String,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}行目: {}", self.line_num, self.message)
    }
}

impl std::error::Error for ParseError {}

/// `pilot.txt` 全体（UTF-8）からパイロットデータを取り出す。
/// Parse all pilot records from a `pilot.txt` source string (UTF-8).
pub fn parse(src: &str) -> Result<Vec<PilotData>, ParseError> {
    let lines = read_data_lines(src);
    let records = split_records(&lines);
    records.iter().map(|r| parse_record(r)).collect()
}

/// レコード単位で寛容に解析する。不正な 1 レコードは読み飛ばし、解析できた
/// パイロットだけを返す。スキップしたレコードのエラーは第 2 要素で返す。
///
/// `unit::parse_lenient` と同じ方針。SRC.NET の `GUI.DataErrorMessage` は
/// 非致命的で、不正なパイロット定義が 1 件あっても残りの読み込みを続行する。
/// 実アーカイブには README / 図鑑用 / テンプレートの `pilot.txt` や、地形適応・
/// 能力値行が欠けた壊れたレコードが混在するため、ファイル全体のロード中断を
/// 避けてこの挙動に倣う (フロントエンド堅牢性重視)。
pub fn parse_lenient(src: &str) -> (Vec<PilotData>, Vec<ParseError>) {
    let lines = read_data_lines(src);
    let records = split_records(&lines);
    let mut pilots = Vec::new();
    let mut errors = Vec::new();
    for r in &records {
        match parse_record(r) {
            Ok(p) => pilots.push(p),
            Err(e) => errors.push(e),
        }
    }
    (pilots, errors)
}

fn parse_record(record: &[SourceLine]) -> Result<PilotData, ParseError> {
    let mut it = record.iter();
    // 1 行目: 名称
    let name_line = it.next().ok_or_else(|| err(0, "空のレコード"))?;
    if name_line.text.contains(',') {
        return Err(err(name_line.line_num, "名称の設定が抜けています。"));
    }
    let name = name_line.text.clone();

    // 2 行目: Nickname,(KanaName,)(Sex,)Class,Adaption,ExpValue
    let detail = it
        .next()
        .ok_or_else(|| err(name_line.line_num, "基本属性行が見つかりません。"))?;
    let fields: Vec<&str> = detail.text.split(',').map(|s| s.trim()).collect();
    let (nickname, kana_name, sex, class, adaption_str, exp_str) = match fields.len() {
        4 => {
            let [a, b, c, d] = [fields[0], fields[1], fields[2], fields[3]];
            (
                a.to_string(),
                a.to_string(),
                Sex::Unspecified,
                b.to_string(),
                c,
                d,
            )
        }
        5 => {
            let nickname = fields[0].to_string();
            let (kana, sex) = if let Some(s) = Sex::parse(fields[1]) {
                (nickname.clone(), s)
            } else {
                (fields[1].to_string(), Sex::Unspecified)
            };
            (
                nickname,
                kana,
                sex,
                fields[2].to_string(),
                fields[3],
                fields[4],
            )
        }
        6 => {
            let nickname = fields[0].to_string();
            let kana = fields[1].to_string();
            let sex = Sex::parse(fields[2])
                .ok_or_else(|| err(detail.line_num, "性別の設定が間違っています。"))?;
            (
                nickname,
                kana,
                sex,
                fields[3].to_string(),
                fields[4],
                fields[5],
            )
        }
        n if n < 4 => return Err(err(detail.line_num, "設定に抜けがあります。")),
        _ => return Err(err(detail.line_num, "余分な「,」があります。")),
    };
    let adaption = Adaption::parse(adaption_str)
        .ok_or_else(|| err(detail.line_num, "地形適応は 4 文字で指定してください。"))?;
    let exp_value: i32 = exp_str
        .parse()
        .map_err(|_| err(detail.line_num, "経験値が数値ではありません。"))?;

    // 3 行目以降: 能力値行を heuristics で探す。
    // 実 SRC は "特殊能力" マーカ + フィーチャー群が detail と stats の間に
    // 挟まることがあるので、先頭の「数値 6 個（カンマ or 空白区切り）」の
    // 行を能力値行とみなす。
    let mut stats_line: Option<&SourceLine> = None;
    let mut consumed_to: usize = 0;
    let remaining: Vec<&SourceLine> = it.collect();
    for (idx, line) in remaining.iter().enumerate() {
        if let Some(_n) = try_stats_tokens(&line.text) {
            stats_line = Some(line);
            consumed_to = idx + 1;
            break;
        }
    }
    let stats = stats_line.ok_or_else(|| err(detail.line_num, "能力値行が見つかりません。"))?;
    let toks =
        try_stats_tokens(&stats.text).ok_or_else(|| err(stats.line_num, "能力値の解釈に失敗。"))?;
    let n = |idx: usize| -> Result<i32, ParseError> {
        toks[idx]
            .parse()
            .map_err(|_| err(stats.line_num, "能力値が数値ではありません。"))
    };
    let infight = n(0)?;
    let shooting = n(1)?;
    let hit = n(2)?;
    let dodge = n(3)?;
    let intuition = n(4)?;
    let technique = n(5)?;
    let personality = toks.get(6).map(|s| s.to_string());
    // 能力値行 8 トークン目の SP（旧式・任意）。`ＳＰ` 行が無いときのフォールバック。
    let sp_from_stats = toks.get(7).and_then(|s| s.parse::<i32>().ok());

    // 能力値行より後ろの任意行から顔グラ画像と BGM を拾う。
    // SRC pilot.txt の慣例では `th_Patchouli.bmp, th06_09.mid` のように
    // カンマ区切りで「顔グラ画像, BGM」を並べる (どちらか片方のみの行もある)。
    // `-` / `-.mid` / `-.bmp` は「指定なし」のプレースホルダなので無視する。
    let mut bgm: Option<String> = None;
    let mut bitmap: Option<String> = None;
    for extra in remaining.iter().skip(consumed_to) {
        if extra.text.is_empty() {
            continue;
        }
        for tok in extra.text.split(',') {
            let tok = tok.trim();
            if tok.is_empty() || tok == "-" || tok.starts_with("-.") {
                continue;
            }
            let lower = tok.to_ascii_lowercase();
            let is_image = lower.ends_with(".bmp")
                || lower.ends_with(".png")
                || lower.ends_with(".jpg")
                || lower.ends_with(".gif");
            if bitmap.is_none() && is_image {
                bitmap = Some(tok.to_string());
            } else if bgm.is_none() && looks_like_bgm(tok) {
                bgm = Some(tok.to_string());
            }
        }
    }

    // 能力値行より前にある「特殊能力」セクション内の `名前=値` 行を捕捉する。
    let mut features: Vec<(String, String)> = Vec::new();
    for pre in remaining.iter().take(consumed_to) {
        if let Some(feat) = parse_feature_line(&pre.text) {
            features.push(feat);
        }
    }

    // `ＳＰ` 行（`ＳＰ, <最大SP>, <cmd>[=<cost>], <level>, ...`）から精神コマンド列と
    // 最大 SP を取り込む。`ＳＰ` 行があれば最大 SP はそちらを優先（旧式の能力値行
    // 8 トークン目より新しい記法）。
    let mut spirit_commands: Vec<SpiritCommand> = Vec::new();
    let mut sp_from_line: Option<i32> = None;
    for line in remaining.iter() {
        if let Some((max_sp, cmds)) = parse_sp_line(&line.text) {
            sp_from_line = Some(max_sp);
            spirit_commands = cmds;
            break;
        }
    }
    let sp = sp_from_line.or(sp_from_stats);

    Ok(PilotData {
        name,
        nickname,
        kana_name,
        sex,
        class,
        adaption,
        exp_value,
        infight,
        shooting,
        hit,
        dodge,
        intuition,
        technique,
        personality,
        sp,
        bgm,
        bitmap,
        features,
        spirit_commands,
    })
}

/// `ＳＰ` / `SP` 行から「最大 SP」と精神コマンド列を取り出す。
///
/// 形式: `ＳＰ, <最大SP>, <cmd>[=<cost>], <level>, <cmd>[=<cost>], <level>, ...`
///
/// 先頭トークンが `ＳＰ`/`SP` でなければ `None`。`<最大SP>` が数値でなければ `None`。
/// 以降は `(コマンド[=コスト], 習得レベル)` のペアの繰り返し。レベル省略時は 1。
fn parse_sp_line(text: &str) -> Option<(i32, Vec<SpiritCommand>)> {
    let toks: Vec<&str> = text.split(',').map(str::trim).collect();
    let head = toks.first()?;
    if !matches!(*head, "ＳＰ" | "SP" | "ｓｐ" | "sp") {
        return None;
    }
    let max_sp = toks.get(1).and_then(|s| s.parse::<i32>().ok())?;
    let mut commands = Vec::new();
    let mut i = 2;
    while i < toks.len() {
        let cmd_tok = toks[i];
        if cmd_tok.is_empty() {
            i += 1;
            continue;
        }
        let (name, cost) = match cmd_tok.split_once('=') {
            Some((n, c)) => (n.trim().to_string(), c.trim().parse::<i32>().ok()),
            None => (cmd_tok.to_string(), None),
        };
        // 次トークンが習得レベル（数値）。無ければ 1。
        let level = toks
            .get(i + 1)
            .and_then(|s| s.parse::<i32>().ok())
            .unwrap_or(1);
        commands.push(SpiritCommand { name, cost, level });
        i += 2;
    }
    Some((max_sp, commands))
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

/// 「能力値行候補」: カンマまたは空白で区切ったとき先頭 6 トークンが整数なら
/// 能力値行とみなす。7 番目以降は personality / sp 等のオプションとして返す。
fn try_stats_tokens(text: &str) -> Option<Vec<String>> {
    let toks: Vec<String> = text
        .split(|c: char| c == ',' || c.is_ascii_whitespace())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();
    if toks.len() < 6 {
        return None;
    }
    for t in &toks[..6] {
        if t.parse::<i32>().is_err() {
            return None;
        }
    }
    Some(toks)
}

fn looks_like_bgm(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    lower.ends_with(".mid") || lower.ends_with(".mp3") || lower.ends_with(".wav")
}

fn err(line_num: usize, message: &str) -> ParseError {
    ParseError {
        line_num,
        message: message.to_string(),
    }
}

/// Settings 由来の補助情報（将来用フック）/ Placeholder for future settings-driven hooks.
///
/// 元 SRC ではメッセージ速度などが load 時には参照されないが、上位 API で
/// `parse_with` パターンを取れるよう将来差し込めるようにしておく。
#[doc(hidden)]
pub fn parse_with(src: &str, _settings: &Settings) -> Result<Vec<PilotData>, ParseError> {
    parse(src)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
リオ・カザミ
リオ,りお,男性,超能力者,SSSS,300
160 220 200 220 240 200 冷静 70
ブレイバー.mid

ガロ・ベルク
ガロ,男性,超能力者,SSSS,400
180 200 200 230 230 200
テーマＢ.mid
";

    #[test]
    fn parses_character_making_output_format() {
        // キャラメイキングの `召喚データ書き込み` (Include.eve) が書き出す
        // pilot.txt 形式: 先頭空行 2 / `特殊能力` セクション / `===` 区切り /
        // 末尾の捕獲武器行。これが parse を通ることを保証する。
        let src = "\n\nメイドキャラ\n\
                   めいど, 女性, 汎用, AABB, 100\n\
                   特殊能力\n\
                   成長タイプ標準, 1\n\
                   底力L1\n\
                   エースボーナス,1\n\
                   150, 140, 130, 120, 110, 100, 強気\n\
                   ＳＰ, 50, 熱血, 30, 必中, 20\n\
                   F00.bmp, -.mid\n\
                   ===\n\
                   ボーナス (撃墜数Lv50 メイドキャラ)\n\
                   ===\n\
                   捕獲, 0, 1, 1, +100, -, -, -, AAAA, +0, |攻 (メイドキャラ)\n";
        let pilots = parse(src).expect("キャラメイキング形式が parse を通る");
        assert_eq!(pilots.len(), 1);
        assert_eq!(pilots[0].name, "メイドキャラ");
        assert_eq!(pilots[0].sex, Sex::Female);
        assert_eq!(pilots[0].infight, 150);
        assert_eq!(pilots[0].personality.as_deref(), Some("強気"));
    }

    #[test]
    fn parses_two_records() {
        let pilots = parse(SAMPLE).expect("parse ok");
        assert_eq!(pilots.len(), 2);

        let hero = &pilots[0];
        assert_eq!(hero.name, "リオ・カザミ");
        assert_eq!(hero.nickname, "リオ");
        assert_eq!(hero.kana_name, "りお");
        assert_eq!(hero.sex, Sex::Male);
        assert_eq!(hero.class, "超能力者");
        assert_eq!(hero.adaption.as_str(), "SSSS");
        assert_eq!(hero.exp_value, 300);
        assert_eq!(hero.infight, 160);
        assert_eq!(hero.technique, 200);
        assert_eq!(hero.personality.as_deref(), Some("冷静"));
        assert_eq!(hero.sp, Some(70));
        assert_eq!(hero.bgm.as_deref(), Some("ブレイバー.mid"));

        let rival = &pilots[1];
        // 性別を含む 5 フィールド形式（KanaName 省略）
        assert_eq!(rival.name, "ガロ・ベルク");
        assert_eq!(rival.kana_name, "ガロ");
        assert_eq!(rival.sex, Sex::Male);
        assert_eq!(rival.class, "超能力者");
        assert_eq!(rival.adaption.as_str(), "SSSS");
        assert_eq!(rival.exp_value, 400);
        assert_eq!(rival.bgm.as_deref(), Some("テーマＢ.mid"));
    }

    #[test]
    fn parses_bitmap_and_bgm_from_combined_line() {
        // SRC pilot.txt の `顔グラ.bmp, BGM.mid` 行から bitmap / bgm を両方拾う。
        // musou202 の `th_Patchouli.bmp, th06_09.mid` と同形式。
        let src = "パチュリー・ノーレッジ\n\
                   パチュリー, ぱちゅりー, 女性, 魔女, AABA, 350\n\
                   115, 162, 138, 152, 149, 150, 普通\n\
                   ＳＰ, 59, 集中, 1\n\
                   th_Patchouli.bmp, th06_09.mid\n";
        let pilots = parse(src).expect("parse ok");
        assert_eq!(pilots.len(), 1);
        let p = &pilots[0];
        assert_eq!(p.name, "パチュリー・ノーレッジ");
        assert_eq!(p.nickname, "パチュリー");
        assert_eq!(p.bitmap.as_deref(), Some("th_Patchouli.bmp"));
        assert_eq!(p.bgm.as_deref(), Some("th06_09.mid"));
    }

    #[test]
    fn parses_spirit_commands_with_costs_and_levels() {
        // 東方夢想伝 霊夢の形式: 最大SP 後ろが「コマンド[=コスト], 習得レベル」の繰り返し。
        let src = "霊夢\n\
                   霊夢, れいむ, 女性, 巫女, AAAA, 200\n\
                   150, 160, 140, 160, 150, 140, 強気\n\
                   ＳＰ, 47, ひらめき=10, 1, 集中=20, 4, 熱血, 14, 幸運, 20, 覚醒, 29, 奇跡, 45\n\
                   th_Reimu.bmp, th_reimu.mid\n";
        let pilots = parse(src).expect("parse ok");
        assert_eq!(pilots.len(), 1);
        let p = &pilots[0];
        // 最大SP は ＳＰ 行から。
        assert_eq!(p.sp, Some(47));
        assert_eq!(p.spirit_commands.len(), 6);
        // コスト明示 / レベル
        assert_eq!(p.spirit_commands[0].name, "ひらめき");
        assert_eq!(p.spirit_commands[0].cost, Some(10));
        assert_eq!(p.spirit_commands[0].level, 1);
        assert_eq!(p.spirit_commands[1].name, "集中");
        assert_eq!(p.spirit_commands[1].cost, Some(20));
        assert_eq!(p.spirit_commands[1].level, 4);
        // コスト省略 → None（App 側で既定コスト解決）
        assert_eq!(p.spirit_commands[2].name, "熱血");
        assert_eq!(p.spirit_commands[2].cost, None);
        assert_eq!(p.spirit_commands[2].level, 14);
        assert_eq!(p.spirit_commands[5].name, "奇跡");
        assert_eq!(p.spirit_commands[5].level, 45);
        // 顔グラ / BGM は ＳＰ 行に影響されず後続行から拾える。
        assert_eq!(p.bitmap.as_deref(), Some("th_Reimu.bmp"));
    }

    #[test]
    fn parses_spirit_command_single_no_cost() {
        // パチュリー形式（コスト省略・1 コマンド）。
        let src = "パチュリー・ノーレッジ\n\
                   パチュリー, ぱちゅりー, 女性, 魔女, AABA, 350\n\
                   115, 162, 138, 152, 149, 150, 普通\n\
                   ＳＰ, 59, 集中, 1\n\
                   th_Patchouli.bmp, th06_09.mid\n";
        let pilots = parse(src).expect("parse ok");
        let p = &pilots[0];
        assert_eq!(p.sp, Some(59));
        assert_eq!(p.spirit_commands.len(), 1);
        assert_eq!(p.spirit_commands[0].name, "集中");
        assert_eq!(p.spirit_commands[0].cost, None);
        assert_eq!(p.spirit_commands[0].level, 1);
    }

    #[test]
    fn pilot_without_sp_line_has_no_spirit_commands() {
        // 旧式（能力値行 8 トークン目に SP、ＳＰ 行なし）。
        let pilots = parse(SAMPLE).expect("parse ok");
        assert_eq!(pilots[0].sp, Some(70)); // 能力値行から
        assert!(pilots[0].spirit_commands.is_empty());
    }

    #[test]
    fn name_with_comma_is_rejected() {
        let src = "Foo,Bar\nBaz,Class,AAAA,100\n10 10 10 10 10 10\n";
        let err = parse(src).unwrap_err();
        assert!(err.message.contains("名称"));
    }

    #[test]
    fn missing_stats_is_rejected() {
        let src = "Pilot\nNick,Class,AAAA,100\n";
        let err = parse(src).unwrap_err();
        assert!(err.message.contains("能力値"));
    }

    #[test]
    fn parse_lenient_skips_bad_record_keeps_good() {
        // 先頭に壊れたレコード (能力値行欠落) があっても、後続の正常レコードは
        // 取り込む。strict `parse` だと先頭の Err でファイル全体が失われていた
        // (実アーカイブの README/図鑑/テンプレ pilot.txt 混在対策)。
        let src = "\
壊れたパイロット
ニック,Class,AAAA,100

リオ・カザミ
リオ,りお,男性,超能力者,SSSS,300
160 220 200 220 240 200 冷静 70
ブレイバー.mid
";
        // strict はファイル全体を失う。
        assert!(parse(src).is_err());
        // lenient は正常レコードを救済し、エラーも 1 件返す。
        let (pilots, errors) = parse_lenient(src);
        assert_eq!(pilots.len(), 1, "正常なリオは取り込まれる");
        assert_eq!(pilots[0].name, "リオ・カザミ");
        assert_eq!(errors.len(), 1, "壊れたレコードは 1 件スキップ");
    }
}
