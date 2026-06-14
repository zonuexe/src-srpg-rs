//! ブラウザ側で保持するアセット（読込済み画像など） / Browser-side loaded assets.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::{Clamped, JsCast};
use web_sys::{
    Blob, BlobPropertyBag, CanvasRenderingContext2d, HtmlCanvasElement, HtmlImageElement,
    ImageData, Url,
};

use src_core::asset::frx;

/// 元 `Title.frx` をビルド時にバイナリ埋め込み。10KB 弱なので wasm に直接同梱。
/// Embed the entire VB6 `Title.frx` (~10 KB) into the wasm binary at build time.
const TITLE_FRX: &[u8] = include_bytes!("../../../SRC_20121125/Title.frx");

/// `Title.frm` の `Picture1.Picture = "Title.frx":030A`。200x40, 8bpp BMP ("SRC" ロゴ)。
pub const TITLE_PICTURE_OFFSET: usize = 0x030A;

/// 生バイト列の magic から MIME を判定。
/// 元 SRC は BMP / ICO のみネイティブ対応で PNG は Susie プラグイン経由だったが、
/// 移植版はブラウザがすべてデコードできるので拡張子に頼らず magic で判定する。
pub fn detect_image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"BM") {
        Some("image/bmp")
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else if bytes.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        Some("image/x-icon")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else {
        None
    }
}

/// `Title.frm` の `Image1.Picture = "Title.frx":268C`。32x32, 4bpp ICO。
pub const TITLE_IMAGE_OFFSET: usize = 0x268C;

/// 読込済みアセット集合。
/// Loaded asset bundle.
#[derive(Default)]
pub struct Assets {
    /// 元 `Title.frm` の `Picture1`（"SRC" バナー）
    pub title_logo: Option<HtmlImageElement>,
    /// 元 `Title.frm` の `Image1`（左下のアイコン；ICO）
    pub title_icon: Option<HtmlImageElement>,
    /// シナリオから取り込んだ画像（顔グラ / ユニット画像 / マップタイル等）。
    /// キーは大小無視・拡張子含むベース名 (例: "braver.bmp")。複数候補ヒット
    /// 用に拡張子なしの別エントリも同時に登録される。
    pub images: HashMap<String, HtmlImageElement>,
    /// シナリオから取り込んだ MP3/OGG/WAV の生バイト + MIME。
    /// `Playsound name` / `Startbgm name` で参照する。キーはベース名と
    /// 拡張子無しの両方。
    pub audio_clips: HashMap<String, (Vec<u8>, &'static str)>,
    /// シナリオから取り込んだ MIDI の生バイト。
    /// キーは拡張子付き / 無し両方。
    pub midi_clips: HashMap<String, Vec<u8>>,
    /// `透過` 指定の `PaintPicture` 用に、カラーキー透明化済みの canvas を
    /// キャッシュする。キーは `images` と同じ (小文字 basename)。毎フレーム
    /// 画素処理し直すのを避けるため、初回処理結果をここに保持する。
    transparent_cache: RefCell<HashMap<String, HtmlCanvasElement>>,
}

impl Assets {
    /// 与えられた画像バイト列を Blob → HtmlImageElement に流して `images` に登録。
    /// `name` はアーカイブ内のファイル名 (パス含む可)。ベース名と拡張子無し版を
    /// 両方キーに張る。
    ///
    /// `on_loaded` は各画像の非同期デコード完了時に呼ばれる (再描画トリガ)。
    /// 静止画面 (タイトル等) は animation loop が再描画しないため、これが
    /// 無いと後からデコードされた画像が画面に出ない。
    pub fn add_image(
        &mut self,
        name: &str,
        bytes: &[u8],
        on_loaded: &Rc<dyn Fn()>,
    ) -> Result<(), JsValue> {
        let Some(mime) = detect_image_mime(bytes) else {
            return Err(JsValue::from_str("未対応の画像 magic"));
        };
        let img = blob_to_image(bytes, mime, on_loaded.clone())?;
        let basename = basename(name).to_string();
        let basename_lc = basename.to_ascii_lowercase();
        let stem = strip_ext(&basename).to_string();
        let stem_lc = stem.to_ascii_lowercase();
        self.images.insert(basename_lc, img.clone());
        self.images.insert(stem_lc, img);
        Ok(())
    }

    /// 与えられたヒント (Pilot.Nickname 等) から画像を最善ヒットで返す。
    /// 大小無視。`hint`、`hint.bmp`、`hint.png` の順に探索する。
    pub fn find_image(&self, hint: &str) -> Option<&HtmlImageElement> {
        if hint.is_empty() {
            return None;
        }
        let lower = hint.to_ascii_lowercase();
        if let Some(img) = self.images.get(&lower) {
            return Some(img);
        }
        for ext in [".bmp", ".png", ".jpg", ".jpeg", ".gif"] {
            let key = format!("{lower}{ext}");
            if let Some(img) = self.images.get(&key) {
                return Some(img);
            }
        }
        None
    }

