//! 戦闘予測 / Combat prediction.
//!
//! SRC `UnitWeapon.cs` 準拠の計算式:
//!
//! **命中率:**
//! ```text
//! ed_hit = 100 + atk.hit + atk.intuition + atk_unit.mobility + weapon.precision
//! ed_avd = def.dodge + def.intuition + def_unit.mobility
//! terrain_mult = (100 - def_terrain_hit_mod) / 100   # hit_mod=10 → 0.90 (正=被命中減)
//! size_mult = XL:2.0 / LL:1.4 / L:1.2 / M:1.0 / S:0.8 / SS:0.5
//! hit_chance = max(0, (ed_hit - ed_avd) * terrain_mult * size_mult)  # 上限なし (>100=必中)
//! ```
//!
//! **ダメージ:**
//! ```text
//! # 武器属性: '格' or max_range==1 → infight、それ以外 → shooting
//! atk_power = weapon.power * pilot_attack / 100 * atk_morale / 100
//! def_power = def_unit.armor * def_morale / 100
//! terrain_dmg_mult = (100 - def_terrain_damage_mod) / 100  # damage_mod=5 → 0.95
//! damage = max(10, (atk_power - def_power) * terrain_dmg_mult)  # 最低ダメージ 10 (原典既定)
//! ```
//!
//! 武器の射程外時は呼び出し側で `manhattan_distance` で弾く想定。

use crate::data::pilot::PilotData;
use crate::data::unit::{Size, UnitData, WeaponData};

/// 1 回攻撃の戦闘予測 / Single attack preview.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CombatPreview {
    /// 命中率 (0..=100)
    pub hit_chance: i32,
    /// 期待ダメージ
    pub damage: i64,
    /// クリティカル発生率 (1..=100)。SRC `戦闘システム詳細.md` 準拠。
    /// 抽選はせず予測表示用の値のみ提供する (`critical_probability`)。
    pub critical_chance: i32,
}

impl CombatPreview {
    /// 散 (散布/scatter) 属性武器の距離補正を適用した予測を返す。`predict_*` は
    /// 距離非依存のため、攻撃側↔防御側の manhattan 距離が判る呼び出し側でこれを通す。
    /// 命中アップ・ダメージダウン (`scatter_hit_bonus`/`scatter_damage_mult`)。
    pub fn apply_scatter(mut self, weapon_class: &str, distance: u32) -> Self {
        self.hit_chance += scatter_hit_bonus(weapon_class, distance);
        self.damage = (self.damage as f64 * scatter_damage_mult(weapon_class, distance)) as i64;
        self
    }
}

/// 散 (散布/scatter) 属性武器の命中補正。SRC.NET `Unit.cs` HitProbability:
/// 「散属性武器は指定したレベル以上離れるほど命中がアップ」。manhattan 距離が
/// 1/2/3/4/5+ で +0/+5/+10/+15/+20。武器 class に `散` が無ければ 0。
pub fn scatter_hit_bonus(weapon_class: &str, distance: u32) -> i32 {
    if !weapon_class.contains('散') {
        return 0;
    }
    ((distance.max(1) - 1) * 5).min(20) as i32
}

/// 散 属性武器のダメージ補正倍率。SRC.NET `Unit.cs` Damage:
/// 「散属性武器は離れるほどダメージダウン」。manhattan 距離が
/// 1/2/3/4/5+ で ×1.0/0.95/0.90/0.85/0.80。武器 class に `散` が無ければ 1.0。
pub fn scatter_damage_mult(weapon_class: &str, distance: u32) -> f64 {
    if !weapon_class.contains('散') {
        return 1.0;
    }
    let steps = (distance.max(1) - 1).min(4);
    1.0 - 0.05 * steps as f64
}

/// 防御側の選択した防御モード / Defender's chosen defense mode for this attack.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DefenseMode {
    /// 通常防御 — 特別ボーナスなし。
    #[default]
    Normal,
    /// 回避 (dodge) — 命中率が防御側の回避 stat / 5 だけ低下する。
    Dodge,
    /// 防御 (defend) — ダメージが半減する。
    Defend,
    /// バリア (barrier) — barrier_strength 分だけダメージを吸収する。
    Barrier { strength: i64 },
    /// シールド (shield) — chance% の確率でダメージを完全無効化する。
    Shield { chance: i32 },
}

/// 精神コマンドによる与/被ダメージ修正のレベル束 (C# `UnitWeapon.cs` の up/down-mod 用)。
///
/// 各フィールドは対応する効果タイプの **最大値** レベル (C# `Unit.SpecialPowerEffectLevel`
/// ＝影響下スペシャルパワーの効果レベルの最大値。異なる精神間で加算はしない)。
/// `atk_increase` / `atk_decrease_dealt` は **攻撃側**、`def_increase_taken` /
/// `def_decrease_taken` は **防御側** の active 効果から解決する。
/// [`predict_with_status_terrain`] は C# 準拠で次を適用する:
/// up = `max(1, 1 + 0.1*atk_increase) + 0.1*def_increase_taken`、
/// down = `1 - 0.1*atk_decrease_dealt - 0.1*def_decrease_taken`。
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct DamageSpiritLevels {
    /// 攻撃側 `ダメージ増加` (熱血/魂/気合 等)。MaxDbl 非加算で与ダメージ ×(1+0.1×Lv)。
    pub atk_increase: f64,
    /// 防御側 `被ダメージ増加` (分析/偵察 等)。up-mod へ +0.1×Lv で加算。
    pub def_increase_taken: f64,
    /// 攻撃側 `ダメージ低下`。down-mod へ -0.1×Lv で減算。
    pub atk_decrease_dealt: f64,
    /// 防御側 `被ダメージ低下` (鉄壁=7.5/不屈=9/無敵=10 等)。down-mod へ -0.1×Lv。
    pub def_decrease_taken: f64,
}

/// 単一精神名から効果タイプ `effect_type` の既定レベルを返す。sp.txt 未読込経路用。
///
/// 実 SRC 標準データ (スパロボ戦記 system/sp.txt) 準拠の既定テーブル:
/// - `ダメージ増加`: 熱血=10 / 魂=20 / 気合=0 (気合 は気力増加でダメージ増加なし)。
/// - `被ダメージ低下`: 鉄壁=7.5 (1-0.75=×0.25=÷4) / 不屈=9 (1-0.9=×0.1) / 無敵=10 (×0)。
/// - `被ダメージ増加`: 分析/偵察=1。
/// - `ダメージ低下`: 分析/偵察=1。
///
/// 該当しなければ 0.0。シナリオ sp.txt 読込済みの実プレイ経路では DB 値
/// ([`crate::db::GameDatabase::sp_effect_level`]) が優先される (本テーブルは合成
/// テスト経路の互換維持用)。
pub fn default_sp_effect_level_single(name: &str, effect_type: &str) -> f64 {
    let has = |needle: &str| name.contains(needle);
    match effect_type {
        // ダメージ増加: 熱血=10 / 魂=20 / 気合=0 (気合 は気力増加でありダメージ増加なし)。
        "ダメージ増加" if has("熱血") => 10.0,
        "ダメージ増加" if has("魂") => 20.0,
        // 被ダメージ低下: 鉄壁=7.5 (÷4) / 不屈=9 (×0.1) / 無敵=10 (×0)。
        "被ダメージ低下" if has("鉄壁") => 7.5,
        "被ダメージ低下" if has("不屈") => 9.0,
        "被ダメージ低下" if has("無敵") => 10.0,
        // 被ダメージ増加 (防御側) / ダメージ低下 (攻撃側): 分析/偵察=1。
        "被ダメージ増加" | "ダメージ低下" if has("分析") || has("偵察") => 1.0,
        _ => 0.0,
    }
}

/// 精神名スライスから効果タイプ `effect_type` の **最大値** レベルを既定テーブルで解決する。
///
/// sp.txt を読み込まない経路 (`predict` / `predict_with_status` の薄いラッパや、
/// [`crate::db::GameDatabase::sp_effect_level`] の DB 未定義フォールバック) 用。
/// C# `Unit.SpecialPowerEffectLevel` と同じく加算ではなく最大値勝ち。該当なしは 0.0。
pub fn default_sp_effect_level(statuses: &[String], effect_type: &str) -> f64 {
    statuses
        .iter()
        .map(|s| default_sp_effect_level_single(s, effect_type))
        .fold(0.0f64, f64::max)
}

/// 精神名スライスから `ダメージ増加` 効果レベルの最大値を既定テーブルで解決する
/// (後方互換ラッパ)。[`default_sp_effect_level`] に `"ダメージ増加"` を渡すのと同じ。
pub fn default_damage_boost_level(statuses: &[String]) -> f64 {
    default_sp_effect_level(statuses, "ダメージ増加")
}

/// 攻撃側 `atk_statuses` / 防御側 `def_statuses` の精神名スライスから既定テーブルで
/// 4 種のダメージ修正レベル束を解決する (sp.txt 未読込経路用)。
pub fn default_damage_spirit_levels(
    atk_statuses: &[String],
    def_statuses: &[String],
) -> DamageSpiritLevels {
    DamageSpiritLevels {
        atk_increase: default_sp_effect_level(atk_statuses, "ダメージ増加"),
        def_increase_taken: default_sp_effect_level(def_statuses, "被ダメージ増加"),
        atk_decrease_dealt: default_sp_effect_level(atk_statuses, "ダメージ低下"),
        def_decrease_taken: default_sp_effect_level(def_statuses, "被ダメージ低下"),
    }
}

/// 4 隣接マスのマンハッタン距離。
pub const fn manhattan(a: (u32, u32), b: (u32, u32)) -> u32 {
    a.0.abs_diff(b.0) + a.1.abs_diff(b.1)
}

/// 武器が `distance` (マンハッタン) で使用可能か。
pub const fn weapon_in_range(weapon: &WeaponData, distance: u32) -> bool {
    let d = distance as i32;
    d >= weapon.min_range && d <= weapon.max_range
}

pub fn predict(
    atk_pilot: &PilotData,
    atk_unit: &UnitData,
    weapon: &WeaponData,
    def_pilot: &PilotData,
    def_unit: &UnitData,
    def_terrain_hit_mod: i32,
    def_terrain_damage_mod: i32,
) -> CombatPreview {
    predict_with_status(
        atk_pilot,
        atk_unit,
        weapon,
        def_pilot,
        def_unit,
        def_terrain_hit_mod,
        def_terrain_damage_mod,
        100,
        100,
        &[],
        &[],
    )
}

/// 地形適応の修正率 (SRC `戦闘システム詳細.md`: S=1.4 / A=1.2 / B=1.0 / C=0.8 /
/// D=0.6 / `-`=0)。不明な文字 (E 等) は B=1.0 相当で安全側に倒す。
pub fn adaptation_mult(letter: u8) -> f64 {
    match letter {
        b'S' | b's' => 1.4,
        b'A' | b'a' => 1.2,
        b'B' | b'b' => 1.0,
        b'C' | b'c' => 0.8,
        b'D' | b'd' => 0.6,
        b'-' => 0.0,
        _ => 1.0,
    }
}

/// 地形クラス名 → 地形適応の環境インデックス (0=空 / 1=陸 / 2=海 / 3=宇)。
/// `Adaption([u8;4])` の並びに対応 (data/pilot.rs)。
pub fn terrain_env(class: &str) -> i32 {
    match class {
        "海" | "水" | "深海" => 2,
        "宇宙" => 3,
        "空" | "空中" => 0,
        // 平地 / 道路 / 森林 / 山 / 都市 など地上系は陸。
        _ => 1,
    }
}

/// 武器の地形適応文字列 (4 文字) から `env` の修正率。未指定 (4 文字未満) は
/// 制限なし (S=1.4) 扱いで、ユニット側適応が支配的になるようにする。
fn weapon_adaptation_mult(weapon_adaption: &str, env: usize) -> f64 {
    let b = weapon_adaption.as_bytes();
    if b.len() >= 4 {
        adaptation_mult(b[env])
    } else {
        1.4
    }
}

/// 攻撃側 / 防御側の地形適応修正率 `(atk, def)` を返す。`atk_env`/`def_env` が
/// 負 (地形情報なし: 単純 `predict` / プレビュー / テスト) なら `(1.0, 1.0)`。
///
/// SRC `戦闘システム詳細.md` 準拠:
/// - 攻撃力 = 武器とユニットの地形適応のうち**低い方**。ユニット適応は
///   ユニットデータとメインパイロットの低い方。武器適応は**防御側地形**で参照。
/// - ユニット適応は自分のいる地形で参照するが、近接 (武/突/接 = `is_melee`) は
///   防御側地形まで踏み込むとみなし防御側地形で参照する。
/// - 防御力 = 防御側ユニット (×パイロット) の自地形適応。
#[allow(clippy::too_many_arguments)]
fn terrain_adaptation_mults(
    atk_pilot: &PilotData,
    atk_unit: &UnitData,
    weapon: &WeaponData,
    def_pilot: &PilotData,
    def_unit: &UnitData,
    atk_env: i32,
    def_env: i32,
    is_melee: bool,
) -> (f64, f64) {
    if atk_env < 0 || def_env < 0 {
        return (1.0, 1.0);
    }
    let de = def_env as usize;
    let au_env = if is_melee { de } else { atk_env as usize };
    let unit_mult = adaptation_mult(atk_unit.adaption.0[au_env])
        .min(adaptation_mult(atk_pilot.adaption.0[au_env]));
    let weapon_mult = weapon_adaptation_mult(&weapon.adaption, de);
    let atk_mult = unit_mult.min(weapon_mult);
    let def_mult =
        adaptation_mult(def_unit.adaption.0[de]).min(adaptation_mult(def_pilot.adaption.0[de]));
    (atk_mult, def_mult)
}

