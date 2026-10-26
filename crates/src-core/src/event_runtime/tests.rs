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

/// ラベルが無い名前は関数として実行せずリテラルのまま残す。
#[test]
fn unknown_function_name_is_left_literal() {
    let stmts = event::parse("Set z 未定義関数(1)\nStage done\n").unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("z"), "未定義関数(1)");
}

use super::*;
use crate::data::event;

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
    assert_eq!(app.database().pilots.len(), 1);
    assert_eq!(app.database().units.len(), 1);
    assert_eq!(app.database().units[0].weapons.len(), 1);
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
fn set_and_expand_variable() {
    let src = "\
Set 宿名 女神の居眠り亭
Stage $(宿名)
Message $(宿名)
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("宿名"), "女神の居眠り亭");
    assert_eq!(app.stage(), "女神の居眠り亭");
    assert_eq!(app.messages(), &["女神の居眠り亭".to_string()]);
}

#[test]
fn dollar_paren_evaluates_function_calls() {
    // `$(Func(...))` は変数キー lookup ではなく関数として評価される。
    // スパロボ戦記タイトルの `Set ユニット画像[i]
    // "Anime\Unit\$(Lindex(タイトル画面アクション[i],1))"` がこの経路。
    // 引数にインデックス変数 `配列[i]` を含むケースも解決できること。
    let src = "\
Set act[真イーグル] List(TrueEagle1.bmp,TrueEagle2.bmp)
Foreach i In act
　Set 画像[i] \"Anime\\Unit\\$(Lindex(act[i],1))\"
Next
Set 結果 $(画像[真イーグル])
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("結果"), "Anime\\Unit\\TrueEagle1.bmp");
}

/// SRC `Localコマンド` 書式1 `Local var1 var2 …` は **宣言のみ**で、
/// 各変数を空文字で作る (C# `LocalCmd.cs`: `for i = 2 To ArgNum` →
/// `SetValue(vname, StringType, "", 0)`)。`Set` の別名ではない。
/// 実コーパスでも `Local i j` のような複数宣言が 358 箇所ある。
#[test]
fn local_declares_each_arg_empty() {
    let stmts = event::parse("Local x y\n").unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("x"), "");
    assert_eq!(app.script_var("y"), "");
    assert!(app.is_script_var_defined("y"), "y も宣言される");
}

/// SRC `Localコマンド` 書式2 `Local var = expr` は式の値で初期化する。
/// `=` は値に含めない (旧実装は `"= 42"` を代入していた)。
/// 実コーパスで最頻の形 (3225 箇所)。
#[test]
fn local_assign_form_evaluates_expression() {
    let src = "Local x = 42\n\
Local y = (1 + 2)\n\
Set 陣営 敵\n\
Local p = 陣営\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("x"), "42");
    assert_eq!(app.script_var("y"), "3");
    assert_eq!(app.script_var("p"), "敵");
}

/// テストヘルパ: src を実行して App を返す。
fn run_script(src: &str) -> App {
    let stmts = event::parse(src).expect("parse");
    let mut app = App::new();
    execute(&mut app, &stmts).expect("execute");
    app
}

#[test]
fn virtual_file_write_then_read_roundtrip() {
    // `Open`/`Print`/`Close` で書いた仮想ファイルを `入力` で開き直して
    // `Read` で 1 行ずつ読み戻せる。
    let app = run_script(
        "Open \"test.txt\" For 出力 As F\n\
             Print F ライン1\n\
             Print F ライン2\n\
             Close F\n\
             Open \"test.txt\" For 入力 As G\n\
             Read G 行A\n\
             Read G 行B\n\
             Read G 行C\n\
             Close G\n",
    );
    assert_eq!(app.script_var("行A"), "ライン1");
    assert_eq!(app.script_var("行B"), "ライン2");
    // 末尾を超えた Read は空文字。
    assert_eq!(app.script_var("行C"), "");
}

#[test]
fn virtual_file_output_mode_truncates() {
    // `出力` で開き直すと旧内容は切り詰められる。
    let app = run_script(
        "Open \"x.txt\" For 出力 As F\nPrint F 古い\nClose F\n\
             Open \"x.txt\" For 出力 As F\nPrint F 新しい\nClose F\n\
             Open \"x.txt\" For 入力 As G\nRead G 行\nClose G\n",
    );
    assert_eq!(app.script_var("行"), "新しい");
    assert_eq!(
        app.virtual_file_lines("x.txt"),
        Some(&["新しい".to_string()][..])
    );
}

#[test]
fn virtual_pilot_file_close_registers_pilot_data() {
    // `pilot.txt` を仮想ファイルに書き出して `Close` すると、内容が
    // 再パースされ PilotData として GameDatabase に登録される
    // (キャラメイキングが書き出すキャラを使用可能にする経路)。
    let app = run_script(
        "Open \"data\\一時フォルダ\\pilot.txt\" For 出力 As F\n\
             Print F\n\
             Print F\n\
             Print F テストキャラ\n\
             Print F テス, -, 汎用, AAAA, 100\n\
             Print F 特殊能力\n\
             Print F 150, 150, 150, 150, 150, 150\n\
             Close F\n",
    );
    assert!(
        app.database().pilot_by_name("テストキャラ").is_some(),
        "Close 時に pilot.txt が再パースされ PilotData が登録される"
    );
}

