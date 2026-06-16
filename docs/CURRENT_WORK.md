# 現在の作業状況 (Session Handoff)

VB6 製 SRC (Simulation RPG Construction) を Rust + WebAssembly に移植中。
本ドキュメントは作業継続のための要約。**解決済み課題は §9 に 1 行で要約**し、本文は
「現状・残課題・恒久リファレンス」に絞る。各課題の commit ハッシュ・実装詳細は memory
`project_gap_audit_roadmap`（穴埋めロードマップ）/ `project_spirit_commands_status`（精神コマンド）に集約。

---

## 現在地（2026-06-16）

**テスト**: `cargo test -p src-core` 全緑（**1878 件**）／ clippy clean（`-D warnings`）／ wasm `cargo check` OK。  
**ブランチ／コミット**: **`feat/necessary-skill-gate`**（**本セッション 67 コミット・未 push**）。`origin/master`=`88ad16f` から先行。
本セッションで実エンジンバグ **4 件**修正（pilot.txt カンマ形式特殊能力・`Input` 配列 lvalue 値展開・`expand_vars` クオート内 `name[expr]` 展開・マップ範囲外クラッシュ）。
push はユーザの明示指示で行う（no-auto-push）。**D スパロボ戦記の「進行不能」は §2 で解決済**（エンジンは戦闘まで完走、原因は harness）。
次セッションの残課題は §1。**★ ユーザ決定（2026-06-16）**: ① ✅ **魅了/憑依は「spec 準拠で実装」完了**（憑依=恒久支配・ボス免疫・SpecialPower 封じ /
魅了=3T 一時・護衛行動・期限切れ復帰。synthetic test 4 件。実装詳細は §1.1）。② 並行方針は **「更にバグ探索を継続」**（in-repo fixture は全駆動済のため、新規の実シナリオ zip を各自取得して
`verify-archive` で drive し一般エンジンバグを探す）。他の残（GBA 大規模移植・A2/演出/詳細 UI の検証制約）は従来どおり。

**到達点**: 監査で洗い出した SRC ゲームプレイの大穴を**ほぼ全て実装済**。具体的には — 戦闘実効値（改造/強化パーツ/育成/状態異常を
反映）・撃破報酬（資金＋育成）・気力経済（撃墜/被弾/性格別変動/技能加速）・武器の EN/残弾消費＋資源を尊重した自動選択・
特殊効果攻撃属性（CC 属性ほぼ全種）・防御特性ファミリ（耐性/弱点/有効/吸収/無効化）・パイロット戦闘技能（底力/超底力/潜在力開放/
得意技/ハンター/見切り/超反応/超能力 等）・BossRank サブシステム（即死/死の宣告）・インターミッション経済ループ（機体改造/換装/
乗り換え/ステータス/データセーブ）・プレイヤー向けユニットコマンド（修理/補給/変形/チャージ/合体/分離/母艦発進）・アビリティ
proper（パーサ＋操作＋効果）・敵 AI 戦術判断（攻撃補助/回復/補給/召喚/敵対象アビリティ・回復精神・マップ兵器・防御地形選好・復活
pre-buff・散開・ChangeMode 逃亡/護衛）。**残るのは外部依存・大規模・設計判断・検証制約のある項目のみ**（§1）。

