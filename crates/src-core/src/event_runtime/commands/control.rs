//! `.eve` コマンド: 制御フロー (分岐・ループ・サブルーチン・ジャンプ)。

use super::*;

pub(super) fn exec(app: &mut App, c: &Cmd) -> Option<Result<usize, ScriptError>> {
    Some(match c.lname.as_str() {
        "goto" => exec_goto(app, c),
        "if" => exec_if(app, c),
        "elseif" => exec_elseif(app, c),
        "else" => exec_else(app, c),
        "endif" => exec_endif(app, c),
        "end" => exec_end(app, c),
        "exit" => exec_exit(app, c),
        "return" => exec_return(app, c),
        "call" => exec_call(app, c),
        "switch" => exec_switch(app, c),
        "case" | "caseelse" => exec_case(app, c),
        "endsw" => exec_endsw(app, c),
        "for" => exec_for(app, c),
        "next" => exec_next(app, c),
        "foreach" => exec_foreach(app, c),
        "skip" => exec_skip(app, c),
        "do" => exec_do(app, c),
        "loop" => exec_loop(app, c),
        "break" => exec_break(app, c),
        "continue" => exec_continue(app, c),
        "require" => exec_require(app, c),
        _ => return None,
    })
}

fn exec_goto(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    let target = expect_arg(xargs, 0, line, "Goto <label>")?;
    jump_to(app, pc, target, line)
}

fn exec_if(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        args,
        raw_args,
        line,
        pc,
        stmts,
        labels,
        ..
    } = *c;
    // `If cond Then` (ブロック) / `If cond Then stmt` / `If cond Exit` / `If cond Goto label`。
    // 本移植は `Then` を省いた `If lhs op rhs stmt` も受け付ける。条件が偽なら対応する
    // ElseIf / Else / EndIf までスキップする。
    // 条件と本体の境界は引数トークンで判定し、条件は原文トークン (クォート付き) を
    // 統一評価器 (`eval::if_condition`) で評価する (原典 `ExecIfCmd`)。
    let (cond, body) = split_if_cond_body(args);
    let body_start = args.len() - body.len();
    let cond = condition_with_calls(app, &raw_args[..cond.len()].join(" "));
    if body.is_empty() {
        if cond {
            Ok(pc + 1)
        } else {
            skip_to_else_or_endif(app, pc, stmts, line)
        }
    } else if cond {
        // 同じ行の本体 (`Exit` / `Goto label` / 任意のコマンド) を実行する。
        exec_command_pc(
            app,
            &args[body_start],
            &args[body_start + 1..],
            &raw_args[body_start + 1..],
            line,
            pc,
            stmts,
            labels,
        )
    } else {
        Ok(pc + 1)
    }
}

fn exec_elseif(_app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        line, pc, stmts, ..
    } = *c;
    // 直前の Then 節を実行し終えたら ElseIf 以降は全て EndIf までスキップ。
    skip_to_endif(pc, stmts, line)
}

fn exec_else(_app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        line, pc, stmts, ..
    } = *c;
    skip_to_endif(pc, stmts, line)
}

fn exec_endif(_app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    Ok(pc + 1)
}

fn exec_end(_app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // Talk 外の End はサブルーチン終端マーカー。本実装ではトップレベル
    // 実行を続行（Return 相当）するだけ。
    Ok(pc + 1)
}

fn exec_exit(_app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { stmts, .. } = *c;
    // 元 SRC `ExitCmd`: 現イベントを即終了。
    // PC をスクリプト末尾に飛ばして execute ループを抜ける。
    Ok(stmts.len())
}

fn exec_return(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, stmts, .. } = *c;
    // Call 中なら戻りアドレスへ + 呼び出し元の Args を復元。
    // それ以外は Exit と同じ。
    //
    // 戻り値 (`Return <value>`) を保存する。
    // `Call(<label>)` 形式の条件式評価 (`evaluate_command_condition`) が
    // `call_label_sync_for_condition` 経由で読み取る。
    // 返り値は **呼び出された側のスコープで** 評価する。
    // `Returnコマンド.md`: 「*value* の値がサブルーチンコールの返り値」。
    // ここで畳み込まないと `Return (n + 2)` の `n` (サブルーチン
    // ローカル変数) が呼び出し元では解決できず、式のまま漏れる。
    let retval = xargs.first().cloned().unwrap_or_default();
    let retval = eval_arith_value(app, &retval).unwrap_or(retval);
    app.set_last_return_value(retval);
    if let Some((ret, saved)) = app.pop_call_return() {
        restore_call_args(app, saved);
        return Ok(ret);
    }
    Ok(stmts.len())
}

