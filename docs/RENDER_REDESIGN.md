# 描画アーキテクチャ抜本再設計案 — 「アドホック座標」から「原典準拠表示リスト」へ

状態: **方針承認済み / 未着手**（2026-06-10）。
対象: `crates/src-web/src/render.rs` の描画ロジック全体と、その src-core 側への移設。

---

## 1. 診断 — なぜ座標・重なりバグが繰り返されるのか

VB6 版 SRC (`SRC_20121125/`) / C# 移植 (`SRC.Sharp/`) からの移植で、画像・文字の
座標位置や重なりが期待と合わず、アドホック修正が繰り返されてきた。調査の結果、
個別バグではなく**構造的要因**が特定された:

1. **座標計算の分散**: 全描画ロジックが `crates/src-web/src/render.rs` (2566行) に
   集中し、各 `draw_*` 関数が独立にオフセット・中央寄せを手計算。マジックナンバー
   (フォント 9〜18px が十数種、行高 12/14/16/20/24/26 混在、pad 2/6/8/12) が散在。
2. **独自レイアウトと原典座標の不整合**: 現実装は独自レイアウト (CANVAS 1280x480、
   マップ中央寄せ ox=320、右パネル STATUS_PANEL_X=480)。一方 `.eve` スクリプトの
   PaintPicture/PaintString は**原典の 640x480 画面座標前提**で書かれており、
   座標変換ずれが構造的に発生し続ける。
3. **テキスト計測の不在**: measureText 不使用。`wrap_text()` は ASCII=1幅/非ASCII=2幅
   の文字数近似のみ。textBaseline 設定が関数ごとにバラバラ ("top"/"middle"/"bottom" 混在)。
4. **Z 順 = 関数呼び出し順**: 重なり制御が draw_scene 内の呼び出し順序に暗黙依存。
   重なりバグの温床 (docs/CURRENT_WORK.md §8.2〜8.4 に実例)。
5. **テスト不能**: Canvas 描画は WASM のみで `cargo test` 不可。座標バグはブラウザ
   目視でしか発見できない。

### 活用できる既存資産

- `crates/src-core/src/script_overlay.rs` — `.eve` 命令を `DrawCmd` enum のリストに
  蓄積し src-web が実行する**表示リストパターンが既に存在**(.eve 専用)。これを
  全シーン描画に一般化するのが本設計の核。
- `crates/src-core/src/scene/mod.rs` — `Scene::size()` / `render_reads()` の宣言的
  検証パターン。
- SRC.Sharp が移植の成功例: ピクセル統一 (Twip 廃止)、`MapToPixelX/Y` の定式化
  (`SRC.Sharp/SRC.Sharp/SRCSharpForm/SRCSharpFormGUI.cs:369-405`)、フォント
  プリセット (`SRCSharpFormGUI.draw.cs:682-718`)、IGUI による描画抽象化。
- `crates/src-core/src/ui/` (IGUI 系 trait) は**実装のないデッドコード**(モック
  テストのみ) → 新アーキテクチャ導入後に削除。

## 2. 決定事項

1. **原典準拠に統一**: 640x480 論理画面・32px セル・原典ウィンドウ配置に完全準拠。
   Canvas 上の拡大/センタリングは外側 1 箇所のみで変換(レターボックス方式)。
2. **基盤先行・段階移行**: 表示リスト + レイアウト層 + ネイティブテスト基盤を先に
   作り、画面を 1 つずつ移行。
3. **優先順**: ①マップ・ユニット描画 → ②ステータス・一覧画面 → ③ダイアログ →
   ④イベント絵・戦闘演出。

## 3. 原典座標仕様 (準拠目標)

| 項目 | 値 | 原典出所 |
|------|-----|---------|
| マスサイズ | 32x32 px | Map.bas (`MapPWidth = 32 * w`) |
| 論理画面 (新GUI) | 640x480 px (20x15 マス) | GUI.bas `MainWidth=20, MainHeight=15` |
| ユニットステータス窓 | `(MainPWidth-240, 10, 235, MainPHeight-20)` 右オーバーレイ | Status.bas:308 |
| HP/EN ゲージ幅 | 88 px | GUI.bas `GauageWidth` |
| 顔画像 | 64x64 px | picFace |
| マップ⇔ピクセル変換 | `MapToPixelX(X) = 32 * ((MainWidth+1)/2 - 1 - (MapX - X))` | SRCSharpFormGUI.cs:369-405 |
| フォント | MS PGothic 系。メッセージ 16pt Bold / ステータス 9pt / システム 9pt Bold / 戦闘アニメ 8pt | SRCSharpFormGUI.draw.cs:682-718 |

