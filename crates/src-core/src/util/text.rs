//! 文字列ユーティリティ: Shift-JIS バイト長基準の文字列操作 (VB5 の `LeftB` 等)、
//! 全角/半角変換、`Like` 演算子のパターンマッチ。

/// 名前から UnitInstance を引く。`uid` (Create で採番した一意 ID) /
/// `unit_data_name` / `pilot_name` のいずれかが一致すれば真。
/// SRC (VB5 `Option Compare Text` / Collection キー照合) はユニット/パイロット名の
/// 照合で全角英数字を半角と同一視する。比較用に全角 ASCII (U+FF01–U+FF5E) を
/// 半角 (U+0021–U+007E) へ畳み込む。例: `バ５` → `バ5`。
///
/// 公式サンプル1話は `Create … バ5`（半角）で生成したバルアドを `X(バ５)`/`Y(バ５)`
/// （全角）で参照してユニガル防衛波を配置する。この正規化が無いと参照が解決できず
/// 全波が誤座標に湧く。
pub(crate) fn fold_fullwidth_ascii(s: &str) -> String {
    s.chars()
        .map(|c| {
            let u = c as u32;
            if (0xFF01..=0xFF5E).contains(&u) {
                char::from_u32(u - 0xFEE0).unwrap_or(c)
            } else {
                c
            }
        })
        .collect()
}

/// VB5 `Like` 演算子のパターンマッチ。
///
/// パターン特殊文字:
/// - `*`  任意の 0 文字以上にマッチ
/// - `?`  任意の 1 文字にマッチ
/// - `#`  任意の 1 桁数字にマッチ
/// - `[charlist]`  charlist 中の 1 文字にマッチ（`A-Z` 範囲記法対応）
/// - `[!charlist]` charlist に含まれない 1 文字にマッチ
/// - その他 → リテラル比較（大文字小文字区別あり）
pub(crate) fn like_match(s: &str, pattern: &str) -> bool {
    fn match_at(s: &[char], si: usize, p: &[char], pi: usize) -> bool {
        let mut si = si;
        let mut pi = pi;
        while pi < p.len() {
            match p[pi] {
                '*' => {
                    // 連続する * をスキップ
                    while pi < p.len() && p[pi] == '*' {
                        pi += 1;
                    }
                    if pi == p.len() {
                        return true; // * で終わり → 何でもマッチ
                    }
                    // 残りのパターンが s の各位置からマッチするか試みる
                    for i in si..=s.len() {
                        if match_at(s, i, p, pi) {
                            return true;
                        }
                    }
                    return false;
                }
                '?' => {
                    if si >= s.len() {
                        return false;
                    }
                    si += 1;
                    pi += 1;
                }
                '#' => {
                    if si >= s.len() || !s[si].is_ascii_digit() {
                        return false;
                    }
                    si += 1;
                    pi += 1;
                }
                '[' => {
                    // 閉じ `]` を探す
                    let close = p[pi + 1..].iter().position(|&c| c == ']');
                    if let Some(rel) = close {
                        let class_start = pi + 1;
                        let class_end = class_start + rel; // exclusive
                        let close_idx = class_end + 1; // position of ']' in p
                        let class = &p[class_start..class_end];
                        let negate = class.first() == Some(&'!');
                        let class = if negate { &class[1..] } else { class };
                        if si >= s.len() {
                            return false;
                        }
                        let in_class = like_char_class_match(s[si], class);
                        if negate == in_class {
                            return false; // (negate&&in_class) or (!negate&&!in_class)
                        }
                        si += 1;
                        pi = close_idx + 1;
                    } else {
                        // 閉じ ] なし → `[` をリテラルとして扱う
                        if si >= s.len() || s[si] != '[' {
                            return false;
                        }
                        si += 1;
                        pi += 1;
                    }
                }
                lit => {
                    if si >= s.len() || s[si] != lit {
                        return false;
                    }
                    si += 1;
                    pi += 1;
                }
            }
        }
        si == s.len()
    }

    let sc: Vec<char> = s.chars().collect();
    let pc: Vec<char> = pattern.chars().collect();
    match_at(&sc, 0, &pc, 0)
}

/// `Like` 演算子の文字クラス (`[A-Z]`, `[aeiou]` 等) マッチ。
pub(crate) fn like_char_class_match(c: char, class: &[char]) -> bool {
    let mut i = 0;
    while i < class.len() {
        if i + 2 < class.len() && class[i + 1] == '-' {
            if c >= class[i] && c <= class[i + 2] {
                return true;
            }
            i += 3;
        } else {
            if c == class[i] {
                return true;
            }
            i += 1;
        }
    }
    false
}

