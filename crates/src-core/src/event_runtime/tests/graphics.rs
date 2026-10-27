//! `.eve` の描画命令 (PaintString / PaintPicture / 図形 / Hotpoint / フェード) のテスト。

use super::*;

#[test]
fn font_resolves_variable_argument() {
    // `Font LetterShadow` のように変数でフォント指定を切り替える記法。
    // 変数が色文字列なら SetFont の color に反映される。
    let app = run_script(
        "Set LetterShadow #000000\n\
             Font LetterShadow\n\
             PaintString 10 10 影\n",
    );
    let setfont = app.script_overlay().cmds.iter().find_map(|c| match c {
        crate::DrawCmd::SetFont { color, .. } => Some(color.clone()),
        _ => None,
    });
    assert_eq!(setfont.as_deref(), Some("#000000"));
}

#[test]
fn paintpicture_pushes_draw_cmd() {
    let src = "PaintPicture path/foo.bmp 10 20 50 60 透過 左右反転\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        crate::DrawCmd::Picture {
            path,
            x,
            y,
            w,
            h,
            transparent,
            flip_x,
            ..
        } => {
            assert_eq!(path, "path/foo.bmp");
            assert_eq!(*x, 10.0);
            assert_eq!(*y, 20.0);
            assert_eq!(*w, Some(50.0));
            assert_eq!(*h, Some(60.0));
            assert!(*transparent);
            assert!(*flip_x);
        }
        _ => panic!("expected Picture"),
    }
}

#[test]
fn paintpicture_dash_position_sets_center_flags() {
    // `PaintPicture img - -`（幅省略・中央寄せ）は center フラグを立て、
    // 実寸を知るフロントエンドに中央寄せを委譲する。明示座標では立たない。
    // スパロボ戦記 機体能力画面の `\event\機体能力.png - - 透過` が
    // (240,240) に左上ずれして表示されていた不具合の回帰防止。
    let mut app = App::new();
    execute(
        &mut app,
        &event::parse("PaintPicture frame.png - - 透過\n").unwrap(),
    )
    .unwrap();
    match &app.script_overlay().cmds[0] {
        crate::DrawCmd::Picture {
            center_x,
            center_y,
            transparent,
            w,
            h,
            ..
        } => {
            assert!(*center_x, "x=- は center_x");
            assert!(*center_y, "y=- は center_y");
            // 幅省略形で w/h の位置に来た `透過` を幅と取り違えず、透過フラグ
            // として認識する (機体能力.png が不透明で覆い隠す不具合の回帰防止)。
            assert!(*transparent, "幅省略形でも 透過 を認識する");
            assert_eq!(*w, None, "透過 を w として食わない");
            assert_eq!(*h, None);
        }
        _ => panic!("expected Picture"),
    }

    let mut app2 = App::new();
    execute(
        &mut app2,
        &event::parse("PaintPicture frame.png 10 20\n").unwrap(),
    )
    .unwrap();
    match &app2.script_overlay().cmds[0] {
        crate::DrawCmd::Picture {
            center_x,
            center_y,
            x,
            y,
            ..
        } => {
            assert!(!*center_x);
            assert!(!*center_y);
            assert_eq!(*x, 10.0);
            assert_eq!(*y, 20.0);
        }
        _ => panic!("expected Picture"),
    }
}

#[test]
fn arith_operator_char_set_covers_evaluator_operators() {
    // `is_arith_operator_char` (式のアトム分割の単一の基準) は、統一評価器が算術演算子
    // として扱う 1 文字の記号 (`+ - * / \ ^`) と括弧をすべて含むこと。
    for c in ['+', '-', '*', '/', '\\', '^', '(', ')'] {
        assert!(is_arith_operator_char(c), "{c:?} は算術演算子のはず");
    }
    for c in ['<', '>', '=', '&', 'a', '1', ' ', '.'] {
        assert!(!is_arith_operator_char(c), "{c:?} は算術演算子ではない");
    }
}

#[test]
fn color_command_pushes_setcolor() {
    let src = "\
Color RGB(50, 100, 200)
Color 赤
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert!(matches!(&cmds[0], crate::DrawCmd::SetColor { color } if color == "#3264c8"));
    assert!(matches!(&cmds[1], crate::DrawCmd::SetColor { color } if color == "#ff0000"));
}

