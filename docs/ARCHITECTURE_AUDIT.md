# モジュール配置・責務分担 監査 (2026-10-27, HEAD `5d27233`)

読み取りのみの監査。行番号は HEAD `5d27233` 時点（以下の実施後はファイル分割で行番号がずれている）。
「確認済」は実コードで裏取りしたもの、「要検証」はテスト未作成の推定。

## 実施状況（branch `refactor/module-boundaries`）

| ロードマップ | 状態 | 結果 |
|---|---|---|
| 1. デッドコード削除 | 済 | unit_instance.rs 3,299 → 614 行。combat.rs の DefenseMode / MapAttackShape、`ui/`、`parse_with` を削除。武器ゲートを `necessary_skill::weapon_usable_by` に切り出し、テストを本番経路に向けた |
| 2. util 新設・循環解消 | 済 | `util/{text,number}.rs`、`data/necessary.rs`。本番コードの循環は `app ⇄ event_runtime` のみ |
| 3. テスト外出し | 済 | `app/tests.rs`、`event_runtime/tests.rs`（トピック別の再分割は未） |
| 4. CLAUDE.md 修正 | 済 | §6 の記述ずれをすべて修正 |
| 5. `impl App` 分割 | 済 | app/mod.rs 1,564 行 + 子モジュール 14 個 |
| 6. event_runtime 分割 | 済 | 周辺関数を 8 個の子モジュールへ。`exec_command_pc` の 186 アームを 1 コマンド 1 関数にして `commands/` 以下の 10 カテゴリへ（mod.rs 約 7,000 → 1,687 行）。`eval_script_function` の分割は未 |
| 7. ビューモデル・ジオメトリを scene/ へ | 未 | |
| 8. B1〜B3 修正 | 済 | 回帰テスト 4 件を追加（すべて修正前に失敗することを確認） |
| 9. `.eve` コマンドをドメイン経路へ | 一部 | SpecialPower / Mind → `execute_spirit_by_event`、Transform → `set_unit_form`、UseAbility → `execute_ability`、Attack → `script_attack`（`resolve_strike` を本線攻撃と共有）。`map_attack*` の combat.rs 移動は未 |
| 10. 戦闘の一本化 | 一部 | 命中・ダメージ判定を `resolve_strike` に切り出し、本線攻撃と `.eve Attack` で共有。援護攻撃・反撃・先制反撃は未 |
| 11〜13 | 未 | |

作業中に見つかった追加の課題:
- **sp.txt の効果行**: 旧パーサは効果を `,` で区切っていたが、実データと C# `ToList` は空白区切り（`挑発 命中率低下Lv5`）。修正済。
- **`.eve Attack`**: 旧実装は命中判定なし・基本データの簡易式で、撃破したユニットを HP 0 以下のまま盤面に残していた。修正済（`script_attack`）。
- **`.eve UseAbility`**: 旧実装はアビリティ名で効果を決め打ちし、ユニットデータの効果を見ていなかった。データにあればそれを使うよう修正済。データに無い名前は旧処理にフォールバックする。
- **`.eve Transform` の変形イベント**（決定: 現状維持）: C# `TransformCmd` は変形イベントを発火しないが、本移植は `Combine` と揃えて発火する（fixture 21 で固定）。2026-10-27 にユーザ判断で確定。
- **B7**: 援護防御の発動判定 `try_find_support_guard`（app/battle.rs）は、最大 HP に基本値（`unit_by_name(..).hp`）を使っていた。戦闘本体は実効 HP を使うので、改造機や BossRank 機では発動閾値が食い違っていた。修正済（回帰テスト 1 件）。

## 0. 全体像

