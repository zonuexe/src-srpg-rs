//! `.eve` イベントスクリプトの最小実行系 / Minimal `.eve` interpreter.
//!
//! 元 `Event.bas::HandleEvent` (`Event.bas:1400`〜) の縮小版。
//! `data::event::EventStatement` の列を受け取り、対応する命令を
//! `App` / `GameDatabase` に反映する。
//!
//! v1 で解釈する命令:
//!
//! - `Stage "name"` → ステージ名を `App.stage` に保存
//! - `Message "text"` → `App.messages` に追加（HUD 表示用）
//! - `MapSize W H` → 空マップを生成 (全マス平地)
//! - `SetTile X Y TerrainID` → 1 マスの地形 ID を設定
//! - `Pilot Name Nick Sex Class Adaption Exp Infight Shooting Hit Dodge Intuition Technique`
//!   → パイロット定義を追加
//! - `Unit Name Class PilotNum ItemNum Trans Speed Size Value Exp HP EN Armor Mob Adaption`
//!   → ユニット定義を追加
//! - `Weapon UnitName WeaponName Power MinRange MaxRange Precision Bullet`
//!   → 直前定義のユニットに武器追加
//! - `Place UnitName PilotName Party X Y` → マップ上にユニット配置
//! - `Turn N` → ターン数を強制設定
//! - `<include>` → v1 では無視（パーサで Include 化済）
//!
//! v2 で追加した制御フロー / 軽量スクリプト命令:
//!
//! - `Label:` / `@Anchor` → ジャンプ先 / セクションアンカー
//! - `Goto Label`
//! - `Set var value` / `Local var [default]`
//! - `If cond Then` … `ElseIf cond Then` … `Else` … `EndIf`
//!   `cond` は `<lhs> <op> <rhs>` または単独の変数名（空文字列以外で真）。
//! - `Talk speaker` … `End` → 区切られたテキストを 1 件のメッセージとして登録
//! - `Telop "..."` → メッセージ追加（`Message` と同義）
//! - `Win` / `Lose` → StageState を Victory / Defeat に遷移
//! - `Wait N` / `Refresh` → v1 ではタイマー無し、no-op
//! - `Confirm 文` → 対話 UI が無いので常に Yes（`選択` = 0）として進行
//! - `$(name)` 形式の変数展開を全引数に適用
//!
//! 未知のコマンドは無視（後続フェーズで拡張）。

use std::collections::HashMap;

use crate::data::event::EventStatement;
use crate::data::map::{MapCell, MapData};
use crate::data::pilot::{Adaption, PilotData, Sex};
use crate::data::unit::{Size, UnitData};
use crate::item_slot::SlotType;
use crate::util::number::{format_num, format_with_pattern};
use crate::util::text::{
    fold_fullwidth_ascii, like_match, sjis_byte_len, sjis_instr, sjis_instrrev, sjis_left,
    sjis_mid, sjis_right, str_display_width, to_fullwidth,
};
use crate::App;
use crate::{Condition, Party, UnitInstance};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScriptError {
    pub line_num: usize,
    pub message: String,
}

impl std::fmt::Display for ScriptError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}行目: {}", self.line_num, self.message)
    }
}

impl std::error::Error for ScriptError {}

/// 実行ステップの暴走防止上限（Goto ループや無限ループの保険）。
/// 実シナリオの初期化スクリプトは Set/Option/Global を数千件並べる場合があるため
/// 余裕を持たせる。
const STEP_LIMIT: usize = 2_000_000;

/// `Call` / ユーザ定義関数に渡せる引数の上限。
///
/// 原典の引数配列は動的で制限が無いが (`CmdData.cls` の
/// `ReDim strArgs(2 To ArgNum)`)、壊れた `ArgNum` で巨大なループを回さない
/// ための番人として上限を置く。実データの最大は `Args(14)` 程度。
const MAX_CALL_ARGS: usize = 256;

/// 中断可能な `.eve` 実行コンテキスト。`Talk` / `Confirm` で対話 UI を表示すると
/// `App.pending_dialog` をセットしてこのコンテキストを `App` 側に預け、
/// ユーザ応答時に `resume(app)` で続行する。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScriptContext {
    pub statements: Vec<EventStatement>,
    pub labels: HashMap<String, usize>,
    pub pc: usize,
}

/// `Hotpoint name x y w h [非表示]` で登録される、Wait Click 時のクリック
/// 領域。位置情報は描画にも使えるが、最小実装では「選択肢メニュー」のラベル
/// として使う。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HotpointEntry {
    /// クリックされたときに `選択` 変数へ書き込む値（unit 名等）。
    pub name: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    /// `非表示` 修飾子付きで登録されたか（debug 表示用ヒント）。
    #[serde(default)]
    pub invisible: bool,
}

/// ループフレーム。`For var = start To end` / `ForEach var collection` を
/// 統一スタックで管理する。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum LoopFrame {
    /// `For var = start To end [Step step]`
    Numeric {
        var: String,
        end: i64,
        step: i64,
        /// `For` 命令自体の PC。
        for_pc: usize,
    },
    /// `ForEach var collection`
    Each {
        var: String,
        list: Vec<String>,
        index: usize,
        /// `ForEach` 命令自体の PC。
        for_pc: usize,
    },
    /// `ForEach group [status]` (SRC 書式1) — グループのユニットを反復。
    /// ループ変数を持たず、各反復で `対象ユニットＩＤ` / `対象パイロット`
    /// システム変数に現在のユニットを束縛する。
    EachUnit {
        /// 反復対象ユニットの識別子 (uid 優先、無ければ unit_data_name)。
        idents: Vec<String>,
        index: usize,
        /// `ForEach` 命令自体の PC。
        for_pc: usize,
    },
}

impl LoopFrame {
    pub fn for_pc(&self) -> usize {
        match self {
            Self::Numeric { for_pc, .. }
            | Self::Each { for_pc, .. }
            | Self::EachUnit { for_pc, .. } => *for_pc,
        }
    }
}

/// 旧名称との互換: 数値ループフレーム単独表現。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ForFrame {
    pub var: String,
    pub end: i64,
    pub step: i64,
    pub for_pc: usize,
}

impl From<ForFrame> for LoopFrame {
    fn from(f: ForFrame) -> Self {
        Self::Numeric {
            var: f.var,
            end: f.end,
            step: f.step,
            for_pc: f.for_pc,
        }
    }
}

/// 与えられた statements を順に解釈して `app` に反映。
/// 対話命令で中断した場合は `app.pending_dialog` がセットされ、残りは
/// `app.script_ctx` に保存される。再開は `resume(app)`。
///
/// 副作用として `app.script_library` に statements + ラベルを集約し、
/// 後段の `trigger_label("Start")` / `trigger_label("Turn 2")` 等の
/// 自動発火を可能にする。
///
/// 既に別スクリプトが pending_dialog / script_ctx で中断中の場合は、
/// 新しいスクリプトは実行せず library 登録のみ行う（連続 .eve ロード時に
/// 最初の中断状態を上書きしないため）。
pub fn execute(app: &mut App, statements: &[EventStatement]) -> Result<(), ScriptError> {
    // 互換 API: `library_append` + `run_from_pc` を 1 セットで実行する
    // 従来挙動。`archive.rs` がこのまま使う。
    let pc = library_append(app, statements);
    run_from_pc(app, pc)
}

/// 与えられた statements を script_library に追加するだけ。実行はしない。
///
/// 戻り値: 追加された範囲の先頭 PC (= 追加直前の library.statements.len)。
/// 後で `run_from_pc(app, pc)` を呼ぶことで実行できる。
///
/// **2-phase ロード**: 多数 .eve をまとめてロードする場合、まず全件
/// `library_append` してラベルを登録してから、`run_from_pc` を呼ぶことで、
/// 「他ファイルで定義されたラベルが見つからない」配線バグを回避できる。
pub fn library_append(app: &mut App, statements: &[EventStatement]) -> usize {
    let lib_start_offset = app.script_library().statements.len();
    app.script_library_mut().append(statements);
    lib_start_offset
}

/// `.eve` の式を文字列として評価する (原典 `GetValueAsString`)。
/// 差分オラクル (`tools/oracle-diff`) と外部ツール向けの入口。
pub fn eval_expression(app: &App, expr: &str) -> String {
    eval::value_as_string(app, expr)
}

