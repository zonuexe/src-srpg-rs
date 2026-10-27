//! マップ攻撃 (マップ兵器) の実行。プレイヤー / AI / `.eve MapAttack` の共通経路。

use super::*;
use crate::event_runtime::{
    fire_after_attack_event_labels, fire_after_use_event_labels, fire_attack_event_labels,
    fire_damage_threshold_labels, fire_destruction_labels, fire_map_attack_destruction_labels,
    fire_use_event_labels, UnitEventId,
};

impl App {
    /// マップ攻撃の本処理。`atk_uid` のユニットが武器 `weapon_name` で `(cx, cy)` を
    /// 狙い、`combat::map_attack_area` で武器属性別の効果範囲を算出して、その中の
    /// 対立勢力ユニットにダメージを与える。反撃 / 経験値 / 資金加算は無効。
    /// プレイヤー / AI のマップ攻撃 (`is_event=false`) と `.eve MapAttack` の共通経路。
    pub(crate) fn execute_map_attack(
        &mut self,
        atk_uid: &str,
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
        let Some(mut atk_idx) = self.database().idx_by_uid(atk_uid) else {
            return;
        };

        let mut atk_unit_name = self.database().unit_instances[atk_idx]
            .unit_data_name
            .clone();
        let weapon = match self
            .database()
            .unit_by_name(&atk_unit_name)
            .and_then(|d| d.weapons.iter().find(|w| w.name == weapon_name))
            .cloned()
        {
            Some(w) => w,
            None => return,
        };

        // 目標地点 (原典 `Unit.MapAttack` の `SelectedX = tx` / `SelectedY = ty`)。
        self.selected_point = Some((cx, cy));
        let atk_pos = (
            self.database().unit_instances[atk_idx].x,
            self.database().unit_instances[atk_idx].y,
        );
        let area: std::collections::HashSet<(u32, u32)> =
            combat::map_attack_area(&weapon, atk_pos, (cx, cy))
                .into_iter()
                .collect();

        // 攻撃側と敵対する全陣営 (味方↔ＮＰＣ 同盟は対象外、中立は対象) を攻撃対象に。
        let atk_party = self.database().unit_instances[atk_idx].party;
        // 対象 idx を集めて damage と armor で個別評価。
        let mut targets: Vec<usize> = self
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
            let atk_uid = self.database().unit_instances[atk_idx].uid.clone();
            let target_uids: Vec<String> = targets
                .iter()
                .map(|&i| self.database().unit_instances[i].uid.clone())
                .collect();
            // 使用イベント (武器 1 回)。サポートアタックの武器では発火しない仕様だが、
            // マップ攻撃はサポートアタック経路を通らないので無条件で発火してよい。
            fire_use_event_labels(self, atk_idx, weapon_name);
            // 各対象への 攻撃イベント。発火で unit_instances の index がずれ得るため
            // 攻撃側/対象とも uid で都度引き直す (攻撃側が消えたら以降は中断)。
            for tuid in &target_uids {
                let Some(ai) = self.database().idx_by_uid(&atk_uid) else {
                    return;
                };
                if let Some(ti) = self.database().idx_by_uid(tuid) {
                    fire_attack_event_labels(self, ai, ti);
                }
            }
            // イベントで盤面が変化し得るので攻撃側を引き直し、対象スナップショットを
            // 現存するものだけへ絞り込む (退避/撃破された対象は除外)。
            let Some(ai) = self.database().idx_by_uid(&atk_uid) else {
                return;
            };
            atk_idx = ai;
            atk_unit_name = self.database().unit_instances[atk_idx]
                .unit_data_name
                .clone();
            targets = target_uids
                .iter()
                .filter_map(|uid| self.database().idx_by_uid(uid))
                .collect();
            if targets.is_empty() {
                return;
            }
        }

        // 攻撃側も実効値込みデータを使用 (育成 / 強化パーツ / 状態異常)。
        let Some((atk_pilot, atk_unit_data)) = self.database().effective_combat_data(atk_idx)
        else {
            return;
        };
        let atk_statuses: Vec<String> = self.database().unit_instances[atk_idx]
            .conditions
            .iter()
            .map(|c| c.name.clone())
            .collect();

        // マップ兵器の EN・残弾消費 (撃破による index 失効前に消費)。
        if let Some(wi) = self
            .database()
            .unit_by_name(&atk_unit_name)
            .and_then(|d| d.weapons.iter().position(|w| w.name == weapon_name))
        {
            self.consume_weapon_resources(atk_idx, wi);
        }

        // is_event=false の後段イベント (損傷率/使用後/攻撃後) 用に攻撃側 ID を退避。
        let atk_ev = UnitEventId::from_unit_instance(&self.database().unit_instances[atk_idx]);
        // 生存対象のスナップショット (損傷率 と 攻撃後 用)。
        // (UnitEventId, pilot, unit, party, old_dmg, new_dmg, max_hp)
        let mut survivors: Vec<(UnitEventId, String, String, crate::Party, i64, i64, i64)> =
            Vec::new();

