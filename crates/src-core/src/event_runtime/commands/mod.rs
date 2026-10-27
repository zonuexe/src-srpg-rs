//! `.eve` コマンドの実行本体。`exec_command_pc` が変数展開した引数を [`Cmd`] に
//! まとめて [`dispatch`] に渡し、カテゴリ別のモジュールがコマンド名で振り分ける。
//! 各コマンドは 1 関数で、戻り値は次に実行する pc (ジャンプ先を含む)。
//! どのカテゴリにも無い名前は `None` を返し、`exec_command_pc` 側の
//! CamelCase 互換コマンド / ラベル呼び出し / 未登録警告へ進む。

use super::*;

mod audio;
mod battle;
mod control;
mod dialog;
mod files;
mod graphics;
mod stage;
mod unit_state;
mod units;
mod variables;

/// 1 コマンド分の実行コンテキスト。
#[derive(Clone, Copy)]
pub(super) struct Cmd<'a> {
    /// 小文字化したコマンド名 (振り分けキー)。
    pub lname: &'a String,
    /// ユーザ定義関数だけを解決した、変数展開前の引数。
    pub args: &'a Vec<String>,
    /// `args` と同じ並びの原文トークン (クォートを外していない)。式評価器に渡す。
    pub raw_args: &'a Vec<String>,
    /// 原典 `GetArgAsString` どおり評価済みの引数 (`eval::arg_as_string`)。
    pub xargs: &'a Vec<String>,
    pub line: usize,
    pub pc: usize,
    pub stmts: &'a [EventStatement],
    pub labels: &'a HashMap<String, usize>,
}

/// `c.lname` のコマンドを実行する。該当コマンドが無ければ `None`。
pub(super) fn dispatch(app: &mut App, c: &Cmd) -> Option<Result<usize, ScriptError>> {
    control::exec(app, c)
        .or_else(|| variables::exec(app, c))
        .or_else(|| dialog::exec(app, c))
        .or_else(|| stage::exec(app, c))
        .or_else(|| units::exec(app, c))
        .or_else(|| unit_state::exec(app, c))
        .or_else(|| battle::exec(app, c))
        .or_else(|| graphics::exec(app, c))
        .or_else(|| audio::exec(app, c))
        .or_else(|| files::exec(app, c))
}
