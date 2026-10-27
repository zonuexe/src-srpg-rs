//! 式評価: 条件式 (`If` / `Do`) と算術・論理式の字句解析/再帰下降評価。

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

/// `s` を整数式として評価できれば値を返す。式パーサが何も食わなければ None。
pub(super) fn try_eval_int(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(v) = t.parse::<i64>() {
        return Some(v);
    }
    let tokens = tokenize_expr(t);
    if tokens.is_empty() {
        return None;
    }
    let mut idx = 0;
    let v = parse_logical(&tokens, &mut idx)?;
    if idx == tokens.len() {
        // SRC.Sharp `(int)value` は切り捨て (ゼロ方向)。`.round()` ではなく
        // `.trunc()` を使う: `3.9 → 3`, `-3.9 → -3`。
        Some(v.trunc() as i64)
    } else {
        None
    }
}

/// `try_eval_int` の float 版。`5 / 2` のように小数を含む式でも正確な
/// 値を返すため、indexed lookup 以外の汎用算術評価で使う。
///
/// SRC.Sharp `Expression.GetValueAsDouble` 相当: 整数演算でも float
/// として返す (caller が整数表現を希望する場合は `format_num` 経由で
/// `5` と `5.0` を区別なくフォーマットする)。
pub(super) fn try_eval_num(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(v) = t.parse::<f64>() {
        return Some(v);
    }
    let tokens = tokenize_expr(t);
    if tokens.is_empty() {
        return None;
    }
    let mut idx = 0;
    let v = parse_logical(&tokens, &mut idx)?;
    if idx == tokens.len() {
        Some(v)
    } else {
        None
    }
}

/// 簡易整数式評価。`+ - * /` と `(...)` をサポート。失敗時は 0。
/// 入力は `expand_vars` 後の文字列を想定（変数 / 関数呼出は既に展開済み）。
pub(super) fn eval_int_expr(s: &str) -> i32 {
    let trimmed = s.trim().trim_matches('"').trim();
    if let Ok(v) = trimmed.parse::<i32>() {
        return v;
    }
    let tokens = tokenize_expr(trimmed);
    let mut idx = 0;
    // `.trunc()` でゼロ方向へ切り捨て (SRC.Sharp `(int)value` と同様)。
    let v = parse_logical(&tokens, &mut idx).unwrap_or(0.0);
    v.trunc() as i32
}

/// `eval_int_expr` の app-aware 版。式中の **裸の識別子** (`j` / `i` 等の
/// ループ変数) を `script_var` として解決し、**引用符付き数値** (`"3"`) も
/// 受理してから評価する。
///
/// 例: `機体選択開始` の `HotPoint name (j * 40 - 35) ("$(i)" * 70 + 15) ...`
/// は、`$(i)` こそ `expand_vars` で展開済みだが `j` は裸のまま残り、
/// `("3" * 70 + 15)` のように引用符付き数値も混じる。素の `eval_int_expr`
/// はこれらを 0 に潰してしまい、全 Hotpoint が左上に重なって配置されていた。
pub(super) fn eval_int_expr_app(app: &App, s: &str) -> i32 {
    eval_int_expr(&resolve_expr_atoms(app, s))
}