| ファイル | 行数 | うち inline テスト | 備考 |
|---|---|---|---|
| `src-core/src/app.rs` | 21,718 | 約 10,060 (253 tests) | `impl App` 1 ブロック 363 fn (11k 行) |
| `src-core/src/event_runtime.rs` | 20,417 | 約 6,390 | `exec_command_pc` 1 関数 5,700 行 / 196 アーム、`eval_script_function` 1,900 行 / 134 アーム |
| `src-web/src/render.rs` | 3,727 | | ゲームルールの一部を含む |
| `src-core/src/unit_instance.rs` | 3,299 | 約 1,900 | 約 850 行がデッドコード |
| `src-core/src/combat.rs` | 3,267 | 約 2,000 | マップ攻撃形状がデッドな重複 |

src-core の約 2/3 が `app.rs` と `event_runtime.rs` に集中している。
他のモジュール (turn / stage / flow / scene / dialog / modal) は型定義だけが置かれ、振る舞いはほぼ全部 App にある。

一番の問題はファイルが大きいことそのものではない。
**同じルールの実装が「App のプレイヤー経路」「.eve コマンド経路」「src-web 描画経路」「未使用のレガシー経路」に分かれ、実装同士の挙動がすでに食い違い始めている**ことである。

---

## 1. 責務の重複と分岐（最優先）

### 1-1. 戦闘解決が 4 系統ある
- **App 本線** `attack_resolve_and_run` (app.rs:3368, 739 行)
- **App 援護** `try_support_attack` (app.rs:4680)
- **App 反撃** `try_counterattack` (app.rs:5076)
- **`.eve` `Attack`** (event_runtime.rs:3941–4091)。`combat::predict_with_status` を直接呼ぶ。地形補正・防御属性・復活・弾薬消費を通らない。
- **(デッド)** `UnitInstance::execute_attack` / `execute_attack_with_counter` / `perform_counter_attack` ほか (unit_instance.rs:82–1394)。本番の呼び出し元はゼロ（確認済）。

App 内の 3 経路は、同じ一連の処理（回避 → 防御属性 → みがわり → バリア → フィールド → シールド → 不死 → 報酬）をそれぞれ手書きしている。
そのせいで次の食い違いがすでに出ている。
- `roll_parry` と `roll_weapon_instakill` は本線だけにある。
- **反撃で攻撃側を撃破したときに破壊イベントが発火しない**（app.rs:5227–5250 は `remove_unit_at` するだけで、`fire_destruction_label` を呼ばない。確認済）。

戦闘入力（状態名、地形 env ×2、`damage_spirit_levels`、ECM）を組み立てるコードは 5 か所にコピーされている（app.rs:3613, 4740, 5136, 6420, 6660）。
src-web にも 6 か所目がある（`build_combat_preview_line`, render.rs:2821）。既定の精神レベルと ECM 1.0 で独自に予測していて、コメント自身が本線との乖離を認めている。

### 1-2. 精神コマンド・アビリティ・変形が二重実装
- **SpecialPower**
  - `.eve` の `SpecialPower` (event_runtime.rs:3363–3510) は SP を消費して同名の Condition を付けるだけ。
  - `App::apply_spirit_effect` (app.rs:7062) が持つ回復・加速・無効化判定を通らない。
  - SP コスト表 `sp_cost_for` は event_runtime.rs:13963 にあり、App 側 (app.rs:6897) がそれを借りている。
- **UseAbility**: event_runtime.rs:4112–4341 が 修理・補給・状態回復 をハードコードしている。`App::apply_ability_effects` (app.rs:8948) を使わない。
- **Transform**: event_runtime.rs:2103 が独自に実装している。`App::transform_unit_by_uid` (app.rs:8596) のコメントには「`.eve Transform` と同じ本体」とあるのに、実際には呼ばれていない。`abilities` を再構築しない差分がある。

### 1-3. マップ攻撃形状が 2 実装
- **本番**: `event_runtime::map_attack_area` / `map_attack` (event_runtime.rs:13617, 13717)。AI の経路 (app.rs:5849, 5858) からも呼ばれる。つまり、プレイヤー／AI の戦闘がスクリプト層に依存している。
- **デッド**: `combat::MapAttackShape::affected_cells` / `is_unit_hit_by_map_attack` (combat.rs:973–1170)。呼び出し元はテストだけ（確認済）。しかもＭ拡の形状が本番の実装と食い違っている。

