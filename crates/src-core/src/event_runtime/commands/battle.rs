//! `.eve` コマンド: イベント上の戦闘・アビリティ・召喚。

use super::*;

pub(super) fn exec(app: &mut App, c: &Cmd) -> Option<Result<usize, ScriptError>> {
    Some(match c.lname.as_str() {
        "mapattack" | "mapweapon" => exec_mapattack(app, c),
        "mapability" => exec_mapability(app, c),
        "explode" => exec_explode(app, c),
        "stopsummoning" => exec_stopsummoning(app, c),
        "attack" => exec_attack(app, c),
        "useability" => exec_useability(app, c),
        _ => return None,
    })
}

fn exec_mapattack(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `MapAttack [unit] weapon X Y` — 座標 (X, Y) を中心にマップ攻撃。
    // `MapWeapon` は SRC Ver.1.6 までの旧名称で、現在は `MapAttack` と
    // 完全に同義 (SRC: MapAttackコマンド.md / 更新履歴(2003) で改名)。
    // 武器の max_range 内のすべての対立勢力ユニットに同時にダメージを
    // 与える。反撃 / 経験値 / 資金は無効 (SRC docs)。
    // 引数解釈: 最後の 2 つは X Y (数値), その手前は weapon, 残りがあれば unit。
    // 末尾に `通常戦闘` を付けると「使用/攻撃イベント」を発火する通常戦闘扱いに
    // なる (SRC `ExecMapAttackCmd` CmdData.cs:14865 — 既定 is_event=true、
    // 末尾 `通常戦闘` で is_event=false)。
    let mut xa: &[String] = xargs;
    let is_event = !matches!(xa.last().map(|s| s.as_str()), Some("通常戦闘"));
    if !is_event {
        xa = &xa[..xa.len() - 1];
    }
    if xa.len() < 3 {
        return Ok(pc + 1);
    }
    let n = xa.len();
    let cx: u32 = xa[n - 2].parse().unwrap_or(0);
    let cy: u32 = xa[n - 1].parse().unwrap_or(0);
    let weapon_name = xa[n - 3].clone();
    let unit_key = if n >= 4 {
        Some(xa[n - 4].clone())
    } else {
        None
    };
    script_map_attack(app, unit_key.as_deref(), &weapon_name, cx, cy, is_event);
    Ok(pc + 1)
}

fn exec_mapability(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // MapAbility は MapAttack と同じ書式の汎用版。本実装では同じ処理。
    if xargs.len() < 3 {
        return Ok(pc + 1);
    }
    let n = xargs.len();
    let cx: u32 = xargs[n - 2].parse().unwrap_or(0);
    let cy: u32 = xargs[n - 1].parse().unwrap_or(0);
    let weapon_name = xargs[n - 3].clone();
    let unit_key = if n >= 4 {
        Some(xargs[n - 4].clone())
    } else {
        None
    };
    // スクリプト経由の MapAbility も既定で is_event=true (イベント非発火)。
    script_map_attack(app, unit_key.as_deref(), &weapon_name, cx, cy, true);
    Ok(pc + 1)
}

fn exec_explode(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `Explode size [X Y]` (`Explodeコマンド.md`):
    // 爆発を (X, Y) で表示 (省略時は画面中央)。size = S/M/L/LL/XL。
    // 視覚演出のみで、ユニット破壊などの副作用は無い。
    // 本実装は ScriptOverlay::FillRect で円形ではなく大きさ別の正方形を
    // 描く近似で代用 (実描画は frontend 任せだが、command を実行した事実は
    // overlay に残る)。
    let size = xargs.first().map(String::as_str).unwrap_or("M");
    let radius_px: f64 = match size {
        "S" => 16.0,
        "M" => 24.0,
        "L" => 32.0,
        "LL" => 48.0,
        "XL" => 64.0,
        _ => 24.0,
    };
    let (cx, cy) = if xargs.len() >= 3 {
        let x: f64 = xargs[1].parse().unwrap_or(0.0);
        let y: f64 = xargs[2].parse().unwrap_or(0.0);
        // タイル座標から pixel に変換 (1 タイル = 32px)
        (x * 32.0 + 16.0, y * 32.0 + 16.0)
    } else {
        // 画面中央 (480/480 既定マップ想定)
        (240.0, 240.0)
    };
    app.script_overlay_mut()
        .push(crate::script_overlay::DrawCmd::SetColor {
            color: "#ff6f00".to_string(),
        });
    app.script_overlay_mut()
        .push(crate::script_overlay::DrawCmd::FillRect {
            x: cx - radius_px,
            y: cy - radius_px,
            w: radius_px * 2.0,
            h: radius_px * 2.0,
        });
    Ok(pc + 1)
}

