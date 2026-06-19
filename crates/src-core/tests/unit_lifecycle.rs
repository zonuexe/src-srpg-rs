//! ユニット配置 / 移動 / 離脱 ライフサイクル edge cases。
//! (Place / Create / Launch / Escape / MoveUnit / Leave / Getoff / Ride)

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

const PRELUDE: &str = r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Pilot "ガロ" ガロ 男性 超能力者 AAAA 100 160 200 180 200 220 180
Unit "ブレイバー" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon "ブレイバー" "ライフル" 2500 2 5 15 -1
Unit "ゾルダII" リアル系 1 3 陸 5 M 2200 300 2400 100 900 80 BBCC
Weapon "ゾルダII" "ゾルダマシンガン" 1200 1 4 12 -1
MapSize 10 10
"#;

fn run_setup(extra: &str) -> App {
    let mut app = App::new();
    let src = format!("{PRELUDE}{extra}");
    let stmts = event::parse(&src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

fn find_unit_named<'a>(app: &'a App, name: &str) -> Option<&'a src_core::UnitInstance> {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == name)
}

// ============================================================
//  Place
// ============================================================

#[test]
fn place_creates_unit_at_coords() {
    let app = run_setup(r#"Place "ブレイバー" "リオ" Player 3 4"#);
    let u = find_unit_named(&app, "ブレイバー").expect("unit placed");
    assert_eq!(u.x, 3);
    assert_eq!(u.y, 4);
    assert_eq!(u.party, src_core::Party::Player);
    assert_eq!(u.pilot_name, "リオ");
}

#[test]
fn place_multiple_units() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 1 1
Place "ゾルダII" "ガロ" Enemy 5 5
"#,
    );
    assert_eq!(app.database().unit_instances.len(), 2);
    let g = find_unit_named(&app, "ブレイバー").unwrap();
    let z = find_unit_named(&app, "ゾルダII").unwrap();
    assert_eq!(g.party, src_core::Party::Player);
    assert_eq!(z.party, src_core::Party::Enemy);
}

// ============================================================
//  MoveUnit
// ============================================================

#[test]
fn moveunit_relocates() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
MoveUnit リオ 5 7
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.x, 5);
    assert_eq!(u.y, 7);
}

#[test]
fn moveunit_unknown_unit_is_noop() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
MoveUnit 存在しない 5 5
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.x, 0);
    assert_eq!(u.y, 0);
}

// ============================================================
//  Escape / Launch
// ============================================================

#[test]
fn escape_marks_unit_off_map() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Escape リオ
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert!(u.off_map, "Escape で off_map=true になるべき");
}

#[test]
fn launch_brings_unit_back_to_map() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Escape リオ
Launch リオ 2 3
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert!(!u.off_map, "Launch で off_map=false になるべき");
    assert_eq!(u.x, 2);
    assert_eq!(u.y, 3);
}

// ============================================================
//  RemoveUnit / RemovePilot
// ============================================================

#[test]
fn remove_unit_drops_from_instances() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
RemoveUnit リオ
"#,
    );
    assert_eq!(app.database().unit_instances.len(), 0);
}

// ============================================================
//  Transform
// ============================================================

#[test]
fn transform_swaps_unit_data_name() {
    // ブレイバー → ゾルダII (本来の意味と違うが API テスト)
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Transform リオ "ゾルダII"
"#,
    );
    // ブレイバーが居らず、ゾルダII (リオ操縦) が居るはず
    assert!(find_unit_named(&app, "ブレイバー").is_none());
    let z = find_unit_named(&app, "ゾルダII");
    assert!(z.is_some());
    if let Some(u) = z {
        assert_eq!(u.pilot_name, "リオ");
    }
}

// ============================================================
//  ChangeParty
// ============================================================

#[test]
fn change_party_switches_affiliation() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
ChangeParty リオ Enemy
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.party, src_core::Party::Enemy);
}

#[test]
fn change_party_accepts_jp_names() {
    // SRC.Sharp の ChangePartyCmd は "味方"/"敵"/"中立"/"ＮＰＣ" を受理。
    // 本実装も EN/JP 両方の party 名を `parse_party` で解釈する。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
ChangeParty リオ 敵
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.party, src_core::Party::Enemy);
}

