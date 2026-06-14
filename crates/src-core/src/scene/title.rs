//! タイトル画面のロジック層 / Title screen logic layer.
//!
//! 元実装: `SRC_20121125/Title.frm` (`frmTitle`)。VB6 の Form をピクセル単位の
//! 抽象レイアウトに変換し、描画フロントエンド（`src-web` 等）へ提供する。
//!
//! 元 .frm の座標値は twips（1/20 pt）なので `/ 15` でピクセル化している
//! （ScaleMode=3 = ピクセル）。

/// 元 Title.frm のクライアント領域サイズ（ピクセル）。
/// Original VB6 form ClientWidth / ClientHeight in pixels.
pub const TITLE_WIDTH: u32 = 386;
pub const TITLE_HEIGHT: u32 = 233;

/// 描画用矩形 / Rectangle in canvas-local pixel coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub const fn new(x: i32, y: i32, w: u32, h: u32) -> Self {
        Self { x, y, w, h }
    }
}

/// タイトル画面の各コントロール配置 / Layout for each title-screen control.
///
/// VB6 の Form/Frame ネストを潰して、すべて Form クライアント座標（ピクセル）に
/// 平坦化してある。元コントロール名はフィールド名のコメントを参照。
#[derive(Debug, Clone, Copy)]
pub struct TitleLayout {
    /// 元: `Frame1` — 中央枠
    pub frame: Rect,
    /// 元: `Image1`（`Title.frx:268C` 埋め込み画像）。今はロゴプレースホルダ
    pub logo: Rect,
    /// 元: `Picture1`（`Title.frx:030A` 埋め込み画像）。"SRC" タイトルロゴ
    pub title_picture: Rect,
    /// 元: `labVersion`
    pub version_label: Rect,
    /// 元: `labAuthor`
    pub author_label: Rect,
    /// 元: `labLicense`
    pub license_label: Rect,
}

impl TitleLayout {
    /// 元 Title.frm から抽出したデフォルト配置。
    pub const fn original() -> Self {
        // Frame1: Left=360tw=24px, Top=120tw=8px, W=5055tw=337px, H=3015tw=201px
        let frame = Rect::new(24, 8, 337, 201);

        // Image1 は Frame1 の子: Left=240tw=16px, Top=840tw=56px, W=H=1440tw=96px
        // 絶対座標 = (frame.x + 16, frame.y + 56)
        let logo = Rect::new(40, 64, 96, 96);

        // Picture1 は Form 直下: Left=2520tw=168px, Top=1320tw=88px,
        // W=3000tw=200px, H=600tw=40px
        let title_picture = Rect::new(168, 88, 200, 40);

        // labVersion は Frame1 の子: Left=2160tw=144px, Top=2040tw=136px,
        // W=2655tw=177px, H=375tw=25px。絶対 = (24+144, 8+136)
        let version_label = Rect::new(168, 144, 177, 25);

        // labAuthor は Frame1 の子: Left=1920tw=128px, Top=2520tw=168px,
        // W=2895tw=193px, H=255tw=17px
        let author_label = Rect::new(152, 176, 193, 17);

        // labLicense は Form 直下: Left=120tw=8px, Top=3240tw=216px,
        // W=5535tw=369px, H=255tw=17px
        let license_label = Rect::new(8, 216, 369, 17);

        Self {
            frame,
            logo,
            title_picture,
            version_label,
            author_label,
            license_label,
        }
    }
}

/// 元: `labAuthor.Caption`
pub const AUTHORS: &str = "Kei Sakamoto / Inui Tetsuyuki";

/// 元: `labLicense.Caption`
pub const LICENSE_NOTICE: &str = "This program is distributed under the terms of GPL";

/// 元: `Form_Load` で組み立てている
/// `"Ver " & App.Major & "." & App.Minor & "." & App.Revision & "a"`。
/// VB6 の `App` プロパティは Cargo パッケージ版で代替する。
pub fn version_string() -> String {
    format!("Ver {}a", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_fits_in_form_bounds() {
        let l = TitleLayout::original();
        for r in [
            l.frame,
            l.logo,
            l.title_picture,
            l.version_label,
            l.author_label,
            l.license_label,
        ] {
            assert!(r.x >= 0);
            assert!(r.y >= 0);
            assert!((r.x as u32) + r.w <= TITLE_WIDTH, "{:?}", r);
            assert!((r.y as u32) + r.h <= TITLE_HEIGHT, "{:?}", r);
        }
    }

    #[test]
    fn version_starts_with_ver() {
        assert!(version_string().starts_with("Ver "));
        assert!(version_string().ends_with("a"));
    }
}
