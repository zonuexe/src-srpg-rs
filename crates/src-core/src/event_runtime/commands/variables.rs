//! `.eve` コマンド: 変数・配列の操作。

use super::*;

pub(super) fn exec(app: &mut App, c: &Cmd) -> Option<Result<usize, ScriptError>> {
    Some(match c.lname.as_str() {
        "set" | "local" | "__assign" => exec_set(app, c),
        "incr" => exec_incr(app, c),
        "unset" => exec_unset(app, c),
        "sort" => exec_sort(app, c),
        "array" => exec_array(app, c),
        "swap" => exec_swap(app, c),
        "copyarray" => exec_copyarray(app, c),
        "global" => exec_global(app, c),
        "upvar" => exec_upvar(app, c),
        _ => return None,
    })
}

fn exec_set(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        lname,
        args,
        raw_args,
        line,
        pc,
        ..
    } = *c;
    // Set var value...
    // `__assign` は `var = expr` 形 (assign sugar が生成する内部コマンド名)。
    // SRC の `=` 代入は RHS が常に式なので括弧無し算術も数値化する (下記)。
    // SRC `Localコマンド` は 2 書式を持つ (C# `LocalCmd.cs`)。
    //   書式1 `Local var1 var2 …` — 各変数を空文字で宣言する
    //   書式2 `Local var = expr`  — 1 変数を式の値で初期化する
    // 書式2 の `=` は値の一部ではない。従来は `Local p = Args(1)` の値が
    // `"= <値>"` になり、`Create p …` 等で「Party 値が不正」を招いていた。
    let local_assign = lname == "local" && args.len() >= 3 && args[1].trim() == "=";
    if args.is_empty() {
        return Err(err(line, "Set/Local には変数名が必要。"));
    }
    if lname == "local" && !local_assign {
        // 書式1: 宣言のみ。SRC は新しいサブルーチンローカル変数を空で作る。
        for name in args.iter() {
            let key = resolve_lhs_name(app, name);
            if !key.is_empty() {
                // サブルーチンを抜けたら宣言前の値へ戻すため退避する。
                app.record_sublocal(&key);
                app.set_script_var(key, String::new());
            }
        }
        return Ok(pc + 1);
    }
    // 書式2 では値は `=` の次から始まる。
    let val_start = if local_assign { 2 } else { 1 };
    // 書式2 `Local var = expr` もサブルーチンローカル宣言なので退避する。
    if local_assign {
        let key = resolve_lhs_name(app, &args[0]);
        if !key.is_empty() {
            app.record_sublocal(&key);
        }
    }
    // LHS は `name[expr]` の `expr` を評価して実際のキーに解決する (xargs は値に評価済み
    // なので原文の args[0] から組み立てる)。
    //
    // 動的 LHS `Set Eval(x) value` は括弧内を式として評価した値を変数名とする
    // (原典 `SetVariable` の `Case "eval"`: `GetValueAsString(括弧内)`)。
    let var = match eval_lhs_target(&args[0]) {
        Some(name_expr) => resolve_lhs_name(app, &eval::value_as_string(app, name_expr)),
        None => resolve_lhs_name(app, &args[0]),
    };
    // 値は原文トークンを統一評価器で評価する (原典 `ExecSetCmd`: 1 項なら項として、
    // `var = a b c` の複数項は式として評価し、数値なら数値・それ以外は文字列で代入)。
    // 原典は `Set var a b` (3 項以上) をエラーとするが、本移植は式として受け付ける。
    // 例外は値の直後が `#` で始まる場合で、旧版シナリオのコメントとして無視する
    // (値そのものが `#` で始まる色コード `#3264c8` はコメントではない)。
    let value = if args.len() > val_start {
        let raw = &raw_args[val_start..];
        let end = if raw.len() > 1 && raw[1].starts_with('#') {
            1
        } else {
            raw.len()
        };
        eval::assignment_value(app, &raw[..end])
    } else if lname == "set" {
        // SRC `SetCmd`: 値なし `Set var` はフラグとして 1 を代入する
        // (`Set 機体確認` → `If 機体確認` が真)。`Local var` は空文字。
        "1".to_string()
    } else {
        String::new()
    };
    // 関数左辺値代入 (`HP(unit) = n` 等) を先に処理。
    // 対象を見つけた場合は set_script_var は呼ばない。
    if try_function_lhs_assign(app, &var, &value) {
        return Ok(pc + 1);
    }
    // システム変数書き込み (`ターン数` / `総ターン数` / `資金` 等)。
    if try_system_var_assign(app, &var, &value) {
        return Ok(pc + 1);
    }
    app.set_script_var(var, value);
    Ok(pc + 1)
}