#[test]
fn drawwidth_command() {
    let src = "DrawWidth 3\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert!(matches!(&cmds[0], crate::DrawCmd::SetLineWidth(n) if *n == 3.0));
}

#[test]
fn circle_command_pushes_draw_cmd() {
    // `Circle x y r [color]` — 色省略時は SetColor を積まず、円のみ。
    let mut app = App::new();
    execute(&mut app, &event::parse("Circle 100 150 50\n").unwrap()).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert_eq!(cmds.len(), 1, "色省略時は SetColor を積まない");
    assert!(
        matches!(&cmds[0], crate::DrawCmd::Circle { cx, cy, r } if *cx == 100.0 && *cy == 150.0 && *r == 50.0)
    );

    // 色指定あり: RGB(...) は #rrggbb に評価され SetColor → Circle の順で積む。
    let mut app2 = App::new();
    execute(
        &mut app2,
        &event::parse("Circle 200 250 100 RGB(255,0,0)\n").unwrap(),
    )
    .unwrap();
    let cmds = &app2.script_overlay().cmds;
    assert!(
        matches!(&cmds[0], crate::DrawCmd::SetColor { color } if color == "#ff0000"),
        "色指定は SetColor を先に積む: {:?}",
        cmds[0]
    );
    assert!(matches!(&cmds[1], crate::DrawCmd::Circle { r, .. } if *r == 100.0));
}

#[test]
fn circle_radius_evaluates_arithmetic() {
    // 半径が変数入り算術式 `(i * 30)` でも app-aware に評価される (Line と同等)。
    let mut app = App::new();
    app.set_script_var("i".to_string(), "3".to_string());
    execute(
        &mut app,
        &event::parse("Circle 240 240 (i * 30)\n").unwrap(),
    )
    .unwrap();
    assert!(
        matches!(&app.script_overlay().cmds[0], crate::DrawCmd::Circle { r, .. } if *r == 90.0)
    );
}

#[test]
fn oval_command_with_ratio() {
    let mut app = App::new();
    execute(&mut app, &event::parse("Oval 100 150 50 0.5\n").unwrap()).unwrap();
    assert!(matches!(
        &app.script_overlay().cmds[0],
        crate::DrawCmd::Oval { cx, cy, r, ratio }
            if *cx == 100.0 && *cy == 150.0 && *r == 50.0 && (*ratio - 0.5).abs() < 1e-9
    ));
}

#[test]
fn arc_command_angles() {
    let mut app = App::new();
    execute(&mut app, &event::parse("Arc 100 150 50 0 180\n").unwrap()).unwrap();
    assert!(matches!(
        &app.script_overlay().cmds[0],
        crate::DrawCmd::Arc { cx, cy, r, start_deg, end_deg }
            if *cx == 100.0 && *cy == 150.0 && *r == 50.0 && *start_deg == 0.0 && *end_deg == 180.0
    ));
}

#[test]
fn polygon_command_collects_vertices() {
    // `Polygon x1 y1 x2 y2 x3 y3` → 3 頂点。色オプションは無い。
    let mut app = App::new();
    execute(
        &mut app,
        &event::parse("Polygon 0 100 20 20 100 50\n").unwrap(),
    )
    .unwrap();
    match &app.script_overlay().cmds[0] {
        crate::DrawCmd::Polygon { points } => {
            assert_eq!(points, &vec![(0.0, 100.0), (20.0, 20.0), (100.0, 50.0)]);
        }
        other => panic!("expected Polygon, got {other:?}"),
    }
}

#[test]
fn fillstyle_and_fillcolor_state() {
    // `FillStyle 塗りつぶし` → solid(true)、`透明` → solid(false)。
    // `FillColor` は線色 (SetColor) ではなく SetFillColor を積む。
    let mut app = App::new();
    execute(
        &mut app,
        &event::parse("FillStyle 塗りつぶし\nFillColor RGB(0,0,255)\nFillStyle 透明\n").unwrap(),
    )
    .unwrap();
    let cmds = &app.script_overlay().cmds;
    assert!(matches!(&cmds[0], crate::DrawCmd::SetFillSolid(true)));
    assert!(
        matches!(&cmds[1], crate::DrawCmd::SetFillColor { color } if color == "#0000ff"),
        "FillColor は SetFillColor を積む (線色と独立): {:?}",
        cmds[1]
    );
    assert!(matches!(&cmds[2], crate::DrawCmd::SetFillSolid(false)));
}

