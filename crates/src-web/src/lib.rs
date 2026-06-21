//! src-web — wasm-bindgen + Canvas 2D frontend for the SRC SRPG port.
//!
//! ブラウザ側エントリポイント。`#[wasm_bindgen(start)]` がページ読み込み時に
//! 自動的に呼ばれ、`<canvas id="src-canvas">` を取得して描画ループ・入力ループ
//! を起動する。

#![forbid(unsafe_code)]

mod archive;
mod assets;
mod audio;
mod midi;
mod render;

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    CanvasRenderingContext2d, Event, HtmlCanvasElement, HtmlInputElement, KeyboardEvent, MouseEvent,
};

use src_core::app::Direction;
use src_core::data::event;
use src_core::event_runtime;
use src_core::{App, Input, CANVAS_HEIGHT, CANVAS_WIDTH};

use crate::assets::Assets;

type SharedApp = Rc<RefCell<App>>;
type SharedAssets = Rc<RefCell<Assets>>;
type SharedCtx = Rc<CanvasRenderingContext2d>;
/// 起動時に読み込んだ汎用素材パックの生バイト (ファイル名, bytes)。
/// シナリオ読込で `App` をリセットした後、terrain/sp/戦闘アニメ定義を
/// 再適用する際に再 fetch を避けるためメモリに保持する。
type SharedPacks = Rc<RefCell<Vec<(String, Vec<u8>)>>>;
type SharedBgm = Rc<RefCell<Option<web_sys::HtmlAudioElement>>>;
/// 直近に読み込んだアーカイブ / 単独ファイルの `(ファイル名, バイト列)`。
/// 「最初からやり直す」時に同じシナリオを再ロードするために保持する。
/// `None` の間は同梱のサンプルシナリオが現在シナリオ。
type SharedScenario = Rc<RefCell<Option<(String, Vec<u8>)>>>;

/// デモ用に同梱する SRC `.eve` シナリオ。Pilot / Unit / Place / SetTile などを
/// event_runtime::execute で App / GameDatabase に反映する。
const SAMPLE_SCENARIO_EVE: &str = include_str!("../assets/sample_scenario.eve");

/// ページ読み込み直後に呼び出されるエントリポイント。
/// Entry point invoked on page load by wasm-bindgen.
#[wasm_bindgen(start)]
pub fn start() -> Result<(), JsValue> {
    console_error_panic_hook::set_once();

    let (canvas, ctx) = bind_canvas("src-canvas")?;
    let ctx = Rc::new(ctx);
    let mut app_inner = App::new();
    // ブラウザでは敵/中立/ＮＰＣ フェイズを 1 体ずつ演出し、味方が攻撃されたら
    // 反撃手段 (反撃/回避/防御) を選択させる (SRC 反撃モード)。
    app_inner.set_animate_ai(true);
    // 攻撃解決ごとに命中フラッシュ・着弾・ダメージ数字のネイティブ演出を再生する。
    app_inner.set_animate_battle(true);
    // サンプル .eve シナリオを実行して、Stage / Map / Pilot / Unit / Place を
    // GameDatabase に反映する（元 SRC のシナリオロード経路相当）。
    apply_sample_scenario(&mut app_inner);
    let app: SharedApp = Rc::new(RefCell::new(app_inner));
    let assets: SharedAssets = Rc::new(RefCell::new(Assets::default()));
    let bgm: SharedBgm = Rc::new(RefCell::new(None));
    let scenario: SharedScenario = Rc::new(RefCell::new(None));
    let packs: SharedPacks = Rc::new(RefCell::new(Vec::new()));

    redraw(&ctx, &app, &assets);

    install_input_handlers(&canvas, app.clone(), assets.clone(), ctx.clone())?;
    install_file_picker(
        app.clone(),
        assets.clone(),
        ctx.clone(),
        scenario.clone(),
        packs.clone(),
    )?;
    install_menu_bar(
        app.clone(),
        assets.clone(),
        ctx.clone(),
        scenario.clone(),
        packs.clone(),
    )?;
    install_animation_loop(app.clone(), assets.clone(), ctx.clone(), bgm.clone())?;
    install_input_overlay(app.clone(), assets.clone(), ctx.clone())?;

    // デバッグ用: `window.__srcDebug()` で App 状態サマリを取得可能にする。
    {
        let app_dbg = app.clone();
        let cb = Closure::<dyn Fn() -> String>::new(move || app_dbg.borrow().debug_summary());
        let _ = js_sys::Reflect::set(
            &web_sys::window().unwrap(),
            &JsValue::from_str("__srcDebug"),
            cb.as_ref().unchecked_ref(),
        );
        cb.forget();
    }
    // デバッグ用: `window.__srcTick(dt)` で App.tick を手動駆動する。
    // バックグラウンドタブで requestAnimationFrame が止まる環境でも、
    // `Wait` タイマや HP バー補間を進められるようにするためのフック。
    {
        let app_tk = app.clone();
        let assets_tk = assets.clone();
        let ctx_tk = ctx.clone();
        let cb = Closure::<dyn Fn(f64)>::new(move |dt: f64| {
            let dirty = app_tk.borrow_mut().tick(dt);
            if dirty {
                redraw(&ctx_tk, &app_tk, &assets_tk);
            }
        });
        let _ = js_sys::Reflect::set(
            &web_sys::window().unwrap(),
            &JsValue::from_str("__srcTick"),
            cb.as_ref().unchecked_ref(),
        );
        cb.forget();
    }
    // デバッグ用: `window.__srcImg()` で script_overlay の Picture コマンドと
    // 画像解決状況をダンプ。アイコン未描画の切り分け用。
    {
        let app_img = app.clone();
        let assets_img = assets.clone();
        let cb = Closure::<dyn Fn() -> String>::new(move || {
            let app = app_img.borrow();
            let assets = assets_img.borrow();
            let mut out = format!("images_registered={}\n", assets.images.len() / 2);
            for c in &app.script_overlay().cmds {
                if let src_core::DrawCmd::Picture { path, w, h, .. } = c {
                    let key = path
                        .replace('\\', "/")
                        .rsplit('/')
                        .next()
                        .unwrap_or("")
                        .to_string();
                    let found = assets.find_image(&key).is_some();
                    out.push_str(&format!(
                        "Picture path={path:?} key={key:?} found={found} w={w:?} h={h:?}\n"
                    ));
                }
            }
            out
        });
        let _ = js_sys::Reflect::set(
            &web_sys::window().unwrap(),
            &JsValue::from_str("__srcImg"),
            cb.as_ref().unchecked_ref(),
        );
        cb.forget();
    }
    // デバッグ用: `window.__srcVar("名前")` で `.eve` シナリオ変数を引く。
    // 配列要素は `__srcVar("arr[key]")` で参照できる。未定義は空文字。
    {
        let app_var = app.clone();
        let cb = Closure::<dyn Fn(String) -> String>::new(move |name: String| {
            app_var.borrow().script_var(&name).to_string()
        });
        let _ = js_sys::Reflect::set(
            &web_sys::window().unwrap(),
            &JsValue::from_str("__srcVar"),
            cb.as_ref().unchecked_ref(),
        );
        cb.forget();
    }

    // タイトル画面のアセットを非同期に読み込み、各完了時に再描画。
    let app_for_load = app.clone();
    let assets_for_load = assets.clone();
    let ctx_for_load = ctx.clone();
    if let Err(e) = assets::load_title_assets(assets.clone(), move || {
        redraw(&ctx_for_load, &app_for_load, &assets_for_load);
    }) {
        web_sys::console::warn_1(&e);
    }

    // 汎用素材パック (グラフィック / 戦闘アニメ / 効果音) を vendor-assets/ から
    // 起動時に自動読込する。未配置 (404) は正常系としてスキップ。
    install_asset_packs(app.clone(), assets.clone(), ctx.clone(), packs.clone());
    Ok(())
}

