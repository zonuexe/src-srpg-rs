//! ゲーム側の出来事に応じたイベントラベルの自動発火 (破壊・全滅・勝敗・使用/攻撃/
//! 進入/接触/行動終了・損傷率)。`App` の戦闘/移動処理からも呼ばれる。

use super::*;

/// SRC `Win` / `GameClear` 後の自動発火ラベル ({Victory, 勝利, Ending,
/// エンディング}) を 1 件だけ発火する。`App::game_clear()` の lookup と一致。
pub(super) fn fire_victory_labels(app: &mut App) {
    // 勝利 / エンディング は章ローカル (各章の `勝利` ラベル)。
    for lab in ["Victory", "勝利", "Ending", "エンディング"] {
        if app.post_stage_event_label(lab.to_string()) {
            return;
        }
    }
}

/// SRC `Lose` / `GameOver` 後の自動発火ラベル ({GameOver, ゲームオーバー})
/// を 1 件だけ発火する。`App::game_over()` の lookup と一致。
pub(super) fn fire_game_over_labels(app: &mut App) {
    for lab in ["GameOver", "ゲームオーバー"] {
        if app.post_event_label(lab.to_string()) {
            return;
        }
    }
}

/// SRC `Event.HandleEvent("Destruction <name>")` 相当の auto-fire。
///
/// pilot 名 / unit_data 名 / 日英両綴 (`Destruction` / `破壊`) を順に試し、
/// 最初にヒットした 1 件だけ発火する (SRC 原典も最初の matching event だけ
/// 実行する)。続いて該当ユニットの陣営が全滅しているかをチェックし、
/// 全滅していれば `全滅 <party>` ラベルを発火する
/// (SRC.Sharp `Unit.attackmap.cs:1047` / `Unit.ref.cs:25393` 同等)。
///
/// 発火は `App::post_event_label` (原典 `EventQue` 相当) 経由で行う:
/// スクリプト実行中 (`Kill` / `Damage` コマンド等から呼ばれた場合) は
/// キューに積まれ、現在のスクリプト完了後に実行される。これにより
/// Destruction ハンドラに `Talk` 等の suspend 系命令が含まれていても
/// 外側の dispatch ctx を上書きしない (旧実装は再入で上書きハザードが
/// あり、同期完結ハンドラ前提で割り切っていた)。
pub(crate) fn fire_destruction_labels(
    app: &mut App,
    pilot_name: &str,
    unit_data_name: &str,
    party: crate::Party,
) {
    fire_destruction_labels_inner(
        app,
        pilot_name,
        unit_data_name,
        party,
        &["Destruction", "破壊"],
    );
}

/// マップ攻撃で撃破された対象用の破壊系イベント (SRC VB5 `Unit.cls:17214` /
/// `Event.bas:1744` で `破壊` と同じ `DestructionEventLabel`)。原典はマップ攻撃の
/// 撃破に対し `破壊` ではなく **`マップ攻撃破壊 <対象>`** を発火する。対象ユニット
/// システム変数の設定と `全滅 <party>` 発火は `破壊` と共通。プレイヤー/AI 発の
/// マップ攻撃 (is_event=false) でのみ使う。
pub(crate) fn fire_map_attack_destruction_labels(
    app: &mut App,
    pilot_name: &str,
    unit_data_name: &str,
    party: crate::Party,
) {
    fire_destruction_labels_inner(app, pilot_name, unit_data_name, party, &["マップ攻撃破壊"]);
}