/// 1 行のコマンドを構文解析し、各引数の (文字列値, 整数値, 実数値の `FormatNum`) を返す (原典
/// `GetArgAsString` / `GetArgAsLong` / `GetArgAsDouble`)。数値はコマンドと同じく評価済みの
/// 文字列値から求める (`Cmd::long` / `Cmd::double`)。差分オラクル (`oracle_args`) 向けの入口。
pub fn eval_command_args(app: &App, line: &str) -> Vec<(String, i64, String)> {
    let Ok(stmts) = crate::data::event::parse(line) else {
        return Vec::new();
    };
    match stmts.first() {
        Some(EventStatement::Command { raw_args, .. }) => raw_args
            .iter()
            .map(|r| {
                let s = eval::arg_as_string(app, r);
                let n = eval::evaluated_arg_as_long(r, &s);
                let d = format_num(eval::evaluated_arg_as_double(r, &s));
                (s, n, d)
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `.eve` の `If` 条件式を評価する (原典 `AIfCmd.Evaluate`)。
pub fn eval_if_condition(app: &App, expr: &str) -> bool {
    eval::if_condition(app, expr)
}

/// 既存 script_library 内の `pc` から実行を開始する。
///
/// 既に script_ctx / pending_dialog で中断中の場合は何もせず Ok を返す
/// (`execute` と同じガード)。
///
/// 実行コンテキストには library 全体を載せるので、別 .eve で定義された
/// ラベルへの Goto / Call が解決可能。
/// `*ユニットコマンド <name> <unit> [condition]` の条件式を評価して
/// 表示可否を返す (`true` で表示)。原典 SRC: 「*condition* の値が 0 でない
/// ときにのみコマンドがメニューに表示されます」。`None` は無条件で `true`。
///
/// 条件式は統一評価器 (`eval`) で整数として評価し、0 → 非表示、それ以外 → 表示。
pub fn evaluate_command_condition(app: &mut App, condition: Option<&str>) -> bool {
    let Some(cond) = condition else {
        return true;
    };
    let cond = cond.trim();
    if cond.is_empty() {
        return true;
    }
    // `Call(<label>)` サブ式を事前にサブルーチン同期実行で置換する。
    // `.eve` 内の `Call(label)` 形式の条件式 (e.g. `Call(乗せ換え確認)`) を
    // `call_label_sync_for_condition` で評価して返り値文字列に置換する。
    let processed = if cond.to_ascii_lowercase().contains("call(") {
        preprocess_call_expressions_in_condition(app, cond)
    } else {
        cond.to_string()
    };
    let processed = preprocess_user_functions(app, &processed);
    // 原典 `Command.bas`: `GetValueAsLong(.Para(4)) <> 0`。
    eval::value_as_long(app, &processed) != 0
}

/// 条件式文字列中の `Call(<label>)` パターンを検出し、
/// `call_label_sync_for_condition` で同期実行して返り値に置換する。
///
/// 例: `"Call(乗せ換え確認)"` → `"1"` または `"0"`
/// 例: `"(Call(確認) > 0)"` → `"(1 > 0)"`
///
/// `Call()` が引数なしや多引数の場合は非対応 (実用シナリオ外)。
fn preprocess_call_expressions_in_condition(app: &mut App, src: &str) -> String {
    let mut result = String::new();
    let mut i = 0;
    let src_bytes = src.as_bytes();
    while i < src_bytes.len() {
        // Case-insensitive で "call(" を探す (5 bytes: c-a-l-l-()。
        // 文字列スライス `src[i..i+5]` は i+5 がマルチバイト文字の途中に
        // 落ちると panic するため、バイト列で比較する
        // (例: `Call(版権ＢＧＭ確認) = あり` の `あ` を跨ぐケース)。
        if i + 5 <= src_bytes.len() && src_bytes[i..i + 5].eq_ignore_ascii_case(b"call(") {
            // find_matching_paren は src[i+4] が '(' であることを期待
            if let Some(close) = find_matching_paren(src, i + 4) {
                let label_name = src[i + 5..close].trim();
                let ret_val = call_label_sync(app, label_name, &[]).unwrap_or_default();
                result.push_str(&ret_val);
                i = close + 1;
                continue;
            }
        }
        let ch_end = next_char_end(src, i);
        result.push_str(&src[i..ch_end]);
        i = ch_end;
    }
    result
}

/// 引数を自前で (原文トークンから) 評価するコマンドか。`exec_command_pc` はこれらの
/// 引数を事前展開しない。
fn evaluates_raw_args(lname: &str) -> bool {
    matches!(lname, "if" | "do" | "loop" | "set" | "local" | "__assign")
}

/// 原典がコマンド引数を評価せず原文のまま使う位置か (`CmdData.cls` の `GetArg(i)`。
/// 0 始まり)。変数名 (`For i` / `Incr x` / `Array a` / `Open … As F` 等)・ラベル (`Goto`)・
/// キーワード (`Wait Click` / `For … To … Step`) が該当する。
fn raw_arg_position(lname: &str, i: usize) -> bool {
    match lname {
        "array" | "sort" | "unset" | "incr" | "input" | "goto" | "wait" => i == 0,
        "copyarray" | "swap" => i <= 1,
        "for" => matches!(i, 0 | 1 | 3 | 5),
        "foreach" => matches!(i, 0 | 2),
        "open" => i == 4,
        // ファイルハンドル (`F` / `#1,`) は `file_handle` が解決する。`Read` / `LineRead` の
        // 2 番目以降は読み込み先の変数名。
        "print" | "write" | "close" => i == 0,
        "lineread" | "read" => true,
        "global" => true,
        _ => false,
    }
}

/// `raw` (原文トークン) が `"..."` / `` `...` `` で囲まれていれば、前処理済みの
/// `arg` を同じクォートで包んで返す。囲まれていなければ `arg` をそのまま返す。
fn requote_like(raw: &str, arg: &str) -> String {
    for q in ['"', '`'] {
        if raw.len() >= 2 && raw.starts_with(q) && raw.ends_with(q) {
            return format!("{q}{arg}{q}");
        }
    }
    arg.to_string()
}

/// ユーザ定義関数の再帰段数上限。相互再帰の暴走を防ぐ番人。
const MAX_USER_FUNC_DEPTH: u32 = 32;

/// ユーザ定義関数の呼び出し 1 系列で消費できる総ステップ数。
///
/// 段ごとに上限を設けると、ネストの度に上限が掛け算になって事実上無制限に
/// なる (実測: 100,000 ステップ × 32 段でハングした)。系列全体で共有する。
const MAX_USER_FUNC_TOTAL_STEPS: usize = 200_000;

thread_local! {
    /// 実行中のユーザ定義関数のネスト段数。
    static USER_FUNC_DEPTH: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    /// ネスト全体で共有するステップ予算の消費量。最外段の開始時に 0 へ戻す。
    static USER_FUNC_STEPS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    /// スクリプト本体 / ラベル表のスナップショット。
    ///
    /// `exec_command_pc` は `&mut App` と `&[EventStatement]` を同時に要求する
    /// ため、実行には本体のコピーが要る。ユーザ定義関数は式のたびに呼ばれるので
    /// 呼び出しごとに clone すると破綻する (実測で約 110 倍の減速)。
    /// `Rc` で共有し、登録内容が変わったときだけ作り直す。
    static USER_FUNC_SNAPSHOT: std::cell::RefCell<Option<ScriptSnapshot>> =
        const { std::cell::RefCell::new(None) };
}

/// [`USER_FUNC_SNAPSHOT`] の中身。`len` の組を版数代わりの妥当性判定に使う。
type ScriptSnapshot = (
    usize,
    usize,
    std::rc::Rc<Vec<EventStatement>>,
    std::rc::Rc<HashMap<String, usize>>,
);

/// スクリプト本体 / ラベル表の共有スナップショットを得る。
fn user_func_snapshot(
    app: &App,
) -> (
    std::rc::Rc<Vec<EventStatement>>,
    std::rc::Rc<HashMap<String, usize>>,
) {
    let lib = app.script_library();
    let (n_stmts, n_labels) = (lib.statements.len(), lib.labels.len());
    USER_FUNC_SNAPSHOT.with(|cell| {
        let mut slot = cell.borrow_mut();
        if let Some((cs, cl, stmts, labels)) = slot.as_ref() {
            if *cs == n_stmts && *cl == n_labels {
                return (std::rc::Rc::clone(stmts), std::rc::Rc::clone(labels));
            }
        }
        let stmts = std::rc::Rc::new(lib.statements.clone());
        let labels = std::rc::Rc::new(lib.labels.clone());
        *slot = Some((
            n_stmts,
            n_labels,
            std::rc::Rc::clone(&stmts),
            std::rc::Rc::clone(&labels),
        ));
        (stmts, labels)
    })
}

/// ラベルがユーザ定義関数として呼び出せるか (script_library に定義済みか)。
fn is_user_function(app: &App, name: &str) -> bool {
    !name.is_empty() && app.script_library().label_pc(name).is_some()
}

/// ユーザ定義関数 / サブルーチンを **同期実行** し、`Return <value>` の値を返す。
///
/// 原典 SRC は式中の未知の関数名についてラベルを探し、見つかればユーザ定義関数
/// として呼ぶ (`Expression.function.cs`: `Event.FindNormalLabel(fname)` →
/// `CallUserFunction`)。`Return` の引数がその返り値になる
/// (`Returnコマンド.md`: 「*value* の値がサブルーチンコールの返り値」)。
///
/// ラベルが無ければ `None` (呼び出し側はトークンをリテラルとして残す)。
///
/// 同期実行なので `Talk` / `Confirm` 等のダイアログや `Wait` タイマが発生した
/// 場合は中断して、そこまでの返り値を返す。関数用サブルーチンでこれらを使うのは
/// 原典でも想定外。
fn call_label_sync(app: &mut App, label_name: &str, args: &[String]) -> Option<String> {
    let lib = app.script_library();
    let pc = lib.label_pc(label_name)?;
    // 再帰段数の番人。超えたら空文字を返して打ち切る。
    let depth = USER_FUNC_DEPTH.with(|d| d.get());
    if depth >= MAX_USER_FUNC_DEPTH {
        return Some(String::new());
    }
    if depth == 0 {
        // 最外段でのみ予算をリセットする。
        USER_FUNC_STEPS.with(|c| c.set(0));
    }
    USER_FUNC_DEPTH.with(|d| d.set(depth + 1));
    let result = call_label_sync_inner(app, pc, args);
    USER_FUNC_DEPTH.with(|d| d.set(depth));
    Some(result)
}

fn call_label_sync_inner(app: &mut App, pc: usize, args: &[String]) -> String {
    let (stmts_rc, labels_rc) = user_func_snapshot(app);
    let stmts: &[EventStatement] = &stmts_rc;
    let labels: &HashMap<String, usize> = &labels_rc;

    // 戻り値フィールドをクリアしてフレームをセットアップ
    app.set_last_return_value(String::new());
    let saved = enter_call_args(app, args);
    // 番兵 PC: stmts.len() を使うと Return 後のループ終了判定に便利
    app.push_call_return(stmts.len(), saved);
    let call_depth_before = app.call_stack_depth();
    let for_depth_before = app.for_stack_len();

    let mut curr_pc = pc + 1; // ラベル行の次から実行
    loop {
        // ネスト全体で共有する予算。使い切ったら打ち切る。
        let used = USER_FUNC_STEPS.with(|c| c.get());
        if used >= MAX_USER_FUNC_TOTAL_STEPS {
            break;
        }
        USER_FUNC_STEPS.with(|c| c.set(used + 1));
        if curr_pc >= stmts.len() {
            break;
        }
        if app.call_stack_depth() < call_depth_before {
            // Return でフレームがポップされた
            break;
        }
        if app.pending_dialog().is_some() || app.pending_timer().is_some() {
            // ダイアログ / Wait が発生した場合は中断 (条件サブルーチン想定外)
            break;
        }
        let stmt = stmts[curr_pc].clone();
        match stmt {
            EventStatement::Include { .. } => {
                curr_pc += 1;
            }
            EventStatement::Command {
                name,
                args,
                line_num,
                raw_args,
            } => {
                if canonical_label_full(&name, &args).is_some() {
                    curr_pc += 1;
                    continue;
                }
                match exec_command_pc(
                    app, &name, &args, &raw_args, line_num, curr_pc, stmts, labels,
                ) {
                    Ok(next_pc) => {
                        curr_pc = next_pc;
                    }
                    Err(_) => break,
                }
            }
        }
    }

    // コールスタックにフレームが残っていればクリーンアップ
    // (通常は Return でポップされているはず)
    while app.call_stack_depth() >= call_depth_before {
        if let Some((_, s)) = app.pop_call_return() {
            restore_call_args(app, s);
        } else {
            break;
        }
    }
    // For スタックのクリーンアップ (For ループ途中で抜けた場合に備えて)
    app.truncate_for_stack(for_depth_before);

    app.take_last_return_value()
}

/// 式文字列中の **ユーザ定義関数呼び出し** を実行し、返り値へ置換する前処理。
///
/// 原典 SRC は式の評価中に未知の関数名をラベルとして解決し、その場で
/// サブルーチンを呼ぶ (`Expression.function.cs`)。本実装の式展開
/// (`eval`) は `&App` (不変) しか持たずサブルーチンを
/// 実行できないため、`&mut App` を持つ地点で先に解決して文字列へ畳み込む。
/// 条件式の `Call(<label>)` を先に潰す既存の前処理と同じ方式。
///
/// 解決するのは **script_library にラベルが定義されている名前だけ**。
/// 組込み関数名やユニット名などは対象外なので、通常の式評価に素通しする。
/// 引数は内側から順に解決される (再帰)。
fn preprocess_user_functions(app: &mut App, src: &str) -> String {
    // 早期リターン: `(` を含まない式にユーザ定義関数呼び出しは有り得ない。
    if !src.contains('(') {
        return src.to_string();
    }
    let mut out = String::new();
    let mut i = 0usize;
    let mut in_quote = false;
    while i < src.len() {
        let ch_end = next_char_end(src, i);
        let ch = &src[i..ch_end];
        if ch == "\"" {
            in_quote = !in_quote;
            out.push_str(ch);
            i = ch_end;
            continue;
        }
        if in_quote {
            out.push_str(ch);
            i = ch_end;
            continue;
        }
        if let Some((name, args_str, total)) = take_user_function_call(app, src, i) {
            // 引数は内側のユーザ定義関数を先に解決してから渡す。
            let inner = preprocess_user_functions(app, args_str);
            let args: Vec<String> = split_function_args(&inner)
                .into_iter()
                .map(|a| eval::param_value(app, a.trim()))
                .collect();
            match call_label_sync(app, &name, &args) {
                Some(v) => out.push_str(&v),
                // ラベルが消えている等で呼べなければ元のトークンを温存。
                None => out.push_str(&src[i..i + total]),
            }
            i += total;
            continue;
        }
        out.push_str(ch);
        i = ch_end;
    }
    out
}

/// `i` の位置から `<ラベル名>(<引数>)` 形式のユーザ定義関数呼び出しを取り出す。
///
/// `take_function_call` と違い **ASCII 始まりに限定しない**。原典は
/// 「先頭がアルファベットでなければ必ずユーザー定義関数」と扱っており
/// (`Expression.function.cs`)、実シナリオでも `シールド判定(Args(1))` の
/// ような日本語名の関数が多用される。
///
/// 名前として認めるのは、区切り文字・演算子・空白を含まない連続した文字列。
/// script_library に同名ラベルがあるときだけ Some を返す。
fn take_user_function_call<'a>(
    app: &App,
    src: &'a str,
    i: usize,
) -> Option<(String, &'a str, usize)> {
    // 直前が識別子構成文字なら、より長い名前の途中なので開始位置ではない。
    if i > 0 {
        let prev = src[..i].chars().next_back()?;
        if !is_expr_delimiter(prev) {
            return None;
        }
    }
    // 名前部分を読む。
    let mut j = i;
    for (off, c) in src[i..].char_indices() {
        if c == '(' {
            j = i + off;
            break;
        }
        if is_expr_delimiter(c) {
            return None;
        }
        j = i + off + c.len_utf8();
    }
    if j >= src.len() || src.as_bytes()[j] != b'(' {
        return None;
    }
    let name = &src[i..j];
    if name.is_empty() || !is_user_function(app, name) {
        return None;
    }
    let close = find_matching_paren(src, j)?;
    // 原典 `Expression.function.cs` は「括弧を含むユニット名等である場合が
    // あるため」、ユーザ定義関数と見なす前にトークン **全体** が
    // ユニット / パイロット / アイテムとして定義済みでないか確かめる。
    // 実サンプルの `Upgrade キャリバーン キャリバーン(不完全)` が該当し、
    // これを関数呼び出しと誤認すると機体名が空文字に化ける。
    let whole = &src[i..close + 1];
    if is_defined_data_name(app, whole) {
        return None;
    }
    Some((name.to_string(), &src[j + 1..close], close + 1 - i))
}

/// トークンがデータベース上のユニット / パイロット / アイテム名か。
fn is_defined_data_name(app: &App, name: &str) -> bool {
    let db = app.database();
    db.unit_by_name(name).is_some()
        || db.pilot_by_name(name).is_some()
        || db.item_by_name(name).is_some()
}

/// 式中で識別子を区切る文字か。
fn is_expr_delimiter(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '(' | ')'
                | ','
                | '"'
                | '+'
                | '-'
                | '*'
                | '/'
                | '\\'
                | '='
                | '<'
                | '>'
                | '&'
                | '$'
                | '['
                | ']'
                | '!'
        )
}

pub fn run_from_pc(app: &mut App, pc: usize) -> Result<(), ScriptError> {
    if app.has_script_context() || app.pending_dialog().is_some() {
        return Ok(());
    }
    let lib = app.script_library();
    let ctx = ScriptContext {
        statements: lib.statements.clone(),
        labels: lib.labels.clone(),
        pc,
    };
    run_loop(app, ctx)
}

/// `App.script_library` 内の `name` ラベルから新規スクリプトコンテキストを
/// 開始する。既に実行中 / 対話中なら no-op で `false` を返す。
pub fn trigger_label(app: &mut App, name: &str) -> bool {
    if app.has_script_context() || app.pending_dialog().is_some() {
        return false;
    }
    let lib = app.script_library();
    let Some(pc) = lib.label_pc(name) else {
        return false;
    };
    let ctx = ScriptContext {
        statements: lib.statements.clone(),
        labels: lib.labels.clone(),
        pc,
    };
    // 引数無し `ClearEvent` が「いま実行中のイベントラベル」を消せるよう pc を記録。
    app.set_current_event_label_pc(Some(pc));
    let _ = run_loop(app, ctx);
    true
}

/// 指定ファイル内のラベルから新規スクリプトコンテキストを開始する。
/// `trigger_label` の **ファイルスコープ版**: 同名 label が複数 .eve に
/// 定義されている (`lib/CMaking.eve::プロローグ` vs 主シナリオ
/// `プロローグ` 等) ときに、対象ファイルの中で最初に見つかる label を
/// 選んで起動する。
///
/// 既に実行中 / 対話中なら no-op で `false`。
/// ファイル / ラベルどちらかが未登録でも `false`。
pub fn trigger_label_in_file(app: &mut App, file_path: &str, label: &str) -> bool {
    if app.has_script_context() || app.pending_dialog().is_some() {
        return false;
    }
    let lib = app.script_library();
    let Some(pc) = lib.label_pc_in_file(file_path, label) else {
        return false;
    };
    let ctx = ScriptContext {
        statements: lib.statements.clone(),
        labels: lib.labels.clone(),
        pc,
    };
    // 引数無し `ClearEvent` 用に実行中イベントラベルの pc を記録。
    app.set_current_event_label_pc(Some(pc));
    let _ = run_loop(app, ctx);
    true
}

/// スクリプト実行の終わり方 (docs/FLOW_REDESIGN.md §2.1)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecOutcome {
    /// 最後まで実行された。
    Completed,
    /// `pending_dialog` / `pending_timer` で中断し、ctx を App に預けた。
    Suspended,
}

/// 中断していたスクリプトを再開。何も無ければ no-op。
pub fn resume(app: &mut App) -> Result<(), ScriptError> {
    let Some(ctx) = app.take_script_context() else {
        return Ok(());
    };
    run_loop(app, ctx)
}

/// 実行ループ本体。`pending_dialog` がセットされたら ctx を App に預けて即抜け。
///
/// **完了プロトコル**: スクリプトが完了 (またはエラー終了) したら、必ず
/// `App::on_script_completed()` に通知して flow 継続を消化させる。suspend した
/// 場合は通知しない (resume 後の完了時に通知される)。これにより呼び出し側は
/// 「インライン完了か suspend か」を区別する必要がない。
fn run_loop(app: &mut App, ctx: ScriptContext) -> Result<(), ScriptError> {
    // `KeyState()` 呼び出しカウンタをリセット。Do While (KeyState()=0) ループが
    // STEP_LIMIT まで走り続けるのを防ぐため、各スクリプト実行ごとに初期化する。
    app.reset_keystate_call_count();
    // 実行ネスト深さを記録。実行中の ctx はローカル変数なので、ネストした
    // run_loop (再入) の完了で外側の実行を「完了」と誤認しないために使う。
    app.enter_script_run();
    let r = run_loop_inner(app, ctx);
    app.exit_script_run();
    match &r {
        Err(e) => {
            // 直近のスクリプトエラーを App に記録 (デバッグ用)。caller が
            // エラーを握り潰しても `debug_summary` で原因を追える。
            app.set_last_script_error(format!("L{}: {}", e.line_num, e.message));
            // エラー終了も「完了」として扱い、flow 継続を stale にしない
            // (実行時エラー黙殺の方針に合わせ、進行は止めない)。
            if app.script_run_depth() == 0 {
                app.set_current_event_label_pc(None);
                app.on_script_completed();
            }
        }
        Ok(ExecOutcome::Completed) => {
            if app.script_run_depth() == 0 {
                // イベントスクリプト完走 → 実行中イベントラベルの追跡を解除。
                // (Talk 中断時は Suspended 枝なのでクリアせず resume まで保持する。)
                app.set_current_event_label_pc(None);
                app.on_script_completed();
            }
        }
        Ok(ExecOutcome::Suspended) => {}
    }
    r.map(|_| ())
}

fn run_loop_inner(app: &mut App, mut ctx: ScriptContext) -> Result<ExecOutcome, ScriptError> {
    let mut steps: usize = 0;
    while ctx.pc < ctx.statements.len() {
        // 実行中の pc を記録 (対話が中断したとき発生元を逆引きできるようにする診断用)。
        app.set_exec_pc(ctx.pc);
        steps += 1;
        if steps > STEP_LIMIT {
            return Err(err(
                ctx.statements[ctx.pc].line_num(),
                "実行ステップ数が上限を超えました（無限ループの可能性）。",
            ));
        }
        // ループ内では statements への参照を直接持たず、cloneして使う。
        let stmt = ctx.statements[ctx.pc].clone();
        match stmt {
            EventStatement::Include { .. } => {
                ctx.pc += 1; /* v1: 無視 */
            }
            EventStatement::Command {
                name,
                args,
                line_num,
                raw_args,
            } => {
                // 単一トークン (`プロローグ:` / `@onsen`) も複数トークン
                // (`ターン 1:` / `Destruction ガロ:`) も canonical_label_full
                // で一括判定。
                if let Some(lbl) = canonical_label_full(&name, &args) {
                    // `スタート` ラベル行の通過 = スタートイベントの中身が
                    // この実行でインライン実行される事実を記録する
                    // (`FlowCont::AfterStageFileRun` が再発火判定に使う)。
                    // ラベルから直接 trigger した場合 (pc がラベル行始まり) も
                    // fall-through / Goto で到達した場合も等しく検知できる。
                    if matches!(lbl.as_str(), "スタート" | "Start") {
                        app.mark_start_label_passed(ctx.pc);
                    }
                    ctx.pc += 1;
                    continue;
                }
                let next = exec_command_pc(
                    app,
                    &name,
                    &args,
                    &raw_args,
                    line_num,
                    ctx.pc,
                    &ctx.statements,
                    &ctx.labels,
                )?;
                ctx.pc = next;
                // 対話 UI / Wait <duration> タイマで中断する
                if app.pending_dialog().is_some() || app.pending_timer().is_some() {
                    app.set_script_context(ctx);
                    return Ok(ExecOutcome::Suspended);
                }
            }
        }
    }
    Ok(ExecOutcome::Completed)
}

/// `name = expr` の assignment sugar 適用から除外すべき名前か。
/// control-flow keyword はそもそも別 arm で先取り処理されるが、保険として
/// ここでも弾いておく。
fn is_assign_sugar_excluded(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "if" | "elseif"
            | "else"
            | "endif"
            | "for"
            | "next"
            | "foreach"
            | "switch"
            | "case"
            | "caseelse"
            | "endsw"
            | "do"
            | "loop"
            | "break"
            | "continue"
            | "set"
            | "local"
            | "incr"
    )
}

