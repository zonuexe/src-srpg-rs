//! SRC の式評価器 (`Expression.bas` の `EvalExpr` / `EvalTerm` の移植)。
//!
//! 式は空白区切りの「項」に分解し (`list_split`、括弧・クォート内は区切らない)、
//! 最も優先度の低い演算子で左右に分けて再帰的に評価する。演算子の選び方・型の
//! 規則は原典どおり:
//!
//! - 優先度 (低い順): `Or` < `And` < `Not` < 比較 (`=` `<>` `!=` `<` `<=` `>` `>=` `Like`)
//!   < `&` < `+` `-` < `Mod` < `\` < `*` `/` < `^`。同じ優先度なら右端の演算子で
//!   分割する (= 左結合)。`Or` / `And` / `Not` だけは左端で分割する。
//! - 演算子は**空白で区切られた項**のときだけ認識する (`n+1` は 1 つの項 = 文字列)。
//! - `=` / `<>` は、左右どちらかの**生のテキスト**が数値リテラルなら数値比較、
//!   それ以外は文字列比較。`<` `>` などは常に数値比較。
//! - 項の解決順: 数値リテラル → クォート文字列 (`"..."` 内は `$(...)` を展開) →
//!   括弧式 → 関数呼び出し → 変数 (システム変数 / 配列 / スクリプト変数)。
//!   未定義変数は名前そのものの文字列 (数値文脈では 0)。
//!
//! 値は「期待する型」(`Ty::Str` / `Ty::Num`) を受け取り、その型で結果を返す
//! (`GetValueAsString` / `GetValueAsDouble` / `GetValueAsLong` に相当)。
//!
//! 原典との意図的な差: クォートの外の `$(...)` も展開する (`eval_term`)。
//! C# 移植 (SRC.Sharp) とは `\` / `Mod` の非整数オペランドの扱いが異なる
//! (VB6 は整数へ丸めてから演算する。本実装は VB6 準拠)。
//!
//! ユーザ定義関数は `&mut App` を要するため、呼び出し側が事前に
//! `preprocess_user_functions` で値へ畳み込んでおく。

use super::*;
use crate::util::number::round_half_to_even;

/// 値の型 (原典 `ValueType`)。期待型としての `Undefined` は「どちらでもよい」
/// (`Set` の値など。演算結果の型がそのまま結果の型になる)。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum Ty {
    #[default]
    Undefined,
    Str,
    Num,
}

/// 評価結果。`ty` が `Str` なら `s`、`Num` なら `n` が値。期待型が `Num` のときは
/// 文字列の結果でも `n` に数値換算が入る (原典の `str_result` / `num_result` の組)。
#[derive(Debug, Clone, Default)]
pub(super) struct Val {
    pub ty: Ty,
    pub s: String,
    pub n: f64,
}

impl Val {
    fn str(s: impl Into<String>) -> Self {
        Val {
            ty: Ty::Str,
            s: s.into(),
            n: 0.0,
        }
    }
    fn num(n: f64) -> Self {
        Val {
            ty: Ty::Num,
            s: String::new(),
            n,
        }
    }
    /// 比較・論理演算の結果 (真=1 / 偽=0) を期待型で返す。
    fn bool_as(b: bool, ety: Ty) -> Self {
        Self::num_as(if b { 1.0 } else { 0.0 }, ety)
    }
    /// 数値結果を期待型で返す (文字列期待なら `FormatNum`)。
    fn num_as(n: f64, ety: Ty) -> Self {
        if ety == Ty::Str {
            Val::str(format_num(n))
        } else {
            Val::num(n)
        }
    }
}

/// 式を文字列として評価する (原典 `GetValueAsString`)。
pub(super) fn value_as_string(app: &App, expr: &str) -> String {
    eval_expr(app, expr, Ty::Str).s
}

/// 式を浮動小数点数として評価する (原典 `GetValueAsDouble`)。
pub(super) fn value_as_double(app: &App, expr: &str) -> f64 {
    eval_expr(app, expr, Ty::Num).n
}

