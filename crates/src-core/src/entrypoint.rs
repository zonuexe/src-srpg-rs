//! 任意の SRC アーカイブからシナリオ開始ファイルを特定するエントリーポイント検出。
//!
//! `~/repo/src-srpg/entrypoint-pattern.md` の §7 総合判定アルゴリズムを実装する。
//! 119 アーカイブのサンプリングから導いた経験則:
//!   1. README の起動指示を最優先で確定 (+100)
//!   2. 拡張子 / 命名 / 階層位置でスコアリング
//!   3. スコア降順でランキング出力

use std::collections::HashSet;

use crate::data::loader;

/// 候補ファイルの種別。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CandidateKind {
    /// SRC イベントスクリプト (.eve)
    Eve,
    /// 実行ファイル (.exe / .app / .jar)
    Exe,
    /// Web エントリーポイント (.html / .htm)
    Html,
    /// SRC セーブ / 状態ファイル (.src)
    Src,
    /// 吉里吉里など他形式 (.ks / .uix)
    Other,
}

/// エントリーポイント候補。
#[derive(Debug, Clone)]
pub struct Candidate {
    /// アーカイブ内のフルパス。
    pub name: String,
    pub kind: CandidateKind,
    /// 総合スコア。高いほどエントリーポイントらしい。
    pub score: i32,
}

/// アーカイブ全体の解析結果。
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    /// 実行可能ファイル (.eve/.src/.exe/.html/.ks/.uix) を 1 つも含まないなら true。
    pub data_only: bool,
    /// スコア降順 (同点はアーカイブ登録順) の候補一覧。
    pub candidates: Vec<Candidate>,
}

impl Analysis {
    /// 最有力のエントリーポイント名。
    pub fn best(&self) -> Option<&str> {
        self.candidates.first().map(|c| c.name.as_str())
    }
}

/// 開始系キーワード (日本語)。
const START_JP: &[&str] = &[
    "スタート",
    "スターと",
    "スター",
    "開始",
    "起動",
    "タイトル",
    "サブタイトル",
    "はじめて",
    "はじめ",
    "プロローグ",
    "序幕",
    "オープニング",
];

/// 開始系キーワード (英語、小文字比較)。
const START_EN: &[&str] = &[
    "start", "begin", "main", "index", "boot", "init", "title", "opening",
];

/// 名前に含まれると減点する終了系キーワード。
const NEG_NAME: &[&str] = &["リスタート", "終了", "gameover", "exit"];

/// README 内で起動指示を示す動詞・語句。
const LAUNCH_KEYWORDS: &[&str] = &[
    "起動",
    "実行",
    "読み込",
    "読込",
    "選んで",
    "選択",
    "始め",
    "はじめ",
    "スタート",
];

/// アーカイブ内ファイル一覧を解析しエントリーポイント候補をランク付けする。
pub fn analyze(entries: &[(String, Vec<u8>)]) -> Analysis {
    // 拡張子フィルタ (§7-2)。
    let mut raw: Vec<(&str, CandidateKind, &[u8])> = Vec::new();
    for (name, data) in entries {
        if let Some(kind) = kind_of(&ext_of(name)) {
            raw.push((name, kind, data));
        }
    }
    // 実行可能ファイルが皆無ならデータ専用アーカイブ (§5)。
    let data_only = raw.is_empty();

    // README の起動指示を抽出 (§4)。指示されたファイル名 (小文字 basename) の集合。
    let cand_basenames: Vec<String> = raw
        .iter()
        .map(|(n, _, _)| basename(n).to_lowercase())
        .collect();
    let directed = readme_directives(entries, &cand_basenames);

    let mut candidates: Vec<Candidate> = raw
        .iter()
        .map(|(name, kind, data)| {
            let base_l = basename(name).to_lowercase();
            let is_directed = directed.contains(base_l.as_str());
            let has_at = *kind == CandidateKind::Eve && eve_has_scenario_id(data);
            Candidate {
                name: (*name).to_string(),
                kind: *kind,
                score: score_of(name, *kind, is_directed, has_at),
            }
        })
        .collect();

    // スコア降順、同点は浅い階層を優先。sort_by は stable なので
    // 残る同点はアーカイブ登録順を保つ。
    candidates.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| depth_of(&a.name).cmp(&depth_of(&b.name)))
    });

    Analysis {
        data_only,
        candidates,
    }
}

