//! 組み込み関数: 文字列・リスト操作。

use super::*;

pub(super) fn eval(app: &App, c: &FnCall) -> Option<Option<String>> {
    Some(match c.canon.as_str() {
        "List" => fn_list(app, c),
        "Llength" => fn_llength(app, c),
        "Lindex" => fn_lindex(app, c),
        "Lsearch" => fn_lsearch(app, c),
        "Lsplit" => fn_lsplit(app, c),
        "Lremove" => fn_lremove(app, c),
        "Replace" => fn_replace(app, c),
        "String" => fn_string(app, c),
        "Wide" => fn_wide(app, c),
        "LCase" => fn_lcase(app, c),
        "UCase" => fn_ucase(app, c),
        "Trim" => fn_trim(app, c),
        "Asc" => fn_asc(app, c),
        "Chr" => fn_chr(app, c),
        "InStrRev" => fn_in_str_rev(app, c),
        "Hex" => fn_hex(app, c),
        "Oct" => fn_oct(app, c),
        "Format" => fn_format(app, c),
        "InStr" => fn_in_str(app, c),
        "Len" => fn_len(app, c),
        "LenB" => fn_len_b(app, c),
        "LeftB" => fn_left_b(app, c),
        "RightB" => fn_right_b(app, c),
        "MidB" => fn_mid_b(app, c),
        "InStrB" => fn_in_str_b(app, c),
        "InStrRevB" => fn_in_str_rev_b(app, c),
        "RGB" | "Rgb" => fn_rgb(app, c),
        "RegExp" | "RegExpMatch" => fn_reg_exp(app, c),
        "RegExpReplace" => fn_reg_exp_replace(app, c),
        "StrCmp" | "StrCompare" => fn_str_cmp(app, c),
        "Left" => fn_left(app, c),
        "Right" => fn_right(app, c),
        "LSet" => fn_lset(app, c),
        "RSet" => fn_rset(app, c),
        "Mid" => fn_mid(app, c),
        _ => return None,
    })
}

fn fn_list(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `List(a, b, c)` → "a b c" 空白区切り。
    // 各引数は評価済み (原典 `List` は GetValueAsString で各引数を評価する。
    // 括弧式 `(Info(マップ,高さ) - 3)` も値になっている)。
    let parts: Vec<String> = args.iter().map(|a| a.to_string()).collect();
    Some(parts.join(" "))
}

fn fn_llength(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Llength(s)` — 空白区切りトークン数。`s` は裸識別子なら変数解決。
    let s = args.first()?.to_string();
    Some(
        s.split_whitespace()
            .filter(|w| !w.is_empty())
            .count()
            .to_string(),
    )
}

fn fn_lindex(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Lindex(s, n)` — 1-indexed 要素。SRC.Sharp 仕様:
    // - 範囲外 / index 0 / 負数 → 空文字
    // - 要素が `(...)` で囲まれていれば外側 paren を剥がす
    //   (`Functions/List.cs::LIndex` line 52-55)
    if args.len() < 2 {
        return None;
    }
    let s = args[0].to_string();
    // signed parse で負数も拾う
    let n_signed: i64 = args[1].to_string().parse().ok()?;
    if n_signed <= 0 {
        return Some(String::new());
    }
    let n = n_signed as usize;
    let tokens: Vec<&str> = s.split_whitespace().collect();
    if n > tokens.len() {
        return Some(String::new());
    }
    let raw = tokens[n - 1];
    // 全体が `(...)` で囲まれていれば外側 paren を 1 段剥がす
    let stripped = if raw.len() >= 2 && raw.starts_with('(') && raw.ends_with(')') {
        &raw[1..raw.len() - 1]
    } else {
        raw
    };
    Some(stripped.to_string())
}