/// 式を整数として評価する (原典 `GetValueAsLong`)。原典は Double を Long へ代入するので
/// VB の型変換どおり偶数丸めになる (`3.5` → 4、`2.5` → 2)。C# 移植は切り捨てで、原典と異なる。
pub(super) fn value_as_long(app: &App, expr: &str) -> i64 {
    vb_clng(value_as_double(app, expr))
}

/// 1 項を整数として評価する (原典 `GetValueAsLong(expr, is_term: true)`)。
pub(super) fn term_as_long(app: &App, term: &str) -> i64 {
    vb_clng(eval_term(app, term, Ty::Num).n)
}

/// `Set var value` / `var = 式` の代入値 (原典 `ExecSetCmd`)。1 項なら項として、
/// 複数項なら式として型を決めずに評価し、数値なら `FormatNum`、文字列ならそのまま返す。
///
/// 本移植の拡張: 演算子を含まない複数項 (`Set 名前 山田 太郎` / `Set who Args(1) Args(2)`) は
/// 各項の値を空白で連結する。原典では `Set` の引数過多エラーになる書き方。
pub(super) fn assignment_value(app: &App, terms: &[String]) -> String {
    let v = match terms {
        [] => return String::new(),
        [t] => eval_term(app, t, Ty::Undefined),
        _ if terms.iter().all(|t| operator_of(t).is_none()) => {
            return terms
                .iter()
                .map(|t| eval_term(app, t, Ty::Str).s)
                .collect::<Vec<_>>()
                .join(" ");
        }
        _ => eval_expr(app, &terms.join(" "), Ty::Undefined),
    };
    if v.ty == Ty::Num {
        format_num(v.n)
    } else {
        v.s
    }
}

/// コマンド引数の型 (原典 `CmdData` の構文解析時の `ArgsType`)。先頭の 1 文字で決まる。
enum ArgKind {
    /// 文字列リテラル (クォートを外した値。`$(...)` を含まないクォート文字列・
    /// バッククォート文字列・数値として読めない `-` / 数字始まりの語)。
    Literal(String),
    /// 数値リテラル。
    Number(f64),
    /// 項として評価する (変数・関数・括弧式・`$(...)` を含むクォート文字列)。
    Term,
}

fn arg_kind(raw: &str) -> ArgKind {
    let Some(first) = raw.chars().next() else {
        return ArgKind::Literal(String::new());
    };
    match first {
        '"' if raw.len() >= 2 && raw.ends_with('"') => {
            if raw.contains("$(") {
                ArgKind::Term
            } else {
                ArgKind::Literal(raw[1..raw.len() - 1].to_string())
            }
        }
        // 閉じクォートが無い `"...` はそのまま文字列。
        '"' => ArgKind::Literal(raw.to_string()),
        '`' if raw.len() >= 2 && raw.ends_with('`') => {
            ArgKind::Literal(raw[1..raw.len() - 1].to_string())
        }
        '`' => ArgKind::Literal(raw.to_string()),
        '-' | '0'..='9' => {
            if is_numeric(raw) {
                ArgKind::Number(str_to_dbl(raw))
            } else {
                ArgKind::Literal(raw.to_string())
            }
        }
        _ => ArgKind::Term,
    }
}

/// コマンド引数 (原文トークン) の文字列値 (原典 `CmdData.GetArgAsString`)。
pub(super) fn arg_as_string(app: &App, raw: &str) -> String {
    match arg_kind(raw) {
        ArgKind::Literal(s) => s,
        ArgKind::Number(n) => format_num(n),
        ArgKind::Term => eval_term(app, raw, Ty::Str).s,
    }
}

/// `Case` の値 1 つ (原典 `ExecSwitchCmd`)。構文解析で型の決まった引数 (文字列リテラル・
/// 数値リテラル) は評価せず原文 (`GetArg`) を使うため、`Case 05` は `"05"` としか一致しない。
/// それ以外は `GetArgAsString` で項として評価する。
pub(super) fn case_value(app: &App, raw: &str) -> String {
    match arg_kind(raw) {
        ArgKind::Literal(s) => s,
        ArgKind::Number(_) => raw.to_string(),
        ArgKind::Term => eval_term(app, raw, Ty::Str).s,
    }
}