/// 半角カタカナ (U+FF61..=U+FF9D) → 全角カタカナの基底対応表。
/// 添字は `code - 0xFF61`。濁点 (FF9E) / 半濁点 (FF9F) は別途合成処理する。
pub(crate) const HALFWIDTH_KANA_BASE: [char; 61] = [
    '。', '「', '」', '、', '・', // FF61..=FF65
    'ヲ', 'ァ', 'ィ', 'ゥ', 'ェ', 'ォ', 'ャ', 'ュ', 'ョ', 'ッ', // FF66..=FF6F
    'ー', // FF70
    'ア', 'イ', 'ウ', 'エ', 'オ', // FF71..=FF75
    'カ', 'キ', 'ク', 'ケ', 'コ', // FF76..=FF7A
    'サ', 'シ', 'ス', 'セ', 'ソ', // FF7B..=FF7F
    'タ', 'チ', 'ツ', 'テ', 'ト', // FF80..=FF84
    'ナ', 'ニ', 'ヌ', 'ネ', 'ノ', // FF85..=FF89
    'ハ', 'ヒ', 'フ', 'ヘ', 'ホ', // FF8A..=FF8E
    'マ', 'ミ', 'ム', 'メ', 'モ', // FF8F..=FF93
    'ヤ', 'ユ', 'ヨ', // FF94..=FF96
    'ラ', 'リ', 'ル', 'レ', 'ロ', // FF97..=FF9B
    'ワ', 'ン', // FF9C..=FF9D
];

/// 全角カタカナに濁点を合成した文字を返す。濁点不可なら `None`。
pub(crate) fn fullwidth_voiced(base: char) -> Option<char> {
    match base {
        'ウ' => Some('ヴ'),
        'ワ' => Some('ヷ'),
        'ヲ' => Some('ヺ'),
        // カ行〜ト・ハ行は Unicode 上で清音 +1 が濁音になる。
        'カ' | 'キ' | 'ク' | 'ケ' | 'コ' | 'サ' | 'シ' | 'ス' | 'セ' | 'ソ' | 'タ' | 'チ'
        | 'ツ' | 'テ' | 'ト' | 'ハ' | 'ヒ' | 'フ' | 'ヘ' | 'ホ' => {
            char::from_u32(base as u32 + 1)
        }
        _ => None,
    }
}

/// 全角カタカナに半濁点を合成した文字を返す。半濁点不可なら `None`。
pub(crate) fn fullwidth_semivoiced(base: char) -> Option<char> {
    match base {
        // ハ行は清音 +2 が半濁音 (パピプペポ)。
        'ハ' | 'ヒ' | 'フ' | 'ヘ' | 'ホ' => char::from_u32(base as u32 + 2),
        _ => None,
    }
}

/// 半角文字を全角に変換する (VB5 `StrConv(s, vbWide)` 相当)。
/// - 半角空白 (0x20) → 全角空白 (U+3000)
/// - 半角 ASCII (0x21..=0x7E) → 全角形 (U+FF01..=U+FF5E)
/// - 半角カタカナ (U+FF61..=U+FF9D) → 全角カタカナ。直後の濁点 ﾞ (FF9E) /
///   半濁点 ﾟ (FF9F) は合成する (ｶﾞ → ガ、ﾊﾟ → パ)。合成できない場合は
///   単独の全角濁点 ゛(U+309B) / 半濁点 ゜(U+309C) を出力。
/// - それ以外 (ひらがな・漢字・既に全角の文字) は不変。
pub(crate) fn to_fullwidth(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let code = c as u32;
        match code {
            0x20 => out.push('\u{3000}'),
            0x21..=0x7E => out.push(char::from_u32(code + 0xFEE0).unwrap_or(c)),
            0xFF61..=0xFF9D => {
                let base = HALFWIDTH_KANA_BASE[(code - 0xFF61) as usize];
                let next = chars.get(i + 1).copied();
                if next == Some('\u{FF9E}') {
                    if let Some(voiced) = fullwidth_voiced(base) {
                        out.push(voiced);
                        i += 2;
                        continue;
                    }
                } else if next == Some('\u{FF9F}') {
                    if let Some(semi) = fullwidth_semivoiced(base) {
                        out.push(semi);
                        i += 2;
                        continue;
                    }
                }
                out.push(base);
            }
            0xFF9E => out.push('\u{309B}'), // 単独の濁点
            0xFF9F => out.push('\u{309C}'), // 単独の半濁点
            _ => out.push(c),
        }
        i += 1;
    }
    out
}

/// 1 文字あたりの Shift-JIS バイト数を返す (ASCII → 1, 非ASCII → 2)
#[inline]
pub(crate) fn sjis_char_bytes(c: char) -> usize {
    if c.is_ascii() {
        1
    } else {
        2
    }
}

/// Shift-JIS バイト長を返す
pub(crate) fn sjis_byte_len(s: &str) -> usize {
    s.chars().map(sjis_char_bytes).sum()
}

