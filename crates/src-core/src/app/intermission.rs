//! インターミッション (機体改造・乗り換え・アイテム交換・次ステージ選択)。

use super::*;

impl App {
    /// インターミッション画面に表示する項目数。ユーザ定義 + 「次のステージへ」。
    /// 「次のステージへ」はシナリオが `Continue <file>` で `次ステージ` を
    /// セットしている時のみ末尾に追加される (SRC.Sharp 同等)。
    pub fn intermission_item_count(&self) -> usize {
        match self.intermission_mode {
            IntermissionMode::Menu => self.intermission_menu_items().len(),
            // 機体改造: 対象ユニット + 末尾「戻る」。
            IntermissionMode::UnitUpgrade => self.intermission_upgrade_units().len() + 1,
            // 換装: (ユニット → 換装先) 行 + 末尾「戻る」。
            IntermissionMode::EquipSwap => self.intermission_swap_rows().len() + 1,
            // 乗り換え: 移動元選択は全味方、移動先選択は移動元を除く + 末尾「戻る」。
            IntermissionMode::RideChange => self.ride_change_units().len() + 1,
        }
    }

    /// メインメニューの表示項目を順序付きで構築する。組込みコマンド (機体改造 /
    /// データセーブ) は、表示すべきインターミッション (ユーザ定義項目あり、または
    /// 「次のステージへ」あり) のときだけ追加する。空メニューには足さない。
    pub(super) fn intermission_menu_items(&self) -> Vec<InterItem> {
        let mut items: Vec<InterItem> = (0..self.intermission_commands.len())
            .map(InterItem::User)
            .collect();
        let has_next = !self.script_var("次ステージ").is_empty();
        if !self.intermission_commands.is_empty() || has_next {
            items.push(InterItem::Upgrade);
            // 換装: 換装可能な味方ユニットが居るときのみ。
            if !self.intermission_swap_rows().is_empty() {
                items.push(InterItem::EquipSwap);
            }
            // 乗り換え: シナリオが `Option 乗り換え` で有効化し、かつ入れ替え先がある
            // (味方ユニット 2 体以上) ときのみ表示 (`乗り換え.md` / `Optionコマンド.md`)。
            if !self.script_var("Option(乗り換え)").is_empty()
                && self.intermission_upgrade_units().len() >= 2
            {
                items.push(InterItem::RideChange);
            }
            // ステータス: 味方ユニットが居れば部隊ロスター閲覧を出す。
            if !self.intermission_upgrade_units().is_empty() {
                items.push(InterItem::Status);
            }
            items.push(InterItem::Save);
        }
        if has_next {
            items.push(InterItem::NextStage);
        }
        items
    }

    /// 機体改造で対象に取れる味方ユニットの `unit_instances` index 列。
    pub(super) fn intermission_upgrade_units(&self) -> Vec<usize> {
        self.database
            .unit_instances
            .iter()
            .enumerate()
            .filter(|(_, u)| u.party == crate::Party::Player)
            .map(|(i, _)| i)
            .collect()
    }

