//! 代入先 (添字付き変数・関数左辺値・システム変数) の解決、システム変数の読み出し、
//! `Call` 引数スタック。値の評価は統一評価器 (`eval`) が担う。

use super::*;

/// `Set name[expr]` の LHS を評価して、実際の格納キーを返す。
/// `expr` 部は再帰的に評価し、`[exprA][exprB]` のような多次元アクセスにも対応。
/// `name` が `[` を含まなければそのまま返す。
pub(super) fn resolve_lhs_name(app: &App, raw: &str) -> String {
    let bytes = raw.as_bytes();
    let Some(bracket_pos) = bytes.iter().position(|&b| b == b'[') else {
        return raw.to_string();
    };
    let mut depth = 1i32;
    let mut k = bracket_pos + 1;
    while k < bytes.len() && depth > 0 {
        match bytes[k] {
            b'[' => depth += 1,
            b']' => depth -= 1,
            _ => {}
        }
        k += 1;
    }
    if depth != 0 {
        return raw.to_string();
    }
    let name = &raw[..bracket_pos];
    let inner = &raw[bracket_pos + 1..k - 1];
    // 多次元添字 `name[i,j]` はトップレベルのカンマで分割し、各添字を
    // 個別に解決して `name[v1,v2]` に正規化する。これにより
    // `Set 搭乗員[1,3] …` と `$(搭乗員[i,j])` (i=1,j=3) が同じキーを指す。
    let inner_val = split_function_args(inner)
        .iter()
        .map(|part| resolve_index_part(app, part))
        .collect::<Vec<_>>()
        .join(",");
    let trailing = &raw[k..];
    let head = format!("{name}[{inner_val}]");
    if trailing.is_empty() {
        head
    } else {
        format!("{head}{}", resolve_lhs_name(app, trailing))
    }
}

/// `HP(unit) = n` / `EN(unit) = n` 等、関数名を左辺値とする代入を処理する。
///
/// SRC.Sharp は次の関数を「代入可能な左辺値」として定義する:
/// - `HP(unit)`     → ユニットの現在 HP を設定 (clamp: 1..=MaxHP)
/// - `EN(unit)`     → ユニットの現在 EN を設定 (clamp: 0..=MaxEN)
/// - `Action(unit)` → 残り行動数を設定 (`has_acted = (n <= 0)`)
/// - `Morale(unit)` → 士気を設定 (clamp: 0..=150)
/// - `SP(pilot)`    → SP を設定 (clamp: 0..=MaxSP)
/// - `Plana(pilot)` → 霊力を設定
///
/// 処理した場合は `true` を返す。`lhs` が関数呼出形式でない、または上記以外の
/// 関数名の場合は `false` を返し、呼び出し元が通常の `set_script_var` を続ける。
/// システム変数への書き込みを処理する。SRC.Sharp `Variable.cs` の `SetVariable`
/// の特殊ケース (`ターン数` / `総ターン数` / `資金`) に対応。
/// 書き込みを処理した場合は `true`、対象外なら `false`。
pub(super) fn try_system_var_assign(app: &mut App, lhs: &str, value: &str) -> bool {
    // 代入値は評価済みなので数値として読むだけ (数値でなければ 0)。
    let n_i64 = eval::str_to_lng(value);
    let n_i64 = || n_i64;
    match lhs {
        "ターン数" => {
            app.set_turn_number(n_i64().max(0) as u32);
            true
        }
        "総ターン数" => {
            app.set_total_turn(n_i64().max(0) as u32);
            true
        }
        "資金" => {
            app.set_money(n_i64());
            true
        }
        // ArgNum は読み取り専用 (SRC 仕様)。Set/Incr を silent no-op で吸収。
        "ArgNum" => true,
        _ => false,
    }
}