/// 1 候補のスコアを算出する (§7-3)。
fn score_of(name: &str, kind: CandidateKind, directed: bool, content_has_at: bool) -> i32 {
    let np = norm_path(name);
    let depth = depth_of(name);
    let stem = stem_of(name);
    let base_l = basename(name).to_lowercase();
    let mut s = 0;

    // --- 加点 ---
    if directed {
        s += 100;
    }
    if depth == 0 {
        s += 80;
    }
    if has_start_keyword(stem) {
        s += 70;
    }
    // シナリオフォルダ＋同名 .eve 慣習 (例: `スパロボ戦記/スパロボ戦記.eve`)。
    // 薄い主シナリオが `Continue` でサブ .eve を呼ぶ配布形態に対応する。
    if kind == CandidateKind::Eve && matches_ancestor_dir(name) {
        s += 60;
    }
    if kind == CandidateKind::Exe {
        s += 50;
    }
    if kind == CandidateKind::Html && stem.to_lowercase().contains("index") {
        s += 40;
    }
    if depth == 1 {
        s += 30;
    }
    if kind == CandidateKind::Eve && content_has_at {
        s += 20;
    }

    // --- 減点 ---
    if np.contains("/lib/") || np.contains("/data/") {
        s -= 40;
    }
    if np.contains("/data/system/") {
        s -= 30;
    }
    if base_l == "include.eve" {
        s -= 20;
    }
    if kind == CandidateKind::Src {
        s -= 20;
    }
    if depth >= 3 {
        s -= 10;
    }
    if has_negative_keyword(stem) {
        s -= 10;
    }

    s
}

/// 拡張子から候補種別を判定。候補対象外なら `None`。
fn kind_of(ext: &str) -> Option<CandidateKind> {
    match ext {
        "eve" => Some(CandidateKind::Eve),
        "exe" | "app" | "jar" => Some(CandidateKind::Exe),
        "html" | "htm" => Some(CandidateKind::Html),
        "src" => Some(CandidateKind::Src),
        "ks" | "uix" => Some(CandidateKind::Other),
        _ => None,
    }
}

/// 開始系キーワードを名前 (拡張子なし) に含むか。
fn has_start_keyword(stem: &str) -> bool {
    let lower = stem.to_lowercase();
    // 誤認パターン (リスタート=セーブ再開 / ショップスタート=サブ機能) を
    // 除去してから判定する。
    let cleaned = lower
        .replace("ショップスタート", "")
        .replace("リスタート", "");
    if cleaned == "op" {
        return true;
    }
    START_JP.iter().any(|k| cleaned.contains(k)) || START_EN.iter().any(|k| cleaned.contains(k))
}

/// 終了系キーワードを名前に含むか。
fn has_negative_keyword(stem: &str) -> bool {
    let lower = stem.to_lowercase();
    NEG_NAME.iter().any(|k| lower.contains(k))
}

/// README ファイルらしい名前か。
fn is_readme(name: &str) -> bool {
    let b = basename(name).to_lowercase();
    b.contains("readme")
        || b.contains("read me")
        || b.contains("お読み")
        || b.contains("およみ")
        || b.contains("取り扱い")
        || b.contains("取扱")
        || b.contains("マニュアル")
        || b.contains("manual")
}