fn fn_lsearch(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Lsearch(list, item [, start])` — list 内で item の出現位置
    // (1-indexed) を返す。見つからなければ **0**。
    // SRC.Sharp `Expressions/Functions/List.cs::LSearch` 同等
    // (line 78-106 で not found 時 0 を返す)。
    //
    // 第 3 引数 `start` で開始位置 (1-indexed) 指定可。デフォルト 1。
    if args.len() < 2 {
        return None;
    }
    let s = args[0].to_string();
    let needle = args[1].to_string();
    let start: usize = args
        .get(2)
        .and_then(|s| s.to_string().parse().ok())
        .unwrap_or(1)
        .max(1);
    let tokens: Vec<&str> = s.split_whitespace().collect();
    for (i, t) in tokens.iter().enumerate().skip(start - 1) {
        if *t == needle {
            return Some((i + 1).to_string());
        }
    }
    Some("0".to_string())
}

fn fn_lsplit(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Lsplit(s, sep)` — sep で s を split し、空白区切りリストに再構成。
    if args.len() < 2 {
        return None;
    }
    let s = args[0].to_string();
    let sep = args[1].to_string();
    let parts: Vec<&str> = s.split(&sep as &str).collect();
    Some(parts.join(" "))
}

fn fn_lremove(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Lremove(s, item)` — s から item を 1 件だけ除去したリストを返す。
    if args.len() < 2 {
        return None;
    }
    let s = args[0].to_string();
    let item = args[1].to_string();
    let mut out: Vec<&str> = Vec::new();
    let mut removed = false;
    for t in s.split_whitespace() {
        if !removed && t == item {
            removed = true;
            continue;
        }
        out.push(t);
    }
    Some(out.join(" "))
}

fn fn_replace(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Replace(s, find, replace[, start[, count]])` — 文字列置換
    // 4-arg: Replace(s, find, replace, start)
    //   → Left(s, start-1) + Right(s, Len-start+1).replace(find, replace)
    // 5-arg: Replace(s, find, replace, start, count)
    //   → Left(s, start-1) + Mid(s, start, count).replace(find, replace)
    //     + Right(s, Len - (start+count-1) - 1)
    if args.len() < 3 {
        return None;
    }
    let s = args[0].to_string();
    let find = args[1].to_string();
    let rep = args[2].to_string();
    // SRC.Sharp 準拠: 空文字列を find に渡すと C# は ArgumentException を
    // 投げるが、Rust では元の文字列をそのまま返す (SRC シナリオでは未使用)。
    if find.is_empty() {
        return Some(s);
    }
    let s_chars: Vec<char> = s.chars().collect();
    let len = s_chars.len();
    if args.len() >= 5 {
        // 5-arg: Replace(s, find, replace, start, count)
        // SRC.Sharp formula: Left(s, start-1) + Mid(s,start,count).replace()
        //   + Right(s, Len - (start+count-1) - 1)
        let start = (numeric_arg(args[3]).unwrap_or(1.0) as i64).max(1) as usize;
        let count = (numeric_arg(args[4]).unwrap_or(0.0) as i64).max(0) as usize;
        let si = (start - 1).min(len);
        let ei = (si + count).min(len);
        let prefix: String = s_chars[..si].iter().collect();
        let mid: String = s_chars[si..ei].iter().collect();
        let mid_replaced = mid.replace(&find, &rep);
        let right_n = (len as i64 - (start as i64 + count as i64 - 1) - 1).max(0) as usize;
        let tail: String = s_chars[len.saturating_sub(right_n)..].iter().collect();
        Some(prefix + &mid_replaced + &tail)
    } else if args.len() == 4 {
        // 4-arg: start only
        let start = (numeric_arg(args[3]).unwrap_or(1.0) as i64).max(1) as usize;
        let si = (start - 1).min(len);
        let prefix: String = s_chars[..si].iter().collect();
        let suffix: String = s_chars[si..].iter().collect();
        let suffix_replaced = suffix.replace(&find, &rep);
        Some(prefix + &suffix_replaced)
    } else {
        // 3-arg: replace all
        Some(s.replace(&find, &rep))
    }
}

