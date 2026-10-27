//! `.eve` コマンド: ユニットの生成・除去・配置・形態・所属。

use super::*;

pub(super) fn exec(app: &mut App, c: &Cmd) -> Option<Result<usize, ScriptError>> {
    Some(match c.lname.as_str() {
        "makepilotlist" => exec_makepilotlist(app, c),
        "makeunitlist" => exec_makeunitlist(app, c),
        "transform" => exec_transform(app, c),
        "combine" => exec_combine(app, c),
        "split" => exec_split(app, c),
        "stow" => exec_stow(app, c),
        "changearea" => exec_changearea(app, c),
        "join" => exec_join(app, c),
        "ride" => exec_ride(app, c),
        "land" | "air" | "water" | "sea" | "cosmos" | "diving" | "地上" | "空中" | "水中"
        | "水上" | "宇宙" | "地中" => exec_land(app, c),
        "launch" => exec_launch(app, c),
        "getoff" => exec_getoff(app, c),
        "leave" => exec_leave(app, c),
        "setrelation" => exec_setrelation(app, c),
        "changeparty" => exec_changeparty(app, c),
        "replacepilot" => exec_replacepilot(app, c),
        "escape" => exec_escape(app, c),
        "showunitstatus" => exec_showunitstatus(app, c),
        "center" => exec_center(app, c),
        "changeunitclass" => exec_changeunitclass(app, c),
        "select" => exec_select(app, c),
        "selecttarget" => exec_selecttarget(app, c),
        "movunit" | "moveunit" => exec_movunit(app, c),
        "move" => exec_move(app, c),
        "kill" | "destroy" => exec_kill(app, c),
        "create" => exec_create(app, c),
        "removeunit" => exec_removeunit(app, c),
        "removepilot" => exec_removepilot(app, c),
        _ => return None,
    })
}

fn exec_makepilotlist(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `MakePilotList mode` (`MakePilotListコマンド.md`):
    // pilots を `mode` 能力値でソートし `パイロットリスト[1..N]` に名前を格納。
    // mode = レベル/ＳＰ/格闘/射撃/命中/回避/技量/反応/経験値 等。
    // 完全な UI 表示は本実装では行わず、script_var への格納のみ。
    let mode = xargs.first().cloned().unwrap_or_default();
    let pilot_names: Vec<String> = app
        .database()
        .pilots
        .iter()
        .map(|p| p.name.clone())
        .collect();
    let mut entries: Vec<(String, i64)> = pilot_names
        .iter()
        .filter_map(|pname| {
            // effective_pilot_data でレベルアップ後スタットを使う
            let ep = app.database().effective_pilot_data(pname)?;
            let v: i64 = match mode.as_str() {
                "レベル" | "Level" => app
                    .database()
                    .unit_instances
                    .iter()
                    .find(|u| u.pilot_name == *pname)
                    .map(|u| i64::from(crate::pilot_instance::level_from_exp(u.total_exp)))
                    .unwrap_or(1),
                "ＳＰ" | "SP" => ep.sp.unwrap_or(0) as i64,
                "格闘" | "Infight" => ep.infight as i64,
                "射撃" | "Shooting" => ep.shooting as i64,
                "命中" | "Hit" => ep.hit as i64,
                "回避" | "Dodge" => ep.dodge as i64,
                "技量" | "Technique" => ep.technique as i64,
                "反応" | "Intuition" => ep.intuition as i64,
                "経験値" | "Exp" => ep.exp_value as i64,
                _ => 0,
            };
            Some((pname.clone(), v))
        })
        .collect();
    // 降順ソート (高い値が先)。等値は元順序維持。
    entries.sort_by_key(|a| std::cmp::Reverse(a.1));
    for (i, (name, _)) in entries.iter().enumerate() {
        let key = format!("パイロットリスト[{}]", i + 1);
        app.set_script_var(key, name.clone());
    }
    // `パイロットリスト数` も格納
    app.set_script_var("パイロットリスト数".to_string(), entries.len().to_string());
    Ok(pc + 1)
}