/// コマンド引数 (原文トークン) の整数値 (原典 `CmdData.GetArgAsLong`)。文字列リテラルは 0。
pub(super) fn arg_as_long(app: &App, raw: &str) -> i64 {
    match arg_kind(raw) {
        ArgKind::Literal(_) => 0,
        ArgKind::Number(n) => vb_clng(n),
        ArgKind::Term => term_as_long(app, raw),
    }
}

/// コマンド引数 (原文トークン) の実数値 (原典 `CmdData.GetArgAsDouble`)。文字列リテラルは 0。
pub(super) fn arg_as_double(app: &App, raw: &str) -> f64 {
    match arg_kind(raw) {
        ArgKind::Literal(_) => 0.0,
        ArgKind::Number(n) => n,
        ArgKind::Term => eval_term(app, raw, Ty::Num).n,
    }
}

/// 評価済みのコマンド引数 `value` (= `arg_as_string(raw)`) から、原文 `raw` の
/// `GetArgAsDouble` を求める。項の数値は文字列値を数値として読んだものと同じなので
/// (`EvalTerm` の数値期待)、引数をもう一度評価せずに済む (`Random` 等を 2 度呼ばない)。
pub(super) fn evaluated_arg_as_double(raw: &str, value: &str) -> f64 {
    match arg_kind(raw) {
        ArgKind::Literal(_) => 0.0,
        ArgKind::Number(n) => n,
        ArgKind::Term => str_to_dbl(value),
    }
}

/// [`evaluated_arg_as_double`] の整数版 (原典 `GetArgAsLong`。偶数丸め)。
pub(super) fn evaluated_arg_as_long(raw: &str, value: &str) -> i64 {
    vb_clng(evaluated_arg_as_double(raw, value))
}

/// 評価済みの文字列を整数として読む (原典 `StrToLng`: 数値として読めれば偶数丸め、
/// 読めなければ 0)。`GetArgAsString` の値を数値化するコマンド (`PaintString` / `PaintPicture`
/// の座標、`Create` のランク等の `CInt(buf)`) が使う。
pub(super) fn str_to_lng(s: &str) -> i64 {
    if is_numeric(s) {
        vb_clng(str_to_dbl(s))
    } else {
        0
    }
}

/// `Print #f, msg` の書き込み文字列 (原典 `ExecPrintCmd`)。`msg` はハンドルより後ろの原文
/// (末尾の `;` は除去済み)。バッククォートで囲まれていればそのまま (`$(...)` も展開しない)。
/// それ以外は、全体が `$(式)` ならその値、1 項ならその値を取り、最後に `$(...)` を展開する。
pub(super) fn print_text(app: &App, msg: &str) -> String {
    if msg.len() >= 2 && msg.starts_with('`') && msg.ends_with('`') {
        return msg[1..msg.len() - 1].to_string();
    }
    let msg = if msg.starts_with("$(") {
        if msg.ends_with(')') {
            value_as_string(app, &msg[2..msg.len() - 1])
        } else {
            msg.to_string()
        }
    } else if list_split(msg).is_some_and(|t| t.len() == 1) {
        value_as_string(app, msg)
    } else {
        msg.to_string()
    };
    replace_sub_expression(app, &msg)
}

/// 関数の引数 1 つの値 (原典 `CallFunction` の `GetValueAsString(params(i), is_term(i))`)。
/// 空白を含まない引数は項として、含む引数は式として評価する。
pub(super) fn param_value(app: &App, param: &str) -> String {
    match list_split(param) {
        Some(terms) if terms.len() == 1 => eval_term(app, param, Ty::Str).s,
        _ => value_as_string(app, param),
    }
}

/// 演算子の種類 (原典 `OperatorType`)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Expo,
    Mult,
    Div,
    IntDiv,
    Mod,
    Plus,
    Minus,
    Cat,
    Eq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,
    Like,
    Not,
    And,
    Or,
}

