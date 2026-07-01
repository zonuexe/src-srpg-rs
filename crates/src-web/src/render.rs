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
    MAP_VIEW_HEIGHT, MAP_VIEW_WIDTH, STATUS_PANEL_H, STATUS_PANEL_WIDTH, STATUS_PANEL_X,
    STATUS_PANEL_Y, TILE_SIZE, VIEW_TILES_X, VIEW_TILES_Y,
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
    reaction_data: Option<&src_core::scene::reaction::ReactionWindowData>,
    weapon_select_data: Option<&src_core::scene::weapon_select::WeaponSelectWindowData>,
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
                    assets,
                    selected_weapon_idx,
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
                    // オリジナル SRC 風の戦闘ウィンドウ (攻防 2 機の HP/EN + 結果)。
                    draw_combat_window(ctx, database, anim, last_message, assets);
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
    // 反撃手段選択中はオリジナル風の戦闘窓を最優先で描画 (素の Menu より上)。
    if let Some(rd) = reaction_data {
        draw_reaction_window(ctx, database, rd, assets);
        return;
    }
    // 武器選択中も同様に専用窓を描画。
    if let Some(wd) = weapon_select_data {
        draw_weapon_select_window(ctx, database, wd, assets);
        return;
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
    // ペン状態は overlay の永続フィールドから seed する。SRC の ObjColor/ObjFillStyle/
    // ObjFillColor/ObjDrawWidth は ClearPicture を跨いで保持されるため、毎フレーム
    // ClearPicture される戦闘アニメで cmds から SetColor 等が消えても色/塗りが残る
    // (seed しないと既定の白/無塗りに戻ってしまう)。cmds 内の SetXxx は順次上書きする。
    let (mut font_str, mut text_color) = match &overlay.current_font {
        Some((family, size_pt, color)) => {
            let family = if family.is_empty() {
                JP_SANS.to_string()
            } else {
                format!("'{family}', {JP_SANS}")
            };
            (format!("{size_pt}px {family}"), color.clone())
        }
        None => (format!("14px {JP_SANS}"), "#ffffff".to_string()),
    };
    let mut stroke_color = if overlay.current_color.is_empty() {
        "#ffffff".to_string()
    } else {
        overlay.current_color.clone()
    };
    // 図形 (Circle/Oval/Polygon/Arc) の塗り状態。FillStyle / FillColor で更新。
    let mut line_width = if overlay.current_line_width > 0.0 {
        overlay.current_line_width
    } else {
        1.0
    };
    let mut fill_solid = overlay.current_fill_solid;
    let mut fill_color = if overlay.current_fill_color.is_empty() {
        "#000000".to_string()
    } else {
        overlay.current_fill_color.clone()
    };
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
                line_width = *n;
                ctx.set_line_width(*n);
            }
            D::SetFillSolid(solid) => {
                fill_solid = *solid;
            }
            D::SetFillColor { color } => {
                fill_color = color.clone();
            }
            D::Circle { cx, cy, r } => {
                draw_ellipse_overlay(
                    ctx,
                    *cx,
                    *cy,
                    *r,
                    *r,
                    &stroke_color,
                    line_width,
                    fill_solid,
                    &fill_color,
                );
            }
            D::Oval { cx, cy, r, ratio } => {
                draw_ellipse_overlay(
                    ctx,
                    *cx,
                    *cy,
                    *r,
                    *r * *ratio,
                    &stroke_color,
                    line_width,
                    fill_solid,
                    &fill_color,
                );
            }
            D::Arc {
                cx,
                cy,
                r,
                start_deg,
                end_deg,
            } => {
                // SRC: 右向き=0・反時計回り増加 (数学座標系、上向き=90)。
                // Canvas は y 軸が下向きなので、SRC 角 θ の点は canvas 角 -θ に来る。
                // SRC は start→end を CCW(数学) 方向に描くため、canvas 上では角度が
                // 減少する向き = anticlockwise=true で描く (C# DrawArc の負 sweep と同等)。
                let start = -*start_deg * core::f64::consts::PI / 180.0;
                let end = -*end_deg * core::f64::consts::PI / 180.0;
                if fill_solid {
                    ctx.begin_path();
                    ctx.move_to(*cx, *cy);
                    let _ = ctx.arc_with_anticlockwise(*cx, *cy, *r, start, end, true);
                    ctx.close_path();
                    ctx.set_fill_style_str(&fill_color);
                    ctx.fill();
                }
                ctx.begin_path();
                let _ = ctx.arc_with_anticlockwise(*cx, *cy, *r, start, end, true);
                ctx.set_stroke_style_str(&stroke_color);
                ctx.set_line_width(line_width);
                ctx.stroke();
            }
            D::Polygon { points } => {
                if points.len() >= 2 {
                    ctx.begin_path();
                    ctx.move_to(points[0].0, points[0].1);
                    for p in &points[1..] {
                        ctx.line_to(p.0, p.1);
                    }
                    ctx.close_path();
                    if fill_solid {
                        ctx.set_fill_style_str(&fill_color);
                        ctx.fill();
                    }
                    ctx.set_stroke_style_str(&stroke_color);
                    ctx.set_line_width(line_width);
                    ctx.stroke();
                }
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

/// `Circle` / `Oval` の楕円描画。中心 (cx,cy)・横半径 rx・縦半径 ry。
/// 塗り (`fill_solid`) のときは内部を `fill_color` で塗ってから輪郭を `stroke_color` で描く。
#[allow(clippy::too_many_arguments)]
fn draw_ellipse_overlay(
    ctx: &CanvasRenderingContext2d,
    cx: f64,
    cy: f64,
    rx: f64,
    ry: f64,
    stroke_color: &str,
    line_width: f64,
    fill_solid: bool,
    fill_color: &str,
) {
    // 半径が 0 以下 (縦横比 0 の Oval 等) は描画しない (Canvas が例外を投げる)。
    if rx <= 0.0 || ry <= 0.0 {
        return;
    }
    ctx.begin_path();
    let _ = ctx.ellipse(cx, cy, rx, ry, 0.0, 0.0, core::f64::consts::TAU);
    if fill_solid {
        ctx.set_fill_style_str(fill_color);
        ctx.fill();
    }
    ctx.set_stroke_style_str(stroke_color);
    ctx.set_line_width(line_width);
    ctx.stroke();
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

/// 戦闘ウィンドウに表示する 1 機ぶんのスナップショット。
struct CombatantHud {
    /// パイロット顔グラのヒント (bitmap → nickname)。
    face: String,
    /// 機体スプライトのヒント (UnitData.bitmap → unit_data_name)。オリジナル
    /// 戦闘窓は顔グラの隣に機体アイコンも並べる。
    sprite: String,
    /// パイロット愛称 (反撃窓の「機体 パイロット」行用)。
    name: String,
    /// 機体名 (同上)。
    unit: String,
    level: i32,
    morale: i32,
    hp_cur: f64,
    hp_max: f64,
    en_cur: f64,
    en_max: f64,
}

/// タイル位置から戦闘 HUD を解決する (顔・Lv・気力・HP/EN・名前)。盤上に
/// ユニットが居なければ `None` (撃破され除去された側など)。
fn resolve_combatant_hud(database: &GameDatabase, pos: (u32, u32)) -> Option<CombatantHud> {
    let u = database.units_at(pos.0, pos.1).next()?;
    let hp_max = database.effective_max_hp(u) as f64;
    let hp_cur = (hp_max - u.displayed_damage).max(0.0);
    let en_max = database.effective_max_en(u);
    let en_cur = (en_max - u.en_consumed).max(0);
    let mp = u.main_pilot_name();
    let pilot = if mp.is_empty() {
        None
    } else {
        database.effective_pilot_data(mp)
    };
    let pilot_inst = u
        .pilot_ids
        .first()
        .and_then(|id| database.pilot_instance_by_id(id));
    let level = pilot_inst.map(|p| p.level).unwrap_or(1);
    let name = pilot
        .as_ref()
        .map(|p| p.nickname.clone())
        .unwrap_or_default();
    let face = pilot
        .as_ref()
        .and_then(|p| p.bitmap.clone())
        .or_else(|| pilot.as_ref().map(|p| p.nickname.clone()))
        .unwrap_or_default();
    // 機体スプライト: UnitData.bitmap があれば優先、無ければ unit_data_name
    // (マップ描画と同じ解決順)。
    let unit_data = database.unit_by_name(&u.unit_data_name);
    let sprite = unit_data
        .as_ref()
        .map(|d| d.bitmap.clone())
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| u.unit_data_name.clone());
    Some(CombatantHud {
        face,
        sprite,
        name,
        unit: unit_data.map(|d| d.name.clone()).unwrap_or_default(),
        level,
        morale: u.morale,
        hp_cur,
        hp_max,
        en_cur: en_cur as f64,
        en_max: en_max as f64,
    })
}

/// HP/EN の値テキスト + ゲージ (オリジナル SRC `Status.bas` 準拠)。値を上、
/// ゲージを直下に描く。ゲージは「赤地に緑の現在値 + 沈み込みベベル枠」で、
/// 原典の `upic.Line ... rgb(0,210,0)/rgb(200,0,0)` と枠 rgb(100,100,100)/
/// rgb(220,220,220) を再現する (減った分が赤く見える。緑→黄→赤の変化はしない)。
#[allow(clippy::too_many_arguments)]
fn draw_combat_bar(
    ctx: &CanvasRenderingContext2d,
    x: f64,
    y: f64,
    w: f64,
    label: &str,
    cur: f64,
    max: f64,
    dark: bool,
) {
    // ラベル「ＨＰ/ＥＮ」= 青、数値 = 黒 (原典 rgb(0,0,150) / rgb(0,0,0))。
    // 暗背景の戦闘会話窓では視認性のため明色に振る。
    let (label_col, value_col) = if dark {
        ("#8fd6e6", "#f2f2f2")
    } else {
        ("#000096", "#101010")
    };
    ctx.set_font(&format!("10px {JP_SANS}"));
    ctx.set_text_align("left");
    ctx.set_text_baseline("top");
    ctx.set_fill_style_str(label_col);
    let _ = ctx.fill_text(label, x, y);
    let label_w = ctx.measure_text(label).map(|m| m.width()).unwrap_or(20.0) + 4.0;
    ctx.set_fill_style_str(value_col);
    let _ = ctx.fill_text(
        &format!("{}/{}", cur.round() as i64, max.round() as i64),
        x + label_w,
        y,
    );

    // ゲージ (枠 7px / 塗り高 5px)。
    let gy = y + 11.0;
    let gh = 6.0;
    let frac = if max <= 0.0 {
        0.0
    } else {
        (cur / max).clamp(0.0, 1.0)
    };
    // 赤背景 → 緑の現在値。
    ctx.set_fill_style_str("#c80000");
    ctx.fill_rect(x + 1.0, gy + 1.0, w - 1.0, gh - 1.0);
    ctx.set_fill_style_str("#00d200");
    ctx.fill_rect(x + 1.0, gy + 1.0, (w - 1.0) * frac, gh - 1.0);
    // 沈み込みベベル枠: 上/左 = 暗灰、下/右 = 明灰。
    ctx.set_line_width(1.0);
    ctx.set_stroke_style_str("#646464");
    ctx.begin_path();
    ctx.move_to(x + 0.5, gy + gh - 0.5);
    ctx.line_to(x + 0.5, gy + 0.5);
    ctx.line_to(x + w - 0.5, gy + 0.5);
    ctx.stroke();
    ctx.set_stroke_style_str("#dcdcdc");
    ctx.begin_path();
    ctx.move_to(x + w - 0.5, gy + 0.5);
    ctx.line_to(x + w - 0.5, gy + gh - 0.5);
    ctx.line_to(x + 0.5, gy + gh - 0.5);
    ctx.stroke();
}

/// 戦闘窓ヘッダの 1 機分を描く。`with_pilot` で 2 種のレイアウトを切り替える:
/// - `true` (武器選択 / 反撃窓): `[パイロット顔] [Lv / 気力] [機体スプライト] [HP / EN]`
/// - `false` (戦闘会話メッセージ窓): `[機体スプライト] [HP / EN]` のみ (顔・Lv/気力なし。
///   発話者の顔は窓下段のポートレートにのみ出る)。
///
/// 画像 (顔・機体) が無いスロットは灰塗りせず薄枠のみ (原典では敵の「怪」等も画像で、
/// 素材未配置時に文字代替はしない方針)。
#[allow(clippy::too_many_arguments)]
fn draw_combatant_hud(
    ctx: &CanvasRenderingContext2d,
    x: f64,
    top: f64,
    block_w: f64,
    hud: &CombatantHud,
    assets: &Assets,
    with_pilot: bool,
    dark: bool,
) {
    let fs = 32.0;
    let gap = 5.0;
    let sprite_img = if hud.sprite.is_empty() {
        None
    } else {
        assets.find_image(&hud.sprite)
    };
    // 暗背景 (戦闘会話窓) では枠・文字を明色に。
    let frame_col = if dark { "#6b7a92" } else { "#b9b6a3" };
    let lv_col = if dark { "#cfe0ff" } else { "#000080" };

    // 画像スロット (画像があれば描画、無ければ薄い空枠のみ)。
    let draw_slot = |img: Option<&web_sys::HtmlImageElement>, ix: f64| {
        if let Some(im) = img {
            let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(im, ix, top, fs, fs);
            ctx.set_stroke_style_str(if dark { "#20242c" } else { "#404040" });
        } else {
            ctx.set_stroke_style_str(frame_col);
        }
        ctx.set_line_width(1.0);
        ctx.stroke_rect(ix + 0.5, top + 0.5, fs, fs);
    };

    let mut cur_x = x;
    if with_pilot {
        // 1) パイロット顔。
        let face_img = if hud.face.is_empty() {
            None
        } else {
            assets.find_image(&hud.face)
        };
        draw_slot(face_img, cur_x);
        cur_x += fs + gap;
        // 2) Lv / 気力 (2 行)。
        ctx.set_fill_style_str(lv_col);
        ctx.set_font(&format!("bold 11px {JP_SANS}"));
        ctx.set_text_align("left");
        ctx.set_text_baseline("top");
        let _ = ctx.fill_text(&format!("Lv{}", hud.level), cur_x, top + 3.0);
        let _ = ctx.fill_text(&format!("M{}", hud.morale), cur_x, top + 18.0);
        cur_x += 32.0; // Lv/気力 列幅
    }

    // 3) 機体スプライト。
    draw_slot(sprite_img, cur_x);
    cur_x += fs + gap;

    // 4) HP / EN (数値 + 緑バー) を 2 行。
    let bw = (x + block_w) - cur_x;
    draw_combat_bar(
        ctx,
        cur_x,
        top + 2.0,
        bw,
        "ＨＰ",
        hud.hp_cur,
        hud.hp_max,
        dark,
    );
    draw_combat_bar(
        ctx,
        cur_x,
        top + 20.0,
        bw,
        "ＥＮ",
        hud.en_cur,
        hud.en_max,
        dark,
    );
}

/// 戦闘演出中に重ねる戦闘会話 (メッセージ) ウィンドウ。非戦闘の Talk ダイアログと
/// 同じ暗色パネル + 金枠・同サイズ (画面下部) に統一し、Windows 風 VB6 デザインは廃止。
/// 上段に攻防 2 機の HUD (機体アイコン + HP/EN)、下段に発話者 (攻撃側) の顔・名前・
/// メッセージを表示する。データは `battle_anim` の攻撃側 / 防御側タイルから live
/// `database` を引いて解決する (撃破され盤面から除去された側は欠落 → 非表示)。
fn draw_combat_window(
    ctx: &CanvasRenderingContext2d,
    database: &GameDatabase,
    anim: &src_core::BattleAnim,
    last_message: Option<&str>,
    assets: &Assets,
) {
    let resolve = |pos: (u32, u32)| resolve_combatant_hud(database, pos);
    let atk = resolve(anim.attacker);
    let def = resolve(anim.defender);
    if atk.is_none() && def.is_none() {
        return;
    }

    let cw = f64::from(CANVAS_WIDTH);
    let ch = f64::from(CANVAS_HEIGHT);
    // Talk ダイアログと同じジオメトリ (画面下 ~45%、全幅、暗色パネル + 金枠)。
    let win_top = ch * 0.55;
    let win_h = ch - win_top - 6.0;
    let pad = 12.0;
    ctx.set_fill_style_str("rgba(0,0,0,0.30)");
    ctx.fill_rect(0.0, 0.0, cw, ch);
    ctx.set_fill_style_str("rgba(20,24,40,0.92)");
    ctx.fill_rect(6.0, win_top, cw - 12.0, win_h);
    ctx.set_stroke_style_str("#f0e68c");
    ctx.set_line_width(2.0);
    ctx.stroke_rect(6.0, win_top, cw - 12.0, win_h);

    // 上段: 攻防 2 機の HUD (機体アイコン + HP/EN のみ、暗背景用の明色)。
    let block_w = (cw - 12.0 - pad * 3.0) / 2.0;
    let hud_top = win_top + pad;
    if let Some(a) = atk.as_ref() {
        draw_combatant_hud(ctx, 6.0 + pad, hud_top, block_w, a, assets, false, true);
    }
    if let Some(d) = def.as_ref() {
        draw_combatant_hud(
            ctx,
            6.0 + pad * 2.0 + block_w,
            hud_top,
            block_w,
            d,
            assets,
            false,
            true,
        );
    }

    // 区切り線。
    let div_y = hud_top + 44.0;
    ctx.set_stroke_style_str("#4a5068");
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(6.0 + pad, div_y + 0.5);
    ctx.line_to(cw - 6.0 - pad, div_y + 0.5);
    ctx.stroke();

    // 下段: 発話者 (攻撃側) の顔 + 名前 + メッセージ (Talk 風)。
    let sy = div_y + pad;
    let face_x = 6.0 + pad;
    let face_size = ((win_top + win_h - pad) - sy).clamp(48.0, 88.0);
    ctx.set_stroke_style_str("#a8b8d0");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(face_x, sy, face_size, face_size);
    if let Some(a) = atk.as_ref() {
        if let Some(img) = assets.find_image(&a.face) {
            let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
                img, face_x, sy, face_size, face_size,
            );
        }
    }

    let text_x = face_x + face_size + pad;
    let mut ty = sy;
    ctx.set_text_align("left");
    ctx.set_text_baseline("top");
    if let Some(a) = atk.as_ref() {
        if !a.name.is_empty() {
            ctx.set_fill_style_str("#ffffff");
            ctx.set_font(&format!("bold 15px {JP_SANS}"));
            let _ = ctx.fill_text(&a.name, text_x, ty);
            ty += 22.0;
        }
    }
    if let Some(msg) = last_message {
        ctx.set_fill_style_str("#e6e6f0");
        ctx.set_font(&format!("14px {JP_SANS}"));
        for line in wrap_text(msg, 46) {
            let _ = ctx.fill_text(&line, text_x, ty);
            ty += 19.0;
            if ty > win_top + win_h - 18.0 {
                break;
            }
        }
    }
}

