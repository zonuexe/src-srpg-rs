//! SRC 組み込み関数 (`Args` / `Pilot()` / `Mid()` 等) の評価。

use super::*;

/// `src[i..]` の先頭が ASCII 英字で始まる関数呼出 `Name(args)` か判定。
/// 戻り値: `(fn_name, args_inside_parens, consumed_byte_count)`。
/// `src[open..]` の位置にある `(` に対応する `)` の index を返す。
/// `open` の位置は `(` でなければならない。`(` / `)` の数が合わなければ `None`。
pub(super) fn find_matching_paren(src: &str, open: usize) -> Option<usize> {
    let bytes = src.as_bytes();
    if open >= bytes.len() || bytes[open] != b'(' {
        return None;
    }
    let mut depth = 1i32;
    let mut k = open + 1;
    while k < bytes.len() {
        match bytes[k] {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(k);
                }
            }
            _ => {}
        }
        k += 1;
    }
    None
}

pub(super) fn take_function_call(src: &str, i: usize) -> Option<(&str, &str, usize)> {
    let bytes = src.as_bytes();
    if i >= bytes.len() || !bytes[i].is_ascii_alphabetic() {
        return None;
    }
    let mut j = i + 1;
    while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
        j += 1;
    }
    if j >= bytes.len() || bytes[j] != b'(' {
        return None;
    }
    // 対応する `)` を括弧ネストで探す。`"..."` 内の括弧はネストとして
    // 数えない (`InStr(Info(...),"(")` のような引数に対応するため)。
    let mut depth = 1;
    let mut in_quote = false;
    let mut k = j + 1;
    while k < bytes.len() {
        match bytes[k] {
            b'"' => in_quote = !in_quote,
            b'(' if !in_quote => depth += 1,
            b')' if !in_quote => {
                depth -= 1;
                if depth == 0 {
                    let fn_name = &src[i..j];
                    let args_str = &src[j + 1..k];
                    return Some((fn_name, args_str, k + 1 - i));
                }
            }
            _ => {}
        }
        k += 1;
    }
    None
}

