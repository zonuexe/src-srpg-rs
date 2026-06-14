//! Sort コマンドの edge cases。
//! `Sort array_name [昇順|降順]`

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn run(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

fn read_array(app: &App, name: &str, n: usize) -> Vec<String> {
    (1..=n)
        .map(|i| app.script_var(&format!("{name}[{i}]")).to_string())
        .collect()
}

#[test]
fn sort_numeric_ascending_by_default() {
    let app = run(r#"
Set xs[1] 30
Set xs[2] 10
Set xs[3] 20
Sort xs
"#);
    let v = read_array(&app, "xs", 3);
    assert_eq!(
        v,
        vec!["10".to_string(), "20".to_string(), "30".to_string()]
    );
}

#[test]
fn sort_numeric_descending() {
    // SRC.Sharp 準拠: 数値インデックス配列の降順ソートは
    // キーを降順、値も降順に並べて zip するため、
    // 結果として「最小キー = 最小値」となり昇順と同一になる。
    // (`SortCmdTests.SortDescendingNumericTest` を参照)
    let app = run(r#"
Set xs[1] 30
Set xs[2] 10
Set xs[3] 20
Sort xs 降順
"#);
    let v = read_array(&app, "xs", 3);
    // 降順ソート結果は昇順と同じ: xs[1]=10, xs[2]=20, xs[3]=30
    assert_eq!(
        v,
        vec!["10".to_string(), "20".to_string(), "30".to_string()]
    );
}

#[test]
fn sort_string_ascending() {
    let app = run(r#"
Set xs[1] charlie
Set xs[2] alpha
Set xs[3] bravo
Sort xs
"#);
    let v = read_array(&app, "xs", 3);
    assert_eq!(v[0], "alpha");
    assert_eq!(v[1], "bravo");
    assert_eq!(v[2], "charlie");
}

#[test]
fn sort_empty_array_is_noop() {
    let app = run("Sort xs\n");
    // 何もしないので変数も生成されない
    let count = app
        .script_vars()
        .iter()
        .filter(|(k, _)| k.starts_with("xs["))
        .count();
    assert_eq!(count, 0);
}

#[test]
fn sort_single_element_unchanged() {
    let app = run(r#"
Set xs[1] alone
Sort xs
"#);
    assert_eq!(app.script_var("xs[1]"), "alone");
}

#[test]
fn sort_mixed_numeric_string_falls_back_to_string_cmp() {
    // 値の一部が数値変換できない場合、文字列比較にフォールバック
    let app = run(r#"
Set xs[1] 30
Set xs[2] abc
Set xs[3] 10
Sort xs
"#);
    let v = read_array(&app, "xs", 3);
    // 文字列比較: "10" < "30" < "abc"
    assert_eq!(v[0], "10");
}

#[test]
fn sort_explicit_ascending_keyword() {
    let app = run(r#"
Set ys[1] 5
Set ys[2] 1
Sort ys 昇順
"#);
    assert_eq!(app.script_var("ys[1]"), "1");
    assert_eq!(app.script_var("ys[2]"), "5");
}

// ============================================================
//  インデックスのみ (key-only sort)
// ============================================================

#[test]
fn sort_index_only_pairs_stay_together() {
    // `Sort arr 昇順 インデックスのみ` — キーと値のペアを保ったまま
    // キーの昇順に並べ直す。値はキーと一緒に移動する。
    // `SortCmdTests.SortKeyOnlyTest` 準拠
    let app = run(r#"
Set arr[3] 100
Set arr[1] 200
Set arr[2] 300
Sort arr 昇順 インデックスのみ
"#);
    // ペアが一緒に動くので arr[1]=200, arr[2]=300, arr[3]=100
    assert_eq!(app.script_var("arr[1]"), "200");
    assert_eq!(app.script_var("arr[2]"), "300");
    assert_eq!(app.script_var("arr[3]"), "100");
}

#[test]
fn sort_index_only_descending_pairs_stay_together() {
    // `Sort d 降順 インデックスのみ` — キー降順でペアを保持して並べ直す。
    // 既に自然順にある場合は変化なし。`SortCmdTests.SortKeyOnly_DescendingTest` 準拠。
    let app = run(r#"
Set d[1] 10
Set d[2] 20
Set d[3] 30
Sort d 降順 インデックスのみ
"#);
    // キー降順でペアを保持 → d[3]=30, d[2]=20, d[1]=10 (結果変わらず)
    assert_eq!(app.script_var("d[1]"), "10");
    assert_eq!(app.script_var("d[2]"), "20");
    assert_eq!(app.script_var("d[3]"), "30");
}

// ============================================================
//  文字ソート (string sort)
// ============================================================

#[test]
fn sort_moji_ascending_string_values() {
    // `Sort arr 昇順 文字` — 値を文字列比較で昇順に
    // `SortCmdTests.SortAscendingStringValueTest` 準拠
    let app = run(r#"
Set arr[1] Charlie
Set arr[2] Alice
Set arr[3] Bob
Sort arr 昇順 文字
"#);
    assert_eq!(app.script_var("arr[1]"), "Alice");
    assert_eq!(app.script_var("arr[2]"), "Bob");
    assert_eq!(app.script_var("arr[3]"), "Charlie");
}

// ============================================================
//  無効オプションはエラー
// ============================================================

#[test]
fn sort_invalid_option_returns_error() {
    // `Sort arr 不正なオプション` → エラーで実行停止
    let mut app = src_core::App::new();
    let stmts = src_core::data::event::parse("Set arr[1] 1\nSort arr 不正なオプション\n").unwrap();
    let result = src_core::event_runtime::execute(&mut app, &stmts);
    assert!(result.is_err(), "無効オプションはエラーであるべき");
}
