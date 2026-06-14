//! 実シナリオ `.eve` から command_catalog 未登録の命令を抽出する CLI。
//!
//! `crates/src-web/tests/fixtures/**/*.eve` を `data::event::parse` で正確に
//! パースし、各 statement の name を `command_catalog::lookup` に当て、
//! ヒットしないものを集計する。出現回数とサンプルファイルも出す。
//!
//! 使い方:
//!
//! ```sh
//! cargo run -p verify-archive --bin scan_eve
//! cargo run -p verify-archive --bin scan_eve -- <root>     # 任意ディレクトリ
//! ```

use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use src_core::command_catalog::lookup;
use src_core::data::{event, loader};

fn workspace_root() -> PathBuf {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    here.ancestors()
        .find(|p| p.join("Cargo.lock").exists())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| here.to_path_buf())
}

fn collect_eves(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_eves(&path, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("eve") {
            out.push(path);
        }
    }
}

/// ラベル名の抽出 (`name:` / `@name` / `*name:` の 3 形式)。コマンド命令と
/// 同じ first-token に並ぶので、event::parse 後の Command name から復元する。
fn canonical_label(name: &str) -> Option<String> {
    if let Some(rest) = name.strip_prefix('@') {
        if !rest.is_empty() {
            return Some(rest.trim_end_matches(':').to_string());
        }
    }
    if let Some(rest) = name.strip_prefix('*') {
        if let Some(stem) = rest.strip_suffix(':') {
            if !stem.is_empty() {
                return Some(stem.to_string());
            }
        }
    }
    if let Some(stem) = name.strip_suffix(':') {
        if !stem.is_empty() {
            return Some(stem.to_string());
        }
    }
    None
}

