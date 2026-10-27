//! `.eve` の制御構文 (If / Switch / For / Do / ForEach / Goto / Call / ラベル) のテスト。

use super::*;

// ───────────── ユーザ定義関数 (ラベルの関数呼び出し) ─────────────

/// 原典 SRC は式中の未知の関数名をラベルとして解決し、`Return <value>` の
/// 値を返す (`Expression.function.cs` / `Returnコマンド.md`)。
#[test]
fn user_function_called_by_label_name() {
    let src = "\
Set a 足す2(5)
Stage done
Exit

足す2:
Local n = Args(1)
Return (n + 2)
";
    let mut app = App::new();
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("a"), "7");
}

/// 日本語名のユーザ定義関数を条件式で使える。
#[test]
fn user_function_in_condition() {
    let src = "\
If シールド判定(3) = 1 Then
  Set d ok
EndIf
Stage done
Exit

シールド判定:
Return 1
";
    let mut app = App::new();
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("d"), "ok");
}

/// 引数の内側のユーザ定義関数も解決される (入れ子)。
#[test]
fn user_function_nested_arguments() {
    let src = r#"Set e 包む(足す2(1))
Stage done
Exit

足す2:
Return (Args(1) + 2)

包む:
Return ("[" & Args(1) & "]")
"#;
    let mut app = App::new();
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("e"), "[3]");
}

/// 括弧を含む **ユニット名** をユーザ定義関数と誤認しない。
/// 原典 `Expression.function.cs` も同じチェックを行う。公式サンプルの
/// `Upgrade キャリバーン キャリバーン(不完全)` が該当する。
#[test]
fn defined_unit_name_with_parens_is_not_a_user_function() {
    let mut app = App::new();
    let (units, _) = crate::data::unit::parse_lenient(
        "キャリバーン(不完全)\n\
キャリバーン, ＭＳ, 1, 2\n\
陸, 4, M, 1000, 100\n",
    );
    app.database_mut().extend_units(units);
    let src = r#"Set u キャリバーン(不完全)
Stage done
Exit

キャリバーン:
Return ""
"#;
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("u"), "キャリバーン(不完全)");
}

/// `Args` は 10 個以上も扱える。原典の引数配列は `ReDim` の動的配列で
/// 個数制限が無い (`CmdData.cls`)。旧実装は `Args(1..9)` 固定だった。
#[test]
fn call_supports_more_than_nine_arguments() {
    let src = r#"Call 集める a b c d e f g h i j k l
Stage done
Exit

集める:
Set n10 $(Args(10))
Set n12 $(Args(12))
Set cnt $(ArgNum)
Return
"#;
    let mut app = App::new();
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("cnt"), "12");
    assert_eq!(app.script_var("n10"), "j");
    assert_eq!(app.script_var("n12"), "l");
}

/// ネストした `Call` の後も、呼び出し元の 10 個目以降の `Args` が戻る。
#[test]
fn nested_call_restores_high_numbered_args() {
    let src = r#"Call 外 a b c d e f g h i j k l
Stage done
Exit

外:
Call 内 x y
Set 外10 $(Args(10))
Set 外num $(ArgNum)
Return

内:
Set 内num $(ArgNum)
Return
"#;
    let mut app = App::new();
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("内num"), "2");
    assert_eq!(app.script_var("外num"), "12", "呼び出し元の ArgNum が復元");
    assert_eq!(app.script_var("外10"), "j", "10 個目以降も復元される");
}

/// `Local` で宣言した変数はサブルーチンを抜けると宣言前の値へ戻る。
/// 原典は `VarStack` + `VarIndex` の復帰で自動的に破棄する。
#[test]
fn sublocal_variables_are_restored_after_return() {
    let src = r#"Set n 外側
Local i = 99
Set r 足す2(7)
Stage done
Exit

足す2:
Local n = Args(1)
Local i = 1
Return (n + 2)
"#;
    let mut app = App::new();
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("r"), "9", "返り値は callee のスコープで評価");
    assert_eq!(app.script_var("n"), "外側", "呼び出し元の n が復元される");
    assert_eq!(app.script_var("i"), "99", "呼び出し元の i が復元される");
}

