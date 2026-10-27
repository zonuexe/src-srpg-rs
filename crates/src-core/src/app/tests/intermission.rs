//! インターミッション (改造・換装・乗り換え・ステータス画面) のテスト。

use super::*;

#[test]
fn victory_proceed_goes_to_intermission_when_commands_registered() {
    use crate::stage::StageState;
    let mut app = App::new();
    app.stage_state = StageState::Victory;
    app.push_intermission_command("改造".to_string(), "x.eve".to_string());
    app.proceed_after_victory();
    assert_eq!(
        app.scene(),
        Scene::Intermission,
        "勝利後 Enter/クリックでインターミッションへ進む"
    );
}

#[test]
fn intermission_upgrade_flow_spends_money_and_boosts_stats() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    app.add_money(5000);
    // インターミッションを表示させる (次ステージありで組込みコマンドが出る)。
    app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
    app.scene = Scene::Intermission;
    app.intermission_mode = IntermissionMode::Menu;
    // メニュー = [機体改造, データセーブ, 次のステージへ]。機体改造 (index 0) を選択。
    assert_eq!(app.intermission_item_label(0).as_deref(), Some("機体改造"));
    app.set_intermission_cursor(0);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.intermission_mode, IntermissionMode::UnitUpgrade);
    // ユニット選択 (index 0 = Hero) → 改造。
    let idx = {
        let uid = first_player_uid(&app);
        app.database().idx_by_uid(&uid).unwrap()
    };
    let hp_before = app
        .database()
        .effective_max_hp(&app.database().unit_instances[idx]);
    let money_before = app.money();
    app.set_intermission_cursor(0);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.database().unit_instances[idx].upgrade_level, 1);
    assert_eq!(app.money(), money_before - 1000, "Lv0→1 は 1000G");
    let hp_after = app
        .database()
        .effective_max_hp(&app.database().unit_instances[idx]);
    assert!(
        hp_after > hp_before,
        "改造で最大HPが上がる: {hp_after} > {hp_before}"
    );
    // 戻る (ユニット数=1 → index 1 が「戻る」) でメインメニューへ。
    app.set_intermission_cursor(1);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.intermission_mode, IntermissionMode::Menu);
    // データセーブ → __quicksave に保存 (項目順は他組込みの増減で変わるため
    // ラベルで index を解決する)。
    let save_idx = (0..app.intermission_item_count())
        .find(|&n| app.intermission_item_label(n).as_deref() == Some("データセーブ"))
        .expect("データセーブ 項目がある");
    app.set_intermission_cursor(save_idx);
    assert!(app.confirm_intermission_selection());
    assert!(
        !app.script_var("__quicksave").is_empty(),
        "データセーブで __quicksave が書き込まれる"
    );
}

/// インターミッション「換装」で `換装` 特殊能力を持つユニットの形態を差し替える。
#[test]
fn intermission_equip_swap_changes_unit_form() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Base", 2, 6);
    // 換装先 "砲撃装備" の UnitData を用意 (Base を複製してリネーム)。
    let mut alt = app.database().unit_by_name("Base").cloned().unwrap();
    alt.name = "砲撃装備".into();
    alt.nickname = "砲撃装備".into();
    app.database_mut().catalog.units.push(alt);
    let uid = first_player_uid(&app);
    app.database_mut()
        .unit_by_uid_mut(&uid)
        .unwrap()
        .active_features = vec![crate::feature::ActiveFeature::new("換装", "砲撃装備")];
    // インターミッション表示 (次ステージありで組込みコマンドが出る)。
    app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
    app.scene = Scene::Intermission;
    app.intermission_mode = IntermissionMode::Menu;

    // メニュー = [機体改造, 換装, データセーブ, 次のステージへ]。換装 = index 1。
    assert_eq!(app.intermission_item_label(1).as_deref(), Some("換装"));
    app.set_intermission_cursor(1);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.intermission_mode, IntermissionMode::EquipSwap);

    // 行 0 = "Base → 砲撃装備"。選択で換装。
    assert_eq!(
        app.intermission_item_label(0).as_deref(),
        Some("Base → 砲撃装備")
    );
    app.set_intermission_cursor(0);
    assert!(app.confirm_intermission_selection());
    assert_eq!(
        app.database().unit_by_uid(&uid).unwrap().unit_data_name,
        "砲撃装備",
        "換装で形態が差し替わる"
    );
}