fn fn_string(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `String(count, s)` — s を count 回繰り返した文字列。
    // VB5 では String(N, char) で char 単一文字だが、SRC は `String(3, "0 ")`
    // のように任意文字列も渡される (スパロボ戦記 で 6 回使用)。
    if args.len() < 2 {
        return None;
    }
    let n: usize = numeric_arg(args[0]).map(|v| v.max(0.0) as usize)?;
    let s = args[1].to_string();
    Some(s.repeat(n))
}

fn fn_wide(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Wide(s)` — 半角文字を全角に変換。VB5 の StrConv(s, vbWide) 相当。
    // 半角 ASCII / 空白に加え、半角カタカナ (濁点・半濁点の合成含む) も
    // 全角カタカナへ変換する (SRC.NET `Expression.cs` の VbStrConv.Wide 準拠)。
    let s = args.first()?.to_string();
    Some(to_fullwidth(&s))
}

fn fn_lcase(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `LCase(s)` — ASCII 大文字を小文字に。日本語は不変。
    let s = args.first()?.to_string();
    Some(s.to_lowercase())
}

fn fn_ucase(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `UCase(s)` — ASCII 小文字を大文字に。日本語は不変。
    let s = args.first()?.to_string();
    Some(s.to_uppercase())
}

fn fn_trim(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Trim(s)` — 先頭末尾の半角空白を除去。内部空白は保持。
    // VB5 の Trim$ は全角空白 (U+3000) は除去しない。
    let s = args.first()?.to_string();
    Some(s.trim_matches(' ').to_string())
}

fn fn_asc(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Asc(s)` — VB5 互換: SJIS (CP932) 経由で文字コードを返す。
    // ASCII (0..=0x7F) は素直にそのバイト値、半角カナ等の単バイト SJIS は
    // そのバイト値、2 バイト SJIS は `high << 8 | low` (例: "あ" → 0x82A0)。
    // SJIS にエンコードできない文字は Unicode コードポイントに fallback。
    // SRC.Sharp は `Strings.Asc()` を使うので同じ値になる。
    let s = args.first()?.to_string();
    let s = s.trim_matches('"');
    let Some(first) = s.chars().next() else {
        return Some("0".to_string());
    };
    let mut buf = [0u8; 4];
    let utf8 = first.encode_utf8(&mut buf);
    let (sjis, _, had_unmappable) = encoding_rs::SHIFT_JIS.encode(utf8);
    let code: u32 = if had_unmappable {
        first as u32
    } else {
        match sjis.as_ref() {
            [b] => *b as u32,
            [hi, lo] => (*hi as u32) << 8 | (*lo as u32),
            _ => first as u32,
        }
    };
    Some(code.to_string())
}

fn fn_chr(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Chr(n)` — VB5 互換: SJIS (CP932) コードを解釈して 1 文字を返す。
    // - 0..=0xFF: 単バイト SJIS (ASCII / 半角カナ / 制御コード)
    // - 0x100..: 2 バイト SJIS `high << 8 | low` として decode (例: 0x82A0 → "あ")
    // SRC.Sharp の Chr は `(char)long` で Unicode コードポイント扱い (XXX
    // 文字コード という FIXME 付き) だが、こちらは VB5 寄りで実装する。
    let n: u32 = numeric_arg(args.first()?).map(|v| v.max(0.0) as u32)?;
    let bytes: Vec<u8> = if n <= 0xFF {
        vec![n as u8]
    } else {
        vec![(n >> 8) as u8, (n & 0xFF) as u8]
    };
    let (cow, _, had_errors) = encoding_rs::SHIFT_JIS.decode(&bytes);
    if had_errors {
        // SJIS として無効なら Unicode コードポイントに fallback
        return Some(char::from_u32(n).map(|c| c.to_string()).unwrap_or_default());
    }
    Some(cow.into_owned())
}

