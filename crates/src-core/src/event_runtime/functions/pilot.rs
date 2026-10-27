//! 組み込み関数: パイロットの能力値・技能の問い合わせ。

use super::*;

pub(super) fn eval(app: &App, c: &FnCall) -> Option<Option<String>> {
    Some(match c.canon.as_str() {
        "Nickname" => fn_nickname(app, c),
        "CountPilot" => fn_count_pilot(app, c),
        "PilotID" => fn_pilot_id(app, c),
        "Pilot" => fn_pilot(app, c),
        "Exp" => fn_exp(app, c),
        "Level" => fn_level(app, c),
        "SP" => fn_sp(app, c),
        "Plana" => fn_plana(app, c),
        "Skill" => fn_skill(app, c),
        _ => return None,
    })
}

fn fn_nickname(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Nickname(name)` — パイロット / ユニット / アイテムの愛称。
    // 同名の場合はパイロット優先。
    //
    // SRC 原典 (Expression `case "nickname"`) はアイテム形式
    // `Nickname(アイテム名, アイテム番号)` も受け付け、第 2 引数の番号で
    // 同名アイテムを識別したうえで `ItemData.Nickname` を返す。本移植の
    // `ItemData` は愛称フィールドを持たないため、第 1 引数からアイテムを
    // 解決し、その名称 (愛称代替) を返す。第 2 引数 (番号) は名称解決で
    // 一意に引けるため無視する。
    let n = fn_arg_value(app, args.first()?);
    let n = n.trim().trim_matches('"');
    if let Some(p) = app
        .database()
        .pilots
        .iter()
        .find(|p| p.name == n || p.nickname == n)
    {
        return Some(p.nickname.clone());
    }
    if let Some(u) = app.database().unit_by_name(n) {
        return Some(u.nickname.clone());
    }
    if let Some(it) = app.database().item_by_name(n) {
        // ItemData に愛称フィールドが無いため名称を愛称として返す。
        return Some(it.name.clone());
    }
    // ユニットインスタンス参照
    if let Some(inst) = find_unit(app, n) {
        if let Some(u) = app.database().unit_by_name(&inst.unit_data_name) {
            return Some(u.nickname.clone());
        }
    }
    Some(n.to_string())
}

fn fn_count_pilot(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `CountPilot(unit)` — 指定ユニットに搭乗中のパイロット数。
    // 本実装は単一パイロットのみモデル化しているので、パイロットが
    // 居れば 1、未搭乗 (pilot_name 空 / "パイロット不在") なら 0。
    let key = fn_arg_value(app, args.first()?);
    let n = find_unit(app, &key)
        .map(|inst| {
            let p = inst.pilot_name.trim();
            i32::from(!p.is_empty() && p != "パイロット不在")
        })
        .unwrap_or(0);
    Some(n.to_string())
}

fn fn_pilot_id(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `PilotID(unit_name [, index])` — SRC.Sharp `Unit/Unit.cs::PilotID`
    // 同等: 指定 unit の (index 番目の) pilot 名を返す。
    // 本実装は同名 pilot を区別する uid を持たないので pilot 名そのもの。
    // index = 0 / 省略時は main pilot (= UnitInstance.pilot_name)。
    let key = fn_arg_value(app, args.first()?);
    if let Some(inst) = find_unit(app, &key) {
        // index 1 以上は SRC では副パイロットだが、本実装は単一
        // pilot のみ。
        return Some(inst.pilot_name.clone());
    }
    // 未解決 → input そのまま返す
    Some(key)
}

fn fn_pilot(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Pilot(unit [, N])` — 当該ユニットに乗っているパイロットの N 番目
    // (1-indexed, 省略時は 1) のパイロット名。未解決 unit は空文字。
    // 引数は裸の識別子 (`Pilot(選択)` 等) も `fn_arg_value` で
    // script_var 解決する (`UnitID` / `PilotID` と同じ規約)。
    let unit_key = fn_arg_value(app, args.first()?);
    let Some(inst) = find_unit(app, &unit_key) else {
        return Some(String::new());
    };
    let idx: usize = args
        .get(1)
        .and_then(|s| fn_arg_value(app, s).parse().ok())
        .unwrap_or(1);
    if idx <= 1 {
        Some(inst.pilot_name.clone())
    } else {
        Some(String::new())
    }
}

fn fn_exp(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    Some(
        find_unit(app, args.first().copied().unwrap_or(""))
            .map(|u| u.total_exp.to_string())
            .unwrap_or_else(|| "0".to_string()),
    )
}

