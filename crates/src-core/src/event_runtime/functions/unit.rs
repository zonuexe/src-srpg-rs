//! 組み込み関数: ユニットの状態・位置・所持品の問い合わせ。

use super::*;

pub(super) fn eval(app: &App, c: &FnCall) -> Option<Option<String>> {
    Some(match c.canon.as_str() {
        "HP" => fn_hp(app, c),
        "MaxHP" => fn_max_hp(app, c),
        "EN" => fn_en(app, c),
        "MaxEN" => fn_max_en(app, c),
        "Armor" => fn_armor(app, c),
        "Mobility" => fn_mobility(app, c),
        "Speed" => fn_speed(app, c),
        "Action" => fn_action(app, c),
        "Damage" => fn_damage(app, c),
        "Condition" => fn_condition(app, c),
        "Status" => fn_status(app, c),
        "Bullet" => fn_bullet(app, c),
        "MaxBullet" => fn_max_bullet(app, c),
        "HasItem" => fn_has_item(app, c),
        "HasStatus" => fn_has_status(app, c),
        "TerrainId" => fn_terrain_id(app, c),
        "Rank" => fn_rank(app, c),
        "Partner" => fn_partner(app, c),
        "CountPartner" => fn_count_partner(app, c),
        "IsEquiped" | "IsEquipped" => fn_is_equiped(app, c),
        "SpecialPower" | "Mind" => fn_special_power(app, c),
        "CountItem" => fn_count_item(app, c),
        "WX" | "WY" => fn_wx(app, c),
        "UnitID" => fn_unit_id(app, c),
        "Item" => fn_item(app, c),
        "ItemID" => fn_item_id(app, c),
        "ItemIndex" => fn_item_index(app, c),
        "Unit" => fn_unit(app, c),
        "Party" => fn_party(app, c),
        "Morale" => fn_morale(app, c),
        "Relation" => fn_relation(app, c),
        "X" => fn_x(app, c),
        "Y" => fn_y(app, c),
        "Area" => fn_area(app, c),
        "Distance" => fn_distance(app, c),
        "Count" => fn_count(app, c),
        "Exists" => fn_exists(app, c),
        "IsAvailable" => fn_is_available(app, c),
        "IsDefined" => fn_is_defined(app, c),
        _ => return None,
    })
}

// 以下 unit-query 関数群は SRC.Sharp `AUnitFunction` 同等の規約:
// unit が解決できなければ **数値 0 (= "0")** を返す (literal を残さない)。
// 旧実装は `find_unit().map(...)` で None フォールスルーしていたため、
// 未解決 unit で式が展開されず literal が残る問題があった。
fn fn_hp(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    Some(
        find_unit(app, args.first().copied().unwrap_or(""))
            .map(|u| {
                let max = app.database().effective_max_hp(u);
                (max - u.damage).max(0).to_string()
            })
            .unwrap_or_else(|| "0".to_string()),
    )
}

fn fn_max_hp(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    Some(
        find_unit(app, args.first().copied().unwrap_or(""))
            .map(|u| app.database().effective_max_hp(u).to_string())
            .unwrap_or_else(|| "0".to_string()),
    )
}

fn fn_en(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    Some(
        find_unit(app, args.first().copied().unwrap_or(""))
            .map(|u| {
                let max = app.database().effective_max_en(u);
                (max - u.en_consumed).max(0).to_string()
            })
            .unwrap_or_else(|| "0".to_string()),
    )
}

fn fn_max_en(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    Some(
        find_unit(app, args.first().copied().unwrap_or(""))
            .map(|u| app.database().effective_max_en(u).to_string())
            .unwrap_or_else(|| "0".to_string()),
    )
}

fn fn_armor(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    Some(
        find_unit(app, args.first().copied().unwrap_or(""))
            .map(|u| app.database().effective_armor(u).to_string())
            .unwrap_or_else(|| "0".to_string()),
    )
}

