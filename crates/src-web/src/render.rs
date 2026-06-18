//! Canvas 2D 描画レイヤ / Canvas 2D drawing layer.
//!
//! `src-core` の論理シーン定義を受け取り、Canvas に描画する。原典 VB6 の
//! `Form` 描画と等価な役割。

use web_sys::CanvasRenderingContext2d;

use src_core::combat;
use src_core::data::terrain;
use src_core::scene::configuration::{
    ConfigurationLayout, LabelledControl, CAPTION as CFG_CAPTION, CONFIG_HEIGHT, CONFIG_WIDTH,
    TITLE_BAR_HEIGHT,
};
use src_core::scene::map_view::{
    MAP_VIEW_HEIGHT, MAP_VIEW_WIDTH, MESSAGE_BOX_HEIGHT, MESSAGE_BOX_X, MESSAGE_BOX_Y,
    PORTRAIT_SIZE, STATUS_PANEL_H, STATUS_PANEL_WIDTH, STATUS_PANEL_X, STATUS_PANEL_Y, TILE_SIZE,
    VIEW_TILES_X, VIEW_TILES_Y,
};
use src_core::scene::pilot_list::{
    self as plist, COLUMNS as PL_COLUMNS, HEADER_TOP, PILOT_LIST_HEIGHT, PILOT_LIST_WIDTH,
    ROW_HEIGHT,
};
use src_core::scene::title::{
    self, Rect, TitleLayout, AUTHORS, LICENSE_NOTICE, TITLE_HEIGHT, TITLE_WIDTH,
};
use src_core::scene::unit_detail::{
    self as udetail, StatusDetail, UNIT_DETAIL_HEIGHT, UNIT_DETAIL_WIDTH,
};
use src_core::scene::unit_list::{
    self as ulist, COLUMNS as UL_COLUMNS, HEADER_TOP as UL_HEADER_TOP, ROW_HEIGHT as UL_ROW_HEIGHT,
    UNIT_LIST_HEIGHT, UNIT_LIST_WIDTH,
};
use src_core::settings::Settings;
use src_core::UnitInstance;
use src_core::{GameDatabase, Scene, Turn, CANVAS_HEIGHT, CANVAS_WIDTH};

use crate::assets::Assets;

const JP_SANS: &str = "'ＭＳ Ｐゴシック', 'Hiragino Kaku Gothic ProN', 'Yu Gothic UI', sans-serif";

#[allow(clippy::too_many_arguments)]
pub fn draw_scene(
    ctx: &CanvasRenderingContext2d,
    scene: Scene,
    assets: &Assets,
    settings: &Settings,
    database: &GameDatabase,
    cursor: Option<(u32, u32)>,
    turn: Turn,
    scroll: (u32, u32),
    stage: &str,
    last_message: Option<&str>,
    selected_weapon_idx: usize,
    messages_total: usize,
    pending_dialog: Option<&src_core::PendingDialog>,
    script_overlay: &src_core::ScriptOverlay,
    command_menu: Option<&src_core::CommandMenu>,
    action_mode: src_core::ActionMode,
    hotpoints: &[src_core::event_runtime::HotpointEntry],
    intermission_items: &[String],
    intermission_cursor: usize,
    battle_anim: Option<&src_core::BattleAnim>,
    move_anim: Option<&src_core::MoveAnim>,
    unit_detail: Option<&StatusDetail>,
) {
    clear(ctx, "#2a2a30"); // 周囲のレターボックスは暗色
    match scene {
        Scene::Title => draw_title(ctx, assets),
        Scene::Configuration => draw_configuration(ctx, settings),
        Scene::Intermission => draw_intermission(ctx, intermission_items, intermission_cursor),
        Scene::MapView => {
            // .eve が Hotpoint 付きの独自画面（タイトル / OP / 難易度設定 /
            // キャラメイキング等）を script_overlay で描いている間は、その画面が
            // 完結した UI なのでマップ・行動オーバーレイ・ステージ状態オーバーレイ
            // を描かず .eve の描画のみを表示する。エンジン既定の UI（ステータス
            // パネル / メッセージボックス / Briefing 暗幕）が独自画面に重なる
            // 不具合を防ぐ。
            let custom_screen = !hotpoints.is_empty() && !script_overlay.cmds.is_empty();
            // `.eve` が opening 等を描画中なら、エンジンのフォールバック表示
            // (no-map 警告 / Briefing 全画面暗幕) は抑制する。Talk dialog 中も
            // 同様: ストーリー演出を engine UI が遮ってはならない。
            let script_active = !script_overlay.cmds.is_empty() || pending_dialog.is_some();
            if custom_screen {
                draw_script_overlay(ctx, script_overlay, assets);
            } else {
                draw_map_view(
                    ctx,
                    database,
                    cursor,
                    turn,
                    scroll,
                    stage,
                    last_message,
                    assets,
                    selected_weapon_idx,
                    messages_total,
                    script_active,
                    action_mode.clone(),
                    battle_anim,
                    move_anim,
                );
                // 行動済みユニットのグレーアウト + ActionMode 範囲オーバーレイ
                draw_action_overlays(ctx, database, scroll, action_mode);
                // .eve スクリプトの蓄積描画コマンドは MapView のみで表示。
                // 設定画面 / タイトルへの透過を防ぐ。
                draw_script_overlay(ctx, script_overlay, assets);
                // ネイティブ戦闘演出 (命中フラッシュ・着弾・ダメージ数字)。
                // ユニットチップの上、コマンドメニュー/暗幕の下に重ねる。
                if let Some(anim) = battle_anim {
                    draw_battle_anim(ctx, anim, scroll, assets);
                }
                // コマンドメニュー（ユニット / マップ）
                if let Some(m) = command_menu {
                    draw_command_menu(ctx, m);
                }
            }
        }
        Scene::PilotList => draw_pilot_list(ctx, database, assets),
        Scene::UnitList => draw_unit_list(ctx, database, assets),
        Scene::UnitDetail => draw_unit_detail(ctx, unit_detail, assets),
    }
    // Dialog (Talk / Confirm) は全シーンより上に描画
    if let Some(d) = pending_dialog {
        // Wait Click + Hotpoint 経由で生成された Menu (`store_value=true`) は、
        // ユーザがキャンバス上の Hotpoint を直接クリックして選ぶ前提なので、
        // ダイアログオーバーレイを描画しないでおく。
        if let src_core::PendingDialog::Menu {
            store_value: true, ..
        } = d
        {
            if !hotpoints.is_empty() {
                return;
            }
        }
        draw_dialog_overlay(ctx, d, assets, database);
    }
}

/// 行動済みユニットの上に半透明グレーをかぶせ、
/// ActionMode が MoveSelect / AttackSelect の場合は範囲を可視化する。
fn draw_action_overlays(
    ctx: &CanvasRenderingContext2d,
    database: &GameDatabase,
    scroll: (u32, u32),
    action_mode: src_core::ActionMode,
) {
    let Some(map) = database.map.as_ref() else {
        return;
    };
    let (sx, sy) = scroll;
    // draw_map_view と同じオフセット計算 (canvas 中央寄せ)
    let ox = (i64::from(CANVAS_WIDTH) - i64::from(MAP_VIEW_WIDTH)) / 2;
    let oy = (i64::from(CANVAS_HEIGHT) - i64::from(MAP_VIEW_HEIGHT)) / 2;
    let to_screen = |tx: u32, ty: u32| -> (i64, i64) {
        let dx = i64::from(tx) - i64::from(sx);
        let dy = i64::from(ty) - i64::from(sy);
        (
            ox + dx * i64::from(TILE_SIZE),
            oy + dy * i64::from(TILE_SIZE),
        )
    };
    let in_view = |tx: u32, ty: u32| -> bool {
        tx >= sx && tx < sx + VIEW_TILES_X && ty >= sy && ty < sy + VIEW_TILES_Y
    };
    // 行動済みグレーアウト (off_map は除外)
    for u in &database.unit_instances {
        if u.off_map || !u.has_acted || !in_view(u.x, u.y) {
            continue;
        }
        let (px, py) = to_screen(u.x, u.y);
        ctx.set_fill_style_str("rgba(0,0,0,0.45)");
        ctx.fill_rect(
            px as f64,
            py as f64,
            f64::from(TILE_SIZE),
            f64::from(TILE_SIZE),
        );
    }

    use src_core::ActionMode;
    match action_mode {
        ActionMode::Browse | ActionMode::PostMoveMenu { .. } => {}
        ActionMode::MoveSelect { uid } => {
            // 移動範囲ハイライト。移動判定 (try_move_unit_to) と同一の
            // GameDatabase::unit_move_range を使い、表示と実移動範囲の食い違いを防ぐ。
            for (rx, ry) in database.unit_move_range(&uid).into_keys() {
                if !in_view(rx, ry) {
                    continue;
                }
                let (px, py) = to_screen(rx, ry);
                ctx.set_fill_style_str("rgba(33,150,243,0.40)");
                ctx.fill_rect(
                    px as f64,
                    py as f64,
                    f64::from(TILE_SIZE),
                    f64::from(TILE_SIZE),
                );
            }
        }
        ActionMode::AttackSelect { uid, .. } => {
            // 武器射程範囲を簡易ハイライト（uid で実体解決、任意武器の min/max マンハッタン）
            if let Some(u) = database.unit_by_uid(&uid) {
                let unit_pos = (u.x, u.y);
                if let Some(def) = database.unit_by_name(&u.unit_data_name) {
                    let max_range = def.weapons.iter().map(|w| w.max_range).max().unwrap_or(0);
                    for dy in -max_range..=max_range {
                        for dx in -max_range..=max_range {
                            let d = dx.abs() + dy.abs();
                            if d == 0 || d > max_range {
                                continue;
                            }
                            // 少なくとも 1 武器の射程内？
                            let any = def
                                .weapons
                                .iter()
                                .any(|w| d >= w.min_range && d <= w.max_range);
                            if !any {
                                continue;
                            }
                            let nx = unit_pos.0 as i32 + dx;
                            let ny = unit_pos.1 as i32 + dy;
                            if nx < 0 || ny < 0 {
                                continue;
                            }
                            let (rx, ry) = (nx as u32, ny as u32);
                            if rx >= map.width || ry >= map.height || !in_view(rx, ry) {
                                continue;
                            }
                            let (px, py) = to_screen(rx, ry);
                            ctx.set_fill_style_str("rgba(229,57,53,0.40)");
                            ctx.fill_rect(
                                px as f64,
                                py as f64,
                                f64::from(TILE_SIZE),
                                f64::from(TILE_SIZE),
                            );
                        }
                    }
                }
            }
        }
        ActionMode::SpiritTarget {
            caster,
            target_enemy,
            ..
        } => {
            // 精神コマンドの対象候補をハイライト (味方=緑 / 敵=赤)。
            if let Some(cp) = database.unit_by_uid(&caster).map(|u| u.party) {
                let color = if target_enemy {
                    "rgba(229,57,53,0.40)"
                } else {
                    "rgba(76,175,80,0.45)"
                };
                for u in &database.unit_instances {
                    let valid = if target_enemy {
                        cp.is_hostile_to(u.party)
                    } else {
                        cp.is_ally_of(u.party)
                    };
                    if !valid || !in_view(u.x, u.y) {
                        continue;
                    }
                    let (px, py) = to_screen(u.x, u.y);
                    ctx.set_fill_style_str(color);
                    ctx.fill_rect(
                        px as f64,
                        py as f64,
                        f64::from(TILE_SIZE),
                        f64::from(TILE_SIZE),
                    );
                }
            }
        }
        ActionMode::SupportTarget { caster, .. } => {
            // 修理 / 補給 の対象候補 (隣接する味方ユニット) を緑でハイライト。
            if let Some(c) = database.unit_by_uid(&caster) {
                let cparty = c.party;
                let (cx, cy) = (c.x as i32, c.y as i32);
                for u in &database.unit_instances {
                    if u.off_map || u.uid == caster {
                        continue;
                    }
                    let d = (u.x as i32 - cx).abs() + (u.y as i32 - cy).abs();
                    if d != 1 || !cparty.is_ally_of(u.party) || !in_view(u.x, u.y) {
                        continue;
                    }
                    let (px, py) = to_screen(u.x, u.y);
                    ctx.set_fill_style_str("rgba(76,175,80,0.45)");
                    ctx.fill_rect(
                        px as f64,
                        py as f64,
                        f64::from(TILE_SIZE),
                        f64::from(TILE_SIZE),
                    );
                }
            }
        }
        ActionMode::AbilityTarget {
            caster,
            ability_idx,
        } => {
            // アビリティの対象候補 (射程内の味方ユニット) を緑でハイライト。
            if let Some(c) = database.unit_by_uid(&caster) {
                let range = database
                    .unit_by_name(&c.unit_data_name)
                    .and_then(|d| d.abilities.get(ability_idx))
                    .map(|a| a.range)
                    .unwrap_or(0);
                let cparty = c.party;
                let (cx, cy) = (c.x as i32, c.y as i32);
                for u in &database.unit_instances {
                    if u.off_map {
                        continue;
                    }
                    let d = (u.x as i32 - cx).abs() + (u.y as i32 - cy).abs();
                    if d > range || !cparty.is_ally_of(u.party) || !in_view(u.x, u.y) {
                        continue;
                    }
                    let (px, py) = to_screen(u.x, u.y);
                    ctx.set_fill_style_str("rgba(76,175,80,0.45)");
                    ctx.fill_rect(
                        px as f64,
                        py as f64,
                        f64::from(TILE_SIZE),
                        f64::from(TILE_SIZE),
                    );
                }
            }
        }
        ActionMode::LandingSelect { candidates, .. } => {
            // 発進/分離ユニットの着地候補マスを黄でハイライト (移動範囲内の空きマス)。
            for (rx, ry) in candidates {
                if !in_view(rx, ry) {
                    continue;
                }
                let (px, py) = to_screen(rx, ry);
                ctx.set_fill_style_str("rgba(253,216,53,0.45)");
                ctx.fill_rect(
                    px as f64,
                    py as f64,
                    f64::from(TILE_SIZE),
                    f64::from(TILE_SIZE),
                );
            }
        }
    }
}

