//! `Data/unit.txt` (`Data/robot.txt`) のパース / Parser for `Data/unit.txt`.
//!
//! 元実装: `UnitDataList.Load` (`UnitDataList.cls:102`)。
//!
//! 1 レコードのおおまかな構造（v1 で対応する範囲）:
//!
//! ```text
//! {Name}[,{KanaName}]
//! {Nickname}[,{KanaName}],{Class},{PilotNum},{ItemNum}
//! {Transportation},{Speed},{Size},{Value},{ExpValue}
//! [特殊能力なし | 特殊能力 + 続く特殊能力リスト行]
//! {HP},{EN},{Armor},{Mobility}
//! {Adaption},{Bitmap}
//! ... 武器 / アビリティ ... (v1 では無視)
//! ```
//!
//! パーサは特殊能力以降を「HP 行 (3 個のカンマで全フィールド数値)」と
//! 「適応行 (4 文字 ASCII + カンマ + 任意のビットマップ名)」を heuristics で
//! 拾う方式で実装。特殊能力 (`名前=値` / 値無し裸名)・武器・(`===` 以降の)
//! アビリティも取り込む。

use serde::{Deserialize, Serialize};

use super::loader::{read_data_lines, split_records, SourceLine};
use super::pilot::{Adaption, ParseError};

/// ユニットサイズ / Unit size class.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Size {
    XL,
    LL,
    L,
    #[default]
    M,
    S,
    SS,
}

impl Size {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "XL" => Some(Self::XL),
            "LL" => Some(Self::LL),
            "L" => Some(Self::L),
            "M" => Some(Self::M),
            "S" => Some(Self::S),
            "SS" => Some(Self::SS),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::XL => "XL",
            Self::LL => "LL",
            Self::L => "L",
            Self::M => "M",
            Self::S => "S",
            Self::SS => "SS",
        }
    }

    /// サイズ序列 (XL=0 … SS=5)。`能力コピー` のサイズ制限 (2 段階以上の差を
    /// 禁止) 等、サイズ間の段階差を測るために使う。
    pub fn rank(self) -> i32 {
        match self {
            Self::XL => 0,
            Self::LL => 1,
            Self::L => 2,
            Self::M => 3,
            Self::S => 4,
            Self::SS => 5,
        }
    }

    /// 2 つのサイズの段階差 (絶対値)。
    pub fn step_diff(self, other: Self) -> i32 {
        (self.rank() - other.rank()).abs()
    }
}

/// 元 `WeaponData` クラスの主要フィールド。
///
/// 元の全フィールド (順): Name, Power, MinRange, MaxRange, Precision, Bullet,
/// ENConsumption, NecessaryMorale, Adaption (4 文字), Critical, Class,
/// NecessarySkill, NecessaryCondition。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WeaponData {
    pub name: String,
    /// 元: `.Power` (攻撃力)
    pub power: i64,
    pub min_range: i32,
    pub max_range: i32,
    /// 元: `.Precision` (命中補正)。"+5" のような符号付きも受理。
    pub precision: i32,
    /// 弾数 (-1 は無制限相当)。元 `.Bullet`。
    pub bullet: i32,
    /// 元: `.ENConsumption`。0 = 消費なし。
    pub en_consumption: i32,
    /// 元: `.NecessaryMorale`。0 = 制限なし。
    pub necessary_morale: i32,
    /// 元: `.Adaption` (4 文字 A/B/C/-/etc)。空文字なら未設定。
    pub adaption: String,
    /// 元: `.Critical` (クリティカル補正)。
    pub critical: i32,
    /// 元: `.Class` (属性: "実"/"B"/"必"/etc)。末尾の `<必要条件>` / `(必要技能)` は
    /// パース時に剥がして `extras` へ分離するため、ここには純粋な武器属性のみ残る。
    pub class: String,
    /// `[必要技能, 必要条件]`。SRC 武器書式 `… 武器属性 <必要条件> (必要技能)` の末尾。
    /// `necessary_skill()` / `necessary_condition()` でアクセスする (空なら制限なし)。
    pub extras: Vec<String>,
}

impl WeaponData {
    /// 必要技能 (満たさないと使用不可・一覧非表示)。空文字は制限なし。
    pub fn necessary_skill(&self) -> &str {
        self.extras.first().map(String::as_str).unwrap_or("")
    }

    /// 必要条件 (満たさないと使用不可だが一覧には表示)。空文字は制限なし。
    pub fn necessary_condition(&self) -> &str {
        self.extras.get(1).map(String::as_str).unwrap_or("")
    }
}

