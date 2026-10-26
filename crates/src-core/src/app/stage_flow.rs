//! ステージ進行 (シナリオ開始・出撃・フェイズ開始/終了・勝敗・次ステージ) と
//! セーブ/ロード、毎フレームの `tick`。元 SRC `SRC.play.cs::StartScenario` / `StartTurn` 系。
//!
//! 上位のシーン進行を以下のメソッドに集約する:
//!
//! ```text
//!   start_scenario(name)         // Title/Configuration から呼ぶ
//!     │ Prologue / プロローグ ラベル発火
//!     ▼
//!   StageState::Briefing
//!     │ begin_sortie()
//!     ▼
//!   StageState::Sortie
//!     │ begin_battle()
//!     │   Start / スタート ラベル発火
//!     │   begin_phase(Player) → "ターン 1" + "ターン 1 味方" 発火
//!     ▼
//!   StageState::Battle  (end_phase で begin_phase(Enemy)→AI→…→begin_phase(Player(T+1)) を回す)
//!     │ game_clear()  ─▶ Victory  (Victory / 勝利 ラベル発火)
//!     │ game_over()   ─▶ Defeat   (GameOver / ゲームオーバー ラベル発火)
//!     ▼
//!   終了オーバーレイ → Title へ戻す
//! ```

use super::*;

impl App {
    pub const fn stage_state(&self) -> crate::stage::StageState {
        self.stage_state
    }

    pub fn briefing(&self) -> &str {
        &self.briefing
    }

    pub fn set_briefing(&mut self, text: String) {
        self.briefing = text;
    }

    pub fn set_stage_state(&mut self, s: crate::stage::StageState) {
        self.stage_state = s;
    }

    /// 元 SRC `StartScenario(fname)` の Rust 版エントリ。
    ///
    /// `name` は表示用のシナリオ名（HUD 表示や save 用のキーになる文字列）。
    /// `.eve` のロード自体は呼び出し側で済ませておく前提で、ここでは:
    ///   1. ステージ進行状態を `Briefing` にリセット
    ///   2. ターン / メニュー / アクションモードを初期化
    ///   3. シーンを MapView に遷移
    ///   4. 自動発火: `Prologue` / `プロローグ` ラベル
    ///
    /// が走る。SRC では `Stage = "プロローグ"` 相当。
    pub fn start_scenario(&mut self, name: impl Into<String>) {
        self.stage = name.into();
        self.stage_state = crate::stage::StageState::Briefing;
        self.turn = Turn::new();
        self.command_menu = None;
        self.action_mode = crate::command_menu::ActionMode::Browse;
        self.scene = Scene::MapView;
        self.settings_snapshot = None;
        if !self.stage.is_empty() {
            self.push_message(format!("シナリオ「{}」開始", self.stage));
        }
        // 元 SRC: Event.HandleEvent("プロローグ")。通過実行の観測のため
        // 発火前に通過記録をリセットする。
        self.start_passed_pcs.clear();
        crate::event_runtime::trigger_label(self, "Prologue");
        crate::event_runtime::trigger_label(self, "プロローグ");
        // 原典 SRC `StartScenario`: プロローグ完了後に `スタート` を発火して
        // Battle へ (SRC.bas L1208-1262)。「完了後」を継続として積む。プロローグ
        // が `スタート` ラベルを通過実行していた場合 (start_passed_pcs) は
        // AfterStageFileRun が再発火を抑止する。
        self.flow.push(crate::flow::FlowCont::AfterStageFileRun);
        self.on_script_completed();
    }

    /// 元 SRC: Briefing → Sortie（出撃ユニット確認）。重複呼び出しは無視。
    pub fn begin_sortie(&mut self) {
        if self.stage_state != crate::stage::StageState::Briefing {
            return;
        }
        self.stage_state = crate::stage::StageState::Sortie;
        self.push_message("出撃準備フェーズ".to_string());
    }

    /// 元 SRC `StartTurn("味方")` 相当: Sortie → Battle に遷移して `Start` ラベル
    /// を発火する。`スタート` の **完了後** (インライン完了でも suspend 後の
    /// 完了でも) `FlowCont::AfterStartEvent` 継続が Player フェーズを開始する。
    /// Sortie 以外からの呼び出しは無視。
    pub fn begin_battle(&mut self) {
        if self.stage_state != crate::stage::StageState::Sortie {
            return;
        }
        self.enter_battle_state();
        // 元 SRC: Event.HandleEvent("スタート") → 完了後に StartTurn "味方"
        // (SRC.bas `StartScenario` 末尾)。「完了後」を継続として積んでから発火。
        self.flow.push(crate::flow::FlowCont::AfterStartEvent);
        // `スタート` / `Start` ラベルは各ステージ .eve に個別に存在するため、
        // 現ステージファイルが分かるならそのファイルスコープで発火する。
        // グローバル発火だと先頭登録ファイルの `スタート` を誤って実行し、
        // 当該ステージの敵配置 (`Call 敵配置` 等) が走らない。
        let stage_file = self.current_stage_file.clone();
        let fired = !stage_file.is_empty()
            && (crate::event_runtime::trigger_label_in_file(self, &stage_file, "Start")
                || crate::event_runtime::trigger_label_in_file(self, &stage_file, "スタート"));
        if !fired {
            crate::event_runtime::trigger_label(self, "Start");
            crate::event_runtime::trigger_label(self, "スタート");
        }
        // `スタート` が未定義で何も起動しなかった場合はここで継続を消化して
        // 味方フェイズを開始する (起動した場合は run_loop の完了通知が消化
        // 済み / suspend 中なら resume 完了時に消化される)。
        self.on_script_completed();
    }