pub(super) fn try_function_lhs_assign(app: &mut App, lhs: &str, value: &str) -> bool {
    // `FuncName(arg)` の形式か判定。
    let Some(open) = lhs.find('(') else {
        return false;
    };
    if !lhs.ends_with(')') {
        return false;
    }
    let func_name = &lhs[..open];
    let arg = lhs[open + 1..lhs.len() - 1].trim();
    let lower = func_name.to_ascii_lowercase();

    // 代入値は評価済みなので数値として読むだけ (数値でなければ 0)。
    let n = eval::str_to_lng(value);

    match lower.as_str() {
        "hp" => {
            // HP(unit) = n → damage = max_hp - clamp(n, 0, max_hp)
            // C# Unit.HP setter: max(0, min(MaxHP, value)) → HP=0 は有効 (ユニット撃墜状態)。
            let Some(idx) = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, arg))
            else {
                return true;
            };
            let max_hp = {
                let db = app.database();
                db.effective_max_hp(&db.unit_instances[idx])
            };
            let new_hp = n.clamp(0, max_hp.max(0));
            app.database_mut().unit_instances[idx].damage = (max_hp - new_hp).max(0);
            true
        }
        "en" => {
            // EN(unit) = n → en_consumed = max_en - clamp(n, 0, max_en)
            let Some(idx) = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, arg))
            else {
                return true;
            };
            let max_en = {
                let db = app.database();
                db.effective_max_en(&db.unit_instances[idx])
            };
            let new_en = (n as i32).clamp(0, max_en);
            app.database_mut().unit_instances[idx].en_consumed = (max_en - new_en).max(0);
            true
        }
        "morale" => {
            // Morale(pilot_or_unit) = n → morale = clamp(n, 50, 150)
            // C# Pilot.SetMorale: clamp(MinMorale=50, MaxMorale=150)。
            let Some(idx) = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, arg))
            else {
                return true;
            };
            app.database_mut().unit_instances[idx].morale = (n as i32).clamp(50, 150);
            true
        }
        "action" => {
            // Action(unit) = n → has_acted = (n <= 0)
            // n > 0: 行動可能 (has_acted = false)、n <= 0: 行動済 (has_acted = true)。
            let Some(idx) = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, arg))
            else {
                return true;
            };
            app.database_mut().unit_instances[idx].has_acted = n <= 0;
            true
        }
        "sp" => {
            // SP(pilot) = n → sp_consumed = max(0, max_sp - n)
            let Some(idx) = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, arg))
            else {
                return true;
            };
            let max_sp = {
                let db = app.database();
                let pilot_name = db.unit_instances[idx].pilot_name.clone();
                db.pilot_by_name(&pilot_name)
                    .and_then(|p| p.sp)
                    .unwrap_or(0)
            };
            let new_sp = (n as i32).clamp(0, max_sp);
            app.database_mut().unit_instances[idx].sp_consumed = (max_sp - new_sp).max(0);
            true
        }
        "plana" => {
            // Plana(pilot) = n → plana = n
            let Some(idx) = app
                .database()
                .unit_instances
                .iter()
                .position(|u| matches_unit_handle(u, arg))
            else {
                return true;
            };
            app.database_mut().unit_instances[idx].plana = n as i32;
            true
        }
        _ => false,
    }
}

/// `name[i,j]` の添字 1 つ分を評価する (原典 `GetVariable` の添字処理:
/// `GetValueAsString(添字, is_term)`)。数値になる添字 (`3` / `(i + 1)` / `j - 1`) は
/// 数値表記に、それ以外 (`R-1` のようなハイフン付きの 1 項) は文字列キーになる。
pub(super) fn resolve_index_part(app: &App, part: &str) -> String {
    eval::param_value(app, part.trim())
}

/// 評価済みの値を数値として読む。数値として読めない文字列 (`"AB"` 等) は `None` を返し、
/// 「数値なら数値・そうでなければ文字列」で扱いを分ける呼出元 (`LSet` / `RSet` 等) の
/// 契約を守る。引数の評価は呼び出し側 (関数引数・コマンド引数) で済んでいる。
pub(super) fn numeric_arg(value: &str) -> Option<f64> {
    let t = value.trim();
    if let Ok(v) = t.parse::<f64>() {
        return Some(v);
    }
    (!t.is_empty() && !t.contains(char::is_whitespace) && eval::is_numeric(t))
        .then(|| eval::str_to_dbl(t))
}

