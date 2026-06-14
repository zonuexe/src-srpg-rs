//! ユニットの移動範囲計算 / Unit movement range computation.
//!
//! 元 SRC では `Unit.cls` 内のメソッド群（`MoveCost` プロパティと
//! `Map.bas::SearchRoute` 系）が同等の処理を担う。
//! ここでは「開始マスから始めて 4 方向に Dijkstra で広げ、`max_mp` 以内に
//! 到達できるマス」を返す関数を提供する。Diagonal は許さない（元 SRC と同じ）。
//!
//! ## 地形適応 (terrain adaptation)
//!
//! `make_unit_cost_fn` は UnitData の `transportation`/`adaption` と
//! `UnitInstance.active_features` を参照し、SRC 仕様の移動コスト計算を行う
//! クロージャを返す。 `compute_range_with` の `cost_fn` 引数に渡して使う。
//!
//! 元実装参照: `Map.cs::AreaInSpeed` の move_area × TerrainClass マトリクス。

use std::collections::{BinaryHeap, HashMap};

use crate::data::map::MapData;
use crate::data::terrain;
use crate::data::terrain_file::TerrainEntry;

/// `start` から `max_mp` の移動ポイントで到達可能なマス一覧と、到達時の残 MP。
/// 侵入コストは `cost_fn(terrain_id)` で得る (シナリオ terrain.txt の値を
/// 優先したい呼出側用)。`0` 以下は通行可能 (コスト 0)、極大値 (例 9999) は
/// 実質通行不能。
pub fn compute_range_with<F: Fn(u32) -> i32>(
    map: &MapData,
    start: (u32, u32),
    max_mp: i32,
    cost_fn: F,
) -> HashMap<(u32, u32), i32> {
    let mut best: HashMap<(u32, u32), i32> = HashMap::new();
    if start.0 >= map.width || start.1 >= map.height || max_mp < 0 {
        return best;
    }
    best.insert(start, max_mp);
    let mut heap: BinaryHeap<(i32, u32, u32)> = BinaryHeap::new();
    heap.push((max_mp, start.0, start.1));

    while let Some((rem, x, y)) = heap.pop() {
        if best.get(&(x, y)).copied().unwrap_or(i32::MIN) > rem {
            continue;
        }
        for (nx, ny) in neighbors(map, x, y) {
            let cost = cost_fn(map.cell(nx, ny).terrain_id);
            if cost <= 0 {
                continue;
            }
            let nrem = rem - cost;
            if nrem < 0 {
                continue;
            }
            let prev = best.get(&(nx, ny)).copied().unwrap_or(i32::MIN);
            if nrem > prev {
                best.insert((nx, ny), nrem);
                heap.push((nrem, nx, ny));
            }
        }
    }
    best
}

/// `compute_range*` の到達コスト表 (start からの最小移動コスト) から、`dest` まで
/// の経路を 1 つ復元する (start..=dest)。移動アニメ (チップのスライド) 用。
///
/// `compute_range*` の表は各マスの値が「start からそこへ到達した時点の**残り移動力**」
/// (start が最大、遠いほど小さい)。よって `dest` から 4 近傍のうち残り移動力が厳密に
/// 大きいマスへ昇って行けば必ず start (残り = max) に至る。各ステップで残り最大の近傍を
/// 選ぶことで最短経路の 1 つを逆順に得て、反転して返す。
///
/// `dest` が到達表に無い / 昇れない場合は途中までの経路 (最低でも `[dest]`) を返す。
pub fn reconstruct_path(
    reachable: &HashMap<(u32, u32), i32>,
    start: (u32, u32),
    dest: (u32, u32),
) -> Vec<(u32, u32)> {
    if dest == start {
        return vec![start];
    }
    let mut path = vec![dest];
    let mut cur = dest;
    let max_steps = reachable.len() + 2;
    for _ in 0..max_steps {
        if cur == start {
            break;
        }
        let cur_rem = reachable.get(&cur).copied().unwrap_or(i32::MIN);
        let neighbors = [
            (cur.0.wrapping_sub(1), cur.1),
            (cur.0 + 1, cur.1),
            (cur.0, cur.1.wrapping_sub(1)),
            (cur.0, cur.1 + 1),
        ];
        let next = neighbors
            .into_iter()
            .filter(|n| cur.0 != 0 || n.0 != u32::MAX) // x=0 の左隣 (wrap) を除外
            .filter(|n| cur.1 != 0 || n.1 != u32::MAX) // y=0 の上隣 (wrap) を除外
            .filter_map(|n| reachable.get(&n).map(|c| (n, *c)))
            .filter(|(_, rem)| *rem > cur_rem)
            .max_by_key(|(_, rem)| *rem);
        match next {
            Some((n, _)) => {
                path.push(n);
                cur = n;
            }
            None => break,
        }
    }
    path.reverse();
    path
}