    /// Battle 状態への突入処理 (`スタート` 発火と味方フェイズ開始は含まない)。
    /// `begin_battle` と `FlowCont::AfterStageFileRun` (スタート通過実行済みの
    /// ステージファイル) の共通部。
    pub(super) fn enter_battle_state(&mut self) {
        // Briefing/Sortie フェーズでスクリプトが PaintString した描画は
        // 戦闘開始時にクリア。scene は MapView のままなので `set_scene` の
        // cleared_on_exit では拾われない。
        self.script_overlay.clear();
        self.hotpoints.clear();
        self.stage_state = crate::stage::StageState::Battle;
        // 原典 SRC `SRC.cs::StartScenario`: マップ未ロードのままステージを開始する
        // 場合はデフォルト 15×15 マップを生成する (`if Map.MapWidth == 1:
        // SetMapSize(15, 15)`)。`ChangeMap` を持たないステージ (例: 東方夢想伝
        // 01〜11) はこの暗黙マップ上で `Create` 配置・戦闘を行う。`スタート`
        // 内で `ChangeMap` / `MapSize` する場合は後勝ちで差し替わる。
        if self.database.map.is_none() {
            self.database
                .replace_map(crate::data::map::MapData::new(15, 15));
        }
        // SRC `Restartコマンド` 用に、戦闘開始時点のスナップショットを
        // `__restart_save` script_var に保存しておく。`_リスタート.src` 相当。
        // 失敗時は無視 (Restart コマンドが no-op になるだけ)。
        if let Ok(json) = self.to_save_json() {
            self.set_script_var("__restart_save".to_string(), json);
        }
    }