    /// 0..intermission_item_count() の n に対応する表示文字列。
    pub fn intermission_item_label(&self, n: usize) -> Option<String> {
        match self.intermission_mode {
            IntermissionMode::Menu => {
                let items = self.intermission_menu_items();
                items.get(n).map(|it| match it {
                    InterItem::User(i) => self.intermission_commands[*i].name.clone(),
                    InterItem::Upgrade => "機体改造".to_string(),
                    InterItem::EquipSwap => "換装".to_string(),
                    InterItem::RideChange => "乗り換え".to_string(),
                    InterItem::Status => "ステータス".to_string(),
                    InterItem::Save => "データセーブ".to_string(),
                    InterItem::NextStage => {
                        crate::scene::intermission::NEXT_STAGE_LABEL.to_string()
                    }
                })
            }
            IntermissionMode::UnitUpgrade => {
                let units = self.intermission_upgrade_units();
                if let Some(&idx) = units.get(n) {
                    let u = &self.database.unit_instances[idx];
                    let name = self.unit_display_name(u);
                    let lv = u.upgrade_level;
                    if lv >= crate::db::UPGRADE_MAX_LEVEL {
                        Some(format!("{name}  改造Lv{lv} (最大)"))
                    } else {
                        Some(format!(
                            "{name}  改造Lv{lv} → 次 {}G",
                            Self::upgrade_cost(lv)
                        ))
                    }
                } else if n == units.len() {
                    Some("← 戻る".to_string())
                } else {
                    None
                }
            }
            IntermissionMode::EquipSwap => {
                let rows = self.intermission_swap_rows();
                if let Some((uid, form)) = rows.get(n) {
                    let cur = self
                        .database
                        .unit_by_uid(uid)
                        .map(|u| self.unit_display_name(u))
                        .unwrap_or_default();
                    Some(format!("{cur} → {form}"))
                } else if n == rows.len() {
                    Some("← 戻る".to_string())
                } else {
                    None
                }
            }
            IntermissionMode::RideChange => {
                let uids = self.ride_change_units();
                if let Some(uid) = uids.get(n) {
                    let label = self.ride_unit_label(uid);
                    // 移動元未選択なら一覧、選択済みなら移動先候補 (→ prefix)。
                    if self.ride_change_source.is_some() {
                        Some(format!("→ {label}"))
                    } else {
                        Some(label)
                    }
                } else if n == uids.len() {
                    Some("← 戻る".to_string())
                } else {
                    None
                }
            }
        }
    }

    /// 乗り換えの選択対象 uid 列。移動元未選択なら全味方、選択済みなら移動元を除く。
    pub(super) fn ride_change_units(&self) -> Vec<String> {
        self.database
            .unit_instances
            .iter()
            .filter(|u| u.party == crate::Party::Player)
            .filter(|u| self.ride_change_source.as_deref() != Some(u.uid.as_str()))
            .map(|u| u.uid.clone())
            .collect()
    }

    /// 乗り換えリストの 1 行表示「ユニット名 (搭乗: パイロット)」。
    pub(super) fn ride_unit_label(&self, uid: &str) -> String {
        let Some(u) = self.database.unit_by_uid(uid) else {
            return String::new();
        };
        let unit = self.unit_display_name(u);
        let pilot = if u.pilot_name.is_empty() {
            "無人".to_string()
        } else {
            u.pilot_name.clone()
        };
        format!("{unit} (搭乗: {pilot})")
    }

    /// 機体改造 Lv `level` → `level+1` への必要資金。
    pub(super) fn upgrade_cost(level: i32) -> i64 {
        i64::from(level + 1) * 1000
    }

    /// ユニットの表示名 (nickname があればそれ、無ければ unit_data_name)。
    pub(super) fn unit_display_name(&self, u: &crate::UnitInstance) -> String {
        self.database
            .unit_by_name(&u.unit_data_name)
            .map(|d| {
                if d.nickname.is_empty() {
                    d.name.clone()
                } else {
                    d.nickname.clone()
                }
            })
            .unwrap_or_else(|| u.unit_data_name.clone())
    }

    /// 機体改造を 1 段階適用する (資金チェック + 消費 + `upgrade_level` 加算)。
    pub(super) fn apply_unit_upgrade(&mut self, idx: usize) {
        let lv = self.database.unit_instances[idx].upgrade_level;
        let name = self.unit_display_name(&self.database.unit_instances[idx]);
        if lv >= crate::db::UPGRADE_MAX_LEVEL {
            self.push_message(format!("{name} はこれ以上改造できません (最大 Lv{lv})"));
            return;
        }
        let cost = Self::upgrade_cost(lv);
        if self.money() < cost {
            self.push_message(format!(
                "資金が足りません ({name} 改造: 必要 {cost}G / 所持 {}G)",
                self.money()
            ));
            return;
        }
        self.add_money(-cost);
        self.database.unit_instances[idx].upgrade_level = lv + 1;
        self.push_message(format!("{name} を改造しました (Lv{} / -{cost}G)", lv + 1));
    }

