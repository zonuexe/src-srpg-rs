//! スクリプトコマンドからのユニットへの直接作用 (ダメージ・回復・マップ攻撃・SP コスト)。

use super::*;

/// `RecoverHP unit [spec]` の本処理。
/// `spec` 解釈:
/// - `"Full"` / `"全"` / `None` → 完全回復 (damage = 0)
/// - `"50%"` 等 → 最大 HP × % を回復
/// - `"500"` 等 → 直接 500 ダメージ減
pub(super) fn recover_hp(app: &mut App, key: &str, spec: Option<&str>) {
    let idx = match app
        .database()
        .unit_instances
        .iter()
        .position(|u| matches_unit_handle(u, key))
    {
        Some(i) => i,
        None => return,
    };
    let max_hp = app
        .database()
        .effective_max_hp(&app.database().unit_instances[idx]);
    let cur_dmg = app.database().unit_instances[idx].damage;
    // `rate` は常にパーセント (SRC 準拠: "HP を rate% 回復")。
    // `%` サフィックスは後方互換として受け入れるが、付いていない数値も百分率扱い。
    // RecoverHP によって HP が 0 以下になることはない (最低値 = 1 → damage = max_hp - 1)。
    let new_dmg = match spec {
        None | Some("") => 0,
        Some(s) if s.eq_ignore_ascii_case("full") || s == "全" => 0,
        Some(s) => {
            let pct: f64 = s.trim_end_matches('%').parse().unwrap_or(0.0);
            let recover = ((max_hp as f64) * pct / 100.0).round() as i64;
            // 回復後ダメージ: 下限 0 (HP = max_hp)、上限 max_hp - 1 (HP = 1)
            (cur_dmg - recover).max(0).min(max_hp - 1)
        }
    };
    app.database_mut().unit_instances[idx].damage = new_dmg;
}

/// `RecoverEN unit [spec]` の本処理。
pub(super) fn recover_en(app: &mut App, key: &str, spec: Option<&str>) {
    let idx = match app
        .database()
        .unit_instances
        .iter()
        .position(|u| matches_unit_handle(u, key))
    {
        Some(i) => i,
        None => return,
    };
    let max_en = app
        .database()
        .effective_max_en(&app.database().unit_instances[idx]);
    let cur_consumed = app.database().unit_instances[idx].en_consumed;
    // `rate` は常にパーセント (RecoverHP と同じ仕様)。EN の下限は 0。
    let new_consumed = match spec {
        None | Some("") => 0,
        Some(s) if s.eq_ignore_ascii_case("full") || s == "全" => 0,
        Some(s) => {
            let pct: f64 = s.trim_end_matches('%').parse().unwrap_or(0.0);
            let recover = ((max_en as f64) * pct / 100.0).round() as i32;
            (cur_consumed - recover).max(0)
        }
    };
    app.database_mut().unit_instances[idx].en_consumed = new_consumed;
}

/// 直接ダメージ / 回復を反映。HP <= 0 ならインスタンスを削除。
pub(super) fn apply_damage(app: &mut App, key: &str, amount: i64) {
    let Some(idx) = app
        .database()
        .unit_instances
        .iter()
        .position(|u| matches_unit_handle(u, key))
    else {
        return;
    };
    let max_hp = app
        .database()
        .effective_max_hp(&app.database().unit_instances[idx]);
    let old_dmg = app.database().unit_instances[idx].damage;
    let u = &mut app.database_mut().unit_instances[idx];
    u.damage = (u.damage + amount).max(0);
    let new_dmg = u.damage;
    if new_dmg >= max_hp && max_hp > 0 {
        // 撃破前にラベル発火用の名前/陣営を退避してから unit_instance を除去。
        let pilot_name = u.pilot_name.clone();
        let unit_data_name = u.unit_data_name.clone();
        let party = u.party;
        app.database_mut().remove_unit_at(idx);
        fire_destruction_labels(app, &pilot_name, &unit_data_name, party);
    } else if new_dmg > old_dmg {
        // HP 残存 + ダメージ増加 → 損傷率閾値跨ぎを検査。
        let u = &app.database().unit_instances[idx];
        let pilot_name = u.pilot_name.clone();
        let unit_data_name = u.unit_data_name.clone();
        let party = u.party;
        fire_damage_threshold_labels(
            app,
            &pilot_name,
            &unit_data_name,
            party,
            old_dmg,
            new_dmg,
            max_hp,
        );
    }
}

