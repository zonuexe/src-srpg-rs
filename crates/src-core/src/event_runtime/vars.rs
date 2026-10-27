//! 変数展開 (`$(name)` / 添字付き変数 / 文字列連結) と代入先の解決、システム変数、
//! `Call` 引数スタック。

use super::*;

/// 引数 1 個を `$(name)` / `Args(N)` / 各種関数呼出記法を展開して返す。
/// 未定義変数は空文字列。
///
/// 対応関数:
/// - `Args(N)`            → N 番目の Call 引数
/// - `HP(name)`           → 現 HP (max - damage)
/// - `MaxHP(name)`        → 最大 HP
/// - `EN(name)`           → 現 EN (max - en_consumed)
/// - `MaxEN(name)`        → 最大 EN
/// - `Morale(name)`       → 士気
/// - `Exp(name)`          → 累積獲得経験値
/// - `X(name)` / `Y(name)`→ ユニット座標
/// - `Distance(a, b)`     → マンハッタン距離
/// - `Count("Player"|"Enemy"|"Allied"|"Neutral")` → 勢力人数
/// - `Exists(name)`       → 1 (存在) / 0 (不在)
/// - `Random(n)`          → 0 .. n-1 のランダム整数（splitmix64）
pub(super) fn expand_vars(app: &App, src: &str) -> String {
    let bytes = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    // ダブルクオート内 (= 文字列リテラル) では、裸の `Name(args)` 関数呼出や
    // `name[expr]` インデックス変数を**展開しない**。これをしないと
    // `Instr(v, "設定[パイロット一覧]")` のリテラルが、たまたま同名の配列変数
    // `設定[パイロット一覧]` が定義済みのとき**その値に化けて**しまい、比較が壊れる
    // (D データロードの行検出が常に失敗する原因だった)。`$(...)` 明示補間のみ
    // クオート内でも従来どおり展開する (`Talk "$(name)"` 等)。
    let mut in_quote = false;
    while i < bytes.len() {
        if bytes[i] == b'"' {
            in_quote = !in_quote;
            out.push('"');
            i += 1;
            continue;
        }
        // `$(name)` 展開。`$(name[expr])` のインデックス記法および
        // `$(Args(1))` のように name 中に paren を含むキーにも対応するため、
        // 開き `(` のあとは括弧バランスを取りながら対応する `)` を探す。
        // 中身は再帰展開しない (literal なキー名として扱う) — `Args(1)` は
        // 文字列キーそのままで script_var に格納されているため、展開すると
        // value への二重 lookup になってしまう。
        if i + 1 < bytes.len() && bytes[i] == b'$' && bytes[i + 1] == b'(' {
            if let Some(close_idx) = find_matching_paren(src, i + 1) {
                let name_raw = &src[i + 2..close_idx];
                out.push_str(&expand_dollar_paren(app, name_raw));
                i = close_idx + 1;
                continue;
            }
        }
        // 関数呼び出し `Name(args)` 形式の検出 (クオート内はスキップ=リテラル扱い)。
        if let Some((fn_name, args_str, total)) = take_function_call(src, i).filter(|_| !in_quote) {
            // 引数文字列を再帰展開してから評価する。ただし `IsVarDefined` は
            // 引数を **変数名そのもの** として必要とするため展開しない。
            // 展開すると `IsVarDefined(配列[1])` が配列要素の値に化け、
            // 定義済みでも「未定義」と誤判定する (添字 `[expr]` の解決は
            // `IsVarDefined` 内の `resolve_lhs_name` が行う)。
            let expanded_args = if fn_name.eq_ignore_ascii_case("IsVarDefined") {
                args_str.to_string()
            } else {
                expand_vars(app, args_str)
            };
            if let Some(v) = eval_script_function(app, fn_name, &expanded_args) {
                out.push_str(&v);
            } else {
                // 未対応 / 評価失敗の関数呼出はトークン全体をそのまま残す。
                // ここで 1 文字ずつ fall-through すると、内側の `x(...)` 等を
                // 別関数として誤検出してしまう (例: `LIndex(...)` の `x(` を
                // `X()` 関数と誤認し `LInde0` に化ける)。
                out.push_str(&src[i..i + total]);
            }
            i += total;
            continue;
        }
        // インデックス変数 `name[expr]` の展開 (クオート内はスキップ=リテラル扱い)。
        if let Some((var_value, total)) = take_indexed_var(app, src, i).filter(|_| !in_quote) {
            out.push_str(&var_value);
            i += total;
            continue;
        }
        let ch_end = next_char_end(src, i);
        out.push_str(&src[i..ch_end]);
        i = ch_end;
    }
    out
}

