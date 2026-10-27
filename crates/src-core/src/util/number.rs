//! 数値ユーティリティ: VB5 互換の丸め (`round_half_to_even`) と `Format` 書式。

/// 銀行丸め (round half to even)。`f64::round_ties_even` 相当だが、
/// MSRV 1.75 では未安定 (1.77 安定) のため手動実装する。
/// 半端ちょうど (小数部 0.5) のときのみ偶数側へ丸め、それ以外は最近接。
pub(crate) fn round_half_to_even(x: f64) -> f64 {
    let floor = x.floor();
    let diff = x - floor;
    if diff < 0.5 {
        floor
    } else if diff > 0.5 {
        floor + 1.0
    } else if (floor as i64) % 2 == 0 {
        floor
    } else {
        floor + 1.0
    }
}

/// VB5 風の `Format(value, "##,##0.00")` パターンを最小限実装。
/// - `0` / `#` の数で小数部桁数を決定（末尾は固定 / トリム）。
/// - `,` がパターン内にあれば整数部に桁区切りを挿入。
/// - `%` は数値を 100 倍し、末尾に `%` を付与（`%` 1 個につき ×100）。
///   SRC.NET の `Format`(= VB5 Format) 準拠。`%` の出力位置は末尾に統一。
/// - その他の未対応文字は無視。
pub(crate) fn format_with_pattern(v: f64, fmt: &str) -> String {
    // `%` をパターンから取り除き、個数分だけ値を 100 倍する。
    let pct_count = fmt.chars().filter(|c| *c == '%').count();
    if pct_count == 0 {
        return format_number_pattern(v, fmt);
    }
    let scaled = v * 100f64.powi(pct_count as i32);
    let numeric_fmt: String = fmt.chars().filter(|c| *c != '%').collect();
    let body = format_number_pattern(scaled, &numeric_fmt);
    format!("{body}{}", "%".repeat(pct_count))
}

/// `format_with_pattern` の数値整形コア (`%` を含まないパターン専用)。
pub(crate) fn format_number_pattern(v: f64, fmt: &str) -> String {
    // 小数部の `0` / `#` 数で digit_count を決める
    let (int_part_fmt, frac_part_fmt) = match fmt.split_once('.') {
        Some((a, b)) => (a, b),
        None => (fmt, ""),
    };
    let zero_digits = frac_part_fmt.chars().filter(|c| *c == '0').count();
    let max_digits = frac_part_fmt
        .chars()
        .filter(|c| *c == '0' || *c == '#')
        .count();
    let want_thousands = int_part_fmt.contains(',');
    // 整数部の `0` 数を数えて左 0 埋めの最小桁数を決める。
    // VB5 `Format(42, "00000")` → "00042" のような桁固定 zero-padding 対応。
    let int_zero_pad: usize = int_part_fmt.chars().filter(|c| *c == '0').count();

    // VB5 `Format` は銀行丸め (round half to even) を用いる。
    // SRC.NET `VB.Compatibility.VB5.Support.Format` 準拠。
    // 例: Format(0.5,"0")="0"、Format(2.5,"0")="2"、Format(0.125,"0.00")="0.12"。
    let factor = 10f64.powi(max_digits as i32);
    let rounded = round_half_to_even(v * factor) / factor;
    let sign = if rounded < 0.0 { "-" } else { "" };
    let abs = rounded.abs();
    let int_v = abs.trunc() as i64;
    let mut int_str = int_v.to_string();
    // 整数部左 0 埋め
    if int_zero_pad > int_str.len() {
        let pad = int_zero_pad - int_str.len();
        int_str = "0".repeat(pad) + &int_str;
    }
    if want_thousands {
        int_str = insert_thousands_separator(&int_str);
    }
    if max_digits == 0 {
        return format!("{sign}{int_str}");
    }
    // 小数部
    let frac_val = abs - abs.trunc();
    let mut frac_str = format!("{:.*}", max_digits, frac_val);
    // "0.xxx" の先頭 "0." を取り除く
    if let Some(dot) = frac_str.find('.') {
        frac_str = frac_str[dot + 1..].to_string();
    }
    // 末尾の余分な 0 を削るが、zero_digits までは保持
    while frac_str.len() > zero_digits && frac_str.ends_with('0') {
        frac_str.pop();
    }
    if frac_str.is_empty() {
        return format!("{sign}{int_str}");
    }
    format!("{sign}{int_str}.{frac_str}")
}

pub(crate) fn insert_thousands_separator(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in chars.iter().enumerate() {
        let from_right = chars.len() - i;
        if i > 0 && from_right % 3 == 0 {
            out.push(',');
        }
        out.push(*c);
    }
    out
}

/// 数値の標準表記 (原典 `FormatNum`)。整数は `Format$(n, "0")`、小数は
/// `Format$(n, "0.###…")` = 有効数字 15 桁・指数表記なし・末尾の 0 は付けない
/// (例: `10 / 3` → `3.33333333333333`、`0.1 + 0.2` → `0.3`)。
pub(crate) fn format_num(v: f64) -> String {
    if !v.is_finite() {
        return format!("{v}");
    }
    if v.fract() == 0.0 && v.abs() < 1.0e15 {
        return format!("{}", v as i64);
    }
    let int_digits = v.abs().log10().floor() as i32 + 1;
    let decimals = (15 - int_digits).max(0) as usize;
    let s = format!("{v:.decimals$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    } else {
        s
    }
}
