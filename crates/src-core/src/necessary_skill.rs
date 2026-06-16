//! 必要技能 / 必要条件 (necessary skill / necessary condition) の評価器。
//!
//! SRC.Sharp `Unit.IsNecessarySkillSatisfied` / `IsNecessarySkillSatisfied2`
//! (`SRC.Sharp/SRC.NET/Unit.cs`) と書式仕様 (`必要技能.md`) の移植。
//!
//! 武器・アビリティ・ユニット用特殊能力の末尾に `(必要技能)` / `<必要条件>` の形で
//! 記述された条件を評価し、満たされた場合にのみ使用可能にするためのゲート判定を行う。
//!
//! # 文法
//! - 条件は半角スペース区切りで複数並べられる (AND)。
//! - `or` (大小無視) で区切った条件は OR グループ (どれか 1 つ満たせば良い)。
//! - 条件の前置詞: `+` (ステータスコマンド中は無条件成立) / `*` (召喚主を参照) /
//!   `!` (否定)。併用順は `+*!`。
//!
//! # フェイルオープン方針
//! 本実装でモデル化していない条件種別 (同調率・霊力・各種ステータス初期値など) は
//! 「満たされている」(`None` → 成立) と扱う。これは「ゲートが無かった従来挙動」と等価で、
//! 武器の*誤封印*を避けるための保守的な分岐 (`必要技能` 監査メモの教訓)。一方、
//! モデル化済みの種別 (パイロット技能・気力・瀕死・HP/EN・ランク・性別・レベル・
//! ユニット名/クラス・@地形・装備・隣接・状態) は原典準拠で fail-closed に判定する。

use crate::data::pilot::{PilotData, Sex};
use crate::db::GameDatabase;
use crate::pilot_instance::PilotInstance;
use crate::unit_instance::UnitInstance;

/// クラス/属性文字列の末尾に付いた `<必要条件> (必要技能)` を切り離す。
///
/// `(class_only, necessary_skill, necessary_condition)` を返す。SRC.Sharp
/// `UnitDataList` の武器属性パース (line 1331-1365) と同じ手順で、末尾が `)` なら
/// 必要技能、続けて末尾が `>` なら必要条件を剥がす。`(` `)` `<` `>` は ASCII なので
/// マルチバイト文字列でもバイト境界は安全。
/// ユニット用特殊能力 (必要技能.md §3) の値末尾に**スペース区切りで**付いた
/// `(必要技能)` / `<必要条件>` を切り離す。`(value_only, necessary_skill, necessary_condition)`。
///
/// `split_necessary` と違い**直前のスペースを必須**にする: 特殊能力の値はそれ自体が
/// `(...)` を含むことがある (例: 形態名 `ガンダム(MA)`、`変形=ガンダム(MA) ジ・O`)。
/// スペース無しの `値(…)` を必要技能と誤認して値を切り詰めると壊れるため、
/// 「` (…)` で終わる」場合だけ必要技能とみなす。SRC のデータ書式は必ず空白を挟む。
pub fn split_feature_necessary(value: &str) -> (String, String, String) {
    let mut buf = value.trim_end().to_string();
    let mut skill = String::new();
    let mut cond = String::new();
    // 末尾が ` (...)` (スペース＋括弧グループ) なら必要技能。
    if buf.ends_with(')') {
        if let Some(open) = buf.rfind('(') {
            if open > 0 && buf[..open].ends_with(' ') {
                skill = buf[open + 1..buf.len() - 1].trim().to_string();
                buf = buf[..open].trim_end().to_string();
            }
        }
    }
    // 末尾が ` <...>` なら必要条件。
    if buf.ends_with('>') {
        if let Some(open) = buf.rfind('<') {
            if open > 0 && buf[..open].ends_with(' ') {
                cond = buf[open + 1..buf.len() - 1].trim().to_string();
                buf = buf[..open].trim_end().to_string();
            }
        }
    }
    (buf, skill, cond)
}

