//! プレイヤー/AI 共通の戦闘の進行 (本線攻撃・援護攻撃・援護防御・先制反撃・反撃)。

use super::*;

/// `resolve_strike` の入力。攻撃側 / 防御側の実効データ (育成・強化パーツ・状態込み) と
/// 防御側の地形補正。`def_unit` は防御特性による装甲調整済みのもの。
#[derive(Clone, Copy)]
struct StrikeSetup<'a> {
    atk_pilot: &'a crate::data::pilot::PilotData,
    atk_unit: &'a crate::data::unit::UnitData,
    weapon: &'a crate::data::unit::WeaponData,
    def_pilot: &'a crate::data::pilot::PilotData,
    def_unit: &'a crate::data::unit::UnitData,
    def_inst: &'a crate::UnitInstance,
    def_hit_mod: i32,
    def_damage_mod: i32,
}

/// 攻撃の種類 (原典 `Unit.Attack` の `attack_mode`)。本線攻撃・反撃・援護攻撃はどれも
/// 同じ `Attack` を通るので、命中・クリティカル・即死・切り払いの判定は共通。
#[derive(Clone, Copy, PartialEq, Eq)]
enum StrikeKind {
    /// 本線攻撃 (`""`)・`.eve Attack`。
    Main,
    /// 反撃・先制反撃 (`捨て身` は効かない)。
    Counter,
    /// 援護攻撃 (`"援護攻撃"`)。ダメージ増加系の精神効果が乗らず、オプション
    /// `サポートアタックダメージ低下` があればダメージ ×0.7 (原典 `Unit.Damage`)。
    Support,
}

/// `resolve_strike` の結果。`damage` は防御選択・クリティカル・即死・切り払い・
/// 防御特性を反映した最終ダメージ (吸収なら負)。命中しなかった場合は適用しない。
struct StrikeOutcome {
    preview: combat::CombatPreview,
    atk_statuses: Vec<String>,
    effective_hit: i32,
    roll: i32,
    hit: bool,
    critical: bool,
    parried: bool,
    damage: i64,
}