    /// ユニット `u` の特殊能力「換装」が持つ換装先フォーム名の列を返す。
    /// 書式 `換装=<換装先1> <換装先2> …`（`換装.md`）。`active_features` を優先し、
    /// 空なら現フォームの `UnitData.features` から引く。DB に存在する形態のみ採用し、
    /// `非表示` トークンは除外する。
    pub(super) fn swap_forms_for(&self, u: &crate::UnitInstance) -> Vec<String> {
        let val = crate::feature::feature_value(&u.active_features, "換装")
            .map(str::to_string)
            .or_else(|| {
                self.database.unit_by_name(&u.unit_data_name).and_then(|d| {
                    d.features
                        .iter()
                        .find(|(n, _)| n == "換装")
                        .map(|(_, v)| v.clone())
                })
            });
        let Some(val) = val else {
            return Vec::new();
        };
        val.split_whitespace()
            .filter(|t| *t != "非表示" && self.database.unit_by_name(t).is_some())
            .map(str::to_string)
            .collect()
    }

    /// 換装サブモードの行: 換装可能な味方ユニットの各換装先を平坦化した
    /// `(uid, 換装先フォーム名)` の列。
    pub(super) fn intermission_swap_rows(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        for u in &self.database.unit_instances {
            if u.party != crate::Party::Player {
                continue;
            }
            for f in self.swap_forms_for(u) {
                rows.push((u.uid.clone(), f));
            }
        }
        rows
    }

    /// 換装を適用する。`set_unit_form` で形態を差し替え（資金消費なし、イベント無し）。
    pub(super) fn apply_equip_swap(&mut self, uid: &str, new_form: &str) {
        let name = self
            .database
            .unit_by_uid(uid)
            .map(|u| self.unit_display_name(u))
            .unwrap_or_default();
        // 換装先の形態の必要技能 (搭乗員が満たさないと換装不可)。
        if let Some(uidx) = self.database.idx_by_uid(uid) {
            if !self.form_skill_ok(uidx, new_form) {
                self.push_message(format!(
                    "必要技能を満たさないため {new_form} に換装できません"
                ));
                return;
            }
        }
        if self.set_unit_form(uid, new_form) {
            let new_name = self
                .database
                .unit_by_name(new_form)
                .map(|d| {
                    if d.nickname.is_empty() {
                        d.name.clone()
                    } else {
                        d.nickname.clone()
                    }
                })
                .unwrap_or_else(|| new_form.to_string());
            self.push_message(format!("{name} を {new_name} に換装しました"));
        }
    }

    /// インターミッションの「データセーブ」。`to_save_json()` を `__quicksave` に
    /// 格納する (マップコマンドの QuickSave と同経路)。フロントエンドが永続化する。
    /// データセーブを実行する。現在の状態を JSON 化し `__quicksave` script_var に
    /// 保存する (フロントエンドが localStorage に永続化する責務を持つ)。インター
    /// ミッションメニューの「データセーブ」と `.eve CallIntermissionCommand データセーブ`
    /// の両経路から共有する。
    pub(crate) fn intermission_data_save(&mut self) {
        match self.to_save_json() {
            Ok(json) => {
                self.set_script_var("__quicksave".to_string(), json);
                self.push_message("データをセーブしました".to_string());
            }
            Err(e) => self.push_message(format!("セーブに失敗しました: {e}")),
        }
    }