/// コマンドメニューを画面左上の決まった位置に描画。
fn draw_command_menu(ctx: &CanvasRenderingContext2d, menu: &src_core::CommandMenu) {
    use src_core::command_menu::{MENU_ITEM_HEIGHT, MENU_PADDING, MENU_WIDTH, MENU_X, MENU_Y};
    let (labels, title): (Vec<String>, &str) = match menu {
        src_core::CommandMenu::Unit { items, .. } => (
            items.iter().map(|a| a.label().to_string()).collect(),
            "ユニット",
        ),
        src_core::CommandMenu::Map { items, .. } => (
            items.iter().map(|a| a.label().to_string()).collect(),
            "マップ",
        ),
    };
    let n = labels.len() as i32;
    let height = MENU_ITEM_HEIGHT * n + MENU_PADDING * 2;
    let x = MENU_X as f64;
    let y = MENU_Y as f64;
    let w = MENU_WIDTH as f64;
    let h = height as f64;
    // 枠
    ctx.set_fill_style_str("rgba(20,24,40,0.95)");
    ctx.fill_rect(x, y, w, h);
    ctx.set_stroke_style_str("#f0e68c");
    ctx.set_line_width(2.0);
    ctx.stroke_rect(x, y, w, h);
    // タイトル（小さく）
    ctx.set_fill_style_str("#9aa0b4");
    ctx.set_text_align("right");
    ctx.set_text_baseline("top");
    ctx.set_font(&format!("11px {JP_SANS}"));
    let _ = ctx.fill_text(title, x + w - 6.0, y + 2.0);
    // 項目列
    ctx.set_text_align("left");
    ctx.set_font(&format!("bold 14px {JP_SANS}"));
    ctx.set_fill_style_str("#e6e6f0");
    for (i, label) in labels.iter().enumerate() {
        let iy = y + (MENU_PADDING + MENU_ITEM_HEIGHT * i as i32) as f64 + 4.0;
        let _ = ctx.fill_text(label, x + 12.0, iy);
    }
}

/// `.eve` の `PaintString` / `Line` / `Font` 等で蓄積された
/// `ScriptOverlay` を Canvas に流し込む。
fn draw_script_overlay(
    ctx: &CanvasRenderingContext2d,
    overlay: &src_core::ScriptOverlay,
    assets: &Assets,
) {
    use src_core::DrawCmd as D;
    let mut font_str = format!("14px {JP_SANS}");
    let mut text_color = "#ffffff".to_string();
    let mut stroke_color = "#ffffff".to_string();
    for cmd in &overlay.cmds {
        match cmd {
            D::SetFont {
                family,
                size_pt,
                color,
            } => {
                let family = if family.is_empty() {
                    JP_SANS.to_string()
                } else {
                    format!("'{family}', {JP_SANS}")
                };
                font_str = format!("{size_pt}px {family}");
                text_color = color.clone();
            }
            D::SetColor { color } => {
                stroke_color = color.clone();
            }
            D::PaintString { x, y, text } => {
                ctx.set_font(&font_str);
                ctx.set_text_align("left");
                ctx.set_text_baseline("top");
                ctx.set_fill_style_str(&text_color);
                let _ = ctx.fill_text(text, *x, *y);
            }
            D::Line { x1, y1, x2, y2 } => {
                ctx.set_stroke_style_str(&stroke_color);
                ctx.set_line_width(1.0);
                ctx.begin_path();
                ctx.move_to(*x1, *y1);
                ctx.line_to(*x2, *y2);
                ctx.stroke();
            }
            D::PSet { x, y } => {
                ctx.set_fill_style_str(&stroke_color);
                ctx.fill_rect(*x, *y, 1.0, 1.0);
            }
            D::FillRect { x, y, w, h } => {
                ctx.set_fill_style_str(&stroke_color);
                ctx.fill_rect(*x, *y, *w, *h);
            }
            D::Fade { color, alpha } => {
                let prev_alpha = ctx.global_alpha();
                ctx.set_global_alpha(*alpha);
                ctx.set_fill_style_str(color);
                ctx.fill_rect(0.0, 0.0, f64::from(CANVAS_WIDTH), f64::from(CANVAS_HEIGHT));
                ctx.set_global_alpha(prev_alpha);
            }
            D::SetLineWidth(n) => {
                ctx.set_line_width(*n);
            }
            D::Picture {
                path,
                x,
                y,
                w,
                h,
                transparent,
                flip_x,
                flip_y,
                monochrome,
                sepia,
                half_mode,
                rotation_deg,
                as_background: _,
                persist: _,
                center_x,
                center_y,
            } => {
                // 座標 `-`（中央寄せ）は画像の実寸が必要なため、ここで確定する。
                // src-core 側は実寸を知らないため center フラグだけ立てて委譲される。
                // 中心は SRC レイアウト基準の 240（幅明示時も `240 - 実寸/2` で一致）。
                const OVERLAY_CENTER: f64 = 240.0;
                let centered = |coord: f64, size: f64, center: bool| -> f64 {
                    if center {
                        OVERLAY_CENTER - size / 2.0
                    } else {
                        coord
                    }
                };
                // assets.find_image は basename / stem いずれでもヒットする。
                // basename を取り出して試す。
                let key = image_lookup_key(path);
                // `透過` 指定時はカラーキー透明化した canvas を優先して描く。
                // 画像が非同期デコード未完了のうちは None が返るので通常画像に
                // フォールバックし、次フレーム以降で透明版へ切り替わる。
                let transparent_canvas = if *transparent {
                    assets.transparent_image(&key)
                } else {
                    None
                };
                // CSS filter (`monochrome` / `sepia`) を Canvas2D の filter プロパティで適用。
                // 描画完了後に "none" に戻す。
                let filter = build_css_filter(*monochrome, *sepia);
                if let Some(f) = &filter {
                    ctx.set_filter(f);
                }
                if let Some(canvas) = transparent_canvas {
                    let iw = w.unwrap_or(f64::from(canvas.width()));
                    let ih = h.unwrap_or(f64::from(canvas.height()));
                    let px = centered(*x, iw, *center_x);
                    let py = centered(*y, ih, *center_y);
                    draw_overlay_picture(
                        ctx,
                        px,
                        py,
                        iw,
                        ih,
                        *flip_x,
                        *flip_y,
                        *rotation_deg,
                        |dx, dy, dw, dh| {
                            ctx.draw_image_with_html_canvas_element_and_dw_and_dh(
                                &canvas, dx, dy, dw, dh,
                            )
                        },
                    );
                } else if let Some(img) = assets.find_image(&key) {
                    let iw = w.unwrap_or(f64::from(img.natural_width()));
                    let ih = h.unwrap_or(f64::from(img.natural_height()));
                    let px = centered(*x, iw, *center_x);
                    let py = centered(*y, ih, *center_y);
                    draw_overlay_picture(
                        ctx,
                        px,
                        py,
                        iw,
                        ih,
                        *flip_x,
                        *flip_y,
                        *rotation_deg,
                        |dx, dy, dw, dh| {
                            ctx.draw_image_with_html_image_element_and_dw_and_dh(
                                img, dx, dy, dw, dh,
                            )
                        },
                    );
                }
                if filter.is_some() {
                    ctx.set_filter("none");
                }
                // 半分マスク: 反対側の半分を背景色で塗りつぶす (SRC `上半分`/`下半分`/...)
                if !half_mode.is_empty() {
                    let iw = w.unwrap_or(32.0);
                    let ih = h.unwrap_or(32.0);
                    let px = centered(*x, iw, *center_x);
                    let py = centered(*y, ih, *center_y);
                    apply_half_mask(ctx, px, py, iw, ih, half_mode);
                }
            }
        }
    }
}

/// `PaintPicture` の 1 枚を描く。
/// `flip_x` / `flip_y` で左右 / 上下反転、`rotation_deg` で度数法回転を適用。
/// 反転と回転の組み合わせは: 画像中心を原点として回転 → flip → draw の順。
#[allow(clippy::too_many_arguments)]
fn draw_overlay_picture(
    ctx: &CanvasRenderingContext2d,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    flip_x: bool,
    flip_y: bool,
    rotation_deg: f64,
    draw: impl Fn(f64, f64, f64, f64) -> Result<(), wasm_bindgen::JsValue>,
) {
    if !flip_x && !flip_y && rotation_deg == 0.0 {
        let _ = draw(x, y, w, h);
        return;
    }
    ctx.save();
    // 画像中央に原点を移動 → 回転 → flip → 中央オフセットを戻す
    let cx = x + w / 2.0;
    let cy = y + h / 2.0;
    ctx.translate(cx, cy).ok();
    if rotation_deg != 0.0 {
        ctx.rotate(rotation_deg.to_radians()).ok();
    }
    if flip_x || flip_y {
        let sx = if flip_x { -1.0 } else { 1.0 };
        let sy = if flip_y { -1.0 } else { 1.0 };
        ctx.scale(sx, sy).ok();
    }
    let _ = draw(-w / 2.0, -h / 2.0, w, h);
    ctx.restore();
}

/// SRC `白黒` / `セピア` オプションを Canvas2D filter CSS 文字列に変換。
/// いずれも未指定なら `None`。両方指定された場合は両方をスペース連結。
fn build_css_filter(monochrome: bool, sepia: bool) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    if monochrome {
        parts.push("grayscale(1)");
    }
    if sepia {
        parts.push("sepia(1)");
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" "))
    }
}

/// SRC `上半分`/`下半分`/`左半分`/`右半分`/`右上`/`左上`/`右下`/`左下` の
/// 「反対側半分を背景色で塗りつぶす」マスクを後付けで描画する。
/// 元 SRC は画像処理段階で行うが、本実装は Canvas2D の rect/fillStyle で
/// オーバーペイントして近似 (ダーク背景色で覆い隠す)。
fn apply_half_mask(ctx: &CanvasRenderingContext2d, x: f64, y: f64, w: f64, h: f64, mode: &str) {
    // 背景色: 黒 (フィルタ的扱い)。シナリオ側で背景色を指定する仕様も
    // 存在するが、フロントエンド設定は別途で簡略化。
    ctx.set_fill_style_str("#000000");
    let half_w = w / 2.0;
    let half_h = h / 2.0;
    match mode {
        "上半分" => {
            // 下半分を塗りつぶす
            ctx.fill_rect(x, y + half_h, w, half_h);
        }
        "下半分" => {
            ctx.fill_rect(x, y, w, half_h);
        }
        "左半分" => {
            ctx.fill_rect(x + half_w, y, half_w, h);
        }
        "右半分" => {
            ctx.fill_rect(x, y, half_w, h);
        }
        // 対角線塗りつぶし: 三角形ポリゴンで近似
        "右上" => {
            ctx.begin_path();
            ctx.move_to(x, y + h);
            ctx.line_to(x + w, y + h);
            ctx.line_to(x, y);
            ctx.close_path();
            ctx.fill();
        }
        "左上" => {
            ctx.begin_path();
            ctx.move_to(x + w, y);
            ctx.line_to(x + w, y + h);
            ctx.line_to(x, y + h);
            ctx.close_path();
            ctx.fill();
        }
        "右下" => {
            ctx.begin_path();
            ctx.move_to(x, y);
            ctx.line_to(x + w, y);
            ctx.line_to(x, y + h);
            ctx.close_path();
            ctx.fill();
        }
        "左下" => {
            ctx.begin_path();
            ctx.move_to(x, y);
            ctx.line_to(x + w, y);
            ctx.line_to(x + w, y + h);
            ctx.close_path();
            ctx.fill();
        }
        _ => {}
    }
}

/// `Bitmap\Unit\foo.bmp` 等の Windows パスから basename `foo.bmp` を抽出。
fn image_lookup_key(path: &str) -> String {
    let normalized = path.replace('\\', "/");
    normalized
        .rsplit('/')
        .next()
        .unwrap_or(&normalized)
        .to_string()
}

/// 対話 UI (`Talk` / `Confirm`) を画面下部のウィンドウ枠で描画。
/// Talk: 話者ラベル + 本文 (継続ヒントは出さない — 原典 SRC.NET と揃える)
/// Confirm: 質問文 + 「Y はい / N いいえ」
/// `WaitClick`: 何も描画しない (原典 `Wait Click` は無音待ち)。
fn draw_dialog_overlay(
    ctx: &CanvasRenderingContext2d,
    d: &src_core::PendingDialog,
    assets: &Assets,
    database: &GameDatabase,
) {
    use src_core::PendingDialog as D;
    // `Wait Click` の合成ダイアログ: 原典 SRC は何も描画しない。
    // 直前の PaintString / PaintPicture をそのまま見せる。
    if matches!(d, D::WaitClick) {
        return;
    }
    let cw = f64::from(CANVAS_WIDTH);
    let ch = f64::from(CANVAS_HEIGHT);

    // ウィンドウ枠: 画面下 40% を覆う
    let win_top = ch * 0.55;
    let win_h = ch - win_top - 6.0;
    let pad = 12.0;

    // 半透明背景
    ctx.set_fill_style_str("rgba(0,0,0,0.30)");
    ctx.fill_rect(0.0, 0.0, cw, ch);
    // 枠
    ctx.set_fill_style_str("rgba(20,24,40,0.92)");
    ctx.fill_rect(6.0, win_top, cw - 12.0, win_h);
    ctx.set_stroke_style_str("#f0e68c");
    ctx.set_line_width(2.0);
    ctx.stroke_rect(6.0, win_top, cw - 12.0, win_h);

    match d {
        D::Talk { speaker, body } => {
            // 顔グラ枠（パイロット名で検索した bitmap があれば描画）
            let face_x = pad + 6.0;
            let face_y = win_top + pad;
            let face_size = 96.0;
            ctx.set_stroke_style_str("#a8b8d0");
            ctx.set_line_width(1.0);
            ctx.stroke_rect(face_x, face_y, face_size, face_size);
            // パイロット bitmap (登録があれば)
            if let Some(p) = database.pilot_by_name(speaker) {
                if let Some(b) = p.bitmap.as_deref() {
                    if let Some(img) = assets.find_image(b) {
                        let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
                            img, face_x, face_y, face_size, face_size,
                        );
                    }
                }
            }
            // 話者名
            ctx.set_fill_style_str("#fff");
            ctx.set_text_align("left");
            ctx.set_text_baseline("top");
            ctx.set_font(&format!("bold 16px {JP_SANS}"));
            let text_x = face_x + face_size + pad;
            let text_y = face_y;
            if !speaker.is_empty() {
                let _ = ctx.fill_text(speaker, text_x, text_y);
            }
            // 本文（行毎に描画）
            ctx.set_font(&format!("15px {JP_SANS}"));
            ctx.set_fill_style_str("#e6e6f0");
            let max_chars = 32; // 1 行あたりの大体の上限
            let mut y = text_y + 24.0;
            for line in body.lines().flat_map(|l| wrap_text(l, max_chars)) {
                let _ = ctx.fill_text(&line, text_x, y);
                y += 20.0;
                if y > win_top + win_h - 28.0 {
                    break;
                }
            }
            // 継続プロンプト (`▶ Enter で進む`) は原典 SRC.NET の Talk
            // ウィンドウには存在しない。クリック / Enter で進行する操作は
            // メッセージウィンドウが表示されている事実そのものから自明なので
            // ヒントを表示せず原典に合わせる。
        }
        D::Confirm { question, .. } => {
            // 中央配置の質問 + Y/N
            ctx.set_fill_style_str("#e0e0ff");
            ctx.set_text_align("center");
            ctx.set_text_baseline("top");
            ctx.set_font(&format!("bold 18px {JP_SANS}"));
            let mut y = win_top + pad + 6.0;
            for line in wrap_text(question, 28).into_iter().take(3) {
                let _ = ctx.fill_text(&line, cw / 2.0, y);
                y += 26.0;
            }
            ctx.set_font(&format!("bold 16px {JP_SANS}"));
            ctx.set_fill_style_str("#a5d6a7");
            let _ = ctx.fill_text("[ Y ] はい", cw / 2.0 - 80.0, win_top + win_h - 40.0);
            ctx.set_fill_style_str("#ef9a9a");
            let _ = ctx.fill_text("[ N ] いいえ", cw / 2.0 + 80.0, win_top + win_h - 40.0);
        }
        D::Input { prompt, .. } => {
            // テキスト入力は HTML overlay 側で表示するため、Canvas には案内のみ。
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            ctx.set_fill_style_str("#e0e0ff");
            ctx.set_font(&format!("bold 18px {JP_SANS}"));
            let _ = ctx.fill_text(prompt, cw / 2.0, win_top + 32.0);
            ctx.set_font(&format!("13px {JP_SANS}"));
            ctx.set_fill_style_str("#9aa0b4");
            let _ = ctx.fill_text(
                "▶ 上のフォームに入力して Enter / Esc でキャンセル",
                cw / 2.0,
                win_top + 60.0,
            );
        }
        D::WaitClick => {
            // 早期 return 済みだが、コンパイラの網羅性チェック対策。
        }
        D::Menu {
            prompt,
            options,
            store_value: _,
            non_cancellable,
            ..
        } => {
            // 上段: prompt、下段: 番号付き選択肢列
            ctx.set_text_align("left");
            ctx.set_text_baseline("top");
            ctx.set_fill_style_str("#e0e0ff");
            ctx.set_font(&format!("bold 17px {JP_SANS}"));
            let mut y = win_top + pad;
            for line in wrap_text(prompt, 36).into_iter().take(2) {
                let _ = ctx.fill_text(&line, pad + 8.0, y);
                y += 24.0;
            }
            y += 4.0;
            ctx.set_font(&format!("15px {JP_SANS}"));
            // 最大 9 件まで表示（1-9 キー対応）
            for (i, opt) in options.iter().take(9).enumerate() {
                let n = i + 1;
                ctx.set_fill_style_str("#fff176");
                let _ = ctx.fill_text(&format!("[{n}]"), pad + 8.0, y);
                ctx.set_fill_style_str("#e6e6f0");
                let _ = ctx.fill_text(opt, pad + 36.0, y);
                y += 20.0;
                if y > win_top + win_h - 24.0 {
                    break;
                }
            }
            // 案内: キャンセル可なら「Esc キャンセル」、選択必須なら「番号で選択」。
            ctx.set_font(&format!("bold 12px {JP_SANS}"));
            ctx.set_fill_style_str("#9aa0b4");
            ctx.set_text_align("right");
            let hint = if *non_cancellable {
                "番号キー / クリックで選択"
            } else {
                "Esc キャンセル"
            };
            let _ = ctx.fill_text(hint, cw - 16.0, win_top + win_h - 18.0);
        }
    }
}

