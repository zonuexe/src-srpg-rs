//! `pilot_message.txt` (メッセージデータ) のパーサと、シチュエーションに合うメッセージの
//! 選択。原典 `MessageDataList.Load` / `MessageData.SelectMessage`。
//!
//! 仕様: `SRC.Sharp/SRC.Sharp.Help/src/メッセージデータ.md`。
//! - 空行区切りのレコード。1 行目 = パイロット名 (またはメッセージタイプ)、以降 =
//!   `シチュエーション, メッセージ`。行の読み方 (コメント・`_` 継続) は `animation.txt` と同じ。
//! - 同じ名前が 2 回定義されたら後の定義で置き換える (戦闘アニメデータは追記)。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// 1 件の `シチュエーション, メッセージ` 行。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageEntry {
    pub situation: String,
    pub message: String,
}

/// `pilot_message.txt` 全体。パイロット名 (メッセージタイプ) → メッセージ行 (定義順)。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageData {
    pub entries: BTreeMap<String, Vec<MessageEntry>>,
}

impl MessageData {
    /// テキストをパースして取り込む。同じ名前のデータは後の定義で置き換える。
    pub fn merge_from_str(&mut self, src: &str) {
        let lines = super::animation::prepare_lines(src);
        let mut i = 0;
        while i < lines.len() {
            while i < lines.len() && lines[i].is_empty() {
                i += 1;
            }
            if i >= lines.len() {
                break;
            }
            let name = lines[i].clone();
            i += 1;
            let mut list = Vec::new();
            while i < lines.len() && !lines[i].is_empty() {
                if let Some((situation, message)) = lines[i].split_once(',') {
                    if !situation.is_empty() {
                        list.push(MessageEntry {
                            situation: situation.to_string(),
                            message: message.trim().to_string(),
                        });
                    }
                }
                i += 1;
            }
            // 名前にコンマがある = 名前の行が抜けた不正レコード。
            if !name.contains(',') {
                self.entries.insert(name, list);
            }
        }
    }

    /// テキストから新規パース。
    pub fn parse(src: &str) -> Self {
        let mut d = Self::default();
        d.merge_from_str(src);
        d
    }

    /// `name` のメッセージ行。
    pub fn get(&self, name: &str) -> Option<&[MessageEntry]> {
        self.entries.get(name).map(Vec::as_slice)
    }
}

/// [`select_message`] に渡す話し手と相手の情報。
#[derive(Debug, Clone, Default)]
pub struct SelectContext {
    /// 相手限定の指定 `(対…)` の候補。相手がいなければ `None`。
    pub target_subs: Option<Vec<String>>,
    /// 相手限定のメッセージがあれば必ずそれを使う ((とどめ)・挑発等・味方が相手)。
    pub force_target: bool,
    /// ユニット限定の指定 `(ユニット名称)` 等の候補。
    pub unit_subs: Vec<String>,
    /// 攻撃側が使う武器の愛称。`格闘` / `射撃` (と `(反撃)`) のときだけ `(武器名)` 指定に使う。
    pub weapon_sub: Option<String>,
}

