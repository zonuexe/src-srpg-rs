//! `.eve` コマンド: 会話・メッセージ・入力ダイアログ。

use super::*;

pub(super) fn exec(app: &mut App, c: &Cmd) -> Option<Result<usize, ScriptError>> {
    Some(match c.lname.as_str() {
        "talk" => exec_talk(app, c),
        "telop" | "displaymessage" => exec_telop(app, c),
        "autotalk" => exec_autotalk(app, c),
        "setmessage" => exec_setmessage(app, c),
        "cancel" => exec_cancel(app, c),
        "wait" => exec_wait(app, c),
        "input" => exec_input(app, c),
        "confirm" => exec_confirm(app, c),
        "menu" | "ask" => exec_menu(app, c),
        "question" => exec_question(app, c),
        _ => return None,
    })
}

fn exec_talk(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, pc, stmts, ..
    } = *c;
    // Talk [character position option]
    // SRC 仕様: 最初の引数のみがキャラクター名（話者）。
    // それ以降の引数は位置指定 (X Y / 母艦 / 中央 / 固定) や
    // 表示オプションであり、話者名には含めない。
    // 例: `Talk システム 8 8` → speaker="システム"、座標 (8,8) は無視。
    //     `Talk 霊夢 (8,6)`   → speaker="霊夢"、座標 (8,6) は無視。
    let speaker = xargs.first().cloned().unwrap_or_default();
    let (raw_body, next_pc) = collect_until_end(app, pc + 1, stmts);
    // SRC の Talk ボディに含まれる HTML 書式タグを除去。
    // `<B>...</B>` / `<COLOR=Red>` 等は SRC のリッチテキスト用であり、
    // プレーンテキスト描画では不要。`<LT>` / `<GT>` は `<` / `>` に変換。
    // 続いてダッシュ文字正規化 (`――` → `──` 等、SRC.Sharp FormatMessage 準拠)。
    let body = normalize_dashes(&strip_talk_tags(&raw_body));
    // SRC `Talkコマンド.md`: 半角 `;` は強制改行、半角 `:` は段階表示の
    // 区切り (メッセージを一部ずつ順に表示)。全角 `；`/`：` は通常文字。
    let body = body.replace(';', "\n");
    let pages: Vec<String> = body
        .split(':')
        .map(|p| p.trim_matches(['\n', ' ', '\u{3000}']).to_string())
        .filter(|p| !p.is_empty())
        .collect();
    // メッセージログには段階表示を 1 行に畳んで積む (HUD 一行表示用)。
    let log_body = pages.join(" ").replace('\n', " ");
    let log_msg = if speaker.is_empty() {
        log_body.clone()
    } else {
        format!("【{speaker}】{log_body}")
    };
    if !log_body.is_empty() {
        app.push_message(log_msg);
    }
    // モーダル表示。先頭ページを Talk に出し、残りは talk_pages に積んで
    // クリック応答ごとに 1 ページずつ送る。空 body はスキップ。
    if let Some((first, rest)) = pages.split_first() {
        app.set_talk_pages(speaker.clone(), rest.to_vec());
        app.set_pending_dialog(crate::dialog::PendingDialog::Talk {
            speaker,
            body: first.clone(),
        });
    }
    Ok(next_pc)
}

fn exec_telop(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        lname, xargs, pc, ..
    } = *c;
    // SRC `Telop message` (`Telopコマンド.md`):
    // テロップ表示 (1 秒表示)。本実装は描画機構を持たないため、
    // message の中の `.` 区切りを改行に変換した文字列を push_message。
    // 接頭辞 `【テロップ】` で通常メッセージと区別する。
    // KeepBGM 連動 / Subtitle.mid 自動演奏は未実装。
    if !xargs.is_empty() {
        // 全引数を空白連結 (Telop はスペース許容で引用符不要)
        let raw = xargs.join(" ");
        // `.` を改行に変換 (1 回までの SRC 仕様だが、全置換で簡略化)
        let formatted = raw.replace('.', "\n");
        let lname_lc = lname.as_str();
        if lname_lc == "telop" {
            app.push_message(format!("【テロップ】{formatted}"));
        } else {
            app.push_message(formatted);
        }
    }
    Ok(pc + 1)
}