fn fn_in_str_rev(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `InStrRev(s1, s2 [, start])` — s1 内で s2 が最後に現れる位置 (1-indexed)。
    // start 省略時は -1 (末尾から検索)。見つからなければ 0。
    if args.len() < 2 {
        return None;
    }
    let s1 = args[0].to_string();
    let s2 = args[1].to_string();
    if s2.is_empty() {
        return Some("0".to_string());
    }
    let chars: Vec<char> = s1.chars().collect();
    let start: i64 = args
        .get(2)
        .and_then(|s| s.to_string().parse().ok())
        .unwrap_or(-1);
    let upto: usize = if start < 0 {
        chars.len()
    } else {
        (start as usize).min(chars.len())
    };
    let haystack: String = chars[..upto].iter().collect();
    if let Some(pos) = haystack.rfind(s2.as_str()) {
        let char_pos = haystack[..pos].chars().count();
        return Some((char_pos + 1).to_string());
    }
    Some("0".to_string())
}

fn fn_hex(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Hex(n)` — 整数の 16 進表現 (大文字、prefix 無し)。
    let v = numeric_arg(args.first()?)?;
    let n = v as i64;
    Some(format!("{n:X}"))
}

fn fn_oct(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Oct(n)` — 整数の 8 進表現 (prefix 無し)。
    let v = numeric_arg(args.first()?)?;
    let n = v as i64;
    Some(format!("{n:o}"))
}