/// 宣言前に未定義だった変数はサブルーチン離脱時に削除される。
#[test]
fn sublocal_undefined_before_call_is_removed() {
    let src = r#"Set r 作る()
Stage done
Exit

作る:
Local 一時 = 5
Return 1
"#;
    let mut app = App::new();
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("r"), "1");
    assert!(
        !app.is_script_var_defined("一時"),
        "サブルーチンローカル変数が漏れている"
    );
}

#[test]
fn execute_basic_script() {
    let src = "\
Stage \"序章 — 起動\"
MapSize 4 3
SetTile 1 1 2
Pilot \"リオ・カザミ\" リオ 男性 超能力者 AAAA 100 160 220 200 220 240 200
Unit \"ブレイバー\" リアル系 1 4 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Weapon \"ブレイバー\" \"ビームライフル\" 2500 2 5 15 -1
Place \"ブレイバー\" \"リオ・カザミ\" Player 0 0
Turn 1
Message \"出撃せよ\"
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();

    assert_eq!(app.stage(), "序章 — 起動");
    let map = app.database().map.as_ref().unwrap();
    assert_eq!(map.width, 4);
    assert_eq!(map.height, 3);
    assert_eq!(map.cell(1, 1).terrain_id, 2);
    assert_eq!(app.database().catalog.pilots.len(), 1);
    assert_eq!(app.database().catalog.units.len(), 1);
    assert_eq!(app.database().catalog.units[0].weapons.len(), 1);
    assert_eq!(app.database().unit_instances.len(), 1);
    assert_eq!(app.turn().number, 1);
    assert_eq!(app.messages(), &["出撃せよ".to_string()]);
}

#[test]
fn unknown_command_is_ignored() {
    let src = "ThisCommandDoesNotExist 1 2 3\nStage \"x\"\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.stage(), "x");
}

#[test]
fn invalid_args_returns_error() {
    let src = "MapSize abc 3\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    let e = execute(&mut app, &stmts).unwrap_err();
    assert_eq!(e.line_num, 1);
}

#[test]
fn if_true_executes_then_branch() {
    let src = "\
Set x 1
If $(x) = 1 Then
  Message yes
Else
  Message no
EndIf
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages(), &["yes".to_string()]);
}

#[test]
fn if_false_falls_through_else() {
    let src = "\
Set x 0
If $(x) = 1 Then
  Message yes
Else
  Message no
EndIf
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages(), &["no".to_string()]);
}

#[test]
fn elseif_chain_picks_matching_branch() {
    let src = "\
Set x 2
If $(x) = 1 Then
  Message a
ElseIf $(x) = 2 Then
  Message b
ElseIf $(x) = 3 Then
  Message c
Else
  Message d
EndIf
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages(), &["b".to_string()]);
}

#[test]
fn goto_jumps_to_label() {
    let src = "\
Goto end:
Message skipped
end:
Message arrived
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages(), &["arrived".to_string()]);
}

#[test]
fn block_if_skip_ignores_inner_single_line_if_goto() {
    // スパロボ戦記 AlphaSecond L67: ブロック If の中に単一行 `If cond Goto label`
    // があると、EndIf スキップの深さ計数が単一行 If を 1 段と誤数えして対応する
    // EndIf を見失い「If に対応する EndIf が見つかりません」になる回帰の防止。
    let src = "\
Set フラグ 0
If フラグ = 1 Then
  If フラグ = 9 Goto どこか
  Set 中身 実行
EndIf
Set 後 到達
どこか:
Exit
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // フラグ=0 なのでブロック If は偽 → EndIf を正しく見つけて後続へ。
    assert_eq!(
        app.script_var("後"),
        "到達",
        "ブロック If の EndIf を見失っている"
    );
    assert_eq!(
        app.script_var("中身"),
        "",
        "偽のブロック If 本体が実行された"
    );
    assert!(
        app.last_script_error().is_none(),
        "EndIf 探索エラー: {:?}",
        app.last_script_error()
    );
}

#[test]
fn anchor_resolves_for_goto() {
    let src = "\
Goto onsen
Message skipped
@onsen
Message arrived
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages(), &["arrived".to_string()]);
}

#[test]
fn if_instr_with_bracket_literal_matches() {
    // 回帰候補: `If Instr(v, "設定[パイロット一覧]")` のように、クオート内に [..] を含む
    // リテラルを条件で使ったとき正しくマッチするか (D データロードの行検出で使用)。
    let mut app = App::new();
    app.set_script_var(
        "v".to_string(),
        "Set 設定[パイロット一覧] 人工知能(ザコ) ".to_string(),
    );
    let src = "If Instr(v, \"設定[パイロット一覧]\") Then\nSet r found\nEndif\n";
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    assert_eq!(app.script_var("r"), "found");
}

