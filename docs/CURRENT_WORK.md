# 現在の作業状況 (Session Handoff)

VB6 製 SRC (Simulation RPG Construction) を Rust + WebAssembly に移植中。
本ドキュメントは作業継続のための要約。**解決済み課題は §9 に 1 行で要約**し、本文は
「現状・残課題・恒久リファレンス」に絞る。各課題の commit ハッシュ・実装詳細は memory
`project_gap_audit_roadmap`（穴埋めロードマップ）/ `project_spirit_commands_status`（精神コマンド）に集約。

---

## 現在地（2026-06-16）

**テスト**: `cargo test -p src-core` 全緑（**1872 件**）／ clippy clean（`-D warnings`）／ wasm `cargo check` OK。  
**ブランチ／コミット**: 直近は **`feat/necessary-skill-gate`** で作業中（必要技能ゲート、2 コミット、未 push）。
以前のセッションは `master` 上で `origin/master`（`0de48d9`）から数コミット先行。push はユーザの明示指示で行う（no-auto-push）。

**到達点**: 監査で洗い出した SRC ゲームプレイの大穴を**ほぼ全て実装済**。具体的には — 戦闘実効値（改造/強化パーツ/育成/状態異常を
反映）・撃破報酬（資金＋育成）・気力経済（撃墜/被弾/性格別変動/技能加速）・武器の EN/残弾消費＋資源を尊重した自動選択・
特殊効果攻撃属性（CC 属性ほぼ全種）・防御特性ファミリ（耐性/弱点/有効/吸収/無効化）・パイロット戦闘技能（底力/超底力/潜在力開放/
得意技/ハンター/見切り/超反応/超能力 等）・BossRank サブシステム（即死/死の宣告）・インターミッション経済ループ（機体改造/換装/
乗り換え/ステータス/データセーブ）・プレイヤー向けユニットコマンド（修理/補給/変形/チャージ/合体/分離/母艦発進）・アビリティ
proper（パーサ＋操作＋効果）・敵 AI 戦術判断（攻撃補助/回復/補給/召喚/敵対象アビリティ・回復精神・マップ兵器・防御地形選好・復活
pre-buff・散開・ChangeMode 逃亡/護衛）。**残るのは外部依存・大規模・設計判断・検証制約のある項目のみ**（§1）。

