//! `Info(...)` 関数のエッジケース / Info function edge cases.
//!
//! SRC.Sharp `SRCCoreTests/Expressions/InfoFunctionTests.cs` を参考に、
//! 各データ区分 (ユニット / ユニットデータ / パイロット / パイロットデータ
//! / アイテム / マップ) の照会パターンをユニットテストで固定する。
//!
//! 著作権配慮: SRC オリジナルコードは含まない。Pilot/Unit データは inline
//! の `.eve` 命令で生成する。

use src_core::data::event;
use src_core::data::special_power;
use src_core::event_runtime;
use src_core::App;

fn run(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

// データ宣言用の inline 共通 prelude
const PILOT_PRELUDE: &str = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Pilot "花子" 花子 女性 一般 BBBC 50 110 130 120 110 100 100
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon "ブレイバー" "ビームライフル" 2500 2 5 15 -1
Place "ブレイバー" "リオ" Player 2 2
"#;

// ============================================================
//  PilotData 系 (= 静的データ)
// ============================================================

#[test]
fn info_pilot_data_name_returns_name() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"パイロットデータ\",\"リオ\",\"名称\")\n"
    ));
    assert_eq!(app.script_var("v"), "リオ");
}

#[test]
fn info_pilot_data_nonexistent_returns_empty() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"パイロットデータ\",\"存在しない\",\"名称\")\n"
    ));
    assert_eq!(app.script_var("v"), "");
}

#[test]
fn info_pilot_data_exp_value() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"パイロットデータ\",\"リオ\",\"経験値\")\n"
    ));
    assert_eq!(app.script_var("v"), "100");
}

#[test]
fn info_pilot_data_sex() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set m Info(\"パイロットデータ\",\"リオ\",\"性別\")\n"
    ));
    let app2 = run(&format!(
        "{PILOT_PRELUDE}Set f Info(\"パイロットデータ\",\"花子\",\"性別\")\n"
    ));
    assert_eq!(app.script_var("m"), "男性");
    assert_eq!(app2.script_var("f"), "女性");
}

#[test]
fn info_pilot_data_sex_unspecified_returns_empty() {
    // SRC.Sharp 準拠: 性別なし (Unspecified) は空文字 ("-" ではない)。
    // InfoFunctionTests.Info_PilotData_Sex_EmptySex_ReturnsEmpty 準拠
    let src = r#"
Pilot "無性別" 無性別 なし 一般 BBBC 50 110 130 120 110 100 100
Set v Info("パイロットデータ","無性別","性別")
"#;
    let app = run(src);
    assert_eq!(app.script_var("v"), "", "性別なし (なし) は空文字を返す");
}

#[test]
fn info_pilot_data_combat_stats() {
    // Pilot コマンド引数順序: name nick sex class adaption exp_value
    // infight shooting hit dodge intuition technique
    // = リオ リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
    let app = run(&format!(
        r#"{PILOT_PRELUDE}
Set i Info("パイロットデータ","リオ","格闘")
Set s Info("パイロットデータ","リオ","射撃")
Set h Info("パイロットデータ","リオ","命中")
Set d Info("パイロットデータ","リオ","回避")
Set t Info("パイロットデータ","リオ","技量")
"#
    ));
    assert_eq!(app.script_var("i"), "160");
    assert_eq!(app.script_var("s"), "220");
    assert_eq!(app.script_var("h"), "200");
    assert_eq!(app.script_var("d"), "220");
    assert_eq!(app.script_var("t"), "200");
}

// ============================================================
//  Pilot 系 (= 配置中インスタンス)
// ============================================================

#[test]
fn info_pilot_instance_name() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"パイロット\",\"リオ\",\"名称\")\n"
    ));
    assert_eq!(app.script_var("v"), "リオ");
}

#[test]
fn info_pilot_instance_nonexistent_returns_empty() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"パイロット\",\"存在しない\",\"名称\")\n"
    ));
    assert_eq!(app.script_var("v"), "");
}

// ============================================================
//  UnitData 系
// ============================================================

#[test]
fn info_unit_data_max_hp() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニットデータ\",\"ブレイバー\",\"最大ＨＰ\")\n"
    ));
    assert_eq!(app.script_var("v"), "3500");
}

#[test]
fn info_unit_data_max_en() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニットデータ\",\"ブレイバー\",\"最大ＥＮ\")\n"
    ));
    assert_eq!(app.script_var("v"), "120");
}

#[test]
fn info_unit_data_armor() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニットデータ\",\"ブレイバー\",\"装甲\")\n"
    ));
    assert_eq!(app.script_var("v"), "1200");
}

#[test]
fn info_unit_data_mobility() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニットデータ\",\"ブレイバー\",\"運動性\")\n"
    ));
    assert_eq!(app.script_var("v"), "110");
}

#[test]
fn info_unit_data_size() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニットデータ\",\"ブレイバー\",\"サイズ\")\n"
    ));
    assert_eq!(app.script_var("v"), "M");
}

