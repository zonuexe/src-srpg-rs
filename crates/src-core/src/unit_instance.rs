//! マップ上に配置されたユニットの実体 / Units placed on the map.
//!
//! 元 SRC では `Unit.cls`（31K 行）が膨大な機能を担うが、ここでは Map 上の
//! 位置と所属勢力、紐付くデータ参照だけを持つ最小の構造体に絞る。戦闘や
//! 移動ロジックは後続フェーズで段階的に追加する。
//!
//! Originally `Unit.cls` carries massive amount of behaviour. We start with
//! the bare minimum needed to render units on the map.

use serde::{Deserialize, Serialize};

use crate::condition::Condition;
use crate::feature::ActiveFeature;
use crate::item_slot::{ItemSlot, SlotType};
use crate::unit_ability::UnitAbility;
use crate::unit_weapon::UnitWeapon;

/// 所属勢力 / Party affiliation.
///
/// 元 SRC の `Party` プロパティ（"味方"/"敵"/"中立"/"ＮＰＣ"）に対応。
/// SRC に「友軍」陣営は無く、プレイヤー側 AI 陣営は "ＮＰＣ"（`SRC.cs` の
/// `Party=="味方"|Party=="ＮＰＣ"` がプレイヤー側判定）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Party {
    /// 元: "味方"
    Player,
    /// 元: "敵"
    Enemy,
    /// 元: "中立"
    Neutral,
    /// 元: "ＮＰＣ"（プレイヤー側 AI 陣営）。
    Npc,
}

impl Party {
    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Player => "味",
            Self::Npc => "Ｎ",
            Self::Enemy => "敵",
            Self::Neutral => "中",
        }
    }

    /// 陣営の所属キャンプ。SRC の敵味方判定（`Unit.IsEnemy`/`IsAlly`）はキャンプ単位:
    /// {味方, ＮＰＣ}=プレイヤー側 / {敵} / {中立}。異なるキャンプ同士が敵対する。
    const fn camp(self) -> u8 {
        match self {
            // プレイヤー側 (味方+ＮＰＣ は同盟)
            Self::Player | Self::Npc => 0,
            Self::Enemy => 1,
            Self::Neutral => 2,
        }
    }

    /// 相手陣営に敵対しているか（攻撃可能か）。SRC `Unit.IsEnemy` 相当。
    /// 味方+ＮＰＣ は同盟、敵は別キャンプ、中立は単独キャンプ（全陣営に敵対）。
    /// すなわち「異なるキャンプ＝敵対」。
    pub const fn is_hostile_to(self, other: Party) -> bool {
        self.camp() != other.camp()
    }

    /// 同盟（同キャンプ）か。SRC `Unit.IsAlly` 相当。自分自身・味方↔ＮＰＣ を含む。
    pub const fn is_ally_of(self, other: Party) -> bool {
        self.camp() == other.camp()
    }
}