        // 対象を後ろから削除するため位置を巻き戻し
        let mut kills = 0usize;
        for &def_idx in targets.iter().rev() {
            let def_inst = self.database().unit_instances[def_idx].clone();
            let Some((def_pilot, mut def_unit)) = self.database().effective_combat_data(def_idx)
            else {
                continue;
            };
            // 防御特性による装甲調整 (弱点→半減 / 吸収→無視)。通常戦闘と一貫させ、
            // マップ攻撃でも VB5 `Unit.cls:6949-6951` の防御特性を反映する。
            self.apply_defense_armor_mod(def_idx, &weapon.class, &mut def_unit);
            // マップ未設定時は terrain_id=0 (平地) として進める。
            let terrain_id = self
                .database()
                .map
                .as_ref()
                .map(|m| m.cell(def_inst.x, def_inst.y).terrain_id)
                .unwrap_or(0);
            let def_hit_mod = self.database().terrain_hit_mod(terrain_id);
            let def_damage_mod = self.database().terrain_damage_mod(terrain_id);
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
                self.database().unit_instances[atk_idx].morale,
                def_inst.morale,
                &atk_statuses,
                &def_statuses,
            );
            // 防御特性 (耐性÷2 / 無効化0 / 吸収=回復) を最終ダメージへ適用する
            // (弱点の装甲半減は上の apply_defense_armor_mod で予測前に適用済み)。
            let dmg = self.defense_attribute_damage(def_idx, &weapon.class, preview.damage);
            // マップ攻撃は必中扱いに近い (ダメージのみ)。吸収 (dmg<0) は回復だが
            // damage は 0 未満にしない (最大HP超過回復の抑止)。
            let old_dmg = self.database().unit_instances[def_idx].damage;
            let new_dmg = (old_dmg + dmg).max(0);
            self.database_mut().unit_instances[def_idx].damage = new_dmg;
            let remaining = def_unit.hp - new_dmg;
            if remaining <= 0 {
                // 精神コマンド「復活」: HP0 でも HP 全快で立ち上がる (1 回で消費)。通常戦闘と
                // 同様にマップ兵器の撃破でも復活を尊重する (撃破・破壊・全滅を発火しない)。
                if self.revive_if_possible(def_idx) {
                    self.push_message(format!("{} は【復活】で立ち上がった！", def_unit.name));
                } else {
                    let (vp, vu) = (def_pilot.name.clone(), def_unit.name.clone());
                    let vparty = self.database().unit_instances[def_idx].party;
                    self.database_mut().remove_unit_at(def_idx);
                    if is_event {
                        // スクリプトの `MapAttack` コマンド: 原典は「イベント上の戦闘」として
                        // 破壊系を発火しないが、ポートは進行保証のため従来どおり 破壊 を発火する
                        // (これが無いとスクリプトのマップ兵器で撃破してもシナリオが進まない)。
                        fire_destruction_labels(self, &vp, &vu, vparty);
                    } else {
                        // プレイヤー/AI 発のマップ攻撃: 原典 (VB5 `Unit.cls:17214`) 準拠で
                        // `破壊` ではなく `マップ攻撃破壊 <対象>` を発火する。`全滅`/勝敗は共通。
                        fire_map_attack_destruction_labels(self, &vp, &vu, vparty);
                    }
                    kills += 1;
                }
            } else if !is_event {
                // 生存対象は後段の 損傷率 / 攻撃後 用に記録 (ダメージ適用後の状態で)。
                let def_ev =
                    UnitEventId::from_unit_instance(&self.database().unit_instances[def_idx]);
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
        self.check_victory();

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
                fire_damage_threshold_labels(
                    self, pilot, unit, *party, *old_dmg, *new_dmg, *max_hp,
                );
            }
            // 使用後イベント (攻撃側が生存している場合のみ・内部で生存判定)。
            fire_after_use_event_labels(self, &atk_ev, weapon_name);
            // 攻撃後イベント (生存対象ごと・両者生存時のみ)。
            for (def_ev, ..) in &survivors {
                fire_after_attack_event_labels(self, &atk_ev, def_ev);
            }
        }
        if kills > 0 {
            self.push_message(format!(
                "{} のマップ攻撃 [{}] で {} ユニット撃破",
                atk_pilot.nickname, weapon_name, kills
            ));
        } else {
            self.push_message(format!(
                "{} のマップ攻撃 [{}] が {} ユニットに着弾",
                atk_pilot.nickname,
                weapon_name,
                targets.len()
            ));
        }
    }
}