fn clear(ctx: &CanvasRenderingContext2d, color: &str) {
    ctx.set_fill_style_str(color);
    ctx.fill_rect(0.0, 0.0, f64::from(CANVAS_WIDTH), f64::from(CANVAS_HEIGHT));
}

// ===== Map view scene =====

/// ネイティブ戦闘演出を 1 フレーム描画する。攻撃側→防御側タイルへの弾道/斬撃、
/// 着弾フラッシュ、ダメージ数字（またはミス表示）を `anim.progress()` に従って
/// 重ねる。SRC_BA の `.eve` は使わずエンジン側のみで完結する最小演出。
fn draw_battle_anim(
    ctx: &CanvasRenderingContext2d,
    anim: &src_core::BattleAnim,
    scroll: (u32, u32),
    assets: &Assets,
) {
    use src_core::AttackKind;
    let (sx, sy) = scroll;
    let ox = (i64::from(CANVAS_WIDTH) - i64::from(MAP_VIEW_WIDTH)) / 2;
    let oy = (i64::from(CANVAS_HEIGHT) - i64::from(MAP_VIEW_HEIGHT)) / 2;
    let ts = f64::from(TILE_SIZE);
    // タイル中心のスクリーン座標。
    let center = |tx: u32, ty: u32| -> (f64, f64) {
        let dx = (i64::from(tx) - i64::from(sx)) as f64;
        let dy = (i64::from(ty) - i64::from(sy)) as f64;
        (
            ox as f64 + dx * ts + ts / 2.0,
            oy as f64 + dy * ts + ts / 2.0,
        )
    };
    let p = anim.progress();
    let (dcx, dcy) = center(anim.defender.0, anim.defender.1);
    let (acx, acy) = center(anim.attacker.0, anim.attacker.1);

    // 演出種別ごとの基調色 (R,G,B)。
    let (kr, kg, kb) = match anim.kind {
        AttackKind::Beam => (120u8, 220u8, 255u8),
        AttackKind::Shoot => (255, 220, 120),
        AttackKind::Melee => (255, 255, 255),
        AttackKind::Generic => (255, 200, 200),
    };

    if anim.hit {
        // 1) 弾道 / 斬撃: 攻撃側→防御側のラインを前半で伸ばす (ビームは持続)。
        let travel = (p / 0.45).clamp(0.0, 1.0);
        let beam = anim.kind == AttackKind::Beam;
        if travel < 1.0 || beam {
            let (ex, ey) = if beam {
                (dcx, dcy)
            } else {
                (acx + (dcx - acx) * travel, acy + (dcy - acy) * travel)
            };
            let line_alpha = if beam { (1.0 - p) * 0.8 } else { 0.7 };
            ctx.set_stroke_style_str(&format!("rgba({kr},{kg},{kb},{line_alpha:.3})"));
            ctx.set_line_width(if anim.kind == AttackKind::Melee {
                2.0
            } else {
                4.0
            });
            ctx.begin_path();
            ctx.move_to(acx, acy);
            ctx.line_to(ex, ey);
            ctx.stroke();
        }
        // 2) 着弾エフェクト: SRC_BA のフレームスプライト (色キー透過) を優先し、
        //    読み込まれていない/未デコードならジオメトリックなフラッシュにフォール
        //    バックする。フレームは impact 進捗 (p=0.25 以降) に沿って 1 枚ずつ進む。
        let mut sprite_drawn = false;
        if p >= 0.25 {
            let impact = ((p - 0.25) / 0.75).clamp(0.0, 1.0);
            // 種別ごとのフレームセット (basename stem prefix, 枚数)。
            let (prefix, count) = match anim.kind {
                AttackKind::Beam => ("effect_burn(lightblue)", 11usize),
                AttackKind::Shoot => ("effect_burn(red)", 11),
                AttackKind::Melee => ("effect_flair", 8),
                AttackKind::Generic => ("effect_burn(white)", 11),
            };
            let frame = ((impact * count as f64) as usize).min(count - 1) + 1;
            let key = format!("{prefix}{frame:02}");
            if let Some(canvas) = assets.transparent_image(&key) {
                let size = ts * 1.8;
                // 終盤 (impact>0.8) でフェードアウト。
                let fade = if impact > 0.8 {
                    ((1.0 - impact) / 0.2).clamp(0.0, 1.0)
                } else {
                    1.0
                };
                let prev = ctx.global_alpha();
                ctx.set_global_alpha((prev * fade).clamp(0.0, 1.0));
                let _ = ctx.draw_image_with_html_canvas_element_and_dw_and_dh(
                    &canvas,
                    dcx - size / 2.0,
                    dcy - size / 2.0,
                    size,
                    size,
                );
                ctx.set_global_alpha(prev);
                sprite_drawn = true;
            }
        }
        // フォールバック: 防御側に拡大しながら消えるリング + 白コア。
        if !sprite_drawn && p >= 0.25 {
            let impact = ((p - 0.3) / 0.7).clamp(0.0, 1.0);
            let radius = ts * (0.25 + impact * 0.55);
            let flash_alpha = (1.0 - impact) * 0.85;
            ctx.set_fill_style_str(&format!("rgba({kr},{kg},{kb},{flash_alpha:.3})"));
            ctx.begin_path();
            let _ = ctx.arc(dcx, dcy, radius, 0.0, std::f64::consts::TAU);
            ctx.fill();
            let core_alpha = (1.0 - impact) * 0.9;
            ctx.set_fill_style_str(&format!("rgba(255,255,255,{core_alpha:.3})"));
            ctx.begin_path();
            let _ = ctx.arc(dcx, dcy, radius * 0.4, 0.0, std::f64::consts::TAU);
            ctx.fill();
        }
        // 3) ダメージ数字: 着弾後に上昇しながらフェード。
        if p >= 0.3 && anim.damage > 0 {
            let rise = (p - 0.3) / 0.7;
            let ny = dcy - ts * 0.3 - rise * ts * 0.7;
            let num_alpha = (1.0 - rise).clamp(0.0, 1.0);
            let text = anim.damage.to_string();
            ctx.set_font(&format!("bold 22px {JP_SANS}"));
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            ctx.set_fill_style_str(&format!("rgba(0,0,0,{:.3})", num_alpha * 0.8));
            let _ = ctx.fill_text(&text, dcx + 1.5, ny + 1.5);
            ctx.set_fill_style_str(&format!("rgba(255,90,70,{num_alpha:.3})"));
            let _ = ctx.fill_text(&text, dcx, ny);
        }
        // 4) 撃破表示。
        if anim.killed && p >= 0.45 {
            let ka = ((p - 0.45) / 0.55).clamp(0.0, 1.0);
            let alpha = (1.0 - (ka - 0.5).abs() * 2.0).clamp(0.0, 1.0);
            ctx.set_font(&format!("bold 16px {JP_SANS}"));
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            ctx.set_fill_style_str(&format!("rgba(255,210,80,{alpha:.3})"));
            let _ = ctx.fill_text("撃破", dcx, dcy - ts * 1.1);
        }
    } else {
        // ミス: "MISS" が上昇しながらフェード。
        let ny = dcy - ts * 0.3 - p * ts * 0.6;
        let alpha = (1.0 - p).clamp(0.0, 1.0);
        ctx.set_font(&format!("bold 18px {JP_SANS}"));
        ctx.set_text_align("center");
        ctx.set_text_baseline("middle");
        ctx.set_fill_style_str(&format!("rgba(0,0,0,{:.3})", alpha * 0.7));
        let _ = ctx.fill_text("MISS", dcx + 1.0, ny + 1.0);
        ctx.set_fill_style_str(&format!("rgba(220,235,255,{alpha:.3})"));
        let _ = ctx.fill_text("MISS", dcx, ny);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_map_view(
    ctx: &CanvasRenderingContext2d,
    database: &GameDatabase,
    cursor: Option<(u32, u32)>,
    turn: Turn,
    scroll: (u32, u32),
    stage: &str,
    last_message: Option<&str>,
    assets: &Assets,
    selected_weapon_idx: usize,
    messages_total: usize,
    script_active: bool,
    action_mode: src_core::ActionMode,
    battle_anim: Option<&src_core::BattleAnim>,
    move_anim: Option<&src_core::MoveAnim>,
) {
    let ox = (i64::from(CANVAS_WIDTH) - i64::from(MAP_VIEW_WIDTH)) / 2;
    let oy = (i64::from(CANVAS_HEIGHT) - i64::from(MAP_VIEW_HEIGHT)) / 2;
    let (sx, sy) = scroll;
    // タイル座標 → スクリーン x/y
    let to_screen = |tx: u32, ty: u32| -> (i64, i64) {
        let dx = i64::from(tx) - i64::from(sx);
        let dy = i64::from(ty) - i64::from(sy);
        (
            ox + dx * i64::from(TILE_SIZE),
            oy + dy * i64::from(TILE_SIZE),
        )
    };
    let in_view = |tx: u32, ty: u32| -> bool {
        tx >= sx && tx < sx + VIEW_TILES_X && ty >= sy && ty < sy + VIEW_TILES_Y
    };

    // タイル描画開始 Y (= MapView 内マップ領域の左上)
    let grid_top = oy;

    if let Some(map) = database.map.as_ref() {
        // 可視範囲のみ描画
        let y_end = (sy + VIEW_TILES_Y).min(map.height);
        let x_end = (sx + VIEW_TILES_X).min(map.width);
        for y in sy..y_end {
            for x in sx..x_end {
                let cell = map.cell(x, y);
                let (px, py) = to_screen(x, y);
                let (_, hints, color, glyph) = database.terrain_display(cell.terrain_id);
                let tile_img = hints.iter().find_map(|h| assets.find_image(h));
                if let Some(img) = tile_img {
                    let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
                        img,
                        px as f64,
                        py as f64,
                        f64::from(TILE_SIZE),
                        f64::from(TILE_SIZE),
                    );
                } else {
                    ctx.set_fill_style_str(color);
                    ctx.fill_rect(
                        px as f64,
                        py as f64,
                        f64::from(TILE_SIZE),
                        f64::from(TILE_SIZE),
                    );
                    ctx.set_stroke_style_str("rgba(0,0,0,0.15)");
                    ctx.set_line_width(1.0);
                    ctx.stroke_rect(
                        px as f64 + 0.5,
                        py as f64 + 0.5,
                        f64::from(TILE_SIZE) - 1.0,
                        f64::from(TILE_SIZE) - 1.0,
                    );
                    if !glyph.is_empty() {
                        ctx.set_fill_style_str("rgba(0,0,0,0.55)");
                        ctx.set_font(&format!("bold 14px {JP_SANS}"));
                        ctx.set_text_align("center");
                        ctx.set_text_baseline("middle");
                        let _ = ctx.fill_text(
                            glyph,
                            px as f64 + f64::from(TILE_SIZE) / 2.0,
                            py as f64 + f64::from(TILE_SIZE) / 2.0,
                        );
                    }
                }
            }
        }

        // カーソル位置のユニット → 移動範囲オーバーレイ（可視範囲のみ）。
        // 移動判定 (try_move_unit_to) と同一の GameDatabase::unit_move_range を使い、
        // 表示と実移動範囲の食い違い (「形状が変」) を防ぐ。
        // MoveSelect/AttackSelect 中は draw_action_overlays が専用範囲を描くため、
        // Browse 時のみ表示し、二重描画による形状崩れを避ける。
        if let (Some((cx, cy)), src_core::ActionMode::Browse) = (cursor, &action_mode) {
            let unit = database
                .units_at(cx, cy)
                .next()
                .map(|u| (u.uid.clone(), u.party));
            if let Some((uid, party)) = unit {
                let overlay = match party {
                    src_core::Party::Player => "rgba(33,150,243,0.35)",
                    src_core::Party::Npc => "rgba(76,175,80,0.35)",
                    src_core::Party::Enemy => "rgba(229,57,53,0.35)",
                    src_core::Party::Neutral => "rgba(253,216,53,0.35)",
                };
                for (rx, ry) in database.unit_move_range(&uid).into_keys() {
                    if (rx, ry) == (cx, cy) || !in_view(rx, ry) {
                        continue;
                    }
                    let (px, py) = to_screen(rx, ry);
                    ctx.set_fill_style_str(overlay);
                    ctx.fill_rect(
                        px as f64,
                        py as f64,
                        f64::from(TILE_SIZE),
                        f64::from(TILE_SIZE),
                    );
                }
            }
        }

        // ユニットチップ（可視範囲のみ）。Escape で退避中のユニットは描画しない。
        for u in &database.unit_instances {
            if u.off_map || u.x >= map.width || u.y >= map.height || !in_view(u.x, u.y) {
                continue;
            }
            let (mut px, mut py) = to_screen(u.x, u.y);
            // 移動スライド演出中の当該ユニットは、経路に沿った補間位置で描く。
            let sliding = move_anim.is_some_and(|m| m.uid == u.uid);
            if let Some(m) = move_anim {
                if m.uid == u.uid {
                    let (fx, fy) = m.position();
                    px = ox + ((fx - f64::from(sx)) * f64::from(TILE_SIZE)).round() as i64;
                    py = oy + ((fy - f64::from(sy)) * f64::from(TILE_SIZE)).round() as i64;
                }
            }
            // 戦闘演出中、攻撃側チップを対象方向へ素早く突き出す (lunge)。格闘は前方へ、
            // 射撃/ビームはわずかに後方へ (リコイル)。演出前半 (進捗 0..0.4) のみ。
            // スライド中のユニットには lunge を適用しない。
            if let Some(anim) = battle_anim.filter(|_| !sliding) {
                if (u.x, u.y) == anim.attacker && anim.attacker != anim.defender {
                    let window = (anim.progress() / 0.4).clamp(0.0, 1.0);
                    if window < 1.0 {
                        let amt = (window * std::f64::consts::PI).sin();
                        let dx = f64::from(anim.defender.0) - f64::from(anim.attacker.0);
                        let dy = f64::from(anim.defender.1) - f64::from(anim.attacker.1);
                        let len = (dx * dx + dy * dy).sqrt().max(1.0);
                        let mag = match anim.kind {
                            src_core::AttackKind::Melee => 0.38 * f64::from(TILE_SIZE),
                            _ => -0.16 * f64::from(TILE_SIZE),
                        };
                        px += (dx / len * amt * mag).round() as i64;
                        py += (dy / len * amt * mag).round() as i64;
                    }
                }
            }
            // unit bitmap が assets に登録されていれば画像で描画、無ければチップ。
            let img = database
                .unit_by_name(&u.unit_data_name)
                .and_then(|d| {
                    if d.bitmap.is_empty() {
                        None
                    } else {
                        assets.find_image(&d.bitmap)
                    }
                })
                .or_else(|| assets.find_image(&u.unit_data_name));
            if let Some(img) = img {
                let pad = 2.0;
                let cell = f64::from(TILE_SIZE) - pad * 2.0;
                let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
                    img,
                    px as f64 + pad,
                    py as f64 + pad,
                    cell,
                    cell,
                );
                // 勢力色の細枠
                ctx.set_stroke_style_str(u.party.color());
                ctx.set_line_width(2.0);
                ctx.stroke_rect(px as f64 + pad, py as f64 + pad, cell, cell);
                // 勢力ラベル (右下に小さく)
                ctx.set_fill_style_str("rgba(0,0,0,0.7)");
                ctx.fill_rect(px as f64 + cell - 8.0, py as f64 + cell - 8.0, 12.0, 12.0);
                ctx.set_fill_style_str("#fff");
                ctx.set_font(&format!("bold 9px {JP_SANS}"));
                ctx.set_text_align("center");
                ctx.set_text_baseline("middle");
                let _ = ctx.fill_text(
                    u.party.short_label(),
                    px as f64 + cell - 2.0,
                    py as f64 + cell - 2.0,
                );
            } else {
                draw_unit_chip(ctx, px, py, u);
            }
        }

        // カーソル（可視範囲のみ）
        if let Some((cx, cy)) = cursor {
            if cx < map.width && cy < map.height && in_view(cx, cy) {
                let (px, py) = to_screen(cx, cy);
                ctx.set_stroke_style_str("#ffeb3b");
                ctx.set_line_width(3.0);
                ctx.stroke_rect(
                    px as f64 + 1.5,
                    py as f64 + 1.5,
                    f64::from(TILE_SIZE) - 3.0,
                    f64::from(TILE_SIZE) - 3.0,
                );
            }
        }

        // 右側ステータスパネル
        draw_status_panel(
            ctx,
            ox + i64::from(STATUS_PANEL_X),
            oy + i64::from(STATUS_PANEL_Y),
            database,
            cursor,
            turn,
            stage,
            (sx, sy),
            assets,
            selected_weapon_idx,
        );

        // 下部メッセージボックス（カーソル位置のパイロットの顔グラを表示）
        let face_hint = cursor
            .and_then(|(cx, cy)| database.units_at(cx, cy).next())
            .and_then(|u| database.pilot_by_name(&u.pilot_name))
            .map(|p| p.bitmap.clone().unwrap_or_else(|| p.nickname.clone()));
        draw_message_box(
            ctx,
            ox + i64::from(MESSAGE_BOX_X),
            oy + i64::from(MESSAGE_BOX_Y),
            last_message,
            face_hint.as_deref(),
            assets,
            messages_total,
        );
    } else {
        // マップ未ロード時は黒で塗っておく (後段の script_overlay の PaintPicture /
        // PaintString が黒地に乗る前提)。`script_active = false` のときだけ
        // 「マップが読み込まれていません」案内を中央に出す: 開発時の問題切り分け
        // に必要だが、`.eve` の opening 中はそのストーリー演出を遮るので抑制する。
        ctx.set_fill_style_str("#000");
        ctx.fill_rect(
            ox as f64,
            grid_top as f64,
            f64::from(MAP_VIEW_WIDTH),
            f64::from(MAP_VIEW_HEIGHT),
        );
        if !script_active {
            ctx.set_fill_style_str("#666");
            ctx.set_font(&format!("italic 12px {JP_SANS}"));
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            let _ = ctx.fill_text(
                "マップが読み込まれていません",
                ox as f64 + f64::from(MAP_VIEW_WIDTH) / 2.0,
                grid_top as f64 + f64::from(MAP_VIEW_HEIGHT) / 2.0,
            );
        }
    }
}