#[test]
fn change_party_to_allied_then_back() {
    // 連続変更は最後の指定が勝つ。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
ChangeParty リオ Allied
ChangeParty リオ Player
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.party, src_core::Party::Player);
}

#[test]
fn change_party_neutral_value() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
ChangeParty リオ Neutral
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.party, src_core::Party::Neutral);
}

#[test]
fn change_party_unknown_unit_is_noop() {
    // 存在しないユニットへの ChangeParty は影響を与えない。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
ChangeParty 存在しない Enemy
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.party, src_core::Party::Player);
}

#[test]
fn change_party_via_unit_data_name_lookup() {
    // matches_unit_handle は unit_data_name でも探す。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
ChangeParty ブレイバー Enemy
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.party, src_core::Party::Enemy);
}

// ============================================================
//  ReplacePilot
// ============================================================

fn unit_damage(app: &App, name: &str) -> i64 {
    find_unit_named(app, name).map(|u| u.damage).unwrap_or(0)
}

#[test]
fn replacepilot_overwrites_pilot_name() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 3 3
ReplacePilot リオ ガロ
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.pilot_name, "ガロ");
}

#[test]
fn replacepilot_preserves_position() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 4 5
ReplacePilot リオ ガロ
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!((u.x, u.y), (4, 5));
}

#[test]
fn replacepilot_preserves_damage() {
    // SRC.Sharp ではパイロットの morale/exp を新パイロットに引き継ぐが、
    // 本実装ではダメージはユニット側にあるので素直に保たれる。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Damage リオ 500
ReplacePilot リオ ガロ
"#,
    );
    assert_eq!(unit_damage(&app, "ブレイバー"), 500);
}

#[test]
fn replacepilot_unknown_unit_is_noop() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
ReplacePilot 存在しないパイロット ガロ
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.pilot_name, "リオ");
}

#[test]
fn replacepilot_via_unit_data_name_lookup() {
    // matches_unit_handle は unit_data_name でも引けるので機体名指定も通る。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
ReplacePilot ブレイバー ガロ
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.pilot_name, "ガロ");
}

// ============================================================
//  MoveUnit (追加 edge cases)
// ============================================================

#[test]
fn moveunit_via_unit_data_name_lookup() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
MoveUnit ブレイバー 3 4
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!((u.x, u.y), (3, 4));
}

#[test]
fn moveunit_preserves_pilot_and_party() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
MoveUnit リオ 2 3
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.pilot_name, "リオ");
    assert_eq!(u.party, src_core::Party::Player);
}

#[test]
fn moveunit_does_not_unstuck_off_map() {
    // Escape で off_map=true になったユニットを MoveUnit しても、
    // 座標は変わるが off_map フラグは触らない (Launch との分担)。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Escape リオ
MoveUnit リオ 5 5
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert!(u.off_map, "MoveUnit は off_map を解除しない");
    assert_eq!((u.x, u.y), (5, 5));
}

#[test]
fn moveunit_to_origin() {
    // 0,0 への移動 (型 u32 の下端) も問題なし。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 5 5
MoveUnit リオ 0 0
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!((u.x, u.y), (0, 0));
}

// ============================================================
//  Transform (追加 edge cases)
// ============================================================

#[test]
fn transform_preserves_position() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 6 7
Transform リオ "ゾルダII"
"#,
    );
    let u = find_unit_named(&app, "ゾルダII").unwrap();
    assert_eq!((u.x, u.y), (6, 7));
}

#[test]
fn transform_preserves_damage_and_party() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Damage リオ 800
Transform リオ "ゾルダII"
"#,
    );
    let u = find_unit_named(&app, "ゾルダII").unwrap();
    assert_eq!(u.damage, 800);
    assert_eq!(u.party, src_core::Party::Player);
}

#[test]
fn transform_unknown_unit_is_noop() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Transform 存在しない "ゾルダII"
"#,
    );
    assert!(find_unit_named(&app, "ブレイバー").is_some());
    assert!(find_unit_named(&app, "ゾルダII").is_none());
}