/// 換装可能なユニットが居なければ「換装」項目を出さない。
#[test]
fn intermission_equip_swap_hidden_without_feature() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Base", 2, 6);
    app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
    app.scene = Scene::Intermission;
    app.intermission_mode = IntermissionMode::Menu;
    let labels: Vec<String> = (0..app.intermission_item_count())
        .filter_map(|n| app.intermission_item_label(n))
        .collect();
    assert!(
        !labels.iter().any(|l| l == "換装"),
        "換装可能ユニットが無ければ項目を出さない: {labels:?}"
    );
}

/// 乗り換えは `Option 乗り換え` 有効時のみ表示される (原典準拠)。
#[test]
fn intermission_ride_change_hidden_without_option() {
    let mut app = App::new();
    let _ = setup_ride_change(&mut app);
    // setup は Option を立てる → 表示される。
    assert!(
        (0..app.intermission_item_count())
            .any(|n| app.intermission_item_label(n).as_deref() == Some("乗り換え")),
        "Option 乗り換え 有効時は表示"
    );
    // Option を解除 → 2 機以上でも非表示。
    app.unset_script_var("Option(乗り換え)");
    assert!(
        (0..app.intermission_item_count())
            .all(|n| app.intermission_item_label(n).as_deref() != Some("乗り換え")),
        "Option 未設定なら 2 機以上でも乗り換えは非表示"
    );
}

/// インターミッション「乗り換え」で 2 機の搭乗パイロットを入れ替える。
#[test]
fn intermission_ride_change_swaps_pilots() {
    let mut app = App::new();
    let (uid_a, uid_b) = setup_ride_change(&mut app);

    // メニューの「乗り換え」を選ぶ → RideChange モード。
    let ride_idx = (0..app.intermission_item_count())
        .find(|&n| app.intermission_item_label(n).as_deref() == Some("乗り換え"))
        .expect("乗り換え 項目がある");
    app.set_intermission_cursor(ride_idx);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.intermission_mode, IntermissionMode::RideChange);

    // 移動元 = UnitA (cursor 0)。
    app.set_intermission_cursor(0);
    assert!(app.confirm_intermission_selection());
    // 移動先候補は移動元を除いた一覧 → cursor 0 = UnitB。先頭は "→ " 付き。
    assert!(app
        .intermission_item_label(0)
        .as_deref()
        .unwrap()
        .starts_with("→ "));
    app.set_intermission_cursor(0);
    assert!(app.confirm_intermission_selection());

    // 搭乗が入れ替わる (pilot_name + pilot_ids)。
    let a = app.database().unit_by_uid(&uid_a).unwrap();
    let b = app.database().unit_by_uid(&uid_b).unwrap();
    assert_eq!(a.pilot_name, "魔理沙");
    assert_eq!(a.pilot_ids, vec!["marisa".to_string()]);
    assert_eq!(b.pilot_name, "アリス");
    assert_eq!(b.pilot_ids, vec!["alice".to_string()]);
}

/// 移動先選択中のキャンセルは移動元選択に戻り、入れ替えは起きない。
#[test]
fn intermission_ride_change_dest_cancel_returns_to_source() {
    let mut app = App::new();
    let (uid_a, _uid_b) = setup_ride_change(&mut app);
    let ride_idx = (0..app.intermission_item_count())
        .find(|&n| app.intermission_item_label(n).as_deref() == Some("乗り換え"))
        .unwrap();
    app.set_intermission_cursor(ride_idx);
    app.confirm_intermission_selection();
    // 移動元 UnitA を選択 → 移動先選択 (→ prefix)。
    app.set_intermission_cursor(0);
    app.confirm_intermission_selection();
    assert!(app
        .intermission_item_label(0)
        .as_deref()
        .unwrap()
        .starts_with("→ "));
    // キャンセル → 移動元選択へ戻る (→ prefix が消える)、入れ替えなし。
    assert!(app.cancel_action());
    assert_eq!(app.intermission_mode, IntermissionMode::RideChange);
    assert!(!app
        .intermission_item_label(0)
        .as_deref()
        .unwrap()
        .starts_with("→ "));
    assert_eq!(
        app.database().unit_by_uid(&uid_a).unwrap().pilot_name,
        "アリス",
        "キャンセルで入れ替わらない"
    );
}