#[test]
fn info_unit_data_weapon_count() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニットデータ\",\"ブレイバー\",\"武器数\")\n"
    ));
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn info_unit_data_weapon_name() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニットデータ\",\"ブレイバー\",\"武器\",\"1\")\n"
    ));
    assert_eq!(app.script_var("v"), "ビームライフル");
}

// ============================================================
//  Unit (instance) 系 — 装備込み現値
// ============================================================

#[test]
fn info_unit_instance_hp_returns_remaining() {
    // 配置直後は damage=0 なので HP=最大値
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニット\",\"ブレイバー\",\"ＨＰ\")\n"
    ));
    assert_eq!(app.script_var("v"), "3500");
}

#[test]
fn info_unit_instance_after_damage() {
    let app = run(&format!(
        r#"{PILOT_PRELUDE}
Damage リオ 500
Set v Info("ユニット","ブレイバー","ＨＰ")
"#
    ));
    assert_eq!(app.script_var("v"), "3000");
}

#[test]
fn info_unit_instance_nonexistent_returns_empty() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニット\",\"存在しない\",\"ＨＰ\")\n"
    ));
    assert_eq!(app.script_var("v"), "");
}

// ============================================================
//  Empty / 未知のキー
// ============================================================

// ============================================================
//  パイロットデータ 追加クエリ
// ============================================================

#[test]
fn info_pilot_data_adaption() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"パイロットデータ\",\"リオ\",\"地形適応\")\n"
    ));
    // リオの地形適応 = "AAAA"
    assert_eq!(app.script_var("v"), "AAAA");
}

#[test]
fn info_pilot_data_nickname() {
    // 愛称
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"パイロットデータ\",\"リオ\",\"愛称\")\n"
    ));
    assert_eq!(app.script_var("v"), "リオ");
}

#[test]
fn info_pilot_data_class() {
    // クラス
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"パイロットデータ\",\"リオ\",\"クラス\")\n"
    ));
    assert_eq!(app.script_var("v"), "超能力者");
}

// ============================================================
//  パイロット (インスタンス) 追加クエリ
// ============================================================

#[test]
fn info_pilot_instance_morale_default_100() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"パイロット\",\"リオ\",\"気力\")\n"
    ));
    assert_eq!(app.script_var("v"), "100");
}

#[test]
fn info_pilot_instance_morale_after_increase() {
    let app = run(&format!(
        "{PILOT_PRELUDE}IncreaseMorale リオ 10\nSet v Info(\"パイロット\",\"リオ\",\"気力\")\n"
    ));
    assert_eq!(app.script_var("v"), "110");
}

// ============================================================
//  ユニットデータ 追加クエリ
// ============================================================

#[test]
fn info_unit_data_speed() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニットデータ\",\"ブレイバー\",\"移動力\")\n"
    ));
    assert_eq!(app.script_var("v"), "5");
}

// ============================================================
//  自動判定 (データ区分省略)
// ============================================================

// ============================================================
//  ユニットデータ — 派生クエリ
// ============================================================

#[test]
fn info_unit_data_max_attack_power() {
    // 最大攻撃力 = 全武器中の最大 power
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニットデータ\",\"ブレイバー\",\"最大攻撃力\")\n"
    ));
    assert_eq!(app.script_var("v"), "2500");
}

#[test]
fn info_unit_data_max_range() {
    // 最長射程 = 全武器中の最大 max_range
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニットデータ\",\"ブレイバー\",\"最長射程\")\n"
    ));
    assert_eq!(app.script_var("v"), "5");
}

#[test]
fn info_unit_data_feature_count_no_features_returns_zero() {
    // 特殊能力数 — 武器なしのユニット定義では 0
    let app = run(r#"
Unit "テスト" リアル系 1 0 陸 5 M 100 100 1000 50 500 80 AAAA
Set v Info("ユニットデータ","テスト","特殊能力数")
"#);
    assert_eq!(app.script_var("v"), "0");
}

// ============================================================
//  武器情報クエリ
// ============================================================

#[test]
fn info_unit_data_weapon_power() {
    // Info(ユニットデータ, ブレイバー, 武器, 1, 攻撃力) → 2500
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニットデータ\",\"ブレイバー\",\"武器\",\"1\",\"攻撃力\")\n"
    ));
    assert_eq!(app.script_var("v"), "2500");
}

#[test]
fn info_unit_data_weapon_by_name() {
    // 番号の代わりに武器名で指定できる
    let app = run(&format!("{PILOT_PRELUDE}Set v Info(\"ユニットデータ\",\"ブレイバー\",\"武器\",\"ビームライフル\",\"攻撃力\")\n"));
    assert_eq!(app.script_var("v"), "2500");
}

#[test]
fn info_unit_data_weapon_max_range() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニットデータ\",\"ブレイバー\",\"武器\",\"1\",\"最大射程\")\n"
    ));
    // Weapon "ブレイバー" "ビームライフル" 2500 2 5 15 -1 → 最大射程=5
    assert_eq!(app.script_var("v"), "5");
}

#[test]
fn info_unit_data_weapon_out_of_bounds_returns_empty() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニットデータ\",\"ブレイバー\",\"武器\",\"99\",\"名称\")\n"
    ));
    assert_eq!(app.script_var("v"), "");
}