#[test]
fn isdead_predicate_in_if() {
    let src = "\
MapSize 4 4
Pilot \"P\" P 男性 c AAAA 10 100 100 100 100 100 100
Unit \"U\" リアル系 1 0 陸 5 M 100 50 1000 80 900 80 AAAA
Place \"U\" \"P\" Enemy 1 1
If IsAlive U Then
  Message still_there
EndIf
Kill U
If IsDead U Then
  Message gone
EndIf
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(
        app.messages(),
        &["still_there".to_string(), "gone".to_string()]
    );
}

#[test]
fn foreach_iterates_indexed_var_keys() {
    let src = "\
Set xs[1] a
Set xs[2] b
Set xs[3] c
Set joined empty
ForEach i In xs
  Set joined $(joined)_$(xs[i])
Next
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("joined"), "empty_a_b_c");
}

#[test]
fn single_line_if_with_paren_cond_executes_body() {
    // Score.eve 由来の形式: `If (cond) Exit` 一行で body を持つ If。
    // 条件が真なら body が実行され、後続行は飛ばされる。
    let src = "\
Set x 1
If ($(x) = 1) Goto skipme
Set never_set 1
skipme:
Stage done
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("never_set"), "");
    assert_eq!(app.stage(), "done");
}

#[test]
fn single_line_if_with_then_keyword_executes_body() {
    // `If a = b Then stmt` 単一行形式（Then 後に body）。
    let src = "\
Set x 2
If $(x) = 2 Then Stage matched
Stage default
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // 単一行 If により Stage matched が即座に実行され、その後 Stage default
    // で上書きされる（多行 If と異なり EndIf を持たないので、結果は最後の Stage）。
    assert_eq!(app.stage(), "default");
}

#[test]
fn single_line_if_skips_body_when_false() {
    let src = "\
Set x 0
If ($(x) = 1) Stage matched
Stage fallback
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.stage(), "fallback");
}

#[test]
fn single_line_if_with_function_call_condition() {
    // `If Func(...) stmt` — 条件が 1 個の関数呼び出しトークン、Then 無し。
    // スパロボ戦記 Include.eve L3233 `If Instr(...) Exit` が該当。
    // ブロック If と誤認して EndIf を探すと「EndIf が見つかりません」で
    // 異常終了するため、単一行 If として正しく分割すること。
    let src = "\
Set a no
Set b no
If Instr(\"abc\",\"b\") Set a yes
If Instr(\"abc\",\"z\") Set b yes
Stage done
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("a"), "yes", "Instr=2 (真) → body 実行");
    assert_eq!(app.script_var("b"), "no", "Instr=0 (偽) → body skip");
    assert_eq!(app.stage(), "done");
}

#[test]
fn single_line_if_with_bare_token_condition() {
    // `If <expr> stmt` — 条件が 1 トークン (関数でも括弧でもない)、Then 無し。
    // スパロボ戦記 Include.eve L1360 `If エピローグカット Exit` が該当。
    let src = "\
Set flag 1
Set off 0
If $(flag) Stage A
If $(off) Stage B
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // $(flag)=1 → 真 → Stage A。$(off)=0 → 偽 → Stage B skip。
    assert_eq!(app.stage(), "A");
}

#[test]
fn condition_resolves_bare_identifier_as_variable() {
    // SRC: `If 選択 = ランダム` のように `$()` を付けない条件比較は、裸の
    // 識別子を変数 (script_var) として自動解決する。リテラル値の場合は
    // そのまま比較。
    let src = "\
Set 選択 ランダム
If 選択 = ランダム Then
　Stage matched
Else
　Stage missed
EndIf
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.stage(), "matched");
}