/// マップ上に置かれたユニット / Unit placed on the map.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnitInstance {
    /// `GameDatabase::units` を引くキー（`UnitData.name`）。
    pub unit_data_name: String,
    /// `GameDatabase::pilots` を引くキー（`PilotData.name`）。空文字列なら無人。
    pub pilot_name: String,
    /// 複数パイロットIDs（メンパイロット＋サブパイロット＋援護）。`
    /// `GameDatabase::pilot_instances` を引くキー。
    #[serde(default)]
    pub pilot_ids: Vec<String>,
    /// 所属勢力。
    pub party: Party,
    /// 0-origin のマップ座標。
    pub x: u32,
    pub y: u32,
    /// 累積ダメージ（実値）。`UnitData.hp - damage` が現 HP（負ならユニット撃破）。
    pub damage: i64,
    /// HUD 表示用の補間ダメージ。`damage` に向けてフレームごとに近付ける。
    /// 戦闘演出（HP バー減少アニメーション）に利用。
    pub displayed_damage: f64,
    /// 累積獲得経験値（撃破時に相手 PilotData.exp_value を加算）。
    /// シナリオロード直後は 0、save/load でも保持される。
    #[serde(default)]
    pub total_exp: i32,
    /// 機体改造段階 (インターミッションの「機体改造」で資金を払って上昇)。
    /// `effective_*` 系が HP/EN/装甲/運動性/武器攻撃力にボーナスを加える基準。
    #[serde(default)]
    pub upgrade_level: i32,
    /// 消費 EN。`UnitData.en - en_consumed` が現 EN。
    #[serde(default)]
    pub en_consumed: i32,
    /// 現在の士気 (0..=150 想定)。元 SRC は 100 で開始。
    #[serde(default = "default_morale")]
    pub morale: i32,
    /// 装備中アイテム名 (`ItemData.name` で参照)。重複は許容しない簡略化。
    #[serde(default)]
    pub equipped_items: Vec<String>,
    /// 装備スロット列表。
    #[serde(default)]
    pub item_slots: Vec<ItemSlot>,
    /// ユニット实例の武器状態列表。`UnitData.weapons` に対応する runtime 状态。
    #[serde(default)]
    pub weapons: Vec<UnitWeapon>,
    /// ユニット实例的 Ability 状态列表。`UnitData.features` 对应的 runtime 状态。
    #[serde(default)]
    pub abilities: Vec<UnitAbility>,
    /// ユニット实例的 Feature 状态列表。`UnitData.features` 对应的 runtime 状态。
    #[serde(default)]
    pub active_features: Vec<ActiveFeature>,
    /// 状態異常 / バフ列表（"毒", "麻痺", "気力高揚" 等）。
    /// 元 SRC `Unit.cls::Status` に対応する。生涯管理もこちらで行う。
    #[serde(default)]
    pub conditions: Vec<Condition>,
    /// 当該フェイズで既に行動した (= 移動 / 攻撃 / 待機 を選んだ)。
    /// フェイズ終了時にリセット。元 SRC では `Unit.Used` 相当。
    #[serde(default)]
    pub has_acted: bool,
    /// このフェイズ内で移動を使用したか（再移動防止）。`begin_phase` でリセット。
    #[serde(default)]
    pub has_moved: bool,
    /// 現在の SP (精神コマンド消費ポイント)。`None` の場合は PilotData.sp を
    /// そのまま使用するフォールバック。`Some(0)` だと SP 系コマンドは不発。
    #[serde(default)]
    pub sp_consumed: i32,
    /// 当ターン残り援護攻撃回数。0 だとサポートアタックは発動しない。
    /// `begin_phase` で `default_support_attack` 値にリセット。
    #[serde(default = "default_support_attack")]
    pub support_attack_remaining: i32,
    /// 当ターン残り援護防御回数。0 だとサポートガードは発動しない。
    /// `begin_phase` で 1 にリセット。
    #[serde(default = "default_support_guard")]
    pub support_guard_remaining: i32,
    /// 当ターン使用済みカウンター (先制反撃) 回数。`カウンター` 技能 Lv 回まで先制反撃でき、
    /// 攻撃を受けるたびに加算する。`begin_phase` で 0 にリセット (SRC `UsedCounterAttack`)。
    #[serde(default)]
    pub used_counter_attack: i32,
    /// マップ外退避フラグ。`Escape` で true に、`Launch` / `Place` で false。
    /// true のあいだ AI / 戦闘 / 描画から除外されるが、`unit_instances` には残るため
    /// `Pilot(unit)` / `Info(...)` で参照可能で、後段の `Launch` で再配置できる。
    #[serde(default)]
    pub off_map: bool,
    /// SRC `Status` 関数 (`Statusコマンド`/`ユニット情報関数.md`) の返値
    /// になる「ユニット状態」。
    ///
    /// 既定値は空 (= `出撃` (on map) / `待機` (off_map) を `Status()` 関数側で
    /// `off_map` から自動判定)。明示的にセットされた場合はそちらを優先:
    /// - `格納` ─ 母艦に収納されている (SRC: 収納イベント等)
    /// - `離脱` ─ `Leave` コマンドで戦線から離れている
    /// - `破壊` ─ HP 0 だが unit_instances には残している
    ///   (Destruction 後のメタ参照用、本実装は通常 unit_instances から remove するため未使用)
    ///
    /// 値が `破棄` のときは Status() が unit_instances 不在と同じ扱い。
    #[serde(default)]
    pub life_state: String,
    /// 思考モード文字列 (`ChangeModeコマンド`)。
    /// 既定値は空 (= `通常` 扱い)。`待機` / `固定` / 陣営名 / メインパイロット名
    /// 等が入る。AI ロジックはこの値を読んで行動パターンを変える想定。
    /// 本実装の AI は最小限なので参照箇所は限定的だが、シナリオ側の
    /// `ChangeMode` 命令で値が変わる事実は `Info(ユニット, モード)` 等で
    /// 取得可能になる。
    #[serde(default)]
    pub ai_mode: String,
    /// 現在の活動領域 (`地上` / `空中` / `水中` / `水上` / `宇宙` / `地中`)。
    /// `地上`/`空中`/`水中` 等のコマンドで明示的に設定された場合、地形由来の
    /// 領域 (`Area()` 関数のデフォルト) を上書きする。空文字は「未上書き = 地形に従う」。
    #[serde(default)]
    pub current_area: String,
    /// チャージ攻撃用のフラグ (`Chargeコマンド` 設定)。次の攻撃で消費し、
    /// チャージ属性の武器が解禁される。本実装では未連動だが、シナリオが
    /// `Charge` を実行した事実をフラグとして保持する。
    #[serde(default)]
    pub charged: bool,
    /// 同名ユニット / 同名パイロット (ザコ等) を区別する一意 ID。SRC の
    /// `対象ユニットＩＤ` システム変数や `グループＩＤ` 経由でユニットを
    /// 参照するために使う。値は `Create` / `Place` 等の生成時に
    /// `app.next_unit_id()` で採番。空文字は「未付与」。
    #[serde(default)]
    pub uid: String,
    /// `ChangeUnitBitmap` コマンドで一時上書きされたビットマップ名。
    /// `None` は通常画像、`Some("-")` は元の画像に戻した状態。
    #[serde(default)]
    pub bitmap_override: Option<String>,
    /// `ChangeUnitBitmap … 非表示` で非表示にされているか。
    #[serde(default)]
    pub is_bitmap_hidden: bool,
    /// `ChangeUnitClass` コマンドで上書きされたユニット分類文字列。
    /// `None` は `UnitData.class` をそのまま使う。
    #[serde(default)]
    pub class_override: Option<String>,
    /// `SetStock` コマンドで設定したアビリティ残り使用回数。
    /// キー = アビリティ名称。`UnitInstance.abilities` に頼らず独立して管理。
    #[serde(default)]
    pub ability_stocks: std::collections::BTreeMap<String, i32>,
    /// パイロットの霊力 (プラーナ)。`Plana(pilot) = n` 代入で設定。
    /// SRC.Sharp では `Pilot.Plana` として管理するが、本実装は
    /// パイロット情報がユニット経由のため `UnitInstance` に格納する。
    #[serde(default)]
    pub plana: i32,
    /// このユニットを召喚した親ユニットの uid。`None` は通常ユニット。
    /// `UseAbility … 召喚 …` で設定し、`StopSummoning`/`召喚解除` で
    /// 親 uid に一致する召喚ユニットを除去する。
    #[serde(default)]
    pub summoned_by: Option<String>,
    /// 母艦 (`母艦` 特殊能力) に格納しているユニットの uid 列。格納ユニットは
    /// `off_map=true` / `life_state="格納"` / `stored_in=Some(母艦uid)`。`発進` で
    /// 出撃させ、毎ターン HP/EN/弾/回数を回復する。
    #[serde(default)]
    pub stored_units: Vec<String>,
    /// この機が母艦に格納されている場合の母艦 uid。`発進` / `Launch` で出撃すると
    /// `None` に戻り `stored_units` からも外れる。
    #[serde(default)]
    pub stored_in: Option<String>,
    /// 合体 (`合体` 特殊能力) によりこの機 (合体形態) に取り込まれた構成ユニットの
    /// uid 列。構成ユニットは `off_map=true` / `life_state="合体"` で温存され、`分離`
    /// で盤上へ復帰する。母艦の `stored_units` とは別系統 (発進対象にしない)。
    #[serde(default)]
    pub combined_from: Vec<String>,
    /// 合体前のこの機の形態 (`unit_data_name`)。`分離` で元の形態へ戻すために保持。
    #[serde(default)]
    pub pre_combine_form: Option<String>,
    /// 合体前のこの機 (host) の搭乗パイロット (`pilot_ids`)。合体時に構成ユニットの
    /// パイロットを合体形態へ統合 (全員搭乗) し、`分離` で host を元の搭乗構成へ戻す
    /// ために保持する。構成ユニット側は自身の `pilot_ids` を温存しているため復帰可能。
    #[serde(default)]
    pub pre_combine_pilots: Vec<String>,
    /// ボスランク (0=通常 / 1〜5=ボス強化、`BossRankコマンド`)。HP/装甲/運動性/攻撃力を
    /// ランクに応じて強化し、即死/石化/憑依を無効化、衰/滅 の減少率を半減する。
    #[serde(default)]
    pub boss_rank: i32,
    /// 魅了 (特殊効果攻撃属性 魅) で一時的に勢力を変更したときの**元の勢力**。
    /// `Some` のあいだは「魅了で他陣営に支配されている」状態で、`魅了` condition が
    /// 期限切れ (`begin_phase` の tick で除去) になると `begin_phase` が `party` をここから
    /// 復帰し `None` へ戻す。憑依 (恒久支配) はこのフィールドを使わない (復帰しない)。
    #[serde(default)]
    pub charm_revert_party: Option<Party>,
}