**方針メモ**: エンジンは SRC_20121125 (VB6) / SRC.Sharp (C#) の**忠実移植**であるべき。原典に定義の無いシナリオ独自要素
（例: 東方夢想伝の精神 決意/気迫/希望）は**推測実装せず**、シナリオ側（sp.txt 効果種別/イベント）の責務とする。

**本セッション（2026-06-16）の実装**（詳細は §9 / memory）:
- **魅了/憑依を spec 準拠で実装**: `weapon_possession` ＋ `charm_revert_party` ＋ `apply_weapon_special_effects` 配線。憑依=恒久支配・ボス免疫・
  SpecialPower 封じ / 魅了=3T 一時・護衛 ai_mode・begin_phase で期限切れ復帰。synthetic test 4 件（§1.1）。
- **必要技能/必要条件ゲート（§1〜§4 実用上完全）**: 評価器 `necessary_skill` ＋ 武器/アビリティ/形態（変形/換装/乗り換え/合体）配線、
  動的化（撃墜数の戦闘中加算・習/ラーニング）、前提バグ修正（pilot.txt カンマ形式特殊能力の取りこぼし）。
- **周辺の原典準拠化**: 乗り換え Option ゲーティング・Ｄ属性気力吸収・マップ兵器での復活尊重・毒/死の宣告の実効HP基準・盗のアイテム盗み。
- **★ D スパロボ戦記の triage を大きく前進**: in-repo fixture を zip→`verify-archive` ドライブ＋新設 `VERIFY_ENTRY=@2`/`VERIFY_AUTOSTART`/
  `VERIFY_AUTOPLAY`/`VERIFY_VAR`/`debug_firable_report` で **ブラウザ/84MB 無しに triage→キャラメイキングを自動駆動**。「進行不能」は主に
  harness 制約（entry-point/メニュー操作/キャラメイキング未駆動）だった。**過程で実エンジンバグ 1 件を修正**（`Input` の配列 lvalue 値展開＝
  `865844c`、複数 Input する全シナリオに効く）→ パイロット複数作成可。**`VERIFY_SEAT_DEBUG` で D マップの戦闘成立（攻撃/反撃/勝敗）も実証**
  （`16caf45`）＝combat エンジンは健全。残は正規プレイの出撃導線（CMaking の正規 exit）のみ。詳細は §2。

---

## 1. ★ 残課題サマリ（次セッション引き継ぎ）

> ゲームプレイ機能はほぼ網羅済み。残りは「外部リソース・設計判断が要る大課題」と「検証制約のある精緻化」に分かれる。

### 1.1 外部リソース・設計判断が要る（大）

- ✅ **D スパロボ戦記（進行不能）＝解決済（2026-06-16・§2）**: `VERIFY_ENTRY="@2" VERIFY_AUTOSTART=1` で root から駆動すると
  Title→難易度設定→機体選択→サブ機体選択を完走し、**味方 2 機(ガンダム) vs 敵 7 機(ガンセクト6＋ボス1)の戦闘まで到達**（ブラウザ/84MB 不要）。
  「敵が出撃しない/進行不能」は**完全に harness（verify-archive のドライブ）の制約**で、エンジンのキャラメイキング・敵候補生成・敵配置は
  すべて正しく機能していた（**エンジン修正不要**）。✅✅✅ **戦闘が走らない真因も特定済**: 機体は意図的に `パイロット不在` で生成され、
  パイロットは `キャラクターメイキング`（CMaking.eve）で作る設計。auto-drive がこれを素通りしていた（`pilot=""`→`effective_combat_data=None`
  →攻撃が静かに不発）。✅ **キャラメイキングの drive 自動化を実装**（`e6a8c36`）し**パイロット作成・ロスター追加まで実証**。
  ✅✅✅ **エンジンバグ 1 件を修正**（`865844c`）: `Input` が代入先（配列 lvalue）を値展開していたため、2 人目以降の名前入力が前回値のまま
  固まっていた（`resolve_lhs_name` で格納キーに解決、回帰テスト付き。**配列変数へ繰り返し Input する全シナリオに効く一般修正**）→ **複数パイロット作成可**。
  ✅✅✅ **戦闘の成立を実証**（`16caf45`）: `VERIFY_SEAT_DEBUG`（`debug_seat_db_pilot`）で無人の味方機に DB パイロットを乗せると、D マップで
  攻撃（ハイパーバズーカ 1380）→反撃→ダメージ→Defeat まで完走＝**combat エンジンは健全**で当初からの唯一の問題は「味方が無人」だったと確定。
  **残（正規プレイの通しのみ・combat とは独立）**: ① キャラメイキングの正規 exit（headless 不可＝`データロード` が要セーブファイル、右クリックは
  再登録ループ）② ロスター追加≠機体搭乗（後段工程）。詳細は §2。
- ✅ **必要技能ゲート（2026-06-16 実装、`feat/necessary-skill-gate`）**: `(念力Lv3)` 形式の括弧条件で武器/アビリティの
  使用可否を制限。`necessary_skill` モジュール（`split_necessary`＋`is_satisfied`、AND-of-OR＋`!`/`*`/`+`）を新設し、
  `is_weapon_available`／`weapon_firable`（ライブ AI/反撃/援護）／`pick_attack_weapon` 強制分岐／`ability_usable` に配線。
  撃墜数Lv*（戦記の主要ゲート）が機能（エース解禁/ザコ封印）。**前提バグも同時修正**: pilot.txt 特殊能力のカンマ形式
  （`撃墜数Lv100, 1` 等）を取りこぼしていた parser を修正（撃墜数/底力/切り払い が実データで有効化）。未モデル種別
  （ステータス閾値・同調率・霊力・生身）は fail-open で誤封印回避。✅ **動的化も完了（2026-06-16）**: ① 撃墜数の*戦闘中加算*
  （`award_kill_rewards` で撃破者主パイロット +1）→ 規定数撃墜で武器解禁。② **習（ラーニング技）**（クリティカルで対象の
  `ラーニング可能技` を主パイロットが習得→ゲートが読む。攻撃側使用時に機能、反撃クリ未対応）。✅ **形態ゲート（§4）も完了**:
  `UnitData` の `必要技能=`/`不必要技能=` を **変形/換装/乗り換え/合体**に配線（`form_skill_ok`、乗り換えは swap→判定→不可なら revert、
  合体は merged 構成員で非破壊事前チェック）。分離は合体前の有効形態へ戻るため非ゲート。
  **残（軽微・意図的）**: ① 必要技能未達武器のステータス画面**非表示**（機能ゲートは完了、表示フィルタは frontend 側未）。
  ② 格闘Lv200 等の**ステータス閾値は fail-open 据置**（誤封印回避を優先＝モデル化しない決定。fail-open は無ゲート従来挙動と等価で無害）。
  ③ ✅ **§3 ユニット用特殊能力の必要技能ゲートを実装**（2026-06-16、`63...`）: `特殊能力名=値 (必要技能)` 形式（必要技能.md §3）を
  `populate_active_features` で評価し、未達なら `is_active=false`。曖昧回避のため**スペース区切り必須**の `split_feature_necessary` で
  値末尾 ` (必要技能)`/` <必要条件>` のみ剥がす（形態名 `ガンダム(MA)` 等の値内 `(...)` は誤切断しない）。未モデル種別は fail-open。
  回帰2件。共有ヘルパ `gated_active_features` に集約し **Create/Place・変形・合体・換装-分離・set_unit_form の全 feature 生成サイトで一貫適用**
  （`65...`）。**注**: in-repo fixture では未使用のため synthetic test で検証。§5 アイテムは Equip がバイパス・交換 UI 無しで適用点なし。
- **魅了/憑依**: ✅✅✅ **実装完了（2026-06-16、`feat/necessary-skill-gate`）**。ユーザ決定「spec 準拠で実装する」に従い、原典 SRC 仕様
  （`特殊効果攻撃属性.md` 69-75 魅 / 113-117 憑）どおりに ChangeParty の `u.party=` 基盤を再利用して最小・慎重に実装した。
  - **検出ヘルパ** `combat::weapon_possession(class) -> Option<&'static str>`（`憑`→"憑依" / `魅`→"魅了"、class トークン先頭一致）を新設。
  - **フィールド** `UnitInstance.charm_revert_party: Option<Party>`（`#[serde(default)]`）＝魅了で一時勢力変更したときの**元勢力**を保持（憑依は使わない）。
  - **配線** `App::apply_weapon_special_effects` の proc 成功分岐へ。early-return 条件に possession を加え、`is_boss`（BossRank 免疫）・
    `firer_idx != def_idx`（自爆除外）でガード:
    - **憑（憑依）**: 相手を攻撃側勢力へ**恒久支配**し condition `憑依`（永続）を付与。**復帰なし**。
    - **魅（魅了）**: 元勢力を `charm_revert_party` へ退避し攻撃側勢力へ移し `ai_mode="護衛 <firer_uid>"`（spec「魅了主を護衛する味方のように行動」）
      ＋ condition `魅了` lifetime=4（3 ターン）。
  - **復帰フック** `begin_phase`（`tick_conditions` の直後）: `charm_revert_party.is_some()` かつ `魅了` が消えたユニットを元勢力へ戻し、
    護衛 `ai_mode` と `charm_revert_party` をクリア。
  - **SpecialPower 封じ** `spirit_command_options`（プレイヤー UI / AI 精神の唯一の関門）冒頭で `憑依` condition 保持なら空を返す（spec「スペシャルパワーは使用できません」）。
  - **検証**: synthetic test 4 件（憑依=恒久支配・ボス免疫・SpecialPower 封じ／魅了=勢力反転・敵対関係再評価・begin_phase 期限切れ復帰）。
    in-repo fixture に 憑/魅 武器が無いため synthetic で検証（§3 ゲートと同様）。critical=100 で proc 決定論化。
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

### ✅✅✅ 2026-06-16 解決: D の「進行不能」は harness 由来で、エンジンは戦闘まで正しく進む

**正しく駆動すれば D は戦闘まで一気に到達し、味方も敵も出撃する**ことをヘッドレスで確認した。再現:
```bash
cd crates/src-web/tests/fixtures && zip -rq /tmp/sparobo.zip スパロボ戦記 && cd -
export VERIFY_SMOKE=1 VERIFY_DRIVE=1 VERIFY_ENTRY="@2" VERIFY_AUTOSTART=1
cargo run -q -p verify-archive --bin verify-archive -- /tmp/sparobo.zip
# → Title→難易度設定→機体選択→サブ機体選択 を完走 (units 0→9 = 味方2＋敵7) → 敵候補="ガンセクト" 敵陣営="インスペクター"
#   → Battle (scene=MapView, on_map=9/9): ガンダム[Player]×2 vs ガンセクト[Enemy]×6 + 機械獣アブドラＵ６[Enemy]×1
```
> **数の内訳 (2026-06-16 実測で訂正)**: 「units 0→9」は**総数**で、内訳は**味方ガンダム 2 機**(キャラメイキングで主＋サブを auto 選択)
> ＋**敵 7 機**(ガンセクト 6＋機械獣アブドラＵ６ 1)。当初メモの「味方×9」は総数の誤読。味方が 2 機なのは autostart が機体選択で
> 既定 1 種＋サブ 1 種を選ぶため (正規プレイは複数キャラメイキング可)。
**結論**: 「敵が出撃しない/進行不能」は**完全に harness（verify-archive のドライブ）の制約**で、エンジンのキャラメイキング・
敵候補生成・敵配置はすべて正しく機能していた。**エンジン側の修正は不要**。harness を 3 点直して解決:
1. **entry-point**: `entrypoint::analyze` が Main.eve(戦闘) を選ぶ → `VERIFY_ENTRY=@2`(root) で上書き（`@N` index 形式、日本語 env 文字化け回避）。
2. **メニュー進行**: `VERIFY_AUTOSTART=1` で `【開始】` を選び難易度設定を抜け、機体メニューでは `決定する` を選ぶ。
3. **クリック→respond**: autostart 時はメニューを `respond_dialog(choice)` で直接確定（クリック座標 `click(120,304+行*20+10)` が
   `決定する`(option 2) を外し `確認する`(option 1, 仮表示ループ) に当たっていた）。`Confirm` は既定 0=はい（`app.rs:1036` で `0→選択=1` 反転、これは正しい）。

> **教訓**: 「実シナリオで進行不能」を engine バグと決めつけない。**完全 fixture をヘッドレス drive で完走させると、多くは harness/操作の
> 再現漏れ**だった（entry-point 選択・対話の正しい選択肢・クリック座標）。新ツール `VERIFY_ENTRY=@N`/`VERIFY_AUTOSTART`/`VERIFY_VAR`＋
> 対話発生元計装（`App.exec_pc`＋逆引き）で 84MB ブラウザ無しに切り分けられる。

**残（任意・engine 非ブロック）— ✅✅✅ 真因を特定済（次セッションは「修正」だけ）**:

① ✅ **検証ツールを追加**（commit `5cf1642`/`78dbcf5`）: `VERIFY_AUTOPLAY=1`（味方フェイズに味方を AI で前進・攻撃させる
`App::debug_run_phase_ai`）＋ `App::debug_firable_report`（戦闘開始時に各ユニットの pilot 解決・combat_data 有無・武器ごとの発射可否を
ダンプ）。再現: `export VERIFY_SMOKE=1 VERIFY_DRIVE=1 VERIFY_ENTRY="@2" VERIFY_AUTOSTART=1 VERIFY_AUTOPLAY=1`。

② ✅✅✅ **交戦が成立しない真因＝味方ユニットにパイロットが付いていない**。firable report の実測:
```
ガンダム[Player]  pilot=""(✗DB欠) combat_data=✗None  ← 味方: パイロット未付与
ガンセクト[Enemy] pilot="人工知能(ザコ)"(✓DB) combat_data=✓  ← 敵: 正常
```
味方の `pilot_name` が空 → `db::effective_combat_data` が `pilot_by_name("")?` で **None** を返す → `attack_resolve_and_run`
（`app.rs:3004`）が冒頭で `return false` し**攻撃が静かに不発**。武器・気力・EN は健全（ビームライフル r4/ハイパーバズーカ r5 等すべて
✓firable、敵 ガンセクトも r6/r4 firable）で、firability や必要技能ゲートは無関係だった。敵が攻撃しないのも、対象の味方が
combat_data=None のため戦闘解決が成立しないから（敵側は正常）。**※ 敵配置に同一マス重複あり（(10,3)×3・(17,3)×2）も観測したが
副次的**（clean ペアでも不発のため主因ではない。別途 `Create 敵`/`Place` のスペーシングは要確認）。

③ **これは「進行不能」ではなく、ヘッドレス auto-drive 固有のアーティファクト**。`機体選択開始` は機体を **`Create 味方 入手ユニット 0 パイロット不在 …`**
（＝**意図的にパイロット不在**で）生成し、パイロットは後段の `IntermissionCommand キャラクターメイキング Lib\CMaking.eve` で作る設計。
auto-drive は当初このインターミッション項目を素通りしていた（＝パイロット未作成）。

④ ✅ **キャラメイキングを drive 自動化した**（commit `e6a8c36`）。実装した navigation:
- インターミッションで `キャラクターメイキング` 項目を選ぶ（`intermission_item_label` で検出、未実行時のみ）。
- `召喚画面` の HotPoint は engine が **Menu 化**（`event_runtime.rs:3940` 付近、Wait Click＋HotPoint→`PendingDialog::Menu`、選択名を `選択` に格納）。
  これを利用し `名前入力`→（Input に**全角カタカナ一意名** `パイロア` 等を与える）→ `決定` の順で進める。姓/性別/愛称は既定値・カタカナで充足。
- 確定後の `パイロット能力表示`（AlphaSecond.eve のタブ閲覧画面 `[ユニット|機体|レーダー|武器]`）は進行肢が無いので**右クリックで cancel して抜ける**。
- → **パイロットを作成し部隊ロスターに追加できることを実証**（`【システム】…を部隊に加えた。`）。

⑤ ✅✅✅ **エンジンバグを 1 件特定・修正**（commit `865844c`）: 当初「2 人目以降のカタカナ名入力が `全角カタカナだけで入力してください`
で固まる」現象の真因は **`Input` コマンドが代入先（第 1 引数の lvalue）を値展開していた**こと。`名前[キー]` 形式の配列変数に既に値があると
（1 人目作成後の `召喚キャラ[名前]="417776"` 等）、`expand_vars` が**現在値をキー名に化けさせ**、テキスト応答が `召喚キャラ[名前]` を更新せず
前回値が残り、その数字で katakana 判定が誤発火していた。**修正**: `Set` と同じく生 `args[0]` を `resolve_lhs_name` で格納キーへ解決
（`event_runtime.rs` Input arm、回帰テスト `input_array_var_target_resolves_to_key_not_value`）。→ **2 人目以降も別名で作成できることを確認**
（`パイロア`/`パイロイ`/`パイロウ`…）。**これは D 専用でなく、配列変数へ繰り返し Input する全シナリオに効く一般バグ修正**。

⑥ **残る具体的ブロッカー（次セッション）**: パイロット作成は通るようになったので、残りは drive navigation:
1. **キャラメイキングの exit（headless では現状到達不可・実機確認要）**: `召喚画面` はラベル＋`Goto` ループで、**唯一の `Break`（exit）は
   `データロード` 経路**（`LoadFileDialog`→セーブ読込→`パイロットリスト` Ask をキャンセル→`RemovePilot`→`パイロットロード終了`→`Break`）。
   `データロード` は実セーブ（`.src`）が要るため fixture ではヘッドレス到達不可。**右クリックは exit にならない**ことを実測確認（drive の
   `respond_dialog_right_click` で wizard メニューを右クリックすると `召喚確定`（既存パイロット再登録）に落ち、同じパイロットを無限に
   再追加してループする）。`召喚制限` は OFF（`部隊に加えますか` Confirm が出るため＝`AlphaSecond.eve:69` の skip 条件不成立）なので人数上限 exit も無い。
   当初の壁＝`LoadFileDialog` 未実装は ✅ 解消（`5c4bba1`）。`VERIFY_CMAKING_EXIT` で データロード経路を駆動する scaffold（`64ade45`, flag 既定 OFF）で
   調査した結果、✅✅✅ **実エンジンバグをもう 1 件発見・修正（`c930c55`）**: `expand_vars` が**クオート内の `name[expr]` を展開していた**ため、
   `If Instr(仮変数,"設定[パイロット一覧]")` の第 2 引数リテラル `"設定[パイロット一覧]"` が、たまたま同名の配列変数 `設定[パイロット一覧]`（作成済み
   パイロット一覧）の**値に化け**、比較が常に失敗していた（データロードの行検出が壊れる真因）。修正＝クオート内は `$(...)` 補間のみ展開し
   裸の `name[expr]`/`Func(args)` はリテラル扱い（回帰 `expand_vars_keeps_indexed_var_literal_inside_quotes`、全テスト緑）。**配列変数と同名の
   文字列リテラルを使う全シナリオに効く一般修正**。→ 修正後はデータロードが正しく行を検出（`「全て作成済み」`まで到達）。
   **ただし データロードは CMaking の exit ではないと判明**: `Break`（264）は内側の読込 Do-Whileを抜けるだけで `Close`→`Goto 召喚画面`（277）で
   **必ずループ**。✅ **CMaking プロローグの構造を全解析した結論＝この fixture では正規 exit がヘッドレス到達不可**: ① 召喚画面 メニューに `ＥＸＩＴ`
   選択肢が無い（`Case ＥＸＩＴ` を起動できない）② 各 Case の `Continue` は唯一の Do（`Wait Click` ループ 110-112）へ戻る＝再描画でループ
   ③ データロード/決定 とも処理後 `Goto 召喚画面`/`Continue` でループ ④ トップレベルに `Return` が無い（`Return` は `召喚キャラデータ作成` 等の
   サブ関数用）。エンジンは `return_from_intermission_subcommand_if_idle` で **idle 時のみ**インターミッションへ戻すが、CMaking は常に Wait Click の
   pending menu を持つため idle にならない。**＝正規 exit は「データロードで save を読む」流れ前提の設計で、save 無し fixture では詰む**。
   次セッション候補: 実機（ブラウザ）で正規 exit 操作を確認（Esc 等で framework が subcommand を抜けるか）。combat は実証済（`VERIFY_SEAT_DEBUG`）
   なので exit/搭乗/出撃は任意課題。
2. **ロスター追加 ≠ 機体搭乗**: キャラメイキングはパイロットを**ロスターに追加するだけ**で、`パイロット不在` 機への搭乗は**後段の別工程**
   （出撃準備／乗せ替え）。drive 終了時も `ガンダム pilot=""` のまま。搭乗工程の drive も要追加。
3. ✅✅✅ **戦闘の成立は実証済**（commit `16caf45`）: 出撃導線（CMaking exit→搭乗）を迂回し、`VERIFY_SEAT_DEBUG=1`（`App::debug_seat_db_pilot`）で
   **パイロット不在のまま出撃した味方機に DB パイロットを乗せる**と、`VERIFY_AUTOPLAY` で **D マップの戦闘が完走**する:
   ```
   seat_debug: パイロット不在の味方 2 機に DB パイロットを搭乗
   人工知能 → 人工知能 [ハイパーバズーカ]: 命中 1380 ダメージ (残HP 1620)
     ↩ 反撃: 人工知能 → 人工知能 [スティンガン]: 命中 1219 ダメージ
   … → stage_state=Defeat（味方 2 vs 敵 7 で敗北＝妥当な決着）
   ```
   攻撃・反撃・ダメージ適用・勝敗判定がすべて機能。**＝D の combat エンジンは健全**で、当初からの唯一の問題は「味方が無人」だったことが確定。
   再現: `export VERIFY_SMOKE=1 VERIFY_DRIVE=1 VERIFY_ENTRY="@2" VERIFY_AUTOSTART=1 VERIFY_AUTOPLAY=1 VERIFY_SEAT_DEBUG=1`。
   **残（任意）**: 「実 pilot を載せた正規プレイ」での通しは #1（CMaking の正規 exit）が要るが、それは combat 検証とは独立した出撃導線の課題。
- **エンジン堅牢化余地（低優先）**: 味方 combat_data=None の攻撃を黙殺せず警告／パイロット欠落ユニットを出撃前に弾く。
- ※ 敵配置に同一マス重複（(10,3)×3・(17,3)×2）も観測。`Create 敵`/`Place` のスペーシング要確認（副次的）。

### （参考）2026-06-16 当初のブレイクスルー: D はヘッドレスで triage 可能（ブラウザ/84MB 不要）

**in-repo fixture `crates/src-web/tests/fixtures/スパロボ戦記/` は完全シナリオ**（eve/Main.eve・lib/CMaking.eve・data/ 一式）。
zip して `verify-archive` のドライブモードで駆動でき、新設 `VERIFY_VAR`（ブラウザ `__srcVar` のヘッドレス相当）で script_var を観測できる。
**§2 の `__srcVar` 切り分けはブラウザ無しで完結する**。再現手順:

```bash
cd crates/src-web/tests/fixtures && zip -rq /tmp/sparobo.zip スパロボ戦記 && cd -
VERIFY_SMOKE=1 VERIFY_DRIVE=1 VERIFY_ASK=1 VERIFY_VAR="敵配置数,敵候補,味方平均レベル,敵陣営,配置場所[7]" \
  cargo run -q -p verify-archive --bin verify-archive -- /tmp/sparobo.zip
```

**確定した根本（drive step 28 = 戦闘スタート時の VERIFY_VAR）**:
- `配置場所[7]="3 21"` ✅ → `Info(マップ,幅/高さ)` は機能（座標は壊れていない）。
- `敵配置数="6.2"` → SRC 準拠（`RoundUp(20×24/90)+ダンジョン進行度/5`=6+0.2）。`/` は float で原典どおり、不具合でない。
- **`敵候補=""` / `敵陣営=""` / `味方平均レベル=""` が空 = 進行不能の根本**。
- **第一ブロッカーは「出撃できる味方がいません」**（drive が `Talk システム: …メイキングしたパイロットを機体に乗せてください` で停止）。
  すなわち **味方ユニット 0**。`味方平均レベル = Int(味方合計レベル / 味方数)`（`Include.eve:1583`）で **味方数=0 → 0 除算で空**。
  `敵陣営`/`敵候補` 空も「味方が居らず正規の出撃準備フローを経ていない」ことの下流。

**→ 真の核は「敵出撃」ではなく「味方が出撃しない」**（§2 旧仮説の「敵配置/Info 由来」はほぼ否定。Info も式中ユーザ関数も主因ではない）。
**さらにその後の第2ブレイクスルー（下記）で、root から駆動すればキャラメイキングは機能し味方ユニットが生成されると判明**＝
「味方が出撃しない」のは harness の entry-point 起点が誤っていたためで、エンジンのキャラメイキング自体は壊れていない。

**構造的な核（2026-06-16 確定）**: 各 .eve は**自分の `プロローグ:` を持つモジュール**で、`プロローグ` ラベルは **13 個**ある
（root スパロボ戦記.eve / Main.eve / CMaking.eve / Shop / Warehouse …）。キャラメイキング本体は `lib/CMaking.eve`
（`プロローグ:`→`召喚画面:`→`名前入力開始:`→`召喚キャラデータ作成:`）。**ところが verify-archive の entry-point スコアラは
`Main.eve`（戦闘モジュール）を選ぶ**ため、smoke drive は Main.eve の `スタート`（戦闘）へ直行し、キャラメイキングの入口に一切入らない
（drive 中 Menu/Ask/Input が 0 件）。ブラウザは root スパロボ戦記.eve 起点で正しい流れに乗るため「味方 1 体」に到達できていた（doc 旧記述と整合）。

### （参考・解決済の経緯）2026-06-16 第2ブレイクスルー: root から駆動するとキャラメイキングは機能し**味方ユニットが生成される**

新ツール `VERIFY_ENTRY=@N`（登録 .eve の 1 始まり index。**日本語 env は文字化けするため index 形式が必須**。root は **@2**）＋
`VERIFY_AUTOSTART=1`（メニューの `【開始】/【START】` 進行アクションを優先）で root(@2) から駆動した結果:

```
Briefing → Title [【START】|…|真ゲッター/マジンガー/…] → 難易度設定 […|【開始】] → Confirm「この設定で開始しますか？」
        → 機体選択 [ガンダム|マジンガーＺ|…大量] → ガンダム[機体能力を確認する|決定する] → ★ units 0→1（味方ユニット生成！）
```

**→ 旧説「キャラメイキングがパイロットを乗せない」は否定。エンジンのキャラメイキングは機能し、味方ユニットを生成する。**
真因は **harness が entry-point に Main.eve（戦闘）を選び、root を起点にしていなかった**こと。`VERIFY_ENTRY=@2 VERIFY_AUTOSTART=1` で再現。

**（解決済の経緯）当時「残ブロッカー」とした点と、その後の決着**:
> 以下 1.〜3. は当時「次セッションのブロッカー」として記録したが、いずれも §2 冒頭の解決（`VERIFY_AUTOSTART` でメニューを
> `respond_dialog` 直接確定）で**解消済**。harness のクリック座標ずれが原因で、エンジン側の不具合ではなかった。経緯として残す。
1. **キャラメイキングの本体 `召喚画面`（CMaking.eve）は標準 `Ask`/`Menu` ではなく独自のグラフィカル click UI**:
   `Do … Wait Click … Loop While (選択="" And Not KeyState(16) And Not KeyState(2))` ＋ `PaintString` で描いたボタン ＋
   `Switch 現ダイアログ`（名前入力→姓入力→性別選択→愛称設定→パイロット画像設定→決定）で遷移する。**KeyState(16)=左クリック / KeyState(2)=右クリック**を読む。
   → smoke drive の generic `respond(0)` では**この独自 click UI の正しい領域をクリックできず完走できない**（機体選択は標準 Ask なので通り units 0→1 するが、
   その後のパイロット詳細作成画面で詰まる。`機体[確認する|決定する]` の `決定する` ループも、決定→召喚画面→drive 完走不可→確認再提示、の現れと推測）。
   → 次手 **(a)** drive に「独自 click UI を座標クリック＋KeyState を立てて駆動する」専用モードを足す（`PaintString` のボタン座標を読むか、
   各 `現ダイアログ` 段階で妥当な領域をクリック）。**(b)** あるいは `召喚キャラデータ作成`（CMaking.eve L746）を直接呼ぶショートカット経路を試す
   — このラベルは `召喚キャラ[名前/姓/ミドルネーム/性別/愛称/画像/…]` を読んで stats を生成するので、**drive に「script_var を既定値で set してラベルを
   call する」機構（例 `VERIFY_SETVAR`/`VERIFY_CALL` env）を足し**、それらの 召喚キャラ[*] を埋めて呼ぶ。ただし `決定` ステップは本ラベル呼出以外に
   フローの遷移（搭載/出撃準備）も行うため、ラベル単独呼出で出撃まで進むかは要検証。**(c)** エンジン側の `Wait Click`/`KeyState` — **調査済**:
   `KeyState(16)` 等は呼び出し 4 回で自動的に "1"(押下) を返す anti-freeze がある（`event_runtime.rs:9014` `KEYSTATE_AUTO_BREAK_THRESHOLD=4`、
   `KeyState(2)`=右クリックは take_wait_click_right のワンショット）。**→ 独自 UI の `Loop While (… Not KeyState(16) …)` はヘッドレスでも
   auto-break で進む**（ただし入力は既定/空のまま各 `現ダイアログ` 段階を高速通過し `召喚キャラ[名前]` 等が空→ハッシュ生成名になる）。
   **残る詰まり = 機体選択メニューのループ**。動的構築で grep 不能だったが、**新設の対話発生元計装で源を特定**（`App.exec_pc` ＋
   verify-archive が `script_library.labels`/`eve_entries` 逆引き）→ **root `スパロボ戦記.eve` の `機体選択開始` ラベル**（engine デコーダで
   `VERIFY_DUMP_PATH="戦記.eve"` ダンプして判読。iconv SHIFT_JIS は一部文字化けする）。**確定したフロー**:
   ```
   機体選択開始:                                  ← 機体リスト Ask で 入手ユニット を選ぶ
     Ask "$(愛称)" キャンセル可 / 機体能力を確認する / 決定する   → Switch 選択
       Case 0 (キャンセル)            → Goto 機体選択開始 (loop)
       Case 1 (機体能力を確認する)    → Create 味方 仮 + Call 機体確認 + RemovePilot + Goto 機体選択開始 (loop)  ← ここで units 0→1 (仮表示)
       Case 2 (決定する)              → Confirm "…でいいですか？" → If 選択=1 (はい) Create 味方+...+サブ機体選択開始 へ / Else Goto loop
   ```
   **エンジン知見（重要）**: `Confirm` の応答は **`respond_dialog(0)`→`選択=1`(はい) に反転**（`app.rs:1036`、`0=>"1"`）。
   従って drive の Confirm 既定 0 は「はい」で正しい。**にもかかわらず `決定する`(Case 2 へ) を選ぶと機体メニューがループし、`Confirm` に
   到達せず機体リストが重複増殖する**（`確認する`(Case 1) を選ぶ baseline は units 0→1 する）。→ **次手**: Case 2 が `Confirm` に届かない理由を
   切り分ける（drive のクリック座標が `決定する`(option 2) を外している疑い＝`click(120,334)` 命中判定、あるいは `Ask` 結果 `選択` が "2" にならず
   `Switch` が Case 2 に入らない疑い）。verify-archive の Menu クリック座標ロジック（選択肢 y=304+行*20+10）と `Ask` の `選択` 格納（`store_value`/
   `option_keys`/index）を突き合わせる。これが通れば 決定→サブ機体選択→…→出撃→敵配置 まで一気に検証できる。
2. キャラメイキング完走後に味方が出撃すれば `味方数`/`味方平均レベル`/`敵候補`/`敵陣営` が埋まり、**そこで初めて「敵出撃」を検証**できる。
   （敵出撃は味方出撃の後段。順序: ① キャラメイキング完走 → ② 出撃 → ③ 敵配置の検証）。
3. **再現コマンド**: `cd crates/src-web/tests/fixtures && zip -rq /tmp/sparobo.zip スパロボ戦記 && cd -` →
   `export VERIFY_SMOKE=1 VERIFY_DRIVE=1 VERIFY_ENTRY="@2" VERIFY_AUTOSTART=1 VERIFY_VAR="味方数,敵候補"` →
   `cargo run -q -p verify-archive --bin verify-archive -- /tmp/sparobo.zip`（`units 0→1` を grep で確認）。

### （旧）敵配置コードの参考（核は上記に更新済）
- `Main.eve` `敵配置:` `For i = 1 To Args(11)` → `set 敵候補確定 Lindex(敵候補, Random(Llength(敵候補)))` → `Create 敵 敵候補確定 …`。
  `敵候補` 空なら `Create 敵 ""` で生成されない（上で確認済）。`敵候補` は `特殊増援候補作成`（Main.eve L462〜520）が構築。
- 式中ユーザ関数 `Call(ランク算出,…)` の機構は `call_label_sync_for_condition`+`enter_call_args` で実証済だが、Create のランク引数は実際には
  無視されるため敵未出撃の主因ではない（VERIFY_VAR で確認済）。
- 旧ブラウザ手順（`__srcVar`）は不要になった（VERIFY_VAR で代替）。実機 84MB zip は各自取得のままだが triage には fixture で足りる。

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
| 戦記-CM | **スパロボ戦記 進行不能＝キャラメイキング**（§2） | ✅✅✅ **解決済（2026-06-16）**。`VERIFY_ENTRY="@2" VERIFY_AUTOSTART=1` で root から駆動するとキャラメイキング→敵候補生成→戦闘配置まで完走（味方 2 vs 敵 7=ガンセクト6＋ボス1）。「進行不能」は**完全に harness のドライブ制約**（entry-point に Main.eve を選ぶ・autostart 時のクリック座標ずれ）で、エンジン修正は不要だった。✅ **戦闘不成立の真因も特定＋drive 自動化**: 機体は意図的に `パイロット不在` 生成、パイロットは `キャラクターメイキング` で作る設計。drive 自動化を実装し**パイロット作成・ロスター追加まで実証**（`e6a8c36`）。✅ **エンジンバグ修正**（`865844c`）: `Input` の配列 lvalue 値展開で 2 人目以降の名前入力が固まる不具合を `resolve_lhs_name` で解消（回帰テスト付・一般修正）→複数パイロット作成可。✅ **戦闘成立も実証**（`VERIFY_SEAT_DEBUG`/`debug_seat_db_pilot`＝無人機に DB パイロット搭乗→D マップで攻撃/反撃/Defeat 完走、`16caf45`）＝combat 健全。残（正規プレイ通しのみ）: CMaking の正規 exit（headless 不可・要セーブ）＋ロスター→搭乗。詳細 §2 |
| 戦記-関数 | 式中ユーザ定義関数 `Call(ランク算出,…)` | 機構は `enter_call_args` 実証済だが再入リスク。VERIFY_VAR で**敵未出撃の主因でないと確認済**（Create のランク引数は無視）。CM 実装後に必要なら着手 |
| 戦記-Info | `Info()` サブクエリ | パイロット系（性別/性格/最大ＳＰ/特殊能力所有 等）は実装済と確認。`配置場所[7]`/`敵配置数` が正常に出ることから Info(マップ,…) も機能。CM 完走で追加要否を再判定 |
| 必要技能 | ✅ **必要技能ゲート＋動的化（2026-06-16 完了）** | `(念力Lv3)` を `necessary_skill` で評価し武器/アビリティ可否ゲートに配線。撃墜数の戦闘中加算・習（ラーニング）も実装済で動的に武器解禁。残（軽微）: 未達武器の画面非表示（frontend）・ステータス閾値は fail-open 据置（§1.1） |
| 魅了/憑依 | **勢力/支配の切替** | ✅✅✅ **実装完了（2026-06-16）**。`combat::weapon_possession` ＋ `UnitInstance.charm_revert_party` ＋ `apply_weapon_special_effects` 配線。魅了=3T 一時・魅了主を護衛する味方として行動（ai_mode=護衛）・begin_phase で期限切れ復帰 / 憑依=恒久支配・BossRank 免疫・`spirit_command_options` で SpecialPower 封じ。synthetic test 4 件。詳細 §1.1 |
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
> **2026-06-16 追加（D triage 用）**: `VERIFY_VAR="a,b,c"`＝各ステップで script_var をダンプ（ブラウザ `__srcVar` のヘッドレス相当）。
> `VERIFY_ENTRY=@N`／`<部分一致>`＝entry .eve を上書き（**日本語名は文字化けするため `@N` index 形式が確実**。root スパロボ戦記.eve=`@2`）。
> `VERIFY_AUTOSTART=1`＝メニューの `【開始】/【START】/決定する` 等の進行アクションを優先し対話を `respond_dialog` で直接確定。
> `VERIFY_AUTOPLAY=1`＝Battle の味方フェイズに味方を AI で前進・攻撃させてから EndPhase（`App::debug_run_phase_ai`）＋戦闘開始時に
> `App::debug_firable_report`（pilot 解決/combat_data/武器発射可否）を出力。**env は `export` で渡す**（インライン `VAR=x cmd` は nix shell 経由で届かないことがある）。

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
│   ├── necessary_skill.rs     ← 必要技能/必要条件 ((念力Lv3)) の評価器（split_necessary / is_satisfied）
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
# D スパロボ戦記をヘッドレス triage（__srcVar 相当）。env は export で渡す。
cd crates/src-web/tests/fixtures && zip -rq /tmp/sparobo.zip スパロボ戦記 && cd -
export VERIFY_SMOKE=1 VERIFY_DRIVE=1 VERIFY_ASK=1 VERIFY_VAR="敵候補,敵陣営,味方平均レベル,配置場所[7]"
cargo run -q -p verify-archive --bin verify-archive -- /tmp/sparobo.zip   # 旧 unset でリセット
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

- **魅了/憑依（特殊効果攻撃属性 魅/憑）を spec 準拠で実装**: `combat::weapon_possession`（`憑`→憑依 / `魅`→魅了 検出）＋
  `UnitInstance.charm_revert_party: Option<Party>`（魅了の元勢力退避、serde default）＋ `apply_weapon_special_effects` の proc 成功分岐へ配線
  （BossRank 免疫・`firer != def` ガード）。**憑依**=攻撃側勢力へ恒久支配＋condition `憑依`(永続)、`spirit_command_options` で SpecialPower 封じ
  （プレイヤー UI / AI 精神の唯一の関門）。**魅了**=元勢力退避→攻撃側勢力へ移し `ai_mode="護衛 <firer_uid>"`＋condition `魅了` lifetime=4、
  `begin_phase`（tick 直後）で期限切れ復帰（元勢力へ戻し護衛 ai_mode/退避フィールドをクリア）。ChangeParty の `u.party=` 基盤を再利用。
  synthetic test 4 件（恒久支配・ボス免疫・SpecialPower 封じ・一時魅了の勢力反転＆敵対関係再評価＆期限切れ復帰）。in-repo fixture に 憑/魅 武器が
  無いため synthetic 検証（§3 ゲートと同様）。
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
- **形態の必要技能ゲート（§4・完了）**: `UnitData` の `必要技能=`/`不必要技能=` を `form_skill_ok` で評価し **変形**（`resolve_transform`）・
  **換装**（`apply_equip_swap`）・**乗り換え**（`apply_ride_change`、swap→判定→不可なら revert）・**合体**（`apply_combine`、merged 構成員で
  非破壊事前チェック）に配線。`.eve Transform` 等のシナリオ駆動は非ゲート。宣言の無い形態は no-op。分離は合体前の有効形態へ戻るため
  非ゲート。テスト 3 件。✅ **§3 ユニット用特殊能力の必要技能ゲートも実装済**（`populate_active_features` で `特殊能力名=値 (必要技能)` を
  評価し未達なら `is_active=false`、スペース区切り必須の `split_feature_necessary`、回帰2件）。共有ヘルパ `gated_active_features` で**変形/合体/換装/
  form-change の全経路へ一貫適用**。**残**: §5 アイテム（Equip バイパス＆交換 UI 無しで適用点なし＝原典も交換 UI 限定なので N/A）。
- **周辺の原典準拠化（同セッション）**: ① **乗り換えを Option コマンドで有効化**（`Option(乗り換え)` 有効 AND 2 機以上、`乗り換え.md`。
  Option は実装済だった）。② **Ｄ属性の気力吸収**（低下分の半分を攻撃側へ、`weapon_morale_absorbs`＋firer_idx）。
  ③ **マップ兵器の撃破で「復活」を尊重**（`revive_if_possible` を pub(crate) 化し map_attack へ、Tier0 残）。
  ④ **毒/死の宣告を実効最大HP基準**に（改造/強化パーツ/ボスランク反映、Tier0 残）。
  ⑤ **盗属性のアイテム盗み**（相手の `アイテム所有`/`レアアイテム所有` を攻撃側へ、資金スキップ）。各テスト付き。
- **D スパロボ戦記の triage 基盤＋エンジンバグ修正**: ① 検証ツール（`VERIFY_ENTRY=@N`/`VERIFY_AUTOSTART`/`VERIFY_AUTOPLAY`/`VERIFY_VAR`＋
  `App::debug_run_phase_ai`/`debug_firable_report`＋対話発生元計装 `App.exec_pc`）でブラウザ無しに D をヘッドレス triage。② キャラメイキングを
  drive 自動化（インターミッション選択→`召喚画面` の HotPoint→Menu 化を使い `名前入力`→一意カタカナ名→`決定`、能力閲覧は cancel）し**複数
  パイロット作成を実証**。③ **`Input` コマンドの配列 lvalue バグを修正**（代入先を値展開せず `resolve_lhs_name` で格納キーに解決、`865844c`、回帰
  テスト付き）＝2 人目以降の名前入力が固まる真因。一般バグ（配列変数へ繰り返し Input する全シナリオに波及）。④ **D 戦闘の成立を実証**
  （`VERIFY_SEAT_DEBUG`/`App::debug_seat_db_pilot` で無人機に DB パイロットを乗せ、D マップで攻撃→反撃→ダメージ→Defeat 完走、`16caf45`）
  ＝combat エンジンは健全と確定。⑤ **`LoadFileDialog` 実装**（`5c4bba1`）＋**`expand_vars` のクオート内 `name[expr]` 展開バグを修正**
  （`c930c55`＝クオート内の配列参照が同名変数値に化けリテラルを壊す一般バグ、回帰テスト付き）＝データロード行検出の真因。残は CMaking の正規
  exit（データロードは exit でなく `Goto 召喚画面` でループ、真の exit `Case ＥＸＩＴ` は未解明、§2 ⑥）。⑥ **他シナリオ駆動でマップ範囲外
  クラッシュを発見・修正**（`b90e40f`）: `MapData::cell/set_cell` が宣言サイズ外の座標で **index out of bounds パニック＝WASM アプリ全体クラッシュ**。
  TukabarkSampleScenario01（テイルズ系）が 15x15 マップに (19,19) 等へ敵配置し、敵フェイズの `cell()` でパニックして**戦闘が一切進まなかった**。
  範囲外は既定セル返し/no-op に堅牢化（回帰テスト付き）→ 同シナリオの戦闘（攻撃/反撃/クリティカル/撃破/精神/マップ兵器）が完走、**D に続き
  2 本目の実シナリオで combat 健全を実証**。併せて drive を「Battle なのに非 MapView なら Advance」に拡張（`463ecb6`、Title/Configuration で停留する
  シナリオの救済）。⑦ **3 本目の実シナリオ `スーパーヒーロー伝説`（らんま系）を Briefing→Battle→Victory まで完走確認**（バグ無し・debug bypass 無し。
  `乱馬→校長[猛虎高飛車]: 撃破！EXP+190 資金+10000`、反撃/クリティカル/`【勝利】敵を全滅させました` まで正常）＝**最もクリーンな end-to-end 検証**。
  本セッションで実エンジンバグ計 **4 件**修正（pilot.txt カンマ形式・`Input` 配列 lvalue・`expand_vars` クオート内展開・マップ範囲外クラッシュ）、
  実シナリオ **3 本**で combat 健全を実証（D／テイルズ／らんま）。

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
