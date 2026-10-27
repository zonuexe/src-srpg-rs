//! `.eve` コマンド: ユニット / パイロットの能力値・状態・装備・成長。

use super::*;

pub(super) fn exec(app: &mut App, c: &Cmd) -> Option<Result<usize, ScriptError>> {
    Some(match c.lname.as_str() {
        "bossrank" => exec_bossrank(app, c),
        "setstatus" => exec_setstatus(app, c),
        "unsetstatus" | "clearstatus" => exec_unsetstatus(app, c),
        "forget" => exec_forget(app, c),
        "restore" | "restoreevent" => exec_restore(app, c),
        "setskill" => exec_setskill(app, c),
        "clearskill" => exec_clearskill(app, c),
        "clearspecialpower" => exec_clearspecialpower(app, c),
        "setbullet" => exec_setbullet(app, c),
        "disable" | "enable" => exec_disable(app, c),
        "supply" => exec_supply(app, c),
        "fix" => exec_fix(app, c),
        "release" => exec_release(app, c),
        "rankup" => exec_rankup(app, c),
        "upgrade" => exec_upgrade(app, c),
        "specialpower" | "mind" => exec_specialpower(app, c),
        "clearmind" => exec_clearmind(app, c),
        "setability" => exec_setability(app, c),
        "clearability" => exec_clearability(app, c),
        "setstock" => exec_setstock(app, c),
        "charge" => exec_charge(app, c),
        "decreasemorale" => exec_decreasemorale(app, c),
        "damage" => exec_damage(app, c),
        "heal" => exec_heal(app, c),
        "recoverhp" => exec_recoverhp(app, c),
        "recoveren" => exec_recoveren(app, c),
        "recoversp" | "recoverplana" => exec_recoversp(app, c),
        "increasemorale" => exec_increasemorale(app, c),
        "expup" => exec_expup(app, c),
        "item" => exec_item(app, c),
        "equip" => exec_equip(app, c),
        "removeitem" | "unequip" => exec_removeitem(app, c),
        "exchangeitem" => exec_exchangeitem(app, c),
        "levelup" => exec_levelup(app, c),
        _ => return None,
    })
}

fn exec_bossrank(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `BossRank [unit] rank` (`BossRankコマンド.md`): ボスランク (0〜5) を設定。
    // ユニットの `boss_rank` を更新 (HP/装甲/運動性/攻撃力強化 + 即死/石化/憑依無効 +
    // 衰/滅 半減は effective_* / 戦闘側が反映)。Rank() 関数互換のため script_var
    // `__rank_<unit>` にも保存する。unit 省略時は selected_unit_for_event。
    let (target, rank_str) = match xargs.len() {
        0 => return Ok(pc + 1),
        1 => (app.selected_unit_for_event().to_string(), xargs[0].clone()),
        _ => (xargs[0].clone(), xargs[1].clone()),
    };
    let rank: i32 = fn_arg_value(app, &rank_str)
        .parse()
        .unwrap_or(0)
        .clamp(0, 5);
    app.set_script_var(format!("__rank_{target}"), rank.to_string());
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &target))
    {
        u.boss_rank = rank;
    }
    Ok(pc + 1)
}

fn exec_setstatus(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `SetStatus unit "状態"`
    if xargs.len() < 2 {
        return Err(err(line, "SetStatus 命令は 2 引数必要 (unit status)。"));
    }
    let target = xargs[0].clone();
    let status = xargs[1].clone();
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &target))
    {
        u.add_condition(Condition::new(status, -1));
    }
    Ok(pc + 1)
}

fn exec_unsetstatus(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `ClearStatus [unit] <status>` (`ClearStatusコマンド.md`):
    // unit 省略時は `selected_unit_for_event` を使う。status を指定
    // して状態異常を解除する。`UnsetStatus` も同等の SRC 命令。
    // `setstatus` の逆操作。
    let (target, status) = match xargs.len() {
        0 => return Err(err(line, "ClearStatus 命令は最低 1 引数必要 (status)。")),
        1 => (app.selected_unit_for_event().to_string(), xargs[0].clone()),
        _ => (xargs[0].clone(), xargs[1].clone()),
    };
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &target))
    {
        u.remove_condition(&status);
    }
    Ok(pc + 1)
}

fn exec_forget(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        args,
        xargs,
        line,
        pc,
        ..
    } = *c;
    // `Forget title` — タイトル (作品) のロードリストから削除。
    // SRC.Sharp `ForgetCmd.cs` 準拠: 引数 1 つ必須。
    // 注: データは即削除されず、次回ロード時に対象外となるだけ。
    if args.len() != 1 {
        return Err(err(line, "Forgetコマンドの引数の数が違います"));
    }
    if let Some(tname) = xargs.first() {
        let tname = tname.trim_matches('"').to_string();
        app.titles_mut().retain(|t| t != &tname);
    }
    Ok(pc + 1)
}