/// 算術 / 比較 / 論理式中の演算子で区切られた各「アトム」を解決する。
/// - `And`, `Or`, `Not`, `Mod` キーワード → そのまま出力 (tokenizer に渡す)
/// - 数値リテラル (引用符付きも可) → そのまま
/// - 定義済みの数値スクリプト変数 → その値
/// - それ以外 → `0`
pub(super) fn resolve_expr_atoms(app: &App, s: &str) -> String {
    fn flush(app: &App, atom: &mut String, out: &mut String) {
        // `Line 50, 40, ...` の tokenizer 出力は `"50,"` のように trailing `,`
        // を含む。trim_matches(',') で先に剥がしてから数値判定する。
        let t = atom
            .trim()
            .trim_matches('"')
            .trim()
            .trim_matches(',')
            .trim();
        if !t.is_empty() {
            // 算術 / 論理キーワードはそのまま出力 (tokenize_expr が認識する)。
            if matches!(
                t.to_ascii_lowercase().as_str(),
                "and" | "or" | "not" | "mod"
            ) {
                out.push(' ');
                out.push_str(t);
                out.push(' ');
            } else if t.parse::<f64>().is_ok() {
                out.push_str(t);
            } else {
                // ① シナリオ変数 (script_var) を優先。
                let sv = app.script_var(t);
                if sv.trim().parse::<f64>().is_ok() {
                    out.push_str(sv.trim());
                } else if let Some(val) =
                    system_variable_value(app, t).filter(|v| v.trim().parse::<f64>().is_ok())
                {
                    // ② システム変数 (味方数 / 味方レベル平均値 / ターン数 等) を解決。
                    out.push_str(val.trim());
                } else {
                    out.push('0');
                }
            }
        }
        atom.clear();
    }
    let mut out = String::new();
    let mut atom = String::new();
    for ch in s.chars() {
        // アトム区切り = 算術演算子 (`is_arith_operator_char`) + 比較 `> < =` +
        // 文字列連結 `&`。算術演算子集合を一元化しているため `\`(整数除算) /
        // `^`(累乗) の取りこぼしが起きない (旧実装では `3 \ 8` の `\` が未定義
        // 変数として `0` に潰れ `308` になっていた)。
        if is_arith_operator_char(ch) || matches!(ch, '>' | '<' | '=' | '&') {
            flush(app, &mut atom, &mut out);
            out.push(ch);
        } else if ch == ' ' || ch == '\t' {
            // スペースはアトム区切りだが出力には入れない (tokenizer が無視する)。
            flush(app, &mut atom, &mut out);
        } else {
            atom.push(ch);
        }
    }
    flush(app, &mut atom, &mut out);
    out
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

/// 算術式 `t` (括弧の有無は問わない) の全アトム (算術演算子 / 比較 `> < =` / 連結 `&` /
/// 空白で区切る) が数値リテラル / 数値スクリプト変数 / 算術キーワード (`And`/`Or`/`Not`/
/// `Mod`) のいずれかであれば、解決・評価して数値文字列を返す。非数値アトムを 1 つでも
/// 含むなら算術式でない (`(こんにちは)` / `山田 太郎` 等) とみなし `None` を返す。
/// これにより文字列値を誤って 0 に潰さない。
pub(super) fn eval_numeric_atoms(app: &App, t: &str) -> Option<String> {
    let atom_is_numeric_or_kw = |atom: &str| -> bool {
        let a = atom.trim().trim_matches('"').trim();
        a.is_empty()
            || a.parse::<f64>().is_ok()
            || app.script_var(a).trim().parse::<f64>().is_ok()
            || matches!(
                a.to_ascii_lowercase().as_str(),
                "and" | "or" | "not" | "mod"
            )
    };
    let mut atom = String::new();
    for ch in t.chars() {
        // アトム区切り = 算術演算子 (`is_arith_operator_char`, 括弧含む) + 比較 `> < =` +
        // 連結 `&` + 空白。算術演算子集合を一元化 (`\` `^` の取りこぼし防止)。
        // これが無いと `(... \ 8 + 1)` の `\` がアトム `\8` に紛れて数値判定に
        // 失敗し、式が評価されず生文字列のまま残る (武器/強化パーツ/特殊能力の
        // ページ数計算 `(N - 1) \ 8 + 1` 等が壊れていた)。
        if is_arith_operator_char(ch) || matches!(ch, '>' | '<' | '=' | '&') || ch.is_whitespace() {
            if !atom_is_numeric_or_kw(&atom) {
                return None;
            }
            atom.clear();
        } else {
            atom.push(ch);
        }
    }
    if !atom_is_numeric_or_kw(&atom) {
        return None;
    }
    // 未定義値が空に解決して二項演算子の直後が `)`/末尾になった場合に `0` を補う
    // (SRC: 未定義数値は 0)。これで `(500 - )` / `(500 * 0 - 500 + )` のような式
    // (Info(...) 等が空に解決) も評価でき、生式の表示漏れを防ぐ。
    let resolved = fill_dangling_operands(&resolve_expr_atoms(app, t));
    let v = try_eval_num(&resolved)?;
    Some(format_num(v))
}

/// `resolve_expr_atoms` の出力 (空白なし) で、二項 `+`/`-`/`*` の直後が `)` または
/// 末尾になっている (= オペランド欠落) 箇所に `0` を補う。除算/累乗 (`/`/`\`/`^`)
/// は 0 補填が危険 (ゼロ除算) なので対象外 (該当式は従来どおり未評価のまま)。
pub(super) fn fill_dangling_operands(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 2);
    for (i, &c) in chars.iter().enumerate() {
        out.push(c);
        if matches!(c, '+' | '-' | '*') {
            let next = chars.get(i + 1);
            if next.is_none() || next == Some(&')') {
                out.push('0');
            }
        }
    }
    out
}