impl App {
    /// 攻撃の本体。`desired_target` が `Some(tile)` ならそのタイルの敵を対象に、
    /// `None` なら射程内最寄り敵を対象に攻撃を実行する。`post_move` は移動後攻撃
    /// (Ｐ属性制限) かどうか。
    ///
    /// `def_mode` は防御側の反撃手段 (SRC 反撃モード):
    /// - `""` / `"反撃"`: 通常命中・ダメージ＋反撃 (自動反撃と同じ)。
    /// - `"回避"`: 命中率 ×0.5、反撃なし。
    /// - `"防御"`: ダメージ ×0.5、反撃なし。
    pub(super) fn attack_resolve_and_run(
        &mut self,
        desired_target: Option<(u32, u32)>,
        post_move: bool,
        def_mode: &str,
    ) -> bool {
        // 再攻撃 (VB5 begin ループ) の再帰呼び出しか。`reattack_in_progress` で受け渡し、
        // この呼び出し内では使用イベント再発火と 3回目の再攻撃を抑止する。
        let is_reattack = self.reattack_in_progress;
        if self.scene != Scene::MapView {
            return false;
        }
        if self.stage_state != crate::stage::StageState::Battle {
            return false;
        }
        let Some((cx, cy)) = self.map_cursor else {
            return false;
        };

        // カーソル上のユニット
        let atk_idx = self
            .database
            .unit_instances
            .iter()
            .position(|u| u.x == cx && u.y == cy);
        let Some(mut atk_idx) = atk_idx else {
            return false;
        };

        let atk_party = self.database.unit_instances[atk_idx].party;
        if atk_party != self.turn.phase.party() {
            return false;
        }

        let atk_pilot_name = self.database.unit_instances[atk_idx].pilot_name.clone();
        let atk_unit_name = self.database.unit_instances[atk_idx].unit_data_name.clone();
        // 戦闘予測には実行時実効値 (育成 / 強化パーツ / 状態異常) を反映した複製を使う。
        let Some((atk_pilot, atk_unit)) = self.database.effective_combat_data(atk_idx) else {
            return false;
        };

        // 攻撃武器の固定指定 (selected_weapon_idx: 0=自動)。所有権ある複製にする。
        let forced_weapon = if self.selected_weapon_idx == 0 {
            None
        } else {
            atk_unit.weapons.get(self.selected_weapon_idx - 1).cloned()
        };
        // SRC `Charge` フラグで Ｃ 属性武器が解禁される。
        let atk_charged = self.database.unit_instances[atk_idx].charged;
        // 精神「突撃」で移動後でも長射程武器が使える。
        let atk_totsugeki = self.database.unit_instances[atk_idx].has_condition("突撃");

        // 対象ユニット (def_idx) と使用武器を確定する。
        let (mut def_idx, weapon) = if let Some(tt) = desired_target {
            // クリックされたタイルの敵を厳密に対象とする (最寄り敵には流れない)。
            let def_idx = self.database.unit_instances.iter().position(|u| {
                !u.off_map && u.x == tt.0 && u.y == tt.1 && u.party.is_hostile_to(atk_party)
            });
            let Some(def_idx) = def_idx else {
                return false;
            };
            let d = combat::manhattan((cx, cy), tt);
            let Some(weapon) = Self::pick_attack_weapon(
                &atk_unit,
                d,
                forced_weapon.as_ref(),
                atk_charged,
                post_move,
                atk_totsugeki,
                |w| self.weapon_firable(atk_idx, w),
                |w| self.weapon_skill_ok(atk_idx, w),
            ) else {
                self.push_message(format!(
                    "{}: 射程内に使用可能な武器がありません.",
                    atk_pilot.nickname
                ));
                return false;
            };
            (def_idx, weapon)
        } else {
            // 射程内最寄り敵を探索 (AI / 旧 a キー)。
            let mut best: Option<(usize, u32)> = None;
            for (i, def) in self.database.unit_instances.iter().enumerate() {
                if i == atk_idx || !def.party.is_hostile_to(atk_party) || def.off_map {
                    continue;
                }
                let d = combat::manhattan((cx, cy), (def.x, def.y));
                if Self::pick_attack_weapon(
                    &atk_unit,
                    d,
                    forced_weapon.as_ref(),
                    atk_charged,
                    post_move,
                    atk_totsugeki,
                    |w| self.weapon_firable(atk_idx, w),
                    |w| self.weapon_skill_ok(atk_idx, w),
                )
                .is_some()
                {
                    match best {
                        None => best = Some((i, d)),
                        Some((_, bd)) if d < bd => best = Some((i, d)),
                        _ => {}
                    }
                }
            }
            let Some((def_idx, dist)) = best else {
                // 武器固定中で対象なしなら情報メッセージを出す
                if let Some(w) = forced_weapon.as_ref() {
                    self.push_message(format!(
                        "{}: 射程内に対象なし ({}).",
                        atk_pilot.nickname, w.name
                    ));
                }
                return false;
            };
            let Some(weapon) = Self::pick_attack_weapon(
                &atk_unit,
                dist,
                forced_weapon.as_ref(),
                atk_charged,
                post_move,
                atk_totsugeki,
                |w| self.weapon_firable(atk_idx, w),
                |w| self.weapon_skill_ok(atk_idx, w),
            ) else {
                return false;
            };
            (def_idx, weapon)
        };

        let def_inst = self.database.unit_instances[def_idx].clone();
        let def_combat = self.database.effective_combat_data(def_idx);
        let map = self.database.map.as_ref();
        let def_terrain_id = map.map(|m| m.cell(def_inst.x, def_inst.y).terrain_id);
        let (Some((def_pilot, mut def_unit)), Some(terrain_id)) = (def_combat, def_terrain_id)
        else {
            return false;
        };
        // 防御特性による装甲調整 (弱点→半減 / 吸収→無視)。VB5 `Unit.cls:6949-6951`。
        self.apply_defense_armor_mod(def_idx, &weapon.class, &mut def_unit);
        let def_hit_mod = self.database.terrain_hit_mod(terrain_id);
        let def_damage_mod = self.database.terrain_damage_mod(terrain_id);
        // Ｃ 属性武器を使った場合、charged フラグを消費 (次回攻撃前に再 Charge 必要)。
        if combat::is_charge_weapon(&weapon) {
            self.database.unit_instances[atk_idx].charged = false;
        }
        // SRC `使用 <unit> <device>:` (`使用イベント.md`) ─ `攻撃イベント` の前に発火。
        // 再攻撃 (VB5 `begin` ラベルは使用イベントより後ろ) では再発火しない。
        if !is_reattack {
            crate::event_runtime::fire_use_event_labels(self, atk_idx, &weapon.name);
        }

        // 戦闘イベント系システム変数を設定 (`変数.md`: 対象ユニット使用武器 等)。
        // 攻撃イベント発火前に攻撃側武器を登録し、相手武器は後で更新する。
        let atk_weapon_num = atk_unit
            .weapons
            .iter()
            .position(|w| w.name == weapon.name)
            .map(|i| i + 1)
            .unwrap_or(1);
        self.set_script_var("対象ユニット使用武器".to_string(), weapon.name.clone());
        self.set_script_var(
            "対象ユニット使用武器番号".to_string(),
            atk_weapon_num.to_string(),
        );
        // 武器発射による EN・残弾消費 (撃破前=index 失効前に消費。命中の有無に依らない)。
        self.consume_weapon_resources(atk_idx, atk_weapon_num - 1);
        // 攻撃元 = 対象 / 攻撃先 = 相手。`攻撃イベント.md` の解説どおり、ユニットを
        // 陣営名で指定した場合に実ユニットを識別できるよう、パイロット名と
        // 一意 uid (`対象ユニットＩＤ` / `相手ユニットＩＤ`) を発火前に設定する。
        // これらは `攻撃後イベント` にもそのまま引き継がれる (script_vars 永続)。
        let atk_uid = self.database.unit_instances[atk_idx].uid.clone();
        self.set_script_var("対象ユニットＩＤ".to_string(), atk_uid.clone());
        self.set_script_var("対象パイロット".to_string(), atk_pilot_name.clone());
        self.set_script_var("相手ユニットＩＤ".to_string(), def_inst.uid.clone());
        self.set_script_var("相手パイロット".to_string(), def_inst.pilot_name.clone());
        self.set_script_var("相手ユニット使用武器".to_string(), String::new());
        self.set_script_var("相手ユニット使用武器番号".to_string(), "0".to_string());
        self.set_script_var("対象ユニット使用アビリティ".to_string(), String::new());
        self.set_script_var(
            "対象ユニット使用アビリティ番号".to_string(),
            "0".to_string(),
        );
        self.set_script_var(
            "対象ユニット使用スペシャルパワー".to_string(),
            String::new(),
        );
        self.set_script_var("サポートアタックユニットＩＤ".to_string(), String::new());
        self.set_script_var("サポートガードユニットＩＤ".to_string(), String::new());

        // SRC `攻撃 <atk> <def>:` (`攻撃イベント.md`) ─ ダメージ適用前に発火。
        // 発火中に Talk 等で UI が中断するとダメージは保留されるが、本実装は
        // Destruction 同様 sync 完結する Set/Message/Goto/ChangeMode を想定。
        crate::event_runtime::fire_attack_event_labels(self, atk_idx, def_idx);

        // 攻撃後ラベル発火用に attacker/defender 識別子を退避。
        // damage 適用で defender が unit_instances から消えたあとに名前が必要。
        let after_atk = crate::event_runtime::UnitEventId::from_unit_instance(
            &self.database.unit_instances[atk_idx],
        );
        let after_def = crate::event_runtime::UnitEventId::from_unit_instance(
            &self.database.unit_instances[def_idx],
        );

        // 先制攻撃 (カウンター/先読み, SRC `COM.bas:1040-1058`): 防御側が反撃武器を射程内に
        // 持ち先手を取れるなら、主攻撃の**前に**防御側が反撃する。`reattack` (再攻撃) では
        // 先制反撃しない (二重反撃を避ける)。先制反撃で攻撃側が撃破されたら主攻撃は不発。
        let preempted = if is_reattack {
            false
        } else {
            self.try_preemptive_counter(def_idx, (cx, cy), (def_inst.x, def_inst.y), &weapon.class)
        };
        if preempted {
            let atk_alive = self
                .database
                .unit_instances
                .iter()
                .any(|u| !u.off_map && u.x == cx && u.y == cy);
            if !atk_alive {
                self.check_victory();
                return true;
            }
            // 先制反撃で index がずれ得る (撃破/みがわり) ため位置で引き直す。
            if let Some(i) = self
                .database
                .unit_instances
                .iter()
                .position(|u| u.x == cx && u.y == cy)
            {
                atk_idx = i;
            }
            if let Some(i) = self
                .database
                .unit_instances
                .iter()
                .position(|u| u.x == def_inst.x && u.y == def_inst.y)
            {
                def_idx = i;
            }
        }

        let strike = self.resolve_strike(
            atk_idx,
            def_idx,
            &StrikeSetup {
                atk_pilot: &atk_pilot,
                atk_unit: &atk_unit,
                weapon: &weapon,
                def_pilot: &def_pilot,
                def_unit: &def_unit,
                def_inst: &def_inst,
                def_hit_mod,
                def_damage_mod,
            },
            def_mode,
            StrikeKind::Main,
        );
        let StrikeOutcome {
            preview,
            atk_statuses,
            effective_hit,
            roll,
            hit,
            critical,
            parried,
            damage: applied_damage,
        } = strike;
        let mut defender_killed = false;

        // 援護防御チェック。`def_mode` で挙動を切替える (SRC 反撃モードの「援護防御
        // ON/OFF」):
        //  - "援護防御": プレイヤーが明示選択 → フェイズに依らず強制発動。
        //  - "反撃"/"回避"/"防御": 個別反撃を選んだ → 援護防御は発動しない。
        //  - ""(自動/ヘッドレス/行動不能の素通り): 従来どおり 敵/中立/ＮＰＣ フェイズの
        //    攻撃時のみ自動発動 (味方が自分から攻撃した Player フェイズは無効)。
        // 直撃 (精神コマンド): サポートガード (援護防御) を無効化する (§0.8 / SRC)。
        let atk_chokugeki = atk_statuses.iter().any(|s| s.contains("直撃"));
        let guard_idx = if !hit || atk_chokugeki {
            None
        } else {
            match def_mode {
                "援護防御" => self.try_find_support_guard(def_idx, preview.damage),
                "反撃" | "回避" | "防御" => None,
                _ if self.turn.phase != crate::turn::Phase::Player => {
                    self.try_find_support_guard(def_idx, preview.damage)
                }
                _ => None,
            }
        };

        let msg = if hit {
            if let Some(sub) = self.apply_migawari(def_idx, atk_idx, &weapon.class, applied_damage)
            {
                // みがわり (身代わり, 最優先・SRC Unit.cls:14855): 指定された身代わりユニットが
                // 防御側のダメージを 1 回肩代わりする。防御側は無傷、condition は消費される。
                format!(
                    "みがわり: {} が {} の身代わりになった！ [{}]",
                    sub, def_pilot.nickname, weapon.name
                )
            } else if let Some(g_idx) = guard_idx {
                // 援護防御: guard unit が 50% ダメージを受ける (防御扱い)
                let guard_dmg = (preview.damage as f64 * 0.5) as i64;
                let guard_unit_name = self.database.unit_instances[g_idx].unit_data_name.clone();
                let guard_pilot_name = self.database.unit_instances[g_idx].pilot_name.clone();
                let guard_unit = self.database.unit_by_name(&guard_unit_name).cloned();
                let guard_pilot = self.database.pilot_by_name(&guard_pilot_name).cloned();
                self.database.unit_instances[g_idx].support_guard_remaining -= 1;
                let g_max_hp = self
                    .database
                    .effective_max_hp(&self.database.unit_instances[g_idx]);
                // バリア吸収 (B): guard 役が バリアLv* を持てば 1000×Lv を吸収。
                let guard_dmg = self.apply_barrier_absorb(g_idx, atk_idx, &weapon.class, guard_dmg);
                // アクティブフィールド (B): 確率発動の 500×Lv 吸収。
                let guard_dmg =
                    self.apply_active_field_absorb(g_idx, atk_idx, &weapon.class, guard_dmg);
                // シールド防御 (B): guard 役が Ｓ防御+シールドを持てば被ダメージ半減。
                let guard_dmg = self.apply_shield_defense(g_idx, &weapon.class, guard_dmg);
                // 不死身: 援護防御で身代わりになった guard も撃破させない。
                let guard_dmg = self.cap_damage_for_immortal(g_idx, g_max_hp, guard_dmg);
                let guard_old_dmg = self.database.unit_instances[g_idx].damage;
                self.database.unit_instances[g_idx].damage += guard_dmg;
                let remaining = g_max_hp - self.database.unit_instances[g_idx].damage;
                if remaining <= 0 && self.revive_if_possible(g_idx) {
                    format!(
                        "援護防御: {} が {} の代わりに受けた → 撃破… だが【復活】！",
                        guard_pilot
                            .as_ref()
                            .map(|p| p.nickname.as_str())
                            .unwrap_or("?"),
                        def_pilot.nickname
                    )
                } else if remaining <= 0 {
                    let m = format!(
                        "援護防御: {} が {} の代わりに受けた → 撃破！",
                        guard_pilot
                            .as_ref()
                            .map(|p| p.nickname.as_str())
                            .unwrap_or("?"),
                        def_pilot.nickname
                    );
                    let guard_party = self.database.unit_instances[g_idx].party;
                    self.database.remove_unit_at(g_idx);
                    if let (Some(gp), Some(gu)) = (guard_pilot, guard_unit) {
                        self.fire_destruction_label(&gp.name, &gu.name, guard_party);
                    }
                    m
                } else {
                    // 損傷率イベント (援護防御で guard が閾値を割った場合)。
                    self.fire_damage_threshold_for_unit(g_idx, g_max_hp, guard_old_dmg);
                    format!(
                        "援護防御: {} が {} の代わりに {} ダメージ (残HP {})",
                        guard_pilot
                            .as_ref()
                            .map(|p| p.nickname.as_str())
                            .unwrap_or("?"),
                        def_pilot.nickname,
                        guard_dmg,
                        remaining
                    )
                }
            } else {
                // 通常ダメージ: defender が受ける (防御選択時は applied_damage で半減済)。
                // 不死身: 撃破させないよう被ダメージをキャップ (サンプルの「不死身で
                // 耐える→損傷率 50% で回復＆不死身解除」演出を成立させる)。
                // バリア吸収 (B): 防御側が バリアLv* を持てば 1000×Lv を吸収。
                let applied_damage =
                    self.apply_barrier_absorb(def_idx, atk_idx, &weapon.class, applied_damage);
                // アクティブフィールド (B): 確率発動の 500×Lv 吸収。
                let applied_damage =
                    self.apply_active_field_absorb(def_idx, atk_idx, &weapon.class, applied_damage);
                // シールド防御 (B): 防御側が Ｓ防御+シールドを持てば被ダメージ半減。
                let applied_damage =
                    self.apply_shield_defense(def_idx, &weapon.class, applied_damage);
                let applied_damage =
                    self.cap_damage_for_immortal(def_idx, def_unit.hp, applied_damage);
                let def_old_dmg = self.database.unit_instances[def_idx].damage;
                self.database.unit_instances[def_idx].damage += applied_damage;
                let remaining = def_unit.hp - self.database.unit_instances[def_idx].damage;
                if remaining <= 0 {
                    if self.revive_if_possible(def_idx) {
                        // 精神コマンド「復活」: HP0 でも HP 全快で立ち上がる (1 回で消費)。
                        format!(
                            "{} ({}) → {} ({}) [{}]: 撃破… だが【復活】で立ち上がった！",
                            atk_pilot.nickname,
                            atk_unit.name,
                            def_pilot.nickname,
                            def_unit.name,
                            weapon.name,
                        )
                    } else {
                        // 精神コマンド「努力」: 次の戦闘で得る経験値 2 倍 (1 回で消費)。
                        let doubled = self
                            .database
                            .unit_instances
                            .iter()
                            .find(|u| u.x == cx && u.y == cy)
                            .map(|u| u.has_condition("努力"))
                            .unwrap_or(false);
                        let exp = def_pilot.exp_value * if doubled { 2 } else { 1 };
                        let victim_value = def_unit.value;
                        let mut m = format!(
                            "{} ({}) → {} ({}) [{}]: {}撃破！ EXP +{}{}",
                            atk_pilot.nickname,
                            atk_unit.name,
                            def_pilot.nickname,
                            def_unit.name,
                            weapon.name,
                            if critical {
                                "クリティカル！ "
                            } else {
                                ""
                            },
                            exp,
                            if doubled { " (努力)" } else { "" },
                        );
                        self.database.remove_unit_at(def_idx);
                        // 撃破側ユニットを位置で再特定し、経験値 / 育成 / 資金を付与する
                        // (PilotInstance 成長 + レベルアップイベント + 資金 + 努力/幸運 消費)。
                        if let Some(killer_idx) = self
                            .database
                            .unit_instances
                            .iter()
                            .position(|u| u.x == cx && u.y == cy)
                        {
                            let money = self.award_kill_rewards(
                                killer_idx,
                                exp,
                                victim_value,
                                def_inst.party,
                            );
                            if money > 0 {
                                m.push_str(&format!(" 資金 +{money}"));
                            }
                        }
                        defender_killed = true;
                        m
                    }
                } else {
                    // 損傷率イベント (SRC `損傷率 <unit> <pct>:`): 戦闘ダメージで HP が
                    // 閾値 (例 50%) を割ったら発火する。旧実装は `Damage` コマンド経由
                    // (apply_damage) でしか発火せず、実戦闘では発火していなかった。
                    // サンプルシナリオの「不死身で耐える→損傷率 50% で HP 回復＋不死身
                    // 解除」演出はこの発火に依存する。
                    self.fire_damage_threshold_for_unit(def_idx, def_unit.hp, def_old_dmg);
                    // 攻撃を受けて生存したユニットは気力 +1 (SRC `Unit.cs`: 損傷による気力上昇)。
                    {
                        let dm = &mut self.database.unit_instances[def_idx];
                        dm.morale = (dm.morale + 1).clamp(0, 150);
                    }
                    // 命中時/損傷時 気力増加スキル。
                    self.apply_combat_morale_skills(atk_idx, def_idx, true);
                    // 特殊効果攻撃属性: 命中かつ生存時に確率で状態異常を付与。
                    // 切り払いで無効化された攻撃は特殊効果も発動しない。
                    let inflicted = if parried {
                        Vec::new()
                    } else {
                        self.apply_weapon_special_effects(
                            def_idx, atk_idx, &weapon, &atk_pilot, &def_pilot,
                        )
                    };
                    // 衰 / 滅: クリティカル時に対象の現在 HP / EN を割合減少させる。
                    // 引 / 転: クリティカル時に対象の位置を移す。盗: 資金を奪う。
                    // 写 / 化: 発動者が対象のユニットへ変化する。
                    let decayed = if critical && !parried {
                        let mut d = self.apply_weapon_crit_decay(def_idx, &weapon);
                        d.extend(self.apply_weapon_crit_reposition(def_idx, (cx, cy), &weapon));
                        d.extend(self.apply_weapon_crit_steal(
                            def_idx,
                            atk_party,
                            def_unit.value,
                            &weapon,
                        ));
                        d.extend(self.apply_weapon_crit_copy(atk_idx, def_idx, &weapon));
                        // 習: クリティカル時に対象の「ラーニング可能技」を習得する。
                        d.extend(self.apply_weapon_learning(atk_idx, def_idx, &weapon));
                        d
                    } else {
                        Vec::new()
                    };
                    // 吹 / Ｋ: 命中時に対象を遠ざかる方向へ押し出す。
                    let knocked = if parried {
                        false
                    } else {
                        self.apply_weapon_knockback(
                            def_idx,
                            (cx, cy),
                            &atk_unit_name,
                            &weapon,
                            critical,
                        )
                    };
                    let mut m = format!(
                        "{} → {} [{}]: 命中 {} ダメージ{} (残HP {})",
                        atk_pilot.nickname,
                        def_pilot.nickname,
                        weapon.name,
                        applied_damage,
                        if critical {
                            " クリティカル！"
                        } else {
                            ""
                        },
                        remaining
                    );
                    if !inflicted.is_empty() {
                        m.push_str(&format!(" → {}", inflicted.join("・")));
                    }
                    if !decayed.is_empty() {
                        m.push_str(&format!(" → {}", decayed.join("・")));
                    }
                    if knocked {
                        m.push_str(" → 吹き飛ばし");
                    }
                    m
                }
            }
        } else {
            // 失敗時/回避時 気力増加スキル (攻撃を外した側 / 回避した側)。
            self.apply_combat_morale_skills(atk_idx, def_idx, false);
            format!(
                "{} → {} [{}]: ミス (要 {}%, ロール {})",
                atk_pilot.nickname, def_pilot.nickname, weapon.name, effective_hit, roll
            )
        };
        self.push_message(msg);

        // 戦闘演出 (animate_battle 時のみ)。`animation.txt` で戦闘アニメサブルーチンが
        // 解決でき、かつそれが script_library に存在すればスクリプト再生を優先する
        // (SRC 汎用戦闘アニメ経路)。再生できなければネイティブ演出にフォールバック。
        if self.animate_battle {
            let played = self.try_play_battle_animation(&atk_unit_name, &weapon.name, hit);
            if !played {
                // ネイティブ演出: 攻撃側→防御側タイルへの命中フラッシュ・着弾・ダメージ
                // 数字を `tick` 駆動で短く再生。援護防御の肩代わりは防御側被弾 0 とする。
                let (anim_hit, anim_damage) = if hit && guard_idx.is_none() {
                    (true, applied_damage)
                } else if hit {
                    (true, 0)
                } else {
                    (false, 0)
                };
                self.battle_anim = Some(crate::battle_anim::BattleAnim::new(
                    (cx, cy),
                    (def_inst.x, def_inst.y),
                    anim_hit,
                    anim_damage,
                    defender_killed,
                    crate::battle_anim::AttackKind::from_weapon(&weapon),
                ));
            }
        }

        // 援護防御が発動した場合、サポートガードユニットの UID をシステム変数に記録。
        if let Some(g_idx) = guard_idx {
            // g_idx はダメージ後 (remove の可能性あり) — まだ存在するか確認
            if let Some(gu) = self.database.unit_instances.get(g_idx) {
                let uid = gu.uid.clone();
                self.set_script_var("サポートガードユニットＩＤ".to_string(), uid);
            }
        }

        // 援護攻撃: 攻撃側と同じ陣営の隣接ユニットで `support_attack_remaining > 0`
        // かつ "サポートアタック" / "援護攻撃" 特殊能力を持つユニットが、防御側を
        // 武器射程内に収めていれば 1 回追撃する。
        // 援護防御でガード機が除去された場合 atk_idx がずれ得るため uid で引き直す。
        let support_uid = match (defender_killed, self.database.idx_by_uid(&atk_uid)) {
            (false, Some(ai)) => self.try_support_attack(ai, def_inst.x, def_inst.y, def_mode),
            _ => None,
        };
        // 反撃: 防御側が生存しており、攻撃側まで射程内の武器を持っていれば 1 回反撃。
        // ただし回避/防御を選んだ場合は反撃しない (SRC 反撃モード)。反撃 or 自動("")のみ。
        // 先制反撃 (preempted) 済みなら主攻撃後の通常反撃は行わない (SRC: def_mode="先制攻撃")。
        let allow_counter =
            !preempted && def_mode != "回避" && def_mode != "防御" && def_mode != "援護防御";
        let still_alive_def = self
            .database
            .unit_instances
            .iter()
            .any(|u| u.x == def_inst.x && u.y == def_inst.y);
        let counter_weapon = if allow_counter && !defender_killed && still_alive_def {
            self.try_counterattack(def_inst.x, def_inst.y, (cx, cy))
        } else {
            None
        };
        // 撃破時の自動発火ラベル `Destruction <unit>` / `<pilot>` / `破壊 <陣営>` を実行。
        if defender_killed {
            self.fire_destruction_label(&def_pilot.name, &def_unit.name, def_inst.party);
        }

        // 攻撃後イベント発火前に残り戦闘系システム変数を更新。
        // 相手ユニット使用武器 = 反撃武器 (反撃なしなら空文字)。
        if let Some((cname, cnum)) = counter_weapon {
            self.set_script_var("相手ユニット使用武器".to_string(), cname);
            self.set_script_var("相手ユニット使用武器番号".to_string(), cnum.to_string());
        }
        if let Some(uid) = support_uid {
            self.set_script_var("サポートアタックユニットＩＤ".to_string(), uid);
        }

        // SRC `使用後 <unit> <device>:` (`使用後イベント.md`) ─ `攻撃後` の前、
        // attacker 生存時のみ発火。
        crate::event_runtime::fire_after_use_event_labels(self, &after_atk, &weapon.name);

        // SRC `攻撃後 <atk> <def>:` (`攻撃後イベント.md`) ─ 損傷率/破壊
        // ラベル発火後、双方が生存している場合に限り発火。
        crate::event_runtime::fire_after_attack_event_labels(self, &after_atk, &after_def);

        // 再攻撃 (VB5 `Unit.cls:10239-10270`): 主攻撃側が 再攻撃 技能 / 再属性 を持ち、
        // 防御側が生存していれば確率で同じ攻撃をもう一度実行する (1回限り)。武器の EN/弾が
        // 尽きていれば 2回目の武器選択で自然に不発になる。`reattack_in_progress` で使用
        // イベント再発火と 3回目の再攻撃を抑止する。
        if !is_reattack && !defender_killed {
            let def_here = self.database.unit_instances.iter().any(|u| {
                !u.off_map && u.x == def_inst.x && u.y == def_inst.y && u.party == def_inst.party
            });
            if let (Some(ai), true) = (self.database.idx_by_uid(&atk_uid), def_here) {
                if self.rolls_reattack(ai, &atk_pilot, &def_pilot, &weapon) {
                    self.push_message(format!("{} の再攻撃！", atk_pilot.nickname));
                    self.map_cursor = Some((
                        self.database.unit_instances[ai].x,
                        self.database.unit_instances[ai].y,
                    ));
                    self.reattack_in_progress = true;
                    self.attack_resolve_and_run(
                        Some((def_inst.x, def_inst.y)),
                        post_move,
                        def_mode,
                    );
                    self.reattack_in_progress = false;
                }
            }
        }

        self.check_victory();
        true
    }