pub(super) fn fire_destruction_labels_inner(
    app: &mut App,
    pilot_name: &str,
    unit_data_name: &str,
    party: crate::Party,
    prefixes: &[&str],
) {
    // pilot名 / unit名 に加え、関数形 `Pilot(<unit名>)` / `Unit(<pilot名>)`、
    // **陣営名** (`破壊 味方:` / `破壊 敵:` 等) も試す。SRC は 破壊ラベルの識別子を
    // ユニット識別子 (pilot/unit/ID/陣営) として照合する (`SearchLabel` is_unit)。
    // 実シナリオは `破壊 Pilot(補給艦リームズ):` や `破壊 味方:` (Switch 対象ユニット)
    // のように書く。
    // 破壊イベント本体が `Switch 対象ユニット` / `$(対象ユニット)` で撃破ユニットを
    // 参照できるよう、対象系システム変数を撃破ユニットに設定する (SRC `対象ユニット`
    // = メインパイロット名)。決戦！宇宙怪獣2話 `破壊 味方: / Switch 対象ユニット`。
    if !pilot_name.is_empty() {
        app.set_script_var("対象ユニット".to_string(), pilot_name.to_string());
        app.set_script_var("対象パイロット".to_string(), pilot_name.to_string());
    }
    let mut targets: Vec<String> = Vec::with_capacity(5);
    if !pilot_name.is_empty() {
        targets.push(pilot_name.to_string());
        targets.push(format!("Unit({pilot_name})"));
    }
    if !unit_data_name.is_empty() {
        targets.push(unit_data_name.to_string());
        targets.push(format!("Pilot({unit_data_name})"));
    }
    targets.push(party_long_label(party).to_string());
    for prefix in prefixes {
        for target in &targets {
            let label = format!("{prefix} {target}");
            if app.post_stage_event_label(label) {
                break;
            }
        }
    }
    fire_total_annihilation_if_any(app);
}

/// 該当陣営のユニットが 1 機も残っていなければ `全滅 <party>` ラベルを発火。
/// 4 陣営それぞれ確認する。ユニット 0 = 一度も登場していない場合も含むが、
/// それは label 未定義なので silent OK。
pub(super) fn fire_total_annihilation_if_any(app: &mut App) {
    for (party, party_label) in party_long_labels() {
        let alive = app
            .database()
            .unit_instances
            .iter()
            .any(|u| u.party == party && !u.off_map);
        if !alive {
            // 1 度でも当該陣営のユニットが配置されていた場合のみ意味あり。
            // 全滅 label が定義されていなければ no-op。章ローカルイベント。
            let label = format!("全滅 {party_label}");
            if app.post_stage_event_label(label) {
                return;
            }
        }
    }
}

/// `Party` 値 → SRC 表記 (`味方`/`敵`/`友軍`/`中立`) の対応表。
/// SRC の auto-fire ラベル (`全滅 <party>` / `損傷率 <party> <pct>` 等) は
/// この日本語綴を使う。
pub(super) fn party_long_labels() -> [(crate::Party, &'static str); 4] {
    use crate::Party;
    [
        (Party::Player, "味方"),
        (Party::Enemy, "敵"),
        (Party::Neutral, "中立"),
        (Party::Npc, "ＮＰＣ"),
    ]
}

pub(super) fn party_long_label(party: crate::Party) -> &'static str {
    for (p, label) in party_long_labels() {
        if p == party {
            return label;
        }
    }
    ""
}

/// SRC `攻撃 <atk> <def>:` (`攻撃イベント.md`) / `攻撃後 <atk> <def>:`
/// (`攻撃後イベント.md`) 共通の auto-fire ヘルパ。
///
/// `prefixes` (`["攻撃"]` または `["攻撃後"]`) と (attacker, defender) の
/// (pilot_name, unit_data_name, party_label) 9 組合せを順に試し、最初の
/// マッチで 1 度だけ発火する。
///
/// 発火順 (SRC.NET 仕様):
/// - prefix → atk identifier 優先順 (pilot → unit → party) → def 同順。
///
/// 仕様乖離: `攻撃イベント` は `Attack`/`MapAttack` 等のシナリオ戦闘では
/// 発火しない (= UI 経路の通常戦闘でだけ発火) という規定があるが、本実装は
/// `attack_target` 経由 (= UI 攻撃のみ) で呼び出すため自然に満たす。
pub(super) fn fire_pair_event_labels(
    app: &mut App,
    prefixes: &[&str],
    atk: &UnitEventId,
    def: &UnitEventId,
) {
    let atk_ids = event_label_identifiers(atk);
    let def_ids = event_label_identifiers(def);
    for prefix in prefixes {
        for a in &atk_ids {
            for d in &def_ids {
                let label = format!("{prefix} {a} {d}");
                // 会話 / 攻撃 / 接触 イベントは章ローカル。
                if app.post_stage_event_label(label) {
                    return;
                }
            }
        }
    }
}

