//! `animation.txt` / `ext_animation.txt`（戦闘アニメデータ）のパーサと解決ロジック。
//!
//! 原典 SRC は汎用戦闘アニメ（`Lib/汎用戦闘アニメ/GBA_*.eve`）を使い、ユニットが使った
//! 武器に応じて戦闘アニメを自動選択する。`animation.txt` は「どの武器(状況)でどの
//! 表示用サブルーチンを呼ぶか」を明示指定するデータ。
//!
//! 仕様: `SRC.Sharp/SRC.Sharp.Help/src/戦闘アニメデータ.md`。
//! パーサ挙動は `SRC.NET/MessageDataList.Load`（`is_effect=true`）/ `GeneralLib.GetLine`
//! に準拠する:
//! - 空行区切りのレコード。1 行目 = データ名称（対象）、以降 = `状況, 特殊効果指定`。
//! - 行頭 `#` はコメント行として**まるごと無視**（レコード境界にならない）。
//! - `//` 以降は行コメント。行末 `_` は次行へ継続（`_` は除去して連結）。
//! - 全角コンマ `，` は `, ` に正規化。
//! - 同一対象が複数回定義された場合、戦闘アニメデータは**追記**（上書きしない）。
//!
//! 本モジュールは「データの読み込みと、武器(状況)→表示用サブルーチン名の解決」までを
//! 担う。解決されたサブルーチン (`戦闘アニメ_*`) を実際に実行して描画する配線は別途
//! （`event_runtime` の script_overlay 経路）で行う。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 1 件の `状況, 特殊効果指定` 行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimEntry {
    /// シチュエーション。武器系は `武器名(攻撃)` / `武器名`(一括) 等、非戦闘系は
    /// `回避` / `破壊` / `変形(...)` 等。
    pub situation: String,
    /// 特殊効果指定。`サブルーチン名 [引数];サブルーチン名 [引数]` の生文字列。
    pub effect: String,
}

/// `animation.txt` 全体。対象名 → 定義行リスト。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AnimationData {
    /// 対象名（ユニット名/愛称(正規化)/クラス/「汎用」）→ 定義行（順序保持・追記）。
    pub entries: BTreeMap<String, Vec<AnimEntry>>,
}

/// 武器使用時の戦闘段階。括弧無し（一括）指定で展開するサフィックスに対応。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WeaponPhase {
    /// 準備（攻撃メッセージ直前）。
    Prep,
    /// 攻撃（攻撃メッセージ直後）。
    Attack,
    /// 命中。
    Hit,
}

impl WeaponPhase {
    /// 括弧無し（一括）指定でサブルーチン名末尾に付くサフィックス。
    fn suffix(self) -> &'static str {
        match self {
            WeaponPhase::Prep => "準備",
            WeaponPhase::Attack => "攻撃",
            WeaponPhase::Hit => "命中",
        }
    }
}

/// 解決された 1 つの表示用サブルーチン呼び出し。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAnim {
    /// 実際に呼び出すサブルーチン名（`戦闘アニメ_*` または `@` 指定の生名）。
    pub subroutine: String,
    /// サブルーチンへ渡す引数（空可、`Args(1..)` 相当の式文字列）。
    pub params: String,
}

impl AnimationData {
    /// `animation.txt` / `ext_animation.txt` のテキストをパースして追記する。
    /// 既存エントリは保持し、同一対象には追加する（SRC `is_effect` 挙動）。
    pub fn merge_from_str(&mut self, src: &str) {
        let lines = prepare_lines(src);
        let mut i = 0;
        while i < lines.len() {
            // 空行スキップ。
            while i < lines.len() && lines[i].is_empty() {
                i += 1;
            }
            if i >= lines.len() {
                break;
            }
            let data_name = lines[i].clone();
            i += 1;
            // データ名称にコンマが含まれる = 名称欠落の不正レコード。状況行を読み飛ばす。
            if data_name.contains(',') {
                while i < lines.len() && !lines[i].is_empty() {
                    i += 1;
                }
                continue;
            }
            let bucket = self.entries.entry(data_name).or_default();
            while i < lines.len() && !lines[i].is_empty() {
                let line = &lines[i];
                if let Some(c) = line.find(',') {
                    let situation = line[..c].trim().to_string();
                    let effect = line[c + 1..].trim().to_string();
                    if !situation.is_empty() {
                        bucket.push(AnimEntry { situation, effect });
                    }
                }
                i += 1;
            }
        }
    }

