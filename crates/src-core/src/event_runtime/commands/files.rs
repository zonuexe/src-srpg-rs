//! `.eve` コマンド: VFS 上のファイル入出力。

use super::*;

pub(super) fn exec(app: &mut App, c: &Cmd) -> Option<Result<usize, ScriptError>> {
    Some(match c.lname.as_str() {
        "print" | "write" => exec_print(app, c),
        "open" => exec_open(app, c),
        "read" | "lineread" => exec_read(app, c),
        "close" => exec_close(app, c),
        "createfolder" => exec_createfolder(app, c),
        "removefolder" => exec_removefolder(app, c),
        "removefile" => exec_removefile(app, c),
        "renamefile" => exec_renamefile(app, c),
        "copyfile" => exec_copyfile(app, c),
        _ => return None,
    })
}

fn exec_print(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        lname, xargs, pc, ..
    } = *c;
    // `Print <handle>, [text]` — 第 1 引数が開いているファイルハンドル
    // なら仮想ファイルへ 1 行書き込む。それ以外の `Print "text"` は
    // Message と同義 (`Write` はファイル専用なので handle 無しは無視)。
    // 書き込みテキストは `$(...)` / 関数呼出を展開してから VFS へ渡す。
    // SRC 構文の `Print #1, abc` は tokenizer 上で `#1,` がトークン化
    // されるため、ハンドル候補の trailing `,` を剥がしてから解決する。
    if let Some(first) = xargs.first() {
        let handle_raw = first.trim_end_matches(',');
        let handle = fn_arg_value(app, handle_raw);
        if app.vfs_is_handle(&handle) {
            let raw = xargs[1..].join(" ");
            let text = expand_vars(app, &raw);
            app.vfs_print(&handle, text);
        } else if lname == "print" {
            app.push_message(first.clone());
        }
    }
    Ok(pc + 1)
}

fn exec_open(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `Open <path> For <mode> As <handle_var>`
    // SRC.Sharp `OpenCmd.cs` 準拠: 引数は正確に 5 (コマンド名を除く)。
    // ArgNum==6 はコマンド名を含むため、xargs.len()==5。
    if xargs.len() != 5 {
        return Err(err(line, "Openコマンドの引数の数が違います"));
    }
    // path / mode には `$(...)` が含まれることがあるため eval 後に展開する。
    let path = xargs
        .first()
        .map(|s| expand_vars(app, &fn_arg_value(app, s)))
        .unwrap_or_default();
    // パス内の上位ディレクトリ参照を禁止する (SRC.Sharp 準拠)。
    if path.contains("..\\") || path.contains("../") {
        return Err(err(line, "ファイル指定に「../」は使えません"));
    }
    let mut mode = String::from("入力");
    let mut handle_var = String::new();
    let mut i = 1;
    while i < xargs.len() {
        match xargs[i].to_ascii_lowercase().as_str() {
            "for" => {
                if let Some(m) = xargs.get(i + 1) {
                    mode = expand_vars(app, &fn_arg_value(app, m));
                }
                i += 2;
            }
            "as" => {
                if let Some(v) = xargs.get(i + 1) {
                    handle_var = v.clone();
                }
                i += 2;
            }
            _ => i += 1,
        }
    }
    if !path.is_empty() && !handle_var.is_empty() {
        let h = app.vfs_open(&path, &mode);
        app.set_script_var(handle_var, h);
    }
    Ok(pc + 1)
}

fn exec_read(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        args, xargs, pc, ..
    } = *c;
    // `Read <handle> <var>` / `LineRead <handle> <var>` — 1 行読込。
    if xargs.len() >= 2 {
        let handle = fn_arg_value(app, &xargs[0]);
        let line = app.vfs_read_line(&handle).unwrap_or_default();
        let var = resolve_lhs_name(app, &args[1]);
        app.set_script_var(var, line);
    }
    Ok(pc + 1)
}

fn exec_close(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    if let Some(h) = xargs.first() {
        let handle = fn_arg_value(app, h);
        app.vfs_close(&handle);
    }
    Ok(pc + 1)
}

fn exec_createfolder(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // CreateFolder path — 仮想フォルダを作成 (VFS では path prefix として扱う)。
    // VFS は BTreeMap<path, lines> なのでフォルダそのものは不要だが、
    // スクリプトが存在確認する場合に備えて空エントリを入れておく。
    if let Some(path) = xargs.first() {
        let norm = format!(
            "{}/",
            path.trim()
                .trim_matches('"')
                .to_ascii_lowercase()
                .replace('\\', "/")
        );
        app.vfs_ensure_folder(&norm);
    }
    Ok(pc + 1)
}

fn exec_removefolder(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // RemoveFolder path — 指定パス以下のファイルを全削除。
    if let Some(path) = xargs.first() {
        let prefix = path
            .trim()
            .trim_matches('"')
            .to_ascii_lowercase()
            .replace('\\', "/");
        app.vfs_remove_folder(&prefix);
    }
    Ok(pc + 1)
}

fn exec_removefile(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // RemoveFile path — 仮想ファイルを削除。
    if let Some(path) = xargs.first() {
        app.vfs_remove_file(path);
    }
    Ok(pc + 1)
}

fn exec_renamefile(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // RenameFile old new — 仮想ファイルのパスを変更。
    if xargs.len() >= 2 {
        app.vfs_rename_file(&xargs[0], &xargs[1]);
    }
    Ok(pc + 1)
}

fn exec_copyfile(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // CopyFile src dst — 仮想ファイルをコピー。
    if xargs.len() >= 2 {
        app.vfs_copy_file(&xargs[0], &xargs[1]);
    }
    Ok(pc + 1)
}