/// 先頭から `byte_count` バイト分の部分文字列を返す (Shift-JIS 換算)
pub(crate) fn sjis_left(s: &str, byte_count: usize) -> String {
    if byte_count == 0 {
        return String::new();
    }
    let mut bytes_used = 0usize;
    let mut result = String::new();
    for c in s.chars() {
        let cb = sjis_char_bytes(c);
        if bytes_used + cb > byte_count {
            break;
        }
        bytes_used += cb;
        result.push(c);
    }
    result
}

/// 末尾から `byte_count` バイト分の部分文字列を返す (Shift-JIS 換算)
pub(crate) fn sjis_right(s: &str, byte_count: usize) -> String {
    if byte_count == 0 {
        return String::new();
    }
    let total = sjis_byte_len(s);
    if byte_count >= total {
        return s.to_string();
    }
    // 先頭から (total - byte_count) バイトをスキップ
    let skip = total - byte_count;
    let mut bytes_skipped = 0usize;
    let mut result = String::new();
    for c in s.chars() {
        let cb = sjis_char_bytes(c);
        if bytes_skipped + cb <= skip {
            bytes_skipped += cb;
        } else {
            result.push(c);
        }
    }
    result
}

/// `start` バイト目 (1-indexed) から `length` バイト分の部分文字列を返す (Shift-JIS 換算)
pub(crate) fn sjis_mid(s: &str, start: usize, length: Option<usize>) -> String {
    if start == 0 {
        return String::new();
    }
    let skip = start - 1; // 0-indexed byte offset
    let mut bytes_acc = 0usize;
    let mut result = String::new();
    let mut in_range = false;
    let mut taken = 0usize;
    for c in s.chars() {
        let cb = sjis_char_bytes(c);
        if !in_range {
            if bytes_acc + cb > skip {
                in_range = true;
            } else {
                bytes_acc += cb;
                continue;
            }
        }
        // in range
        if let Some(max) = length {
            if taken + cb > max {
                break;
            }
        }
        taken += cb;
        result.push(c);
    }
    result
}

/// `s` 内で `pattern` が最初に出現するバイト位置 (1-indexed) を返す (Shift-JIS 換算)
/// `start` は検索開始バイト位置 (1-indexed)。見つからない場合は 0。
pub(crate) fn sjis_instr(s: &str, pattern: &str, start: usize) -> usize {
    if pattern.is_empty() {
        return 1;
    }
    let s_chars: Vec<char> = s.chars().collect();
    let p_chars: Vec<char> = pattern.chars().collect();
    // s の各文字に対応するバイト位置の累積
    let mut byte_pos = 1usize; // 1-indexed
    for si in 0..s_chars.len() {
        let cb = sjis_char_bytes(s_chars[si]);
        if byte_pos < start {
            byte_pos += cb;
            continue;
        }
        // s[si..] が pattern で始まるか確認
        if si + p_chars.len() <= s_chars.len() && s_chars[si..si + p_chars.len()] == p_chars[..] {
            return byte_pos;
        }
        byte_pos += cb;
    }
    0
}

/// `s` 内で `pattern` が最後に出現するバイト位置 (1-indexed) を返す (Shift-JIS 換算)
/// `end_byte` は検索終端バイト位置 (1-indexed, inclusive)。None なら末尾まで。
/// 見つからない場合は 0。
pub(crate) fn sjis_instrrev(s: &str, pattern: &str, end_byte: Option<usize>) -> usize {
    if pattern.is_empty() {
        return sjis_byte_len(s) + 1;
    }
    let s_chars: Vec<char> = s.chars().collect();
    let p_chars: Vec<char> = pattern.chars().collect();
    // 各文字の開始バイト位置 (1-indexed) を収集
    let mut char_byte_starts: Vec<usize> = Vec::with_capacity(s_chars.len());
    let mut bp = 1usize;
    for &c in &s_chars {
        char_byte_starts.push(bp);
        bp += sjis_char_bytes(c);
    }
    let effective_end = end_byte.unwrap_or(usize::MAX);
    let mut last_found = 0usize;
    for si in 0..s_chars.len() {
        if char_byte_starts[si] > effective_end {
            break;
        }
        if si + p_chars.len() <= s_chars.len() && s_chars[si..si + p_chars.len()] == p_chars[..] {
            last_found = char_byte_starts[si];
        }
    }
    last_found
}

/// SRC.Sharp `GeneralLib.StrWidth` 相当: 半角文字は 1、全角文字は 2 として
/// 文字列の表示幅を返す。
/// - ASCII 0x20–0xFF (半角英数・記号) と半角カタカナ (ｦ–ﾟ / U+FF66–U+FF9F) は 1
/// - それ以外 (全角 CJK・全角英数・全角記号 等) は 2
pub(crate) fn str_display_width(s: &str) -> usize {
    s.chars()
        .map(|c| {
            let n = c as u32;
            if (0x20..=0xFF).contains(&n) || (0xFF66..=0xFF9F).contains(&n) {
                1
            } else {
                2
            }
        })
        .sum()
}