/// `predict` の状態異常 / 精神コマンド対応版。SRC `UnitWeapon.cs::HitPoint` / `Damage` 準拠。
///
/// `atk_statuses` / `def_statuses` は `UnitInstance.conditions` の name を渡す。
/// `atk_morale` / `def_morale` は `UnitInstance.morale`（初期値 100）。
///
/// | 状態 | 効果 |
/// | --- | --- |
/// | `必中` (attacker) | 命中 100 固定 |
/// | `集中` (attacker) | 命中 +30 |
/// | `ダメージ増加` (attacker) | ダメージ ×(1 + 0.1×Lv)。Lv は sp.txt 由来 (熱血/魂/気合 等)。`dmg_levels.atk_increase` で受領 |
/// | `被ダメージ増加` (defender) | up-mod へ +0.1×Lv (加算、分析/偵察 等)。`dmg_levels.def_increase_taken` |
/// | `ダメージ低下` (attacker) | down-mod へ -0.1×Lv (分析/偵察 等)。`dmg_levels.atk_decrease_dealt` |
/// | `被ダメージ低下` (defender) | down-mod へ -0.1×Lv。鉄壁=7.5(÷4)/不屈=9(×0.1)/無敵=10(×0)。`dmg_levels.def_decrease_taken` |
/// | `捨て身` (attacker) | ダメージ 3 倍 |
/// | `捨て身` (defender) | 無防備 = 命中 100 |
/// | `直撃` (attacker) | 防御側 `分身`/`バリア` を無効化 |
/// | `集中` (defender) | 回避 +30 (= 攻撃側命中 -30) |
/// | `ひらめき` (defender) | 命中 0 |
/// | `毒` (defender) | 命中 +10 |
/// | `麻痺` / `睡眠` / `凍結` (defender) | 命中 100、ダメージ 1.5 倍 |
/// | `石化` / `行動不能` (defender) | 命中 100 |
/// | `分身` (defender) | 命中 -40 |
/// | `ステルス` (defender) | 命中 -30 (距離情報なしの近似) |
/// | `バリア` (defender) | ダメージ 1/2 |
#[allow(clippy::too_many_arguments)]
pub fn predict_with_status(
    atk_pilot: &PilotData,
    atk_unit: &UnitData,
    weapon: &WeaponData,
    def_pilot: &PilotData,
    def_unit: &UnitData,
    def_terrain_hit_mod: i32,
    def_terrain_damage_mod: i32,
    atk_morale: i32,
    def_morale: i32,
    atk_statuses: &[String],
    def_statuses: &[String],
) -> CombatPreview {
    // 地形情報なし版 (地形適応 ×1.0)。地形適応込みは predict_with_status_terrain。
    // DB を持たないラッパ経路では既定テーブルで 4 種のダメージ修正レベルを解決する。
    let dmg_levels = default_damage_spirit_levels(atk_statuses, def_statuses);
    predict_with_status_terrain(
        atk_pilot,
        atk_unit,
        weapon,
        def_pilot,
        def_unit,
        def_terrain_hit_mod,
        def_terrain_damage_mod,
        atk_morale,
        def_morale,
        atk_statuses,
        def_statuses,
        -1,
        -1,
        dmg_levels,
        1.0, // ＥＣＭ 補正なし (ラッパ経路は盤面非依存)
    )
}

/// [`predict_with_status`] の地形適応対応版。`atk_env`/`def_env` は地形適応の
/// 環境インデックス (0=空/1=陸/2=海/3=宇、[`terrain_env`] で算出)。負なら適応 ×1.0。
///
/// `dmg_levels` は精神コマンドによる 4 種のダメージ修正レベル束 ([`DamageSpiritLevels`])。
/// C# `UnitWeapon.cs` 準拠で与ダメージへ次を適用する:
/// up = `max(1, 1 + 0.1*atk_increase) + 0.1*def_increase_taken`、
/// down = `1 - 0.1*atk_decrease_dealt - 0.1*def_decrease_taken`。各レベルは
/// シナリオ sp.txt から ([`crate::db::GameDatabase::sp_effect_level`])、または既定テーブル
/// ([`default_damage_spirit_levels`]) で解決して渡す。鉄壁 (被ダメージ低下Lv7.5→÷4) /
/// 不屈 (被ダメージ低下Lv9→×0.1) もこの down-mod 経由で処理する (旧来のハードコードを置換)。
#[allow(clippy::too_many_arguments)]
pub fn predict_with_status_terrain(
    atk_pilot: &PilotData,
    atk_unit: &UnitData,
    weapon: &WeaponData,
    def_pilot: &PilotData,
    def_unit: &UnitData,
    def_terrain_hit_mod: i32,
    def_terrain_damage_mod: i32,
    atk_morale: i32,
    def_morale: i32,
    atk_statuses: &[String],
    def_statuses: &[String],
    atk_env: i32,
    def_env: i32,
    dmg_levels: DamageSpiritLevels,
    // ＥＣＭ エリア補正 (防御能力 (B)): 命中率に掛ける係数 (1.0 = 補正なし)。
    // 周囲の味方/敵 ＥＣＭ Lv 差から App 側で算出して渡す (`App::ecm_hit_mult`)。
    // 予測 (プレビュー) にも反映する確定補正。`必中`/`ひらめき` の絶対上書きより前に適用。
    ecm_hit_mult: f64,
) -> CombatPreview {
    let has = |s: &[String], name: &str| s.iter().any(|t| t.contains(name));

    // ── 命中率 ──────────────────────────────────────────
    // SRC: (100 + 攻撃側命中値 - 防御側回避値) × 地形命中補正 × サイズ補正
    //   攻撃側命中値 = pilot.hit + pilot.intuition + unit.mobility + weapon.precision
    //   防御側回避値 = pilot.dodge + pilot.intuition + unit.mobility
    let ed_hit = 100 + atk_pilot.hit + atk_pilot.intuition + atk_unit.mobility + weapon.precision;
    let ed_avd = def_pilot.dodge + def_pilot.intuition + def_unit.mobility;

    // 状態異常補正 (加算)
    let mut hit_adj = 0i32;
    if has(atk_statuses, "集中") {
        hit_adj += 30;
    }
    if has(def_statuses, "集中") {
        hit_adj -= 30;
    }
    if has(def_statuses, "毒") {
        hit_adj += 10;
    }
    // 分身 / 超回避 は (B) で**実行段の完全回避ロール** (App::check_dodge_feature) へ移行した
    // ため、予測 (hit%) には反映しない (SRC の HitProbability も畳み込まない＝CheckDodgeFeature
    // は別経路)。ステルス は依然 hit% 低下 (-30) として扱う。
    if has(def_statuses, "ステルス") {
        hit_adj -= 30;
    }

    // 地形命中補正: SRC `Unit.cls:6295` / C# `(100 - HitMod)/100`。命中修正 (回避修正) は
    // **正の値ほど被命中を下げる** (防御地形)。terrain.txt の 命中修正 列はこの正の規約で格納される
    // (`マップデータ.md`)。例: 森林 hit_mod=10 → 0.90 倍 / 山 30 → 0.70 倍。
    let terrain_hit_mult = ((100 - def_terrain_hit_mod) as f64 / 100.0).clamp(0.0, 1.5);

    // サイズ補正 (防御側ユニットのサイズ)
    let size_mult: f64 = match def_unit.size {
        Size::XL => 2.0,
        Size::LL => 1.4,
        Size::L => 1.2,
        Size::M => 1.0,
        Size::S => 0.8,
        Size::SS => 0.5,
    };

    let raw_hit_f = ((ed_hit - ed_avd + hit_adj) as f64 * terrain_hit_mult * size_mult) as i32;
    // SRC `Unit.cls:6694-6696`（VB6 原典）/ C# `UnitWeapon.cs` `Math.Max(0, prob)`:
    // 命中率は最低 0 のみクランプし、上限は設けない（100 超は必中。判定は各所
    // `roll(0..=99) < hit_chance` のため >100 は常に命中、<1 は殆ど命中しない）。
    // 旧実装の `clamp(5, 95)` は他 SRPG 慣習由来で原典非準拠だった。表示上の 100 上限は
    // 描画側で `min(100)` する（C# も COM.cs `MinLng(HitProbability,100)` ＝表示のみ）。
    let mut hit_chance = raw_hit_f.max(0);
    // 盲目 / 撹乱 (攻撃側): 命中率が半減する (特殊効果攻撃属性 盲 / 撹)。
    if has(atk_statuses, "盲目") || has(atk_statuses, "撹乱") {
        hit_chance /= 2;
    }
    // 盲目 / 狂戦士 (防御側): これらのユニットへの攻撃の命中率は 1.5 倍 (95 上限)。
    if has(def_statuses, "盲目") || has(def_statuses, "狂戦士") {
        hit_chance = (hit_chance * 3 / 2).min(95);
    }
    // ＥＣＭ エリア補正 (防御能力 (B)): 周囲の味方/敵 ＥＣＭ Lv 差による命中低下を掛ける
    // (1.0 = 補正なし)。`必中`/`ひらめき` の絶対上書きより前に適用する。
    if ecm_hit_mult < 1.0 {
        hit_chance = (f64::from(hit_chance) * ecm_hit_mult) as i32;
    }
    // 必中 (attacker) / 捨て身 (defender=無防備) / 行動不能系 (麻痺/睡眠/凍結/石化/行動不能)
    // → 命中 100 (SRC `Unit.cs`: 「動けなければ絶対に命中」)。
    if has(atk_statuses, "必中")
        || has(def_statuses, "捨て身")
        || has(def_statuses, "麻痺")
        || has(def_statuses, "睡眠")
        || has(def_statuses, "凍結")
        || has(def_statuses, "石化")
        || has(def_statuses, "行動不能")
    {
        hit_chance = 100;
    }
    if has(def_statuses, "ひらめき") {
        hit_chance = 0;
    }

    // ── ダメージ ─────────────────────────────────────────
    // 武器属性: '格' 含む or (射程外属性なし and max_range==1) → 格闘 (infight)
    //           '射' 含む or max_range >= 2 → 射撃 (shooting)
    let is_melee =
        weapon.class.contains('格') || (!weapon.class.contains('射') && weapon.max_range == 1);
    let pilot_attack = if is_melee {
        atk_pilot.infight
    } else {
        atk_pilot.shooting
    };

    // 地形適応修正率 (SRC: 攻撃力/防御力それぞれに乗る)。env 負なら (1.0, 1.0)。
    let (atk_adapt, def_adapt) = terrain_adaptation_mults(
        atk_pilot, atk_unit, weapon, def_pilot, def_unit, atk_env, def_env, is_melee,
    );

    // 攻撃力 = weapon.power × pilot_attack/100 × morale/100 × 地形適応
    let atk_power = ((weapon.power * i64::from(pilot_attack) / 100 * i64::from(atk_morale) / 100)
        as f64
        * atk_adapt) as i64;

    // 防御側パイロットの Defense 係数 (耐久 技能): SRC は装甲を Pilot.Defense/100 倍する
    // (`UnitWeapon.cs::Damage` `arm = arm * withBlock.Defense / 100`)。既定オプション下の
    // Defense は `100 + 5 * SkillLevel("耐久")` (`Pilot.cls:402`)。
    // 「防御力成長 / 防御力レベルアップ」オプション下の Level 加算項は既定オフのため未モデル化。
    // 耐久 Lv は攻撃側ハンター技能 (下記) と同じ要領で features の `耐久Lv<n>` 接尾辞から抽出
    // (無印 = Lv1、半角/全角を許容)。
    let def_endurance_lv: i64 = def_pilot
        .features
        .iter()
        .find_map(|(fname, _)| {
            let rest = fname.trim().strip_prefix("耐久")?;
            if rest.is_empty() {
                return Some(1);
            }
            rest.strip_prefix("Lv")
                .or_else(|| rest.strip_prefix("LV"))
                .or_else(|| rest.strip_prefix("lv"))
                .or_else(|| rest.strip_prefix("Ｌｖ"))
                .or_else(|| rest.strip_prefix("ＬＶ"))
                .map(|a| {
                    a.chars()
                        .filter(char::is_ascii_digit)
                        .collect::<String>()
                        .parse()
                        .unwrap_or(1)
                })
        })
        .unwrap_or(0);
    let def_defense = 100 + 5 * def_endurance_lv;

    // 防御力 = armor × morale/100 × Defense/100 × 地形適応
    let def_power = ((def_unit.armor * i64::from(def_morale) / 100) as f64
        * (def_defense as f64 / 100.0)
        * def_adapt) as i64;

    // 地形ダメージ補正: damage_mod=5 → 0.95 倍 (正値ほど軽減)
    let terrain_dmg_mult = ((100 - def_terrain_damage_mod) as f64 / 100.0).clamp(0.0, 2.0);

    let mut raw_dmg = ((atk_power - def_power) as f64 * terrain_dmg_mult) as i64;

    // 精神コマンドによるダメージ増加 (up-mod): SRC `UnitWeapon.cs`。
    //   dmg_mod = MaxDbl(1, 1 + 0.1*atk.SpecialPowerEffectLevel("ダメージ増加"))   ← 攻撃側 (非加算)
    //           + 0.1*def.SpecialPowerEffectLevel("被ダメージ増加")                ← 防御側 (加算)
    // 倍率は sp.txt データ駆動 (caller が DB から解決して渡す)。常に乗算する
    // (1.0 倍でも構わない) ことで C# のキャスト切り捨て挙動と一致させる。
    {
        let up_mod =
            (1.0 + 0.1 * dmg_levels.atk_increase).max(1.0) + 0.1 * dmg_levels.def_increase_taken;
        raw_dmg = (raw_dmg as f64 * up_mod) as i64;
    }
    // 捨て身: 与ダメージ 3 倍 (代償として防御時 無防備 = 上の命中 100)。
    if has(atk_statuses, "捨て身") {
        raw_dmg *= 3;
    }
    // 攻撃力ＵＰ / ＤＯＷＮ (SetStatus / 特殊効果攻撃属性 低攻): 与ダメージ ×1.25 / ×0.75。
    // ダメージ増加系スペシャルパワー (熱血/魂 等) がかかっている場合はそちらが優先 (重複させない)。
    if has(atk_statuses, "攻撃力ＵＰ") && dmg_levels.atk_increase <= 0.0 {
        raw_dmg = (raw_dmg as f64 * 1.25) as i64;
    }
    if has(atk_statuses, "攻撃力ＤＯＷＮ") {
        raw_dmg = (raw_dmg as f64 * 0.75) as i64;
    }
    // 狂戦士 (特殊効果攻撃属性 狂): 与ダメージ ×1.25 (攻撃側)。
    if has(atk_statuses, "狂戦士") {
        raw_dmg = (raw_dmg as f64 * 1.25) as i64;
    }
    // 潜在力開放 (パイロット技能): 高気力 (130 以上) のとき与ダメージ ×1.25 (`Unit.cs::Damage`)。
    // 静的パイロット features を参照する (effective_combat_data が `..base.clone()` で保持)。
    if atk_morale >= 130
        && atk_pilot
            .features
            .iter()
            .any(|(f, _)| f.contains("潜在力開放"))
    {
        raw_dmg = (raw_dmg as f64 * 1.25) as i64;
    }
    // ブースト (ユニット特殊能力): 高気力 (130 以上) のとき与ダメージ ×1.25
    // (`UnitWeapon.cs` `Unit.IsFeatureAvailable("ブースト")`)。潜在力開放 とは独立した別係数で、
    // C# も別 if で双方を適用する (両方持てば ×1.5625)。攻撃側ユニットの静的 features を参照。
    // 注: ブースト 等のユニット特殊能力は terrain.txt の `全ユニット共通` 配下に各ユニットへ
    // 明示列挙されており (継承テンプレートではない)、パーサが features へ取り込む。
    if atk_morale >= 130
        && atk_unit
            .features
            .iter()
            .any(|(f, _)| f.contains("ブースト"))
    {
        raw_dmg = (raw_dmg as f64 * 1.25) as i64;
    }
    // 得意技 / 不得手 (パイロット技能): 技能データ (`得意技=格射` 等の武器 class 文字列) の
    // いずれかの文字が使用武器の class に含まれれば、与ダメージ ×1.2 / ×0.8 (`Unit.cs::Damage`)。
    let class_matches = |data: &str| {
        data.chars()
            .any(|c| !c.is_whitespace() && weapon.class.contains(c))
    };
    if atk_pilot
        .features
        .iter()
        .any(|(f, d)| f.contains("得意技") && class_matches(d))
    {
        raw_dmg = (raw_dmg as f64 * 1.2) as i64;
    }
    if atk_pilot
        .features
        .iter()
        .any(|(f, d)| f.contains("不得手") && class_matches(d))
    {
        raw_dmg = (raw_dmg as f64 * 0.8) as i64;
    }
    // ハンター (メインパイロット技能): 攻撃側の `ハンターLv*=別名 ターゲット…` 技能の
    // ターゲット (別名=先頭トークンを除く) が防御側のユニット名称 / クラス / サイズ
    // (「Lサイズ」形式) / パイロット名称 / 性別 のいずれかに一致すれば、与ダメージを
    // ×(1 + Lv×0.1) する (`Unit.cs::Damage`「ハンター能力」、攻撃に関する特殊能力.md)。
    {
        let def_size_label = format!("{}サイズ", def_unit.size.label());
        let def_sex = match def_pilot.sex {
            crate::data::pilot::Sex::Male => "男性",
            crate::data::pilot::Sex::Female => "女性",
            crate::data::pilot::Sex::Unspecified => "",
        };
        let matches_target = |tname: &str| {
            !tname.is_empty()
                && (tname == def_unit.name
                    || tname == def_unit.class
                    || tname == def_size_label
                    || tname == def_pilot.name
                    || (!def_sex.is_empty() && tname == def_sex))
        };
        for (fname, fdata) in &atk_pilot.features {
            let Some(rest) = fname.trim().strip_prefix("ハンター") else {
                continue;
            };
            // 別名 (先頭トークン) を除いたターゲット列にマッチするか。
            if fdata.split_whitespace().skip(1).any(matches_target) {
                // `ハンターLv<n>` の Lv を抽出 (無印 = Lv1、半角/全角を許容)。
                let lv: i64 = rest
                    .strip_prefix("Lv")
                    .or_else(|| rest.strip_prefix("LV"))
                    .or_else(|| rest.strip_prefix("lv"))
                    .or_else(|| rest.strip_prefix("Ｌｖ"))
                    .or_else(|| rest.strip_prefix("ＬＶ"))
                    .map(|a| {
                        a.chars()
                            .filter(char::is_ascii_digit)
                            .collect::<String>()
                            .parse()
                            .unwrap_or(1)
                    })
                    .unwrap_or(1);
                raw_dmg = ((raw_dmg * (10 + lv)) / 10).max(1);
                break;
            }
        }
    }
    // 行動不能の防御側 (睡眠=寝こみを襲う ×1.5、`Unit.cs::Damage`。本実装は麻痺/凍結 も同様)。
    if has(def_statuses, "麻痺") || has(def_statuses, "凍結") || has(def_statuses, "睡眠") {
        raw_dmg = (raw_dmg as f64 * 1.5) as i64;
    }
    // 精神コマンドによるダメージ低下 (down-mod): SRC `UnitWeapon.cs`。
    //   dmg_mod = 1 - 0.1*atk.SpecialPowerEffectLevel("ダメージ低下")        ← 攻撃側
    //               - 0.1*def.SpecialPowerEffectLevel("被ダメージ低下")      ← 防御側
    // 鉄壁 (被ダメージ低下Lv7.5→×0.25=÷4) / 不屈 (被ダメージ低下Lv9→×0.1) /
    // 無敵 (Lv10→×0) はこの経路で処理する (旧来の `鉄壁→/4`・`不屈→min(1)` ハードコードを置換)。
    // 倍率は最低ダメージ (max(10)) の **前** に適用する (C# も同順)。倍率が負なら 0 へクランプ。
    {
        let down_mod =
            (1.0 - 0.1 * dmg_levels.atk_decrease_dealt - 0.1 * dmg_levels.def_decrease_taken)
                .max(0.0);
        raw_dmg = (raw_dmg as f64 * down_mod) as i64;
    }
    // バリア: 攻撃側 `直撃` で無効化 (シールド防御の無効化)。`バリア中和` 状態
    // (特殊効果攻撃属性 中) の防御側はバリア / フィールドが無効化される。
    // 注: バリア は強度吸収型のシールド防御 (DefenseMode::Barrier) であり sp.txt の
    // 被ダメージ低下 効果ではないため、down-mod とは別経路 (÷2 近似) で扱う。
    if has(def_statuses, "バリア") && !has(atk_statuses, "直撃") && !has(def_statuses, "バリア中和")
    {
        raw_dmg /= 2;
    }

    // 最低ダメージは 10 (SRC `Unit.cls:7460-7474` / C# `UnitWeapon.cs:3567`)。
    // 全ての減算・減衰 (バリア/被ダメージ低下) の後に適用する。原典はオプション
    // 「ダメージ下限解除」で 0・「ダメージ下限１」で 1 へ下げられるが既定は 10
    // (両オプションは未モデル＝既定 10)。完全耐性 (防御特性で 100% カット) の場合は
    // 下限を適用しない (dmg_mod >= 100) が、その分岐は app.rs 側の無効化処理が担う。
    let damage = raw_dmg.max(10);

    let critical_chance = critical_probability(atk_pilot, def_pilot, weapon, def_statuses);

    CombatPreview {
        hit_chance,
        damage,
        critical_chance,
    }
}

