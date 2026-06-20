//! 式評価器のオラクル準拠テスト。
//!
//! 原典 SRC.Sharp `SRCCoreTests/Expressions/`（ExpressionArithmetic* /
//! ExpressionOperator* / OperatorType* / GetValueAsLong*）が pin する
//! 算術・演算子・型強制の挙動を、本実装の実行時評価器 (`event_runtime` の
//! `parse_logical` チェーン) に対して end-to-end で突き合わせる。
//!
//! 評価サーフェス: `Set z (式)`。括弧付き算術は SRC `ExecSetCmd` 同様に
//! 式評価される。比較・論理演算子も同じ評価チェーンを通る。
//!
//! 著作権配慮: SRC オリジナルコードは含まない。input→expected のペアのみ移植。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn val(expr: &str) -> String {
    let src = format!("Set z ({expr})\n");
    let mut app = App::new();
    let stmts = event::parse(&src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app.script_var("z").to_string()
}

// ============================================================
//  (1) 算術と優先順位
// ============================================================

#[test]
fn arithmetic_basic() {
    assert_eq!(val("3 + 4"), "7");
    assert_eq!(val("5 - 4"), "1"); // 旧スタブは加算に化けた領域。実評価器は正しく減算。
    assert_eq!(val("3 * 4"), "12");
    assert_eq!(val("7 - 7"), "0");
    assert_eq!(val("2 - 5"), "-3");
}

#[test]
fn arithmetic_precedence() {
    assert_eq!(val("2 + 3 * 4"), "14"); // * が + より優先
    assert_eq!(val("10 - 2 * 3 + 1"), "5");
    assert_eq!(val("(2 + 3) * 4"), "20");
    assert_eq!(val("((2 + 3) * (4 - 1))"), "15");
    assert_eq!(val("10 - 5 - 4"), "1"); // 左結合
    assert_eq!(val("1 + 2 + 3 + 4"), "10");
}

#[test]
fn power_operator() {
    assert_eq!(val("2 ^ 3"), "8");
    assert_eq!(val("5 ^ 0"), "1");
    assert_eq!(val("7 ^ 1"), "7");
    assert_eq!(val("4 ^ 0.5"), "2"); // 分数指数 → 平方根 (Math.Pow)
}

// ============================================================
//  (2) 除算演算子 `/`(浮動) と `\`(整数) は別物 — オラクル最重要 PITFALL
// ============================================================

#[test]
fn float_vs_integer_division() {
    assert_eq!(val("5 / 2"), "2.5"); // `/` は浮動小数除算 (端数保持)
    assert_eq!(val("5 \\ 2"), "2"); // `\` は整数除算 (切り捨て)
    assert_eq!(val("12 / 3"), "4");
    assert_eq!(val("1000 \\ 3"), "333");
    assert_eq!(val("7 \\ 2"), "3");
}

// ============================================================
//  (3) 単項マイナス・負数
// ============================================================

#[test]
fn unary_minus_and_negatives() {
    assert_eq!(val("-5"), "-5");
    assert_eq!(val("-5 + 2"), "-3");
    assert_eq!(val("-2 * 3"), "-6");
    assert_eq!(val("-3 + -4"), "-7"); // 二項+の右に単項-
    assert_eq!(val("-2 * -3"), "6"); // 二項*の右に単項-
    assert_eq!(val("-7 \\ 2"), "-3"); // 整数除算はゼロ方向切り捨て (-4 ではない)
}

// ============================================================
//  (4) Mod — 剰余の符号は被除数に従う (VB6/C# `%`)
// ============================================================

#[test]
fn modulo_semantics() {
    assert_eq!(val("7 Mod 3"), "1");
    assert_eq!(val("6 Mod 3"), "0");
    assert_eq!(val("3 Mod 10"), "3");
    assert_eq!(val("10 Mod 10"), "0");
    assert_eq!(val("-7 Mod 3"), "-1"); // 符号は被除数 → -1 (2 ではない)
}