#[test]
fn pen_state_persists_across_clearpicture() {
    // SRC の ObjColor/ObjFillStyle/ObjFillColor/ObjDrawWidth は ClearPicture を
    // 跨いで保持される。戦闘アニメは Color/FillStyle をループ外で 1 度だけ設定し、
    // 毎フレーム ClearPicture するため、保持されないと図形が既定色/無塗りになる。
    let src = "\
Color RGB(255,0,0)
FillStyle 塗りつぶし
FillColor RGB(0,0,255)
DrawWidth 3
PaintPicture a.bmp 0 0
Refresh
ClearPicture
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let ov = app.script_overlay();
    assert_eq!(
        ov.current_color, "#ff0000",
        "Color は ClearPicture 後も保持"
    );
    assert!(ov.current_fill_solid, "FillStyle 塗りつぶしは保持");
    assert_eq!(ov.current_fill_color, "#0000ff", "FillColor は保持");
    assert_eq!(ov.current_line_width, 3.0, "DrawWidth は保持");
    // 次フレームの描画 push で cmds はクリアされるが、ペン状態は残る。
    execute(&mut app, &event::parse("Circle 10 10 5\n").unwrap()).unwrap();
    let ov = app.script_overlay();
    assert_eq!(ov.current_color, "#ff0000", "新フレームでもペン状態は維持");
    assert!(
        ov.cmds
            .iter()
            .any(|c| matches!(c, crate::DrawCmd::Circle { .. })),
        "新フレームの Circle が積まれる"
    );
    // シーン遷移相当の clear() はペン状態を既定へ戻す。
    app.script_overlay_mut().clear();
    let ov = app.script_overlay();
    assert_eq!(ov.current_color, "", "clear() でペン色リセット");
    assert!(!ov.current_fill_solid, "clear() で塗りリセット");
}

#[test]
fn hotpoint_registers_with_eval_coords() {
    let src = "\
Set base 200
Hotpoint UnitA (50 + 10) ($(base) - 25) 50 50
Hotpoint UnitB 100 100 50 50 非表示
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let hp = app.hotpoints();
    assert_eq!(hp.len(), 2);
    assert_eq!(hp[0].name, "UnitA");
    assert_eq!(hp[0].x, 60);
    assert_eq!(hp[0].y, 175);
    assert_eq!(hp[0].w, 50);
    assert!(!hp[0].invisible);
    assert_eq!(hp[1].name, "UnitB");
    assert!(hp[1].invisible);
}

#[test]
fn hotpoint_coords_resolve_loop_variable() {
    // スパロボ戦記 機体選択開始 と同型: ループ変数 `j` を含む算術式
    // `(j * 40 - 35)` を Hotpoint 座標に使う (裸の識別子 `j` は式中で変数として引く)。
    // 引用符付き数値 `"2"` も座標式に混じる (`("$(i)" * 70 + 15)` 由来)。
    let src = "\
Set j 3
Set i 2
Hotpoint Cell (j * 40 - 35) (\"$(i)\" * 70 + 15) 32 32
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let hp = app.hotpoints();
    assert_eq!(hp.len(), 1);
    // x = 3*40-35 = 85
    assert_eq!(hp[0].x, 85, "ループ変数 j が解決されていない");
    // y = 2*70+15 = 155 (引用符付き "2" を数値として扱う)
    assert_eq!(hp[0].y, 155, "引用符付き数値が解決されていない");
}

#[test]
fn refresh_keeps_hotpoints() {
    // Refresh は画面再描画のみで Hotpoint は維持する (SRC 仕様)
    let src = "\
Hotpoint A 0 0 50 50
Hotpoint B 100 0 50 50
Refresh
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.hotpoints().len(), 2);
}

#[test]
fn clearobj_clears_hotpoints() {
    let src = "\
Hotpoint A 0 0 50 50
ClearObj
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert!(app.hotpoints().is_empty());
}

