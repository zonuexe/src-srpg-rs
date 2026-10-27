//! 式評価の補助: `Switch` / `Case` の比較と、統一評価器 (`eval`) を使う数値ヘルパ。

use super::*;

pub(super) fn is_comparison_op(s: &str) -> bool {
    matches!(s, "=" | "==" | "<>" | "!=" | "<" | "<=" | ">" | ">=")
        || s.eq_ignore_ascii_case("like")
}

pub(super) fn eval_binop(lhs: &str, op: &str, rhs: &str) -> bool {
    // SRC の比較規則: 両辺とも数値なら数値比較、両辺とも非数値なら文字列
    // 比較。**一方だけが数値**なら数値比較とし、非数値側 (未設定変数 →
    // その名前、空文字、`?` 等) は 0 とみなす。これにより
    // `If 未設定カウンタ <> 0` が `0 <> 0` = 偽になる (SRC 準拠)。
    let (mut lhs_n, mut rhs_n) = (lhs.parse::<f64>().ok(), rhs.parse::<f64>().ok());
    if lhs_n.is_none() && rhs_n.is_some() {
        lhs_n = Some(0.0);
    } else if rhs_n.is_none() && lhs_n.is_some() {
        rhs_n = Some(0.0);
    }
    match op {
        "=" | "==" => match (lhs_n, rhs_n) {
            (Some(a), Some(b)) => (a - b).abs() < f64::EPSILON,
            _ => lhs == rhs,
        },
        "<>" | "!=" => match (lhs_n, rhs_n) {
            (Some(a), Some(b)) => (a - b).abs() >= f64::EPSILON,
            _ => lhs != rhs,
        },
        "<" => match (lhs_n, rhs_n) {
            (Some(a), Some(b)) => a < b,
            _ => lhs < rhs,
        },
        "<=" => match (lhs_n, rhs_n) {
            (Some(a), Some(b)) => a <= b,
            _ => lhs <= rhs,
        },
        ">" => match (lhs_n, rhs_n) {
            (Some(a), Some(b)) => a > b,
            _ => lhs > rhs,
        },
        ">=" => match (lhs_n, rhs_n) {
            (Some(a), Some(b)) => a >= b,
            _ => lhs >= rhs,
        },
        op if op.eq_ignore_ascii_case("like") => like_match(lhs, rhs),
        _ => false,
    }
}

/// 式を整数として評価する (原典 `GetValueAsLong`、統一評価器)。i32 範囲へ飽和させる。
/// 座標・サイズ等の数値引数 (`(j * 40 - 35)` / `("3" * 70 + 15)` / ループ変数 `i`) に使う。
pub(super) fn eval_int_expr_app(app: &App, s: &str) -> i32 {
    eval::value_as_long(app, s.trim()).clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

/// 式中の算術演算子 (`+ - * / \\ ^`) と括弧の 1 文字集合。算術式のアトム分割
/// (`parse_paintstring_args` の座標式判定・添字式判定など)、「式中の演算子を認識する」
/// 必要のある箇所はこの関数を参照する。比較 / 論理 / 連結 (`< > = &`) と語演算子
/// (And/Or/Not/Mod) は含めない。
pub(super) fn is_arith_operator_char(c: char) -> bool {
    matches!(c, '+' | '-' | '*' | '/' | '\\' | '^' | '(' | ')')
}