/// `name [= rhs...]` を `Set <name> rhs...` に組替えるための引数列を構築。
fn assign_sugar_args(name: &str, rhs: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(rhs.len() + 1);
    out.push(name.to_string());
    out.extend(rhs.iter().cloned());
    out
}

/// ラベル名 → PC のマップを事前構築。`label:` / `@anchor` / `*label:` 全形式。
/// 現在は `ScriptLibrary::append` が同じロジックを持つため execute() からは
/// 使われないが、テスト / 別経路用に残す。
#[allow(dead_code)]
fn collect_labels(stmts: &[EventStatement]) -> HashMap<String, usize> {
    let mut m = HashMap::new();
    for (i, s) in stmts.iter().enumerate() {
        if let EventStatement::Command { name, .. } = s {
            if let Some(canon) = canonical_label(name) {
                m.entry(canon).or_insert(i);
            }
        }
    }
    m
}

/// 1 命令を実行し、次に進むべき PC を返す。
///
/// `labels` は呼び出し元 (`run_loop_inner`) が持つ ctx 由来の global ラベル表で、
/// 多文 body / alias の再帰 dispatch にそのまま引き継ぐためだけに保持している
/// (Goto / Continue のラベル解決は `App::script_library` のファイルスコープ版を
/// 使うため本パラメタは直接参照しない)。再帰専用なので clippy 警告を抑制する。
#[allow(clippy::only_used_in_recursion)]
#[allow(clippy::too_many_arguments)] // 1 行分の実行文脈 (引数・原文・位置・本体・ラベル表) を素直に列挙。
fn exec_command_pc(
    app: &mut App,
    name: &str,
    args: &[String],
    raw_args: &[String],
    line: usize,
    pc: usize,
    stmts: &[EventStatement],
    labels: &HashMap<String, usize>,
) -> Result<usize, ScriptError> {
    // VB5 風代入文の救済: `var = expr` (第 1 引数が裸の `=`) は
    // `Set var expr` と等価に dispatch する。
    // 実シナリオ (`UnitHP = HP(Args(1))` 等) で頻出する記法で、これを
    // catch しないと dispatcher は `UnitHP` を未登録コマンドとして
    // warning ノイズの源にしてしまう。
    //
    // 制御フロー命令 (`If` / `For` / `Set` etc.) は元来 `=` を別意味で
    // 解釈するので、ここでは「name が control-flow keyword でない」
    // ことを確認する必要は無い: そもそも control-flow は別 arm で
    // 先取り処理される。本 sugar は dispatcher の入口に置くため、
    // `If x = y` のような cond 評価には影響しない (それらは args[1]
    // までを cond として解釈するが、`name` 自体は `If` / `Set` 等の
    // 既知 keyword であって、本 sugar が起動するためには第 1 引数が
    // 裸の `=` でかつ `name` が control-flow でないことが必要)。
    if args.first().map(String::as_str) == Some("=") && !is_assign_sugar_excluded(name) {
        let rhs: Vec<String> = args[1..].to_vec();
        // `__assign` は `Set` と同じハンドラへ流すが「`=` 代入形」であることを伝える
        // 内部コマンド名。`Set var value` と異なり RHS は常に式なので括弧無し算術
        // (`資本金 = 資本金 - 営業収支`) も数値化される。
        let assign_args = assign_sugar_args(name, &rhs);
        // 原文トークンも同じ形 (`name` + `=` の右側) に揃える。
        let assign_raw: Vec<String> = if raw_args.len() == args.len() {
            assign_sugar_args(name, &raw_args[1..])
        } else {
            assign_args.clone()
        };
        return exec_command_pc(
            app,
            "__assign",
            &assign_args,
            &assign_raw,
            line,
            pc,
            stmts,
            labels,
        );
    }
    // 各引数を $(var) / Args(N) / 関数呼出 / インデックス変数で展開後、
    // `&` 連結演算子を畳み込む（`"abc" & x & ".bmp"` → "abcvalue.bmp"）。
    // 式展開の前にユーザ定義関数呼び出しを解決して値へ畳み込む
    // (評価器は `&App` しか持たずサブルーチンを実行できないため)。
    let args: Vec<String> = args
        .iter()
        .map(|a| preprocess_user_functions(app, a))
        .collect();
    // 原文トークンは、クォートで囲まれていたものだけ前処理済みの引数をクォートで
    // 包み直す (ユーザ定義関数を 2 度実行しないため)。並びが合わなければ引数で代用。
    let raw_args: Vec<String> = if raw_args.len() == args.len() {
        raw_args
            .iter()
            .zip(&args)
            .map(|(raw, a)| requote_like(raw, a))
            .collect()
    } else {
        args.clone()
    };
    let lname = name.to_ascii_lowercase();
    // 条件式を原文トークンから統一評価器で評価するコマンドは、ここで引数を展開しない。
    // 展開すると `KeyState(2)` (ワンショット消費) や `Random()` (乱数を進める) が
    // 評価器と合わせて 2 度呼ばれてしまう。
    //
    // それ以外の引数は原典 `GetArgAsString` どおり 1 引数ずつ評価する (`eval::arg_as_string`:
    // クォート文字列はそのまま、数値は数値、それ以外は項として変数・関数・括弧式を評価)。
    // 変数名・ラベル・キーワードとして原文のまま使う位置 (`raw_arg_position`) は評価しない。
    let xargs: Vec<String> = if evaluates_raw_args(&lname) {
        args.clone()
    } else {
        raw_args
            .iter()
            .enumerate()
            .map(|(i, raw)| {
                if raw_arg_position(&lname, i) {
                    args[i].clone()
                } else {
                    eval::arg_as_string(app, raw)
                }
            })
            .collect()
    };
    let c = commands::Cmd {
        lname: &lname,
        args: &args,
        raw_args: &raw_args,
        xargs: &xargs,
        line,
        pc,
        stmts,
        labels,
    };
    if let Some(r) = commands::dispatch(app, &c) {
        return r;
    }

    // 既存コマンド（CamelCase 一致のもの）。変数展開後の xargs を渡す。
    match name {
        "Stage" => {
            let v = expect_arg(&xargs, 0, line, "Stage <name>")?;
            app.set_stage(v.to_string());
        }
        "Message" => {
            let v = expect_arg(&xargs, 0, line, "Message <text>")?;
            app.push_message(v.to_string());
        }
        "MapSize" => {
            let w = parse_u32(&xargs, 0, line)?;
            let h = parse_u32(&xargs, 1, line)?;
            app.database_mut().replace_map(MapData::new(w, h));
        }
        "SetTile" => {
            if xargs.len() < 3 {
                return Err(err(line, "SetTile 命令は 3 引数必要 (x y terrain_id)。"));
            }
            // 座標/地形 ID は式評価 (`For y .../For x ... SetTile x y t` のループ変数対応)。
            let x = eval_coord_u32(&xargs, 0);
            let y = eval_coord_u32(&xargs, 1);
            let tid = eval_coord_u32(&xargs, 2);
            let Some(map) = app.database_mut().map.as_mut() else {
                return Err(err(line, "SetTile より前に MapSize が必要。"));
            };
            if x >= map.width || y >= map.height {
                return Err(err(line, "SetTile 座標がマップ外。"));
            }
            map.set_cell(
                x,
                y,
                MapCell {
                    terrain_id: tid,
                    bitmap_no: 0,
                },
            );
        }
        "Pilot" => {
            // SRC `Pilotコマンド.md` のイベント命令本来の書式は
            //   Pilot name level [ID]
            // ロード済みデータから name を level (味方) で作成する。`Unit name
            // [rank]` と対になる隊列構築イディオム (Unit / Pilot / Ride)。
            // 12 引数以上のときだけ旧来のデータ定義形式 (PilotData push) として
            // 後方互換で扱う (実シナリオではほぼ出現しない)。
            if xargs.len() >= 12 {
                let pilot = PilotData {
                    spirit_commands: Vec::new(),
                    name: xargs[0].clone(),
                    nickname: xargs[1].clone(),
                    kana_name: xargs[1].clone(),
                    sex: Sex::parse(&xargs[2]).unwrap_or(Sex::Unspecified),
                    class: xargs[3].clone(),
                    adaption: Adaption::parse(&xargs[4])
                        .ok_or_else(|| err(line, "Pilot.Adaption は 4 文字 ASCII。"))?,
                    exp_value: parse_i32_at(&xargs[5], line)?,
                    infight: parse_i32_at(&xargs[6], line)?,
                    shooting: parse_i32_at(&xargs[7], line)?,
                    hit: parse_i32_at(&xargs[8], line)?,
                    dodge: parse_i32_at(&xargs[9], line)?,
                    intuition: parse_i32_at(&xargs[10], line)?,
                    technique: parse_i32_at(&xargs[11], line)?,
                    personality: None,
                    sp: None,
                    bgm: None,
                    bitmap: None,
                    features: Vec::new(),
                };
                app.database_mut().catalog.pilots.push(pilot);
                return Ok(pc + 1);
            }
            // インスタンス書式: Pilot name level [ID]
            if xargs.is_empty() {
                return Err(err(line, "Pilot 命令は name level が必要。"));
            }
            let name = xargs[0].to_string();
            // レベルは原典どおり `GetArgAsLong`。
            let level = c.long(app, 1).map_or(1, |n| n as i32);
            // データ名 / 通称から PilotData を解決して runtime インスタンスを作成。
            let data_name = app
                .database()
                .catalog
                .pilots
                .iter()
                .find(|p| p.name == name || p.nickname == name)
                .map(|p| p.name.clone());
            if let Some(dn) = data_name {
                if !app
                    .database()
                    .pilot_instances
                    .iter()
                    .any(|p| p.pilot_data_name == dn)
                {
                    app.database_mut().create_pilot_instance(&dn, &dn);
                }
                if let Some(pi) = app
                    .database_mut()
                    .pilot_instances
                    .iter_mut()
                    .find(|p| p.pilot_data_name == dn)
                {
                    pi.level = level.clamp(1, 999);
                }
            }
        }
        "Unit" => {
            // `Unit` には 2 つの書式がある:
            //  1. データ定義形式 (14+ 引数):
            //     Unit Name Class PilotNum ItemNum Trans Speed Size Value Exp HP EN Armor Mob Adaption
            //  2. インスタンス化形式 (`Unit name [level]` — 1〜2 引数):
            //     既存ユニットデータをカレントユニットとして生成し、後続の
            //     `Pilot` / `Ride` で搭乗員を組み立てる SRC の隊列構築イディオム。
            //
            // 短い形式 (`Unit <name> [rank]`) は既存ユニットデータを 1 体
            // インスタンス化して **カレントユニット** に設定する。SRC の
            // `UList.Add(uname, rank, "味方")` + `Event.SelectedUnitForEvent = u`
            // に対応。生成ユニットは未配置 (off_map) の Player ユニットで、
            // 後続の `Ride <pilot>` (unit 省略形) がここに搭乗員を載せる。
            // スパロボ戦記 の `乗せ換え処理` 等が `Unit 仮ユニット 0` を使う。
            if xargs.len() < 14 {
                if let Some(name_arg) = xargs.first() {
                    let unit_data_name = name_arg.to_string();
                    let mut inst =
                        UnitInstance::new(unit_data_name, String::new(), Party::Player, 0, 0);
                    inst.off_map = true;
                    let uid = app.database_mut().register_unit(inst);
                    app.set_selected_unit_for_event(uid);
                }
                return Ok(pc + 1);
            }
            let size =
                Size::parse(&xargs[6]).ok_or_else(|| err(line, "Unit.Size は XL/LL/L/M/S/SS。"))?;
            let unit = UnitData {
                abilities: Vec::new(),
                name: xargs[0].clone(),
                kana_name: xargs[0].clone(),
                nickname: xargs[0].clone(),
                class: xargs[1].clone(),
                pilot_num: parse_i32_at(&xargs[2], line)?,
                item_num: parse_i32_at(&xargs[3], line)?,
                transportation: xargs[4].clone(),
                speed: parse_i32_at(&xargs[5], line)?,
                size,
                value: parse_i64_at(&xargs[7], line)?,
                exp_value: parse_i32_at(&xargs[8], line)?,
                hp: parse_i64_at(&xargs[9], line)?,
                en: parse_i32_at(&xargs[10], line)?,
                armor: parse_i64_at(&xargs[11], line)?,
                mobility: parse_i32_at(&xargs[12], line)?,
                adaption: Adaption::parse(&xargs[13])
                    .ok_or_else(|| err(line, "Unit.Adaption は 4 文字 ASCII。"))?,
                bitmap: String::new(),
                weapons: Vec::new(),
                features: Vec::new(),
            };
            app.database_mut().catalog.units.push(unit);
        }
        "Weapon" => {
            // Weapon UnitName WeaponName Power MinRange MaxRange Precision Bullet
            if xargs.len() < 7 {
                return Err(err(line, "Weapon 命令は 7 引数必要。"));
            }
            let wd = crate::data::unit::WeaponData {
                name: xargs[1].clone(),
                power: parse_i64_at(&xargs[2], line)?,
                min_range: parse_i32_at(&xargs[3], line)?,
                max_range: parse_i32_at(&xargs[4], line)?,
                precision: parse_i32_at(&xargs[5], line)?,
                bullet: parse_i32_at(&xargs[6], line)?,
                en_consumption: 0,
                necessary_morale: 0,
                adaption: String::new(),
                critical: 0,
                class: String::new(),
                extras: Vec::new(),
            };
            let unit_name = &xargs[0];
            let pos = app
                .database()
                .catalog
                .units
                .iter()
                .position(|u| u.name == *unit_name)
                .ok_or_else(|| err(line, "Weapon: 対象 Unit が未定義。"))?;
            app.database_mut().catalog.units[pos].weapons.push(wd);
        }
        "Place" => {
            // Place UnitName PilotName Party X Y
            if xargs.len() < 5 {
                return Err(err(line, "Place 命令は 5 引数必要。"));
            }
            let unit_data_name = xargs[0].clone();
            let party = parse_party(&xargs[2], line)?;
            // 座標は式評価 (ループ変数/算術式対応)。
            let x = eval_coord_u32(&xargs, 3);
            let y = eval_coord_u32(&xargs, 4);
            let mut inst = UnitInstance::new(unit_data_name.clone(), xargs[1].clone(), party, x, y);
            populate_active_features(&mut inst, app);
            app.database_mut().register_unit(inst);
        }
        "Turn" => {
            let n = parse_u32(&xargs, 0, line)?;
            app.set_turn_number(n);
        }
        "Briefing" => {
            let v = expect_arg(&xargs, 0, line, "Briefing <text>")?;
            app.set_briefing(v.to_string());
        }
        "Start" => {
            // Briefing/Sortie を飛ばして Battle に直行する .eve コマンド。
            // シナリオによってはブリーフィングを使わず即戦闘に入る場合用。
            app.set_stage_state(crate::stage::StageState::Battle);
        }
        _ => {
            // dispatcher の match に届かなかったコマンド。
            //
            // 1) `name` が script_library のラベルとして登録されているなら
            //    SRC 互換の **implicit Call** として扱う。real シナリオは
            //    `Call sub args` の `Call` を省略してラベル名直書きで呼び出す
            //    記法 (`wPaintString 25 70 hello` 等) を多用するので、これを
            //    catch しないと dispatcher は無反応になる。
            if app.script_library().label_pc(name).is_some() {
                // 戻りアドレスは現命令の次。Args(1).. をシナリオ変数として
                // 束縛 (`call` arm と同じセマンティクス)。
                let saved = enter_call_args(app, &xargs);
                app.push_call_return(pc + 1, saved);
                return jump_to(app, pc, name, line);
            }
            // 2) ラベルでもないなら catalog 問い合わせて Stub なら silent OK、
            //    未登録 / Implemented 表明だが届かないなら warning。
            crate::command_catalog::handle_unrecognized(name, line);
        }
    }
    Ok(pc + 1)
}

