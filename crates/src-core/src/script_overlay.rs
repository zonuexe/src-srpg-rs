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
    /// `FillStyle` の状態反映用。`true`=塗りつぶし (VbFSSolid 等の非透明)、
    /// `false`=透明 (VbFSTransparent)。後続の Circle/Oval/Polygon/Arc に適用。
    /// SRC の網かけ/斜線等のハッチスタイルは本実装では solid 塗りで近似する。
    SetFillSolid(bool),
    /// `FillColor` の状態反映用。後続の塗り図形 (Circle/Oval/Polygon/Arc) の
    /// 内部塗り色。線色 (SetColor) とは独立。
    SetFillColor { color: String },
    /// `Circle x y r [color]` — 中心 (cx,cy)・半径 r の真円。
    /// 輪郭は現在の線色 (color 省略時)、塗りは FillStyle=solid のとき FillColor。
    Circle { cx: f64, cy: f64, r: f64 },
    /// `Oval x y r ratio [color]` — 中心 (cx,cy)・横半径 r・縦横比 ratio の楕円。
    Oval {
        cx: f64,
        cy: f64,
        r: f64,
        ratio: f64,
    },
    /// `Polygon x1 y1 x2 y2 …` — 頂点列を結ぶ多角形 (閉path)。色は現在の線色のみ。
    Polygon { points: Vec<(f64, f64)> },
    /// `Arc x y r start end [color]` — 中心 (cx,cy)・半径 r の円弧。
    /// 角度は度数法・右向き=0・反時計回りに増加 (上向き=90)。
    Arc {
        cx: f64,
        cy: f64,
        r: f64,
        start_deg: f64,
        end_deg: f64,
    },
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
    /// 遅延クリア フラグ（SRC immediate-mode のバックバッファ消去意味論）。
    /// `ClearPicture` はバックバッファのみを消し、画面 (= 直前 `Refresh` が
    /// 表示したフレーム) は次の present まで保持される。本実装の retained-overlay
    /// では「`ClearPicture` で即 cmds を空にすると Wait 中のフレームが消える」ため、
    /// `ClearPicture` はこのフラグだけ立て、次の描画 push か `Refresh`(present) で
    /// 実際に cmds を消す。汎用戦闘アニメの `Paint; Refresh; ClearPicture; Wait`
    /// フレームループが正しく各フレームを表示するための要。
    #[serde(default)]
    pub pending_clear: bool,
    /// 現在の塗りスタイル（`FillStyle`）。`true`=塗りつぶし。SRC `Event.ObjFillStyle`。
    /// `ClearPicture`(defer) を跨いで保持され、`clear()`(シーン遷移) で既定へ戻る。
    #[serde(default)]
    pub current_fill_solid: bool,
    /// 現在の塗り色（`FillColor`）。SRC `Event.ObjFillColor`。
    #[serde(default)]
    pub current_fill_color: String,
    /// 現在の線幅（`DrawWidth`）。0 は未設定（=既定 1px）。SRC `Event.ObjDrawWidth`。
    #[serde(default)]
    pub current_line_width: f64,
}

impl ScriptOverlay {
    pub fn clear(&mut self) {
        self.cmds.clear();
        self.pending_clear = false;
        // シーン遷移等の本クリアでは描画ペン状態 (色/フォント/塗り/線幅) も既定へ戻す。
        // (ClearPicture=defer_clear はこれらを保持＝SRC の ObjColor 等の永続性に対応)。
        self.current_font = None;
        self.current_color = String::new();
        self.current_fill_solid = false;
        self.current_fill_color = String::new();
        self.current_line_width = 0.0;
    }

    /// `ClearPicture` 用の遅延クリア。cmds は消さず、次の描画 push / `present` で消す。
    pub fn defer_clear(&mut self) {
        self.pending_clear = true;
    }

    /// `Refresh`(present) 相当。保留中のクリアがあればここで適用する
    /// （`ClearPicture; Refresh` でバックバッファの空を表示するケース）。
    pub fn present(&mut self) {
        if self.pending_clear {
            self.cmds.clear();
            self.pending_clear = false;
        }
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
        // 保留中の ClearPicture を、新フレーム最初の描画でここで適用する
        // (immediate-mode のバックバッファ消去 → 新規描画開始)。
        if self.pending_clear {
            self.cmds.clear();
            self.pending_clear = false;
        }
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
            // 塗り/線幅も永続ペン状態として保持 (ClearPicture を跨いで有効=SRC Obj* 準拠)。
            DrawCmd::SetFillSolid(solid) => {
                self.current_fill_solid = *solid;
            }
            DrawCmd::SetFillColor { color } => {
                self.current_fill_color = color.clone();
            }
            DrawCmd::SetLineWidth(n) => {
                self.current_line_width = *n;
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
