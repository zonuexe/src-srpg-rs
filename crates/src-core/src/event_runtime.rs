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

/// 全ロード済み `.eve` を集約したスクリプトライブラリ。
/// `App.script_library` として保持し、`Start:` / `Turn N:` 等の
/// 自動発火ラベルを引いて `trigger_label` で実行する。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScriptLibrary {
    pub statements: Vec<EventStatement>,
    pub labels: HashMap<String, usize>,
    /// 各 .eve ファイルの登録範囲 (PC 区間)。`Continue eve\onsen.eve` の
    /// ようなファイル名指定の next-stage 起動で、basename → start_pc を
    /// 引くのに使う。
    #[serde(default)]
    pub files: Vec<FileEntry>,
    /// `*ユニットコマンド` / `*マップコマンド` ラベルで定義されたシナリオ独自の
    /// コマンドメニュー項目。
    #[serde(default)]
    pub custom_commands: Vec<CustomCommandDef>,
}

/// `*ユニットコマンド` / `*マップコマンド` ラベルで定義される、シナリオ独自の
/// カスタムユニット / マップコマンドのメニュー項目定義。
///
/// 書式（SRC ユニットコマンドイベント参照）:
///
/// | 行頭プレフィックス             | 使用可能タイミング                     |
/// |-------------------------------|--------------------------------------|
/// | `ユニットコマンド …:`          | 移動前のみ（デフォルト）               |
/// | `*ユニットコマンド …:`         | 移動後も使用可能                       |
/// | `*-ユニットコマンド …:`        | 同上                                   |
/// | `-*ユニットコマンド …:`        | 行動終了後も使用可能                   |
/// | `**ユニットコマンド …:`        | 移動後・行動終了後どちらでも使用可能   |
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CustomCommandDef {
    /// `true` ならユニットコマンド、`false` ならマップコマンド。
    pub is_unit: bool,
    /// メニュー表示名 (`乗せ換え` 等)。本体ラベルへのジャンプにも使う。
    pub name: String,
    /// 対象勢力 / 条件 (`味方` / `敵` / ユニット名 等。空なら無条件)。
    pub target: String,
    /// 表示可否を判定する条件式 (`Call(乗せ換え確認)` / `(Morale() > 110)` 等)。
    /// 無条件なら `None`。評価は `evaluate_command_condition` が行う。
    pub condition: Option<String>,
    /// コマンド本体ラベル行の PC。
    pub body_pc: usize,
    /// `*` / `*-` / `**` プレフィックス — 移動後も表示する。
    #[serde(default)]
    pub post_move_ok: bool,
    /// `-*` / `**` プレフィックス — 行動終了後も表示する。
    #[serde(default)]
    pub post_act_ok: bool,
}

/// `[*|-]ユニットコマンド` / `[*|-]マップコマンド` 行を解析して `CustomCommandDef` を
/// 返す。該当しない行は `None`。
fn parse_custom_command(name: &str, args: &[String], pc: usize) -> Option<CustomCommandDef> {
    // 行頭プレフィックスでタイミングフラグを決定する。
    // SRC ユニットコマンドイベント仕様:
    //   `ユニットコマンド`    → post_move_ok=false, post_act_ok=false（デフォルト）
    //   `*ユニットコマンド`   → post_move_ok=true
    //   `*-ユニットコマンド`  → post_move_ok=true
    //   `-*ユニットコマンド`  → post_act_ok=true
    //   `**ユニットコマンド`  → post_move_ok=true, post_act_ok=true
    let (keyword, post_move_ok, post_act_ok) = if let Some(rest) = name.strip_prefix("**") {
        (rest, true, true)
    } else if let Some(rest) = name.strip_prefix("*-") {
        (rest, true, false)
    } else if let Some(rest) = name.strip_prefix("-*") {
        (rest, false, true)
    } else if let Some(rest) = name.strip_prefix('*') {
        (rest, true, false)
    } else {
        (name, false, false)
    };
    let is_unit = match keyword {
        "ユニットコマンド" => true,
        "マップコマンド" => false,
        _ => return None,
    };
    // ラベル行 (末尾 `:`) であること。
    if !args.last()?.ends_with(':') {
        return None;
    }
    // 末尾 `:` を剥がした引数列。
    let mut parts: Vec<String> = args.to_vec();
    if let Some(l) = parts.last_mut() {
        while l.ends_with(':') {
            l.pop();
        }
    }
    parts.retain(|p| !p.is_empty());
    let display = parts.first()?.clone();
    let target = parts.get(1).cloned().unwrap_or_default();
    // 条件式をそのまま保存する (`Call(X)` もラッパーごと保持)。
    // 評価は `evaluate_command_condition` が `Call()` パターンを検出して
    // `call_label_sync_for_condition` で同期実行する。
    let condition = parts.get(2).map(|c| c.trim().to_string());
    Some(CustomCommandDef {
        is_unit,
        name: display,
        target,
        condition,
        body_pc: pc,
        post_move_ok,
        post_act_ok,
    })
}

/// `ScriptLibrary` に登録された 1 ファイル分のメタデータ。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct FileEntry {
    /// `basename(path)` を小文字化したキー (`onsen.eve` 等)。
    pub basename: String,
    /// この file の statements が library に置かれた先頭 PC。
    pub start_pc: usize,
    /// 末尾 PC (exclusive)。
    pub end_pc: usize,
}

impl ScriptLibrary {
    /// `execute` 時に新規 statements を取り込む。後段に追記し、ラベルも統合。
    pub fn append(&mut self, statements: &[EventStatement]) {
        let base = self.statements.len();
        self.statements.extend_from_slice(statements);
        for (i, s) in statements.iter().enumerate() {
            if let EventStatement::Command { name, args, .. } = s {
                // `*ユニットコマンド` / `*マップコマンド` はシナリオ独自の
                // メニュー項目。通常ラベルとしては登録せず別管理する
                // (フラットな `labels` に `ユニットコマンド` で誤登録される
                // のを防ぐ)。
                if let Some(def) = parse_custom_command(name, args, base + i) {
                    self.custom_commands.push(def);
                    continue;
                }
                if let Some(canon) = canonical_label_full(name, args) {
                    self.labels.entry(canon).or_insert(base + i);
                }
            }
        }
    }

    /// `append` と同等だが、ファイル名 (= basename) も記録する。
    /// 後の `find_file` 経由で `Continue <path>` から start_pc を引くのに使う。
    pub fn append_with_name(&mut self, statements: &[EventStatement], file_path: &str) {
        let start = self.statements.len();
        self.append(statements);
        let end = self.statements.len();
        let basename = library_basename(file_path);
        if !basename.is_empty() {
            self.files.push(FileEntry {
                basename,
                start_pc: start,
                end_pc: end,
            });
        }
    }

    pub fn label_pc(&self, name: &str) -> Option<usize> {
        self.labels.get(name).copied()
    }

    /// パス (`Eve\onsen.eve` でも `onsen.eve` でも可) から FileEntry を引く。
    /// 大文字小文字 / 区切り文字 (`/`, `\`) は無視。複数該当した場合は
    /// 先に登録された方を返す (SRC 原典の `LoadEventData` 順)。
    pub fn find_file(&self, path: &str) -> Option<&FileEntry> {
        let needle = library_basename(path);
        if needle.is_empty() {
            return None;
        }
        self.files.iter().find(|e| e.basename == needle)
    }

    /// **特定ファイル内** の最初の `label` の PC を返す。
    ///
    /// 多くのシナリオは複数の .eve に同名 `プロローグ:` ラベルを持つ
    /// (lib/CMaking.eve / lib/Help.eve / 主シナリオ.eve …) が、`label_pc`
    /// は first-wins で alphabetically 早いものを返してしまう。main entry の
    /// プロローグや、特定の intermission ファイルのエントリを狙いたい場合は
    /// 本 helper でファイル PC 範囲内の label に絞って検索する。
    ///
    /// ファイル内に統制したい label が複数あった場合は最初に見つかったもの
    /// を返す (SRC 原典の Event.bas 解釈順)。
    pub fn label_pc_in_file(&self, file_path: &str, label: &str) -> Option<usize> {
        let entry = self.find_file(file_path)?;
        for i in entry.start_pc..entry.end_pc.min(self.statements.len()) {
            if let EventStatement::Command { name, args, .. } = &self.statements[i] {
                if canonical_label_full(name, args).as_deref() == Some(label) {
                    return Some(i);
                }
            }
        }
        None
    }

    /// `current_pc` を含むファイル内で `label` を探し、見つからなければ
    /// global `labels` にフォールバックする。
    ///
    /// 同名ラベルが複数 .eve に存在する場合 (`敵配置` が Main.eve と
    /// EventBattle.eve の双方にある等)、フラットな `labels` は first-wins
    /// で別ファイルの定義を返してしまう。SRC のシナリオは各 .eve が自前の
    /// サブルーチンを持つ前提なので、`Call` / `Goto` は現ファイル内を
    /// 優先解決して cross-file の誤飛びを防ぐ。
    pub fn label_pc_scoped(&self, current_pc: usize, label: &str) -> Option<usize> {
        if let Some(entry) = self
            .files
            .iter()
            .find(|e| current_pc >= e.start_pc && current_pc < e.end_pc)
        {
            let end = entry.end_pc.min(self.statements.len());
            for i in entry.start_pc..end {
                if let EventStatement::Command { name, args, .. } = &self.statements[i] {
                    if canonical_label_full(name, args).as_deref() == Some(label) {
                        return Some(i);
                    }
                }
            }
        }
        self.labels.get(label).copied()
    }

    /// `current_pc` を含むファイル**内のみ**で `label` を探す。
    ///
    /// [`Self::label_pc_scoped`] と違い、ファイル内に無くても global labels へ
    /// **フォールバックしない**。`Continue <file>` のエピローグ解決に使う:
    /// 現シナリオファイルにエピローグが無いのに別ファイル (同名 `エピローグ:`
    /// を持つ後続章など) のエピローグへ誤ジャンプするのを防ぐ。
    ///
    /// ファイル登録自体が無い (`execute()` 直叩き = テスト等) 場合は、互換の
    /// ため global labels で解決する (旧 `labels.contains_key` 挙動の保存)。
    pub fn label_pc_within_file(&self, current_pc: usize, label: &str) -> Option<usize> {
        if let Some(entry) = self
            .files
            .iter()
            .find(|e| current_pc >= e.start_pc && current_pc < e.end_pc)
        {
            let end = entry.end_pc.min(self.statements.len());
            for i in entry.start_pc..end {
                if let EventStatement::Command { name, args, .. } = &self.statements[i] {
                    if canonical_label_full(name, args).as_deref() == Some(label) {
                        return Some(i);
                    }
                }
            }
            // ファイル登録あり: 当該ファイル内に無ければ None (誤飛び防止)。
            return None;
        }
        // ファイル未登録経路: 旧挙動どおり global で解決。
        self.labels.get(label).copied()
    }
}

/// `Eve\onsen.eve` / `lib/CMaking.eve` 等から、`/` `\\` の末尾以降を
/// lowercase で取り出したものを返す。
fn library_basename(path: &str) -> String {
    let slash = path.rfind('/').map(|i| i + 1).unwrap_or(0);
    let bslash = path.rfind('\\').map(|i| i + 1).unwrap_or(0);
    path[slash.max(bslash)..].to_ascii_lowercase()
}

/// `name:` / `@name` / `*name:` を正規化したラベルキーを返す。
/// SRC では `*label:` (アスタリスク接頭辞) はイベントハンドラ用の特殊
/// ラベルだが、本実装ではラベル名の一部とは見做さず通常ラベル相当に
/// 正規化する。これにより `*スタート:` が `trigger_label("スタート")`
/// で発火可能になる。
fn canonical_label(token: &str) -> Option<String> {
    let stripped = token.strip_suffix(':').unwrap_or(token);
    if let Some(rest) = stripped.strip_prefix('*') {
        if !rest.is_empty() {
            return Some(rest.to_string());
        }
    }
    if let Some(s) = stripped.strip_prefix('@') {
        if !s.is_empty() {
            return Some(s.to_string());
        }
    }
    // `label:` 形式 (`:` で終わるが prefix 無し) のみここで採用。
    if token.ends_with(':') && !stripped.is_empty() {
        return Some(stripped.to_string());
    }
    None
}

/// 複数トークンに分割される空白入りラベル (`ターン 1:` / `Destruction ガロ:`)
/// にも対応した正規化。`name` + `args` を組み合わせて末尾 `:` を持つ完全な
/// ラベル名を構築する。
///
/// 「name が SRC 自動発火キーワード」のときのみ multi-token ラベル
/// として扱う。これで `Goto end:` のような control-flow + target を
/// ラベルと誤認しない (VB6 原典では `Goto target:` の表記は許容され、
/// target 側の `:` は jump 時に剥がす)。
fn canonical_label_full(name: &str, args: &[String]) -> Option<String> {
    // 1) 単一トークンの label / anchor (`プロローグ:` / `@onsen` / `*スタート:`)
    if args.is_empty() {
        return canonical_label(name);
    }
    // 2) anchor 系 (`@xxx` / `*xxx:`) は単一トークン採用に固定。
    if name.starts_with('@') || name.starts_with('*') {
        return canonical_label(name);
    }
    // 3) multi-token ラベル: SRC が自動発火する label のみホワイトリスト方式で
    //    受理。`Turn N` / `ターン N` / `Turn N 味方` / `Destruction <名>`
    //    / `破壊 <名>` 等。
    //
    //    元 SRC `Event.bas::HandleEvent` の発火ラベル一覧参照。`Goto`/`Call`
    //    のような control-flow とは name 部で確実に分離するため、ホワイト
    //    リスト方式にしている。
    //
    //    SRC.Sharp `SRCCore` 内 `HandleEvent("…")` 呼出箇所を grep して
    //    実存する name に揃えた (Friendship / Item / Damage 等の標準
    //    auto-fire は VB6 原典に存在しないため除外)。
    const AUTOFIRE_KEYWORDS: &[&str] = &[
        // 制御 / 進行
        "Turn",
        "ターン",
        "Destruction",
        "破壊",
        "全滅",
        "勝利条件",
        // ユニット動作
        "Move",
        "移動",
        "進入",
        "接触",
        "攻撃",
        "攻撃後",
        "使用",
        "使用後",
        "行動終了",
        "損傷率",
        "脱出",
        "Escape",
        // ユニット変化
        "Transform",
        "変形",
        "Combine",
        "合体",
        "Split",
        "分離",
        "収納",
        "ハイパーモード",
        // 育成
        "LevelUp",
        "レベルアップ",
        // その他
        "Conversation",
        "会話",
        "特殊効果",
        "再開",
        "MapAttack",
        "マップ攻撃破壊",
    ];
    if !AUTOFIRE_KEYWORDS.contains(&name) {
        return None;
    }
    if let Some(last) = args.last() {
        if last.ends_with(':') {
            let mut combined = String::from(name);
            for a in args {
                combined.push(' ');
                combined.push_str(a);
            }
            combined.pop(); // 末尾 `:`
            return Some(combined);
        }
    }
    None
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
                let ret_val = call_label_sync_for_condition(app, label_name);
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

/// `evaluate_command_condition` が `Call(<label>)` 形式の条件を評価する際に
/// 用いる同期的サブルーチン呼び出し。
///
/// 該当ラベルから始まるサブルーチンをステップ実行し、`Return <value>` で返された
/// 文字列値を返す。ラベルが見つからない / 実行エラーの場合は空文字列。
///
/// 同期実行なので `Talk` / `Confirm` 等のダイアログや `Wait` タイマが呼ばれた
/// 場合は中断して空文字列を返す。条件サブルーチンではこれらを使用しないこと。
fn call_label_sync_for_condition(app: &mut App, label_name: &str) -> String {
    let lib = app.script_library();
    let Some(pc) = lib.label_pc(label_name) else {
        return String::new();
    };
    let stmts = lib.statements.clone();
    let labels = lib.labels.clone();

    // 戻り値フィールドをクリアしてフレームをセットアップ
    app.set_last_return_value(String::new());
    let saved = enter_call_args(app, &[]);
    // 番兵 PC: stmts.len() を使うと Return 後のループ終了判定に便利
    app.push_call_return(stmts.len(), saved);
    let call_depth_before = app.call_stack_depth();
    let for_depth_before = app.for_stack_len();

    let mut curr_pc = pc + 1; // ラベル行の次から実行
    const MAX_COND_STEPS: usize = 100_000;
    for _ in 0..MAX_COND_STEPS {
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
                match exec_command_pc(app, &name, &args, line_num, curr_pc, &stmts, &labels) {
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
                app.on_script_completed();
            }
        }
        Ok(ExecOutcome::Completed) => {
            if app.script_run_depth() == 0 {
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
    // VB6 風代入文の救済: `var = expr` (第 1 引数が裸の `=`) は
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
        return exec_command_pc(
            app,
            "Set",
            &assign_sugar_args(name, &rhs),
            line,
            pc,
            stmts,
            labels,
        );
    }
    // 各引数を $(var) / Args(N) / 関数呼出 / インデックス変数で展開後、
    // `&` 連結演算子を畳み込む（`"abc" & x & ".bmp"` → "abcvalue.bmp"）。
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
            let (raw_cond, raw_body) = split_if_cond_body(args);
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
        "set" | "local" => {
            // Set var value...  (value が複数トークンの場合はスペース連結)
            if args.is_empty() {
                return Err(err(line, "Set/Local には変数名が必要。"));
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
            let value = if xargs.len() >= 2 {
                // `#` コメントを除去 (SRC.Sharp `SetCmd.cs` 後方互換性):
                // `Set var value # comment` 形式で `#` 以降を無視する。
                // 重要: チェック対象は xargs[2] (値の直後のトークン) のみ。
                // xargs[1] が `#` で始まる場合は色コード (`#3264c8` 等) であり
                // コメントとは区別する (この場合 xargs.len()==2 で下記 end=2 分岐)。
                let end = if xargs.len() > 2 && xargs[2].starts_with('#') {
                    2 // `#` コメント発見 → 値は xargs[1] のみ
                } else {
                    xargs.len()
                };
                if end == 2 {
                    // 単一トークン値 (`Set マップ決定 選択` 形式) は SRC 同様に
                    // 裸の識別子を script_var として自動解決する。`fn_arg_value`
                    // は「変数として引いて空でなければ採用、そうでなければ literal」
                    // という SRC 流の値評価セマンティクス。
                    fn_arg_value(app, &xargs[1])
                } else {
                    xargs[1..end].join(" ")
                }
            } else if lname == "set" {
                // SRC `SetCmd`: 値なし `Set var` はフラグとして 1 を代入する
                // (`Set 機体確認` → `If 機体確認` が真)。`Local var` は空文字。
                "1".to_string()
            } else {
                String::new()
            };
            // SRC の `Set` は値を式評価する。`expand_vars` は関数呼出を
            // 評価するが `(a + b)` のような括弧付き算術はトークンとして
            // 残す。値全体が単一の括弧式で中身が純粋な算術なら数値化する。
            let value = eval_paren_arith_value(app, &value).unwrap_or(value);
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
            let retval = xargs.first().cloned().unwrap_or_default();
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
            // `IncrCmd` は `GetArgAsDouble` を使い、非数値文字列は VB6 `Val()`
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
            // SRC `Suspend` (`Suspendコマンド.md`):
            // ゲーム進行を中断 (= 中断セーブの自動作成 + Title 復帰)。
            // 本実装は中断セーブの自動保存は行わず、シーンを Title に戻し
            // インターミッションコマンド / 進行情報をクリアして「ゲーム中断」
            // 状態を擬似的に再現する。フロントエンドが to_save_json() を
            // 呼んで localStorage 保存する責務を持つ。
            app.set_scene(crate::Scene::Title);
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
                            .map(|u| (u.total_exp / 100).max(0) as i64 + 1)
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
                        "レベル" | "Level" => (1 + inst.total_exp / 100).min(99) as i64,
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
            // `Finish` または `Finish label` — ステージを Victory にして
            // (label 指定があれば) 後段ラベルへ自動遷移する用途。
            // 簡略化: stage_state を Victory にし、label があれば script_var に記録。
            app.set_stage_state(crate::stage::StageState::Victory);
            if let Some(label) = xargs.first() {
                app.set_script_var("__next_stage".to_string(), label.clone());
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
            // `Transform unit "newunit"`
            if xargs.len() < 2 {
                return Err(err(line, "Transform 命令は 2 引数必要 (unit new_unit)。"));
            }
            let target = xargs[0].clone();
            let new_unit = xargs[1].clone();
            let mut fire: Option<(String, String, crate::Party)> = None;
            if let Some(idx) = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, &target))
            {
                app.database_mut().unit_instances[idx].unit_data_name = new_unit.clone();
                // 変形後フォームの UnitData.features で active_features を更新
                let new_features = app
                    .database()
                    .unit_by_name(&new_unit)
                    .map(|ud| {
                        ud.features
                            .iter()
                            .map(|(n, v)| crate::feature::ActiveFeature::new(n.clone(), v.clone()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                app.database_mut().unit_instances[idx].active_features = new_features;
                let u = &app.database().unit_instances[idx];
                fire = Some((u.pilot_name.clone(), u.unit_data_name.clone(), u.party));
            }
            if let Some((pilot_name, unit_data_name, party)) = fire {
                fire_unit_event_labels(
                    app,
                    &["変形", "Transform"],
                    &pilot_name,
                    &unit_data_name,
                    party,
                    Some(&new_unit),
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
                app.database_mut().unit_instances[i].unit_data_name = mode.clone();
                let new_feats = app
                    .database()
                    .unit_by_name(&mode)
                    .map(|ud| {
                        ud.features
                            .iter()
                            .map(|(n, v)| crate::feature::ActiveFeature::new(n.clone(), v.clone()))
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                app.database_mut().unit_instances[i].active_features = new_feats;
                let u = &app.database().unit_instances[i];
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
                    let new_feats = app
                        .database()
                        .unit_by_name(&new_data)
                        .map(|ud| {
                            ud.features
                                .iter()
                                .map(|(n, v)| {
                                    crate::feature::ActiveFeature::new(n.clone(), v.clone())
                                })
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
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
                                lib.labels.insert(key.clone(), i);
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
            // 指定ラベルを script_library.labels から削除。引数省略時は
            // **現在実行中の auto-fire ラベル** を消すが、本実装はその情報を
            // 持たないため省略時は no-op (`攻撃 A B` 等で `ClearEvent` 単独
            // 使用するシナリオは `ClearEvent "攻撃 A B"` への置換が必要)。
            // 機能的には `Forget` のエイリアス + ラベル名引数の正規化。
            // SRC.Sharp 準拠: 2 引数以上はエラー。
            if xargs.len() > 1 {
                return Err(err(line, "ClearEventコマンドの引数の数が違います"));
            }
            if let Some(label) = xargs.first() {
                let key = label
                    .trim_start_matches('@')
                    .trim_start_matches('*')
                    .trim_end_matches(':')
                    .trim_matches('"')
                    .to_string();
                app.script_library_mut().labels.remove(&key);
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
                if let Some(u) = app
                    .database_mut()
                    .unit_instances
                    .iter_mut()
                    .find(|u| matches_unit_handle(u, &target_handle))
                {
                    u.pilot_name = pilot;
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
            //    元実装: VB6 `Event.bas` / SRC.Sharp
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
            // `Upgrade unit attr n` — UnitData の base 値を + n
            // (簡略化: hp/en/armor/mobility/speed のみ対応)
            if xargs.len() < 3 {
                return Ok(pc + 1);
            }
            let unit_name = xargs[0].clone();
            let attr = xargs[1].to_ascii_lowercase();
            let n: i64 = xargs[2].parse().unwrap_or(0);
            if let Some(d) = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &unit_name))
                .map(|u| u.unit_data_name.clone())
            {
                if let Some(ud) = app.database_mut().units.iter_mut().find(|u| u.name == d) {
                    match attr.as_str() {
                        "hp" => ud.hp += n,
                        "en" => ud.en += n as i32,
                        "armor" => ud.armor += n,
                        "mobility" => ud.mobility += n as i32,
                        "speed" => ud.speed += n as i32,
                        _ => {}
                    }
                }
            }
            return Ok(pc + 1);
        }
        "changeparty" => {
            // `ChangeParty unit "Enemy"` — 所属勢力変更
            if xargs.len() < 2 {
                return Err(err(line, "ChangeParty 命令は 2 引数必要 (unit party)。"));
            }
            let target = xargs[0].clone();
            let party = parse_party(&xargs[1], line)?;
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
            // `Escape unit_or_pilot` — マップから一時退避（撤退）。
            // `off_map = true` を立てて AI / 戦闘 / 描画 / 勝利判定から除外。
            // 後段の `Launch` / `Place` で `off_map = false` に戻して再配置できる。
            // 引数は裸識別子 (例: `対象ユニットＩＤ` システム変数) も解決する。
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
                }
            }
            return Ok(pc + 1);
        }
        "specialpower" => {
            // `SpecialPower unit power_name [target]` — 精神コマンドを発動。
            // 1) 当該パイロットの PilotInstance.sp_remaining を計算。
            // 2) `power_name` の SP コストが足りなければ無発動。
            // 3) 足りれば PilotInstance.sp_remaining を消費し、condition を適用。
            // 4) lifetime に基づいて condition を付与（永久 or 1ターン）。
            if xargs.len() < 2 {
                return Ok(pc + 1);
            }
            let target = xargs[0].clone();
            let power = xargs[1].clone();
            let target_of_power = xargs.get(2).cloned();

            // Find the unit
            let unit_idx = match app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, &target))
            {
                Some(i) => i,
                None => return Ok(pc + 1),
            };

            // Get pilot ID
            let pilot_id = app.database().unit_instances[unit_idx]
                .pilot_ids
                .first()
                .map(|s| s.as_str())
                .unwrap_or(&app.database().unit_instances[unit_idx].pilot_name);

            // Try to find existing pilot instance
            let pilot_idx = app
                .database()
                .pilot_instances
                .iter()
                .position(|p| p.id == pilot_id || p.pilot_data_name == pilot_id);

            if let Some(pilot_idx) = pilot_idx {
                // PilotInstance exists - look up SP cost from special_powers
                let sp_consumption = app
                    .database()
                    .special_powers
                    .iter()
                    .find(|sp| sp.name == power)
                    .map(|sp| sp.sp_consumption)
                    .unwrap_or_else(|| sp_cost_for(&power));

                if !app.database_mut().pilot_instances[pilot_idx].try_consume_sp(sp_consumption) {
                    log::warn!("SpecialPower: insufficient SP for '{}'", power);
                    return Ok(pc + 1);
                }

                let lifetime = match app
                    .database()
                    .special_powers
                    .iter()
                    .find(|sp| sp.name == power)
                {
                    Some(sp) => match sp.duration.as_str() {
                        "瞬間" => 0,
                        "発動ターン" | "1ターン" => 1,
                        "2ターン" => 2,
                        "3ターン" => 3,
                        _ => -1,
                    },
                    None => -1,
                };

                if lifetime >= 0 {
                    let actual_target = target_of_power.as_deref().unwrap_or(&target);
                    if let Some(idx) = app
                        .database()
                        .unit_instances
                        .iter()
                        .position(|u| matches_unit_handle(u, actual_target))
                    {
                        let u = &mut app.database_mut().unit_instances[idx];
                        u.add_condition(Condition::new(
                            &power,
                            if lifetime == 0 { 1 } else { lifetime },
                        ));
                    }
                }
            } else {
                // Fallback: no PilotInstance - use old behavior with PilotData.sp
                let max_sp = app
                    .database()
                    .pilot_by_name(pilot_id)
                    .and_then(|p| p.sp)
                    .unwrap_or(0);
                let cur_consumed = app.database().unit_instances[unit_idx].sp_consumed;
                let remaining_sp = max_sp - cur_consumed;
                let cost = sp_cost_for(&power);
                if max_sp > 0 && cost > remaining_sp {
                    return Ok(pc + 1);
                }
                let u = &mut app.database_mut().unit_instances[unit_idx];
                u.sp_consumed += cost;
                if !u.has_condition(&power) {
                    u.add_condition(Condition::new(power, -1));
                }
            }

            return Ok(pc + 1);
        }
        "mind" => {
            // SRC `Mind pilot sp_name` — SP を消費せずに精神コマンドを強制発動。
            // イベント演出や NPC 強化でよく使われる。`SpecialPower` と異なり SP 消費なし。
            // `MindAnime sp_name pilot` は視覚演出のみ (Stub)。
            if xargs.len() < 2 {
                return Ok(pc + 1);
            }
            let pilot_key = xargs[0].clone();
            let sp_name = xargs[1].clone();
            // パイロット名 → UnitInstance を逆引きして Condition を付与。
            if let Some(u) = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| u.pilot_name == pilot_key || matches_unit_handle(u, &pilot_key))
            {
                if !u.has_condition(&sp_name) {
                    u.add_condition(Condition::new(sp_name, -1));
                }
            }
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
            map_attack(app, unit_key.as_deref(), &weapon_name, cx, cy);
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
            map_attack(app, unit_key.as_deref(), &weapon_name, cx, cy);
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
                        .unit_instances
                        .retain(|u| u.summoned_by.as_deref() != Some(uid.as_str()));
                }
            }
            return Ok(pc + 1);
        }
        "attack" => {
            // SRC `Attack unit1 weapon1 unit2 weapon2` (`Attackコマンド.md`):
            // スクリプト経由の戦闘実行。射程/気力/EN 等の使用条件は無視。
            // weapon1: `自動` で最強武器自動選択 / 武器名で固定
            // weapon2: `防御`/`回避`/`無抵抗` で反撃なし / `自動` or 武器名で反撃
            // 仕様準拠: `攻撃` / `攻撃後` / `損傷率` ラベルは Attack 命令経由では
            // 発火させない (spec の "Attackコマンドや MapAttackコマンドなどによる
            // イベント上の戦闘では発生しません" を遵守)。
            if xargs.len() < 4 {
                return Err(err(
                    line,
                    "Attack 命令は 4 引数必要 (unit1 weapon1 unit2 weapon2)。",
                ));
            }
            let key1 = fn_arg_value(app, &xargs[0]);
            let w1_arg = xargs[1].clone();
            let key2 = fn_arg_value(app, &xargs[2]);
            let w2_arg = xargs[3].clone();
            // 攻撃側の武器選択
            let atk_data = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &key1))
                .and_then(|u| app.database().unit_by_name(&u.unit_data_name).cloned());
            let def_data = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &key2))
                .and_then(|u| app.database().unit_by_name(&u.unit_data_name).cloned());
            let Some(atk_data) = atk_data else {
                return Ok(pc + 1);
            };
            let Some(_def_data) = def_data else {
                return Ok(pc + 1);
            };
            // weapon1 = "自動" → 最大攻撃力武器、それ以外 → 名前一致
            let weapon = if w1_arg == "自動" || w1_arg.eq_ignore_ascii_case("auto") {
                atk_data.weapons.iter().max_by_key(|w| w.power)
            } else {
                atk_data.weapons.iter().find(|w| w.name == w1_arg)
            };
            let Some(weapon) = weapon else {
                return Ok(pc + 1);
            };
            // SRC 準拠ダメージ計算 (pilot stats + morale 考慮)
            let atk_pilot = app
                .database()
                .pilot_by_name(
                    &app.database()
                        .unit_instances
                        .iter()
                        .find(|u| matches_unit_handle(u, &key1))
                        .map(|u| u.pilot_name.clone())
                        .unwrap_or_default(),
                )
                .cloned();
            let def_pilot = app
                .database()
                .pilot_by_name(
                    &app.database()
                        .unit_instances
                        .iter()
                        .find(|u| matches_unit_handle(u, &key2))
                        .map(|u| u.pilot_name.clone())
                        .unwrap_or_default(),
                )
                .cloned();
            let atk_morale = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &key1))
                .map(|u| u.morale)
                .unwrap_or(100);
            let def_morale = app
                .database()
                .unit_instances
                .iter()
                .find(|u| matches_unit_handle(u, &key2))
                .map(|u| u.morale)
                .unwrap_or(100);
            let default_pilot = crate::data::pilot::PilotData {
                spirit_commands: Vec::new(),
                name: String::new(),
                nickname: String::new(),
                kana_name: String::new(),
                sex: crate::data::pilot::Sex::Unspecified,
                class: String::new(),
                adaption: crate::data::pilot::Adaption::parse("AAAA").unwrap(),
                exp_value: 0,
                infight: 100,
                shooting: 100,
                hit: 0,
                dodge: 0,
                intuition: 0,
                technique: 0,
                personality: None,
                sp: None,
                bgm: None,
                bitmap: None,
                features: Vec::new(),
            };
            let atk_pilot_ref = atk_pilot.as_ref().unwrap_or(&default_pilot);
            let def_pilot_ref = def_pilot.as_ref().unwrap_or(&default_pilot);
            let preview = crate::combat::predict_with_status(
                atk_pilot_ref,
                &atk_data,
                weapon,
                def_pilot_ref,
                &_def_data,
                0,
                0,
                atk_morale,
                def_morale,
                &[],
                &[],
            );
            let damage = preview.damage;
            apply_damage_no_event(app, &key2, damage);
            // 反撃: weapon2 が `防御`/`回避`/`無抵抗` 以外なら攻撃側にもダメージ
            if !matches!(
                w2_arg.as_str(),
                "防御" | "回避" | "無抵抗" | "Defense" | "Evade" | "Nodefense"
            ) {
                let counter = if w2_arg == "自動" || w2_arg.eq_ignore_ascii_case("auto") {
                    _def_data.weapons.iter().max_by_key(|w| w.power)
                } else {
                    _def_data.weapons.iter().find(|w| w.name == w2_arg)
                };
                if let Some(cw) = counter {
                    let counter_preview = crate::combat::predict_with_status(
                        def_pilot_ref,
                        &_def_data,
                        cw,
                        atk_pilot_ref,
                        &atk_data,
                        0,
                        0,
                        def_morale,
                        atk_morale,
                        &[],
                        &[],
                    );
                    apply_damage_no_event(app, &key1, counter_preview.damage);
                }
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
            // SRC `UseAbility unit ability [target]` (`UseAbilityコマンド.md`):
            // ユニットに指定アビリティを発動させる。本実装はアビリティ効果の
            // フル dispatch を持たないが、SRC.NET `AbilityEffect.cs` で頻出する
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
                        .and_then(|u| app.database().unit_by_name(&u.unit_data_name))
                        .map(|d| d.hp)
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
            // Hotpoint は別管理なのでそのまま。
            app.script_overlay_mut().clear();
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
        "fillstyle" | "background" | "drawoption" | "renametitle" | "renamebgm" | "freememory"
        | "exec" | "make" | "playmovie" | "redraw" | "setbackground" => {
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
                    let parent_argnum = saved
                        .get(9)
                        .and_then(|s| s.parse::<usize>().ok())
                        .unwrap_or(0);

                    // 祖先の Args(1..parent_argnum) を base の直後から設定
                    // まず base+1 以降をクリア (前回の UpVar 分を上書き)
                    for k in (base + 1)..=9 {
                        app.set_script_var(format!("Args({k})"), String::new());
                    }
                    for i in 0..parent_argnum.min(9 - base) {
                        let val = saved.get(i).cloned().unwrap_or_default();
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
        "polygon" | "circle" | "arc" => {
            // グラフィック描画命令。本実装はスクリプト実行用なので no-op。
            return Ok(pc + 1);
        }
        "fillcolor" => {
            let color = xargs
                .first()
                .cloned()
                .unwrap_or_else(|| "#000000".to_string());
            app.script_overlay_mut()
                .push(crate::script_overlay::DrawCmd::SetColor { color });
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
            // 撃破ラベル発火用に該当ユニットの (pilot, unit_data) を退避。
            // `Kill name` は名前 1 致するもの全てを除去するので、複数該当
            // した場合は順に発火する。
            let mut destroyed: Vec<(String, String)> = Vec::new();
            for u in &app.database().unit_instances {
                if matches_unit_handle(u, target) {
                    destroyed.push((u.pilot_name.clone(), u.unit_data_name.clone()));
                }
            }
            app.database_mut()
                .unit_instances
                .retain(|u| !matches_unit_handle(u, target));
            for (p, ud) in destroyed {
                fire_destruction_labels(app, &p, &ud);
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
            let party = parse_party(&xargs[0], line)?;
            let unit_data_name = fn_arg_value(app, &xargs[1]);
            // rank = xargs[2] — 未使用 (UnitInstance に未対応)
            let pilot = fn_arg_value(app, &xargs[3]);
            // level = xargs[4] — 未使用
            // 座標は app-aware 式評価で解決する。SRC は座標を式として評価するため、
            // `For i = ... / Create 中立 壁 0 P 1 4 i / Next` のような**裸のループ変数**や
            // 算術式が座標に来る (`eval_coord_u32`)。
            let x = eval_coord_u32(app, &xargs, 5);
            let y = eval_coord_u32(app, &xargs, 6);
            let mut inst = UnitInstance::new(unit_data_name, pilot.clone(), party, x, y);
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
                .unit_instances
                .retain(|u| !matches_unit_handle(u, target));
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
            // `Level()` 関数の規約に合わせ、`level = total_exp / 100 + 1` で算出。
            // レベル繰り上がりは event 発火条件。
            let (old_level, new_level, pilot_ids): (i32, i32, Vec<String>) =
                if let Some(i) = target_idx {
                    let u = &mut app.database_mut().unit_instances[i];
                    let old = (u.total_exp / 100).max(0) + 1;
                    u.total_exp += n;
                    let new_ = (u.total_exp / 100).max(0) + 1;
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
            // LevelUp unit [n]   ─ 1 レベル = 100 exp 換算で total_exp に加算
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
                    u.total_exp += n * 100;
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
                        if pilot_inst.add_exp(n * 100) {
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
            let party = parse_party(&xargs[2], line)?;
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

/// `If` の条件部評価。`xargs` は変数展開後の引数列。
///
/// 受け付ける形:
/// - `IsDead unit_name` / `IsAlive unit_name` (述語関数)
/// - `lhs op rhs` (比較式)
/// - 単独 `var` (空でない & 0 でないなら真)
///
/// 末尾 `Then` は省略可。`app` は述語評価で必要。
fn eval_condition_args_with(app: &App, xargs: &[String]) -> bool {
    let mut a: Vec<&str> = xargs.iter().map(|s| s.as_str()).collect();
    if let Some(last) = a.last() {
        if last.eq_ignore_ascii_case("then") {
            a.pop();
        }
    }
    // 論理演算子 And / Or で分割。Or が一番低優先度、And は次。
    // 例: `A = B And C = D Or E = F` は `(A=B AND C=D) OR (E=F)`。
    if let Some(or_pos) = a.iter().position(|t| t.eq_ignore_ascii_case("or")) {
        let left: Vec<String> = a[..or_pos].iter().map(|s| s.to_string()).collect();
        let right: Vec<String> = a[or_pos + 1..].iter().map(|s| s.to_string()).collect();
        return eval_condition_args_with(app, &left) || eval_condition_args_with(app, &right);
    }
    if let Some(and_pos) = a.iter().position(|t| t.eq_ignore_ascii_case("and")) {
        let left: Vec<String> = a[..and_pos].iter().map(|s| s.to_string()).collect();
        let right: Vec<String> = a[and_pos + 1..].iter().map(|s| s.to_string()).collect();
        return eval_condition_args_with(app, &left) && eval_condition_args_with(app, &right);
    }
    // 先頭 "Not" → 残りを反転評価。残りが単一 (paren-balanced) なら
    // 中身を split して再評価する。
    if let Some(head) = a.first() {
        if head.eq_ignore_ascii_case("not") {
            let rest_joined: String = a[1..].join(" ");
            let stripped = strip_outer_parens(&rest_joined);
            let parts = split_balanced(&stripped);
            return !eval_condition_args_with(app, &parts);
        }
    }
    // 述語関数（IsDead など）
    if a.len() >= 2 {
        match a[0].to_ascii_lowercase().as_str() {
            "isdead" | "killed" => return !unit_alive(app, a[1]),
            "isalive" | "alive" => return unit_alive(app, a[1]),
            _ => {}
        }
    }
    if a.len() >= 3 {
        // SRC: `If 選択 = ランダム` のように `$()` を省略した場合は、
        // 裸の識別子を script_var として自動解決する。`fn_arg_value` の
        // セマンティクス (引数 1 個を「裸識別子なら変数、クオートなら literal」
        // で評価) を再利用。
        // 比較対象が括弧付き算術式 (`(真武器番[武器数] - 1)` 等) の場合は
        // 先に算術評価する。`eval_paren_arith_value` は全アトムが数値/数値変数/
        // キーワードのときだけ Some を返すため、文字列比較 (`If 選択 = ランダム`)
        // や関数 (`Info(...)`) は誤評価せず `fn_arg_value` にフォールバックする。
        // これが無いと `If (N - 1) > 3` の左辺が文字列のまま比較され常に偽になる。
        let rhs_raw = a[2..].join(" ");
        let lhs = eval_paren_arith_value(app, a[0]).unwrap_or_else(|| fn_arg_value(app, a[0]));
        let rhs =
            eval_paren_arith_value(app, &rhs_raw).unwrap_or_else(|| fn_arg_value(app, &rhs_raw));
        eval_binop(&lhs, a[1], &rhs)
    } else if a.len() == 1 {
        // 単独トークン: paren-wrapped なら中身を再評価 (`If (cond) ...` のため)。
        let t = a[0].trim();
        if t.starts_with('(') && t.ends_with(')') && t.len() >= 2 {
            let inner = strip_outer_parens(t);
            let parts = split_balanced(&inner);
            return eval_condition_args_with(app, &parts);
        }
        // 数値リテラルは 0 以外が真。
        if let Ok(n) = t.parse::<f64>() {
            return n != 0.0;
        }
        // クオート文字列は中身の truthiness。
        if let Some(inner) = t.strip_prefix('"').and_then(|s| s.strip_suffix('"')) {
            return !(inner.is_empty() || inner == "0");
        }
        // 裸の識別子は変数参照。**定義済み** なら値の truthiness で判定する
        // (`If カウンタ` が 0 なら偽、`If フラグ` が 1 なら真)。
        // 未定義のトークンは従来どおり「空 / 0 以外を真」とする
        // (リテラル / 未対応記法を誤って偽にしないため)。
        let key = if t.contains('[') {
            resolve_lhs_name(app, t)
        } else {
            t.to_string()
        };
        if app.is_script_var_defined(&key) {
            let v = app.script_var(&key);
            !(v.is_empty() || v == "0")
        } else {
            false
        }
    } else {
        false
    }
}

/// 名前から UnitInstance を引く。`uid` (Create で採番した一意 ID) /
/// `unit_data_name` / `pilot_name` のいずれかが一致すれば真。
fn matches_unit_handle(u: &UnitInstance, key: &str) -> bool {
    !u.uid.is_empty() && u.uid == key || u.unit_data_name == key || u.pilot_name == key
}

/// 与えた名前のユニットが生存しているか。
fn unit_alive(app: &App, key: &str) -> bool {
    app.database()
        .unit_instances
        .iter()
        .any(|u| matches_unit_handle(u, key))
}

/// `RecoverHP unit [spec]` の本処理。
/// `spec` 解釈:
/// - `"Full"` / `"全"` / `None` → 完全回復 (damage = 0)
/// - `"50%"` 等 → 最大 HP × % を回復
/// - `"500"` 等 → 直接 500 ダメージ減
fn recover_hp(app: &mut App, key: &str, spec: Option<&str>) {
    let idx = match app
        .database()
        .unit_instances
        .iter()
        .position(|u| matches_unit_handle(u, key))
    {
        Some(i) => i,
        None => return,
    };
    let max_hp = app
        .database()
        .unit_by_name(&app.database().unit_instances[idx].unit_data_name)
        .map(|u| u.hp)
        .unwrap_or(0);
    let cur_dmg = app.database().unit_instances[idx].damage;
    // `rate` は常にパーセント (SRC 準拠: "HP を rate% 回復")。
    // `%` サフィックスは後方互換として受け入れるが、付いていない数値も百分率扱い。
    // RecoverHP によって HP が 0 以下になることはない (最低値 = 1 → damage = max_hp - 1)。
    let new_dmg = match spec {
        None | Some("") => 0,
        Some(s) if s.eq_ignore_ascii_case("full") || s == "全" => 0,
        Some(s) => {
            let pct: f64 = s.trim_end_matches('%').parse().unwrap_or(0.0);
            let recover = ((max_hp as f64) * pct / 100.0).round() as i64;
            // 回復後ダメージ: 下限 0 (HP = max_hp)、上限 max_hp - 1 (HP = 1)
            (cur_dmg - recover).max(0).min(max_hp - 1)
        }
    };
    app.database_mut().unit_instances[idx].damage = new_dmg;
}

/// `RecoverEN unit [spec]` の本処理。
fn recover_en(app: &mut App, key: &str, spec: Option<&str>) {
    let idx = match app
        .database()
        .unit_instances
        .iter()
        .position(|u| matches_unit_handle(u, key))
    {
        Some(i) => i,
        None => return,
    };
    let max_en = app
        .database()
        .unit_by_name(&app.database().unit_instances[idx].unit_data_name)
        .map(|u| u.en)
        .unwrap_or(0);
    let cur_consumed = app.database().unit_instances[idx].en_consumed;
    // `rate` は常にパーセント (RecoverHP と同じ仕様)。EN の下限は 0。
    let new_consumed = match spec {
        None | Some("") => 0,
        Some(s) if s.eq_ignore_ascii_case("full") || s == "全" => 0,
        Some(s) => {
            let pct: f64 = s.trim_end_matches('%').parse().unwrap_or(0.0);
            let recover = ((max_en as f64) * pct / 100.0).round() as i32;
            (cur_consumed - recover).max(0)
        }
    };
    app.database_mut().unit_instances[idx].en_consumed = new_consumed;
}

/// `Attack` / `MapAttack` 命令経由のダメージ適用 (損傷率/破壊ラベルを
/// 発火させない版)。原典: "Attackコマンドや MapAttackコマンドなどによる
/// イベント上の戦闘では発生しません"。
/// HP <= 0 になっても unit_instance を残す (Destruction は発火しない)。
fn apply_damage_no_event(app: &mut App, key: &str, amount: i64) {
    let Some(idx) = app
        .database()
        .unit_instances
        .iter()
        .position(|u| matches_unit_handle(u, key))
    else {
        return;
    };
    let max_hp = app
        .database()
        .unit_by_name(&app.database().unit_instances[idx].unit_data_name)
        .map(|u| u.hp)
        .unwrap_or(0);
    let u = &mut app.database_mut().unit_instances[idx];
    u.damage = (u.damage + amount).max(0);
    // HP <= 0 でも instance は残す (撃破イベントを発火させないため)。
    // SRC.NET の Attack 命令も "イベント上の戦闘" として label 発火を抑制する。
    let _ = max_hp;
}

/// 直接ダメージ / 回復を反映。HP <= 0 ならインスタンスを削除。
fn apply_damage(app: &mut App, key: &str, amount: i64) {
    let Some(idx) = app
        .database()
        .unit_instances
        .iter()
        .position(|u| matches_unit_handle(u, key))
    else {
        return;
    };
    let max_hp = app
        .database()
        .unit_by_name(&app.database().unit_instances[idx].unit_data_name)
        .map(|u| u.hp)
        .unwrap_or(0);
    let old_dmg = app.database().unit_instances[idx].damage;
    let u = &mut app.database_mut().unit_instances[idx];
    u.damage = (u.damage + amount).max(0);
    let new_dmg = u.damage;
    if new_dmg >= max_hp && max_hp > 0 {
        // 撃破前にラベル発火用の名前を退避してから unit_instance を除去。
        let pilot_name = u.pilot_name.clone();
        let unit_data_name = u.unit_data_name.clone();
        app.database_mut().remove_unit_at(idx);
        fire_destruction_labels(app, &pilot_name, &unit_data_name);
    } else if new_dmg > old_dmg {
        // HP 残存 + ダメージ増加 → 損傷率閾値跨ぎを検査。
        let u = &app.database().unit_instances[idx];
        let pilot_name = u.pilot_name.clone();
        let unit_data_name = u.unit_data_name.clone();
        let party = u.party;
        fire_damage_threshold_labels(
            app,
            &pilot_name,
            &unit_data_name,
            party,
            old_dmg,
            new_dmg,
            max_hp,
        );
    }
}

/// SRC `Win` / `GameClear` 後の自動発火ラベル ({Victory, 勝利, Ending,
/// エンディング}) を 1 件だけ発火する。`App::game_clear()` の lookup と一致。
fn fire_victory_labels(app: &mut App) {
    // 勝利 / エンディング は章ローカル (各章の `勝利` ラベル)。
    for lab in ["Victory", "勝利", "Ending", "エンディング"] {
        if app.post_stage_event_label(lab.to_string()) {
            return;
        }
    }
}

/// SRC `Lose` / `GameOver` 後の自動発火ラベル ({GameOver, ゲームオーバー})
/// を 1 件だけ発火する。`App::game_over()` の lookup と一致。
fn fire_game_over_labels(app: &mut App) {
    for lab in ["GameOver", "ゲームオーバー"] {
        if app.post_event_label(lab.to_string()) {
            return;
        }
    }
}

/// SRC `Event.HandleEvent("Destruction <name>")` 相当の auto-fire。
///
/// pilot 名 / unit_data 名 / 日英両綴 (`Destruction` / `破壊`) を順に試し、
/// 最初にヒットした 1 件だけ発火する (SRC 原典も最初の matching event だけ
/// 実行する)。続いて該当ユニットの陣営が全滅しているかをチェックし、
/// 全滅していれば `全滅 <party>` ラベルを発火する
/// (SRC.Sharp `Unit.attackmap.cs:1047` / `Unit.ref.cs:25393` 同等)。
///
/// 発火は `App::post_event_label` (原典 `EventQue` 相当) 経由で行う:
/// スクリプト実行中 (`Kill` / `Damage` コマンド等から呼ばれた場合) は
/// キューに積まれ、現在のスクリプト完了後に実行される。これにより
/// Destruction ハンドラに `Talk` 等の suspend 系命令が含まれていても
/// 外側の dispatch ctx を上書きしない (旧実装は再入で上書きハザードが
/// あり、同期完結ハンドラ前提で割り切っていた)。
pub(crate) fn fire_destruction_labels(app: &mut App, pilot_name: &str, unit_data_name: &str) {
    for prefix in ["Destruction", "破壊"] {
        for target in [pilot_name, unit_data_name] {
            if target.is_empty() {
                continue;
            }
            let label = format!("{prefix} {target}");
            if app.post_stage_event_label(label) {
                break;
            }
        }
    }
    fire_total_annihilation_if_any(app);
}

/// 該当陣営のユニットが 1 機も残っていなければ `全滅 <party>` ラベルを発火。
/// 4 陣営それぞれ確認する。ユニット 0 = 一度も登場していない場合も含むが、
/// それは label 未定義なので silent OK。
fn fire_total_annihilation_if_any(app: &mut App) {
    for (party, party_label) in party_long_labels() {
        let alive = app
            .database()
            .unit_instances
            .iter()
            .any(|u| u.party == party && !u.off_map);
        if !alive {
            // 1 度でも当該陣営のユニットが配置されていた場合のみ意味あり。
            // 全滅 label が定義されていなければ no-op。章ローカルイベント。
            let label = format!("全滅 {party_label}");
            if app.post_stage_event_label(label) {
                return;
            }
        }
    }
}

/// `Party` 値 → SRC 表記 (`味方`/`敵`/`友軍`/`中立`) の対応表。
/// SRC の auto-fire ラベル (`全滅 <party>` / `損傷率 <party> <pct>` 等) は
/// この日本語綴を使う。
fn party_long_labels() -> [(crate::Party, &'static str); 4] {
    use crate::Party;
    [
        (Party::Player, "味方"),
        (Party::Enemy, "敵"),
        (Party::Neutral, "中立"),
        (Party::Npc, "ＮＰＣ"),
    ]
}

fn party_long_label(party: crate::Party) -> &'static str {
    for (p, label) in party_long_labels() {
        if p == party {
            return label;
        }
    }
    ""
}

/// SRC `攻撃 <atk> <def>:` (`攻撃イベント.md`) / `攻撃後 <atk> <def>:`
/// (`攻撃後イベント.md`) 共通の auto-fire ヘルパ。
///
/// `prefixes` (`["攻撃"]` または `["攻撃後"]`) と (attacker, defender) の
/// (pilot_name, unit_data_name, party_label) 9 組合せを順に試し、最初の
/// マッチで 1 度だけ発火する。
///
/// 発火順 (SRC.NET 仕様):
/// - prefix → atk identifier 優先順 (pilot → unit → party) → def 同順。
///
/// 仕様乖離: `攻撃イベント` は `Attack`/`MapAttack` 等のシナリオ戦闘では
/// 発火しない (= UI 経路の通常戦闘でだけ発火) という規定があるが、本実装は
/// `attack_target` 経由 (= UI 攻撃のみ) で呼び出すため自然に満たす。
fn fire_pair_event_labels(app: &mut App, prefixes: &[&str], atk: &UnitEventId, def: &UnitEventId) {
    let atk_party_label = party_long_label(atk.party);
    let def_party_label = party_long_label(def.party);
    let atk_ids = [atk.pilot.as_str(), atk.unit.as_str(), atk_party_label];
    let def_ids = [def.pilot.as_str(), def.unit.as_str(), def_party_label];
    for prefix in prefixes {
        for a in atk_ids {
            if a.is_empty() {
                continue;
            }
            for d in def_ids {
                if d.is_empty() {
                    continue;
                }
                let label = format!("{prefix} {a} {d}");
                // 会話 / 攻撃 イベントは章ローカル。
                if app.post_stage_event_label(label) {
                    return;
                }
            }
        }
    }
}

/// 攻撃 / 攻撃後 イベントの 1 ユニット側識別子 3 種 (pilot, unit, party)。
#[derive(Debug, Clone)]
pub struct UnitEventId {
    pub pilot: String,
    pub unit: String,
    pub party: crate::Party,
}

impl UnitEventId {
    pub fn from_unit_instance(u: &crate::UnitInstance) -> Self {
        Self {
            pilot: u.pilot_name.clone(),
            unit: u.unit_data_name.clone(),
            party: u.party,
        }
    }
}

/// SRC `使用 <unit> <device>:` (`使用イベント.md`) の auto-fire。
/// `attack_target` で `攻撃イベント` 発火前に呼ぶ。`device` は武器名 /
/// アビリティ名 / SP 名 / `召喚解除` のいずれか。
///
/// 仕様準拠: サポートアタックの武器では発火しない (本実装ではサポート
/// アタックパスは別の関数経路を通るため自然に満たす)。反撃武器は
/// "攻撃前タイミング" で発火するため、本実装でも反撃時に attack_target が
/// 再帰呼び出しされる場合は同様の挙動になる。
pub fn fire_use_event_labels(app: &mut App, idx: usize, device: &str) {
    let Some(u) = app.database().unit_instances.get(idx) else {
        return;
    };
    let pilot_name = u.pilot_name.clone();
    let unit_data_name = u.unit_data_name.clone();
    let party = u.party;
    fire_unit_event_labels(
        app,
        &["使用"],
        &pilot_name,
        &unit_data_name,
        party,
        Some(device),
    );
}

/// SRC `使用後 <unit> <device>:` (`使用後イベント.md`) の auto-fire。
/// `攻撃後イベント` 発火直前に呼ぶ。
///
/// 仕様準拠: 使用したユニットが生存している場合にのみ発火する。`UnitEventId`
/// で attacker を退避してから呼ぶ前提 (撃破された attacker は `unit_instances`
/// から除去されており、生存判定で false になる)。
pub fn fire_after_use_event_labels(app: &mut App, atk: &UnitEventId, device: &str) {
    let alive = app
        .database()
        .unit_instances
        .iter()
        .any(|u| !u.off_map && (u.pilot_name == atk.pilot || u.unit_data_name == atk.unit));
    if !alive {
        return;
    }
    fire_unit_event_labels(
        app,
        &["使用後"],
        &atk.pilot,
        &atk.unit,
        atk.party,
        Some(device),
    );
}

/// SRC `攻撃 <atk> <def>:` (`攻撃イベント.md`) の auto-fire。
/// `attack_target` 直前で呼び出す。両ユニットがマップ上に居る前提。
pub fn fire_attack_event_labels(app: &mut App, atk_idx: usize, def_idx: usize) {
    let atk = match app.database().unit_instances.get(atk_idx) {
        Some(u) => UnitEventId::from_unit_instance(u),
        None => return,
    };
    let def = match app.database().unit_instances.get(def_idx) {
        Some(u) => UnitEventId::from_unit_instance(u),
        None => return,
    };
    fire_pair_event_labels(app, &["攻撃"], &atk, &def);
}

/// SRC `攻撃後 <atk> <def>:` (`攻撃後イベント.md`) の auto-fire。
/// `attack_target` の damage / Destruction / 反撃 全完了後に呼び出す。
/// 両ユニットがマップ上に **生存** している場合のみ発火する (撃破された
/// 側が片方でも居なくなった場合は no-op)。
///
/// 呼び出し側は damage 適用前に `UnitEventId` を退避してから渡すこと。
/// 撃破で `unit_instances` から抜けたユニットは生存判定で false になり、
/// 発火されない。
pub fn fire_after_attack_event_labels(app: &mut App, atk: &UnitEventId, def: &UnitEventId) {
    let alive = |pilot: &str, unit: &str| {
        app.database()
            .unit_instances
            .iter()
            .any(|u| !u.off_map && (u.pilot_name == pilot || u.unit_data_name == unit))
    };
    if !alive(&atk.pilot, &atk.unit) || !alive(&def.pilot, &def.unit) {
        return;
    }
    fire_pair_event_labels(app, &["攻撃後"], atk, def);
}

/// SRC `進入 <unit> <x> <y>:` または `進入 <unit> <terrain>:`
/// (`進入イベント.md`) の auto-fire。
///
/// 移動完了時に呼び出す。`(x, y)` は本実装の 0-based 座標 (`X()`/`Y()` 関数
/// と同じ規約)。SRC 原典は 1-based だが、本実装は内部一貫性を優先する。
/// 引き続き `脱出 <unit> <direction>` をマップ端到達時にチェック・発火する
/// (原典: "進入イベントに続いて脱出イベントが発生")。
///
/// `Move` 命令経由のスクリプト移動では発火しない (原典準拠)。本関数は UI
/// 経路 (`app.rs::try_move_unit_to` 等) からのみ呼び出すこと。
pub fn fire_entry_event_labels(app: &mut App, idx: usize) {
    let Some(u) = app.database().unit_instances.get(idx) else {
        return;
    };
    let pilot_name = u.pilot_name.clone();
    let unit_data_name = u.unit_data_name.clone();
    let party = u.party;
    let x = u.x;
    let y = u.y;
    // (1) 進入 <unit> <x> <y>
    let coord_suffix = format!("{x} {y}");
    fire_unit_event_labels(
        app,
        &["進入"],
        &pilot_name,
        &unit_data_name,
        party,
        Some(&coord_suffix),
    );
    // (2) 進入 <unit> <terrain_name>
    let terrain_name = {
        let map = app.database().map.as_ref();
        let terrain_id = map.map(|m| m.cell(x, y).terrain_id);
        terrain_id.and_then(|tid| {
            app.database()
                .terrains
                .iter()
                .find(|t| t.id == tid)
                .map(|t| t.name.clone())
                .or_else(|| crate::data::terrain::lookup(tid).map(|t| t.name.to_string()))
        })
    };
    if let Some(name) = terrain_name {
        if !name.is_empty() {
            fire_unit_event_labels(
                app,
                &["進入"],
                &pilot_name,
                &unit_data_name,
                party,
                Some(&name),
            );
        }
    }
    // (3) 脱出 <unit> <direction> がマップ端到達時に発火
    let map_size = app
        .database()
        .map
        .as_ref()
        .map(|m| (m.width, m.height))
        .unwrap_or((0, 0));
    let mut dirs: Vec<&str> = Vec::new();
    if map_size.0 > 0 && map_size.1 > 0 {
        if y == 0 {
            dirs.push("N");
        }
        if y + 1 == map_size.1 {
            dirs.push("S");
        }
        if x == 0 {
            dirs.push("W");
        }
        if x + 1 == map_size.0 {
            dirs.push("E");
        }
    }
    for dir in dirs {
        fire_unit_event_labels(
            app,
            &["脱出"],
            &pilot_name,
            &unit_data_name,
            party,
            Some(dir),
        );
    }
}

/// SRC `接触 <unit1> <unit2>:` (`接触イベント.md`) の auto-fire。
/// 行動終了直後に該当ユニットの 4 近傍ユニットと組み合わせて発火する。
///
/// 各 (idx, neighbor) ペアで `攻撃` と同じ 3x3 識別子マッチ
/// (pilot × unit × party) を試行。最初にヒットしたものを 1 度発火する。
/// SRC 原典: "行動終了後に発生"。
pub fn fire_contact_event_labels(app: &mut App, idx: usize) {
    let (x, y, atk) = match app.database().unit_instances.get(idx) {
        Some(u) => (u.x, u.y, UnitEventId::from_unit_instance(u)),
        None => return,
    };
    let neighbors: Vec<(u32, u32)> = {
        let dx_dy: &[(i32, i32)] = &[(0, -1), (0, 1), (-1, 0), (1, 0)];
        dx_dy
            .iter()
            .filter_map(|(dx, dy)| {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 {
                    return None;
                }
                Some((nx as u32, ny as u32))
            })
            .collect()
    };
    let neighbor_ids: Vec<UnitEventId> = neighbors
        .iter()
        .flat_map(|(nx, ny)| {
            app.database()
                .unit_instances
                .iter()
                .filter(move |u| u.x == *nx && u.y == *ny && !u.off_map)
                .map(UnitEventId::from_unit_instance)
                .collect::<Vec<_>>()
        })
        .collect();
    for def in neighbor_ids {
        fire_pair_event_labels(app, &["接触"], &atk, &def);
    }
}

/// SRC `行動終了 <unit>:` (`行動終了イベント.md`) の auto-fire。
///
/// `has_acted = true` をセットした直後に App から呼び出す。`idx` は
/// `unit_instances` 内のインデックス。pilot 名 / ユニット名 / 陣営名 の
/// いずれか先頭マッチで 1 度だけ発火する。
///
/// 注意: `has_acted = true` をセットしてからこの関数を呼ぶこと。スクリプト
/// 側から `Count()` 等で「行動終了済」を参照する場合の整合性を保つため。
pub fn fire_action_end_labels(app: &mut App, idx: usize) {
    let Some(u) = app.database().unit_instances.get(idx) else {
        return;
    };
    let pilot_name = u.pilot_name.clone();
    let unit_data_name = u.unit_data_name.clone();
    let party = u.party;
    fire_unit_event_labels(
        app,
        &["行動終了"],
        &pilot_name,
        &unit_data_name,
        party,
        None,
    );
}

/// `fire_unit_event_labels` の public wrapper。`App::fire_boarding_event` 等
/// app.rs から呼べるよう公開している。戻り値は発火したかどうか。
pub fn fire_unit_event_labels_public(
    app: &mut App,
    prefixes: &[&str],
    pilot_name: &str,
    unit_data_name: &str,
    party: crate::Party,
) -> bool {
    // `fire_unit_event_labels` は戻り値を返さないので、post_event_label を
    // 直接呼ぶ模倣 (3 prefix × 3 identifier の優先順)。
    let party_label = party_long_label(party);
    for prefix in prefixes {
        for target in [pilot_name, unit_data_name, party_label] {
            if target.is_empty() {
                continue;
            }
            let label = format!("{prefix} {target}");
            // 収納 / 変形 等のユニットイベントは章ローカル。
            if app.post_stage_event_label(label) {
                return true;
            }
        }
    }
    false
}

/// 1 ユニットに紐づく auto-fire ラベルを `prefix [suffix]` 形式で順に試して
/// 最初にヒットしたものを発火する。`prefix` は `["変形", "Transform"]` のように
/// 日英両綴。`identifier` 候補は (pilot_name, unit_data_name, party_label) を
/// 順に試す。
///
/// SRC `Event.bas::HandleEvent` 順に揃えており、最初の matching label だけ
/// 発火する (原典は同 prefix 複数 label の同時発火を行わない)。
fn fire_unit_event_labels(
    app: &mut App,
    prefixes: &[&str],
    pilot_name: &str,
    unit_data_name: &str,
    party: crate::Party,
    suffix: Option<&str>,
) {
    let party_label = party_long_label(party);
    for prefix in prefixes {
        for target in [pilot_name, unit_data_name, party_label] {
            if target.is_empty() {
                continue;
            }
            let label = match suffix {
                Some(s) => format!("{prefix} {target} {s}"),
                None => format!("{prefix} {target}"),
            };
            // ユニットイベント (会話 / 損傷 / 収納 等) は章ローカル。
            if app.post_stage_event_label(label) {
                return;
            }
        }
    }
}

/// SRC `損傷率 <unit> <pct>:` (`損傷率イベント.md`) の auto-fire。
///
/// `apply_damage` 等で HP が減少し、損傷率 (= damage * 100 / max_hp) が
/// 既存値から threshold 以上に跨いだ場合に発火する。`unit` 部は
/// メインパイロット名 / ユニット名 / 陣営名 のいずれにもマッチ。
/// 複数閾値を同時に跨いだ場合は全て発火する。
///
/// 仕様準拠ポイント:
/// - 破壊された場合は発火しない (本関数は `unit_instances` 内に target が
///   残存する前提で呼び出す)
/// - 同一ラベルが pilot/unit/party 経由で複数回マッチしても 1 度のみ発火
///
/// 仕様乖離ポイント:
/// - SRC.NET は `Attack`/`MapAttack` 等のシナリオ戦闘では発火させないが、
///   本実装は damage source を区別せず、HP が減れば一律発火する。シナリオ
///   作成時の用途 (HP 50% で台詞) を優先するための簡略化。
fn fire_damage_threshold_labels(
    app: &mut App,
    pilot_name: &str,
    unit_data_name: &str,
    party: crate::Party,
    old_dmg: i64,
    new_dmg: i64,
    max_hp: i64,
) {
    if max_hp <= 0 || new_dmg <= old_dmg {
        return;
    }
    let party_label = party_long_label(party);
    let identifiers: Vec<&str> = [pilot_name, unit_data_name, party_label]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    let candidates: Vec<(String, i64)> = app
        .script_library()
        .labels
        .keys()
        .filter_map(|k| {
            let suffix = k.strip_prefix("損傷率 ")?;
            // suffix は "<name> <pct>" 形式。pct は末尾トークン。
            let last_space = suffix.rfind(' ')?;
            let name = suffix[..last_space].trim();
            let pct_str = suffix[last_space + 1..].trim();
            let pct: i64 = pct_str.parse().ok()?;
            if !identifiers.contains(&name) {
                return None;
            }
            Some((k.clone(), pct))
        })
        .collect();
    // 重複 (同じ label が pilot / unit_data の両方でマッチした等) は
    // HashMap キー同一なので filter_map 段で既に de-dup 済み。
    // 閾値順 (小さい順) に発火する: 50→30→10 と複数跨いだ場合の発生順を SRC.NET
    // と揃える。
    let mut hit: Vec<(String, i64)> = candidates
        .into_iter()
        .filter(|(_, pct)| {
            let scaled = pct.saturating_mul(max_hp);
            let old_scaled = old_dmg.saturating_mul(100);
            let new_scaled = new_dmg.saturating_mul(100);
            old_scaled < scaled && new_scaled >= scaled
        })
        .collect();
    hit.sort_by_key(|(_, pct)| *pct);
    for (label, _) in hit {
        // 損傷率 イベントは章ローカル。
        app.post_stage_event_label(label);
    }
}

/// `Menu prompt` 後続行を `End` まで収集して選択肢列を返す。
/// 空行・コメント行は読み飛ばし。
fn collect_menu_options(app: &App, start: usize, stmts: &[EventStatement]) -> (Vec<String>, usize) {
    let mut options = Vec::new();
    let mut i = start;
    while i < stmts.len() {
        if let EventStatement::Command { name, args, .. } = &stmts[i] {
            if name.eq_ignore_ascii_case("end") {
                return (options, i + 1);
            }
            // 1 行を 1 選択肢として連結。`$(var)` / 関数呼出 / `name[expr]` は
            // expand_vars で実値に展開する (元 SRC は Ask 選択肢を式評価する)。
            let mut s = expand_vars(app, name);
            for a in args {
                s.push(' ');
                s.push_str(&expand_vars(app, a));
            }
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
fn cond_term_len(xargs: &[String], k: usize) -> usize {
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
fn split_if_cond_body(xargs: &[String]) -> (Vec<String>, Vec<String>) {
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
fn if_opens_block(args: &[String]) -> bool {
    split_if_cond_body(args).1.is_empty()
}

fn is_comparison_op(s: &str) -> bool {
    matches!(s, "=" | "==" | "<>" | "!=" | "<" | "<=" | ">" | ">=")
        || s.eq_ignore_ascii_case("like")
}

/// VB6 `Like` 演算子のパターンマッチ。
///
/// パターン特殊文字:
/// - `*`  任意の 0 文字以上にマッチ
/// - `?`  任意の 1 文字にマッチ
/// - `#`  任意の 1 桁数字にマッチ
/// - `[charlist]`  charlist 中の 1 文字にマッチ（`A-Z` 範囲記法対応）
/// - `[!charlist]` charlist に含まれない 1 文字にマッチ
/// - その他 → リテラル比較（大文字小文字区別あり）
fn like_match(s: &str, pattern: &str) -> bool {
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
fn like_char_class_match(c: char, class: &[char]) -> bool {
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

fn eval_binop(lhs: &str, op: &str, rhs: &str) -> bool {
    // SRC の比較規則: 両辺とも数値なら数値比較、両辺とも非数値なら文字列
    // 比較。**一方だけが数値**なら数値比較とし、非数値側 (未設定変数 →
    // その名前、空文字、`?` 等) は 0 とみなす。これにより
    // `If 未設定カウンタ <> 0` が `0 <> 0` = 偽になる (SRC 準拠)。
    let (mut lhs_n, mut rhs_n) = (lhs.parse::<f64>().ok(), rhs.parse::<f64>().ok());
    if lhs_n.is_none() && rhs_n.is_some() {
        lhs_n = Some(0.0);
    } else if rhs_n.is_none() && lhs_n.is_some() {
        rhs_n = Some(0.0);
    }
    match op {
        "=" | "==" => match (lhs_n, rhs_n) {
            (Some(a), Some(b)) => (a - b).abs() < f64::EPSILON,
            _ => lhs == rhs,
        },
        "<>" | "!=" => match (lhs_n, rhs_n) {
            (Some(a), Some(b)) => (a - b).abs() >= f64::EPSILON,
            _ => lhs != rhs,
        },
        "<" => match (lhs_n, rhs_n) {
            (Some(a), Some(b)) => a < b,
            _ => lhs < rhs,
        },
        "<=" => match (lhs_n, rhs_n) {
            (Some(a), Some(b)) => a <= b,
            _ => lhs <= rhs,
        },
        ">" => match (lhs_n, rhs_n) {
            (Some(a), Some(b)) => a > b,
            _ => lhs > rhs,
        },
        ">=" => match (lhs_n, rhs_n) {
            (Some(a), Some(b)) => a >= b,
            _ => lhs >= rhs,
        },
        op if op.eq_ignore_ascii_case("like") => like_match(lhs, rhs),
        _ => false,
    }
}

/// `If` 偽分岐: 同レベルの `ElseIf` / `Else` / `EndIf` まで読み飛ばす。
///
/// - 一致する `EndIf` → その次の PC を返す（If 文を抜ける）
/// - 同レベル `Else` → その次の PC を返す（Else 節を実行）
/// - 同レベル `ElseIf` → 条件評価し、真ならその次の PC、偽なら更に次の分岐へ
fn skip_to_else_or_endif(
    app: &App,
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
                "elseif" if depth == 0 => {
                    // 変数展開 + `&` 連結 → 条件評価
                    let expanded: Vec<String> = args.iter().map(|a| expand_arg(app, a)).collect();
                    let xargs = collapse_concat(expanded);
                    if eval_condition_args_with(app, &xargs) {
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
fn skip_to_endif(pc: usize, stmts: &[EventStatement], line: usize) -> Result<usize, ScriptError> {
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
fn collect_until_end(app: &App, start: usize, stmts: &[EventStatement]) -> (String, usize) {
    let mut body = String::new();
    let mut i = start;
    while i < stmts.len() {
        if let EventStatement::Command { name, args, .. } = &stmts[i] {
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
            body.push_str(&expand_vars(app, name));
            for a in args {
                body.push(' ');
                body.push_str(&expand_vars(app, a));
            }
        }
        i += 1;
    }
    (body.trim().to_string(), i)
}

/// SRC `Talk` ボディに含まれる HTML 書式タグを除去・変換する。
///
/// SRC は Talk メッセージ中に VB6 Rich TextBox 用の簡易タグを許容する:
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

/// ラベルへジャンプ。`@anchor` / `label:` どちらも `collect_labels` で正規化済み。
/// `target` ラベルを **現 PC を含むファイル内優先** で解決して飛び先 PC を返す。
fn jump_to(app: &App, pc: usize, target: &str, line: usize) -> Result<usize, ScriptError> {
    // 末尾 `:` / 先頭 `@` を取り除いた名前で検索
    let t = target.trim_start_matches('@').trim_end_matches(':');
    app.script_library()
        .label_pc_scoped(pc, t)
        .ok_or_else(|| err(line, &format!("Goto 先ラベルが見つかりません: {target:?}")))
}

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
fn expand_vars(app: &App, src: &str) -> String {
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
fn take_indexed_var(app: &App, src: &str, i: usize) -> Option<(String, usize)> {
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
fn resolve_lhs_name(app: &App, raw: &str) -> String {
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
fn try_system_var_assign(app: &mut App, lhs: &str, value: &str) -> bool {
    let n_i64 = || -> i64 {
        value
            .parse::<i64>()
            .ok()
            .or_else(|| value.parse::<f64>().ok().map(|f| f.trunc() as i64))
            .or_else(|| try_eval_int(value))
            .unwrap_or(0)
    };
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

fn try_function_lhs_assign(app: &mut App, lhs: &str, value: &str) -> bool {
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

    // 数値変換: 非数値は 0 扱い (SRC.Sharp VB6 `Val()` 相当)。
    let n: i64 = value
        .parse()
        .unwrap_or_else(|_| try_eval_int(value).unwrap_or(0));

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
fn resolve_index_part(app: &App, part: &str) -> String {
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
    if let Some(n) = try_eval_int(&val) {
        return n.to_string();
    }
    // 算術式の添字 (`((Args(2) - 1) * 4 + i)` / `j - 1` 等) は、`expand_vars` が
    // `Args()` や `$(...)` は解決しても、`i` / `j` のような **裸の算術変数**を
    // 残すため `try_eval_int` が失敗する。演算子を含む場合に限り
    // `resolve_expr_atoms` で裸変数アトムを数値へ解決してから再評価する。
    // (`アイテム数` / `ページ数` 等の文字列添字キーは演算子を含まないので
    // この経路を通らず、リテラルキーのまま保持される。`" - "` は算術の
    // マイナスを表し、`R-1` のようなハイフン付きキーと区別する。)
    let looks_arith = val.contains(['(', ')', '+', '*', '/', '\\', '^']) || val.contains(" - ");
    if looks_arith {
        if let Some(n) = try_eval_int(&resolve_expr_atoms(app, &val)) {
            return n.to_string();
        }
    }
    val.trim().to_string()
}

/// 1 引数トークンを展開する。`expand_vars` (`$(var)` / 関数呼出 /
/// インデックス変数の展開) に加え、トークン全体が `(...)` で括られその
/// 内側にトップレベルの `&` 連結を含む場合は、カッコを外して連結を畳む。
///
/// 元 SRC の引数トークナイザは `(...)` を 1 トークンとして保持するため、
/// `("u_" & Left("x",2))` のような括弧付き `&` 連結は token 単位で動く
/// `collapse_concat` には捕まらない。ここで token 内部の `&` を処理する。
///
/// 算術式 `(200 - 128 / 2)` は `&` を含まないので連結扱いされず、従来通り
/// `expand_vars` をそのまま通って数値評価パスに委ねられる。
fn expand_arg(app: &App, a: &str) -> String {
    if let Some(inner) = paren_wrapped_concat(a) {
        let mut result = String::new();
        for part in split_top_level_concat(inner) {
            let p = part.trim();
            if p.len() >= 2 && p.starts_with('"') && p.ends_with('"') {
                // 純粋な `"..."` リテラルはクオートを剥がして中身を
                // `expand_vars` で展開する (非括弧形 tokenizer と同じ挙動)。
                // クオート内はリテラル扱いなので連結再帰はしない。
                result.push_str(&expand_vars(app, &p[1..p.len() - 1]));
            } else if paren_wrapped_concat(p).is_some() {
                // ネスト括弧連結は再帰で処理。
                result.push_str(&expand_arg(app, p));
            } else {
                // 非クオートの裸オペランド (変数名 / 算術式 / 関数呼出など)。
                // `fn_arg_value` で変数解決・システム変数展開・indexed 参照を
                // 行う。`("DL_GUI_" & DL_ShowGUI)` 中の `DL_ShowGUI` のような
                // ベア識別子が変数として正しく解決される。
                // 算術式が残る場合は `expand_vars` でさらに `$(...)` を展開する。
                let resolved = fn_arg_value(app, p);
                result.push_str(&expand_vars(app, &resolved));
            }
        }
        return result;
    }
    expand_vars(app, a)
}

/// トークン全体が単一の `(...)` グループで、内側にトップレベル
/// (クオート外・ネスト括弧外) の `&` を含むなら、その内側を返す。
/// `(a) & (b)` のように複数グループに分かれる場合や `&` を含まない
/// 算術式は `None`。
fn paren_wrapped_concat(a: &str) -> Option<&str> {
    let t = a.trim();
    let bytes = t.as_bytes();
    if bytes.first() != Some(&b'(') || bytes.last() != Some(&b')') {
        return None;
    }
    let mut depth = 0i32;
    let mut in_quote = false;
    let mut has_concat = false;
    for (idx, &b) in bytes.iter().enumerate() {
        if b == b'"' {
            in_quote = !in_quote;
            continue;
        }
        if in_quote {
            continue;
        }
        match b {
            b'(' => depth += 1,
            b')' => {
                depth -= 1;
                // 先頭 `(` に対応する `)` が末尾以外で現れたら、全体は
                // 1 つの括弧グループではない。
                if depth == 0 && idx != bytes.len() - 1 {
                    return None;
                }
            }
            b'&' if depth == 1 => has_concat = true,
            _ => {}
        }
    }
    if depth != 0 || in_quote || !has_concat {
        return None;
    }
    Some(&t[1..t.len() - 1])
}

/// `inner` をトップレベル (クオート外・括弧外) の `&` で分割する。
fn split_top_level_concat(inner: &str) -> Vec<&str> {
    let bytes = inner.as_bytes();
    let mut parts = Vec::new();
    let mut depth = 0i32;
    let mut in_quote = false;
    let mut start = 0;
    for (idx, &b) in bytes.iter().enumerate() {
        match b {
            b'"' => in_quote = !in_quote,
            b'(' if !in_quote => depth += 1,
            b')' if !in_quote => depth -= 1,
            b'&' if !in_quote && depth == 0 => {
                parts.push(&inner[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    parts.push(&inner[start..]);
    parts
}

/// `&` 連結演算子の畳み込み。引数列で `"&"` トークンを見つけたら前後を
/// 空白なしで連結する。`["abc", "&", "xyz"]` → `["abcxyz"]`。
/// 元 SRC は VB6 風の文字列連結 `a & b` を使うため。
fn collapse_concat(args: Vec<String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        if args[i] == "&" && !out.is_empty() && i + 1 < args.len() {
            let last = out.pop().unwrap();
            let next = args[i + 1].clone();
            out.push(format!("{last}{next}"));
            i += 2;
        } else {
            out.push(args[i].clone());
            i += 1;
        }
    }
    out
}

/// インデックス変数の識別子に含めて良い文字か。
/// 区切り文字以外なら true。
fn is_indexed_ident_char(c: char) -> bool {
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

/// `s` を整数式として評価できれば値を返す。式パーサが何も食わなければ None。
fn try_eval_int(s: &str) -> Option<i64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(v) = t.parse::<i64>() {
        return Some(v);
    }
    let tokens = tokenize_expr(t);
    if tokens.is_empty() {
        return None;
    }
    let mut idx = 0;
    let v = parse_logical(&tokens, &mut idx)?;
    if idx == tokens.len() {
        // SRC.Sharp `(int)value` は切り捨て (ゼロ方向)。`.round()` ではなく
        // `.trunc()` を使う: `3.9 → 3`, `-3.9 → -3`。
        Some(v.trunc() as i64)
    } else {
        None
    }
}

/// `try_eval_int` の float 版。`5 / 2` のように小数を含む式でも正確な
/// 値を返すため、indexed lookup 以外の汎用算術評価で使う。
///
/// SRC.Sharp `Expression.GetValueAsDouble` 相当: 整数演算でも float
/// として返す (caller が整数表現を希望する場合は `format_num` 経由で
/// `5` と `5.0` を区別なくフォーマットする)。
fn try_eval_num(s: &str) -> Option<f64> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Ok(v) = t.parse::<f64>() {
        return Some(v);
    }
    let tokens = tokenize_expr(t);
    if tokens.is_empty() {
        return None;
    }
    let mut idx = 0;
    let v = parse_logical(&tokens, &mut idx)?;
    if idx == tokens.len() {
        Some(v)
    } else {
        None
    }
}

/// `src[i..]` の先頭が ASCII 英字で始まる関数呼出 `Name(args)` か判定。
/// 戻り値: `(fn_name, args_inside_parens, consumed_byte_count)`。
/// `src[open..]` の位置にある `(` に対応する `)` の index を返す。
/// `open` の位置は `(` でなければならない。`(` / `)` の数が合わなければ `None`。
fn find_matching_paren(src: &str, open: usize) -> Option<usize> {
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

fn take_function_call(src: &str, i: usize) -> Option<(&str, &str, usize)> {
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
fn expand_dollar_paren(app: &App, name_raw: &str) -> String {
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

/// 関数呼び出しを評価して文字列値を返す。対応外なら `None`。
/// 名前は ASCII 範囲で case-insensitive (SRC 流: `Llength` / `LLength` どちらも可)。
fn eval_script_function(app: &App, name: &str, args_str: &str) -> Option<String> {
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
            // VB6 では String(N, char) で char 単一文字だが、SRC は `String(3, "0 ")`
            // のように任意文字列も渡される (スパロボ戦記 で 6 回使用)。
            if args.len() < 2 {
                return None;
            }
            let n: usize = numeric_arg(app, args[0]).map(|v| v.max(0.0) as usize)?;
            let s = fn_arg_value(app, args[1]);
            Some(s.repeat(n))
        }
        "Wide" => {
            // `Wide(s)` — 半角文字を全角に変換。VB6 の StrConv(s, vbWide) 相当。
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
            // VB6 の Trim$ は全角空白 (U+3000) は除去しない。
            let s = fn_arg_value(app, args.first()?);
            Some(s.trim_matches(' ').to_string())
        }
        "Asc" => {
            // `Asc(s)` — VB6 互換: SJIS (CP932) 経由で文字コードを返す。
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
            // `Chr(n)` — VB6 互換: SJIS (CP932) コードを解釈して 1 文字を返す。
            // - 0..=0xFF: 単バイト SJIS (ASCII / 半角カナ / 制御コード)
            // - 0x100..: 2 バイト SJIS `high << 8 | low` として decode (例: 0x82A0 → "あ")
            // SRC.Sharp の Chr は `(char)long` で Unicode コードポイント扱い (XXX
            // 文字コード という FIXME 付き) だが、こちらは VB6 寄りで実装する。
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
            // VB6 の銀行丸めとも Rust の round (ゼロから遠ざける) とも異なり、
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
            // `Log(x)` — 自然対数 (VB6 `Log` 同等、SRC 準拠)。x <= 0 は None。
            let v = numeric_arg(app, args.first()?)?;
            if v <= 0.0 {
                return None;
            }
            Some(format_num(v.ln()))
        }
        "Sgn" => {
            // `Sgn(x)` — 符号関数: x > 0 → 1, x < 0 → -1, x == 0 → 0。
            // VB6 `Sgn` 同等 (`Math.Sign`)。
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
            // `Mod(a, b)` — 整数剰余。`a Mod b` 演算子も VB6/SRC で存在するが、
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
            // VB6 `Mod` は整数演算: a, b を整数に切り捨ててから剰余。
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
            // それ以外の VB6 format 構文 ($, E+ 等) はサポートしない。
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
                // C# LevelUpCmd.cs: レベル上限は 99。
                let level = ((inst.total_exp / 100).max(0) + 1).min(99);
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
fn split_function_args(s: &str) -> Vec<&str> {
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

/// 元 SRC の `Info([データ区分,]データ,情報種類,…)` を解釈する。
/// 引数文字列は既に `expand_vars` で展開済 (ネスト関数も解決済)。
///
/// 未対応の組合せは空文字列を返す（VB6 SRC では type error になるが、
/// シナリオ既存コードは Info() が空を返す前提でフォールバックを書いている
/// ことが多いので互換性優先）。
fn info_query(app: &App, args: &[&str]) -> String {
    if args.is_empty() {
        return String::new();
    }
    // 第 1 引数がデータ区分キーワードか判定。クォートは剥がして比較。
    // `name` 引数は裸の識別子 (`Info(ユニット,乗せ換えユニット,愛称)` 等) を
    // 取り得るので `fn_arg_value` で script_var 解決する。
    let head = args[0].trim().trim_matches('"');
    let (kind, name, rest): (InfoKind, String, &[&str]) = match info_data_kind(head) {
        Some(kind) => {
            // マップ / オプション は data 名を持たず、続く全引数を rest として扱う。
            if matches!(kind, InfoKind::Map | InfoKind::Option) {
                (kind, String::new(), &args[1..])
            } else {
                let name = fn_arg_value(app, args.get(1).copied().unwrap_or(""));
                (kind, name, &args[2..])
            }
        }
        None => {
            // データ区分省略: name から自動判定
            let name = fn_arg_value(app, head);
            let kind = detect_info_kind(app, &name);
            (kind, name, &args[1..])
        }
    };
    let info_kind = rest
        .first()
        .map(|s| s.trim().trim_matches('"').to_string())
        .unwrap_or_default();
    let sub = rest.get(1..).unwrap_or(&[]);
    info_dispatch(app, kind, &name, &info_kind, sub)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InfoKind {
    Unit,
    UnitData,
    Pilot,
    PilotData,
    NonPilot,
    Item,
    SpecialPower,
    Map,
    Option,
}

fn info_data_kind(s: &str) -> Option<InfoKind> {
    match s {
        "ユニット" => Some(InfoKind::Unit),
        "ユニットデータ" => Some(InfoKind::UnitData),
        "パイロット" => Some(InfoKind::Pilot),
        "パイロットデータ" => Some(InfoKind::PilotData),
        "非戦闘員" => Some(InfoKind::NonPilot),
        "アイテム" => Some(InfoKind::Item),
        "スペシャルパワー" => Some(InfoKind::SpecialPower),
        "マップ" => Some(InfoKind::Map),
        "オプション" => Some(InfoKind::Option),
        _ => None,
    }
}

fn detect_info_kind(app: &App, name: &str) -> InfoKind {
    let n = name.trim().trim_matches('"');
    if app
        .database()
        .unit_instances
        .iter()
        .any(|u| matches_unit_handle(u, n))
    {
        return InfoKind::Unit;
    }
    if app
        .database()
        .pilots
        .iter()
        .any(|p| p.name == n || p.nickname == n)
    {
        return InfoKind::PilotData;
    }
    if app.database().units.iter().any(|u| u.name == n) {
        return InfoKind::UnitData;
    }
    if app.database().items.iter().any(|i| i.name == n) {
        return InfoKind::Item;
    }
    InfoKind::UnitData
}

fn info_dispatch(app: &App, kind: InfoKind, name: &str, info: &str, sub: &[&str]) -> String {
    match kind {
        InfoKind::Unit => info_unit(app, name, info, sub),
        InfoKind::UnitData => info_unit_data(app, name, info, sub),
        InfoKind::Pilot => info_pilot(app, name, info, sub, true),
        InfoKind::PilotData => info_pilot(app, name, info, sub, false),
        InfoKind::Item => info_item(app, name, info, sub),
        InfoKind::Map => info_map(app, info, sub),
        InfoKind::Option => {
            // `Info(オプション, name)` — Option コマンドで設定された値を返す。
            // Option(name) が設定済みなら "On"、未設定なら "Off"。
            // SRC.Sharp `Other.cs::InfoOption` 相当。
            let opt_name = if info.is_empty() { name } else { info };
            let key = format!("Option({opt_name})");
            if app.script_var(&key).is_empty() {
                "Off".to_string()
            } else {
                "On".to_string()
            }
        }
        InfoKind::SpecialPower => info_special_power(app, name, info),
        // 非戦闘員は未実装: 空文字
        InfoKind::NonPilot => String::new(),
    }
}

/// `Info(ユニット, name, ...)` — UnitInstance + 装備込み補正値を優先。
fn info_unit(app: &App, name: &str, info: &str, sub: &[&str]) -> String {
    let inst = match app
        .database()
        .unit_instances
        .iter()
        .find(|u| matches_unit_handle(u, name))
    {
        Some(u) => u,
        None => return String::new(),
    };
    let data = match app.database().unit_by_name(&inst.unit_data_name) {
        Some(d) => d,
        None => return String::new(),
    };
    match info {
        "ＨＰ" | "HP" => {
            let max = app.database().effective_max_hp(inst);
            (max - inst.damage).max(0).to_string()
        }
        "最大ＨＰ" | "MaxHP" => app.database().effective_max_hp(inst).to_string(),
        "ＥＮ" | "EN" => {
            let max = app.database().effective_max_en(inst);
            (max - inst.en_consumed).max(0).to_string()
        }
        "最大ＥＮ" | "MaxEN" => app.database().effective_max_en(inst).to_string(),
        "装甲" => app.database().effective_armor(inst).to_string(),
        "運動性" => app.database().effective_mobility(inst).to_string(),
        "移動力" => app.database().effective_speed(inst).to_string(),
        "気力" => inst.morale.to_string(),
        "累積経験値" => inst.total_exp.to_string(),
        "経験値" => inst.total_exp.to_string(),
        "アイテム数" => inst.equipped_item_names().len().to_string(),
        "アイテム" => {
            let names = inst.equipped_item_names();
            sub.first()
                .and_then(|s| fn_arg_value(app, s).parse::<usize>().ok())
                .and_then(|n| names.get(n.saturating_sub(1)))
                .map(|s| s.to_string())
                .unwrap_or_default()
        }
        // UnitData 由来の参照は base のヘルパに委譲
        _ => info_unit_data_inner(data, info, sub, app),
    }
}

/// `Info(ユニットデータ, name, ...)` — UnitData の固定値のみ。
fn info_unit_data(app: &App, name: &str, info: &str, sub: &[&str]) -> String {
    let n = name.trim().trim_matches('"');
    // 仮想 instance を経由した名前指定にも対応 (Args(1) が unit_data_name の場合)
    let data = app.database().unit_by_name(n).or_else(|| {
        app.database()
            .unit_instances
            .iter()
            .find(|u| matches_unit_handle(u, n))
            .and_then(|u| app.database().unit_by_name(&u.unit_data_name))
    });
    let Some(data) = data else {
        return String::new();
    };
    info_unit_data_inner(data, info, sub, app)
}

fn info_unit_data_inner(
    data: &crate::data::unit::UnitData,
    info: &str,
    sub: &[&str],
    app: &App,
) -> String {
    match info {
        "名称" => data.name.clone(),
        "愛称" => data.nickname.clone(),
        "読み仮名" => data.kana_name.clone(),
        "ユニットクラス" | "クラス" => data.class.clone(),
        "規定パイロット数" => data.pilot_num.to_string(),
        "最大アイテム数" => data.item_num.to_string(),
        "移動可能地形" => data.transportation.clone(),
        "移動力" => data.speed.to_string(),
        "サイズ" => data.size.label().to_string(),
        "修理費" => data.value.to_string(),
        "経験値" => data.exp_value.to_string(),
        "最大ＨＰ" | "ＨＰ" | "MaxHP" | "HP" => data.hp.to_string(),
        "最大ＥＮ" | "ＥＮ" | "MaxEN" | "EN" => data.en.to_string(),
        "装甲" => data.armor.to_string(),
        "運動性" => data.mobility.to_string(),
        "地形適応" => data.adaption.as_str().to_string(),
        "グラフィック" => data.bitmap.clone(),
        "武器数" => data.weapons.len().to_string(),
        "武器" => weapon_info(data, sub, app),
        "特殊能力数" => data.features.len().to_string(),
        "特殊能力" => feature_at(&data.features, sub.first(), app),
        "特殊能力名称" => feature_name(&data.features, sub.first(), app),
        "特殊能力所有" => feature_owned(&data.features, sub.first(), app),
        "特殊能力レベル" => feature_level(&data.features, sub.first(), app),
        "特殊能力データ" | "特殊能力解説" => {
            feature_data(&data.features, sub.first(), app)
        }
        "最大攻撃力" => data
            .weapons
            .iter()
            .map(|w| w.power)
            .max()
            .unwrap_or(0)
            .to_string(),
        "最長射程" => data
            .weapons
            .iter()
            .map(|w| w.max_range)
            .max()
            .unwrap_or(0)
            .to_string(),
        _ => String::new(),
    }
}

fn weapon_info(data: &crate::data::unit::UnitData, sub: &[&str], app: &App) -> String {
    let Some(first) = sub.first() else {
        return String::new();
    };
    let key = fn_arg_value(app, first);
    let weapon = if let Ok(idx) = key.parse::<usize>() {
        if idx == 0 || idx > data.weapons.len() {
            return String::new();
        }
        &data.weapons[idx - 1]
    } else {
        match data.weapons.iter().find(|w| w.name == key) {
            Some(w) => w,
            None => return String::new(),
        }
    };
    let Some(attr) = sub.get(1) else {
        return weapon.name.clone();
    };
    let attr = attr.trim().trim_matches('"');
    match attr {
        "名称" => weapon.name.clone(),
        "攻撃力" => weapon.power.to_string(),
        "最小射程" => weapon.min_range.to_string(),
        "最大射程" => weapon.max_range.to_string(),
        "命中率" | "命中" => weapon.precision.to_string(),
        "最大弾数" | "弾数" => weapon.bullet.to_string(),
        "消費ＥＮ" => weapon.en_consumption.to_string(),
        "必要気力" => weapon.necessary_morale.to_string(),
        "地形適応" => weapon.adaption.clone(),
        "クリティカル率" => weapon.critical.to_string(),
        "属性" => weapon.class.clone(),
        // 必要技能 → extras[0] / 必要条件 → extras[1] (生文字列を返す簡易対応。
        // 原典 Info(武器,…,必要技能) は満足判定の 1/0 を返すが、本ヘルパは静的 UnitData
        // のみでユニット実体が無いため文字列で代替する)。
        "必要技能" => weapon.necessary_skill().to_string(),
        "必要条件" => weapon.necessary_condition().to_string(),
        "使用可" | "修得" => "1".to_string(),
        _ => String::new(),
    }
}

fn feature_at(feats: &[(String, String)], sub: Option<&&str>, app: &App) -> String {
    let Some(idx) = sub.and_then(|s| fn_arg_value(app, s).parse::<usize>().ok()) else {
        return String::new();
    };
    if idx == 0 || idx > feats.len() {
        return String::new();
    }
    feats[idx - 1].0.clone()
}

fn feature_name(feats: &[(String, String)], sub: Option<&&str>, app: &App) -> String {
    // 番号指定 (n 番目の名称) も 名前指定 (= 名称そのもの) も同じ識別子を返す。
    let Some(arg) = sub else { return String::new() };
    let key = fn_arg_value(app, arg);
    if let Ok(idx) = key.parse::<usize>() {
        if idx == 0 || idx > feats.len() {
            return String::new();
        }
        return feats[idx - 1].0.clone();
    }
    if feats.iter().any(|(k, _)| k == &key) {
        return key;
    }
    String::new()
}

fn feature_owned(feats: &[(String, String)], sub: Option<&&str>, app: &App) -> String {
    let Some(arg) = sub else { return "0".into() };
    let key = fn_arg_value(app, arg);
    if feats.iter().any(|(k, _)| k == &key) {
        "1".into()
    } else {
        "0".into()
    }
}

fn feature_level(feats: &[(String, String)], sub: Option<&&str>, app: &App) -> String {
    let Some(arg) = sub else { return "0".into() };
    let key = fn_arg_value(app, arg);
    let entry = if let Ok(idx) = key.parse::<usize>() {
        feats.get(idx.saturating_sub(1))
    } else {
        feats.iter().find(|(k, _)| k == &key)
    };
    let Some((_, v)) = entry else {
        return "0".into();
    };
    // 値の末尾トークンがレベル数値ならそれを、無ければ 1 を返す。
    let last_num = v
        .split_whitespace()
        .rev()
        .find_map(|t| t.parse::<i32>().ok());
    last_num
        .map(|n| n.to_string())
        .unwrap_or_else(|| "1".into())
}

fn feature_data(feats: &[(String, String)], sub: Option<&&str>, app: &App) -> String {
    let Some(arg) = sub else { return String::new() };
    let key = fn_arg_value(app, arg);
    let entry = if let Ok(idx) = key.parse::<usize>() {
        feats.get(idx.saturating_sub(1))
    } else {
        feats.iter().find(|(k, _)| k == &key)
    };
    entry.map(|(_, v)| v.clone()).unwrap_or_default()
}

fn info_pilot(app: &App, name: &str, info: &str, sub: &[&str], is_instance: bool) -> String {
    let n = name.trim().trim_matches('"');
    // パイロットは PilotData として持つ。Instance は UnitInstance.pilot_name で
    // 紐付くので、必要に応じて両方を引く。
    // effective_pilot_data: PilotInstance が存在すればレベルアップ後スタットを返す。
    let effective = app.database().effective_pilot_data(n).or_else(|| {
        // nickname でも試みる
        let real_name = app
            .database()
            .pilots
            .iter()
            .find(|p| p.nickname == n)
            .map(|p| p.name.clone())?;
        app.database().effective_pilot_data(&real_name)
    });
    let Some(data) = effective else {
        return String::new();
    };
    // パイロット (Instance) 側の補正情報は士気・経験値が UnitInstance に乗る。
    let inst = if is_instance {
        app.database()
            .unit_instances
            .iter()
            .find(|u| u.pilot_name == data.name || u.pilot_name == data.nickname)
    } else {
        None
    };
    match info {
        "名称" => data.name.clone(),
        "愛称" => data.nickname.clone(),
        "読み仮名" => data.kana_name.clone(),
        "性別" => match data.sex {
            Sex::Male => "男性".to_string(),
            Sex::Female => "女性".to_string(),
            // SRC.Sharp 準拠: 性別未指定は空文字 (`"-"` ではない)。
            // InfoFunctionTests.Info_PilotData_Sex_EmptySex_ReturnsEmpty 参照。
            Sex::Unspecified => String::new(),
        },
        "クラス" | "ユニットクラス" => data.class.clone(),
        "地形適応" => data.adaption.as_str().to_string(),
        "経験値" => data.exp_value.to_string(),
        "格闘" | "格闘基本値" => data.infight.to_string(),
        "射撃" | "射撃基本値" => data.shooting.to_string(),
        "命中" | "命中基本値" => data.hit.to_string(),
        "回避" | "回避基本値" => data.dodge.to_string(),
        "技量" | "技量基本値" => data.technique.to_string(),
        "反応" | "反応基本値" => data.intuition.to_string(),
        // 修正値・支援修正値 系は未実装 (0 固定)
        "格闘修正値"
        | "射撃修正値"
        | "命中修正値"
        | "回避修正値"
        | "技量修正値"
        | "反応修正値"
        | "格闘支援修正値"
        | "射撃支援修正値"
        | "命中支援修正値"
        | "回避支援修正値"
        | "技量支援修正値"
        | "反応支援修正値" => "0".to_string(),
        "性格" => data.personality.clone().unwrap_or_default(),
        "最大ＳＰ" => data.sp.map(|n| n.to_string()).unwrap_or_default(),
        "ＳＰ" => {
            // インスタンスがあれば現在 SP (max - consumed) を、無ければ最大値を返す。
            let max = data.sp.unwrap_or(0);
            let consumed = inst.map(|u| u.sp_consumed).unwrap_or(0);
            ((max - consumed).max(0)).to_string()
        }
        "グラフィック" => data
            .bitmap
            .clone()
            .unwrap_or_else(|| format!("{}.bmp", data.nickname)),
        "ＭＩＤＩ" => data.bgm.clone().unwrap_or_default(),
        "気力" => inst
            .map(|u| u.morale.to_string())
            .unwrap_or_else(|| "100".into()),
        "累積経験値" => inst
            .map(|u| u.total_exp.to_string())
            .unwrap_or_else(|| "0".into()),
        "レベル" => {
            // 累積経験値 / 100 を簡易レベルとする (元 SRC 仕様の近似)。
            let total = inst.map(|u| u.total_exp).unwrap_or(0);
            (1 + total / 100).to_string()
        }
        "特殊能力数" => data.features.len().to_string(),
        "特殊能力" => feature_at(&data.features, sub.first(), app),
        "特殊能力名称" => feature_name(&data.features, sub.first(), app),
        "特殊能力所有" => feature_owned(&data.features, sub.first(), app),
        "特殊能力レベル" => feature_level(&data.features, sub.first(), app),
        "特殊能力データ" | "特殊能力解説" => {
            feature_data(&data.features, sub.first(), app)
        }
        _ => String::new(),
    }
}

fn info_item(app: &App, name: &str, info: &str, sub: &[&str]) -> String {
    let n = name.trim().trim_matches('"');
    let Some(item) = app.database().item_by_name(n) else {
        return String::new();
    };
    match info {
        "名称" => item.name.clone(),
        "アイテムクラス" => item.class.clone(),
        "装備個所" => item.part.clone(),
        "最大ＨＰ修正値" => item.hp_mod.to_string(),
        "最大ＥＮ修正値" => item.en_mod.to_string(),
        "装甲修正値" => item.armor_mod.to_string(),
        "運動性修正値" => item.mobility_mod.to_string(),
        "移動力修正値" => item.speed_mod.to_string(),
        "解説文" => item.comment.clone(),
        "特殊能力数" => item.features.len().to_string(),
        "特殊能力" => feature_at(&item.features, sub.first(), app),
        "特殊能力名称" => feature_name(&item.features, sub.first(), app),
        "特殊能力所有" => feature_owned(&item.features, sub.first(), app),
        "特殊能力レベル" => feature_level(&item.features, sub.first(), app),
        "特殊能力データ" | "特殊能力解説" => {
            feature_data(&item.features, sub.first(), app)
        }
        _ => String::new(),
    }
}

/// `Info(スペシャルパワー, name, info)` — 静的 `SpecialPowerData` を参照。
/// SRC.NET `Expression.cs` の `spd`(SPDList) 系 情報種類に対応。
/// 本実装が保持しないフィールド (解説文 / 適用条件 / アニメ / 効果* 等) は
/// 空文字を返す (graceful degradation)。
fn info_special_power(app: &App, name: &str, info: &str) -> String {
    let n = name.trim().trim_matches('"');
    // SRC は名称で引くが、シナリオでは短縮名/読み仮名が使われることもあるため寛容に照合。
    let Some(sp) = app
        .database()
        .special_powers
        .iter()
        .find(|s| s.name == n || s.short_name == n || s.kana_name == n)
    else {
        return String::new();
    };
    match info {
        "名称" => sp.name.clone(),
        "読み仮名" => sp.kana_name.clone(),
        "短縮名" => sp.short_name.clone(),
        "消費ＳＰ" => sp.sp_consumption.to_string(),
        "対象" => sp.target_type.clone(),
        "持続期間" => sp.duration.clone(),
        _ => String::new(),
    }
}

fn info_map(app: &App, info: &str, sub: &[&str]) -> String {
    // 時間帯はマップ不在でも返せるグローバル状態。
    if info == "時間帯" {
        return app.time_of_day().to_string();
    }
    let Some(map) = app.database().map.as_ref() else {
        return String::new();
    };
    match info {
        "幅" => map.width.to_string(),
        "高さ" => map.height.to_string(),
        "ファイル名" => String::new(),
        "時間帯" => app.time_of_day().to_string(), // (map 存在時も統一)
        _ => {
            // `Info(マップ, X, Y, ...)` 形式: info_kind ではなく X 座標が来た場合。
            // 第 1 sub = X, 第 2 = Y, 第 3 = attr
            let x: u32 = match info.parse() {
                Ok(v) => v,
                Err(_) => return String::new(),
            };
            let y: u32 = match sub.first().and_then(|s| fn_arg_value(app, s).parse().ok()) {
                Some(v) => v,
                None => return String::new(),
            };
            let attr = sub.get(1).map(|s| s.trim().trim_matches('"')).unwrap_or("");
            if x >= map.width || y >= map.height {
                return String::new();
            }
            let cell = map.cell(x, y);
            // 地形テーブル参照（シナリオ定義 → ビルトイン → デフォルト値）
            let db = app.database();
            let terrain_entry = db.terrain_by_id(cell.terrain_id);
            let builtin = crate::data::terrain::lookup(cell.terrain_id);
            match attr {
                "地形タイプ" => cell.terrain_id.to_string(),
                "地形名" => {
                    if let Some(t) = terrain_entry {
                        t.name.clone()
                    } else if let Some(t) = builtin {
                        t.name.to_string()
                    } else {
                        cell.terrain_id.to_string()
                    }
                }
                "ビットマップ名" => cell.bitmap_no.to_string(),
                "移動コスト" => db.terrain_move_cost(cell.terrain_id).to_string(),
                "回避修正" => db.terrain_hit_mod(cell.terrain_id).to_string(),
                "ダメージ修正" => db.terrain_damage_mod(cell.terrain_id).to_string(),
                "ＨＰ回復量" | "ＥＮ回復量" => "0".into(),
                "ユニットＩＤ" => app
                    .database()
                    .units_at(x, y)
                    .next()
                    .map(|u| u.unit_data_name.clone())
                    .unwrap_or_default(),
                _ => String::new(),
            }
        }
    }
}

/// 関数引数 1 個を「裸の識別子なら変数として解決、クオートなら literal」で
/// 評価して文字列値を返す。クオート未付き / `$()` 未展開の識別子も変数として
/// 解決する SRC 流のセマンティクスを模倣。
/// `Color` 命令の文字列を CSS カラーに正規化。
/// `RGB(r,g,b)` 形式や `rgb(r,g,b)` 形式はそのまま、英数色名は小文字化。
/// 色トークンを解決する。`FrameColor1` のように色がスクリプト変数で
/// 与えられる場合 (Alpha2ndStatus.ini 由来のテーマ色) はその値を引いてから
/// `canonical_color` を適用する。定義済み変数でなければそのまま色名として扱う。
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

/// 関数名の正規化: ASCII の case-insensitive。代表的な別名 (`Llength` /
/// `LLength`, `Length`) を 1 つに寄せる。
fn canonical_function_name(s: &str) -> String {
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

/// VB6 風の `Format(value, "##,##0.00")` パターンを最小限実装。
/// - `0` / `#` の数で小数部桁数を決定（末尾は固定 / トリム）。
/// - `,` がパターン内にあれば整数部に桁区切りを挿入。
/// - `%` は数値を 100 倍し、末尾に `%` を付与（`%` 1 個につき ×100）。
///   SRC.NET の `Format`(= VB6 Format) 準拠。`%` の出力位置は末尾に統一。
/// - その他の未対応文字は無視。
fn format_with_pattern(v: f64, fmt: &str) -> String {
    // `%` をパターンから取り除き、個数分だけ値を 100 倍する。
    let pct_count = fmt.chars().filter(|c| *c == '%').count();
    if pct_count == 0 {
        return format_number_pattern(v, fmt);
    }
    let scaled = v * 100f64.powi(pct_count as i32);
    let numeric_fmt: String = fmt.chars().filter(|c| *c != '%').collect();
    let body = format_number_pattern(scaled, &numeric_fmt);
    format!("{body}{}", "%".repeat(pct_count))
}

/// 銀行丸め (round half to even)。`f64::round_ties_even` 相当だが、
/// MSRV 1.75 では未安定 (1.77 安定) のため手動実装する。
/// 半端ちょうど (小数部 0.5) のときのみ偶数側へ丸め、それ以外は最近接。
fn round_half_to_even(x: f64) -> f64 {
    let floor = x.floor();
    let diff = x - floor;
    if diff < 0.5 {
        floor
    } else if diff > 0.5 {
        floor + 1.0
    } else if (floor as i64) % 2 == 0 {
        floor
    } else {
        floor + 1.0
    }
}

/// `format_with_pattern` の数値整形コア (`%` を含まないパターン専用)。
fn format_number_pattern(v: f64, fmt: &str) -> String {
    // 小数部の `0` / `#` 数で digit_count を決める
    let (int_part_fmt, frac_part_fmt) = match fmt.split_once('.') {
        Some((a, b)) => (a, b),
        None => (fmt, ""),
    };
    let zero_digits = frac_part_fmt.chars().filter(|c| *c == '0').count();
    let max_digits = frac_part_fmt
        .chars()
        .filter(|c| *c == '0' || *c == '#')
        .count();
    let want_thousands = int_part_fmt.contains(',');
    // 整数部の `0` 数を数えて左 0 埋めの最小桁数を決める。
    // VB6 `Format(42, "00000")` → "00042" のような桁固定 zero-padding 対応。
    let int_zero_pad: usize = int_part_fmt.chars().filter(|c| *c == '0').count();

    // VB6 `Format` は銀行丸め (round half to even) を用いる。
    // SRC.NET `VB.Compatibility.VB6.Support.Format` 準拠。
    // 例: Format(0.5,"0")="0"、Format(2.5,"0")="2"、Format(0.125,"0.00")="0.12"。
    let factor = 10f64.powi(max_digits as i32);
    let rounded = round_half_to_even(v * factor) / factor;
    let sign = if rounded < 0.0 { "-" } else { "" };
    let abs = rounded.abs();
    let int_v = abs.trunc() as i64;
    let mut int_str = int_v.to_string();
    // 整数部左 0 埋め
    if int_zero_pad > int_str.len() {
        let pad = int_zero_pad - int_str.len();
        int_str = "0".repeat(pad) + &int_str;
    }
    if want_thousands {
        int_str = insert_thousands_separator(&int_str);
    }
    if max_digits == 0 {
        return format!("{sign}{int_str}");
    }
    // 小数部
    let frac_val = abs - abs.trunc();
    let mut frac_str = format!("{:.*}", max_digits, frac_val);
    // "0.xxx" の先頭 "0." を取り除く
    if let Some(dot) = frac_str.find('.') {
        frac_str = frac_str[dot + 1..].to_string();
    }
    // 末尾の余分な 0 を削るが、zero_digits までは保持
    while frac_str.len() > zero_digits && frac_str.ends_with('0') {
        frac_str.pop();
    }
    if frac_str.is_empty() {
        return format!("{sign}{int_str}");
    }
    format!("{sign}{int_str}.{frac_str}")
}

/// 半角カタカナ (U+FF61..=U+FF9D) → 全角カタカナの基底対応表。
/// 添字は `code - 0xFF61`。濁点 (FF9E) / 半濁点 (FF9F) は別途合成処理する。
const HALFWIDTH_KANA_BASE: [char; 61] = [
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
fn fullwidth_voiced(base: char) -> Option<char> {
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
fn fullwidth_semivoiced(base: char) -> Option<char> {
    match base {
        // ハ行は清音 +2 が半濁音 (パピプペポ)。
        'ハ' | 'ヒ' | 'フ' | 'ヘ' | 'ホ' => char::from_u32(base as u32 + 2),
        _ => None,
    }
}

/// 半角文字を全角に変換する (VB6 `StrConv(s, vbWide)` 相当)。
/// - 半角空白 (0x20) → 全角空白 (U+3000)
/// - 半角 ASCII (0x21..=0x7E) → 全角形 (U+FF01..=U+FF5E)
/// - 半角カタカナ (U+FF61..=U+FF9D) → 全角カタカナ。直後の濁点 ﾞ (FF9E) /
///   半濁点 ﾟ (FF9F) は合成する (ｶﾞ → ガ、ﾊﾟ → パ)。合成できない場合は
///   単独の全角濁点 ゛(U+309B) / 半濁点 ゜(U+309C) を出力。
/// - それ以外 (ひらがな・漢字・既に全角の文字) は不変。
fn to_fullwidth(s: &str) -> String {
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

fn insert_thousands_separator(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in chars.iter().enumerate() {
        let from_right = chars.len() - i;
        if i > 0 && from_right % 3 == 0 {
            out.push(',');
        }
        out.push(*c);
    }
    out
}

// 数値 (f64) を `0.0` のような末尾無駄を削った文字列で整形。
// ──────────────────────────────────────────────────────────────────────────────
// Shift-JIS バイト列操作ヘルパー
// SRC の *B 系関数 (LenB / LeftB / RightB / MidB / InStrB / InStrRevB) は
// Shift-JIS エンコーディング基準のバイト位置を使う。
// VB6 実装と同様に ASCII 文字=1バイト、それ以外 (日本語等)=2バイトで近似する。
// ──────────────────────────────────────────────────────────────────────────────

/// 1 文字あたりの Shift-JIS バイト数を返す (ASCII → 1, 非ASCII → 2)
#[inline]
fn sjis_char_bytes(c: char) -> usize {
    if c.is_ascii() {
        1
    } else {
        2
    }
}

/// Shift-JIS バイト長を返す
fn sjis_byte_len(s: &str) -> usize {
    s.chars().map(sjis_char_bytes).sum()
}

/// 先頭から `byte_count` バイト分の部分文字列を返す (Shift-JIS 換算)
fn sjis_left(s: &str, byte_count: usize) -> String {
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
fn sjis_right(s: &str, byte_count: usize) -> String {
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
fn sjis_mid(s: &str, start: usize, length: Option<usize>) -> String {
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
fn sjis_instr(s: &str, pattern: &str, start: usize) -> usize {
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
fn sjis_instrrev(s: &str, pattern: &str, end_byte: Option<usize>) -> usize {
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
fn str_display_width(s: &str) -> usize {
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

fn format_num(v: f64) -> String {
    if v.fract() == 0.0 && v.is_finite() && v.abs() < 1.0e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

fn fn_arg_value(app: &App, arg: &str) -> String {
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

/// 数学関数引数用の数値評価ヘルパ。`fn_arg_value` の結果を `try_eval_num`
/// で算術式として評価し、失敗したら 0 を返す。
fn numeric_arg(app: &App, arg: &str) -> Option<f64> {
    let s = fn_arg_value(app, arg);
    if let Ok(v) = s.parse::<f64>() {
        return Some(v);
    }
    try_eval_num(&s)
}

/// SRC のシステム変数 (`味方数` / `敵数` / `友軍数` / `中立数` / `ターン数` 等)
/// に対する動的解決。該当しない名前なら `None`。
fn system_variable_value(app: &App, name: &str) -> Option<String> {
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
            .map(|u| (((u.total_exp / 100).max(0) + 1).min(99)) as i64)
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
        let db = app.database();
        let new_feats: Vec<crate::feature::ActiveFeature> = unit_data
            .features
            .iter()
            .map(|(name, value)| {
                let (val, skill, cond) = crate::necessary_skill::split_feature_necessary(value);
                let mut f = crate::feature::ActiveFeature::new(name.clone(), val);
                let skill_ok =
                    skill.is_empty() || crate::necessary_skill::is_satisfied(&skill, inst, db);
                let cond_ok =
                    cond.is_empty() || crate::necessary_skill::is_satisfied(&cond, inst, db);
                if !(skill_ok && cond_ok) {
                    f.is_active = false;
                }
                f
            })
            .collect();
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

/// `Switch` から見て、ネスト深度 0 で `Case "v"` / `CaseElse` / `EndSw` を探索。
/// `Case` 値と `value` が一致したらその次の PC を返す。
/// 一致するものが無く `CaseElse` が見つかればそこへ。最終的に `EndSw` まで来れば
/// その次の PC を返す（マッチ無しで Switch を抜ける）。
fn find_matching_case(
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
fn case_value_matches(case_v: &str, value: &str) -> bool {
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
fn case_part_matches(part: &str, value: &str) -> bool {
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

fn split_to_range(s: &str) -> Option<(&str, &str)> {
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
fn skip_to_endsw(pc: usize, stmts: &[EventStatement], line: usize) -> Result<usize, ScriptError> {
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
fn foreach_status_mask(status_args: &[String]) -> (bool, bool) {
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

/// `Call` 時の Args 束縛: 呼び出し元の `Args(1..9)` / `ArgNum` / `upvar_level` /
/// `upvar_base_argnum` をスナップショットして退避し、`new_args` を新しい
/// `Args(1..)` / `ArgNum` として束縛する。退避した値を返す
/// (`push_call_return` に渡し、`Return` で `restore_call_args` する)。
///
/// SRC の `Args` は呼び出しフレーム単位なので、ネストした `Call` が
/// 呼び出し元の `Args` を破壊しないようこのスナップショット/復元が要る。
///
/// saved の layout: [Args(1)..Args(9), ArgNum, upvar_level, upvar_base_argnum]  (長さ 12)
fn enter_call_args(app: &mut App, new_args: &[String]) -> Vec<String> {
    let mut saved: Vec<String> = (1..=9)
        .map(|k| app.script_var(&format!("Args({k})")).to_string())
        .collect();
    // ArgNum も退避 (10 番目要素)
    saved.push(app.script_var("ArgNum").to_string());
    // upvar_level と upvar_base_argnum を退避 (11・12 番目要素)
    saved.push(app.upvar_level().to_string());
    saved.push(app.upvar_base_argnum().to_string());

    // 新フレーム用に Args をクリアして引数をセット
    for k in 1..=9 {
        app.set_script_var(format!("Args({k})"), String::new());
    }
    // 各引数は `fn_arg_value` で解決する。`Call 敵配置 … 敵配置数` のように
    // 裸変数を渡すと、expand_vars は裸変数を展開しないため `Args(N)` に
    // 変数 *名* が入ってしまう。SRC の `Call` は引数を式評価して渡すので、
    // 単一の括弧式 (`(500 - Info(…))` 等) は数値化する。
    for (i, v) in new_args.iter().enumerate() {
        let resolved = fn_arg_value(app, v);
        let resolved = eval_paren_arith_value(app, &resolved).unwrap_or(resolved);
        app.set_script_var(format!("Args({})", i + 1), resolved);
    }
    // ArgNum = 渡した引数の数
    app.set_script_var("ArgNum".to_string(), new_args.len().to_string());
    // 新フレームの upvar 状態を初期化
    app.set_upvar_level(0);
    app.set_upvar_base_argnum(0);
    saved
}

/// `Return` 時に呼び出し元の `Args(1..9)` / `ArgNum` / `upvar_level` /
/// `upvar_base_argnum` を復元する。
/// saved の layout: [Args(1)..Args(9), ArgNum, upvar_level, upvar_base_argnum]  (長さ 12)
fn restore_call_args(app: &mut App, saved: Vec<String>) {
    for (i, v) in saved.iter().take(9).enumerate() {
        app.set_script_var(format!("Args({})", i + 1), v.clone());
    }
    // 10 番目要素が ArgNum
    if let Some(argnum) = saved.get(9) {
        app.set_script_var("ArgNum".to_string(), argnum.clone());
    }
    // 11・12 番目要素が upvar_level と upvar_base_argnum
    if let Some(ul) = saved.get(10).and_then(|s| s.parse::<usize>().ok()) {
        app.set_upvar_level(ul);
    } else {
        app.set_upvar_level(0);
    }
    if let Some(ub) = saved.get(11).and_then(|s| s.parse::<usize>().ok()) {
        app.set_upvar_base_argnum(ub);
    } else {
        app.set_upvar_base_argnum(0);
    }
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

/// `ForEach group` (書式1) の 1 反復ぶん: 識別子 `ident` のユニットを
/// 「対象」としてシステム変数 `対象ユニットＩＤ` / `対象パイロット` に束縛する。
/// `Create` が設定するのと同じ変数なので、後続の `Pilot(対象ユニットＩＤ)` /
/// `Skill(対象パイロット, …)` 等がそのまま現在のユニットを参照できる。
fn bind_foreach_unit(app: &mut App, ident: &str) {
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
    app.set_script_var("対象ユニットＩＤ".to_string(), uid);
    app.set_script_var("対象パイロット".to_string(), pilot);
}

/// `パイロット一覧(mode)` / `ユニット一覧(mode)` (ForEach 書式3/4) を解決し、
/// 搭乗中パイロット名 / ユニット名の一覧を返す。`mode` が `レベル` を含むときは
/// レベル降順、それ以外は `unit_instances` の登録順。
///
/// SRC の `mode` には多数の整列基準があるが、本実装はレベルのみ厳密に扱い、
/// 残りは登録順にフォールバックする (反復対象の集合自体は同じ)。
fn collect_roster(app: &App, mode: &str, pilots: bool) -> Vec<String> {
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
        rows.push((key, i64::from(u.total_exp) / 100 + 1));
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
fn collect_foreach_items(app: &App, collection: &str) -> Vec<String> {
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

/// 簡易整数式評価。`+ - * /` と `(...)` をサポート。失敗時は 0。
/// 入力は `expand_vars` 後の文字列を想定（変数 / 関数呼出は既に展開済み）。
fn eval_int_expr(s: &str) -> i32 {
    let trimmed = s.trim().trim_matches('"').trim();
    if let Ok(v) = trimmed.parse::<i32>() {
        return v;
    }
    let tokens = tokenize_expr(trimmed);
    let mut idx = 0;
    // `.trunc()` でゼロ方向へ切り捨て (SRC.Sharp `(int)value` と同様)。
    let v = parse_logical(&tokens, &mut idx).unwrap_or(0.0);
    v.trunc() as i32
}

/// `eval_int_expr` の app-aware 版。式中の **裸の識別子** (`j` / `i` 等の
/// ループ変数) を `script_var` として解決し、**引用符付き数値** (`"3"`) も
/// 受理してから評価する。
///
/// 例: `機体選択開始` の `HotPoint name (j * 40 - 35) ("$(i)" * 70 + 15) ...`
/// は、`$(i)` こそ `expand_vars` で展開済みだが `j` は裸のまま残り、
/// `("3" * 70 + 15)` のように引用符付き数値も混じる。素の `eval_int_expr`
/// はこれらを 0 に潰してしまい、全 Hotpoint が左上に重なって配置されていた。
fn eval_int_expr_app(app: &App, s: &str) -> i32 {
    eval_int_expr(&resolve_expr_atoms(app, s))
}

/// 算術 / 比較 / 論理式中の演算子で区切られた各「アトム」を解決する。
/// - `And`, `Or`, `Not`, `Mod` キーワード → そのまま出力 (tokenizer に渡す)
/// - 数値リテラル (引用符付きも可) → そのまま
/// - 定義済みの数値スクリプト変数 → その値
/// - それ以外 → `0`
fn resolve_expr_atoms(app: &App, s: &str) -> String {
    fn flush(app: &App, atom: &mut String, out: &mut String) {
        // `Line 50, 40, ...` の tokenizer 出力は `"50,"` のように trailing `,`
        // を含む。trim_matches(',') で先に剥がしてから数値判定する。
        let t = atom
            .trim()
            .trim_matches('"')
            .trim()
            .trim_matches(',')
            .trim();
        if !t.is_empty() {
            // 算術 / 論理キーワードはそのまま出力 (tokenize_expr が認識する)。
            if matches!(
                t.to_ascii_lowercase().as_str(),
                "and" | "or" | "not" | "mod"
            ) {
                out.push(' ');
                out.push_str(t);
                out.push(' ');
            } else if t.parse::<f64>().is_ok() {
                out.push_str(t);
            } else {
                // ① シナリオ変数 (script_var) を優先。
                let sv = app.script_var(t);
                if sv.trim().parse::<f64>().is_ok() {
                    out.push_str(sv.trim());
                } else if let Some(val) =
                    system_variable_value(app, t).filter(|v| v.trim().parse::<f64>().is_ok())
                {
                    // ② システム変数 (味方数 / 味方レベル平均値 / ターン数 等) を解決。
                    out.push_str(val.trim());
                } else {
                    out.push('0');
                }
            }
        }
        atom.clear();
    }
    let mut out = String::new();
    let mut atom = String::new();
    for ch in s.chars() {
        // アトム区切り = 算術演算子 (`is_arith_operator_char`) + 比較 `> < =` +
        // 文字列連結 `&`。算術演算子集合を一元化しているため `\`(整数除算) /
        // `^`(累乗) の取りこぼしが起きない (旧実装では `3 \ 8` の `\` が未定義
        // 変数として `0` に潰れ `308` になっていた)。
        if is_arith_operator_char(ch) || matches!(ch, '>' | '<' | '=' | '&') {
            flush(app, &mut atom, &mut out);
            out.push(ch);
        } else if ch == ' ' || ch == '\t' {
            // スペースはアトム区切りだが出力には入れない (tokenizer が無視する)。
            flush(app, &mut atom, &mut out);
        } else {
            atom.push(ch);
        }
    }
    flush(app, &mut atom, &mut out);
    out
}

/// 値が単一の括弧式 `(...)` で、その中身が純粋な算術式 (全アトムが数値
/// リテラル、または数値に解決できるスクリプト変数) なら評価して数値文字列を
/// 返す。それ以外は `None`。
///
/// SRC の `Set` は値を式評価する。`expand_vars` は関数呼出 (`RoundUp(…)` 等)
/// は評価するが `(a + b)` のような括弧付き算術はトークンとして残すため、
/// `Set 敵配置数 (RoundUp(…) + (進行度 / 5))` が文字列のまま格納されていた。
///
/// 非数値アトムを 1 つでも含むなら算術式ではない (`Set msg (こんにちは)`)
/// とみなし評価しない。これにより文字列値を誤って 0 に潰さない。
fn eval_paren_arith_value(app: &App, s: &str) -> Option<String> {
    let t = s.trim();
    if t.len() < 2 || !t.starts_with('(') || !t.ends_with(')') {
        return None;
    }
    // 全体が 1 つの括弧対で囲われていること (`(a)+(b)` や `(a)(b)` を弾く)。
    if find_matching_paren(t, 0)? != t.len() - 1 {
        return None;
    }
    // 演算子 / スペースで区切った各アトムが数値 / 数値変数 /
    // 認識済みキーワード (`And`, `Or`, `Not`, `Mod`) であることを検証する。
    // 非数値リテラル (`こんにちは` など) を 0 に潰さないためのガード。
    let atom_is_numeric_or_kw = |atom: &str| -> bool {
        let a = atom.trim().trim_matches('"').trim();
        a.is_empty()
            || a.parse::<f64>().is_ok()
            || app.script_var(a).trim().parse::<f64>().is_ok()
            || matches!(
                a.to_ascii_lowercase().as_str(),
                "and" | "or" | "not" | "mod"
            )
    };
    let mut atom = String::new();
    for ch in t.chars() {
        // アトム区切り = 算術演算子 (`is_arith_operator_char`) + 比較 `> < =` +
        // 連結 `&` + 空白。算術演算子集合を一元化 (`\` `^` の取りこぼし防止)。
        // これが無いと `(... \ 8 + 1)` の `\` がアトム `\8` に紛れて数値判定に
        // 失敗し、式が評価されず生文字列のまま残る (武器/強化パーツ/特殊能力の
        // ページ数計算 `(N - 1) \ 8 + 1` 等が壊れていた)。
        if is_arith_operator_char(ch) || matches!(ch, '>' | '<' | '=' | '&') || ch.is_whitespace() {
            if !atom_is_numeric_or_kw(&atom) {
                return None;
            }
            atom.clear();
        } else {
            atom.push(ch);
        }
    }
    if !atom_is_numeric_or_kw(&atom) {
        return None;
    }
    // 未定義値が空に解決して二項演算子の直後が `)`/末尾になった場合に `0` を補う
    // (SRC: 未定義数値は 0)。これで `(500 - )` / `(500 * 0 - 500 + )` のような式
    // (Info(...) 等が空に解決) も評価でき、生式の表示漏れを防ぐ。
    let resolved = fill_dangling_operands(&resolve_expr_atoms(app, t));
    let v = try_eval_num(&resolved)?;
    Some(format_num(v))
}

/// `resolve_expr_atoms` の出力 (空白なし) で、二項 `+`/`-`/`*` の直後が `)` または
/// 末尾になっている (= オペランド欠落) 箇所に `0` を補う。除算/累乗 (`/`/`\`/`^`)
/// は 0 補填が危険 (ゼロ除算) なので対象外 (該当式は従来どおり未評価のまま)。
fn fill_dangling_operands(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 2);
    for (i, &c) in chars.iter().enumerate() {
        out.push(c);
        if matches!(c, '+' | '-' | '*') {
            let next = chars.get(i + 1);
            if next.is_none() || next == Some(&')') {
                out.push('0');
            }
        }
    }
    out
}

/// `tokenize_expr` が **記号演算子トークン** を生成する 1 文字の集合 (Single
/// Source of Truth)。算術式のアトム分割 (`resolve_expr_atoms` /
/// `eval_paren_arith_value`)・座標式判定 (`parse_paintstring_args`)・添字式
/// 判定など、「式中の演算子を認識する」必要のある全箇所はこの関数を参照する。
///
/// 以前は各所が独自に `'+' | '-' | '*' | '/' | ...` をハードコードしていたため、
/// `tokenize_expr` に VB6 整数除算 `\` と累乗 `^` を追加した際に分割側の更新が
/// 漏れ、`(N - 1) \ 8 + 1` 等が評価されない不具合を生んだ。これを一元化し、
/// `arith_operator_char_set_matches_tokenizer` テストで `tokenize_expr` との
/// 整合を機械的に保証する (新演算子の追加漏れをテストが検出する)。
///
/// 比較 / 論理 / 連結 (`< > = &`) は算術ではないためここには含めず、条件式を
/// 扱う呼び出し側が個別に加える。語演算子 (And/Or/Not/Mod) も記号ではないので
/// 各呼び出し側がキーワードとして扱う。
fn is_arith_operator_char(c: char) -> bool {
    matches!(c, '+' | '-' | '*' | '/' | '\\' | '^' | '(' | ')')
}

#[derive(Debug, Clone)]
enum ExprTok {
    Num(f64),
    Plus,
    Minus,
    Star,
    Slash,
    IntDiv, // `\` VB6 integer division
    Mod,    // `Mod` keyword — modulo
    Caret,  // `^` exponentiation
    LParen,
    RParen,
    // 比較演算子
    Gt, // `>`
    Lt, // `<`
    Ge, // `>=`
    Le, // `<=`
    Eq, // `=`
    Ne, // `<>`
    // 論理演算子
    And, // `And` keyword
    Or,  // `Or` keyword
    Not, // `Not` keyword (prefix)
}

fn tokenize_expr(s: &str) -> Vec<ExprTok> {
    let bytes = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b' ' | b'\t' => i += 1,
            b'+' => {
                out.push(ExprTok::Plus);
                i += 1;
            }
            b'-' => {
                out.push(ExprTok::Minus);
                i += 1;
            }
            b'*' => {
                out.push(ExprTok::Star);
                i += 1;
            }
            b'/' => {
                out.push(ExprTok::Slash);
                i += 1;
            }
            b'\\' => {
                // VB6 integer division
                out.push(ExprTok::IntDiv);
                i += 1;
            }
            b'^' => {
                out.push(ExprTok::Caret);
                i += 1;
            }
            b'(' => {
                out.push(ExprTok::LParen);
                i += 1;
            }
            b')' => {
                out.push(ExprTok::RParen);
                i += 1;
            }
            b'>' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    out.push(ExprTok::Ge);
                    i += 2;
                } else {
                    out.push(ExprTok::Gt);
                    i += 1;
                }
            }
            b'<' => {
                if i + 1 < bytes.len() && bytes[i + 1] == b'=' {
                    out.push(ExprTok::Le);
                    i += 2;
                } else if i + 1 < bytes.len() && bytes[i + 1] == b'>' {
                    out.push(ExprTok::Ne);
                    i += 2;
                } else {
                    out.push(ExprTok::Lt);
                    i += 1;
                }
            }
            b'=' => {
                out.push(ExprTok::Eq);
                i += 1;
            }
            b'0'..=b'9' | b'.' => {
                let start = i;
                while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                    i += 1;
                }
                if let Ok(v) = s[start..i].parse::<f64>() {
                    out.push(ExprTok::Num(v));
                }
            }
            b'M' | b'm' => {
                // `Mod` keyword (case-insensitive) — byte-level comparison to avoid UTF-8 issues
                if i + 3 <= bytes.len()
                    && (bytes[i] == b'M' || bytes[i] == b'm')
                    && (bytes[i + 1] == b'O' || bytes[i + 1] == b'o')
                    && (bytes[i + 2] == b'D' || bytes[i + 2] == b'd')
                    && (i + 3 >= bytes.len() || !bytes[i + 3].is_ascii_alphanumeric())
                {
                    out.push(ExprTok::Mod);
                    i += 3;
                } else {
                    break; // unknown keyword — bail
                }
            }
            b'A' | b'a' => {
                // `And` keyword (case-insensitive)
                if i + 3 <= bytes.len()
                    && bytes[i].eq_ignore_ascii_case(&b'a')
                    && bytes[i + 1].eq_ignore_ascii_case(&b'n')
                    && bytes[i + 2].eq_ignore_ascii_case(&b'd')
                    && (i + 3 >= bytes.len() || !bytes[i + 3].is_ascii_alphanumeric())
                {
                    out.push(ExprTok::And);
                    i += 3;
                } else {
                    break;
                }
            }
            b'O' | b'o' => {
                // `Or` keyword (case-insensitive)
                if i + 2 <= bytes.len()
                    && bytes[i].eq_ignore_ascii_case(&b'o')
                    && bytes[i + 1].eq_ignore_ascii_case(&b'r')
                    && (i + 2 >= bytes.len() || !bytes[i + 2].is_ascii_alphanumeric())
                {
                    out.push(ExprTok::Or);
                    i += 2;
                } else {
                    break;
                }
            }
            b'N' | b'n' => {
                // `Not` keyword (case-insensitive)
                if i + 3 <= bytes.len()
                    && bytes[i].eq_ignore_ascii_case(&b'n')
                    && bytes[i + 1].eq_ignore_ascii_case(&b'o')
                    && bytes[i + 2].eq_ignore_ascii_case(&b't')
                    && (i + 3 >= bytes.len() || !bytes[i + 3].is_ascii_alphanumeric())
                {
                    out.push(ExprTok::Not);
                    i += 3;
                } else {
                    break;
                }
            }
            _ => {
                // unknown char — bail
                break;
            }
        }
    }
    out
}

fn parse_expr(tokens: &[ExprTok], idx: &mut usize) -> Option<f64> {
    let mut left = parse_term(tokens, idx)?;
    while *idx < tokens.len() {
        match tokens[*idx] {
            ExprTok::Plus => {
                *idx += 1;
                let r = parse_term(tokens, idx)?;
                left += r;
            }
            ExprTok::Minus => {
                *idx += 1;
                let r = parse_term(tokens, idx)?;
                left -= r;
            }
            _ => break,
        }
    }
    Some(left)
}

fn parse_term(tokens: &[ExprTok], idx: &mut usize) -> Option<f64> {
    let mut left = parse_power(tokens, idx)?;
    while *idx < tokens.len() {
        match tokens[*idx] {
            ExprTok::Star => {
                *idx += 1;
                let r = parse_power(tokens, idx)?;
                left *= r;
            }
            ExprTok::Slash => {
                *idx += 1;
                let r = parse_power(tokens, idx)?;
                if r != 0.0 {
                    left /= r;
                }
            }
            ExprTok::IntDiv => {
                // VB6 integer division: truncate toward zero
                *idx += 1;
                let r = parse_power(tokens, idx)?;
                if r as i64 != 0 {
                    left = (left as i64 / r as i64) as f64;
                }
            }
            ExprTok::Mod => {
                // VB6 Mod: integer modulo
                *idx += 1;
                let r = parse_power(tokens, idx)?;
                if r as i64 != 0 {
                    left = (left as i64 % r as i64) as f64;
                }
            }
            _ => break,
        }
    }
    Some(left)
}

/// 累乗: 右結合 `a ^ b ^ c` = `a ^ (b ^ c)`
fn parse_power(tokens: &[ExprTok], idx: &mut usize) -> Option<f64> {
    let base = parse_factor(tokens, idx)?;
    if matches!(tokens.get(*idx), Some(ExprTok::Caret)) {
        *idx += 1;
        // 右結合のため再帰呼び出し
        let exp = parse_power(tokens, idx)?;
        Some(base.powf(exp))
    } else {
        Some(base)
    }
}

fn parse_factor(tokens: &[ExprTok], idx: &mut usize) -> Option<f64> {
    let tok = tokens.get(*idx)?;
    match tok {
        ExprTok::Num(v) => {
            *idx += 1;
            Some(*v)
        }
        ExprTok::Minus => {
            *idx += 1;
            Some(-parse_factor(tokens, idx)?)
        }
        ExprTok::Plus => {
            *idx += 1;
            parse_factor(tokens, idx)
        }
        ExprTok::Not => {
            // `Not x` — 論理否定: x が 0.0 なら 1.0、それ以外は 0.0
            *idx += 1;
            let v = parse_factor(tokens, idx)?;
            Some(if v == 0.0 { 1.0 } else { 0.0 })
        }
        ExprTok::LParen => {
            *idx += 1;
            let v = parse_logical(tokens, idx)?;
            if matches!(tokens.get(*idx), Some(ExprTok::RParen)) {
                *idx += 1;
            }
            Some(v)
        }
        _ => None,
    }
}

/// 比較演算子レベル: `=`, `<>`, `<`, `>`, `<=`, `>=`
/// 戻り値は 0.0 (false) または 1.0 (true)。算術式の結果が比較されない場合は
/// そのまま算術値を返す (precedence は `+/-` より低い)。
fn parse_comparison(tokens: &[ExprTok], idx: &mut usize) -> Option<f64> {
    let mut left = parse_expr(tokens, idx)?;
    while *idx < tokens.len() {
        match tokens[*idx] {
            ExprTok::Gt => {
                *idx += 1;
                let r = parse_expr(tokens, idx)?;
                left = if left > r { 1.0 } else { 0.0 };
            }
            ExprTok::Lt => {
                *idx += 1;
                let r = parse_expr(tokens, idx)?;
                left = if left < r { 1.0 } else { 0.0 };
            }
            ExprTok::Ge => {
                *idx += 1;
                let r = parse_expr(tokens, idx)?;
                left = if left >= r { 1.0 } else { 0.0 };
            }
            ExprTok::Le => {
                *idx += 1;
                let r = parse_expr(tokens, idx)?;
                left = if left <= r { 1.0 } else { 0.0 };
            }
            ExprTok::Eq => {
                *idx += 1;
                let r = parse_expr(tokens, idx)?;
                left = if (left - r).abs() < 1e-9 { 1.0 } else { 0.0 };
            }
            ExprTok::Ne => {
                *idx += 1;
                let r = parse_expr(tokens, idx)?;
                left = if (left - r).abs() >= 1e-9 { 1.0 } else { 0.0 };
            }
            _ => break,
        }
    }
    Some(left)
}

/// 論理演算子レベル: `And` / `Or` (最低優先度)。
/// 0.0 を false、それ以外を true として短絡評価する。
fn parse_logical(tokens: &[ExprTok], idx: &mut usize) -> Option<f64> {
    let mut left = parse_comparison(tokens, idx)?;
    while *idx < tokens.len() {
        match tokens[*idx] {
            ExprTok::And => {
                *idx += 1;
                let r = parse_comparison(tokens, idx)?;
                left = if left != 0.0 && r != 0.0 { 1.0 } else { 0.0 };
            }
            ExprTok::Or => {
                *idx += 1;
                let r = parse_comparison(tokens, idx)?;
                left = if left != 0.0 || r != 0.0 { 1.0 } else { 0.0 };
            }
            _ => break,
        }
    }
    Some(left)
}

/// 文字列の外側の `(...)` を 1 重剥がす。剥がせなかったら元のまま。
fn strip_outer_parens(s: &str) -> String {
    let t = s.trim();
    if t.starts_with('(') && t.ends_with(')') && t.len() >= 2 {
        t[1..t.len() - 1].trim().to_string()
    } else {
        t.to_string()
    }
}

/// インライン条件文字列を評価。`expand_vars` 後、空白で分割
/// (paren を維持) して `eval_condition_args_with` に渡す。
fn eval_inline_condition(app: &App, text: &str) -> bool {
    let expanded = expand_vars(app, text);
    // paren / quote を尊重した空白分割（簡易: paren_depth + quote 状態）
    let parts = split_balanced(&expanded);
    eval_condition_args_with(app, &parts)
}

/// `eval_inline_condition` の `&mut App` 版。条件式中に `Call(<label>)` サブ式が
/// あれば、`evaluate_command_condition`（`If` 用）と同じく
/// `preprocess_call_expressions_in_condition` でサブルーチンを同期実行し返り値に
/// 置換してから評価する。
///
/// `Do While Call(cond)` / `Loop While Call(cond)` のように `&mut App` 文脈の
/// ループ条件で使う。`Call()` を含まない場合は不変版と同じ挙動。
fn eval_inline_condition_mut(app: &mut App, text: &str) -> bool {
    if text.to_ascii_lowercase().contains("call(") {
        // `Call(label)` を返り値文字列に置換してから通常評価へ委譲する。
        // 置換後は `${...}` 変数を含みうるが、`eval_inline_condition` 内の
        // `expand_vars` が解決する。
        let processed = preprocess_call_expressions_in_condition(app, text);
        eval_inline_condition(app, &processed)
    } else {
        eval_inline_condition(app, text)
    }
}

/// 文字列を空白で分割。`(...)` / `"..."` の内側はまとめる。
fn split_balanced(s: &str) -> Vec<String> {
    // UTF-8 多バイト文字 (日本語等) を壊さないよう、char 単位で走査する。
    // `b as char` を使った旧実装は `選択` のような文字を E9 81 B8 ... の
    // 各バイトに分解して "é\u{81}¸æ..." のようなゴミ文字列を生成していた。
    let mut out = Vec::new();
    let mut buf = String::new();
    let mut paren = 0i32;
    let mut in_quote = false;
    for c in s.chars() {
        if c == '"' {
            in_quote = !in_quote;
            buf.push(c);
        } else if !in_quote && c == '(' {
            paren += 1;
            buf.push(c);
        } else if !in_quote && c == ')' {
            if paren > 0 {
                paren -= 1;
            }
            buf.push(c);
        } else if !in_quote && paren == 0 && (c == ' ' || c == '\t') {
            if !buf.is_empty() {
                out.push(std::mem::take(&mut buf));
            }
        } else {
            buf.push(c);
        }
    }
    if !buf.is_empty() {
        out.push(buf);
    }
    out
}

/// `open` の対応する `close` まで前方スキップ。Do/Loop の Skip 用。
fn skip_to_matching(
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
fn skip_to_loop_or_next_end(
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
fn find_back(
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
fn jump_back_to(
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
fn skip_to_next(pc: usize, stmts: &[EventStatement], line: usize) -> Result<usize, ScriptError> {
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

/// マップ攻撃の効果範囲を表す `(x, y)` 集合を計算する。
/// 武器の `class` 文字列に含まれる SRC の属性記号を解釈する:
///
/// - `Ｍ全`             : 全方位 (攻撃者を中心としたマンハッタン菱形 ≤ max_range)
/// - `Ｍ投L<n>` / `Ｍ投` : 指定地点を中心とした菱形 (半径 = L 値 or max_range)
/// - `Ｍ直`             : 攻撃者から指定方向へ max_range タイルの直線
/// - `Ｍ拡`             : 攻撃者から指定方向へ max_range タイル幅 3 の直線
/// - `Ｍ移` / `Ｍ線`     : 攻撃者と指定地点を結ぶ直線 (近似で軸線優先)
/// - `Ｍ扇L<n>`         : 簡略実装で `Ｍ拡` 同等 (将来詳細化)
/// - 不明 / `Ｍ` 接頭辞なし : 指定地点中心の菱形 (半径 = max_range) — 旧挙動互換
pub(crate) fn map_attack_area(
    weapon: &crate::data::unit::WeaponData,
    src: (u32, u32),
    target: (u32, u32),
) -> Vec<(u32, u32)> {
    let cls = weapon.class.as_str();
    let range = weapon.max_range.max(0);
    let lvl_after = |key: &str| -> Option<i32> {
        cls.find(key).map(|idx| {
            let tail = &cls[idx + key.len()..];
            // 'L' があれば数字を読む
            if let Some(rest) = tail.strip_prefix('L') {
                let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
                digits.parse::<i32>().unwrap_or(range)
            } else {
                range
            }
        })
    };

    let mut out: Vec<(u32, u32)> = Vec::new();
    if cls.contains("Ｍ全") {
        // 攻撃者中心の菱形 (max_range)
        diamond_into(&mut out, src.0 as i32, src.1 as i32, range);
    } else if let Some(lvl) = lvl_after("Ｍ投") {
        // ターゲット中心の菱形 (L<lvl> あれば lvl、無ければ max_range)
        diamond_into(&mut out, target.0 as i32, target.1 as i32, lvl);
    } else if cls.contains("Ｍ直") {
        line_into(&mut out, src, target, range, 0);
    } else if cls.contains("Ｍ拡") || cls.contains("Ｍ扇") {
        // 簡略実装: 3 マス幅の直線
        line_into(&mut out, src, target, range, 1);
    } else if cls.contains("Ｍ移") || cls.contains("Ｍ線") {
        // 攻撃者からターゲットまでの直線 (距離 ≤ max_range)
        line_into(&mut out, src, target, range, 0);
    } else {
        // フォールバック: 旧挙動 (target 中心の菱形, radius=max_range)
        diamond_into(&mut out, target.0 as i32, target.1 as i32, range);
    }
    out
}

/// (cx, cy) を中心としたマンハッタン半径 `radius` の菱形領域。
fn diamond_into(out: &mut Vec<(u32, u32)>, cx: i32, cy: i32, radius: i32) {
    if radius < 0 {
        return;
    }
    for dy in -radius..=radius {
        let span = radius - dy.abs();
        for dx in -span..=span {
            let x = cx + dx;
            let y = cy + dy;
            if x >= 0 && y >= 0 {
                out.push((x as u32, y as u32));
            }
        }
    }
}

/// 攻撃者 `src` から `target` へ向かう直線（軸線優先）。
/// `width_each` = 1 なら線の両側 1 マス（合計 3 マス幅）。
fn line_into(
    out: &mut Vec<(u32, u32)>,
    src: (u32, u32),
    target: (u32, u32),
    range: i32,
    width_each: i32,
) {
    let dx = target.0 as i32 - src.0 as i32;
    let dy = target.1 as i32 - src.1 as i32;
    let (sx, sy) = if dx.abs() >= dy.abs() {
        (dx.signum(), 0)
    } else {
        (0, dy.signum())
    };
    if sx == 0 && sy == 0 {
        return;
    }
    for step in 1..=range {
        let x = src.0 as i32 + sx * step;
        let y = src.1 as i32 + sy * step;
        if x < 0 || y < 0 {
            continue;
        }
        for w in -width_each..=width_each {
            // 線方向に垂直なオフセット
            let (ox, oy) = if sx != 0 { (0, w) } else { (w, 0) };
            let xx = x + ox;
            let yy = y + oy;
            if xx >= 0 && yy >= 0 {
                out.push((xx as u32, yy as u32));
            }
        }
    }
}

/// `MapAttack` の本処理。`unit_key` が無ければ第 1 引数として渡された
/// ユニット種別の最初のインスタンスを攻撃元とみなす。weapon 名で武器を
/// 引き、`map_attack_area` で武器属性別の効果範囲を算出して、その中の
/// 対立勢力ユニットにダメージを与える。反撃 / 経験値 / 資金加算は無効。
pub(crate) fn map_attack(
    app: &mut App,
    unit_key: Option<&str>,
    weapon_name: &str,
    cx: u32,
    cy: u32,
) {
    // 攻撃側ユニットの解決: unit_key 指定時はそれを引き、無ければ任意の Player ユニットへ。
    let atk_idx = if let Some(k) = unit_key {
        app.database()
            .unit_instances
            .iter()
            .position(|u| matches_unit_handle(u, k))
    } else {
        app.database()
            .unit_instances
            .iter()
            .position(|u| u.party == crate::Party::Player)
    };
    let Some(atk_idx) = atk_idx else { return };

    let atk_unit_name = app.database().unit_instances[atk_idx]
        .unit_data_name
        .clone();
    let weapon = match app
        .database()
        .unit_by_name(&atk_unit_name)
        .and_then(|d| d.weapons.iter().find(|w| w.name == weapon_name))
        .cloned()
    {
        Some(w) => w,
        None => return,
    };

    let atk_pos = (
        app.database().unit_instances[atk_idx].x,
        app.database().unit_instances[atk_idx].y,
    );
    let area: std::collections::HashSet<(u32, u32)> = map_attack_area(&weapon, atk_pos, (cx, cy))
        .into_iter()
        .collect();

    // 攻撃側と敵対する全陣営 (味方↔ＮＰＣ 同盟は対象外、中立は対象) を攻撃対象に。
    let atk_party = app.database().unit_instances[atk_idx].party;
    // 対象 idx を集めて damage と armor で個別評価。
    let targets: Vec<usize> = app
        .database()
        .unit_instances
        .iter()
        .enumerate()
        .filter(|(_, u)| u.party.is_hostile_to(atk_party))
        .filter(|(_, u)| area.contains(&(u.x, u.y)))
        .map(|(i, _)| i)
        .collect();
    if targets.is_empty() {
        return;
    }
    // 攻撃側も実効値込みデータを使用 (育成 / 強化パーツ / 状態異常)。
    let Some((atk_pilot, atk_unit_data)) = app.database().effective_combat_data(atk_idx) else {
        return;
    };
    let atk_statuses: Vec<String> = app.database().unit_instances[atk_idx]
        .conditions
        .iter()
        .map(|c| c.name.clone())
        .collect();

    // マップ兵器の EN・残弾消費 (撃破による index 失効前に消費)。
    if let Some(wi) = app
        .database()
        .unit_by_name(&atk_unit_name)
        .and_then(|d| d.weapons.iter().position(|w| w.name == weapon_name))
    {
        app.consume_weapon_resources(atk_idx, wi);
    }

    // 対象を後ろから削除するため位置を巻き戻し
    let mut kills = 0usize;
    for &def_idx in targets.iter().rev() {
        let def_inst = app.database().unit_instances[def_idx].clone();
        let Some((def_pilot, def_unit)) = app.database().effective_combat_data(def_idx) else {
            continue;
        };
        // マップ未設定時は terrain_id=0 (平地) として進める。
        let terrain_id = app
            .database()
            .map
            .as_ref()
            .map(|m| m.cell(def_inst.x, def_inst.y).terrain_id)
            .unwrap_or(0);
        let def_hit_mod = app.database().terrain_hit_mod(terrain_id);
        let def_damage_mod = app.database().terrain_damage_mod(terrain_id);
        let def_statuses: Vec<String> =
            def_inst.conditions.iter().map(|c| c.name.clone()).collect();
        let preview = crate::combat::predict_with_status(
            &atk_pilot,
            &atk_unit_data,
            &weapon,
            &def_pilot,
            &def_unit,
            def_hit_mod,
            def_damage_mod,
            app.database().unit_instances[atk_idx].morale,
            def_inst.morale,
            &atk_statuses,
            &def_statuses,
        );
        // マップ攻撃は必中扱いに近い (ダメージのみ)
        app.database_mut().unit_instances[def_idx].damage += preview.damage;
        let remaining = def_unit.hp - app.database().unit_instances[def_idx].damage;
        if remaining <= 0 {
            // 精神コマンド「復活」: HP0 でも HP 全快で立ち上がる (1 回で消費)。通常戦闘と
            // 同様にマップ兵器の撃破でも復活を尊重する (撃破・破壊・全滅を発火しない)。
            if app.revive_if_possible(def_idx) {
                app.push_message(format!("{} は【復活】で立ち上がった！", def_unit.name));
            } else {
                // 通常戦闘と同様に 破壊 <name> / 全滅 <party> イベントを発火する。
                // これが無いとマップ兵器でラスト1機を撃破してもシナリオが進行しない。
                let (vp, vu) = (def_pilot.name.clone(), def_unit.name.clone());
                app.database_mut().remove_unit_at(def_idx);
                fire_destruction_labels(app, &vp, &vu);
                kills += 1;
            }
        }
    }
    // 勝敗判定 (マップ兵器でラスト1機撃破時の勝利確定)。
    app.check_victory();
    if kills > 0 {
        app.push_message(format!(
            "{} のマップ攻撃 [{}] で {} ユニット撃破",
            atk_pilot.nickname, weapon_name, kills
        ));
    } else {
        app.push_message(format!(
            "{} のマップ攻撃 [{}] が {} ユニットに着弾",
            atk_pilot.nickname,
            weapon_name,
            targets.len()
        ));
    }
}

/// 精神コマンド名 → 標準 SP コスト。SRC のデフォルト値に近似。
/// `sp.txt` で上書き対応するのは将来課題。
pub(crate) fn sp_cost_for(name: &str) -> i32 {
    match name {
        "必中" => 15,
        "集中" => 20,
        "ひらめき" => 20,
        "熱血" => 30,
        "魂" => 60,
        "気合" => 5,
        "不屈" => 25,
        "鉄壁" => 25,
        "加速" => 10,
        "信頼" => 25,
        "友情" => 30,
        "補給" => 50,
        "復活" => 90,
        "愛" => 90,
        "幸運" => 15,
        "祝福" => 40,
        "応援" => 10,
        "脱力" => 35,
        "覚醒" => 65,
        "再動" => 50,
        "突撃" => 25,
        "奇跡" => 75,
        // SRC 標準以外 (シナリオ独自) の精神コマンドのコストはここに持たない。
        // 既定 0 にフォールバックし、コストは sp.txt (scenario の SpecialPowerData) で解決する。
        _ => 0,
    }
}

fn parse_party(s: &str, line: usize) -> Result<Party, ScriptError> {
    match s {
        "Player" | "味方" => Ok(Party::Player),
        // ＮＰＣ はコンピューター操作のプレイヤー側陣営。SRC 正準名は "ＮＰＣ"。
        // "友軍"/"Allied" は旧移植の後方互換エイリアス。
        "NPC" | "ＮＰＣ" | "Allied" | "友軍" => Ok(Party::Npc),
        "Enemy" | "敵" => Ok(Party::Enemy),
        "Neutral" | "中立" => Ok(Party::Neutral),
        _ => Err(err(line, &format!("Party 値が不正: {s:?}"))),
    }
}

fn err(line_num: usize, message: &str) -> ScriptError {
    ScriptError {
        line_num,
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::event;

    #[test]
    fn execute_basic_script() {
        let src = "\
Stage \"序章 — 起動\"
MapSize 4 3
SetTile 1 1 2
Pilot \"リオ・カザミ\" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit \"ブレイバー\" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon \"ブレイバー\" \"ビームライフル\" 2500 2 5 15 -1
Place \"ブレイバー\" \"リオ・カザミ\" Player 0 0
Turn 1
Message \"出撃せよ\"
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();

        assert_eq!(app.stage(), "序章 — 起動");
        let map = app.database().map.as_ref().unwrap();
        assert_eq!(map.width, 4);
        assert_eq!(map.height, 3);
        assert_eq!(map.cell(1, 1).terrain_id, 2);
        assert_eq!(app.database().pilots.len(), 1);
        assert_eq!(app.database().units.len(), 1);
        assert_eq!(app.database().units[0].weapons.len(), 1);
        assert_eq!(app.database().unit_instances.len(), 1);
        assert_eq!(app.turn().number, 1);
        assert_eq!(app.messages(), &["出撃せよ".to_string()]);
    }

    #[test]
    fn unknown_command_is_ignored() {
        let src = "ThisCommandDoesNotExist 1 2 3\nStage \"x\"\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.stage(), "x");
    }

    #[test]
    fn invalid_args_returns_error() {
        let src = "MapSize abc 3\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        let e = execute(&mut app, &stmts).unwrap_err();
        assert_eq!(e.line_num, 1);
    }

    #[test]
    fn set_and_expand_variable() {
        let src = "\
Set 宿名 女神の居眠り亭
Stage $(宿名)
Message $(宿名)
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("宿名"), "女神の居眠り亭");
        assert_eq!(app.stage(), "女神の居眠り亭");
        assert_eq!(app.messages(), &["女神の居眠り亭".to_string()]);
    }

    #[test]
    fn dollar_paren_evaluates_function_calls() {
        // `$(Func(...))` は変数キー lookup ではなく関数として評価される。
        // スパロボ戦記タイトルの `Set ユニット画像[i]
        // "Anime\Unit\$(Lindex(タイトル画面アクション[i],1))"` がこの経路。
        // 引数にインデックス変数 `配列[i]` を含むケースも解決できること。
        let src = "\
Set act[真イーグル] List(TrueEagle1.bmp,TrueEagle2.bmp)
Foreach i In act
　Set 画像[i] \"Anime\\Unit\\$(Lindex(act[i],1))\"
Next
Set 結果 $(画像[真イーグル])
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("結果"), "Anime\\Unit\\TrueEagle1.bmp");
    }

    #[test]
    fn local_alias_of_set() {
        let src = "Local x 42\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("x"), "42");
    }

    /// テストヘルパ: src を実行して App を返す。
    fn run_script(src: &str) -> App {
        let stmts = event::parse(src).expect("parse");
        let mut app = App::new();
        execute(&mut app, &stmts).expect("execute");
        app
    }

    #[test]
    fn virtual_file_write_then_read_roundtrip() {
        // `Open`/`Print`/`Close` で書いた仮想ファイルを `入力` で開き直して
        // `Read` で 1 行ずつ読み戻せる。
        let app = run_script(
            "Open \"test.txt\" For 出力 As F\n\
             Print F ライン1\n\
             Print F ライン2\n\
             Close F\n\
             Open \"test.txt\" For 入力 As G\n\
             Read G 行A\n\
             Read G 行B\n\
             Read G 行C\n\
             Close G\n",
        );
        assert_eq!(app.script_var("行A"), "ライン1");
        assert_eq!(app.script_var("行B"), "ライン2");
        // 末尾を超えた Read は空文字。
        assert_eq!(app.script_var("行C"), "");
    }

    #[test]
    fn virtual_file_output_mode_truncates() {
        // `出力` で開き直すと旧内容は切り詰められる。
        let app = run_script(
            "Open \"x.txt\" For 出力 As F\nPrint F 古い\nClose F\n\
             Open \"x.txt\" For 出力 As F\nPrint F 新しい\nClose F\n\
             Open \"x.txt\" For 入力 As G\nRead G 行\nClose G\n",
        );
        assert_eq!(app.script_var("行"), "新しい");
        assert_eq!(
            app.virtual_file_lines("x.txt"),
            Some(&["新しい".to_string()][..])
        );
    }

    #[test]
    fn virtual_pilot_file_close_registers_pilot_data() {
        // `pilot.txt` を仮想ファイルに書き出して `Close` すると、内容が
        // 再パースされ PilotData として GameDatabase に登録される
        // (キャラメイキングが書き出すキャラを使用可能にする経路)。
        let app = run_script(
            "Open \"data\\一時フォルダ\\pilot.txt\" For 出力 As F\n\
             Print F\n\
             Print F\n\
             Print F テストキャラ\n\
             Print F テス, -, 汎用, AAAA, 100\n\
             Print F 特殊能力\n\
             Print F 150, 150, 150, 150, 150, 150\n\
             Close F\n",
        );
        assert!(
            app.database().pilot_by_name("テストキャラ").is_some(),
            "Close 時に pilot.txt が再パースされ PilotData が登録される"
        );
    }

    #[test]
    fn two_dimensional_array_index_resolves() {
        // `name[i,j]` 多次元添字: リテラル添字 (`[1,3]`) と変数添字
        // (`[i,j]` で i=1,j=3) が同じキーを指し、Set と読出が一致する。
        // スパロボ戦記 AlphaSecond.eve の `搭乗員[i,j]` 相当。
        let app = run_script(
            "Set 搭乗員[1,3] リオ\n\
             Set i 1\n\
             Set j 3\n\
             Set lit $(搭乗員[1,3])\n\
             Set var $(搭乗員[i,j])\n\
             Set d1 グフ\n\
             Set arr[5] $(d1)\n\
             Set one $(arr[5])\n",
        );
        assert_eq!(app.script_var("lit"), "リオ", "リテラル添字 [1,3]");
        assert_eq!(app.script_var("var"), "リオ", "変数添字 [i,j]");
        assert_eq!(app.script_var("搭乗員[1,3]"), "リオ", "正規化キー");
        assert_eq!(app.script_var("one"), "グフ", "1 次元添字は従来どおり");
    }

    #[test]
    fn custom_unit_commands_are_collected_not_label_registered() {
        // `*ユニットコマンド` / `マップコマンド` 行はメニュー項目として
        // 収集され、フラットな `labels` には `ユニットコマンド` で誤登録されない。
        // また `-*ユニットコマンド` は post_act_ok=true として収集される
        // (無効化マーカーではなく「行動終了後も使用可能」を意味する)。
        let src = "\
*ユニットコマンド 乗せ換え 味方 Call(乗せ換え確認):
Return

ユニットコマンド 換装 味方:
Return

-*ユニットコマンド ステータス 全:
Return

マップコマンド 全体表示:
Return
";
        let stmts = event::parse(src).expect("parse");
        let mut app = App::new();
        app.script_library_mut().append(&stmts);
        let cc = &app.script_library().custom_commands;
        assert_eq!(cc.len(), 4, "全 4 件が収集される");

        let nori = cc.iter().find(|c| c.name == "乗せ換え").expect("乗せ換え");
        assert!(nori.is_unit);
        assert_eq!(nori.target, "味方");
        assert_eq!(
            nori.condition.as_deref(),
            Some("Call(乗せ換え確認)"),
            "条件式は Call() ラッパーごと保存される"
        );
        assert!(nori.post_move_ok, "* プレフィックス → post_move_ok=true");
        assert!(!nori.post_act_ok);

        let kanso = cc.iter().find(|c| c.name == "換装").expect("換装");
        assert_eq!(kanso.condition, None, "条件なしは None");
        assert!(!kanso.post_move_ok, "プレフィックスなし → デフォルト");
        assert!(!kanso.post_act_ok);

        let status = cc
            .iter()
            .find(|c| c.name == "ステータス")
            .expect("ステータス");
        assert!(status.is_unit);
        assert!(
            !status.post_move_ok,
            "-* プレフィックス → post_move_ok=false"
        );
        assert!(status.post_act_ok, "-* プレフィックス → post_act_ok=true");

        let map = cc.iter().find(|c| c.name == "全体表示").expect("全体表示");
        assert!(!map.is_unit, "マップコマンドは is_unit=false");

        assert!(
            !app.script_library().labels.contains_key("ユニットコマンド"),
            "custom command 行を `ユニットコマンド` ラベルに誤登録しない"
        );
    }

    #[test]
    fn custom_command_prefix_variants() {
        // 全プレフィックスバリアントの post_move_ok / post_act_ok フラグを検証。
        let src = "\
ユニットコマンド デフォルト 全:
Return

*ユニットコマンド ポストムーブ 全:
Return

*-ユニットコマンド ポストムーブ2 全:
Return

-*ユニットコマンド ポストアクト 全:
Return

**ユニットコマンド 両方 全:
Return
";
        let stmts = event::parse(src).expect("parse");
        let mut app = App::new();
        app.script_library_mut().append(&stmts);
        let cc = &app.script_library().custom_commands;
        assert_eq!(cc.len(), 5);

        let find = |nm: &str| {
            cc.iter()
                .find(|c| c.name == nm)
                .unwrap_or_else(|| panic!("{nm} not found"))
        };

        let d = find("デフォルト");
        assert!(!d.post_move_ok && !d.post_act_ok, "no prefix → both false");

        let pm = find("ポストムーブ");
        assert!(pm.post_move_ok && !pm.post_act_ok, "* → post_move_ok only");

        let pm2 = find("ポストムーブ2");
        assert!(
            pm2.post_move_ok && !pm2.post_act_ok,
            "*- → post_move_ok only"
        );

        let pa = find("ポストアクト");
        assert!(!pa.post_move_ok && pa.post_act_ok, "-* → post_act_ok only");

        let both = find("両方");
        assert!(both.post_move_ok && both.post_act_ok, "** → both true");
    }

    #[test]
    fn ampersand_concat_inside_parentheses() {
        // `("a" & Func())` のように括弧で 1 トークンに括られた `&` 連結。
        // tokenizer は `(...)` を 1 トークンに保つため `collapse_concat`
        // には捕まらず、`expand_arg` がトークン内部の `&` を畳む。
        // スパロボ戦記 Include.eve の `Ride ("$(A)" & "＋" & "$(B)")` 型。
        let app = run_script("Set p (\"u_\" & Left(\"ブレイバー\", 2))\n");
        assert_eq!(app.script_var("p"), "u_ブレ");
    }

    #[test]
    fn ampersand_concat_inside_parentheses_with_dollar_vars() {
        // 括弧付き `&` 連結のオペランドが `$(var)` リテラル。
        let app = run_script("Set 左 リオ\nSet 右 ガロ\nSet p (\"$(左)\" & \"＋\" & \"$(右)\")\n");
        assert_eq!(app.script_var("p"), "リオ＋ガロ");
    }

    #[test]
    fn set_evaluates_parenthesized_arithmetic() {
        // SRC の `Set` は値を式評価する。`&` を含まない括弧式は連結扱い
        // されずトークンとして残るが、値全体が単一の括弧式なら数値化する。
        let app = run_script("Set v (200 - 128 / 2)\n");
        assert_eq!(app.script_var("v"), "136");
    }

    #[test]
    fn set_evaluates_parenthesized_arithmetic_with_variable() {
        // 括弧式中の裸変数 (`進行度`) も script_var として解決して評価する。
        // スパロボ戦記 Main.eve `Set 敵配置数 (RoundUp(…) + (進行度 / 5))`
        // 相当 — 関数評価後に残る `(5 + (進行度 / 5))` を数値化する。
        let app = run_script("Set 進行度 10\nSet n (5 + (進行度 / 5))\n");
        assert_eq!(app.script_var("n"), "7");
    }

    #[test]
    fn set_paren_with_non_numeric_atom_kept_verbatim() {
        // 非数値アトムを含む括弧式は算術ではないとみなし、誤って 0 に
        // 潰さず文字列のまま残す (`Set msg (こんにちは)` を壊さない)。
        let app = run_script("Set msg (こんにちは)\n");
        assert_eq!(app.script_var("msg"), "(こんにちは)");
    }

    #[test]
    fn set_non_parenthesized_arithmetic_kept_verbatim() {
        // 括弧で囲われていない値は従来どおり式評価しない
        // (`Set 型番 RX-78-2` を `-53` と誤評価しないため)。
        let app = run_script("Set 型番 78-2\n");
        assert_eq!(app.script_var("型番"), "78-2");
    }

    #[test]
    fn if_true_executes_then_branch() {
        let src = "\
Set x 1
If $(x) = 1 Then
  Message yes
Else
  Message no
EndIf
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["yes".to_string()]);
    }

    #[test]
    fn if_false_falls_through_else() {
        let src = "\
Set x 0
If $(x) = 1 Then
  Message yes
Else
  Message no
EndIf
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["no".to_string()]);
    }

    #[test]
    fn elseif_chain_picks_matching_branch() {
        let src = "\
Set x 2
If $(x) = 1 Then
  Message a
ElseIf $(x) = 2 Then
  Message b
ElseIf $(x) = 3 Then
  Message c
Else
  Message d
EndIf
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["b".to_string()]);
    }

    #[test]
    fn goto_jumps_to_label() {
        let src = "\
Goto end:
Message skipped
end:
Message arrived
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["arrived".to_string()]);
    }

    #[test]
    fn talk_pauses_with_dialog_and_logs_message() {
        let src = "\
Talk 番頭さん
お帰りなさいませ
ごゆるりとどうぞ
End
Message after_talk
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // 対話モーダル中: after_talk はまだ実行されていない
        assert!(app.pending_dialog().is_some());
        match app.pending_dialog().unwrap() {
            crate::PendingDialog::Talk { speaker, body } => {
                assert_eq!(speaker, "番頭さん");
                assert!(body.contains("お帰りなさいませ"));
                assert!(body.contains("ごゆるりとどうぞ"));
            }
            _ => panic!("expected Talk dialog"),
        }
        // ログには既に積まれている
        assert_eq!(app.messages().len(), 1);
        assert!(app.messages()[0].starts_with("【番頭さん】"));
        // 任意キーで進行（choice 値は無視）
        assert!(app.respond_dialog(0));
        // 続きが流れる
        assert_eq!(
            app.messages().last().map(String::as_str),
            Some("after_talk")
        );
    }

    #[test]
    fn paren_arith_missing_operand_defaults_to_zero() {
        // SRC: 未定義数値は 0。Info(...) 等が空に解決して `)` 直前に二項演算子が
        // 残っても括弧算術が評価できること (スパロボ戦記 AlphaSecond の
        // `次のレベル(500 - )` / `(500 * 0 - 500 + )` 由来の生式表示の防止)。
        let app = App::new();
        assert_eq!(
            eval_paren_arith_value(&app, "(500 - )"),
            Some("500".to_string())
        );
        assert_eq!(
            eval_paren_arith_value(&app, "(500 * 0 - 500 + )"),
            Some("-500".to_string())
        );
        // 通常の括弧算術は従来どおり評価される (回帰防止)。
        assert_eq!(
            eval_paren_arith_value(&app, "(2 + 3)"),
            Some("5".to_string())
        );
        // try_eval_num 自体は厳格なまま (配列添字解決の fail→fallback を壊さない)。
        assert_eq!(try_eval_num("(500 - )"), None);
    }

    #[test]
    fn block_if_skip_ignores_inner_single_line_if_goto() {
        // スパロボ戦記 AlphaSecond L67: ブロック If の中に単一行 `If cond Goto label`
        // があると、EndIf スキップの深さ計数が単一行 If を 1 段と誤数えして対応する
        // EndIf を見失い「If に対応する EndIf が見つかりません」になる回帰の防止。
        let src = "\
Set フラグ 0
If フラグ = 1 Then
  If フラグ = 9 Goto どこか
  Set 中身 実行
EndIf
Set 後 到達
どこか:
Exit
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // フラグ=0 なのでブロック If は偽 → EndIf を正しく見つけて後続へ。
        assert_eq!(
            app.script_var("後"),
            "到達",
            "ブロック If の EndIf を見失っている"
        );
        assert_eq!(
            app.script_var("中身"),
            "",
            "偽のブロック If 本体が実行された"
        );
        assert!(
            app.last_script_error().is_none(),
            "EndIf 探索エラー: {:?}",
            app.last_script_error()
        );
    }

    #[test]
    fn right_click_exits_wait_click_via_keystate2() {
        // スパロボ戦記 AlphaSecond のステータス画面: `Wait Click` → 右クリックで
        // `選択 = ""` かつ `KeyState(2) = 1` となり `Case "" → If KeyState(2) Then Break`
        // で画面を抜けられること。右クリック未対応だと抜け出せず詰む回帰の防止。
        let src = "\
状態画面:
Hotpoint タブ 10 10 50 50
Do
  Wait Click
  Switch 選択
  Case タブ
    Set 結果 タブ選択
    Break
  Case \"\"
    If KeyState(2) Then
      Set 結果 終了
      Break
    EndIf
  EndSw
Loop While (1)
Exit
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        app.script_library_mut().append(&stmts);
        crate::event_runtime::trigger_label(&mut app, "状態画面");
        // Wait Click で中断している
        assert!(
            app.pending_dialog().is_some(),
            "Wait Click で中断していない"
        );
        // 右クリック → 選択="" + KeyState(2)=1 → ループ脱出
        assert!(app.respond_dialog_right_click());
        assert!(
            app.pending_dialog().is_none(),
            "右クリックで Wait Click を抜けられていない"
        );
        assert_eq!(app.script_var("結果"), "終了");
    }

    #[test]
    fn talk_colon_pages_and_semicolon_linebreak() {
        // SRC Talkコマンド.md: 半角 `;` は強制改行、半角 `:` は段階表示の区切り。
        // musou202 の難易度選択 "Normalね。:;情報が..." と同じ構造。
        let src = "\
Talk パチュリー
Normalね。:;情報がだいたい表示されたり、色々と普通だと思うわ。
End
Message done
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // 1 ページ目: `:` より前。`:`/`;` の生文字は出ない。
        match app.pending_dialog().unwrap() {
            crate::PendingDialog::Talk { speaker, body } => {
                assert_eq!(speaker, "パチュリー");
                assert_eq!(body, "Normalね。");
                assert!(!body.contains(':') && !body.contains(';'));
            }
            _ => panic!("expected Talk dialog (page 1)"),
        }
        // 応答 → 2 ページ目 (`;` は改行に変換)。スクリプトはまだ再開しない。
        assert!(app.respond_dialog(0));
        match app.pending_dialog().unwrap() {
            crate::PendingDialog::Talk { speaker, body } => {
                assert_eq!(speaker, "パチュリー");
                assert_eq!(body, "情報がだいたい表示されたり、色々と普通だと思うわ。");
            }
            _ => panic!("expected Talk dialog (page 2)"),
        }
        // done はまだ
        assert_ne!(app.messages().last().map(String::as_str), Some("done"));
        // 最終ページ応答 → スクリプト再開
        assert!(app.respond_dialog(0));
        assert!(app.pending_dialog().is_none());
        assert_eq!(app.messages().last().map(String::as_str), Some("done"));
    }

    #[test]
    fn multiple_talks_pause_one_at_a_time() {
        let src = "\
Talk A
hello
End
Talk B
world
End
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert!(matches!(
            app.pending_dialog(),
            Some(crate::PendingDialog::Talk { speaker, .. }) if speaker == "A"
        ));
        app.respond_dialog(0);
        assert!(matches!(
            app.pending_dialog(),
            Some(crate::PendingDialog::Talk { speaker, .. }) if speaker == "B"
        ));
        app.respond_dialog(0);
        assert!(app.pending_dialog().is_none());
    }

    #[test]
    fn talk_terminated_by_suspend_only_shows_preceding_body() {
        // SRC Talkコマンド.md: Suspend は Talk ブロックの終端として機能する。
        // 重要: Suspend の後に続くコマンド ("PlaySound" 等) がダイアログ本文に
        // 混入してはならない。musou202.lzh の "Talk システム / ... / Suspend" と
        // 同じ構造。
        let src = "\
Talk システム
少女祈祷中...
Suspend
Message after_suspend
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        app.set_scene(crate::Scene::MapView);
        execute(&mut app, &stmts).unwrap();
        // Suspend が Talk 終端なので pending_dialog が立つ
        assert!(app.pending_dialog().is_some());
        match app.pending_dialog().unwrap() {
            crate::PendingDialog::Talk { speaker, body } => {
                assert_eq!(speaker, "システム");
                assert_eq!(body, "少女祈祷中...");
                // Suspend や後続コマンドが本文に混入していない
                assert!(!body.contains("Suspend"));
                assert!(!body.contains("after_suspend"));
            }
            _ => panic!("expected Talk dialog"),
        }
        // ダイアログ応答後、Suspend は消費済みなのでタイトル復帰しない (Scene そのまま)
        app.respond_dialog(0);
        assert_eq!(
            app.messages().last().map(String::as_str),
            Some("after_suspend")
        );
        assert_eq!(app.scene(), crate::Scene::MapView);
    }

    #[test]
    fn talk_position_args_not_included_in_speaker() {
        // `Talk システム 8 8` → speaker は "システム" のみ。
        // 座標引数 "8 8" がスピーカー名に混入してはならない。
        let src = "\
Talk システム 8 8
メッセージ
End
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        match app.pending_dialog().unwrap() {
            crate::PendingDialog::Talk { speaker, body } => {
                assert_eq!(speaker, "システム");
                assert!(
                    !speaker.contains('8'),
                    "座標がスピーカーに混入: {speaker:?}"
                );
                assert_eq!(body, "メッセージ");
            }
            _ => panic!("expected Talk dialog"),
        }
    }

    #[test]
    fn talk_parenthesized_position_not_in_speaker() {
        // `Talk 霊夢 (8,6)` → speaker は "霊夢" のみ。
        let src = "\
Talk 霊夢 (8,6)
春ね…
End
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        match app.pending_dialog().unwrap() {
            crate::PendingDialog::Talk { speaker, body } => {
                assert_eq!(speaker, "霊夢");
                assert_eq!(body, "春ね…");
            }
            _ => panic!("expected Talk dialog"),
        }
    }

    #[test]
    fn talk_body_html_tags_stripped() {
        // SRC Talk ボディの書式タグは除去される。
        // タグは常にテキスト行の中に埋め込まれる (行頭に単独で書かれない)。
        // <B>/</B>/<I>/</I>/<COLOR=...>/</COLOR>/<SIZE=n>/</SIZE> など。
        // <LT> → '<'、<GT> → '>' への変換も確認。
        // musou202.lzh 東方夢想伝01.eve の実例と同型。
        let src = "\
Talk 霊夢
一つは<B>『ボーダーボーナス』</B>…名前はお馴染みよね？
さらに<I>強調テキスト</I>もあるわ。
<LT>タグ<GT>を含む行
End
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        match app.pending_dialog().unwrap() {
            crate::PendingDialog::Talk { body, .. } => {
                // タグ除去済み
                assert!(!body.contains("<B>"), "body={body:?}");
                assert!(!body.contains("</B>"), "body={body:?}");
                assert!(!body.contains("<I>"), "body={body:?}");
                assert!(!body.contains("</I>"), "body={body:?}");
                // テキスト内容は残っている
                assert!(body.contains("『ボーダーボーナス』"), "body={body:?}");
                assert!(body.contains("強調テキスト"), "body={body:?}");
                // <LT>/<GT> 変換
                assert!(body.contains('<'), "body={body:?}");
                assert!(body.contains('>'), "body={body:?}");
                assert!(!body.contains("<LT>"), "body={body:?}");
                assert!(!body.contains("<GT>"), "body={body:?}");
            }
            _ => panic!("expected Talk dialog"),
        }
    }

    #[test]
    fn talk_inner_talk_splits_into_multiple_dialogs() {
        // SRC 仕様: Talk ブロック内の Talk コマンドは話者切り替え。
        // メインループが次の Talk を実行するため、各話者のメッセージが
        // それぞれ別のダイアログとして表示される。
        // musou202.lzh "Talk 魔理沙 / ... / Talk パチュリー / ... / End" と同構造。
        let src = "\
Talk 魔理沙
よっ、魔理沙だぜ。
Talk パチュリー
パチュリーよ。
End
Message done
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // 最初は 魔理沙 のダイアログ
        assert!(matches!(
            app.pending_dialog(),
            Some(crate::PendingDialog::Talk { speaker, .. }) if speaker == "魔理沙"
        ));
        if let crate::PendingDialog::Talk { body, .. } = app.pending_dialog().unwrap() {
            assert_eq!(body, "よっ、魔理沙だぜ。");
            // "Talk パチュリー" がテキストとして混入していない
            assert!(!body.contains("Talk"));
        }
        app.respond_dialog(0);
        // 次は パチュリー のダイアログ
        assert!(matches!(
            app.pending_dialog(),
            Some(crate::PendingDialog::Talk { speaker, .. }) if speaker == "パチュリー"
        ));
        if let crate::PendingDialog::Talk { body, .. } = app.pending_dialog().unwrap() {
            assert_eq!(body, "パチュリーよ。");
        }
        app.respond_dialog(0);
        assert!(app.pending_dialog().is_none());
        assert_eq!(app.messages().last().map(String::as_str), Some("done"));
    }

    #[test]
    fn win_lose_set_stage_state() {
        let mut app = App::new();
        let stmts = event::parse("Win\n").unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.stage_state(), crate::stage::StageState::Victory);

        let mut app2 = App::new();
        let stmts = event::parse("Lose\n").unwrap();
        execute(&mut app2, &stmts).unwrap();
        assert_eq!(app2.stage_state(), crate::stage::StageState::Defeat);
    }

    #[test]
    fn confirm_pauses_and_resumes() {
        // SRC `Confirm` 仕様: Yes → 選択=1, No → 選択=0。
        let src = "\
Confirm 続けますか
If $(選択) = 1 Then
  Message yes
Else
  Message no
EndIf
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // Confirm でモーダル化、まだ if 分岐は走っていない
        assert!(app.pending_dialog().is_some());
        match app.pending_dialog().unwrap() {
            crate::PendingDialog::Confirm { question, var_name } => {
                assert_eq!(question, "続けますか");
                assert_eq!(var_name, "選択");
            }
            _ => panic!("expected Confirm dialog"),
        }
        // Yes (内部 choice=0 → 選択="1") で応答
        assert!(app.respond_dialog(0));
        assert_eq!(app.script_var("選択"), "1");
        assert_eq!(app.messages(), &["yes".to_string()]);
        // No (内部 choice=1 → 選択="0") ならもう一度走らせて確認
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert!(app.respond_dialog(1));
        assert_eq!(app.script_var("選択"), "0");
        assert_eq!(app.messages(), &["no".to_string()]);
    }

    #[test]
    fn anchor_resolves_for_goto() {
        let src = "\
Goto onsen
Message skipped
@onsen
Message arrived
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["arrived".to_string()]);
    }

    #[test]
    fn menu_pauses_with_options() {
        let src = "\
Menu 行動を選んでください
攻撃
防御
逃走
End
If $(選択) = 2 Then
  Message defend
EndIf
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        match app.pending_dialog().unwrap() {
            crate::PendingDialog::Menu {
                prompt,
                options,
                var_name,
                ..
            } => {
                assert_eq!(prompt, "行動を選んでください");
                assert_eq!(options.len(), 3);
                assert_eq!(options[0], "攻撃");
                assert_eq!(options[1], "防御");
                assert_eq!(options[2], "逃走");
                assert_eq!(var_name, "選択");
            }
            _ => panic!("expected Menu"),
        }
        // 2 = 防御 を選ぶ
        app.respond_dialog(2);
        assert_eq!(app.script_var("選択"), "2");
        assert_eq!(app.messages(), &["defend".to_string()]);
    }

    #[test]
    fn input_command_pauses_and_uses_text() {
        let src = "\
Input name 名前を入力 太郎
Stage $(name)
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // Input でモーダル中、default が暫定セットされている
        assert!(matches!(
            app.pending_dialog(),
            Some(crate::PendingDialog::Input { default, .. }) if default == "太郎"
        ));
        assert_eq!(app.script_var("name"), "太郎");
        // ユーザがテキスト入力して確定
        assert!(app.respond_dialog_text("二郎".to_string()));
        assert_eq!(app.script_var("name"), "二郎");
        assert_eq!(app.stage(), "二郎");
    }

    #[test]
    fn input_default_kept_when_canceled_via_respond_dialog() {
        let src = "Input name 名前 太郎\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // 数値応答 (DialogYes 相当) は default を保持して進む
        assert!(app.respond_dialog(0));
        assert_eq!(app.script_var("name"), "太郎");
    }

    #[test]
    fn input_array_var_target_resolves_to_key_not_value() {
        // 回帰: 配列変数 `name[key]` への Input は、代入先を**現在値に値展開**しては
        // ならない。既に値が入った状態で Input すると、旧実装は前回値をキー名に化けさせ、
        // テキスト応答が元変数を更新しない不具合があった (D スパロボ戦記キャラメイキングの
        // 2 人目以降の名前入力が前回値のまま固まる原因)。
        let mut app = App::new();
        // 1 回目の入力相当で既に値が入っている状態を作る。
        app.set_script_var("召喚キャラ[名前]".to_string(), "417776".to_string());
        let stmts = event::parse("Input 召喚キャラ[名前] 名前を入力 \"\"\n").unwrap();
        execute(&mut app, &stmts).unwrap();
        // モーダルの var_name は格納キー (現在値 "417776" ではない)。
        assert!(matches!(
            app.pending_dialog(),
            Some(crate::PendingDialog::Input { var_name, .. }) if var_name == "召喚キャラ[名前]"
        ));
        // テキスト応答が正しいキーを更新する。
        assert!(app.respond_dialog_text("パイロイ".to_string()));
        assert_eq!(app.script_var("召喚キャラ[名前]"), "パイロイ");
    }

    #[test]
    fn expand_vars_keeps_indexed_var_literal_inside_quotes() {
        // 回帰: クオート内の `name[expr]` は、たまたま同名の配列変数が定義済みでも
        // **値に化けず literal のまま**にする (D データロードの行検出が
        // `Instr(v,"設定[パイロット一覧]")` のリテラル破壊で失敗していた真因)。
        let mut app = App::new();
        app.set_script_var("設定[パイロット一覧]".to_string(), "VAL".to_string());
        // クオート内は literal。
        assert_eq!(
            expand_vars(&app, "\"設定[パイロット一覧]\""),
            "\"設定[パイロット一覧]\""
        );
        // クオート外は従来どおり値に展開。
        assert_eq!(expand_vars(&app, "設定[パイロット一覧]"), "VAL");
        // `$(...)` 明示補間はクオート内でも従来どおり展開。
        app.set_script_var("x".to_string(), "Y".to_string());
        assert_eq!(expand_vars(&app, "\"$(x)\""), "\"Y\"");
    }

    #[test]
    fn if_instr_with_bracket_literal_matches() {
        // 回帰候補: `If Instr(v, "設定[パイロット一覧]")` のように、クオート内に [..] を含む
        // リテラルを条件で使ったとき正しくマッチするか (D データロードの行検出で使用)。
        let mut app = App::new();
        app.set_script_var(
            "v".to_string(),
            "Set 設定[パイロット一覧] 人工知能(ザコ) ".to_string(),
        );
        let src = "If Instr(v, \"設定[パイロット一覧]\") Then\nSet r found\nEndif\n";
        execute(&mut app, &event::parse(src).unwrap()).unwrap();
        assert_eq!(app.script_var("r"), "found");
    }

    #[test]
    fn local_multivar_decl_does_not_break_read() {
        // `Local F 仮変数` (複数名宣言) の後、`仮変数` を Set→Instr で読めるか。
        // (D データロードはこの形を使う。本実装の Local は Set 相当なので
        //  `Local F 仮変数` = `Set F 仮変数` になるが、仮変数 自体は別途 Set で埋まる。)
        let mut app = App::new();
        let src = "Local F 仮変数\nSet 仮変数 \"設定[パイロット一覧] 人工知能(ザコ) \"\nIf Instr(仮変数, \"設定[パイロット一覧]\") Then\nSet r found\nEndif\n";
        execute(&mut app, &event::parse(src).unwrap()).unwrap();
        assert_eq!(
            app.script_var("r"),
            "found",
            "Local 宣言後の 仮変数 読みが壊れている"
        );
    }

    #[test]
    fn loadfiledialog_returns_verify_var_else_empty() {
        // 実機はファイル選択ダイアログ。ヘッドレスでは未設定なら "" (キャンセル相当)、
        // `__verify_loadfile` 設定時はそのパスを返す (検証ドライバの `データロード` 駆動用)。
        let mut app = App::new();
        execute(
            &mut app,
            &event::parse("Set r LoadFileDialog(プレイデータ, src)\n").unwrap(),
        )
        .unwrap();
        assert_eq!(app.script_var("r"), "");
        app.set_script_var(
            "__verify_loadfile".to_string(),
            "/save/test.src".to_string(),
        );
        execute(
            &mut app,
            &event::parse("Set r2 LoadFileDialog(プレイデータ, src)\n").unwrap(),
        )
        .unwrap();
        assert_eq!(app.script_var("r2"), "/save/test.src");
    }

    #[test]
    fn moveunit_relocates_instance() {
        let src = "\
MapSize 6 5
Pilot \"リオ・カザミ\" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit \"ブレイバー\" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Place \"ブレイバー\" \"リオ・カザミ\" Player 0 0
MoveUnit ブレイバー 3 2
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let u = &app.database().unit_instances[0];
        assert_eq!((u.x, u.y), (3, 2));
    }

    #[test]
    fn damage_command_reduces_hp_and_destroys() {
        let src = "\
MapSize 4 4
Pilot \"P\" P 男性 c AAAA 10 100 100 100 100 100 100
Unit \"ゾルダII\" リアル系 1 0 陸 5 M 100 50 2400 80 900 80 AAAA
Place \"ゾルダII\" \"P\" Enemy 1 1
Damage ゾルダII 1000
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.database().unit_instances[0].damage, 1000);

        let src2 = "Damage ゾルダII 9999\n";
        let stmts2 = event::parse(src2).unwrap();
        execute(&mut app, &stmts2).unwrap();
        assert!(app.database().unit_instances.is_empty());
    }

    #[test]
    fn heal_command_restores_hp() {
        let src = "\
MapSize 4 4
Pilot \"P\" P 男性 c AAAA 10 100 100 100 100 100 100
Unit \"ゾルダII\" リアル系 1 0 陸 5 M 100 50 2400 80 900 80 AAAA
Place \"ゾルダII\" \"P\" Enemy 1 1
Damage ゾルダII 1500
Heal ゾルダII 500
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.database().unit_instances[0].damage, 1000);
    }

    #[test]
    fn kill_command_removes_instance() {
        let src = "\
MapSize 4 4
Pilot \"P\" P 男性 c AAAA 10 100 100 100 100 100 100
Unit \"ゾルダII\" リアル系 1 0 陸 5 M 100 50 2400 80 900 80 AAAA
Place \"ゾルダII\" \"P\" Enemy 1 1
Kill ゾルダII
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert!(app.database().unit_instances.is_empty());
    }

    #[test]
    fn isdead_predicate_in_if() {
        let src = "\
MapSize 4 4
Pilot \"P\" P 男性 c AAAA 10 100 100 100 100 100 100
Unit \"U\" リアル系 1 0 陸 5 M 100 50 1000 80 900 80 AAAA
Place \"U\" \"P\" Enemy 1 1
If IsAlive U Then
  Message still_there
EndIf
Kill U
If IsDead U Then
  Message gone
EndIf
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(
            app.messages(),
            &["still_there".to_string(), "gone".to_string()]
        );
    }

    fn setup_two_units(app: &mut App) {
        let src = "\
MapSize 6 5
Pilot \"リオ\" リオ 男性 c AAAA 100 100 100 100 100 100 100
Unit \"ブレイバー\" リアル系 1 0 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Place \"ブレイバー\" \"リオ\" Player 1 1
Pilot \"ガロ\" ガロ 男性 c AAAA 100 100 100 100 100 100 100
Unit \"ゾルダII\" リアル系 1 0 陸 5 M 800 200 2400 80 900 80 AAAA
Place \"ゾルダII\" \"ガロ\" Enemy 4 3
";
        let stmts = event::parse(src).unwrap();
        execute(app, &stmts).unwrap();
    }

    #[test]
    fn hp_and_maxhp_functions() {
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "\
Set h HP(ブレイバー)
Set mh MaxHP(ブレイバー)
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("h"), "3500");
        assert_eq!(app.script_var("mh"), "3500");
        // ダメージを与えると HP が下がる
        let stmts2 = event::parse("Damage ブレイバー 1000\nSet h HP(ブレイバー)\n").unwrap();
        execute(&mut app, &stmts2).unwrap();
        assert_eq!(app.script_var("h"), "2500");
    }

    #[test]
    fn distance_function() {
        let mut app = App::new();
        setup_two_units(&mut app);
        // ブレイバー (1,1) と ゾルダII (4,3) は |3| + |2| = 5
        let src = "Set d Distance(ブレイバー, ゾルダII)\n";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("d"), "5");
    }

    #[test]
    fn count_party_function() {
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "\
Set ally Count(\"Player\")
Set foe Count(\"Enemy\")
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("ally"), "1");
        assert_eq!(app.script_var("foe"), "1");
    }

    #[test]
    fn exists_function() {
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "\
Set a Exists(ブレイバー)
Set b Exists(アークシップ)
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("a"), "1");
        assert_eq!(app.script_var("b"), "0");
    }

    #[test]
    fn position_functions() {
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "Set gx X(ブレイバー)\nSet gy Y(ブレイバー)\n";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("gx"), "1");
        assert_eq!(app.script_var("gy"), "1");
    }

    #[test]
    fn random_function_in_range() {
        // SRC `Random(n)` は 1..n (GeneralLib.Dice 準拠)。
        let mut app = App::new();
        for _ in 0..50 {
            execute(&mut app, &event::parse("Set r Random(10)\n").unwrap()).unwrap();
            let r: u32 = app.script_var("r").parse().unwrap();
            assert!((1..=10).contains(&r), "Random(10)={r} は 1..=10 外");
        }
    }

    #[test]
    fn create_command_adds_instance() {
        // SRC `Create party unit rank pilot level x y` 7-引数構文
        let mut app = App::new();
        setup_two_units(&mut app);
        // "Allied" は後方互換エイリアス → Party::Npc に解決される。
        let src = "Create Allied ブレイバー 1 リオ 10 0 0\n";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.database().unit_instances.len(), 3);
        assert_eq!(app.database().unit_instances[2].party, crate::Party::Npc);
        assert_eq!(
            app.database().unit_instances[2].unit_data_name,
            "ブレイバー"
        );
        assert_eq!(app.database().unit_instances[2].pilot_name, "リオ");
    }

    #[test]
    fn settile_and_place_resolve_loop_variable_in_coordinates() {
        // マップ構築の典型: For ループで SetTile の座標にループ変数を使う。
        // Place の座標も同様に式評価される。
        let src = "\
MapSize 5 5
For y = 0 to 4
SetTile 2 y 1
Next
Place ブレイバー リオ 味方 (2 + 1) 3
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // SetTile が列 x=2 の各行 (0..4) に terrain 1 を敷いた。
        let map = app.database().map.as_ref().unwrap();
        for y in 0..5u32 {
            assert_eq!(map.cell(2, y).terrain_id, 1, "SetTile(2,{y}) が未設定");
        }
        // Place は算術式座標 (2+1, 3) = (3,3) に配置。
        let placed = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "ブレイバー")
            .expect("Place されていない");
        assert_eq!((placed.x, placed.y), (3, 3));
    }

    #[test]
    fn system_variable_resolves_in_coordinate_expression() {
        // 味方レベル平均値 等のシステム変数が座標式で解決される (リンと凛 第038話 型:
        // `(味方レベル平均値 - 1)` を座標に使う)。
        let src = "\
MapSize 10 10
Place A pa 味方 0 0
Place B pb 味方 1 0
Create 味方 C 0 pc 0 (味方レベル平均値) 5
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // A,B はレベル 1 → 平均 1 (件数 2 ではない) → C は (x=1, y=5) に生成。
        let c = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "C")
            .expect("C が生成されていない");
        assert_eq!(
            (c.x, c.y),
            (1, 5),
            "味方レベル平均値 が座標式で解決されていない"
        );
    }

    #[test]
    fn create_resolves_loop_variable_in_coordinates() {
        // SRC は座標を式評価する。For ループ変数 `i` が Create の座標に裸で来ても
        // 解決される (天使ちゃんマジテトリス `基本システム.eve` の壁生成パターン)。
        let src = "\
For i = 1 to 3
Create 中立 壁 0 パイロット不在 0 4 i
Next
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let mut ys: Vec<u32> = app
            .database()
            .unit_instances
            .iter()
            .filter(|u| u.unit_data_name == "壁")
            .map(|u| u.y)
            .collect();
        ys.sort();
        assert_eq!(
            ys,
            vec![1, 2, 3],
            "座標のループ変数が解決されていない: {ys:?}"
        );
        // x は数値リテラルなので全て 4。
        assert!(app
            .database()
            .unit_instances
            .iter()
            .filter(|u| u.unit_data_name == "壁")
            .all(|u| u.x == 4));
    }

    #[test]
    fn npc_party_parses_to_npc_with_aliases() {
        // SRC 正準は "ＮＰＣ"。旧移植エイリアス "友軍" / "NPC" / "Allied" も
        // すべて Party::Npc に解決される。
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "Create ＮＰＣ ブレイバー 1 リオ 10 0 0\n\
                   Create 友軍 ブレイバー 1 リオ 10 1 1\n\
                   Create NPC ブレイバー 1 リオ 10 2 2\n";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        for i in 2..5 {
            assert_eq!(
                app.database().unit_instances[i].party,
                crate::Party::Npc,
                "index {i} の陣営が Npc でない"
            );
        }
    }

    #[test]
    fn removeunit_removes_instance() {
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "RemoveUnit ゾルダII\n";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.database().unit_instances.len(), 1);
        assert_eq!(
            app.database().unit_instances[0].unit_data_name,
            "ブレイバー"
        );
    }

    #[test]
    fn removepilot_removes_pilot_def() {
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "RemovePilot ガロ\n";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert!(app.database().pilots.iter().all(|p| p.name != "ガロ"));
    }

    #[test]
    fn recoverhp_full_resets_damage() {
        let mut app = App::new();
        setup_two_units(&mut app);
        // SRC 準拠: `RecoverHP unit rate` (rate は %) — 100% で全快
        let src = "\
Damage ブレイバー 1500
RecoverHP ブレイバー 100
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        // ブレイバーが先頭
        assert_eq!(app.database().unit_instances[0].damage, 0);
    }

    #[test]
    fn recoverhp_percent() {
        let mut app = App::new();
        setup_two_units(&mut app);
        // rate は % なので 50 = 50%
        let src = "\
Damage ブレイバー 2000
RecoverHP ブレイバー 50
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        // 3500 * 50% = 1750 回復、ダメージは 2000 - 1750 = 250
        assert_eq!(app.database().unit_instances[0].damage, 250);
    }

    #[test]
    fn recoveren_full() {
        let mut app = App::new();
        setup_two_units(&mut app);
        // EN を 100 消費させた後 RecoverEN で全回復
        app.database_mut().unit_instances[0].en_consumed = 100;
        let src = "RecoverEN ブレイバー 100\n";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.database().unit_instances[0].en_consumed, 0);
    }

    #[test]
    fn increase_morale_command() {
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "IncreaseMorale ブレイバー 20\n";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.database().unit_instances[0].morale, 120);
        // 過剰加算は 150 でクランプ
        let src2 = "IncreaseMorale ブレイバー 999\n";
        let stmts2 = event::parse(src2).unwrap();
        execute(&mut app, &stmts2).unwrap();
        assert_eq!(app.database().unit_instances[0].morale, 150);
    }

    #[test]
    fn expup_and_levelup_commands() {
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "\
ExpUp ブレイバー 50
LevelUp ブレイバー 2
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        // ExpUp 50 + LevelUp 2 → 50 + 200 = 250
        assert_eq!(app.database().unit_instances[0].total_exp, 250);
    }

    fn setup_units_with_item(app: &mut App) {
        setup_two_units(app);
        // Item を定義に追加 (Item 命令から参照される ItemData)
        app.database_mut().items.push(crate::data::item::ItemData {
            name: "ハイブリッドアーマー".to_string(),
            class: "armor".to_string(),
            part: "本体".to_string(),
            hp_mod: 500,
            en_mod: 50,
            armor_mod: 100,
            mobility_mod: 10,
            speed_mod: 1,
            comment: String::new(),
            features: Vec::new(),
        });
    }

    #[test]
    fn item_command_equips() {
        let mut app = App::new();
        setup_units_with_item(&mut app);
        let src = "Item ブレイバー ハイブリッドアーマー\n";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(
            app.database().unit_instances[0].equipped_item_names(),
            vec!["ハイブリッドアーマー"]
        );
    }

    #[test]
    fn item_command_idempotent() {
        let mut app = App::new();
        setup_units_with_item(&mut app);
        let src = "\
Item ブレイバー ハイブリッドアーマー
Item ブレイバー ハイブリッドアーマー
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(
            app.database().unit_instances[0].equipped_item_names().len(),
            1
        );
    }

    #[test]
    fn removeitem_removes_equipped() {
        let mut app = App::new();
        setup_units_with_item(&mut app);
        let src = "\
Item ブレイバー ハイブリッドアーマー
RemoveItem ブレイバー ハイブリッドアーマー
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert!(app.database().unit_instances[0]
            .equipped_item_names()
            .is_empty());
    }

    #[test]
    fn exchangeitem_swaps() {
        let mut app = App::new();
        setup_units_with_item(&mut app);
        app.database_mut().items.push(crate::data::item::ItemData {
            name: "メガランチャー".to_string(),
            class: "weapon".to_string(),
            part: "本体".to_string(),
            hp_mod: 0,
            en_mod: 100,
            armor_mod: 0,
            mobility_mod: 0,
            speed_mod: 0,
            comment: String::new(),
            features: Vec::new(),
        });
        let src = "\
Item ブレイバー ハイブリッドアーマー
ExchangeItem ブレイバー ハイブリッドアーマー メガランチャー
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(
            app.database().unit_instances[0].equipped_item_names(),
            vec!["メガランチャー"]
        );
    }

    #[test]
    fn equipped_item_increases_max_hp() {
        let mut app = App::new();
        setup_units_with_item(&mut app);
        let src = "\
Set base MaxHP(ブレイバー)
Item ブレイバー ハイブリッドアーマー
Set boosted MaxHP(ブレイバー)
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("base"), "3500");
        assert_eq!(app.script_var("boosted"), "4000"); // 3500 + 500
    }

    #[test]
    fn hasitem_predicate() {
        let mut app = App::new();
        setup_units_with_item(&mut app);
        let src = "\
Item ブレイバー ハイブリッドアーマー
If HasItem(ブレイバー, ハイブリッドアーマー) = 1 Then
  Message has
EndIf
If HasItem(ブレイバー, 存在しないアイテム) = 0 Then
  Message missing
EndIf
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["has".to_string(), "missing".to_string()]);
    }

    #[test]
    fn switch_case_range() {
        let src = "\
Set x 5
Switch $(x)
Case 1 To 3
  Message small
Case 4 To 6
  Message middle
Case Is > 10
  Message big
EndSw
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["middle".to_string()]);
    }

    #[test]
    fn switch_case_is_comparison() {
        let src = "\
Set x 100
Switch $(x)
Case Is < 10
  Message tiny
Case Is < 50
  Message small
Case Is >= 50
  Message big
EndSw
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["big".to_string()]);
    }

    #[test]
    fn money_command_absolute_and_relative() {
        let src = "\
Money 1000
Money +500
Money -200
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.money(), 1300);
    }

    #[test]
    fn money_function_in_expression() {
        let src = "\
Money 750
Set m Money()
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("m"), "750");
    }

    #[test]
    fn setstatus_adds_and_unset_removes() {
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "\
SetStatus ブレイバー 毒
SetStatus ブレイバー 麻痺
UnsetStatus ブレイバー 毒
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        let u = &app.database().unit_instances[0];
        assert!(u.has_condition("麻痺"));
        assert!(!u.has_condition("毒"));
    }

    #[test]
    fn hasstatus_function() {
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "\
SetStatus ブレイバー 気力高揚
Set h HasStatus(ブレイバー, 気力高揚)
Set m HasStatus(ブレイバー, 麻痺)
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("h"), "1");
        assert_eq!(app.script_var("m"), "0");
    }

    #[test]
    fn transform_changes_unit_data_name() {
        let mut app = App::new();
        setup_two_units(&mut app);
        // 既存 ゾルダII を Transform で別名にする
        let src = "Transform ゾルダII ガロゾルダII\n";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        // pilot_name でマッチさせたので、index 1 のユニットが変更されているはず
        let zolda = &app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.pilot_name == "ガロ")
            .unwrap();
        assert_eq!(zolda.unit_data_name, "ガロゾルダII");
    }

    #[test]
    fn gameclear_and_gameover_set_state() {
        let mut app = App::new();
        execute(&mut app, &event::parse("GameClear\n").unwrap()).unwrap();
        assert_eq!(app.stage_state(), crate::stage::StageState::Victory);
        let mut app2 = App::new();
        execute(&mut app2, &event::parse("GameOver\n").unwrap()).unwrap();
        assert_eq!(app2.stage_state(), crate::stage::StageState::Defeat);
    }

    #[test]
    fn finish_with_label_records_next_stage() {
        let mut app = App::new();
        execute(&mut app, &event::parse("Finish 次章\n").unwrap()).unwrap();
        assert_eq!(app.stage_state(), crate::stage::StageState::Victory);
        assert_eq!(app.script_var("__next_stage"), "次章");
    }

    #[test]
    fn clear_event_and_restore_label() {
        // `ClearEvent` がラベルを無効化し、`Restore` で再登録できることを確認。
        // (以前 `Forget` のテストだったが、`Forget` は titles リスト管理のため
        //  `ClearEvent` が正しいコマンド。SRC.Sharp `ClearEventCmd.cs` 準拠。)
        let src = "\
hello:
  Message x
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert!(app.script_library().label_pc("hello").is_some());
        execute(&mut app, &event::parse("ClearEvent hello\n").unwrap()).unwrap();
        assert!(app.script_library().label_pc("hello").is_none());
        execute(&mut app, &event::parse("Restore hello\n").unwrap()).unwrap();
        assert!(app.script_library().label_pc("hello").is_some());
    }

    #[test]
    fn forget_removes_from_titles_list() {
        // `Forget title` は titles リストからタイトルを削除する。
        // SRC.Sharp `ForgetCmd.cs` 準拠。
        let mut app = App::new();
        app.titles_mut().push("シン・サーガ".to_string());
        app.titles_mut().push("ブレイバー".to_string());
        execute(&mut app, &event::parse("Forget シン・サーガ\n").unwrap()).unwrap();
        assert!(!app.titles().contains(&"シン・サーガ".to_string()));
        assert!(app.titles().contains(&"ブレイバー".to_string()));
    }

    #[test]
    fn load_adds_titles_to_list() {
        // `Load title` は titles リストに作品名を追加する。重複は無視。
        let mut app = App::new();
        execute(
            &mut app,
            &event::parse("Load ブレイバー\nLoad ブレイバー\nLoad ザンナー3\n").unwrap(),
        )
        .unwrap();
        assert_eq!(
            app.titles(),
            &["ブレイバー".to_string(), "ザンナー3".to_string()]
        );
    }

    #[test]
    fn setskill_adds_skill_to_pilot_instance() {
        // SetSkill はパイロット名・スキル名・レベルの 3 引数。
        // PilotInstance.skills に登録され、Skill() 関数で参照できる。
        let mut app = App::new();
        setup_two_units(&mut app);
        execute(&mut app, &event::parse("SetSkill リオ 必中 1\n").unwrap()).unwrap();
        let pilot_inst = app
            .database()
            .pilot_instances
            .iter()
            .find(|p| p.pilot_data_name == "リオ")
            .expect("PilotInstance created by SetSkill");
        assert!(pilot_inst.skills.iter().any(|s| s.starts_with("必中")));
    }

    #[test]
    fn join_restores_from_leave() {
        // SRC `Joinコマンド.md`: `Join [unit]` は `Leave` で離脱させたユニットを
        // 部隊に復帰させる。`off_map = false` / `life_state = ""` に戻す。
        let mut app = App::new();
        setup_two_units(&mut app);
        // まず Leave で離脱させる
        execute(&mut app, &event::parse("Leave ブレイバー\n").unwrap()).unwrap();
        assert_eq!(app.database().unit_instances[0].life_state, "離脱");
        // Join で復帰させる
        execute(&mut app, &event::parse("Join ブレイバー\n").unwrap()).unwrap();
        assert_eq!(app.database().unit_instances[0].life_state, "");
        assert!(!app.database().unit_instances[0].off_map);
    }

    #[test]
    fn ride_two_arg_mounts_pilot_onto_named_unit() {
        let mut app = App::new();
        setup_two_units(&mut app);
        // `Ride pilot unit` — ガロ を ブレイバー に搭乗させる。
        execute(&mut app, &event::parse("Ride ガロ ブレイバー\n").unwrap()).unwrap();
        let g = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "ブレイバー")
            .unwrap();
        assert_eq!(g.pilot_name, "ガロ");
    }

    #[test]
    fn unit_short_form_sets_current_unit_for_ride() {
        let mut app = App::new();
        setup_two_units(&mut app);
        // `Unit <name>` で カレントユニット を生成 → `Ride <pilot>` で搭乗。
        execute(
            &mut app,
            &event::parse("Unit \"ゾルダII\" 0\nRide リオ\n").unwrap(),
        )
        .unwrap();
        let uid = app.selected_unit_for_event().to_string();
        assert!(!uid.is_empty());
        let cur = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.uid == uid)
            .unwrap();
        assert_eq!(cur.unit_data_name, "ゾルダII");
        assert_eq!(cur.pilot_name, "リオ");
        assert!(cur.off_map, "カレントユニットは未配置 (off_map)");
    }

    #[test]
    fn setbullet_updates_weapon_ammo() {
        let mut app = App::new();
        setup_two_units(&mut app);
        // 簡易の weapons は無いので付与してから
        let unit_idx = app
            .database_mut()
            .units
            .iter_mut()
            .position(|u| u.name == "ブレイバー")
            .unwrap();
        app.database_mut().units[unit_idx]
            .weapons
            .push(crate::data::unit::WeaponData {
                name: "ビームライフル".to_string(),
                power: 2500,
                min_range: 2,
                max_range: 5,
                precision: 15,
                bullet: 99,
                en_consumption: 0,
                necessary_morale: 0,
                adaption: String::new(),
                critical: 0,
                class: String::new(),
                extras: Vec::new(),
            });
        execute(
            &mut app,
            &event::parse("SetBullet ブレイバー ビームライフル 3\n").unwrap(),
        )
        .unwrap();
        let unit = app.database().unit_by_name("ブレイバー").unwrap();
        let w = unit
            .weapons
            .iter()
            .find(|w| w.name == "ビームライフル")
            .unwrap();
        assert_eq!(w.bullet, 3);
    }

    #[test]
    fn disable_enable_set_command_flags() {
        let mut app = App::new();
        execute(&mut app, &event::parse("Disable Save\n").unwrap()).unwrap();
        // SRC.Sharp 準拠: `Disable(name)` = "1"
        assert_eq!(app.script_var("Disable(Save)"), "1");
        execute(&mut app, &event::parse("Enable Save\n").unwrap()).unwrap();
        // Enable は変数を削除する
        assert_eq!(app.script_var("Disable(Save)"), "");
    }

    #[test]
    fn string_concat_with_ampersand() {
        let src = "\
Set name タロウ
Set greeting hello & $(name) & san
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("greeting"), "helloタロウsan");
    }

    #[test]
    fn rgb_function() {
        let src = "Set c RGB(50, 100, 200)\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("c"), "#3264c8"); // SRC.Sharp: "#rrggbb" 形式
    }

    #[test]
    fn rgb_function_arithmetic_args() {
        // SRC.Sharp 準拠: RGB の引数に算術式を使える。
        // RGB(200 + 55, 0, 0) → R=255, G=0, B=0 → "#ff0000"
        let src = "Set c RGB(200 + 55, 0, 0)\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("c"), "#ff0000");
    }

    #[test]
    fn rgb_function_variable_args() {
        // SRC.Sharp 準拠: RGB の引数に変数を使える。
        let src = "Set r 255\nSet g 0\nSet b 128\nSet c RGB(r, g, b)\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("c"), "#ff0080");
    }

    #[test]
    fn iif_function_picks_branch() {
        let src = "\
Set a 1
Set b IIF($(a), yes, no)
Set c IIF(0, yes, no)
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("b"), "yes");
        assert_eq!(app.script_var("c"), "no");
    }

    #[test]
    fn dir_reflects_virtual_filesystem() {
        // 仮想ファイルが無ければ空文字、書き出した後は basename を返す。
        let app = run_script(
            "Set d1 Dir(\"data\\foo.txt\", ファイル)\n\
             Open \"data\\foo.txt\" For 出力 As F\nPrint F x\nClose F\n\
             Set d2 Dir(\"data\\foo.txt\", ファイル)\n",
        );
        assert_eq!(app.script_var("d1"), "", "未作成ファイルは空文字");
        assert_eq!(app.script_var("d2"), "foo.txt", "作成後は basename");
    }

    #[test]
    fn left_right_mid_substring_functions() {
        let src = "\
Set s こんにちは世界
Set a Left(s, 3)
Set b Right(s, 2)
Set c Mid(s, 4, 3)
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("a"), "こんに");
        assert_eq!(app.script_var("b"), "世界");
        assert_eq!(app.script_var("c"), "ちは世");
    }

    #[test]
    fn left_mid_accept_arithmetic_count_args() {
        // `Left`/`Mid`/`Right` の文字数引数は `(te - 1)` のような変数入り
        // 算術式でも評価される (生 parse だと None を返してリテラル漏れした)。
        let app = run_script(
            "Set s こんにちは世界\n\
             Set te 4\n\
             Set a Left(s, (te - 1))\n\
             Set b Mid(s, (te - 1))\n\
             Set c Right(s, (te - 2))\n",
        );
        assert_eq!(app.script_var("a"), "こんに");
        assert_eq!(app.script_var("b"), "にちは世界");
        assert_eq!(app.script_var("c"), "世界");
    }

    #[test]
    fn font_resolves_variable_argument() {
        // `Font LetterShadow` のように変数でフォント指定を切り替える記法。
        // 変数が色文字列なら SetFont の color に反映される。
        let app = run_script(
            "Set LetterShadow #000000\n\
             Font LetterShadow\n\
             PaintString 10 10 影\n",
        );
        let setfont = app.script_overlay().cmds.iter().find_map(|c| match c {
            crate::DrawCmd::SetFont { color, .. } => Some(color.clone()),
            _ => None,
        });
        assert_eq!(setfont.as_deref(), Some("#000000"));
    }

    #[test]
    fn require_applies_ini_key_value_assignments() {
        // `Require <path>` は script_library に登録済みの設定ファイルの
        // `key = value` 行をスクリプト変数として適用する。
        let ini = "\
# テーマ設定\n\
LetterColor1 = #ffffff\n\
LetterShadow = #000000\n\
FrameColor1 = #505098\n";
        let ini_stmts = event::parse(ini).unwrap();
        let mut app = App::new();
        app.script_library_mut()
            .append_with_name(&ini_stmts, "Lib\\Alpha2ndStatus.ini");
        let stmts = event::parse("Require Lib\\Alpha2ndStatus.ini\n").unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("LetterColor1"), "#ffffff");
        assert_eq!(app.script_var("LetterShadow"), "#000000");
        assert_eq!(app.script_var("FrameColor1"), "#505098");
    }

    #[test]
    fn strcmp_returns_ordering() {
        let src = "\
Set a StrCmp(\"abc\", \"abc\")
Set b StrCmp(\"abc\", \"abd\")
Set c StrCmp(\"xyz\", \"abc\")
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("a"), "0");
        assert_eq!(app.script_var("b"), "-1");
        assert_eq!(app.script_var("c"), "1");
    }

    #[test]
    fn paintpicture_pushes_draw_cmd() {
        let src = "PaintPicture path/foo.bmp 10 20 50 60 透過 左右反転\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let cmds = &app.script_overlay().cmds;
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            crate::DrawCmd::Picture {
                path,
                x,
                y,
                w,
                h,
                transparent,
                flip_x,
                ..
            } => {
                assert_eq!(path, "path/foo.bmp");
                assert_eq!(*x, 10.0);
                assert_eq!(*y, 20.0);
                assert_eq!(*w, Some(50.0));
                assert_eq!(*h, Some(60.0));
                assert!(*transparent);
                assert!(*flip_x);
            }
            _ => panic!("expected Picture"),
        }
    }

    #[test]
    fn paintpicture_dash_position_sets_center_flags() {
        // `PaintPicture img - -`（幅省略・中央寄せ）は center フラグを立て、
        // 実寸を知るフロントエンドに中央寄せを委譲する。明示座標では立たない。
        // スパロボ戦記 機体能力画面の `\event\機体能力.png - - 透過` が
        // (240,240) に左上ずれして表示されていた不具合の回帰防止。
        let mut app = App::new();
        execute(
            &mut app,
            &event::parse("PaintPicture frame.png - - 透過\n").unwrap(),
        )
        .unwrap();
        match &app.script_overlay().cmds[0] {
            crate::DrawCmd::Picture {
                center_x,
                center_y,
                transparent,
                w,
                h,
                ..
            } => {
                assert!(*center_x, "x=- は center_x");
                assert!(*center_y, "y=- は center_y");
                // 幅省略形で w/h の位置に来た `透過` を幅と取り違えず、透過フラグ
                // として認識する (機体能力.png が不透明で覆い隠す不具合の回帰防止)。
                assert!(*transparent, "幅省略形でも 透過 を認識する");
                assert_eq!(*w, None, "透過 を w として食わない");
                assert_eq!(*h, None);
            }
            _ => panic!("expected Picture"),
        }

        let mut app2 = App::new();
        execute(
            &mut app2,
            &event::parse("PaintPicture frame.png 10 20\n").unwrap(),
        )
        .unwrap();
        match &app2.script_overlay().cmds[0] {
            crate::DrawCmd::Picture {
                center_x,
                center_y,
                x,
                y,
                ..
            } => {
                assert!(!*center_x);
                assert!(!*center_y);
                assert_eq!(*x, 10.0);
                assert_eq!(*y, 20.0);
            }
            _ => panic!("expected Picture"),
        }
    }

    #[test]
    fn arith_operator_char_set_matches_tokenizer() {
        // 抜本対策の要: `tokenize_expr` が算術演算子トークンを生む記号は
        // すべて `is_arith_operator_char` に含まれること。新演算子を
        // tokenize_expr に足して SoT 更新を忘れると本テストが落ちる
        // (VB6 整数除算 `\` / 累乗 `^` 取りこぼし型バグの再発防止)。
        use ExprTok::*;
        for c in 0u8..=127u8 {
            let ch = c as char;
            let toks = tokenize_expr(&ch.to_string());
            let produces_arith_op = toks.len() == 1
                && matches!(
                    toks[0],
                    Plus | Minus | Star | Slash | IntDiv | Caret | LParen | RParen
                );
            if produces_arith_op {
                assert!(
                    is_arith_operator_char(ch),
                    "tokenize_expr は {ch:?} を算術演算子として扱うが is_arith_operator_char が false"
                );
            }
        }
        // 代表演算子が含まれ、比較/連結/英数字は含まれないこと。
        for c in ['+', '-', '*', '/', '\\', '^', '(', ')'] {
            assert!(is_arith_operator_char(c), "{c:?} は算術演算子のはず");
        }
        for c in ['<', '>', '=', '&', 'a', '1', ' ', '.'] {
            assert!(!is_arith_operator_char(c), "{c:?} は算術演算子ではない");
        }
    }

    #[test]
    fn color_command_pushes_setcolor() {
        let src = "\
Color RGB(50, 100, 200)
Color 赤
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let cmds = &app.script_overlay().cmds;
        assert!(matches!(&cmds[0], crate::DrawCmd::SetColor { color } if color == "#3264c8"));
        assert!(matches!(&cmds[1], crate::DrawCmd::SetColor { color } if color == "#ff0000"));
    }

    #[test]
    fn drawwidth_command() {
        let src = "DrawWidth 3\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let cmds = &app.script_overlay().cmds;
        assert!(matches!(&cmds[0], crate::DrawCmd::SetLineWidth(n) if *n == 3.0));
    }

    #[test]
    fn indexed_var_set_and_read() {
        let src = "\
Set items[1] alpha
Set items[2] beta
Set items[(1 + 2)] gamma
Set i 2
Set v items[i]
Set w items[3]
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("items[1]"), "alpha");
        assert_eq!(app.script_var("items[2]"), "beta");
        assert_eq!(app.script_var("items[3]"), "gamma");
        // 読み出し: items[i] → items[2] → "beta"
        assert_eq!(app.script_var("v"), "beta");
        assert_eq!(app.script_var("w"), "gamma");
    }

    /// 未定義の indexed 変数を比較条件に使うと、リテラル `配列[1]` が漏れて
    /// `<> ""` が常に真になる回帰。未定義 indexed 参照は空文字に解決する。
    #[test]
    fn undefined_indexed_var_compares_as_empty() {
        let src = "\
If 格納属性表示[1] <> \"\" Then
  Set 結果 NONEMPTY
Else
  Set 結果 EMPTY
EndIf
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("結果"), "EMPTY");
    }

    /// 値文脈 (Set 値 / Switch) でも未定義 indexed 参照はリテラルを漏らさず
    /// 空文字に解決する。`fn_arg_value` を通る経路の網羅。
    #[test]
    fn undefined_indexed_var_resolves_empty_in_value_contexts() {
        // Set 値: 未定義 indexed → 空文字 (リテラル `配列[9]` を入れない)
        let src1 = "Set x 配列[9]\n";
        let mut app = App::new();
        execute(&mut app, &event::parse(src1).unwrap()).unwrap();
        assert_eq!(app.script_var("x"), "");

        // Switch: 未定義 indexed の値は `Case \"\"` にマッチする
        let src2 = "\
Switch 配列[9]
Case \"\"
  Set hit EMPTY
Case Else
  Set hit OTHER
EndSw
";
        let mut app = App::new();
        execute(&mut app, &event::parse(src2).unwrap()).unwrap();
        assert_eq!(app.script_var("hit"), "EMPTY");

        // 定義済みなら従来どおり値が伝播する
        let src3 = "\
Set 配列[2] beta
Set y 配列[2]
";
        let mut app = App::new();
        execute(&mut app, &event::parse(src3).unwrap()).unwrap();
        assert_eq!(app.script_var("y"), "beta");
    }

    // ───────────────────────────────────────────────────────────────────
    // 抜本的回帰ガード — 変数参照解決のクラス的バグを防ぐ網羅テスト群。
    //
    // バグのクラス: 「変数参照が、解決できないとリテラルのソーステキストを
    // 値文脈へ漏らす / 逆に名前が必要な箇所で値へ過剰展開される」。
    // 値解決の集約点は `fn_arg_value`、LHS 解決の集約点は `resolve_lhs_name`。
    // この 2 つの不変条件を全文脈で固定する。
    // ───────────────────────────────────────────────────────────────────

    /// `fn_arg_value` の契約を直接固定する。
    /// - 未定義 indexed 参照 → 空文字 (リテラル `name[k]` を漏らさない)
    /// - 定義済み indexed → 値
    /// - 未定義の **裸スカラー** → リテラル温存 (SRC 規約: `If 選択 = ランダム`
    ///   の `ランダム` のように未定義識別子はリテラルとして比較される)
    #[test]
    fn fn_arg_value_contract_for_variable_references() {
        let mut app = App::new();
        app.set_script_var("定義[1]".to_string(), "val".to_string());
        app.set_script_var("空[1]".to_string(), String::new());
        app.set_script_var("scal".to_string(), "5".to_string());

        // 未定義 indexed → 空
        assert_eq!(fn_arg_value(&app, "未定義[1]"), "");
        assert_eq!(fn_arg_value(&app, "未定義[xyz]"), "");
        // 定義済み indexed → 値 / 空代入 → 空
        assert_eq!(fn_arg_value(&app, "定義[1]"), "val");
        assert_eq!(fn_arg_value(&app, "空[1]"), "");
        // 定義済みスカラー → 値
        assert_eq!(fn_arg_value(&app, "scal"), "5");
        // 未定義スカラー → リテラル温存 (SRC 規約)
        assert_eq!(fn_arg_value(&app, "ランダム"), "ランダム");
        // クオート文字列 / 数値リテラルはそのまま
        assert_eq!(fn_arg_value(&app, "\"abc\""), "abc");
        assert_eq!(fn_arg_value(&app, "42"), "42");
    }

    /// 未定義 indexed 参照は **全ての値文脈** で空に解決する (リテラル不漏れ)。
    /// 比較 (両極性) / Set 値 / Switch を 1 本で網羅し、新たな値文脈の追加で
    /// 抜けが出ても捕まえられるようにする。
    #[test]
    fn undefined_indexed_reference_is_empty_in_every_value_context() {
        let src = "\
If 配列[9] <> \"\" Then
  Set cmp_ne T
Else
  Set cmp_ne F
EndIf
If 配列[9] = \"\" Then
  Set cmp_eq T
Else
  Set cmp_eq F
EndIf
Set assigned 配列[9]
Switch 配列[9]
Case \"\"
  Set sw EMPTY
Case Else
  Set sw OTHER
EndSw
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("cmp_ne"), "F", "<> \"\" は偽であるべき");
        assert_eq!(app.script_var("cmp_eq"), "T", "= \"\" は真であるべき");
        assert_eq!(app.script_var("assigned"), "", "Set 値は空であるべき");
        assert_eq!(app.script_var("sw"), "EMPTY", "Switch は Case \"\" に一致");
    }

    /// LHS を取る命令 (`Set` / `Incr` / `Unset`) は indexed 参照を同一の
    /// 実キーへ解決する。`resolve_lhs_name` を共有しているかの一貫性ガード。
    #[test]
    fn indexed_lhs_is_consistent_across_set_incr_unset() {
        let src = "\
Set i 1
Set 表[i] 10
Incr 表[i]
Incr 表[1] 5
Incr 新規[2]
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // Set→Incr→Incr が同じ実キー 表[1] を更新: 10 +1 +5 = 16
        assert_eq!(app.script_var("表[1]"), "16");
        // 未定義 indexed への Incr は 0 起点で定義される
        assert_eq!(app.script_var("新規[2]"), "1");

        // Unset も同じキーを解決して削除する
        let src2 = "\
Set j 3
Set 表[j] hello
Unset 表[3]
Set after IsVarDefined(表[j])
";
        let mut app = App::new();
        execute(&mut app, &event::parse(src2).unwrap()).unwrap();
        assert_eq!(app.script_var("after"), "0", "Unset 表[3] が 表[j] を消す");
    }

    /// `IsVarDefined` を .eve 実行パス経由で indexed 変数に対し検証する。
    /// 引数が値へ過剰展開されると定義済みでも 0 になる回帰のガード。
    #[test]
    fn isvardefined_indexed_through_execution_path() {
        let src = "\
Set arr[1] x
Set arr[2] \"\"
Set k 1
Set d_value IsVarDefined(arr[1])
Set d_empty IsVarDefined(arr[2])
Set d_undef IsVarDefined(arr[3])
Set d_varidx IsVarDefined(arr[k])
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("d_value"), "1", "値ありは定義済み");
        assert_eq!(app.script_var("d_empty"), "1", "空代入も定義済み");
        assert_eq!(app.script_var("d_undef"), "0", "未代入は未定義");
        assert_eq!(app.script_var("d_varidx"), "1", "変数添字 arr[k]→arr[1]");
    }

    #[test]
    fn foreach_iterates_indexed_var_keys() {
        let src = "\
Set xs[1] a
Set xs[2] b
Set xs[3] c
Set joined empty
ForEach i In xs
  Set joined $(joined)_$(xs[i])
Next
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("joined"), "empty_a_b_c");
    }

    #[test]
    fn hotpoint_registers_with_eval_coords() {
        let src = "\
Set base 200
Hotpoint UnitA (50 + 10) ($(base) - 25) 50 50
Hotpoint UnitB 100 100 50 50 非表示
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let hp = app.hotpoints();
        assert_eq!(hp.len(), 2);
        assert_eq!(hp[0].name, "UnitA");
        assert_eq!(hp[0].x, 60);
        assert_eq!(hp[0].y, 175);
        assert_eq!(hp[0].w, 50);
        assert!(!hp[0].invisible);
        assert_eq!(hp[1].name, "UnitB");
        assert!(hp[1].invisible);
    }

    #[test]
    fn hotpoint_coords_resolve_loop_variable() {
        // スパロボ戦記 機体選択開始 と同型: ループ変数 `j` を含む算術式
        // `(j * 40 - 35)` を Hotpoint 座標に使う。`j` は裸の識別子なので
        // expand_vars では展開されず、app-aware な eval が必要。
        // 引用符付き数値 `"2"` も座標式に混じる (`("$(i)" * 70 + 15)` 由来)。
        let src = "\
Set j 3
Set i 2
Hotpoint Cell (j * 40 - 35) (\"$(i)\" * 70 + 15) 32 32
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let hp = app.hotpoints();
        assert_eq!(hp.len(), 1);
        // x = 3*40-35 = 85
        assert_eq!(hp[0].x, 85, "ループ変数 j が解決されていない");
        // y = 2*70+15 = 155 (引用符付き "2" を数値として扱う)
        assert_eq!(hp[0].y, 155, "引用符付き数値が解決されていない");
    }

    #[test]
    fn refresh_keeps_hotpoints() {
        // Refresh は画面再描画のみで Hotpoint は維持する (SRC 仕様)
        let src = "\
Hotpoint A 0 0 50 50
Hotpoint B 100 0 50 50
Refresh
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.hotpoints().len(), 2);
    }

    #[test]
    fn clearobj_clears_hotpoints() {
        let src = "\
Hotpoint A 0 0 50 50
ClearObj
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert!(app.hotpoints().is_empty());
    }

    #[test]
    fn wait_click_with_hotpoints_presents_menu_storing_name() {
        let src = "\
Hotpoint A 0 0 50 50
Hotpoint B 100 0 50 50
Hotpoint C 200 0 50 50
Wait Click
Stage $(選択)
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        match app.pending_dialog().unwrap() {
            crate::PendingDialog::Menu {
                options,
                var_name,
                store_value,
                ..
            } => {
                assert_eq!(
                    options,
                    &vec!["A".to_string(), "B".to_string(), "C".to_string()]
                );
                assert_eq!(var_name, "選択");
                assert!(*store_value);
            }
            _ => panic!("expected Menu"),
        }
        // 2 番目 (B) を選ぶ
        assert!(app.respond_dialog(2));
        assert_eq!(app.script_var("選択"), "B");
        assert_eq!(app.stage(), "B");
    }

    #[test]
    fn hotpoint_click_at_resolves_menu_directly() {
        // Hotpoint クリックで Menu を直接確定できる（番号入力不要）。
        let src = "\
Hotpoint A 0 0 50 50
Hotpoint B 100 0 50 50
Hotpoint C 200 0 50 50
Wait Click
Stage $(選択)
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert!(app.pending_dialog().is_some());
        // B の中央 (125, 25) をクリック → "選択" = "B"
        let consumed = app.handle_input(crate::Input::ClickAt { x: 125, y: 25 });
        assert!(consumed);
        assert_eq!(app.script_var("選択"), "B");
        assert_eq!(app.stage(), "B");
    }

    #[test]
    fn hotpoint_click_outside_does_not_consume() {
        // Hotpoint の外側をクリックしても Menu は閉じない（無反応）。
        let src = "\
Hotpoint A 0 0 50 50
Wait Click
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert!(app.pending_dialog().is_some());
        let consumed = app.handle_input(crate::Input::ClickAt { x: 200, y: 200 });
        assert!(!consumed);
        assert!(app.pending_dialog().is_some());
    }

    #[test]
    fn info_returns_unit_data_fields() {
        // Info(ユニットデータ, ...) でユニットの基礎データを引ける。
        let mut app = App::new();
        let src = "\
Unit ブレイバー リアル系 1 0 陸 5 M 3000 400 3500 120 1200 110 AAAA
Weapon ブレイバー ビームライフル 2500 1 5 +15 -1
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(
            expand_vars(&app, "Info(ユニットデータ, ブレイバー, 名称)"),
            "ブレイバー"
        );
        assert_eq!(
            expand_vars(&app, "Info(ユニットデータ, ブレイバー, 最大ＨＰ)"),
            "3500"
        );
        assert_eq!(
            expand_vars(&app, "Info(ユニットデータ, ブレイバー, 武器数)"),
            "1"
        );
        assert_eq!(
            expand_vars(&app, "Info(ユニットデータ, ブレイバー, 武器, 1)"),
            "ビームライフル"
        );
        assert_eq!(
            expand_vars(
                &app,
                "Info(ユニットデータ, ブレイバー, 武器, ビームライフル, 攻撃力)"
            ),
            "2500"
        );
        assert_eq!(
            expand_vars(&app, "Info(ユニットデータ, ブレイバー, 武器, 1, 最大射程)"),
            "5"
        );
        // 最大攻撃力 / 最長射程 集約
        assert_eq!(
            expand_vars(&app, "Info(ユニットデータ, ブレイバー, 最大攻撃力)"),
            "2500"
        );
        assert_eq!(
            expand_vars(&app, "Info(ユニットデータ, ブレイバー, 最長射程)"),
            "5"
        );
    }

    #[test]
    fn feature_necessary_skill_gates_is_active() {
        // 必要技能.md §3: 特殊能力 `分身=値 (撃墜数Lv100)` は、撃墜数Lv100 を持たない
        // パイロットでは無効 (is_active=false)、持つエースでは有効。無条件特殊能力 (装甲) は
        // 常に有効。populate_active_features 経由で確認。
        use crate::data::pilot::{PilotData, Sex};
        let make_unit = || crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "分身機".into(),
            kana_name: "ぶんしんき".into(),
            nickname: "分身機".into(),
            class: "ロボ".into(),
            pilot_num: 1,
            item_num: 0,
            transportation: "陸".into(),
            speed: 5,
            size: Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 100,
            armor: 500,
            mobility: 100,
            adaption: Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: Vec::new(),
            features: vec![
                ("装甲".into(), "1000".into()),             // 無条件 → 常に有効
                ("分身".into(), "値 (撃墜数Lv100)".into()), // §3 必要技能ゲート付き
            ],
        };
        let make_pilot = |name: &str, feats: Vec<(&str, &str)>| PilotData {
            spirit_commands: Vec::new(),
            name: name.into(),
            nickname: name.into(),
            kana_name: name.into(),
            sex: Sex::Male,
            class: String::new(),
            adaption: Adaption::parse("AAAA").unwrap(),
            exp_value: 0,
            infight: 100,
            shooting: 100,
            hit: 100,
            dodge: 100,
            intuition: 100,
            technique: 100,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: None,
            features: feats
                .into_iter()
                .map(|(n, v)| (n.to_string(), v.to_string()))
                .collect(),
        };
        // ザコ (撃墜数なし) → 分身 無効、装甲 有効。
        let mut app = App::new();
        app.database_mut().units.push(make_unit());
        app.database_mut().pilots.push(make_pilot("ザコ", vec![]));
        let mut zako =
            crate::unit_instance::UnitInstance::new("分身機", "ザコ", crate::Party::Player, 0, 0);
        populate_active_features(&mut zako, &app);
        assert!(
            !crate::feature::has_feature(&zako.active_features, "分身"),
            "撃墜数なしで §3 ゲートの分身が有効になっている"
        );
        assert!(
            crate::feature::has_feature(&zako.active_features, "装甲"),
            "無条件の装甲が無効化された"
        );
        // エース (撃墜数Lv100) → 分身 有効。
        let mut app2 = App::new();
        app2.database_mut().units.push(make_unit());
        app2.database_mut()
            .pilots
            .push(make_pilot("エース", vec![("撃墜数Lv100", "1")]));
        let mut ace =
            crate::unit_instance::UnitInstance::new("分身機", "エース", crate::Party::Player, 0, 0);
        populate_active_features(&mut ace, &app2);
        assert!(
            crate::feature::has_feature(&ace.active_features, "分身"),
            "撃墜数Lv100 を持つエースで分身が無効のまま"
        );
    }

    #[test]
    fn info_returns_features_from_parsed_unit_txt() {
        // unit.txt 由来の features を Info で参照できる。
        // 直接 UnitData を database に積んで、parser を介さずに動作確認。
        let mut app = App::new();
        app.database_mut().units.push(crate::data::unit::UnitData {
            abilities: Vec::new(),
            name: "マグナＺ".into(),
            kana_name: "まぐなぜっと".into(),
            nickname: "マグナＺ".into(),
            class: "汎用".into(),
            pilot_num: 1,
            item_num: 3,
            transportation: "陸".into(),
            speed: 5,
            size: Size::M,
            value: 6000,
            exp_value: 150,
            hp: 4700,
            en: 120,
            armor: 1500,
            mobility: 55,
            adaption: Adaption::parse("AAAA").unwrap(),
            bitmap: "MGZ_MagnaZ.bmp".into(),
            weapons: Vec::new(),
            features: vec![
                (
                    "全身画像".into(),
                    "非表示 Anime\\Unit\\EC_MGZ.bmp 96 96".into(),
                ),
                ("ブースト".into(), "マジンパワー".into()),
            ],
        });
        // 特殊能力データ
        assert_eq!(
            expand_vars(
                &app,
                "Info(ユニットデータ, マグナＺ, 特殊能力データ, 全身画像)"
            ),
            "非表示 Anime\\Unit\\EC_MGZ.bmp 96 96"
        );
        // 特殊能力所有 (0/1)
        assert_eq!(
            expand_vars(
                &app,
                "Info(ユニットデータ, マグナＺ, 特殊能力所有, 全身画像)"
            ),
            "1"
        );
        assert_eq!(
            expand_vars(
                &app,
                "Info(ユニットデータ, マグナＺ, 特殊能力所有, 存在しない能力)"
            ),
            "0"
        );
        // 特殊能力数
        assert_eq!(
            expand_vars(&app, "Info(ユニットデータ, マグナＺ, 特殊能力数)"),
            "2"
        );
        // 特殊能力 N (N 番目の名前)
        assert_eq!(
            expand_vars(&app, "Info(ユニットデータ, マグナＺ, 特殊能力, 1)"),
            "全身画像"
        );
        assert_eq!(
            expand_vars(&app, "Info(ユニットデータ, マグナＺ, 特殊能力, 2)"),
            "ブースト"
        );
    }

    #[test]
    fn info_pilot_returns_base_stats() {
        let mut app = App::new();
        let src = "Pilot リオ リオ 男性 超能力者 SSSS 300 160 220 200 220 240 200\n";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(
            expand_vars(&app, "Info(パイロットデータ, リオ, 名称)"),
            "リオ"
        );
        assert_eq!(
            expand_vars(&app, "Info(パイロットデータ, リオ, 性別)"),
            "男性"
        );
        assert_eq!(
            expand_vars(&app, "Info(パイロットデータ, リオ, 格闘)"),
            "160"
        );
        assert_eq!(
            expand_vars(&app, "Info(パイロットデータ, リオ, 技量)"),
            "200"
        );
        assert_eq!(
            expand_vars(&app, "Info(パイロットデータ, リオ, 地形適応)"),
            "SSSS"
        );
    }

    /// `Info(パイロット, name, 格闘)` がレベルアップ後のスタットを返すことを確認。
    /// Pass 45: `info_pilot` が `effective_pilot_data` を使うよう修正。
    #[test]
    fn info_pilot_reflects_leveled_up_stats() {
        let src = "\
Pilot \"リオ\" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Place ブレイバー リオ Player 1 1
Exit
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();

        // 初期状態: 格闘 = 100
        let initial = expand_vars(&app, "Info(パイロット, リオ, 格闘)")
            .parse::<i32>()
            .unwrap_or(0);
        assert_eq!(initial, 100, "初期格闘は 100 のはず");

        // ExpUp でレベルを上げる (300 exp → level 4, リアル系 rate=12)
        let exp_src = "ExpUp ブレイバー 300\n";
        let exp_stmts = event::parse(exp_src).unwrap();
        execute(&mut app, &exp_stmts).unwrap();

        let leveled = expand_vars(&app, "Info(パイロット, リオ, 格闘)")
            .parse::<i32>()
            .unwrap_or(0);
        assert!(
            leveled > initial,
            "レベルアップ後の格闘 ({leveled}) は初期値 ({initial}) より高いはず"
        );
    }

    #[test]
    fn info_auto_detects_when_kind_omitted() {
        // データ区分省略時は自動判定。
        let mut app = App::new();
        let src = "\
Unit ブレイバー リアル系 1 0 陸 5 M 3000 400 3500 120 1200 110 AAAA
Pilot リオ リオ 男性 超能力者 SSSS 300 160 220 200 220 240 200
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        // ブレイバー → UnitData として引かれる
        assert_eq!(expand_vars(&app, "Info(ブレイバー, 最大ＨＰ)"), "3500");
        // リオ → PilotData として引かれる
        assert_eq!(expand_vars(&app, "Info(リオ, 格闘)"), "160");
    }

    #[test]
    fn info_map_returns_dimensions() {
        let mut app = App::new();
        let src = "MapSize 10 12\n";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(expand_vars(&app, "Info(マップ, 幅)"), "10");
        assert_eq!(expand_vars(&app, "Info(マップ, 高さ)"), "12");
        assert_eq!(expand_vars(&app, "Info(マップ, 時間帯)"), "昼");
    }

    #[test]
    fn single_line_if_with_paren_cond_executes_body() {
        // Score.eve 由来の形式: `If (cond) Exit` 一行で body を持つ If。
        // 条件が真なら body が実行され、後続行は飛ばされる。
        let src = "\
Set x 1
If ($(x) = 1) Goto skipme
Set never_set 1
skipme:
Stage done
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("never_set"), "");
        assert_eq!(app.stage(), "done");
    }

    #[test]
    fn single_line_if_with_then_keyword_executes_body() {
        // `If a = b Then stmt` 単一行形式（Then 後に body）。
        let src = "\
Set x 2
If $(x) = 2 Then Stage matched
Stage default
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // 単一行 If により Stage matched が即座に実行され、その後 Stage default
        // で上書きされる（多行 If と異なり EndIf を持たないので、結果は最後の Stage）。
        assert_eq!(app.stage(), "default");
    }

    #[test]
    fn single_line_if_skips_body_when_false() {
        let src = "\
Set x 0
If ($(x) = 1) Stage matched
Stage fallback
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.stage(), "fallback");
    }

    #[test]
    fn single_line_if_with_function_call_condition() {
        // `If Func(...) stmt` — 条件が 1 個の関数呼び出しトークン、Then 無し。
        // スパロボ戦記 Include.eve L3233 `If Instr(...) Exit` が該当。
        // ブロック If と誤認して EndIf を探すと「EndIf が見つかりません」で
        // 異常終了するため、単一行 If として正しく分割すること。
        let src = "\
Set a no
Set b no
If Instr(\"abc\",\"b\") Set a yes
If Instr(\"abc\",\"z\") Set b yes
Stage done
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("a"), "yes", "Instr=2 (真) → body 実行");
        assert_eq!(app.script_var("b"), "no", "Instr=0 (偽) → body skip");
        assert_eq!(app.stage(), "done");
    }

    #[test]
    fn single_line_if_with_bare_token_condition() {
        // `If <expr> stmt` — 条件が 1 トークン (関数でも括弧でもない)、Then 無し。
        // スパロボ戦記 Include.eve L1360 `If エピローグカット Exit` が該当。
        let src = "\
Set flag 1
Set off 0
If $(flag) Stage A
If $(off) Stage B
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // $(flag)=1 → 真 → Stage A。$(off)=0 → 偽 → Stage B skip。
        assert_eq!(app.stage(), "A");
    }

    #[test]
    fn int_floor_negative_per_src_spec() {
        let app = App::new();
        assert_eq!(expand_vars(&app, "Int(1.8)"), "1");
        assert_eq!(expand_vars(&app, "Int(-8.3)"), "-9");
        assert_eq!(expand_vars(&app, "Int(5)"), "5");
        assert_eq!(expand_vars(&app, "Int(0)"), "0");
    }

    #[test]
    fn eval_evaluates_arithmetic_and_variables() {
        let mut app = App::new();
        app.set_script_var("x".to_string(), "42".to_string());
        assert_eq!(expand_vars(&app, "Eval(1 + 3)"), "4");
        assert_eq!(expand_vars(&app, "Eval(x)"), "42");
        // 数値リテラル
        assert_eq!(expand_vars(&app, "Eval(7)"), "7");
    }

    #[test]
    fn roundup_rounddown_with_digits() {
        let app = App::new();
        assert_eq!(expand_vars(&app, "RoundUp(1.23, 1)"), "1.3");
        assert_eq!(expand_vars(&app, "RoundDown(1.29, 1)"), "1.2");
        assert_eq!(expand_vars(&app, "Round(1.234, 2)"), "1.23");
        assert_eq!(expand_vars(&app, "Round(1.235, 2)"), "1.24");
    }

    #[test]
    fn isvardefined_checks_existence() {
        let mut app = App::new();
        app.set_script_var("撃墜数".to_string(), "5".to_string());
        assert_eq!(expand_vars(&app, "IsVarDefined(撃墜数)"), "1");
        assert_eq!(expand_vars(&app, "IsVarDefined(未定義)"), "0");
    }

    /// indexed 変数に対する `IsVarDefined`。引数を展開すると配列要素の値に
    /// 化けて定義済みでも 0 になる回帰。引数は変数名のまま渡す。
    #[test]
    fn isvardefined_handles_indexed_vars() {
        let src = "\
Set 配列[1] hello
Set 配列[2] \"\"
Set idx 1
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // 値ありの indexed key
        assert_eq!(expand_vars(&app, "IsVarDefined(配列[1])"), "1");
        // 空文字を明示代入した key も「定義済み」
        assert_eq!(expand_vars(&app, "IsVarDefined(配列[2])"), "1");
        // 未定義の indexed key
        assert_eq!(expand_vars(&app, "IsVarDefined(配列[3])"), "0");
        // 添字が変数: 配列[idx] → 配列[1] → 定義済み
        assert_eq!(expand_vars(&app, "IsVarDefined(配列[idx])"), "1");
    }

    #[test]
    fn isnumeric_detects_numbers() {
        let app = App::new();
        assert_eq!(expand_vars(&app, "IsNumeric(\"125\")"), "1");
        assert_eq!(expand_vars(&app, "IsNumeric(\"abc\")"), "0");
        assert_eq!(expand_vars(&app, "IsNumeric(\"-1.5\")"), "1");
    }

    #[test]
    fn instr_finds_substring_1indexed() {
        let app = App::new();
        assert_eq!(expand_vars(&app, "InStr(\"hello world\", \"world\")"), "7");
        assert_eq!(expand_vars(&app, "InStr(\"hello\", \"xyz\")"), "0");
    }

    #[test]
    fn format_basic_patterns() {
        let app = App::new();
        // 1234.5 → "1,234.50"
        assert_eq!(
            expand_vars(&app, "Format(1234.5, \"##,##0.00\")"),
            "1,234.50"
        );
        assert_eq!(expand_vars(&app, "Format(5, \"0\")"), "5");
    }

    #[test]
    fn lsearch_finds_position_or_returns_zero() {
        // SRC.Sharp `Functions/List.cs::LSearch` 仕様: 見つからなければ 0。
        let mut app = App::new();
        app.set_script_var("xs".into(), "alpha beta gamma".into());
        assert_eq!(expand_vars(&app, "Lsearch(xs, alpha)"), "1");
        assert_eq!(expand_vars(&app, "Lsearch(xs, gamma)"), "3");
        assert_eq!(expand_vars(&app, "Lsearch(xs, delta)"), "0");
    }

    #[test]
    fn lremove_drops_first_match() {
        let mut app = App::new();
        app.set_script_var("xs".into(), "a b c b".into());
        assert_eq!(expand_vars(&app, "Lremove(xs, b)"), "a c b");
    }

    #[test]
    fn line_with_bf_emits_fillrect() {
        let src = "Line 10 20 50 60 BF\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let cmds = &app.script_overlay().cmds;
        let has_fillrect = cmds.iter().any(|c| {
            matches!(
                c,
                crate::script_overlay::DrawCmd::FillRect { w, h, .. }
                    if *w > 0.0 && *h > 0.0
            )
        });
        assert!(has_fillrect, "expected FillRect, got: {:?}", cmds);
    }

    #[test]
    fn wait_duration_sets_pending_timer_and_suspends() {
        // `Wait 0.5` で pending_timer がセットされ、後続命令の実行が停止する。
        let src = "\
Set first 1
Wait 0.5
Set second 2
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("first"), "1");
        // タイマがセットされ後続命令は未実行
        assert!(app.pending_timer().is_some());
        assert_eq!(app.script_var("second"), "");
        // タイマ満了をシミュレート → resume で残り命令が走る
        app.tick(0.6);
        assert!(app.pending_timer().is_none());
        assert_eq!(app.script_var("second"), "2");
    }

    #[test]
    fn audio_commands_push_pending_audio() {
        use crate::audio::AudioRequest;
        let src = "\
Startbgm Opening.mid
Playsound click.wav
Stopbgm
Keepbgm
PlayVoice voice.ogg
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let reqs = app.take_pending_audio();
        assert_eq!(reqs.len(), 5);
        assert_eq!(
            reqs[0],
            AudioRequest::StartBgm {
                name: "Opening.mid".into()
            }
        );
        assert_eq!(
            reqs[1],
            AudioRequest::PlaySound {
                name: "click.wav".into()
            }
        );
        assert_eq!(reqs[2], AudioRequest::StopBgm);
        assert_eq!(reqs[3], AudioRequest::KeepBgm);
        assert_eq!(
            reqs[4],
            AudioRequest::PlayVoice {
                name: "voice.ogg".into()
            }
        );
    }

    #[test]
    fn condition_resolves_bare_identifier_as_variable() {
        // SRC: `If 選択 = ランダム` のように `$()` を付けない条件比較は、裸の
        // 識別子を変数 (script_var) として自動解決する。リテラル値の場合は
        // そのまま比較。
        let src = "\
Set 選択 ランダム
If 選択 = ランダム Then
　Stage matched
Else
　Stage missed
EndIf
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.stage(), "matched");
    }

    #[test]
    fn system_variables_resolve_dynamically() {
        // `味方数` / `敵数` / `ターン数` 等のシステム変数が動的に解決される。
        let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Pilot リオ リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ブレイバー Real 1 0 陸 5 M 1000 100 5000 110 1500 100 AAAA
Place ゾルダ ガロ 敵 5 5
Place ブレイバー リオ 味方 1 1
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // If 味方数 = 1 が true で Stage matched が走る
        let src2 = "\
If 味方数 = 1 Then
　Stage matched_player
Else
　Stage no_match
EndIf
If 敵数 = 1 Then
　Set 敵 1
EndIf
";
        let stmts2 = event::parse(src2).unwrap();
        execute(&mut app, &stmts2).unwrap();
        assert_eq!(app.stage(), "matched_player");
        assert_eq!(app.script_var("敵"), "1");
    }

    #[test]
    fn create_assigns_unique_uid_and_escape_resolves_target_unit_id() {
        // 同名パイロットを複数回 Create しても、対象ユニットＩＤ は一意の uid を
        // 保持し、Escape 対象ユニットＩＤ は最後に作成したユニットだけを退避する。
        let src = "\
Pilot パイロット不在 - - 汎用 AAAA 0 0 0 0 0 0 0
Unit ブレイバー Real 1 0 陸 5 M 1000 100 5000 110 1500 100 AAAA
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Create 味方 ブレイバー 0 パイロット不在 1 1 1 非同期
Escape 対象ユニットＩＤ
Create 味方 ゾルダ 0 パイロット不在 1 5 5 非同期
Escape 対象ユニットＩＤ
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // 両ユニットが off_map になっているはず
        let off_map_count = app
            .database()
            .unit_instances
            .iter()
            .filter(|u| u.off_map)
            .count();
        assert_eq!(off_map_count, 2);
        // uid は単調増加
        assert_eq!(app.database().unit_instances[0].uid, "U1");
        assert_eq!(app.database().unit_instances[1].uid, "U2");
    }

    #[test]
    fn character_creation_loop_preserves_roster_through_removepilot() {
        // 機体選択 Case 2 の `Create → Escape → Getoff → Removepilot` ループを
        // 模擬。Removepilot が unit を巻き込まずに pilot 名だけ消し、
        // ロスター (off_map=true) が蓄積する。
        let src = "\
Pilot パイロット不在 - - 汎用 AAAA 0 0 0 0 0 0 0
Unit ブレイバー Real 1 0 陸 5 M 1000 100 5000 110 1500 100 AAAA
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Set 入手ユニット ブレイバー
Create 味方 入手ユニット 0 パイロット不在 1 1 1 非同期
Escape 対象ユニットＩＤ
Getoff パイロット不在
Removepilot パイロット不在
Pilot パイロット不在 - - 汎用 AAAA 0 0 0 0 0 0 0
Set 入手ユニット ゾルダ
Create 味方 入手ユニット 0 パイロット不在 1 1 1 非同期
Escape 対象ユニットＩＤ
Getoff パイロット不在
Removepilot パイロット不在
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // 2 体のユニットがロスターに残る (両方 off_map)
        assert_eq!(app.database().unit_instances.len(), 2);
        let off_map_count = app
            .database()
            .unit_instances
            .iter()
            .filter(|u| u.off_map && u.party == crate::Party::Player)
            .count();
        assert_eq!(off_map_count, 2);
        // パイロット定義は両方とも除去済み
        assert!(app.database().pilots.is_empty());
    }

    #[test]
    fn organize_deploys_off_map_player_units() {
        // Escape で退避した Player ユニットが Organize で再配置される。
        let src = "\
Pilot リオ リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ブレイバー Real 1 0 陸 5 M 1000 100 5000 110 1500 100 AAAA
Place ブレイバー リオ 味方 0 0
Escape リオ
Organize 5 7 7
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let u = &app.database().unit_instances[0];
        assert!(!u.off_map, "Organize should re-deploy");
        // (7, 7) 中心 / 同マス空きなので (7, 7) そのもの
        assert_eq!((u.x, u.y), (7, 7));
    }

    #[test]
    fn call_can_target_label_in_previously_loaded_eve() {
        // 別 .eve に定義されたラベルへも Call/Goto がジャンプできること。
        // 複数 .eve 読込時、過去にロードした script_library 内のラベルを
        // 解決できることを担保。スパロボ戦記の
        //   Require \"eve\\EventBattle.eve\"
        //   Call \"イベントバトル\"
        // パターンが動くために必要。
        let mut app = App::new();
        // 1 つ目: イベントバトル: ラベル + Stage を設定 + Return
        let src1 = "\
イベントバトル:
Stage event_called
Return
";
        let stmts1 = event::parse(src1).unwrap();
        execute(&mut app, &stmts1).unwrap();
        // 1 つ目の execute は Return で抜けるだけ (top-level に到達しない)
        // 2 つ目: top-level から Call イベントバトル
        let src2 = "\
Call イベントバトル
Set 完了 ok
";
        let stmts2 = event::parse(src2).unwrap();
        execute(&mut app, &stmts2).unwrap();
        // Call が別 .eve のラベルにジャンプ → Stage event_called → Return →
        // 2 つ目の continuation で Set 完了 ok
        assert_eq!(app.stage(), "event_called");
        assert_eq!(app.script_var("完了"), "ok");
    }

    #[test]
    fn menu_options_expand_var_references() {
        // Menu / Ask Format 1 の選択肢行に `$(var)` / 関数呼出 を含む場合、
        // 表示時に値展開する。バトルセレクト画面の
        //   Ask "..." キャンセル可
        //   出撃
        //   出撃可能機体確認
        //   $(ボーナス確認選択肢)
        //   End
        // で 3 番目が "ボーナス確認" 等に置換されること。
        let src = "\
Set ボーナス確認選択肢 ボーナス確認（入手済み）
Ask 選択してください
出撃
出撃可能機体確認
$(ボーナス確認選択肢)
End
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        match app.pending_dialog().expect("dialog") {
            crate::PendingDialog::Menu { options, .. } => {
                assert_eq!(options[0], "出撃");
                assert_eq!(options[1], "出撃可能機体確認");
                assert_eq!(options[2], "ボーナス確認（入手済み）");
            }
            _ => panic!("expected Menu"),
        }
    }

    #[test]
    fn single_line_if_with_not_prefix_and_inline_body() {
        // `If Not 選択 = 1 Goto X` — Then 省略 + Not 接頭辞 + inline body。
        let src = "\
Set 選択 2
If Not 選択 = 1 Goto skipme
Set never 1
skipme:
Stage hit
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("never"), "");
        assert_eq!(app.stage(), "hit");
    }

    #[test]
    fn switch_value_resolves_bare_identifier_as_variable() {
        // SRC `Switch 選択` — 単一識別子の value は script_var として
        // 自動解決する。BattleSelect.eve の `Switch 選択 / Case イベントバトル`
        // パターンが動くこと。
        let src = "\
Set 選択 イベントバトル
Switch 選択
Case フリーバトル
　Stage free
Case イベントバトル
　Stage event
Case Else
　Stage other
EndSw
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.stage(), "event");
    }

    #[test]
    fn asterisk_label_can_be_triggered() {
        // SRC `*ラベル:` 形式 (イベントハンドラ用) もラベルテーブルに登録され、
        // `trigger_label("ラベル")` で発火する。
        let src = "\
*ハンドラ:
Stage fired
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        // 直接 execute せずライブラリ登録だけ走らせる
        app.script_library_mut().append(&stmts);
        assert!(trigger_label(&mut app, "ハンドラ"));
        assert_eq!(app.stage(), "fired");
    }

    #[test]
    fn escape_then_launch_keeps_unit_for_redeploy() {
        // SRC `Escape` → `Launch` の往復: ユニットは削除せず off_map で退避し、
        // Launch で再配置できる。
        let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Place ゾルダ ガロ 敵 5 5
Escape ガロ
Launch ガロ 9 10
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // ユニットは消えていない (off_map で残っている)
        assert_eq!(app.database().unit_instances.len(), 1);
        let u = &app.database().unit_instances[0];
        assert!(!u.off_map, "Launch should reset off_map");
        assert_eq!(u.x, 9);
        assert_eq!(u.y, 10);
    }

    #[test]
    fn obattle_map_pick_loop_terminates_on_confirm_yes() {
        // スパロボ戦記 OBattle.eve の Ask → If → Confirm → ループ
        // パターンが正しく抜けることを確認。
        let src = "\
Set マップ名称[ランダム] ランダム
Set マップ名称[1] 草原
オリジナル機バトルマップ選択:
Ask マップ名称 マップを選んでください キャンセル可
If 選択 = ランダム or 選択 = \"\" Then
　Set マップ決定 1
Else
　Set マップ決定 選択
Endif
ChangeMap \"map\\map-$(マップ決定).map\"
Confirm このマップでいいですか？
If Not 選択 = 1 Then
　Goto オリジナル機バトルマップ選択
Endif
Set 完了 ok
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // 1) Ask が出ているはず
        match app.pending_dialog().expect("dialog 1") {
            crate::PendingDialog::Menu {
                options,
                store_value,
                ..
            } => {
                assert!(*store_value);
                assert!(options.iter().any(|o| o == "ランダム"));
            }
            other => panic!("expected Menu, got {:?}", other),
        }
        // 2) 先頭の選択肢を選ぶ (1-indexed)。Format 2 なので `選択` には
        //    その要素の添字が入る。
        assert!(app.respond_dialog(1));
        // 3) Confirm が出ているはず
        match app.pending_dialog().expect("dialog 2") {
            crate::PendingDialog::Confirm { question, .. } => {
                assert!(question.contains("このマップ"));
            }
            other => panic!("expected Confirm, got {:?}", other),
        }
        // 4) Yes (internal choice=0 → SRC 仕様 選択="1")
        assert!(app.respond_dialog(0));
        // 5) ループを抜けて Set 完了 ok まで進む
        assert!(app.pending_dialog().is_none());
        assert_eq!(app.script_var("完了"), "ok");
    }

    #[test]
    fn set_resolves_single_bare_identifier_value() {
        // `Set マップ決定 選択` で 選択 の値を マップ決定 に複製。
        let src = "\
Set 選択 草原A
Set マップ決定 選択
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("マップ決定"), "草原A");
    }

    #[test]
    fn ask_format2_uses_array_keys_as_options() {
        // `Ask array message [opts]` 形式 (SRC Ask Format 2)。
        // 先頭引数 (array 名) で indexed-var の key を取って options 化する。
        // 後続行は選択肢として吸い込まれない (= If ... が消費されない)。
        let src = "\
Set マップ名称[ランダム] ランダム
Set マップ名称[1] 草原
Set マップ名称[2] 砂漠
Ask マップ名称 マップを選んでください キャンセル可
If 選択 = ランダム Then
　Stage rand_branch
EndIf
Set 後続 1
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // ダイアログが出ているはず
        let dlg = app.pending_dialog().expect("dialog");
        let crate::PendingDialog::Menu {
            options,
            option_keys,
            ..
        } = dlg
        else {
            panic!("expected Menu");
        };
        // 表示は array 要素の **値**。
        assert!(options.iter().any(|o| o == "ランダム"));
        assert!(options.iter().any(|o| o == "草原"));
        assert!(options.iter().any(|o| o == "砂漠"));
        // option_keys には対応する **添字** が同順で並ぶ (SRC Ask Format 2 は
        // 選んだ要素の添字を `選択` に格納する)。
        assert_eq!(option_keys.len(), options.len());
        assert!(option_keys.iter().any(|k| k == "ランダム"));
        assert!(option_keys.iter().any(|k| k == "1"));
        assert!(option_keys.iter().any(|k| k == "2"));
        // 後続行はまだ実行されていない
        assert_eq!(app.script_var("後続"), "");
    }

    #[test]
    fn combine_changes_unit_data_name() {
        let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Unit ゾルダⅡ改 Mass 1 0 陸 5 M 1500 110 2800 90 1000 90 BBBB
Place ゾルダ ガロ 敵 5 5
Combine ガロ ゾルダⅡ改
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.database().unit_instances[0].unit_data_name, "ゾルダⅡ改");
    }

    #[test]
    fn split_reads_separation_feature() {
        // ユニットに `分離=` 特殊能力を仕込み、Split で先頭の派生機体に戻す
        use crate::data::pilot::{Adaption, Sex};
        use crate::data::unit::{Size, UnitData};
        let mut app = App::new();
        app.database_mut()
            .pilots
            .push(crate::data::pilot::PilotData {
                spirit_commands: Vec::new(),
                name: "ベース".into(),
                nickname: "B".into(),
                kana_name: "B".into(),
                sex: Sex::Unspecified,
                class: String::new(),
                adaption: Adaption::parse("AAAA").unwrap(),
                exp_value: 0,
                infight: 0,
                shooting: 0,
                hit: 0,
                dodge: 0,
                intuition: 0,
                technique: 0,
                personality: None,
                sp: None,
                bgm: None,
                bitmap: None,
                features: Vec::new(),
            });
        let merged = UnitData {
            abilities: Vec::new(),
            name: "合体形態".into(),
            kana_name: "合体形態".into(),
            nickname: "合".into(),
            class: String::new(),
            pilot_num: 1,
            item_num: 0,
            transportation: "陸".into(),
            speed: 5,
            size: Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 200,
            armor: 1500,
            mobility: 80,
            adaption: Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: Vec::new(),
            features: vec![("分離".to_string(), "非表示 ベースA ベースB".to_string())],
        };
        app.database_mut().units.push(merged);
        // 分離先のユニットデータ
        app.database_mut().units.push(UnitData {
            abilities: Vec::new(),
            name: "ベースA".into(),
            kana_name: "ベースA".into(),
            nickname: "ベースA".into(),
            class: String::new(),
            pilot_num: 1,
            item_num: 0,
            transportation: "陸".into(),
            speed: 5,
            size: Size::M,
            value: 0,
            exp_value: 0,
            hp: 2500,
            en: 100,
            armor: 800,
            mobility: 90,
            adaption: Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: Vec::new(),
            features: Vec::new(),
        });
        app.database_mut().register_unit(crate::UnitInstance::new(
            "合体形態",
            "ベース",
            crate::Party::Player,
            1,
            1,
        ));
        execute(&mut app, &event::parse("Split ベース\n").unwrap()).unwrap();
        assert_eq!(app.database().unit_instances[0].unit_data_name, "ベースA");
    }

    /// `Split` 完了時に `分離 <unit> <old_name>:` ラベルが発火することを検証。
    /// `21_transform_combine_split_autofire.eve` fixture では feature を注入
    /// できないため、本 unit test 側で programmatic にユニットを構築する。
    #[test]
    fn split_fires_separation_event_label() {
        use crate::data::pilot::{Adaption, Sex};
        use crate::data::unit::{Size, UnitData};
        let mut app = App::new();
        app.database_mut()
            .pilots
            .push(crate::data::pilot::PilotData {
                spirit_commands: Vec::new(),
                name: "ベース".into(),
                nickname: "B".into(),
                kana_name: "B".into(),
                sex: Sex::Unspecified,
                class: String::new(),
                adaption: Adaption::parse("AAAA").unwrap(),
                exp_value: 0,
                infight: 0,
                shooting: 0,
                hit: 0,
                dodge: 0,
                intuition: 0,
                technique: 0,
                personality: None,
                sp: None,
                bgm: None,
                bitmap: None,
                features: Vec::new(),
            });
        let merged = UnitData {
            abilities: Vec::new(),
            name: "合体形態".into(),
            kana_name: "合体形態".into(),
            nickname: "合".into(),
            class: String::new(),
            pilot_num: 1,
            item_num: 0,
            transportation: "陸".into(),
            speed: 5,
            size: Size::M,
            value: 0,
            exp_value: 0,
            hp: 5000,
            en: 200,
            armor: 1500,
            mobility: 80,
            adaption: Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: Vec::new(),
            features: vec![("分離".to_string(), "非表示 ベースA ベースB".to_string())],
        };
        app.database_mut().units.push(merged);
        app.database_mut().units.push(UnitData {
            abilities: Vec::new(),
            name: "ベースA".into(),
            kana_name: "ベースA".into(),
            nickname: "ベースA".into(),
            class: String::new(),
            pilot_num: 1,
            item_num: 0,
            transportation: "陸".into(),
            speed: 5,
            size: Size::M,
            value: 0,
            exp_value: 0,
            hp: 2500,
            en: 100,
            armor: 800,
            mobility: 90,
            adaption: Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: Vec::new(),
            features: Vec::new(),
        });
        app.database_mut().register_unit(crate::UnitInstance::new(
            "合体形態",
            "ベース",
            crate::Party::Player,
            1,
            1,
        ));
        // 分離後の発火 label を仕込む。SRC 仕様: `分離 <unit> <old_name>:`
        // old_name は分離 **前** の形態 (= "合体形態") を指す。
        let src = "
Split ベース
Message done
Exit

分離 ベース 合体形態:
Set split_fired 1
Return
";
        execute(&mut app, &event::parse(src).unwrap()).unwrap();
        assert_eq!(
            app.database().unit_instances[0].unit_data_name,
            "ベースA",
            "split は分離先 ベースA へ切り替えるはず"
        );
        assert_eq!(
            app.script_var("split_fired"),
            "1",
            "`分離 ベース 合体形態:` ラベルが発火するはず"
        );
    }

    /// `fire_action_end_labels` 単独テスト — UI 経由でユニットが行動終了したとき
    /// `行動終了 <unit>:` ラベルが pilot/unit/party 名いずれかで 1 度発火する。
    /// fixture 経由では UnitAction 経路を駆動できないため lib テストで検証する。
    #[test]
    fn fire_action_end_labels_resolves_identifier_priority() {
        let src = "
Pilot \"リオ\" リオ 男性 一般 BBBC 50 100 120 110 110 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
Place ブレイバー リオ Player 0 0
Exit

行動終了 リオ:
Set fired_pilot 1
Return

行動終了 ブレイバー:
Set fired_unit 1
Return

行動終了 味方:
Incr fired_party
Return
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // ユニットがちゃんと配置されている前提。
        assert_eq!(app.database().unit_instances.len(), 1);
        // 行動終了発火: pilot 名先勝。
        fire_action_end_labels(&mut app, 0);
        assert_eq!(app.script_var("fired_pilot"), "1", "pilot 名 label が先勝");
        assert_eq!(
            app.script_var("fired_unit"),
            "",
            "pilot にヒットしたら unit は発火しない"
        );
        assert_eq!(
            app.script_var("fired_party"),
            "",
            "pilot にヒットしたら party も発火しない"
        );

        // 2 回目: 再度発火させても pilot label が再実行 (state はそのまま)。
        fire_action_end_labels(&mut app, 0);
        assert_eq!(
            app.script_var("fired_pilot"),
            "1",
            "Set 文なので同じ値で再代入されるだけ (Incr は無いため不変)"
        );
    }

    /// `fire_attack_event_labels` / `fire_after_attack_event_labels` 単独テスト。
    /// 戦闘 UI 経路の `attack_target` で発火するため fixture 不可。
    #[test]
    fn fire_attack_and_after_attack_event_labels_basic() {
        let src = "
Pilot \"リオ\" リオ 男性 一般 BBBC 50 100 120 110 110 100 100
Pilot \"ガロ\" ガロ 男性 一般 BBBC 50 100 120 110 110 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2500 110 1000 100 BBBC
Place ブレイバー リオ Player 1 1
Place ゾルダ ガロ Enemy 5 5
Exit

攻撃 リオ ガロ:
Set pre_pilot 1
Return

攻撃後 リオ ガロ:
Set post_pilot 1
Return
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.database().unit_instances.len(), 2);
        let atk_idx = 0; // ブレイバー/リオ
        let def_idx = 1; // ゾルダ/ガロ
                         // 攻撃 (pre) のみ発火を確認。
        fire_attack_event_labels(&mut app, atk_idx, def_idx);
        assert_eq!(app.script_var("pre_pilot"), "1", "攻撃 リオ ガロ が発火");
        assert_eq!(app.script_var("post_pilot"), "", "攻撃後 はまだ発火しない");

        // 攻撃後 (post): 両者生存ならば発火。
        let atk = UnitEventId::from_unit_instance(&app.database().unit_instances[atk_idx]);
        let def = UnitEventId::from_unit_instance(&app.database().unit_instances[def_idx]);
        fire_after_attack_event_labels(&mut app, &atk, &def);
        assert_eq!(app.script_var("post_pilot"), "1", "攻撃後 リオ ガロ が発火");

        // 撃破済 (defender 不在) は post fire しない。
        app.database_mut().unit_instances.remove(def_idx);
        app.set_script_var("post_pilot".into(), "0".into());
        fire_after_attack_event_labels(&mut app, &atk, &def);
        assert_eq!(
            app.script_var("post_pilot"),
            "0",
            "defender 不在では 攻撃後 は発火しないはず"
        );
    }

    /// `対象ユニット使用武器` / `相手ユニット使用武器` 等の戦闘イベント系
    /// システム変数が `攻撃イベント` 発火前に設定されていることを確認。
    /// `attack_target()` がセットした値をイベント内スクリプトから `$(...)` で
    /// 参照できれば OK (N2 実装検証)。
    #[test]
    fn attack_event_system_vars_are_readable_in_event() {
        let src = "
Pilot \"リオ\" リオ 男性 一般 BBBC 50 100 120 110 110 100 100
Pilot \"ガロ\" ガロ 男性 一般 BBBC 50 100 120 110 110 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
  武器 ビームライフル 実弾 3000 1 5 90 10
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2500 110 1000 100 BBBC
  武器 ゾルダバズーカ 実弾 2500 1 3 80 8
Place ブレイバー リオ Player 1 1
Place ゾルダ ガロ Enemy 3 3
Exit

攻撃 リオ ガロ:
Set atk_weapon $(対象ユニット使用武器)
Set atk_weapon_num $(対象ユニット使用武器番号)
Return

攻撃後 リオ ガロ:
Set def_weapon $(相手ユニット使用武器)
Return
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.database().unit_instances.len(), 2);

        // attack_target の代わりに手動で戦闘イベント変数をセットしてから発火。
        // これは attack_target() が行う設定と同じパターンを再現する。
        app.set_script_var(
            "対象ユニット使用武器".to_string(),
            "ビームライフル".to_string(),
        );
        app.set_script_var("対象ユニット使用武器番号".to_string(), "1".to_string());
        app.set_script_var("相手ユニット使用武器".to_string(), String::new());
        app.set_script_var("相手ユニット使用武器番号".to_string(), "0".to_string());

        fire_attack_event_labels(&mut app, 0, 1);
        assert_eq!(
            app.script_var("atk_weapon"),
            "ビームライフル",
            "攻撃イベント内で 対象ユニット使用武器 を読める"
        );
        assert_eq!(
            app.script_var("atk_weapon_num"),
            "1",
            "攻撃イベント内で 対象ユニット使用武器番号 を読める"
        );

        // 攻撃後イベントで 相手ユニット使用武器 が参照できる。
        app.set_script_var(
            "相手ユニット使用武器".to_string(),
            "ゾルダバズーカ".to_string(),
        );
        let atk = UnitEventId::from_unit_instance(&app.database().unit_instances[0]);
        let def = UnitEventId::from_unit_instance(&app.database().unit_instances[1]);
        fire_after_attack_event_labels(&mut app, &atk, &def);
        assert_eq!(
            app.script_var("def_weapon"),
            "ゾルダバズーカ",
            "攻撃後イベント内で 相手ユニット使用武器 を読める"
        );
    }

    /// `attack_target()` 実機経路が `対象/相手 ユニットＩＤ` と
    /// `対象/相手 パイロット` を `攻撃イベント` 発火前に設定することを検証する
    /// (§D / §7.5)。`攻撃イベント.md`: 陣営名で指定された攻撃イベントが実際に
    /// 攻撃した/された個別ユニットを識別するために必要なシステム変数。
    #[test]
    fn attack_target_sets_target_and_opponent_identity_vars() {
        let src = "
Pilot \"リオ\" リオ 男性 一般 BBBC 50 100 120 110 110 100 100
Pilot \"ガロ\" ガロ 男性 一般 BBBC 50 100 120 110 110 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2500 110 1000 100 BBBC
Weapon ブレイバー ビームライフル 3000 1 5 90 10
Weapon ゾルダ ゾルダバズーカ 2500 1 3 80 8
Place ブレイバー リオ Player 1 1
Place ゾルダ ガロ Enemy 3 3
Exit

攻撃 リオ ガロ:
Set cap_atk_id $(対象ユニットＩＤ)
Set cap_atk_pilot $(対象パイロット)
Set cap_def_id $(相手ユニットＩＤ)
Set cap_def_pilot $(相手パイロット)
Return
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.database().unit_instances.len(), 2);
        let atk_uid = app.database().unit_instances[0].uid.clone();
        let def_uid = app.database().unit_instances[1].uid.clone();

        // 実戦闘経路を起動: MapView + Battle + マップ。
        app.database_mut().replace_map(crate::data::map::demo());
        app.set_scene(crate::Scene::MapView);
        app.set_stage_state(crate::stage::StageState::Battle);
        // プレイヤー操作を再現: ブレイバー(1,1)選択 → 攻撃コマンド → ゾルダ(3,3)を対象にクリック。
        let px = |c: u32| (c * 32 + 16) as i32;
        let py = |r: u32| (r * 32 + 16) as i32;
        app.handle_input(crate::Input::ClickAt { x: px(1), y: py(1) }); // ユニットメニュー
        assert!(
            app.handle_input(crate::Input::AttackTarget),
            "攻撃コマンドを選択できない (射程内に対象なし等)"
        );
        app.handle_input(crate::Input::ClickAt { x: px(3), y: py(3) }); // 攻撃対象を確定

        // 攻撃元 = 対象 / 攻撃先 = 相手。
        assert_eq!(app.script_var("cap_atk_pilot"), "リオ");
        assert_eq!(app.script_var("cap_def_pilot"), "ガロ");
        assert_eq!(
            app.script_var("cap_atk_id"),
            atk_uid,
            "対象ユニットＩＤ = 攻撃側 uid"
        );
        assert_eq!(
            app.script_var("cap_def_id"),
            def_uid,
            "相手ユニットＩＤ = 防御側 uid"
        );
    }

    /// `fire_entry_event_labels` — 移動完了時の `進入 <unit> <x> <y>` / `脱出
    /// <unit> <dir>` 発火を検証する。座標形式 + 地形形式 + マップ端方位の
    /// 4 シナリオを一括検証。
    #[test]
    fn fire_entry_event_labels_coord_terrain_and_escape() {
        let src = "
Pilot \"アリス\" アリス 男性 一般 BBBC 50 100 120 110 110 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
MapSize 5 5
Place ブレイバー アリス Player 0 0
Exit

進入 アリス 2 3:
Set entry_coord 1
Return

# 0-based 座標 (4, 0) は右上端 → 進入 + 脱出 N + 脱出 E が連鎖発火する。
進入 ブレイバー 4 0:
Set entry_unit 1
Return

脱出 味方 N:
Incr escape_north
Return

脱出 味方 E:
Incr escape_east
Return
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();

        // (2, 3) へ移動 → 進入 アリス 2 3 発火 (端ではないので脱出は無し)
        app.database_mut().unit_instances[0].x = 2;
        app.database_mut().unit_instances[0].y = 3;
        fire_entry_event_labels(&mut app, 0);
        assert_eq!(app.script_var("entry_coord"), "1");
        assert_eq!(app.script_var("escape_north"), "");
        assert_eq!(app.script_var("escape_east"), "");

        // (4, 0) へ移動 → 右上端 → 進入 + 脱出 N + 脱出 E
        app.database_mut().unit_instances[0].x = 4;
        app.database_mut().unit_instances[0].y = 0;
        fire_entry_event_labels(&mut app, 0);
        assert_eq!(app.script_var("entry_unit"), "1");
        assert_eq!(app.script_var("escape_north"), "1");
        assert_eq!(app.script_var("escape_east"), "1");
    }

    /// `App::fire_resume_event` — from_save_json 後の `再開:` ラベル発火を検証。
    /// シリアライズ前に `再開:` ラベルを ScriptLibrary に登録し、デシリアライズ
    /// した App でも label が引けることを確認 (ScriptLibrary は serde 対象)。
    #[test]
    fn fire_resume_event_after_load_runs_resume_label() {
        let src = r#"
Pilot "ジェイ" J 男性 一般 BBBC 50 100 100 100 100 100 100
Unit ノーマル リアル系 1 0 陸 5 M 1000 100 2000 100 1000 100 BBBC
Place ノーマル ジェイ Player 1 1
Exit

再開:
Set resumed 1
Return
"#;
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // セーブ → 別 App に復元 → fire_resume_event
        let json = app.to_save_json().unwrap();
        let mut restored = App::from_save_json(&json).unwrap();
        assert_eq!(restored.script_var("resumed"), "", "ロード直後は未発火");
        let fired = restored.fire_resume_event();
        assert!(fired, "再開: ラベルが定義されているので発火するはず");
        assert_eq!(restored.script_var("resumed"), "1");
    }

    /// `fire_use_event_labels` / `fire_after_use_event_labels` — 武器/アビリティ
    /// 使用前後で `使用 <unit> <device>:` / `使用後 <unit> <device>:` が発火する。
    /// 使用後は attacker 生存時のみ。
    #[test]
    fn fire_use_and_after_use_event_labels_basic() {
        let src = "
Pilot \"リオ\" リオ 男性 一般 BBBC 50 100 120 110 110 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
Place ブレイバー リオ Player 1 1
Exit

使用 リオ ライフル:
Incr use_pilot
Return

使用後 リオ ライフル:
Incr post_pilot
Return
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        fire_use_event_labels(&mut app, 0, "ライフル");
        assert_eq!(
            app.script_var("use_pilot"),
            "1",
            "使用 リオ ライフル が発火"
        );
        // 使用後: attacker 生存
        let atk = UnitEventId::from_unit_instance(&app.database().unit_instances[0]);
        fire_after_use_event_labels(&mut app, &atk, "ライフル");
        assert_eq!(
            app.script_var("post_pilot"),
            "1",
            "使用後 リオ ライフル が発火"
        );
        // 使用後: attacker 撃破済なら発火しない
        app.database_mut().unit_instances.clear();
        app.set_script_var("post_pilot".into(), "0".into());
        fire_after_use_event_labels(&mut app, &atk, "ライフル");
        assert_eq!(
            app.script_var("post_pilot"),
            "0",
            "attacker 不在では 使用後 は発火しないはず"
        );
    }

    /// `fire_contact_event_labels` — 4 近傍ユニットとの組合せで `接触 <unit1>
    /// <unit2>` が発火する。pilot × unit × party のクロスマッチも保証。
    #[test]
    fn fire_contact_event_labels_neighbors() {
        let src = "
Pilot \"アリス\" アリス 男性 一般 BBBC 50 100 120 110 110 100 100
Pilot \"ボブ\" ボブ 男性 一般 BBBC 50 100 120 110 110 100 100
Pilot \"イヴ\" イヴ 男性 一般 BBBC 50 100 120 110 110 100 100
Unit U1 リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
Unit U2 リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
Unit U3 リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
MapSize 5 5
Place U1 アリス Player 2 2
Place U2 ボブ Enemy 3 2
Place U3 イヴ Enemy 2 1
Exit

接触 アリス ボブ:
Set ab 1
Return

接触 アリス イヴ:
Set ae 1
Return
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        fire_contact_event_labels(&mut app, 0);
        // アリスの隣接 (3,2)=ボブ, (2,1)=イヴ で 2 件発火するはず。
        assert_eq!(app.script_var("ab"), "1");
        assert_eq!(app.script_var("ae"), "1");
    }

    /// `攻撃 <atk_pilot> <def_party>` / `攻撃 <atk_party> <def_unit>` のような
    /// pilot ↔ unit ↔ party の交差マッチもサポートする (3x3 全 9 組合せ)。
    /// 優先順は (pilot → unit → party) × (pilot → unit → party) で
    /// 最初にヒットしたものを発火する。
    #[test]
    fn fire_attack_event_labels_cross_identifiers() {
        let src = "
Pilot \"リオ\" リオ 男性 一般 BBBC 50 100 120 110 110 100 100
Pilot \"ガロ\" ガロ 男性 一般 BBBC 50 100 120 110 110 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 BBBC
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2500 110 1000 100 BBBC
Place ブレイバー リオ Player 1 1
Place ゾルダ ガロ Enemy 5 5
Exit

# atk = 味方 (party), def = ゾルダ (unit) でクロス指定。
攻撃 味方 ゾルダ:
Set cross_fired 1
Return
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        fire_attack_event_labels(&mut app, 0, 1);
        assert_eq!(
            app.script_var("cross_fired"),
            "1",
            "`攻撃 味方 ゾルダ` (party × unit) もマッチして発火するはず"
        );
    }

    #[test]
    fn mapattack_area_m_zen_is_diamond_around_attacker() {
        // Ｍ全: 攻撃者中心の菱形 (radius = max_range)
        let mut w = crate::data::unit::WeaponData {
            name: "W".into(),
            power: 1,
            min_range: 1,
            max_range: 2,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: "Ｍ全".into(),
            extras: Vec::new(),
        };
        let area = map_attack_area(&w, (5, 5), (10, 10));
        // 菱形 radius=2, 攻撃者中心 (5,5)
        assert!(area.contains(&(5, 5)));
        assert!(area.contains(&(7, 5)));
        assert!(area.contains(&(5, 7)));
        assert!(area.contains(&(6, 6)));
        assert!(!area.contains(&(8, 5)));
        // ターゲット位置 (10,10) は含まれない
        assert!(!area.contains(&(10, 10)));

        // Ｍ投L3: ターゲット中心の菱形 (radius=3)
        w.class = "Ｍ投L3".into();
        let area = map_attack_area(&w, (5, 5), (10, 10));
        assert!(area.contains(&(10, 10)));
        assert!(area.contains(&(13, 10)));
        assert!(!area.contains(&(14, 10)));
    }

    #[test]
    fn mapattack_area_m_choku_is_straight_line() {
        let w = crate::data::unit::WeaponData {
            name: "W".into(),
            power: 1,
            min_range: 1,
            max_range: 3,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical: 0,
            class: "Ｍ直".into(),
            extras: Vec::new(),
        };
        // 攻撃者 (5,5)、ターゲット (8,5) → 右方向の直線
        let area = map_attack_area(&w, (5, 5), (8, 5));
        assert!(area.contains(&(6, 5)));
        assert!(area.contains(&(7, 5)));
        assert!(area.contains(&(8, 5)));
        assert!(!area.contains(&(5, 5)));
        assert!(!area.contains(&(9, 5)));
        assert!(!area.contains(&(6, 4)));
    }

    #[test]
    fn mapattack_damages_all_in_range() {
        let mut app = App::new();
        let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot リオ リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot ノヴァ ノヴァ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ブレイバー Real 1 0 陸 5 M 1000 100 5000 100 1500 100 AAAA
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2000 80 800 80 BBBB
Unit アークシップ 母艦 1 0 空 4 L 5000 200 8000 200 2000 50 ACBA
Weapon ブレイバー メガキャノン 8000 1 3 +0 0
Place ブレイバー リオ 味方 5 5
Place ゾルダ ガロ 敵 6 5
Place アークシップ ノヴァ 味方 4 5
MapAttack リオ メガキャノン 6 5
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        // 敵 (ゾルダ) はマップ攻撃で撃破され、味方 (アークシップ) は対象外なので生存
        let alive_enemies = app
            .database()
            .unit_instances
            .iter()
            .filter(|u| u.party == crate::Party::Enemy)
            .count();
        assert_eq!(alive_enemies, 0);
        let alive_allies = app
            .database()
            .unit_instances
            .iter()
            .filter(|u| u.party == crate::Party::Player)
            .count();
        assert_eq!(alive_allies, 2);
    }

    #[test]
    fn mapweapon_is_alias_of_mapattack() {
        // `MapWeapon` は SRC Ver.1.6 までの旧名称で `MapAttack` と同義。
        // 旧名称でも同じくマップ攻撃が実行されること。
        let mut app = App::new();
        let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot リオ リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ブレイバー Real 1 0 陸 5 M 1000 100 5000 100 1500 100 AAAA
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2000 80 800 80 BBBB
Weapon ブレイバー メガキャノン 8000 1 3 +0 0
Place ブレイバー リオ 味方 5 5
Place ゾルダ ガロ 敵 6 5
MapWeapon リオ メガキャノン 6 5
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        let alive_enemies = app
            .database()
            .unit_instances
            .iter()
            .filter(|u| u.party == crate::Party::Enemy)
            .count();
        assert_eq!(alive_enemies, 0);
    }

    #[test]
    fn bossrank_command_sets_boss_rank() {
        let mut app = App::new();
        let src = "\
Pilot ガロ ガロ 男性 一般 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2000 80 800 80 BBBB
Place ゾルダ ガロ 敵 5 5
BossRank ガロ 3
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        let u = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.pilot_name == "ガロ")
            .unwrap();
        assert_eq!(u.boss_rank, 3, "BossRank コマンドで boss_rank=3");
    }

    #[test]
    fn call_intermission_command_data_save_writes_quicksave() {
        // `.eve CallIntermissionCommand データセーブ` はメニュー経路と同じ実体に
        // 委譲し、現在状態を `__quicksave` script_var へ書き込む。
        let mut app = App::new();
        assert!(app.script_var("__quicksave").is_empty());
        let stmts = event::parse("CallIntermissionCommand データセーブ\n").unwrap();
        execute(&mut app, &stmts).unwrap();
        assert!(
            !app.script_var("__quicksave").is_empty(),
            "データセーブで __quicksave が書かれるべき"
        );
    }

    #[test]
    fn specialpower_consumes_sp_and_recoversp_restores() {
        let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Place ゾルダ ガロ 敵 5 5
";
        // PilotData.sp は Pilot 命令経由では設定されないため、直接書き換える。
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        app.database_mut().pilots[0].sp = Some(60);
        // 熱血 (cost 30) → 残り 30
        execute(&mut app, &event::parse("SpecialPower ガロ 熱血\n").unwrap()).unwrap();
        let u = &app.database().unit_instances[0];
        assert_eq!(u.sp_consumed, 30);
        assert!(u.has_condition("熱血"));
        // もう一度 熱血 → 残り 0
        execute(&mut app, &event::parse("SpecialPower ガロ 熱血\n").unwrap()).unwrap();
        assert_eq!(app.database().unit_instances[0].sp_consumed, 60);
        // 魂 (cost 60) → SP 不足で無発動
        execute(&mut app, &event::parse("SpecialPower ガロ 魂\n").unwrap()).unwrap();
        assert_eq!(app.database().unit_instances[0].sp_consumed, 60);
        assert!(!app.database().unit_instances[0].has_condition("魂"));
        // RecoverSP で全回復 (SRC仕様: pilot + rate の2引数)
        execute(&mut app, &event::parse("RecoverSP ガロ 100\n").unwrap()).unwrap();
        assert_eq!(app.database().unit_instances[0].sp_consumed, 0);
    }

    #[test]
    fn specialpower_applies_status() {
        let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Place ゾルダ ガロ 敵 5 5
SpecialPower ガロ 熱血
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let inst = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.pilot_name == "ガロ")
            .unwrap();
        assert!(inst.has_condition("熱血"));
    }

    #[test]
    fn changemap_swaps_current_map() {
        use crate::data::map::{MapCell, MapData};
        let mut app = App::new();
        // 事前ロード相当: 2 つのマップを store_map
        let mut m1 = MapData::new(2, 2);
        m1.set_cell(
            0,
            0,
            MapCell {
                terrain_id: 1,
                bitmap_no: 0,
            },
        );
        let mut m2 = MapData::new(3, 3);
        m2.set_cell(
            0,
            0,
            MapCell {
                terrain_id: 9,
                bitmap_no: 0,
            },
        );
        app.database_mut().store_map("map-1.map".into(), m1);
        app.database_mut().store_map("map-2.map".into(), m2);
        let src = "\
ChangeMap \"map\\map-2.map\"
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        let m = app.database().map.as_ref().expect("map set");
        assert_eq!(m.width, 3);
        assert_eq!(m.cell(0, 0).terrain_id, 9);
    }

    #[test]
    fn if_supports_and_or() {
        // `If a = b And c = d Then` / `Or` 形式に対応する。
        let src = "\
Set a 1
Set b 1
Set c 2
Set d 2
If $(a) = $(b) And $(c) = $(d) Then
　Stage and_true
Else
　Stage and_false
EndIf
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.stage(), "and_true");

        let src2 = "\
Set a 0
Set b 1
If $(a) = $(b) Or $(a) = 0 Then
　Stage or_true
EndIf
";
        let stmts = event::parse(src2).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.stage(), "or_true");
    }

    #[test]
    fn foreach_over_string_keyed_indexed_var() {
        // `タイトル画面アクション[Ｇ３ブレイバー] = ...` のような文字列キーの
        // インデックス変数を ForEach で走査できる。
        let src = "\
Set arr[ブレイバー] G
Set arr[ゾルダ] Z
Set arr[エルガイム] E
ForEach k In arr
　Set 訪問[$(k)] 1
Next
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("訪問[ブレイバー]"), "1");
        assert_eq!(app.script_var("訪問[ゾルダ]"), "1");
        assert_eq!(app.script_var("訪問[エルガイム]"), "1");
    }

    #[test]
    fn set_eval_resolves_dynamic_lhs() {
        // `Set Eval(a) 100` — a に格納された名前を実際の代入先キーとして使う。
        let src = "\
Set name var_x
Set Eval(name) 99
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("var_x"), "99");
    }

    #[test]
    fn sort_indexed_array_ascending() {
        let src = "\
Set arr[1] 3
Set arr[2] 1
Set arr[3] 2
Sort arr 昇順
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // BTreeMap でキーが昇順なので arr[1] < arr[2] < arr[3] の順で再代入される
        assert_eq!(app.script_var("arr[1]"), "1");
        assert_eq!(app.script_var("arr[2]"), "2");
        assert_eq!(app.script_var("arr[3]"), "3");
    }

    #[test]
    fn paintstring_dash_x_uses_center() {
        // `PaintString - 100 text` — x が "-" のとき SRC 480×480 map window
        // の中央 (240) を使用。
        let src = "PaintString - 100 hello\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let cmds = &app.script_overlay().cmds;
        assert_eq!(cmds.len(), 1);
        match &cmds[0] {
            crate::script_overlay::DrawCmd::PaintString { x, y, text } => {
                assert_eq!(*x, 240.0);
                assert_eq!(*y, 100.0);
                assert_eq!(text, "hello");
            }
            _ => panic!("not PaintString"),
        }
    }

    #[test]
    fn font_parses_size_and_color_in_any_order() {
        // `Font 14pt Bold #5E5E5E` / `Font #5E5E5E` 等の柔軟な並びを許容する。
        let src = "Font 14pt Bold #5E5E5E\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        match &app.script_overlay().cmds[0] {
            crate::script_overlay::DrawCmd::SetFont {
                family,
                size_pt,
                color,
            } => {
                assert_eq!(*size_pt, 14);
                assert_eq!(color, "#5E5E5E");
                assert!(family.starts_with("bold "), "family={family}");
            }
            _ => panic!("not SetFont"),
        }
    }

    #[test]
    fn hotpointstring_renders_and_registers_hotpoint() {
        // `HotpointString x y text` は PaintString に加えて Hotpoint を登録する。
        let src = "HotpointString 100 200 START\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let hp = app.hotpoints();
        assert_eq!(hp.len(), 1);
        assert_eq!(hp[0].name, "START");
        assert_eq!(hp[0].x, 100);
        // y は文字列上端を指すので、Hotpoint 矩形は y - line_height = 200 - 22 = 178
        assert_eq!(hp[0].y, 178);
    }

    #[test]
    fn switch_case_supports_space_separated_multi_value() {
        // `Case A B` (空白区切り) で複数値マッチを受理する。
        let src = "\
Set 選択 START
Switch $(選択)
Case START EXハード
　Stage matched
Case 別
　Stage other
EndSw
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.stage(), "matched");
    }

    #[test]
    fn nickname_looks_up_pilot_first() {
        let mut app = App::new();
        let src = "\
Pilot リオ リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(expand_vars(&app, "Nickname(リオ)"), "リオ");
    }

    #[test]
    fn party_function_returns_party_label() {
        let mut app = App::new();
        let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass-produced 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Place ゾルダ ガロ 敵 5 5
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(expand_vars(&app, "Party(ガロ)"), "敵");
        assert_eq!(expand_vars(&app, "Party(ゾルダ)"), "敵");
    }

    #[test]
    fn unit_data_parser_captures_feature_values() {
        // 実 SRC ライクな unit.txt 文字列をパースして features を読み出す。
        let src = "\
マグナＺ
マグナＺ, まぐなぜっと,汎用, 1, 3
陸, 5, M, 6000, 150
特殊能力
全身画像=非表示 Anime\\Unit\\EC_MGZ.bmp 96 96
ブースト=マジンパワー
4700, 120, 1500, 55
AAAA, MGZ_MagnaZ.bmp
";
        let units = crate::data::unit::parse(src).expect("parses");
        assert_eq!(units.len(), 1);
        let u = &units[0];
        assert_eq!(u.features.len(), 2);
        assert_eq!(u.features[0].0, "全身画像");
        assert_eq!(u.features[0].1, "非表示 Anime\\Unit\\EC_MGZ.bmp 96 96");
        assert_eq!(u.features[1].0, "ブースト");
        assert_eq!(u.features[1].1, "マジンパワー");
        // 後段の HP/Adaption も従来どおり読めること
        assert_eq!(u.hp, 4700);
        assert_eq!(u.bitmap, "MGZ_MagnaZ.bmp");
    }

    #[test]
    fn execute_does_not_run_when_already_paused() {
        // 最初の eve が Wait Click で中断
        let stmts1 = event::parse("Wait Click\n").unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts1).unwrap();
        assert!(app.pending_dialog().is_some());

        // 2 つ目の eve は中断中なので実行されない
        let stmts2 = event::parse("Stage \"BadScenario\"\nMessage \"shouldnotrun\"\n").unwrap();
        execute(&mut app, &stmts2).unwrap();
        assert_eq!(app.stage(), "");
        assert!(app.messages().is_empty());
        // library には登録されているはず
        // (Stage / Message は label じゃないので label_pc は無い。
        //  代わりに statements に積まれているかを総量で確認)
        assert!(app.script_library().statements.len() >= 3);
    }

    #[test]
    fn arithmetic_eval_in_hotpoint() {
        let src = "\
Set n 100
Hotpoint X (($(n) + 50) * 2 - 100) 0 30 30
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // (100 + 50) * 2 - 100 = 200
        assert_eq!(app.hotpoints()[0].x, 200);
    }

    #[test]
    fn do_while_skips_when_false() {
        let src = "\
Set x 10
Do While ($(x) < 5)
  Set ran 1
Loop
Message after
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("ran"), "");
        assert_eq!(app.messages(), &["after".to_string()]);
    }

    #[test]
    fn do_while_iterates_with_incr() {
        let src = "\
Set i 0
Do While ($(i) < 3)
  Incr i
Loop
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("i"), "3");
    }

    #[test]
    fn loop_until_inverts() {
        let src = "\
Set i 0
Do
  Incr i
Loop Until ($(i) >= 3)
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("i"), "3");
    }

    #[test]
    fn break_exits_loop() {
        let src = "\
Set i 0
Do
  Incr i
  If $(i) = 5 Then
    Break
  EndIf
Loop
Message done
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("i"), "5");
        assert_eq!(app.messages(), &["done".to_string()]);
    }

    #[test]
    fn list_and_llength_lindex_functions() {
        let src = "\
Set xs List(a, b, c, d)
Set n Llength($(xs))
Set v Lindex($(xs), 3)
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("xs"), "a b c d");
        assert_eq!(app.script_var("n"), "4");
        assert_eq!(app.script_var("v"), "c");
    }

    #[test]
    fn list_evaluates_parenthesized_arithmetic_elements() {
        // スパロボ戦記 `Set 配置場所[8] List(int(幅/2), (高さ - 3))` 相当。
        // 関数評価後に残る括弧式の List 要素を数値化する。
        let src = "\
Set 高さ 27
Set 座標 List(8, (高さ - 3))
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("座標"), "8 24");
    }

    #[test]
    fn replace_function() {
        let src = "\
Set s \"abcabc\"
Set t Replace(s, b, X)
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("t"), "aXcaXc");
    }

    #[test]
    fn not_negates_if_condition() {
        let src = "\
Set x 0
If Not ($(x) = 1) Then
  Message yes
EndIf
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["yes".to_string()]);
    }

    #[test]
    fn wait_click_pauses_with_dialog() {
        let src = "\
Wait Click
Message after
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert!(app.pending_dialog().is_some());
        assert!(app.messages().is_empty());
        // 応答すると Message after が動く
        app.respond_dialog(0);
        assert_eq!(app.messages(), &["after".to_string()]);
    }

    #[test]
    fn case_insensitive_function_names() {
        let src = "\
Set xs \"a b c\"
Set n1 Llength($(xs))
Set n2 LLength($(xs))
Set n3 LENGTH($(xs))
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("n1"), "3");
        assert_eq!(app.script_var("n2"), "3");
        assert_eq!(app.script_var("n3"), "3");
    }

    #[test]
    fn fn_args_resolve_bare_identifier_to_variable() {
        let src = "\
Set foo \"a b c d\"
Set n Llength(foo)
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("n"), "4");
    }

    #[test]
    fn foreach_iterates_party_units() {
        let mut app = App::new();
        setup_two_units(&mut app);
        // Player には ブレイバー 1 体だけ
        let src = "\
Set found 0
ForEach u Player
  Incr found
Next
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("found"), "1");
    }

    #[test]
    fn foreach_iterates_comma_list() {
        let src = "\
Set joined empty
ForEach name a, b, c
  Set joined $(joined)_$(name)
Next
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("joined"), "empty_a_b_c");
    }

    #[test]
    fn foreach_with_empty_collection_skips_body() {
        // 存在しない勢力 → 何も実行されない
        let src = "\
Set ran 0
ForEach u Allied
  Set ran 1
Next
Message done
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("ran"), "0");
        assert_eq!(app.messages(), &["done".to_string()]);
    }

    #[test]
    fn foreach_group_form_iterates_party_units() {
        // SRC 書式1 `ForEach group` — ループ変数を持たず、各反復で
        // `対象パイロット` / `対象ユニットＩＤ` に現在のユニットを束縛する。
        // スパロボ戦記 Include.eve の `Foreach 味方` がこの形。
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "\
Set cnt 0
Set last \"\"
ForEach 味方
  Incr cnt
  Set last $(対象パイロット)
Next
";
        execute(&mut app, &event::parse(src).unwrap()).unwrap();
        // Player は ブレイバー/リオ 1 体。
        assert_eq!(app.script_var("cnt"), "1");
        assert_eq!(app.script_var("last"), "リオ");
    }

    #[test]
    fn foreach_group_form_all_iterates_every_unit() {
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "Set cnt 0\nForEach 全\n  Incr cnt\nNext\n";
        execute(&mut app, &event::parse(src).unwrap()).unwrap();
        assert_eq!(app.script_var("cnt"), "2");
    }

    #[test]
    fn foreach_group_form_binds_target_unit_id() {
        // Create したユニットは uid を持つ → 対象ユニットＩＤ に uid が入る。
        let mut app = App::new();
        let src = "\
Pilot \"リオ\" リオ 男性 c AAAA 100 100 100 100 100 100 100
Unit \"ブレイバー\" リアル系 1 0 陸 5 M 3000 400 3500 120 1200 110 AAAA
MapSize 5 5
Create 味方 ブレイバー 0 リオ 1 1 1
ForEach 味方
  Set uid $(対象ユニットＩＤ)
Next
";
        execute(&mut app, &event::parse(src).unwrap()).unwrap();
        assert_eq!(app.script_var("uid"), "U1");
    }

    #[test]
    fn foreach_group_form_empty_group_skips_body() {
        // 該当ユニットが居ない勢力 → 本体は実行されない (2 引数エラーにしない)。
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "Set ran 0\nForEach 友軍\n  Set ran 1\nNext\nMessage ok\n";
        execute(&mut app, &event::parse(src).unwrap()).unwrap();
        assert_eq!(app.script_var("ran"), "0");
        assert_eq!(app.messages(), &["ok".to_string()]);
    }

    #[test]
    fn foreach_group_form_status_filter() {
        // 書式1 の status (`出撃` / `待機` / `全て`) でユニットを絞り込む。
        let mut app = App::new();
        setup_two_units(&mut app); // ブレイバー(味方/出撃), ゾルダII(敵/出撃)
                                   // `Unit` 短形式で待機 (off_map) の Player ユニットを 1 体追加。
        execute(&mut app, &event::parse("Unit \"ゾルダII\" 0\n").unwrap()).unwrap();

        execute(
            &mut app,
            &event::parse("Set a 0\nForEach 味方 出撃\n  Incr a\nNext\n").unwrap(),
        )
        .unwrap();
        assert_eq!(app.script_var("a"), "1", "出撃のみ → ブレイバー");

        execute(
            &mut app,
            &event::parse("Set b 0\nForEach 味方 待機\n  Incr b\nNext\n").unwrap(),
        )
        .unwrap();
        assert_eq!(
            app.script_var("b"),
            "1",
            "待機のみ → 追加した off_map ゾルダII"
        );

        execute(
            &mut app,
            &event::parse("Set c 0\nForEach 味方 全て\n  Incr c\nNext\n").unwrap(),
        )
        .unwrap();
        assert_eq!(app.script_var("c"), "2", "全て → 出撃 + 待機");
    }

    #[test]
    fn foreach_format3_pilot_roster() {
        // 書式3 `ForEach var In パイロット一覧(mode)` — 搭乗中パイロット名を反復。
        let mut app = App::new();
        setup_two_units(&mut app); // ブレイバー/リオ, ゾルダII/ガロ
        let src = "\
Set names \"\"
ForEach p In パイロット一覧(レベル)
  Set names \"$(names) $(p)\"
Next
";
        execute(&mut app, &event::parse(src).unwrap()).unwrap();
        let names = app.script_var("names");
        assert!(names.contains("リオ"), "names={names}");
        assert!(names.contains("ガロ"), "names={names}");
    }

    #[test]
    fn skip_jumps_to_next_in_for_loop() {
        // `Skip` は対応する Next へジャンプする (ループ継続 = "continue" 相当)。
        // SRC.Sharp `SkipCmd.cs` 準拠。
        let src = r#"
Set count 0
For i = 1 To 3
  Incr count
  Skip
  Incr count 100
Next
"#;
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // 各イテレーションで count +1 → 計 3。`Incr count 100` は実行されない。
        assert_eq!(app.script_var("count"), "3");
    }

    #[test]
    fn skip_jumps_to_loop_in_do_loop() {
        // `Skip` は Do/Loop ループでも Loop まで飛ぶ。
        let src = r#"
Set n 0
Set i 0
Do
  Incr i
  If $(i) > 3 Then
    Goto done
  EndIf
  Incr n
  Skip
  Incr n 100
Loop
done:
"#;
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // i=1,2,3: n+1 each → n=3。`Incr n 100` は実行されない。
        assert_eq!(app.script_var("n"), "3");
    }

    #[test]
    fn skip_outside_loop_returns_error() {
        // ループ外で `Skip` を使うとエラー。
        let src = "Skip\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        let result = execute(&mut app, &stmts);
        assert!(result.is_err(), "Skip outside loop should be an error");
    }

    #[test]
    fn print_is_alias_of_message() {
        let mut app = App::new();
        execute(&mut app, &event::parse("Print hello\n").unwrap()).unwrap();
        assert_eq!(app.messages(), &["hello".to_string()]);
    }

    #[test]
    fn supply_recovers_hp_and_en() {
        let mut app = App::new();
        setup_two_units(&mut app);
        app.database_mut().unit_instances[0].damage = 1000;
        app.database_mut().unit_instances[0].en_consumed = 50;
        execute(&mut app, &event::parse("Supply ブレイバー\n").unwrap()).unwrap();
        assert_eq!(app.database().unit_instances[0].damage, 0);
        assert_eq!(app.database().unit_instances[0].en_consumed, 0);
    }

    #[test]
    fn fix_sets_item_slot_fixed_and_release_clears_it() {
        // SRC `Fixコマンド.md`: Fix はパイロット・アイテムをインターミッションで
        // 乗り換え・交換不可にする。HP 回復ではない。
        // Release はその固定を解除する (`Releaseコマンド.md`)。
        let mut app = App::new();
        setup_two_units(&mut app);
        // アイテムスロットに装備を追加して Fix でロック → Release で解除
        use crate::item_slot::{ItemSlot, SlotType};
        app.database_mut().unit_instances[0]
            .item_slots
            .push(ItemSlot::with_item(SlotType::Item, "魔装甲"));
        // Fix でアイテムをロック
        execute(&mut app, &event::parse("Fix 魔装甲\n").unwrap()).unwrap();
        assert!(
            app.database().unit_instances[0].item_slots[0].is_fixed,
            "Fix でアイテムスロットが固定される"
        );
        // HP は変えない (旧動作 recover_hp の廃止確認)
        app.database_mut().unit_instances[0].damage = 500;
        execute(&mut app, &event::parse("Fix リオ\n").unwrap()).unwrap();
        assert_eq!(
            app.database().unit_instances[0].damage,
            500,
            "Fix は HP を回復しない"
        );
        // Release で解除
        execute(&mut app, &event::parse("Release 魔装甲\n").unwrap()).unwrap();
        assert!(
            !app.database().unit_instances[0].item_slots[0].is_fixed,
            "Release でアイテムスロット固定が解除される"
        );
        // Release (引数なし) で全解除
        app.database_mut().unit_instances[0].item_slots[0].is_fixed = true;
        execute(&mut app, &event::parse("Release\n").unwrap()).unwrap();
        assert!(
            !app.database().unit_instances[0].item_slots[0].is_fixed,
            "Release 引数なしで全解除"
        );
    }

    #[test]
    fn unit_function_returns_unit_name_for_pilot() {
        // `Unit(pilot)` 関数はパイロット名からユニット名を返す。
        let mut app = App::new();
        setup_two_units(&mut app);
        let stmts = event::parse("Set u Unit(リオ)\n").unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("u"), "ブレイバー");
        // 敵も同様
        let stmts2 = event::parse("Set u2 Unit(ガロ)\n").unwrap();
        execute(&mut app, &stmts2).unwrap();
        assert_eq!(app.script_var("u2"), "ゾルダII");
        // 未登録パイロットは空文字
        let stmts3 = event::parse("Set u3 Unit(不明)\n").unwrap();
        execute(&mut app, &stmts3).unwrap();
        assert_eq!(app.script_var("u3"), "");
    }

    #[test]
    fn area_function_returns_terrain_class() {
        // `Area(unit)` 関数はユニットのいる地形クラスを返す。
        // 地形が設定されていない場合は空文字またはデフォルト値を返す。
        let mut app = App::new();
        setup_two_units(&mut app);
        let stmts = event::parse("Set a Area(ブレイバー)\n").unwrap();
        execute(&mut app, &stmts).unwrap();
        // マップ地形未設定なので空文字を返す (エラーにならないことを確認)
        let _ = app.script_var("a"); // crash しなければ OK
    }

    #[test]
    fn itemid_function_returns_item_name() {
        // `ItemID(unit, num)` 関数はアイテム名 (本実装では名称 = ID) を返す。
        let mut app = App::new();
        setup_two_units(&mut app);
        use crate::item_slot::{ItemSlot, SlotType};
        app.database_mut().unit_instances[0]
            .item_slots
            .push(ItemSlot::with_item(SlotType::Item, "ビームサーベル"));
        let stmts = event::parse("Set id ItemID(リオ, 1)\n").unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("id"), "ビームサーベル");
        // 存在しないインデックスは空文字
        let stmts2 = event::parse("Set id2 ItemID(リオ, 99)\n").unwrap();
        execute(&mut app, &stmts2).unwrap();
        assert_eq!(app.script_var("id2"), "");
    }

    /// テスト用に `database().items` (アイテム DB) へ名称のみのアイテムを登録する。
    /// `ItemData` は Default を持たないため全フィールドを明示する。
    fn push_item_named(app: &mut App, name: &str) {
        app.database_mut().items.push(crate::data::item::ItemData {
            name: name.to_string(),
            class: String::new(),
            part: String::new(),
            hp_mod: 0,
            en_mod: 0,
            armor_mod: 0,
            mobility_mod: 0,
            speed_mod: 0,
            comment: String::new(),
            features: Vec::new(),
        });
    }

    #[test]
    fn item_index_returns_db_position() {
        // SRC 原名: `真アイテム番号` / `アイテム番号` / `入手アイテム番号`。
        // アイテム DB 上の 1-indexed 連番を返す。
        let mut app = App::new();
        push_item_named(&mut app, "強化パーツA");
        push_item_named(&mut app, "強化パーツB");
        push_item_named(&mut app, "強化パーツC");

        // 3 系統とも同じ番号を返す。
        for fname in ["真アイテム番号", "アイテム番号", "入手アイテム番号"] {
            assert_eq!(
                eval_script_function(&app, fname, "強化パーツB").as_deref(),
                Some("2"),
                "{fname}"
            );
        }
        assert_eq!(
            eval_script_function(&app, "真アイテム番号", "強化パーツA").as_deref(),
            Some("1")
        );
        assert_eq!(
            eval_script_function(&app, "真アイテム番号", "強化パーツC").as_deref(),
            Some("3")
        );
        // 未登録名は "0"
        assert_eq!(
            eval_script_function(&app, "真アイテム番号", "存在しない").as_deref(),
            Some("0")
        );
    }

    #[test]
    fn item_index_numeric_passthrough() {
        // 引数が既に数値ならそのまま返す (SRC の numeric passthrough)。
        let mut app = App::new();
        push_item_named(&mut app, "強化パーツA");
        assert_eq!(
            eval_script_function(&app, "真アイテム番号", "5").as_deref(),
            Some("5")
        );
    }

    #[test]
    fn nickname_item_resolves_via_index_arg() {
        // スパロボ戦記 Status.eve の強化パーツ表示パターンの再現:
        //   Nickname(Item(Args(1)), 真アイテム番号(Item(Args(1))))
        // 装備中の強化パーツについて、リテラル式ではなくアイテム愛称
        // (本移植では名称) が返ることを確認する。
        let mut app = App::new();
        setup_two_units(&mut app);
        push_item_named(&mut app, "マグネットコーティング");
        use crate::item_slot::{ItemSlot, SlotType};
        app.database_mut().unit_instances[0]
            .item_slots
            .push(ItemSlot::with_item(
                SlotType::Item,
                "マグネットコーティング",
            ));

        // end-to-end: ネストした関数式が展開され愛称が得られる
        // (`Item(リオ, 1)` は 1 番目の装備アイテム名を返す)。
        let out = expand_vars(
            &app,
            "Nickname(Item(リオ, 1), 真アイテム番号(Item(リオ, 1)))",
        );
        assert_eq!(out, "マグネットコーティング");
        // リテラル式が残っていないこと。
        assert!(!out.contains("Nickname"), "literal leaked: {out}");
        assert!(!out.contains("真アイテム番号"), "literal leaked: {out}");
    }

    #[test]
    fn upgrade_modifies_unit_data() {
        let mut app = App::new();
        setup_two_units(&mut app);
        let before = app.database().unit_by_name("ブレイバー").unwrap().hp;
        execute(
            &mut app,
            &event::parse("Upgrade ブレイバー hp 500\n").unwrap(),
        )
        .unwrap();
        let after = app.database().unit_by_name("ブレイバー").unwrap().hp;
        assert_eq!(after, before + 500);
    }

    #[test]
    fn changeparty_switches_unit_party() {
        let mut app = App::new();
        setup_two_units(&mut app);
        execute(
            &mut app,
            &event::parse("ChangeParty ブレイバー Enemy\n").unwrap(),
        )
        .unwrap();
        assert_eq!(app.database().unit_instances[0].party, crate::Party::Enemy);
    }

    #[test]
    fn rankup_increments_pilot_rank() {
        let mut app = App::new();
        execute(&mut app, &event::parse("RankUp リオ\n").unwrap()).unwrap();
        execute(&mut app, &event::parse("RankUp リオ 3\n").unwrap()).unwrap();
        assert_eq!(app.script_var("__rank_リオ"), "4");
    }

    #[test]
    fn do_loop_terminates_via_exit() {
        // 単純 Do/Loop の動作確認: Set counter; Do { Incr counter; If >= 3 Exit; } Loop
        let src = "\
Set counter 0
Do
  Incr counter
  If $(counter) >= 3 Then
    Exit
  EndIf
Loop
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("counter"), "3");
    }

    #[test]
    fn turn_and_phase_functions() {
        let mut app = App::new();
        app.set_turn_number(5);
        let src = "Set t Turn()\nSet p Phase()\nSet s Stage()\n";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("t"), "5");
        assert_eq!(app.script_var("p"), "Player");
        assert_eq!(app.script_var("s"), "");
    }

    #[test]
    fn terrainid_function() {
        let src = "\
MapSize 4 4
SetTile 2 1 3
Set t TerrainId(2, 1)
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("t"), "3");
    }

    #[test]
    fn paintstring_pushes_to_overlay() {
        let src = "\
Font ゴシック 14pt #ffe0e0
PaintString 100 200 こんにちは
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let cmds = &app.script_overlay().cmds;
        assert_eq!(cmds.len(), 2);
        match &cmds[0] {
            crate::DrawCmd::SetFont {
                family,
                size_pt,
                color,
            } => {
                assert_eq!(family, "ゴシック");
                assert_eq!(*size_pt, 14);
                assert_eq!(color, "#ffe0e0");
            }
            _ => panic!("expected SetFont"),
        }
        match &cmds[1] {
            crate::DrawCmd::PaintString { x, y, text } => {
                assert_eq!(*x, 100.0);
                assert_eq!(*y, 200.0);
                assert_eq!(text, "こんにちは");
            }
            _ => panic!("expected PaintString"),
        }
    }

    #[test]
    fn paintstring_coord_expression_not_leaked_into_text() {
        // 座標が関数呼び出し / 変数を含む括弧式の場合でも、それを text に
        // 流出させない (画面に式がそのまま描画されるバグの再発防止)。
        // スパロボ戦記 String.eve VibrationString と同型。
        let src = "\
Set k 2
PaintString (k * 10 + 5) (k * 20) ヴァイブ
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let ps = app
            .script_overlay()
            .cmds
            .iter()
            .find_map(|c| match c {
                crate::DrawCmd::PaintString { x, y, text } => Some((*x, *y, text.clone())),
                _ => None,
            })
            .expect("PaintString が無い");
        // text は式ではなく "ヴァイブ" のみ
        assert_eq!(ps.2, "ヴァイブ");
        // 座標は評価される (k=2 → x=25, y=40)
        assert_eq!(ps.0, 25.0);
        assert_eq!(ps.1, 40.0);
    }

    #[test]
    fn line_command_pushes_draw_cmd() {
        let src = "Line 0 0 100 100 #ff0000\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let cmds = &app.script_overlay().cmds;
        assert_eq!(cmds.len(), 2);
        assert!(matches!(&cmds[0], crate::DrawCmd::SetColor { color } if color == "#ff0000"));
        assert!(
            matches!(&cmds[1], crate::DrawCmd::Line { x1, y1, x2, y2 } if *x1 == 0.0 && *y1 == 0.0 && *x2 == 100.0 && *y2 == 100.0)
        );
    }

    #[test]
    fn refresh_keeps_accumulated_drawing() {
        // 元 SRC `Refresh` は present のみで描画内容はクリアしない
        // (クリアは Cls / ClearPicture / ClearObj の役割)。
        // `draw / Refresh / draw` で全描画が蓄積されたまま残る。
        let src = "\
PaintString 10 10 a
PaintString 20 20 b
Refresh
PaintString 30 30 c
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let cmds = &app.script_overlay().cmds;
        // Refresh はクリアしないので a / b / c すべて残る
        assert_eq!(cmds.len(), 3);
        assert!(matches!(&cmds[2], crate::DrawCmd::PaintString { text, .. } if text == "c"));
    }

    #[test]
    fn fadeout_pushes_fade() {
        let src = "FadeOut 30\n";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let cmds = &app.script_overlay().cmds;
        assert_eq!(cmds.len(), 1);
        assert!(matches!(&cmds[0], crate::DrawCmd::Fade { alpha, .. } if *alpha > 0.0));
    }

    /// 回帰: `WhiteIn` は白フェードアウトを露出 (除去) する。引数なし `WhiteIn` が
    /// 全画面白を積みっぱなしにすると「白いマップ」で操作不能になる
    /// (東方夢想伝: タイトルテロップ末尾の `WhiteIn`)。
    #[test]
    fn whitein_clears_whiteout_overlay() {
        let stmts = event::parse("WhiteOut 255\nWhiteIn\n").unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let has_white = app
            .script_overlay()
            .cmds
            .iter()
            .any(|c| matches!(c, crate::DrawCmd::Fade { color, .. } if color == "#ffffff"));
        assert!(
            !has_white,
            "WhiteIn 後に白 Fade が残存: {:?}",
            app.script_overlay().cmds
        );
    }

    /// 回帰: `FadeIn` は黒フェードアウトを露出 (除去) する。
    #[test]
    fn fadein_clears_fadeout_overlay() {
        let stmts = event::parse("FadeOut 60\nFadeIn\n").unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        let has_black = app
            .script_overlay()
            .cmds
            .iter()
            .any(|c| matches!(c, crate::DrawCmd::Fade { color, .. } if color == "#000000"));
        assert!(
            !has_black,
            "FadeIn 後に黒 Fade が残存: {:?}",
            app.script_overlay().cmds
        );
    }

    #[test]
    fn trigger_label_runs_dormant_section() {
        // Goto で本体をスキップした後、Start ラベルを後から手動 trigger
        let src = "\
Goto skip
Start:
  Message hit_start
Exit
skip:
Message body
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["body".to_string()]);
        // ライブラリに Start ラベルがあるはず
        assert!(app.script_library().label_pc("Start").is_some());
        // 手動 trigger
        let fired = crate::event_runtime::trigger_label(&mut app, "Start");
        assert!(fired);
        assert!(app.messages().contains(&"hit_start".to_string()));
    }

    #[test]
    fn trigger_label_returns_false_if_no_label() {
        let mut app = App::new();
        execute(&mut app, &event::parse("Message x\n").unwrap()).unwrap();
        assert!(!crate::event_runtime::trigger_label(
            &mut app,
            "NoSuchLabel"
        ));
    }

    #[test]
    fn trigger_label_blocked_during_dialog() {
        let src = "\
Goto skip
Start:
  Message hit
Exit
skip:
Talk リオ
こんにちは
End
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // モーダル中
        assert!(app.pending_dialog().is_some());
        assert!(!crate::event_runtime::trigger_label(&mut app, "Start"));
    }

    #[test]
    fn function_in_if_condition() {
        let mut app = App::new();
        setup_two_units(&mut app);
        let src = "\
If Distance(ブレイバー, ゾルダII) > 3 Then
  Message far
Else
  Message close
EndIf
";
        let stmts = event::parse(src).unwrap();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages().last().map(String::as_str), Some("far"));
    }

    #[test]
    fn switch_picks_matching_case() {
        let src = "\
Set x 2
Switch $(x)
Case 1
  Message one
Case 2
  Message two
Case 3
  Message three
CaseElse
  Message other
EndSw
Message after
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["two".to_string(), "after".to_string()]);
    }

    #[test]
    fn switch_falls_through_to_caseelse() {
        let src = "\
Set x 9
Switch $(x)
Case 1
  Message one
CaseElse
  Message other
EndSw
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["other".to_string()]);
    }

    #[test]
    fn switch_with_no_match_and_no_caseelse_skips() {
        let src = "\
Set x 9
Switch $(x)
Case 1
  Message one
EndSw
Message after
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["after".to_string()]);
    }

    #[test]
    fn for_loop_iterates_count_times() {
        let src = "\
Set total 0
For i = 1 To 5
  Incr total
Next
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("total"), "5");
        // SRC.Sharp 準拠: Next は必ず変数を更新してから終了判定するため、
        // ループ終了後の i は end + step = 5 + 1 = 6。
        assert_eq!(app.script_var("i"), "6");
    }

    #[test]
    fn for_loop_with_step() {
        let src = "\
Set total 0
For i = 0 To 10 Step 2
  Incr total $(i)
Next
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        // 0+2+4+6+8+10 = 30
        assert_eq!(app.script_var("total"), "30");
    }

    #[test]
    fn for_loop_skips_when_start_exceeds_end() {
        let src = "\
Set ran 0
For i = 10 To 1
  Set ran 1
Next
Message after
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("ran"), "0");
        assert_eq!(app.messages(), &["after".to_string()]);
    }

    #[test]
    fn nested_for_loops() {
        let src = "\
Set total 0
For i = 1 To 3
  For j = 1 To 2
    Incr total
  Next
Next
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("total"), "6");
    }

    #[test]
    fn call_jumps_to_label_and_returns() {
        let src = "\
Message before
Call greeting
Message after
Exit
greeting:
  Message hello
Return
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(
            app.messages(),
            &[
                "before".to_string(),
                "hello".to_string(),
                "after".to_string()
            ]
        );
    }

    #[test]
    fn call_passes_args_via_args_function() {
        let src = "\
Call greet 太郎 さん
Exit
greet:
  Set who Args(1) Args(2)
  Message $(who)
Return
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["太郎 さん".to_string()]);
    }

    #[test]
    fn incr_command() {
        let src = "\
Set x 5
Incr x
Incr x 3
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.script_var("x"), "9");
    }

    #[test]
    fn exit_terminates_execution() {
        let src = "\
Message before
Exit
Message after
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(app.messages(), &["before".to_string()]);
    }

    #[test]
    fn no_op_commands_dont_crash() {
        let src = "\
Wait 30
StartBGM song.mid
StopBGM
PlaySound boom.wav
FadeOut 50
ChangeMap -.map
ChangeTerrain 0 0 1
Font ゴシック Regular 通常
PaintString - hello 100
Refresh
Hide
Show
KeepBGM
Option foo
Input var prompt default
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
    }

    #[test]
    fn nested_if_does_not_consume_outer_endif() {
        let src = "\
Set a 0
Set b 1
If $(a) = 1 Then
  If $(b) = 1 Then
    Message inner
  EndIf
  Message outer_then
Else
  Message outer_else
EndIf
Message after
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        execute(&mut app, &stmts).unwrap();
        assert_eq!(
            app.messages(),
            &["outer_else".to_string(), "after".to_string()]
        );
    }

    #[test]
    fn infinite_loop_aborts() {
        let src = "\
loop:
Goto loop
";
        let stmts = event::parse(src).unwrap();
        let mut app = App::new();
        let e = execute(&mut app, &stmts).unwrap_err();
        assert!(e.message.contains("上限"));
    }

    #[test]
    fn parse_i32_at_accepts_arithmetic_expression() {
        // 平文の整数はそのまま解釈する。
        assert_eq!(parse_i32_at("5", 1).unwrap(), 5);
        assert_eq!(parse_i32_at("-3", 1).unwrap(), -3);
        // `(味方レベル平均値 - 1)` 等が展開後に括弧付き算術として残るケース。
        assert_eq!(parse_i32_at("(4 - 1)", 1).unwrap(), 3);
        assert_eq!(parse_i32_at("(2 + 3 * 4)", 1).unwrap(), 14);
        // 真に不正な値は従来どおりエラー。
        assert!(parse_i32_at("abc", 1).is_err());
    }

    #[test]
    fn parse_i64_at_accepts_arithmetic_expression() {
        assert_eq!(parse_i64_at("100", 1).unwrap(), 100);
        assert_eq!(parse_i64_at("(10 - 4)", 1).unwrap(), 6);
        assert!(parse_i64_at("xyz", 1).is_err());
    }
}