/// マップ攻撃の効果範囲を表す `(x, y)` 集合を計算する。
/// 武器の `class` 文字列に含まれる SRC の属性記号を解釈する:
///
/// - `Ｍ全`             : 全方位 (攻撃者を中心としたマンハッタン菱形 ≤ max_range)
/// - `Ｍ投L<n>` / `Ｍ投` : 指定地点を中心とした菱形 (半径 = L 値 or max_range)
/// - `Ｍ直`             : 攻撃者から指定方向へ max_range タイルの直線
/// - `Ｍ拡`             : 攻撃者から指定方向へ max_range タイル幅 3 の直線
/// - `Ｍ移` / `Ｍ線`     : 攻撃者と指定地点を結ぶ直線 (近似で軸線優先)
/// - `Ｍ扇L<n>`         : 簡略実装で `Ｍ拡` 同等 (将来詳細化)
/// - 不明 / `Ｍ` 接頭辞なし : 指定地点中心の菱形 (半径 = max_range) — 旧挙動互換
pub(crate) fn map_attack_area(
    weapon: &crate::data::unit::WeaponData,
    src: (u32, u32),
    target: (u32, u32),
) -> Vec<(u32, u32)> {
    let cls = weapon.class.as_str();
    let range = weapon.max_range.max(0);
    let lvl_after = |key: &str| -> Option<i32> {
        cls.find(key).map(|idx| {
            let tail = &cls[idx + key.len()..];
            // 'L' があれば数字を読む
            if let Some(rest) = tail.strip_prefix('L') {
                let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                digits.parse::<i32>().unwrap_or(range)
            } else {
                range
            }
        })
    };

    let mut out: Vec<(u32, u32)> = Vec::new();
    if cls.contains("Ｍ全") {
        // 攻撃者中心の菱形 (max_range)
        diamond_into(&mut out, src.0 as i32, src.1 as i32, range);
    } else if let Some(lvl) = lvl_after("Ｍ投") {
        // ターゲット中心の菱形 (L<lvl> あれば lvl、無ければ max_range)
        diamond_into(&mut out, target.0 as i32, target.1 as i32, lvl);
    } else if cls.contains("Ｍ直") {
        line_into(&mut out, src, target, range, 0);
    } else if cls.contains("Ｍ拡") || cls.contains("Ｍ扇") {
        // 簡略実装: 3 マス幅の直線
        line_into(&mut out, src, target, range, 1);
    } else if cls.contains("Ｍ移") || cls.contains("Ｍ線") {
        // 攻撃者からターゲットまでの直線 (距離 ≤ max_range)
        line_into(&mut out, src, target, range, 0);
    } else {
        // フォールバック: 旧挙動 (target 中心の菱形, radius=max_range)
        diamond_into(&mut out, target.0 as i32, target.1 as i32, range);
    }
    out
}

/// (cx, cy) を中心としたマンハッタン半径 `radius` の菱形領域。
pub(super) fn diamond_into(out: &mut Vec<(u32, u32)>, cx: i32, cy: i32, radius: i32) {
    if radius < 0 {
        return;
    }
    for dy in -radius..=radius {
        let span = radius - dy.abs();
        for dx in -span..=span {
            let x = cx + dx;
            let y = cy + dy;
            if x >= 0 && y >= 0 {
                out.push((x as u32, y as u32));
            }
        }
    }
}