/// 起動時に汎用素材パックを `vendor-assets/` から fetch して読み込む。
/// 再配布規約のためリポジトリには同梱せず、各自が配置したファイルを配信・読込する。
/// 未配置 (HTTP エラー) のパックは警告を出さずスキップする。
fn install_asset_packs(app: SharedApp, assets: SharedAssets, ctx: SharedCtx, packs: SharedPacks) {
    // 汎用素材パック (ZIP) に加え、SRC 本体の標準システムデータ (terrain.txt 等) も
    // 同じ経路で取り込む。`load_into_app` が単体テキストを内容種別で振り分け、
    // `packs` にキャッシュされるためシナリオ読込後の `reapply_packs` でも再適用される。
    for name in ASSET_PACK_FILES
        .iter()
        .chain(SYSTEM_DATA_FILES.iter())
        .copied()
    {
        let app = app.clone();
        let assets = assets.clone();
        let ctx = ctx.clone();
        let packs = packs.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let url = format!("vendor-assets/{name}");
            let bytes = match fetch_bytes(&url).await {
                Ok(b) => b,
                Err(_) => {
                    // 未配置はエラーではない: 案内のみ。
                    web_sys::console::log_1(&JsValue::from_str(&format!(
                        "素材パック {name} 未配置 (vendor-assets/ に置くと起動時に自動読込)"
                    )));
                    return;
                }
            };
            let on_img = make_image_redraw_cb(app.clone(), assets.clone(), ctx.clone());
            // 素材パック (ZIP) は full_load=true で画像/音声/.eve を登録 + entrypoint
            // 実行。SRC 本体システムデータ (terrain.txt / スペシャルパワー.eve 等の単体
            // ファイル) は full_load=false で **データ取込 + .eve ラベル登録のみ** 行い
            // top-level は実行しない (ライブラリ .eve を scenario として走らせない)。
            let full_load = ASSET_PACK_FILES.contains(&name);
            let result = {
                let mut a = app.borrow_mut();
                let mut s = assets.borrow_mut();
                archive::load_into_app(&mut a, &mut s, name, &bytes, &on_img, full_load)
            };
            match result {
                Ok(log) => web_sys::console::log_1(&JsValue::from_str(&format!(
                    "素材パック {name} 読込:\n{log}"
                ))),
                Err(e) => web_sys::console::warn_1(&JsValue::from_str(&format!(
                    "素材パック {name} 読込失敗: {e}"
                ))),
            }
            // シナリオ読込後の再適用用に生バイトをキャッシュ (ASSET_PACK_FILES 順を維持)。
            cache_pack_bytes(&packs, name, bytes);
            redraw(&ctx, &app, &assets);
        });
    }
}

/// 公式配布の汎用素材集ファイル名。再適用順は graphics → battle anim → SFX。
const ASSET_PACK_FILES: [&str; 3] = [
    "SRC_Graph101121.zip",
    "SRC_BA110418.zip",
    "SRC_Wave091207.zip",
];

/// SRC 本体の標準システムデータ。`vendor-assets/` に **単体ファイル** で配置すると
/// 起動時に取り込み、シナリオ読込後も下地として再適用する。再配布規約のため
/// リポジトリには同梱せず各自が SRC 本体 (`Data/System/`) から配置する。
///
/// `terrain.txt` (標準地形 91 種: 平地/道路/街/海/宇宙…) はサンプルを含む多くの
/// シナリオが依存する。これが無いと組込みのミニ地形表 (ID 体系が別物) が使われ、
/// マップの地形名・移動コスト・地形名ベースの進入イベント等が不正になる。
///
/// `スペシャルパワー.eve` は精神コマンド/SP 演出のサブルーチン (`Mindanime` 等) を
/// 定義する共有ライブラリ。シナリオが `Mindanime` 等を Call するため、ラベルを下地
/// として登録しておく必要がある (full_load=false なので top-level は実行しない)。
/// 公式の汎用グラフィック集 (`SRC_Graph*.zip`) にも含まれるが、単体配置にも対応する。
///
/// `load_into_app` は単体テキスト/.eve を内容種別で振り分けるため ZIP 化は不要。
const SYSTEM_DATA_FILES: [&str; 2] = ["terrain.txt", "スペシャルパワー.eve"];

/// 取得した素材パックの生バイトを `ASSET_PACK_FILES` の順序を保ってキャッシュする。
/// 同名は上書き (再ロード対策)。
fn cache_pack_bytes(packs: &SharedPacks, name: &str, bytes: Vec<u8>) {
    let mut p = packs.borrow_mut();
    if let Some(slot) = p.iter_mut().find(|(n, _)| n == name) {
        slot.1 = bytes;
    } else {
        p.push((name.to_string(), bytes));
    }
    // ASSET_PACK_FILES の並びでソート (定義順に再適用するため)。
    p.sort_by_key(|(n, _)| {
        ASSET_PACK_FILES
            .iter()
            .position(|f| f == n)
            .unwrap_or(usize::MAX)
    });
}

