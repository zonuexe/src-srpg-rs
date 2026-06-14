//! シナリオデータ（`Data/*.txt`）の読み込み層 / Scenario data loader.
//!
//! 元 SRC は `Data\pilot.txt` / `Data\unit.txt` / `Data\item.txt` などを
//! テキスト形式で読み込む。
//! `Event.bas:1284` の `Open fname For Input` 群と `PilotDataList.Load` などが
//! 該当する。各ファイルはレコード単位の独自フォーマット。
//!
//! ここではまずパイロットデータ (`pilot.txt`) のみ移植している。
//! 拡張は段階的に。
//!
//! Originally `PilotDataList.Load` etc. parse text files under
//! `Data\` for each scenario. We start with the pilot data parser.

pub mod animation;
pub mod event;
pub mod item;
pub mod loader;
pub mod map;
pub mod pilot;
pub mod special_power;
pub mod terrain;
pub mod terrain_file;
pub mod unit;