/// `tokenize_expr` が **記号演算子トークン** を生成する 1 文字の集合 (Single
/// Source of Truth)。算術式のアトム分割 (`resolve_expr_atoms` /
/// `eval_paren_arith_value`)・座標式判定 (`parse_paintstring_args`)・添字式
/// 判定など、「式中の演算子を認識する」必要のある全箇所はこの関数を参照する。
///
/// 以前は各所が独自に `'+' | '-' | '*' | '/' | ...` をハードコードしていたため、
/// `tokenize_expr` に VB5 整数除算 `\` と累乗 `^` を追加した際に分割側の更新が
/// 漏れ、`(N - 1) \ 8 + 1` 等が評価されない不具合を生んだ。これを一元化し、
/// `arith_operator_char_set_matches_tokenizer` テストで `tokenize_expr` との
/// 整合を機械的に保証する (新演算子の追加漏れをテストが検出する)。
///
/// 比較 / 論理 / 連結 (`< > = &`) は算術ではないためここには含めず、条件式を
/// 扱う呼び出し側が個別に加える。語演算子 (And/Or/Not/Mod) も記号ではないので
/// 各呼び出し側がキーワードとして扱う。
pub(super) fn is_arith_operator_char(c: char) -> bool {
    matches!(c, '+' | '-' | '*' | '/' | '\\' | '^' | '(' | ')')
}

#[derive(Debug, Clone)]
pub(super) enum ExprTok {
    Num(f64),
    Plus,
    Minus,
    Star,
    Slash,
    IntDiv, // `\` VB5 integer division
    Mod,    // `Mod` keyword — modulo
    Caret,  // `^` exponentiation
    LParen,
    RParen,
    // 比較演算子
    Gt, // `>`
    Lt, // `<`
    Ge, // `>=`
    Le, // `<=`
    Eq, // `=`
    Ne, // `<>`
    // 論理演算子
    And, // `And` keyword
    Or,  // `Or` keyword
    Not, // `Not` keyword (prefix)
}