/// 既存呼出側の互換用ラッパ: ビルトイン `terrain::lookup` を使う。
/// 新規コードは `compute_range_with` を直接使うことを推奨。
pub fn compute_range(map: &MapData, start: (u32, u32), max_mp: i32) -> HashMap<(u32, u32), i32> {
    compute_range_with(map, start, max_mp, |id| {
        terrain::lookup(id).map(|t| t.move_cost).unwrap_or(0)
    })
}

// ============================================================
//  地形適応対応のコスト関数ビルダ
// ============================================================

/// ユニットの現在移動領域。元 SRC の `Unit.Area` 相当。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveArea {
    /// 空中 — 飛行ユニット。
    Air,
    /// 地上 — 一般地上ユニット。
    Ground,
    /// 水上 — 水上移動ユニット。
    Surface,
    /// 水中 — 潜水ユニット。
    Underwater,
    /// 宇宙 — 宇宙専用ユニット。
    Space,
}

/// SRC 地形適応文字 (A/B/C/D/S/-) を数値スコアに変換。
/// 元実装: `Unit.cs::get_Adaption` の switch 式。
/// S=5, A=4, B=3, C=2, D=1, '-'=0 (侵入不可)。
fn adaption_value(b: u8) -> u8 {
    match b {
        b'S' => 5,
        b'A' => 4,
        b'B' => 3,
        b'C' => 2,
        b'D' => 1,
        _ => 0,
    }
}

/// 生の地形クラス文字列を SRC 正規クラスに正規化する。
///
/// 組み込み地形 (`data/terrain.rs`) の class は "平地"/"海" など日本語名を使う。
/// シナリオ terrain.txt (`data/terrain_file.rs`) は "陸"/"水"/"深水"/"空"/"宇宙" を使う。
/// 両方を統一して移動コスト判定できるようここで正規化する。
pub fn normalize_terrain_class(class: &str) -> &str {
    match class {
        // 組み込み地形の日本語名 → SRC クラス
        "平地" | "道路" | "森林" | "山" | "都市" | "屋内" => "陸",
        "海" => "水",
        // terrain_file.rs で既に正規化済みの SRC クラスはそのまま
        "陸" | "水" | "深水" | "空" | "宇宙" | "月面" => class,
        // 不明は陸として扱う
        _ => "陸",
    }
}

/// 通行不能コスト (9999)。`terrain_file.rs` の "-" move_cost と同じ値。
const BLOCKED: i32 = 9999;

// --- 領域別コスト計算 ---

fn cost_air(tc: &str, base: i32, is_adaptable_in_space: bool) -> i32 {
    match tc {
        // 空クラスは通常コスト
        "空" => base,
        // 宇宙クラスは宇宙適応があれば通常コスト、なければ侵入不可
        "宇宙" => {
            if is_adaptable_in_space {
                base
            } else {
                BLOCKED
            }
        }
        // その他地形 (陸/水/深水/月面 等) は飛行して通過: コスト min(base, 2)
        _ => base.min(2),
    }
}

#[allow(clippy::too_many_arguments)]
fn cost_ground(
    tc: &str,
    base: i32,
    is_trans_on_ground: bool,
    is_trans_in_water: bool,
    is_trans_on_water: bool,
    is_adaptable_in_water: bool,
    is_swimable: bool,
    is_adaptable_in_space: bool,
) -> i32 {
    match tc {
        "陸" | "屋内" | "月面" => {
            if is_trans_on_ground {
                base
            } else {
                BLOCKED
            }
        }
        "水" => {
            if is_trans_in_water || is_trans_on_water {
                2
            } else if is_adaptable_in_water {
                base
            } else {
                BLOCKED
            }
        }
        "深水" => {
            if is_trans_in_water || is_trans_on_water {
                2
            } else if is_swimable {
                base
            } else {
                BLOCKED
            }
        }
        "空" => BLOCKED,
        "宇宙" => {
            if is_adaptable_in_space {
                base
            } else {
                BLOCKED
            }
        }
        _ => base,
    }
}

fn cost_surface(tc: &str, base: i32, is_trans_on_ground: bool, is_adaptable_in_space: bool) -> i32 {
    match tc {
        "陸" | "屋内" | "月面" => {
            if is_trans_on_ground {
                base
            } else {
                BLOCKED
            }
        }
        "水" | "深水" => 2,
        "空" => BLOCKED,
        "宇宙" => {
            if is_adaptable_in_space {
                base
            } else {
                BLOCKED
            }
        }
        _ => base,
    }
}