fn exec_call(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `Call label [arg1 arg2 ...]`
    // 戻りアドレスは現命令の次。Args(1).. をシナリオ変数として束縛。
    let target = expect_arg(xargs, 0, line, "Call <label> [args...]")?;
    let new_args: Vec<String> = xargs.iter().skip(1).cloned().collect();
    let saved = enter_call_args(app, &new_args);
    app.push_call_return(pc + 1, saved);
    jump_to(app, pc, target, line)
}

fn exec_switch(_app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs,
        line,
        pc,
        stmts,
        ..
    } = *c;
    // `Switch value` の value 解決:
    //   - 単一トークン (`Switch 選択`) は裸識別子を script_var として
    //     解決する。SRC では Switch の値は式評価される。
    //   - 複数トークン (`Switch $(a) + 1`) は join したものを value にする。
    // SRC.Sharp 準拠: 引数なしはエラー。
    if xargs.is_empty() {
        return Err(err(line, "Switchコマンドの引数の数が違います"));
    }
    let value = if xargs.len() == 1 {
        xargs[0].to_string()
    } else {
        xargs.join(" ")
    };
    find_matching_case(pc, stmts, &value, line)
}

fn exec_case(_app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        line, pc, stmts, ..
    } = *c;
    // 直前ケース本体の末尾に到達 → 同階層の EndSw までスキップ。
    skip_to_endsw(pc, stmts, line)
}

fn exec_endsw(_app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    Ok(pc + 1)
}

fn exec_for(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs,
        line,
        pc,
        stmts,
        ..
    } = *c;
    // `For var = start To end [Step n]`
    // xargs: [var, "=", start, "To", end, ("Step", n)?]
    if xargs.len() < 5 || xargs[1] != "=" || !xargs[3].eq_ignore_ascii_case("to") {
        return Err(err(line, "For 構文: For var = start To end [Step n]"));
    }
    let var = xargs[0].clone();
    // SRC は `For i = 1 To Info(...)` で Info() が "" を返すと
    // ループを実行せずに次へ進む。空文字 / 非数値は 0 と解釈してその
    // 旨を反映する (start > end → 即ループスキップ)。
    // 終端等の式は `eval_int_expr_app` で評価する。`Set` は算術を
    // 評価せず literal を保持するため、`For i = 1 To 敵配置数` の
    // `敵配置数` が `(6 + (ダンジョン進行度 / 5))` のように裸変数入りの
    // 式文字列であることがある。app-aware 評価で変数も解決する。
    let parse_lenient = |app: &App, s: &str| -> i64 {
        let t = s.trim();
        if t.is_empty() {
            return 0;
        }
        t.parse::<i64>()
            .unwrap_or_else(|_| i64::from(eval_int_expr_app(app, t)))
    };
    let start = parse_lenient(app, &xargs[2]);
    let end = parse_lenient(app, &xargs[4]);
    let step = if xargs.len() >= 7 && xargs[5].eq_ignore_ascii_case("step") {
        parse_lenient(app, &xargs[6])
    } else {
        1
    };
    if step == 0 {
        return Err(err(line, "For の Step 値が 0。"));
    }
    app.set_script_var(var.clone(), start.to_string());
    // ループに 1 回も入らない条件
    let skip = (step > 0 && start > end) || (step < 0 && start < end);
    if skip {
        return skip_to_next(pc, stmts, line);
    }
    app.push_for_frame(LoopFrame::Numeric {
        var,
        end,
        step,
        for_pc: pc,
    });
    Ok(pc + 1)
}