    /// 元 SRC `StartTurn(uparty)` の冒頭処理: フェイズに入ったときの初期化と
    /// ターンラベル発火。呼び出し側は事前に `turn.number` を確定させておくこと
    /// （Neutral → Player のラップで +1 する責務は `end_phase` 側にある）。
    ///
    /// - `has_acted` をリセット
    /// - コマンドメニュー / 行動モードを Browse に戻す
    /// - Player フェイズなら "ターン N 開始" HUD メッセージ
    /// - 自動発火: `Turn N` / `ターン N`（Player のみ）と
    ///   `Turn N <stage>` / `ターン N <stage>`（全フェイズ）
    pub(super) fn begin_phase(&mut self, phase: crate::Phase) {
        self.turn.phase = phase;
        let party = phase.party();
        // フェイズ開始時の状態異常処理:
        // - 毒: 最大 HP の 10% ダメージ（撃破は防ぐため最低 HP 1）
        // - 1 回限定 (lifetime==1) の精神コマンド系 (必中 / 集中 / ひらめき / 熱血 /
        //   魂 / 気合 / 不屈 / 鉄壁 等) は当該陣営フェイズ開始時に解除する。
        //   `clear_one_turn_conditions` と同じ「lifetime==1 = 発動ターン限り」の規約に
        //   従い、ユニットコマンド「精神」で付与した全コマンドを一律に解除する。
        for i in 0..self.database.unit_instances.len() {
            if self.database.unit_instances[i].party != party {
                continue;
            }
            // 改造・強化パーツ・ボスランクを反映した実効最大 HP を基準にする
            // (毒ダメージ / 死の宣告 が育成後の HP に追従する)。
            let max_hp = self
                .database
                .effective_max_hp(&self.database.unit_instances[i]);
            let u = &mut self.database.unit_instances[i];
            // 毒: 最大 HP の 10%。毒属性への弱点で倍・耐性で半減 (特殊効果攻撃属性.md)。
            if u.has_condition("毒") && max_hp > 0 {
                let weak_poison = crate::feature::feature_value(&u.active_features, "弱点")
                    .is_some_and(|v| v.split_whitespace().any(|t| t == "毒"));
                let resist_poison = crate::feature::feature_value(&u.active_features, "耐性")
                    .is_some_and(|v| v.split_whitespace().any(|t| t == "毒"));
                let base = (max_hp / 10).max(1);
                let damage = if weak_poison {
                    base * 2
                } else if resist_poison {
                    (base / 2).max(1)
                } else {
                    base
                };
                let new_dmg = (u.damage + damage).min(max_hp - 1);
                u.damage = new_dmg;
            }
            // 死の宣告 (告): 期限切れ (lifetime≤1=この自軍フェイズで解ける) になると
            // HP が 1 になる (特殊効果攻撃属性.md)。tick_conditions より前に判定する。
            if max_hp > 0
                && u.conditions
                    .iter()
                    .any(|c| c.name == "死の宣告" && !c.is_permanent() && c.lifetime <= 1)
            {
                u.damage = (max_hp - 1).max(0);
            }
            // 状態異常の経過: 有限 lifetime を 1 減らし、0 以下で解除 (永続=-1 は保持)。
            // 旧実装は lifetime==1 のみ除去し 2 以上を永久に残していたため、特殊効果
            // 攻撃属性 (縛/痺/凍 等) の複数ターン状態が解除されなかった。当該陣営の
            // フェイズが来るたびに 1 ターン経過 (SRC `特殊効果攻撃属性.md`)。
            u.tick_conditions();
        }
        // 魅了 (特殊効果攻撃属性 魅) の期限切れ復帰: `魅了` condition が上の tick で消えた
        // ユニットは元の勢力 (charm_revert_party) へ戻し、設定した護衛 ai_mode と退避
        // フィールドをクリアする。魅了ユニットの勢力は攻撃側 (= 現フェイズ party になり得る)
        // へ移っているため、tick の直後にここで判定する。憑依 (恒久支配) は
        // charm_revert_party を使わないため対象外 (復帰しない)。
        for u in &mut self.database.unit_instances {
            if u.charm_revert_party.is_some() && !u.has_condition("魅了") {
                if let Some(orig) = u.charm_revert_party.take() {
                    u.party = orig;
                }
                if u.ai_mode.starts_with("護衛") {
                    u.ai_mode.clear();
                }
            }
        }
        // 基礎 EN 自然回復 (VB5 `Unit.cls:28182`「ＥＮは毎ターン5回復」): 当該陣営の
        // 全ユニットは、特殊能力 ＥＮ回復 の有無に関わらずフェイズ開始時に EN +5 される
        // (MaxEN 超過分は en_consumed の `.max(0)` クランプで自動的に切り捨てられる)。
        // `回復不能` (特殊効果攻撃属性 害) のユニットは対象外。VB5 の
        // `ＥＮ自然回復無効` オプションや `IsSpecialPowerInEffect("回復不能")` 判定、
        // 味方最初のターンのスキップは本実装では対象外 (下の特殊能力回復と同様に
        // `回復不能` condition のみを見る)。霊力回復は本実装では別途 (未対応)。
        for i in 0..self.database.unit_instances.len() {
            if self.database.unit_instances[i].party != party {
                continue;
            }
            let inst = &self.database.unit_instances[i];
            if inst.has_condition("回復不能") {
                continue;
            }
            self.database.unit_instances[i].en_consumed =
                (self.database.unit_instances[i].en_consumed - 5).max(0);
        }
        // 回復系特殊能力 (`回復系特殊能力.md`): 当該陣営フェイズ開始時に、
        // ＨＰ回復Lv*/ＥＮ回復Lv* は実効最大値の 10×Lv% を回復、ＨＰ消費Lv*/ＥＮ消費Lv*
        // は同率を減少させる (ＨＰ は最低 1 / ＥＮ は最低 0)。
        // ※ 基礎 EN 回復 (毎ターン 5) は上のブロックで、霊力 (プラーナ) 回復は下のブロックで実施。
        for i in 0..self.database.unit_instances.len() {
            if self.database.unit_instances[i].party != party {
                continue;
            }
            let inst = &self.database.unit_instances[i];
            let feats = &inst.active_features;
            let hp_heal = crate::feature::feature_level(feats, "ＨＰ回復");
            let hp_drain = crate::feature::feature_level(feats, "ＨＰ消費");
            let en_heal = crate::feature::feature_level(feats, "ＥＮ回復");
            let en_drain = crate::feature::feature_level(feats, "ＥＮ消費");
            if hp_heal.is_none() && hp_drain.is_none() && en_heal.is_none() && en_drain.is_none() {
                continue;
            }
            // 回復不能 (特殊効果攻撃属性 害): 特殊能力・地形による HP/EN 自然回復を
            // 阻害する (アビリティ/精神による回復は別経路なので影響しない)。消費系は継続。
            let no_regen = inst.has_condition("回復不能");
            let eff_max_hp = self.database.effective_max_hp(inst);
            let eff_max_en = self.database.effective_max_en(inst);
            let u = &mut self.database.unit_instances[i];
            if let Some(lv) = hp_heal {
                if !no_regen {
                    let heal = eff_max_hp * i64::from(10 * lv) / 100;
                    u.damage = (u.damage - heal).max(0);
                }
            }
            if let Some(lv) = hp_drain {
                let dmg = eff_max_hp * i64::from(10 * lv) / 100;
                u.damage = (u.damage + dmg).min((eff_max_hp - 1).max(0));
            }
            if let Some(lv) = en_heal {
                if !no_regen {
                    let rec = eff_max_en * (10 * lv) / 100;
                    u.en_consumed = (u.en_consumed - rec).max(0);
                }
            }
            if let Some(lv) = en_drain {
                let drn = eff_max_en * (10 * lv) / 100;
                u.en_consumed = (u.en_consumed + drn).min(eff_max_en);
            }
        }
        // 霊力 (プラーナ) 自然回復 (VB5 `Unit.cls:27374`): 主パイロットが霊力を持つ
        // (MaxPlana>0) ユニットは、フェイズ開始時に
        // `plana += MaxPlana/16 + MaxPlana*霊力回復Lv/10 - MaxPlana*霊力消費Lv/10`
        // し、[0, MaxPlana] にクランプする。HP/EN と異なり `回復不能` でも回復する
        // (VB5 では回復不能ガードの外に置かれている)。
        for i in 0..self.database.unit_instances.len() {
            if self.database.unit_instances[i].party != party {
                continue;
            }
            let inst = &self.database.unit_instances[i];
            let max_plana = self.database.effective_max_plana(inst);
            if max_plana <= 0 {
                continue;
            }
            let heal =
                crate::feature::feature_level(&inst.active_features, "霊力回復").unwrap_or(0);
            let drain =
                crate::feature::feature_level(&inst.active_features, "霊力消費").unwrap_or(0);
            let rec = max_plana / 16 + max_plana * heal / 10 - max_plana * drain / 10;
            let u = &mut self.database.unit_instances[i];
            u.plana = (u.plana + rec).clamp(0, max_plana);
        }
        // 母艦格納中ユニットの毎ターン回復 (回復系特殊能力.md 母艦): HP/EN を実効最大値の
        // 50% 回復、弾薬・アビリティ使用回数を全快。当該陣営フェイズ開始時。
        for i in 0..self.database.unit_instances.len() {
            if self.database.unit_instances[i].party != party
                || self.database.unit_instances[i].stored_in.is_none()
            {
                continue;
            }
            let inst = &self.database.unit_instances[i];
            let max_hp = self.database.effective_max_hp(inst);
            let max_en = self.database.effective_max_en(inst);
            let unit_name = inst.unit_data_name.clone();
            let (max_bullets, max_uses): (Vec<i32>, Vec<Option<i32>>) = self
                .database
                .unit_by_name(&unit_name)
                .map(|d| {
                    (
                        d.weapons.iter().map(|w| w.bullet).collect(),
                        d.abilities.iter().map(|a| a.uses).collect(),
                    )
                })
                .unwrap_or_default();
            let u = &mut self.database.unit_instances[i];
            u.damage = (u.damage - max_hp / 2).max(0);
            u.en_consumed = (u.en_consumed - max_en / 2).max(0);
            for w in &mut u.weapons {
                if let Some(b) = max_bullets.get(w.weapon_index) {
                    w.bullet_remaining = *b;
                }
            }
            for (j, a) in u.abilities.iter_mut().enumerate() {
                if let Some(uses) = max_uses.get(j) {
                    a.stock_remaining = *uses;
                }
            }
        }
        for u in &mut self.database.unit_instances {
            if u.party == party {
                u.has_acted = false;
                u.has_moved = false;
                // サポートアタック / サポートガード回数は毎フェイズ 1 リセット
                u.support_attack_remaining = 1;
                u.support_guard_remaining = 1;
                // カウンター (先制反撃) 使用回数も毎フェイズ 0 リセット (SRC UsedCounterAttack)。
                u.used_counter_attack = 0;
            }
        }
        self.command_menu = None;
        self.action_mode = crate::command_menu::ActionMode::Browse;
        let n = self.turn.number;
        let stage_name = phase.stage_name();
        if phase == crate::Phase::Player {
            self.push_message(format!("ターン {n} 開始: {}", phase.label()));
        }
        // SRC `Event.HandleEvent("ターン", Turn, party)` 相当。原典 (`Event.cs`) は
        // フェイズ開始ごとに `ターン 全 <陣営>`（毎ターン）と `ターン <N> <陣営>`
        // の 2 つを発火する。全フェイズ (味方/敵/中立/ＮＰＣ) が対象。
        // 英語エイリアス `Turn ...` も併せて発火し、後方互換を保つ。
        //
        // 発火は post_event_label (EventQue) 経由: 先行のターンイベントが Talk
        // 等で suspend しても後続イベントはキューに残り、完了後に順次実行される
        // (旧実装は trigger_label の実行中ガードに弾かれて黙って消えていた)。
        // 章ローカルイベントなので現ステージファイルにスコープして発火する
        // (全章同時ロード下で別章の同名 `ターン N <陣営>` へ漏れるのを防ぐ)。
        self.post_stage_event_label(format!("ターン 全 {stage_name}"));
        self.post_stage_event_label(format!("ターン {n} {stage_name}"));
        self.post_stage_event_label(format!("Turn All {stage_name}"));
        self.post_stage_event_label(format!("Turn {n} {stage_name}"));
        // 旧移植が発火していた陣営なしラベル (Player フェイズのみ) も後方互換で残す。
        if phase == crate::Phase::Player {
            self.post_stage_event_label(format!("ターン {n}"));
            self.post_stage_event_label(format!("Turn {n}"));
        }
        // フェイズ開始時点で勝敗が確定しているか判定する。`スタート` が味方を
        // 1 体も配置しなかった場合 (キャラ選択をキャンセルした等)、味方フェイズ
        // 開始直後に味方全滅=敗北を確定させ、「操作不能な味方フェイズ」を防ぐ。
        self.check_victory();
    }

