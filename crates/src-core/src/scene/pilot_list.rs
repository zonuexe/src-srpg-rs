//! パイロット一覧画面 / Pilot list overview.
//!
//! Shows pilot data from the current scenario.

/// 画面サイズ (pixel)。Main.frm の幅に合わせつつ、高さは表項目数に応じて広めに。
pub const PILOT_LIST_WIDTH: u32 = 568;
pub const PILOT_LIST_HEIGHT: u32 = 432;

/// 各カラムのヘッダ。順序は表示順。先頭の "顔" カラムはサムネ表示用。
/// Column headers in display order.
pub const COLUMNS: &[Column] = &[
    Column {
        title: "顔",
        width: 40,
    },
    Column {
        title: "名前",
        width: 80,
    },
    Column {
        title: "愛称",
        width: 64,
    },
    Column {
        title: "Lv",
        width: 28,
    },
    Column {
        title: "Exp",
        width: 36,
    },
    Column {
        title: "SP",
        width: 28,
    },
    Column {
        title: "士気",
        width: 36,
    },
    Column {
        title: "格",
        width: 28,
    },
    Column {
        title: "射",
        width: 28,
    },
    Column {
        title: "命",
        width: 28,
    },
    Column {
        title: "回",
        width: 28,
    },
    Column {
        title: "反",
        width: 28,
    },
    Column {
        title: "技",
        width: 28,
    },
    Column {
        title: "クラス",
        width: 80,
    },
];

#[derive(Debug, Clone, Copy)]
pub struct Column {
    pub title: &'static str,
    pub width: u32,
}

/// テーブル行の Y 高さ。顔グラ 32×32 + 余白 4 を収める。
pub const ROW_HEIGHT: u32 = 36;

/// ヘッダ + 表先頭の Y オフセット。
pub const HEADER_TOP: u32 = 24;

/// 表示可能な最大行数。`PILOT_LIST_HEIGHT - HEADER_TOP - 余白` から算出。
pub const fn max_rows() -> usize {
    let avail = (PILOT_LIST_HEIGHT - HEADER_TOP - ROW_HEIGHT - 24) as usize;
    avail / ROW_HEIGHT as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn columns_total_width_fits() {
        let sum: u32 = COLUMNS.iter().map(|c| c.width).sum();
        assert!(sum <= PILOT_LIST_WIDTH);
    }

    #[test]
    fn at_least_a_few_rows_visible() {
        assert!(max_rows() >= 5);
    }
}