/// 関数呼び出しを評価して文字列値を返す。対応外なら `None`。
/// 名前は ASCII 範囲で case-insensitive (SRC 流: `Llength` / `LLength` どちらも可)。
pub(super) fn eval_script_function(app: &App, name: &str, args_str: &str) -> Option<String> {
    let args: Vec<&str> = split_function_args(args_str);
    let canon = canonical_function_name(name);
    match canon.as_str() {
        "Args" => {
            // Args(N) — 値は Set 時に Args(N) というキーで script_var に格納済み。
            // N は数値リテラルだけでなく変数でも良い (`Args(配置用乱数)` 等) ため、
            // まず `fn_arg_value` で解決してから数値判定する。
            let n = fn_arg_value(app, args.first()?);
            if !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()) {
                Some(app.script_var(&format!("Args({n})")).to_string())
            } else {
                None
            }
        }
        // 以下 unit-query 関数群は SRC.Sharp `AUnitFunction` 同等の規約:
        // unit が解決できなければ **数値 0 (= "0")** を返す (literal を残さない)。
        // 旧実装は `find_unit().map(...)` で None フォールスルーしていたため、
        // 未解決 unit で式が展開されず literal が残る問題があった。
        "HP" => Some(
            find_unit(app, args.first().copied().unwrap_or(""))
                .map(|u| {
                    let max = app.database().effective_max_hp(u);
                    (max - u.damage).max(0).to_string()
                })
                .unwrap_or_else(|| "0".to_string()),
        ),
        "MaxHP" => Some(
            find_unit(app, args.first().copied().unwrap_or(""))
                .map(|u| app.database().effective_max_hp(u).to_string())
                .unwrap_or_else(|| "0".to_string()),
        ),
        "EN" => Some(
            find_unit(app, args.first().copied().unwrap_or(""))
                .map(|u| {
                    let max = app.database().effective_max_en(u);
                    (max - u.en_consumed).max(0).to_string()
                })
                .unwrap_or_else(|| "0".to_string()),
        ),
        "MaxEN" => Some(
            find_unit(app, args.first().copied().unwrap_or(""))
                .map(|u| app.database().effective_max_en(u).to_string())
                .unwrap_or_else(|| "0".to_string()),
        ),
        "Armor" => Some(
            find_unit(app, args.first().copied().unwrap_or(""))
                .map(|u| app.database().effective_armor(u).to_string())
                .unwrap_or_else(|| "0".to_string()),
        ),
        "Mobility" => Some(
            find_unit(app, args.first().copied().unwrap_or(""))
                .map(|u| app.database().effective_mobility(u).to_string())
                .unwrap_or_else(|| "0".to_string()),
        ),
        "Speed" => Some(
            find_unit(app, args.first().copied().unwrap_or(""))
                .map(|u| app.database().effective_speed(u).to_string())
                .unwrap_or_else(|| "0".to_string()),
        ),
        "Action" => {
            // `Action(unit)` — 残り行動数。`has_acted` で 0/1 判定。
            // SRC.Sharp は二回行動等を考慮するが、本実装は最大 1 で十分。
            // 未解決 unit は 0 (= 行動不能扱い)。
            let key = fn_arg_value(app, args.first()?);
            let n = find_unit(app, &key)
                .map(|u| if u.has_acted { 0 } else { 1 })
                .unwrap_or(0);
            Some(n.to_string())
        }
        "Damage" => {
            // `Damage(unit)` — 損傷率 %  = damage * 100 / max_hp。
            // 0 = 無傷、100 = 破壊済。未解決 unit は 0。
            let key = fn_arg_value(app, args.first()?);
            let pct = find_unit(app, &key)
                .map(|u| {
                    let max = app.database().effective_max_hp(u);
                    if max <= 0 {
                        0
                    } else {
                        (u.damage * 100 / max).clamp(0, 100) as i32
                    }
                })
                .unwrap_or(0);
            Some(pct.to_string())
        }
        "Condition" => {
            // `Condition(unit, status)` — unit が status の影響下なら 1。
            // status は精神/状態異常名 (`熱血` / `麻痺` 等)。`HasStatus` の
            // alias でもあるが原典の関数名はこちらが正典。
            if args.len() < 2 {
                return None;
            }
            let key = fn_arg_value(app, args[0]);
            let status = fn_arg_value(app, args[1]);
            let status = status.trim().trim_matches('"');
            let has = find_unit(app, &key)
                .map(|u| u.has_condition(status))
                .unwrap_or(false);
            Some(if has { "1" } else { "0" }.to_string())
        }
        "Status" => {
            // `Status(unit)` — ユニットの状態文字列 (SRC `ユニット情報関数.md`):
            //   - `出撃` ─ マップ上に存在
            //   - `待機` ─ 出撃していない
            //   - `格納` ─ 母艦に収納されている
            //   - `離脱` ─ `Leave` コマンドで戦線から離れている
            //   - `破壊` ─ 破壊されている (unit_instances に残しているケース)
            //   - `破棄` ─ `RemoveUnit` / `RemovePilot` で削除されている
            // 優先順: 明示的に life_state がセットされていればそれ、未設定なら
            // off_map から `出撃` / `待機` を自動判定、未解決なら `破棄`。
            let key = fn_arg_value(app, args.first()?);
            Some(match find_unit(app, &key) {
                Some(u) if !u.life_state.is_empty() => u.life_state.clone(),
                Some(u) if u.off_map => "待機".to_string(),
                Some(_) => "出撃".to_string(),
                None => "破棄".to_string(),
            })
        }
        "Bullet" => {
            // `Bullet(unit, weapon)` — 残弾数。-1 (無限弾) の武器は -1 を返す。
            // 未解決 unit / 武器名不一致は 0。
            if args.len() < 2 {
                return None;
            }
            let key = fn_arg_value(app, args[0]);
            let wname = fn_arg_value(app, args[1]);
            let wname = wname.trim().trim_matches('"');
            let n = find_unit(app, &key)
                .and_then(|u| {
                    app.database()
                        .unit_by_name(&u.unit_data_name)
                        .and_then(|d| d.weapons.iter().enumerate().find(|(_, w)| w.name == wname))
                        .map(|(i, w)| {
                            if w.bullet < 0 {
                                -1
                            } else {
                                u.weapons
                                    .get(i)
                                    .map(|uw| uw.bullet_remaining)
                                    .unwrap_or(w.bullet)
                            }
                        })
                })
                .unwrap_or(0);
            Some(n.to_string())
        }
        "MaxBullet" => {
            // `MaxBullet(unit, weapon)` — 武器の最大弾数。データ参照のみ。
            if args.len() < 2 {
                return None;
            }
            let key = fn_arg_value(app, args[0]);
            let wname = fn_arg_value(app, args[1]);
            let wname = wname.trim().trim_matches('"');
            let n = find_unit(app, &key)
                .and_then(|u| {
                    app.database()
                        .unit_by_name(&u.unit_data_name)
                        .and_then(|d| d.weapons.iter().find(|w| w.name == wname))
                        .map(|w| w.bullet)
                })
                .unwrap_or(0);
            Some(n.to_string())
        }
        "HasItem" => {
            if args.len() < 2 {
                return None;
            }
            let unit = find_unit(app, args[0])?;
            let item = args[1].trim().trim_matches('"');
            let has = unit.has_item_equipped(item);
            Some(if has { "1" } else { "0" }.to_string())
        }
        "HasStatus" => {
            if args.len() < 2 {
                return None;
            }
            let unit = find_unit(app, args[0])?;
            let status = args[1].trim().trim_matches('"');
            let has = unit.has_condition(status);
            Some(if has { "1" } else { "0" }.to_string())
        }
        "Money" => Some(app.money().to_string()),
        "Turn" => Some(app.turn().number.to_string()),
        "Phase" => Some(match app.turn().phase {
            crate::Phase::Player => "Player".to_string(),
            crate::Phase::Enemy => "Enemy".to_string(),
            crate::Phase::Neutral => "Neutral".to_string(),
            crate::Phase::Npc => "NPC".to_string(),
        }),
        "Stage" => Some(app.stage().to_string()),
        "TerrainId" => {
            // 範囲外 / 引数不正 / マップ未定義 はすべて "0" を返す
            // (literal `TerrainId(...)` を残さない)。SRC.Sharp 同等の
            // unit-query デフォルト挙動と揃える。
            if args.len() < 2 {
                return Some("0".to_string());
            }
            let Ok(x) = args[0].trim().parse::<u32>() else {
                return Some("0".to_string());
            };
            let Ok(y) = args[1].trim().parse::<u32>() else {
                return Some("0".to_string());
            };
            let Some(m) = app.database().map.as_ref() else {
                return Some("0".to_string());
            };
            if x >= m.width || y >= m.height {
                return Some("0".to_string());
            }
            Some(m.cell(x, y).terrain_id.to_string())
        }
        "Rank" => {
            // `Rank(unit)` — SRC ユニットランク (`RankUpコマンド` / `BossRankコマンド`
            // で `__rank_<unit>` に蓄積)。未設定なら 0。
            // 引数解決は fn_arg_value で行い、quoted/variable も受け付ける。
            let raw = fn_arg_value(app, args.first()?);
            let key = format!("__rank_{}", raw.trim().trim_matches('"'));
            let v = app.script_var(&key);
            Some(if v.is_empty() {
                "0".to_string()
            } else {
                v.to_string()
            })
        }
        "List" => {
            // `List(a, b, c)` → "a b c" 空白区切り。
            // 各引数は `fn_arg_value` で quote 剥がし / 変数解決を行い、
            // 単一の括弧式 (`(Info(マップ,高さ) - 3)` 等) は数値化する
            // (SRC.Sharp `List` は GetValueAsString 経由で式評価)。
            let parts: Vec<String> = args
                .iter()
                .map(|a| {
                    let v = fn_arg_value(app, a);
                    eval_paren_arith_value(app, &v).unwrap_or(v)
                })
                .collect();
            Some(parts.join(" "))
        }
        "Llength" => {
            // `Llength(s)` — 空白区切りトークン数。`s` は裸識別子なら変数解決。
            let s = fn_arg_value(app, args.first()?);
            Some(
                s.split_whitespace()
                    .filter(|w| !w.is_empty())
                    .count()
                    .to_string(),
            )
        }
        "Lindex" => {
            // `Lindex(s, n)` — 1-indexed 要素。SRC.Sharp 仕様:
            // - 範囲外 / index 0 / 負数 → 空文字
            // - 要素が `(...)` で囲まれていれば外側 paren を剥がす
            //   (`Functions/List.cs::LIndex` line 52-55)
            if args.len() < 2 {
                return None;
            }
            let s = fn_arg_value(app, args[0]);
            // signed parse で負数も拾う
            let n_signed: i64 = fn_arg_value(app, args[1]).parse().ok()?;
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
        "Lsearch" => {
            // `Lsearch(list, item [, start])` — list 内で item の出現位置
            // (1-indexed) を返す。見つからなければ **0**。
            // SRC.Sharp `Expressions/Functions/List.cs::LSearch` 同等
            // (line 78-106 で not found 時 0 を返す)。
            //
            // 第 3 引数 `start` で開始位置 (1-indexed) 指定可。デフォルト 1。
            if args.len() < 2 {
                return None;
            }
            let s = fn_arg_value(app, args[0]);
            let needle = fn_arg_value(app, args[1]);
            let start: usize = args
                .get(2)
                .and_then(|s| fn_arg_value(app, s).parse().ok())
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
        "Lsplit" => {
            // `Lsplit(s, sep)` — sep で s を split し、空白区切りリストに再構成。
            if args.len() < 2 {
                return None;
            }
            let s = fn_arg_value(app, args[0]);
            let sep = fn_arg_value(app, args[1]);
            let parts: Vec<&str> = s.split(&sep as &str).collect();
            Some(parts.join(" "))
        }
        "Lremove" => {
            // `Lremove(s, item)` — s から item を 1 件だけ除去したリストを返す。
            if args.len() < 2 {
                return None;
            }
            let s = fn_arg_value(app, args[0]);
            let item = fn_arg_value(app, args[1]);
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
        "Replace" => {
            // `Replace(s, find, replace[, start[, count]])` — 文字列置換
            // 4-arg: Replace(s, find, replace, start)
            //   → Left(s, start-1) + Right(s, Len-start+1).replace(find, replace)
            // 5-arg: Replace(s, find, replace, start, count)
            //   → Left(s, start-1) + Mid(s, start, count).replace(find, replace)
            //     + Right(s, Len - (start+count-1) - 1)
            if args.len() < 3 {
                return None;
            }
            let s = fn_arg_value(app, args[0]);
            let find = fn_arg_value(app, args[1]);
            let rep = fn_arg_value(app, args[2]);
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
                let start = (numeric_arg(app, args[3]).unwrap_or(1.0) as i64).max(1) as usize;
                let count = (numeric_arg(app, args[4]).unwrap_or(0.0) as i64).max(0) as usize;
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
                let start = (numeric_arg(app, args[3]).unwrap_or(1.0) as i64).max(1) as usize;
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
        "String" => {
            // `String(count, s)` — s を count 回繰り返した文字列。
            // VB5 では String(N, char) で char 単一文字だが、SRC は `String(3, "0 ")`
            // のように任意文字列も渡される (スパロボ戦記 で 6 回使用)。
            if args.len() < 2 {
                return None;
            }
            let n: usize = numeric_arg(app, args[0]).map(|v| v.max(0.0) as usize)?;
            let s = fn_arg_value(app, args[1]);
            Some(s.repeat(n))
        }
        "Wide" => {
            // `Wide(s)` — 半角文字を全角に変換。VB5 の StrConv(s, vbWide) 相当。
            // 半角 ASCII / 空白に加え、半角カタカナ (濁点・半濁点の合成含む) も
            // 全角カタカナへ変換する (SRC.NET `Expression.cs` の VbStrConv.Wide 準拠)。
            let s = fn_arg_value(app, args.first()?);
            Some(to_fullwidth(&s))
        }
        "LCase" => {
            // `LCase(s)` — ASCII 大文字を小文字に。日本語は不変。
            let s = fn_arg_value(app, args.first()?);
            Some(s.to_lowercase())
        }
        "UCase" => {
            // `UCase(s)` — ASCII 小文字を大文字に。日本語は不変。
            let s = fn_arg_value(app, args.first()?);
            Some(s.to_uppercase())
        }
        "Trim" => {
            // `Trim(s)` — 先頭末尾の半角空白を除去。内部空白は保持。
            // VB5 の Trim$ は全角空白 (U+3000) は除去しない。
            let s = fn_arg_value(app, args.first()?);
            Some(s.trim_matches(' ').to_string())
        }
        "Asc" => {
            // `Asc(s)` — VB5 互換: SJIS (CP932) 経由で文字コードを返す。
            // ASCII (0..=0x7F) は素直にそのバイト値、半角カナ等の単バイト SJIS は
            // そのバイト値、2 バイト SJIS は `high << 8 | low` (例: "あ" → 0x82A0)。
            // SJIS にエンコードできない文字は Unicode コードポイントに fallback。
            // SRC.Sharp は `Strings.Asc()` を使うので同じ値になる。
            let s = fn_arg_value(app, args.first()?);
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
        "Chr" => {
            // `Chr(n)` — VB5 互換: SJIS (CP932) コードを解釈して 1 文字を返す。
            // - 0..=0xFF: 単バイト SJIS (ASCII / 半角カナ / 制御コード)
            // - 0x100..: 2 バイト SJIS `high << 8 | low` として decode (例: 0x82A0 → "あ")
            // SRC.Sharp の Chr は `(char)long` で Unicode コードポイント扱い (XXX
            // 文字コード という FIXME 付き) だが、こちらは VB5 寄りで実装する。
            let n: u32 = numeric_arg(app, args.first()?).map(|v| v.max(0.0) as u32)?;
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
        "InStrRev" => {
            // `InStrRev(s1, s2 [, start])` — s1 内で s2 が最後に現れる位置 (1-indexed)。
            // start 省略時は -1 (末尾から検索)。見つからなければ 0。
            if args.len() < 2 {
                return None;
            }
            let s1 = fn_arg_value(app, args[0]);
            let s2 = fn_arg_value(app, args[1]);
            if s2.is_empty() {
                return Some("0".to_string());
            }
            let chars: Vec<char> = s1.chars().collect();
            let start: i64 = args
                .get(2)
                .and_then(|s| fn_arg_value(app, s).parse().ok())
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
        "Min" => {
            // `Min(a, b, ...)` — 1 引数以上をサポート。SRC.Sharp と同等。
            if args.is_empty() {
                return None;
            }
            let mut result = numeric_arg(app, args[0])?;
            for arg in &args[1..] {
                if let Some(v) = numeric_arg(app, arg) {
                    if v < result {
                        result = v;
                    }
                }
            }
            Some(format_num(result))
        }
        "Max" => {
            // `Max(a, b, ...)` — 1 引数以上をサポート。SRC.Sharp と同等。
            if args.is_empty() {
                return None;
            }
            let mut result = numeric_arg(app, args[0])?;
            for arg in &args[1..] {
                if let Some(v) = numeric_arg(app, arg) {
                    if v > result {
                        result = v;
                    }
                }
            }
            Some(format_num(result))
        }
        "Abs" => {
            let v = numeric_arg(app, args.first()?)?;
            Some(format_num(v.abs()))
        }
        "Int" => {
            // `Int(x)` — 数値以下の最大整数 (floor)。SRC 仕様で負数は -8.3 → -9。
            // 数値でない文字列は 0。
            let v = numeric_arg(app, args.first()?).unwrap_or(0.0);
            Some(format_num(v.floor()))
        }
        "Eval" => {
            // `Eval(expr)` — 式を評価。SRC では式の左辺としても使えるが
            // ここでは値式としてのみ実装。
            // 1) 数値式として評価できれば数値を返す (小数も保持)
            // 2) 単独識別子なら script_var を引く
            // 3) いずれにも該当しなければ生文字列をそのまま返す
            let s = fn_arg_value(app, args.first()?);
            if let Some(n) = try_eval_num(&s) {
                return Some(format_num(n));
            }
            let t = s.trim();
            let v = app.script_var(t);
            if !v.is_empty() {
                return Some(v.to_string());
            }
            Some(s)
        }
        "Round" => {
            // `Round(x [, digits])` — 四捨五入。digits=0 なら整数。
            // SRC.NET `Expression.cs::CallFunction "round"` 準拠で +∞ 方向への
            // 半数切り上げ (floor(scaled) 後、小数部 >= 0.5 なら +1)。
            // VB5 の銀行丸めとも Rust の round (ゼロから遠ざける) とも異なり、
            // 負数では +∞ 方向に丸める。例: Round(-2.5) = -2、Round(2.5) = 3。
            let v = numeric_arg(app, args.first()?)?;
            let d: i32 = args
                .get(1)
                .and_then(|s| numeric_arg(app, s))
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
        "RoundUp" => {
            // `RoundUp(x [, digits])` — 切り上げ。
            let v = numeric_arg(app, args.first()?)?;
            let d: i32 = args
                .get(1)
                .and_then(|s| numeric_arg(app, s))
                .map(|x| x as i32)
                .unwrap_or(0);
            let factor = 10f64.powi(d);
            Some(format_num((v * factor).ceil() / factor))
        }
        "RoundDown" => {
            // `RoundDown(x [, digits])` — 切り捨て。
            let v = numeric_arg(app, args.first()?)?;
            let d: i32 = args
                .get(1)
                .and_then(|s| numeric_arg(app, s))
                .map(|x| x as i32)
                .unwrap_or(0);
            let factor = 10f64.powi(d);
            Some(format_num((v * factor).floor() / factor))
        }
        "Sqr" => {
            let v = numeric_arg(app, args.first()?)?;
            Some(format_num(v.sqrt()))
        }
        "Sin" => {
            let v = numeric_arg(app, args.first()?)?;
            Some(format_num(v.sin()))
        }
        "Cos" => {
            let v = numeric_arg(app, args.first()?)?;
            Some(format_num(v.cos()))
        }
        "Tan" => {
            let v = numeric_arg(app, args.first()?)?;
            Some(format_num(v.tan()))
        }
        "Atn" => {
            let v = numeric_arg(app, args.first()?)?;
            Some(format_num(v.atan()))
        }
        "Log" => {
            // `Log(x)` — 自然対数 (VB5 `Log` 同等、SRC 準拠)。x <= 0 は None。
            let v = numeric_arg(app, args.first()?)?;
            if v <= 0.0 {
                return None;
            }
            Some(format_num(v.ln()))
        }
        "Sgn" => {
            // `Sgn(x)` — 符号関数: x > 0 → 1, x < 0 → -1, x == 0 → 0。
            // VB5 `Sgn` 同等 (`Math.Sign`)。
            let v = numeric_arg(app, args.first()?)?;
            Some(format_num(if v > 0.0 {
                1.0
            } else if v < 0.0 {
                -1.0
            } else {
                0.0
            }))
        }
        "Mod" => {
            // `Mod(a, b)` — 整数剰余。`a Mod b` 演算子も VB5/SRC で存在するが、
            // 関数形式は本実装で `Mod(a, b)` をサポートする (SRC.Sharp parity)。
            // 0 除算は 0 を返す (SRC は実行時エラーだが本実装は黙殺)。
            if args.len() < 2 {
                return None;
            }
            let a = numeric_arg(app, args[0])?;
            let b = numeric_arg(app, args[1])?;
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
        "Hex" => {
            // `Hex(n)` — 整数の 16 進表現 (大文字、prefix 無し)。
            let v = numeric_arg(app, args.first()?)?;
            let n = v as i64;
            Some(format!("{n:X}"))
        }
        "Oct" => {
            // `Oct(n)` — 整数の 8 進表現 (prefix 無し)。
            let v = numeric_arg(app, args.first()?)?;
            let n = v as i64;
            Some(format!("{n:o}"))
        }
        "Atan2" => {
            // `Atan2(y, x)` — 2 引数版 arctangent。象限を返す。
            if args.len() < 2 {
                return None;
            }
            let y = numeric_arg(app, args[0])?;
            let x = numeric_arg(app, args[1])?;
            Some(format_num(y.atan2(x)))
        }
        "Now" => {
            // SRC `Now` システム変数相当の関数アクセス: `YYYY/MM/DD HH:MM:SS`。
            // src-web が `Date::now()` で `App.wall_clock_ms` を更新する前提。
            // src-core 単独 (テスト) では `1970/01/01 00:00:00` 固定。
            Some(crate::time_util::format_now(app.wall_clock_ms()))
        }
        "GetTime" => {
            // SRC `GetTime()` — システム起動からのミリ秒。本実装はサンプル時刻
            // からの差分が分からないため、Unix epoch ミリ秒をそのまま返す。
            // SRC.NET `GeneralLib.timeGetTime()` の用途は主に乱数シード等で、
            // 単調増加していれば仕様充足する。
            Some(format_num(app.wall_clock_ms()))
        }
        "Year" | "Month" | "Day" | "Hour" | "Minute" | "Second" | "Weekday" => {
            // `Year([time_str])` 等。引数省略時は `Now()` の値を使う。
            // SRC.NET 仕様: 引数が IsDate で解釈できない場合は 0 を返す。
            let epoch_ms = if let Some(arg) = args.first() {
                let s = fn_arg_value(app, arg);
                if s.trim().is_empty() {
                    app.wall_clock_ms()
                } else {
                    match crate::time_util::parse_datetime(&s) {
                        Some(v) => v,
                        None => return Some("0".to_string()),
                    }
                }
            } else {
                app.wall_clock_ms()
            };
            let b = crate::time_util::breakdown(epoch_ms);
            Some(match name {
                "Year" => b.year.to_string(),
                "Month" => b.month.to_string(),
                "Day" => b.day.to_string(),
                "Hour" => b.hour.to_string(),
                "Minute" => b.minute.to_string(),
                "Second" => b.second.to_string(),
                "Weekday" => crate::time_util::weekday_name(b.weekday).to_string(),
                _ => unreachable!(),
            })
        }
        "DiffTime" => {
            // `DiffTime(t1, t2)` — t1 から t2 までの秒数 (t2 - t1)。
            // どちらも `Now` (=リテラル文字列) または datetime 文字列を許可。
            if args.len() < 2 {
                return None;
            }
            let resolve = |arg: &str| -> Option<f64> {
                let s = fn_arg_value(app, arg);
                let t = s.trim();
                if t.is_empty() || t.eq_ignore_ascii_case("now") {
                    return Some(app.wall_clock_ms());
                }
                crate::time_util::parse_datetime(t)
            };
            let t1 = resolve(args[0])?;
            let t2 = resolve(args[1])?;
            Some(format_num((t2 - t1) / 1000.0))
        }
        "IsVarDefined" => {
            // `IsVarDefined(name)` — 変数 / 配列要素が定義されていれば 1。
            // SRC.Sharp `Expression.IsVariableDefined` 同等: 空文字代入でも
            // **定義済 (1)** を返す (`Set var ""` 後の判定が 1)。
            //
            // 元 SRC は名称を文字通り (式評価せず) 使うが、本実装では
            // expand_vars 経由で既に解決済の文字列を script_var キーとして引く。
            // 数値リテラルが来た場合は (= 既に値で置換された) 「定義済み」と
            // 判定する。
            // 引数が無い / 空 (`IsVarDefined()`) は「未定義」= 0。
            // `args.first()` が None のときも literal 落ちさせず 0 を返す。
            let raw_orig = args.first().map_or("", |s| s.trim());
            // SRC.Sharp 準拠: `$` プレフィックスは除去してから参照する。
            let raw = raw_orig.strip_prefix('$').unwrap_or(raw_orig);
            if raw.is_empty() {
                return Some("0".to_string());
            }
            if raw.parse::<f64>().is_ok() {
                return Some("1".to_string());
            }
            // 直接 / 解決済の indexed key — `contains_key` で空代入も拾う
            if app.is_script_var_defined(raw) {
                return Some("1".to_string());
            }
            let resolved = resolve_lhs_name(app, raw);
            Some(
                if app.is_script_var_defined(&resolved) {
                    "1"
                } else {
                    "0"
                }
                .to_string(),
            )
        }
        "IsNumeric" => {
            let s = fn_arg_value(app, args.first()?);
            // SRC.Sharp 互換: .NET `decimal.TryParse` 準拠。
            // 科学的表記法 (1e5, 1E10) および NaN / Infinity は数値扱いしない。
            let t = s.trim();
            let is_num = !t.is_empty()
                && !t.as_bytes().iter().any(|&b| b == b'e' || b == b'E')
                && t.parse::<f64>().is_ok_and(|v| v.is_finite());
            Some(if is_num { "1" } else { "0" }.to_string())
        }
        "Nickname" => {
            // `Nickname(name)` — パイロット / ユニット / アイテムの愛称。
            // 同名の場合はパイロット優先。
            //
            // SRC 原典 (Expression `case "nickname"`) はアイテム形式
            // `Nickname(アイテム名, アイテム番号)` も受け付け、第 2 引数の番号で
            // 同名アイテムを識別したうえで `ItemData.Nickname` を返す。本移植の
            // `ItemData` は愛称フィールドを持たないため、第 1 引数からアイテムを
            // 解決し、その名称 (愛称代替) を返す。第 2 引数 (番号) は名称解決で
            // 一意に引けるため無視する。
            let n = fn_arg_value(app, args.first()?);
            let n = n.trim().trim_matches('"');
            if let Some(p) = app
                .database()
                .pilots
                .iter()
                .find(|p| p.name == n || p.nickname == n)
            {
                return Some(p.nickname.clone());
            }
            if let Some(u) = app.database().unit_by_name(n) {
                return Some(u.nickname.clone());
            }
            if let Some(it) = app.database().item_by_name(n) {
                // ItemData に愛称フィールドが無いため名称を愛称として返す。
                return Some(it.name.clone());
            }
            // ユニットインスタンス参照
            if let Some(inst) = find_unit(app, n) {
                if let Some(u) = app.database().unit_by_name(&inst.unit_data_name) {
                    return Some(u.nickname.clone());
                }
            }
            Some(n.to_string())
        }
        "Format" => {
            // `Format(value, "fmt")` — 簡易実装: 数値ならそのまま、"#,##0" 形式は
            // 桁区切りカンマを付与、"0.00" 形式は小数桁固定、`%` は 100 倍 + %付与。
            // それ以外の VB5 format 構文 ($, E+ 等) はサポートしない。
            let v_str = fn_arg_value(app, args.first()?);
            let fmt = args
                .get(1)
                .map(|s| fn_arg_value(app, s))
                .unwrap_or_default();
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
        "InStr" => {
            // `InStr(s1, s2 [, start])` — s1 内で s2 が始まる位置 (1-indexed)。
            // 見つからなければ 0。
            if args.len() < 2 {
                return None;
            }
            let s1 = fn_arg_value(app, args[0]);
            let s2 = fn_arg_value(app, args[1]);
            let start: usize = args
                .get(2)
                .and_then(|s| fn_arg_value(app, s).parse().ok())
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
        "Term" => {
            // `Term(用語[, unitId])` — RenameTerm で設定された用語別名を引く。
            // 別名未設定なら用語名そのものを返す (SRC.Sharp 準拠)。
            // `unitId` は「用語名」特殊能力オーバーライド用だが本実装は無視。
            let term = fn_arg_value(app, args.first()?);
            let key = format!("__term_{term}");
            let alias = app.script_var(&key).to_string();
            if alias.is_empty() {
                Some(term)
            } else {
                Some(alias)
            }
        }
        "TextWidth" => {
            // `TextWidth(text)` — 描画幅 (ピクセル) の近似値。フォントサイズが
            // ランタイムに保持されていないため、char_count × 14 (14pt 等幅相当) で
            // 概算する。
            let s = fn_arg_value(app, args.first()?);
            let s = s.trim_matches('"');
            Some((s.chars().count() as i32 * 14).to_string())
        }
        "TextHeight" => {
            // `TextHeight(text)` — 1 行高さ。フォントサイズ 14pt 想定で 20 を返す。
            Some("20".to_string())
        }
        "KeyState" => {
            // `KeyState(2)` — 右マウスボタン。SRC `Wait Click` の「右ボタン = キャンセル」
            // を実現するため、直近の Wait Click を右クリックで解除していれば 1 を返す。
            // これでステータス画面の `Case "" → If KeyState(2) Then Break` が機能する。
            // 重要: 右クリックでない場合は **下のビジーループ脱出ロジックへフォール
            // スルー** する (early-return しない)。`Do While (KeyState(2)=0)` 形式の
            // ビジーループが脱出できず STEP_LIMIT までフリーズするのを防ぐため。
            if let Some(a) = args.first() {
                if fn_arg_value(app, a).trim() == "2" && app.take_wait_click_right() {
                    // ワンショット消費: 直後の `Do While KeyState(2) Loop` (解放待ち) は
                    // 0 を見て即脱出する。これをしないと無限ループで STEP_LIMIT に達する。
                    return Some("1".to_string());
                }
            }
            // `KeyState(n)` — 押されているキー番号を判定。Web 移植ではリアルタイムの
            // キー押下状態を取得できないため、通常は 0 を返す。
            //
            // ただし `Do While (KeyState()=0) ... Loop` パターン（ユーザ入力待ちの
            // ビジーループ）が STEP_LIMIT まで走り続けてブラウザをフリーズさせるのを
            // 防ぐため、同一スクリプト実行内で一定回数呼ばれた後は "1"（押された）を
            // 返してループを強制終了させる。
            // しきい値 4: Do While (KeyState=0) ループの初回評価は 3 回以内に
            // KeyState を呼ぶことが多く、4 回目以降で "1" を返すとループが脱出する。
            const KEYSTATE_AUTO_BREAK_THRESHOLD: usize = 4;
            let count = app.increment_keystate_call_count();
            if count >= KEYSTATE_AUTO_BREAK_THRESHOLD {
                Some("1".to_string())
            } else {
                Some("0".to_string())
            }
        }
        "PlayingMidi" | "PlayingSound" => Some("0".to_string()),
        "WindowWidth" => {
            // メインウィンドウの横幅 (ピクセル)。WASM 環境では 800 を返す。
            Some("800".to_string())
        }
        "WindowHeight" => {
            // メインウィンドウの縦幅 (ピクセル)。WASM 環境では 600 を返す。
            Some("600".to_string())
        }
        "Font" => {
            // `Font(属性名)` — 現在のフォント設定を返す。
            // WASM 環境ではフォントをここから取得できないため固定値を返す。
            let attr = fn_arg_value(app, args.first()?);
            match attr.trim().trim_matches('"') {
                "フォント名" => Some("MS ゴシック".to_string()),
                "サイズ" => Some("12".to_string()),
                "太字" => Some("0".to_string()),
                "斜体" => Some("0".to_string()),
                "色" => Some("#000000".to_string()),
                "書き込み" => Some("通常".to_string()),
                _ => Some("".to_string()),
            }
        }
        "PointX" | "BaseX" => {
            // 描画カーソル X 座標 (`Line` / `PSet` / `PaintString` の終端座標)。
            // SRC.NET `Event.BaseX` (`picMain.CurrentX`) 同等。
            Some(format_num(app.script_overlay().cursor_x))
        }
        "PointY" | "BaseY" => Some(format_num(app.script_overlay().cursor_y)),
        "Len" => {
            // 文字列長 (UTF-8 文字数)
            let s = fn_arg_value(app, args.first()?);
            Some(s.chars().count().to_string())
        }
        "LenB" => {
            // LenB(s) — Shift-JIS エンコードでのバイト数 (ASCII=1, 非ASCII=2)
            let s = fn_arg_value(app, args.first()?);
            Some(sjis_byte_len(&s).to_string())
        }
        "LeftB" => {
            // LeftB(s, byteCount) — 先頭 n バイト (Shift-JIS 換算)
            let s = fn_arg_value(app, args.first()?);
            let n = numeric_arg(app, args.get(1)?).unwrap_or(0.0) as usize;
            Some(sjis_left(&s, n))
        }
        "RightB" => {
            // RightB(s, byteCount) — 末尾 n バイト (Shift-JIS 換算)
            let s = fn_arg_value(app, args.first()?);
            let n = numeric_arg(app, args.get(1)?).unwrap_or(0.0) as usize;
            Some(sjis_right(&s, n))
        }
        "MidB" => {
            // MidB(s, start[, length]) — バイト位置から substring (1-indexed, Shift-JIS 換算)
            let s = fn_arg_value(app, args.first()?);
            let start = (numeric_arg(app, args.get(1)?).unwrap_or(1.0) as i64).max(1) as usize;
            if args.len() >= 3 {
                let length = numeric_arg(app, args.get(2)?).unwrap_or(0.0) as usize;
                Some(sjis_mid(&s, start, Some(length)))
            } else {
                Some(sjis_mid(&s, start, None))
            }
        }
        "InStrB" => {
            // InStrB(s, pattern[, start]) — バイト位置で最初の出現を探す (Shift-JIS 換算, 1-indexed)
            let s = fn_arg_value(app, args.first()?);
            let pattern = fn_arg_value(app, args.get(1)?);
            let start_byte = if args.len() >= 3 {
                (numeric_arg(app, args.get(2)?).unwrap_or(1.0) as i64).max(1) as usize
            } else {
                1
            };
            Some(sjis_instr(&s, &pattern, start_byte).to_string())
        }
        "InStrRevB" => {
            // InStrRevB(s, pattern[, start]) — バイト位置で最後の出現を探す (Shift-JIS 換算)
            let s = fn_arg_value(app, args.first()?);
            let pattern = fn_arg_value(app, args.get(1)?);
            let end_byte = if args.len() >= 3 {
                Some((numeric_arg(app, args.get(2)?).unwrap_or(0.0) as i64).max(1) as usize)
            } else {
                None
            };
            Some(sjis_instrrev(&s, &pattern, end_byte).to_string())
        }
        "Dir" => {
            // `Dir(path, kind)` — 仮想ファイルシステムに該当ファイルが
            // あれば basename を返す ("見つかった")。無ければ空文字列。
            // 実ファイルシステムへのアクセスは行わない。
            let path = args
                .first()
                .map(|p| fn_arg_value(app, p))
                .unwrap_or_default();
            Some(
                app.virtual_file_basename_if_exists(&path)
                    .unwrap_or_default(),
            )
        }
        "FileExists" => {
            // `FileExists(path)` — VFS に該当ファイルが存在するなら 1、無ければ 0。
            let path = args
                .first()
                .map(|p| fn_arg_value(app, p))
                .unwrap_or_default();
            Some(
                if app.virtual_file_exists(&path) {
                    "1"
                } else {
                    "0"
                }
                .to_string(),
            )
        }
        "FolderExists" => {
            // `FolderExists(path)` — 本実装は VFS にフォルダの概念がない
            // ため常に 0。SRC.Sharp との乖離だが、シナリオの大半は使わない。
            Some("0".to_string())
        }
        "FileLen" => {
            // `FileLen(path)` — VFS 上のファイルサイズ (UTF-8 バイト数)。
            // 未登録なら 0。
            let path = args
                .first()
                .map(|p| fn_arg_value(app, p))
                .unwrap_or_default();
            Some(app.virtual_file_len(&path).to_string())
        }
        "Loc" => {
            // `Loc(handle)` — ファイルハンドルの読取カーソル (行番号)。
            let handle = args
                .first()
                .map(|p| fn_arg_value(app, p))
                .unwrap_or_default();
            Some(app.vfs_loc(&handle).to_string())
        }
        "EOF" => {
            // `EOF(handle)` — 末尾に到達していれば 1、まだ読めるなら 0。
            let handle = args
                .first()
                .map(|p| fn_arg_value(app, p))
                .unwrap_or_default();
            Some(if app.vfs_eof(&handle) { "1" } else { "0" }.to_string())
        }
        "LOF" => {
            // `LOF(handle)` — 開いているファイルの総行数。
            let handle = args
                .first()
                .map(|p| fn_arg_value(app, p))
                .unwrap_or_default();
            Some(app.vfs_lof(&handle).to_string())
        }
        "RGB" | "Rgb" => {
            // `RGB(r, g, b)` → "#rrggbb" 形式の16進カラー文字列 (SRC.Sharp 準拠)
            // 各引数は変数・算術式を含む可能性があるため numeric_arg で評価する。
            if args.len() < 3 {
                return None;
            }
            let r = numeric_arg(app, args[0]).unwrap_or(0.0) as i32;
            let g = numeric_arg(app, args[1]).unwrap_or(0.0) as i32;
            let b = numeric_arg(app, args[2]).unwrap_or(0.0) as i32;
            let r = r.clamp(0, 255) as u8;
            let g = g.clamp(0, 255) as u8;
            let b = b.clamp(0, 255) as u8;
            Some(format!("#{r:02x}{g:02x}{b:02x}"))
        }
        "RegExp" | "RegExpMatch" => {
            // SRC `RegExp(string, pattern [, 大小区別あり|大小区別なし])` (`正規表現関数.md`):
            // マッチした文字列を返す。マッチしなければ空文字列。
            // デフォルトは「大小区別あり」(case-sensitive)。
            // 元 SRC は VBScript.RegExp (Perl 互換に近い)。本実装は Rust `regex`
            // クレートで近似。Perl の `\d` `\w` 等は同等、ECMAScript の
            // 後方参照等は非対応 (regex crate の制約)。
            if args.len() < 2 {
                return None;
            }
            let s = fn_arg_value(app, args[0]);
            let pat = fn_arg_value(app, args[1]);
            let case_insensitive = matches!(
                args.get(2).map(|a| fn_arg_value(app, a)).as_deref(),
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
        "RegExpReplace" => {
            // SRC `RegExpReplace(string, searchPattern, replacePattern [, 大小区別あり|大小区別なし])`:
            // 置換結果を返す。デフォルトは「大小区別あり」(case-sensitive)。
            if args.len() < 3 {
                return None;
            }
            let s = fn_arg_value(app, args[0]);
            let search = fn_arg_value(app, args[1]);
            let replace = fn_arg_value(app, args[2]);
            let case_insensitive = matches!(
                args.get(3).map(|a| fn_arg_value(app, a)).as_deref(),
                Some("大小区別なし") | Some("CaseInsensitive") | Some("Insensitive")
            );
            let result = regex::RegexBuilder::new(&search)
                .case_insensitive(case_insensitive)
                .build()
                .map(|re| re.replace_all(&s, replace.as_str()).into_owned())
                .unwrap_or_else(|_| s.clone());
            Some(result)
        }
        "Partner" => {
            // `Partner(num)` — 合体技の num 番目のパートナー Unit ID。
            // num=0 は使用ユニット自身、num>=1 は合体技パートナー。
            // 本実装は合体技を完全にはモデル化していないため、合体技履歴を
            // `直前合体技パートナー[N]` script_var に格納する規約とする。
            // 未設定なら空文字 (SRC.NET 仕様)。
            let n: i32 = fn_arg_value(app, args.first()?).parse().unwrap_or(0);
            if n == 0 {
                return Some(app.script_var("直前合体技ユニット").to_string());
            }
            let key = format!("直前合体技パートナー[{n}]");
            Some(app.script_var(&key).to_string())
        }
        "CountPartner" => {
            // `CountPartner()` — 直前戦闘の合体技パートナー数。`直前合体技パートナー[*]`
            // の要素数を返す。未使用 (引数があってもなくても) で 0。
            let prefix = "直前合体技パートナー[";
            let n = app
                .script_vars()
                .keys()
                .filter(|k| k.starts_with(prefix) && k.ends_with(']'))
                .count();
            Some(n.to_string())
        }
        "IsEquiped" | "IsEquipped" => {
            // `IsEquiped(unit, item)` — 指定アイテム装備中なら 1、未装備なら 0。
            // 既存の `HasItem` と同様だが、IsEquiped は SRC.NET 正典名。
            if args.len() < 2 {
                return None;
            }
            let unit = find_unit(app, args[0])?;
            let item = fn_arg_value(app, args[1]);
            let item = item.trim().trim_matches('"');
            let has = unit.has_item_equipped(item);
            Some(if has { "1" } else { "0" }.to_string())
        }
        "SpecialPower" | "Mind" => {
            // `SpecialPower(unit, sp_name)` / `Mind(unit, sp_name)` — unit が指定 SP の影響下にあれば 1。
            // 本実装は SP buff を `conditions` テーブルに格納しているため、
            // `has_condition(sp_name)` で判定 (`HasStatus` / `Condition` と同等)。
            // SRC.NET の `Mind` 関数 (旧名) と同じ意味論。
            if args.len() < 2 {
                return None;
            }
            let unit = find_unit(app, args[0])?;
            let sp = fn_arg_value(app, args[1]);
            let sp = sp.trim().trim_matches('"');
            let has = unit.has_condition(sp);
            Some(if has { "1" } else { "0" }.to_string())
        }
        "CountItem" => {
            // `CountItem(unit)` — unit が装備しているアイテム総数。
            // 未解決 unit は 0。SRC.Sharp `Unit/Unit.cs::CountItem` 同等。
            // 「未装備」指定で未装備在庫 (`spare_items`) の総数を返す。
            let key = fn_arg_value(app, args.first()?);
            if key == "未装備" {
                return Some(app.database().spare_items.len().to_string());
            }
            let n = find_unit(app, &key)
                .map(|u| {
                    u.item_slots
                        .iter()
                        .filter(|s| s.equipped_item.is_some())
                        .count()
                })
                .unwrap_or(0);
            Some(n.to_string())
        }
        "WX" | "WY" => {
            // `WX(unit_or_x)` / `WY(unit_or_y)` — マップウィンドウ上の表示座標。
            // 1 タイル = 32 px (本実装の `render.rs` 基準。SRC.NET は 24~32 可変
            // だが、現フロントエンドの規約 32 px に揃える)。
            // 引数が数値ならその数値 × 32、ユニット指定ならユニット位置 × 32。
            // SRC 仕様の「画面表示左上隅」は本実装で「タイル左上」と一致する。
            const TILE_PX: i32 = 32;
            let arg = fn_arg_value(app, args.first()?);
            let arg = arg.trim();
            // 数値解釈優先
            if let Ok(n) = arg.parse::<i32>() {
                return Some((n * TILE_PX).to_string());
            }
            // ユニット参照
            if let Some(u) = find_unit(app, arg) {
                let coord = if name == "WX" { u.x as i32 } else { u.y as i32 };
                return Some((coord * TILE_PX).to_string());
            }
            Some("0".to_string())
        }
        "IIF" | "Iif" => {
            // `IIF(cond, a, b)` — 条件式が真なら a、偽なら b。
            // SRC.Sharp 準拠: cond は比較式 (`5 > 3`, `x = 1` 等) を含む完全な
            // 条件式として評価する。`eval_inline_condition` を使用。
            if args.len() < 3 {
                return None;
            }
            let cond = eval_inline_condition(app, args[0]);
            let chosen = if cond { args[1] } else { args[2] };
            Some(fn_arg_value(app, chosen))
        }
        "StrCmp" | "StrCompare" => {
            // `StrCmp(a, b)` → 0 (一致) / -1 (a < b) / 1 (a > b)
            if args.len() < 2 {
                return None;
            }
            let a = fn_arg_value(app, args[0]);
            let b = fn_arg_value(app, args[1]);
            Some(
                match a.cmp(&b) {
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Greater => 1,
                }
                .to_string(),
            )
        }
        "Left" => {
            // `Left(s, n)` — 先頭 n 文字。`n` は `(te - 1)` のような
            // 変数入り算術式でありうるので app-aware に評価する。
            if args.len() < 2 {
                return None;
            }
            let s = fn_arg_value(app, args[0]);
            let n = eval_int_expr_app(app, args[1]).max(0) as usize;
            Some(s.chars().take(n).collect())
        }
        "Right" => {
            // `Right(s, n)` — 末尾 n 文字
            if args.len() < 2 {
                return None;
            }
            let s = fn_arg_value(app, args[0]);
            let n = eval_int_expr_app(app, args[1]).max(0) as usize;
            let count = s.chars().count();
            let skip = count.saturating_sub(n);
            Some(s.chars().skip(skip).collect())
        }
        "LSet" => {
            // `LSet(str, width)` — 左寄せ、右側を空白で `width` までパディング
            // (`width` より長い場合はそのまま返す)。SRC.Sharp
            // `Functions/String.cs::LSet` 同等。
            // 幅は `str_display_width` (全角=2, 半角=1) で計算する。
            // 第1引数は "(式)" 形式の数値式になりうるため numeric_arg で評価を試みる。
            // 例: `LSet((改造段階[ＨＰ] * 費用 + 費用), 6)` → `"3000  "`
            if args.len() < 2 {
                return None;
            }
            let s = if let Some(n) = numeric_arg(app, args[0]) {
                format!("{}", n as i64)
            } else {
                fn_arg_value(app, args[0])
            };
            let w: usize = numeric_arg(app, args[1])? as usize;
            let dw = str_display_width(&s);
            if dw >= w {
                Some(s)
            } else {
                Some(format!("{s}{}", " ".repeat(w - dw)))
            }
        }
        "RSet" => {
            // `RSet(str, width)` — 右寄せ、左側を空白でパディング。
            // 幅は `str_display_width` (全角=2, 半角=1) で計算する。
            // 第1引数は "(式)" 形式の数値式になりうるため numeric_arg で評価を試みる。
            // 例: `RSet((改造段階[ＨＰ] * 費用 + 費用), 6)` → `"  3000"`
            if args.len() < 2 {
                return None;
            }
            let s = if let Some(n) = numeric_arg(app, args[0]) {
                format!("{}", n as i64)
            } else {
                fn_arg_value(app, args[0])
            };
            let w: usize = numeric_arg(app, args[1])? as usize;
            let dw = str_display_width(&s);
            if dw >= w {
                Some(s)
            } else {
                Some(format!("{}{s}", " ".repeat(w - dw)))
            }
        }
        "Mid" => {
            // `Mid(s, start, [len])` — 1-indexed 部分文字列。
            // `start` / `len` は変数入り算術式でありうるので app-aware に評価。
            if args.len() < 2 {
                return None;
            }
            let s = fn_arg_value(app, args[0]);
            let start = eval_int_expr_app(app, args[1]).max(0) as usize;
            let skip = start.saturating_sub(1);
            if let Some(len_arg) = args.get(2) {
                let n = eval_int_expr_app(app, len_arg).max(0) as usize;
                Some(s.chars().skip(skip).take(n).collect())
            } else {
                Some(s.chars().skip(skip).collect())
            }
        }
        "Info" => {
            // 元 SRC の Info(): ユニット / パイロット / アイテム / マップ等の
            // 動的データ照会。`info_query` ヘルパに委譲。
            Some(info_query(app, &args))
        }
        "UnitID" => {
            // `UnitID(name)` — SRC.Sharp `Unit/Unit.cs::UnitID` 同等:
            // unit を解決して unit の一意 ID を返す。uid が未設定なら
            // unit_data_name を返す。
            let key = fn_arg_value(app, args.first()?);
            if let Some(inst) = find_unit(app, &key) {
                if !inst.uid.is_empty() {
                    return Some(inst.uid.clone());
                }
                return Some(inst.unit_data_name.clone());
            }
            // 未解決の場合は入力をそのまま返す (legacy 互換)
            Some(key)
        }
        "CountPilot" => {
            // `CountPilot(unit)` — 指定ユニットに搭乗中のパイロット数。
            // 本実装は単一パイロットのみモデル化しているので、パイロットが
            // 居れば 1、未搭乗 (pilot_name 空 / "パイロット不在") なら 0。
            let key = fn_arg_value(app, args.first()?);
            let n = find_unit(app, &key)
                .map(|inst| {
                    let p = inst.pilot_name.trim();
                    i32::from(!p.is_empty() && p != "パイロット不在")
                })
                .unwrap_or(0);
            Some(n.to_string())
        }
        "PilotID" => {
            // `PilotID(unit_name [, index])` — SRC.Sharp `Unit/Unit.cs::PilotID`
            // 同等: 指定 unit の (index 番目の) pilot 名を返す。
            // 本実装は同名 pilot を区別する uid を持たないので pilot 名そのもの。
            // index = 0 / 省略時は main pilot (= UnitInstance.pilot_name)。
            let key = fn_arg_value(app, args.first()?);
            if let Some(inst) = find_unit(app, &key) {
                // index 1 以上は SRC では副パイロットだが、本実装は単一
                // pilot のみ。
                return Some(inst.pilot_name.clone());
            }
            // 未解決 → input そのまま返す
            Some(key)
        }
        "Pilot" => {
            // `Pilot(unit [, N])` — 当該ユニットに乗っているパイロットの N 番目
            // (1-indexed, 省略時は 1) のパイロット名。未解決 unit は空文字。
            // 引数は裸の識別子 (`Pilot(選択)` 等) も `fn_arg_value` で
            // script_var 解決する (`UnitID` / `PilotID` と同じ規約)。
            let unit_key = fn_arg_value(app, args.first()?);
            let Some(inst) = find_unit(app, &unit_key) else {
                return Some(String::new());
            };
            let idx: usize = args
                .get(1)
                .and_then(|s| fn_arg_value(app, s).parse().ok())
                .unwrap_or(1);
            if idx <= 1 {
                Some(inst.pilot_name.clone())
            } else {
                Some(String::new())
            }
        }
        "Item" => {
            // `未装備` 指定で未装備在庫の num 番目のアイテム名を返す。
            let key = fn_arg_value(app, args.first()?);
            let idx: usize = fn_arg_value(app, args.get(1).unwrap_or(&""))
                .parse()
                .unwrap_or(0);
            if key == "未装備" {
                let spare = &app.database().spare_items;
                return Some(spare.get(idx.wrapping_sub(1)).cloned().unwrap_or_default());
            }
            let Some(inst) = find_unit(app, &key) else {
                return Some(String::new());
            };
            let names = inst.equipped_item_names();
            if idx == 0 || idx > names.len() {
                Some(String::new())
            } else {
                Some(names[idx - 1].to_string())
            }
        }
        "ItemID" => {
            // `ItemID(unit, num)` — SRC `ユニット情報関数.md`:
            // ユニットが装備している num 番目のアイテムの ID を返す。
            // 本実装はアイテム固有 ID を持たないので名称をそのまま返す
            // (SRC の実装と等価な挙動: 同名アイテムを区別しない)。
            // `未装備` を指定すると未装備在庫の num 番目を返す。
            let key = fn_arg_value(app, args.first()?);
            let idx: usize = fn_arg_value(app, args.get(1).unwrap_or(&""))
                .parse()
                .unwrap_or(0);
            if key == "未装備" {
                let spare = &app.database().spare_items;
                return Some(spare.get(idx.wrapping_sub(1)).cloned().unwrap_or_default());
            }
            let Some(inst) = find_unit(app, &key) else {
                return Some(String::new());
            };
            let names = inst.equipped_item_names();
            if idx == 0 || idx > names.len() {
                Some(String::new())
            } else {
                // 本実装では item_slot に個別 ID がないのでアイテム名を ID として返す
                Some(names[idx - 1].to_string())
            }
        }
        // SRC 原名: `アイテム番号` / `真アイテム番号` / `入手アイテム番号`
        // (SRC.Sharp `Expression` の同系 case 群)。
        // 引数のアイテム名 (または `Item(...)` 等のネスト式) を解決し、アイテム
        // データベース上の 1-indexed 連番を返す。引数が既に数値ならそのまま返す
        // (SRC の numeric passthrough 挙動)。本移植では真化前後・入手済みを区別する
        // 別テーブルを持たないため、3 系統とも同一のアイテム DB (`database().items`)
        // を引く。未登録名なら "0" を返す。
        "ItemIndex" => {
            let name = fn_arg_value(app, args.first()?);
            let name = name.trim().trim_matches('"');
            // numeric passthrough: 既に数値ならそのまま返す。
            if !name.is_empty() && name.parse::<i64>().is_ok() {
                return Some(name.to_string());
            }
            let idx = app
                .database()
                .items
                .iter()
                .position(|it| it.name == name)
                .map(|p| p + 1)
                .unwrap_or(0);
            Some(idx.to_string())
        }
        "Unit" => {
            // `Unit(pilot_name_or_unit_id)` — SRC `ユニット情報関数.md`:
            // 指定パイロット (またはユニット ID) が乗っているユニットの名称を返す。
            // 未解決の場合は空文字。
            let key = fn_arg_value(app, args.first()?);
            let Some(inst) = find_unit(app, &key) else {
                return Some(String::new());
            };
            Some(inst.unit_data_name.clone())
        }
        "Not" => {
            // `Not(cond)` 関数形式 (SRC のヘルパ)
            let v = args.first()?.trim();
            Some(if v.is_empty() || v == "0" {
                "1".to_string()
            } else {
                "0".to_string()
            })
        }
        "Party" => {
            // `Party(unit_or_pilot)` — 当該ユニット (パイロット名 / ユニット名 /
            // 変数経由) の所属勢力ラベル ("味方"/"友軍"/"敵"/"中立") を返す。
            // 未解決 unit は空文字。
            let key = args.first()?;
            let Some(inst) = find_unit(app, key) else {
                return Some(String::new());
            };
            Some(
                match inst.party {
                    crate::Party::Player => "味方",
                    crate::Party::Enemy => "敵",
                    crate::Party::Neutral => "中立",
                    crate::Party::Npc => "ＮＰＣ",
                }
                .to_string(),
            )
        }
        "Morale" => {
            let key = fn_arg_value(app, args.first().copied().unwrap_or(""));
            let key = key.trim().trim_matches('"');
            // ユニット配置済み → UnitInstance.morale を返す。
            if let Some(u) = find_unit(app, key) {
                return Some(u.morale.to_string());
            }
            // ユニット未配置でも PilotInstance があればそちらを返す
            // (SRC.Sharp 準拠: Pilot.Morale は配置有無に関わらず参照可能)。
            if let Some(pi) = app
                .database()
                .pilot_instances
                .iter()
                .find(|p| p.pilot_data_name == key || p.id == key)
            {
                return Some(pi.morale.to_string());
            }
            Some("0".to_string())
        }
        "Exp" => Some(
            find_unit(app, args.first().copied().unwrap_or(""))
                .map(|u| u.total_exp.to_string())
                .unwrap_or_else(|| "0".to_string()),
        ),
        "Level" => {
            // `Level(pilot)` — SRC.Sharp Pilot.Level 相当。
            // 優先度: UnitInstance → PilotInstance → PilotData (level 1) → 0
            // SRC.Sharp: `Pilot.Level` は `PList.Add(name, level, ...)` で設定され
            // `LevelUp` で増加する。Rust では `PilotInstance.level` に対応。
            let key = fn_arg_value(app, args.first()?);
            let key = key.trim().trim_matches('"');
            // ① ユニット配置済み → UnitInstance 経由 (total_exp から level を計算)
            if let Some(inst) = app
                .database()
                .unit_instances
                .iter()
                .find(|u| u.pilot_name == key || matches_unit_handle(u, key))
            {
                // C# LevelUpCmd.cs: レベル上限は 99 (level_from_exp が担保)。
                let level = crate::pilot_instance::level_from_exp(inst.total_exp);
                return Some(level.to_string());
            }
            // ② PilotInstance があればその level を返す (ユニット未配置)
            if let Some(pi) = app
                .database()
                .pilot_instances
                .iter()
                .find(|p| p.pilot_data_name == key || p.id == key)
            {
                return Some(pi.level.to_string());
            }
            // ③ PilotData のみ定義 → level 1
            if app.database().pilot_by_name(key).is_some() {
                return Some("1".to_string());
            }
            Some("0".to_string())
        }
        "SP" => {
            // `SP(pilot_or_unit)` — 現在 SP。PilotData.sp - UnitInstance.sp_consumed。
            // 未解決は 0。
            let key = fn_arg_value(app, args.first()?);
            let key = key.trim().trim_matches('"');
            if let Some(inst) = app
                .database()
                .unit_instances
                .iter()
                .find(|u| u.pilot_name == key || matches_unit_handle(u, key))
            {
                let max_sp = app
                    .database()
                    .pilot_by_name(&inst.pilot_name)
                    .and_then(|p| p.sp)
                    .unwrap_or(0);
                let remaining = (max_sp - inst.sp_consumed).max(0);
                return Some(remaining.to_string());
            }
            // ユニット未配置だがパイロット定義があれば max_sp をそのまま返す
            if let Some(p) = app.database().pilot_by_name(key) {
                return Some(p.sp.unwrap_or(0).to_string());
            }
            Some("0".to_string())
        }
        "Plana" => {
            // `Plana(pilot)` — パイロットの霊力値。`Plana(pilot) = n` で設定され、
            // `UnitInstance.plana` に格納される。
            // ユニット未配置の場合は PilotInstance にフォールバック。
            let key = fn_arg_value(app, args.first()?);
            let key = key.trim().trim_matches('"');
            // ① UnitInstance から引く
            if let Some(u) = find_unit(app, key) {
                return Some(u.plana.to_string());
            }
            // ② PilotInstance にフォールバック (ユニット未配置)
            if let Some(pi) = app
                .database()
                .pilot_instances
                .iter()
                .find(|p| p.pilot_data_name == key || p.id == key)
            {
                return Some(pi.plana.to_string());
            }
            Some("0".to_string())
        }
        "Relation" => {
            // `Relation(pilot_a, pilot_b)` — 2 パイロット間の好感度。
            // `SetRelation` で `__rel_<a>_<b>` に保存された値を読み出す。
            // 未設定なら 0。
            if args.len() < 2 {
                return Some("0".to_string());
            }
            let a = fn_arg_value(app, args[0]);
            let b = fn_arg_value(app, args[1]);
            let key = format!("__rel_{a}_{b}");
            let v = app.script_var(&key);
            Some(if v.is_empty() {
                "0".to_string()
            } else {
                v.to_string()
            })
        }
        "X" => Some(
            find_unit(app, args.first().copied().unwrap_or(""))
                .map(|u| u.x.to_string())
                .unwrap_or_else(|| "0".to_string()),
        ),
        "Y" => Some(
            find_unit(app, args.first().copied().unwrap_or(""))
                .map(|u| u.y.to_string())
                .unwrap_or_else(|| "0".to_string()),
        ),
        "Area" => {
            // `Area(unit)` — SRC 仕様: "地上"/"空中"/"水上"/"水中"/"宇宙" を返す。
            // SRC.Sharp `Expressions/Functions/Unit/Unit.cs::Area` 同等。
            // 1) UnitInstance.current_area が空でなければ優先 (`.eve` コマンドで明示設定)。
            // 2) それ以外は unit の transportation/adaption/features と
            //    現在地の地形クラスから `movement::unit_area_on_terrain` で推定。
            // 地形未定義 / マップ範囲外 / ユニット未検出時は空文字。
            let u = find_unit(app, args.first().copied().unwrap_or(""))?;
            if !u.current_area.is_empty() {
                return Some(u.current_area.clone());
            }
            let Some(map) = app.database().map.as_ref() else {
                return Some(String::new());
            };
            if u.x >= map.width || u.y >= map.height {
                return Some(String::new());
            }
            let terrain_id = map.cell(u.x, u.y).terrain_id;
            let raw_class = app
                .database()
                .terrains
                .iter()
                .find(|t| t.id == terrain_id)
                .map(|t| t.class.clone())
                .or_else(|| {
                    crate::data::terrain::DEFAULT_TERRAINS
                        .iter()
                        .find(|t| t.id == terrain_id)
                        .map(|t| t.class.to_string())
                })
                .unwrap_or_default();
            // ユニット静的データを引いて area 推定
            let unit_data_name = u.unit_data_name.clone();
            let active_feature_names: Vec<String> =
                u.active_features.iter().map(|f| f.name.clone()).collect();
            let (transportation, adaption) = app
                .database()
                .unit_by_name(&unit_data_name)
                .map(|ud| (ud.transportation.clone(), ud.adaption.0))
                .unwrap_or_else(|| ("陸".to_string(), *b"AAAA"));
            let area = crate::movement::unit_area_on_terrain(
                &raw_class,
                &transportation,
                &adaption,
                &active_feature_names,
            );
            Some(area.to_string())
        }
        "Distance" => {
            // `Distance(unitA, unitB)` — マンハッタン距離。いずれかが
            // 未解決の場合は 0 を返す (SRC.Sharp 同等)。
            if args.len() < 2 {
                return None;
            }
            let Some(a) = find_unit(app, args[0]) else {
                return Some("0".to_string());
            };
            let Some(b) = find_unit(app, args[1]) else {
                return Some("0".to_string());
            };
            let d = (a.x as i64 - b.x as i64).abs() + (a.y as i64 - b.y as i64).abs();
            Some(d.to_string())
        }
        "Count" => {
            // `Count(prefix)` — 配列変数 `prefix[*]` の要素数を返す。
            // SRC.Sharp `Other.cs::Count` (line 6-83) 同等: 名前が
            // `prefix[` で始まる script_var の個数。
            //
            // 過去実装は party label → 該当陣営ユニット数 を返していたが、
            // 実シナリオの `Count(入手ユニット候補)` 等は **配列プレフィックス**
            // のカウント用途。party 数は `味方数` / `敵数` 等のシステム変数で
            // 取得する設計。
            //
            // 後方互換のため、prefix が party label と一致した場合のみ legacy
            // 挙動 (該当陣営の unit_instances 数) も併用する。
            if args.is_empty() {
                return None;
            }
            let key = fn_arg_value(app, args.first()?);
            let key = key.trim().trim_matches('"');
            let prefix = format!("{key}[");
            let array_count = app
                .script_vars()
                .keys()
                .filter(|k| k.starts_with(&prefix))
                .count();
            if array_count > 0 {
                return Some(array_count.to_string());
            }
            // legacy fallback: party 名と一致したら該当陣営の unit 数
            if let Some(party) = parse_party_label(key) {
                let n = app
                    .database()
                    .unit_instances
                    .iter()
                    .filter(|u| u.party == party && !u.off_map)
                    .count();
                return Some(n.to_string());
            }
            Some("0".to_string())
        }
        "Exists" => {
            let key = args.first()?;
            Some(
                if find_unit(app, key).is_some() {
                    "1"
                } else {
                    "0"
                }
                .to_string(),
            )
        }
        "Skill" => {
            // `Skill(pilot, skill_name)` — pilot が指定スキルを持っていれば
            // そのレベル値、無ければ 0。SRC.Sharp `Functions/Pilot.cs::Skill` 同等。
            // 優先順位: PilotInstance.skills → PilotData.features。
            if args.len() < 2 {
                return None;
            }
            let pilot_key = fn_arg_value(app, args[0]);
            let pilot_key = pilot_key.trim().trim_matches('"');
            let skill_name = fn_arg_value(app, args[1]);
            let skill_name = skill_name.trim().trim_matches('"');
            // 1) PilotInstance.skills をチェック (SetSkill で動的付与されたスキル)
            let inst_level = app
                .database()
                .pilot_instances
                .iter()
                .find(|p| p.id == pilot_key || p.pilot_data_name == pilot_key)
                .and_then(|pi| {
                    pi.skills
                        .iter()
                        .find(|s| s.starts_with(skill_name))
                        .map(|s| {
                            // "スキル名 N" → N, "スキル名" → 1
                            s[skill_name.len()..]
                                .trim()
                                .parse::<i64>()
                                .ok()
                                .unwrap_or(1)
                        })
                });
            if let Some(lv) = inst_level {
                return Some(lv.to_string());
            }
            // 2) PilotData.features (静的データ)
            let pd = app
                .database()
                .pilots
                .iter()
                .find(|p| p.name == pilot_key || p.nickname == pilot_key)
                .or_else(|| {
                    let inst = app
                        .database()
                        .unit_instances
                        .iter()
                        .find(|u| matches_unit_handle(u, pilot_key))?;
                    app.database()
                        .pilots
                        .iter()
                        .find(|p| p.name == inst.pilot_name || p.nickname == inst.pilot_name)
                });
            let Some(pd) = pd else {
                return Some("0".to_string());
            };
            let level = pd
                .features
                .iter()
                .find(|(k, _)| k == skill_name)
                .map(|(_, v)| {
                    if v.is_empty() {
                        1
                    } else {
                        v.trim().parse::<i64>().unwrap_or(1)
                    }
                })
                .unwrap_or(0);
            Some(level.to_string())
        }
        "IsAvailable" => {
            // `IsAvailable(unit, feature)` — unit が指定の特殊能力 (feature)
            // を持っているか。SRC.Sharp `Unit/Unit.cs::IsAvailable` 同等。
            // 優先: UnitInstance.active_features (is_active=true) →
            // フォールバック: UnitData.features の key 一致。
            if args.len() < 2 {
                return None;
            }
            let key = fn_arg_value(app, args[0]);
            let feat = fn_arg_value(app, args[1]);
            let feat = feat.trim().trim_matches('"').to_string();
            let has = match find_unit(app, &key) {
                Some(inst) => {
                    // 1) active_features で is_active なものを検索
                    let from_active = inst
                        .active_features
                        .iter()
                        .any(|f| f.is_active && f.name == feat);
                    if from_active {
                        true
                    } else {
                        // 2) static UnitData.features にある場合も true
                        app.database()
                            .units
                            .iter()
                            .find(|u| u.name == inst.unit_data_name)
                            .map(|u| u.features.iter().any(|(k, _)| k.as_str() == feat))
                            .unwrap_or(false)
                    }
                }
                None => false,
            };
            Some(if has { "1" } else { "0" }.to_string())
        }
        "IsDefined" => {
            // `IsDefined(name [, kind])` — SRC.Sharp
            // `Expressions/Functions/Other/Other.cs::IsDefined` 同等。
            // - 1 arg: pilot / unit_instance / item の何れかに存在すれば 1
            // - 2 args: kind ("パイロット" / "ユニット" / "アイテム") を絞る
            let pname = fn_arg_value(app, args.first()?);
            let pname = pname.trim().trim_matches('"');
            let kind = args.get(1).map(|s| {
                let v = fn_arg_value(app, s);
                v.trim().trim_matches('"').to_string()
            });
            let is_pilot = || {
                app.database()
                    .pilots
                    .iter()
                    .any(|p| p.name == pname || p.nickname == pname)
                    || app
                        .database()
                        .unit_instances
                        .iter()
                        .any(|u| u.pilot_name == pname && !u.off_map)
            };
            let is_unit = || {
                app.database()
                    .unit_instances
                    .iter()
                    .any(|u| matches_unit_handle(u, pname) && !u.off_map)
            };
            let is_item = || app.database().items.iter().any(|i| i.name == pname);
            let found = match kind.as_deref() {
                Some("パイロット") => is_pilot(),
                Some("ユニット") => is_unit(),
                Some("アイテム") => is_item(),
                Some(_) => false,
                None => is_pilot() || is_unit() || is_item(),
            };
            Some(if found { "1" } else { "0" }.to_string())
        }
        "Random" => {
            // SRC `Random(n)` = `GeneralLib.Dice(n)` 相当で **1..n** を返す
            // (`(int)(n * rand[0,1)) + 1`)。`n <= 1` のときは n をそのまま返す。
            // `Lindex(list, Random(Llength(list)))` のように 1-indexed の
            // `Lindex` 引数に直結する用法が多いため 0..n-1 では破綻する。
            let n: i64 = fn_arg_value(app, args.first()?).trim().parse().ok()?;
            if n <= 1 {
                return Some(n.max(0).to_string());
            }
            let v = pseudo_random(app) % (n as u32) + 1;
            Some(v.to_string())
        }
        "LoadFileDialog" | "SaveFileDialog" => {
            // 実機はファイル選択ダイアログを開く。ヘッドレス (WASM/検証) には GUI が無いので、
            // script_var `__verify_loadfile` に設定されたパスを返す。未設定なら "" =「キャンセル
            // された」相当 (通常プレイでも "" のままなので副作用なし)。検証ドライバが
            // `__verify_loadfile` をセットすると `データロード` 経路を駆動できる。
            Some(app.script_var("__verify_loadfile").to_string())
        }
        _ => None,
    }
}

/// `args_str` を `,` で分割。ただし `"..."` 内 / `(...)` 内 / `[...]` 内のカンマは
/// 区切りとして扱わない。クォートは保持。
pub(super) fn split_function_args(s: &str) -> Vec<&str> {
    // 引数が無い (`Func()`) ときだけ空 Vec。それ以外は空トークンも保持する:
    // `LIndex(, 1)` は **2 引数** (空, "1") であり、空を捨てると位置引数が
    // ずれて関数評価が壊れる (`Lindex` が引数不足で None を返す等)。
    if s.trim().is_empty() {
        return Vec::new();
    }
    let bytes = s.as_bytes();
    let mut out: Vec<&str> = Vec::new();
    let mut start = 0usize;
    let mut paren = 0i32;
    let mut bracket = 0i32;
    let mut in_quote = false;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        match b {
            b'"' => in_quote = !in_quote,
            b'(' if !in_quote => paren += 1,
            b')' if !in_quote && paren > 0 => paren -= 1,
            b'[' if !in_quote => bracket += 1,
            b']' if !in_quote && bracket > 0 => bracket -= 1,
            b',' if !in_quote && paren == 0 && bracket == 0 => {
                out.push(s[start..i].trim());
                start = i + 1;
            }
            _ => {}
        }
        i += 1;
    }
    out.push(s[start..].trim());
    out
}

/// 関数名の正規化: ASCII の case-insensitive。代表的な別名 (`Llength` /
/// `LLength`, `Length`) を 1 つに寄せる。
pub(super) fn canonical_function_name(s: &str) -> String {
    let lower = s.to_ascii_lowercase();
    match lower.as_str() {
        "llength" | "length" => "Llength".to_string(),
        "lindex" => "Lindex".to_string(),
        "lsearch" => "Lsearch".to_string(),
        "lsplit" => "Lsplit".to_string(),
        "lremove" => "Lremove".to_string(),
        "replace" => "Replace".to_string(),
        "args" => "Args".to_string(),
        "hp" => "HP".to_string(),
        "maxhp" => "MaxHP".to_string(),
        "en" => "EN".to_string(),
        "maxen" => "MaxEN".to_string(),
        "morale" => "Morale".to_string(),
        "exp" => "Exp".to_string(),
        "level" => "Level".to_string(),
        "sp" => "SP".to_string(),
        "plana" => "Plana".to_string(),
        "relation" => "Relation".to_string(),
        "x" => "X".to_string(),
        "y" => "Y".to_string(),
        "distance" => "Distance".to_string(),
        "count" => "Count".to_string(),
        "exists" => "Exists".to_string(),
        "random" => "Random".to_string(),
        "money" => "Money".to_string(),
        "turn" => "Turn".to_string(),
        "phase" => "Phase".to_string(),
        "stage" => "Stage".to_string(),
        "terrainid" => "TerrainId".to_string(),
        "rank" => "Rank".to_string(),
        "list" => "List".to_string(),
        "info" => "Info".to_string(),
        "not" => "Not".to_string(),
        "min" => "Min".to_string(),
        "max" => "Max".to_string(),
        "abs" => "Abs".to_string(),
        "len" => "Len".to_string(),
        "hasitem" => "HasItem".to_string(),
        "hasstatus" => "HasStatus".to_string(),
        "armor" => "Armor".to_string(),
        "mobility" => "Mobility".to_string(),
        "speed" => "Speed".to_string(),
        "dir" => "Dir".to_string(),
        "rgb" => "RGB".to_string(),
        "iif" => "IIF".to_string(),
        "strcmp" | "strcompare" | "strcomp" => "StrCmp".to_string(),
        "left" => "Left".to_string(),
        "right" => "Right".to_string(),
        "mid" => "Mid".to_string(),
        "unitid" => "UnitID".to_string(),
        "pilotid" => "PilotID".to_string(),
        "pilot" => "Pilot".to_string(),
        "countpilot" => "CountPilot".to_string(),
        "item" => "Item".to_string(),
        "itemid" => "ItemID".to_string(),
        // SRC 原名: アイテム番号系 (真化前後・入手済みは本移植では非区別)。
        "アイテム番号" | "真アイテム番号" | "入手アイテム番号" => {
            "ItemIndex".to_string()
        }
        "unit" => "Unit".to_string(),
        "area" => "Area".to_string(),
        "party" => "Party".to_string(),
        "int" => "Int".to_string(),
        "eval" => "Eval".to_string(),
        "round" => "Round".to_string(),
        "roundup" => "RoundUp".to_string(),
        "rounddown" => "RoundDown".to_string(),
        "sqr" => "Sqr".to_string(),
        "sin" => "Sin".to_string(),
        "cos" => "Cos".to_string(),
        "tan" => "Tan".to_string(),
        "atn" => "Atn".to_string(),
        "log" => "Log".to_string(),
        "sgn" => "Sgn".to_string(),
        "mod" => "Mod".to_string(),
        "hex" => "Hex".to_string(),
        "oct" => "Oct".to_string(),
        "atan2" => "Atan2".to_string(),
        "now" => "Now".to_string(),
        "gettime" => "GetTime".to_string(),
        "year" => "Year".to_string(),
        "month" => "Month".to_string(),
        "day" => "Day".to_string(),
        "hour" => "Hour".to_string(),
        "minute" => "Minute".to_string(),
        "second" => "Second".to_string(),
        "weekday" => "Weekday".to_string(),
        "difftime" => "DiffTime".to_string(),
        "fileexists" => "FileExists".to_string(),
        "folderexists" => "FolderExists".to_string(),
        "filelen" => "FileLen".to_string(),
        "loc" => "Loc".to_string(),
        "eof" => "EOF".to_string(),
        "lof" => "LOF".to_string(),
        "action" => "Action".to_string(),
        "damage" => "Damage".to_string(),
        "condition" => "Condition".to_string(),
        "status" => "Status".to_string(),
        "bullet" => "Bullet".to_string(),
        "maxbullet" => "MaxBullet".to_string(),
        "countitem" => "CountItem".to_string(),
        "wx" => "WX".to_string(),
        "wy" => "WY".to_string(),
        "regexp" => "RegExp".to_string(),
        "regexpmatch" => "RegExpMatch".to_string(),
        "regexpreplace" => "RegExpReplace".to_string(),
        "partner" => "Partner".to_string(),
        "countpartner" => "CountPartner".to_string(),
        "isequiped" => "IsEquiped".to_string(),
        "isequipped" => "IsEquipped".to_string(),
        "specialpower" | "mind" => "SpecialPower".to_string(),
        "isvardefined" => "IsVarDefined".to_string(),
        "isdefined" => "IsDefined".to_string(),
        "isavailable" => "IsAvailable".to_string(),
        "isnumeric" => "IsNumeric".to_string(),
        "skill" => "Skill".to_string(),
        "lset" => "LSet".to_string(),
        "rset" => "RSet".to_string(),
        "nickname" => "Nickname".to_string(),
        "format" => "Format".to_string(),
        "instr" => "InStr".to_string(),
        "instrrev" => "InStrRev".to_string(),
        "string" => "String".to_string(),
        "wide" => "Wide".to_string(),
        "lcase" => "LCase".to_string(),
        "ucase" => "UCase".to_string(),
        "trim" => "Trim".to_string(),
        "asc" => "Asc".to_string(),
        "chr" => "Chr".to_string(),
        "term" => "Term".to_string(),
        "textwidth" => "TextWidth".to_string(),
        "textheight" => "TextHeight".to_string(),
        "keystate" => "KeyState".to_string(),
        "windowwidth" => "WindowWidth".to_string(),
        "windowheight" => "WindowHeight".to_string(),
        "font" => "Font".to_string(),
        "lenb" => "LenB".to_string(),
        "leftb" => "LeftB".to_string(),
        "rightb" => "RightB".to_string(),
        "midb" => "MidB".to_string(),
        "instrb" => "InStrB".to_string(),
        "instrrevb" => "InStrRevB".to_string(),
        "playingmidi" => "PlayingMidi".to_string(),
        "playingsound" => "PlayingSound".to_string(),
        "pointx" => "PointX".to_string(),
        "pointy" => "PointY".to_string(),
        "basex" => "BaseX".to_string(),
        "basey" => "BaseY".to_string(),
        _ => s.to_string(),
    }
}
