//! E2E シナリオテストハーネス / End-to-end scenario test harness.
//!
//! 「配線不足」バグ（パーサに新コマンドを追加したが実行ハンドラが no-op /
//! State 更新したが描画反映漏れ / `pending_*` の遷移漏れ）を捕まえるための
//! 統合テスト基盤。コマンド単体ではなく、`.eve` を一気通貫で実行して
//! App の最終状態をスナップショット比較する。
//!
//! # 使い方
//!
//! ```no_run
//! use src_core::test_harness::{Harness, Step};
//!
//! let mut h = Harness::from_eve_source(r#"
//! Confirm 続けますか
//! If $(選択) = 1 Then
//!   Message yes
//! EndIf
//! "#).unwrap();
//! h.drive(&[Step::Yes]).unwrap();
//! assert!(h.snapshot().contains("選択 = 1"));
//! ```
//!
//! integration test (`crates/src-core/tests/scenarios.rs`) は
//! `tests/fixtures/scenarios/<name>.eve` を読んで
//! `tests/fixtures/scenarios/<name>.expected` と比較する。
//! `.steps` がある場合は事前にドライバ入力として食わせる。
//!
//! 環境変数 `SRC_UPDATE_SNAPSHOTS=1` で expected ファイルを actual で上書き。

use std::fmt::Write as _;

use crate::data::event;
use crate::dialog::PendingDialog;
use crate::event_runtime::{self, ScriptError};
use crate::App;

/// シナリオ駆動の 1 ステップ。
/// `.steps` テキストファイル形式と 1:1 対応する。
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    /// `Talk` / `Confirm`(Yes) — `respond_dialog(0)` 相当。
    Yes,
    /// `Confirm`(No) — `respond_dialog(1)` 相当。
    No,
    /// `Menu` の選択 (1-indexed)。0 はキャンセル扱い。
    Choice(u32),
    /// `Input` のテキスト応答。
    Text(String),
    /// `Wait <duration>` 中の経過時間 (秒)。`App::tick(dt)` を呼ぶ。
    Tick(f64),
    /// `Talk` 進行 (Yes と等価) / dialog 無時は no-op の汎用「次へ」。
    Advance,
    /// canvas 上での絶対座標クリック。Hotpoint 経由 Menu の hit-test を
    /// 期待するときに使う。Menu 以外の dialog で来た場合は無視。
    ClickAt { x: i32, y: i32 },
    /// 名前一致する Hotpoint を 1 つ探してその中心をクリックする糖衣。
    Hotpoint(String),
    /// ライブラリ登録済みラベルから新規実行コンテキストを起動。
    /// 自動発火 (`Start` / `Turn N`) を test 側で起こすのに使う。
    TriggerLabel(String),
    /// セーブ → 即ロードして round-trip を実施。
    /// 配線漏れ（`#[serde(skip)]` 残り）を炙り出すのに有効。
    SaveLoadRoundTrip,
    /// 最大 N 回まで「dialog が立っていれば Advance、timer なら大きな Tick」を
    /// 繰り返して `ModalGate::Open` に到達させる汎用ステップ。
    /// 自走するシナリオ (`Talk` / `Wait` / `Goto` だけで完結する) の自動完走テストに使う。
    ///
    /// 同一 `ModalGate` が連続して再提示された場合 (= 進行不能ループ。
    /// 例: スパロボ戦記タイトルの hotpoint Menu で選択肢 0 が
    /// `Goto ＯＰ再描画` を踏み続ける) は `max` まで回さず早期に打ち切る。
    Drain(u32),
}

/// シナリオハーネスの実行結果。
#[derive(Debug, Clone, PartialEq)]
pub enum DriveOutcome {
    /// 全ステップ消化済み、`pending_dialog` も `pending_timer` も無し。
    Finished,
    /// `pending_dialog` が残っているがステップ列が尽きた。
    StuckOnDialog(String),
    /// `pending_timer` が残っているがステップ列が尽きた。
    StuckOnTimer(f64),
    /// ステップが渡されたが応答できる pending state が無い。
    NoPendingForStep(Step),
}