/// 攻撃者 `src` から `target` へ向かう直線（軸線優先）。
/// `width_each` = 1 なら線の両側 1 マス（合計 3 マス幅）。
pub(super) fn line_into(
    out: &mut Vec<(u32, u32)>,
    src: (u32, u32),
    target: (u32, u32),
    range: i32,
    width_each: i32,
) {
    let dx = target.0 as i32 - src.0 as i32;
    let dy = target.1 as i32 - src.1 as i32;
    let (sx, sy) = if dx.abs() >= dy.abs() {
        (dx.signum(), 0)
    } else {
        (0, dy.signum())
    };
    if sx == 0 && sy == 0 {
        return;
    }
    for step in 1..=range {
        let x = src.0 as i32 + sx * step;
        let y = src.1 as i32 + sy * step;
        if x < 0 || y < 0 {
            continue;
        }
        for w in -width_each..=width_each {
            // 線方向に垂直なオフセット
            let (ox, oy) = if sx != 0 { (0, w) } else { (w, 0) };
            let xx = x + ox;
            let yy = y + oy;
            if xx >= 0 && yy >= 0 {
                out.push((xx as u32, yy as u32));
            }
        }
    }
}

/// `MapAttack` の本処理。`unit_key` が無ければ第 1 引数として渡された
/// ユニット種別の最初のインスタンスを攻撃元とみなす。weapon 名で武器を
/// 引き、`map_attack_area` で武器属性別の効果範囲を算出して、その中の
/// 対立勢力ユニットにダメージを与える。反撃 / 経験値 / 資金加算は無効。
pub(crate) fn map_attack(
    app: &mut App,
    unit_key: Option<&str>,
    weapon_name: &str,
    cx: u32,
    cy: u32,
    is_event: bool,
) {
    // `is_event`: SRC `Unit.MapAttack(w, tx, ty, is_event)` (Unit.cs:27012) の第4引数。
    // - false = プレイヤー/AI の「マップ攻撃」コマンド経由 → ダメージ適用前に
    //   使用イベント (武器 1 回) と 各対象への 攻撃イベント を発火する。
    // - true  = スクリプトの `MapAttack` コマンド経由 (既定) → これらのイベントは
    //   発火しない (末尾に `通常戦闘` を付けた場合のみ false になる)。
    // 攻撃側ユニットの解決: unit_key 指定時はそれを引き、無ければ任意の Player ユニットへ。
    let atk_idx = if let Some(k) = unit_key {
        app.database()
            .unit_instances
            .iter()
            .position(|u| matches_unit_handle(u, k))
    } else {
        app.database()
            .unit_instances
            .iter()
            .position(|u| u.party == crate::Party::Player)
    };
    let Some(mut atk_idx) = atk_idx else { return };

    let mut atk_unit_name = app.database().unit_instances[atk_idx]
        .unit_data_name
        .clone();
    let weapon = match app
        .database()
        .unit_by_name(&atk_unit_name)
        .and_then(|d| d.weapons.iter().find(|w| w.name == weapon_name))
        .cloned()
    {
        Some(w) => w,
        None => return,
    };

    let atk_pos = (
        app.database().unit_instances[atk_idx].x,
        app.database().unit_instances[atk_idx].y,
    );
    let area: std::collections::HashSet<(u32, u32)> = map_attack_area(&weapon, atk_pos, (cx, cy))
        .into_iter()
        .collect();

    // 攻撃側と敵対する全陣営 (味方↔ＮＰＣ 同盟は対象外、中立は対象) を攻撃対象に。
    let atk_party = app.database().unit_instances[atk_idx].party;
    // 対象 idx を集めて damage と armor で個別評価。
    let mut targets: Vec<usize> = app
        .database()
        .unit_instances
        .iter()
        .enumerate()
        .filter(|(_, u)| u.party.is_hostile_to(atk_party))
        .filter(|(_, u)| area.contains(&(u.x, u.y)))
        .map(|(i, _)| i)
        .collect();
    if targets.is_empty() {
        return;
    }

    // SRC `Unit.MapAttack` (Unit.cs:27316): プレイヤー/AI 発のマップ攻撃 (is_event=false)
    // では、ダメージ適用前に 使用イベント (武器 1 回) → 各対象への 攻撃イベント を発火する。
    // 反撃/経験値同様、イベントは攻撃の「前」に起こるため、ここでまとめて処理する。
    if !is_event {
        let atk_uid = app.database().unit_instances[atk_idx].uid.clone();
        let target_uids: Vec<String> = targets
            .iter()
            .map(|&i| app.database().unit_instances[i].uid.clone())
            .collect();
        // 使用イベント (武器 1 回)。サポートアタックの武器では発火しない仕様だが、
        // マップ攻撃はサポートアタック経路を通らないので無条件で発火してよい。
        fire_use_event_labels(app, atk_idx, weapon_name);
        // 各対象への 攻撃イベント。発火で unit_instances の index がずれ得るため
        // 攻撃側/対象とも uid で都度引き直す (攻撃側が消えたら以降は中断)。
        for tuid in &target_uids {
            let Some(ai) = app.database().idx_by_uid(&atk_uid) else {
                return;
            };
            if let Some(ti) = app.database().idx_by_uid(tuid) {
                fire_attack_event_labels(app, ai, ti);
            }
        }
        // イベントで盤面が変化し得るので攻撃側を引き直し、対象スナップショットを
        // 現存するものだけへ絞り込む (退避/撃破された対象は除外)。
        let Some(ai) = app.database().idx_by_uid(&atk_uid) else {
            return;
        };
        atk_idx = ai;
        atk_unit_name = app.database().unit_instances[atk_idx]
            .unit_data_name
            .clone();
        targets = target_uids
            .iter()
            .filter_map(|uid| app.database().idx_by_uid(uid))
            .collect();
        if targets.is_empty() {
            return;
        }
    }

    // 攻撃側も実効値込みデータを使用 (育成 / 強化パーツ / 状態異常)。
    let Some((atk_pilot, atk_unit_data)) = app.database().effective_combat_data(atk_idx) else {
        return;
    };
    let atk_statuses: Vec<String> = app.database().unit_instances[atk_idx]
        .conditions
        .iter()
        .map(|c| c.name.clone())
        .collect();

    // マップ兵器の EN・残弾消費 (撃破による index 失効前に消費)。
    if let Some(wi) = app
        .database()
        .unit_by_name(&atk_unit_name)
        .and_then(|d| d.weapons.iter().position(|w| w.name == weapon_name))
    {
        app.consume_weapon_resources(atk_idx, wi);
    }

    // is_event=false の後段イベント (損傷率/使用後/攻撃後) 用に攻撃側 ID を退避。
    let atk_ev = UnitEventId::from_unit_instance(&app.database().unit_instances[atk_idx]);
    // 生存対象のスナップショット (損傷率 と 攻撃後 用)。
    // (UnitEventId, pilot, unit, party, old_dmg, new_dmg, max_hp)
    let mut survivors: Vec<(UnitEventId, String, String, crate::Party, i64, i64, i64)> = Vec::new();

    // 対象を後ろから削除するため位置を巻き戻し
    let mut kills = 0usize;
    for &def_idx in targets.iter().rev() {
        let def_inst = app.database().unit_instances[def_idx].clone();
        let Some((def_pilot, mut def_unit)) = app.database().effective_combat_data(def_idx) else {
            continue;
        };
        // 防御特性による装甲調整 (弱点→半減 / 吸収→無視)。通常戦闘と一貫させ、
        // マップ攻撃でも VB5 `Unit.cls:6949-6951` の防御特性を反映する。
        app.apply_defense_armor_mod(def_idx, &weapon.class, &mut def_unit);
        // マップ未設定時は terrain_id=0 (平地) として進める。
        let terrain_id = app
            .database()
            .map
            .as_ref()
            .map(|m| m.cell(def_inst.x, def_inst.y).terrain_id)
            .unwrap_or(0);
        let def_hit_mod = app.database().terrain_hit_mod(terrain_id);
        let def_damage_mod = app.database().terrain_damage_mod(terrain_id);
        let def_statuses: Vec<String> =
            def_inst.conditions.iter().map(|c| c.name.clone()).collect();
        let preview = crate::combat::predict_with_status(
            &atk_pilot,
            &atk_unit_data,
            &weapon,
            &def_pilot,
            &def_unit,
            def_hit_mod,
            def_damage_mod,
            app.database().unit_instances[atk_idx].morale,
            def_inst.morale,
            &atk_statuses,
            &def_statuses,
        );
        // 防御特性 (耐性÷2 / 無効化0 / 吸収=回復) を最終ダメージへ適用する
        // (弱点の装甲半減は上の apply_defense_armor_mod で予測前に適用済み)。
        let dmg = app.defense_attribute_damage(def_idx, &weapon.class, preview.damage);
        // マップ攻撃は必中扱いに近い (ダメージのみ)。吸収 (dmg<0) は回復だが
        // damage は 0 未満にしない (最大HP超過回復の抑止)。
        let old_dmg = app.database().unit_instances[def_idx].damage;
        let new_dmg = (old_dmg + dmg).max(0);
        app.database_mut().unit_instances[def_idx].damage = new_dmg;
        let remaining = def_unit.hp - new_dmg;
        if remaining <= 0 {
            // 精神コマンド「復活」: HP0 でも HP 全快で立ち上がる (1 回で消費)。通常戦闘と
            // 同様にマップ兵器の撃破でも復活を尊重する (撃破・破壊・全滅を発火しない)。
            if app.revive_if_possible(def_idx) {
                app.push_message(format!("{} は【復活】で立ち上がった！", def_unit.name));
            } else {
                let (vp, vu) = (def_pilot.name.clone(), def_unit.name.clone());
                let vparty = app.database().unit_instances[def_idx].party;
                app.database_mut().remove_unit_at(def_idx);
                if is_event {
                    // スクリプトの `MapAttack` コマンド: 原典は「イベント上の戦闘」として
                    // 破壊系を発火しないが、ポートは進行保証のため従来どおり 破壊 を発火する
                    // (これが無いとスクリプトのマップ兵器で撃破してもシナリオが進まない)。
                    fire_destruction_labels(app, &vp, &vu, vparty);
                } else {
                    // プレイヤー/AI 発のマップ攻撃: 原典 (VB5 `Unit.cls:17214`) 準拠で
                    // `破壊` ではなく `マップ攻撃破壊 <対象>` を発火する。`全滅`/勝敗は共通。
                    fire_map_attack_destruction_labels(app, &vp, &vu, vparty);
                }
                kills += 1;
            }
        } else if !is_event {
            // 生存対象は後段の 損傷率 / 攻撃後 用に記録 (ダメージ適用後の状態で)。
            let def_ev = UnitEventId::from_unit_instance(&app.database().unit_instances[def_idx]);
            survivors.push((
                def_ev,
                def_pilot.name.clone(),
                def_unit.name.clone(),
                def_inst.party,
                old_dmg,
                new_dmg,
                def_unit.hp,
            ));
        }
    }
    // 勝敗判定 (マップ兵器でラスト1機撃破時の勝利確定)。
    app.check_victory();

    // SRC `Unit.MapAttack` の後段イベント (is_event=false, Unit.cs:28560-28688)。
    // ダメージ確定後に 損傷率 (生存対象) → 使用後 (攻撃側) → 攻撃後 (生存対象ごと) を発火する。
    // 破壊/全滅 は撃破ループ内の fire_destruction_labels で発火済み。マップ攻撃破壊 (Help 未記載
    // の内部イベント) は仕様不明のため未実装。スクリプトの素の MapAttack (is_event=true) では
    // これらは発火しない (通常攻撃と同じく "イベント上の戦闘" は後段イベント非発火)。
    if !is_event {
        // 対象順 (昇順) に揃える (ループは降順で集めている)。
        survivors.reverse();
        // 損傷率イベント (HP が減った生存対象)。
        for (_, pilot, unit, party, old_dmg, new_dmg, max_hp) in &survivors {
            fire_damage_threshold_labels(app, pilot, unit, *party, *old_dmg, *new_dmg, *max_hp);
        }
        // 使用後イベント (攻撃側が生存している場合のみ・内部で生存判定)。
        fire_after_use_event_labels(app, &atk_ev, weapon_name);
        // 攻撃後イベント (生存対象ごと・両者生存時のみ)。
        for (def_ev, ..) in &survivors {
            fire_after_attack_event_labels(app, &atk_ev, def_ev);
        }
    }
    if kills > 0 {
        app.push_message(format!(
            "{} のマップ攻撃 [{}] で {} ユニット撃破",
            atk_pilot.nickname, weapon_name, kills
        ));
    } else {
        app.push_message(format!(
            "{} のマップ攻撃 [{}] が {} ユニットに着弾",
            atk_pilot.nickname,
            weapon_name,
            targets.len()
        ));
    }
}