/// アビリティ (ユニット補助能力) のデータ。`===` 区切り以降の行
/// `名称, 効果, 射程, 回数, 消費ＥＮ, 必要気力, 属性 <必要条件> (必要技能)` を保持する。
/// 武器とは別系統 (`アビリティ.md` / `アビリティ効果.md` / `ユニットデータ.md`)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AbilityData {
    /// アビリティ名称 (末尾 `(...)` の非表示サフィックスも生のまま保持)。
    pub name: String,
    /// 効果文字列。複数効果は半角スペース区切り (例 `回復Lv2 治癒`)。`アビリティ効果.md`。
    pub effect: String,
    /// 最大射程。0 = 自分のみ / 召喚。最小射程は一律 0 (属性 援/小 で変化)。
    pub range: i32,
    /// 残り使用回数。`-` (無制限 / EN 消費制) は `None`。
    pub uses: Option<i32>,
    /// 消費 EN。`-` (EN 消費なし) は `None`。
    pub en_cost: Option<i32>,
    /// 必要気力。`-` (制限なし) は `None`。
    pub morale: Option<i32>,
    /// アビリティ属性 + 必要条件 + 必要技能 (生文字列、未解釈)。
    pub attributes: String,
}

impl AbilityData {
    /// 表示名 (末尾 `(...)` 非表示サフィックスを除去)。
    pub fn display_name(&self) -> &str {
        match self.name.split_once('(') {
            Some((head, _)) => head.trim_end(),
            None => self.name.as_str(),
        }
    }

    /// 射程 0 (自分のみ / 召喚) で対象選択が不要か。
    pub fn is_self_only(&self) -> bool {
        self.range <= 0
    }

    /// マップ型アビリティ (`Ｍ全` / `Ｍ投` / `Ｍ直` 等、属性に全角 `Ｍ` を含む)。
    /// 複数のユニットへ同時に効果を及ぼす (`マップ攻撃に関する属性.md`)。
    pub fn is_map_type(&self) -> bool {
        self.attributes.contains('Ｍ')
    }

    /// 全体型マップアビリティ (`Ｍ全`)。射程・座標を無視して盤上全体が対象。
    pub fn is_map_all(&self) -> bool {
        self.attributes.contains("Ｍ全")
    }

    /// 敵対象アビリティか (`脱` = 気力低下 / `除` = 特殊効果解除)。これらの属性を
    /// 持つアビリティは味方ではなく敵を対象に取る (`ユニットデータ.md` アビリティ属性)。
    pub fn targets_enemy(&self) -> bool {
        self.attributes.contains('脱') || self.attributes.contains('除')
    }

    /// `能力コピー` 効果を持つか (対象選択は味方だが効果は発動者自身に及ぶ特殊効果)。
    pub fn has_copy_effect(&self) -> bool {
        self.effect.contains("能力コピー")
    }
}

/// 元 `UnitData` の主要フィールド。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnitData {
    /// 元: `.Name`
    pub name: String,
    /// 元: `.KanaName`
    pub kana_name: String,
    /// 元: `.Nickname`
    pub nickname: String,
    /// 元: `.Class`
    pub class: String,
    /// 元: `.PilotNum`（負値は VB6 で「括弧付き指定」を意味する; ここでは絶対値で保持）
    pub pilot_num: i32,
    /// 元: `.ItemNum`
    pub item_num: i32,
    /// 元: `.Transportation`（"陸"/"空"/"海"/"宇"/組合せ）
    pub transportation: String,
    /// 元: `.Speed`
    pub speed: i32,
    /// 元: `.Size`
    pub size: Size,
    /// 元: `.Value`（修理費）
    pub value: i64,
    /// 元: `.ExpValue`
    pub exp_value: i32,
    /// 元: `.HP`
    pub hp: i64,
    /// 元: `.EN`
    pub en: i32,
    /// 元: `.Armor`
    pub armor: i64,
    /// 元: `.Mobility`
    pub mobility: i32,
    /// 元: `.Adaption`
    pub adaption: Adaption,
    /// 元: `.Bitmap` (空可)
    pub bitmap: String,
    /// 元: `.colWeaponData` (順序保持)
    pub weapons: Vec<WeaponData>,
    /// 「特殊能力」セクションの `名前=値` 行（順序保持）。VB6 では
    /// `UnitData.Feature(idx) / FeatureData(idx)` に対応。`値` は `=` 以降を
    /// trim しただけの生文字列（"非表示 ..." の prefix も保持）。
    #[serde(default)]
    pub features: Vec<(String, String)>,
    /// `===` 区切り以降のアビリティデータ (順序保持)。アビリティを持たない
    /// ユニットでは空。`アビリティ.md` / `アビリティ効果.md`。
    #[serde(default)]
    pub abilities: Vec<AbilityData>,
}

