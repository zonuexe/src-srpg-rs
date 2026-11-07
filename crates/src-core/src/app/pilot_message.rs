//! 戦闘中のパイロットの台詞 (原典 `Unit.PilotMessage` / `Unit.IsMessageDefined` /
//! `Unit.PlayMessage`)。メッセージデータ (`pilot_message.txt`) からシチュエーションに合う
//! 台詞を選び、メッセージ欄に `愛称「台詞」` の形で出す。
//!
//! 話し手と相手はユニットの複製で受け取る (撃破されたユニットも「破壊」の台詞を言うため)。
//! ダイアログデータ (`pilot_dialog.txt`) があればそちらを先に使う。`SelectMessage` コマンド・
//! 複数乗りの話し手選び・合体技のパートナーはまだ扱わない。

use super::App;
use crate::data::dialog::Dialog;
use crate::data::message::{select_by_situation, select_message, SelectContext};
use crate::data::unit::WeaponData;
use crate::unit_instance::UnitInstance;

/// 台詞を選ぶときの状況 (原典では `SelectedUnit` / `SelectedTarget` / `IsDefense` から
/// 求めるもの)。
#[derive(Clone, Copy, Default)]
pub(super) struct MsgCtx<'a> {
    /// 戦っている相手。攻撃の当事者 (攻撃側と目標) だけが持ち、援護役は持たない。
    pub target: Option<&'a UnitInstance>,
    /// 話し手が使っている武器。
    pub weapon: Option<&'a WeaponData>,
    /// 相手が使っている武器。
    pub target_weapon: Option<&'a WeaponData>,
    /// 反撃している側か (原典 `IsDefense`)。
    pub defending: bool,
}

/// 原典 `PilotMessage` の `msg_mode`。
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum MsgMode {
    /// 攻撃以外の台詞。
    Plain,
    /// 武器名で呼ぶ攻撃の台詞。
    Attack,
    /// カウンター (先制攻撃) の攻撃の台詞。
    Counter,
}

/// 格闘系の武器か (原典 `IsWeaponClassifiedAs(w, "格闘系")`。戦闘計算と同じ判定)。
fn is_melee(w: &WeaponData) -> bool {
    w.class.contains('格') || (!w.class.contains('射') && w.max_range == 1)
}

/// 名前の `(…)` 以降と `専用` までを取り除く (原典 `PlayMessage` の `$(ユニット)` 等)。
fn short_unit_name(name: &str) -> String {
    let name = name.split('(').next().unwrap_or(name);
    match name.find("専用") {
        Some(p) => name[p + "専用".len()..].to_string(),
        None => name.to_string(),
    }
}

/// 武器名の `(…)` / `<…>` / `＜…＞` 以降を取り除く。
fn short_weapon_name(name: &str) -> &str {
    name.split(['(', '<', '＜']).next().unwrap_or(name)
}

impl App {
    /// ユニットの主パイロットのメッセージタイプ。
    fn message_type(&self, u: &UnitInstance) -> String {
        self.message_type_of(&u.pilot_name)
    }

    /// パイロットのメッセージタイプ (パイロット用特殊能力「メッセージ」があればその値、
    /// 無ければパイロット名)。
    fn message_type_of(&self, pilot_name: &str) -> String {
        self.database
            .pilot_by_name(pilot_name)
            .and_then(|p| {
                p.features
                    .iter()
                    .find(|(n, v)| n == "メッセージ" && !v.is_empty())
                    .map(|(_, v)| v.clone())
            })
            .unwrap_or_else(|| pilot_name.to_string())
    }

    /// ダイアログデータを引くパイロットリストの候補 (原典 `PilotMessage` の `pnames`)。
    /// 乗員全員 → 主パイロットの順。移植版はサポートパイロットを区別しないので、
    /// 「パイロット全員とサポート全員」と「パイロット全員」は同じになる。
    fn dialog_keys(&self, u: &UnitInstance) -> Vec<String> {
        let crew: Vec<String> = u
            .pilot_ids
            .iter()
            .filter_map(|id| {
                self.database
                    .pilot_instances
                    .iter()
                    .find(|p| &p.id == id)
                    .map(|p| self.message_type_of(&p.pilot_data_name))
            })
            .collect();
        let main = self.message_type(u);
        let mut keys = Vec::new();
        if crew.len() > 1 {
            keys.push(crew.join(" "));
        }
        keys.push(main);
        keys
    }

