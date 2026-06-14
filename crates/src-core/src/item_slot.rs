use serde::{Deserialize, Serialize};

/// Equipment slot type. Matches SRC's item part classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SlotType {
    /// 片手武器 (one-handed weapon)
    RightHand,
    /// 左手/盾 (left hand / shield)
    LeftHand,
    /// 右肩武器 (right shoulder weapon)
    RightShoulder,
    /// 左肩武器 (left shoulder weapon)
    LeftShoulder,
    /// 胴体/装甲 (body armor)
    Body,
    /// 頭部 (head)
    Head,
    /// 汎用アイテムスロット (general item slot)
    Item,
}

impl SlotType {
    /// Parse a slot type from a string (matching SRC's part names).
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "片手" | "右手" => Some(Self::RightHand),
            "左手" | "盾" => Some(Self::LeftHand),
            "右肩" => Some(Self::RightShoulder),
            "左肩" => Some(Self::LeftShoulder),
            "胴" | "体" | "装甲" => Some(Self::Body),
            "頭" => Some(Self::Head),
            _ => Some(Self::Item), // default to general item slot
        }
    }

    /// Human-readable label for display.
    pub fn label(self) -> &'static str {
        match self {
            Self::RightHand => "右手",
            Self::LeftHand => "左手",
            Self::RightShoulder => "右肩",
            Self::LeftShoulder => "左肩",
            Self::Body => "胴体",
            Self::Head => "頭部",
            Self::Item => "アイテム",
        }
    }
}

/// An equipment slot on a unit instance.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemSlot {
    /// The type of this slot.
    pub slot_type: SlotType,
    /// Currently equipped item name (references `ItemData.name`). None = empty slot.
    pub equipped_item: Option<String>,
    /// This slot is fixed (cursed equipment, story-locked). Cannot be removed.
    #[serde(default)]
    pub is_fixed: bool,
}

impl ItemSlot {
    /// Create a new empty slot of the given type.
    pub fn new(slot_type: SlotType) -> Self {
        Self {
            slot_type,
            equipped_item: None,
            is_fixed: false,
        }
    }

    /// Create a new slot with an item already equipped.
    pub fn with_item(slot_type: SlotType, item_name: impl Into<String>) -> Self {
        Self {
            slot_type,
            equipped_item: Some(item_name.into()),
            is_fixed: false,
        }
    }

    /// Create a fixed slot (item cannot be removed).
    pub fn fixed(slot_type: SlotType, item_name: impl Into<String>) -> Self {
        Self {
            slot_type,
            equipped_item: Some(item_name.into()),
            is_fixed: true,
        }
    }

    /// Check if this slot is empty.
    pub fn is_empty(&self) -> bool {
        self.equipped_item.is_none()
    }

    /// Equip an item. Returns false if slot is fixed.
    pub fn equip(&mut self, item_name: impl Into<String>) -> bool {
        if self.is_fixed && self.equipped_item.is_some() {
            return false; // can't replace fixed equipment
        }
        self.equipped_item = Some(item_name.into());
        true
    }

    /// Unequip the item. Returns false if slot is fixed.
    pub fn unequip(&mut self) -> bool {
        if self.is_fixed {
            return false;
        }
        self.equipped_item = None;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_slot_equip_and_unequip() {
        let mut slot = ItemSlot::new(SlotType::RightHand);
        assert!(slot.is_empty(), "New slot should be empty");

        assert!(slot.equip(" sword"), "Should be able to equip");
        assert!(!slot.is_empty(), "Slot should not be empty after equip");

        assert!(slot.unequip(), "Should be able to unequip");
        assert!(slot.is_empty(), "Slot should be empty after unequip");
    }

    #[test]
    fn item_slot_fixed_cannot_unequip() {
        let mut slot = ItemSlot::fixed(SlotType::LeftHand, " shield");
        assert!(!slot.is_empty(), "Fixed slot should have item");

        assert!(
            !slot.unequip(),
            "Unequip should return false for fixed slot"
        );
        assert!(!slot.is_empty(), "Item should still be equipped");
    }

    #[test]
    fn item_slot_fixed_cannot_replace() {
        let mut slot = ItemSlot::fixed(SlotType::RightShoulder, " old_weapon");
        assert!(
            !slot.equip(" new_weapon"),
            "Equip should return false for fixed slot with item"
        );
        assert_eq!(
            slot.equipped_item.as_deref(),
            Some(" old_weapon"),
            "Item should remain unchanged"
        );
    }

    #[test]
    fn slot_type_from_str() {
        assert_eq!(SlotType::parse("片手"), Some(SlotType::RightHand));
        assert_eq!(SlotType::parse("右手"), Some(SlotType::RightHand));
        assert_eq!(SlotType::parse("左手"), Some(SlotType::LeftHand));
        assert_eq!(SlotType::parse("盾"), Some(SlotType::LeftHand));
        assert_eq!(SlotType::parse("右肩"), Some(SlotType::RightShoulder));
        assert_eq!(SlotType::parse("左肩"), Some(SlotType::LeftShoulder));
        assert_eq!(SlotType::parse("胴"), Some(SlotType::Body));
        assert_eq!(SlotType::parse("体"), Some(SlotType::Body));
        assert_eq!(SlotType::parse("装甲"), Some(SlotType::Body));
        assert_eq!(SlotType::parse("頭"), Some(SlotType::Head));
        assert_eq!(SlotType::parse("unknown"), Some(SlotType::Item));
        assert_eq!(SlotType::parse("ランダム"), Some(SlotType::Item));
    }
}
