//! 制御構造のブロック走査 (`If` / `Switch` / `For` / `Do` / `ForEach` の対応行探索)。

use super::*;

/// `Menu prompt` 後続行を `End` まで収集して選択肢列を返す。
/// 空行・コメント行は読み飛ばし。
pub(super) fn collect_menu_options(
    app: &App,
    start: usize,
    stmts: &[EventStatement],
) -> (Vec<String>, usize) {
    let mut options = Vec::new();
    let mut i = start;
    while i < stmts.len() {
        if let EventStatement::Command {
            name,
            args,
            raw_args,
            ..
        } = &stmts[i]
        {
            if name.eq_ignore_ascii_case("end") {
                return (options, i + 1);
            }
            // 1 行を 1 選択肢とし、`$(...)` を展開する (原典 `FormatMessage`)。
            let s = message_line(app, name, args, raw_args);
            // 空展開で空白だけが残った行はメニューから外す (SRC 仕様)。
            let trimmed = s.trim();
            if !trimmed.is_empty() {
                options.push(trimmed.to_string());
            }
        }
        i += 1;
    }
    (options, i)
}

/// `If` 条件式の 1 「項 (term)」が消費するトークン数を返す。
/// - `IsDead X` / `IsAlive X` 述語 → 2
/// - `a op b` 比較 → 3
/// - それ以外 (変数 / 値 / 関数呼出 `Func(...)` / 括弧式 `(...)`) → 1
pub(super) fn cond_term_len(xargs: &[String], k: usize) -> usize {
    match xargs.get(k).map(String::as_str) {
        Some(t) if t.eq_ignore_ascii_case("isdead") || t.eq_ignore_ascii_case("isalive") => 2,
        Some(_) => {
            if xargs.len() > k + 1 && is_comparison_op(&xargs[k + 1]) {
                3
            } else {
                1
            }
        }
        None => 0,
    }
}

/// `If` 命令の引数列を `(条件トークン, 本体トークン)` に分割する。
///
/// 検出規則:
/// - `Then` トークン (case-insensitive) があれば、それを境界とする。
/// - `Then` 無し: 条件式を term 単位で走査する。term は単一トークン
///   (変数 / 値 / 関数呼出 / 括弧式)、`IsDead/IsAlive arg` の 2 トークン、
///   `a op b` 比較の 3 トークンのいずれか。`And` / `Or` で連結された term は
///   繋げて条件に含める。残り (もしあれば) が本体。
///
/// 本体が空 → 複数行 If（`EndIf` を期待）。
/// 本体が非空 → 単一行 If（条件真なら本体を実行、偽なら次行へ）。
///
/// `expand_vars` 後は関数呼出が値に潰れて構造が失われるため、呼び出し側は
/// **展開前の args** を渡してトークン境界を判定する。
pub(super) fn split_if_cond_body(xargs: &[String]) -> (Vec<String>, Vec<String>) {
    if let Some(p) = xargs.iter().position(|t| t.eq_ignore_ascii_case("then")) {
        let cond = xargs[..p].to_vec();
        let body = xargs.get(p + 1..).unwrap_or(&[]).to_vec();
        return (cond, body);
    }
    // `Then` 無し。先頭の `Not` は条件の一部として扱う。
    let has_not = xargs
        .first()
        .map(|s| s.eq_ignore_ascii_case("not"))
        .unwrap_or(false);
    let mut k = if has_not { 1 } else { 0 };
    k += cond_term_len(xargs, k);
    while xargs
        .get(k)
        .map(|s| s.eq_ignore_ascii_case("and") || s.eq_ignore_ascii_case("or"))
        .unwrap_or(false)
    {
        k += 1; // And / Or 演算子
        k += cond_term_len(xargs, k);
    }
    let cond_len = k.min(xargs.len());
    let cond = xargs[..cond_len].to_vec();
    let body = xargs.get(cond_len..).unwrap_or(&[]).to_vec();
    (cond, body)
}