#[test]
fn call_can_target_label_in_previously_loaded_eve() {
    // 別 .eve に定義されたラベルへも Call/Goto がジャンプできること。
    // 複数 .eve 読込時、過去にロードした script_library 内のラベルを
    // 解決できることを担保。スパロボ戦記の
    //   Require \"eve\\EventBattle.eve\"
    //   Call \"イベントバトル\"
    // パターンが動くために必要。
    let mut app = App::new();
    // 1 つ目: イベントバトル: ラベル + Stage を設定 + Return
    let src1 = "\
イベントバトル:
Stage event_called
Return
";
    let stmts1 = event::parse(src1).unwrap();
    execute(&mut app, &stmts1).unwrap();
    // 1 つ目の execute は Return で抜けるだけ (top-level に到達しない)
    // 2 つ目: top-level から Call イベントバトル
    let src2 = "\
Call イベントバトル
Set 完了 ok
";
    let stmts2 = event::parse(src2).unwrap();
    execute(&mut app, &stmts2).unwrap();
    // Call が別 .eve のラベルにジャンプ → Stage event_called → Return →
    // 2 つ目の continuation で Set 完了 ok
    assert_eq!(app.stage(), "event_called");
    assert_eq!(app.script_var("完了"), "ok");
}

#[test]
fn single_line_if_with_not_prefix_and_inline_body() {
    // `If Not 選択 = 1 Goto X` — Then 省略 + Not 接頭辞 + inline body。
    let src = "\
Set 選択 2
If Not 選択 = 1 Goto skipme
Set never 1
skipme:
Stage hit
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("never"), "");
    assert_eq!(app.stage(), "hit");
}

#[test]
fn switch_value_resolves_bare_identifier_as_variable() {
    // SRC `Switch 選択` — 単一識別子の value は script_var として
    // 自動解決する。BattleSelect.eve の `Switch 選択 / Case イベントバトル`
    // パターンが動くこと。
    let src = "\
Set 選択 イベントバトル
Switch 選択
Case フリーバトル
　Stage free
Case イベントバトル
　Stage event
Case Else
　Stage other
EndSw
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.stage(), "event");
}

#[test]
fn asterisk_label_can_be_triggered() {
    // SRC `*ラベル:` 形式 (イベントハンドラ用) もラベルテーブルに登録され、
    // `trigger_label("ラベル")` で発火する。
    let src = "\
*ハンドラ:
Stage fired
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    // 直接 execute せずライブラリ登録だけ走らせる
    app.script_library_mut().append(&stmts);
    assert!(trigger_label(&mut app, "ハンドラ"));
    assert_eq!(app.stage(), "fired");
}

#[test]
fn call_intermission_command_data_save_writes_quicksave() {
    // `.eve CallIntermissionCommand データセーブ` はメニュー経路と同じ実体に
    // 委譲し、現在状態を `__quicksave` script_var へ書き込む。
    let mut app = App::new();
    assert!(app.script_var("__quicksave").is_empty());
    let stmts = event::parse("CallIntermissionCommand データセーブ\n").unwrap();
    execute(&mut app, &stmts).unwrap();
    assert!(
        !app.script_var("__quicksave").is_empty(),
        "データセーブで __quicksave が書かれるべき"
    );
}

#[test]
fn if_supports_and_or() {
    // `If a = b And c = d Then` / `Or` 形式に対応する。
    let src = "\
Set a 1
Set b 1
Set c 2
Set d 2
If $(a) = $(b) And $(c) = $(d) Then
　Stage and_true
Else
　Stage and_false
EndIf
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.stage(), "and_true");

    let src2 = "\
Set a 0
Set b 1
If $(a) = $(b) Or $(a) = 0 Then
　Stage or_true
EndIf
";
    let stmts = event::parse(src2).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.stage(), "or_true");
}

#[test]
fn foreach_over_string_keyed_indexed_var() {
    // `タイトル画面アクション[Ｇ３ブレイバー] = ...` のような文字列キーの
    // インデックス変数を ForEach で走査できる。
    let src = "\
Set arr[ブレイバー] G
Set arr[ゾルダ] Z
Set arr[エルガイム] E
ForEach k In arr
　Set 訪問[$(k)] 1
Next
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("訪問[ブレイバー]"), "1");
    assert_eq!(app.script_var("訪問[ゾルダ]"), "1");
    assert_eq!(app.script_var("訪問[エルガイム]"), "1");
}

#[test]
fn switch_case_supports_space_separated_multi_value() {
    // `Case A B` (空白区切り) で複数値マッチを受理する。
    let src = "\
Set 選択 START
Switch $(選択)
Case START EXハード
　Stage matched
Case 別
　Stage other
EndSw
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.stage(), "matched");
}