/// ハーネス本体。`App` を内包し、`Step` 列を順に流し込んで snapshot を出す。
pub struct Harness {
    app: App,
}

impl Harness {
    /// `.eve` ソースをパース + 実行して `Harness` を生成する。
    /// 初回 `execute()` で pending dialog/timer が立ったまま戻る場合もある。
    pub fn from_eve_source(src: &str) -> Result<Self, ScriptError> {
        let stmts = event::parse(src).map_err(|e| ScriptError {
            line_num: e.line_num,
            message: e.message,
        })?;
        let mut app = App::with_rng_seed(0xC0FFEE_C0FFEE_u64);
        event_runtime::execute(&mut app, &stmts)?;
        Ok(Self { app })
    }

    /// 既存の `App` を取り込む（save/load round-trip 検証用）。
    pub fn from_app(app: App) -> Self {
        Self { app }
    }

    pub fn app(&self) -> &App {
        &self.app
    }

    pub fn app_mut(&mut self) -> &mut App {
        &mut self.app
    }

    /// ステップ列を順に適用する。最初の `NoPendingForStep` / エラーで中断。
    pub fn drive(&mut self, steps: &[Step]) -> Result<DriveOutcome, ScriptError> {
        for step in steps {
            match self.apply(step.clone())? {
                ApplyResult::Ok => {}
                ApplyResult::NotApplicable => {
                    return Ok(DriveOutcome::NoPendingForStep(step.clone()));
                }
            }
        }
        Ok(self.terminal_state())
    }

    fn terminal_state(&self) -> DriveOutcome {
        if let Some(d) = self.app.pending_dialog() {
            return DriveOutcome::StuckOnDialog(d.kind().to_string());
        }
        if let Some(t) = self.app.pending_timer() {
            return DriveOutcome::StuckOnTimer(t);
        }
        DriveOutcome::Finished
    }