/// シナリオ読込で `App` をリセットした直後に、キャッシュ済み素材パックの
/// データ (terrain/sp 等) を `full_load=false` で下地として再適用する。
/// 画像/音声は `Assets` 側に残るため再デコードしない。`.eve` ライブラリは
/// top-level 実行が後続シナリオを壊すため再実行しない (戦闘アニメ画像は
/// `Assets` に残る)。シナリオはこの下地の上に読み込まれる。
fn reapply_packs(app: &mut App, assets: &mut Assets, packs: &SharedPacks, on_img: &Rc<dyn Fn()>) {
    for (name, bytes) in packs.borrow().iter() {
        match archive::load_into_app(app, assets, name, bytes, on_img, false) {
            Ok(_) => web_sys::console::log_1(&JsValue::from_str(&format!(
                "素材パック {name} のデータ (terrain/sp 等) を再適用"
            ))),
            Err(e) => web_sys::console::warn_1(&JsValue::from_str(&format!(
                "素材パック {name} 再適用失敗: {e}"
            ))),
        }
    }
}

/// URL から実バイト列を取得する。HTTP エラー (404 等) は `Err` を返す。
async fn fetch_bytes(url: &str) -> Result<Vec<u8>, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let resp_val = JsFuture::from(window.fetch_with_str(url)).await?;
    let resp: web_sys::Response = resp_val.dyn_into()?;
    if !resp.ok() {
        return Err(JsValue::from_str(&format!("HTTP {}", resp.status())));
    }
    let buf = JsFuture::from(resp.array_buffer()?).await?;
    let arr = js_sys::Uint8Array::new(&buf);
    let mut bytes = vec![0u8; arr.length() as usize];
    arr.copy_to(&mut bytes);
    Ok(bytes)
}

/// 同梱のサンプル `.eve` シナリオを App に反映する。
/// 初期ロードと「最初からやり直す」（シナリオ未読込時）の両方で使う。
fn apply_sample_scenario(app: &mut App) {
    match event::parse(SAMPLE_SCENARIO_EVE) {
        Ok(stmts) => {
            if let Err(e) = event_runtime::execute(app, &stmts) {
                web_sys::console::warn_1(&JsValue::from_str(&format!("sample_scenario.eve: {e}")));
            }
        }
        Err(e) => {
            web_sys::console::warn_1(&JsValue::from_str(&format!(
                "sample_scenario.eve parse: {e}"
            )));
        }
    }
}

fn bind_canvas(id: &str) -> Result<(HtmlCanvasElement, CanvasRenderingContext2d), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let canvas: HtmlCanvasElement = document
        .get_element_by_id(id)
        .ok_or_else(|| JsValue::from_str(&format!("canvas #{id} not found")))?
        .dyn_into()?;

    canvas.set_width(CANVAS_WIDTH);
    canvas.set_height(CANVAS_HEIGHT);
    canvas.set_tab_index(0);

    let ctx: CanvasRenderingContext2d = canvas
        .get_context("2d")?
        .ok_or_else(|| JsValue::from_str("2d context unavailable"))?
        .dyn_into()?;
    Ok((canvas, ctx))
}

fn install_input_handlers(
    canvas: &HtmlCanvasElement,
    app: SharedApp,
    assets: SharedAssets,
    ctx: SharedCtx,
) -> Result<(), JsValue> {
    // click: シーン依存のヒット判定付き
    {
        let app = app.clone();
        let assets = assets.clone();
        let ctx = ctx.clone();
        let canvas_ref = canvas.clone();
        let handler = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
            let _ = canvas_ref.focus();
            let (x, y) = canvas_local(&canvas_ref, &e);
            dispatch(&app, &assets, &ctx, Input::ClickAt { x, y });
        });
        canvas.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref())?;
        handler.forget();
    }
    // contextmenu: 右クリックをキャンセル / RightClickAt として捕まえる
    {
        let app = app.clone();
        let assets = assets.clone();
        let ctx = ctx.clone();
        let canvas_ref = canvas.clone();
        let handler = Closure::<dyn FnMut(MouseEvent)>::new(move |e: MouseEvent| {
            e.prevent_default();
            let _ = canvas_ref.focus();
            let (x, y) = canvas_local(&canvas_ref, &e);
            dispatch(&app, &assets, &ctx, Input::RightClickAt { x, y });
        });
        canvas.add_event_listener_with_callback("contextmenu", handler.as_ref().unchecked_ref())?;
        handler.forget();
    }
    // keydown: 矢印 → カーソル移動、それ以外 → Advance
    {
        let app = app.clone();
        let assets = assets.clone();
        let ctx = ctx.clone();
        let handler = Closure::<dyn FnMut(KeyboardEvent)>::new(move |e: KeyboardEvent| {
            let key = e.key();
            if key == "Tab" {
                return; // フォーカス移動は阻害しない
            }
            // 修飾キー単独 / IME 合成中 / Function キー類は無視。
            // 元 SRC では Shift / Ctrl / Alt 単独で進行しないのに合わせる。
            if matches!(
                key.as_str(),
                "Shift"
                    | "Control"
                    | "Alt"
                    | "Meta"
                    | "CapsLock"
                    | "NumLock"
                    | "ScrollLock"
                    | "Dead"
                    | "Process"
                    | "AltGraph"
                    | "ContextMenu"
                    | "Unidentified"
                    | "F1"
                    | "F2"
                    | "F3"
                    | "F4"
                    | "F5"
                    | "F6"
                    | "F7"
                    | "F8"
                    | "F9"
                    | "F10"
                    | "F11"
                    | "F12"
            ) || e.is_composing()
            {
                return;
            }
            let input = match key.as_str() {
                "ArrowUp" => Input::MoveCursor(Direction::Up),
                "ArrowDown" => Input::MoveCursor(Direction::Down),
                "ArrowLeft" => Input::MoveCursor(Direction::Left),
                "ArrowRight" => Input::MoveCursor(Direction::Right),
                // Space = メッセージ送り・決定 (Advance)。**ターン終了には割り当てない**。
                // 以前は Space=EndPhase で、プロローグ会話を押しっぱなしで送ると味方
                // ターン開始直後にターンが即終了→敵フェイズになる事故があったため、
                // ターン終了はメニュー (マップコマンド「ターン終了」) からのみとする。
                // 会話中は Advance が respond_dialog(0) でページ送り (従来どおり)。
                " " | "Spacebar" => Input::Advance,
                "a" | "A" => Input::AttackTarget,
                "w" | "W" => Input::CycleWeapon,
                // P / U / M = PilotList / UnitList / MapView 切替
                "p" | "P" => Input::GotoPilotList,
                "u" | "U" => Input::GotoUnitList,
                "m" | "M" => Input::GotoMapView,
                // 対話 UI 用: Y / Enter で Yes、N で No
                "y" | "Y" => Input::DialogYes,
                "n" | "N" => Input::DialogNo,
                // Ctrl 無し S = セーブ、Ctrl 無し L = ロード
                "s" | "S" if !e.ctrl_key() && !e.meta_key() => {
                    save_to_local_storage(&app);
                    redraw(&ctx, &app, &assets);
                    return;
                }
                "l" | "L" if !e.ctrl_key() && !e.meta_key() => {
                    if load_from_local_storage(&app) {
                        redraw(&ctx, &app, &assets);
                    }
                    return;
                }
                // 数字キーは Menu 中なら DialogChoice、それ以外ならセーブスロット切替
                k if k.len() == 1 && matches!(k.chars().next(), Some('0'..='9')) => {
                    let n = k.chars().next().unwrap().to_digit(10).unwrap();
                    let in_menu = matches!(
                        app.borrow().pending_dialog(),
                        Some(src_core::PendingDialog::Menu { .. })
                    );
                    if in_menu {
                        e.prevent_default();
                        dispatch(&app, &assets, &ctx, Input::DialogChoice(n));
                    } else {
                        set_current_slot(n as u8, &app);
                        redraw(&ctx, &app, &assets);
                    }
                    return;
                }
                "Escape" | "Esc" => {
                    // Esc は常に Input::Cancel。App 側で文脈判定する:
                    //  - Hotpoint Wait Click 画面 → 右クリック相当で「戻る」
                    //    (トラックパッドの副ボタン設定に依存せず抜けられる)
                    //  - 通常 Menu(Ask) → choice 0 でキャンセル
                    //  - dialog 無し → ActionMode / CommandMenu を抜ける
                    e.prevent_default();
                    dispatch(&app, &assets, &ctx, Input::Cancel);
                    return;
                }
                _ => Input::Advance,
            };
            e.prevent_default();
            dispatch(&app, &assets, &ctx, input);
        });
        canvas.add_event_listener_with_callback("keydown", handler.as_ref().unchecked_ref())?;
        handler.forget();
    }
    Ok(())
}