#[test]
fn execute_does_not_run_when_already_paused() {
    // 最初の eve が Wait Click で中断
    let stmts1 = event::parse("Wait Click\n").unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts1).unwrap();
    assert!(app.pending_dialog().is_some());

    // 2 つ目の eve は中断中なので実行されない
    let stmts2 = event::parse("Stage \"BadScenario\"\nMessage \"shouldnotrun\"\n").unwrap();
    execute(&mut app, &stmts2).unwrap();
    assert_eq!(app.stage(), "");
    assert!(app.messages().is_empty());
    // library には登録されているはず
    // (Stage / Message は label じゃないので label_pc は無い。
    //  代わりに statements に積まれているかを総量で確認)
    assert!(app.script_library().statements.len() >= 3);
}

#[test]
fn do_while_skips_when_false() {
    let src = "\
Set x 10
Do While ($(x) < 5)
  Set ran 1
Loop
Message after
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("ran"), "");
    assert_eq!(app.messages(), &["after".to_string()]);
}

#[test]
fn do_while_iterates_with_incr() {
    let src = "\
Set i 0
Do While ($(i) < 3)
  Incr i
Loop
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("i"), "3");
}

#[test]
fn loop_until_inverts() {
    let src = "\
Set i 0
Do
  Incr i
Loop Until ($(i) >= 3)
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("i"), "3");
}

#[test]
fn break_exits_loop() {
    let src = "\
Set i 0
Do
  Incr i
  If $(i) = 5 Then
    Break
  EndIf
Loop
Message done
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("i"), "5");
    assert_eq!(app.messages(), &["done".to_string()]);
}

#[test]
fn not_negates_if_condition() {
    let src = "\
Set x 0
If Not ($(x) = 1) Then
  Message yes
EndIf
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages(), &["yes".to_string()]);
}

#[test]
fn foreach_iterates_party_units() {
    let mut app = App::new();
    setup_two_units(&mut app);
    // Player には ブレイバー 1 体だけ
    let src = "\
Set found 0
ForEach u Player
  Incr found
Next
";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("found"), "1");
}

#[test]
fn foreach_iterates_comma_list() {
    let src = "\
Set joined empty
ForEach name a, b, c
  Set joined $(joined)_$(name)
Next
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("joined"), "empty_a_b_c");
}

#[test]
fn foreach_with_empty_collection_skips_body() {
    // 存在しない勢力 → 何も実行されない
    let src = "\
Set ran 0
ForEach u Allied
  Set ran 1
Next
Message done
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("ran"), "0");
    assert_eq!(app.messages(), &["done".to_string()]);
}

#[test]
fn foreach_group_form_iterates_party_units() {
    // SRC 書式1 `ForEach group` — ループ変数を持たず、各反復で
    // `対象パイロット` / `対象ユニットＩＤ` に現在のユニットを束縛する。
    // スパロボ戦記 Include.eve の `Foreach 味方` がこの形。
    let mut app = App::new();
    setup_two_units(&mut app);
    let src = "\
Set cnt 0
Set last \"\"
ForEach 味方
  Incr cnt
  Set last $(対象パイロット)
Next
";
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    // Player は ブレイバー/リオ 1 体。
    assert_eq!(app.script_var("cnt"), "1");
    assert_eq!(app.script_var("last"), "リオ");
}

#[test]
fn foreach_group_form_all_iterates_every_unit() {
    let mut app = App::new();
    setup_two_units(&mut app);
    let src = "Set cnt 0\nForEach 全\n  Incr cnt\nNext\n";
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    assert_eq!(app.script_var("cnt"), "2");
}

#[test]
fn foreach_group_form_binds_target_unit_id() {
    // Create したユニットは uid を持つ → 対象ユニットＩＤ に uid が入る。
    let mut app = App::new();
    let src = "\
Pilot \"リオ\" リオ 男性 c AAAA 100 100 100 100 100 100 100
Unit \"ブレイバー\" リアル系 1 0 陸 5 M 3000 400 3500 120 1200 110 AAAA
MapSize 5 5
Create 味方 ブレイバー 0 リオ 1 1 1
ForEach 味方
  Set uid $(対象ユニットＩＤ)
Next
";
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    assert_eq!(app.script_var("uid"), "U1");
}