fn exec_makeunitlist(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `MakeUnitList mode` (`MakeUnitListコマンド.md`):
    // unit_instances (出撃/待機のユニット) を `mode` でソートし
    // `ユニットリスト[1..N]` に uid を格納。
    // C# Event.cs::MakeUnitList: SRC.UList を Status=="出撃"|"待機" でフィルタ。
    let mode = xargs.first().cloned().unwrap_or_default();
    let db = app.database();
    let mut entries: Vec<(String, i64)> = db
        .unit_instances
        .iter()
        .map(|inst| {
            let v: i64 = match mode.as_str() {
                "ＨＰ" | "HP" => {
                    let max = db.effective_max_hp(inst);
                    (max - inst.damage).max(0)
                }
                "ＥＮ" | "EN" => {
                    let max = db.effective_max_en(inst);
                    (max - inst.en_consumed).max(0) as i64
                }
                "装甲" | "Armor" => db.effective_armor(inst),
                "運動性" | "Mobility" => db.effective_mobility(inst) as i64,
                "移動力" | "Speed" => db.effective_speed(inst) as i64,
                "最大攻撃力" | "MaxAttack" => db
                    .unit_by_name(&inst.unit_data_name)
                    .map(|u| u.weapons.iter().map(|w| w.power).max().unwrap_or(0))
                    .unwrap_or(0),
                "最長射程" | "MaxRange" => db
                    .unit_by_name(&inst.unit_data_name)
                    .map(|u| {
                        u.weapons
                            .iter()
                            .map(|w| w.max_range as i64)
                            .max()
                            .unwrap_or(0)
                    })
                    .unwrap_or(0),
                "レベル" | "Level" => {
                    i64::from(crate::pilot_instance::level_from_exp(inst.total_exp))
                }
                "気力" | "Morale" => inst.morale as i64,
                _ => 0,
            };
            // ユニット識別子: uid 優先、無ければ unit_data_name
            let ident = if inst.uid.is_empty() {
                inst.unit_data_name.clone()
            } else {
                inst.uid.clone()
            };
            (ident, v)
        })
        .collect();
    let _ = db;
    entries.sort_by_key(|a| std::cmp::Reverse(a.1));
    for (i, (name, _)) in entries.iter().enumerate() {
        let key = format!("ユニットリスト[{}]", i + 1);
        app.set_script_var(key, name.clone());
    }
    app.set_script_var("ユニットリスト数".to_string(), entries.len().to_string());
    Ok(pc + 1)
}