/// 原典 `MessageData.SelectMessage`。`entries` から `msg_situation` に合うメッセージを
/// 1 つ選ぶ。`dice(n)` は 1..=n の乱数。
pub fn select_message(
    entries: &[MessageEntry],
    msg_situation: &str,
    ctx: &SelectContext,
    dice: &mut dyn FnMut(usize) -> usize,
) -> Option<String> {
    // `格闘` / `射撃` (サブシチュエーション付きを含む) は `攻撃` でも探す。
    let mut situations = vec![msg_situation.to_string()];
    for head in ["格闘", "射撃"] {
        if let Some(rest) = msg_situation.strip_prefix(head) {
            const RESTS: [&str; 10] = [
                "",
                "(命中)",
                "(回避)",
                "(とどめ)",
                "(クリティカル)",
                "(反撃)",
                "(命中)(反撃)",
                "(回避)(反撃)",
                "(とどめ)(反撃)",
                "(クリティカル)(反撃)",
            ];
            if RESTS.contains(&rest) {
                situations.push(format!("攻撃{rest}"));
            }
        }
    }

    // 第一次審査: シチュエーションで始まるもの。
    let list0: Vec<&MessageEntry> = entries
        .iter()
        .filter(|e| {
            situations
                .iter()
                .any(|s| e.situation.starts_with(s.as_str()))
        })
        .collect();
    if list0.is_empty() {
        return None;
    }
    fn pick(list: &[&MessageEntry], dice: &mut dyn FnMut(usize) -> usize) -> String {
        list[dice(list.len()) - 1].message.clone()
    }

    // 相手限定のメッセージ。
    let mut selected = None;
    if let Some(subs) = &ctx.target_subs {
        let list: Vec<&MessageEntry> = list0
            .iter()
            .copied()
            .filter(|e| e.situation.contains("(対"))
            .filter(|e| {
                situations
                    .iter()
                    .any(|s| subs.iter().any(|sub| e.situation == format!("{s}{sub}")))
            })
            .collect();
        if !list.is_empty() {
            selected = Some(pick(&list, dice));
            if dice(2) == 1 || ctx.force_target {
                return selected;
            }
        }
    }

    // サブシチュエーションなしと、ユニット限定のサブシチュエーション。
    let mut unit_subs = ctx.unit_subs.clone();
    if let Some(w) = &ctx.weapon_sub {
        if ["格闘", "射撃", "格闘(反撃)", "射撃(反撃)"].contains(&msg_situation) {
            unit_subs.push(format!("({w})"));
        }
    }
    let list: Vec<&MessageEntry> = list0
        .iter()
        .copied()
        .filter(|e| {
            situations.iter().any(|s| {
                e.situation == *s
                    || unit_subs
                        .iter()
                        .any(|sub| e.situation == format!("{s}{sub}"))
            })
        })
        .collect();
    if !list.is_empty() {
        selected = Some(pick(&list, dice));
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    const SRC: &str = "\
# コメント
ジェイ
回避, おっと！！
攻撃, 落ちろ！
射撃(ビームライフル), 狙い撃つ！
攻撃(対ロイ), ロイ！ 覚悟ッ！！
回避(龍機神), 龍機神をなめるなよ！
破壊, うおおお_
っ！！

ジェイ
回避, 上書き
";

    fn first(_: usize) -> usize {
        1
    }

    #[test]
    fn later_record_replaces_earlier_one() {
        let d = MessageData::parse(SRC);
        let list = d.get("ジェイ").unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].message, "上書き");
        let d = MessageData::parse(SRC.split("\n\n").next().unwrap());
        let list = d.get("ジェイ").unwrap();
        assert_eq!(list.len(), 6);
        assert_eq!(list[5].message, "うおおおっ！！");
    }

    #[test]
    fn select_follows_situation_rules() {
        let d = MessageData::parse(SRC.split("\n\n").next().unwrap());
        let list = d.get("ジェイ").unwrap();
        let ctx = SelectContext::default();
        // 射撃 は 攻撃 でも探す。
        assert_eq!(
            select_message(list, "射撃", &ctx, &mut first).as_deref(),
            Some("落ちろ！")
        );
        // (武器名) は 格闘 / 射撃 のときだけ。
        let ctx = SelectContext {
            weapon_sub: Some("ビームライフル".into()),
            ..Default::default()
        };
        let all: Vec<String> = (1..=2)
            .map(|n| select_message(list, "射撃", &ctx, &mut |_| n).unwrap())
            .collect();
        assert_eq!(all, ["落ちろ！", "狙い撃つ！"]);
        // ユニット限定は、そのユニットのときだけ。
        assert_eq!(
            select_message(list, "回避", &SelectContext::default(), &mut first).as_deref(),
            Some("おっと！！")
        );
        let ctx = SelectContext {
            unit_subs: vec!["(龍機神)".into()],
            ..Default::default()
        };
        assert_eq!(
            select_message(list, "回避", &ctx, &mut |n| n).as_deref(),
            Some("龍機神をなめるなよ！")
        );
        // 定義のないシチュエーション。
        assert_eq!(select_message(list, "ダメージ小", &ctx, &mut first), None);
    }

    #[test]
    fn target_specific_message_wins_half_the_time() {
        let d = MessageData::parse(SRC.split("\n\n").next().unwrap());
        let list = d.get("ジェイ").unwrap();
        let ctx = SelectContext {
            target_subs: Some(vec!["(対ロイ)".into()]),
            ..Default::default()
        };
        // Dice(2) = 1 なら相手限定、2 なら一般のメッセージ。
        let mut rolls = [1, 1].into_iter();
        assert_eq!(
            select_message(list, "攻撃", &ctx, &mut |_| rolls.next().unwrap()).as_deref(),
            Some("ロイ！ 覚悟ッ！！")
        );
        let mut rolls = [1, 2, 1].into_iter();
        assert_eq!(
            select_message(list, "攻撃", &ctx, &mut |_| rolls.next().unwrap()).as_deref(),
            Some("落ちろ！")
        );
        // 相手がロイでなければ相手限定は使わない。
        let ctx = SelectContext {
            target_subs: Some(vec!["(対ジム)".into()]),
            force_target: true,
            ..Default::default()
        };
        assert_eq!(
            select_message(list, "攻撃", &ctx, &mut first).as_deref(),
            Some("落ちろ！")
        );
    }
}