/// イベントラベルのユニット側識別子候補。pilot 名 / unit 名 / 陣営名に加えて、
/// **関数形** `Pilot(<unit名>)` / `Unit(<pilot名>)` も生成する。
///
/// SRC のシナリオは特定ユニットを指すのに `接触 Pilot(補給艦リームズ) 敵:` /
/// `破壊 Pilot(補給艦リームズ):` のように関数をラベル名に書く。ラベルは関数テキスト
/// のまま登録されるため、当該ユニットの unit名/pilot名から関数形キーを再構成して
/// 突き合わせる (式評価不要)。
pub(super) fn event_label_identifiers(id: &UnitEventId) -> Vec<String> {
    let mut ids = Vec::with_capacity(5);
    if !id.pilot.is_empty() {
        ids.push(id.pilot.clone());
        ids.push(format!("Unit({})", id.pilot));
    }
    if !id.unit.is_empty() {
        ids.push(id.unit.clone());
        ids.push(format!("Pilot({})", id.unit));
    }
    ids.push(party_long_label(id.party).to_string());
    ids
}

/// 攻撃 / 攻撃後 イベントの 1 ユニット側識別子 3 種 (pilot, unit, party)。
#[derive(Debug, Clone)]
pub struct UnitEventId {
    pub pilot: String,
    pub unit: String,
    pub party: crate::Party,
}

impl UnitEventId {
    pub fn from_unit_instance(u: &crate::UnitInstance) -> Self {
        Self {
            pilot: u.pilot_name.clone(),
            unit: u.unit_data_name.clone(),
            party: u.party,
        }
    }
}

/// SRC `使用 <unit> <device>:` (`使用イベント.md`) の auto-fire。
/// `attack_target` で `攻撃イベント` 発火前に呼ぶ。`device` は武器名 /
/// アビリティ名 / SP 名 / `召喚解除` のいずれか。
///
/// 仕様準拠: サポートアタックの武器では発火しない (本実装ではサポート
/// アタックパスは別の関数経路を通るため自然に満たす)。反撃武器は
/// "攻撃前タイミング" で発火するため、本実装でも反撃時に attack_target が
/// 再帰呼び出しされる場合は同様の挙動になる。
pub fn fire_use_event_labels(app: &mut App, idx: usize, device: &str) {
    let Some(u) = app.database().unit_instances.get(idx) else {
        return;
    };
    let pilot_name = u.pilot_name.clone();
    let unit_data_name = u.unit_data_name.clone();
    let party = u.party;
    fire_unit_event_labels(
        app,
        &["使用"],
        &pilot_name,
        &unit_data_name,
        party,
        Some(device),
    );
}

/// SRC `使用後 <unit> <device>:` (`使用後イベント.md`) の auto-fire。
/// `攻撃後イベント` 発火直前に呼ぶ。
///
/// 仕様準拠: 使用したユニットが生存している場合にのみ発火する。`UnitEventId`
/// で attacker を退避してから呼ぶ前提 (撃破された attacker は `unit_instances`
/// から除去されており、生存判定で false になる)。
pub fn fire_after_use_event_labels(app: &mut App, atk: &UnitEventId, device: &str) {
    let alive = app
        .database()
        .unit_instances
        .iter()
        .any(|u| !u.off_map && (u.pilot_name == atk.pilot || u.unit_data_name == atk.unit));
    if !alive {
        return;
    }
    fire_unit_event_labels(
        app,
        &["使用後"],
        &atk.pilot,
        &atk.unit,
        atk.party,
        Some(device),
    );
}