    /// 元 SRC `GameClear`: 勝利演出に入る。Victory / 勝利 / Ending ラベルを試行。
    /// Battle 以外（既に Victory/Defeat 等）からの呼び出しは無視。
    pub fn game_clear(&mut self) {
        use crate::stage::StageState;
        if self.stage_state != StageState::Battle {
            return;
        }
        self.stage_state = StageState::Victory;
        self.command_menu = None;
        self.action_mode = crate::command_menu::ActionMode::Browse;
        self.push_message("【勝利】敵を全滅させました".to_string());
        // SRC では GameClear は単に TerminateSRC するが、シナリオ側で Ending を
        // 用意していることが多いので慣例的に発火する。スクリプト実行中
        // (GameClear/Win コマンド経由) はキューに積まれ完了後に実行される。
        // 勝利 / エンディング は章ローカルなので現ステージファイルにスコープ。
        //
        // `クリア` は原典の自動発火ラベルではないが、本移植のシナリオ (東方夢想伝等)
        // が勝利エントリとして使う。エンジンは他に `クリア` を発火する経路を持たない
        // ため (旧実装は check_victory が `クリア` 定義時に委譲するだけで誰も発火せず、
        // 敵全滅後に Battle のまま進行不能だった) ここで先頭候補として発火する。
        // game_clear は冒頭で stage_state=Victory にするため再入は冪等 (二重発火しない)。
        for lab in ["クリア", "Victory", "勝利", "Ending", "エンディング"] {
            if self.post_stage_event_label(lab.to_string()) {
                break;
            }
        }
    }