### 1-4. 式評価器が 3 系統、アトム解決が 4 系統
- **(A)** `eval_condition_args_with` + `eval_binop` (event_runtime.rs:6943, 8027)
  - 用途: If / Do / IIF。
  - 文字列比較と Like に対応する。未定義の識別子は false。等値判定は `f64::EPSILON`。
- **(B)** `tokenize_expr` → `parse_logical` ほか (12954–13274)
  - 数値専用。等値判定は `1e-9`。
  - 入口は `try_eval_int`、`try_eval_num`、`eval_int_expr` の 3 つ。
- **(C)** `evaluate_command_condition` (646)
  - (B) の結果に文字列の真偽判定を足したもの。メニュー条件などに使う公開 API。
  - 同じ式でも If で評価したときと結果が変わり得る。
- **アトム解決**も実装ごとに異なる。
  - `resolve_expr_atoms` はシステム変数を解決する。
  - `eval_numeric_atoms` はシステム変数を解決しない。
  - そのため `Set x (味方数 + 1)` が経路によっては落ちる可能性がある（要検証）。

CLAUDE.md が挙げている `expression/` モジュールは存在しない。

### 1-5. その他の小さな重複
- **`Turn::end_phase`** (turn.rs:93) は本番から呼ばれない。App がフェイズの巡回とターン数の加算を 2 か所 (app.rs:5306, 5420) で書き直している（確認済）。
- **陣営ラベルのパース**: `custom_command_targets_party` (app.rs:576) と `parse_party_label` (event_runtime.rs:12221) が重複。
- **「Lv」接尾辞のパーサ**が 4 つある: app.rs:7467、pilot_instance.rs:202、necessary_skill.rs:389、app.rs:616。`feature::feature_level` (feature.rs:59) が既に存在する。
- **パイロット成長式**: `db::grown_pilot` (db.rs:1033) と `PilotInstance::apply_stat_growth` (pilot_instance.rs:168) で二重。手で同期している。
- **全角→半角の畳み込み**: `fold_fullwidth_ascii` (event_runtime.rs:7040) と `loader::normalize_fullwidth` (data/loader.rs:182)。
- **武器の使用可否**
  - render.rs:2740 は気力と弾数しか見ていない。App の `weapon_skill_ok` (app.rs:8005) と食い違う。
  - `tests/necessary_skill_gate.rs` はデッドな `UnitInstance::is_weapon_available` をテストしている。本番のゲートは検証されていない。

---

## 2. 責務の不整合から来ているバグ候補

| # | 内容 | 根拠 | 状態 |
|---|---|---|---|
| B1 | 反撃で攻撃側を撃破すると破壊・全滅イベントが発火しない | app.rs:5227–5250 | 確認済（SRC 仕様との照合は未） |
| B2 | `.eve` の `Kill` / `Destroy` / `RemoveUnit` / `StopSummoning` が `unit_instances.retain` で直接消すため、`pos_index` が古いまま残る。その結果 `uid_at` が空きマスを占有中と判定する | event_runtime.rs:6236, 3936 ほか。`remove_unit_at` (db.rs:228) を迂回している | コード上は確認済、実害は要テスト |
| B3 | `RecoverHP` / `RecoverEN` / `Damage` が基本 HP (`unit_by_name(..).hp`) を基準にしている。アイテム、改造、BossRank 込みの `effective_max_hp` (db.rs:630) と食い違う | event_runtime.rs:7112, 7145, 7177, 7199 | 確認済 |
| B4 | スクリプトから呼んだ SpecialPower では、回復・気力などの効果が出ない。さらに原典仕様に反して SP を消費し、不足時は効果を出さなかった | 1-2 参照 | 修正済（回帰テスト 3 件） |
| B5 | `.eve` の Transform で abilities が再構築されない | 1-2 参照 | 修正済（回帰テスト 1 件） |
| B6 | `eval_numeric_atoms` がシステム変数を解決しない | 1-4 参照 | 要検証 |