/// クリティカル発生率 (基本値) を返す。SRC `Unit.cs::CriticalProbability`
/// および `戦闘システム詳細.md` 準拠:
///
///   (攻撃側の技量 − 防御側の技量) + 武器のＣＴ率修正
///
/// 防御側が行動不能 (麻痺 / 凍結 / 睡眠 / 石化 / 行動不能) の場合は +10。
/// 最終的に [1, 100] にクランプする (SRC は通常武器で最低 1%)。
///
/// 注: 底力 / 超底力 の命中・回避補正 (HP 1/4 以下) は `GameDatabase::combat_bonuses`
/// (命中/回避へ +30/+50) で反映済み。クリティカル率に対する 超反応 / 超能力 / 底力・超底力・
/// 覚悟 (+50) の補正は `App::crit_skill_bonus` で別途加算する (本関数では扱わない — `weapon.critical`
/// が特殊効果攻撃属性の発動率計算と共有のため、ここへ足すと proc 率へ漏れる)。バトルコンフィグは
/// 未対応。特殊効果武器の発動確率に対する耐性/弱点補正は `App::adjust_proc_for_resistance`
/// (`apply_weapon_special_effects` 内) で別途反映する。
pub fn critical_probability(
    atk_pilot: &PilotData,
    def_pilot: &PilotData,
    weapon: &WeaponData,
    def_statuses: &[String],
) -> i32 {
    let mut prob = weapon.critical + atk_pilot.technique - def_pilot.technique;
    // 相手が行動不能等の状態にある場合は +10。
    const DISABLED: [&str; 5] = ["行動不能", "石化", "凍結", "麻痺", "睡眠"];
    if def_statuses
        .iter()
        .any(|s| DISABLED.iter().any(|d| s.contains(d)))
    {
        prob += 10;
    }
    prob.clamp(1, 100)
}

/// 武器の `class` 文字列から特殊効果攻撃属性 (`特殊効果攻撃属性.md`) を抽出し、
/// 命中時に防御側へ付与する `(状態異常名, lifetime)` の列を返す。
///
/// 代表的な行動阻害・状態異常属性に対応 (Ｓ/縛/痺/眠/乱/凍/石/毒/不/止/劣/低防/低攻/低運/盲/撹/害/ゾ/黙/狂/中/踊)。
/// 位置移動 (吹/Ｋ/引/転) と クリティカル減衰 (衰/滅) は status 属性ではないため別関数
/// (`weapon_knockback` / `weapon_crit_reposition` / `weapon_crit_decay_levels`) で扱う。
/// `属性L<n>` でターン数を上書きできる。lifetime は「効果ターン数 + 1」を返す:
/// `begin_phase` が当該陣営フェイズ開始時に lifetime を 1 減らすため、相手の N
/// フェイズに効かせるには N+1 が必要 (L0 = 戦闘中のみ → 最小 lifetime 1)。
pub fn weapon_special_effects(class: &str) -> Vec<(String, i32)> {
    let mut out = Vec::new();
    for tok in class.split_whitespace() {
        let (attr, level) = split_attr_level(tok);
        // 弱<属性> / 効<属性>: 対象に指定属性への弱点 (proc/crit 率増) を 3 ターン付加。
        // 剋<属性>: 対象の指定属性を持つ武器・アビリティを 3 ターン使用不能にする。
        if let Some(el) = attr
            .strip_prefix('弱')
            .or_else(|| attr.strip_prefix('効'))
            .filter(|e| !e.is_empty())
        {
            out.push((format!("弱点:{el}"), level.unwrap_or(3) + 1));
            continue;
        }
        if let Some(el) = attr.strip_prefix('剋').filter(|e| !e.is_empty()) {
            out.push((format!("剋:{el}"), level.unwrap_or(3) + 1));
            continue;
        }
        let mapped: Option<(&str, i32)> = match attr.as_str() {
            "Ｓ" | "S" => Some(("行動不能", 1)),
            "縛" => Some(("捕縛", 2)),
            "痺" => Some(("麻痺", 3)),
            "眠" => Some(("睡眠", 3)),
            "乱" => Some(("混乱", 3)),
            "凍" => Some(("凍結", 3)),
            "石" => Some(("石化", 3)),
            "毒" => Some(("毒", 3)),
            "不" => Some(("行動不能", 1)),
            "止" => Some(("足止め", 1)),
            "劣" | "低防" => Some(("装甲低下", 3)),
            // 能力 DOWN 系 (3 ターン)。攻撃力 DOWN=与ダメ ×0.75 / 運動性 DOWN=命中回避 -15 /
            // 移動力 DOWN=移動力半減。
            "低攻" => Some(("攻撃力ＤＯＷＮ", 3)),
            "低運" => Some(("運動性ＤＯＷＮ", 3)),
            "低移" => Some(("移動力ＤＯＷＮ", 3)),
            // 命中率低下系。盲=盲目 (3T、攻撃側命中 ×0.5/被攻撃命中 ×1.5)、撹=撹乱 (2T、攻撃側命中 ×0.5)。
            "盲" => Some(("盲目", 3)),
            "撹" => Some(("撹乱", 2)),
            // 回復阻害系。害=回復不能 (特殊能力/地形による HP/EN 自然回復を阻害)、
            // ゾ=ゾンビ (アビリティ/精神による HP/EN 回復を阻害)。既定 3 ターン。
            "害" => Some(("回復不能", 3)),
            "ゾ" => Some(("ゾンビ", 3)),
            // 黙=沈黙 (3T): 術 / 音 属性の武器・アビリティを使用不能にする。
            "黙" => Some(("沈黙", 3)),
            // 狂=狂戦士 (3T): 与ダメージ ×1.25 / 被命中 ×1.5。AI 制御喪失部分 (味方の
            // 操作不能・暴走ターゲティング) は未モデルだが、戦闘修正と援護除外は反映。
            "狂" => Some(("狂戦士", 3)),
            // 中=バリア中和 (1T): 相手のバリア / フィールドを 1 ターン無効化する。
            "中" => Some(("バリア中和", 1)),
            // 踊=踊り (3T): 行動不能 (常時回避ニュアンスは未モデル)。
            "踊" => Some(("踊り", 3)),
            // 恐=恐怖 (3T): AI が敵から逃げ続ける (ai_act_unit の逃走分岐)。
            "恐" => Some(("恐怖", 3)),
            // 告=死の宣告: 期限切れ (次の自軍フェイズ) で HP が 1 になる。default_turns=0
            // → lifetime 1 (次の自軍フェイズで発動)。告L<n> で n フェイズ後。
            "告" => Some(("死の宣告", 0)),
            _ => None,
        };
        if let Some((name, default_turns)) = mapped {
            let turns = level.unwrap_or(default_turns);
            let lifetime = if turns <= 0 { 1 } else { turns + 1 };
            out.push((name.to_string(), lifetime));
        }
    }
    out
}