    fn message_dice(&mut self, n: usize) -> usize {
        (self.next_u32() % n.max(1) as u32) as usize + 1
    }

    /// 原典 `SelectMessage` に渡す話し手と相手の情報。
    fn select_context(&self, u: &UnitInstance, situation: &str, ctx: &MsgCtx) -> SelectContext {
        let db = &self.database;
        let unit = db.unit_by_name(&u.unit_data_name);
        let pilot = db.pilot_by_name(&u.pilot_name);
        let mut unit_subs = vec![format!("({})", u.unit_data_name)];
        if let Some(d) = unit {
            unit_subs.push(format!("({})", d.nickname));
            unit_subs.push(format!("({})", d.class));
        }
        let message_class = |d: Option<&crate::data::unit::UnitData>| {
            d.and_then(|d| {
                d.features
                    .iter()
                    .find(|(n, _)| n == "メッセージクラス")
                    .map(|(_, v)| v.clone())
            })
        };
        if let Some(c) = message_class(unit) {
            unit_subs.push(format!("({c})"));
        }
        let target_subs = ctx.target.map(|t| {
            let tunit = db.unit_by_name(&t.unit_data_name);
            let tpilot = db.pilot_by_name(&t.pilot_name);
            let mut subs = vec![
                format!("(対{})", t.pilot_name),
                format!("(対{})", t.unit_data_name),
            ];
            if let Some(p) = tpilot {
                subs.push(format!("(対{})", p.nickname));
                match p.sex {
                    crate::data::pilot::Sex::Male => subs.push("(対男性)".into()),
                    crate::data::pilot::Sex::Female => subs.push("(対女性)".into()),
                    crate::data::pilot::Sex::Unspecified => {}
                }
                for (n, _) in &p.features {
                    subs.push(format!("(対{})", strip_level(n)));
                }
            }
            if let Some(d) = tunit {
                subs.push(format!("(対{})", d.nickname));
                subs.push(format!("(対{})", d.class));
                subs.push(format!("(対{})", d.size.label()));
                for (n, _) in &d.features {
                    subs.push(format!("(対{})", strip_level(n)));
                }
            }
            if let Some(c) = message_class(tunit) {
                subs.push(format!("(対{c})"));
            }
            if let Some(name) = db
                .map
                .as_ref()
                .filter(|m| t.x < m.width && t.y < m.height)
                .and_then(|m| crate::data::terrain::lookup(m.cell(t.x, t.y).terrain_id))
                .map(|t| t.name)
            {
                subs.push(format!("(対{name})"));
            }
            // 対ザコ / 対強敵 (技量と HP で比べる)。
            let tech = |p: Option<&crate::data::pilot::PilotData>| p.map_or(0, |p| p.technique);
            let zako = t.pilot_name.contains("(ザコ)");
            let hp = |x: &UnitInstance| db.effective_max_hp(x) - x.damage;
            if zako && (tech(pilot) > tech(tpilot) || hp(u) > hp(t) / 2) {
                subs.push("(対ザコ)".into());
            }
            if t.boss_rank > 0 || (!zako && tech(pilot) <= tech(tpilot)) {
                subs.push("(対強敵)".into());
            }
            if let Some(w) = ctx.target_weapon {
                subs.push(format!("(対{})", w.name));
                for ch in w.class.chars().filter(|c| !c.is_ascii()) {
                    subs.push(format!("(対{ch}属性)"));
                }
            }
            subs
        });
        let force_target = situation.contains("(とどめ)")
            || ["挑発", "脱力", "魅惑", "威圧"].contains(&situation)
            || ctx.target.is_some_and(|t| t.party == u.party);
        SelectContext {
            target_subs,
            force_target,
            unit_subs,
            weapon_sub: ctx
                .weapon
                .filter(|_| !ctx.defending && ctx.target.is_some())
                .map(|w| w.name.clone()),
        }
    }

    /// `situation` の台詞を選ぶ。メッセージデータが無ければ `None`。
    fn select_pilot_message(
        &mut self,
        u: &UnitInstance,
        situation: &str,
        ctx: &MsgCtx,
    ) -> Option<String> {
        let mtype = self.message_type(u);
        let entries = self.database.catalog.messages.get(&mtype)?.to_vec();
        let sctx = self.select_context(u, situation, ctx);
        select_message(&entries, situation, &sctx, &mut |n| self.message_dice(n))
    }