fn exec_incr(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        args,
        xargs,
        line,
        pc,
        ..
    } = *c;
    // `Incr var [delta]` — delta 省略時は 1。
    //
    // delta は **非数値でもエラーにしない** (0 扱い)。SRC.Sharp の
    // `IncrCmd` は `GetArgAsDouble` を使い、非数値文字列は VB5 `Val()`
    // 流に 0 となる (例外を投げない)。実シナリオは
    // `Incr 仮変数 Mid(名前, i, 1)` のように 1 文字 (非数値) を
    // 渡してハッシュを計算する用途があり、ここでエラーにすると
    // スクリプト全体が異常終了してしまう (CMaking.eve が該当)。
    // SRC.Sharp 準拠: 引数 3 個以上はエラー。
    if args.is_empty() {
        return Err(err(line, "Incr には変数名が必要。"));
    }
    if args.len() > 2 {
        return Err(err(line, "Incrコマンドの引数の数が違います"));
    }
    // LHS は Set と同様に原文の args[0] から resolve_lhs_name で実キーに解決する
    // (評価済みの xargs[0] は配列要素の値になってしまい、別キーを破壊する)。
    let var = resolve_lhs_name(app, &args[0]);
    // delta は f64 で計算 (SRC.Sharp は GetArgAsDouble を使用)
    let delta: f64 = if xargs.len() >= 2 {
        let s = xargs[1].trim();
        s.parse::<f64>()
            .ok()
            .or_else(|| eval::try_numeric(app, s))
            .unwrap_or(0.0)
    } else {
        1.0
    };
    // システム変数の読み取り: script_vars より動的値を優先。
    let cur_str =
        system_variable_value(app, &var).unwrap_or_else(|| app.script_var(&var).to_string());
    let cur: f64 = cur_str.parse::<f64>().unwrap_or(0.0);
    let result = cur + delta;
    // 整数結果は整数表記で保存 (SRC との互換性)
    let stored = if result.fract() == 0.0 {
        (result as i64).to_string()
    } else {
        result.to_string()
    };
    // システム変数への書き戻し。対象外なら通常の script_var に保存。
    if !try_system_var_assign(app, &var, &stored) {
        app.set_script_var(var, stored);
    }
    Ok(pc + 1)
}

fn exec_unset(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { args, line, pc, .. } = *c;
    // `Unset var` — シナリオ変数を完全に削除する (key ごと remove)。
    // SRC.Sharp `UndefineVariable` 同等: 削除後 `IsVarDefined` = 0。
    // `name[expr]` のような indexed LHS にも対応。
    // SRC.Sharp 準拠: 引数なしはエラー。
    if args.is_empty() {
        return Err(err(line, "Unsetには変数名が必要です"));
    }
    let key = resolve_lhs_name(app, &args[0]);
    app.unset_script_var(&key);
    Ok(pc + 1)
}

