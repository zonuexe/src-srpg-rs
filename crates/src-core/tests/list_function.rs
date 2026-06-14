//! List 関数 (`List` / `LIndex` / `LLength` / `LSearch`) のテスト。
//!
//! SRC.Sharp `SRCCoreTests/Expressions/ListFunctionTests.cs` を参考に
//! 25 ケースのエッジを網羅。実シナリオで Lindex 3628 回 / List 2627 回 /
//! Llength 260 回呼ばれる頻出関数なので、SRC 原典挙動と完全に合わせる。
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
//  List
// ============================================================

#[test]
fn list_single_arg() {
    let app = run(r#"Set v List("abc")"#);
    assert_eq!(app.script_var("v"), "abc");
}

#[test]
fn list_multiple_args_space_separated() {
    let app = run(r#"Set v List("a","b","c")"#);
    assert_eq!(app.script_var("v"), "a b c");
}

#[test]
fn list_numeric_args() {
    let app = run("Set v List(1,2,3)");
    assert_eq!(app.script_var("v"), "1 2 3");
}

#[test]
fn list_four_args() {
    let app = run("Set v List(1,2,3,4)");
    assert_eq!(app.script_var("v"), "1 2 3 4");
}

// ============================================================
//  LLength
// ============================================================

#[test]
fn llength_space_separated() {
    let app = run(r#"Set v Llength("a b c")"#);
    assert_eq!(app.script_var("v"), "3");
}

#[test]
fn llength_single_element() {
    let app = run(r#"Set v Llength("abc")"#);
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn llength_empty_string() {
    let app = run(r#"Set v Llength("")"#);
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn llength_two_elements() {
    let app = run(r#"Set v Llength("x y")"#);
    assert_eq!(app.script_var("v"), "2");
}

#[test]
fn llength_japanese_list() {
    let app = run(r#"Set v Llength("あ い う")"#);
    assert_eq!(app.script_var("v"), "3");
}

// ============================================================
//  LIndex
// ============================================================

#[test]
fn lindex_first_element() {
    let app = run(r#"Set v Lindex("a b c",1)"#);
    assert_eq!(app.script_var("v"), "a");
}

#[test]
fn lindex_second_element() {
    let app = run(r#"Set v Lindex("a b c",2)"#);
    assert_eq!(app.script_var("v"), "b");
}

#[test]
fn lindex_last_element() {
    let app = run(r#"Set v Lindex("a b c",3)"#);
    assert_eq!(app.script_var("v"), "c");
}

#[test]
fn lindex_out_of_bounds_returns_empty() {
    let app = run(r#"Set v Lindex("a b c",5)"#);
    assert_eq!(app.script_var("v"), "");
}

#[test]
fn lindex_index_zero_returns_empty() {
    let app = run(r#"Set v Lindex("a b c",0)"#);
    assert_eq!(app.script_var("v"), "");
}

#[test]
fn lindex_japanese_list() {
    let app = run(r#"Set v Lindex("りんご みかん ぶどう",2)"#);
    assert_eq!(app.script_var("v"), "みかん");
}

#[test]
fn lindex_numeric_list() {
    let app = run(r#"Set v Lindex("10 20 30",1)"#);
    assert_eq!(app.script_var("v"), "10");
}

// ============================================================
//  LSearch
// ============================================================

#[test]
fn lsearch_found_returns_position() {
    let app = run(r#"Set v Lsearch("a b c","b")"#);
    assert_eq!(app.script_var("v"), "2");
}

#[test]
fn lsearch_first_element_returns_one() {
    let app = run(r#"Set v Lsearch("a b c","a")"#);
    assert_eq!(app.script_var("v"), "1");
}

#[test]
fn lsearch_last_element() {
    let app = run(r#"Set v Lsearch("a b c","c")"#);
    assert_eq!(app.script_var("v"), "3");
}

#[test]
fn lsearch_not_found_returns_zero() {
    // SRC.Sharp 仕様: not found は 0 (length+1 ではない)
    let app = run(r#"Set v Lsearch("a b c","z")"#);
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn lsearch_empty_list_returns_zero() {
    let app = run(r#"Set v Lsearch("","a")"#);
    assert_eq!(app.script_var("v"), "0");
}

#[test]
fn lsearch_with_start_position() {
    // "a b a c" で a を 2 番目以降から検索 → 3 番目の a が返る
    let app = run(r#"Set v Lsearch("a b a c","a",2)"#);
    assert_eq!(app.script_var("v"), "3");
}

#[test]
fn lsearch_japanese_list() {
    let app = run(r#"Set v Lsearch("あ い う","い")"#);
    assert_eq!(app.script_var("v"), "2");
}

// ============================================================
//  Combined patterns (実シナリオでよく使われる)
// ============================================================

#[test]
fn pattern_lindex_of_list_inline() {
    // `Lindex(List(a,b,c),2)` のような関数のネスト
    let app = run(r#"Set v Lindex(List(alpha,beta,gamma),2)"#);
    assert_eq!(app.script_var("v"), "beta");
}

#[test]
fn pattern_llength_of_list() {
    let app = run(r#"Set v Llength(List(a,b,c,d,e))"#);
    assert_eq!(app.script_var("v"), "5");
}

// ============================================================
//  LIndex の追加エッジケース (SRC.Sharp ListFunctionMoreTests)
// ============================================================

#[test]
fn lindex_negative_index_returns_empty() {
    // SRC.Sharp: 負数 index は empty (LIndex_NegativeIndex_ReturnsEmpty)
    let app = run(r#"Set v Lindex("a b c",-1)"#);
    assert_eq!(app.script_var("v"), "");
}

#[test]
fn lindex_parenthesized_element_strips_outer_parens() {
    // SRC.Sharp Functions/List.cs::LIndex (line 52-55): 要素が
    // `(...)` で囲まれていれば外側 paren を 1 段剥がす
    let app = run(r#"Set v Lindex("(alpha) (beta) (gamma)",2)"#);
    assert_eq!(app.script_var("v"), "beta");
}

#[test]
fn lindex_no_paren_unchanged() {
    let app = run(r#"Set v Lindex("alpha beta gamma",2)"#);
    assert_eq!(app.script_var("v"), "beta");
}

#[test]
fn lindex_partial_paren_unchanged() {
    // 片方だけ paren の場合は剥がさない
    let app = run(r#"Set v Lindex("(alpha beta) gamma",1)"#);
    // 空白で split されるので要素 1 = "(alpha"
    // → SRC.Sharp も同様 (片方だけは剥がさない)
    let v = app.script_var("v");
    assert!(v.starts_with('('), "片方だけの paren は剥がさない: {v}");
}

#[test]
fn lsearch_duplicate_elements_returns_first() {
    // 重複要素は最初の出現位置を返す (start 引数省略時)
    let app = run(r#"Set v Lsearch("a b a c","a")"#);
    assert_eq!(app.script_var("v"), "1");
}
