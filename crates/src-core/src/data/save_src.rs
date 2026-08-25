//! SRC セーブデータ (`.src`) パーサ / SRC save-data (`.src`) parser.
//!
//! 原典 VB6 `SRC.bas` の 2 系統の書式に対応する。
//!
//! - `SaveData` (通常セーブ, `SRC.bas:1853`) — 「ここから開始」用に配布される
//!   `スタート.src` / `Start.src` 等はこちら。
//!
//!   ```text
//!   <version>            ' 10000*Major + 100*Minor + Revision
//!   <title count>        ' version <= 10000 の旧形式では version 自体が件数
//!   <title>...           ' title count 行
//!   <次ステージ>          ' 開始する .eve のパス (シナリオルート相対)
//!   <TotalTurn> <Money> <dummy>
//!   <global var count>
//!   <name>,<value>...    ' グローバル変数
//!   ```
//!
//! - `DumpData` (中断データ, `SRC.bas:2029`) — version の直後が
//!   `ScenarioFileName` (クオート文字列) である点で通常セーブと区別できる。
//!
//!   ```text
//!   <version>
//!   <ScenarioFileName>   ' 中断時に実行中だった .eve
//!   <title count>
//!   <title>...
//!   <Turn> <TotalTurn> <Money>
//!   ...                  ' 以降 イベント/パイロット/ユニット/マップ/BGM/乱数
//!   ```
//!
//! 本パーサはシナリオ起動に必要なヘッダ部
//! (開始 `.eve` / タイトル / 所持金 / グローバル変数) までを読む。
//! 中断データの部隊・マップ状態の復元は未対応 (`SaveKind::Suspend` で判別可能)。

use serde::{Deserialize, Serialize};

/// `.src` の書式種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SaveKind {
    /// 通常セーブ (`SRC.bas::SaveData`)。
    Save,
    /// 中断データ (`SRC.bas::DumpData`)。
    Suspend,
}

/// `.src` のヘッダ部。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SaveFile {
    pub version: i64,
    pub kind: Option<SaveKindWrap>,
    /// `IncludeData` に渡される追加データタイトル。
    pub titles: Vec<String>,
    /// 開始する `.eve`。通常セーブなら `次ステージ`、中断データなら
    /// `ScenarioFileName`。シナリオルート相対・区切りは `\` のことが多い。
    pub start_eve: String,
    pub turn: i64,
    pub total_turn: i64,
    pub money: i64,
    /// グローバル変数 (`Option(...)` 等を含む)。
    pub global_vars: Vec<(String, String)>,
}

/// `SaveKind` を `Default` 導出可能にするためのラッパ。
pub type SaveKindWrap = SaveKind;

impl SaveFile {
    /// 中断データか。
    pub fn is_suspend(&self) -> bool {
        self.kind == Some(SaveKind::Suspend)
    }
}

/// VB6 `Input #` 相当のトークン列。値はカンマ / 改行区切り、文字列は `"` で
/// 括られ `""` が 1 個の `"` を表す。
fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_quote = false;
    let mut had_quote = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if in_quote {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    cur.push('"');
                    chars.next();
                } else {
                    in_quote = false;
                }
            } else {
                cur.push(c);
            }
            continue;
        }
        match c {
            '"' => {
                in_quote = true;
                had_quote = true;
            }
            ',' | '\n' => {
                let v = if had_quote {
                    cur.clone()
                } else {
                    cur.trim().to_string()
                };
                // 引用符なしの空トークン (CRLF の空行等) は区切りとして捨てる。
                if had_quote || !v.is_empty() {
                    out.push(v);
                }
                cur.clear();
                had_quote = false;
            }
            '\r' => {}
            _ => cur.push(c),
        }
    }
    let v = if had_quote {
        cur
    } else {
        cur.trim().to_string()
    };
    if had_quote || !v.is_empty() {
        out.push(v);
    }
    out
}

fn as_i64(s: &str) -> Option<i64> {
    s.trim().parse::<i64>().ok()
}

