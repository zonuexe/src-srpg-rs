//! `.eve` コマンド: ステージ進行・勝敗・セーブ / ロード・インターミッション。

use super::*;

pub(super) fn exec(app: &mut App, c: &Cmd) -> Option<Result<usize, ScriptError>> {
    Some(match c.lname.as_str() {
        "suspend" => exec_suspend(app, c),
        "win" | "gameclear" => exec_win(app, c),
        "lose" | "gameover" => exec_lose(app, c),
        "finish" => exec_finish(app, c),
        "money" => exec_money(app, c),
        "savedata" => exec_savedata(app, c),
        "load" => exec_load(app, c),
        "restart" => exec_restart(app, c),
        "quicksave" => exec_quicksave(app, c),
        "quickload" => exec_quickload(app, c),
        "clearevent" => exec_clearevent(app, c),
        "organize" => exec_organize(app, c),
        "intermissioncommand" => exec_intermissioncommand(app, c),
        "callintermissioncommand" => exec_callintermissioncommand(app, c),
        "changemode" => exec_changemode(app, c),
        "option" => exec_option(app, c),
        "changeterrain" => exec_changeterrain(app, c),
        "changemap" => exec_changemap(app, c),
        "debug" => exec_debug(app, c),
        "quit" => exec_quit(app, c),
        "pause" => exec_pause(app, c),
        _ => return None,
    })
}