    /// ユーザ定義インターミッションコマンドの `.eve` (`プロローグ`) を起動する。
    /// サブコマンド実行中は MapView に切り替え、完了で Intermission に戻る。
    pub(super) fn run_user_intermission_command(&mut self, cmd_idx: usize) {
        let file = self.intermission_commands[cmd_idx].file.clone();
        self.intermission_running = true;
        self.scene = Scene::MapView;
        self.script_overlay.clear();
        self.hotpoints.clear();
        self.flow
            .push(crate::flow::FlowCont::ReturnToIntermissionMenu);
        let mut started = crate::event_runtime::trigger_label_in_file(self, &file, "プロローグ");
        if !started {
            if let Some(entry) = self.script_library.find_file(&file) {
                let pc = entry.start_pc;
                let _ = crate::event_runtime::run_from_pc(self, pc);
                started = true;
            }
        }
        if !started && self.flow.last() == Some(&crate::flow::FlowCont::ReturnToIntermissionMenu) {
            self.flow.pop();
            self.return_from_intermission_subcommand_if_idle();
        }
    }

    /// 「次のステージへ」を確定する。`advance_to_next_stage` で次シナリオへ進む。
    /// `Continue` で次ステージを再予約した場合は scene=Intermission を尊重する。
    pub(super) fn select_next_stage(&mut self) -> bool {
        self.intermission_running = false;
        if self.advance_to_next_stage() {
            if !self.script_var("次ステージ").is_empty() {
                return true;
            }
            self.scene = Scene::MapView;
            return true;
        }
        false
    }

    /// 現在カーソル位置の項目を実行。モードに応じて分岐する。
    pub(super) fn confirm_intermission_selection(&mut self) -> bool {
        match self.intermission_mode {
            IntermissionMode::Menu => {
                let items = self.intermission_menu_items();
                let Some(&item) = items.get(self.intermission_cursor) else {
                    return false;
                };
                match item {
                    InterItem::User(i) => {
                        self.run_user_intermission_command(i);
                        true
                    }
                    InterItem::Upgrade => {
                        self.intermission_mode = IntermissionMode::UnitUpgrade;
                        self.intermission_cursor = 0;
                        true
                    }
                    InterItem::EquipSwap => {
                        self.intermission_mode = IntermissionMode::EquipSwap;
                        self.intermission_cursor = 0;
                        true
                    }
                    InterItem::RideChange => {
                        self.intermission_mode = IntermissionMode::RideChange;
                        self.ride_change_source = None;
                        self.intermission_cursor = 0;
                        true
                    }
                    InterItem::Status => {
                        // 味方ロスター先頭機の単機ステータス詳細を開く。`◀ / ▶` で
                        // 巡回し、閉じる / Esc / 右クリックでインターミッションへ戻る。
                        self.status_detail_index = 0;
                        self.scene = Scene::UnitDetail;
                        self.scene_return_to = Some(Scene::Intermission);
                        true
                    }
                    InterItem::Save => {
                        self.intermission_data_save();
                        true
                    }
                    InterItem::NextStage => self.select_next_stage(),
                }
            }
            IntermissionMode::UnitUpgrade => {
                let units = self.intermission_upgrade_units();
                if let Some(&idx) = units.get(self.intermission_cursor) {
                    self.apply_unit_upgrade(idx);
                    true
                } else {
                    // 末尾「戻る」: メインメニューへ。
                    self.intermission_mode = IntermissionMode::Menu;
                    self.intermission_cursor = 0;
                    true
                }
            }
            IntermissionMode::EquipSwap => {
                let rows = self.intermission_swap_rows();
                if let Some((uid, form)) = rows.get(self.intermission_cursor).cloned() {
                    self.apply_equip_swap(&uid, &form);
                    // カーソルが範囲外にならないようクランプ (換装で行数が変わりうる)。
                    let count = self.intermission_item_count();
                    if self.intermission_cursor >= count {
                        self.intermission_cursor = count.saturating_sub(1);
                    }
                    true
                } else {
                    self.intermission_mode = IntermissionMode::Menu;
                    self.intermission_cursor = 0;
                    true
                }
            }
            IntermissionMode::RideChange => {
                let uids = self.ride_change_units();
                let Some(uid) = uids.get(self.intermission_cursor).cloned() else {
                    // 末尾「戻る」: 移動元選択済みなら解除して移動元選択へ、
                    // そうでなければメインメニューへ。
                    if self.ride_change_source.take().is_some() {
                        self.intermission_cursor = 0;
                    } else {
                        self.intermission_mode = IntermissionMode::Menu;
                        self.intermission_cursor = 0;
                    }
                    return true;
                };
                match self.ride_change_source.take() {
                    None => {
                        // 移動元を確定 → 移動先選択へ。
                        self.ride_change_source = Some(uid);
                        self.intermission_cursor = 0;
                    }
                    Some(src) => {
                        // 移動先を確定 → 搭乗パイロットを入れ替え。
                        self.apply_ride_change(&src, &uid);
                        self.intermission_cursor = 0;
                    }
                }
                true
            }
        }
    }