#[test]
fn info_auto_detect_unit_instance() {
    // データ区分省略 → ユニット名から自動判定
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ブレイバー\",\"最大ＨＰ\")\n"
    ));
    assert_eq!(app.script_var("v"), "3500");
}

#[test]
fn info_auto_detect_pilot_data() {
    // データ区分省略 → パイロット名から自動判定 (deployed pilot → Unit kind, ＨＰ を取得)
    // リオ is deployed → detect returns Unit kind → info_unit("リオ","ＨＰ") = 3500
    let app = run(&format!("{PILOT_PRELUDE}Set v Info(\"リオ\",\"ＨＰ\")\n"));
    assert_eq!(app.script_var("v"), "3500");
}

// ============================================================
//  ユニット (インスタンス) — EN / 気力 / 累積経験値
// ============================================================

#[test]
fn info_unit_instance_en_returns_remaining() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニット\",\"ブレイバー\",\"ＥＮ\")\n"
    ));
    assert_eq!(app.script_var("v"), "120");
}

#[test]
fn info_unit_instance_morale_default_100() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニット\",\"ブレイバー\",\"気力\")\n"
    ));
    assert_eq!(app.script_var("v"), "100");
}

#[test]
fn info_unit_instance_cumulative_exp_zero_initially() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニット\",\"ブレイバー\",\"累積経験値\")\n"
    ));
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn info_unit_instance_item_count_zero_when_none() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"ユニット\",\"ブレイバー\",\"アイテム数\")\n"
    ));
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn info_empty_first_param_returns_empty() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"\",\"リオ\",\"名称\")\n"
    ));
    assert_eq!(app.script_var("v"), "");
}

#[test]
fn info_unknown_info_kind_returns_empty() {
    let app = run(&format!(
        "{PILOT_PRELUDE}Set v Info(\"パイロットデータ\",\"リオ\",\"存在しない種別\")\n"
    ));
    assert_eq!(app.script_var("v"), "");
}

// ============================================================
//  スペシャルパワー データ区分 (= 静的 SpecialPowerData)
// ============================================================

/// sp.txt 形式のデータを投入した App を返す。
fn run_with_sp(sp_src: &str, eve_src: &str) -> App {
    let mut app = App::new();
    let sps = special_power::parse(sp_src).expect("parse sp");
    app.database_mut().extend_special_powers(sps);
    let stmts = event::parse(eve_src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

// L1=name[,kana], L2=short,sp,target,duration,...
const SP_PRELUDE: &str = "\
熱血, ねっけつ
ネツ, 30, 自分, 発動ターン, -, -, 熱血
";

#[test]
fn info_special_power_name() {
    let app = run_with_sp(
        SP_PRELUDE,
        "Set v Info(\"スペシャルパワー\",\"熱血\",\"名称\")\n",
    );
    assert_eq!(app.script_var("v"), "熱血");
}

#[test]
fn info_special_power_kana() {
    let app = run_with_sp(
        SP_PRELUDE,
        "Set v Info(\"スペシャルパワー\",\"熱血\",\"読み仮名\")\n",
    );
    assert_eq!(app.script_var("v"), "ねっけつ");
}

#[test]
fn info_special_power_short_name() {
    let app = run_with_sp(
        SP_PRELUDE,
        "Set v Info(\"スペシャルパワー\",\"熱血\",\"短縮名\")\n",
    );
    assert_eq!(app.script_var("v"), "ネツ");
}

#[test]
fn info_special_power_sp_cost() {
    let app = run_with_sp(
        SP_PRELUDE,
        "Set v Info(\"スペシャルパワー\",\"熱血\",\"消費ＳＰ\")\n",
    );
    assert_eq!(app.script_var("v"), "30");
}

#[test]
fn info_special_power_target_and_duration() {
    let app = run_with_sp(SP_PRELUDE, "Set t Info(\"スペシャルパワー\",\"熱血\",\"対象\")\nSet d Info(\"スペシャルパワー\",\"熱血\",\"持続期間\")\n");
    assert_eq!(app.script_var("t"), "自分");
    assert_eq!(app.script_var("d"), "発動ターン");
}

#[test]
fn info_special_power_lookup_by_short_name() {
    // 短縮名でも照合できる (寛容なルックアップ)
    let app = run_with_sp(
        SP_PRELUDE,
        "Set v Info(\"スペシャルパワー\",\"ネツ\",\"消費ＳＰ\")\n",
    );
    assert_eq!(app.script_var("v"), "30");
}

#[test]
fn info_special_power_nonexistent_returns_empty() {
    let app = run_with_sp(
        SP_PRELUDE,
        "Set v Info(\"スペシャルパワー\",\"存在しない\",\"名称\")\n",
    );
    assert_eq!(app.script_var("v"), "");
}

#[test]
fn info_special_power_unsupported_field_returns_empty() {
    // 本実装が保持しないフィールド (解説文 等) は空文字
    let app = run_with_sp(
        SP_PRELUDE,
        "Set v Info(\"スペシャルパワー\",\"熱血\",\"解説文\")\n",
    );
    assert_eq!(app.script_var("v"), "");
}