## 4. 新アーキテクチャ

```
src-core (純粋 Rust・ネイティブテスト可)            src-web (薄い実行器)
┌─────────────────────────────────┐   ┌──────────────────────┐
│ render/layout.rs  原典定数+Rect  │   │ 1) レターボックス変換 │
│ render/frame.rs   RenderFrame    │──→│    (scale+offset 1箇所)│
│ render/scenes/*   シーン別フレーム│   │ 2) FrameCmd を Canvas │
│ TextMetrics trait (計測抽象)     │   │    に逐次実行          │
└─────────────────────────────────┘   │ 3) measureText 提供    │
  App::build_frame(&TextMetrics)       └──────────────────────┘
   → RenderFrame (毎フレーム生成・serialize しない)
```

主要型 (新規 `crates/src-core/src/render/` モジュール):

```rust
// layout.rs — 原典定数の唯一の出所。マジックナンバーをここに集約
pub const CELL_PX: i32 = 32;
pub const MAIN_W_CELLS: i32 = 20;          // 新GUI
pub const MAIN_H_CELLS: i32 = 15;
pub const SCREEN_W: i32 = 640;             // = MAIN_W_CELLS * CELL_PX
pub const SCREEN_H: i32 = 480;
pub const GAUGE_W: i32 = 88;
pub const FACE_PX: i32 = 64;
pub struct Rect { pub x: i32, pub y: i32, pub w: i32, pub h: i32 }
impl Rect { fn inset(..), fn rows(..), fn anchor_*(..), fn contains(..) }
pub fn unit_status_rect() -> Rect;          // 原典 Status.bas:308 の式
pub fn map_to_pixel(map_xy, view_center) -> (i32, i32);  // SRC.Sharp 式
pub fn pixel_to_map(px_xy, view_center) -> (i32, i32);   // 逆変換(入力用)

// frame.rs — Z 層付き表示リスト
pub enum Layer {  // VB6 の描画順を enum 化。u8 順でソート
    MapLower, MapUpper, Units, Ranges, Cursor,
    ScriptOverlay, BattleAnim, StatusWindow, Menu, Message, Dialog, Fade,
}
pub enum FontPreset { Message, Status, System, BattleAnime }  // SRC.Sharp DrawStringMode 相当
pub struct FrameCmd { pub layer: Layer, pub cmd: DrawCmd }    // 既存 DrawCmd を流用・拡張
pub struct RenderFrame { cmds: Vec<FrameCmd> }                // push 後 layer 安定ソート
impl RenderFrame { fn push(layer, cmd), fn sorted_cmds(&self) -> &[FrameCmd] }

// 計測抽象 — web: ctx.measureText / native テスト: 全角=size, 半角=size/2 の決定的近似
pub trait TextMetrics { fn text_width(&self, text: &str, font: FontPreset) -> i32; }
```

`DrawCmd` (script_overlay.rs) への追加: `Text { rect, text, font: FontPreset, align, wrap }`
(計測込みの整形済み出力でも可)、`Image { name, dst: Rect, .. }`、`StrokeRect`、
`Gauge { rect, ratio, kind }`。**既存バリアントの serde 形は変更しない**(セーブ互換維持。
RenderFrame 自体は毎フレーム導出されるため serialize 不要)。

src-web 側は `render.rs` を最終的に「①論理 640x480 → 実 Canvas のスケール+レターボックス
変換(`ctx.translate/scale` 1 箇所)②`FrameCmd` 逐次実行 ③`TextMetrics` の measureText 実装」
のみに縮退させる。**入力系も同じ変換を逆適用**(マウス実座標 → 論理 640x480 →
`pixel_to_map`)し、描画と入力の座標源を単一化する。

## 5. 段階移行フェーズ

各フェーズは単独で `just check` + `just test` + ブラウザ目視 (`just serve`) が通る単位。
移行中は `App::build_frame()` が対応済みシーンのみ `Some(RenderFrame)` を返し、src-web は
None のシーンを既存レガシーパスで描く(新旧併存の管理点はこの 1 箇所)。