    /// 元 SRC `GameOver`: 敗北演出に入る。GameOver / ゲームオーバー ラベルを試行。
    /// Battle 以外からの呼び出しは無視。
    pub fn game_over(&mut self) {
        use crate::stage::StageState;
        if self.stage_state != StageState::Battle {
            return;
        }
        self.stage_state = StageState::Defeat;
        self.command_menu = None;
        self.action_mode = crate::command_menu::ActionMode::Browse;
        self.push_message("【敗北】味方全滅".to_string());
        // シナリオ独自の `GameOver` / `ゲームオーバー` ラベルを最優先で発火。
        for lab in ["GameOver", "ゲームオーバー"] {
            if self.post_event_label(lab.to_string()) {
                return;
            }
        }
        // SRC 本体相当の `Data/System/GameOver.eve` は **`プロローグ:` ラベル**で
        // コンティニュー Ask を出す (ラベル名が `GameOver` ではない)。そのため上の
        // 候補に当たらず「何も起こらない」状態だった。当該ファイル内の `プロローグ`
        // をファイルスコープ発火する (シナリオ本編の `プロローグ` とは別物なので、
        // global ではなく GameOver.eve に限定して誤ってオープニングへ飛ぶのを防ぐ)。
        // コンティニュー (`選択=1` → `Quickload`) は `__restart_save` からの再ロードに
        // 橋渡しされる (event_runtime の `quickload` ハンドラ)。
        if self.post_event_label_in_file("GameOver.eve", "プロローグ".to_string()) {
            return;
        }
        // シナリオも GameOver.eve も出口を持たない場合の組込みフォールバック。
        // これが無いと敗北画面 (「敗北」) のまま操作不能で詰む (実機報告: 東方夢想伝01)。
        // Enter/クリック = コンティニュー (ステージ開始時スナップショットから再開)、
        // 右クリック/Esc = タイトルへ。`advance` / `cancel_action` がこのフラグを見る。
        self.pending_game_over = true;
        let has_continue = !self.script_var("__restart_save").is_empty()
            || !self.script_var("__quicksave").is_empty();
        if has_continue {
            self.push_message(
                "Enter / クリックでコンティニュー、右クリック / Esc でタイトルへ".to_string(),
            );
        } else {
            self.push_message("右クリック / Esc でタイトルへ".to_string());
        }
    }

    /// 敗北フォールバックのコンティニュー: ステージ開始時のスナップショット
    /// (`__restart_save`、無ければ `__quicksave`) から再開を要求する。スナップ
    /// ショットが無ければタイトルへ戻る。
    pub(super) fn game_over_continue(&mut self) {
        self.pending_game_over = false;
        let snap = {
            let restart = self.script_var("__restart_save");
            if restart.is_empty() {
                self.script_var("__quicksave").to_string()
            } else {
                restart.to_string()
            }
        };
        if snap.is_empty() {
            self.return_to_title();
        } else {
            self.request_reload(snap);
        }
    }