/// 項 `t` が演算子なら (種類, 優先度, 右端優先か) を返す。右端優先 (`true`) の演算子は
/// 同じ優先度が後に出れば置き換える (= 左結合)。`Or` / `And` / `Not` は左端を採る。
fn operator_of(t: &str) -> Option<(Op, u8, bool)> {
    let op = match t {
        "^" => (Op::Expo, 10, true),
        "*" => (Op::Mult, 9, true),
        "/" => (Op::Div, 9, true),
        "\\" => (Op::IntDiv, 8, true),
        "+" => (Op::Plus, 6, true),
        "-" => (Op::Minus, 6, true),
        "&" => (Op::Cat, 5, true),
        "<" => (Op::Lt, 4, true),
        "=" => (Op::Eq, 4, true),
        ">" => (Op::Gt, 4, true),
        "!=" | "<>" => (Op::NotEq, 4, true),
        "<=" => (Op::LtEq, 4, true),
        ">=" => (Op::GtEq, 4, true),
        _ => match t.to_ascii_lowercase().as_str() {
            "mod" => (Op::Mod, 7, true),
            // 原典は Like を優先度 7 以上のときだけ採用し、採用後の優先度は 4 とする。
            "like" => (Op::Like, 4, true),
            "not" => (Op::Not, 3, false),
            "and" => (Op::And, 2, false),
            "or" => (Op::Or, 1, false),
            _ => return None,
        },
    };
    Some(op)
}

/// 式を評価する (原典 `EvalExpr`)。
pub(super) fn eval_expr(app: &App, expr: &str, ety: Ty) -> Val {
    let Some(terms) = list_split(expr) else {
        // 括弧の対応が取れていない文字列
        return if ety == Ty::Num {
            Val::num(0.0)
        } else {
            Val::str(expr)
        };
    };
    match terms.len() {
        0 => {
            return Val {
                ty: ety,
                ..Val::default()
            }
        }
        1 => return eval_term(app, &terms[0], ety),
        _ => {}
    }

    // 優先度に合わせ、どの演算を実行するか判定する。
    let mut op_idx: Option<usize> = None;
    let mut op_type = Op::Plus;
    let mut op_pri: u8 = 100;
    for (i, t) in terms.iter().enumerate() {
        let Some((op, pri, rightmost)) = operator_of(t) else {
            continue;
        };
        // 原典: Like は「現在の優先度 >= 7」のときだけ採用する。
        let take = match op {
            Op::Like => op_pri >= 7,
            _ if rightmost => op_pri >= pri,
            _ => op_pri > pri,
        };
        if take {
            op_type = op;
            op_pri = pri;
            op_idx = Some(i);
        }
    }
    let Some(op_idx) = op_idx else {
        // 単なる文字列
        return Val::str(expr);
    };

    // 演算子の引数。片側が 1 項なら項として評価する。
    let (lop, is_lop_term) = match op_idx {
        0 => (String::new(), true),
        1 => (terms[0].clone(), true),
        _ => (terms[..op_idx].join(" "), false),
    };
    let (rop, is_rop_term) = if op_idx == terms.len() - 1 {
        (terms[terms.len() - 1].clone(), true)
    } else {
        (terms[op_idx + 1..].join(" "), false)
    };
    let side = |s: &str, is_term: bool, t: Ty| {
        if is_term {
            eval_term(app, s, t)
        } else {
            eval_expr(app, s, t)
        }
    };
    let lnum = || side(&lop, is_lop_term, Ty::Num).n;
    let rnum = || side(&rop, is_rop_term, Ty::Num).n;
    let lstr = || side(&lop, is_lop_term, Ty::Str).s;
    let rstr = || side(&rop, is_rop_term, Ty::Str).s;

    match op_type {
        Op::Plus => Val::num_as(lnum() + rnum(), ety),
        Op::Minus => Val::num_as(lnum() - rnum(), ety),
        Op::Mult => Val::num_as(lnum() * rnum(), ety),
        Op::Div => {
            let (l, r) = (lnum(), rnum());
            Val::num_as(if r != 0.0 { l / r } else { 0.0 }, ety)
        }
        Op::IntDiv => {
            // VB6 `\`: 両辺を整数へ丸めて (偶数丸め) から整数除算する。
            let (l, r) = (lnum(), rnum());
            let (l, r) = (vb_clng(l), vb_clng(r));
            Val::num_as(if r != 0 { (l / r) as f64 } else { 0.0 }, ety)
        }
        Op::Mod => {
            // VB6 `Mod`: 両辺を整数へ丸めて (偶数丸め) から剰余を取る (符号は左辺)。
            let (l, r) = (vb_clng(lnum()), vb_clng(rnum()));
            Val::num_as(if r != 0 { (l % r) as f64 } else { 0.0 }, ety)
        }
        Op::Expo => Val::num_as(lnum().powf(rnum()), ety),
        Op::Cat => {
            let s = lstr() + &rstr();
            if ety == Ty::Num {
                Val::num(str_to_dbl(&s))
            } else {
                Val::str(s)
            }
        }
        Op::Eq | Op::NotEq => {
            let equal = if is_number(&lop) || is_number(&rop) {
                lnum() == rnum()
            } else {
                lstr() == rstr()
            };
            Val::bool_as(equal == (op_type == Op::Eq), ety)
        }
        Op::Lt => Val::bool_as(lnum() < rnum(), ety),
        Op::LtEq => Val::bool_as(lnum() <= rnum(), ety),
        Op::Gt => Val::bool_as(lnum() > rnum(), ety),
        Op::GtEq => Val::bool_as(lnum() >= rnum(), ety),
        Op::Like => Val::bool_as(like_match(&lstr(), &rstr()), ety),
        Op::Not => Val::bool_as(rnum() == 0.0, ety),
        Op::And => {
            if lnum() == 0.0 {
                return Val::bool_as(false, ety);
            }
            Val::bool_as(rnum() != 0.0, ety)
        }
        Op::Or => {
            if lnum() != 0.0 {
                return Val::bool_as(true, ety);
            }
            Val::bool_as(rnum() != 0.0, ety)
        }
    }
}