- **Phase R0 — 基盤**: `src-core/src/render/{layout,frame}.rs` 新設、`TextMetrics` trait、
  src-web にレターボックス変換 + FrameCmd 実行器 + measureText 実装を追加。golden テスト
  基盤 (`crates/src-core/tests/render_frame.rs`)。`map_to_pixel` が SRC.Sharp の式と一致
  することを表で検証するテストを含む。
- **Phase R1 — マップ・ユニット描画** (優先①): `draw_map_view` のタイル/ユニット/カーソル/
  移動範囲を `render/scenes/map_view.rs` のフレーム生成へ移植。原典 640x480・`MapToPixel`
  式に切替。スクロール・マウスヒットテストを `pixel_to_map` に統一。
- **Phase R2 — ステータス・一覧画面** (優先②): ステータスパネルを原典の右オーバーレイ窓
  (235px, Status.bas 準拠) に変更。PilotList/UnitList/Intermission の行レイアウトを
  `Rect::rows` ベースに移植。
- **Phase R3 — ダイアログ・メニュー**: Talk/Confirm/Menu/コマンドメニューを
  Layer::Dialog/Menu に移植。折返しを TextMetrics ベースに置換 (`wrap_text` の文字数近似を廃止)。
- **Phase R4 — イベント絵・戦闘演出**: script_overlay を Layer::ScriptOverlay として
  RenderFrame に合流 (`.eve` 座標は 640x480 論理座標にそのまま一致するようになる)。
  battle_anim の進捗マジック定数を名前付き定数化。
- **Phase R5 — 後片付け**: src-web/render.rs のレガシーパス削除、`ui/` trait 層
  (IGUI 系デッドコード) 削除、CLAUDE.md の描画記述更新 (現状「描画エントリポイント:
  src-core::render::draw_scene()」は実態と不一致)。

## 6. テスト戦略

- **golden テスト**: `App` を test_harness で状態構築 → `build_frame(&FixedMetrics)` →
  `RenderFrame` を整形ダンプし expected と比較 (既存の `.eve` シナリオ
  `tests/fixtures/scenarios/*.expected` と同じ流儀)。座標リグレッションがネイティブ
  `just test` で検出可能になる。
- **原典突合テスト**: `map_to_pixel`/`pixel_to_map` の往復性と SRC.Sharp 式との一致、
  `unit_status_rect` 等の原典式の検証。
- **宣言検証の拡張**: 既存 `Scene::render_reads()` パターンに倣い、各 Layer の描画順序
  不変条件をテスト化。
- **目視検証**: 各フェーズ完了時に `just serve` + ブラウザでシナリオ実行
  (preview/スクリーンショット)。

## 7. リスクと対策

| リスク | 対策 |
|--------|------|
| セーブデータ互換 (`script_overlay` は serialize 対象) | `DrawCmd` 既存バリアントの serde 形を変えない。追加フィールドは `#[serde(default)]`。`RenderFrame` は非永続 (毎フレーム導出) |
| 入力ヒットテストのずれ | 描画と入力を同じ `layout.rs` 定数 + 逆変換関数に強制。Rect::contains でヒットテスト |
| 移行中の新旧混在 | `build_frame() -> Option<RenderFrame>` の 1 点でシーン単位に切替。新旧で同一シーンを二重描画しない |
| 640x480 への縮小で現 1280 レイアウトの情報量が減る | 原典の新GUI 同様、ステータスはオーバーレイ窓に移行。レターボックスで実 Canvas サイズは自由 |
| TextMetrics の web/native 差で golden テストがずれる | golden テストは FixedMetrics (決定的近似) 固定。実機差はレイアウト破綻でなく数 px の見た目差に留まる設計 (rect ベース配置) |

## 8. 検証方法 (各フェーズ共通)

```sh
nix --extra-experimental-features 'nix-command flakes' develop --command just check
nix --extra-experimental-features 'nix-command flakes' develop --command just test
nix --extra-experimental-features 'nix-command flakes' develop --command just lint
# 目視: just serve → http://127.0.0.1:8080 でシナリオ実行
```

コミット前に `just fmt` 必須 (CLAUDE.md 規約)。