/// `src[i..]` の先頭が `name[expr]` 形式のインデックス変数参照かを判定。
/// マッチした場合、展開後の値と消費 byte 数を返す。
/// 識別子は「空白 / 区切り文字以外」の連続文字列。少なくとも 1 文字必要。
pub(super) fn take_indexed_var(app: &App, src: &str, i: usize) -> Option<(String, usize)> {
    let bytes = src.as_bytes();
    // 識別子の先頭文字判定: 区切り文字でないことを要求
    let start_ch = src[i..].chars().next()?;
    if !is_indexed_ident_char(start_ch) {
        return None;
    }
    // 識別子を読み進める
    let mut j = i;
    while j < bytes.len() {
        let ch = src[j..].chars().next()?;
        if !is_indexed_ident_char(ch) {
            break;
        }
        j += ch.len_utf8();
    }
    let name = &src[i..j];
    if name.is_empty() || j >= bytes.len() || bytes[j] != b'[' {
        return None;
    }
    // 対応する `]` を balanced で探す
    let bracket_start = j;
    let mut depth = 1i32;
    let mut k = j + 1;
    while k < bytes.len() && depth > 0 {
        match bytes[k] {
            b'[' => depth += 1,
            b']' => depth -= 1,
            _ => {}
        }
        k += 1;
    }
    if depth != 0 {
        return None;
    }
    let _ = bracket_start;
    // `name[inner]` 全体を `resolve_lhs_name` で多次元キーに正規化して
    // script_var を引く。`Set` の LHS と同じキー解決規約なので
    // `Set 搭乗員[1,2] …` と読出 `搭乗員[i,j]` が一致する。
    let key = resolve_lhs_name(app, &src[i..k]);
    let v = app.script_var(&key);
    if v.is_empty() {
        // 未定義 / 空はリテラルのまま残す。`expand_vars` は Talk 本文等の
        // 一般テキストにも適用されるため、`アイテム[特殊]` のように偶然
        // `識別子[...]` を含むリテラル文を空に潰さないための保全措置。
        // 値文脈 (比較 / Set 値 / Switch 等) で未定義 indexed 参照を空に
        // 解決するのは `fn_arg_value` の責務。
        return None;
    }
    Some((v.to_string(), k - i))
}

/// `Set name[expr]` の LHS を評価して、実際の格納キーを返す。
/// `expr` 部は再帰的に評価し、`[exprA][exprB]` のような多次元アクセスにも対応。
/// `name` が `[` を含まなければそのまま返す。
pub(super) fn resolve_lhs_name(app: &App, raw: &str) -> String {
    let bytes = raw.as_bytes();
    let Some(bracket_pos) = bytes.iter().position(|&b| b == b'[') else {
        return raw.to_string();
    };
    let mut depth = 1i32;
    let mut k = bracket_pos + 1;
    while k < bytes.len() && depth > 0 {
        match bytes[k] {
            b'[' => depth += 1,
            b']' => depth -= 1,
            _ => {}
        }
        k += 1;
    }
    if depth != 0 {
        return raw.to_string();
    }
    let name = &raw[..bracket_pos];
    let inner = &raw[bracket_pos + 1..k - 1];
    // 多次元添字 `name[i,j]` はトップレベルのカンマで分割し、各添字を
    // 個別に解決して `name[v1,v2]` に正規化する。これにより
    // `Set 搭乗員[1,3] …` と `$(搭乗員[i,j])` (i=1,j=3) が同じキーを指す。
    let inner_val = split_function_args(inner)
        .iter()
        .map(|part| resolve_index_part(app, part))
        .collect::<Vec<_>>()
        .join(",");
    let trailing = &raw[k..];
    let head = format!("{name}[{inner_val}]");
    if trailing.is_empty() {
        head
    } else {
        format!("{head}{}", resolve_lhs_name(app, trailing))
    }
}