impl UnitInstance {
    /// ボスユニットか (即死/石化/憑依 を無効化、衰/滅 半減の判定)。
    pub fn is_boss(&self) -> bool {
        self.boss_rank > 0
    }
}

fn default_support_attack() -> i32 {
    1
}

fn default_support_guard() -> i32 {
    1
}

fn default_morale() -> i32 {
    100
}

impl UnitInstance {
    pub fn new(
        unit_data_name: impl Into<String>,
        pilot_name: impl Into<String>,
        party: Party,
        x: u32,
        y: u32,
    ) -> Self {
        Self {
            unit_data_name: unit_data_name.into(),
            pilot_name: pilot_name.into(),
            party,
            x,
            y,
            damage: 0,
            displayed_damage: 0.0,
            total_exp: 0,
            upgrade_level: 0,
            en_consumed: 0,
            morale: 100,
            equipped_items: Vec::new(),
            item_slots: Vec::new(),
            conditions: Vec::new(),
            has_acted: false,
            has_moved: false,
            sp_consumed: 0,
            support_attack_remaining: default_support_attack(),
            support_guard_remaining: default_support_guard(),
            used_counter_attack: 0,
            off_map: false,
            life_state: String::new(),
            ai_mode: String::new(),
            current_area: String::new(),
            charged: false,
            uid: String::new(),
            weapons: Vec::new(),
            abilities: Vec::new(),
            active_features: Vec::new(),
            pilot_ids: Vec::new(),
            bitmap_override: None,
            is_bitmap_hidden: false,
            class_override: None,
            ability_stocks: std::collections::BTreeMap::new(),
            plana: 0,
            summoned_by: None,
            stored_units: Vec::new(),
            stored_in: None,
            combined_from: Vec::new(),
            pre_combine_form: None,
            pre_combine_pilots: Vec::new(),
            boss_rank: 0,
            charm_revert_party: None,
        }
    }