/// インターミッション「ステータス」は単機ステータス詳細を開き、Enter で
/// インターミッションへ戻る。
#[test]
fn intermission_status_opens_unit_detail_and_returns() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Hero", 2, 6);
    app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
    app.scene = Scene::Intermission;
    app.intermission_mode = IntermissionMode::Menu;

    let idx = (0..app.intermission_item_count())
        .find(|&n| app.intermission_item_label(n).as_deref() == Some("ステータス"))
        .expect("ステータス 項目がある");
    app.set_intermission_cursor(idx);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.scene(), Scene::UnitDetail);
    assert_eq!(app.status_detail_index(), 0);

    // ビューモデルが実体から構築できる。
    let detail = app.build_status_detail(0).expect("先頭機の詳細");
    assert_eq!(detail.unit_name, "Hero");
    assert_eq!(detail.total, 1);
    assert!(detail.has_pilot, "PILOT が搭乗");

    // Enter でインターミッションへ復帰。
    app.handle_input(Input::Advance);
    assert_eq!(
        app.scene(),
        Scene::Intermission,
        "ステータス閲覧後はインターミッションへ戻る"
    );
    assert_eq!(app.intermission_mode, IntermissionMode::Menu);
}

/// 単機ステータス詳細: ◀ / ▶ ボタンでロスターを巡回し、右クリック (Esc) /
/// 閉じる ボタンでインターミッションへ戻る。
#[test]
fn unit_detail_cycles_roster_and_closes() {
    let mut app = App::new();
    enter_mapview_with_demo_map(&mut app);
    place_player_unit(&mut app, "Alpha", 2, 6);
    place_player_unit(&mut app, "Bravo", 3, 6);
    app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
    app.scene = Scene::Intermission;
    app.intermission_mode = IntermissionMode::Menu;
    let idx = (0..app.intermission_item_count())
        .find(|&n| app.intermission_item_label(n).as_deref() == Some("ステータス"))
        .expect("ステータス 項目");
    app.set_intermission_cursor(idx);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.scene(), Scene::UnitDetail);
    assert_eq!(app.status_detail_index(), 0);

    // ▶ ボタンで次の機体へ。
    let nb = crate::scene::unit_detail::next_button();
    assert!(app.handle_input(Input::ClickAt {
        x: nb.x + 2,
        y: nb.y + 2,
    }));
    assert_eq!(app.status_detail_index(), 1);
    // 末尾で ▶ → 先頭へラップ。
    assert!(app.handle_input(Input::ClickAt {
        x: nb.x + 2,
        y: nb.y + 2,
    }));
    assert_eq!(app.status_detail_index(), 0);
    // ◀ ボタンで前 (= ラップして末尾) へ。
    let pb = crate::scene::unit_detail::prev_button();
    assert!(app.handle_input(Input::ClickAt {
        x: pb.x + 2,
        y: pb.y + 2,
    }));
    assert_eq!(app.status_detail_index(), 1);

    // 閉じる ボタンでインターミッションへ。
    let cb = crate::scene::unit_detail::close_button();
    assert!(app.handle_input(Input::ClickAt {
        x: cb.x + 2,
        y: cb.y + 2,
    }));
    assert_eq!(app.scene(), Scene::Intermission);

    // 再度開いて右クリック (Cancel) で戻る。
    app.set_intermission_cursor(idx);
    assert!(app.confirm_intermission_selection());
    assert_eq!(app.scene(), Scene::UnitDetail);
    assert!(app.handle_input(Input::Cancel));
    assert_eq!(app.scene(), Scene::Intermission);
}

