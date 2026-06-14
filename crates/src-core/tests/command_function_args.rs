//! コマンド引数に関数呼び出しを渡したときの配線テスト。
//!
//! `.eve` の各コマンドは引数を `expand_vars` で展開してから dispatch する。
//! 関数呼び出し (`Max(...)` / `Lindex(...)` / `Info(...)` 等) や `$(Func(...))`
//! が **コマンド引数として** 正しく評価されるかを網羅する。
//!
//! スパロボ戦記タイトルの `Set ユニット画像[i]
//! "Anime\Unit\$(Lindex(タイトル画面アクション[i],1))"` で
//! `$(...)` 内の関数が評価されず画像パスが空になっていた不具合
//! (`expand_dollar_paren`) の回帰防止を主目的とする。
//!
//! 著作権配慮: SRC オリジナルコードを含まない。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn run(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

/// HP / Damage 系テスト用の最小ユニット定義。ブレイバー HP=3500。
const PRELUDE: &str = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
MapSize 10 10
Place "ブレイバー" "リオ" Player 2 2
"#;

fn run_with_units(extra: &str) -> App {
    run(&format!("{PRELUDE}{extra}"))
}

// ============================================================
//  Set — 関数を値として受ける基本形
// ============================================================

#[test]
fn set_value_is_a_bare_function_call() {
    let app = run(r#"Set v Max(10, 25)"#);
    assert_eq!(app.script_var("v"), "25");
}

#[test]
fn set_value_is_a_nested_function_call() {
    // 関数の引数がさらに関数。
    let app = run(r#"Set v Abs(Min(-5, -2))"#);
    assert_eq!(app.script_var("v"), "5");
}

#[test]
fn set_value_function_with_dollar_var_argument() {
    // 関数の引数に `$(var)` を含む。
    let app = run("Set n 3\nSet v Abs($(n) - 10)\n");
    assert_eq!(app.script_var("v"), "7");
}

// ============================================================
//  `$(Func(...))` — 文字列補間の中での関数評価
//  (expand_dollar_paren の主対象)
// ============================================================

#[test]
fn dollar_paren_function_evaluated_in_string_arg() {
    let app = run(r#"Set msg "ans=$(Max(3,9))""#);
    assert_eq!(app.script_var("msg"), "ans=9");
}

#[test]
fn dollar_paren_lindex_of_indexed_var() {
    // スパロボ戦記タイトルの画像パス構築パターン:
    // `Set ユニット画像[i] "…\$(Lindex(タイトル画面アクション[i],1))"`。
    // `$(...)` 内の関数 + その引数のインデックス変数 (`配列[キー]`) を解決。
    let app = run("Set 候補[き] List(あ,い,う)\nSet r \"x_$(Lindex(候補[き],1))\"\n");
    assert_eq!(app.script_var("r"), "x_あ");
}

#[test]
fn dollar_paren_info_call_in_string_arg() {
    // `$(Info(...))` も関数として評価される (旧実装は `[` を含むと
    // 変数キー lookup に落ちて空文字になっていた)。
    let app = run_with_units("Set s \"hp=$(Info(ユニットデータ,ブレイバー,最大ＨＰ))\"\n");
    assert_eq!(app.script_var("s"), "hp=3500");
}

// ============================================================
//  インデックス変数 × 関数
// ============================================================

#[test]
fn lindex_of_indexed_var_as_command_arg() {
    let app = run("Set 候補[き] List(あ,い,う)\nSet v Lindex(候補[き],2)\n");
    assert_eq!(app.script_var("v"), "い");
}

#[test]
fn set_lhs_index_is_a_function_call() {
    // Set LHS の `name[expr]` の expr が関数。
    let app = run("Set arr[Max(1, 2)] hello\nSet v $(arr[2])\n");
    assert_eq!(app.script_var("v"), "hello");
}

// ============================================================
//  実コマンドの引数としての関数
// ============================================================

#[test]
fn damage_amount_from_function() {
    let app = run_with_units("Damage ブレイバー Max(500, 100)\nSet hp HP(ブレイバー)\n");
    // 3500 - 500 = 3000
    assert_eq!(app.script_var("hp"), "3000");
}

#[test]
fn heal_amount_from_function() {
    let app = run_with_units(
        "Damage ブレイバー 2000\nHeal ブレイバー Min(800, 5000)\nSet hp HP(ブレイバー)\n",
    );
    // 3500 - 2000 + 800 = 2300
    assert_eq!(app.script_var("hp"), "2300");
}

#[test]
fn moveunit_coords_from_functions() {
    let app = run_with_units("MoveUnit リオ Max(2, 5) Min(8, 3)\n");
    let u = &app.database().unit_instances[0];
    assert_eq!((u.x, u.y), (5, 3));
}

#[test]
fn money_delta_from_function() {
    let app = run("Money +Max(100, 250)\n");
    assert_eq!(app.money(), 250);
}

#[test]
fn incr_delta_from_function() {
    let app = run("Set c 5\nIncr c Abs(-3)\n");
    assert_eq!(app.script_var("c"), "8");
}

#[test]
fn if_condition_uses_function_result() {
    let app = run("Set x 0\nIf Max(1, 9) = 9 Then\nSet x 1\nEndIf\n");
    assert_eq!(app.script_var("x"), "1");
}

#[test]
fn place_coords_from_functions() {
    // Place の X / Y 引数が関数。
    let app = run_with_units("Place \"ブレイバー\" \"リオ\" Enemy Min(4, 1) Max(0, 6)\n");
    let placed = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.party == src_core::Party::Enemy)
        .expect("enemy placed");
    assert_eq!((placed.x, placed.y), (1, 6));
}

#[test]
fn ampersand_concat_with_function_call_tokens() {
    // `&` 連結のオペランドに関数 — 各オペランドが独立トークンの形。
    let app = run(r#"Set p "u_" & Left("ブレイバー", 2)"#);
    assert_eq!(app.script_var("p"), "u_ブレ");
}

#[test]
fn ampersand_concat_with_function_call_parenthesized() {
    // `("a" & Func())` のように括弧で 1 トークンに括った中の `&` 連結も
    // 評価される (`expand_arg`)。スパロボ戦記 Include.eve の
    // `Ride ("$(A)" & "＋" & "$(B)")` 型に対応。
    let app = run(r#"Set p ("u_" & Left("ブレイバー", 2))"#);
    assert_eq!(app.script_var("p"), "u_ブレ");
}

// ============================================================
//  括弧付き `&` 連結の網羅ケース (expand_arg)
// ============================================================

#[test]
fn parenthesized_concat_three_string_literals() {
    // オペランド 3 個以上。
    let app = run(r#"Set p ("a" & "b" & "c")"#);
    assert_eq!(app.script_var("p"), "abc");
}

#[test]
fn parenthesized_concat_nested_group() {
    // オペランド自身が括弧付き連結。内側の `(...)` を再帰的に畳む。
    let app = run(r#"Set p (("a" & "b") & "c")"#);
    assert_eq!(app.script_var("p"), "abc");
}

#[test]
fn parenthesized_concat_deeply_nested() {
    // 多段ネスト。`expand_arg` の再帰で全段が畳まれる。
    let app = run(r#"Set p ((("1" & "2") & "3") & "4")"#);
    assert_eq!(app.script_var("p"), "1234");
}

#[test]
fn parenthesized_concat_with_quoted_dollar_vars() {
    // `"$(var)"` リテラルをオペランドにした連結 (スパロボ Ride 型)。
    let app = run("Set 元 リオ\nSet 相 ガロ\nSet p (\"$(元)\" & \"＋\" & \"$(相)\")\n");
    assert_eq!(app.script_var("p"), "リオ＋ガロ");
}

#[test]
fn parenthesized_concat_with_bare_dollar_vars() {
    // クオートで括らない `$(var)` オペランドも展開される。
    let app = run("Set x foo\nSet y bar\nSet p ($(x) & \"_\" & $(y))\n");
    assert_eq!(app.script_var("p"), "foo_bar");
}

#[test]
fn parenthesized_concat_mixed_operand_kinds() {
    // 文字列リテラル / 関数呼出 / `$(var)` を混在させた連結。
    let app = run("Set sfx bmp\nSet p (\"u_\" & Left(\"ブレイバー\", 2) & \".\" & $(sfx))\n");
    assert_eq!(app.script_var("p"), "u_ブレ.bmp");
}

#[test]
fn parenthesized_concat_ampersand_inside_quotes_is_literal() {
    // クオート内の `&` は連結演算子ではなくリテラル文字。
    let app = run(r#"Set p ("a & b" & "c")"#);
    assert_eq!(app.script_var("p"), "a & bc");
}

#[test]
fn parenthesized_concat_without_surrounding_spaces() {
    // `&` の前後に空白が無い形でも畳む。
    let app = run(r#"Set p ("a"&"b"&"c")"#);
    assert_eq!(app.script_var("p"), "abc");
}

#[test]
fn parenthesized_concat_function_arg_comma_not_split() {
    // 関数呼出オペラント内の `,` で連結が分割されないこと。
    let app = run(r#"Set p ("x" & Mid("abcdef", 2, 3))"#);
    assert_eq!(app.script_var("p"), "xbcd");
}

#[test]
fn parenthesized_concat_followed_by_token_ampersand() {
    // 括弧付き連結トークンの後ろに、独立トークンの `&` が続く形。
    // `expand_arg` が括弧トークンを畳み、`collapse_concat` が残りを畳む。
    let app = run(r#"Set p ("a" & "b") & "c""#);
    assert_eq!(app.script_var("p"), "abc");
}

#[test]
fn parenthesized_arithmetic_is_not_concat() {
    // `&` を含まない括弧式は連結扱いされず、トークンをそのまま通す。
    // `Set` は値が単一の括弧式なら算術評価する (SRC 準拠)。
    let app = run("Set v (200 - 128 / 2)\n");
    assert_eq!(app.script_var("v"), "136");
}

#[test]
fn parenthesized_concat_in_message_command() {
    // Set 以外の実コマンド引数でも括弧付き連結が評価される。
    let app = run("Set pre 候\nMessage (\"$(pre)\" & \"補\")\n");
    assert_eq!(app.messages(), &["候補".to_string()]);
}
