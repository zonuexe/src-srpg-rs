//! 式の評価・変数・組み込み関数のテスト。

use super::*;

/// ラベルが無い名前は関数として実行せずリテラルのまま残す。
#[test]
fn unknown_function_name_is_left_literal() {
    let stmts = event::parse("Set z 未定義関数(1)\nStage done\n").unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("z"), "未定義関数(1)");
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
fn set_paren_with_non_numeric_atom_is_evaluated_as_expression() {
    // 括弧で囲った値は式として評価する (原典 EvalTerm)。中身の `こんにちは` は未定義変数
    // なので名前そのものの文字列になり、0 に潰れない。括弧は残らない (C# oracle で確認)。
    let app = run_script("Set msg (こんにちは)\n");
    assert_eq!(app.script_var("msg"), "こんにちは");
}

#[test]
fn set_non_parenthesized_arithmetic_kept_verbatim() {
    // 括弧で囲われていない値は従来どおり式評価しない
    // (`Set 型番 RX-78-2` を `-53` と誤評価しないため)。
    let app = run_script("Set 型番 78-2\n");
    assert_eq!(app.script_var("型番"), "78-2");
}

#[test]
fn paren_arith_missing_operand_defaults_to_zero() {
    // SRC: 未定義数値は 0。Info(...) 等が空に解決して `)` 直前に二項演算子が
    // 残っても括弧算術が評価できること (スパロボ戦記 AlphaSecond の
    // `次のレベル(500 - )` / `(500 * 0 - 500 + )` 由来の生式表示の防止)。
    let app = App::new();
    assert_eq!(eval::value_as_string(&app, "(500 - )"), "500");
    assert_eq!(eval::value_as_string(&app, "(500 * 0 - 500 + )"), "-500");
    // 通常の括弧算術は従来どおり評価される (回帰防止)。
    assert_eq!(eval::value_as_string(&app, "(2 + 3)"), "5");
}