    fn apply(&mut self, step: Step) -> Result<ApplyResult, ScriptError> {
        match step {
            Step::Yes | Step::Advance => {
                if self.app.pending_dialog().is_some() {
                    self.app.respond_dialog(0);
                    Ok(ApplyResult::Ok)
                } else if self.app.pending_timer().is_some() {
                    // Timer 中の Advance は no-op (Tick を使うべき) として扱う
                    Ok(ApplyResult::Ok)
                } else {
                    // pending 無し: Advance は黙って通す、Yes は不適合扱い
                    if matches!(step, Step::Advance) {
                        Ok(ApplyResult::Ok)
                    } else {
                        Ok(ApplyResult::NotApplicable)
                    }
                }
            }
            Step::No => {
                if self.app.pending_dialog().is_some() {
                    self.app.respond_dialog(1);
                    Ok(ApplyResult::Ok)
                } else {
                    Ok(ApplyResult::NotApplicable)
                }
            }
            Step::Choice(n) => {
                if self.app.pending_dialog().is_some() {
                    self.app.respond_dialog(n);
                    Ok(ApplyResult::Ok)
                } else {
                    Ok(ApplyResult::NotApplicable)
                }
            }
            Step::Text(s) => {
                if self.app.respond_dialog_text(s) {
                    Ok(ApplyResult::Ok)
                } else {
                    Ok(ApplyResult::NotApplicable)
                }
            }
            Step::Tick(dt) => {
                self.app.tick(dt);
                Ok(ApplyResult::Ok)
            }
            Step::ClickAt { x, y } => {
                // `try_hotpoint_click` 相当を public な API だけで再現:
                // pending Menu の場合に hotpoint と当たり判定。
                let Some(idx) = self.hit_hotpoint(x, y) else {
                    return Ok(ApplyResult::NotApplicable);
                };
                self.app.respond_dialog((idx as u32) + 1);
                Ok(ApplyResult::Ok)
            }
            Step::Hotpoint(name) => {
                let Some((idx, cx, cy)) = self
                    .app
                    .hotpoints()
                    .iter()
                    .enumerate()
                    .find(|(_, h)| h.name == name)
                    .map(|(i, h)| (i, h.x + h.w / 2, h.y + h.h / 2))
                else {
                    return Ok(ApplyResult::NotApplicable);
                };
                let _ = (cx, cy); // 中心は概念だけ控え、応答はインデックスで
                self.app.respond_dialog((idx as u32) + 1);
                Ok(ApplyResult::Ok)
            }
            Step::TriggerLabel(name) => {
                let fired = event_runtime::trigger_label(&mut self.app, &name);
                if fired {
                    Ok(ApplyResult::Ok)
                } else {
                    Ok(ApplyResult::NotApplicable)
                }
            }
            Step::SaveLoadRoundTrip => {
                let json = self.app.to_save_json().map_err(|m| ScriptError {
                    line_num: 0,
                    message: format!("save error: {m}"),
                })?;
                let restored = App::from_save_json(&json).map_err(|m| ScriptError {
                    line_num: 0,
                    message: format!("load error: {m}"),
                })?;
                self.app = restored;
                Ok(ApplyResult::Ok)
            }
            Step::Drain(max) => {
                use crate::modal::ModalGate;
                use std::collections::HashMap;
                // スクリプトが同じ再開 PC で何度もサスペンドし続けたら
                // 「進行不能ループ」とみなして打ち切る。
                //
                // スパロボ戦記タイトル画面は選択肢 0 が `Goto ＯＰ再描画` を
                // 踏み続ける無限ループで、各周回が数百の `Set`/`PaintPicture`
                // を再実行する (overlay/hotpoint が際限なく伸び、コストは
                // 周回数に対して超線形)。4096 回回すと数十分かかる。
                //
                // 一方、正当な `Wait` カウントダウンループ (例: プロローグの
                // pc=86446) も同一 PC を繰り返すが高々十数回で抜ける。観測した
                // 最大は 15 回なので、十分な余裕を見て 32 回を上限とする。
                // 直列に並んだ `Wait` 列はそれぞれ別 PC なので影響を受けない。
                const LOOP_LIMIT: u32 = 32;
                // 再開 PC が取れない (script_ctx 無し) 場合のフォールバック鍵。
                const NO_PC: usize = usize::MAX;
                let mut pc_seen: HashMap<usize, u32> = HashMap::new();
                for _ in 0..max {
                    let gate = self.app.modal_gate();
                    if matches!(gate, ModalGate::Open) {
                        return Ok(ApplyResult::Ok);
                    }
                    let pc = self.app.script_resume_pc().unwrap_or(NO_PC);
                    let hits = pc_seen.entry(pc).or_insert(0);
                    *hits += 1;
                    if *hits >= LOOP_LIMIT {
                        // 同一 PC ループ。これ以上回しても Open には到達しない。
                        return Ok(ApplyResult::Ok);
                    }
                    match gate {
                        ModalGate::Open => unreachable!(),
                        ModalGate::Dialog(_) | ModalGate::DialogOverTimer { .. } => {
                            self.app.respond_dialog(0);
                        }
                        ModalGate::Timer(_) => {
                            // 60 秒分一気に進める (実シナリオの Wait は通常 0.5-10 秒)。
                            self.app.tick(60.0);
                        }
                    }
                }
                // 最大反復後も Open に到達しない場合、ステップとしては Ok を返す
                // (terminal_state で Stuck* が出る)。
                Ok(ApplyResult::Ok)
            }
        }
    }

    fn hit_hotpoint(&self, x: i32, y: i32) -> Option<usize> {
        if !matches!(self.app.pending_dialog(), Some(PendingDialog::Menu { .. })) {
            return None;
        }
        self.app
            .hotpoints()
            .iter()
            .enumerate()
            .rev()
            .find(|(_, h)| x >= h.x && x < h.x + h.w && y >= h.y && y < h.y + h.h)
            .map(|(i, _)| i)
    }

