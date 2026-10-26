//! スクリプト実行コンテキストの保存/再開、イベントキュー、スクリプト完了時の遷移。

use super::*;

impl App {
    /// 内部用: 中断中のスクリプトを保存 / 取り出し。
    pub fn take_script_context(&mut self) -> Option<crate::event_runtime::ScriptContext> {
        self.script_ctx.take()
    }

    pub fn set_script_context(&mut self, ctx: crate::event_runtime::ScriptContext) {
        self.script_ctx = Some(ctx);
    }

    pub fn has_script_context(&self) -> bool {
        self.script_ctx.is_some()
    }

    /// 中断中スクリプトの再開 PC。`Wait` / 対話命令でサスペンド中のみ `Some`。
    /// 同一 PC で何度もサスペンドし続ける = スクリプトが進行不能ループに
    /// 陥っている指標になる (test_harness の `Drain` が利用)。
    pub fn script_resume_pc(&self) -> Option<usize> {
        self.script_ctx.as_ref().map(|c| c.pc)
    }

    /// スクリプト完了通知 (docs/FLOW_REDESIGN.md §2.1)。
    ///
    /// `event_runtime::run_loop` がスクリプトの完了 (インライン完了 / resume
    /// 後の完了 / エラー終了) を検知するたびに呼ぶ。idle (スクリプト/対話/
    /// タイマのいずれも無い) な間、`flow` スタックの継続を pop して実行する。
    /// 継続の実行が新たなスクリプトを起動して suspend したら drain は止まり、
    /// そのスクリプトの完了時に再開される。
    ///
    /// 継続実行中に内側で完了したスクリプトからの再帰呼び出しは
    /// `flow_draining` ガードで弾く (外側のループが続きを処理する)。
    pub fn on_script_completed(&mut self) {
        if self.flow_draining {
            return;
        }
        // スクリプト実行中 (ネストした run_loop の完了など) は drain しない。
        // 最外殻の run_loop が完了したときにまとめて処理する。
        if self.script_depth > 0 {
            return;
        }
        self.flow_draining = true;
        // 暴走防止: `Continue` 同士のループバック (A→B→A…) のような
        // 終わらないチェインで WASM がハングしないよう、1 回の drain で
        // 処理する継続/イベント数に上限を置く (正常系では到達しない)。
        let mut steps: usize = 0;
        const DRAIN_STEP_LIMIT: usize = 256;
        while self.script_ctx.is_none()
            && self.pending_dialog.is_none()
            && self.pending_timer.is_none()
        {
            steps += 1;
            if steps > DRAIN_STEP_LIMIT {
                self.push_message(
                    "進行継続の処理が上限を超えました (Continue ループの可能性)".to_string(),
                );
                break;
            }
            // 割込みイベント (EventQue) を継続より先に消化する。原典の
            // 「HandleEvent はキューを処理し切ってから呼び出し元へ戻る」
            // (Event.bas) と同じ順序。
            if let Some((label, scope)) = self.event_queue.pop_front() {
                match scope {
                    // 章ローカルイベント: 当該ファイル内のラベルとして発火する。
                    // ファイル内に無ければ何もしない (他章へ漏らさない)。
                    Some(file) => {
                        crate::event_runtime::trigger_label_in_file(self, &file, &label);
                    }
                    None => {
                        crate::event_runtime::trigger_label(self, &label);
                    }
                }
                continue;
            }
            let Some(cont) = self.flow.pop() else {
                break;
            };
            self.run_flow_cont(cont);
        }
        self.flow_draining = false;
    }

    /// 割込みイベントを投函する (原典 `Event.bas::EventQue` への追加に相当)。
    ///
    /// `label` が script_library に定義されていればキューに積んで `true`。
    /// 未定義なら何もせず `false` (呼び出し側の「候補を順に試して最初の 1 件
    /// だけ発火」ロジックのため、存在判定は投函時に行う)。
    ///
    /// スクリプト実行中でなければ投函と同時に drain して即実行する
    /// (従来の即時 `trigger_label` と同じタイミング)。実行中なら現在の
    /// スクリプト完了後に FIFO で実行される (再入による ctx 上書きを防ぐ)。
    pub(crate) fn post_event_label(&mut self, label: String) -> bool {
        if self.script_library.label_pc(&label).is_none() {
            return false;
        }
        self.event_queue.push_back((label, None));
        if self.script_depth == 0 {
            self.on_script_completed();
        }
        true
    }