/// `HP(unit) = n` / `EN(unit) = n` 等、関数名を左辺値とする代入を処理する。
///
/// SRC.Sharp は次の関数を「代入可能な左辺値」として定義する:
/// - `HP(unit)`     → ユニットの現在 HP を設定 (clamp: 1..=MaxHP)
/// - `EN(unit)`     → ユニットの現在 EN を設定 (clamp: 0..=MaxEN)
/// - `Action(unit)` → 残り行動数を設定 (`has_acted = (n <= 0)`)
/// - `Morale(unit)` → 士気を設定 (clamp: 0..=150)
/// - `SP(pilot)`    → SP を設定 (clamp: 0..=MaxSP)
/// - `Plana(pilot)` → 霊力を設定
///
/// 処理した場合は `true` を返す。`lhs` が関数呼出形式でない、または上記以外の
/// 関数名の場合は `false` を返し、呼び出し元が通常の `set_script_var` を続ける。
/// システム変数への書き込みを処理する。SRC.Sharp `Variable.cs` の `SetVariable`
/// の特殊ケース (`ターン数` / `総ターン数` / `資金`) に対応。
/// 書き込みを処理した場合は `true`、対象外なら `false`。
pub(super) fn try_system_var_assign(app: &mut App, lhs: &str, value: &str) -> bool {
    let n_i64 = eval::try_numeric(app, value).map_or(0, |v| v.trunc() as i64);
    let n_i64 = || n_i64;
    match lhs {
        "ターン数" => {
            app.set_turn_number(n_i64().max(0) as u32);
            true
        }
        "総ターン数" => {
            app.set_total_turn(n_i64().max(0) as u32);
            true
        }
        "資金" => {
            app.set_money(n_i64());
            true
        }
        // ArgNum は読み取り専用 (SRC 仕様)。Set/Incr を silent no-op で吸収。
        "ArgNum" => true,
        _ => false,
    }
}

pub(super) fn try_function_lhs_assign(app: &mut App, lhs: &str, value: &str) -> bool {
    // `FuncName(arg)` の形式か判定。
    let Some(open) = lhs.find('(') else {
        return false;
    };
    if !lhs.ends_with(')') {
        return false;
    }
    let func_name = &lhs[..open];
    let arg = lhs[open + 1..lhs.len() - 1].trim();
    let lower = func_name.to_ascii_lowercase();

    // 数値変換: 非数値は 0 扱い (SRC.Sharp VB5 `Val()` 相当)。
    let n: i64 = eval::try_numeric(app, value).map_or(0, |v| v.trunc() as i64);

    match lower.as_str() {
        "hp" => {
            // HP(unit) = n → damage = max_hp - clamp(n, 0, max_hp)
            // C# Unit.HP setter: max(0, min(MaxHP, value)) → HP=0 は有効 (ユニット撃墜状態)。
            let Some(idx) = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, arg))
            else {
                return true;
            };
            let max_hp = {
                let db = app.database();
                db.effective_max_hp(&db.unit_instances[idx])
            };
            let new_hp = n.clamp(0, max_hp.max(0));
            app.database_mut().unit_instances[idx].damage = (max_hp - new_hp).max(0);
            true
        }
        "en" => {
            // EN(unit) = n → en_consumed = max_en - clamp(n, 0, max_en)
            let Some(idx) = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, arg))
            else {
                return true;
            };
            let max_en = {
                let db = app.database();
                db.effective_max_en(&db.unit_instances[idx])
            };
            let new_en = (n as i32).clamp(0, max_en);
            app.database_mut().unit_instances[idx].en_consumed = (max_en - new_en).max(0);
            true
        }
        "morale" => {
            // Morale(pilot_or_unit) = n → morale = clamp(n, 50, 150)
            // C# Pilot.SetMorale: clamp(MinMorale=50, MaxMorale=150)。
            let Some(idx) = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, arg))
            else {
                return true;
            };
            app.database_mut().unit_instances[idx].morale = (n as i32).clamp(50, 150);
            true
        }
        "action" => {
            // Action(unit) = n → has_acted = (n <= 0)
            // n > 0: 行動可能 (has_acted = false)、n <= 0: 行動済 (has_acted = true)。
            let Some(idx) = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, arg))
            else {
                return true;
            };
            app.database_mut().unit_instances[idx].has_acted = n <= 0;
            true
        }
        "sp" => {
            // SP(pilot) = n → sp_consumed = max(0, max_sp - n)
            let Some(idx) = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, arg))
            else {
                return true;
            };
            let max_sp = {
                let db = app.database();
                let pilot_name = db.unit_instances[idx].pilot_name.clone();
                db.pilot_by_name(&pilot_name)
                    .and_then(|p| p.sp)
                    .unwrap_or(0)
            };
            let new_sp = (n as i32).clamp(0, max_sp);
            app.database_mut().unit_instances[idx].sp_consumed = (max_sp - new_sp).max(0);
            true
        }
        "plana" => {
            // Plana(pilot) = n → plana = n
            let Some(idx) = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, arg))
            else {
                return true;
            };
            app.database_mut().unit_instances[idx].plana = n as i32;
            true
        }
        _ => false,
    }
}