#[test]
fn two_dimensional_array_index_resolves() {
    // `name[i,j]` 多次元添字: リテラル添字 (`[1,3]`) と変数添字
    // (`[i,j]` で i=1,j=3) が同じキーを指し、Set と読出が一致する。
    // スパロボ戦記 AlphaSecond.eve の `搭乗員[i,j]` 相当。
    let app = run_script(
        "Set 搭乗員[1,3] リオ\n\
             Set i 1\n\
             Set j 3\n\
             Set lit $(搭乗員[1,3])\n\
             Set var $(搭乗員[i,j])\n\
             Set d1 グフ\n\
             Set arr[5] $(d1)\n\
             Set one $(arr[5])\n",
    );
    assert_eq!(app.script_var("lit"), "リオ", "リテラル添字 [1,3]");
    assert_eq!(app.script_var("var"), "リオ", "変数添字 [i,j]");
    assert_eq!(app.script_var("搭乗員[1,3]"), "リオ", "正規化キー");
    assert_eq!(app.script_var("one"), "グフ", "1 次元添字は従来どおり");
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
fn custom_command_prefix_variants() {
    // 全プレフィックスバリアントの post_move_ok / post_act_ok フラグを検証。
    let src = "\
ユニットコマンド デフォルト 全:
Return

*ユニットコマンド ポストムーブ 全:
Return

*-ユニットコマンド ポストムーブ2 全:
Return

-*ユニットコマンド ポストアクト 全:
Return

**ユニットコマンド 両方 全:
Return
";
    let stmts = event::parse(src).expect("parse");
    let mut app = App::new();
    app.script_library_mut().append(&stmts);
    let cc = &app.script_library().custom_commands;
    assert_eq!(cc.len(), 5);

    let find = |nm: &str| {
        cc.iter()
            .find(|c| c.name == nm)
            .unwrap_or_else(|| panic!("{nm} not found"))
    };

    let d = find("デフォルト");
    assert!(!d.post_move_ok && !d.post_act_ok, "no prefix → both false");

    let pm = find("ポストムーブ");
    assert!(pm.post_move_ok && !pm.post_act_ok, "* → post_move_ok only");

    let pm2 = find("ポストムーブ2");
    assert!(
        pm2.post_move_ok && !pm2.post_act_ok,
        "*- → post_move_ok only"
    );

    let pa = find("ポストアクト");
    assert!(!pa.post_move_ok && pa.post_act_ok, "-* → post_act_ok only");

    let both = find("両方");
    assert!(both.post_move_ok && both.post_act_ok, "** → both true");
}

#[test]
fn ampersand_concat_inside_parentheses() {
    // `("a" & Func())` のように括弧で 1 トークンに括られた `&` 連結。
    // tokenizer は `(...)` を 1 トークンに保つため `collapse_concat`
    // には捕まらず、`expand_arg` がトークン内部の `&` を畳む。
    // スパロボ戦記 Include.eve の `Ride ("$(A)" & "＋" & "$(B)")` 型。
    let app = run_script("Set p (\"u_\" & Left(\"ブレイバー\", 2))\n");
    assert_eq!(app.script_var("p"), "u_ブレ");
}

#[test]
fn ampersand_concat_inside_parentheses_with_dollar_vars() {
    // 括弧付き `&` 連結のオペランドが `$(var)` リテラル。
    let app = run_script("Set 左 リオ\nSet 右 ガロ\nSet p (\"$(左)\" & \"＋\" & \"$(右)\")\n");
    assert_eq!(app.script_var("p"), "リオ＋ガロ");
}

#[test]
fn set_evaluates_parenthesized_arithmetic() {
    // SRC の `Set` は値を式評価する。`&` を含まない括弧式は連結扱い
    // されずトークンとして残るが、値全体が単一の括弧式なら数値化する。
    let app = run_script("Set v (200 - 128 / 2)\n");
    assert_eq!(app.script_var("v"), "136");
}

#[test]
fn set_evaluates_parenthesized_arithmetic_with_variable() {
    // 括弧式中の裸変数 (`進行度`) も script_var として解決して評価する。
    // スパロボ戦記 Main.eve `Set 敵配置数 (RoundUp(…) + (進行度 / 5))`
    // 相当 — 関数評価後に残る `(5 + (進行度 / 5))` を数値化する。
    let app = run_script("Set 進行度 10\nSet n (5 + (進行度 / 5))\n");
    assert_eq!(app.script_var("n"), "7");
}

#[test]
fn set_paren_with_non_numeric_atom_kept_verbatim() {
    // 非数値アトムを含む括弧式は算術ではないとみなし、誤って 0 に
    // 潰さず文字列のまま残す (`Set msg (こんにちは)` を壊さない)。
    let app = run_script("Set msg (こんにちは)\n");
    assert_eq!(app.script_var("msg"), "(こんにちは)");
}

#[test]
fn set_non_parenthesized_arithmetic_kept_verbatim() {
    // 括弧で囲われていない値は従来どおり式評価しない
    // (`Set 型番 RX-78-2` を `-53` と誤評価しないため)。
    let app = run_script("Set 型番 78-2\n");
    assert_eq!(app.script_var("型番"), "78-2");
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
fn talk_pauses_with_dialog_and_logs_message() {
    let src = "\
Talk 番頭さん
お帰りなさいませ
ごゆるりとどうぞ
End
Message after_talk
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // 対話モーダル中: after_talk はまだ実行されていない
    assert!(app.pending_dialog().is_some());
    match app.pending_dialog().unwrap() {
        crate::PendingDialog::Talk { speaker, body } => {
            assert_eq!(speaker, "番頭さん");
            assert!(body.contains("お帰りなさいませ"));
            assert!(body.contains("ごゆるりとどうぞ"));
        }
        _ => panic!("expected Talk dialog"),
    }
    // ログには既に積まれている
    assert_eq!(app.messages().len(), 1);
    assert!(app.messages()[0].starts_with("【番頭さん】"));
    // 任意キーで進行（choice 値は無視）
    assert!(app.respond_dialog(0));
    // 続きが流れる
    assert_eq!(
        app.messages().last().map(String::as_str),
        Some("after_talk")
    );
}

#[test]
fn paren_arith_missing_operand_defaults_to_zero() {
    // SRC: 未定義数値は 0。Info(...) 等が空に解決して `)` 直前に二項演算子が
    // 残っても括弧算術が評価できること (スパロボ戦記 AlphaSecond の
    // `次のレベル(500 - )` / `(500 * 0 - 500 + )` 由来の生式表示の防止)。
    let app = App::new();
    assert_eq!(
        eval_paren_arith_value(&app, "(500 - )"),
        Some("500".to_string())
    );
    assert_eq!(
        eval_paren_arith_value(&app, "(500 * 0 - 500 + )"),
        Some("-500".to_string())
    );
    // 通常の括弧算術は従来どおり評価される (回帰防止)。
    assert_eq!(
        eval_paren_arith_value(&app, "(2 + 3)"),
        Some("5".to_string())
    );
    // try_eval_num 自体は厳格なまま (配列添字解決の fail→fallback を壊さない)。
    assert_eq!(try_eval_num("(500 - )"), None);
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
fn right_click_exits_wait_click_via_keystate2() {
    // スパロボ戦記 AlphaSecond のステータス画面: `Wait Click` → 右クリックで
    // `選択 = ""` かつ `KeyState(2) = 1` となり `Case "" → If KeyState(2) Then Break`
    // で画面を抜けられること。右クリック未対応だと抜け出せず詰む回帰の防止。
    let src = "\
状態画面:
Hotpoint タブ 10 10 50 50
Do
  Wait Click
  Switch 選択
  Case タブ
    Set 結果 タブ選択
    Break
  Case \"\"
    If KeyState(2) Then
      Set 結果 終了
      Break
    EndIf
  EndSw
Loop While (1)
Exit
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    app.script_library_mut().append(&stmts);
    crate::event_runtime::trigger_label(&mut app, "状態画面");
    // Wait Click で中断している
    assert!(
        app.pending_dialog().is_some(),
        "Wait Click で中断していない"
    );
    // 右クリック → 選択="" + KeyState(2)=1 → ループ脱出
    assert!(app.respond_dialog_right_click());
    assert!(
        app.pending_dialog().is_none(),
        "右クリックで Wait Click を抜けられていない"
    );
    assert_eq!(app.script_var("結果"), "終了");
}

#[test]
fn talk_colon_pages_and_semicolon_linebreak() {
    // SRC Talkコマンド.md: 半角 `;` は強制改行、半角 `:` は段階表示の区切り。
    // musou202 の難易度選択 "Normalね。:;情報が..." と同じ構造。
    let src = "\
Talk パチュリー
Normalね。:;情報がだいたい表示されたり、色々と普通だと思うわ。
End
Message done
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // 1 ページ目: `:` より前。`:`/`;` の生文字は出ない。
    match app.pending_dialog().unwrap() {
        crate::PendingDialog::Talk { speaker, body } => {
            assert_eq!(speaker, "パチュリー");
            assert_eq!(body, "Normalね。");
            assert!(!body.contains(':') && !body.contains(';'));
        }
        _ => panic!("expected Talk dialog (page 1)"),
    }
    // 応答 → 2 ページ目 (`;` は改行に変換)。スクリプトはまだ再開しない。
    assert!(app.respond_dialog(0));
    match app.pending_dialog().unwrap() {
        crate::PendingDialog::Talk { speaker, body } => {
            assert_eq!(speaker, "パチュリー");
            assert_eq!(body, "情報がだいたい表示されたり、色々と普通だと思うわ。");
        }
        _ => panic!("expected Talk dialog (page 2)"),
    }
    // done はまだ
    assert_ne!(app.messages().last().map(String::as_str), Some("done"));
    // 最終ページ応答 → スクリプト再開
    assert!(app.respond_dialog(0));
    assert!(app.pending_dialog().is_none());
    assert_eq!(app.messages().last().map(String::as_str), Some("done"));
}

#[test]
fn multiple_talks_pause_one_at_a_time() {
    let src = "\
Talk A
hello
End
Talk B
world
End
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert!(matches!(
        app.pending_dialog(),
        Some(crate::PendingDialog::Talk { speaker, .. }) if speaker == "A"
    ));
    app.respond_dialog(0);
    assert!(matches!(
        app.pending_dialog(),
        Some(crate::PendingDialog::Talk { speaker, .. }) if speaker == "B"
    ));
    app.respond_dialog(0);
    assert!(app.pending_dialog().is_none());
}

#[test]
fn talk_terminated_by_suspend_only_shows_preceding_body() {
    // SRC Talkコマンド.md: Suspend は Talk ブロックの終端として機能する。
    // 重要: Suspend の後に続くコマンド ("PlaySound" 等) がダイアログ本文に
    // 混入してはならない。musou202.lzh の "Talk システム / ... / Suspend" と
    // 同じ構造。
    let src = "\
Talk システム
少女祈祷中...
Suspend
Message after_suspend
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    app.set_scene(crate::Scene::MapView);
    execute(&mut app, &stmts).unwrap();
    // Suspend が Talk 終端なので pending_dialog が立つ
    assert!(app.pending_dialog().is_some());
    match app.pending_dialog().unwrap() {
        crate::PendingDialog::Talk { speaker, body } => {
            assert_eq!(speaker, "システム");
            assert_eq!(body, "少女祈祷中...");
            // Suspend や後続コマンドが本文に混入していない
            assert!(!body.contains("Suspend"));
            assert!(!body.contains("after_suspend"));
        }
        _ => panic!("expected Talk dialog"),
    }
    // ダイアログ応答後、Suspend は消費済みなのでタイトル復帰しない (Scene そのまま)
    app.respond_dialog(0);
    assert_eq!(
        app.messages().last().map(String::as_str),
        Some("after_suspend")
    );
    assert_eq!(app.scene(), crate::Scene::MapView);
}

#[test]
fn talk_position_args_not_included_in_speaker() {
    // `Talk システム 8 8` → speaker は "システム" のみ。
    // 座標引数 "8 8" がスピーカー名に混入してはならない。
    let src = "\
Talk システム 8 8
メッセージ
End
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    match app.pending_dialog().unwrap() {
        crate::PendingDialog::Talk { speaker, body } => {
            assert_eq!(speaker, "システム");
            assert!(
                !speaker.contains('8'),
                "座標がスピーカーに混入: {speaker:?}"
            );
            assert_eq!(body, "メッセージ");
        }
        _ => panic!("expected Talk dialog"),
    }
}

#[test]
fn talk_parenthesized_position_not_in_speaker() {
    // `Talk 霊夢 (8,6)` → speaker は "霊夢" のみ。
    let src = "\
Talk 霊夢 (8,6)
春ね…
End
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    match app.pending_dialog().unwrap() {
        crate::PendingDialog::Talk { speaker, body } => {
            assert_eq!(speaker, "霊夢");
            assert_eq!(body, "春ね…");
        }
        _ => panic!("expected Talk dialog"),
    }
}

#[test]
fn talk_body_html_tags_stripped() {
    // SRC Talk ボディの書式タグは除去される。
    // タグは常にテキスト行の中に埋め込まれる (行頭に単独で書かれない)。
    // <B>/</B>/<I>/</I>/<COLOR=...>/</COLOR>/<SIZE=n>/</SIZE> など。
    // <LT> → '<'、<GT> → '>' への変換も確認。
    // musou202.lzh 東方夢想伝01.eve の実例と同型。
    let src = "\
Talk 霊夢
一つは<B>『ボーダーボーナス』</B>…名前はお馴染みよね？
さらに<I>強調テキスト</I>もあるわ。
<LT>タグ<GT>を含む行
End
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    match app.pending_dialog().unwrap() {
        crate::PendingDialog::Talk { body, .. } => {
            // タグ除去済み
            assert!(!body.contains("<B>"), "body={body:?}");
            assert!(!body.contains("</B>"), "body={body:?}");
            assert!(!body.contains("<I>"), "body={body:?}");
            assert!(!body.contains("</I>"), "body={body:?}");
            // テキスト内容は残っている
            assert!(body.contains("『ボーダーボーナス』"), "body={body:?}");
            assert!(body.contains("強調テキスト"), "body={body:?}");
            // <LT>/<GT> 変換
            assert!(body.contains('<'), "body={body:?}");
            assert!(body.contains('>'), "body={body:?}");
            assert!(!body.contains("<LT>"), "body={body:?}");
            assert!(!body.contains("<GT>"), "body={body:?}");
        }
        _ => panic!("expected Talk dialog"),
    }
}

#[test]
fn talk_inner_talk_splits_into_multiple_dialogs() {
    // SRC 仕様: Talk ブロック内の Talk コマンドは話者切り替え。
    // メインループが次の Talk を実行するため、各話者のメッセージが
    // それぞれ別のダイアログとして表示される。
    // musou202.lzh "Talk 魔理沙 / ... / Talk パチュリー / ... / End" と同構造。
    let src = "\
Talk 魔理沙
よっ、魔理沙だぜ。
Talk パチュリー
パチュリーよ。
End
Message done
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // 最初は 魔理沙 のダイアログ
    assert!(matches!(
        app.pending_dialog(),
        Some(crate::PendingDialog::Talk { speaker, .. }) if speaker == "魔理沙"
    ));
    if let crate::PendingDialog::Talk { body, .. } = app.pending_dialog().unwrap() {
        assert_eq!(body, "よっ、魔理沙だぜ。");
        // "Talk パチュリー" がテキストとして混入していない
        assert!(!body.contains("Talk"));
    }
    app.respond_dialog(0);
    // 次は パチュリー のダイアログ
    assert!(matches!(
        app.pending_dialog(),
        Some(crate::PendingDialog::Talk { speaker, .. }) if speaker == "パチュリー"
    ));
    if let crate::PendingDialog::Talk { body, .. } = app.pending_dialog().unwrap() {
        assert_eq!(body, "パチュリーよ。");
    }
    app.respond_dialog(0);
    assert!(app.pending_dialog().is_none());
    assert_eq!(app.messages().last().map(String::as_str), Some("done"));
}

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
fn confirm_pauses_and_resumes() {
    // SRC `Confirm` 仕様: Yes → 選択=1, No → 選択=0。
    let src = "\
Confirm 続けますか
If $(選択) = 1 Then
  Message yes
Else
  Message no
EndIf
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // Confirm でモーダル化、まだ if 分岐は走っていない
    assert!(app.pending_dialog().is_some());
    match app.pending_dialog().unwrap() {
        crate::PendingDialog::Confirm { question, var_name } => {
            assert_eq!(question, "続けますか");
            assert_eq!(var_name, "選択");
        }
        _ => panic!("expected Confirm dialog"),
    }
    // Yes (内部 choice=0 → 選択="1") で応答
    assert!(app.respond_dialog(0));
    assert_eq!(app.script_var("選択"), "1");
    assert_eq!(app.messages(), &["yes".to_string()]);
    // No (内部 choice=1 → 選択="0") ならもう一度走らせて確認
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert!(app.respond_dialog(1));
    assert_eq!(app.script_var("選択"), "0");
    assert_eq!(app.messages(), &["no".to_string()]);
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
fn menu_pauses_with_options() {
    let src = "\
Menu 行動を選んでください
攻撃
防御
逃走
End
If $(選択) = 2 Then
  Message defend
EndIf
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    match app.pending_dialog().unwrap() {
        crate::PendingDialog::Menu {
            prompt,
            options,
            var_name,
            ..
        } => {
            assert_eq!(prompt, "行動を選んでください");
            assert_eq!(options.len(), 3);
            assert_eq!(options[0], "攻撃");
            assert_eq!(options[1], "防御");
            assert_eq!(options[2], "逃走");
            assert_eq!(var_name, "選択");
        }
        _ => panic!("expected Menu"),
    }
    // 2 = 防御 を選ぶ
    app.respond_dialog(2);
    assert_eq!(app.script_var("選択"), "2");
    assert_eq!(app.messages(), &["defend".to_string()]);
}

#[test]
fn input_command_pauses_and_uses_text() {
    let src = "\
Input name 名前を入力 太郎
Stage $(name)
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // Input でモーダル中、default が暫定セットされている
    assert!(matches!(
        app.pending_dialog(),
        Some(crate::PendingDialog::Input { default, .. }) if default == "太郎"
    ));
    assert_eq!(app.script_var("name"), "太郎");
    // ユーザがテキスト入力して確定
    assert!(app.respond_dialog_text("二郎".to_string()));
    assert_eq!(app.script_var("name"), "二郎");
    assert_eq!(app.stage(), "二郎");
}

#[test]
fn input_default_kept_when_canceled_via_respond_dialog() {
    let src = "Input name 名前 太郎\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // 数値応答 (DialogYes 相当) は default を保持して進む
    assert!(app.respond_dialog(0));
    assert_eq!(app.script_var("name"), "太郎");
}

#[test]
fn input_array_var_target_resolves_to_key_not_value() {
    // 回帰: 配列変数 `name[key]` への Input は、代入先を**現在値に値展開**しては
    // ならない。既に値が入った状態で Input すると、旧実装は前回値をキー名に化けさせ、
    // テキスト応答が元変数を更新しない不具合があった (D スパロボ戦記キャラメイキングの
    // 2 人目以降の名前入力が前回値のまま固まる原因)。
    let mut app = App::new();
    // 1 回目の入力相当で既に値が入っている状態を作る。
    app.set_script_var("召喚キャラ[名前]".to_string(), "417776".to_string());
    let stmts = event::parse("Input 召喚キャラ[名前] 名前を入力 \"\"\n").unwrap();
    execute(&mut app, &stmts).unwrap();
    // モーダルの var_name は格納キー (現在値 "417776" ではない)。
    assert!(matches!(
        app.pending_dialog(),
        Some(crate::PendingDialog::Input { var_name, .. }) if var_name == "召喚キャラ[名前]"
    ));
    // テキスト応答が正しいキーを更新する。
    assert!(app.respond_dialog_text("パイロイ".to_string()));
    assert_eq!(app.script_var("召喚キャラ[名前]"), "パイロイ");
}

#[test]
fn expand_vars_keeps_indexed_var_literal_inside_quotes() {
    // 回帰: クオート内の `name[expr]` は、たまたま同名の配列変数が定義済みでも
    // **値に化けず literal のまま**にする (D データロードの行検出が
    // `Instr(v,"設定[パイロット一覧]")` のリテラル破壊で失敗していた真因)。
    let mut app = App::new();
    app.set_script_var("設定[パイロット一覧]".to_string(), "VAL".to_string());
    // クオート内は literal。
    assert_eq!(
        expand_vars(&app, "\"設定[パイロット一覧]\""),
        "\"設定[パイロット一覧]\""
    );
    // クオート外は従来どおり値に展開。
    assert_eq!(expand_vars(&app, "設定[パイロット一覧]"), "VAL");
    // `$(...)` 明示補間はクオート内でも従来どおり展開。
    app.set_script_var("x".to_string(), "Y".to_string());
    assert_eq!(expand_vars(&app, "\"$(x)\""), "\"Y\"");
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
fn local_multivar_decl_does_not_break_read() {
    // `Local F 仮変数` (複数名宣言) の後、`仮変数` を Set→Instr で読めるか。
    // (D データロードはこの形を使う。本実装の Local は Set 相当なので
    //  `Local F 仮変数` = `Set F 仮変数` になるが、仮変数 自体は別途 Set で埋まる。)
    let mut app = App::new();
    let src = "Local F 仮変数\nSet 仮変数 \"設定[パイロット一覧] 人工知能(ザコ) \"\nIf Instr(仮変数, \"設定[パイロット一覧]\") Then\nSet r found\nEndif\n";
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    assert_eq!(
        app.script_var("r"),
        "found",
        "Local 宣言後の 仮変数 読みが壊れている"
    );
}

#[test]
fn loadfiledialog_returns_verify_var_else_empty() {
    // 実機はファイル選択ダイアログ。ヘッドレスでは未設定なら "" (キャンセル相当)、
    // `__verify_loadfile` 設定時はそのパスを返す (検証ドライバの `データロード` 駆動用)。
    let mut app = App::new();
    execute(
        &mut app,
        &event::parse("Set r LoadFileDialog(プレイデータ, src)\n").unwrap(),
    )
    .unwrap();
    assert_eq!(app.script_var("r"), "");
    app.set_script_var(
        "__verify_loadfile".to_string(),
        "/save/test.src".to_string(),
    );
    execute(
        &mut app,
        &event::parse("Set r2 LoadFileDialog(プレイデータ, src)\n").unwrap(),
    )
    .unwrap();
    assert_eq!(app.script_var("r2"), "/save/test.src");
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

fn setup_two_units(app: &mut App) {
    let src = "\
MapSize 6 5
Pilot \"リオ\" リオ 男性 c AAAA 100 100 100 100 100 100 100
Unit \"ブレイバー\" リアル系 1 0 陸宇 5 M 3000 400 3500 120 1200 110 AAAA
Place \"ブレイバー\" \"リオ\" Player 1 1
Pilot \"ガロ\" ガロ 男性 c AAAA 100 100 100 100 100 100 100
Unit \"ゾルダII\" リアル系 1 0 陸 5 M 800 200 2400 80 900 80 AAAA
Place \"ゾルダII\" \"ガロ\" Enemy 4 3
";
    let stmts = event::parse(src).unwrap();
    execute(app, &stmts).unwrap();
}

#[test]
fn hp_and_maxhp_functions() {
    let mut app = App::new();
    setup_two_units(&mut app);
    let src = "\
Set h HP(ブレイバー)
Set mh MaxHP(ブレイバー)
";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("h"), "3500");
    assert_eq!(app.script_var("mh"), "3500");
    // ダメージを与えると HP が下がる
    let stmts2 = event::parse("Damage ブレイバー 1000\nSet h HP(ブレイバー)\n").unwrap();
    execute(&mut app, &stmts2).unwrap();
    assert_eq!(app.script_var("h"), "2500");
}

#[test]
fn distance_function() {
    let mut app = App::new();
    setup_two_units(&mut app);
    // ブレイバー (1,1) と ゾルダII (4,3) は |3| + |2| = 5
    let src = "Set d Distance(ブレイバー, ゾルダII)\n";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("d"), "5");
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
fn exists_function() {
    let mut app = App::new();
    setup_two_units(&mut app);
    let src = "\
Set a Exists(ブレイバー)
Set b Exists(アークシップ)
";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("a"), "1");
    assert_eq!(app.script_var("b"), "0");
}

#[test]
fn position_functions() {
    let mut app = App::new();
    setup_two_units(&mut app);
    let src = "Set gx X(ブレイバー)\nSet gy Y(ブレイバー)\n";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("gx"), "1");
    assert_eq!(app.script_var("gy"), "1");
}

#[test]
fn random_function_in_range() {
    // SRC `Random(n)` は 1..n (GeneralLib.Dice 準拠)。
    let mut app = App::new();
    for _ in 0..50 {
        execute(&mut app, &event::parse("Set r Random(10)\n").unwrap()).unwrap();
        let r: u32 = app.script_var("r").parse().unwrap();
        assert!((1..=10).contains(&r), "Random(10)={r} は 1..=10 外");
    }
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
fn system_variable_resolves_in_coordinate_expression() {
    // 味方レベル平均値 等のシステム変数が座標式で解決される (リンと凛 第038話 型:
    // `(味方レベル平均値 - 1)` を座標に使う)。
    let src = "\
MapSize 10 10
Place A pa 味方 0 0
Place B pb 味方 1 0
Create 味方 C 0 pc 0 (味方レベル平均値) 5
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // A,B はレベル 1 → 平均 1 (件数 2 ではない) → C は (x=1, y=5) に生成。
    let c = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "C")
        .expect("C が生成されていない");
    assert_eq!(
        (c.x, c.y),
        (1, 5),
        "味方レベル平均値 が座標式で解決されていない"
    );
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
    assert_eq!(
        ys,
        vec![1, 2, 3],
        "座標のループ変数が解決されていない: {ys:?}"
    );
    // x は数値リテラルなので全て 4。
    assert!(app
        .database()
        .unit_instances
        .iter()
        .filter(|u| u.unit_data_name == "壁")
        .all(|u| u.x == 4));
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
    assert!(app.database().pilots.iter().all(|p| p.name != "ガロ"));
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

fn setup_units_with_item(app: &mut App) {
    setup_two_units(app);
    // Item を定義に追加 (Item 命令から参照される ItemData)
    app.database_mut().items.push(crate::data::item::ItemData {
        name: "ハイブリッドアーマー".to_string(),
        class: "armor".to_string(),
        part: "本体".to_string(),
        hp_mod: 500,
        en_mod: 50,
        armor_mod: 100,
        mobility_mod: 10,
        speed_mod: 1,
        comment: String::new(),
        features: Vec::new(),
    });
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
    app.database_mut().items.push(crate::data::item::ItemData {
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
fn switch_case_range() {
    let src = "\
Set x 5
Switch $(x)
Case 1 To 3
  Message small
Case 4 To 6
  Message middle
Case Is > 10
  Message big
EndSw
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages(), &["middle".to_string()]);
}

#[test]
fn switch_case_is_comparison() {
    let src = "\
Set x 100
Switch $(x)
Case Is < 10
  Message tiny
Case Is < 50
  Message small
Case Is >= 50
  Message big
EndSw
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.messages(), &["big".to_string()]);
}

#[test]
fn money_command_absolute_and_relative() {
    let src = "\
Money 1000
Money +500
Money -200
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.money(), 1300);
}

#[test]
fn money_function_in_expression() {
    let src = "\
Money 750
Set m Money()
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("m"), "750");
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

#[test]
fn setbullet_updates_weapon_ammo() {
    let mut app = App::new();
    setup_two_units(&mut app);
    // 簡易の weapons は無いので付与してから
    let unit_idx = app
        .database_mut()
        .units
        .iter_mut()
        .position(|u| u.name == "ブレイバー")
        .unwrap();
    app.database_mut().units[unit_idx]
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

#[test]
fn string_concat_with_ampersand() {
    let src = "\
Set name タロウ
Set greeting hello & $(name) & san
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("greeting"), "helloタロウsan");
}

#[test]
fn rgb_function() {
    let src = "Set c RGB(50, 100, 200)\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("c"), "#3264c8"); // SRC.Sharp: "#rrggbb" 形式
}

#[test]
fn rgb_function_arithmetic_args() {
    // SRC.Sharp 準拠: RGB の引数に算術式を使える。
    // RGB(200 + 55, 0, 0) → R=255, G=0, B=0 → "#ff0000"
    let src = "Set c RGB(200 + 55, 0, 0)\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("c"), "#ff0000");
}

#[test]
fn rgb_function_variable_args() {
    // SRC.Sharp 準拠: RGB の引数に変数を使える。
    let src = "Set r 255\nSet g 0\nSet b 128\nSet c RGB(r, g, b)\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("c"), "#ff0080");
}

#[test]
fn iif_function_picks_branch() {
    let src = "\
Set a 1
Set b IIF($(a), yes, no)
Set c IIF(0, yes, no)
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("b"), "yes");
    assert_eq!(app.script_var("c"), "no");
}

#[test]
fn dir_reflects_virtual_filesystem() {
    // 仮想ファイルが無ければ空文字、書き出した後は basename を返す。
    let app = run_script(
        "Set d1 Dir(\"data\\foo.txt\", ファイル)\n\
             Open \"data\\foo.txt\" For 出力 As F\nPrint F x\nClose F\n\
             Set d2 Dir(\"data\\foo.txt\", ファイル)\n",
    );
    assert_eq!(app.script_var("d1"), "", "未作成ファイルは空文字");
    assert_eq!(app.script_var("d2"), "foo.txt", "作成後は basename");
}

#[test]
fn left_right_mid_substring_functions() {
    let src = "\
Set s こんにちは世界
Set a Left(s, 3)
Set b Right(s, 2)
Set c Mid(s, 4, 3)
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("a"), "こんに");
    assert_eq!(app.script_var("b"), "世界");
    assert_eq!(app.script_var("c"), "ちは世");
}

#[test]
fn left_mid_accept_arithmetic_count_args() {
    // `Left`/`Mid`/`Right` の文字数引数は `(te - 1)` のような変数入り
    // 算術式でも評価される (生 parse だと None を返してリテラル漏れした)。
    let app = run_script(
        "Set s こんにちは世界\n\
             Set te 4\n\
             Set a Left(s, (te - 1))\n\
             Set b Mid(s, (te - 1))\n\
             Set c Right(s, (te - 2))\n",
    );
    assert_eq!(app.script_var("a"), "こんに");
    assert_eq!(app.script_var("b"), "にちは世界");
    assert_eq!(app.script_var("c"), "世界");
}

#[test]
fn font_resolves_variable_argument() {
    // `Font LetterShadow` のように変数でフォント指定を切り替える記法。
    // 変数が色文字列なら SetFont の color に反映される。
    let app = run_script(
        "Set LetterShadow #000000\n\
             Font LetterShadow\n\
             PaintString 10 10 影\n",
    );
    let setfont = app.script_overlay().cmds.iter().find_map(|c| match c {
        crate::DrawCmd::SetFont { color, .. } => Some(color.clone()),
        _ => None,
    });
    assert_eq!(setfont.as_deref(), Some("#000000"));
}

#[test]
fn require_applies_ini_key_value_assignments() {
    // `Require <path>` は script_library に登録済みの設定ファイルの
    // `key = value` 行をスクリプト変数として適用する。
    let ini = "\
# テーマ設定\n\
LetterColor1 = #ffffff\n\
LetterShadow = #000000\n\
FrameColor1 = #505098\n";
    let ini_stmts = event::parse(ini).unwrap();
    let mut app = App::new();
    app.script_library_mut()
        .append_with_name(&ini_stmts, "Lib\\Alpha2ndStatus.ini");
    let stmts = event::parse("Require Lib\\Alpha2ndStatus.ini\n").unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("LetterColor1"), "#ffffff");
    assert_eq!(app.script_var("LetterShadow"), "#000000");
    assert_eq!(app.script_var("FrameColor1"), "#505098");
}

#[test]
fn strcmp_returns_ordering() {
    let src = "\
Set a StrCmp(\"abc\", \"abc\")
Set b StrCmp(\"abc\", \"abd\")
Set c StrCmp(\"xyz\", \"abc\")
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("a"), "0");
    assert_eq!(app.script_var("b"), "-1");
    assert_eq!(app.script_var("c"), "1");
}

#[test]
fn paintpicture_pushes_draw_cmd() {
    let src = "PaintPicture path/foo.bmp 10 20 50 60 透過 左右反転\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        crate::DrawCmd::Picture {
            path,
            x,
            y,
            w,
            h,
            transparent,
            flip_x,
            ..
        } => {
            assert_eq!(path, "path/foo.bmp");
            assert_eq!(*x, 10.0);
            assert_eq!(*y, 20.0);
            assert_eq!(*w, Some(50.0));
            assert_eq!(*h, Some(60.0));
            assert!(*transparent);
            assert!(*flip_x);
        }
        _ => panic!("expected Picture"),
    }
}

#[test]
fn paintpicture_dash_position_sets_center_flags() {
    // `PaintPicture img - -`（幅省略・中央寄せ）は center フラグを立て、
    // 実寸を知るフロントエンドに中央寄せを委譲する。明示座標では立たない。
    // スパロボ戦記 機体能力画面の `\event\機体能力.png - - 透過` が
    // (240,240) に左上ずれして表示されていた不具合の回帰防止。
    let mut app = App::new();
    execute(
        &mut app,
        &event::parse("PaintPicture frame.png - - 透過\n").unwrap(),
    )
    .unwrap();
    match &app.script_overlay().cmds[0] {
        crate::DrawCmd::Picture {
            center_x,
            center_y,
            transparent,
            w,
            h,
            ..
        } => {
            assert!(*center_x, "x=- は center_x");
            assert!(*center_y, "y=- は center_y");
            // 幅省略形で w/h の位置に来た `透過` を幅と取り違えず、透過フラグ
            // として認識する (機体能力.png が不透明で覆い隠す不具合の回帰防止)。
            assert!(*transparent, "幅省略形でも 透過 を認識する");
            assert_eq!(*w, None, "透過 を w として食わない");
            assert_eq!(*h, None);
        }
        _ => panic!("expected Picture"),
    }

    let mut app2 = App::new();
    execute(
        &mut app2,
        &event::parse("PaintPicture frame.png 10 20\n").unwrap(),
    )
    .unwrap();
    match &app2.script_overlay().cmds[0] {
        crate::DrawCmd::Picture {
            center_x,
            center_y,
            x,
            y,
            ..
        } => {
            assert!(!*center_x);
            assert!(!*center_y);
            assert_eq!(*x, 10.0);
            assert_eq!(*y, 20.0);
        }
        _ => panic!("expected Picture"),
    }
}

#[test]
fn arith_operator_char_set_matches_tokenizer() {
    // 抜本対策の要: `tokenize_expr` が算術演算子トークンを生む記号は
    // すべて `is_arith_operator_char` に含まれること。新演算子を
    // tokenize_expr に足して SoT 更新を忘れると本テストが落ちる
    // (VB5 整数除算 `\` / 累乗 `^` 取りこぼし型バグの再発防止)。
    use ExprTok::*;
    for c in 0u8..=127u8 {
        let ch = c as char;
        let toks = tokenize_expr(&ch.to_string());
        let produces_arith_op = toks.len() == 1
            && matches!(
                toks[0],
                Plus | Minus | Star | Slash | IntDiv | Caret | LParen | RParen
            );
        if produces_arith_op {
            assert!(
                is_arith_operator_char(ch),
                "tokenize_expr は {ch:?} を算術演算子として扱うが is_arith_operator_char が false"
            );
        }
    }
    // 代表演算子が含まれ、比較/連結/英数字は含まれないこと。
    for c in ['+', '-', '*', '/', '\\', '^', '(', ')'] {
        assert!(is_arith_operator_char(c), "{c:?} は算術演算子のはず");
    }
    for c in ['<', '>', '=', '&', 'a', '1', ' ', '.'] {
        assert!(!is_arith_operator_char(c), "{c:?} は算術演算子ではない");
    }
}

#[test]
fn color_command_pushes_setcolor() {
    let src = "\
Color RGB(50, 100, 200)
Color 赤
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert!(matches!(&cmds[0], crate::DrawCmd::SetColor { color } if color == "#3264c8"));
    assert!(matches!(&cmds[1], crate::DrawCmd::SetColor { color } if color == "#ff0000"));
}

#[test]
fn drawwidth_command() {
    let src = "DrawWidth 3\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert!(matches!(&cmds[0], crate::DrawCmd::SetLineWidth(n) if *n == 3.0));
}

#[test]
fn circle_command_pushes_draw_cmd() {
    // `Circle x y r [color]` — 色省略時は SetColor を積まず、円のみ。
    let mut app = App::new();
    execute(&mut app, &event::parse("Circle 100 150 50\n").unwrap()).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert_eq!(cmds.len(), 1, "色省略時は SetColor を積まない");
    assert!(
        matches!(&cmds[0], crate::DrawCmd::Circle { cx, cy, r } if *cx == 100.0 && *cy == 150.0 && *r == 50.0)
    );

    // 色指定あり: RGB(...) は #rrggbb に評価され SetColor → Circle の順で積む。
    let mut app2 = App::new();
    execute(
        &mut app2,
        &event::parse("Circle 200 250 100 RGB(255,0,0)\n").unwrap(),
    )
    .unwrap();
    let cmds = &app2.script_overlay().cmds;
    assert!(
        matches!(&cmds[0], crate::DrawCmd::SetColor { color } if color == "#ff0000"),
        "色指定は SetColor を先に積む: {:?}",
        cmds[0]
    );
    assert!(matches!(&cmds[1], crate::DrawCmd::Circle { r, .. } if *r == 100.0));
}

#[test]
fn circle_radius_evaluates_arithmetic() {
    // 半径が変数入り算術式 `(i * 30)` でも app-aware に評価される (Line と同等)。
    let mut app = App::new();
    app.set_script_var("i".to_string(), "3".to_string());
    execute(
        &mut app,
        &event::parse("Circle 240 240 (i * 30)\n").unwrap(),
    )
    .unwrap();
    assert!(
        matches!(&app.script_overlay().cmds[0], crate::DrawCmd::Circle { r, .. } if *r == 90.0)
    );
}

#[test]
fn oval_command_with_ratio() {
    let mut app = App::new();
    execute(&mut app, &event::parse("Oval 100 150 50 0.5\n").unwrap()).unwrap();
    assert!(matches!(
        &app.script_overlay().cmds[0],
        crate::DrawCmd::Oval { cx, cy, r, ratio }
            if *cx == 100.0 && *cy == 150.0 && *r == 50.0 && (*ratio - 0.5).abs() < 1e-9
    ));
}

#[test]
fn arc_command_angles() {
    let mut app = App::new();
    execute(&mut app, &event::parse("Arc 100 150 50 0 180\n").unwrap()).unwrap();
    assert!(matches!(
        &app.script_overlay().cmds[0],
        crate::DrawCmd::Arc { cx, cy, r, start_deg, end_deg }
            if *cx == 100.0 && *cy == 150.0 && *r == 50.0 && *start_deg == 0.0 && *end_deg == 180.0
    ));
}

#[test]
fn polygon_command_collects_vertices() {
    // `Polygon x1 y1 x2 y2 x3 y3` → 3 頂点。色オプションは無い。
    let mut app = App::new();
    execute(
        &mut app,
        &event::parse("Polygon 0 100 20 20 100 50\n").unwrap(),
    )
    .unwrap();
    match &app.script_overlay().cmds[0] {
        crate::DrawCmd::Polygon { points } => {
            assert_eq!(points, &vec![(0.0, 100.0), (20.0, 20.0), (100.0, 50.0)]);
        }
        other => panic!("expected Polygon, got {other:?}"),
    }
}

#[test]
fn fillstyle_and_fillcolor_state() {
    // `FillStyle 塗りつぶし` → solid(true)、`透明` → solid(false)。
    // `FillColor` は線色 (SetColor) ではなく SetFillColor を積む。
    let mut app = App::new();
    execute(
        &mut app,
        &event::parse("FillStyle 塗りつぶし\nFillColor RGB(0,0,255)\nFillStyle 透明\n").unwrap(),
    )
    .unwrap();
    let cmds = &app.script_overlay().cmds;
    assert!(matches!(&cmds[0], crate::DrawCmd::SetFillSolid(true)));
    assert!(
        matches!(&cmds[1], crate::DrawCmd::SetFillColor { color } if color == "#0000ff"),
        "FillColor は SetFillColor を積む (線色と独立): {:?}",
        cmds[1]
    );
    assert!(matches!(&cmds[2], crate::DrawCmd::SetFillSolid(false)));
}

#[test]
fn battle_anim_frame_visible_during_wait() {
    // SRC 汎用戦闘アニメのフレームループ `PaintPicture; Refresh; ClearPicture; Wait`。
    // 原典の immediate-mode では ClearPicture はバックバッファのみを消し、
    // 画面は直前 Refresh が表示したフレームを Wait 中も保持する。
    // → Wait で中断した時点の overlay には当該フレームの描画が残っていること。
    let src = "\
For i = 1 To 2
  PaintPicture test.bmp 10 20 透過
  Refresh
  ClearPicture
  Wait 1
Next
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // 1 フレーム目の Wait で中断している。
    assert!(app.pending_timer().is_some(), "Wait で中断しているはず");
    let cmds = &app.script_overlay().cmds;
    assert!(
        cmds.iter()
            .any(|c| matches!(c, crate::DrawCmd::Picture { path, .. } if path == "test.bmp")),
        "Wait 中はフレームの PaintPicture が表示に残るべき (ClearPicture は次フレーム用): {cmds:?}"
    );
    // タイマ満了で再開 → 2 フレーム目を描き、また Wait で中断。フレームは 1 枚分のみ。
    app.tick(0.2);
    assert!(
        app.pending_timer().is_some(),
        "2 フレーム目の Wait で再度中断"
    );
    let cmds = &app.script_overlay().cmds;
    let pics = cmds
        .iter()
        .filter(|c| matches!(c, crate::DrawCmd::Picture { .. }))
        .count();
    assert_eq!(
        pics, 1,
        "毎フレーム ClearPicture されるので累積せず 1 枚: {cmds:?}"
    );
}

#[test]
fn pen_state_persists_across_clearpicture() {
    // SRC の ObjColor/ObjFillStyle/ObjFillColor/ObjDrawWidth は ClearPicture を
    // 跨いで保持される。戦闘アニメは Color/FillStyle をループ外で 1 度だけ設定し、
    // 毎フレーム ClearPicture するため、保持されないと図形が既定色/無塗りになる。
    let src = "\
Color RGB(255,0,0)
FillStyle 塗りつぶし
FillColor RGB(0,0,255)
DrawWidth 3
PaintPicture a.bmp 0 0
Refresh
ClearPicture
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let ov = app.script_overlay();
    assert_eq!(
        ov.current_color, "#ff0000",
        "Color は ClearPicture 後も保持"
    );
    assert!(ov.current_fill_solid, "FillStyle 塗りつぶしは保持");
    assert_eq!(ov.current_fill_color, "#0000ff", "FillColor は保持");
    assert_eq!(ov.current_line_width, 3.0, "DrawWidth は保持");
    // 次フレームの描画 push で cmds はクリアされるが、ペン状態は残る。
    execute(&mut app, &event::parse("Circle 10 10 5\n").unwrap()).unwrap();
    let ov = app.script_overlay();
    assert_eq!(ov.current_color, "#ff0000", "新フレームでもペン状態は維持");
    assert!(
        ov.cmds
            .iter()
            .any(|c| matches!(c, crate::DrawCmd::Circle { .. })),
        "新フレームの Circle が積まれる"
    );
    // シーン遷移相当の clear() はペン状態を既定へ戻す。
    app.script_overlay_mut().clear();
    let ov = app.script_overlay();
    assert_eq!(ov.current_color, "", "clear() でペン色リセット");
    assert!(!ov.current_fill_solid, "clear() で塗りリセット");
}

#[test]
fn indexed_var_set_and_read() {
    let src = "\
Set items[1] alpha
Set items[2] beta
Set items[(1 + 2)] gamma
Set i 2
Set v items[i]
Set w items[3]
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("items[1]"), "alpha");
    assert_eq!(app.script_var("items[2]"), "beta");
    assert_eq!(app.script_var("items[3]"), "gamma");
    // 読み出し: items[i] → items[2] → "beta"
    assert_eq!(app.script_var("v"), "beta");
    assert_eq!(app.script_var("w"), "gamma");
}

/// 未定義の indexed 変数を比較条件に使うと、リテラル `配列[1]` が漏れて
/// `<> ""` が常に真になる回帰。未定義 indexed 参照は空文字に解決する。
#[test]
fn undefined_indexed_var_compares_as_empty() {
    let src = "\
If 格納属性表示[1] <> \"\" Then
  Set 結果 NONEMPTY
Else
  Set 結果 EMPTY
EndIf
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("結果"), "EMPTY");
}