pub fn split_necessary(raw: &str) -> (String, String, String) {
    let mut buf = raw.trim().to_string();
    let mut skill = String::new();
    let mut cond = String::new();

    // 必要技能: 末尾が ')'。
    if buf.ends_with(')') {
        if let Some(gt) = buf.find("> ") {
            // `<必要条件> (必要技能)` 形式: "> " より後ろが "(必要技能)"。
            let after = buf[gt + 2..].to_string();
            buf = buf[..gt + 1].trim().to_string();
            skill = paren_inner(&after, '(', ')');
        } else if let Some(op) = buf.find('(') {
            skill = buf[op + 1..]
                .strip_suffix(')')
                .unwrap_or(&buf[op + 1..])
                .trim()
                .to_string();
            buf = buf[..op].trim().to_string();
        }
    }

    // 必要条件: 末尾が '>'。
    if buf.ends_with('>') {
        if let Some(op) = buf.find('<') {
            cond = buf[op + 1..]
                .strip_suffix('>')
                .unwrap_or(&buf[op + 1..])
                .trim()
                .to_string();
            buf = buf[..op].trim().to_string();
        }
    }

    // 残余クラスの末尾カンマ (トークナイザ起因の余剰) を除去。
    let class = buf.trim_end_matches(',').trim().to_string();
    (class, skill, cond)
}

/// `open`〜`close` で囲まれた中身を取り出す (`(術Lv3)` → `術Lv3`)。
fn paren_inner(s: &str, open: char, close: char) -> String {
    match s.find(open) {
        Some(op) => s[op + 1..]
            .strip_suffix(close)
            .unwrap_or(&s[op + 1..])
            .trim()
            .to_string(),
        None => String::new(),
    }
}

/// 必要技能/必要条件の式 `expr` を `unit` について評価する。空文字は常に成立。
///
/// `IsNecessarySkillSatisfied` 移植。トークン列を AND-of-OR として解釈する。
pub fn is_satisfied(expr: &str, unit: &UnitInstance, db: &GameDatabase) -> bool {
    let toks: Vec<&str> = expr.split_whitespace().collect();
    if toks.is_empty() {
        return true;
    }
    let mut i = 0;
    while i < toks.len() {
        // i から始まる OR グループを評価する。
        let mut group_ok = eval_atom_satisfied(toks[i], unit, db);
        while i + 1 < toks.len() && toks[i + 1].eq_ignore_ascii_case("or") {
            i += 2;
            if i >= toks.len() {
                break; // 末尾 "or" (不正) — グループ確定。
            }
            group_ok = group_ok || eval_atom_satisfied(toks[i], unit, db);
        }
        if !group_ok {
            return false; // この AND グループが不成立。
        }
        i += 1;
    }
    true
}

/// 単一条件を評価し、判定不能 (未モデル種別) は成立扱い (fail-open)。
fn eval_atom_satisfied(atom: &str, unit: &UnitInstance, db: &GameDatabase) -> bool {
    eval_atom(atom, unit, db).unwrap_or(true)
}

