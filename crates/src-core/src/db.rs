//! ゲーム全体のデータベース / Aggregated game-wide static data.
//!
//! 元 SRC の `SRC.bas` で `Public PDList As New PilotDataList` のように
//! グローバル変数として持っていた各種データリストの集約。Rust 移植では
//! グローバル変数を避け、`App` 構造体の中に `GameDatabase` を抱える形にする。

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::data::item::ItemData;
use crate::data::map::MapData;
use crate::data::pilot::PilotData;
use crate::data::special_power::SpecialPowerData;
use crate::data::terrain_file::TerrainEntry;
use crate::data::unit::UnitData;
use crate::pilot_instance::PilotInstance;
use crate::unit_instance::UnitInstance;

/// シナリオロード時に蓄積される全データ。
/// Aggregate of every static record loaded from the scenario's `Data/*.txt`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameDatabase {
    /// 元: `SRC.bas::PDList` (`PilotDataList`)
    pub pilots: Vec<PilotData>,
    /// 元: `SRC.bas::UDList` (`UnitDataList`)
    pub units: Vec<UnitData>,
    /// 元: `Map.bas::MapData`
    pub map: Option<MapData>,
    /// 元: `SRC.bas::UList` (`Units`) — マップ上に配置されたユニット実体。
    pub unit_instances: Vec<UnitInstance>,
    /// Runtime pilot instances — mutable state for pilots assigned to units.
    #[serde(default)]
    pub pilot_instances: Vec<PilotInstance>,
    /// 元: `SRC.bas::IDList` (`ItemDataList`)
    pub items: Vec<ItemData>,
    /// 元: `SRC.bas::SPDList` (`SpecialPowerDataList`)
    pub special_powers: Vec<SpecialPowerData>,
    /// 元: `SRC.bas::TDList` (`TerrainDataList`)。空ならビルトイン
    /// `data::terrain::DEFAULT_TERRAINS` にフォールバック。
    pub terrains: Vec<TerrainEntry>,
    /// シナリオ ZIP 内の `.map` ファイルを basename (lowercase) でキャッシュ。
    /// `ChangeMap "path"` 命令で参照され、見つかれば `map` を差し替える。
    #[serde(default)]
    pub maps: BTreeMap<String, MapData>,
    /// 戦闘アニメデータ (`animation.txt` / `ext_animation.txt`)。武器(状況)→表示用
    /// サブルーチン名の解決に使う。SRC `MessageDataList`(animation) 相当。
    #[serde(default)]
    pub animation: crate::data::animation::AnimationData,
    /// どのユニットにも装備されていない「未装備」アイテムの在庫
    /// (アイテム名のリスト)。`RemoveItem unit` (item 省略 = 取り外し) で
    /// ここへ移り、`Item`/`Equip` で在庫から取り出される。
    /// SRC.NET の `IList` 中 `Unit is null` なアイテム群に相当。
    #[serde(default)]
    pub spare_items: Vec<String>,
    /// `UnitInstance.uid` 採番カウンタ。`register_unit` / `mint_uid` で +1。
    /// SRC の一意 ID 採番に相当。空 uid を許さない不変条件の供給源。
    #[serde(default = "default_uid_start")]
    next_uid: u64,
    /// 位置 → uid のグリッド索引。「どのユニットがそのマスに居るか」の単一の
    /// 真実源。`move_unit` / `remove_unit` / `set_off_map` / `register_unit`
    /// を通じてのみ更新する。SRC.Sharp `Map.MapDataForUnit[x,y]` 相当。
    /// `off_map` のユニットは載せない。シリアライズせず、ロード後に
    /// `rebuild_pos_index` で再構築する。
    #[serde(skip)]
    pos_index: BTreeMap<(u32, u32), String>,
}

/// `next_uid` の serde デフォルト (1 始まり、`U1` から採番)。
fn default_uid_start() -> u64 {
    1
}

impl Default for GameDatabase {
    fn default() -> Self {
        Self::new()
    }
}

impl GameDatabase {
    pub const fn new() -> Self {
        Self {
            pilots: Vec::new(),
            units: Vec::new(),
            map: None,
            unit_instances: Vec::new(),
            pilot_instances: Vec::new(),
            items: Vec::new(),
            special_powers: Vec::new(),
            terrains: Vec::new(),
            maps: BTreeMap::new(),
            animation: crate::data::animation::AnimationData {
                entries: BTreeMap::new(),
            },
            spare_items: Vec::new(),
            next_uid: 1,
            pos_index: BTreeMap::new(),
        }
    }

    /// 戦闘アニメデータ (`animation.txt` / `ext_animation.txt`) をパースして取り込む。
    /// 既存定義は保持し、同一対象には追記する (SRC `is_effect` 挙動)。
    pub fn merge_animation_data(&mut self, src: &str) {
        self.animation.merge_from_str(src);
    }

    /// 武器使用時の戦闘アニメを解決する (`unit_data_name` のユニットが `weapon` を
    /// `situation_label`(攻撃/命中/準備/反撃 等) で使ったときの表示用サブルーチン)。
    /// 戦闘アニメデータが無ければ空。
    pub fn resolve_weapon_animation(
        &self,
        unit_data_name: &str,
        weapon: &str,
        situation_label: &str,
        phase: crate::data::animation::WeaponPhase,
    ) -> Vec<crate::data::animation::ResolvedAnim> {
        if self.animation.is_empty() {
            return Vec::new();
        }
        let (nickname, class) = self
            .unit_by_name(unit_data_name)
            .map(|u| (u.nickname.clone(), u.class.clone()))
            .unwrap_or_default();
        self.animation.resolve_weapon(
            unit_data_name,
            &nickname,
            &class,
            weapon,
            situation_label,
            phase,
        )
    }