    /// `透過` 用: 画像の左上ピクセル色をカラーキーとして透明化した canvas を返す。
    ///
    /// SRC の `透過` は BMP の特定色を透明扱いする指定。ブラウザの `<img>` は
    /// カラーキー透過を持たないため、offscreen canvas に展開 → 画素の RGB が
    /// 左上ピクセルと一致するものの alpha を 0 にした canvas を生成する。
    /// 初回のみ処理してキャッシュし、以降は使い回す。
    ///
    /// 画像がまだ非同期デコード未完了 (`natural_width == 0`) なら `None` を返す。
    /// 呼び出し側 (描画ループ) は次フレームで再試行する。
    pub fn transparent_image(&self, key: &str) -> Option<HtmlCanvasElement> {
        let lower = key.to_ascii_lowercase();
        if let Some(c) = self.transparent_cache.borrow().get(&lower) {
            return Some(c.clone());
        }
        let img = self.find_image(&lower)?;
        let w = img.natural_width();
        let h = img.natural_height();
        if w == 0 || h == 0 {
            return None;
        }
        let canvas = color_key_canvas(img, w, h).ok()?;
        self.transparent_cache
            .borrow_mut()
            .insert(lower, canvas.clone());
        Some(canvas)
    }

    /// MP3/OGG/WAV をキャッシュ。`name` はアーカイブパス。
    pub fn add_audio(&mut self, name: &str, bytes: &[u8], mime: &'static str) {
        let basename = basename(name).to_string();
        let basename_lc = basename.to_ascii_lowercase();
        let stem_lc = strip_ext(&basename).to_ascii_lowercase();
        self.audio_clips.insert(basename_lc, (bytes.to_vec(), mime));
        self.audio_clips.insert(stem_lc, (bytes.to_vec(), mime));
    }

    /// MIDI をキャッシュ。
    pub fn add_midi(&mut self, name: &str, bytes: &[u8]) {
        let basename = basename(name).to_string();
        let basename_lc = basename.to_ascii_lowercase();
        let stem_lc = strip_ext(&basename).to_ascii_lowercase();
        self.midi_clips.insert(basename_lc, bytes.to_vec());
        self.midi_clips.insert(stem_lc, bytes.to_vec());
    }

    /// 名前ヒントから音声を引く。`hint.mp3`/`.ogg`/`.wav` の順で探索。
    pub fn find_audio(&self, hint: &str) -> Option<&(Vec<u8>, &'static str)> {
        if hint.is_empty() {
            return None;
        }
        let lower = basename(hint).to_ascii_lowercase();
        if let Some(v) = self.audio_clips.get(&lower) {
            return Some(v);
        }
        for ext in [".mp3", ".ogg", ".wav"] {
            let key = format!("{lower}{ext}");
            if let Some(v) = self.audio_clips.get(&key) {
                return Some(v);
            }
        }
        None
    }