fn exec_restore(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `Restore label` — ラベル再登録。statements 内を探して最初の
    // 該当ラベルを labels に書き戻す。
    // SRC.Sharp 準拠: 引数なしはエラー。
    if xargs.is_empty() {
        return Err(err(line, "RestoreEventコマンドには引数が必要です"));
    }
    if let Some(label) = xargs.first() {
        let key = label
            .trim_start_matches('@')
            .trim_end_matches(':')
            .to_string();
        let lib = app.script_library_mut();
        let stmts = lib.statements.clone();
        for (i, s) in stmts.iter().enumerate() {
            if let EventStatement::Command { name, .. } = s {
                if let Some(canon) = canonical_label(name) {
                    if canon == key {
                        lib.labels_lower.insert(key.to_ascii_lowercase(), i);
                        lib.labels.insert(key.clone(), i);
                        // ClearEvent で無効化されていた場合は再度有効化する。
                        lib.enable_pc(i);
                        break;
                    }
                }
            }
        }
    }
    Ok(pc + 1)
}

fn exec_setskill(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `SetSkill pilot skill level [name]` (`SetSkillコマンド.md`):
    // パイロットに特殊能力を追加。level=0 で封印、-1 でレベル表示なし。
    // PilotInstance.skills に "スキル名" または "スキル名 N" 形式で保持。
    // PilotInstance が存在しない場合は無視 (Place 後にのみ有効)。
    if xargs.len() < 3 {
        return Err(err(
            line,
            "SetSkill 命令は 3 引数必要 (pilot skill level)。",
        ));
    }
    let pilot_key = xargs[0].clone();
    let skill_name = xargs[1].clone();
    let level: i32 = xargs[2].parse().unwrap_or(0);
    // PilotInstance を探す。無ければパイロット名で作成。
    let idx = app
        .database()
        .pilot_instances
        .iter()
        .position(|p| p.id == pilot_key || p.pilot_data_name == pilot_key);
    let idx = idx.or_else(|| {
        // PilotData の名前 / 通称でも探す
        let pname = app
            .database()
            .pilots
            .iter()
            .find(|p| p.name == pilot_key || p.nickname == pilot_key)
            .map(|p| p.name.clone())?;
        app.database_mut().create_pilot_instance(&pname, &pname)?;
        app.database()
            .pilot_instances
            .iter()
            .position(|p| p.pilot_data_name == pname)
    });
    if let Some(idx) = idx {
        // level=0: 封印 → スキルをリストから除去
        if level == 0 {
            app.database_mut().pilot_instances[idx]
                .skills
                .retain(|s| !s.starts_with(&skill_name));
        } else {
            let entry = if level == -1 {
                skill_name.clone()
            } else {
                format!("{} {}", skill_name, level)
            };
            let skills = &mut app.database_mut().pilot_instances[idx].skills;
            // 既存エントリを更新
            if let Some(pos) = skills.iter().position(|s| s.starts_with(&skill_name)) {
                skills[pos] = entry;
            } else {
                skills.push(entry);
            }
        }
    }
    Ok(pc + 1)
}

fn exec_clearskill(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `ClearSkill pilot skill` (`ClearSkillコマンド.md`):
    // SetSkill で付与した skill を解除。
    if xargs.len() < 2 {
        return Err(err(line, "ClearSkill 命令は 2 引数必要 (pilot skill)。"));
    }
    let pilot_key = xargs[0].clone();
    let skill_name = xargs[1].clone();
    let idx = app
        .database()
        .pilot_instances
        .iter()
        .position(|p| p.id == pilot_key || p.pilot_data_name == pilot_key);
    if let Some(idx) = idx {
        app.database_mut().pilot_instances[idx]
            .skills
            .retain(|s| !s.starts_with(&skill_name));
    }
    Ok(pc + 1)
}

fn exec_clearspecialpower(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `ClearSpecialPower unit [sp_name]` (`ClearSpecialPowerコマンド.md`):
    // ユニットに付与されている SP buff (condition) を解除。
    // sp_name 省略時は全 SP 系 condition を解除する。
    // SP 系の判定は本実装で「精神コマンド名と一致する condition」と
    // して扱う (熱血/魂/激励/集中/必中/不屈 等)。簡略化して引数指定
    // 必須のパスを優先 — 省略時は全 condition 解除。
    if xargs.is_empty() {
        return Err(err(
            line,
            "ClearSpecialPower 命令は最低 1 引数必要 (unit)。",
        ));
    }
    let target = xargs[0].clone();
    let sp_name = xargs.get(1).cloned();
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &target))
    {
        if let Some(name) = sp_name {
            u.remove_condition(&name);
        } else {
            // 全 condition 解除 (SP 由来かの区別はせず簡略化)
            u.conditions.clear();
        }
    }
    Ok(pc + 1)
}