    /// 一意な `uid` (`U{n}`) を 1 つ採番する。SRC `グループＩＤ` 相当。
    pub fn mint_uid(&mut self) -> String {
        let v = self.next_uid;
        self.next_uid = self.next_uid.saturating_add(1);
        format!("U{v}")
    }

    /// `UnitInstance` を DB に登録する。`uid` が空なら採番し、`pos_index` を
    /// 同期した上で `unit_instances` に push する。返り値は確定した `uid`。
    /// マップ上ユニットの唯一の生成口とし、空 uid を生まない不変条件を担保する。
    pub fn register_unit(&mut self, mut inst: UnitInstance) -> String {
        if inst.uid.is_empty() {
            inst.uid = self.mint_uid();
        }
        let uid = inst.uid.clone();
        if !inst.off_map {
            self.pos_index.insert((inst.x, inst.y), uid.clone());
        }
        self.unit_instances.push(inst);
        uid
    }

    /// `pos_index` を `unit_instances` から再構築する。ロード後・一括改変後・
    /// `replace_unit_instances` 後に呼ぶ。`off_map` のユニットは載せない。
    pub fn rebuild_pos_index(&mut self) {
        self.pos_index.clear();
        for u in &self.unit_instances {
            if !u.off_map {
                self.pos_index.insert((u.x, u.y), u.uid.clone());
            }
        }
    }

    /// 指定マスに居る (配置済みの) ユニットの uid。SRC.Sharp
    /// `Map.MapDataForUnit[x,y]` 相当のグリッド索引参照。
    pub fn uid_at(&self, x: u32, y: u32) -> Option<&str> {
        self.pos_index.get(&(x, y)).map(String::as_str)
    }

    /// uid → `unit_instances` の現在インデックス。削除/並べ替えに強いよう
    /// 都度走査する (uid→idx の永続マップは持たない)。
    pub fn idx_by_uid(&self, uid: &str) -> Option<usize> {
        self.unit_instances.iter().position(|u| u.uid == uid)
    }

    /// uid でユニット実体を引く (不変参照)。
    pub fn unit_by_uid(&self, uid: &str) -> Option<&UnitInstance> {
        self.unit_instances.iter().find(|u| u.uid == uid)
    }

    /// uid でユニット実体を引く (可変参照)。座標は直接書き換えず必ず
    /// `move_unit` を使うこと (`pos_index` 同期のため)。
    pub fn unit_by_uid_mut(&mut self, uid: &str) -> Option<&mut UnitInstance> {
        self.unit_instances.iter_mut().find(|u| u.uid == uid)
    }

    /// uid のユニットを (x,y) へ移動し `pos_index` を同期する。射程/コスト判定は
    /// 呼び出し側の責務。存在しなければ `false`。
    pub fn move_unit(&mut self, uid: &str, x: u32, y: u32) -> bool {
        let Some(i) = self.idx_by_uid(uid) else {
            return false;
        };
        let old = (self.unit_instances[i].x, self.unit_instances[i].y);
        // 旧マスの索引はこの uid を指している場合のみ削除 (別ユニットで上書き済みなら残す)。
        if self.uid_at(old.0, old.1) == Some(uid) {
            self.pos_index.remove(&old);
        }
        self.unit_instances[i].x = x;
        self.unit_instances[i].y = y;
        if !self.unit_instances[i].off_map {
            self.pos_index.insert((x, y), uid.to_string());
        }
        true
    }

    /// uid のユニットの `off_map` フラグを設定し `pos_index` を同期する。
    /// `off_map=true` でグリッドから外し、`false` で現在地に載せ直す。
    pub fn set_off_map(&mut self, uid: &str, off: bool) {
        let Some(i) = self.idx_by_uid(uid) else {
            return;
        };
        let (x, y) = (self.unit_instances[i].x, self.unit_instances[i].y);
        self.unit_instances[i].off_map = off;
        if off {
            if self.uid_at(x, y) == Some(uid) {
                self.pos_index.remove(&(x, y));
            }
        } else {
            self.pos_index.insert((x, y), uid.to_string());
        }
    }

    /// インデックス指定でユニットを除去し `pos_index` を同期する。
    /// 戦闘の撃破処理など、既存の index ベース経路から呼ぶ橋渡し。
    pub fn remove_unit_at(&mut self, idx: usize) -> UnitInstance {
        let inst = self.unit_instances.remove(idx);
        if self.uid_at(inst.x, inst.y) == Some(inst.uid.as_str()) {
            self.pos_index.remove(&(inst.x, inst.y));
        }
        inst
    }

    /// uid 指定でユニットを除去し `pos_index` を同期する。
    pub fn remove_unit(&mut self, uid: &str) -> Option<UnitInstance> {
        let i = self.idx_by_uid(uid)?;
        Some(self.remove_unit_at(i))
    }