fn cost_underwater(
    tc: &str,
    base: i32,
    is_trans_on_ground: bool,
    is_trans_in_water: bool,
    is_swimable: bool,
    is_adaptable_in_space: bool,
) -> i32 {
    match tc {
        "陸" | "屋内" | "月面" => {
            if is_trans_on_ground {
                base
            } else {
                BLOCKED
            }
        }
        "水" => {
            if is_trans_in_water {
                2
            } else {
                base
            }
        }
        "深水" => {
            if is_trans_in_water {
                2
            } else if is_swimable {
                base
            } else {
                BLOCKED
            }
        }
        "空" => BLOCKED,
        "宇宙" => {
            if is_adaptable_in_space {
                base
            } else {
                BLOCKED
            }
        }
        _ => base,
    }
}

fn cost_space(
    tc: &str,
    base: i32,
    is_trans_in_sky: bool,
    is_trans_on_ground: bool,
    is_trans_in_moon_sky: bool,
) -> i32 {
    match tc {
        "宇宙" => base,
        "陸" | "屋内" => {
            if is_trans_in_sky {
                2
            } else if is_trans_on_ground {
                base
            } else {
                BLOCKED
            }
        }
        "月面" => {
            if is_trans_in_moon_sky {
                2
            } else if is_trans_on_ground {
                base
            } else {
                BLOCKED
            }
        }
        _ => BLOCKED,
    }
}

/// ユニットの移動領域を `transportation`・`adaption`・`current_area` から推定する。
///
/// 元実装参照: `Unit.cs` L40622 (ユニットのいる地形は？) の区分判定。
/// 本実装では「ユニットが現在いる地形」を引数として取らず、
/// transportation の優先順位で判定する簡易版。
pub fn derive_unit_move_area(
    transportation: &str,
    adaption: &[u8; 4],
    current_area: &str,
    has_aerial_move: bool,
    has_surface_water_move: bool,
    has_underwater_move: bool,
    has_space_move: bool,
) -> MoveArea {
    // current_area が明示設定されていれば最優先
    if !current_area.is_empty() {
        return match current_area {
            "空中" => MoveArea::Air,
            "水上" => MoveArea::Surface,
            "水中" => MoveArea::Underwater,
            "宇宙" => MoveArea::Space,
            _ => MoveArea::Ground,
        };
    }

    let can_air = transportation.contains('空') || has_aerial_move;
    let can_ground = transportation.contains('陸');
    let can_space = transportation.contains("宇宙") || has_space_move;
    let can_water_surface = transportation.contains("水上") || has_surface_water_move;
    let can_water_sub =
        (transportation.contains('水') && !transportation.contains("水上")) || has_underwater_move;

    if can_air {
        // 地上移動もできるなら地形適応スコアを比較。空の方が高い(≥)なら空中。
        if !can_ground
            || adaption[1] == b'-'
            || adaption_value(adaption[0]) >= adaption_value(adaption[1])
        {
            return MoveArea::Air;
        }
    }
    if can_water_surface {
        return MoveArea::Surface;
    }
    if can_water_sub {
        return MoveArea::Underwater;
    }
    if can_space && !can_ground && !can_air {
        return MoveArea::Space;
    }
    MoveArea::Ground
}