fn exec_transform(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `Transform [unit] mode` (`Transformコマンド.md`, C# `TransformCmd`)。
    // 形態の差し替えはプレイヤーの変形 / 換装と同じ `App::set_unit_form` を通し、
    // 新形態の active_features (必要技能ゲート込み) と abilities を作り直す。
    // 現形態と同じなら何もしない (C# 準拠)。
    // 注: C# `TransformCmd` は 変形 イベントを発火しない (発火はプレイヤーの変形
    // コマンドと AI 側) が、本移植は `Combine` と揃えて発火する (fixture 21 で固定)。
    let (target, new_form) = match xargs.len() {
        0 => return Err(err(line, "Transformコマンドの引数の数が違います")),
        1 => (app.selected_unit_for_event().to_string(), xargs[0].clone()),
        _ => (xargs[0].clone(), xargs[1].clone()),
    };
    let found = app
        .database()
        .unit_instances
        .iter()
        .find(|u| matches_unit_handle(u, &target))
        .map(|u| (u.uid.clone(), u.unit_data_name != new_form));
    let Some((uid, true)) = found else {
        return Ok(pc + 1);
    };
    app.set_unit_form(&uid, &new_form);
    let fire = app
        .database()
        .unit_by_uid(&uid)
        .map(|u| (u.pilot_name.clone(), u.unit_data_name.clone(), u.party));
    if let Some((pilot_name, unit_data_name, party)) = fire {
        fire_unit_event_labels(
            app,
            &["変形", "Transform"],
            &pilot_name,
            &unit_data_name,
            party,
            Some(&new_form),
        );
    }
    Ok(pc + 1)
}

fn exec_combine(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `Combine [unit] mode` — unit を合体形態 mode へ。
    // 引数 1 個なら unit 省略時の current selection だが、本実装では
    // mode = xargs[xargs.len()-1] とし、unit が指定されていればそれを、
    // 無ければ Player 勢力の任意のユニットを選ぶ。
    // C# CombineCmd: ArgNum == 2 (unit省略) or ArgNum == 3 (unit + mode)、
    // 0 引数や 3+ は EventErrorException。
    if xargs.is_empty() {
        return Err(err(line, "Combineコマンドの引数の数が違います"));
    }
    let mode = xargs[xargs.len() - 1].clone();
    let target_key = if xargs.len() >= 2 {
        Some(xargs[0].clone())
    } else {
        None
    };
    let target_idx = if let Some(k) = target_key {
        app.database()
            .unit_instances
            .iter()
            .position(|u| matches_unit_handle(u, &k))
    } else {
        app.database()
            .unit_instances
            .iter()
            .position(|u| u.party == crate::Party::Player)
    };
    if let Some(i) = target_idx {
        let main_uid = app.database().unit_instances[i].uid.clone();
        let main_party = app.database().unit_instances[i].party;
        // 合体形態の `分離` feature から構成パーツ名を取得 (表示フラグ除く)。
        // 例: 未完成エクスカリバー `分離=非表示 キャリバーン(不完全) ヴィヴィアン(不完全)`。
        let part_names: Vec<String> = app
            .database()
            .unit_by_name(&mode)
            .and_then(|ud| {
                ud.features
                    .iter()
                    .find(|(n, _)| n == "分離" || n == "Separate")
                    .map(|(_, v)| v.clone())
            })
            .map(|v| {
                v.split_whitespace()
                    .filter(|t| !matches!(*t, "非表示" | "表示"))
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();
        // 相方パーツ (main 以外・同陣営・パーツ名一致) のパイロットを吸収し、
        // 相方ユニットと Unit コマンドが先に作った空の合体形態テンプレ
        // (無人・同名) を除去する。これで合体形態が 1 機に統合される。
        let mut absorbed_pilots: Vec<String> = Vec::new();
        let mut remove_uids: Vec<String> = Vec::new();
        for u in &app.database().unit_instances {
            if u.uid == main_uid {
                continue;
            }
            let is_partner =
                u.party == main_party && part_names.iter().any(|p| p == &u.unit_data_name);
            let is_empty_template = u.unit_data_name == mode && u.pilot_name.is_empty();
            if is_partner {
                if !u.pilot_name.is_empty() {
                    absorbed_pilots.push(u.pilot_name.clone());
                }
                remove_uids.push(u.uid.clone());
            } else if is_empty_template {
                remove_uids.push(u.uid.clone());
            }
        }
        // main を合体形態へ morph + HP/EN 全快 + 相方をサブパイロットに。
        {
            let u = &mut app.database_mut().unit_instances[i];
            u.unit_data_name = mode.clone();
            u.damage = 0;
            u.en_consumed = 0;
            u.weapons.clear();
            for p in &absorbed_pilots {
                if !u.pilot_ids.contains(p) {
                    u.pilot_ids.push(p.clone());
                }
            }
        }
        // 合体形態の特殊能力を再初期化。
        let feats = app
            .database()
            .unit_by_name(&mode)
            .map(|ud| ud.features.clone())
            .unwrap_or_default();
        let new_feats = crate::necessary_skill::gated_active_features(
            &feats,
            &app.database().unit_instances[i],
            app.database(),
        );
        app.database_mut().unit_instances[i].active_features = new_feats;
        // 相方 / 空テンプレを除去 (pos_index も再構築)。
        if !remove_uids.is_empty() {
            app.database_mut()
                .remove_units_where(|u| remove_uids.contains(&u.uid));
        }
        // 合体イベントラベル発火 (除去で index がずれるため uid で引き直す)。
        if let Some(u) = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.uid == main_uid)
        {
            let pilot_name = u.pilot_name.clone();
            let unit_data_name = u.unit_data_name.clone();
            let party = u.party;
            fire_unit_event_labels(
                app,
                &["合体", "Combine"],
                &pilot_name,
                &unit_data_name,
                party,
                Some(&mode),
            );
        }
    }
    Ok(pc + 1)
}

fn exec_split(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Split [unit]` — unit を 派生機体 / 分離 feature の最初のトークンに戻す。
    // unit 省略時は Player 勢力の任意のユニット。
    let target_key = xargs.first().cloned();
    let target_idx = if let Some(k) = target_key {
        app.database()
            .unit_instances
            .iter()
            .position(|u| matches_unit_handle(u, &k))
    } else {
        app.database()
            .unit_instances
            .iter()
            .position(|u| u.party == crate::Party::Player)
    };
    if let Some(i) = target_idx {
        let unit_name = app.database().unit_instances[i].unit_data_name.clone();
        let new_data = app
            .database()
            .unit_by_name(&unit_name)
            .and_then(|d| {
                d.features
                    .iter()
                    .find(|(k, _)| k == "分離" || k == "派生機体")
                    .map(|(_, v)| v.clone())
            })
            .and_then(|raw| {
                // "非表示 X Y Z" の "非表示" を除去し、先頭トークンを採用
                raw.split_whitespace()
                    .find(|t| *t != "非表示")
                    .map(str::to_string)
            });
        if let Some(new_data) = new_data {
            app.database_mut().unit_instances[i].unit_data_name = new_data.clone();
            let feats = app
                .database()
                .unit_by_name(&new_data)
                .map(|ud| ud.features.clone())
                .unwrap_or_default();
            let new_feats = crate::necessary_skill::gated_active_features(
                &feats,
                &app.database().unit_instances[i],
                app.database(),
            );
            app.database_mut().unit_instances[i].active_features = new_feats;
            let u = &app.database().unit_instances[i];
            let pilot_name = u.pilot_name.clone();
            let unit_data_name = u.unit_data_name.clone();
            let party = u.party;
            // SRC `分離 <unit> <name>:` の name は **分離前の形態名**。
            fire_unit_event_labels(
                app,
                &["分離", "Split"],
                &pilot_name,
                &unit_data_name,
                party,
                Some(&unit_name),
            );
        }
    }
    Ok(pc + 1)
}

fn exec_stow(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC 風 `Stow unit carrier` (収納コマンド、本実装独自):
    // ユニットを母艦に格納する。`fire_boarding_event` のスクリプト版で、
    // `収納 <unit>:` ラベルを発火する。原典には対応する直接の Stow
    // コマンドは無いが、UI から呼ぶ `App::fire_boarding_event` と
    // 同等のフロー (`相手パイロット`/`相手ユニットＩＤ` 設定、life_state、
    // off_map) をスクリプトから明示できるようにする。
    if xargs.len() < 2 {
        return Ok(pc + 1);
    }
    let unit_key = xargs[0].to_string();
    let carrier_key = xargs[1].to_string();
    let unit_idx = app
        .database()
        .unit_instances
        .iter()
        .position(|u| matches_unit_handle(u, &unit_key));
    let carrier_idx = app
        .database()
        .unit_instances
        .iter()
        .position(|u| matches_unit_handle(u, &carrier_key));
    if let (Some(u_idx), Some(c_idx)) = (unit_idx, carrier_idx) {
        app.fire_boarding_event(u_idx, c_idx);
    }
    Ok(pc + 1)
}

fn exec_changearea(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `ChangeArea [unit] area` (`ChangeAreaコマンド.md`):
    // ユニットの活動領域を強制変更。Land/Air/Water/... コマンドの上位 API。
    // 引数 1: unit 省略 → selected_unit_for_event。
    // 引数 2+: unit + area 文字列 (`地上`/`空中`/`水中`/`水上`/`地中`/`宇宙`)。
    let (target, area) = match xargs.len() {
        0 => return Ok(pc + 1),
        1 => (app.selected_unit_for_event().to_string(), xargs[0].clone()),
        _ => (xargs[0].clone(), xargs[1].clone()),
    };
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &target))
    {
        u.current_area = area;
    }
    Ok(pc + 1)
}

fn exec_join(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `Joinコマンド.md`: `Join [unit]`
    // `Leave` で離脱させたユニット/パイロット/非戦闘員を部隊に復帰させる。
    // 0 引数: カレント選択ユニットを復帰。
    // 1 引数: 名前/UID で指定したユニットを復帰 (`off_map = false`,
    //         `life_state = ""` に戻す)。
    // パイロット名称でも unit_instances を逆引きして復帰できる。
    // ※旧実装は `pilot unit` の 2 引数形式 (= Ride 相当) だったが誤り。
    let key = if let Some(a) = xargs.first() {
        a.to_string()
    } else {
        app.selected_unit_for_event().to_string()
    };
    let uid = app
        .database()
        .unit_instances
        .iter()
        .find(|u| matches_unit_handle(u, &key) || (!key.is_empty() && u.pilot_name == key))
        .map(|u| u.uid.clone());
    if let Some(uid) = uid {
        app.database_mut().set_off_map(&uid, false);
        if let Some(u) = app.database_mut().unit_by_uid_mut(&uid) {
            u.life_state = String::new();
        }
    }
    Ok(pc + 1)
}

fn exec_ride(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Ride pilot [unit]` — パイロットをユニットに搭乗させる。
    // - 1 引数形式 (`Ride pilot`): 直前の `Unit <name>` で生成した
    //   カレントユニット (`app.selected_unit_for_event`) に載せる。
    //   SRC `RideCmd` の case 2 相当。
    // - 2 引数形式 (`Ride pilot unit`): 名前 / uid で引いたユニットに
    //   載せる。SRC `RideCmd` の case 3 を簡略化したもの。
    // 搭乗 = 対象 UnitInstance.pilot_name を pilot 名で上書きする。
    if xargs.is_empty() {
        return Ok(pc + 1);
    }
    let pilot = xargs[0].to_string();
    let target_handle = if xargs.len() >= 2 {
        xargs[1].to_string()
    } else {
        app.selected_unit_for_event().to_string()
    };
    if !target_handle.is_empty() {
        if let Some(tidx) = app
            .database()
            .unit_instances
            .iter()
            .position(|u| matches_unit_handle(u, &target_handle))
        {
            // SRC `RideCmd` は乗り換え前に `p.GetOff()` で元機を降りる。
            // pilot が target 以外のユニットに乗っていれば無人化して
            // パイロット重複を防ぐ (`Ride サラ ヴィヴィアン` で元機ブレスに
            // サラが残らないように)。1 引数形式 (新規サブパイロット搭乗) では
            // 元機が無いので no-op。
            let old: Vec<usize> = app
                .database()
                .unit_instances
                .iter()
                .enumerate()
                .filter(|(i, u)| *i != tidx && u.pilot_name == pilot)
                .map(|(i, _)| i)
                .collect();
            for i in old {
                let u = &mut app.database_mut().unit_instances[i];
                u.pilot_name.clear();
                u.pilot_ids.retain(|p| p != &pilot);
            }
            app.database_mut().unit_instances[tidx].pilot_name = pilot;
        }
    }
    Ok(pc + 1)
}

fn exec_land(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        lname, xargs, pc, ..
    } = *c;
    // SRC 移動領域コマンド: `地上` / `空中` / `水中` / `水上` / `宇宙` / `地中`
    // (`地上.md` 等):
    // 指定ユニットの活動領域 (`Area()` 関数の返値) を切り替える。
    // 引数 0: 直前選択ユニット、1 引数: unit 指定。
    // `UnitInstance.current_area` を更新 (地形由来の Area を上書きする)。
    // 行動終了にはならない (SRC 仕様)。移動後の使用は不可だが、
    // 本実装はチェックを行わず無条件に受け付ける (簡略化)。
    let area_label = match lname.as_str() {
        "land" | "地上" => "地上",
        "air" | "空中" => "空中",
        "water" | "水中" => "水中",
        "sea" | "水上" => "水上",
        "cosmos" | "宇宙" => "宇宙",
        "diving" | "地中" => "地中",
        _ => "",
    };
    let target = if xargs.is_empty() {
        app.selected_unit_for_event().to_string()
    } else {
        xargs[0].clone()
    };
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &target))
    {
        u.current_area = area_label.to_string();
    }
    Ok(pc + 1)
}

