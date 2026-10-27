//! 式評価の補助: `If` の項分割に使う比較演算子判定と、算術演算子の文字判定。

pub(super) fn is_comparison_op(s: &str) -> bool {
    matches!(s, "=" | "==" | "<>" | "!=" | "<" | "<=" | ">" | ">=")
        || s.eq_ignore_ascii_case("like")
}

/// 式中の算術演算子 (`+ - * / \\ ^`) と括弧の 1 文字集合。算術式のアトム分割
/// (`parse_paintstring_args` の座標式判定・添字式判定など)、「式中の演算子を認識する」
/// 必要のある箇所はこの関数を参照する。比較 / 論理 / 連結 (`< > = &`) と語演算子
/// (And/Or/Not/Mod) は含めない。
pub(super) fn is_arith_operator_char(c: char) -> bool {
    matches!(c, '+' | '-' | '*' | '/' | '\\' | '^' | '(' | ')')
}
