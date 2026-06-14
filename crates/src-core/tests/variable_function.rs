//! 変数 / 配列変数のテスト / Variable & array variable behavior.
//!
//! SRC.Sharp `SRCCoreTests/Expressions/VariableTests.cs` を参考に、
//! 配列インデックス操作 / 空文字代入 / Unset の挙動を固定する。
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
//  Indexed (array) variables
// ============================================================

#[test]
fn set_without_value_assigns_one_as_flag() {
    // SRC `SetCmd`: 値なし `Set var` はフラグとして 1 を代入する。
    // `Local var` は宣言のみで空文字。
    let app = run(r#"
Set フラグ
Local ローカル変数
"#);
    assert_eq!(app.script_var("フラグ"), "1", "Set 値なしは 1");
    assert_eq!(app.script_var("ローカル変数"), "", "Local 値なしは空文字");
}

#[test]
fn array_variable_numeric_index() {
    let app = run(r#"
Set arr[0] 99
Set v $(arr[0])
"#);
    assert_eq!(app.script_var("v"), "99");
}

#[test]
fn array_variable_string_index() {
    // `map[キー] = "値"` 形式
    let app = run(r#"
Set map[キー] 値
Set v $(map[キー])
"#);
    assert_eq!(app.script_var("v"), "値");
}

#[test]
fn array_variable_overwrite_element() {
    let app = run(r#"
Set a[1] 10
Set a[1] 20
Set v $(a[1])
"#);
    assert_eq!(app.script_var("v"), "20");
}

#[test]
fn array_variable_unset_removes_element() {
    let app = run(r#"
Set b[2] 5
Set before IsVarDefined(b[2])
Unset b[2]
Set after IsVarDefined(b[2])
"#);
    assert_eq!(app.script_var("before"), "1");
    assert_eq!(app.script_var("after"), "0");
}

#[test]
fn unset_plain_variable_removes_it() {
    let app = run(r#"
Set x 42
Set before IsVarDefined(x)
Unset x
Set after IsVarDefined(x)
"#);
    assert_eq!(app.script_var("before"), "1");
    assert_eq!(app.script_var("after"), "0");
    assert_eq!(app.script_var("x"), "", "削除後の参照は空文字");
}

#[test]
fn array_variable_with_expression_index() {
    // インデックスに変数 / 算術が使えること
    let app = run(r#"
Set i 2
Set xs[2] world
Set v $(xs[i])
"#);
    assert_eq!(app.script_var("v"), "world");
}

// ============================================================
//  Empty string (空代入)
// ============================================================

#[test]
fn set_variable_to_empty_string_is_defined() {
    // SRC.Sharp 仕様: 空文字代入も IsVarDefined = 1
    let app = run(r#"
Set empty ""
Set d IsVarDefined(empty)
"#);
    assert_eq!(app.script_var("d"), "1");
    assert_eq!(app.script_var("empty"), "");
}

#[test]
fn get_undefined_variable_returns_empty() {
    let app = run("Set v $(never_defined)");
    // expand_vars は未定義 → 空文字
    assert_eq!(app.script_var("v"), "");
}

// ============================================================
//  Local & Incr edge cases
// ============================================================

#[test]
fn local_is_alias_of_set() {
    let app = run(r#"
Local x 42
Set v $(x)
"#);
    assert_eq!(app.script_var("v"), "42");
}

#[test]
fn incr_with_default_step_one() {
    let app = run(r#"
Set n 5
Incr n
Incr n
Incr n
"#);
    assert_eq!(app.script_var("n"), "8");
}

#[test]
fn incr_with_explicit_delta() {
    let app = run(r#"
Set n 100
Incr n 25
Incr n -10
"#);
    assert_eq!(app.script_var("n"), "115");
}

#[test]
fn incr_undefined_starts_from_zero() {
    let app = run(r#"
Incr counter
Incr counter
"#);
    assert_eq!(app.script_var("counter"), "2");
}

#[test]
fn incr_non_numeric_delta_is_zero_not_error() {
    // SRC.Sharp IncrCmd は GetArgAsDouble を使い、非数値文字列は 0 扱い
    // (例外を投げない)。実シナリオ (CMaking.eve) は `Incr 仮変数 Mid(名前,i,1)`
    // のように 1 文字を渡してハッシュ計算する。ここでエラーにすると
    // スクリプト全体が異常終了してしまうため、非数値 delta は 0 とする。
    let app = run(r#"
Set h 10
Incr h "あ"
Incr h "("
Incr h X
"#);
    // 非数値 delta は全て 0 加算 → 10 のまま
    assert_eq!(app.script_var("h"), "10");
}

#[test]
fn incr_arithmetic_expression_delta() {
    // delta が算術式なら評価する。
    let app = run(r#"
Set n 0
Incr n (3 + 4)
"#);
    assert_eq!(app.script_var("n"), "7");
}

// ============================================================
//  Set command # comment stripping (backward compat)
// ============================================================

#[test]
fn set_hash_comment_is_stripped() {
    // SRC.Sharp `SetCmd.cs`: `Set var value # comment` の `#` 以降を無視する後方互換機能。
    // 古いシナリオが `Set x 100 # これはコメント` のように書くため。
    let app = run("Set x 100 # これはコメント\n");
    assert_eq!(app.script_var("x"), "100");
}

#[test]
fn set_hash_comment_with_string_value() {
    let app = run(r#"Set name "テスト" # 名前設定"#);
    assert_eq!(app.script_var("name"), "テスト");
}

#[test]
fn set_multi_token_no_comment_unchanged() {
    // `#` がない場合は複数トークンをスペース連結した値になる（既存動作維持）。
    let app = run("Set x hello world\n");
    assert_eq!(app.script_var("x"), "hello world");
}

// ============================================================
//  Numeric to string conversion
// ============================================================

#[test]
fn get_value_as_string_numeric_variable() {
    // `Set n 100` で n=100。$(n) は "100"
    let app = run(r#"
Set n 100
Set v $(n)
"#);
    assert_eq!(app.script_var("v"), "100");
}

#[test]
fn get_value_as_double_string_numeric() {
    // 文字列に格納された数値を算術で取り出せる
    let app = run(r#"
Set s 42
Set v Abs(-$(s))
"#);
    assert_eq!(app.script_var("v"), "42");
}

// ============================================================
//  fn_arg_value: 空文字列変数を正しく返す
// ============================================================

#[test]
fn empty_string_variable_resolves_to_empty() {
    // SRC.Sharp 準拠: `Set v ""` 後に `$(v)` は空文字列を返す (変数名ではない)。
    // ExpressionGetValueIsTermTests.GetValueAsString_EmptyVariableDefinedAsEmpty 準拠
    let app = run(r#"
Set v ""
Set result $(v)
"#);
    assert_eq!(
        app.script_var("result"),
        "",
        "defined empty var should resolve to empty string"
    );
}

#[test]
fn empty_string_var_in_condition_is_falsy() {
    // `Set v ""` → `If v = ""` は真
    let app = run(r#"
Set v ""
If $(v) = "" Then
  Set ok 1
EndIf
"#);
    assert_eq!(app.script_var("ok"), "1");
}

// ============================================================
//  IsVarDefined: `$` プレフィックスを除去して参照
// ============================================================

#[test]
fn incr_on_array_element() {
    // `Incr arr[1] 5` はスクリプト変数 arr[1] をインクリメントできる。
    let app = run(r#"
Set arr[1] 10
Incr arr[1] 5
"#);
    assert_eq!(app.script_var("arr[1]"), "15");
}

#[test]
fn incr_array_element_starting_from_zero() {
    // 未定義 array 要素 (= "") を Incr すると 0 として扱われる。
    let app = run(r#"
Incr counts[x]
Incr counts[x]
Incr counts[x]
"#);
    assert_eq!(app.script_var("counts[x]"), "3");
}

#[test]
fn is_var_defined_with_dollar_prefix() {
    // SRC.Sharp 準拠: `IsVarDefined("$myStr")` は "myStr" を参照する。
    // ExpressionGetValueIsTermTests.IsVariableDefined_WithDollarPrefix 準拠
    let app = run(r#"
Set myStr テスト
Set r IsVarDefined($myStr)
"#);
    assert_eq!(app.script_var("r"), "1");
}

#[test]
fn is_var_defined_empty_string_var_is_defined() {
    // `Set v ""` 後は IsVarDefined("v") = 1 (空文字でも「定義済み」)
    let app = run(r#"
Set v ""
Set r IsVarDefined(v)
"#);
    assert_eq!(app.script_var("r"), "1");
}