    /// パイロットリスト `key` のダイアログから `situation` のものを選ぶ (原典
    /// `DialogData.SelectDialog`)。話し手のだれかがしゃべれないダイアログは選ばない。
    fn select_pilot_dialog(
        &mut self,
        u: &UnitInstance,
        key: &str,
        situation: &str,
        ctx: &MsgCtx,
    ) -> Option<Dialog> {
        let dialogs = self.database.catalog.dialogs.get(key)?.to_vec();
        let available: Vec<bool> = dialogs
            .iter()
            .map(|d| self.dialog_available(u, d))
            .collect();
        let sctx = self.select_context(u, situation, ctx);
        let indexed: Vec<(usize, &Dialog)> = dialogs.iter().enumerate().collect();
        select_by_situation(
            &indexed,
            |(_, d)| d.situation.as_str(),
            |(i, _)| available[*i],
            situation,
            &sctx,
            &mut |n| self.message_dice(n),
        )
        .map(|(_, d)| (*d).clone())
    }

    /// ダイアログの話し手が全員しゃべれるか (原典 `Dialog.IsAvailable`)。主パイロットと
    /// 非戦闘員は常にいる。ほかのパイロットは、作成されていて、睡眠・麻痺・石化・恐怖・
    /// 沈黙・混乱 でないこと。合体技のパートナー (`@名前`) はまだ扱わないので使わない。
    fn dialog_available(&self, u: &UnitInstance, d: &Dialog) -> bool {
        let db = &self.database;
        let mtype = self.message_type(u);
        let main_nickname = db.pilot_by_name(&u.pilot_name).map(|p| p.nickname.as_str());
        d.lines.iter().all(|line| {
            let mut name = line.speaker.as_str();
            if name.starts_with('@') {
                return false;
            }
            // 表情パターン「名前(表情)」は名前で判定する。
            if let Some(p) = name.rfind('(') {
                let base = &name[..p];
                if db.catalog.non_pilots.contains(name) && db.pilot_by_name(base).is_some() {
                    name = base;
                }
            }
            if name == u.pilot_name || Some(name) == main_nickname || name == mtype {
                return true;
            }
            if db.catalog.non_pilots.contains(name) {
                return true;
            }
            let Some(p) = db.pilot_by_name(name) else {
                // パイロットでない話者 (`システム` 等)。
                return true;
            };
            let unit = db.unit_instances.iter().find(|x| x.pilot_name == p.name);
            let created = unit.is_some()
                || db
                    .pilot_instances
                    .iter()
                    .any(|pi| pi.pilot_data_name == p.name);
            created
                && !unit.is_some_and(|x| {
                    ["睡眠", "麻痺", "石化", "恐怖", "沈黙", "混乱"]
                        .iter()
                        .any(|c| x.has_condition(c))
                })
        })
    }

    /// `situation` の台詞がダイアログデータかメッセージデータにあるか。
    fn has_pilot_line(&mut self, u: &UnitInstance, situation: &str, ctx: &MsgCtx) -> bool {
        self.dialog_keys(u)
            .iter()
            .any(|key| self.select_pilot_dialog(u, key, situation, ctx).is_some())
            || self.select_pilot_message(u, situation, ctx).is_some()
    }

    /// しゃべれない状態か (原典 `IsMessageDefined` の判定)。
    fn cannot_speak(u: &UnitInstance, situation: &str) -> bool {
        ["沈黙", "憑依", "石化", "凍結", "麻痺", "睡眠"]
            .iter()
            .any(|c| u.has_condition(c))
            || (u.has_condition("恐怖") && situation != "恐怖")
            || (u.has_condition("混乱") && situation != "混乱")
    }

    /// 原典 `Unit.IsMessageDefined`。
    pub(super) fn is_message_defined(
        &mut self,
        u: &UnitInstance,
        situation: &str,
        ctx: &MsgCtx,
    ) -> bool {
        !Self::cannot_speak(u, situation) && self.has_pilot_line(u, situation, ctx)
    }