fn exec_autotalk(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `AutoTalk` (`AutoTalkコマンド.md`):
    // 自動会話: 隣接した味方/敵同士で `会話` イベントが定義されていれば
    // 自動的に発火させる。本実装は隣接判定をしつつ `会話 <a> <b>` ラベル
    // 検索で最初にヒットしたペアを発火する。
    // 引数 0: 全 unit ペア走査、1+: 起点 unit 指定。
    let candidates: Vec<usize> = if xargs.is_empty() {
        (0..app.database().unit_instances.len()).collect()
    } else {
        let key = xargs[0].clone();
        app.database()
            .unit_instances
            .iter()
            .enumerate()
            .filter(|(_, u)| matches_unit_handle(u, &key))
            .map(|(i, _)| i)
            .collect()
    };
    for i in candidates {
        // 隣接ユニットと組合せて `会話 <a> <b>` ラベルを試行
        let (x, y) = match app.database().unit_instances.get(i) {
            Some(u) => (u.x, u.y),
            None => continue,
        };
        let neighbors: Vec<usize> = (0..app.database().unit_instances.len())
            .filter(|&j| {
                if j == i {
                    return false;
                }
                let v = &app.database().unit_instances[j];
                let dx = (v.x as i64 - x as i64).abs();
                let dy = (v.y as i64 - y as i64).abs();
                dx + dy == 1
            })
            .collect();
        let atk = UnitEventId::from_unit_instance(&app.database().unit_instances[i]);
        for j in neighbors {
            let def = UnitEventId::from_unit_instance(&app.database().unit_instances[j]);
            fire_pair_event_labels(app, &["会話", "Conversation"], &atk, &def);
        }
    }
    Ok(pc + 1)
}

fn exec_setmessage(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `SetMessage [type] message` (`SetMessageコマンド.md`):
    // 次戦闘で表示する特別なメッセージをセット。本実装は battle 演出を
    // 持たないため、`script_vars` に保存して `Talk` 等で参照できる
    // ようにする。SRC.NET の `攻撃メッセージ` 等は文字列なので汎用変数
    // で代用する。
    // 引数 1: message のみ → `次戦闘メッセージ` に保存
    // 引数 2: type, message → `次戦闘メッセージ_<type>` に保存
    let (key, value) = match xargs.len() {
        0 => return Ok(pc + 1),
        1 => ("次戦闘メッセージ".to_string(), xargs[0].clone()),
        _ => (
            format!("次戦闘メッセージ_{}", xargs[0]),
            xargs[1..].join(" "),
        ),
    };
    app.set_script_var(key, value);
    Ok(pc + 1)
}

fn exec_cancel(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `Cancel` (`Cancelコマンド.md`):
    // 直前の対話 (Ask/Menu/Confirm/Input) をキャンセル扱いで閉じる。
    // 通常はユーザ側のキャンセル操作と等価。スクリプトから明示的に
    // 中断したい時 (デバッグや特殊フロー) に使う。
    // 本実装: pending_dialog をクリア + `選択` を 0 にする。
    app.cancel_pending_dialog();
    app.set_script_var("選択".to_string(), "0".to_string());
    Ok(pc + 1)
}

fn exec_wait(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `Wait <duration>` — 秒数指定: pending_timer をセットしてスクリプト
    // を中断 (run_loop が return Ok する経路は pending_dialog 検査のみ
    // なので、pending_timer も同様にチェックする)。
    // `Wait Click` / `Wait Press` / `Wait Key` は対話入力待ちのため
    // PendingDialog で中断し、ユーザ応答で再開。
    // Hotpoint が登録されていれば、その名前を選択肢にした Menu を提示し
    // ユーザ応答で `選択` 変数に hotpoint.name を格納する。
    // SRC `Waitコマンド.md` の 4 書式:
    //   書式1 Wait time        (1 引数)         — 0.1×time 秒待機
    //   書式2 Wait Start       (1 引数 "Start") — 同期基準時刻を記録
    //   書式3 Wait Until time  (2 引数)         — 基準から 0.1×time 秒まで待機
    //   書式4 Wait Click       (1 引数)         — クリック/キー入力待ち
    if xargs.is_empty() {
        return Err(err(line, "Waitコマンドの引数の数が違います"));
    }
    let kind = xargs.first().map(|s| s.as_str()).unwrap_or("");
    let kind_lc = kind.to_ascii_lowercase();
    if kind_lc == "start" {
        // 書式2: 同期基準時刻をリセット (待機はしない)。
        app.reset_wait_clock();
        return Ok(pc + 1);
    }
    if kind_lc == "until" {
        // 書式3: `Wait Until time` — ちょうど 2 引数。基準時刻から
        // 0.1×time 秒経過時まで待つ (= 直前の Wait からの増分だけ待機)。
        if xargs.len() != 2 {
            return Err(err(line, "Waitコマンドの引数の数が違います"));
        }
        let t: f64 = xargs.get(1).and_then(|s| s.parse().ok()).unwrap_or(0.0);
        let delta = app.advance_wait_clock(0.1 * t);
        if delta > 0.0 {
            app.set_pending_timer(delta.min(5.0));
        }
        return Ok(pc + 1);
    }
    if xargs.len() > 1 {
        return Err(err(line, "Waitコマンドの引数の数が違います"));
    }
    if matches!(kind_lc.as_str(), "click" | "press" | "key" | "input") {
        // 新しい Wait Click 開始: 右クリック (KeyState(2)) フラグをリセット。
        app.set_wait_click_right(false);
        let hp = app.hotpoints();
        if !hp.is_empty() {
            let options: Vec<String> = hp.iter().map(|h| h.name.clone()).collect();
            app.set_pending_dialog(crate::dialog::PendingDialog::Menu {
                prompt: "選択してください".to_string(),
                options,
                var_name: "選択".to_string(),
                store_value: true,
                option_keys: Vec::new(),
                // Hotpoint メニューは右クリックでキャンセル可 (従来どおり)。
                non_cancellable: false,
            });
        } else {
            // 原典 SRC は Wait Click で何も描画せず spin-wait するだけ。
            // 直前の PaintString / PaintPicture 描画をそのまま見せたいので
            // `PendingDialog::WaitClick` で suspend だけ行い、レンダリングは
            // skip させる。
            app.set_pending_dialog(crate::dialog::PendingDialog::WaitClick);
        }
    } else if !kind.is_empty() {
        // 書式1: 数値秒数。SRC 仕様では実待機は 0.1×time 秒
        // (`Wait 10` = 1 秒)。巨大値での擬似フリーズ防止に 5 秒で
        // クランプする。
        let t: f64 = kind.parse().unwrap_or(0.0);
        let secs = 0.1 * t;
        if secs > 0.0 {
            app.set_pending_timer(secs.min(5.0));
        }
    }
    Ok(pc + 1)
}

