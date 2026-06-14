//! マップデータ / Map data.
//!
//! 元実装: `Map.bas` の `MapData` グローバル配列と `LoadMapData` (`Map.bas:397`)。
//! 元フォーマット:
//!
//! ```text
//! "MapData"
//! {format-marker}
//! {width}, {height}
//! {terrain_id} {bitmap_no}    // for each of width*height cells (column-major)
//! ...
//! [optional: "Layer" section with width*height pairs]
//! ```
//!
//! 移植版 v1 は配列のみで保持し、ファイルパーサは未実装。`demo()` で
//! 組み込みのサンプルマップを返す。

use serde::{Deserialize, Serialize};

use super::pilot::ParseError;
use super::terrain::DEFAULT_TERRAINS;

/// 1 セル分のマップデータ / One map cell.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MapCell {
    /// 元: `MapData(*,*,TerrainType)`
    pub terrain_id: u32,
    /// 元: `MapData(*,*,BitmapNo)`
    pub bitmap_no: u32,
}

/// マップ全体 / Whole map grid.
///
/// 元 VB6 は 1-origin の配列だが、Rust 側では 0-origin で持つ。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapData {
    pub width: u32,
    pub height: u32,
    pub cells: Vec<MapCell>,
}

impl MapData {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            cells: vec![MapCell::default(); (width * height) as usize],
        }
    }

    pub fn cell(&self, x: u32, y: u32) -> MapCell {
        debug_assert!(x < self.width && y < self.height);
        self.cells[(y * self.width + x) as usize]
    }

    pub fn set_cell(&mut self, x: u32, y: u32, cell: MapCell) {
        debug_assert!(x < self.width && y < self.height);
        self.cells[(y * self.width + x) as usize] = cell;
    }
}

/// 組み込みのデモマップ / Built-in demo map for visualisation.
///
/// 24x16 セル (デフォルトビュー 16x10 より大きく、スクロール動作確認用)。
/// 上部に山岳・右端に海・中央南北に道・西部に複数の都市・全体に森を散らす。
pub fn demo() -> MapData {
    const W: u32 = 24;
    const H: u32 = 16;
    let mut m = MapData::new(W, H);

    let plain = 0u32;
    let road = 1u32;
    let forest = 2u32;
    let mountain = 3u32;
    let sea = 4u32;
    let city = 5u32;

    for y in 0..H {
        for x in 0..W {
            let id = if y <= 1 {
                mountain
            } else if x >= 20 {
                sea
            } else if x == 11 || y == 8 {
                road
            } else if (y == 12 && (x == 2 || x == 3)) || (y == 13 && x == 2) {
                city
            } else if (x + y) % 5 == 0 {
                forest
            } else {
                plain
            };
            m.set_cell(
                x,
                y,
                MapCell {
                    terrain_id: id,
                    bitmap_no: 0,
                },
            );
        }
    }
    m
}

// ===== .map ファイルパーサ / .map file parser =====