    /// 章ローカルな自動発火イベント (`ターン` / `破壊` / `全滅` / `損傷率` /
    /// `会話` / `勝利条件` 等) を **現ステージファイルにスコープして** 投函する。
    ///
    /// `post_event_label` (global) と違い、現ステージファイル内に `label` が
    /// 定義されていなければ投函しない (`false`)。全 22 章を 1 ライブラリに同時
    /// ロードする本実装では、章ローカルイベントを global 解決すると、現章に
    /// 当該イベントが無いとき別章の同名ラベル (例: 01 章の敵フェイズで `ターン
    /// 1 敵` を引くと 12 章のそれにヒット) が誤発火して「話が飛ぶ」。原典 SRC は
    /// シナリオを 1 本ずつロードするため衝突しない挙動を、ファイルスコープで再現する。
    ///
    /// `current_stage_file` が未設定 (単一ファイル / テスト) の場合は従来どおり
    /// global 解決にフォールバックする。
    pub(crate) fn post_stage_event_label(&mut self, label: String) -> bool {
        if self.current_stage_file.is_empty() {
            // ステージファイル概念が無い経路: 従来どおり global。
            return self.post_event_label(label);
        }
        if self
            .script_library
            .label_pc_in_file(&self.current_stage_file, &label)
            .is_none()
        {
            return false;
        }
        let file = self.current_stage_file.clone();
        self.event_queue.push_back((label, Some(file)));
        if self.script_depth == 0 {
            self.on_script_completed();
        }
        true
    }

    /// 指定ファイル (`file` = basename / パス。例: `GameOver.eve`) 内の `label` を
    /// EventQue にスコープ投函する。[`post_stage_event_label`] の任意ファイル版で、
    /// `current_stage_file` に依らず特定の .eve のラベルを発火したいとき (SRC 本体の
    /// `Data/System/GameOver.eve::プロローグ` 等) に使う。
    ///
    /// 当該ファイルに `label` が無ければ投函せず `false`。
    pub(crate) fn post_event_label_in_file(&mut self, file: &str, label: String) -> bool {
        if self.script_library.label_pc_in_file(file, &label).is_none() {
            return false;
        }
        self.event_queue.push_back((label, Some(file.to_string())));
        if self.script_depth == 0 {
            self.on_script_completed();
        }
        true
    }

    /// スクリプトが要求した再ロード用 JSON を登録する (`Quickload` / `Restart` 等)。
    pub(crate) fn request_reload(&mut self, json: String) {
        self.pending_reload = Some(json);
    }

    /// フロントエンドが再ロード要求を取り出す。`Some(json)` なら
    /// [`App::from_save_json`] で `self` を置換し [`App::fire_resume_event`] を呼ぶこと。
    pub fn take_pending_reload(&mut self) -> Option<String> {
        self.pending_reload.take()
    }

    /// `event_runtime::run_loop` の実行ネスト深さの増減。
    pub(crate) fn enter_script_run(&mut self) {
        self.script_depth += 1;
    }

    pub(crate) fn exit_script_run(&mut self) {
        self.script_depth = self.script_depth.saturating_sub(1);
    }

    pub(crate) fn script_run_depth(&self) -> usize {
        self.script_depth
    }

    /// 継続 1 件の実行。VB5 原典の「`HandleEvent` から戻った直後のコード」に相当。
    pub(super) fn run_flow_cont(&mut self, cont: crate::flow::FlowCont) {
        use crate::flow::FlowCont;
        match cont {
            FlowCont::AfterStartEvent => {
                // 原典 SRC.bas `StartScenario` 末尾: HandleEvent "スタート" の
                // 完了後に StartTurn "味方"。
                self.begin_phase(crate::Phase::Player);
                self.center_view_on_first_player_unit();
            }
            FlowCont::ReturnToIntermissionMenu => {
                self.intermission_running = false;
                self.scene = Scene::Intermission;
                self.intermission_mode = IntermissionMode::Menu;
                self.script_overlay.clear();
                self.hotpoints.clear();
            }
            FlowCont::AfterStageFileRun => {
                // `Continue` ループバック (ステージファイルが `次ステージ` を
                // 再予約した) 場合はインターミッションに留まる。
                if !self.script_var("次ステージ").is_empty() {
                    return;
                }
                if !matches!(
                    self.stage_state,
                    crate::stage::StageState::Briefing | crate::stage::StageState::Sortie
                ) {
                    return;
                }
                if self.consume_stage_start_ran() {
                    // ステージファイルが `スタート` ラベルを通過実行済み →
                    // 再発火せず (敵の二重配置防止) Battle 状態に入って
                    // 味方フェイズを開始する。
                    self.enter_battle_state();
                    self.begin_phase(crate::Phase::Player);
                    self.center_view_on_first_player_unit();
                } else {
                    // `スタート` 未実行 → 通常経路で発火する (`begin_battle` が
                    // `current_stage_file` スコープで `スタート` を起動し、
                    // 完了後に AfterStartEvent 継続が味方フェイズを開始する)。
                    self.begin_sortie();
                    self.begin_battle();
                }
            }
            FlowCont::LoadNextStage => {
                // 原典: IsScenarioFinished → StartScenario(次ステージ)。
                // 予約が無ければ no-op (シナリオ完結)。
                let _ = self.advance_to_next_stage();
            }
        }
    }