/// `If` コマンドが **ブロック** (`If cond Then` で次行以降が本体、`EndIf` で閉じる)
/// を開くか判定する。単一行 If (`If cond Goto label` / `If cond Then stmt` /
/// `If cond stmt`) は本体が同一行にあり `EndIf` を持たないため `false`。
///
/// `skip_to_else_or_endif` / `skip_to_endif` の深さ計数で単一行 If を誤って 1 段と
/// 数えると、対応しない `EndIf` を探して外側の `EndIf` を食い潰し
/// 「If に対応する EndIf が見つかりません」エラーになる (スパロボ戦記 AlphaSecond の
/// `If 設定[召喚制限] = 召喚制限あり Goto 召喚確定` を含むブロックで発生)。
pub(super) fn if_opens_block(args: &[String]) -> bool {
    split_if_cond_body(args).1.is_empty()
}

/// `If` 偽分岐: 同レベルの `ElseIf` / `Else` / `EndIf` まで読み飛ばす。
///
/// - 一致する `EndIf` → その次の PC を返す（If 文を抜ける）
/// - 同レベル `Else` → その次の PC を返す（Else 節を実行）
/// - 同レベル `ElseIf` → 条件評価し、真ならその次の PC、偽なら更に次の分岐へ
pub(super) fn skip_to_else_or_endif(
    app: &mut App,
    pc: usize,
    stmts: &[EventStatement],
    line: usize,
) -> Result<usize, ScriptError> {
    let mut depth = 0usize;
    let mut i = pc + 1;
    while i < stmts.len() {
        if let EventStatement::Command {
            name,
            args,
            raw_args,
            ..
        } = &stmts[i]
        {
            let l = name.to_ascii_lowercase();
            match l.as_str() {
                // 単一行 If (`If cond Goto ...` 等) は EndIf を持たないので数えない。
                "if" if if_opens_block(args) => depth += 1,
                "if" => {}
                "endif" if depth == 0 => return Ok(i + 1),
                "endif" => depth -= 1,
                "elseif" if depth == 0 => {
                    // `If` と同じく原文トークンの条件部を統一評価器で評価する。
                    let raw = if raw_args.len() == args.len() {
                        raw_args
                    } else {
                        args
                    };
                    let cond_len = split_if_cond_body(args).0.len();
                    let cond_text: String = raw[..cond_len]
                        .iter()
                        .map(|a| preprocess_user_functions(app, a))
                        .collect::<Vec<_>>()
                        .join(" ");
                    if condition_with_calls(app, &cond_text) {
                        return Ok(i + 1);
                    }
                    // ElseIf 偽: 引き続き走査
                }
                "else" if depth == 0 => return Ok(i + 1),
                _ => {}
            }
        }
        i += 1;
    }
    Err(err(line, "If に対応する EndIf が見つかりません。"))
}

/// `ElseIf` / `Else` セクション末尾から `EndIf` までスキップ。
pub(super) fn skip_to_endif(
    pc: usize,
    stmts: &[EventStatement],
    line: usize,
) -> Result<usize, ScriptError> {
    let mut depth = 0usize;
    let mut i = pc + 1;
    while i < stmts.len() {
        if let EventStatement::Command { name, args, .. } = &stmts[i] {
            let l = name.to_ascii_lowercase();
            match l.as_str() {
                // 単一行 If (`If cond Goto ...` 等) は EndIf を持たないので数えない。
                "if" if if_opens_block(args) => depth += 1,
                "if" => {}
                "endif" if depth == 0 => return Ok(i + 1),
                "endif" => depth -= 1,
                _ => {}
            }
        }
        i += 1;
    }
    Err(err(line, "対応する EndIf が見つかりません。"))
}