/// canvas の表示サイズ (CSS px) と論理解像度のスケール差を吸収して
/// canvas-local 整数座標を返す。
fn canvas_local(canvas: &HtmlCanvasElement, e: &MouseEvent) -> (i32, i32) {
    let rect = canvas.get_bounding_client_rect();
    let scale_x = f64::from(canvas.width()) / rect.width().max(1.0);
    let scale_y = f64::from(canvas.height()) / rect.height().max(1.0);
    let x = ((f64::from(e.client_x()) - rect.left()) * scale_x).round() as i32;
    let y = ((f64::from(e.client_y()) - rect.top()) * scale_y).round() as i32;
    (x, y)
}

/// `<input id=src-file>` の change を捕まえてアーカイブ / 単独ファイルを読込。
/// シナリオ画像 (非同期デコード) の onload ごとに呼ぶ再描画コールバックを作る。
///
/// タイトル等の静止画面では animation loop が `dirty` 時しか再描画しないため、
/// アーカイブから取り込んだ画像が後からデコード完了しても画面に反映されない。
/// これを各画像の `onload` 駆動の再描画で補う。`setTimeout` によるトレーリング
/// エッジ集約で、数千枚規模ロード時の過剰再描画を抑える (約 60ms ごと)。
fn make_image_redraw_cb(app: SharedApp, assets: SharedAssets, ctx: SharedCtx) -> Rc<dyn Fn()> {
    let scheduled = Rc::new(std::cell::Cell::new(false));
    Rc::new(move || {
        if scheduled.get() {
            return;
        }
        scheduled.set(true);
        let app = app.clone();
        let assets = assets.clone();
        let ctx = ctx.clone();
        let scheduled = scheduled.clone();
        let cb = Closure::once_into_js(move || {
            scheduled.set(false);
            redraw(&ctx, &app, &assets);
        });
        if let Some(win) = web_sys::window() {
            let _ =
                win.set_timeout_with_callback_and_timeout_and_arguments_0(cb.unchecked_ref(), 60);
        }
    })
}

fn install_file_picker(
    app: SharedApp,
    assets: SharedAssets,
    ctx: SharedCtx,
    scenario: SharedScenario,
    packs: SharedPacks,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let Some(input_el) = document.get_element_by_id("src-file") else {
        return Ok(());
    };
    let input: HtmlInputElement = input_el.dyn_into()?;
    let input_for_handler = input.clone();
    let handler = Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
        let files = match input_for_handler.files() {
            Some(f) => f,
            None => return,
        };
        let Some(file) = files.item(0) else { return };
        let name = file.name();
        let promise = file.array_buffer();
        let app = app.clone();
        let assets = assets.clone();
        let ctx = ctx.clone();
        let scenario = scenario.clone();
        let packs = packs.clone();
        wasm_bindgen_futures::spawn_local(async move {
            match JsFuture::from(promise).await {
                Ok(buf) => {
                    let arr = js_sys::Uint8Array::new(&buf);
                    let mut bytes = vec![0u8; arr.length() as usize];
                    arr.copy_to(&mut bytes);
                    let on_img = make_image_redraw_cb(app.clone(), assets.clone(), ctx.clone());
                    {
                        let mut a = app.borrow_mut();
                        let mut s = assets.borrow_mut();
                        // 既存状態をリセットし、汎用素材パックのライブラリ/データを
                        // 下地として再適用してから、新シナリオをその上に読み込む。
                        // Assets (画像/音声) はリセットせず保持する。
                        *a = src_core::App::new();
                        a.set_animate_ai(true);
                        a.set_animate_battle(true);
                        reapply_packs(&mut a, &mut s, &packs, &on_img);
                    }
                    let result = {
                        let mut a = app.borrow_mut();
                        let mut s = assets.borrow_mut();
                        archive::load_into_app(&mut a, &mut s, &name, &bytes, &on_img, true)
                    };
                    match result {
                        Ok(log) => {
                            web_sys::console::log_1(&JsValue::from_str(&log));
                            // 「最初からやり直す」用に読込元を保持。
                            *scenario.borrow_mut() = Some((name.clone(), bytes.clone()));
                            let mut a = app.borrow_mut();
                            a.push_message(format!("読込完了: {name}"));
                        }
                        Err(e) => {
                            web_sys::console::warn_1(&JsValue::from_str(&format!(
                                "アーカイブ読込エラー: {e}"
                            )));
                            let mut a = app.borrow_mut();
                            a.push_message(format!("読込失敗: {e}"));
                        }
                    }
                    redraw(&ctx, &app, &assets);
                }
                Err(e) => {
                    web_sys::console::warn_1(&e);
                }
            }
        });
    });
    input.add_event_listener_with_callback("change", handler.as_ref().unchecked_ref())?;
    handler.forget();
    Ok(())
}

