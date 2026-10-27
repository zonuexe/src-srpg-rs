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
    // Talk [character position option]
    // SRC 仕様: 最初の引数のみがキャラクター名（話者）。
    // それ以降の引数は位置指定 (X Y / 母艦 / 中央 / 固定) や
    // 表示オプションであり、話者名には含めない (`center_for_talk`)。
    // 例: `Talk システム 8 8` → speaker="システム"、座標 (8,8) を中央表示。
    center_for_talk(app, c);
    let block = talk_block(app, c, "talk");
    // モーダル表示。先頭ページを Talk に出し、残りは talk_pages に積んで
    // クリック応答ごとに 1 ページずつ送る。空 body はスキップ。
    if let Some((first, rest)) = block.pages.split_first() {
        app.set_talk_pages(block.speaker.clone(), rest.to_vec());
        app.set_pending_dialog(crate::dialog::PendingDialog::Talk {
            speaker: block.speaker,
            body: first.clone(),
        });
    }
    Ok(block.next_pc)
}

/// `Talk` / `AutoTalk` ブロック (コマンド行から `End` / `Suspend` / 次の同種コマンドまで)。
struct TalkBlock {
    speaker: String,
    /// 段階表示 (`:`) で区切ったページ。
    pages: Vec<String>,
    next_pc: usize,
}

/// `Talk` / `AutoTalk` の本文を集めてページに分け、メッセージログに積む。
fn talk_block(app: &mut App, c: &Cmd, block_cmd: &str) -> TalkBlock {
    let Cmd {
        xargs, pc, stmts, ..
    } = *c;
    let speaker = xargs.first().cloned().unwrap_or_default();
    let (raw_body, next_pc) = collect_until_end(app, pc + 1, stmts, block_cmd);
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
    if !log_body.is_empty() {
        app.push_message(if speaker.is_empty() {
            log_body
        } else {
            format!("【{speaker}】{log_body}")
        });
    }
    TalkBlock {
        speaker,
        pages,
        next_pc,
    }
}

/// `Talk` / `AutoTalk` でパイロット画像の描画に使うオプション (位置指定ではない)。
/// 話者の位置 (第 1 引数) に書かれたときは話者扱いになる。
const TALK_IMAGE_OPTIONS: &[&str] = &[
    "白黒",
    "セピア",
    "明",
    "暗",
    "上下反転",
    "左右反転",
    "上半分",
    "下半分",
    "右半分",
    "左半分",
    "右上",
    "左上",
    "右下",
    "左下",
    "ネガポジ反転",
    "シルエット",
    "夕焼け",
    "水中",
    "通常",
];

/// `Talk` / `AutoTalk` の中央表示 (原典 `ExecTalkCmd` / `CenterUnit`)。表示オプションを
/// 除いた最後の引数の位置で決まる:
/// - 話者だけ: 話者のユニットを中央表示する。プロローグ・エピローグ中、画像の描画中、
///   マップが無いときは行わない。
/// - 1 語: `母艦` (味方の母艦)、`中央` (上の条件に関わらず話者)、`固定` (行わない)。
/// - 2 語: 座標 `X Y`。
fn center_for_talk(app: &mut App, c: &Cmd) {
    let xargs = c.xargs;
    let mut last = None;
    let mut k = 0;
    while k < xargs.len() {
        match xargs[k].as_str() {
            "" | "非表示" | "枠外" => {}
            "右回転" | "左回転" => k += 1,
            "フィルタ" => k += 2,
            o if k > 0 && TALK_IMAGE_OPTIONS.contains(&o) => {}
            _ => last = Some(k),
        }
        k += 1;
    }
    let speaker = xargs.first().map_or("", String::as_str);
    match last {
        Some(0) => {
            let scenario_running = matches!(
                app.stage_state(),
                crate::stage::StageState::Sortie | crate::stage::StageState::Battle
            );
            if scenario_running && !app.script_overlay().is_visible() {
                center_on_speaker(app, speaker);
            }
        }
        Some(1) => match xargs[1].as_str() {
            "母艦" => center_on_mothership(app),
            "中央" => center_on_speaker(app, speaker),
            _ => {}
        },
        Some(2) => {
            if let (Some(x), Some(y)) = (c.long(app, 1), c.long(app, 2)) {
                if x >= 0 && y >= 0 {
                    app.center_map_on(x as u32, y as u32);
                }
            }
        }
        _ => {}
    }
}

/// 話者 (パイロット名・愛称。`@` 付きや `名前(表情)` も可) が乗るユニットを中央表示する。
/// 出撃していない味方のパイロットなら母艦を中央表示する (原典 `CenterUnit`)。
fn center_on_speaker(app: &mut App, speaker: &str) {
    let name = speaker.trim_start_matches('@');
    let db = app.database();
    let pilot_name = |n: &str| {
        db.catalog
            .pilots
            .iter()
            .find(|p| p.name == n)
            .or_else(|| db.catalog.pilots.iter().find(|p| p.nickname == n))
            .map(|p| p.name.clone())
    };
    let Some(pname) = pilot_name(name).or_else(|| {
        let base = name.split_once('(').map_or(name, |(b, _)| b);
        pilot_name(base)
    }) else {
        return;
    };
    let carries = |u: &&crate::UnitInstance| u.pilot_name == pname || u.pilot_ids.contains(&pname);
    match db
        .unit_instances
        .iter()
        .filter(carries)
        .find(|u| !u.off_map)
    {
        Some(u) => {
            let (x, y) = (u.x, u.y);
            app.center_map_on(x, y);
        }
        None => {
            let player = db
                .unit_instances
                .iter()
                .filter(carries)
                .any(|u| u.party == crate::Party::Player);
            if player {
                center_on_mothership(app);
            }
        }
    }
}

/// 出撃中の味方で `母艦` を持つユニットを中央表示する。
fn center_on_mothership(app: &mut App) {
    let pos = app
        .database()
        .unit_instances
        .iter()
        .find(|u| {
            u.party == crate::Party::Player
                && !u.off_map
                && crate::feature::has_feature(&u.active_features, "母艦")
        })
        .map(|u| (u.x, u.y));
    if let Some((x, y)) = pos {
        app.center_map_on(x, y);
    }
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
    // SRC `AutoTalk [character position option]` ～ `End` (`AutoTalkコマンド.md`):
    // メッセージが自動で進む Talk。原典は戦闘メッセージと同じ処理で表示するので、
    // 本実装も戦闘メッセージと同じくメッセージログに積み、応答を待たずに進む。
    // 中央表示と本文の扱い (段階表示・改行・書式タグ) は Talk と同じ。
    center_for_talk(app, c);
    Ok(talk_block(app, c, "autotalk").next_pc)
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
        let t = c.double(app, 1).unwrap_or(0.0);
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
        // クランプする。秒数は原典どおり `GetArgAsDouble` (変数・式も評価する)。
        let t = c.double(app, 0).unwrap_or(0.0);
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