    /// Check if this unit has a condition with the given name.
    pub fn has_condition(&self, name: &str) -> bool {
        self.conditions.iter().any(|c| c.matches_name(name))
    }

    /// 攻撃不能（行動不能）か。`AttackDisabled` 効果を持つ状態異常（麻痺/混乱/睡眠/
    /// 行動不能/捕縛 等）が 1 つでもあれば真。SRC `MaxAction()==0` 相当のゲート判定に
    /// 使い、反撃や能動的な反撃モード選択を抑止する。
    pub fn attack_disabled(&self) -> bool {
        self.conditions.iter().any(|c| {
            c.effects()
                .contains(&crate::condition::ConditionEffect::AttackDisabled)
        })
    }

    /// 移動不能か。`MoveDisabled` 効果を持つ状態異常（捕縛/麻痺/移動不能/足止め/
    /// 凍結/石化 等）が 1 つでもあれば真。移動範囲計算で空範囲にするゲートに使う。
    pub fn move_disabled(&self) -> bool {
        self.conditions.iter().any(|c| {
            c.effects()
                .contains(&crate::condition::ConditionEffect::MoveDisabled)
        })
    }

    /// Add a condition. If a condition with the same name already exists,
    /// update its lifetime (take the larger value).
    pub fn add_condition(&mut self, condition: Condition) {
        if let Some(existing) = self
            .conditions
            .iter_mut()
            .find(|c| c.matches_name(&condition.name))
        {
            if condition.is_permanent() || condition.lifetime > existing.lifetime {
                existing.lifetime = condition.lifetime;
            }
            if condition.level > existing.level {
                existing.level = condition.level;
            }
        } else {
            self.conditions.push(condition);
        }
    }