    /// 原典 `Unit.PilotMessage`。`situation` の台詞を、見つからなければ代わりの
    /// シチュエーション (`分身` なら `回避` 等) の台詞を出す。
    pub(super) fn pilot_message(
        &mut self,
        u: &UnitInstance,
        situation: &str,
        mode: MsgMode,
        ctx: &MsgCtx,
    ) {
        let melee = ctx.weapon.is_some_and(is_melee);
        let kind = if melee { "格闘" } else { "射撃" };
        let mut wname = String::new();
        let mut situations = vec![situation.to_string()];
        match situation {
            "分身"
            | "切り払い"
            | "迎撃"
            | "反射"
            | "当て身技"
            | "阻止"
            | "ダミー"
            | "緊急テレポート" => situations.push("回避".into()),
            "ビーム無効化" | "攻撃無効化" | "シールド防御" => {
                situations.push("ダメージ小".into())
            }
            "回避" | "破壊" | "ダメージ大" | "ダメージ中" | "ダメージ小" | "かけ声" =>
                {}
            _ if mode != MsgMode::Plain => {
                wname = situation.to_string();
                situations = if !ctx.defending {
                    vec![situation.into(), kind.into()]
                } else if mode == MsgMode::Counter {
                    vec![format!("{situation}(反撃)"), situation.into(), kind.into()]
                } else {
                    vec![
                        format!("{situation}(反撃)"),
                        situation.into(),
                        format!("{kind}(反撃)"),
                        kind.into(),
                    ]
                };
            }
            _ if ["(命中)", "(回避)", "(とどめ)", "(クリティカル)"]
                .iter()
                .any(|s| situation.contains(s)) =>
            {
                // サブシチュエーション付きの攻撃の台詞。
                let p = situation.rfind('(').unwrap_or(0);
                wname = situation[..p].to_string();
                let sub = &situation[p..];
                situations = if !ctx.defending {
                    vec![situation.into(), format!("{kind}{sub}")]
                } else {
                    vec![
                        format!("{situation}(反撃)"),
                        situation.into(),
                        format!("{kind}{sub}(反撃)"),
                        format!("{kind}{sub}"),
                    ]
                };
            }
            _ => {
                if let Some(w) = ctx.weapon {
                    wname = w.name.clone();
                }
            }
        }

        // かけ声は 3 分の 2 の確率で出す。
        if situation.starts_with("かけ声") && self.message_dice(3) == 1 {
            return;
        }
        // しゃべれない場合。
        if ["石化", "凍結", "麻痺"].iter().any(|c| u.has_condition(c)) {
            return;
        }
        if u.has_condition("沈黙") || u.has_condition("憑依") {
            if !situation.contains('(') && situation != "かけ声" {
                self.display_battle_message(Some(&self.speaker_name(&u.pilot_name)), "…………");
            }
            return;
        }
        if u.has_condition("睡眠") {
            if !situation.contains('(') {
                self.display_battle_message(Some(&self.speaker_name(&u.pilot_name)), "ＺＺＺ……");
            }
            return;
        }
        if u.has_condition("恐怖") {
            if self.has_pilot_line(u, "恐怖", ctx) {
                situations = vec!["恐怖".into()];
            } else {
                if !situation.contains('(') {
                    let sex = self.database.pilot_by_name(&u.pilot_name).map(|p| p.sex);
                    let lines: &[&str] = match sex {
                        Some(crate::data::pilot::Sex::Male) => &[
                            "う、うわああああっ！",
                            "うわあああっ！",
                            "ひ、ひいいいっ！",
                            "ひいいいっ！",
                        ],
                        Some(crate::data::pilot::Sex::Female) => &[
                            "きゃあああああっ！",
                            "きゃああっ！",
                            "うわあああっ！",
                            "た、助けてええっ！",
                        ],
                        _ => &[],
                    };
                    if !lines.is_empty() {
                        let msg = lines[self.message_dice(4) - 1];
                        self.display_battle_message(Some(&self.speaker_name(&u.pilot_name)), msg);
                    }
                }
                return;
            }
        }
        if u.has_condition("混乱") && self.has_pilot_line(u, "混乱", ctx) {
            situations = vec!["混乱".into()];
        }

        // ダイアログデータ (パイロットリストの優先順)、メッセージデータの順に探す。
        for key in self.dialog_keys(u) {
            for s in &situations {
                if let Some(d) = self.select_pilot_dialog(u, &key, s, ctx) {
                    self.play_dialog(u, &d, &wname, ctx);
                    return;
                }
            }
        }
        for s in &situations {
            if let Some(msg) = self.select_pilot_message(u, s, ctx) {
                self.play_message(u, &msg, &wname, ctx);
                return;
            }
        }
    }

