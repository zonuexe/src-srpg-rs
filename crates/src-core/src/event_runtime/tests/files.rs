//! `.eve` の仮想ファイル入出力 (Open / Print / Require / Dir) のテスト。

use super::*;

#[test]
fn virtual_file_write_then_read_roundtrip() {
    // `Open`/`Print`/`Close` で書いた仮想ファイルを `入力` で開き直して
    // `Read` で 1 行ずつ読み戻せる。
    let app = run_script(
        "Open \"test.txt\" For 出力 As F\n\
             Print F ライン1\n\
             Print F ライン2\n\
             Close F\n\
             Open \"test.txt\" For 入力 As G\n\
             Read G 行A\n\
             Read G 行B\n\
             Read G 行C\n\
             Close G\n",
    );
    assert_eq!(app.script_var("行A"), "ライン1");
    assert_eq!(app.script_var("行B"), "ライン2");
    // 末尾を超えた Read は空文字。
    assert_eq!(app.script_var("行C"), "");
}

#[test]
fn virtual_file_output_mode_truncates() {
    // `出力` で開き直すと旧内容は切り詰められる。
    let app = run_script(
        "Open \"x.txt\" For 出力 As F\nPrint F 古い\nClose F\n\
             Open \"x.txt\" For 出力 As F\nPrint F 新しい\nClose F\n\
             Open \"x.txt\" For 入力 As G\nRead G 行\nClose G\n",
    );
    assert_eq!(app.script_var("行"), "新しい");
    assert_eq!(
        app.virtual_file_lines("x.txt"),
        Some(&["新しい".to_string()][..])
    );
}

#[test]
fn virtual_pilot_file_close_registers_pilot_data() {
    // `pilot.txt` を仮想ファイルに書き出して `Close` すると、内容が
    // 再パースされ PilotData として GameDatabase に登録される
    // (キャラメイキングが書き出すキャラを使用可能にする経路)。
    let app = run_script(
        "Open \"data\\一時フォルダ\\pilot.txt\" For 出力 As F\n\
             Print F\n\
             Print F\n\
             Print F テストキャラ\n\
             Print F テス, -, 汎用, AAAA, 100\n\
             Print F 特殊能力\n\
             Print F 150, 150, 150, 150, 150, 150\n\
             Close F\n",
    );
    assert!(
        app.database().pilot_by_name("テストキャラ").is_some(),
        "Close 時に pilot.txt が再パースされ PilotData が登録される"
    );
}

#[test]
fn dir_reflects_virtual_filesystem() {
    // 仮想ファイルが無ければ空文字、書き出した後は basename を返す。
    let app = run_script(
        "Set d1 Dir(\"data\\foo.txt\", ファイル)\n\
             Open \"data\\foo.txt\" For 出力 As F\nPrint F x\nClose F\n\
             Set d2 Dir(\"data\\foo.txt\", ファイル)\n",
    );
    assert_eq!(app.script_var("d1"), "", "未作成ファイルは空文字");
    assert_eq!(app.script_var("d2"), "foo.txt", "作成後は basename");
}

#[test]
fn require_applies_ini_key_value_assignments() {
    // `Require <path>` は script_library に登録済みの設定ファイルの
    // `key = value` 行をスクリプト変数として適用する。
    let ini = "\
# テーマ設定\n\
LetterColor1 = #ffffff\n\
LetterShadow = #000000\n\
FrameColor1 = #505098\n";
    let ini_stmts = event::parse(ini).unwrap();
    let mut app = App::new();
    app.script_library_mut()
        .append_with_name(&ini_stmts, "Lib\\Alpha2ndStatus.ini");
    let stmts = event::parse("Require Lib\\Alpha2ndStatus.ini\n").unwrap();
    execute(&mut app, &stmts).unwrap();
    assert_eq!(app.script_var("LetterColor1"), "#ffffff");
    assert_eq!(app.script_var("LetterShadow"), "#000000");
    assert_eq!(app.script_var("FrameColor1"), "#505098");
}

/// `Print F msg;` は改行せず、次の Print が同じ行に続く (原典 ExecPrintCmd)。
/// 実データの戦闘アニメ定義 (BattleAnime.eve) が 1 行を 3 回の Print で組み立てる。
#[test]
fn print_semicolon_continues_the_same_line() {
    let src = "\
Set w ビーム
Open \"out.txt\" For 出力 As F
Print F \"$(w)(回避),\";
Print F `$(Call(回避戦績処理))`;
Print F \"ブラックイン\"
Close F
";
    let mut app = App::new();
    execute(&mut app, &event::parse(src).unwrap()).unwrap();
    assert_eq!(
        app.virtual_file_lines("out.txt").unwrap(),
        ["ビーム(回避),$(Call(回避戦績処理))ブラックイン"]
    );
}