    /// テキストから新規パース。
    pub fn parse(src: &str) -> Self {
        let mut d = AnimationData::default();
        d.merge_from_str(src);
        d
    }

    /// 空か。
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// 武器使用時の戦闘アニメを解決する。
    ///
    /// `unit_name` / `nickname` / `class` から優先度順（名称 > 愛称(正規化) > クラス
    /// (正規化) > 「汎用」）に対象を探し、`武器名(<状況>)` を優先、無ければ括弧無し
    /// `武器名`(一括) を試す。`situation_label` は括弧内容（"攻撃"/"命中"/"準備"/
    /// "反撃"/"とどめ" 等）。`phase` は一括指定時に付けるサフィックス。
    ///
    /// 戻り値は `;` 区切りで複数指定された表示用サブルーチンの全件。
    pub fn resolve_weapon(
        &self,
        unit_name: &str,
        nickname: &str,
        class: &str,
        weapon: &str,
        situation_label: &str,
        phase: WeaponPhase,
    ) -> Vec<ResolvedAnim> {
        let paren_key = format!("{weapon}({situation_label})");
        for target in self.target_priority(unit_name, nickname, class) {
            let Some(bucket) = self.entries.get(&target) else {
                continue;
            };
            // 1) 括弧付き `武器名(状況)` を優先。
            if let Some(e) = bucket.iter().find(|e| e.situation == paren_key) {
                return parse_effect(&e.effect, |sub| {
                    build_weapon_sub(sub, Some(situation_label), phase)
                });
            }
            // 2) 括弧無し `武器名`（一括: 準備/攻撃/命中）。
            if let Some(e) = bucket.iter().find(|e| e.situation == weapon) {
                return parse_effect(&e.effect, |sub| build_weapon_sub(sub, None, phase));
            }
        }
        Vec::new()
    }

    /// 非戦闘系シチュエーション（`回避` / `破壊` / `変形(...)` 等）を解決する。
    /// サブルーチン名は `戦闘アニメ_<sub>発動`。
    pub fn resolve_situation(
        &self,
        unit_name: &str,
        nickname: &str,
        class: &str,
        situation: &str,
    ) -> Vec<ResolvedAnim> {
        for target in self.target_priority(unit_name, nickname, class) {
            let Some(bucket) = self.entries.get(&target) else {
                continue;
            };
            if let Some(e) = bucket.iter().find(|e| e.situation == situation) {
                return parse_effect(&e.effect, |sub| format!("戦闘アニメ_{sub}発動"));
            }
        }
        Vec::new()
    }

    /// 対象名の探索優先度リスト（高い順、重複/空は除去）。
    fn target_priority(&self, unit_name: &str, nickname: &str, class: &str) -> Vec<String> {
        let mut out = Vec::new();
        let mut push = |s: String| {
            if !s.is_empty() && !out.contains(&s) {
                out.push(s);
            }
        };
        push(unit_name.to_string());
        push(normalize_nickname(nickname));
        push(normalize_class(class));
        push("汎用".to_string());
        out
    }
}