    /// 攻撃 1 回分の予測 (命中率・ダメージ・クリティカル率)。状態異常・精神コマンド・
    /// 特殊能力・地形適応・与被ダメージ修正・ECM・散布補正を反映する。本線攻撃・援護攻撃・
    /// 反撃で共通。防御側の状態と位置は `setup.def_inst` (呼び出し側が取ったスナップショット)
    /// を使い、攻撃側は現在の状態を使う。戻り値の 2 つ目は攻撃側の状態名一覧。
    fn strike_preview(
        &self,
        atk_idx: usize,
        def_idx: usize,
        setup: &StrikeSetup,
        kind: StrikeKind,
    ) -> (combat::CombatPreview, Vec<String>) {
        let is_counter = kind == StrikeKind::Counter;
        let StrikeSetup {
            atk_pilot,
            atk_unit,
            weapon,
            def_pilot,
            def_unit,
            def_inst,
            def_hit_mod,
            def_damage_mod,
        } = *setup;
        let atk_morale = self.database.unit_instances[atk_idx].morale;
        let def_morale = def_inst.morale;
        // 反撃では 捨て身 を無効にする (§0.8 / SRC: 自分から攻撃したときのみ ×3)。
        let mut atk_statuses: Vec<String> = self.database.unit_instances[atk_idx]
            .conditions
            .iter()
            .map(|c| c.name.clone())
            .filter(|n| !(is_counter && n == "捨て身"))
            .collect();
        push_combat_feature_statuses(&self.database.unit_instances[atk_idx], &mut atk_statuses);
        let mut def_statuses: Vec<String> =
            def_inst.conditions.iter().map(|c| c.name.clone()).collect();
        push_combat_feature_statuses(def_inst, &mut def_statuses);
        // 地形適応 (SRC `戦闘システム詳細.md`): 攻撃側は自地形、防御側は自地形で参照。
        let atk_env = self.terrain_env_at(
            self.database.unit_instances[atk_idx].x,
            self.database.unit_instances[atk_idx].y,
        );
        let def_env = self.terrain_env_at(def_inst.x, def_inst.y);
        // 与/被ダメージ修正 (ダメージ増加/被ダメージ増加/ダメージ低下/被ダメージ低下) を
        // 攻撃側/防御側それぞれの精神効果から sp.txt 経由で解決 (シナリオ固有値を反映)。
        let mut dmg_levels = self
            .database
            .damage_spirit_levels(&atk_statuses, &def_statuses);
        if kind == StrikeKind::Support {
            // 援護攻撃にはダメージ増加 / 被ダメージ増加の精神効果が乗らない (低下系は乗る)。
            dmg_levels.atk_increase = 0.0;
            dmg_levels.def_increase_taken = 0.0;
        }
        let preview = combat::predict_with_status_terrain(
            atk_pilot,
            atk_unit,
            weapon,
            def_pilot,
            def_unit,
            def_hit_mod,
            def_damage_mod,
            atk_morale,
            def_morale,
            &atk_statuses,
            &def_statuses,
            atk_env,
            def_env,
            dmg_levels,
            self.ecm_hit_mult(def_idx, atk_idx, &weapon.class),
        );
        // 散 (散布) 属性武器の距離補正 (命中アップ・ダメージダウン)。
        let atk_def_dist = combat::manhattan(
            (
                self.database.unit_instances[atk_idx].x,
                self.database.unit_instances[atk_idx].y,
            ),
            (
                self.database.unit_instances[def_idx].x,
                self.database.unit_instances[def_idx].y,
            ),
        );
        let mut preview = preview.apply_scatter(&weapon.class, atk_def_dist);
        if kind == StrikeKind::Support
            && self.is_script_var_defined("Option(サポートアタックダメージ低下)")
        {
            preview.damage = (preview.damage as f64 * 0.7) as i64;
        }
        (preview, atk_statuses)
    }

    /// `atk_idx` が `weapon` で `def_idx` を攻撃するときの予測 (読み取り専用)。本線攻撃と
    /// 同じ組み立て (実効データ・防御特性による装甲調整・防御側の地形補正・状態・精神効果・
    /// ECM・散布補正) なので、反撃ウィンドウ・武器選択窓・ステータスパネルの表示が実際の
    /// 戦闘と食い違わない。
    pub(crate) fn forecast_strike(
        &self,
        atk_idx: usize,
        def_idx: usize,
        weapon: &crate::data::unit::WeaponData,
    ) -> Option<combat::CombatPreview> {
        let (atk_pilot, atk_unit) = self.database.effective_combat_data(atk_idx)?;
        let (def_pilot, mut def_unit) = self.database.effective_combat_data(def_idx)?;
        self.apply_defense_armor_mod(def_idx, &weapon.class, &mut def_unit);
        let def_inst = self.database.unit_instances[def_idx].clone();
        let terrain_id = self
            .database
            .map
            .as_ref()
            .map_or(0, |m| m.cell(def_inst.x, def_inst.y).terrain_id);
        let (preview, _) = self.strike_preview(
            atk_idx,
            def_idx,
            &StrikeSetup {
                atk_pilot: &atk_pilot,
                atk_unit: &atk_unit,
                weapon,
                def_pilot: &def_pilot,
                def_unit: &def_unit,
                def_inst: &def_inst,
                def_hit_mod: self.database.terrain_hit_mod(terrain_id),
                def_damage_mod: self.database.terrain_damage_mod(terrain_id),
            },
            StrikeKind::Main,
        );
        Some(preview)
    }