/// 現在のシナリオ状態を破棄して最初からやり直す。シナリオが読込済みなら同じ
/// アーカイブを再ロードし、未読込ならサンプルシナリオを最初から実行する。
/// メニュー「最初からやり直す」から呼ぶ。誤操作防止に確認ダイアログを挟む。
fn perform_reset(
    app: &SharedApp,
    assets: &SharedAssets,
    ctx: &SharedCtx,
    scenario: &SharedScenario,
    packs: &SharedPacks,
) {
    let confirmed = web_sys::window()
        .and_then(|w| {
            w.confirm_with_message(
                "現在のシナリオ状態をリセットして最初からやり直します。よろしいですか？",
            )
            .ok()
        })
        .unwrap_or(false);
    if !confirmed {
        return;
    }
    // App を初期化し、汎用素材パックのライブラリ/データを下地として再適用
    // してから、シナリオ (またはサンプル) を再反映する。
    let on_img = make_image_redraw_cb(app.clone(), assets.clone(), ctx.clone());
    {
        let mut a = app.borrow_mut();
        let mut s = assets.borrow_mut();
        *a = App::new();
        a.set_animate_ai(true);
        a.set_animate_battle(true);
        reapply_packs(&mut a, &mut s, packs, &on_img);
    }
    let cached = scenario.borrow().clone();
    match cached {
        Some((name, bytes)) => {
            let result = {
                let mut a = app.borrow_mut();
                let mut s = assets.borrow_mut();
                archive::load_into_app(&mut a, &mut s, &name, &bytes, &on_img, true)
            };
            match result {
                Ok(log) => {
                    web_sys::console::log_1(&JsValue::from_str(&log));
                    app.borrow_mut()
                        .push_message(format!("最初からやり直し: {name}"));
                }
                Err(e) => {
                    web_sys::console::warn_1(&JsValue::from_str(&format!(
                        "リセット再読込エラー: {e}"
                    )));
                    app.borrow_mut().push_message(format!("リセット失敗: {e}"));
                }
            }
        }
        None => {
            apply_sample_scenario(&mut app.borrow_mut());
            app.borrow_mut()
                .push_message("最初からやり直し: サンプルシナリオ".to_string());
        }
    }
    redraw(ctx, app, assets);
}

/// 指定 id の要素に click リスナを 1 つ取り付ける小ヘルパ。要素が無ければ no-op。
/// メニュー項目の配線で繰り返し使う。
fn on_click(id: &str, f: impl FnMut(Event) + 'static) -> Result<(), JsValue> {
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| JsValue::from_str("no document"))?;
    if let Some(el) = document.get_element_by_id(id) {
        let handler = Closure::<dyn FnMut(Event)>::new(f);
        el.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref())?;
        handler.forget();
    }
    Ok(())
}

/// 現在のセーブスロット表示 (slot-row の aria-current) を更新する。
fn mark_current_slot(slot: u8) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    for i in 0..10u8 {
        if let Some(el) = document.get_element_by_id(&format!("mi-slot-{i}")) {
            if i == slot {
                let _ = el.set_attribute("aria-current", "true");
            } else {
                let _ = el.remove_attribute("aria-current");
            }
        }
    }
}

/// メニューバー (システム / マップコマンド / ヘルプ) の各項目を配線する。
/// 動作はすべて既存の入力経路 (`dispatch` / セーブ・ロード / `perform_reset`) と
/// `App` の公開メソッドを組み合わせて実現し、新たなゲームロジックは持たない。
fn install_menu_bar(
    app: SharedApp,
    assets: SharedAssets,
    ctx: SharedCtx,
    scenario: SharedScenario,
    packs: SharedPacks,
) -> Result<(), JsValue> {
    // ===== システム =====
    // シナリオ読込: 隠しファイル入力 (#src-file) のダイアログを開く。
    on_click("mi-load-scenario", move |_e: Event| {
        if let Some(el) = web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| d.get_element_by_id("src-file"))
        {
            if let Ok(input) = el.dyn_into::<web_sys::HtmlElement>() {
                input.click();
            }
        }
    })?;
    // 最初からやり直す
    {
        let (a, s, c, sc, pk) = (
            app.clone(),
            assets.clone(),
            ctx.clone(),
            scenario.clone(),
            packs.clone(),
        );
        on_click("mi-reset", move |_e: Event| {
            perform_reset(&a, &s, &c, &sc, &pk)
        })?;
    }
    // セーブ / ロード (現在スロット)
    {
        let (a, s, c) = (app.clone(), assets.clone(), ctx.clone());
        on_click("mi-save", move |_e: Event| {
            save_to_local_storage(&a);
            redraw(&c, &a, &s);
        })?;
    }
    {
        let (a, s, c) = (app.clone(), assets.clone(), ctx.clone());
        on_click("mi-load", move |_e: Event| {
            if load_from_local_storage(&a) {
                redraw(&c, &a, &s);
            }
        })?;
    }
    // セーブスロット 0..=9 切替
    for slot in 0..10u8 {
        let (a, s, c) = (app.clone(), assets.clone(), ctx.clone());
        on_click(&format!("mi-slot-{slot}"), move |_e: Event| {
            set_current_slot(slot, &a);
            mark_current_slot(slot);
            redraw(&c, &a, &s);
        })?;
    }
    mark_current_slot(current_slot());

    // ===== マップコマンド (原典準拠) =====
    {
        let (a, s, c) = (app.clone(), assets.clone(), ctx.clone());
        on_click("mi-endturn", move |_e: Event| {
            dispatch(&a, &s, &c, Input::EndPhase);
        })?;
    }
    {
        // 部隊表: 原典「部隊表」= マップコマンド UnitList と同じく PilotList 画面へ。
        let (a, s, c) = (app.clone(), assets.clone(), ctx.clone());
        on_click("mi-unitlist", move |_e: Event| {
            dispatch(&a, &s, &c, Input::GotoPilotList);
        })?;
    }
    {
        let (a, s, c) = (app.clone(), assets.clone(), ctx.clone());
        on_click("mi-autocounter", move |_e: Event| {
            let on = a.borrow_mut().toggle_auto_counter();
            a.borrow_mut().push_message(format!(
                "自動反撃モード: {}",
                if on {
                    "ＯＮ (自動反撃)"
                } else {
                    "ＯＦＦ (手動選択)"
                }
            ));
            redraw(&c, &a, &s);
        })?;
    }
    {
        let (a, s, c) = (app.clone(), assets.clone(), ctx.clone());
        on_click("mi-settings", move |_e: Event| {
            a.borrow_mut().enter_configuration();
            redraw(&c, &a, &s);
        })?;
    }
    {
        let (a, s, c) = (app.clone(), assets.clone(), ctx.clone());
        on_click("mi-quicksave", move |_e: Event| {
            quick_save_to_local_storage(&a);
            redraw(&c, &a, &s);
        })?;
    }
    {
        let (a, s, c) = (app.clone(), assets.clone(), ctx.clone());
        on_click("mi-quickload", move |_e: Event| {
            if quick_load_from_local_storage(&a) {
                redraw(&c, &a, &s);
            }
        })?;
    }

    // ===== ヘルプ =====
    on_click("mi-help-keys", move |_e: Event| {
        if let Some(w) = web_sys::window() {
            let _ = w.alert_with_message(
                "操作方法\n\n\
                 ・矢印キー / クリック: カーソル移動・選択\n\
                 ・Enter / Space / クリック: 決定・メッセージ送り\n\
                 ・右クリック / Esc: キャンセル\n\
                 ・ターン終了: メニューの「マップコマンド → ターン終了」\n\
                 ・a: 攻撃  w: 武器切替\n\
                 ・p: 部隊表(パイロット)  u: ユニット一覧  m: マップ\n\
                 ・s: セーブ  l: ロード  0-9: セーブスロット切替",
            );
        }
    })?;
    on_click("mi-help-about", move |_e: Event| {
        if let Some(w) = web_sys::window() {
            let _ = w.alert_with_message(
                "SRC (Simulation RPG Construction) — Rust / WebAssembly port\n\
                 VB6 / SRC.NET 原典を Rust + wasm-bindgen に移植。",
            );
        }
    })?;
    {
        // デバッグ情報をクリップボードへコピー (App 状態サマリ = `__srcDebug()` と同内容)。
        let a = app.clone();
        on_click("mi-copy-debug", move |_e: Event| {
            let summary = a.borrow().debug_summary();
            copy_to_clipboard(&summary);
            if let Some(w) = web_sys::window() {
                let _ = w.alert_with_message("デバッグ情報をクリップボードにコピーしました。");
            }
        })?;
    }
    Ok(())
}