pub(super) fn tokenize_expr(s: &str) -> Vec<ExprTok> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' => i += 1,
            b'+' => {
                out.push(ExprTok::Plus);
                i += 1;
            }
            b'-' => {
                out.push(ExprTok::Minus);
                i += 1;
            }
            b'*' => {
                out.push(ExprTok::Star);
                i += 1;
            }
            b'/' => {
                out.push(ExprTok::Slash);
                i += 1;
            }
            b'\\' => {
                // VB5 integer division
                out.push(ExprTok::IntDiv);
                i += 1;
            }
            b'^' => {
                out.push(ExprTok::Caret);
                i += 1;
            }
            b'(' => {
                out.push(ExprTok::LParen);
                i += 1;
            }
            b')' => {
                out.push(ExprTok::RParen);
                i += 1;
            }
            b'>' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    out.push(ExprTok::Ge);
                    i += 2;
                } else {
                    out.push(ExprTok::Gt);
                    i += 1;
                }
            }
            b'<' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    out.push(ExprTok::Le);
                    i += 2;
                } else if i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                    out.push(ExprTok::Ne);
                    i += 2;
                } else {
                    out.push(ExprTok::Lt);
                    i += 1;
                }
            }
            b'=' => {
                out.push(ExprTok::Eq);
                i += 1;
            }
            b'0'..=b'9' | b'.' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                if let Ok(v) = s[start..i].parse::<f64>() {
                    out.push(ExprTok::Num(v));
                }
            }
            b'M' | b'm' => {
                // `Mod` keyword (case-insensitive) — byte-level comparison to avoid UTF-8 issues
                if i + 3 <= bytes.len()
                    && (bytes[i] == b'M' || bytes[i] == b'm')
                    && (bytes[i + 1] == b'O' || bytes[i + 1] == b'o')
                    && (bytes[i + 2] == b'D' || bytes[i + 2] == b'd')
                    && (i + 3 >= bytes.len() || !bytes[i + 3].is_ascii_alphanumeric())
                {
                    out.push(ExprTok::Mod);
                    i += 3;
                } else {
                    break; // unknown keyword — bail
                }
            }
            b'A' | b'a' => {
                // `And` keyword (case-insensitive)
                if i + 3 <= bytes.len()
                    && bytes[i].eq_ignore_ascii_case(&b'a')
                    && bytes[i + 1].eq_ignore_ascii_case(&b'n')
                    && bytes[i + 2].eq_ignore_ascii_case(&b'd')
                    && (i + 3 >= bytes.len() || !bytes[i + 3].is_ascii_alphanumeric())
                {
                    out.push(ExprTok::And);
                    i += 3;
                } else {
                    break;
                }
            }
            b'O' | b'o' => {
                // `Or` keyword (case-insensitive)
                if i + 2 <= bytes.len()
                    && bytes[i].eq_ignore_ascii_case(&b'o')
                    && bytes[i + 1].eq_ignore_ascii_case(&b'r')
                    && (i + 2 >= bytes.len() || !bytes[i + 2].is_ascii_alphanumeric())
                {
                    out.push(ExprTok::Or);
                    i += 2;
                } else {
                    break;
                }
            }
            b'N' | b'n' => {
                // `Not` keyword (case-insensitive)
                if i + 3 <= bytes.len()
                    && bytes[i].eq_ignore_ascii_case(&b'n')
                    && bytes[i + 1].eq_ignore_ascii_case(&b'o')
                    && bytes[i + 2].eq_ignore_ascii_case(&b't')
                    && (i + 3 >= bytes.len() || !bytes[i + 3].is_ascii_alphanumeric())
                {
                    out.push(ExprTok::Not);
                    i += 3;
                } else {
                    break;
                }
            }
            _ => {
                // unknown char — bail
                break;
            }
        }
    }
    out
}

pub(super) fn parse_expr(tokens: &[ExprTok], idx: &mut usize) -> Option<f64> {
    let mut left = parse_term(tokens, idx)?;
    while *idx < tokens.len() {
        match tokens[*idx] {
            ExprTok::Plus => {
                *idx += 1;
                let r = parse_term(tokens, idx)?;
                left += r;
            }
            ExprTok::Minus => {
                *idx += 1;
                let r = parse_term(tokens, idx)?;
                left -= r;
            }
            _ => break,
        }
    }
    Some(left)
}

pub(super) fn parse_term(tokens: &[ExprTok], idx: &mut usize) -> Option<f64> {
    let mut left = parse_power(tokens, idx)?;
    while *idx < tokens.len() {
        match tokens[*idx] {
            ExprTok::Star => {
                *idx += 1;
                let r = parse_power(tokens, idx)?;
                left *= r;
            }
            ExprTok::Slash => {
                *idx += 1;
                let r = parse_power(tokens, idx)?;
                // SRC.Sharp `Expression.EvalExpr` は除数 0 のとき結果を 0 に潰す
                // (`rnum != 0d` ガード)。左辺を残すと `x / 0 == x` となり原典と乖離する。
                left = if r != 0.0 { left / r } else { 0.0 };
            }
            ExprTok::IntDiv => {
                // VB5 integer division: truncate toward zero。除数 0 は SRC 同様 0。
                *idx += 1;
                let r = parse_power(tokens, idx)?;
                left = if r as i64 != 0 {
                    (left as i64 / r as i64) as f64
                } else {
                    0.0
                };
            }
            ExprTok::Mod => {
                // VB5 Mod: integer modulo。除数 0 は SRC 同様 0。
                *idx += 1;
                let r = parse_power(tokens, idx)?;
                left = if r as i64 != 0 {
                    (left as i64 % r as i64) as f64
                } else {
                    0.0
                };
            }
            _ => break,
        }
    }
    Some(left)
}

/// 累乗: 右結合 `a ^ b ^ c` = `a ^ (b ^ c)`
pub(super) fn parse_power(tokens: &[ExprTok], idx: &mut usize) -> Option<f64> {
    let base = parse_factor(tokens, idx)?;
    if matches!(tokens.get(*idx), Some(ExprTok::Caret)) {
        *idx += 1;
        // 右結合のため再帰呼び出し
        let exp = parse_power(tokens, idx)?;
        Some(base.powf(exp))
    } else {
        Some(base)
    }
}