fn exec_launch(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Launch unit_or_pilot x y` — マップ外退避中のユニットを (x, y) に
    // 出撃。座標を上書きし `off_map = false` で再配置する。該当ユニットが
    // unit_instances にいない場合は no-op (Escape で削除されているケース
    // ではなく `Pilot(...)` 解決失敗等の異常系)。
    if xargs.len() < 3 {
        return Ok(pc + 1);
    }
    let target = &xargs[0];
    let nx: u32 = xargs[1].parse().unwrap_or(0);
    let ny: u32 = xargs[2].parse().unwrap_or(0);
    let uid = app
        .database()
        .unit_instances
        .iter()
        .find(|u| matches_unit_handle(u, target))
        .map(|u| u.uid.clone());
    if let Some(uid) = uid {
        // 先に座標を確定 (off_map 中は索引非登録)、続けて off_map 解除で索引へ載せる。
        app.database_mut().move_unit(&uid, nx, ny);
        app.database_mut().set_off_map(&uid, false);
    }
    Ok(pc + 1)
}

fn exec_getoff(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Getoff pilot` — 指定パイロットを乗機から降ろす。
    // 該当 UnitInstance.pilot_name を空にすることで、後段の RemovePilot
    // が誤ってユニット本体を巻き込まないようにする。
    if let Some(target_arg) = xargs.first() {
        let key = target_arg.to_string();
        if let Some(u) = app
            .database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| u.pilot_name == key)
        {
            u.pilot_name = String::new();
        }
    }
    Ok(pc + 1)
}

