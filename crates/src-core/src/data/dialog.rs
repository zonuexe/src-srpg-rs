//! `pilot_dialog.txt` (ダイアログデータ) のパーサ。原典 `DialogDataList.Load`。
//!
//! 仕様: `SRC.Sharp/SRC.Sharp.Help/src/ダイアログデータ.md`。複数の話者による台詞。
//! - 空行区切りのレコード。1 行目 = パイロットリスト (空白区切り)。
//! - 続いて `シチュエーション` の行と、その下の `話者, メッセージ` の行。コンマの無い行は
//!   次のシチュエーションになる。
//! - 行の読み方 (コメント・`_` 継続) はメッセージデータと同じ。同じパイロットリストが
//!   2 回定義されたら後の定義で置き換える。
//!
//! シチュエーションに合うダイアログの選び方はメッセージデータと同じ
//! ([`super::message::select_by_situation`])。

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// ダイアログの 1 行 (`話者, メッセージ`)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialogLine {
    /// 話者のパイロット名 (愛称・表情パターン・`@合体技パートナー`・`システム` もある)。
    pub speaker: String,
    pub message: String,
}

/// 1 つのシチュエーションに対するダイアログ (原典 `Dialog`)。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Dialog {
    pub situation: String,
    pub lines: Vec<DialogLine>,
}

/// `pilot_dialog.txt` 全体。パイロットリスト → ダイアログ (定義順)。
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DialogData {
    pub entries: BTreeMap<String, Vec<Dialog>>,
}

impl DialogData {
    /// テキストをパースして取り込む。同じパイロットリストは後の定義で置き換える。
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
            // パイロットリストは空白区切り (原典は LIndex で並べ直す)。
            let pilot_list = lines[i].split_whitespace().collect::<Vec<_>>().join(" ");
            i += 1;
            let mut dialogs: Vec<Dialog> = Vec::new();
            while i < lines.len() && !lines[i].is_empty() {
                let line = &lines[i];
                match (line.split_once(','), dialogs.last_mut()) {
                    (Some((speaker, message)), Some(d)) => d.lines.push(DialogLine {
                        speaker: speaker.trim().to_string(),
                        message: message.trim().to_string(),
                    }),
                    // コンマの無い行はシチュエーション。
                    (None, _) => dialogs.push(Dialog {
                        situation: line.clone(),
                        lines: Vec::new(),
                    }),
                    // シチュエーションより前の台詞は読み飛ばす。
                    (Some(_), None) => {}
                }
                i += 1;
            }
            self.entries.insert(pilot_list, dialogs);
        }
    }

    /// テキストから新規パース。
    pub fn parse(src: &str) -> Self {
        let mut d = Self::default();
        d.merge_from_str(src);
        d
    }

    /// パイロットリストのダイアログ。
    pub fn get(&self, pilot_list: &str) -> Option<&[Dialog]> {
        self.entries.get(pilot_list).map(Vec::as_slice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_situations_and_speakers() {
        let d = DialogData::parse(
            "\
アレクシス  猫丸
回避
猫丸, うーん、良く分かったな？
回避
猫丸, アレクシス！ 後ろ！！
アレクシス, ええいっ_
！！
ダメージ小
システム, -

猫丸
破壊
猫丸, 直撃ぃ？！
",
        );
        let list = d
            .get("アレクシス 猫丸")
            .expect("パイロットリストは空白 1 つで連結");
        assert_eq!(list.len(), 3);
        assert_eq!(list[1].situation, "回避");
        assert_eq!(
            list[1].lines[1],
            DialogLine {
                speaker: "アレクシス".into(),
                message: "ええいっ！！".into()
            }
        );
        assert_eq!(list[2].lines[0].message, "-");
        assert_eq!(d.get("猫丸").unwrap()[0].lines[0].message, "直撃ぃ？！");
    }
}