    /// 2 ユニットの搭乗パイロット (`pilot_name` + `pilot_ids`) を入れ替える。
    pub(super) fn swap_unit_pilots(&mut self, a: usize, b: usize) {
        let an = std::mem::take(&mut self.database.unit_instances[a].pilot_name);
        let ai = std::mem::take(&mut self.database.unit_instances[a].pilot_ids);
        let bn = std::mem::take(&mut self.database.unit_instances[b].pilot_name);
        let bi = std::mem::take(&mut self.database.unit_instances[b].pilot_ids);
        self.database.unit_instances[a].pilot_name = bn;
        self.database.unit_instances[a].pilot_ids = bi;
        self.database.unit_instances[b].pilot_name = an;
        self.database.unit_instances[b].pilot_ids = ai;
    }

    /// 乗り換え: 2 ユニットの搭乗パイロットを入れ替える。
    /// 片方が無人でも入れ替え可 (空ユニットが生じうる点は SRC と同じ player 責務)。
    /// 形態の必要技能 (`必要技能.md` §4: 乗り換え先のパイロット制限) を満たさない組合せは
    /// 入れ替え後にチェックして revert する。インターミッション (off_map) では地形/隣接
    /// 条件は fail-open となり、実質パイロット技能条件のみが効く (原典の「常に満たす」に整合)。
    pub(super) fn apply_ride_change(&mut self, src_uid: &str, dst_uid: &str) {
        if src_uid == dst_uid {
            return;
        }
        let (Some(si), Some(di)) = (
            self.database.idx_by_uid(src_uid),
            self.database.idx_by_uid(dst_uid),
        ) else {
            return;
        };
        let src_name = self.unit_display_name(&self.database.unit_instances[si]);
        let dst_name = self.unit_display_name(&self.database.unit_instances[di]);
        self.swap_unit_pilots(si, di);
        // 入れ替え後の搭乗員が双方の形態の必要技能を満たすか。満たさなければ revert。
        let src_form = self.database.unit_instances[si].unit_data_name.clone();
        let dst_form = self.database.unit_instances[di].unit_data_name.clone();
        if !self.form_skill_ok(si, &src_form) || !self.form_skill_ok(di, &dst_form) {
            self.swap_unit_pilots(si, di);
            self.push_message(format!(
                "必要技能を満たさないため {src_name} と {dst_name} を乗り換えできません"
            ));
            return;
        }
        self.push_message(format!("{src_name} と {dst_name} の搭乗を入れ替えました"));
    }

    /// (dialog / timer / 中断コンテキスト) なら何もしない。
    ///
    /// `respond_dialog` / `respond_dialog_text` / `tick` の resume 後に
    /// 呼ばれ、サブコマンドの Exit を検出してメニューに復帰させる。
    pub fn return_from_intermission_subcommand_if_idle(&mut self) {
        if !self.intermission_running {
            return;
        }
        let idle = self.script_ctx.is_none()
            && self.pending_dialog.is_none()
            && self.pending_timer.is_none();
        if idle {
            self.intermission_running = false;
            self.scene = Scene::Intermission;
            self.intermission_mode = IntermissionMode::Menu;
            self.script_overlay.clear();
            self.hotpoints.clear();
        }
    }
}
