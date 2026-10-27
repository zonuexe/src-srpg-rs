//! ファイル選択 / アーカイブ展開 / Archive extraction.
//!
//! ブラウザの `<input type=file>` から受け取った 1 ファイル (単独テキスト / `.zip` /
//! `.lzh`) を展開し、画像・音声を [`Assets`] に登録してから、シナリオとしての取り込みと
//! 起動を src-core ([`src_core::scenario::load_scenario`]) に任せる。

use std::rc::Rc;

use src_core::App;

use crate::assets::{detect_image_mime, Assets};
use crate::audio;

/// 展開済みアーカイブ全体を `App` と `Assets` に反映。
///
/// `on_image_loaded` は取り込んだ各画像の非同期デコード完了時に呼ばれる。
/// 静止画面でも後からデコードされた画像を反映させる再描画トリガ。
///
/// `full_load=false` は「データ専用の再適用」モード。画像/音声/MIDI の登録も `.eve` の
/// 実行も行わず、データと `.eve` / `.ini` の登録だけを行う。シナリオ読込で `App` を
/// リセットした後に汎用素材パックの terrain/sp や共有 Lib のサブルーチンを下地として
/// 再適用する用途。画像/音声は `Assets` 側に残るため再デコード不要。
pub fn load_into_app(
    app: &mut App,
    assets: &mut Assets,
    file_name: &str,
    bytes: &[u8],
    on_image_loaded: &Rc<dyn Fn()>,
    full_load: bool,
) -> Result<String, String> {
    let entries = src_core::archive::extract(file_name, bytes)?;
    if entries.is_empty() {
        return Err("アーカイブにエントリがありません。".to_string());
    }
    let mut log = format!("読み込み: {file_name} ({} entries)\n", entries.len());
    // 画像・音声は先に登録する (スクリプトは描画・再生の時点で名前を引くので、
    // `.eve` の実行より前に揃っていればよい)。
    if full_load {
        register_media(assets, &entries, on_image_loaded, &mut log);
    }
    log.push_str(&src_core::scenario::load_scenario(app, &entries, full_load));
    Ok(log)
}

/// 画像 (BMP / PNG / ICO / JPG / GIF)・音声 (MP3 / OGG / WAV)・MIDI を [`Assets`] に登録する。
/// 自動再生はしない (シナリオの `Startbgm` / `Playsound` が名前で引く)。
fn register_media(
    assets: &mut Assets,
    entries: &[(String, Vec<u8>)],
    on_image_loaded: &Rc<dyn Fn()>,
    log: &mut String,
) {
    // 画像は一旦集めてから「Unit / Pilot フォルダ優先」で登録する。
    // zip エントリ順 (アルファベット順) のままだと戦闘エフェクトの
    // アニメフレームが上限を食い潰し、肝心のユニット / 顔グラが登録
    // されない (スパロボ戦記: Bitmap/Anime/Animation 等が先, Bitmap/Unit が後)。
    let mut deferred_images: Vec<(&str, &[u8])> = Vec::new();
    for (name, data) in entries {
        let lname = name.to_ascii_lowercase();
        if let Some(mime) = audio::audio_mime_from_name(&lname) {
            assets.add_audio(name, data, mime);
        } else if lname.ends_with(".mid") || lname.ends_with(".midi") {
            assets.add_midi(name, data);
        } else if detect_image_mime(data).is_some() && data.len() <= 2 * 1024 * 1024 {
            // 大きすぎる (>2MB) 画像はスキップしてメモリを節約する。
            deferred_images.push((name.as_str(), data.as_slice()));
        }
    }
    // 画像登録: Unit / Pilot フォルダ (タイトルアイコン / マップスプライト /
    // 顔グラ / タイトル機体全身画像) はゲーム必須なので **上限に関わらず全て
    // 登録** し、メモリ抑制は tier 2 (背景 / エフェクト 等) の枚数だけを絞る。
    {
        // tier 0/1 (event UI アイコン / Unit・Pilot スプライト / Anime/Unit 全身
        // 画像) はゲーム必須なので **上限に関わらず全登録** する。tier 判定は
        // 849e460 で抽出した純関数 `image_priority_tier` を共有する。
        deferred_images.sort_by_key(|(name, _)| image_priority_tier(name));
        // メモリ抑制用の総枚数上限。tier 0/1 は必須なのでこの上限を超えても
        // 登録し続け、tier 2 (背景 / エフェクト) だけがこの上限の影響を受ける。
        // これにより大規模アーカイブ (例: 84MB 版スパロボ戦記、画像 6500+ 枚)
        // でもタイトルの機体整列演出 / ユニットアイコン / 顔グラ / ステータス
        // 画面の UI アイコンが欠落しない (bd20b44 / 849e460 で直した「画像が
        // 描画されない」不具合の再発防止。tier 0+1 が上限ギリギリ・超過の構成
        // でも安全)。
        const TOTAL_BUDGET: usize = 5000;
        let mut registered = 0usize;
        let mut tier2_dropped = 0usize;
        for (name, data) in deferred_images.into_iter() {
            if image_priority_tier(name) == 2 && registered >= TOTAL_BUDGET {
                tier2_dropped += 1;
                continue;
            }
            match assets.add_image(name, data, on_image_loaded) {
                Ok(()) => registered += 1,
                Err(e) => log.push_str(&format!(
                    "  ⚠ {name} 画像登録失敗: {}\n",
                    e.as_string().unwrap_or_default()
                )),
            }
        }
        if tier2_dropped > 0 {
            log.push_str(&format!(
                "  ℹ️ 背景/エフェクト画像 {tier2_dropped} 件を上限超過のため省略 \
                 (ユニット/機体画像は全登録)\n"
            ));
        }
    }
    if !assets.images.is_empty() {
        log.push_str(&format!(
            "  📷 画像 {} 件を Assets に登録\n",
            assets.images.len() / 2 // basename + stem の 2 キーで張っているので半分
        ));
    }
    if !assets.audio_clips.is_empty() {
        log.push_str(&format!(
            "  🔊 音声 {} 件を Assets に登録\n",
            assets.audio_clips.len() / 2
        ));
    }
    if !assets.midi_clips.is_empty() {
        log.push_str(&format!(
            "  ♪ MIDI {} 件を Assets に登録\n",
            assets.midi_clips.len() / 2
        ));
    }
}