    /// 原典 `Unit.PlayDialog`。話し手ごとに `$(ユニット)` 等を置き換えて表示する。
    /// メッセージが `-` の行で打ち切る。
    fn play_dialog(&mut self, u: &UnitInstance, d: &Dialog, wname: &str, ctx: &MsgCtx) {
        for line in &d.lines {
            if line.message == "-" {
                return;
            }
            let msg = self.expand_message(u, &line.message, wname, ctx);
            let speaker = if line.speaker == u.pilot_name {
                Some(self.speaker_name(&u.pilot_name))
            } else if line.speaker == "システム" {
                None
            } else {
                let name = line.speaker.trim_start_matches('@');
                Some(self.speaker_name(name))
            };
            self.display_battle_message(speaker.as_deref(), &msg);
        }
    }

    /// 原典 `Unit.PlayMessage`。`$(ユニット)` 等を置き換えて表示する。`-` は表示しない。
    fn play_message(&mut self, u: &UnitInstance, msg: &str, wname: &str, ctx: &MsgCtx) {
        if msg == "-" {
            return;
        }
        let msg = self.expand_message(u, msg, wname, ctx);
        let speaker = self.speaker_name(&u.pilot_name);
        self.display_battle_message(Some(&speaker), &msg);
    }

    /// 台詞の `$(ユニット)` / `$(パイロット)` / `$(武器)` / `$(相手…)` / `$(損傷率)` を
    /// 置き換える (原典 `PlayMessage` / `PlayDialog`)。
    fn expand_message(&self, u: &UnitInstance, msg: &str, wname: &str, ctx: &MsgCtx) -> String {
        let db = &self.database;
        let nickname = |x: &UnitInstance| {
            short_unit_name(
                &db.unit_by_name(&x.unit_data_name)
                    .map_or_else(|| x.unit_data_name.clone(), |d| d.nickname.clone()),
            )
        };
        let pilot_nickname = |x: &UnitInstance| {
            let n = db
                .pilot_by_name(&x.pilot_name)
                .map_or_else(|| x.pilot_name.clone(), |p| p.nickname.clone());
            n.split('(').next().unwrap_or(&n).to_string()
        };
        let damage_rate = |x: &UnitInstance| {
            let max = db.effective_max_hp(x).max(1);
            (100 * x.damage.clamp(0, max) / max).to_string()
        };
        let mut msg = msg
            .replace("$(ユニット)", &nickname(u))
            .replace("$(機体)", &nickname(u))
            .replace("$(パイロット)", &pilot_nickname(u))
            .replace("$(武器)", short_weapon_name(wname))
            .replace("$(損傷率)", &damage_rate(u));
        if let Some(t) = ctx.target {
            let tw = ctx.target_weapon.map_or("", |w| short_weapon_name(&w.name));
            msg = msg
                .replace("$(相手ユニット)", &nickname(t))
                .replace("$(相手機体)", &nickname(t))
                .replace("$(相手パイロット)", &pilot_nickname(t))
                .replace("$(相手武器)", tw)
                .replace("$(相手損傷率)", &damage_rate(t));
        }
        msg
    }

    /// 話し手の表示名 (パイロット・非戦闘員の愛称。見つからなければ名前のまま)。
    fn speaker_name(&self, name: &str) -> String {
        self.database
            .pilot_by_name(name)
            .map_or_else(|| name.to_string(), |p| p.nickname.clone())
    }

    /// 原典 `DisplayBattleMessage` の代わりに、メッセージ欄へ `愛称「台詞」` を出す
    /// (話し手が無い `システム` は台詞だけ)。`;` はページ送り、`:` はクリック待ちなので、
    /// `;` ごとに 1 行にして `:` は消す。効果音の指定 (`.wav` 等) は出さない。
    fn display_battle_message(&mut self, speaker: Option<&str>, msg: &str) {
        for page in msg.split(';') {
            let text = page.replace(':', "");
            let text = text.trim();
            let lower = text.to_ascii_lowercase();
            if text.is_empty()
                || [".wav", ".mid", ".mp3", ".ogg"]
                    .iter()
                    .any(|e| lower.ends_with(e))
            {
                continue;
            }
            self.push_message(match speaker {
                Some(name) => format!("{name}「{text}」"),
                None => text.to_string(),
            });
        }
    }
}