/// 1 項を評価する (原典 `EvalTerm`)。
pub(super) fn eval_term(app: &App, expr: &str, ety: Ty) -> Val {
    let Some(first) = expr.chars().next() else {
        return Val::default();
    };
    match first {
        // タブを含む項は Trim のため式として評価する。
        '\t' => return eval_expr(app, expr, ety),
        ' ' => return eval_term(app, expr.trim(), ety),
        '"' => {
            let mut v = Val::str(if expr.len() >= 2 && expr.ends_with('"') {
                replace_sub_expression(app, &expr[1..expr.len() - 1])
            } else {
                expr.to_string()
            });
            if ety != Ty::Str {
                v.n = str_to_dbl(&v.s);
            }
            return v;
        }
        // 色指定
        '#' => return Val::str(expr),
        '(' => {
            if expr.len() >= 2 && expr.ends_with(')') {
                return eval_expr(app, &expr[1..expr.len() - 1], ety);
            }
            let mut v = Val::str(expr);
            if ety != Ty::Str {
                v.n = str_to_dbl(expr);
            }
            return v;
        }
        '+' | '-' | '0'..='9' if is_numeric(expr) => {
            return if ety == Ty::Str {
                Val::str(expr)
            } else {
                Val::num(str_to_dbl(expr))
            };
        }
        '`' => {
            let mut v = Val::str(if expr.len() >= 2 && expr.ends_with('`') {
                expr[1..expr.len() - 1].to_string()
            } else {
                expr.to_string()
            });
            if ety != Ty::Str {
                v.n = str_to_dbl(&v.s);
            }
            return v;
        }
        _ => {}
    }
    // 本移植の拡張: クォートの外の `$(...)` もクォート内と同じく展開し、文字列の項とする
    // (`If $(x) = 1`)。原典では `$(x)` という名前の未定義変数 (文字列) になるため、
    // 実シナリオはこの書き方をしない (条件式 16,625 行で 0 件。`"..."` 内だけで使う)。
    // 関数呼び出し (`Llength($(x))`) と配列 (`a[$(i)]`) は各経路が `$(...)` を展開する。
    let is_whole_sub_expr = expr.starts_with("$(")
        && find_matching_paren(expr, 1).is_some_and(|close| close == expr.len() - 1);
    if is_whole_sub_expr {
        return sub_expr_term(app, expr, ety);
    }
    if let Some(v) = call_function(app, expr, ety) {
        return v;
    }
    if expr.contains("$(") && !expr.ends_with(']') {
        return sub_expr_term(app, expr, ety);
    }
    get_variable(app, expr, ety)
}