fn exec_sort(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // `Sort array_name [昇順|降順] [数値|文字] [インデックスのみ|文字インデックス]`
    // (SRC.Sharp `SortCmd.cs` 準拠)
    //
    // 昇順/降順: ソート方向 (デフォルト: 昇順)
    // 数値/文字: 値の比較方法 (デフォルト: 自動判定)
    // インデックスのみ: インデックス(キー)だけでソートし値は付随して移動
    // 文字インデックス: 文字列キーを持つ配列 (キーも一緒に移動)
    if xargs.is_empty() {
        return Err(err(line, "Sort には配列変数名が必要。"));
    }
    let arr_name = xargs[0].clone();
    let mut order_asc = true;
    let mut force_string_val = false;
    let mut is_key_sort = false;
    let mut is_string_key = false;
    for opt in &xargs[1..] {
        match opt.as_str() {
            "昇順" => order_asc = true,
            "降順" => order_asc = false,
            "数値" => force_string_val = false,
            "文字" => force_string_val = true,
            "インデックスのみ" => is_key_sort = true,
            "文字インデックス" => is_string_key = true,
            other => {
                return Err(err(
                    line,
                    &format!(
                        "Sort コマンドに不正なオプション「{}」が使われています",
                        other
                    ),
                ))
            }
        }
    }
    let prefix = format!("{arr_name}[");
    let mut pairs: Vec<(String, String)> = app
        .script_vars()
        .iter()
        .filter(|(k, _)| k.starts_with(&prefix) && k.ends_with(']'))
        .map(|(k, v)| (k[prefix.len()..k.len() - 1].to_string(), v.clone()))
        .collect();
    if pairs.is_empty() {
        return Ok(pc + 1);
    }
    // 非数値キーが存在する場合は自動的に文字インデックス扱いにする
    if !is_string_key && pairs.iter().any(|(k, _)| k.parse::<f64>().is_err()) {
        is_string_key = true;
    }
    // 非数値の値が存在する場合は文字列比較に切り替え
    let use_string_cmp = force_string_val || pairs.iter().any(|(_, v)| v.parse::<f64>().is_err());
    // 値比較クロージャ
    let cmp_val = |a: &str, b: &str| -> std::cmp::Ordering {
        if use_string_cmp {
            a.cmp(b)
        } else {
            let av = a.parse::<f64>().unwrap_or(0.0);
            let bv = b.parse::<f64>().unwrap_or(0.0);
            av.partial_cmp(&bv).unwrap_or(std::cmp::Ordering::Equal)
        }
    };
    // 数値キー比較クロージャ
    let cmp_num_key = |a: &str, b: &str| -> std::cmp::Ordering {
        let ak = a.parse::<f64>().unwrap_or(0.0);
        let bk = b.parse::<f64>().unwrap_or(0.0);
        ak.partial_cmp(&bk).unwrap_or(std::cmp::Ordering::Equal)
    };
    if is_string_key {
        // 文字インデックス: (キー, 値) ペアを一緒に移動
        // インデックスのみ → キーで, それ以外 → 値でソート
        pairs.sort_by(|a, b| {
            let cmp = if is_key_sort {
                a.0.cmp(&b.0)
            } else {
                cmp_val(&a.1, &b.1)
            };
            if order_asc {
                cmp
            } else {
                cmp.reverse()
            }
        });
    } else {
        // 数値インデックス:
        // ステップ1: キーをソート方向順に並べる (キー位置を確定させる)
        pairs.sort_by(|a, b| {
            let cmp = cmp_num_key(&a.0, &b.0);
            if order_asc {
                cmp
            } else {
                cmp.reverse()
            }
        });
        if !is_key_sort {
            // ステップ2: キーは固定し、値だけをソートして再配置
            let keys: Vec<String> = pairs.iter().map(|(k, _)| k.clone()).collect();
            let mut vals: Vec<String> = pairs.iter().map(|(_, v)| v.clone()).collect();
            vals.sort_by(|a, b| {
                let cmp = cmp_val(a, b);
                if order_asc {
                    cmp
                } else {
                    cmp.reverse()
                }
            });
            pairs = keys.into_iter().zip(vals).collect();
        }
        // インデックスのみ: ステップ1 のキー順でペアはそのまま
    }
    // 書き戻し
    for (key, val) in &pairs {
        app.set_script_var(format!("{arr_name}[{key}]"), val.clone());
    }
    Ok(pc + 1)
}