/// クイックセーブ用 localStorage キー。スロット式セーブとは独立。
const QUICKSAVE_KEY: &str = "src_srpg_rs_quicksave";

/// 現在の App 状態をクイックセーブ専用キーに保存する (原典マップコマンド
/// 「クイックセーブ」相当)。
fn quick_save_to_local_storage(app: &SharedApp) {
    let json = match app.borrow().to_save_json() {
        Ok(s) => s,
        Err(e) => {
            web_sys::console::warn_1(&JsValue::from_str(&format!("quicksave failed: {e}")));
            return;
        }
    };
    let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) else {
        return;
    };
    if let Err(e) = storage.set_item(QUICKSAVE_KEY, &json) {
        web_sys::console::warn_1(&e);
    } else {
        app.borrow_mut()
            .push_message(format!("クイックセーブ完了 ({} bytes)", json.len()));
    }
}

/// クイックセーブ地点から復元する (原典「クイックロード」相当)。
/// ロード後は `再開` ラベルを発火して PaintPicture 等の画面描画を復元する。
fn quick_load_from_local_storage(app: &SharedApp) -> bool {
    let Some(storage) = web_sys::window().and_then(|w| w.local_storage().ok().flatten()) else {
        return false;
    };
    let json = match storage.get_item(QUICKSAVE_KEY) {
        Ok(Some(j)) => j,
        _ => {
            app.borrow_mut()
                .push_message("クイックセーブデータがありません".to_string());
            return true;
        }
    };
    match src_core::App::from_save_json(&json) {
        Ok(loaded) => {
            {
                let mut a = app.borrow_mut();
                *a = loaded;
                a.fire_resume_event();
                a.push_message("クイックロード完了".to_string());
            }
            true
        }
        Err(e) => {
            web_sys::console::warn_1(&JsValue::from_str(&format!("quickload failed: {e}")));
            app.borrow_mut().push_message(format!("ロード失敗: {e}"));
            true
        }
    }
}

/// スクリプト (`Quickload` / `Restart` / ゲームオーバー コンティニュー) が要求した
/// 再ロードを処理する。`App::take_pending_reload` が `Some(json)` を返したら
/// `from_save_json` で App 全体を置換し、`再開` ラベルを発火して画面を復元する。
/// 置換した場合 `true` (呼び出し側で再描画する)。
fn perform_pending_reload(app: &SharedApp) -> bool {
    let json = app.borrow_mut().take_pending_reload();
    let Some(json) = json else {
        return false;
    };
    match src_core::App::from_save_json(&json) {
        Ok(loaded) => {
            let mut a = app.borrow_mut();
            *a = loaded;
            a.fire_resume_event();
            a.push_message("コンティニュー: ステージを再開".to_string());
            true
        }
        Err(e) => {
            web_sys::console::warn_1(&JsValue::from_str(&format!("reload failed: {e}")));
            app.borrow_mut().push_message(format!("再ロード失敗: {e}"));
            false
        }
    }
}

/// `AudioRequest` を実際の HtmlAudioElement / PicoAudio.js 呼び出しに変換。
fn process_audio_request(
    req: src_core::AudioRequest,
    assets: &mut Assets,
    bgm: &SharedBgm,
    volume: u8,
) {
    use src_core::AudioRequest as A;
    match req {
        A::StartBgm { name } => {
            // 既存 BGM を停止
            if let Some(prev) = bgm.borrow_mut().take() {
                audio::stop_audio(&prev);
            }
            midi::stop_midi();
            // MIDI 優先で探索
            if let Some(bytes) = assets.find_midi(&name).cloned() {
                if let Err(e) = midi::play_midi(&bytes, volume, true) {
                    web_sys::console::warn_1(&JsValue::from_str(&format!(
                        "Startbgm midi {name}: {}",
                        e.as_string().unwrap_or_default()
                    )));
                }
                return;
            }
            if let Some((bytes, mime)) = assets.find_audio(&name).cloned() {
                match audio::play_audio(&bytes, mime, volume, true) {
                    Ok(handle) => {
                        *bgm.borrow_mut() = Some(handle);
                    }
                    Err(e) => web_sys::console::warn_1(&JsValue::from_str(&format!(
                        "Startbgm audio {name}: {}",
                        e.as_string().unwrap_or_default()
                    ))),
                }
            }
        }
        A::StopBgm => {
            if let Some(prev) = bgm.borrow_mut().take() {
                audio::stop_audio(&prev);
            }
            midi::stop_midi();
        }
        A::KeepBgm => {
            // 本実装ではフラグなしで動作させる (次の Startbgm でも明示的に再生)。
            // 将来、シーン遷移で BGM を自動停止するロジックを足す時にフラグ参照する。
        }
        A::PlaySound { name } | A::PlayVoice { name } => {
            if let Some((bytes, mime)) = assets.find_audio(&name).cloned() {
                if let Err(e) = audio::play_audio(&bytes, mime, volume, false) {
                    web_sys::console::warn_1(&JsValue::from_str(&format!(
                        "Playsound {name}: {}",
                        e.as_string().unwrap_or_default()
                    )));
                }
                return;
            }
            // MIDI を SE 的に使うケースもある (1 回再生)
            if let Some(bytes) = assets.find_midi(&name).cloned() {
                if let Err(e) = midi::play_midi(&bytes, volume, false) {
                    web_sys::console::warn_1(&JsValue::from_str(&format!(
                        "Playsound midi {name}: {}",
                        e.as_string().unwrap_or_default()
                    )));
                }
            }
        }
        A::PlayMidi { name } => {
            // 純粋 MIDI 再生 (PlaySound の MIDI fallback と異なり、最初から MIDI 経路)
            if let Some(bytes) = assets.find_midi(&name).cloned() {
                if let Err(e) = midi::play_midi(&bytes, volume, false) {
                    web_sys::console::warn_1(&JsValue::from_str(&format!(
                        "PlayMIDI {name}: {}",
                        e.as_string().unwrap_or_default()
                    )));
                }
            }
        }
    }
}

