//! 武器・アビリティ・特殊能力の属性文字列末尾に付く `<必要条件> (必要技能)` の
//! 切り離し (パース側)。評価は [`crate::necessary_skill`] が担う。

/// ユニット用特殊能力 (必要技能.md §3) の値末尾に**スペース区切りで**付いた
/// `(必要技能)` / `<必要条件>` を切り離す。`(value_only, necessary_skill, necessary_condition)`。
///
/// `split_necessary` と違い**直前のスペースを必須**にする: 特殊能力の値はそれ自体が
/// `(...)` を含むことがある (例: 形態名 `ガンダム(MA)`、`変形=ガンダム(MA) ジ・O`)。
/// スペース無しの `値(…)` を必要技能と誤認して値を切り詰めると壊れるため、
/// 「` (…)` で終わる」場合だけ必要技能とみなす。SRC のデータ書式は必ず空白を挟む。
pub fn split_feature_necessary(value: &str) -> (String, String, String) {
    let mut buf = value.trim_end().to_string();
    let mut skill = String::new();
    let mut cond = String::new();
    // 末尾が ` (...)` (スペース＋括弧グループ) なら必要技能。
    if buf.ends_with(')') {
        if let Some(open) = buf.rfind('(') {
            if open > 0 && buf[..open].ends_with(' ') {
                skill = buf[open + 1..buf.len() - 1].trim().to_string();
                buf = buf[..open].trim_end().to_string();
            }
        }
    }
    // 末尾が ` <...>` なら必要条件。
    if buf.ends_with('>') {
        if let Some(open) = buf.rfind('<') {
            if open > 0 && buf[..open].ends_with(' ') {
                cond = buf[open + 1..buf.len() - 1].trim().to_string();
                buf = buf[..open].trim_end().to_string();
            }
        }
    }
    (buf, skill, cond)
}

/// クラス/属性文字列の末尾に付いた `<必要条件> (必要技能)` を切り離す。
///
/// `(class_only, necessary_skill, necessary_condition)` を返す。SRC.Sharp
/// `UnitDataList` の武器属性パース (line 1331-1365) と同じ手順で、末尾が `)` なら
/// 必要技能、続けて末尾が `>` なら必要条件を剥がす。`(` `)` `<` `>` は ASCII なので
/// マルチバイト文字列でもバイト境界は安全。
pub fn split_necessary(raw: &str) -> (String, String, String) {
    let mut buf = raw.trim().to_string();
    let mut skill = String::new();
    let mut cond = String::new();

    // 必要技能: 末尾が ')'。
    if buf.ends_with(')') {
        if let Some(gt) = buf.find("> ") {
            // `<必要条件> (必要技能)` 形式: "> " より後ろが "(必要技能)"。
            let after = buf[gt + 2..].to_string();
            buf = buf[..gt + 1].trim().to_string();
            skill = paren_inner(&after, '(', ')');
        } else if let Some(op) = buf.find('(') {
            skill = buf[op + 1..]
                .strip_suffix(')')
                .unwrap_or(&buf[op + 1..])
                .trim()
                .to_string();
            buf = buf[..op].trim().to_string();
        }
    }

    // 必要条件: 末尾が '>'。
    if buf.ends_with('>') {
        if let Some(op) = buf.find('<') {
            cond = buf[op + 1..]
                .strip_suffix('>')
                .unwrap_or(&buf[op + 1..])
                .trim()
                .to_string();
            buf = buf[..op].trim().to_string();
        }
    }

    // 残余クラスの末尾カンマ (トークナイザ起因の余剰) を除去。
    let class = buf.trim_end_matches(',').trim().to_string();
    (class, skill, cond)
}

/// `open`〜`close` で囲まれた中身を取り出す (`(術Lv3)` → `術Lv3`)。
fn paren_inner(s: &str, open: char, close: char) -> String {
    match s.find(open) {
        Some(op) => s[op + 1..]
            .strip_suffix(close)
            .unwrap_or(&s[op + 1..])
            .trim()
            .to_string(),
        None => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_necessary_extracts_skill_from_class() {
        let (class, skill, cond) = split_necessary("武 (念力Lv3)");
        assert_eq!(class, "武");
        assert_eq!(skill, "念力Lv3");
        assert_eq!(cond, "");
    }

    #[test]
    fn split_feature_necessary_requires_space_before_paren() {
        // §3: 特殊能力の値末尾 ` (必要技能)` を剥がす。スペース必須。
        let (val, skill, cond) = split_feature_necessary("FormB (念力Lv3)");
        assert_eq!(val, "FormB");
        assert_eq!(skill, "念力Lv3");
        assert_eq!(cond, "");
        // スペース無しの値内 (...) は必要技能と誤認せず温存 (形態名 ガンダム(MA) 等)。
        let (val2, skill2, _) = split_feature_necessary("ガンダム(MA)");
        assert_eq!(val2, "ガンダム(MA)");
        assert_eq!(skill2, "");
        // 必要条件 ` <...>` も剥がす。
        let (val3, skill3, cond3) = split_feature_necessary("X <母艦3マス以内> (術Lv4)");
        assert_eq!(val3, "X");
        assert_eq!(skill3, "術Lv4");
        assert_eq!(cond3, "母艦3マス以内");
        // 条件なしの値はそのまま。
        let (val4, skill4, _) = split_feature_necessary("ただの値");
        assert_eq!(val4, "ただの値");
        assert_eq!(skill4, "");
    }

    #[test]
    fn split_necessary_extracts_condition_and_skill() {
        let (class, skill, cond) = split_necessary("格Ｐ無 <母艦3マス以内> (術Lv4)");
        assert_eq!(class, "格Ｐ無");
        assert_eq!(skill, "術Lv4");
        assert_eq!(cond, "母艦3マス以内");
    }

    #[test]
    fn split_necessary_condition_only() {
        let (class, skill, cond) = split_necessary("武 <瀕死>");
        assert_eq!(class, "武");
        assert_eq!(skill, "");
        assert_eq!(cond, "瀕死");
    }

    #[test]
    fn split_necessary_no_marker() {
        let (class, skill, cond) = split_necessary("格Ｐ無墜L20");
        assert_eq!(class, "格Ｐ無墜L20");
        assert_eq!(skill, "");
        assert_eq!(cond, "");
    }
}