/// `name[i,j]` の添字 1 つ分を解決する。`$(...)` / 関数を展開し、裸の
/// 識別子なら script_var として引き、最後に整数式として評価できれば
/// 数値へ正規化する。
pub(super) fn resolve_index_part(app: &App, part: &str) -> String {
    let mut val = expand_vars(app, part);
    if val.trim() == part.trim() {
        let single = part.trim();
        if !single.is_empty() && single.chars().all(is_indexed_ident_char) {
            let v = app.script_var(single);
            if !v.is_empty() {
                val = v.to_string();
            }
        }
    }
    // 数値になる添字 (`3` / `(i + 1)` / `j - 1` / `((Args(2) - 1) * 4 + i)`) は数値へ
    // 正規化する。演算子は空白で区切られた項のときだけ働くので、`R-1` のようなハイフン付き
    // の文字列キーは 1 項のまま (原典 GetValueAsString と同じ)。
    match eval::try_numeric(app, &val) {
        Some(n) => format_num(n),
        None => val.trim().to_string(),
    }
}

/// インデックス変数の識別子に含めて良い文字か。
/// 区切り文字以外なら true。
pub(super) fn is_indexed_ident_char(c: char) -> bool {
    !c.is_whitespace()
        && !matches!(
            c,
            '(' | ')'
                | '['
                | ']'
                | ','
                | '"'
                | '\''
                | '+'
                | '-'
                | '*'
                | '/'
                | '<'
                | '>'
                | '='
                | '&'
                | '|'
                | '!'
                | '$'
                | '\\'
                | '#'
                | ';'
                | ':'
        )
}

/// `$(...)` の中身 (`name_raw`) を解決する。
///
/// - 中身全体が関数呼び出し (`Lindex(...)` / `Info(...)` / `Args(1)` 等) なら
///   式として評価する。`$(Lindex(タイトル画面アクション[i],1))` のように
///   引数にインデックス変数を含むケースも `expand_vars` 経由で再帰展開される。
/// - それ以外は変数キーとして lookup する (`name[expr]` は実キーに解決)。
///
/// 関数評価が未対応関数等で失敗 (展開後も文字列が変わらない) した場合は、
/// `$(Args(1))` のように関数名そのものが変数キーとして格納されている
/// 可能性に備え、リテラルキー lookup にフォールバックする。
pub(super) fn expand_dollar_paren(app: &App, name_raw: &str) -> String {
    let trimmed = name_raw.trim();
    let is_whole_fn_call =
        take_function_call(trimmed, 0).is_some_and(|(_, _, total)| total == trimmed.len());
    if is_whole_fn_call {
        let evaluated = expand_vars(app, trimmed);
        if evaluated != trimmed {
            return evaluated;
        }
    }
    let key = if name_raw.contains('[') {
        resolve_lhs_name(app, name_raw)
    } else {
        name_raw.to_string()
    };
    // システム変数を優先して解決 (`$(ターン数)` / `$(資金)` 等)。
    if let Some(sys) = system_variable_value(app, &key) {
        return sys;
    }
    app.script_var(&key).to_string()
}