fn exec_stopsummoning(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `StopSummoning [unit]` (`StopSummoningコマンド.md` / `召喚解除.md`):
    // 指定ユニットが召喚したユニットを解放 (マップから除去) する。
    // 引数省略時は selected_unit_for_event を対象とする。
    let summoner_key = if let Some(t) = xargs.first() {
        fn_arg_value(app, t)
    } else {
        app.selected_unit_for_event().to_string()
    };
    // 召喚親の uid を解決してから、その uid を親に持つ召喚ユニットを除去。
    let summoner_uid = app
        .database()
        .unit_instances
        .iter()
        .find(|u| matches_unit_handle(u, &summoner_key))
        .map(|u| u.uid.clone());
    if let Some(uid) = summoner_uid {
        // 空 uid は召喚親として無効 (誤一致防止)。
        if !uid.is_empty() {
            app.database_mut()
                .remove_units_where(|u| u.summoned_by.as_deref() == Some(uid.as_str()));
        }
    }
    Ok(pc + 1)
}

fn exec_attack(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `Attack unit1 weapon1 unit2 weapon2` (`Attackコマンド.md`):
    // イベント上の戦闘。命中・ダメージはプレイヤー / AI の攻撃と同じ判定
    // (`App::script_attack` → `resolve_strike`) で、使用条件 (射程/気力/EN) は問わない。
    // weapon1: `自動` で最強武器自動選択 / 武器名で固定
    // weapon2: `防御`/`回避`/`無抵抗`/`反撃不能` で反撃なし / `自動` or 武器名で反撃
    // 仕様準拠: 使用 / 攻撃 / 損傷率 / 破壊 / 全滅イベントは発火させず、経験値・資金も
    // 入らない ("Attackコマンドの結果、他のイベントが発生することはありません")。
    if xargs.len() < 4 {
        return Err(err(
            line,
            "Attack 命令は 4 引数必要 (unit1 weapon1 unit2 weapon2)。",
        ));
    }
    let uid_of = |app: &App, arg: &str| {
        let key = fn_arg_value(app, arg);
        app.database()
            .unit_instances
            .iter()
            .find(|u| matches_unit_handle(u, &key))
            .map(|u| u.uid.clone())
    };
    if let (Some(atk), Some(def)) = (uid_of(app, &xargs[0]), uid_of(app, &xargs[2])) {
        app.script_attack(&atk, &xargs[1], &def, &xargs[3]);
    }
    Ok(pc + 1)
}

