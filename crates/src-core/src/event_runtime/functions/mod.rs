//! SRC 組み込み関数 (`Args` / `Pilot()` / `Mid()` 等) の評価。

use super::*;

mod math;
mod pilot;
mod system;
mod text;
mod unit;

/// 組み込み関数 1 回分の呼び出しコンテキスト。カテゴリ別モジュールの各関数が受け取る。
#[derive(Clone, Copy)]
struct FnCall<'a> {
    /// スクリプト上の関数名 (大文字小文字は原文のまま)。
    name: &'a str,
    /// `split_function_args` で分割した引数 (未評価)。
    args: &'a Vec<&'a str>,
    /// `canonical_function_name` で正規化した関数名 (振り分けキー)。
    canon: &'a String,
}

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
    let c = FnCall {
        name,
        args: &args,
        canon: &canon,
    };
    unit::eval(app, &c)
        .or_else(|| pilot::eval(app, &c))
        .or_else(|| text::eval(app, &c))
        .or_else(|| math::eval(app, &c))
        .or_else(|| system::eval(app, &c))
        .flatten()
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
