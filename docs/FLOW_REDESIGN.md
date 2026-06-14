# ゲーム進行アーキテクチャ再設計案 — 「idle 推定」から「明示的継続」へ

状態: **Phase 1 実装済み / Phase 2 一部実装済み**（§4 参照。2026-06-10）。
Phase 2 残り（idle 述語 shim の削除＝実機検証後）と Phase 3 は未着手。
対象: `crates/src-core` の大域進行制御（シーン/ステージ/イベント再開/インターミッション/AI）。

---

## 1. 診断 — なぜガード追加が繰り返されるのか

### 1.1 原典 VB6 の制御モデル（確認済み事実）

`SRC_20121125/SRC.bas` の `StartScenario`（L1069）/ `StartTurn`（L1266）は**線形手続き**である:

```
StartScenario(fname):
    LoadEventData
    Stage = "プロローグ";  HandleEvent "プロローグ"
    If IsScenarioFinished Then Exit Sub        ' ← Continue 等による中断脱出
    Stage = "味方";  DumpData _リスタート.src
    HandleEvent "スタート"
    If IsScenarioFinished Then Exit Sub
    StartTurn "味方"

StartTurn "味方":
    Do
        Turn += 1; 状態回復; HandleEvent "ターン", Turn, "味方"
        If 操作可能な味方あり Then Exit Do      ' → メッセージポンプへ戻り入力待ち
        StartTurn "敵"; StartTurn "中立"; StartTurn "ＮＰＣ"
        (各所で If IsScenarioFinished Then Exit Sub)
    Loop While True
```

ポイントは 2 つ:

1. **「次に何をするか」= コールスタック上の戻り先**。`HandleEvent "スタート"` が
   （内部の DoEvents ブロッキング待ちを何度経ようと）returnすれば、次の行
   `StartTurn "味方"` が必ず実行される。継続情報は VB6 スタックが暗黙に保持。
2. **大域中断は `IsScenarioFinished` の手動巻き戻し**。すべての `HandleEvent`
   呼び出し直後に `Exit Sub` 判定があり、`Continue` 実行はスタック全体を
   巻き戻して次シナリオの `StartScenario` に到達する。

### 1.2 現行 Rust 移植が捨てたもの

WASM ではブロッキング待ちができないため、現行実装は正しく**制御を反転**した
（`PendingDialog` で suspend → `respond_dialog` → `resume`）。しかしその際、
**継続情報（イベント完了後に何をするか）を一緒に持ち込まなかった**。

- `event_runtime::run_loop`（event_runtime.rs:740）は**完了でも suspend でも同じ
  `Ok(())`** を返す。呼び出し側はスクリプトが「終わった」のか「待ちに入った」のか
  戻り値では分からず、`has_script_context()` / `pending_dialog` を再検査するしかない。
- 「スタートイベントが終わった」という**完了通知がどこにも無い**ため、進行は
  `auto_progress_stage_state_if_idle()`（app.rs:4252）による **idle 推定**で再構成
  される: 「全部 None なら、StageState を見て現在地を当て、次へ進めてよいはず」。
- その `auto_progress` は 5 箇所（respond_dialog ×3 / tick / start_scenario）から
  呼ばれ、idle の定義（script_ctx / pending_dialog / pending_timer /
  intermission_running / scene / stage 非空 / current_stage_file 非空…）が
  呼び出し文脈ごとに微妙に違う。

### 1.3 帰結 — ガードの増殖は構造的必然

「フローの現在地」を **StageState という粗い代理変数から逆算**しているため、
シナリオの形が 1 つ増えるたびに逆算が壊れ、ガードで補修することになる:

| 既存ハック | 失われた継続の復元対象 |
|---|---|
| `start_battle_phase_after_inline_load`（app.rs:4205） | 「スタートを発火した後、完了したら味方フェイズ」という継続が無いので、**インライン完了 vs suspend** で呼び出し側が分岐し、再発火 (敵二重配置) を避ける専用パスが要る |
| `stage 空 && current_stage_file 空なら bail`（app.rs:4262） | 「いまシナリオ進行フローの中に居るか」をフィールドから推定している（フロー自体が無いので） |
| `intermission_commands 登録有無では判定しない`（app.rs:4267 コメント） | 「インターミッションはフローのこの位置」という情報が無く、登録の有無を現在地の代理にして失敗した痕跡 |
| `次ステージ 非空なら MapView 強制遷移しない`（app.rs:4170） | `Continue` ループバックという**フロー上の分岐**を、script_var の値から事後推定 |
| `return_from_intermission_subcommand_if_idle`（app.rs:4294） | 「サブコマンド .eve が完了したらメニューへ戻る」という継続を idle ポーリングで代替 |

