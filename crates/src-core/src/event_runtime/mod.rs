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
/// 条件式は `expand_vars` で関数呼び出し / 変数参照 / 比較演算子を解決後、
/// `try_eval_int` で整数化する。0 → 非表示、それ以外 (1 / 正の値) → 表示。
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
    let expanded = expand_vars(app, &processed);
    // 数値ならそのまま 0 判定。expand_vars が `1` / `0` / `False` 等の
    // 文字列に解決するケースを順に許容する。
    if let Some(n) = try_eval_int(&expanded) {
        return n != 0;
    }
    let s = expanded.trim().to_ascii_lowercase();
    !matches!(s.as_str(), "" | "0" | "false" | "no" | "いいえ")
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
            } => {
                if canonical_label_full(&name, &args).is_some() {
                    curr_pc += 1;
                    continue;
                }
                match exec_command_pc(app, &name, &args, line_num, curr_pc, stmts, labels) {
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
/// (`expand_vars` / `fn_arg_value`) は `&App` (不変) しか持たずサブルーチンを
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
                .map(|a| a.trim().to_string())
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
fn exec_command_pc(
    app: &mut App,
    name: &str,
    args: &[String],
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
        return exec_command_pc(
            app,
            "__assign",
            &assign_sugar_args(name, &rhs),
            line,
            pc,
            stmts,
            labels,
        );
    }
    // 各引数を $(var) / Args(N) / 関数呼出 / インデックス変数で展開後、
    // `&` 連結演算子を畳み込む（`"abc" & x & ".bmp"` → "abcvalue.bmp"）。
    // 式展開の前にユーザ定義関数呼び出しを解決して値へ畳み込む
    // (`expand_arg` は `&App` しか持たずサブルーチンを実行できないため)。
    let args: Vec<String> = args
        .iter()
        .map(|a| preprocess_user_functions(app, a))
        .collect();
    let expanded: Vec<String> = args.iter().map(|a| expand_arg(app, a)).collect();
    let xargs: Vec<String> = collapse_concat(expanded);
    // 制御フロー命令を case-insensitive で先に処理。
    let lname = name.to_ascii_lowercase();
    match lname.as_str() {
        "goto" => {
            let target = expect_arg(&xargs, 0, line, "Goto <label>")?;
            return jump_to(app, pc, target, line);
        }
        "if" => {
            // `If lhs op rhs Then` の Then は省略可。条件が偽なら対応する
            // ElseIf / Else / EndIf までスキップ。
            //
            // 単一行 If 形式もサポート:
            //   `If cond Then stmt`  / `If (cond) stmt`  / `If lhs op rhs stmt`
            // この場合は EndIf を持たず、条件が真なら stmt をその場で実行、
            // 偽なら何もせず次行へ進む。
            // 条件 / 本体の境界は **展開前** の `args` で判定する。
            // `expand_vars` 後の `xargs` は `Instr(...)` 等の関数呼出が値に
            // 潰れており、`If Instr(...) Exit` の「条件 1 トークン + 本体」
            // 構造が失われてブロック If と誤認されてしまうため。
            let (raw_cond, raw_body) = split_if_cond_body(&args);
            let cond_len = raw_cond.len();
            // 本体開始位置 (cond と body の間に `Then` が挟まる場合がある)。
            let body_start = args.len().saturating_sub(raw_body.len());
            let (cond_tokens, body_tokens): (Vec<String>, Vec<String>) =
                if xargs.len() == args.len() && body_start <= xargs.len() {
                    (xargs[..cond_len].to_vec(), xargs[body_start..].to_vec())
                } else {
                    // `&` 連結等で展開後トークン数がズレた場合は xargs で分割。
                    split_if_cond_body(&xargs)
                };
            // `Call(<label>)` を含む条件トークンを事前に同期実行で置換する。
            // `eval_condition_args_with` は `&App` を取るため、その前に処理が必要。
            let preprocessed_cond_tokens: Vec<String> = {
                let joined = cond_tokens.join(" ");
                if joined.to_ascii_lowercase().contains("call(") {
                    let replaced = preprocess_call_expressions_in_condition(app, &joined);
                    split_balanced(&replaced)
                } else {
                    cond_tokens.clone()
                }
            };
            let cond = eval_condition_args_with(app, &preprocessed_cond_tokens);
            return if body_tokens.is_empty() {
                if cond {
                    Ok(pc + 1)
                } else {
                    skip_to_else_or_endif(app, pc, stmts, line)
                }
            } else if cond {
                // body を inline 実行。args は expand 済なので二重展開を避け
                // られないが、リテラルは idempotent なので実害は小さい。
                let body_name = body_tokens[0].clone();
                let body_rest: Vec<String> = body_tokens[1..].to_vec();
                exec_command_pc(app, &body_name, &body_rest, line, pc, stmts, labels)
            } else {
                Ok(pc + 1)
            };
        }
        "elseif" => {
            // 直前の Then 節を実行し終えたら ElseIf 以降は全て EndIf までスキップ。
            return skip_to_endif(pc, stmts, line);
        }
        "else" => {
            return skip_to_endif(pc, stmts, line);
        }
        "endif" => {
            return Ok(pc + 1);
        }
        "set" | "local" | "__assign" => {
            // Set var value...  (value が複数トークンの場合はスペース連結)
            // `__assign` は `var = expr` 形 (assign sugar が生成する内部コマンド名)。
            // SRC の `=` 代入は RHS が常に式なので括弧無し算術も数値化する (下記)。
            // SRC `Localコマンド` は 2 書式を持つ (C# `LocalCmd.cs`)。
            //   書式1 `Local var1 var2 …` — 各変数を空文字で宣言する
            //   書式2 `Local var = expr`  — 1 変数を式の値で初期化する
            // 書式2 の `=` は値の一部ではない。従来は `Local p = Args(1)` の値が
            // `"= <値>"` になり、`Create p …` 等で「Party 値が不正」を招いていた。
            let local_assign = lname == "local" && args.len() >= 3 && args[1].trim() == "=";
            let is_assign_form = lname == "__assign" || local_assign;
            if args.is_empty() {
                return Err(err(line, "Set/Local には変数名が必要。"));
            }
            if lname == "local" && !local_assign {
                // 書式1: 宣言のみ。SRC は新しいサブルーチンローカル変数を空で作る。
                for name in args.iter() {
                    let key = resolve_lhs_name(app, name);
                    if !key.is_empty() {
                        // サブルーチンを抜けたら宣言前の値へ戻すため退避する。
                        app.record_sublocal(&key);
                        app.set_script_var(key, String::new());
                    }
                }
                return Ok(pc + 1);
            }
            // 書式2 では値は `=` の次から始まる。
            let val_start = if local_assign { 2 } else { 1 };
            // 書式2 `Local var = expr` もサブルーチンローカル宣言なので退避する。
            if local_assign {
                let key = resolve_lhs_name(app, &args[0]);
                if !key.is_empty() {
                    app.record_sublocal(&key);
                }
            }
            // LHS は `name[expr]` の `expr` を eval して実際のキーに解決する。
            // expand_vars 後の xargs[0] は「変数値」展開済みで Set には使えないので
            // 元の args[0] から名前を組み立てる。
            //
            // 動的 LHS: `Set Eval(x) value` — Eval(x) は変数 x の値（= 変数名）を
            // 取り出してそれを実際の LHS として扱う。
            let var = if let Some(inside) = args[0]
                .strip_prefix("Eval(")
                .or_else(|| args[0].strip_prefix("eval("))
            {
                if let Some(name_expr) = inside.strip_suffix(')') {
                    // 1) 関数 / `$(...)` を含むなら expand_vars で値を取り出す。
                    // 2) 裸の識別子なら script_var として引く。
                    // 3) いずれも失敗したら literal をキーとして使う。
                    let expanded = expand_vars(app, name_expr.trim());
                    let key = if expanded.trim() != name_expr.trim() {
                        expanded
                    } else {
                        let v = app.script_var(name_expr.trim());
                        if !v.is_empty() {
                            v.to_string()
                        } else {
                            name_expr.trim().to_string()
                        }
                    };
                    resolve_lhs_name(app, &key)
                } else {
                    resolve_lhs_name(app, &args[0])
                }
            } else {
                resolve_lhs_name(app, &args[0])
            };
            let value = if xargs.len() > val_start {
                // `#` コメントを除去 (SRC.Sharp `SetCmd.cs` 後方互換性):
                // `Set var value # comment` 形式で `#` 以降を無視する。
                // 重要: チェック対象は xargs[2] (値の直後のトークン) のみ。
                // xargs[1] が `#` で始まる場合は色コード (`#3264c8` 等) であり
                // コメントとは区別する (この場合 xargs.len()==2 で下記 end=2 分岐)。
                let end = if xargs.len() > val_start + 1 && xargs[val_start + 1].starts_with('#') {
                    val_start + 1 // `#` コメント発見 → 値は 1 トークンのみ
                } else {
                    xargs.len()
                };
                if end == val_start + 1 {
                    // 単一トークン値 (`Set マップ決定 選択` 形式) は SRC 同様に
                    // 裸の識別子を script_var として自動解決する。`fn_arg_value`
                    // は「変数として引いて空でなければ採用、そうでなければ literal」
                    // という SRC 流の値評価セマンティクス。
                    fn_arg_value(app, &xargs[val_start])
                } else {
                    xargs[val_start..end].join(" ")
                }
            } else if lname == "set" {
                // SRC `SetCmd`: 値なし `Set var` はフラグとして 1 を代入する
                // (`Set 機体確認` → `If 機体確認` が真)。`Local var` は空文字。
                "1".to_string()
            } else {
                String::new()
            };
            // SRC の `Set` (ExecSetCmd) / `=` 代入は値を `EvalTerm` で式評価し、結果が数値型
            // なら数値代入する。`expand_vars` は関数呼出を評価するが算術式はトークンとして残す。
            // - `var = expr` 形 (assign sugar → `__assign`): RHS は常に式 (SRC の `=` 代入)。
            //   括弧無しの算術 (`資本金 = 資本金 - 営業収支` / `HP = HP - 10`) もここで数値化する。
            // - `Set var value` 形: 値が引用符付き文字列 (`"$(a)-$(b)"`) や Format の整形済み
            //   出力 (`-05`) でもあり得るため、従来どおり**括弧付き算術のみ**を数値化し、
            //   それ以外は文字列のまま残す (パーサが引用符を剥がすため形では区別できない)。
            let value = if is_assign_form {
                eval_arith_value(app, &value).unwrap_or(value)
            } else {
                eval_paren_arith_value(app, &value).unwrap_or(value)
            };
            // 関数左辺値代入 (`HP(unit) = n` 等) を先に処理。
            // 対象を見つけた場合は set_script_var は呼ばない。
            if try_function_lhs_assign(app, &var, &value) {
                return Ok(pc + 1);
            }
            // システム変数書き込み (`ターン数` / `総ターン数` / `資金` 等)。
            if try_system_var_assign(app, &var, &value) {
                return Ok(pc + 1);
            }
            app.set_script_var(var, value);
            return Ok(pc + 1);
        }
        "talk" => {
            // Talk [character position option]
            // SRC 仕様: 最初の引数のみがキャラクター名（話者）。
            // それ以降の引数は位置指定 (X Y / 母艦 / 中央 / 固定) や
            // 表示オプションであり、話者名には含めない。
            // 例: `Talk システム 8 8` → speaker="システム"、座標 (8,8) は無視。
            //     `Talk 霊夢 (8,6)`   → speaker="霊夢"、座標 (8,6) は無視。
            let speaker = xargs.first().cloned().unwrap_or_default();
            let (raw_body, next_pc) = collect_until_end(app, pc + 1, stmts);
            // SRC の Talk ボディに含まれる HTML 書式タグを除去。
            // `<B>...</B>` / `<COLOR=Red>` 等は SRC のリッチテキスト用であり、
            // プレーンテキスト描画では不要。`<LT>` / `<GT>` は `<` / `>` に変換。
            // 続いてダッシュ文字正規化 (`――` → `──` 等、SRC.Sharp FormatMessage 準拠)。
            let body = normalize_dashes(&strip_talk_tags(&raw_body));
            // SRC `Talkコマンド.md`: 半角 `;` は強制改行、半角 `:` は段階表示の
            // 区切り (メッセージを一部ずつ順に表示)。全角 `；`/`：` は通常文字。
            let body = body.replace(';', "\n");
            let pages: Vec<String> = body
                .split(':')
                .map(|p| p.trim_matches(['\n', ' ', '\u{3000}']).to_string())
                .filter(|p| !p.is_empty())
                .collect();
            // メッセージログには段階表示を 1 行に畳んで積む (HUD 一行表示用)。
            let log_body = pages.join(" ").replace('\n', " ");
            let log_msg = if speaker.is_empty() {
                log_body.clone()
            } else {
                format!("【{speaker}】{log_body}")
            };
            if !log_body.is_empty() {
                app.push_message(log_msg);
            }
            // モーダル表示。先頭ページを Talk に出し、残りは talk_pages に積んで
            // クリック応答ごとに 1 ページずつ送る。空 body はスキップ。
            if let Some((first, rest)) = pages.split_first() {
                app.set_talk_pages(speaker.clone(), rest.to_vec());
                app.set_pending_dialog(crate::dialog::PendingDialog::Talk {
                    speaker,
                    body: first.clone(),
                });
            }
            return Ok(next_pc);
        }
        "end" => {
            // Talk 外の End はサブルーチン終端マーカー。本実装ではトップレベル
            // 実行を続行（Return 相当）するだけ。
            return Ok(pc + 1);
        }
        "exit" => {
            // 元 SRC `ExitCmd`: 現イベントを即終了。
            // PC をスクリプト末尾に飛ばして execute ループを抜ける。
            return Ok(stmts.len());
        }
        "return" => {
            // Call 中なら戻りアドレスへ + 呼び出し元の Args を復元。
            // それ以外は Exit と同じ。
            //
            // 戻り値 (`Return <value>`) を保存する。
            // `Call(<label>)` 形式の条件式評価 (`evaluate_command_condition`) が
            // `call_label_sync_for_condition` 経由で読み取る。
            // 返り値は **呼び出された側のスコープで** 評価する。
            // `Returnコマンド.md`: 「*value* の値がサブルーチンコールの返り値」。
            // ここで畳み込まないと `Return (n + 2)` の `n` (サブルーチン
            // ローカル変数) が呼び出し元では解決できず、式のまま漏れる。
            let retval = xargs.first().cloned().unwrap_or_default();
            let retval = eval_arith_value(app, &retval).unwrap_or(retval);
            app.set_last_return_value(retval);
            if let Some((ret, saved)) = app.pop_call_return() {
                restore_call_args(app, saved);
                return Ok(ret);
            }
            return Ok(stmts.len());
        }
        "call" => {
            // `Call label [arg1 arg2 ...]`
            // 戻りアドレスは現命令の次。Args(1).. をシナリオ変数として束縛。
            let target = expect_arg(&xargs, 0, line, "Call <label> [args...]")?;
            let new_args: Vec<String> = xargs.iter().skip(1).cloned().collect();
            let saved = enter_call_args(app, &new_args);
            app.push_call_return(pc + 1, saved);
            return jump_to(app, pc, target, line);
        }
        "switch" => {
            // `Switch value` の value 解決:
            //   - 単一トークン (`Switch 選択`) は裸識別子を script_var として
            //     解決する。SRC では Switch の値は式評価される。
            //   - 複数トークン (`Switch $(a) + 1`) は join したものを value にする。
            // SRC.Sharp 準拠: 引数なしはエラー。
            if xargs.is_empty() {
                return Err(err(line, "Switchコマンドの引数の数が違います"));
            }
            let value = if xargs.len() == 1 {
                fn_arg_value(app, &xargs[0])
            } else {
                xargs.join(" ")
            };
            return find_matching_case(pc, stmts, &value, line);
        }
        "case" | "caseelse" => {
            // 直前ケース本体の末尾に到達 → 同階層の EndSw までスキップ。
            return skip_to_endsw(pc, stmts, line);
        }
        "endsw" => {
            return Ok(pc + 1);
        }
        "for" => {
            // `For var = start To end [Step n]`
            // xargs: [var, "=", start, "To", end, ("Step", n)?]
            if xargs.len() < 5 || xargs[1] != "=" || !xargs[3].eq_ignore_ascii_case("to") {
                return Err(err(line, "For 構文: For var = start To end [Step n]"));
            }
            let var = xargs[0].clone();
            // SRC は `For i = 1 To Info(...)` で Info() が "" を返すと
            // ループを実行せずに次へ進む。空文字 / 非数値は 0 と解釈してその
            // 旨を反映する (start > end → 即ループスキップ)。
            // 終端等の式は `eval_int_expr_app` で評価する。`Set` は算術を
            // 評価せず literal を保持するため、`For i = 1 To 敵配置数` の
            // `敵配置数` が `(6 + (ダンジョン進行度 / 5))` のように裸変数入りの
            // 式文字列であることがある。app-aware 評価で変数も解決する。
            let parse_lenient = |app: &App, s: &str| -> i64 {
                let t = s.trim();
                if t.is_empty() {
                    return 0;
                }
                t.parse::<i64>()
                    .unwrap_or_else(|_| i64::from(eval_int_expr_app(app, t)))
            };
            let start = parse_lenient(app, &xargs[2]);
            let end = parse_lenient(app, &xargs[4]);
            let step = if xargs.len() >= 7 && xargs[5].eq_ignore_ascii_case("step") {
                parse_lenient(app, &xargs[6])
            } else {
                1
            };
            if step == 0 {
                return Err(err(line, "For の Step 値が 0。"));
            }
            app.set_script_var(var.clone(), start.to_string());
            // ループに 1 回も入らない条件
            let skip = (step > 0 && start > end) || (step < 0 && start < end);
            if skip {
                return skip_to_next(pc, stmts, line);
            }
            app.push_for_frame(LoopFrame::Numeric {
                var,
                end,
                step,
                for_pc: pc,
            });
            return Ok(pc + 1);
        }
        "next" => {
            // For / ForEach フレームを参照してインクリメント。
            // 終了条件を満たせばフレームを pop、それ以外はループ先頭へ戻る。
            // SRC.Sharp 準拠: 対応する For がない場合はエラー。
            let Some(frame) = app.last_for_frame().cloned() else {
                return Err(err(line, "Nextコマンドに対応するForコマンドがありません"));
            };
            match frame {
                LoopFrame::Numeric {
                    var,
                    end,
                    step,
                    for_pc,
                } => {
                    let current = app.script_var(&var).parse::<i64>().unwrap_or(0);
                    let next_v = current + step;
                    let done = (step > 0 && next_v > end) || (step < 0 && next_v < end);
                    // SRC.Sharp 準拠: ループ変数は Next 実行時に必ず更新される。
                    // ループ終了条件を満たしても next_v を書き戻す (out-of-bounds 値)。
                    // 例: `For i = 3 To 1 Step -1` → 終了後 i = 0 (= 1 + (-1))
                    app.set_script_var(var, next_v.to_string());
                    if done {
                        app.pop_for_frame();
                        return Ok(pc + 1);
                    }
                    return Ok(for_pc + 1);
                }
                LoopFrame::Each {
                    var,
                    list,
                    index,
                    for_pc,
                } => {
                    let next_index = index + 1;
                    if next_index >= list.len() {
                        app.pop_for_frame();
                        return Ok(pc + 1);
                    }
                    app.set_script_var(var.clone(), list[next_index].clone());
                    if let Some(LoopFrame::Each { index: i, .. }) = app.last_for_frame_mut() {
                        *i = next_index;
                    }
                    return Ok(for_pc + 1);
                }
                LoopFrame::EachUnit {
                    idents,
                    index,
                    for_pc,
                } => {
                    let next_index = index + 1;
                    if next_index >= idents.len() {
                        app.pop_for_frame();
                        return Ok(pc + 1);
                    }
                    bind_foreach_unit(app, &idents[next_index]);
                    if let Some(LoopFrame::EachUnit { index: i, .. }) = app.last_for_frame_mut() {
                        *i = next_index;
                    }
                    return Ok(for_pc + 1);
                }
            }
        }
        "foreach" => {
            // SRC ForEach の書式:
            //  書式1: `ForEach group [status]` — グループのユニットを反復。
            //         ループ変数を持たず `対象ユニットＩＤ` / `対象パイロット`
            //         で参照する (スパロボ戦記 `Foreach 味方` 等)。
            //  書式2: `ForEach var [In] collection` — var にリスト要素を代入。
            // collection: 勢力ラベル ("Player"/"Enemy"/"味方"/...) なら該当 unit 一覧
            // それ以外はカンマ区切り or 空白区切りリスト
            if xargs.is_empty() {
                return Err(err(
                    line,
                    "ForEach 命令は引数が必要 (group | var collection)。",
                ));
            }
            let has_in = xargs.iter().any(|a| a.eq_ignore_ascii_case("in"));
            // 書式1 判定: `In` を持たず先頭が勢力ラベル / 「全」のとき。
            // (`ForEach u Player` のように先頭が変数名なら書式2 のまま。)
            let head = xargs[0].trim().trim_matches('"');
            let group_party = parse_party_label(head);
            // 書式1 判定: `In` を持たず、かつ
            //  (a) 既知の陣営ラベル / "全" / "all"、または
            //  (b) 1 引数で status 相当でない任意の識別子 (グループID として扱う)。
            // (b) の場合はグループ ID 一致フィルタは現状サポートしておらず、
            // 全ユニットを対象に反復する (graceful fallback)。
            let is_group_form = !has_in
                && (group_party.is_some()
                    || matches!(head, "全" | "all" | "All")
                    || (xargs.len() == 1 && !head.is_empty()));
            if is_group_form {
                // 書式1 の status (`出撃` / `待機` / `全て` / `(出撃 待機)` 等)。
                // 本実装のユニットは off_map false=出撃 / true=待機・格納・離脱 の
                // 2 状態。破壊ユニットは unit_instances から除去済み。
                let (incl_deployed, incl_offmap) = foreach_status_mask(&xargs[1..]);
                let idents: Vec<String> = app
                    .database()
                    .unit_instances
                    .iter()
                    .filter(|u| group_party.map_or(true, |p| u.party == p))
                    .filter(|u| (!u.off_map && incl_deployed) || (u.off_map && incl_offmap))
                    .map(|u| {
                        if u.uid.is_empty() {
                            u.unit_data_name.clone()
                        } else {
                            u.uid.clone()
                        }
                    })
                    .collect();
                if idents.is_empty() {
                    return skip_to_next(pc, stmts, line);
                }
                bind_foreach_unit(app, &idents[0]);
                app.push_for_frame(LoopFrame::EachUnit {
                    idents,
                    index: 0,
                    for_pc: pc,
                });
                return Ok(pc + 1);
            }
            if xargs.len() < 2 {
                return Err(err(line, "ForEach 命令は 2 引数必要 (var collection)。"));
            }
            let var = xargs[0].clone();
            // `In` キーワードは飛ばす
            let start_idx = if xargs
                .get(1)
                .map(|s| s.eq_ignore_ascii_case("in"))
                .unwrap_or(false)
            {
                2
            } else {
                1
            };
            if xargs.len() <= start_idx {
                return Err(err(line, "ForEach 命令: collection が無い。"));
            }
            let collection = xargs[start_idx..].join(" ");
            let list = collect_foreach_items(app, &collection);
            if list.is_empty() {
                return skip_to_next(pc, stmts, line);
            }
            app.set_script_var(var.clone(), list[0].clone());
            app.push_for_frame(LoopFrame::Each {
                var,
                list,
                index: 0,
                for_pc: pc,
            });
            return Ok(pc + 1);
        }
        "skip" => {
            // `Skip` — 対応するループの末尾 (Loop / Next) へジャンプ。
            // ループ継続 ("continue" 相当)。SRC.Sharp `SkipCmd.cs` 準拠:
            // 引数なし。ループ外で使用するとエラー。
            let after = skip_to_loop_or_next_end(pc, stmts, line)
                .map_err(|_| err(line, "Skipコマンドがループの外で使われています"))?;
            return Ok(after.saturating_sub(1));
        }
        "incr" => {
            // `Incr var [delta]` — delta 省略時は 1。
            //
            // delta は **非数値でもエラーにしない** (0 扱い)。SRC.Sharp の
            // `IncrCmd` は `GetArgAsDouble` を使い、非数値文字列は VB5 `Val()`
            // 流に 0 となる (例外を投げない)。実シナリオは
            // `Incr 仮変数 Mid(名前, i, 1)` のように 1 文字 (非数値) を
            // 渡してハッシュを計算する用途があり、ここでエラーにすると
            // スクリプト全体が異常終了してしまう (CMaking.eve が該当)。
            // SRC.Sharp 準拠: 引数 3 個以上はエラー。
            if args.is_empty() {
                return Err(err(line, "Incr には変数名が必要。"));
            }
            if args.len() > 2 {
                return Err(err(line, "Incrコマンドの引数の数が違います"));
            }
            // LHS は Set と同様に raw args[0] から resolve_lhs_name で実キーに
            // 解決する。expand_vars 済の xargs[0] は indexed 変数だと値へ化けて
            // しまい、`Incr 配列[i]` が別キー (要素の値) を破壊する。
            let var = resolve_lhs_name(app, &args[0]);
            // delta は f64 で計算 (SRC.Sharp は GetArgAsDouble を使用)
            let delta: f64 = if xargs.len() >= 2 {
                let s = xargs[1].trim();
                s.parse::<f64>()
                    .ok()
                    .or_else(|| try_eval_int(s).map(|v| v as f64))
                    .unwrap_or(0.0)
            } else {
                1.0
            };
            // システム変数の読み取り: script_vars より動的値を優先。
            let cur_str = system_variable_value(app, &var)
                .unwrap_or_else(|| app.script_var(&var).to_string());
            let cur: f64 = cur_str.parse::<f64>().unwrap_or(0.0);
            let result = cur + delta;
            // 整数結果は整数表記で保存 (SRC との互換性)
            let stored = if result.fract() == 0.0 {
                (result as i64).to_string()
            } else {
                result.to_string()
            };
            // システム変数への書き戻し。対象外なら通常の script_var に保存。
            if !try_system_var_assign(app, &var, &stored) {
                app.set_script_var(var, stored);
            }
            return Ok(pc + 1);
        }
        "telop" | "displaymessage" => {
            // SRC `Telop message` (`Telopコマンド.md`):
            // テロップ表示 (1 秒表示)。本実装は描画機構を持たないため、
            // message の中の `.` 区切りを改行に変換した文字列を push_message。
            // 接頭辞 `【テロップ】` で通常メッセージと区別する。
            // KeepBGM 連動 / Subtitle.mid 自動演奏は未実装。
            if !xargs.is_empty() {
                // 全引数を空白連結 (Telop はスペース許容で引用符不要)
                let raw = xargs.join(" ");
                // `.` を改行に変換 (1 回までの SRC 仕様だが、全置換で簡略化)
                let formatted = raw.replace('.', "\n");
                let lname_lc = lname.as_str();
                if lname_lc == "telop" {
                    app.push_message(format!("【テロップ】{formatted}"));
                } else {
                    app.push_message(formatted);
                }
            }
            return Ok(pc + 1);
        }
        "suspend" => {
            // SRC `Suspend` (C# `SuspendCmd` は `return NextID` = **何もせず次へ**):
            // Talk ブロック内では「メッセージウィンドウを残したままクリック待ち」の
            // 区切りとして機能する (= `collect_until_end` が Talk 終端として消費する)。
            // Talk 外の単独 `Suspend` は no-op (次へ進む) であり、ゲーム中断/Title 復帰
            // ではない (中断は `Quit` コマンドの役割)。旧実装は単独 Suspend で
            // scene=Title にしていたが、これは誤りで、cinematic 途中の Suspend が
            // タイトルへ飛ばしてしまっていた。
            return Ok(pc + 1);
        }
        "makepilotlist" => {
            // SRC `MakePilotList mode` (`MakePilotListコマンド.md`):
            // pilots を `mode` 能力値でソートし `パイロットリスト[1..N]` に名前を格納。
            // mode = レベル/ＳＰ/格闘/射撃/命中/回避/技量/反応/経験値 等。
            // 完全な UI 表示は本実装では行わず、script_var への格納のみ。
            let mode = xargs.first().cloned().unwrap_or_default();
            let pilot_names: Vec<String> = app
                .database()
                .pilots
                .iter()
                .map(|p| p.name.clone())
                .collect();
            let mut entries: Vec<(String, i64)> = pilot_names
                .iter()
                .filter_map(|pname| {
                    // effective_pilot_data でレベルアップ後スタットを使う
                    let ep = app.database().effective_pilot_data(pname)?;
                    let v: i64 = match mode.as_str() {
                        "レベル" | "Level" => app
                            .database()
                            .unit_instances
                            .iter()
                            .find(|u| u.pilot_name == *pname)
                            .map(|u| i64::from(crate::pilot_instance::level_from_exp(u.total_exp)))
                            .unwrap_or(1),
                        "ＳＰ" | "SP" => ep.sp.unwrap_or(0) as i64,
                        "格闘" | "Infight" => ep.infight as i64,
                        "射撃" | "Shooting" => ep.shooting as i64,
                        "命中" | "Hit" => ep.hit as i64,
                        "回避" | "Dodge" => ep.dodge as i64,
                        "技量" | "Technique" => ep.technique as i64,
                        "反応" | "Intuition" => ep.intuition as i64,
                        "経験値" | "Exp" => ep.exp_value as i64,
                        _ => 0,
                    };
                    Some((pname.clone(), v))
                })
                .collect();
            // 降順ソート (高い値が先)。等値は元順序維持。
            entries.sort_by_key(|a| std::cmp::Reverse(a.1));
            for (i, (name, _)) in entries.iter().enumerate() {
                let key = format!("パイロットリスト[{}]", i + 1);
                app.set_script_var(key, name.clone());
            }
            // `パイロットリスト数` も格納
            app.set_script_var("パイロットリスト数".to_string(), entries.len().to_string());
            return Ok(pc + 1);
        }
        "makeunitlist" => {
            // SRC `MakeUnitList mode` (`MakeUnitListコマンド.md`):
            // unit_instances (出撃/待機のユニット) を `mode` でソートし
            // `ユニットリスト[1..N]` に uid を格納。
            // C# Event.cs::MakeUnitList: SRC.UList を Status=="出撃"|"待機" でフィルタ。
            let mode = xargs.first().cloned().unwrap_or_default();
            let db = app.database();
            let mut entries: Vec<(String, i64)> = db
                .unit_instances
                .iter()
                .map(|inst| {
                    let v: i64 = match mode.as_str() {
                        "ＨＰ" | "HP" => {
                            let max = db.effective_max_hp(inst);
                            (max - inst.damage).max(0)
                        }
                        "ＥＮ" | "EN" => {
                            let max = db.effective_max_en(inst);
                            (max - inst.en_consumed).max(0) as i64
                        }
                        "装甲" | "Armor" => db.effective_armor(inst),
                        "運動性" | "Mobility" => db.effective_mobility(inst) as i64,
                        "移動力" | "Speed" => db.effective_speed(inst) as i64,
                        "最大攻撃力" | "MaxAttack" => db
                            .unit_by_name(&inst.unit_data_name)
                            .map(|u| u.weapons.iter().map(|w| w.power).max().unwrap_or(0))
                            .unwrap_or(0),
                        "最長射程" | "MaxRange" => db
                            .unit_by_name(&inst.unit_data_name)
                            .map(|u| {
                                u.weapons
                                    .iter()
                                    .map(|w| w.max_range as i64)
                                    .max()
                                    .unwrap_or(0)
                            })
                            .unwrap_or(0),
                        "レベル" | "Level" => {
                            i64::from(crate::pilot_instance::level_from_exp(inst.total_exp))
                        }
                        "気力" | "Morale" => inst.morale as i64,
                        _ => 0,
                    };
                    // ユニット識別子: uid 優先、無ければ unit_data_name
                    let ident = if inst.uid.is_empty() {
                        inst.unit_data_name.clone()
                    } else {
                        inst.uid.clone()
                    };
                    (ident, v)
                })
                .collect();
            let _ = db;
            entries.sort_by_key(|a| std::cmp::Reverse(a.1));
            for (i, (name, _)) in entries.iter().enumerate() {
                let key = format!("ユニットリスト[{}]", i + 1);
                app.set_script_var(key, name.clone());
            }
            app.set_script_var("ユニットリスト数".to_string(), entries.len().to_string());
            return Ok(pc + 1);
        }
        "bossrank" => {
            // SRC `BossRank [unit] rank` (`BossRankコマンド.md`): ボスランク (0〜5) を設定。
            // ユニットの `boss_rank` を更新 (HP/装甲/運動性/攻撃力強化 + 即死/石化/憑依無効 +
            // 衰/滅 半減は effective_* / 戦闘側が反映)。Rank() 関数互換のため script_var
            // `__rank_<unit>` にも保存する。unit 省略時は selected_unit_for_event。
            let (target, rank_str) = match xargs.len() {
                0 => return Ok(pc + 1),
                1 => (app.selected_unit_for_event().to_string(), xargs[0].clone()),
                _ => (xargs[0].clone(), xargs[1].clone()),
            };
            let rank: i32 = fn_arg_value(app, &rank_str)
                .parse()
                .unwrap_or(0)
                .clamp(0, 5);
            app.set_script_var(format!("__rank_{target}"), rank.to_string());
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target))
            {
                u.boss_rank = rank;
            }
            return Ok(pc + 1);
        }
        "win" | "gameclear" => {
            // SRC.Sharp 準拠: 引数ありはエラー。
            if !args.is_empty() {
                return Err(err(line, "Win/GameClearコマンドの引数の数が違います"));
            }
            app.set_stage_state(crate::stage::StageState::Victory);
            app.push_message("【勝利】".to_string());
            // 元 SRC では `Win` / `GameClear` 後にエンディング系イベントが
            // 自動発火する。`App::game_clear()` (UI 経由) と同等の lookup
            // 順 で 1 件発火。
            fire_victory_labels(app);
            return Ok(pc + 1);
        }
        "lose" | "gameover" => {
            // SRC.Sharp 準拠: 引数ありはエラー。
            if !args.is_empty() {
                return Err(err(line, "Lose/GameOverコマンドの引数の数が違います"));
            }
            app.set_stage_state(crate::stage::StageState::Defeat);
            app.push_message("【敗北】".to_string());
            fire_game_over_labels(app);
            return Ok(pc + 1);
        }
        "finish" => {
            // SRC `Finish [unit]` (`Finishコマンド.md` / C# `FinishCmd`): 指定
            // ユニットの行動を **1 回分終了** させるユニットコマンド。
            // ステージ終了 (Victory) ではない点に注意 (旧実装は Win/GameClear と
            // 混同して `stage_state=Victory` にしており、決戦！宇宙怪獣1話 の
            // スタートイベント `Finish ジェイド＝ソウマ` で開始即勝利になっていた)。
            //
            // unit 省略時は `selected_unit_for_event`。本実装の行動モデルは
            // boolean (`has_acted`, 最大 1 行動) なので has_acted=true が
            // 「1 行動消費」に相当する。既に行動済みなら no-op (冪等)。
            let key = match xargs.first() {
                Some(u) => u.clone(),
                None => app.selected_unit_for_event().to_string(),
            };
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &key))
            {
                u.has_acted = true;
            }
            return Ok(pc + 1);
        }
        "money" => {
            // `Money value` — 資金を value だけ増やす (SRC.Sharp: IncrMoney(value))。
            // value は常にデルタ値（差分）。マイナス値で減少。`+` プレフィックスは省略可能。
            // 例: `Money 30000` → 30000 追加、`Money -1000` → 1000 減少。
            // ※ 絶対値設定ではない (SRC.Sharp `MoneyCmd.cs` は常に IncrMoney を呼ぶ)。
            // C# MoneyCmd.cs: ArgNum != 2 → EventErrorException。
            if xargs.len() != 1 {
                return Err(err(line, "Moneyコマンドの引数の数が間違っています"));
            }
            if let Some(arg) = xargs.first() {
                let s = arg.trim();
                // `+` プレフィックスを除去してから parse（`-` は i64::parse が処理）
                let s = s.strip_prefix('+').unwrap_or(s);
                if let Ok(n) = s.parse::<i64>() {
                    app.add_money(n);
                }
            }
            return Ok(pc + 1);
        }
        "setstatus" => {
            // `SetStatus unit "状態"`
            if xargs.len() < 2 {
                return Err(err(line, "SetStatus 命令は 2 引数必要 (unit status)。"));
            }
            let target = xargs[0].clone();
            let status = xargs[1].clone();
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target))
            {
                u.add_condition(Condition::new(status, -1));
            }
            return Ok(pc + 1);
        }
        "unsetstatus" | "clearstatus" => {
            // SRC `ClearStatus [unit] <status>` (`ClearStatusコマンド.md`):
            // unit 省略時は `selected_unit_for_event` を使う。status を指定
            // して状態異常を解除する。`UnsetStatus` も同等の SRC 命令。
            // `setstatus` の逆操作。
            let (target, status) = match xargs.len() {
                0 => return Err(err(line, "ClearStatus 命令は最低 1 引数必要 (status)。")),
                1 => (app.selected_unit_for_event().to_string(), xargs[0].clone()),
                _ => (xargs[0].clone(), xargs[1].clone()),
            };
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target))
            {
                u.remove_condition(&status);
            }
            return Ok(pc + 1);
        }
        "transform" => {
            // SRC `Transform [unit] mode` (`Transformコマンド.md`, C# `TransformCmd`)。
            // 形態の差し替えはプレイヤーの変形 / 換装と同じ `App::set_unit_form` を通し、
            // 新形態の active_features (必要技能ゲート込み) と abilities を作り直す。
            // 現形態と同じなら何もしない (C# 準拠)。
            // 注: C# `TransformCmd` は 変形 イベントを発火しない (発火はプレイヤーの変形
            // コマンドと AI 側) が、本移植は `Combine` と揃えて発火する (fixture 21 で固定)。
            let (target, new_form) = match xargs.len() {
                0 => return Err(err(line, "Transformコマンドの引数の数が違います")),
                1 => (app.selected_unit_for_event().to_string(), xargs[0].clone()),
                _ => (xargs[0].clone(), xargs[1].clone()),
            };
            let found = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &target))
                .map(|u| (u.uid.clone(), u.unit_data_name != new_form));
            let Some((uid, true)) = found else {
                return Ok(pc + 1);
            };
            app.set_unit_form(&uid, &new_form);
            let fire = app
                .database()
                .unit_by_uid(&uid)
                .map(|u| (u.pilot_name.clone(), u.unit_data_name.clone(), u.party));
            if let Some((pilot_name, unit_data_name, party)) = fire {
                fire_unit_event_labels(
                    app,
                    &["変形", "Transform"],
                    &pilot_name,
                    &unit_data_name,
                    party,
                    Some(&new_form),
                );
            }
            return Ok(pc + 1);
        }
        "combine" => {
            // `Combine [unit] mode` — unit を合体形態 mode へ。
            // 引数 1 個なら unit 省略時の current selection だが、本実装では
            // mode = xargs[xargs.len()-1] とし、unit が指定されていればそれを、
            // 無ければ Player 勢力の任意のユニットを選ぶ。
            // C# CombineCmd: ArgNum == 2 (unit省略) or ArgNum == 3 (unit + mode)、
            // 0 引数や 3+ は EventErrorException。
            if xargs.is_empty() {
                return Err(err(line, "Combineコマンドの引数の数が違います"));
            }
            let mode = xargs[xargs.len() - 1].clone();
            let target_key = if xargs.len() >= 2 {
                Some(xargs[0].clone())
            } else {
                None
            };
            let target_idx = if let Some(k) = target_key {
                app.database()
                    .unit_instances
                    .iter()
                    .position(|u| matches_unit_handle(u, &k))
            } else {
                app.database()
                    .unit_instances
                    .iter()
                    .position(|u| u.party == crate::Party::Player)
            };
            if let Some(i) = target_idx {
                let main_uid = app.database().unit_instances[i].uid.clone();
                let main_party = app.database().unit_instances[i].party;
                // 合体形態の `分離` feature から構成パーツ名を取得 (表示フラグ除く)。
                // 例: 未完成エクスカリバー `分離=非表示 キャリバーン(不完全) ヴィヴィアン(不完全)`。
                let part_names: Vec<String> = app
                    .database()
                    .unit_by_name(&mode)
                    .and_then(|ud| {
                        ud.features
                            .iter()
                            .find(|(n, _)| n == "分離" || n == "Separate")
                            .map(|(_, v)| v.clone())
                    })
                    .map(|v| {
                        v.split_whitespace()
                            .filter(|t| !matches!(*t, "非表示" | "表示"))
                            .map(|s| s.to_string())
                            .collect()
                    })
                    .unwrap_or_default();
                // 相方パーツ (main 以外・同陣営・パーツ名一致) のパイロットを吸収し、
                // 相方ユニットと Unit コマンドが先に作った空の合体形態テンプレ
                // (無人・同名) を除去する。これで合体形態が 1 機に統合される。
                let mut absorbed_pilots: Vec<String> = Vec::new();
                let mut remove_uids: Vec<String> = Vec::new();
                for u in &app.database().unit_instances {
                    if u.uid == main_uid {
                        continue;
                    }
                    let is_partner =
                        u.party == main_party && part_names.iter().any(|p| p == &u.unit_data_name);
                    let is_empty_template = u.unit_data_name == mode && u.pilot_name.is_empty();
                    if is_partner {
                        if !u.pilot_name.is_empty() {
                            absorbed_pilots.push(u.pilot_name.clone());
                        }
                        remove_uids.push(u.uid.clone());
                    } else if is_empty_template {
                        remove_uids.push(u.uid.clone());
                    }
                }
                // main を合体形態へ morph + HP/EN 全快 + 相方をサブパイロットに。
                {
                    let u = &mut app.database_mut().unit_instances[i];
                    u.unit_data_name = mode.clone();
                    u.damage = 0;
                    u.en_consumed = 0;
                    u.weapons.clear();
                    for p in &absorbed_pilots {
                        if !u.pilot_ids.contains(p) {
                            u.pilot_ids.push(p.clone());
                        }
                    }
                }
                // 合体形態の特殊能力を再初期化。
                let feats = app
                    .database()
                    .unit_by_name(&mode)
                    .map(|ud| ud.features.clone())
                    .unwrap_or_default();
                let new_feats = crate::necessary_skill::gated_active_features(
                    &feats,
                    &app.database().unit_instances[i],
                    app.database(),
                );
                app.database_mut().unit_instances[i].active_features = new_feats;
                // 相方 / 空テンプレを除去 (pos_index も再構築)。
                if !remove_uids.is_empty() {
                    app.database_mut()
                        .remove_units_where(|u| remove_uids.contains(&u.uid));
                }
                // 合体イベントラベル発火 (除去で index がずれるため uid で引き直す)。
                if let Some(u) = app
                    .database()
                    .unit_instances
                    .iter()
                    .find(|u| u.uid == main_uid)
                {
                    let pilot_name = u.pilot_name.clone();
                    let unit_data_name = u.unit_data_name.clone();
                    let party = u.party;
                    fire_unit_event_labels(
                        app,
                        &["合体", "Combine"],
                        &pilot_name,
                        &unit_data_name,
                        party,
                        Some(&mode),
                    );
                }
            }
            return Ok(pc + 1);
        }
        "split" => {
            // `Split [unit]` — unit を 派生機体 / 分離 feature の最初のトークンに戻す。
            // unit 省略時は Player 勢力の任意のユニット。
            let target_key = xargs.first().cloned();
            let target_idx = if let Some(k) = target_key {
                app.database()
                    .unit_instances
                    .iter()
                    .position(|u| matches_unit_handle(u, &k))
            } else {
                app.database()
                    .unit_instances
                    .iter()
                    .position(|u| u.party == crate::Party::Player)
            };
            if let Some(i) = target_idx {
                let unit_name = app.database().unit_instances[i].unit_data_name.clone();
                let new_data = app
                    .database()
                    .unit_by_name(&unit_name)
                    .and_then(|d| {
                        d.features
                            .iter()
                            .find(|(k, _)| k == "分離" || k == "派生機体")
                            .map(|(_, v)| v.clone())
                    })
                    .and_then(|raw| {
                        // "非表示 X Y Z" の "非表示" を除去し、先頭トークンを採用
                        raw.split_whitespace()
                            .find(|t| *t != "非表示")
                            .map(str::to_string)
                    });
                if let Some(new_data) = new_data {
                    app.database_mut().unit_instances[i].unit_data_name = new_data.clone();
                    let feats = app
                        .database()
                        .unit_by_name(&new_data)
                        .map(|ud| ud.features.clone())
                        .unwrap_or_default();
                    let new_feats = crate::necessary_skill::gated_active_features(
                        &feats,
                        &app.database().unit_instances[i],
                        app.database(),
                    );
                    app.database_mut().unit_instances[i].active_features = new_feats;
                    let u = &app.database().unit_instances[i];
                    let pilot_name = u.pilot_name.clone();
                    let unit_data_name = u.unit_data_name.clone();
                    let party = u.party;
                    // SRC `分離 <unit> <name>:` の name は **分離前の形態名**。
                    fire_unit_event_labels(
                        app,
                        &["分離", "Split"],
                        &pilot_name,
                        &unit_data_name,
                        party,
                        Some(&unit_name),
                    );
                }
            }
            return Ok(pc + 1);
        }
        "savedata" => {
            // `SaveData [slot]` — slot 省略時は 0
            let slot = xargs
                .first()
                .and_then(|s| s.parse::<u8>().ok())
                .unwrap_or(0);
            if let Ok(json) = app.to_save_json() {
                app.push_message(format!("【セーブ】slot {slot} ({} bytes 想定)", json.len()));
                // 実保存はフロントエンド側 (localStorage) で行う。ここでは記録のみ。
                app.set_script_var(format!("__save_slot_{slot}"), json);
            }
            return Ok(pc + 1);
        }
        "load" => {
            // `Load title [title2 ...]` — タイトル (作品) をロードリストに追加。
            // SRC.Sharp `LoadCmd.cs` 準拠: 引数として作品名を受け取り、
            // 未登録のものを `App.titles` に追加する。
            // 実際のデータロード (データファイル読み込み) はフロントエンド委任。
            // セーブ番号を引数とする旧形式 `Load [slot]` と区別するため、
            // 数値のみの引数はセーブロード要求として扱う。
            if let Some(first) = xargs.first() {
                if let Ok(slot) = first.parse::<u8>() {
                    app.push_message(format!("【ロード要求】slot {slot}"));
                    return Ok(pc + 1);
                }
            }
            for arg in &xargs {
                let tname = arg.trim_matches('"').to_string();
                if !tname.is_empty() && !app.titles().contains(&tname) {
                    app.titles_mut().push(tname);
                }
            }
            return Ok(pc + 1);
        }
        "restart" => {
            // SRC `Restart` (`リスタート.md`):
            // ステージをスタートイベント時点からやり直す。`begin_battle` で
            // 自動保存された `__restart_save` script_var を復元する責務を
            // フロントエンド (`from_save_json` + `fire_resume_event`) に委任。
            // QuickSave 後にリスタートすると QuickLoad は無効化される仕様
            // (本実装は `__quicksave` をクリアして同等の動作にする)。
            if !app.script_var("__restart_save").is_empty() {
                // QuickLoad 無効化
                let snap = app.script_var("__restart_save").to_string();
                app.set_script_var("__quicksave".to_string(), String::new());
                // フロントエンドが取り出して self を置換する (core は self 置換不可)。
                app.request_reload(snap);
                app.push_message("【リスタート要求】".to_string());
            } else {
                app.push_message("リスタートデータがありません".to_string());
            }
            return Ok(pc + 1);
        }
        "stow" => {
            // SRC 風 `Stow unit carrier` (収納コマンド、本実装独自):
            // ユニットを母艦に格納する。`fire_boarding_event` のスクリプト版で、
            // `収納 <unit>:` ラベルを発火する。原典には対応する直接の Stow
            // コマンドは無いが、UI から呼ぶ `App::fire_boarding_event` と
            // 同等のフロー (`相手パイロット`/`相手ユニットＩＤ` 設定、life_state、
            // off_map) をスクリプトから明示できるようにする。
            if xargs.len() < 2 {
                return Ok(pc + 1);
            }
            let unit_key = fn_arg_value(app, &xargs[0]);
            let carrier_key = fn_arg_value(app, &xargs[1]);
            let unit_idx = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, &unit_key));
            let carrier_idx = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, &carrier_key));
            if let (Some(u_idx), Some(c_idx)) = (unit_idx, carrier_idx) {
                app.fire_boarding_event(u_idx, c_idx);
            }
            return Ok(pc + 1);
        }
        "changearea" => {
            // SRC `ChangeArea [unit] area` (`ChangeAreaコマンド.md`):
            // ユニットの活動領域を強制変更。Land/Air/Water/... コマンドの上位 API。
            // 引数 1: unit 省略 → selected_unit_for_event。
            // 引数 2+: unit + area 文字列 (`地上`/`空中`/`水中`/`水上`/`地中`/`宇宙`)。
            let (target, area) = match xargs.len() {
                0 => return Ok(pc + 1),
                1 => (app.selected_unit_for_event().to_string(), xargs[0].clone()),
                _ => (xargs[0].clone(), xargs[1].clone()),
            };
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target))
            {
                u.current_area = area;
            }
            return Ok(pc + 1);
        }
        "quicksave" => {
            // SRC `QuickSave` (`QuickSaveコマンド.md`):
            // クイックセーブ。`__quicksave` script_var に JSON を保存し、
            // フロントエンドが localStorage に永続化する責務。
            if let Ok(json) = app.to_save_json() {
                app.set_script_var("__quicksave".to_string(), json);
            }
            return Ok(pc + 1);
        }
        "quickload" => {
            // SRC `QuickLoad` (`QuickLoadコマンド.md`):
            // クイックロード。本実装は self 置換不可のため、フロントエンドが
            // [`App::take_pending_reload`] で JSON を取り出し `from_save_json` で
            // 置換 + `fire_resume_event` する責務を持つ。
            //
            // `__quicksave` が無ければ、戦闘開始時の自動スナップショット
            // `__restart_save` (`enter_battle_state` が保存) をフォールバックに使う。
            // これにより、ゲームオーバー → コンティニュー (GameOver.eve の `Quickload`) で
            // プレイヤーがクイックセーブしていなくてもステージ頭から再開できる。
            let json = {
                let qs = app.script_var("__quicksave");
                if !qs.is_empty() {
                    qs.to_string()
                } else {
                    app.script_var("__restart_save").to_string()
                }
            };
            if !json.is_empty() {
                app.request_reload(json);
                app.push_message("【クイックロード要求】".to_string());
            } else {
                app.push_message("クイックセーブデータがありません".to_string());
            }
            return Ok(pc + 1);
        }
        "forget" => {
            // `Forget title` — タイトル (作品) のロードリストから削除。
            // SRC.Sharp `ForgetCmd.cs` 準拠: 引数 1 つ必須。
            // 注: データは即削除されず、次回ロード時に対象外となるだけ。
            if args.len() != 1 {
                return Err(err(line, "Forgetコマンドの引数の数が違います"));
            }
            if let Some(tname) = xargs.first() {
                let tname = tname.trim_matches('"').to_string();
                app.titles_mut().retain(|t| t != &tname);
            }
            return Ok(pc + 1);
        }
        "restore" | "restoreevent" => {
            // `Restore label` — ラベル再登録。statements 内を探して最初の
            // 該当ラベルを labels に書き戻す。
            // SRC.Sharp 準拠: 引数なしはエラー。
            if xargs.is_empty() {
                return Err(err(line, "RestoreEventコマンドには引数が必要です"));
            }
            if let Some(label) = xargs.first() {
                let key = label
                    .trim_start_matches('@')
                    .trim_end_matches(':')
                    .to_string();
                let lib = app.script_library_mut();
                let stmts = lib.statements.clone();
                for (i, s) in stmts.iter().enumerate() {
                    if let EventStatement::Command { name, .. } = s {
                        if let Some(canon) = canonical_label(name) {
                            if canon == key {
                                lib.labels_lower.insert(key.to_ascii_lowercase(), i);
                                lib.labels.insert(key.clone(), i);
                                // ClearEvent で無効化されていた場合は再度有効化する。
                                lib.enable_pc(i);
                                break;
                            }
                        }
                    }
                }
            }
            return Ok(pc + 1);
        }
        "setskill" => {
            // SRC `SetSkill pilot skill level [name]` (`SetSkillコマンド.md`):
            // パイロットに特殊能力を追加。level=0 で封印、-1 でレベル表示なし。
            // PilotInstance.skills に "スキル名" または "スキル名 N" 形式で保持。
            // PilotInstance が存在しない場合は無視 (Place 後にのみ有効)。
            if xargs.len() < 3 {
                return Err(err(
                    line,
                    "SetSkill 命令は 3 引数必要 (pilot skill level)。",
                ));
            }
            let pilot_key = xargs[0].clone();
            let skill_name = xargs[1].clone();
            let level: i32 = xargs[2].parse().unwrap_or(0);
            // PilotInstance を探す。無ければパイロット名で作成。
            let idx = app
                .database()
                .pilot_instances
                .iter()
                .position(|p| p.id == pilot_key || p.pilot_data_name == pilot_key);
            let idx = idx.or_else(|| {
                // PilotData の名前 / 通称でも探す
                let pname = app
                    .database()
                    .pilots
                    .iter()
                    .find(|p| p.name == pilot_key || p.nickname == pilot_key)
                    .map(|p| p.name.clone())?;
                app.database_mut().create_pilot_instance(&pname, &pname)?;
                app.database()
                    .pilot_instances
                    .iter()
                    .position(|p| p.pilot_data_name == pname)
            });
            if let Some(idx) = idx {
                // level=0: 封印 → スキルをリストから除去
                if level == 0 {
                    app.database_mut().pilot_instances[idx]
                        .skills
                        .retain(|s| !s.starts_with(&skill_name));
                } else {
                    let entry = if level == -1 {
                        skill_name.clone()
                    } else {
                        format!("{} {}", skill_name, level)
                    };
                    let skills = &mut app.database_mut().pilot_instances[idx].skills;
                    // 既存エントリを更新
                    if let Some(pos) = skills.iter().position(|s| s.starts_with(&skill_name)) {
                        skills[pos] = entry;
                    } else {
                        skills.push(entry);
                    }
                }
            }
            return Ok(pc + 1);
        }
        "clearskill" => {
            // SRC `ClearSkill pilot skill` (`ClearSkillコマンド.md`):
            // SetSkill で付与した skill を解除。
            if xargs.len() < 2 {
                return Err(err(line, "ClearSkill 命令は 2 引数必要 (pilot skill)。"));
            }
            let pilot_key = xargs[0].clone();
            let skill_name = xargs[1].clone();
            let idx = app
                .database()
                .pilot_instances
                .iter()
                .position(|p| p.id == pilot_key || p.pilot_data_name == pilot_key);
            if let Some(idx) = idx {
                app.database_mut().pilot_instances[idx]
                    .skills
                    .retain(|s| !s.starts_with(&skill_name));
            }
            return Ok(pc + 1);
        }
        "clearevent" => {
            // SRC `ClearEvent [label]` (`ClearEventコマンド.md`):
            // - 引数あり: 指定ラベルを無効化。
            // - 引数省略: **現在実行中の auto-fire ラベル** を無効化 (SRC
            //   `Event.CurrentLabel`)。`*攻撃 ＮＰＣ 敵` 等の一度きりイベントの末尾で使う。
            // 無効化は disabled_pcs (label 解決でスキップ) で行う。自動発火は
            // file-scoped (`label_pc_in_file` = statements 走査) のため、labels マップ
            // からの削除だけでは止まらない。両方を無効化する。
            // SRC.Sharp 準拠: 2 引数以上はエラー。
            if xargs.len() > 1 {
                return Err(err(line, "ClearEventコマンドの引数の数が違います"));
            }
            match xargs.first() {
                Some(label) => {
                    let key = label
                        .trim_start_matches('@')
                        .trim_start_matches('*')
                        .trim_end_matches(':')
                        .trim_matches('"')
                        .to_string();
                    // file-scoped 発火も止めるため pc を解決して無効化 (現ステージ
                    // ファイル優先 → global フォールバック)。
                    let resolved = {
                        let lib = app.script_library();
                        let cur = app.current_stage_file();
                        if cur.is_empty() {
                            lib.label_pc(&key)
                        } else {
                            lib.label_pc_in_file(cur, &key)
                                .or_else(|| lib.label_pc(&key))
                        }
                    };
                    if let Some(p) = resolved {
                        app.script_library_mut().disable_pc(p);
                    }
                    app.script_library_mut().labels.remove(&key);
                }
                None => {
                    if let Some(p) = app.current_event_label_pc() {
                        app.script_library_mut().disable_pc(p);
                    }
                }
            }
            return Ok(pc + 1);
        }
        "clearspecialpower" => {
            // SRC `ClearSpecialPower unit [sp_name]` (`ClearSpecialPowerコマンド.md`):
            // ユニットに付与されている SP buff (condition) を解除。
            // sp_name 省略時は全 SP 系 condition を解除する。
            // SP 系の判定は本実装で「精神コマンド名と一致する condition」と
            // して扱う (熱血/魂/激励/集中/必中/不屈 等)。簡略化して引数指定
            // 必須のパスを優先 — 省略時は全 condition 解除。
            if xargs.is_empty() {
                return Err(err(
                    line,
                    "ClearSpecialPower 命令は最低 1 引数必要 (unit)。",
                ));
            }
            let target = xargs[0].clone();
            let sp_name = xargs.get(1).cloned();
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target))
            {
                if let Some(name) = sp_name {
                    u.remove_condition(&name);
                } else {
                    // 全 condition 解除 (SP 由来かの区別はせず簡略化)
                    u.conditions.clear();
                }
            }
            return Ok(pc + 1);
        }
        "join" => {
            // SRC `Joinコマンド.md`: `Join [unit]`
            // `Leave` で離脱させたユニット/パイロット/非戦闘員を部隊に復帰させる。
            // 0 引数: カレント選択ユニットを復帰。
            // 1 引数: 名前/UID で指定したユニットを復帰 (`off_map = false`,
            //         `life_state = ""` に戻す)。
            // パイロット名称でも unit_instances を逆引きして復帰できる。
            // ※旧実装は `pilot unit` の 2 引数形式 (= Ride 相当) だったが誤り。
            let key = if let Some(a) = xargs.first() {
                fn_arg_value(app, a)
            } else {
                app.selected_unit_for_event().to_string()
            };
            let uid = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &key) || (!key.is_empty() && u.pilot_name == key))
                .map(|u| u.uid.clone());
            if let Some(uid) = uid {
                app.database_mut().set_off_map(&uid, false);
                if let Some(u) = app.database_mut().unit_by_uid_mut(&uid) {
                    u.life_state = String::new();
                }
            }
            return Ok(pc + 1);
        }
        "ride" => {
            // `Ride pilot [unit]` — パイロットをユニットに搭乗させる。
            // - 1 引数形式 (`Ride pilot`): 直前の `Unit <name>` で生成した
            //   カレントユニット (`app.selected_unit_for_event`) に載せる。
            //   SRC `RideCmd` の case 2 相当。
            // - 2 引数形式 (`Ride pilot unit`): 名前 / uid で引いたユニットに
            //   載せる。SRC `RideCmd` の case 3 を簡略化したもの。
            // 搭乗 = 対象 UnitInstance.pilot_name を pilot 名で上書きする。
            if xargs.is_empty() {
                return Ok(pc + 1);
            }
            let pilot = fn_arg_value(app, &xargs[0]);
            let target_handle = if xargs.len() >= 2 {
                fn_arg_value(app, &xargs[1])
            } else {
                app.selected_unit_for_event().to_string()
            };
            if !target_handle.is_empty() {
                if let Some(tidx) = app
                    .database()
                    .unit_instances
                    .iter()
                    .position(|u| matches_unit_handle(u, &target_handle))
                {
                    // SRC `RideCmd` は乗り換え前に `p.GetOff()` で元機を降りる。
                    // pilot が target 以外のユニットに乗っていれば無人化して
                    // パイロット重複を防ぐ (`Ride サラ ヴィヴィアン` で元機ブレスに
                    // サラが残らないように)。1 引数形式 (新規サブパイロット搭乗) では
                    // 元機が無いので no-op。
                    let old: Vec<usize> = app
                        .database()
                        .unit_instances
                        .iter()
                        .enumerate()
                        .filter(|(i, u)| *i != tidx && u.pilot_name == pilot)
                        .map(|(i, _)| i)
                        .collect();
                    for i in old {
                        let u = &mut app.database_mut().unit_instances[i];
                        u.pilot_name.clear();
                        u.pilot_ids.retain(|p| p != &pilot);
                    }
                    app.database_mut().unit_instances[tidx].pilot_name = pilot;
                }
            }
            return Ok(pc + 1);
        }
        "land" | "air" | "water" | "sea" | "cosmos" | "diving" | "地上" | "空中" | "水中"
        | "水上" | "宇宙" | "地中" => {
            // SRC 移動領域コマンド: `地上` / `空中` / `水中` / `水上` / `宇宙` / `地中`
            // (`地上.md` 等):
            // 指定ユニットの活動領域 (`Area()` 関数の返値) を切り替える。
            // 引数 0: 直前選択ユニット、1 引数: unit 指定。
            // `UnitInstance.current_area` を更新 (地形由来の Area を上書きする)。
            // 行動終了にはならない (SRC 仕様)。移動後の使用は不可だが、
            // 本実装はチェックを行わず無条件に受け付ける (簡略化)。
            let area_label = match lname.as_str() {
                "land" | "地上" => "地上",
                "air" | "空中" => "空中",
                "water" | "水中" => "水中",
                "sea" | "水上" => "水上",
                "cosmos" | "宇宙" => "宇宙",
                "diving" | "地中" => "地中",
                _ => "",
            };
            let target = if xargs.is_empty() {
                app.selected_unit_for_event().to_string()
            } else {
                xargs[0].clone()
            };
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target))
            {
                u.current_area = area_label.to_string();
            }
            return Ok(pc + 1);
        }
        "launch" => {
            // `Launch unit_or_pilot x y` — マップ外退避中のユニットを (x, y) に
            // 出撃。座標を上書きし `off_map = false` で再配置する。該当ユニットが
            // unit_instances にいない場合は no-op (Escape で削除されているケース
            // ではなく `Pilot(...)` 解決失敗等の異常系)。
            if xargs.len() < 3 {
                return Ok(pc + 1);
            }
            let target = &xargs[0];
            let nx: u32 = xargs[1].parse().unwrap_or(0);
            let ny: u32 = xargs[2].parse().unwrap_or(0);
            let uid = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, target))
                .map(|u| u.uid.clone());
            if let Some(uid) = uid {
                // 先に座標を確定 (off_map 中は索引非登録)、続けて off_map 解除で索引へ載せる。
                app.database_mut().move_unit(&uid, nx, ny);
                app.database_mut().set_off_map(&uid, false);
            }
            return Ok(pc + 1);
        }
        "getoff" => {
            // `Getoff pilot` — 指定パイロットを乗機から降ろす。
            // 該当 UnitInstance.pilot_name を空にすることで、後段の RemovePilot
            // が誤ってユニット本体を巻き込まないようにする。
            if let Some(target_arg) = xargs.first() {
                let key = fn_arg_value(app, target_arg);
                if let Some(u) = app
                    .database_mut()
                    .unit_instances
                    .iter_mut()
                    .find(|u| u.pilot_name == key)
                {
                    u.pilot_name = String::new();
                }
            }
            return Ok(pc + 1);
        }
        "leave" => {
            // `Leave unit` — 部隊から外す (`Leaveコマンド.md`)。Escape と異なり
            // ユニット状態が `離脱` になり Status() で識別可能。
            if let Some(target_arg) = xargs.first() {
                let key = fn_arg_value(app, target_arg);
                let uid = app
                    .database()
                    .unit_instances
                    .iter()
                    .find(|u| matches_unit_handle(u, &key))
                    .map(|u| u.uid.clone());
                if let Some(uid) = uid {
                    app.database_mut().set_off_map(&uid, true);
                    if let Some(u) = app.database_mut().unit_by_uid_mut(&uid) {
                        u.life_state = "離脱".to_string();
                    }
                }
            }
            return Ok(pc + 1);
        }
        "setrelation" => {
            // SRC `SetRelation pilot_a pilot_b value` (`SetRelationコマンド.md`):
            // パイロット間好感度を設定。本実装は `__rel_<a>_<b>` script_var に
            // 数値を保存し、`Relation()` 関数で読み出す。対称関係なので両方向
            // キーに同値をセット。値は通常 -100..=100 (SRC 仕様の範囲)。
            if xargs.len() < 3 {
                return Ok(pc + 1);
            }
            let a = xargs[0].clone();
            let b = xargs[1].clone();
            let value = fn_arg_value(app, &xargs[2]);
            app.set_script_var(format!("__rel_{a}_{b}"), value.clone());
            app.set_script_var(format!("__rel_{b}_{a}"), value);
            return Ok(pc + 1);
        }
        "setbullet" => {
            // `SetBullet unit_name weapon_name n` — 残弾数を直接設定。
            // UnitInstance に弾管理を持たないため、UnitData.weapons[i].bullet を
            // 書き換える（インスタンス毎ではないが SRC 流の最小表現）。
            if xargs.len() < 3 {
                return Err(err(line, "SetBullet 命令は 3 引数必要 (unit weapon n)。"));
            }
            let unit_name = xargs[0].clone();
            let weapon_name = xargs[1].clone();
            let n: i32 = xargs[2].parse().unwrap_or(0);
            // unit_data_name か pilot_name で unit_data を引く
            let data_name = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &unit_name))
                .map(|u| u.unit_data_name.clone());
            if let Some(d) = data_name {
                if let Some(unit_data) = app.database_mut().units.iter_mut().find(|u| u.name == d) {
                    if let Some(w) = unit_data.weapons.iter_mut().find(|w| w.name == weapon_name) {
                        w.bullet = n;
                    }
                }
            }
            return Ok(pc + 1);
        }
        "disable" | "enable" => {
            // SRC `Disable [unit] name` / `Enable [unit] name`
            // (`Disableコマンド.md` / `Enableコマンド.md`):
            // 武器/アビリティ/特殊能力/形態/アイテムを使用可否切替。
            // SRC.Sharp 準拠の変数名:
            //   1 引数: `Disable(<name>)` = 1 (グローバル; 全ユニット共通)
            //   2 引数: `Disable(<unit>,<name>)` = 1 (ユニット個別)
            // Enable はその変数を削除する。
            // セーブデータにも保存される (script_var 経由なので serde 通る)。
            let is_disable = lname == "disable";
            // C# DisableCmd.cs: switch(ArgNum) — case 2 (global) / case 3 (unit-specific)、
            // それ以外は EventErrorException。
            let (key, ability_name, unit_name_opt) = match xargs.len() {
                0 | 3.. => return Err(err(line, "Disable/Enable コマンドの引数の数が違います")),
                1 => (format!("Disable({})", xargs[0]), xargs[0].clone(), None),
                2 => (
                    format!("Disable({},{})", xargs[0], xargs[1]),
                    xargs[1].clone(),
                    Some(xargs[0].clone()),
                ),
            };
            if is_disable {
                app.set_script_var(key, "1".to_string());
            } else {
                app.unset_script_var(&key);
            }
            // UnitInstance の weapon.is_disabled フラグも同期する。
            // C# では Unit.Update() で再計算されるが、Rust は直接フラグをセットする。
            let n = app.database().unit_instances.len();
            for i in 0..n {
                let matches = match &unit_name_opt {
                    Some(uname) => {
                        let u = &app.database().unit_instances[i];
                        matches_unit_handle(u, uname)
                    }
                    None => true, // グローバル: 全ユニット対象
                };
                if matches {
                    for w in &mut app.database_mut().unit_instances[i].weapons {
                        if w.weapon_data_name == ability_name {
                            w.is_disabled = is_disable;
                        }
                    }
                    // 特殊能力 (active_features) も切替える。`Disable unit ＨＰ回復` のように
                    // レベル無し基底名で指定された能力を実体 (`ＨＰ回復Lv1` 等) と突き合わせ、
                    // is_active を反転する。回復系等は feature_level が is_active を見るため、
                    // これで「水上=ＨＰ回復ON / 陸上=OFF」(決戦2話の核ギミック) が成立する。
                    for af in &mut app.database_mut().unit_instances[i].active_features {
                        if crate::feature::feature_name_matches_base(&af.name, &ability_name) {
                            af.is_active = !is_disable;
                        }
                    }
                }
            }
            return Ok(pc + 1);
        }
        "do" => {
            // `Do` / `Do While cond` / `Do Until cond` 構文。
            // While: cond が偽なら対応 Loop の次へ
            // Until: cond が真なら対応 Loop の次へ
            // SRC.Sharp 準拠: While / Until 以外のキーワードはエラー。
            if xargs.len() >= 2 {
                let kind = xargs[0].to_ascii_lowercase();
                if kind == "while" || kind == "until" {
                    let cond_text = strip_outer_parens(&xargs[1..].join(" "));
                    let cond = eval_inline_condition_mut(app, &cond_text);
                    let should_skip = (kind == "while" && !cond) || (kind == "until" && cond);
                    if should_skip {
                        return skip_to_matching("do", "loop", pc, stmts, line);
                    }
                } else {
                    return Err(err(line, "Doコマンドには While または Until が必要です"));
                }
            }
            return Ok(pc + 1);
        }
        "loop" => {
            // `Loop` / `Loop While cond` / `Loop Until cond`
            // 後方 Do へ戻るか否かを条件で判定。
            // SRC.Sharp 準拠: While / Until 以外のキーワードはエラー。
            let goto_back = if xargs.len() >= 2 {
                let kind = xargs[0].to_ascii_lowercase();
                let cond_text = strip_outer_parens(&xargs[1..].join(" "));
                let cond = eval_inline_condition_mut(app, &cond_text);
                if kind == "while" {
                    cond
                } else if kind == "until" {
                    !cond
                } else {
                    return Err(err(line, "Loopコマンドには While または Until が必要です"));
                }
            } else {
                true
            };
            if goto_back {
                // Loop は Do の位置そのものへ戻して再評価させる。
                let do_pc = find_back("do", "loop", pc, stmts, line)?;
                return Ok(do_pc);
            }
            return Ok(pc + 1);
        }
        "break" => {
            // 最内の Loop / Next まで前方ジャンプ
            return skip_to_loop_or_next_end(pc, stmts, line);
        }
        "continue" => {
            // SRC `Continue` には 2 つの異なる意味がある:
            //
            // 1. 引数なし: 最内 For/Do/Loop の次反復に飛ぶ (= 通常の "continue"
            //    キーワード)。
            //
            // 2. `Continue <filename.eve>`: 現シナリオを終了し、次に読み込む
            //    シナリオファイル名をシステム変数「次ステージ」にセットして
            //    エピローグ ラベルへジャンプする。エピローグ が無ければそのまま
            //    スクリプトを終了。フロントエンド側 (archive.rs 等) は
            //    `script_var("次ステージ")` を確認して、対応する .eve を
            //    新規シナリオとして起動する責務を負う。
            //
            //    元実装: VB5 `Event.bas` / SRC.Sharp
            //    `CmdDatas/Commands/Stage/ContinueCmd.cs:ExecInternal`
            //    に対応。
            if xargs.is_empty() {
                // ループ内なら次反復へスキップ。ループ外なら Exit 相当のステージ遷移
                // として扱う (実シナリオで `Continue` をシナリオ終了に使う慣用句)。
                match skip_to_loop_or_next_end(pc, stmts, line) {
                    Ok(after) => return Ok(after.saturating_sub(1)),
                    Err(_) => return Ok(stmts.len()), // ループ外 → 終了
                }
            }
            // SRC.Sharp 準拠: ステージ遷移形式は `Continue filename [option]` の
            // 最大 2 引数。3 引数以上はエラー。
            if xargs.len() > 2 {
                return Err(err(line, "Continueコマンドの引数の数が違います"));
            }
            let next_stage = xargs.join(" ");
            app.set_script_var("次ステージ".to_string(), next_stage);
            // シナリオ終了 = 原典の `IsScenarioFinished` スタック巻き戻しに相当。
            // 旧ステージの flow 継続と未消化の割込みイベントを破棄する
            // (これをしないと旧ステージの AfterStartEvent 等が新ステージ突入後に
            // 発火して begin_phase が二重に走る)。
            app.scenario_transition_reset();
            // インターミッション制シナリオ (`IntermissionCommand` 登録あり) では、
            // `Continue` で次ステージへ自動遷移せず `Scene::Intermission` で停止する。
            // ユーザがメニュー (キャラメイキング 等) を選んでから「次のステージへ」で
            // 本編に進める。SRC.Sharp `InterMission.InterMissionCommand()` 相当。
            if !app.intermission_commands().is_empty() {
                app.set_scene(crate::Scene::Intermission);
                app.set_intermission_cursor(0);
                // タイトル画面の Hotpoint / 描画コマンドが Intermission に
                // 重ね描画されないようクリアする。
                app.clear_hotpoints();
                app.script_overlay_mut().clear();
            } else {
                // 非インターミッション: 現スクリプト (エピローグ含む) 完了後に
                // 次ステージを起動する継続を積む。原典の
                // `StartScenario(次ステージ)` 相当 (フロントエンド非依存)。
                app.push_flow_cont(crate::flow::FlowCont::LoadNextStage);
            }
            // 現シナリオファイル**内**のエピローグへのみジャンプする。
            // global labels には後続章 (同名 `エピローグ:`) のエピローグも
            // 登録されているため、`labels.contains_key` + global フォールバック
            // で判定すると、エピローグを持たない薄い entry (`*スタート.eve` が
            // `Continue 01.eve` だけ書いてある型) の `Continue` が別ファイルの
            // エピローグへ誤ジャンプし、本編プロローグ/スタートを飛ばして
            // しまう (東方夢想伝: entry → 01.eve エピローグ直行 → 味方0体で
            // 即敗北)。`label_pc_within_file` は当該ファイル内に無ければ None。
            if let Some(epilogue_pc) = app.script_library().label_pc_within_file(pc, "エピローグ")
            {
                return Ok(epilogue_pc);
            }
            return Ok(stmts.len());
        }
        "unset" => {
            // `Unset var` — シナリオ変数を完全に削除する (key ごと remove)。
            // SRC.Sharp `UndefineVariable` 同等: 削除後 `IsVarDefined` = 0。
            // `name[expr]` のような indexed LHS にも対応。
            // SRC.Sharp 準拠: 引数なしはエラー。
            if args.is_empty() {
                return Err(err(line, "Unsetには変数名が必要です"));
            }
            let key = resolve_lhs_name(app, &args[0]);
            app.unset_script_var(&key);
            return Ok(pc + 1);
        }
        "print" | "write" => {
            // `Print <handle>, [text]` — 第 1 引数が開いているファイルハンドル
            // なら仮想ファイルへ 1 行書き込む。それ以外の `Print "text"` は
            // Message と同義 (`Write` はファイル専用なので handle 無しは無視)。
            // 書き込みテキストは `$(...)` / 関数呼出を展開してから VFS へ渡す。
            // SRC 構文の `Print #1, abc` は tokenizer 上で `#1,` がトークン化
            // されるため、ハンドル候補の trailing `,` を剥がしてから解決する。
            if let Some(first) = xargs.first() {
                let handle_raw = first.trim_end_matches(',');
                let handle = fn_arg_value(app, handle_raw);
                if app.vfs_is_handle(&handle) {
                    let raw = xargs[1..].join(" ");
                    let text = expand_vars(app, &raw);
                    app.vfs_print(&handle, text);
                } else if lname == "print" {
                    app.push_message(first.clone());
                }
            }
            return Ok(pc + 1);
        }
        "open" => {
            // `Open <path> For <mode> As <handle_var>`
            // SRC.Sharp `OpenCmd.cs` 準拠: 引数は正確に 5 (コマンド名を除く)。
            // ArgNum==6 はコマンド名を含むため、xargs.len()==5。
            if xargs.len() != 5 {
                return Err(err(line, "Openコマンドの引数の数が違います"));
            }
            // path / mode には `$(...)` が含まれることがあるため eval 後に展開する。
            let path = xargs
                .first()
                .map(|s| expand_vars(app, &fn_arg_value(app, s)))
                .unwrap_or_default();
            // パス内の上位ディレクトリ参照を禁止する (SRC.Sharp 準拠)。
            if path.contains("..\\") || path.contains("../") {
                return Err(err(line, "ファイル指定に「../」は使えません"));
            }
            let mut mode = String::from("入力");
            let mut handle_var = String::new();
            let mut i = 1;
            while i < xargs.len() {
                match xargs[i].to_ascii_lowercase().as_str() {
                    "for" => {
                        if let Some(m) = xargs.get(i + 1) {
                            mode = expand_vars(app, &fn_arg_value(app, m));
                        }
                        i += 2;
                    }
                    "as" => {
                        if let Some(v) = xargs.get(i + 1) {
                            handle_var = v.clone();
                        }
                        i += 2;
                    }
                    _ => i += 1,
                }
            }
            if !path.is_empty() && !handle_var.is_empty() {
                let h = app.vfs_open(&path, &mode);
                app.set_script_var(handle_var, h);
            }
            return Ok(pc + 1);
        }
        "read" | "lineread" => {
            // `Read <handle> <var>` / `LineRead <handle> <var>` — 1 行読込。
            if xargs.len() >= 2 {
                let handle = fn_arg_value(app, &xargs[0]);
                let line = app.vfs_read_line(&handle).unwrap_or_default();
                let var = resolve_lhs_name(app, &args[1]);
                app.set_script_var(var, line);
            }
            return Ok(pc + 1);
        }
        "close" => {
            if let Some(h) = xargs.first() {
                let handle = fn_arg_value(app, h);
                app.vfs_close(&handle);
            }
            return Ok(pc + 1);
        }
        "supply" => {
            // `Supply unit` — HP/EN を完全回復
            if let Some(target) = xargs.first() {
                recover_hp(app, target, None);
                recover_en(app, target, None);
            }
            return Ok(pc + 1);
        }
        "fix" => {
            // `Fix name` — SRC `Fixコマンド.md`:
            // パイロットまたはアイテムをインターミッションでの乗り換え・
            // 交換対象外に固定する。イベントによる強制交換 (`Ride`/`Equip`) は
            // 影響を受けない。
            //
            // 引数省略時はデフォルトユニットのメインパイロットを固定する。
            let target = if let Some(t) = xargs.first() {
                fn_arg_value(app, t)
            } else {
                // 省略時: selected_unit_for_event が指すユニットのパイロット
                let uid = app.selected_unit_for_event().to_string();
                app.database()
                    .unit_instances
                    .iter()
                    .find(|u| u.uid == uid || matches_unit_handle(u, &uid))
                    .map(|u| u.pilot_name.clone())
                    .unwrap_or_default()
            };
            if !target.is_empty() {
                // アイテムスロット内の name 一致を検索し is_fixed を立てる
                let mut found_item = false;
                for u in app.database_mut().unit_instances.iter_mut() {
                    for slot in u.item_slots.iter_mut() {
                        if slot.equipped_item.as_deref() == Some(target.as_str()) {
                            slot.is_fixed = true;
                            found_item = true;
                        }
                    }
                }
                // アイテムが見つからなければパイロットとして固定
                if !found_item {
                    for u in app.database_mut().unit_instances.iter_mut() {
                        if u.pilot_name == target {
                            for inst in u.pilot_ids.iter() {
                                let _ = inst; // pilot_instance 固定は下で処理
                            }
                        }
                    }
                    // PilotInstance.is_fixed を設定
                    for inst in app.database_mut().pilot_instances.iter_mut() {
                        if inst.pilot_data_name == target || inst.id == target {
                            inst.is_fixed = true;
                        }
                    }
                }
            }
            return Ok(pc + 1);
        }
        "release" => {
            // `Release [name]` — SRC `Releaseコマンド.md`:
            // `Fix` で固定されたパイロット・アイテムの固定を解除する。
            // 引数省略時は全パイロット・全アイテムスロットの固定を解除。
            let target = xargs.first().map(|t| fn_arg_value(app, t));
            match &target {
                None => {
                    // 全解除
                    for u in app.database_mut().unit_instances.iter_mut() {
                        for slot in u.item_slots.iter_mut() {
                            slot.is_fixed = false;
                        }
                    }
                    for inst in app.database_mut().pilot_instances.iter_mut() {
                        inst.is_fixed = false;
                    }
                }
                Some(name) if !name.is_empty() => {
                    // アイテム解除
                    for u in app.database_mut().unit_instances.iter_mut() {
                        for slot in u.item_slots.iter_mut() {
                            if slot.equipped_item.as_deref() == Some(name.as_str()) {
                                slot.is_fixed = false;
                            }
                        }
                    }
                    // パイロット解除
                    for inst in app.database_mut().pilot_instances.iter_mut() {
                        if inst.pilot_data_name == *name || inst.id == *name {
                            inst.is_fixed = false;
                        }
                    }
                }
                _ => {}
            }
            return Ok(pc + 1);
        }
        "rankup" => {
            // `RankUp pilot [n=1]` — pilot の rank を script_var に記録
            if let Some(target) = xargs.first() {
                let n: i32 = xargs.get(1).and_then(|s| s.parse().ok()).unwrap_or(1);
                let key = format!("__rank_{target}");
                let cur: i32 = app.script_var(&key).parse().unwrap_or(0);
                app.set_script_var(key, (cur + n).to_string());
            }
            return Ok(pc + 1);
        }
        "upgrade" => {
            // SRC `Upgrade [unit1] unit2` (`Upgradeコマンド.md` / C# `UpgradeCmd`):
            // unit1 を unit2 のユニットデータに **入れ替える** (機体グレードアップ /
            // ボスの形態変化)。ユニットランク (改造段階) は維持、HP/EN/弾薬は全快、
            // パイロット・位置・陣営は引き継ぐ。unit1 省略時は selected_unit_for_event。
            // (旧実装の `Upgrade unit attr n` ステータス加算は SRC に無い誤実装だった。)
            let (target_key, new_unit) = match xargs.len() {
                0 => return Ok(pc + 1),
                1 => (app.selected_unit_for_event().to_string(), xargs[0].clone()),
                _ => (xargs[0].clone(), xargs[1].clone()),
            };
            // 変換先ユニットデータが無ければ no-op (堅牢性優先)。
            if app.database().unit_by_name(&new_unit).is_none() {
                return Ok(pc + 1);
            }
            let Some(idx) = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, &target_key))
            else {
                return Ok(pc + 1);
            };
            // インスタンスを取り出して機体データを差し替え、data 由来状態を再初期化。
            let mut inst = app.database().unit_instances[idx].clone();
            inst.unit_data_name = new_unit;
            inst.damage = 0; // HP 全快
            inst.en_consumed = 0; // EN 全快
            inst.weapons.clear(); // 弾薬全快 (空 = 残弾フル)
            inst.abilities.clear();
            inst.active_features.clear();
            inst.life_state.clear(); // 破壊扱いを解除 (= 復活)
            populate_active_features(&mut inst, app);
            app.database_mut().unit_instances[idx] = inst;
            return Ok(pc + 1);
        }
        "changeparty" => {
            // `ChangeParty unit "Enemy"` — 所属勢力変更
            if xargs.len() < 2 {
                return Err(err(line, "ChangeParty 命令は 2 引数必要 (unit party)。"));
            }
            let target = xargs[0].clone();
            let party = parse_party(app, &xargs[1], line)?;
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target))
            {
                u.party = party;
            }
            return Ok(pc + 1);
        }
        "replacepilot" => {
            // `ReplacePilot unit pilot` — Join と同義 (UnitInstance.pilot_name 差替え)
            if xargs.len() < 2 {
                return Err(err(line, "ReplacePilot 命令は 2 引数必要 (unit pilot)。"));
            }
            let unit = xargs[0].clone();
            let pilot = xargs[1].clone();
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &unit))
            {
                u.pilot_name = pilot;
            }
            return Ok(pc + 1);
        }
        "escape" => {
            // `Escape [unit_or_pilot | party]` — マップから一時退避（撤退）。
            // `off_map = true` を立てて AI / 戦闘 / 描画 / 勝利判定から除外。
            // 後段の `Launch` / `Place` で `off_map = false` に戻して再配置できる。
            // SRC `EscapeCmd` 準拠:
            //  - 引数なし: `SelectedUnitForEvent` (ForEach ループ対象等) を退避。
            //    `ForEach 敵 / Escape / Next` で全敵退避するイディオムを成立させる。
            //  - 陣営ラベル (味方/敵/ＮＰＣ/中立): 当該陣営の出撃中ユニットを全退避。
            //  - それ以外: パイロット名 / ユニット ID で 1 体退避。
            // 末尾 `非同期` オプションは無視。
            let args_no_opt: Vec<&String> =
                xargs.iter().filter(|a| a.as_str() != "非同期").collect();
            match args_no_opt.first() {
                None => {
                    // 引数なし: 選択ユニットを退避。
                    let key = app.selected_unit_for_event().to_string();
                    let uid = app
                        .database()
                        .unit_instances
                        .iter()
                        .find(|u| matches_unit_handle(u, &key))
                        .map(|u| u.uid.clone());
                    if let Some(uid) = uid {
                        app.database_mut().set_off_map(&uid, true);
                    }
                }
                Some(arg) => {
                    let key = fn_arg_value(app, arg);
                    if let Some(party) = parse_party_label(key.trim().trim_matches('"')) {
                        // 陣営ラベル: 当該陣営の出撃中ユニットを全退避。
                        let uids: Vec<String> = app
                            .database()
                            .unit_instances
                            .iter()
                            .filter(|u| u.party == party && !u.off_map)
                            .map(|u| u.uid.clone())
                            .collect();
                        for uid in uids {
                            app.database_mut().set_off_map(&uid, true);
                        }
                    } else {
                        // パイロット名 / ユニット ID で 1 体退避。
                        let uid = app
                            .database()
                            .unit_instances
                            .iter()
                            .find(|u| matches_unit_handle(u, &key))
                            .map(|u| u.uid.clone());
                        if let Some(uid) = uid {
                            app.database_mut().set_off_map(&uid, true);
                        }
                    }
                }
            }
            return Ok(pc + 1);
        }
        "specialpower" | "mind" => {
            // SRC `SpecialPower [unit] spower [target]` (`SpecialPowerコマンド.md`)。
            // `Mind pilot spower` は Ver.1.6 までの同等コマンド。SP は消費せず、効果は
            // App の精神コマンド経路 (`execute_spirit_by_event` → `apply_spirit_effect`)
            // で適用する。引数の解釈は C# `SpecialPowerCmd` に合わせる:
            //  - 1 引数: `spower` (使用者 = SelectedUnitForEvent)
            //  - 2 引数: `spower target` (みがわり/挑発 効果を持つ SP) / `unit spower`
            //  - 3 引数: `unit spower target`
            // SRC はユニット指定にシステム変数 (相手ユニットＩＤ 等) を許す。裸の変数名
            // (生贄ルーチンの `SpecialPower 相手ユニットＩＤ みがわり 対象ユニットＩＤ`) は
            // 実ユニットに一致しなければ同名 script_var の値 (uid) へフォールバック解決する。
            let selected = || app.selected_unit_for_event().to_string();
            let (caster_key, power, target_key) = match xargs.len() {
                0 => return Err(err(line, "SpecialPowerコマンドの引数の数が違います")),
                1 => (selected(), xargs[0].clone(), None),
                2 if app.spirit_needs_target(&xargs[0]) => {
                    (selected(), xargs[0].clone(), Some(xargs[1].clone()))
                }
                2 => (xargs[0].clone(), xargs[1].clone(), None),
                _ => (xargs[0].clone(), xargs[1].clone(), Some(xargs[2].clone())),
            };
            let uid_of = |app: &App, key: String| {
                let key = resolve_handle_var(app, key);
                app.database()
                    .unit_instances
                    .iter()
                    .find(|u| matches_unit_handle(u, &key))
                    .map(|u| u.uid.clone())
            };
            let Some(caster) = uid_of(app, caster_key) else {
                return Ok(pc + 1);
            };
            let target = match target_key {
                Some(k) => match uid_of(app, k) {
                    Some(t) => Some(t),
                    None => return Ok(pc + 1),
                },
                None => None,
            };
            app.execute_spirit_by_event(&caster, target.as_deref(), &power);
            return Ok(pc + 1);
        }
        "clearmind" => {
            // `ClearMind pilot [sp_name]` — 精神コマンド効果を解除。
            // sp_name 省略時は全精神コマンド系 condition を解除。
            if xargs.is_empty() {
                return Ok(pc + 1);
            }
            let pilot_key = xargs[0].clone();
            let sp_name = xargs.get(1).cloned();
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| u.pilot_name == pilot_key || matches_unit_handle(u, &pilot_key))
            {
                if let Some(name) = sp_name {
                    u.conditions.retain(|c| c.name != name);
                } else {
                    u.conditions.clear();
                }
            }
            return Ok(pc + 1);
        }
        "setability" => {
            // `SetAbility pilot skill level` — パイロットに特殊能力を付与。
            // `SetSkill` と同義 (こちらが旧名称)。同じ実装に委譲する。
            if xargs.len() < 3 {
                return Ok(pc + 1);
            }
            return exec_command_pc(app, "SetSkill", &xargs, line, pc, stmts, labels);
        }
        "clearability" => {
            // `ClearAbility pilot skill` — パイロットの特殊能力を削除。
            // `ClearSkill` と同義 (こちらが旧名称)。
            if xargs.len() < 2 {
                return Ok(pc + 1);
            }
            return exec_command_pc(app, "ClearSkill", &xargs, line, pc, stmts, labels);
        }
        "mapattack" | "mapweapon" => {
            // `MapAttack [unit] weapon X Y` — 座標 (X, Y) を中心にマップ攻撃。
            // `MapWeapon` は SRC Ver.1.6 までの旧名称で、現在は `MapAttack` と
            // 完全に同義 (SRC: MapAttackコマンド.md / 更新履歴(2003) で改名)。
            // 武器の max_range 内のすべての対立勢力ユニットに同時にダメージを
            // 与える。反撃 / 経験値 / 資金は無効 (SRC docs)。
            // 引数解釈: 最後の 2 つは X Y (数値), その手前は weapon, 残りがあれば unit。
            // 末尾に `通常戦闘` を付けると「使用/攻撃イベント」を発火する通常戦闘扱いに
            // なる (SRC `ExecMapAttackCmd` CmdData.cs:14865 — 既定 is_event=true、
            // 末尾 `通常戦闘` で is_event=false)。
            let mut xa: &[String] = &xargs;
            let is_event = !matches!(xa.last().map(|s| s.as_str()), Some("通常戦闘"));
            if !is_event {
                xa = &xa[..xa.len() - 1];
            }
            if xa.len() < 3 {
                return Ok(pc + 1);
            }
            let n = xa.len();
            let cx: u32 = xa[n - 2].parse().unwrap_or(0);
            let cy: u32 = xa[n - 1].parse().unwrap_or(0);
            let weapon_name = xa[n - 3].clone();
            let unit_key = if n >= 4 {
                Some(xa[n - 4].clone())
            } else {
                None
            };
            map_attack(app, unit_key.as_deref(), &weapon_name, cx, cy, is_event);
            return Ok(pc + 1);
        }
        "mapability" => {
            // MapAbility は MapAttack と同じ書式の汎用版。本実装では同じ処理。
            if xargs.len() < 3 {
                return Ok(pc + 1);
            }
            let n = xargs.len();
            let cx: u32 = xargs[n - 2].parse().unwrap_or(0);
            let cy: u32 = xargs[n - 1].parse().unwrap_or(0);
            let weapon_name = xargs[n - 3].clone();
            let unit_key = if n >= 4 {
                Some(xargs[n - 4].clone())
            } else {
                None
            };
            // スクリプト経由の MapAbility も既定で is_event=true (イベント非発火)。
            map_attack(app, unit_key.as_deref(), &weapon_name, cx, cy, true);
            return Ok(pc + 1);
        }
        "organize" => {
            // `Organize count x y` — 中間メニューで count 体までを (x, y) 付近に
            // プレイヤーが配置するためのコマンド。本実装では UI を持たないので、
            // off_map になっている Player ユニットを最大 count 体まで (x, y) 中心
            // 螺旋状に再配置するだけの最小実装。
            if xargs.len() < 3 {
                return Ok(pc + 1);
            }
            let count: usize = fn_arg_value(app, &xargs[0]).parse().unwrap_or(9);
            let cx: i32 = fn_arg_value(app, &xargs[1]).parse().unwrap_or(5);
            let cy: i32 = fn_arg_value(app, &xargs[2]).parse().unwrap_or(5);
            // 配置済 (! off_map) の Player ユニットは触らず、off_map なものから
            // 配置する。何も off_map に無い場合は no-op。
            let (mw, mh) = app
                .database()
                .map
                .as_ref()
                .map(|m| (m.width as i32, m.height as i32))
                .unwrap_or((20, 20));
            let mut placed = 0usize;
            // 螺旋順オフセット: (0,0) → 周囲 1 マス → 2 マス ...
            let offsets: Vec<(i32, i32)> = {
                let mut v: Vec<(i32, i32)> = Vec::new();
                for r in 0i32..=5 {
                    for dy in -r..=r {
                        for dx in -r..=r {
                            if dx.abs().max(dy.abs()) == r {
                                v.push((dx, dy));
                            }
                        }
                    }
                }
                v
            };
            let mut occupied: std::collections::HashSet<(u32, u32)> = app
                .database()
                .unit_instances
                .iter()
                .filter(|u| !u.off_map)
                .map(|u| (u.x, u.y))
                .collect();
            // 1 順目: off_map の Player ユニットを優先配置。
            // 2 順目: それでも 0 体だった場合は、on_map の Player ユニットも
            //         移動対象とする (キャラ作成→Escape のチェーンが切れて
            //         on_map のまま残っているケースの救済)。
            for pass in 0..2 {
                for i in 0..app.database().unit_instances.len() {
                    if placed >= count {
                        break;
                    }
                    let candidate = {
                        let u = &app.database().unit_instances[i];
                        if u.party != crate::Party::Player {
                            false
                        } else if pass == 0 {
                            u.off_map
                        } else {
                            // 2 順目: 既に配置済 (= occupied に含まれる) を除く
                            !u.off_map && occupied.contains(&(u.x, u.y))
                        }
                    };
                    if !candidate {
                        continue;
                    }
                    // 空き座標を探す
                    let mut chosen: Option<(u32, u32)> = None;
                    for (dx, dy) in &offsets {
                        let nx = cx + *dx;
                        let ny = cy + *dy;
                        if nx < 0 || ny < 0 || nx >= mw || ny >= mh {
                            continue;
                        }
                        let pos = (nx as u32, ny as u32);
                        if !occupied.contains(&pos) {
                            chosen = Some(pos);
                            break;
                        }
                    }
                    if let Some(pos) = chosen {
                        // 2 順目では先に古い位置を occupied から外す
                        if pass == 1 {
                            let prev = {
                                let u = &app.database().unit_instances[i];
                                (u.x, u.y)
                            };
                            occupied.remove(&prev);
                        }
                        let uid = app.database().unit_instances[i].uid.clone();
                        app.database_mut().move_unit(&uid, pos.0, pos.1);
                        app.database_mut().set_off_map(&uid, false);
                        occupied.insert(pos);
                        placed += 1;
                    }
                }
                // 1 順目で 1 体以上配置できたら 2 順目はスキップ
                if placed > 0 {
                    break;
                }
            }
            return Ok(pc + 1);
        }
        "intermissioncommand" => {
            // `IntermissionCommand <name> <file>` — name を選択肢として登録、
            // 選択時に file を実行する。第 2 引数 "削除" で当該名を削除。
            // SRC.Sharp `Intermission.cs` の `IntermissionCommand(file)` グローバル
            // 変数経由のセットと等価。
            // SRC.Sharp 準拠: 2 引数未満はエラー。
            if xargs.len() < 2 {
                return Err(err(line, "IntermissionCommandコマンドの引数の数が違います"));
            }
            let name = xargs[0].clone();
            let arg2 = xargs[1].clone();
            if arg2 == "削除" {
                app.remove_intermission_command(&name);
                app.unset_script_var(&format!("IntermissionCommand({name})"));
            } else {
                // SRC.Sharp 準拠: `IntermissionCommand(name)` グローバル変数にファイル名を記録する
                app.set_script_var(format!("IntermissionCommand({name})"), arg2.clone());
                app.push_intermission_command(name, arg2);
            }
            return Ok(pc + 1);
        }
        "callintermissioncommand" => {
            // `CallIntermissionCommand <name>` — 名前付きの中断メニューを起動。
            // データセーブや機体改造など、GUI 相互作用が必要なものが多く、
            // WASM では stub としてログ出力のみ。
            let name = xargs.first().map(|s| s.as_str()).unwrap_or("");
            match name {
                "データセーブ" => {
                    // メニュー経路 (`InterItem::Save`) と同じ実体に委譲する。
                    // 現在状態を JSON 化して `__quicksave` に保存 (フロントが
                    // localStorage へ永続化)。
                    app.intermission_data_save();
                }
                "機体改造"
                | "ユニットの強化"
                | "乗せ換え"
                | "アイテム交換"
                | "換装"
                | "パイロットステータス"
                | "ユニットステータス" => {
                    log::info!(
                        "CallIntermissionCommand: {} (not implemented in WASM)",
                        name
                    );
                }
                _ => {
                    log::warn!("CallIntermissionCommand: unknown command '{}'", name);
                }
            }
            return Ok(pc + 1);
        }
        "effect" => {
            // `Effect <name> [target] [params...]` — visual effect
            log::info!(
                "Effect: {} (stub)",
                xargs.first().map(|s| s.as_str()).unwrap_or("")
            );
            return Ok(pc + 1);
        }
        "explode" => {
            // SRC `Explode size [X Y]` (`Explodeコマンド.md`):
            // 爆発を (X, Y) で表示 (省略時は画面中央)。size = S/M/L/LL/XL。
            // 視覚演出のみで、ユニット破壊などの副作用は無い。
            // 本実装は ScriptOverlay::FillRect で円形ではなく大きさ別の正方形を
            // 描く近似で代用 (実描画は frontend 任せだが、command を実行した事実は
            // overlay に残る)。
            let size = xargs.first().map(String::as_str).unwrap_or("M");
            let radius_px: f64 = match size {
                "S" => 16.0,
                "M" => 24.0,
                "L" => 32.0,
                "LL" => 48.0,
                "XL" => 64.0,
                _ => 24.0,
            };
            let (cx, cy) = if xargs.len() >= 3 {
                let x: f64 = xargs[1].parse().unwrap_or(0.0);
                let y: f64 = xargs[2].parse().unwrap_or(0.0);
                // タイル座標から pixel に変換 (1 タイル = 32px)
                (x * 32.0 + 16.0, y * 32.0 + 16.0)
            } else {
                // 画面中央 (480/480 既定マップ想定)
                (240.0, 240.0)
            };
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::SetColor {
                    color: "#ff6f00".to_string(),
                });
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::FillRect {
                    x: cx - radius_px,
                    y: cy - radius_px,
                    w: radius_px * 2.0,
                    h: radius_px * 2.0,
                });
            return Ok(pc + 1);
        }
        "sepia" | "monotone" | "colorfilter" => {
            // SRC ビジュアルフィルタ: `Sepia` / `Monotone` / `ColorFilter` —
            // 画面全体に色フィルタを適用する。本実装は `DrawCmd::Fade` で擬似的に
            // 半透明のオーバーレイ色を被せる。
            // Sepia → セピア (薄茶)、Monotone → グレー、ColorFilter → 引数指定。
            let color = match lname.as_str() {
                "sepia" => "rgba(112,66,20,0.35)".to_string(),
                "monotone" => "rgba(128,128,128,0.35)".to_string(),
                "colorfilter" => xargs
                    .first()
                    .map(|c| resolve_color(app, c))
                    .unwrap_or_else(|| "rgba(0,0,0,0.2)".to_string()),
                _ => "rgba(0,0,0,0.2)".to_string(),
            };
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::Fade { color, alpha: 1.0 });
            return Ok(pc + 1);
        }
        "whiteout" => {
            // SRC `WhiteOut n`: 画面が白へフェードアウト (終状態: 白で覆う)。
            // 引数 n は 0..255 程度の段階値。n が大きいほど不透明 (= より白い)。
            // アニメーションせず終状態のみ描くので白の全画面 Fade を 1 枚積む。
            let n: f64 = xargs.first().and_then(|s| s.parse().ok()).unwrap_or(255.0);
            let alpha = (n / 255.0).clamp(0.0, 1.0);
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::Fade {
                    color: "#ffffff".to_string(),
                    alpha,
                });
            return Ok(pc + 1);
        }
        "whitein" => {
            // SRC `WhiteIn n`: 白から通常画面へフェードイン。終状態は **通常画面**
            // (白を除去)。旧実装は WhiteOut と同一視して白を積んでおり、引数なし
            // (`WhiteIn` → n=255 → alpha=1.0) で全画面が白のまま残り「白いマップ」で
            // 操作不能になっていた (東方夢想伝: タイトルテロップ末尾の `WhiteIn`)。
            // フェードインなので白の全画面 Fade を取り除いて画面を露出させる。
            app.script_overlay_mut()
                .remove_fades_of(is_white_fade_color);
            return Ok(pc + 1);
        }
        "showimage" => {
            // `ShowImage bitmap x y [z]` — 画像を指定座標に表示。
            // 本実装はスクリプトオーバーレイに PaintPicture 相当で委譲。
            // 当面 no-op で安全スルー。
            return Ok(pc + 1);
        }
        "setstock" => {
            // SRC `SetStock [unit] ability stock` (`SetStockコマンド.md`):
            // ユニットのアビリティ残り使用回数を指定値に変更する。
            if xargs.len() < 2 {
                return Ok(pc + 1);
            }
            // 引数は `unit ability stock` または `ability stock` の 2 形式。
            let (unit_key, ability_key, stock_raw) = if xargs.len() >= 3 {
                (xargs[0].clone(), xargs[1].clone(), xargs[2].clone())
            } else {
                // unit 省略: 選択中ユニット
                let uid = app.selected_unit_for_event().to_string();
                (uid, xargs[0].clone(), xargs[1].clone())
            };
            let stock: i32 = stock_raw.parse().unwrap_or(0);
            // アビリティ番号指定 (1-origin 数字) か名前指定かを判断。
            // 番号指定の場合は UnitData.features から名前を解決する。
            let resolved_ability_name: Option<String> = if let Ok(n) = ability_key.parse::<usize>()
            {
                let idx = n.saturating_sub(1);
                app.database()
                    .unit_instances
                    .iter()
                    .find(|u| matches_unit_handle(u, &unit_key))
                    .and_then(|u| app.database().unit_by_name(&u.unit_data_name))
                    .and_then(|d| d.features.get(idx))
                    .map(|(name, _)| name.clone())
            } else {
                Some(ability_key.clone())
            };
            if let Some(ability_name) = resolved_ability_name {
                if let Some(u) = app
                    .database_mut()
                    .unit_instances
                    .iter_mut()
                    .find(|u| matches_unit_handle(u, &unit_key))
                {
                    u.ability_stocks.insert(ability_name, stock);
                }
            }
            return Ok(pc + 1);
        }
        "stopsummoning" => {
            // `StopSummoning [unit]` (`StopSummoningコマンド.md` / `召喚解除.md`):
            // 指定ユニットが召喚したユニットを解放 (マップから除去) する。
            // 引数省略時は selected_unit_for_event を対象とする。
            let summoner_key = if let Some(t) = xargs.first() {
                fn_arg_value(app, t)
            } else {
                app.selected_unit_for_event().to_string()
            };
            // 召喚親の uid を解決してから、その uid を親に持つ召喚ユニットを除去。
            let summoner_uid = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &summoner_key))
                .map(|u| u.uid.clone());
            if let Some(uid) = summoner_uid {
                // 空 uid は召喚親として無効 (誤一致防止)。
                if !uid.is_empty() {
                    app.database_mut()
                        .remove_units_where(|u| u.summoned_by.as_deref() == Some(uid.as_str()));
                }
            }
            return Ok(pc + 1);
        }
        "attack" => {
            // SRC `Attack unit1 weapon1 unit2 weapon2` (`Attackコマンド.md`):
            // イベント上の戦闘。命中・ダメージはプレイヤー / AI の攻撃と同じ判定
            // (`App::script_attack` → `resolve_strike`) で、使用条件 (射程/気力/EN) は問わない。
            // weapon1: `自動` で最強武器自動選択 / 武器名で固定
            // weapon2: `防御`/`回避`/`無抵抗`/`反撃不能` で反撃なし / `自動` or 武器名で反撃
            // 仕様準拠: 使用 / 攻撃 / 損傷率 / 破壊 / 全滅イベントは発火させず、経験値・資金も
            // 入らない ("Attackコマンドの結果、他のイベントが発生することはありません")。
            if xargs.len() < 4 {
                return Err(err(
                    line,
                    "Attack 命令は 4 引数必要 (unit1 weapon1 unit2 weapon2)。",
                ));
            }
            let uid_of = |app: &App, arg: &str| {
                let key = fn_arg_value(app, arg);
                app.database()
                    .unit_instances
                    .iter()
                    .find(|u| matches_unit_handle(u, &key))
                    .map(|u| u.uid.clone())
            };
            if let (Some(atk), Some(def)) = (uid_of(app, &xargs[0]), uid_of(app, &xargs[2])) {
                app.script_attack(&atk, &xargs[1], &def, &xargs[3]);
            }
            return Ok(pc + 1);
        }
        "charge" => {
            // SRC `Charge [unit]` (`Chargeコマンド.md`):
            // 指定ユニットの charged フラグを true にする。次の攻撃でチャージ攻撃属性
            // (`Ｃ` 属性) の武器が解禁される想定。本実装は属性連動を未対応だが、
            // フラグだけ立てて `IsAvailable(unit, チャージ)` 風の判定に使える状態にする。
            // 引数 0: 直前選択ユニット、1+: unit 指定。
            let target = if xargs.is_empty() {
                app.selected_unit_for_event().to_string()
            } else {
                xargs[0].clone()
            };
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target))
            {
                u.charged = true;
            }
            return Ok(pc + 1);
        }
        "useability" => {
            // SRC `UseAbility [unit] ability [target]` (`UseAbilityコマンド.md`):
            // ユニットに指定アビリティを発動させる。使用者のユニットデータに同名の
            // アビリティがあれば、プレイヤー / AI と同じ `App::execute_ability` で
            // 回数 / EN を消費して効果を適用する (C# `ExecuteAbility(.., is_event: true)`:
            // 使用条件は問わず、行動消費・経験値・使用イベントなし)。
            if let Some((caster, idx, target, name)) = resolve_data_ability(app, &xargs) {
                app.set_script_var("直前使用アビリティ".to_string(), name);
                app.execute_ability(&caster, idx, &target);
                return Ok(pc + 1);
            }
            // 以下はユニットデータにアビリティが無いときの互換処理 (アビリティ名で
            // 効果を決め打ちする旧実装)。SRC.NET `AbilityEffect.cs` で頻出する
            // 基本効果のいくつかをハードコードで処理する:
            //   - `修理装置` / `修理`         → 対象 unit の HP 全回復
            //   - `補給装置` / `補給`         → 対象 unit の EN 全回復 + 弾薬補給は省略
            //   - `状態異常回復` / `状態回復` → 対象 unit の全 condition クリア
            //   - `自爆`                      → 発動 unit を撃破 (Destruction 発火)
            // それ以外のアビリティは `直前使用アビリティ` script_var 保存のみ。
            if xargs.len() < 2 {
                return Ok(pc + 1);
            }
            let unit_key = xargs[0].clone();
            let ability = xargs[1].clone();
            let target_key = xargs.get(2).cloned().unwrap_or_else(|| unit_key.clone());
            app.set_script_var("直前使用アビリティ".to_string(), ability.clone());
            match ability.as_str() {
                "修理装置" | "修理" => {
                    recover_hp(app, &target_key, Some("全"));
                }
                "補給装置" | "補給" => {
                    recover_en(app, &target_key, Some("全"));
                }
                "状態異常回復" | "状態回復" => {
                    if let Some(u) = app
                        .database_mut()
                        .unit_instances
                        .iter_mut()
                        .find(|u| matches_unit_handle(u, &target_key))
                    {
                        u.conditions.clear();
                    }
                }
                "自爆" => {
                    // 自爆: 発動 unit を撃破。max_hp ぶんダメージを与えて
                    // Destruction を発火させる。
                    let max_hp = app
                        .database()
                        .unit_instances
                        .iter()
                        .find(|u| matches_unit_handle(u, &unit_key))
                        .map(|u| app.database().effective_max_hp(u))
                        .unwrap_or(0);
                    if max_hp > 0 {
                        apply_damage(app, &unit_key, max_hp);
                    }
                }
                // 以下は condition として付与し、combat.rs / display 側で
                // 解釈する規約。`Condition()` 関数で参照可能。
                "バリア" | "バリア展開" | "シールド" => {
                    if let Some(u) = app
                        .database_mut()
                        .unit_instances
                        .iter_mut()
                        .find(|u| matches_unit_handle(u, &target_key))
                    {
                        u.add_condition(crate::condition::Condition::new("バリア".to_string(), -1));
                    }
                }
                "分身" | "ホログラム" => {
                    if let Some(u) = app
                        .database_mut()
                        .unit_instances
                        .iter_mut()
                        .find(|u| matches_unit_handle(u, &target_key))
                    {
                        u.add_condition(crate::condition::Condition::new("分身".to_string(), -1));
                    }
                }
                "集中" | "精神統一" => {
                    if let Some(u) = app
                        .database_mut()
                        .unit_instances
                        .iter_mut()
                        .find(|u| matches_unit_handle(u, &target_key))
                    {
                        u.add_condition(crate::condition::Condition::new("集中".to_string(), -1));
                    }
                }
                "ステルス" | "隠れ身" => {
                    if let Some(u) = app
                        .database_mut()
                        .unit_instances
                        .iter_mut()
                        .find(|u| matches_unit_handle(u, &target_key))
                    {
                        u.add_condition(crate::condition::Condition::new(
                            "ステルス".to_string(),
                            -1,
                        ));
                    }
                }
                "合体技" | "援護攻撃" => {
                    // 合体技/援護攻撃発動: Partner 履歴に対象 unit を追加して
                    // Partner / CountPartner 関数で参照可能にする。
                    let count_key = "直前合体技パートナー数".to_string();
                    let n: i32 = app.script_var(&count_key).parse().unwrap_or(0);
                    let n = n + 1;
                    app.set_script_var(format!("直前合体技パートナー[{n}]"), target_key.clone());
                    app.set_script_var(count_key, n.to_string());
                    app.set_script_var("直前合体技ユニット".to_string(), unit_key.clone());
                }
                "憑依" => {
                    // 憑依: 発動 unit のメインパイロットを target unit に転送。
                    // 発動 unit は pilot_name="" になり、target unit が
                    // pilot_name = 発動側パイロット で乗り換え (簡略化)。
                    let src_pilot = app
                        .database()
                        .unit_instances
                        .iter()
                        .find(|u| matches_unit_handle(u, &unit_key))
                        .map(|u| u.pilot_name.clone());
                    if let Some(pilot) = src_pilot {
                        // ソース側を空にする
                        if let Some(u) = app
                            .database_mut()
                            .unit_instances
                            .iter_mut()
                            .find(|u| matches_unit_handle(u, &unit_key))
                        {
                            u.pilot_name = String::new();
                        }
                        // ターゲット側に乗せる
                        if let Some(u) = app
                            .database_mut()
                            .unit_instances
                            .iter_mut()
                            .find(|u| matches_unit_handle(u, &target_key))
                        {
                            u.pilot_name = pilot;
                        }
                    }
                }
                "精神感応" => {
                    // 精神感応: 発動 unit から target unit へ SP の半分を転送。
                    // SRC 仕様の簡略化: 発動 sp_consumed += half, target -= half。
                    let src_sp = app
                        .database()
                        .unit_instances
                        .iter()
                        .find(|u| matches_unit_handle(u, &unit_key))
                        .map(|u| u.sp_consumed);
                    if let Some(consumed) = src_sp {
                        // 発動側の現在 SP を半分転送 (consumed を増やす)
                        let half = (100 - consumed).max(0) / 2;
                        if half > 0 {
                            if let Some(u) = app
                                .database_mut()
                                .unit_instances
                                .iter_mut()
                                .find(|u| matches_unit_handle(u, &unit_key))
                            {
                                u.sp_consumed += half;
                            }
                            if let Some(u) = app
                                .database_mut()
                                .unit_instances
                                .iter_mut()
                                .find(|u| matches_unit_handle(u, &target_key))
                            {
                                u.sp_consumed = (u.sp_consumed - half).max(0);
                            }
                        }
                    }
                }
                "気力増加" | "気力上昇" => {
                    // 対象 unit の士気を 10 上げる (clamp 0..=150)。
                    if let Some(u) = app
                        .database_mut()
                        .unit_instances
                        .iter_mut()
                        .find(|u| matches_unit_handle(u, &target_key))
                    {
                        u.morale = (u.morale + 10).clamp(0, 150);
                    }
                }
                "召喚" => {
                    // `UseAbility 親 召喚 子ユニットデータ [x y]`:
                    // 召喚親と同じ陣営の子ユニットを生成し、summoned_by に親 uid を
                    // 記録する。座標省略時は親ユニットの隣 (右隣、範囲外なら親位置)。
                    // 子ユニットデータ名は target_key (= xargs[2])。
                    let summoned_data = if xargs.len() >= 3 {
                        fn_arg_value(app, &xargs[2])
                    } else {
                        String::new()
                    };
                    // 親ユニットを解決。Place 配置ユニットは uid が空のことが
                    // あるため、空なら一意 uid を割り当ててから関係付けする
                    // (空 uid 同士の誤一致を防ぐ)。
                    let parent_pos = if summoned_data.is_empty() {
                        None
                    } else {
                        let fresh = app.next_unit_id();
                        let p = app
                            .database_mut()
                            .unit_instances
                            .iter_mut()
                            .find(|u| matches_unit_handle(u, &unit_key))
                            .map(|u| {
                                if u.uid.is_empty() {
                                    u.uid = fresh;
                                }
                                (u.uid.clone(), u.party, u.x, u.y)
                            });
                        p
                    };
                    if let Some((parent_uid, party, px, py)) = parent_pos {
                        let (sx, sy) = if xargs.len() >= 5 {
                            (
                                parse_u32(&xargs, 3, line).unwrap_or(px),
                                parse_u32(&xargs, 4, line).unwrap_or(py),
                            )
                        } else {
                            (px, py)
                        };
                        let mut inst =
                            UnitInstance::new(summoned_data, String::new(), party, sx, sy);
                        inst.summoned_by = Some(parent_uid);
                        populate_active_features(&mut inst, app);
                        let uid = app.database_mut().register_unit(inst);
                        app.set_script_var("対象ユニットＩＤ".to_string(), uid);
                    }
                }
                _ => {} // unknown ability — 履歴保存のみ
            }
            return Ok(pc + 1);
        }
        "autotalk" => {
            // SRC `AutoTalk` (`AutoTalkコマンド.md`):
            // 自動会話: 隣接した味方/敵同士で `会話` イベントが定義されていれば
            // 自動的に発火させる。本実装は隣接判定をしつつ `会話 <a> <b>` ラベル
            // 検索で最初にヒットしたペアを発火する。
            // 引数 0: 全 unit ペア走査、1+: 起点 unit 指定。
            let candidates: Vec<usize> = if xargs.is_empty() {
                (0..app.database().unit_instances.len()).collect()
            } else {
                let key = xargs[0].clone();
                app.database()
                    .unit_instances
                    .iter()
                    .enumerate()
                    .filter(|(_, u)| matches_unit_handle(u, &key))
                    .map(|(i, _)| i)
                    .collect()
            };
            for i in candidates {
                // 隣接ユニットと組合せて `会話 <a> <b>` ラベルを試行
                let (x, y) = match app.database().unit_instances.get(i) {
                    Some(u) => (u.x, u.y),
                    None => continue,
                };
                let neighbors: Vec<usize> = (0..app.database().unit_instances.len())
                    .filter(|&j| {
                        if j == i {
                            return false;
                        }
                        let v = &app.database().unit_instances[j];
                        let dx = (v.x as i64 - x as i64).abs();
                        let dy = (v.y as i64 - y as i64).abs();
                        dx + dy == 1
                    })
                    .collect();
                let atk = UnitEventId::from_unit_instance(&app.database().unit_instances[i]);
                for j in neighbors {
                    let def = UnitEventId::from_unit_instance(&app.database().unit_instances[j]);
                    fire_pair_event_labels(app, &["会話", "Conversation"], &atk, &def);
                }
            }
            return Ok(pc + 1);
        }
        "setmessage" => {
            // SRC `SetMessage [type] message` (`SetMessageコマンド.md`):
            // 次戦闘で表示する特別なメッセージをセット。本実装は battle 演出を
            // 持たないため、`script_vars` に保存して `Talk` 等で参照できる
            // ようにする。SRC.NET の `攻撃メッセージ` 等は文字列なので汎用変数
            // で代用する。
            // 引数 1: message のみ → `次戦闘メッセージ` に保存
            // 引数 2: type, message → `次戦闘メッセージ_<type>` に保存
            let (key, value) = match xargs.len() {
                0 => return Ok(pc + 1),
                1 => ("次戦闘メッセージ".to_string(), xargs[0].clone()),
                _ => (
                    format!("次戦闘メッセージ_{}", xargs[0]),
                    xargs[1..].join(" "),
                ),
            };
            let expanded = expand_vars(app, &value);
            app.set_script_var(key, expanded);
            return Ok(pc + 1);
        }
        "cancel" => {
            // SRC `Cancel` (`Cancelコマンド.md`):
            // 直前の対話 (Ask/Menu/Confirm/Input) をキャンセル扱いで閉じる。
            // 通常はユーザ側のキャンセル操作と等価。スクリプトから明示的に
            // 中断したい時 (デバッグや特殊フロー) に使う。
            // 本実装: pending_dialog をクリア + `選択` を 0 にする。
            app.cancel_pending_dialog();
            app.set_script_var("選択".to_string(), "0".to_string());
            return Ok(pc + 1);
        }
        "changemode" => {
            // SRC `ChangeMode [unit] mode` (`ChangeModeコマンド.md`):
            // ユニットの思考モードを変更。本実装は AI が最小限のため、
            // `ai_mode` 文字列フィールドへの単純な代入のみ行う。
            // 1 引数: unit 省略 → selected_unit_for_event。
            // 2 引数: unit + mode。
            // 3 引数: unit + group_id + mode 等の拡張は未対応 (簡略化)。
            let (target, mode) = match xargs.len() {
                0 => return Err(err(line, "ChangeMode 命令は最低 1 引数必要 (mode)。")),
                1 => (app.selected_unit_for_event().to_string(), xargs[0].clone()),
                _ => (xargs[0].clone(), xargs[1..].join(" ")),
            };
            // 陣営指定: 該当陣営の全ユニットを更新 (`味方`/`敵`/`友軍`/`中立`)
            let party = parse_party_label(&target);
            if let Some(p) = party {
                for u in app
                    .database_mut()
                    .unit_instances
                    .iter_mut()
                    .filter(|u| u.party == p)
                {
                    u.ai_mode = mode.clone();
                }
            } else {
                // 単一ユニット指定
                if let Some(u) = app
                    .database_mut()
                    .unit_instances
                    .iter_mut()
                    .find(|u| matches_unit_handle(u, &target))
                {
                    u.ai_mode = mode;
                }
            }
            return Ok(pc + 1);
        }
        "showunitstatus" => {
            // ShowUnitStatus <unit> — ユニット詳細情報を表示
            // Find the unit by name or pilot name
            let Some(unit_name) = xargs.first().map(|s| s.as_str()) else {
                log::warn!("ShowUnitStatus: no unit specified");
                return Ok(pc + 1);
            };
            let Some(unit) = app.database().unit_instances.iter().find(|u| {
                u.unit_data_name == unit_name || u.uid == unit_name || u.pilot_name == unit_name
            }) else {
                log::warn!("ShowUnitStatus: unit '{}' not found", unit_name);
                return Ok(pc + 1);
            };
            // Get unit data
            let Some(unit_data) = app.database().unit_by_name(&unit.unit_data_name) else {
                log::warn!(
                    "ShowUnitStatus: unit data '{}' not found",
                    unit.unit_data_name
                );
                return Ok(pc + 1);
            };
            // Get effective stats (with equipment bonuses)
            let max_hp = app.database().effective_max_hp(unit);
            let max_en = app.database().effective_max_en(unit);
            let armor = app.database().effective_armor(unit);
            let mobility = app.database().effective_mobility(unit);
            let speed = app.database().effective_speed(unit);
            // Current HP/EN considering damage and consumption
            let cur_hp = max_hp - unit.damage;
            let cur_en = max_en - unit.en_consumed;
            // Log the status
            log::info!(
                "ShowUnitStatus: {} (Pilot: {}) HP:{}/{} EN:{}/{} Armor:{} Mobility:{} Speed:{} Morale:{}",
                unit_data.nickname,
                unit.pilot_name,
                cur_hp, max_hp,
                cur_en, max_en,
                armor, mobility, speed,
                unit.morale
            );
            // Push a message to the HUD
            app.push_message(format!(
                "【{}】HP:{}/{} EN:{}/{} 装甲:{} 運動性:{} 移動力:{} 士気:{}",
                unit_data.nickname,
                cur_hp,
                max_hp,
                cur_en,
                max_en,
                armor,
                mobility,
                speed,
                unit.morale
            ));
            return Ok(pc + 1);
        }
        "wait" => {
            // `Wait <duration>` — 秒数指定: pending_timer をセットしてスクリプト
            // を中断 (run_loop が return Ok する経路は pending_dialog 検査のみ
            // なので、pending_timer も同様にチェックする)。
            // `Wait Click` / `Wait Press` / `Wait Key` は対話入力待ちのため
            // PendingDialog で中断し、ユーザ応答で再開。
            // Hotpoint が登録されていれば、その名前を選択肢にした Menu を提示し
            // ユーザ応答で `選択` 変数に hotpoint.name を格納する。
            // SRC `Waitコマンド.md` の 4 書式:
            //   書式1 Wait time        (1 引数)         — 0.1×time 秒待機
            //   書式2 Wait Start       (1 引数 "Start") — 同期基準時刻を記録
            //   書式3 Wait Until time  (2 引数)         — 基準から 0.1×time 秒まで待機
            //   書式4 Wait Click       (1 引数)         — クリック/キー入力待ち
            if xargs.is_empty() {
                return Err(err(line, "Waitコマンドの引数の数が違います"));
            }
            let kind = xargs.first().map(|s| s.as_str()).unwrap_or("");
            let kind_lc = kind.to_ascii_lowercase();
            if kind_lc == "start" {
                // 書式2: 同期基準時刻をリセット (待機はしない)。
                app.reset_wait_clock();
                return Ok(pc + 1);
            }
            if kind_lc == "until" {
                // 書式3: `Wait Until time` — ちょうど 2 引数。基準時刻から
                // 0.1×time 秒経過時まで待つ (= 直前の Wait からの増分だけ待機)。
                if xargs.len() != 2 {
                    return Err(err(line, "Waitコマンドの引数の数が違います"));
                }
                let t: f64 = xargs.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
                let delta = app.advance_wait_clock(0.1 * t);
                if delta > 0.0 {
                    app.set_pending_timer(delta.min(5.0));
                }
                return Ok(pc + 1);
            }
            if xargs.len() > 1 {
                return Err(err(line, "Waitコマンドの引数の数が違います"));
            }
            if matches!(kind_lc.as_str(), "click" | "press" | "key" | "input") {
                // 新しい Wait Click 開始: 右クリック (KeyState(2)) フラグをリセット。
                app.set_wait_click_right(false);
                let hp = app.hotpoints();
                if !hp.is_empty() {
                    let options: Vec<String> = hp.iter().map(|h| h.name.clone()).collect();
                    app.set_pending_dialog(crate::dialog::PendingDialog::Menu {
                        prompt: "選択してください".to_string(),
                        options,
                        var_name: "選択".to_string(),
                        store_value: true,
                        option_keys: Vec::new(),
                        // Hotpoint メニューは右クリックでキャンセル可 (従来どおり)。
                        non_cancellable: false,
                    });
                } else {
                    // 原典 SRC は Wait Click で何も描画せず spin-wait するだけ。
                    // 直前の PaintString / PaintPicture 描画をそのまま見せたいので
                    // `PendingDialog::WaitClick` で suspend だけ行い、レンダリングは
                    // skip させる。
                    app.set_pending_dialog(crate::dialog::PendingDialog::WaitClick);
                }
            } else if !kind.is_empty() {
                // 書式1: 数値秒数。SRC 仕様では実待機は 0.1×time 秒
                // (`Wait 10` = 1 秒)。巨大値での擬似フリーズ防止に 5 秒で
                // クランプする。
                let t: f64 = kind.parse().unwrap_or(0.0);
                let secs = 0.1 * t;
                if secs > 0.0 {
                    app.set_pending_timer(secs.min(5.0));
                }
            }
            return Ok(pc + 1);
        }
        "startbgm" => {
            if let Some(name) = xargs.first() {
                app.push_audio_request(crate::audio::AudioRequest::StartBgm { name: name.clone() });
            }
            return Ok(pc + 1);
        }
        "stopbgm" => {
            app.push_audio_request(crate::audio::AudioRequest::StopBgm);
            return Ok(pc + 1);
        }
        "keepbgm" => {
            app.push_audio_request(crate::audio::AudioRequest::KeepBgm);
            return Ok(pc + 1);
        }
        "playsound" => {
            if let Some(name) = xargs.first() {
                app.push_audio_request(crate::audio::AudioRequest::PlaySound {
                    name: name.clone(),
                });
            }
            return Ok(pc + 1);
        }
        "playvoice" => {
            if let Some(name) = xargs.first() {
                app.push_audio_request(crate::audio::AudioRequest::PlayVoice {
                    name: name.clone(),
                });
            }
            return Ok(pc + 1);
        }
        "playmidi" => {
            // SRC `PlayMIDI name [volume]` (`PlayMIDIコマンド.md`):
            // Midi/<name>.mid を 1 回再生 (BGM とは別チャネル)。
            // volume は本実装で未対応 (フロントエンド側で固定値)。
            if let Some(name) = xargs.first() {
                app.push_audio_request(crate::audio::AudioRequest::PlayMidi { name: name.clone() });
            }
            return Ok(pc + 1);
        }
        "show" => {
            // `Show` — マップウィンドウを表示する SRC コマンド。
            // 本実装ではタイトル / Configuration から MapView に遷移する。
            // Intermission 中はエピローグで `Show` が呼ばれても上書きしない。
            if matches!(
                app.scene(),
                crate::Scene::Title | crate::Scene::Configuration
            ) {
                app.set_scene(crate::Scene::MapView);
            }
            return Ok(pc + 1);
        }
        "hide" => {
            // SRC `Hide` (`Hideコマンド.md`) ─ メインウィンドウを隠す。
            // 本実装はウィンドウ可視フラグを持たないため、`script_overlay` を
            // クリアして「メインウィンドウが空」相当の状態を作る (Cls 相当)。
            // SRC.NET の挙動 (背景画像のみ残す) とは厳密一致しないが、
            // プロローグ風カット導入では十分。
            app.script_overlay_mut().clear();
            return Ok(pc + 1);
        }
        "option" => {
            // SRC `Option name [解除]` (`Optionコマンド.md`):
            // `Option(name)` というグローバル変数に 1 を設定する。
            // 第2引数が存在する場合 (「解除」等) は変数を削除 (SRC.Sharp: UndefineVariable)。
            // 参照は `IsVarDefined("Option(name)")` / `IsOptionDefined()` 等で行う。
            // SRC.Sharp 準拠: 0 個または 3 個以上の引数はエラー。
            if xargs.is_empty() || xargs.len() > 2 {
                return Err(err(line, "Optionコマンドの引数の数が違います"));
            }
            let name = &xargs[0];
            let key = format!("Option({name})");
            if xargs.get(1).is_some() {
                // 「解除」やその他の 2 番目の引数 → 変数を削除
                app.unset_script_var(&key);
            } else {
                app.set_script_var(key, "1".to_string());
            }
            return Ok(pc + 1);
        }
        "changeterrain" => {
            // SRC `ChangeTerrain X Y name bitmap` (`ChangeTerrainコマンド.md`):
            // 座標 (X, Y) の地形を name (TerrainEntry / 組込地形名) に切替し、
            // bitmap_no を上書き。マップが未定義 / 座標範囲外 / 地形未定義は no-op。
            // ローカル地形 (`地形名(ローカル)`) 指定は `(ローカル)` 接尾を剥がして
            // 同名地形を引く (本実装は local-bitmap を区別しない)。
            if xargs.len() < 4 {
                return Err(err(
                    line,
                    "ChangeTerrain 命令は 4 引数必要 (X Y name bitmap)。",
                ));
            }
            let Ok(x) = xargs[0].trim().parse::<u32>() else {
                return Ok(pc + 1);
            };
            let Ok(y) = xargs[1].trim().parse::<u32>() else {
                return Ok(pc + 1);
            };
            let mut name = xargs[2].clone();
            if let Some(stripped) = name.strip_suffix("(ローカル)") {
                name = stripped.to_string();
            }
            let bitmap: u32 = xargs[3].trim().parse().unwrap_or(0);
            // name → terrain_id 解決。scenario terrains → built-in の順。
            let terrain_id = app
                .database()
                .terrains
                .iter()
                .find(|t| t.name == name)
                .map(|t| t.id)
                .or_else(|| {
                    crate::data::terrain::DEFAULT_TERRAINS
                        .iter()
                        .find(|t| t.name == name)
                        .map(|t| t.id)
                });
            let Some(tid) = terrain_id else {
                // 不明な地形名は no-op。エラーを返すと旧シナリオが落ちるため。
                return Ok(pc + 1);
            };
            if let Some(map) = app.database_mut().map.as_mut() {
                if x < map.width && y < map.height {
                    map.set_cell(
                        x,
                        y,
                        MapCell {
                            terrain_id: tid,
                            bitmap_no: bitmap,
                        },
                    );
                }
            }
            return Ok(pc + 1);
        }
        "center" => {
            // SRC `Center x y [option]` または `Center unit [option]`
            // (`Centerコマンド.md`): マップ表示中心を移動。
            // option (`非同期`) は本実装で無視 (即時反映)。
            // 引数 0: no-op、引数 1: unit 名、引数 2+: x y。
            if xargs.is_empty() {
                return Ok(pc + 1);
            }
            // 数値 2 引数なら座標指定。1 引数 (または最初が数値でなければ) は unit 指定。
            if xargs.len() >= 2 {
                if let (Ok(x), Ok(y)) = (
                    xargs[0].trim().parse::<u32>(),
                    xargs[1].trim().parse::<u32>(),
                ) {
                    app.set_map_cursor(x, y);
                    return Ok(pc + 1);
                }
            }
            // unit 指定
            let key = fn_arg_value(app, &xargs[0]);
            let pos = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &key))
                .map(|u| (u.x, u.y));
            if let Some((x, y)) = pos {
                app.set_map_cursor(x, y);
            }
            return Ok(pc + 1);
        }
        "changemap" => {
            // `ChangeMap <path> [非同期]` — シナリオ ZIP からロード済みの `.map`
            // (basename 一致) を `GameDatabase.map` に差し替える。見つからない
            // 場合は前のマップを維持。
            if let Some(path) = xargs.first() {
                if let Some(replacement) = app.database().find_map(path).cloned() {
                    app.database_mut().map = Some(replacement);
                }
            }
            return Ok(pc + 1);
        }
        "refresh" => {
            // 元 SRC `Refresh` は **画面の強制再描画 (present) のみ**で、
            // 蓄積した描画内容はクリアしない (クリアは `Cls` / `ClearPicture` /
            // `ClearObj` の役割)。本実装のレンダラは `script_overlay` を毎フレーム
            // 描き直すので、`Refresh` は実質 no-op。
            //
            // 以前は `script_overlay.clear()` していたが、これは誤りで、
            // `draw / Refresh / draw` で背景フレームが消えてしまう。スパロボ戦記の
            // タイトル系 (`PaintPicture メッセージウィンドウ / Refresh / Wait` の
            // フレームアニメ、`HotpointString ... / Refresh / Wait Click` 等) は
            // Refresh の後も描画が残ることを前提にしており、クリアすると
            // 「画面が一瞬見えてすぐ消える」状態になっていた。
            //
            // present(): 保留中の ClearPicture があればここで適用する
            // (`ClearPicture; Refresh` でバックバッファの空を表示するケース)。
            // 通常パターン `Paint; Refresh; ClearPicture` では Refresh 時点で
            // 保留は無いので従来どおり no-op。
            app.script_overlay_mut().present();
            return Ok(pc + 1);
        }
        "hotpointstring" => {
            // `HotpointString x y text...` — 文字列を描画しつつ、その矩形領域を
            // クリック領域として登録する。x が "-" の場合は水平方向中央寄せ
            // (簡易実装)。クリック名はテキストそのもの。
            let (x, y, text) = parse_paintstring_args(app, &xargs);
            if !text.is_empty() {
                app.script_overlay_mut()
                    .push(crate::script_overlay::DrawCmd::PaintString {
                        x,
                        y,
                        text: text.clone(),
                    });
                // Hotpoint 領域の幅は文字数 × ピクセル係数 (概算)。
                // フォントサイズが取れないので 14pt 相当 (高さ ~20px) を仮定。
                let char_count = text.chars().count() as i32;
                let w = (char_count * 14).max(40);
                let h = 22;
                app.push_hotpoint(crate::event_runtime::HotpointEntry {
                    name: text,
                    x: x as i32,
                    y: y as i32 - h,
                    w,
                    h,
                    invisible: false,
                });
            }
            return Ok(pc + 1);
        }
        "hotpoint" => {
            // `Hotpoint name x y w h [非表示]`
            // name = クリック時に `選択` に格納される文字列 (unit 名など)
            // 引数は算術式 `(x - 25)` 形式が含まれるので eval する。
            if xargs.is_empty() {
                return Ok(pc + 1);
            }
            let name = xargs[0].clone();
            // 座標 / サイズは `(j * 40 - 35)` のようにループ変数を含む算術式に
            // なりうるので、裸の識別子を解決する app-aware 評価を使う。
            let x = xargs.get(1).map(|s| eval_int_expr_app(app, s)).unwrap_or(0);
            let y = xargs.get(2).map(|s| eval_int_expr_app(app, s)).unwrap_or(0);
            let w = xargs.get(3).map(|s| eval_int_expr_app(app, s)).unwrap_or(0);
            let h = xargs.get(4).map(|s| eval_int_expr_app(app, s)).unwrap_or(0);
            let invisible = xargs
                .get(5)
                .map(|s| s == "非表示" || s.eq_ignore_ascii_case("invisible"))
                .unwrap_or(false);
            app.push_hotpoint(crate::event_runtime::HotpointEntry {
                name,
                x,
                y,
                w,
                h,
                invisible,
            });
            return Ok(pc + 1);
        }
        "clearpicture" => {
            // `ClearPicture` — Paint / PaintPicture で積んだ表示を消去。
            // ただし SRC immediate-mode ではバックバッファのみを消し、画面は直前
            // Refresh が表示したフレームを次の present まで保持する。本実装は
            // 遅延クリア (defer_clear) で、次の描画 push か Refresh まで cmds を残す。
            // → 汎用戦闘アニメの `Paint; Refresh; ClearPicture; Wait` 各フレームが
            //   Wait 中も正しく表示される (即 clear すると毎フレーム空になっていた)。
            // Hotpoint は別管理なのでそのまま。
            app.script_overlay_mut().defer_clear();
            return Ok(pc + 1);
        }
        "clearobj" => {
            // `ClearObj` — オブジェクト消去 = Hotpoint と画像オブジェクトを
            // 一括クリア。Refresh とは別に明示的に呼ばれる。
            app.script_overlay_mut().clear();
            app.clear_hotpoints();
            return Ok(pc + 1);
        }
        "font" => {
            // `Font [family] [Npt] [Bold|Italic|Underline] [#color | カラー名]`
            // 引数順は任意 (実シナリオは "14pt Bold" / "50pt Italic Bold" /
            // "Ｐゴシック 14pt" / "#5E5E5E" 等の様々な並びを使う)。
            // 引数 0 個なら既定値にリセット。
            let mut family = String::from("sans-serif");
            let mut size: u32 = 14;
            let mut color = String::from("#ffffff");
            let mut style_bold = false;
            let mut style_italic = false;
            // 各トークンを走査する前に、定義済みスクリプト変数を値へ展開する。
            // `Font LetterShadow` / `Font LetterColor1` のように変数でフォント
            // 指定を切り替える実シナリオ記法 (Alpha2ndStatus.ini 由来のテーマ
            // 設定) に対応するため。変数値は "10pt #000000" のように複数トークン
            // でありうるので空白で再分割する。未定義変数はリテラル扱い。
            let mut tokens: Vec<String> = Vec::new();
            for tok in xargs.iter() {
                let t = tok.trim();
                if t.is_empty() {
                    continue;
                }
                let v = app.script_var(t);
                if !v.is_empty() {
                    tokens.extend(v.split_whitespace().map(str::to_string));
                } else {
                    tokens.push(t.to_string());
                }
            }
            for tok in &tokens {
                let t = tok.trim();
                if t.is_empty() {
                    continue;
                }
                // サイズ: "14pt" / "14"
                let size_candidate = t.strip_suffix("pt").or_else(|| t.strip_suffix("PT"));
                if let Some(sn) = size_candidate {
                    if let Ok(n) = sn.parse::<u32>() {
                        size = n;
                        continue;
                    }
                }
                if let Ok(n) = t.parse::<u32>() {
                    if (6..=144).contains(&n) {
                        size = n;
                        continue;
                    }
                }
                // 色: "#RRGGBB" or 色名 (canonical_color が日本語色名にも対応)
                let lc = t.to_ascii_lowercase();
                if t.starts_with('#') || lc.starts_with("rgb(") {
                    color = canonical_color(t);
                    continue;
                }
                match t {
                    "Bold" | "bold" => {
                        style_bold = true;
                        continue;
                    }
                    "Italic" | "italic" => {
                        style_italic = true;
                        continue;
                    }
                    "Underline" | "underline" => continue,
                    "黒" | "白" | "赤" | "緑" | "青" | "黄" | "灰" => {
                        color = canonical_color(t);
                        continue;
                    }
                    _ => {}
                }
                // 残りは family 名として扱う
                family = t.to_string();
            }
            // style はフロントエンドが family 名から拾えるよう接頭辞で表現する。
            // canvas2d は font 文字列 "italic bold 14pt Family" を受け付ける。
            let prefix = match (style_italic, style_bold) {
                (true, true) => "italic bold ",
                (true, false) => "italic ",
                (false, true) => "bold ",
                (false, false) => "",
            };
            let family = format!("{prefix}{family}");
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::SetFont {
                    family,
                    size_pt: size,
                    color,
                });
            return Ok(pc + 1);
        }
        "paintstring" | "paintstringr" | "paintsysstring" => {
            // `PaintString x y text...` または `PaintString text x y` の二形式に
            // 対応するため、整数が見つかる位置で分岐。
            // 簡略実装: 引数の最初の 2 整数を x,y、残りを連結して text。
            let (x, y, text) = parse_paintstring_args(app, &xargs);
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::PaintString { x, y, text });
            return Ok(pc + 1);
        }
        "paintpicture" => {
            // `PaintPicture path x y [w h] [透過] [左右反転]`
            // 算術式 `(x - 5)` を含むのでそれぞれ eval_int_expr で評価。
            // 座標 / サイズに "-" が指定された場合は「中央寄せ」「自然サイズ」を意味する。
            if xargs.is_empty() {
                return Ok(pc + 1);
            }
            let path = xargs[0].clone();
            let parse_int_or_dash = |s: &str| -> Option<i32> {
                if s.trim() == "-" {
                    None
                } else {
                    // `(j * 40 - 35)` 等のループ変数入り算術式を解決。
                    Some(eval_int_expr_app(app, s))
                }
            };
            // x/y: "-" は MAP_AREA 中央寄せ。サイズは後で w/h と組み合わせて確定する。
            let raw_x = xargs.get(1).and_then(|s| parse_int_or_dash(s));
            let raw_y = xargs.get(2).and_then(|s| parse_int_or_dash(s));
            // オプション (透過 / 反転 / 白黒 等) は w/h を省略すると index 3 以降に
            // 直接現れる (`PaintPicture img - - 透過`)。最初のオプションキーワード
            // 位置 `opts_start` を求め、それより前の数値引数だけを w/h として解釈
            // する。これが無いと `透過` が w として食われ (eval で 0 → None)、
            // 透過フラグが index 5 以降からしか拾われず効かないため、不透明描画
            // となって機体能力.png が画面全体を覆い隠していた。
            let opts_start = (3..xargs.len())
                .find(|&i| is_paint_option(&xargs[i]))
                .unwrap_or(xargs.len());
            // w/h: `0` 以下は「画像の自然サイズを使う」とみなして None にする。
            // スパロボ戦記タイトルは `PaintPicture 画像 x y Lindex(Info(...全身画像),3) ...`
            // のようにサイズを Info() から取るが、未対応データだと空文字 → 0 と
            // 評価され、0×0 で何も描画されなくなるため (機体アイコンが出ない)。
            let w = if opts_start > 3 {
                xargs
                    .get(3)
                    .and_then(|s| parse_int_or_dash(s))
                    .map(f64::from)
                    .filter(|v| *v > 0.0)
            } else {
                None
            };
            let h = if opts_start > 4 {
                xargs
                    .get(4)
                    .and_then(|s| parse_int_or_dash(s))
                    .map(f64::from)
                    .filter(|v| *v > 0.0)
            } else {
                None
            };
            const MAP_CENTER_X: f64 = 240.0;
            const MAP_CENTER_Y: f64 = 240.0;
            // 座標 `-` は中央寄せ。幅が明示されていれば src-core で確定できるが、
            // `PaintPicture img - -`（幅省略 = 画像実寸）の場合は実寸が分かる
            // フロントエンドで確定させる必要があるため center フラグを立てる。
            // (src-core の x/y はプレースホルダ。w 明示時も render が同値に再計算する)
            let center_x = raw_x.is_none();
            let center_y = raw_y.is_none();
            let x = match raw_x {
                Some(v) => f64::from(v),
                None => MAP_CENTER_X - w.unwrap_or(0.0) / 2.0,
            };
            let y = match raw_y {
                Some(v) => f64::from(v),
                None => MAP_CENTER_Y - h.unwrap_or(0.0) / 2.0,
            };
            let mut transparent = false;
            let mut flip_x = false;
            let mut flip_y = false;
            let mut monochrome = false;
            let mut sepia = false;
            let mut half_mode = String::new();
            let mut rotation_deg: f64 = 0.0;
            let mut as_background = false;
            let mut persist = false;
            let mut i_flag = opts_start;
            while i_flag < xargs.len() {
                let flag = &xargs[i_flag];
                let l = flag.to_ascii_lowercase();
                if flag == "透過" || l == "transparent" {
                    transparent = true;
                } else if flag == "左右反転" || l == "flip" || l == "flipx" || l == "fliph" {
                    flip_x = true;
                } else if flag == "上下反転" || l == "flipy" || l == "flipv" {
                    flip_y = true;
                } else if flag == "白黒" || l == "monochrome" || l == "grayscale" {
                    monochrome = true;
                } else if flag == "セピア" || l == "sepia" {
                    sepia = true;
                } else if flag == "背景" || l == "background" {
                    as_background = true;
                } else if flag == "保持" || l == "persist" || l == "keep" {
                    persist = true;
                } else if flag == "右回転" || l == "rotate" || l == "rotateright" {
                    // 次の引数を角度として消費
                    if let Some(angle_arg) = xargs.get(i_flag + 1) {
                        if let Ok(a) = angle_arg.trim().parse::<f64>() {
                            rotation_deg = a;
                            i_flag += 1;
                        }
                    }
                } else if flag == "左回転" || l == "rotateleft" {
                    if let Some(angle_arg) = xargs.get(i_flag + 1) {
                        if let Ok(a) = angle_arg.trim().parse::<f64>() {
                            rotation_deg = -a;
                            i_flag += 1;
                        }
                    }
                } else if matches!(
                    flag.as_str(),
                    "上半分" | "下半分" | "左半分" | "右半分" | "右上" | "左上" | "右下" | "左下"
                ) {
                    half_mode = flag.clone();
                }
                i_flag += 1;
            }
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::Picture {
                    path,
                    x,
                    y,
                    w,
                    h,
                    transparent,
                    flip_x,
                    flip_y,
                    monochrome,
                    sepia,
                    half_mode,
                    rotation_deg,
                    as_background,
                    persist,
                    center_x,
                    center_y,
                });
            return Ok(pc + 1);
        }
        "color" => {
            // `Color RGB(r,g,b)` / `Color "Red"` / `Color rgb(...)` /
            // `Color FrameColor1` (.ini 由来のテーマ色変数) → 描画色を更新
            let color = xargs.join(" ").trim().trim_matches('"').to_string();
            if !color.is_empty() {
                let c = resolve_color(app, &color);
                app.script_overlay_mut()
                    .push(crate::script_overlay::DrawCmd::SetColor { color: c });
            }
            return Ok(pc + 1);
        }
        "drawwidth" => {
            let n: f64 = xargs
                .first()
                .map(|s| eval_int_expr(s) as f64)
                .unwrap_or(1.0);
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::SetLineWidth(n.max(0.5)));
            return Ok(pc + 1);
        }
        "cls" => {
            // ClearPicture と同義 (画面消去)
            app.script_overlay_mut().clear();
            return Ok(pc + 1);
        }
        "sort" => {
            // `Sort array_name [昇順|降順] [数値|文字] [インデックスのみ|文字インデックス]`
            // (SRC.Sharp `SortCmd.cs` 準拠)
            //
            // 昇順/降順: ソート方向 (デフォルト: 昇順)
            // 数値/文字: 値の比較方法 (デフォルト: 自動判定)
            // インデックスのみ: インデックス(キー)だけでソートし値は付随して移動
            // 文字インデックス: 文字列キーを持つ配列 (キーも一緒に移動)
            if xargs.is_empty() {
                return Err(err(line, "Sort には配列変数名が必要。"));
            }
            let arr_name = xargs[0].clone();
            let mut order_asc = true;
            let mut force_string_val = false;
            let mut is_key_sort = false;
            let mut is_string_key = false;
            for opt in &xargs[1..] {
                match opt.as_str() {
                    "昇順" => order_asc = true,
                    "降順" => order_asc = false,
                    "数値" => force_string_val = false,
                    "文字" => force_string_val = true,
                    "インデックスのみ" => is_key_sort = true,
                    "文字インデックス" => is_string_key = true,
                    other => {
                        return Err(err(
                            line,
                            &format!(
                                "Sort コマンドに不正なオプション「{}」が使われています",
                                other
                            ),
                        ))
                    }
                }
            }
            let prefix = format!("{arr_name}[");
            let mut pairs: Vec<(String, String)> = app
                .script_vars()
                .iter()
                .filter(|(k, _)| k.starts_with(&prefix) && k.ends_with(']'))
                .map(|(k, v)| (k[prefix.len()..k.len() - 1].to_string(), v.clone()))
                .collect();
            if pairs.is_empty() {
                return Ok(pc + 1);
            }
            // 非数値キーが存在する場合は自動的に文字インデックス扱いにする
            if !is_string_key && pairs.iter().any(|(k, _)| k.parse::<f64>().is_err()) {
                is_string_key = true;
            }
            // 非数値の値が存在する場合は文字列比較に切り替え
            let use_string_cmp =
                force_string_val || pairs.iter().any(|(_, v)| v.parse::<f64>().is_err());
            // 値比較クロージャ
            let cmp_val = |a: &str, b: &str| -> std::cmp::Ordering {
                if use_string_cmp {
                    a.cmp(b)
                } else {
                    let av = a.parse::<f64>().unwrap_or(0.0);
                    let bv = b.parse::<f64>().unwrap_or(0.0);
                    av.partial_cmp(&bv).unwrap_or(std::cmp::Ordering::Equal)
                }
            };
            // 数値キー比較クロージャ
            let cmp_num_key = |a: &str, b: &str| -> std::cmp::Ordering {
                let ak = a.parse::<f64>().unwrap_or(0.0);
                let bk = b.parse::<f64>().unwrap_or(0.0);
                ak.partial_cmp(&bk).unwrap_or(std::cmp::Ordering::Equal)
            };
            if is_string_key {
                // 文字インデックス: (キー, 値) ペアを一緒に移動
                // インデックスのみ → キーで, それ以外 → 値でソート
                pairs.sort_by(|a, b| {
                    let cmp = if is_key_sort {
                        a.0.cmp(&b.0)
                    } else {
                        cmp_val(&a.1, &b.1)
                    };
                    if order_asc {
                        cmp
                    } else {
                        cmp.reverse()
                    }
                });
            } else {
                // 数値インデックス:
                // ステップ1: キーをソート方向順に並べる (キー位置を確定させる)
                pairs.sort_by(|a, b| {
                    let cmp = cmp_num_key(&a.0, &b.0);
                    if order_asc {
                        cmp
                    } else {
                        cmp.reverse()
                    }
                });
                if !is_key_sort {
                    // ステップ2: キーは固定し、値だけをソートして再配置
                    let keys: Vec<String> = pairs.iter().map(|(k, _)| k.clone()).collect();
                    let mut vals: Vec<String> = pairs.iter().map(|(_, v)| v.clone()).collect();
                    vals.sort_by(|a, b| {
                        let cmp = cmp_val(a, b);
                        if order_asc {
                            cmp
                        } else {
                            cmp.reverse()
                        }
                    });
                    pairs = keys.into_iter().zip(vals).collect();
                }
                // インデックスのみ: ステップ1 のキー順でペアはそのまま
            }
            // 書き戻し
            for (key, val) in &pairs {
                app.set_script_var(format!("{arr_name}[{key}]"), val.clone());
            }
            return Ok(pc + 1);
        }
        "array" => {
            // SRC `Array variable string separator` (`Arrayコマンド.md`):
            // string を separator で分割して variable[1..N] に格納。
            // separator が `リスト` ならリスト形式 (空白区切り) として解釈。
            if xargs.len() < 3 {
                return Err(err(line, "Array 命令は 3 引数必要 (var string sep)。"));
            }
            let var_name = xargs[0].clone();
            let value = expand_vars(app, &fn_arg_value(app, &xargs[1]));
            let sep = fn_arg_value(app, &xargs[2]);
            let sep = sep.trim().trim_matches('"');
            let parts: Vec<&str> = if sep == "リスト" || sep.eq_ignore_ascii_case("list") {
                // リスト形式 = 空白区切り (Llength/Lindex と同じ規約)
                value.split_whitespace().collect()
            } else if sep.is_empty() {
                // 区切り文字無指定 → 文字列を 1 文字ずつ
                value.split("").filter(|s| !s.is_empty()).collect()
            } else {
                value.split(sep).collect()
            };
            // 既存の variable[*] 要素は SRC では維持されるが、本実装では
            // 上書き範囲のみ書き換える (元のサイズより小さい場合に余りを残す)。
            for (i, part) in parts.iter().enumerate() {
                let key = format!("{var_name}[{}]", i + 1);
                app.set_script_var(key, part.to_string());
            }
            return Ok(pc + 1);
        }
        "swap" => {
            // SRC `Swap var1 var2` (`Swapコマンド.md`): 2 変数の値交換。
            // 配列要素も addressable なら受理 (LHS 解決経由)。
            // SRC.Sharp 準拠: 正確に 2 引数必要。
            if xargs.len() < 2 {
                return Err(err(line, "Swap 命令は 2 引数必要 (var1 var2)。"));
            }
            if xargs.len() > 2 {
                return Err(err(line, "Swapコマンドの引数の数が違います"));
            }
            let a = resolve_lhs_name(app, &xargs[0]);
            let b = resolve_lhs_name(app, &xargs[1]);
            let av = app.script_var(&a).to_string();
            let bv = app.script_var(&b).to_string();
            app.set_script_var(a, bv);
            app.set_script_var(b, av);
            return Ok(pc + 1);
        }
        "copyarray" => {
            // SRC `CopyArray src dst` (`CopyArrayコマンド.md`):
            // `src[*]` の全要素を `dst[*]` にコピー。dst の既存要素は維持しつつ
            // 同じキーを上書きする (SRC.NET 仕様)。完全置換ではない。
            // SRC.Sharp 準拠: 正確に 2 引数必要。
            if xargs.len() < 2 {
                return Err(err(line, "CopyArray 命令は 2 引数必要 (src dst)。"));
            }
            if xargs.len() > 2 {
                return Err(err(line, "CopyArrayコマンドの引数の数が違います"));
            }
            let src = xargs[0].clone();
            let dst = xargs[1].clone();
            let prefix = format!("{src}[");
            let pairs: Vec<(String, String)> = app
                .script_vars()
                .iter()
                .filter(|(k, _)| k.starts_with(&prefix) && k.ends_with(']'))
                .map(|(k, v)| {
                    let idx = &k[prefix.len()..k.len() - 1];
                    (format!("{dst}[{idx}]"), v.clone())
                })
                .collect();
            for (k, v) in pairs {
                app.set_script_var(k, v);
            }
            return Ok(pc + 1);
        }
        "global" => {
            // SRC `Global variable` (`Globalコマンド.md`): グローバル変数宣言。
            // 本実装は単一スコープだが、宣言していない変数は `IsVarDefined` = 0 を
            // 返すため、未定義ならば空文字で初期化する必要がある。
            // SRC.Sharp `GlobalCmd.cs` の実装:
            //   - 各引数を変数名として処理 (複数指定可能: `Global g1 g2 g3`)
            //   - 先頭が `$` なら除去して登録 (`Global $counter` → `counter`)
            //   - 既定義の場合は値を変更しない (保持)
            for arg in &xargs {
                let vname = arg.trim_start_matches('$');
                if !app.is_script_var_defined(vname) {
                    app.set_script_var(vname.to_string(), String::new());
                }
            }
            return Ok(pc + 1);
        }
        "require" => {
            // `Require <path>` — 設定ファイル (`.ini`) を取り込み、その
            // トップレベル `key = value` 行をスクリプト変数として適用する。
            // Alpha2ndStatus.ini の LetterColor / FrameColor 等のテーマ設定は
            // これで定義される。対象は archive ローダが script_library に
            // 登録済み (自動実行はされない)。
            if let Some(path) = xargs.first() {
                apply_required_file(app, path);
            }
            return Ok(pc + 1);
        }
        "savescreen" => {
            // SRC `SaveScreen` (`SaveScreenコマンド.md`):
            // 現在の script_overlay をスナップショットとして保存。LoadScreen で復元。
            // serde 経由で JSON 化して script_var に格納する。
            if let Ok(json) = serde_json::to_string(app.script_overlay()) {
                app.set_script_var("__screen_snapshot".to_string(), json);
            }
            return Ok(pc + 1);
        }
        "loadscreen" => {
            // SRC `LoadScreen` (`LoadScreenコマンド.md`): SaveScreen で保存した
            // スナップショットを復元。script_overlay 全体を置換。
            let snapshot = app.script_var("__screen_snapshot").to_string();
            if !snapshot.is_empty() {
                if let Ok(restored) =
                    serde_json::from_str::<crate::script_overlay::ScriptOverlay>(&snapshot)
                {
                    *app.script_overlay_mut() = restored;
                }
            }
            return Ok(pc + 1);
        }
        "playflash" => {
            // SRC `PlayFlash file` (Flash 演出再生): フロントエンド側で対応する
            // 演出が無いため、`__playing_flash` フラグを script_var に立てる
            // (シナリオ側の `If 演出中 ...` 風の分岐用)。
            if let Some(name) = xargs.first() {
                app.set_script_var("__playing_flash".to_string(), name.clone());
            }
            return Ok(pc + 1);
        }
        "stopflash" | "clearflash" => {
            // SRC `StopFlash` / `ClearFlash`: Flash 演出停止。
            app.set_script_var("__playing_flash".to_string(), String::new());
            return Ok(pc + 1);
        }
        "debug" => {
            // SRC `Debug message` (`Debugコマンド.md`):
            // メッセージをログに出力 ([INF] レベル)。本実装は app.messages に追記。
            let msg = xargs.join(" ");
            log::info!("[EVE Debug] {msg}");
            app.push_message(format!("[Debug] {msg}"));
            return Ok(pc + 1);
        }
        "quit" => {
            // SRC `Quit` (`Quitコマンド.md`): `Suspend` 相当 — タイトルへ戻る。
            app.set_scene(crate::Scene::Title);
            return Ok(pc + 1);
        }
        "pause" => {
            // SRC `Pause` (`Pauseコマンド.md`): `Wait` 相当の一時停止。
            // 引数があれば Wait と同じ処理、なければ 0 秒待機。
            let secs: f64 = xargs
                .first()
                .and_then(|s| fn_arg_value(app, s).parse().ok())
                .unwrap_or(0.0);
            if secs > 0.0 {
                app.set_pending_timer(secs.min(5.0));
            }
            return Ok(pc + 1);
        }
        "decreasemorale" => {
            // SRC `DecreaseMorale [unit] value` (`IncreaseMoraleコマンド.md` の逆):
            // ユニットの気力を減少させる。
            if xargs.is_empty() {
                return Err(err(line, "DecreaseMorale には対象名が必要。"));
            }
            let delta = if xargs.len() >= 2 {
                parse_i32_at(&xargs[1], line)?
            } else {
                5
            };
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &xargs[0]))
            {
                // C# では DecreaseMorale は IncreaseMorale の逆で同じ Pilot.SetMorale を
                // 経由するため、下限は 50 (MinMorale 既定値)。
                u.morale = (u.morale - delta).clamp(50, 150);
            }
            return Ok(pc + 1);
        }
        "sunset" => {
            // SRC `Sunset` コマンド: マップを夕方状態にする。
            app.set_time_of_day("夕");
            return Ok(pc + 1);
        }
        "noon" => {
            // SRC `Noon` コマンド: マップを昼状態にする。
            app.set_time_of_day("昼");
            return Ok(pc + 1);
        }
        "night" => {
            // SRC `Night` コマンド: マップを夜状態にする。
            app.set_time_of_day("夜");
            return Ok(pc + 1);
        }
        "changeunitbitmap" => {
            // SRC `ChangeUnitBitmap [unit] bitmap` (`ChangeUnitBitmapコマンド.md`):
            // ユニットのビットマップを一時的に変更する。`-` で元に戻す。
            // `非表示`/`非表示解除` でユニットを非表示化/解除。
            if xargs.is_empty() {
                return Ok(pc + 1);
            }
            let (unit_key, bitmap) = if xargs.len() >= 2 {
                (xargs[0].clone(), xargs[1].clone())
            } else {
                (app.selected_unit_for_event().to_string(), xargs[0].clone())
            };
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &unit_key))
            {
                match bitmap.as_str() {
                    "非表示" => u.is_bitmap_hidden = true,
                    "非表示解除" => u.is_bitmap_hidden = false,
                    "-" => {
                        u.bitmap_override = None;
                        u.is_bitmap_hidden = false;
                    }
                    _ => u.bitmap_override = Some(bitmap),
                }
            }
            return Ok(pc + 1);
        }
        "changepilotbitmap" => {
            // SRC `ChangePilotBitmap [pilot] bitmap` (`ChangeUnitBitmapコマンド.md` 参照):
            // パイロットのビットマップを一時的に変更する。`-` で元に戻す。
            // PilotInstance は通常空なので script_var `__pilot_bitmap_<name>` に格納。
            if xargs.is_empty() {
                return Ok(pc + 1);
            }
            let (pilot_key, bitmap) = if xargs.len() >= 2 {
                (xargs[0].clone(), xargs[1].clone())
            } else {
                (app.selected_unit_for_event().to_string(), xargs[0].clone())
            };
            // PilotInstance が存在すればそちらを優先更新。
            if let Some(p) = app
                .database_mut()
                .pilot_instances
                .iter_mut()
                .find(|p| p.pilot_data_name == pilot_key)
            {
                p.bitmap_override = if bitmap == "-" {
                    None
                } else {
                    Some(bitmap.clone())
                };
            }
            // script_vars にも記録 (PilotInstance 不在のケースに備えるフォールバック)。
            let key = format!("__pilot_bitmap_{pilot_key}");
            if bitmap == "-" {
                app.unset_script_var(&key);
            } else {
                app.set_script_var(key, bitmap);
            }
            return Ok(pc + 1);
        }
        "changeunitclass" => {
            // SRC `ChangeUnitClass [unit] class` (`ChangeUnitClassコマンド.md`):
            // ユニットの分類を一時的に変更する。`-` で元に戻す。
            if xargs.is_empty() {
                return Ok(pc + 1);
            }
            let (unit_key, class) = if xargs.len() >= 2 {
                (xargs[0].clone(), xargs[1].clone())
            } else {
                (app.selected_unit_for_event().to_string(), xargs[0].clone())
            };
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &unit_key))
            {
                u.class_override = if class == "-" { None } else { Some(class) };
            }
            return Ok(pc + 1);
        }
        "renameterm" => {
            // RenameTerm term name — 用語の表示名を変更する。
            // `Term(term)` 関数で読み戻せるよう `__term_<term>` に保存。
            // 例: `RenameTerm スペシャルパワー ヒーローアクション`
            // SRC.Sharp 準拠: 正確に 2 引数必要。
            if xargs.len() != 2 {
                return Err(err(line, "RenameTermコマンドの引数の数が違います"));
            }
            let term = &xargs[0];
            let new_name = &xargs[1];
            let key = format!("__term_{term}");
            app.set_script_var(key, new_name.clone());
            return Ok(pc + 1);
        }
        "createfolder" => {
            // CreateFolder path — 仮想フォルダを作成 (VFS では path prefix として扱う)。
            // VFS は BTreeMap<path, lines> なのでフォルダそのものは不要だが、
            // スクリプトが存在確認する場合に備えて空エントリを入れておく。
            if let Some(path) = xargs.first() {
                let norm = format!(
                    "{}/",
                    path.trim()
                        .trim_matches('"')
                        .to_ascii_lowercase()
                        .replace('\\', "/")
                );
                app.vfs_ensure_folder(&norm);
            }
            return Ok(pc + 1);
        }
        "removefolder" => {
            // RemoveFolder path — 指定パス以下のファイルを全削除。
            if let Some(path) = xargs.first() {
                let prefix = path
                    .trim()
                    .trim_matches('"')
                    .to_ascii_lowercase()
                    .replace('\\', "/");
                app.vfs_remove_folder(&prefix);
            }
            return Ok(pc + 1);
        }
        "removefile" => {
            // RemoveFile path — 仮想ファイルを削除。
            if let Some(path) = xargs.first() {
                app.vfs_remove_file(path);
            }
            return Ok(pc + 1);
        }
        "renamefile" => {
            // RenameFile old new — 仮想ファイルのパスを変更。
            if xargs.len() >= 2 {
                app.vfs_rename_file(&xargs[0], &xargs[1]);
            }
            return Ok(pc + 1);
        }
        "copyfile" => {
            // CopyFile src dst — 仮想ファイルをコピー。
            if xargs.len() >= 2 {
                app.vfs_copy_file(&xargs[0], &xargs[1]);
            }
            return Ok(pc + 1);
        }
        "setwindowframewidth" => {
            // `SetWindowFrameWidth n` (SRC.Sharp `SetWindowFrameWidthCmd.cs`):
            // ステータスウィンドウの枠幅を n に設定し、グローバル変数
            // `StatusWindow(FrameWidth)` に n を格納する。
            if let Some(arg) = xargs.first() {
                let n = fn_arg_value(app, arg);
                app.set_script_var("StatusWindow(FrameWidth)".to_string(), n);
            }
            return Ok(pc + 1);
        }
        "setwindowcolor" => {
            // `SetWindowColor #RRGGBB [枠|背景]` (SRC.Sharp `SetWindowColorCmd.cs`):
            // ウィンドウ色を設定。色は `#rrggbb` (7 文字) で指定。
            // Windows COLORREF 形式 `(B<<16)|(G<<8)|R` に変換して格納。
            // 対象: `枠`=FrameColor のみ / `背景`=BackBolor のみ / なし=両方。
            // ※ "BackBolor" は SRC.Sharp の typo (BackColor の誤記) をそのまま踏襲。
            if xargs.is_empty() {
                return Err(err(line, "SetWindowColor には色指定が必要。"));
            }
            let color_str = fn_arg_value(app, &xargs[0]);
            // `#RRGGBB` 形式チェック
            if color_str.len() != 7 || !color_str.starts_with('#') {
                return Err(err(line, "SetWindowColor: 色指定が不正 (#rrggbb 形式)。"));
            }
            let hex = &color_str[1..];
            let rgb = match u32::from_str_radix(hex, 16) {
                Ok(v) => v,
                Err(_) => return Err(err(line, "SetWindowColor: 色指定が不正 (16進数不正)。")),
            };
            // COLORREF = (B<<16)|(G<<8)|R
            let r = (rgb >> 16) & 0xFF;
            let g = (rgb >> 8) & 0xFF;
            let b = rgb & 0xFF;
            let colorref = (b << 16) | (g << 8) | r;

            let target = xargs.get(1).map(|s| s.as_str()).unwrap_or("");
            let set_frame = target == "枠" || target.is_empty();
            let set_bg = target == "背景" || target.is_empty();
            if !set_frame && !set_bg {
                return Err(err(
                    line,
                    "SetWindowColor: 対象は「枠」または「背景」のみ。",
                ));
            }
            if set_frame {
                app.set_script_var("StatusWindow(FrameColor)".to_string(), colorref.to_string());
            }
            if set_bg {
                app.set_script_var("StatusWindow(BackBolor)".to_string(), colorref.to_string());
            }
            return Ok(pc + 1);
        }
        "setstatusstringcolor" => {
            // `SetStatusStringColor #RRGGBB <対象>` (SRC.Sharp `SetStatusStringColorCmd.cs`):
            // ステータスウィンドウの文字色を設定。色は `#rrggbb` 形式。
            // 対象: `通常`=StringColor / `能力名`=ANameColor / `有効`=EnableColor / `無効`=DisableColor
            if xargs.len() < 2 {
                return Err(err(line, "SetStatusStringColor には色指定と対象が必要。"));
            }
            let color_str = fn_arg_value(app, &xargs[0]);
            if color_str.len() != 7 || !color_str.starts_with('#') {
                return Err(err(
                    line,
                    "SetStatusStringColor: 色指定が不正 (#rrggbb 形式)。",
                ));
            }
            let hex = &color_str[1..];
            let rgb = match u32::from_str_radix(hex, 16) {
                Ok(v) => v,
                Err(_) => {
                    return Err(err(
                        line,
                        "SetStatusStringColor: 色指定が不正 (16進数不正)。",
                    ))
                }
            };
            // COLORREF = (B<<16)|(G<<8)|R
            let r = (rgb >> 16) & 0xFF;
            let g = (rgb >> 8) & 0xFF;
            let b = rgb & 0xFF;
            let colorref = (b << 16) | (g << 8) | r;

            let target = fn_arg_value(app, &xargs[1]);
            let vname = match target.as_str() {
                "通常" => "StatusWindow(StringColor)",
                "能力名" => "StatusWindow(ANameColor)",
                "有効" => "StatusWindow(EnableColor)",
                "無効" => "StatusWindow(DisableColor)",
                _ => {
                    return Err(err(
                        line,
                        "SetStatusStringColor: 対象は「通常」「能力名」「有効」「無効」のみ。",
                    ))
                }
            };
            app.set_script_var(vname.to_string(), colorref.to_string());
            return Ok(pc + 1);
        }
        "fillstyle" => {
            // `FillStyle <style>` — 図形 (Circle/Oval/Polygon/Arc) の塗り方式。
            // `透明`=塗りなし、それ以外 (`塗りつぶし`/`横線`/`縦線`/`斜線`/`クロス`/
            // `網かけ` 等) は本実装では solid 塗りで近似 (C# は != Transparent で塗る)。
            let style = xargs.first().map(|s| s.trim()).unwrap_or("");
            let solid = !style.is_empty() && style != "透明";
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::SetFillSolid(solid));
            return Ok(pc + 1);
        }
        "background" | "drawoption" | "renametitle" | "renamebgm" | "freememory" | "exec"
        | "make" | "playmovie" | "redraw" | "setbackground" => {
            // 表示 / システム / 拡張系: 現段階では no-op (将来拡張)
            return Ok(pc + 1);
        }
        "upvar" => {
            // SRC `UpVar` コマンド: 現フレームから見た祖先フレームの引数への
            // アクセスを可能にする。呼び出しごとに 1 段上の祖先を追加で参照。
            //
            // 実装: `upvar_level` を 1 増やし、その段の祖先フレームの
            // 保存済み Args を現フレームの Args に「上書きマージ」する。
            //
            // saved_args layout: [Args(1)..Args(9), ArgNum, ...]  (先頭 10 要素を使用)
            //
            // SRC 仕様詳細:
            //   - 引数なしサブルーチン内: Args(i) = 親フレームの i 番目引数
            //   - 引数ありサブルーチン内: Args(1..c) は自分の引数を維持し、
            //     Args(c+1..) に親フレームの引数を追記。ArgNum = c + 親 ArgNum
            //   - 複数回呼んだ場合: 各回でさらに 1 段上の祖先を追記
            //   - チェーン UpVar (祖先も UpVar 済み): 祖先の saved_args がその
            //     時点で拡張済みのため、自動的に全祖先の引数が合成される
            let upvar_level = app.upvar_level() + 1;
            app.set_upvar_level(upvar_level);

            // 最初の UpVar 呼び出しでベースとなる ArgNum を記録
            if upvar_level == 1 {
                let base = app.script_var("ArgNum").parse::<usize>().unwrap_or(0);
                app.set_upvar_base_argnum(base);
            }

            let base = app.upvar_base_argnum();
            let depth = app.call_stack_depth();

            // 必要な祖先フレームが存在する場合のみマージ
            if depth >= upvar_level {
                let ancestor_idx = depth - upvar_level;
                if let Some(saved) = app.call_stack_saved_args(ancestor_idx) {
                    let saved = saved.clone(); // 借用解除のためコピー
                                               // layout: [upvar_level, upvar_base_argnum, ArgNum, Args(1)..]
                    let parent_argnum = saved
                        .get(2)
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0)
                        .min(MAX_CALL_ARGS);

                    // 祖先の Args(1..parent_argnum) を base の直後から設定。
                    // まず base+1 以降をクリア (前回の UpVar 分を上書き)。
                    let cur = current_argnum(app);
                    for k in (base + 1)..=cur.max(base + parent_argnum) {
                        app.set_script_var(format!("Args({k})"), String::new());
                    }
                    for i in 0..parent_argnum {
                        let val = saved.get(3 + i).cloned().unwrap_or_default();
                        app.set_script_var(format!("Args({})", base + i + 1), val);
                    }
                    // ArgNum を更新
                    app.set_script_var("ArgNum".to_string(), (base + parent_argnum).to_string());
                }
            }
            // 祖先が存在しない (トップレベルから呼ばれた) 場合は no-op
            return Ok(pc + 1);
        }
        "line" => {
            // `Line x1 y1 x2 y2 [color] [B|BF]`
            //   B  = box outline (4 lines)
            //   BF = box filled (FillRect)
            // 色はトークン中 `B`/`BF` 以外を吸収するため、色判定を先に行う。
            if xargs.len() >= 4 {
                // 座標は `(307 + i * 30)` のような変数入り算術式でありうるため
                // app-aware に評価する (生 parse だと式が 0 に潰れて線がずれる)。
                let x1 = eval_int_expr_app(app, &xargs[0]) as f64;
                let y1 = eval_int_expr_app(app, &xargs[1]) as f64;
                let x2 = eval_int_expr_app(app, &xargs[2]) as f64;
                let y2 = eval_int_expr_app(app, &xargs[3]) as f64;
                let mut color: Option<String> = None;
                let mut box_mode: Option<&'static str> = None;
                for tok in xargs.iter().skip(4) {
                    let t = tok.trim();
                    if t.eq_ignore_ascii_case("b") {
                        box_mode = Some("B");
                    } else if t.eq_ignore_ascii_case("bf") {
                        box_mode = Some("BF");
                    } else if !t.is_empty() {
                        // 色トークン。`FrameColor1` のような変数指定 (.ini 由来の
                        // テーマ色) も解決する。
                        color = Some(resolve_color(app, t));
                    }
                }
                if let Some(c) = color {
                    app.script_overlay_mut()
                        .push(crate::script_overlay::DrawCmd::SetColor { color: c });
                }
                let (lx, ly, lw, lh) = (x1.min(x2), y1.min(y2), (x2 - x1).abs(), (y2 - y1).abs());
                match box_mode {
                    Some("BF") => {
                        app.script_overlay_mut()
                            .push(crate::script_overlay::DrawCmd::FillRect {
                                x: lx,
                                y: ly,
                                w: lw,
                                h: lh,
                            });
                    }
                    Some("B") => {
                        // 4 辺
                        for (px1, py1, px2, py2) in [
                            (lx, ly, lx + lw, ly),
                            (lx + lw, ly, lx + lw, ly + lh),
                            (lx + lw, ly + lh, lx, ly + lh),
                            (lx, ly + lh, lx, ly),
                        ] {
                            app.script_overlay_mut()
                                .push(crate::script_overlay::DrawCmd::Line {
                                    x1: px1,
                                    y1: py1,
                                    x2: px2,
                                    y2: py2,
                                });
                        }
                    }
                    _ => {
                        app.script_overlay_mut()
                            .push(crate::script_overlay::DrawCmd::Line { x1, y1, x2, y2 });
                    }
                }
            }
            return Ok(pc + 1);
        }
        "pset" => {
            if xargs.len() >= 2 {
                // `PSet x, y` の SRC 構文は tokenizer 上で trailing `,` が
                // 付くため strip してから parse する。
                let x = eval_int_expr_app(app, &xargs[0]) as f64;
                let y = eval_int_expr_app(app, &xargs[1]) as f64;
                app.script_overlay_mut()
                    .push(crate::script_overlay::DrawCmd::PSet { x, y });
            }
            return Ok(pc + 1);
        }
        "circle" => {
            // `Circle x y r [color]` — 中心 (x,y)・半径 r の真円 (SRC `Circleコマンド`)。
            // 色省略時は現在の線色。座標は変数入り算術式でありうるため app-aware に評価。
            if xargs.len() >= 3 {
                let cx = eval_int_expr_app(app, &xargs[0]) as f64;
                let cy = eval_int_expr_app(app, &xargs[1]) as f64;
                let r = eval_int_expr_app(app, &xargs[2]) as f64;
                if let Some(c) = optional_draw_color(app, &xargs[3..]) {
                    app.script_overlay_mut()
                        .push(crate::script_overlay::DrawCmd::SetColor { color: c });
                }
                app.script_overlay_mut()
                    .push(crate::script_overlay::DrawCmd::Circle { cx, cy, r });
            }
            return Ok(pc + 1);
        }
        "oval" => {
            // `Oval x y r ratio [color]` — 中心 (x,y)・横半径 r・縦横比 ratio の楕円。
            if xargs.len() >= 4 {
                let cx = eval_int_expr_app(app, &xargs[0]) as f64;
                let cy = eval_int_expr_app(app, &xargs[1]) as f64;
                let r = eval_int_expr_app(app, &xargs[2]) as f64;
                let ratio = numeric_arg(app, &xargs[3]).unwrap_or(1.0);
                if let Some(c) = optional_draw_color(app, &xargs[4..]) {
                    app.script_overlay_mut()
                        .push(crate::script_overlay::DrawCmd::SetColor { color: c });
                }
                app.script_overlay_mut()
                    .push(crate::script_overlay::DrawCmd::Oval { cx, cy, r, ratio });
            }
            return Ok(pc + 1);
        }
        "arc" => {
            // `Arc x y r start end [color]` — 中心 (x,y)・半径 r の円弧。
            // 角度は度数法・右向き=0・反時計回り増加 (SRC `Arcコマンド`)。
            if xargs.len() >= 5 {
                let cx = eval_int_expr_app(app, &xargs[0]) as f64;
                let cy = eval_int_expr_app(app, &xargs[1]) as f64;
                let r = eval_int_expr_app(app, &xargs[2]) as f64;
                let start_deg = numeric_arg(app, &xargs[3]).unwrap_or(0.0) % 360.0;
                let end_deg = numeric_arg(app, &xargs[4]).unwrap_or(0.0) % 360.0;
                if let Some(c) = optional_draw_color(app, &xargs[5..]) {
                    app.script_overlay_mut()
                        .push(crate::script_overlay::DrawCmd::SetColor { color: c });
                }
                app.script_overlay_mut()
                    .push(crate::script_overlay::DrawCmd::Arc {
                        cx,
                        cy,
                        r,
                        start_deg,
                        end_deg,
                    });
            }
            return Ok(pc + 1);
        }
        "polygon" => {
            // `Polygon x1 y1 x2 y2 …` — 頂点列を結ぶ多角形 (SRC `Polygonコマンド`)。
            // 色オプションは無く、現在の線色のみ。頂点は座標ペアで任意個数。
            let mut points = Vec::new();
            for pair in xargs.chunks(2) {
                if pair.len() < 2 {
                    break; // 余った単独座標は無視 (C# `2*pnum < ArgNum` と同等)
                }
                let px = eval_int_expr_app(app, &pair[0]) as f64;
                let py = eval_int_expr_app(app, &pair[1]) as f64;
                points.push((px, py));
            }
            if points.len() >= 2 {
                app.script_overlay_mut()
                    .push(crate::script_overlay::DrawCmd::Polygon { points });
            }
            return Ok(pc + 1);
        }
        "fillcolor" => {
            // `FillColor <color>` — 図形の内部塗り色。線色 (Color) とは独立。
            // 旧実装は SetColor (線色) に潰していたが、塗り色は SetFillColor で別管理。
            let raw = xargs.join(" ");
            let raw = raw.trim();
            let color = if raw.is_empty() {
                "#000000".to_string()
            } else {
                resolve_color(app, raw)
            };
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::SetFillColor { color });
            return Ok(pc + 1);
        }
        "fadeout" => {
            // `FadeOut n` — 画面が黒へフェードアウト (終状態: 黒で覆う)。
            // n は段階値。終状態のみ描くので黒の全画面 Fade を 1 枚積む。
            let n = xargs
                .first()
                .and_then(|s| s.parse::<u32>().ok())
                .unwrap_or(0);
            let alpha = (n as f64 / 60.0).clamp(0.0, 1.0);
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::Fade {
                    color: "#000000".to_string(),
                    alpha,
                });
            return Ok(pc + 1);
        }
        "fadein" => {
            // `FadeIn n` — 黒から通常画面へフェードイン。終状態は通常画面なので
            // 黒の全画面 Fade を除去して露出する (WhiteIn と同様、残すと黒画面で
            // 操作不能になる)。
            app.script_overlay_mut()
                .remove_fades_of(is_black_fade_color);
            return Ok(pc + 1);
        }
        "input" => {
            // `Input var prompt [default]` — テキスト入力モーダルを出す。
            // SRC.Sharp 準拠: 引数は 2〜3 個 (var + prompt [+ default])。
            // 0, 1, 4+ 個はエラー。
            if xargs.len() < 2 || xargs.len() > 3 {
                return Err(err(line, "Inputコマンドの引数の数が違います"));
            }
            // 第 1 引数は代入先 (lvalue)。`xargs` は値展開済みなので、`名前[キー]` 形式の
            // 配列変数が**現在値に化けて**しまう (例: 2 回目の Input で前回値 "417776" が
            // キー名になり、入力が元の変数を更新しない)。`Set` と同じく生の `args[0]` を
            // `resolve_lhs_name` で格納キーへ解決する (添字 expr は評価し、変数値展開はしない)。
            let var = resolve_lhs_name(app, &args[0]);
            let prompt = xargs[1].clone();
            let default = xargs.get(2).cloned().unwrap_or_default();
            app.set_script_var(var.clone(), default.clone());
            app.set_pending_dialog(crate::dialog::PendingDialog::Input {
                prompt,
                var_name: var,
                default,
            });
            return Ok(pc + 1);
        }
        "confirm" => {
            // `Confirm message` — Yes/No 対話 UI を出して中断。
            // SRC.Sharp 準拠: 引数は必ず 1 個 (message のみ)。0 個・2+ 個はエラー。
            // 応答後 `respond_dialog(0|1)` で `選択` 変数に格納し再開。
            if xargs.len() != 1 {
                return Err(err(line, "Confirmコマンドの引数の数が違います"));
            }
            let question = xargs[0].clone();
            app.set_pending_dialog(crate::dialog::PendingDialog::Confirm {
                question,
                var_name: "選択".to_string(),
            });
            return Ok(pc + 1);
        }
        "menu" | "ask" => {
            // 元 SRC `Ask` には 2 形式ある:
            //   Format 1: `Ask message [option...]` + 選択肢を次行から `End` まで列挙
            //   Format 2: `Ask array message [option...]` — 配列 `array[*]` の値を選択肢に
            // 第 1 引数が `name[*]` インデックス変数の prefix と一致したら Format 2。
            // `Menu` は SRC では Format 1 のみだが、本実装では同じ枝で扱う。
            //
            // Format 2 で `キャンセル可` オプションが含まれる場合は store_value=false
            // のままで OK (キャンセル時は 0 が `選択` に入る)。
            let lname_lc = lname.as_str();
            // Format 2 判定: 第 1 引数 prefix が script_vars に存在するか
            let fmt2_array = if !xargs.is_empty() {
                let arr_name = &xargs[0];
                let prefix = format!("{arr_name}[");
                let has_keys = app
                    .script_vars()
                    .keys()
                    .any(|k| k.starts_with(&prefix) && k.ends_with(']'));
                if has_keys {
                    Some(arr_name.clone())
                } else {
                    None
                }
            } else {
                None
            };
            if let (Some(arr_name), "ask") = (fmt2_array, lname_lc) {
                // Format 2: array → options。message は xargs[1..]、オプション語
                // ("キャンセル可" 等) は本実装では prompt にも含める形で吸収。
                let prompt = if xargs.len() >= 2 {
                    xargs[1..].join(" ")
                } else {
                    String::new()
                };
                // ForEach 順序で配列要素を列挙。表示は **値**、添字 (key) は
                // 別途 option_keys に保持する。SRC `Ask` Format 2 は選んだ要素の
                // **添字** を `選択` に格納する仕様 (例: `乗せ換え表示[Unitid(i)]`
                // を選ぶと `選択 = Unitid(i)` となり `Pilot(選択)` が解決できる)。
                let mut options: Vec<String> = Vec::new();
                let mut option_keys: Vec<String> = Vec::new();
                for k in collect_foreach_items(app, &arr_name) {
                    let full = format!("{arr_name}[{k}]");
                    let v = app.script_var(&full);
                    if !v.is_empty() {
                        options.push(v.to_string());
                        option_keys.push(k);
                    }
                }
                if options.is_empty() {
                    return Ok(pc + 1);
                }
                app.set_pending_dialog(crate::dialog::PendingDialog::Menu {
                    prompt,
                    options,
                    var_name: "選択".to_string(),
                    store_value: true,
                    option_keys,
                    // Format 2 (配列マップ選択) は従来どおりキャンセル可。
                    non_cancellable: false,
                });
                return Ok(pc + 1);
            }
            // `Ask 終了` は特殊ターミネータ: 開いている ListBox を閉じて次に進む。
            // SRC.Sharp 準拠: 後続行を読まずに即リターン。
            if lname_lc == "ask" && xargs.len() == 1 && xargs[0].eq_ignore_ascii_case("終了") {
                return Ok(pc + 1);
            }
            // Format 1: prompt はすべての引数を空白連結、選択肢は後続行を End まで。
            // `Ask` で引数なし → デフォルトプロンプト「いずれかを選んでください」。
            // `キャンセル可` オプションがあればキャンセル可、無ければ選択必須
            // (キャンセルで 選択=0 → キャラ未選択で味方0体 → 即敗北 を防ぐ)。
            let cancelable = xargs.iter().any(|a| a == "キャンセル可");
            let prompt_parts: Vec<String> = xargs
                .iter()
                .filter(|a| a.as_str() != "キャンセル可")
                .cloned()
                .collect();
            let prompt = if prompt_parts.is_empty() {
                if lname_lc == "ask" {
                    "いずれかを選んでください".to_string()
                } else {
                    String::new()
                }
            } else {
                prompt_parts.join(" ")
            };
            let (options, next_pc) = collect_menu_options(app, pc + 1, stmts);
            if options.is_empty() {
                // SRC.Sharp 準拠: 選択肢なし → 選択 = "0" にして終了
                app.set_script_var("選択".to_string(), "0".to_string());
                return Ok(next_pc);
            }
            app.set_pending_dialog(crate::dialog::PendingDialog::Menu {
                prompt,
                options,
                var_name: "選択".to_string(),
                store_value: false,
                option_keys: Vec::new(),
                non_cancellable: !cancelable,
            });
            return Ok(next_pc);
        }
        "question" => {
            // SRC `Question <time> [message]` (`Questionコマンド.md`):
            //   - time: 0.1 秒単位の制限時間。本実装では時間切れを単純化し、
            //     `Menu` と同じ即時応答ダイアログにする (タイマ無視)。
            //   - message: 説明行 (省略可、デフォルト「さあ、どうする？」)。
            //   選択肢は次行から `End` まで。先頭選択肢が選ばれたら 選択=1、
            //   時間切れなら 0。本実装は時間切れを発生させないので必ず 1+ になる。
            // 注意: 時間切れ実装は本実装で省略。SRC.NET の `Question` は
            // `Wait Timer` 同様のタイマと連動するが、PendingDialog は応答必須。
            let prompt = if xargs.len() <= 1 {
                String::from("さあ、どうする？")
            } else {
                xargs[1..].join(" ")
            };
            let (options, next_pc) = collect_menu_options(app, pc + 1, stmts);
            if options.is_empty() {
                // SRC.Sharp 準拠: 選択肢なし → 選択 = "0" にして終了
                app.set_script_var("選択".to_string(), "0".to_string());
                return Ok(next_pc);
            }
            app.set_pending_dialog(crate::dialog::PendingDialog::Menu {
                prompt,
                options,
                var_name: "選択".to_string(),
                store_value: false,
                option_keys: Vec::new(),
                // Question は時間切れ=選択0 を持つため従来どおりキャンセル可。
                non_cancellable: false,
            });
            return Ok(next_pc);
        }
        "select" => {
            // SRC `Select <unit>` (`Selectコマンド.md`):
            // 引数 1: unit 識別子 (pilot 名 / unit_data 名 / uid)。
            // `SelectedUnitForEvent` に指定ユニットをセットする。
            // C# SelectCmd.cs: `Event.SelectedUnitForEvent = GetArgAsUnit(2);`
            if xargs.len() != 1 {
                return Err(err(line, "Selectコマンドの引数の数が違います"));
            }
            let key = fn_arg_value(app, &xargs[0]);
            app.set_selected_unit_for_event(key);
            return Ok(pc + 1);
        }
        "selecttarget" => {
            // SRC `SelectTarget unit` (`SelectTargetコマンド.md`):
            // デフォルトターゲットを設定。`相手パイロット` / `相手ユニットＩＤ`
            // システム変数を更新し、汎用戦闘アニメ等で参照可能にする。
            // 引数 1: unit 識別子 (pilot 名 / unit_data 名 / uid)。
            // C# SelectTargetCmd.cs: `ArgNum != 2` → EventErrorException。
            if xargs.len() != 1 {
                return Err(err(line, "SelectTargetコマンドの引数の数が違います"));
            }
            if let Some(target) = xargs.first() {
                let key = fn_arg_value(app, target);
                let resolved = app
                    .database()
                    .unit_instances
                    .iter()
                    .find(|u| matches_unit_handle(u, &key))
                    .map(|u| (u.pilot_name.clone(), u.uid.clone()));
                if let Some((pilot, uid)) = resolved {
                    app.set_script_var("相手パイロット".to_string(), pilot);
                    app.set_script_var("相手ユニットＩＤ".to_string(), uid);
                }
            }
            return Ok(pc + 1);
        }
        "movunit" | "moveunit" => {
            // MoveUnit unit_name x y
            if xargs.len() < 3 {
                return Err(err(line, "MoveUnit 命令は 3 引数必要 (name x y)。"));
            }
            let target = &xargs[0];
            let nx = parse_u32(&xargs, 1, line)?;
            let ny = parse_u32(&xargs, 2, line)?;
            let uid = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, target))
                .map(|u| u.uid.clone());
            if let Some(uid) = uid {
                app.database_mut().move_unit(&uid, nx, ny);
            }
            return Ok(pc + 1);
        }
        "move" => {
            // SRC `Move [unit] x y [option]` (`Moveコマンド.md`):
            //  - 2 引数: `Move x y` → 直前選択ユニットを移動
            //  - 3 引数: `Move unit x y`
            //  - 4 引数: `Move unit x y option` (`非同期` / `アニメ表示`) — オプションは
            //    本実装で無視 (常に即時移動)
            // 移動力制限・EN 消費は無視 (原典準拠)、母艦格納時は自動発進 (off_map=false)。
            // SRC `Move` 命令経由のスクリプト移動は `進入イベント` を **発火させない**
            // (原典: "Moveコマンド等でユニットを移動させた場合、進入イベントは発生しません")。
            let (target_key, nx, ny) = match xargs.len() {
                0 | 1 => return Err(err(line, "Move 命令は最低 2 引数必要 (x y)。")),
                2 => {
                    let nx = parse_u32(&xargs, 0, line)?;
                    let ny = parse_u32(&xargs, 1, line)?;
                    (app.selected_unit_for_event().to_string(), nx, ny)
                }
                _ => {
                    let target = xargs[0].clone();
                    let nx = parse_u32(&xargs, 1, line)?;
                    let ny = parse_u32(&xargs, 2, line)?;
                    (target, nx, ny)
                }
            };
            let uid = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &target_key))
                .map(|u| u.uid.clone());
            let found = uid.is_some();
            if let Some(uid) = uid {
                app.database_mut().move_unit(&uid, nx, ny);
                app.database_mut().set_off_map(&uid, false);
            }
            // SRC.Sharp 準拠: 対象ユニットが存在しない場合はエラー (3 引数形式のみ)。
            if !found && xargs.len() >= 3 {
                return Err(err(
                    line,
                    &format!("Move: 対象「{target_key}」が見つかりません"),
                ));
            }
            return Ok(pc + 1);
        }
        "damage" => {
            // Damage unit_name amount
            if xargs.len() < 2 {
                return Err(err(line, "Damage 命令は 2 引数必要 (name amount)。"));
            }
            let target = &xargs[0];
            let amount = parse_i64_at(&xargs[1], line)?;
            apply_damage(app, target, amount);
            return Ok(pc + 1);
        }
        "heal" => {
            // Heal unit_name amount  (負ダメージで HP 回復)
            if xargs.len() < 2 {
                return Err(err(line, "Heal 命令は 2 引数必要 (name amount)。"));
            }
            let target = &xargs[0];
            let amount = parse_i64_at(&xargs[1], line)?;
            apply_damage(app, target, -amount);
            return Ok(pc + 1);
        }
        "kill" | "destroy" => {
            if xargs.is_empty() {
                return Err(err(line, "Kill 命令は対象名が必要。"));
            }
            // SRC.Sharp 準拠: 引数は正確に 1 個。
            if xargs.len() > 1 {
                return Err(err(line, "Kill/Destroyコマンドの引数の数が違います"));
            }
            let target = &xargs[0];
            // `Kill name` は名前が一致するもの全てを除去するので、複数該当
            // した場合は順に撃破ラベルを発火する。
            let destroyed = app
                .database_mut()
                .remove_units_where(|u| matches_unit_handle(u, target));
            for u in destroyed {
                fire_destruction_labels(app, &u.pilot_name, &u.unit_data_name, u.party);
            }
            return Ok(pc + 1);
        }
        "create" => {
            // SRC `Create party unit rank pilot level x y [ID option]`
            // (本実装では rank / level / ID / option は無視。)
            // unit / pilot は裸識別子で渡されることが多いので fn_arg_value
            // で script_var 解決する (`Create 味方 入手ユニット 0 パイロット不在 ...`
            // → 入手ユニット の値で実際のユニット名が決まる)。
            if xargs.len() < 7 {
                return Err(err(
                    line,
                    "Create 命令は 7 引数必要 (party unit rank pilot level x y)。",
                ));
            }
            let party = parse_party(app, &xargs[0], line)?;
            let unit_data_name = fn_arg_value(app, &xargs[1]);
            // rank = xargs[2] = 改造段階。SRC `UList.Add(uname, urank, uparty)` は
            // `Unit.Rank=urank` で 1 段ごとに HP+200/装甲+100/EN+10/運動性+5 を加える
            // (差分オラクル placeunit で C# 実測: rank2 → MaxHP+400・装甲+200)。
            // 本実装の改造段階は `UnitInstance.upgrade_level`・`effective_*` がこの加算を
            // 持つので、rank をそこへ配線する (旧実装は rank を捨てて常に素のステータスだった)。
            let rank: i32 = fn_arg_value(app, &xargs[2]).parse().unwrap_or(0);
            let pilot = fn_arg_value(app, &xargs[3]);
            // level = xargs[4] = 主パイロットの初期レベル。SRC `PList.Add(pname, plevel, …)` は
            // `Pilot.Level=plevel` でレベルを設定する。本実装はレベルを累積経験値から導くため
            // (`level_from_exp`)、level を `exp_for_level` で初期 total_exp へ変換する
            // (旧実装は level を捨てて常にレベル 1 だった = 増援が指定レベルにならなかった)。
            // 注: レベルに応じた能力値成長 (`grown_pilot`) は class ベースの簡易式で、SRC の
            // 成長タイプ準拠成長とは乖離する (穴埋めロードマップ・docs/SRC_SHARP_DIVERGENCE.md)。
            let level: i32 = fn_arg_value(app, &xargs[4]).parse().unwrap_or(1);
            // 座標は app-aware 式評価で解決する。SRC は座標を式として評価するため、
            // `For i = ... / Create 中立 壁 0 P 1 4 i / Next` のような**裸のループ変数**や
            // 算術式が座標に来る (`eval_coord_u32`)。
            let x = eval_coord_u32(app, &xargs, 5);
            let y = eval_coord_u32(app, &xargs, 6);
            let mut inst = UnitInstance::new(unit_data_name, pilot.clone(), party, x, y);
            inst.upgrade_level = rank.max(0);
            inst.total_exp = crate::pilot_instance::exp_for_level(level);
            populate_active_features(&mut inst, app);
            let uid = app.database_mut().register_unit(inst);
            // 対象ユニットＩＤ は最新作成ユニットの uid (一意)、
            // 対象パイロット は pilot 名 (重複可) を入れる。
            // これで後続の `Escape 対象ユニットＩＤ` が正しいユニットを参照する。
            app.set_script_var("対象ユニットＩＤ".to_string(), uid);
            app.set_script_var("対象パイロット".to_string(), pilot);
            return Ok(pc + 1);
        }
        "removeunit" => {
            if xargs.is_empty() {
                return Err(err(line, "RemoveUnit には対象名が必要。"));
            }
            let target = &xargs[0];
            app.database_mut()
                .remove_units_where(|u| matches_unit_handle(u, target));
            return Ok(pc + 1);
        }
        "removepilot" => {
            // パイロット定義のみ削除。SRC では unit_instance も連動削除する
            // 仕様だが、機体選択フローの `Escape → Getoff → RemovePilot` の
            // 順では既に Getoff でパイロット名がクリアされている前提で
            // 後段の Removepilot は安全。本実装では Getoff が pilot_name を
            // 空にするので、ここで pilot_name 一致削除しても影響なしのはず。
            if xargs.is_empty() {
                return Err(err(line, "RemovePilot には対象名が必要。"));
            }
            let target = fn_arg_value(app, &xargs[0]);
            let db = app.database_mut();
            db.pilots
                .retain(|p| p.name != target && p.nickname != target);
            return Ok(pc + 1);
        }
        "recoverhp" => {
            // `RecoverHP [unit] rate` (SRC.Sharp `RecoverHPCmd.cs` 準拠)
            // unit は省略可能。省略時は `selected_unit_for_event` を使用。
            // rate は HP の回復率 (%)。負の値で HP を減少させることもできる。
            // RecoverHP によって HP が 0 以下になることはない (最低 1)。
            // C# RecoverHPCmd.cs: ArgNum==2 (1 user arg) or ArgNum==3 (2 user args)、それ以外エラー。
            match xargs.len() {
                0 | 3.. => return Err(err(line, "RecoverHP の引数の数が違います。")),
                1 => {
                    // RecoverHP rate — 選択ユニットを使用
                    let key = app.selected_unit_for_event().to_string();
                    if !key.is_empty() {
                        recover_hp(app, &key, Some(&xargs[0]));
                    }
                }
                2 => {
                    // RecoverHP unit rate
                    let key = xargs[0].clone();
                    recover_hp(app, &key, Some(&xargs[1]));
                }
            }
            return Ok(pc + 1);
        }
        "recoveren" => {
            // `RecoverEN [unit] rate` — HP と同様にユニット省略可能。
            // C# RecoverENCmd.cs: ArgNum==2 or ArgNum==3、それ以外エラー。
            match xargs.len() {
                0 | 3.. => return Err(err(line, "RecoverEN の引数の数が違います。")),
                1 => {
                    let key = app.selected_unit_for_event().to_string();
                    if !key.is_empty() {
                        recover_en(app, &key, Some(&xargs[0]));
                    }
                }
                2 => {
                    let key = xargs[0].clone();
                    recover_en(app, &key, Some(&xargs[1]));
                }
            }
            return Ok(pc + 1);
        }
        "recoversp" | "recoverplana" => {
            // SRC `RecoverSP [pilot] rate` (`RecoverSPコマンド.md`):
            // パイロット名 (省略時はデフォルトユニットのメインパイロット) の SP を
            // rate% 回復する。rate は浮動小数点も可。
            // 実装は UnitInstance.sp_consumed で SP を追跡。
            // パイロット名からユニットを逆引き (pilot_name 一致)。
            // C# RecoverSPCmd.cs: ArgNum==2 (rate only) or ArgNum==3 (pilot + rate)。
            // 1引数 = rate (デフォルトユニット)、2引数 = pilot + rate。
            let (pilot_key, rate_str): (String, &str) = match xargs.len() {
                0 => {
                    return Err(err(line, "RecoverSPコマンドの引数の数が違います"));
                }
                1 => {
                    // rate のみ → デフォルトユニットのメインパイロット
                    let key = app.selected_unit_for_event().to_string();
                    (key, xargs[0].as_str())
                }
                _ => (xargs[0].clone(), xargs[1].as_str()),
            };
            // パイロット名 or ユニット識別子で UnitInstance を探す
            let idx = app.database().unit_instances.iter().position(|u| {
                matches_unit_handle(u, &pilot_key)
                    || u.pilot_name == pilot_key
                    || app.database().pilots.iter().any(|p| {
                        (p.name == pilot_key || p.nickname == pilot_key) && u.pilot_name == p.name
                    })
            });
            if let Some(idx) = idx {
                let max_sp = {
                    let inst = &app.database().unit_instances[idx];
                    app.database()
                        .pilot_by_name(&inst.pilot_name)
                        .and_then(|p| p.sp)
                        .unwrap_or(0)
                };
                let cur = app.database().unit_instances[idx].sp_consumed;
                let new_consumed = if rate_str.eq_ignore_ascii_case("full") || rate_str == "全" {
                    0
                } else {
                    let pct: f64 = rate_str.trim_end_matches('%').parse().unwrap_or(0.0);
                    let recover = ((max_sp as f64) * pct / 100.0) as i32;
                    (cur - recover).max(0)
                };
                app.database_mut().unit_instances[idx].sp_consumed = new_consumed;
            }
            return Ok(pc + 1);
        }
        "increasemorale" => {
            // IncreaseMorale unit [delta]
            if xargs.is_empty() {
                return Err(err(line, "IncreaseMorale には対象名が必要。"));
            }
            let delta = if xargs.len() >= 2 {
                parse_i32_at(&xargs[1], line)?
            } else {
                5
            };
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &xargs[0]))
            {
                // C# IncreaseMoraleCmd.cs: モラルは [50, 150] にクランプ。
                u.morale = (u.morale + delta).clamp(50, 150);
            }
            return Ok(pc + 1);
        }
        "expup" => {
            // ExpUp unit n
            if xargs.len() < 2 {
                return Err(err(line, "ExpUp 命令は 2 引数必要 (name n)。"));
            }
            let n = parse_i32_at(&xargs[1], line)?;
            let target_idx = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, &xargs[0]));
            // `Level()` 関数の規約に合わせ、`level_from_exp` (SRC: 500 exp/level) で算出。
            // レベル繰り上がりは event 発火条件。
            let (old_level, new_level, pilot_ids): (i32, i32, Vec<String>) =
                if let Some(i) = target_idx {
                    let u = &mut app.database_mut().unit_instances[i];
                    let old = crate::pilot_instance::level_from_exp(u.total_exp);
                    u.total_exp += n;
                    let new_ = crate::pilot_instance::level_from_exp(u.total_exp);
                    (old, new_, u.pilot_ids.clone())
                } else {
                    (0, 0, Vec::new())
                };
            for pilot_id in pilot_ids {
                let pilot_data_name = app
                    .database()
                    .pilot_instance_by_id(&pilot_id)
                    .map(|p| p.pilot_data_name.clone());
                let pilot_data = pilot_data_name
                    .as_ref()
                    .and_then(|name| app.database().pilot_by_name(name).cloned());
                if let Some(pilot_data) = pilot_data {
                    if let Some(pilot_inst) = app.database_mut().pilot_instance_by_id_mut(&pilot_id)
                    {
                        if pilot_inst.add_exp(n) {
                            pilot_inst.apply_stat_growth(&pilot_data);
                        }
                    }
                }
            }
            // SRC `LevelUp <unit>:` (`レベルアップイベント.md`) — メインパイロット
            // のレベルアップ時に発火。本実装は unit 単位 (UnitInstance.total_exp /
            // 100 + 1) で level を扱うため、その値が増加した場合に発火する。
            // pilot/unit/party いずれの綴でも 1 度発火 (`fire_unit_event_labels`)。
            if new_level > old_level {
                if let Some(i) = target_idx {
                    let u = &app.database().unit_instances[i];
                    let pilot_name = u.pilot_name.clone();
                    let unit_data_name = u.unit_data_name.clone();
                    let party = u.party;
                    fire_unit_event_labels(
                        app,
                        &["レベルアップ", "LevelUp"],
                        &pilot_name,
                        &unit_data_name,
                        party,
                        None,
                    );
                }
            }
            return Ok(pc + 1);
        }
        "item" => {
            // SRC `Itemコマンド.md`:
            //   書式1: `Item item_name` (1 引数) — アイテムを作成して
            //     未装備在庫 (`spare_items`) に追加。装備は後続の `Equip` コマンドで。
            //     実シナリオ (`shop_01` / `DUInclude` 等) で頻繁に使われる形式。
            //   書式2: `Item unit item_name` (2 引数, Equip と同義の互換形式) —
            //     ユニットに直接装備させる。旧シナリオ互換として残す。
            if xargs.is_empty() {
                return Ok(pc + 1);
            }
            if xargs.len() == 1 {
                let item_name = xargs[0].clone();
                app.database_mut().spare_items.push(item_name);
                return Ok(pc + 1);
            }
            // 2 引数以上 → Equip と同義で処理。
            let target = xargs[0].clone();
            let item_name = xargs[1].clone();
            let slot_type = app
                .database()
                .item_by_name(&item_name)
                .and_then(|it| SlotType::parse(&it.part))
                .unwrap_or(SlotType::Item);
            let mut equipped = false;
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target))
            {
                if !u.has_item_equipped(&item_name) {
                    u.equip_item(slot_type, item_name.clone());
                    equipped = true;
                }
            }
            if equipped {
                if let Some(idx) = app
                    .database()
                    .spare_items
                    .iter()
                    .position(|s| s == &item_name)
                {
                    app.database_mut().spare_items.remove(idx);
                }
            }
            return Ok(pc + 1);
        }
        "equip" => {
            // `Equip [unit] item` — ユニットにアイテムを装備させる。
            //   1 引数: `Equip item_name` — 選択中ユニット (selected_unit_for_event) に装備。
            //   2 引数: `Equip unit item_name` — 指定ユニットに装備。
            // 在庫にあれば消費し、なければ新規装備として扱う。
            if xargs.is_empty() {
                return Ok(pc + 1);
            }
            let (target, item_name) = if xargs.len() == 1 {
                (app.selected_unit_for_event().to_string(), xargs[0].clone())
            } else {
                (xargs[0].clone(), xargs[1].clone())
            };
            let slot_type = app
                .database()
                .item_by_name(&item_name)
                .and_then(|it| SlotType::parse(&it.part))
                .unwrap_or(SlotType::Item);
            let mut equipped = false;
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target))
            {
                if !u.has_item_equipped(&item_name) {
                    u.equip_item(slot_type, item_name.clone());
                    equipped = true;
                }
            }
            // 装備したアイテムが未装備在庫にあれば 1 つ取り出す
            // (在庫 → ユニットへ移動)。
            if equipped {
                if let Some(idx) = app
                    .database()
                    .spare_items
                    .iter()
                    .position(|s| s == &item_name)
                {
                    app.database_mut().spare_items.remove(idx);
                }
            }
            return Ok(pc + 1);
        }
        "removeitem" | "unequip" => {
            // SRC `RemoveItem [unit] [item]` (`RemoveItemコマンド.md`):
            // - item 指定あり: アイテムを **削除** (ユニットから外して破棄)。
            // - item 省略 (unit のみ / 両省略): ユニットの全アイテムを
            //   **取り外し** → 未装備在庫 (`spare_items`) へ移す。
            if xargs.len() >= 2 {
                // (4) unit + item: 指定アイテムを削除 (在庫には残さない)。
                let target = xargs[0].clone();
                let item = xargs[1].clone();
                if let Some(u) = app
                    .database_mut()
                    .unit_instances
                    .iter_mut()
                    .find(|u| matches_unit_handle(u, &target))
                {
                    u.unequip_item(&item);
                }
                return Ok(pc + 1);
            }
            // (1)(2) item 省略: 対象ユニットの全アイテムを取り外して在庫へ。
            let target = if let Some(t) = xargs.first() {
                fn_arg_value(app, t)
            } else {
                app.selected_unit_for_event().to_string()
            };
            let removed: Vec<String> = if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target))
            {
                let names: Vec<String> = u
                    .equipped_item_names()
                    .into_iter()
                    .map(String::from)
                    .collect();
                for name in &names {
                    u.unequip_item(name);
                }
                names
            } else {
                Vec::new()
            };
            app.database_mut().spare_items.extend(removed);
            return Ok(pc + 1);
        }
        "exchangeitem" => {
            if xargs.len() < 3 {
                return Err(err(line, "ExchangeItem 命令は 3 引数必要 (unit old new)。"));
            }
            let target = xargs[0].clone();
            let old = xargs[1].clone();
            let new_item = xargs[2].clone();
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &target))
            {
                for slot in &mut u.item_slots {
                    if slot.equipped_item.as_deref() == Some(&old) {
                        if !slot.is_fixed {
                            slot.equipped_item = Some(new_item.clone());
                        }
                        break;
                    }
                }
            }
            return Ok(pc + 1);
        }
        "levelup" => {
            // LevelUp unit [n]   ─ n レベル上げる (1 レベル = EXP_PER_LEVEL(500) exp)。
            // SRC.Sharp 準拠: 対象が存在しない場合はエラー。
            if xargs.is_empty() {
                return Err(err(line, "LevelUp には対象名が必要。"));
            }
            let n = if xargs.len() >= 2 {
                parse_i32_at(&xargs[1], line)?
            } else {
                1
            };
            let key = xargs[0].clone();
            let found = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .any(|u| matches_unit_handle(u, &key));
            if !found {
                return Err(err(
                    line,
                    &format!("LevelUp: 対象「{key}」が見つかりません"),
                ));
            }
            let pilot_ids: Vec<String> = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| matches_unit_handle(u, &key))
                .map(|u| {
                    u.total_exp += n * crate::pilot_instance::EXP_PER_LEVEL;
                    u.pilot_ids.clone()
                })
                .unwrap_or_default();
            for pilot_id in pilot_ids {
                let pilot_data_name = app
                    .database()
                    .pilot_instance_by_id(&pilot_id)
                    .map(|p| p.pilot_data_name.clone());
                let pilot_data = pilot_data_name
                    .as_ref()
                    .and_then(|name| app.database().pilot_by_name(name).cloned());
                if let Some(pilot_data) = pilot_data {
                    if let Some(pilot_inst) = app.database_mut().pilot_instance_by_id_mut(&pilot_id)
                    {
                        if pilot_inst.add_exp(n * crate::pilot_instance::EXP_PER_LEVEL) {
                            pilot_inst.apply_stat_growth(&pilot_data);
                        }
                    }
                }
            }
            return Ok(pc + 1);
        }
        _ => {}
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
            let x = eval_coord_u32(app, &xargs, 0);
            let y = eval_coord_u32(app, &xargs, 1);
            let tid = eval_coord_u32(app, &xargs, 2);
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
                app.database_mut().pilots.push(pilot);
                return Ok(pc + 1);
            }
            // インスタンス書式: Pilot name level [ID]
            if xargs.is_empty() {
                return Err(err(line, "Pilot 命令は name level が必要。"));
            }
            let name = fn_arg_value(app, &xargs[0]);
            let level: i32 = xargs
                .get(1)
                .and_then(|s| fn_arg_value(app, s).parse().ok())
                .unwrap_or(1);
            // データ名 / 通称から PilotData を解決して runtime インスタンスを作成。
            let data_name = app
                .database()
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
                    let unit_data_name = fn_arg_value(app, name_arg);
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
            app.database_mut().units.push(unit);
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
                .units
                .iter()
                .position(|u| u.name == *unit_name)
                .ok_or_else(|| err(line, "Weapon: 対象 Unit が未定義。"))?;
            app.database_mut().units[pos].weapons.push(wd);
        }
        "Place" => {
            // Place UnitName PilotName Party X Y
            if xargs.len() < 5 {
                return Err(err(line, "Place 命令は 5 引数必要。"));
            }
            let unit_data_name = xargs[0].clone();
            let party = parse_party(app, &xargs[2], line)?;
            // 座標は式評価 (ループ変数/算術式対応)。
            let x = eval_coord_u32(app, &xargs, 3);
            let y = eval_coord_u32(app, &xargs, 4);
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

/// コマンド引数のユニット指定を解決する。`handle` が実ユニット (uid/unit名/pilot名) に
/// 一致すればそのまま、一致しなければ同名 script_var の値へフォールバックする。SRC は
/// コマンド引数のユニット位置にシステム変数 (`相手ユニットＩＤ` 等) を許すため、裸の
/// 変数名 (生贄ルーチンの `SpecialPower 相手ユニットＩＤ みがわり 対象ユニットＩＤ`) を
/// uid へ解決できる。実ユニット名を優先するので通常の literal 指定は壊れない。
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
/// `expand_vars` で `#rrggbb` に評価済み・`resolve_color` が日本語色名/変数色も解決する。
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
fn parse_paintstring_args(app: &App, xargs: &[String]) -> (f64, f64, String) {
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
        // 数値リテラル / 算術式 / 整数 のいずれかをパースする。
        if let Ok(n) = t.parse::<f64>() {
            return Some(n);
        }
        // `(SCX(...) - 4 + Random(8))` のようにユーザ定義関数や変数を含む
        // 座標式は app-aware に評価する (未解決アトムは 0)。
        Some(eval_int_expr_app(app, t) as f64)
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

/// 座標/サイズ引数を **app-aware 式評価** で `u32` に解決する。SRC は座標を式として
/// 評価するため、裸のループ変数 (`i`) や算術式 (`(j * 2)`) も `script_var` 経由で
/// 解決する (`parse_u32` は裸変数を解決できない)。負値は 0 にクランプ。引数が無い /
/// 評価不能なら 0。`Create` / `Place` / `SetTile` 等の座標で使う。
fn eval_coord_u32(app: &App, args: &[String], i: usize) -> u32 {
    args.get(i)
        .map(|s| eval_int_expr_app(app, s).max(0) as u32)
        .unwrap_or(0)
}

fn parse_u32(args: &[String], i: usize, line: usize) -> Result<u32, ScriptError> {
    let s = expect_arg(args, i, line, "<u32>")?;
    if let Ok(v) = s.trim().parse::<u32>() {
        return Ok(v);
    }
    // `(3 + 4 - 4)` のような算術式座標も受理する。SRC は数値引数を式評価する
    // ため、`Create 敵 … (LIndex(…) + Random(5) - Random(5))` 等の座標式が
    // 展開後に括弧付き算術として残る。負値は 0 にクランプ。式として評価
    // できない真の不正値 (`abc` 等) は従来どおりエラーにする。
    match try_eval_int(s) {
        Some(v) => Ok(v.max(0) as u32),
        None => Err(err(line, &format!("u32 として解釈できません: {s:?}"))),
    }
}

fn parse_i32_at(s: &str, line: usize) -> Result<i32, ScriptError> {
    if let Ok(v) = s.trim().parse::<i32>() {
        return Ok(v);
    }
    // `parse_u32` と同様、`(味方レベル平均値 - 1)` のような算術式引数も評価する。
    // SRC は数値引数を式評価するため、展開後に括弧付き算術として残る座標/数値が
    // ある。i32 範囲外はクランプ。式として評価できない真の不正値はエラー。
    match try_eval_int(s) {
        Some(v) => Ok(v.clamp(i32::MIN as i64, i32::MAX as i64) as i32),
        None => Err(err(line, &format!("i32 として解釈できません: {s:?}"))),
    }
}

fn parse_i64_at(s: &str, line: usize) -> Result<i64, ScriptError> {
    if let Ok(v) = s.trim().parse::<i64>() {
        return Ok(v);
    }
    // `parse_i32_at` と同様、算術式引数を式評価でフォールバック解釈する。
    match try_eval_int(s) {
        Some(v) => Ok(v),
        None => Err(err(line, &format!("i64 として解釈できません: {s:?}"))),
    }
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
/// 未定義なら `fn_arg_value` が名前自身を返すので、`Create 味方 …` の
/// ような定数指定は影響を受けない。
fn parse_party(app: &App, s: &str, line: usize) -> Result<Party, ScriptError> {
    let resolved = fn_arg_value(app, s);
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
pub(crate) use unit_effects::*;
pub use vars::*;

#[cfg(test)]
mod tests;