pub(super) fn fn_arg_value(app: &App, arg: &str) -> String {
    let s = arg.trim();
    if s.starts_with('"') && s.ends_with('"') && s.len() >= 2 {
        return s[1..s.len() - 1].to_string();
    }
    // 数値リテラル
    if s.parse::<f64>().is_ok() {
        return s.to_string();
    }
    // SRC のシステム変数 (動的に評価されるカウンタ等)
    if let Some(sys) = system_variable_value(app, s) {
        return sys;
    }
    // 変数として引いて見て、定義済みなら値を返す (空文字列の変数も正しく返す)。
    // SRC.Sharp 準拠: 定義済み変数 `Set v ""` は空文字列を返す。
    // `!v.is_empty()` ではなく `is_script_var_defined` でチェックすること。
    if app.is_script_var_defined(s) {
        return app.script_var(s).to_string();
    }
    // `name[expr]` の indexed 参照: 直接キーで引けなくても `resolve_lhs_name`
    // で添字を評価したキーで再検索する。未定義なら **空文字** を返す
    // (SRC: 未定義変数は空)。リテラル `name[expr]` を比較式に漏らさない
    // — 漏らすと `If 配列[1] <> ""` のような未定義チェックが常に真になる。
    if s.len() > 2 && s.ends_with(']') {
        if let Some(open) = s.find('[') {
            if open > 0 {
                let key = resolve_lhs_name(app, s);
                return app.script_var(&key).to_string();
            }
        }
    }
    // 算術式の自動評価は **意図的に行わない**: `"1-2-3"` のような文字列を
    // -4 と誤評価してしまう (Set msg のような string 文脈で破綻する)。
    // 算術評価が必要な数学関数 (Abs / Min / Max / Round / Int 等) は
    // 各 arm 側で `numeric_arg(app, s)` を呼んで明示的に評価する。
    //
    // 最終フォールバックは **trim 前の生引数** を返す。`LSet("AB",5)` →
    // `"AB   "` のような関数評価結果の trailing space を保持するため。
    arg.to_string()
}

/// 数学関数引数用の数値評価ヘルパ。`fn_arg_value` の結果を統一評価器で評価する。
///
/// SRC `EvalTerm` は関数引数も式評価し**裸の変数を数値解決**するため、数値関数
/// (`Round`/`Int`/`Min`/`Max`/`Abs`/`Mod` 等、本ヘルパを呼ぶ arm に限定) の引数で
/// `Round(温泉宿１収入 + 温泉宿２収入)` のような裸変数算術を評価できるようにする。
/// 文字列関数 (`Left`/`Mid` 等) は `fn_arg_value` を直接使うため影響しない。
/// ネストした関数呼出は `expand_vars` が事前に値へ展開済みなので、ここに届く時点で
/// 残るのは裸変数・システム変数・算術演算子・数値リテラルのみ。
pub(super) fn numeric_arg(app: &App, arg: &str) -> Option<f64> {
    let s = fn_arg_value(app, arg);
    if let Ok(v) = s.parse::<f64>() {
        return Some(v);
    }
    // 裸の変数・システム変数を含む算術式 (`温泉宿１収入 + 温泉宿２収入` / `資金 + 100`) を
    // 統一評価器で評価する。文字列になる値 (`"AB"` 等) は `None` を返し、
    // 「数値なら数値・そうでなければ文字列」で分岐する呼出元 (LSet/RSet) の契約を守る。
    eval::try_numeric(app, &s)
}