fn exec_array(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `Array variable string separator` (`Arrayコマンド.md`):
    // string を separator で分割して variable[1..N] に格納。
    // separator が `リスト` ならリスト形式 (空白区切り) として解釈。
    if xargs.len() < 3 {
        return Err(err(line, "Array 命令は 3 引数必要 (var string sep)。"));
    }
    let var_name = xargs[0].clone();
    let value = xargs[1].to_string();
    let sep = xargs[2].to_string();
    let sep = sep.trim().trim_matches('"');
    let parts: Vec<&str> = if sep == "リスト" || sep.eq_ignore_ascii_case("list") {
        // リスト形式 = 空白区切り (Llength/Lindex と同じ規約)
        value.split_whitespace().collect()
    } else if sep.is_empty() {
        // 区切り文字無指定 → 文字列を 1 文字ずつ
        value.split("").filter(|s| !s.is_empty()).collect()
    } else {
        value.split(sep).collect()
    };
    // 既存の variable[*] 要素は SRC では維持されるが、本実装では
    // 上書き範囲のみ書き換える (元のサイズより小さい場合に余りを残す)。
    for (i, part) in parts.iter().enumerate() {
        let key = format!("{var_name}[{}]", i + 1);
        app.set_script_var(key, part.to_string());
    }
    Ok(pc + 1)
}

fn exec_swap(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `Swap var1 var2` (`Swapコマンド.md`): 2 変数の値交換。
    // 配列要素も addressable なら受理 (LHS 解決経由)。
    // SRC.Sharp 準拠: 正確に 2 引数必要。
    if xargs.len() < 2 {
        return Err(err(line, "Swap 命令は 2 引数必要 (var1 var2)。"));
    }
    if xargs.len() > 2 {
        return Err(err(line, "Swapコマンドの引数の数が違います"));
    }
    let a = resolve_lhs_name(app, &xargs[0]);
    let b = resolve_lhs_name(app, &xargs[1]);
    let av = app.script_var(&a).to_string();
    let bv = app.script_var(&b).to_string();
    app.set_script_var(a, bv);
    app.set_script_var(b, av);
    Ok(pc + 1)
}

fn exec_copyarray(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd {
        xargs, line, pc, ..
    } = *c;
    // SRC `CopyArray src dst` (`CopyArrayコマンド.md`):
    // `src[*]` の全要素を `dst[*]` にコピー。dst の既存要素は維持しつつ
    // 同じキーを上書きする (SRC.NET 仕様)。完全置換ではない。
    // SRC.Sharp 準拠: 正確に 2 引数必要。
    if xargs.len() < 2 {
        return Err(err(line, "CopyArray 命令は 2 引数必要 (src dst)。"));
    }
    if xargs.len() > 2 {
        return Err(err(line, "CopyArrayコマンドの引数の数が違います"));
    }
    let src = xargs[0].clone();
    let dst = xargs[1].clone();
    let prefix = format!("{src}[");
    let pairs: Vec<(String, String)> = app
        .script_vars()
        .iter()
        .filter(|(k, _)| k.starts_with(&prefix) && k.ends_with(']'))
        .map(|(k, v)| {
            let idx = &k[prefix.len()..k.len() - 1];
            (format!("{dst}[{idx}]"), v.clone())
        })
        .collect();
    for (k, v) in pairs {
        app.set_script_var(k, v);
    }
    Ok(pc + 1)
}