fn exec_leave(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Leave unit` — 部隊から外す (`Leaveコマンド.md`)。Escape と異なり
    // ユニット状態が `離脱` になり Status() で識別可能。
    if let Some(target_arg) = xargs.first() {
        let key = target_arg.to_string();
        let uid = app
            .database()
            .unit_instances
            .iter()
            .find(|u| matches_unit_handle(u, &key))
            .map(|u| u.uid.clone());
        if let Some(uid) = uid {
            app.database_mut().set_off_map(&uid, true);
            if let Some(u) = app.database_mut().unit_by_uid_mut(&uid) {
                u.life_state = "離脱".to_string();
            }
        }
    }
    Ok(pc + 1)
}

fn exec_setrelation(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `SetRelation pilot_a pilot_b value` (`SetRelationコマンド.md`):
    // パイロット間好感度を設定。本実装は `__rel_<a>_<b>` script_var に
    // 数値を保存し、`Relation()` 関数で読み出す。対称関係なので両方向
    // キーに同値をセット。値は通常 -100..=100 (SRC 仕様の範囲)。
    if xargs.len() < 3 {
        return Ok(pc + 1);
    }
    let a = xargs[0].clone();
    let b = xargs[1].clone();
    let value = xargs[2].to_string();
    app.set_script_var(format!("__rel_{a}_{b}"), value.clone());
    app.set_script_var(format!("__rel_{b}_{a}"), value);
    Ok(pc + 1)
}

fn exec_changeparty(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `ChangeParty unit "Enemy"` — 所属勢力変更
    if xargs.len() < 2 {
        return Err(err(line, "ChangeParty 命令は 2 引数必要 (unit party)。"));
    }
    let target = xargs[0].clone();
    let party = parse_party(&xargs[1], line)?;
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &target))
    {
        u.party = party;
    }
    Ok(pc + 1)
}

fn exec_replacepilot(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `ReplacePilot unit pilot` — Join と同義 (UnitInstance.pilot_name 差替え)
    if xargs.len() < 2 {
        return Err(err(line, "ReplacePilot 命令は 2 引数必要 (unit pilot)。"));
    }
    let unit = xargs[0].clone();
    let pilot = xargs[1].clone();
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &unit))
    {
        u.pilot_name = pilot;
    }
    Ok(pc + 1)
}

fn exec_escape(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Escape [unit_or_pilot | party]` — マップから一時退避（撤退）。
    // `off_map = true` を立てて AI / 戦闘 / 描画 / 勝利判定から除外。
    // 後段の `Launch` / `Place` で `off_map = false` に戻して再配置できる。
    // SRC `EscapeCmd` 準拠:
    //  - 引数なし: `SelectedUnitForEvent` (ForEach ループ対象等) を退避。
    //    `ForEach 敵 / Escape / Next` で全敵退避するイディオムを成立させる。
    //  - 陣営ラベル (味方/敵/ＮＰＣ/中立): 当該陣営の出撃中ユニットを全退避。
    //  - それ以外: パイロット名 / ユニット ID で 1 体退避。
    // 末尾 `非同期` オプションは無視。
    let args_no_opt: Vec<&String> = xargs.iter().filter(|a| a.as_str() != "非同期").collect();
    match args_no_opt.first() {
        None => {
            // 引数なし: 選択ユニットを退避。
            let key = app.selected_unit_for_event().to_string();
            let uid = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &key))
                .map(|u| u.uid.clone());
            if let Some(uid) = uid {
                app.database_mut().set_off_map(&uid, true);
            }
        }
        Some(arg) => {
            let key = arg.to_string();
            if let Some(party) = parse_party_label(key.trim().trim_matches('"')) {
                // 陣営ラベル: 当該陣営の出撃中ユニットを全退避。
                let uids: Vec<String> = app
                    .database()
                    .unit_instances
                    .iter()
                    .filter(|u| u.party == party && !u.off_map)
                    .map(|u| u.uid.clone())
                    .collect();
                for uid in uids {
                    app.database_mut().set_off_map(&uid, true);
                }
            } else {
                // パイロット名 / ユニット ID で 1 体退避。
                let uid = app
                    .database()
                    .unit_instances
                    .iter()
                    .find(|u| matches_unit_handle(u, &key))
                    .map(|u| u.uid.clone());
                if let Some(uid) = uid {
                    app.database_mut().set_off_map(&uid, true);
                }
            }
        }
    }
    Ok(pc + 1)
}