fn fn_mobility(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    Some(
        find_unit(app, args.first().copied().unwrap_or(""))
            .map(|u| app.database().effective_mobility(u).to_string())
            .unwrap_or_else(|| "0".to_string()),
    )
}

fn fn_speed(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    Some(
        find_unit(app, args.first().copied().unwrap_or(""))
            .map(|u| app.database().effective_speed(u).to_string())
            .unwrap_or_else(|| "0".to_string()),
    )
}

fn fn_action(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Action(unit)` — 残り行動数。`has_acted` で 0/1 判定。
    // SRC.Sharp は二回行動等を考慮するが、本実装は最大 1 で十分。
    // 未解決 unit は 0 (= 行動不能扱い)。
    let key = args.first()?.to_string();
    let n = find_unit(app, &key)
        .map(|u| if u.has_acted { 0 } else { 1 })
        .unwrap_or(0);
    Some(n.to_string())
}

fn fn_damage(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Damage(unit)` — 損傷率 %  = damage * 100 / max_hp。
    // 0 = 無傷、100 = 破壊済。未解決 unit は 0。
    let key = args.first()?.to_string();
    let pct = find_unit(app, &key)
        .map(|u| {
            let max = app.database().effective_max_hp(u);
            if max <= 0 {
                0
            } else {
                (u.damage * 100 / max).clamp(0, 100) as i32
            }
        })
        .unwrap_or(0);
    Some(pct.to_string())
}

fn fn_condition(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Condition(unit, status)` — unit が status の影響下なら 1。
    // status は精神/状態異常名 (`熱血` / `麻痺` 等)。`HasStatus` の
    // alias でもあるが原典の関数名はこちらが正典。
    if args.len() < 2 {
        return None;
    }
    let key = args[0].to_string();
    let status = args[1].to_string();
    let status = status.trim().trim_matches('"');
    let has = find_unit(app, &key)
        .map(|u| u.has_condition(status))
        .unwrap_or(false);
    Some(if has { "1" } else { "0" }.to_string())
}

fn fn_status(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Status(unit)` — ユニットの状態文字列 (SRC `ユニット情報関数.md`):
    //   - `出撃` ─ マップ上に存在
    //   - `待機` ─ 出撃していない
    //   - `格納` ─ 母艦に収納されている
    //   - `離脱` ─ `Leave` コマンドで戦線から離れている
    //   - `破壊` ─ 破壊されている (unit_instances に残しているケース)
    //   - `破棄` ─ `RemoveUnit` / `RemovePilot` で削除されている
    // 優先順: 明示的に life_state がセットされていればそれ、未設定なら
    // off_map から `出撃` / `待機` を自動判定、未解決なら `破棄`。
    let key = args.first()?.to_string();
    Some(match find_unit(app, &key) {
        Some(u) if !u.life_state.is_empty() => u.life_state.clone(),
        Some(u) if u.off_map => "待機".to_string(),
        Some(_) => "出撃".to_string(),
        None => "破棄".to_string(),
    })
}

