//! ユニット一覧画面 / Unit list overview.
//!
//! Shows UnitInstance details from the current scenario.

pub const UNIT_LIST_WIDTH: u32 = 568;
pub const UNIT_LIST_HEIGHT: u32 = 432;

#[derive(Debug, Clone, Copy)]
pub struct Column {
    pub title: &'static str,
    pub width: u32,
}

/// カラム定義。順序が表示順。先頭の "画像" カラムはサムネ表示用。
pub const COLUMNS: &[Column] = &[
    Column {
        title: "画像",
        width: 40,
    },
    Column {
        title: "ユニット名",
        width: 80,
    },
    Column {
        title: "パイロット",
        width: 72,
    },
    Column {
        title: "HP",
        width: 56,
    },
    Column {
        title: "EN",
        width: 44,
    },
    Column {
        title: "装甲",
        width: 44,
    },
    Column {
        title: "運動性",
        width: 44,
    },
    Column {
        title: "移動力",
        width: 44,
    },
    Column {
        title: "士気",
        width: 36,
    },
    Column {
        title: "状態",
        width: 64,
    },
];

pub const ROW_HEIGHT: u32 = 36;
pub const HEADER_TOP: u32 = 24;

pub const fn max_rows() -> usize {
    let avail = (UNIT_LIST_HEIGHT - HEADER_TOP - ROW_HEIGHT - 24) as usize;
    avail / ROW_HEIGHT as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_total_width_fits() {
        let sum: u32 = COLUMNS.iter().map(|c| c.width).sum();
        assert!(sum <= UNIT_LIST_WIDTH);
    }

    #[test]
    fn at_least_a_few_rows_visible() {
        assert!(max_rows() >= 5);
    }
}