/// 値文脈 (Set 値 / Switch) でも未定義 indexed 参照はリテラルを漏らさず
/// 空文字に解決する。`fn_arg_value` を通る経路の網羅。
#[test]
fn undefined_indexed_var_resolves_empty_in_value_contexts() {
    // Set 値: 未定義 indexed → 空文字 (リテラル `配列[9]` を入れない)
    let src1 = "Set x 配列[9]\n";
    let mut app = App::new();
    execute(&mut app, &event::parse(src1).unwrap()).unwrap();
    assert_eq!(app.script_var("x"), "");

    // Switch: 未定義 indexed の値は `Case \"\"` にマッチする
    let src2 = "\
Switch 配列[9]
Case \"\"
  Set hit EMPTY
Case Else
  Set hit OTHER
EndSw
";
    let mut app = App::new();
    execute(&mut app, &event::parse(src2).unwrap()).unwrap();
    assert_eq!(app.script_var("hit"), "EMPTY");

    // 定義済みなら従来どおり値が伝播する
    let src3 = "\
Set 配列[2] beta
Set y 配列[2]
";
    let mut app = App::new();
    execute(&mut app, &event::parse(src3).unwrap()).unwrap();
    assert_eq!(app.script_var("y"), "beta");
}

// ───────────────────────────────────────────────────────────────────
// 抜本的回帰ガード — 変数参照解決のクラス的バグを防ぐ網羅テスト群。
//
// バグのクラス: 「変数参照が、解決できないとリテラルのソーステキストを
// 値文脈へ漏らす / 逆に名前が必要な箇所で値へ過剰展開される」。
// 値解決の集約点は `fn_arg_value`、LHS 解決の集約点は `resolve_lhs_name`。
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