/// 攻撃の前に言う台詞の種類 (原典 `Unit.Attack` の `second_attack` / `attack_mode`)。
#[derive(Clone, Copy)]
pub(super) enum Opening<'a> {
    /// 通常の攻撃・反撃・先制攻撃。
    Normal,
    /// 再攻撃の 2 回目。
    Reattack,
    /// カウンター。
    Counter,
    /// 援護攻撃。中身は援護される攻撃側。
    Support(&'a UnitInstance),
}

/// 1 回の攻撃の結果 (台詞選び用)。
#[derive(Clone, Copy, Default)]
pub(super) struct StrikeReport {
    pub hit: bool,
    pub critical: bool,
    /// 目標が切り払った (`hit` も true)。
    pub parried: bool,
    /// 目標が撃破された。
    pub killed: bool,
    /// みがわり・援護防御で目標は攻撃を受けなかった。
    pub covered: bool,
    /// 目標が受けたダメージ。
    pub damage: i64,
    /// 攻撃後の目標の HP。
    pub hp_after: i64,
    /// 目標の最大 HP。
    pub max_hp: i64,
}

impl App {
    /// 攻撃の前の台詞 (原典 `Unit.Attack` の「攻撃メッセージの前に出力されるメッセージ」と
    /// 攻撃メッセージ)。`ctx.weapon` が使う武器。
    pub(super) fn attack_opening_messages(
        &mut self,
        atk: &UnitInstance,
        opening: Opening,
        ctx: &MsgCtx,
    ) {
        let Some(weapon) = ctx.weapon else {
            return;
        };
        let wname = weapon.name.as_str();
        match opening {
            Opening::Reattack => {
                self.pilot_message(atk, "再攻撃", MsgMode::Plain, ctx);
                return;
            }
            Opening::Support(leader) => {
                let pilot = self.database.pilot_by_name(&leader.pilot_name);
                let names = [
                    Some(leader.pilot_name.clone()),
                    pilot.map(|p| p.nickname.clone()),
                ];
                let candidates: Vec<String> = names
                    .into_iter()
                    .flatten()
                    .map(|n| format!("サポートアタック({n})"))
                    .chain(["サポートアタック".to_string()])
                    .collect();
                for s in candidates {
                    if self.is_message_defined(atk, &s, ctx) {
                        self.pilot_message(atk, &s, MsgMode::Plain, ctx);
                        break;
                    }
                }
            }
            Opening::Counter => self.pilot_message(atk, "カウンター", MsgMode::Plain, ctx),
            Opening::Normal => {
                if !["格闘", "射撃", "攻撃"].contains(&wname)
                    && !weapon.class.contains('合')
                    && self.is_message_defined(atk, wname, ctx)
                {
                    let own = format!("かけ声({wname})");
                    let s = if self.is_message_defined(atk, &own, ctx) {
                        own
                    } else if ctx.defending {
                        "かけ声(反撃)".to_string()
                    } else {
                        "かけ声".to_string()
                    };
                    self.pilot_message(atk, &s, MsgMode::Plain, ctx);
                }
            }
        }
        let mode = if matches!(opening, Opening::Counter) {
            MsgMode::Counter
        } else {
            MsgMode::Attack
        };
        self.pilot_message(atk, wname, mode, ctx);
    }

    /// `r` に、目標の攻撃前の状態 `before` と今の状態から、受けたダメージと残り HP を入れる。
    pub(super) fn strike_report(
        &self,
        before: &UnitInstance,
        max_hp: i64,
        r: StrikeReport,
    ) -> StrikeReport {
        let hp_after = if r.killed {
            0
        } else {
            self.database
                .idx_by_uid(&before.uid)
                .map_or(0, |i| max_hp - self.database.unit_instances[i].damage)
        };
        StrikeReport {
            damage: (max_hp - before.damage - hp_after).max(0),
            hp_after,
            max_hp,
            ..r
        }
    }

