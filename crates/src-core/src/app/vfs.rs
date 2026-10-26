//! `.eve` の `Open` / `Print` / `Read` / `Close` 等が使う仮想ファイルシステム。

use super::*;

impl App {
    /// パスを正規化する (`\` → `/`、小文字化、クォート / 前後空白除去)。
    pub(super) fn vfs_normalize(path: &str) -> String {
        path.trim()
            .trim_matches('"')
            .replace('\\', "/")
            .to_lowercase()
    }

    /// ファイルを開いてハンドル文字列を返す。
    /// `mode`: `入力`/`Input` = 読込、`出力`/`Output` = 書込 (切詰)、
    /// `追加`/`追加出力`/`Append` = 追記。
    pub fn vfs_open(&mut self, path: &str, mode: &str) -> String {
        let norm = Self::vfs_normalize(path);
        let m = mode.trim().trim_matches('"');
        let write = !matches!(m, "入力" | "Input" | "input");
        let append = matches!(m, "追加" | "追加出力" | "Append" | "append");
        if write && !append {
            self.virtual_files.insert(norm.clone(), Vec::new());
        } else {
            self.virtual_files.entry(norm.clone()).or_default();
        }
        let handle = format!("__fh{}", self.next_file_handle);
        self.next_file_handle = self.next_file_handle.wrapping_add(1);
        self.open_files.insert(
            handle.clone(),
            OpenFileHandle {
                path: norm,
                write,
                read_cursor: 0,
            },
        );
        handle
    }

    /// `s` が開いているファイルハンドルか。
    pub fn vfs_is_handle(&self, s: &str) -> bool {
        self.open_files.contains_key(s)
    }

    /// 開いているファイルに 1 行追記する。
    pub fn vfs_print(&mut self, handle: &str, line: String) {
        if let Some(h) = self.open_files.get(handle) {
            let path = h.path.clone();
            self.virtual_files.entry(path).or_default().push(line);
        }
    }

    /// 開いているファイルから次の 1 行を読む。末尾 / 不正ハンドルなら `None`。
    pub fn vfs_read_line(&mut self, handle: &str) -> Option<String> {
        let (path, cursor) = {
            let h = self.open_files.get(handle)?;
            (h.path.clone(), h.read_cursor)
        };
        let line = self.virtual_files.get(&path)?.get(cursor).cloned();
        if line.is_some() {
            if let Some(h) = self.open_files.get_mut(handle) {
                h.read_cursor += 1;
            }
        }
        line
    }

    /// ファイルを閉じる。書き込みモードで閉じたファイルが pilot/unit 等の
    /// データファイルなら内容を再パースして GameDatabase に加法マージする
    /// (キャラメイキングが書き出したパイロットを使用可能にする)。
    pub fn vfs_close(&mut self, handle: &str) {
        let Some(h) = self.open_files.remove(handle) else {
            return;
        };
        if h.write {
            self.vfs_reload_data_file(&h.path);
        }
    }

    /// 仮想ファイルがデータファイル (`pilot.txt` 等) ならパースして DB に
    /// 加法マージする。パース失敗は黙って無視する。
    pub(super) fn vfs_reload_data_file(&mut self, path: &str) {
        let basename = path.rsplit('/').next().unwrap_or(path);
        let Some(lines) = self.virtual_files.get(path) else {
            return;
        };
        let text = lines.join("\n");
        match basename {
            "pilot.txt" => {
                // 壊れた 1 レコードがあっても残りのパイロットは取り込む。
                let (pilots, _errors) = crate::data::pilot::parse_lenient(&text);
                self.database.extend_pilots(pilots);
            }
            "unit.txt" | "robot.txt" => {
                // 壊れた 1 レコードがあっても残りのユニットは取り込む。
                let (units, _errors) = crate::data::unit::parse_lenient(&text);
                self.database.extend_units(units);
            }
            "item.txt" => {
                let (items, _errors) = crate::data::item::parse_lenient(&text);
                self.database.extend_items(items);
            }
            _ => {}
        }
    }