fn exec_global(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { xargs, pc, .. } = *c;
    // SRC `Global variable` (`Globalコマンド.md`): グローバル変数宣言。
    // 本実装は単一スコープだが、宣言していない変数は `IsVarDefined` = 0 を
    // 返すため、未定義ならば空文字で初期化する必要がある。
    // SRC.Sharp `GlobalCmd.cs` の実装:
    //   - 各引数を変数名として処理 (複数指定可能: `Global g1 g2 g3`)
    //   - 先頭が `$` なら除去して登録 (`Global $counter` → `counter`)
    //   - 既定義の場合は値を変更しない (保持)
    for arg in xargs {
        let vname = arg.trim_start_matches('$');
        if !app.is_script_var_defined(vname) {
            app.set_script_var(vname.to_string(), String::new());
        }
    }
    Ok(pc + 1)
}

fn exec_upvar(app: &mut App, c: &Cmd) -> Result<usize, ScriptError> {
    let Cmd { pc, .. } = *c;
    // SRC `UpVar` コマンド: 現フレームから見た祖先フレームの引数への
    // アクセスを可能にする。呼び出しごとに 1 段上の祖先を追加で参照。
    //
    // 実装: `upvar_level` を 1 増やし、その段の祖先フレームの
    // 保存済み Args を現フレームの Args に「上書きマージ」する。
    //
    // saved_args layout: [Args(1)..Args(9), ArgNum, ...]  (先頭 10 要素を使用)
    //
    // SRC 仕様詳細:
    //   - 引数なしサブルーチン内: Args(i) = 親フレームの i 番目引数
    //   - 引数ありサブルーチン内: Args(1..c) は自分の引数を維持し、
    //     Args(c+1..) に親フレームの引数を追記。ArgNum = c + 親 ArgNum
    //   - 複数回呼んだ場合: 各回でさらに 1 段上の祖先を追記
    //   - チェーン UpVar (祖先も UpVar 済み): 祖先の saved_args がその
    //     時点で拡張済みのため、自動的に全祖先の引数が合成される
    let upvar_level = app.upvar_level() + 1;
    app.set_upvar_level(upvar_level);

    // 最初の UpVar 呼び出しでベースとなる ArgNum を記録
    if upvar_level == 1 {
        let base = app.script_var("ArgNum").parse::<usize>().unwrap_or(0);
        app.set_upvar_base_argnum(base);
    }

    let base = app.upvar_base_argnum();
    let depth = app.call_stack_depth();

    // 必要な祖先フレームが存在する場合のみマージ
    if depth >= upvar_level {
        let ancestor_idx = depth - upvar_level;
        if let Some(saved) = app.call_stack_saved_args(ancestor_idx) {
            let saved = saved.clone(); // 借用解除のためコピー
                                       // layout: [upvar_level, upvar_base_argnum, ArgNum, Args(1)..]
            let parent_argnum = saved
                .get(2)
                .and_then(|s| s.parse::<usize>().ok())
                .unwrap_or(0)
                .min(MAX_CALL_ARGS);

            // 祖先の Args(1..parent_argnum) を base の直後から設定。
            // まず base+1 以降をクリア (前回の UpVar 分を上書き)。
            let cur = current_argnum(app);
            for k in (base + 1)..=cur.max(base + parent_argnum) {
                app.set_script_var(format!("Args({k})"), String::new());
            }
            for i in 0..parent_argnum {
                let val = saved.get(3 + i).cloned().unwrap_or_default();
                app.set_script_var(format!("Args({})", base + i + 1), val);
            }
            // ArgNum を更新
            app.set_script_var("ArgNum".to_string(), (base + parent_argnum).to_string());
        }
    }
    // 祖先が存在しない (トップレベルから呼ばれた) 場合は no-op
    Ok(pc + 1)
}

/// `Eval(式)` 形式の左辺なら括弧内の式を返す (関数名は大文字小文字を区別しない)。
fn eval_lhs_target(lhs: &str) -> Option<&str> {
    let head = lhs.get(..5)?;
    let inner = lhs[5..].strip_suffix(')')?;
    head.eq_ignore_ascii_case("eval(").then(|| inner.trim())
}