pub fn parse(src: &str) -> Result<Vec<UnitData>, ParseError> {
    let lines = read_data_lines(src);
    let records = split_records(&lines);
    records.iter().map(|r| parse_record(r)).collect()
}

/// レコード単位で寛容に解析する。不正な 1 レコードは読み飛ばし、解析できた
/// ユニットだけを返す。スキップしたレコードのエラーは第 2 要素で返す。
///
/// SRC.NET の `GUI.DataErrorMessage` は非致命的で、不正なユニット定義が 1 件
/// あっても残りの読み込みを続行する。実アーカイブには移動性能行を完全に欠く
/// 人間ユニット (RDCD `光夏海` 等) のような壊れたレコードが混在するため、
/// unit.txt 全体のロード中断を避けてこの挙動に倣う (フロントエンド堅牢性重視)。
pub fn parse_lenient(src: &str) -> (Vec<UnitData>, Vec<ParseError>) {
    let lines = read_data_lines(src);
    let records = split_records(&lines);
    let mut units = Vec::new();
    let mut errors = Vec::new();
    for r in &records {
        match parse_record(r) {
            Ok(u) => units.push(u),
            Err(e) => errors.push(e),
        }
    }
    (units, errors)
}

fn parse_record(record: &[SourceLine]) -> Result<UnitData, ParseError> {
    let mut it = record.iter();

    // L1: Name[,KanaName]
    let name_line = it.next().ok_or_else(|| err(0, "空のレコード"))?;
    let (name, kana_from_name) = match name_line.text.split_once(',') {
        Some((n, k)) => (n.trim().to_string(), Some(k.trim().to_string())),
        None => (name_line.text.clone(), None),
    };

    // L2: Nickname[,KanaName],Class,PilotNum,ItemNum
    let detail = it
        .next()
        .ok_or_else(|| err(name_line.line_num, "基本属性行が見つかりません。"))?;
    // 行末コンマ (`1 ,` 等) が生む末尾の空フィールドを除去してから解析。
    // 実シナリオ (mva06 等) で `愛称, タイプ, 1 ,` のように trailing comma が
    // 入ることがある。SRC.NET は VB6 の `Split` が trailing empty を返さない
    // 挙動に依存しており、こちらも同様に末尾の空要素を除去する。
    let mut fields: Vec<&str> = detail.text.split(',').map(str::trim).collect();
    while fields.last() == Some(&"") {
        fields.pop();
    }
    let (nickname, kana_from_detail, class, pilot_num_s, item_num_s) = match fields.len() {
        4 => (
            fields[0].to_string(),
            None,
            fields[1].to_string(),
            fields[2],
            fields[3],
        ),
        5 => (
            fields[0].to_string(),
            Some(fields[1].to_string()),
            fields[2].to_string(),
            fields[3],
            fields[4],
        ),
        n if n < 4 => return Err(err(detail.line_num, "設定に抜けがあります。")),
        _ => return Err(err(detail.line_num, "余分な「,」があります。")),
    };
    let kana_name = kana_from_detail
        .or(kana_from_name)
        .unwrap_or_else(|| nickname.clone());
    let pilot_num = parse_pilot_num(pilot_num_s, detail.line_num)?;
    let item_num: i32 = item_num_s
        .parse()
        .map_err(|_| err(detail.line_num, "アイテム数が数値ではありません。"))?;

    // L3: Transportation,Speed,Size,Value,ExpValue
    let mv = it
        .next()
        .ok_or_else(|| err(detail.line_num, "移動性能行が見つかりません。"))?;
    let mvf: Vec<&str> = mv.text.split(',').map(str::trim).collect();
    // 本来は 5 フィールド (移動地形, 移動力, サイズ, 修理費, 経験値)。
    // 一部の古いデータ (RDCD / TTGL 等) はサイズを省略した 4 フィールド形式
    // (`陸, 4, 5000, 180`)。SRC.NET (UnitDataList.cs:522-541) は不正/欠落サイズを
    // 既定 "M" にフォールバックして非致命的に処理するため、こちらも 4 フィールド
    // 行を救済し、unit.txt 全体のロード中断を避ける。
    //   - mvf[2] が正当なサイズ語 → 経験値省略形式 (..., サイズ, 修理費)
    //   - mvf[2] がサイズ語でない  → サイズ省略形式 (..., 修理費, 経験値) / Size=M
    let (transportation_s, speed_s, size_token, value_s, exp_value_s) = match mvf.len() {
        n if n >= 5 => (mvf[0], mvf[1], Some(mvf[2]), mvf[3], mvf[4]),
        4 if Size::parse(mvf[2]).is_some() => (mvf[0], mvf[1], Some(mvf[2]), mvf[3], "0"),
        4 => (mvf[0], mvf[1], None, mvf[2], mvf[3]),
        _ => return Err(err(mv.line_num, "移動性能の項目が不足しています。")),
    };
    let transportation = transportation_s.to_string();
    let speed: i32 = speed_s
        .parse()
        .map_err(|_| err(mv.line_num, "移動力が数値ではありません。"))?;
    let size = match size_token {
        Some(tok) => Size::parse(tok).ok_or_else(|| {
            err(
                mv.line_num,
                "サイズが不正です (XL/LL/L/M/S/SS のいずれか)。",
            )
        })?,
        // サイズ省略 → SRC.NET 既定の M で補完。
        None => Size::M,
    };
    let value: i64 = value_s
        .parse()
        .map_err(|_| err(mv.line_num, "修理費が数値ではありません。"))?;
    let exp_value: i32 = exp_value_s
        .parse()
        .map_err(|_| err(mv.line_num, "経験値が数値ではありません。"))?;

    // 残りの行から HP/EN/Armor/Mobility 行と Adaption 行を heuristics で拾う。
    // Adaption 行以降は武器データとして解釈する（VB6: ",===" or 空行で終端）。
    let mut hp = 0i64;
    let mut en = 0i32;
    let mut armor = 0i64;
    let mut mobility = 0i32;
    let mut adaption: Option<Adaption> = None;
    let mut bitmap = String::new();
    let mut weapons: Vec<WeaponData> = Vec::new();
    let mut features: Vec<(String, String)> = Vec::new();
    let mut abilities: Vec<AbilityData> = Vec::new();
    let mut stats_seen = false;
    let mut adaption_seen = false;
    let mut in_abilities = false;
    let mut in_features = false;

    for line in it {
        if line.text.is_empty() {
            continue;
        }
        if line.text == "===" {
            // `===` 以降はアビリティデータ (武器の終端でもある)。
            in_abilities = true;
            continue;
        }
        if in_abilities {
            // 行頭 `#` はコメント (ユニットデータ.md)。アビリティ行のみ採用。
            if line.text.starts_with('#') {
                continue;
            }
            if let Some(a) = try_parse_ability(&line.text) {
                abilities.push(a);
            }
            continue;
        }
        if !stats_seen {
            if let Some((h, e, a, m)) = try_parse_stats(&line.text) {
                hp = h;
                en = e;
                armor = a;
                mobility = m;
                stats_seen = true;
                continue;
            }
            // 「特殊能力」セクションマーカー: 以降の行を特殊能力として取り込む。
            let t = line.text.trim();
            if t == "特殊能力" {
                in_features = true;
                continue;
            }
            // 「特殊能力なし」等のマーカーは能力を持たないので積まない。
            if t == "特殊能力なし" || t == "全ユニット共通" {
                continue;
            }
            // `名前=値` 形式 (例: `分離=ユニットA ユニットB`)。マーカー有無に依らず採用。
            if let Some(feat) = try_parse_feature(&line.text) {
                features.push(feat);
                continue;
            }
            // 特殊能力セクション内の **値無し裸名** (例: 水上移動 / ＨＰ回復Lv1)。
            // SRC の特殊能力は値を持たないものが大半。`=` 必須だと全て取りこぼすため、
            // 「特殊能力」マーカー配下に限り裸名を value 空で取り込む。
            if in_features {
                features.push((t.to_string(), String::new()));
                continue;
            }
        }
        if stats_seen && !adaption_seen {
            if let Some((adp, bmp)) = try_parse_adaption(&line.text) {
                adaption = Some(adp);
                bitmap = bmp;
                adaption_seen = true;
                continue;
            }
        }
        if stats_seen && adaption_seen {
            // 武器候補行: name,power,min,max,precision,bullet[,extras...]
            if let Some(w) = try_parse_weapon(&line.text) {
                weapons.push(w);
                continue;
            }
            // 特殊能力 / アビリティ等は v1 では無視
        }
    }

    // SRC のシナリオには戦闘ステータスを持たない「データキャリア」ユニットが
    // 存在する (例: マップ情報、辞典データ)。これらは features のみを持ち、
    // `Info(ユニットデータ, X, 特殊能力データ, Y)` の引き先として使われる。
    // stats / adaption 行が無い場合はゼロ / "AAAA" を補ってパースを成功させる。
    let adaption = adaption.unwrap_or_else(|| Adaption::parse("AAAA").unwrap());

    Ok(UnitData {
        name,
        kana_name,
        nickname,
        class,
        pilot_num,
        item_num,
        transportation,
        speed,
        size,
        value,
        exp_value,
        hp,
        en,
        armor,
        mobility,
        adaption,
        bitmap,
        weapons,
        features,
        abilities,
    })
}