/// `$(...)` を展開した文字列の項 (クォート文字列と同じ扱い)。
fn sub_expr_term(app: &App, expr: &str, ety: Ty) -> Val {
    let mut v = Val::str(replace_sub_expression(app, expr));
    if ety != Ty::Str {
        v.n = str_to_dbl(&v.s);
    }
    v
}

/// 項 `expr` 全体が組み込み関数呼び出しなら評価する (原典 `CallFunction`)。
/// 関数でなければ `None`。引数の評価は `eval_script_function` が行う。
fn call_function(app: &App, expr: &str, ety: Ty) -> Option<Val> {
    let (name, args, total) = take_function_call(expr, 0)?;
    if total != expr.len() {
        return None;
    }
    let s = eval_script_function(app, name, args)?;
    let mut v = Val::str(s);
    if ety != Ty::Str {
        v.n = str_to_dbl(&v.s);
    }
    Some(v)
}

/// 変数を評価する (原典 `GetVariable`)。システム変数 → 配列要素 → スクリプト変数。
/// 未定義なら文字列値は変数名そのもの (未定義の配列要素は空)・数値は 0。
fn get_variable(app: &App, name: &str, ety: Ty) -> Val {
    let with_num = |s: String| {
        let mut v = Val::str(s);
        if ety != Ty::Str {
            v.n = str_to_dbl(&v.s);
        }
        v
    };
    if let Some(sys) = system_variable_value(app, name) {
        return with_num(sys);
    }
    let is_array = name.len() > 2 && name.ends_with(']') && name.find('[').is_some_and(|i| i > 0);
    let key = if is_array {
        resolve_lhs_name(app, name)
    } else {
        name.to_string()
    };
    if app.is_script_var_defined(&key) {
        return with_num(app.script_var(&key).to_string());
    }
    Val {
        ty: Ty::Str,
        s: if is_array {
            String::new()
        } else {
            name.to_string()
        },
        n: 0.0,
    }
}

/// 文字列中の `$(式)` を式の値で置き換える (原典 `ReplaceSubExpression`)。
pub(super) fn replace_sub_expression(app: &App, s: &str) -> String {
    let mut out = s.to_string();
    let mut search_from = 0;
    while let Some(rel) = out[search_from..].find("$(") {
        let start = search_from + rel;
        let Some(close) = find_matching_paren(&out, start + 1) else {
            break;
        };
        let value = value_as_string(app, &out[start + 2..close]);
        out.replace_range(start..=close, &value);
        search_from = start + value.len();
    }
    out
}

/// 空白で項に分解する (原典 `ListSplit`)。`"..."` / `` `...` `` / `(...)` / `[...]`
/// の内側では区切らない。括弧の対応が取れなければ `None`。
pub(super) fn list_split(s: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut in_dq, mut in_bq) = (false, false);
    let mut depth = 0i32;
    for c in s.chars() {
        if !in_dq && !in_bq && depth == 0 && (c == ' ' || c == '\t') {
            if !cur.is_empty() {
                out.push(std::mem::take(&mut cur));
            }
            continue;
        }
        if in_bq {
            if c == '`' {
                in_bq = false;
            }
        } else if in_dq {
            if c == '"' {
                in_dq = false;
            }
        } else {
            match c {
                '(' | '[' => depth += 1,
                ')' | ']' => {
                    depth -= 1;
                    if depth < 0 {
                        return None;
                    }
                }
                '`' => in_bq = true,
                '"' => in_dq = true,
                _ => {}
            }
        }
        cur.push(c);
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    if depth != 0 || in_dq || in_bq {
        return None;
    }
    Some(out)
}

/// VB の `IsNumeric` 相当: 空白を除いて 10 進数として読めるか
/// (符号・小数点・桁区切り `,` を許す。指数表記は許さない)。
pub(super) fn is_numeric(s: &str) -> bool {
    let t: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let body = t.strip_prefix(['+', '-']).unwrap_or(&t);
    if body.is_empty() {
        return false;
    }
    let (int_part, frac_part) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };
    let int_ok = int_part.chars().all(|c| c.is_ascii_digit() || c == ',');
    let frac_ok = match frac_part {
        Some(f) => f.chars().all(|c| c.is_ascii_digit()),
        None => true,
    };
    let has_digit = body.chars().any(|c| c.is_ascii_digit());
    int_ok && frac_ok && has_digit && !int_part.starts_with(',')
}

