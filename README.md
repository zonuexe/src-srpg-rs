# src-srpg-rs

[SRC (Simulation RPG Construction)][SRC]のWebブラウザで動作する[派生版]として、Rust+WebAssemblyに移植するプロジェクト。

> [!IMPORTANT]
> このプロジェクトはまだ開発途中であり、多くのシナリオを完全動作できる段階ではありません。

| Crate | 役割 |
| --- | --- |
| `crates/src-core` | プラットフォーム非依存のエンジン本体。元 VB6 の `*.bas` / `*.cls` を移植していく先 |
| `crates/src-web` | wasm-bindgen + web-sys + Canvas 2D によるブラウザフロントエンド |

現状は **環境構築フェーズ**。ブラウザに「Hello SRC」スプラッシュを表示するだけの最小構成が動く。以降の移植のための土台。

## 必要環境

- [Nix](https://nixos.org/) (Flakes 有効)
  - `nix` が PATH になければ `/nix/var/nix/profiles/default/bin/nix` を使用する。
- それ以外の依存（Rust toolchain / wasm32 ターゲット / trunk / wasm-bindgen-cli / wasm-opt / Just）はすべて `flake.nix` 経由で供給される。ホストへの直接インストール不要。

## 開発シェルに入る

```sh
nix develop
```

シェル内で `just --list` を叩くと利用可能タスクが見える。

## ブラウザで表示する

```sh
nix develop --command just serve
# または
nix develop --command trunk serve
```

<http://127.0.0.1:8080/>をブラウザで開くと、Canvasにタイトル画面が描画される。
クリック or 任意キー押下で Configuration（設定変更）→ Main の順にシーンが遷移する。
ソースを編集すれば自動で再ビルド・リロードされる。

## ブラウザ自動検証 (Claude Preview MCP)

`.claude/launch.json`に`trunk-serve`構成が含まれている。Claude Code上のPreview MCPから`preview_start`で起動し、`preview_screenshot` / `preview_click`などで描画の自動検証ができる。

```text
preview_start({ name: "trunk-serve" })       # 開発サーバ起動
preview_screenshot({ serverId: ... })         # タイトル画面の確認
preview_click({ serverId: ..., selector: "#src-canvas" })
preview_screenshot({ serverId: ... })         # Configuration 画面の確認
```

## 本番ビルド

```sh
nix develop --command just build
```

`dist/` 配下に静的アセットが出力される。任意の静的ホスティングで配信可能。

## よく使うタスク

| コマンド | 内容 |
| --- | --- |
| `just serve` | 開発サーバ起動 (http://127.0.0.1:8080) |
| `just build` | 本番ビルド (`dist/`) |
| `just check` | `wasm32-unknown-unknown` 向けの型チェック |
| `just test`  | `src-core` のネイティブテスト |
| `just lint`  | Clippy (`-D warnings`) |
| `just fmt`   | rustfmt |
| `just verify`| 開発サーバ + Preview MCP からの自動検証用エントリ |
| `just clean` | `dist/` と `target/` を削除 |

## ディレクトリ構成

```
.
├── flake.nix              # 開発環境定義 (fenix で stable Rust + wasm32 target)
├── rust-toolchain.toml    # Rust ツールチェイン pin
├── Cargo.toml             # ワークスペース定義
├── Trunk.toml             # ビルド設定 (entry: crates/src-web/index.html)
├── justfile               # タスクランナー
├── crates/
│   ├── src-core/          # エンジン本体（プラットフォーム非依存）
│   └── src-web/           # WASM フロントエンド + index.html
└── SRC_20121125/          # 原典 VB6 ソース (GPL-3.0)
```

## 移植方針

- **コード対応関係**: VB6の`*.bas`/`*.cls`は基本1ファイル＝1モジュール（または型）として `crates/src-core/src/` 配下にマッピングする予定。
- **識別子**: 原典の日本語識別子は英語化し、元の名前はコメントで併記する（例: `// 元: ユニットを移動させる`）。
- **エンコーディング**: 原典はShift_JIS。読込時は変換する。Rust側ソースはすべてUTF-8。
- **GUI**: VB6の`*.frm`（Form）はCanvas 2D上で再現。ウィンドウやダイアログはブラウザのHTML/CSSとCanvasを組み合わせて表現する。
- **プラットフォーム非依存の徹底**: `src-core` は `#![forbid(unsafe_code)]`。`std::fs`・スレッド・ブロッキングI/Oを使わず、ファイルアクセスはVFS（仮想ファイルシステム）、待機処理は async/await で表現する。WASM以外への移植余地を残すため、描画・音声・入力は trait 層で抽象化し `src-web` が実装する。
- **`.eve` スクリプト互換の維持**: 原典のイベントスクリプト言語（式評価器・3層変数スコープ Local/Global/Sub-local・`${name}` / `$(name)` 展開）を再現し、既存シナリオを無改変で実行できることを目標とする。
- **整数演算で統一**: 移動範囲・射程などの計算では浮動小数点の等値比較を避け、整数で計算する（原典との結果一致のため）。
- **VB6 の直訳を避ける**: 挙動を保ったまま Rust idiom へ再設計する。原典の構造をそのまま写すのではなく、型・所有権・エラー処理を Rust 流に組み直す。
- **セーブ互換**: 動的状態型には `Serialize` / `Deserialize` を付与し、新フィールドは `#[serde(default)]` で後方互換を確保する。

## テスト・検証

- **挙動一致の検証**: ネイティブの単体/統合テスト（`just test`）に加え、実在の第三者シナリオ（`.eve`）を丸ごとロード・実行するスモークテストで原典との挙動一致・回帰を検出する（シナリオ素材は非同梱・各自取得）。
- **SRC.Sharp との差分管理**: 原典 VB6 と SRC# で挙動が分かれる箇所や意図的な乖離は [`docs/SRC_SHARP_DIVERGENCE.md`](docs/SRC_SHARP_DIVERGENCE.md) に記録し、原典準拠を基本とする。
- **仕様カバレッジの追跡**: 原典ヘルプ（`SRC.Sharp.Help`）に対する実装カバレッジを [`docs/SRC_COVERAGE_REPORT.md`](docs/SRC_COVERAGE_REPORT.md) で管理する。

## ライセンス

GPL-3.0-or-later。原典 SRC のライセンスを継承する（全文は [`LICENSE`](LICENSE)）。

```
Copyright © , 1997-2012 Kei Sakamoto, Inui Tetsuyuki
```

本プロジェクトはSRCの**派生版**として公開するものであり、SRC公式の[派生版に関する方針][派生版]、ならびに以下の派生版規約に準拠する。

- [派生版 規約(形式1)](http://www.src-srpg.jpn.org/hasei_kiyaku1.html)
- [派生版 規約(形式2)](http://www.src-srpg.jpn.org/hasei_kiyaku2.html)

## 謝辞

[SRC (Simulation RPG Construction)][SRC]のオリジナル作者であるKei Sakamoto氏および、長年にわたって管理に携ってきた乾哲雄樹氏に深く感謝いたします。
また、7474氏によるC#移植版([SRC#][SRC.Sharp])も移植にあたって挙動・仕様の参照実装として大いに活用させていただいており、非常に感謝いたします。

[SRC]: http://www.src-srpg.jpn.org/
[SRC.Sharp]: https://github.com/7474/SRC
[派生版]: http://www.src-srpg.jpn.org/development_hasei.shtml