/// 反撃手段選択ウィンドウ (オリジナル SRC 戦闘窓風)。中央寄せの明色 VB6 窓に、
/// タイトル (反撃：<武器> 攻撃力=N 命中率=N%) + 攻防 2 機の HUD + 防御側
/// 「機体 パイロット」+ 各選択肢 (反撃/回避/防御/援護防御) を命中率付きで描く。
/// 選択肢行のジオメトリは src-core `dialog::reaction_choice_at` と共有する。
fn draw_reaction_window(
    ctx: &CanvasRenderingContext2d,
    database: &GameDatabase,
    data: &src_core::scene::reaction::ReactionWindowData,
    assets: &Assets,
) {
    use src_core::dialog::{
        reaction_win_x, REACTION_OPT_H, REACTION_OPT_TOP, REACTION_PAD, REACTION_WIN_H,
        REACTION_WIN_W, REACTION_WIN_Y,
    };
    let cw = f64::from(CANVAS_WIDTH);
    let ch = f64::from(CANVAS_HEIGHT);
    // 半透明バックドロップ。
    ctx.set_fill_style_str("rgba(0,0,0,0.30)");
    ctx.fill_rect(0.0, 0.0, cw, ch);

    let wx = reaction_win_x();
    let wy = REACTION_WIN_Y;
    let ww = REACTION_WIN_W;
    let wh = REACTION_WIN_H;
    draw_vb6_dialog(
        ctx,
        wx as i64,
        wy as i64,
        ww as u32,
        wh as u32,
        &format!(
            "反撃：{} 攻撃力={} 命中率={}%",
            truncate(&data.weapon, 8),
            data.power,
            data.base_hit
        ),
    );

    let pad = REACTION_PAD;
    // 2 機ヘッダ (攻撃側 左 / 防御側 右)。
    let hud_top = wy + 22.0;
    let block_w = (ww - pad * 3.0) / 2.0;
    if let Some(a) = resolve_combatant_hud(database, data.attacker) {
        draw_combatant_hud(ctx, wx + pad, hud_top, block_w, &a, assets, true, false);
    }
    let def_hud = resolve_combatant_hud(database, data.defender);
    if let Some(d) = def_hud.as_ref() {
        draw_combatant_hud(
            ctx,
            wx + pad * 2.0 + block_w,
            hud_top,
            block_w,
            d,
            assets,
            true,
            false,
        );
        // 防御側「機体 パイロット」行。
        ctx.set_fill_style_str("#101010");
        ctx.set_font(&format!("bold 12px {JP_SANS}"));
        ctx.set_text_align("left");
        ctx.set_text_baseline("top");
        let _ = ctx.fill_text(&format!("{} {}", d.unit, d.name), wx + pad, wy + 74.0);
    }

    // 選択肢 (反撃/回避/防御/援護防御) + 命中率。クリック判定と同一ジオメトリ。
    let opt_top = wy + REACTION_OPT_TOP;
    ctx.set_text_baseline("middle");
    for (i, opt) in data.options.iter().take(6).enumerate() {
        let oy = opt_top + i as f64 * REACTION_OPT_H;
        let mid = oy + REACTION_OPT_H / 2.0;
        // 先頭 (既定=反撃) の行をうっすらハイライト。
        if i == 0 {
            ctx.set_fill_style_str("rgba(40,90,200,0.12)");
            ctx.fill_rect(wx + pad - 2.0, oy, ww - pad * 2.0 + 4.0, REACTION_OPT_H);
        }
        ctx.set_text_align("left");
        ctx.set_fill_style_str("#1840c0");
        ctx.set_font(&format!("bold 13px {JP_SANS}"));
        let _ = ctx.fill_text(&format!("[{}]", i + 1), wx + pad + 4.0, mid);
        ctx.set_fill_style_str("#101010");
        ctx.set_font(&format!("13px {JP_SANS}"));
        let _ = ctx.fill_text(&opt.label, wx + pad + 38.0, mid);
        ctx.set_text_align("right");
        ctx.set_fill_style_str("#0a6a78");
        let _ = ctx.fill_text(&format!("命中 {}%", opt.hit_pct), wx + ww - pad - 4.0, mid);
    }

    ctx.set_text_align("right");
    ctx.set_text_baseline("bottom");
    ctx.set_fill_style_str("#6a6a60");
    ctx.set_font(&format!("11px {JP_SANS}"));
    let _ = ctx.fill_text("番号キー / クリックで選択", wx + ww - pad, wy + wh - 6.0);
}

