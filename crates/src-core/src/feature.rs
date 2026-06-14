use serde::{Deserialize, Serialize};

/// Runtime feature state for a unit instance.
/// `UnitData.features` contains static `(name, value)` pairs parsed from unit.txt.
/// `ActiveFeature` tracks whether each feature is currently active on a specific unit instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveFeature {
    /// Feature name (matches `UnitData.features[].0`).
    pub name: String,
    /// Feature value (matches `UnitData.features[].1`).
    pub value: String,
    /// Whether this feature is currently active (may depend on pilot skills, conditions, etc.).
    #[serde(default = "default_true")]
    pub is_active: bool,
}

fn default_true() -> bool {
    true
}

impl ActiveFeature {
    /// Create a new active feature.
    pub fn new(name: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
            is_active: true,
        }
    }

    /// Check if this feature is available/active.
    pub fn is_available(&self) -> bool {
        self.is_active
    }

    /// Reset feature to active state.
    pub fn reset(&mut self) {
        self.is_active = true;
    }
}

/// Query helper: check if a feature list contains a feature with the given name.
pub fn has_feature(features: &[ActiveFeature], name: &str) -> bool {
    features.iter().any(|f| f.name == name && f.is_active)
}

/// Query helper: get the value of a feature by name.
pub fn feature_value<'a>(features: &'a [ActiveFeature], name: &str) -> Option<&'a str> {
    features
        .iter()
        .find(|f| f.name == name && f.is_active)
        .map(|f| f.value.as_str())
}

/// レベル付き特殊能力 (`<base>Lv<n>`、SRC 書式 `修理装置Lv*` / `ＨＰ回復Lv*` 等) を
/// 探し、所持していれば**レベル**を返す。レベル指定なし (`<base>` のみ) は `1`。
/// `<base>` に続く接尾辞が `Lv<n>` 以外 (例 `ＨＰ回復阻害`) のものは別能力として除外する。
/// `=別名` はパーサが value 側へ分離するため name には現れない前提。
pub fn feature_level(features: &[ActiveFeature], base: &str) -> Option<i32> {
    features.iter().filter(|f| f.is_active).find_map(|f| {
        let rest = f.name.trim().strip_prefix(base)?;
        if rest.is_empty() {
            return Some(1);
        }
        // `<base>Lv<n>` のみ受理 (Lv は半角/全角・大小を許容)。他の接尾辞は別能力。
        let after_lv = rest
            .strip_prefix("Lv")
            .or_else(|| rest.strip_prefix("LV"))
            .or_else(|| rest.strip_prefix("lv"))
            .or_else(|| rest.strip_prefix("Ｌｖ"))
            .or_else(|| rest.strip_prefix("ＬＶ"))?;
        let digits: String = after_lv.chars().filter(char::is_ascii_digit).collect();
        Some(digits.parse().unwrap_or(1))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn active_feature_default_active() {
        let feature = ActiveFeature::new("Fly", "10");
        assert!(feature.is_available());
        assert_eq!(feature.name, "Fly");
        assert_eq!(feature.value, "10");
    }

    #[test]
    fn has_feature_checks_name_and_active() {
        let features = vec![
            ActiveFeature::new("Fly", "10"),
            ActiveFeature {
                name: "Range".into(),
                value: "5".into(),
                is_active: false,
            },
        ];
        assert!(has_feature(&features, "Fly"));
        assert!(!has_feature(&features, "Range")); // inactive
        assert!(!has_feature(&features, "Missile"));
    }

    #[test]
    fn feature_level_parses_lv_suffix_and_defaults() {
        let features = vec![
            ActiveFeature::new("修理装置", ""),
            ActiveFeature::new("ＨＰ回復Lv3", ""),
            ActiveFeature::new("ＥＮ回復Lv2", "別名"),
            ActiveFeature::new("ＨＰ回復阻害", ""),
        ];
        // レベル指定なし → 1。
        assert_eq!(feature_level(&features, "修理装置"), Some(1));
        // `Lv<n>` 接尾辞からレベル抽出。
        assert_eq!(feature_level(&features, "ＨＰ回復"), Some(3));
        assert_eq!(feature_level(&features, "ＥＮ回復"), Some(2));
        // 別接尾辞 (阻害) は別能力として除外 (ＨＰ回復阻害 を ＨＰ回復 と誤認しない)。
        // ＨＰ回復 は Lv3 が先にヒットするので、阻害のみのケースも検証。
        let only_block = vec![ActiveFeature::new("ＥＮ回復阻害", "")];
        assert_eq!(feature_level(&only_block, "ＥＮ回復"), None);
        // 非所持 → None。
        assert_eq!(feature_level(&features, "補給装置"), None);
    }

    #[test]
    fn feature_level_ignores_inactive() {
        let features = vec![ActiveFeature {
            name: "ＨＰ回復Lv2".into(),
            value: String::new(),
            is_active: false,
        }];
        assert_eq!(feature_level(&features, "ＨＰ回復"), None);
    }

    #[test]
    fn feature_value_returns_correct_value() {
        let features = vec![
            ActiveFeature::new("Fly", "10"),
            ActiveFeature {
                name: "Range".into(),
                value: "5".into(),
                is_active: false,
            },
        ];
        assert_eq!(feature_value(&features, "Fly"), Some("10"));
        assert_eq!(feature_value(&features, "Range"), None); // inactive
        assert_eq!(feature_value(&features, "Missile"), None);
    }
}