**方針メモ**: エンジンは SRC_20121125 (VB6) / SRC.Sharp (C#) の**忠実移植**であるべき。原典に定義の無いシナリオ独自要素
（例: 東方夢想伝の精神 決意/気迫/希望）は**推測実装せず**、シナリオ側（sp.txt 効果種別/イベント）の責務とする。

**直近セッション（2026-06-15/16）の実装**（詳細は §9）:
- **A2 合体/分離の精緻化**: 3 機ルール・全パートナー必須・初期合体形態の分離。
- **§C 敵 AI 戦術判断（完了）**: 防御地形選好・復活精神の能動 pre-buff・マップ兵器脅威下の散開。
- **パイロット技能**: ハンター（対象別ダメージ）・見切り（切り払い必中）を C# で書式確定し実装。
- **方針対応**: シナリオ独自精神 決意/気迫/希望 の推測実装を除去（原典に定義無しを grep 確認）。
- **前半**: 気力経済・武器 EN/弾消費・特殊効果攻撃属性ほぼ全種・防御特性・BossRank・底力系/与ダメージ系パイロット技能。

---

## 1. ★ 残課題サマリ（次セッション引き継ぎ）

> ゲームプレイ機能はほぼ網羅済み。残りは「外部リソース・設計判断が要る大課題」と「検証制約のある精緻化」に分かれる。

### 1.1 外部リソース・設計判断が要る（大）

- **D スパロボ戦記「敵出撃」**（最優先の大課題・§2 に診断メモ）: 進行不能の核（敵が出撃しない）。`式中ユーザ定義関数の実行`
  （`Call(ランク算出,…)`）／`Info() サブクエリ網羅`／`キャラメイキング搭載` の 3 土台機能に帰着。**84MB 実機ファイルでの
  `__srcVar` 切り分けが必須**。詳細は §2・§4「戦記-*」。
- ✅ **必要技能ゲート（2026-06-16 実装、`feat/necessary-skill-gate`）**: `(念力Lv3)` 形式の括弧条件で武器/アビリティの
  使用可否を制限。`necessary_skill` モジュール（`split_necessary`＋`is_satisfied`、AND-of-OR＋`!`/`*`/`+`）を新設し、
  `is_weapon_available`／`weapon_firable`（ライブ AI/反撃/援護）／`pick_attack_weapon` 強制分岐／`ability_usable` に配線。
  撃墜数Lv*（戦記の主要ゲート）が機能（エース解禁/ザコ封印）。**前提バグも同時修正**: pilot.txt 特殊能力のカンマ形式
  （`撃墜数Lv100, 1` 等）を取りこぼしていた parser を修正（撃墜数/底力/切り払い が実データで有効化）。未モデル種別
  （ステータス閾値・同調率・霊力・生身）は fail-open で誤封印回避。✅ **動的化も完了（2026-06-16）**: ① 撃墜数の*戦闘中加算*
  （`award_kill_rewards` で撃破者主パイロット +1）→ 規定数撃墜で武器解禁。② **習（ラーニング技）**（クリティカルで対象の
  `ラーニング可能技` を主パイロットが習得→ゲートが読む。攻撃側使用時に機能、反撃クリ未対応）。✅ **形態ゲート（§4）も完了**:
  `UnitData` の `必要技能=`/`不必要技能=` を **変形/換装/乗り換え**に配線（`form_skill_ok`、乗り換えは swap→判定→不可なら revert）。
  **残（軽微・意図的）**: ① 必要技能未達武器のステータス画面**非表示**（機能ゲートは完了、表示フィルタは frontend 側未）。
  ② 格闘Lv200 等の**ステータス閾値は fail-open 据置**（誤封印回避を優先＝モデル化しない決定。fail-open は無ゲート従来挙動と等価で無害）。
  ③ §3 ユニット用特殊能力の自己ゲート・合体/分離の形態ゲートは未（ニッチ）。§5 アイテムは Equip がバイパス・交換 UI 無しで適用点なし。
- **魅了/憑依**: 勢力/支配の切替（魅了=魅了主を護衛する味方のように行動 / 憑依=完全支配）。勝敗・ターン・AI・save と広く干渉。
  §4 で意図的非対応としてきた設計判断。再検討はユーザの決定要。
- **E GBA クローズアップ戦闘アニメ**: 専用バトルスプライト＋固定レイアウトの段階移植。dict 変数／`_GBA_*`／`Redraw` clear 等。
  **複数セッション規模・要実シナリオ検証**（§4「GBA」）。

### 1.2 検証制約・MVP 拡張（小〜中）

- **A2 着地点選択**: 分離/発進ユニットの着地を移動範囲から対話選択（原典）。現状は隣接自動。**対話的 UI でヘッドレス検証不可**。
  併せて: 初期合体形態の分離後の再合体トラッキング・分離パイロットの主/副の細分配分（現状は形態 1 へ集約）。
- **B インターミッション**: ステータスの単機詳細化（現状はロスター閲覧の MVP、UI 拡張・frontend）。
  ✅ **乗り換えの Option ゲーティングは解消（2026-06-16）**: `Option` コマンドは実装済だった（`Option(name)` 変数）ため、
  乗り換え表示を `Option(乗り換え)` 有効 AND 2 機以上 に修正（原典準拠、`乗り換え.md`）。
- **C 敵 AI のさらなる戦術判断**: 防御地形選好以上の高度な陣形最適化等。穴ではなく深掘り余地・balance 判断要・大規模。
- **演出**: エフェクトセットの見栄え調整・属性別 `EFFECT_` 選択の最適化・移動経路アニメの滑らかさ。小。

> **横断的な教訓（memory にも記録）**:
> 1. 「形式不確定/未確定」とされた技能は **C# (`Unit.cs`) を読めば仕様が確定**できる（ハンター=`Damage` / 見切り=`CheckParryFeature`
>    はこれで実装できた）。docs 未定義を理由に諦めない。
> 2. **原典に定義の無いシナリオ独自要素は推測実装しない**（シナリオ側 sp.txt/イベントの責務）。推測実装は移植の偽装になる。
> 3. 「実行時資源を尊重する完全版メソッドがあるのにライブ経路は射程・威力だけの簡易版を使う」**二重実装の配線漏れ**に注意
>    （気力経済・武器 EN/弾消費がその典型だった。`execute_attack`/`best_available_weapon` 等の未使用完全版と app.rs ライブ経路を差分する）。
> 4. **地形による HP/EN 回復・EN/SP の毎ターン自動回復は原典に無く、実装しないのが正解**（`Unit.cs` でコメントアウト／HLP000182 で
>    回復率は地形ステータス／SP は per-battle プール）。"穴" と誤認して実装しないこと。母艦格納ユニットの +50% 回復のみ有効。

---

## 2. スパロボ戦記「敵出撃」診断メモ（D の最優先課題）

現状: 「次のステージへ」で **T1 味方フェーズに到達し味方 1 体が表示される**ところまで来た（進行不能の核は解消）。残る進行不能は
**敵が出撃しない**（勝敗がつかない）こと。

- **症状**: `(no stage)`（マップ名空）／敵ユニット 0／最初に選んだ機体（メイン/サブ決定機ではない）が出る。
- **敵配置のコード**: `スパロボ戦記/eve/Main.eve` `敵配置:`（~L1363）/ `ボス配置:`（~L1413）。
  ```
  For i = 1 To Args(11)                                  ← Args(11) = 敵配置数（カウント）
    set 敵候補確定 Lindex(敵候補, Random(Llength(敵候補)))  ← 敵候補リストから抽選
    Create 敵 敵候補確定 Call(ランク算出,敵候補確定,700) 敵パイロット (味方平均レベル…) (LIndex(配置場所…))
  ```
  呼び出しは `スタート` 内 `Call 敵配置 配置場所[7] … 敵配置数`（Main.eve ~L526）。
- **詰まりの仮説**（要 `__srcVar` で確定）:
  1. `敵配置数`（=Args(11)）が空/0 → `For 1 To 0` で 0 体。
  2. `敵候補` リストが空（Llength=0）→ `Lindex(敵候補, Random(0))` が空 → `Create 敵 ""` で生成されない。
     `敵候補` は `特殊増援候補作成`（Main.eve L462〜520）が構築。
  3. `Call(ランク算出, …)` の**式中ユーザ関数呼び出し**が Create 引数で未評価（ただし Create のランク引数は実際には無視される）。
  4. `味方平均レベル` / `Info(マップ,幅/高さ)` / `配置場所[…]` が空 → 座標/カウントが壊れる。
- **診断手順**: 壊れた戦闘で `__srcVar("敵配置数")` `__srcVar("敵候補")` `__srcVar("配置場所[7]")` `__srcVar("味方平均レベル")` を確認。
  空のものが根本。`Info()` 由来か `特殊増援候補作成` 由来かユーザ関数（`Call(...)`/`ランク算出`）由来かを切り分け、該当の
  Info サブクエリ実装 or 式中ユーザ関数実行（§4「戦記-*」）を進める。
- **関連（同根）**: マップ名空＝`Call SubTitle "…$(Lindex(Info(ユニットデータ,…,マップ決定),1))"` の Info 空。メイン/サブ選択が
  反映されない＝キャラメイキング搭載/選択ロジック。いずれも「`Info()` 網羅・式中ユーザ関数実行・キャラメイキング」に帰着。
- **メモ**: 式中ユーザ関数の機構は `call_label_sync_for_condition`+`enter_call_args` で実証済だが、再入＋実シナリオ検証が要り
  盲目実装は interpreter 全体への回帰リスク大。敵未出撃の核は Info 由来の可能性が高く関数実行単独では解けない見込み。
- **ブラウザロード手順**（`just serve` 中・実ブラウザ可視タブ）:
  ```javascript
  const res = await fetch('/sparobosenki.zip');
  const file = new File([await res.arrayBuffer()], 'スパロボ戦記.zip', { type: 'application/zip' });
  const dt = new DataTransfer(); dt.items.add(file);
  const input = document.getElementById('src-file');
  input.files = dt.files; input.dispatchEvent(new Event('change', { bubbles: true }));
  // 60〜90 秒で完了 → window.__srcDebug()
  ```

---

## 3. 設計の要点（コードを触る前に把握すべき箇所）

- **ユニット識別は uid**: `GameDatabase.pos_index: BTreeMap<(u32,u32),uid>`（serde skip、load 後 `rebuild_pos_index`）が
  「どのマスに誰が居るか」の単一の真実源。座標変更は必ず `move_unit`/`remove_unit`/`set_off_map` 経由。`unit_instances` への
  直接 `.x=/.y=/.push/.remove` は禁止（db.rs 内のみ）。
- **フェイズ/ターン**: `turn.rs` の `Phase`＝Player/Enemy/Neutral/Npc。`Turn::end_phase()`（Npc→Player で +1）。ターンイベント
  発火は `app.rs::begin_phase`。
- **敵味方関係**: `Party::is_hostile_to`/`is_ally_of`（unit_instance.rs、内部 `camp()`）。{味方,ＮＰＣ}/{敵}/{中立}、異キャンプ＝
  敵対。combat/AI 標的/援護/反撃/マップ攻撃が全てこれ経由。
- **逐次 AI**: `App.animate_ai`（フロントが起動・シナリオ読込時に true）。`end_phase` は `ai_runner` を起動し
  `tick`→`ai_runner_tick` が 1 体ずつ進める。`animate_ai=false`（テスト/ヘッドレス）は同期一括処理（全テスト互換）。演出再生中
  （`battle_anim`/`move_anim`）はランナー待機。
- **反撃モード**: `ai_act_unit` が攻撃直前に対象を先読み、味方かつ手動なら `begin_reaction_prompt`→`PendingDialog::Menu`→
  `resolve_reaction`→`attack_resolve_and_run(def_mode)`。`def_mode`（"反撃"/"回避"/"防御"/"援護防御"/""）の補正は同関数内。
- **戦闘予測 `combat::predict_with_status_terrain`**: 命中・ダメージ・クリティカル率を一括算出。地形適応・状態異常・パイロット
  技能（潜在力開放/得意技/ハンター 等）・防御特性をここで反映。全戦闘サイト（通常/反撃/援護/マップ攻撃）が同関数を使う。
- **戦闘演出**: `battle_anim`/`move_anim` は共に `#[serde(skip)]` transient。フロントが読んで描画。`tick` が move→battle の順で進める。
- **数値引数の式評価**: 座標等は `eval_coord_u32`（→`eval_int_expr_app`→`resolve_expr_atoms`）。裸のループ変数・script_var・
  システム変数（味方数/レベル平均値/ターン数 等）を解決。直書き数値専用の `parse_u32`/`parse_i32_at` とは使い分け。
- **実効戦闘データ `db::effective_combat_data(idx) -> (PilotData, UnitData)`**: レベル成長＋強化パーツ＋改造（`upgrade_level`）＋
  技能/特殊能力/状態異常ボーナス（`combat_bonuses`）を合成。生の静的データではなく必ずこれを戦闘へ渡す（改造/育成/デバフが効く要石）。

---

## 4. 残・後続課題テーブル

| # | 課題 | 状況 |
|---|------|------|
| 戦記-敵 | **スパロボ戦記「敵出撃」** | **最優先**（§2）。`Main.eve 敵配置/ボス配置` が `敵候補`/`敵配置数`/`Call(ランク算出)`/`Info` に依存。空のため敵 0→進行不能。要 `__srcVar` 切り分け |
| 戦記-関数 | **式中ユーザ定義関数の実行** | `Call(ランク算出,…)` 等、ラベルを式の中で実行して `Return` 値を使う基盤。`fn_arg_value` は `&App`（読み取り専用）でサブルーチンを実行できず生式が漏れる。`enter_call_args`（`&mut App`）経由で `Label(args)` を実行する設計が要る。再入注意 |
| 戦記-Info | **`Info()` サブクエリ網羅** | `Info(パイロット,…,累積経験値/性格/性別/最大ＳＰ/特殊能力所有,成長タイプX)` / `Info(ユニットデータ,…,マップ決定/全身画像)` 等、AlphaSecond ステータス・マップ名・敵候補が使うクエリが未対応で空。`event_runtime.rs` の `"Info"` 周辺に追加 |
| 戦記-CM | **キャラメイキング搭載/選択** | 作ったパイロットがメイン/サブとして機体に乗らず、最初に選んだ機体が出る。`lib/CMaking.eve`（1608 行）＋ `仮ユニット`→実機体の搭載/乗り換え。深い |
| 必要技能 | ✅ **必要技能ゲート＋動的化（2026-06-16 完了）** | `(念力Lv3)` を `necessary_skill` で評価し武器/アビリティ可否ゲートに配線。撃墜数の戦闘中加算・習（ラーニング）も実装済で動的に武器解禁。残（軽微）: 未達武器の画面非表示（frontend）・ステータス閾値は fail-open 据置（§1.1） |
| 魅了/憑依 | **勢力/支配の切替** | 魅了=魅了主を護衛する味方のように行動 / 憑依=完全支配。勝敗/ターン/AI/save と広く干渉。意図的非対応としてきた設計判断（再検討はユーザ要） |
| GBA | **GBA クローズアップ戦闘アニメ移植** | 専用バトルスプライト＋固定レイアウト。`BaseX/BaseY=0` 固定画面に `_GBA_GetUnitBmpFile(UID,…)` でユニット個別スプライトを描く。dict 変数（`戦闘アニメ変数[…]`/`_GBA[…]`）＋数十の `_GBA_*`＋`Redraw`/`Keep` の画面クリア意味論依存。**複数セッション規模・要実シナリオ検証** |
| 演出 | エフェクトセットの見栄え調整・属性別 `EFFECT_` 選択の最適化。移動経路アニメは実装済だが滑らかさ向上余地 | 小 |
| AI | **NPC/中立 AI の優先度分離** | 標的は `is_hostile_to` で正しく分離。優先度ロジックは敵と共通。SetRelation/友好度上書きは SRC 準拠で**意図的に非対応**。明確な差別化ルールが見当たらず実装余地は限定的 |
| 手動 | スパロボ戦記の乗せ換え→戦闘通し目視 | 84MB ロードが必要な手動タスク（自動化対象外） |

### 恒久的な制約（仕様・運用メモ）

- **プレビューの RAF スロットリング**: Claude Preview は offscreen で `requestAnimationFrame` が間引かれ逐次 AI が自動進行しない。
  検証時は `window.__srcTick(0.5)` で手動駆動。実ブラウザ（可視タブ）では自動進行する。canvas の `getBoundingClientRect()` が
  **0×0**（offscreen）→ 合成クリックは座標が壊れ無効。クリック系（選択肢/ユニット移動）はプレビューでは検証不可、ロジックは
  ユニットテストで担保する。
- **セーブ互換は破棄**: uid 再設計でセーブ形式が変化（方針: 互換不問）。`pos_index` は serde skip で load 後再構築。
- **素材パックは各自取得**: `crates/src-web/vendor-assets/` に `SRC_Graph101121.zip` / `SRC_BA110418.zip` / `SRC_Wave091207.zip`
  を配置すると起動時自動読込。再配布規約のためリポジトリ非同梱（.gitignore、`.gitkeep` のみ追跡）。検証用シナリオ（musou202.lzh /
  スパロボ戦記.zip）も各自取得。

---

## 5. 開発環境

| 項目 | 内容 |
| --- | --- |
| Rust | rustc 1.95.0 (Nix Flake)、ターゲット `wasm32-unknown-unknown` 固定 |
| ビルド/実行 | `nix --extra-experimental-features 'nix-command flakes' develop --command <cmd>` |
| 型チェック | `just check`（= `cargo check --workspace --target wasm32-unknown-unknown`） |
| テスト | `just test` / `cargo test -p src-core`（ネイティブのみ。WASM テスト不可） |
| Lint | `just lint`（clippy `-D warnings`） |
| fmt | `just fmt`（= `cargo fmt --all`）— **コミット前必須** |
| 開発サーバ | `just serve`（trunk serve port 8080） |
| 単一 .eve 実行 | `cargo run -p verify-archive --bin run_eve -- <path.eve>` |
| アーカイブ smoke | `VERIFY_SMOKE=1 cargo run -p verify-archive -- <path.zip>` |
| 未登録命令 | `extract_text <outdir> <archives...>` → `scan_eve <outdir>` |

> verify-archive 系はネイティブビルド（`cargo build -p verify-archive --bins`）。ドライブモード（`VERIFY_DRIVE=1`
> `VERIFY_ANIMATE=1` `VERIFY_ASK=<n>`）で**ブラウザを開かずに進行不能/勝敗/ダメージを切り分け**られる（`tools/verify-archive/src/main.rs`）。

---

## 6. アーキテクチャ

```
crates/
├── src-core/                  ← 純 Rust ロジック (no_std 互換)
│   src/
│   ├── app.rs                 ← App: シーン遷移 / 戦闘解決 / tick / AI ランナー / 演出状態
│   ├── battle_anim.rs         ← BattleAnim(攻撃演出) / MoveAnim(移動スライド) / AttackKind
│   ├── command_catalog.rs     ← 全コマンドの SoT（カタログ）
│   ├── combat.rs              ← 戦闘予測 / 命中・ダメージ・クリティカル率
│   ├── movement.rs            ← Dijkstra 移動範囲 / reconstruct_path
│   ├── db.rs                  ← GameDatabase（pilots/units/instances/items/terrains/maps/animation, pos_index）
│   ├── data/                  ← .eve / pilot / unit / item / sp / terrain / map / animation パーサ
│   ├── event_runtime.rs       ← .eve インタプリタ（最大ファイル。式評価・system 変数もここ）
│   ├── unit_instance.rs       ← UnitInstance（HP/EN/Morale/Items/Conditions/has_acted）
│   └── scene/                 ← Title/Configuration/MapView/PilotList/UnitList
└── src-web/                   ← wasm-bindgen + Canvas2D フロントエンド
    └── src/
        ├── archive.rs         ← zip/lzh 展開 + データロード（warn+skip 堅牢化）
        ├── render.rs          ← draw_scene / draw_map_view / draw_battle_anim
        └── lib.rs             ← RAF ループ / 入力 / アセットパック自動読込
tools/verify-archive/          ← CLI 検証（main=smoke, bin/ に scan_eve/extract_text/run_eve/coverage）
docs/                          ← CURRENT_WORK.md（本書）/ ARCHIVE_SCAN_REPORT.md / SRC_SHARP_DIVERGENCE.md / FLOW_REDESIGN.md
```

---

## 7. 実装済み機能サマリ

- **シーン進行**: Title → Configuration →（Intermission）→ MapView（Briefing→Sortie→Battle→Victory/Defeat）↔ PilotList/UnitList。
  進行制御は完了プロトコル＋FlowCont 継続スタック＋割込みイベントキュー（`post_event_label`）。`Continue` チェインはエンジン内
  チェイン化。詳細は `docs/FLOW_REDESIGN.md`。
- **`.eve` インタプリタ**: 制御フロー（Goto/If/For/ForEach/Switch/Do/Loop/Break/Continue/Call/Return/Exit）、変数（Set/Local/
  Unset/Incr/`$(name)`/`Args(N)`/`name[expr]`/`&`連結）、対話（Talk/Confirm/Menu/Ask/Input/Wait Click）、ユニット（Create/Place/
  Launch/Escape/Kill/Transform/Combine/Split/Join/Ride/Leave/ChangeParty）、育成・アイテム・精神・待機・ステージ・データ宣言・
  VFS ファイル I/O。
- **データパーサ**: pilot/unit/item/sp/terrain/.map/.eve/animation。全角コンマ正規化・バッククオート・行末コンマ耐性・レコード
  単位の寛容パース（warn+skip）。
- **戦闘**: 命中/ダメージ/クリティカル/回避/防御/反撃/援護攻撃/援護防御/行動不能ゲート。地形適応・状態異常・特殊効果攻撃属性・
  防御特性・パイロット技能・気力経済・武器の EN/残弾消費を反映。
- **インターミッション**: 機体改造/換装/乗り換え/ステータス/データセーブ。撃破報酬で「撃破→資金→改造→戦闘反映」の経済ループが閉じている。
- **演出**: ネイティブ戦闘演出（フラッシュ/ダメージ/lunge）＋ SRC_BA エフェクトスプライト、AI 移動スライド。`animation.txt`
  戦闘アニメ実行配線（同梱シナリオ時）。GBA クローズアップは未対応（§4）。
- **アーカイブ互換性**: 全 3 ディレクトリ計 1496 本を smoke スキャン済、クラッシュ/ロード中断 0。scan_eve で未実装エンジンコマンド無しを確認。

---

## 8. デバッグ / 動作確認の小技

```bash
# 単一 .eve 実行
cargo run -p verify-archive --bin run_eve -- <path.eve>
# アーカイブ smoke / ファイル内容ダンプ
VERIFY_SMOKE=1 target/debug/verify-archive <path.zip>
VERIFY_DUMP_PATH=ファイル名.eve target/debug/verify-archive <path.zip>
# シナリオ自動プレイ駆動（ブラウザ不要で進行不能/勝敗/ダメージを切り分け）
VERIFY_SMOKE=1 VERIFY_DRIVE=1 VERIFY_ANIMATE=1 VERIFY_ASK=1 cargo run -q -p verify-archive --bin verify-archive -- <archive>
# 未登録コマンド洗い出し
target/debug/extract_text /tmp/out archive/SRCシナリオ_10K～99K/*
target/debug/scan_eve /tmp/out
```

ブラウザ（`just serve` 中）コンソール: `window.__srcDebug()`（App 状態サマリ。`parties=[味 敵 中 Ｎ]` / `file` /
`victory[全滅敵 全滅中立 クリア]` を含む）／ `window.__srcVar("name")`（.eve 変数）／ `window.__srcImg()`（画像解決ダンプ）／
`window.__srcTick(0.5)`（RAF スロットリング回避の手動駆動）。

---

## 9. 完了済みマイルストーン（履歴・要約）

詳細実装はコード・`git log`・memory `project_gap_audit_roadmap` を参照。以下は「もう触らなくてよい」既消化項目の索引。

### 2026-06-16 セッション（`feat/necessary-skill-gate`）

- **必要技能/必要条件ゲート**: `(念力Lv3)` 形式の括弧条件を `necessary_skill` モジュール（`split_necessary`＋`is_satisfied`、
  AND-of-OR＋`!`/`*`/`+`、SRC.Sharp `IsNecessarySkillSatisfied(2)`／`必要技能.md` 準拠）で評価。`is_weapon_available`／
  `weapon_firable`（ライブ AI/反撃/援護）／`pick_attack_weapon` 強制分岐／`ability_usable` に配線。撃墜数Lv*・気力・瀕死・
  HP/EN・ランク・性別・レベル・パイロット技能（別名/オーラ加算）・ユニット名/クラス・@地形・装備・隣接・状態 を判定、
  未モデル種別（ステータス閾値・同調率・霊力・生身）は fail-open。**前提バグ修正**: pilot.txt 特殊能力のカンマ形式
  （`撃墜数Lv100, 1` 等）取りこぼしを `parse_feature_line` で解消（撃墜数/底力/切り払い が実データで有効化）。テスト +18。
  残: 撃墜数の戦闘中加算・習（ラーニング、前提解消済）・未達武器の画面非表示・ステータス閾値モデル化（§1.1/§4）。
- **ゲートの動的化**: ① **撃墜数の戦闘中加算**（`award_kill_rewards` で撃破者主パイロット +1、`increment_kill_count`）→
  `(撃墜数Lv20)` 等が規定数撃墜で武器解禁。② **習（ラーニング技）**（`apply_weapon_learning`、クリティカル効果ブロックへ配線）→
  対象の `ラーニング可能技=<技>` を主パイロットが習得しゲート解禁。習属性は連結（`無習`）対応・既習得は再習得しない・反撃クリは
  未対応。テスト 2 件。**ステータス閾値条件（格闘Lv200 等）は誤封印回避を優先し fail-open 据置（モデル化しない決定）**。
- **形態の必要技能ゲート（§4）**: `UnitData` の `必要技能=`/`不必要技能=` を `form_skill_ok` で評価し **変形**（`resolve_transform`）・
  **換装**（`apply_equip_swap`）・**乗り換え**（`apply_ride_change`、swap→判定→不可なら revert）に配線。`.eve Transform` 等の
  シナリオ駆動は非ゲート。宣言の無い形態は no-op。テスト 2 件。合体/分離・§3 特殊能力自己ゲートは未（ニッチ）、§5 アイテムは適用点無し。
- **周辺の原典準拠化（同セッション）**: ① **乗り換えを Option コマンドで有効化**（`Option(乗り換え)` 有効 AND 2 機以上、`乗り換え.md`。
  Option は実装済だった）。② **Ｄ属性の気力吸収**（低下分の半分を攻撃側へ、`weapon_morale_absorbs`＋firer_idx）。
  ③ **マップ兵器の撃破で「復活」を尊重**（`revive_if_possible` を pub(crate) 化し map_attack へ、Tier0 残）。
  ④ **毒/死の宣告を実効最大HP基準**に（改造/強化パーツ/ボスランク反映、Tier0 残）。各テスト付き。

### 2026-06-15/16 セッション（master、`origin/master`=`0de48d9` 以降）

- **監査ベースの大穴埋め（Tier 0–2）**: 戦闘実効値（`effective_combat_data`）/ 撃破報酬（資金＋育成、幸運有効化）/
  インターミッション機体改造・データセーブ（経済ループ）/ 特殊効果攻撃属性（CC 属性付与の地固め）。
- **特殊効果攻撃属性をほぼ全種**: Ｓ縛痺眠乱凍石毒不止劣 / 低攻低運低移 / 盲撹 / 害ゾ黙狂 / 中踊 / 衰滅 / 吹Ｋ引転 / 脱Ｄ / 盗 /
  弱効剋 / 写化 / 恐。反撃・援護でも proc、proc が crit を置換。耐性/弱点（発動率＋毒率）。
- **防御特性ファミリ＋切り払い**（C# `Unit.cs::Damage`/`CriticalProbability`/`CheckParryFeature` 準拠）: 耐性÷2/無効化0/吸収-1/2回復、
  発動率 弱点+10/耐性÷2、切り払い prob=2×防御Lv−攻撃Lv ＋直撃の無効化。
- **BossRank サブシステム**（`UnitInstance.boss_rank`＋`BossRank` コマンド＋ランク別ステータス強化）＋即死/死の宣告/ボス耐性。
- **パイロット戦闘技能**: 底力/超底力（命中回避 +30/+50）/ 超反応/超能力/底力（クリティカル）/ 潜在力開放（高気力で与ダメ×1.25）/
  得意技/不得手（武器属性別 ±20%）/ ハンター（対象別 ×(10+Lv)/10）/ 行動不能の絶対命中＋睡眠の被ダメ×1.5 / 見切り（切り払い必中）。
- **気力経済を完全実装**: 撃墜（撃破者+4・同陣営+1・機械不動）＋被撃破陣営の性格別変動（超強気+2/弱気-1）＋被弾+1 ＋
  命中時/損傷時/失敗時/回避時 気力増加スキル。→「戦って気力を上げ→精神/底力/潜在力開放が発動」する中核ループが機能。
- **武器の資源管理を完成**: ライブ戦闘が EN/弾数を一切消費せず無限だった大穴を解消（`consume_weapon_resources` を全攻撃の発射時に
  配線）＋武器の自動選択が残弾/EN/必要気力を尊重（`weapon_firable`/`best_firable_weapon_in_range`）。
- **多ユニット合体/母艦**: `stored_units`/`stored_in`/`combined_from`/`pre_combine_form`/`pre_combine_pilots`。母艦発進＋毎ターン回復 /
  合体・分離（状態・パイロット温存）/ 搭載・合体のムーブ統合 / 3 機ルール・全パートナー必須・初期合体形態の分離。
- **アビリティ proper**: パーサ（`===` 区切り→`AbilityData`）＋プレイヤー操作（`UnitMenuItem::Ability`＋射程対象選択）＋効果
  （回復/補給/気力/治癒/装填/再行動/状態/付加/変身/SP回復/召喚/強化/能力コピー/M型/敵対象）。
- **プレイヤー向けユニットコマンド**: 修理/補給/変形/チャージ。インターミッション 換装/乗り換え/ステータス。
- **回復系特殊能力**: ＨＰ回復/ＥＮ回復/ＨＰ消費/ＥＮ消費 Lv*（`begin_phase` で毎ターン 10×Lv%）。
- **敵 AI 戦術判断**: 攻撃補助精神 / 回復・補給・召喚・敵対象アビリティ / 回復精神（free action）/ マップ兵器 / ChangeMode 逃亡・護衛 /
  防御地形選好 / 復活精神の能動 pre-buff / マップ兵器脅威下の散開。
- **方針対応**: シナリオ独自精神 決意/気迫/希望 の推測実装を除去（原典に定義無しを確認、忠実移植方針）。

### それ以前のセッション

- **インターミッション制シナリオの進行修正＋UI**: 進行不能の核（`Continue` チェイン後の Briefing 停止）を解消、`Talk :/;`・
  `Wait Click` 右クリック/Esc 脱出（`KeyState(2)` ワンショット）・単一行 `If Goto` の EndIf 修正・括弧算術の欠落オペランド 0 化・
  顔グラ解決・Windows ライクなメニューバー・各種オーバーレイ修正。
- **東方夢想伝01 進行不能の根治**: 戦闘撃破→`破壊`/`全滅` イベント発火の一本化（`fire_destruction_labels`）/ `クリア` 発火 /
  敵全滅 idle 委譲デッドロックの救済（`proceed_after_victory`）/ 敗北ソフトロックのフォールバック（`pending_game_over`）/
  マップ兵器撃破の破壊・全滅・check_victory 発火 / 精神コマンド効果側の完成（25 種）/ ゲームオーバー・コンティニュー画面。
- **進行制御の再設計**: 完了プロトコル＋FlowCont 継続スタック＋割込みイベントキュー（`docs/FLOW_REDESIGN.md`）。
- **戦闘演出 #1 一式**（`5f6322c`〜`308ec59`）: ネイティブ演出 / SRC_BA エフェクト / lunge / animation.txt 基盤。
- **ゲームプレイ拡張**（`d1b0411`〜`d83f0d2`）: 援護防御選択肢 / 行動不能ゲート / クリティカル機構 / AI 移動スライド。
- **ゲームプレイ系の SRC 準拠再構築**（`9401c06`〜`b38b14f`）: uid 基準の状態管理（pos_index）/ ターン・フェイズ再構築 /
  キャンプ判定一元化 / 逐次 AI 演出＋反撃モード。地形適応（`adaptation_mult`/`predict_with_status_terrain`）。
- **アーカイブ互換性**（`a5c021f`〜`ea9f32c`）: データロード堅牢化（warn+skip）/ 全角コンマ正規化 / 未終了クオート寛容化 /
  unit.txt 4 フィールド対応 / 多数の `.eve` コマンド実装 / 式評価のシステム変数解決。

---

## 10. 参照

- 元実装: `SRC_20121125/`（VB6）／ C# 移植: `SRC.Sharp/SRC.NET/`（仕様確定の一次情報。`Unit.cs` が戦闘の中核）
- SRC コマンド仕様: `SRC.Sharp/SRC.Sharp.Help/src/menu.md` をインデックスに使う
- アーカイブスキャン詳細: [`docs/ARCHIVE_SCAN_REPORT.md`](ARCHIVE_SCAN_REPORT.md)
- SRC.Sharp との乖離記録: [`docs/SRC_SHARP_DIVERGENCE.md`](SRC_SHARP_DIVERGENCE.md)
- 進行制御の再設計: [`docs/FLOW_REDESIGN.md`](FLOW_REDESIGN.md)
- フィクスチャ: `crates/src-web/tests/fixtures/`
- 穴埋めロードマップ・精神コマンド状況: memory `project_gap_audit_roadmap` / `project_spirit_commands_status`