/// `first-token` が「実コマンド」候補かを大雑把にフィルタする。
///
/// 除外したいパターン:
/// - 末尾 `:` → ラベル (`onsen:` 等)
/// - `(` を含む → 関数呼出 (`HP(args(1))`)
/// - `[` を含む → 配列アクセス (`sort_mode[1]`)
/// - 末尾 `#` 系 → コメント残り (`Continue##############`)
/// - `*` 始まり / `@` 始まり → アンカー
fn is_command_like(name: &str) -> bool {
    if name.is_empty() {
        return false;
    }
    if name.ends_with(':') || name.starts_with('@') || name.starts_with('*') {
        return false;
    }
    if name.contains('(')
        || name.contains('[')
        || name.contains('#')
        || name.contains('\\')
        || name.contains('=')
    {
        return false;
    }
    // 台詞/歌詞/選択肢/説明文が誤ってコマンド名として拾われるのを除外する。
    // 実コマンド名は**空白も文章記号も含まない単一トークン**なので、それらを含む
    // 候補はテキスト断片とみなして落とす (頭文字Ｄ歌詞 "DON'T"、"Mr.タロウ"、
    // "BETA、横浜基地に来ます"、"Easy　シミュレーション..." 等)。
    if name.chars().any(|c| c.is_whitespace()) {
        return false;
    }
    const TEXT_PUNCT: &str = "、。！？；：「」『』（）【】・…ー〜～＜＞，．,!?;:'\"。．！？／＆｜";
    if name.chars().any(|c| c == '.' || TEXT_PUNCT.contains(c)) {
        return false;
    }
    // 先頭が ASCII alpha でないと識別子として扱わない (`123foo` 等を除外)。
    name.bytes()
        .next()
        .map(|b| b.is_ascii_alphabetic())
        .unwrap_or(false)
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let root = if args.len() >= 2 {
        PathBuf::from(&args[1])
    } else {
        workspace_root().join("crates/src-web/tests/fixtures")
    };
    if !root.exists() {
        eprintln!("[error] スキャン対象が見つかりません: {}", root.display());
        return ExitCode::FAILURE;
    }
    let mut eves = Vec::new();
    collect_eves(&root, &mut eves);
    eves.sort();
    println!("# 実シナリオの catalog 未登録命令スキャン\n");
    println!("- スキャン対象: {} ({} files)", root.display(), eves.len());
    println!();

    // 未登録命令: name → (出現回数, 出現ファイルサンプル)
    let mut missing: BTreeMap<String, (usize, PathBuf)> = BTreeMap::new();
    let mut total_stmts: usize = 0;
    let mut total_unique_commands: BTreeMap<String, usize> = BTreeMap::new();
    // catalog 未登録だが、どこかにラベル定義がある = ユーザー定義関数呼び出し。
    // 未実装コマンドと区別して除外件数を記録する (ノイズ削減の透明性確保)。
    let mut user_label_calls: BTreeMap<String, usize> = BTreeMap::new();
    let mut parse_errors: Vec<(PathBuf, String)> = Vec::new();
    // 全 .eve に登場するラベル集合 (**case-insensitive**)。
    // `wPaintString:` `@onsen` などをここに登録し、後段で「未登録 first-token
    // のうち実はラベル参照 (= ユーザー定義関数) だったもの」を分離する。
    //
    // SRC のコマンド/ラベル名は case-insensitive (`command_catalog::matches` も
    // `eq_ignore_ascii_case`)。ラベル `SamoTalk:` を `Samotalk` / `SAMOTALK` で
    // 呼ぶケースが実シナリオに多く、case-sensitive 照合では取りこぼして未登録
    // ノイズが膨らむため、ASCII 小文字に正規化して保持・照合する。
    let mut labels: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();

    // 1st pass: ラベル集めだけ
    for path in &eves {
        let Ok(bytes) = fs::read(path) else { continue };
        let text = loader::decode_text(&bytes);
        let Ok(stmts) = event::parse(&text) else {
            continue;
        };
        for s in &stmts {
            if let event::EventStatement::Command { name, .. } = s {
                if let Some(stem) = canonical_label(name) {
                    labels.insert(stem.to_ascii_lowercase());
                }
            }
        }
    }

    for path in &eves {
        let Ok(bytes) = fs::read(path) else { continue };
        let text = loader::decode_text(&bytes);
        let stmts = match event::parse(&text) {
            Ok(s) => s,
            Err(e) => {
                parse_errors.push((path.clone(), e.message.clone()));
                continue;
            }
        };
        for s in &stmts {
            total_stmts += 1;
            let (name, args) = match s {
                event::EventStatement::Command { name, args, .. } => {
                    (name.as_str(), args.as_slice())
                }
                event::EventStatement::Include { .. } => continue,
            };
            if !is_command_like(name) {
                continue;
            }
            // VB6 風代入文 `name = expr` は runtime で `Set name expr` に解釈
            // されるので unknown 扱いしない。
            if args.first().map(String::as_str) == Some("=") {
                continue;
            }
            *total_unique_commands.entry(name.to_string()).or_insert(0) += 1;
            if lookup(name).is_none() {
                if labels.contains(&name.to_ascii_lowercase()) {
                    // ユーザー定義ラベル (= 自作関数) への呼び出し。未実装コマンド
                    // ではないので未登録には数えず、除外件数だけ記録する。
                    *user_label_calls.entry(name.to_string()).or_insert(0) += 1;
                } else {
                    let entry = missing
                        .entry(name.to_string())
                        .or_insert_with(|| (0, path.clone()));
                    entry.0 += 1;
                }
            }
        }
    }

    if !parse_errors.is_empty() {
        println!("## パースエラー ({} files)\n", parse_errors.len());
        for (p, m) in parse_errors.iter().take(5) {
            println!(
                "  - {}: {}",
                p.strip_prefix(workspace_root()).unwrap_or(p).display(),
                m
            );
        }
        if parse_errors.len() > 5 {
            println!("  - ... ({} 件省略)", parse_errors.len() - 5);
        }
        println!();
    }

    let user_label_call_total: usize = user_label_calls.values().sum();
    println!("## 集計\n");
    println!("- 総 command statement 数: {}", total_stmts);
    println!("- ユニーク command 名: {}", total_unique_commands.len());
    println!(
        "- ユーザー定義関数呼び出しとして除外: {} 種 ({} 回) — ラベル定義あり",
        user_label_calls.len(),
        user_label_call_total
    );
    println!("- うち catalog 未登録 (真の未実装候補): {}", missing.len());
    println!();

    if missing.is_empty() {
        println!("✅ 全 command が catalog に登録されています。");
        return ExitCode::SUCCESS;
    }

    println!("## catalog 未登録 (出現順)\n");
    let mut rows: Vec<(String, usize, PathBuf)> = missing
        .into_iter()
        .map(|(name, (count, path))| (name, count, path))
        .collect();
    // 出現多い順
    rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    for (name, count, path) in &rows {
        let rel = path.strip_prefix(workspace_root()).unwrap_or(path);
        println!("  - {name}  ({count}x)  e.g. {}", rel.display());
    }
    println!();
    println!("---");
    println!(
        "実シナリオで触れる命令は、warning ノイズになるので command_catalog \
         に Stub として追加するか、実装する。"
    );

    ExitCode::SUCCESS
}
