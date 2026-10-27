//! スクリプトコマンドからのユニットへの直接作用 (ダメージ・回復)。

use super::*;

/// `RecoverHP unit rate` の本処理。最大 HP の `rate`% を回復する (`None` は完全回復)。
pub(super) fn recover_hp(app: &mut App, key: &str, rate: Option<f64>) {
    let idx = match app
        .database()
        .unit_instances
        .iter()
        .position(|u| matches_unit_handle(u, key))
    {
        Some(i) => i,
        None => return,
    };
    let max_hp = app
        .database()
        .effective_max_hp(&app.database().unit_instances[idx]);
    let cur_dmg = app.database().unit_instances[idx].damage;
    // RecoverHP によって HP が 0 以下になることはない (最低値 = 1 → damage = max_hp - 1)。
    let new_dmg = match rate {
        None => 0,
        Some(pct) => {
            let recover = ((max_hp as f64) * pct / 100.0).round() as i64;
            // 回復後ダメージ: 下限 0 (HP = max_hp)、上限 max_hp - 1 (HP = 1)
            (cur_dmg - recover).max(0).min(max_hp - 1)
        }
    };
    app.database_mut().unit_instances[idx].damage = new_dmg;
}

/// `RecoverEN unit rate` の本処理。最大 EN の `rate`% を回復する (`None` は完全回復)。
pub(super) fn recover_en(app: &mut App, key: &str, rate: Option<f64>) {
    let idx = match app
        .database()
        .unit_instances
        .iter()
        .position(|u| matches_unit_handle(u, key))
    {
        Some(i) => i,
        None => return,
    };
    let max_en = app
        .database()
        .effective_max_en(&app.database().unit_instances[idx]);
    let cur_consumed = app.database().unit_instances[idx].en_consumed;
    // EN の下限は 0。
    let new_consumed = match rate {
        None => 0,
        Some(pct) => {
            let recover = ((max_en as f64) * pct / 100.0).round() as i32;
            (cur_consumed - recover).max(0)
        }
    };
    app.database_mut().unit_instances[idx].en_consumed = new_consumed;
}

/// 直接ダメージ / 回復を反映。HP <= 0 ならインスタンスを削除。
pub(super) fn apply_damage(app: &mut App, key: &str, amount: i64) {
    let Some(idx) = app
        .database()
        .unit_instances
        .iter()
        .position(|u| matches_unit_handle(u, key))
    else {
        return;
    };
    let max_hp = app
        .database()
        .effective_max_hp(&app.database().unit_instances[idx]);
    let old_dmg = app.database().unit_instances[idx].damage;
    let u = &mut app.database_mut().unit_instances[idx];
    u.damage = (u.damage + amount).max(0);
    let new_dmg = u.damage;
    if new_dmg >= max_hp && max_hp > 0 {
        // 撃破前にラベル発火用の名前/陣営を退避してから unit_instance を除去。
        let pilot_name = u.pilot_name.clone();
        let unit_data_name = u.unit_data_name.clone();
        let party = u.party;
        app.database_mut().remove_unit_at(idx);
        fire_destruction_labels(app, &pilot_name, &unit_data_name, party);
    } else if new_dmg > old_dmg {
        // HP 残存 + ダメージ増加 → 損傷率閾値跨ぎを検査。
        let u = &app.database().unit_instances[idx];
        let pilot_name = u.pilot_name.clone();
        let unit_data_name = u.unit_data_name.clone();
        let party = u.party;
        fire_damage_threshold_labels(
            app,
            &pilot_name,
            &unit_data_name,
            party,
            old_dmg,
            new_dmg,
            max_hp,
        );
    }
}