/// 「特殊能力」セクション内の `名前=値` 行を `(name, value)` として返す。
/// 行頭の `名前=` 以外（区切り無しの marker 行）は `None`。
fn try_parse_feature(text: &str) -> Option<(String, String)> {
    let (name, value) = text.split_once('=')?;
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    Some((name.to_string(), value.trim().to_string()))
}

fn try_parse_weapon(text: &str) -> Option<WeaponData> {
    // 武器行のフルフォーマット (省略あり):
    //   name, power, min, max, precision, bullet,
    //   en_consumption, necessary_morale, adaption, critical, class,
    //   necessary_skill, necessary_condition
    // 最低 6 フィールド (基本ステータス) があればパース成立。
    let toks: Vec<&str> = text.split(',').map(str::trim).collect();
    if toks.len() < 6 {
        return None;
    }
    let name = toks[0].to_string();
    if name.is_empty() {
        return None;
    }
    // 数値フィールドが整数として読めることを要件にして、武器以外（特殊能力等）と区別
    let power: i64 = toks[1].parse().ok()?;
    let min_range: i32 = toks[2].parse().ok()?;
    let max_range: i32 = toks[3].parse().ok()?;
    // precision は "+5" のような符号付きがあるので parse は + を受理する Rust の
    // 標準に任せる。"-" 単独は 0 にフォールバック (装備例外用)。
    let precision: i32 = parse_signed_or_dash(toks[4])?;
    let bullet: i32 = parse_signed_or_dash(toks[5]).unwrap_or(-1);

    let en_consumption = toks
        .get(6)
        .and_then(|s| parse_signed_or_dash(s))
        .unwrap_or(0);
    let necessary_morale = toks
        .get(7)
        .and_then(|s| parse_signed_or_dash(s))
        .unwrap_or(0);
    let adaption = toks.get(8).map(|s| (*s).to_string()).unwrap_or_default();
    let critical = toks
        .get(9)
        .and_then(|s| parse_signed_or_dash(s))
        .unwrap_or(0);
    // 武器属性フィールド (toks[10] 以降) は本来「武器属性 <必要条件> (必要技能)」が
    // 半角スペースで連結されている。トークナイザがカンマ分割するため、剰余トークンを
    // スペースで再結合して原典どおり 1 つの属性文字列に戻し、末尾の必要技能/条件を剥がす。
    let raw_class = if toks.len() > 10 {
        toks[10..].join(" ")
    } else {
        String::new()
    };
    let (class, necessary_skill, necessary_condition) =
        crate::necessary_skill::split_necessary(&raw_class);
    let class = if class == "-" { String::new() } else { class };
    let extras = if necessary_skill.is_empty() && necessary_condition.is_empty() {
        Vec::new()
    } else {
        vec![necessary_skill, necessary_condition]
    };
    Some(WeaponData {
        name,
        power,
        min_range,
        max_range,
        precision,
        bullet,
        en_consumption,
        necessary_morale,
        adaption,
        critical,
        class,
        extras,
    })
}