fn matches_unit_handle(u: &UnitInstance, key: &str) -> bool {
    if (!u.uid.is_empty() && u.uid == key) || u.unit_data_name == key || u.pilot_name == key {
        return true;
    }
    // SRC 互換: 全角/半角を同一視して再照合（例: ハンドル `バ5` ↔ 参照 `バ５`）。
    let k = fold_fullwidth_ascii(key);
    (!u.uid.is_empty() && fold_fullwidth_ascii(&u.uid) == k)
        || fold_fullwidth_ascii(&u.unit_data_name) == k
        || fold_fullwidth_ascii(&u.pilot_name) == k
}

/// 与えた名前のユニットが生存しているか。
fn unit_alive(app: &App, key: &str) -> bool {
    app.database()
        .unit_instances
        .iter()
        .any(|u| matches_unit_handle(u, key))
}

/// `UseAbility` の引数から (使用者 uid, アビリティ index, 対象 uid, アビリティ名) を
/// 解決する。使用者のユニットデータに該当アビリティが無ければ `None`。
/// 引数の解釈は C# `UseAbilityCmd` に合わせる:
///  - 1 引数: `ability` (使用者・対象とも SelectedUnitForEvent)
///  - 2 引数: `ability target` (SelectedUnitForEvent が ability を持つ) / `unit ability`
///  - 3 引数: `unit ability target`
fn resolve_data_ability(app: &App, xargs: &[String]) -> Option<(String, usize, String, String)> {
    let uid_of = |key: &str| {
        app.database()
            .unit_instances
            .iter()
            .find(|u| matches_unit_handle(u, key))
            .map(|u| u.uid.clone())
    };
    let selected = uid_of(app.selected_unit_for_event());
    let (caster, name, target) = match xargs {
        [] => return None,
        [name] => {
            let s = selected?;
            (s.clone(), name.clone(), s)
        }
        [a, b] => match &selected {
            Some(s) if app.ability_index_by_name(s, a).is_some() => {
                (s.clone(), a.clone(), uid_of(b)?)
            }
            _ => {
                let c = uid_of(a)?;
                (c.clone(), b.clone(), c)
            }
        },
        [a, b, c, ..] => (uid_of(a)?, b.clone(), uid_of(c)?),
    };
    let idx = app.ability_index_by_name(&caster, &name)?;
    Some((caster, idx, target, name))
}