#[test]
fn hotpoint_click_outside_does_not_consume() {
    // Hotpoint の外側をクリックしても Menu は閉じない（無反応）。
    let src = "\
Hotpoint A 0 0 50 50
Wait Click
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert!(app.pending_dialog().is_some());
    let consumed = app.handle_input(crate::Input::ClickAt { x: 200, y: 200 });
    assert!(!consumed);
    assert!(app.pending_dialog().is_some());
}

#[test]
fn line_with_bf_emits_fillrect() {
    let src = "Line 10 20 50 60 BF\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    let has_fillrect = cmds.iter().any(|c| {
        matches!(
            c,
            crate::script_overlay::DrawCmd::FillRect { w, h, .. }
                if *w > 0.0 && *h > 0.0
        )
    });
    assert!(has_fillrect, "expected FillRect, got: {:?}", cmds);
}

#[test]
fn paintstring_dash_x_uses_center() {
    // `PaintString - 100 text` — x が "-" のとき SRC 480×480 map window
    // の中央 (240) を使用。
    let src = "PaintString - 100 hello\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        crate::script_overlay::DrawCmd::PaintString { x, y, text } => {
            assert_eq!(*x, 240.0);
            assert_eq!(*y, 100.0);
            assert_eq!(text, "hello");
        }
        _ => panic!("not PaintString"),
    }
}

#[test]
fn font_parses_size_and_color_in_any_order() {
    // `Font 14pt Bold #5E5E5E` / `Font #5E5E5E` 等の柔軟な並びを許容する。
    let src = "Font 14pt Bold #5E5E5E\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    match &app.script_overlay().cmds[0] {
        crate::script_overlay::DrawCmd::SetFont {
            family,
            size_pt,
            color,
        } => {
            assert_eq!(*size_pt, 14);
            assert_eq!(color, "#5E5E5E");
            assert!(family.starts_with("bold "), "family={family}");
        }
        _ => panic!("not SetFont"),
    }
}

#[test]
fn hotpointstring_renders_and_registers_hotpoint() {
    // `HotpointString x y text` は PaintString に加えて Hotpoint を登録する。
    let src = "HotpointString 100 200 START\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let hp = app.hotpoints();
    assert_eq!(hp.len(), 1);
    assert_eq!(hp[0].name, "START");
    assert_eq!(hp[0].x, 100);
    // y は文字列上端を指すので、Hotpoint 矩形は y - line_height = 200 - 22 = 178
    assert_eq!(hp[0].y, 178);
}

#[test]
fn arithmetic_eval_in_hotpoint() {
    let src = "\
Set n 100
Hotpoint X (($(n) + 50) * 2 - 100) 0 30 30
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // (100 + 50) * 2 - 100 = 200
    assert_eq!(app.hotpoints()[0].x, 200);
}

#[test]
fn paintstring_pushes_to_overlay() {
    let src = "\
Font ゴシック 14pt #ffe0e0
PaintString 100 200 こんにちは
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert_eq!(cmds.len(), 2);
    match &cmds[0] {
        crate::DrawCmd::SetFont {
            family,
            size_pt,
            color,
        } => {
            assert_eq!(family, "ゴシック");
            assert_eq!(*size_pt, 14);
            assert_eq!(color, "#ffe0e0");
        }
        _ => panic!("expected SetFont"),
    }
    match &cmds[1] {
        crate::DrawCmd::PaintString { x, y, text } => {
            assert_eq!(*x, 100.0);
            assert_eq!(*y, 200.0);
            assert_eq!(text, "こんにちは");
        }
        _ => panic!("expected PaintString"),
    }
}

#[test]
fn paintstring_coord_expression_not_leaked_into_text() {
    // 座標が関数呼び出し / 変数を含む括弧式の場合でも、それを text に
    // 流出させない (画面に式がそのまま描画されるバグの再発防止)。
    // スパロボ戦記 String.eve VibrationString と同型。
    let src = "\
Set k 2
PaintString (k * 10 + 5) (k * 20) ヴァイブ
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let ps = app
        .script_overlay()
        .cmds
        .iter()
        .find_map(|c| match c {
            crate::DrawCmd::PaintString { x, y, text } => Some((*x, *y, text.clone())),
            _ => None,
        })
        .expect("PaintString が無い");
    // text は式ではなく "ヴァイブ" のみ
    assert_eq!(ps.2, "ヴァイブ");
    // 座標は評価される (k=2 → x=25, y=40)
    assert_eq!(ps.0, 25.0);
    assert_eq!(ps.1, 40.0);
}