    /// 現在の App 状態を diff 比較しやすい text snapshot に整形する。
    pub fn snapshot(&self) -> String {
        format_snapshot(&self.app)
    }
}

enum ApplyResult {
    Ok,
    NotApplicable,
}

/// `.steps` テキスト形式パーサ。1 行 1 ステップ、`#` 始まりはコメント。
///
/// 受理する形式:
/// - `yes` / `no` / `advance`
/// - `choice <n>` (1-indexed)
/// - `text <文字列>` (改行までを文字列として取り込む)
/// - `tick <秒数>`
/// - `clickat <x> <y>`
/// - `hotpoint <名前>`
/// - `trigger <ラベル>`
/// - `saveload` (save→load round-trip)
pub fn parse_steps(src: &str) -> Result<Vec<Step>, String> {
    let mut out = Vec::new();
    for (idx, raw) in src.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (head, rest) = match line.find(char::is_whitespace) {
            Some(i) => (&line[..i], line[i..].trim()),
            None => (line, ""),
        };
        let step = match head.to_ascii_lowercase().as_str() {
            "yes" => Step::Yes,
            "no" => Step::No,
            "advance" => Step::Advance,
            "choice" => {
                let n: u32 = rest
                    .parse()
                    .map_err(|_| format!("{}行目: choice の引数は整数: {rest}", idx + 1))?;
                Step::Choice(n)
            }
            "text" => Step::Text(rest.to_string()),
            "tick" => {
                let dt: f64 = rest
                    .parse()
                    .map_err(|_| format!("{}行目: tick の引数は数値: {rest}", idx + 1))?;
                Step::Tick(dt)
            }
            "clickat" => {
                let mut it = rest.split_ascii_whitespace();
                let x: i32 = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| format!("{}行目: clickat x の解釈失敗", idx + 1))?;
                let y: i32 = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .ok_or_else(|| format!("{}行目: clickat y の解釈失敗", idx + 1))?;
                Step::ClickAt { x, y }
            }
            "hotpoint" => Step::Hotpoint(rest.to_string()),
            "trigger" => Step::TriggerLabel(rest.to_string()),
            "saveload" => Step::SaveLoadRoundTrip,
            "drain" => {
                let n: u32 = rest
                    .parse()
                    .map_err(|_| format!("{}行目: drain の引数は整数: {rest}", idx + 1))?;
                Step::Drain(n)
            }
            other => return Err(format!("{}行目: 未対応のステップ: {other}", idx + 1)),
        };
        out.push(step);
    }
    Ok(out)
}