/// `requestAnimationFrame` ループを開始し、App.tick(dt) で HP バー等の補間を進める。
/// アニメーションが進行中（dirty=true）のフレームだけ再描画する。
/// 併せて `App.take_pending_audio()` をドレインして BGM / SE を実再生する。
fn install_animation_loop(
    app: SharedApp,
    assets: SharedAssets,
    ctx: SharedCtx,
    bgm: SharedBgm,
) -> Result<(), JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("no window"))?;
    let perf = window
        .performance()
        .ok_or_else(|| JsValue::from_str("no performance"))?;

    type AnimCallback = Closure<dyn FnMut()>;
    let f: Rc<RefCell<Option<AnimCallback>>> = Rc::new(RefCell::new(None));
    let g = f.clone();
    let last_t = Rc::new(RefCell::new(perf.now()));

    *g.borrow_mut() = Some(Closure::<dyn FnMut()>::new(move || {
        let win = web_sys::window().unwrap();
        let perf = win.performance().unwrap();
        let now = perf.now();
        let dt_secs = {
            let mut last = last_t.borrow_mut();
            let d = (now - *last) / 1000.0;
            *last = now;
            d.min(0.1) // フレームスキップ時のオーバーシュート防止
        };

        // SRC 時間関数 (`Now`/`Year`/...) が参照する wall clock を毎フレーム
        // 更新。`js_sys::Date::now()` は Unix epoch ミリ秒 (UTC) を返す。
        let wall_now_ms = js_sys::Date::now();
        let dirty = {
            let mut a = app.borrow_mut();
            a.set_wall_clock_ms(wall_now_ms);
            a.tick(dt_secs)
        };
        // pending_audio をドレインして処理
        let audio_reqs = app.borrow_mut().take_pending_audio();
        if !audio_reqs.is_empty() {
            let vol = app.borrow().settings().mp3_volume;
            let mut assets_ref = assets.borrow_mut();
            for req in audio_reqs {
                process_audio_request(req, &mut assets_ref, &bgm, vol);
            }
        }
        // スクリプト駆動の再ロード要求 (Quickload / Restart) を処理。
        let reloaded = perform_pending_reload(&app);
        if dirty || reloaded {
            redraw(&ctx, &app, &assets);
        }

        // 次のフレームを予約
        let next = f.borrow();
        if let Some(cb) = next.as_ref() {
            let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
        }
    }));

    web_sys::window()
        .unwrap()
        .request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref())?;
    Ok(())
}

/// `PendingDialog::Input` 用のテキスト入力オーバーレイ（HTML form）を設置し、
/// dialog の出現/消滅に同期して表示/非表示を切替える。
fn install_input_overlay(
    app: SharedApp,
    assets: SharedAssets,
    ctx: SharedCtx,
) -> Result<(), JsValue> {
    use wasm_bindgen::JsCast;
    use web_sys::{Element, HtmlElement, HtmlInputElement};

    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let Some(overlay) = document.get_element_by_id("src-input-overlay") else {
        return Ok(());
    };
    let overlay: HtmlElement = overlay.dyn_into()?;
    let prompt_el: Element = document
        .get_element_by_id("src-input-prompt")
        .ok_or_else(|| JsValue::from_str("no #src-input-prompt"))?;
    let field: HtmlInputElement = document
        .get_element_by_id("src-input-field")
        .ok_or_else(|| JsValue::from_str("no #src-input-field"))?
        .dyn_into()?;
    let cancel_btn: HtmlElement = document
        .get_element_by_id("src-input-cancel")
        .ok_or_else(|| JsValue::from_str("no #src-input-cancel"))?
        .dyn_into()?;
    let form_el: Element = document
        .get_element_by_id("src-input-form")
        .ok_or_else(|| JsValue::from_str("no #src-input-form"))?;

    // overlay 状態を `requestAnimationFrame` 毎にチェックして同期。
    // 単純化のため毎フレーム pending_dialog を参照する。
    type AnimCallback = Closure<dyn FnMut()>;
    let f: Rc<RefCell<Option<AnimCallback>>> = Rc::new(RefCell::new(None));
    let g = f.clone();
    let overlay_for_tick = overlay.clone();
    let prompt_for_tick = prompt_el.clone();
    let field_for_tick = field.clone();
    let app_for_tick = app.clone();
    let last_kind = Rc::new(RefCell::new(String::new()));
    *g.borrow_mut() = Some(Closure::<dyn FnMut()>::new(move || {
        let a = app_for_tick.borrow();
        let mut last = last_kind.borrow_mut();
        match a.pending_dialog() {
            Some(src_core::PendingDialog::Input {
                prompt, default, ..
            }) => {
                let kind = format!("Input:{prompt}:{default}");
                if *last != kind {
                    prompt_for_tick.set_text_content(Some(prompt));
                    field_for_tick.set_value(default);
                    let _ = overlay_for_tick.style().set_property("display", "flex");
                    let _ = field_for_tick.focus();
                    *last = kind;
                }
            }
            _ => {
                if !last.is_empty() {
                    let _ = overlay_for_tick.style().set_property("display", "none");
                    *last = String::new();
                }
            }
        }
        let win = web_sys::window().unwrap();
        let next = f.borrow();
        if let Some(cb) = next.as_ref() {
            let _ = win.request_animation_frame(cb.as_ref().unchecked_ref());
        }
    }));
    web_sys::window()
        .unwrap()
        .request_animation_frame(g.borrow().as_ref().unwrap().as_ref().unchecked_ref())?;

    // submit ハンドラ
    {
        let app = app.clone();
        let ctx = ctx.clone();
        let assets = assets.clone();
        let field = field.clone();
        let handler = Closure::<dyn FnMut(Event)>::new(move |e: Event| {
            e.prevent_default();
            let text = field.value();
            app.borrow_mut().respond_dialog_text(text);
            redraw(&ctx, &app, &assets);
        });
        form_el.add_event_listener_with_callback("submit", handler.as_ref().unchecked_ref())?;
        handler.forget();
    }
    // cancel ボタンハンドラ — default のまま再開
    {
        let app = app.clone();
        let ctx = ctx.clone();
        let assets = assets.clone();
        let handler = Closure::<dyn FnMut(Event)>::new(move |_e: Event| {
            // default をセットして再開（respond_dialog_text に "" でも default 既設定）
            let default = match app.borrow().pending_dialog() {
                Some(src_core::PendingDialog::Input { default, .. }) => default.clone(),
                _ => String::new(),
            };
            app.borrow_mut().respond_dialog_text(default);
            redraw(&ctx, &app, &assets);
        });
        cancel_btn.add_event_listener_with_callback("click", handler.as_ref().unchecked_ref())?;
        handler.forget();
    }
    Ok(())
}