/// Transform 後に active_features が新フォームの UnitData.features に更新される。
/// Pass 48: Transform コマンドの active_features 更新を実装。
#[test]
fn transform_updates_active_features() {
    use src_core::data::unit;
    use src_core::event_runtime::execute;

    // 飛行フォーム "ブレイバーF" を UnitData として直接登録
    let unit_txt = "\
ブレイバーF
ブレイバーF, ぶれいばーF, リアル系, 1, 0
空, 5, M, 1000, 100
特殊能力
飛行=
3500, 120, 1200, 110
AAAA, braverF.bmp
";
    let units = unit::parse(unit_txt).expect("unit parse");
    let mut app = App::new();
    app.database_mut().extend_units(units);

    let src = "\
Pilot リオ リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Place ブレイバー リオ Player 0 0
Set before IsAvailable(ブレイバー, 飛行)
Transform リオ ブレイバーF
Set after IsAvailable(リオ, 飛行)
";
    let stmts = src_core::data::event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();

    assert_eq!(app.script_var("before"), "0", "変形前は飛行なし");
    assert_eq!(app.script_var("after"), "1", "変形後は飛行あり");
}

// ============================================================
//  RemoveUnit / RemovePilot (追加 edge cases)
// ============================================================

#[test]
fn remove_unit_via_pilot_name() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
RemoveUnit リオ
"#,
    );
    assert_eq!(app.database().unit_instances.len(), 0);
}

#[test]
fn remove_unit_unknown_is_noop() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
RemoveUnit 存在しない
"#,
    );
    assert_eq!(app.database().unit_instances.len(), 1);
}

#[test]
fn remove_pilot_drops_definition() {
    // RemovePilot は pilots テーブルからパイロットデータを削除。
    let mut app = App::new();
    let stmts = event::parse(
        r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Pilot "ガロ" ガロ 男性 超能力者 AAAA 100 160 200 180 200 220 180
RemovePilot リオ
"#,
    )
    .expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    let names: Vec<&str> = app
        .database()
        .pilots
        .iter()
        .map(|p| p.name.as_str())
        .collect();
    assert!(!names.contains(&"リオ"), "リオ should be removed");
    assert!(names.contains(&"ガロ"), "ガロ should remain");
}

#[test]
fn remove_pilot_unknown_is_noop() {
    let mut app = App::new();
    let stmts = event::parse(
        r#"
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
RemovePilot 存在しない
"#,
    )
    .expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    assert_eq!(app.database().pilots.len(), 1);
}

// ============================================================
//  Escape / Launch (追加 edge cases)
// ============================================================

#[test]
fn launch_resets_off_map_for_on_map_unit() {
    // off_map でないユニットへの Launch も、座標再配置だけ働く。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Launch リオ 7 8
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert!(!u.off_map);
    assert_eq!((u.x, u.y), (7, 8));
}

#[test]
fn escape_launch_roundtrip_preserves_pilot_and_data() {
    // Escape → Launch で同じ ユニットが復帰 (新しい座標で)。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Escape リオ
Launch リオ 9 9
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert!(!u.off_map);
    assert_eq!(u.pilot_name, "リオ");
    assert_eq!((u.x, u.y), (9, 9));
}

#[test]
fn escape_unknown_unit_is_noop() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Escape 存在しない
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert!(!u.off_map);
}

// ============================================================
//  Combine / Split
// ============================================================

#[test]
fn combine_sets_unit_data_name_to_mode() {
    // Combine [unit] mode — 最後の引数が新形態。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Combine リオ "ゾルダII"
"#,
    );
    let u = find_unit_named(&app, "ゾルダII").unwrap();
    assert_eq!(u.pilot_name, "リオ");
}

#[test]
fn combine_one_arg_targets_first_player_unit() {
    // 引数 1 個なら Player の任意のユニットを selection 扱い。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Combine "ゾルダII"
"#,
    );
    assert!(find_unit_named(&app, "ゾルダII").is_some());
    assert!(find_unit_named(&app, "ブレイバー").is_none());
}