/// 武器選択ウィンドウ (オリジナル SRC 武器選択窓風)。中央寄せの明色 VB6 窓に、
/// タイトル + 攻防 2 機の HUD + 武器表 (名称/攻撃/命中/CT/弾EN/適応/分類)。
/// ×=使用不可 (グレー)。行のジオメトリは src-core `dialog::weapon_select_choice_at`
/// と共有する。
fn draw_weapon_select_window(
    ctx: &CanvasRenderingContext2d,
    database: &GameDatabase,
    data: &src_core::scene::weapon_select::WeaponSelectWindowData,
    assets: &Assets,
) {
    use src_core::dialog::{
        weapon_win_x, WEAPON_PAD, WEAPON_ROW_H, WEAPON_ROW_TOP, WEAPON_WIN_H, WEAPON_WIN_W,
        WEAPON_WIN_Y,
    };
    let cw = f64::from(CANVAS_WIDTH);
    let ch = f64::from(CANVAS_HEIGHT);
    ctx.set_fill_style_str("rgba(0,0,0,0.30)");
    ctx.fill_rect(0.0, 0.0, cw, ch);

    let wx = weapon_win_x();
    let wy = WEAPON_WIN_Y;
    let ww = WEAPON_WIN_W;
    let wh = WEAPON_WIN_H;
    let title = if data.is_counter {
        "反撃武器選択"
    } else {
        "武器選択"
    };
    draw_vb6_dialog(ctx, wx as i64, wy as i64, ww as u32, wh as u32, title);

    let pad = WEAPON_PAD;
    // 上段: 攻防 2 機の HUD。
    let hud_top = wy + 22.0;
    let block_w = (ww - pad * 3.0) / 2.0;
    if let Some(a) = resolve_combatant_hud(database, data.attacker) {
        draw_combatant_hud(ctx, wx + pad, hud_top, block_w, &a, assets, true, false);
    }
    if let Some(d) = resolve_combatant_hud(database, data.defender) {
        draw_combatant_hud(
            ctx,
            wx + pad * 2.0 + block_w,
            hud_top,
            block_w,
            &d,
            assets,
            true,
            false,
        );
    }

    // 列 X (名称=左, 数値=右寄せ, 適応/分類=左)。
    let col_name = wx + 14.0;
    let col_pow = wx + 318.0;
    let col_hit = wx + 380.0;
    let col_ct = wx + 428.0;
    let col_ammo = wx + 502.0;
    let col_adp = wx + 512.0;
    let col_cls = wx + 556.0;

    // 表ヘッダ。
    let hdr_y = wy + 74.0;
    ctx.set_font(&format!("bold 11px {JP_SANS}"));
    ctx.set_fill_style_str("#13409a");
    ctx.set_text_baseline("top");
    ctx.set_text_align("left");
    let _ = ctx.fill_text("名称", col_name, hdr_y);
    let _ = ctx.fill_text("適応", col_adp, hdr_y);
    let _ = ctx.fill_text("分類", col_cls, hdr_y);
    ctx.set_text_align("right");
    let _ = ctx.fill_text("攻撃", col_pow, hdr_y);
    let _ = ctx.fill_text("命中", col_hit, hdr_y);
    let _ = ctx.fill_text("CT", col_ct, hdr_y);
    let _ = ctx.fill_text("弾/EN", col_ammo, hdr_y);
    // ヘッダ下の区切り線。
    ctx.set_stroke_style_str("#b9b6a3");
    ctx.set_line_width(1.0);
    ctx.begin_path();
    ctx.move_to(wx + pad, hdr_y + 16.0);
    ctx.line_to(wx + ww - pad, hdr_y + 16.0);
    ctx.stroke();

    // 武器行。
    let row_top = wy + WEAPON_ROW_TOP;
    ctx.set_font(&format!("12px {JP_SANS}"));
    ctx.set_text_baseline("middle");
    for (i, r) in data.rows.iter().take(9).enumerate() {
        let ry = row_top + i as f64 * WEAPON_ROW_H;
        let mid = ry + WEAPON_ROW_H / 2.0;
        let col = if r.usable { "#101010" } else { "#9a988c" };
        ctx.set_fill_style_str(col);
        ctx.set_text_align("left");
        // 使用不可は名称頭に × (オリジナル準拠)。
        let name = if r.usable {
            format!(" {}", truncate(&r.name, 12))
        } else {
            format!("×{}", truncate(&r.name, 12))
        };
        let _ = ctx.fill_text(&name, col_name, mid);
        let _ = ctx.fill_text(&truncate(&r.adaption, 4), col_adp, mid);
        let _ = ctx.fill_text(&truncate(&r.class, 6), col_cls, mid);
        ctx.set_text_align("right");
        let _ = ctx.fill_text(&r.power.to_string(), col_pow, mid);
        let _ = ctx.fill_text(&format!("{}%", r.hit_pct), col_hit, mid);
        let ct = if r.critical > 0 {
            format!("{}%", r.critical)
        } else {
            "-".to_string()
        };
        let _ = ctx.fill_text(&ct, col_ct, mid);
        let _ = ctx.fill_text(&r.ammo, col_ammo, mid);
    }

    ctx.set_text_align("right");
    ctx.set_text_baseline("bottom");
    ctx.set_fill_style_str("#6a6a60");
    ctx.set_font(&format!("11px {JP_SANS}"));
    let _ = ctx.fill_text(
        "番号キー / クリックで選択   右クリックで戻る",
        wx + ww - pad,
        wy + wh - 6.0,
    );
}

