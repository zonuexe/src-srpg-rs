//! `.eve` の描画系命令を蓄積して画面に上書き描画する仕組み。
//!
//! 元 SRC は `PaintString`, `PaintPicture`, `Line`, `Font`, `FillColor` 等の
//! 命令で直接 `Form.PSet`/`Form.Line`/`Form.Print` を呼んで描画していた。
//! ここでは命令実行時に `DrawCmd` を `App.script_overlay` に積み、
//! `render::draw_scene` の最後にまとめて Canvas に反映する。
//!
//! `Refresh` で蓄積をクリアするのが原典準拠。

use serde::{Deserialize, Serialize};

/// 描画命令の最小サブセット。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum DrawCmd {
    /// 現在のフォント設定。後続 PaintString に適用される。
    /// `size_pt` は ポイント数、`color` は CSS カラー文字列。
    SetFont {
        family: String,
        size_pt: u32,
        color: String,
    },
    /// 現在の描画色（Line/PSet 等の線色）。
    SetColor { color: String },
    /// テキスト描画。`x`/`y` は Canvas 論理座標。
    PaintString { x: f64, y: f64, text: String },
    /// 線描画。
    Line { x1: f64, y1: f64, x2: f64, y2: f64 },
    /// 1 点描画（PSet）。
    PSet { x: f64, y: f64 },
    /// 矩形塗りつぶし（FillColor 設定済みのスタイルで）。
    FillRect { x: f64, y: f64, w: f64, h: f64 },
    /// 画面全体のフェード（次回 Refresh まで持続）。
    /// `alpha` は 0.0..=1.0、`color` は CSS カラー。
    Fade { color: String, alpha: f64 },
    /// `PaintPicture path x y [w h] [option…]` で蓄積される画像描画。
    /// `path` は assets で解決する画像名（basename / stem いずれで引いてもよい）。
    Picture {
        path: String,
        x: f64,
        y: f64,
        w: Option<f64>,
        h: Option<f64>,
        /// 透過合成（PNG 等のアルファ尊重）。canvas は常に透過。
        transparent: bool,
        /// 左右反転（horizontal flip）。
        flip_x: bool,
        /// 上下反転（vertical flip）— SRC option `上下反転`。
        #[serde(default)]
        flip_y: bool,
        /// 白黒表示 — SRC option `白黒`。フロントエンドが grayscale フィルタ適用。
        #[serde(default)]
        monochrome: bool,
        /// セピア表示 — SRC option `セピア`。フロントエンドが sepia フィルタ適用。
        #[serde(default)]
        sepia: bool,
        /// 半分マスク — SRC option `上半分`/`下半分`/`左半分`/`右半分` で、
        /// 反対側の半分を背景色で塗りつぶす。空文字なら未指定。
        /// `右上`/`左上`/`右下`/`左下` (対角線塗りつぶし) もここに入れて、
        /// フロントエンドで識別する。
        #[serde(default)]
        half_mode: String,
        /// 回転角度 (度数法、+ は右回転 / - は左回転)。0 なら回転なし。
        /// SRC option `右回転 N` / `左回転 N` で設定される。
        #[serde(default)]
        rotation_deg: f64,
        /// `背景` オプション — マップ背景として書き込まれ、`ChangeMap` まで残る。
        /// 本実装ではフラグとして保持し、フロントエンド側で render persistence を
        /// 別レイヤで処理する想定 (現状は通常 overlay と同じ扱い)。
        #[serde(default)]
        as_background: bool,
        /// `保持` オプション — `ClearPicture` で消えない。本実装ではフラグ保持のみ
        /// で実際の保持挙動は別タスク (ClearPicture が選別 retain する必要)。
        #[serde(default)]
        persist: bool,
        /// 座標 `-`（中央寄せ）指定。`x`/`y` は src-core 側ではプレースホルダで、
        /// 画像の実寸を知るフロントエンドが `中央 - 実寸/2` で確定する。
        /// 幅未指定の `PaintPicture img - -` を画面中央に正しく配置するため。
        #[serde(default)]
        center_x: bool,
        #[serde(default)]
        center_y: bool,
    },
    /// `DrawWidth n` の状態反映用 (Line / 矩形枠の線幅)。
    SetLineWidth(f64),
}

/// 1 フレーム分の描画コマンドリスト。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ScriptOverlay {
    pub cmds: Vec<DrawCmd>,
    /// 現在の Font 設定（SetFont で更新）。
    pub current_font: Option<(String, u32, String)>,
    /// 現在の線色。
    pub current_color: String,
    /// 描画カーソル X 座標 (`BaseX` / `PointX` 関数が返す)。
    /// `Line`/`PSet`/`PaintString` 等の終端座標で更新される。
    /// SRC.NET の `Event.BaseX` (`picMain.CurrentX`) に対応。
    #[serde(default)]
    pub cursor_x: f64,
    /// 描画カーソル Y 座標 (`BaseY` / `PointY` 関数が返す)。
    #[serde(default)]
    pub cursor_y: f64,
}

impl ScriptOverlay {
    pub fn clear(&mut self) {
        self.cmds.clear();
    }

    /// 全画面 Fade のうち、色が `color_matches` に一致するものを全て除去する。
    ///
    /// `WhiteIn` / `FadeIn` 等の **フェードイン** は「白(黒)から通常画面へ戻す」
    /// 演出で、終状態は通常画面 (= フェード除去)。アニメーションせず終状態だけを
    /// 描く本実装では、対応する fade-out の全画面塗りを残すと画面が白/黒のまま
    /// 操作不能になるため、フェードイン時に該当色の Fade を取り除いて露出させる。
    /// セピア/モノトーン等の半透明カラーフィルタ Fade は色が一致しないので残る。
    pub fn remove_fades_of(&mut self, color_matches: impl Fn(&str) -> bool) {
        self.cmds
            .retain(|c| !matches!(c, DrawCmd::Fade { color, .. } if color_matches(color)));
    }

    pub fn push(&mut self, c: DrawCmd) {
        // SetFont / SetColor は state も更新
        match &c {
            DrawCmd::SetFont {
                family,
                size_pt,
                color,
            } => {
                self.current_font = Some((family.clone(), *size_pt, color.clone()));
            }
            DrawCmd::SetColor { color } => {
                self.current_color = color.clone();
            }
            // 描画カーソルを終端座標で更新する (SRC.NET picMain.CurrentX/Y 同等)
            DrawCmd::PSet { x, y } => {
                self.cursor_x = *x;
                self.cursor_y = *y;
            }
            DrawCmd::Line { x2, y2, .. } => {
                self.cursor_x = *x2;
                self.cursor_y = *y2;
            }
            DrawCmd::PaintString { x, y, .. } => {
                self.cursor_x = *x;
                self.cursor_y = *y;
            }
            _ => {}
        }
        self.cmds.push(c);
    }

    /// `BaseX = N` / `BaseY = N` 代入 (script 側からの明示的カーソル設定)。
    pub fn set_cursor(&mut self, x: f64, y: f64) {
        self.cursor_x = x;
        self.cursor_y = y;
    }
}