// ===== Title scene =====

/// タイトル画面描画 / Draw the title screen.
fn draw_title(ctx: &CanvasRenderingContext2d, assets: &Assets) {
    let layout = TitleLayout::original();
    let offset_x = (i64::from(CANVAS_WIDTH) - i64::from(TITLE_WIDTH)) / 2;
    let offset_y = (i64::from(CANVAS_HEIGHT) - i64::from(TITLE_HEIGHT)) / 2;

    // タイトル本体の背景 (VB6 BUTTONFACE 相当)
    ctx.set_fill_style_str("#c0c0c0");
    ctx.fill_rect(
        offset_x as f64,
        offset_y as f64,
        f64::from(TITLE_WIDTH),
        f64::from(TITLE_HEIGHT),
    );

    // Frame1
    draw_vb6_etched_frame(
        ctx,
        offset_x + i64::from(layout.frame.x),
        offset_y + i64::from(layout.frame.y),
        layout.frame.w,
        layout.frame.h,
    );

    // Image1 (Title.frx:0x268C, ICO)
    {
        let x = (offset_x + i64::from(layout.logo.x)) as f64;
        let y = (offset_y + i64::from(layout.logo.y)) as f64;
        let w = f64::from(layout.logo.w);
        let h = f64::from(layout.logo.h);
        if let Some(img) = assets.title_icon.as_ref() {
            let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(img, x, y, w, h);
        } else {
            draw_image_placeholder(
                ctx,
                offset_x + i64::from(layout.logo.x),
                offset_y + i64::from(layout.logo.y),
                layout.logo.w,
                layout.logo.h,
                "Image1",
            );
        }
    }

    // Picture1 ("SRC" バナー)
    {
        let x = (offset_x + i64::from(layout.title_picture.x)) as f64;
        let y = (offset_y + i64::from(layout.title_picture.y)) as f64;
        let w = f64::from(layout.title_picture.w);
        let h = f64::from(layout.title_picture.h);
        if let Some(img) = assets.title_logo.as_ref() {
            let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(img, x, y, w, h);
        } else {
            draw_title_logo_text(
                ctx,
                offset_x + i64::from(layout.title_picture.x),
                offset_y + i64::from(layout.title_picture.y),
                layout.title_picture.w,
                layout.title_picture.h,
            );
        }
    }

    // labVersion (Times New Roman 15.75pt, 右揃え)
    ctx.set_fill_style_str("#000");
    ctx.set_text_align("right");
    ctx.set_text_baseline("middle");
    ctx.set_font("italic 21px 'Times New Roman', serif");
    let _ = ctx.fill_text(
        &title::version_string(),
        (offset_x + i64::from(layout.version_label.x) + i64::from(layout.version_label.w)) as f64,
        (offset_y + i64::from(layout.version_label.y) + i64::from(layout.version_label.h) / 2)
            as f64,
    );

    // labAuthor (Times New Roman 11.25pt, 右揃え)
    ctx.set_font("15px 'Times New Roman', serif");
    let _ = ctx.fill_text(
        AUTHORS,
        (offset_x + i64::from(layout.author_label.x) + i64::from(layout.author_label.w)) as f64,
        (offset_y + i64::from(layout.author_label.y) + i64::from(layout.author_label.h) / 2) as f64,
    );

    // labLicense (中央揃え)
    ctx.set_text_align("center");
    ctx.set_font("12px sans-serif");
    let _ = ctx.fill_text(
        LICENSE_NOTICE,
        (offset_x + i64::from(layout.license_label.x) + i64::from(layout.license_label.w) / 2)
            as f64,
        (offset_y + i64::from(layout.license_label.y) + i64::from(layout.license_label.h) / 2)
            as f64,
    );

    // 入力プロンプト (移植版独自)
    ctx.set_fill_style_str("#777");
    ctx.set_font("italic 11px sans-serif");
    let _ = ctx.fill_text(
        "click or press any key to continue",
        f64::from(CANVAS_WIDTH) / 2.0,
        f64::from(CANVAS_HEIGHT) - 8.0,
    );
}

// ===== Configuration scene =====

fn draw_configuration(ctx: &CanvasRenderingContext2d, settings: &Settings) {
    let layout = ConfigurationLayout::original();
    let offset_x = (i64::from(CANVAS_WIDTH) - i64::from(CONFIG_WIDTH)) / 2;
    let offset_y = (i64::from(CANVAS_HEIGHT) - i64::from(CONFIG_HEIGHT)) / 2;

    // ダイアログ本体の枠 + タイトルバー風の描画
    draw_vb6_dialog(
        ctx,
        offset_x,
        offset_y,
        CONFIG_WIDTH,
        CONFIG_HEIGHT,
        CFG_CAPTION,
    );

    // クライアント領域開始 (タイトルバー分下にずらす — 値は src-core と共有)
    let cx = offset_x;
    let cy = offset_y + i64::from(TITLE_BAR_HEIGHT);

    // 各ラベル
    draw_label(ctx, cx, cy, layout.message_speed_label);
    draw_combo(
        ctx,
        cx,
        cy,
        layout.message_speed_combo,
        settings.message_speed.label(),
    );

    // チェックボックス群
    for (cb, value) in [
        (layout.battle_animation, settings.battle_animation),
        (layout.extended_animation, settings.extended_animation),
        (layout.weapon_animation, settings.weapon_animation),
        (
            layout.special_power_animation,
            settings.special_power_animation,
        ),
        (layout.move_animation, settings.move_animation),
        (layout.auto_move_cursor, settings.auto_move_cursor),
        (layout.show_square_line, settings.show_square_line),
        (layout.show_turn, settings.show_turn),
        (layout.keep_enemy_bgm, settings.keep_enemy_bgm),
        (layout.use_direct_music, settings.use_direct_music),
    ] {
        draw_checkbox(ctx, cx, cy, cb, value);
    }

    // MIDI リセット
    draw_label(ctx, cx, cy, layout.midi_reset_label);
    draw_combo(
        ctx,
        cx,
        cy,
        layout.midi_reset_combo,
        settings.midi_reset.label(),
    );

    // MP3 音量
    draw_label(ctx, cx, cy, layout.mp3_volume_label);
    draw_text_input(
        ctx,
        cx,
        cy,
        layout.mp3_volume_text,
        &settings.mp3_volume.to_string(),
    );
    draw_scrollbar(ctx, cx, cy, layout.mp3_volume_scroll, settings.mp3_volume);

    // OK / Cancel
    draw_button(ctx, cx, cy, layout.ok);
    draw_button(ctx, cx, cy, layout.cancel);

    // 入力プロンプト (移植版独自)
    ctx.set_fill_style_str("#bbb");
    ctx.set_font("italic 11px sans-serif");
    ctx.set_text_align("center");
    ctx.set_text_baseline("alphabetic");
    let _ = ctx.fill_text(
        "click or press any key to continue",
        f64::from(CANVAS_WIDTH) / 2.0,
        f64::from(CANVAS_HEIGHT) - 8.0,
    );
}

// ===== Intermission scene =====

fn draw_intermission(ctx: &CanvasRenderingContext2d, items: &[String], cursor: usize) {
    use src_core::scene::intermission::{
        item_rect, INTERMISSION_HEIGHT, INTERMISSION_WIDTH, TITLE_LABEL,
    };

    // 背景: ややダークなパネル
    let ox = (i64::from(CANVAS_WIDTH) - i64::from(INTERMISSION_WIDTH)) / 2;
    let oy = (i64::from(CANVAS_HEIGHT) - i64::from(INTERMISSION_HEIGHT)) / 2;
    ctx.set_fill_style_str("#16213a");
    ctx.fill_rect(
        ox as f64,
        oy as f64,
        f64::from(INTERMISSION_WIDTH),
        f64::from(INTERMISSION_HEIGHT),
    );

    // タイトル
    ctx.set_fill_style_str("#fff176");
    ctx.set_font(&format!("bold 28px {JP_SANS}"));
    ctx.set_text_align("center");
    ctx.set_text_baseline("top");
    let _ = ctx.fill_text(
        TITLE_LABEL,
        (ox + i64::from(INTERMISSION_WIDTH) / 2) as f64,
        (oy + 24) as f64,
    );

    // 説明
    ctx.set_fill_style_str("#aab");
    ctx.set_font(&format!("14px {JP_SANS}"));
    let _ = ctx.fill_text(
        "矢印キー / クリックで選択、Enter で決定",
        (ox + i64::from(INTERMISSION_WIDTH) / 2) as f64,
        (oy + 60) as f64,
    );

    // 各項目
    ctx.set_text_align("left");
    ctx.set_text_baseline("middle");
    ctx.set_font(&format!("18px {JP_SANS}"));
    if items.is_empty() {
        ctx.set_fill_style_str("#777");
        ctx.set_text_align("center");
        let _ = ctx.fill_text(
            "(項目が登録されていません)",
            (ox + i64::from(INTERMISSION_WIDTH) / 2) as f64,
            (oy + i64::from(INTERMISSION_HEIGHT) / 2) as f64,
        );
        return;
    }

    for (i, label) in items.iter().enumerate() {
        let r = item_rect(i);
        let rx = ox + i64::from(r.x);
        let ry = oy + i64::from(r.y);
        let selected = i == cursor;
        // 背景
        ctx.set_fill_style_str(if selected { "#3b4a78" } else { "#22304d" });
        ctx.fill_rect(rx as f64, ry as f64, f64::from(r.w), f64::from(r.h));
        // 枠
        ctx.set_stroke_style_str(if selected { "#fff176" } else { "#4a5b8a" });
        ctx.set_line_width(if selected { 2.0 } else { 1.0 });
        ctx.stroke_rect(rx as f64, ry as f64, f64::from(r.w), f64::from(r.h));
        // ラベル（縦中央寄せ）
        ctx.set_fill_style_str(if selected { "#fff" } else { "#cfd6e6" });
        let _ = ctx.fill_text(label, (rx + 16) as f64, (ry + i64::from(r.h) / 2) as f64);
    }
}