fn exec_setbullet(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `SetBullet unit_name weapon_name n` — 残弾数を直接設定。
    // UnitInstance に弾管理を持たないため、UnitData.weapons[i].bullet を
    // 書き換える（インスタンス毎ではないが SRC 流の最小表現）。
    if xargs.len() < 3 {
        return Err(err(line, "SetBullet 命令は 3 引数必要 (unit weapon n)。"));
    }
    let unit_name = xargs[0].clone();
    let weapon_name = xargs[1].clone();
    let n: i32 = xargs[2].parse().unwrap_or(0);
    // unit_data_name か pilot_name で unit_data を引く
    let data_name = app
        .database()
        .unit_instances
        .iter()
        .find(|u| matches_unit_handle(u, &unit_name))
        .map(|u| u.unit_data_name.clone());
    if let Some(d) = data_name {
        if let Some(unit_data) = app.database_mut().units.iter_mut().find(|u| u.name == d) {
            if let Some(w) = unit_data.weapons.iter_mut().find(|w| w.name == weapon_name) {
                w.bullet = n;
            }
        }
    }
    Ok(pc + 1)
}

fn exec_disable(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        lname,
        xargs,
        line,
        pc,
        ..
    } = *c;
    // SRC `Disable [unit] name` / `Enable [unit] name`
    // (`Disableコマンド.md` / `Enableコマンド.md`):
    // 武器/アビリティ/特殊能力/形態/アイテムを使用可否切替。
    // SRC.Sharp 準拠の変数名:
    //   1 引数: `Disable(<name>)` = 1 (グローバル; 全ユニット共通)
    //   2 引数: `Disable(<unit>,<name>)` = 1 (ユニット個別)
    // Enable はその変数を削除する。
    // セーブデータにも保存される (script_var 経由なので serde 通る)。
    let is_disable = lname == "disable";
    // C# DisableCmd.cs: switch(ArgNum) — case 2 (global) / case 3 (unit-specific)、
    // それ以外は EventErrorException。
    let (key, ability_name, unit_name_opt) = match xargs.len() {
        0 | 3.. => return Err(err(line, "Disable/Enable コマンドの引数の数が違います")),
        1 => (format!("Disable({})", xargs[0]), xargs[0].clone(), None),
        2 => (
            format!("Disable({},{})", xargs[0], xargs[1]),
            xargs[1].clone(),
            Some(xargs[0].clone()),
        ),
    };
    if is_disable {
        app.set_script_var(key, "1".to_string());
    } else {
        app.unset_script_var(&key);
    }
    // UnitInstance の weapon.is_disabled フラグも同期する。
    // C# では Unit.Update() で再計算されるが、Rust は直接フラグをセットする。
    let n = app.database().unit_instances.len();
    for i in 0..n {
        let matches = match &unit_name_opt {
            Some(uname) => {
                let u = &app.database().unit_instances[i];
                matches_unit_handle(u, uname)
            }
            None => true, // グローバル: 全ユニット対象
        };
        if matches {
            for w in &mut app.database_mut().unit_instances[i].weapons {
                if w.weapon_data_name == ability_name {
                    w.is_disabled = is_disable;
                }
            }
            // 特殊能力 (active_features) も切替える。`Disable unit ＨＰ回復` のように
            // レベル無し基底名で指定された能力を実体 (`ＨＰ回復Lv1` 等) と突き合わせ、
            // is_active を反転する。回復系等は feature_level が is_active を見るため、
            // これで「水上=ＨＰ回復ON / 陸上=OFF」(決戦2話の核ギミック) が成立する。
            for af in &mut app.database_mut().unit_instances[i].active_features {
                if crate::feature::feature_name_matches_base(&af.name, &ability_name) {
                    af.is_active = !is_disable;
                }
            }
        }
    }
    Ok(pc + 1)
}

fn exec_supply(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Supply unit` — HP/EN を完全回復
    if let Some(target) = xargs.first() {
        recover_hp(app, target, None);
        recover_en(app, target, None);
    }
    Ok(pc + 1)
}

fn exec_fix(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Fix name` — SRC `Fixコマンド.md`:
    // パイロットまたはアイテムをインターミッションでの乗り換え・
    // 交換対象外に固定する。イベントによる強制交換 (`Ride`/`Equip`) は
    // 影響を受けない。
    //
    // 引数省略時はデフォルトユニットのメインパイロットを固定する。
    let target = if let Some(t) = xargs.first() {
        fn_arg_value(app, t)
    } else {
        // 省略時: selected_unit_for_event が指すユニットのパイロット
        let uid = app.selected_unit_for_event().to_string();
        app.database()
            .unit_instances
            .iter()
            .find(|u| u.uid == uid || matches_unit_handle(u, &uid))
            .map(|u| u.pilot_name.clone())
            .unwrap_or_default()
    };
    if !target.is_empty() {
        // アイテムスロット内の name 一致を検索し is_fixed を立てる
        let mut found_item = false;
        for u in app.database_mut().unit_instances.iter_mut() {
            for slot in u.item_slots.iter_mut() {
                if slot.equipped_item.as_deref() == Some(target.as_str()) {
                    slot.is_fixed = true;
                    found_item = true;
                }
            }
        }
        // アイテムが見つからなければパイロットとして固定
        if !found_item {
            for u in app.database_mut().unit_instances.iter_mut() {
                if u.pilot_name == target {
                    for inst in u.pilot_ids.iter() {
                        let _ = inst; // pilot_instance 固定は下で処理
                    }
                }
            }
            // PilotInstance.is_fixed を設定
            for inst in app.database_mut().pilot_instances.iter_mut() {
                if inst.pilot_data_name == target || inst.id == target {
                    inst.is_fixed = true;
                }
            }
        }
    }
    Ok(pc + 1)
}