#[test]
fn foreach_group_form_empty_group_skips_body() {
    // 該当ユニットが居ない勢力 → 本体は実行されない (2 引数エラーにしない)。
    let mut app = App::new();
    setup_two_units(&mut app);
    let src = "Set ran 0\nForEach 友軍\n  Set ran 1\nNext\nMessage ok\n";
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    assert_eq!(app.script_var("ran"), "0");
    assert_eq!(app.messages(), &["ok".to_string()]);
}

#[test]
fn foreach_group_form_status_filter() {
    // 書式1 の status (`出撃` / `待機` / `全て`) でユニットを絞り込む。
    let mut app = App::new();
    setup_two_units(&mut app); // ブレイバー(味方/出撃), ゾルダII(敵/出撃)
                               // `Unit` 短形式で待機 (off_map) の Player ユニットを 1 体追加。
    execute(&mut app, &event::parse("Unit \"ゾルダII\" 0\n").unwrap()).unwrap();

    execute(
        &mut app,
        &event::parse("Set a 0\nForEach 味方 出撃\n  Incr a\nNext\n").unwrap(),
    )
    .unwrap();
    assert_eq!(app.script_var("a"), "1", "出撃のみ → ブレイバー");

    execute(
        &mut app,
        &event::parse("Set b 0\nForEach 味方 待機\n  Incr b\nNext\n").unwrap(),
    )
    .unwrap();
    assert_eq!(
        app.script_var("b"),
        "1",
        "待機のみ → 追加した off_map ゾルダII"
    );

    execute(
        &mut app,
        &event::parse("Set c 0\nForEach 味方 全て\n  Incr c\nNext\n").unwrap(),
    )
    .unwrap();
    assert_eq!(app.script_var("c"), "2", "全て → 出撃 + 待機");
}

#[test]
fn foreach_format3_pilot_roster() {
    // 書式3 `ForEach var In パイロット一覧(mode)` — 搭乗中パイロット名を反復。
    let mut app = App::new();
    setup_two_units(&mut app); // ブレイバー/リオ, ゾルダII/ガロ
    let src = "\
Set names \"\"
ForEach p In パイロット一覧(レベル)
  Set names \"$(names) $(p)\"
Next
";
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    let names = app.script_var("names");
    assert!(names.contains("リオ"), "names={names}");
    assert!(names.contains("ガロ"), "names={names}");
}

#[test]
fn skip_jumps_to_next_in_for_loop() {
    // `Skip` は対応する Next へジャンプする (ループ継続 = "continue" 相当)。
    // SRC.Sharp `SkipCmd.cs` 準拠。
    let src = r#"
Set count 0
For i = 1 To 3
  Incr count
  Skip
  Incr count 100
Next
"#;
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // 各イテレーションで count +1 → 計 3。`Incr count 100` は実行されない。
    assert_eq!(app.script_var("count"), "3");
}

#[test]
fn skip_jumps_to_loop_in_do_loop() {
    // `Skip` は Do/Loop ループでも Loop まで飛ぶ。
    let src = r#"
Set n 0
Set i 0
Do
  Incr i
  If $(i) > 3 Then
    Goto done
  EndIf
  Incr n
  Skip
  Incr n 100
Loop
done:
"#;
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // i=1,2,3: n+1 each → n=3。`Incr n 100` は実行されない。
    assert_eq!(app.script_var("n"), "3");
}

#[test]
fn skip_outside_loop_returns_error() {
    // ループ外で `Skip` を使うとエラー。
    let src = "Skip\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    let result = execute(&mut app, &stmts);
    assert!(result.is_err(), "Skip outside loop should be an error");
}

#[test]
fn do_loop_terminates_via_exit() {
    // 単純 Do/Loop の動作確認: Set counter; Do { Incr counter; If >= 3 Exit; } Loop
    let src = "\
Set counter 0
Do
  Incr counter
  If $(counter) >= 3 Then
    Exit
  EndIf
Loop
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("counter"), "3");
}

#[test]
fn trigger_label_runs_dormant_section() {
    // Goto で本体をスキップした後、Start ラベルを後から手動 trigger
    let src = "\
Goto skip
Start:
  Message hit_start
Exit
skip:
Message body
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages(), &["body".to_string()]);
    // ライブラリに Start ラベルがあるはず
    assert!(app.script_library().label_pc("Start").is_some());
    // 手動 trigger
    let fired = crate::event_runtime::trigger_label(&mut app, "Start");
    assert!(fired);
    assert!(app.messages().contains(&"hit_start".to_string()));
}

