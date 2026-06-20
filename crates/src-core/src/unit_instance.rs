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
    /// 描画用の塗り色（CSS カラー）。VB6 SRC は HUD の色付けで似た区別をしていた。
    pub const fn color(self) -> &'static str {
        match self {
            Self::Player => "#1e88e5",
            // ＮＰＣ はプレイヤー側陣営。緑系で味方と区別。
            Self::Npc => "#43a047",
            Self::Enemy => "#e53935",
            Self::Neutral => "#fdd835",
        }
    }

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

/// Result of an attack execution.
#[derive(Debug, Clone)]
pub struct AttackResult {
    /// Whether the attack hit.
    pub hit: bool,
    /// Damage dealt (0 if missed).
    pub damage: i64,
    /// Whether this was a critical hit.
    pub is_critical: bool,
    /// Whether the target was destroyed (damage >= max_hp).
    pub target_destroyed: bool,
    /// Experience gained from this attack (if target destroyed).
    pub exp_gained: i32,
    /// EN consumed by this attack.
    pub en_consumed: i32,
}

impl AttackResult {
    fn miss() -> Self {
        Self {
            hit: false,
            damage: 0,
            is_critical: false,
            target_destroyed: false,
            exp_gained: 0,
            en_consumed: 0,
        }
    }
}

