//! 関数式 (`expand_vars` / `eval_script_function`) のエッジケース。
//!
//! SRC.Sharp `SRCCoreTests/Expressions/*` を参考に、実シナリオで多用される
//! 関数の挙動をユニットテストとして固定する。VB6 原典の semantic から
//! ずれていたら検出できるようにする。
//!
//! 著作権配慮: SRC オリジナルコードは含まない。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn run(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

// ============================================================
//  String functions
// ============================================================

// ============================================================
//  組込システム変数 / Built-in system variables
// ============================================================

#[test]
fn app_version_reports_src_2_compatible_value() {
    // SRC.Sharp `Expression.cs` の `appversion` = 10000*Major+100*Minor+Revision。
    // 本移植は 2.2.33 相当 (20233) を報告し、SRC 2.00 以降を要求する版数ゲート
    // (Welcome.eve 冒頭の `If AppVersion < 20000`) を通す。
    let app = run("Set v AppVersion\n");
    assert_eq!(app.script_var("v"), "20233");

    // 大小無視で解決される (原典は変数名を小文字化して判別する)。
    let app2 = run("Set v appversion\n");
    assert_eq!(app2.script_var("v"), "20233");
}

#[test]
fn app_version_passes_src2_gate() {
    // Welcome.eve 冒頭の版数ゲートと同型: AppVersion >= 20000 で通過すること。
    let app = run("If AppVersion < 20000 Then\nSet 古い 1\nElse\nSet 新しい 1\nEndif\n");
    assert_eq!(app.script_var("古い"), "");
    assert_eq!(app.script_var("新しい"), "1");
}

