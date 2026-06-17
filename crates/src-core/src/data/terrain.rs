//! 地形データ / Terrain definition.
//!
//! 元実装: `TerrainData.cls`。元 SRC ではシナリオごとに `terrain.txt`
//! （または scenario の `<terrain>` セクション）から読み込むが、移植版では
//! 当面、組み込みのデフォルト地形カタログを使う。

/// 1 種類の地形 / One terrain definition.
#[derive(Debug, Clone)]
pub struct Terrain {
    /// 元: `.ID`
    pub id: u32,
    /// 元: `.Name`
    pub name: &'static str,
    /// 元: `.Class`
    pub class: &'static str,
    /// 元: `.MoveCost`
    pub move_cost: i32,
    /// 元: `.HitMod` (命中修正 / 回避修正)。SRC 規約で**正の値ほど被命中を下げる**
    /// (防御地形)。combat 側で `(100 - hit_mod)` として適用 (`combat.rs`)。
    pub hit_mod: i32,
    /// 元: `.DamageMod`
    pub damage_mod: i32,
    /// 移植版独自: タイル可視化用の塗り色（CSS カラー文字列）。
    pub color: &'static str,
    /// 移植版独自: タイル文字（タイル中央に重ねるアイコン的な 1 文字）。
    pub glyph: &'static str,
    /// 移植版独自: タイル画像のヒント。`Assets::find_image` で検索する。
    /// `Assets` に画像が無ければ `color`/`glyph` フォールバック。
    /// 候補は ',' で区切って複数指定可（例: "plain,平地"）。
    pub bitmap_hint: &'static str,
}

/// 移植版の組み込みデフォルト地形 / Built-in default terrain palette.
///
/// id は元 SRC の典型的な番号体系に概ね合わせている（0 平地, 1 道, 2 森, ...）。
/// 実シナリオの terrain.txt が無い段階のフォールバック。
pub const DEFAULT_TERRAINS: &[Terrain] = &[
    Terrain {
        id: 0,
        name: "平地",
        class: "平地",
        move_cost: 1,
        hit_mod: 0,
        damage_mod: 0,
        color: "#7cb342",
        glyph: "",
        bitmap_hint: "plain,平地",
    },
    Terrain {
        id: 1,
        name: "道",
        class: "道路",
        move_cost: 1,
        hit_mod: 0,
        damage_mod: 0,
        color: "#bdbdbd",
        glyph: "",
        bitmap_hint: "road,道",
    },
    Terrain {
        id: 2,
        name: "森林",
        class: "森林",
        move_cost: 2,
        hit_mod: 10,
        damage_mod: 5,
        color: "#2e7d32",
        glyph: "木",
        bitmap_hint: "woods,forest,森林,林",
    },
    Terrain {
        id: 3,
        name: "山",
        class: "山",
        move_cost: 3,
        hit_mod: 15,
        damage_mod: 10,
        color: "#8d6e63",
        glyph: "山",
        bitmap_hint: "mountain,山",
    },
    Terrain {
        id: 4,
        name: "海",
        class: "海",
        move_cost: 99,
        hit_mod: 0,
        damage_mod: 0,
        color: "#1e88e5",
        glyph: "～",
        bitmap_hint: "sea,海",
    },
    Terrain {
        id: 5,
        name: "都市",
        class: "都市",
        move_cost: 1,
        hit_mod: 20,
        damage_mod: 15,
        color: "#cfd8dc",
        glyph: "市",
        bitmap_hint: "city,都市",
    },
    Terrain {
        id: 6,
        name: "宇宙",
        class: "宇宙",
        move_cost: 1,
        hit_mod: 0,
        damage_mod: 0,
        color: "#1a237e",
        glyph: "*",
        bitmap_hint: "space,宇宙",
    },
];

/// `id` から `Terrain` を逆引き。未定義 id は `None`。
pub fn lookup(id: u32) -> Option<&'static Terrain> {
    DEFAULT_TERRAINS.iter().find(|t| t.id == id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_default_terrains_lookup_correctly() {
        for t in DEFAULT_TERRAINS {
            assert_eq!(lookup(t.id).map(|x| x.id), Some(t.id));
        }
    }

    #[test]
    fn unknown_id_is_none() {
        assert!(lookup(999).is_none());
    }
}