/// SRC `攻撃 <atk> <def>:` (`攻撃イベント.md`) の auto-fire。
/// `attack_target` 直前で呼び出す。両ユニットがマップ上に居る前提。
pub fn fire_attack_event_labels(app: &mut App, atk_idx: usize, def_idx: usize) {
    let atk = match app.database().unit_instances.get(atk_idx) {
        Some(u) => UnitEventId::from_unit_instance(u),
        None => return,
    };
    let def = match app.database().unit_instances.get(def_idx) {
        Some(u) => UnitEventId::from_unit_instance(u),
        None => return,
    };
    fire_pair_event_labels(app, &["攻撃"], &atk, &def);
}

/// SRC `攻撃後 <atk> <def>:` (`攻撃後イベント.md`) の auto-fire。
/// `attack_target` の damage / Destruction / 反撃 全完了後に呼び出す。
/// 両ユニットがマップ上に **生存** している場合のみ発火する (撃破された
/// 側が片方でも居なくなった場合は no-op)。
///
/// 呼び出し側は damage 適用前に `UnitEventId` を退避してから渡すこと。
/// 撃破で `unit_instances` から抜けたユニットは生存判定で false になり、
/// 発火されない。
pub fn fire_after_attack_event_labels(app: &mut App, atk: &UnitEventId, def: &UnitEventId) {
    let alive = |pilot: &str, unit: &str| {
        app.database()
            .unit_instances
            .iter()
            .any(|u| !u.off_map && (u.pilot_name == pilot || u.unit_data_name == unit))
    };
    if !alive(&atk.pilot, &atk.unit) || !alive(&def.pilot, &def.unit) {
        return;
    }
    fire_pair_event_labels(app, &["攻撃後"], atk, def);
}

/// SRC `進入 <unit> <x> <y>:` または `進入 <unit> <terrain>:`
/// (`進入イベント.md`) の auto-fire。
///
/// 移動完了時に呼び出す。ラベルの `<x> <y>` は原典どおり 1 始まり (左上隅が (1,1)、
/// `X()` / `Y()` 関数と同じ) で、内部座標から `coord::to_script` で直して照合する。
/// 引き続き `脱出 <unit> <direction>` をマップ端到達時にチェック・発火する
/// (原典: "進入イベントに続いて脱出イベントが発生")。
///
/// `Move` 命令経由のスクリプト移動では発火しない (原典準拠)。本関数は UI
/// 経路 (`app.rs::try_move_unit_to` 等) からのみ呼び出すこと。
pub fn fire_entry_event_labels(app: &mut App, idx: usize) {
    let Some(u) = app.database().unit_instances.get(idx) else {
        return;
    };
    let pilot_name = u.pilot_name.clone();
    let unit_data_name = u.unit_data_name.clone();
    let party = u.party;
    let x = u.x;
    let y = u.y;
    // (1) 進入 <unit> <x> <y>
    let coord_suffix = format!("{} {}", coord::to_script(x), coord::to_script(y));
    fire_unit_event_labels(
        app,
        &["進入"],
        &pilot_name,
        &unit_data_name,
        party,
        Some(&coord_suffix),
    );
    // (2) 進入 <unit> <terrain_name>
    let terrain_name = {
        let map = app.database().map.as_ref();
        let terrain_id = map.map(|m| m.cell(x, y).terrain_id);
        terrain_id.and_then(|tid| {
            app.database()
                .catalog
                .terrains
                .iter()
                .find(|t| t.id == tid)
                .map(|t| t.name.clone())
                .or_else(|| crate::data::terrain::lookup(tid).map(|t| t.name.to_string()))
        })
    };
    if let Some(name) = terrain_name {
        if !name.is_empty() {
            fire_unit_event_labels(
                app,
                &["進入"],
                &pilot_name,
                &unit_data_name,
                party,
                Some(&name),
            );
        }
    }
    // (3) 脱出 <unit> <direction> がマップ端到達時に発火
    let map_size = app
        .database()
        .map
        .as_ref()
        .map(|m| (m.width, m.height))
        .unwrap_or((0, 0));
    let mut dirs: Vec<&str> = Vec::new();
    if map_size.0 > 0 && map_size.1 > 0 {
        if y == 0 {
            dirs.push("N");
        }
        if y + 1 == map_size.1 {
            dirs.push("S");
        }
        if x == 0 {
            dirs.push("W");
        }
        if x + 1 == map_size.0 {
            dirs.push("E");
        }
    }
    for dir in dirs {
        fire_unit_event_labels(
            app,
            &["脱出"],
            &pilot_name,
            &unit_data_name,
            party,
            Some(dir),
        );
    }
}