つまり**根本原因は「継続の消失」**であり、ガードの精緻化（idle 述語の型化など）は
症状緩和にしかならない。

---

## 2. 提案 — 完了プロトコル + シナリオディレクタ

原典の「コールスタック」を、**serde 可能な明示的データ**として持ち直す。
2 段構えで、どちらも独立に価値がある。

### 2.1 中核その 1: スクリプト完了プロトコル（ExecOutcome）

```rust
pub enum ExecOutcome {
    /// スクリプトは最後まで実行された（suspend なし、または resume の末に完了）
    Completed,
    /// pending_dialog / pending_timer で中断し、ctx は App に預けた
    Suspended,
}
```

- `run_loop` / `resume` / `trigger_label*` / `run_from_pc` は `ExecOutcome` を返す。
- **`Completed` になった瞬間（初回実行・resume 後を問わず）、必ず単一の
  `App::on_script_completed()` を通す**。進行判断はここ 1 箇所だけが行う。
- これにより「インライン完了」と「suspend 後に完了」が呼び出し側から**区別不要**
  になる。`start_battle_phase_after_inline_load` の存在理由が消える。

### 2.2 中核その 2: 継続スタック（FlowCont）とディレクタ

`App` に serde 可能な継続スタックを持たせ、イベント起動時に
「完了後にやること」を必ず積む:

```rust
/// VB6 の「HandleEvent から戻った後の次の行」に相当する継続。
/// 1 バリアント = SRC.bas の 1 ブロッキング点直後のコード。
#[derive(Serialize, Deserialize)]
pub enum FlowCont {
    /// プロローグ完了 → リスタートセーブ → スタート発火（StartScenario 中盤）
    AfterPrologue,
    /// スタート完了 → 味方フェイズ開始（StartScenario 末尾 = StartTurn "味方"）
    AfterStartEvent,
    /// ターンイベント完了 → 操作可能ユニット判定 → 入力待ち or AI フェイズ
    AfterTurnEvent { phase: Phase },
    /// フェイズ内 AI 続行（ai_runner の次ユニットへ）
    ResumeAiPhase { phase: Phase },
    /// 勝利/敗北イベント完了 → インターミッション or 次ステージ
    AfterVictoryEvent,
    /// インターミッションサブコマンド .eve 完了 → メニューへ復帰
    ReturnToIntermissionMenu,
    /// Continue で予約された次ステージのロード（= StartScenario 再入）
    LoadNextStage,
    /// 割込みイベント（破壊/行動終了 等）完了 → 元の処理へ復帰
    AfterInterrupt,
}

pub struct App {
    // ...
    flow: Vec<FlowCont>,   // 継続スタック（serde 対象）
}
```

動作規則（これだけ）:

1. イベントを起動する側は `trigger_label_with(label, cont)` で継続を積む。
2. `on_script_completed()` は `flow.pop()` して継続を実行する。継続の実行が
   さらにイベントを起動して suspend したら、そこで止まる（次の完了でまた pop）。
3. `Continue` / `GameOver` 等の大域中断は、VB6 の `IsScenarioFinished` 巻き戻しに
   対応する **`flow` の一括差し替え**（例: 全部捨てて `LoadNextStage` を積む）。
   「次ステージ非空なら…」のような事後推定が、明示的なスタック操作になる。
4. 割込みイベント（ユニット破壊・接触・行動終了など、処理の途中で発火するもの）
   は原典 `EventQue` と同様に**構造化キュー**へ積み、現在のスクリプト完了後・
   継続 pop **前**に消化する。

**`auto_progress_stage_state_if_idle` / `return_from_intermission_subcommand_if_idle`
は削除**。idle 推定そのものが不要になる（進行はすべて完了イベント駆動）。
`StageState` は進行判断から外れ、表示・勝敗判定ゲート用の**読み取り専用ラベル**に降格。

### 2.3 待ちの一般化（第 3 段階）

現在 `pending_dialog` / `pending_timer` / `pending_reaction` / `battle_anim` /
`move_anim` / `ai_runner` が個別にお互いを牽制している（ai_runner_tick の modal
チェック等）。最終形では「スクリプトの suspend」と同列の **Wait 条件**として統一する:

```rust
pub enum WaitKind {
    Dialog(PendingDialog),
    Timer(f64),
    Animation,          // battle_anim / move_anim の完了待ち
    Reaction(PendingReaction),
}
```