/// コマンド引数のユニット指定を解決する。`handle` が実ユニット (uid/unit名/pilot名) に
/// 一致すればそのまま、一致しなければ同名 script_var の値へフォールバックする。SRC は
/// コマンド引数のユニット位置にシステム変数 (`相手ユニットＩＤ` 等) を許すため、裸の
/// 変数名 (生贄ルーチンの `SpecialPower 相手ユニットＩＤ みがわり 対象ユニットＩＤ`) を
/// uid へ解決できる。実ユニット名を優先するので通常の literal 指定は壊れない。
fn resolve_handle_var(app: &App, handle: String) -> String {
    if app
        .database()
        .unit_instances
        .iter()
        .any(|u| matches_unit_handle(u, &handle))
    {
        return handle;
    }
    let v = app.script_var(&handle);
    if v.is_empty() {
        handle
    } else {
        v.to_string()
    }
}

/// SRC `Talk` ボディに含まれる HTML 書式タグを除去・変換する。
///
/// SRC は Talk メッセージ中に VB5 Rich TextBox 用の簡易タグを許容する:
///   `<B>`, `</B>`, `<I>`, `</I>`, `<BIG>`, `</BIG>`, `<SMALL>`, `</SMALL>`,
///   `<SIZE=n>`, `</SIZE>`, `<COLOR=...>`, `</COLOR>`
///
/// プレーンテキスト描画では不要なため除去する。
/// `<LT>` → `<`、`<GT>` → `>` は SRC 仕様どおりに変換する。
fn strip_talk_tags(s: &str) -> String {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let mut out = String::with_capacity(len);
    let mut i = 0;
    while i < len {
        if bytes[i] != b'<' {
            // 非タグ文字はそのままコピー（マルチバイト安全: バイト単位でスキャン）
            let start = i;
            while i < len && bytes[i] != b'<' {
                i += 1;
            }
            out.push_str(&s[start..i]);
            continue;
        }
        // `<` から `>` までのタグ候補を探す。
        // ネストや複数 `>` は考慮不要 (SRC タグは単純)。
        let tag_start = i;
        i += 1; // skip '<'
        while i < len && bytes[i] != b'>' {
            i += 1;
        }
        if i >= len {
            // `>` が見つからなかった → タグではないのでそのまま出力
            out.push_str(&s[tag_start..i]);
            continue;
        }
        let inner = &s[tag_start + 1..i]; // `<` と `>` の間
        i += 1; // skip '>'
        let upper = inner.to_ascii_uppercase();
        match upper.as_str() {
            "LT" => out.push('<'),
            "GT" => out.push('>'),
            // 書式タグは除去（開始・終了どちらも）
            _ if matches!(
                upper.as_str(),
                "B" | "/B" | "I" | "/I" | "BIG" | "/BIG" | "SMALL" | "/SMALL" | "SIZE" | "/SIZE"
            ) => {}
            _ if upper.starts_with("SIZE=") || upper.starts_with("COLOR=") || upper == "/COLOR" => {
            }
            // 未知のタグはそのまま残す（`<` `>` を含め復元）
            _ => {
                out.push('<');
                out.push_str(inner);
                out.push('>');
            }
        }
    }
    out
}