/// 単一条件 (`IsNecessarySkillSatisfied2` 移植)。
/// `None` = 本実装で判定不能な種別 (fail-open 対象)。
fn eval_atom(atom: &str, unit: &UnitInstance, db: &GameDatabase) -> Option<bool> {
    // 前置詞: + (ステータスコマンド中は無条件成立)。本実装はステータスコマンド状態を
    // 追跡しないため接頭辞のみ剥がして通常評価する (戦闘中は剥がしても等価)。
    if let Some(rest) = atom.strip_prefix('+') {
        return eval_atom(rest, unit, db);
    }
    // 前置詞: * (召喚主を参照)。召喚主が居なければ不成立。
    if let Some(rest) = atom.strip_prefix('*') {
        let Some(sid) = unit.summoned_by.as_deref() else {
            return Some(false);
        };
        let Some(summoner) = db.unit_by_uid(sid) else {
            return None; // 解決不能 — fail-open。
        };
        return eval_atom(rest, summoner, db);
    }
    // 前置詞: ! (否定)。判定不能はそのまま伝播 (fail-open のまま)。
    if let Some(rest) = atom.strip_prefix('!') {
        return eval_atom(rest, unit, db).map(|b| !b);
    }

    // `LvN` でレベル指定を分離。無ければ必要レベル 1。
    let (sname, nlevel) = match atom.find("Lv") {
        Some(p) => (&atom[..p], parse_leading_int(&atom[p + 2..])),
        None => (atom, 1),
    };

    // 本実装で値を追跡しない種別は fail-open (None)。`eval_named` のパイロット技能
    // パスへ流すと「未所持=封印」と誤判定するため、ここで打ち切る。
    if is_unmodeled(sname) {
        return None;
    }

    // 名称が変化しない条件 (技能名のエイリアス解決不要)。
    if let Some(result) = eval_fixed(sname, nlevel, unit, db) {
        return result;
    }

    // 名称が変わりうる条件 (パイロット技能・名称/愛称・ユニット名/クラス・地形・
    // 装備・隣接・状態)。
    eval_named(sname, nlevel, unit, db)
}

/// 本実装でモデル化していない条件種別 (fail-open 対象)。
/// - ステータス系 (格闘/射撃/魔力/命中/回避/技量/反応 と各初期値): 攻撃力・命中値等の
///   閾値判定。C# はパイロット能力値で判定するが、本実装は閾値モデルを持たないため未対応。
/// - 同調率/霊力: パイロット技能扱いだが値を追跡しないため未対応。
/// - 生身: 人間ユニット判定を持たないため未対応。
fn is_unmodeled(sname: &str) -> bool {
    matches!(
        sname,
        "格闘"
            | "射撃"
            | "魔力"
            | "命中"
            | "回避"
            | "技量"
            | "反応"
            | "格闘初期値"
            | "射撃初期値"
            | "魔力初期値"
            | "命中初期値"
            | "回避初期値"
            | "技量初期値"
            | "反応初期値"
            | "生身"
            | "同調率"
            | "霊力"
    )
}

/// 名称が一意な条件群を評価する。
///
/// 戻り値は二段の `Option`:
/// - 外側 `None`  = この種別は扱わない → `eval_named` へ委譲。
/// - `Some(None)` = 扱うが判定不能 (fail-open)。
/// - `Some(Some(b))` = 確定結果。
fn eval_fixed(
    sname: &str,
    nlevel: i32,
    unit: &UnitInstance,
    db: &GameDatabase,
) -> Option<Option<bool>> {
    let ge = |slevel: i32| Some(Some(slevel >= nlevel));
    match sname {
        "レベル" => ge(main_pilot_level(unit, db)),
        "気力" => Some(Some(unit.morale >= 100 + nlevel * 10)),
        "瀕死" => {
            let max_hp = db.effective_max_hp(unit);
            Some(Some(current_hp(unit, db) <= max_hp / 4))
        }
        "ＨＰ" => {
            let max_hp = db.effective_max_hp(unit).max(1);
            // 10*HP/MaxHP >= nlevel  ⇔  10*HP >= nlevel*MaxHP
            Some(Some(current_hp(unit, db) * 10 >= nlevel as i64 * max_hp))
        }
        "ＥＮ" => {
            let max_en = db.effective_max_en(unit).max(1);
            Some(Some(current_en(unit, db) * 10 >= nlevel * max_en))
        }
        "ランク" => ge(unit.boss_rank),
        "男性" => Some(Some(any_pilot_sex(unit, db, Sex::Male))),
        "女性" => Some(Some(any_pilot_sex(unit, db, Sex::Female))),
        "アイテム" => Some(Some(true)), // 使い捨てアイテム表記用 (常に成立)。
        "当て身技" | "自動反撃" => Some(Some(false)), // アビリティ付与専用武器の非表示用。
        // ユニット位置: 明示設定 (current_area) がある時のみ判定、未設定は fail-open。
        "地上" | "空中" | "水中" | "水上" | "宇宙" | "地中" => {
            if unit.current_area.is_empty() {
                Some(None)
            } else {
                Some(Some(unit.current_area == sname))
            }
        }
        _ => None,
    }
}

