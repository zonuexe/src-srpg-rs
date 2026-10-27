//! `.eve` の対話命令 (Talk / Ask / Menu / Input / Confirm / Wait) のテスト。

use super::*;

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
fn print_is_alias_of_message() {
    let mut app = App::new();
    execute(&mut app, &event::parse("Print hello\n").unwrap()).unwrap();
    assert_eq!(app.messages(), &["hello".to_string()]);
}

/// Talk 本文は `$(...)` だけを展開する (原典 FormatMessage)。関数呼び出しの形をした
/// 文字列はそのまま表示し、未定義変数の `$(...)` は変数名になる。
#[test]
fn talk_body_only_replaces_sub_expressions() {
    let src = "\
Set n 3
Talk 番頭さん
残り$(n)部屋 Len(abc)
$(未定義)
End
";
    let mut app = App::new();
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    match app.pending_dialog().unwrap() {
        crate::PendingDialog::Talk { body, .. } => {
            assert!(body.contains("残り3部屋 Len(abc)"), "{body:?}");
            assert!(body.contains("未定義"), "{body:?}");
        }
        _ => panic!("expected Talk dialog"),
    }
}