/// `fn_arg_value` の契約を直接固定する。
/// - 未定義 indexed 参照 → 空文字 (リテラル `name[k]` を漏らさない)
/// - 定義済み indexed → 値
/// - 未定義の **裸スカラー** → リテラル温存 (SRC 規約: `If 選択 = ランダム`
///   の `ランダム` のように未定義識別子はリテラルとして比較される)
#[test]
fn fn_arg_value_contract_for_variable_references() {
    let mut app = App::new();
    app.set_script_var("定義[1]".to_string(), "val".to_string());
    app.set_script_var("空[1]".to_string(), String::new());
    app.set_script_var("scal".to_string(), "5".to_string());

    // 未定義 indexed → 空
    assert_eq!(fn_arg_value(&app, "未定義[1]"), "");
    assert_eq!(fn_arg_value(&app, "未定義[xyz]"), "");
    // 定義済み indexed → 値 / 空代入 → 空
    assert_eq!(fn_arg_value(&app, "定義[1]"), "val");
    assert_eq!(fn_arg_value(&app, "空[1]"), "");
    // 定義済みスカラー → 値
    assert_eq!(fn_arg_value(&app, "scal"), "5");
    // 未定義スカラー → リテラル温存 (SRC 規約)
    assert_eq!(fn_arg_value(&app, "ランダム"), "ランダム");
    // クオート文字列 / 数値リテラルはそのまま
    assert_eq!(fn_arg_value(&app, "\"abc\""), "abc");
    assert_eq!(fn_arg_value(&app, "42"), "42");
}