fn exec_input(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        args,
        xargs,
        line,
        pc,
        ..
    } = *c;
    // `Input var prompt [default]` — テキスト入力モーダルを出す。
    // SRC.Sharp 準拠: 引数は 2〜3 個 (var + prompt [+ default])。
    // 0, 1, 4+ 個はエラー。
    if xargs.len() < 2 || xargs.len() > 3 {
        return Err(err(line, "Inputコマンドの引数の数が違います"));
    }
    // 第 1 引数は代入先 (lvalue)。`xargs` は値展開済みなので、`名前[キー]` 形式の
    // 配列変数が**現在値に化けて**しまう (例: 2 回目の Input で前回値 "417776" が
    // キー名になり、入力が元の変数を更新しない)。`Set` と同じく生の `args[0]` を
    // `resolve_lhs_name` で格納キーへ解決する (添字 expr は評価し、変数値展開はしない)。
    let var = resolve_lhs_name(app, &args[0]);
    let prompt = xargs[1].clone();
    let default = xargs.get(2).cloned().unwrap_or_default();
    app.set_script_var(var.clone(), default.clone());
    app.set_pending_dialog(crate::dialog::PendingDialog::Input {
        prompt,
        var_name: var,
        default,
    });
    Ok(pc + 1)
}

fn exec_confirm(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `Confirm message` — Yes/No 対話 UI を出して中断。
    // SRC.Sharp 準拠: 引数は必ず 1 個 (message のみ)。0 個・2+ 個はエラー。
    // 応答後 `respond_dialog(0|1)` で `選択` 変数に格納し再開。
    if xargs.len() != 1 {
        return Err(err(line, "Confirmコマンドの引数の数が違います"));
    }
    let question = xargs[0].clone();
    app.set_pending_dialog(crate::dialog::PendingDialog::Confirm {
        question,
        var_name: "選択".to_string(),
    });
    Ok(pc + 1)
}

