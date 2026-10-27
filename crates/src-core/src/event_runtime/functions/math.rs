//! 組み込み関数: 数値計算・論理。

use super::*;

pub(super) fn eval(app: &App, c: &FnCall) -> Option<Option<String>> {
    Some(match c.canon.as_str() {
        "Min" => fn_min(app, c),
        "Max" => fn_max(app, c),
        "Abs" => fn_abs(app, c),
        "Int" => fn_int(app, c),
        "Eval" => fn_eval(app, c),
        "Round" => fn_round(app, c),
        "RoundUp" => fn_round_up(app, c),
        "RoundDown" => fn_round_down(app, c),
        "Sqr" => fn_sqr(app, c),
        "Sin" => fn_sin(app, c),
        "Cos" => fn_cos(app, c),
        "Tan" => fn_tan(app, c),
        "Atn" => fn_atn(app, c),
        "Log" => fn_log(app, c),
        "Sgn" => fn_sgn(app, c),
        "Mod" => fn_mod(app, c),
        "Atan2" => fn_atan2(app, c),
        "IsNumeric" => fn_is_numeric(app, c),
        "IIF" | "Iif" => fn_iif(app, c),
        "Not" => fn_not(app, c),
        "Random" => fn_random(app, c),
        _ => return None,
    })
}

fn fn_min(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Min(a, b, ...)` — 1 引数以上をサポート。SRC.Sharp と同等。
    if args.is_empty() {
        return None;
    }
    let mut result = numeric_arg(args[0])?;
    for arg in &args[1..] {
        if let Some(v) = numeric_arg(arg) {
            if v < result {
                result = v;
            }
        }
    }
    Some(format_num(result))
}

fn fn_max(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Max(a, b, ...)` — 1 引数以上をサポート。SRC.Sharp と同等。
    if args.is_empty() {
        return None;
    }
    let mut result = numeric_arg(args[0])?;
    for arg in &args[1..] {
        if let Some(v) = numeric_arg(arg) {
            if v > result {
                result = v;
            }
        }
    }
    Some(format_num(result))
}

fn fn_abs(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    let v = numeric_arg(args.first()?)?;
    Some(format_num(v.abs()))
}

fn fn_int(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Int(x)` — 数値以下の最大整数 (floor)。SRC 仕様で負数は -8.3 → -9。
    // 数値でない文字列は 0。
    let v = numeric_arg(args.first()?).unwrap_or(0.0);
    Some(format_num(v.floor()))
}

fn fn_eval(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Eval(expr)` (原典 `CallFunction` の "eval"): 引数の値 (評価済み) を、もう一度
    // 式として評価する (`Eval("n + 1")` / `Eval(式を入れた変数)`)。
    Some(crate::event_runtime::eval::value_as_string(
        app,
        args.first()?.trim(),
    ))
}

fn fn_round(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Round(x [, digits])` — 四捨五入。digits=0 なら整数。
    // SRC.NET `Expression.cs::CallFunction "round"` 準拠で +∞ 方向への
    // 半数切り上げ (floor(scaled) 後、小数部 >= 0.5 なら +1)。
    // VB5 の銀行丸めとも Rust の round (ゼロから遠ざける) とも異なり、
    // 負数では +∞ 方向に丸める。例: Round(-2.5) = -2、Round(2.5) = 3。
    let v = numeric_arg(args.first()?)?;
    let d: i32 = args
        .get(1)
        .and_then(|s| numeric_arg(s))
        .map(|x| x as i32)
        .unwrap_or(0);
    let factor = 10f64.powi(d);
    let scaled = v * factor;
    let mut n = scaled.floor();
    if scaled - n >= 0.5 {
        n += 1.0;
    }
    Some(format_num(n / factor))
}

fn fn_round_up(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `RoundUp(x [, digits])` — 切り上げ。
    let v = numeric_arg(args.first()?)?;
    let d: i32 = args
        .get(1)
        .and_then(|s| numeric_arg(s))
        .map(|x| x as i32)
        .unwrap_or(0);
    let factor = 10f64.powi(d);
    Some(format_num((v * factor).ceil() / factor))
}

fn fn_round_down(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `RoundDown(x [, digits])` — 切り捨て。
    let v = numeric_arg(args.first()?)?;
    let d: i32 = args
        .get(1)
        .and_then(|s| numeric_arg(s))
        .map(|x| x as i32)
        .unwrap_or(0);
    let factor = 10f64.powi(d);
    Some(format_num((v * factor).floor() / factor))
}

fn fn_sqr(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    let v = numeric_arg(args.first()?)?;
    Some(format_num(v.sqrt()))
}

fn fn_sin(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    let v = numeric_arg(args.first()?)?;
    Some(format_num(v.sin()))
}

fn fn_cos(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    let v = numeric_arg(args.first()?)?;
    Some(format_num(v.cos()))
}

fn fn_tan(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    let v = numeric_arg(args.first()?)?;
    Some(format_num(v.tan()))
}

fn fn_atn(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    let v = numeric_arg(args.first()?)?;
    Some(format_num(v.atan()))
}

fn fn_log(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Log(x)` — 自然対数 (VB5 `Log` 同等、SRC 準拠)。x <= 0 は None。
    let v = numeric_arg(args.first()?)?;
    if v <= 0.0 {
        return None;
    }
    Some(format_num(v.ln()))
}