/// パイロット技能・名称/クラス・地形・装備・隣接・状態。
fn eval_named(sname: &str, nlevel: i32, unit: &UnitInstance, db: &GameDatabase) -> Option<bool> {
    // 1. パイロット用特殊能力 / パイロット名称・愛称。
    let mut slevel = pilot_condition_level(unit, db, sname);

    // 2. ユニット名称 / 愛称 / クラス。
    if slevel == 0 {
        if let Some(d) = db.unit_by_name(&unit.unit_data_name) {
            let class_only = d.class.split('(').next().unwrap_or(&d.class).trim();
            if sname == d.name || sname == d.nickname || sname == class_only {
                slevel = 1;
            }
        }
    }

    // 3. 特殊形式 (地形 / 装備 / 隣接 / 状態)。
    if slevel == 0 {
        if let Some(terrain) = sname.strip_prefix('@') {
            match unit_terrain_name(unit, db) {
                Some(tn) => slevel = (tn == terrain) as i32,
                None => return None, // 出撃外/マップ無し — 判定不能。
            }
        } else if let Some(iname) = sname.strip_suffix("装備") {
            slevel = unit_has_item(unit, db, iname) as i32;
        } else if sname.ends_with("隣接") || sname.ends_with("マス以内") {
            match eval_proximity(unit, db, sname) {
                Some(b) => slevel = b as i32,
                None => return None, // 出撃外 — 判定不能。
            }
        } else if let Some(state) = sname.strip_suffix("状態") {
            slevel = unit.has_condition(state) as i32;
        }
    }

    Some(slevel >= nlevel)
}

// ─────────────────────────── ヘルパ ───────────────────────────

/// `"Lv"` 以降の先頭整数を取り出す (符号・数字)。失敗時 0 (C# `StrToDbl` 準拠)。
fn parse_leading_int(s: &str) -> i32 {
    let s = s.trim();
    let mut end = 0;
    for (i, c) in s.char_indices() {
        if c.is_ascii_digit() || (i == 0 && (c == '+' || c == '-')) {
            end = i + c.len_utf8();
        } else {
            break;
        }
    }
    s[..end].parse().unwrap_or(0)
}

fn current_hp(unit: &UnitInstance, db: &GameDatabase) -> i64 {
    (db.effective_max_hp(unit) - unit.damage).max(0)
}

fn current_en(unit: &UnitInstance, db: &GameDatabase) -> i32 {
    (db.effective_max_en(unit) - unit.en_consumed).max(0)
}

/// ユニットに搭乗する各パイロットの (静的データ, 実体) を列挙する。
fn pilots_on<'a>(
    unit: &UnitInstance,
    db: &'a GameDatabase,
) -> Vec<(&'a PilotData, Option<&'a PilotInstance>)> {
    let mut out: Vec<(&PilotData, Option<&PilotInstance>)> = Vec::new();
    for id in &unit.pilot_ids {
        if let Some(pi) = db.pilot_instance_by_id(id) {
            if let Some(pd) = db.pilot_by_name(&pi.pilot_data_name) {
                out.push((pd, Some(pi)));
            }
        }
    }
    if out.is_empty() {
        if let Some(pd) = db.pilot_by_name(&unit.pilot_name) {
            out.push((pd, None));
        }
    }
    out
}

fn main_pilot_level(unit: &UnitInstance, db: &GameDatabase) -> i32 {
    unit.pilot_ids
        .first()
        .and_then(|id| db.pilot_instance_by_id(id))
        .map(|pi| pi.level)
        .unwrap_or(0)
}

fn any_pilot_sex(unit: &UnitInstance, db: &GameDatabase, sex: Sex) -> bool {
    pilots_on(unit, db).iter().any(|(pd, _)| pd.sex == sex)
}

