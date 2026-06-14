//! mini_campaign の追加 assert / Focused asserts for the 3-chapter campaign.
//!
//! `tests/fixtures/scenarios/14_mini_campaign.eve` を Harness で走らせ、
//! snapshot 比較とは別に「3 話分のラベルを順に通過した」「ボスが撃破され
//! た」「GameClear に到達した」など、シナリオ進行上の不変量を明示的に
//! 検証する。snapshot は表示揺れに弱いので、ここで意味的なゴールを別途
//! 保証する。

use std::fs;
use std::path::{Path, PathBuf};

use src_core::stage::StageState;
use src_core::test_harness::{DriveOutcome, Harness, Step};

fn fixture_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/scenarios/14_mini_campaign.eve")
}

#[test]
fn mini_campaign_reaches_ending_with_game_clear() {
    let src = fs::read_to_string(fixture_path()).expect("read mini_campaign.eve");
    let mut h = Harness::from_eve_source(&src).expect("parse + initial execute");

    let outcome = h
        .drive(&[Step::Drain(500)])
        .expect("drain through all Talk / Wait");

    assert_eq!(
        outcome,
        DriveOutcome::Finished,
        "campaign did not complete (modal still pending?)"
    );

    let app = h.app();

    // 最終 Stage がエンディング、状態は Victory
    assert_eq!(app.stage(), "エンディング");
    assert_eq!(app.stage_state(), StageState::Victory);

    // 各話で 1 つ以上の talk message が積まれている (3 章分の固有台詞)
    let msgs: Vec<&str> = app.messages().iter().map(String::as_str).collect();
    let joined = msgs.join("\n");
    assert!(
        joined.contains("気合を入れろ"),
        "第1話ノヴァの台詞が見当たらない"
    );
    assert!(
        joined.contains("ゾルダとは違うのだよ"),
        "第2話ランバ・ラルの台詞が見当たらない"
    );
    assert!(
        joined.contains("認めたくないものだな"),
        "第3話ガロの台詞が見当たらない"
    );
    assert!(
        joined.contains("--- THE END ---"),
        "Telop エンディングが見当たらない"
    );
    assert!(
        joined.contains("【勝利】"),
        "GameClear に伴う勝利テロップが無い"
    );

    // 敵ユニットは全滅 (Player 2 機のみ生存)
    let units = &app.database().unit_instances;
    let alive_player = units
        .iter()
        .filter(|u| matches!(u.party, src_core::Party::Player) && !u.off_map)
        .count();
    let alive_enemy = units
        .iter()
        .filter(|u| matches!(u.party, src_core::Party::Enemy) && !u.off_map)
        .count();
    assert_eq!(alive_player, 2, "Player 機 2 機が生存しているはず");
    assert_eq!(alive_enemy, 0, "敵ユニットが残っている");
}