/// `Talk` ブロックの本文を収集する。終端条件:
///
/// - `End`     → 消費して終了 (next_pc = i+1)。Talk ブロックを閉じる。
/// - `Suspend` → 消費して終了 (next_pc = i+1)。SRC 仕様ではメッセージウィンドウを
///   残したまま続行するが、本実装では End と同等に扱う。
///   Talk-context の Suspend を「トップレベル Suspend (タイトル復帰)」
///   として実行してしまうことを防ぐため、ここで消費する。
/// - 内部 `Talk` → 消費しないで終了 (next_pc = i)。次の話者に切り替わる。
///   メインループが新しい Talk コマンドとして実行する。
///   `WTalk` は SRC 組み込みコマンドではなく（`WTalk.eve` 等のユーザー定義
///   ライブラリ呼び出し）のため、ここではターミネータとして扱わない。
/// - EOF       → そのまま終了。
pub(super) fn collect_until_end(
    app: &App,
    start: usize,
    stmts: &[EventStatement],
) -> (String, usize) {
    let mut body = String::new();
    let mut i = start;
    while i < stmts.len() {
        if let EventStatement::Command {
            name,
            args,
            raw_args,
            ..
        } = &stmts[i]
        {
            if name.eq_ignore_ascii_case("end") {
                return (body.trim().to_string(), i + 1);
            }
            // Suspend は Talk ブロック終端として消費する。
            // トップレベルの Suspend (タイトル復帰) が Talk ボディに混入するのを防ぐ。
            if name.eq_ignore_ascii_case("suspend") {
                return (body.trim().to_string(), i + 1);
            }
            // 内部 Talk は消費しない。メインループが次の Talk として処理する。
            // WTalk は SRC 組み込みコマンドではなく「WTalk.eve」等のユーザー定義
            // ライブラリ呼び出しであるため、ここではターミネータとして扱わない。
            // Talk ブロック本文中に `Wtalk BM "..."` が現れてもブロックを
            // 打ち切らず、本文の一部として展開する。
            if name.eq_ignore_ascii_case("talk") {
                return (body.trim().to_string(), i);
            }
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&message_line(app, name, args, raw_args));
        }
        i += 1;
    }
    (body.trim().to_string(), i)
}

/// `Talk` 本文 / `Ask` 選択肢の 1 行を組み立てる (原典 `FormatMessage`: 行の原文に
/// `$(...)` の展開だけを行う。関数呼び出しや配列参照の形をした文字列はそのまま)。
fn message_line(app: &App, name: &str, args: &[String], raw_args: &[String]) -> String {
    let raw = if raw_args.len() == args.len() {
        raw_args
    } else {
        args
    };
    let mut line = name.to_string();
    for a in raw {
        line.push(' ');
        line.push_str(a);
    }
    eval::replace_sub_expression(app, &line)
}

/// ラベルへジャンプ。`@anchor` / `label:` どちらも `collect_labels` で正規化済み。
/// `target` ラベルを **現 PC を含むファイル内優先** で解決して飛び先 PC を返す。
pub(super) fn jump_to(
    app: &App,
    pc: usize,
    target: &str,
    line: usize,
) -> Result<usize, ScriptError> {
    // 末尾 `:` / 先頭 `@` を取り除いた名前で検索
    let t = target.trim_start_matches('@').trim_end_matches(':');
    app.script_library()
        .label_pc_scoped(pc, t)
        .ok_or_else(|| err(line, &format!("Goto 先ラベルが見つかりません: {target:?}")))
}

/// `Switch` から見て、ネスト深度 0 で `Case "v"` / `CaseElse` / `EndSw` を探索。
/// `Case` 値と `value` が一致したらその次の PC を返す。
/// 一致するものが無く `CaseElse` が見つかればそこへ。最終的に `EndSw` まで来れば
/// その次の PC を返す（マッチ無しで Switch を抜ける）。
pub(super) fn find_matching_case(
    pc: usize,
    stmts: &[EventStatement],
    value: &str,
    line: usize,
) -> Result<usize, ScriptError> {
    let mut depth = 0usize;
    let mut i = pc + 1;
    while i < stmts.len() {
        if let EventStatement::Command { name, args, .. } = &stmts[i] {
            let l = name.to_ascii_lowercase();
            match l.as_str() {
                "switch" => depth += 1,
                "endsw" if depth == 0 => return Ok(i + 1),
                "endsw" => depth -= 1,
                "case" if depth == 0 => {
                    // `Case 1 2 3` (空白区切り) と `Case 1, 2, 3` (カンマ区切り)
                    // の両形式を許容する。各値ごとに比較し、いずれか一致で採用。
                    // 範囲 (`A To B`) / `Is op N` は複数 token を消費するため、
                    // まずは joined で評価し、それでもマッチしなければ各 arg を
                    // 単独で比較する。
                    let case_v = args.join(" ");
                    if case_value_matches(&case_v, value) {
                        return Ok(i + 1);
                    }
                    if args.iter().any(|a| case_value_matches(a, value)) {
                        return Ok(i + 1);
                    }
                }
                "caseelse" if depth == 0 => return Ok(i + 1),
                _ => {}
            }
        }
        i += 1;
    }
    Err(err(line, "Switch に対応する EndSw が見つかりません。"))
}