/// セーブスロット (0..=9)。現在スロットは module-local RefCell に保持。
/// 0-9 数字キーで「現在スロット」を切替、S でその番号に保存、L で復元。
const SAVE_KEY_PREFIX: &str = "src_srpg_rs_save_";

thread_local! {
    static CURRENT_SLOT: RefCell<u8> = const { RefCell::new(0) };
}

fn set_current_slot(slot: u8, app: &SharedApp) {
    CURRENT_SLOT.with(|s| *s.borrow_mut() = slot);
    let mut a = app.borrow_mut();
    a.push_message(format!("セーブスロットを {slot} に設定"));
}

fn current_slot() -> u8 {
    CURRENT_SLOT.with(|s| *s.borrow())
}

fn save_to_local_storage(app: &SharedApp) {
    let slot = current_slot();
    let json = {
        let a = app.borrow();
        match a.to_save_json() {
            Ok(s) => s,
            Err(e) => {
                web_sys::console::warn_1(&JsValue::from_str(&format!("save failed: {e}")));
                return;
            }
        }
    };
    let Some(window) = web_sys::window() else {
        return;
    };
    let storage = match window.local_storage() {
        Ok(Some(s)) => s,
        _ => return,
    };
    let key = format!("{SAVE_KEY_PREFIX}{slot}");
    if let Err(e) = storage.set_item(&key, &json) {
        web_sys::console::warn_1(&e);
    } else {
        let mut a = app.borrow_mut();
        a.push_message(format!("セーブ完了: slot {slot} ({} bytes)", json.len()));
    }
}

fn load_from_local_storage(app: &SharedApp) -> bool {
    let slot = current_slot();
    let Some(window) = web_sys::window() else {
        return false;
    };
    let storage = match window.local_storage() {
        Ok(Some(s)) => s,
        _ => return false,
    };
    let key = format!("{SAVE_KEY_PREFIX}{slot}");
    let json = match storage.get_item(&key) {
        Ok(Some(j)) => j,
        _ => {
            let mut a = app.borrow_mut();
            a.push_message(format!("slot {slot}: セーブデータがありません"));
            return true;
        }
    };
    match src_core::App::from_save_json(&json) {
        Ok(loaded) => {
            *app.borrow_mut() = loaded;
            let mut a = app.borrow_mut();
            a.push_message(format!("ロード完了: slot {slot}"));
            true
        }
        Err(e) => {
            web_sys::console::warn_1(&JsValue::from_str(&format!("load failed: {e}")));
            let mut a = app.borrow_mut();
            a.push_message(format!("ロード失敗: {e}"));
            true
        }
    }
}

fn dispatch(app: &SharedApp, assets: &SharedAssets, ctx: &SharedCtx, input: Input) {
    let need_redraw = app.borrow_mut().handle_input(input);
    // スクリプトが `Quickload` / `Restart` を要求していれば App を置換する
    // (ゲームオーバー → コンティニューでのステージ再開等)。
    let reloaded = perform_pending_reload(app);
    if need_redraw || reloaded {
        redraw(ctx, app, assets);
    }
}

/// テキストを `navigator.clipboard.writeText` でクリップボードへコピーする。
/// Promise は await せず fire-and-forget (コピー自体はユーザジェスチャ内で発火済み)。
fn copy_to_clipboard(text: &str) {
    let Some(win) = web_sys::window() else {
        return;
    };
    let _ = win.navigator().clipboard().write_text(text);
    web_sys::console::log_1(&JsValue::from_str(&format!(
        "[srcDebug] copied {} chars to clipboard",
        text.len()
    )));
}

fn redraw(ctx: &SharedCtx, app: &SharedApp, assets: &SharedAssets) {
    let app_ref = app.borrow();
    let last_msg = app_ref.messages().last().map(|s| s.as_str());
    let total = app_ref.messages().len();
    let intermission_items: Vec<String> = (0..app_ref.intermission_item_count())
        .filter_map(|i| app_ref.intermission_item_label(i))
        .collect();
    // 単機ステータス詳細のビューモデル (UnitDetail シーン時のみ構築)。
    let unit_detail = if app_ref.scene() == src_core::Scene::UnitDetail {
        app_ref.build_status_detail(app_ref.status_detail_index())
    } else {
        None
    };
    // 反撃手段選択中なら戦闘窓ビューモデルを構築。
    let reaction_data = app_ref.reaction_window_data();
    render::draw_scene(
        ctx,
        app_ref.scene(),
        &assets.borrow(),
        app_ref.settings(),
        app_ref.database(),
        app_ref.map_cursor(),
        app_ref.turn(),
        app_ref.map_scroll(),
        app_ref.stage(),
        last_msg,
        app_ref.selected_weapon_idx(),
        total,
        app_ref.pending_dialog(),
        app_ref.script_overlay(),
        app_ref.command_menu(),
        app_ref.action_mode(),
        app_ref.hotpoints(),
        &intermission_items,
        app_ref.intermission_cursor(),
        app_ref.battle_anim(),
        app_ref.move_anim(),
        unit_detail.as_ref(),
        reaction_data.as_ref(),
    );
}