impl UnitInstance {
    /// Execute a full attack against a defender.
    ///
    /// This performs:
    /// 1. Weapon availability check
    /// 2. Hit roll (using RNG seed for determinism in tests)
    /// 3. Damage calculation (using combat.rs predict_with_status)
    /// 4. Critical roll
    /// 5. Apply damage to defender
    /// 6. Consume EN/bullets on attacker
    /// 7. Set attacker.has_acted = true
    ///
    /// `rng_seed` is used for deterministic hit/critical rolls.
    /// In production, pass a real random value. In tests, pass a fixed value.
    pub fn execute_attack(
        &mut self,
        defender: &mut UnitInstance,
        weapon_idx: usize,
        db: &crate::db::GameDatabase,
        rng_seed: u64,
    ) -> AttackResult {
        // Step 1: Check weapon availability
        if !self.is_weapon_available(weapon_idx, db) {
            return AttackResult::miss();
        }

        // Get static data
        let Some(unit_data) = db.unit_by_name(&self.unit_data_name) else {
            return AttackResult::miss();
        };
        let unit_weapon = &self.weapons[weapon_idx];
        let Some(weapon_data) = unit_data.weapons.get(unit_weapon.weapon_index) else {
            return AttackResult::miss();
        };

        // Get defender data
        let Some(def_unit_data) = db.unit_by_name(&defender.unit_data_name) else {
            return AttackResult::miss();
        };

        // パイロットのレベルアップ済みスタットを反映した PilotData を取得。
        // PilotInstance があればその infight/shooting 等を優先し、なければ
        // 静的 PilotData をそのまま使う (effective_pilot_data のフォールバック)。
        let default_pilot = crate::data::pilot::PilotData {
            spirit_commands: Vec::new(),
            name: String::new(),
            nickname: String::new(),
            kana_name: String::new(),
            sex: crate::data::pilot::Sex::Unspecified,
            class: String::new(),
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            exp_value: 0,
            infight: 100,
            shooting: 100,
            hit: 0,
            dodge: 0,
            intuition: 0,
            technique: 0,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: None,
            features: Vec::new(),
        };
        let atk_pilot_owned = db
            .effective_pilot_data(&self.pilot_name)
            .or_else(|| {
                self.pilot_ids
                    .first()
                    .and_then(|id| db.effective_pilot_data(id))
            })
            .unwrap_or_else(|| default_pilot.clone());
        let def_pilot_owned = db
            .effective_pilot_data(&defender.pilot_name)
            .or_else(|| {
                defender
                    .pilot_ids
                    .first()
                    .and_then(|id| db.effective_pilot_data(id))
            })
            .unwrap_or(default_pilot);
        let atk_pilot = &atk_pilot_owned;
        let def_pilot = &def_pilot_owned;

        // Get defender terrain mods (default to 0)
        let def_terrain_hit_mod = 0;
        let def_terrain_damage_mod = 0;

        // Build status lists from conditions (all conditions are "active" if present)
        let atk_status_names: Vec<String> =
            self.conditions.iter().map(|c| c.name.clone()).collect();
        let def_status_names: Vec<String> =
            defender.conditions.iter().map(|c| c.name.clone()).collect();

        // Step 2-3: Calculate hit and damage using combat prediction
        let preview = crate::combat::predict_with_status(
            atk_pilot,
            unit_data,
            weapon_data,
            def_pilot,
            def_unit_data,
            def_terrain_hit_mod,
            def_terrain_damage_mod,
            self.morale,
            defender.morale,
            &atk_status_names,
            &def_status_names,
        );

        // Step 2: Hit roll
        let hit_roll = (rng_seed % 100) as i32;
        let hit = hit_roll < preview.hit_chance;

        // Step 4: Critical roll (base 5% + weapon.critical)
        let is_critical = hit && (((rng_seed / 100) % 100) as i32) < (5 + weapon_data.critical);
        let damage = if is_critical {
            preview.damage * 2
        } else {
            preview.damage
        };

        // Step 5: Apply damage to defender
        let actual_damage = if hit { damage } else { 0 };
        defender.damage += actual_damage;

        // Check if defender is destroyed
        let max_hp = db.effective_max_hp(defender);
        let target_destroyed = defender.damage >= max_hp;

        // Step 6: Consume resources
        let en_consumed = weapon_data.en_consumption;
        self.en_consumed += en_consumed;
        if weapon_data.bullet > 0 {
            // Consume bullet from the UnitWeapon
            if let Some(uw) = self.weapons.get_mut(weapon_idx) {
                uw.consume_bullet();
            }
        }

        // Step 7: Mark as acted
        self.has_acted = true;

        // Calculate exp gained
        let exp_gained = if target_destroyed {
            def_unit_data.exp_value
        } else {
            0
        };

        // Add exp to attacker's total
        self.total_exp += exp_gained;

        AttackResult {
            hit,
            damage: actual_damage,
            is_critical,
            target_destroyed,
            exp_gained,
            en_consumed,
        }
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
    /// 条件による命中ボーナス（格闘L3 → +30 等）。
    #[serde(default)]
    pub bonus_hit: i32,
    /// 条件による回避ボーナス。
    #[serde(default)]
    pub bonus_dodge: i32,
    /// 条件による装甲ボーナス。
    #[serde(default)]
    pub bonus_armor: i64,
    /// Unit can enter water terrain (from pilot skill "水上移動").
    #[serde(default)]
    pub can_enter_water: bool,
    /// Unit can enter air terrain (from pilot skill "空中移動").
    #[serde(default)]
    pub can_enter_air: bool,
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
            bonus_hit: 0,
            bonus_dodge: 0,
            bonus_armor: 0,
            can_enter_water: false,
            can_enter_air: false,
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

    /// Recalculate all effective stats from base data + pilot + items + conditions + features.
    /// Call this after: Place, Ride, Item, RemoveItem, LevelUp, SetSkill, Transform, Combine, Split.
    pub fn update(&mut self, db: &crate::db::GameDatabase) {
        // Get base unit data
        let Some(unit_data) = db.unit_by_name(&self.unit_data_name) else {
            return;
        };

        // Start with base stats
        let bonus_hp: i64 = 0;
        let bonus_en: i32 = 0;
        let mut bonus_armor: i64 = 0;
        let bonus_mobility: i32 = 0;
        let bonus_speed: i32 = 0;
        let mut bonus_infight: i32 = 0;
        let mut bonus_shooting: i32 = 0;
        let mut bonus_hit: i32 = 0;
        let mut bonus_dodge: i32 = 0;

        // 1. Apply item bonuses (already handled by db.effective_* methods)
        // Items are tracked via item_slots, and db.effective_* already sums them

        // 2. Apply pilot stat bonuses
        for pilot_id in &self.pilot_ids {
            if let Some(pilot_inst) = db.pilot_instance_by_id(pilot_id) {
                // PilotInstance already has infight, shooting, etc. set from PilotData
                // Apply skill-based bonuses
                // e.g., "格闘L3" → +30 infight, "射撃L2" → +20 shooting
                bonus_infight += pilot_inst.skill_level("格闘") * 10;
                bonus_shooting += pilot_inst.skill_level("射撃") * 10;
                bonus_hit += pilot_inst.skill_level("命中") * 10;
                bonus_dodge += pilot_inst.skill_level("回避") * 10;
            }
            // Also check static pilot data for features
            if let Some(pilot_data) = db.pilot_by_name(pilot_id) {
                for (feat_name, _feat_value) in &pilot_data.features {
                    if feat_name.contains("格闘UP") || feat_name.contains("格闘強化") {
                        bonus_infight += 20;
                    }
                    if feat_name.contains("射撃UP") || feat_name.contains("射撃強化") {
                        bonus_shooting += 20;
                    }
                }
            }
        }

        // 3. Apply condition effects
        for cond in &self.conditions {
            for eff in cond.effects() {
                match eff {
                    crate::condition::ConditionEffect::HitDown { amount } => {
                        bonus_hit -= amount;
                    }
                    crate::condition::ConditionEffect::DodgeDown { amount } => {
                        bonus_dodge -= amount;
                    }
                    crate::condition::ConditionEffect::ArmorDown { amount } => {
                        bonus_armor -= amount as i64;
                    }
                    _ => {}
                }
            }
        }

        // 4. Apply feature bonuses from unit data
        for feat in &self.active_features {
            if !feat.is_active {
                continue;
            }
            if feat.name.contains("格闘強化") || feat.name.contains("格闘UP") {
                bonus_infight += 20;
            }
            if feat.name.contains("射撃強化") || feat.name.contains("射撃UP") {
                bonus_shooting += 20;
            }
            if feat.name.contains("装甲強化") || feat.name.contains("装甲UP") {
                bonus_armor += 200;
            }
        }

        self.bonus_hit = bonus_hit;
        self.bonus_dodge = bonus_dodge;
        self.bonus_armor = bonus_armor;

        // Apply pilot movement skill modifiers
        let mut water = false;
        let mut air = false;
        for pilot_id in &self.pilot_ids {
            if let Some(pilot_inst) = db.pilot_instance_by_id(pilot_id) {
                if pilot_inst.has_skill("水上移動") {
                    water = true;
                }
                if pilot_inst.has_skill("空中移動") {
                    air = true;
                }
            }
        }
        self.can_enter_water = water;
        self.can_enter_air = air;

        let _ = (
            bonus_hp,
            bonus_en,
            bonus_mobility,
            bonus_speed,
            bonus_infight,
            bonus_shooting,
        );
        let _ = unit_data;
    }

    /// Check if a weapon at the given index is available for use.
    /// Checks: has_acted, EN, bullets, morale, conditions, weapon disabled flag.
    pub fn is_weapon_available(&self, weapon_idx: usize, db: &crate::db::GameDatabase) -> bool {
        // Must not have already acted this phase
        if self.has_acted {
            return false;
        }

        // Weapon must exist in runtime weapons list
        let Some(unit_weapon) = self.weapons.get(weapon_idx) else {
            return false;
        };

        // Weapon must not be disabled
        if unit_weapon.is_disabled {
            return false;
        }

        // Must have ammo
        if !unit_weapon.has_ammo() {
            return false;
        }

        // Get static weapon data
        let Some(unit_data) = db.unit_by_name(&self.unit_data_name) else {
            return false;
        };
        let Some(weapon_data) = unit_data.weapons.get(unit_weapon.weapon_index) else {
            return false;
        };

        // 沈黙 (特殊効果攻撃属性 黙): 術 / 音 属性の武器は使用不能。
        if self.has_condition("沈黙")
            && (weapon_data.class.contains('術') || weapon_data.class.contains('音'))
        {
            return false;
        }
        // 剋<属性> (特殊効果攻撃属性 剋): 指定属性を持つ武器は使用不能。
        for cond in &self.conditions {
            if let Some(el) = cond.name.strip_prefix("剋:") {
                if weapon_data.class.split_whitespace().any(|t| t == el) {
                    return false;
                }
            }
        }

        // Must have enough EN
        let max_en = db.effective_max_en(self);
        let current_en = max_en - self.en_consumed;
        if current_en < weapon_data.en_consumption {
            return false;
        }

        // Must meet morale requirement
        if weapon_data.necessary_morale > 0 && self.morale < weapon_data.necessary_morale {
            return false;
        }

        // Check if conditions prevent attack (麻痺/混乱/睡眠/行動不能/捕縛/凍結/石化 等、
        // ConditionEffect::AttackDisabled を持つ状態異常を一元的に判定)。
        if self.attack_disabled() {
            return false;
        }

        // 必要技能 / 必要条件 ((念力Lv3) 形式の括弧条件)。満たさない武器は使用不可。
        let ns = weapon_data.necessary_skill();
        if !ns.is_empty() && !crate::necessary_skill::is_satisfied(ns, self, db) {
            return false;
        }
        let nc = weapon_data.necessary_condition();
        if !nc.is_empty() && !crate::necessary_skill::is_satisfied(nc, self, db) {
            return false;
        }

        true
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

    /// Get the (min, max) range of a weapon, considering any feature modifications.
    pub fn weapon_range(
        &self,
        weapon_idx: usize,
        db: &crate::db::GameDatabase,
    ) -> Option<(i32, i32)> {
        let unit_weapon = self.weapons.get(weapon_idx)?;
        let unit_data = db.unit_by_name(&self.unit_data_name)?;
        let weapon_data = unit_data.weapons.get(unit_weapon.weapon_index)?;

        let min_range = weapon_data.min_range;
        let mut max_range = weapon_data.max_range;

        // Apply feature modifications (e.g., "射程+1" features)
        for feat in &self.active_features {
            if !feat.is_active {
                continue;
            }
            if feat.name.contains("射程") {
                // Parse "+1" or "-1" from value
                if let Some(val) = feat.value.strip_prefix('+') {
                    if let Ok(n) = val.parse::<i32>() {
                        max_range += n;
                    }
                } else if let Some(val) = feat.value.strip_prefix('-') {
                    if let Ok(n) = val.parse::<i32>() {
                        max_range -= n;
                    }
                }
            }
        }

        Some((min_range, max_range))
    }

    /// Get the best available weapon index for attacking.
    /// Returns the weapon with highest power among available weapons.
    pub fn best_available_weapon(&self, db: &crate::db::GameDatabase) -> Option<usize> {
        let unit_data = db.unit_by_name(&self.unit_data_name)?;
        let mut best_idx: Option<usize> = None;
        let mut best_power: i64 = -1;

        for (idx, _) in self.weapons.iter().enumerate() {
            if self.is_weapon_available(idx, db) {
                let unit_weapon = &self.weapons[idx];
                if let Some(weapon_data) = unit_data.weapons.get(unit_weapon.weapon_index) {
                    if weapon_data.power > best_power {
                        best_power = weapon_data.power;
                        best_idx = Some(idx);
                    }
                }
            }
        }

        best_idx
    }

    /// Counter-attack is possible if: unit has not acted, is alive, no preventing conditions, and has a weapon in range of the attacker.
    pub fn can_counter_attack(&self, distance: u32, db: &crate::db::GameDatabase) -> bool {
        if self.has_acted {
            return false;
        }
        let max_hp = db.effective_max_hp(self);
        if self.damage >= max_hp {
            return false;
        }
        if self.has_condition("麻痺")
            || self.has_condition("混乱")
            || self.has_condition("睡眠")
            || self.has_condition("行動不能")
        {
            return false;
        }
        self.find_weapon_in_range(distance, db).is_some()
    }

    /// Find the best weapon index available for counter-attack at the given distance.
    /// Unlike `is_weapon_available`, this does NOT check `has_acted` (counter-attack is reactive).
    fn find_weapon_in_range(&self, distance: u32, db: &crate::db::GameDatabase) -> Option<usize> {
        let unit_data = db.unit_by_name(&self.unit_data_name)?;
        let mut best_idx: Option<usize> = None;
        let mut best_power: i64 = -1;

        for (idx, unit_weapon) in self.weapons.iter().enumerate() {
            if let Some(weapon_data) = unit_data.weapons.get(unit_weapon.weapon_index) {
                let d = distance as i32;
                if d >= weapon_data.min_range
                    && d <= weapon_data.max_range
                    && unit_weapon.has_ammo()
                    && !unit_weapon.is_disabled
                {
                    let max_en = db.effective_max_en(self);
                    let current_en = max_en - self.en_consumed;
                    if current_en >= weapon_data.en_consumption && weapon_data.power > best_power {
                        best_power = weapon_data.power;
                        best_idx = Some(idx);
                    }
                }
            }
        }
        best_idx
    }

    /// Execute a counter-attack against a target (the original attacker).
    /// Caller must apply damage and check destruction. Does not consume defender resources.
    pub fn perform_counter_attack(
        &self,
        target: &UnitInstance,
        weapon_idx: usize,
        db: &crate::db::GameDatabase,
        rng_seed: u64,
    ) -> AttackResult {
        // Get static data
        let Some(unit_data) = db.unit_by_name(&self.unit_data_name) else {
            return AttackResult::miss();
        };
        let unit_weapon = &self.weapons[weapon_idx];
        let Some(weapon_data) = unit_data.weapons.get(unit_weapon.weapon_index) else {
            return AttackResult::miss();
        };
        let Some(target_unit_data) = db.unit_by_name(&target.unit_data_name) else {
            return AttackResult::miss();
        };

        // レベルアップ済みスタットを反映したパイロットデータを取得。
        let default_pilot = crate::data::pilot::PilotData {
            spirit_commands: Vec::new(),
            name: String::new(),
            nickname: String::new(),
            kana_name: String::new(),
            sex: crate::data::pilot::Sex::Unspecified,
            class: String::new(),
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            exp_value: 0,
            infight: 100,
            shooting: 100,
            hit: 0,
            dodge: 0,
            intuition: 0,
            technique: 0,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: None,
            features: Vec::new(),
        };
        let counter_pilot_owned = db
            .effective_pilot_data(&self.pilot_name)
            .or_else(|| {
                self.pilot_ids
                    .first()
                    .and_then(|id| db.effective_pilot_data(id))
            })
            .unwrap_or_else(|| default_pilot.clone());
        let target_pilot_owned = db
            .effective_pilot_data(&target.pilot_name)
            .or_else(|| {
                target
                    .pilot_ids
                    .first()
                    .and_then(|id| db.effective_pilot_data(id))
            })
            .unwrap_or(default_pilot);
        let counter_pilot = &counter_pilot_owned;
        let target_pilot = &target_pilot_owned;

        // Build status lists
        let counter_status_names: Vec<String> =
            self.conditions.iter().map(|c| c.name.clone()).collect();
        let target_status_names: Vec<String> =
            target.conditions.iter().map(|c| c.name.clone()).collect();

        // Calculate hit and damage
        let preview = crate::combat::predict_with_status(
            counter_pilot,
            unit_data,
            weapon_data,
            target_pilot,
            target_unit_data,
            0,
            0,
            self.morale,
            target.morale,
            &counter_status_names,
            &target_status_names,
        );

        let hit_roll = (rng_seed % 100) as i32;
        let hit = hit_roll < preview.hit_chance;

        let is_critical = hit && (((rng_seed / 100) % 100) as i32) < (5 + weapon_data.critical);
        let damage = if is_critical {
            preview.damage * 2
        } else {
            preview.damage
        };
        let actual_damage = if hit { damage } else { 0 };
        let en_consumed = weapon_data.en_consumption;

        AttackResult {
            hit,
            damage: actual_damage,
            is_critical,
            target_destroyed: false,
            exp_gained: 0,
            en_consumed,
        }
    }

    /// Execute a full attack with automatic counter-attack.
    ///
    /// Returns `(attack_result, counter_attack_result)`.
    /// `counter_attack_result` is `Some(AttackResult)` if the defender counter-attacked,
    /// `None` otherwise.
    ///
    /// The counter-attack is triggered when:
    /// - The attack hits and deals damage
    /// - The defender survives (damage < max_hp)
    /// - The defender can counter-attack (has not acted, has weapon in range, no preventing conditions)
    pub fn execute_attack_with_counter(
        &mut self,
        defender: &mut UnitInstance,
        weapon_idx: usize,
        db: &crate::db::GameDatabase,
        attack_rng: u64,
        counter_rng: u64,
    ) -> (AttackResult, Option<AttackResult>) {
        let attack_result = self.execute_attack(defender, weapon_idx, db, attack_rng);

        if !attack_result.hit || attack_result.damage == 0 {
            return (attack_result, None);
        }

        let def_max_hp = db.effective_max_hp(defender);
        if defender.damage >= def_max_hp {
            return (attack_result, None);
        }

        let distance = crate::combat::manhattan((self.x, self.y), (defender.x, defender.y));

        if !defender.can_counter_attack(distance, db) {
            return (attack_result, None);
        }

        let Some(counter_weapon_idx) = defender.find_weapon_in_range(distance, db) else {
            return (attack_result, None);
        };

        let counter_result =
            defender.perform_counter_attack(self, counter_weapon_idx, db, counter_rng);

        self.damage += counter_result.damage;

        let def_unit_data = db.unit_by_name(&defender.unit_data_name);
        if let Some(ud) = def_unit_data {
            if let Some(wd) = ud
                .weapons
                .get(defender.weapons[counter_weapon_idx].weapon_index)
            {
                defender.en_consumed += wd.en_consumption;
                if wd.bullet > 0 {
                    if let Some(uw_mut) = defender.weapons.get_mut(counter_weapon_idx) {
                        uw_mut.consume_bullet();
                    }
                }
            }
        }

        let atk_max_hp = db.effective_max_hp(self);
        let counter_destroyed = self.damage >= atk_max_hp;

        (
            attack_result,
            Some(AttackResult {
                target_destroyed: counter_destroyed,
                ..counter_result
            }),
        )
    }

    /// Check if this unit can perform a support attack for an ally being attacked.
    /// Requirements:
    /// - Adjacent to the ally (manhattan distance = 1)
    /// - Has "援護" skill (check via pilot_instance or pilot_data features)
    /// - Has not used support attack this turn (support_attack_remaining > 0)
    /// - Has weapon in range of the attacker
    /// - Not prevented by conditions
    pub fn can_support_attack(
        &self,
        ally: &UnitInstance,
        attacker: &UnitInstance,
        db: &crate::db::GameDatabase,
    ) -> bool {
        // Must have support attacks remaining
        if self.support_attack_remaining <= 0 {
            return false;
        }
        // Must be adjacent to ally
        let dist_to_ally = crate::combat::manhattan((self.x, self.y), (ally.x, ally.y));
        if dist_to_ally > 1 {
            return false;
        }
        // Must not have acted
        if self.has_acted {
            return false;
        }
        // Must have "援護" skill
        if !self.has_support_skill(db) {
            return false;
        }
        // Must not be prevented by conditions
        if self.has_condition("麻痺")
            || self.has_condition("混乱")
            || self.has_condition("睡眠")
            || self.has_condition("行動不能")
        {
            return false;
        }
        // Must have weapon in range of attacker
        let dist_to_attacker = crate::combat::manhattan((self.x, self.y), (attacker.x, attacker.y));
        self.find_weapon_in_range(dist_to_attacker, db).is_some()
    }

    /// Check if this unit can perform a support guard for an ally.
    /// Requirements: same as support attack but doesn't need weapon in range.
    pub fn can_support_guard(&self, ally: &UnitInstance, db: &crate::db::GameDatabase) -> bool {
        if self.support_guard_remaining <= 0 {
            return false;
        }
        let dist = crate::combat::manhattan((self.x, self.y), (ally.x, ally.y));
        if dist > 1 {
            return false;
        }
        if self.has_acted {
            return false;
        }
        if !self.has_support_skill(db) {
            return false;
        }
        if self.has_condition("麻痺")
            || self.has_condition("混乱")
            || self.has_condition("睡眠")
            || self.has_condition("行動不能")
        {
            return false;
        }
        true
    }

    /// Check if this unit has the "援護" (support) skill.
    fn has_support_skill(&self, db: &crate::db::GameDatabase) -> bool {
        // Check pilot instance skills
        for pilot_id in &self.pilot_ids {
            if let Some(pilot_inst) = db.pilot_instance_by_id(pilot_id) {
                if pilot_inst.has_skill("援護") {
                    return true;
                }
            }
            // Also check pilot data features
            if let Some(pilot_data) = db.pilot_by_name(pilot_id) {
                for (feat_name, _) in &pilot_data.features {
                    if feat_name.contains("援護") {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Execute a support attack against the attacker.
    /// Returns the attack result. Decrements support_attack_remaining.
    pub fn execute_support_attack(
        &mut self,
        attacker: &mut UnitInstance,
        db: &crate::db::GameDatabase,
        rng_seed: u64,
    ) -> AttackResult {
        let dist = crate::combat::manhattan((self.x, self.y), (attacker.x, attacker.y));
        let Some(weapon_idx) = self.find_weapon_in_range(dist, db) else {
            return AttackResult::miss();
        };

        // Execute attack (similar to execute_attack but for support)
        let Some(unit_data) = db.unit_by_name(&self.unit_data_name) else {
            return AttackResult::miss();
        };
        let unit_weapon = &self.weapons[weapon_idx];
        let Some(weapon_data) = unit_data.weapons.get(unit_weapon.weapon_index) else {
            return AttackResult::miss();
        };
        let Some(attacker_unit_data) = db.unit_by_name(&attacker.unit_data_name) else {
            return AttackResult::miss();
        };

        let default_pilot = crate::data::pilot::PilotData {
            spirit_commands: Vec::new(),
            name: String::new(),
            nickname: String::new(),
            kana_name: String::new(),
            sex: crate::data::pilot::Sex::Unspecified,
            class: String::new(),
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            exp_value: 0,
            infight: 100,
            shooting: 100,
            hit: 0,
            dodge: 0,
            intuition: 0,
            technique: 0,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: None,
            features: Vec::new(),
        };
        let atk_pilot_owned = db
            .effective_pilot_data(&self.pilot_name)
            .or_else(|| {
                self.pilot_ids
                    .first()
                    .and_then(|id| db.effective_pilot_data(id))
            })
            .unwrap_or_else(|| default_pilot.clone());
        let def_pilot_owned = db
            .effective_pilot_data(&attacker.pilot_name)
            .or_else(|| {
                attacker
                    .pilot_ids
                    .first()
                    .and_then(|id| db.effective_pilot_data(id))
            })
            .unwrap_or(default_pilot);
        let atk_pilot = &atk_pilot_owned;
        let def_pilot = &def_pilot_owned;

        let atk_statuses: Vec<String> = self.conditions.iter().map(|c| c.name.clone()).collect();
        let def_statuses: Vec<String> =
            attacker.conditions.iter().map(|c| c.name.clone()).collect();

        let preview = crate::combat::predict_with_status(
            atk_pilot,
            unit_data,
            weapon_data,
            def_pilot,
            attacker_unit_data,
            0,
            0,
            self.morale,
            attacker.morale,
            &atk_statuses,
            &def_statuses,
        );

        let hit_roll = (rng_seed % 100) as i32;
        let hit = hit_roll < preview.hit_chance;
        let is_critical = hit && (((rng_seed / 100) % 100) as i32) < (5 + weapon_data.critical);
        let damage = if is_critical {
            preview.damage * 2
        } else {
            preview.damage
        };
        let actual_damage = if hit { damage } else { 0 };

        // Apply damage
        attacker.damage += actual_damage;

        // Consume resources
        self.en_consumed += weapon_data.en_consumption;
        if weapon_data.bullet > 0 {
            if let Some(uw) = self.weapons.get_mut(weapon_idx) {
                uw.consume_bullet();
            }
        }

        // Decrement support attack remaining
        self.support_attack_remaining -= 1;

        // Support attack does NOT set has_acted (separate from main action)
        // Support attack does NOT grant exp

        let atk_max_hp = db.effective_max_hp(attacker);
        let target_destroyed = attacker.damage >= atk_max_hp;

        AttackResult {
            hit,
            damage: actual_damage,
            is_critical,
            target_destroyed,
            exp_gained: 0,
            en_consumed: weapon_data.en_consumption,
        }
    }

    /// Execute support guard: absorb damage for an ally.
    /// Returns the amount of damage absorbed.
    pub fn execute_support_guard(
        &mut self,
        _ally: &mut UnitInstance,
        incoming_damage: i64,
        _db: &crate::db::GameDatabase,
    ) -> i64 {
        // Support guard absorbs all damage (simplified — in full SRC, guard can be partial)
        let absorbed = incoming_damage;

        // Decrement support guard remaining (not support attack remaining)
        self.support_guard_remaining -= 1;

        // Apply damage to the guard unit instead of the ally
        self.damage += absorbed;

        absorbed
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
    fn party_color_is_distinct_per_party() {
        let colors = [
            Party::Player.color(),
            Party::Npc.color(),
            Party::Enemy.color(),
            Party::Neutral.color(),
        ];
        for (i, a) in colors.iter().enumerate() {
            for (j, b) in colors.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b);
                }
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
    fn update_does_not_panic_on_missing_unit_data() {
        let mut unit = UnitInstance::new("NonExistentUnit", "Pilot1", Party::Player, 0, 0);
        let db = crate::db::GameDatabase::new();
        unit.update(&db);
    }

    #[test]
    fn update_recalculates_after_item_equip() {
        let mut db = crate::db::GameDatabase::new();
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: Vec::new(),
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let item_data = crate::data::item::ItemData {
            name: "強化パーツ".to_string(),
            class: "装備".to_string(),
            part: "本体".to_string(),
            hp_mod: 500,
            en_mod: 0,
            armor_mod: 100,
            mobility_mod: 0,
            speed_mod: 0,
            comment: String::new(),
            features: Vec::new(),
        };
        db.extend_items(vec![item_data]);
        let mut unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        let base_hp = db.effective_max_hp(&unit);
        assert_eq!(base_hp, 5000);
        unit.equip_item(crate::item_slot::SlotType::Body, "強化パーツ");
        let with_item_hp = db.effective_max_hp(&unit);
        assert_eq!(with_item_hp, 5500);
        unit.update(&db);
        let after_update_hp = db.effective_max_hp(&unit);
        assert_eq!(after_update_hp, 5500);
    }

    #[test]
    fn update_recalculates_after_condition_change() {
        let mut db = crate::db::GameDatabase::new();
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: Vec::new(),
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        assert_eq!(unit.bonus_armor, 0);
        unit.add_condition(crate::condition::Condition::with_details(
            "装甲低下",
            3,
            1,
            "",
        ));
        unit.update(&db);
        assert_eq!(unit.bonus_armor, -100);
    }

    #[test]
    fn weapon_unavailable_when_has_acted() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 10,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        unit.weapons.push(crate::unit_weapon::UnitWeapon::from_data(
            "テスト武器",
            0,
            10,
        ));
        unit.has_acted = true;
        assert!(!unit.is_weapon_available(0, &db));
    }

    #[test]
    fn weapon_unavailable_when_no_bullets() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 10,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        unit.weapons.push(crate::unit_weapon::UnitWeapon::from_data(
            "テスト武器",
            0,
            10,
        ));
        unit.weapons[0].bullet_remaining = 0;
        assert!(!unit.is_weapon_available(0, &db));
    }

    #[test]
    fn weapon_unavailable_when_morale_too_low() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 10,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 120,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        assert!(!unit.is_weapon_available(0, &db));
    }

    #[test]
    fn weapon_unavailable_when_condition_prevents_attack() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 10,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        unit.weapons.push(crate::unit_weapon::UnitWeapon::from_data(
            "テスト武器",
            0,
            10,
        ));
        unit.add_condition(crate::condition::Condition::with_details("麻痺", 3, 1, ""));
        assert!(!unit.is_weapon_available(0, &db));
    }

    /// 沈黙 (特殊効果攻撃属性 黙) 中は 術 / 音 属性の武器のみ使用不能になる。
    #[test]
    fn weapon_unavailable_when_silenced_for_jutsu_weapon() {
        let mut db = crate::db::GameDatabase::new();
        let mk = |name: &str, class: &str| crate::data::unit::WeaponData {
            name: name.to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 10,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: class.to_string(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![mk("術武器", "術"), mk("通常武器", "")],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        unit.weapons
            .push(crate::unit_weapon::UnitWeapon::from_data("術武器", 0, 10));
        unit.weapons
            .push(crate::unit_weapon::UnitWeapon::from_data("通常武器", 1, 10));
        unit.add_condition(crate::condition::Condition::with_details("沈黙", 3, 1, ""));
        assert!(
            !unit.is_weapon_available(0, &db),
            "沈黙中は術属性武器が使えない"
        );
        assert!(
            unit.is_weapon_available(1, &db),
            "沈黙でも非術/音の通常武器は使える"
        );
    }

    /// 剋<属性> (特殊効果攻撃属性 剋) の状態は指定属性を持つ武器のみ使用不能にする。
    #[test]
    fn weapon_unavailable_when_element_locked() {
        let mut db = crate::db::GameDatabase::new();
        let mk = |name: &str, class: &str| crate::data::unit::WeaponData {
            name: name.to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 10,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: class.to_string(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![mk("火炎砲", "火"), mk("氷柱砲", "氷")],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        unit.weapons
            .push(crate::unit_weapon::UnitWeapon::from_data("火炎砲", 0, 10));
        unit.weapons
            .push(crate::unit_weapon::UnitWeapon::from_data("氷柱砲", 1, 10));
        unit.add_condition(crate::condition::Condition::with_details("剋:火", 3, 1, ""));
        assert!(
            !unit.is_weapon_available(0, &db),
            "剋:火 で火属性武器が封じられる"
        );
        assert!(
            unit.is_weapon_available(1, &db),
            "剋:火 でも氷属性武器は使える"
        );
    }

    #[test]
    fn weapon_range_returns_correct_range() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 2,
            max_range: 5,
            precision: 10,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        unit.weapons.push(crate::unit_weapon::UnitWeapon::from_data(
            "テスト武器",
            0,
            10,
        ));
        let range = unit.weapon_range(0, &db);
        assert_eq!(range, Some((2, 5)));
    }

    #[test]
    fn best_available_weapon_returns_highest_power() {
        let mut db = crate::db::GameDatabase::new();
        let weak_weapon = crate::data::unit::WeaponData {
            name: "弱い武器".to_string(),
            power: 500,
            min_range: 1,
            max_range: 3,
            precision: 10,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let strong_weapon = crate::data::unit::WeaponData {
            name: "強い武器".to_string(),
            power: 2000,
            min_range: 1,
            max_range: 5,
            precision: 10,
            bullet: 10,
            en_consumption: 10,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weak_weapon, strong_weapon],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        unit.weapons
            .push(crate::unit_weapon::UnitWeapon::from_data("弱い武器", 0, 10));
        unit.weapons
            .push(crate::unit_weapon::UnitWeapon::from_data("強い武器", 1, 10));
        let best = unit.best_available_weapon(&db);
        assert_eq!(best, Some(1));
    }

    #[test]
    fn attack_consumes_bullets() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 10,
            bullet: 3,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data.clone()]);
        let mut attacker = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        attacker
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                3,
            ));
        let mut defender = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 1, 0);
        defender
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let result = attacker.execute_attack(&mut defender, 0, &db, 50);
        let _ = result.hit;
        assert_eq!(attacker.weapons[0].bullet_remaining, 2);
    }

    #[test]
    fn attack_sets_has_acted() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 10,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut attacker = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        attacker
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let mut defender = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 1, 0);
        defender
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        assert!(!attacker.has_acted);
        attacker.execute_attack(&mut defender, 0, &db, 50);
        assert!(attacker.has_acted);
    }

