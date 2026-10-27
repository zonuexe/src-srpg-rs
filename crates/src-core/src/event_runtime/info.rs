//! `Info()` 関数のデータ問い合わせ (ユニット・パイロット・アイテム・SP・マップ)。

use super::*;

/// 元 SRC の `Info([データ区分,]データ,情報種類,…)` を解釈する。
/// 引数文字列は既に `expand_vars` で展開済 (ネスト関数も解決済)。
///
/// 未対応の組合せは空文字列を返す（VB5 SRC では type error になるが、
/// シナリオ既存コードは Info() が空を返す前提でフォールバックを書いている
/// ことが多いので互換性優先）。
pub(super) fn info_query(app: &App, args: &[&str]) -> String {
    if args.is_empty() {
        return String::new();
    }
    // 第 1 引数がデータ区分キーワードか判定。クォートは剥がして比較。
    // 引数は評価済み (`Info(ユニット,乗せ換えユニット,愛称)` の裸の識別子も変数として
    // 解決されている)。
    let head = args[0].trim().trim_matches('"');
    let (kind, name, rest): (InfoKind, String, &[&str]) = match info_data_kind(head) {
        Some(kind) => {
            // マップ / オプション は data 名を持たず、続く全引数を rest として扱う。
            if matches!(kind, InfoKind::Map | InfoKind::Option) {
                (kind, String::new(), &args[1..])
            } else {
                let name = (args.get(1).copied().unwrap_or("")).to_string();
                (kind, name, &args[2..])
            }
        }
        None => {
            // データ区分省略: name から自動判定
            let name = head.to_string();
            let kind = detect_info_kind(app, &name);
            (kind, name, &args[1..])
        }
    };
    let info_kind = rest
        .first()
        .map(|s| s.trim().trim_matches('"').to_string())
        .unwrap_or_default();
    let sub = rest.get(1..).unwrap_or(&[]);
    info_dispatch(app, kind, &name, &info_kind, sub)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InfoKind {
    Unit,
    UnitData,
    Pilot,
    PilotData,
    NonPilot,
    Item,
    SpecialPower,
    Map,
    Option,
}

pub(super) fn info_data_kind(s: &str) -> Option<InfoKind> {
    match s {
        "ユニット" => Some(InfoKind::Unit),
        "ユニットデータ" => Some(InfoKind::UnitData),
        "パイロット" => Some(InfoKind::Pilot),
        "パイロットデータ" => Some(InfoKind::PilotData),
        "非戦闘員" => Some(InfoKind::NonPilot),
        "アイテム" => Some(InfoKind::Item),
        "スペシャルパワー" => Some(InfoKind::SpecialPower),
        "マップ" => Some(InfoKind::Map),
        "オプション" => Some(InfoKind::Option),
        _ => None,
    }
}

pub(super) fn detect_info_kind(app: &App, name: &str) -> InfoKind {
    let n = name.trim().trim_matches('"');
    if app
        .database()
        .unit_instances
        .iter()
        .any(|u| matches_unit_handle(u, n))
    {
        return InfoKind::Unit;
    }
    if app
        .database()
        .pilots
        .iter()
        .any(|p| p.name == n || p.nickname == n)
    {
        return InfoKind::PilotData;
    }
    if app.database().units.iter().any(|u| u.name == n) {
        return InfoKind::UnitData;
    }
    if app.database().items.iter().any(|i| i.name == n) {
        return InfoKind::Item;
    }
    InfoKind::UnitData
}

pub(super) fn info_dispatch(
    app: &App,
    kind: InfoKind,
    name: &str,
    info: &str,
    sub: &[&str],
) -> String {
    match kind {
        InfoKind::Unit => info_unit(app, name, info, sub),
        InfoKind::UnitData => info_unit_data(app, name, info, sub),
        InfoKind::Pilot => info_pilot(app, name, info, sub, true),
        InfoKind::PilotData => info_pilot(app, name, info, sub, false),
        InfoKind::Item => info_item(app, name, info, sub),
        InfoKind::Map => info_map(app, info, sub),
        InfoKind::Option => {
            // `Info(オプション, name)` — Option コマンドで設定された値を返す。
            // Option(name) が設定済みなら "On"、未設定なら "Off"。
            // SRC.Sharp `Other.cs::InfoOption` 相当。
            let opt_name = if info.is_empty() { name } else { info };
            let key = format!("Option({opt_name})");
            if app.script_var(&key).is_empty() {
                "Off".to_string()
            } else {
                "On".to_string()
            }
        }
        InfoKind::SpecialPower => info_special_power(app, name, info),
        // 非戦闘員は未実装: 空文字
        InfoKind::NonPilot => String::new(),
    }
}

/// `Info(ユニット, name, ...)` — UnitInstance + 装備込み補正値を優先。
pub(super) fn info_unit(app: &App, name: &str, info: &str, sub: &[&str]) -> String {
    let inst = match app
        .database()
        .unit_instances
        .iter()
        .find(|u| matches_unit_handle(u, name))
    {
        Some(u) => u,
        None => return String::new(),
    };
    let data = match app.database().unit_by_name(&inst.unit_data_name) {
        Some(d) => d,
        None => return String::new(),
    };
    match info {
        "ＨＰ" | "HP" => {
            let max = app.database().effective_max_hp(inst);
            (max - inst.damage).max(0).to_string()
        }
        "最大ＨＰ" | "MaxHP" => app.database().effective_max_hp(inst).to_string(),
        "ＥＮ" | "EN" => {
            let max = app.database().effective_max_en(inst);
            (max - inst.en_consumed).max(0).to_string()
        }
        "最大ＥＮ" | "MaxEN" => app.database().effective_max_en(inst).to_string(),
        "装甲" => app.database().effective_armor(inst).to_string(),
        "運動性" => app.database().effective_mobility(inst).to_string(),
        "移動力" => app.database().effective_speed(inst).to_string(),
        "気力" => inst.morale.to_string(),
        "累積経験値" => inst.total_exp.to_string(),
        "経験値" => inst.total_exp.to_string(),
        "アイテム数" => inst.equipped_item_names().len().to_string(),
        "アイテム" => {
            let names = inst.equipped_item_names();
            sub.first()
                .and_then(|s| s.to_string().parse::<usize>().ok())
                .and_then(|n| names.get(n.saturating_sub(1)))
                .map(|s| s.to_string())
                .unwrap_or_default()
        }
        // UnitData 由来の参照は base のヘルパに委譲
        _ => info_unit_data_inner(data, info, sub, app),
    }
}

/// `Info(ユニットデータ, name, ...)` — UnitData の固定値のみ。
pub(super) fn info_unit_data(app: &App, name: &str, info: &str, sub: &[&str]) -> String {
    let n = name.trim().trim_matches('"');
    // 仮想 instance を経由した名前指定にも対応 (Args(1) が unit_data_name の場合)
    let data = app.database().unit_by_name(n).or_else(|| {
        app.database()
            .unit_instances
            .iter()
            .find(|u| matches_unit_handle(u, n))
            .and_then(|u| app.database().unit_by_name(&u.unit_data_name))
    });
    let Some(data) = data else {
        return String::new();
    };
    info_unit_data_inner(data, info, sub, app)
}

pub(super) fn info_unit_data_inner(
    data: &crate::data::unit::UnitData,
    info: &str,
    sub: &[&str],
    app: &App,
) -> String {
    match info {
        "名称" => data.name.clone(),
        "愛称" => data.nickname.clone(),
        "読み仮名" => data.kana_name.clone(),
        "ユニットクラス" | "クラス" => data.class.clone(),
        "規定パイロット数" => data.pilot_num.to_string(),
        "最大アイテム数" => data.item_num.to_string(),
        "移動可能地形" => data.transportation.clone(),
        "移動力" => data.speed.to_string(),
        "サイズ" => data.size.label().to_string(),
        "修理費" => data.value.to_string(),
        "経験値" => data.exp_value.to_string(),
        "最大ＨＰ" | "ＨＰ" | "MaxHP" | "HP" => data.hp.to_string(),
        "最大ＥＮ" | "ＥＮ" | "MaxEN" | "EN" => data.en.to_string(),
        "装甲" => data.armor.to_string(),
        "運動性" => data.mobility.to_string(),
        "地形適応" => data.adaption.as_str().to_string(),
        "グラフィック" => data.bitmap.clone(),
        "武器数" => data.weapons.len().to_string(),
        "武器" => weapon_info(data, sub, app),
        "特殊能力数" => data.features.len().to_string(),
        "特殊能力" => feature_at(&data.features, sub.first(), app),
        "特殊能力名称" => feature_name(&data.features, sub.first(), app),
        "特殊能力所有" => feature_owned(&data.features, sub.first(), app),
        "特殊能力レベル" => feature_level(&data.features, sub.first(), app),
        "特殊能力データ" | "特殊能力解説" => {
            feature_data(&data.features, sub.first(), app)
        }
        "最大攻撃力" => data
            .weapons
            .iter()
            .map(|w| w.power)
            .max()
            .unwrap_or(0)
            .to_string(),
        "最長射程" => data
            .weapons
            .iter()
            .map(|w| w.max_range)
            .max()
            .unwrap_or(0)
            .to_string(),
        _ => String::new(),
    }
}

pub(super) fn weapon_info(data: &crate::data::unit::UnitData, sub: &[&str], _app: &App) -> String {
    let Some(first) = sub.first() else {
        return String::new();
    };
    let key = first.to_string();
    let weapon = if let Ok(idx) = key.parse::<usize>() {
        if idx == 0 || idx > data.weapons.len() {
            return String::new();
        }
        &data.weapons[idx - 1]
    } else {
        match data.weapons.iter().find(|w| w.name == key) {
            Some(w) => w,
            None => return String::new(),
        }
    };
    let Some(attr) = sub.get(1) else {
        return weapon.name.clone();
    };
    let attr = attr.trim().trim_matches('"');
    match attr {
        "名称" => weapon.name.clone(),
        "攻撃力" => weapon.power.to_string(),
        "最小射程" => weapon.min_range.to_string(),
        "最大射程" => weapon.max_range.to_string(),
        "命中率" | "命中" => weapon.precision.to_string(),
        "最大弾数" | "弾数" => weapon.bullet.to_string(),
        "消費ＥＮ" => weapon.en_consumption.to_string(),
        "必要気力" => weapon.necessary_morale.to_string(),
        "地形適応" => weapon.adaption.clone(),
        "クリティカル率" => weapon.critical.to_string(),
        "属性" => weapon.class.clone(),
        // 必要技能 → extras[0] / 必要条件 → extras[1] (生文字列を返す簡易対応。
        // 原典 Info(武器,…,必要技能) は満足判定の 1/0 を返すが、本ヘルパは静的 UnitData
        // のみでユニット実体が無いため文字列で代替する)。
        "必要技能" => weapon.necessary_skill().to_string(),
        "必要条件" => weapon.necessary_condition().to_string(),
        "使用可" | "修得" => "1".to_string(),
        _ => String::new(),
    }
}

pub(super) fn feature_at(feats: &[(String, String)], sub: Option<&&str>, _app: &App) -> String {
    let Some(idx) = sub.and_then(|s| s.to_string().parse::<usize>().ok()) else {
        return String::new();
    };
    if idx == 0 || idx > feats.len() {
        return String::new();
    }
    feats[idx - 1].0.clone()
}

pub(super) fn feature_name(feats: &[(String, String)], sub: Option<&&str>, _app: &App) -> String {
    // 番号指定 (n 番目の名称) も 名前指定 (= 名称そのもの) も同じ識別子を返す。
    let Some(arg) = sub else { return String::new() };
    let key = arg.to_string();
    if let Ok(idx) = key.parse::<usize>() {
        if idx == 0 || idx > feats.len() {
            return String::new();
        }
        return feats[idx - 1].0.clone();
    }
    if feats.iter().any(|(k, _)| k == &key) {
        return key;
    }
    String::new()
}

pub(super) fn feature_owned(feats: &[(String, String)], sub: Option<&&str>, _app: &App) -> String {
    let Some(arg) = sub else { return "0".into() };
    let key = arg.to_string();
    if feats.iter().any(|(k, _)| k == &key) {
        "1".into()
    } else {
        "0".into()
    }
}

pub(super) fn feature_level(feats: &[(String, String)], sub: Option<&&str>, _app: &App) -> String {
    let Some(arg) = sub else { return "0".into() };
    let key = arg.to_string();
    let entry = if let Ok(idx) = key.parse::<usize>() {
        feats.get(idx.saturating_sub(1))
    } else {
        feats.iter().find(|(k, _)| k == &key)
    };
    let Some((_, v)) = entry else {
        return "0".into();
    };
    // 値の末尾トークンがレベル数値ならそれを、無ければ 1 を返す。
    let last_num = v
        .split_whitespace()
        .rev()
        .find_map(|t| t.parse::<i32>().ok());
    last_num
        .map(|n| n.to_string())
        .unwrap_or_else(|| "1".into())
}

pub(super) fn feature_data(feats: &[(String, String)], sub: Option<&&str>, _app: &App) -> String {
    let Some(arg) = sub else { return String::new() };
    let key = arg.to_string();
    let entry = if let Ok(idx) = key.parse::<usize>() {
        feats.get(idx.saturating_sub(1))
    } else {
        feats.iter().find(|(k, _)| k == &key)
    };
    entry.map(|(_, v)| v.clone()).unwrap_or_default()
}

pub(super) fn info_pilot(
    app: &App,
    name: &str,
    info: &str,
    sub: &[&str],
    is_instance: bool,
) -> String {
    let n = name.trim().trim_matches('"');
    // `Info(パイロット, …)` (is_instance=true) はレベル成長後の `effective_pilot_data` を、
    // `Info(パイロットデータ, …)` (is_instance=false) は成長前の生 `PilotData` を引く。
    // SRC.Sharp Info も実体は `PList`、データは `PDList` (静的) と引き分ける
    // (`Info.cs`)。配置済みパイロットでも パイロットデータ は素のデータを返さねばならない。
    // どちらも nickname フォールバックする。
    let resolve = |key: &str| -> Option<crate::data::pilot::PilotData> {
        if is_instance {
            app.database().effective_pilot_data(key)
        } else {
            app.database().pilot_by_name(key).cloned()
        }
    };
    let data = resolve(n).or_else(|| {
        let real_name = app
            .database()
            .pilots
            .iter()
            .find(|p| p.nickname == n)
            .map(|p| p.name.clone())?;
        resolve(&real_name)
    });
    let Some(data) = data else {
        return String::new();
    };
    // パイロット (Instance) 側の補正情報は士気・経験値が UnitInstance に乗る。
    let inst = if is_instance {
        app.database()
            .unit_instances
            .iter()
            .find(|u| u.pilot_name == data.name || u.pilot_name == data.nickname)
    } else {
        None
    };
    match info {
        "名称" => data.name.clone(),
        "愛称" => data.nickname.clone(),
        "読み仮名" => data.kana_name.clone(),
        "性別" => match data.sex {
            Sex::Male => "男性".to_string(),
            Sex::Female => "女性".to_string(),
            // SRC.Sharp 準拠: 性別未指定は空文字 (`"-"` ではない)。
            // InfoFunctionTests.Info_PilotData_Sex_EmptySex_ReturnsEmpty 参照。
            Sex::Unspecified => String::new(),
        },
        "クラス" | "ユニットクラス" => data.class.clone(),
        "地形適応" => data.adaption.as_str().to_string(),
        "経験値" => data.exp_value.to_string(),
        "格闘" | "格闘基本値" => data.infight.to_string(),
        "射撃" | "射撃基本値" => data.shooting.to_string(),
        "命中" | "命中基本値" => data.hit.to_string(),
        "回避" | "回避基本値" => data.dodge.to_string(),
        "技量" | "技量基本値" => data.technique.to_string(),
        "反応" | "反応基本値" => data.intuition.to_string(),
        // 修正値・支援修正値 系は未実装 (0 固定)
        "格闘修正値"
        | "射撃修正値"
        | "命中修正値"
        | "回避修正値"
        | "技量修正値"
        | "反応修正値"
        | "格闘支援修正値"
        | "射撃支援修正値"
        | "命中支援修正値"
        | "回避支援修正値"
        | "技量支援修正値"
        | "反応支援修正値" => "0".to_string(),
        "性格" => data.personality.clone().unwrap_or_default(),
        "最大ＳＰ" => data.sp.map(|n| n.to_string()).unwrap_or_default(),
        "ＳＰ" => {
            // インスタンスがあれば現在 SP (max - consumed) を、無ければ最大値を返す。
            let max = data.sp.unwrap_or(0);
            let consumed = inst.map(|u| u.sp_consumed).unwrap_or(0);
            ((max - consumed).max(0)).to_string()
        }
        "グラフィック" => data
            .bitmap
            .clone()
            .unwrap_or_else(|| format!("{}.bmp", data.nickname)),
        "ＭＩＤＩ" => data.bgm.clone().unwrap_or_default(),
        "気力" => inst
            .map(|u| u.morale.to_string())
            .unwrap_or_else(|| "100".into()),
        "累積経験値" => inst
            .map(|u| u.total_exp.to_string())
            .unwrap_or_else(|| "0".into()),
        "レベル" => {
            // 累積経験値からレベルを算出 (SRC: 500 exp/level、level_from_exp)。
            let total = inst.map(|u| u.total_exp).unwrap_or(0);
            crate::pilot_instance::level_from_exp(total).to_string()
        }
        "特殊能力数" => data.features.len().to_string(),
        "特殊能力" => feature_at(&data.features, sub.first(), app),
        "特殊能力名称" => feature_name(&data.features, sub.first(), app),
        "特殊能力所有" => feature_owned(&data.features, sub.first(), app),
        "特殊能力レベル" => feature_level(&data.features, sub.first(), app),
        "特殊能力データ" | "特殊能力解説" => {
            feature_data(&data.features, sub.first(), app)
        }
        _ => String::new(),
    }
}

pub(super) fn info_item(app: &App, name: &str, info: &str, sub: &[&str]) -> String {
    let n = name.trim().trim_matches('"');
    let Some(item) = app.database().item_by_name(n) else {
        return String::new();
    };
    match info {
        "名称" => item.name.clone(),
        "アイテムクラス" => item.class.clone(),
        "装備個所" => item.part.clone(),
        "最大ＨＰ修正値" => item.hp_mod.to_string(),
        "最大ＥＮ修正値" => item.en_mod.to_string(),
        "装甲修正値" => item.armor_mod.to_string(),
        "運動性修正値" => item.mobility_mod.to_string(),
        "移動力修正値" => item.speed_mod.to_string(),
        "解説文" => item.comment.clone(),
        "特殊能力数" => item.features.len().to_string(),
        "特殊能力" => feature_at(&item.features, sub.first(), app),
        "特殊能力名称" => feature_name(&item.features, sub.first(), app),
        "特殊能力所有" => feature_owned(&item.features, sub.first(), app),
        "特殊能力レベル" => feature_level(&item.features, sub.first(), app),
        "特殊能力データ" | "特殊能力解説" => {
            feature_data(&item.features, sub.first(), app)
        }
        _ => String::new(),
    }
}

/// `Info(スペシャルパワー, name, info)` — 静的 `SpecialPowerData` を参照。
/// SRC.NET `Expression.cs` の `spd`(SPDList) 系 情報種類に対応。
/// 本実装が保持しないフィールド (解説文 / 適用条件 / アニメ / 効果* 等) は
/// 空文字を返す (graceful degradation)。
pub(super) fn info_special_power(app: &App, name: &str, info: &str) -> String {
    let n = name.trim().trim_matches('"');
    // SRC は名称で引くが、シナリオでは短縮名/読み仮名が使われることもあるため寛容に照合。
    let Some(sp) = app
        .database()
        .special_powers
        .iter()
        .find(|s| s.name == n || s.short_name == n || s.kana_name == n)
    else {
        return String::new();
    };
    match info {
        "名称" => sp.name.clone(),
        "読み仮名" => sp.kana_name.clone(),
        "短縮名" => sp.short_name.clone(),
        "消費ＳＰ" => sp.sp_consumption.to_string(),
        "対象" => sp.target_type.clone(),
        "持続期間" => sp.duration.clone(),
        _ => String::new(),
    }
}

pub(super) fn info_map(app: &App, info: &str, sub: &[&str]) -> String {
    // 時間帯はマップ不在でも返せるグローバル状態。
    if info == "時間帯" {
        return app.time_of_day().to_string();
    }
    let Some(map) = app.database().map.as_ref() else {
        return String::new();
    };
    match info {
        "幅" => map.width.to_string(),
        "高さ" => map.height.to_string(),
        "ファイル名" => String::new(),
        "時間帯" => app.time_of_day().to_string(), // (map 存在時も統一)
        _ => {
            // `Info(マップ, X, Y, ...)` 形式: info_kind ではなく X 座標が来た場合。
            // 第 1 sub = X, 第 2 = Y, 第 3 = attr
            let x: u32 = match info.parse() {
                Ok(v) => v,
                Err(_) => return String::new(),
            };
            let y: u32 = match sub.first().and_then(|s| s.to_string().parse().ok()) {
                Some(v) => v,
                None => return String::new(),
            };
            let attr = sub.get(1).map(|s| s.trim().trim_matches('"')).unwrap_or("");
            if x >= map.width || y >= map.height {
                return String::new();
            }
            let cell = map.cell(x, y);
            // 地形テーブル参照（シナリオ定義 → ビルトイン → デフォルト値）
            let db = app.database();
            let terrain_entry = db.terrain_by_id(cell.terrain_id);
            let builtin = crate::data::terrain::lookup(cell.terrain_id);
            match attr {
                "地形タイプ" => cell.terrain_id.to_string(),
                "地形名" => {
                    if let Some(t) = terrain_entry {
                        t.name.clone()
                    } else if let Some(t) = builtin {
                        t.name.to_string()
                    } else {
                        cell.terrain_id.to_string()
                    }
                }
                "ビットマップ名" => cell.bitmap_no.to_string(),
                "移動コスト" => db.terrain_move_cost(cell.terrain_id).to_string(),
                "回避修正" => db.terrain_hit_mod(cell.terrain_id).to_string(),
                "ダメージ修正" => db.terrain_damage_mod(cell.terrain_id).to_string(),
                "ＨＰ回復量" | "ＥＮ回復量" => "0".into(),
                "ユニットＩＤ" => app
                    .database()
                    .units_at(x, y)
                    .next()
                    .map(|u| u.unit_data_name.clone())
                    .unwrap_or_default(),
                _ => String::new(),
            }
        }
    }
}
