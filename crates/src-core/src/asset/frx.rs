//! VB6 .frx (Form resource) ファイルの最小パーサ。
//! Minimal parser for VB6 .frx (Form resource) files.
//!
//! .frm 内の `Picture = "Foo.frx":HEX` のような参照は、`Foo.frx` のオフセット
//! `HEX` から以下のレイアウトでリソースを取り出す:
//!
//! ```text
//! offset+0  : u32 outer_len   (リソース全体長 / not always required)
//! offset+4  : u32 kind        (タイプマーカ。通常 0x0000746c = "lt\0\0")
//! offset+8  : u32 inner_len   (続く画像バイト数)
//! offset+12 : [u8; inner_len] (BMP / ICO / etc. の生バイト)
//! ```
//!
//! 例として `Title.frx` の `0x030A` には 200x40 の 8bpp BMP が、`0x268C` と
//! `0x0000` には Window アイコン用 ICO が格納されている。

/// 1 リソース分のスライスとメタ情報 / One extracted resource slice plus metadata.
#[derive(Debug, Clone, Copy)]
pub struct FrxResource<'a> {
    /// VB6 内部の型タグ。多くの埋め込み画像で `0x0000746c`。
    pub kind: u32,
    /// 解析時に確認した outer length（呼び出し側がリソース全体長を必要とする場合用）。
    pub outer_len: u32,
    /// 生の画像バイト（先頭が `BM` であれば BMP、`00 00 01 00` なら ICO 等）。
    pub bytes: &'a [u8],
}

/// `file` の `offset` 位置から 1 リソースを取り出す。
/// Extracts one resource starting at `offset`.
pub fn read_at(file: &[u8], offset: usize) -> Option<FrxResource<'_>> {
    let header = file.get(offset..offset.checked_add(12)?)?;
    let outer_len = u32::from_le_bytes(header[0..4].try_into().ok()?);
    let kind = u32::from_le_bytes(header[4..8].try_into().ok()?);
    let inner_len = u32::from_le_bytes(header[8..12].try_into().ok()?) as usize;

    let body_start = offset + 12;
    let body_end = body_start.checked_add(inner_len)?;
    let bytes = file.get(body_start..body_end)?;
    Some(FrxResource {
        kind,
        outer_len,
        bytes,
    })
}

/// 取り出したバイト列が Windows BMP かどうか判定。
pub fn is_bmp(bytes: &[u8]) -> bool {
    bytes.len() >= 2 && bytes.starts_with(b"BM")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 12 バイトのリトルエンディアン header + ペイロード合成。
    fn build(outer: u32, kind: u32, payload: &[u8]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(&outer.to_le_bytes());
        v.extend_from_slice(&kind.to_le_bytes());
        v.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn reads_a_simple_resource() {
        let buf = build(0x100, 0x0000746c, b"BMxx");
        let r = read_at(&buf, 0).unwrap();
        assert_eq!(r.kind, 0x0000746c);
        assert_eq!(r.outer_len, 0x100);
        assert_eq!(r.bytes, b"BMxx");
        assert!(is_bmp(r.bytes));
    }

    #[test]
    fn returns_none_when_header_truncated() {
        let buf = vec![0u8; 8];
        assert!(read_at(&buf, 0).is_none());
    }

    #[test]
    fn returns_none_when_payload_truncated() {
        let mut buf = build(0x10, 0, b"abcd");
        // ペイロード長を 8 と偽装した上で末尾を削る
        buf.splice(8..12, [8, 0, 0, 0]);
        buf.truncate(15);
        assert!(read_at(&buf, 0).is_none());
    }
}