/// 未定義 indexed 参照は **全ての値文脈** で空に解決する (リテラル不漏れ)。
/// 比較 (両極性) / Set 値 / Switch を 1 本で網羅し、新たな値文脈の追加で
/// 抜けが出ても捕まえられるようにする。
#[test]
fn undefined_indexed_reference_is_empty_in_every_value_context() {
    let src = "\
If 配列[9] <> \"\" Then
  Set cmp_ne T
Else
  Set cmp_ne F
EndIf
If 配列[9] = \"\" Then
  Set cmp_eq T
Else
  Set cmp_eq F
EndIf
Set assigned 配列[9]
Switch 配列[9]
Case \"\"
  Set sw EMPTY
Case Else
  Set sw OTHER
EndSw
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("cmp_ne"), "F", "<> \"\" は偽であるべき");
    assert_eq!(app.script_var("cmp_eq"), "T", "= \"\" は真であるべき");
    assert_eq!(app.script_var("assigned"), "", "Set 値は空であるべき");
    assert_eq!(app.script_var("sw"), "EMPTY", "Switch は Case \"\" に一致");
}

/// LHS を取る命令 (`Set` / `Incr` / `Unset`) は indexed 参照を同一の
/// 実キーへ解決する。`resolve_lhs_name` を共有しているかの一貫性ガード。
#[test]
fn indexed_lhs_is_consistent_across_set_incr_unset() {
    let src = "\
Set i 1
Set 表[i] 10
Incr 表[i]
Incr 表[1] 5
Incr 新規[2]
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // Set→Incr→Incr が同じ実キー 表[1] を更新: 10 +1 +5 = 16
    assert_eq!(app.script_var("表[1]"), "16");
    // 未定義 indexed への Incr は 0 起点で定義される
    assert_eq!(app.script_var("新規[2]"), "1");

    // Unset も同じキーを解決して削除する
    let src2 = "\
Set j 3
Set 表[j] hello
Unset 表[3]
Set after IsVarDefined(表[j])
";
    let mut app = App::new();
    execute(&mut app, &event::parse(src2).unwrap()).unwrap();
    assert_eq!(app.script_var("after"), "0", "Unset 表[3] が 表[j] を消す");
}

/// `IsVarDefined` を .eve 実行パス経由で indexed 変数に対し検証する。
/// 引数が値へ過剰展開されると定義済みでも 0 になる回帰のガード。
#[test]
fn isvardefined_indexed_through_execution_path() {
    let src = "\
Set arr[1] x
Set arr[2] \"\"
Set k 1
Set d_value IsVarDefined(arr[1])
Set d_empty IsVarDefined(arr[2])
Set d_undef IsVarDefined(arr[3])
Set d_varidx IsVarDefined(arr[k])
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("d_value"), "1", "値ありは定義済み");
    assert_eq!(app.script_var("d_empty"), "1", "空代入も定義済み");
    assert_eq!(app.script_var("d_undef"), "0", "未代入は未定義");
    assert_eq!(app.script_var("d_varidx"), "1", "変数添字 arr[k]→arr[1]");
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
fn hotpoint_registers_with_eval_coords() {
    let src = "\
Set base 200
Hotpoint UnitA (50 + 10) ($(base) - 25) 50 50
Hotpoint UnitB 100 100 50 50 非表示
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let hp = app.hotpoints();
    assert_eq!(hp.len(), 2);
    assert_eq!(hp[0].name, "UnitA");
    assert_eq!(hp[0].x, 60);
    assert_eq!(hp[0].y, 175);
    assert_eq!(hp[0].w, 50);
    assert!(!hp[0].invisible);
    assert_eq!(hp[1].name, "UnitB");
    assert!(hp[1].invisible);
}

#[test]
fn hotpoint_coords_resolve_loop_variable() {
    // スパロボ戦記 機体選択開始 と同型: ループ変数 `j` を含む算術式
    // `(j * 40 - 35)` を Hotpoint 座標に使う。`j` は裸の識別子なので
    // expand_vars では展開されず、app-aware な eval が必要。
    // 引用符付き数値 `"2"` も座標式に混じる (`("$(i)" * 70 + 15)` 由来)。
    let src = "\
Set j 3
Set i 2
Hotpoint Cell (j * 40 - 35) (\"$(i)\" * 70 + 15) 32 32
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let hp = app.hotpoints();
    assert_eq!(hp.len(), 1);
    // x = 3*40-35 = 85
    assert_eq!(hp[0].x, 85, "ループ変数 j が解決されていない");
    // y = 2*70+15 = 155 (引用符付き "2" を数値として扱う)
    assert_eq!(hp[0].y, 155, "引用符付き数値が解決されていない");
}

#[test]
fn refresh_keeps_hotpoints() {
    // Refresh は画面再描画のみで Hotpoint は維持する (SRC 仕様)
    let src = "\
Hotpoint A 0 0 50 50
Hotpoint B 100 0 50 50
Refresh
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.hotpoints().len(), 2);
}

#[test]
fn clearobj_clears_hotpoints() {
    let src = "\
Hotpoint A 0 0 50 50
ClearObj
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert!(app.hotpoints().is_empty());
}