fn exec_release(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Release [name]` — SRC `Releaseコマンド.md`:
    // `Fix` で固定されたパイロット・アイテムの固定を解除する。
    // 引数省略時は全パイロット・全アイテムスロットの固定を解除。
    let target = xargs.first().map(|t| fn_arg_value(app, t));
    match &target {
        None => {
            // 全解除
            for u in app.database_mut().unit_instances.iter_mut() {
                for slot in u.item_slots.iter_mut() {
                    slot.is_fixed = false;
                }
            }
            for inst in app.database_mut().pilot_instances.iter_mut() {
                inst.is_fixed = false;
            }
        }
        Some(name) if !name.is_empty() => {
            // アイテム解除
            for u in app.database_mut().unit_instances.iter_mut() {
                for slot in u.item_slots.iter_mut() {
                    if slot.equipped_item.as_deref() == Some(name.as_str()) {
                        slot.is_fixed = false;
                    }
                }
            }
            // パイロット解除
            for inst in app.database_mut().pilot_instances.iter_mut() {
                if inst.pilot_data_name == *name || inst.id == *name {
                    inst.is_fixed = false;
                }
            }
        }
        _ => {}
    }
    Ok(pc + 1)
}

fn exec_rankup(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `RankUp pilot [n=1]` — pilot の rank を script_var に記録
    if let Some(target) = xargs.first() {
        let n: i32 = xargs.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
        let key = format!("__rank_{target}");
        let cur: i32 = app.script_var(&key).parse().unwrap_or(0);
        app.set_script_var(key, (cur + n).to_string());
    }
    Ok(pc + 1)
}

fn exec_upgrade(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `Upgrade [unit1] unit2` (`Upgradeコマンド.md` / C# `UpgradeCmd`):
    // unit1 を unit2 のユニットデータに **入れ替える** (機体グレードアップ /
    // ボスの形態変化)。ユニットランク (改造段階) は維持、HP/EN/弾薬は全快、
    // パイロット・位置・陣営は引き継ぐ。unit1 省略時は selected_unit_for_event。
    // (旧実装の `Upgrade unit attr n` ステータス加算は SRC に無い誤実装だった。)
    let (target_key, new_unit) = match xargs.len() {
        0 => return Ok(pc + 1),
        1 => (app.selected_unit_for_event().to_string(), xargs[0].clone()),
        _ => (xargs[0].clone(), xargs[1].clone()),
    };
    // 変換先ユニットデータが無ければ no-op (堅牢性優先)。
    if app.database().unit_by_name(&new_unit).is_none() {
        return Ok(pc + 1);
    }
    let Some(idx) = app
        .database()
        .unit_instances
        .iter()
        .position(|u| matches_unit_handle(u, &target_key))
    else {
        return Ok(pc + 1);
    };
    // インスタンスを取り出して機体データを差し替え、data 由来状態を再初期化。
    let mut inst = app.database().unit_instances[idx].clone();
    inst.unit_data_name = new_unit;
    inst.damage = 0; // HP 全快
    inst.en_consumed = 0; // EN 全快
    inst.weapons.clear(); // 弾薬全快 (空 = 残弾フル)
    inst.abilities.clear();
    inst.active_features.clear();
    inst.life_state.clear(); // 破壊扱いを解除 (= 復活)
    populate_active_features(&mut inst, app);
    app.database_mut().unit_instances[idx] = inst;
    Ok(pc + 1)
}

fn exec_specialpower(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `SpecialPower [unit] spower [target]` (`SpecialPowerコマンド.md`)。
    // `Mind pilot spower` は Ver.1.6 までの同等コマンド。SP は消費せず、効果は
    // App の精神コマンド経路 (`execute_spirit_by_event` → `apply_spirit_effect`)
    // で適用する。引数の解釈は C# `SpecialPowerCmd` に合わせる:
    //  - 1 引数: `spower` (使用者 = SelectedUnitForEvent)
    //  - 2 引数: `spower target` (みがわり/挑発 効果を持つ SP) / `unit spower`
    //  - 3 引数: `unit spower target`
    // SRC はユニット指定にシステム変数 (相手ユニットＩＤ 等) を許す。裸の変数名
    // (生贄ルーチンの `SpecialPower 相手ユニットＩＤ みがわり 対象ユニットＩＤ`) は
    // 実ユニットに一致しなければ同名 script_var の値 (uid) へフォールバック解決する。
    let selected = || app.selected_unit_for_event().to_string();
    let (caster_key, power, target_key) = match xargs.len() {
        0 => return Err(err(line, "SpecialPowerコマンドの引数の数が違います")),
        1 => (selected(), xargs[0].clone(), None),
        2 if app.spirit_needs_target(&xargs[0]) => {
            (selected(), xargs[0].clone(), Some(xargs[1].clone()))
        }
        2 => (xargs[0].clone(), xargs[1].clone(), None),
        _ => (xargs[0].clone(), xargs[1].clone(), Some(xargs[2].clone())),
    };
    let uid_of = |app: &App, key: String| {
        let key = resolve_handle_var(app, key);
        app.database()
            .unit_instances
            .iter()
            .find(|u| matches_unit_handle(u, &key))
            .map(|u| u.uid.clone())
    };
    let Some(caster) = uid_of(app, caster_key) else {
        return Ok(pc + 1);
    };
    let target = match target_key {
        Some(k) => match uid_of(app, k) {
            Some(t) => Some(t),
            None => return Ok(pc + 1),
        },
        None => None,
    };
    app.execute_spirit_by_event(&caster, target.as_deref(), &power);
    Ok(pc + 1)
}

fn exec_clearmind(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `ClearMind pilot [sp_name]` — 精神コマンド効果を解除。
    // sp_name 省略時は全精神コマンド系 condition を解除。
    if xargs.is_empty() {
        return Ok(pc + 1);
    }
    let pilot_key = xargs[0].clone();
    let sp_name = xargs.get(1).cloned();
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| u.pilot_name == pilot_key || matches_unit_handle(u, &pilot_key))
    {
        if let Some(name) = sp_name {
            u.conditions.retain(|c| c.name != name);
        } else {
            u.conditions.clear();
        }
    }
    Ok(pc + 1)
}

fn exec_setability(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs,
        line,
        pc,
        stmts,
        labels,
        ..
    } = *c;
    // `SetAbility pilot skill level` — パイロットに特殊能力を付与。
    // `SetSkill` と同義 (こちらが旧名称)。同じ実装に委譲する。
    if xargs.len() < 3 {
        return Ok(pc + 1);
    }
    exec_command_pc(app, "SetSkill", xargs, xargs, line, pc, stmts, labels)
}

fn exec_clearability(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs,
        line,
        pc,
        stmts,
        labels,
        ..
    } = *c;
    // `ClearAbility pilot skill` — パイロットの特殊能力を削除。
    // `ClearSkill` と同義 (こちらが旧名称)。
    if xargs.len() < 2 {
        return Ok(pc + 1);
    }
    exec_command_pc(app, "ClearSkill", xargs, xargs, line, pc, stmts, labels)
}

fn exec_setstock(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `SetStock [unit] ability stock` (`SetStockコマンド.md`):
    // ユニットのアビリティ残り使用回数を指定値に変更する。
    if xargs.len() < 2 {
        return Ok(pc + 1);
    }
    // 引数は `unit ability stock` または `ability stock` の 2 形式。
    let (unit_key, ability_key, stock_raw) = if xargs.len() >= 3 {
        (xargs[0].clone(), xargs[1].clone(), xargs[2].clone())
    } else {
        // unit 省略: 選択中ユニット
        let uid = app.selected_unit_for_event().to_string();
        (uid, xargs[0].clone(), xargs[1].clone())
    };
    let stock: i32 = stock_raw.parse().unwrap_or(0);
    // アビリティ番号指定 (1-origin 数字) か名前指定かを判断。
    // 番号指定の場合は UnitData.features から名前を解決する。
    let resolved_ability_name: Option<String> = if let Ok(n) = ability_key.parse::<usize>() {
        let idx = n.saturating_sub(1);
        app.database()
            .unit_instances
            .iter()
            .find(|u| matches_unit_handle(u, &unit_key))
            .and_then(|u| app.database().unit_by_name(&u.unit_data_name))
            .and_then(|d| d.features.get(idx))
            .map(|(name, _)| name.clone())
    } else {
        Some(ability_key.clone())
    };
    if let Some(ability_name) = resolved_ability_name {
        if let Some(u) = app
            .database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| matches_unit_handle(u, &unit_key))
        {
            u.ability_stocks.insert(ability_name, stock);
        }
    }
    Ok(pc + 1)
}

fn exec_charge(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `Charge [unit]` (`Chargeコマンド.md`):
    // 指定ユニットの charged フラグを true にする。次の攻撃でチャージ攻撃属性
    // (`Ｃ` 属性) の武器が解禁される想定。本実装は属性連動を未対応だが、
    // フラグだけ立てて `IsAvailable(unit, チャージ)` 風の判定に使える状態にする。
    // 引数 0: 直前選択ユニット、1+: unit 指定。
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
        u.charged = true;
    }
    Ok(pc + 1)
}

fn exec_decreasemorale(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `DecreaseMorale [unit] value` (`IncreaseMoraleコマンド.md` の逆):
    // ユニットの気力を減少させる。
    if xargs.is_empty() {
        return Err(err(line, "DecreaseMorale には対象名が必要。"));
    }
    let delta = if xargs.len() >= 2 {
        parse_i32_at(&xargs[1], line)?
    } else {
        5
    };
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &xargs[0]))
    {
        // C# では DecreaseMorale は IncreaseMorale の逆で同じ Pilot.SetMorale を
        // 経由するため、下限は 50 (MinMorale 既定値)。
        u.morale = (u.morale - delta).clamp(50, 150);
    }
    Ok(pc + 1)
}

fn exec_damage(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // Damage unit_name amount
    if xargs.len() < 2 {
        return Err(err(line, "Damage 命令は 2 引数必要 (name amount)。"));
    }
    let target = &xargs[0];
    let amount = parse_i64_at(&xargs[1], line)?;
    apply_damage(app, target, amount);
    Ok(pc + 1)
}

fn exec_heal(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // Heal unit_name amount  (負ダメージで HP 回復)
    if xargs.len() < 2 {
        return Err(err(line, "Heal 命令は 2 引数必要 (name amount)。"));
    }
    let target = &xargs[0];
    let amount = parse_i64_at(&xargs[1], line)?;
    apply_damage(app, target, -amount);
    Ok(pc + 1)
}

fn exec_recoverhp(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `RecoverHP [unit] rate` (SRC.Sharp `RecoverHPCmd.cs` 準拠)
    // unit は省略可能。省略時は `selected_unit_for_event` を使用。
    // rate は HP の回復率 (%)。負の値で HP を減少させることもできる。
    // RecoverHP によって HP が 0 以下になることはない (最低 1)。
    // C# RecoverHPCmd.cs: ArgNum==2 (1 user arg) or ArgNum==3 (2 user args)、それ以外エラー。
    match xargs.len() {
        0 | 3.. => return Err(err(line, "RecoverHP の引数の数が違います。")),
        1 => {
            // RecoverHP rate — 選択ユニットを使用
            let key = app.selected_unit_for_event().to_string();
            if !key.is_empty() {
                recover_hp(app, &key, Some(&xargs[0]));
            }
        }
        2 => {
            // RecoverHP unit rate
            let key = xargs[0].clone();
            recover_hp(app, &key, Some(&xargs[1]));
        }
    }
    Ok(pc + 1)
}

fn exec_recoveren(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `RecoverEN [unit] rate` — HP と同様にユニット省略可能。
    // C# RecoverENCmd.cs: ArgNum==2 or ArgNum==3、それ以外エラー。
    match xargs.len() {
        0 | 3.. => return Err(err(line, "RecoverEN の引数の数が違います。")),
        1 => {
            let key = app.selected_unit_for_event().to_string();
            if !key.is_empty() {
                recover_en(app, &key, Some(&xargs[0]));
            }
        }
        2 => {
            let key = xargs[0].clone();
            recover_en(app, &key, Some(&xargs[1]));
        }
    }
    Ok(pc + 1)
}

fn exec_recoversp(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `RecoverSP [pilot] rate` (`RecoverSPコマンド.md`):
    // パイロット名 (省略時はデフォルトユニットのメインパイロット) の SP を
    // rate% 回復する。rate は浮動小数点も可。
    // 実装は UnitInstance.sp_consumed で SP を追跡。
    // パイロット名からユニットを逆引き (pilot_name 一致)。
    // C# RecoverSPCmd.cs: ArgNum==2 (rate only) or ArgNum==3 (pilot + rate)。
    // 1引数 = rate (デフォルトユニット)、2引数 = pilot + rate。
    let (pilot_key, rate_str): (String, &str) = match xargs.len() {
        0 => {
            return Err(err(line, "RecoverSPコマンドの引数の数が違います"));
        }
        1 => {
            // rate のみ → デフォルトユニットのメインパイロット
            let key = app.selected_unit_for_event().to_string();
            (key, xargs[0].as_str())
        }
        _ => (xargs[0].clone(), xargs[1].as_str()),
    };
    // パイロット名 or ユニット識別子で UnitInstance を探す
    let idx =
        app.database().unit_instances.iter().position(|u| {
            matches_unit_handle(u, &pilot_key)
                || u.pilot_name == pilot_key
                || app.database().pilots.iter().any(|p| {
                    (p.name == pilot_key || p.nickname == pilot_key) && u.pilot_name == p.name
                })
        });
    if let Some(idx) = idx {
        let max_sp = {
            let inst = &app.database().unit_instances[idx];
            app.database()
                .pilot_by_name(&inst.pilot_name)
                .and_then(|p| p.sp)
                .unwrap_or(0)
        };
        let cur = app.database().unit_instances[idx].sp_consumed;
        let new_consumed = if rate_str.eq_ignore_ascii_case("full") || rate_str == "全" {
            0
        } else {
            let pct: f64 = rate_str.trim_end_matches('%').parse().unwrap_or(0.0);
            let recover = ((max_sp as f64) * pct / 100.0) as i32;
            (cur - recover).max(0)
        };
        app.database_mut().unit_instances[idx].sp_consumed = new_consumed;
    }
    Ok(pc + 1)
}

fn exec_increasemorale(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // IncreaseMorale unit [delta]
    if xargs.is_empty() {
        return Err(err(line, "IncreaseMorale には対象名が必要。"));
    }
    let delta = if xargs.len() >= 2 {
        parse_i32_at(&xargs[1], line)?
    } else {
        5
    };
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &xargs[0]))
    {
        // C# IncreaseMoraleCmd.cs: モラルは [50, 150] にクランプ。
        u.morale = (u.morale + delta).clamp(50, 150);
    }
    Ok(pc + 1)
}

fn exec_expup(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // ExpUp unit n
    if xargs.len() < 2 {
        return Err(err(line, "ExpUp 命令は 2 引数必要 (name n)。"));
    }
    let n = parse_i32_at(&xargs[1], line)?;
    let target_idx = app
        .database()
        .unit_instances
        .iter()
        .position(|u| matches_unit_handle(u, &xargs[0]));
    // `Level()` 関数の規約に合わせ、`level_from_exp` (SRC: 500 exp/level) で算出。
    // レベル繰り上がりは event 発火条件。
    let (old_level, new_level, pilot_ids): (i32, i32, Vec<String>) = if let Some(i) = target_idx {
        let u = &mut app.database_mut().unit_instances[i];
        let old = crate::pilot_instance::level_from_exp(u.total_exp);
        u.total_exp += n;
        let new_ = crate::pilot_instance::level_from_exp(u.total_exp);
        (old, new_, u.pilot_ids.clone())
    } else {
        (0, 0, Vec::new())
    };
    for pilot_id in pilot_ids {
        let pilot_data_name = app
            .database()
            .pilot_instance_by_id(&pilot_id)
            .map(|p| p.pilot_data_name.clone());
        let pilot_data = pilot_data_name
            .as_ref()
            .and_then(|name| app.database().pilot_by_name(name).cloned());
        if let Some(pilot_data) = pilot_data {
            if let Some(pilot_inst) = app.database_mut().pilot_instance_by_id_mut(&pilot_id) {
                if pilot_inst.add_exp(n) {
                    pilot_inst.apply_stat_growth(&pilot_data);
                }
            }
        }
    }
    // SRC `LevelUp <unit>:` (`レベルアップイベント.md`) — メインパイロット
    // のレベルアップ時に発火。本実装は unit 単位 (UnitInstance.total_exp /
    // 100 + 1) で level を扱うため、その値が増加した場合に発火する。
    // pilot/unit/party いずれの綴でも 1 度発火 (`fire_unit_event_labels`)。
    if new_level > old_level {
        if let Some(i) = target_idx {
            let u = &app.database().unit_instances[i];
            let pilot_name = u.pilot_name.clone();
            let unit_data_name = u.unit_data_name.clone();
            let party = u.party;
            fire_unit_event_labels(
                app,
                &["レベルアップ", "LevelUp"],
                &pilot_name,
                &unit_data_name,
                party,
                None,
            );
        }
    }
    Ok(pc + 1)
}

fn exec_item(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `Itemコマンド.md`:
    //   書式1: `Item item_name` (1 引数) — アイテムを作成して
    //     未装備在庫 (`spare_items`) に追加。装備は後続の `Equip` コマンドで。
    //     実シナリオ (`shop_01` / `DUInclude` 等) で頻繁に使われる形式。
    //   書式2: `Item unit item_name` (2 引数, Equip と同義の互換形式) —
    //     ユニットに直接装備させる。旧シナリオ互換として残す。
    if xargs.is_empty() {
        return Ok(pc + 1);
    }
    if xargs.len() == 1 {
        let item_name = xargs[0].clone();
        app.database_mut().spare_items.push(item_name);
        return Ok(pc + 1);
    }
    // 2 引数以上 → Equip と同義で処理。
    let target = xargs[0].clone();
    let item_name = xargs[1].clone();
    let slot_type = app
        .database()
        .item_by_name(&item_name)
        .and_then(|it| SlotType::parse(&it.part))
        .unwrap_or(SlotType::Item);
    let mut equipped = false;
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &target))
    {
        if !u.has_item_equipped(&item_name) {
            u.equip_item(slot_type, item_name.clone());
            equipped = true;
        }
    }
    if equipped {
        if let Some(idx) = app
            .database()
            .spare_items
            .iter()
            .position(|s| s == &item_name)
        {
            app.database_mut().spare_items.remove(idx);
        }
    }
    Ok(pc + 1)
}

fn exec_equip(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Equip [unit] item` — ユニットにアイテムを装備させる。
    //   1 引数: `Equip item_name` — 選択中ユニット (selected_unit_for_event) に装備。
    //   2 引数: `Equip unit item_name` — 指定ユニットに装備。
    // 在庫にあれば消費し、なければ新規装備として扱う。
    if xargs.is_empty() {
        return Ok(pc + 1);
    }
    let (target, item_name) = if xargs.len() == 1 {
        (app.selected_unit_for_event().to_string(), xargs[0].clone())
    } else {
        (xargs[0].clone(), xargs[1].clone())
    };
    let slot_type = app
        .database()
        .item_by_name(&item_name)
        .and_then(|it| SlotType::parse(&it.part))
        .unwrap_or(SlotType::Item);
    let mut equipped = false;
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &target))
    {
        if !u.has_item_equipped(&item_name) {
            u.equip_item(slot_type, item_name.clone());
            equipped = true;
        }
    }
    // 装備したアイテムが未装備在庫にあれば 1 つ取り出す
    // (在庫 → ユニットへ移動)。
    if equipped {
        if let Some(idx) = app
            .database()
            .spare_items
            .iter()
            .position(|s| s == &item_name)
        {
            app.database_mut().spare_items.remove(idx);
        }
    }
    Ok(pc + 1)
}