    /// 1 回の攻撃の命中・ダメージ判定。プレイヤー / AI の本線攻撃
    /// (`attack_resolve_and_run`)・援護攻撃・反撃・`.eve Attack` (`script_attack`) の共通コア
    /// (原典はどれも `Unit.Attack` を通る)。
    /// 状態異常・精神コマンド・地形・散布補正を反映した予測から、命中 / クリティカル /
    /// 即死 / 切り払い / 防御特性 を判定する。乱数は本線攻撃と同じ順に消費する。
    /// ダメージの適用 (援護防御・みがわり・バリア等) は呼び出し側が行う。
    fn resolve_strike(
        &mut self,
        atk_idx: usize,
        def_idx: usize,
        setup: &StrikeSetup,
        def_mode: &str,
        kind: StrikeKind,
    ) -> StrikeOutcome {
        let StrikeSetup {
            atk_pilot,
            weapon,
            def_pilot,
            def_unit,
            ..
        } = *setup;
        let (preview, atk_statuses) = self.strike_preview(atk_idx, def_idx, setup, kind);

        // 命中判定。回避を選んだ防御側は命中率が半減する (SRC 反撃モード)。
        let effective_hit = if def_mode == "回避" {
            preview.hit_chance / 2
        } else {
            preview.hit_chance
        };
        // 1 つの乱数から命中(下位)とクリティカル(上位)を取り出す。下位 2 桁は従来の
        // 命中ロールと同一なので、命中可否・後続の乱数列 (反撃/援護) は一切変わらない。
        let r = self.next_u32();
        let roll = (r % 100) as i32;
        // 回避系特殊能力 (B): 命中するはずでも 分身/超回避 で完全回避され得る。
        let hit =
            roll < effective_hit && !self.check_dodge_feature(def_idx, atk_idx, &weapon.class);
        // クリティカル判定 (命中時のみ)。技能補正 (超反応/超能力/底力) を base へ加算した後、
        // 防御選択でクリ率が半減する (SRC `戦闘システム詳細.md`「防御」: クリティカル率 ×0.5)。
        let crit_base = preview.critical_chance + self.crit_skill_bonus(atk_idx, def_idx);
        let crit_chance = if def_mode == "防御" {
            crit_base / 2
        } else {
            crit_base
        };
        let crit_roll = ((r / 100) % 100) as i32;
        // 特殊効果攻撃属性 (CC 属性) を持つ武器では通常のクリティカルは発生しない。
        // 特殊効果の発動がクリティカルの代わりとみなされる (SRC 特殊効果攻撃属性.md)。
        // 状態異常付与 (weapon_special_effects) と 気力減少 (脱/Ｄ) の双方が該当。
        let has_special_effect = !crate::combat::weapon_special_effects(&weapon.class).is_empty()
            || crate::combat::weapon_morale_reduction(&weapon.class).is_some()
            || weapon.class.split_whitespace().any(|t| t == "即");
        let critical = hit && crit_roll < crit_chance && !has_special_effect;
        // クリティカルはダメージ 1.5 倍 (SRC)。その後、防御選択でさらに半減する。
        let crit_damage = if critical {
            (preview.damage * 3 / 2).max(0)
        } else {
            preview.damage
        };
        let applied_damage = if def_mode == "防御" {
            (crit_damage / 2).max(0)
        } else {
            crit_damage.max(0)
        };
        // 即死 (即): 命中時に特殊効果発動率で proc し、ボス以外を致死化する
        // (`特殊効果攻撃属性.md`)。proc すると致死ダメージにして撃破サイトへ流す。
        // 非 即 武器は RNG を消費しない (既存の乱数列を保つ)。
        let applied_damage =
            if hit && self.roll_weapon_instakill(def_idx, weapon, atk_pilot, def_pilot) {
                def_unit.hp.max(applied_damage)
            } else {
                applied_damage
            };
        // 切り払い: 格闘系の攻撃を防御側が確率で無効化する (攻撃側 直撃 で無効化されない)。
        let atk_has_chokugeki = atk_statuses.iter().any(|s| s.contains("直撃"));
        let parried = hit && !atk_has_chokugeki && self.roll_parry(atk_idx, def_idx, &weapon.class);
        if parried {
            self.push_message(format!(
                "{} は {} の [{}] を切り払った！",
                def_pilot.nickname, atk_pilot.nickname, weapon.name
            ));
        }
        let applied_damage = if parried { 0 } else { applied_damage };
        // 防御特性 (耐性 ÷2 / 無効化 0 / 吸収 回復) を最終ダメージへ適用する。
        let dmg_before_def = applied_damage;
        let applied_damage = self.defense_attribute_damage(def_idx, &weapon.class, applied_damage);
        if applied_damage != dmg_before_def {
            if applied_damage < 0 {
                self.push_message(format!("{} は攻撃を吸収した！", def_pilot.nickname));
            } else if applied_damage == 0 {
                self.push_message(format!("{} には効かない (無効化)", def_pilot.nickname));
            }
        }
        StrikeOutcome {
            preview,
            atk_statuses,
            effective_hit,
            roll,
            hit,
            critical,
            parried,
            damage: applied_damage,
        }
    }

    /// `.eve Attack unit1 weapon1 unit2 weapon2` (`Attackコマンド.md`, C# `AttackCmd`)。
    /// `atk_uid` が `weapon1` で `def_uid` を攻撃し、`weapon2` が `防御`/`回避`/`無抵抗`/
    /// `反撃不能` 以外なら防御側が反撃する。命中・ダメージはプレイヤー / AI の攻撃と
    /// 同じ `resolve_strike` で判定する。イベント上の戦闘なので武器の使用条件
    /// (射程・気力) は問わず、使用 / 攻撃 / 損傷率 / 破壊 / 全滅イベント・経験値・資金・
    /// 援護攻撃 / 援護防御は発生しない。撃破されたユニットは盤面から除去する。
    pub(crate) fn script_attack(&mut self, atk_uid: &str, w1: &str, def_uid: &str, w2: &str) {
        let def_mode = match w2 {
            "防御" | "Defense" => "防御",
            "回避" | "Evade" => "回避",
            _ => "",
        };
        let counter = !matches!(
            w2,
            "防御" | "回避" | "無抵抗" | "反撃不能" | "Defense" | "Evade" | "Nodefense"
        );
        self.script_strike(atk_uid, def_uid, w1, def_mode);
        if counter {
            self.script_strike(def_uid, atk_uid, w2, "");
        }
    }

    /// `script_attack` の片側 1 回分。`weapon_arg` は武器名か `自動` (最大攻撃力)。
    fn script_strike(&mut self, atk_uid: &str, def_uid: &str, weapon_arg: &str, def_mode: &str) {
        let (Some(atk_idx), Some(def_idx)) = (
            self.database.idx_by_uid(atk_uid),
            self.database.idx_by_uid(def_uid),
        ) else {
            return;
        };
        let (Some((atk_pilot, atk_unit)), Some((def_pilot, mut def_unit))) = (
            self.database.effective_combat_data(atk_idx),
            self.database.effective_combat_data(def_idx),
        ) else {
            return;
        };
        let weapon = if weapon_arg == "自動" || weapon_arg.eq_ignore_ascii_case("auto") {
            atk_unit
                .weapons
                .iter()
                .filter(|w| !w.class.contains('Ｍ'))
                .max_by_key(|w| w.power)
        } else {
            atk_unit.weapons.iter().find(|w| w.name == weapon_arg)
        };
        let Some(weapon) = weapon.cloned() else {
            return;
        };
        self.apply_defense_armor_mod(def_idx, &weapon.class, &mut def_unit);
        let def_inst = self.database.unit_instances[def_idx].clone();
        let (def_hit_mod, def_damage_mod) = self
            .database
            .map
            .as_ref()
            .map(|m| m.cell(def_inst.x, def_inst.y).terrain_id)
            .map(|t| {
                (
                    self.database.terrain_hit_mod(t),
                    self.database.terrain_damage_mod(t),
                )
            })
            .unwrap_or_default();
        if let Some(wi) = atk_unit.weapons.iter().position(|w| w.name == weapon.name) {
            self.consume_weapon_resources(atk_idx, wi);
        }
        let strike = self.resolve_strike(
            atk_idx,
            def_idx,
            &StrikeSetup {
                atk_pilot: &atk_pilot,
                atk_unit: &atk_unit,
                weapon: &weapon,
                def_pilot: &def_pilot,
                def_unit: &def_unit,
                def_inst: &def_inst,
                def_hit_mod,
                def_damage_mod,
            },
            def_mode,
            StrikeKind::Main,
        );
        if !strike.hit {
            self.push_message(format!(
                "{} の [{}] を {} は回避した",
                atk_pilot.nickname, weapon.name, def_pilot.nickname
            ));
            return;
        }
        let dmg = self.apply_barrier_absorb(def_idx, atk_idx, &weapon.class, strike.damage);
        let dmg = self.apply_active_field_absorb(def_idx, atk_idx, &weapon.class, dmg);
        let dmg = self.apply_shield_defense(def_idx, &weapon.class, dmg);
        let dmg = self.cap_damage_for_immortal(def_idx, def_unit.hp, dmg);
        let u = &mut self.database.unit_instances[def_idx];
        u.damage = (u.damage + dmg).max(0);
        let destroyed = u.damage >= def_unit.hp;
        self.push_message(format!(
            "{} の [{}] → {} に {} ダメージ{}",
            atk_pilot.nickname,
            weapon.name,
            def_pilot.nickname,
            dmg,
            if strike.critical {
                " (クリティカル)"
            } else {
                ""
            }
        ));
        if destroyed {
            self.database.remove_unit_at(def_idx);
        }
    }

    /// 再攻撃 (VB5 `Unit.cls:10251-10270`) の発動判定。攻撃側が 再攻撃 技能を持てば
    /// `slevel = (直感 >= 相手直感 ? 2*Lv : Lv)` が `Dice(32)` 以上で発動。武器 `再Ln`
    /// 属性は `n` (レベル無しは 1) が `Dice(16)` 以上で発動。SP効果「再攻撃」condition は
    /// 無条件発動。能力を持たない攻撃側では乱数を消費しない (既存 RNG 列を保つ)。
    pub(super) fn rolls_reattack(
        &mut self,
        atk_idx: usize,
        atk_pilot: &crate::data::pilot::PilotData,
        def_pilot: &crate::data::pilot::PilotData,
        weapon: &crate::data::unit::WeaponData,
    ) -> bool {
        // SP効果「再攻撃」: 無条件で発動 (SRC IsUnderSpecialPowerEffect("再攻撃"))。
        if self.database.unit_instances[atk_idx].has_condition("再攻撃") {
            return true;
        }
        // 再攻撃 技能。
        let lv = self.unit_pilot_skill_level(atk_idx, "再攻撃");
        if lv > 0 {
            let slevel = if atk_pilot.intuition >= def_pilot.intuition {
                2 * lv
            } else {
                lv
            };
            let dice = (self.next_u32() % 32) as i32 + 1; // 1..=32
            if slevel >= dice {
                return true;
            }
        }
        // 再 武器属性 (`再Ln`、レベル無しは 1)。
        if weapon.class.contains('再') {
            let n = crate::db::weapon_class_level(&weapon.class, '再')
                .map(|f| f as i32)
                .unwrap_or(1);
            let dice = (self.next_u32() % 16) as i32 + 1; // 1..=16
            if n >= dice {
                return true;
            }
        }
        false
    }

    /// `animation.txt` から戦闘アニメサブルーチンを解決し、script_library に**実在する**
    /// ものだけを (準備→攻撃→[命中]) の順に返す。SRC 原典の段階順に対応。
    pub(super) fn resolve_battle_anim_calls(
        &self,
        atk_unit_data_name: &str,
        weapon: &str,
        hit: bool,
    ) -> Vec<crate::data::animation::ResolvedAnim> {
        use crate::data::animation::WeaponPhase;
        let phases: &[(&str, WeaponPhase)] = if hit {
            &[
                ("準備", WeaponPhase::Prep),
                ("攻撃", WeaponPhase::Attack),
                ("命中", WeaponPhase::Hit),
            ]
        } else {
            &[("準備", WeaponPhase::Prep), ("攻撃", WeaponPhase::Attack)]
        };
        let mut calls = Vec::new();
        for (label, phase) in phases {
            for r in
                self.database
                    .resolve_weapon_animation(atk_unit_data_name, weapon, label, *phase)
            {
                // 実在するラベルのみ再生対象 (未ロードの GBA サブルーチンは飛ばす)。
                if self.script_library().label_pc(&r.subroutine).is_some() {
                    calls.push(r);
                }
            }
        }
        calls
    }

    /// 戦闘アニメをイベントスクリプトとして再生できるなら起動して `true` を返す。
    ///
    /// SRC 汎用戦闘アニメ経路の最小実装: `animation.txt` で解決した `戦闘アニメ_*`
    /// サブルーチンを (準備→攻撃→[命中]) の順に `Call` する合成ドライバを生成し、
    /// イベントエンジンで実行する。サブルーチン内の `PaintPicture` は script_overlay に
    /// 積まれ、`Wait` は `pending_timer` で中断 → `tick` が再開する。
    ///
    /// 起動条件: `animate_battle` ＋ 設定「戦闘アニメを表示する」＋ 解決サブルーチンが
    /// script_library に実在 ＋ 他スクリプト/ダイアログ非実行中。いずれも満たさなければ
    /// `false` を返し、呼び出し側はネイティブ演出にフォールバックする。
    pub(super) fn try_play_battle_animation(
        &mut self,
        atk_unit_data_name: &str,
        weapon: &str,
        hit: bool,
    ) -> bool {
        if !self.animate_battle || !self.settings.battle_animation {
            return false;
        }
        if self.has_script_context() || self.pending_dialog.is_some() {
            return false;
        }
        let calls = self.resolve_battle_anim_calls(atk_unit_data_name, weapon, hit);
        if calls.is_empty() {
            return false;
        }
        // 合成ドライバ .eve を生成 (対象/相手ユニットＩＤ は呼び出し側で設定済み)。
        let mut src = String::new();
        for c in &calls {
            if c.params.is_empty() {
                src.push_str(&format!("Call {}\n", c.subroutine));
            } else {
                src.push_str(&format!("Call {} {}\n", c.subroutine, c.params));
            }
        }
        let Ok(stmts) = crate::data::event::parse(&src) else {
            return false;
        };
        let pc = crate::event_runtime::library_append(self, &stmts);
        let _ = crate::event_runtime::run_from_pc(self, pc);
        true
    }

