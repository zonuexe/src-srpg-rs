//! 入力ディスパッチ: キー/クリック、カーソル移動、ユニット/マップメニュー、移動。

use super::*;

impl App {
    /// 入力を消費して状態を進める。redraw が必要なら `true` を返す。
    /// Consume an input. Returns `true` when a redraw is required.
    pub fn handle_input(&mut self, input: Input) -> bool {
        // 対話 UI が出ているうちは、それ以外の入力は喰わず dialog 操作のみ。
        if self.pending_dialog.is_some() {
            return match input {
                // Talk: 任意キーで進行。Confirm: Yes (0)。
                Input::Advance | Input::DialogYes | Input::AttackTarget | Input::EndPhase => {
                    self.respond_dialog(0)
                }
                Input::DialogNo => self.respond_dialog(1),
                Input::DialogChoice(n) => self.respond_dialog(n),
                // クリック処理: Hotpoint 付きの Menu はヒット判定で確定、
                // それ以外 (Talk / Confirm / 普通の Menu) はクリックで Advance。
                Input::ClickAt { x, y } => {
                    // 反撃ウィンドウ表示中は専用ジオメトリで選択肢行をクリック判定する
                    // (プレーン Menu とは別配置のため menu_choice_at は使えない)。
                    if self.pending_reaction.is_some() {
                        let n =
                            if let Some(crate::dialog::PendingDialog::Menu { options, .. }) =
                                &self.pending_dialog
                            {
                                options.len()
                            } else {
                                0
                            };
                        return match crate::scene::reaction::reaction_choice_at(n, x, y) {
                            Some(c) => self.respond_dialog(c),
                            None => false,
                        };
                    }
                    // 武器選択窓も専用ジオメトリで武器行をクリック判定する。
                    if self.pending_weapon_select.is_some() {
                        let n =
                            if let Some(crate::dialog::PendingDialog::Menu { options, .. }) =
                                &self.pending_dialog
                            {
                                options.len()
                            } else {
                                0
                            };
                        return match crate::scene::weapon_select::weapon_select_choice_at(n, x, y) {
                            Some(c) => self.respond_dialog(c),
                            None => false,
                        };
                    }
                    let is_hotpoint_menu = matches!(
                        self.pending_dialog,
                        Some(crate::dialog::PendingDialog::Menu {
                            store_value: true,
                            ..
                        })
                    ) && !self.hotpoints.is_empty();
                    if is_hotpoint_menu {
                        // ヒットすれば確定、外したら無反応 (キャンセルではない)
                        self.try_hotpoint_click(x, y)
                    } else {
                        // プレーンな Menu(Ask) は選択肢行をクリック確定する。
                        // 旧実装は任意クリックで respond_dialog(0)=キャンセルしており、
                        // 難易度/キャラ選択を「クリックで選べない」状態だった
                        // (選択が 0 になり、東方夢想伝では味方 0 体で即敗北)。
                        // Talk / Confirm / WaitClick は従来どおり任意クリックで Advance。
                        let plain_menu_choice = if let Some(crate::dialog::PendingDialog::Menu {
                            prompt,
                            options,
                            ..
                        }) = &self.pending_dialog
                        {
                            Some(crate::scene::dialog::menu_choice_at(
                                prompt,
                                options.len(),
                                x,
                                y,
                            ))
                        } else {
                            None
                        };
                        match plain_menu_choice {
                            // 選択肢行をクリック → 確定。
                            Some(Some(n)) => self.respond_dialog(n),
                            // プレーン Menu の選択肢外クリック → 無反応
                            // (キャンセルは右クリック / Esc)。
                            Some(None) => false,
                            // 非 Menu (Talk / Confirm 等) → Advance(Yes)。
                            None => self.respond_dialog(0),
                        }
                    }
                }
                // 右クリック: SRC `Wait Click` の「右ボタン = キャンセル/戻る」。
                // 選択を空にして KeyState(2)=1 でスクリプトを再開し、ステータス画面等の
                // `Case "" → If KeyState(2) Then Break` で抜けられるようにする。
                Input::RightClickAt { .. } => self.respond_dialog_right_click(),
                // Esc: Hotpoint Wait Click 画面は右クリック相当で「戻る」。トラックパッドの
                // 副ボタン設定に依存せず確実に抜けられるようにする。通常の Menu(Ask) は
                // 従来どおり choice 0 でキャンセル。
                Input::Cancel => {
                    if self.respond_dialog_right_click() {
                        true
                    } else if matches!(
                        self.pending_dialog,
                        Some(crate::dialog::PendingDialog::Menu { .. })
                    ) {
                        self.respond_dialog(0)
                    } else {
                        false
                    }
                }
                _ => false,
            };
        }
        match input {
            Input::Advance => self.advance(),
            Input::ClickAt { x, y } => self.handle_click(x, y),
            Input::RightClickAt { x, y } => self.handle_right_click(x, y),
            Input::MoveCursor(dir) => self.move_cursor(dir),
            Input::EndPhase => self.end_phase(),
            Input::AttackTarget => self.player_attack_shortcut(),
            Input::CycleWeapon => self.cycle_weapon(),
            Input::DialogYes | Input::DialogNo | Input::DialogChoice(_) => false,
            Input::Cancel => self.cancel_action(),
            Input::GotoPilotList => {
                if matches!(self.scene, Scene::MapView | Scene::UnitList) {
                    self.scene = Scene::PilotList;
                    true
                } else {
                    false
                }
            }
            Input::GotoUnitList => {
                if matches!(self.scene, Scene::MapView | Scene::PilotList) {
                    self.scene = Scene::UnitList;
                    true
                } else {
                    false
                }
            }
            Input::GotoMapView => {
                if matches!(self.scene, Scene::PilotList | Scene::UnitList) {
                    self.scene = Scene::MapView;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// 右クリック / Esc によるキャンセル。`MoveSelect` / `AttackSelect` →
    /// Browse へ、`command_menu` → 閉じる、それ以外は no-op。
    pub(super) fn cancel_action(&mut self) -> bool {
        use crate::command_menu::ActionMode;
        // 敗北フォールバック中のキャンセル (右クリック/Esc) → タイトルへ。
        if self.pending_game_over {
            self.return_to_title();
            return true;
        }
        // 単機ステータス詳細のキャンセル (右クリック/Esc) → インターミッションへ戻る
        // (元 SRC: パイロット/ユニットステータスは右クリックで戻る)。
        if self.scene == Scene::UnitDetail {
            self.exit_list_scene();
            return true;
        }
        // 乗り換えの移動先選択中のキャンセル → 移動元選択へ戻る (メニューには戻らない)。
        if self.scene == Scene::Intermission
            && self.intermission_mode == IntermissionMode::RideChange
            && self.ride_change_source.take().is_some()
        {
            self.intermission_cursor = 0;
            return true;
        }
        // インターミッションのサブモード (機体改造 / 換装 / 乗り換え) 中のキャンセル → メインメニューへ。
        if self.scene == Scene::Intermission && self.intermission_mode != IntermissionMode::Menu {
            self.intermission_mode = IntermissionMode::Menu;
            self.intermission_cursor = 0;
            return true;
        }
        // PostMoveMenu 表示中のキャンセル: 移動を巻き戻してブラウズに戻る
        if self.command_menu.is_some() {
            if let ActionMode::PostMoveMenu { uid, snapshot } = self.action_mode.clone() {
                self.rollback_move(&uid, &snapshot);
            }
            self.command_menu = None;
            self.action_mode = ActionMode::Browse;
            return true;
        }
        match self.action_mode.clone() {
            ActionMode::Browse => false,
            ActionMode::PostMoveMenu { uid, snapshot } => {
                self.rollback_move(&uid, &snapshot);
                self.action_mode = ActionMode::Browse;
                true
            }
            ActionMode::AttackSelect {
                uid,
                snapshot: Some(snapshot),
            } => {
                // 攻撃目標選択キャンセル → 移動後メニューに戻る (移動はまだ確定のまま)
                let pos = self.database.unit_by_uid(&uid).map(|u| (u.x, u.y));
                self.action_mode = ActionMode::PostMoveMenu { uid, snapshot };
                if let Some(pos) = pos {
                    self.open_unit_menu(pos);
                }
                true
            }
            ActionMode::SpiritTarget { caster, .. } => {
                // 精神コマンドの対象選択キャンセル: SP は未消費。発動主体の
                // ユニットメニューへ戻り、別コマンドや行動を選び直せるようにする。
                self.action_mode = ActionMode::Browse;
                if let Some(pos) = self.database.unit_by_uid(&caster).map(|u| (u.x, u.y)) {
                    self.open_unit_menu(pos);
                }
                true
            }
            ActionMode::SupportTarget { caster, .. } => {
                // 修理 / 補給 の対象選択キャンセル: 効果なし。発動主体の
                // ユニットメニューへ戻り、別コマンドや行動を選び直せるようにする。
                self.action_mode = ActionMode::Browse;
                if let Some(pos) = self.database.unit_by_uid(&caster).map(|u| (u.x, u.y)) {
                    self.open_unit_menu(pos);
                }
                true
            }
            ActionMode::AbilityTarget { caster, .. } => {
                // アビリティ対象選択キャンセル: 効果なし。発動主体のメニューへ戻る。
                self.action_mode = ActionMode::Browse;
                if let Some(pos) = self.database.unit_by_uid(&caster).map(|u| (u.x, u.y)) {
                    self.open_unit_menu(pos);
                }
                true
            }
            ActionMode::MoveSelect { .. } | ActionMode::AttackSelect { snapshot: None, .. } => {
                self.action_mode = ActionMode::Browse;
                true
            }
            ActionMode::LandingSelect { uid, carrier, .. } => {
                self.action_mode = ActionMode::Browse;
                if let Some(carrier) = carrier {
                    // 発進: 配置せず格納のまま、母艦メニューへ戻す。
                    self.reopen_unit_menu_for(&carrier);
                } else {
                    // 分離: 分離自体は成立済なので、残り全機を隣接自動配置で確定する。
                    let source = self.landing_source.unwrap_or((0, 0));
                    if let Some((tx, ty)) = self.find_empty_adjacent_tile(source) {
                        self.place_separated_unit_at(&uid, tx, ty);
                    }
                    let remaining = std::mem::take(&mut self.landing_queue);
                    for ru in remaining {
                        if let Some((tx, ty)) = self.find_empty_adjacent_tile(source) {
                            self.place_separated_unit_at(&ru, tx, ty);
                        }
                    }
                    self.finish_landing_queue();
                }
                true
            }
        }
    }

    pub(super) fn handle_right_click(&mut self, _x: i32, _y: i32) -> bool {
        self.cancel_action()
    }

    pub(super) fn cycle_weapon(&mut self) -> bool {
        if self.scene != Scene::MapView {
            return false;
        }
        let Some((cx, cy)) = self.map_cursor else {
            return false;
        };
        let Some(u) = self.database.units_at(cx, cy).next() else {
            return false;
        };
        let Some(d) = self.database.unit_by_name(&u.unit_data_name) else {
            return false;
        };
        // 0 = 自動、1..=N = 武器固定
        let modulus = d.weapons.len() + 1;
        if modulus == 0 {
            return false;
        }
        self.selected_weapon_idx = (self.selected_weapon_idx + 1) % modulus;
        true
    }

    /// 武器が「移動後攻撃」で使用可能か。`post_move=false` なら常に true。
    /// SRC: 移動後は近距離武器のみデフォルト可、遠距離 (max_range>1) は Ｐ 属性、
    /// Ｑ 属性 (移動後使用不可) は除外。メニュー可否判定と攻撃実行で共有し、
    /// 「攻撃を選べるが押しても無反応」の齟齬を防ぐ。
    pub(super) fn weapon_usable_post_move(
        w: &crate::data::unit::WeaponData,
        post_move: bool,
        totsugeki: bool,
    ) -> bool {
        if !post_move {
            return true;
        }
        if w.max_range > 1 {
            // 通常、長射程武器は移動後使用不可 (Ｐ 属性を除く)。精神「突撃」発動中は
            // マップ攻撃 (Ｍ) 以外の長射程武器も移動後使用可能 (スペシャルパワー.md)。
            w.class.contains('Ｐ') || (totsugeki && !w.class.contains('Ｍ'))
        } else {
            !w.class.contains('Ｑ')
        }
    }

    /// 距離 `dist` の敵に対し使用する武器を選ぶ。`forced` 指定時はその武器が
    /// 合法 (射程内・チャージ・移動後制限) なら採用、不可なら `None`。自動選択時は
    /// 合法武器のうち威力最大 (移動後制限なしは従来の `best_weapon_in_range_with_charge`)。
    #[allow(clippy::too_many_arguments)] // 武器選択の制約 (射程/チャージ/移動後/突撃/資源/必要技能) を素直に列挙。
    pub(crate) fn pick_attack_weapon(
        atk_unit: &crate::data::unit::UnitData,
        dist: u32,
        forced: Option<&crate::data::unit::WeaponData>,
        charged: bool,
        post_move: bool,
        totsugeki: bool,
        firable: impl Fn(&crate::data::unit::WeaponData) -> bool,
        skill_ok: impl Fn(&crate::data::unit::WeaponData) -> bool,
    ) -> Option<crate::data::unit::WeaponData> {
        let legal = |w: &crate::data::unit::WeaponData| {
            combat::weapon_in_range(w, dist)
                && (charged || !combat::is_charge_weapon(w))
                && Self::weapon_usable_post_move(w, post_move, totsugeki)
                && firable(w)
        };
        match forced {
            // 強制武器 (プレイヤーが武器選択した場合) は資源 (EN/弾/気力) を満たさなくても
            // 発射を許す (UI 側で選択可否を制御する想定)。ただし必要技能/条件は資源ではなく
            // 使用資格そのものなので強制でも必須 (満たさない武器は撃てない)。
            Some(w) => (combat::weapon_in_range(w, dist)
                && (charged || !combat::is_charge_weapon(w))
                && Self::weapon_usable_post_move(w, post_move, totsugeki)
                && skill_ok(w))
            .then(|| w.clone()),
            None => atk_unit
                .weapons
                .iter()
                .filter(|w| legal(w))
                .max_by_key(|w| w.power)
                .cloned(),
        }
    }

    /// カーソル上の自勢力ユニットで最寄りの敵対ユニットを攻撃。
    /// `hit_chance` に対して PRNG を回し、命中時はダメージを加算。
    /// 撃破時はマップから除去。HUD ログに結果を流す。
    /// **AI / 内部専用**。プレイヤーのクリック攻撃は `attack_unit_at` (状態機械経由) を使う。
    pub(super) fn attack_target(&mut self) -> bool {
        // 自動 def_mode ("") = 防御側は自動反撃 (従来挙動)。
        self.attack_resolve_and_run(None, false, "")
    }

    /// `a` キー (Input::AttackTarget) のプレイヤー用挙動。旧来は最寄り敵を直接攻撃して
    /// メニュー状態機械を迂回していた (位置ずれ・選択不整合の原因)。現在はユニット
    /// コマンドメニューに「攻撃」があればそれを選択し、`AttackSelect` 状態へ遷移させる
    /// だけにする (クリックと同一フロー)。メニュー未表示なら no-op。
    pub(super) fn player_attack_shortcut(&mut self) -> bool {
        use crate::command_menu::{CommandMenu, MenuActionId, UnitAction, UnitMenuItem};
        let has_attack = matches!(
            &self.command_menu,
            Some(CommandMenu::Unit { items, .. })
                if items.iter().any(|i| matches!(i, UnitMenuItem::Builtin(UnitAction::Attack)))
        );
        if has_attack {
            self.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Builtin(
                UnitAction::Attack,
            )))
        } else {
            false
        }
    }

    /// `atk_uid` のユニットが `target` タイルのユニットを攻撃する。
    /// クリックされたタイルの敵を厳密に対象とし、最寄り敵ヒューリスティックは使わない。
    /// 対象が敵でない / 射程内に合法武器が無い場合は false (別ユニットを攻撃しない)。
    pub(super) fn attack_unit_at(
        &mut self,
        atk_uid: &str,
        target: (u32, u32),
        post_move: bool,
    ) -> bool {
        let Some((ax, ay)) = self.database.unit_by_uid(atk_uid).map(|u| (u.x, u.y)) else {
            return false;
        };
        self.map_cursor = Some((ax, ay));
        self.attack_resolve_and_run(Some(target), post_move, "")
    }

    pub(super) fn move_cursor(&mut self, dir: Direction) -> bool {
        // 単機ステータス詳細: Left/Up=前の機体, Right/Down=次の機体 (ラップ)。
        if self.scene == Scene::UnitDetail {
            return match dir {
                Direction::Left | Direction::Up => self.cycle_status_detail(-1),
                Direction::Right | Direction::Down => self.cycle_status_detail(1),
            };
        }
        if self.scene == Scene::Intermission {
            // インターミッション画面では Up/Down が項目選択カーソル移動。
            // Left/Right は無視。
            let item_count = self.intermission_item_count();
            if item_count == 0 {
                return false;
            }
            match dir {
                Direction::Up => {
                    if self.intermission_cursor == 0 {
                        self.intermission_cursor = item_count - 1;
                    } else {
                        self.intermission_cursor -= 1;
                    }
                    return true;
                }
                Direction::Down => {
                    self.intermission_cursor = (self.intermission_cursor + 1) % item_count;
                    return true;
                }
                _ => return false,
            }
        }
        if self.scene != Scene::MapView {
            return false;
        }
        let Some(map) = self.database.map.as_ref() else {
            return false;
        };
        if map.width == 0 || map.height == 0 {
            return false;
        }
        let (mut x, mut y) = self.map_cursor.unwrap_or((0, 0));
        match dir {
            Direction::Left if x > 0 => x -= 1,
            Direction::Right if x + 1 < map.width => x += 1,
            Direction::Up if y > 0 => y -= 1,
            Direction::Down if y + 1 < map.height => y += 1,
            _ => return false,
        }
        let next = Some((x, y));
        if self.map_cursor == next {
            return false;
        }
        self.map_cursor = next;
        self.ensure_cursor_visible();
        true
    }

    /// 「次へ」操作。Title / Configuration / Intermission の遷移のみを担う。
    ///
    /// MapView では **何もしない**。原典 SRC にはステージ進行をユーザの Enter
    /// 押下でゲートする仕様が無く（[`スタートイベント.md`] 参照）、
    /// `Briefing → Sortie → Battle` は [`Self::auto_progress_stage_state_if_idle`]
    /// が idle 時に自動進行させる。`Victory` / `Defeat` も独自オーバーレイと
    /// Enter→タイトルのゲートを持たず、勝利/敗北/Ending ラベルの発火に委ねる。
    pub(super) fn advance(&mut self) -> bool {
        match self.scene {
            Scene::Title => {
                self.enter_configuration();
                true
            }
            Scene::Configuration => {
                self.settings_snapshot = None;
                // インターミッションコマンドが登録されていれば、戦闘前に
                // メニュー画面を挟む。無ければ従来通り MapView へ直行。
                if !self.intermission_commands.is_empty() {
                    self.scene = Scene::Intermission;
                    self.intermission_cursor = 0;
                    self.intermission_mode = IntermissionMode::Menu;
                } else {
                    self.scene = Scene::MapView;
                }
                true
            }
            Scene::Intermission => {
                self.confirm_intermission_selection();
                true
            }
            // 戦闘画面では Enter で進行しない。ステージ進行は auto_progress と
            // ラベル発火が担う（誤押下でのシーン遷移も防ぐ）。
            // 例外: 敗北フォールバック中は Enter/クリック = コンティニュー、勝利状態で
            // クリア が進行を解決しない場合は Enter/クリック = 次へ (soft-lock 脱出)。
            Scene::MapView => {
                if self.pending_game_over {
                    self.game_over_continue();
                    true
                } else if self.stage_state == crate::stage::StageState::Victory {
                    self.proceed_after_victory();
                    true
                } else {
                    false
                }
            }
            Scene::PilotList => {
                self.scene = Scene::UnitList;
                true
            }
            Scene::UnitList => {
                self.exit_list_scene();
                true
            }
            // 単機ステータス詳細: Enter で閉じてインターミッションへ戻る
            // (巡回は ◀ / ▶ ボタンか矢印キー)。
            Scene::UnitDetail => {
                self.exit_list_scene();
                true
            }
        }
    }

    /// 一覧シーン (UnitList) を抜けるときの遷移。`scene_return_to` があればそこへ
    /// (インターミッション「ステータス」からの復帰)、無ければ既定の Title へ。
    pub(super) fn exit_list_scene(&mut self) {
        match self.scene_return_to.take() {
            Some(Scene::Intermission) => {
                self.scene = Scene::Intermission;
                self.intermission_mode = IntermissionMode::Menu;
                self.intermission_cursor = 0;
            }
            Some(s) => self.scene = s,
            None => self.scene = Scene::Title,
        }
    }

    /// 単機ステータス詳細で巡回対象になる味方ロスター (off_map 含む)。
    /// `intermission_upgrade_units` と同じく Player 陣営の `unit_instances` index 列。
    pub(crate) fn status_roster(&self) -> Vec<usize> {
        self.intermission_upgrade_units()
    }

    /// 単機ステータス詳細で次/前の機体へ巡回する (ロスター内をラップ)。
    /// `delta` は +1 (次) / -1 (前)。ロスターが空なら何もしない。
    pub(super) fn cycle_status_detail(&mut self, delta: isize) -> bool {
        let total = self.status_roster().len();
        if total == 0 {
            return false;
        }
        let cur = self.status_detail_index.min(total - 1) as isize;
        let next = (cur + delta).rem_euclid(total as isize) as usize;
        self.status_detail_index = next;
        true
    }

    /// 設定画面 (Configuration) へ遷移し、キャンセル復元用に現在の設定を
    /// スナップショットする。メニューバー「設定変更」/ タイトルクリックから呼ぶ。
    ///
    /// 対話 (`pending_dialog`) やスクリプト (`script_ctx`) が動作中は遷移しない。
    /// 原典 SRC でも設定変更はイベント再生中には実行できず、また遷移すると Talk
    /// 等が Configuration 画面に重なって操作不能になる (クリックは pending_dialog に
    /// 吸われる) ため。
    pub fn enter_configuration(&mut self) {
        if self.pending_dialog.is_some() || self.has_script_context() {
            return;
        }
        self.scene = Scene::Configuration;
        self.settings_snapshot = Some(self.settings.clone());
    }

    /// canvas-local 座標でクリックを処理。
    pub(super) fn handle_click(&mut self, x: i32, y: i32) -> bool {
        let (sw, sh) = self.scene.size();
        let ox = (CANVAS_WIDTH as i32 - sw as i32) / 2;
        let oy = (CANVAS_HEIGHT as i32 - sh as i32) / 2;
        let lx = x - ox;
        let ly = y - oy;

        match self.scene {
            Scene::Title => {
                self.enter_configuration();
                true
            }
            Scene::Configuration => self.handle_click_configuration(lx, ly),
            Scene::Intermission => self.handle_click_intermission(lx, ly),
            // MapView は メニュー判定で canvas 絶対座標も使うので (x, y) 双方を渡す。
            Scene::MapView => self.handle_click_map_view(x, y, lx, ly),
            Scene::PilotList => {
                self.scene = Scene::UnitList;
                true
            }
            Scene::UnitList => {
                self.exit_list_scene();
                true
            }
            Scene::UnitDetail => self.handle_click_unit_detail(lx, ly),
        }
    }

    /// 単機ステータス詳細でのクリック: フッタの ◀ / ▶ / 閉じる ボタンのみ反応する
    /// (ボタン外クリックは無反応＝誤操作で閉じない)。
    pub(super) fn handle_click_unit_detail(&mut self, lx: i32, ly: i32) -> bool {
        use crate::scene::unit_detail::{hit_button, DetailButton};
        match hit_button(lx, ly) {
            Some(DetailButton::Prev) => self.cycle_status_detail(-1),
            Some(DetailButton::Next) => self.cycle_status_detail(1),
            Some(DetailButton::Close) => {
                self.exit_list_scene();
                true
            }
            None => false,
        }
    }

    /// MapView でのクリック処理:
    /// - カーソル位置に現フェーズの所属ユニットがいて、クリック先が
    ///   そのユニットの移動範囲内の空マスなら、ユニットを移動。
    /// - そうでなければカーソルを移動。
    ///
    /// 入力 `(lx, ly)` は MapView 内のローカル座標。ビューポート相対の
    /// タイル位置に変換した後、`map_scroll` を足してマップ絶対座標にする。
    pub(super) fn handle_click_map_view(
        &mut self,
        abs_x: i32,
        abs_y: i32,
        lx: i32,
        ly: i32,
    ) -> bool {
        // 敗北フォールバック中の左クリック = コンティニュー (soft-lock 脱出)。
        if self.pending_game_over {
            self.game_over_continue();
            return true;
        }
        // 勝利状態でクリア等が進行を解決しない場合の左クリック = 次へ (soft-lock 脱出)。
        // 勝利演出 (Talk) 中はクリックが pending_dialog へ行くのでここには来ない。
        if self.stage_state == crate::stage::StageState::Victory {
            self.proceed_after_victory();
            return true;
        }
        // 敗北確定後はマップ操作を一切受け付けない (クリックでユニットが動く等の破綻防止)。
        if self.stage_state == crate::stage::StageState::Defeat {
            return false;
        }
        // 1) コマンドメニューが表示中なら、まずメニュー項目クリックを試みる。
        //    メニューは canvas 絶対座標で描画されるので abs_x/abs_y で判定。
        if self.command_menu.is_some() {
            if let Some(action) = crate::scene::command_menu::hit_test_menu_item(
                self.command_menu.as_ref(),
                abs_x,
                abs_y,
            ) {
                return self.execute_menu_action(action);
            }
            // メニュー外をクリックしたらメニューを閉じる
            self.command_menu = None;
            // 移動後メニューを外クリックで閉じた場合は「移動を確定」してブラウズに戻す
            // (巻き戻さない)。これで続けて別ユニットを選択でき、移動済みユニットは
            // その場に留まる。移動の取り消しは右クリック (cancel_action) のみが行う。
            if matches!(
                self.action_mode,
                crate::command_menu::ActionMode::PostMoveMenu { .. }
            ) {
                self.action_mode = crate::command_menu::ActionMode::Browse;
            }
        }

        let Some((vtx, vty)) = map_view::pixel_to_tile(lx, ly) else {
            return false;
        };
        let target = (vtx + self.map_scroll.0, vty + self.map_scroll.1);
        if let Some(map) = self.database.map.as_ref() {
            if target.0 >= map.width || target.1 >= map.height {
                return false;
            }
        } else {
            return false;
        }
        let prev_cursor = self.map_cursor;
        self.map_cursor = Some(target);
        self.ensure_cursor_visible();

        // Battle 以外ではメニューフローを使わず、旧来の「cursor 上ユニットを直接移動」を維持。
        if self.stage_state != crate::stage::StageState::Battle {
            if let Some(prev) = prev_cursor {
                if prev != target && self.try_move_unit_to(prev, target) {
                    return true;
                }
            }
            return true;
        }

        use crate::command_menu::ActionMode;
        // 2) ActionMode に応じてクリック処理を分岐 (Battle 時のみ)
        match self.action_mode.clone() {
            ActionMode::MoveSelect { uid } => {
                // 占有マス (味方の母艦 / 合体相手) へのクリックは 搭載 / 合体 を試みる
                // (原典: 母艦上へ移動で搭載・2 機合体は相手上へ移動)。成立すれば確定。
                if let Some(occ) = self
                    .database
                    .uid_at(target.0, target.1)
                    .map(|s| s.to_string())
                {
                    if occ != uid && self.try_board_or_combine_by_move(&uid, &occ) {
                        self.action_mode = ActionMode::Browse;
                        self.command_menu = None;
                        return true;
                    }
                }
                // 移動前スナップショットを取得してから移動を確定 (キャンセル復帰用)。
                let snap =
                    self.database
                        .unit_by_uid(&uid)
                        .map(|u| crate::command_menu::MoveSnapshot {
                            x: u.x,
                            y: u.y,
                            en_consumed: u.en_consumed,
                            current_area: u.current_area.clone(),
                        });
                if let Some(snapshot) = snap {
                    let from = (snapshot.x, snapshot.y);
                    if self.try_move_unit_to(from, target) {
                        // 移動成功 → 移動後メニューを表示 (同一 uid を追従)
                        self.action_mode = ActionMode::PostMoveMenu {
                            uid: uid.clone(),
                            snapshot,
                        };
                        self.open_unit_menu(target);
                    }
                }
                // 移動失敗 (範囲外等) はそのまま (mode 継続)
                true
            }
            ActionMode::PostMoveMenu { .. } => true,
            ActionMode::AttackSelect { uid, snapshot } => {
                // クリックしたタイルのユニットを攻撃 (Phase D で対象解決を厳密化)。
                let post_move = snapshot.is_some();
                // UI モード + 武器選択ウィンドウ有効時は、攻撃可能な目標なら武器選択窓を
                // 出して解決を保留する (応答後に resolve_weapon_select が解決)。
                if self.animate_battle
                    && self.weapon_window_enabled
                    && self.can_attack_target(&uid, target, post_move)
                {
                    self.begin_weapon_select(uid.clone(), target, post_move);
                } else if self.attack_unit_at(&uid, target, post_move) {
                    // 攻撃したら行動終了 → has_acted、メニュー閉じる。
                    self.finish_player_attack(&uid);
                }
                true
            }
            ActionMode::SpiritTarget {
                caster,
                spirit,
                cost,
                target_enemy,
            } => {
                // クリックしたタイルのユニットを精神コマンドの対象に取る。
                let clicked = self
                    .database
                    .uid_at(target.0, target.1)
                    .and_then(|uid| self.database.unit_by_uid(uid))
                    .map(|u| (u.uid.clone(), u.party));
                let caster_party = self.database.unit_by_uid(&caster).map(|u| u.party);
                if let (Some((target_uid, tparty)), Some(cparty)) = (clicked, caster_party) {
                    let valid = if target_enemy {
                        cparty.is_hostile_to(tparty)
                    } else {
                        cparty.is_ally_of(tparty)
                    };
                    if valid {
                        self.apply_spirit_to_target(&caster, &target_uid, &spirit, cost);
                        self.action_mode = ActionMode::Browse;
                        // 発動主体のメニューを再表示し連続発動 / 行動を可能にする。
                        if let Some(pos) = self.database.unit_by_uid(&caster).map(|u| (u.x, u.y)) {
                            self.open_unit_menu(pos);
                        }
                    }
                    // 無効な対象 (陣営違い) のクリックは no-op で選択モード継続。
                }
                true
            }
            ActionMode::SupportTarget { caster, kind } => {
                // クリックしたタイルのユニットを修理 / 補給の対象に取る。
                // 候補 (隣接・味方・要支援) に含まれるときのみ適用し、行動終了する。
                let clicked = self
                    .database
                    .uid_at(target.0, target.1)
                    .map(|s| s.to_string());
                if let Some(tuid) = clicked {
                    if self.support_target_uids(&caster, kind).contains(&tuid) {
                        self.apply_support_to_target(&caster, &tuid, kind);
                        // 行動終了 (修理 / 補給 は発動で行動を消費する)。
                        if let Some(u) = self.database.unit_by_uid_mut(&caster) {
                            u.has_acted = true;
                        }
                        if let Some(i) = self.database.idx_by_uid(&caster) {
                            crate::event_runtime::fire_action_end_labels(self, i);
                            // SRC `接触 <unit1> <unit2>:` ─ 行動終了後に隣接ペアで発火。
                            crate::event_runtime::fire_contact_event_labels(self, i);
                        }
                        self.action_mode = ActionMode::Browse;
                        self.command_menu = None;
                    }
                    // 候補外 (非隣接 / 陣営違い / 支援不要) のクリックは no-op で継続。
                }
                true
            }
            ActionMode::AbilityTarget {
                caster,
                ability_idx,
            } => {
                // クリックしたタイルのユニットをアビリティの対象に取る。
                // 有効 (味方・射程内) のときのみ適用し、消費・行動終了処理を行う。
                let clicked = self
                    .database
                    .uid_at(target.0, target.1)
                    .map(|s| s.to_string());
                if let Some(tuid) = clicked {
                    if self.ability_target_valid(&caster, ability_idx, &tuid) {
                        self.apply_ability(&caster, ability_idx, &tuid);
                        self.finish_ability(&caster);
                    }
                    // 無効な対象 (射程外 / 陣営違い) のクリックは no-op で継続。
                }
                true
            }
            ActionMode::LandingSelect { .. } => {
                // 着地候補マスのクリックで配置を確定する。候補外は no-op (継続)。
                self.confirm_landing(target.0, target.1);
                true
            }
            ActionMode::Browse => {
                // 通常: クリック先にユニットがあればメニュー、無ければマップメニュー
                let on_unit = self
                    .database
                    .uid_at(target.0, target.1)
                    .and_then(|uid| self.database.unit_by_uid(uid))
                    .map(|u| (u.party, u.has_acted));
                if let Some((party, has_acted)) = on_unit {
                    let is_player_active = party == self.turn.phase.party() && !has_acted;
                    if is_player_active && self.stage_state == crate::stage::StageState::Battle {
                        self.open_unit_menu(target);
                    }
                } else if self.stage_state == crate::stage::StageState::Battle {
                    self.open_map_menu();
                }
                true
            }
        }
    }

    /// ユニットコマンドメニューを開く（指定タイルにユニットがあること前提）。
    pub(super) fn open_unit_menu(&mut self, pos: (u32, u32)) {
        use crate::command_menu::{CommandMenu, UnitAction, UnitMenuItem};
        let Some(u) = self
            .database
            .unit_instances
            .iter()
            .find(|u| u.x == pos.0 && u.y == pos.1)
            .cloned()
        else {
            return;
        };
        let post_move = u.has_moved;
        let mut items = Vec::new();

        // 移動コマンドは移動前のみ
        if !post_move {
            items.push(UnitMenuItem::Builtin(UnitAction::Move));
        }

        // 攻撃コマンド: 射程内に敵がいるか（移動後は武器属性で絞り込み）
        let unit_def = self.database.unit_by_name(&u.unit_data_name).cloned();
        let totsugeki = u.has_condition("突撃");
        if let Some(def) = unit_def.as_ref() {
            let any_in_range = self.database.unit_instances.iter().any(|other| {
                if !other.party.is_hostile_to(u.party) || other.off_map {
                    return false;
                }
                let d = combat::manhattan(pos, (other.x, other.y));
                // メニュー可否と攻撃実行で同一述語を使い齟齬を防ぐ。
                def.weapons.iter().any(|w| {
                    combat::weapon_in_range(w, d)
                        && Self::weapon_usable_post_move(w, post_move, totsugeki)
                })
            });
            if any_in_range {
                items.push(UnitMenuItem::Builtin(UnitAction::Attack));
            }
        }

        // 武装一覧は移動前のみ
        if !post_move {
            items.push(UnitMenuItem::Builtin(UnitAction::WeaponList));
        }

        // 精神コマンド: パイロットが習得済み (level<=パイロットレベル) の SP コマンドを
        // 持つ味方ユニットで、まだ行動を確定していない (移動後でも可) ときに表示する。
        if u.party == crate::Party::Player
            && !u.has_acted
            && !self.spirit_command_options(&u.uid).is_empty()
        {
            items.push(UnitMenuItem::Spirit);
        }

        // 修理 / 補給（特殊能力 修理装置 / 補給装置）: 隣接に要支援の味方が居る
        // 味方ユニットで、まだ行動を確定していない（移動後も可）ときに表示する。
        // 原典 SRC: 修理/補給は発動で行動終了する（精神コマンドと異なり継続不可）。
        if u.party == crate::Party::Player && !u.has_acted {
            for kind in [
                crate::command_menu::SupportKind::Repair,
                crate::command_menu::SupportKind::Supply,
            ] {
                // `修理装置Lv*` のようにレベル接尾辞付きでも拾えるよう feature_level で判定。
                if crate::feature::feature_level(&u.active_features, kind.feature_name()).is_some()
                    && !self.support_target_uids(&u.uid, kind).is_empty()
                {
                    items.push(UnitMenuItem::Support(kind));
                }
            }
        }

        // 変形（特殊能力 変形）: 変形先を持つ味方ユニットで、移動前かつ未行動の
        // ときに表示する。原典 SRC: 変形は移動前のみ・行動を消費しない。
        if u.party == crate::Party::Player
            && !post_move
            && !u.has_acted
            && self.transform_forms(&u).is_some()
        {
            items.push(UnitMenuItem::Transform);
        }

        // チャージ: チャージ攻撃 (Ｃ 属性) 武器を持つ味方ユニットで、未チャージ・
        // 未行動のときに表示する。原典 SRC: チャージで行動終了し、次ターンに
        // チャージ攻撃が解禁される (charged フラグは攻撃使用まで持続)。
        if u.party == crate::Party::Player && !u.has_acted && !u.charged {
            if let Some(def) = unit_def.as_ref() {
                if def.weapons.iter().any(combat::is_charge_weapon) {
                    items.push(UnitMenuItem::Charge);
                }
            }
        }

        // アビリティ: アビリティを持つ未行動の味方ユニットで表示する。使用可否
        // (回数 / EN / 気力) はサブメニュー側で × 表示・判定する (原典準拠)。
        if u.party == crate::Party::Player
            && !u.has_acted
            && unit_def.as_ref().is_some_and(|d| !d.abilities.is_empty())
        {
            items.push(UnitMenuItem::Ability);
        }

        // 発進: 母艦にユニットを格納している味方ユニット。原典 SRC: 発進は行動を
        // 消費しない (メニューが開くのは未行動ユニットなので !has_acted は不問)。
        if u.party == crate::Party::Player && !u.stored_units.is_empty() {
            items.push(UnitMenuItem::Launch);
        }

        // 合体: 合体特殊能力を持ち、合体相手が全て 2 マス以内に居る味方ユニット。
        // 原典 SRC (合体.md): 合体コマンドで合体するのは 3 体以上合体 (相手 2 機以上) の
        // 場合のみ。2 体合体は相手ユニット上への移動で行う (move 経路)。合体は移動前のみ・
        // 発動で行動終了。
        if u.party == crate::Party::Player
            && !u.has_acted
            && !post_move
            && self.combine_partner_count(&u.uid) >= 2
            && self.combine_partners(&u.uid).is_some()
        {
            items.push(UnitMenuItem::Combine);
        }

        // 分離: 構成ユニットを内包している (実行時に合体した) 味方ユニット、または `分離`
        // 特殊能力を持つ味方ユニット (最初から合体形態で `.eve` 配置されたユニット)。
        // 原典 SRC: 分離は行動を消費しない。
        if u.party == crate::Party::Player
            && (!u.combined_from.is_empty()
                || crate::feature::has_feature(&u.active_features, "分離"))
        {
            items.push(UnitMenuItem::Separate);
        }

        items.push(UnitMenuItem::Builtin(UnitAction::Wait));

        {
            // シナリオ定義の `[*|-]ユニットコマンド` で、対象勢力がこのユニットに
            // 合致するものを追記する。
            //
            // 原典 SRC ([ユニットコマンドイベント.md](../../../SRC.Sharp/SRC.Sharp.Help/src/ユニットコマンドイベント.md)):
            //  - `condition` 式の値が 0 でないときだけ表示
            //  - 同時に表示される項目は最大 10 件
            //  - タイミングフラグ:
            //      - `ユニットコマンド`   → 移動前のみ (post_move_ok=false, post_act_ok=false)
            //      - `*ユニットコマンド`  → 移動後も表示 (post_move_ok=true)
            //      - `*-ユニットコマンド` → 同上
            //      - `-*ユニットコマンド` → 行動終了後も表示 (post_act_ok=true)
            //      - `**ユニットコマンド` → 移動後・行動終了後どちらでも表示
            //
            // 加えて、組込コマンド (移動 / 攻撃 / 武装一覧 / 待機) と同名の
            // カスタムコマンドは builtin が優先 (重複表示を避ける)。
            let has_acted = u.has_acted;
            let builtin_names: std::collections::HashSet<&str> = items
                .iter()
                .filter_map(|it| match it {
                    UnitMenuItem::Builtin(a) => Some(a.label()),
                    UnitMenuItem::Custom { .. }
                    | UnitMenuItem::Spirit
                    | UnitMenuItem::Support(_)
                    | UnitMenuItem::Transform
                    | UnitMenuItem::Charge
                    | UnitMenuItem::Ability
                    | UnitMenuItem::Launch
                    | UnitMenuItem::Combine
                    | UnitMenuItem::Separate => None,
                })
                .collect();
            let mut seen: std::collections::HashSet<String> =
                builtin_names.iter().map(|s| s.to_string()).collect();
            const MAX_CUSTOM: usize = 10;
            let mut added = 0usize;
            // condition 評価には `&mut App` が要る (`Call(<label>)` 形式の同期実行のため)。
            // まず候補を収集してから評価する (script_library への借用を回避)。
            let candidates: Vec<(String, Option<String>)> = self
                .script_library()
                .custom_commands
                .iter()
                .filter(|c| {
                    if !c.is_unit || !custom_command_targets_party(&c.target, u.party) {
                        return false;
                    }
                    // タイミングフィルタ (SRC.NET の AsterNum チェックに相当)
                    if has_acted {
                        c.post_act_ok
                    } else if post_move {
                        c.post_move_ok
                    } else {
                        true // 移動前: 全コマンドを表示
                    }
                })
                .map(|c| (c.name.clone(), c.condition.clone()))
                .collect();
            for (name, condition) in candidates {
                if !seen.insert(name.clone()) {
                    continue;
                }
                if !crate::event_runtime::evaluate_command_condition(self, condition.as_deref()) {
                    continue;
                }
                items.push(UnitMenuItem::Custom { name });
                added += 1;
                if added >= MAX_CUSTOM {
                    break;
                }
            }
        }

        self.command_menu = Some(CommandMenu::Unit {
            uid: u.uid.clone(),
            items,
            cursor: 0,
        });
    }

    /// マップコマンドメニューを開く。
    pub(super) fn open_map_menu(&mut self) {
        use crate::command_menu::{CommandMenu, MapAction};
        let mut items = vec![
            MapAction::EndTurn,
            MapAction::UnitList,
            MapAction::Settings,
            MapAction::ToggleAutoCounter,
            MapAction::ToggleWeaponWindow,
        ];
        // 「作戦目的」はシナリオが `勝利条件:` ラベルを定義している場合のみ表示する。
        if self.has_victory_condition_event() {
            items.push(MapAction::VictoryConditions);
        }
        items.push(MapAction::QuickSave);
        items.push(MapAction::QuickLoad);
        self.command_menu = Some(CommandMenu::Map { items, cursor: 0 });
    }

    /// メニュー項目が選ばれた時のアクション解決。
    pub(super) fn execute_menu_action(
        &mut self,
        action: crate::command_menu::MenuActionId,
    ) -> bool {
        use crate::command_menu::{
            ActionMode, CommandMenu, MapAction, MenuActionId, UnitAction, UnitMenuItem,
        };
        match action {
            MenuActionId::Unit(a) => {
                // メニューを閉じてからモード遷移 (対象は uid で束縛)
                let uid = match &self.command_menu {
                    Some(CommandMenu::Unit { uid, .. }) => uid.clone(),
                    _ => return false,
                };
                self.command_menu = None;
                // 移動後メニューのスナップショットを退避してから action_mode をリセット。
                // 端アクション (待機 / 武装一覧 / カスタム) 後に PostMoveMenu が残ると、
                // 続くクリックが no-op になり右クリックで確定済みユニットが巻き戻るため、
                // 既定で Browse に戻し、Move/Attack のみがモード遷移で上書きする。
                let post_move_snapshot =
                    if let ActionMode::PostMoveMenu { snapshot, .. } = &self.action_mode {
                        Some(snapshot.clone())
                    } else {
                        None
                    };
                self.action_mode = ActionMode::Browse;
                // シナリオ定義コマンドは対象ユニットを束縛して本体を実行。
                let a = match a {
                    UnitMenuItem::Builtin(a) => a,
                    UnitMenuItem::Custom { name } => {
                        self.invoke_custom_unit_command(&uid, &name);
                        return true;
                    }
                    UnitMenuItem::Spirit => {
                        // 精神コマンドのサブメニューを開く (SP コマンド一覧)。
                        self.open_spirit_menu(&uid);
                        return true;
                    }
                    UnitMenuItem::Support(kind) => {
                        // 修理 / 補給: 隣接味方の対象選択へ遷移する。
                        self.begin_support_target(&uid, kind);
                        return true;
                    }
                    UnitMenuItem::Transform => {
                        // 変形: 変形先が 1 つなら即変形、複数ならサブメニューを開く。
                        self.open_transform_menu(&uid);
                        return true;
                    }
                    UnitMenuItem::Charge => {
                        // チャージ: charged フラグを立てて行動終了 (完了は次ターン)。
                        if let Some(u) = self.database.unit_by_uid_mut(&uid) {
                            u.charged = true;
                            u.has_acted = true;
                        }
                        self.push_message(
                            "チャージ開始 — 次ターンにチャージ攻撃が可能".to_string(),
                        );
                        if let Some(i) = self.database.idx_by_uid(&uid) {
                            crate::event_runtime::fire_action_end_labels(self, i);
                            // SRC `接触 <unit1> <unit2>:` ─ 行動終了後に隣接ペアで発火。
                            crate::event_runtime::fire_contact_event_labels(self, i);
                        }
                        return true;
                    }
                    UnitMenuItem::Ability => {
                        // アビリティ: 一覧サブメニューを開く。
                        self.open_ability_menu(&uid);
                        return true;
                    }
                    UnitMenuItem::Launch => {
                        // 発進: 格納ユニット選択サブメニューを開く。
                        self.open_launch_menu(&uid);
                        return true;
                    }
                    UnitMenuItem::Combine => {
                        // 合体: 合体相手を取り込み合体形態へ。発動で行動終了。
                        self.apply_combine(&uid);
                        return true;
                    }
                    UnitMenuItem::Separate => {
                        // 分離: 構成ユニットを盤上へ戻す。行動は消費しない。
                        self.apply_separate(&uid);
                        return true;
                    }
                };
                match a {
                    UnitAction::Move => {
                        self.action_mode = ActionMode::MoveSelect { uid };
                        self.push_message("移動先を選択 (右クリックでキャンセル)".to_string());
                    }
                    UnitAction::Attack => {
                        // 移動後メニューからの攻撃なら、キャンセル復帰用にスナップショットを引き継ぐ。
                        self.action_mode = ActionMode::AttackSelect {
                            uid,
                            snapshot: post_move_snapshot,
                        };
                        self.push_message("攻撃目標を選択 (右クリックでキャンセル)".to_string());
                    }
                    UnitAction::WeaponList => {
                        // 武装一覧: 情報表示のみ - 武装名をメッセージに列挙
                        if let Some(u) = self.database.unit_by_uid(&uid) {
                            if let Some(def) = self.database.unit_by_name(&u.unit_data_name) {
                                let names: Vec<String> =
                                    def.weapons.iter().map(|w| w.name.clone()).collect();
                                self.push_message(format!(
                                    "武装: {}",
                                    if names.is_empty() {
                                        "(なし)".to_string()
                                    } else {
                                        names.join(" / ")
                                    }
                                ));
                            }
                        }
                    }
                    UnitAction::Wait => {
                        // 行動終了
                        if let Some(u) = self.database.unit_by_uid_mut(&uid) {
                            u.has_acted = true;
                        }
                        if let Some(i) = self.database.idx_by_uid(&uid) {
                            crate::event_runtime::fire_action_end_labels(self, i);
                            // SRC `接触 <unit1> <unit2>:` ─ 行動終了後に隣接ペアで発火。
                            crate::event_runtime::fire_contact_event_labels(self, i);
                        }
                    }
                }
                true
            }
            MenuActionId::Map(a) => {
                self.command_menu = None;
                match a {
                    MapAction::EndTurn => self.end_phase(),
                    MapAction::UnitList => {
                        self.scene = Scene::PilotList;
                        true
                    }
                    MapAction::Settings => {
                        self.enter_configuration();
                        true
                    }
                    MapAction::ToggleAutoCounter => {
                        let on = self.toggle_auto_counter();
                        self.push_message(format!(
                            "自動反撃モード: {}",
                            if on {
                                "ＯＮ (自動反撃)"
                            } else {
                                "ＯＦＦ (手動選択)"
                            }
                        ));
                        true
                    }
                    MapAction::ToggleWeaponWindow => {
                        let on = self.toggle_weapon_window();
                        self.push_message(format!(
                            "武器選択ウィンドウ: {}",
                            if on {
                                "ＯＮ (攻撃時に選択)"
                            } else {
                                "ＯＦＦ (自動/Wキー)"
                            }
                        ));
                        true
                    }
                    MapAction::VictoryConditions => {
                        // シナリオ定義の `勝利条件:` ラベルを発火する。メニューには
                        // ラベル定義時のみ現れるが、発火に失敗した場合の保険も置く。
                        if !self.fire_victory_condition_event() {
                            self.push_message("勝利条件は設定されていません".to_string());
                        }
                        true
                    }
                    MapAction::QuickSave => {
                        // 現在の状態を JSON 化して script_var `__quicksave` に
                        // 保存。フロントエンド (src-web) は同 var を localStorage
                        // に永続化する責務を持つ。
                        if let Ok(json) = self.to_save_json() {
                            self.set_script_var("__quicksave".to_string(), json);
                            self.push_message("【クイックセーブ】".to_string());
                        }
                        true
                    }
                    MapAction::QuickLoad => {
                        // 直前のクイックセーブを復元。本実装は self を置換できない
                        // ため、フロントエンドが `script_var("__quicksave")` から
                        // JSON を取り出し `App::from_save_json` で復元 + `fire_resume_event`
                        // を呼ぶ責務を持つ。ここでは表示のみ。
                        if !self.script_var("__quicksave").is_empty() {
                            self.push_message("【クイックロード要求】".to_string());
                        } else {
                            self.push_message("クイックセーブデータがありません".to_string());
                        }
                        true
                    }
                }
            }
        }
    }

    /// `from` のユニットを `to` に移動できれば実施。返り値は実行有無。
    /// 条件: 同マス (`from` -> `to`) で別、from にユニット、to にユニットなし、
    /// to が from から MP 内、ユニットの所属が現フェーズと一致。
    pub(super) fn try_move_unit_to(&mut self, from: (u32, u32), to: (u32, u32)) -> bool {
        let unit_idx = self
            .database
            .unit_instances
            .iter()
            .position(|u| u.x == from.0 && u.y == from.1);
        let Some(idx) = unit_idx else {
            return false;
        };

        // 現フェーズの所属ユニットのみ移動可能
        if self.database.unit_instances[idx].party != self.turn.phase.party() {
            return false;
        }

        // to に既に別ユニットがいるなら移動不可
        let occupied = self
            .database
            .unit_instances
            .iter()
            .enumerate()
            .any(|(i, u)| i != idx && u.x == to.0 && u.y == to.1);
        if occupied {
            return false;
        }

        // 地形適応・搭乗種別・特殊能力・装備込み移動力を考慮した到達範囲。
        // 描画 (move range overlay) と同一の GameDatabase::unit_move_range を使う。
        let uid = self.database.unit_instances[idx].uid.clone();
        let range = self.database.unit_move_range(&uid);
        if !range.contains_key(&to) {
            return false;
        }

        // 移動を反映 (pos_index 同期は move_unit に一元化)
        self.database.move_unit(&uid, to.0, to.1);
        self.database.unit_instances[idx].has_moved = true;
        // SRC `進入 <unit> <x> <y>:` (`進入イベント.md`) + 端到達なら `脱出 <unit> <dir>`
        crate::event_runtime::fire_entry_event_labels(self, idx);
        true
    }

    /// `uid` のユニットを移動前スナップショットへ巻き戻し、`has_moved` をクリアする。
    /// 移動前の座標・EN 消費・現在エリアを復元する (SRC.Sharp の cancel 復帰相当)。
    pub(super) fn rollback_move(
        &mut self,
        uid: &str,
        snapshot: &crate::command_menu::MoveSnapshot,
    ) {
        self.database.move_unit(uid, snapshot.x, snapshot.y);
        if let Some(u) = self.database.unit_by_uid_mut(uid) {
            u.en_consumed = snapshot.en_consumed;
            u.current_area = snapshot.current_area.clone();
            u.has_moved = false;
        }
    }

    pub(super) fn handle_click_intermission(&mut self, x: i32, y: i32) -> bool {
        let count = self.intermission_item_count();
        if count == 0 {
            return false;
        }
        let Some(idx) = crate::scene::intermission::hit_item(x, y, count) else {
            return false;
        };
        self.intermission_cursor = idx;
        self.confirm_intermission_selection()
    }

    pub(super) fn handle_click_configuration(&mut self, x: i32, y: i32) -> bool {
        let layout = configuration::ConfigurationLayout::original();
        let Some(hit) = configuration::hit_test(&layout, x, y) else {
            return false;
        };
        use configuration::HitTarget::*;
        match hit {
            Checkbox(field) => {
                field.toggle(&mut self.settings);
                true
            }
            MessageSpeedCombo => {
                self.settings.message_speed = self.settings.message_speed.next();
                true
            }
            MidiResetCombo => {
                self.settings.midi_reset = self.settings.midi_reset.next();
                true
            }
            Mp3VolumeBar { ratio } => {
                self.settings.mp3_volume = (ratio * 100.0).round().clamp(0.0, 100.0) as u8;
                true
            }
            OkButton => {
                self.scene = Scene::MapView;
                self.settings_snapshot = None;
                true
            }
            CancelButton => {
                if let Some(prev) = self.settings_snapshot.take() {
                    self.settings = prev;
                }
                self.scene = Scene::MapView;
                true
            }
        }
    }
}