// ============================================================
//  Place 追加バリエーション
// ============================================================

#[test]
fn place_each_party_value() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Place "ゾルダII" "ガロ" Enemy 1 1
"#,
    );
    let g = find_unit_named(&app, "ブレイバー").unwrap();
    let z = find_unit_named(&app, "ゾルダII").unwrap();
    assert_eq!(g.party, src_core::Party::Player);
    assert_eq!(z.party, src_core::Party::Enemy);
}

#[test]
fn place_using_jp_party_name() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" 味方 2 2
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.party, src_core::Party::Player);
}

// ============================================================
//  Leave / GetOff / Join (現行 Rust 実装の挙動)
// ============================================================

#[test]
fn leave_marks_unit_off_map() {
    // 本実装の Leave は Escape と同等 (off_map=true)。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Leave リオ
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert!(u.off_map);
}

#[test]
fn getoff_clears_pilot_name() {
    // Getoff は乗機からパイロットを降ろす (pilot_name 空に)。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
GetOff リオ
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.pilot_name, "");
}

#[test]
fn ride_two_arg_mounts_pilot_onto_unit() {
    // `Ride pilot unit` — 名前指定ユニットに搭乗員を載せる。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Ride ガロ ブレイバー
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    assert_eq!(u.pilot_name, "ガロ");
}

#[test]
fn join_restores_left_unit_from_leave() {
    // SRC `Joinコマンド.md`: `Join [unit]` は Leave で離脱させたユニットを復帰させる。
    // パイロット差替えは Ride / ReplacePilot を使う。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 0 0
Leave ブレイバー
Join ブレイバー
"#,
    );
    let u = find_unit_named(&app, "ブレイバー").unwrap();
    // 離脱解除 → off_map=false, life_state="" に戻る
    assert!(!u.off_map, "Join 後は off_map=false");
    assert_eq!(u.life_state, "", "Join 後は life_state が空");
    // パイロットは変わらない
    assert_eq!(u.pilot_name, "リオ");
}

#[test]
fn unit_short_form_creates_current_unit_and_ride_mounts() {
    // `Unit <name>` でカレントユニットを生成 → `Ride <pilot>` (unit 省略) で
    // そのカレントユニットに搭乗員を載せる。SRC の乗せ換えイディオム。
    let app = run_setup(
        r#"
Unit "ゾルダII" 0
Ride ガロ
"#,
    );
    let uid = app.selected_unit_for_event();
    assert!(!uid.is_empty(), "カレントユニット uid が設定される");
    let cur = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.uid == uid)
        .expect("current unit instance exists");
    assert_eq!(cur.unit_data_name, "ゾルダII");
    assert_eq!(cur.pilot_name, "ガロ");
    assert!(cur.off_map, "未配置 (off_map) で生成される");
}