    /// Remove all conditions matching the given name. Returns count removed.
    pub fn remove_condition(&mut self, name: &str) -> usize {
        let before = self.conditions.len();
        self.conditions.retain(|c| !c.matches_name(name));
        before - self.conditions.len()
    }

    /// Tick all conditions (decrement lifetime). Removes expired ones.
    pub fn tick_conditions(&mut self) {
        self.conditions
            .retain_mut(|c| !c.tick() || c.is_permanent());
    }

    /// Clear conditions with lifetime == 1 (one-turn SP effects).
    /// Called at the end of each phase.
    pub fn clear_one_turn_conditions(&mut self) {
        self.conditions.retain(|c| c.lifetime != 1);
    }

    /// Get all equipped item names.
    pub fn equipped_item_names(&self) -> Vec<&str> {
        self.item_slots
            .iter()
            .filter_map(|s| s.equipped_item.as_deref())
            .collect()
    }

    /// Check if a specific item is equipped.
    pub fn has_item_equipped(&self, item_name: &str) -> bool {
        self.item_slots
            .iter()
            .any(|s| s.equipped_item.as_deref() == Some(item_name))
    }

    /// Equip an item in the first available slot of the given type.
    /// Creates a new slot if none exists of that type.
    pub fn equip_item(&mut self, slot_type: SlotType, item_name: impl Into<String>) -> bool {
        let item_name = item_name.into();
        for slot in &mut self.item_slots {
            if slot.slot_type == slot_type && slot.is_empty() {
                return slot.equip(item_name);
            }
        }
        // No empty slot of this type - add a new one
        self.item_slots
            .push(ItemSlot::with_item(slot_type, item_name));
        true
    }

    /// Unequip an item by name. Returns false if item not found or slot is fixed.
    pub fn unequip_item(&mut self, item_name: &str) -> bool {
        for slot in &mut self.item_slots {
            if slot.equipped_item.as_deref() == Some(item_name) {
                return slot.unequip();
            }
        }
        false
    }

    pub fn main_pilot_name(&self) -> &str {
        self.pilot_ids
            .first()
            .map(|s| s.as_str())
            .unwrap_or(&self.pilot_name)
    }

    pub fn all_pilot_names(&self) -> Vec<&str> {
        self.pilot_ids.iter().map(|s| s.as_str()).collect()
    }

    pub fn pilot_count(&self) -> usize {
        self.pilot_ids.len()
    }

    pub fn add_pilot_id(&mut self, id: impl Into<String>) {
        self.pilot_ids.push(id.into());
    }

    pub fn remove_pilot_at(&mut self, index: usize) -> Option<String> {
        if index < self.pilot_ids.len() {
            Some(self.pilot_ids.remove(index))
        } else {
            None
        }
    }

