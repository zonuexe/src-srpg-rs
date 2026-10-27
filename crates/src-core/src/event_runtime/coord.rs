//! スクリプト座標と内部座標の変換。
//!
//! 原典 SRC のマップ座標は左上隅が (1,1) の 1 始まり (`Createコマンド.md` /
//! `Moveコマンド.md` / `進入イベント.md` の「左上隅が(1,1)」)。本実装の盤面
//! (`MapData` / `UnitInstance.x` / `.y`) は 0 始まりで持つため、`.eve` との
//! 境界 (コマンドの座標引数・`X()` / `Y()` 等の戻り値・`進入 <unit> <x> <y>:`
//! ラベル) では必ずここを通して ±1 する。
//!
//! 本移植独自のデータ定義コマンド (`MapSize` / `SetTile` / `Place` / `MoveUnit`)
//! と `TerrainId()` は原典に無い内部形式で、0 始まりのまま変換しない
//! (`docs/ARCHITECTURE_AUDIT.md`)。

use crate::App;

/// スクリプト座標 (1 始まり) を盤内に丸めて内部座標 (0 始まり) にする。
///
/// 原典 `ExecCreateCmd` / `ExecMoveCmd` / `ExecLaunchCmd` / `ExecCenterCmd` /
/// `ExecOrganizeCmd` / `ExecMapAttackCmd` の
/// `If ux < 1 Then ux = 1 ElseIf ux > MapWidth Then ux = MapWidth` と同じで、
/// 0 以下は左端・上端、幅・高さ超は右端・下端になる。マップ未定義なら下限だけ丸める。
pub fn clamp_from_script(app: &App, x: i64, y: i64) -> (u32, u32) {
    let (w, h) = app
        .database()
        .map
        .as_ref()
        .map_or((i64::MAX, i64::MAX), |m| {
            (i64::from(m.width).max(1), i64::from(m.height).max(1))
        });
    (clamp_axis(x, w), clamp_axis(y, h))
}

/// スクリプト座標 (1 始まり) が盤内なら内部座標 (0 始まり) にする。盤外・マップ
/// 未定義は `None` (原典 `Info(マップ, X, Y, …)` の `mx < 1 Or MapWidth < mx` →
/// 空、`ExecChangeTerrainCmd` の範囲エラー)。
pub fn checked_from_script(app: &App, x: i64, y: i64) -> Option<(u32, u32)> {
    let m = app.database().map.as_ref()?;
    let inside = |v: i64, len: u32| (1..=i64::from(len)).contains(&v);
    (inside(x, m.width) && inside(y, m.height)).then(|| ((x - 1) as u32, (y - 1) as u32))
}

/// 盤サイズで丸めない 1 軸の変換 (表示位置の計算や AI の目的地の解釈用)。
/// 盤外の値もそのまま 1 ずらす。
pub const fn axis_from_script(v: i64) -> i64 {
    v - 1
}

/// 内部座標 (0 始まり) をスクリプト座標 (1 始まり) にする (`X()` / `Y()` /
/// `進入` ラベルの座標)。
pub const fn to_script(v: u32) -> i64 {
    v as i64 + 1
}

fn clamp_axis(v: i64, len: i64) -> u32 {
    (v.clamp(1, len) - 1).min(i64::from(u32::MAX)) as u32
}
