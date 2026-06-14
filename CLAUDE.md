# src-srpg-rs — Claude Code ガイド

## ビルド環境

`cargo` / `rustc` / `trunk` / `just` は **Nix devShell** 内にのみ存在し、PATH には出ていない。
`nix-command` と `flakes` は実験的機能として明示的に有効化する必要がある。

### コマンドの実行方法

```
nix --extra-experimental-features 'nix-command flakes' develop --command <cmd>
```

`just` をエントリポイントにすると簡潔：

```
nix --extra-experimental-features 'nix-command flakes' develop --command just <recipe>
```

### よく使うレシピ（justfile）

| レシピ | 実際のコマンド | 備考 |
|--------|---------------|------|
| `just serve` | `trunk serve` | 開発サーバ http://127.0.0.1:8080 |
| `just build` | `trunk build --release` | dist/ に出力 |
| `just check` | `cargo check --workspace --target wasm32-unknown-unknown` | 全クレート型チェック |
| `just test` | `cargo test -p src-core` | ネイティブ単体テスト（src-core のみ） |
| `just lint` | `cargo clippy --workspace --target wasm32-unknown-unknown --all-targets -- -D warnings` | |
| `just fmt` | `cargo fmt --all` | |

> **注意**: テストは `src-core` クレートのネイティブビルドでのみ実行可能。WASM ターゲットではテストを実行しない。

> **コミット前に必ず `just fmt`（`cargo fmt --all`）を実行する。** リポジトリ全体を
> rustfmt 準拠に保つ運用とする。部分的な手動整形ではなく、コミット直前に
> ワークスペース全体を整形してから差分を確定すること。

> **コミット運用（自律コミット）**: 作業の区切り（機能実装・バグ修正・リファクタ・ドキュメント
> 更新の単位）ごとに、**ユーザの明示指示を待たず自律的にコミットする**。コミット前に必ず
> `just fmt` → `just test`（`cargo test -p src-core` 全緑）→ `just lint`（clippy `-D warnings`）
> → `just check`（wasm 型チェック）を通し、緑であることを確認してからコミットする（赤なら
> コミットしない）。コミットメッセージは Conventional Commits 風（`feat:` / `fix:` / `refactor:` /
> `docs:` / `test:`）の日本語要約。論理的に分離できる変更は複数コミットに分ける。
> **制約**: ① `git push` はユーザが明示的に指示したときのみ（自動 push しない）。
> ② `main`（デフォルトブランチ）には直接コミットせず、作業は feature ブランチで行う
> （`main` 上なら先にブランチを切る）。③ 破壊的・不可逆な操作（履歴改変・force push・
> ブランチ削除等）は事前確認する。

## ワークスペース構成

| クレート | パス | 役割 |
|---------|------|------|
| `src-core` | `crates/src-core` | ゲームロジック（no_std 互換、純粋 Rust） |
| `src-web` | `crates/src-web` | WASM フロントエンド（Yew + wasm-bindgen） |
| `verify-archive` | `tools/verify-archive` | アーカイブ検証ツール |

## ツールチェーン

- チャネル: `stable`
- ターゲット: `wasm32-unknown-unknown`（`rust-toolchain.toml` で固定）
- コンポーネント: `rustfmt`, `clippy`, `rust-src`, `rust-analyzer`

---

## アーキテクチャ

### クレート責務

**`src-core`** — プラットフォーム非依存のゲームロジック
- `#![forbid(unsafe_code)]`
- WASM 互換: `std::fs` を使わず VFS、スレッド非使用、時刻は `instant` クレート
- 元 VB6 の `.bas` / `.cls` を 1 モジュール（または型）単位で移植

**`src-web`** — WASM フロントエンド（wasm-bindgen + Canvas 2D）
- Archive（ZIP展開）/ Assets / Audio / Render モジュール
- Canvas API 呼び出しとイベント処理（キーボード・マウス・ファイル選択）
- 描画エントリポイント: `src-core::render::draw_scene()`

### 主要モジュール（src-core）

| モジュール | 役割 |
|-----------|------|
| `app.rs` | 上位状態機械・シーン管理・ゲーム全体制御 (`App`, `Scene`) |
| `db.rs` | 静的データ集約 (`GameDatabase`) |
| `data/` | データパーサー（pilot.txt / unit.txt / map.txt 等） |
| `unit_instance.rs` | マップ上のユニット実体（位置・所属勢力・状態） |
| `pilot_instance.rs` | パイロット実体（level, exp, sp, morale, plana, skills） |
| `unit_weapon.rs` | 武器の実行時状態（残弾・EN消費） |
| `unit_ability.rs` | アビリティの実行時状態 |
| `condition.rs` | 状態異常・精神コマンド効果 |
| `feature.rs` | ユニット特殊能力・スキル有効化判定 |
| `item_slot.rs` | 装備スロット管理 |
| `event_runtime.rs` | `.eve` イベントスクリプト実行 (`ScriptContext`, `execute()`) |
| `expression/` | 式評価器（演算子・関数・変数解決） |
| `combat.rs` | 戦闘予測・実行（命中率・ダメージ計算） |
| `movement.rs` | 移動範囲計算（Dijkstra） |
| `scene/` | シーン実装（Title / MapView / UnitList 等） |
| `ui/` | UI 抽象 trait 層（src-web が実装） |
| `turn.rs` | ターン / フェーズ管理 |
| `stage.rs` | ステージ進行状態（Briefing / Sortie / Battle / Victory） |
| `dialog.rs` | 対話 UI（Talk / Confirm / Input / Menu） |
| `command_menu.rs` | コマンドメニュー（ユニット・マップ） |