/// 原典 `IsNumber`: 数値として読めて、かつ `(` で始まらない (`(1)` を除外)。
pub(super) fn is_number(s: &str) -> bool {
    is_numeric(s) && !s.starts_with('(')
}

/// 原典 `StrToDbl` (`double.TryParse`、失敗なら 0)。前後の空白・符号・小数点・
/// 指数・桁区切り `,` を受け付ける。
pub(super) fn str_to_dbl(s: &str) -> f64 {
    let t = s.trim();
    if t.is_empty()
        || !t
            .chars()
            .all(|c| c.is_ascii_digit() || matches!(c, '+' | '-' | '.' | 'e' | 'E' | ','))
    {
        return 0.0;
    }
    t.replace(',', "").parse::<f64>().unwrap_or(0.0)
}

/// VB の `CLng` (偶数丸め) 相当。範囲外は飽和させる。
fn vb_clng(x: f64) -> i64 {
    if x.is_finite() {
        round_half_to_even(x) as i64
    } else {
        0
    }
}

/// `If` / `ElseIf` の条件式を評価する (原典 `ExecIfCmd`、項数は構文解析時に決まる)。
///
/// - 1 項 (`$` / `(` で始まらない) でパイロット名なら、そのパイロットが盤上
///   (出撃 / 格納) にいるか。パイロット名でなければ項を整数として評価する。
/// - 2 項 `Not X` (X が `$` / `(` で始まらない) で X がパイロット名なら、盤上に
///   いないか。パイロット名でなければ X が 0 か。
/// - それ以外は式を整数として評価し、0 以外を真とする。
///
/// 注: C# 移植 (SRC.Sharp) は項数を記録し忘れており、常に式として評価する。
pub(super) fn if_condition(app: &App, expr: &str) -> bool {
    let terms = list_split(expr).unwrap_or_default();
    let is_expr_head = |t: &str| t.starts_with('$') || t.starts_with('(');
    match terms.as_slice() {
        [t] if !is_expr_head(t) => match pilot_on_board(app, t) {
            Some(on_board) => on_board,
            None => term_as_long(app, t) != 0,
        },
        // 本移植の拡張: `IsDead X` / `Killed X` / `IsAlive X` / `Alive X` (ユニットの生死)。
        [pred, t] if is_unit_predicate(pred) => {
            let alive = unit_alive(app, t);
            if pred.eq_ignore_ascii_case("isalive") || pred.eq_ignore_ascii_case("alive") {
                alive
            } else {
                !alive
            }
        }
        [not, t] if not.eq_ignore_ascii_case("not") && !is_expr_head(t) => {
            match pilot_on_board(app, t) {
                Some(on_board) => !on_board,
                None => term_as_long(app, t) == 0,
            }
        }
        _ => value_as_long(app, expr) != 0,
    }
}

fn is_unit_predicate(t: &str) -> bool {
    ["isdead", "killed", "isalive", "alive"]
        .iter()
        .any(|p| t.eq_ignore_ascii_case(p))
}

/// `name` がパイロット名なら、そのパイロットのユニットが盤上にいるかを返す。
/// パイロット名でなければ `None`。
fn pilot_on_board(app: &App, name: &str) -> Option<bool> {
    let db = app.database();
    let defined = db
        .pilot_instances
        .iter()
        .any(|p| p.id == name || p.pilot_data_name == name)
        || db.unit_instances.iter().any(|u| u.pilot_name == name);
    if !defined {
        return None;
    }
    Some(db.unit_instances.iter().any(|u| u.pilot_name == name))
}

/// `If` / `ElseIf` の条件を評価する。`Call(<label>)` があれば先にサブルーチンを実行して
/// 返り値へ置き換える (`&mut App` が要るため評価器の外で行う)。
pub(super) fn condition_with_calls(app: &mut App, text: &str) -> bool {
    if text.to_ascii_lowercase().contains("call(") {
        let processed = preprocess_call_expressions_in_condition(app, text);
        if_condition(app, &processed)
    } else {
        if_condition(app, text)
    }
}