/// 本移植が `AppVersion` で報告する SRC 互換バージョン番号。
///
/// 原典 SRC.Sharp `Expression.cs` の `appversion` は
/// `10000*Major + 100*Minor + Revision` を返す。本移植は公式フルパッケージ
/// 版 2.2.33 (本リポジトリの互換目標) 相当を報告する: `10000*2 + 100*2 + 33`。
/// SRC 2.00 以降を要求するシナリオ (`If AppVersion < 20000` 等) を通すための値。
pub const SRC_APP_VERSION: i64 = 10000 * 2 + 100 * 2 + 33;

/// SRC のシステム変数 (`味方数` / `敵数` / `友軍数` / `中立数` / `ターン数` 等)
/// に対する動的解決。該当しない名前なら `None`。
pub(super) fn system_variable_value(app: &App, name: &str) -> Option<String> {
    use crate::Party;
    let count_party = |p: Party| -> String {
        app.database()
            .unit_instances
            .iter()
            .filter(|u| !u.off_map && u.party == p)
            .count()
            .to_string()
    };
    // 各陣営の平均レベル (配置済みユニットのパイロットレベルの平均、切り捨て)。
    // レベルは `Level()` と同じく total_exp から算出 ((exp/100)+1, 上限 99)。
    let avg_level = |p: Party| -> String {
        let levels: Vec<i64> = app
            .database()
            .unit_instances
            .iter()
            .filter(|u| !u.off_map && u.party == p)
            .map(|u| i64::from(crate::pilot_instance::level_from_exp(u.total_exp)))
            .collect();
        if levels.is_empty() {
            "0".to_string()
        } else {
            (levels.iter().sum::<i64>() / levels.len() as i64).to_string()
        }
    };
    match name {
        "味方数" => Some(count_party(Party::Player)),
        "敵数" => Some(count_party(Party::Enemy)),
        // SRC.Sharp: "友軍" は "ＮＰＣ" 陣営。`ＮＰＣ数` を `友軍数` のエイリアスとして扱う。
        "ＮＰＣ数" | "友軍数" => Some(count_party(Party::Npc)),
        "中立数" => Some(count_party(Party::Neutral)),
        // 各陣営の平均レベル。実シナリオ (リンと凛 第038話) が `(味方レベル平均値 - 1)` を
        // 座標式に使う。SRC.Sharp 由来の集約値。
        "味方レベル平均値" => Some(avg_level(Party::Player)),
        "敵レベル平均値" => Some(avg_level(Party::Enemy)),
        "ＮＰＣレベル平均値" | "友軍レベル平均値" => Some(avg_level(Party::Npc)),
        "中立レベル平均値" => Some(avg_level(Party::Neutral)),
        "ターン数" => Some(app.turn().number.to_string()),
        "総ターン数" => Some(app.total_turn().to_string()),
        // SRC.Sharp `Expression.cs` の組込変数 `AppVersion`。SRC 2.00 以降を
        // 要求するシナリオ (Welcome.eve 冒頭の版数ゲート等) を通すために報告する。
        // 原典は変数名を小文字化して判別するため大小無視でマッチする。
        n if n.eq_ignore_ascii_case("AppVersion") => Some(SRC_APP_VERSION.to_string()),
        // `フェイズ` は現在のフェーズの陣営名。SRC.Sharp の `SRC.Stage` 相当。
        "フェイズ" => Some(app.turn().phase.stage_name().to_string()),
        // `資金` は `App.money()` から動的に解決する。
        "資金" => Some(app.money().to_string()),
        // ArgNum はトップレベル (Call 外) でも "0" を返す。
        // Call 中は enter_call_args が script_var("ArgNum") に引数数をセット済みなので
        // script_var 値が空文字でなければそちらを優先する。
        "ArgNum" => {
            let v = app.script_var("ArgNum");
            Some(if v.is_empty() {
                "0".to_string()
            } else {
                v.to_string()
            })
        }
        _ => None,
    }
}