/// 物理行 → 論理行（コメント除去・`_` 継続連結・全角コンマ正規化）。
/// 空行は区切りとして残す。`#` 行頭コメントは完全に除去する。
fn prepare_lines(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut acc = String::new();
    let mut continuing = false;
    for raw in src.split('\n') {
        let raw = raw.strip_suffix('\r').unwrap_or(raw);
        // 行頭 `#` はコメント行 → まるごとスキップ（継続中でも行は無視）。
        if raw.trim_start().starts_with('#') {
            continue;
        }
        // `//` 以降を除去（SRC GetLine は単純な最初の `//`）。
        let mut buf = match raw.find("//") {
            Some(p) => raw[..p].to_string(),
            None => raw.to_string(),
        };
        // 末尾 `_` なら継続。末尾空白を除いてから判定/除去する。
        let trimmed_end = buf.trim_end();
        if let Some(head) = trimmed_end.strip_suffix('_') {
            acc.push_str(head);
            continuing = true;
            continue;
        }
        buf = trimmed_end.to_string();
        acc.push_str(&buf);
        out.push(acc.replace('，', ", ").trim().to_string());
        acc.clear();
        continuing = false;
    }
    if continuing || !acc.is_empty() {
        out.push(acc.replace('，', ", ").trim().to_string());
    }
    out
}

/// 特殊効果指定（`sub [args];sub [args]`）を `;` 分割し、各 `sub` に名前変換を
/// 適用して [`ResolvedAnim`] のリストにする。`@` 接頭辞は変換無効化（生名・`@`除去）。
fn parse_effect(effect: &str, name_of: impl Fn(&str) -> String) -> Vec<ResolvedAnim> {
    let mut out = Vec::new();
    for seg in effect.split(';') {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        // sub と params を最初の空白で分割。
        let (sub, params) = match seg.split_once(char::is_whitespace) {
            Some((s, p)) => (s.trim(), p.trim()),
            None => (seg, ""),
        };
        let subroutine = if let Some(literal) = sub.strip_prefix('@') {
            literal.to_string()
        } else {
            name_of(sub)
        };
        out.push(ResolvedAnim {
            subroutine,
            params: params.to_string(),
        });
    }
    out
}

/// 武器シチュエーションの表示用サブルーチン名を構築する。
/// - 括弧付き: `戦闘アニメ_<sub><括弧内容>`
/// - 括弧無し（一括）: `戦闘アニメ_<sub><段階サフィックス>`
fn build_weapon_sub(sub: &str, paren_label: Option<&str>, phase: WeaponPhase) -> String {
    match paren_label {
        Some(label) => format!("戦闘アニメ_{sub}{label}"),
        None => format!("戦闘アニメ_{sub}{}", phase.suffix()),
    }
}

/// ユニット愛称を戦闘アニメデータ検索用に正規化する（仕様: 戦闘アニメデータ.md）。
/// - 先頭の「～用」（最初の `用` まで）を削除
/// - 先頭の「～型」（最初の `型` まで）を削除
/// - 末尾の `(～)` / `（～）` を削除
/// - 末尾の `改` / `カスタム` を削除
pub fn normalize_nickname(nickname: &str) -> String {
    let mut s = nickname.trim().to_string();
    // 先頭「～用」: 最初の '用' まで（含む）を削除。
    if let Some(pos) = s.find('用') {
        s = s[pos + '用'.len_utf8()..].to_string();
    }
    // 先頭「～型」: 最初の '型' まで（含む）を削除。
    if let Some(pos) = s.find('型') {
        s = s[pos + '型'.len_utf8()..].to_string();
    }
    // 末尾の括弧 `(...)` / `（...）` を削除。
    s = strip_trailing_parens(&s);
    // 末尾の「カスタム」「改」を削除。
    for suffix in ["カスタム", "改"] {
        if let Some(t) = s.strip_suffix(suffix) {
            s = t.to_string();
        }
    }
    s.trim().to_string()
}

