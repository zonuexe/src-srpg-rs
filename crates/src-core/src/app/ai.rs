//! 敵/中立/ＮＰＣ フェイズの AI 行動選択と逐次実行。

use super::*;

impl App {
    /// 逐次演出モードで `phase`(敵/中立/ＮＰＣ) を開始し、`ai_runner` の行動キューを
    /// 構築する。`begin_phase` がターンイベント(Talk 等)を立てた場合は modal で
    /// ブロックされ、`tick` 側がそれを待ってからステップする。
    pub(super) fn begin_ai_phase(&mut self, phase: crate::Phase) {
        // 勝敗確定後は新しいフェイズを開始しない (敗北後に中立/敵フェイズが
        // 走り続けるのを防ぐ)。
        if matches!(
            self.stage_state,
            crate::stage::StageState::Victory | crate::stage::StageState::Defeat
        ) {
            self.ai_runner = None;
            return;
        }
        self.begin_phase(phase);
        let party = phase.party();
        let queue: std::collections::VecDeque<String> = self
            .database
            .unit_instances
            .iter()
            .filter(|u| u.party == party && !u.off_map)
            .map(|u| u.uid.clone())
            .collect();
        self.ai_runner = Some(AiRunner {
            queue,
            step_timer: AI_STEP_SECS,
        });
    }

    /// 逐次 AI ランナーを `dt` 秒分進める。`tick` から呼ばれる。返り値は再描画要否。
    /// modal(ダイアログ/反撃プロンプト)・スクリプト Wait 中は進めない。
    pub(super) fn ai_runner_tick(&mut self, dt: f64) -> bool {
        if self.ai_runner.is_none() {
            return false;
        }
        // 勝敗が確定したら AI / フェイズ進行を即停止する。Defeat/Victory は
        // ユニット撃破 (check_victory) だけでなく、シナリオの `全滅 味方` →
        // `GameOver` イベント (戦闘演出後に EventQue 経由で発火) など別経路でも
        // 立つため、各 tick 冒頭で必ず確認する。これをしないと味方全滅後も
        // 中立/敵フェイズが動き続け「敗北したのに進行する」状態になる。
        if matches!(
            self.stage_state,
            crate::stage::StageState::Victory | crate::stage::StageState::Defeat
        ) {
            self.ai_runner = None;
            return false;
        }
        // modal (ターンイベントの Talk / 反撃プロンプト) や Wait タイマ中は待機。
        if self.pending_dialog.is_some() || self.pending_timer.is_some() {
            return false;
        }
        // 移動/戦闘演出の再生中は次の 1 体を動かさず、演出を見せきってから進める。
        if self.battle_anim.is_some() || self.move_anim.is_some() {
            return false;
        }
        // 「間」のカウントダウン。
        {
            let runner = self.ai_runner.as_mut().unwrap();
            runner.step_timer -= dt;
            if runner.step_timer > 0.0 {
                return false;
            }
            runner.step_timer = AI_STEP_SECS;
        }
        // 次に行動する有効なユニットを探す (撃破/退避/行動済みはスキップ)。
        let phase_party = self.turn.phase.party();
        let mut act_uid: Option<String> = None;
        while let Some(uid) = self.ai_runner.as_mut().unwrap().queue.pop_front() {
            if let Some(u) = self.database.unit_by_uid(&uid) {
                if !u.off_map && u.party == phase_party && !u.has_acted {
                    act_uid = Some(uid);
                    break;
                }
            }
        }
        if let Some(uid) = act_uid {
            // カメラを行動ユニットへ寄せて 1 体実行 (移動→攻撃)。
            if let Some(u) = self.database.unit_by_uid(&uid) {
                self.map_cursor = Some((u.x, u.y));
                self.ensure_cursor_visible();
            }
            if let Some(idx) = self.database.idx_by_uid(&uid) {
                self.ai_act_unit(idx);
            }
            self.check_victory();
            if matches!(
                self.stage_state,
                crate::stage::StageState::Victory | crate::stage::StageState::Defeat
            ) {
                self.ai_runner = None;
            }
            return true;
        }
        // このフェイズのキューが空 → 次フェイズへ。ＮＰＣ→Player でラップ時 turn++。
        let next = self.turn.phase.next();
        if next == crate::Phase::Player {
            self.turn.number = self.turn.number.saturating_add(1);
            self.begin_phase(crate::Phase::Player);
            self.ai_runner = None; // プレイヤーのターンへ。
        } else {
            self.begin_ai_phase(next);
        }
        true
    }