    /// `不死身` (撃破阻止): `target_idx` が不死身なら、与ダメージを HP が 10 未満に
    /// ならないようキャップして返す (C# `Unit.attack.cs` の てかげん/不死身 と同一)。
    /// 通常攻撃・援護攻撃・援護防御・反撃の全ダメージ経路で共通に使う。
    pub(super) fn cap_damage_for_immortal(&self, target_idx: usize, max_hp: i64, dmg: i64) -> i64 {
        if self.database.unit_instances[target_idx].has_condition("不死身") {
            let cur_hp = max_hp - self.database.unit_instances[target_idx].damage;
            if cur_hp <= 10 {
                0
            } else {
                dmg.min(cur_hp - 10)
            }
        } else {
            dmg
        }
    }

    /// シールド防御 (防御能力 (B) 段階・SRC.NET `Unit.cs::CheckShieldFeature` 準拠):
    /// 防御側 `def_idx` が **Ｓ防御技能** + シールド系特殊能力 (シールド/小型/大型/アクティブ
    /// シールド) を持ち、行動可能 (行動不能/麻痺/石化/凍結/睡眠 でない) で武器が 精/殺/浸 で
    /// ないとき、`Ｓ防御Lv(+大型1/アクティブ2) >= Dice(16)` で発動し被ダメージを半減する
    /// (小型=2/3、破属性は 3/4・小型破は 5/6)。発動時は最低 10。実行段 (予測ではなく) で
    /// ロールするため、プレビューは確率的シールドを反映しない (SRC と同じ)。
    ///
    /// 発動しなければ `dmg` をそのまま返す。`バリア`/`フィールド` 系 (S防御非依存の吸収/÷2) は
    /// 対象外で、現状は予測の ÷2 近似 ((A)) のまま。
    /// みがわり (身代わり, SRC `Unit.cls:14855`): 防御側 `def_idx` が `みがわり` condition
    /// (data=身代わりユニット uid) を持つとき、攻撃ダメージ `damage` を身代わりユニットへ
    /// **1 回だけ**肩代わりさせる。肩代わりが成立したら身代わりユニットの愛称を返し
    /// condition を消費する。身代わりが盤上に居ない / 防御側自身なら `None`（通常処理）。
    ///
    /// 身代わりユニットは自身の バリア/フィールド/シールド/不死身 を適用してダメージを
    /// 受け、HP0 なら（復活が無ければ）撃破・破壊イベント発火・勝敗判定まで処理する。
    /// 近似: 肩代わりダメージは防御側に向かっていた値をそのまま用い、身代わりの装甲で
    /// 再計算はしない（SRC は再計算する）。Ｓ防御依存シールド等は身代わり基準で適用。
    pub(super) fn apply_migawari(
        &mut self,
        def_idx: usize,
        atk_idx: usize,
        weapon_class: &str,
        damage: i64,
    ) -> Option<String> {
        let sub_uid = self.database.unit_instances[def_idx]
            .conditions
            .iter()
            .find(|c| c.name == "みがわり" && !c.data.is_empty())
            .map(|c| c.data.clone())?;
        let sub_idx = self
            .database
            .idx_by_uid(&sub_uid)
            .filter(|&i| i != def_idx && !self.database.unit_instances[i].off_map)?;
        // condition を消費（1 回限り）。
        self.database.unit_instances[def_idx]
            .conditions
            .retain(|c| c.name != "みがわり");
        // 身代わりユニットがダメージを受ける（自身の バリア/フィールド/シールド/不死身）。
        let sub_max_hp = self
            .database
            .effective_max_hp(&self.database.unit_instances[sub_idx]);
        let dmg = self.apply_barrier_absorb(sub_idx, atk_idx, weapon_class, damage);
        let dmg = self.apply_active_field_absorb(sub_idx, atk_idx, weapon_class, dmg);
        let dmg = self.apply_shield_defense(sub_idx, weapon_class, dmg);
        let dmg = self.cap_damage_for_immortal(sub_idx, sub_max_hp, dmg);
        let sub_name = self.database.unit_instances[sub_idx].unit_data_name.clone();
        let sub_nick = self
            .database
            .pilot_by_name(&self.database.unit_instances[sub_idx].pilot_name)
            .map(|p| p.nickname.clone())
            .unwrap_or_else(|| sub_name.clone());
        self.database.unit_instances[sub_idx].damage += dmg;
        if sub_max_hp - self.database.unit_instances[sub_idx].damage <= 0
            && !self.revive_if_possible(sub_idx)
        {
            let p = self.database.unit_instances[sub_idx].pilot_name.clone();
            let party = self.database.unit_instances[sub_idx].party;
            self.database.remove_unit_at(sub_idx);
            crate::event_runtime::fire_destruction_labels(self, &p, &sub_name, party);
            self.check_victory();
        }
        Some(sub_nick)
    }

    pub(super) fn apply_shield_defense(
        &mut self,
        def_idx: usize,
        weapon_class: &str,
        dmg: i64,
    ) -> i64 {
        if dmg <= 0 || self.unit_pilot_skill_level(def_idx, "Ｓ防御") <= 0 {
            return dmg;
        }
        // 精 / 殺 / 浸 属性にはシールド防御不可。
        if weapon_class.contains('精') || weapon_class.contains('殺') || weapon_class.contains('浸')
        {
            return dmg;
        }
        // C# 探索順 (シールド→小型→大型→アクティブ) で最初に持つものを使い、発動率と半減率を決める。
        // 行動不能系の状態異常下では発動しない。
        let (prob, is_small, fname) = {
            let u = &self.database.unit_instances[def_idx];
            if u.has_condition("行動不能")
                || u.has_condition("麻痺")
                || u.has_condition("石化")
                || u.has_condition("凍結")
                || u.has_condition("睡眠")
            {
                return dmg;
            }
            let sdef = self.unit_pilot_skill_level(def_idx, "Ｓ防御");
            let h = |name: &str| crate::feature::has_feature(&u.active_features, name);
            if h("シールド") {
                (sdef, false, "シールド")
            } else if h("小型シールド") {
                (sdef, true, "小型シールド")
            } else if h("大型シールド") {
                (sdef + 1, false, "大型シールド")
            } else if h("アクティブシールド") {
                (sdef + 2, false, "アクティブシールド")
            } else {
                return dmg;
            }
        };
        // Ｓ防御Lv(+補正) >= Dice(16) で発動。
        let roll = (self.next_u32() % 16) as i32 + 1;
        if prob < roll {
            return dmg;
        }
        let is_break = weapon_class.contains('破');
        let reduced = match (is_break, is_small) {
            (true, true) => 5 * dmg / 6,
            (true, false) => 3 * dmg / 4,
            (false, true) => 2 * dmg / 3,
            (false, false) => dmg / 2,
        };
        let reduced = if (1..10).contains(&reduced) {
            10
        } else {
            reduced
        };
        self.push_message(format!("シールド防御({fname}): {dmg} → {reduced} ダメージ"));
        reduced
    }

    /// バリア吸収強度 (防御能力 (B)): 防御側 `def_idx` の `バリア`/`バリアLv*` 特殊能力が
    /// 武器属性に対し有効なとき `1000×Lv` を返す (複数は最大、該当無しは 0)。対象属性
    /// (`バリアLv*=別名 対象属性 …` の 2 番目のトークン) は `defense_attr_matches` で
    /// 相殺/中和 (バリア・フィールドのオプション・防御系特殊能力.md): 攻撃側 `attacker_idx` が
    /// 防御側に **隣接 (マンハッタン1)** していて、防御側のバリア/フィールドと **同名 (別名=値の
    /// 1 番目トークンが一致)** のバリア/フィールドを持つとき、その Lv を返す (複数は最大)。
    /// 相殺=完全無効 / 中和=この Lv 分だけ効果減少、に使う。条件を満たさなければ None。
    /// SRC.NET `Unit.cs::IsSameCategory` (別名一致) + 隣接判定 準拠。
    pub(super) fn same_named_adjacent_barrier_level(
        &self,
        attacker_idx: usize,
        def_idx: usize,
        alias: &str,
    ) -> Option<i32> {
        if alias.is_empty() {
            return None;
        }
        let a = &self.database.unit_instances[attacker_idx];
        let d = &self.database.unit_instances[def_idx];
        if a.off_map || d.off_map {
            return None;
        }
        let dist =
            (i64::from(a.x) - i64::from(d.x)).abs() + (i64::from(a.y) - i64::from(d.y)).abs();
        if dist != 1 {
            return None;
        }
        let mut best: Option<i32> = None;
        for f in &a.active_features {
            if !f.is_active {
                continue;
            }
            let lv = crate::feature::feature_level(std::slice::from_ref(f), "バリア")
                .or_else(|| crate::feature::feature_level(std::slice::from_ref(f), "フィールド"))
                .or_else(|| {
                    crate::feature::feature_level(std::slice::from_ref(f), "アクティブフィールド")
                });
            if let Some(lv) = lv {
                if f.value.split_whitespace().next().unwrap_or("") == alias {
                    best = Some(best.map_or(lv, |b| b.max(lv)));
                }
            }
        }
        best
    }

    /// バリア吸収 (防御能力 (B)・SRC `バリア`/`フィールド` 特殊能力): 防御側の バリア=1000×Lv /
    /// フィールド=500×Lv が、対象属性に該当する攻撃を吸収する (吸収後の残ダメージを返す。
    /// 完全吸収は 0)。確率ではなく**確定**だが、予測シグネチャ変更を避け実行段で適用する。
    /// 攻撃側 `直撃` / 防御側 `バリア中和` で無効。`attacker_idx` はダメージを与える側。
    ///
    /// **knobs (忠実性)**: 各特殊能力の値 `…=別名 対象属性 ＥＮ消費 必要気力 …` から ＥＮ消費
    /// (3 番目, 既定 0) と 必要気力 (4 番目, 既定 0) を読み、`気力≧必要気力` かつ `残EN≧ＥＮ消費`
    /// のときのみ発動し、発動した能力ぶんの ＥＮ を消費する (`&mut self`)。対象属性は 2 番目
    /// (省略=全)。`アクティブフィールド` は確率発動なので別経路、`解説…` 値はヘルプ文で対象外。
    pub(super) fn apply_barrier_absorb(
        &mut self,
        def_idx: usize,
        attacker_idx: usize,
        weapon_class: &str,
        dmg: i64,
    ) -> i64 {
        if dmg <= 0 {
            return dmg;
        }
        if self.database.unit_instances[attacker_idx].has_condition("直撃")
            || self.database.unit_instances[def_idx].has_condition("バリア中和")
        {
            return dmg;
        }
        let morale = self.database.unit_instances[def_idx].morale;
        let max_en = self
            .database
            .effective_max_en(&self.database.unit_instances[def_idx]);
        // 発動した能力の吸収強度合計と消費 ＥＮ 合計を、features を借りる scope 内で算出。
        let (total_strength, total_en) = {
            let u = &self.database.unit_instances[def_idx];
            let mut avail_en = max_en - u.en_consumed;
            let mut strength = 0i64;
            let mut en_used = 0i32;
            for f in &u.active_features {
                if !f.is_active || f.value.trim_start().starts_with("解説") {
                    continue;
                }
                if crate::feature::feature_name_matches_base(&f.name, "アクティブフィールド")
                {
                    continue; // 確率発動は apply_active_field_absorb
                }
                let (base_mult, lv) = if let Some(lv) =
                    crate::feature::feature_level(std::slice::from_ref(f), "バリア")
                {
                    (1000i64, lv)
                } else if let Some(lv) =
                    crate::feature::feature_level(std::slice::from_ref(f), "フィールド")
                {
                    (500i64, lv)
                } else {
                    continue;
                };
                let toks: Vec<&str> = f.value.split_whitespace().collect();
                let target = toks.get(1).copied().unwrap_or("");
                if !(target.is_empty() || Self::defense_attr_matches(weapon_class, target)) {
                    continue;
                }
                let en_cost: i32 = toks.get(2).and_then(|t| t.parse().ok()).unwrap_or(0);
                let morale_req: i32 = toks.get(3).and_then(|t| t.parse().ok()).unwrap_or(0);
                if morale < morale_req || avail_en < en_cost {
                    continue; // 気力不足 / EN 不足では発動しない
                }
                // 相殺/中和 (オプション・トークン 5 番目以降): 隣接する同名バリア/フィールドを
                // 持つ攻撃側で、相殺=完全無効 / 中和=攻撃側 Lv 分だけ効果減少。
                let mut eff_lv = lv;
                let sousai = toks.iter().skip(4).any(|t| t.contains("相殺"));
                let chuwa = toks.iter().skip(4).any(|t| t.contains("中和"));
                if sousai || chuwa {
                    let alias = toks.first().copied().unwrap_or("");
                    if let Some(atk_lv) =
                        self.same_named_adjacent_barrier_level(attacker_idx, def_idx, alias)
                    {
                        if sousai {
                            continue; // 相殺: 完全無効
                        }
                        eff_lv = (lv - atk_lv).max(0);
                        if eff_lv <= 0 {
                            continue;
                        }
                    }
                }
                avail_en -= en_cost;
                en_used += en_cost;
                strength += base_mult * i64::from(eff_lv);
            }
            (strength, en_used)
        };
        if total_strength <= 0 {
            return dmg;
        }
        self.database.unit_instances[def_idx].en_consumed += total_en;
        (dmg - total_strength).max(0)
    }