/// 文字列末尾付近の数字を技能レベルとして取り出す (`撃墜数Lv100` → 100 /
/// `念力` → 1)。`skill_level` / `feature_level` の部分一致規約に合わせる。
fn extract_level(s: &str) -> i32 {
    match s.rfind(|c: char| c.is_ascii_digit()) {
        Some(pos) => {
            let start = s[..=pos]
                .rfind(|c: char| !c.is_ascii_digit())
                .map(|i| i + 1)
                .unwrap_or(0);
            s[start..=pos].parse().unwrap_or(1)
        }
        None => 1,
    }
}

/// 1 人のパイロットが `base` 技能を持つレベル (静的 features + 実体 skills の最大)。
///
/// 技能の別名 (`術Lv3=魔法` の `魔法`) は value 側に入るため、name だけでなく value も
/// 部分一致で見る。これにより別名指定の必要技能 (`(魔法Lv3)`) を取りこぼさない
/// (value 一致時もレベルは name の `LvN` から解決)。多少緩めだが、緩い方向の誤判定は
/// 「ゲート無し＝従来挙動」と等価で誤封印を避ける安全側。
fn one_pilot_skill_level(pd: &PilotData, pi: Option<&PilotInstance>, base: &str) -> i32 {
    let mut lv = 0;
    for (n, v) in &pd.features {
        if n.contains(base) || v.contains(base) {
            lv = lv.max(extract_level(n));
        }
    }
    if let Some(pi) = pi {
        for s in &pi.skills {
            if s.contains(base) {
                lv = lv.max(extract_level(s));
            }
        }
    }
    lv
}

/// パイロット用特殊能力 / パイロット名称・愛称の条件レベルを返す。
///
/// サブ/サポートパイロットの技能も対象 (原典: メインに限らない)。オーラ/超能力/
/// 超感覚は全搭乗員のレベルを*加算* (原典準拠)、他技能は最大値を採る。
/// パイロット名称・愛称に一致すればレベル 1。
fn pilot_condition_level(unit: &UnitInstance, db: &GameDatabase, sname: &str) -> i32 {
    let pilots = pilots_on(unit, db);
    if pilots.is_empty() {
        return 0;
    }
    // 名称 / 愛称 一致 (専用条件)。
    for (pd, _) in &pilots {
        if sname == pd.name || sname == pd.nickname {
            return 1;
        }
    }
    let summed = matches!(sname, "オーラ" | "超能力" | "超感覚");
    let mut total = 0;
    let mut max = 0;
    for (pd, pi) in &pilots {
        let lv = one_pilot_skill_level(pd, *pi, sname);
        total += lv;
        max = max.max(lv);
    }
    if summed {
        total
    } else {
        max
    }
}

fn unit_terrain_name(unit: &UnitInstance, db: &GameDatabase) -> Option<String> {
    if unit.off_map {
        return None;
    }
    let map = db.map.as_ref()?;
    if unit.x >= map.width || unit.y >= map.height {
        return None;
    }
    let id = map.cell(unit.x, unit.y).terrain_id;
    db.terrain_by_id(id).map(|t| t.name.clone())
}

fn unit_has_item(unit: &UnitInstance, db: &GameDatabase, iname: &str) -> bool {
    unit.equipped_items
        .iter()
        .any(|name| name == iname || db.item_by_name(name).is_some_and(|it| it.class == iname))
}