    #[test]
    fn attack_miss_deals_no_damage() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 10,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut attacker = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        attacker
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let mut defender = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 1, 0);
        defender
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        defender.damage = 100;
        let initial_damage = defender.damage;
        defender.add_condition(crate::condition::Condition::new("ひらめき", 1));
        let result = attacker.execute_attack(&mut defender, 0, &db, 0);
        assert!(!result.hit);
        assert_eq!(result.damage, 0);
        assert_eq!(defender.damage, initial_damage);
    }

    #[test]
    fn attack_kills_target_when_damage_exceeds_hp() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 5000,
            min_range: 1,
            max_range: 5,
            precision: 10,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 100,
            hp: 1000,
            en: 100,
            armor: 0,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut attacker = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        attacker
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let mut defender = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 1, 0);
        defender
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let max_hp = db.effective_max_hp(&defender);
        assert!(defender.damage < max_hp);
        let result = attacker.execute_attack(&mut defender, 0, &db, 50);
        assert!(result.hit);
        assert!(result.target_destroyed);
        assert!(defender.damage >= max_hp);
    }

    #[test]
    fn attack_gains_exp_on_kill() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 5000,
            min_range: 1,
            max_range: 5,
            precision: 10,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 100,
            hp: 1000,
            en: 100,
            armor: 0,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut attacker = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        attacker
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let mut defender = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 1, 0);
        defender
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let initial_exp = attacker.total_exp;
        let result = attacker.execute_attack(&mut defender, 0, &db, 50);
        assert!(result.target_destroyed);
        assert!(result.exp_gained > 0);
        assert_eq!(attacker.total_exp, initial_exp + result.exp_gained);
    }

    #[test]
    fn attack_unavailable_weapon_returns_miss() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 10,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut attacker = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        attacker
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let mut defender = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 1, 0);
        defender
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        attacker.has_acted = true;
        let result = attacker.execute_attack(&mut defender, 0, &db, 50);
        assert!(!result.hit);
        assert_eq!(result.damage, 0);
    }

    #[test]
    fn counter_attack_triggers_when_defender_survives() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 100,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 100,
            hp: 5000,
            en: 100,
            armor: 0,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut attacker = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        attacker
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let mut defender = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 1, 0);
        defender
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let (atk_result, counter_result) =
            attacker.execute_attack_with_counter(&mut defender, 0, &db, 50, 50);
        assert!(atk_result.hit);
        assert!(counter_result.is_some());
        if let Some(counter) = counter_result {
            let _ = counter.hit;
        }
    }

    #[test]
    fn counter_attack_does_not_trigger_when_defender_killed() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 5000,
            min_range: 1,
            max_range: 5,
            precision: 100,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 100,
            hp: 1000,
            en: 100,
            armor: 0,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut attacker = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        attacker
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let mut defender = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 1, 0);
        defender
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let (atk_result, counter_result) =
            attacker.execute_attack_with_counter(&mut defender, 0, &db, 50, 50);
        assert!(atk_result.target_destroyed);
        assert!(counter_result.is_none());
    }

    #[test]
    fn counter_attack_does_not_trigger_when_defender_has_acted() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 100,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 100,
            hp: 5000,
            en: 100,
            armor: 0,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut attacker = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        attacker
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let mut defender = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 1, 0);
        defender
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        defender.has_acted = true;
        let (_, counter_result) =
            attacker.execute_attack_with_counter(&mut defender, 0, &db, 50, 50);
        assert!(counter_result.is_none());
    }

    #[test]
    fn counter_attack_does_not_trigger_when_no_weapon_in_range() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 1,
            precision: 100,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 100,
            hp: 5000,
            en: 100,
            armor: 0,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut attacker = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        attacker
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let mut defender = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 5, 0);
        defender
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let (_, counter_result) =
            attacker.execute_attack_with_counter(&mut defender, 0, &db, 50, 50);
        assert!(counter_result.is_none());
    }

    #[test]
    fn counter_attack_uses_best_available_weapon() {
        let mut db = crate::db::GameDatabase::new();
        let weak_weapon = crate::data::unit::WeaponData {
            name: "弱い武器".to_string(),
            power: 500,
            min_range: 1,
            max_range: 5,
            precision: 100,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let strong_weapon = crate::data::unit::WeaponData {
            name: "強い武器".to_string(),
            power: 2000,
            min_range: 1,
            max_range: 5,
            precision: 100,
            bullet: 10,
            en_consumption: 10,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 100,
            hp: 5000,
            en: 100,
            armor: 0,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weak_weapon, strong_weapon],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let mut attacker = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        attacker
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data("弱い武器", 0, 10));
        attacker
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data("強い武器", 1, 10));
        let mut defender = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 1, 0);
        defender
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data("弱い武器", 0, 10));
        defender
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data("強い武器", 1, 10));
        let (_, counter_result) =
            attacker.execute_attack_with_counter(&mut defender, 0, &db, 50, 50);
        assert!(counter_result.is_some());
        let counter = counter_result.unwrap();
        assert!(counter.hit);
    }

    #[test]
    fn support_attack_triggers_with_adjacent_ally() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 100,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 100,
            hp: 5000,
            en: 100,
            armor: 0,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data.clone()]);
        let pilot_data = crate::data::pilot::PilotData {
            spirit_commands: Vec::new(),
            name: "援護パイロット".to_string(),
            nickname: "援護".to_string(),
            kana_name: "えんご".to_string(),
            sex: crate::data::pilot::Sex::Unspecified,
            class: String::new(),
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            exp_value: 0,
            infight: 0,
            shooting: 0,
            hit: 0,
            dodge: 0,
            intuition: 0,
            technique: 0,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: None,
            features: vec![("援護".to_string(), "".to_string())],
        };
        db.extend_pilots(vec![pilot_data]);
        let mut support_unit = UnitInstance::new("テスト機", "援護パイロット", Party::Player, 0, 0);
        support_unit
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        support_unit.add_pilot_id("援護パイロット".to_string());
        let mut ally_unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 1, 0);
        ally_unit
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let mut enemy_unit = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 2, 0);
        enemy_unit
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        assert!(support_unit.can_support_attack(&ally_unit, &enemy_unit, &db));
    }

    #[test]
    fn support_guard_absorbs_damage() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 100,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 100,
            hp: 5000,
            en: 100,
            armor: 0,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let pilot_data = crate::data::pilot::PilotData {
            spirit_commands: Vec::new(),
            name: "援護パイロット".to_string(),
            nickname: "援護".to_string(),
            kana_name: "えんご".to_string(),
            sex: crate::data::pilot::Sex::Unspecified,
            class: String::new(),
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            exp_value: 0,
            infight: 0,
            shooting: 0,
            hit: 0,
            dodge: 0,
            intuition: 0,
            technique: 0,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: None,
            features: vec![("援護".to_string(), "".to_string())],
        };
        db.extend_pilots(vec![pilot_data]);
        let mut guard_unit = UnitInstance::new("テスト機", "援護パイロット", Party::Player, 1, 0);
        guard_unit.add_pilot_id("援護パイロット".to_string());
        let mut ally_unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 0, 0);
        let initial_guard_damage = guard_unit.damage;
        let incoming_damage = 500_i64;
        let absorbed = guard_unit.execute_support_guard(&mut ally_unit, incoming_damage, &db);
        assert_eq!(absorbed, incoming_damage);
        assert_eq!(guard_unit.damage, initial_guard_damage + incoming_damage);
        assert_eq!(guard_unit.support_guard_remaining, 0);
    }

    #[test]
    fn support_attack_limited_to_once_per_turn() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 100,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 100,
            hp: 5000,
            en: 100,
            armor: 0,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data.clone()]);
        let pilot_data = crate::data::pilot::PilotData {
            spirit_commands: Vec::new(),
            name: "援護パイロット".to_string(),
            nickname: "援護".to_string(),
            kana_name: "えんご".to_string(),
            sex: crate::data::pilot::Sex::Unspecified,
            class: String::new(),
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            exp_value: 0,
            infight: 0,
            shooting: 0,
            hit: 0,
            dodge: 0,
            intuition: 0,
            technique: 0,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: None,
            features: vec![("援護".to_string(), "".to_string())],
        };
        db.extend_pilots(vec![pilot_data]);
        let mut support_unit = UnitInstance::new("テスト機", "援護パイロット", Party::Player, 0, 0);
        support_unit
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        support_unit.add_pilot_id("援護パイロット".to_string());
        let mut ally_unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 1, 0);
        ally_unit
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let mut enemy_unit = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 2, 0);
        enemy_unit
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        assert_eq!(support_unit.support_attack_remaining, 1);
        let result = support_unit.execute_support_attack(&mut enemy_unit, &db, 50);
        let _ = result.hit;
        assert_eq!(support_unit.support_attack_remaining, 0);
        assert!(!support_unit.can_support_attack(&ally_unit, &enemy_unit, &db));
    }

    #[test]
    fn support_attack_requires_adjacency() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 100,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 100,
            hp: 5000,
            en: 100,
            armor: 0,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let pilot_data = crate::data::pilot::PilotData {
            spirit_commands: Vec::new(),
            name: "援護パイロット".to_string(),
            nickname: "援護".to_string(),
            kana_name: "えんご".to_string(),
            sex: crate::data::pilot::Sex::Unspecified,
            class: String::new(),
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            exp_value: 0,
            infight: 0,
            shooting: 0,
            hit: 0,
            dodge: 0,
            intuition: 0,
            technique: 0,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: None,
            features: vec![("援護".to_string(), "".to_string())],
        };
        db.extend_pilots(vec![pilot_data]);
        let mut support_unit = UnitInstance::new("テスト機", "援護パイロット", Party::Player, 0, 0);
        support_unit
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        support_unit.add_pilot_id("援護パイロット".to_string());
        let mut ally_unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 5, 5);
        ally_unit
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let mut enemy_unit = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 6, 5);
        enemy_unit
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        assert!(!support_unit.can_support_attack(&ally_unit, &enemy_unit, &db));
    }

    #[test]
    fn support_attack_requires_skill() {
        let mut db = crate::db::GameDatabase::new();
        let weapon_data = crate::data::unit::WeaponData {
            name: "テスト武器".to_string(),
            power: 1000,
            min_range: 1,
            max_range: 5,
            precision: 100,
            bullet: 10,
            en_consumption: 5,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        };
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 100,
            hp: 5000,
            en: 100,
            armor: 0,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: vec![weapon_data],
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let pilot_data = crate::data::pilot::PilotData {
            spirit_commands: Vec::new(),
            name: "通常パイロット".to_string(),
            nickname: "通常".to_string(),
            kana_name: "つうじょう".to_string(),
            sex: crate::data::pilot::Sex::Unspecified,
            class: String::new(),
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            exp_value: 0,
            infight: 0,
            shooting: 0,
            hit: 0,
            dodge: 0,
            intuition: 0,
            technique: 0,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: None,
            features: Vec::new(),
        };
        db.extend_pilots(vec![pilot_data]);
        let mut normal_unit = UnitInstance::new("テスト機", "通常パイロット", Party::Player, 0, 0);
        normal_unit
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        normal_unit.add_pilot_id("通常パイロット".to_string());
        let mut ally_unit = UnitInstance::new("テスト機", "Pilot1", Party::Player, 1, 0);
        ally_unit
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        let mut enemy_unit = UnitInstance::new("テスト機", "Pilot1", Party::Enemy, 2, 0);
        enemy_unit
            .weapons
            .push(crate::unit_weapon::UnitWeapon::from_data(
                "テスト武器",
                0,
                10,
            ));
        assert!(!normal_unit.can_support_attack(&ally_unit, &enemy_unit, &db));
    }

    #[test]
    fn update_sets_water_movement_from_pilot_skill() {
        let mut db = crate::db::GameDatabase::new();
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: Vec::new(),
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let pilot_data = crate::data::pilot::PilotData {
            spirit_commands: Vec::new(),
            name: "水上パイロット".to_string(),
            nickname: "水上".to_string(),
            kana_name: "すいじょう".to_string(),
            sex: crate::data::pilot::Sex::Unspecified,
            class: String::new(),
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            exp_value: 0,
            infight: 100,
            shooting: 100,
            hit: 100,
            dodge: 100,
            intuition: 100,
            technique: 100,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: None,
            features: vec![("水上移動".to_string(), "".to_string())],
        };
        db.extend_pilots(vec![pilot_data.clone()]);

        db.create_pilot_instance("水上パイロット", "p1");

        let mut unit = UnitInstance::new("テスト機", "水上パイロット", Party::Player, 0, 0);
        unit.add_pilot_id("p1".to_string());
        assert!(!unit.can_enter_water);
        unit.update(&db);
        assert!(unit.can_enter_water);
    }

    #[test]
    fn update_sets_air_movement_from_pilot_skill() {
        let mut db = crate::db::GameDatabase::new();
        let unit_data = crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "テスト機".to_string(),
            kana_name: "てすとき".to_string(),
            nickname: "テスト".to_string(),
            class: "t".to_string(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".to_string(),
            speed: 100,
            size: crate::data::unit::Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 120,
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: Vec::new(),
            features: Vec::new(),
        };
        db.extend_units(vec![unit_data]);
        let pilot_data = crate::data::pilot::PilotData {
            spirit_commands: Vec::new(),
            name: "空中パイロット".to_string(),
            nickname: "空中".to_string(),
            kana_name: "くうちゅう".to_string(),
            sex: crate::data::pilot::Sex::Unspecified,
            class: String::new(),
            adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
            exp_value: 0,
            infight: 100,
            shooting: 100,
            hit: 100,
            dodge: 100,
            intuition: 100,
            technique: 100,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: None,
            features: vec![("空中移動".to_string(), "".to_string())],
        };
        db.extend_pilots(vec![pilot_data.clone()]);

        db.create_pilot_instance("空中パイロット", "p1");

        let mut unit = UnitInstance::new("テスト機", "空中パイロット", Party::Player, 0, 0);
        unit.add_pilot_id("p1".to_string());
        assert!(!unit.can_enter_air);
        unit.update(&db);
        assert!(unit.can_enter_air);
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