fn exec_showunitstatus(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // ShowUnitStatus <unit> — ユニット詳細情報を表示
    // Find the unit by name or pilot name
    let Some(unit_name) = xargs.first().map(|s| s.as_str()) else {
        log::warn!("ShowUnitStatus: no unit specified");
        return Ok(pc + 1);
    };
    let Some(unit) =
        app.database().unit_instances.iter().find(|u| {
            u.unit_data_name == unit_name || u.uid == unit_name || u.pilot_name == unit_name
        })
    else {
        log::warn!("ShowUnitStatus: unit '{}' not found", unit_name);
        return Ok(pc + 1);
    };
    // Get unit data
    let Some(unit_data) = app.database().unit_by_name(&unit.unit_data_name) else {
        log::warn!(
            "ShowUnitStatus: unit data '{}' not found",
            unit.unit_data_name
        );
        return Ok(pc + 1);
    };
    // Get effective stats (with equipment bonuses)
    let max_hp = app.database().effective_max_hp(unit);
    let max_en = app.database().effective_max_en(unit);
    let armor = app.database().effective_armor(unit);
    let mobility = app.database().effective_mobility(unit);
    let speed = app.database().effective_speed(unit);
    // Current HP/EN considering damage and consumption
    let cur_hp = max_hp - unit.damage;
    let cur_en = max_en - unit.en_consumed;
    // Log the status
    log::info!(
        "ShowUnitStatus: {} (Pilot: {}) HP:{}/{} EN:{}/{} Armor:{} Mobility:{} Speed:{} Morale:{}",
        unit_data.nickname,
        unit.pilot_name,
        cur_hp,
        max_hp,
        cur_en,
        max_en,
        armor,
        mobility,
        speed,
        unit.morale
    );
    // Push a message to the HUD
    app.push_message(format!(
        "【{}】HP:{}/{} EN:{}/{} 装甲:{} 運動性:{} 移動力:{} 士気:{}",
        unit_data.nickname, cur_hp, max_hp, cur_en, max_en, armor, mobility, speed, unit.morale
    ));
    Ok(pc + 1)
}

fn exec_center(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `Center x y [option]` または `Center unit [option]`
    // (`Centerコマンド.md`): マップ表示中心を移動。
    // option (`非同期`) は本実装で無視 (即時反映)。
    // 引数 0: no-op、引数 1: unit 名、引数 2+: x y。
    if xargs.is_empty() {
        return Ok(pc + 1);
    }
    // 数値 2 引数なら座標指定。1 引数 (または最初が数値でなければ) は unit 指定。
    if xargs.len() >= 2 {
        if let (Ok(x), Ok(y)) = (
            xargs[0].trim().parse::<u32>(),
            xargs[1].trim().parse::<u32>(),
        ) {
            app.set_map_cursor(x, y);
            return Ok(pc + 1);
        }
    }
    // unit 指定
    let key = xargs[0].to_string();
    let pos = app
        .database()
        .unit_instances
        .iter()
        .find(|u| matches_unit_handle(u, &key))
        .map(|u| (u.x, u.y));
    if let Some((x, y)) = pos {
        app.set_map_cursor(x, y);
    }
    Ok(pc + 1)
}