#[test]
fn trigger_label_returns_false_if_no_label() {
    let mut app = App::new();
    execute(&mut app, &event::parse("Message x\n").unwrap()).unwrap();
    assert!(!crate::event_runtime::trigger_label(
        &mut app,
        "NoSuchLabel"
    ));
}

#[test]
fn trigger_label_blocked_during_dialog() {
    let src = "\
Goto skip
Start:
  Message hit
Exit
skip:
Talk リオ
こんにちは
End
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // モーダル中
    assert!(app.pending_dialog().is_some());
    assert!(!crate::event_runtime::trigger_label(&mut app, "Start"));
}

#[test]
fn function_in_if_condition() {
    let mut app = App::new();
    setup_two_units(&mut app);
    let src = "\
If Distance(ブレイバー, ゾルダII) > 3 Then
  Message far
Else
  Message close
EndIf
";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages().last().map(String::as_str), Some("far"));
}

#[test]
fn switch_picks_matching_case() {
    let src = "\
Set x 2
Switch $(x)
Case 1
  Message one
Case 2
  Message two
Case 3
  Message three
CaseElse
  Message other
EndSw
Message after
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages(), &["two".to_string(), "after".to_string()]);
}

#[test]
fn switch_falls_through_to_caseelse() {
    let src = "\
Set x 9
Switch $(x)
Case 1
  Message one
CaseElse
  Message other
EndSw
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages(), &["other".to_string()]);
}

#[test]
fn switch_with_no_match_and_no_caseelse_skips() {
    let src = "\
Set x 9
Switch $(x)
Case 1
  Message one
EndSw
Message after
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages(), &["after".to_string()]);
}

#[test]
fn for_loop_iterates_count_times() {
    let src = "\
Set total 0
For i = 1 To 5
  Incr total
Next
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("total"), "5");
    // SRC.Sharp 準拠: Next は必ず変数を更新してから終了判定するため、
    // ループ終了後の i は end + step = 5 + 1 = 6。
    assert_eq!(app.script_var("i"), "6");
}

#[test]
fn for_loop_with_step() {
    let src = "\
Set total 0
For i = 0 To 10 Step 2
  Incr total $(i)
Next
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // 0+2+4+6+8+10 = 30
    assert_eq!(app.script_var("total"), "30");
}

#[test]
fn for_loop_skips_when_start_exceeds_end() {
    let src = "\
Set ran 0
For i = 10 To 1
  Set ran 1
Next
Message after
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("ran"), "0");
    assert_eq!(app.messages(), &["after".to_string()]);
}

#[test]
fn nested_for_loops() {
    let src = "\
Set total 0
For i = 1 To 3
  For j = 1 To 2
    Incr total
  Next
Next
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("total"), "6");
}

#[test]
fn call_jumps_to_label_and_returns() {
    let src = "\
Message before
Call greeting
Message after
Exit
greeting:
  Message hello
Return
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(
        app.messages(),
        &[
            "before".to_string(),
            "hello".to_string(),
            "after".to_string()
        ]
    );
}

#[test]
fn call_passes_args_via_args_function() {
    let src = "\
Call greet 太郎 さん
Exit
greet:
  Set who Args(1) Args(2)
  Message $(who)
Return
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages(), &["太郎 さん".to_string()]);
}

#[test]
fn exit_terminates_execution() {
    let src = "\
Message before
Exit
Message after
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages(), &["before".to_string()]);
}

#[test]
fn no_op_commands_dont_crash() {
    let src = "\
Wait 30
StartBGM song.mid
StopBGM
PlaySound boom.wav
FadeOut 50
ChangeMap -.map
ChangeTerrain 0 0 1
Font ゴシック Regular 通常
PaintString - hello 100
Refresh
Hide
Show
KeepBGM
Option foo
Input var prompt default
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
}

#[test]
fn nested_if_does_not_consume_outer_endif() {
    let src = "\
Set a 0
Set b 1
If $(a) = 1 Then
  If $(b) = 1 Then
    Message inner
  EndIf
  Message outer_then
Else
  Message outer_else
EndIf
Message after
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(
        app.messages(),
        &["outer_else".to_string(), "after".to_string()]
    );
}

#[test]
fn infinite_loop_aborts() {
    let src = "\
loop:
Goto loop
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    let e = execute(&mut app, &stmts).unwrap_err();
    assert!(e.message.contains("上限"));
}