/// 評価済みの値を整数として読む (原典 `GetValueAsLong`: 数値でなければ 0、ゼロ方向へ切り捨て)。
pub(super) fn int_arg(value: &str) -> i64 {
    eval::str_to_dbl(value).trunc() as i64
}

/// 本移植が `AppVersion` で報告する SRC 互換バージョン番号。
///
/// 原典 SRC.Sharp `Expression.cs` の `appversion` は
/// `10000*Major + 100*Minor + Revision` を返す。本移植は公式フルパッケージ
/// 版 2.2.33 (本リポジトリの互換目標) 相当を報告する: `10000*2 + 100*2 + 33`。
/// SRC 2.00 以降を要求するシナリオ (`If AppVersion < 20000` 等) を通すための値。
pub const SRC_APP_VERSION: i64 = 10000 * 2 + 100 * 2 + 33;

/// SRC のシステム変数 (`味方数` / `敵数` / `友軍数` / `中立数` / `ターン数` 等)
/// に対する動的解決。該当しない名前なら `None`。
pub(super) fn system_variable_value(app: &App, name: &str) -> Option<String> {
    use crate::Party;
    let count_party = |p: Party| -> String {
        app.database()
            .unit_instances
            .iter()
            .filter(|u| !u.off_map && u.party == p)
            .count()
            .to_string()
    };
    // 各陣営の平均レベル (配置済みユニットのパイロットレベルの平均、切り捨て)。
    // レベルは `Level()` と同じく total_exp から算出 ((exp/100)+1, 上限 99)。
    let avg_level = |p: Party| -> String {
        let levels: Vec<i64> = app
            .database()
            .unit_instances
            .iter()
            .filter(|u| !u.off_map && u.party == p)
            .map(|u| i64::from(crate::pilot_instance::level_from_exp(u.total_exp)))
            .collect();
        if levels.is_empty() {
            "0".to_string()
        } else {
            (levels.iter().sum::<i64>() / levels.len() as i64).to_string()
        }
    };
    match name {
        "味方数" => Some(count_party(Party::Player)),
        "敵数" => Some(count_party(Party::Enemy)),
        // SRC.Sharp: "友軍" は "ＮＰＣ" 陣営。`ＮＰＣ数` を `友軍数` のエイリアスとして扱う。
        "ＮＰＣ数" | "友軍数" => Some(count_party(Party::Npc)),
        "中立数" => Some(count_party(Party::Neutral)),
        // 各陣営の平均レベル。実シナリオ (リンと凛 第038話) が `(味方レベル平均値 - 1)` を
        // 座標式に使う。SRC.Sharp 由来の集約値。
        "味方レベル平均値" => Some(avg_level(Party::Player)),
        "敵レベル平均値" => Some(avg_level(Party::Enemy)),
        "ＮＰＣレベル平均値" | "友軍レベル平均値" => Some(avg_level(Party::Npc)),
        "中立レベル平均値" => Some(avg_level(Party::Neutral)),
        "ターン数" => Some(app.turn().number.to_string()),
        "総ターン数" => Some(app.total_turn().to_string()),
        // SRC.Sharp `Expression.cs` の組込変数 `AppVersion`。SRC 2.00 以降を
        // 要求するシナリオ (Welcome.eve 冒頭の版数ゲート等) を通すために報告する。
        // 原典は変数名を小文字化して判別するため大小無視でマッチする。
        n if n.eq_ignore_ascii_case("AppVersion") => Some(SRC_APP_VERSION.to_string()),
        // `フェイズ` は現在のフェーズの陣営名。SRC.Sharp の `SRC.Stage` 相当。
        "フェイズ" => Some(app.turn().phase.stage_name().to_string()),
        // `資金` は `App.money()` から動的に解決する。
        "資金" => Some(app.money().to_string()),
        // ArgNum はトップレベル (Call 外) でも "0" を返す。
        // Call 中は enter_call_args が script_var("ArgNum") に引数数をセット済みなので
        // script_var 値が空文字でなければそちらを優先する。
        "ArgNum" => {
            let v = app.script_var("ArgNum");
            Some(if v.is_empty() {
                "0".to_string()
            } else {
                v.to_string()
            })
        }
        _ => None,
    }
}