/// 画像登録の優先度 tier (小さいほど優先)。総画像数が `IMAGE_LIMIT` を超える
/// シナリオ (スパロボ戦記 = 6500+ 枚 > 4500) では tier の大きいものが切り捨て
/// られるため、UI に必須の小アイコンを確実に残すための分類。
///
/// - 0: `event/` の UI アイコン (武器属性/種別/射撃/格闘/複合/次のページ 等。
///   ステータス画面で必須。点数が少なく小さいので最優先で確保) +
///   `Bitmap/Unit` `Bitmap/Pilot` (マップスプライト / 機体アイコン / 顔グラ)。
/// - 1: `Anime/Unit` (タイトル全身画像 + 戦闘アニメ)。`/unit/` を含むが
///   `Bitmap/Unit` とは別物なので分離する。
/// - 2: それ以外 (背景 / エフェクト 等)。超過時に最初に切られる。
fn image_priority_tier(name: &str) -> u8 {
    // 先頭に `/` を補い、ルート直下の `Unit/` `Pilot/` `event/` もマッチさせる。
    let l = format!("/{}", name.to_ascii_lowercase().replace('\\', "/"));
    if l.contains("/event/") {
        return 0;
    }
    let unit_or_pilot = l.contains("/unit/") || l.contains("/pilot/");
    match (unit_or_pilot, l.contains("/anime/")) {
        (true, false) => 0,
        (true, true) => 1,
        _ => 2,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lone_eve_text_is_executed_as_event() {
        let mut app = App::new();
        let mut assets = Assets::default();
        let src = "Stage \"テスト\"\nMapSize 2 2\n";
        let noop: Rc<dyn Fn()> = Rc::new(|| {});
        let log = load_into_app(
            &mut app,
            &mut assets,
            "demo.eve",
            src.as_bytes(),
            &noop,
            true,
        )
        .unwrap();
        assert!(log.contains("demo.eve"));
        assert_eq!(app.stage(), "テスト");
        assert!(app.database().map.is_some());
    }

    #[test]
    fn image_priority_event_icons_are_top_tier() {
        // event/ の UI アイコンは最優先 (tier 0) — 画像数超過時も切り捨てない。
        // スパロボ戦記 のステータス画面の属性/種別/射撃/格闘アイコンが該当。
        assert_eq!(image_priority_tier("Bitmap\\event\\属性.png"), 0);
        assert_eq!(image_priority_tier("Bitmap/event/射撃.png"), 0);
        assert_eq!(image_priority_tier("bitmap\\EVENT\\次のページへ.png"), 0);
        // Unit / Pilot スプライトも tier 0。
        assert_eq!(image_priority_tier("Bitmap\\Unit\\foo.bmp"), 0);
        assert_eq!(image_priority_tier("Bitmap\\Pilot\\bar.bmp"), 0);
        // Anime/Unit は tier 1。
        assert_eq!(image_priority_tier("Anime\\Unit\\EC_X.bmp"), 1);
        // 背景 / エフェクトは tier 2 (超過時に最初に切られる)。
        assert_eq!(image_priority_tier("Back\\EFFECT_Window.gif"), 2);
        assert_eq!(image_priority_tier("Bitmap\\Anime\\Effect\\boom.bmp"), 2);
    }
}