fn fn_level(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Level(pilot)` — SRC.Sharp Pilot.Level 相当。
    // 優先度: UnitInstance → PilotInstance → PilotData (level 1) → 0
    // SRC.Sharp: `Pilot.Level` は `PList.Add(name, level, ...)` で設定され
    // `LevelUp` で増加する。Rust では `PilotInstance.level` に対応。
    let key = fn_arg_value(app, args.first()?);
    let key = key.trim().trim_matches('"');
    // ① ユニット配置済み → UnitInstance 経由 (total_exp から level を計算)
    if let Some(inst) = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.pilot_name == key || matches_unit_handle(u, key))
    {
        // C# LevelUpCmd.cs: レベル上限は 99 (level_from_exp が担保)。
        let level = crate::pilot_instance::level_from_exp(inst.total_exp);
        return Some(level.to_string());
    }
    // ② PilotInstance があればその level を返す (ユニット未配置)
    if let Some(pi) = app
        .database()
        .pilot_instances
        .iter()
        .find(|p| p.pilot_data_name == key || p.id == key)
    {
        return Some(pi.level.to_string());
    }
    // ③ PilotData のみ定義 → level 1
    if app.database().pilot_by_name(key).is_some() {
        return Some("1".to_string());
    }
    Some("0".to_string())
}

fn fn_sp(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `SP(pilot_or_unit)` — 現在 SP。PilotData.sp - UnitInstance.sp_consumed。
    // 未解決は 0。
    let key = fn_arg_value(app, args.first()?);
    let key = key.trim().trim_matches('"');
    if let Some(inst) = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.pilot_name == key || matches_unit_handle(u, key))
    {
        let max_sp = app
            .database()
            .pilot_by_name(&inst.pilot_name)
            .and_then(|p| p.sp)
            .unwrap_or(0);
        let remaining = (max_sp - inst.sp_consumed).max(0);
        return Some(remaining.to_string());
    }
    // ユニット未配置だがパイロット定義があれば max_sp をそのまま返す
    if let Some(p) = app.database().pilot_by_name(key) {
        return Some(p.sp.unwrap_or(0).to_string());
    }
    Some("0".to_string())
}

fn fn_plana(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Plana(pilot)` — パイロットの霊力値。`Plana(pilot) = n` で設定され、
    // `UnitInstance.plana` に格納される。
    // ユニット未配置の場合は PilotInstance にフォールバック。
    let key = fn_arg_value(app, args.first()?);
    let key = key.trim().trim_matches('"');
    // ① UnitInstance から引く
    if let Some(u) = find_unit(app, key) {
        return Some(u.plana.to_string());
    }
    // ② PilotInstance にフォールバック (ユニット未配置)
    if let Some(pi) = app
        .database()
        .pilot_instances
        .iter()
        .find(|p| p.pilot_data_name == key || p.id == key)
    {
        return Some(pi.plana.to_string());
    }
    Some("0".to_string())
}

fn fn_skill(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Skill(pilot, skill_name)` — pilot が指定スキルを持っていれば
    // そのレベル値、無ければ 0。SRC.Sharp `Functions/Pilot.cs::Skill` 同等。
    // 優先順位: PilotInstance.skills → PilotData.features。
    if args.len() < 2 {
        return None;
    }
    let pilot_key = fn_arg_value(app, args[0]);
    let pilot_key = pilot_key.trim().trim_matches('"');
    let skill_name = fn_arg_value(app, args[1]);
    let skill_name = skill_name.trim().trim_matches('"');
    // 1) PilotInstance.skills をチェック (SetSkill で動的付与されたスキル)
    let inst_level = app
        .database()
        .pilot_instances
        .iter()
        .find(|p| p.id == pilot_key || p.pilot_data_name == pilot_key)
        .and_then(|pi| {
            pi.skills
                .iter()
                .find(|s| s.starts_with(skill_name))
                .map(|s| {
                    // "スキル名 N" → N, "スキル名" → 1
                    s[skill_name.len()..]
                        .trim()
                        .parse::<i64>()
                        .ok()
                        .unwrap_or(1)
                })
        });
    if let Some(lv) = inst_level {
        return Some(lv.to_string());
    }
    // 2) PilotData.features (静的データ)
    let pd = app
        .database()
        .pilots
        .iter()
        .find(|p| p.name == pilot_key || p.nickname == pilot_key)
        .or_else(|| {
            let inst = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, pilot_key))?;
            app.database()
                .pilots
                .iter()
                .find(|p| p.name == inst.pilot_name || p.nickname == inst.pilot_name)
        });
    let Some(pd) = pd else {
        return Some("0".to_string());
    };
    let level = pd
        .features
        .iter()
        .find(|(k, _)| k == skill_name)
        .map(|(_, v)| {
            if v.is_empty() {
                1
            } else {
                v.trim().parse::<i64>().unwrap_or(1)
            }
        })
        .unwrap_or(0);
    Some(level.to_string())
}