`tick()` の仕事は「期限切れ/完了した Wait を解除し、解除されたら resume または
継続 pop」だけになる。AI フェイズも `ResumeAiPhase` 継続として表現でき、
`ai_runner` の「modal なら待機」分岐は自然に消える。

---

## 3. 代替案と不採用理由

| 案 | 内容 | 不採用理由 |
|---|---|---|
| A. async/await コルーチン | StartScenario を `async fn` で直訳し、待ちを `await` にする | **Future は serialize できず save/load（`to_save_json` が script_ctx ごと保存する現行要件）と両立しない**。`&mut App` を await 跨ぎで持つ借用問題も深刻。WASM 単一スレッドでの自前 executor 分も複雑化 |
| B. ゲートの型化のみ（ProgressGate 等） | idle 述語を enum に集約して呼び出し側を統一 | 述語の重複は減るが、**「完了通知が無い」「継続が無い」という根本はそのまま**。シナリオ形が増えれば述語自体を直し続けることになる。§2.1 の一部として吸収する方が良い |
| C. 全面書き直し（イベントエンジンごと） | event_runtime を含めて再設計 | 18,000 行 + テスト 1678 件の資産があり、**.eve インタプリタ自体は問題を起こしていない**。問題は上位の進行層のみ。不要なリスク |

---

## 4. 移行計画（テストを緑に保ったまま 3 段階）

### Phase 1 — 完了プロトコル（外部挙動不変・最小差分）✅ 実装済み

実装メモ（2026-06-10）:

- `ExecOutcome` は `run_loop` 内部の判定に留め、公開 API
  (`trigger_label` / `resume` 等) のシグネチャは変更しなかった（差分最小化）。
  完了通知は `run_loop` が一元的に `App::on_script_completed()` を呼ぶ。
- `FlowCont` は `AfterStartEvent` / `ReturnToIntermissionMenu` /
  `AfterStageFileRun` の 3 バリアント（`crates/src-core/src/flow.rs`）。
- 「スタートをインライン実行したか」は推定ではなく **`スタート`/`Start`
  ラベル行の通過の事実**（`App::stage_start_ran`、run_loop_inner が記録）で
  判定する。これにより旧ヒューリスティクスが誤る「suspend したがスタートは
  インライン実行済み」のケース（再発火＝敵二重配置）も正しくなる。
- 副次修正: `begin_battle` が `スタート` suspend 中に即 `begin_phase` して
  ターン 1 イベントが黙って消えていた潜在バグを解消（フェイズ開始は
  `AfterStartEvent` 継続に移動）。
- `start_battle_phase_after_inline_load` は削除済み。`auto_progress` /
  `return_from_intermission_subcommand_if_idle` は互換 shim として残存
  （Phase 2 で削除予定）。`__srcDebug()` に `flow` を表示。

1. `ExecOutcome` を導入し `run_loop` / `resume` / `trigger_label*` の戻り値を変更。
2. `App::flow: Vec<FlowCont>` と `on_script_completed()` を追加。
3. 現行の `auto_progress` / `return_from_intermission_*` /
   `start_battle_phase_after_inline_load` の中身を、対応する `FlowCont`
   バリアントの実装として移植（呼び出し箇所は完了プロトコル経由に一本化）。
4. 旧関数は deprecated shim として残し、テストが直接呼んでいる箇所
   （app.rs:5419 等）を順次移行。

**完了条件**: `cargo test -p src-core` 全緑、スパロボ戦記/musou202/温泉旅館の
実機進行が現状と同等。`start_battle_phase_after_inline_load` の削除。

### Phase 2 — ディレクタ移植（StartScenario/StartTurn の写経）🔶 一部実装済み

実装済み（2026-06-10）:

- **割込みイベントの構造化キュー（原典 `EventQue` 相当）**:
  `App::event_queue: VecDeque<String>`（serde 対象）＋ `post_event_label()`。
  ラベル存在判定は投函時、実行はスクリプト完了後 FIFO（`on_script_completed`
  が flow 継続より先に消化）。`run_loop` に実行ネスト深度（`script_depth`）を
  導入し、最外殻の完了時のみ drain する。
- **再入実行の廃止**: `fire_destruction_labels` / `全滅` / `Victory`/`GameOver` /
  攻撃・攻撃後ペアイベント / ユニットイベント / `損傷率` / `begin_phase` の
  ターンイベント群を `trigger_label`（再入・ctx 上書きハザード、同期完結
  ハンドラ前提の割り切り）から `post_event_label` に置換。これにより
  (a) スクリプト実行中の `Kill` が積む破壊ハンドラに Talk 等の suspend 系命令を
  含められる（外側 ctx を上書きしない）、(b) 先行ターンイベントの suspend で
  後続ターンイベントが黙殺されるバグが解消。