    /// `pos_index` が `unit_instances` と整合しているか検査する (テスト用)。
    /// ミューテータを迂回した座標書き換えがあれば false。
    #[cfg(test)]
    pub fn pos_index_is_consistent(&self) -> bool {
        let mut expected: BTreeMap<(u32, u32), String> = BTreeMap::new();
        for u in &self.unit_instances {
            if !u.off_map {
                expected.insert((u.x, u.y), u.uid.clone());
            }
        }
        expected == self.pos_index
    }

    /// 事前ロードされた `.map` の登録。キーは basename (lowercase)。
    pub fn store_map(&mut self, name: String, map: MapData) {
        self.maps.insert(name, map);
    }

    /// `ChangeMap "path"` 用のルックアップ。basename 部分のみで照合し、
    /// 拡張子の大小も無視。
    pub fn find_map(&self, path: &str) -> Option<&MapData> {
        let lname = path.to_ascii_lowercase();
        let slash = lname.rfind('/').map(|i| i + 1).unwrap_or(0);
        let bslash = lname.rfind('\\').map(|i| i + 1).unwrap_or(0);
        let base = &lname[slash.max(bslash)..];
        self.maps.get(base)
    }

    /// 地形定義を加法的に取り込む。複数の terrain.txt をマージできる。
    /// 同 id は後勝ち。
    pub fn extend_terrains(&mut self, terrains: Vec<TerrainEntry>) {
        for t in terrains {
            if let Some(slot) = self.terrains.iter_mut().find(|e| e.id == t.id) {
                *slot = t;
            } else {
                self.terrains.push(t);
            }
        }
    }

    /// 地形 id から `TerrainEntry`（シナリオ定義）を引く。
    /// 見つからなければ `None`（呼び出し側でビルトインカタログにフォールバック）。
    pub fn terrain_by_id(&self, id: u32) -> Option<&TerrainEntry> {
        self.terrains.iter().find(|t| t.id == id)
    }

    /// 地形 id から移動コストを引く。シナリオ定義優先、無ければビルトイン、それも無ければ 1。
    pub fn terrain_move_cost(&self, id: u32) -> i32 {
        if let Some(t) = self.terrain_by_id(id) {
            return t.move_cost;
        }
        crate::data::terrain::lookup(id)
            .map(|t| t.move_cost)
            .unwrap_or(1)
    }

    /// 命中修正（マイナスは攻撃側不利）。
    pub fn terrain_hit_mod(&self, id: u32) -> i32 {
        if let Some(t) = self.terrain_by_id(id) {
            return t.hit_mod;
        }
        crate::data::terrain::lookup(id)
            .map(|t| t.hit_mod)
            .unwrap_or(0)
    }

    /// ダメージ修正（プラスはダメージ軽減）。
    pub fn terrain_damage_mod(&self, id: u32) -> i32 {
        if let Some(t) = self.terrain_by_id(id) {
            return t.damage_mod;
        }
        crate::data::terrain::lookup(id)
            .map(|t| t.damage_mod)
            .unwrap_or(0)
    }

    /// 表示用に「(日本語名, 英名候補リスト, color, glyph)」を返す。
    /// シナリオ定義があれば優先、無ければビルトイン、それも無ければ汎用。
    pub fn terrain_display(&self, id: u32) -> (String, Vec<String>, &'static str, &'static str) {
        if let Some(t) = self.terrain_by_id(id) {
            let mut hints = Vec::new();
            if !t.english.is_empty() {
                hints.push(t.english.clone());
            }
            hints.push(t.name.clone());
            // ビルトインの color/glyph 推定: クラスごとに静的マッピング
            let (color, glyph) = match t.class.as_str() {
                "海" => ("#1e88e5", "～"),
                "山" => ("#8d6e63", "山"),
                "森林" | "林" => ("#2e7d32", "木"),
                "都市" => ("#cfd8dc", "市"),
                "空" => ("#e1f5fe", ""),
                "宇宙" => ("#1a237e", "*"),
                _ => ("#7cb342", ""),
            };
            return (t.name.clone(), hints, color, glyph);
        }
        if let Some(t) = crate::data::terrain::lookup(id) {
            let hints: Vec<String> = t
                .bitmap_hint
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            return (t.name.to_string(), hints, t.color, t.glyph);
        }
        (format!("?{id}"), Vec::new(), "#ff00ff", "?")
    }

    /// アイテム定義を加法的に取り込む。同名は後勝ち。
    pub fn extend_items(&mut self, items: Vec<ItemData>) {
        for i in items {
            if let Some(slot) = self.items.iter_mut().find(|e| e.name == i.name) {
                *slot = i;
            } else {
                self.items.push(i);
            }
        }
    }

    /// 特殊能力定義を加法的に取り込む。同名は後勝ち。
    pub fn extend_special_powers(&mut self, sps: Vec<SpecialPowerData>) {
        for s in sps {
            if let Some(slot) = self.special_powers.iter_mut().find(|e| e.name == s.name) {
                *slot = s;
            } else {
                self.special_powers.push(s);
            }
        }
    }