    /// シナリオ遷移 (`Continue <file>` / 大域中断) に伴う進行状態の巻き戻し。
    /// 原典の `IsScenarioFinished` によるコールスタック巻き戻しに相当し、
    /// 旧ステージの継続 (AfterStartEvent 等) と未消化の割込みイベントを
    /// 破棄する (docs/FLOW_REDESIGN.md §2.2 規則 3)。
    pub(crate) fn scenario_transition_reset(&mut self) {
        self.flow.clear();
        self.event_queue.clear();
    }

    /// flow 継続を積む (event_runtime / オーケストレーション用)。
    pub fn push_flow_cont(&mut self, cont: crate::flow::FlowCont) {
        self.flow.push(cont);
    }

    /// `スタート` / `Start` ラベル行 (PC) をスクリプト実行が通過した
    /// (= スタートイベントの中身がインライン実行された) ことを記録する。
    /// event_runtime::run_loop_inner から呼ばれる。
    pub(crate) fn mark_start_label_passed(&mut self, pc: usize) {
        self.start_passed_pcs.push(pc);
    }

    /// 現ステージ (`current_stage_file`) の `スタート` セクションがインライン
    /// 実行済みかを判定し、記録を消費 (クリア) する。
    /// `current_stage_file` が未設定 / library 未登録の場合は「いずれかの
    /// `スタート` を通過したか」にフォールバックする (start_scenario 等の
    /// ファイル概念が無い経路)。
    pub(super) fn consume_stage_start_ran(&mut self) -> bool {
        let ran = if self.current_stage_file.is_empty() {
            !self.start_passed_pcs.is_empty()
        } else if let Some((s, e)) = self
            .script_library
            .find_file(&self.current_stage_file)
            .map(|f| (f.start_pc, f.end_pc))
        {
            self.start_passed_pcs.iter().any(|&pc| pc >= s && pc < e)
        } else {
            !self.start_passed_pcs.is_empty()
        };
        self.start_passed_pcs.clear();
        ran
    }

    /// アーカイブロード完了後のステージブートストラップ (フロントエンド /
    /// verify-archive のロードオーケストレーションから呼ぶ)。
    ///
    /// `Stage` コマンドも `Continue` チェインも使わないシナリオ (エントリ
    /// .eve のプロローグ実行だけで終わる型。例: 東中無双２) は、原典
    /// `StartScenario` の後半「プロローグ完了 → `スタート` 発火 → 味方
    /// フェイズ」を駆動する者が居らず Briefing で停止していた。エントリ
    /// .eve を現ステージファイルとして `AfterStageFileRun` 継続を積み、
    /// idle なら即進行する (プロローグが suspend 中なら完了時に進行)。
    pub fn bootstrap_stage_after_load(&mut self, entry_file: &str) {
        // インターミッション制はメニュー操作 (「次のステージへ」) で進行する。
        //
        // ただし `IntermissionCommand` は「インターミッションのメニューに項目を
        // 追加する」だけの命令であり (`IntermissionCommandコマンド.md`)、
        // シナリオがインターミッション制であることを意味しない。ステージ制の
        // シナリオもプロローグで辞典・ショップ等を登録するのに使う
        // (例: 多次元世界エニバース の `IntermissionCommand "エニバース辞典" …`)。
        // 起動ファイルが `スタート` / `Start` を持つならステージ制と判断して
        // ブートストラップする。
        if !self.intermission_commands.is_empty() && !self.file_has_start_label(entry_file) {
            return;
        }
        if self.stage_state != crate::stage::StageState::Briefing {
            return;
        }
        // 既にチェイン進行中 (`Continue` が LoadNextStage を予約 / advance が
        // AfterStageFileRun を積載) なら二重に積まない。
        if !self.flow.is_empty() {
            return;
        }
        // ユニット/パイロット定義がなければ素材パック (ライブラリ .eve のみ収録)
        // であり、シナリオとしてブートストラップする必要がない。
        if self.database.units.is_empty() && self.database.pilots.is_empty() {
            return;
        }
        if self.current_stage_file.is_empty() && !entry_file.is_empty() {
            self.current_stage_file = entry_file.to_string();
        }
        self.flow.push(crate::flow::FlowCont::AfterStageFileRun);
        self.on_script_completed();
    }

    /// 指定ファイルが `スタート` / `Start` ラベルを定義しているか。
    pub(super) fn file_has_start_label(&self, file_path: &str) -> bool {
        if file_path.is_empty() {
            return false;
        }
        let lib = self.script_library();
        lib.label_pc_in_file(file_path, "スタート").is_some()
            || lib.label_pc_in_file(file_path, "Start").is_some()
    }
}