// ===== Pilot list scene =====

fn draw_pilot_list(ctx: &CanvasRenderingContext2d, database: &GameDatabase, assets: &Assets) {
    let ox = (i64::from(CANVAS_WIDTH) - i64::from(PILOT_LIST_WIDTH)) / 2;
    let oy = (i64::from(CANVAS_HEIGHT) - i64::from(PILOT_LIST_HEIGHT)) / 2;

    // 背景
    ctx.set_fill_style_str("#f4f4ec");
    ctx.fill_rect(
        ox as f64,
        oy as f64,
        f64::from(PILOT_LIST_WIDTH),
        f64::from(PILOT_LIST_HEIGHT),
    );
    ctx.set_stroke_style_str("#444");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(
        ox as f64 + 0.5,
        oy as f64 + 0.5,
        f64::from(PILOT_LIST_WIDTH) - 1.0,
        f64::from(PILOT_LIST_HEIGHT) - 1.0,
    );

    // ヘッダタイトル
    ctx.set_fill_style_str("#000");
    ctx.set_font(&format!("bold 13px {JP_SANS}"));
    ctx.set_text_align("left");
    ctx.set_text_baseline("middle");
    let _ = ctx.fill_text(
        &format!("パイロット一覧 ({} 件)", database.pilots.len()),
        ox as f64 + 8.0,
        oy as f64 + 12.0,
    );

    // カラムヘッダ
    let header_y = oy + i64::from(HEADER_TOP);
    let mut col_x = ox + 4;
    ctx.set_fill_style_str("#dadad0");
    ctx.fill_rect(
        ox as f64 + 1.0,
        header_y as f64,
        f64::from(PILOT_LIST_WIDTH) - 2.0,
        f64::from(ROW_HEIGHT),
    );
    ctx.set_fill_style_str("#000");
    ctx.set_font(&format!("bold 11px {JP_SANS}"));
    for col in PL_COLUMNS {
        let _ = ctx.fill_text(
            col.title,
            col_x as f64 + 2.0,
            header_y as f64 + f64::from(ROW_HEIGHT) / 2.0,
        );
        col_x += i64::from(col.width);
    }

    // 罫線（ヘッダ下）
    ctx.set_stroke_style_str("#999");
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(
        ox as f64 + 4.0,
        (header_y + i64::from(ROW_HEIGHT)) as f64 + 0.5,
    );
    ctx.line_to(
        ox as f64 + f64::from(PILOT_LIST_WIDTH) - 4.0,
        (header_y + i64::from(ROW_HEIGHT)) as f64 + 0.5,
    );
    ctx.stroke();

    // 行
    ctx.set_font(&format!("11px {JP_SANS}"));
    let max = plist::max_rows();
    for (row_idx, pilot) in database.pilots.iter().take(max).enumerate() {
        let row_y = header_y + i64::from(ROW_HEIGHT) + (row_idx as i64) * i64::from(ROW_HEIGHT);
        // ストライプ
        if row_idx % 2 == 1 {
            ctx.set_fill_style_str("#eae8d8");
            ctx.fill_rect(
                ox as f64 + 1.0,
                row_y as f64,
                f64::from(PILOT_LIST_WIDTH) - 2.0,
                f64::from(ROW_HEIGHT),
            );
        }

        ctx.set_fill_style_str("#000");
        let stats = format!(
            "{}/{}/{}/{}/{}/{}",
            pilot.infight, pilot.shooting, pilot.hit, pilot.dodge, pilot.intuition, pilot.technique
        );
        let sex = match pilot.sex {
            src_core::data::pilot::Sex::Male => "男",
            src_core::data::pilot::Sex::Female => "女",
            src_core::data::pilot::Sex::Unspecified => "-",
        };
        let cells: [&str; 7] = [
            pilot.name.as_str(),
            pilot.nickname.as_str(),
            sex,
            pilot.class.as_str(),
            pilot.adaption.as_str(),
            "",
            stats.as_str(),
        ];
        let exp_str = pilot.exp_value.to_string();
        let mut x = ox + 4;
        for (i, col) in PL_COLUMNS.iter().enumerate() {
            ctx.save();
            ctx.begin_path();
            ctx.rect(
                x as f64,
                row_y as f64,
                f64::from(col.width),
                f64::from(ROW_HEIGHT),
            );
            ctx.clip();
            if i == 0 {
                // 顔グラセル: pilot.bitmap → nickname → name でフォールバック
                let hint = pilot
                    .bitmap
                    .as_deref()
                    .filter(|s| !s.is_empty())
                    .unwrap_or(pilot.nickname.as_str());
                let img = assets
                    .find_image(hint)
                    .or_else(|| assets.find_image(&pilot.name));
                if let Some(img) = img {
                    let pad = 2.0;
                    let cell_w = f64::from(col.width) - pad * 2.0;
                    let cell_h = f64::from(ROW_HEIGHT) - pad * 2.0;
                    let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
                        img,
                        x as f64 + pad,
                        row_y as f64 + pad,
                        cell_w,
                        cell_h,
                    );
                } else {
                    ctx.set_stroke_style_str("#a8a89a");
                    ctx.stroke_rect(
                        x as f64 + 2.0,
                        row_y as f64 + 2.0,
                        f64::from(col.width) - 4.0,
                        f64::from(ROW_HEIGHT) - 4.0,
                    );
                    ctx.set_fill_style_str("#a8a89a");
                    ctx.set_text_align("center");
                    let _ = ctx.fill_text(
                        "—",
                        x as f64 + f64::from(col.width) / 2.0,
                        row_y as f64 + f64::from(ROW_HEIGHT) / 2.0,
                    );
                    ctx.set_text_align("left");
                    ctx.set_fill_style_str("#000");
                }
            } else {
                let text = if i == 6 {
                    exp_str.as_str()
                } else {
                    cells[i - 1]
                };
                let _ = ctx.fill_text(
                    text,
                    x as f64 + 2.0,
                    row_y as f64 + f64::from(ROW_HEIGHT) / 2.0,
                );
            }
            ctx.restore();
            x += i64::from(col.width);
        }
    }

    // 入力プロンプト
    ctx.set_fill_style_str("#666");
    ctx.set_font(&format!("italic 10px {JP_SANS}"));
    ctx.set_text_align("center");
    let _ = ctx.fill_text(
        "click or press any key to return to Title",
        ox as f64 + f64::from(PILOT_LIST_WIDTH) / 2.0,
        oy as f64 + f64::from(PILOT_LIST_HEIGHT) - 10.0,
    );
}

/// 右側のステータスパネル（カーソル位置のユニット詳細 + ターン情報）。
/// 実 SRC 風: 上段ターン / Stage、中段 HP/EN/装甲/運動/能力値、下段 武器一覧。
#[allow(clippy::too_many_arguments, clippy::cast_precision_loss)]
fn draw_status_panel(
    ctx: &CanvasRenderingContext2d,
    ox: i64,
    oy: i64,
    database: &GameDatabase,
    cursor: Option<(u32, u32)>,
    turn: Turn,
    stage: &str,
    scroll: (u32, u32),
    assets: &Assets,
    selected_weapon_idx: usize,
) {
    let w = f64::from(STATUS_PANEL_WIDTH);
    let h = f64::from(STATUS_PANEL_H);

    // 背景 (薄いグレー)
    ctx.set_fill_style_str("#1f1f24");
    ctx.fill_rect(ox as f64, oy as f64, w, h);
    ctx.set_stroke_style_str("#5a5a66");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(ox as f64 + 0.5, oy as f64 + 0.5, w - 1.0, h - 1.0);

    let mut y = oy as f64 + 6.0;
    let pad_x = ox as f64 + 8.0;

    // 上段: ターン / フェーズ / ステージ
    ctx.set_fill_style_str("#fff176");
    ctx.set_font(&format!("bold 12px {JP_SANS}"));
    ctx.set_text_align("left");
    ctx.set_text_baseline("top");
    let _ = ctx.fill_text(
        &format!("T{} {}", turn.number, turn.phase.label()),
        pad_x,
        y,
    );
    y += 16.0;
    ctx.set_fill_style_str("#cfd8dc");
    ctx.set_font(&format!("11px {JP_SANS}"));
    let stage_text = if stage.is_empty() {
        "(no stage)"
    } else {
        stage
    };
    let _ = ctx.fill_text(stage_text, pad_x, y);
    y += 16.0;
    let (sx, sy) = scroll;
    ctx.set_fill_style_str("#888");
    ctx.set_font(&format!("10px {JP_SANS}"));
    let map_dim = database
        .map
        .as_ref()
        .map(|m| format!("Map {}x{}  view@{},{}", m.width, m.height, sx, sy))
        .unwrap_or_else(|| "Map: (none)".into());
    let _ = ctx.fill_text(&map_dim, pad_x, y);
    y += 14.0;

    // 区切り線
    ctx.set_stroke_style_str("#3a3a45");
    ctx.begin_path();
    ctx.move_to(pad_x, y + 2.0);
    ctx.line_to(ox as f64 + w - 8.0, y + 2.0);
    ctx.stroke();
    y += 8.0;

    // 中段: カーソル位置のユニット詳細
    let Some((cx, cy)) = cursor else {
        ctx.set_fill_style_str("#6a6a78");
        ctx.set_font(&format!("italic 11px {JP_SANS}"));
        let _ = ctx.fill_text("カーソルなし", pad_x, y);
        return;
    };

    // 地形情報を 1 行で
    if let Some(map) = database.map.as_ref() {
        if cx < map.width && cy < map.height {
            if let Some(t) = terrain::lookup(map.cell(cx, cy).terrain_id) {
                ctx.set_fill_style_str("#9ad7ff");
                ctx.set_font(&format!("11px {JP_SANS}"));
                let _ = ctx.fill_text(
                    &format!(
                        "地形: {} ({},{}) 移動{} 回避{:+}",
                        t.name, cx, cy, t.move_cost, t.hit_mod
                    ),
                    pad_x,
                    y,
                );
                y += 16.0;
            }
        }
    }

    let unit_inst = database.units_at(cx, cy).next();
    let Some(u) = unit_inst else {
        ctx.set_fill_style_str("#6a6a78");
        ctx.set_font(&format!("italic 11px {JP_SANS}"));
        let _ = ctx.fill_text("(ユニットなし)", pad_x, y);
        return;
    };

    let unit_def = database.unit_by_name(&u.unit_data_name);
    let pilot_def = database.pilot_by_name(&u.pilot_name);

    // ユニット名 + パイロット顔グラ (右パネル上部にコンパクトに 48×48)
    ctx.set_fill_style_str("#ffffff");
    ctx.set_font(&format!("bold 12px {JP_SANS}"));
    let face_size = 48.0;
    let text_x = pad_x + face_size + 6.0;
    if let Some(p) = pilot_def {
        let hint = p.bitmap.clone().unwrap_or_else(|| p.nickname.clone());
        if let Some(img) = assets.find_image(&hint) {
            // 顔グラ枠 + 画像
            ctx.set_stroke_style_str("#444");
            ctx.set_line_width(1.0);
            ctx.stroke_rect(pad_x + 0.5, y + 0.5, face_size, face_size);
            let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
                img, pad_x, y, face_size, face_size,
            );
        } else {
            // 顔グラなしの placeholder
            ctx.set_fill_style_str("#3a3a45");
            ctx.fill_rect(pad_x, y, face_size, face_size);
        }
    }
    let _ = ctx.fill_text(
        &format!("[{}] {}", u.party.short_label(), u.unit_data_name),
        text_x,
        y,
    );
    y += 14.0;
    if let Some(p) = pilot_def {
        ctx.set_fill_style_str("#cfd8dc");
        ctx.set_font(&format!("11px {JP_SANS}"));
        let _ = ctx.fill_text(&format!("Pilot: {}", p.nickname), text_x, y);
        y += 14.0;
    }
    // 顔グラ右下まで使う場合は y を顔グラ下端に同期
    if pilot_def.is_some() {
        y = y.max(oy as f64 + 90.0 - 12.0);
    }

    // HP / EN バー (HP は damage に向けて補間中の displayed_damage を使う)
    if let Some(d) = unit_def {
        let current_hp = (d.hp as f64 - u.displayed_damage).max(0.0);
        draw_bar(
            ctx,
            pad_x,
            y,
            w - 16.0,
            10.0,
            current_hp,
            d.hp as f64,
            "#e53935",
            "HP",
        );
        y += 14.0;
        draw_bar(
            ctx,
            pad_x,
            y,
            w - 16.0,
            10.0,
            d.en as f64,
            d.en as f64,
            "#1e88e5",
            "EN",
        );
        y += 14.0;
        // 装甲 / 運動 / 移動 / サイズ
        ctx.set_fill_style_str("#e0e0e0");
        ctx.set_font(&format!("10px {JP_SANS}"));
        let _ = ctx.fill_text(
            &format!("装甲 {}  運動 {}  移動 {}", d.armor, d.mobility, d.speed),
            pad_x,
            y,
        );
        y += 12.0;
        let _ = ctx.fill_text(
            &format!(
                "適応 {}  サイズ {}  移地 {}",
                d.adaption.as_str(),
                d.size.label(),
                d.transportation
            ),
            pad_x,
            y,
        );
        y += 14.0;
    }

    // パイロット能力値（コンパクト 2 列）
    if let Some(p) = pilot_def {
        ctx.set_fill_style_str("#cfd8dc");
        ctx.set_font(&format!("10px {JP_SANS}"));
        let col = w / 2.0 - 4.0;
        let mut row_y = y;
        let _ = ctx.fill_text(&format!("格闘 {}", p.infight), pad_x, row_y);
        let _ = ctx.fill_text(&format!("射撃 {}", p.shooting), pad_x + col, row_y);
        row_y += 12.0;
        let _ = ctx.fill_text(&format!("命中 {}", p.hit), pad_x, row_y);
        let _ = ctx.fill_text(&format!("回避 {}", p.dodge), pad_x + col, row_y);
        row_y += 12.0;
        let _ = ctx.fill_text(&format!("反応 {}", p.intuition), pad_x, row_y);
        let _ = ctx.fill_text(&format!("技量 {}", p.technique), pad_x + col, row_y);
        row_y += 12.0;
        if let Some(sp) = p.sp {
            let _ = ctx.fill_text(&format!("SP {}", sp), pad_x, row_y);
            row_y += 12.0;
        }
        y = row_y + 4.0;
    }

    // 武器一覧 (compact) + 選択中ハイライト
    if let Some(d) = unit_def {
        if !d.weapons.is_empty() {
            ctx.set_stroke_style_str("#3a3a45");
            ctx.begin_path();
            ctx.move_to(pad_x, y);
            ctx.line_to(ox as f64 + w - 8.0, y);
            ctx.stroke();
            y += 6.0;
            ctx.set_fill_style_str("#9ad7ff");
            ctx.set_font(&format!("bold 10px {JP_SANS}"));
            let label = if selected_weapon_idx == 0 {
                "武器 (W で選択: 自動)".to_string()
            } else {
                format!("武器 (W: 固定 #{selected_weapon_idx})")
            };
            let _ = ctx.fill_text(&label, pad_x, y);
            y += 12.0;
            ctx.set_font(&format!("10px {JP_SANS}"));
            let panel_bottom = oy as f64 + h - 30.0;
            for (i, w_def) in d.weapons.iter().enumerate() {
                if y + 12.0 > panel_bottom {
                    ctx.set_fill_style_str("#e0e0e0");
                    let _ = ctx.fill_text("…", pad_x, y);
                    break;
                }
                let active = selected_weapon_idx == i + 1;
                if active {
                    ctx.set_fill_style_str("rgba(255,235,59,0.18)");
                    ctx.fill_rect(pad_x - 2.0, y - 1.0, w - 12.0, 12.0);
                }
                ctx.set_fill_style_str(if active { "#fff176" } else { "#e0e0e0" });
                let line = format!(
                    "{}{:<10}P{} {}-{}",
                    if active { "▶ " } else { "  " },
                    truncate(&w_def.name, 10),
                    w_def.power,
                    w_def.min_range,
                    w_def.max_range
                );
                let _ = ctx.fill_text(&line, pad_x, y);
                y += 12.0;
            }
        }
    }

    // 戦闘予測（パネル末尾）
    if let Some(preview_line) = build_combat_preview_line(database, Some(u), cx, cy) {
        let bottom = oy as f64 + h - 4.0;
        ctx.set_fill_style_str("#fff176");
        ctx.set_font(&format!("bold 10px {JP_SANS}"));
        ctx.set_text_align("left");
        ctx.set_text_baseline("bottom");
        let _ = ctx.fill_text(&preview_line, pad_x, bottom);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_bar(
    ctx: &CanvasRenderingContext2d,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    current: f64,
    max: f64,
    color: &str,
    label: &str,
) {
    // 背景
    ctx.set_fill_style_str("#33333a");
    ctx.fill_rect(x, y, w, h);
    let frac = if max <= 0.0 {
        0.0
    } else {
        (current / max).clamp(0.0, 1.0)
    };
    ctx.set_fill_style_str(color);
    ctx.fill_rect(x, y, w * frac, h);
    ctx.set_stroke_style_str("#000");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0);
    // ラベル
    ctx.set_fill_style_str("#fff");
    ctx.set_font(&format!("bold 9px {JP_SANS}"));
    ctx.set_text_align("left");
    ctx.set_text_baseline("middle");
    let _ = ctx.fill_text(
        &format!("{label} {}/{}", current.round() as i64, max.round() as i64),
        x + 4.0,
        y + h / 2.0,
    );
}