    /// 現フェーズの所属ユニットを順に動かす AI:
    ///
    /// 各ユニットについて
    /// 1. ターゲット候補: 対立勢力ユニットを「HP 残量 (低い方優先)」「マンハッタン
    ///    距離 (近い方優先)」「経験値 (高い方優先)」の複合スコアでランク付け。
    /// 2. movement::compute_range_with でユニットの移動範囲を Dijkstra で計算。
    /// 3. ベストターゲットへ「攻撃可能な距離 = 任意武器の最大射程」を満たす
    ///    位置を移動範囲から選び、最短経路の一手を実際の位置として採用。
    /// 4. 移動後、AttackTarget を呼んで攻撃。
    pub(super) fn run_ai_phase(&mut self) {
        let party = self.turn.phase.party();
        let positions: Vec<(u32, u32)> = self
            .database
            .unit_instances
            .iter()
            .filter(|u| u.party == party && !u.off_map)
            .map(|u| (u.x, u.y))
            .collect();
        let map_dims = match self.database.map.as_ref() {
            Some(m) => Some((m.width, m.height)),
            None => return,
        };
        let _ = map_dims;

        for (sx, sy) in positions {
            // 最新のインデックスを引き直す（前手の攻撃で削除されている可能性）
            let idx = self
                .database
                .unit_instances
                .iter()
                .position(|u| u.x == sx && u.y == sy && u.party == party);
            let Some(idx) = idx else {
                continue;
            };

            self.ai_act_unit(idx);
        }
    }