/// クリティカル時発動の減衰系属性 `衰L<n>`(HP) / `滅L<n>`(EN) を武器 class から抽出し、
/// `(衰レベル, 滅レベル)` を返す (`特殊効果攻撃属性.md`)。属性が無ければ `None`。
/// レベルは 1..=3 にクランプ (Lv1=現在値の3/4、Lv2=1/2、Lv3=1/4)。
pub fn weapon_crit_decay_levels(class: &str) -> (Option<i32>, Option<i32>) {
    let mut hp = None;
    let mut en = None;
    for tok in class.split_whitespace() {
        let (attr, level) = split_attr_level(tok);
        let lv = level.unwrap_or(1).clamp(1, 3);
        match attr.as_str() {
            "衰" => hp = Some(lv),
            "滅" => en = Some(lv),
            _ => {}
        }
    }
    (hp, en)
}

/// 減衰レベル (1..=3) に対応する「現在値に残す分子」(分母 4)。Lv1→3、Lv2→2、Lv3→1。
/// 例: 現在値 100 に Lv2 → `100 * 2 / 4 = 50`。
pub fn crit_decay_keep_numer(level: i32) -> i64 {
    i64::from((4 - level.clamp(1, 3)).max(1))
}

/// 吹き飛ばし系属性 `吹L<n>` / `ＫL<n>`(ノックバック) を武器 class から抽出し、
/// `(マス数, is_knockback)` を返す (`特殊効果攻撃属性.md`)。`is_knockback=true` は
/// Ｋ 属性 (攻撃側サイズが標的より 2 段階以上小さいと不発のサイズ制限あり)。
/// 該当属性が無ければ `None`。レベル省略時は 1 マス。
pub fn weapon_knockback(class: &str) -> Option<(i32, bool)> {
    for tok in class.split_whitespace() {
        let (attr, level) = split_attr_level(tok);
        match attr.as_str() {
            "吹" => return Some((level.unwrap_or(1).max(1), false)),
            "Ｋ" | "K" => return Some((level.unwrap_or(1).max(1), true)),
            _ => {}
        }
    }
    None
}

/// 気力減少属性 `脱` / `Ｄ`(気力吸収) を武器 class から抽出し、低下量を返す
/// (`特殊効果攻撃属性.md`)。低下量は `5×レベル` (レベル省略時 10)。該当が無ければ `None`。
/// `Ｄ` の「吸収 (低下分の半分を攻撃側へ)」は [`weapon_morale_absorbs`] で判定する。
pub fn weapon_morale_reduction(class: &str) -> Option<i32> {
    for tok in class.split_whitespace() {
        let (attr, level) = split_attr_level(tok);
        match attr.as_str() {
            "脱" | "Ｄ" | "D" => return Some(level.map(|l| 5 * l.max(1)).unwrap_or(10)),
            _ => {}
        }
    }
    None
}

/// 武器が気力*吸収*属性 (`Ｄ`/`D`) を持つか。`脱` は低下のみ、`Ｄ` は低下分の半分を
/// 攻撃側の気力へ移す (`特殊効果攻撃属性.md`)。
pub fn weapon_morale_absorbs(class: &str) -> bool {
    class.split_whitespace().any(|tok| {
        let (attr, _) = split_attr_level(tok);
        attr == "Ｄ" || attr == "D"
    })
}

/// 支配系属性 `憑`(憑依) / `魅`(魅了) を武器 class から抽出する (`特殊効果攻撃属性.md`
/// 69-75 魅 / 113-117 憑)。`憑`→`"憑依"`(相手を乗っ取り恒久支配)、`魅`→`"魅了"`(3 ターン、
/// 魅了主を護衛する味方ユニットとして行動)。いずれの属性も持たなければ `None`。
/// どちらも `BossRank` 適用ユニットには無効だが、その判定は勢力切替を伴うため
/// 呼び出し側 (`App::apply_weapon_special_effects`) で行う。class トークンの先頭一致を採る。
pub fn weapon_possession(class: &str) -> Option<&'static str> {
    for tok in class.split_whitespace() {
        let (attr, _) = split_attr_level(tok);
        match attr.as_str() {
            "憑" => return Some("憑依"),
            "魅" => return Some("魅了"),
            _ => {}
        }
    }
    None
}

/// クリティカル時の位置移動属性を武器 class から抽出する (`特殊効果攻撃属性.md`)。
/// 返り値 `(引き寄せ有無, 強制転移距離)`。`引`=攻撃側に隣接させる、`転L<n>`=ランダムに
/// `n` 距離テレポート (レベル省略は 1)。いずれも無ければ `(false, None)`。
pub fn weapon_crit_reposition(class: &str) -> (bool, Option<i32>) {
    let mut pull = false;
    let mut teleport = None;
    for tok in class.split_whitespace() {
        let (attr, level) = split_attr_level(tok);
        match attr.as_str() {
            "引" => pull = true,
            "転" => teleport = Some(level.unwrap_or(1).max(1)),
            _ => {}
        }
    }
    (pull, teleport)
}

/// `"痺L3"` → `("痺", Some(3))`、`"縛"` → `("縛", None)`。`L`/`Ｌ` で区切る。
fn split_attr_level(tok: &str) -> (String, Option<i32>) {
    let chars: Vec<char> = tok.chars().collect();
    if let Some(pos) = chars.iter().position(|&c| c == 'L' || c == 'Ｌ') {
        let attr: String = chars[..pos].iter().collect();
        let level: String = chars[pos + 1..].iter().collect();
        (attr, level.parse::<i32>().ok())
    } else {
        (tok.to_string(), None)
    }
}

/// `predict_with_status` に防御モードを適用した版.
#[allow(clippy::too_many_arguments)]
pub fn predict_with_defense(
    atk_pilot: &PilotData,
    atk_unit: &UnitData,
    weapon: &WeaponData,
    def_pilot: &PilotData,
    def_unit: &UnitData,
    def_terrain_hit_mod: i32,
    def_terrain_damage_mod: i32,
    atk_morale: i32,
    def_morale: i32,
    atk_statuses: &[String],
    def_statuses: &[String],
    defense_mode: DefenseMode,
) -> CombatPreview {
    let base = predict_with_status(
        atk_pilot,
        atk_unit,
        weapon,
        def_pilot,
        def_unit,
        def_terrain_hit_mod,
        def_terrain_damage_mod,
        atk_morale,
        def_morale,
        atk_statuses,
        def_statuses,
    );

    match defense_mode {
        DefenseMode::Normal => base,
        DefenseMode::Dodge => {
            let dodge_penalty = def_pilot.dodge / 5;
            CombatPreview {
                hit_chance: base.hit_chance.saturating_sub(dodge_penalty),
                damage: base.damage,
                critical_chance: base.critical_chance,
            }
        }
        DefenseMode::Defend => CombatPreview {
            hit_chance: base.hit_chance,
            damage: base.damage / 2,
            critical_chance: base.critical_chance,
        },
        DefenseMode::Barrier { strength } => {
            let absorbed = strength.min(base.damage);
            let remaining = base.damage - absorbed;
            CombatPreview {
                hit_chance: base.hit_chance,
                damage: remaining,
                critical_chance: base.critical_chance,
            }
        }
        DefenseMode::Shield { chance } => {
            let expected_damage = base.damage * (100 - chance) as i64 / 100;
            CombatPreview {
                hit_chance: base.hit_chance,
                damage: expected_damage,
                critical_chance: base.critical_chance,
            }
        }
    }
}

/// Map attack shape / 範囲攻撃の形状.
///
/// SRC weapons can have map attack types specified in their class or name field.
/// The shape determines which units are hit around the target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MapAttackShape {
    /// 単体攻撃 (single target) — only the target is hit.
    Single,
    /// Ｍ全 (all) — hits all units in the weapon's range.
    All,
    /// Ｍ投L<n> (throw) — hits units within <n> cells of the target.
    Throw { radius: u32 },
    /// Ｍ直 (straight) — hits units in a line from attacker through target.
    Straight,
    /// Ｍ拡 (spread) — hits target and adjacent cells (cross pattern).
    Spread,
    /// Ｍ扇 (fan) — hits扇形 (fan-shaped) area from attacker.
    Fan,
    /// Ｍ線 (line) — hits all units on the line between two points.
    Line,
}

impl MapAttackShape {
    /// Parse map attack shape from a weapon's class or name field.
    /// SRC uses patterns like "Ｍ全", "Ｍ投L3", "Ｍ直", "Ｍ拡", "Ｍ扇", "Ｍ線".
    pub fn from_weapon_class(class: &str) -> Self {
        if class.contains("Ｍ全") {
            Self::All
        } else if let Some(rest) = class.strip_prefix("Ｍ投L") {
            if let Ok(n) = rest.parse::<u32>() {
                Self::Throw { radius: n }
            } else {
                Self::Throw { radius: 1 }
            }
        } else if class.contains("Ｍ直") {
            Self::Straight
        } else if class.contains("Ｍ拡") {
            Self::Spread
        } else if class.contains("Ｍ扇") {
            Self::Fan
        } else if class.contains("Ｍ線") {
            Self::Line
        } else {
            Self::Single
        }
    }

    /// Determine which cells are hit by this map attack.
    ///
    /// `attacker_pos`: (x, y) of the attacker
    /// `target_pos`: (x, y) of the primary target
    /// `max_range`: weapon's max range
    ///
    /// Returns a list of (x, y) cells that are hit.
    pub fn affected_cells(
        &self,
        attacker_pos: (u32, u32),
        target_pos: (u32, u32),
        max_range: i32,
    ) -> Vec<(u32, u32)> {
        match self {
            Self::Single => vec![target_pos],
            Self::All => {
                // All units within weapon range of attacker
                let range = max_range as u32;
                let mut cells = Vec::new();
                for dx in 0..=range {
                    for dy in 0..=range {
                        if dx + dy <= range {
                            cells.push((
                                attacker_pos.0.wrapping_add(dx),
                                attacker_pos.1.wrapping_add(dy),
                            ));
                            if dx > 0 {
                                cells.push((
                                    attacker_pos.0.wrapping_sub(dx),
                                    attacker_pos.1.wrapping_add(dy),
                                ));
                            }
                            if dy > 0 {
                                cells.push((
                                    attacker_pos.0.wrapping_add(dx),
                                    attacker_pos.1.wrapping_sub(dy),
                                ));
                            }
                            if dx > 0 && dy > 0 {
                                cells.push((
                                    attacker_pos.0.wrapping_sub(dx),
                                    attacker_pos.1.wrapping_sub(dy),
                                ));
                            }
                        }
                    }
                }
                cells
            }
            Self::Throw { radius } => {
                // All units within radius of target
                let mut cells = Vec::new();
                for dx in 0..=*radius {
                    for dy in 0..=*radius {
                        if dx + dy <= *radius {
                            cells.push((
                                target_pos.0.wrapping_add(dx),
                                target_pos.1.wrapping_add(dy),
                            ));
                            if dx > 0 {
                                cells.push((
                                    target_pos.0.wrapping_sub(dx),
                                    target_pos.1.wrapping_add(dy),
                                ));
                            }
                            if dy > 0 {
                                cells.push((
                                    target_pos.0.wrapping_add(dx),
                                    target_pos.1.wrapping_sub(dy),
                                ));
                            }
                            if dx > 0 && dy > 0 {
                                cells.push((
                                    target_pos.0.wrapping_sub(dx),
                                    target_pos.1.wrapping_sub(dy),
                                ));
                            }
                        }
                    }
                }
                cells
            }
            Self::Spread => {
                // Target + 4 adjacent cells (cross pattern)
                let (tx, ty) = target_pos;
                vec![
                    target_pos,
                    (tx.wrapping_add(1), ty),
                    (tx.wrapping_sub(1), ty),
                    (tx, ty.wrapping_add(1)),
                    (tx, ty.wrapping_sub(1)),
                ]
            }
            Self::Straight => {
                // Line from attacker through target, extending to max_range
                line_cells(attacker_pos, target_pos, max_range as u32)
            }
            Self::Fan => {
                // Fan-shaped area from attacker toward target
                fan_cells(attacker_pos, target_pos, max_range as u32)
            }
            Self::Line => {
                // Line between attacker and target
                line_cells(attacker_pos, target_pos, max_range as u32)
            }
        }
    }
}