fn exec_next(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { line, pc, .. } = *c;
    // For / ForEach フレームを参照してインクリメント。
    // 終了条件を満たせばフレームを pop、それ以外はループ先頭へ戻る。
    // SRC.Sharp 準拠: 対応する For がない場合はエラー。
    let Some(frame) = app.last_for_frame().cloned() else {
        return Err(err(line, "Nextコマンドに対応するForコマンドがありません"));
    };
    match frame {
        LoopFrame::Numeric {
            var,
            end,
            step,
            for_pc,
        } => {
            let current = app.script_var(&var).parse::<i64>().unwrap_or(0);
            let next_v = current + step;
            let done = (step > 0 && next_v > end) || (step < 0 && next_v < end);
            // SRC.Sharp 準拠: ループ変数は Next 実行時に必ず更新される。
            // ループ終了条件を満たしても next_v を書き戻す (out-of-bounds 値)。
            // 例: `For i = 3 To 1 Step -1` → 終了後 i = 0 (= 1 + (-1))
            app.set_script_var(var, next_v.to_string());
            if done {
                app.pop_for_frame();
                return Ok(pc + 1);
            }
            Ok(for_pc + 1)
        }
        LoopFrame::Each {
            var,
            list,
            index,
            for_pc,
        } => {
            let next_index = index + 1;
            if next_index >= list.len() {
                app.pop_for_frame();
                return Ok(pc + 1);
            }
            app.set_script_var(var.clone(), list[next_index].clone());
            if let Some(LoopFrame::Each { index: i, .. }) = app.last_for_frame_mut() {
                *i = next_index;
            }
            Ok(for_pc + 1)
        }
        LoopFrame::EachUnit {
            idents,
            index,
            for_pc,
        } => {
            let next_index = index + 1;
            if next_index >= idents.len() {
                app.pop_for_frame();
                return Ok(pc + 1);
            }
            bind_foreach_unit(app, &idents[next_index]);
            if let Some(LoopFrame::EachUnit { index: i, .. }) = app.last_for_frame_mut() {
                *i = next_index;
            }
            Ok(for_pc + 1)
        }
    }
}

fn exec_foreach(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs,
        line,
        pc,
        stmts,
        ..
    } = *c;
    // SRC ForEach の書式:
    //  書式1: `ForEach group [status]` — グループのユニットを反復。
    //         ループ変数を持たず `対象ユニットＩＤ` / `対象パイロット`
    //         で参照する (スパロボ戦記 `Foreach 味方` 等)。
    //  書式2: `ForEach var [In] collection` — var にリスト要素を代入。
    // collection: 勢力ラベル ("Player"/"Enemy"/"味方"/...) なら該当 unit 一覧
    // それ以外はカンマ区切り or 空白区切りリスト
    if xargs.is_empty() {
        return Err(err(
            line,
            "ForEach 命令は引数が必要 (group | var collection)。",
        ));
    }
    let has_in = xargs.iter().any(|a| a.eq_ignore_ascii_case("in"));
    // 書式1 判定: `In` を持たず先頭が勢力ラベル / 「全」のとき。
    // (`ForEach u Player` のように先頭が変数名なら書式2 のまま。)
    let head = xargs[0].trim().trim_matches('"');
    let group_party = parse_party_label(head);
    // 書式1 判定: `In` を持たず、かつ
    //  (a) 既知の陣営ラベル / "全" / "all"、または
    //  (b) 1 引数で status 相当でない任意の識別子 (グループID として扱う)。
    // (b) の場合はグループ ID 一致フィルタは現状サポートしておらず、
    // 全ユニットを対象に反復する (graceful fallback)。
    let is_group_form = !has_in
        && (group_party.is_some()
            || matches!(head, "全" | "all" | "All")
            || (xargs.len() == 1 && !head.is_empty()));
    if is_group_form {
        // 書式1 の status (`出撃` / `待機` / `全て` / `(出撃 待機)` 等)。
        // 本実装のユニットは off_map false=出撃 / true=待機・格納・離脱 の
        // 2 状態。破壊ユニットは unit_instances から除去済み。
        let (incl_deployed, incl_offmap) = foreach_status_mask(&xargs[1..]);
        let idents: Vec<String> = app
            .database()
            .unit_instances
            .iter()
            .filter(|u| group_party.map_or(true, |p| u.party == p))
            .filter(|u| (!u.off_map && incl_deployed) || (u.off_map && incl_offmap))
            .map(|u| {
                if u.uid.is_empty() {
                    u.unit_data_name.clone()
                } else {
                    u.uid.clone()
                }
            })
            .collect();
        if idents.is_empty() {
            return skip_to_next(pc, stmts, line);
        }
        bind_foreach_unit(app, &idents[0]);
        app.push_for_frame(LoopFrame::EachUnit {
            idents,
            index: 0,
            for_pc: pc,
        });
        return Ok(pc + 1);
    }
    if xargs.len() < 2 {
        return Err(err(line, "ForEach 命令は 2 引数必要 (var collection)。"));
    }
    let var = xargs[0].clone();
    // `In` キーワードは飛ばす
    let start_idx = if xargs
        .get(1)
        .map(|s| s.eq_ignore_ascii_case("in"))
        .unwrap_or(false)
    {
        2
    } else {
        1
    };
    if xargs.len() <= start_idx {
        return Err(err(line, "ForEach 命令: collection が無い。"));
    }
    let collection = xargs[start_idx..].join(" ");
    let list = collect_foreach_items(app, &collection);
    if list.is_empty() {
        return skip_to_next(pc, stmts, line);
    }
    app.set_script_var(var.clone(), list[0].clone());
    app.push_for_frame(LoopFrame::Each {
        var,
        list,
        index: 0,
        for_pc: pc,
    });
    Ok(pc + 1)
}

