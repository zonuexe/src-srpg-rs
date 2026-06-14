//! SRC イベントスクリプト (.eve) の最小パーサ / Minimal `.eve` parser.
//!
//! 元 SRC は `Event.bas::LoadEventData2` (`Event.bas:1284`〜) で
//! `.eve` ファイルを 1 行ずつ読み込み、後段の `HandleEvent` で実行する。
//! ここでは構文解析のみを行い、各行を `EventStatement` として返す。
//! 実行は後続フェーズで `app` / `database` を巻き込んで実装する想定。
//!
//! 受理する 1 行の形式:
//!
//! - 空行 / `#` 始まり / `//` 以降末尾   → 出力に含めない
//! - `<filename.eve>`                  → `Include(filename)`
//! - それ以外                           → `Command { name, args }`
//!
//! トークナイザは空白セパレータ、`"..."` で囲まれた値を 1 トークンとして読む
//! (SRC `.eve` の流儀)。

use super::loader::{read_lines, SourceLine};
use super::pilot::ParseError;

/// 解析後の 1 ステートメント / One parsed statement.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum EventStatement {
    /// `<other.eve>` 形式の include 指示。
    Include { path: String, line_num: usize },
    /// 通常の命令行（先頭が command 名、残りが引数）。
    Command {
        name: String,
        args: Vec<String>,
        line_num: usize,
    },
}

impl EventStatement {
    pub const fn line_num(&self) -> usize {
        match self {
            Self::Include { line_num, .. } | Self::Command { line_num, .. } => *line_num,
        }
    }
}

/// `.eve` テキスト全体を解析。
pub fn parse(src: &str) -> Result<Vec<EventStatement>, ParseError> {
    let lines = merge_line_continuations(read_lines(src));
    let mut out = Vec::new();
    for line in lines {
        if line.text.is_empty() {
            continue;
        }
        if let Some(p) = parse_include(&line) {
            out.push(p);
            continue;
        }
        let toks = tokenize(&line.text, line.line_num)?;
        if toks.is_empty() {
            continue;
        }
        let mut it = toks.into_iter();
        let name = it.next().unwrap();
        let args: Vec<String> = it.collect();
        out.push(EventStatement::Command {
            name,
            args,
            line_num: line.line_num,
        });
    }
    Ok(out)
}

/// VB6 流の行継続 (` _` = 空白 + アンダースコアで行末) を結合する。
///
/// スパロボ戦記の `String.eve` / `Score.eve` は長い `PaintString` 等を
/// ` _` で複数行に分けて書く。これを結合しないと行が途中で切れて
/// 壊れた式が画面に描画される (`(SCX() ＋22 ((i-5)) ...` 等)。
///
/// 継続マーカー判定:
/// 1. `text` が `_` で終わり、かつ直前が空白 (または `_` 単独行)。
///    識別子末尾の `_` (例: `foo_`) は継続扱いしない。
/// 2. 上記に該当しなくても、行内のクオート (`"`) 個数が奇数 (文字列が
///    開きっぱなし) で `_` 末尾なら継続。`X-Story` 等で長台詞を
///    `";_` のように直前に空白なしで継続するケースが実在する。
fn merge_line_continuations(lines: Vec<SourceLine>) -> Vec<SourceLine> {
    fn inside_unclosed_quote(text: &str) -> bool {
        text.bytes().filter(|&b| b == b'"').count() % 2 == 1
    }
    fn is_continuation(text: &str) -> bool {
        if !text.ends_with('_') {
            return false;
        }
        let before = &text[..text.len() - 1];
        if before.is_empty() || before.ends_with(char::is_whitespace) {
            return true;
        }
        inside_unclosed_quote(text)
    }
    /// 末尾の継続マーカー `_` と直前の空白を除去。
    fn strip_marker(text: &str) -> String {
        text[..text.len() - 1].trim_end().to_string()
    }
    let mut out: Vec<SourceLine> = Vec::new();
    let mut pending: Option<SourceLine> = None;
    for line in lines {
        if let Some(mut p) = pending.take() {
            let cont = is_continuation(&line.text);
            p.text.push(' ');
            p.text.push_str(&line.text);
            if cont {
                p.text = strip_marker(&p.text);
                pending = Some(p);
            } else {
                out.push(p);
            }
        } else if is_continuation(&line.text) {
            pending = Some(SourceLine {
                line_num: line.line_num,
                text: strip_marker(&line.text),
            });
        } else {
            out.push(line);
        }
    }
    if let Some(p) = pending {
        out.push(p);
    }
    out
}

fn parse_include(line: &SourceLine) -> Option<EventStatement> {
    let t = &line.text;
    if t.starts_with('<') && t.ends_with('>') && t.len() >= 2 {
        let inner = &t[1..t.len() - 1];
        if !inner.is_empty() && inner != ">" {
            return Some(EventStatement::Include {
                path: inner.to_string(),
                line_num: line.line_num,
            });
        }
    }
    None
}