fn truncate(s: &str, max_chars: usize) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i >= max_chars {
            break;
        }
        out.push(c);
    }
    out
}

/// 下部のメッセージボックス（顔グラ枠 + 本文 + ▼ 進行マーカ）。
#[allow(clippy::too_many_arguments)]
fn draw_message_box(
    ctx: &CanvasRenderingContext2d,
    ox: i64,
    oy: i64,
    last_message: Option<&str>,
    face_hint: Option<&str>,
    assets: &Assets,
    queued: usize,
) {
    let w = f64::from(MAP_VIEW_WIDTH);
    let h = f64::from(MESSAGE_BOX_HEIGHT);

    // 背景 + 枠（VB6 メッセージウィンドウ風: 白背景 + 凹型枠）
    ctx.set_fill_style_str("#f0f0f0");
    ctx.fill_rect(ox as f64, oy as f64, w, h);
    ctx.set_stroke_style_str("#808080");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(ox as f64 + 0.5, oy as f64 + 0.5, w - 1.0, h - 1.0);

    // 顔グラ (実画像があれば描画、無ければプレースホルダ枠)
    let p = f64::from(PORTRAIT_SIZE);
    let face_x = ox as f64 + 8.0;
    let face_y = oy as f64 + (h - p) / 2.0;
    let face_img = face_hint.and_then(|h| assets.find_image(h));
    match face_img {
        Some(img) => {
            ctx.set_fill_style_str("#000");
            ctx.fill_rect(face_x, face_y, p, p);
            let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(img, face_x, face_y, p, p);
            ctx.set_stroke_style_str("#444");
            ctx.set_line_width(1.0);
            ctx.stroke_rect(face_x + 0.5, face_y + 0.5, p - 1.0, p - 1.0);
        }
        None => {
            ctx.set_fill_style_str("#cfd8dc");
            ctx.fill_rect(face_x, face_y, p, p);
            ctx.set_stroke_style_str("#888");
            ctx.set_line_width(1.0);
            ctx.set_line_dash(&js_sys::Array::of2(&3.0.into(), &3.0.into()).into())
                .ok();
            ctx.stroke_rect(face_x + 0.5, face_y + 0.5, p - 1.0, p - 1.0);
            ctx.set_line_dash(&js_sys::Array::new().into()).ok();
            ctx.set_fill_style_str("#888");
            ctx.set_font(&format!("10px {JP_SANS}"));
            ctx.set_text_align("center");
            ctx.set_text_baseline("middle");
            let _ = ctx.fill_text("face", face_x + p / 2.0, face_y + p / 2.0);
        }
    }

    // メッセージ本文
    ctx.set_fill_style_str("#222");
    ctx.set_font(&format!("12px {JP_SANS}"));
    ctx.set_text_align("left");
    ctx.set_text_baseline("top");
    let text_x = face_x + p + 12.0;
    let text_y = oy as f64 + 12.0;
    match last_message {
        Some(msg) => {
            // 簡易折返し: 35 文字ごと
            for (i, chunk) in wrap_text(msg, 36).into_iter().take(4).enumerate() {
                let _ = ctx.fill_text(&chunk, text_x, text_y + 16.0 * i as f64);
            }
        }
        None => {
            ctx.set_fill_style_str("#888");
            ctx.set_font(&format!("italic 11px {JP_SANS}"));
            let _ = ctx.fill_text(
                "メッセージなし（スペース=フェーズ終了 / a=攻撃 / w=武器 / 矢印=カーソル）",
                text_x,
                text_y,
            );
        }
    }

    // ▼ 進行マーカ（右下に三角）。queued > 0 のときだけ表示。
    if queued > 0 {
        let mx = ox as f64 + w - 18.0;
        let my = oy as f64 + h - 14.0;
        ctx.set_fill_style_str("#3a3a45");
        ctx.begin_path();
        ctx.move_to(mx - 6.0, my);
        ctx.line_to(mx + 6.0, my);
        ctx.line_to(mx, my + 8.0);
        ctx.close_path();
        ctx.fill();
        // 件数バッジ
        if queued > 1 {
            ctx.set_fill_style_str("#3a3a45");
            ctx.set_font(&format!("bold 10px {JP_SANS}"));
            ctx.set_text_align("right");
            ctx.set_text_baseline("bottom");
            let _ = ctx.fill_text(&format!("{queued}件"), mx - 10.0, my + 8.0);
        }
    }
}