/// Get cells on a line from start through end, extending to max_length.
fn line_cells(start: (u32, u32), end: (u32, u32), max_length: u32) -> Vec<(u32, u32)> {
    let dx = end.0 as i32 - start.0 as i32;
    let dy = end.1 as i32 - start.1 as i32;
    let dist = dx.abs().max(dy.abs()).max(1) as u32;
    let steps = dist.min(max_length);

    let mut cells = Vec::new();
    for i in 0..=steps {
        let x = (start.0 as i32 + dx * i as i32 / dist as i32) as u32;
        let y = (start.1 as i32 + dy * i as i32 / dist as i32) as u32;
        cells.push((x, y));
    }
    cells
}

/// Get fan-shaped cells from attacker toward target.
fn fan_cells(_attacker: (u32, u32), target: (u32, u32), _max_range: u32) -> Vec<(u32, u32)> {
    // Simplified fan: target + cells adjacent to target toward attacker
    let mut cells = vec![target];
    let (tx, ty) = target;

    let adjacent = [
        (tx.wrapping_add(1), ty),
        (tx.wrapping_sub(1), ty),
        (tx, ty.wrapping_add(1)),
        (tx, ty.wrapping_sub(1)),
    ];
    cells.extend(adjacent);

    cells
}

/// Check if a unit at `unit_pos` is hit by a map attack targeting `target_pos`.
pub fn is_unit_hit_by_map_attack(
    shape: MapAttackShape,
    attacker_pos: (u32, u32),
    target_pos: (u32, u32),
    unit_pos: (u32, u32),
    max_range: i32,
) -> bool {
    let affected = shape.affected_cells(attacker_pos, target_pos, max_range);
    affected.contains(&unit_pos)
}

/// `attacker_unit` の武器のうち射程内のもののうち、最もダメージ期待値が高い
/// (= power が大きい) ものを返す。
pub fn best_weapon_in_range(unit: &UnitData, distance: u32) -> Option<&WeaponData> {
    unit.weapons
        .iter()
        .filter(|w| weapon_in_range(w, distance))
        .filter(|w| !is_charge_weapon(w))
        .max_by_key(|w| w.power)
}

/// `best_weapon_in_range` の charge 対応版: `charged=true` ならチャージ攻撃
/// (`Ｃ` 属性) も候補に含める。原典: `Chargeコマンド` で charged フラグを
/// 立ててから使うチャージ攻撃武器の解禁判定。
pub fn best_weapon_in_range_with_charge(
    unit: &UnitData,
    distance: u32,
    charged: bool,
) -> Option<&WeaponData> {
    unit.weapons
        .iter()
        .filter(|w| weapon_in_range(w, distance))
        .filter(|w| !is_charge_weapon(w) || charged)
        .max_by_key(|w| w.power)
}

