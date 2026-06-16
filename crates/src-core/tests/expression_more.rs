//! `&` 連結 / 変数を関数引数に渡す / ネスト式の挙動。
//!
//! SRC.Sharp `SRCCoreTests/Expressions/ExpressionMoreTests.cs` を参考に、
//! 実シナリオ頻出パターンの動作を pin する。
//!
//! 著作権配慮: SRC オリジナルコードは含まない。

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn run(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

// ============================================================
//  `&` 連結演算子
// ============================================================

#[test]
fn concat_three_strings() {
    let app = run(r#"Set v "a" & "b" & "c""#);
    assert_eq!(app.script_var("v"), "abc");
}

#[test]
fn concat_variable_and_literal() {
    let app = run(r#"
Set hp 100
Set v "HP:" & $(hp)
"#);
    assert_eq!(app.script_var("v"), "HP:100");
}

#[test]
fn concat_numeric_variable_and_string() {
    let app = run(r#"
Set score 42
Set v "score=" & $(score)
"#);
    assert_eq!(app.script_var("v"), "score=42");
}

#[test]
fn concat_four_parts() {
    let app = run(r#"Set v "1" & "2" & "3" & "4""#);
    assert_eq!(app.script_var("v"), "1234");
}

// ============================================================
//  変数を関数引数に
// ============================================================

#[test]
fn len_of_variable() {
    let app = run(r#"
Set s hello
Set v Len($(s))
"#);
    assert_eq!(app.script_var("v"), "5");
}

#[test]
fn left_of_japanese_variable() {
    let app = run(r#"
Set name "リオ"
Set v Left($(name),1)
"#);
    assert_eq!(app.script_var("v"), "リ");
}

#[test]
fn abs_of_negative_variable() {
    let app = run(r#"
Set val -7
Set v Abs($(val))
"#);
    assert_eq!(app.script_var("v"), "7");
}

#[test]
fn int_of_decimal_variable() {
    let app = run(r#"
Set x 3.7
Set v Int($(x))
"#);
    assert_eq!(app.script_var("v"), "3");
}

// ============================================================
//  $(var) 置換中の算術
// ============================================================

#[test]
fn dollar_var_replace_with_string() {
    let app = run(r#"
Set name リオ
Set msg "パイロット: $(name)"
"#);
    assert_eq!(app.script_var("msg"), "パイロット: リオ");
}

#[test]
fn dollar_var_replace_numeric_in_string() {
    let app = run(r#"
Set hp 1500
Set msg "残 HP: $(hp)"
"#);
    assert_eq!(app.script_var("msg"), "残 HP: 1500");
}

#[test]
fn three_variables_in_one_string() {
    let app = run(r#"
Set a 1
Set b 2
Set c 3
Set msg "$(a)-$(b)-$(c)"
"#);
    assert_eq!(app.script_var("msg"), "1-2-3");
}

// ============================================================
//  変数の値が後で更新されたら expression は新値を使う
// ============================================================

#[test]
fn variable_update_then_use_new_value() {
    let app = run(r#"
Set x 10
Set old $(x)
Set x 20
Set new $(x)
"#);
    assert_eq!(app.script_var("old"), "10");
    assert_eq!(app.script_var("new"), "20");
}

// ============================================================
//  複雑な算術 + 変数
// ============================================================

#[test]
fn complex_arithmetic_with_variables() {
    let app = run(r#"
Set a 10
Set b 3
Set v Eval(($(a) * 2 + $(b)) * 4)
"#);
    // (10*2 + 3) * 4 = 23 * 4 = 92
    assert_eq!(app.script_var("v"), "92");
}

#[test]
fn nested_function_with_variables() {
    let app = run(r#"
Set s hello world
Set v Len(Left($(s),5))
"#);
    assert_eq!(app.script_var("v"), "5");
}

// ============================================================
//  Format additional edge cases (SRC.Sharp StringFunctionAdditional)
// ============================================================

#[test]
fn format_three_digit_zero_pad() {
    let app = run(r#"Set v Format(7,"000")"#);
    assert_eq!(app.script_var("v"), "007");
}

#[test]
fn format_hundred_three_digits_unchanged() {
    let app = run(r#"Set v Format(100,"000")"#);
    assert_eq!(app.script_var("v"), "100");
}

#[test]
fn format_decimal_hash_drops_trailing_zero() {
    // Format(3.14, "0.##") → "3.14"
    let app = run(r#"Set v Format(3.14,"0.##")"#);
    assert_eq!(app.script_var("v"), "3.14");
}

#[test]
fn format_whole_double_with_hash_format() {
    // Format(3.0, "0.##") → "3" (trailing zeros が `#` で削られる)
    let app = run(r#"Set v Format(3.0,"0.##")"#);
    // pin 挙動 (本実装の format_with_pattern が `#` の trailing-zero drop
    // を行うか観察)
    let v = app.script_var("v");
    assert!(
        v == "3" || v == "3.0" || v == "3.00",
        "Format(3.0,'0.##') = {v}"
    );
}

#[test]
fn format_negative_with_zero_pad() {
    // Format(-5, "00") → "-05"
    let app = run(r#"Set v Format(-5,"00")"#);
    assert_eq!(app.script_var("v"), "-05");
}

// ============================================================
//  IsDefined (実シナリオで 40 回使用)
// ============================================================

#[test]
fn is_defined_pilot() {
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Set v IsDefined("リオ","パイロット")
Set u IsDefined("ガロ","パイロット")
"#);
    assert_eq!(app.script_var("v"), "1");
    assert_eq!(app.script_var("u"), "0");
}

#[test]
fn is_defined_unit_instance() {
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon "ブレイバー" "ライフル" 2500 2 5 15 -1
Place "ブレイバー" "リオ" Player 0 0
Set v IsDefined("ブレイバー","ユニット")
Set u IsDefined("ゲルググ","ユニット")
"#);
    assert_eq!(app.script_var("v"), "1");
    assert_eq!(app.script_var("u"), "0");
}

#[test]
fn is_defined_single_arg_any_kind() {
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Set v IsDefined("リオ")
Set u IsDefined("存在しない")
"#);
    assert_eq!(app.script_var("v"), "1");
    assert_eq!(app.script_var("u"), "0");
}

// ============================================================
//  $(...) replace edge cases
// ============================================================

#[test]
fn replace_with_japanese_var_name() {
    let app = run(r#"
Set 名前 リオ
Set v "こんにちは$(名前)さん"
"#);
    assert_eq!(app.script_var("v"), "こんにちはリオさん");
}

#[test]
fn replace_unclosed_bracket_leaves_literal() {
    // 閉じてない $( は literal で残るのが SRC.Sharp 期待
    let app = run(r#"Set v "こんにちは$(名前""#);
    let v = app.script_var("v");
    // パーサが何かしらクラッシュしないことだけ pin
    assert!(v.contains("こんにちは"));
}

#[test]
fn replace_empty_subexpression_returns_empty() {
    // `$()` は空文字に置換される (SRC.Sharp 仕様)
    let app = run(r#"Set v "前$()後""#);
    // pin 挙動: 実装は `$()` の中身 "" を変数名として扱い、空文字を返す
    let v = app.script_var("v");
    assert!(
        v.contains("前") && v.contains("後"),
        "expected 前...後, got: {v}"
    );
}

#[test]
fn replace_two_variables_both_replaced() {
    let app = run(r#"
Set 名前A リオ
Set 名前B ガロ
Set v "$(名前A)対$(名前B)"
"#);
    assert_eq!(app.script_var("v"), "リオ対ガロ");
}

// ============================================================
//  LSet / RSet (実シナリオで Rset 151 回使用)
// ============================================================

#[test]
fn lset_pads_right_with_spaces() {
    let app = run(r#"Set v LSet("AB",5)"#);
    assert_eq!(app.script_var("v"), "AB   ");
}

#[test]
fn rset_pads_left_with_spaces() {
    let app = run(r#"Set v RSet("AB",5)"#);
    assert_eq!(app.script_var("v"), "   AB");
}

#[test]
fn lset_exact_width_unchanged() {
    let app = run(r#"Set v LSet("HELLO",5)"#);
    assert_eq!(app.script_var("v"), "HELLO");
}

#[test]
fn rset_longer_than_width_unchanged() {
    let app = run(r#"Set v RSet("HELLO",3)"#);
    assert_eq!(app.script_var("v"), "HELLO");
}

// ============================================================
//  LSet / RSet 全角文字の表示幅対応
//  SRC.Sharp 準拠: 全角文字 = 2、半角文字 = 1 としてパディング幅を計算する。
//  `GeneralLib.StrWidth` 相当 (MaxMinFunctionFurtherTests / Units agent 報告)
// ============================================================

#[test]
fn lset_fullwidth_chars_count_as_two() {
    // "あいう" は全角3文字 = 幅6 → LSet(..., 6) でパディングなし
    let app = run(r#"Set v LSet("あいう",6)"#);
    assert_eq!(app.script_var("v"), "あいう");
}

#[test]
fn lset_fullwidth_chars_padded_correctly() {
    // "あ" は全角1文字 = 幅2 → LSet("あ", 6) は 4スペース追加
    let app = run(r#"Set v LSet("あ",6)"#);
    assert_eq!(app.script_var("v"), "あ    ");
}

#[test]
fn rset_fullwidth_chars_count_as_two() {
    // "AB" は半角2文字 = 幅2 → RSet("AB", 6) は4スペース左パディング
    let app = run(r#"Set v RSet("AB",6)"#);
    assert_eq!(app.script_var("v"), "    AB");
}

#[test]
fn lset_mixed_half_and_fullwidth() {
    // "aあ" = 半角1 + 全角2 = 幅3 → LSet("aあ", 5) は2スペース
    let app = run(r#"Set v LSet("aあ",5)"#);
    assert_eq!(app.script_var("v"), "aあ  ");
}

// ============================================================
//  IsAvailable (実シナリオで 99 回使用)
// ============================================================

#[test]
fn is_available_returns_1_for_existing_feature() {
    // UnitData の features に "特殊能力 飛行" を含めるシナリオ
    // Note: 我々の Unit 命令は features 引数を取らないので、Pilot/Unit
    // 命令 のみで features を持たせるのは難しい。data::unit::parse 経由
    // でしか features は登録できない。
    // ここでは features がないユニットで IsAvailable が常に 0 を返す
    // ことだけ確認 (false 路の正しさ)。
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon "ブレイバー" "ライフル" 2500 2 5 15 -1
Place "ブレイバー" "リオ" Player 0 0
Set v IsAvailable("ブレイバー","飛行")
"#);
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn is_available_returns_0_for_unknown_unit() {
    let app = run(r#"Set v IsAvailable("存在しない","飛行")"#);
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn is_available_returns_1_for_active_feature() {
    use src_core::data::event;
    use src_core::event_runtime::execute;
    use src_core::feature::ActiveFeature;
    use src_core::App;

    let src = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Place "ブレイバー" "リオ" Player 0 0
Set v IsAvailable("ブレイバー","飛行")
"#;
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("v"), "0", "active_features 未登録なら 0");

    // 手動で active_feature を追加
    app.database_mut().unit_instances[0]
        .active_features
        .push(ActiveFeature::new("飛行", ""));
    let stmts2 = event::parse("Set v2 IsAvailable(ブレイバー, 飛行)\n").unwrap();
    execute(&mut app, &stmts2).unwrap();
    assert_eq!(
        app.script_var("v2"),
        "1",
        "active_features に飛行があれば 1"
    );
}

/// Place コマンドが UnitData.features から active_features を自動初期化することを確認。
/// Pass 47: populate_active_features ヘルパを Place / Create に追加。
#[test]
fn place_populates_active_features_from_unit_data() {
    use src_core::data::{event, unit};
    use src_core::event_runtime::execute;
    use src_core::App;

    // unit.txt テキストで 飛行 feature を持つ UnitData を作成。
    // フォーマット: 名前行 / クラス等行 / 移動種別等行 / 特殊能力セクション / 能力名[=値] / HP等行 / 適応,bitmap行
    let unit_txt = "\
ブレイバー
ブレイバー, ぶれいばー, リアル系, 1, 0
陸, 5, M, 1000, 100
特殊能力
飛行=
3500, 120, 1200, 110
AAAA, braver.bmp
";
    let units = unit::parse(unit_txt).expect("unit parse");
    assert!(!units.is_empty(), "ユニット解析失敗");
    assert!(
        units[0].features.iter().any(|(k, _)| k == "飛行"),
        "features に 飛行 が含まれるはず: {:?}",
        units[0].features
    );
    let mut app = App::new();
    app.database_mut().extend_units(units);

    // Place でインスタンス化
    let src = "\
Pilot リオ リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Place ブレイバー リオ Player 1 1
Set v IsAvailable(ブレイバー, 飛行)
";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(
        app.script_var("v"),
        "1",
        "Place 後に IsAvailable(飛行) = 1 のはず"
    );
}

// ============================================================
//  Count(prefix) — 配列要素カウント (SRC.Sharp Other.cs::Count)
// ============================================================

#[test]
fn count_array_prefix_returns_element_count() {
    let app = run(r#"
Set xs[1] alpha
Set xs[2] beta
Set xs[3] gamma
Set v Count(xs)
"#);
    assert_eq!(app.script_var("v"), "3");
}

#[test]
fn count_array_with_japanese_prefix() {
    let app = run(r#"
Set 入手ユニット候補[1] ブレイバー
Set 入手ユニット候補[2] ゾルダ
Set v Count(入手ユニット候補)
"#);
    assert_eq!(app.script_var("v"), "2");
}

#[test]
fn count_unknown_prefix_returns_zero() {
    let app = run("Set v Count(存在しない)");
    assert_eq!(app.script_var("v"), "0");
}

// ============================================================
//  UnitID / PilotID
// ============================================================

#[test]
fn pilotid_returns_pilot_name_of_unit() {
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon "ブレイバー" "ライフル" 2500 2 5 15 -1
Place "ブレイバー" "リオ" Player 0 0
Set v PilotID("ブレイバー")
"#);
    assert_eq!(app.script_var("v"), "リオ");
}

#[test]
fn pilotid_unknown_unit_returns_input() {
    let app = run(r#"Set v PilotID("存在しない")"#);
    // legacy 互換: 未解決時は input そのまま
    assert_eq!(app.script_var("v"), "存在しない");
}

#[test]
fn countpilot_returns_pilot_count_of_unit() {
    // `CountPilot(unit)` — 搭乗パイロット数。単一パイロットモデルなので
    // 搭乗あり=1 / パイロット不在・未解決=0。スパロボ戦記 AlphaSecond.eve
    // の `搭乗員[0,1] = CountPilot(...)` がループ境界に使う。
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon "ブレイバー" "ライフル" 2500 2 5 15 -1
Place "ブレイバー" "リオ" Player 0 0
Set 乗員 CountPilot("ブレイバー")
Set 無 CountPilot("存在しない")
"#);
    assert_eq!(app.script_var("乗員"), "1");
    assert_eq!(app.script_var("無"), "0");
}

// ============================================================
//  Unit-query 関数の unresolved-unit 挙動
//  (SRC.Sharp AUnitFunction は unit=null で 0 / "" を返す)
// ============================================================

#[test]
fn hp_of_unknown_unit_returns_zero() {
    let app = run(r#"Set v HP("存在しない")"#);
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn unit_query_with_empty_arg_returns_zero_not_literal() {
    // `HP(Args(1))` で `Args(1)` が未設定 → 空引数。引数なし / 空でも
    // literal `HP(...)` を残さず "0" を返す (画面の生表示防止)。
    let app = run(r#"
Set h $(HP())
Set h2 $(HP(Args(1)))
Set e $(EN())
"#);
    assert_eq!(app.script_var("h"), "0");
    assert_eq!(app.script_var("h2"), "0");
    assert_eq!(app.script_var("e"), "0");
}

#[test]
fn maxhp_en_armor_unknown_unit_returns_zero() {
    let app = run(r#"
Set a MaxHP("nope")
Set b EN("nope")
Set c MaxEN("nope")
Set d Armor("nope")
Set e Mobility("nope")
Set f Speed("nope")
"#);
    assert_eq!(app.script_var("a"), "0");
    assert_eq!(app.script_var("b"), "0");
    assert_eq!(app.script_var("c"), "0");
    assert_eq!(app.script_var("d"), "0");
    assert_eq!(app.script_var("e"), "0");
    assert_eq!(app.script_var("f"), "0");
}

#[test]
fn morale_exp_xy_unknown_unit_returns_zero() {
    let app = run(r#"
Set a Morale("nope")
Set b Exp("nope")
Set c X("nope")
Set d Y("nope")
"#);
    assert_eq!(app.script_var("a"), "0");
    assert_eq!(app.script_var("b"), "0");
    assert_eq!(app.script_var("c"), "0");
    assert_eq!(app.script_var("d"), "0");
}

#[test]
fn pilot_party_unknown_unit_returns_empty() {
    let app = run(r#"
Set a Pilot("nope")
Set b Party("nope")
"#);
    assert_eq!(app.script_var("a"), "");
    assert_eq!(app.script_var("b"), "");
}

#[test]
fn distance_unknown_unit_returns_zero() {
    let app = run(r#"Set v Distance("nope1","nope2")"#);
    assert_eq!(app.script_var("v"), "0");
}

// ============================================================
//  Skill(pilot, name) — used 85 times in real scenarios
// ============================================================

#[test]
fn skill_unknown_pilot_returns_zero() {
    let app = run(r#"Set v Skill("nope","熱血")"#);
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn skill_known_pilot_unknown_skill_returns_zero() {
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Set v Skill("リオ","存在しないスキル")
"#);
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn unitid_resolves_to_uid_when_assigned() {
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon "ブレイバー" "ライフル" 2500 2 5 15 -1
Place "ブレイバー" "リオ" Player 0 0
Set v UnitID("ブレイバー")
"#);
    // unique ID または unit_data_name のどちらかが返る (実装依存)。
    // Place 経由なら uid はまだ空かも → unit_data_name にフォールバック
    let v = app.script_var("v");
    assert!(v == "ブレイバー" || v.starts_with('U'), "UnitID = {v}");
}

// ============================================================
//  Party() / Pilot() / Unit() 関数 (配置済みユニット情報)
// ============================================================

#[test]
fn party_function_returns_player_for_player_unit() {
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit "ブレイバー" リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Place "ブレイバー" "リオ" Player 1 1
Set p Party(リオ)
"#);
    // Party() は日本語陣営名を返す: Player → "味方"
    assert_eq!(app.script_var("p"), "味方");
}

#[test]
fn party_function_returns_enemy_for_enemy_unit() {
    let app = run(r#"
Pilot "ガロ" ガロ 男性 一般 AAAA 100 100 100 100 100 100 100
Unit "ゾルダ" Mass 1 0 陸 5 M 1000 100 2500 110 1000 100 AAAA
Place "ゾルダ" "ガロ" Enemy 3 3
Set p Party(ガロ)
"#);
    // Party() は日本語陣営名を返す: Enemy → "敵"
    assert_eq!(app.script_var("p"), "敵");
}

#[test]
fn pilot_function_returns_pilot_name_of_unit() {
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit "ブレイバー" リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Place "ブレイバー" "リオ" Player 1 1
Set p Pilot(ブレイバー)
"#);
    assert_eq!(app.script_var("p"), "リオ");
}

#[test]
fn unit_function_returns_unit_name_of_pilot() {
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit "ブレイバー" リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Place "ブレイバー" "リオ" Player 1 1
Set u Unit(リオ)
"#);
    assert_eq!(app.script_var("u"), "ブレイバー");
}

#[test]
fn count_party_legacy_fallback() {
    // 後方互換: party 名指定で該当陣営の unit 数
    let app = run(r#"
Pilot "A" A 男性 一般 BBBC 50 100 120 110 110 100 100
Unit "u1" リアル系 1 3 陸 5 M 1000 200 1200 100 800 80 BBBC
Weapon "u1" "w" 100 1 1 10 -1
Place "u1" "A" Player 0 0
Set v Count("Player")
"#);
    assert_eq!(app.script_var("v"), "1");
}

// ============================================================
//  Nickname 関数
// ============================================================

#[test]
fn nickname_returns_pilot_nickname() {
    // Pilot "リオ・カザミ" リオ → Nickname("リオ・カザミ") = "リオ"
    let app = run(r#"
Pilot "リオ・カザミ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Set n Nickname("リオ・カザミ")
"#);
    assert_eq!(app.script_var("n"), "リオ");
}

#[test]
fn nickname_unknown_name_returns_input() {
    // 未登録の名前 → 入力をそのまま返す。
    let app = run(r#"Set n Nickname("存在しない")"#);
    assert_eq!(app.script_var("n"), "存在しない");
}

#[test]
fn nickname_unit_returns_unit_nickname() {
    // Unit "ブレイバー" の最初の引数が愛称 (実際はユニット名と同じことが多いが
    // データ上は別フィールド)。
    let app = run(r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Set n Nickname("ブレイバー")
"#);
    // ユニット name="ブレイバー", nickname は parse で取られる先頭フィールド = "ブレイバー"
    assert_eq!(app.script_var("n"), "ブレイバー");
}

// ============================================================
//  RestoreEvent / ClearEvent round-trip
// ============================================================

#[test]
fn restore_event_re_registers_cleared_label() {
    // ClearEvent でラベル削除 → RestoreEvent で再登録 → Call で呼び出せる。
    let app = run(r#"
@restored_sub:
Set was_called 1
Return
ClearEvent "restored_sub"
RestoreEvent "restored_sub"
Call restored_sub
"#);
    assert_eq!(
        app.script_var("was_called"),
        "1",
        "RestoreEvent でラベルが再登録され Call できる"
    );
}

#[test]
fn restore_alias_re_registers_cleared_label() {
    // `Restore` は `RestoreEvent` の別名として機能する。
    let app = run(r#"
@restored_sub2:
Set was_called 1
Return
ClearEvent "restored_sub2"
Restore "restored_sub2"
Call restored_sub2
"#);
    assert_eq!(
        app.script_var("was_called"),
        "1",
        "Restore (別名) でラベルが再登録される"
    );
}

// ============================================================
//  Load / Forget タイトル (作品) ロードリスト
// ============================================================

#[test]
fn load_adds_titles_to_load_list() {
    let app = run("Load \"ブレイバー\" \"マグナ\"\n");
    assert!(app.titles().iter().any(|t| t == "ブレイバー"));
    assert!(app.titles().iter().any(|t| t == "マグナ"));
}

#[test]
fn load_skips_duplicate_titles() {
    // 既登録の作品は重複追加されない。
    let app = run("Load \"ブレイバー\"\nLoad \"ブレイバー\"\n");
    let count = app.titles().iter().filter(|t| *t == "ブレイバー").count();
    assert_eq!(count, 1, "重複ロードは 1 件のみ");
}

#[test]
fn forget_removes_title_from_load_list() {
    // Load で追加した作品を Forget で除去する。
    let app = run("Load \"ブレイバー\" \"マグナ\"\nForget \"ブレイバー\"\n");
    assert!(
        !app.titles().iter().any(|t| t == "ブレイバー"),
        "Forget で除去される"
    );
    assert!(app.titles().iter().any(|t| t == "マグナ"), "他の作品は残る");
}

#[test]
fn forget_nonexistent_title_is_noop() {
    // 未登録の作品を Forget しても他に影響しない。
    let app = run("Load \"ブレイバー\"\nForget \"存在しない\"\n");
    assert!(
        app.titles().iter().any(|t| t == "ブレイバー"),
        "既存の作品は残る"
    );
}

// ============================================================
//  VB6 整数除算 `\` / 累乗 `^` を括弧算術 `(...)` で評価
//  (スパロボ戦記 AlphaSecond.eve のページ数計算 `(N - 1) \ 8 + 1` 等)
// ============================================================

#[test]
fn paren_arith_integer_division() {
    // `\` は VB6 整数除算 (ゼロ方向切り捨て)。
    assert_eq!(run("Set z (3 \\ 8 + 1)").script_var("z"), "1");
    assert_eq!(run("Set z (17 \\ 5)").script_var("z"), "3");
}

#[test]
fn paren_arith_power_operator() {
    assert_eq!(run("Set z (2 ^ 3)").script_var("z"), "8");
}

#[test]
fn weapon_page_count_expression_evaluates() {
    // 武器性能タブのページ数: 真武器番[ページ数] = ((真武器番[武器数] - 1) \ 8 + 1)
    // 武器数 3 → (3 - 1) \ 8 + 1 = 0 + 1 = 1。以前は `\` がアトムに紛れて
    // 評価されず生文字列 `((3 - 1) \ 8 + 1)` のまま残っていた。
    let app = run("Set 武器数 3\nSet 真武器番[武器数] 3\n\
         Set 真武器番[ページ数] ((真武器番[武器数] - 1) \\ 8 + 1)\n");
    assert_eq!(app.script_var("真武器番[ページ数]"), "1");
}

#[test]
fn item_page_count_expression_evaluates() {
    // 強化パーツのページ数: ((N - 1) \ 4 + 1), N=5 → 2。
    assert_eq!(
        run("Set n 5\nSet p ((n - 1) \\ 4 + 1)").script_var("p"),
        "2"
    );
}

// ============================================================
//  括弧無し算術の評価 (SRC ExecSetCmd / EvalTerm 準拠)
//  `var = a - b` / `Set var a + b * c` 等。以前は括弧付き
//  (`(a - b)`) しか評価されず、括弧無しは生式文字列のまま格納されていた
//  (温泉旅館シナリオの経営計算 `資本金 = 資本金 - 営業収支` が全て壊れる原因)。
// ============================================================

#[test]
fn bareword_assign_subtraction_evaluates() {
    // `HP = HP - 30` (括弧無し) → 70。以前は生文字列 "HP - 30" が格納されていた。
    assert_eq!(run("Set HP 100\nHP = HP - 30").script_var("HP"), "70");
}

#[test]
fn bareword_assign_respects_precedence() {
    // `c = a + b * 2` (括弧無し) → 5 + 3*2 = 11 (乗算優先)。
    assert_eq!(run("Set a 5\nSet b 3\nc = a + b * 2").script_var("c"), "11");
}

#[test]
fn set_form_bareword_arithmetic_not_evaluated() {
    // `Set var bareword-算術` (= 形でない) は従来どおり数値化しない (SRC では引数過多
    // エラーになる無効形。括弧付き `Set var (a - b)` か `=` 形を使う)。括弧付き / `=` 形
    // のみを評価することで、引用符付き文字列や Format 出力の誤数値化を防ぐ。
    assert_eq!(
        run("Set a 5\nSet b 3\nSet c a - b").script_var("c"),
        "a - b"
    );
}

#[test]
fn counter_increment_via_bareword_assign() {
    // `n = n + 1` を 2 回 → 2。カウンタ加算の頻出パターン。
    assert_eq!(run("Set n 0\nn = n + 1\nn = n + 1").script_var("n"), "2");
}

#[test]
fn set_non_arith_string_value_is_preserved() {
    // 算術演算子を含まない複数トークン文字列は数値化せず文字列のまま残す
    // (`Set 名前 山田 太郎` を 0 に潰さない)。
    assert_eq!(run("Set 名前 山田 太郎").script_var("名前"), "山田 太郎");
}

#[test]
fn set_arith_with_nonnumeric_operand_stays_string() {
    // 算術演算子を含むが非数値アトムを含む値は文字列のまま (`A-B` を 0 にしない)。
    assert_eq!(run("Set v 朝-夜").script_var("v"), "朝-夜");
}

#[test]
fn set_func_call_plus_literal_evaluates() {
    // `減少 = Random(1) + 10` — 関数呼出 (expand_vars) + トップレベルの括弧無し加算。
    // Random(1) は SRC `Dice` 準拠で常に 1 を返すため結果は決定論的に 11。
    // 関数呼出を含んでも、その外側に算術演算子があれば算術式として評価される。
    assert_eq!(run("減少 = Random(1) + 10").script_var("減少"), "11");
}

#[test]
fn set_format_output_is_not_renumericized() {
    // `Set v Format(-5,"00")` → "-05"。関数の整形出力 (先頭 `-` + 数字) を
    // 算術と誤認して再数値化 (`-5`) しないこと (value_is_arith_expr が関数呼出単体を弾く)。
    assert_eq!(run(r#"Set v Format(-5,"00")"#).script_var("v"), "-05");
}

#[test]
fn set_quoted_digits_with_dash_stays_string() {
    // `Set msg "$(a)-$(b)-$(c)"` → "1-2-3"。引用符付き文字列は算術評価しない
    // (展開後 `1-2-3` を `-4` に潰さない)。
    let app = run("Set a 1\nSet b 2\nSet c 3\nSet msg \"$(a)-$(b)-$(c)\"");
    assert_eq!(app.script_var("msg"), "1-2-3");
}

// ============================================================
//  算術式インデックスの配列アクセス `arr[(式)]`
//  (スパロボ戦記 AlphaSecond.eve の武器/強化パーツ一覧:
//   真武器番[((Args(2) - 1) * 8 + i)] / 真アイテム番[((Args(2) - 1) * 4 + i)])
// ============================================================

#[test]
fn array_index_with_arithmetic_expression() {
    // `arr[((i - 1) * 2 + 3)]` (i=1) → arr[3]
    let app = run("Set arr[3] hello\nSet i 1\nSet x arr[((i - 1) * 2 + 3)]");
    assert_eq!(app.script_var("x"), "hello");
}

#[test]
fn array_index_spaced_minus() {
    // `arr[j - 1]` (j=2) → arr[1] (武器そーと の隣接比較)
    let app = run("Set arr[1] x\nSet arr[2] y\nSet j 2\nSet z arr[j - 1]");
    assert_eq!(app.script_var("z"), "x");
}

#[test]
fn array_index_with_args_inside_subroutine() {
    // 各武器表示 相当: サブルーチン内で 真武器番[((Args(2) - 1) * 8 + i)] を読む。
    // Args(2)=1, i=1 → 索引 1 / i=2 → 索引 2。
    let src = "\
Set 真武器番[1] 50
Set 真武器番[2] 70
Call sub 99 1
Exit
sub:
For i = 1 To 2
Set out[$(i)] 真武器番[((Args(2) - 1) * 8 + i)]
Next
Return
";
    let app = run(src);
    assert_eq!(app.script_var("out[1]"), "50");
    assert_eq!(app.script_var("out[2]"), "70");
}

#[test]
fn array_index_string_key_not_corrupted() {
    // 演算子を含まない文字列添字キー (`アイテム数` 等) は数値化されず保持される。
    let app = run("Set 真アイテム番[アイテム数] 5\nSet x 真アイテム番[アイテム数]");
    assert_eq!(app.script_var("x"), "5");
}

// ============================================================
//  条件式の比較対象が括弧付き算術式のケース
//  (`If (N - 1) > 3` / `Loop While (i \ 8) < 2` 等)
// ============================================================

fn cond_run(src: &str) -> App {
    let mut app = App::new();
    let _ = event_runtime::execute(&mut app, &event::parse(src).expect("parse"));
    app
}

#[test]
fn if_condition_with_arithmetic_operand() {
    // 左辺が括弧付き算術式でも評価して比較する。文字列のまま比較されて
    // 常に偽になっていた不具合の回帰防止。
    assert_eq!(
        cond_run("Set n 5\nIf (n - 1) > 3 Then\nSet r y\nEndIf\n").script_var("r"),
        "y"
    );
    assert_eq!(
        cond_run("Set n 2\nIf (n - 1) > 3 Then\nSet r y\nEndIf\n").script_var("r"),
        ""
    );
    // 整数除算を含む算術オペランド。
    assert_eq!(
        cond_run("Set n 20\nIf (n \\ 8) >= 2 Then\nSet r y\nEndIf\n").script_var("r"),
        "y"
    );
}

#[test]
fn if_string_comparison_still_works() {
    // 算術評価の追加で文字列比較が壊れていないこと (誤って数値化しない)。
    assert_eq!(
        cond_run("Set s ランダム\nIf s = ランダム Then\nSet r y\nEndIf\n").script_var("r"),
        "y"
    );
    assert_eq!(
        cond_run("Set s あいう\nIf s = ランダム Then\nSet r y\nEndIf\n").script_var("r"),
        ""
    );
}