fn wrap_text(s: &str, max_chars_per_line: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut n = 0;
    for ch in s.chars() {
        if ch == '\n' {
            out.push(std::mem::take(&mut cur));
            n = 0;
            continue;
        }
        cur.push(ch);
        // 全角は 2 幅相当として数える
        n += if ch.is_ascii() { 1 } else { 2 };
        if n >= max_chars_per_line {
            out.push(std::mem::take(&mut cur));
            n = 0;
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn build_combat_preview_line(
    database: &GameDatabase,
    cursor_unit: Option<&UnitInstance>,
    cx: u32,
    cy: u32,
) -> Option<String> {
    let atk = cursor_unit?;
    let atk_pilot = database.pilot_by_name(&atk.pilot_name)?;
    let atk_unit = database.unit_by_name(&atk.unit_data_name)?;
    let map = database.map.as_ref()?;

    // 全マップから敵対勢力ユニットを線形探索。マンハッタン距離が武器射程に
    // 入るものから、最大射程までを優先順位なし（最も近い／最も強い武器）で取る。
    let mut best: Option<(u32, u32, &UnitInstance, &src_core::data::unit::WeaponData)> = None;
    for def in &database.unit_instances {
        if def.party == atk.party {
            continue;
        }
        let dist = combat::manhattan((cx, cy), (def.x, def.y));
        if let Some(w) = combat::best_weapon_in_range(atk_unit, dist) {
            // より小さい距離を優先、同距離なら power 大
            let take = match best {
                None => true,
                Some((bx, by, _, bw)) => {
                    let bd = combat::manhattan((cx, cy), (bx, by));
                    dist < bd || (dist == bd && w.power > bw.power)
                }
            };
            if take {
                best = Some((def.x, def.y, def, w));
            }
        }
    }

    let (dx, dy, def, weapon) = best?;
    let def_pilot = database.pilot_by_name(&def.pilot_name)?;
    let def_unit = database.unit_by_name(&def.unit_data_name)?;
    let tid = map.cell(dx, dy).terrain_id;
    let hit_mod = database.terrain_hit_mod(tid);
    let damage_mod = database.terrain_damage_mod(tid);
    // 地形適応をプレビューにも反映し、実解決ダメージと表示を一致させる。
    let env_of = |x: u32, y: u32| {
        terrain::lookup(map.cell(x, y).terrain_id)
            .map(|t| combat::terrain_env(t.class))
            .unwrap_or(1)
    };
    let cp = combat::predict_with_status_terrain(
        atk_pilot,
        atk_unit,
        weapon,
        def_pilot,
        def_unit,
        hit_mod,
        damage_mod,
        100,
        100,
        &[],
        &[],
        env_of(cx, cy),
        env_of(dx, dy),
        // 状態異常スナップショットなしのプレビュー行 → 与・被ダメージ修正なし。
        combat::DamageSpiritLevels::default(),
    )
    // 散 (散布) 属性武器の距離補正 (命中アップ・ダメージダウン) をプレビューにも反映。
    .apply_scatter(&weapon.class, combat::manhattan((cx, cy), (dx, dy)));
    Some(format!(
        "→ vs [{}] {} ({},{})  [{}] dist={}  命中:{}%  ダメージ:{}",
        def.party.short_label(),
        def_pilot.nickname,
        dx,
        dy,
        weapon.name,
        combat::manhattan((cx, cy), (dx, dy)),
        cp.hit_chance.min(100),
        cp.damage
    ))
}

/// マップタイル上に重ねる小さなユニットチップ。
/// チップ色は所属勢力、上 1/3 にユニット愛称、下 1/3 に勢力ラベルを表示。
fn draw_unit_chip(ctx: &CanvasRenderingContext2d, x: i64, y: i64, u: &UnitInstance) {
    let pad = 3.0;
    let xf = x as f64 + pad;
    let yf = y as f64 + pad;
    let size = f64::from(TILE_SIZE) - pad * 2.0;

    // 角丸風（Canvas2D の rect だけで擬似的に表現: 内側を fill + 外周を stroke）
    ctx.set_fill_style_str(u.party.color());
    ctx.fill_rect(xf, yf, size, size);
    ctx.set_stroke_style_str("rgba(0,0,0,0.55)");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(xf + 0.5, yf + 0.5, size - 1.0, size - 1.0);

    // 上端に薄色の帯
    ctx.set_fill_style_str("rgba(255,255,255,0.85)");
    ctx.fill_rect(xf + 1.0, yf + 1.0, size - 2.0, size * 0.4);

    // 愛称（unit_data_name の先頭 4 文字）
    let label: String = u.unit_data_name.chars().take(4).collect();
    ctx.set_fill_style_str("#111");
    ctx.set_font(&format!("bold 9px {JP_SANS}"));
    ctx.set_text_align("center");
    ctx.set_text_baseline("middle");
    let _ = ctx.fill_text(&label, xf + size / 2.0, yf + size * 0.25);

    // 下半は勢力ラベル
    ctx.set_fill_style_str("#fff");
    ctx.set_font(&format!("bold 11px {JP_SANS}"));
    let _ = ctx.fill_text(u.party.short_label(), xf + size / 2.0, yf + size * 0.72);
}

// ===== Unit list scene =====

fn draw_unit_list(ctx: &CanvasRenderingContext2d, database: &GameDatabase, assets: &Assets) {
    let ox = (i64::from(CANVAS_WIDTH) - i64::from(UNIT_LIST_WIDTH)) / 2;
    let oy = (i64::from(CANVAS_HEIGHT) - i64::from(UNIT_LIST_HEIGHT)) / 2;

    // 背景 + 外枠
    ctx.set_fill_style_str("#eef4f8");
    ctx.fill_rect(
        ox as f64,
        oy as f64,
        f64::from(UNIT_LIST_WIDTH),
        f64::from(UNIT_LIST_HEIGHT),
    );
    ctx.set_stroke_style_str("#445566");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(
        ox as f64 + 0.5,
        oy as f64 + 0.5,
        f64::from(UNIT_LIST_WIDTH) - 1.0,
        f64::from(UNIT_LIST_HEIGHT) - 1.0,
    );

    // タイトル
    ctx.set_fill_style_str("#000");
    ctx.set_font(&format!("bold 13px {JP_SANS}"));
    ctx.set_text_align("left");
    ctx.set_text_baseline("middle");
    let _ = ctx.fill_text(
        &format!("ユニット一覧 ({} 件)", database.units.len()),
        ox as f64 + 8.0,
        oy as f64 + 12.0,
    );

    // カラムヘッダ
    let header_y = oy + i64::from(UL_HEADER_TOP);
    ctx.set_fill_style_str("#cfd9e0");
    ctx.fill_rect(
        ox as f64 + 1.0,
        header_y as f64,
        f64::from(UNIT_LIST_WIDTH) - 2.0,
        f64::from(UL_ROW_HEIGHT),
    );
    let mut col_x = ox + 4;
    ctx.set_fill_style_str("#000");
    ctx.set_font(&format!("bold 11px {JP_SANS}"));
    for col in UL_COLUMNS {
        let _ = ctx.fill_text(
            col.title,
            col_x as f64 + 2.0,
            header_y as f64 + f64::from(UL_ROW_HEIGHT) / 2.0,
        );
        col_x += i64::from(col.width);
    }
    ctx.set_stroke_style_str("#778899");
    ctx.begin_path();
    ctx.move_to(
        ox as f64 + 4.0,
        (header_y + i64::from(UL_ROW_HEIGHT)) as f64 + 0.5,
    );
    ctx.line_to(
        ox as f64 + f64::from(UNIT_LIST_WIDTH) - 4.0,
        (header_y + i64::from(UL_ROW_HEIGHT)) as f64 + 0.5,
    );
    ctx.stroke();

    // 行
    ctx.set_font(&format!("11px {JP_SANS}"));
    let max = ulist::max_rows();
    for (row_idx, u) in database.units.iter().take(max).enumerate() {
        let row_y =
            header_y + i64::from(UL_ROW_HEIGHT) + (row_idx as i64) * i64::from(UL_ROW_HEIGHT);
        if row_idx % 2 == 1 {
            ctx.set_fill_style_str("#dfe7ec");
            ctx.fill_rect(
                ox as f64 + 1.0,
                row_y as f64,
                f64::from(UNIT_LIST_WIDTH) - 2.0,
                f64::from(UL_ROW_HEIGHT),
            );
        }
        ctx.set_fill_style_str("#000");

        let hp = u.hp.to_string();
        let en = u.en.to_string();
        let armor = u.armor.to_string();
        let mob = u.mobility.to_string();
        let move_speed = format!("{}/{}", u.transportation, u.speed);

        // 画像セル + テキストセル ("画像" カラムを先頭に追加してから10列のテキスト)
        let cells: [&str; 10] = [
            u.name.as_str(),
            u.nickname.as_str(),
            u.class.as_str(),
            u.size.label(),
            hp.as_str(),
            en.as_str(),
            armor.as_str(),
            mob.as_str(),
            u.adaption.as_str(),
            move_speed.as_str(),
        ];

        let mut x = ox + 4;
        for (i, col) in UL_COLUMNS.iter().enumerate() {
            ctx.save();
            ctx.begin_path();
            ctx.rect(
                x as f64,
                row_y as f64,
                f64::from(col.width),
                f64::from(UL_ROW_HEIGHT),
            );
            ctx.clip();
            if i == 0 {
                // 画像セル: UnitData.bitmap → Assets.find_image
                let hint = if !u.bitmap.is_empty() {
                    u.bitmap.as_str()
                } else {
                    u.name.as_str()
                };
                if let Some(img) = assets.find_image(hint) {
                    let pad = 2.0;
                    let cell_w = f64::from(col.width) - pad * 2.0;
                    let cell_h = f64::from(UL_ROW_HEIGHT) - pad * 2.0;
                    let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
                        img,
                        x as f64 + pad,
                        row_y as f64 + pad,
                        cell_w,
                        cell_h,
                    );
                } else {
                    // プレースホルダ
                    ctx.set_stroke_style_str("#9aa0a8");
                    ctx.stroke_rect(
                        x as f64 + 2.0,
                        row_y as f64 + 2.0,
                        f64::from(col.width) - 4.0,
                        f64::from(UL_ROW_HEIGHT) - 4.0,
                    );
                    ctx.set_fill_style_str("#9aa0a8");
                    ctx.set_text_align("center");
                    let _ = ctx.fill_text(
                        "—",
                        x as f64 + f64::from(col.width) / 2.0,
                        row_y as f64 + f64::from(UL_ROW_HEIGHT) / 2.0,
                    );
                    ctx.set_text_align("left");
                    ctx.set_fill_style_str("#000");
                }
            } else {
                let _ = ctx.fill_text(
                    cells[i - 1],
                    x as f64 + 2.0,
                    row_y as f64 + f64::from(UL_ROW_HEIGHT) / 2.0,
                );
            }
            ctx.restore();
            x += i64::from(col.width);
        }
    }

    // 入力プロンプト
    ctx.set_fill_style_str("#5a6b78");
    ctx.set_font(&format!("italic 10px {JP_SANS}"));
    ctx.set_text_align("center");
    let _ = ctx.fill_text(
        "click or press any key to return to Title",
        ox as f64 + f64::from(UNIT_LIST_WIDTH) / 2.0,
        oy as f64 + f64::from(UNIT_LIST_HEIGHT) - 10.0,
    );
}

/// 単機ステータス詳細画面 (`Scene::UnitDetail`) を描画する。
/// 味方ロスター 1 機ぶんの実効ステータス (機体 + 搭乗パイロット + 武器) を
/// 2 カラムで表示し、フッタに ◀ / ▶ / 閉じる ボタンを置く。
fn draw_unit_detail(
    ctx: &CanvasRenderingContext2d,
    detail: Option<&StatusDetail>,
    assets: &Assets,
) {
    let ox = (i64::from(CANVAS_WIDTH) - i64::from(UNIT_DETAIL_WIDTH)) / 2;
    let oy = (i64::from(CANVAS_HEIGHT) - i64::from(UNIT_DETAIL_HEIGHT)) / 2;
    let w = f64::from(UNIT_DETAIL_WIDTH);
    let h = f64::from(UNIT_DETAIL_HEIGHT);

    // 背景 + 外枠
    ctx.set_fill_style_str("#eef2f6");
    ctx.fill_rect(ox as f64, oy as f64, w, h);
    ctx.set_stroke_style_str("#44525e");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(ox as f64 + 0.5, oy as f64 + 0.5, w - 1.0, h - 1.0);

    ctx.set_text_baseline("middle");

    let Some(d) = detail else {
        // ロスターが空 (本来 ステータス 項目自体が出ないが防御的に)。
        ctx.set_fill_style_str("#445");
        ctx.set_font(&format!("14px {JP_SANS}"));
        ctx.set_text_align("center");
        let _ = ctx.fill_text(
            "表示できるユニットがいません",
            ox as f64 + w / 2.0,
            oy as f64 + h / 2.0,
        );
        draw_detail_button(ctx, ox, oy, udetail::close_button(), "閉じる");
        return;
    };

    // タイトルバー
    ctx.set_fill_style_str("#2b3a4a");
    ctx.fill_rect(ox as f64 + 1.0, oy as f64 + 1.0, w - 2.0, 27.0);
    ctx.set_fill_style_str("#fff");
    ctx.set_font(&format!("bold 14px {JP_SANS}"));
    ctx.set_text_align("left");
    let title = if d.unit_nickname.is_empty() || d.unit_nickname == d.unit_name {
        d.unit_name.clone()
    } else {
        format!("{}（{}）", d.unit_name, d.unit_nickname)
    };
    let _ = ctx.fill_text(
        &format!("ステータス  ◆ {title}  [{}]", d.party_label),
        ox as f64 + 12.0,
        oy as f64 + 15.0,
    );
    ctx.set_text_align("right");
    let _ = ctx.fill_text(
        &format!("{} / {}", d.index + 1, d.total),
        ox as f64 + w - 12.0,
        oy as f64 + 15.0,
    );
    ctx.set_text_align("left");

    // ===== 機体カラム (左) =====
    let lx = ox + 16;
    let mut ly = oy + 44;
    section_header(ctx, lx, ly, "■ 機体");
    // ユニットサムネ (右肩)
    if let Some(img) = assets.find_image(&d.unit_name) {
        let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
            img,
            (ox + 250) as f64,
            (oy + 36) as f64,
            48.0,
            48.0,
        );
    }
    ly += 24;
    ctx.set_font(&format!("12px {JP_SANS}"));
    ctx.set_fill_style_str("#102030");
    let stat_line = |ctx: &CanvasRenderingContext2d, y: i64, label: &str, val: &str| {
        ctx.set_fill_style_str("#54616c");
        let _ = ctx.fill_text(label, lx as f64, y as f64);
        ctx.set_fill_style_str("#102030");
        let _ = ctx.fill_text(val, lx as f64 + 64.0, y as f64);
    };
    stat_line(
        ctx,
        ly,
        "クラス",
        if d.class.is_empty() { "—" } else { &d.class },
    );
    ly += 20;
    stat_line(
        ctx,
        ly,
        "サイズ",
        &format!("{}   適応 {}", d.size, d.unit_adaption),
    );
    ly += 20;
    stat_line(ctx, ly, "HP", &format!("{} / {}", d.hp_cur, d.hp_max));
    ly += 20;
    stat_line(ctx, ly, "EN", &format!("{} / {}", d.en_cur, d.en_max));
    ly += 20;
    stat_line(
        ctx,
        ly,
        "装甲",
        &format!("{}    運動性 {}", d.armor, d.mobility),
    );
    ly += 20;
    stat_line(
        ctx,
        ly,
        "移動力",
        &format!("{}    改造 +{}", d.speed, d.upgrade_level),
    );
    ly += 20;
    stat_line(ctx, ly, "士気", &d.unit_morale.to_string());
    ly += 20;
    let cond = if d.conditions.is_empty() {
        "—".to_string()
    } else {
        d.conditions.join(" / ")
    };
    ctx.set_fill_style_str("#54616c");
    let _ = ctx.fill_text("状態", lx as f64, ly as f64);
    ctx.set_fill_style_str(if d.conditions.is_empty() {
        "#102030"
    } else {
        "#b5483a"
    });
    draw_wrapped(ctx, &[cond], lx + 64, ly, 232.0, 18, 2);

    // ===== パイロットカラム (右) =====
    let rx = ox + 330;
    let mut ry = oy + 44;
    section_header(ctx, rx, ry, "■ パイロット");
    if let Some(img) = assets.find_image(&d.pilot_name) {
        let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
            img,
            (ox + 568) as f64,
            (oy + 36) as f64,
            48.0,
            48.0,
        );
    }
    ry += 24;
    ctx.set_font(&format!("12px {JP_SANS}"));
    if !d.has_pilot {
        ctx.set_fill_style_str("#8a6");
        let _ = ctx.fill_text("（パイロット不在）", rx as f64, ry as f64);
    } else {
        let rstat = |ctx: &CanvasRenderingContext2d, y: i64, label: &str, val: &str| {
            ctx.set_fill_style_str("#54616c");
            let _ = ctx.fill_text(label, rx as f64, y as f64);
            ctx.set_fill_style_str("#102030");
            let _ = ctx.fill_text(val, rx as f64 + 64.0, y as f64);
        };
        let pname = if d.pilot_nickname.is_empty() || d.pilot_nickname == d.pilot_name {
            d.pilot_name.clone()
        } else {
            format!("{}（{}）", d.pilot_name, d.pilot_nickname)
        };
        ctx.set_fill_style_str("#102030");
        ctx.set_font(&format!("bold 12px {JP_SANS}"));
        let _ = ctx.fill_text(&pname, rx as f64, ry as f64);
        ctx.set_font(&format!("12px {JP_SANS}"));
        ry += 20;
        rstat(ctx, ry, "Lv", &format!("{}    Exp {}", d.level, d.exp));
        ry += 20;
        rstat(ctx, ry, "SP", &format!("{} / {}", d.sp_cur, d.sp_max));
        ry += 20;
        rstat(
            ctx,
            ry,
            "格闘",
            &format!("{}    射撃 {}", d.infight, d.shooting),
        );
        ry += 20;
        rstat(ctx, ry, "命中", &format!("{}    回避 {}", d.hit, d.dodge));
        ry += 20;
        rstat(
            ctx,
            ry,
            "技量",
            &format!("{}    反応 {}", d.technique, d.intuition),
        );
        ry += 20;
        rstat(ctx, ry, "適応", &d.pilot_adaption);
        ry += 22;
        ctx.set_fill_style_str("#54616c");
        let _ = ctx.fill_text("精神", rx as f64, ry as f64);
        ctx.set_fill_style_str("#234");
        if d.spirit_commands.is_empty() {
            let _ = ctx.fill_text("—", rx as f64 + 40.0, ry as f64);
        } else {
            draw_wrapped(ctx, &d.spirit_commands, rx + 40, ry, 240.0, 18, 3);
        }
        ry += if d.spirit_commands.len() > 4 { 56 } else { 38 };
        ctx.set_fill_style_str("#54616c");
        let _ = ctx.fill_text("技能", rx as f64, ry as f64);
        ctx.set_fill_style_str("#234");
        if d.skills.is_empty() {
            let _ = ctx.fill_text("—", rx as f64 + 40.0, ry as f64);
        } else {
            draw_wrapped(ctx, &d.skills, rx + 40, ry, 240.0, 18, 2);
        }
    }

    // ===== 武器セクション (下) =====
    let wy0 = oy + 308;
    section_header(ctx, lx, wy0, "■ 武器");
    ctx.set_stroke_style_str("#aab4bc");
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(lx as f64, (wy0 + 14) as f64 + 0.5);
    ctx.line_to(
        (ox + UNIT_DETAIL_WIDTH as i64 - 16) as f64,
        (wy0 + 14) as f64 + 0.5,
    );
    ctx.stroke();
    // ヘッダ
    ctx.set_font(&format!("bold 11px {JP_SANS}"));
    ctx.set_fill_style_str("#54616c");
    let col_name = lx;
    let col_pow = ox + 280;
    let col_rng = ox + 380;
    let col_ammo = ox + 470;
    let hy = wy0 + 28;
    let _ = ctx.fill_text("名称", col_name as f64, hy as f64);
    let _ = ctx.fill_text("攻撃力", col_pow as f64, hy as f64);
    let _ = ctx.fill_text("射程", col_rng as f64, hy as f64);
    let _ = ctx.fill_text("弾/EN", col_ammo as f64, hy as f64);
    ctx.set_font(&format!("12px {JP_SANS}"));
    let max_wrows = 5usize;
    for (i, wrow) in d.weapons.iter().take(max_wrows).enumerate() {
        let y = hy + 20 + i as i64 * 20;
        // 必要技能未達の武器はグレー表示し、名称に「技能不足」を併記する。
        ctx.set_fill_style_str(if wrow.usable { "#102030" } else { "#9aa0a8" });
        let name = if wrow.usable {
            wrow.name.clone()
        } else {
            format!("{} (技能不足)", wrow.name)
        };
        let _ = ctx.fill_text(&name, col_name as f64, y as f64);
        let _ = ctx.fill_text(&wrow.power.to_string(), col_pow as f64, y as f64);
        let _ = ctx.fill_text(&wrow.range, col_rng as f64, y as f64);
        let _ = ctx.fill_text(&wrow.ammo, col_ammo as f64, y as f64);
    }
    if d.weapons.is_empty() {
        ctx.set_fill_style_str("#8a929a");
        let _ = ctx.fill_text("（武器なし）", col_name as f64, (hy + 20) as f64);
    } else if d.weapons.len() > max_wrows {
        ctx.set_fill_style_str("#8a929a");
        let _ = ctx.fill_text(
            &format!("… 他 {} 件", d.weapons.len() - max_wrows),
            col_name as f64,
            (hy + 20 + max_wrows as i64 * 20) as f64,
        );
    }

    // ===== フッタボタン =====
    draw_detail_button(ctx, ox, oy, udetail::prev_button(), "◀ 前");
    draw_detail_button(ctx, ox, oy, udetail::next_button(), "次 ▶");
    draw_detail_button(ctx, ox, oy, udetail::close_button(), "閉じる");
    ctx.set_fill_style_str("#5a6b78");
    ctx.set_font(&format!("italic 10px {JP_SANS}"));
    ctx.set_text_align("center");
    let _ = ctx.fill_text(
        "◀ / ▶ で他の機体  ·  Enter / 右クリックで戻る",
        ox as f64 + w / 2.0,
        oy as f64 + h - 8.0,
    );
    ctx.set_text_align("left");
}