/// `Case` 値と Switch 値のマッチ。受理する書式:
/// - `Case "v1"` / `Case v1` — 単純等値
/// - `Case v1, v2, v3` — カンマ区切り or-リスト
/// - `Case Is < 5` / `Case Is >= 10` 等 — 比較演算
/// - `Case 1 To 10` — 数値範囲（両端含む）
pub(super) fn case_value_matches(case_v: &str, value: &str) -> bool {
    let c = case_v.trim();
    if c.contains(',') {
        for part in c.split(',') {
            if case_part_matches(part.trim(), value) {
                return true;
            }
        }
        false
    } else {
        case_part_matches(c, value)
    }
}

/// `Case` の 1 パートと値の比較。
pub(super) fn case_part_matches(part: &str, value: &str) -> bool {
    let p = part.trim().trim_matches('"').trim();
    // 1) "Is op N" — 比較
    if let Some(rest) = p.strip_prefix("Is ").or_else(|| p.strip_prefix("is ")) {
        let rest = rest.trim();
        // op の長さ順 (`<=` `>=` `<>` `!=` を `<` `>` より先に判定)
        for op in ["<=", ">=", "<>", "!=", "<", ">", "=", "=="] {
            if let Some(rhs) = rest.strip_prefix(op) {
                let rhs = rhs.trim().trim_matches('"').trim();
                return eval_binop(value, op, rhs);
            }
        }
    }
    // 2) "A To B" — 範囲
    if let Some((a, b)) = split_to_range(p) {
        if let (Ok(v), Ok(lo), Ok(hi)) = (value.parse::<f64>(), a.parse::<f64>(), b.parse::<f64>())
        {
            return v >= lo && v <= hi;
        }
    }
    // 3) 単純等値（数値化可能なら数値比較で）
    if let (Ok(a), Ok(b)) = (p.parse::<f64>(), value.parse::<f64>()) {
        return (a - b).abs() < f64::EPSILON;
    }
    p == value
}

pub(super) fn split_to_range(s: &str) -> Option<(&str, &str)> {
    // `A To B` / `A to B` を見つける（大文字小文字混在対応の手書き）。
    // `to_lowercase()` で得たバイト位置を元の文字列に再利用すると、
    // 一部の Unicode 文字 (例: `İ` → `i\u{307}`, `ß` → `ss`) で
    // 大文字小文字変換時にバイト長が変わって char 境界を割るため、
    // 元の文字列を `char_indices` で走査して直接 " to " / " To " を探す。
    let bytes = s.as_bytes();
    let needle_len = 4; // " to " 固定 (ASCII 4 byte)
    let mut i = 0;
    while i + needle_len <= bytes.len() {
        let window = &bytes[i..i + needle_len];
        if (window[0] == b' ')
            && (window[1] == b't' || window[1] == b'T')
            && (window[2] == b'o' || window[2] == b'O')
            && (window[3] == b' ')
        {
            // ASCII の検出位置なので元の str を safe にスライス可能。
            let a = &s[..i];
            let b = &s[i + needle_len..];
            return Some((a.trim(), b.trim()));
        }
        i += 1;
    }
    None
}

/// 同階層の `EndSw` までスキップ。
pub(super) fn skip_to_endsw(
    pc: usize,
    stmts: &[EventStatement],
    line: usize,
) -> Result<usize, ScriptError> {
    let mut depth = 0usize;
    let mut i = pc + 1;
    while i < stmts.len() {
        if let EventStatement::Command { name, .. } = &stmts[i] {
            let l = name.to_ascii_lowercase();
            match l.as_str() {
                "switch" => depth += 1,
                "endsw" if depth == 0 => return Ok(i + 1),
                "endsw" => depth -= 1,
                _ => {}
            }
        }
        i += 1;
    }
    Err(err(line, "対応する EndSw が見つかりません。"))
}