/// README 群を走査し、起動指示で名指しされたファイル名 (小文字 basename) を返す。
fn readme_directives(entries: &[(String, Vec<u8>)], cand_basenames: &[String]) -> HashSet<String> {
    let mut hits = HashSet::new();
    for (name, data) in entries {
        if !is_readme(name) {
            continue;
        }
        let text = loader::decode_text(data);
        for line in text.lines() {
            let lower = line.to_lowercase();
            if !LAUNCH_KEYWORDS.iter().any(|k| lower.contains(k)) {
                continue;
            }
            for b in cand_basenames {
                if !b.is_empty() && lower.contains(b.as_str()) {
                    hits.insert(b.clone());
                }
            }
        }
    }
    hits
}

/// `.eve` の先頭数行に `@シナリオ識別子` 行が含まれるか (§6 簡易判定)。
fn eve_has_scenario_id(data: &[u8]) -> bool {
    let prefix = if data.len() > 4096 {
        &data[..4096]
    } else {
        data
    };
    let text = loader::decode_text(prefix);
    text.lines()
        .take(30)
        .any(|l| l.trim_start().starts_with('@'))
}

/// パスを小文字化 + `\` を `/` に統一し、先頭に `/` を補う。
fn norm_path(name: &str) -> String {
    let mut s = name.to_lowercase().replace('\\', "/");
    if !s.starts_with('/') {
        s.insert(0, '/');
    }
    s
}

/// 末尾コンポーネント (`/` `\` 区切り) を取り出す。
fn basename(name: &str) -> &str {
    let i = name.rfind(['/', '\\']).map(|i| i + 1).unwrap_or(0);
    &name[i..]
}

/// 拡張子 (小文字、`.` なし) を返す。無ければ空文字。
fn ext_of(name: &str) -> String {
    let b = basename(name);
    match b.rfind('.') {
        Some(i) if i + 1 < b.len() => b[i + 1..].to_lowercase(),
        _ => String::new(),
    }
}

/// 拡張子を除いたファイル名を返す。
fn stem_of(name: &str) -> &str {
    let b = basename(name);
    match b.rfind('.') {
        Some(i) if i > 0 => &b[..i],
        _ => b,
    }
}

/// 階層の深さ = パス区切りの数。ルート直下は 0。
fn depth_of(name: &str) -> usize {
    name.chars().filter(|c| *c == '/' || *c == '\\').count()
}