/// セクション見出しを描く。
fn section_header(ctx: &CanvasRenderingContext2d, x: i64, y: i64, label: &str) {
    ctx.set_fill_style_str("#2b6080");
    ctx.set_font(&format!("bold 13px {JP_SANS}"));
    ctx.set_text_align("left");
    let _ = ctx.fill_text(label, x as f64, y as f64);
}

/// フッタのナビゲーションボタン (シーンローカル `rect` をオフセット `(ox, oy)` で描画)。
fn draw_detail_button(ctx: &CanvasRenderingContext2d, ox: i64, oy: i64, rect: Rect, label: &str) {
    let x = ox + rect.x as i64;
    let y = oy + rect.y as i64;
    ctx.set_fill_style_str("#d4dde4");
    ctx.fill_rect(x as f64, y as f64, f64::from(rect.w), f64::from(rect.h));
    ctx.set_stroke_style_str("#6b7a86");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(
        x as f64 + 0.5,
        y as f64 + 0.5,
        f64::from(rect.w) - 1.0,
        f64::from(rect.h) - 1.0,
    );
    ctx.set_fill_style_str("#1a2730");
    ctx.set_font(&format!("bold 12px {JP_SANS}"));
    ctx.set_text_align("center");
    ctx.set_text_baseline("middle");
    let _ = ctx.fill_text(
        label,
        x as f64 + f64::from(rect.w) / 2.0,
        y as f64 + f64::from(rect.h) / 2.0,
    );
    ctx.set_text_align("left");
}

/// `items` を `/` 区切りで `max_w` 幅に折り返して描画する (最大 `max_lines` 行)。
fn draw_wrapped(
    ctx: &CanvasRenderingContext2d,
    items: &[String],
    x: i64,
    y: i64,
    max_w: f64,
    line_h: i64,
    max_lines: usize,
) {
    let mut line = String::new();
    let mut yy = y;
    let mut drawn = 0usize;
    for it in items {
        let candidate = if line.is_empty() {
            it.clone()
        } else {
            format!("{line} / {it}")
        };
        let width = ctx
            .measure_text(&candidate)
            .map(|m| m.width())
            .unwrap_or(0.0);
        if width > max_w && !line.is_empty() {
            let _ = ctx.fill_text(&line, x as f64, yy as f64);
            drawn += 1;
            if drawn >= max_lines {
                let _ = ctx.fill_text("…", (x as f64) + max_w - 8.0, yy as f64);
                return;
            }
            yy += line_h;
            line = it.clone();
        } else {
            line = candidate;
        }
    }
    if !line.is_empty() {
        let _ = ctx.fill_text(&line, x as f64, yy as f64);
    }
}

// ===== VB6 ウィジェット描画ヘルパ / VB6 widget drawing helpers =====

/// VB6 標準の凹型枠（Frame1 など）/ Etched frame.
fn draw_vb6_etched_frame(ctx: &CanvasRenderingContext2d, x: i64, y: i64, w: u32, h: u32) {
    let xf = x as f64;
    let yf = y as f64;
    let wf = f64::from(w);
    let hf = f64::from(h);

    ctx.set_stroke_style_str("#808080");
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(xf, yf + hf);
    ctx.line_to(xf, yf);
    ctx.line_to(xf + wf, yf);
    ctx.stroke();

    ctx.set_stroke_style_str("#ffffff");
    ctx.begin_path();
    ctx.move_to(xf + wf, yf);
    ctx.line_to(xf + wf, yf + hf);
    ctx.line_to(xf, yf + hf);
    ctx.stroke();
}

/// VB6 ダイアログ枠（タイトルバー付き）/ VB6 modal dialog frame with title bar.
fn draw_vb6_dialog(ctx: &CanvasRenderingContext2d, x: i64, y: i64, w: u32, h: u32, caption: &str) {
    let xf = x as f64;
    let yf = y as f64;
    let wf = f64::from(w);
    let hf = f64::from(h);

    // ボディ
    ctx.set_fill_style_str("#c0c0c0");
    ctx.fill_rect(xf, yf, wf, hf);

    // 外側 raised フレーム (上/左 明るく、下/右 暗く)
    ctx.set_stroke_style_str("#ffffff");
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(xf, yf + hf);
    ctx.line_to(xf, yf);
    ctx.line_to(xf + wf, yf);
    ctx.stroke();
    ctx.set_stroke_style_str("#404040");
    ctx.begin_path();
    ctx.move_to(xf + wf, yf);
    ctx.line_to(xf + wf, yf + hf);
    ctx.line_to(xf, yf + hf);
    ctx.stroke();

    // タイトルバー (Windows 9x 風の青)
    let bar_h = 16.0;
    ctx.set_fill_style_str("#000080");
    ctx.fill_rect(xf + 2.0, yf + 2.0, wf - 4.0, bar_h);
    ctx.set_fill_style_str("#ffffff");
    ctx.set_text_align("left");
    ctx.set_text_baseline("middle");
    ctx.set_font(&format!("bold 11px {JP_SANS}"));
    let _ = ctx.fill_text(caption, xf + 6.0, yf + 2.0 + bar_h / 2.0);
}

fn draw_label(ctx: &CanvasRenderingContext2d, ox: i64, oy: i64, c: LabelledControl) {
    ctx.set_fill_style_str("#000");
    ctx.set_text_align("left");
    ctx.set_text_baseline("middle");
    ctx.set_font(&format!("12px {JP_SANS}"));
    let _ = ctx.fill_text(
        c.caption,
        (ox + i64::from(c.bounds.x)) as f64,
        (oy + i64::from(c.bounds.y) + i64::from(c.bounds.h) / 2) as f64,
    );
}

fn draw_checkbox(
    ctx: &CanvasRenderingContext2d,
    ox: i64,
    oy: i64,
    c: LabelledControl,
    checked: bool,
) {
    let x = (ox + i64::from(c.bounds.x)) as f64;
    let y = (oy + i64::from(c.bounds.y) + i64::from(c.bounds.h) / 2 - 6) as f64;
    let size = 12.0;

    // 凹型ボックス
    ctx.set_fill_style_str("#ffffff");
    ctx.fill_rect(x, y, size, size);
    ctx.set_stroke_style_str("#808080");
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(x, y + size);
    ctx.line_to(x, y);
    ctx.line_to(x + size, y);
    ctx.stroke();
    ctx.set_stroke_style_str("#404040");
    ctx.begin_path();
    ctx.move_to(x + size, y);
    ctx.line_to(x + size, y + size);
    ctx.line_to(x, y + size);
    ctx.stroke();

    if checked {
        ctx.set_stroke_style_str("#000");
        ctx.set_line_width(1.5);
        ctx.begin_path();
        ctx.move_to(x + 2.0, y + 6.0);
        ctx.line_to(x + 5.0, y + 9.0);
        ctx.line_to(x + 10.0, y + 3.0);
        ctx.stroke();
    }

    // 右側にラベル
    ctx.set_fill_style_str("#000");
    ctx.set_text_align("left");
    ctx.set_text_baseline("middle");
    ctx.set_font(&format!("12px {JP_SANS}"));
    let _ = ctx.fill_text(c.caption, x + size + 4.0, y + size / 2.0);
}

fn draw_combo(ctx: &CanvasRenderingContext2d, ox: i64, oy: i64, r: Rect, value: &str) {
    let x = (ox + i64::from(r.x)) as f64;
    let y = (oy + i64::from(r.y)) as f64;
    let w = f64::from(r.w);
    let h = f64::from(r.h);

    ctx.set_fill_style_str("#ffffff");
    ctx.fill_rect(x, y, w, h);
    ctx.set_stroke_style_str("#808080");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0);

    // ドロップダウンの三角
    let arrow_x = x + w - h;
    ctx.set_fill_style_str("#c0c0c0");
    ctx.fill_rect(arrow_x, y + 1.0, h - 1.0, h - 2.0);
    ctx.set_fill_style_str("#000");
    ctx.begin_path();
    ctx.move_to(arrow_x + h * 0.3, y + h * 0.4);
    ctx.line_to(arrow_x + h * 0.7, y + h * 0.4);
    ctx.line_to(arrow_x + h * 0.5, y + h * 0.7);
    ctx.close_path();
    ctx.fill();

    ctx.set_fill_style_str("#000");
    ctx.set_text_align("left");
    ctx.set_text_baseline("middle");
    ctx.set_font(&format!("12px {JP_SANS}"));
    let _ = ctx.fill_text(value, x + 4.0, y + h / 2.0);
}

fn draw_text_input(ctx: &CanvasRenderingContext2d, ox: i64, oy: i64, r: Rect, value: &str) {
    let x = (ox + i64::from(r.x)) as f64;
    let y = (oy + i64::from(r.y)) as f64;
    let w = f64::from(r.w);
    let h = f64::from(r.h);

    ctx.set_fill_style_str("#ffffff");
    ctx.fill_rect(x, y, w, h);
    ctx.set_stroke_style_str("#808080");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(x + 0.5, y + 0.5, w - 1.0, h - 1.0);

    ctx.set_fill_style_str("#000");
    ctx.set_text_align("center");
    ctx.set_text_baseline("middle");
    ctx.set_font(&format!("12px {JP_SANS}"));
    let _ = ctx.fill_text(value, x + w / 2.0, y + h / 2.0);
}

fn draw_scrollbar(ctx: &CanvasRenderingContext2d, ox: i64, oy: i64, r: Rect, value: u8) {
    let x = (ox + i64::from(r.x)) as f64;
    let y = (oy + i64::from(r.y)) as f64;
    let w = f64::from(r.w);
    let h = f64::from(r.h);

    // トラック
    ctx.set_fill_style_str("#a0a0a0");
    ctx.fill_rect(x, y, w, h);

    // 両端ボタン
    let btn = h;
    ctx.set_fill_style_str("#c0c0c0");
    ctx.fill_rect(x, y, btn, h);
    ctx.fill_rect(x + w - btn, y, btn, h);
    ctx.set_stroke_style_str("#404040");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(x + 0.5, y + 0.5, btn - 1.0, h - 1.0);
    ctx.stroke_rect(x + w - btn + 0.5, y + 0.5, btn - 1.0, h - 1.0);

    // つまみ
    let track_x = x + btn;
    let track_w = w - 2.0 * btn;
    let thumb_w = (track_w * 0.2).max(8.0);
    let thumb_x = track_x + (track_w - thumb_w) * f64::from(value) / 100.0;
    ctx.set_fill_style_str("#c0c0c0");
    ctx.fill_rect(thumb_x, y, thumb_w, h);
    ctx.stroke_rect(thumb_x + 0.5, y + 0.5, thumb_w - 1.0, h - 1.0);
}

fn draw_button(ctx: &CanvasRenderingContext2d, ox: i64, oy: i64, c: LabelledControl) {
    let x = (ox + i64::from(c.bounds.x)) as f64;
    let y = (oy + i64::from(c.bounds.y)) as f64;
    let w = f64::from(c.bounds.w);
    let h = f64::from(c.bounds.h);

    ctx.set_fill_style_str("#c0c0c0");
    ctx.fill_rect(x, y, w, h);
    // 凸型枠
    ctx.set_stroke_style_str("#ffffff");
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(x, y + h);
    ctx.line_to(x, y);
    ctx.line_to(x + w, y);
    ctx.stroke();
    ctx.set_stroke_style_str("#404040");
    ctx.begin_path();
    ctx.move_to(x + w, y);
    ctx.line_to(x + w, y + h);
    ctx.line_to(x, y + h);
    ctx.stroke();

    ctx.set_fill_style_str("#000");
    ctx.set_text_align("center");
    ctx.set_text_baseline("middle");
    ctx.set_font(&format!("12px {JP_SANS}"));
    let _ = ctx.fill_text(c.caption, x + w / 2.0, y + h / 2.0);
}

fn draw_image_placeholder(
    ctx: &CanvasRenderingContext2d,
    x: i64,
    y: i64,
    w: u32,
    h: u32,
    label: &str,
) {
    let xf = x as f64;
    let yf = y as f64;
    let wf = f64::from(w);
    let hf = f64::from(h);

    ctx.set_fill_style_str("#e8e8e8");
    ctx.fill_rect(xf, yf, wf, hf);
    ctx.set_stroke_style_str("#888");
    ctx.set_line_width(1.0);
    ctx.set_line_dash(&js_sys::Array::of2(&4.0.into(), &3.0.into()).into())
        .ok();
    ctx.stroke_rect(xf + 0.5, yf + 0.5, wf - 1.0, hf - 1.0);
    ctx.set_line_dash(&js_sys::Array::new().into()).ok();

    ctx.set_fill_style_str("#666");
    ctx.set_text_align("center");
    ctx.set_text_baseline("middle");
    ctx.set_font("10px monospace");
    let _ = ctx.fill_text(label, xf + wf / 2.0, yf + hf / 2.0);
}

fn draw_title_logo_text(ctx: &CanvasRenderingContext2d, x: i64, y: i64, w: u32, h: u32) {
    let xf = x as f64;
    let yf = y as f64;
    let wf = f64::from(w);
    let hf = f64::from(h);

    ctx.set_fill_style_str("#1a1a40");
    ctx.fill_rect(xf, yf, wf, hf);

    ctx.set_fill_style_str("#ffd966");
    ctx.set_text_align("center");
    ctx.set_text_baseline("middle");
    ctx.set_font("bold 32px 'Times New Roman', serif");
    let _ = ctx.fill_text("SRC", xf + wf / 2.0, yf + hf / 2.0);
}