/// アビリティ行をパースする (`===` 以降)。
///   名称, 効果[, 射程, 回数, 消費ＥＮ, 必要気力, 属性 <必要条件> (必要技能)]
/// 最低 2 フィールド (名称 + 効果) で成立。回数/消費ＥＮ/必要気力 の `-` は「無し」。
fn try_parse_ability(text: &str) -> Option<AbilityData> {
    let toks: Vec<&str> = text.split(',').map(str::trim).collect();
    if toks.len() < 2 {
        return None;
    }
    let name = toks[0].to_string();
    let effect = toks[1].to_string();
    if name.is_empty() || effect.is_empty() {
        return None;
    }
    // 射程: 省略・`-` は 0 (自分のみ)。
    let range = toks
        .get(2)
        .and_then(|s| parse_signed_or_dash(s))
        .unwrap_or(0);
    let uses = toks.get(3).and_then(|s| parse_ability_opt(s));
    let en_cost = toks.get(4).and_then(|s| parse_ability_opt(s));
    let morale = toks.get(5).and_then(|s| parse_ability_opt(s));
    let attributes = if toks.len() > 6 {
        let joined = toks[6..].join(", ");
        let joined = joined.trim();
        // 属性なしの `-` プレースホルダは空文字に正規化。
        if joined == "-" {
            String::new()
        } else {
            joined.to_string()
        }
    } else {
        String::new()
    };
    Some(AbilityData {
        name,
        effect,
        range,
        uses,
        en_cost,
        morale,
        attributes,
    })
}