- **`start_scenario` の継続化**: プロローグ発火後の進行を `auto_progress` 直呼び
  から `FlowCont::AfterStageFileRun` 継続＋完了通知に変更。
- **`Continue <file>` のエンジン内チェイン化（`FlowCont::LoadNextStage`）**:
  非インターミッションの `Continue` は、原典の `IsScenarioFinished` スタック
  巻き戻しに相当する **flow/event_queue の一括破棄（`scenario_transition_reset`）**
  を行った上で `LoadNextStage` を積み、現スクリプト（エピローグ含む）完了後に
  エンジンが `advance_to_next_stage` を自動実行する。旧実装は「archive ロード時の
  while ループ／テスト側の手動 advance が拾う」前提で、**戦闘中の `Continue`
  （勝利→エピローグ→次ステージ）を誰も消費せず停止するギャップ**があった。
  あわせて `advance_to_next_stage` は解決失敗時に `次ステージ` 予約を復元する
  （診断・再試行可能性の維持）。drain には暴走 Continue ループ対策の
  ステップ上限（256）を設置。
- **意味論変更**（fixture 更新済み: `18_total_annihilation_autofire`）:
  スクリプト内 `Kill` の破壊/全滅イベントは「Kill 文の位置で割込み実行」から
  「スクリプト完了後に実行」へ（原典 Event.bas 準拠の順序）。

残り:

- 実機（スパロボ戦記 84MB / musou202 / 温泉旅館）のブラウザ通し検証後、
  `auto_progress_stage_state_if_idle` / `return_from_intermission_subcommand_if_idle`
  shim を削除（archive ロード末尾のブートストラップ継続 push が前提）。
- 勝敗 → インターミッション/次ステージ遷移の継続化、`StageState` の表示専用化。

1. `start_scenario` → `AfterPrologue` → `AfterStartEvent` → `AfterTurnEvent` の
   チェーンを SRC.bas L1069-1400 と**行単位で突き合わせて**実装
   （リスタートセーブのタイミング、`IsScenarioFinished` 判定位置を含む）。
2. `Continue` / 勝敗 / インターミッションを flow 差し替えとして再実装。
3. 割込みイベントの構造化キュー（原典 `EventQue` 相当）導入。
4. `StageState` を進行判断から排除（表示・check_victory ゲートのみに）。

**完了条件**: §1.3 の表のハックが全て削除されている。idle 述語が repo から消える。

### Phase 3 — 待ちの統一

1. `WaitKind` に dialog/timer/anim/reaction を統合、`tick` を「Wait 解除 → 継続」へ縮小。
2. `ai_runner` を `ResumeAiPhase` 継続 + Animation Wait で再表現
   （`animate_ai=false` は「Wait を即時解決する」モードとして同期テスト互換を維持）。

---

## 5. リスクと検証

- **再入**: イベント実行中に発火する割込み（破壊→破壊台詞）は、原典も
  「キューに積んで後で消化」(Event.bas `EventQue`) であり、継続 pop 前の
  キュー消化として同型に移せる。**継続の実行中に継続を pop しない**規則を
  `on_script_completed` の構造で強制する（ループはするが再帰しない）。
- **セーブ互換**: `flow` は serde 追加フィールド（`#[serde(default)]`）。
  互換不問の方針（CURRENT_WORK §恒久制約）なので問題なし。むしろ
  「インターミッションのどの段階か」までセーブに乗り、復元が正確になる。
- **テスト同期パス**: 継続はスクリプトが suspend しなければその場で即実行される
  （inline 完了 = 即 pop）ため、`animate_ai=false` の run-to-completion な
  既存テストの実行モデルは変わらない。
- **検証手段**: `__srcDebug()` に `flow` スタックのダンプを追加すれば、
  「いまフローのどこで止まっているか」が常に可視化される。現在の
  「scene/stage_state から現在地を推測する」デバッグより大幅に楽になる。

---

## 6. 参照

- 原典: `SRC_20121125/SRC.bas` L1069 `StartScenario` / L1266 `StartTurn`、
  `Event.bas`（EventQue / CallStack / DoEvents 待ち）
- 現行の症状一覧: `docs/CURRENT_WORK.md` §1（auto_progress ガード是正の履歴）
- 現行コード: `crates/src-core/src/app.rs:4252`（auto_progress）/
  app.rs:4205（inline_load 特例）/ `event_runtime.rs:732-800`（resume/run_loop）