/// 元 `Map.bas::LoadMapData` (`Map.bas:397`) のフォーマットを解釈する。
///
/// 期待される構成:
/// ```text
/// "MapData"
/// {format_marker}     // 通常は空文字や "Format" 等。スキップする
/// {width}, {height}
/// {terrain_id} {bitmap_no}    // width * height セル分（列メジャ: x 外, y 内）
/// ...
/// [optional: "Layer" + width * height pairs]
/// ```
///
/// VB6 の `Input #` はカンマ・空白・改行・タブをすべてセパレータとして
/// 扱い、ダブルクオート文字列は囲みを取り除いて 1 トークンとして読む。
/// 本実装も同等の挙動。`#` 行頭コメント / `//` 行末コメントは未サポート
/// （元ファイルにも含まれないため）。
pub fn parse(src: &str) -> Result<MapData, ParseError> {
    let mut t = Tokenizer::new(src);

    let header = t.next_token()?.ok_or_else(|| eof(0))?;
    if header.value != "MapData" {
        return Err(ParseError {
            line_num: header.line_num,
            message: format!("MapData ヘッダがありません (実際: {:?})", header.value),
        });
    }
    // 2 トークン目はフォーマットマーカ。読み捨て。
    let _ = t.next_token()?.ok_or_else(|| eof(header.line_num))?;

    let w_tok = t.next_token()?.ok_or_else(|| eof(header.line_num))?;
    let width: u32 = w_tok
        .value
        .parse()
        .map_err(|_| err(w_tok.line_num, "width が整数ではありません。"))?;
    let h_tok = t.next_token()?.ok_or_else(|| eof(w_tok.line_num))?;
    let height: u32 = h_tok
        .value
        .parse()
        .map_err(|_| err(h_tok.line_num, "height が整数ではありません。"))?;

    if width == 0 || height == 0 {
        return Err(err(
            h_tok.line_num,
            "width/height は 1 以上である必要があります。",
        ));
    }

    let mut map = MapData::new(width, height);
    // 元コード: For i = 1 To MapWidth: For j = 1 To MapHeight (列メジャ)
    for x in 0..width {
        for y in 0..height {
            let tid = t.next_int()?;
            let bmp = t.next_int()?;
            map.set_cell(
                x,
                y,
                MapCell {
                    terrain_id: tid as u32,
                    bitmap_no: bmp as u32,
                },
            );
        }
    }
    // "Layer" セクションはここでは無視（v1）。
    Ok(map)
}

/// `Input #` 風トークナイザ。カンマ / 空白 / 改行 / タブをセパレータ扱い。
/// `"..."` で囲まれた値はクオートを外して 1 トークン。
struct Tokenizer<'a> {
    src: &'a str,
    bytes: &'a [u8],
    pos: usize,
    line: usize,
}

#[derive(Debug, Clone)]
struct Token {
    value: String,
    line_num: usize,
}

impl<'a> Tokenizer<'a> {
    fn new(src: &'a str) -> Self {
        Self {
            src,
            bytes: src.as_bytes(),
            pos: 0,
            line: 1,
        }
    }

    fn skip_separators(&mut self) {
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            match b {
                b' ' | b'\t' | b'\r' | b',' => self.pos += 1,
                b'\n' => {
                    self.line += 1;
                    self.pos += 1;
                }
                _ => break,
            }
        }
    }

    fn next_token(&mut self) -> Result<Option<Token>, ParseError> {
        self.skip_separators();
        if self.pos >= self.bytes.len() {
            return Ok(None);
        }
        let start_line = self.line;

        // クオート文字列
        if self.bytes[self.pos] == b'"' {
            self.pos += 1;
            let mut buf = String::new();
            // UTF-8 多バイト文字 (日本語等) を壊さないよう、char 単位で走査する。
            // `b as char` を使う旧実装は `森林ステージ` のような名前を
            // バイト単位の不正な Latin-1 風文字列に分解してしまっていた。
            loop {
                if self.pos >= self.bytes.len() {
                    return Err(err(
                        start_line,
                        "クオート文字列の終端 `\"` が見つかりません。",
                    ));
                }
                let Some(ch) = self.src[self.pos..].chars().next() else {
                    return Err(err(
                        start_line,
                        "クオート文字列の終端 `\"` が見つかりません。",
                    ));
                };
                if ch == '"' {
                    self.pos += ch.len_utf8();
                    break;
                }
                if ch == '\n' {
                    self.line += 1;
                }
                buf.push(ch);
                self.pos += ch.len_utf8();
            }
            return Ok(Some(Token {
                value: buf,
                line_num: start_line,
            }));
        }

        // 非クオート: 次のセパレータまで
        let begin = self.pos;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if matches!(b, b' ' | b'\t' | b'\r' | b'\n' | b',') {
                break;
            }
            self.pos += 1;
        }
        let value = self.src[begin..self.pos].to_string();
        Ok(Some(Token {
            value,
            line_num: start_line,
        }))
    }

    fn next_int(&mut self) -> Result<i64, ParseError> {
        let line_before = self.line;
        let tok = self
            .next_token()?
            .ok_or_else(|| err(line_before, "整数を期待していましたが EOF に到達しました。"))?;
        tok.value.parse::<i64>().map_err(|_| {
            err(
                tok.line_num,
                &format!("整数として解釈できません: {:?}", tok.value),
            )
        })
    }
}

