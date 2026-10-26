//! 開発用デバッグ補助 (ブラウザコンソールの `__srcDebug` 等から呼ばれる)。

use super::*;

impl App {
    /// 検証/デバッグ専用: オンマップ各ユニットの武器の発射可否を要約する (verify-archive が
    /// 「交戦が成立しない」原因 — 武器なし／必要技能 NG／必要気力不足／EN・残弾切れ — を切り分ける)。
    pub fn debug_firable_report(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (idx, u) in self.database.unit_instances.iter().enumerate() {
            if u.off_map {
                continue;
            }
            let weapons = self
                .database
                .unit_by_name(&u.unit_data_name)
                .map(|d| d.weapons.clone())
                .unwrap_or_default();
            let parts: Vec<String> = weapons
                .iter()
                .map(|w| {
                    let mark = if self.weapon_firable(idx, w) {
                        "✓"
                    } else {
                        "✗"
                    };
                    format!(
                        "{mark}{}(r{} 気{} EN{} 技{:?})",
                        w.name,
                        w.max_range,
                        w.necessary_morale,
                        w.en_consumption,
                        w.necessary_skill()
                    )
                })
                .collect();
            let max_en = self.database.effective_max_en(u);
            let pilot_ok = self.database.pilot_by_name(&u.pilot_name).is_some();
            let combat_ok = self.database.effective_combat_data(idx).is_some();
            out.push(format!(
                "{}[{:?}]@({},{}) pilot={:?}{} combat_data={} 気力{} EN{}/{}: {}",
                u.unit_data_name,
                u.party,
                u.x,
                u.y,
                u.pilot_name,
                if pilot_ok { "(✓DB)" } else { "(✗DB欠)" },
                if combat_ok { "✓" } else { "✗None" },
                u.morale,
                max_en.saturating_sub(u.en_consumed),
                max_en,
                if parts.is_empty() {
                    "(武器なし)".to_string()
                } else {
                    parts.join(" ")
                }
            ));
        }
        out
    }

    /// 検証/デバッグ専用: パイロット不在 (`pilot_name` が DB に無い) の味方機に DB の
    /// 任意パイロットを乗せ、`effective_combat_data` を解決可能にする。キャラメイキングの
    /// 出撃導線 (exit/搭乗) をヘッドレスで完走できないシナリオ (D スパロボ戦記) で、
    /// 「piloted な味方が実マップで戦闘成立するか」を検証するための affordance。乗せた機数を返す。
    /// 通常プレイ経路からは呼ばれない。
    pub fn debug_seat_db_pilot(&mut self) -> usize {
        let Some(pilot) = self.database.pilots.first().map(|p| p.name.clone()) else {
            return 0;
        };
        let valid: std::collections::HashSet<String> = self
            .database
            .pilots
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let mut n = 0;
        for u in self.database.unit_instances.iter_mut() {
            if u.party == crate::Party::Player && !u.off_map && !valid.contains(&u.pilot_name) {
                u.pilot_name = pilot.clone();
                n += 1;
            }
        }
        n
    }

    /// 検証/デバッグ専用: 現在のフェイズ党派のユニットを AI ロジックで一括行動させる。
    /// `run_ai_phase` を党派非依存に再利用 (ターゲットは `is_hostile_to` で解決されるため、
    /// 味方フェイズに呼べば味方が敵へ前進・攻撃する)。verify-archive のヘッドレス drive が
    /// 「味方フェイズに味方を自動行動させて戦闘を勝敗まで通す」ために使う。通常プレイの
    /// 手動操作には一切影響しない (このメソッドはエンジン内部から呼ばれない)。
    /// Battle 中の MapView でのみ作用し、行動後に勝敗判定を回す。
    pub fn debug_run_phase_ai(&mut self) {
        if self.scene == Scene::MapView && self.stage_state == crate::stage::StageState::Battle {
            self.run_ai_phase();
            self.check_victory();
        }
    }

    /// デバッグ用: App の主要状態を 1 行 JSON 風文字列で返す。
    /// フロントエンドが `window.__srcDebug()` 経由で吸い出して
    /// シナリオ進行の詰まり箇所を診断する。
    pub fn debug_summary(&self) -> String {
        let dialog = match &self.pending_dialog {
            None => "none".to_string(),
            Some(crate::dialog::PendingDialog::Talk { speaker, .. }) => {
                format!("Talk({speaker})")
            }
            Some(crate::dialog::PendingDialog::WaitClick) => "WaitClick".to_string(),
            Some(crate::dialog::PendingDialog::Confirm { .. }) => "Confirm".to_string(),
            Some(crate::dialog::PendingDialog::Menu { options, .. }) => {
                format!("Menu({})", options.len())
            }
            Some(crate::dialog::PendingDialog::Input { .. }) => "Input".to_string(),
        };
        let hp_names: Vec<&str> = self
            .hotpoints
            .iter()
            .take(12)
            .map(|h| h.name.as_str())
            .collect();
        let pcount = |p: crate::Party| {
            self.database
                .unit_instances
                .iter()
                .filter(|u| u.party == p && !u.off_map)
                .count()
        };
        format!(
            "scene={:?} stage_state={:?} dialog={dialog} script_ctx={} \
             flow={:?} pending_timer={:?} hotpoints={}{:?} overlay_cmds={} \
             intermission_cmds={} 次ステージ={:?} units={} \
             parties=[味:{} 敵:{} 中:{} Ｎ:{}] file={:?} \
             victory[全滅敵={} 全滅中立={} クリア={}] messages={} \
             last_msg={:?} script_err={:?}",
            self.scene,
            self.stage_state,
            self.script_ctx.is_some(),
            self.flow,
            self.pending_timer,
            self.hotpoints.len(),
            hp_names,
            self.script_overlay.cmds.len(),
            self.intermission_commands.len(),
            self.script_var("次ステージ"),
            self.database.unit_instances.len(),
            pcount(crate::Party::Player),
            pcount(crate::Party::Enemy),
            pcount(crate::Party::Neutral),
            pcount(crate::Party::Npc),
            self.current_stage_file,
            self.stage_defines_label("全滅 敵"),
            self.stage_defines_label("全滅 中立"),
            self.stage_defines_label("クリア"),
            self.messages.len(),
            self.messages.last().map(String::as_str).unwrap_or(""),
            self.last_script_error,
        )
    }
}