/// snapshot 整形。決定的になるよう sort/format を厳密に固定。
fn format_snapshot(app: &App) -> String {
    let mut out = String::new();

    let _ = writeln!(out, "== scene ==");
    let _ = writeln!(out, "scene = {:?}", app.scene());
    let _ = writeln!(out, "stage = {}", app.stage());
    let _ = writeln!(out, "stage_state = {:?}", app.stage_state());
    let _ = writeln!(out, "turn = {} / {:?}", app.turn().number, app.turn().phase);
    let _ = writeln!(out, "money = {}", app.money());
    let _ = writeln!(out, "briefing = {}", one_line(app.briefing()));

    let _ = writeln!(out);
    let _ = writeln!(out, "== modal ==");
    let _ = writeln!(out, "modal_gate = {}", app.modal_gate().kind_label());
    match app.pending_dialog() {
        None => {
            let _ = writeln!(out, "pending_dialog = None");
        }
        Some(PendingDialog::Talk { speaker, body }) => {
            let _ = writeln!(out, "pending_dialog = Talk");
            let _ = writeln!(out, "  speaker = {speaker}");
            let _ = writeln!(out, "  body = {}", one_line(body));
        }
        Some(PendingDialog::WaitClick) => {
            let _ = writeln!(out, "pending_dialog = WaitClick");
        }
        Some(PendingDialog::Confirm { question, var_name }) => {
            let _ = writeln!(out, "pending_dialog = Confirm");
            let _ = writeln!(out, "  question = {question}");
            let _ = writeln!(out, "  var_name = {var_name}");
        }
        Some(PendingDialog::Menu {
            prompt,
            options,
            var_name,
            store_value,
            ..
        }) => {
            let _ = writeln!(out, "pending_dialog = Menu");
            let _ = writeln!(out, "  prompt = {prompt}");
            let _ = writeln!(out, "  var_name = {var_name}");
            let _ = writeln!(out, "  store_value = {store_value}");
            for (i, o) in options.iter().enumerate() {
                let _ = writeln!(out, "  options[{i}] = {o}");
            }
        }
        Some(PendingDialog::Input {
            prompt,
            var_name,
            default,
        }) => {
            let _ = writeln!(out, "pending_dialog = Input");
            let _ = writeln!(out, "  prompt = {prompt}");
            let _ = writeln!(out, "  var_name = {var_name}");
            let _ = writeln!(out, "  default = {default}");
        }
    }
    match app.pending_timer() {
        None => {
            let _ = writeln!(out, "pending_timer = None");
        }
        Some(t) => {
            // 浮動小数の微小誤差を吸収するため小数 3 桁で固定表示。
            let _ = writeln!(out, "pending_timer = {:.3}", t);
        }
    }
    let audio = app.pending_audio();
    let _ = writeln!(out, "pending_audio = {}", audio.len());
    for (i, a) in audio.iter().enumerate() {
        let _ = writeln!(out, "  [{i}] {a:?}");
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "== messages ({}) ==", app.messages().len());
    for (i, m) in app.messages().iter().enumerate() {
        let _ = writeln!(out, "[{i}] {}", one_line(m));
    }

    let _ = writeln!(out);
    // 内部システム変数 (`__quicksave` / `__restart_save` / `__save_slot_*` 等)
    // は値が serialize 済 JSON の塊で snapshot ノイズになるため除外する。
    // `__` プレフィックスを SRC スクリプトが直接ユーザ変数名として使うことは
    // 通常想定外 (.eve 文法上 `__name` は ascii 変数として有効だが、本実装の
    // 内部キーとは衝突可能性が低い)。
    let visible_vars: Vec<(&String, &String)> = app
        .script_vars()
        .iter()
        .filter(|(k, _)| !k.starts_with("__"))
        .collect();
    let _ = writeln!(out, "== vars ({}) ==", visible_vars.len());
    // BTreeMap なので既に sorted。
    for (k, v) in &visible_vars {
        let _ = writeln!(out, "{k} = {}", one_line(v));
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "== units ({}) ==", app.database().unit_instances.len());
    // unit_instance は順序を持つので、配置順をそのまま出す。
    for (i, u) in app.database().unit_instances.iter().enumerate() {
        let unit_data = app
            .database()
            .units
            .iter()
            .find(|d| d.name == u.unit_data_name);
        let max_hp = unit_data.map(|d| d.hp).unwrap_or(0);
        let max_en = unit_data.map(|d| d.en).unwrap_or(0);
        let cur_hp = max_hp.saturating_sub(u.damage);
        let cur_en = max_en.saturating_sub(u.en_consumed);
        let statuses = if u.conditions.is_empty() {
            String::new()
        } else {
            let names: Vec<_> = u.conditions.iter().map(|c| c.name.as_str()).collect();
            format!(" status=[{}]", names.join(","))
        };
        let acted = if u.has_acted { " has_acted" } else { "" };
        let off = if u.off_map { " off_map" } else { "" };
        let pilot = if u.pilot_name.is_empty() {
            String::from("(unmanned)")
        } else {
            u.pilot_name.clone()
        };
        let _ = writeln!(
            out,
            "[{i}] {}/{pilot} {:?} @({},{}) HP={}/{} EN={}/{} morale={}{}{}{}",
            u.unit_data_name,
            u.party,
            u.x,
            u.y,
            cur_hp,
            max_hp,
            cur_en,
            max_en,
            u.morale,
            statuses,
            acted,
            off,
        );
    }

    let _ = writeln!(out);
    let _ = writeln!(out, "== hotpoints ({}) ==", app.hotpoints().len());
    for (i, h) in app.hotpoints().iter().enumerate() {
        let _ = writeln!(
            out,
            "[{i}] {} @({},{},{},{}){}",
            h.name,
            h.x,
            h.y,
            h.w,
            h.h,
            if h.invisible { " invisible" } else { "" },
        );
    }

    out
}