    /// 単一ユニットの AI 行動 1 手。run_ai_phase の内部ループから呼ばれる。
    pub(super) fn ai_act_unit(&mut self, idx: usize) {
        // 行動不能 (麻痺/睡眠/混乱/捕縛/凍結/石化 等、AttackDisabled を持つ状態異常) の
        // ユニットは行動せずフェイズを消費する。特殊効果攻撃属性の状態異常が AI に効く。
        if self.database.unit_instances[idx].attack_disabled() {
            self.database.unit_instances[idx].has_acted = true;
            return;
        }
        let party = self.database.unit_instances[idx].party;
        let unit_pos = (
            self.database.unit_instances[idx].x,
            self.database.unit_instances[idx].y,
        );
        let unit_data_name = self.database.unit_instances[idx].unit_data_name.clone();
        // SRC `ChangeMode` で設定された思考モードを反映。
        // `待機` / `固定` のユニットは敵が射程内に来るまで動かない。
        // 本実装はターン中に再評価される最小ロジック:
        //   - `固定` → 完全静止 (移動なし、攻撃なし)
        //   - `待機` → 移動なし、ただし攻撃可能ターゲットがいれば攻撃
        let ai_mode = self.database.unit_instances[idx].ai_mode.clone();
        let mode_keeps_position = matches!(ai_mode.as_str(), "待機" | "固定");
        let mode_no_attack = ai_mode == "固定";
        if mode_no_attack {
            self.database.unit_instances[idx].has_acted = true;
            return;
        }
        // 支援精神 (回復系) を持つ AI は、半分以下まで負傷した味方が居れば回復する。
        // 精神は行動を消費しないため、この後も通常の移動・攻撃を続ける。
        self.ai_use_support_spirit(idx);
        // 瀕死の自ユニットは撃破に備えて「復活」精神を事前発動する (free action)。
        self.ai_use_revive_spirit(idx);
        // 回復/補給アビリティを持つ AI は、射程内に支援を要する味方が居れば優先する。
        if self.ai_use_support_ability(idx) {
            return;
        }
        // 敵対象アビリティ (脱=気力減少 等) を持つ AI は、射程内に敵が居れば使う。
        if self.ai_use_offensive_ability(idx) {
            return;
        }
        // 召喚アビリティを持つ AI は、数的に劣勢なら増援を召喚する。
        if self.ai_use_summon_ability(idx) {
            return;
        }
        // マップ兵器を持つ AI は、2 体以上の敵を巻き込める照準があれば優先発射する。
        if self.ai_use_map_weapon(idx) {
            return;
        }
        // 自ユニットの最大射程 (= 攻撃移動目標距離)
        let max_range = self
            .database
            .unit_by_name(&unit_data_name)
            .map(|u| u.weapons.iter().map(|w| w.max_range).max().unwrap_or(1))
            .unwrap_or(1) as u32;
        let speed = self
            .database
            .unit_by_name(&unit_data_name)
            .map(|u| u.speed)
            .unwrap_or(3);

        let unit_data = self.database.unit_by_name(&unit_data_name).unwrap();
        // 攻撃側の実効戦闘データ (育成 / 強化パーツ / 状態異常 込み) を一度だけ算出。
        let atk_combat = self.database.effective_combat_data(idx);

        // ターゲット候補をスコア付けして並べ替え
        let mut candidates: Vec<((u32, u32), i64, i64)> = self
            .database
            .unit_instances
            .iter()
            .filter(|u| u.party.is_hostile_to(party) && !u.off_map)
            .map(|u| {
                let dist = combat::manhattan(unit_pos, (u.x, u.y));
                // 残 HP は強化パーツ込みの実効最大 HP を基準にする。
                let hp_remaining = self.database.effective_max_hp(u) - u.damage;
                let exp_value = self
                    .database
                    .pilot_by_name(&u.pilot_name)
                    .map(|p| i64::from(p.exp_value))
                    .unwrap_or(0);

                // Use combat prediction to estimate damage against this target
                let best_weapon = combat::best_weapon_in_range(unit_data, dist);

                // 攻守とも実効値込みデータで与ダメージを推定する (静的データではなく
                // 育成 / 強化パーツ / 状態異常を反映)。攻撃側データが無い場合は 0。
                let estimated_damage = match (best_weapon, &atk_combat) {
                    (Some(weapon), Some((ap, au))) => self
                        .database
                        .idx_by_uid(&u.uid)
                        .and_then(|i| self.database.effective_combat_data(i))
                        .map(|(dp, du)| {
                            crate::combat::predict(ap, au, weapon, &dp, &du, 0, 0).damage
                        })
                        .unwrap_or(0),
                    _ => 0,
                };

                // Score: prefer targets that can be killed (damage >= hp_remaining)
                // Then prefer closer targets, then higher exp value
                let can_kill = estimated_damage >= hp_remaining;
                let score = if can_kill {
                    // High priority for killable targets (negative score = sorted first)
                    -10000 + i64::from(dist)
                } else {
                    // Otherwise: prefer closer targets with less HP and higher exp
                    i64::from(dist) * 10 + hp_remaining.max(0) / 100 - exp_value / 100
                };
                ((u.x, u.y), score, hp_remaining)
            })
            .collect();
        // 撃破可能 (= 残 HP が小さい) ターゲットを優先するため score 昇順。
        candidates.sort_by_key(|(_, score, _)| *score);
        let Some(&(target_pos, _, _)) = candidates.first() else {
            return;
        };
        let mut target_pos = target_pos;
        // ChangeMode「護衛 <対象>」: 指定ユニットの近くへ移動して守る (移動目標を
        // 守護対象に置換)。攻撃自体は移動後に射程内の敵を狙う (attack_target)。
        if let Some(name) = ai_mode
            .strip_prefix("護衛")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            if let Some(gpos) = self
                .database
                .unit_instances
                .iter()
                .find(|u| {
                    !u.off_map
                        && (u.unit_data_name == name || u.pilot_name == name || u.uid == name)
                })
                .map(|u| (u.x, u.y))
            {
                target_pos = gpos;
            }
        }
        // ChangeMode による思考誘導 (決戦！宇宙怪獣2話 の NPC 制御):
        //  - `ChangeMode <unit> <x> <y>` (座標モード): 指定座標へ向かう (ボスを釣る)。
        //  - `ChangeMode <unit> <対象ユニット名>` (ユニット狙い): 指定ユニットを
        //    優先ターゲットにする (NPC をボスへ集中させる)。
        // (`通常`/`待機`/`固定`/`逃亡`/`護衛 …` は別途処理済み。)
        {
            let mode = ai_mode.trim();
            let toks: Vec<&str> = mode.split_whitespace().collect();
            let coord = match toks.as_slice() {
                [a, b] => match (a.parse::<u32>(), b.parse::<u32>()) {
                    (Ok(x), Ok(y)) => Some((x, y)),
                    _ => None,
                },
                _ => None,
            };
            if let Some(c) = coord {
                target_pos = c;
            } else if !mode.is_empty()
                && !matches!(mode, "通常" | "待機" | "固定" | "逃亡")
                && !mode.starts_with("護衛")
            {
                // ユニット狙い: 名前一致する敵対ユニットを優先ターゲットに。
                if let Some(tpos) = self
                    .database
                    .unit_instances
                    .iter()
                    .find(|u| {
                        !u.off_map
                            && u.party.is_hostile_to(party)
                            && (u.unit_data_name == mode || u.pilot_name == mode || u.uid == mode)
                    })
                    .map(|u| (u.x, u.y))
                {
                    target_pos = tpos;
                }
            }
        }

        // 移動範囲を Dijkstra で計算 (地形適応・特殊能力を考慮)。
        let move_cost_fn = {
            let terrain_table = self.database.catalog.terrains.clone();
            let transportation = unit_data.transportation.clone();
            let adaption = unit_data.adaption.0;
            let current_area = self.database.unit_instances[idx].current_area.clone();
            let active_feature_names: Vec<String> = self.database.unit_instances[idx]
                .active_features
                .iter()
                .map(|f| f.name.clone())
                .collect();
            let terrain_adapt_names: Vec<String> = self.database.unit_instances[idx]
                .active_features
                .iter()
                .filter(|f| f.is_active && f.name == "地形適応")
                .flat_map(|f| {
                    // value = "別名 地形名称1 地形名称2..." (first token is alias)
                    f.value
                        .split_whitespace()
                        .skip(1)
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                })
                .collect();
            crate::movement::make_unit_cost_fn(
                terrain_table,
                transportation,
                adaption,
                current_area,
                active_feature_names,
                terrain_adapt_names,
            )
        };
        let reachable = match self.database.map.as_ref() {
            Some(m) => crate::movement::compute_range_with(m, unit_pos, speed, move_cost_fn),
            None => std::collections::HashMap::new(),
        };