/// ユニット属性を考慮した地形コストクロージャを返す。
///
/// 返値は `compute_range_with` の `cost_fn` 引数に直接渡せる。
///
/// # 引数
/// - `terrain_table` : シナリオ terrain.txt のエントリ一覧 (空でも可)。
/// - `transportation` : 元 `UnitData.transportation`（"陸"/"空"/"空陸宇宙" 等）。
/// - `adaption`       : 元 `UnitData.adaption.0` (0=空, 1=陸, 2=水, 3=宇宙)。
/// - `current_area`   : `UnitInstance.current_area`。空文字なら transportation から推定。
/// - `active_feature_names` : `UnitInstance.active_features` の name 一覧。
/// - `terrain_adapt_names`  : `地形適応` 特殊能力で指定された地形名称の一覧。
///   一致した地形の移動コストを 1 に上書き（侵入不可地形でも侵入可能にする）。
pub fn make_unit_cost_fn(
    terrain_table: Vec<TerrainEntry>,
    transportation: String,
    adaption: [u8; 4],
    current_area: String,
    active_feature_names: Vec<String>,
    terrain_adapt_names: Vec<String>,
) -> impl Fn(u32) -> i32 {
    // 特殊能力フラグを事前計算
    let hf = |name: &str| -> bool { active_feature_names.iter().any(|f| f == name) };
    let has_aerial_move = hf("空中移動");
    let has_land_move = hf("陸上移動");
    let has_surface_water_move = hf("水上移動") || hf("ホバー移動");
    let has_underwater_move = hf("水中移動");
    let has_space_move = hf("宇宙移動");
    let is_swimable = hf("水泳");

    // 特殊能力で拡張した実効 transportation
    let mut eff_trans = transportation.clone();
    if has_aerial_move && !eff_trans.contains('空') {
        eff_trans.push('空');
    }
    if has_land_move && !eff_trans.contains('陸') {
        eff_trans.push('陸');
    }
    if has_underwater_move && !eff_trans.contains('水') {
        eff_trans.push('水');
    }

    let can_air = eff_trans.contains('空');
    let can_ground = eff_trans.contains('陸');
    let can_space = eff_trans.contains("宇宙") || has_space_move;
    let can_water_surface = eff_trans.contains("水上") || has_surface_water_move;
    let can_water_sub =
        (eff_trans.contains('水') && !eff_trans.contains("水上")) || has_underwater_move;

    let adp = adaption;

    // 移動領域 (MoveArea) を決定
    let move_area = derive_unit_move_area(
        &eff_trans,
        &adp,
        &current_area,
        has_aerial_move,
        has_surface_water_move,
        has_underwater_move,
        has_space_move,
    );

    // 各能力フラグを事前計算
    let is_trans_on_ground = can_ground && adp[1] != b'-';
    let is_trans_in_water = can_water_sub && adp[2] != b'-';
    let is_trans_on_water = can_water_surface;
    let is_trans_in_sky = can_air && adp[0] != b'-';
    let is_trans_in_moon_sky = (can_air && adp[0] != b'-') || (can_space && adp[3] != b'-');
    let is_adaptable_in_water = adp[2] != b'-' || has_underwater_move;
    let is_adaptable_in_space = adp[3] != b'-' || has_space_move;

    // 地形適応名称セット (move closure に move される)
    let terrain_adapt_set: std::collections::HashSet<String> =
        terrain_adapt_names.into_iter().collect();

    move |id: u32| -> i32 {
        // シナリオ terrain.txt 優先 → 組み込みデフォルト
        let (terrain_name, raw_class, base) = terrain_table
            .iter()
            .find(|t| t.id == id)
            .map(|t| (t.name.as_str(), t.class.as_str(), t.move_cost))
            .or_else(|| {
                terrain::DEFAULT_TERRAINS
                    .iter()
                    .find(|t| t.id == id)
                    .map(|t| (t.name, t.class, t.move_cost))
            })
            .unwrap_or(("", "陸", 1));

        // 地形適応: 指定地形名に一致する場合、コスト = 1（侵入不可地形も侵入可能）
        if !terrain_adapt_set.is_empty() && terrain_adapt_set.contains(terrain_name) {
            return 1;
        }

        let tc = normalize_terrain_class(raw_class);

        match move_area {
            MoveArea::Air => cost_air(tc, base, is_adaptable_in_space),
            MoveArea::Ground => cost_ground(
                tc,
                base,
                is_trans_on_ground,
                is_trans_in_water,
                is_trans_on_water,
                is_adaptable_in_water,
                is_swimable,
                is_adaptable_in_space,
            ),
            MoveArea::Surface => cost_surface(tc, base, is_trans_on_ground, is_adaptable_in_space),
            MoveArea::Underwater => cost_underwater(
                tc,
                base,
                is_trans_on_ground,
                is_trans_in_water,
                is_swimable,
                is_adaptable_in_space,
            ),
            MoveArea::Space => cost_space(
                tc,
                base,
                is_trans_in_sky,
                is_trans_on_ground,
                is_trans_in_moon_sky,
            ),
        }
    }
}

fn neighbors(map: &MapData, x: u32, y: u32) -> impl Iterator<Item = (u32, u32)> + '_ {
    let mut out: Vec<(u32, u32)> = Vec::with_capacity(4);
    if x > 0 {
        out.push((x - 1, y));
    }
    if y > 0 {
        out.push((x, y - 1));
    }
    if x + 1 < map.width {
        out.push((x + 1, y));
    }
    if y + 1 < map.height {
        out.push((x, y + 1));
    }
    out.into_iter()
}