fn exec_changeunitclass(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `ChangeUnitClass [unit] class` (`ChangeUnitClassコマンド.md`):
    // ユニットの分類を一時的に変更する。`-` で元に戻す。
    if xargs.is_empty() {
        return Ok(pc + 1);
    }
    let (unit_key, class) = if xargs.len() >= 2 {
        (xargs[0].clone(), xargs[1].clone())
    } else {
        (app.selected_unit_for_event().to_string(), xargs[0].clone())
    };
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &unit_key))
    {
        u.class_override = if class == "-" { None } else { Some(class) };
    }
    Ok(pc + 1)
}

fn exec_select(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `Select <unit>` (`Selectコマンド.md`):
    // 引数 1: unit 識別子 (pilot 名 / unit_data 名 / uid)。
    // `SelectedUnitForEvent` に指定ユニットをセットする。
    // C# SelectCmd.cs: `Event.SelectedUnitForEvent = GetArgAsUnit(2);`
    if xargs.len() != 1 {
        return Err(err(line, "Selectコマンドの引数の数が違います"));
    }
    let key = xargs[0].to_string();
    app.set_selected_unit_for_event(key);
    Ok(pc + 1)
}

fn exec_selecttarget(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `SelectTarget unit` (`SelectTargetコマンド.md`):
    // デフォルトターゲットを設定。`相手パイロット` / `相手ユニットＩＤ`
    // システム変数を更新し、汎用戦闘アニメ等で参照可能にする。
    // 引数 1: unit 識別子 (pilot 名 / unit_data 名 / uid)。
    // C# SelectTargetCmd.cs: `ArgNum != 2` → EventErrorException。
    if xargs.len() != 1 {
        return Err(err(line, "SelectTargetコマンドの引数の数が違います"));
    }
    if let Some(target) = xargs.first() {
        let key = target.to_string();
        let resolved = app
            .database()
            .unit_instances
            .iter()
            .find(|u| matches_unit_handle(u, &key))
            .map(|u| (u.pilot_name.clone(), u.uid.clone()));
        if let Some((pilot, uid)) = resolved {
            app.set_script_var("相手パイロット".to_string(), pilot);
            app.set_script_var("相手ユニットＩＤ".to_string(), uid);
        }
    }
    Ok(pc + 1)
}

fn exec_movunit(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // MoveUnit unit_name x y
    if xargs.len() < 3 {
        return Err(err(line, "MoveUnit 命令は 3 引数必要 (name x y)。"));
    }
    let target = &xargs[0];
    let nx = parse_u32(app, xargs, 1, line)?;
    let ny = parse_u32(app, xargs, 2, line)?;
    let uid = app
        .database()
        .unit_instances
        .iter()
        .find(|u| matches_unit_handle(u, target))
        .map(|u| u.uid.clone());
    if let Some(uid) = uid {
        app.database_mut().move_unit(&uid, nx, ny);
    }
    Ok(pc + 1)
}

fn exec_move(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `Move [unit] x y [option]` (`Moveコマンド.md`):
    //  - 2 引数: `Move x y` → 直前選択ユニットを移動
    //  - 3 引数: `Move unit x y`
    //  - 4 引数: `Move unit x y option` (`非同期` / `アニメ表示`) — オプションは
    //    本実装で無視 (常に即時移動)
    // 移動力制限・EN 消費は無視 (原典準拠)、母艦格納時は自動発進 (off_map=false)。
    // SRC `Move` 命令経由のスクリプト移動は `進入イベント` を **発火させない**
    // (原典: "Moveコマンド等でユニットを移動させた場合、進入イベントは発生しません")。
    let (target_key, nx, ny) = match xargs.len() {
        0 | 1 => return Err(err(line, "Move 命令は最低 2 引数必要 (x y)。")),
        2 => {
            let nx = parse_u32(app, xargs, 0, line)?;
            let ny = parse_u32(app, xargs, 1, line)?;
            (app.selected_unit_for_event().to_string(), nx, ny)
        }
        _ => {
            let target = xargs[0].clone();
            let nx = parse_u32(app, xargs, 1, line)?;
            let ny = parse_u32(app, xargs, 2, line)?;
            (target, nx, ny)
        }
    };
    let uid = app
        .database()
        .unit_instances
        .iter()
        .find(|u| matches_unit_handle(u, &target_key))
        .map(|u| u.uid.clone());
    let found = uid.is_some();
    if let Some(uid) = uid {
        app.database_mut().move_unit(&uid, nx, ny);
        app.database_mut().set_off_map(&uid, false);
    }
    // SRC.Sharp 準拠: 対象ユニットが存在しない場合はエラー (3 引数形式のみ)。
    if !found && xargs.len() >= 3 {
        return Err(err(
            line,
            &format!("Move: 対象「{target_key}」が見つかりません"),
        ));
    }
    Ok(pc + 1)
}