/// Talk / Question ボディに含まれるダッシュ文字列を正規化する。
///
/// SRC.Sharp `Expression.replace.cs` `FormatMessage` 準拠:
/// `ー` (U+30FC)、`―` (U+2015)、`─` (U+2500) が **連続して2文字以上** 続く場合、
/// 2 文字ずつ `──` (U+2500 x2) に変換する。
///
/// 単独のダッシュ文字 (前後が非ダッシュ) は変換しない。
/// 例: `ボーダー` → 変換なし (`ー` は非連続)。
/// 例: `テスト――テスト` → `テスト──テスト`。
/// 例: `ーーー` (3 文字) → `──ー` (2 + 1 に分割)。
fn normalize_dashes(s: &str) -> String {
    let is_dash = |c: char| matches!(c, '\u{2500}' | '\u{2015}' | '\u{30FC}');
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if is_dash(chars[i]) && i + 1 < chars.len() && is_dash(chars[i + 1]) {
            // 連続するダッシュペアを `──` (U+2500 x2) に変換
            out.push('\u{2500}');
            out.push('\u{2500}');
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

/// 色文字列を空白除去 + 小文字化して正規化する (Fade 色一致判定用)。
fn normalized_color(c: &str) -> String {
    c.chars()
        .filter(|ch| !ch.is_whitespace())
        .collect::<String>()
        .to_ascii_lowercase()
}

/// `WhiteOut` が積む白の全画面 Fade 色か (`WhiteIn` の除去対象)。
fn is_white_fade_color(c: &str) -> bool {
    matches!(
        normalized_color(c).as_str(),
        "#ffffff" | "#fff" | "white" | "rgb(255,255,255)"
    )
}

/// `FadeOut` が積む黒の全画面 Fade 色か (`FadeIn` の除去対象)。
fn is_black_fade_color(c: &str) -> bool {
    matches!(
        normalized_color(c).as_str(),
        "#000000" | "#000" | "black" | "rgb(0,0,0)"
    )
}

/// 関数引数 1 個を「裸の識別子なら変数として解決、クオートなら literal」で
/// 評価して文字列値を返す。クオート未付き / `$()` 未展開の識別子も変数として
/// 解決する SRC 流のセマンティクスを模倣。
/// `Color` 命令の文字列を CSS カラーに正規化。
/// `RGB(r,g,b)` 形式や `rgb(r,g,b)` 形式はそのまま、英数色名は小文字化。
/// 色トークンを解決する。`FrameColor1` のように色がスクリプト変数で
/// 与えられる場合 (Alpha2ndStatus.ini 由来のテーマ色) はその値を引いてから
/// `canonical_color` を適用する。定義済み変数でなければそのまま色名として扱う。
/// 図形描画命令 (Circle/Oval/Arc) の末尾 color オプションを解決する。
/// 残余トークンが空でなければ結合して `resolve_color` で CSS カラーへ。
/// 空（色省略）なら `None` を返し、呼び出し側は現在の線色を据え置く。
/// SRC は color を `#rrggbb` 形式で受けるが、本移植では `RGB(...)` が
/// 引数評価で `#rrggbb` に評価済み・`resolve_color` が日本語色名/変数色も解決する。
fn optional_draw_color(app: &App, rest: &[String]) -> Option<String> {
    let joined = rest
        .iter()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    if joined.is_empty() {
        None
    } else {
        Some(resolve_color(app, &joined))
    }
}

fn resolve_color(app: &App, s: &str) -> String {
    let t = s.trim().trim_matches('"').trim();
    let v = app.script_var(t);
    if v.is_empty() {
        canonical_color(t)
    } else {
        canonical_color(v.trim())
    }
}

fn canonical_color(s: &str) -> String {
    let trimmed = s.trim();
    if trimmed.starts_with("rgb(") || trimmed.starts_with("RGB(") {
        return trimmed.to_lowercase();
    }
    let lower = trimmed.to_ascii_lowercase();
    // 日本語色名 → CSS
    match trimmed {
        "黒" | "Black" => return "#000000".to_string(),
        "白" | "White" => return "#ffffff".to_string(),
        "赤" | "Red" => return "#ff0000".to_string(),
        "緑" | "Green" => return "#00ff00".to_string(),
        "青" | "Blue" => return "#0000ff".to_string(),
        "黄" | "Yellow" => return "#ffff00".to_string(),
        "灰" | "Gray" | "Grey" => return "#808080".to_string(),
        _ => {}
    }
    if trimmed.starts_with('#') {
        return trimmed.to_string();
    }
    lower
}

// 数値 (f64) を `0.0` のような末尾無駄を削った文字列で整形。
// ──────────────────────────────────────────────────────────────────────────────
// Shift-JIS バイト列操作ヘルパー
// SRC の *B 系関数 (LenB / LeftB / RightB / MidB / InStrB / InStrRevB) は
// Shift-JIS エンコーディング基準のバイト位置を使う。
// VB5 実装と同様に ASCII 文字=1バイト、それ以外 (日本語等)=2バイトで近似する。
// ──────────────────────────────────────────────────────────────────────────────

/// `UnitInstance` の `active_features` / `abilities` を `UnitData` から初期化する。
/// `Place` / `Create` 後に呼び出すことで、ユニット特殊能力が `IsAvailable` /
/// `make_unit_cost_fn` (地形適応) 等から、アビリティがユニットメニュー /
/// `UseAbility` から参照可能になる。
fn populate_active_features(inst: &mut UnitInstance, app: &App) {
    if let Some(unit_data) = app.database().unit_by_name(&inst.unit_data_name).cloned() {
        // 必要技能.md §3: ユニット用特殊能力 `特殊能力名=値 (必要技能)`。値末尾の
        // スペース区切り `(必要技能)`/`<必要条件>` を剥がして評価し、満たさなければ
        // その特殊能力を無効 (is_active=false) にする。未モデル種別は fail-open
        // (is_satisfied が true) なので誤封印しない。武器/アビリティ/形態ゲートと同じ評価器。
        let new_feats = crate::necessary_skill::gated_active_features(
            &unit_data.features,
            inst,
            app.database(),
        );
        inst.active_features = new_feats;
        // アビリティの実行時状態 (残り回数) を静的データから初期化。index は
        // `UnitData.abilities` と対応させ、使用時にメタデータを引けるようにする。
        inst.abilities = unit_data
            .abilities
            .iter()
            .map(|a| {
                let mut ua =
                    crate::unit_ability::UnitAbility::new(a.name.clone(), a.effect.clone());
                ua.stock_remaining = a.uses;
                ua
            })
            .collect();
    }
}

fn find_unit<'a>(app: &'a App, key: &str) -> Option<&'a UnitInstance> {
    let key = key.trim().trim_matches('"');
    app.database()
        .unit_instances
        .iter()
        .find(|u| matches_unit_handle(u, key))
}