/// `ForEach group [status]` (書式1) の status 指定を解釈する。
/// 戻り値 `(出撃ユニットを含む, 非出撃ユニットを含む)`。
///
/// SRC の status は `出撃` / `待機` / `格納` / `破壊` / `離脱` / `全て` の 6 種で、
/// `(出撃 待機)` のように括弧で複数指定可。省略時の既定は `出撃 格納`。
/// 本実装のユニットは off_map false=出撃 / true=待機・格納・離脱 の 2 状態に
/// 縮約され、破壊ユニットは `unit_instances` から既に除去されている。
pub(super) fn foreach_status_mask(status_args: &[String]) -> (bool, bool) {
    let mut tokens: Vec<String> = Vec::new();
    for a in status_args {
        let s = a.trim().trim_matches(['(', ')']);
        tokens.extend(s.split_whitespace().map(str::to_string));
    }
    if tokens.is_empty() {
        // 既定: 出撃 格納 → 本実装では出撃 + 非出撃 = 全ユニット。
        return (true, true);
    }
    let mut deployed = false;
    let mut off_map = false;
    for t in &tokens {
        match t.as_str() {
            "出撃" => deployed = true,
            "待機" | "格納" | "離脱" => off_map = true,
            "全て" | "全" => {
                deployed = true;
                off_map = true;
            }
            // `破壊` ユニットは保持しないため対象 0 件。
            _ => {}
        }
    }
    (deployed, off_map)
}

/// `ForEach group` (書式1) の 1 反復ぶん: 識別子 `ident` のユニットを
/// 「対象」としてシステム変数 `対象ユニットＩＤ` / `対象パイロット` に束縛する。
/// `Create` が設定するのと同じ変数なので、後続の `Pilot(対象ユニットＩＤ)` /
/// `Skill(対象パイロット, …)` 等がそのまま現在のユニットを参照できる。
pub(super) fn bind_foreach_unit(app: &mut App, ident: &str) {
    let (uid, pilot) = app
        .database()
        .unit_instances
        .iter()
        .find(|u| matches_unit_handle(u, ident))
        .map(|u| (u.uid.clone(), u.pilot_name.clone()))
        .unwrap_or_default();
    // uid 未採番のユニット (Place 由来) は識別子そのものを ID 扱いする。
    let uid = if uid.is_empty() {
        ident.to_string()
    } else {
        uid
    };
    // SRC `ForEachCmd` は各反復で `Event.SelectedUnitForEvent` を当該ユニットに
    // 設定する。これにより ループ内の引数省略コマンド (`Escape` / `Finish` 等) が
    // 現在のループ対象に作用する (`ForEach 敵 / Escape / Next` で全敵退避 等)。
    app.set_selected_unit_for_event(uid.clone());
    app.set_script_var("対象ユニットＩＤ".to_string(), uid);
    app.set_script_var("対象パイロット".to_string(), pilot);
}

/// `パイロット一覧(mode)` / `ユニット一覧(mode)` (ForEach 書式3/4) を解決し、
/// 搭乗中パイロット名 / ユニット名の一覧を返す。`mode` が `レベル` を含むときは
/// レベル降順、それ以外は `unit_instances` の登録順。
///
/// SRC の `mode` には多数の整列基準があるが、本実装はレベルのみ厳密に扱い、
/// 残りは登録順にフォールバックする (反復対象の集合自体は同じ)。
pub(super) fn collect_roster(app: &App, mode: &str, pilots: bool) -> Vec<String> {
    let by_level = mode.contains("レベル") || mode.contains("Level");
    let mut seen = std::collections::HashSet::new();
    let mut rows: Vec<(String, i64)> = Vec::new();
    for u in &app.database().unit_instances {
        let key = if pilots {
            u.pilot_name.clone()
        } else {
            u.unit_data_name.clone()
        };
        if key.is_empty() || !seen.insert(key.clone()) {
            continue;
        }
        rows.push((
            key,
            i64::from(crate::pilot_instance::level_from_exp(u.total_exp)),
        ));
    }
    if by_level {
        rows.sort_by_key(|(_, level)| std::cmp::Reverse(*level));
    }
    rows.into_iter().map(|(k, _)| k).collect()
}