fn exec_removeitem(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `RemoveItem [unit] [item]` (`RemoveItemコマンド.md`):
    // - item 指定あり: アイテムを **削除** (ユニットから外して破棄)。
    // - item 省略 (unit のみ / 両省略): ユニットの全アイテムを
    //   **取り外し** → 未装備在庫 (`spare_items`) へ移す。
    if xargs.len() >= 2 {
        // (4) unit + item: 指定アイテムを削除 (在庫には残さない)。
        let target = xargs[0].clone();
        let item = xargs[1].clone();
        if let Some(u) = app
            .database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| matches_unit_handle(u, &target))
        {
            u.unequip_item(&item);
        }
        return Ok(pc + 1);
    }
    // (1)(2) item 省略: 対象ユニットの全アイテムを取り外して在庫へ。
    let target = if let Some(t) = xargs.first() {
        fn_arg_value(app, t)
    } else {
        app.selected_unit_for_event().to_string()
    };
    let removed: Vec<String> = if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &target))
    {
        let names: Vec<String> = u
            .equipped_item_names()
            .into_iter()
            .map(String::from)
            .collect();
        for name in &names {
            u.unequip_item(name);
        }
        names
    } else {
        Vec::new()
    };
    app.database_mut().spare_items.extend(removed);
    Ok(pc + 1)
}