    /// かかっている精神コマンド (`active` = 精神名スナップショット) の効果タイプ
    /// `effect_type` のレベルの **最大値** を返す。元: C# `Unit.SpecialPowerEffectLevel`
    /// (`Unit.sp.cs`) ＝ 影響下の各スペシャルパワーの効果レベルの最大値
    /// (異なる精神間で加算はせず最大値勝ち)。
    ///
    /// 各精神名は `self.special_powers` から引き、その `effects` 内の `effect_type`
    /// レベルを参照する。ロード済み DB に存在しない精神名 (sp.txt 未読込の合成テスト
    /// 経路等) は既定テーブル ([`combat::default_sp_effect_level_single`]) でフォールバック
    /// する。該当効果が一つも無ければ 0.0。
    ///
    /// caller は `ダメージ増加` (攻撃側) / `被ダメージ増加` (防御側) / `ダメージ低下`
    /// (攻撃側) / `被ダメージ低下` (防御側) を解決して
    /// [`combat::DamageSpiritLevels`] を組み立て、`predict_with_status_terrain` へ渡す。
    pub fn sp_effect_level(&self, active: &[String], effect_type: &str) -> f64 {
        let mut max_lv = 0.0f64;
        for name in active {
            let lv = match self.special_powers.iter().find(|s| &s.name == name) {
                Some(spd) => spd
                    .effects
                    .iter()
                    .filter(|(etype, _)| etype == effect_type)
                    .map(|(_, lv)| *lv)
                    .fold(0.0f64, f64::max),
                // DB 未定義: 既定テーブルへフォールバック (単一名分)。
                None => crate::combat::default_sp_effect_level_single(name, effect_type),
            };
            if lv > max_lv {
                max_lv = lv;
            }
        }
        max_lv
    }

    /// 攻撃側 `atk` / 防御側 `def` の精神名スナップショットから、与/被ダメージ修正の
    /// 4 種レベル束 ([`combat::DamageSpiritLevels`]) を解決する。caller は結果を
    /// `combat::predict_with_status_terrain` へ渡す。
    pub fn damage_spirit_levels(
        &self,
        atk: &[String],
        def: &[String],
    ) -> crate::combat::DamageSpiritLevels {
        crate::combat::DamageSpiritLevels {
            atk_increase: self.sp_effect_level(atk, "ダメージ増加"),
            def_increase_taken: self.sp_effect_level(def, "被ダメージ増加"),
            atk_decrease_dealt: self.sp_effect_level(atk, "ダメージ低下"),
            def_decrease_taken: self.sp_effect_level(def, "被ダメージ低下"),
        }
    }

    /// パイロット定義を加法的に取り込む。元 `PDList.Load` 相当。複数の
    /// pilot.txt (`data/pilot.txt` + `data/<シナリオ>/pilot.txt` 等) を
    /// マージできる。同名は後勝ち。
    pub fn extend_pilots(&mut self, pilots: Vec<PilotData>) {
        for p in pilots {
            if let Some(slot) = self.pilots.iter_mut().find(|e| e.name == p.name) {
                *slot = p;
            } else {
                self.pilots.push(p);
            }
        }
    }

    /// ユニット定義を加法的に取り込む。元 `UDList.Load` 相当。複数の
    /// unit.txt をマージできる。同名は後勝ち。
    pub fn extend_units(&mut self, units: Vec<UnitData>) {
        for u in units {
            if let Some(slot) = self.units.iter_mut().find(|e| e.name == u.name) {
                *slot = u;
            } else {
                self.units.push(u);
            }
        }
    }

    /// マップを差し替え。元 `Map.bas::LoadMapData` の結果代入相当。
    pub fn replace_map(&mut self, map: MapData) {
        self.map = Some(map);
    }

    /// マップ上のユニット実体を差し替え。元 `UList` リセット相当。
    pub fn replace_unit_instances(&mut self, instances: Vec<UnitInstance>) {
        self.unit_instances = instances;
        self.rebuild_pos_index();
    }

    /// uid のユニットの移動到達範囲 (タイル → 残コスト)。地形適応・搭乗種別・
    /// 特殊能力・現在エリア・装備込み移動力を考慮する。**移動判定 (try_move_unit_to)
    /// と描画 (move range overlay) で同一ロジックを使う**ことで「表示と実際の
    /// 移動可能範囲が食い違う / 形状が変」問題を防ぐ。
    pub fn unit_move_range(&self, uid: &str) -> std::collections::HashMap<(u32, u32), i32> {
        let (Some(map), Some(u)) = (self.map.as_ref(), self.unit_by_uid(uid)) else {
            return std::collections::HashMap::new();
        };
        // 移動不能 (捕縛/麻痺/凍結/石化/止 等) のユニットは移動範囲を持たない。
        if u.move_disabled() {
            return std::collections::HashMap::new();
        }
        let Some(def) = self.unit_by_name(&u.unit_data_name) else {
            return std::collections::HashMap::new();
        };
        let mp = self.effective_speed(u);
        let active_feature_names: Vec<String> =
            u.active_features.iter().map(|f| f.name.clone()).collect();
        let terrain_adapt_names: Vec<String> = u
            .active_features
            .iter()
            .filter(|f| f.is_active && f.name == "地形適応")
            .flat_map(|f| {
                // value = "別名 地形名称1 地形名称2..." (先頭トークンは別名)
                f.value
                    .split_whitespace()
                    .skip(1)
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
            })
            .collect();
        let cost_fn = crate::movement::make_unit_cost_fn(
            self.terrains.clone(),
            def.transportation.clone(),
            def.adaption.0,
            u.current_area.clone(),
            active_feature_names,
            terrain_adapt_names,
        );
        crate::movement::compute_range_with(map, (u.x, u.y), mp, cost_fn)
    }