## 3. レイヤリング

本番コードの依存循環（`#[cfg(test)]` を除く）:
1. `app ⇄ event_runtime`（App がマップ攻撃、ラベル発火、SP コストをスクリプト層から借りている）
2. `db → unit_instance → db`
3. `unit_instance ⇄ necessary_skill`
4. `necessary_skill → db → unit_instance → necessary_skill`
5. `data ⇄ necessary_skill`（`data/unit.rs:542` が `split_necessary` を使う）
6. `db → event_runtime`（`round_half_to_even` を使うためだけ。db.rs:679）

根本原因は **`GameDatabase` が静的データの集約ではないこと**。
`unit_instances`、`pilot_instances`、`map`、`pos_index` といった実行時の盤面を抱えている。
さらに盤面を使う問い合わせ（`effective_*`、`combat_bonuses`、`unit_move_range_from`）も持っているので、上位の層を引き込んでいる。
db.rs:21 の doc と CLAUDE.md の「静的データ集約」は実態と合っていない。

目標とするレイヤ構成:

```
L0 util       : time_util, 数値丸め, SJIS/全角/VB 書式, settings
L1 data/      : パーサと静的レコード (split_necessary 等のパース補助を含む)
L2 値型       : condition, feature, item_slot, unit_weapon, unit_ability,
                pilot_instance, unit_instance(状態のみ), turn, stage, flow, dialog, modal
L3 純ルール   : combat (マップ攻撃形状を含む), movement, spirit/ability 効果
L4 world      : GameDatabase (将来は静的 Catalog と実行時 Board に分割)
L5 orchestration : app (戦闘の進行, AI, 入力), event_runtime (スクリプト), scenario ingest
L6 presentation : scene/* (レイアウト, ヒットテスト, ビューモデル) → src-web は描画だけ
```

小さな移動 4 つで、循環は 1 番の `app ⇄ event_runtime` だけになる。
- **(a)** unit_instance のデッド戦闘コードを削除する。循環 2・3・4 が消える。
- **(b)** `round_half_to_even` を util へ移す。循環 6 が消える。
- **(c)** `split_necessary`、`split_feature_necessary`、`paren_inner` を data/ へ移す。循環 5 が消える。
- **(d)** 未使用の `data::pilot::parse_with` を削除する。`data → settings` の依存が消える。

## 4. 配置の問題（モジュール単位）

### app.rs — 12 クラスタが入り混じっている
| クラスタ | 行 | 規模 | 抽出のしやすさ |
|---|---|---|---|
| スクリプト状態のアクセサとデバッグ | 663–1853 | 105 fn | `ScriptRuntime` サブ構造体にできる。機械的だが、触る行数は多い |
| `.eve` 用 VFS | 1854–2062 | 19 fn、フィールド 3 | **容易**（`Vfs` 構造体へ） |
| ステージ／シナリオ進行、セーブ | 2063–2815 | 25 fn | 一部は散在している（app.rs:5287, 10220, 11541） |
| 入力、メニュー | 2984–3367, 9815–9950, 10291–11048 | 約 1,280 行 | 3 か所に散在 |
| 戦闘の進行 | 3368–5286 | 1,919 行 | 1-1 の統合が前提 |
| 戦闘ルール（特殊効果、報酬、経験値） | 7204–8231 | 1,028 行 | `Rng` を切り出せば純関数にできる |
| フェイズ、AI | 5287–6264 | 978 行 | `app/ai.rs` に分割できる |
| 反撃、武器選択のモーダル | 6265–6834 | 570 行 | |
| 精神コマンド | 6835–7203, 8232–8317 | 455 行 | |
| ユニットコマンド（変形、合体、発進など） | 8318–9807 | 1,490 行 | |
| ステータス詳細のビューモデル | 9971 | 195 行 | **容易**（scene/unit_detail.rs へ） |
| インターミッション | 11049–11652 | 604 行 | `IntermissionState` 込みで分割できる |