#[test]
fn quoted_indexed_var_stays_literal() {
    // 回帰: クオート内の `name[expr]` は、たまたま同名の配列変数が定義済みでも
    // **値に化けず literal のまま**にする (D データロードの行検出が
    // `Instr(v,"設定[パイロット一覧]")` のリテラル破壊で失敗していた真因)。
    let mut app = App::new();
    app.set_script_var("設定[パイロット一覧]".to_string(), "VAL".to_string());
    // クオート内は literal。
    assert_eq!(
        eval::value_as_string(&app, "\"設定[パイロット一覧]\""),
        "設定[パイロット一覧]"
    );
    // クオート外は従来どおり値に展開。
    assert_eq!(eval::value_as_string(&app, "設定[パイロット一覧]"), "VAL");
    // `$(...)` 明示補間はクオート内でも従来どおり展開。
    app.set_script_var("x".to_string(), "Y".to_string());
    assert_eq!(eval::value_as_string(&app, "\"$(x)\""), "Y");
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
    // `Place` は内部座標 (1, 1)。`X()` / `Y()` は原典どおり 1 始まりで返す。
    assert_eq!(app.script_var("gx"), "2");
    assert_eq!(app.script_var("gy"), "2");
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
    // A,B はレベル 1 → 平均 1 (件数 2 ではない) → C は SRC (1, 5) = 内部 (0, 4) に生成。
    let c = app
        .database()
        .unit_instances
        .iter()
        .find(|u| u.unit_data_name == "C")
        .expect("C が生成されていない");
    assert_eq!(
        (c.x, c.y),
        (0, 4),
        "味方レベル平均値 が座標式で解決されていない"
    );
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
/// 空文字に解決する。引数評価 (`eval::param_value`) を通る経路の網羅。
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

/// 引数 1 つの評価 (`eval::param_value`、原典 `GetValueAsString(param, is_term)`) の
/// 変数参照まわりの契約を直接固定する。
/// - 未定義 indexed 参照 → 空文字 (リテラル `name[k]` を漏らさない)
/// - 定義済み indexed → 値
/// - 未定義の **裸スカラー** → リテラル温存 (SRC 規約: `If 選択 = ランダム`
///   の `ランダム` のように未定義識別子はリテラルとして比較される)
#[test]
fn param_value_contract_for_variable_references() {
    let mut app = App::new();
    app.set_script_var("定義[1]".to_string(), "val".to_string());
    app.set_script_var("空[1]".to_string(), String::new());
    app.set_script_var("scal".to_string(), "5".to_string());

    // 未定義 indexed → 空
    assert_eq!(eval::param_value(&app, "未定義[1]"), "");
    assert_eq!(eval::param_value(&app, "未定義[xyz]"), "");
    // 定義済み indexed → 値 / 空代入 → 空
    assert_eq!(eval::param_value(&app, "定義[1]"), "val");
    assert_eq!(eval::param_value(&app, "空[1]"), "");
    // 定義済みスカラー → 値
    assert_eq!(eval::param_value(&app, "scal"), "5");
    // 未定義スカラー → リテラル温存 (SRC 規約)
    assert_eq!(eval::param_value(&app, "ランダム"), "ランダム");
    // クオート文字列 / 数値リテラルはそのまま
    assert_eq!(eval::param_value(&app, "\"abc\""), "abc");
    assert_eq!(eval::param_value(&app, "42"), "42");
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
    app.database_mut().catalog.units.push(make_unit());
    app.database_mut()
        .catalog
        .pilots
        .push(make_pilot("ザコ", vec![]));
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
    app2.database_mut().catalog.units.push(make_unit());
    app2.database_mut()
        .catalog
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
fn info_pilot_returns_base_stats() {
    let mut app = App::new();
    let src = "Pilot リオ リオ 男性 超能力者 SSSS 300 160 220 200 220 240 200\n";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(
        eval::value_as_string(&app, "Info(パイロットデータ, リオ, 名称)"),
        "リオ"
    );
    assert_eq!(
        eval::value_as_string(&app, "Info(パイロットデータ, リオ, 性別)"),
        "男性"
    );
    assert_eq!(
        eval::value_as_string(&app, "Info(パイロットデータ, リオ, 格闘)"),
        "160"
    );
    assert_eq!(
        eval::value_as_string(&app, "Info(パイロットデータ, リオ, 技量)"),
        "200"
    );
    assert_eq!(
        eval::value_as_string(&app, "Info(パイロットデータ, リオ, 地形適応)"),
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
    let initial = eval::value_as_string(&app, "Info(パイロット, リオ, 格闘)")
        .parse::<i32>()
        .unwrap_or(0);
    assert_eq!(initial, 101, "初期格闘は base100 + lv1 = 101 のはず");

    // ExpUp でレベルを上げる (1500 exp → level 4, SRC: 500 exp/level、格闘 += lv)
    let exp_src = "ExpUp ブレイバー 1500\n";
    let exp_stmts = event::parse(exp_src).unwrap();
    execute(&mut app, &exp_stmts).unwrap();

    let leveled = eval::value_as_string(&app, "Info(パイロット, リオ, 格闘)")
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
    assert_eq!(
        eval::value_as_string(&app, "Info(ブレイバー, 最大ＨＰ)"),
        "3500"
    );
    // リオ → PilotData として引かれる
    assert_eq!(eval::value_as_string(&app, "Info(リオ, 格闘)"), "160");
}

#[test]
fn info_map_returns_dimensions() {
    let mut app = App::new();
    let src = "MapSize 10 12\n";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(eval::value_as_string(&app, "Info(マップ, 幅)"), "10");
    assert_eq!(eval::value_as_string(&app, "Info(マップ, 高さ)"), "12");
    assert_eq!(eval::value_as_string(&app, "Info(マップ, 時間帯)"), "昼");
}

#[test]
fn int_floor_negative_per_src_spec() {
    let app = App::new();
    assert_eq!(eval::value_as_string(&app, "Int(1.8)"), "1");
    assert_eq!(eval::value_as_string(&app, "Int(-8.3)"), "-9");
    assert_eq!(eval::value_as_string(&app, "Int(5)"), "5");
    assert_eq!(eval::value_as_string(&app, "Int(0)"), "0");
}

#[test]
fn eval_evaluates_arithmetic_and_variables() {
    let mut app = App::new();
    app.set_script_var("x".to_string(), "42".to_string());
    assert_eq!(eval::value_as_string(&app, "Eval(1 + 3)"), "4");
    assert_eq!(eval::value_as_string(&app, "Eval(x)"), "42");
    // 数値リテラル
    assert_eq!(eval::value_as_string(&app, "Eval(7)"), "7");
}

#[test]
fn roundup_rounddown_with_digits() {
    let app = App::new();
    assert_eq!(eval::value_as_string(&app, "RoundUp(1.23, 1)"), "1.3");
    assert_eq!(eval::value_as_string(&app, "RoundDown(1.29, 1)"), "1.2");
    assert_eq!(eval::value_as_string(&app, "Round(1.234, 2)"), "1.23");
    assert_eq!(eval::value_as_string(&app, "Round(1.235, 2)"), "1.24");
}

#[test]
fn isvardefined_checks_existence() {
    let mut app = App::new();
    app.set_script_var("撃墜数".to_string(), "5".to_string());
    assert_eq!(eval::value_as_string(&app, "IsVarDefined(撃墜数)"), "1");
    assert_eq!(eval::value_as_string(&app, "IsVarDefined(未定義)"), "0");
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
    assert_eq!(eval::value_as_string(&app, "IsVarDefined(配列[1])"), "1");
    // 空文字を明示代入した key も「定義済み」
    assert_eq!(eval::value_as_string(&app, "IsVarDefined(配列[2])"), "1");
    // 未定義の indexed key
    assert_eq!(eval::value_as_string(&app, "IsVarDefined(配列[3])"), "0");
    // 添字が変数: 配列[idx] → 配列[1] → 定義済み
    assert_eq!(eval::value_as_string(&app, "IsVarDefined(配列[idx])"), "1");
}

#[test]
fn isnumeric_detects_numbers() {
    let app = App::new();
    assert_eq!(eval::value_as_string(&app, "IsNumeric(\"125\")"), "1");
    assert_eq!(eval::value_as_string(&app, "IsNumeric(\"abc\")"), "0");
    assert_eq!(eval::value_as_string(&app, "IsNumeric(\"-1.5\")"), "1");
}

#[test]
fn instr_finds_substring_1indexed() {
    let app = App::new();
    assert_eq!(
        eval::value_as_string(&app, "InStr(\"hello world\", \"world\")"),
        "7"
    );
    assert_eq!(
        eval::value_as_string(&app, "InStr(\"hello\", \"xyz\")"),
        "0"
    );
}

#[test]
fn format_basic_patterns() {
    let app = App::new();
    // 1234.5 → "1,234.50"
    assert_eq!(
        eval::value_as_string(&app, "Format(1234.5, \"##,##0.00\")"),
        "1,234.50"
    );
    assert_eq!(eval::value_as_string(&app, "Format(5, \"0\")"), "5");
}

#[test]
fn lsearch_finds_position_or_returns_zero() {
    // SRC.Sharp `Functions/List.cs::LSearch` 仕様: 見つからなければ 0。
    let mut app = App::new();
    app.set_script_var("xs".into(), "alpha beta gamma".into());
    assert_eq!(eval::value_as_string(&app, "Lsearch(xs, alpha)"), "1");
    assert_eq!(eval::value_as_string(&app, "Lsearch(xs, gamma)"), "3");
    assert_eq!(eval::value_as_string(&app, "Lsearch(xs, delta)"), "0");
}

#[test]
fn lremove_drops_first_match() {
    let mut app = App::new();
    app.set_script_var("xs".into(), "a b c b".into());
    assert_eq!(eval::value_as_string(&app, "Lremove(xs, b)"), "a c b");
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
fn nickname_looks_up_pilot_first() {
    let mut app = App::new();
    let src = "\
Pilot リオ リオ 男性 超能力者 AAAA 100 100 100 100 100 100 100
";
    let stmts = event::parse(src).unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(eval::value_as_string(&app, "Nickname(リオ)"), "リオ");
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
fn parse_int_reads_evaluated_numbers_only() {
    // データ定義形式の数値欄は評価済みの値を数値として読むだけ (再評価しない)。
    assert_eq!(parse_i32_at("5", 1).unwrap(), 5);
    assert_eq!(parse_i32_at("-3", 1).unwrap(), -3);
    assert_eq!(parse_i64_at("2.5", 1).unwrap(), 2);
    assert!(parse_i32_at("abc", 1).is_err());
    assert!(parse_i64_at("(10 - 4)", 1).is_err());
}

/// B6: 数値引数・括弧算術・数学関数の引数でもシステム変数を解決する。旧実装の
/// `eval_numeric_atoms` はスクリプト変数しか見ず、`(味方数 + 1)` が評価されなかった。
#[test]
fn numeric_contexts_resolve_system_variables() {
    let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Place ゾルダ ガロ 味方 5 5
Money 500
";
    let stmts = event::parse(src).unwrap();
    let mut app = App::new();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(eval::value_as_long(&app, "(味方数 + 1)"), 2);
    assert_eq!(eval::value_as_string(&app, "(資金 + 100)"), "600");
    let stmts = event::parse("Set m Max(資金 - 100, 0)\nSet p (味方数 * 10)\n").unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("m"), "400");
    assert_eq!(app.script_var("p"), "10");
}

// ============================================================
//  コマンド引数の評価 (原典 GetArgAsString)
// ============================================================

/// コマンド引数は 1 回だけ評価される。評価結果がたまたま別の変数名と同じでも、
/// もう一度その変数を引かない (旧実装は fn_arg_value で 2 度解決し得た)。
#[test]
fn command_args_are_evaluated_once() {
    let src = "\
Set a b
Set b 5
Call sub a \"Len(x)\" (1 + 2)
Exit
sub:
Set got1 Args(1)
Set got2 Args(2)
Set got3 Args(3)
Return
";
    let mut app = App::new();
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    assert_eq!(app.script_var("got1"), "b");
    // クォート文字列はリテラル (中の関数呼び出しの形は評価しない)。
    assert_eq!(app.script_var("got2"), "Len(x)");
    assert_eq!(app.script_var("got3"), "3");
}

/// 組み込み関数の引数は 1 つずつ評価してから渡す (原典 `GetValueAsString(param, is_term)`)。
/// 旧実装は `$(...)` と関数呼び出しだけを文字列展開しており、空白を含む式の引数
/// (`n + m`) は式のまま、変数に入れたユニット名は名前のまま関数に渡っていた。
#[test]
fn function_params_are_evaluated_like_the_original() {
    let src = "\
Pilot ガロ ガロ 男性 超能力者 AAAA 100 100 100 100 100 100 100
Unit ゾルダ Mass 1 0 陸 5 M 1000 100 2400 80 900 80 BBBB
Place ゾルダ ガロ 敵 5 5
Set n 10
Set m 3
Set lst \"x y z\"
Set idx 2
Set who ガロ
Set a Len(n + m)
Set b Lindex(lst, idx + 1)
Set c List(n + 1, \"a b\")
Set d HP(who)
Set e IsVarDefined(n)
";
    let mut app = App::new();
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    assert_eq!(app.script_var("a"), "2");
    assert_eq!(app.script_var("b"), "z");
    assert_eq!(app.script_var("c"), "11 a b");
    assert_eq!(app.script_var("d"), "2400");
    assert_eq!(app.script_var("e"), "1");
}