/// ユニットクラスを正規化する。専用指定・人間ユニットの括弧を削除する。
/// 例: `(剣士(ロイ専用))` → `剣士`。
pub fn normalize_class(class: &str) -> String {
    let mut s = class.trim().to_string();
    // 全体が 1 重の括弧で囲まれていれば 1 層外す。
    s = unwrap_one_paren_layer(&s);
    // 残った `(...専用)` / `（...専用）` グループを削除。
    s = remove_senyou_groups(&s);
    unwrap_one_paren_layer(&s).trim().to_string()
}

/// 末尾の `(...)` / `（...）` を 1 つ削除する。
fn strip_trailing_parens(s: &str) -> String {
    let t = s.trim_end();
    if let Some(stripped) = t.strip_suffix(')') {
        if let Some(open) = stripped.rfind('(') {
            return stripped[..open].to_string();
        }
    }
    if let Some(stripped) = t.strip_suffix('）') {
        if let Some(open) = stripped.rfind('（') {
            return stripped[..open].to_string();
        }
    }
    t.to_string()
}

/// 文字列全体が 1 組の括弧で囲まれていれば 1 層外す。
fn unwrap_one_paren_layer(s: &str) -> String {
    let t = s.trim();
    if let Some(inner) = t.strip_prefix('(').and_then(|x| x.strip_suffix(')')) {
        return inner.to_string();
    }
    if let Some(inner) = t.strip_prefix('（').and_then(|x| x.strip_suffix('）')) {
        return inner.to_string();
    }
    t.to_string()
}

