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

/// 値が単一の括弧式 `(...)` で、その中身が純粋な算術式 (全アトムが数値
/// リテラル、または数値に解決できるスクリプト変数) なら評価して数値文字列を
/// 返す。それ以外は `None`。
///
/// SRC の `Set` は値を式評価する。`expand_vars` は関数呼出 (`RoundUp(…)` 等)
/// は評価するが `(a + b)` のような括弧付き算術はトークンとして残すため、
/// `Set 敵配置数 (RoundUp(…) + (進行度 / 5))` が文字列のまま格納されていた。
///
/// 非数値アトムを 1 つでも含むなら算術式ではない (`Set msg (こんにちは)`)
/// とみなし評価しない。これにより文字列値を誤って 0 に潰さない。
pub(super) fn eval_paren_arith_value(app: &App, s: &str) -> Option<String> {
    let t = s.trim();
    if t.len() < 2 || !t.starts_with('(') || !t.ends_with(')') {
        return None;
    }
    // 全体が 1 つの括弧対で囲われていること (`(a)+(b)` や `(a)(b)` を弾く)。
    if find_matching_paren(t, 0)? != t.len() - 1 {
        return None;
    }
    eval_numeric_atoms(app, t)
}

/// `Set var value` / `var = value` の値を SRC `ExecSetCmd` (`EvalTerm`) 相当で評価する。
/// `EvalTerm` は値を式評価し、結果が数値型なら数値代入・そうでなければ文字列代入する。
/// 本移植では関数呼出 (`RoundUp(…)` 等) は `expand_vars` が、括弧付き算術は
/// [`eval_paren_arith_value`] が評価していたが、**括弧の無い算術**
/// (`資本金 = 資本金 - 営業収支` / `HP = HP - 10` / `カウンタ = カウンタ + 1` 等) が
/// 取りこぼされ、生の式文字列がそのまま変数に格納されていた (温泉旅館シナリオの
/// 経営計算が全て壊れる原因)。本関数は括弧の有無に依らず算術式を評価する。
///
/// 算術式と見なす条件: 算術演算子 / 括弧 (`+ - * / \ ^ ( )`) を 1 つ以上含むこと。
/// これが無い値 (単一識別子・空白区切りの複数値・純粋な文字列) は対象外 (`None`)。
/// さらに全アトムが数値 / 数値変数 / キーワードであることを要求し、文字列値
/// (`山田 太郎` 等) を誤って 0 に潰さない (SRC: EvalTerm が StringType を返す相当)。
pub(super) fn eval_arith_value(app: &App, s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if !t
        .chars()
        .any(|c| matches!(c, '+' | '-' | '*' | '/' | '\\' | '^' | '(' | ')'))
    {
        return None;
    }
    eval_numeric_atoms(app, t)
}

/// 式 `t` を評価し、結果が数値なら数値文字列 (`FormatNum`) を返す。文字列になる式
/// (`(こんにちは)` / `山田 太郎` 等) なら `None` を返し、呼び出し側は元の文字列を使う。
/// 未定義値が空に解決して二項演算子の右辺が欠けた式 (`(500 - )`) は欠けた側を 0 とする。
pub(super) fn eval_numeric_atoms(app: &App, t: &str) -> Option<String> {
    eval::try_numeric(app, t).map(format_num)
}

/// 式中の算術演算子 (`+ - * / \\ ^`) と括弧の 1 文字集合。算術式のアトム分割
/// (`parse_paintstring_args` の座標式判定・添字式判定など)、「式中の演算子を認識する」
/// 必要のある箇所はこの関数を参照する。比較 / 論理 / 連結 (`< > = &`) と語演算子
/// (And/Or/Not/Mod) は含めない。
pub(super) fn is_arith_operator_char(c: char) -> bool {
    matches!(c, '+' | '-' | '*' | '/' | '\\' | '^' | '(' | ')')
}
