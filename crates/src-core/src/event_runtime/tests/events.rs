//! 自動発火イベント (攻撃 / 使用 / 進入 / 接触 / 行動終了 等のラベル) と進行命令のテスト。

use super::*;

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
fn gameclear_and_gameover_set_state() {
    let mut app = App::new();
    execute(&mut app, &event::parse("GameClear\n").unwrap()).unwrap();
    assert_eq!(app.stage_state(), crate::stage::StageState::Victory);
    let mut app2 = App::new();
    execute(&mut app2, &event::parse("GameOver\n").unwrap()).unwrap();
    assert_eq!(app2.stage_state(), crate::stage::StageState::Defeat);
}

#[test]
fn finish_ends_unit_action_not_stage() {
    // SRC `Finish [unit]` は指定ユニットの行動を終了させる (has_acted=true)。
    // ステージ終了 (Victory) ではない (旧実装の混同を是正)。
    let mut app = App::new();
    let src = "\
Pilot \"リオ\" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit \"ブレイバー\" Real 1 0 陸 5 M 1000 100 5000 100 1500 100 AAAA
Create 味方 ブレイバー 0 リオ 10 3 3
Finish リオ
";
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    // ステージは Victory にならない。
    assert_ne!(app.stage_state(), crate::stage::StageState::Victory);
    // 対象ユニットは行動済み (Action() = 0)。
    let acted = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.pilot_name == "リオ")
        .map(|u| u.has_acted)
        .unwrap_or(false);
    assert!(acted, "Finish 後に has_acted=true であるべき");
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
fn no_arg_clearevent_makes_autofire_event_oneshot() {
    // 引数無し `ClearEvent` が「実行中のイベントラベル」を一度きり化することを検証。
    // (決戦2話 `*攻撃 ＮＰＣ 敵` 末尾の `ClearEvent` 相当。) execute は先頭 Exit で
    // 停止し label のみ登録。trigger_label で 2 回発火し、1 回目のみ body が走る
    // (2 回目は ClearEvent で無効化済み) ことを確認する。
    let src = "\
Exit

攻撃 味方 敵:
Set ran 1
ClearEvent
Exit
";
    let mut app = App::new();
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    assert!(app.script_library().label_pc("攻撃 味方 敵").is_some());

    // 1 回目: body が走り ran=1。
    assert!(trigger_label(&mut app, "攻撃 味方 敵"));
    assert_eq!(app.script_var("ran"), "1", "1 回目は発火するはず");

    // ran をリセットし再発火を観測。ClearEvent で無効化済みなら body は走らず
    // ran は "0" のまま。
    app.set_script_var("ran".to_string(), "0".to_string());
    trigger_label(&mut app, "攻撃 味方 敵");
    assert_eq!(
        app.script_var("ran"),
        "0",
        "引数無し ClearEvent 後は再発火しないはず (一度きり化)"
    );
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
    app.database_mut().remove_unit_at(def_idx);
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
    app.database_mut().remove_units_where(|_| true);
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

/// SRC `Unit.MapAttack` (Unit.cs:27316): プレイヤー/AI 発のマップ攻撃 (is_event=false)
/// ではダメージ適用前に 使用イベント (武器 1 回) + 各対象への 攻撃イベント を発火する。
/// スクリプトの `MapAttack` コマンドは既定 is_event=true で発火せず、末尾に
/// `通常戦闘` を付けたときだけ is_event=false になって発火する (CmdData.cs:14865)。
#[test]
fn map_attack_fires_use_and_attack_events_only_in_normal_battle_mode() {
    fn run(tail: &str) -> App {
        let mut app = App::new();
        let src = format!(
            "Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100\n\
                 Pilot リオ リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100\n\
                 Unit ブレイバー Real 1 0 陸 5 M 1000 100 5000 100 1500 100 AAAA\n\
                 Unit ゾルダ Mass 1 0 陸 5 M 1000 100 9000 80 800 80 BBBB\n\
                 Weapon ブレイバー メガキャノン 600 1 3 +0 0\n\
                 Place ブレイバー リオ 味方 5 5\n\
                 Place ゾルダ ガロ 敵 6 5\n\
                 MapAttack リオ メガキャノン 6 5{tail}\n\
                 Exit\n\
                 \n\
                 使用 リオ メガキャノン:\n\
                 Incr used\n\
                 Return\n\
                 \n\
                 攻撃 リオ ガロ:\n\
                 Incr attacked\n\
                 Return\n"
        );
        let stmts = event::parse(&src).unwrap();
        execute(&mut app, &stmts).unwrap();
        app
    }

    // 既定 (is_event=true): 使用 / 攻撃 とも発火しない。
    let app = run("");
    assert_eq!(
        app.script_var("used"),
        "",
        "スクリプト MapAttack は 使用 イベントを発火しないはず"
    );
    assert_eq!(
        app.script_var("attacked"),
        "",
        "スクリプト MapAttack は 攻撃 イベントを発火しないはず"
    );

    // `通常戦闘` 指定 (is_event=false): 使用 (1 回) + 攻撃 (対象 1 機) が発火。
    let app = run(" 通常戦闘");
    assert_eq!(
        app.script_var("used"),
        "1",
        "通常戦闘 MapAttack は 使用 イベントを発火するはず"
    );
    assert_eq!(
        app.script_var("attacked"),
        "1",
        "通常戦闘 MapAttack は 攻撃 イベントを発火するはず"
    );
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