fn exec_useability(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `UseAbility [unit] ability [target]` (`UseAbilityコマンド.md`):
    // ユニットに指定アビリティを発動させる。使用者のユニットデータに同名の
    // アビリティがあれば、プレイヤー / AI と同じ `App::execute_ability` で
    // 回数 / EN を消費して効果を適用する (C# `ExecuteAbility(.., is_event: true)`:
    // 使用条件は問わず、行動消費・経験値・使用イベントなし)。
    if let Some((caster, idx, target, name)) = resolve_data_ability(app, xargs) {
        app.set_script_var("直前使用アビリティ".to_string(), name);
        app.execute_ability(&caster, idx, &target);
        return Ok(pc + 1);
    }
    // 以下はユニットデータにアビリティが無いときの互換処理 (アビリティ名で
    // 効果を決め打ちする旧実装)。SRC.NET `AbilityEffect.cs` で頻出する
    // 基本効果のいくつかをハードコードで処理する:
    //   - `修理装置` / `修理`         → 対象 unit の HP 全回復
    //   - `補給装置` / `補給`         → 対象 unit の EN 全回復 + 弾薬補給は省略
    //   - `状態異常回復` / `状態回復` → 対象 unit の全 condition クリア
    //   - `自爆`                      → 発動 unit を撃破 (Destruction 発火)
    // それ以外のアビリティは `直前使用アビリティ` script_var 保存のみ。
    if xargs.len() < 2 {
        return Ok(pc + 1);
    }
    let unit_key = xargs[0].clone();
    let ability = xargs[1].clone();
    let target_key = xargs.get(2).cloned().unwrap_or_else(|| unit_key.clone());
    app.set_script_var("直前使用アビリティ".to_string(), ability.clone());
    match ability.as_str() {
        "修理装置" | "修理" => {
            recover_hp(app, &target_key, Some("全"));
        }
        "補給装置" | "補給" => {
            recover_en(app, &target_key, Some("全"));
        }
        "状態異常回復" | "状態回復" => {
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target_key))
            {
                u.conditions.clear();
            }
        }
        "自爆" => {
            // 自爆: 発動 unit を撃破。max_hp ぶんダメージを与えて
            // Destruction を発火させる。
            let max_hp = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &unit_key))
                .map(|u| app.database().effective_max_hp(u))
                .unwrap_or(0);
            if max_hp > 0 {
                apply_damage(app, &unit_key, max_hp);
            }
        }
        // 以下は condition として付与し、combat.rs / display 側で
        // 解釈する規約。`Condition()` 関数で参照可能。
        "バリア" | "バリア展開" | "シールド" => {
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target_key))
            {
                u.add_condition(crate::condition::Condition::new("バリア".to_string(), -1));
            }
        }
        "分身" | "ホログラム" => {
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target_key))
            {
                u.add_condition(crate::condition::Condition::new("分身".to_string(), -1));
            }
        }
        "集中" | "精神統一" => {
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target_key))
            {
                u.add_condition(crate::condition::Condition::new("集中".to_string(), -1));
            }
        }
        "ステルス" | "隠れ身" => {
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target_key))
            {
                u.add_condition(crate::condition::Condition::new("ステルス".to_string(), -1));
            }
        }
        "合体技" | "援護攻撃" => {
            // 合体技/援護攻撃発動: Partner 履歴に対象 unit を追加して
            // Partner / CountPartner 関数で参照可能にする。
            let count_key = "直前合体技パートナー数".to_string();
            let n: i32 = app.script_var(&count_key).parse().unwrap_or(0);
            let n = n + 1;
            app.set_script_var(format!("直前合体技パートナー[{n}]"), target_key.clone());
            app.set_script_var(count_key, n.to_string());
            app.set_script_var("直前合体技ユニット".to_string(), unit_key.clone());
        }
        "憑依" => {
            // 憑依: 発動 unit のメインパイロットを target unit に転送。
            // 発動 unit は pilot_name="" になり、target unit が
            // pilot_name = 発動側パイロット で乗り換え (簡略化)。
            let src_pilot = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &unit_key))
                .map(|u| u.pilot_name.clone());
            if let Some(pilot) = src_pilot {
                // ソース側を空にする
                if let Some(u) = app
                    .database_mut()
                    .unit_instances
                    .iter_mut()
                    .find(|u| matches_unit_handle(u, &unit_key))
                {
                    u.pilot_name = String::new();
                }
                // ターゲット側に乗せる
                if let Some(u) = app
                    .database_mut()
                    .unit_instances
                    .iter_mut()
                    .find(|u| matches_unit_handle(u, &target_key))
                {
                    u.pilot_name = pilot;
                }
            }
        }
        "精神感応" => {
            // 精神感応: 発動 unit から target unit へ SP の半分を転送。
            // SRC 仕様の簡略化: 発動 sp_consumed += half, target -= half。
            let src_sp = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &unit_key))
                .map(|u| u.sp_consumed);
            if let Some(consumed) = src_sp {
                // 発動側の現在 SP を半分転送 (consumed を増やす)
                let half = (100 - consumed).max(0) / 2;
                if half > 0 {
                    if let Some(u) = app
                        .database_mut()
                        .unit_instances
                        .iter_mut()
                        .find(|u| matches_unit_handle(u, &unit_key))
                    {
                        u.sp_consumed += half;
                    }
                    if let Some(u) = app
                        .database_mut()
                        .unit_instances
                        .iter_mut()
                        .find(|u| matches_unit_handle(u, &target_key))
                    {
                        u.sp_consumed = (u.sp_consumed - half).max(0);
                    }
                }
            }
        }
        "気力増加" | "気力上昇" => {
            // 対象 unit の士気を 10 上げる (clamp 0..=150)。
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target_key))
            {
                u.morale = (u.morale + 10).clamp(0, 150);
            }
        }
        "召喚" => {
            // `UseAbility 親 召喚 子ユニットデータ [x y]`:
            // 召喚親と同じ陣営の子ユニットを生成し、summoned_by に親 uid を
            // 記録する。座標省略時は親ユニットの隣 (右隣、範囲外なら親位置)。
            // 子ユニットデータ名は target_key (= xargs[2])。
            let summoned_data = if xargs.len() >= 3 {
                fn_arg_value(app, &xargs[2])
            } else {
                String::new()
            };
            // 親ユニットを解決。Place 配置ユニットは uid が空のことが
            // あるため、空なら一意 uid を割り当ててから関係付けする
            // (空 uid 同士の誤一致を防ぐ)。
            let parent_pos = if summoned_data.is_empty() {
                None
            } else {
                let fresh = app.next_unit_id();
                let p = app
                    .database_mut()
                    .unit_instances
                    .iter_mut()
                    .find(|u| matches_unit_handle(u, &unit_key))
                    .map(|u| {
                        if u.uid.is_empty() {
                            u.uid = fresh;
                        }
                        (u.uid.clone(), u.party, u.x, u.y)
                    });
                p
            };
            if let Some((parent_uid, party, px, py)) = parent_pos {
                let (sx, sy) = if xargs.len() >= 5 {
                    (
                        parse_u32(xargs, 3, line).unwrap_or(px),
                        parse_u32(xargs, 4, line).unwrap_or(py),
                    )
                } else {
                    (px, py)
                };
                let mut inst = UnitInstance::new(summoned_data, String::new(), party, sx, sy);
                inst.summoned_by = Some(parent_uid);
                populate_active_features(&mut inst, app);
                let uid = app.database_mut().register_unit(inst);
                app.set_script_var("対象ユニットＩＤ".to_string(), uid);
            }
        }
        _ => {} // unknown ability — 履歴保存のみ
    }
    Ok(pc + 1)
}

/// `MapAttack` / `MapAbility` の攻撃側を解決して `App::execute_map_attack` に渡す。
/// `unit_key` が無ければ最初の味方ユニットを攻撃元とみなす。
fn script_map_attack(
    app: &mut App,
    unit_key: Option<&str>,
    weapon_name: &str,
    cx: u32,
    cy: u32,
    is_event: bool,
) {
    let atk = app
        .database()
        .unit_instances
        .iter()
        .find(|u| match unit_key {
            Some(k) => matches_unit_handle(u, k),
            None => u.party == crate::Party::Player,
        })
        .map(|u| u.uid.clone());
    if let Some(uid) = atk {
        app.execute_map_attack(&uid, weapon_name, cx, cy, is_event);
    }
}