### 静的データ → 実行時インスタンスの対応

```
PilotData (static)        UnitData (static)         MapData
       ↓                         ↓                      ↓
PilotInstance (runtime)   UnitInstance (runtime)    Tile (x,y,terrain_id)
  level/exp/sp/morale       weapons: Vec<UnitWeapon>   move_cost / hit_mod
  plana / skills            abilities / conditions      damage_mod
  combat stats              active_features / item_slots
```

---

## テスト方針

- テストランナー: `cargo test -p src-core`（ネイティブのみ、WASM 不可）
- 統合テスト: `crates/src-core/tests/` 以下に配置
- ユニットテスト: 各ファイル末尾の `#[cfg(test)]` ブロック

主要テストファイル例:
- `pilot_function.rs` — `Pilot()` 関数の文法テスト
- `status_morale.rs` — 状態異常・モラル計算
- `map_attack.rs` — 広域攻撃の形状
- `item_equip.rs` — 装備スロット検証
- `damage_heal.rs` — ダメージ / 回復コマンド

---

## コーディング規約

### シリアライゼーション

- すべての動的状態型に `Serialize + Deserialize` を付ける（save/load対応）
- 新フィールド追加時は `#[serde(default)]` でバージョン互換性を確保
- セーブデータ出入口: `App::to_save_json()` / `App::from_save_json()`

### エラーハンドリング

- `.eve` パーサエラーは `ScriptError { line_num, message }` で返す
- 実行時エラーは `App.messages` に追記するか黙殺（フロントエンド堅牢性重視）

### 命名

- VB6 原典の日本語名は英語化し、コメントで原名を併記
- ユニット識別子: `uid`（一意 ID）、データ参照: `*_data_name`（名前文字列）

### `.eve` 変数スコープ

3 層構造: Local（関数内）/ Global（シナリオ全体）/ Sub-local（イベント内）
- `App.script_vars: BTreeMap<String, String>` で管理
- 展開形式: `${name}` / `$(name)` の両形式に対応

### ファイル I/O

- VFS（Virtual FileSystem）ベース、物理ファイルアクセスは禁止
- `src-web/archive.rs` が ZIP 展開 → `App.database` / Assets に蓄積
- `.eve` 内の `Open / Read / Write` は VFS 操作

---

## SRC 原典の参照

原典 SRC / SRC.NET の仕様を確認するときは **`SRC.Sharp/SRC.Sharp.Help/src/menu.md`**
をインデックスとして使う。各機能ドキュメントへのリンクが目次形式で網羅されており、
ここから辿ると目的の挙動仕様 (例: `Waitコマンド.md` / `Talkコマンド.md` /
`スタートイベント.md` / `ユニットコマンドイベント.md`) に最短で到達できる。

主なカテゴリ:

- **操作方法**: 画面構成 / 基本操作 / 各コマンド (`ユニットコマンド` / `マップコマンド`
  / `インターミッションコマンド`)
- **データの作成**: pilot / unit / item / sp / terrain 等の各データ形式
- **シナリオの作成**: イベントラベル / イベントコマンド / 関数のリファレンス

C# 実装 (`SRC.Sharp/SRC.NET/`) を読む前に、まず menu.md でドキュメント側の仕様
を確認すると、移植の意図と差分を把握しやすい。

---

## 禁止事項・制約

| 制約 | 理由 |
|------|------|
| `unsafe` コード | `#![forbid(unsafe_code)]` |
| `std::fs` 直接アクセス | WASM 非対応 |
| スレッド化 | WASM 単一スレッド |
| ブロッキング I/O | async/await で非同期化すること |
| 浮動小数点の等値比較 | 移動範囲計算等は整数に統一 |
| VB6 コードの直訳 | Rust idiom に合わせて再設計 |

---

## デバッグ補助（ブラウザコンソール）

開発サーバ起動中（`just serve`）にブラウザコンソールで使用可能:

```js
window.__srcDebug()      // App 状態サマリを出力
window.__srcVar("name")  // .eve 変数の値を参照
window.__srcImg()        // 画像解決状況ダンプ
```