#[test]
fn unit_short_form_does_not_break_data_definition() {
    // 14+ 引数の `Unit` データ定義形式は従来通り UnitData を登録する
    // (短い形式の追加で誤って instance 化されないこと)。
    let app = run_setup(r#"Unit "テスト機" リアル系 1 0 陸 5 M 1000 100 2000 80 800 70 AAAA"#);
    assert!(app.database().units.iter().any(|u| u.name == "テスト機"));
    assert!(
        !app.database()
            .unit_instances
            .iter()
            .any(|u| u.unit_data_name == "テスト機"),
        "データ定義形式は instance を作らない"
    );
}

#[test]
fn ask_format2_boarding_message_resolves_pilot_and_unit_names() {
    // スパロボ戦記 Include.eve「乗せ換え処理」の搭乗メッセージ回帰テスト。
    // `Ask 乗せ換え表示 ...` (SRC Ask Format 2) は選んだ要素の **添字** を
    // `選択` に格納する。続く `Pilot(選択)` / `Info(ユニット,乗せ換えユニット,愛称)`
    // は裸の変数を引数に取るので、関数側で script_var 解決する必要がある。
    // 以前は (1) 添字でなく表示文字列が `選択` に入り、(2) 関数が裸の変数を
    // 解決しなかったため、メッセージが「Nickname()がに搭乗した。」と化けていた。
    let mut app = App::new();
    let src = format!(
        "{PRELUDE}{}",
        "\
Place \"ブレイバー\" \"リオ\" 味方 3 3
Set 乗せ換えユニット ブレイバー
Set 乗せ換え表示[$(UnitID(リオ))] \"リオ ブレイバー\"
Ask 乗せ換え表示 \"$(Info(ユニット, 乗せ換えユニット, 愛称))に乗せるキャラクターを選んでください。\" キャンセル可
If Not 選択 = \"\" Then
　If 選択 = \"パイロットを降ろす\" Then
　　Set どこ おりた
　Else
　　Talk システム
　　$(Nickname(Pilot(選択)))が$(Info(ユニット,乗せ換えユニット,愛称))に搭乗した。
　　End
　Endif
Endif
"
    );
    let stmts = event::parse(&src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");

    // Ask Format 2 のメニューが出ているはず。
    assert!(
        matches!(
            app.pending_dialog(),
            Some(src_core::PendingDialog::Menu { .. })
        ),
        "Ask Format 2 ダイアログが出る"
    );
    // 先頭の選択肢 (リオの乗機) を選ぶ。
    assert!(app.respond_dialog(1));

    // 搭乗メッセージが実パイロット名・実ユニット名で展開されている。
    match app.pending_dialog() {
        Some(src_core::PendingDialog::Talk { body, .. }) => {
            assert_eq!(body, "リオがブレイバーに搭乗した。", "body = {body}");
        }
        other => panic!("expected Talk dialog, got {other:?}"),
    }
}

// ============================================================
//  Organize
// ============================================================

fn run(src: &str) -> App {
    let mut app = App::new();
    let stmts = src_core::data::event::parse(src).expect("parse");
    src_core::event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

#[test]
fn organize_places_off_map_player_units() {
    // Escape → off_map=true。Organize で再配置されると off_map=false になる。
    let app = run(r#"
MapSize 10 10
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit "ブレイバー" リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Place "ブレイバー" "リオ" Player 0 0
Escape リオ
Organize 2 5 5
"#);
    let u = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "ブレイバー")
        .expect("unit exists");
    assert!(!u.off_map, "Organize が off_map を解除する");
}

#[test]
fn organize_respects_count_limit() {
    // 2 体が off_map, count=1 → 1 体だけ再配置される。
    let app = run(r#"
MapSize 10 10
Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Pilot "ガロ" ガロ 男性 一般 AAAA 100 100 100 100 100 100 100
Unit "ブレイバー" リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Unit "ゾルダ" Mass 1 0 陸 5 M 1000 100 2500 110 1000 100 AAAA
Place "ブレイバー" "リオ" Player 0 0
Place "ゾルダ" "ガロ" Player 1 0
Escape リオ
Escape ガロ
Organize 1 5 5
"#);
    let off_count = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| u.off_map)
        .count();
    assert_eq!(off_count, 1, "count=1 なので 1 体は off_map のまま");
}

// ============================================================
//  召喚システム (UseAbility 召喚 / StopSummoning)
// ============================================================

fn count_units(app: &App, name: &str) -> usize {
    app.database()
        .unit_instances
        .iter()
        .filter(|u| u.unit_data_name == name)
        .count()
}

#[test]
fn use_ability_summon_creates_marked_unit() {
    // ブレイバーが ゾルダII を召喚 → ゾルダII が生成され summoned_by に親 uid。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 2 2
UseAbility ブレイバー 召喚 ゾルダII
"#,
    );
    assert_eq!(count_units(&app, "ゾルダII"), 1, "召喚ユニットが生成される");
    let parent_uid = find_unit_named(&app, "ブレイバー").unwrap().uid.clone();
    let summoned = find_unit_named(&app, "ゾルダII").unwrap();
    assert_eq!(summoned.summoned_by.as_deref(), Some(parent_uid.as_str()));
    // 親と同じ陣営
    assert_eq!(
        summoned.party,
        find_unit_named(&app, "ブレイバー").unwrap().party
    );
}

#[test]
fn stop_summoning_removes_summoned_units() {
    // 召喚後 StopSummoning で召喚ユニットが除去され、親は残る。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 2 2
UseAbility ブレイバー 召喚 ゾルダII
StopSummoning ブレイバー
"#,
    );
    assert_eq!(count_units(&app, "ゾルダII"), 0, "召喚ユニットが除去される");
    assert_eq!(count_units(&app, "ブレイバー"), 1, "親は残る");
}

#[test]
fn stop_summoning_only_removes_own_summons() {
    // 別ユニットの召喚分は StopSummoning の対象外。
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 2 2
Place "ゾルダII" "ガロ" Enemy 7 7
UseAbility ブレイバー 召喚 ゾルダII
StopSummoning ガロ
"#,
    );
    // ガロは何も召喚していないので、ブレイバーの召喚分は残る (= ゾルダIIが2体)
    assert_eq!(
        count_units(&app, "ゾルダII"),
        2,
        "他ユニットの召喚解除では除去されない"
    );
}