/// `WeaponData.class` に `Ｃ` (全角) または `C` (半角) 属性が含まれるかを判定。
/// SRC.NET 仕様: チャージ攻撃武器は `Charge` コマンド後にしか使えない。
pub fn is_charge_weapon(w: &WeaponData) -> bool {
    w.class.contains('Ｃ') || w.class.contains('C')
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::pilot::{Adaption, PilotData, Sex};
    use crate::data::unit::{Size, UnitData, WeaponData};

    #[test]
    fn weapon_special_effects_parses_attrs_and_levels() {
        // 単一属性 (既定ターン数 + 1 が lifetime)。
        assert_eq!(weapon_special_effects("痺"), vec![("麻痺".to_string(), 4)]);
        assert_eq!(weapon_special_effects("縛"), vec![("捕縛".to_string(), 3)]);
        assert_eq!(
            weapon_special_effects("Ｓ"),
            vec![("行動不能".to_string(), 2)]
        );
        // レベル指定でターン数を上書き (痺L1 → 1ターン → lifetime 2)。
        assert_eq!(
            weapon_special_effects("痺L1"),
            vec![("麻痺".to_string(), 2)]
        );
        // 他属性と混在しても CC 属性のみ抽出。
        assert_eq!(
            weapon_special_effects("実 Ｐ 凍"),
            vec![("凍結".to_string(), 4)]
        );
        // 該当属性なし。
        assert!(weapon_special_effects("実 格").is_empty());
        // 複数の特殊効果属性。
        assert_eq!(
            weapon_special_effects("毒 劣"),
            vec![("毒".to_string(), 4), ("装甲低下".to_string(), 4)]
        );
        // 能力 DOWN 系 (低攻=攻撃力ＤＯＷＮ / 低運=運動性ＤＯＷＮ、3 ターン)。
        assert_eq!(
            weapon_special_effects("低攻"),
            vec![("攻撃力ＤＯＷＮ".to_string(), 4)]
        );
        assert_eq!(
            weapon_special_effects("低運"),
            vec![("運動性ＤＯＷＮ".to_string(), 4)]
        );
        assert_eq!(
            weapon_special_effects("低移"),
            vec![("移動力ＤＯＷＮ".to_string(), 4)]
        );
        // 命中率低下系 (盲=盲目3T / 撹=撹乱2T)。
        assert_eq!(weapon_special_effects("盲"), vec![("盲目".to_string(), 4)]);
        assert_eq!(weapon_special_effects("撹"), vec![("撹乱".to_string(), 3)]);
        // 回復阻害系 (害=回復不能 / ゾ=ゾンビ、各 3T)。
        assert_eq!(
            weapon_special_effects("害"),
            vec![("回復不能".to_string(), 4)]
        );
        assert_eq!(
            weapon_special_effects("ゾ"),
            vec![("ゾンビ".to_string(), 4)]
        );
        // 沈黙系 (黙=沈黙3T)。
        assert_eq!(weapon_special_effects("黙"), vec![("沈黙".to_string(), 4)]);
        // 狂戦士 (狂=狂戦士3T)。
        assert_eq!(
            weapon_special_effects("狂"),
            vec![("狂戦士".to_string(), 4)]
        );
        // バリア中和 (中=バリア中和1T)。
        assert_eq!(
            weapon_special_effects("中"),
            vec![("バリア中和".to_string(), 2)]
        );
        // 踊り (踊=踊り3T)。
        assert_eq!(weapon_special_effects("踊"), vec![("踊り".to_string(), 4)]);
        // 恐怖 (恐=恐怖3T)。
        assert_eq!(weapon_special_effects("恐"), vec![("恐怖".to_string(), 4)]);
        // 死の宣告 (告=死の宣告、default lifetime 1 / 告L2 → 3)。
        assert_eq!(
            weapon_special_effects("告"),
            vec![("死の宣告".to_string(), 1)]
        );
        assert_eq!(
            weapon_special_effects("告L2"),
            vec![("死の宣告".to_string(), 3)]
        );
        // 弱/効 (弱点付加) と 剋 (属性封じ): 属性名を抽出して condition 名に展開。
        assert_eq!(
            weapon_special_effects("弱火"),
            vec![("弱点:火".to_string(), 4)]
        );
        assert_eq!(
            weapon_special_effects("効光L2"),
            vec![("弱点:光".to_string(), 3)]
        );
        assert_eq!(
            weapon_special_effects("剋火"),
            vec![("剋:火".to_string(), 4)]
        );
    }

    /// バリア中和 (中) の防御側はバリアによるダメージ半減が無効化される。
    #[test]
    fn barrier_neutralize_disables_barrier() {
        let with_barrier = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(800, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &["バリア".to_string()],
        )
        .damage;
        let neutralized = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(800, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &["バリア".to_string(), "バリア中和".to_string()],
        )
        .damage;
        assert_eq!(
            neutralized,
            with_barrier * 2,
            "バリア中和でバリアの半減が無効 (ダメージ 2 倍)"
        );
    }

    /// 衰 / 滅 (クリティカル時の HP / EN 減衰) のレベル抽出と分子計算。
    #[test]
    fn weapon_crit_decay_parses_and_keeps_fraction() {
        // 衰L2 → HP レベル 2、滅 (レベル省略 → 1)。
        assert_eq!(weapon_crit_decay_levels("衰L2"), (Some(2), None));
        assert_eq!(weapon_crit_decay_levels("滅"), (None, Some(1)));
        assert_eq!(weapon_crit_decay_levels("実 衰L3 滅L1"), (Some(3), Some(1)));
        assert_eq!(weapon_crit_decay_levels("格 射"), (None, None));
        // 分子 (分母4): Lv1→3 (3/4)、Lv2→2 (1/2)、Lv3→1 (1/4)。
        assert_eq!(crit_decay_keep_numer(1), 3);
        assert_eq!(crit_decay_keep_numer(2), 2);
        assert_eq!(crit_decay_keep_numer(3), 1);
    }

    /// 吹き飛ばし / ノックバック属性のレベル抽出。
    #[test]
    fn weapon_knockback_parses_levels() {
        assert_eq!(weapon_knockback("吹L2"), Some((2, false)));
        assert_eq!(weapon_knockback("Ｋ"), Some((1, true)));
        assert_eq!(weapon_knockback("実 吹L3"), Some((3, false)));
        assert_eq!(weapon_knockback("格 射"), None);
    }

    /// 気力減少属性 (脱 / Ｄ) の低下量抽出。
    #[test]
    fn weapon_morale_reduction_parses() {
        assert_eq!(weapon_morale_reduction("脱"), Some(10)); // 省略時 10
        assert_eq!(weapon_morale_reduction("脱L3"), Some(15)); // 5×3
        assert_eq!(weapon_morale_reduction("Ｄ"), Some(10));
        assert_eq!(weapon_morale_reduction("格 射"), None);
    }

    /// 引き寄せ / 強制転移 属性の抽出。
    #[test]
    fn weapon_crit_reposition_parses() {
        assert_eq!(weapon_crit_reposition("引"), (true, None));
        assert_eq!(weapon_crit_reposition("転L3"), (false, Some(3)));
        assert_eq!(weapon_crit_reposition("実 引 転L2"), (true, Some(2)));
        assert_eq!(weapon_crit_reposition("格"), (false, None));
    }

    /// 狂戦士 (狂): 攻撃側で与ダメージ ×1.25、防御側で被命中 ×1.5。
    #[test]
    fn status_kyousenshi_modifies_damage_and_hit() {
        let dmg_base = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(800, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        )
        .damage;
        let dmg_berserk = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(800, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &["狂戦士".to_string()],
            &[],
        )
        .damage;
        assert_eq!(
            dmg_berserk,
            (dmg_base as f64 * 1.25) as i64,
            "狂戦士 (攻撃側) で与ダメージ ×1.25"
        );
        // 被命中 ×1.5。
        let hit_base = predict_with_status(
            &p(30, 0, 0),
            &u(0, vec![]),
            &weapon(0, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        )
        .hit_chance;
        let hit_vs_berserk = predict_with_status(
            &p(30, 0, 0),
            &u(0, vec![]),
            &weapon(0, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &["狂戦士".to_string()],
        )
        .hit_chance;
        assert_eq!(
            hit_vs_berserk,
            (hit_base * 3 / 2).min(95),
            "狂戦士 (防御側) で被命中 ×1.5"
        );
    }

    /// 攻撃力ＤＯＷＮ 状態は与ダメージを ×0.75 に、攻撃力ＵＰ は ×1.25 にする。
    #[test]
    fn attack_power_status_scales_damage() {
        let base = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(800, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        )
        .damage;
        let down = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(800, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &["攻撃力ＤＯＷＮ".to_string()],
            &[],
        )
        .damage;
        let up = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(800, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &["攻撃力ＵＰ".to_string()],
            &[],
        )
        .damage;
        assert_eq!(down, (base as f64 * 0.75) as i64, "攻撃力ＤＯＷＮ で ×0.75");
        assert_eq!(up, (base as f64 * 1.25) as i64, "攻撃力ＵＰ で ×1.25");
    }

    fn weapon(power: i64, min: i32, max: i32, prec: i32) -> WeaponData {
        WeaponData {
            name: "W".into(),
            power,
            min_range: min,
            max_range: max,
            precision: prec,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: String::new(),
            extras: Vec::new(),
        }
    }

    fn p(hit: i32, dodge: i32, shooting: i32) -> PilotData {
        PilotData {
            spirit_commands: Vec::new(),
            name: "P".into(),
            nickname: "P".into(),
            kana_name: "P".into(),
            sex: Sex::Unspecified,
            class: String::new(),
            adaption: Adaption::parse("AAAA").unwrap(),
            exp_value: 0,
            infight: shooting,
            shooting,
            hit,
            dodge,
            intuition: 0,
            technique: 0,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: None,
            features: Vec::new(),
        }
    }

    fn u(armor: i64, weapons: Vec<WeaponData>) -> UnitData {
        UnitData {
            abilities: Vec::new(),
            name: "U".into(),
            kana_name: "U".into(),
            nickname: "U".into(),
            class: String::new(),
            pilot_num: 1,
            item_num: 0,
            transportation: "陸".into(),
            speed: 0,
            size: Size::M,
            value: 0,
            exp_value: 0,
            hp: 0,
            en: 0,
            armor,
            mobility: 0,
            adaption: Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons,
            features: Vec::new(),
        }
    }

    #[test]
    fn weapon_in_range_basic() {
        let w = weapon(0, 2, 5, 0);
        assert!(!weapon_in_range(&w, 1));
        assert!(weapon_in_range(&w, 2));
        assert!(weapon_in_range(&w, 5));
        assert!(!weapon_in_range(&w, 6));
    }

    #[test]
    fn best_weapon_picks_strongest_in_range() {
        let unit = u(
            0,
            vec![
                weapon(100, 1, 1, 5),
                weapon(500, 2, 5, 10),
                weapon(800, 3, 7, 15),
            ],
        );
        let dist2 = best_weapon_in_range(&unit, 2).unwrap();
        assert_eq!(dist2.power, 500);
        let dist3 = best_weapon_in_range(&unit, 3).unwrap();
        assert_eq!(dist3.power, 800);
        assert!(best_weapon_in_range(&unit, 99).is_none());
    }

    #[test]
    fn higher_power_higher_damage() {
        let lo = predict(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(100, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
        );
        let hi = predict(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
        );
        assert!(hi.damage > lo.damage);
    }

    /// 潜在力開放 (パイロット技能): 気力 130 以上で与ダメージ ×1.25。130 未満では非発動。
    #[test]
    fn potential_release_boosts_damage_at_high_morale() {
        let mut esp = p(0, 0, 100);
        esp.features.push(("潜在力開放".into(), String::new()));
        let plain = p(0, 0, 100);
        let w = weapon(500, 1, 1, 0);
        let dp = p(0, 0, 0);
        // 気力 130: 潜在力開放 持ちは plain の ×1.25。
        let with_skill = predict_with_status(
            &esp,
            &u(0, vec![]),
            &w,
            &dp,
            &u(0, vec![]),
            0,
            0,
            130,
            100,
            &[],
            &[],
        );
        let without = predict_with_status(
            &plain,
            &u(0, vec![]),
            &w,
            &dp,
            &u(0, vec![]),
            0,
            0,
            130,
            100,
            &[],
            &[],
        );
        assert_eq!(
            with_skill.damage,
            (without.damage as f64 * 1.25) as i64,
            "潜在力開放: 気力 130 で与ダメージ ×1.25"
        );
        // 気力 129 では非発動 (技能持ちでも plain と同じ)。
        let below = predict_with_status(
            &esp,
            &u(0, vec![]),
            &w,
            &dp,
            &u(0, vec![]),
            0,
            0,
            129,
            100,
            &[],
            &[],
        );
        let below_plain = predict_with_status(
            &plain,
            &u(0, vec![]),
            &w,
            &dp,
            &u(0, vec![]),
            0,
            0,
            129,
            100,
            &[],
            &[],
        );
        assert_eq!(
            below.damage, below_plain.damage,
            "気力 130 未満では潜在力開放は非発動"
        );
    }

    /// ブースト (攻撃側ユニット特殊能力): 気力 130 以上で与ダメージ ×1.25 (潜在力開放 と独立)。
    #[test]
    fn boost_feature_boosts_damage_at_high_morale() {
        let ap = p(0, 0, 100);
        let dp = p(0, 0, 0);
        let w = weapon(500, 1, 1, 0);
        let mut boosted = u(0, vec![]);
        boosted
            .features
            .push(("ブースト".into(), "マジンパワー".into()));
        let plain = u(0, vec![]);
        // 気力 130 + ブースト → plain の ×1.25。
        let with_boost = predict_with_status(
            &ap,
            &boosted,
            &w,
            &dp,
            &u(0, vec![]),
            0,
            0,
            130,
            100,
            &[],
            &[],
        );
        let without = predict_with_status(
            &ap,
            &plain,
            &w,
            &dp,
            &u(0, vec![]),
            0,
            0,
            130,
            100,
            &[],
            &[],
        );
        assert_eq!(
            with_boost.damage,
            (without.damage as f64 * 1.25) as i64,
            "ブースト: 気力 130 で与ダメージ ×1.25"
        );
        // 気力 129 では非発動。
        let below = predict_with_status(
            &ap,
            &boosted,
            &w,
            &dp,
            &u(0, vec![]),
            0,
            0,
            129,
            100,
            &[],
            &[],
        );
        let below_plain = predict_with_status(
            &ap,
            &plain,
            &w,
            &dp,
            &u(0, vec![]),
            0,
            0,
            129,
            100,
            &[],
            &[],
        );
        assert_eq!(
            below.damage, below_plain.damage,
            "気力 130 未満ではブースト非発動"
        );
    }

    /// 行動不能の防御側 (睡眠/石化/行動不能) は命中 100、睡眠は与ダメージ ×1.5。
    #[test]
    fn disabled_defender_is_always_hit_and_sleep_takes_extra_damage() {
        let ap = p(0, 0, 100);
        let au = u(0, vec![]);
        let w = weapon(500, 1, 1, 0);
        // 高回避の防御側でも、睡眠/石化/行動不能 なら命中 100。
        let dodgy = p(0, 200, 0);
        let du = u(0, vec![]);
        for st in ["睡眠", "石化", "行動不能"] {
            let pred = predict_with_status(
                &ap,
                &au,
                &w,
                &dodgy,
                &du,
                0,
                0,
                100,
                100,
                &[],
                &[st.to_string()],
            );
            assert_eq!(pred.hit_chance, 100, "{st} の防御側は命中 100");
        }
        // 睡眠 の防御側は与ダメージ ×1.5。
        let normal = predict_with_status(&ap, &au, &w, &p(0, 0, 0), &du, 0, 0, 100, 100, &[], &[]);
        let asleep = predict_with_status(
            &ap,
            &au,
            &w,
            &p(0, 0, 0),
            &du,
            0,
            0,
            100,
            100,
            &[],
            &["睡眠".to_string()],
        );
        assert_eq!(
            asleep.damage,
            (normal.damage as f64 * 1.5) as i64,
            "睡眠 の防御側は与ダメージ ×1.5"
        );
    }

    /// 得意技 (×1.2) / 不得手 (×0.8): 武器 class に技能データの文字が含まれるときのみ発動。
    #[test]
    fn specialty_weapon_scales_damage() {
        let mut w_melee = weapon(500, 1, 1, 0);
        w_melee.class = "格".into();
        let mut w_shoot = weapon(500, 1, 1, 0);
        w_shoot.class = "射".into();
        let dp = p(0, 0, 0);
        let du = u(0, vec![]);
        let base = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &w_melee,
            &dp,
            &du,
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        // 得意技=格: 格闘武器で ×1.2。
        let mut good = p(0, 0, 100);
        good.features.push(("得意技".into(), "格".into()));
        let good_melee = predict_with_status(
            &good,
            &u(0, vec![]),
            &w_melee,
            &dp,
            &du,
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        assert_eq!(
            good_melee.damage,
            (base.damage as f64 * 1.2) as i64,
            "得意技=格: 格闘武器で ×1.2"
        );
        // 得意技=格 でも射撃武器 (class 射) には非適用。
        let good_shoot = predict_with_status(
            &good,
            &u(0, vec![]),
            &w_shoot,
            &dp,
            &du,
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        assert_eq!(
            good_shoot.damage, base.damage,
            "得意技=格 は射撃武器には効かない"
        );
        // 不得手=格: 格闘武器で ×0.8。
        let mut bad = p(0, 0, 100);
        bad.features.push(("不得手".into(), "格".into()));
        let bad_melee = predict_with_status(
            &bad,
            &u(0, vec![]),
            &w_melee,
            &dp,
            &du,
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        assert_eq!(
            bad_melee.damage,
            (base.damage as f64 * 0.8) as i64,
            "不得手=格: 格闘武器で ×0.8"
        );
    }

    /// ハンター (パイロット技能): 指定ターゲット (ユニット名/クラス/サイズ/パイロット名/性別)
    /// に一致する相手への与ダメージが Lv×10% 増加。別名 (先頭トークン) はターゲットにしない。
    #[test]
    fn hunter_skill_scales_damage_vs_listed_targets() {
        let w = weapon(500, 1, 1, 0);
        let dp = p(0, 0, 0); // 防御側パイロット "P"
        let mut du = u(0, vec![]); // 防御側ユニット: name "U" / size M
        du.class = "ドラゴン".into();
        let ap = p(0, 0, 100);
        let base = predict_with_status(&ap, &u(0, vec![]), &w, &dp, &du, 0, 0, 100, 100, &[], &[]);

        // ハンターLv3=竜狩り(別名) ドラゴン → クラス一致で ×1.3。
        let mut hunter = p(0, 0, 100);
        hunter
            .features
            .push(("ハンターLv3".into(), "竜狩り ドラゴン".into()));
        let hit = predict_with_status(
            &hunter,
            &u(0, vec![]),
            &w,
            &dp,
            &du,
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        assert_eq!(
            hit.damage,
            (base.damage * 13 / 10).max(1),
            "ハンターLv3: 対象クラスへ ×1.3"
        );

        // 別名 (先頭トークン=竜狩り) はターゲットにしない: クラス "竜狩り" の相手には非適用。
        let mut du_alias = u(0, vec![]);
        du_alias.class = "竜狩り".into();
        let miss = predict_with_status(
            &hunter,
            &u(0, vec![]),
            &w,
            &dp,
            &du_alias,
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        assert_eq!(
            miss.damage, base.damage,
            "ハンター: 別名 (先頭トークン) はターゲットにしない"
        );

        // サイズ指定 (Mサイズ) でも一致する。無印 = Lv1 → ×1.1。
        let mut hunter_size = p(0, 0, 100);
        hunter_size
            .features
            .push(("ハンター".into(), "サイズ狩り Mサイズ".into()));
        let hit_size = predict_with_status(
            &hunter_size,
            &u(0, vec![]),
            &w,
            &dp,
            &du,
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        assert_eq!(
            hit_size.damage,
            (base.damage * 11 / 10).max(1),
            "ハンター無印 (Lv1): Mサイズへ ×1.1"
        );
    }

    /// 耐久 (防御側パイロット技能): 装甲を Pilot.Defense/100 倍する。
    /// 既定オプション下の Defense = 100 + 5 * Lv (`Pilot.cls:402`)。
    /// 耐久 持ちの防御側は def_power が増えるため被ダメージが減る。
    #[test]
    fn endurance_skill_raises_defense_and_reduces_damage() {
        // 攻撃力 1500 / 装甲 1000 / 気力・地形適応中立 → 被ダメージ = 1500 - def_power。
        let w = weapon(1500, 1, 1, 0);
        let ap = p(0, 0, 100);
        let au = u(0, vec![]);

        // 耐久なし: Defense=100 → def_power = 1000 → ダメージ = 500。
        let plain = p(0, 0, 0);
        let no_skill = predict_with_status(
            &ap,
            &au,
            &w,
            &plain,
            &u(1000, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        assert_eq!(
            no_skill.damage, 500,
            "耐久なし: def_power=1000 → ダメージ500"
        );

        // 耐久Lv2: Defense = 100 + 5*2 = 110 → def_power = 1000×110/100 = 1100
        //   → ダメージ = 1500 - 1100 = 400。
        let mut tough = p(0, 0, 0);
        tough.features.push(("耐久Lv2".into(), String::new()));
        let with_skill = predict_with_status(
            &ap,
            &au,
            &w,
            &tough,
            &u(1000, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        assert_eq!(
            with_skill.damage, 400,
            "耐久Lv2: Defense=110 → def_power=1100 → ダメージ400"
        );
        assert!(
            with_skill.damage < no_skill.damage,
            "耐久 持ちの防御側は被ダメージが減る"
        );

        // 耐久無印 (Lv1): Defense = 105 → def_power = 1050 → ダメージ = 450。
        let mut tough1 = p(0, 0, 0);
        tough1.features.push(("耐久".into(), String::new()));
        let with_lv1 = predict_with_status(
            &ap,
            &au,
            &w,
            &tough1,
            &u(1000, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        assert_eq!(
            with_lv1.damage, 450,
            "耐久無印 (Lv1): Defense=105 → def_power=1050 → ダメージ450"
        );
    }

    #[test]
    fn adaptation_mult_table_matches_src() {
        // SRC 戦闘システム詳細.md: S=1.4 A=1.2 B=1.0 C=0.8 D=0.6 -=0。
        assert_eq!(adaptation_mult(b'S'), 1.4);
        assert_eq!(adaptation_mult(b'A'), 1.2);
        assert_eq!(adaptation_mult(b'B'), 1.0);
        assert_eq!(adaptation_mult(b'C'), 0.8);
        assert_eq!(adaptation_mult(b'D'), 0.6);
        assert_eq!(adaptation_mult(b'-'), 0.0);
        assert_eq!(adaptation_mult(b'E'), 1.0); // 不明は B 相当
    }

    #[test]
    fn terrain_env_maps_classes() {
        assert_eq!(terrain_env("平地"), 1);
        assert_eq!(terrain_env("道路"), 1);
        assert_eq!(terrain_env("海"), 2);
        assert_eq!(terrain_env("宇宙"), 3);
        assert_eq!(terrain_env("空中"), 0);
    }

    #[test]
    fn terrain_adaptation_scales_damage() {
        // 適応 A (×1.2) のユニット同士 (helper の u()/p() は AAAA)。
        // 地形適応なし (env=-1) と 陸 (env=1) で攻撃力・防御力ともに ×1.2 され、
        // ダメージは 1.2 倍になる。武器は射撃 (max_range 2)、攻撃力 100。
        let atk_p = p(0, 0, 100);
        let def_p = p(0, 0, 0);
        let w = weapon(2000, 1, 2, 100);
        let atk_u = u(0, vec![]);
        let def_u = u(750, vec![]);

        let base =
            predict_with_status(&atk_p, &atk_u, &w, &def_p, &def_u, 0, 0, 100, 100, &[], &[]);
        let adapted = predict_with_status_terrain(
            &atk_p,
            &atk_u,
            &w,
            &def_p,
            &def_u,
            0,
            0,
            100,
            100,
            &[],
            &[],
            1,
            1,
            DamageSpiritLevels::default(),
            1.0,
        );
        assert_eq!(base.damage, 1250, "適応なし: 2000 - 750");
        assert_eq!(adapted.damage, 1500, "適応A: 2000×1.2 - 750×1.2");
    }

    #[test]
    fn forest_reduces_hit_chance() {
        // 平地: hit_mod=0、森林: hit_mod=10 (正=防御地形で被命中減)
        let on_plains = predict(
            &p(0, 0, 0),
            &u(0, vec![]),
            &weapon(0, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
        )
        .hit_chance;
        let in_forest = predict(
            &p(0, 0, 0),
            &u(0, vec![]),
            &weapon(0, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            10,
            5,
        )
        .hit_chance;
        assert!(in_forest < on_plains);
    }

    #[test]
    fn positive_terrain_hit_mod_reduces_hit() {
        // SRC `マップデータ.md`/`Unit.cls:6295`: 地形の命中修正 (回避修正) は正の値ほど被命中を
        // 下げる (防御地形)。terrain.txt はこの正の規約で格納される。`(100 - hit_mod)` で
        // 命中率が下がることを確認 (旧実装は `(100 + hit_mod)` で正値=被命中増の逆規約だった)。
        let base = predict(
            &p(80, 0, 0),
            &u(0, vec![]),
            &weapon(0, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
        )
        .hit_chance; // 180
        let on_terrain = predict(
            &p(80, 0, 0),
            &u(0, vec![]),
            &weapon(0, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            50,
            0,
        )
        .hit_chance; // 180 × (100-50)/100 = 90
        assert_eq!(base, 180);
        assert_eq!(on_terrain, 90, "防御地形 hit_mod=50 → ×0.50");
        assert!(on_terrain < base);
    }

    #[test]
    fn manhattan_basic() {
        assert_eq!(manhattan((0, 0), (3, 4)), 7);
        assert_eq!(manhattan((5, 5), (5, 5)), 0);
    }

    #[test]
    fn status_hisshu_forces_hit_100() {
        let prev = predict_with_status(
            &p(0, 99, 0), // 防御側 dodge 99 → 普段なら 命中率最低 5
            &u(0, vec![]),
            &weapon(0, 1, 1, 0),
            &p(0, 99, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &["必中".to_string()],
            &[],
        );
        assert_eq!(prev.hit_chance, 100);
    }

    #[test]
    fn status_hirameki_zeros_hit() {
        let prev = predict_with_status(
            &p(0, 0, 0),
            &u(0, vec![]),
            &weapon(0, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &["ひらめき".to_string()],
        );
        assert_eq!(prev.hit_chance, 0);
    }

    /// 盲目 / 撹乱 (攻撃側) は命中率を半減、盲目 (防御側) は被命中を 1.5 倍にする。
    #[test]
    fn status_moumoku_kakuran_modify_hit() {
        // 攻撃側 hit 30 / 防御側 dodge 0 → 中間値のベースライン。
        let base = predict_with_status(
            &p(30, 0, 0),
            &u(0, vec![]),
            &weapon(0, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        )
        .hit_chance;
        let atk_blind = predict_with_status(
            &p(30, 0, 0),
            &u(0, vec![]),
            &weapon(0, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &["盲目".to_string()],
            &[],
        )
        .hit_chance;
        let atk_kakuran = predict_with_status(
            &p(30, 0, 0),
            &u(0, vec![]),
            &weapon(0, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &["撹乱".to_string()],
            &[],
        )
        .hit_chance;
        let def_blind = predict_with_status(
            &p(30, 0, 0),
            &u(0, vec![]),
            &weapon(0, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &["盲目".to_string()],
        )
        .hit_chance;
        assert_eq!(atk_blind, base / 2, "盲目 (攻撃側) で命中半減");
        assert_eq!(atk_kakuran, base / 2, "撹乱 (攻撃側) で命中半減");
        assert_eq!(
            def_blind,
            (base * 3 / 2).min(95),
            "盲目 (防御側) で被命中 1.5 倍"
        );
    }

    #[test]
    fn status_nekketsu_doubles_damage() {
        let base = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        let with_nekketsu = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &["熱血".to_string()],
            &[],
        );
        assert_eq!(with_nekketsu.damage, base.damage * 2);
    }

    #[test]
    fn default_damage_boost_table_uses_max_not_stack() {
        // 既定テーブル (sp.txt 未読込経路): 熱血=Lv10 / 魂=Lv20 / 気合=Lv0。
        // C# `Unit.SpecialPowerEffectLevel` と同じく加算ではなく最大値勝ち。
        let s = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<String>>();
        // 熱血 のみ → Lv10 (×2.0)。
        assert_eq!(default_damage_boost_level(&s(&["熱血"])), 10.0);
        // 魂 のみ → Lv20 (×3.0)。
        assert_eq!(default_damage_boost_level(&s(&["魂"])), 20.0);
        // 気合 のみ → Lv0 (ダメージ増加なし → ×1.0)。
        assert_eq!(default_damage_boost_level(&s(&["気合"])), 0.0);
        // 熱血 + 魂 → max(10, 20) = 20 (×3.0)。加算 (×6) ではない。
        assert_eq!(default_damage_boost_level(&s(&["熱血", "魂"])), 20.0);
        // 該当なし → 0.0。
        assert_eq!(default_damage_boost_level(&s(&["必中"])), 0.0);
    }

    #[test]
    fn status_kiai_does_not_increase_damage() {
        // 気合 は気力 (morale) 増加効果でありダメージ増加効果を持たない。
        // 旧実装は ×1.2 していたが C# 原典準拠では与ダメージ不変 (×1.0)。
        let base = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        let with_kiai = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &["気合".to_string()],
            &[],
        );
        assert_eq!(with_kiai.damage, base.damage, "気合 は与ダメージを変えない");
    }

    #[test]
    fn status_nekketsu_and_tamashii_use_max_not_stack() {
        // 熱血 (Lv10) + 魂 (Lv20) → max=Lv20 → ×3.0 (×6 ではない)。
        let base = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        let both = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &["熱血".to_string(), "魂".to_string()],
            &[],
        );
        assert_eq!(both.damage, base.damage * 3, "熱血+魂: max(×2,×3)=×3");
    }

    #[test]
    fn status_teppeki_quarters_damage() {
        let base = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(800, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        let with_teppeki = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(800, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &["鉄壁".to_string()],
        );
        // 鉄壁 = 被ダメージ低下Lv7.5 → down-mod 1-0.75 = ×0.25 = ÷4 (データ駆動 down-mod 経由)。
        assert_eq!(with_teppeki.damage, base.damage / 4);
    }

    #[test]
    fn status_fukutsu_tenths_damage_then_floor() {
        // 不屈 = 被ダメージ低下Lv9 → down-mod 1-0.9 = ×0.1。旧実装は damage.min(1) で
        // 「→1」だったが、C# 準拠では ×0.1 後に最低ダメージ 10 の床が効く。
        // base=800 → ×0.1。down_mod = 1.0 - 0.1*9.0 = 0.09999999999999998 (倍精度浮動小数)。
        // 800 × 0.0999... = 79.99... → (i64 へ切り捨て) = 79。C# も同じ double 演算・(int) キャストで
        // 79 を返す (床 10 は無関係)。旧来は 1 になっていた → 真の挙動は 79。
        let base = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(800, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        let with_fukutsu = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(800, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &["不屈".to_string()],
        );
        assert_eq!(base.damage, 800);
        // ×0.1 (倍精度) → 79 (旧実装の 1 ではない)。
        assert_eq!(
            with_fukutsu.damage, 79,
            "不屈 = 被ダメージ低下Lv9 → ×0.1 (倍精度 79)"
        );
    }

    #[test]
    fn status_fukutsu_small_damage_hits_floor() {
        // 小ダメージで 不屈 (×0.1) が床 10 を下回る場合: base=50 → ×0.1 = 5 → floor 10。
        let with_fukutsu = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(50, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &["不屈".to_string()],
        );
        assert_eq!(with_fukutsu.damage, 10, "5 → 最低ダメージ床 10");
    }

    #[test]
    fn status_def_higaidamage_increase_adds_to_up_mod() {
        // 防御側 被ダメージ増加Lv1 (分析/偵察) → up-mod へ +0.1 = ×1.1。
        let base = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(1000, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        let with_analysis = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(1000, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &["分析".to_string()],
        );
        assert_eq!(base.damage, 1000);
        // 分析 = ダメージ低下Lv1 (攻撃側効果 → 防御側付与なので適用なし) +
        //        被ダメージ増加Lv1 (防御側効果 → up-mod +0.1 = ×1.1)。1000 × 1.1 = 1100。
        assert_eq!(
            with_analysis.damage, 1100,
            "防御側 被ダメージ増加Lv1 → ×1.1"
        );
    }

    #[test]
    fn status_atk_damage_decrease_subtracts_from_down_mod() {
        // 攻撃側 ダメージ低下Lv1 (分析/偵察) → down-mod へ -0.1 = ×0.9。
        let with_decrease = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(1000, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &["分析".to_string()],
            &[],
        );
        // 攻撃側 分析: ダメージ低下Lv1 → down-mod ×0.9 (被ダメージ増加 は防御側効果なので攻撃側付与では効かない)。
        assert_eq!(with_decrease.damage, 900, "攻撃側 ダメージ低下Lv1 → ×0.9");
    }

    #[test]
    fn damage_spirit_levels_default_table() {
        let s = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<String>>();
        // 攻撃側 熱血 + 防御側 鉄壁。
        let lv = default_damage_spirit_levels(&s(&["熱血"]), &s(&["鉄壁"]));
        assert_eq!(lv.atk_increase, 10.0);
        assert_eq!(lv.def_decrease_taken, 7.5);
        assert_eq!(lv.def_increase_taken, 0.0);
        assert_eq!(lv.atk_decrease_dealt, 0.0);
        // 防御側 不屈 → 被ダメージ低下Lv9。
        let lv = default_damage_spirit_levels(&[], &s(&["不屈"]));
        assert_eq!(lv.def_decrease_taken, 9.0);
        // 分析 は攻撃側 ダメージ低下Lv1 / 防御側 被ダメージ増加Lv1 の両効果を持つ。
        let lv = default_damage_spirit_levels(&s(&["分析"]), &s(&["分析"]));
        assert_eq!(lv.atk_decrease_dealt, 1.0);
        assert_eq!(lv.def_increase_taken, 1.0);
    }

    #[test]
    fn status_sutemi_triples_attacker_damage() {
        let base = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        let sutemi = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &["捨て身".to_string()],
            &[],
        );
        assert_eq!(sutemi.damage, base.damage * 3, "捨て身: 与ダメージ 3 倍");
    }

    #[test]
    fn status_sutemi_defender_is_defenseless() {
        // 高回避 (dodge 80) の防御側でも 捨て身 (無防備) なら命中 100。
        let evasive = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 80, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        assert!(
            evasive.hit_chance < 100,
            "高回避で base 命中 < 100: {}",
            evasive.hit_chance
        );
        let sutemi = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 80, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &["捨て身".to_string()],
        );
        assert_eq!(sutemi.hit_chance, 100, "捨て身 (無防備) で命中 100");
    }

    #[test]
    fn status_chokugeki_nullifies_barrier() {
        // バリア: ダメージ 1/2 → 直撃 で無効化。
        // (分身 は (B) で実行段ロール化したため予測テストの対象外。)
        let barrier = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(800, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &["バリア".to_string()],
        );
        let chokugeki_dmg = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(800, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &["直撃".to_string()],
            &["バリア".to_string()],
        );
        assert_eq!(
            chokugeki_dmg.damage,
            barrier.damage * 2,
            "直撃でバリアの半減が無効"
        );
    }

    // 注: 分身/超回避 は (B) で実行段の完全回避ロール (App::check_dodge_feature) へ移行し、
    // 予測 (hit%) には反映しないため、旧 status_chougaihi_reduces_hit_by_ten_times_level
    // (予測の命中ペナルティ検証) は撤去した。回避ロールは app 側のテストで検証する。

    #[test]
    fn ecm_hit_mult_scales_predicted_hit() {
        // ＥＣＭ 補正係数が予測命中率に乗る (必中/ひらめき の上書きより前)。
        let mk = |mult: f64| {
            predict_with_status_terrain(
                &p(200, 0, 100),
                &u(0, vec![]),
                &weapon(500, 1, 1, 0),
                &p(0, 0, 0),
                &u(0, vec![]),
                0,
                0,
                100,
                100,
                &[],
                &[],
                -1,
                -1,
                DamageSpiritLevels::default(),
                mult,
            )
            .hit_chance
        };
        let base = mk(1.0);
        let ecm = mk(0.5);
        assert_eq!(ecm, base / 2, "ＥＣＭ 0.5 で命中半減");
    }

    #[test]
    fn status_paralysis_boosts_hit_and_damage() {
        let prev = predict_with_status(
            &p(0, 99, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 99, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &["麻痺".to_string()],
        );
        assert_eq!(prev.hit_chance, 100);
        // damage = 500 * 1.5 = 750
        assert!(prev.damage >= 700);
    }

    #[test]
    fn dodge_reduces_hit_chance() {
        let dodged = predict_with_defense(
            &p(0, 0, 0),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 50, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
            DefenseMode::Dodge,
        );
        assert_eq!(dodged.hit_chance, 40);
    }

    #[test]
    fn hit_chance_has_no_upper_cap() {
        // SRC 原典 (`Unit.cls:6694-6696`) は命中率に上限を設けない (>100=必中)。
        // 旧実装の clamp(5,95) は非原典だった。差分オラクル (placeattack) で C# と突合し是正済。
        // 命中値 100(hit)+50(precision)=250、回避 0 → 命中率 250 (95 で頭打ちにしない)。
        let prev = predict(
            &p(100, 0, 0),
            &u(0, vec![]),
            &weapon(500, 1, 1, 50),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
        );
        assert_eq!(prev.hit_chance, 250);
        assert!(prev.hit_chance > 95, "命中率は 95 で頭打ちにしない");
    }

    #[test]
    fn minimum_damage_is_ten() {
        // SRC 原典 (`Unit.cls:7460-7474` / C# `UnitWeapon.cs:3567`) の最低ダメージは既定 10。
        // 旧実装は max(1) だった。装甲(1000) >> 攻撃力(100×0.5=50) でも 10 を下限とする。
        let prev = predict(
            &p(0, 0, 50),
            &u(0, vec![]),
            &weapon(100, 2, 2, 0),
            &p(0, 0, 0),
            &u(1000, vec![]),
            0,
            0,
        );
        assert_eq!(prev.damage, 10);
    }

    #[test]
    fn defend_halves_damage() {
        let base = predict(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
        );
        let defended = predict_with_defense(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
            DefenseMode::Defend,
        );
        assert_eq!(defended.damage, base.damage / 2);
    }

    #[test]
    fn barrier_absorbs_damage_up_to_limit() {
        // Barrier 500 vs damage 800 -> remaining 300
        let prev = predict_with_defense(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(800, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
            DefenseMode::Barrier { strength: 500 },
        );
        assert_eq!(prev.damage, 300);
    }

    #[test]
    fn barrier_fully_absorbs_small_damage() {
        // Barrier 1000 vs damage 500 -> remaining 0
        let prev = predict_with_defense(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(500, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
            DefenseMode::Barrier { strength: 1000 },
        );
        assert_eq!(prev.damage, 0);
    }

    #[test]
    fn shield_reduces_expected_damage() {
        // Shield 50% -> expected damage halved
        let base = predict(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(1000, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
        );
        let shielded = predict_with_defense(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(1000, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
            DefenseMode::Shield { chance: 50 },
        );
        assert_eq!(shielded.damage, base.damage / 2);
    }

    #[test]
    fn map_attack_all_hits_all_in_range() {
        let shape = MapAttackShape::All;
        let attacker = (5, 5);
        let target = (5, 5);
        let cells = shape.affected_cells(attacker, target, 2);
        // With range 2, should cover diamond shape: dx+dy <= 2
        assert!(cells.contains(&(5, 5)));
        assert!(cells.contains(&(6, 5)));
        assert!(cells.contains(&(4, 5)));
        assert!(cells.contains(&(5, 6)));
        assert!(cells.contains(&(5, 4)));
        assert!(cells.contains(&(6, 6)));
        assert!(cells.contains(&(4, 4)));
        assert!(cells.contains(&(7, 5)));
        assert!(cells.contains(&(3, 5)));
    }

    #[test]
    fn map_attack_spread_hits_cross_pattern() {
        let shape = MapAttackShape::Spread;
        let attacker = (0, 0);
        let target = (5, 5);
        let cells = shape.affected_cells(attacker, target, 10);
        // Spread hits target + 4 adjacent (cross pattern)
        assert_eq!(cells.len(), 5);
        assert!(cells.contains(&(5, 5)));
        assert!(cells.contains(&(6, 5)));
        assert!(cells.contains(&(4, 5)));
        assert!(cells.contains(&(5, 6)));
        assert!(cells.contains(&(5, 4)));
    }

    #[test]
    fn map_attack_throw_hits_radius() {
        let shape = MapAttackShape::Throw { radius: 2 };
        let attacker = (0, 0);
        let target = (10, 10);
        let cells = shape.affected_cells(attacker, target, 5);
        // Within radius 2 of target (10,10): dx+dy <= 2
        assert!(cells.contains(&(10, 10)));
        assert!(cells.contains(&(11, 10)));
        assert!(cells.contains(&(9, 10)));
        assert!(cells.contains(&(10, 11)));
        assert!(cells.contains(&(10, 9)));
        assert!(cells.contains(&(11, 11)));
        assert!(cells.contains(&(9, 9)));
        // (12, 10) has dx=2, dy=0, sum=2, should be included
        assert!(cells.contains(&(12, 10)));
    }

    #[test]
    fn map_attack_single_only_hits_target() {
        let shape = MapAttackShape::Single;
        let attacker = (0, 0);
        let target = (5, 5);
        let cells = shape.affected_cells(attacker, target, 99);
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0], (5, 5));
    }

    #[test]
    fn is_unit_hit_by_map_attack_correct() {
        let shape = MapAttackShape::Spread;
        let attacker = (0, 0);
        let target = (5, 5);
        let max_range = 10;
        // Target cell and its 4 adjacent
        assert!(is_unit_hit_by_map_attack(
            shape,
            attacker,
            target,
            (5, 5),
            max_range
        ));
        assert!(is_unit_hit_by_map_attack(
            shape,
            attacker,
            target,
            (6, 5),
            max_range
        ));
        assert!(is_unit_hit_by_map_attack(
            shape,
            attacker,
            target,
            (4, 5),
            max_range
        ));
        assert!(is_unit_hit_by_map_attack(
            shape,
            attacker,
            target,
            (5, 6),
            max_range
        ));
        assert!(is_unit_hit_by_map_attack(
            shape,
            attacker,
            target,
            (5, 4),
            max_range
        ));
        // Not hit: diagonals and further
        assert!(!is_unit_hit_by_map_attack(
            shape,
            attacker,
            target,
            (6, 6),
            max_range
        ));
        assert!(!is_unit_hit_by_map_attack(
            shape,
            attacker,
            target,
            (7, 5),
            max_range
        ));
    }

    #[test]
    fn map_attack_shape_from_weapon_class_parsing() {
        assert_eq!(
            MapAttackShape::from_weapon_class("Ｍ全"),
            MapAttackShape::All
        );
        assert_eq!(
            MapAttackShape::from_weapon_class("Ｍ投L3"),
            MapAttackShape::Throw { radius: 3 }
        );
        assert_eq!(
            MapAttackShape::from_weapon_class("Ｍ投L1"),
            MapAttackShape::Throw { radius: 1 }
        );
        assert_eq!(
            MapAttackShape::from_weapon_class("Ｍ直"),
            MapAttackShape::Straight
        );
        assert_eq!(
            MapAttackShape::from_weapon_class("Ｍ拡"),
            MapAttackShape::Spread
        );
        assert_eq!(
            MapAttackShape::from_weapon_class("Ｍ扇"),
            MapAttackShape::Fan
        );
        assert_eq!(
            MapAttackShape::from_weapon_class("Ｍ線"),
            MapAttackShape::Line
        );
        // No match -> Single
        assert_eq!(
            MapAttackShape::from_weapon_class("通常兵器"),
            MapAttackShape::Single
        );
        assert_eq!(
            MapAttackShape::from_weapon_class(""),
            MapAttackShape::Single
        );
    }

    #[test]
    fn size_xl_doubles_hit_chance() {
        // XL サイズ (×2.0) vs M サイズ (×1.0)
        // atk.hit=0, def.dodge=70 → base = 100-70 = 30 → M: 30; XL: 60
        let def_m = u(0, vec![]);
        let def_xl = UnitData {
            abilities: Vec::new(),
            size: Size::XL,
            ..u(0, vec![])
        };
        let w = weapon(0, 1, 1, 0);
        let low_hit =
            predict(&p(0, 0, 0), &u(0, vec![]), &w, &p(0, 70, 0), &def_m, 0, 0).hit_chance;
        let high_hit =
            predict(&p(0, 0, 0), &u(0, vec![]), &w, &p(0, 70, 0), &def_xl, 0, 0).hit_chance;
        assert_eq!(low_hit, 30); // (100-70) * 1.0 = 30
        assert_eq!(high_hit, 60); // (100-70) * 2.0 = 60
    }

    #[test]
    fn size_ss_halves_hit_chance() {
        // SS サイズ (×0.5) — 命中率が半分になる。
        let def_ss = UnitData {
            abilities: Vec::new(),
            size: Size::SS,
            ..u(0, vec![])
        };
        let hit = predict(
            &p(0, 0, 0),
            &u(0, vec![]),
            &weapon(0, 1, 1, 0),
            &p(0, 70, 0),
            &def_ss,
            0,
            0,
        )
        .hit_chance;
        // (100-70) * 0.5 = 15 → clamp(15, 5, 95) = 15
        assert_eq!(hit, 15);
    }

    #[test]
    fn morale_scales_damage() {
        // 気力 150 のとき: weapon.power * pilot_attack/100 * 150/100 = 1.5 倍
        let base = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(1000, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        let high_morale = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(1000, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            150,
            100,
            &[],
            &[],
        );
        // base: 1000 * 100/100 * 100/100 = 1000
        // high morale: 1000 * 100/100 * 150/100 = 1500
        assert_eq!(base.damage, 1000);
        assert_eq!(high_morale.damage, 1500);
    }

    #[test]
    fn pilot_attack_stat_scales_damage() {
        // infight=50 → 50% 武器威力
        let half_power = predict_with_status(
            &p(0, 0, 50),
            &u(0, vec![]),
            &weapon(1000, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        // infight=100 → 100% 武器威力
        let full_power = predict_with_status(
            &p(0, 0, 100),
            &u(0, vec![]),
            &weapon(1000, 1, 1, 0),
            &p(0, 0, 0),
            &u(0, vec![]),
            0,
            0,
            100,
            100,
            &[],
            &[],
        );
        assert_eq!(half_power.damage, 500);
        assert_eq!(full_power.damage, 1000);
    }

    // ── クリティカル発生率 ──────────────────────────────
    /// technique / weapon critical を指定したパイロット・武器を作る。
    fn p_tech(technique: i32) -> PilotData {
        let mut pilot = p(0, 0, 0);
        pilot.technique = technique;
        pilot
    }
    fn weapon_ct(ct: i32) -> WeaponData {
        let mut w = weapon(1000, 1, 1, 0);
        w.critical = ct;
        w
    }

    #[test]
    fn critical_basic_technique_difference() {
        // (攻撃側技量 − 防御側技量) + 武器CT = (50 − 20) + 10 = 40
        let prob = critical_probability(&p_tech(50), &p_tech(20), &weapon_ct(10), &[]);
        assert_eq!(prob, 40);
    }

    #[test]
    fn critical_clamps_to_min_one() {
        // 大きく負でも最低 1%。
        let prob = critical_probability(&p_tech(0), &p_tech(200), &weapon_ct(0), &[]);
        assert_eq!(prob, 1);
    }

    #[test]
    fn critical_clamps_to_max_hundred() {
        let prob = critical_probability(&p_tech(200), &p_tech(0), &weapon_ct(50), &[]);
        assert_eq!(prob, 100);
    }

    #[test]
    fn critical_plus_ten_when_defender_disabled() {
        // 同技量・CT0 なら基本 0 → クランプで 1。麻痺なら +10 で 10。
        let base = critical_probability(&p_tech(30), &p_tech(30), &weapon_ct(0), &[]);
        assert_eq!(base, 1, "基本 0 はクランプで 1");
        let disabled = critical_probability(
            &p_tech(30),
            &p_tech(30),
            &weapon_ct(0),
            &["麻痺".to_string()],
        );
        assert_eq!(disabled, 10, "行動不能で +10");
    }

    #[test]
    fn critical_weapon_ct_contributes() {
        // 技量差 0、武器CT=25 → 25。
        let prob = critical_probability(&p_tech(40), &p_tech(40), &weapon_ct(25), &[]);
        assert_eq!(prob, 25);
    }

    #[test]
    fn scatter_attribute_distance_modifiers_match_src() {
        // SRC.NET Unit.cs: 散 属性武器は距離 1/2/3/4/5+ で命中 +0/+5/+10/+15/+20、
        // ダメージ ×1.0/0.95/0.90/0.85/0.80。武器 class に 散 が無ければ無補正。
        for d in 0..=6u32 {
            // 非 散 武器は常に無補正。
            assert_eq!(scatter_hit_bonus("格魔", d), 0);
            assert!((scatter_damage_mult("格魔", d) - 1.0).abs() < 1e-9);
        }
        // 散 武器の命中ボーナス (距離 0/1 は同値=+0)。
        assert_eq!(scatter_hit_bonus("格魔散", 1), 0);
        assert_eq!(scatter_hit_bonus("格魔散", 2), 5);
        assert_eq!(scatter_hit_bonus("格魔散", 3), 10);
        assert_eq!(scatter_hit_bonus("格魔散", 4), 15);
        assert_eq!(scatter_hit_bonus("格魔散", 5), 20);
        assert_eq!(
            scatter_hit_bonus("格魔散", 9),
            20,
            "5 マス以上は +20 で頭打ち"
        );
        // 散 武器のダメージ倍率。
        assert!((scatter_damage_mult("散", 1) - 1.00).abs() < 1e-9);
        assert!((scatter_damage_mult("散", 2) - 0.95).abs() < 1e-9);
        assert!((scatter_damage_mult("散", 3) - 0.90).abs() < 1e-9);
        assert!((scatter_damage_mult("散", 4) - 0.85).abs() < 1e-9);
        assert!((scatter_damage_mult("散", 5) - 0.80).abs() < 1e-9);
        assert!(
            (scatter_damage_mult("散", 9) - 0.80).abs() < 1e-9,
            "5 マス以上は ×0.8 で頭打ち"
        );
        // apply_scatter: 距離 2 で命中 +5・ダメージ ×0.95。
        let p = CombatPreview {
            hit_chance: 90,
            damage: 1000,
            critical_chance: 5,
        }
        .apply_scatter("格魔散", 2);
        assert_eq!(p.hit_chance, 95);
        assert_eq!(p.damage, 950);
    }
}