/// SRC `接触 <unit1> <unit2>:` (`接触イベント.md`) の auto-fire。
/// 行動終了直後に該当ユニットの 4 近傍ユニットと組み合わせて発火する。
///
/// 各 (idx, neighbor) ペアで `攻撃` と同じ 3x3 識別子マッチ
/// (pilot × unit × party) を試行。最初にヒットしたものを 1 度発火する。
/// SRC 原典: "行動終了後に発生"。
pub fn fire_contact_event_labels(app: &mut App, idx: usize) {
    let (x, y, atk) = match app.database().unit_instances.get(idx) {
        Some(u) => (u.x, u.y, UnitEventId::from_unit_instance(u)),
        None => return,
    };
    let neighbors: Vec<(u32, u32)> = {
        let dx_dy: &[(i32, i32)] = &[(0, -1), (0, 1), (-1, 0), (1, 0)];
        dx_dy
            .iter()
            .filter_map(|(dx, dy)| {
                let nx = x as i32 + dx;
                let ny = y as i32 + dy;
                if nx < 0 || ny < 0 {
                    return None;
                }
                Some((nx as u32, ny as u32))
            })
            .collect()
    };
    let neighbor_ids: Vec<UnitEventId> = neighbors
        .iter()
        .flat_map(|(nx, ny)| {
            app.database()
                .unit_instances
                .iter()
                .filter(move |u| u.x == *nx && u.y == *ny && !u.off_map)
                .map(UnitEventId::from_unit_instance)
                .collect::<Vec<_>>()
        })
        .collect();
    for def in neighbor_ids {
        fire_pair_event_labels(app, &["接触"], &atk, &def);
    }
}

/// SRC `行動終了 <unit>:` (`行動終了イベント.md`) の auto-fire。
///
/// `has_acted = true` をセットした直後に App から呼び出す。`idx` は
/// `unit_instances` 内のインデックス。pilot 名 / ユニット名 / 陣営名 の
/// いずれか先頭マッチで 1 度だけ発火する。
///
/// 注意: `has_acted = true` をセットしてからこの関数を呼ぶこと。スクリプト
/// 側から `Count()` 等で「行動終了済」を参照する場合の整合性を保つため。
pub fn fire_action_end_labels(app: &mut App, idx: usize) {
    let Some(u) = app.database().unit_instances.get(idx) else {
        return;
    };
    let pilot_name = u.pilot_name.clone();
    let unit_data_name = u.unit_data_name.clone();
    let party = u.party;
    fire_unit_event_labels(
        app,
        &["行動終了"],
        &pilot_name,
        &unit_data_name,
        party,
        None,
    );
}

/// `fire_unit_event_labels` の public wrapper。`App::fire_boarding_event` 等
/// app.rs から呼べるよう公開している。戻り値は発火したかどうか。
pub fn fire_unit_event_labels_public(
    app: &mut App,
    prefixes: &[&str],
    pilot_name: &str,
    unit_data_name: &str,
    party: crate::Party,
) -> bool {
    // `fire_unit_event_labels` は戻り値を返さないので、post_event_label を
    // 直接呼ぶ模倣 (3 prefix × 3 identifier の優先順)。
    let party_label = party_long_label(party);
    for prefix in prefixes {
        for target in [pilot_name, unit_data_name, party_label] {
            if target.is_empty() {
                continue;
            }
            let label = format!("{prefix} {target}");
            // 収納 / 変形 等のユニットイベントは章ローカル。
            if app.post_stage_event_label(label) {
                return true;
            }
        }
    }
    false
}

