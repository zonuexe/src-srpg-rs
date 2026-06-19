//! 汎用戦闘アニメ Lib (GBA クローズアップ) の解決パイプライン end-to-end テスト。
//!
//! 著作権配慮: 本テストは `crates/src-web/tests/fixtures/スパロボ戦記/` に既存配置
//! されているシナリオを **参照** するのみで、オリジナルの文章/コードを embed しない。
//!
//! 目的: 実 fixture の `data/.../animation.txt`（武器→演出サブルーチンの対応表）と
//! `lib/BattleAnime*.eve`（汎用戦闘アニメ Lib 本体）をロードし、
//! `AnimationData::resolve_weapon` が返すサブルーチン名が **実際に Lib に存在する**
//! ことを突合する。これで「武器使用 → animation.txt 解決 → 戦闘アニメ Lib ラベル」の
//! 一連の配線が実データで成立することを保証する（GBA Phase 1/2 の到達点検証）。

use std::fs;
use std::path::{Path, PathBuf};

use src_core::data::animation::{AnimationData, WeaponPhase};
use src_core::data::{event, loader};
use src_core::event_runtime;
use src_core::App;

fn scenario_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("src-web/tests/fixtures/スパロボ戦記")
}

/// `lib/BattleAnime*.eve` を全て script_library へ登録する。
fn load_battle_anim_lib(app: &mut App, lib_dir: &Path) -> usize {
    let mut loaded = 0;
    let Ok(entries) = fs::read_dir(lib_dir) else {
        return 0;
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.starts_with("BattleAnime") && s.ends_with(".eve"))
                .unwrap_or(false)
        })
        .collect();
    paths.sort();
    for p in paths {
        let Ok(bytes) = fs::read(&p) else { continue };
        let txt = loader::decode_text(&bytes);
        if let Ok(stmts) = event::parse(&txt) {
            event_runtime::library_append(app, &stmts);
            loaded += 1;
        }
    }
    loaded
}

#[test]
fn battle_anim_resolution_matches_real_lib_labels() {
    let root = scenario_root();
    if !root.exists() {
        eprintln!("[skip] スパロボ戦記 fixture が無い: {}", root.display());
        return;
    }

    // 1) 汎用戦闘アニメ Lib (BattleAnime*.eve) を library に登録。
    let mut app = App::new();
    let files = load_battle_anim_lib(&mut app, &root.join("lib"));
    assert!(
        files >= 5,
        "BattleAnime{{,G,O,R,S}}.eve が揃うはず: {files}"
    );

    // 2) animation.txt をロード。
    let anim_txt = root.join("data/スパロボ戦記/animation.txt");
    let bytes = fs::read(&anim_txt).expect("animation.txt が読めない");
    let mut anim = AnimationData::default();
    anim.merge_from_str(&loader::decode_text(&bytes));
    assert!(!anim.is_empty(), "animation.txt がパースできていない");

    // 3) 「汎用」バケットの代表的な武器を解決し、得たサブルーチンが Lib に実在する
    //    ことを突合する。`光子力ビーム` の演出は
    //      ブラックイン;拡大小ビーム照射 Yellow …;ブラックアウト
    //    で、攻撃フェーズでは 戦闘アニメ_<sub>攻撃 の 3 ラベルへ解決される。
    //    (unit/nickname/class を空にすると探索が「汎用」へフォールバックする。)
    let resolved = anim.resolve_weapon("", "", "", "光子力ビーム", "攻撃", WeaponPhase::Attack);
    assert!(
        !resolved.is_empty(),
        "光子力ビーム の戦闘アニメが解決できない (汎用バケット未到達?)"
    );

    let lib = app.script_library();
    for r in &resolved {
        assert!(
            lib.label_pc(&r.subroutine).is_some(),
            "解決サブルーチン `{}` が BattleAnime Lib に実在しない (解決名と Lib ラベルの不整合)",
            r.subroutine
        );
    }

    // 期待: ブラックイン / 拡大小ビーム照射 / ブラックアウト の攻撃フェーズ版。
    let names: Vec<&str> = resolved.iter().map(|r| r.subroutine.as_str()).collect();
    assert!(
        names.contains(&"戦闘アニメ_拡大小ビーム照射攻撃"),
        "解決名に 拡大小ビーム照射攻撃 が含まれるはず: {names:?}"
    );
}

#[test]
fn gba_closeup_is_gated_on_zenshin_setting() {
    // GBA クローズアップ（全身戦闘アニメ）は `設定[全身戦闘アニメ] = オン` で分岐する。
    // 未設定だと 2D 経路（武器個別サブルーチン `戦闘アニメ_ビームライフル準備` 等を Call）
    // を通り、それらは BattleAnime Lib 外なので「Goto 先未検出」で停止する＝
    // クローズアップ経路に入っていない証拠。設定を ON にすると 2D 経路の早期 return /
    // 武器個別 Call を踏まずクローズアップ本体（描画 primitives）へ進む。
    // → 描画 primitive の網羅性ではなく「GBA 分岐がどの変数で開くか」を実データで固定する。
    let root = scenario_root();
    if !root.exists() {
        eprintln!("[skip] スパロボ戦記 fixture が無い: {}", root.display());
        return;
    }

    // 全身戦闘アニメ OFF: 2D 経路へ入り、Lib 外の武器個別ラベルへ Goto して停止する。
    let mut app_off = App::new();
    assert!(load_battle_anim_lib(&mut app_off, &root.join("lib")) >= 5);
    let stmts = event::parse("Call 戦闘アニメ_拡大小ビーム照射攻撃\n").unwrap();
    let pc = event_runtime::library_append(&mut app_off, &stmts);
    let off = event_runtime::run_from_pc(&mut app_off, pc);
    let off_err = off.err().map(|e| e.message).unwrap_or_default();
    assert!(
        off_err.contains("戦闘アニメ_ビームライフル準備"),
        "全身 OFF では 2D 経路の武器個別ラベルへ Goto するはず: {off_err:?}"
    );

    // 全身戦闘アニメ ON: クローズアップ本体（照射ビーム攻撃変数設定/戦闘アニメ背景描写 等の
    // ヘルパは BattleAnime.eve に在る）へ入り、未対応命令/欠落ラベルで ScriptError を出さず
    // **最初の Wait まで完走**する（run_from_pc は Wait 中断時 Ok を返す）。対象ユニット未配置でも
    // Info(…) は空/0 を返し、VFS file I/O (Open/Print/Close/Load 特殊処理) も成立する。
    let mut app_on = App::new();
    assert!(load_battle_anim_lib(&mut app_on, &root.join("lib")) >= 5);
    app_on.set_script_var("設定[全身戦闘アニメ]".to_string(), "オン".to_string());
    let stmts = event::parse("Call 戦闘アニメ_拡大小ビーム照射攻撃\n").unwrap();
    let pc = event_runtime::library_append(&mut app_on, &stmts);
    let on = event_runtime::run_from_pc(&mut app_on, pc);
    assert!(
        on.is_ok(),
        "全身 ON ではクローズアップ本体が未対応命令/欠落ラベルなく Wait まで完走するはず: {:?}",
        on.err()
    );
    // クローズアップ本体に入った証拠: 描画コマンドが overlay に積まれ、Wait で中断している。
    assert!(
        app_on.pending_timer().is_some() || !app_on.script_overlay().cmds.is_empty(),
        "クローズアップ本体が描画 or Wait まで到達しているはず"
    );
}