fn fn_format(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Format(value, "fmt")` — 簡易実装: 数値ならそのまま、"#,##0" 形式は
    // 桁区切りカンマを付与、"0.00" 形式は小数桁固定、`%` は 100 倍 + %付与。
    // それ以外の VB5 format 構文 ($, E+ 等) はサポートしない。
    let v_str = args.first()?.to_string();
    let fmt = args.get(1).map(|s| s.to_string()).unwrap_or_default();
    let fmt = fmt.trim_matches('"');
    if fmt.is_empty() {
        return Some(v_str);
    }
    let v: f64 = match v_str.parse() {
        Ok(n) => n,
        Err(_) => return Some(v_str),
    };
    Some(format_with_pattern(v, fmt))
}

fn fn_in_str(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `InStr(s1, s2 [, start])` — s1 内で s2 が始まる位置 (1-indexed)。
    // 見つからなければ 0。
    if args.len() < 2 {
        return None;
    }
    let s1 = args[0].to_string();
    let s2 = args[1].to_string();
    let start: usize = args
        .get(2)
        .and_then(|s| s.to_string().parse().ok())
        .unwrap_or(1);
    let start_idx = start.saturating_sub(1);
    let chars: Vec<char> = s1.chars().collect();
    if start_idx >= chars.len() {
        return Some("0".to_string());
    }
    let haystack: String = chars[start_idx..].iter().collect();
    if let Some(pos) = haystack.find(s2.as_str()) {
        // byte 位置を char 位置に変換
        let char_pos = haystack[..pos].chars().count();
        return Some((start_idx + char_pos + 1).to_string());
    }
    Some("0".to_string())
}

fn fn_len(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // 文字列長 (UTF-8 文字数)
    let s = args.first()?.to_string();
    Some(s.chars().count().to_string())
}

fn fn_len_b(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // LenB(s) — Shift-JIS エンコードでのバイト数 (ASCII=1, 非ASCII=2)
    let s = args.first()?.to_string();
    Some(sjis_byte_len(&s).to_string())
}

fn fn_left_b(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // LeftB(s, byteCount) — 先頭 n バイト (Shift-JIS 換算)
    let s = args.first()?.to_string();
    let n = numeric_arg(args.get(1)?).unwrap_or(0.0) as usize;
    Some(sjis_left(&s, n))
}

fn fn_right_b(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // RightB(s, byteCount) — 末尾 n バイト (Shift-JIS 換算)
    let s = args.first()?.to_string();
    let n = numeric_arg(args.get(1)?).unwrap_or(0.0) as usize;
    Some(sjis_right(&s, n))
}

fn fn_mid_b(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // MidB(s, start[, length]) — バイト位置から substring (1-indexed, Shift-JIS 換算)
    let s = args.first()?.to_string();
    let start = (numeric_arg(args.get(1)?).unwrap_or(1.0) as i64).max(1) as usize;
    if args.len() >= 3 {
        let length = numeric_arg(args.get(2)?).unwrap_or(0.0) as usize;
        Some(sjis_mid(&s, start, Some(length)))
    } else {
        Some(sjis_mid(&s, start, None))
    }
}

fn fn_in_str_b(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // InStrB(s, pattern[, start]) — バイト位置で最初の出現を探す (Shift-JIS 換算, 1-indexed)
    let s = args.first()?.to_string();
    let pattern = args.get(1)?.to_string();
    let start_byte = if args.len() >= 3 {
        (numeric_arg(args.get(2)?).unwrap_or(1.0) as i64).max(1) as usize
    } else {
        1
    };
    Some(sjis_instr(&s, &pattern, start_byte).to_string())
}

fn fn_in_str_rev_b(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // InStrRevB(s, pattern[, start]) — バイト位置で最後の出現を探す (Shift-JIS 換算)
    let s = args.first()?.to_string();
    let pattern = args.get(1)?.to_string();
    let end_byte = if args.len() >= 3 {
        Some((numeric_arg(args.get(2)?).unwrap_or(0.0) as i64).max(1) as usize)
    } else {
        None
    };
    Some(sjis_instrrev(&s, &pattern, end_byte).to_string())
}

fn fn_rgb(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `RGB(r, g, b)` → "#rrggbb" 形式の16進カラー文字列 (SRC.Sharp 準拠)
    // 各引数は変数・算術式を含む可能性があるため numeric_arg で評価する。
    if args.len() < 3 {
        return None;
    }
    let r = numeric_arg(args[0]).unwrap_or(0.0) as i32;
    let g = numeric_arg(args[1]).unwrap_or(0.0) as i32;
    let b = numeric_arg(args[2]).unwrap_or(0.0) as i32;
    let r = r.clamp(0, 255) as u8;
    let g = g.clamp(0, 255) as u8;
    let b = b.clamp(0, 255) as u8;
    Some(format!("#{r:02x}{g:02x}{b:02x}"))
}

fn fn_reg_exp(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // SRC `RegExp(string, pattern [, 大小区別あり|大小区別なし])` (`正規表現関数.md`):
    // マッチした文字列を返す。マッチしなければ空文字列。
    // デフォルトは「大小区別あり」(case-sensitive)。
    // 元 SRC は VBScript.RegExp (Perl 互換に近い)。本実装は Rust `regex`
    // クレートで近似。Perl の `\d` `\w` 等は同等、ECMAScript の
    // 後方参照等は非対応 (regex crate の制約)。
    if args.len() < 2 {
        return None;
    }
    let s = args[0].to_string();
    let pat = args[1].to_string();
    let case_insensitive = matches!(
        args.get(2).map(|a| a.to_string()).as_deref(),
        Some("大小区別なし") | Some("CaseInsensitive") | Some("Insensitive")
    );
    // RegexBuilder で case_insensitive を明示的に指定する (default-features
    // off では `(?i)` プレフィックスが unicode-case を要求するため代替)。
    let result = regex::RegexBuilder::new(&pat)
        .case_insensitive(case_insensitive)
        .build()
        .map(|re| {
            re.find(&s)
                .map(|m| m.as_str().to_string())
                .unwrap_or_default()
        })
        .unwrap_or_default();
    Some(result)
}

fn fn_reg_exp_replace(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // SRC `RegExpReplace(string, searchPattern, replacePattern [, 大小区別あり|大小区別なし])`:
    // 置換結果を返す。デフォルトは「大小区別あり」(case-sensitive)。
    if args.len() < 3 {
        return None;
    }
    let s = args[0].to_string();
    let search = args[1].to_string();
    let replace = args[2].to_string();
    let case_insensitive = matches!(
        args.get(3).map(|a| a.to_string()).as_deref(),
        Some("大小区別なし") | Some("CaseInsensitive") | Some("Insensitive")
    );
    let result = regex::RegexBuilder::new(&search)
        .case_insensitive(case_insensitive)
        .build()
        .map(|re| re.replace_all(&s, replace.as_str()).into_owned())
        .unwrap_or_else(|_| s.clone());
    Some(result)
}

fn fn_str_cmp(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `StrCmp(a, b)` → 0 (一致) / -1 (a < b) / 1 (a > b)
    if args.len() < 2 {
        return None;
    }
    let a = args[0].to_string();
    let b = args[1].to_string();
    Some(
        match a.cmp(&b) {
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Greater => 1,
        }
        .to_string(),
    )
}

fn fn_left(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Left(s, n)` — 先頭 n 文字。`n` は `(te - 1)` のような
    // 変数入り算術式でありうるので app-aware に評価する。
    if args.len() < 2 {
        return None;
    }
    let s = args[0].to_string();
    let n = int_arg(args[1]).max(0) as usize;
    Some(s.chars().take(n).collect())
}