fn exec_exchangeitem(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    if xargs.len() < 3 {
        return Err(err(line, "ExchangeItem 命令は 3 引数必要 (unit old new)。"));
    }
    let target = xargs[0].clone();
    let old = xargs[1].clone();
    let new_item = xargs[2].clone();
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &target))
    {
        for slot in &mut u.item_slots {
            if slot.equipped_item.as_deref() == Some(&old) {
                if !slot.is_fixed {
                    slot.equipped_item = Some(new_item.clone());
                }
                break;
            }
        }
    }
    Ok(pc + 1)
}

fn exec_levelup(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // LevelUp unit [n]   ─ n レベル上げる (1 レベル = EXP_PER_LEVEL(500) exp)。
    // SRC.Sharp 準拠: 対象が存在しない場合はエラー。
    if xargs.is_empty() {
        return Err(err(line, "LevelUp には対象名が必要。"));
    }
    let n = if xargs.len() >= 2 {
        parse_i32_at(&xargs[1], line)?
    } else {
        1
    };
    let key = xargs[0].clone();
    let found = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .any(|u| matches_unit_handle(u, &key));
    if !found {
        return Err(err(
            line,
            &format!("LevelUp: 対象「{key}」が見つかりません"),
        ));
    }
    let pilot_ids: Vec<String> = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &key))
        .map(|u| {
            u.total_exp += n * crate::pilot_instance::EXP_PER_LEVEL;
            u.pilot_ids.clone()
        })
        .unwrap_or_default();
    for pilot_id in pilot_ids {
        let pilot_data_name = app
            .database()
            .pilot_instance_by_id(&pilot_id)
            .map(|p| p.pilot_data_name.clone());
        let pilot_data = pilot_data_name
            .as_ref()
            .and_then(|name| app.database().pilot_by_name(name).cloned());
        if let Some(pilot_data) = pilot_data {
            if let Some(pilot_inst) = app.database_mut().pilot_instance_by_id_mut(&pilot_id) {
                if pilot_inst.add_exp(n * crate::pilot_instance::EXP_PER_LEVEL) {
                    pilot_inst.apply_stat_growth(&pilot_data);
                }
            }
        }
    }
    Ok(pc + 1)
}