fn err(line_num: usize, message: &str) -> ParseError {
    ParseError {
        line_num,
        message: message.to_string(),
    }
}

fn eof(line_num: usize) -> ParseError {
    err(line_num, "予期しない EOF。")
}

/// デフォルト地形カタログに含まれない terrain_id を一覧で返す（バリデーション用）。
pub fn missing_terrain_ids(map: &MapData) -> Vec<u32> {
    let mut out = Vec::new();
    for y in 0..map.height {
        for x in 0..map.width {
            let id = map.cell(x, y).terrain_id;
            if !DEFAULT_TERRAINS.iter().any(|t| t.id == id) && !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_map_uses_known_terrain_ids() {
        let m = demo();
        assert_eq!(m.width, 24);
        assert_eq!(m.height, 16);
        assert!(missing_terrain_ids(&m).is_empty());
    }

    #[test]
    fn cell_setter_round_trips() {
        let mut m = MapData::new(3, 2);
        m.set_cell(
            1,
            1,
            MapCell {
                terrain_id: 4,
                bitmap_no: 7,
            },
        );
        assert_eq!(m.cell(1, 1).terrain_id, 4);
        assert_eq!(m.cell(0, 0).terrain_id, 0);
    }

    #[test]
    fn parse_minimum_2x2_map() {
        let src = "\
\"MapData\"
\"Format\"
2, 2
0 0
2 0
1 0
4 0
";
        let m = parse(src).expect("parse ok");
        assert_eq!(m.width, 2);
        assert_eq!(m.height, 2);
        // 列メジャ: (0,0), (0,1), (1,0), (1,1)
        assert_eq!(m.cell(0, 0).terrain_id, 0);
        assert_eq!(m.cell(0, 1).terrain_id, 2);
        assert_eq!(m.cell(1, 0).terrain_id, 1);
        assert_eq!(m.cell(1, 1).terrain_id, 4);
    }

    #[test]
    fn parse_rejects_missing_header() {
        let src = "Foo\nBar\n1,1\n0 0\n";
        let e = parse(src).unwrap_err();
        assert!(e.message.contains("MapData ヘッダ"));
    }

    #[test]
    fn parse_handles_comma_and_whitespace_mixed() {
        let src = "\"MapData\",\"f\",1,1,5,0";
        let m = parse(src).unwrap();
        assert_eq!(m.width, 1);
        assert_eq!(m.cell(0, 0).terrain_id, 5);
    }

    #[test]
    fn parse_truncated_returns_eof_error() {
        let src = "\"MapData\"\n\"f\"\n2,2\n0 0\n0 0\n0 0\n"; // 4 セル必要だが 3 セルしかない
        assert!(parse(src).is_err());
    }

    #[test]
    fn tokenizer_preserves_japanese_in_quoted_string() {
        // 旧実装は b as char で日本語クォート内文字を mojibake 化していた。
        // 1x1 map with header / format / size / single cell — header と format が
        // 日本語クォート文字列でも正しく取り回せること。
        let src = "\"MapData\"\n\"森林ステージ\"\n1,1\n5 7\n";
        let m = parse(src).expect("parse ok");
        assert_eq!(m.width, 1);
        assert_eq!(m.height, 1);
        assert_eq!(m.cell(0, 0).terrain_id, 5);
        assert_eq!(m.cell(0, 0).bitmap_no, 7);
    }
}