// ============================================================
//  (5) 比較演算子は値 1/0 を返す。等価は単一 `=`、非等価は `<>`
// ============================================================

#[test]
fn comparison_yields_one_or_zero() {
    assert_eq!(val("3 = 3"), "1");
    assert_eq!(val("3 = 4"), "0");
    assert_eq!(val("3 <> 4"), "1");
    assert_eq!(val("3 < 4"), "1");
    assert_eq!(val("5 < 4"), "0");
    assert_eq!(val("5 > 4"), "1");
    assert_eq!(val("4 <= 4"), "1");
    assert_eq!(val("5 >= 4"), "1");
    assert_eq!(val("3 >= 4"), "0");
}

#[test]
fn arithmetic_binds_tighter_than_comparison() {
    assert_eq!(val("2 + 3 > 4"), "1"); // (2+3) > 4
    assert_eq!(val("1 < 2 And 2 < 3 And 3 < 4"), "1"); // < が And より優先
}

// ============================================================
//  (6) 論理演算子 And / Or / Not (真偽は非ゼロ/ゼロ)
// ============================================================

#[test]
fn logical_operators() {
    assert_eq!(val("1 And 1"), "1");
    assert_eq!(val("1 And 0"), "0");
    assert_eq!(val("0 And 0"), "0");
    assert_eq!(val("0 Or 1"), "1");
    assert_eq!(val("1 Or 1"), "1");
    assert_eq!(val("0 Or 0"), "0");
    assert_eq!(val("(1 = 1) And (2 = 2)"), "1");
    assert_eq!(val("(1 = 1) Or (2 = 3)"), "1");
}

#[test]
fn not_is_logical_truthiness_not_bitwise() {
    // `Not` は論理否定 (非ゼロ→0、ゼロ→1)。ビット反転ではない (`Not 1` は -2 ではない)。
    assert_eq!(val("Not 0"), "1");
    assert_eq!(val("Not 1"), "0");
    assert_eq!(val("Not 5"), "0");
    assert_eq!(val("Not (1 = 1)"), "0");
    assert_eq!(val("Not (1 = 2)"), "1");
}

// ============================================================
//  (7) ゼロ除算 — SRC は 0 を返す (差異①の回帰防止)
//  本実装は以前「左辺(分子)をそのまま残す」バグ (`5 / 0 == 5`) があった。
// ============================================================

#[test]
fn division_by_zero_yields_zero() {
    assert_eq!(val("1 / 0"), "0");
    assert_eq!(val("5 / 0"), "0");
    assert_eq!(val("5 \\ 0"), "0");
    assert_eq!(val("5 Mod 0"), "0");
    // 実害例: 平均計算で母数が 0 のとき 0 になる (合計値が漏れない)。
    assert_eq!(val("100 / 0"), "0");
}

// ============================================================
//  (8) `Not` の優先順位 — オラクルと整合 (2026-06-20 是正)
//  `Not` は比較より緩く、And/Or より固く束縛する (VB6 / SRC.Sharp 準拠)。
//  `Not 1 = 2` → `Not (1 = 2)` → `Not 0` → 1。`parse_not` レベルを比較と論理の
//  間に挿入し parse_factor から Not を外して整合させた (旧: 本実装は 0)。
// ============================================================

#[test]
fn not_binds_looser_than_comparison() {
    // オラクル一致: `Not 1 = 2` = `Not (1 = 2)` = `Not 0` = 1。
    assert_eq!(val("Not 1 = 2"), "1");
    // 括弧付きも従来どおり一致。
    assert_eq!(val("Not (1 = 2)"), "1");
    assert_eq!(val("Not (1 = 1)"), "0");
    // `Not` は And/Or より固い: `Not 0 And 1` = `(Not 0) And 1` = 1。
    assert_eq!(val("Not 0 And 1"), "1");
    // 単項 `Not` の真偽値 (非ゼロ→0、ゼロ→1) は不変。
    assert_eq!(val("Not 0"), "1");
    assert_eq!(val("Not 5"), "0");
}