/// ユニットが特定地形クラス上に位置している場合の `Area()` 文字列を返す。
///
/// 元 SRC の `Unit.cs::SetAttributeAfterPlace` と同等の地形→領域マッピング。
/// `UnitInstance.current_area` が空の場合に `Area(unit)` 関数のフォールバックとして使う。
///
/// 返り値は SRC `Area()` 関数の仕様値: "地上" / "空中" / "水上" / "水中" / "宇宙"。
pub fn unit_area_on_terrain(
    terrain_class: &str,
    transportation: &str,
    adaption: &[u8; 4],
    active_feature_names: &[String],
) -> &'static str {
    let hf = |name: &str| -> bool { active_feature_names.iter().any(|f| f == name) };
    let can_air = transportation.contains('空') || hf("空中移動");
    let can_ground = transportation.contains('陸') || hf("陸上移動");
    let can_space = transportation.contains("宇宙") || hf("宇宙移動");
    let can_water_surface = transportation.contains("水上") || hf("水上移動") || hf("ホバー移動");

    let tc = normalize_terrain_class(terrain_class);

    // 空クラス地形 → 飛行ユニットのみいられる → 常に空中
    if tc == "空" {
        return "空中";
    }

    // 宇宙 / 月面
    if tc == "宇宙" || tc == "月面" {
        return if can_space || can_air {
            "宇宙"
        } else {
            "地上"
        };
    }

    // 水 / 深水
    if tc == "水" || tc == "深水" {
        // 空適応が陸適応以上なら空中
        if can_air
            && (!can_ground
                || adaption[1] == b'-'
                || adaption_value(adaption[0]) >= adaption_value(adaption[1]))
        {
            return "空中";
        }
        if can_water_surface {
            return "水上";
        }
        return "水中";
    }

    // 陸 / 屋内 / 月面 (地上系)
    if can_air
        && (!can_ground
            || adaption[1] == b'-'
            || adaption_value(adaption[0]) >= adaption_value(adaption[1]))
    {
        return "空中";
    }
    if can_ground {
        return "地上";
    }
    // 地上移動もできないなら空中扱い
    "空中"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::map::{MapCell, MapData};
    use crate::data::terrain_file::TerrainEntry;

    #[test]
    fn reconstruct_path_descends_cost_map_to_start() {
        // 5x1 の平地を Dijkstra して (0,0)→(4,0) の経路を復元。
        let map = MapData::new(5, 1);
        let reachable = compute_range(&map, (0, 0), 99);
        let path = reconstruct_path(&reachable, (0, 0), (4, 0));
        assert_eq!(path.first(), Some(&(0, 0)));
        assert_eq!(path.last(), Some(&(4, 0)));
        // 経路は隣接ステップの連続。
        for w in path.windows(2) {
            let d = (w[0].0 as i64 - w[1].0 as i64).abs() + (w[0].1 as i64 - w[1].1 as i64).abs();
            assert_eq!(d, 1, "経路が隣接していない: {:?}", w);
        }
    }

    #[test]
    fn reconstruct_path_same_start_dest_is_singleton() {
        let map = MapData::new(3, 3);
        let reachable = compute_range(&map, (1, 1), 99);
        assert_eq!(reconstruct_path(&reachable, (1, 1), (1, 1)), vec![(1, 1)]);
    }

    fn map_3x3_with(ids: [u32; 9]) -> MapData {
        let mut m = MapData::new(3, 3);
        for (i, id) in ids.iter().enumerate() {
            let x = (i % 3) as u32;
            let y = (i / 3) as u32;
            m.set_cell(
                x,
                y,
                MapCell {
                    terrain_id: *id,
                    bitmap_no: 0,
                },
            );
        }
        m
    }

    #[test]
    fn range_zero_only_start() {
        // 全マス平地（move_cost 1）, MP 0 → 自セルのみ
        let m = map_3x3_with([0; 9]);
        let r = compute_range(&m, (1, 1), 0);
        assert_eq!(r.len(), 1);
        assert_eq!(r.get(&(1, 1)), Some(&0));
    }

    #[test]
    fn range_one_expands_to_adjacent() {
        let m = map_3x3_with([0; 9]);
        let r = compute_range(&m, (1, 1), 1);
        // 中央 + 4 方向 = 5 マス
        assert_eq!(r.len(), 5);
        assert!(r.contains_key(&(0, 1)));
        assert!(r.contains_key(&(2, 1)));
        assert!(r.contains_key(&(1, 0)));
        assert!(r.contains_key(&(1, 2)));
    }

    #[test]
    fn range_blocked_by_sea() {
        // 4(海, move_cost=99) を右側に挟むと右へ抜けられない
        let m = map_3x3_with([0, 0, 0, 0, 0, 4, 0, 0, 0]);
        let r = compute_range(&m, (0, 1), 3);
        // 海マス自身は到達不能
        assert!(!r.contains_key(&(2, 1)));
    }

    #[test]
    fn forest_cost_two_limits_range() {
        // 中央が森 (move_cost=2)
        let m = map_3x3_with([0, 0, 0, 0, 2, 0, 0, 0, 0]);
        let r = compute_range(&m, (0, 1), 2);
        // 中央(森)に MP 0 で到達できるか? start (0,1) からは (1,1) へコスト 2 → rem 0
        assert_eq!(r.get(&(1, 1)), Some(&0));
        // 中央を抜けて (2,1) へはコスト 1 必要だが MP 0 → 不可
        assert!(!r.contains_key(&(2, 1)));
    }

    #[test]
    fn no_diagonal_movement() {
        let m = map_3x3_with([0; 9]);
        let r = compute_range(&m, (1, 1), 1);
        // 斜めは到達できない
        assert!(!r.contains_key(&(0, 0)));
        assert!(!r.contains_key(&(2, 2)));
    }

    // ============================================================
    //  make_unit_cost_fn — 地形適応テスト
    // ============================================================

    fn terrain_entry(id: u32, class: &str, move_cost: i32) -> TerrainEntry {
        TerrainEntry {
            id,
            name: class.to_string(),
            english: String::new(),
            class: class.to_string(),
            move_cost,
            hit_mod: 0,
            damage_mod: 0,
            features: vec![],
        }
    }

    /// 陸移動ユニット (水適応 '-') は "水" クラス地形に侵入できない (BLOCKED=9999)。
    #[test]
    fn ground_unit_blocked_by_water_terrain() {
        let table = vec![terrain_entry(0, "陸", 1), terrain_entry(1, "水", 2)];
        // adaption: 空=A, 陸=A, 水='-'(侵入不可), 宇宙='-'
        let cost_fn = make_unit_cost_fn(
            table,
            "陸".to_string(),
            *b"AA--",
            String::new(),
            vec![],
            vec![],
        );
        assert_eq!(cost_fn(0), 1); // 陸クラス → 通常コスト
        assert_eq!(cost_fn(1), BLOCKED); // 水クラス → 侵入不可
    }

    /// 空中移動ユニットは水マスを飛行コスト 2 で通過できる。
    #[test]
    fn air_unit_can_fly_over_water() {
        let table = vec![terrain_entry(0, "陸", 1), terrain_entry(1, "水", 3)];
        let cost_fn = make_unit_cost_fn(
            table,
            "空".to_string(),
            *b"AAAA",
            String::new(),
            vec![],
            vec![],
        );
        assert_eq!(cost_fn(0), 1); // 陸クラス → min(1, 2) = 1
        assert_eq!(cost_fn(1), 2); // 水クラス → min(3, 2) = 2
    }

    /// 空中移動ユニットは "空" クラス地形を通常コストで移動できる。
    #[test]
    fn air_unit_uses_normal_cost_on_air_terrain() {
        let table = vec![terrain_entry(0, "空", 1)];
        let cost_fn = make_unit_cost_fn(
            table,
            "空".to_string(),
            *b"AAAA",
            String::new(),
            vec![],
            vec![],
        );
        assert_eq!(cost_fn(0), 1);
    }

    /// 宇宙適応なし (adaption[3]=b'-') の空中ユニットは "宇宙" 地形に侵入不可。
    #[test]
    fn air_unit_without_space_adaption_blocked_in_space() {
        let table = vec![terrain_entry(0, "宇宙", 1)];
        let cost_fn = make_unit_cost_fn(
            table,
            "空".to_string(),
            *b"AAA-", // 宇宙 adaption = '-'
            String::new(),
            vec![],
            vec![],
        );
        assert_eq!(cost_fn(0), BLOCKED);
    }

    /// 宇宙適応あり (adaption[3]!=b'-') の空中ユニットは "宇宙" 地形を通常コストで移動。
    #[test]
    fn air_unit_with_space_adaption_enters_space() {
        let table = vec![terrain_entry(0, "宇宙", 1)];
        let cost_fn = make_unit_cost_fn(
            table,
            "空".to_string(),
            *b"AAAA", // 全適応 A
            String::new(),
            vec![],
            vec![],
        );
        assert_eq!(cost_fn(0), 1);
    }

    /// "空中移動" 特殊能力を持つユニットは空中移動ルールを使う。
    #[test]
    fn aerial_move_feature_enables_air_movement() {
        let table = vec![terrain_entry(0, "陸", 1), terrain_entry(1, "水", 3)];
        let cost_fn = make_unit_cost_fn(
            table,
            "陸".to_string(), // transportation = 陸 のみ
            *b"AAAA",
            String::new(),
            vec!["空中移動".to_string()], // feature で飛行
            vec![],
        );
        // 陸クラス → min(1, 2) = 1
        assert_eq!(cost_fn(0), 1);
        // 水クラス → 飛行 → min(3, 2) = 2 (侵入可)
        assert_eq!(cost_fn(1), 2);
    }

    /// 水中移動ユニット (transportation "水") は "水" クラス地形に侵入できる。
    #[test]
    fn water_unit_can_enter_water_terrain() {
        let table = vec![terrain_entry(0, "陸", 1), terrain_entry(1, "水", 2)];
        let cost_fn = make_unit_cost_fn(
            table,
            "水".to_string(),
            *b"-AAA", // 空 adaption = '-'
            String::new(),
            vec![],
            vec![],
        );
        // 陸は侵入不可 (is_trans_on_ground = false)
        assert_eq!(cost_fn(0), BLOCKED);
        // 水は侵入可 (is_trans_in_water = true → cost 2)
        assert_eq!(cost_fn(1), 2);
    }

    /// current_area="空中" が設定されているユニットは地上移動ルールではなく空中ルールを使う。
    #[test]
    fn current_area_override_forces_air_movement() {
        let table = vec![terrain_entry(0, "陸", 1), terrain_entry(1, "水", 3)];
        let cost_fn = make_unit_cost_fn(
            table,
            "陸".to_string(), // transportation = 陸
            *b"AAAA",
            "空中".to_string(), // override: 空中
            vec![],
            vec![],
        );
        // 水クラス → 空中飛行 → min(3, 2) = 2
        assert_eq!(cost_fn(1), 2);
    }

    /// 組み込みデフォルト地形 (terrain.rs) との統合: id=4 "海" は "水" クラスとして扱われ
    /// 水適応 '-' の陸移動ユニットには侵入不可。
    #[test]
    fn builtin_sea_terrain_blocked_for_ground_unit() {
        // 空のシナリオ terrain_table → フォールバックで組み込み地形 id=4 (海, move_cost=99)
        // adaption: 水='-' で水クラスへの適応なし
        let cost_fn = make_unit_cost_fn(
            vec![],
            "陸".to_string(),
            *b"AA--",
            String::new(),
            vec![],
            vec![],
        );
        // id=4 は組み込み "海" → normalize "水" → 地上ユニット (水適応なし) 侵入不可
        assert_eq!(cost_fn(4), BLOCKED);
    }

    /// 組み込み地形 id=4 "海" を空中ユニットは飛行で通過できる (min(99,2)=2)。
    #[test]
    fn builtin_sea_terrain_flyable_for_air_unit() {
        let cost_fn = make_unit_cost_fn(
            vec![],
            "空".to_string(),
            *b"AAAA",
            String::new(),
            vec![],
            vec![],
        );
        assert_eq!(cost_fn(4), 2);
    }

    // ============================================================
    //  unit_area_on_terrain テスト
    // ============================================================

    /// 陸移動ユニットが "陸" クラス地形にいる → "地上"。
    #[test]
    fn area_ground_unit_on_land_is_chijou() {
        let features: Vec<String> = vec![];
        assert_eq!(unit_area_on_terrain("陸", "陸", b"AA--", &features), "地上");
        // 組み込み "平地" も 陸クラス正規化 → "地上"
        assert_eq!(
            unit_area_on_terrain("平地", "陸", b"AA--", &features),
            "地上"
        );
    }

    /// 空移動ユニットが "陸" クラス地形にいる → "空中" (air adaption >= ground adaption)。
    #[test]
    fn area_air_unit_on_land_is_kuuchu() {
        let features: Vec<String> = vec![];
        // 空 adaption = A (4), 陸 adaption = A (4) → 4 >= 4 → 空中
        assert_eq!(unit_area_on_terrain("陸", "空", b"AAAA", &features), "空中");
    }

    /// 空移動ユニットが "水" クラス地形にいる → "空中"。
    #[test]
    fn area_air_unit_on_water_is_kuuchu() {
        let features: Vec<String> = vec![];
        assert_eq!(unit_area_on_terrain("水", "空", b"AAAA", &features), "空中");
    }

    /// 水移動ユニットが "水" クラス地形にいる → "水中"。
    #[test]
    fn area_water_unit_on_water_is_suichu() {
        let features: Vec<String> = vec![];
        // 水移動、空なし → 水中
        assert_eq!(unit_area_on_terrain("水", "水", b"-AAA", &features), "水中");
    }

    /// "空" クラス地形は常に "空中" (空移動ユニットのみいられる)。
    #[test]
    fn area_any_unit_on_sky_terrain_is_kuuchu() {
        let features: Vec<String> = vec![];
        assert_eq!(unit_area_on_terrain("空", "陸", b"AAAA", &features), "空中");
        assert_eq!(unit_area_on_terrain("空", "空", b"AAAA", &features), "空中");
    }

    /// "宇宙" クラス地形の空中ユニット → "宇宙"。
    #[test]
    fn area_air_unit_on_space_terrain_is_uchu() {
        let features: Vec<String> = vec![];
        assert_eq!(
            unit_area_on_terrain("宇宙", "空", b"AAAA", &features),
            "宇宙"
        );
    }

    /// "水上移動" 特殊能力を持つ陸ユニットが "水" クラス地形にいる → "水上"。
    #[test]
    fn area_surface_water_unit_on_water_is_suijou() {
        let features = vec!["水上移動".to_string()];
        assert_eq!(unit_area_on_terrain("水", "陸", b"AA--", &features), "水上");
    }

    /// 移動範囲計算との統合テスト: 空中ユニットは水マスを通過して反対側に到達できる。
    #[test]
    fn air_unit_range_crosses_water() {
        // 3x3 マップ: 中列 (x=1) が "水" クラス地形 (id=1, move_cost=3)
        // 空中ユニット: 飛行コスト min(3,2)=2
        let table = vec![terrain_entry(0, "陸", 1), terrain_entry(1, "水", 3)];
        // マップ: 全列 id=0 (陸) ただし x=1 の列は id=1 (水)
        let mut m = MapData::new(3, 3);
        for y in 0..3u32 {
            for x in 0..3u32 {
                let tid = if x == 1 { 1 } else { 0 };
                m.set_cell(
                    x,
                    y,
                    MapCell {
                        terrain_id: tid,
                        bitmap_no: 0,
                    },
                );
            }
        }
        let cost_fn = make_unit_cost_fn(
            table,
            "空".to_string(),
            *b"AAAA",
            String::new(),
            vec![],
            vec![],
        );
        // MP=3: 水マス cost=2, その先 cost=1 → rem=0 で到達
        let r = compute_range_with(&m, (0, 1), 3, cost_fn);
        assert!(r.contains_key(&(2, 1)), "空中ユニットは水越しに到達できる");
    }

    /// 移動範囲計算との統合テスト: 水適応なしの地上ユニットは水マスを越えられない。
    #[test]
    fn ground_unit_range_blocked_by_water() {
        let table = vec![terrain_entry(0, "陸", 1), terrain_entry(1, "水", 3)];
        let mut m = MapData::new(3, 3);
        for y in 0..3u32 {
            for x in 0..3u32 {
                let tid = if x == 1 { 1 } else { 0 };
                m.set_cell(
                    x,
                    y,
                    MapCell {
                        terrain_id: tid,
                        bitmap_no: 0,
                    },
                );
            }
        }
        // adaption: 水='-' → is_adaptable_in_water = false
        let cost_fn = make_unit_cost_fn(
            table,
            "陸".to_string(),
            *b"AA--",
            String::new(),
            vec![],
            vec![],
        );
        // MP=10: 水マスは BLOCKED → 右側 (x=2) に到達不可
        let r = compute_range_with(&m, (0, 1), 10, cost_fn);
        assert!(
            !r.contains_key(&(2, 1)),
            "地上ユニットは水マスを越えられない"
        );
        assert!(!r.contains_key(&(1, 1)), "水マス自体にも到達不可");
    }

    /// TerrainEntry を name と class を別々に指定して生成するヘルパー。
    fn terrain_entry_named(id: u32, name: &str, class: &str, move_cost: i32) -> TerrainEntry {
        TerrainEntry {
            id,
            name: name.to_string(),
            english: String::new(),
            class: class.to_string(),
            move_cost,
            hit_mod: 0,
            damage_mod: 0,
            features: vec![],
        }
    }

    /// 地形適応: 通常は侵入不可の水クラス地形でも、地形適応に指定した名称なら cost=1 で侵入可能。
    #[test]
    fn terrain_adapt_allows_normally_blocked_terrain() {
        // id=0 "陸地" (陸クラス, cost=1), id=1 "水田" (水クラス, cost=2)
        // 水田は水クラスなので水適応なし (adp[2]='-') の陸移動ユニットは通常 BLOCKED
        let mk_table = || {
            vec![
                terrain_entry_named(0, "陸地", "陸", 1),
                terrain_entry_named(1, "水田", "水", 2),
            ]
        };

        // 地形適応なし → 水田は侵入不可
        let cost_fn_no_adapt = make_unit_cost_fn(
            mk_table(),
            "陸".to_string(),
            *b"AA--",
            String::new(),
            vec![],
            vec![],
        );
        assert_eq!(cost_fn_no_adapt(0), 1); // 陸地 → 通常コスト
        assert_eq!(cost_fn_no_adapt(1), BLOCKED); // 水田 → 侵入不可

        // 地形適応に "水田" を指定 → cost=1 で侵入可能
        let cost_fn_with_adapt = make_unit_cost_fn(
            mk_table(),
            "陸".to_string(),
            *b"AA--",
            String::new(),
            vec![],
            vec!["水田".to_string()], // 地形適応
        );
        assert_eq!(cost_fn_with_adapt(0), 1); // 陸地 → 通常コスト
        assert_eq!(cost_fn_with_adapt(1), 1); // 水田 → 地形適応により cost=1
    }

    /// 地形適応: 指定名称に一致しない地形には通常ルールが適用される。
    #[test]
    fn terrain_adapt_does_not_affect_other_terrains() {
        let table = vec![
            terrain_entry_named(0, "陸地", "陸", 1),
            terrain_entry_named(1, "水田", "水", 2),
            terrain_entry_named(2, "沼地", "水", 3),
        ];
        // "水田" のみ適応指定。沼地 (水クラス) は依然 BLOCKED
        let cost_fn = make_unit_cost_fn(
            table,
            "陸".to_string(),
            *b"AA--",
            String::new(),
            vec![],
            vec!["水田".to_string()],
        );
        assert_eq!(cost_fn(1), 1); // 水田 → 適応済 cost=1
        assert_eq!(cost_fn(2), BLOCKED); // 沼地 → 適応外のまま BLOCKED
    }
}