fn fn_right(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Right(s, n)` — 末尾 n 文字
    if args.len() < 2 {
        return None;
    }
    let s = args[0].to_string();
    let n = int_arg(args[1]).max(0) as usize;
    let count = s.chars().count();
    let skip = count.saturating_sub(n);
    Some(s.chars().skip(skip).collect())
}

fn fn_lset(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `LSet(str, width)` — 左寄せ、右側を空白で `width` までパディング
    // (`width` より長い場合はそのまま返す)。SRC.Sharp
    // `Functions/String.cs::LSet` 同等。
    // 幅は `str_display_width` (全角=2, 半角=1) で計算する。
    // 第1引数は "(式)" 形式の数値式になりうるため numeric_arg で評価を試みる。
    // 例: `LSet((改造段階[ＨＰ] * 費用 + 費用), 6)` → `"3000  "`
    if args.len() < 2 {
        return None;
    }
    let s = if let Some(n) = numeric_arg(args[0]) {
        format!("{}", n as i64)
    } else {
        args[0].to_string()
    };
    let w: usize = numeric_arg(args[1])? as usize;
    let dw = str_display_width(&s);
    if dw >= w {
        Some(s)
    } else {
        Some(format!("{s}{}", " ".repeat(w - dw)))
    }
}

fn fn_rset(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `RSet(str, width)` — 右寄せ、左側を空白でパディング。
    // 幅は `str_display_width` (全角=2, 半角=1) で計算する。
    // 第1引数は "(式)" 形式の数値式になりうるため numeric_arg で評価を試みる。
    // 例: `RSet((改造段階[ＨＰ] * 費用 + 費用), 6)` → `"  3000"`
    if args.len() < 2 {
        return None;
    }
    let s = if let Some(n) = numeric_arg(args[0]) {
        format!("{}", n as i64)
    } else {
        args[0].to_string()
    };
    let w: usize = numeric_arg(args[1])? as usize;
    let dw = str_display_width(&s);
    if dw >= w {
        Some(s)
    } else {
        Some(format!("{}{s}", " ".repeat(w - dw)))
    }
}

fn fn_mid(_app: &App, c: &FnCall) -> Option<String> {
    let FnCall { args, .. } = *c;
    // `Mid(s, start, [len])` — 1-indexed 部分文字列。
    // `start` / `len` は変数入り算術式でありうるので app-aware に評価。
    if args.len() < 2 {
        return None;
    }
    let s = args[0].to_string();
    let start = int_arg(args[1]).max(0) as usize;
    let skip = start.saturating_sub(1);
    if let Some(len_arg) = args.get(2) {
        let n = int_arg(len_arg).max(0) as usize;
        Some(s.chars().skip(skip).take(n).collect())
    } else {
        Some(s.chars().skip(skip).collect())
    }
}