fn exec_skip(_app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        line, pc, stmts, ..
    } = *c;
    // `Skip` — 対応するループの末尾 (Loop / Next) へジャンプ。
    // ループ継続 ("continue" 相当)。SRC.Sharp `SkipCmd.cs` 準拠:
    // 引数なし。ループ外で使用するとエラー。
    let after = skip_to_loop_or_next_end(pc, stmts, line)
        .map_err(|_| err(line, "Skipコマンドがループの外で使われています"))?;
    Ok(after.saturating_sub(1))
}

fn exec_do(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs,
        raw_args,
        line,
        pc,
        stmts,
        ..
    } = *c;
    // `Do` / `Do While cond` / `Do Until cond` 構文。
    // While: cond が偽なら対応 Loop の次へ
    // Until: cond が真なら対応 Loop の次へ
    // SRC.Sharp 準拠: While / Until 以外のキーワードはエラー。
    if xargs.len() >= 2 {
        let kind = xargs[0].to_ascii_lowercase();
        if kind == "while" || kind == "until" {
            let cond = loop_condition(app, &raw_args[1..]);
            let should_skip = (kind == "while" && !cond) || (kind == "until" && cond);
            if should_skip {
                return skip_to_matching("do", "loop", pc, stmts, line);
            }
        } else {
            return Err(err(line, "Doコマンドには While または Until が必要です"));
        }
    }
    Ok(pc + 1)
}

fn exec_loop(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs,
        raw_args,
        line,
        pc,
        stmts,
        ..
    } = *c;
    // `Loop` / `Loop While cond` / `Loop Until cond`
    // 後方 Do へ戻るか否かを条件で判定。
    // SRC.Sharp 準拠: While / Until 以外のキーワードはエラー。
    let goto_back = if xargs.len() >= 2 {
        let kind = xargs[0].to_ascii_lowercase();
        let cond = loop_condition(app, &raw_args[1..]);
        if kind == "while" {
            cond
        } else if kind == "until" {
            !cond
        } else {
            return Err(err(line, "Loopコマンドには While または Until が必要です"));
        }
    } else {
        true
    };
    if goto_back {
        // Loop は Do の位置そのものへ戻して再評価させる。
        let do_pc = find_back("do", "loop", pc, stmts, line)?;
        return Ok(do_pc);
    }
    Ok(pc + 1)
}

fn exec_break(_app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        line, pc, stmts, ..
    } = *c;
    // 最内の Loop / Next まで前方ジャンプ
    skip_to_loop_or_next_end(pc, stmts, line)
}

