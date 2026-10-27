//! ロード済み `.eve` 全体を保持するスクリプトライブラリ (ラベル索引・ファイル境界・
//! `*ユニットコマンド` 等のカスタムコマンド定義)。

use super::*;

/// 全ロード済み `.eve` を集約したスクリプトライブラリ。
/// `App.script_library` として保持し、`Start:` / `Turn N:` 等の
/// 自動発火ラベルを引いて `trigger_label` で実行する。
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ScriptLibrary {
    pub statements: Vec<EventStatement>,
    pub labels: HashMap<String, usize>,
    /// `labels` の ASCII 小文字化インデックス。SRC のラベルは大小を区別しない
    /// ため大小無視の検索が要るが、`labels` の線形走査で行うとユーザ定義関数の
    /// 判定 (式中の各位置で照会する) が破綻する。O(1) で引けるよう別に持つ。
    #[serde(default)]
    pub(super) labels_lower: HashMap<String, usize>,
    /// 各 .eve ファイルの登録範囲 (PC 区間)。`Continue eve\onsen.eve` の
    /// ようなファイル名指定の next-stage 起動で、basename → start_pc を
    /// 引くのに使う。
    #[serde(default)]
    pub files: Vec<FileEntry>,
    /// `*ユニットコマンド` / `*マップコマンド` ラベルで定義されたシナリオ独自の
    /// コマンドメニュー項目。
    #[serde(default)]
    pub custom_commands: Vec<CustomCommandDef>,
    /// `ClearEvent` で無効化されたラベル行の pc 集合 (SRC `LabelData.Enable=false`
    /// 相当)。`label_pc` / `label_pc_in_file` はこの集合の pc をスキップするので、
    /// 一度きりイベント (`*攻撃 ＮＰＣ 敵` 等) が再発火しなくなる。`Restore` で解除。
    #[serde(default)]
    pub disabled_pcs: std::collections::HashSet<usize>,
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
pub(super) fn parse_custom_command(
    name: &str,
    args: &[String],
    pc: usize,
) -> Option<CustomCommandDef> {
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
                    self.labels_lower
                        .entry(canon.to_ascii_lowercase())
                        .or_insert(base + i);
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
        self.labels_get_ci(name)
    }

    /// `labels` を **大小無視** で引く。exact 一致を優先し、見つからなければ ASCII
    /// 大文字小文字を無視して走査する。SRC のイベントラベルは大小を区別しない
    /// (例: シナリオの `Mindanime` 命令が Lib の `MindAnime:` サブルーチンを呼ぶ)。
    /// exact 優先なので通常の解決は従来どおり O(1)、フォールバック走査はミス時のみ。
    pub(super) fn labels_get_ci(&self, name: &str) -> Option<usize> {
        if let Some(pc) = self.labels.get(name) {
            if !self.disabled_pcs.contains(pc) {
                return Some(*pc);
            }
        }
        if let Some(pc) = self.labels_lower.get(&name.to_ascii_lowercase()) {
            if !self.disabled_pcs.contains(pc) {
                return Some(*pc);
            }
        }
        // `labels_lower` を持たない旧セーブデータ由来のライブラリのみ、
        // 従来どおり線形走査でフォールバックする。
        if self.labels_lower.is_empty() && !self.labels.is_empty() {
            return self
                .labels
                .iter()
                .filter(|(_, v)| !self.disabled_pcs.contains(v))
                .find(|(k, _)| k.eq_ignore_ascii_case(name))
                .map(|(_, v)| *v);
        }
        None
    }

    /// `ClearEvent` 用: 指定 pc のラベル行を無効化する (再発火・解決をスキップ)。
    pub fn disable_pc(&mut self, pc: usize) {
        self.disabled_pcs.insert(pc);
    }

    /// `Restore` 用: [`Self::disable_pc`] を解除する。
    pub fn enable_pc(&mut self, pc: usize) {
        self.disabled_pcs.remove(&pc);
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
            if self.disabled_pcs.contains(&i) {
                continue; // ClearEvent で無効化済みのラベル行はスキップ
            }
            if let EventStatement::Command { name, args, .. } = &self.statements[i] {
                if canonical_label_full(name, args)
                    .as_deref()
                    .is_some_and(|c| c.eq_ignore_ascii_case(label))
                {
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
                    if canonical_label_full(name, args)
                        .as_deref()
                        .is_some_and(|c| c.eq_ignore_ascii_case(label))
                    {
                        return Some(i);
                    }
                }
            }
        }
        self.labels_get_ci(label)
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
                    if canonical_label_full(name, args)
                        .as_deref()
                        .is_some_and(|c| c.eq_ignore_ascii_case(label))
                    {
                        return Some(i);
                    }
                }
            }
            // ファイル登録あり: 当該ファイル内に無ければ None (誤飛び防止)。
            return None;
        }
        // ファイル未登録経路: 旧挙動どおり global で解決。
        self.labels_get_ci(label)
    }
}

/// `Eve\onsen.eve` / `lib/CMaking.eve` 等から、`/` `\\` の末尾以降を
/// lowercase で取り出したものを返す。
pub(super) fn library_basename(path: &str) -> String {
    let slash = path.rfind('/').map(|i| i + 1).unwrap_or(0);
    let bslash = path.rfind('\\').map(|i| i + 1).unwrap_or(0);
    path[slash.max(bslash)..].to_ascii_lowercase()
}

/// `name:` / `@name` / `*name:` を正規化したラベルキーを返す。
/// SRC では `*label:` (アスタリスク接頭辞) はイベントハンドラ用の特殊
/// ラベルだが、本実装ではラベル名の一部とは見做さず通常ラベル相当に
/// 正規化する。これにより `*スタート:` が `trigger_label("スタート")`
/// で発火可能になる。
pub(super) fn canonical_label(token: &str) -> Option<String> {
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
/// ラベルと誤認しない (VB5 原典では `Goto target:` の表記は許容され、
/// target 側の `:` は jump 時に剥がす)。
pub(super) fn canonical_label_full(name: &str, args: &[String]) -> Option<String> {
    // 1) 単一トークンの label / anchor (`プロローグ:` / `@onsen` / `*スタート:`)
    if args.is_empty() {
        return canonical_label(name);
    }
    // 2) `@xxx` anchor は単一トークンの Goto/Call ターゲット専用。
    if name.starts_with('@') {
        return canonical_label(name);
    }
    // 2b) `*` 接頭辞付き自動発火ラベル (`*ターン 全 味方:` / `*攻撃 ＮＰＣ 敵:`) は
    //     `*` を剥がして multi-token として扱う。SRC は `*` をキーに保持しつつ
    //     SearchLabel が token 単位で吸収して照合するが、本実装は exact-key 方式
    //     なので発火側 (`*` 無しの HandleEvent 候補) と一致するよう登録時に `*` を除去
    //     する。`*ユニットコマンド`/`*マップコマンド` は parse_custom_command が先に処理済み。
    //     (旧実装は `*` 付き multi-token を canonical_label で単一トークンへ潰し、
    //      `*攻撃 ＮＰＣ 敵:` を "攻撃" として誤登録 → NPC 攻撃イベントが発火しなかった。)
    let name = name.strip_prefix('*').unwrap_or(name);
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
    //    auto-fire は VB5 原典に存在しないため除外)。
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