fn exec_suspend(_app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `Suspend` (C# `SuspendCmd` は `return NextID` = **何もせず次へ**):
    // Talk ブロック内では「メッセージウィンドウを残したままクリック待ち」の
    // 区切りとして機能する (= `collect_until_end` が Talk 終端として消費する)。
    // Talk 外の単独 `Suspend` は no-op (次へ進む) であり、ゲーム中断/Title 復帰
    // ではない (中断は `Quit` コマンドの役割)。旧実装は単独 Suspend で
    // scene=Title にしていたが、これは誤りで、cinematic 途中の Suspend が
    // タイトルへ飛ばしてしまっていた。
    Ok(pc + 1)
}

fn exec_win(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { args, line, pc, .. } = *c;
    // SRC.Sharp 準拠: 引数ありはエラー。
    if !args.is_empty() {
        return Err(err(line, "Win/GameClearコマンドの引数の数が違います"));
    }
    app.set_stage_state(crate::stage::StageState::Victory);
    app.push_message("【勝利】".to_string());
    // 元 SRC では `Win` / `GameClear` 後にエンディング系イベントが
    // 自動発火する。`App::game_clear()` (UI 経由) と同等の lookup
    // 順 で 1 件発火。
    fire_victory_labels(app);
    Ok(pc + 1)
}

fn exec_lose(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { args, line, pc, .. } = *c;
    // SRC.Sharp 準拠: 引数ありはエラー。
    if !args.is_empty() {
        return Err(err(line, "Lose/GameOverコマンドの引数の数が違います"));
    }
    app.set_stage_state(crate::stage::StageState::Defeat);
    app.push_message("【敗北】".to_string());
    fire_game_over_labels(app);
    Ok(pc + 1)
}

fn exec_finish(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `Finish [unit]` (`Finishコマンド.md` / C# `FinishCmd`): 指定
    // ユニットの行動を **1 回分終了** させるユニットコマンド。
    // ステージ終了 (Victory) ではない点に注意 (旧実装は Win/GameClear と
    // 混同して `stage_state=Victory` にしており、決戦！宇宙怪獣1話 の
    // スタートイベント `Finish ジェイド＝ソウマ` で開始即勝利になっていた)。
    //
    // unit 省略時は `selected_unit_for_event`。本実装の行動モデルは
    // boolean (`has_acted`, 最大 1 行動) なので has_acted=true が
    // 「1 行動消費」に相当する。既に行動済みなら no-op (冪等)。
    let key = match xargs.first() {
        Some(u) => u.clone(),
        None => app.selected_unit_for_event().to_string(),
    };
    if let Some(u) = app
        .database_mut()
        .unit_instances
        .iter_mut()
        .find(|u| matches_unit_handle(u, &key))
    {
        u.has_acted = true;
    }
    Ok(pc + 1)
}

fn exec_money(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `Money value` — 資金を value だけ増やす (SRC.Sharp: IncrMoney(value))。
    // value は常にデルタ値（差分）。マイナス値で減少。`+` プレフィックスは省略可能。
    // 例: `Money 30000` → 30000 追加、`Money -1000` → 1000 減少。
    // ※ 絶対値設定ではない (SRC.Sharp `MoneyCmd.cs` は常に IncrMoney を呼ぶ)。
    // C# MoneyCmd.cs: ArgNum != 2 → EventErrorException。
    if xargs.len() != 1 {
        return Err(err(line, "Moneyコマンドの引数の数が間違っています"));
    }
    // 増減額は原典どおり `GetArgAsLong`。
    if let Some(n) = c.long(app, 0) {
        app.add_money(n);
    }
    Ok(pc + 1)
}

fn exec_savedata(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `SaveData [slot]` — slot 省略時は 0
    let slot = xargs
        .first()
        .and_then(|s| s.parse::<u8>().ok())
        .unwrap_or(0);
    if let Ok(json) = app.to_save_json() {
        app.push_message(format!("【セーブ】slot {slot} ({} bytes 想定)", json.len()));
        // 実保存はフロントエンド側 (localStorage) で行う。ここでは記録のみ。
        app.set_script_var(format!("__save_slot_{slot}"), json);
    }
    Ok(pc + 1)
}

fn exec_load(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Load title [title2 ...]` — タイトル (作品) をロードリストに追加。
    // SRC.Sharp `LoadCmd.cs` 準拠: 引数として作品名を受け取り、
    // 未登録のものを `App.titles` に追加する。
    // 実際のデータロード (データファイル読み込み) はフロントエンド委任。
    // セーブ番号を引数とする旧形式 `Load [slot]` と区別するため、
    // 数値のみの引数はセーブロード要求として扱う。
    if let Some(first) = xargs.first() {
        if let Ok(slot) = first.parse::<u8>() {
            app.push_message(format!("【ロード要求】slot {slot}"));
            return Ok(pc + 1);
        }
    }
    for arg in xargs {
        let tname = arg.trim_matches('"').to_string();
        if !tname.is_empty() && !app.titles().contains(&tname) {
            app.titles_mut().push(tname);
        }
    }
    Ok(pc + 1)
}

fn exec_restart(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `Restart` (`リスタート.md`):
    // ステージをスタートイベント時点からやり直す。`begin_battle` で
    // 自動保存された `__restart_save` script_var を復元する責務を
    // フロントエンド (`from_save_json` + `fire_resume_event`) に委任。
    // QuickSave 後にリスタートすると QuickLoad は無効化される仕様
    // (本実装は `__quicksave` をクリアして同等の動作にする)。
    if !app.script_var("__restart_save").is_empty() {
        // QuickLoad 無効化
        let snap = app.script_var("__restart_save").to_string();
        app.set_script_var("__quicksave".to_string(), String::new());
        // フロントエンドが取り出して self を置換する (core は self 置換不可)。
        app.request_reload(snap);
        app.push_message("【リスタート要求】".to_string());
    } else {
        app.push_message("リスタートデータがありません".to_string());
    }
    Ok(pc + 1)
}

fn exec_quicksave(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `QuickSave` (`QuickSaveコマンド.md`):
    // クイックセーブ。`__quicksave` script_var に JSON を保存し、
    // フロントエンドが localStorage に永続化する責務。
    if let Ok(json) = app.to_save_json() {
        app.set_script_var("__quicksave".to_string(), json);
    }
    Ok(pc + 1)
}

fn exec_quickload(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `QuickLoad` (`QuickLoadコマンド.md`):
    // クイックロード。本実装は self 置換不可のため、フロントエンドが
    // [`App::take_pending_reload`] で JSON を取り出し `from_save_json` で
    // 置換 + `fire_resume_event` する責務を持つ。
    //
    // `__quicksave` が無ければ、戦闘開始時の自動スナップショット
    // `__restart_save` (`enter_battle_state` が保存) をフォールバックに使う。
    // これにより、ゲームオーバー → コンティニュー (GameOver.eve の `Quickload`) で
    // プレイヤーがクイックセーブしていなくてもステージ頭から再開できる。
    let json = {
        let qs = app.script_var("__quicksave");
        if !qs.is_empty() {
            qs.to_string()
        } else {
            app.script_var("__restart_save").to_string()
        }
    };
    if !json.is_empty() {
        app.request_reload(json);
        app.push_message("【クイックロード要求】".to_string());
    } else {
        app.push_message("クイックセーブデータがありません".to_string());
    }
    Ok(pc + 1)
}

fn exec_clearevent(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `ClearEvent [label]` (`ClearEventコマンド.md`):
    // - 引数あり: 指定ラベルを無効化。
    // - 引数省略: **現在実行中の auto-fire ラベル** を無効化 (SRC
    //   `Event.CurrentLabel`)。`*攻撃 ＮＰＣ 敵` 等の一度きりイベントの末尾で使う。
    // 無効化は disabled_pcs (label 解決でスキップ) で行う。自動発火は
    // file-scoped (`label_pc_in_file` = statements 走査) のため、labels マップ
    // からの削除だけでは止まらない。両方を無効化する。
    // SRC.Sharp 準拠: 2 引数以上はエラー。
    if xargs.len() > 1 {
        return Err(err(line, "ClearEventコマンドの引数の数が違います"));
    }
    match xargs.first() {
        Some(label) => {
            let key = label
                .trim_start_matches('@')
                .trim_start_matches('*')
                .trim_end_matches(':')
                .trim_matches('"')
                .to_string();
            // file-scoped 発火も止めるため pc を解決して無効化 (現ステージ
            // ファイル優先 → global フォールバック)。
            let resolved = {
                let lib = app.script_library();
                let cur = app.current_stage_file();
                if cur.is_empty() {
                    lib.label_pc(&key)
                } else {
                    lib.label_pc_in_file(cur, &key)
                        .or_else(|| lib.label_pc(&key))
                }
            };
            if let Some(p) = resolved {
                app.script_library_mut().disable_pc(p);
            }
            app.script_library_mut().labels.remove(&key);
        }
        None => {
            if let Some(p) = app.current_event_label_pc() {
                app.script_library_mut().disable_pc(p);
            }
        }
    }
    Ok(pc + 1)
}

fn exec_organize(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `Organize count x y` — 中間メニューで count 体までを (x, y) 付近に
    // プレイヤーが配置するためのコマンド。本実装では UI を持たないので、
    // off_map になっている Player ユニットを最大 count 体まで (x, y) 中心
    // 螺旋状に再配置するだけの最小実装。
    if xargs.len() < 3 {
        return Ok(pc + 1);
    }
    // 数・座標は原典どおり `GetArgAsLong`。
    let count = c.long(app, 0).unwrap_or(0).max(0) as usize;
    // 中心は 1 始まりで盤内に丸める (原典 `ExecOrganizeCmd`)。
    let (cx, cy) = coord::clamp_from_script(
        app,
        c.long(app, 1).unwrap_or(0),
        c.long(app, 2).unwrap_or(0),
    );
    let (cx, cy) = (cx as i32, cy as i32);
    // 配置済 (! off_map) の Player ユニットは触らず、off_map なものから
    // 配置する。何も off_map に無い場合は no-op。
    let (mw, mh) = app
        .database()
        .map
        .as_ref()
        .map(|m| (m.width as i32, m.height as i32))
        .unwrap_or((20, 20));
    let mut placed = 0usize;
    // 螺旋順オフセット: (0,0) → 周囲 1 マス → 2 マス ...
    let offsets: Vec<(i32, i32)> = {
        let mut v: Vec<(i32, i32)> = Vec::new();
        for r in 0i32..=5 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs().max(dy.abs()) == r {
                        v.push((dx, dy));
                    }
                }
            }
        }
        v
    };
    let mut occupied: std::collections::HashSet<(u32, u32)> = app
        .database()
        .unit_instances
        .iter()
        .filter(|u| !u.off_map)
        .map(|u| (u.x, u.y))
        .collect();
    // 1 順目: off_map の Player ユニットを優先配置。
    // 2 順目: それでも 0 体だった場合は、on_map の Player ユニットも
    //         移動対象とする (キャラ作成→Escape のチェーンが切れて
    //         on_map のまま残っているケースの救済)。
    for pass in 0..2 {
        for i in 0..app.database().unit_instances.len() {
            if placed >= count {
                break;
            }
            let candidate = {
                let u = &app.database().unit_instances[i];
                if u.party != crate::Party::Player {
                    false
                } else if pass == 0 {
                    u.off_map
                } else {
                    // 2 順目: 既に配置済 (= occupied に含まれる) を除く
                    !u.off_map && occupied.contains(&(u.x, u.y))
                }
            };
            if !candidate {
                continue;
            }
            // 空き座標を探す
            let mut chosen: Option<(u32, u32)> = None;
            for (dx, dy) in &offsets {
                let nx = cx + *dx;
                let ny = cy + *dy;
                if nx < 0 || ny < 0 || nx >= mw || ny >= mh {
                    continue;
                }
                let pos = (nx as u32, ny as u32);
                if !occupied.contains(&pos) {
                    chosen = Some(pos);
                    break;
                }
            }
            if let Some(pos) = chosen {
                // 2 順目では先に古い位置を occupied から外す
                if pass == 1 {
                    let prev = {
                        let u = &app.database().unit_instances[i];
                        (u.x, u.y)
                    };
                    occupied.remove(&prev);
                }
                let uid = app.database().unit_instances[i].uid.clone();
                app.database_mut().move_unit(&uid, pos.0, pos.1);
                app.database_mut().set_off_map(&uid, false);
                occupied.insert(pos);
                placed += 1;
            }
        }
        // 1 順目で 1 体以上配置できたら 2 順目はスキップ
        if placed > 0 {
            break;
        }
    }
    Ok(pc + 1)
}

fn exec_intermissioncommand(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `IntermissionCommand <name> <file>` — name を選択肢として登録、
    // 選択時に file を実行する。第 2 引数 "削除" で当該名を削除。
    // SRC.Sharp `Intermission.cs` の `IntermissionCommand(file)` グローバル
    // 変数経由のセットと等価。
    // SRC.Sharp 準拠: 2 引数未満はエラー。
    if xargs.len() < 2 {
        return Err(err(line, "IntermissionCommandコマンドの引数の数が違います"));
    }
    let name = xargs[0].clone();
    let arg2 = xargs[1].clone();
    if arg2 == "削除" {
        app.remove_intermission_command(&name);
        app.unset_script_var(&format!("IntermissionCommand({name})"));
    } else {
        // SRC.Sharp 準拠: `IntermissionCommand(name)` グローバル変数にファイル名を記録する
        app.set_script_var(format!("IntermissionCommand({name})"), arg2.clone());
        app.push_intermission_command(name, arg2);
    }
    Ok(pc + 1)
}

fn exec_callintermissioncommand(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `CallIntermissionCommand <name>` — 名前付きの中断メニューを起動。
    // データセーブや機体改造など、GUI 相互作用が必要なものが多く、
    // WASM では stub としてログ出力のみ。
    let name = xargs.first().map(|s| s.as_str()).unwrap_or("");
    match name {
        "データセーブ" => {
            // メニュー経路 (`InterItem::Save`) と同じ実体に委譲する。
            // 現在状態を JSON 化して `__quicksave` に保存 (フロントが
            // localStorage へ永続化)。
            app.intermission_data_save();
        }
        "機体改造"
        | "ユニットの強化"
        | "乗せ換え"
        | "アイテム交換"
        | "換装"
        | "パイロットステータス"
        | "ユニットステータス" => {
            log::info!(
                "CallIntermissionCommand: {} (not implemented in WASM)",
                name
            );
        }
        _ => {
            log::warn!("CallIntermissionCommand: unknown command '{}'", name);
        }
    }
    Ok(pc + 1)
}

fn exec_changemode(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `ChangeMode [unit] mode` (`ChangeModeコマンド.md`): ユニットの思考モードを
    // `ai_mode` に設定する。mode が座標 `X Y` のときは原典 `ExecChangeModeCmd` どおり
    // `GetArgAsLong` で読み、`"X Y"` (1 始まりのまま) として保存する (AI が目的地として
    // 使うときに内部座標へ直す)。
    // 引数 2 個は、両方が数値なら選択中ユニットの座標モード、そうでなければ `unit mode`。
    let coord = |i: usize| {
        format!(
            "{} {}",
            c.long(app, i).unwrap_or(0),
            c.long(app, i + 1).unwrap_or(0)
        )
    };
    let numeric = |i: usize| xargs.get(i).is_some_and(|s| eval::is_numeric(s));
    let (target, mode) = match xargs.len() {
        0 => return Err(err(line, "ChangeMode 命令は最低 1 引数必要 (mode)。")),
        1 => (app.selected_unit_for_event().to_string(), xargs[0].clone()),
        2 if numeric(0) && numeric(1) => (app.selected_unit_for_event().to_string(), coord(0)),
        3 if numeric(1) && numeric(2) => (xargs[0].clone(), coord(1)),
        _ => (xargs[0].clone(), xargs[1..].join(" ")),
    };
    // 陣営指定: 該当陣営の全ユニットを更新 (`味方`/`敵`/`友軍`/`中立`)
    let party = parse_party_label(&target);
    if let Some(p) = party {
        for u in app
            .database_mut()
            .unit_instances
            .iter_mut()
            .filter(|u| u.party == p)
        {
            u.ai_mode = mode.clone();
        }
    } else {
        // 単一ユニット指定
        if let Some(u) = app
            .database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| matches_unit_handle(u, &target))
        {
            u.ai_mode = mode;
        }
    }
    Ok(pc + 1)
}

fn exec_option(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `Option name [解除]` (`Optionコマンド.md`):
    // `Option(name)` というグローバル変数に 1 を設定する。
    // 第2引数が存在する場合 (「解除」等) は変数を削除 (SRC.Sharp: UndefineVariable)。
    // 参照は `IsVarDefined("Option(name)")` / `IsOptionDefined()` 等で行う。
    // SRC.Sharp 準拠: 0 個または 3 個以上の引数はエラー。
    if xargs.is_empty() || xargs.len() > 2 {
        return Err(err(line, "Optionコマンドの引数の数が違います"));
    }
    let name = &xargs[0];
    let key = format!("Option({name})");
    if xargs.get(1).is_some() {
        // 「解除」やその他の 2 番目の引数 → 変数を削除
        app.unset_script_var(&key);
    } else {
        app.set_script_var(key, "1".to_string());
    }
    Ok(pc + 1)
}

fn exec_changeterrain(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `ChangeTerrain X Y name bitmap` (`ChangeTerrainコマンド.md`):
    // 座標 (X, Y) の地形を name (TerrainEntry / 組込地形名) に切替し、
    // bitmap_no を上書き。マップが未定義 / 座標範囲外 / 地形未定義は no-op。
    // ローカル地形 (`地形名(ローカル)`) 指定は `(ローカル)` 接尾を剥がして
    // 同名地形を引く (本実装は local-bitmap を区別しない)。
    if xargs.len() < 4 {
        return Err(err(
            line,
            "ChangeTerrain 命令は 4 引数必要 (X Y name bitmap)。",
        ));
    }
    // 座標・ビットマップ番号は原典どおり `GetArgAsLong`。座標は 1 始まりで、
    // 範囲外 (原典はエラー) とマップ未定義は no-op。
    let Some((x, y)) = coord::checked_from_script(
        app,
        c.long(app, 0).unwrap_or(0),
        c.long(app, 1).unwrap_or(0),
    ) else {
        return Ok(pc + 1);
    };
    let mut name = xargs[2].clone();
    if let Some(stripped) = name.strip_suffix("(ローカル)") {
        name = stripped.to_string();
    }
    let bitmap = c.long(app, 3).unwrap_or(0).max(0) as u32;
    // name → terrain_id 解決。scenario terrains → built-in の順。
    let terrain_id = app
        .database()
        .catalog
        .terrains
        .iter()
        .find(|t| t.name == name)
        .map(|t| t.id)
        .or_else(|| {
            crate::data::terrain::DEFAULT_TERRAINS
                .iter()
                .find(|t| t.name == name)
                .map(|t| t.id)
        });
    let Some(tid) = terrain_id else {
        // 不明な地形名は no-op。エラーを返すと旧シナリオが落ちるため。
        return Ok(pc + 1);
    };
    if let Some(map) = app.database_mut().map.as_mut() {
        map.set_cell(
            x,
            y,
            MapCell {
                terrain_id: tid,
                bitmap_no: bitmap,
            },
        );
    }
    Ok(pc + 1)
}

fn exec_changemap(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // `ChangeMap <path> [非同期]` — シナリオ ZIP からロード済みの `.map`
    // (basename 一致) を `GameDatabase.map` に差し替える。見つからない
    // 場合は前のマップを維持。
    if let Some(path) = xargs.first() {
        if let Some(replacement) = app.database().find_map(path).cloned() {
            app.database_mut().map = Some(replacement);
        }
    }
    Ok(pc + 1)
}

fn exec_debug(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `Debug message` (`Debugコマンド.md`):
    // メッセージをログに出力 ([INF] レベル)。本実装は app.messages に追記。
    let msg = xargs.join(" ");
    log::info!("[EVE Debug] {msg}");
    app.push_message(format!("[Debug] {msg}"));
    Ok(pc + 1)
}

fn exec_quit(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `Quit` (`Quitコマンド.md`): `Suspend` 相当 — タイトルへ戻る。
    app.set_scene(crate::Scene::Title);
    Ok(pc + 1)
}

fn exec_pause(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `Pause` (`Pauseコマンド.md`): `Wait` 相当の一時停止。
    // 引数があれば Wait と同じ処理、なければ 0 秒待機。
    let secs: f64 = xargs
        .first()
        .and_then(|s| s.to_string().parse().ok())
        .unwrap_or(0.0);
    if secs > 0.0 {
        app.set_pending_timer(secs.min(5.0));
    }
    Ok(pc + 1)
}