/// `.src` のヘッダ部をパースする。
pub fn parse(text: &str) -> Result<SaveFile, String> {
    let toks = tokenize(text);
    if toks.is_empty() {
        return Err("空のセーブデータです。".to_string());
    }
    let mut i = 0usize;
    let version = as_i64(&toks[0]).ok_or_else(|| {
        format!(
            "1 項目目がバージョン番号ではありません: {:?}",
            trunc(&toks[0])
        )
    })?;
    i += 1;

    let mut out = SaveFile {
        version,
        ..Default::default()
    };

    // 旧形式 (version <= 10000) は version 自体がタイトル件数を兼ねる
    // (`SRC.bas:1904` の `num = SaveDataVersion`)。この場合は必ず通常セーブ。
    let (kind, title_count) = if version > 10000 {
        let next = toks.get(i).map(String::as_str).unwrap_or("");
        match as_i64(next) {
            // 数値ならタイトル件数 = 通常セーブ。
            Some(n) => {
                i += 1;
                (SaveKind::Save, n)
            }
            // 文字列なら ScenarioFileName = 中断データ。
            None => {
                out.start_eve = next.to_string();
                i += 1;
                let n = toks.get(i).and_then(|t| as_i64(t)).unwrap_or(0);
                i += 1;
                (SaveKind::Suspend, n)
            }
        }
    } else {
        (SaveKind::Save, version)
    };
    out.kind = Some(kind);

    let title_count = title_count.clamp(0, 4096) as usize;
    for _ in 0..title_count {
        match toks.get(i) {
            Some(t) => {
                out.titles.push(t.clone());
                i += 1;
            }
            None => return Err("タイトル一覧が途中で終わっています。".to_string()),
        }
    }

    match kind {
        SaveKind::Save => {
            // 次ステージ, TotalTurn, Money, dummy
            out.start_eve = toks.get(i).cloned().unwrap_or_default();
            i += 1;
            out.total_turn = toks.get(i).and_then(|t| as_i64(t)).unwrap_or(0);
            i += 1;
            out.money = toks.get(i).and_then(|t| as_i64(t)).unwrap_or(0);
            i += 1;
            // パーツ用ダミー (`SRC.bas:1876`)。
            i += 1;
        }
        SaveKind::Suspend => {
            // Turn, TotalTurn, Money
            out.turn = toks.get(i).and_then(|t| as_i64(t)).unwrap_or(0);
            i += 1;
            out.total_turn = toks.get(i).and_then(|t| as_i64(t)).unwrap_or(0);
            i += 1;
            out.money = toks.get(i).and_then(|t| as_i64(t)).unwrap_or(0);
            i += 1;
        }
    }

    // グローバル変数: 件数 → (name, value) * 件数。
    // 通常セーブは `SaveGlobalVariables`、中断データは `DumpEventData` の
    // 先頭がこの並びになる。件数として解釈できなければ読み飛ばす。
    if let Some(n) = toks.get(i).and_then(|t| as_i64(t)) {
        i += 1;
        let n = n.clamp(0, 65536) as usize;
        for _ in 0..n {
            let name = match toks.get(i) {
                Some(v) => v.clone(),
                None => break,
            };
            let value = toks.get(i + 1).cloned().unwrap_or_default();
            i += 2;
            out.global_vars.push((name, value));
        }
    }

    Ok(out)
}

fn trunc(s: &str) -> String {
    if s.chars().count() <= 32 {
        s.to_string()
    } else {
        s.chars().take(32).collect::<String>() + "…"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// ALTIZASTER/Start.src (通常セーブ, タイトル 0 件) 実物の書式。
    #[test]
    fn parses_normal_save() {
        let s = r#"20033
0
"Eve\Alt-01.eve"
0
0
0
2
"次ステージ","Eve\Alt-01.eve"
"セーブデータファイル名","スタートまでクリア.src"
0
0
0
"#;
        let f = parse(s).unwrap();
        assert_eq!(f.version, 20033);
        assert_eq!(f.kind, Some(SaveKind::Save));
        assert!(f.titles.is_empty());
        assert_eq!(f.start_eve, r"Eve\Alt-01.eve");
        assert_eq!(f.global_vars.len(), 2);
        assert_eq!(f.global_vars[0].0, "次ステージ");
        assert!(!f.is_suspend());
    }

    /// Cendrillon `▼スタート.src` (中断データ, タイトル 2 件) 実物の書式。
    #[test]
    fn parses_suspend_save() {
        let s = r#"20233
"Eve\#00.eve"
2
"零刻のサンドリオン"
"BattleAnime"
0
0
0
39
"次ステージ",""
"セーブデータファイル名","▼スタートまでクリア.src"
"#;
        let f = parse(s).unwrap();
        assert_eq!(f.kind, Some(SaveKind::Suspend));
        assert_eq!(f.start_eve, r"Eve\#00.eve");
        assert_eq!(f.titles, vec!["零刻のサンドリオン", "BattleAnime"]);
        assert!(f.is_suspend());
        // 39 件と宣言されているが本文は 2 件しか無いので、あるだけ読む。
        assert_eq!(f.global_vars.len(), 2);
    }

    /// 旧形式 (version <= 10000): 先頭がタイトル件数を兼ねる。
    #[test]
    fn parses_legacy_save() {
        let s = "0\n\"Scenario\\第１話.eve\"\n0\n0\n0\n0\n";
        let f = parse(s).unwrap();
        assert_eq!(f.version, 0);
        assert_eq!(f.kind, Some(SaveKind::Save));
        assert_eq!(f.start_eve, r"Scenario\第１話.eve");
    }

    #[test]
    fn tokenizer_handles_escaped_quotes_and_commas() {
        let t = tokenize("\"a\"\"b\",\"c,d\"\n1\n");
        assert_eq!(
            t,
            vec!["a\"b".to_string(), "c,d".to_string(), "1".to_string()]
        );
    }

    #[test]
    fn rejects_non_numeric_header() {
        assert!(parse("\"not a version\"\n").is_err());
    }

    #[test]
    fn rejects_empty() {
        assert!(parse("").is_err());
    }
}