/// `<ユニット>隣接` / `<ユニット>Nマス以内`。指定ユニットが味方かつ盤上に近接していれば成立。
/// 出撃外は判定不能 (`None`)。
fn eval_proximity(unit: &UnitInstance, db: &GameDatabase, sname: &str) -> Option<bool> {
    if unit.off_map {
        return None;
    }
    let (target, range) = if let Some(name) = sname.strip_suffix("隣接") {
        (name, 1u32)
    } else {
        let name = sname.strip_suffix("マス以内")?;
        // 末尾 1 桁が距離 (1..9)。
        let mut chars = name.char_indices().rev();
        let (di, dc) = chars.next()?;
        let r = dc.to_digit(10)?;
        (&name[..di], r)
    };
    let is_carrier = target == "母艦";
    for other in &db.unit_instances {
        if other.off_map || std::ptr::eq(other, unit) {
            continue;
        }
        if other.x == unit.x && other.y == unit.y {
            continue;
        }
        if !unit.party.is_ally_of(other.party) {
            continue;
        }
        let dist = unit.x.abs_diff(other.x) + unit.y.abs_diff(other.y);
        if dist > range {
            continue;
        }
        let matched = if is_carrier {
            db.unit_by_name(&other.unit_data_name)
                .is_some_and(|d| d.features.iter().any(|(n, _)| n.contains("母艦")))
        } else {
            other.unit_data_name == target
                || db
                    .pilot_instance_by_id(other.pilot_ids.first().map(String::as_str).unwrap_or(""))
                    .map(|p| p.pilot_data_name == target)
                    .unwrap_or(false)
                || other.pilot_name == target
        };
        if matched {
            return Some(true);
        }
    }
    Some(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_necessary_extracts_skill_from_class() {
        let (class, skill, cond) = split_necessary("武 (念力Lv3)");
        assert_eq!(class, "武");
        assert_eq!(skill, "念力Lv3");
        assert_eq!(cond, "");
    }

    #[test]
    fn split_feature_necessary_requires_space_before_paren() {
        // §3: 特殊能力の値末尾 ` (必要技能)` を剥がす。スペース必須。
        let (val, skill, cond) = split_feature_necessary("FormB (念力Lv3)");
        assert_eq!(val, "FormB");
        assert_eq!(skill, "念力Lv3");
        assert_eq!(cond, "");
        // スペース無しの値内 (...) は必要技能と誤認せず温存 (形態名 ガンダム(MA) 等)。
        let (val2, skill2, _) = split_feature_necessary("ガンダム(MA)");
        assert_eq!(val2, "ガンダム(MA)");
        assert_eq!(skill2, "");
        // 必要条件 ` <...>` も剥がす。
        let (val3, skill3, cond3) = split_feature_necessary("X <母艦3マス以内> (術Lv4)");
        assert_eq!(val3, "X");
        assert_eq!(skill3, "術Lv4");
        assert_eq!(cond3, "母艦3マス以内");
        // 条件なしの値はそのまま。
        let (val4, skill4, _) = split_feature_necessary("ただの値");
        assert_eq!(val4, "ただの値");
        assert_eq!(skill4, "");
    }

    #[test]
    fn split_necessary_extracts_condition_and_skill() {
        let (class, skill, cond) = split_necessary("格Ｐ無 <母艦3マス以内> (術Lv4)");
        assert_eq!(class, "格Ｐ無");
        assert_eq!(skill, "術Lv4");
        assert_eq!(cond, "母艦3マス以内");
    }

    #[test]
    fn split_necessary_condition_only() {
        let (class, skill, cond) = split_necessary("武 <瀕死>");
        assert_eq!(class, "武");
        assert_eq!(skill, "");
        assert_eq!(cond, "瀕死");
    }

    #[test]
    fn split_necessary_no_marker() {
        let (class, skill, cond) = split_necessary("格Ｐ無墜L20");
        assert_eq!(class, "格Ｐ無墜L20");
        assert_eq!(skill, "");
        assert_eq!(cond, "");
    }

    #[test]
    fn parse_leading_int_handles_digits_and_sign() {
        assert_eq!(parse_leading_int("3"), 3);
        assert_eq!(parse_leading_int("100abc"), 100);
        assert_eq!(parse_leading_int(""), 0);
        assert_eq!(parse_leading_int("xyz"), 0);
    }

    #[test]
    fn extract_level_reads_trailing_number() {
        assert_eq!(extract_level("撃墜数Lv100"), 100);
        assert_eq!(extract_level("念力"), 1);
        assert_eq!(extract_level("切り払いLv3"), 3);
    }
}