fn parse_party_label(s: &str) -> Option<crate::Party> {
    match s {
        "Player" | "味方" => Some(crate::Party::Player),
        "Enemy" | "敵" => Some(crate::Party::Enemy),
        "Neutral" | "中立" => Some(crate::Party::Neutral),
        // SRC 正準は "ＮＰＣ"。"友軍"/"Allied" は旧移植の後方互換エイリアス。
        "NPC" | "ＮＰＣ" | "Allied" | "友軍" => Some(crate::Party::Npc),
        _ => None,
    }
}

/// 簡易 PRNG (splitmix64 of script_vars + unit count)。決定論的だが、
/// 同 App 内でステップごとに変化する程度の擬似乱数。
fn pseudo_random(app: &App) -> u32 {
    let seed = (app.script_vars().len() as u64)
        .wrapping_add(app.database().unit_instances.len() as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(app.messages().len() as u64);
    let mut z = seed;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    (z ^ (z >> 31)) as u32
}

/// `PaintString` の引数を解析して (x, y, text) を返す。
/// 受け付ける形:
/// - `PaintString x y text...`
/// - `PaintString text x y` (元 SRC で散見)
///
/// 先頭 2 トークンが両方整数なら (x, y, 残り)、そうでなく
/// 最後 2 トークンが両方整数なら (x, y, 先頭) として扱う。
fn parse_paintstring_args(xargs: &[String]) -> (f64, f64, String) {
    // SRC PaintString は座標に `-` を許容（マップウィンドウ中央寄せの意）。
    // 元 SRC のマップウィンドウは 480×480 なので、その中央 (240, 240) を
    // デフォルト座標として用いる。本実装のキャンバスは 640×480。
    const DEFAULT_X: f64 = 240.0;
    const DEFAULT_Y: f64 = 240.0;
    let parse_coord = |s: &str| -> Option<f64> {
        let t = s.trim();
        if t == "-" {
            return None;
        }
        // 引数は評価済みなので数値として読むだけ (原典 `CInt(GetArgAsString(i))`)。
        Some(eval::str_to_lng(t) as f64)
    };
    fn is_coord_tok(s: &str) -> bool {
        let t = s.trim();
        t == "-"
            || t.parse::<f64>().is_ok()
            // `(...)` で囲まれた式は座標式とみなす。中身にユーザ定義関数
            // (`SCX(...)` 等) や変数を含んでいても、PaintString の第 1・2 引数は
            // 座標という構文上の前提から coord として扱う (text に流出させない)。
            || (t.starts_with('(') && t.ends_with(')') && t.len() >= 2)
            || {
                // 算術式風 ("x*y" / "(N \ 2)" 等) なら true。演算子集合は
                // `is_arith_operator_char` に一元化 (`\` `^` の取りこぼし防止)。
                t.chars().all(|c| {
                    c.is_ascii_digit() || is_arith_operator_char(c) || c == '.' || c.is_whitespace()
                }) && t.chars().any(|c| c.is_ascii_digit())
            }
    }
    if xargs.len() >= 3 && is_coord_tok(&xargs[0]) && is_coord_tok(&xargs[1]) {
        let x = parse_coord(&xargs[0]).unwrap_or(DEFAULT_X);
        let y = parse_coord(&xargs[1]).unwrap_or(DEFAULT_Y);
        return (x, y, xargs[2..].join(" "));
    }
    if xargs.len() >= 3 {
        let last = xargs.len();
        if is_coord_tok(&xargs[last - 2]) && is_coord_tok(&xargs[last - 1]) {
            let x = parse_coord(&xargs[last - 2]).unwrap_or(DEFAULT_X);
            let y = parse_coord(&xargs[last - 1]).unwrap_or(DEFAULT_Y);
            return (x, y, xargs[..last - 2].join(" "));
        }
    }
    (DEFAULT_X, DEFAULT_Y, xargs.join(" "))
}

/// `Require <path>` の対象ファイルを script_library から basename で引き、
/// その範囲のトップレベル `key = value` 代入をすべてスクリプト変数へ適用する。
/// `.ini` 形式の設定ファイル (純粋な代入文の集合) を想定する。
fn apply_required_file(app: &mut App, path: &str) {
    let Some(fe) = app.script_library().find_file(path) else {
        return;
    };
    let (start, end) = (fe.start_pc, fe.end_pc);
    // 借用衝突を避けるため代入文を先に収集する。
    let assigns: Vec<(String, String)> = app.script_library().statements[start..end]
        .iter()
        .filter_map(|s| match s {
            EventStatement::Command { name, args, .. } => {
                // `key = value` (代入糖衣: 第 1 引数が裸の `=`) のみ取り込む。
                if args.first().map(String::as_str) == Some("=") {
                    Some((name.clone(), args[1..].join(" ")))
                } else {
                    None
                }
            }
            _ => None,
        })
        .collect();
    for (key, raw) in assigns {
        let value = raw.trim().trim_matches('"').trim().to_string();
        app.set_script_var(key, value);
    }
}

/// `src` の byte index `i` から、その位置の文字終端 byte index を返す。
fn next_char_end(src: &str, i: usize) -> usize {
    src[i..]
        .char_indices()
        .nth(1)
        .map(|(off, _)| i + off)
        .unwrap_or(src.len())
}

fn expect_arg<'a>(
    args: &'a [String],
    i: usize,
    line: usize,
    usage: &str,
) -> Result<&'a str, ScriptError> {
    args.get(i)
        .map(|s| s.as_str())
        .ok_or_else(|| err(line, &format!("引数不足: {usage}")))
}