#[allow(clippy::too_many_arguments)]
fn draw_map_view(
    ctx: &CanvasRenderingContext2d,
    database: &GameDatabase,
    cursor: Option<(u32, u32)>,
    turn: Turn,
    scroll: (u32, u32),
    stage: &str,
    assets: &Assets,
    selected_weapon_idx: usize,
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

        // サイドバーのメッセージボックスは廃止 (メッセージは Talk / 戦闘窓に集約)。
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
    _scroll: (u32, u32),
    assets: &Assets,
    selected_weapon_idx: usize,
) {
    let w = f64::from(STATUS_PANEL_WIDTH);
    let h = f64::from(STATUS_PANEL_H);

    // ===== オリジナル SRC (Status.cs) 準拠の配色 =====
    // 背景=明グレー / ラベル=シアン / 値=黒 / 能力名=青 / 無効武器=暗赤 / バー=緑。
    const BG: &str = "#ece9d8";
    const LABEL: &str = "#0a7e8c"; // シアン系の項目ラベル
    const VALUE: &str = "#101010"; // 値・名前 (黒)
    const ABILITY: &str = "#000096"; // 特殊能力名 (青) — StatusFontColorAbilityName
    const DISABLED: &str = "#960000"; // 使用不可武器 (暗赤) — StatusFontColorAbilityDisable
    const HEADER: &str = "#13409a"; // 武器列ヘッダ (青)
    const MUTED: &str = "#76736a";

    // 背景 + VB6 風の凹枠 (外=暗シャドウ / 内=明ハイライト)。
    ctx.set_fill_style_str(BG);
    ctx.fill_rect(ox as f64, oy as f64, w, h);
    ctx.set_stroke_style_str("#ffffff");
    ctx.set_line_width(1.0);
    ctx.stroke_rect(ox as f64 + 1.5, oy as f64 + 1.5, w - 3.0, h - 3.0);
    ctx.set_stroke_style_str("#82806f");
    ctx.stroke_rect(ox as f64 + 0.5, oy as f64 + 0.5, w - 1.0, h - 1.0);

    let pad_x = ox as f64 + 6.0;
    let right = ox as f64 + w - 6.0;
    let mut y = oy as f64 + 4.0;
    ctx.set_text_align("left");
    ctx.set_text_baseline("top");

    // ラベル(シアン)+値(黒) を 1 行に描く小ヘルパ。フォントは呼び出し前に設定する。
    let lv =
        |ctx: &CanvasRenderingContext2d, x: f64, y: f64, label: &str, gap: f64, value: &str| {
            ctx.set_text_align("left");
            ctx.set_fill_style_str(LABEL);
            let _ = ctx.fill_text(label, x, y);
            ctx.set_fill_style_str(VALUE);
            let _ = ctx.fill_text(value, x + gap, y);
        };

    // --- ターン / フェーズ (控えめな見出し行) ---
    ctx.set_fill_style_str(MUTED);
    ctx.set_font(&format!("10px {JP_SANS}"));
    let _ = ctx.fill_text(
        &format!("T{} {}", turn.number, turn.phase.label()),
        pad_x,
        y,
    );
    if !stage.is_empty() {
        ctx.set_text_align("right");
        let _ = ctx.fill_text(&truncate(stage, 9), right, y);
        ctx.set_text_align("left");
    }
    y += 14.0;

    let Some((cx, cy)) = cursor else {
        ctx.set_fill_style_str(MUTED);
        ctx.set_font(&format!("italic 11px {JP_SANS}"));
        let _ = ctx.fill_text("カーソルなし", pad_x, y + 4.0);
        return;
    };

    let unit_inst = database.units_at(cx, cy).next();
    let Some(u) = unit_inst else {
        // ユニットがいないマス: 地形のみ表示。
        if let Some(map) = database.map.as_ref() {
            if cx < map.width && cy < map.height {
                if let Some(t) = terrain::lookup(map.cell(cx, cy).terrain_id) {
                    ctx.set_font(&format!("11px {JP_SANS}"));
                    lv(
                        ctx,
                        pad_x,
                        y,
                        "地形",
                        34.0,
                        &format!("{} ({},{})", t.name, cx, cy),
                    );
                    y += 16.0;
                    lv(
                        ctx,
                        pad_x,
                        y,
                        "移動",
                        34.0,
                        &format!("{}  回避 {:+}%", t.move_cost, t.hit_mod),
                    );
                }
            }
        }
        return;
    };

    let unit_def = database.unit_by_name(&u.unit_data_name);
    let main_pilot = u.main_pilot_name();
    let pilot_data = if main_pilot.is_empty() {
        None
    } else {
        database.effective_pilot_data(main_pilot)
    };
    let pilot_inst = u
        .pilot_ids
        .first()
        .and_then(|id| database.pilot_instance_by_id(id));

    // ===== パイロットブロック =====
    let face = 42.0;
    let face_top = y;
    if let Some(pd) = pilot_data.as_ref() {
        let hint = pd.bitmap.clone().unwrap_or_else(|| pd.nickname.clone());
        match assets.find_image(&hint) {
            Some(img) => {
                let _ = ctx.draw_image_with_html_image_element_and_dw_and_dh(
                    img, pad_x, face_top, face, face,
                );
            }
            None => {
                ctx.set_fill_style_str("#c9c6b5");
                ctx.fill_rect(pad_x, face_top, face, face);
            }
        }
        ctx.set_stroke_style_str("#82806f");
        ctx.set_line_width(1.0);
        ctx.stroke_rect(pad_x + 0.5, face_top + 0.5, face, face);
    }

    // 顔グラ右: 名前 / レベル / 気力 / ＳＰ
    let tx = pad_x + face + 5.0;
    let mut py = face_top;
    ctx.set_fill_style_str(VALUE);
    ctx.set_font(&format!("bold 12px {JP_SANS}"));
    let pname = pilot_data
        .as_ref()
        .map(|p| p.nickname.clone())
        .unwrap_or_else(|| "(無人)".to_string());
    let _ = ctx.fill_text(&truncate(&pname, 10), tx, py);
    py += 13.0;
    ctx.set_font(&format!("10px {JP_SANS}"));
    // レベル (撃墜数)。撃墜数は PilotInstance.skills の「撃墜数 N」(increment_kill_count
    // が書き戻す) を skill_level で読む。未撃破なら 0。
    let level = pilot_inst.map(|p| p.level).unwrap_or(1);
    let kills = pilot_inst.map(|p| p.skill_level("撃墜数")).unwrap_or(0);
    lv(ctx, tx, py, "Lv", 22.0, &format!("{level} ({kills})"));
    py += 12.0;
    // 気力 + 性格 (例: 100 (強気))。
    let personality = pilot_data.as_ref().and_then(|p| p.personality.clone());
    let morale_text = match personality.as_deref() {
        Some(per) if !per.is_empty() => format!("{} ({per})", u.morale),
        _ => u.morale.to_string(),
    };
    lv(ctx, tx, py, "気力", 24.0, &morale_text);
    py += 12.0;
    if let Some(pd) = pilot_data.as_ref() {
        let sp_max = pd.sp.unwrap_or(0);
        if sp_max > 0 {
            let sp_cur = pilot_inst.map(|p| p.sp_remaining).unwrap_or(sp_max);
            lv(ctx, tx, py, "SP", 22.0, &format!("{sp_cur}/{sp_max}"));
            py += 12.0;
        }
    }
    y = (face_top + face).max(py) + 4.0;

    // パイロット能力値 (2 列)。
    if let Some(pd) = pilot_data.as_ref() {
        ctx.set_font(&format!("10px {JP_SANS}"));
        let col2 = pad_x + (w - 12.0) / 2.0;
        lv(ctx, pad_x, y, "格闘", 28.0, &pd.infight.to_string());
        lv(ctx, col2, y, "射撃", 28.0, &pd.shooting.to_string());
        y += 12.0;
        lv(ctx, pad_x, y, "命中", 28.0, &pd.hit.to_string());
        lv(ctx, col2, y, "回避", 28.0, &pd.dodge.to_string());
        y += 12.0;
        lv(ctx, pad_x, y, "技量", 28.0, &pd.technique.to_string());
        lv(ctx, col2, y, "反応", 28.0, &pd.intuition.to_string());
        y += 13.0;

        // 特殊能力 (スペシャルパワー + 技能) — 能力名は青。
        let mut abilities: Vec<String> = pd.features.iter().map(|(n, _)| n.clone()).collect();
        if let Some(pi) = pilot_inst {
            for s in &pi.skills {
                if !abilities.iter().any(|a| a == s) {
                    abilities.push(s.clone());
                }
            }
        }
        if !abilities.is_empty() {
            ctx.set_fill_style_str(ABILITY);
            ctx.set_font(&format!("10px {JP_SANS}"));
            for line in wrap_text(&abilities.join(" "), 50).into_iter().take(2) {
                let _ = ctx.fill_text(&line, pad_x, y);
                y += 12.0;
            }
        }
        // 霊力 (plana)。
        if let Some(pi) = pilot_inst {
            if pi.plana > 0 {
                ctx.set_font(&format!("10px {JP_SANS}"));
                lv(ctx, pad_x, y, "霊力", 28.0, &pi.plana.to_string());
                y += 12.0;
            }
        }
    }

    // 区切り線。
    let divider = |ctx: &CanvasRenderingContext2d, y: f64| {
        ctx.set_stroke_style_str("#b9b6a3");
        ctx.set_line_width(1.0);
        ctx.begin_path();
        ctx.move_to(pad_x, y + 0.5);
        ctx.line_to(right, y + 0.5);
        ctx.stroke();
    };
    y += 2.0;
    divider(ctx, y);
    y += 5.0;

    // ===== 機体ブロック =====
    if let Some(d) = unit_def {
        ctx.set_fill_style_str(VALUE);
        ctx.set_font(&format!("bold 11px {JP_SANS}"));
        let _ = ctx.fill_text(&truncate(&d.name, 20), pad_x, y);
        y += 13.0;
        // 現在地形 + 回避補正。
        if let Some(map) = database.map.as_ref() {
            if cx < map.width && cy < map.height {
                let tid = map.cell(cx, cy).terrain_id;
                if let Some(t) = terrain::lookup(tid) {
                    let hit = database.terrain_hit_mod(tid);
                    let eff = if hit != 0 {
                        format!("  回避{hit:+}%")
                    } else {
                        String::new()
                    };
                    ctx.set_fill_style_str(LABEL);
                    ctx.set_font(&format!("10px {JP_SANS}"));
                    let _ = ctx.fill_text(&format!("{}{}", t.name, eff), pad_x, y);
                    y += 13.0;
                }
            }
        }
        // HP / EN (ラベル+値 + 緑バー)。
        let bar = |ctx: &CanvasRenderingContext2d, y: f64, label: &str, cur: f64, max: f64| {
            ctx.set_font(&format!("10px {JP_SANS}"));
            lv(
                ctx,
                pad_x,
                y,
                label,
                26.0,
                &format!("{}/{}", cur.round() as i64, max.round() as i64),
            );
            let by = y + 11.0;
            let bw = w - 12.0;
            let bh = 4.0;
            ctx.set_fill_style_str("#cfccba");
            ctx.fill_rect(pad_x, by, bw, bh);
            let frac = if max <= 0.0 {
                0.0
            } else {
                (cur / max).clamp(0.0, 1.0)
            };
            ctx.set_fill_style_str("#1c9e3a");
            ctx.fill_rect(pad_x, by, bw * frac, bh);
            ctx.set_stroke_style_str("#82806f");
            ctx.set_line_width(1.0);
            ctx.stroke_rect(pad_x + 0.5, by + 0.5, bw - 1.0, bh - 1.0);
        };
        let hp_max = database.effective_max_hp(u);
        let hp_cur = (hp_max as f64 - u.displayed_damage).max(0.0);
        let en_max = database.effective_max_en(u);
        let en_cur = (en_max - u.en_consumed).max(0);
        bar(ctx, y, "ＨＰ", hp_cur, hp_max as f64);
        y += 17.0;
        bar(ctx, y, "ＥＮ", en_cur as f64, en_max as f64);
        y += 17.0;
        // 装甲 / 運動性 / 移動力 / サイズ / 適応。
        ctx.set_font(&format!("10px {JP_SANS}"));
        let col2 = pad_x + (w - 12.0) / 2.0;
        // 装甲は base+bonus 表示 (改造段階・強化パーツ・ボス補正の上乗せ分を分割)。
        let armor_eff = database.effective_armor(u);
        let armor_bonus = armor_eff - d.armor;
        let armor_text = if armor_bonus > 0 {
            format!("{}+{}", d.armor, armor_bonus)
        } else {
            armor_eff.to_string()
        };
        lv(ctx, pad_x, y, "装甲", 28.0, &armor_text);
        lv(
            ctx,
            col2,
            y,
            "運動",
            28.0,
            &database.effective_mobility(u).to_string(),
        );
        y += 12.0;
        // タイプ (移動形態): base transportation + 移動系特殊能力の追加 (SRC 準拠)。
        let move_type = src_core::data::unit::move_type_label(&d.transportation, |n| {
            d.features.iter().any(|(f, _)| f == n) || pilot_inst.is_some_and(|p| p.has_skill(n))
        });
        lv(ctx, pad_x, y, "ﾀｲﾌﾟ", 28.0, &move_type);
        lv(
            ctx,
            col2,
            y,
            "移動",
            28.0,
            &database.effective_speed(u).to_string(),
        );
        y += 12.0;
        lv(ctx, pad_x, y, "適応", 28.0, d.adaption.as_str());
        lv(ctx, col2, y, "ｻｲｽﾞ", 28.0, d.size.label());
        y += 13.0;
        // 機体特殊能力 (例: 霊力変換器) — 能力名青。
        let ufeatures: Vec<String> = d.features.iter().map(|(n, _)| n.clone()).collect();
        if !ufeatures.is_empty() {
            ctx.set_fill_style_str(ABILITY);
            ctx.set_font(&format!("10px {JP_SANS}"));
            for line in wrap_text(&ufeatures.join(" "), 50).into_iter().take(2) {
                let _ = ctx.fill_text(&line, pad_x, y);
                y += 12.0;
            }
        }
    }

    // ===== 武器ブロック (攻撃 / 射程 を右寄せ 2 カラム) =====
    // 戦闘予測行はパネル末尾に出すので、表示される場合のみ 1 行ぶん確保する。
    let preview = build_combat_preview_line(database, Some(u), cx, cy);
    if let Some(d) = unit_def {
        if !d.weapons.is_empty() {
            y += 1.0;
            divider(ctx, y);
            y += 4.0;
            let col_pow = right - 46.0; // 攻撃列 (右寄せ基準)
            let col_rng = right; // 射程列 (右端)
            ctx.set_fill_style_str(HEADER);
            ctx.set_font(&format!("bold 10px {JP_SANS}"));
            ctx.set_text_align("right");
            let _ = ctx.fill_text("攻撃", col_pow, y);
            let _ = ctx.fill_text("射程", col_rng, y);
            ctx.set_text_align("left");
            y += 12.0;

            ctx.set_font(&format!("10px {JP_SANS}"));
            let reserve = if preview.is_some() { 13.0 } else { 3.0 };
            let panel_bottom = oy as f64 + h - reserve;
            for (i, wd) in d.weapons.iter().enumerate() {
                if y + 11.0 > panel_bottom {
                    ctx.set_fill_style_str(MUTED);
                    ctx.set_text_align("left");
                    let _ = ctx.fill_text("…", pad_x, y);
                    break;
                }
                let active = selected_weapon_idx == i + 1;
                // 現在使用不可 (必要気力未達 / 残弾切れ) → 暗赤。
                let ammo_out = wd.bullet > 0
                    && u.weapons
                        .get(i)
                        .map(|uw| uw.bullet_remaining <= 0)
                        .unwrap_or(false);
                let disabled = wd.necessary_morale > u.morale || ammo_out;
                if active {
                    ctx.set_fill_style_str("rgba(40,90,200,0.14)");
                    ctx.fill_rect(pad_x - 2.0, y - 1.0, w - 8.0, 12.0);
                }
                // 名称 (左)。
                ctx.set_fill_style_str(if disabled { DISABLED } else { VALUE });
                ctx.set_text_align("left");
                let prefix = if active { "▶" } else { "" };
                let _ = ctx.fill_text(&format!("{prefix}{}", truncate(&wd.name, 16)), pad_x, y);
                // 攻撃 / 射程 (右寄せ)。
                ctx.set_text_align("right");
                let _ = ctx.fill_text(&wd.power.to_string(), col_pow, y);
                let mut rng = if wd.min_range == wd.max_range {
                    wd.max_range.to_string()
                } else {
                    format!("{}-{}", wd.min_range, wd.max_range)
                };
                // MAP 兵器 (class に全角Ｍ) は射程末尾に M を付す (オリジナル「1-3M」表記)。
                if wd.class.contains('Ｍ') {
                    rng.push('M');
                }
                let _ = ctx.fill_text(&rng, col_rng, y);
                ctx.set_text_align("left");
                y += 11.0;
            }
        }
    }

    // 戦闘予測 (パネル末尾, 控えめに)。
    if let Some(preview_line) = preview {
        let bottom = oy as f64 + h - 3.0;
        ctx.set_fill_style_str("#0a6a78");
        ctx.set_font(&format!("9px {JP_SANS}"));
        ctx.set_text_align("left");
        ctx.set_text_baseline("bottom");
        let _ = ctx.fill_text(&preview_line, pad_x, bottom);
    }
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
        // ＥＣＭ エリア補正は HUD プレビュー行では未反映 (盤面走査が要・App ヘルパ依存)。
        // 実戦闘とその予測 (App::attack_resolve_and_run) では反映される。follow-up。
        1.0,
    )
    // 散 (散布) 属性武器の距離補正 (命中アップ・ダメージダウン) をプレビューにも反映。
    .apply_scatter(&weapon.class, combat::manhattan((cx, cy), (dx, dy)));
    // 160px パネル幅に収まる短縮形 (相手愛称・命中率・与ダメージ)。
    Some(format!(
        "▶{} 命中{}% 与{}",
        truncate(&def_pilot.nickname, 5),
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