/// 単機ステータス詳細のビューモデルが実効値 (改造段階・残ダメージ・武器の残弾/EN・
/// 状態異常・パイロットの精神コマンド/SP) を正しく反映する。
#[test]
fn build_status_detail_reflects_effective_values() {
    use crate::condition::Condition;
    use crate::data::pilot::{Adaption, PilotData, Sex, SpiritCommand};
    use crate::data::unit::{Size, UnitData, WeaponData};
    use crate::unit_weapon::UnitWeapon;
    use crate::{Party, UnitInstance};

    let mut app = App::new();

    let pilot = PilotData {
        spirit_commands: vec![
            SpiritCommand {
                name: "ひらめき".into(),
                cost: Some(10),
                level: 1,
            },
            SpiritCommand {
                name: "熱血".into(),
                cost: Some(30),
                level: 9,
            },
        ],
        name: "アスカ".into(),
        nickname: "A".into(),
        kana_name: "あすか".into(),
        sex: Sex::Female,
        class: String::new(),
        adaption: Adaption::parse("ABAA").unwrap(),
        exp_value: 0,
        infight: 120,
        shooting: 130,
        hit: 20,
        dodge: 25,
        intuition: 15,
        technique: 18,
        personality: None,
        sp: Some(80),
        bgm: None,
        bitmap: None,
        features: vec![("底力".into(), "1".into())],
    };
    let weapon = |name: &str, power: i64, lo, hi, bullet, en| WeaponData {
        name: name.into(),
        power,
        min_range: lo,
        max_range: hi,
        precision: 0,
        bullet,
        en_consumption: en,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: String::new(),
        extras: Vec::new(),
    };
    let unit_data = UnitData {
        abilities: Vec::new(),
        name: "エヴァ".into(),
        kana_name: String::new(),
        nickname: "初号機".into(),
        class: "リアル".into(),
        pilot_num: 1,
        item_num: 0,
        transportation: "陸".into(),
        speed: 6,
        size: Size::L,
        value: 0,
        exp_value: 0,
        hp: 5000,
        en: 100,
        armor: 800,
        mobility: 120,
        adaption: Adaption::parse("AAAA").unwrap(),
        bitmap: String::new(),
        weapons: vec![
            weapon("バルカン", 800, 1, 1, 0, 0),
            weapon("ミサイル", 1500, 1, 4, 6, 0),
            weapon("ビーム", 2000, 1, 3, 0, 15),
        ],
        features: Vec::new(),
    };
    app.database_mut().catalog.pilots.push(pilot);
    app.database_mut().catalog.units.push(unit_data);

    let mut inst = UnitInstance::new("エヴァ", "アスカ", Party::Player, 0, 0);
    inst.upgrade_level = 3;
    inst.damage = 1000;
    inst.weapons = vec![
        UnitWeapon::from_data("バルカン", 0, 0),
        UnitWeapon::from_data("ミサイル", 1, 6),
        UnitWeapon::from_data("ビーム", 2, 0),
    ];
    inst.weapons[1].bullet_remaining = 4; // ミサイルを 2 発消費。
    inst.add_condition(Condition::new("加速", 5));
    app.database_mut().register_unit(inst);

    let d = app.build_status_detail(0).expect("先頭機の詳細");
    // 機体: 改造込みの実効 HP と残ダメージ反映。
    assert_eq!(d.unit_name, "エヴァ");
    assert_eq!(d.upgrade_level, 3);
    assert!(d.hp_max > 5000, "改造段階で素 HP を上回る");
    assert_eq!(d.hp_cur, d.hp_max - 1000);
    assert_eq!(d.size, "L");
    assert!(d.conditions.iter().any(|c| c == "加速"));
    // パイロット: SP/精神コマンド。
    assert!(d.has_pilot);
    assert_eq!(d.sp_max, 80);
    // 実効値: レベル成長込み (VB5: 格闘 += level、level 1 でも +1)。
    assert_eq!(d.infight, 121);
    assert_eq!(
        d.spirit_commands,
        vec!["ひらめき Lv1".to_string(), "熱血 Lv9".to_string()]
    );
    // 武器: 射程表記と残弾/EN 表記。
    assert_eq!(d.weapons.len(), 3);
    assert_eq!(d.weapons[0].ammo, "-", "弾/EN なしは '-'");
    assert_eq!(d.weapons[1].range, "1-4");
    assert_eq!(d.weapons[1].ammo, "残4/6");
    assert_eq!(d.weapons[2].ammo, "EN15");
    assert!(
        d.weapons.iter().all(|w| w.usable),
        "必要技能の無い武器は全て使用可"
    );
}