fn exec_continue(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs,
        line,
        pc,
        stmts,
        ..
    } = *c;
    // SRC `Continue` には 2 つの異なる意味がある:
    //
    // 1. 引数なし: 最内 For/Do/Loop の次反復に飛ぶ (= 通常の "continue"
    //    キーワード)。
    //
    // 2. `Continue <filename.eve>`: 現シナリオを終了し、次に読み込む
    //    シナリオファイル名をシステム変数「次ステージ」にセットして
    //    エピローグ ラベルへジャンプする。エピローグ が無ければそのまま
    //    スクリプトを終了。フロントエンド側 (archive.rs 等) は
    //    `script_var("次ステージ")` を確認して、対応する .eve を
    //    新規シナリオとして起動する責務を負う。
    //
    //    元実装: VB5 `Event.bas` / SRC.Sharp
    //    `CmdDatas/Commands/Stage/ContinueCmd.cs:ExecInternal`
    //    に対応。
    if xargs.is_empty() {
        // ループ内なら次反復へスキップ。ループ外なら Exit 相当のステージ遷移
        // として扱う (実シナリオで `Continue` をシナリオ終了に使う慣用句)。
        match skip_to_loop_or_next_end(pc, stmts, line) {
            Ok(after) => return Ok(after.saturating_sub(1)),
            Err(_) => return Ok(stmts.len()), // ループ外 → 終了
        }
    }
    // SRC.Sharp 準拠: ステージ遷移形式は `Continue filename [option]` の
    // 最大 2 引数。3 引数以上はエラー。
    if xargs.len() > 2 {
        return Err(err(line, "Continueコマンドの引数の数が違います"));
    }
    let next_stage = xargs.join(" ");
    app.set_script_var("次ステージ".to_string(), next_stage);
    // シナリオ終了 = 原典の `IsScenarioFinished` スタック巻き戻しに相当。
    // 旧ステージの flow 継続と未消化の割込みイベントを破棄する
    // (これをしないと旧ステージの AfterStartEvent 等が新ステージ突入後に
    // 発火して begin_phase が二重に走る)。
    app.scenario_transition_reset();
    // インターミッション制シナリオ (`IntermissionCommand` 登録あり) では、
    // `Continue` で次ステージへ自動遷移せず `Scene::Intermission` で停止する。
    // ユーザがメニュー (キャラメイキング 等) を選んでから「次のステージへ」で
    // 本編に進める。SRC.Sharp `InterMission.InterMissionCommand()` 相当。
    if !app.intermission_commands().is_empty() {
        app.set_scene(crate::Scene::Intermission);
        app.set_intermission_cursor(0);
        // タイトル画面の Hotpoint / 描画コマンドが Intermission に
        // 重ね描画されないようクリアする。
        app.clear_hotpoints();
        app.script_overlay_mut().clear();
    } else {
        // 非インターミッション: 現スクリプト (エピローグ含む) 完了後に
        // 次ステージを起動する継続を積む。原典の
        // `StartScenario(次ステージ)` 相当 (フロントエンド非依存)。
        app.push_flow_cont(crate::flow::FlowCont::LoadNextStage);
    }
    // 現シナリオファイル**内**のエピローグへのみジャンプする。
    // global labels には後続章 (同名 `エピローグ:`) のエピローグも
    // 登録されているため、`labels.contains_key` + global フォールバック
    // で判定すると、エピローグを持たない薄い entry (`*スタート.eve` が
    // `Continue 01.eve` だけ書いてある型) の `Continue` が別ファイルの
    // エピローグへ誤ジャンプし、本編プロローグ/スタートを飛ばして
    // しまう (東方夢想伝: entry → 01.eve エピローグ直行 → 味方0体で
    // 即敗北)。`label_pc_within_file` は当該ファイル内に無ければ None。
    if let Some(epilogue_pc) = app.script_library().label_pc_within_file(pc, "エピローグ") {
        return Ok(epilogue_pc);
    }
    Ok(stmts.len())
}

fn exec_require(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Require <path>` — 設定ファイル (`.ini`) を取り込み、その
    // トップレベル `key = value` 行をスクリプト変数として適用する。
    // Alpha2ndStatus.ini の LetterColor / FrameColor 等のテーマ設定は
    // これで定義される。対象は archive ローダが script_library に
    // 登録済み (自動実行はされない)。
    if let Some(path) = xargs.first() {
        apply_required_file(app, path);
    }
    Ok(pc + 1)
}

/// `Do While` / `Loop Until` 等の条件 (原典 `GetArgAsLong(3) <> 0`)。原典は条件を 1 項
/// (`(n < 10)`) で書くが、本移植は空白を含む `n < 10` も 1 つの式として受け付ける。
fn loop_condition(app: &mut App, raw: &[String]) -> bool {
    let text = raw.join(" ");
    let text = if text.to_ascii_lowercase().contains("call(") {
        preprocess_call_expressions_in_condition(app, &text)
    } else {
        text
    };
    eval::value_as_long(app, &text) != 0
}