#[test]
fn wait_click_with_hotpoints_presents_menu_storing_name() {
    let src = "\
Hotpoint A 0 0 50 50
Hotpoint B 100 0 50 50
Hotpoint C 200 0 50 50
Wait Click
Stage $(選択)
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    match app.pending_dialog().unwrap() {
        crate::PendingDialog::Menu {
            options,
            var_name,
            store_value,
            ..
        } => {
            assert_eq!(
                options,
                &vec!["A".to_string(), "B".to_string(), "C".to_string()]
            );
            assert_eq!(var_name, "選択");
            assert!(*store_value);
        }
        _ => panic!("expected Menu"),
    }
    // 2 番目 (B) を選ぶ
    assert!(app.respond_dialog(2));
    assert_eq!(app.script_var("選択"), "B");
    assert_eq!(app.stage(), "B");
}

#[test]
fn hotpoint_click_at_resolves_menu_directly() {
    // Hotpoint クリックで Menu を直接確定できる（番号入力不要）。
    let src = "\
Hotpoint A 0 0 50 50
Hotpoint B 100 0 50 50
Hotpoint C 200 0 50 50
Wait Click
Stage $(選択)
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert!(app.pending_dialog().is_some());
    // B の中央 (125, 25) をクリック → "選択" = "B"
    let consumed = app.handle_input(crate::Input::ClickAt { x: 125, y: 25 });
    assert!(consumed);
    assert_eq!(app.script_var("選択"), "B");
    assert_eq!(app.stage(), "B");
}

#[test]
fn hotpoint_click_outside_does_not_consume() {
    // Hotpoint の外側をクリックしても Menu は閉じない（無反応）。
    let src = "\
Hotpoint A 0 0 50 50
Wait Click
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert!(app.pending_dialog().is_some());
    let consumed = app.handle_input(crate::Input::ClickAt { x: 200, y: 200 });
    assert!(!consumed);
    assert!(app.pending_dialog().is_some());
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
        expand_vars(&app, "Info(ユニットデータ, ブレイバー, 名称)"),
        "ブレイバー"
    );
    assert_eq!(
        expand_vars(&app, "Info(ユニットデータ, ブレイバー, 最大ＨＰ)"),
        "3500"
    );
    assert_eq!(
        expand_vars(&app, "Info(ユニットデータ, ブレイバー, 武器数)"),
        "1"
    );
    assert_eq!(
        expand_vars(&app, "Info(ユニットデータ, ブレイバー, 武器, 1)"),
        "ビームライフル"
    );
    assert_eq!(
        expand_vars(
            &app,
            "Info(ユニットデータ, ブレイバー, 武器, ビームライフル, 攻撃力)"
        ),
        "2500"
    );
    assert_eq!(
        expand_vars(&app, "Info(ユニットデータ, ブレイバー, 武器, 1, 最大射程)"),
        "5"
    );
    // 最大攻撃力 / 最長射程 集約
    assert_eq!(
        expand_vars(&app, "Info(ユニットデータ, ブレイバー, 最大攻撃力)"),
        "2500"
    );
    assert_eq!(
        expand_vars(&app, "Info(ユニットデータ, ブレイバー, 最長射程)"),
        "5"
    );
}

#[test]
fn feature_necessary_skill_gates_is_active() {
    // 必要技能.md §3: 特殊能力 `分身=値 (撃墜数Lv100)` は、撃墜数Lv100 を持たない
    // パイロットでは無効 (is_active=false)、持つエースでは有効。無条件特殊能力 (装甲) は
    // 常に有効。populate_active_features 経由で確認。
    use crate::data::pilot::{PilotData, Sex};
    let make_unit = || crate::data::unit::UnitData {
        abilities: Vec::new(),
        name: "分身機".into(),
        kana_name: "ぶんしんき".into(),
        nickname: "分身機".into(),
        class: "ロボ".into(),
        pilot_num: 1,
        item_num: 0,
        transportation: "陸".into(),
        speed: 5,
        size: Size::M,
        value: 0,
        exp_value: 0,
        hp: 5000,
        en: 100,
        armor: 500,
        mobility: 100,
        adaption: Adaption::parse("AAAA").unwrap(),
        bitmap: String::new(),
        weapons: Vec::new(),
        features: vec![
            ("装甲".into(), "1000".into()),             // 無条件 → 常に有効
            ("分身".into(), "値 (撃墜数Lv100)".into()), // §3 必要技能ゲート付き
        ],
    };
    let make_pilot = |name: &str, feats: Vec<(&str, &str)>| PilotData {
        spirit_commands: Vec::new(),
        name: name.into(),
        nickname: name.into(),
        kana_name: name.into(),
        sex: Sex::Male,
        class: String::new(),
        adaption: Adaption::parse("AAAA").unwrap(),
        exp_value: 0,
        infight: 100,
        shooting: 100,
        hit: 100,
        dodge: 100,
        intuition: 100,
        technique: 100,
        personality: None,
        sp: None,
        bgm: None,
        bitmap: None,
        features: feats
            .into_iter()
            .map(|(n, v)| (n.to_string(), v.to_string()))
            .collect(),
    };
    // ザコ (撃墜数なし) → 分身 無効、装甲 有効。
    let mut app = App::new();
    app.database_mut().units.push(make_unit());
    app.database_mut().pilots.push(make_pilot("ザコ", vec![]));
    let mut zako =
        crate::unit_instance::UnitInstance::new("分身機", "ザコ", crate::Party::Player, 0, 0);
    populate_active_features(&mut zako, &app);
    assert!(
        !crate::feature::has_feature(&zako.active_features, "分身"),
        "撃墜数なしで §3 ゲートの分身が有効になっている"
    );
    assert!(
        crate::feature::has_feature(&zako.active_features, "装甲"),
        "無条件の装甲が無効化された"
    );
    // エース (撃墜数Lv100) → 分身 有効。
    let mut app2 = App::new();
    app2.database_mut().units.push(make_unit());
    app2.database_mut()
        .pilots
        .push(make_pilot("エース", vec![("撃墜数Lv100", "1")]));
    let mut ace =
        crate::unit_instance::UnitInstance::new("分身機", "エース", crate::Party::Player, 0, 0);
    populate_active_features(&mut ace, &app2);
    assert!(
        crate::feature::has_feature(&ace.active_features, "分身"),
        "撃墜数Lv100 を持つエースで分身が無効のまま"
    );
}

#[test]
fn info_returns_features_from_parsed_unit_txt() {
    // unit.txt 由来の features を Info で参照できる。
    // 直接 UnitData を database に積んで、parser を介さずに動作確認。
    let mut app = App::new();
    app.database_mut().units.push(crate::data::unit::UnitData {
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
        expand_vars(
            &app,
            "Info(ユニットデータ, マグナＺ, 特殊能力データ, 全身画像)"
        ),
        "非表示 Anime\\Unit\\EC_MGZ.bmp 96 96"
    );
    // 特殊能力所有 (0/1)
    assert_eq!(
        expand_vars(
            &app,
            "Info(ユニットデータ, マグナＺ, 特殊能力所有, 全身画像)"
        ),
        "1"
    );
    assert_eq!(
        expand_vars(
            &app,
            "Info(ユニットデータ, マグナＺ, 特殊能力所有, 存在しない能力)"
        ),
        "0"
    );
    // 特殊能力数
    assert_eq!(
        expand_vars(&app, "Info(ユニットデータ, マグナＺ, 特殊能力数)"),
        "2"
    );
    // 特殊能力 N (N 番目の名前)
    assert_eq!(
        expand_vars(&app, "Info(ユニットデータ, マグナＺ, 特殊能力, 1)"),
        "全身画像"
    );
    assert_eq!(
        expand_vars(&app, "Info(ユニットデータ, マグナＺ, 特殊能力, 2)"),
        "ブースト"
    );
}

#[test]
fn info_pilot_returns_base_stats() {
    let mut app = App::new();
    let src = "Pilot リオ リオ 男性 超能力者 SSSS 300 160 220 200 220 240 200\n";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(
        expand_vars(&app, "Info(パイロットデータ, リオ, 名称)"),
        "リオ"
    );
    assert_eq!(
        expand_vars(&app, "Info(パイロットデータ, リオ, 性別)"),
        "男性"
    );
    assert_eq!(
        expand_vars(&app, "Info(パイロットデータ, リオ, 格闘)"),
        "160"
    );
    assert_eq!(
        expand_vars(&app, "Info(パイロットデータ, リオ, 技量)"),
        "200"
    );
    assert_eq!(
        expand_vars(&app, "Info(パイロットデータ, リオ, 地形適応)"),
        "SSSS"
    );
}

/// `Info(パイロット, name, 格闘)` がレベルアップ後のスタットを返すことを確認。
/// Pass 45: `info_pilot` が `effective_pilot_data` を使うよう修正。
#[test]
fn info_pilot_reflects_leveled_up_stats() {
    let src = "\
Pilot \"リオ\" リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ブレイバー リアル系 1 0 陸 5 M 1000 100 3500 120 1200 110 AAAA
Place ブレイバー リオ Player 1 1
Exit
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();

    // 初期状態 (level 1): 格闘 = base 100 + lv 1 = 101
    // (VB5 `Pilot.cls:582-593` 準拠でレベル 1 でも +lv 成長する)。
    let initial = expand_vars(&app, "Info(パイロット, リオ, 格闘)")
        .parse::<i32>()
        .unwrap_or(0);
    assert_eq!(initial, 101, "初期格闘は base100 + lv1 = 101 のはず");

    // ExpUp でレベルを上げる (1500 exp → level 4, SRC: 500 exp/level、格闘 += lv)
    let exp_src = "ExpUp ブレイバー 1500\n";
    let exp_stmts = event::parse(exp_src).unwrap();
    execute(&mut app, &exp_stmts).unwrap();

    let leveled = expand_vars(&app, "Info(パイロット, リオ, 格闘)")
        .parse::<i32>()
        .unwrap_or(0);
    assert!(
        leveled > initial,
        "レベルアップ後の格闘 ({leveled}) は初期値 ({initial}) より高いはず"
    );
}

#[test]
fn info_auto_detects_when_kind_omitted() {
    // データ区分省略時は自動判定。
    let mut app = App::new();
    let src = "\
Unit ブレイバー リアル系 1 0 陸 5 M 3000 400 3500 120 1200 110 AAAA
Pilot リオ リオ 男性 超能力者 SSSS 300 160 220 200 220 240 200
";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    // ブレイバー → UnitData として引かれる
    assert_eq!(expand_vars(&app, "Info(ブレイバー, 最大ＨＰ)"), "3500");
    // リオ → PilotData として引かれる
    assert_eq!(expand_vars(&app, "Info(リオ, 格闘)"), "160");
}

#[test]
fn info_map_returns_dimensions() {
    let mut app = App::new();
    let src = "MapSize 10 12\n";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(expand_vars(&app, "Info(マップ, 幅)"), "10");
    assert_eq!(expand_vars(&app, "Info(マップ, 高さ)"), "12");
    assert_eq!(expand_vars(&app, "Info(マップ, 時間帯)"), "昼");
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
fn int_floor_negative_per_src_spec() {
    let app = App::new();
    assert_eq!(expand_vars(&app, "Int(1.8)"), "1");
    assert_eq!(expand_vars(&app, "Int(-8.3)"), "-9");
    assert_eq!(expand_vars(&app, "Int(5)"), "5");
    assert_eq!(expand_vars(&app, "Int(0)"), "0");
}

#[test]
fn eval_evaluates_arithmetic_and_variables() {
    let mut app = App::new();
    app.set_script_var("x".to_string(), "42".to_string());
    assert_eq!(expand_vars(&app, "Eval(1 + 3)"), "4");
    assert_eq!(expand_vars(&app, "Eval(x)"), "42");
    // 数値リテラル
    assert_eq!(expand_vars(&app, "Eval(7)"), "7");
}

#[test]
fn roundup_rounddown_with_digits() {
    let app = App::new();
    assert_eq!(expand_vars(&app, "RoundUp(1.23, 1)"), "1.3");
    assert_eq!(expand_vars(&app, "RoundDown(1.29, 1)"), "1.2");
    assert_eq!(expand_vars(&app, "Round(1.234, 2)"), "1.23");
    assert_eq!(expand_vars(&app, "Round(1.235, 2)"), "1.24");
}

#[test]
fn isvardefined_checks_existence() {
    let mut app = App::new();
    app.set_script_var("撃墜数".to_string(), "5".to_string());
    assert_eq!(expand_vars(&app, "IsVarDefined(撃墜数)"), "1");
    assert_eq!(expand_vars(&app, "IsVarDefined(未定義)"), "0");
}

/// indexed 変数に対する `IsVarDefined`。引数を展開すると配列要素の値に
/// 化けて定義済みでも 0 になる回帰。引数は変数名のまま渡す。
#[test]
fn isvardefined_handles_indexed_vars() {
    let src = "\
Set 配列[1] hello
Set 配列[2] \"\"
Set idx 1
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // 値ありの indexed key
    assert_eq!(expand_vars(&app, "IsVarDefined(配列[1])"), "1");
    // 空文字を明示代入した key も「定義済み」
    assert_eq!(expand_vars(&app, "IsVarDefined(配列[2])"), "1");
    // 未定義の indexed key
    assert_eq!(expand_vars(&app, "IsVarDefined(配列[3])"), "0");
    // 添字が変数: 配列[idx] → 配列[1] → 定義済み
    assert_eq!(expand_vars(&app, "IsVarDefined(配列[idx])"), "1");
}

#[test]
fn isnumeric_detects_numbers() {
    let app = App::new();
    assert_eq!(expand_vars(&app, "IsNumeric(\"125\")"), "1");
    assert_eq!(expand_vars(&app, "IsNumeric(\"abc\")"), "0");
    assert_eq!(expand_vars(&app, "IsNumeric(\"-1.5\")"), "1");
}

#[test]
fn instr_finds_substring_1indexed() {
    let app = App::new();
    assert_eq!(expand_vars(&app, "InStr(\"hello world\", \"world\")"), "7");
    assert_eq!(expand_vars(&app, "InStr(\"hello\", \"xyz\")"), "0");
}

#[test]
fn format_basic_patterns() {
    let app = App::new();
    // 1234.5 → "1,234.50"
    assert_eq!(
        expand_vars(&app, "Format(1234.5, \"##,##0.00\")"),
        "1,234.50"
    );
    assert_eq!(expand_vars(&app, "Format(5, \"0\")"), "5");
}

#[test]
fn lsearch_finds_position_or_returns_zero() {
    // SRC.Sharp `Functions/List.cs::LSearch` 仕様: 見つからなければ 0。
    let mut app = App::new();
    app.set_script_var("xs".into(), "alpha beta gamma".into());
    assert_eq!(expand_vars(&app, "Lsearch(xs, alpha)"), "1");
    assert_eq!(expand_vars(&app, "Lsearch(xs, gamma)"), "3");
    assert_eq!(expand_vars(&app, "Lsearch(xs, delta)"), "0");
}

#[test]
fn lremove_drops_first_match() {
    let mut app = App::new();
    app.set_script_var("xs".into(), "a b c b".into());
    assert_eq!(expand_vars(&app, "Lremove(xs, b)"), "a c b");
}

#[test]
fn line_with_bf_emits_fillrect() {
    let src = "Line 10 20 50 60 BF\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    let has_fillrect = cmds.iter().any(|c| {
        matches!(
            c,
            crate::script_overlay::DrawCmd::FillRect { w, h, .. }
                if *w > 0.0 && *h > 0.0
        )
    });
    assert!(has_fillrect, "expected FillRect, got: {:?}", cmds);
}