    /// Check if morale is sufficient for a special power.
    /// Some SP powers require minimum morale (e.g., "魂" requires 120).
    pub fn morale_sufficient_for_power(&self, power_name: &str) -> bool {
        let required = match power_name {
            "熱血" => 80,
            "魂" => 120,
            "ひらめき" => 100,
            "不屈" => 100,
            "鉄壁" => 100,
            "集中" => 80,
            "気合" => 100,
            // SRC 標準以外 (シナリオ独自) の精神コマンドの必要気力はここに持たない
            // (例: 東方夢想伝の 気迫 は原典に無い)。必要気力は sp.txt 側で解決すべき。
            _ => 0,
        };
        self.morale >= required
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn party_hostility_matrix_matches_src() {
        use Party::{Enemy, Neutral, Npc, Player};
        // SRC `Unit.IsEnemy`/`IsAlly` の正準: キャンプ {味方,ＮＰＣ}/{敵}/{中立}。
        // 味方 ↔ ＮＰＣ は同盟（攻撃不可）。
        assert!(!Player.is_hostile_to(Npc));
        assert!(!Npc.is_hostile_to(Player));
        assert!(Player.is_ally_of(Npc));
        assert!(Player.is_ally_of(Player));
        // 敵は 味方/ＮＰＣ/中立 に敵対。
        for t in [Player, Npc, Neutral] {
            assert!(Enemy.is_hostile_to(t), "敵 → {t:?} は敵対のはず");
            assert!(t.is_hostile_to(Enemy), "{t:?} → 敵 は敵対のはず");
        }
        assert!(!Enemy.is_hostile_to(Enemy));
        // 中立は全陣営に敵対（単独キャンプ）。
        for t in [Player, Npc, Enemy] {
            assert!(Neutral.is_hostile_to(t), "中立 → {t:?} は敵対のはず");
            assert!(t.is_hostile_to(Neutral), "{t:?} → 中立 は敵対のはず");
        }
        assert!(!Neutral.is_hostile_to(Neutral));
        // is_ally_of は is_hostile_to の補集合。
        for a in [Player, Npc, Enemy, Neutral] {
            for b in [Player, Npc, Enemy, Neutral] {
                assert_eq!(a.is_ally_of(b), !a.is_hostile_to(b));
            }
        }
    }

    #[test]
    fn unit_pilot_count_defaults_zero() {
        let unit = UnitInstance::new("TestUnit", "Pilot1", Party::Player, 0, 0);
        assert_eq!(unit.pilot_count(), 0);
    }

    #[test]
    fn unit_add_pilot_id_increases_count() {
        let mut unit = UnitInstance::new("TestUnit", "Pilot1", Party::Player, 0, 0);
        unit.add_pilot_id("pilot_1");
        assert_eq!(unit.pilot_count(), 1);
    }

    #[test]
    fn unit_main_pilot_name_returns_first() {
        let mut unit = UnitInstance::new("TestUnit", "Pilot1", Party::Player, 0, 0);
        unit.add_pilot_id("pilot_main");
        unit.add_pilot_id("pilot_sub");
        assert_eq!(unit.main_pilot_name(), "pilot_main");
    }

    #[test]
    fn unit_all_pilot_names_returns_all() {
        let mut unit = UnitInstance::new("TestUnit", "Pilot1", Party::Player, 0, 0);
        unit.add_pilot_id("pilot_main");
        unit.add_pilot_id("pilot_sub");
        unit.add_pilot_id("pilot_support");
        assert_eq!(
            unit.all_pilot_names(),
            vec!["pilot_main", "pilot_sub", "pilot_support"]
        );
    }

    #[test]
    fn unit_remove_pilot_at_works() {
        let mut unit = UnitInstance::new("TestUnit", "Pilot1", Party::Player, 0, 0);
        unit.add_pilot_id("pilot_main");
        unit.add_pilot_id("pilot_sub");
        assert_eq!(unit.pilot_count(), 2);
        let removed = unit.remove_pilot_at(0);
        assert_eq!(removed, Some("pilot_main".to_string()));
        assert_eq!(unit.pilot_count(), 1);
    }

    #[test]
    fn clear_one_turn_conditions_removes_lifetime_1() {
        let mut unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        unit.add_condition(Condition::new("熱血", 1));
        assert!(unit.has_condition("熱血"));
        unit.clear_one_turn_conditions();
        assert!(!unit.has_condition("熱血"));
    }

    #[test]
    fn clear_one_turn_conditions_keeps_permanent() {
        let mut unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        unit.add_condition(Condition::new("鉄壁", -1));
        assert!(unit.has_condition("鉄壁"));
        unit.clear_one_turn_conditions();
        assert!(unit.has_condition("鉄壁"));
    }

    #[test]
    fn clear_one_turn_conditions_keeps_multi_turn() {
        let mut unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        unit.add_condition(Condition::new("毒", 3));
        assert!(unit.has_condition("毒"));
        unit.clear_one_turn_conditions();
        assert!(unit.has_condition("毒"));
    }

    #[test]
    fn morale_sufficient_for_power_checks_requirement() {
        let mut unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        unit.morale = 100;
        assert!(!unit.morale_sufficient_for_power("魂"));
    }

    #[test]
    fn morale_sufficient_for_power_no_requirement() {
        let mut unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        unit.morale = 0;
        assert!(unit.morale_sufficient_for_power("_unknown_power"));
    }
}