    /// アクティブフィールド吸収 (防御能力 (B)・防御系特殊能力.md): フィールドの**確率発動版**。
    /// 防御側が `アクティブフィールドLv*` + Ｓ防御技能を持ち、`Ｓ防御Lv >= Dice(16)` で発動し
    /// 対象属性に該当する攻撃の `500×Lv` を吸収する。攻撃側 `直撃` / 防御側 `バリア中和` で無効。
    /// 確率発動なので予測には乗せず実行段で適用する (`&mut self`)。
    pub(super) fn apply_active_field_absorb(
        &mut self,
        def_idx: usize,
        attacker_idx: usize,
        weapon_class: &str,
        dmg: i64,
    ) -> i64 {
        if dmg <= 0 || self.unit_pilot_skill_level(def_idx, "Ｓ防御") <= 0 {
            return dmg;
        }
        if self.database.unit_instances[attacker_idx].has_condition("直撃")
            || self.database.unit_instances[def_idx].has_condition("バリア中和")
        {
            return dmg;
        }
        // 吸収強度 (500×Lv) + ＥＮ消費 を、対象属性一致 + 気力/EN ゲートを満たす中から最大で選ぶ。
        // 値: `…=別名 対象属性 ＥＮ消費 必要気力 …` (既定 ＥＮ消費0 / 必要気力0)。解説は除外。
        let morale = self.database.unit_instances[def_idx].morale;
        let max_en = self
            .database
            .effective_max_en(&self.database.unit_instances[def_idx]);
        let (strength, en_cost) = {
            let u = &self.database.unit_instances[def_idx];
            let avail_en = max_en - u.en_consumed;
            let mut best = (0i64, 0i32);
            for f in &u.active_features {
                if !f.is_active || f.value.trim_start().starts_with("解説") {
                    continue;
                }
                let Some(lv) =
                    crate::feature::feature_level(std::slice::from_ref(f), "アクティブフィールド")
                else {
                    continue;
                };
                let toks: Vec<&str> = f.value.split_whitespace().collect();
                let target = toks.get(1).copied().unwrap_or("");
                if !(target.is_empty() || Self::defense_attr_matches(weapon_class, target)) {
                    continue;
                }
                let ec: i32 = toks.get(2).and_then(|t| t.parse().ok()).unwrap_or(0);
                let mr: i32 = toks.get(3).and_then(|t| t.parse().ok()).unwrap_or(0);
                if morale < mr || avail_en < ec {
                    continue;
                }
                let s = 500 * i64::from(lv);
                if s > best.0 {
                    best = (s, ec);
                }
            }
            best
        };
        if strength <= 0 {
            return dmg;
        }
        // Ｓ防御Lv >= Dice(16) で発動。発動時に ＥＮ を消費。
        let sdef = self.unit_pilot_skill_level(def_idx, "Ｓ防御");
        let roll = (self.next_u32() % 16) as i32 + 1;
        if sdef < roll {
            return dmg;
        }
        self.database.unit_instances[def_idx].en_consumed += en_cost;
        let reduced = (dmg - strength).max(0);
        self.push_message(format!("アクティブフィールド: {dmg} → {reduced} ダメージ"));
        reduced
    }

    /// ＥＣＭ エリア補正 (防御能力 (B)・SRC.NET `Unit.cs:10419`): 防御側 `def_idx` の周囲
    /// (中心からマンハッタン 3 以内かつ各軸 ±2 マス) を走査し、**防御側の味方**の最大
    /// ＥＣＭ Lv (ecm) と **攻撃側の味方**の最大 ＥＣＭ Lv (eccm) を求め、命中率に掛ける
    /// 係数 `(100 − f×max(ecm−eccm,0)) / 100` を返す (f は ホーミング `Ｈ` 武器で 10、他 5)。
    /// 補正が無ければ 1.0。`predict_with_status_terrain` の `ecm_hit_mult` 引数へ渡す。
    pub(super) fn ecm_hit_mult(
        &self,
        def_idx: usize,
        attacker_idx: usize,
        weapon_class: &str,
    ) -> f64 {
        let def = &self.database.unit_instances[def_idx];
        let (cx, cy, def_party) = (i64::from(def.x), i64::from(def.y), def.party);
        let atk_party = self.database.unit_instances[attacker_idx].party;
        let mut ecm = 0i32;
        let mut eccm = 0i32;
        for u in &self.database.unit_instances {
            if u.off_map {
                continue;
            }
            let ddx = (i64::from(u.x) - cx).abs();
            let ddy = (i64::from(u.y) - cy).abs();
            if ddx > 2 || ddy > 2 || ddx + ddy > 3 {
                continue;
            }
            let lv = crate::feature::feature_level(&u.active_features, "ＥＣＭ").unwrap_or(0);
            if lv <= 0 {
                continue;
            }
            if !u.party.is_hostile_to(def_party) {
                ecm = ecm.max(lv);
            }
            if !u.party.is_hostile_to(atk_party) {
                eccm = eccm.max(lv);
            }
        }
        let diff = (ecm - eccm).max(0);
        if diff == 0 {
            return 1.0;
        }
        let f = if weapon_class.contains('Ｈ') { 10 } else { 5 };
        f64::from((100 - f * diff).max(0)) / 100.0
    }

    /// 回避系特殊能力 (防御能力 (B)・SRC.NET `Unit.cs::CheckDodgeFeature` 準拠): 命中する
    /// はずの攻撃を完全回避するか判定する (`true` なら攻撃はミス扱い)。
    /// 分身 は気力 130 以上で 50% (`Dice(2)`)、超回避Lv「n」 は `n >= Dice(10)` ≈ 10n%。
    /// 攻撃側 `直撃` / 防御側の行動不能系状態では発動しない。命中ロールが当たった
    /// (would-hit) ときだけ呼ぶこと (SRC 同様・無駄な乱数消費を避ける)。
    /// 予測 (プレビュー) には反映しない＝SRC の HitProbability も 分身/超回避 を畳み込まない。
    pub(super) fn check_dodge_feature(
        &mut self,
        def_idx: usize,
        attacker_idx: usize,
        _weapon_class: &str,
    ) -> bool {
        // 直撃 (攻撃側) で回避無効。
        if self.database.unit_instances[attacker_idx].has_condition("直撃") {
            return false;
        }
        let (morale, has_bunshin, chougaihi_lv) = {
            let u = &self.database.unit_instances[def_idx];
            // 能動防御は行動可能でないと発動しない。
            if u.has_condition("行動不能")
                || u.has_condition("麻痺")
                || u.has_condition("石化")
                || u.has_condition("凍結")
                || u.has_condition("睡眠")
            {
                return false;
            }
            (
                u.morale,
                crate::feature::has_feature(&u.active_features, "分身"),
                crate::feature::feature_level(&u.active_features, "超回避").unwrap_or(0),
            )
        };
        // 分身: 気力130以上で 50% (Dice(2))。
        if has_bunshin && morale >= 130 && (self.next_u32() % 2) == 0 {
            self.push_message("分身: 攻撃を回避！".to_string());
            return true;
        }
        // 超回避Lv*: Lv >= Dice(10) ≈ 10×Lv%。
        if chougaihi_lv > 0 {
            let roll = (self.next_u32() % 10) as i32 + 1;
            if chougaihi_lv >= roll {
                self.push_message("超回避: 攻撃を回避！".to_string());
                return true;
            }
        }
        false
    }

    /// 被弾して生存した `target_idx` に対し `損傷率 <unit> <pct>:` 閾値イベントを
    /// 発火する。`old_dmg` はこの被弾前の累積ダメージ。撃破された (除去済み) 場合は
    /// 呼ばない。通常攻撃・援護攻撃・援護防御・反撃の被弾生存経路で共通に使う。
    pub(super) fn fire_damage_threshold_for_unit(
        &mut self,
        target_idx: usize,
        max_hp: i64,
        old_dmg: i64,
    ) {
        let (pilot_name, unit_data_name, party) = {
            let u = &self.database.unit_instances[target_idx];
            (u.pilot_name.clone(), u.unit_data_name.clone(), u.party)
        };
        let new_dmg = self.database.unit_instances[target_idx].damage;
        crate::event_runtime::fire_damage_threshold_labels(
            self,
            &pilot_name,
            &unit_data_name,
            party,
            old_dmg,
            new_dmg,
            max_hp,
        );
    }