fn fn_bullet(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Bullet(unit, weapon)` — 残弾数。-1 (無限弾) の武器は -1 を返す。
    // 未解決 unit / 武器名不一致は 0。
    if args.len() < 2 {
        return None;
    }
    let key = args[0].to_string();
    let wname = args[1].to_string();
    let wname = wname.trim().trim_matches('"');
    let n = find_unit(app, &key)
        .and_then(|u| {
            app.database()
                .unit_by_name(&u.unit_data_name)
                .and_then(|d| d.weapons.iter().enumerate().find(|(_, w)| w.name == wname))
                .map(|(i, w)| {
                    if w.bullet < 0 {
                        -1
                    } else {
                        u.weapons
                            .iter()
                            .find(|uw| uw.weapon_index == i)
                            .map_or(w.bullet, |uw| uw.bullet_remaining)
                    }
                })
        })
        .unwrap_or(0);
    Some(n.to_string())
}

fn fn_max_bullet(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `MaxBullet(unit, weapon)` — 武器の最大弾数。データ参照のみ。
    if args.len() < 2 {
        return None;
    }
    let key = args[0].to_string();
    let wname = args[1].to_string();
    let wname = wname.trim().trim_matches('"');
    let n = find_unit(app, &key)
        .and_then(|u| {
            app.database()
                .unit_by_name(&u.unit_data_name)
                .and_then(|d| d.weapons.iter().find(|w| w.name == wname))
                .map(|w| w.bullet)
        })
        .unwrap_or(0);
    Some(n.to_string())
}

fn fn_has_item(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    if args.len() < 2 {
        return None;
    }
    let unit = find_unit(app, args[0])?;
    let item = args[1].trim().trim_matches('"');
    let has = unit.has_item_equipped(item);
    Some(if has { "1" } else { "0" }.to_string())
}

fn fn_has_status(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    if args.len() < 2 {
        return None;
    }
    let unit = find_unit(app, args[0])?;
    let status = args[1].trim().trim_matches('"');
    let has = unit.has_condition(status);
    Some(if has { "1" } else { "0" }.to_string())
}

fn fn_terrain_id(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // 範囲外 / 引数不正 / マップ未定義 はすべて "0" を返す
    // (literal `TerrainId(...)` を残さない)。SRC.Sharp 同等の
    // unit-query デフォルト挙動と揃える。
    if args.len() < 2 {
        return Some("0".to_string());
    }
    let Ok(x) = args[0].trim().parse::<u32>() else {
        return Some("0".to_string());
    };
    let Ok(y) = args[1].trim().parse::<u32>() else {
        return Some("0".to_string());
    };
    let Some(m) = app.database().map.as_ref() else {
        return Some("0".to_string());
    };
    if x >= m.width || y >= m.height {
        return Some("0".to_string());
    }
    Some(m.cell(x, y).terrain_id.to_string())
}

fn fn_rank(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Rank(unit)` — SRC ユニットランク (`RankUpコマンド` / `BossRankコマンド`
    // で `__rank_<unit>` に蓄積)。未設定なら 0。
    // 引数は評価済み (クォート文字列・変数とも値になっている)。
    let raw = args.first()?.to_string();
    let key = format!("__rank_{}", raw.trim().trim_matches('"'));
    let v = app.script_var(&key);
    Some(if v.is_empty() {
        "0".to_string()
    } else {
        v.to_string()
    })
}

fn fn_partner(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Partner(num)` — 合体技の num 番目のパートナー Unit ID。
    // num=0 は使用ユニット自身、num>=1 は合体技パートナー。
    // 本実装は合体技を完全にはモデル化していないため、合体技履歴を
    // `直前合体技パートナー[N]` script_var に格納する規約とする。
    // 未設定なら空文字 (SRC.NET 仕様)。
    let n: i32 = args.first()?.to_string().parse().unwrap_or(0);
    if n == 0 {
        return Some(app.script_var("直前合体技ユニット").to_string());
    }
    let key = format!("直前合体技パートナー[{n}]");
    Some(app.script_var(&key).to_string())
}

fn fn_count_partner(app: &App, c: &FnCall) -> Option<String> {
    let _ = c;
    // `CountPartner()` — 直前戦闘の合体技パートナー数。`直前合体技パートナー[*]`
    // の要素数を返す。未使用 (引数があってもなくても) で 0。
    let prefix = "直前合体技パートナー[";
    let n = app
        .script_vars()
        .keys()
        .filter(|k| k.starts_with(prefix) && k.ends_with(']'))
        .count();
    Some(n.to_string())
}

fn fn_is_equiped(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `IsEquiped(unit, item)` — 指定アイテム装備中なら 1、未装備なら 0。
    // 既存の `HasItem` と同様だが、IsEquiped は SRC.NET 正典名。
    if args.len() < 2 {
        return None;
    }
    let unit = find_unit(app, args[0])?;
    let item = args[1].to_string();
    let item = item.trim().trim_matches('"');
    let has = unit.has_item_equipped(item);
    Some(if has { "1" } else { "0" }.to_string())
}

fn fn_special_power(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `SpecialPower(unit, sp_name)` / `Mind(unit, sp_name)` — unit が指定 SP の影響下にあれば 1。
    // 本実装は SP buff を `conditions` テーブルに格納しているため、
    // `has_condition(sp_name)` で判定 (`HasStatus` / `Condition` と同等)。
    // SRC.NET の `Mind` 関数 (旧名) と同じ意味論。
    if args.len() < 2 {
        return None;
    }
    let unit = find_unit(app, args[0])?;
    let sp = args[1].to_string();
    let sp = sp.trim().trim_matches('"');
    let has = unit.has_condition(sp);
    Some(if has { "1" } else { "0" }.to_string())
}

fn fn_count_item(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `CountItem(unit)` — unit が装備しているアイテム総数。
    // 未解決 unit は 0。SRC.Sharp `Unit/Unit.cs::CountItem` 同等。
    // 「未装備」指定で未装備在庫 (`spare_items`) の総数を返す。
    let key = args.first()?.to_string();
    if key == "未装備" {
        return Some(app.database().spare_items.len().to_string());
    }
    let n = find_unit(app, &key)
        .map(|u| {
            u.item_slots
                .iter()
                .filter(|s| s.equipped_item.is_some())
                .count()
        })
        .unwrap_or(0);
    Some(n.to_string())
}

fn fn_wx(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { name, args, .. } = *c;
    // `WX(unit_or_x)` / `WY(unit_or_y)` — マップウィンドウ上の表示座標。
    // 1 タイル = 32 px (本実装の `render.rs` 基準。SRC.NET は 24~32 可変
    // だが、現フロントエンドの規約 32 px に揃える)。
    // 引数が数値ならその座標 (1 始まり) のタイル左上、ユニット指定ならユニット位置の
    // タイル左上。SRC 仕様の「画面表示左上隅」は本実装で「タイル左上」と一致する。
    const TILE_PX: i64 = 32;
    let arg = args.first()?.to_string();
    let arg = arg.trim();
    // 数値解釈優先
    if let Ok(n) = arg.parse::<i64>() {
        return Some((coord::axis_from_script(n) * TILE_PX).to_string());
    }
    // ユニット参照
    if let Some(u) = find_unit(app, arg) {
        let pos = if name == "WX" { u.x } else { u.y };
        return Some((i64::from(pos) * TILE_PX).to_string());
    }
    Some("0".to_string())
}

fn fn_unit_id(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `UnitID(name)` — SRC.Sharp `Unit/Unit.cs::UnitID` 同等:
    // unit を解決して unit の一意 ID を返す。uid が未設定なら
    // unit_data_name を返す。
    let key = args.first()?.to_string();
    if let Some(inst) = find_unit(app, &key) {
        if !inst.uid.is_empty() {
            return Some(inst.uid.clone());
        }
        return Some(inst.unit_data_name.clone());
    }
    // 未解決の場合は入力をそのまま返す (legacy 互換)
    Some(key)
}

fn fn_item(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `未装備` 指定で未装備在庫の num 番目のアイテム名を返す。
    let key = args.first()?.to_string();
    let idx: usize = (args.get(1).unwrap_or(&""))
        .to_string()
        .parse()
        .unwrap_or(0);
    if key == "未装備" {
        let spare = &app.database().spare_items;
        return Some(spare.get(idx.wrapping_sub(1)).cloned().unwrap_or_default());
    }
    let Some(inst) = find_unit(app, &key) else {
        return Some(String::new());
    };
    let names = inst.equipped_item_names();
    if idx == 0 || idx > names.len() {
        Some(String::new())
    } else {
        Some(names[idx - 1].to_string())
    }
}

fn fn_item_id(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `ItemID(unit, num)` — SRC `ユニット情報関数.md`:
    // ユニットが装備している num 番目のアイテムの ID を返す。
    // 本実装はアイテム固有 ID を持たないので名称をそのまま返す
    // (SRC の実装と等価な挙動: 同名アイテムを区別しない)。
    // `未装備` を指定すると未装備在庫の num 番目を返す。
    let key = args.first()?.to_string();
    let idx: usize = (args.get(1).unwrap_or(&""))
        .to_string()
        .parse()
        .unwrap_or(0);
    if key == "未装備" {
        let spare = &app.database().spare_items;
        return Some(spare.get(idx.wrapping_sub(1)).cloned().unwrap_or_default());
    }
    let Some(inst) = find_unit(app, &key) else {
        return Some(String::new());
    };
    let names = inst.equipped_item_names();
    if idx == 0 || idx > names.len() {
        Some(String::new())
    } else {
        // 本実装では item_slot に個別 ID がないのでアイテム名を ID として返す
        Some(names[idx - 1].to_string())
    }
}

// SRC 原名: `アイテム番号` / `真アイテム番号` / `入手アイテム番号`
// (SRC.Sharp `Expression` の同系 case 群)。
// 引数のアイテム名 (または `Item(...)` 等のネスト式) を解決し、アイテム
// データベース上の 1-indexed 連番を返す。引数が既に数値ならそのまま返す
// (SRC の numeric passthrough 挙動)。本移植では真化前後・入手済みを区別する
// 別テーブルを持たないため、3 系統とも同一のアイテム DB (`database().catalog.items`)
// を引く。未登録名なら "0" を返す。
fn fn_item_index(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    let name = args.first()?.to_string();
    let name = name.trim().trim_matches('"');
    // numeric passthrough: 既に数値ならそのまま返す。
    if !name.is_empty() && name.parse::<i64>().is_ok() {
        return Some(name.to_string());
    }
    let idx = app
        .database()
        .catalog
        .items
        .iter()
        .position(|it| it.name == name)
        .map(|p| p + 1)
        .unwrap_or(0);
    Some(idx.to_string())
}

fn fn_unit(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Unit(pilot_name_or_unit_id)` — SRC `ユニット情報関数.md`:
    // 指定パイロット (またはユニット ID) が乗っているユニットの名称を返す。
    // 未解決の場合は空文字。
    let key = args.first()?.to_string();
    let Some(inst) = find_unit(app, &key) else {
        return Some(String::new());
    };
    Some(inst.unit_data_name.clone())
}

fn fn_party(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Party(unit_or_pilot)` — 当該ユニット (パイロット名 / ユニット名 /
    // 変数経由) の所属勢力ラベル ("味方"/"友軍"/"敵"/"中立") を返す。
    // 未解決 unit は空文字。
    let key = args.first()?;
    let Some(inst) = find_unit(app, key) else {
        return Some(String::new());
    };
    Some(
        match inst.party {
            crate::Party::Player => "味方",
            crate::Party::Enemy => "敵",
            crate::Party::Neutral => "中立",
            crate::Party::Npc => "ＮＰＣ",
        }
        .to_string(),
    )
}

fn fn_morale(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    let key = (args.first().copied().unwrap_or("")).to_string();
    let key = key.trim().trim_matches('"');
    // ユニット配置済み → UnitInstance.morale を返す。
    if let Some(u) = find_unit(app, key) {
        return Some(u.morale.to_string());
    }
    // ユニット未配置でも PilotInstance があればそちらを返す
    // (SRC.Sharp 準拠: Pilot.Morale は配置有無に関わらず参照可能)。
    if let Some(pi) = app
        .database()
        .pilot_instances
        .iter()
        .find(|p| p.pilot_data_name == key || p.id == key)
    {
        return Some(pi.morale.to_string());
    }
    Some("0".to_string())
}

fn fn_relation(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Relation(pilot_a, pilot_b)` — 2 パイロット間の好感度。
    // `SetRelation` で `__rel_<a>_<b>` に保存された値を読み出す。
    // 未設定なら 0。
    if args.len() < 2 {
        return Some("0".to_string());
    }
    let a = args[0].to_string();
    let b = args[1].to_string();
    let key = format!("__rel_{a}_{b}");
    let v = app.script_var(&key);
    Some(if v.is_empty() {
        "0".to_string()
    } else {
        v.to_string()
    })
}

fn fn_x(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    Some(
        find_unit(app, args.first().copied().unwrap_or(""))
            .map(|u| coord::to_script(u.x).to_string())
            .unwrap_or_else(|| "0".to_string()),
    )
}

fn fn_y(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    Some(
        find_unit(app, args.first().copied().unwrap_or(""))
            .map(|u| coord::to_script(u.y).to_string())
            .unwrap_or_else(|| "0".to_string()),
    )
}

fn fn_area(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Area(unit)` — SRC 仕様: "地上"/"空中"/"水上"/"水中"/"宇宙" を返す。
    // SRC.Sharp `Expressions/Functions/Unit/Unit.cs::Area` 同等。
    // 1) UnitInstance.current_area が空でなければ優先 (`.eve` コマンドで明示設定)。
    // 2) それ以外は unit の transportation/adaption/features と
    //    現在地の地形クラスから `movement::unit_area_on_terrain` で推定。
    // 地形未定義 / マップ範囲外 / ユニット未検出時は空文字。
    let u = find_unit(app, args.first().copied().unwrap_or(""))?;
    if !u.current_area.is_empty() {
        return Some(u.current_area.clone());
    }
    let Some(map) = app.database().map.as_ref() else {
        return Some(String::new());
    };
    if u.x >= map.width || u.y >= map.height {
        return Some(String::new());
    }
    let terrain_id = map.cell(u.x, u.y).terrain_id;
    let raw_class = app
        .database()
        .catalog
        .terrains
        .iter()
        .find(|t| t.id == terrain_id)
        .map(|t| t.class.clone())
        .or_else(|| {
            crate::data::terrain::DEFAULT_TERRAINS
                .iter()
                .find(|t| t.id == terrain_id)
                .map(|t| t.class.to_string())
        })
        .unwrap_or_default();
    // ユニット静的データを引いて area 推定
    let unit_data_name = u.unit_data_name.clone();
    let active_feature_names: Vec<String> =
        u.active_features.iter().map(|f| f.name.clone()).collect();
    let (transportation, adaption) = app
        .database()
        .unit_by_name(&unit_data_name)
        .map(|ud| (ud.transportation.clone(), ud.adaption.0))
        .unwrap_or_else(|| ("陸".to_string(), *b"AAAA"));
    let area = crate::movement::unit_area_on_terrain(
        &raw_class,
        &transportation,
        &adaption,
        &active_feature_names,
    );
    Some(area.to_string())
}

fn fn_distance(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Distance(unitA, unitB)` — マンハッタン距離。いずれかが
    // 未解決の場合は 0 を返す (SRC.Sharp 同等)。
    if args.len() < 2 {
        return None;
    }
    let Some(a) = find_unit(app, args[0]) else {
        return Some("0".to_string());
    };
    let Some(b) = find_unit(app, args[1]) else {
        return Some("0".to_string());
    };
    let d = (a.x as i64 - b.x as i64).abs() + (a.y as i64 - b.y as i64).abs();
    Some(d.to_string())
}

fn fn_count(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Count(prefix)` — 配列変数 `prefix[*]` の要素数を返す。
    // SRC.Sharp `Other.cs::Count` (line 6-83) 同等: 名前が
    // `prefix[` で始まる script_var の個数。
    //
    // 過去実装は party label → 該当陣営ユニット数 を返していたが、
    // 実シナリオの `Count(入手ユニット候補)` 等は **配列プレフィックス**
    // のカウント用途。party 数は `味方数` / `敵数` 等のシステム変数で
    // 取得する設計。
    //
    // 後方互換のため、prefix が party label と一致した場合のみ legacy
    // 挙動 (該当陣営の unit_instances 数) も併用する。
    if args.is_empty() {
        return None;
    }
    let key = args.first()?.to_string();
    let key = key.trim().trim_matches('"');
    let prefix = format!("{key}[");
    let array_count = app
        .script_vars()
        .keys()
        .filter(|k| k.starts_with(&prefix))
        .count();
    if array_count > 0 {
        return Some(array_count.to_string());
    }
    // legacy fallback: party 名と一致したら該当陣営の unit 数
    if let Some(party) = parse_party_label(key) {
        let n = app
            .database()
            .unit_instances
            .iter()
            .filter(|u| u.party == party && !u.off_map)
            .count();
        return Some(n.to_string());
    }
    Some("0".to_string())
}

fn fn_exists(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    let key = args.first()?;
    Some(
        if find_unit(app, key).is_some() {
            "1"
        } else {
            "0"
        }
        .to_string(),
    )
}

fn fn_is_available(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `IsAvailable(unit, feature)` — unit が指定の特殊能力 (feature)
    // を持っているか。SRC.Sharp `Unit/Unit.cs::IsAvailable` 同等。
    // 優先: UnitInstance.active_features (is_active=true) →
    // フォールバック: UnitData.features の key 一致。
    if args.len() < 2 {
        return None;
    }
    let key = args[0].to_string();
    let feat = args[1].to_string();
    let feat = feat.trim().trim_matches('"').to_string();
    let has = match find_unit(app, &key) {
        Some(inst) => {
            // 1) active_features で is_active なものを検索
            let from_active = inst
                .active_features
                .iter()
                .any(|f| f.is_active && f.name == feat);
            if from_active {
                true
            } else {
                // 2) static UnitData.features にある場合も true
                app.database()
                    .catalog
                    .units
                    .iter()
                    .find(|u| u.name == inst.unit_data_name)
                    .map(|u| u.features.iter().any(|(k, _)| k.as_str() == feat))
                    .unwrap_or(false)
            }
        }
        None => false,
    };
    Some(if has { "1" } else { "0" }.to_string())
}

fn fn_is_defined(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `IsDefined(name [, kind])` — SRC.Sharp
    // `Expressions/Functions/Other/Other.cs::IsDefined` 同等。
    // - 1 arg: pilot / unit_instance / item の何れかに存在すれば 1
    // - 2 args: kind ("パイロット" / "ユニット" / "アイテム") を絞る
    let pname = args.first()?.to_string();
    let pname = pname.trim().trim_matches('"');
    let kind = args.get(1).map(|s| {
        let v = s.to_string();
        v.trim().trim_matches('"').to_string()
    });
    let is_pilot = || {
        app.database()
            .catalog
            .pilots
            .iter()
            .any(|p| p.name == pname || p.nickname == pname)
            || app
                .database()
                .unit_instances
                .iter()
                .any(|u| u.pilot_name == pname && !u.off_map)
    };
    let is_unit = || {
        app.database()
            .unit_instances
            .iter()
            .any(|u| matches_unit_handle(u, pname) && !u.off_map)
    };
    let is_item = || app.database().catalog.items.iter().any(|i| i.name == pname);
    let found = match kind.as_deref() {
        Some("パイロット") => is_pilot(),
        Some("ユニット") => is_unit(),
        Some("アイテム") => is_item(),
        Some(_) => false,
        None => is_pilot() || is_unit() || is_item(),
    };
    Some(if found { "1" } else { "0" }.to_string())
}
