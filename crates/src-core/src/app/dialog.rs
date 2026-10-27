//! スクリプトが中断して待つ対話 (Talk / Confirm / Input / Menu) への応答。

use super::*;

impl App {
    /// 表示中の対話 UI（`Talk` / `Confirm`）。
    pub fn pending_dialog(&self) -> Option<&crate::dialog::PendingDialog> {
        self.pending_dialog.as_ref()
    }

    pub fn set_pending_dialog(&mut self, d: crate::dialog::PendingDialog) {
        self.pending_dialog = Some(d);
    }

    /// Talk の段階表示 (`:`) の残りページを設定する。`speaker` は継続ページ共通の
    /// 話者名。先頭ページは別途 `set_pending_dialog` で出す想定。
    pub fn set_talk_pages(&mut self, speaker: String, pages: Vec<String>) {
        self.talk_pages_speaker = speaker;
        self.talk_pages = pages;
    }

    /// 現在の pending dialog を捨てる (応答せず取り消す)。
    /// SRC `Cancel` コマンドや、シナリオ強制中断時に使う。`respond_dialog` と
    /// 違い script_var への結果代入は行わない。戻り値はキャンセル対象が
    /// 存在したか。
    pub fn cancel_pending_dialog(&mut self) -> bool {
        self.pending_dialog.take().is_some()
    }

    /// 対話 UI に応答してスクリプトを再開する。
    ///
    /// - `Talk` の場合: `choice` は無視。任意キーで進行。
    /// - `Confirm` の場合: 0=Yes / 1=No を `var_name` に代入。
    ///
    /// 戻り値: 応答した dialog がある場合 true。
    pub fn respond_dialog(&mut self, choice: u32) -> bool {
        // 精神コマンドのサブメニューは SP 消費 / condition 付与へ委譲。
        if self.pending_spirit.is_some() {
            return self.resolve_spirit(choice);
        }
        // 変形先サブメニューは形態変更へ委譲。
        if self.pending_transform.is_some() {
            return self.resolve_transform(choice);
        }
        // アビリティ一覧サブメニューは効果適用へ委譲。
        if self.pending_ability.is_some() {
            return self.resolve_ability(choice);
        }
        // 発進サブメニューは格納ユニットの出撃へ委譲。
        if self.pending_launch.is_some() {
            return self.resolve_launch(choice);
        }
        // 反撃モードのメニューは戦闘解決へ委譲 (script_var ではなく攻撃を実行)。
        if self.pending_reaction.is_some() {
            return self.resolve_reaction(choice);
        }
        // 武器選択メニューは選んだ武器で攻撃解決へ委譲。
        if self.pending_weapon_select.is_some() {
            return self.resolve_weapon_select(choice);
        }
        let Some(d) = self.pending_dialog.take() else {
            return false;
        };
        // キャンセル不可の Menu (キャンセル可 のない `Ask`) は choice 0 (Esc / 右クリック
        // / 任意進行) を受け付けず、ダイアログを維持して選択を強制する。
        // (キャンセルで 選択=0 になりキャラ未選択 → 味方0体 → 即敗北 を防ぐ。)
        if let crate::dialog::PendingDialog::Menu {
            non_cancellable: true,
            ..
        } = &d
        {
            if choice == 0 {
                self.pending_dialog = Some(d);
                return false;
            }
        }
        // Talk の段階表示 (`:`): 残りページがあれば次ページを出し、スクリプトは
        // 再開しない。最終ページまで送り切ってから通常の resume に進む。
        if matches!(d, crate::dialog::PendingDialog::Talk { .. }) && !self.talk_pages.is_empty() {
            let next = self.talk_pages.remove(0);
            let speaker = self.talk_pages_speaker.clone();
            self.pending_dialog = Some(crate::dialog::PendingDialog::Talk {
                speaker,
                body: next,
            });
            return true;
        }
        match &d {
            crate::dialog::PendingDialog::Confirm { var_name, .. } => {
                // SRC `Confirm` 仕様: `選択 = 1` で「はい」、`選択 = 0` で「いいえ」。
                // 本実装は内部で DialogYes → choice=0、DialogNo → choice=1 を
                // 取るので、SRC 流儀に合わせて反転する。`DialogChoice(n)` で
                // n>=2 が来た場合 (Talk から流用された誤入力など) は 0 として扱う。
                let v = match choice {
                    0 => "1", // Yes
                    1 => "0", // No
                    _ => "0",
                };
                self.set_script_var(var_name.clone(), v.to_string());
            }
            crate::dialog::PendingDialog::Menu {
                var_name,
                options,
                store_value,
                option_keys,
                ..
            } => {
                let picked = choice as usize;
                let val = if !option_keys.is_empty() && choice > 0 && picked <= option_keys.len() {
                    // SRC `Ask` Format 2: 選んだ要素の配列添字を格納する。
                    option_keys[picked - 1].clone()
                } else if *store_value && choice > 0 && picked <= options.len() {
                    options[picked - 1].clone()
                } else {
                    choice.to_string()
                };
                self.set_script_var(var_name.clone(), val);
            }
            crate::dialog::PendingDialog::Talk { .. } => {}
            crate::dialog::PendingDialog::WaitClick => {}
            crate::dialog::PendingDialog::Input {
                var_name, default, ..
            } => {
                // 数値応答だけが来た場合は default を残す
                self.set_script_var(var_name.clone(), default.clone());
            }
        }
        // スクリプト再開（失敗しても無視: シナリオ次第）。完了していれば
        // run_loop が `on_script_completed` で flow 継続を消化する。
        let _ = crate::event_runtime::resume(self);
        self.on_script_completed();
        self.return_from_intermission_subcommand_if_idle();
        self.auto_progress_stage_state_if_idle();
        true
    }

