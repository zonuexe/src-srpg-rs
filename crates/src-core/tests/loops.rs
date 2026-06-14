//! ループ (`For`/`Next`/`Do`/`Loop`/`ForEach`/`Break`/`Continue`) の
//! エッジケース。

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
//  For / Next
// ============================================================

#[test]
fn for_basic_sum() {
    let app = run(r#"
Set sum 0
For i = 1 To 5
  Incr sum $(i)
Next
"#);
    assert_eq!(app.script_var("sum"), "15");
}

#[test]
fn for_with_step() {
    let app = run(r#"
Set acc 0
For i = 0 To 10 Step 2
  Incr acc $(i)
Next
"#);
    // 0+2+4+6+8+10 = 30
    assert_eq!(app.script_var("acc"), "30");
}

#[test]
fn for_negative_step() {
    let app = run(r#"
Set log ""
For i = 5 To 1 Step -1
  Set log "$(log)$(i)"
Next
"#);
    assert_eq!(app.script_var("log"), "54321");
}

#[test]
fn for_zero_iterations_when_start_gt_end() {
    let app = run(r#"
Set count 0
For i = 5 To 1
  Incr count
Next
"#);
    // step が省略 (+1) で start > end は 0 iter
    assert_eq!(app.script_var("count"), "0");
}

#[test]
fn for_single_iteration_start_equals_end() {
    let app = run(r#"
Set log ""
For i = 3 To 3
  Set log $(i)
Next
"#);
    assert_eq!(app.script_var("log"), "3");
}

#[test]
fn for_nested() {
    let app = run(r#"
Set count 0
For i = 1 To 3
  For j = 1 To 4
    Incr count
  Next
Next
"#);
    // 3 * 4 = 12
    assert_eq!(app.script_var("count"), "12");
}

#[test]
fn break_in_for_loop() {
    let app = run(r#"
Set last 0
For i = 1 To 100
  If $(i) = 7 Then
    Break
  EndIf
  Set last $(i)
Next
"#);
    // Break で抜けた直前は 6 (i=7 で抜ける)
    assert_eq!(app.script_var("last"), "6");
}

#[test]
fn continue_in_for_loop_skips_iteration() {
    let app = run(r#"
Set sum 0
For i = 1 To 5
  If $(i) = 3 Then
    Continue
  EndIf
  Incr sum $(i)
Next
"#);
    // 1+2+4+5 = 12 (3 skip)
    assert_eq!(app.script_var("sum"), "12");
}

// ============================================================
//  ForEach
// ============================================================

#[test]
fn foreach_iterates_array_keys() {
    let app = run(r#"
Set xs[1] alpha
Set xs[2] beta
Set xs[3] gamma
Set log ""
ForEach i In xs
  Set log "$(log)-$(xs[i])"
Next
"#);
    // keys 順は順序保証されないが、3 つの値が連結されているはず
    let log = app.script_var("log");
    assert!(log.contains("alpha"), "log = {log}");
    assert!(log.contains("beta"), "log = {log}");
    assert!(log.contains("gamma"), "log = {log}");
}

// ============================================================
//  ForEach — 追加パターン
// ============================================================

#[test]
fn foreach_iterates_space_separated_list_variable() {
    // Set した空白区切り変数を ForEach で反復
    let app = run(r#"
Set myList "アリス ボブ キャロル"
Set count 0
ForEach item In myList
  Incr count
Next
"#);
    assert_eq!(app.script_var("count"), "3");
}

#[test]
fn foreach_collects_items_from_space_separated_list() {
    // 各要素を log に追記して内容を検証
    let app = run(r#"
Set myList "a b c"
Set log ""
ForEach item In myList
  Set log "$(log)$(item)"
Next
"#);
    assert_eq!(app.script_var("log"), "abc");
}

#[test]
fn foreach_empty_list_variable_skips_body() {
    // 空のリスト変数ではボディを実行しない
    let app = run(r#"
Set myList ""
Set count 0
ForEach item In myList
  Incr count
Next
"#);
    assert_eq!(app.script_var("count"), "0");
}

// ============================================================
//  Do / Loop
// ============================================================

#[test]
fn do_loop_while_condition() {
    let app = run(r#"
Set i 0
Do
  Incr i
Loop While $(i) < 5
"#);
    assert_eq!(app.script_var("i"), "5");
}

#[test]
fn do_loop_until_condition() {
    let app = run(r#"
Set i 0
Do
  Incr i
Loop Until $(i) >= 3
"#);
    assert_eq!(app.script_var("i"), "3");
}

#[test]
fn continue_in_do_loop() {
    // i=3 のとき Continue でスキップ → count は 4 回インクリメント (i=1,2,4,5)
    let app = run(r#"
Set i 0
Set count 0
Do
  Incr i
  If $(i) = 3 Then
    Continue
  EndIf
  Incr count
Loop While $(i) < 5
"#);
    assert_eq!(app.script_var("count"), "4");
}

#[test]
fn break_in_do_loop() {
    let app = run(r#"
Set i 0
Do
  Incr i
  If $(i) = 4 Then
    Break
  EndIf
Loop While 1
"#);
    assert_eq!(app.script_var("i"), "4");
}

// ============================================================
//  Do While/Until … Loop (条件が先頭)
// ============================================================

#[test]
fn do_while_condition_at_top_executes_body() {
    // 条件が最初から真 → ボディを実行
    let app = run(r#"
Set i 0
Do While $(i) < 3
  Incr i
Loop
"#);
    assert_eq!(app.script_var("i"), "3");
}

#[test]
fn do_while_condition_at_top_skips_body_when_false() {
    // 条件が最初から偽 → ボディを一度も実行しない
    let app = run(r#"
Set i 10
Do While $(i) < 3
  Incr i
Loop
"#);
    assert_eq!(app.script_var("i"), "10");
}

#[test]
fn do_until_condition_at_top_executes_body() {
    // Do Until: 条件が偽の間ループ
    let app = run(r#"
Set i 0
Do Until $(i) >= 4
  Incr i
Loop
"#);
    assert_eq!(app.script_var("i"), "4");
}

#[test]
fn do_until_condition_at_top_skips_body_when_already_true() {
    // 条件がすでに真 → ボディを一度も実行しない
    let app = run(r#"
Set i 10
Do Until $(i) >= 4
  Incr i
Loop
"#);
    assert_eq!(app.script_var("i"), "10");
}

// ============================================================
//  Do/Loop While Call(cond) — ループ条件中の Call() サブ式
//  (§0.1-A: eval_inline_condition_mut が Call() を同期実行で評価)
// ============================================================

/// ライブラリに条件サブルーチンを登録した App を作り、main スクリプトを実行する。
fn run_with_lib(lib_src: &str, main_src: &str) -> App {
    let mut app = App::new();
    let lib = event::parse(lib_src).expect("parse lib");
    app.script_library_mut().append(&lib);
    let main = event::parse(main_src).expect("parse main");
    event_runtime::execute(&mut app, &main).expect("execute");
    app
}

#[test]
fn do_while_call_condition_loops() {
    // `Do While Call(should_continue)`: サブルーチンが Return 1 を返す間ループ。
    // 共有 script_vars 経由で $(i) を参照し、i >= 3 で Return 0 → ループ終了。
    let app = run_with_lib(
        r#"
should_continue:
If $(i) < 3 Then
  Return 1
EndIf
Return 0
"#,
        r#"
Set i 0
Do While Call(should_continue)
  Incr i
Loop
"#,
    );
    assert_eq!(app.script_var("i"), "3");
}

#[test]
fn loop_while_call_condition_loops() {
    // `Loop While Call(should_continue)`: 末尾条件版。少なくとも 1 回は実行。
    let app = run_with_lib(
        r#"
should_continue:
If $(i) < 4 Then
  Return 1
EndIf
Return 0
"#,
        r#"
Set i 0
Do
  Incr i
Loop While Call(should_continue)
"#,
    );
    // i=1,2,3,4 と進み、i=4 で Call が 0 を返してループ終了。
    assert_eq!(app.script_var("i"), "4");
}

#[test]
fn do_while_call_condition_false_skips_body() {
    // 先頭条件が最初から偽 (Return 0) → ボディを一度も実行しない。
    let app = run_with_lib(
        r#"
never:
Return 0
"#,
        r#"
Set i 10
Do While Call(never)
  Incr i
Loop
"#,
    );
    assert_eq!(app.script_var("i"), "10");
}

#[test]
fn loop_until_call_condition_loops() {
    // `Loop Until Call(done)`: Call が 1 (真) を返したらループ終了。
    let app = run_with_lib(
        r#"
done:
If $(i) >= 3 Then
  Return 1
EndIf
Return 0
"#,
        r#"
Set i 0
Do
  Incr i
Loop Until Call(done)
"#,
    );
    assert_eq!(app.script_var("i"), "3");
}

// ============================================================
//  For loop variable post-loop value (SRC.Sharp 準拠)
// ============================================================

#[test]
fn for_loop_var_is_end_plus_step_after_loop() {
    // SRC.Sharp `NextCmd.cs` 準拠: Next は必ず変数を更新してから終了判定。
    // `For i = 1 To 5` → 終了後 i = 5 + 1 = 6
    let app = run(r#"
For i = 1 To 5
Next
"#);
    assert_eq!(app.script_var("i"), "6");
}

#[test]
fn for_loop_negative_step_var_ends_at_end_plus_step() {
    // `For i = 3 To 1 Step -1` → 終了後 i = 1 + (-1) = 0
    // `ControlCmdMoreTests.ForCmd_NegativeStep_VariableDecrements` 準拠
    let app = run(r#"
Set count 0
For i = 3 To 1 Step -1
  Incr count
Next
"#);
    assert_eq!(app.script_var("count"), "3");
    assert_eq!(app.script_var("i"), "0");
}

// ============================================================
//  ForEach ユニット一覧 / パイロット一覧 (書式3/4)
// ============================================================

fn run_with_two_units(src: &str) -> App {
    let setup = "\
Pilot \"リオ\" リオ 男性 超能力者 AAAA 100 100 120 100 100 100 100
Pilot \"ガロ\" ガロ 男性 一般 BBBC 50 100 100 100 100 100 100
Unit \"ブレイバー\" リアル系 1 0 陸 5 M 3000 400 3500 120 1200 110 AAAA
Unit \"ゾルダII\" リアル系 1 0 陸 5 M 2000 300 2400 100 900 80 BBCC
MapSize 5 5
Place \"ブレイバー\" \"リオ\" Player 1 1
Place \"ゾルダII\" \"ガロ\" Enemy 3 3
";
    let full = format!("{setup}{src}");
    let mut app = App::new();
    let stmts = event::parse(&full).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

#[test]
fn foreach_unit_roster_iterates_all_deployed_units() {
    // `ForEach u In ユニット一覧(出撃)` は出撃中ユニット名を列挙する。
    let app = run_with_two_units(
        "Set units \"\"\nForEach u In ユニット一覧(出撃)\n  Set units \"$(units) $(u)\"\nNext\n",
    );
    let units = app.script_var("units");
    assert!(units.contains("ブレイバー"), "units={units}");
    assert!(units.contains("ゾルダII"), "units={units}");
}

#[test]
fn foreach_pilot_roster_iterates_all_pilots() {
    // `ForEach p In パイロット一覧(登録順)` は配置中パイロット名を列挙する。
    let app = run_with_two_units(
        "Set pilots \"\"\nForEach p In パイロット一覧(登録順)\n  Set pilots \"$(pilots) $(p)\"\nNext\n",
    );
    let pilots = app.script_var("pilots");
    assert!(pilots.contains("リオ"), "pilots={pilots}");
    assert!(pilots.contains("ガロ"), "pilots={pilots}");
}

#[test]
fn foreach_pilot_roster_by_level_orders_descending() {
    // `ForEach p In パイロット一覧(レベル)` はレベル降順。
    // リオ (total_exp=200) → level 3, ガロ (total_exp=0) → level 1。
    let app = run_with_two_units(
        "ExpUp ブレイバー 200\nSet first \"\"\nForEach p In パイロット一覧(レベル)\n  If $(first) = \"\" Then\n    Set first $(p)\n  EndIf\nNext\n",
    );
    assert_eq!(
        app.script_var("first"),
        "リオ",
        "レベルが高い方が先頭のはず"
    );
}

#[test]
fn foreach_group_form_counts_player_units() {
    // 書式1 `ForEach 味方` は Player 陣営のユニットを反復し、
    // `対象パイロット` にパイロット名を束縛する。
    let app = run_with_two_units("Set cnt 0\nForEach 味方\n  Incr cnt\nNext\n");
    assert_eq!(app.script_var("cnt"), "1");
}

#[test]
fn foreach_group_form_counts_enemy_units() {
    // 書式1 `ForEach 敵` は Enemy 陣営のユニットを反復する。
    let app = run_with_two_units("Set cnt 0\nForEach 敵\n  Incr cnt\nNext\n");
    assert_eq!(app.script_var("cnt"), "1");
}

#[test]
fn foreach_group_form_binds_target_pilot_each_iter() {
    // 書式1 `ForEach 味方` の各反復で `対象パイロット` が正しく束縛される。
    let app =
        run_with_two_units("Set last \"\"\nForEach 味方\n  Set last $(対象パイロット)\nNext\n");
    assert_eq!(app.script_var("last"), "リオ");
}

#[test]
fn foreach_unit_roster_by_level_orders_descending() {
    // `ForEach u In ユニット一覧(レベル)` は経験値降順。
    // ブレイバーに ExpUp → level 高い → 先頭に来る。
    let app = run_with_two_units(
        "ExpUp ブレイバー 200\nSet first \"\"\nForEach u In ユニット一覧(レベル)\n  If $(first) = \"\" Then\n    Set first $(u)\n  EndIf\nNext\n",
    );
    assert_eq!(
        app.script_var("first"),
        "ブレイバー",
        "ユニット一覧(レベル) で高レベルユニットが先頭"
    );
}

#[test]
fn foreach_unit_roster_empty_when_no_units() {
    // ユニットが配置されていなければ ユニット一覧 は空 → 反復 0 回。
    let mut app = App::new();
    let stmts =
        event::parse("Set cnt 0\nForEach u In ユニット一覧(出撃)\n  Incr cnt\nNext\n").unwrap();
    event_runtime::execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("cnt"), "0");
}

#[test]
fn foreach_all_iterates_every_unit() {
    // `ForEach u In 全` は全陣営のユニットを列挙。
    let app = run_with_two_units("Set cnt 0\nForEach u In 全\n  Incr cnt\nNext\n");
    assert_eq!(app.script_var("cnt"), "2");
}

#[test]
fn break_in_foreach_exits_early() {
    // ForEach でリストの途中に Break するとループを抜ける。
    let app = run(r#"
Set mylist 1 2 3 4 5
Set cnt 0
ForEach item In mylist
  If $(item) = 3 Then
    Break
  EndIf
  Incr cnt
Next
"#);
    // item=3 で Break → cnt = 2 (1 と 2 の 2 回)
    assert_eq!(app.script_var("cnt"), "2");
}

#[test]
fn continue_in_foreach_skips_current_item() {
    // ForEach で特定要素を Continue でスキップする。
    let app = run(r#"
Set mylist 1 2 3 4 5
Set sum 0
ForEach item In mylist
  If $(item) = 3 Then
    Continue
  EndIf
  Incr sum $(item)
Next
"#);
    // 1+2+4+5 = 12 (3 skip)
    assert_eq!(app.script_var("sum"), "12");
}