#[test]
fn line_command_pushes_draw_cmd() {
    let src = "Line 0 0 100 100 #ff0000\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert_eq!(cmds.len(), 2);
    assert!(matches!(&cmds[0], crate::DrawCmd::SetColor { color } if color == "#ff0000"));
    assert!(
        matches!(&cmds[1], crate::DrawCmd::Line { x1, y1, x2, y2 } if *x1 == 0.0 && *y1 == 0.0 && *x2 == 100.0 && *y2 == 100.0)
    );
}

#[test]
fn refresh_keeps_accumulated_drawing() {
    // 元 SRC `Refresh` は present のみで描画内容はクリアしない
    // (クリアは Cls / ClearPicture / ClearObj の役割)。
    // `draw / Refresh / draw` で全描画が蓄積されたまま残る。
    let src = "\
PaintString 10 10 a
PaintString 20 20 b
Refresh
PaintString 30 30 c
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    // Refresh はクリアしないので a / b / c すべて残る
    assert_eq!(cmds.len(), 3);
    assert!(matches!(&cmds[2], crate::DrawCmd::PaintString { text, .. } if text == "c"));
}

#[test]
fn fadeout_pushes_fade() {
    let src = "FadeOut 30\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert_eq!(cmds.len(), 1);
    assert!(matches!(&cmds[0], crate::DrawCmd::Fade { alpha, .. } if *alpha > 0.0));
}

/// 回帰: `WhiteIn` は白フェードアウトを露出 (除去) する。引数なし `WhiteIn` が
/// 全画面白を積みっぱなしにすると「白いマップ」で操作不能になる
/// (東方夢想伝: タイトルテロップ末尾の `WhiteIn`)。
#[test]
fn whitein_clears_whiteout_overlay() {
    let stmts = event::parse("WhiteOut 255\nWhiteIn\n").unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let has_white = app
        .script_overlay()
        .cmds
        .iter()
        .any(|c| matches!(c, crate::DrawCmd::Fade { color, .. } if color == "#ffffff"));
    assert!(
        !has_white,
        "WhiteIn 後に白 Fade が残存: {:?}",
        app.script_overlay().cmds
    );
}

/// 回帰: `FadeIn` は黒フェードアウトを露出 (除去) する。
#[test]
fn fadein_clears_fadeout_overlay() {
    let stmts = event::parse("FadeOut 60\nFadeIn\n").unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let has_black = app
        .script_overlay()
        .cmds
        .iter()
        .any(|c| matches!(c, crate::DrawCmd::Fade { color, .. } if color == "#000000"));
    assert!(
        !has_black,
        "FadeIn 後に黒 Fade が残存: {:?}",
        app.script_overlay().cmds
    );
}

/// `ClearPicture x1 y1 x2 y2` は矩形消去 (原典 `ClearPicture2`)、`ChangeMode x y` は
/// 選択中ユニットの座標モード (原典 `ExecChangeModeCmd` の 2 引数・数値形)。
#[test]
fn clearpicture_range_and_changemode_coordinates() {
    let mut app = App::new();
    setup_two_units(&mut app);
    let braver = app.database().unit_instances[0].uid.clone();
    app.set_selected_unit_for_event(braver);
    let src = "\
Set w 100
Line 0 0 50 50
ClearPicture 10 20 (w + 10) 120
ChangeMode 4 (1 + 2)
ChangeMode 敵 固定
ChangeMode ガロ 2 (1 + 2)
";
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    assert_eq!(
        app.script_overlay().cmds.last(),
        Some(&crate::DrawCmd::ClearRect {
            x1: 10.0,
            y1: 20.0,
            x2: 110.0,
            y2: 120.0
        })
    );
    // Line は消えずに残る (矩形の外側の描画は保たれる)。
    assert!(app
        .script_overlay()
        .cmds
        .iter()
        .any(|c| matches!(c, crate::DrawCmd::Line { .. })));
    let units = &app.database().unit_instances;
    assert_eq!(units[0].ai_mode, "4 3");
    // 陣営指定 (`敵 固定`) の後、ユニット指定の座標モードで上書きする。
    assert_eq!(units[1].ai_mode, "2 3");
}