fn exec_kill(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    if xargs.is_empty() {
        return Err(err(line, "Kill 命令は対象名が必要。"));
    }
    // SRC.Sharp 準拠: 引数は正確に 1 個。
    if xargs.len() > 1 {
        return Err(err(line, "Kill/Destroyコマンドの引数の数が違います"));
    }
    let target = &xargs[0];
    // `Kill name` は名前が一致するもの全てを除去するので、複数該当
    // した場合は順に撃破ラベルを発火する。
    let destroyed = app
        .database_mut()
        .remove_units_where(|u| matches_unit_handle(u, target));
    for u in destroyed {
        fire_destruction_labels(app, &u.pilot_name, &u.unit_data_name, u.party);
    }
    Ok(pc + 1)
}

fn exec_create(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `Create party unit rank pilot level x y [ID option]`
    // (本実装では rank / level / ID / option は無視。)
    // unit / pilot は裸識別子で渡されることが多いので fn_arg_value
    // で script_var 解決する (`Create 味方 入手ユニット 0 パイロット不在 ...`
    // → 入手ユニット の値で実際のユニット名が決まる)。
    if xargs.len() < 7 {
        return Err(err(
            line,
            "Create 命令は 7 引数必要 (party unit rank pilot level x y)。",
        ));
    }
    let party = parse_party(&xargs[0], line)?;
    let unit_data_name = xargs[1].to_string();
    // rank = xargs[2] = 改造段階。SRC `UList.Add(uname, urank, uparty)` は
    // `Unit.Rank=urank` で 1 段ごとに HP+200/装甲+100/EN+10/運動性+5 を加える
    // (差分オラクル placeunit で C# 実測: rank2 → MaxHP+400・装甲+200)。
    // 本実装の改造段階は `UnitInstance.upgrade_level`・`effective_*` がこの加算を
    // 持つので、rank をそこへ配線する (旧実装は rank を捨てて常に素のステータスだった)。
    let rank: i32 = xargs[2].to_string().parse().unwrap_or(0);
    let pilot = xargs[3].to_string();
    // level = xargs[4] = 主パイロットの初期レベル。SRC `PList.Add(pname, plevel, …)` は
    // `Pilot.Level=plevel` でレベルを設定する。本実装はレベルを累積経験値から導くため
    // (`level_from_exp`)、level を `exp_for_level` で初期 total_exp へ変換する
    // (旧実装は level を捨てて常にレベル 1 だった = 増援が指定レベルにならなかった)。
    // 注: レベルに応じた能力値成長 (`grown_pilot`) は class ベースの簡易式で、SRC の
    // 成長タイプ準拠成長とは乖離する (穴埋めロードマップ・docs/SRC_SHARP_DIVERGENCE.md)。
    let level: i32 = xargs[4].to_string().parse().unwrap_or(1);
    // 座標は app-aware 式評価で解決する。SRC は座標を式として評価するため、
    // `For i = ... / Create 中立 壁 0 P 1 4 i / Next` のような**裸のループ変数**や
    // 算術式が座標に来る (`eval_coord_u32`)。
    let x = eval_coord_u32(app, xargs, 5);
    let y = eval_coord_u32(app, xargs, 6);
    let mut inst = UnitInstance::new(unit_data_name, pilot.clone(), party, x, y);
    inst.upgrade_level = rank.max(0);
    inst.total_exp = crate::pilot_instance::exp_for_level(level);
    populate_active_features(&mut inst, app);
    let uid = app.database_mut().register_unit(inst);
    // 対象ユニットＩＤ は最新作成ユニットの uid (一意)、
    // 対象パイロット は pilot 名 (重複可) を入れる。
    // これで後続の `Escape 対象ユニットＩＤ` が正しいユニットを参照する。
    app.set_script_var("対象ユニットＩＤ".to_string(), uid);
    app.set_script_var("対象パイロット".to_string(), pilot);
    Ok(pc + 1)
}

fn exec_removeunit(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    if xargs.is_empty() {
        return Err(err(line, "RemoveUnit には対象名が必要。"));
    }
    let target = &xargs[0];
    app.database_mut()
        .remove_units_where(|u| matches_unit_handle(u, target));
    Ok(pc + 1)
}

fn exec_removepilot(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // パイロット定義のみ削除。SRC では unit_instance も連動削除する
    // 仕様だが、機体選択フローの `Escape → Getoff → RemovePilot` の
    // 順では既に Getoff でパイロット名がクリアされている前提で
    // 後段の Removepilot は安全。本実装では Getoff が pilot_name を
    // 空にするので、ここで pilot_name 一致削除しても影響なしのはず。
    if xargs.is_empty() {
        return Err(err(line, "RemovePilot には対象名が必要。"));
    }
    let target = xargs[0].to_string();
    let db = app.database_mut();
    db.pilots
        .retain(|p| p.name != target && p.nickname != target);
    Ok(pc + 1)
}