fn exec_menu(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        lname,
        xargs,
        pc,
        stmts,
        ..
    } = *c;
    // 元 SRC `Ask` には 2 形式ある:
    //   Format 1: `Ask message [option...]` + 選択肢を次行から `End` まで列挙
    //   Format 2: `Ask array message [option...]` — 配列 `array[*]` の値を選択肢に
    // 第 1 引数が `name[*]` インデックス変数の prefix と一致したら Format 2。
    // `Menu` は SRC では Format 1 のみだが、本実装では同じ枝で扱う。
    //
    // Format 2 で `キャンセル可` オプションが含まれる場合は store_value=false
    // のままで OK (キャンセル時は 0 が `選択` に入る)。
    let lname_lc = lname.as_str();
    // Format 2 判定: 第 1 引数 prefix が script_vars に存在するか
    let fmt2_array = if !xargs.is_empty() {
        let arr_name = &xargs[0];
        let prefix = format!("{arr_name}[");
        let has_keys = app
            .script_vars()
            .keys()
            .any(|k| k.starts_with(&prefix) && k.ends_with(']'));
        if has_keys {
            Some(arr_name.clone())
        } else {
            None
        }
    } else {
        None
    };
    if let (Some(arr_name), "ask") = (fmt2_array, lname_lc) {
        // Format 2: array → options。message は xargs[1..]、オプション語
        // ("キャンセル可" 等) は本実装では prompt にも含める形で吸収。
        let prompt = if xargs.len() >= 2 {
            xargs[1..].join(" ")
        } else {
            String::new()
        };
        // ForEach 順序で配列要素を列挙。表示は **値**、添字 (key) は
        // 別途 option_keys に保持する。SRC `Ask` Format 2 は選んだ要素の
        // **添字** を `選択` に格納する仕様 (例: `乗せ換え表示[Unitid(i)]`
        // を選ぶと `選択 = Unitid(i)` となり `Pilot(選択)` が解決できる)。
        let mut options: Vec<String> = Vec::new();
        let mut option_keys: Vec<String> = Vec::new();
        for k in collect_foreach_items(app, &arr_name) {
            let full = format!("{arr_name}[{k}]");
            let v = app.script_var(&full);
            if !v.is_empty() {
                options.push(v.to_string());
                option_keys.push(k);
            }
        }
        if options.is_empty() {
            return Ok(pc + 1);
        }
        app.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt,
            options,
            var_name: "選択".to_string(),
            store_value: true,
            option_keys,
            // Format 2 (配列マップ選択) は従来どおりキャンセル可。
            non_cancellable: false,
        });
        return Ok(pc + 1);
    }
    // `Ask 終了` は特殊ターミネータ: 開いている ListBox を閉じて次に進む。
    // SRC.Sharp 準拠: 後続行を読まずに即リターン。
    if lname_lc == "ask" && xargs.len() == 1 && xargs[0].eq_ignore_ascii_case("終了") {
        return Ok(pc + 1);
    }
    // Format 1: prompt はすべての引数を空白連結、選択肢は後続行を End まで。
    // `Ask` で引数なし → デフォルトプロンプト「いずれかを選んでください」。
    // `キャンセル可` オプションがあればキャンセル可、無ければ選択必須
    // (キャンセルで 選択=0 → キャラ未選択で味方0体 → 即敗北 を防ぐ)。
    let cancelable = xargs.iter().any(|a| a == "キャンセル可");
    let prompt_parts: Vec<String> = xargs
        .iter()
        .filter(|a| a.as_str() != "キャンセル可")
        .cloned()
        .collect();
    let prompt = if prompt_parts.is_empty() {
        if lname_lc == "ask" {
            "いずれかを選んでください".to_string()
        } else {
            String::new()
        }
    } else {
        prompt_parts.join(" ")
    };
    let (options, next_pc) = collect_menu_options(app, pc + 1, stmts);
    if options.is_empty() {
        // SRC.Sharp 準拠: 選択肢なし → 選択 = "0" にして終了
        app.set_script_var("選択".to_string(), "0".to_string());
        return Ok(next_pc);
    }
    app.set_pending_dialog(crate::dialog::PendingDialog::Menu {
        prompt,
        options,
        var_name: "選択".to_string(),
        store_value: false,
        option_keys: Vec::new(),
        non_cancellable: !cancelable,
    });
    Ok(next_pc)
}

fn exec_question(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, pc, stmts, ..
    } = *c;
    // SRC `Question <time> [message]` (`Questionコマンド.md`):
    //   - time: 0.1 秒単位の制限時間。本実装では時間切れを単純化し、
    //     `Menu` と同じ即時応答ダイアログにする (タイマ無視)。
    //   - message: 説明行 (省略可、デフォルト「さあ、どうする？」)。
    //   選択肢は次行から `End` まで。先頭選択肢が選ばれたら 選択=1、
    //   時間切れなら 0。本実装は時間切れを発生させないので必ず 1+ になる。
    // 注意: 時間切れ実装は本実装で省略。SRC.NET の `Question` は
    // `Wait Timer` 同様のタイマと連動するが、PendingDialog は応答必須。
    let prompt = if xargs.len() <= 1 {
        String::from("さあ、どうする？")
    } else {
        xargs[1..].join(" ")
    };
    let (options, next_pc) = collect_menu_options(app, pc + 1, stmts);
    if options.is_empty() {
        // SRC.Sharp 準拠: 選択肢なし → 選択 = "0" にして終了
        app.set_script_var("選択".to_string(), "0".to_string());
        return Ok(next_pc);
    }
    app.set_pending_dialog(crate::dialog::PendingDialog::Menu {
        prompt,
        options,
        var_name: "選択".to_string(),
        store_value: false,
        option_keys: Vec::new(),
        // Question は時間切れ=選択0 を持つため従来どおりキャンセル可。
        non_cancellable: false,
    });
    Ok(next_pc)
}