    /// `Wait Click` 系を **右クリック** で解除する（SRC の「右ボタン = キャンセル」）。
    /// Menu (Hotpoint) なら選択変数を空 (`選択 = ""`) にし、`KeyState(2)` が `1` を
    /// 返すフラグを立ててスクリプトを再開する。これによりステータス画面の
    /// `Wait Click` → `Case "" → If KeyState(2) Then Break` 等で画面を抜けられる。
    /// 対象は `Menu(store_value)` / `WaitClick` のみ。Talk / Confirm / Input は
    /// 右クリックを無視 (誤って進めない)。
    pub fn respond_dialog_right_click(&mut self) -> bool {
        // 武器選択窓は右クリックで取り消し (目標を選び直せる AttackSelect へ戻る)。
        if self.cancel_weapon_select() {
            return true;
        }
        // 対象は Hotpoint 付き `Wait Click` (store_value Menu + hotpoints) と、
        // 描画なしの `WaitClick` のみ。Talk / Confirm / Ask / Input は右クリックを
        // 無視する (左クリック判定 `is_hotpoint_menu` と同じ条件で線引き)。
        let is_hotpoint_menu = matches!(
            self.pending_dialog,
            Some(crate::dialog::PendingDialog::Menu {
                store_value: true,
                ..
            })
        ) && !self.hotpoints.is_empty();
        let is_wait_click = matches!(
            self.pending_dialog,
            Some(crate::dialog::PendingDialog::WaitClick)
        );
        if !is_hotpoint_menu && !is_wait_click {
            return false;
        }
        let Some(d) = self.pending_dialog.take() else {
            return false;
        };
        if let crate::dialog::PendingDialog::Menu { var_name, .. } = &d {
            // 選択 = "" (どの Hotpoint も選んでいない = キャンセル)
            self.set_script_var(var_name.clone(), String::new());
        }
        // KeyState(2) (右ボタン) を立てる。スクリプト再開後に評価される。
        self.set_wait_click_right(true);
        let _ = crate::event_runtime::resume(self);
        self.on_script_completed();
        self.return_from_intermission_subcommand_if_idle();
        self.auto_progress_stage_state_if_idle();
        true
    }

    /// `Input` モーダルへのテキスト応答。`var_name` に `text` を格納し再開。
    /// 他種の dialog だった場合は無視して `false`。
    pub fn respond_dialog_text(&mut self, text: String) -> bool {
        let Some(d) = self.pending_dialog.take() else {
            return false;
        };
        match &d {
            crate::dialog::PendingDialog::Input { var_name, .. } => {
                self.set_script_var(var_name.clone(), text);
            }
            // Input 以外の場合は元に戻して取り下げ
            other => {
                self.pending_dialog = Some(other.clone());
                return false;
            }
        }
        let _ = crate::event_runtime::resume(self);
        self.on_script_completed();
        self.return_from_intermission_subcommand_if_idle();
        self.auto_progress_stage_state_if_idle();
        true
    }
}