/// ファイルのステム名が祖先ディレクトリ名のいずれかと一致するか。
fn matches_ancestor_dir(name: &str) -> bool {
    let stem = stem_of(name).to_lowercase();
    if stem.is_empty() {
        return false;
    }
    let normalized = name.replace('\\', "/");
    let mut comps: Vec<&str> = normalized.split('/').filter(|s| !s.is_empty()).collect();
    comps.pop(); // ファイル名コンポーネントを除く
    comps.iter().any(|d| d.to_lowercase() == stem)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, body: &str) -> (String, Vec<u8>) {
        (name.to_string(), body.as_bytes().to_vec())
    }

    #[test]
    fn root_start_keyword_outranks_plain_eve() {
        let entries = vec![entry("スタート.eve", ""), entry("scenario/01.eve", "")];
        let a = analyze(&entries);
        assert_eq!(a.best(), Some("スタート.eve"));
        // ルート (+80) + 開始系 (+70) = 150。
        assert_eq!(a.candidates[0].score, 150);
        // 1 階層の非開始系 = +30。
        assert_eq!(a.candidates[1].score, 30);
    }

    #[test]
    fn lib_and_data_eve_are_penalised() {
        let entries = vec![
            entry("game/Main.eve", ""),
            entry("game/Lib/Include.eve", ""),
            entry("game/Data/System/Exit.eve", ""),
        ];
        let a = analyze(&entries);
        // game/Main.eve: 1 階層 (+30) + 開始系 main (+70) = 100。
        assert_eq!(a.best(), Some("game/Main.eve"));
        let inc = a
            .candidates
            .iter()
            .find(|c| c.name.contains("Include"))
            .unwrap();
        // 2 階層 + /lib/ (-40) + include.eve (-20) = -60。
        assert_eq!(inc.score, -60);
        let exit = a
            .candidates
            .iter()
            .find(|c| c.name.contains("Exit"))
            .unwrap();
        // /data/ (-40) + /data/system/ (-30) + depth>=3 (-10) + exit 名 (-10) = -90。
        assert_eq!(exit.score, -90);
    }

    #[test]
    fn at_identifier_line_adds_bonus() {
        let entries = vec![
            entry("plain/a.eve", "Talk\nEnd\n"),
            entry("ided/b.eve", "@東方Project\nプロローグ:\n"),
        ];
        let a = analyze(&entries);
        let plain = a
            .candidates
            .iter()
            .find(|c| c.name.contains("a.eve"))
            .unwrap();
        let ided = a
            .candidates
            .iter()
            .find(|c| c.name.contains("b.eve"))
            .unwrap();
        assert_eq!(ided.score - plain.score, 20);
    }

    #[test]
    fn readme_directive_confirms_entrypoint() {
        let entries = vec![
            entry("Readme.txt", "遊ぶには「Boss.eve」を読み込んでください。\n"),
            entry("Boss.eve", ""),
            entry("Start.eve", ""),
        ];
        let a = analyze(&entries);
        // Boss.eve: ルート (+80) + README 指示 (+100) = 180。
        // Start.eve: ルート (+80) + 開始系 (+70) = 150。
        assert_eq!(a.best(), Some("Boss.eve"));
        assert_eq!(a.candidates[0].score, 180);
    }

    #[test]
    fn eve_matching_scenario_folder_name_is_boosted() {
        let entries = vec![
            entry(
                "スパロボ戦記/スパロボ戦記.eve",
                "@スパロボ戦記\nプロローグ:\n",
            ),
            entry("スパロボ戦記/eve/Main.eve", "*プロローグ:\n"),
        ];
        let a = analyze(&entries);
        // フォルダ同名 .eve が main キーワード持ちのサブ .eve を上回る。
        assert_eq!(a.best(), Some("スパロボ戦記/スパロボ戦記.eve"));
        // 1 階層 (+30) + フォルダ同名 (+60) + @識別子 (+20) = 110。
        assert_eq!(a.candidates[0].score, 110);
        // eve/Main.eve: 2 階層 + 開始系 main (+70) = 70。同名加点は付かない。
        let main = a
            .candidates
            .iter()
            .find(|c| c.name.contains("Main"))
            .unwrap();
        assert_eq!(main.score, 70);
    }

    #[test]
    fn restart_is_not_a_start_keyword() {
        let entries = vec![
            entry("リスタート.eve", ""),
            entry("ショップスタート.eve", ""),
        ];
        let a = analyze(&entries);
        let restart = a
            .candidates
            .iter()
            .find(|c| c.name.contains("リスタート"))
            .unwrap();
        // ルート (+80) のみ。開始系 +70 は付かず、終了系 -10。
        assert_eq!(restart.score, 70);
        let shop = a
            .candidates
            .iter()
            .find(|c| c.name.contains("ショップ"))
            .unwrap();
        // ルート (+80) のみ。開始系は付かない。
        assert_eq!(shop.score, 80);
    }

    #[test]
    fn exe_at_root_scores_high() {
        let entries = vec![entry("Game.exe", ""), entry("scenario.eve", "")];
        let a = analyze(&entries);
        // Game.exe: ルート (+80) + .exe (+50) = 130。
        assert_eq!(a.best(), Some("Game.exe"));
        assert_eq!(a.candidates[0].score, 130);
    }

    #[test]
    fn data_only_archive_is_detected() {
        let entries = vec![entry("unit/zolda.bmp", ""), entry("bgm/op.mid", "")];
        let a = analyze(&entries);
        assert!(a.data_only);
        assert!(a.candidates.is_empty());
        assert_eq!(a.best(), None);
    }

    #[test]
    fn src_savefile_is_penalised() {
        let entries = vec![entry("save.src", "")];
        let a = analyze(&entries);
        assert!(!a.data_only);
        // ルート (+80) + .src (-20) = 60。
        assert_eq!(a.candidates[0].score, 60);
    }
}