fn one_line(s: &str) -> String {
    s.replace('\n', " \\n ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn confirm_yes_path() {
        let mut h = Harness::from_eve_source(
            "Confirm 続けますか\nIf $(選択) = 1 Then\n  Message yes\nElse\n  Message no\nEndIf\n",
        )
        .unwrap();
        assert_eq!(h.app().pending_dialog().map(|d| d.kind()), Some("Confirm"));
        let outcome = h.drive(&[Step::Yes]).unwrap();
        assert_eq!(outcome, DriveOutcome::Finished);
        assert_eq!(h.app().script_var("選択"), "1");
        assert_eq!(h.app().messages().last().unwrap(), "yes");
    }

    #[test]
    fn confirm_no_path() {
        let mut h =
            Harness::from_eve_source("Confirm ok\nIf $(選択) = 0 Then\n  Message n\nEndIf\n")
                .unwrap();
        h.drive(&[Step::No]).unwrap();
        assert_eq!(h.app().messages().last().unwrap(), "n");
    }

    #[test]
    fn menu_choice_select_value() {
        // Menu (Hotpoint 経由でない素の Menu) は store_value=false なので
        // 数値選択番号が変数に入る。
        let mut h =
            Harness::from_eve_source("Menu pick\n one\n two\n three\nEnd\nMessage $(選択)\n")
                .unwrap();
        h.drive(&[Step::Choice(2)]).unwrap();
        assert_eq!(h.app().script_var("選択"), "2");
    }

    #[test]
    fn input_text_response() {
        let mut h = Harness::from_eve_source("Input name 名前は default\n").unwrap();
        h.drive(&[Step::Text("リオ".to_string())]).unwrap();
        assert_eq!(h.app().script_var("name"), "リオ");
    }

    #[test]
    fn wait_timer_resumes_after_tick() {
        let mut h = Harness::from_eve_source("Set before 1\nWait 1.0\nSet after 1\n").unwrap();
        // Wait 中は pending_timer 残留
        assert!(h.app().pending_timer().is_some());
        assert_eq!(h.app().script_var("before"), "1");
        assert_eq!(h.app().script_var("after"), "");
        h.drive(&[Step::Tick(1.5)]).unwrap();
        assert!(h.app().pending_timer().is_none());
        assert_eq!(h.app().script_var("after"), "1");
    }

    #[test]
    fn parse_steps_basic() {
        let src = "# comment\nyes\nno\nchoice 3\ntext  リオ・カザミ\ntick 0.5\nadvance\nsaveload\n";
        let got = parse_steps(src).unwrap();
        assert_eq!(
            got,
            vec![
                Step::Yes,
                Step::No,
                Step::Choice(3),
                Step::Text("リオ・カザミ".to_string()),
                Step::Tick(0.5),
                Step::Advance,
                Step::SaveLoadRoundTrip,
            ]
        );
    }

    #[test]
    fn snapshot_is_stable_after_save_load() {
        let mut h = Harness::from_eve_source("Set x 1\nSet y hello\nMessage hi\n").unwrap();
        let before = h.snapshot();
        h.drive(&[Step::SaveLoadRoundTrip]).unwrap();
        let after = h.snapshot();
        assert_eq!(before, after, "save/load round-trip changed snapshot");
    }
}