fn fn_sgn(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Sgn(x)` — 符号関数: x > 0 → 1, x < 0 → -1, x == 0 → 0。
    // VB5 `Sgn` 同等 (`Math.Sign`)。
    let v = numeric_arg(args.first()?)?;
    Some(format_num(if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }))
}

fn fn_mod(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Mod(a, b)` — 整数剰余。`a Mod b` 演算子も VB5/SRC で存在するが、
    // 関数形式は本実装で `Mod(a, b)` をサポートする (SRC.Sharp parity)。
    // 0 除算は 0 を返す (SRC は実行時エラーだが本実装は黙殺)。
    if args.len() < 2 {
        return None;
    }
    let a = numeric_arg(args[0])?;
    let b = numeric_arg(args[1])?;
    if b == 0.0 {
        return Some("0".to_string());
    }
    // VB5 `Mod` は整数演算: a, b を整数に切り捨ててから剰余。
    let ai = a as i64;
    let bi = b as i64;
    if bi == 0 {
        return Some("0".to_string());
    }
    Some(format_num((ai % bi) as f64))
}

fn fn_atan2(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Atan2(y, x)` — 2 引数版 arctangent。象限を返す。
    if args.len() < 2 {
        return None;
    }
    let y = numeric_arg(args[0])?;
    let x = numeric_arg(args[1])?;
    Some(format_num(y.atan2(x)))
}

fn fn_is_numeric(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    let s = args.first()?.to_string();
    // SRC.Sharp 互換: .NET `decimal.TryParse` 準拠。
    // 科学的表記法 (1e5, 1E10) および NaN / Infinity は数値扱いしない。
    let t = s.trim();
    let is_num = !t.is_empty()
        && !t.as_bytes().iter().any(|&b| b == b'e' || b == b'E')
        && t.parse::<f64>().is_ok_and(|v| v.is_finite());
    Some(if is_num { "1" } else { "0" }.to_string())
}

fn fn_iif(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `IIF(cond, a, b)` — 条件式が真なら a、偽なら b。
    // 原典 `CallFunction` の "iif": 条件は `If` と同じ規則、結果は選んだ側の式の値。
    if args.len() < 3 {
        return None;
    }
    let cond = crate::event_runtime::eval::if_condition(app, args[0].trim());
    let chosen = if cond { args[1] } else { args[2] };
    Some(crate::event_runtime::eval::value_as_string(
        app,
        chosen.trim(),
    ))
}

fn fn_not(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Not(cond)` 関数形式 (SRC のヘルパ)
    let v = args.first()?.trim();
    Some(if v.is_empty() || v == "0" {
        "1".to_string()
    } else {
        "0".to_string()
    })
}

fn fn_random(app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // SRC `Random(n)` = `GeneralLib.Dice(n)` 相当で **1..n** を返す
    // (`(int)(n * rand[0,1)) + 1`)。`n <= 1` のときは n をそのまま返す。
    // `Lindex(list, Random(Llength(list)))` のように 1-indexed の
    // `Lindex` 引数に直結する用法が多いため 0..n-1 では破綻する。
    let n: i64 = args.first()?.to_string().trim().parse().ok()?;
    if n <= 1 {
        return Some(n.max(0).to_string());
    }
    let v = pseudo_random(app) % (n as u32) + 1;
    Some(v.to_string())
}
