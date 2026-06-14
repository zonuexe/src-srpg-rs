//! VFS ファイル操作コマンドと RenameTerm のテスト。
//! CreateFolder / RemoveFolder / RemoveFile / RenameFile / CopyFile /
//! RenameTerm + Term()

use src_core::data::event;
use src_core::event_runtime;
use src_core::App;

fn run(src: &str) -> App {
    let mut app = App::new();
    let stmts = event::parse(src).expect("parse");
    event_runtime::execute(&mut app, &stmts).expect("execute");
    app
}

// ============================================================
//  RenameTerm / Term()
// ============================================================

#[test]
fn rename_term_changes_term_name() {
    let app =
        run("RenameTerm スペシャルパワー ヒーローアクション\nSet t $(Term(スペシャルパワー))\n");
    assert_eq!(app.script_var("t"), "ヒーローアクション");
}

#[test]
fn rename_term_unknown_term_returns_itself() {
    let app = run("Set t $(Term(ＨＰ))\n");
    assert_eq!(app.script_var("t"), "ＨＰ");
}

#[test]
fn rename_term_overwrite() {
    let app = run("RenameTerm ＨＰ ライフ\nRenameTerm ＨＰ 生命力\nSet t $(Term(ＨＰ))\n");
    assert_eq!(app.script_var("t"), "生命力");
}

#[test]
fn rename_term_stores_in_script_var_namespace() {
    // 内部実装として __term_<用語> に保存されることを確認
    let app = run("RenameTerm アビリティ 必殺技\n");
    assert_eq!(app.script_var("__term_アビリティ"), "必殺技");
}

// ============================================================
//  RemoveFile / RenameFile / CopyFile
// ============================================================

#[test]
fn remove_file_deletes_virtual_file() {
    // ファイルを作ってから削除
    let app = run(r#"
Open log.txt For Output As #h
Print #h, hello
Close #h
RemoveFile log.txt
"#);
    assert!(app.virtual_file_lines("log.txt").is_none());
}

#[test]
fn rename_file_moves_content() {
    let app = run(r#"
Open old.txt For Output As #h
Print #h, line1
Close #h
RenameFile old.txt new.txt
"#);
    assert!(app.virtual_file_lines("old.txt").is_none());
    let lines = app
        .virtual_file_lines("new.txt")
        .expect("new.txt not found");
    assert_eq!(lines, ["line1"]);
}

#[test]
fn copy_file_duplicates_content() {
    let app = run(r#"
Open src.txt For Output As #h
Print #h, content
Close #h
CopyFile src.txt dst.txt
"#);
    let src = app.virtual_file_lines("src.txt").expect("src.txt");
    let dst = app.virtual_file_lines("dst.txt").expect("dst.txt");
    assert_eq!(src, dst);
    assert_eq!(dst, ["content"]);
}

#[test]
fn remove_file_nonexistent_is_noop() {
    // 存在しないファイルの削除はエラーにならない
    let app = run("RemoveFile notexist.txt\n");
    let _ = app; // no panic
}

#[test]
fn rename_file_nonexistent_is_noop() {
    let app = run("RenameFile notexist.txt other.txt\n");
    assert!(app.virtual_file_lines("other.txt").is_none());
}

// ============================================================
//  RemoveFolder
// ============================================================

#[test]
fn remove_folder_deletes_all_files_under_prefix() {
    let app = run(r#"
Open logs/a.txt For Output As #h
Print #h, aaa
Close #h
Open logs/b.txt For Output As #h2
Print #h2, bbb
Close #h2
Open other.txt For Output As #h3
Print #h3, ccc
Close #h3
RemoveFolder logs
"#);
    assert!(
        app.virtual_file_lines("logs/a.txt").is_none(),
        "logs/a.txt should be removed"
    );
    assert!(
        app.virtual_file_lines("logs/b.txt").is_none(),
        "logs/b.txt should be removed"
    );
    // other.txt は残る
    assert!(
        app.virtual_file_lines("other.txt").is_some(),
        "other.txt should remain"
    );
}

// ============================================================
//  CreateFolder
// ============================================================

#[test]
fn create_folder_is_accepted_without_error() {
    // CreateFolder は VFS にフォルダエントリを作るだけ。パニックしないこと。
    let app = run("CreateFolder logs\n");
    let _ = app;
}

// ============================================================
//  Open / Print / LineRead / Close 往復 (ファイル I/O)
// ============================================================

#[test]
fn write_then_lineread_round_trip() {
    // 出力モードで 2 行書き込み → 入力モードで LineRead して読み戻す。
    let app = run(r#"
Open data.txt For Output As F
Print F, alpha
Print F, beta
Close F
Open data.txt For Input As F
LineRead F a
LineRead F b
Close F
"#);
    assert_eq!(app.script_var("a"), "alpha");
    assert_eq!(app.script_var("b"), "beta");
}

#[test]
fn lineread_past_eof_returns_empty() {
    // 行数を超えて LineRead すると空文字を返す (vfs_read_line None → 既定値)。
    let app = run(r#"
Open one.txt For Output As F
Print F, only
Close F
Open one.txt For Input As F
LineRead F a
LineRead F b
Close F
"#);
    assert_eq!(app.script_var("a"), "only");
    assert_eq!(app.script_var("b"), "", "EOF 超過は空文字");
}

#[test]
fn output_mode_truncates_existing_content() {
    // 出力モードで開き直すと既存内容は切り詰められる。
    let app = run(r#"
Open log.txt For Output As F
Print F, old
Close F
Open log.txt For Output As F
Print F, new
Close F
Open log.txt For Input As F
LineRead F a
LineRead F b
Close F
"#);
    assert_eq!(app.script_var("a"), "new", "出力モードで切り詰め");
    assert_eq!(app.script_var("b"), "", "旧内容は消えている");
}

#[test]
fn append_mode_preserves_existing_content() {
    // 追加モードは既存内容を残して末尾に追記する。
    let app = run(r#"
Open log.txt For Output As F
Print F, first
Close F
Open log.txt For 追加 As F
Print F, second
Close F
Open log.txt For Input As F
LineRead F a
LineRead F b
Close F
"#);
    assert_eq!(app.script_var("a"), "first");
    assert_eq!(app.script_var("b"), "second");
}