    /// `u` の今の状態 (盤面から消えていれば `u` のまま)。
    pub(super) fn unit_snapshot(&self, u: &UnitInstance) -> UnitInstance {
        self.database
            .idx_by_uid(&u.uid)
            .map_or_else(|| u.clone(), |i| self.database.unit_instances[i].clone())
    }

    /// かばう台詞 `situation` があるか (原典 `IsMessageDefined(…, True)`: 状態異常を問わない)。
    pub(super) fn has_guard_line(&mut self, u: &UnitInstance, situation: &str) -> bool {
        self.has_pilot_line(u, situation, &MsgCtx::default())
    }

    /// 援護防御・かばうに入ったユニットの台詞 (原典 `Unit.Attack` の「かばう際の
    /// メッセージ」)。援護防御は「サポートガード(名前 / 愛称)」→「サポートガード」、
    /// かばうは「かばう(名前 / 愛称)」。
    pub(super) fn guard_messages(
        &mut self,
        pick: super::battle::GuardPick,
        guarded: &UnitInstance,
    ) {
        let guard = self.database.unit_instances[pick.idx].clone();
        let ctx = MsgCtx::default();
        let nickname = self
            .database
            .pilot_by_name(&guarded.pilot_name)
            .map(|p| p.nickname.clone());
        let head = if pick.support_guard {
            "サポートガード"
        } else {
            "かばう"
        };
        let candidates: Vec<String> = [Some(guarded.pilot_name.clone()), nickname]
            .into_iter()
            .flatten()
            .map(|n| format!("{head}({n})"))
            .chain(pick.support_guard.then(|| head.to_string()))
            .collect();
        for s in candidates {
            if self.is_message_defined(&guard, &s, &ctx) {
                self.pilot_message(&guard, &s, MsgMode::Plain, &ctx);
                return;
            }
        }
    }

    /// 攻撃の結果の台詞 (原典 `Unit.Attack` の命中・回避・クリティカル・破壊・とどめ・
    /// ダメージのメッセージ)。`atk_ctx` は攻撃側から、`def_ctx` は目標から見た状況。
    pub(super) fn strike_result_messages(
        &mut self,
        atk: &UnitInstance,
        def: &UnitInstance,
        atk_ctx: &MsgCtx,
        def_ctx: &MsgCtx,
        r: &StrikeReport,
    ) {
        let Some(weapon) = atk_ctx.weapon else {
            return;
        };
        let wname = weapon.name.clone();
        // 切り払いは命中したうえでダメージを 0 にする (台詞は回避と同じ扱い)。
        if !r.hit || r.parried {
            let s = if r.parried { "切り払い" } else { "回避" };
            self.pilot_message(def, s, MsgMode::Plain, def_ctx);
            self.pilot_message(atk, &format!("{wname}(回避)"), MsgMode::Plain, atk_ctx);
            return;
        }
        self.pilot_message(atk, &format!("{wname}(命中)"), MsgMode::Plain, atk_ctx);
        if r.critical {
            self.pilot_message(
                atk,
                &format!("{wname}(クリティカル)"),
                MsgMode::Plain,
                atk_ctx,
            );
        }
        if r.killed && !r.covered {
            self.pilot_message(def, "破壊", MsgMode::Plain, def_ctx);
        }
        let sub = if r.killed {
            "とどめ"
        } else {
            "ダメージ"
        };
        self.pilot_message(atk, &format!("{wname}({sub})"), MsgMode::Plain, atk_ctx);
        if r.killed || r.covered {
            return;
        }
        let (dmg, hp, max) = (r.damage, r.hp_after, r.max_hp);
        let s = if (dmg * 20 <= max && hp * 4 >= max) || dmg <= 10 {
            "ダメージ小"
        } else if hp * 4 < max {
            "ダメージ大"
        } else if hp * 5 >= max * 4 {
            "ダメージ小"
        } else {
            "ダメージ中"
        };
        self.pilot_message(def, s, MsgMode::Plain, def_ctx);
    }
}

/// 特殊能力名の末尾の `Lv<n>` を取り除く。
fn strip_level(name: &str) -> &str {
    match name.rfind("Lv") {
        Some(p) if name[p + 2..].chars().all(|c| c.is_ascii_digit()) => &name[..p],
        _ => name,
    }
}