/// マップ兵器の目標地点の選び方 (原典 `MapAttackCommand` / `Map.AreaIn*`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MapAim {
    /// `Ｍ全`: 目標地点を選ばず、攻撃側を中心に撃つ。
    Around,
    /// `Ｍ直` / `Ｍ拡` / `Ｍ扇`: 攻撃側から十字方向の射程内のマスで方向を選ぶ。
    Direction,
    /// `Ｍ移`: 射程内の空きマス。
    EmptyTile,
    /// `Ｍ投` / `Ｍ線` ほか: 射程内の任意のマス。
    Point,
}

fn map_aim(weapon: &crate::data::unit::WeaponData) -> MapAim {
    let c = weapon.class.as_str();
    if c.contains("Ｍ全") {
        MapAim::Around
    } else if c.contains("Ｍ直") || c.contains("Ｍ拡") || c.contains("Ｍ扇") {
        MapAim::Direction
    } else if c.contains("Ｍ移") {
        MapAim::EmptyTile
    } else {
        MapAim::Point
    }
}

/// プレイヤーのマップ攻撃 (ユニットコマンド「マップ攻撃」)。
impl App {
    /// `uid` のマップ兵器と使用可否 (EN・弾・気力・必要技能・チャージ・移動後の制限)。
    fn player_map_weapons(&self, uid: &str, post_move: bool) -> Vec<(String, bool)> {
        let Some(idx) = self.database.idx_by_uid(uid) else {
            return Vec::new();
        };
        let u = &self.database.unit_instances[idx];
        let Some(def) = self.database.unit_by_name(&u.unit_data_name) else {
            return Vec::new();
        };
        def.weapons
            .iter()
            .filter(|w| combat::is_map_weapon(w))
            .map(|w| {
                let usable = (u.charged || !combat::is_charge_weapon(w))
                    && Self::weapon_usable_post_move(w, post_move, false)
                    && self.weapon_firable(idx, w)
                    && self.weapon_skill_ok(idx, w);
                (w.name.clone(), usable)
            })
            .collect()
    }

    /// `uid` がマップ兵器を持つか (ユニットコマンド「マップ攻撃」の表示条件)。使用可否は
    /// アビリティと同じく一覧サブメニュー側で `×` 表示・判定する。
    pub(super) fn has_map_weapon(&self, uid: &str) -> bool {
        !self.player_map_weapons(uid, false).is_empty()
    }

    /// マップ兵器の一覧サブメニューを開く。使えない兵器は `×` 付きで並べる。
    pub(super) fn open_map_weapon_menu(
        &mut self,
        uid: &str,
        snapshot: Option<crate::command_menu::MoveSnapshot>,
    ) {
        let entries = self.player_map_weapons(uid, snapshot.is_some());
        let options = entries
            .iter()
            .map(|(name, usable)| format!("{}{name}", if *usable { "" } else { "×" }))
            .collect();
        let nick = self
            .database
            .unit_by_uid(uid)
            .map(|u| u.pilot_name.clone())
            .unwrap_or_default();
        self.pending_map_weapon = Some(PendingMapWeapon {
            uid: uid.to_string(),
            entries,
            snapshot,
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: format!("{nick} マップ攻撃"),
            options,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            non_cancellable: false,
        });
    }

    /// マップ兵器一覧サブメニューの選択を解決する。`choice` は 1-based (0 / 範囲外は
    /// キャンセルでユニットメニューへ戻る)。`Ｍ全` は即発射、それ以外は目標地点の選択へ。
    pub(super) fn resolve_map_weapon(&mut self, choice: u32) -> bool {
        let Some(pm) = self.pending_map_weapon.take() else {
            return false;
        };
        self.pending_dialog = None;
        let i = choice as usize;
        if i == 0 || i > pm.entries.len() {
            self.return_to_unit_menu(&pm.uid, pm.snapshot);
            return true;
        }
        let (weapon, usable) = pm.entries[i - 1].clone();
        if !usable {
            self.push_message("そのマップ兵器は使用できません (EN / 弾数 / 気力)".to_string());
            self.return_to_unit_menu(&pm.uid, pm.snapshot);
            return true;
        }
        let aim = self.map_weapon_of(&pm.uid, &weapon).map(|w| map_aim(&w));
        if aim == Some(MapAim::Around) {
            let pos = self.database.unit_by_uid(&pm.uid).map(|u| (u.x, u.y));
            if let Some(pos) = pos {
                self.fire_player_map_attack(&pm.uid, &weapon, pos);
            }
            return true;
        }
        self.command_menu = None;
        self.action_mode = crate::command_menu::ActionMode::MapAttackSelect {
            uid: pm.uid,
            weapon: weapon.clone(),
            point: None,
            snapshot: pm.snapshot,
        };
        self.push_message(format!(
            "{weapon}: 目標地点を選択 (もう一度選ぶと発射・右クリックでキャンセル)"
        ));
        true
    }