/// `Call` 時の Args 束縛: 呼び出し元の `Args(1..9)` / `ArgNum` / `upvar_level` /
/// `upvar_base_argnum` をスナップショットして退避し、`new_args` を新しい
/// `Args(1..)` / `ArgNum` として束縛する。退避した値を返す
/// (`push_call_return` に渡し、`Return` で `restore_call_args` する)。
///
/// SRC の `Args` は呼び出しフレーム単位なので、ネストした `Call` が
/// 呼び出し元の `Args` を破壊しないようこのスナップショット/復元が要る。
///
/// saved の layout (可変長):
/// `[upvar_level, upvar_base_argnum, ArgNum, Args(1), ..., Args(ArgNum)]`
///
/// 原典の引数配列は `ReDim strArgs(2 To ArgNum)` の動的配列で個数制限が無い
/// (`CmdData.cls`)。旧実装は `Args(1..9)` 固定で 10 個目以降を取りこぼして
/// いた (実コーパスのサンプル 79 件中 9 件が `Args(10)` 以上を参照し、
/// 最大 `Args(14)` を確認)。
pub(super) fn enter_call_args(app: &mut App, new_args: &[String]) -> Vec<String> {
    let prev_argnum = current_argnum(app);
    let mut saved: Vec<String> = vec![
        app.upvar_level().to_string(),
        app.upvar_base_argnum().to_string(),
        prev_argnum.to_string(),
    ];
    saved.extend((1..=prev_argnum).map(|k| app.script_var(&format!("Args({k})")).to_string()));

    // 新フレーム用に Args をクリアして引数をセット。
    // 呼び出し元の分と新引数の分、両方を覆う範囲を消す。
    for k in 1..=prev_argnum.max(new_args.len()) {
        app.set_script_var(format!("Args({k})"), String::new());
    }
    // 引数は呼び出し側で評価済みの値 (`Call` はコマンド引数として `GetArgAsString`、
    // ユーザ定義関数は `eval::param_value`) なので、そのまま束縛する。
    for (i, v) in new_args.iter().enumerate() {
        app.set_script_var(format!("Args({})", i + 1), v.clone());
    }
    // ArgNum = 渡した引数の数
    app.set_script_var("ArgNum".to_string(), new_args.len().to_string());
    // 新フレームの upvar 状態を初期化
    app.set_upvar_level(0);
    app.set_upvar_base_argnum(0);
    saved
}

/// `Return` 時に呼び出し元の `Args(..)` / `ArgNum` / `upvar_level` /
/// `upvar_base_argnum` を復元する。layout は [`enter_call_args`] 参照。
pub(super) fn restore_call_args(app: &mut App, saved: Vec<String>) {
    let upvar_level = saved.first().and_then(|s| s.parse::<usize>().ok());
    let upvar_base = saved.get(1).and_then(|s| s.parse::<usize>().ok());
    let saved_argnum = saved
        .get(2)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0)
        .min(MAX_CALL_ARGS);
    // 呼び出された側が使っていた Args を消してから、呼び出し元の分を書き戻す。
    let callee_argnum = current_argnum(app);
    for k in (saved_argnum + 1)..=callee_argnum.max(saved_argnum) {
        app.set_script_var(format!("Args({k})"), String::new());
    }
    for k in 1..=saved_argnum {
        let v = saved.get(2 + k).cloned().unwrap_or_default();
        app.set_script_var(format!("Args({k})"), v);
    }
    app.set_script_var("ArgNum".to_string(), saved_argnum.to_string());
    app.set_upvar_level(upvar_level.unwrap_or(0));
    app.set_upvar_base_argnum(upvar_base.unwrap_or(0));
}

/// 現フレームの `ArgNum`。異常値は上限で頭打ちにする。
pub(super) fn current_argnum(app: &App) -> usize {
    app.script_var("ArgNum")
        .parse::<usize>()
        .unwrap_or(0)
        .min(MAX_CALL_ARGS)
}