    /// 同じマスにいるユニットを返す（重複配置の検出にも使う）。
    /// `off_map` (Escape 退避中) のユニットは除外する。
    pub fn units_at(&self, x: u32, y: u32) -> impl Iterator<Item = &UnitInstance> + '_ {
        self.unit_instances
            .iter()
            .filter(move |u| !u.off_map && u.x == x && u.y == y)
    }

    /// 名前で検索（元 `PDList.Item(Name)` 相当のうち Name 一致のみ）。
    /// 正式名で照合し、無ければ愛称 (nickname) でも照合する。
    /// `.eve` の `Talk <話者>` は愛称 (例: "パチュリー") を使うことが多く、
    /// パイロットの正式名 (例: "パチュリー・ノーレッジ") とは一致しないため、
    /// 顔グラ等の解決で愛称フォールバックが要る。
    pub fn pilot_by_name(&self, name: &str) -> Option<&PilotData> {
        self.pilots
            .iter()
            .find(|p| p.name == name)
            .or_else(|| self.pilots.iter().find(|p| p.nickname == name))
    }

    /// Find a pilot instance by ID.
    pub fn pilot_instance_by_id(&self, id: &str) -> Option<&PilotInstance> {
        self.pilot_instances.iter().find(|p| p.id == id)
    }

    /// PilotInstance のレベルアップ済みスタットを PilotData として返す。
    /// PilotInstance が見つかれば、infight/shooting/hit/dodge/intuition/technique を
    /// インスタンス値で上書きした PilotData を生成して返す。見つからなければ
    /// 静的 PilotData をそのまま返す (クローン)。
    /// 戦闘計算の際に PilotData の代わりにこの値を使うことで
    /// レベルアップ後のスタットが反映される。
    /// PilotInstance のレベルアップ済みスタットを PilotData として返す。
    /// 優先度:
    /// 1. `PilotInstance` が存在する場合 → `infight/shooting/hit/dodge/intuition/technique` を
    ///    インスタンス値で上書きした PilotData を返す。
    /// 2. `UnitInstance.total_exp` から level を計算し `apply_stat_growth` 相当で補正した
    ///    PilotData を返す (PilotInstance 未生成の場合でもレベルアップ後スタットが反映される)。
    /// 3. どちらも無ければ静的 PilotData をそのまま返す。
    pub fn effective_pilot_data(&self, pilot_name: &str) -> Option<crate::data::pilot::PilotData> {
        let data = self.pilot_by_name(pilot_name)?.clone();
        // 1) PilotInstance が存在すれば優先
        if let Some(inst) = self
            .pilot_instances
            .iter()
            .find(|p| p.pilot_data_name == pilot_name || p.id == pilot_name)
        {
            return Some(crate::data::pilot::PilotData {
                infight: inst.infight,
                shooting: inst.shooting,
                hit: inst.hit,
                dodge: inst.dodge,
                intuition: inst.intuition,
                technique: inst.technique,
                ..data
            });
        }
        // 2) UnitInstance.total_exp から level を算出し成長補正
        if let Some(unit_inst) = self
            .unit_instances
            .iter()
            .find(|u| u.pilot_name == pilot_name)
        {
            return Some(grown_pilot(&data, unit_inst.total_exp));
        }
        Some(data)
    }

    /// Find a mutable pilot instance by ID.
    pub fn pilot_instance_by_id_mut(&mut self, id: &str) -> Option<&mut PilotInstance> {
        self.pilot_instances.iter_mut().find(|p| p.id == id)
    }

    /// Create a pilot instance from static data and add it.
    pub fn create_pilot_instance(
        &mut self,
        pilot_data_name: impl Into<String>,
        id: impl Into<String>,
    ) -> Option<&PilotInstance> {
        let name = pilot_data_name.into();
        if let Some(pdata) = self.pilot_by_name(&name) {
            let instance = PilotInstance::from_data(&name, id.into(), pdata);
            self.pilot_instances.push(instance);
            self.pilot_instances.last()
        } else {
            None
        }
    }

    pub fn unit_by_name(&self, name: &str) -> Option<&UnitData> {
        self.units.iter().find(|u| u.name == name)
    }

    /// アイテム名から `ItemData` を引く。
    pub fn item_by_name(&self, name: &str) -> Option<&ItemData> {
        self.items.iter().find(|i| i.name == name)
    }

    /// 装備品の総合計を引数 closure で集計。`base` に対し各装備品 `ItemData` が
    /// 持つ補正値を加算した値を返す。
    fn equipped_sum<T, F>(&self, u: &UnitInstance, base: T, f: F) -> T
    where
        T: std::ops::Add<Output = T> + Copy,
        F: Fn(&ItemData) -> T,
    {
        let mut acc = base;
        for slot in &u.item_slots {
            if let Some(name) = slot.equipped_item.as_deref() {
                if let Some(it) = self.item_by_name(name) {
                    acc = acc + f(it);
                }
            }
        }
        acc
    }

    /// 装備込みの最大 HP。
    pub fn effective_max_hp(&self, u: &UnitInstance) -> i64 {
        let base = self
            .unit_by_name(&u.unit_data_name)
            .map(|d| d.hp)
            .unwrap_or(0);
        let with_items = self.equipped_sum(u, base, |it| it.hp_mod);
        let hp = with_items + i64::from(u.upgrade_level) * UPGRADE_HP_PER_LEVEL;
        if u.boss_rank > 0 {
            let (num, add) = boss_hp_boost(u.boss_rank);
            hp * num / 2 + add
        } else {
            hp
        }
    }

    /// 装備込みの最大 EN。
    pub fn effective_max_en(&self, u: &UnitInstance) -> i32 {
        let base = self
            .unit_by_name(&u.unit_data_name)
            .map(|d| d.en)
            .unwrap_or(0);
        let with_items = self.equipped_sum(u, base, |it| it.en_mod);
        with_items + u.upgrade_level * UPGRADE_EN_PER_LEVEL
    }

    /// 装備込みの装甲。
    pub fn effective_armor(&self, u: &UnitInstance) -> i64 {
        let base = self
            .unit_by_name(&u.unit_data_name)
            .map(|d| d.armor)
            .unwrap_or(0);
        let with_items = self.equipped_sum(u, base, |it| it.armor_mod);
        with_items
            + i64::from(u.upgrade_level) * UPGRADE_ARMOR_PER_LEVEL
            + boss_armor_boost(u.boss_rank)
    }

    /// 装備込みの運動性。
    pub fn effective_mobility(&self, u: &UnitInstance) -> i32 {
        let base = self
            .unit_by_name(&u.unit_data_name)
            .map(|d| d.mobility)
            .unwrap_or(0);
        let with_items = self.equipped_sum(u, base, |it| it.mobility_mod);
        with_items + u.upgrade_level * UPGRADE_MOBILITY_PER_LEVEL + boss_mobility_boost(u.boss_rank)
    }

    /// 装備込みの移動力。
    pub fn effective_speed(&self, u: &UnitInstance) -> i32 {
        let base = self
            .unit_by_name(&u.unit_data_name)
            .map(|d| d.speed)
            .unwrap_or(0);
        let with_items = self.equipped_sum(u, base, |it| it.speed_mod);
        // 精神コマンド「加速」(+2) / 「神速」(+3) による移動力ボーナス。
        // 発動陣営の次フェイズ開始まで lifetime=1 の condition として保持される。
        let spirit_bonus = if u.has_condition("神速") {
            3
        } else if u.has_condition("加速") {
            2
        } else {
            0
        };
        // 状態異常: 移動力ＵＰ (+1) / 移動力ＤＯＷＮ (半減、特殊効果攻撃属性 低移)。
        let mut sp = with_items + spirit_bonus;
        if u.has_condition("移動力ＵＰ") {
            sp += 1;
        }
        if u.has_condition("移動力ＤＯＷＮ") {
            sp = (sp / 2).max(1);
        }
        sp.max(0)
    }

    /// 技能・特殊能力・状態異常による戦闘ボーナス (格闘/射撃/命中/回避/装甲) を集計する。
    /// レベル成長は `effective_pilot_data` 側、装備補正は `effective_armor` 等で扱うため
    /// ここでは扱わない (二重加算しない)。元 `UnitInstance::update` のボーナス計算を純関数化。
    pub fn combat_bonuses(&self, inst: &UnitInstance) -> CombatBonuses {
        let mut b = CombatBonuses::default();
        // パイロット技能 (格闘L3 → +30 等) と静的パイロット特殊能力。
        for pilot_id in &inst.pilot_ids {
            if let Some(pi) = self.pilot_instance_by_id(pilot_id) {
                b.infight += pi.skill_level("格闘") * 10;
                b.shooting += pi.skill_level("射撃") * 10;
                b.hit += pi.skill_level("命中") * 10;
                b.dodge += pi.skill_level("回避") * 10;
            }
            if let Some(pd) = self.pilot_by_name(pilot_id) {
                for (feat, _) in &pd.features {
                    if feat.contains("格闘UP") || feat.contains("格闘強化") {
                        b.infight += 20;
                    }
                    if feat.contains("射撃UP") || feat.contains("射撃強化") {
                        b.shooting += 20;
                    }
                }
            }
        }
        // 状態異常 (装甲低下 / 命中低下 / 回避低下)。
        for cond in &inst.conditions {
            for eff in cond.effects() {
                match eff {
                    crate::condition::ConditionEffect::HitDown { amount } => b.hit -= amount,
                    crate::condition::ConditionEffect::DodgeDown { amount } => b.dodge -= amount,
                    crate::condition::ConditionEffect::ArmorDown { amount } => {
                        b.armor -= amount as i64
                    }
                    _ => {}
                }
            }
        }
        // ユニット側の有効な特殊能力 (格闘強化 / 射撃強化 / 装甲強化)。
        for feat in &inst.active_features {
            if !feat.is_active {
                continue;
            }
            if feat.name.contains("格闘強化") || feat.name.contains("格闘UP") {
                b.infight += 20;
            }
            if feat.name.contains("射撃強化") || feat.name.contains("射撃UP") {
                b.shooting += 20;
            }
            if feat.name.contains("装甲強化") || feat.name.contains("装甲UP") {
                b.armor += 200;
            }
        }
        // 底力 / 超底力 (パイロット主技能): 現 HP が最大 HP の 1/4 以下のとき命中・回避を
        // 底上げする (`Unit.cs` 命中率計算: 底力=+30 / 超底力=+50)。combat::predict は攻撃側の
        // `hit` を加算し防御側の `dodge` を減算するため、命中・回避の双方へ同値を足すことで
        // 「攻撃時は当てやすく、防御時は避けやすい」原典の対称な効果を再現する。
        let max_hp = self.effective_max_hp(inst);
        if max_hp > 0 && (max_hp - inst.damage) * 4 <= max_hp {
            let guts = self.guts_hit_bonus(inst);
            b.hit += guts;
            b.dodge += guts;
        }
        b
    }

    /// 主パイロット (`pilot_ids` 先頭、無ければ `pilot_name`) の 底力 / 超底力 技能による
    /// 命中・回避ボーナス値を返す (超底力=50 / 底力=30 / 無し=0)。HP 1/4 以下での発動判定は
    /// 呼び出し側 (`combat_bonuses`) で行う。`超底力` は `底力` を含むため超底力を先に判定する。
    fn guts_hit_bonus(&self, inst: &UnitInstance) -> i32 {
        let main = inst
            .pilot_ids
            .first()
            .map(String::as_str)
            .unwrap_or(inst.pilot_name.as_str());
        // PilotInstance があれば取り込み済み技能 (from_data が 底力 を skills 化) を見る。
        if let Some(pi) = self.pilot_instance_by_id(main) {
            if pi.has_skill("超底力") {
                return 50;
            }
            if pi.has_skill("底力") {
                return 30;
            }
            return 0;
        }
        // PilotInstance 無し → 静的 PilotData の features をフォールバック参照する。
        if let Some(pd) = self
            .pilot_by_name(main)
            .or_else(|| self.pilot_by_name(&inst.pilot_name))
        {
            if pd.features.iter().any(|(f, _)| f.contains("超底力")) {
                return 50;
            }
            if pd.features.iter().any(|(f, _)| f.contains("底力")) {
                return 30;
            }
        }
        0
    }

    /// 戦闘予測 (`combat::predict*`) に渡す「実行時実効値込み」の `(PilotData, UnitData)`
    /// を `idx` のユニットについて構築する。静的データではなく実行時の値を反映する:
    /// パイロットはレベル成長 (`PilotInstance` または `total_exp` 由来) + 技能/特殊能力/
    /// 状態異常ボーナス、ユニットは強化パーツ込みの armor / mobility / hp / en + 状態異常・
    /// 特殊能力の装甲補正。これにより改造・強化パーツ・育成・装甲低下デバフ等が戦闘に効く。
    pub fn effective_combat_data(&self, idx: usize) -> Option<(PilotData, UnitData)> {
        let inst = self.unit_instances.get(idx)?;
        let base_pilot = self.pilot_by_name(&inst.pilot_name)?;
        // パイロットのレベル成長は、当該インスタンス固有の PilotInstance を最優先で、
        // 無ければインスタンス自身の total_exp から算出する (同名パイロット共有時の
        // 取り違えを防ぐため、名前ベースの effective_pilot_data は使わない)。
        let mut pilot = if let Some(pi) = inst
            .pilot_ids
            .iter()
            .find_map(|id| self.pilot_instances.iter().find(|p| &p.id == id))
        {
            PilotData {
                infight: pi.infight,
                shooting: pi.shooting,
                hit: pi.hit,
                dodge: pi.dodge,
                intuition: pi.intuition,
                technique: pi.technique,
                ..base_pilot.clone()
            }
        } else {
            grown_pilot(base_pilot, inst.total_exp)
        };
        let mut unit = self.unit_by_name(&inst.unit_data_name)?.clone();
        unit.armor = self.effective_armor(inst);
        unit.mobility = self.effective_mobility(inst);
        unit.hp = self.effective_max_hp(inst);
        unit.en = self.effective_max_en(inst);
        // 機体改造 (Rank) による武器攻撃力の加算。SRC.NET `Unit.cs` UpdateWeaponPower 準拠:
        //   - 攻撃力 0 の武器は据え置き (`Unit.cs:4094`「もともと攻撃力が0の武器は0に固定」)。
        //   - 固 (固定ダメージ) 武器は加算なし (`Unit.cs:4407`)。
        //   - Ｒ / 改 属性: `<attr>L<n>` のレベル指定があれば +10×n×Rank、無ければ +50×Rank
        //     (`Unit.cs:4422/4466/4513/4518`)。
        //   - それ以外の通常武器: +100×Rank (`Unit.cs:4551`)。
        // 旧実装は `+base×10%×Rank` (乗算) で、C# の **加算** 方式と乖離していた
        // (改造ユニットの攻撃ダメージが base>1000 の武器で過大、固定ダメージ武器も誤増加)。
        // V-UP アイテム (num) は未モデルのため Rank のみ。
        if inst.upgrade_level > 0 {
            let lv = i64::from(inst.upgrade_level);
            for w in &mut unit.weapons {
                if w.power == 0 || w.class.contains('固') {
                    continue;
                }
                let boost = if w.class.contains('Ｒ') {
                    match weapon_class_level(&w.class, 'Ｒ') {
                        Some(n) => (10.0 * n * lv as f64) as i64,
                        None => 50 * lv,
                    }
                } else if w.class.contains('改') {
                    match weapon_class_level(&w.class, '改') {
                        Some(n) => (10.0 * n * lv as f64) as i64,
                        None => 50 * lv,
                    }
                } else {
                    100 * lv
                };
                w.power += boost;
            }
        }
        // ボスランク: 全武器の攻撃力に加算 (BossRankコマンド.md)。
        let boss_atk = boss_attack_boost(inst.boss_rank);
        if boss_atk > 0 {
            for w in &mut unit.weapons {
                w.power += boss_atk;
            }
        }
        let b = self.combat_bonuses(inst);
        pilot.infight += b.infight;
        pilot.shooting += b.shooting;
        pilot.hit += b.hit;
        pilot.dodge += b.dodge;
        unit.armor = (unit.armor + b.armor).max(0);
        Some((pilot, unit))
    }
}