#[test]
fn summon_at_explicit_coordinates() {
    let app = run_setup(
        r#"
Place "ブレイバー" "リオ" Player 2 2
UseAbility ブレイバー 召喚 ゾルダII 5 6
"#,
    );
    let summoned = find_unit_named(&app, "ゾルダII").unwrap();
    assert_eq!((summoned.x, summoned.y), (5, 6), "指定座標に召喚");
}

// ============================================================
//  Escape: 引数なし (ForEach 連携) / 陣営ラベル
// ============================================================

fn off_map_by_pilot(app: &App, pilot: &str) -> bool {
    app.database()
        .unit_instances
        .iter()
        .find(|u| u.pilot_name == pilot)
        .map(|u| u.off_map)
        .unwrap_or_else(|| panic!("{pilot} が見つからない"))
}

const ESCAPE_SETUP: &str = r#"
Pilot "敵A" 敵A 男性 超能力者 AAAA 100 100 100 100 100 100 100
Pilot "敵B" 敵B 男性 超能力者 AAAA 100 100 100 100 100 100 100
Place "ブレイバー" "リオ" Player 1 1
Place "ゾルダII" "敵A" Enemy 5 5
Place "ゾルダII" "敵B" Enemy 6 6
"#;

#[test]
fn foreach_enemy_escape_retreats_all_enemies() {
    // SRC イディオム `ForEach 敵 / Escape / Next` で全敵を退避 (off_map)。
    // ForEach 書式1 が各反復で SelectedUnitForEvent を設定し、引数なし Escape が
    // それを対象にすることで成立する (決戦！宇宙怪獣1話 の勝利演出 `バルアド撃破`)。
    let app = run_setup(&format!("{ESCAPE_SETUP}ForEach 敵\nEscape\nNext\n"));
    assert!(off_map_by_pilot(&app, "敵A"), "敵A が退避していない");
    assert!(off_map_by_pilot(&app, "敵B"), "敵B が退避していない");
    assert!(!off_map_by_pilot(&app, "リオ"), "味方リオまで退避している");
}

#[test]
fn escape_party_label_retreats_whole_party() {
    // SRC `Escape 敵`: 当該陣営の出撃中ユニットを全退避 (EscapeCmd case 2)。
    let app = run_setup(&format!("{ESCAPE_SETUP}Escape 敵\n"));
    assert!(off_map_by_pilot(&app, "敵A"), "敵A が退避していない");
    assert!(off_map_by_pilot(&app, "敵B"), "敵B が退避していない");
    assert!(!off_map_by_pilot(&app, "リオ"), "味方リオまで退避している");
}

#[test]
fn escape_named_unit_retreats_single() {
    // 回帰: パイロット名指定の Escape は当該 1 体のみ退避 (従来挙動を維持)。
    let app = run_setup(&format!("{ESCAPE_SETUP}Escape 敵A\n"));
    assert!(off_map_by_pilot(&app, "敵A"), "敵A が退避していない");
    assert!(!off_map_by_pilot(&app, "敵B"), "敵B まで退避している");
    assert!(!off_map_by_pilot(&app, "リオ"), "味方リオまで退避している");
}