pub(super) fn parse_factor(tokens: &[ExprTok], idx: &mut usize) -> Option<f64> {
    let tok = tokens.get(*idx)?;
    match tok {
        ExprTok::Num(v) => {
            *idx += 1;
            Some(*v)
        }
        ExprTok::Minus => {
            *idx += 1;
            Some(-parse_factor(tokens, idx)?)
        }
        ExprTok::Plus => {
            *idx += 1;
            parse_factor(tokens, idx)
        }
        ExprTok::LParen => {
            *idx += 1;
            let v = parse_logical(tokens, idx)?;
            if matches!(tokens.get(*idx), Some(ExprTok::RParen)) {
                *idx += 1;
            }
            Some(v)
        }
        _ => None,
    }
}

/// 比較演算子レベル: `=`, `<>`, `<`, `>`, `<=`, `>=`
/// 戻り値は 0.0 (false) または 1.0 (true)。算術式の結果が比較されない場合は
/// そのまま算術値を返す (precedence は `+/-` より低い)。
pub(super) fn parse_comparison(tokens: &[ExprTok], idx: &mut usize) -> Option<f64> {
    let mut left = parse_expr(tokens, idx)?;
    while *idx < tokens.len() {
        match tokens[*idx] {
            ExprTok::Gt => {
                *idx += 1;
                let r = parse_expr(tokens, idx)?;
                left = if left > r { 1.0 } else { 0.0 };
            }
            ExprTok::Lt => {
                *idx += 1;
                let r = parse_expr(tokens, idx)?;
                left = if left < r { 1.0 } else { 0.0 };
            }
            ExprTok::Ge => {
                *idx += 1;
                let r = parse_expr(tokens, idx)?;
                left = if left >= r { 1.0 } else { 0.0 };
            }
            ExprTok::Le => {
                *idx += 1;
                let r = parse_expr(tokens, idx)?;
                left = if left <= r { 1.0 } else { 0.0 };
            }
            ExprTok::Eq => {
                *idx += 1;
                let r = parse_expr(tokens, idx)?;
                left = if (left - r).abs() < 1e-9 { 1.0 } else { 0.0 };
            }
            ExprTok::Ne => {
                *idx += 1;
                let r = parse_expr(tokens, idx)?;
                left = if (left - r).abs() >= 1e-9 { 1.0 } else { 0.0 };
            }
            _ => break,
        }
    }
    Some(left)
}

/// `Not` 演算子レベル: 比較より緩く、`And`/`Or` より固く束縛する (VB5 / SRC.Sharp 準拠)。
/// `Not 1 = 2` → `Not (1 = 2)` → `Not 0` → 1。`Not Not x` も許容 (右再帰)。
/// `Not` が無ければ比較レベルへ素通しする (非 Not 式の挙動は不変)。
pub(super) fn parse_not(tokens: &[ExprTok], idx: &mut usize) -> Option<f64> {
    if matches!(tokens.get(*idx), Some(ExprTok::Not)) {
        *idx += 1;
        let v = parse_not(tokens, idx)?;
        Some(if v == 0.0 { 1.0 } else { 0.0 })
    } else {
        parse_comparison(tokens, idx)
    }
}

/// 論理演算子レベル: `And` / `Or` (最低優先度)。
/// 0.0 を false、それ以外を true として短絡評価する。
/// オペランドは `parse_not` (比較 < Not < And/Or の優先順) を通す。
pub(super) fn parse_logical(tokens: &[ExprTok], idx: &mut usize) -> Option<f64> {
    let mut left = parse_not(tokens, idx)?;
    while *idx < tokens.len() {
        match tokens[*idx] {
            ExprTok::And => {
                *idx += 1;
                let r = parse_not(tokens, idx)?;
                left = if left != 0.0 && r != 0.0 { 1.0 } else { 0.0 };
            }
            ExprTok::Or => {
                *idx += 1;
                let r = parse_not(tokens, idx)?;
                left = if left != 0.0 || r != 0.0 { 1.0 } else { 0.0 };
            }
            _ => break,
        }
    }
    Some(left)
}