fn tokenize(text: &str, line_num: usize) -> Result<Vec<String>, ParseError> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // 外側 (クオート外) の空白をスキップ
        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t') {
            i += 1;
        }
        if i >= bytes.len() {
            break;
        }

        // 1 トークン読込:
        // - `"` をトグルしてクオート内のスペースを保持
        // - `(...)` 内のスペース / カンマもトークン分割に使わない（関数記法）
        // - `[...]` 内のスペースも同様。`Set 配列[Count(x) + 1] 値` のように
        //   インデックス式が空白を含むケースを 1 トークンに保つ。
        // 純粋に `"..."` でくくられた値はクオートを剥がして返す。
        let begin = i;
        let mut in_quote = false;
        // SRC は Asc 96 (`` ` ``) を `"` を内包できる代替クオート境界として扱う
        // (`GeneralLib.ListSplit`)。`` `"` `` のような「バッククオートで囲った
        // ダブルクオート文字」を 1 トークンとして読むため両方を追跡する。
        let mut in_backtick = false;
        let mut paren_depth: i32 = 0;
        let mut bracket_depth: i32 = 0;
        while i < bytes.len() {
            let b = bytes[i];
            if b == b'"' && !in_backtick {
                in_quote = !in_quote;
                i += 1;
                continue;
            }
            if b == b'`' && !in_quote {
                in_backtick = !in_backtick;
                i += 1;
                continue;
            }
            if !in_quote && !in_backtick {
                if b == b'(' {
                    paren_depth += 1;
                    i += 1;
                    continue;
                }
                if b == b')' {
                    if paren_depth > 0 {
                        paren_depth -= 1;
                    }
                    i += 1;
                    continue;
                }
                if b == b'[' {
                    bracket_depth += 1;
                    i += 1;
                    continue;
                }
                if b == b']' {
                    if bracket_depth > 0 {
                        bracket_depth -= 1;
                    }
                    i += 1;
                    continue;
                }
                if paren_depth == 0 && bracket_depth == 0 && matches!(b, b' ' | b'\t') {
                    break;
                }
            }
            i += 1;
        }
        // 行末で未終了クオート / バッククオートに到達しても **エラーにしない**。
        // SRC.NET `GeneralLib.ListSplit` (GeneralLib.cs:805-816) は文字列終端で
        // 未終了クオートに遭遇すると、開始位置から行末までを最後の 1 トークンと
        // して確定し、戻り値 -1 を返すのみでクラッシュしない。`tokenize` は (行継続
        // 結合後の) 1 論理行単位で呼ばれるため、未終了クオートが後続行へ波及する
        // ことはない。実シナリオには行末の余分な `"` や閉じ忘れた台詞クオートが
        // 散見される (例: ASTOR151 の各話 .eve 末尾の孤立 `"`) ため、SRC と同じ
        // 寛容さでそのままトークン化する。`line_num` は将来の警告用に残す。
        let _ = (in_quote, in_backtick, line_num);
        let raw = &text[begin..i];
        let pushed = if raw.len() >= 2
            && ((raw.starts_with('"') && raw.ends_with('"'))
                || (raw.starts_with('`') && raw.ends_with('`')))
        {
            raw[1..raw.len() - 1].to_string()
        } else {
            raw.to_string()
        };
        out.push(pushed);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_continuation_merges() {
        // VB6 流 ` _` 行継続。String.eve VibrationString と同型。
        let src = "\
PaintString (10 + 5) (20 - 3 + _
            40) hello