    /// サブメニュー・目標選択のキャンセルでユニットメニューへ戻る。移動後からなら
    /// 移動後メニュー (移動はまだ確定のまま)。
    pub(super) fn return_to_unit_menu(
        &mut self,
        uid: &str,
        snapshot: Option<crate::command_menu::MoveSnapshot>,
    ) {
        use crate::command_menu::ActionMode;
        self.action_mode = match snapshot {
            Some(snapshot) => ActionMode::PostMoveMenu {
                uid: uid.to_string(),
                snapshot,
            },
            None => ActionMode::Browse,
        };
        self.reopen_unit_menu_for(uid);
    }

    fn map_weapon_of(&self, uid: &str, weapon: &str) -> Option<crate::data::unit::WeaponData> {
        let u = self.database.unit_by_uid(uid)?;
        self.database
            .unit_by_name(&u.unit_data_name)?
            .weapons
            .iter()
            .find(|w| w.name == weapon && combat::is_map_weapon(w))
            .cloned()
    }

    /// `target` をマップ兵器 `weapon` の目標地点に選べるか (射程・方向・空きマス)。
    fn map_attack_target_ok(&self, uid: &str, weapon: &str, target: (u32, u32)) -> bool {
        let (Some(u), Some(w)) = (
            self.database.unit_by_uid(uid),
            self.map_weapon_of(uid, weapon),
        ) else {
            return false;
        };
        if let Some(m) = self.database.map.as_ref() {
            if target.0 >= m.width || target.1 >= m.height {
                return false;
            }
        }
        let pos = (u.x, u.y);
        let dist = combat::manhattan(pos, target);
        match map_aim(&w) {
            MapAim::Around => target == pos,
            MapAim::Direction => {
                (target.0 == pos.0 || target.1 == pos.1) && combat::weapon_in_range(&w, dist)
            }
            MapAim::EmptyTile => {
                combat::weapon_in_range(&w, dist)
                    && self.database.uid_at(target.0, target.1).is_none()
            }
            MapAim::Point => combat::weapon_in_range(&w, dist),
        }
    }

    /// 目標地点選択中のクリック。選べないマスは無視し、初回は目標地点を仮決めして効果
    /// 範囲を表示する。仮決めした地点をもう一度選ぶと発射する。
    pub(super) fn click_map_attack_target(&mut self, target: (u32, u32)) {
        use crate::command_menu::ActionMode;
        let ActionMode::MapAttackSelect {
            uid, weapon, point, ..
        } = self.action_mode.clone()
        else {
            return;
        };
        if !self.map_attack_target_ok(&uid, &weapon, target) {
            return;
        }
        if point == Some(target) {
            self.fire_player_map_attack(&uid, &weapon, target);
        } else if let ActionMode::MapAttackSelect { point, .. } = &mut self.action_mode {
            *point = Some(target);
        }
    }

    /// プレイヤーのマップ攻撃を発射して行動を終える。
    fn fire_player_map_attack(&mut self, uid: &str, weapon: &str, target: (u32, u32)) {
        self.command_menu = None;
        self.action_mode = crate::command_menu::ActionMode::Browse;
        // プレイヤーのマップ攻撃コマンド = 通常戦闘扱い (is_event=false)。
        self.execute_map_attack(uid, weapon, target.0, target.1, false);
        self.finish_player_attack(uid);
    }

    /// 目標地点選択中の表示用マス (目標地点に選べるマスと効果範囲)。目標地点選択中で
    /// なければ `None`。src-web はこれを塗るだけにする。
    pub fn map_attack_select_tiles(&self) -> Option<crate::scene::map_view::MapAttackTiles> {
        let crate::command_menu::ActionMode::MapAttackSelect {
            uid, weapon, point, ..
        } = &self.action_mode
        else {
            return None;
        };
        let u = self.database.unit_by_uid(uid)?;
        let w = self.map_weapon_of(uid, weapon)?;
        let m = self.database.map.as_ref()?;
        let pos = (u.x, u.y);
        let r = u32::try_from(w.max_range.max(0)).unwrap_or(0);
        let mut targets = Vec::new();
        for y in pos.1.saturating_sub(r)..=(pos.1 + r).min(m.height.saturating_sub(1)) {
            for x in pos.0.saturating_sub(r)..=(pos.0 + r).min(m.width.saturating_sub(1)) {
                if self.map_attack_target_ok(uid, weapon, (x, y)) {
                    targets.push((x, y));
                }
            }
        }
        let aim = point.or(self.map_cursor.filter(|c| targets.contains(c)));
        let area = aim.map_or_else(Vec::new, |t| combat::map_attack_area(&w, pos, t));
        Some(crate::scene::map_view::MapAttackTiles { targets, area })
    }
}