#[test]
fn len_returns_char_count_not_byte_count() {
    // 日本語 3 文字 (UTF-8 では 9 bytes) → Len は 3 を返す
    let app = run(r#"
Set s "あいう"
Set n Len($(s))
"#);
    assert_eq!(app.script_var("n"), "3");
}

#[test]
fn left_right_mid_handle_japanese_chars() {
    let app = run(r#"
Set s "あいうえお"
Set l Left($(s),2)
Set r Right($(s),2)
Set m Mid($(s),2,2)
"#);
    assert_eq!(app.script_var("l"), "あい");
    assert_eq!(app.script_var("r"), "えお");
    assert_eq!(app.script_var("m"), "いう");
}

#[test]
fn instr_returns_1indexed_position_or_zero() {
    let app = run(r#"
Set hit InStr("hello","ll")
Set miss InStr("hello","xx")
"#);
    // "ll" は 3 文字目 (1-indexed)
    assert_eq!(app.script_var("hit"), "3");
    assert_eq!(app.script_var("miss"), "0");
}

#[test]
fn replace_substitutes_substring() {
    let app = run(r#"
Set s Replace("alpha-beta-gamma","-","_")
"#);
    assert_eq!(app.script_var("s"), "alpha_beta_gamma");
}

// ============================================================
//  Math functions
// ============================================================

#[test]
fn min_max_abs_basic() {
    let app = run(r#"
Set a Min(3,7)
Set b Max(3,7)
Set c Abs(-5)
"#);
    assert_eq!(app.script_var("a"), "3");
    assert_eq!(app.script_var("b"), "7");
    assert_eq!(app.script_var("c"), "5");
}

#[test]
fn round_roundup_rounddown_no_digits() {
    let app = run(r#"
Set a Round(3.4)
Set b Round(3.6)
Set c RoundUp(3.1)
Set d RoundDown(3.9)
"#);
    assert_eq!(app.script_var("a"), "3");
    assert_eq!(app.script_var("b"), "4");
    assert_eq!(app.script_var("c"), "4");
    assert_eq!(app.script_var("d"), "3");
}

#[test]
fn round_with_digits_2dp() {
    let app = run(r#"
Set a Round(3.14159,2)
Set b Round(3.149,2)
"#);
    assert_eq!(app.script_var("a"), "3.14");
    assert_eq!(app.script_var("b"), "3.15");
}

#[test]
fn int_floors_toward_negative_infinity() {
    // VB6 / SRC.Sharp Int() は Floor 仕様 (truncate-to-zero ではない)。
    // SRC.Sharp `MathFunctionTests.cs::Int_NegativeDecimal_TruncatesDown`
    // → Math.Floor(-3.1) = -4 を要求。
    let app = run(r#"
Set a Int(3.9)
Set b Int(-3.9)
"#);
    assert_eq!(app.script_var("a"), "3");
    assert_eq!(app.script_var("b"), "-4");
}

// ============================================================
//  List functions
// ============================================================

#[test]
fn list_lindex_llength_basic() {
    let app = run(r#"
Set lst List(a,b,c,d)
Set n Llength($(lst))
Set first Lindex($(lst),1)
Set last Lindex($(lst),4)
"#);
    assert_eq!(app.script_var("n"), "4");
    assert_eq!(app.script_var("first"), "a");
    assert_eq!(app.script_var("last"), "d");
}

#[test]
fn lsearch_returns_1indexed_or_zero() {
    // SRC.Sharp `Functions/List.cs::LSearch` 仕様: 見つからなければ 0
    // (`Lsearch_NotFound_ReturnsZero` テスト参照)。
    let app = run(r#"
Set lst List(alpha,beta,gamma)
Set found Lsearch($(lst),beta)
Set miss Lsearch($(lst),xxx)
"#);
    assert_eq!(app.script_var("found"), "2");
    assert_eq!(app.script_var("miss"), "0");
}

#[test]
fn lsplit_lremove_basic() {
    let app = run(r#"
Set lst Lsplit("a,b,c,d",",")
Set n Llength($(lst))
Set lst2 Lremove($(lst),b)
Set n2 Llength($(lst2))
"#);
    assert_eq!(app.script_var("n"), "4");
    assert_eq!(app.script_var("n2"), "3");
}

// ============================================================
//  Predicates
// ============================================================

#[test]
fn isvardefined_empty_string_set_returns_defined() {
    // SRC.Sharp `VariableTests.cs::SetVariableAsString_EmptyString_IsDefinedAndEmpty`
    // 仕様: `Set var ""` 後の IsVarDefined は **1** (defined)。
    let app = run(r#"
Set defined hello
Set defined_empty ""
Set d1 IsVarDefined(defined)
Set d2 IsVarDefined(defined_empty)
Set d3 IsVarDefined(neverset)
"#);
    assert_eq!(app.script_var("d1"), "1");
    assert_eq!(app.script_var("d2"), "1", "空文字代入も defined のはず");
    assert_eq!(app.script_var("d3"), "0");
}

#[test]
fn isnumeric_basic() {
    let app = run(r#"
Set a IsNumeric("42")
Set b IsNumeric("3.14")
Set c IsNumeric("abc")
Set d IsNumeric("")
"#);
    assert_eq!(app.script_var("a"), "1");
    assert_eq!(app.script_var("b"), "1");
    assert_eq!(app.script_var("c"), "0");
    assert_eq!(app.script_var("d"), "0");
}

// ============================================================
//  Conditional / format
// ============================================================

#[test]
fn iif_picks_branch() {
    let app = run(r#"
Set a IIF(1,yes,no)
Set b IIF(0,yes,no)
"#);
    assert_eq!(app.script_var("a"), "yes");
    assert_eq!(app.script_var("b"), "no");
}

#[test]
fn rgb_returns_hex_color() {
    // SRC.Sharp: RGB(r,g,b) → "#rrggbb" 形式 (PaintRGBFunctionTests.cs 準拠)
    let app = run(r#"
Set c RGB(255,128,0)
"#);
    assert_eq!(app.script_var("c"), "#ff8000");
}

#[test]
fn rgb_black_is_hash_000000() {
    let app = run(r#"Set v RGB(0,0,0)"#);
    assert_eq!(app.script_var("v"), "#000000");
}

#[test]
fn rgb_white_is_hash_ffffff() {
    let app = run(r#"Set v RGB(255,255,255)"#);
    assert_eq!(app.script_var("v"), "#ffffff");
}

#[test]
fn rgb_low_values_are_zero_padded() {
    // R=1,G=2,B=3 → #010203
    let app = run(r#"Set v RGB(1,2,3)"#);
    assert_eq!(app.script_var("v"), "#010203");
}

#[test]
fn rgb_mixed_returns_correct_hex() {
    // R=128,G=64,B=32 → #804020
    let app = run(r#"Set v RGB(128,64,32)"#);
    assert_eq!(app.script_var("v"), "#804020");
}

#[test]
fn format_zero_padded_and_signed() {
    let app = run(r#"
Set a Format(42,"00000")
Set b Format(-5,"00000")
"#);
    assert_eq!(app.script_var("a"), "00042");
    // 符号付き format の挙動を pin
    let v = app.script_var("b");
    assert!(v == "-0005" || v == "-00005", "Format(-5,00000) = {v}");
}

// ============================================================
//  Random (seeded so we get deterministic-ish coverage)
// ============================================================

#[test]
fn random_range_within_bounds() {
    // SRC `Random(n)` は 1..n (GeneralLib.Dice 準拠) — 値域を 100 回確認。
    let app = run(r#"
Set seed 1
"#);
    let mut max_seen = 0i64;
    let mut min_seen = i64::MAX;
    let mut app = app;
    for _ in 0..100 {
        let stmts = event::parse("Set r Random(10)\n").unwrap();
        event_runtime::execute(&mut app, &stmts).expect("exec");
        let v: i64 = app.script_var("r").parse().unwrap_or(-1);
        assert!((1..=10).contains(&v), "Random(10) outside 1..=10: {v}");
        max_seen = max_seen.max(v);
        min_seen = min_seen.min(v);
    }
    assert!(min_seen < max_seen, "Random distribution suspicious");
}

// ============================================================
//  Additional arithmetic (Sgn/Mod/Hex/Oct/Atan2)
// ============================================================

#[test]
fn sgn_returns_minus_one_zero_or_one() {
    let app = run(r#"
Set p Sgn(5)
Set z Sgn(0)
Set m Sgn(-7)
"#);
    assert_eq!(app.script_var("p"), "1");
    assert_eq!(app.script_var("z"), "0");
    assert_eq!(app.script_var("m"), "-1");
}

#[test]
fn mod_function_returns_integer_remainder() {
    // SRC.Sharp と VB6 の Mod は整数演算: 10 mod 3 = 1, (-10) mod 3 = -1
    let app = run(r#"
Set a Mod(10,3)
Set b Mod(20,7)
Set c Mod(-10,3)
"#);
    assert_eq!(app.script_var("a"), "1");
    assert_eq!(app.script_var("b"), "6");
    // 負数 mod は実装で符号が分かれるが、Rust の整数 % は被除数の符号を保持。
    assert_eq!(app.script_var("c"), "-1");
}

#[test]
fn mod_function_zero_divisor_returns_zero() {
    let app = run(r#"
Set d Mod(7,0)
"#);
    assert_eq!(app.script_var("d"), "0", "ゼロ除算は黙って 0");
}

#[test]
fn hex_oct_format_integers() {
    let app = run(r#"
Set h Hex(255)
Set o Oct(8)
"#);
    assert_eq!(app.script_var("h"), "FF");
    assert_eq!(app.script_var("o"), "10");
}

#[test]
fn atan2_quadrants() {
    let app = run(r#"
Set q1 Atan2(1,1)
Set q3 Atan2(-1,-1)
"#);
    // π/4 ≈ 0.785, -3π/4 ≈ -2.356
    let q1: f64 = app.script_var("q1").parse().unwrap();
    let q3: f64 = app.script_var("q3").parse().unwrap();
    assert!((q1 - std::f64::consts::FRAC_PI_4).abs() < 1e-6);
    assert!((q3 + 3.0 * std::f64::consts::FRAC_PI_4).abs() < 1e-6);
}

// ============================================================
//  Time functions (Now/Year/.../DiffTime/GetTime)
// ============================================================

#[test]
fn now_returns_epoch_when_wall_clock_zero() {
    // src-core 単独 (テスト) は wall_clock_ms=0 で固定 = 1970-01-01 00:00:00 UTC
    let app = run("Set t Now()\n");
    assert_eq!(app.script_var("t"), "1970/01/01 00:00:00");
}

#[test]
fn time_components_from_literal_datetime() {
    // Year("2024/12/25 15:30:45") = 2024 等
    let app = run(r#"
Set y Year("2024/12/25 15:30:45")
Set m Month("2024/12/25 15:30:45")
Set d Day("2024/12/25 15:30:45")
Set h Hour("2024/12/25 15:30:45")
Set mi Minute("2024/12/25 15:30:45")
Set s Second("2024/12/25 15:30:45")
"#);
    assert_eq!(app.script_var("y"), "2024");
    assert_eq!(app.script_var("m"), "12");
    assert_eq!(app.script_var("d"), "25");
    assert_eq!(app.script_var("h"), "15");
    assert_eq!(app.script_var("mi"), "30");
    assert_eq!(app.script_var("s"), "45");
}

#[test]
fn weekday_returns_japanese_day_name() {
    // 2024-12-25 は水曜
    let app = run(r#"
Set w Weekday("2024/12/25")
"#);
    assert_eq!(app.script_var("w"), "水曜");
}

#[test]
fn difftime_returns_seconds_between() {
    // 2024/01/01 00:00:00 → 2024/01/01 00:01:30 = 90 秒
    let app = run(r#"
Set d DiffTime("2024/01/01 00:00:00","2024/01/01 00:01:30")
"#);
    assert_eq!(app.script_var("d"), "90");
}

#[test]
fn time_funcs_default_to_now() {
    // 引数省略時は Now() の値を使う (wall_clock=0 → 1970)
    let app = run(r#"
Set y Year()
Set m Month()
"#);
    assert_eq!(app.script_var("y"), "1970");
    assert_eq!(app.script_var("m"), "1");
}

#[test]
fn invalid_datetime_returns_zero() {
    let app = run(r#"
Set y Year("not a date")
"#);
    assert_eq!(app.script_var("y"), "0");
}

// ============================================================
//  File functions (FileExists/FileLen/EOF/LOF/Loc)
// ============================================================

#[test]
fn file_exists_after_open_write() {
    // Open で書き込みモード → ファイルが VFS に登録される。
    let app = run(r#"
Open "data/test.txt" For Output As #1
Print #1, hello
Close #1
Set e FileExists("data/test.txt")
Set n FileExists("nonexistent.txt")
"#);
    assert_eq!(app.script_var("e"), "1");
    assert_eq!(app.script_var("n"), "0");
}

#[test]
fn file_len_returns_byte_count() {
    let app = run(r#"
Open "data/sz.txt" For Output As #1
Print #1, abc
Print #1, defgh
Close #1
Set l FileLen("data/sz.txt")
"#);
    // 各行 + 改行: "abc\n" (4) + "defgh\n" (6) = 10
    assert_eq!(app.script_var("l"), "10");
}

#[test]
fn folder_exists_always_zero() {
    // VFS にフォルダ概念が無いので常に 0 (乖離注記)
    let app = run("Set f FolderExists(\"Data\")\n");
    assert_eq!(app.script_var("f"), "0");
}

// ============================================================
//  Unit info functions (Action/Damage/Condition/Status/Bullet)
// ============================================================

const UNIT_SETUP: &str = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon "ブレイバー" "ライフル" 2500 2 5 15 3
Weapon "ブレイバー" "ビームサーベル" 3000 1 1 12 -1
Place "ブレイバー" "リオ" Player 1 1
"#;

fn run_with_unit(extra: &str) -> App {
    let mut app = App::new();
    let src = format!("{UNIT_SETUP}{extra}");
    let stmts = event::parse(&src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

#[test]
fn action_returns_one_when_not_acted_zero_otherwise() {
    // 初期状態: has_acted = false → 1
    let app = run_with_unit("Set a Action(リオ)\n");
    assert_eq!(app.script_var("a"), "1");

    // has_acted=true にしてから Action() = 0 を確認するためには runtime 側で
    // 直接書き換えるしかないので、追加の execute で再評価する。
    let mut app = app;
    app.database_mut().unit_instances[0].has_acted = true;
    let stmts = event::parse("Set b Action(リオ)\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("b"), "0");
}

#[test]
fn damage_returns_percent_zero_to_hundred() {
    // 初期 damage=0 → 0%
    let app = run_with_unit("Set d Damage(リオ)\n");
    assert_eq!(app.script_var("d"), "0");

    // damage=1750 (HP 3500 の 50%)
    let mut app = app;
    app.database_mut().unit_instances[0].damage = 1750;
    let stmts = event::parse("Set d Damage(リオ)\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("d"), "50");
}

#[test]
fn condition_checks_status_presence() {
    // SetStatus で熱血付与 → Condition(リオ,熱血) = 1
    let app = run_with_unit(
        r#"
SetStatus リオ 熱血
Set c1 Condition(リオ,熱血)
Set c2 Condition(リオ,鉄壁)
"#,
    );
    assert_eq!(app.script_var("c1"), "1");
    assert_eq!(app.script_var("c2"), "0");
}

#[test]
fn status_returns_sortie_or_standby() {
    // 通常は出撃
    let app = run_with_unit("Set s Status(リオ)\n");
    assert_eq!(app.script_var("s"), "出撃");
    // off_map=true → 待機
    let mut app = app;
    app.database_mut().unit_instances[0].off_map = true;
    let stmts = event::parse("Set s Status(リオ)\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("s"), "待機");
}

#[test]
fn status_returns_destroyed_for_unknown_unit() {
    let app = run_with_unit("Set s Status(存在しないパイロット)\n");
    assert_eq!(app.script_var("s"), "破棄");
}

#[test]
fn pointx_pointy_track_draw_cursor() {
    // Line / PSet 実行で描画カーソルが終端座標に動き、PointX/PointY/BaseX/BaseY
    // で読み取れる。初期値 0,0、Line で (50,40) → (200,180) を引いた後は終点。
    let app = run(r#"
PSet 10, 20
Set px1 PointX()
Set py1 PointY()
Line 50, 40, 200, 180
Set bx BaseX()
Set by BaseY()
"#);
    assert_eq!(app.script_var("px1"), "10");
    assert_eq!(app.script_var("py1"), "20");
    assert_eq!(app.script_var("bx"), "200");
    assert_eq!(app.script_var("by"), "180");
}

#[test]
fn bullet_returns_remaining_or_minus_one_for_infinite() {
    // ライフル: bullet=3 (有限), ビームサーベル: bullet=-1 (無限)
    let app = run_with_unit(
        r#"
Set b1 Bullet(リオ,ライフル)
Set b2 Bullet(リオ,ビームサーベル)
Set mb1 MaxBullet(リオ,ライフル)
"#,
    );
    assert_eq!(app.script_var("b1"), "3");
    assert_eq!(app.script_var("b2"), "-1");
    assert_eq!(app.script_var("mb1"), "3");
}

#[test]
fn count_item_returns_equipped_item_count() {
    // 装備品が無ければ 0
    let app = run_with_unit("Set c CountItem(リオ)\n");
    assert_eq!(app.script_var("c"), "0");
}

#[test]
fn wx_wy_return_pixel_coords() {
    // 1 タイル = 32 px。Place ブレイバー at (1, 1) → WX = 32, WY = 32。
    let app = run_with_unit(
        r#"
Set wx WX(リオ)
Set wy WY(リオ)
Set wx5 WX(5)
"#,
    );
    assert_eq!(app.script_var("wx"), "32");
    assert_eq!(app.script_var("wy"), "32");
    assert_eq!(app.script_var("wx5"), "160"); // 5 * 32
}

// ============================================================
//  X / Y 座標関数
// ============================================================

#[test]
fn x_y_return_grid_coordinates() {
    // X(unit) / Y(unit) はマップ上のグリッド座標を返す。
    // run_with_unit は Place at (1,1)。
    let app = run_with_unit(
        r#"
Set gx X(リオ)
Set gy Y(リオ)
"#,
    );
    assert_eq!(app.script_var("gx"), "1");
    assert_eq!(app.script_var("gy"), "1");
}

#[test]
fn x_y_after_move() {
    let app = run_with_unit(
        r#"
Move リオ 3 7
Set gx X(リオ)
Set gy Y(リオ)
"#,
    );
    assert_eq!(app.script_var("gx"), "3");
    assert_eq!(app.script_var("gy"), "7");
}

#[test]
fn x_y_unknown_unit_returns_zero() {
    let app = run("Set gx X(存在しない)\nSet gy Y(存在しない)\n");
    assert_eq!(app.script_var("gx"), "0");
    assert_eq!(app.script_var("gy"), "0");
}

// ============================================================
//  Move / ClearStatus / Question / Array / Swap / Global
//  (Pass 5: event command dispatches)
// ============================================================

#[test]
fn move_command_teleports_unit_ignoring_movement_range() {
    let app = run_with_unit("Move リオ 4 5\n");
    let u = &app.database().unit_instances[0];
    assert_eq!((u.x, u.y), (4, 5));
}

#[test]
fn clearstatus_removes_condition() {
    let app = run_with_unit(
        r#"
SetStatus リオ 熱血
ClearStatus リオ 熱血
Set c Condition(リオ,熱血)
"#,
    );
    assert_eq!(app.script_var("c"), "0");
}

#[test]
fn array_splits_string_with_separator() {
    let app = run(r#"
Array fruits "りんご,みかん,ぶどう" ","
Set f1 $(fruits[1])
Set f2 $(fruits[2])
Set f3 $(fruits[3])
"#);
    assert_eq!(app.script_var("f1"), "りんご");
    assert_eq!(app.script_var("f2"), "みかん");
    assert_eq!(app.script_var("f3"), "ぶどう");
}

#[test]
fn array_with_list_separator_splits_by_whitespace() {
    let app = run(r#"
Array items "alpha beta gamma" リスト
Set i1 $(items[1])
Set i3 $(items[3])
"#);
    assert_eq!(app.script_var("i1"), "alpha");
    assert_eq!(app.script_var("i3"), "gamma");
}

#[test]
fn swap_exchanges_two_variables() {
    let app = run(r#"
Set a 100
Set b 200
Swap a b
"#);
    assert_eq!(app.script_var("a"), "200");
    assert_eq!(app.script_var("b"), "100");
}

#[test]
fn format_thousands_separator_pattern() {
    // `#,##0` パターン: 3 桁区切りカンマを付与
    let app = run(r##"
Set a Format(1234567,"#,##0")
Set b Format(100,"#,##0")
Set c Format(-9876,"#,##0")
"##);
    assert_eq!(app.script_var("a"), "1,234,567");
    assert_eq!(app.script_var("b"), "100");
    assert_eq!(app.script_var("c"), "-9,876");
}

#[test]
fn format_thousands_with_decimals() {
    // `#,##0.00` パターン: 3 桁区切り + 小数 2 桁
    let app = run(r##"
Set a Format(1234.5,"#,##0.00")
Set b Format(1000000.789,"#,##0.0")
"##);
    assert_eq!(app.script_var("a"), "1,234.50");
    // 0.789 → 0.8 (四捨五入)
    assert_eq!(app.script_var("b"), "1,000,000.8");
}

#[test]
fn format_percent_pattern() {
    // `%` は値を 100 倍し末尾に % を付与 (SRC doc / VB6 Format 準拠)
    let app = run(r##"
Set a Format(0.5,"0%")
Set b Format(0.125,"0.0%")
Set c Format(1.5,"#0%")
"##);
    assert_eq!(app.script_var("a"), "50%");
    assert_eq!(app.script_var("b"), "12.5%");
    assert_eq!(app.script_var("c"), "150%");
}

#[test]
fn format_uses_bankers_rounding_at_integer() {
    // VB6 Format は銀行丸め (round half to even)。
    // 0.5→0, 1.5→2, 2.5→2, 3.5→4 (タイは偶数側へ)。
    let app = run(r#"
Set a Format(0.5,"0")
Set b Format(1.5,"0")
Set c Format(2.5,"0")
Set d Format(3.5,"0")
"#);
    assert_eq!(app.script_var("a"), "0");
    assert_eq!(app.script_var("b"), "2");
    assert_eq!(app.script_var("c"), "2");
    assert_eq!(app.script_var("d"), "4");
}

#[test]
fn format_uses_bankers_rounding_at_decimal() {
    // 小数桁でも銀行丸め。0.125 / 0.375 は f64 で厳密表現でき、
    // ×100 後 12.5 / 37.5 のタイになるため偶数側へ丸まる。
    let app = run(r#"
Set a Format(0.125,"0.00")
Set b Format(0.375,"0.00")
"#);
    assert_eq!(app.script_var("a"), "0.12", "12.5 → 12 (偶数側へ切り下げ)");
    assert_eq!(app.script_var("b"), "0.38", "37.5 → 38 (偶数側へ切り上げ)");
}

#[test]
fn global_command_defines_variable() {
    // SRC.Sharp `GlobalCmd.cs` 準拠: `Global var` は変数 var を空文字で定義する。
    // 定義後は `IsVarDefined(var)` = 1 となる。
    let app = run(r#"
Global flag
Set d IsVarDefined(flag)
"#);
    assert_eq!(app.script_var("d"), "1");
}

#[test]
fn global_command_preserves_existing_value() {
    // SRC.Sharp 準拠: 既に定義されている変数に Global を実行しても値は変わらない。
    let app = run(r#"
Set counter 42
Global counter
Set v $(counter)
"#);
    assert_eq!(app.script_var("v"), "42");
}

#[test]
fn global_command_multiple_vars() {
    // SRC.Sharp 準拠: `Global g1 g2 g3` で複数変数を一度に宣言できる。
    let app = run(r#"
Global g1 g2 g3
Set d1 IsVarDefined(g1)
Set d2 IsVarDefined(g2)
Set d3 IsVarDefined(g3)
"#);
    assert_eq!(app.script_var("d1"), "1");
    assert_eq!(app.script_var("d2"), "1");
    assert_eq!(app.script_var("d3"), "1");
}

#[test]
fn global_command_strips_dollar_prefix() {
    // SRC.Sharp 準拠: `Global $varName` は先頭の `$` を除去して登録する。
    let app = run(r#"
Global $myGlobal
Set d IsVarDefined(myGlobal)
"#);
    assert_eq!(app.script_var("d"), "1");
}

#[test]
fn center_command_moves_map_cursor_by_unit() {
    let mut app = run_with_unit("\n");
    // unit はブレイバー/リオ at (1, 1)。事前にカーソルを (0, 0) に。
    app.set_map_cursor(0, 0);
    assert_eq!(app.map_cursor(), Some((0, 0)));
    let stmts = event::parse("Center リオ\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.map_cursor(), Some((1, 1)));
}

#[test]
fn center_command_moves_map_cursor_by_coords() {
    let mut app = run_with_unit("\n");
    // マップが無いと範囲チェックでスキップされるので最低限の MapSize を持たせる
    let stmts = event::parse("MapSize 10 10\nCenter 4 5\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.map_cursor(), Some((4, 5)));
}

#[test]
fn clearevent_removes_label_from_library() {
    // ラベル登録 → ClearEvent でラベル削除 → trigger できない
    let app = run(r#"
Goto end
@evt:
Set fired 1
Return
@end:
ClearEvent "evt"
"#);
    // ClearEvent はラベル登録後に呼ばれたので evt は labels から削除されている
    assert!(
        !app.script_library().labels.contains_key("evt"),
        "evt ラベルは ClearEvent で削除されているはず"
    );
}

#[test]
fn clearskill_removes_skill_from_pilot() {
    // SetSkill でスキルを追加し、ClearSkill で削除。
    // Skill() 関数で確認。
    let app = run_with_unit(
        r#"
SetSkill リオ 格闘 3
ClearSkill リオ 格闘
Set c Skill(リオ,格闘)
"#,
    );
    assert_eq!(app.script_var("c"), "0");
}

#[test]
fn clearspecialpower_removes_all_conditions() {
    let app = run_with_unit(
        r#"
SetStatus リオ 熱血
SetStatus リオ 必中
ClearSpecialPower リオ
Set h Condition(リオ,熱血)
Set b Condition(リオ,必中)
"#,
    );
    assert_eq!(app.script_var("h"), "0");
    assert_eq!(app.script_var("b"), "0");
}

#[test]
fn copyarray_copies_indexed_elements() {
    let app = run(r#"
Set src[1] a
Set src[2] b
Set src[3] c
CopyArray src dst
"#);
    assert_eq!(app.script_var("dst[1]"), "a");
    assert_eq!(app.script_var("dst[2]"), "b");
    assert_eq!(app.script_var("dst[3]"), "c");
}

#[test]
fn changemode_sets_ai_mode_string() {
    let mut app = run_with_unit("\n");
    let stmts = event::parse("ChangeMode リオ 待機\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.database().unit_instances[0].ai_mode, "待機");
}

#[test]
fn changemode_party_updates_all_party_units() {
    // リオ (Player) と ガロ (Enemy) を 2 体配置、`ChangeMode 敵 固定` で
    // 敵のみ更新されることを確認
    let src = r#"
Pilot "リオ" リオ 男性 一般 BBBC 50 100 100 100 100 100 100
Pilot "ガロ" ガロ 男性 一般 BBBC 50 100 100 100 100 100 100
Unit G リアル系 1 0 陸 5 M 1000 100 2000 100 1000 100 BBBC
Unit Z リアル系 1 0 陸 5 M 1000 100 2000 100 1000 100 BBBC
Place G リオ Player 1 1
Place Z ガロ Enemy 5 5
ChangeMode 敵 固定
"#;
    let mut app = App::new();
    let stmts = event::parse(src).unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(
        app.database().unit_instances[0].ai_mode,
        "",
        "Player は未変更"
    );
    assert_eq!(
        app.database().unit_instances[1].ai_mode,
        "固定",
        "Enemy のみ 固定"
    );
}

// ============================================================
//  Pass 8: Area / Charge / UseAbility / AutoTalk / Telop / Suspend / BossRank
//          + Partner / CountPartner / IsEquiped / SpecialPower
// ============================================================

// ============================================================
//  Pass 9: MakePilotList / MakeUnitList / Attack / PlayMIDI / Regex
// ============================================================

// ============================================================
//  Pass 10: SelectTarget / Explode / SetRelation / Filters / RankUp
// ============================================================

// ============================================================
//  Pass 11: QuickSave/QuickLoad / Disable / SaveScreen / UseAbility ability
// ============================================================

#[test]
fn quicksave_stores_json_in_quicksave_var() {
    let mut app = run_with_unit("\n");
    let stmts = event::parse("QuickSave\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    let json = app.script_var("__quicksave");
    assert!(!json.is_empty());
    // 復元可能であることを確認
    let restored = src_core::App::from_save_json(json).unwrap();
    assert_eq!(restored.database().unit_instances.len(), 1);
}

#[test]
fn quickload_message_when_no_save() {
    let mut app = App::new();
    let stmts = event::parse("QuickLoad\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    // __quicksave が空なので push_message されない (実装上の判定)
    // 何も起こらない (no_panic) ことを確認
    assert!(app.script_var("__quicksave").is_empty());
}

#[test]
fn disable_two_arg_form_stores_per_unit() {
    // SRC.Sharp 準拠: `Disable unit name` → `Disable(unit,name)` = "1"
    let app = run_with_unit("Disable リオ ライフル\n");
    let v = app.script_var("Disable(リオ,ライフル)");
    assert_eq!(v, "1");
}

#[test]
fn enable_one_arg_form_clears_global() {
    // SRC.Sharp 準拠: `Enable name` は `Disable(name)` を削除する
    let app = run("Disable 機体改造\nEnable 機体改造\n");
    let v = app.script_var("Disable(機体改造)");
    assert_eq!(v, "");
}

#[test]
fn disable_one_arg_form_stores_globally() {
    // SRC.Sharp 準拠: `Disable name` → `Disable(name)` = "1"
    let app = run("Disable 機体改造\n");
    let v = app.script_var("Disable(機体改造)");
    assert_eq!(v, "1");
}

#[test]
fn savescreen_loadscreen_round_trip() {
    let mut app = run_with_unit("\n");
    // 描画コマンドを積む
    let stmts = event::parse("PSet 10, 20\nSaveScreen\nCls\nLoadScreen\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    // Cls で 1 度クリアされた後、LoadScreen で復元されているはず
    let cursor = (app.script_overlay().cursor_x, app.script_overlay().cursor_y);
    assert_eq!(cursor, (10.0, 20.0));
}

#[test]
fn useability_repair_recovers_hp() {
    let mut app = run_with_unit("\n");
    app.database_mut().unit_instances[0].damage = 1500;
    let stmts = event::parse("UseAbility リオ 修理装置\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.database().unit_instances[0].damage, 0);
}

#[test]
fn useability_supply_recovers_en() {
    let mut app = run_with_unit("\n");
    app.database_mut().unit_instances[0].en_consumed = 50;
    let stmts = event::parse("UseAbility リオ 補給装置\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.database().unit_instances[0].en_consumed, 0);
}

#[test]
fn useability_clear_status_removes_conditions() {
    let mut app = run_with_unit("\n");
    app.database_mut().unit_instances[0]
        .add_condition(src_core::condition::Condition::new("毒".to_string(), -1));
    assert!(!app.database().unit_instances[0].conditions.is_empty());
    let stmts = event::parse("UseAbility リオ 状態異常回復\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert!(app.database().unit_instances[0].conditions.is_empty());
}

// ============================================================
//  Pass 12: AbilityEffect 追加効果 + ChangeMode AI 連動
// ============================================================

// ============================================================
//  Pass 13: condition × combat 連動 + life_state + Charge
// ============================================================

// ============================================================
//  Pass 14: Charge 注入 / PaintPicture options / UseAbility 追加
// ============================================================

// ============================================================
//  Pass 15: Restart / ChangeArea / PaintPicture 半分系
// ============================================================

// ============================================================
//  Pass 16: PaintPicture 回転/背景/保持 + 収納イベント
// ============================================================

#[test]
fn paintpicture_parses_rotation_right() {
    use src_core::script_overlay::DrawCmd;
    let mut app = App::new();
    let stmts = event::parse("PaintPicture foo.bmp 0 0 100 100 右回転 45\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    let pic = cmds
        .iter()
        .find(|c| matches!(c, DrawCmd::Picture { .. }))
        .unwrap();
    if let DrawCmd::Picture { rotation_deg, .. } = pic {
        assert_eq!(*rotation_deg, 45.0);
    }
}

#[test]
fn paintpicture_parses_rotation_left_negative() {
    use src_core::script_overlay::DrawCmd;
    let mut app = App::new();
    let stmts = event::parse("PaintPicture foo.bmp 0 0 100 100 左回転 30\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    let pic = cmds
        .iter()
        .find(|c| matches!(c, DrawCmd::Picture { .. }))
        .unwrap();
    if let DrawCmd::Picture { rotation_deg, .. } = pic {
        assert_eq!(*rotation_deg, -30.0, "左回転 N → -N 度");
    }
}

#[test]
fn paintpicture_parses_background_and_persist() {
    use src_core::script_overlay::DrawCmd;
    let mut app = App::new();
    let stmts = event::parse("PaintPicture foo.bmp 0 0 100 100 背景 保持\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    let pic = cmds
        .iter()
        .find(|c| matches!(c, DrawCmd::Picture { .. }))
        .unwrap();
    if let DrawCmd::Picture {
        as_background,
        persist,
        ..
    } = pic
    {
        assert!(*as_background);
        assert!(*persist);
    }
}

#[test]
fn stow_command_fires_boarding_event_and_sets_state() {
    let src = r#"
Pilot "リオ" リオ 男性 一般 BBBC 50 100 100 100 100 100 100
Pilot "ノヴァ" ノヴァ 男性 一般 BBBC 50 100 100 100 100 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 2000 100 800 100 BBBC
Unit アークシップ リアル系 1 0 陸 5 M 1000 100 5000 100 1500 100 BBBC
Place ブレイバー リオ Player 1 1
Place アークシップ ノヴァ Player 3 3
Stow リオ ノヴァ
Exit

収納 リオ:
Set boarded 1
Return
"#;
    let mut app = App::new();
    let stmts = event::parse(src).unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    // 収納 ラベルが発火
    assert_eq!(app.script_var("boarded"), "1");
    // life_state = "格納" にセット
    let amuro_unit = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.pilot_name == "リオ")
        .unwrap();
    assert_eq!(amuro_unit.life_state, "格納");
    assert!(amuro_unit.off_map);
    // 相手パイロット システム変数が母艦パイロット名
    assert_eq!(app.script_var("相手パイロット"), "ノヴァ");
}

#[test]
fn changearea_sets_current_area() {
    let mut app = run_with_unit("\n");
    let stmts = event::parse("ChangeArea リオ 水中\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.database().unit_instances[0].current_area, "水中");
}

#[test]
fn restart_clears_quicksave_when_restart_save_exists() {
    let mut app = run_with_unit("\n");
    app.set_script_var("__restart_save".to_string(), "{}".to_string());
    app.set_script_var("__quicksave".to_string(), "{}".to_string());
    let stmts = event::parse("Restart\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    // Restart 後は __quicksave がクリアされる (SRC 仕様: QuickLoad 無効化)
    assert!(app.script_var("__quicksave").is_empty());
}

#[test]
fn paintpicture_parses_half_mode() {
    use src_core::script_overlay::DrawCmd;
    let mut app = App::new();
    let stmts = event::parse("PaintPicture foo.bmp 0 0 100 100 上半分\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    let pic = cmds
        .iter()
        .find(|c| matches!(c, DrawCmd::Picture { .. }))
        .unwrap();
    if let DrawCmd::Picture { half_mode, .. } = pic {
        assert_eq!(half_mode, "上半分");
    }
}

#[test]
fn paintpicture_parses_corner_mode() {
    use src_core::script_overlay::DrawCmd;
    let mut app = App::new();
    let stmts = event::parse("PaintPicture foo.bmp 0 0 100 100 右上\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    let pic = cmds
        .iter()
        .find(|c| matches!(c, DrawCmd::Picture { .. }))
        .unwrap();
    if let DrawCmd::Picture { half_mode, .. } = pic {
        assert_eq!(half_mode, "右上");
    }
}

#[test]
fn useability_haunt_transfers_pilot() {
    let src = r#"
Pilot "幽霊" 幽霊 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot "本体" 本体 男性 一般 BBBC 50 100 100 100 100 100 100
Unit G リアル系 1 0 陸 5 M 1000 100 2000 100 800 100 BBBC
Unit Z リアル系 1 0 陸 5 M 1000 100 2000 100 800 100 BBBC
Place G 幽霊 Player 1 1
Place Z 本体 Player 5 5
UseAbility 幽霊 憑依 本体
"#;
    let mut app = App::new();
    let stmts = event::parse(src).unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    // 幽霊 unit は pilot_name="" に、本体 unit に 幽霊 が乗っている
    let units = &app.database().unit_instances;
    let src_unit = units.iter().find(|u| u.unit_data_name == "G").unwrap();
    let dst_unit = units.iter().find(|u| u.unit_data_name == "Z").unwrap();
    assert!(src_unit.pilot_name.is_empty(), "憑依元 pilot は空");
    assert_eq!(dst_unit.pilot_name, "幽霊", "憑依先 pilot は転送元");
}

#[test]
fn useability_morale_increase() {
    let mut app = run_with_unit("\n");
    app.database_mut().unit_instances[0].morale = 100;
    let stmts = event::parse("UseAbility リオ 気力増加 リオ\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.database().unit_instances[0].morale, 110);
}

#[test]
fn paintpicture_parses_flip_y_and_monochrome() {
    use src_core::script_overlay::DrawCmd;
    let mut app = App::new();
    let stmts = event::parse("PaintPicture foo.bmp 10 20 100 100 上下反転 白黒\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    let pic = cmds
        .iter()
        .find(|c| matches!(c, DrawCmd::Picture { .. }))
        .unwrap();
    if let DrawCmd::Picture {
        flip_y,
        monochrome,
        sepia,
        ..
    } = pic
    {
        assert!(*flip_y);
        assert!(*monochrome);
        assert!(!*sepia);
    }
}

#[test]
fn paintpicture_parses_sepia() {
    use src_core::script_overlay::DrawCmd;
    let mut app = App::new();
    let stmts = event::parse("PaintPicture foo.bmp 0 0 100 100 透過 セピア\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    let pic = cmds
        .iter()
        .find(|c| matches!(c, DrawCmd::Picture { .. }))
        .unwrap();
    if let DrawCmd::Picture {
        transparent,
        sepia,
        monochrome,
        ..
    } = pic
    {
        assert!(*transparent);
        assert!(*sepia);
        assert!(!*monochrome);
    }
}

#[test]
fn barrier_reduces_damage_by_half() {
    use src_core::combat;
    use src_core::data::pilot::{Adaption, PilotData, Sex};
    use src_core::data::unit::{Size, UnitData, WeaponData};

    let pilot = PilotData {
        spirit_commands: Vec::new(),
        name: "A".into(),
        nickname: "A".into(),
        kana_name: "A".into(),
        sex: Sex::Unspecified,
        class: String::new(),
        adaption: Adaption::parse("AAAA").unwrap(),
        exp_value: 0,
        infight: 100,
        shooting: 100,
        hit: 100,
        dodge: 100,
        intuition: 100,
        technique: 100,
        personality: None,
        sp: None,
        bgm: None,
        bitmap: None,
        features: Vec::new(),
    };
    let unit = UnitData {
        abilities: Vec::new(),
        name: "U".into(),
        kana_name: "U".into(),
        nickname: "U".into(),
        class: "リアル系".into(),
        pilot_num: 1,
        item_num: 0,
        transportation: "陸".into(),
        speed: 5,
        size: Size::M,
        value: 0,
        exp_value: 0,
        hp: 3000,
        en: 100,
        armor: 200,
        mobility: 100,
        adaption: Adaption::parse("AAAA").unwrap(),
        bitmap: String::new(),
        weapons: vec![],
        features: Vec::new(),
    };
    let weapon = WeaponData {
        name: "W".into(),
        power: 2000,
        min_range: 1,
        max_range: 3,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: String::new(),
        critical: 0,
        class: String::new(),
        extras: vec![],
    };
    let no_barrier = combat::predict_with_status(
        &pilot,
        &unit,
        &weapon,
        &pilot,
        &unit,
        0,
        0,
        100,
        100,
        &[],
        &[],
    );
    let with_barrier = combat::predict_with_status(
        &pilot,
        &unit,
        &weapon,
        &pilot,
        &unit,
        0,
        0,
        100,
        100,
        &[],
        &["バリア".to_string()],
    );
    // Barrier halves the damage
    assert_eq!(with_barrier.damage * 2, no_barrier.damage);
}

#[test]
fn afterimage_reduces_hit_chance() {
    use src_core::combat;
    use src_core::data::pilot::{Adaption, PilotData, Sex};
    use src_core::data::unit::{Size, UnitData, WeaponData};

    let pilot = PilotData {
        spirit_commands: Vec::new(),
        name: "A".into(),
        nickname: "A".into(),
        kana_name: "A".into(),
        sex: Sex::Unspecified,
        class: String::new(),
        adaption: Adaption::parse("AAAA").unwrap(),
        exp_value: 0,
        infight: 100,
        shooting: 100,
        hit: 50,
        dodge: 50,
        intuition: 100,
        technique: 100,
        personality: None,
        sp: None,
        bgm: None,
        bitmap: None,
        features: Vec::new(),
    };
    let unit = UnitData {
        abilities: Vec::new(),
        name: "U".into(),
        kana_name: "U".into(),
        nickname: "U".into(),
        class: "リアル系".into(),
        pilot_num: 1,
        item_num: 0,
        transportation: "陸".into(),
        speed: 5,
        size: Size::M,
        value: 0,
        exp_value: 0,
        hp: 3000,
        en: 100,
        armor: 200,
        mobility: 100,
        adaption: Adaption::parse("AAAA").unwrap(),
        bitmap: String::new(),
        weapons: vec![],
        features: Vec::new(),
    };
    let weapon = WeaponData {
        name: "W".into(),
        power: 2000,
        min_range: 1,
        max_range: 3,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: String::new(),
        critical: 0,
        class: String::new(),
        extras: vec![],
    };
    let normal = combat::predict_with_status(
        &pilot,
        &unit,
        &weapon,
        &pilot,
        &unit,
        0,
        0,
        100,
        100,
        &[],
        &[],
    );
    let afterimage = combat::predict_with_status(
        &pilot,
        &unit,
        &weapon,
        &pilot,
        &unit,
        0,
        0,
        100,
        100,
        &[],
        &["分身".to_string()],
    );
    assert!(afterimage.hit_chance < normal.hit_chance);
    // Should be reduced by 40 (clamped at 5..=95)
    assert!(normal.hit_chance - afterimage.hit_chance <= 40);
}

#[test]
fn leave_command_sets_life_state_to_left() {
    let mut app = run_with_unit("\n");
    let stmts = event::parse("Leave リオ\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    let u = &app.database().unit_instances[0];
    assert_eq!(u.life_state, "離脱");
    let stmts = event::parse("Set s Status(リオ)\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("s"), "離脱");
}

#[test]
fn status_returns_stored_when_life_state_is_stored() {
    let mut app = run_with_unit("\n");
    app.database_mut().unit_instances[0].life_state = "格納".to_string();
    let stmts = event::parse("Set s Status(リオ)\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("s"), "格納");
}

#[test]
fn charge_weapon_filtered_when_not_charged() {
    use src_core::combat;
    use src_core::data::pilot::Adaption;
    use src_core::data::unit::{Size, UnitData, WeaponData};

    let unit = UnitData {
        abilities: Vec::new(),
        name: "U".into(),
        kana_name: "U".into(),
        nickname: "U".into(),
        class: "リアル系".into(),
        pilot_num: 1,
        item_num: 0,
        transportation: "陸".into(),
        speed: 5,
        size: Size::M,
        value: 0,
        exp_value: 0,
        hp: 3000,
        en: 100,
        armor: 200,
        mobility: 100,
        adaption: Adaption::parse("AAAA").unwrap(),
        bitmap: String::new(),
        weapons: vec![
            WeaponData {
                name: "通常".into(),
                power: 1000,
                min_range: 1,
                max_range: 3,
                precision: 0,
                bullet: -1,
                en_consumption: 0,
                necessary_morale: 0,
                adaption: String::new(),
                critical: 0,
                class: String::new(),
                extras: vec![],
            },
            WeaponData {
                name: "チャージ".into(),
                power: 5000,
                min_range: 1,
                max_range: 3,
                precision: 0,
                bullet: -1,
                en_consumption: 0,
                necessary_morale: 0,
                adaption: String::new(),
                critical: 0,
                class: "Ｃ".into(),
                extras: vec![],
            },
        ],
        features: Vec::new(),
    };
    let uncharged = combat::best_weapon_in_range_with_charge(&unit, 2, false);
    assert_eq!(
        uncharged.map(|w| w.name.as_str()),
        Some("通常"),
        "未チャージ時は Ｃ 武器を除外"
    );
    let charged = combat::best_weapon_in_range_with_charge(&unit, 2, true);
    assert_eq!(
        charged.map(|w| w.name.as_str()),
        Some("チャージ"),
        "チャージ済なら強力 Ｃ 武器を選択"
    );
    // 既存 API (charge 非対応) は常に Ｃ を除外
    let legacy = combat::best_weapon_in_range(&unit, 2);
    assert_eq!(legacy.map(|w| w.name.as_str()), Some("通常"));
}

#[test]
fn useability_barrier_adds_condition() {
    let mut app = run_with_unit("\n");
    let stmts = event::parse("UseAbility リオ バリア展開\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert!(app.database().unit_instances[0].has_condition("バリア"));
}

#[test]
fn useability_concentration_adds_condition() {
    let mut app = run_with_unit("\n");
    let stmts = event::parse("UseAbility リオ 集中\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert!(app.database().unit_instances[0].has_condition("集中"));
}

#[test]
fn useability_combo_attack_updates_partner_state() {
    let app = run_with_unit(
        r#"
UseAbility リオ 合体技 キャノス
"#,
    );
    assert_eq!(app.script_var("直前合体技ユニット"), "リオ");
    assert_eq!(app.script_var("直前合体技パートナー数"), "1");
    assert_eq!(app.script_var("直前合体技パートナー[1]"), "キャノス");
}

#[test]
fn fire_victory_condition_event_via_app_helper() {
    let mut app = run_with_unit(
        r#"
Exit

勝利条件:
Set vc_fired 1
Return
"#,
    );
    assert!(app.has_victory_condition_event());
    assert!(app.fire_victory_condition_event());
    assert_eq!(app.script_var("vc_fired"), "1");
}

#[test]
fn selecttarget_sets_system_vars() {
    let src = r#"
Pilot "リオ" リオ 男性 一般 BBBC 50 100 100 100 100 100 100
Pilot "ガロ" ガロ 男性 一般 BBBC 50 100 100 100 100 100 100
Unit G リアル系 1 0 陸 5 M 1000 100 2000 100 1000 100 BBBC
Unit Z Mass 1 0 陸 5 M 1000 100 2000 100 1000 100 BBBC
Place G リオ Player 1 1
Place Z ガロ Enemy 5 5
SelectTarget ガロ
Set p $(相手パイロット)
"#;
    let mut app = App::new();
    let stmts = event::parse(src).unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("相手パイロット"), "ガロ");
}

#[test]
fn explode_pushes_overlay_commands() {
    let mut app = run_with_unit("\n");
    let before = app.script_overlay().cmds.len();
    let stmts = event::parse("Explode L 3 3\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    let after = app.script_overlay().cmds.len();
    assert!(after > before, "Explode should push overlay commands");
}

#[test]
fn setrelation_relation_round_trip() {
    let app = run(r#"
SetRelation リオ ガロ 50
Set r1 Relation(リオ,ガロ)
Set r2 Relation(ガロ,リオ)
Set r3 Relation(リオ,タク)
"#);
    assert_eq!(app.script_var("r1"), "50");
    assert_eq!(app.script_var("r2"), "50", "対称関係");
    assert_eq!(app.script_var("r3"), "0", "未設定なら 0");
}

#[test]
fn rankup_increments_unit_rank() {
    let app = run(r#"
RankUp リオ
RankUp リオ 2
Set r Rank(リオ)
"#);
    assert_eq!(app.script_var("r"), "3", "1 + 2 = 3");
}

#[test]
fn sepia_pushes_fade_command() {
    let mut app = run_with_unit("\n");
    let before = app.script_overlay().cmds.len();
    let stmts = event::parse("Sepia\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    let after = app.script_overlay().cmds.len();
    assert!(after > before, "Sepia should push DrawCmd::Fade");
}

#[test]
fn whitein_reveals_by_removing_whiteout_fade() {
    // `WhiteIn` は白→通常画面へのフェードイン。終状態は通常画面なので、
    // `WhiteOut` が積んだ白の全画面 Fade を除去して露出する。
    // (旧実装は WhiteIn でも白 Fade を積み、引数なし WhiteIn が画面を白で
    //  覆ったまま残して「白いマップ」で操作不能になっていた。)
    let is_white = |c: &src_core::script_overlay::DrawCmd| matches!(c, src_core::script_overlay::DrawCmd::Fade { color, .. } if color == "#ffffff");
    let mut app = run_with_unit("\n");
    event_runtime::execute(&mut app, &event::parse("WhiteOut 255\n").unwrap()).unwrap();
    assert!(
        app.script_overlay().cmds.iter().any(is_white),
        "WhiteOut は白 Fade を積む"
    );
    event_runtime::execute(&mut app, &event::parse("WhiteIn\n").unwrap()).unwrap();
    assert!(
        !app.script_overlay().cmds.iter().any(is_white),
        "WhiteIn は白 Fade を除去して画面を露出する"
    );
}

#[test]
fn makepilotlist_sorts_by_attribute() {
    // 2 pilot 定義し SP でソート
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot "ガロ" ガロ 男性 一般 BBBC 50 150 150 150 150 150 150
MakePilotList 格闘
Set p1 $(パイロットリスト[1])
Set p2 $(パイロットリスト[2])
Set n $(パイロットリスト数)
"#);
    // ガロ (格闘 150) > リオ (格闘 100)
    assert_eq!(app.script_var("p1"), "ガロ");
    assert_eq!(app.script_var("p2"), "リオ");
    assert_eq!(app.script_var("n"), "2");
}

#[test]
fn makeunitlist_sorts_by_attribute() {
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot "ガロ" ガロ 男性 一般 AAAA 100 100 100 100 100 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2500 110 1000 100 BBBC
Place ブレイバー リオ Player 1 1
Place ゾルダ ガロ Enemy 2 2
MakeUnitList ＨＰ
Set u1 $(ユニットリスト[1])
Set u2 $(ユニットリスト[2])
"#);
    // ブレイバー (HP 3500) > ゾルダ (HP 2500)。
    // MakeUnitList は一意 uid を格納する (U1=ブレイバー, U2=ゾルダ; Place 順に採番)。
    assert_eq!(app.script_var("u1"), "U1");
    assert_eq!(app.script_var("u2"), "U2");
}

#[test]
fn makeunitlist_sorts_by_morale() {
    // IncreaseMorale で差をつけて 気力 順ソート
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot "ガロ" ガロ 男性 一般 AAAA 100 100 100 100 100 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2500 110 1000 100 BBBC
Place ブレイバー リオ Player 1 1
Place ゾルダ ガロ Enemy 2 2
IncreaseMorale ガロ 20
MakeUnitList 気力
Set u1 $(ユニットリスト[1])
Set u2 $(ユニットリスト[2])
"#);
    // ガロ (気力 120) > リオ (気力 100)。
    // MakeUnitList は一意 uid を格納する (U1=ブレイバー, U2=ゾルダ; Place 順に採番)。
    assert_eq!(app.script_var("u1"), "U2");
    assert_eq!(app.script_var("u2"), "U1");
}

#[test]
fn makeunitlist_number_reflects_unit_count() {
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot "ガロ" ガロ 男性 一般 AAAA 100 100 100 100 100 100 100
Pilot "ノヴァ" ノヴァ 男性 一般 AAAA 100 100 100 100 100 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2500 110 1000 100 BBBC
Unit アークシップ スーパーロボット系 1 0 陸 5 L 1000 100 5000 200 2000 50 BBBC
Place ブレイバー リオ Player 1 1
Place ゾルダ ガロ Enemy 2 2
Place アークシップ ノヴァ Allied 3 3
MakeUnitList ＨＰ
Set n $(ユニットリスト数)
"#);
    assert_eq!(app.script_var("n"), "3");
}

#[test]
fn attack_command_applies_damage_to_target() {
    // unit1=リオ(ブレイバー) attacks unit2=ガロ(ゾルダ)
    let src = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot "ガロ" ガロ 男性 一般 BBBC 50 100 100 100 100 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
Weapon "ブレイバー" "ライフル" 2500 2 5 15 -1
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2500 110 800 100 BBBC
Place ブレイバー リオ Player 1 1
Place ゾルダ ガロ Enemy 5 5
Attack リオ ライフル ガロ 無抵抗
"#;
    let mut app = App::new();
    let stmts = event::parse(src).unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    // SRC式: weapon.power(2500) * shooting(100)/100 * morale(100)/100 - armor(800) * morale(100)/100 = 1700
    let zolda = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "ゾルダ")
        .unwrap();
    assert_eq!(zolda.damage, 1700, "Attack コマンドのダメージが適用される");
    // ブレイバーは無抵抗なので damage は 0
    let braver = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "ブレイバー")
        .unwrap();
    assert_eq!(braver.damage, 0, "無抵抗 なら反撃ダメージ無し");
}

#[test]
fn attack_command_no_destruction_event_fires() {
    // 過剰ダメージでも Destruction ラベルは Attack 経由では発火しない
    let src = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot "ガロ" ガロ 男性 一般 BBBC 50 100 100 100 100 100 100
Unit G リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
Weapon "G" "強撃" 99999 1 5 100 -1
Unit Z Mass 1 0 陸 5 M 1000 100 100 110 0 100 BBBC
Place G リオ Player 1 1
Place Z ガロ Enemy 5 5
Attack リオ 強撃 ガロ 無抵抗
Exit

Destruction ガロ:
Set destroyed 1
Return
"#;
    let mut app = App::new();
    let stmts = event::parse(src).unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    // SRC 仕様: Attack 命令経由では Destruction は発火しない
    assert_eq!(app.script_var("destroyed"), "");
}

#[test]
fn playmidi_pushes_audio_request() {
    let mut app = App::new();
    let stmts = event::parse("PlayMIDI Subtitle.mid\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    let reqs = app.take_pending_audio();
    assert!(reqs.iter().any(|r| matches!(r, src_core::audio::AudioRequest::PlayMidi { name } if name.contains("Subtitle"))));
}

#[test]
fn regexp_matches_pattern() {
    // SRC RegExp returns the matched string (not "1"/"0")
    let app = run(r#"
Set a RegExp("test123","[0-9]+")
Set b RegExp("hello","[0-9]+")
"#);
    assert_eq!(app.script_var("a"), "123"); // matched substring
    assert_eq!(app.script_var("b"), ""); // no match → ""
}

#[test]
fn regexp_case_sensitive_by_default() {
    // SRC default is 大小区別あり (case-sensitive) per 正規表現関数.md
    let app = run(r#"
Set a RegExp("Hello","hello")
"#);
    assert_eq!(app.script_var("a"), ""); // no match (case-sensitive)
}

#[test]
fn regexp_case_insensitive_option() {
    let app = run(r#"
Set a RegExp("Hello","hello",大小区別なし)
"#);
    assert_eq!(app.script_var("a"), "Hello"); // match (case-insensitive)
}

#[test]
fn regexp_replace_substitutes() {
    let app = run(r#"
Set a RegExpReplace("abc123def","[0-9]+","X")
"#);
    assert_eq!(app.script_var("a"), "abcXdef");
}

#[test]
fn regexp_returns_first_match() {
    // 正規表現関数.md 例: RegExp("あいあうあえあお","あ.") → "あい"
    let app = run(r#"Set v RegExp("あいあうあえあお","あ.")"#);
    assert_eq!(app.script_var("v"), "あい");
}

#[test]
fn regexp_case_sensitive_does_not_match_different_case() {
    let app = run(r#"Set v RegExp("Hello World","world")"#);
    assert_eq!(app.script_var("v"), ""); // no match — case sensitive by default
}

#[test]
fn regexp_case_insensitive_matches_different_case() {
    let app = run(r#"Set v RegExp("Hello World","world",大小区別なし)"#);
    assert_eq!(app.script_var("v"), "World"); // matches
}

#[test]
fn regexp_replace_case_sensitive_by_default() {
    // RegExpReplace default is 大小区別あり (case-sensitive)
    let app = run(r#"Set v RegExpReplace("Hello World","world","SRC")"#);
    assert_eq!(app.script_var("v"), "Hello World"); // no replacement (no case-insensitive match)
}

#[test]
fn regexp_replace_case_insensitive_option() {
    let app = run(r#"Set v RegExpReplace("Hello World","world","SRC",大小区別なし)"#);
    assert_eq!(app.script_var("v"), "Hello SRC");
}

#[test]
fn area_command_overrides_terrain_class() {
    // 平地 (陸クラス) に配置された地上ユニット → Area() = "地上"。
    // SRC 仕様: Area() は地形クラス名ではなくユニットの現在領域を返す。
    // 空中 コマンド後は Area() = "空中" を返す。
    let mut app = run_with_unit("MapSize 5 5\n");
    let stmts = event::parse("Set a Area(リオ)\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    // 陸移動ユニットが "平地"(陸クラス) にいる → "地上"
    assert_eq!(app.script_var("a"), "地上");

    let stmts = event::parse("空中 リオ\nSet b Area(リオ)\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("b"), "空中");
}

#[test]
fn charge_sets_charged_flag() {
    let mut app = run_with_unit("\n");
    let stmts = event::parse("Charge リオ\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert!(app.database().unit_instances[0].charged);
}

#[test]
fn useability_stores_last_ability() {
    let app = run_with_unit("UseAbility リオ 念力\n");
    assert_eq!(app.script_var("直前使用アビリティ"), "念力");
}

#[test]
fn telop_prefixes_message() {
    let app = run("Telop ドキドキ.次回もお楽しみに\n");
    // .  は改行に変換、テロップ接頭辞
    let messages = app.messages();
    assert!(
        messages
            .iter()
            .any(|m| m.contains("【テロップ】") && m.contains("ドキドキ") && m.contains("\n")),
        "telop message expected; got: {messages:?}"
    );
}

#[test]
fn suspend_returns_to_title() {
    let mut app = run_with_unit("\n");
    app.set_scene(src_core::Scene::MapView);
    let stmts = event::parse("Suspend\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.scene(), src_core::Scene::Title);
}

#[test]
fn bossrank_stores_rank_in_script_var() {
    let app = run_with_unit("BossRank リオ 3\nSet r Rank(リオ)\n");
    assert_eq!(app.script_var("r"), "3");
}

#[test]
fn partner_returns_self_for_zero() {
    let app = run(r#"
Set 直前合体技ユニット U1
Set p Partner(0)
"#);
    assert_eq!(app.script_var("p"), "U1");
}

#[test]
fn countpartner_counts_indexed_partners() {
    let app = run(r#"
Set 直前合体技パートナー[1] U2
Set 直前合体技パートナー[2] U3
Set 直前合体技パートナー[3] U4
Set c CountPartner()
"#);
    assert_eq!(app.script_var("c"), "3");
}

#[test]
fn isequiped_checks_equipped_item() {
    // 装備品なしの場合は 0
    let app = run_with_unit("Set e IsEquiped(リオ,ビームライフル)\n");
    assert_eq!(app.script_var("e"), "0");
}

#[test]
fn specialpower_checks_active_buff() {
    let app = run_with_unit(
        r#"
SetStatus リオ 熱血
Set s SpecialPower(リオ,熱血)
Set t SpecialPower(リオ,鉄壁)
"#,
    );
    assert_eq!(app.script_var("s"), "1");
    assert_eq!(app.script_var("t"), "0");
}

#[test]
fn setmessage_stores_to_script_var() {
    let app = run(r#"
SetMessage 攻撃 必殺技
Set m $(次戦闘メッセージ_攻撃)
"#);
    assert_eq!(app.script_var("m"), "必殺技");
}

#[test]
fn changeterrain_modifies_tile_terrain_id() {
    // 海(2) は DEFAULT_TERRAINS にあるはず
    let mut app = run_with_unit("MapSize 6 6\n");
    let stmts = event::parse("ChangeTerrain 3 3 海 0\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    let map = app.database().map.as_ref().unwrap();
    let cell = map.cell(3, 3);
    // 海 = 4 (組込テーブル DEFAULT_TERRAINS)
    assert_eq!(cell.terrain_id, 4, "海地形 ID は 4");
}

// ============================================================
//  TextWidth / TextHeight / GetTime
// ============================================================

#[test]
fn textwidth_returns_char_count_times_14() {
    // "hello" = 5 文字 → 5 × 14 = 70 ピクセル
    let app = run(r#"Set v TextWidth("hello")"#);
    assert_eq!(app.script_var("v"), "70");
}

#[test]
fn textwidth_japanese_returns_char_count_times_14() {
    // "あいう" = 3 文字 → 3 × 14 = 42
    let app = run(r#"Set v TextWidth("あいう")"#);
    assert_eq!(app.script_var("v"), "42");
}

#[test]
fn textwidth_empty_returns_zero() {
    let app = run(r#"Set v TextWidth("")"#);
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn textheight_always_returns_20() {
    let app = run(r#"Set v TextHeight("anything")"#);
    assert_eq!(app.script_var("v"), "20");
}

#[test]
fn gettime_returns_numeric_in_test_env() {
    // テスト環境では wall_clock_ms = 0 → GetTime() = 0
    let app = run("Set v GetTime()");
    let v: i64 = app.script_var("v").parse().expect("numeric");
    assert!(v >= 0);
}

// ============================================================
//  IIf: 条件式 (比較演算子) を正しく評価する
// ============================================================

#[test]
fn iif_condition_with_comparison_greater_than() {
    // SRC.Sharp 準拠: IIf(5 > 3, "yes", "no") → "yes"
    // UtilityFunctionTests.IIfWithComparisonCondition 準拠
    let app = run(r#"Set v IIf(5 > 3, "yes", "no")"#);
    assert_eq!(app.script_var("v"), "yes");
}

#[test]
fn iif_condition_with_comparison_false() {
    // SRC.Sharp 準拠: IIf(5 < 3, 100, 200) → 200
    let app = run("Set v IIf(5 < 3, 100, 200)");
    assert_eq!(app.script_var("v"), "200");
}

#[test]
fn iif_condition_with_variable() {
    // 変数を含む条件式も評価できる
    let app = run("Set x 10\nSet v IIf(x = 10, ok, ng)");
    assert_eq!(app.script_var("v"), "ok");
}

// ============================================================
//  Log 関数
// ============================================================

#[test]
fn log_of_e_is_one() {
    // ln(e) = 1.0
    use std::f64::consts::E;
    let app = run(&format!("Set v Log({E})"));
    let v: f64 = app.script_var("v").parse().expect("numeric");
    assert!((v - 1.0).abs() < 1e-6, "Log(e) should be 1.0, got {v}");
}

#[test]
fn log_of_one_is_zero() {
    // ln(1) = 0
    let app = run("Set v Log(1)");
    assert_eq!(app.script_var("v"), "0");
}

// ============================================================
//  AutoTalk コマンド
// ============================================================

#[test]
fn autotalk_fires_conversation_label_for_adjacent_units() {
    // 隣接した 2 ユニットが `会話 リオ ガロ:` ラベルを発火する。
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot "ガロ" ガロ 男性 一般 AAAA 100 100 100 100 100 100 100
Unit "ブレイバー" リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Unit "ゾルダ" Mass 1 0 陸 5 M 1000 100 2500 100 900 100 AAAA
MapSize 5 5
Place "ブレイバー" "リオ" Player 2 2
Place "ゾルダ" "ガロ" Enemy 2 3
AutoTalk
Exit

会話 リオ ガロ:
Set talked yes
Return
"#);
    assert_eq!(
        app.script_var("talked"),
        "yes",
        "AutoTalk で 会話ラベルが発火する"
    );
}

#[test]
fn autotalk_does_not_fire_for_non_adjacent_units() {
    // 離れたユニットでは 会話ラベルは発火しない。
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot "ガロ" ガロ 男性 一般 AAAA 100 100 100 100 100 100 100
Unit "ブレイバー" リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Unit "ゾルダ" Mass 1 0 陸 5 M 1000 100 2500 100 900 100 AAAA
MapSize 10 10
Place "ブレイバー" "リオ" Player 0 0
Place "ゾルダ" "ガロ" Enemy 9 9
AutoTalk
Exit

会話 リオ ガロ:
Set talked yes
Return
"#);
    assert_eq!(
        app.script_var("talked"),
        "",
        "離れたユニットでは 会話ラベルは発火しない"
    );
}

// ============================================================
//  Require — .ini 設定ファイルの取り込み
// ============================================================

/// .ini ファイルを script_library に登録した App で eve を実行する。
fn run_with_required_file(ini_content: &str, eve: &str) -> App {
    let mut app = App::new();
    let ini_stmts = event::parse(ini_content).expect("parse ini");
    app.script_library_mut()
        .append_with_name(&ini_stmts, "theme.ini");
    let stmts = event::parse(eve).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

#[test]
fn require_applies_ini_key_values_as_script_vars() {
    // Require は対象ファイルの `key = value` 行をスクリプト変数へ取り込む。
    let app = run_with_required_file(
        "LetterColor = \"255,0,0\"\nFrameColor = \"0,0,255\"\n",
        "Require \"theme.ini\"\n",
    );
    assert_eq!(app.script_var("LetterColor"), "255,0,0");
    assert_eq!(app.script_var("FrameColor"), "0,0,255");
}

#[test]
fn require_strips_surrounding_quotes() {
    // 値の前後のダブルクォートは除去される。
    let app = run_with_required_file("Title = \"サンプルゲーム\"\n", "Require \"theme.ini\"\n");
    assert_eq!(app.script_var("Title"), "サンプルゲーム");
}

#[test]
fn require_unknown_file_is_noop() {
    // 登録されていないファイルを Require しても何も起きない (パニックしない)。
    let app = run_with_required_file("Foo = \"bar\"\n", "Require \"missing.ini\"\n");
    assert_eq!(app.script_var("Foo"), "", "未登録ファイルは取り込まれない");
}