/// 1 ユニットに紐づく auto-fire ラベルを `prefix [suffix]` 形式で順に試して
/// 最初にヒットしたものを発火する。`prefix` は `["変形", "Transform"]` のように
/// 日英両綴。`identifier` 候補は (pilot_name, unit_data_name, party_label) を
/// 順に試す。
///
/// SRC `Event.bas::HandleEvent` 順に揃えており、最初の matching label だけ
/// 発火する (原典は同 prefix 複数 label の同時発火を行わない)。
pub(super) fn fire_unit_event_labels(
    app: &mut App,
    prefixes: &[&str],
    pilot_name: &str,
    unit_data_name: &str,
    party: crate::Party,
    suffix: Option<&str>,
) {
    let party_label = party_long_label(party);
    for prefix in prefixes {
        for target in [pilot_name, unit_data_name, party_label] {
            if target.is_empty() {
                continue;
            }
            let label = match suffix {
                Some(s) => format!("{prefix} {target} {s}"),
                None => format!("{prefix} {target}"),
            };
            // ユニットイベント (会話 / 損傷 / 収納 等) は章ローカル。
            if app.post_stage_event_label(label) {
                return;
            }
        }
    }
}

/// SRC `損傷率 <unit> <pct>:` (`損傷率イベント.md`) の auto-fire。
///
/// `apply_damage` 等で HP が減少し、損傷率 (= damage * 100 / max_hp) が
/// 既存値から threshold 以上に跨いだ場合に発火する。`unit` 部は
/// メインパイロット名 / ユニット名 / 陣営名 のいずれにもマッチ。
/// 複数閾値を同時に跨いだ場合は全て発火する。
///
/// 仕様準拠ポイント:
/// - 破壊された場合は発火しない (本関数は `unit_instances` 内に target が
///   残存する前提で呼び出す)
/// - 同一ラベルが pilot/unit/party 経由で複数回マッチしても 1 度のみ発火
///
/// 仕様乖離ポイント:
/// - SRC.NET は `Attack`/`MapAttack` 等のシナリオ戦闘では発火させないが、
///   本実装は damage source を区別せず、HP が減れば一律発火する。シナリオ
///   作成時の用途 (HP 50% で台詞) を優先するための簡略化。
pub(crate) fn fire_damage_threshold_labels(
    app: &mut App,
    pilot_name: &str,
    unit_data_name: &str,
    party: crate::Party,
    old_dmg: i64,
    new_dmg: i64,
    max_hp: i64,
) {
    if max_hp <= 0 || new_dmg <= old_dmg {
        return;
    }
    let party_label = party_long_label(party);
    let identifiers: Vec<&str> = [pilot_name, unit_data_name, party_label]
        .into_iter()
        .filter(|s| !s.is_empty())
        .collect();
    let candidates: Vec<(String, i64)> = app
        .script_library()
        .labels
        .keys()
        .filter_map(|k| {
            let suffix = k.strip_prefix("損傷率 ")?;
            // suffix は "<name> <pct>" 形式。pct は末尾トークン。
            let last_space = suffix.rfind(' ')?;
            let name = suffix[..last_space].trim();
            let pct_str = suffix[last_space + 1..].trim();
            let pct: i64 = pct_str.parse().ok()?;
            if !identifiers.contains(&name) {
                return None;
            }
            Some((k.clone(), pct))
        })
        .collect();
    // 重複 (同じ label が pilot / unit_data の両方でマッチした等) は
    // HashMap キー同一なので filter_map 段で既に de-dup 済み。
    // 閾値順 (小さい順) に発火する: 50→30→10 と複数跨いだ場合の発生順を SRC.NET
    // と揃える。
    let mut hit: Vec<(String, i64)> = candidates
        .into_iter()
        .filter(|(_, pct)| {
            let scaled = pct.saturating_mul(max_hp);
            let old_scaled = old_dmg.saturating_mul(100);
            let new_scaled = new_dmg.saturating_mul(100);
            old_scaled < scaled && new_scaled >= scaled
        })
        .collect();
    hit.sort_by_key(|(_, pct)| *pct);
    for (label, _) in hit {
        // 損傷率 イベントは章ローカル。
        app.post_stage_event_label(label);
    }
}