/// 単機ステータス詳細は、必要技能を満たさない武器を `usable=false` で示す
/// (機能ゲートと同じ `necessary_skill::is_satisfied` を閲覧表示にも反映)。
#[test]
fn status_detail_marks_necessary_skill_gated_weapons() {
    use crate::data::pilot::{Adaption, PilotData, Sex};
    use crate::data::unit::{Size, UnitData, WeaponData};
    use crate::unit_weapon::UnitWeapon;
    use crate::{Party, UnitInstance};

    let mk_weapon = |name: &str, necessary: &str| WeaponData {
        name: name.into(),
        power: 1000,
        min_range: 1,
        max_range: 1,
        precision: 0,
        bullet: -1,
        en_consumption: 0,
        necessary_morale: 0,
        adaption: "AAAA".into(),
        critical: 0,
        class: "格".into(),
        extras: if necessary.is_empty() {
            Vec::new()
        } else {
            vec![necessary.into()]
        },
    };
    let mut app = App::new();
    // 念力 技能を持たないパイロット。
    app.database_mut().catalog.pilots.push(PilotData {
        spirit_commands: Vec::new(),
        name: "P".into(),
        nickname: "P".into(),
        kana_name: "P".into(),
        sex: Sex::Unspecified,
        class: String::new(),
        adaption: Adaption::parse("AAAA").unwrap(),
        exp_value: 0,
        infight: 100,
        shooting: 100,
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
    app.database_mut().catalog.units.push(UnitData {
        abilities: Vec::new(),
        name: "機".into(),
        kana_name: String::new(),
        nickname: "機".into(),
        class: String::new(),
        pilot_num: 1,
        item_num: 0,
        transportation: "陸".into(),
        speed: 5,
        size: Size::M,
        value: 0,
        exp_value: 0,
        hp: 3000,
        en: 100,
        armor: 500,
        mobility: 100,
        adaption: Adaption::parse("AAAA").unwrap(),
        bitmap: String::new(),
        weapons: vec![mk_weapon("通常技", ""), mk_weapon("封印技", "念力Lv3")],
        features: Vec::new(),
    });
    let mut inst = UnitInstance::new("機", "P", Party::Player, 0, 0);
    inst.weapons = vec![
        UnitWeapon::from_data("通常技", 0, -1),
        UnitWeapon::from_data("封印技", 1, -1),
    ];
    app.database_mut().register_unit(inst);

    let d = app.build_status_detail(0).expect("詳細");
    assert!(d.weapons[0].usable, "必要技能なしの武器は使用可");
    assert!(
        !d.weapons[1].usable,
        "念力Lv3 を満たさないパイロットの武器は使用不可"
    );
}

/// 通常の部隊表 (戻り先指定なし) は従来どおり UnitList → Title で抜ける。
#[test]
fn unit_list_without_return_goes_to_title() {
    let mut app = App::new();
    app.scene = Scene::UnitList;
    app.handle_input(Input::Advance);
    assert_eq!(app.scene(), Scene::Title, "戻り先未指定なら Title へ");
}

/// `IntermissionCommand` は「インターミッションのメニューに項目を追加する」
/// だけの命令で (`IntermissionCommandコマンド.md`)、シナリオがインター
/// ミッション制であることを意味しない。プロローグで辞典やショップを登録
/// するステージ制シナリオ (例: 多次元世界エニバース) が、これを理由に
/// ステージ開始をブロックされないこと。
#[test]
fn intermission_command_does_not_block_stage_bootstrap() {
    let src = "プロローグ:\n\
IntermissionCommand \"辞典\" \"lib.eve\"\n\
Exit\n\
\n\
スタート:\n\
Exit\n";
    let stmts = crate::data::event::parse(src).unwrap();
    let mut app = App::new();
    app.script_library_mut()
        .append_with_name(&stmts, "stage01.eve");
    // 素材パック判定を避けるためユニット定義を入れる。
    let (units, _) =
        crate::data::unit::parse_lenient("テスト機\nテスト, ＭＳ, 1, 2\n陸, 4, M, 1000, 100\n");
    app.database_mut().extend_units(units);
    // プロローグを実行して IntermissionCommand を登録させる。
    crate::event_runtime::trigger_label(&mut app, "プロローグ");
    assert!(
        !app.intermission_commands.is_empty(),
        "IntermissionCommand が登録されていない"
    );

    app.bootstrap_stage_after_load("stage01.eve");
    assert_eq!(
        app.stage_state(),
        crate::stage::StageState::Battle,
        "IntermissionCommand があってもステージ制なら Battle へ進む"
    );
}

#[test]
fn auto_progress_starts_battle_even_with_intermission_commands() {
    // 回帰テスト: musou 系インターミッション制シナリオで `Continue` チェイン後に
    // 本編 (MapView / Briefing) へ入ると、`IntermissionCommand` が登録済みでも
    // auto_progress が Battle まで進め、マップ未ロードならデフォルト 15×15
    // マップが生成されること。手動 Enter 進行を撤去した後の停止バグの再発防止。
    let mut app = App::new();
    // musou 系は `Stage` コマンドを使わないため stage 表示名は空のまま、
    // 本編突入は `Continue` チェイン (advance_to_next_stage) 経由で
    // current_stage_file が立つことで判定する。該当ファイルは未登録なので
    // ラベル発火は no-op だが current_stage_file は設定される。
    app.set_script_var("次ステージ".to_string(), "東方夢想伝01.eve".to_string());
    let _ = app.advance_to_next_stage();
    app.set_scene(Scene::MapView);
    assert!(app.stage().is_empty());
    // プロローグで登録されたインターミッションコマンドはステージ突入後も残る。
    app.push_intermission_command(
        "プラクティス".to_string(),
        "Lib/プラクティス.eve".to_string(),
    );
    assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);
    assert!(app.database().map.is_none());

    app.auto_progress_stage_state_if_idle();

    assert_eq!(
        app.stage_state(),
        crate::stage::StageState::Battle,
        "intermission_commands 登録済みでも Battle へ進むべき"
    );
    assert!(
        app.database().map.is_some(),
        "マップ未ロードならデフォルトマップが生成されるべき"
    );
}

#[test]
fn intermission_subcommand_returns_to_menu_after_suspend() {
    // FlowCont::ReturnToIntermissionMenu: サブコマンド .eve が suspend した
    // 場合も、完了後にインターミッションメニューへ復帰すること。
    let sub = crate::data::event::parse("プロローグ:\nWait Click\nIncr 実行済\nExit\n").unwrap();
    let mut app = App::new();
    app.script_library_mut().append_with_name(&sub, "lib/x.eve");
    app.push_intermission_command("改造".to_string(), "lib/x.eve".to_string());
    app.set_scene(Scene::Intermission);
    app.set_intermission_cursor(0);

    assert!(app.confirm_intermission_selection());
    // サブコマンド実行中は MapView (PaintString / Hotpoint 表示のため)。
    assert_eq!(app.scene(), Scene::MapView);
    assert!(app.pending_dialog().is_some());

    assert!(app.respond_dialog(0));
    assert_eq!(app.script_var("実行済"), "1");
    assert_eq!(
        app.scene(),
        Scene::Intermission,
        "サブコマンド完了後はメニューへ復帰するべき"
    );
}