    /// タイトル画面へ戻る (敗北フォールバック / 汎用の終了)。
    pub(super) fn return_to_title(&mut self) {
        self.pending_game_over = false;
        self.scene = Scene::Title;
        self.stage_state = crate::stage::StageState::Briefing;
        self.command_menu = None;
        self.action_mode = crate::command_menu::ActionMode::Browse;
    }

    /// 勝利状態 (Victory) で `クリア` 等が進行を解決しなかった場合に前進する
    /// (Enter / クリックから呼ばれる)。`次ステージ` が予約されていれば次ステージへ、
    /// なければインターミッション (登録があれば) かタイトルへ。
    pub(super) fn proceed_after_victory(&mut self) {
        if !self.script_var("次ステージ").is_empty() && self.advance_to_next_stage() {
            if self.script_var("次ステージ").is_empty() {
                self.scene = Scene::MapView;
            }
            return;
        }
        if !self.intermission_commands.is_empty() {
            self.scene = Scene::Intermission;
            self.intermission_cursor = 0;
            self.intermission_mode = IntermissionMode::Menu;
            self.stage_state = crate::stage::StageState::Briefing;
        } else {
            self.return_to_title();
        }
    }

    /// SRC `Continue <filename>` で予約された「次ステージ」をクリアし、
    /// 対応するエントリへ遷移する。
    ///
    /// VB5 原典では `SRC.StartScenario(次ステージ)` がエピローグ後に
    /// 呼ばれて新しいシナリオファイルをロードする。本実装は次の優先順位で
    /// 解決する:
    ///
    /// 1. **ラベル名一致**: `次ステージ` の値が `script_library.label_pc`
    ///    に存在すればそのラベルから `trigger_label` で起動。
    /// 2. **ファイル名一致** (basename, case-insensitive): `Continue
    ///    Eve\onsen.eve` のようにパス指定された場合、library 内の
    ///    対応 .eve の start_pc から `run_from_pc` で起動。
    ///
    /// フロントエンド (`archive.rs` / scenario chain orchestrator) から
    /// script 完了後に呼び出すこと。
    ///
    /// 戻り値: 次ステージが起動できれば `true`。未セット / 該当無し /
    /// 既に script 中断中の場合は `false`。
    pub fn advance_to_next_stage(&mut self) -> bool {
        let next = self.script_var("次ステージ").to_string();
        if next.is_empty() {
            return false;
        }
        // 再呼び出しで同じラベル / ファイルに無限ループしないよう先にクリア。
        self.set_script_var("次ステージ".to_string(), String::new());
        // 現ステージファイルとして記録。`begin_battle` の `スタート` 発火を
        // このファイルスコープで行うため (同名ラベル誤発火の防止)。
        self.current_stage_file = next.clone();
        // 新しいステージは Briefing から始める。原典 `StartScenario` は
        // ステージを最初から立ち上げ直す。前ステージが Victory / Defeat の
        // まま残っていると `FlowCont::AfterStageFileRun` のガード
        // (Briefing / Sortie のみ通す) に弾かれ、次ステージが永久に開始
        // しなくなる (実コーパス: ＰＴＡにようこそ！ が 1 話クリア後に停止)。
        if matches!(
            self.stage_state,
            crate::stage::StageState::Victory | crate::stage::StageState::Defeat
        ) {
            self.stage_state = crate::stage::StageState::Briefing;
        }
        // ステージファイル実行の完了後処理 (Battle 突入 or ループバック尊重) を
        // 継続として積む。`スタート` 通過の事実は `start_passed_pcs` で観測する。
        self.start_passed_pcs.clear();
        self.flow.push(crate::flow::FlowCont::AfterStageFileRun);
        // 1) ラベル名として試す
        if crate::event_runtime::trigger_label(self, &next) {
            return true;
        }
        // 2) ファイル名 (basename 一致) として試す。
        //    原典 SRC `StartScenario` 準拠: ステージファイルは「プロローグ →
        //    (begin_battle 経由で) スタート」の順で起動する。`マップコマンド X:`
        //    / `進入 ...:` / `ターン N:` 等はゲームイベントで発火する **ハンドラ
        //    定義** であり、ロード時に本体を実行してはいけない。
        //
        //    ファイル先頭から run_from_pc すると、先頭がこれらハンドラ定義の
        //    ときにその本体を開幕にインライン実行してしまう (東方夢想伝01:
        //    冒頭の `マップコマンド ボーナス条件確認` 本体 = ボーダー説明 Talk が
        //    ステージ開始前に誤再生され、本来の `プロローグ` 幻想郷ナレーションも
        //    飛ばされる)。プロローグがあればそれを起動し、無ければ従来どおり
        //    ファイル先頭から実行する。スタート発火は AfterStageFileRun 継続が担う。
        if self.script_library().find_file(&next).is_some() {
            if crate::event_runtime::trigger_label_in_file(self, &next, "プロローグ")
                || crate::event_runtime::trigger_label_in_file(self, &next, "Prologue")
            {
                return true;
            }
            // プロローグ無し: ラベルを持たない薄いステージ / top-level コード型は
            // ファイル先頭から実行する (従来挙動)。
            let file_start_pc = self.script_library().find_file(&next).map(|e| e.start_pc);
            if let Some(pc) = file_start_pc {
                let _ = crate::event_runtime::run_from_pc(self, pc);
                return true;
            }
        }
        // 何も起動しなかった → 積んだ継続を取り下げ (stale 防止)、予約を復元
        // する。解決失敗 (ラベル/ファイル未登録 = 壊れたシナリオや外部ファイル
        // 参照) を握り潰して予約ごと消すと、呼び出し側/フロントエンドが後から
        // 再試行・診断できなくなるため。
        if self.flow.last() == Some(&crate::flow::FlowCont::AfterStageFileRun) {
            self.flow.pop();
        }
        self.set_script_var("次ステージ".to_string(), next);
        false
    }