- **フェイズ開始時の処理**（毒、死の宣告、EN 回復など。app.rs:2208–2395）は `database` にしか触らない純粋なロジックなので、`GameDatabase` のメソッドか rules 層に移せる。
- **抽出時の注意**
  - `to_save_json` は App 全体をシリアライズする。永続化されるフィールドをサブ構造体へ移すときは `#[serde(flatten)]` を付ける。
  - RNG の呼び出し順は保存する必要がある（app.rs:5004 のコメント）。

### event_runtime.rs — インタプリタにドメインロジックが混在している
- **コマンド**（196 アーム）の内訳: ユニット操作 46 / 描画 42 / 制御 22 / 会話 15 / 進行 17 / 戦闘・精神 14 / マップ 14 / 変数 11 / ファイル 9 / サウンド 6
- **関数**（134 アーム）の内訳: ゲーム状態の問い合わせ 54 / 文字列 26 / 数学 23 / その他
- **App を直接操作している箇所**: `.unit_instances` に直接 140 回（うち書き込み 64 回）、`matches_unit_handle` のインライン検索 75 回。
  - `GameDatabase` のフィールドが全部 `pub` なので、`pos_index` のような不変条件を守らせられない（→ B2）。
- **純粋なユーティリティ**（SJIS 文字列関数、全角カナ、VB の数値書式、`round_half_to_even`、`like_match` など。約 450 行）は App に依存していない。

### src-web — エンジンのフローが wasm 側にある
- **`archive.rs::load_into_app`** (archive.rs:37–470) が以下をすべて担っている。どれもネイティブではテストできない。
  - データファイル名による振り分け
  - `.eve` の 2 パス実行
  - エントリポイントの選定
  - `advance_to_next_stage` のループ
  - MapView シーンへの強制遷移