/// `専用` を含む括弧グループ（半角/全角）を削除する。
fn remove_senyou_groups(s: &str) -> String {
    let mut result = s.to_string();
    for (open, close) in [('(', ')'), ('（', '）')] {
        while let Some(o) = result.find(open) {
            let Some(rel) = result[o..].find(close) else {
                break;
            };
            // 閉じ括弧文字の末尾バイト位置（マルチバイト全角 `）` も char 境界で扱う）。
            let close_end = o + rel + close.len_utf8();
            if !result[o..close_end].contains("専用") {
                break;
            }
            let mut next = result[..o].to_string();
            next.push_str(&result[close_end..]);
            result = next;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_records_situations_and_accumulates() {
        let src = "\
主人公機
ブラックホールランチャー(準備), 粒子集中 黒
ブラックホールランチャー(攻撃), 画像前面発射 \"Common\\GravityBall01.bmp\" - - 3
ブラックホールランチャー(命中), 重力圧縮;飛沫 黒

汎用
ビームライフル, ビーム
";
        let d = AnimationData::parse(src);
        assert_eq!(d.entries.len(), 2);
        let hero = &d.entries["主人公機"];
        assert_eq!(hero.len(), 3);
        assert_eq!(hero[0].situation, "ブラックホールランチャー(準備)");
        assert_eq!(hero[0].effect, "粒子集中 黒");
        assert_eq!(d.entries["汎用"][0].situation, "ビームライフル");
    }

    #[test]
    fn skips_hash_comments_and_strips_slash_and_joins_continuation() {
        let src = "\
# これはコメント
主人公機
ビームライフル(攻撃), 発射 // 末尾コメント
ビームサーベル(命中), 斬撃 _
赤
";
        let d = AnimationData::parse(src);
        let hero = &d.entries["主人公機"];
        assert_eq!(hero.len(), 2);
        assert_eq!(hero[0].effect, "発射");
        // 継続行: "斬撃 _" + "赤" → "斬撃 赤"
        assert_eq!(hero[1].situation, "ビームサーベル(命中)");
        assert_eq!(hero[1].effect, "斬撃 赤");
    }

    #[test]
    fn duplicate_target_accumulates_not_overwrites() {
        let src = "\
汎用
A(攻撃), x

汎用
B(攻撃), y
";
        let d = AnimationData::parse(src);
        assert_eq!(d.entries["汎用"].len(), 2);
    }

    #[test]
    fn fullwidth_comma_normalized() {
        // 全角コンマでも状況/効果が分割される。
        let d = AnimationData::parse("汎用\nビーム(攻撃)， 発射\n");
        assert_eq!(d.entries["汎用"][0].situation, "ビーム(攻撃)");
        assert_eq!(d.entries["汎用"][0].effect, "発射");
    }

    #[test]
    fn resolve_weapon_paren_form() {
        let d = AnimationData::parse("汎用\n光の剣(命中), 斬撃\n");
        let r = d.resolve_weapon("X", "", "", "光の剣", "命中", WeaponPhase::Hit);
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].subroutine, "戦闘アニメ_斬撃命中");
        assert_eq!(r[0].params, "");
    }

    #[test]
    fn resolve_weapon_bare_form_uses_phase_suffix() {
        let d = AnimationData::parse("汎用\n氷の槍, 槍\n");
        let prep = d.resolve_weapon("X", "", "", "氷の槍", "準備", WeaponPhase::Prep);
        let atk = d.resolve_weapon("X", "", "", "氷の槍", "攻撃", WeaponPhase::Attack);
        let hit = d.resolve_weapon("X", "", "", "氷の槍", "命中", WeaponPhase::Hit);
        assert_eq!(prep[0].subroutine, "戦闘アニメ_槍準備");
        assert_eq!(atk[0].subroutine, "戦闘アニメ_槍攻撃");
        assert_eq!(hit[0].subroutine, "戦闘アニメ_槍命中");
    }

    #[test]
    fn resolve_multiple_subs_and_params() {
        let d = AnimationData::parse("汎用\nW(命中), 重力圧縮;飛沫 黒\n");
        let r = d.resolve_weapon("X", "", "", "W", "命中", WeaponPhase::Hit);
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].subroutine, "戦闘アニメ_重力圧縮命中");
        assert_eq!(r[0].params, "");
        assert_eq!(r[1].subroutine, "戦闘アニメ_飛沫命中");
        assert_eq!(r[1].params, "黒");
    }

    #[test]
    fn resolve_at_prefix_is_literal() {
        let d = AnimationData::parse("汎用\nW(クリティカル), @戦闘アニメ_睡眠クリティカル\n");
        let r = d.resolve_weapon("X", "", "", "W", "クリティカル", WeaponPhase::Hit);
        assert_eq!(r[0].subroutine, "戦闘アニメ_睡眠クリティカル");
    }

    #[test]
    fn resolve_priority_name_over_generic() {
        let d = AnimationData::parse("汎用\nW(攻撃), g\n\nブレイバー\nW(攻撃), u\n");
        // ユニット名が最優先。
        let r = d.resolve_weapon("ブレイバー", "", "", "W", "攻撃", WeaponPhase::Attack);
        assert_eq!(r[0].subroutine, "戦闘アニメ_u攻撃");
    }

    #[test]
    fn resolve_non_weapon_situation() {
        let d = AnimationData::parse("汎用\n回避, 残像\n");
        let r = d.resolve_situation("X", "", "", "回避");
        assert_eq!(r[0].subroutine, "戦闘アニメ_残像発動");
    }

    #[test]
    fn nickname_normalization_matches_doc_example() {
        // アリス専用高機動型シルフィード改(Ａモード) => シルフィード
        assert_eq!(
            normalize_nickname("アリス専用高機動型シルフィード改(Ａモード)"),
            "シルフィード"
        );
    }

    #[test]
    fn class_normalization_strips_senyou_parens() {
        // (剣士(ロイ専用)) => 剣士
        assert_eq!(normalize_class("(剣士(ロイ専用))"), "剣士");
    }

    #[test]
    fn resolve_via_normalized_nickname() {
        let d = AnimationData::parse("シルフィード\nW(攻撃), s\n");
        let r = d.resolve_weapon(
            "別名ユニット",
            "アリス専用高機動型シルフィード改(Ａモード)",
            "",
            "W",
            "攻撃",
            WeaponPhase::Attack,
        );
        assert_eq!(r[0].subroutine, "戦闘アニメ_s攻撃");
    }
}