    /// `atk_idx` のユニットが (def_x, def_y) を攻撃した直後、同陣営の隣接ユニットで
    /// サポートアタック特殊能力を持つものから 1 体追撃。残り回数を 1 消費。
    /// 命中・ダメージは `predict_with_status` のダメージを 75% に減衰させる。
    /// 戻り値: 援護攻撃を実施したユニットの UID / サポートアタック不能なら `None`。
    pub(super) fn try_support_attack(
        &mut self,
        atk_idx: usize,
        def_x: u32,
        def_y: u32,
        def_mode: &str,
    ) -> Option<String> {
        let atk_party = self.database.unit_instances[atk_idx].party;
        let atk_pos = (
            self.database.unit_instances[atk_idx].x,
            self.database.unit_instances[atk_idx].y,
        );
        // 隣接した同盟陣営 (味方↔ＮＰＣ 含む) でサポートアタック可能なユニットを探す
        let supporter_idx = self
            .database
            .unit_instances
            .iter()
            .enumerate()
            .find(|(i, u)| {
                *i != atk_idx
                    && u.party.is_ally_of(atk_party)
                    && u.support_attack_remaining > 0
                    && Self::is_adjacent((u.x, u.y), atk_pos)
                    && u.conditions
                        .iter()
                        .any(|c| c.name.contains("サポートアタック") || c.name.contains("援護攻撃"))
            })
            .map(|(i, _)| i);
        let sup_idx = supporter_idx?;

        let def_idx = self
            .database
            .unit_instances
            .iter()
            .position(|u| u.x == def_x && u.y == def_y)?;

        // 援護攻撃を行うユニットの UID を退避 (戻り値用)
        let sup_uid = self.database.unit_instances[sup_idx].uid.clone();

        let (sup_pilot, sup_unit) = self.database.effective_combat_data(sup_idx)?;
        let dist = combat::manhattan(
            (
                self.database.unit_instances[sup_idx].x,
                self.database.unit_instances[sup_idx].y,
            ),
            (def_x, def_y),
        );
        let weapon = self.best_firable_weapon_in_range(sup_idx, &sup_unit, dist)?;
        // 援護武器の EN・残弾消費 (撃破前=index 失効前に消費)。
        if let Some(wi) = sup_unit.weapons.iter().position(|w| w.name == weapon.name) {
            self.consume_weapon_resources(sup_idx, wi);
        }
        let def_inst = self.database.unit_instances[def_idx].clone();
        let (def_pilot, mut def_unit) = self.database.effective_combat_data(def_idx)?;
        // 防御特性 (弱点→装甲半減 / 吸収→無視) を援護攻撃でも一貫適用。
        self.apply_defense_armor_mod(def_idx, &weapon.class, &mut def_unit);
        let terrain_id = self
            .database
            .map
            .as_ref()
            .map(|m| m.cell(def_inst.x, def_inst.y).terrain_id)
            .unwrap_or(0);
        let def_hit_mod = self.database.terrain_hit_mod(terrain_id);
        let def_damage_mod = self.database.terrain_damage_mod(terrain_id);
        // 命中・クリティカル・即死・切り払い・防御特性は本線攻撃と同じ判定 (原典は
        // `Attack w2, target, "援護攻撃", def_mode`)。防御側の反撃モードも本線攻撃と共通。
        let strike = self.resolve_strike(
            sup_idx,
            def_idx,
            &StrikeSetup {
                atk_pilot: &sup_pilot,
                atk_unit: &sup_unit,
                weapon: &weapon,
                def_pilot: &def_pilot,
                def_unit: &def_unit,
                def_inst: &def_inst,
                def_hit_mod,
                def_damage_mod,
            },
            def_mode,
            StrikeKind::Support,
        );
        // SP コスト消費 (= サポートアタック回数を 1 減らす)
        self.database.unit_instances[sup_idx].support_attack_remaining -= 1;
        if strike.hit {
            let dmg = strike.damage;
            // みがわり (身代わり, 最優先・SRC Unit.cls:14855): 防御側が みがわり を持てば
            // 身代わりが肩代わりし、防御側への被ダメージは 0 になる。
            let dmg = if let Some(sub) = self.apply_migawari(def_idx, sup_idx, &weapon.class, dmg) {
                self.push_message(format!(
                    "みがわり: {} が {} の身代わりになった！",
                    sub, def_pilot.nickname
                ));
                0
            } else {
                // バリア/アクティブフィールド/シールド吸収 → 不死身キャップ。
                let dmg = self.apply_barrier_absorb(def_idx, sup_idx, &weapon.class, dmg);
                let dmg = self.apply_active_field_absorb(def_idx, sup_idx, &weapon.class, dmg);
                let dmg = self.apply_shield_defense(def_idx, &weapon.class, dmg);
                self.cap_damage_for_immortal(def_idx, def_unit.hp, dmg)
            };
            let def_old_dmg = self.database.unit_instances[def_idx].damage;
            self.database.unit_instances[def_idx].damage += dmg;
            self.push_message(format!(
                "{} のサポートアタック [{}] → {}: {} ダメージ{}",
                sup_pilot.nickname,
                weapon.name,
                def_pilot.nickname,
                dmg,
                if strike.critical {
                    " (クリティカル)"
                } else {
                    ""
                }
            ));
            let remaining = def_unit.hp - self.database.unit_instances[def_idx].damage;
            if remaining <= 0 && !self.revive_if_possible(def_idx) {
                // 援護攻撃側 (sup) が撃破 → 報酬は援護側へ。除去前に位置を退避。
                let (sx, sy) = (
                    self.database.unit_instances[sup_idx].x,
                    self.database.unit_instances[sup_idx].y,
                );
                let doubled = self.database.unit_instances[sup_idx].has_condition("努力");
                let exp = def_pilot.exp_value * if doubled { 2 } else { 1 };
                let victim_value = def_unit.value;
                let victim_party = self.database.unit_instances[def_idx].party;
                self.database.remove_unit_at(def_idx);
                self.fire_destruction_label(&def_pilot.name, &def_unit.name, victim_party);
                if let Some(killer_idx) = self
                    .database
                    .unit_instances
                    .iter()
                    .position(|u| u.x == sx && u.y == sy)
                {
                    let money =
                        self.award_kill_rewards(killer_idx, exp, victim_value, victim_party);
                    if money > 0 {
                        self.push_message(format!("  援護撃破: 資金 +{money}"));
                    }
                }
            } else if remaining > 0 {
                // 損傷率イベント (援護攻撃で HP が閾値を割った場合)。
                self.fire_damage_threshold_for_unit(def_idx, def_unit.hp, def_old_dmg);
                // 攻撃を受けて生存したユニットは気力 +1 (SRC: 損傷による気力上昇)。
                {
                    let dm = &mut self.database.unit_instances[def_idx];
                    dm.morale = (dm.morale + 1).clamp(0, 150);
                }
                // 命中時/損傷時 気力増加スキル。
                self.apply_combat_morale_skills(sup_idx, def_idx, true);
                // 生存: 援護武器の特殊効果攻撃属性 (状態異常付与) を防御側へ proc。
                let inflicted = self.apply_weapon_special_effects(
                    def_idx, sup_idx, &weapon, &sup_pilot, &def_pilot,
                );
                if !inflicted.is_empty() {
                    self.push_message(format!("  → {}", inflicted.join("・")));
                }
            }
        } else {
            // 失敗時/回避時 気力増加スキル。
            self.apply_combat_morale_skills(sup_idx, def_idx, false);
            self.push_message(format!("{} のサポートアタックはミス", sup_pilot.nickname));
        }
        Some(sup_uid)
    }

    pub(super) fn is_adjacent(a: (u32, u32), b: (u32, u32)) -> bool {
        let dx = (a.0 as i64 - b.0 as i64).abs();
        let dy = (a.1 as i64 - b.1 as i64).abs();
        dx + dy == 1
    }

    /// 援護防御ユニット検索: `def_idx` のユニットが `expected_damage` のダメージを
    /// 受けようとしている際、代わりにダメージを受けてくれる隣接ユニットを探す。
    ///
    /// SRC `Unit.cs::LookForSupportGuard` に準拠。
    /// 戻り値: 援護防御を行うユニットのインデックス / 該当なしなら `None`。
    pub(super) fn try_find_support_guard(
        &self,
        def_idx: usize,
        expected_damage: i64,
    ) -> Option<usize> {
        let def_inst = &self.database.unit_instances[def_idx];
        let def_pos = (def_inst.x, def_inst.y);
        let def_party = def_inst.party;
        // 最大 HP は改造・アイテム・BossRank 込みの実効値 (戦闘本体と同じ基準)。
        self.database.unit_by_name(&def_inst.unit_data_name)?;
        let def_max_hp = self.database.effective_max_hp(def_inst);
        let def_current_hp = def_max_hp - def_inst.damage;

        // ダメージが最小閾値 (MaxHP の 5% or 現 HP の 20%) を下回る場合は発動しない。
        // (SRC: `my_dmg < MaxHP / 20 & my_dmg < HP / 5` → 両方以下なら skip)
        if expected_damage < def_max_hp / 20 && expected_damage < def_current_hp / 5 {
            return None;
        }

        let mut best: Option<(usize, i64)> = None; // (index, current_hp)
        for (i, u) in self.database.unit_instances.iter().enumerate() {
            if i == def_idx {
                continue;
            }
            // 援護できるのは同盟陣営 (同陣営 + 味方↔ＮＰＣ)。
            if !u.party.is_ally_of(def_party) {
                continue;
            }
            // 隣接していること
            if !Self::is_adjacent((u.x, u.y), def_pos) {
                continue;
            }
            // 援護防御能力を持つこと ("サポートガード"/"援護防御"/"援護" のいずれか)
            let has_guard_ability = u.conditions.iter().any(|c| {
                c.name.contains("サポートガード") || c.name.contains("援護防御") || c.name == "援護"
            }) || u.active_features.iter().any(|f| {
                f.is_active
                    && (f.name.contains("サポートガード")
                        || f.name.contains("援護防御")
                        || f.name == "援護")
            });
            if !has_guard_ability {
                continue;
            }
            // 援護防御回数が残っていること
            if u.support_guard_remaining <= 0 {
                continue;
            }
            // 混乱/暴走/恐怖/狂戦士状態でないこと
            if u.has_condition("混乱")
                || u.has_condition("暴走")
                || u.has_condition("恐怖")
                || u.has_condition("狂戦士")
            {
                continue;
            }
            // 代わりに受けても撃破されないこと (50% ダメージで HP が残る)
            if self.database.unit_by_name(&u.unit_data_name).is_none() {
                continue;
            }
            let guard_current_hp = self.database.effective_max_hp(u) - u.damage;
            let guard_dmg = expected_damage / 2; // 防御扱い = 50%
            if guard_dmg >= guard_current_hp {
                // 倒れてしまうのでかばわない
                continue;
            }
            // HP が最も高いユニットを優先
            if let Some((_, best_hp)) = best {
                if guard_current_hp <= best_hp {
                    continue;
                }
            }
            best = Some((i, guard_current_hp));
        }
        best.map(|(i, _)| i)
    }

    /// 反撃メニューに「援護防御」選択肢を出すべきか (read-only)。`def_uid` の隣接に
    /// 援護防御可能な同盟ユニット (能力あり・残回数 > 0・混乱/暴走/恐怖/狂戦士でない) が
    /// いれば真。ダメージ依存の閾値/生存判定は解決時 (`try_find_support_guard`) に委ねる
    /// ため、ここでは省く (選んでも条件を満たさなければ不発になるだけ)。
    pub(super) fn support_guard_available(&self, def_uid: &str) -> bool {
        let Some(def) = self.database.unit_by_uid(def_uid) else {
            return false;
        };
        let def_pos = (def.x, def.y);
        let def_party = def.party;
        self.database.unit_instances.iter().any(|u| {
            u.uid != def_uid
                && u.party.is_ally_of(def_party)
                && !u.off_map
                && Self::is_adjacent((u.x, u.y), def_pos)
                && u.support_guard_remaining > 0
                && (u.conditions.iter().any(|c| {
                    c.name.contains("サポートガード")
                        || c.name.contains("援護防御")
                        || c.name == "援護"
                }) || u.active_features.iter().any(|f| {
                    f.is_active
                        && (f.name.contains("サポートガード")
                            || f.name.contains("援護防御")
                            || f.name == "援護")
                }))
                && !u.has_condition("混乱")
                && !u.has_condition("暴走")
                && !u.has_condition("恐怖")
                && !u.has_condition("狂戦士")
        })
    }

    /// 撃破時に自動発火するラベルを試行。最初に見つかったものを 1 つだけ投函。
    /// 章ローカルイベントなので現ステージファイルにスコープする
    /// (`post_stage_event_label`)。スクリプト実行中なら完了後に実行される。
    /// 戦闘でユニットを撃破したときの破壊イベント発火。SRC `破壊 <name>` (破壊
    /// イベント) と、該当陣営が全滅していれば `全滅 <party>` (全滅イベント) を発火する。
    ///
    /// 旧実装は誤って `撃破 <name>` ラベル (原典に存在しない綴) を発火し、かつ全滅
    /// イベントを一切発火しなかったため、**戦闘で最後の敵を撃破してもシナリオの
    /// `破壊 <name>` / `全滅 敵 → クリア` が走らず進行不能** だった (実機報告: ターンだけ
    /// 増えて撃破しても何も起こらない)。`.eve` `Kill`/`Damage` 経路で使われている
    /// 正しい `fire_destruction_labels` (破壊 + 全滅) に委譲して一本化する。
    pub(super) fn fire_destruction_label(&mut self, pilot: &str, unit: &str, party: crate::Party) {
        crate::event_runtime::fire_destruction_labels(self, pilot, unit, party);
    }

    /// 先制攻撃 (SRC `COM.bas:1040-1058`): 攻撃側が `def_pos` の防御側を攻撃する際、
    /// 防御側が反撃武器を射程内に持ち、以下のいずれかで先手を取れるなら主攻撃の**前に**
    /// 防御側が反撃する。成立条件 (いずれか): 攻撃側武器が `後`(後攻) / 防御側反撃武器が
    /// `先`(先制) 属性、`カウンター` SP (condition)、`先読み` 技能 `Lv>=Dice(16)`、`カウンター`
    /// 技能の使用回数残あり (`used<Lv`、使うたび加算)。成立したら `true` (= 主攻撃後の通常反撃を
    /// 抑止)。乱数は `先読み` 技能保持時のみ消費する (能力非保持ユニットの RNG 列を保つ)。
    pub(super) fn try_preemptive_counter(
        &mut self,
        def_idx: usize,
        atk_pos: (u32, u32),
        def_pos: (u32, u32),
        atk_weapon_class: &str,
    ) -> bool {
        if self.database.unit_instances[def_idx].attack_disabled() {
            return false;
        }
        let Some(atk_idx) = self
            .database
            .unit_instances
            .iter()
            .position(|u| u.x == atk_pos.0 && u.y == atk_pos.1)
        else {
            return false;
        };
        if !self.database.unit_instances[def_idx]
            .party
            .is_hostile_to(self.database.unit_instances[atk_idx].party)
        {
            return false;
        }
        // 反撃武器が射程内にあるか。
        let dist = combat::manhattan(def_pos, atk_pos);
        let Some((_, def_unit)) = self.database.effective_combat_data(def_idx) else {
            return false;
        };
        let Some(cw) = self.best_firable_weapon_in_range(def_idx, &def_unit, dist) else {
            return false;
        };
        let counter_lv = self.unit_pilot_skill_level(def_idx, "カウンター");
        let foresight_lv = self.unit_pilot_skill_level(def_idx, "先読み");
        let used = self.database.unit_instances[def_idx].used_counter_attack;
        let has_counter_sp = self.database.unit_instances[def_idx].has_condition("カウンター");
        // SRC COM.bas の判定順: 後/先/カウンターSP/先読み(Dice) → カウンター技能(回数消費)。
        // 先読み の Dice は技能保持時のみ評価 (`||` 短絡で RNG 列を保つ)。
        let preempt = if atk_weapon_class.contains('後')
            || cw.class.contains('先')
            || has_counter_sp
            || (foresight_lv > 0 && foresight_lv > (self.next_u32() % 16) as i32)
        {
            true
        } else if counter_lv > used {
            self.database.unit_instances[def_idx].used_counter_attack = used + 1;
            true
        } else {
            false
        };
        if !preempt {
            return false;
        }
        self.push_message(format!(
            "{} の先制攻撃！",
            self.database
                .pilot_by_name(&self.database.unit_instances[def_idx].pilot_name)
                .map(|p| p.nickname.clone())
                .unwrap_or_else(|| self.database.unit_instances[def_idx].pilot_name.clone())
        ));
        self.try_counterattack(def_pos.0, def_pos.1, atk_pos);
        true
    }

