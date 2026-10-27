//! `.eve` のユニット操作命令 (Create / Place / Damage / Recover / Item / SetStatus / マップ攻撃 等) のテスト。

use super::*;

/// SRC 互換: 全角英数字を半角と同一視してユニットハンドルを照合する。
/// 公式サンプル1話の `Create … バ5`（半角）↔ `X(バ５)`/`Y(バ５)`（全角）の
/// 参照解決を保証する回帰テスト。
#[test]
fn unit_handle_lookup_folds_fullwidth_to_halfwidth() {
    // 全角→半角畳み込みの基本
    assert_eq!(fold_fullwidth_ascii("バ５"), "バ5");
    assert_eq!(fold_fullwidth_ascii("ＡＢ12"), "AB12");
    assert_eq!(fold_fullwidth_ascii("バ5"), "バ5"); // 既に半角は不変

    // ハンドル `バ5`(半角) を `バ５`(全角) で参照できる (SRC 互換)
    let mut u = UnitInstance::new("隕石バルアド５", "宇宙怪獣(ザコ)", Party::Enemy, 19, 20);
    u.uid = "バ5".into();
    assert!(
        matches_unit_handle(&u, "バ５"),
        "全角参照がハンドルに一致するはず"
    );
    assert!(
        matches_unit_handle(&u, "バ5"),
        "半角参照(完全一致)も当然一致"
    );
    assert!(!matches_unit_handle(&u, "バ6"), "別ハンドルには一致しない");

    // find_unit (X()/Y() が使う) 経由でも解決し、座標が読める
    let mut app = App::new();
    app.database_mut().register_unit(u);
    let found = find_unit(&app, "バ５").expect("全角ハンドルでユニットが見つかるはず");
    assert_eq!((found.x, found.y), (19, 20));
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
fn create_rank_applies_upgrade_level() {
    // SRC `Create party unit rank …` の rank=改造段階。差分オラクル placeunit で C# SRCCore と
    // 突合し、rank2 が MaxHP+400 (=2×UPGRADE_HP_PER_LEVEL) になることを確認 (旧実装は rank を
    // 捨てて常に素のステータスだった)。同一ユニットを rank 0/2 で生成し effective_max_hp を比較。
    let mut app = App::new();
    setup_two_units(&mut app);
    let src = "Create 味方 ブレイバー 0 リオ 0 0 0\n\
                   Create 敵 ブレイバー 2 リオ 0 1 1\n";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    let db = app.database();
    let rank0 = db
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Player && u.unit_data_name == "ブレイバー")
        .unwrap();
    let rank2 = db
        .unit_instances
        .iter()
        .find(|u| u.party == crate::Party::Enemy && u.unit_data_name == "ブレイバー")
        .unwrap();
    assert_eq!(rank0.upgrade_level, 0);
    assert_eq!(rank2.upgrade_level, 2);
    assert_eq!(
        db.effective_max_hp(rank2),
        db.effective_max_hp(rank0) + 2 * crate::db::UPGRADE_HP_PER_LEVEL,
    );
}