/// `ForEach` の collection 表現を解釈してリスト化:
/// - `"Player"` / `"味方"` / `"Enemy"` 等の勢力ラベル → 該当勢力ユニット名一覧
/// - `"all"` / `"全"` → 全ユニット名一覧
/// - `パイロット一覧(mode)` / `ユニット一覧(mode)` → 搭乗中パイロット/ユニット一覧
/// - インデックス変数名 (`name[N]` の形で複数 Set 済み) → 1..N の数値文字列
/// - カンマ区切りリテラル → 各要素
pub(super) fn collect_foreach_items(app: &App, collection: &str) -> Vec<String> {
    let s = collection.trim().trim_matches('"').trim();
    // ForEach 書式3/4: `パイロット一覧(mode)` / `ユニット一覧(mode)`。
    // `パ`/`ユ` は非 ASCII なので `take_function_call` では関数扱いされず、
    // ここでリテラル文字列として判定する。
    for (prefix, is_pilot) in [("パイロット一覧(", true), ("ユニット一覧(", false)] {
        if let Some(rest) = s.strip_prefix(prefix) {
            let mode = rest.strip_suffix(')').unwrap_or(rest).trim();
            return collect_roster(app, mode, is_pilot);
        }
    }
    if let Some(party) = parse_party_label(s) {
        return app
            .database()
            .unit_instances
            .iter()
            .filter(|u| u.party == party)
            .map(|u| u.unit_data_name.clone())
            .collect();
    }
    if s == "all" || s == "全" || s == "All" {
        return app
            .database()
            .unit_instances
            .iter()
            .map(|u| u.unit_data_name.clone())
            .collect();
    }
    // インデックス変数: `name[key]` キーが存在すれば、その key を列挙して返す。
    // 数値キーは数値順、文字列キーはアルファベット順 (script_vars は BTreeMap で
    // 順序保持されているのでそのまま走査)。
    let prefix = format!("{s}[");
    let mut number_keys: Vec<(i64, String)> = Vec::new();
    let mut string_keys: Vec<String> = Vec::new();
    for k in app.script_vars().keys() {
        if !k.starts_with(&prefix) || !k.ends_with(']') {
            continue;
        }
        let mid = &k[prefix.len()..k.len() - 1];
        if let Ok(n) = mid.parse::<i64>() {
            number_keys.push((n, mid.to_string()));
        } else {
            string_keys.push(mid.to_string());
        }
    }
    if !number_keys.is_empty() || !string_keys.is_empty() {
        let mut out: Vec<String> = Vec::new();
        number_keys.sort_by_key(|(n, _)| *n);
        out.extend(number_keys.into_iter().map(|(_, k)| k));
        out.extend(string_keys);
        return out;
    }
    // コンマを含まない場合は変数参照として解決する。
    // 未定義 / 空 → 0 回反復 (SRC: 未定義配列の ForEach はスキップ)。
    // 定義済みスカラーは空白区切りのリストと解釈。
    if !s.contains(',') {
        let v = app.script_var(s).to_string();
        if v.is_empty() {
            return Vec::new();
        }
        return v
            .split_whitespace()
            .filter(|p| !p.is_empty())
            .map(|p| p.to_string())
            .collect();
    }
    s.split(',')
        .map(|p| p.trim().trim_matches('"').trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

/// `open` の対応する `close` まで前方スキップ。Do/Loop の Skip 用。
pub(super) fn skip_to_matching(
    open: &str,
    close: &str,
    pc: usize,
    stmts: &[EventStatement],
    line: usize,
) -> Result<usize, ScriptError> {
    let mut depth = 0i32;
    let mut i = pc + 1;
    while i < stmts.len() {
        if let EventStatement::Command { name, .. } = &stmts[i] {
            let l = name.to_ascii_lowercase();
            if l == open {
                depth += 1;
            } else if l == close {
                if depth == 0 {
                    return Ok(i + 1);
                }
                depth -= 1;
            }
        }
        i += 1;
    }
    Err(err(line, &format!("対応する {close} が見つかりません。")))
}

/// `Break` 用: For/Next または Do/Loop のうち、現在の最内ループから抜ける。
/// 最内ループの終端を見つけるため、`for`/`do` をネストカウンタで追って
/// 最初に `next` / `loop` が depth 0 で現れた位置の次を返す。
pub(super) fn skip_to_loop_or_next_end(
    pc: usize,
    stmts: &[EventStatement],
    line: usize,
) -> Result<usize, ScriptError> {
    let mut depth_for = 0i32;
    let mut depth_do = 0i32;
    let mut i = pc + 1;
    while i < stmts.len() {
        if let EventStatement::Command { name, .. } = &stmts[i] {
            let l = name.to_ascii_lowercase();
            match l.as_str() {
                "for" | "foreach" => depth_for += 1,
                "do" => depth_do += 1,
                "next" if depth_for == 0 => return Ok(i + 1),
                "next" => depth_for -= 1,
                "loop" if depth_do == 0 => return Ok(i + 1),
                "loop" => depth_do -= 1,
                _ => {}
            }
        }
        i += 1;
    }
    Err(err(line, "Break: 対応する Loop / Next が見つかりません。"))
}

/// `pc` から後方に `open` / `close` キーワードでネスト深度を追跡し、
/// 直近の `open` 命令の位置を返す。
pub(super) fn find_back(
    open: &str,
    close: &str,
    pc: usize,
    stmts: &[EventStatement],
    line: usize,
) -> Result<usize, ScriptError> {
    let mut depth = 0i32;
    if pc == 0 {
        return Err(err(line, &format!("対応する {open} が見つかりません。")));
    }
    let mut i = pc - 1;
    loop {
        if let EventStatement::Command { name, .. } = &stmts[i] {
            let l = name.to_ascii_lowercase();
            if l == close {
                depth += 1;
            } else if l == open {
                if depth == 0 {
                    return Ok(i);
                }
                depth -= 1;
            }
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    Err(err(line, &format!("対応する {open} が見つかりません。")))
}

/// `pc` から `open` / `close` キーワードでネスト深度を追跡しながら
/// 直近の `open` 命令まで後方ジャンプ。`Loop` → 対応する `Do` 用。
/// 戻り値: 一致した `open` 命令の次の PC（ループボディの先頭）。
#[allow(dead_code)]
pub(super) fn jump_back_to(
    pc: usize,
    stmts: &[EventStatement],
    line: usize,
    open: &str,
    close: &str,
) -> Result<usize, ScriptError> {
    let mut depth = 0i32;
    if pc == 0 {
        return Err(err(line, &format!("対応する {open} が見つかりません。")));
    }
    let mut i = pc - 1;
    loop {
        if let EventStatement::Command { name, .. } = &stmts[i] {
            let l = name.to_ascii_lowercase();
            if l == close {
                depth += 1;
            } else if l == open {
                if depth == 0 {
                    return Ok(i + 1);
                }
                depth -= 1;
            }
        }
        if i == 0 {
            break;
        }
        i -= 1;
    }
    Err(err(line, &format!("対応する {open} が見つかりません。")))
}

/// 同階層の `Next` までスキップ（For 偽分岐用）。
pub(super) fn skip_to_next(
    pc: usize,
    stmts: &[EventStatement],
    line: usize,
) -> Result<usize, ScriptError> {
    let mut depth = 0usize;
    let mut i = pc + 1;
    while i < stmts.len() {
        if let EventStatement::Command { name, .. } = &stmts[i] {
            let l = name.to_ascii_lowercase();
            match l.as_str() {
                "for" => depth += 1,
                "next" if depth == 0 => return Ok(i + 1),
                "next" => depth -= 1,
                _ => {}
            }
        }
        i += 1;
    }
    Err(err(line, "対応する Next が見つかりません。"))
}