    /// `(dx, dy)` の防御側ユニットが `target` へ反撃可能なら 1 度だけ実行。
    /// 命中・ダメージの判定は本線攻撃と同じ `resolve_strike`。
    /// 戻り値: 反撃に使用した武器の `(名前, 1-based 番号)` / 反撃不能なら `None`。
    pub(super) fn try_counterattack(
        &mut self,
        dx: u32,
        dy: u32,
        target: (u32, u32),
    ) -> Option<(String, usize)> {
        let def_idx = self
            .database
            .unit_instances
            .iter()
            .position(|u| u.x == dx && u.y == dy)?;
        let atk_idx = self
            .database
            .unit_instances
            .iter()
            .position(|u| u.x == target.0 && u.y == target.1)?;
        // 反撃が成立するのは敵対関係のときのみ (同盟同士は反撃しない)。
        if !self.database.unit_instances[def_idx]
            .party
            .is_hostile_to(self.database.unit_instances[atk_idx].party)
        {
            return None;
        }
        // 行動不能 (麻痺/混乱/睡眠/捕縛 等) のユニットは反撃できない。SRC
        // `MaxAction()==0` ゲート相当。手動/自動/ヘッドレス全経路で一元的に抑止する。
        if self.database.unit_instances[def_idx].attack_disabled() {
            return None;
        }
        // 反撃側 = defender、被弾側 = attacker。双方とも実効値込みのデータを使う。
        let (def_pilot, def_unit) = self.database.effective_combat_data(def_idx)?;
        let (atk_pilot, mut atk_unit) = self.database.effective_combat_data(atk_idx)?;
        let dist = combat::manhattan((dx, dy), target);
        // プレイヤーが反撃武器選択で武器を指定していれば優先 (1 回限り。射程内 + 発射可能
        // でなければ自動選定へフォールバック)。
        let forced = self.forced_counter_weapon.take().and_then(|idx| {
            def_unit
                .weapons
                .get(idx)
                .filter(|w| self.weapon_usable_vs(def_idx, w, dist, false))
                .cloned()
        });
        let weapon = match forced {
            Some(w) => w,
            None => self.best_firable_weapon_in_range(def_idx, &def_unit, dist)?,
        };
        // 反撃の被弾側 (= 元攻撃者 atk_idx) の防御特性 (弱点→装甲半減 / 吸収→無視) を適用。
        self.apply_defense_armor_mod(atk_idx, &weapon.class, &mut atk_unit);
        let t_id = self
            .database
            .map
            .as_ref()
            .map(|m| m.cell(target.0, target.1).terrain_id)?;
        let atk_hit_mod = self.database.terrain_hit_mod(t_id);
        let atk_damage_mod = self.database.terrain_damage_mod(t_id);
        // 反撃側 = defender (def_idx)、被弾側 = attacker (atk_idx)。
        let atk_inst = self.database.unit_instances[atk_idx].clone();
        // 反撃武器の 1-based インデックスを取得
        let weapon_num = def_unit
            .weapons
            .iter()
            .position(|w| w.name == weapon.name)
            .map(|i| i + 1)
            .unwrap_or(1);
        let weapon_name = weapon.name.clone();
        // 反撃武器の EN・残弾消費 (撃破前=index 失効前に消費)。
        self.consume_weapon_resources(def_idx, weapon_num - 1);
        // 命中・クリティカル・即死・切り払い・防御特性は本線攻撃と同じ判定 (原典の反撃は
        // `Attack w, target, "", ""`)。反撃側 = defender (def_idx)、被弾側 = attacker (atk_idx)。
        let strike = self.resolve_strike(
            def_idx,
            atk_idx,
            &StrikeSetup {
                atk_pilot: &def_pilot,
                atk_unit: &def_unit,
                weapon: &weapon,
                def_pilot: &atk_pilot,
                def_unit: &atk_unit,
                def_inst: &atk_inst,
                def_hit_mod: atk_hit_mod,
                def_damage_mod: atk_damage_mod,
            },
            "",
            StrikeKind::Counter,
        );
        let msg = if strike.hit {
            let counter_dmg = strike.damage;
            // みがわり (身代わり, 最優先・SRC Unit.cls:14855): 反撃を受ける側 (atk_idx) が
            // みがわり を持てば身代わりが肩代わりし、被ダメージは 0 になる。
            let counter_dmg = if let Some(sub) =
                self.apply_migawari(atk_idx, def_idx, &weapon.class, counter_dmg)
            {
                self.push_message(format!(
                    "みがわり: {} が {} の身代わりになった！",
                    sub, atk_unit.name
                ));
                0
            } else {
                // バリア/アクティブフィールド/シールド吸収 → 不死身キャップ
                // (反撃の攻撃側は def_idx)。
                let d = self.apply_barrier_absorb(atk_idx, def_idx, &weapon.class, counter_dmg);
                let d = self.apply_active_field_absorb(atk_idx, def_idx, &weapon.class, d);
                let d = self.apply_shield_defense(atk_idx, &weapon.class, d);
                self.cap_damage_for_immortal(atk_idx, atk_unit.hp, d)
            };
            let atk_old_dmg = self.database.unit_instances[atk_idx].damage;
            self.database.unit_instances[atk_idx].damage += counter_dmg;
            let remaining = atk_unit.hp - self.database.unit_instances[atk_idx].damage;
            if remaining <= 0 && self.revive_if_possible(atk_idx) {
                format!(
                    "  ↩ 反撃: {} → {} [{}]: 撃破… だが【復活】！",
                    def_pilot.nickname, atk_pilot.nickname, weapon.name
                )
            } else if remaining <= 0 {
                // 反撃側 (def, 位置 dx,dy) が撃破 → 報酬は反撃側へ。
                let doubled = self.database.unit_instances[def_idx].has_condition("努力");
                let exp = atk_pilot.exp_value * if doubled { 2 } else { 1 };
                let victim_value = atk_unit.value;
                let victim_party = self.database.unit_instances[atk_idx].party;
                let mut m = format!(
                    "  ↩ 反撃: {} → {} [{}]: 撃破！",
                    def_pilot.nickname, atk_pilot.nickname, weapon.name
                );
                self.database.remove_unit_at(atk_idx);
                self.fire_destruction_label(&atk_pilot.name, &atk_unit.name, victim_party);
                if let Some(killer_idx) = self
                    .database
                    .unit_instances
                    .iter()
                    .position(|u| u.x == dx && u.y == dy)
                {
                    let money =
                        self.award_kill_rewards(killer_idx, exp, victim_value, victim_party);
                    if money > 0 {
                        m.push_str(&format!(" 資金 +{money}"));
                    }
                }
                m
            } else {
                // 損傷率イベント (反撃で HP が閾値を割った場合)。
                self.fire_damage_threshold_for_unit(atk_idx, atk_unit.hp, atk_old_dmg);
                // 反撃を受けて生存した攻撃側は気力 +1 (SRC: 損傷による気力上昇)。
                {
                    let am = &mut self.database.unit_instances[atk_idx];
                    am.morale = (am.morale + 1).clamp(0, 150);
                }
                // 命中時/損傷時 気力増加スキル (反撃側=def が命中、被弾側=atk が損傷)。
                self.apply_combat_morale_skills(def_idx, atk_idx, true);
                // 生存: 反撃武器の特殊効果攻撃属性 (状態異常付与) を被弾側へ proc。
                // 反撃側 = def (atk_pilot=def_pilot)、被弾側 = atk (def_pilot=atk_pilot)。
                let inflicted = self.apply_weapon_special_effects(
                    atk_idx, def_idx, &weapon, &def_pilot, &atk_pilot,
                );
                let mut m = format!(
                    "  ↩ 反撃: {} → {} [{}]: 命中 {} ダメージ (残HP {}){}",
                    def_pilot.nickname,
                    atk_pilot.nickname,
                    weapon.name,
                    counter_dmg,
                    remaining,
                    if strike.critical {
                        " (クリティカル)"
                    } else {
                        ""
                    }
                );
                if !inflicted.is_empty() {
                    m.push_str(&format!(" → {}", inflicted.join("・")));
                }
                m
            }
        } else {
            // 失敗時/回避時 気力増加スキル (反撃側=def が失敗、被弾側=atk が回避)。
            self.apply_combat_morale_skills(def_idx, atk_idx, false);
            format!(
                "  ↩ 反撃: {} → {} [{}]: ミス",
                def_pilot.nickname, atk_pilot.nickname, weapon.name
            )
        };
        self.push_message(msg);
        Some((weapon_name, weapon_num))
    }

    /// 戦闘中に勝敗を判定。Player 側ユニットが居なくなれば敗北、
    /// 対立勢力 (Enemy) が居なくなれば勝利。両方とも 0 件の場合は判定しない
    /// （まだユニットが配置されていない / すでに判定済み等）。
    ///
    /// 勝敗確定時は `game_clear()` / `game_over()` を呼び、それぞれ
    /// `Victory`/`勝利` / `GameOver`/`ゲームオーバー` ラベルを自動発火する。
    /// `(x, y)` の地形に対応する地形適応の環境インデックス (0=空/1=陸/2=海/3=宇)。
    /// 戦闘ダメージの地形適応 ([`crate::combat::predict_with_status_terrain`]) に渡す。
    /// マップ未ロード / 範囲外は -1 (地形適応なし=×1.0)。地形未定義は陸 (1) 扱い。
    pub fn terrain_env_at(&self, x: u32, y: u32) -> i32 {
        let Some(map) = self.database.map.as_ref() else {
            return -1;
        };
        if x >= map.width || y >= map.height {
            return -1;
        }
        let tid = map.cell(x, y).terrain_id;
        crate::data::terrain::lookup(tid)
            .map(|t| crate::combat::terrain_env(t.class))
            .unwrap_or(1)
    }

    /// `(x, y)` の地形の防御価値。値が大きいほど守りやすい (被ダメージ軽減 + 回避上昇)。
    /// 戦闘では `damage_mod` が正値ほど被ダメージを減らし (`(100 - damage_mod)/100`)、
    /// `hit_mod` も**正値ほど被命中を下げる** (`(100 - hit_mod)/100`、SRC 規約) ため、両者を
    /// 合わせた `damage_mod + hit_mod` を指標とする (平地=0 / 森林=15 / 山=25 / 都市=35)。
    /// マップ未ロード / 範囲外は 0 (中立)。敵 AI の防御地形選好に使う。
    pub(super) fn tile_defensive_value(&self, x: u32, y: u32) -> i32 {
        let Some(map) = self.database.map.as_ref() else {
            return 0;
        };
        if x >= map.width || y >= map.height {
            return 0;
        }
        let tid = map.cell(x, y).terrain_id;
        self.database.terrain_damage_mod(tid) + self.database.terrain_hit_mod(tid)
    }

    /// `party` に敵対する勢力のオンマップユニットがマップ兵器 (武器 class に全角Ｍ) を
    /// 1 つでも持つか。敵 AI の散開判断 (マップ兵器回避) に使う。マップ兵器を持つ敵が
    /// 居なければ散開しない (= 通常の攻撃位置選択)。
    pub(super) fn hostile_has_map_weapon(&self, party: crate::Party) -> bool {
        self.database.unit_instances.iter().any(|u| {
            !u.off_map
                && u.party.is_hostile_to(party)
                && self
                    .database
                    .unit_by_name(&u.unit_data_name)
                    .is_some_and(|d| d.weapons.iter().any(|w| w.class.contains('Ｍ')))
        })
    }
}