/// `PaintPicture` の引数がオプションキーワード (透過 / 反転 / 白黒 等) か判定。
/// w/h を省略した `PaintPicture img - - 透過` 形式で、オプションが w/h の位置に
/// 現れたときに数値引数と取り違えないために使う。data 中に出る `暗` / `夕焼け`
/// (現状は描画未対応だがオプション扱い) も含めて region 判定だけは正しく行う。
fn is_paint_option(s: &str) -> bool {
    let l = s.to_ascii_lowercase();
    matches!(
        s,
        "透過"
            | "左右反転"
            | "上下反転"
            | "白黒"
            | "セピア"
            | "背景"
            | "保持"
            | "右回転"
            | "左回転"
            | "上半分"
            | "下半分"
            | "左半分"
            | "右半分"
            | "右上"
            | "左上"
            | "右下"
            | "左下"
            | "暗"
            | "夕焼け"
    ) || matches!(
        l.as_str(),
        "transparent"
            | "flip"
            | "flipx"
            | "fliph"
            | "flipy"
            | "flipv"
            | "monochrome"
            | "grayscale"
            | "sepia"
            | "background"
            | "persist"
            | "keep"
            | "rotate"
            | "rotateright"
            | "rotateleft"
    )
}

/// 評価済みの座標/サイズ引数を `u32` として読む (原典 `CInt(GetArgAsString(i))`)。
/// 負値は 0 にクランプ。引数が無い / 数値でなければ 0。`Create` / `SetTile` 等の座標で使う。
fn eval_coord_u32(args: &[String], i: usize) -> u32 {
    args.get(i).map_or(0, |s| eval::str_to_lng(s).max(0) as u32)
}

/// 評価済みの引数を整数として読む。本移植独自のデータ定義形式 (`MapSize` / `Turn` /
/// 12 引数の `Pilot` / 14 引数の `Unit`) の数値欄用で、数値でなければエラーにする。
fn parse_int(s: &str, line: usize) -> Result<i64, ScriptError> {
    if eval::is_numeric(s) {
        Ok(eval::str_to_lng(s))
    } else {
        Err(err(line, &format!("数値として解釈できません: {s:?}")))
    }
}

fn parse_u32(args: &[String], i: usize, line: usize) -> Result<u32, ScriptError> {
    let s = expect_arg(args, i, line, "<u32>")?;
    Ok(parse_int(s, line)?.clamp(0, i64::from(u32::MAX)) as u32)
}

fn parse_i32_at(s: &str, line: usize) -> Result<i32, ScriptError> {
    Ok(parse_int(s, line)?.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
}

fn parse_i64_at(s: &str, line: usize) -> Result<i64, ScriptError> {
    parse_int(s, line)
}

/// 陣営名引数を `Party` に解決する。
///
/// 原典 SRC はコマンド引数を項として評価する
/// (`CmdData.GetArgAsString` → `Expression.GetValueAsString(arg, is_term: true)`)。
/// このため陣営名にも変数を渡せる:
///
/// ```text
/// Local ＲＧparty = Args(1)
/// Create ＲＧparty ＲＧunit 0 ＲＧpilot ＲＧlevel ＲＧx ＲＧy
/// ```
///
/// 引数は評価済み (`GetArgAsString`) で渡る。未定義の名前は名前自身になるので、
/// `Create 味方 …` のような定数指定は影響を受けない。
fn parse_party(s: &str, line: usize) -> Result<Party, ScriptError> {
    let resolved = s.to_string();
    match resolved.as_str() {
        "Player" | "味方" => Ok(Party::Player),
        // ＮＰＣ はコンピューター操作のプレイヤー側陣営。SRC 正準名は "ＮＰＣ"。
        // "友軍"/"Allied" は旧移植の後方互換エイリアス。
        "NPC" | "ＮＰＣ" | "Allied" | "友軍" => Ok(Party::Npc),
        "Enemy" | "敵" => Ok(Party::Enemy),
        "Neutral" | "中立" => Ok(Party::Neutral),
        _ => Err(err(line, &format!("Party 値が不正: {resolved:?}"))),
    }
}

fn err(line_num: usize, message: &str) -> ScriptError {
    ScriptError {
        line_num,
        message: message.to_string(),
    }
}

mod blocks;
mod commands;
mod eval;
use eval::condition_with_calls;
mod expression;
mod functions;
mod info;
mod labels;
mod library;
mod unit_effects;
mod vars;

use blocks::*;
use expression::*;
use functions::*;
use info::*;
pub use labels::*;
pub use library::*;
use unit_effects::*;
pub use vars::*;

#[cfg(test)]
mod tests;