#[test]
fn wait_duration_sets_pending_timer_and_suspends() {
    // `Wait 0.5` で pending_timer がセットされ、後続命令の実行が停止する。
    let src = "\
Set first 1
Wait 0.5
Set second 2
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("first"), "1");
    // タイマがセットされ後続命令は未実行
    assert!(app.pending_timer().is_some());
    assert_eq!(app.script_var("second"), "");
    // タイマ満了をシミュレート → resume で残り命令が走る
    app.tick(0.6);
    assert!(app.pending_timer().is_none());
    assert_eq!(app.script_var("second"), "2");
}

#[test]
fn audio_commands_push_pending_audio() {
    use crate::audio::AudioRequest;
    let src = "\
Startbgm Opening.mid
Playsound click.wav
Stopbgm
Keepbgm
PlayVoice voice.ogg
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let reqs = app.take_pending_audio();
    assert_eq!(reqs.len(), 5);
    assert_eq!(
        reqs[0],
        AudioRequest::StartBgm {
            name: "Opening.mid".into()
        }
    );
    assert_eq!(
        reqs[1],
        AudioRequest::PlaySound {
            name: "click.wav".into()
        }
    );
    assert_eq!(reqs[2], AudioRequest::StopBgm);
    assert_eq!(reqs[3], AudioRequest::KeepBgm);
    assert_eq!(
        reqs[4],
        AudioRequest::PlayVoice {
            name: "voice.ogg".into()
        }
    );
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
fn system_variables_resolve_dynamically() {
    // `味方数` / `敵数` / `ターン数` 等のシステム変数が動的に解決される。
    let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Pilot リオ リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ブレイバー Real 1 0 陸 5 M 1000 100 5000 110 1500 100 AAAA
Place ゾルダ ガロ 敵 5 5
Place ブレイバー リオ 味方 1 1
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // If 味方数 = 1 が true で Stage matched が走る
    let src2 = "\
If 味方数 = 1 Then
　Stage matched_player
Else
　Stage no_match
EndIf
If 敵数 = 1 Then
　Set 敵 1
EndIf
";
    let stmts2 = event::parse(src2).unwrap();
    execute(&mut app, &stmts2).unwrap();
    assert_eq!(app.stage(), "matched_player");
    assert_eq!(app.script_var("敵"), "1");
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
    assert!(app.database().pilots.is_empty());
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
    // (7, 7) 中心 / 同マス空きなので (7, 7) そのもの
    assert_eq!((u.x, u.y), (7, 7));
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
fn menu_options_expand_var_references() {
    // Menu / Ask Format 1 の選択肢行に `$(var)` / 関数呼出 を含む場合、
    // 表示時に値展開する。バトルセレクト画面の
    //   Ask "..." キャンセル可
    //   出撃
    //   出撃可能機体確認
    //   $(ボーナス確認選択肢)
    //   End
    // で 3 番目が "ボーナス確認" 等に置換されること。
    let src = "\
Set ボーナス確認選択肢 ボーナス確認（入手済み）
Ask 選択してください
出撃
出撃可能機体確認
$(ボーナス確認選択肢)
End
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    match app.pending_dialog().expect("dialog") {
        crate::PendingDialog::Menu { options, .. } => {
            assert_eq!(options[0], "出撃");
            assert_eq!(options[1], "出撃可能機体確認");
            assert_eq!(options[2], "ボーナス確認（入手済み）");
        }
        _ => panic!("expected Menu"),
    }
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
    assert_eq!(u.x, 9);
    assert_eq!(u.y, 10);
}

#[test]
fn obattle_map_pick_loop_terminates_on_confirm_yes() {
    // スパロボ戦記 OBattle.eve の Ask → If → Confirm → ループ
    // パターンが正しく抜けることを確認。
    let src = "\
Set マップ名称[ランダム] ランダム
Set マップ名称[1] 草原
オリジナル機バトルマップ選択:
Ask マップ名称 マップを選んでください キャンセル可
If 選択 = ランダム or 選択 = \"\" Then
　Set マップ決定 1
Else
　Set マップ決定 選択
Endif
ChangeMap \"map\\map-$(マップ決定).map\"
Confirm このマップでいいですか？
If Not 選択 = 1 Then
　Goto オリジナル機バトルマップ選択
Endif
Set 完了 ok
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // 1) Ask が出ているはず
    match app.pending_dialog().expect("dialog 1") {
        crate::PendingDialog::Menu {
            options,
            store_value,
            ..
        } => {
            assert!(*store_value);
            assert!(options.iter().any(|o| o == "ランダム"));
        }
        other => panic!("expected Menu, got {:?}", other),
    }
    // 2) 先頭の選択肢を選ぶ (1-indexed)。Format 2 なので `選択` には
    //    その要素の添字が入る。
    assert!(app.respond_dialog(1));
    // 3) Confirm が出ているはず
    match app.pending_dialog().expect("dialog 2") {
        crate::PendingDialog::Confirm { question, .. } => {
            assert!(question.contains("このマップ"));
        }
        other => panic!("expected Confirm, got {:?}", other),
    }
    // 4) Yes (internal choice=0 → SRC 仕様 選択="1")
    assert!(app.respond_dialog(0));
    // 5) ループを抜けて Set 完了 ok まで進む
    assert!(app.pending_dialog().is_none());
    assert_eq!(app.script_var("完了"), "ok");
}

#[test]
fn set_resolves_single_bare_identifier_value() {
    // `Set マップ決定 選択` で 選択 の値を マップ決定 に複製。
    let src = "\
Set 選択 草原A
Set マップ決定 選択
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("マップ決定"), "草原A");
}

#[test]
fn ask_format2_uses_array_keys_as_options() {
    // `Ask array message [opts]` 形式 (SRC Ask Format 2)。
    // 先頭引数 (array 名) で indexed-var の key を取って options 化する。
    // 後続行は選択肢として吸い込まれない (= If ... が消費されない)。
    let src = "\
Set マップ名称[ランダム] ランダム
Set マップ名称[1] 草原
Set マップ名称[2] 砂漠
Ask マップ名称 マップを選んでください キャンセル可
If 選択 = ランダム Then
　Stage rand_branch
EndIf
Set 後続 1
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // ダイアログが出ているはず
    let dlg = app.pending_dialog().expect("dialog");
    let crate::PendingDialog::Menu {
        options,
        option_keys,
        ..
    } = dlg
    else {
        panic!("expected Menu");
    };
    // 表示は array 要素の **値**。
    assert!(options.iter().any(|o| o == "ランダム"));
    assert!(options.iter().any(|o| o == "草原"));
    assert!(options.iter().any(|o| o == "砂漠"));
    // option_keys には対応する **添字** が同順で並ぶ (SRC Ask Format 2 は
    // 選んだ要素の添字を `選択` に格納する)。
    assert_eq!(option_keys.len(), options.len());
    assert!(option_keys.iter().any(|k| k == "ランダム"));
    assert!(option_keys.iter().any(|k| k == "1"));
    assert!(option_keys.iter().any(|k| k == "2"));
    // 後続行はまだ実行されていない
    assert_eq!(app.script_var("後続"), "");
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
    app.database_mut().units.push(merged);
    // 分離先のユニットデータ
    app.database_mut().units.push(UnitData {
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
    app.database_mut().units.push(merged);
    app.database_mut().units.push(UnitData {
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
    app.database_mut().unit_instances.remove(def_idx);
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
    app.database_mut().unit_instances.clear();
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
fn specialpower_consumes_sp_and_recoversp_restores() {
    let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Place ゾルダ ガロ 敵 5 5
";
    // PilotData.sp は Pilot 命令経由では設定されないため、直接書き換える。
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    app.database_mut().pilots[0].sp = Some(60);
    // 熱血 (cost 30) → 残り 30
    execute(&mut app, &event::parse("SpecialPower ガロ 熱血\n").unwrap()).unwrap();
    let u = &app.database().unit_instances[0];
    assert_eq!(u.sp_consumed, 30);
    assert!(u.has_condition("熱血"));
    // もう一度 熱血 → 残り 0
    execute(&mut app, &event::parse("SpecialPower ガロ 熱血\n").unwrap()).unwrap();
    assert_eq!(app.database().unit_instances[0].sp_consumed, 60);
    // 魂 (cost 60) → SP 不足で無発動
    execute(&mut app, &event::parse("SpecialPower ガロ 魂\n").unwrap()).unwrap();
    assert_eq!(app.database().unit_instances[0].sp_consumed, 60);
    assert!(!app.database().unit_instances[0].has_condition("魂"));
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
fn set_eval_resolves_dynamic_lhs() {
    // `Set Eval(a) 100` — a に格納された名前を実際の代入先キーとして使う。
    let src = "\
Set name var_x
Set Eval(name) 99
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("var_x"), "99");
}

#[test]
fn sort_indexed_array_ascending() {
    let src = "\
Set arr[1] 3
Set arr[2] 1
Set arr[3] 2
Sort arr 昇順
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // BTreeMap でキーが昇順なので arr[1] < arr[2] < arr[3] の順で再代入される
    assert_eq!(app.script_var("arr[1]"), "1");
    assert_eq!(app.script_var("arr[2]"), "2");
    assert_eq!(app.script_var("arr[3]"), "3");
}

#[test]
fn paintstring_dash_x_uses_center() {
    // `PaintString - 100 text` — x が "-" のとき SRC 480×480 map window
    // の中央 (240) を使用。
    let src = "PaintString - 100 hello\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        crate::script_overlay::DrawCmd::PaintString { x, y, text } => {
            assert_eq!(*x, 240.0);
            assert_eq!(*y, 100.0);
            assert_eq!(text, "hello");
        }
        _ => panic!("not PaintString"),
    }
}

#[test]
fn font_parses_size_and_color_in_any_order() {
    // `Font 14pt Bold #5E5E5E` / `Font #5E5E5E` 等の柔軟な並びを許容する。
    let src = "Font 14pt Bold #5E5E5E\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    match &app.script_overlay().cmds[0] {
        crate::script_overlay::DrawCmd::SetFont {
            family,
            size_pt,
            color,
        } => {
            assert_eq!(*size_pt, 14);
            assert_eq!(color, "#5E5E5E");
            assert!(family.starts_with("bold "), "family={family}");
        }
        _ => panic!("not SetFont"),
    }
}

#[test]
fn hotpointstring_renders_and_registers_hotpoint() {
    // `HotpointString x y text` は PaintString に加えて Hotpoint を登録する。
    let src = "HotpointString 100 200 START\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let hp = app.hotpoints();
    assert_eq!(hp.len(), 1);
    assert_eq!(hp[0].name, "START");
    assert_eq!(hp[0].x, 100);
    // y は文字列上端を指すので、Hotpoint 矩形は y - line_height = 200 - 22 = 178
    assert_eq!(hp[0].y, 178);
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
fn nickname_looks_up_pilot_first() {
    let mut app = App::new();
    let src = "\
Pilot リオ リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(expand_vars(&app, "Nickname(リオ)"), "リオ");
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
    assert_eq!(expand_vars(&app, "Party(ガロ)"), "敵");
    assert_eq!(expand_vars(&app, "Party(ゾルダ)"), "敵");
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
fn arithmetic_eval_in_hotpoint() {
    let src = "\
Set n 100
Hotpoint X (($(n) + 50) * 2 - 100) 0 30 30
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    // (100 + 50) * 2 - 100 = 200
    assert_eq!(app.hotpoints()[0].x, 200);
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
fn list_and_llength_lindex_functions() {
    let src = "\
Set xs List(a, b, c, d)
Set n Llength($(xs))
Set v Lindex($(xs), 3)
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("xs"), "a b c d");
    assert_eq!(app.script_var("n"), "4");
    assert_eq!(app.script_var("v"), "c");
}

#[test]
fn list_evaluates_parenthesized_arithmetic_elements() {
    // スパロボ戦記 `Set 配置場所[8] List(int(幅/2), (高さ - 3))` 相当。
    // 関数評価後に残る括弧式の List 要素を数値化する。
    let src = "\
Set 高さ 27
Set 座標 List(8, (高さ - 3))
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("座標"), "8 24");
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
fn wait_click_pauses_with_dialog() {
    let src = "\
Wait Click
Message after
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert!(app.pending_dialog().is_some());
    assert!(app.messages().is_empty());
    // 応答すると Message after が動く
    app.respond_dialog(0);
    assert_eq!(app.messages(), &["after".to_string()]);
}

#[test]
fn case_insensitive_function_names() {
    let src = "\
Set xs \"a b c\"
Set n1 Llength($(xs))
Set n2 LLength($(xs))
Set n3 LENGTH($(xs))
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("n1"), "3");
    assert_eq!(app.script_var("n2"), "3");
    assert_eq!(app.script_var("n3"), "3");
}

#[test]
fn fn_args_resolve_bare_identifier_to_variable() {
    let src = "\
Set foo \"a b c d\"
Set n Llength(foo)
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("n"), "4");
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
fn print_is_alias_of_message() {
    let mut app = App::new();
    execute(&mut app, &event::parse("Print hello\n").unwrap()).unwrap();
    assert_eq!(app.messages(), &["hello".to_string()]);
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
fn area_function_returns_terrain_class() {
    // `Area(unit)` 関数はユニットのいる地形クラスを返す。
    // 地形が設定されていない場合は空文字またはデフォルト値を返す。
    let mut app = App::new();
    setup_two_units(&mut app);
    let stmts = event::parse("Set a Area(ブレイバー)\n").unwrap();
    execute(&mut app, &stmts).unwrap();
    // マップ地形未設定なので空文字を返す (エラーにならないことを確認)
    let _ = app.script_var("a"); // crash しなければ OK
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

/// テスト用に `database().items` (アイテム DB) へ名称のみのアイテムを登録する。
/// `ItemData` は Default を持たないため全フィールドを明示する。
fn push_item_named(app: &mut App, name: &str) {
    app.database_mut().items.push(crate::data::item::ItemData {
        name: name.to_string(),
        class: String::new(),
        part: String::new(),
        hp_mod: 0,
        en_mod: 0,
        armor_mod: 0,
        mobility_mod: 0,
        speed_mod: 0,
        comment: String::new(),
        features: Vec::new(),
    });
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
    let out = expand_vars(
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

#[test]
fn terrainid_function() {
    let src = "\
MapSize 4 4
SetTile 2 1 3
Set t TerrainId(2, 1)
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("t"), "3");
}

#[test]
fn paintstring_pushes_to_overlay() {
    let src = "\
Font ゴシック 14pt #ffe0e0
PaintString 100 200 こんにちは
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert_eq!(cmds.len(), 2);
    match &cmds[0] {
        crate::DrawCmd::SetFont {
            family,
            size_pt,
            color,
        } => {
            assert_eq!(family, "ゴシック");
            assert_eq!(*size_pt, 14);
            assert_eq!(color, "#ffe0e0");
        }
        _ => panic!("expected SetFont"),
    }
    match &cmds[1] {
        crate::DrawCmd::PaintString { x, y, text } => {
            assert_eq!(*x, 100.0);
            assert_eq!(*y, 200.0);
            assert_eq!(text, "こんにちは");
        }
        _ => panic!("expected PaintString"),
    }
}

#[test]
fn paintstring_coord_expression_not_leaked_into_text() {
    // 座標が関数呼び出し / 変数を含む括弧式の場合でも、それを text に
    // 流出させない (画面に式がそのまま描画されるバグの再発防止)。
    // スパロボ戦記 String.eve VibrationString と同型。
    let src = "\
Set k 2
PaintString (k * 10 + 5) (k * 20) ヴァイブ
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let ps = app
        .script_overlay()
        .cmds
        .iter()
        .find_map(|c| match c {
            crate::DrawCmd::PaintString { x, y, text } => Some((*x, *y, text.clone())),
            _ => None,
        })
        .expect("PaintString が無い");
    // text は式ではなく "ヴァイブ" のみ
    assert_eq!(ps.2, "ヴァイブ");
    // 座標は評価される (k=2 → x=25, y=40)
    assert_eq!(ps.0, 25.0);
    assert_eq!(ps.1, 40.0);
}

#[test]
fn line_command_pushes_draw_cmd() {
    let src = "Line 0 0 100 100 #ff0000\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert_eq!(cmds.len(), 2);
    assert!(matches!(&cmds[0], crate::DrawCmd::SetColor { color } if color == "#ff0000"));
    assert!(
        matches!(&cmds[1], crate::DrawCmd::Line { x1, y1, x2, y2 } if *x1 == 0.0 && *y1 == 0.0 && *x2 == 100.0 && *y2 == 100.0)
    );
}

#[test]
fn refresh_keeps_accumulated_drawing() {
    // 元 SRC `Refresh` は present のみで描画内容はクリアしない
    // (クリアは Cls / ClearPicture / ClearObj の役割)。
    // `draw / Refresh / draw` で全描画が蓄積されたまま残る。
    let src = "\
PaintString 10 10 a
PaintString 20 20 b
Refresh
PaintString 30 30 c
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    // Refresh はクリアしないので a / b / c すべて残る
    assert_eq!(cmds.len(), 3);
    assert!(matches!(&cmds[2], crate::DrawCmd::PaintString { text, .. } if text == "c"));
}

#[test]
fn fadeout_pushes_fade() {
    let src = "FadeOut 30\n";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let cmds = &app.script_overlay().cmds;
    assert_eq!(cmds.len(), 1);
    assert!(matches!(&cmds[0], crate::DrawCmd::Fade { alpha, .. } if *alpha > 0.0));
}

/// 回帰: `WhiteIn` は白フェードアウトを露出 (除去) する。引数なし `WhiteIn` が
/// 全画面白を積みっぱなしにすると「白いマップ」で操作不能になる
/// (東方夢想伝: タイトルテロップ末尾の `WhiteIn`)。
#[test]
fn whitein_clears_whiteout_overlay() {
    let stmts = event::parse("WhiteOut 255\nWhiteIn\n").unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let has_white = app
        .script_overlay()
        .cmds
        .iter()
        .any(|c| matches!(c, crate::DrawCmd::Fade { color, .. } if color == "#ffffff"));
    assert!(
        !has_white,
        "WhiteIn 後に白 Fade が残存: {:?}",
        app.script_overlay().cmds
    );
}

/// 回帰: `FadeIn` は黒フェードアウトを露出 (除去) する。
#[test]
fn fadein_clears_fadeout_overlay() {
    let stmts = event::parse("FadeOut 60\nFadeIn\n").unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    let has_black = app
        .script_overlay()
        .cmds
        .iter()
        .any(|c| matches!(c, crate::DrawCmd::Fade { color, .. } if color == "#000000"));
    assert!(
        !has_black,
        "FadeIn 後に黒 Fade が残存: {:?}",
        app.script_overlay().cmds
    );
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
fn incr_command() {
    let src = "\
Set x 5
Incr x
Incr x 3
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("x"), "9");
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

#[test]
fn parse_i32_at_accepts_arithmetic_expression() {
    // 平文の整数はそのまま解釈する。
    assert_eq!(parse_i32_at("5", 1).unwrap(), 5);
    assert_eq!(parse_i32_at("-3", 1).unwrap(), -3);
    // `(味方レベル平均値 - 1)` 等が展開後に括弧付き算術として残るケース。
    assert_eq!(parse_i32_at("(4 - 1)", 1).unwrap(), 3);
    assert_eq!(parse_i32_at("(2 + 3 * 4)", 1).unwrap(), 14);
    // 真に不正な値は従来どおりエラー。
    assert!(parse_i32_at("abc", 1).is_err());
}

#[test]
fn parse_i64_at_accepts_arithmetic_expression() {
    assert_eq!(parse_i64_at("100", 1).unwrap(), 100);
    assert_eq!(parse_i64_at("(10 - 4)", 1).unwrap(), 6);
    assert!(parse_i64_at("xyz", 1).is_err());
}