    pub(super) fn end_phase(&mut self) -> bool {
        if self.scene != Scene::MapView {
            return false;
        }
        if self.stage_state != crate::stage::StageState::Battle {
            return false; // ブリーフィング中などはフェーズを進めない
        }
        // 逐次演出モード: 敵フェイズを開始してランナーを初期化し、以降は `tick` が
        // 1 体ずつ駆動する (SRC.cs `StartTurn` の CPU ループ相当)。
        if self.animate_ai {
            self.begin_ai_phase(crate::Phase::Enemy);
            return true;
        }
        // 同期モード (ヘッドレス/テスト): 元 SRC `StartTurn("味方")` の do/loop に対応。
        //   Player の "ターン終了" 後、敵 → 中立 → ＮＰＣ を順に処理し
        //   それぞれ AI を回したら、ターン数を +1 して Player に戻す。
        // 無限ループ防止のため最大 4 フェーズ進める。
        for _ in 0..4 {
            let next = self.turn.phase.next();
            // ＮＰＣ → Player でラップするときに turn 数を進める
            if next == crate::Phase::Player {
                self.turn.number = self.turn.number.saturating_add(1);
            }
            self.begin_phase(next);
            if self.turn.phase == crate::Phase::Player {
                break;
            }
            // 元 SRC `CPUOperation(uparty)` 相当
            self.run_ai_phase();
            self.check_victory();
            if matches!(
                self.stage_state,
                crate::stage::StageState::Victory | crate::stage::StageState::Defeat
            ) {
                return true;
            }
        }
        true
    }

    /// 現ステージファイルが `label` を定義しているか (戦闘終了イベントの委譲判定用)。
    /// `current_stage_file` 未設定時は global にフォールバック。
    pub(super) fn stage_defines_label(&self, label: &str) -> bool {
        if self.current_stage_file.is_empty() {
            return self.script_library.label_pc(label).is_some();
        }
        self.script_library
            .label_pc_in_file(&self.current_stage_file, label)
            .is_some()
    }

    pub(crate) fn check_victory(&mut self) {
        use crate::stage::StageState;
        if self.stage_state != StageState::Battle {
            return;
        }
        if self.database.unit_instances.is_empty() {
            return;
        }
        // off_map (Escape 退避中) のユニットは勝敗判定から除外。
        let player_alive = self
            .database
            .unit_instances
            .iter()
            .any(|u| u.party == crate::Party::Player && !u.off_map);
        // 味方全滅 = 敗北。敵/中立の生死に関わらず確実に発火する。
        //
        // 旧実装は「味方も敵も全滅 → 引き分けで無処理」で early return しており、
        // 敵だけ先に全滅して敵対する中立が残る局面で味方全滅すると `!味方 && !敵`
        // が真になり敗北を発火できず詰んでいた (ユーザ報告: 中立フェイズで味方が
        // 倒されても敗北せず進行不能)。中立は SRC のキャンプ仕様で味方に敵対する
        // ため、味方が居なくなれば一律敗北とする。
        if !player_alive {
            self.game_over();
            return;
        }
        // 勝利判定: 味方に敵対する陣営 (敵 + 中立) が全滅したか。中立を勝利条件に
        // 含めないと、敵対中立が残るのに早期勝利してしまう。
        let hostile_alive =
            self.database.unit_instances.iter().any(|u| {
                !u.off_map && matches!(u.party, crate::Party::Enemy | crate::Party::Neutral)
            });
        if !hostile_alive {
            // シナリオの `全滅 敵` / `全滅 中立` ハンドラは破壊イベント経由
            // (`fire_destruction_labels` → `fire_total_annihilation_if_any`) で発火する。
            // それが勝利演出 (Talk) や次ステージ (Continue) を出していれば「idle ではない」
            // ので組込み勝利は発火しない (委譲)。
            //
            // 逆に、ハンドラが無い / 空 / 進行を解決しないために **敵全滅 & Battle のまま
            // idle** (dialog / script / flow / event_queue いずれも無し) になった場合は、
            // 組込み `game_clear` (= `クリア` / `Victory` 発火) を呼んで soft-lock を防ぐ。
            // 旧実装は `全滅 敵` 定義時に無条件委譲し、ハンドラが進行させないと敵全滅後も
            // Battle のまま詰んでいた (実機報告: 東方夢想伝01)。idle 判定で進行中の勝利
            // 演出は中断しない。
            let scenario_handles_victory =
                self.stage_defines_label("全滅 敵") || self.stage_defines_label("全滅 中立");
            let idle = self.pending_dialog.is_none()
                && !self.has_script_context()
                && self.flow.is_empty()
                && self.event_queue.is_empty();
            if !scenario_handles_victory || idle {
                self.game_clear();
            }
        }
    }