/// ボスランク (1〜5) による HP 補正 `(分子, 分母=2, 加算)`。`BossRankコマンド.md` の
/// 通常ユニット基準: rank1=×1.5 / rank2=×2 / rank3-5=×2 に加え flat (+10000/+20000/+40000)。
pub fn boss_hp_boost(rank: i32) -> (i64, i64) {
    match rank {
        1 => (3, 0),
        2 => (4, 0),
        3 => (4, 10000),
        4 => (4, 20000),
        5 => (4, 40000),
        _ => (2, 0),
    }
}
/// ボスランクによる装甲加算 (`BossRankコマンド.md` 通常ユニット基準)。
pub fn boss_armor_boost(rank: i32) -> i64 {
    match rank {
        1 => 300,
        2 => 600,
        3 => 1000,
        4 => 1500,
        5 => 2500,
        _ => 0,
    }
}
/// ボスランクによる運動性加算。
pub fn boss_mobility_boost(rank: i32) -> i32 {
    match rank {
        1 => 5,
        2 => 10,
        3 => 15,
        4 => 20,
        5 => 25,
        _ => 0,
    }
}
/// 武器 class 文字列から `<attr>L<number>` 形式のレベル指定を読む
/// (SRC.NET `Unit.WeaponLevel` 準拠)。例: `class="ＲL3"`, `attr='Ｒ'` → `Some(3.0)`。
/// `<attr>L` が無ければ `None` (= レベル未指定)。数値部は `0-9 . -` を読む。
fn weapon_class_level(class: &str, attr: char) -> Option<f64> {
    let needle = format!("{attr}L");
    let start = class.find(&needle)? + needle.len();
    let num: String = class[start..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    num.parse::<f64>().ok()
}

/// ボスランクによる攻撃力加算 (全武器の power に加算)。
pub fn boss_attack_boost(rank: i32) -> i64 {
    match rank {
        1 => 100,
        2 => 200,
        3..=5 => 300,
        _ => 0,
    }
}

/// 機体改造の上限段階。
pub const UPGRADE_MAX_LEVEL: i32 = 10;
/// 機体改造 1 段階あたりの最大 HP 上昇量。
/// SRC 原典 `Unit.cls:1719` `lngMaxHP = .HP + 200 * Rank` 準拠 (+200/段)。
pub const UPGRADE_HP_PER_LEVEL: i64 = 200;
/// 機体改造 1 段階あたりの最大 EN 上昇量。SRC `Unit.cls:1720` `.EN + 10 * Rank`。
pub const UPGRADE_EN_PER_LEVEL: i32 = 10;
/// 機体改造 1 段階あたりの装甲上昇量。
/// SRC 原典 `Unit.cls:1721` `lngArmor = .Armor + 100 * Rank` 準拠 (+100/段)。
pub const UPGRADE_ARMOR_PER_LEVEL: i64 = 100;
/// 機体改造 1 段階あたりの運動性上昇量。
pub const UPGRADE_MOBILITY_PER_LEVEL: i32 = 5;

/// 技能・特殊能力・状態異常による戦闘ステータス補正の集計値。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct CombatBonuses {
    pub infight: i32,
    pub shooting: i32,
    pub hit: i32,
    pub dodge: i32,
    pub armor: i64,
}

/// `base` パイロットに `total_exp` 由来のレベル成長を適用した `PilotData` を返す。
/// レベルは `pilot_instance::level_from_exp` (SRC 原典: 500 exp/level、1..=99)。
///
/// 成長式は VB6 原典 `Pilot.cls:582-593` 準拠: **`lv = Level`**(level-1 ではなく Level そのもの。
/// レベル 1 でも成長する)、格闘/射撃/技量/反応 `+= lv`、命中/回避 `+= 2*lv`。差分オラクル
/// placeunit で C# SRCCore と一致を確認 (人工知能 lv10 格闘=110・命中=165・反応=90)。成長スキル
/// (`格闘成長` 等)・`追加レベル`・`攻撃力低成長` Option は未モデル (素の式)。
/// `pilot_instance::apply_stat_growth` と同一の式に保つこと。
pub fn grown_pilot(base: &PilotData, total_exp: i32) -> PilotData {
    let lv = crate::pilot_instance::level_from_exp(total_exp);
    PilotData {
        infight: base.infight + lv,
        shooting: base.shooting + lv,
        hit: base.hit + lv * 2,
        dodge: base.dodge + lv * 2,
        intuition: base.intuition + lv,
        technique: base.technique + lv,
        ..base.clone()
    }
}