        // 恐怖 (特殊効果攻撃属性 恐) / ChangeMode「逃亡」: 敵から逃げ続ける。
        // 到達マスのうち敵への最小距離が最大のマスへ移動し、攻撃はしない。
        if self.database.unit_instances[idx].has_condition("恐怖") || ai_mode == "逃亡" {
            let enemies: Vec<(u32, u32)> = candidates.iter().map(|(p, _, _)| *p).collect();
            let occ: std::collections::HashSet<(u32, u32)> = self
                .database
                .unit_instances
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != idx)
                .map(|(_, u)| (u.x, u.y))
                .collect();
            let flee = reachable
                .iter()
                .filter(|((x, y), _)| !occ.contains(&(*x, *y)))
                .max_by_key(|((x, y), _)| {
                    enemies
                        .iter()
                        .map(|e| combat::manhattan((*x, *y), *e))
                        .min()
                        .unwrap_or(0)
                })
                .map(|((x, y), _)| (*x, *y));
            if let Some(dest) = flee {
                if dest != unit_pos {
                    let uid = self.database.unit_instances[idx].uid.clone();
                    self.begin_move_anim(&uid, &reachable, unit_pos, dest);
                    self.database.move_unit(&uid, dest.0, dest.1);
                }
            }
            self.database.unit_instances[idx].has_acted = true;
            return;
        }

        // ターゲットを射程内に収める最良の到達マス: ターゲット直近 ≤ max_range
        // のマスのうち、占有されていない / 自分の現在位置 OK を選ぶ。
        let occupied: std::collections::HashSet<(u32, u32)> = self
            .database
            .unit_instances
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != idx)
            .map(|(_, u)| (u.x, u.y))
            .collect();
        // マップ兵器回避: 敵対勢力がマップ兵器を持つとき、同陣営ユニットとの密集を避けて
        // 攻撃マスを選ぶ (1 発で複数巻き込まれないよう散開する)。脅威が無ければ密集度は
        // 常に 0 で従来挙動と同じ (= 既存テストへ影響なし)。
        let map_threat = self.hostile_has_map_weapon(party);
        let ally_positions: Vec<(u32, u32)> = if map_threat {
            self.database
                .unit_instances
                .iter()
                .enumerate()
                .filter(|(i, u)| *i != idx && !u.off_map && u.party == party)
                .map(|(_, u)| (u.x, u.y))
                .collect()
        } else {
            Vec::new()
        };
        let cluster_at = |x: u32, y: u32| -> i64 {
            ally_positions
                .iter()
                .filter(|(ax, ay)| combat::manhattan((x, y), (*ax, *ay)) <= 2)
                .count() as i64
        };
        let best_move = reachable
            .iter()
            .filter(|((x, y), _)| !occupied.contains(&(*x, *y)))
            .filter(|((x, y), _)| combat::manhattan((*x, *y), target_pos) <= max_range)
            // 攻撃可能 (射程内) なマスのうち、防御地形 (森/山/都市など) を選好し、次に
            // マップ兵器脅威下では密集を避ける。同条件ならターゲットに最も近い / 残 MP 最大を選好。
            .min_by_key(|((x, y), rem)| {
                (
                    -self.tile_defensive_value(*x, *y), // 防御価値が高いほど優先 (負で最小化)
                    cluster_at(*x, *y), // マップ兵器脅威下は密集を回避 (脅威無し=常に0)
                    combat::manhattan((*x, *y), target_pos),
                    -*rem, // BinaryHeap と同じ符号反転で大きい方を最小に
                )
            })
            .map(|((x, y), _)| (*x, *y));
        if mode_keeps_position {
            // `待機` モード: 移動せず、攻撃のみ試行 (現在位置から射程内に
            // 敵がいる場合のみ attack_target で命中する)。
        } else if let Some(dest) = best_move {
            if dest != unit_pos {
                let uid = self.database.unit_instances[idx].uid.clone();
                self.database.move_unit(&uid, dest.0, dest.1);
            }
        } else {
            // 射程に届く到達マスが無い: ターゲットに最も近い到達マスへ移動。
            let approach = reachable
                .iter()
                .filter(|((x, y), _)| !occupied.contains(&(*x, *y)))
                .min_by_key(|((x, y), _)| combat::manhattan((*x, *y), target_pos))
                .map(|((x, y), _)| (*x, *y));
            if let Some(dest) = approach {
                if dest != unit_pos {
                    let uid = self.database.unit_instances[idx].uid.clone();
                    self.begin_move_anim(&uid, &reachable, unit_pos, dest);
                    self.database.move_unit(&uid, dest.0, dest.1);
                }
            }
        }

        // 攻撃試行 (attack_target はカーソル位置のユニットを使う)
        let new_pos = (
            self.database.unit_instances[idx].x,
            self.database.unit_instances[idx].y,
        );
        self.map_cursor = Some(new_pos);
        // 敵 AI: 射程内に攻撃対象が居れば、攻撃前に攻撃補助の精神コマンドを使う。
        if self.ai_nearest_target_tile(idx, new_pos).is_some() {
            self.ai_use_offensive_spirit(idx);
        }
        // 逐次演出かつ手動反撃モードのとき、攻撃対象が味方なら反撃手段の選択を出して
        // 中断する (応答後に attack を解決)。対象が味方以外/自動反撃モードなら自動解決。
        if self.animate_ai && !self.auto_counter {
            if let Some((def_uid, tile)) = self.ai_nearest_target_tile(idx, new_pos) {
                if self.reaction_applies(&def_uid) {
                    let atk_uid = self.database.unit_instances[idx].uid.clone();
                    self.begin_reaction_prompt(atk_uid, def_uid, tile);
                    return;
                }
            }
        }
        let _ = self.attack_target();
    }

    /// 敵 AI のマップ兵器使用。`Ｍ` 属性の武器を持ち、現在位置からの照準で 2 体以上の
    /// 敵対ユニットを巻き込める場合、最も多く巻き込む座標へマップ攻撃を発射する。発射したら
    /// `true`。マップ兵器は移動前提でないため現在位置から評価する。テスト用ユニットは
    /// マップ武器を持たないので無効 (= 既存テストへ影響なし)。
    pub(super) fn ai_use_map_weapon(&mut self, idx: usize) -> bool {
        let pos = (
            self.database.unit_instances[idx].x,
            self.database.unit_instances[idx].y,
        );
        let party = self.database.unit_instances[idx].party;
        let unit_name = self.database.unit_instances[idx].unit_data_name.clone();
        let caster = self.database.unit_instances[idx].uid.clone();
        // マップ兵器 (class に全角 Ｍ) の WeaponData を収集。
        let map_weapons: Vec<crate::data::unit::WeaponData> =
            match self.database.unit_by_name(&unit_name) {
                Some(d) => d
                    .weapons
                    .iter()
                    .filter(|w| w.class.contains('Ｍ'))
                    .cloned()
                    .collect(),
                None => return false,
            };
        if map_weapons.is_empty() {
            return false;
        }
        let enemies: Vec<(u32, u32)> = self
            .database
            .unit_instances
            .iter()
            .filter(|u| !u.off_map && u.party.is_hostile_to(party))
            .map(|u| (u.x, u.y))
            .collect();
        if enemies.len() < 2 {
            return false;
        }
        for weapon in &map_weapons {
            // 各敵位置を照準候補に、最も多く敵を巻き込む中心を探す。
            let mut best: Option<((u32, u32), usize)> = None;
            for &center in &enemies {
                let area = crate::combat::map_attack_area(weapon, pos, center);
                let hits = enemies.iter().filter(|e| area.contains(e)).count();
                if hits >= 2 && best.as_ref().map(|(_, h)| hits > *h).unwrap_or(true) {
                    best = Some((center, hits));
                }
            }
            if let Some((center, _)) = best {
                // AI 発のマップ攻撃コマンド = 通常戦闘扱い (is_event=false)。
                // SRC `Unit.MapAttack` 同様、ダメージ適用前に 使用/攻撃 イベントを発火。
                self.execute_map_attack(&caster, &weapon.name, center.0, center.1, false);
                if let Some(i) = self.database.idx_by_uid(&caster) {
                    self.database.unit_instances[i].has_acted = true;
                }
                return true;
            }
        }
        false
    }

    /// 敵 AI の支援アビリティ使用。回復 (HP、効果に `回復`、霊力/ＳＰ は除く) または
    /// 補給 (EN、効果に `補給`) を提供するアビリティを持ち、射程内にその支援を必要とする
    /// 味方が居れば、最も必要度の高い味方へ発動する。発動したら `true`。回復対象は最大
    /// ダメージ、補給対象は最大 EN 消費で選ぶ。テスト用ユニットはアビリティ無しなので
    /// 無効 (= 既存テストへ影響なし)。
    pub(super) fn ai_use_support_ability(&mut self, idx: usize) -> bool {
        let caster = self.database.unit_instances[idx].uid.clone();
        let unit_name = self.database.unit_instances[idx].unit_data_name.clone();
        // (アビリティ index, 回復するか, 補給するか) を収集。
        let support_idxs: Vec<(usize, bool, bool)> = match self.database.unit_by_name(&unit_name) {
            Some(d) => d
                .abilities
                .iter()
                .enumerate()
                .filter_map(|(i, a)| {
                    let heals = a.effect.contains("回復")
                        && !a.effect.contains("霊力")
                        && !a.effect.contains("ＳＰ");
                    let supplies = a.effect.contains("補給");
                    (heals || supplies).then_some((i, heals, supplies))
                })
                .collect(),
            None => return false,
        };
        for (ab_idx, heals, supplies) in support_idxs {
            if !self.ability_usable(&caster, ab_idx) {
                continue;
            }
            // 射程内で最も支援を必要とする味方を対象にする
            // (回復=最大ダメージ / 補給=最大 EN 消費)。
            let target_uids: Vec<String> = self
                .database
                .unit_instances
                .iter()
                .filter(|u| {
                    !u.off_map && ((heals && u.damage > 0) || (supplies && u.en_consumed > 0))
                })
                .map(|u| u.uid.clone())
                .collect();
            let mut best: Option<(String, i64)> = None;
            for tuid in target_uids {
                if !self.ability_target_valid(&caster, ab_idx, &tuid) {
                    continue;
                }
                let need = self
                    .database
                    .unit_by_uid(&tuid)
                    .map(|u| {
                        let h = if heals { u.damage } else { 0 };
                        let s = if supplies {
                            i64::from(u.en_consumed)
                        } else {
                            0
                        };
                        h.max(s)
                    })
                    .unwrap_or(0);
                if best.as_ref().map(|(_, d)| need > *d).unwrap_or(true) {
                    best = Some((tuid, need));
                }
            }
            if let Some((tuid, _)) = best {
                self.apply_ability(&caster, ab_idx, &tuid);
                if let Some(i) = self.database.idx_by_uid(&caster) {
                    if self.database.unit_instances[i].has_acted {
                        crate::event_runtime::fire_action_end_labels(self, i);
                    }
                }
                return true;
            }
        }
        false
    }

    /// 敵 AI の攻撃アビリティ使用。敵対勢力を対象とするアビリティ (`脱`=気力減少 /
    /// `除`=状態解除 等の属性) を持ち、射程内に有効な敵が居れば、最も気力の高い敵へ
    /// 発動する (`脱` で気力を削ぐ価値が高い相手を優先)。発動したら `true`。テスト用
    /// ユニットはアビリティ無しなので無効 (= 既存テストへ影響なし)。
    pub(super) fn ai_use_offensive_ability(&mut self, idx: usize) -> bool {
        let caster = self.database.unit_instances[idx].uid.clone();
        let unit_name = self.database.unit_instances[idx].unit_data_name.clone();
        let atk_idxs: Vec<usize> = match self.database.unit_by_name(&unit_name) {
            Some(d) => d
                .abilities
                .iter()
                .enumerate()
                .filter(|(_, a)| a.targets_enemy())
                .map(|(i, _)| i)
                .collect(),
            None => return false,
        };
        for ab_idx in atk_idxs {
            if !self.ability_usable(&caster, ab_idx) {
                continue;
            }
            // 射程内で最も気力の高い敵を対象にする。
            let target_uids: Vec<String> = self
                .database
                .unit_instances
                .iter()
                .filter(|u| !u.off_map)
                .map(|u| u.uid.clone())
                .collect();
            let mut best: Option<(String, i32)> = None;
            for tuid in target_uids {
                if !self.ability_target_valid(&caster, ab_idx, &tuid) {
                    continue;
                }
                let morale = self
                    .database
                    .unit_by_uid(&tuid)
                    .map(|u| u.morale)
                    .unwrap_or(0);
                if best.as_ref().map(|(_, m)| morale > *m).unwrap_or(true) {
                    best = Some((tuid, morale));
                }
            }
            if let Some((tuid, _)) = best {
                self.apply_ability(&caster, ab_idx, &tuid);
                if let Some(i) = self.database.idx_by_uid(&caster) {
                    if self.database.unit_instances[i].has_acted {
                        crate::event_runtime::fire_action_end_labels(self, i);
                    }
                }
                return true;
            }
        }
        false
    }

    /// 敵 AI の召喚アビリティ使用。`召喚` 効果のアビリティを持ち、自陣営がオンマップ数で
    /// 敵対勢力に劣勢 (味方数 ≤ 敵数) のとき、増援を召喚する。発動したら `true`。劣勢
    /// ゲートで数的に拮抗すれば止まるため乱発しない (召喚アビリティは多くが回数制限付き
    /// でさらに有限)。テスト用ユニットはアビリティ無しなので無効 (= 既存テストへ影響なし)。
    pub(super) fn ai_use_summon_ability(&mut self, idx: usize) -> bool {
        let caster = self.database.unit_instances[idx].uid.clone();
        let party = self.database.unit_instances[idx].party;
        let unit_name = self.database.unit_instances[idx].unit_data_name.clone();
        let summon_idxs: Vec<usize> = match self.database.unit_by_name(&unit_name) {
            Some(d) => d
                .abilities
                .iter()
                .enumerate()
                .filter(|(_, a)| a.effect.contains("召喚"))
                .map(|(i, _)| i)
                .collect(),
            None => return false,
        };
        if summon_idxs.is_empty() {
            return false;
        }
        // 劣勢判定: オンマップの自陣営数 ≤ 敵対勢力数 のときのみ召喚する。
        let allies = self
            .database
            .unit_instances
            .iter()
            .filter(|u| !u.off_map && u.party == party)
            .count();
        let enemies = self
            .database
            .unit_instances
            .iter()
            .filter(|u| !u.off_map && u.party.is_hostile_to(party))
            .count();
        if allies > enemies {
            return false;
        }
        for ab_idx in summon_idxs {
            if !self.ability_usable(&caster, ab_idx) {
                continue;
            }
            // 召喚は発動者自身 (隣接空きマスに生成) を対象にする。
            if !self.ability_target_valid(&caster, ab_idx, &caster) {
                continue;
            }
            self.apply_ability(&caster, ab_idx, &caster);
            if let Some(i) = self.database.idx_by_uid(&caster) {
                if self.database.unit_instances[i].has_acted {
                    crate::event_runtime::fire_action_end_labels(self, i);
                }
            }
            return true;
        }
        false
    }

    /// 敵 AI の攻撃補助精神コマンド使用。パイロットが習得済み (SP/気力/レベル充足) の
    /// 攻撃補助系精神 (魂 > 熱血 > 必中 の優先順) を 1 つ発動する。既に攻撃補助状態を
    /// 持つ場合や該当精神を習得していない場合は何もしない。テスト用ユニットは
    /// `spirit_commands` が空なので無効 (= 既存テストへ影響なし)。
    pub(super) fn ai_use_offensive_spirit(&mut self, idx: usize) {
        if self.database.unit_instances[idx].has_condition("魂")
            || self.database.unit_instances[idx].has_condition("熱血")
            || self.database.unit_instances[idx].has_condition("必中")
        {
            return;
        }
        let uid = self.database.unit_instances[idx].uid.clone();
        let opts = self.spirit_command_options(&uid);
        if opts.is_empty() {
            return;
        }
        for name in ["魂", "熱血", "必中"] {
            if let Some((_, cost)) = opts.iter().find(|(n, _)| n == name) {
                // 気力条件 (魂=120 / 熱血=80 等) を満たすこと。
                if !self.database.unit_instances[idx].morale_sufficient_for_power(name) {
                    continue;
                }
                let cost = *cost;
                self.consume_unit_sp(&uid, cost);
                self.database.unit_instances[idx].add_condition(crate::Condition::new(name, 1));
                let nick = self.database.unit_instances[idx].pilot_name.clone();
                self.push_message(format!("{nick} は精神コマンド【{name}】を使用！"));
                return;
            }
        }
    }

    /// 敵 AI の支援精神コマンド使用。HP 回復系精神 (信頼/友情/愛/根性/ド根性) を習得し
    /// (SP/レベル充足)、最大 HP の半分以下まで負傷した同陣営ユニットが居れば 1 つ発動して
    /// 回復する。精神は行動を消費しないため発動後も移動・攻撃を続ける。発動したら `true`。
    /// 安価な単体 (信頼) → 全体 (友情/愛) → 自分のみ (根性/ド根性) の優先順で試す。テスト用
    /// ユニットは `spirit_commands` が空なので無効 (= 既存テストへ影響なし)。
    pub(super) fn ai_use_support_spirit(&mut self, idx: usize) -> bool {
        let uid = self.database.unit_instances[idx].uid.clone();
        let party = self.database.unit_instances[idx].party;
        let opts = self.spirit_command_options(&uid);
        if opts.is_empty() {
            return false;
        }
        // 最も負傷した同陣営ユニット (自分含む) を探す。半分以下まで減った味方が
        // 居なければ SP を温存する。
        let mut worst: Option<(String, i64)> = None;
        let mut any_below_half = false;
        for u in self.database.unit_instances.iter() {
            if u.off_map || u.party != party || u.damage <= 0 {
                continue;
            }
            let max_hp = self.database.effective_max_hp(u);
            if u.damage * 2 >= max_hp {
                any_below_half = true;
            }
            if worst.as_ref().map(|(_, d)| u.damage > *d).unwrap_or(true) {
                worst = Some((u.uid.clone(), u.damage));
            }
        }
        if !any_below_half {
            return false;
        }
        let Some((worst_uid, _)) = worst else {
            return false;
        };
        for name in ["信頼", "友情", "愛", "根性", "ド根性"] {
            let Some((_, cost)) = opts.iter().find(|(n, _)| n == name) else {
                continue;
            };
            let cost = *cost;
            match self.spirit_target_kind(name) {
                SpiritTargetKind::SingleAlly => {
                    self.consume_unit_sp(&uid, cost);
                    self.apply_spirit_effect(&worst_uid, name);
                }
                SpiritTargetKind::AllAllies => {
                    self.consume_unit_sp(&uid, cost);
                    let targets: Vec<String> = self
                        .database
                        .unit_instances
                        .iter()
                        .filter(|u| !u.off_map && u.party == party)
                        .map(|u| u.uid.clone())
                        .collect();
                    for t in &targets {
                        self.apply_spirit_effect(t, name);
                    }
                }
                SpiritTargetKind::SelfOnly => {
                    // 根性 / ド根性 は自分のみ。自分が最も負傷しているときだけ使う。
                    if worst_uid != uid {
                        continue;
                    }
                    self.consume_unit_sp(&uid, cost);
                    self.apply_spirit_effect(&uid, name);
                }
                SpiritTargetKind::SingleEnemy => continue,
            }
            let nick = self.database.unit_instances[idx].pilot_name.clone();
            self.push_message(format!("{nick} は精神コマンド【{name}】を使用！"));
            return true;
        }
        false
    }

    /// 復活精神の能動 pre-buff: 瀕死 (実効最大 HP の 1/4 以下) の自ユニットが「復活」精神を
    /// 持ち SP が足りていれば、撃破される前に自分へ「復活」を掛ける。撃破予測の近似として
    /// 低 HP を用いる (次の敵フェイズ前に死にやすい)。精神は行動を消費しない free action。
    /// 既に「復活」を保持しているなら再発動しない (SP 温存)。発動したら `true`。テスト用
    /// ユニットは `spirit_commands` が空なので無効 (= 既存テストへ影響なし)。
    pub(super) fn ai_use_revive_spirit(&mut self, idx: usize) -> bool {
        // 既に復活を保持していれば不要。
        if self.database.unit_instances[idx].has_condition("復活") {
            return false;
        }
        // 瀕死判定: 実効最大 HP の 1/4 以下。
        let max_hp = self
            .database
            .effective_max_hp(&self.database.unit_instances[idx]);
        let cur_hp = max_hp - self.database.unit_instances[idx].damage;
        if cur_hp * 4 > max_hp {
            return false; // まだ余裕がある
        }
        // 復活精神が発動可能か (level / 残 SP)。
        let uid = self.database.unit_instances[idx].uid.clone();
        let opts = self.spirit_command_options(&uid);
        let Some((_, cost)) = opts.iter().find(|(n, _)| n == "復活") else {
            return false;
        };
        let cost = *cost;
        self.consume_unit_sp(&uid, cost);
        self.apply_spirit_effect(&uid, "復活");
        let nick = self.database.unit_instances[idx].pilot_name.clone();
        self.push_message(format!("{nick} は精神コマンド【復活】を使用！"));
        true
    }

    /// 移動スライド演出を仕込む (逐次演出時のみ)。`reachable` から `start`→`dest` の
    /// 経路を復元して `move_anim` にセットする。`animate_ai=false` (ヘッドレス) では何もしない。
    pub(super) fn begin_move_anim(
        &mut self,
        uid: &str,
        reachable: &std::collections::HashMap<(u32, u32), i32>,
        start: (u32, u32),
        dest: (u32, u32),
    ) {
        if !self.animate_ai {
            return;
        }
        let path = crate::movement::reconstruct_path(reachable, start, dest);
        if path.len() > 1 {
            self.move_anim = Some(crate::battle_anim::MoveAnim::new(uid.to_string(), path));
        }
    }

    /// AI ユニット (atk_idx, カーソル `cursor`) が狙う射程内最寄り敵対ユニットの
    /// (uid, タイル) を返す (read-only)。`attack_resolve_and_run` の None 分岐と同じ選択。
    pub(super) fn ai_nearest_target_tile(
        &self,
        atk_idx: usize,
        cursor: (u32, u32),
    ) -> Option<(String, (u32, u32))> {
        let atk = &self.database.unit_instances[atk_idx];
        let atk_party = atk.party;
        let atk_charged = atk.charged;
        let atk_unit = self.database.unit_by_name(&atk.unit_data_name)?;
        let forced = if self.selected_weapon_idx == 0 {
            None
        } else {
            atk_unit.weapons.get(self.selected_weapon_idx - 1)
        };
        let mut best: Option<(usize, u32)> = None;
        for (i, def) in self.database.unit_instances.iter().enumerate() {
            if i == atk_idx || !def.party.is_hostile_to(atk_party) || def.off_map {
                continue;
            }
            let d = combat::manhattan(cursor, (def.x, def.y));
            // post_move=false なので 突撃 (totsugeki) は影響しない。
            if Self::pick_attack_weapon(
                atk_unit,
                d,
                forced,
                atk_charged,
                false,
                false,
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
        let (di, _) = best?;
        let d = &self.database.unit_instances[di];
        Some((d.uid.clone(), (d.x, d.y)))
    }
}