/// `Call` 時の Args 束縛: 呼び出し元の `Args(1..9)` / `ArgNum` / `upvar_level` /
/// `upvar_base_argnum` をスナップショットして退避し、`new_args` を新しい
/// `Args(1..)` / `ArgNum` として束縛する。退避した値を返す
/// (`push_call_return` に渡し、`Return` で `restore_call_args` する)。
///
/// SRC の `Args` は呼び出しフレーム単位なので、ネストした `Call` が
/// 呼び出し元の `Args` を破壊しないようこのスナップショット/復元が要る。
///
/// saved の layout (可変長):
/// `[upvar_level, upvar_base_argnum, ArgNum, Args(1), ..., Args(ArgNum)]`
///
/// 原典の引数配列は `ReDim strArgs(2 To ArgNum)` の動的配列で個数制限が無い
/// (`CmdData.cls`)。旧実装は `Args(1..9)` 固定で 10 個目以降を取りこぼして
/// いた (実コーパスのサンプル 79 件中 9 件が `Args(10)` 以上を参照し、
/// 最大 `Args(14)` を確認)。
pub(super) fn enter_call_args(app: &mut App, new_args: &[String]) -> Vec<String> {
    let prev_argnum = current_argnum(app);
    let mut saved: Vec<String> = vec![
        app.upvar_level().to_string(),
        app.upvar_base_argnum().to_string(),
        prev_argnum.to_string(),
    ];
    saved.extend((1..=prev_argnum).map(|k| app.script_var(&format!("Args({k})")).to_string()));

    // 新フレーム用に Args をクリアして引数をセット。
    // 呼び出し元の分と新引数の分、両方を覆う範囲を消す。
    for k in 1..=prev_argnum.max(new_args.len()) {
        app.set_script_var(format!("Args({k})"), String::new());
    }
    // 引数は呼び出し側で評価済みの値 (`Call` はコマンド引数として `GetArgAsString`、
    // ユーザ定義関数は `eval::param_value`) なので、そのまま束縛する。
    for (i, v) in new_args.iter().enumerate() {
        app.set_script_var(format!("Args({})", i + 1), v.clone());
    }
    // ArgNum = 渡した引数の数
    app.set_script_var("ArgNum".to_string(), new_args.len().to_string());
    // 新フレームの upvar 状態を初期化
    app.set_upvar_level(0);
    app.set_upvar_base_argnum(0);
    saved
}

/// `Return` 時に呼び出し元の `Args(..)` / `ArgNum` / `upvar_level` /
/// `upvar_base_argnum` を復元する。layout は [`enter_call_args`] 参照。
pub(super) fn restore_call_args(app: &mut App, saved: Vec<String>) {
    let upvar_level = saved.first().and_then(|s| s.parse::<usize>().ok());
    let upvar_base = saved.get(1).and_then(|s| s.parse::<usize>().ok());
    let saved_argnum = saved
        .get(2)
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0)
        .min(MAX_CALL_ARGS);
    // 呼び出された側が使っていた Args を消してから、呼び出し元の分を書き戻す。
    let callee_argnum = current_argnum(app);
    for k in (saved_argnum + 1)..=callee_argnum.max(saved_argnum) {
        app.set_script_var(format!("Args({k})"), String::new());
    }
    for k in 1..=saved_argnum {
        let v = saved.get(2 + k).cloned().unwrap_or_default();
        app.set_script_var(format!("Args({k})"), v);
    }
    app.set_script_var("ArgNum".to_string(), saved_argnum.to_string());
    app.set_upvar_level(upvar_level.unwrap_or(0));
    app.set_upvar_base_argnum(upvar_base.unwrap_or(0));
}

/// 現フレームの `ArgNum`。異常値は上限で頭打ちにする。
pub(super) fn current_argnum(app: &App) -> usize {
    app.script_var("ArgNum")
        .parse::<usize>()
        .unwrap_or(0)
        .min(MAX_CALL_ARGS)
}