/// アビリティの 回数 / 消費ＥＮ / 必要気力 フィールド: `-`(半角/全角) と空は
/// `None`(無制限 / コストなし / 制限なし)、数値は `Some(n)`。
fn parse_ability_opt(s: &str) -> Option<i32> {
    let s = s.trim();
    if s.is_empty() || s == "-" || s == "－" {
        return None;
    }
    s.parse::<i32>().ok()
}

fn parse_signed_or_dash(s: &str) -> Option<i32> {
    if s == "-" || s.is_empty() {
        return Some(0);
    }
    s.parse::<i32>().ok()
}

fn parse_pilot_num(s: &str, line_num: usize) -> Result<i32, ParseError> {
    // 元は "(3)" のように括弧つきだと負値で保存していたが、ここでは絶対値だけ取る。
    let core = s
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();
    core.parse::<i32>()
        .map_err(|_| err(line_num, "パイロット数が数値ではありません。"))
}

/// HP,EN,Armor,Mobility 行を判定。3 個のカンマ区切り、全 4 フィールドが整数。
fn try_parse_stats(text: &str) -> Option<(i64, i32, i64, i32)> {
    let toks: Vec<&str> = text.split(',').map(str::trim).collect();
    if toks.len() != 4 {
        return None;
    }
    let hp: i64 = toks[0].parse().ok()?;
    let en: i32 = toks[1].parse().ok()?;
    let armor: i64 = toks[2].parse().ok()?;
    let mobility: i32 = toks[3].parse().ok()?;
    Some((hp, en, armor, mobility))
}