- **アーカイブ展開とデータの振り分けがコピーされている**
  - `unzip`: 4 重（archive.rs:521、verify-archive main.rs:949、bin/extract_text.rs:82、tests/super_hero_smoke.rs:26）
  - `unlzh`: 3 重。verify-archive 版にだけ `is_decoder_supported` の分岐があり、すでに食い違っている。
  - データファイル → `extend_*` の振り分け: 約 9 か所（app.rs:1929、verify-archive、oracle_*、tests/*）
- **render.rs のルール判定**: 戦闘予測 (2821)、武器の無効判定 (2740)、戦闘 HUD のビューモデル (1158)。

### デッド／形だけのモジュール
- `ui/`（IGUI 系トレイト）: 実装も使用箇所もない。同期・ブロッキング型の API なので、中断・再開する現行の設計とも合わない。
- `Scene::render_reads()` / `overlays()`: scene/mod.rs の外では読まれていない。
- `combat::predict_with_defense` / `DefenseMode` / `default_damage_boost_level`: テストからしか使われない。
- `test_harness.rs`: `pub mod` だが本番での使用はない。7 個の統合テスト専用なので、`tests/common/` へ移せる。

### 命名と配置
- `command_catalog`（`.eve` コマンドのレジストリ）と `command_menu`（ユニット／マップの UI メニュー）は名前が似ているが無関係。
  - 前者はスクリプト層の `script::catalog` 相当。
  - 前者は実際の dispatcher の `match` と別管理なので、両者がずれていく恐れがある。
- `dialog.rs` に、スクリプトの中断状態 `PendingDialog` と、反撃・武器選択ウィンドウのジオメトリが同居している。ジオメトリは scene/ に寄せる。
- `Party::color()` が CSS の hex を返す (unit_instance.rs)。プレゼンテーション層の関心がコアに漏れている（軽微）。

## 5. 推奨ロードマップ（リスクの低い順）

各段階は振る舞いを変えないコミット単位に分けられる。B 系のバグ修正は別コミットにする。

**フェーズ 1: 削除と移動だけ（リスク極小）**
1. デッドコードを削除する: unit_instance の旧戦闘・武器判定系（約 850 行）、`combat::MapAttackShape` 一式と `predict_with_defense`、`ui/`、`data::pilot::parse_with`。
   `necessary_skill_gate.rs` は App の `weapon_skill_ok` 経由のテストに書き換える。
2. `util/` を新設する（`round_half_to_even`、SJIS、全角、VB 書式、`like_match`、全角 ASCII の畳み込みを統合）。
   `split_*necessary` は data/ へ移す。→ app⇄event_runtime 以外の循環がすべて消える。
3. テストを外出しする:
   - `app.rs` → `app/mod.rs` にし、`app/tests/{combat,ai,intermission,ui_input,stage_flow,unit_commands}.rs` + `support.rs` を作る。子モジュールなので private な項目もそのまま見える。
   - `event_runtime.rs` も同様に分ける。
   - 約 16k 行が本体から抜ける。
4. CLAUDE.md の記述ずれを直す（§6）。

**フェーズ 2: 機械的なファイル分割（振る舞いは変えない）**
5. `impl App` を `app/{vfs,ai,intermission,input,unit_commands,spirit,combat_flow,debug}.rs` に分ける。VFS は `Vfs` 構造体にする。
6. `event_runtime/` を `{library, vars, labels, expression, commands/*, functions/*, blocks}` に分割する。`exec_command_pc` はカテゴリ別の関数に委譲する薄い `match` にする。
7. `build_status_detail`、`reaction_window_data`、`weapon_select_window_data` を scene/ へ移す。dialog.rs と command_menu.rs のジオメトリも scene/ に寄せる。

**フェーズ 3: 経路の一本化（バグ修正を伴う。差分テストを先に書く）**
8. **B1〜B3 を修正する**:
   - ユニットの削除をすべて `remove_unit*` 経由にする。
   - Recover と Damage を `effective_max_*` 基準にする。
   - 反撃で撃破したときに破壊ラベルを発火する。
9. **`.eve` コマンドを App のドメイン経路に寄せる**:
   - Transform → `transform_unit_by_uid`
   - SpecialPower → `apply_spirit_to_target`
   - UseAbility → `apply_ability_effects`
   - Attack → App の戦闘解決
   - `map_attack*` → combat.rs
10. **App 内の戦闘を一本化する**: 戦闘入力のビルダーを 1 つにし（6 か所を置き換え。src-web の予測も含む）、`resolve_strike` を本線・援護・反撃で共有する。RNG の呼び出し順に注意し、C# oracle とサンプルシナリオで検証する。
11. **シナリオ取り込みを一本化する**:
    - src-core に `ingest_entries` / `boot_scenario` を置き、アーカイブ展開は共有モジュール（または feature）にまとめる。
    - src-web はアセットの登録と描画だけにする。

**フェーズ 4: 長期**
12. **式評価器を統一する**: `Value {Num, Str}` を持つ単一の AST 評価器と、単一の `resolve_atom` にする。既存のシナリオ互換性（未定義 → false、0 埋め など）に依存しているので、oracle で守る。
13. **`GameDatabase` を分割する**: 静的な `Catalog` と実行時の `Board` に分け、フィールドを `pub(crate)` + アクセサにして不変条件を守らせる。`#[serde(flatten)]` でセーブ互換を保つ。

## 6. CLAUDE.md の記述ずれ（確認済）
- src-web は Yew を使っていない（素の wasm-bindgen + web-sys）。
- `src-core::render::draw_scene()` は存在しない（描画は src-web/render.rs:41）。
- `expression/` モジュールは存在しない（event_runtime.rs に内包されている）。
- 「no_std 互換」「時刻は `instant` クレート」は実態と違う（std を使っており、instant には依存していない）。
- `db.rs` は「静的データ集約」ではない（実行時の盤面を含む）。
- 「ui/ を src-web が実装」は誤り（どこにも実装がない）。
- モジュール表に flow, modal, necessary_skill, command_catalog, entrypoint, test_harness, battle_anim, script_overlay が載っていない。