    /// 名前ヒントから MIDI を引く。`hint.mid` で探索。
    pub fn find_midi(&self, hint: &str) -> Option<&Vec<u8>> {
        if hint.is_empty() {
            return None;
        }
        let lower = basename(hint).to_ascii_lowercase();
        if let Some(v) = self.midi_clips.get(&lower) {
            return Some(v);
        }
        for ext in [".mid", ".midi"] {
            let key = format!("{lower}{ext}");
            if let Some(v) = self.midi_clips.get(&key) {
                return Some(v);
            }
        }
        None
    }
}

fn basename(path: &str) -> &str {
    path.rsplit_once(['/', '\\'])
        .map(|(_, b)| b)
        .unwrap_or(path)
}

fn strip_ext(name: &str) -> &str {
    name.rsplit_once('.').map(|(s, _)| s).unwrap_or(name)
}

fn blob_to_image(
    bytes: &[u8],
    mime: &str,
    on_loaded: Rc<dyn Fn()>,
) -> Result<HtmlImageElement, JsValue> {
    let blob = make_blob(bytes, mime)?;
    let url = Url::create_object_url_with_blob(&blob)?;
    let img = HtmlImageElement::new()?;
    let url_clone = url.clone();
    let onload = Closure::<dyn FnMut()>::new(move || {
        let _ = Url::revoke_object_url(&url_clone);
        on_loaded();
    });
    img.set_onload(Some(onload.as_ref().unchecked_ref()));
    onload.forget();
    img.set_src(&url);
    Ok(img)
}

/// `Title.frx` 起点で各画像リソースを非同期に読み込んで `Assets` に格納する。
/// 完了するたびに `on_each_loaded` を呼ぶ（再描画トリガに使う）。
pub fn load_title_assets(
    assets: Rc<RefCell<Assets>>,
    on_each_loaded: impl Fn() + Clone + 'static,
) -> Result<(), JsValue> {
    // Picture1: BMP
    load_frx_image(TITLE_FRX, TITLE_PICTURE_OFFSET, "image/bmp", {
        let assets = assets.clone();
        let cb = on_each_loaded.clone();
        move |img| {
            assets.borrow_mut().title_logo = Some(img);
            cb();
        }
    })?;

    // Image1: ICO
    load_frx_image(TITLE_FRX, TITLE_IMAGE_OFFSET, "image/x-icon", {
        let assets = assets.clone();
        let cb = on_each_loaded.clone();
        move |img| {
            assets.borrow_mut().title_icon = Some(img);
            cb();
        }
    })?;

    Ok(())
}

/// .frx の単一リソースを `<img>` 要素に流し込む汎用ローダ。
/// Generic loader: read one .frx resource and feed it into an `HtmlImageElement`.
fn load_frx_image(
    file: &'static [u8],
    offset: usize,
    mime: &str,
    on_loaded: impl FnOnce(HtmlImageElement) + 'static,
) -> Result<(), JsValue> {
    let res = frx::read_at(file, offset).ok_or_else(|| {
        JsValue::from_str(&format!(
            ".frx resource at offset {offset:#x}: parse failed"
        ))
    })?;

    let blob = make_blob(res.bytes, mime)?;
    let url = Url::create_object_url_with_blob(&blob)?;
    let img = HtmlImageElement::new()?;

    // onload 1 回で発火 → 状態書込 → ObjectURL 解放 → 呼び出し側コールバック
    type LoadedCb = Box<dyn FnOnce(HtmlImageElement)>;
    let on_loaded_cell: Rc<RefCell<Option<LoadedCb>>> =
        Rc::new(RefCell::new(Some(Box::new(on_loaded))));
    let onload = {
        let img_clone = img.clone();
        let url_clone = url.clone();
        let cell = on_loaded_cell.clone();
        Closure::<dyn FnMut()>::new(move || {
            let _ = Url::revoke_object_url(&url_clone);
            if let Some(cb) = cell.borrow_mut().take() {
                cb(img_clone.clone());
            }
        })
    };
    img.set_onload(Some(onload.as_ref().unchecked_ref()));
    onload.forget();

    img.set_src(&url);
    Ok(())
}

/// `img` を offscreen canvas に展開し、左上ピクセルと同じ RGB の画素を
/// 透明 (alpha 0) にした canvas を返す。SRC の `透過` 相当のカラーキー処理。
fn color_key_canvas(img: &HtmlImageElement, w: u32, h: u32) -> Result<HtmlCanvasElement, JsValue> {
    let document = web_sys::window()
        .and_then(|win| win.document())
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let canvas: HtmlCanvasElement = document.create_element("canvas")?.dyn_into()?;
    canvas.set_width(w);
    canvas.set_height(h);
    let ctx: CanvasRenderingContext2d = canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("2d ctx unavailable"))?
        .dyn_into()?;
    ctx.draw_image_with_html_image_element(img, 0.0, 0.0)?;
    let image_data = ctx.get_image_data(0.0, 0.0, f64::from(w), f64::from(h))?;
    let mut data = image_data.data();
    if data.len() >= 4 {
        // カラーキー = (0,0) の画素色。
        let (kr, kg, kb) = (data[0], data[1], data[2]);
        let mut i = 0;
        while i + 3 < data.len() {
            if data[i] == kr && data[i + 1] == kg && data[i + 2] == kb {
                data[i + 3] = 0;
            }
            i += 4;
        }
        let keyed = ImageData::new_with_u8_clamped_array_and_sh(Clamped(&data), w, h)?;
        ctx.put_image_data(&keyed, 0.0, 0.0)?;
    }
    Ok(canvas)
}

fn make_blob(bytes: &[u8], mime: &str) -> Result<Blob, JsValue> {
    let array = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    array.copy_from(bytes);
    let parts = js_sys::Array::new();
    parts.push(&array.buffer());
    let bag = BlobPropertyBag::new();
    bag.set_type(mime);
    Blob::new_with_buffer_source_sequence_and_options(&parts, &bag)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_bmp_png_ico_jpg_gif() {
        assert_eq!(detect_image_mime(b"BMfoo"), Some("image/bmp"));
        assert_eq!(
            detect_image_mime(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A, 0]),
            Some("image/png")
        );
        assert_eq!(detect_image_mime(&[0, 0, 1, 0, 1, 0]), Some("image/x-icon"));
        assert_eq!(
            detect_image_mime(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some("image/jpeg")
        );
        assert_eq!(detect_image_mime(b"GIF89a..."), Some("image/gif"));
        assert_eq!(detect_image_mime(b"nope"), None);
    }
}