/// Adaption,Bitmap 行を判定。先頭フィールドが 4 文字 ASCII。
fn try_parse_adaption(text: &str) -> Option<(Adaption, String)> {
    let mut parts = text.splitn(2, ',');
    let first = parts.next()?.trim();
    let adp = Adaption::parse(first)?;
    let bmp = parts
        .next()
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    Some((adp, bmp))
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

    const SAMPLE: &str = "\
ブレイバー
ブレイバー,ぶれいばー,リアル系,1,4
陸宇,5,M,3000,400
特殊能力なし
3500,120,1200,110
AAAA,Braver.bmp

ゾルダII
ゾルダII,ぞるだつー,リアル系,1,2
陸,5,M,800,200
特殊能力なし
2400,80,900,80
BBBB,ZoldaII.bmp
";

    #[test]
    fn parses_bare_name_special_abilities() {
        // 特殊能力セクションの **値無し裸名** (水上移動 / ＨＰ回復Lv1) を取り込む。
        // `名前=値` 形式 (分離) も併存できる。マーカー行「特殊能力」は積まない。
        const BOSS: &str = "\
怪獣
怪獣,かいじゅう,敵,1,0
陸水,3,LL,10000,150
特殊能力
水上移動
ＨＰ回復Lv1
分離=パーツA パーツB
24000,200,1000,60
-AAA,boss.bmp
触手,1600,1,1,+20,-,-,-,AAAA,+0,実
";
        let units = parse(BOSS).expect("parse ok");
        let u = &units[0];
        let feat_names: Vec<&str> = u.features.iter().map(|(n, _)| n.as_str()).collect();
        assert!(
            feat_names.contains(&"水上移動"),
            "裸名 水上移動 を取り込むはず: {feat_names:?}"
        );
        assert!(
            feat_names.contains(&"ＨＰ回復Lv1"),
            "裸名 ＨＰ回復Lv1 を取り込むはず: {feat_names:?}"
        );
        // `名前=値` 形式は value も保持。
        let bunri = u
            .features
            .iter()
            .find(|(n, _)| n == "分離")
            .expect("分離 が無い");
        assert_eq!(bunri.1, "パーツA パーツB");
        // 「特殊能力」マーカー自体は能力として積まない。
        assert!(
            !feat_names.contains(&"特殊能力"),
            "マーカー行を能力にしてはいけない"
        );
        // 武器・ステータスも従来どおり読める。
        assert_eq!(u.hp, 24000);
        assert_eq!(u.weapons.len(), 1);
    }

    #[test]
    fn parses_two_units() {
        let units = parse(SAMPLE).expect("parse ok");
        assert_eq!(units.len(), 2);

        let braver = &units[0];
        assert_eq!(braver.name, "ブレイバー");
        assert_eq!(braver.nickname, "ブレイバー");
        assert_eq!(braver.kana_name, "ぶれいばー");
        assert_eq!(braver.class, "リアル系");
        assert_eq!(braver.pilot_num, 1);
        assert_eq!(braver.item_num, 4);
        assert_eq!(braver.transportation, "陸宇");
        assert_eq!(braver.speed, 5);
        assert_eq!(braver.size, Size::M);
        assert_eq!(braver.value, 3000);
        assert_eq!(braver.exp_value, 400);
        assert_eq!(braver.hp, 3500);
        assert_eq!(braver.en, 120);
        assert_eq!(braver.armor, 1200);
        assert_eq!(braver.mobility, 110);
        assert_eq!(braver.adaption.as_str(), "AAAA");
        assert_eq!(braver.bitmap, "Braver.bmp");

        let zolda = &units[1];
        assert_eq!(zolda.name, "ゾルダII");
        assert_eq!(zolda.hp, 2400);
        assert_eq!(zolda.adaption.as_str(), "BBBB");
    }

    #[test]
    fn parses_unit_with_inline_kana() {
        // 名称行に kana が併記される形式
        let src = "\
スーパーブレイバー,すーぱーぶれいばー
スーパーブレイバー,リアル系,1,4
陸宇,4,L,4000,500
特殊能力なし
4500,150,1500,100
AAAA,SuperBraver.bmp
";
        let units = parse(src).unwrap();
        assert_eq!(units.len(), 1);
        // 詳細行にも KanaName が無いので 名前行の kana が使われる
        assert_eq!(units[0].kana_name, "すーぱーぶれいばー");
    }

    #[test]
    fn parens_pilot_num_is_accepted() {
        let src = "\
合体ロボ
合体ロボ,スーパー系,(3),4
陸,4,L,5000,500
特殊能力なし
6000,150,2000,90
AAAA,Robo.bmp
";
        let units = parse(src).unwrap();
        assert_eq!(units[0].pilot_num, 3);
    }

    #[test]
    fn data_carrier_unit_without_stats_parses_with_zeros() {
        // SRC のシナリオには `マップ情報` / `辞典データ` のようなデータキャリア
        // ユニットがあり、`特殊能力` セクションだけを持つ。stats / adaption が
        // 無くてもエラーにせず、features を保持したまま 0 / "AAAA" で補完する。
        let src = "\
マップ情報
-, -, 1, 4
陸, 0, M, 0, 0
特殊能力
1.map=フィフスルナ宙域 宇宙 Map13.mid
2.map=サイド２宙域 宇宙 Map13.mid
";
        let units = parse(src).expect("should parse data-carrier");
        assert_eq!(units.len(), 1);
        let u = &units[0];
        assert_eq!(u.name, "マップ情報");
        assert_eq!(u.hp, 0);
        assert_eq!(u.adaption.as_str(), "AAAA");
        assert_eq!(u.features.len(), 2);
        assert_eq!(u.features[0].0, "1.map");
        assert_eq!(u.features[0].1, "フィフスルナ宙域 宇宙 Map13.mid");
    }

    #[test]
    fn parses_weapons_after_adaption() {
        let src = "\
ブレイバー
ブレイバー,ぶれいばー,リアル系,1,4
陸宇,5,M,3000,400
特殊能力なし
3500,120,1200,110
AAAA,Braver.bmp
バルカン砲,200,1,1,10,99
ビームライフル,2500,2,5,15,-1
===
";
        let u = &parse(src).unwrap()[0];
        assert_eq!(u.weapons.len(), 2);
        let w0 = &u.weapons[0];
        assert_eq!(w0.name, "バルカン砲");
        assert_eq!(w0.power, 200);
        assert_eq!(w0.min_range, 1);
        assert_eq!(w0.max_range, 1);
        assert_eq!(w0.precision, 10);
        assert_eq!(w0.bullet, 99);
        let w1 = &u.weapons[1];
        assert_eq!(w1.name, "ビームライフル");
        assert_eq!(w1.power, 2500);
        assert_eq!(w1.max_range, 5);
        assert_eq!(w1.bullet, -1);
    }

    #[test]
    fn parses_abilities_after_divider() {
        let src = "\
天使機
天使機,てんし,リアル系,1,4
陸宇,5,M,3000,400
特殊能力なし
3500,120,1200,110
AAAA,Angel.bmp
バルカン砲,200,1,1,10,99
===
天使の微笑み, 回復Lv2 治癒, 3, -, 70, -, Ｍ全
修理装置, 回復Lv3, 1, 5, -, -, -
# これはコメント, カンマ入り でもアビリティにしない
自爆(隠し), 自爆, 0, 1, 50, 120, -
";
        let u = &parse(src).unwrap()[0];
        // 武器は従来どおり ===の前のみ。
        assert_eq!(u.weapons.len(), 1);
        // コメント行 (#) は無視され、アビリティは 3 件。
        assert_eq!(u.abilities.len(), 3);

        let a0 = &u.abilities[0];
        assert_eq!(a0.name, "天使の微笑み");
        assert_eq!(a0.effect, "回復Lv2 治癒");
        assert_eq!(a0.range, 3);
        assert_eq!(a0.uses, None); // 回数 "-" = 無制限
        assert_eq!(a0.en_cost, Some(70));
        assert_eq!(a0.morale, None);
        assert_eq!(a0.attributes, "Ｍ全");

        let a1 = &u.abilities[1];
        assert_eq!(a1.name, "修理装置");
        assert_eq!(a1.effect, "回復Lv3");
        assert_eq!(a1.range, 1);
        assert_eq!(a1.uses, Some(5));
        assert_eq!(a1.en_cost, None);
        assert_eq!(a1.attributes, ""); // 属性 "-" は空に正規化

        let a2 = &u.abilities[2];
        assert_eq!(a2.name, "自爆(隠し)");
        assert_eq!(a2.display_name(), "自爆"); // (...) 非表示サフィックス除去
        assert_eq!(a2.effect, "自爆");
        assert_eq!(a2.range, 0);
        assert!(a2.is_self_only());
        assert_eq!(a2.uses, Some(1));
        assert_eq!(a2.en_cost, Some(50));
        assert_eq!(a2.morale, Some(120));
    }

    #[test]
    fn no_divider_means_no_abilities() {
        let src = "\
普通機
普通機,ふつう,リアル系,1,4
陸,5,M,3000,400
特殊能力なし
3500,120,1200,110
AAAA,Normal.bmp
バルカン砲,200,1,1,10,99
";
        let u = &parse(src).unwrap()[0];
        assert!(u.abilities.is_empty(), "=== 区切りが無ければアビリティ無し");
        assert_eq!(u.weapons.len(), 1);
    }

    #[test]
    fn parses_move_row_with_size_omitted() {
        // RDCD / TTGL 等の 4 フィールド移動性能行 (サイズ省略)。
        // `陸, 4, 5000, 180` → 修理費 5000 / 経験値 180 / サイズは既定 M。
        let src = "\
仮面ライダーディケイド
仮面ライダーディケイド,かめんらいだーでぃけいど,(世界の破壊者),1,2
陸, 4, 5000, 180
特殊能力なし
4000,100,1000,100
AAAA,Decade.bmp
";
        let units = parse(src).expect("4-field move row should parse");
        assert_eq!(units.len(), 1);
        let u = &units[0];
        assert_eq!(u.transportation, "陸");
        assert_eq!(u.speed, 4);
        assert_eq!(u.size, Size::M); // サイズ省略 → 既定 M
        assert_eq!(u.value, 5000); // 修理費
        assert_eq!(u.exp_value, 180); // 経験値
    }

    #[test]
    fn parses_move_row_with_exp_omitted() {
        // 4 フィールドでも mvf[2] が正当なサイズ語なら経験値省略形式と解釈する。
        let src = "\
テスト機
テスト機,てすとき,リアル系,1,2
陸, 4, L, 5000
特殊能力なし
4000,100,1000,100
AAAA,Test.bmp
";
        let u = &parse(src).expect("exp-omitted move row should parse")[0];
        assert_eq!(u.size, Size::L);
        assert_eq!(u.value, 5000);
        assert_eq!(u.exp_value, 0); // 経験値省略 → 0
    }
}