Message normal
";
        let stmts = parse(src).unwrap();
        assert_eq!(stmts.len(), 2);
        match &stmts[0] {
            EventStatement::Command { name, args, .. } => {
                assert_eq!(name, "PaintString");
                // 2 行が結合され `(20 - 3 + 40)` が 1 トークンになる
                assert_eq!(
                    args,
                    &vec![
                        "(10 + 5)".to_string(),
                        "(20 - 3 + 40)".to_string(),
                        "hello".to_string(),
                    ]
                );
            }
            other => panic!("想定外: {other:?}"),
        }
    }

    #[test]
    fn bracket_index_with_spaces_stays_one_token() {
        // `配列[式]` のインデックス式が空白を含んでも 1 トークンに保つ。
        // ブラケット深度を追わないと `Set 配列[Count(x) + 1] v` が
        // `配列[Count(x)` / `+` / `1]` / `v` に割れて Set LHS が壊れる。
        let src = "Set ヘルプ選択肢[Count(ヘルプ選択肢) + 1] 戦闘関連\n";
        let stmts = parse(src).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            EventStatement::Command { name, args, .. } => {
                assert_eq!(name, "Set");
                assert_eq!(
                    args,
                    &vec![
                        "ヘルプ選択肢[Count(ヘルプ選択肢) + 1]".to_string(),
                        "戦闘関連".to_string(),
                    ]
                );
            }
            other => panic!("想定外: {other:?}"),
        }
    }

    #[test]
    fn underscore_in_identifier_not_continuation() {
        // 識別子末尾の `_` は行継続ではない。
        let src = "Set foo_ 1\nMessage x\n";
        let stmts = parse(src).unwrap();
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn unclosed_quote_with_underscore_continues() {
        // 文字列の途中で `_` 改行で次行へ続くケース (`X-Story`/`Ori-fan` 実例)。
        // 直前が空白でなくても、`"` が奇数個ある (=未閉じ) なら継続として扱う。
        let src = "Wtalk BM _\n\"前半;_\n後半\"\n";
        let stmts = parse(src).unwrap();
        assert_eq!(stmts.len(), 1);
        match &stmts[0] {
            EventStatement::Command { name, args, .. } => {
                assert_eq!(name, "Wtalk");
                assert_eq!(args.len(), 2);
                assert_eq!(args[0], "BM");
                // クオート内に行継続マーカーがあっても 1 つの文字列トークンに統合。
                assert!(args[1].contains("前半;"), "args[1] = {:?}", args[1]);
                assert!(args[1].contains("後半"), "args[1] = {:?}", args[1]);
            }
            other => panic!("想定外: {other:?}"),
        }
    }

    #[test]
    fn skip_blank_and_comment_lines() {
        let src = "\
# heading comment

Stage \"開始ステージ\"
// trailing
Message \"hello\"  // 末尾コメント
";
        let stmts = parse(src).unwrap();
        assert_eq!(stmts.len(), 2);
        match &stmts[0] {
            EventStatement::Command { name, args, .. } => {
                assert_eq!(name, "Stage");
                assert_eq!(args, &vec!["開始ステージ".to_string()]);
            }
            _ => panic!("not Command"),
        }
        match &stmts[1] {
            EventStatement::Command { name, args, .. } => {
                assert_eq!(name, "Message");
                assert_eq!(args, &vec!["hello".to_string()]);
            }
            _ => panic!("not Command"),
        }
    }

    #[test]
    fn include_directive_parsed() {
        let src = "<library.eve>\nMessage \"x\"\n";
        let stmts = parse(src).unwrap();
        assert_eq!(stmts.len(), 2);
        match &stmts[0] {
            EventStatement::Include { path, .. } => assert_eq!(path, "library.eve"),
            _ => panic!("not Include"),
        }
    }

    #[test]
    fn multiple_args_with_quoted_string() {
        let src = "Pilot \"リオ・カザミ\" リオ 男性 超能力者 AAAA 100\n";
        let stmts = parse(src).unwrap();
        match &stmts[0] {
            EventStatement::Command { name, args, .. } => {
                assert_eq!(name, "Pilot");
                assert_eq!(args.len(), 6);
                assert_eq!(args[0], "リオ・カザミ");
                assert_eq!(args[1], "リオ");
                assert_eq!(args[5], "100");
            }
            _ => panic!("not Command"),
        }
    }

    #[test]
    fn unterminated_quote_is_tolerated() {
        // SRC.NET `GeneralLib.ListSplit` は未終了クオートでもエラーにせず、
        // 行末までを 1 トークンとして確定する。実シナリオ末尾の孤立 `"` や
        // 閉じ忘れた台詞でパースが破綻しないこと。
        let src = "Message \"foo\n";
        let stmts = parse(src).unwrap();
        match &stmts[0] {
            EventStatement::Command { name, args, .. } => {
                assert_eq!(name, "Message");
                // 開きクオートは対応がないため剥がされず、残りがそのまま残る。
                assert_eq!(args, &vec!["\"foo".to_string()]);
            }
            _ => panic!("not Command"),
        }
    }

    #[test]
    fn lone_trailing_quote_is_tolerated() {
        // ASTOR151 各話 .eve 末尾に見られる孤立 `"` 1 文字の行。
        let src = "Message \"hi\"\n\"\n";
        let stmts = parse(src).unwrap();
        // 1 行目は正常、2 行目の孤立 `"` も単独トークンとして受理される。
        assert_eq!(stmts.len(), 2);
        match &stmts[1] {
            EventStatement::Command { name, args, .. } => {
                assert_eq!(name, "\"");
                assert!(args.is_empty());
            }
            _ => panic!("not Command"),
        }
    }

    #[test]
    fn backtick_quotes_a_doublequote_char() {
        // SRC は `` ` `` を `"` を内包できる代替クオート境界として扱う。
        // 実シナリオ (Ori-fan ST_Func.eve) の `Case` 行が破綻しないこと。
        let src = "Case \"、\" \"。\" `“` `”` `\"`\n";
        let stmts = parse(src).unwrap();
        match &stmts[0] {
            EventStatement::Command { name, args, .. } => {
                assert_eq!(name, "Case");
                // クオート/バッククオートは剥がされ、中身のみが残る
                assert_eq!(
                    args,
                    &vec![
                        "、".to_string(),
                        "。".to_string(),
                        "“".to_string(),
                        "”".to_string(),
                        "\"".to_string(),
                    ]
                );
            }
            _ => panic!("not Command"),
        }
    }

    #[test]
    fn unterminated_backtick_is_tolerated() {
        // バッククオートも同様に行末までを 1 トークンとして確定する。
        let src = "Case `foo\n";
        let stmts = parse(src).unwrap();
        match &stmts[0] {
            EventStatement::Command { name, args, .. } => {
                assert_eq!(name, "Case");
                assert_eq!(args, &vec!["`foo".to_string()]);
            }
            _ => panic!("not Command"),
        }
    }
}