    /// 配置済み (on_map) の最初の味方ユニットへカーソルを置き、ビューをスクロール
    /// して画面内に収める。戦闘開始時に味方が画面外に配置されていても見えるように。
    pub(super) fn center_view_on_first_player_unit(&mut self) {
        if let Some((x, y)) = self
            .database
            .unit_instances
            .iter()
            .find(|u| u.party == crate::Party::Player && !u.off_map)
            .map(|u| (u.x, u.y))
        {
            self.map_cursor = Some((x, y));
            self.ensure_cursor_visible();
        }
    }

    /// インターミッション項目から起動したサブコマンド (.eve) のスクリプトが
    /// 完了したら `Scene::Intermission` に戻す。スクリプトがまだ pending
    /// 原典 SRC 互換: Prologue が完了したら、ユーザ入力を待たずに
    /// `スタート` ラベルを発火して Battle 状態へ移行する。
    ///
    /// 原典 [`スタートイベント.md`](../../SRC.Sharp/SRC.Sharp.Help/src/スタートイベント.md):
    /// 「プロローグイベントの後にメインウィンドウが表示され、スタートイベントが
    /// 発生します。スタートイベントが終了するとプレイヤーの操作が可能になり、
    /// 戦闘が開始されます」— ユーザに `Enter で出撃準備` を押させる仕様は無い。
    ///
    /// idle (`script_ctx` 無し / `pending_dialog` 無し / `pending_timer` 無し /
    /// インターミッション中でない) のときに Briefing → Sortie → Battle を
    /// 一気に進める。
    /// `respond_dialog` / `respond_dialog_text` / `tick` の resume 後 と
    /// `start_scenario` 末尾から呼ばれる。
    pub fn auto_progress_stage_state_if_idle(&mut self) {
        // flow 継続が残っていれば on_script_completed が処理する。
        // ここで begin_battle を呼ぶと スタート ラベルが二重発火する。
        if !self.flow.is_empty() {
            return;
        }
        // シナリオが実際に起動しているかの判定:
        //  - `stage` 非空: `Stage` コマンド / `start_scenario` で表示名が付いている。
        //  - `current_stage_file` 非空: `Continue` チェインで本編ステージファイルに
        //    入っている (`advance_to_next_stage` が設定)。
        // どちらも空 = テストで `App::new()` 直後に直接スクリプトを動かす類の合成
        // ケースなので auto-progress しない (期待を壊さない)。
        // 旧実装は `stage` のみで判定していたが、`Stage` コマンドを使わず `Continue`
        // チェインだけで本編に入るシナリオ (musou 系) は `stage` が空のままで、本編
        // 突入後も自動進行できず「マップが読み込まれていません」で詰んでいた。
        if self.stage.is_empty() && self.current_stage_file.is_empty() {
            return;
        }
        // 「インターミッション中」の判定は `intermission_running` (サブコマンド実行中)
        // と `scene == Intermission` (メニュー表示中) で行う。
        // `intermission_commands` の登録有無は判定に使わない: インターミッション制
        // シナリオでは登録がシナリオ全体で残り続けるため、「登録あり = 進めない」に
        // すると本編 (MapView) に入った後も Briefing のまま `スタート` が発火せず
        // 詰む (musou 系の `Continue` チェイン後の症状)。
        if self.has_script_context()
            || self.pending_dialog.is_some()
            || self.pending_timer.is_some()
            || self.intermission_running
            || matches!(self.scene, Scene::Intermission)
        {
            return;
        }
        if self.stage_state == crate::stage::StageState::Briefing {
            self.begin_sortie();
        }
        // begin_battle は `スタート` ラベルを発火する。それが Talk 等で
        // suspend したら以降の自動進行は止まり、ユーザが応答 → resume → 再度
        // ここに戻ってくる経路で進む。
        if self.stage_state == crate::stage::StageState::Sortie {
            self.begin_battle();
        }
    }
}