    /// 仮想ファイルの全行を返す (テスト / デバッグ用)。
    pub fn virtual_file_lines(&self, path: &str) -> Option<&[String]> {
        self.virtual_files
            .get(&Self::vfs_normalize(path))
            .map(Vec::as_slice)
    }

    /// 仮想ファイルが存在すれば basename を返す。`Dir(path, …)` 用。
    pub fn virtual_file_basename_if_exists(&self, path: &str) -> Option<String> {
        let norm = Self::vfs_normalize(path);
        if self.virtual_files.contains_key(&norm) {
            Some(norm.rsplit('/').next().unwrap_or(&norm).to_string())
        } else {
            None
        }
    }

    /// `FileExists(path)` — VFS に該当ファイルがあるなら true。
    pub fn virtual_file_exists(&self, path: &str) -> bool {
        let norm = Self::vfs_normalize(path);
        self.virtual_files.contains_key(&norm)
    }

    /// `FileLen(path)` — VFS 上のファイルサイズ (バイト数 = 行内 UTF-8 バイト
    /// + 改行 1 バイト/行)。未登録なら 0。
    pub fn virtual_file_len(&self, path: &str) -> u64 {
        let norm = Self::vfs_normalize(path);
        match self.virtual_files.get(&norm) {
            Some(lines) => lines.iter().map(|l| l.len() as u64 + 1).sum::<u64>(),
            None => 0,
        }
    }

    /// `Loc(handle)` — 開いているファイルの読み取りカーソル (行番号、0 起点)。
    /// 不正ハンドルなら 0。
    pub fn vfs_loc(&self, handle: &str) -> u64 {
        self.open_files
            .get(handle)
            .map(|h| h.read_cursor as u64)
            .unwrap_or(0)
    }

    /// `EOF(handle)` — 末尾 (= 読取可能な行が無い) なら true。
    pub fn vfs_eof(&self, handle: &str) -> bool {
        let Some(h) = self.open_files.get(handle) else {
            return true;
        };
        let Some(lines) = self.virtual_files.get(&h.path) else {
            return true;
        };
        h.read_cursor >= lines.len()
    }

    /// `LOF(handle)` — 開いているファイルの総行数。不正ハンドルなら 0。
    pub fn vfs_lof(&self, handle: &str) -> u64 {
        let Some(h) = self.open_files.get(handle) else {
            return 0;
        };
        self.virtual_files
            .get(&h.path)
            .map(|lines| lines.len() as u64)
            .unwrap_or(0)
    }

    /// `CreateFolder` — VFS に空フォルダエントリを作る。
    /// 実際の BTreeMap には `path/` キーで空 Vec を挿入する。
    pub fn vfs_ensure_folder(&mut self, norm_path: &str) {
        self.virtual_files.entry(norm_path.to_string()).or_default();
    }

    /// `RemoveFolder` — 指定パス以下のファイルを全削除する。
    pub fn vfs_remove_folder(&mut self, prefix: &str) {
        let prefix_slash = if prefix.ends_with('/') {
            prefix.to_string()
        } else {
            format!("{prefix}/")
        };
        self.virtual_files
            .retain(|k, _| !k.starts_with(&prefix_slash) && k != prefix);
    }

    /// `RemoveFile` — 仮想ファイルを削除する。
    pub fn vfs_remove_file(&mut self, path: &str) {
        let norm = Self::vfs_normalize(path);
        self.virtual_files.remove(&norm);
    }

    /// `RenameFile` — 仮想ファイルのパスを変更する。
    pub fn vfs_rename_file(&mut self, old_path: &str, new_path: &str) {
        let old_norm = Self::vfs_normalize(old_path);
        let new_norm = Self::vfs_normalize(new_path);
        if let Some(content) = self.virtual_files.remove(&old_norm) {
            self.virtual_files.insert(new_norm, content);
        }
    }

    /// `CopyFile` — 仮想ファイルをコピーする。
    pub fn vfs_copy_file(&mut self, src_path: &str, dst_path: &str) {
        let src_norm = Self::vfs_normalize(src_path);
        let dst_norm = Self::vfs_normalize(dst_path);
        if let Some(content) = self.virtual_files.get(&src_norm).cloned() {
            self.virtual_files.insert(dst_norm, content);
        }
    }
}