#[test]
fn create_level_sets_initial_exp_and_level() {
    // SRC `Create party unit rank pilot level …` の level=主パイロット初期レベル。
    // 旧実装は level を捨ててレベル 1 固定だった (増援が指定レベルにならない)。level 10 で
    // 生成すると total_exp=exp_for_level(10) になり level_from_exp で 10 に解決される。
    // 自己完結のため対象ユニットを inline 定義し、Create で 1 体だけ生成する
    // (setup_two_units は同名 ブレイバー を Place 済みのため使わない)。
    let mut app = App::new();
    let src = "\
MapSize 6 5
Pilot \"リオ\" リオ 男性 c AAAA 100 100 100 100 100 100 100
Unit \"ブレイバー\" リアル系 1 0 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Create 味方 ブレイバー 0 リオ 10 0 0
";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    let inst = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "ブレイバー")
        .unwrap();
    assert_eq!(inst.total_exp, crate::pilot_instance::exp_for_level(10));
    assert_eq!(crate::pilot_instance::level_from_exp(inst.total_exp), 10);
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
    // SRC 座標 (1 始まり) の y = 1..3 は内部 0..2。
    assert_eq!(
        ys,
        vec![0, 1, 2],
        "座標のループ変数が解決されていない: {ys:?}"
    );
    // x は数値リテラルなので全て SRC 4 = 内部 3。
    assert!(app
        .database()
        .unit_instances
        .iter()
        .filter(|u| u.unit_data_name == "壁")
        .all(|u| u.x == 3));
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
    assert!(app
        .database()
        .catalog
        .pilots
        .iter()
        .all(|p| p.name != "ガロ"));
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
    // ExpUp 50 + LevelUp 2 → 50 + 2*500 = 1050 (SRC: 500 exp/level)
    assert_eq!(app.database().unit_instances[0].total_exp, 1050);
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
    app.database_mut()
        .catalog
        .items
        .push(crate::data::item::ItemData {
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

/// `Unit <名前> <ランク>` のランクは改造段階として効く (原典 `UList.Add(uname, rank, …)`)。
#[test]
fn unit_short_form_applies_rank() {
    let mut app = App::new();
    setup_two_units(&mut app);
    execute(&mut app, &event::parse("Unit \"ゾルダII\" 2\n").unwrap()).unwrap();
    let uid = app.selected_unit_for_event().to_string();
    let cur = app.database().unit_by_uid(&uid).unwrap();
    assert_eq!(cur.upgrade_level, 2);
}

#[test]
fn setbullet_updates_weapon_ammo() {
    let mut app = App::new();
    setup_two_units(&mut app);
    // 簡易の weapons は無いので付与してから
    let unit_idx = app
        .database_mut()
        .catalog
        .units
        .iter_mut()
        .position(|u| u.name == "ブレイバー")
        .unwrap();
    app.database_mut().catalog.units[unit_idx]
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

// ───────────────────────────────────────────────────────────────────
// 抜本的回帰ガード — 変数参照解決のクラス的バグを防ぐ網羅テスト群。
//
// バグのクラス: 「変数参照が、解決できないとリテラルのソーステキストを
// 値文脈へ漏らす / 逆に名前が必要な箇所で値へ過剰展開される」。
// 値解決の集約点は統一評価器 (`eval`)、LHS 解決の集約点は `resolve_lhs_name`。
// この 2 つの不変条件を全文脈で固定する。
// ───────────────────────────────────────────────────────────────────

/// 陣営名引数に変数を渡せる (原典は引数を `EvalTerm` で項評価する)。
/// ライブラリ .eve の `Local p = Args(1) / Create p …` 用法が該当。
#[test]
fn party_arg_resolves_variable() {
    let src = "\
Set 陣営 敵
Create 陣営 ブレイバー 0 リオ 1 1 1
Create 味方 ブレイバー 0 リオ 1 2 2
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let parties: Vec<_> = app
        .database()
        .unit_instances
        .iter()
        .map(|u| u.party)
        .collect();
    assert_eq!(parties, vec![Party::Enemy, Party::Player]);
}

/// 未定義の陣営名はリテラル扱いのまま (回帰防止)。
#[test]
fn party_arg_unknown_still_errors() {
    let stmts = event::parse("Create 謎陣営 ブレイバー 0 リオ 1 1 1\n").unwrap();
    let mut app = App::new();
    let e = execute(&mut app, &stmts).unwrap_err();
    assert!(e.to_string().contains("Party"), "{e}");
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
        eval::value_as_string(&app, "Info(ユニットデータ, ブレイバー, 名称)"),
        "ブレイバー"
    );
    assert_eq!(
        eval::value_as_string(&app, "Info(ユニットデータ, ブレイバー, 最大ＨＰ)"),
        "3500"
    );
    assert_eq!(
        eval::value_as_string(&app, "Info(ユニットデータ, ブレイバー, 武器数)"),
        "1"
    );
    assert_eq!(
        eval::value_as_string(&app, "Info(ユニットデータ, ブレイバー, 武器, 1)"),
        "ビームライフル"
    );
    assert_eq!(
        eval::value_as_string(
            &app,
            "Info(ユニットデータ, ブレイバー, 武器, ビームライフル, 攻撃力)"
        ),
        "2500"
    );
    assert_eq!(
        eval::value_as_string(&app, "Info(ユニットデータ, ブレイバー, 武器, 1, 最大射程)"),
        "5"
    );
    // 最大攻撃力 / 最長射程 集約
    assert_eq!(
        eval::value_as_string(&app, "Info(ユニットデータ, ブレイバー, 最大攻撃力)"),
        "2500"
    );
    assert_eq!(
        eval::value_as_string(&app, "Info(ユニットデータ, ブレイバー, 最長射程)"),
        "5"
    );
}

#[test]
fn info_returns_features_from_parsed_unit_txt() {
    // unit.txt 由来の features を Info で参照できる。
    // 直接 UnitData を database に積んで、parser を介さずに動作確認。
    let mut app = App::new();
    app.database_mut()
        .catalog
        .units
        .push(crate::data::unit::UnitData {
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
        eval::value_as_string(
            &app,
            "Info(ユニットデータ, マグナＺ, 特殊能力データ, 全身画像)"
        ),
        "非表示 Anime\\Unit\\EC_MGZ.bmp 96 96"
    );
    // 特殊能力所有 (0/1)
    assert_eq!(
        eval::value_as_string(
            &app,
            "Info(ユニットデータ, マグナＺ, 特殊能力所有, 全身画像)"
        ),
        "1"
    );
    assert_eq!(
        eval::value_as_string(
            &app,
            "Info(ユニットデータ, マグナＺ, 特殊能力所有, 存在しない能力)"
        ),
        "0"
    );
    // 特殊能力数
    assert_eq!(
        eval::value_as_string(&app, "Info(ユニットデータ, マグナＺ, 特殊能力数)"),
        "2"
    );
    // 特殊能力 N (N 番目の名前)
    assert_eq!(
        eval::value_as_string(&app, "Info(ユニットデータ, マグナＺ, 特殊能力, 1)"),
        "全身画像"
    );
    assert_eq!(
        eval::value_as_string(&app, "Info(ユニットデータ, マグナＺ, 特殊能力, 2)"),
        "ブースト"
    );
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
    assert!(app.database().catalog.pilots.is_empty());
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
    // SRC (7, 7) = 内部 (6, 6) 中心 / 同マス空きなので (6, 6) そのもの
    assert_eq!((u.x, u.y), (6, 6));
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
    // Launch の座標は 1 始まり: SRC (9, 10) = 内部 (8, 9)。
    assert_eq!(u.x, 8);
    assert_eq!(u.y, 9);
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
        .catalog
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
    app.database_mut().catalog.units.push(merged);
    // 分離先のユニットデータ
    app.database_mut().catalog.units.push(UnitData {
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
        .catalog
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
    app.database_mut().catalog.units.push(merged);
    app.database_mut().catalog.units.push(UnitData {
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

/// SRC `Unit.MapAttack` (Unit.cs:28560-28688): プレイヤー/AI 発のマップ攻撃
/// (is_event=false) ではダメージ確定後に 損傷率 (生存対象) → 使用後 (攻撃側) →
/// 攻撃後 (生存対象ごと) を発火する。スクリプトの素の `MapAttack` (is_event=true) では
/// これらの後段イベントは発火しない (通常攻撃と同じく "イベント上の戦闘" は非発火)。
#[test]
fn map_attack_fires_after_damage_events_only_in_normal_battle_mode() {
    fn run(tail: &str) -> App {
        let mut app = App::new();
        // 対象 ゾルダ(HP20000/装甲400) はメガキャノン(2500) で撃破されず生存する想定
        // (生存対象でのみ 損傷率 / 攻撃後 / 使用後 が発火する)。
        let src = format!(
            "Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100\n\
                 Pilot リオ リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100\n\
                 Unit ブレイバー Real 1 0 陸 5 M 1000 100 5000 100 1500 100 AAAA\n\
                 Unit ゾルダ Mass 1 0 陸 5 M 1000 100 20000 80 400 80 BBBB\n\
                 Weapon ブレイバー メガキャノン 2500 1 3 +0 0\n\
                 Place ブレイバー リオ 味方 5 5\n\
                 Place ゾルダ ガロ 敵 6 5\n\
                 MapAttack リオ メガキャノン 6 5{tail}\n\
                 Exit\n\
                 \n\
                 損傷率 ガロ 1:\n\
                 Incr 損傷率発火\n\
                 Return\n\
                 \n\
                 使用後 リオ メガキャノン:\n\
                 Incr 使用後発火\n\
                 Return\n\
                 \n\
                 攻撃後 リオ ガロ:\n\
                 Incr 攻撃後発火\n\
                 Return\n"
        );
        let stmts = event::parse(&src).unwrap();
        execute(&mut app, &stmts).unwrap();
        app
    }

    // 既定 (is_event=true): 後段イベント (損傷率 / 使用後 / 攻撃後) は発火しない。
    let app = run("");
    assert_eq!(
        app.script_var("損傷率発火"),
        "",
        "script MapAttack は 損傷率 非発火"
    );
    assert_eq!(
        app.script_var("使用後発火"),
        "",
        "script MapAttack は 使用後 非発火"
    );
    assert_eq!(
        app.script_var("攻撃後発火"),
        "",
        "script MapAttack は 攻撃後 非発火"
    );

    // `通常戦闘` 指定 (is_event=false): 損傷率 + 使用後 + 攻撃後 が発火。
    let app = run(" 通常戦闘");
    // 前提: 対象が生存していること (撃破されると 損傷率/攻撃後 が出ずテストが無意味化)。
    assert!(
        app.database()
            .unit_instances
            .iter()
            .any(|u| u.pilot_name == "ガロ"),
        "対象ゾルダが生存している前提"
    );
    assert_eq!(
        app.script_var("使用後発火"),
        "1",
        "通常戦闘 MapAttack は 使用後 イベントを発火するはず"
    );
    assert_eq!(
        app.script_var("攻撃後発火"),
        "1",
        "通常戦闘 MapAttack は 攻撃後 イベントを発火するはず"
    );
    assert_eq!(
        app.script_var("損傷率発火"),
        "1",
        "通常戦闘 MapAttack は 生存対象に 損傷率 イベントを発火するはず"
    );
}

/// SRC VB5 `Unit.cls:17214`: プレイヤー/AI 発のマップ攻撃 (is_event=false) の撃破は
/// `破壊` ではなく **`マップ攻撃破壊 <対象>`** を発火する。スクリプトの素の `MapAttack`
/// (is_event=true) はポートの進行保証のため従来どおり `破壊` を発火する。
#[test]
fn map_attack_kill_fires_map_destruction_for_player_but_normal_for_script() {
    fn run(tail: &str) -> App {
        let mut app = App::new();
        // ゾルダ(HP100/装甲10) はメガキャノン(9000) で確実に撃破される。
        let src = format!(
            "Pilot リオ リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100\n\
                 Pilot ガロ ガロ 男性 一般 AAAA 100 100 100 100 100 100 100\n\
                 Unit ブレイバー Real 1 0 陸 5 M 1000 100 5000 100 1500 100 AAAA\n\
                 Unit ゾルダ Mass 1 0 陸 5 M 1000 100 100 80 10 80 BBBB\n\
                 Weapon ブレイバー メガキャノン 9000 1 3 +0 0\n\
                 Place ブレイバー リオ 味方 5 5\n\
                 Place ゾルダ ガロ 敵 6 5\n\
                 MapAttack リオ メガキャノン 6 5{tail}\n\
                 Exit\n\
                 \n\
                 破壊 ガロ:\n\
                 Incr 破壊発火\n\
                 Return\n\
                 \n\
                 マップ攻撃破壊 ガロ:\n\
                 Incr マップ破壊発火\n\
                 Return\n"
        );
        let stmts = event::parse(&src).unwrap();
        execute(&mut app, &stmts).unwrap();
        app
    }

    // スクリプト MapAttack (is_event=true): 破壊 が発火、マップ攻撃破壊 は不発。
    let app = run("");
    assert_eq!(
        app.script_var("破壊発火"),
        "1",
        "スクリプト MapAttack の撃破は 破壊 を発火する"
    );
    assert_eq!(
        app.script_var("マップ破壊発火"),
        "",
        "スクリプト MapAttack は マップ攻撃破壊 を発火しない"
    );

    // 通常戦闘 MapAttack (is_event=false): マップ攻撃破壊 が発火、破壊 は不発。
    let app = run(" 通常戦闘");
    assert_eq!(
        app.script_var("マップ破壊発火"),
        "1",
        "通常戦闘 MapAttack の撃破は マップ攻撃破壊 を発火する (原典準拠)"
    );
    assert_eq!(
        app.script_var("破壊発火"),
        "",
        "通常戦闘 MapAttack は 破壊 を発火しない (原典は マップ攻撃破壊)"
    );
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

/// `SpecialPower` は SP を消費しない (`SpecialPowerコマンド.md`「ＳＰは消費しません」)。
/// 旧実装は SP を消費し、不足なら効果を出さなかった。
#[test]
fn specialpower_does_not_consume_sp() {
    let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Place ゾルダ ガロ 敵 5 5
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    app.database_mut().catalog.pilots[0].sp = Some(10);
    // 魂 (cost 60) は残り SP 10 を超えるが、イベントからは発動する。
    execute(&mut app, &event::parse("SpecialPower ガロ 魂\n").unwrap()).unwrap();
    let u = &app.database().unit_instances[0];
    assert_eq!(u.sp_consumed, 0, "SpecialPower で SP が消費された");
    assert!(u.has_condition("魂"));
}

/// B4: `SpecialPower` の瞬間効果はプレイヤー発動と同じ経路で適用される。
/// 旧実装は `信頼` を名前だけの condition として付けるだけで HP を回復しなかった。
#[test]
fn specialpower_instant_effect_uses_spirit_effect_path() {
    let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Place ゾルダ ガロ 敵 5 5
Damage ガロ 600
SpecialPower ガロ 信頼
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let u = &app.database().unit_instances[0];
    assert!(
        u.damage < 600,
        "信頼 で HP が回復していない (damage={})",
        u.damage
    );
    assert!(
        !u.has_condition("信頼"),
        "瞬間効果が condition として残っている"
    );
}

/// `SpecialPower spower` (1 引数) は SelectedUnitForEvent が使用者になる。
#[test]
fn specialpower_single_arg_uses_selected_unit() {
    let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Place ゾルダ ガロ 敵 5 5
Select ガロ
SpecialPower 熱血
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert!(app.database().unit_instances[0].has_condition("熱血"));
}

#[test]
fn recoversp_restores_consumed_sp() {
    let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Place ゾルダ ガロ 敵 5 5
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    app.database_mut().catalog.pilots[0].sp = Some(60);
    app.database_mut().unit_instances[0].sp_consumed = 60;
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
fn party_function_returns_party_label() {
    let mut app = App::new();
    let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass-produced 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Place ゾルダ ガロ 敵 5 5
";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(eval::value_as_string(&app, "Party(ガロ)"), "敵");
    assert_eq!(eval::value_as_string(&app, "Party(ゾルダ)"), "敵");
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
    let out = eval::value_as_string(
        &app,
        "Nickname(Item(リオ, 1), 真アイテム番号(Item(リオ, 1)))",
    );
    assert_eq!(out, "マグネットコーティング");
    // リテラル式が残っていないこと。
    assert!(!out.contains("Nickname"), "literal leaked: {out}");
    assert!(!out.contains("真アイテム番号"), "literal leaked: {out}");
}

#[test]
fn upgrade_replaces_unit_with_target() {
    // SRC `Upgrade unit1 unit2`: unit1 を unit2 のデータに入れ替える (機体変化)。
    let mut app = App::new();
    setup_two_units(&mut app);
    // setup_two_units が定義する 2 機目 (ゾルダII) へ入れ替える。
    let target = "ゾルダII";
    assert!(
        app.database().unit_by_name(target).is_some(),
        "テスト前提: {target} が定義されていること"
    );
    let pilot = app.database().unit_instances[0].pilot_name.clone();
    execute(
        &mut app,
        &event::parse(&format!("Upgrade ブレイバー {target}\n")).unwrap(),
    )
    .unwrap();
    let u = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.pilot_name == pilot)
        .expect("パイロットの機体が消えた");
    assert_eq!(u.unit_data_name, target, "機体が入れ替わっていない");
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
fn kill_and_removeunit_keep_pos_index_in_sync() {
    // 回帰: スクリプトからのユニット除去が位置索引を同期しないと、
    // 空いたマスが占有中と判定され続ける。
    for cmd in ["Kill ゾルダII", "RemoveUnit ゾルダII"] {
        let mut app = App::new();
        execute(&mut app, &event::parse(ZOLDA_SETUP).unwrap()).unwrap();
        assert!(app.database().uid_at(1, 1).is_some());
        execute(&mut app, &event::parse(&format!("{cmd}\n")).unwrap()).unwrap();
        assert!(app.database().unit_instances.is_empty(), "{cmd}");
        assert_eq!(app.database().uid_at(1, 1), None, "{cmd} 後もマスが占有中");
        assert!(app.database().pos_index_is_consistent(), "{cmd}");
    }
}

#[test]
fn damage_uses_effective_max_hp() {
    // 回帰: BossRank 1 で最大 HP 2400 → 3600。基本 HP 以上のダメージでも
    // 実効最大 HP 未満なら撃破されない。
    let mut app = App::new();
    execute(&mut app, &event::parse(ZOLDA_SETUP).unwrap()).unwrap();
    app.database_mut().unit_instances[0].boss_rank = 1;
    execute(&mut app, &event::parse("Damage ゾルダII 3000\n").unwrap()).unwrap();
    assert_eq!(
        app.database().unit_instances.len(),
        1,
        "実効最大 HP 未満で撃破された"
    );
    assert_eq!(app.database().unit_instances[0].damage, 3000);
}

#[test]
fn recover_hp_en_rate_uses_effective_max() {
    // 回帰: 改造 1 段階で最大 HP 2400 → 2600、最大 EN 80 → 90。
    // 回復率は実効最大値に対する割合。
    let mut app = App::new();
    execute(&mut app, &event::parse(ZOLDA_SETUP).unwrap()).unwrap();
    {
        let u = &mut app.database_mut().unit_instances[0];
        u.upgrade_level = 1;
        u.damage = 2000;
        u.en_consumed = 80;
    }
    let src = "RecoverHP ゾルダII 50\nRecoverEN ゾルダII 50\n";
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    let u = &app.database().unit_instances[0];
    assert_eq!(u.damage, 2000 - 1300, "HP 回復量が 2600 の 50% でない");
    assert_eq!(u.en_consumed, 80 - 45, "EN 回復量が 90 の 50% でない");
}

/// 数値引数は原典 `GetArgAsLong` / `GetArgAsDouble` どおり評価済みの値から読み、
/// もう一度評価しない。`[unit] value` 形式は引数 1 個なら選択中のユニットが対象。
#[test]
fn numeric_args_follow_get_arg_as_long() {
    let mut app = App::new();
    setup_two_units(&mut app);
    let braver = app.database().unit_instances[0].uid.clone();
    app.set_selected_unit_for_event(braver.clone());
    let src = "\
Set a b
Set b 99
Hotpoint h a 0 1 1
Set k 0
Incr k \"5\"
SetStatus ブレイバー 攻撃不能 3
SetStatus 移動不能 -1
IncreaseMorale 10
Set 待ち時間 5
Wait 待ち時間
";
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    // `Wait 変数` は変数の値 (0.1 秒単位) だけ待つ (旧実装は原文を数値化して 0 秒)。
    assert_eq!(app.pending_timer(), Some(0.5));
    // 座標 `a` の値 `b` は数値ではないので 0 (旧実装は `b` を再評価して 99)。
    assert_eq!(app.hotpoints()[0].x, 0);
    // 引用符付きの文字列は 0。
    assert_eq!(app.script_var("k"), "0");
    let u = &app.database().unit_instances[0];
    let lifetime = |name: &str| {
        u.conditions
            .iter()
            .find(|c| c.name == name)
            .map(|c| c.lifetime)
    };
    assert_eq!(lifetime("攻撃不能"), Some(3));
    assert_eq!(lifetime("移動不能"), Some(-1));
    assert_eq!(u.morale, 110);
}
