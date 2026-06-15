# 現在の作業状況 (Session Handoff)

> **2026-06-10 追記**: 進行制御のアーキテクチャ再設計を開始。診断と 3 段階移行計画は
> [`docs/FLOW_REDESIGN.md`](FLOW_REDESIGN.md) を参照。**Phase 1（完了プロトコル +
> FlowCont 継続スタック）実装済み**: `start_battle_phase_after_inline_load` 削除、
> `スタート` 通過は `stage_start_ran`（事実）で判定、suspend 中 `begin_phase` による
> ターン 1 イベント消失バグ修正。**Phase 2 一部実装済み**: 原典 EventQue 相当の
> 割込みイベントキュー（`post_event_label` / `script_depth`）を導入し、破壊・全滅・
> 勝敗・ターン等の auto-fire を再入 `trigger_label` から投函式に置換（ctx 上書き
> ハザード解消・suspend するハンドラ対応）。さらに `Continue <file>` をエンジン内
> チェイン化（`FlowCont::LoadNextStage` + `scenario_transition_reset`）し、戦闘中の
> `Continue` が誰にも消費されず停止するギャップを解消。テスト 1686 件パス。残りは
> 実機検証後の idle 述語 shim 削除（`docs/FLOW_REDESIGN.md` §4 Phase 2 残り）。

> **2026-06-14 追記 (本セッション)**: §0.8 Tier 2「プレイヤー向けユニットコマンド」配線に着手。
> **修理 / 補給**（特殊能力 修理装置 / 補給装置 ベース。隣接味方を対象選択し HP 全回復 /
> EN・残弾全回復＋気力 -10、発動で行動終了。`a8688e0`）と **変形**（特殊能力 変形。変形先 1 つは
> 即変形・複数はサブメニュー、移動前のみ・行動非消費。`ebf92f7`）をユニットメニューに追加。いずれも
> 既存の効果経路（`spirit_heal_full` / `spirit_resupply` / `.eve Transform` 本体）を共有して実装。
> さらに **チャージ**（Ｃ属性武器を持つユニット。発動で行動終了し次ターンに解禁。戦闘側の `charged`
> 解禁判定は既存。`b923a92`）と **回復系特殊能力**（ＨＰ回復/ＥＮ回復/ＨＰ消費/ＥＮ消費 Lv* を
> `begin_phase` で毎ターン 10×Lv% 増減。`feature_level` ヘルパ新設。`bc1d993`）を追加。
> さらに **アビリティ proper（BLOCKER 級）を 2 フェーズで実装**: Phase A=パーサ（`===` 区切り以降の
> アビリティセクションを `AbilityData`/`UnitData.abilities` にパースし `UnitInstance.abilities` を
> Create/Place で populate。`4b80ea7`）/ Phase B=プレイヤー操作（`UnitMenuItem::Ability` 一覧サブメニュー
> ＋×表示＋射程対象選択 `ActionMode::AbilityTarget`、回数/EN/気力 消費、効果 回復/補給/気力増加/治癒/装填/
> 再行動/状態/付加。`758d9b6`）。
> さらに **インターミッション組込コマンドを一通り完成**: **換装**（`換装` 特殊能力の (ユニット→換装先)
> リストから形態差し替え。`set_unit_form` を変形と共有。`d30495c`）/ **乗り換え**（味方 2 機の搭乗
> パイロット pilot_name+pilot_ids を 2 段階選択で交換。`fd70458`）/ **ステータス**（既存ロスター画面
> PilotList→UnitList を再利用し `scene_return_to` でインターミッション復帰。`7279e89`）。
> → 組込み 機体改造/換装/乗り換え/ステータス/データセーブ が揃った。
> さらに **精神 突撃/捨て身/直撃 の戦闘実装**（`1f9c59e`）・**アビリティ効果 変身/SP回復 追加**（`ef478c2`）。
> さらに **重い領域「合体/分離/搭乗」に着手し多ユニットモデルを構築**: **母艦**（`stored_units`/`stored_in` を
> `UnitInstance` に追加。`発進` コマンドで格納ユニットを隣接出撃、毎ターン HP/EN 50%回復+弾/回数全快。`8e109b1`）/
> **合体・分離**（`combined_from`/`pre_combine_form`。合体=2マス内の相手を温存 off_map して合体形態へ変身・行動終了、
> 分離=構成ユニットを隣接復帰+元形態へ。状態・パイロットを温存する真の合体。`1359deb`）。
> テスト **1766 件**緑・clippy clean・wasm check OK・**コミット済**。
> さらに **搭載/合体のムーブ統合**（母艦・合体相手の上へ移動で搭載/合体。`895e65b`）で母艦/合体の操作ループ完成。
> **合体/分離/搭乗 の残り（MVP の限界）**: 3機以上ルール未区別・着地点は隣接自動・最初から合体形態のユニットの
> 分離（構成無し）・合体時のパイロット統合（全員搭乗）。
> さらに **アビリティ召喚** (`12231d0`)・**修理装置 Lv 別回復率** (`5f7b63f`)・**修理/補給 経験値** (`2597162`)。
> テスト **1771 件**緑。

> **2026-06-15 追記**: §「★ 残課題サマリ」B クイックウィンを 4 件消化。**MapWeapon を MapAttack の
> 別名として実装**（旧名称シナリオのマップ攻撃が無反応だったのを解消。`4dd53e6`）/ **マップコマンド
> 「作戦目的」を配線**（dead API だった `fire_victory_condition_event` を `勝利条件:` ラベル定義時のみ
> メニュー表示・選択で発火。`4ada399`）/ **移動不能/足止め/捕縛は対応済と確認**（`move_disabled()` が
> 網羅・`unit_move_range` が読む。コード変更不要）/ **`.eve CallIntermissionCommand データセーブ` 実体化**
> （ログ stub →メニュー経路と共有。`ebabf15`）。回帰テスト 3 件追加。`master` から feature ブランチ
> `feat/quickwins-mapweapon` 上。test/clippy/wasm-check 全緑。B 残りは「ステータスの単機詳細化」と
> 「乗り換えの Option ゲーティング」（いずれも MVP 拡張で quick win ではない）。

> **2026-06-15 追記 (続き)**: §「★ 残課題サマリ」A 精緻化に着手。**A1 アビリティ未対応効果 4 種を実装**
> （強化/能力コピー/M型マップ/敵対象。`apply_ability_effects` の `_ => {}` 解消＋対象判定を敵対象へ拡張。
> `apply_ability_area`/`apply_ability_attributes`/`copy_size_ok` 新設、`Size::rank/step_diff`・`AbilityData`
> ヘルパ追加。`bb8ebc5`）/ **A2 合体時のパイロット統合**（構成ユニットのパイロットを合体形態へ集約・分離で
> 復帰。`pre_combine_pilots` 追加。技能ボーナスが戦闘に反映。`2c81342`）/ **A4 特殊効果攻撃属性を反撃/援護
> でも proc**（`try_counterattack`/`try_support_attack` の命中・生存分岐に追加。`1d23396`）／**A4 proc が
> crit を置換**（特殊効果武器は通常クリティカルしない。`0ab3e33`）／**A4 能力補正系属性**（低攻/低運 を
> `weapon_special_effects` に追加＋攻撃力/運動性 ＵＰ/ＤＯＷＮ 状態を combat へ反映。`9fe72ec`）。回帰テスト
> 11 件追加。A2 残は 3機ルール/最初から合体形態の分離/着地点選択、A4 残は残る CC属性(AI挙動絡み)/耐性弱点。
> test/clippy/wasm-check 全緑。

> **2026-06-15 追記 (続き2)**: 「着手しやすい・干渉しない順」で残課題を消化中。**A3 修理/補給の経験値を
> 対象レベル依存に**（SRC `Unit.cs::GetExp` の倍率テーブル。EN コスト/補給全快は原典上 feature 版対象外と
> 確認。`9756f65`）／**A4 命中率低下系 盲/撹 を追加**（盲目=攻撃側命中×0.5・被命中×1.5 / 撹乱=攻撃側命中×0.5。
> `e61ad98`）。回帰テスト 5 件。**耐性/弱点は属性データモデルが無く（combat.rs に「該当データを持たない」と明記）
> 新設が要る＝大規模**のため後回し。残りはいずれも大/設計要（残 CC 属性は AI 挙動絡み、A2 残はエッジ/UI）。
> test/clippy/wasm-check 全緑。

> **2026-06-15 追記 (続き3 — /goal 積み残し消化)**: 戦闘式に閉じる CC 属性をさらに消化。**害(回復不能)**=
> 特殊能力/地形の自然回復を `begin_phase` で阻害（`5f295b8`）/ **ゾ(ゾンビ)**=アビリティ/精神/修理補給の能動
> 回復を `recovery_blocked` ヘルパで阻害（`5f295b8`）/ **黙(沈黙)**=術/音 武器・アビリティを `is_weapon_available`
> /`ability_usable` で封じる（`b057b00`）/ **狂(狂戦士)**=与ダメ×1.25・被命中×1.5（`118998f`）。回帰テスト 9 件。
> 残る CC 属性（魅/恐/憑=AI制御、吹K引転=位置移動、衰滅盗習写化=クリティカル特殊、脱D=気力、低移=移動モデル、
> 耐性/弱点=属性データモデル）はいずれも condition 付与の枠を超えた別系統機構が要るため設計フェーズ案件として整理。
> ブランチ `feat/cc-attrs-recovery`。test/clippy/wasm-check 全緑。

> **2026-06-15 追記 (続き4)**: 位置移動系・クリティカル特殊の CC 属性まで踏み込んで実装。**バリア中和(中)**
> `fd3c94b` / **踊り(踊)** `2b4c610` / **HP/EN減衰(衰/滅)** `b871373` / **ノックバック(吹/Ｋ)** `cdbce74` /
> **引き寄せ・強制転移(引/転)** `48f35f1`。位置移動は `weapon_knockback`/`weapon_crit_reposition` ＋ `apply_*`
> で盤外/占有停止・XL/移動力0不発を実装（衝突ダメージは未モデル）。さらに **気力減少(脱/Ｄ)** `6c1b189`。
> **condition 付与・位置移動・クリティカル減衰・気力減少の機構で実装可能な CC 属性をほぼ網羅**。残る CC 属性は
> 写化(発動者変身)・盗習(資金/ラーニング)・即告(BossRank 要)・低移(移動コスト)・魅恐憑(AI 制御)・
> 耐性弱点(属性データモデル) で、いずれも新しいサブシステムが必要な設計案件。スパロボ戦記の
> 「式中ユーザ定義関数」は再入＋実シナリオ(84MB ローカルのみ)検証が要るため盲目実装は interpreter 全体への
> 回帰リスク大として保留。**さらに大幅に追加**: 耐性/弱点(発動確率 `de95704`＋毒率 `e450b28`) / 弱効剋(属性付加・
> 封じ `06a4be7`) / 低移(移動力DOWN `0a22c6d`) / 写化(発動者変身 `faa4659`) / 恐(恐怖=敵AI逃走 `dea9147`)。
> **これで実装可能な特殊効果攻撃属性をほぼ全て網羅**（Ｓ縛痺眠乱凍石毒不止劣/低攻低運低移/盲撹/害ゾ/黙/狂/中/踊/
> 衰滅/吹Ｋ引転/脱Ｄ/盗/弱効剋/写化/恐）。**残りは設計判断・新サブシステムが必須**: 魅了/憑依(相手勢力の上書き＝§3 で
> SRC 準拠の意図的非対応) / 習(ラーニング技サブシステム) / 即告(kill-path＋BossRank) / 耐性弱点の与ダメージ反映
> (元素ダメージ計算モデル) / スパロボ戦記(Info網羅＋式中関数＋キャラメイキング、要 84MB 実機検証) / GBA戦闘アニメ
> (複数セッション)。ブランチ `feat/cc-attrs-recovery`、全テスト緑。push/マージはユーザ指示待ち。

VB6 製 SRC (Simulation RPG Construction) を Rust + WebAssembly に移植中。
本ドキュメントは作業継続のための要約。**解決済み課題は §9 に 1 行で要約**し、本文は
「現状・残課題・恒久リファレンス」に絞る。

**現在地（2026-06-14 セッション末）**: 直近セッションの実装ログは上の「2026-06-14 追記」を参照。  
**テスト**: `cargo test -p src-core` 全緑（**1771 件**）／ clippy clean ／ wasm `cargo check` OK。**全てコミット済**
（最終コミット `84b13eb`）。`main` には未マージ（feature ブランチ `fix/intermission-stage-progression` 上）。  
**直近で実装済み（§0.8 Tier 2/3 を広く消化）**: 修理/補給/変形/チャージ/アビリティ/発進/合体/分離 のユニット
コマンド・回復系特殊能力・アビリティ proper（パーサ+操作+効果）・インターミッション組込一式（換装/乗り換え/
ステータス）・精神 突撃/捨て身/直撃・多ユニット合体モデル（母艦/合体/分離/搭載のムーブ統合）・Lv 別修理/支援経験値。

---

## ★ 残課題サマリ（次セッション引き継ぎ）

> 直近セッションで Tier 2/3 のユニットコマンド系・多ユニット合体・アビリティ系をほぼ消化したため、
> **残りは「既存機能の精緻化」と「土台機能（スパロボ戦記系）」と「大規模（GBA 戦闘アニメ・敵 AI 深掘り）」**に
> 分かれる。優先度順（着手しやすさ × 価値）に整理する。

### A. 既存機能の精緻化（小〜中・着手しやすい）

| # | 課題 | 現状 / 必要なこと |
|---|------|------------------|
| A1 | ✅ **アビリティ未対応効果**（`bb8ebc5`） | `apply_ability_effects` の `_ => {}` を解消。**強化**（指定特殊能力を一時状態として付与＝付加と同機構。既存能力へのレベル加算は未モデル）/ **能力コピー**（発動者を射程内味方の形態へ変化＝`set_unit_form` 共有・pilot 保持＋サイズ制限 `copy_size_ok`）/ **M型マップアビリティ**（`Ｍ全`/`Ｍ投`… → `apply_ability_area` で射程内全有効対象へ適用、`Ｍ全` は盤上全体）/ **敵対象アビリティ**（`脱`/`除` 属性で `ability_target_valid` を敵対象に切替、`apply_ability_attributes` で 脱=気力-10/除=状態解除）。**残**: 除の対象状態をアビリティ由来に限定（現状は全状態クリア）・強化のレベル加算モデル。 |
| A2 | 🔶 **合体/分離の精緻化**（一部 `2c81342`） | ✅ **合体時のパイロット統合**（全員搭乗。`pre_combine_pilots` で host の元搭乗構成を保持し、構成ユニットの `pilot_ids` を合体形態へ集約。分離で各機へ復帰。技能ボーナスが戦闘に効く）。**残**: 3機以上ルール未区別（現状は 2 マス内を全合体）・最初から合体形態の `.eve` 配置ユニットの分離（構成 uid が無い→`分離` フォームから生成が要る）・着地点は隣接自動（原典は移動範囲から選択） |
| A3 | ✅ **修理/補給の残り**（`9756f65`） | ✅ **経験値を対象レベル依存に**（`support_exp_with_level_diff`＝SRC `Unit.cs::GetExp` 倍率テーブル。基準値 修理:補給=10:15、原典 100:150 比を踏襲）。EN コスト＝原典の `修理装置Lv*` 特殊能力に EN フィールドが無く、EN 消費はアビリティ版（`en_cost` 対応済）の話のため feature 版は対象外。補給の全快も原典どおり（Lv 別なし）。→ **実質完了** |
| A4 | 🔶 **特殊効果攻撃属性の残り**（§0.5、`1d23396`/`0ab3e33`/`9fe72ec`） | ✅ **反撃/援護でも proc**（`try_counterattack`/`try_support_attack` の命中・生存分岐に `apply_weapon_special_effects` を追加。撃破/復活時は付与せず）。✅ **proc が crit を置換**（特殊効果武器は通常クリティカルしない＝`attack_resolve_and_run` で `critical && !has_special_effect`）。✅ **能力補正系属性**（`低攻`→攻撃力ＤＯＷＮ ×0.75 / `低運`→運動性ＤＯＷＮ 命中回避-15。あわせて `状態=攻撃力ＵＰ/運動性ＵＰ` 等の補正状態を combat へ反映）。✅ **命中率低下系**（`盲`→盲目・`撹`→撹乱。盲目/撹乱で攻撃側命中 ×0.5、盲目側への被命中 ×1.5。`e61ad98`）。✅ **回復阻害/沈黙/狂戦士**（`害`→回復不能 / `ゾ`→ゾンビ `5f295b8` / `黙`→沈黙 `b057b00` / `狂`→狂戦士 `118998f`）。✅ **バリア中和/踊り**（`中`→バリア半減無効 `fd3c94b` / `踊`→行動不能 `2b4c610`）。✅ **クリティカル減衰**（`衰`→HP減衰・`滅`→EN減衰、Lv1=3/4…Lv3=1/4 `b871373`）。✅ **位置移動系**（`吹`/`Ｋ`→ノックバック `cdbce74` / `引`→引き寄せ・`転`→ランダム転移 `48f35f1`、盤外/占有で停止・XL/移動力0で不発・衝突ダメージ未モデル）。✅ **気力減少**（`脱`/`Ｄ`→気力 -5×Lv（省略10）を proc 時に適用、crit も置換。Ｄ の吸収は未対応。`6c1b189`）。✅ **資金奪取**（`盗`→クリティカル時に相手修理費の1/4を資金獲得、相手1体1回。アイテム盗みは未対応。`0fcb26d`）。✅ **耐性/弱点（発動確率＋毒率）**（`adjust_proc_for_resistance`＝`耐性=`/`弱点=` と武器属性一致で発動率 ÷2／×2 `de95704`、毒ダメージも 毒弱点で倍・耐性で半減 `e450b28`）。✅ **弱/効/剋**（`弱<属性>`/`効<属性>`→`弱点:属性` condition＝後続同属性の proc 倍、`剋<属性>`→`剋:属性` condition＝該当属性武器を封じる。`06a4be7`）。✅ **移動力DOWN**（`低移`→`移動力ＤＯＷＮ`＝effective_speed 半減、`移動力ＵＰ` も +1。`0a22c6d`）。✅ **写/化**（クリティカル時に発動者を対象形態へ変化、写はサイズ制限。`faa4659`）。✅ **恐怖**（`恐`→敵 AI が味方から逃走。`dea9147`）。**残（設計判断・新サブシステムが必要）**: 魅了/憑依（相手勢力の上書きが要るが §3 で SRC 準拠の意図的非対応）・習（ラーニング技サブシステム）・即/告（kill-path＋BossRank サブシステム）・耐性/弱点の与ダメージ反映（元素ダメージ計算モデル） |
| A5 | **精神 決意/気迫/希望** | シナリオ独自（東方夢想伝）で原典定義が無く確定不能。暫定実装のまま（決意→必中+熱血 / 気迫→気力+20 / 希望→必中+集中）。実シナリオの定義が判れば `apply_spirit_effect` を修正。捨て身の「反撃時無効/被弾まで継続」・直撃の切り払い/サポートガード無効化も未モデル |

### B. Tier 3 クイックウィン（小・要確認）

- ✅ **MapWeapon イベント命令** — `MapAttack` の別名として実装済（`4dd53e6`）。SRC Ver.1.6 まで
  の旧名称（`MapAttackコマンド.md` / 更新履歴2003 で改名）。dispatch を MapAttack に併記、catalog を
  Implemented 化、回帰テスト追加。
- ✅ **作戦目的のマップコマンド配線** — `fire_victory_condition_event`（呼び出し元ゼロの dead API）を
  マップコマンドメニューに配線済（`4ada399`）。`勝利条件:` ラベル定義時のみ「作戦目的」項目を表示し
  選択で発火。回帰テスト追加。
- ✅ **移動不能/足止め/捕縛** — 確認の結果、対応済だった。`UnitInstance::move_disabled()` が
  捕縛/麻痺/移動不能/足止め/凍結/石化（`ConditionEffect::MoveDisabled` 保持）を網羅し、
  `db.rs::unit_move_range` が読んで空範囲にしている。コード変更不要。
- ✅ **`.eve CallIntermissionCommand データセーブ`** — ログ stub だったのを実体化済（`ebabf15`）。
  `intermission_data_save` を `pub(crate)` 化しメニュー経路と `.eve` 経路で共有（`to_save_json` →
  `__quicksave`）。回帰テスト追加。
- インターミッション**ステータスの単機詳細化**（現状は既存ロスター画面の再利用 MVP）・**乗り換えの Option ゲーティング**（`Option` コマンド未対応のため「2 機以上」で代替中）。

### C. 敵 AI の深掘り（中〜大）

AI は機能している（`ai_runner_tick`/`ai_act_unit`）が浅い。**精神コマンド・アビリティ・マップ兵器を使わない**、
`ChangeMode` の逃亡/護衛を無視。穴ではなく深掘り余地（§3 参照）。

### D. 土台機能（スパロボ戦記系・大）— §1.1 / §3「戦記-*」

進行不能の核は解消済だが「**敵が出撃しない**」が残存。`式中ユーザ定義関数の実行`（`Call(ランク算出,…)`）/
`Info() サブクエリ網羅` / `キャラメイキング搭載` の 3 土台機能に帰着。詳細は §1.1・§3。

### E. 大規模（複数セッション）— §3

**GBA クローズアップ戦闘アニメ**（専用バトルスプライト＋固定レイアウト。dict 変数／`_GBA_*`／`Redraw` clear 等の段階移植）。

> **進め方の指針**: A（精緻化）→ B（クイックウィン）は単発で着手可。C/D/E は設計から要るので
> セッション冒頭で方針合わせ推奨。各課題の commit ハッシュ・実装詳細は memory `project_gap_audit_roadmap` に集約。

---

## 0. 本セッション (2026-06-13/14) — 監査ベースの大規模ゲームプレイ穴埋め＋東方夢想伝01 進行不能の根治

「精神コマンド/ゲームオーバーのような大穴が他に無いか包括検査して埋める」という依頼から出発。
7 サブシステムを SRC 原典（`SRC.Sharp.Help` / `SRC.NET`）と突合する監査を並列実行し、見つかった穴を
Tier 0–3 に整理して順に実装。後半はユーザの**東方夢想伝01 ブラウザ実機検証**で見つかった**進行不能の連鎖
（撃破イベント未発火・勝敗デッドロック・ソフトロック）を `__srcDebug()` で特定して根治**した。
**全て未コミット**・全テスト緑（1730 件）・clippy clean・wasm check OK。

### 0.1 ✅ Tier 1 — 精神コマンドの効果側を完成（25種中8種しか効いていなかった）

`d54339c`（前セッション）で「メニュー表示＋SP消費＋condition付与」までは動いていたが、効果を読む側が
無く `combat.rs` が解釈する 8 種（集中/必中/ひらめき/熱血/魂/気合/不屈/鉄壁）以外は SP を払って無効だった。
- **`App::apply_spirit_effect`**（app.rs）を中心に名前で分岐。対象選択が要る精神は **`ActionMode::SpiritTarget`**
  ＋ `begin_spirit_target` / `apply_spirit_to_target` で AttackSelect 同様の対象選択フローに（キャンセル時 SP 未消費）。
  対象種別は sp.txt の `target_type` 優先、無ければ組込み（`spirit_target_kind`）。
- **完全に機能**: 加速/神速（`db::effective_speed` +2/+3）、友情（全体½）/愛（全体全快）/信頼（単体⅓）、
  覚醒/再動（再行動）、補給（EN・弾全快+気力-10）、脱力（敵気力-10）/激励/鼓舞、応援（→努力）/祝福（→幸運）、
  努力（撃破経験値2倍・消費）、復活（全撃破サイトで HP 全快復活・1回消費 = `revive_if_possible`）、奇跡（複合）。
- **要確認（シナリオ独自・暫定実装）**: 決意→必中+熱血 / 気迫→気力+20 / 希望→必中+集中。原典・SRC.NET に
  定義が無く（grep でノイズのみ）東方夢想伝独自と思われる。正しい効果が判れば `apply_spirit_effect` を要修正。

### 0.2 ✅ Tier 0（要石）— 戦闘が実行時の実効値を読むように

`combat::predict*` に**生の静的 `pilot_by_name`/`unit_by_name`** を渡しており、強化パーツ・改造・レベル成長・
装甲低下/命中低下デバフ・格闘/射撃強化が**戦闘に一切反映されない**（`bonus_*` は `let _ = (...)` で破棄）大穴だった。
- **`db::effective_combat_data(idx) -> (PilotData, UnitData)`** 新設。レベル成長（`PilotInstance` または
  `total_exp`、同名取り違え回避でインスタンス固有）＋強化パーツ（armor/mobility/hp/en）＋技能・特殊能力・
  状態異常ボーナス（`combat_bonuses`）を合成。`grown_pilot` / `combat_bonuses` を純関数化。
- **全戦闘サイトを切替**: 通常攻撃（攻/防）/反撃/援護攻撃/援護防御HP閾値/AIスコアリング/`map_attack`。

### 0.3 ✅ Tier 2 — 撃破報酬（資金＋育成）

`App::award_kill_rewards(killer_idx, exp, victim_value)` 新設。撃破時に**資金**（`victim_value/2` ×
獲得資金増加 +10%/Lv × 幸運 ×2消費、**撃破側が味方の時のみ**）と**経験値**を付与し、メインパイロットの
`PilotInstance` を成長（`add_exp`+`apply_stat_growth`）＋レベルアップ時 `レベルアップ/LevelUp` 発火。
通常攻撃/反撃/援護攻撃の全撃破サイトに配線（`努力` の2倍もここで一元消費）。Tier 1 の `幸運` が有効化。

### 0.4 ✅ Tier 2 — インターミッション組込「機体改造」「データセーブ」（経済ループ開通）

- **`UnitInstance.upgrade_level`**（serde default）を `effective_max_hp/en/armor/mobility`（+定数/Lv）と
  `effective_combat_data` の武器攻撃力（+10%/Lv）に反映 → Tier 0 経由で改造が戦闘に効く。
- `App` に **`IntermissionMode`（Menu / UnitUpgrade）** を追加。`intermission_menu_items` で順序解決し組込
  「機体改造」「データセーブ」を表示（ユーザ項目 or 次ステージがある時のみ）。機体改造はユニット選択サブ
  モードで資金を払い `upgrade_level++`（上限 `db::UPGRADE_MAX_LEVEL=10`、費用 `(lv+1)*1000`）。
  データセーブは `to_save_json`→`__quicksave`。`upgrade_level` は serialize に乗り save/load・ステージ跨ぎで永続。
- 「撃破→資金→改造→戦闘反映」の経済ループが閉じた。

### 0.5 ✅ Tier 2 — 特殊効果攻撃属性（武器ヒットで状態異常付与）

`combat::weapon_special_effects(class)` が武器 class の CC 属性（Ｓ/縛/痺/眠/乱/凍/石/毒/不/止/劣、
`属性L<n>` でターン数上書き）を抽出。`App::apply_weapon_special_effects` が命中・生存時に確率（CT率+技量差/2）で
防御側へ付与。状態異常が実際に効くための地固め:
- **`begin_phase` を lifetime 減算（`tick_conditions`）に変更**（旧 `retain(!=1)` は 2 以上を永久残し →
  複数ターン状態異常が解除されなかった）。lifetime=ターン数+1（フェイズ開始 tick を吸収）。
- `condition.rs` に **凍結/石化→行動不能+移動不能** 追加、`is_weapon_available` を `attack_disabled()` 化、
  `ai_act_unit` は行動不能をスキップ、`unit_move_range` は `move_disabled()` で空範囲。
- **未対応**: CC 以外の多数属性（魅/恐/狂/盲/脱/低攻低運低移/弱効剋/吹K引転/衰滅盗習写化 等）、耐性/弱点、
  反撃・援護での proc、proc が crit を置換する原典仕様。

### 0.6 ✅ 東方夢想伝01 進行不能の根治（実機 `__srcDebug` で特定）

| 症状 | 原因 | 修正 |
|------|------|------|
| 撃破しても何も起こらない | 戦闘撃破が `撃破`/`Destruction`（原典に無い綴）を発火し `破壊 <name>`/`全滅 <party>` を発火せず → シナリオの破壊/全滅イベントが走らない | `App::fire_destruction_label` を `.eve` 経路と同じ `event_runtime::fire_destruction_labels`（破壊+全滅）に委譲して一本化 |
| 敵全滅後も Battle のまま進行不能 | `クリア` をエンジンが**どこからも発火していなかった**（参照は `check_victory` の委譲条件1箇所のみ） | `game_clear` の発火ラベル先頭に `クリア` 追加（Victory 設定で冪等）。`check_victory` の委譲条件から `クリア` 除外 |
| 敵全滅 & idle でも委譲のまま詰む | `全滅 敵`/`全滅 中立` 定義時に無条件委譲し、ハンドラが進行を解決しないとデッドロック | `check_victory`: 敵全滅 & **idle**（dialog/script/flow/event_queue 無し）なら委譲打ち切り `game_clear` で救済。`proceed_after_victory`（Enter/クリックで次ステージ/インターミッション/タイトルへ脱出） |
| マップ兵器の撃破が無反応 | `map_attack` が `remove_unit_at` のみで破壊/全滅/`check_victory` を発火せず | 通常戦闘同様に `fire_destruction_labels` + `check_victory` を発火 |
| 敵が味方を撃破して「資金 +N」 | `award_kill_rewards` の資金が陣営を見ていなかった | 撃破側が Player の時のみ資金 |
| 敗北後に操作不能で詰む | `game_over` がシナリオ `GameOver`/`GameOver.eve` の出口を持たないシナリオでフォールバック無し | `pending_game_over` フォールバック: Enter/左クリック=コンティニュー（`__restart_save`/`__quicksave` 再ロード、無ければタイトル）、右クリック/Esc=タイトル |

回帰テスト多数追加（`combat_kill_fires_destruction_and_annihilation` / `enemy_wipe_with_only_kuria_label_fires_kuria_and_clears` /
`enemy_wipe_with_unresolving_zenmetsu_falls_back_to_game_clear` / `victory_proceed_goes_to_intermission` /
`defeat_without_gameover_label_offers_continue_then_title` 他）。
注: `(no stage)` は `self.stage` 空（シナリオが `Stage` コマンド未使用）の表示フォールバックで無害。

### 0.7 ✅ UI / 診断

- **`debug_summary`（`__srcDebug()`）を拡張**: `parties=[味 敵 中 Ｎ]` / `file=...` / `victory[全滅敵 全滅中立 クリア]`
  を追加。勝敗状態・勝利ラベル定義・現ステージファイルが一目で分かり、進行不能診断はこれで確定できた。
- **ヘルプメニューに「デバッグ情報をコピー」**（index.html `mi-copy-debug` + lib.rs。`navigator.clipboard.writeText`。
  `Navigator`/`Clipboard` web-sys feature 追加）。
- **Space をターン終了から外し `Advance`（決定・メッセージ送り）に再割当**: 押しっぱなしで会話送り後に味方
  ターンが即終了→敵フェイズになる事故を根治。ターン終了はメニュー（マップコマンド「ターン終了」）のみ。

### 0.8 監査で残った穴（次セッション候補・優先度順）

- **Tier 2 ✅ インターミッション組込コマンド完成**: **換装 (`d30495c`)** =`換装`特殊能力の形態差し替え・
  `set_unit_form` を変形と共有 / **乗り換え (`fd70458`)** =味方2機の搭乗交換、2段階選択、Option 未対応のため
  「2機以上」で代替・displace でなく swap で空ユニット回避 / **ステータス (`7279e89`)** =既存ロスター画面
  (PilotList→UnitList) を `scene_return_to` で再利用しインターミッション復帰。→ 機体改造/換装/乗り換え/
  ステータス/データセーブ が揃った（ステータスは単機詳細+管理画面ではなくロスター閲覧の MVP）。
  ✅ **修理 / 補給 / 変形 / チャージ** はプレイヤー向けユニットコマンドとして配線済（`a8688e0` / `ebf92f7` /
  `b923a92`、§本セッション）。✅ **合体 / 分離 / 母艦(発進) も配線済**（多ユニットモデル `stored_units`/`stored_in`/
  `combined_from`/`pre_combine_form` を `UnitInstance` に追加。母艦=発進+毎ターン回復 `8e109b1` / 合体・分離=
  相手温存して合体形態へ・構成ユニット復帰 `1359deb` / 搭載・合体のムーブ統合 `895e65b`）。
  → 合体/分離/搭乗 のプレイヤー操作ループは一通り完成（残りは 3機ルール/パイロット統合等の精緻化）。
  ✅ **アビリティ proper（BLOCKER 級）配線済**（Phase A パーサ `4b80ea7` / Phase B 操作・効果 `758d9b6`）:
  `===` 区切り以降を `AbilityData` にパース → `UnitMenuItem::Ability` 一覧（×=使用不可）→ 射程対象選択
  （`ActionMode::AbilityTarget`）→ 回数/EN/気力 消費 + 効果（回復/補給/気力増加/治癒/装填/再行動/状態/付加）。
  MVP 未対応（無害スキップ）: 強化/能力コピー/M型マップアビリティ/敵対象（変身・霊力回復/ＳＰ回復 `ef478c2`・
  召喚 `12231d0` は実装済）。
  - **注記（修理/補給/変形の残課題）**: ✅修理装置 Lv 別回復率（30/50/100%、`5f7b63f`）・✅修理/補給 経験値
    （一律10、`2597162`）は実装済。残: EN コスト未実装・補給は全快固定。変形は表示ラベル固定「変形」（原典の
    コマンド別名は未対応）・地中エリア除外や `必要技能` による変形先制限は未対応。
- ✅ **回復系特殊能力 (`bc1d993`)**: ＨＰ回復/ＥＮ回復/ＨＰ消費/ＥＮ消費 Lv* を `begin_phase` で毎ターン
  10×Lv% 増減（`feature::feature_level` ヘルパ新設）。**基礎 EN 回復（毎ターン5）/ 霊力回復（1/16+Lv）は
  C# 要確認のため未実装**（Tier 3「毎ターン HP/EN/再生 回復が無い」の主要部を消化）。
- **Tier 1 残り**: 決意/気迫/希望 の効果確定（§0.1。シナリオ独自で原典定義が無く確定不能）。
  ✅ **突撃/捨て身/直撃 の戦闘側を実装済（`1f9c59e`）**: 捨て身=攻撃側×3ダメージ/防御側 無防備(命中100)、
  直撃=防御側 分身・バリア を無効化、突撃=移動後でも長射程武器(マップ攻撃以外)使用可。
  残ニュアンス: 捨て身の「反撃時無効/被弾まで継続」、直撃の 切り払い/サポートガード 無効化は未モデル。
- **敵AI の深掘り**: AI は機能している（`ai_runner_tick`/`ai_act_unit`）が浅い（精神/アビリティ/マップ兵器を
  使わない、ChangeMode の逃亡/護衛を無視）。穴ではなく深掘り余地。
- `.eve CallIntermissionCommand データセーブ` はログ stub（メニュー経路のみ実装）。

---

## 0′. 前セッション (2026-06-13) — 東方夢想伝(musou202.lzh) 進行不能の根治＋地形適応／次は精神コマンド

musou202.lzh（東方夢想伝）を**ブラウザ実機＋ネイティブ・ドライブで往復検証**しながら、
プロローグ→インターミッション→Stage1 戦闘までの**進行不能の連鎖を 1 つずつ根治**した。
全コミット済み・全テスト緑・clippy clean。最終 `beb5d39`。

### 0.1 本セッションで修正した不具合（コミット済み・新しい順）

| commit | 内容 |
|--------|------|
| `beb5d39` | **キャンセル可のない Ask を選択必須に**。`PendingDialog::Menu.non_cancellable` 追加。Ask Format 1 で `キャンセル可` 有無を判定し、無ければ Esc/右クリック/任意進行(choice 0)を拒否。**キャラ未選択→味方0体→即敗北**を防ぐ |
| `dedebee` | **味方0体での破綻状態を解消**。`begin_phase` 末尾で `check_victory`（味方フェイズ開始時 味方0体なら即敗北）／`handle_click_map_view` は Defeat/Victory 時クリック無効 |
| `3ca5e22` | **地形適応を実装**（SRC 戦闘システム詳細.md）。`combat::adaptation_mult`(S=1.4/A=1.2/B=1.0/C=0.8/D=0.6/-=0)・`terrain_env`・`predict_with_status_terrain`。通常攻撃/反撃/サポート/プレビューに反映。`App::terrain_env_at`。**注意: 主地形A=×1.2 で従来(実質B)よりダメージ増** |
| `b8f25e5` | **勝敗確定後に AI/フェイズ進行を停止**。`ai_runner_tick` 冒頭＋`begin_ai_phase` に Defeat/Victory ガード（シナリオ `全滅 味方`→`GameOver` 経由の Defeat でも止める） |
| `3ec6d20` | **味方全滅=一律敗北**（引き分けガード撤去）／勝利判定に**中立を敵対勢力として含める**／シナリオが `全滅 敵`/`全滅 中立`/`クリア` を持つ場合は組込み勝利を委譲 |
| `e9446af` | **WhiteIn/FadeIn=画面露出**（白/黒 Fade 除去）。`タイトルテロップ`(Lib/一括処理.eve)末尾の引数なし `WhiteIn` が全画面白を残し「白いマップ」化していた。`ScriptOverlay::remove_fades_of` |
| `fd5fe2a` | **プレーン Ask(Menu) をクリックで選択可能に**。`dialog::menu_choice_at`（canvas座標→選択肢行）。旧実装はクリック=Advance(0)=キャンセルで選べなかった |
| `3e791f7` | **章ローカル自動発火イベントを current_stage_file にスコープ**。全22章同時ロードで `ターン N 敵`等が別章へ漏れ「話が飛ぶ」のを解消。`event_queue: VecDeque<(label, Option<file>)>`＋`post_stage_event_label` |
| `0b0ab6b` | **Continue のエピローグ誤飛び**を修正（`label_pc_within_file`＝ファイル内のみ・global フォールバック無し）。**StartScenario** で `advance_to_next_stage` が先頭ラベル本体でなく `プロローグ` を優先起動 |
| `e3b2c7c` | **ロード時に全 .eve を実行する誤り**を修正。エントリポイントのみ `run_from_pc`（多章シナリオで GameOver.eve 含む全章が走り Defeat 化していた） |

### 0.2 ✅ 完了（未コミット）— 精神コマンド（SP コマンド）の実装

**ゴール（達成）**: ユニットメニューに「精神コマンド」を追加し、パイロットの SP コマンド
（集中/ひらめき/熱血/必中/気合/不屈/鉄壁 等）を発動できるようにした。戦闘側の効果
（condition）は既存（`combat.rs::predict_with_status`）に乗せた。

**実装内容**:
1. **`data/pilot.rs`**: `SpiritCommand { name, cost: Option<i32>, level }` 型を追加し
   `PilotData.spirit_commands: Vec<SpiritCommand>`（`#[serde(default)]`）を新設。`ＳＰ`/`SP`
   行（`ＳＰ, <最大SP>, <cmd>[=<cost>], <level>, ...`）を `parse_sp_line` でパース。`ＳＰ` 行が
   あれば最大 SP はそちらを優先（旧式の能力値行 8 トークン目はフォールバック）。コスト省略
   （`cost=None`）時は App 側で sp.txt（`special_powers`）→組込みテーブルの順に解決。
2. **`command_menu.rs`**: `UnitMenuItem::Spirit` variant を追加（ラベル「精神コマンド」）。
3. **`app.rs`**:
   - `open_unit_menu`: 味方ユニットで `!has_acted` かつ発動可能コマンドがあれば `Spirit` 項目を追加。
   - `spirit_command_options(uid)`: `level <= パイロットレベル && cost <= 残りSP` を満たすコマンド列を返す。
     パイロットレベルは `PilotInstance.level`、無ければ `total_exp` から算出（`effective_pilot_data` と同式）。
     残り SP は `PilotInstance.sp_remaining`、無ければ `PilotData.sp - UnitInstance.sp_consumed`。
   - `open_spirit_menu` / `resolve_spirit`: `PendingDialog::Menu` ＋ `pending_spirit`（`#[serde(skip)]`）で
     サブメニューを出し、選択で SP 消費（`consume_unit_sp`）＋`Condition::new(name, 1)` 付与。発動後は
     ユニットメニューを再表示（SRC: 精神は行動を消費しない）。`respond_dialog` 冒頭で `pending_reaction`
     と同様に `pending_spirit` を委譲。
   - `begin_phase`: 1 ターンバフ解除を「lifetime==1 を一律解除」に一般化（`clear_one_turn_conditions`
     と同規約。全精神コマンドを当該陣営の次フェイズ開始で解除）。
4. **`event_runtime.rs`**: `sp_cost_for` を `pub(crate)` 化（既定コストテーブルを App から再利用）。
5. **テスト**: `data/pilot.rs`（3 件: コスト/レベル付き列・単一・SP 行なし）／`app.rs`（2 件:
   発動→condition 付与＋SP 消費＋次ターン解除・SP 不足で非表示）。計 5 件追加。

**未実装の効果**（幸運/覚醒/奇跡/努力 等）: condition を付与するだけで戦闘効果は no-op（無害）。
集中/ひらめき/熱血/必中/鉄壁/不屈/気合/魂 は `combat.rs` が処理するため生存可能になる。

**残課題（任意・低優先）**: 敵フェイズ中の反撃プロンプトに防御系精神（不屈/ひらめき/鉄壁）の
選択肢を出す統合（現状は自フェイズで事前発動した lifetime=1 バフが敵フェイズまで持続するため
生存目的は達成済み）。`Info(パイロット, …, 精神コマンド)` 等のクエリ対応。

### 0.3 ✅ 完了（未コミット）— ゲームオーバー/コンティニュー画面

- **ゲームオーバー/コンティニュー画面＝実装済み**: 旧状態では `味方全滅`→`game_over` が
  `GameOver`/`ゲームオーバー` ラベルしか試さず、`Data/System/GameOver.eve` の実体（**`プロローグ:`
  ラベル**＝コンティニュー Ask→`選択=1` で `Quickload`→ else `GameClear`）に当たらず**何も起こらない**
  状態だった。修正内容:
  - `app.rs::game_over`: `GameOver`/`ゲームオーバー` が無ければ `GameOver.eve` 内の `プロローグ` を
    **ファイルスコープ発火**（`post_event_label_in_file`。シナリオ本編の `プロローグ`=オープニングへ
    誤飛びしないよう GameOver.eve に限定）。これでコンティニュー Ask が出る。
  - `event_runtime.rs::quickload`: `__quicksave` が空なら戦闘開始時スナップショット `__restart_save`
    （`enter_battle_state` が保存）をフォールバックに使い、再ロード JSON を `App.pending_reload` に積む。
    `restart` コマンドも同様に `pending_reload` を設定。
  - `App::take_pending_reload`（新規 pub API）: フロントエンドが取り出して `from_save_json` で self を
    置換＋`fire_resume_event`（core は self 置換不可のため）。
  - `lib.rs`: `perform_pending_reload` を `dispatch`／アニメーションループでポーリングし、コンティニュー
    （`選択=1`→`Quickload`）でステージ頭から再開できるよう配線。
  - テスト 4 件追加（プロローグ発火→Ask 表示／コンティニュー→__restart_save 再ロード要求／拒否で
    再ロードなし／Quickload 単体の __restart_save フォールバック）。
  - **未検証**: 実 musou202 のブラウザ実機でのコンティニュー往復は未確認（要手動）。`GameClear` は
    `stage_state==Battle` 限定のため、コンティニュー拒否時は Defeat 状態のまま留まる（タイトル復帰の
    完全実装は別途）。
- **「敵フェイズから始まる」**: 当方では **T1 味方フェイズ開始を確認済み**で再現せず（味方を
  正しく選択すれば味方フェイズ開始）。0体時の破綻は §0.1 `dedebee` で解消。要・具体的な再現手順か
  `__srcDebug()` 出力。

### 0.4 検証ハーネス（今セッションで追加・次セッションでも有用）

- **verify-archive のドライブモード**（`tools/verify-archive/src/main.rs`、env で起動）:
  - `VERIFY_SMOKE=1`: DB 構築＋エントリ .eve 実行（従来）。
  - `VERIFY_DRIVE=1`: **クリック連打＋tick 相当でシナリオを自動プレイ駆動**。各 dialog 種別・選択・
    ユニット生成(Party別)・ステージ状態遷移を逐次ログ。Battle idle では EndPhase＋tick で AI を回す。
  - `VERIFY_ANIMATE=1`: `animate_ai`/`animate_battle` を ON（ブラウザと同じ tick 駆動経路を再現）。
  - `VERIFY_ASK=<n>`: Menu/Ask の選択肢（既定1、`0`=キャンセル相当で味方0体の再現に使える）。
  - 例: `VERIFY_SMOKE=1 VERIFY_DRIVE=1 VERIFY_ANIMATE=1 VERIFY_ASK=1 cargo run -q -p verify-archive --bin verify-archive -- <archive>`
  - **ブラウザを開かずに進行不能/勝敗/ダメージを切り分けられる**。今セッションの全バグはこれで特定した。
- **ブラウザ実機検証の注意**（Claude Preview）:
  - canvas の `getBoundingClientRect()` が **0×0**（offscreen）→ **合成クリックは座標が壊れ無効**。
    クリック系（選択肢クリック/ユニット移動）はプレビューでは検証不可。ロジックはユニットテストで担保。
  - **RAF が間引かれ tick が止まる** → `window.__srcTick(2.0)` でタイマ/AI を手動駆動。
  - ファイル注入: lzh を `dist/` にコピー（trunk が配信）→ `fetch('/musou202.lzh')`→`File`→`#src-file`
    に `DataTransfer` でセット→`change` 発火。キーボード（Enter=Talk進行 / 1-9=Ask選択 / 矢印=カーソル/
    Intermission選択 + Enter=確定 / Space=EndPhase）で駆動。
  - musou202.lzh は各自で取得し、検証用 fixture として配置する（リポジトリには含めない）。
- **musou202 シナリオ構造メモ**: `東方夢想伝スタート.eve`＝プロローグ(難易度/キャラ選択)＋
  `IntermissionCommand`×3＋`Continue 東方夢想伝01.eve`（エピローグ無し）。01.eve は
  `マップコマンド`/`進入`/`プロローグ`/`スタート`(味方配置は キャラ選択フラグ依存)/`ターン`/
  `全滅`/`クリア`/`エピローグ`。01〜11章は ChangeMap 無し＝暗黙15×15。各章が同名ラベルを持つ。

---

## 1. 直近セッションの成果（インターミッション制シナリオの進行修正 ＋ UI ＋ 多数のバグ修正）

musou202（東方夢想伝）/ スパロボ戦記 / 温泉旅館 のような **`Continue` チェイン＋インターミッション
制シナリオ**を実機テストしながら、**進行不能の連鎖を 1 つずつ根治**した。全段階でテスト緑・
clippy clean（最終 **1678 件**）。`__srcDebug()` の `script_err`/`scene`/`stage_state` を毎回の
診断起点にしたのが効いた（盲探りで 1 度退行を出した教訓 → データ駆動に切替）。

### 進行フロー（core）

- **手動 Enter 進行＋オーバーレイの撤去**: Briefing→Sortie→Battle の Enter ゲートと
  「Enter で戦闘開始」独自オーバーレイを撤去。`auto_progress_stage_state_if_idle` 一本化。
- **auto_progress のガード是正**（複数）: ① `intermission_commands` 登録有無では bail しない
  （登録はシナリオ全体で残るため本編突入後も Briefing で詰む）。② 起動判定を `stage` 非空
  **または** `current_stage_file` 非空に（`Stage` コマンドを使わず `Continue` だけで本編に
  入る musou 系対応）。`crates/src-core/src/app.rs`。
- **begin_battle にデフォルト 15×15 マップ**: `Map.MapWidth==1 → SetMapSize(15,15)`（SRC.cs）
  準拠。`ChangeMap` を持たないステージ（musou 01〜11）が「マップが読み込まれていません」で
  止まる問題を解消。
- **`Continue` チェイン後の Briefing 停止を解消**（★今セッションの核）: 「次のステージへ」で
  ステージファイルが `スタート` を**インライン実行し中断せず終わった**場合、`tick` は idle
  Briefing で `auto_progress` を呼ばないため Briefing で固まっていた。`confirm_intermission_selection`
  に `start_battle_phase_after_inline_load`（**`スタート` を再発火せず**味方フェイズ開始＝敵の
  二重配置を防止）を追加。`Continue` で次ステージ再予約（`味方数=0` ループバック）した場合は
  尊重して MapView/Battle に強制遷移しない（`次ステージ` 非空で判定）。
- **戦闘開始時に味方ユニットへ自動スクロール**（`center_view_on_first_player_unit`）: 初期ビュー
  (0,0) 外に配置された味方が見えるように。`begin_battle` / `start_battle_phase_after_inline_load` 両方。

### .eve インタプリタ / 式評価（core）

- **`Talk` の `;`=強制改行・`:`=段階表示**（SRC Talkコマンド.md）: 生の `:;` が表示されていた。
  `;`→改行、`:`→ページ分割し、`talk_pages` でクリック応答ごとに 1 ページ送り。
- **`Wait Click` の右クリック/Esc 脱出**: SRC は右ボタン (`KeyState(2)`) でキャンセル/戻る。
  `respond_dialog_right_click`（選択="" + フラグ）＋ `KeyState(2)` を**ワンショット**化（読んだら
  消費）。これで `Do While KeyState(2) Loop`（解放待ち）が無限ループせず脱出できる。Esc も
  `Input::Cancel`→同経路に配線（トラックパッド副ボタン非依存）。`app.rs`/`event_runtime.rs`/`lib.rs`。
- **単一行 `If cond Goto label` の EndIf 誤カウント修正**: `skip_to_else_or_endif`/`skip_to_endif`
  が全 `If` を 1 段と数え、単一行 If（EndIf 無し）が外側の EndIf を食い潰していた。`if_opens_block`
  （本体が同一行に無い＝`Then` 終端）で判定。
- **括弧算術の欠落オペランドを 0 に**（SRC: 未定義数値は 0）: `Info(...)` が空に解決して
  `(500 - )` / `(500*0-500+)` の末尾/`)` 直前に二項 `+`/`-`/`*` が残ると評価できず生式が漏れて
  いた。`eval_paren_arith_value` 限定で `fill_dangling_operands` を適用（`try_eval_num` は厳格の
  まま＝配列添字の fail→fallback を壊さない）。
- **パイロット顔グラ解決**: `pilot.rs` の `bitmap` が常に `None` ハードコードだったのを `th_*.bmp,
  *.mid` 行からパース。`db.rs::pilot_by_name` に**愛称フォールバック**（`Talk 話者` は愛称が多い）。

### フロント（src-web）

- **Windows ライクなメニューバー**（`index.html` ＋ `lib.rs install_menu_bar`）: システム
  （シナリオ読込/最初からやり直す/セーブ/ロード/スロット0-9）・マップコマンド（ターン終了/部隊表/
  自動反撃/設定変更/クイックセーブ・ロード）・ヘルプ・キャプチャ。`perform_reset` を抽出して共有。
- **スプラッシュ重なり**: ロード後 `pending_dialog` があれば MapView へ（最初の Talk が PaintPicture
  を伴わず Title に残留する問題）。`archive.rs`。
- **設定変更オーバーレイ**: `enter_configuration` を対話/スクリプト中は no-op に（Talk と重なって
  操作不能になる問題）。
- **カスタムユニットコマンド実行時に overlay/Hotpoint クリア**: ステータス画面を別ユニットで再表示
  したとき前ユニットが残る問題（`invoke_custom_unit_command`）。

> **撤回した変更**: `Organize` の「パイロット不在を除外」。SRC 的には正しいが、スパロボ戦記で
> パイロット未搭載（キャラメイキング搭載問題）だと出撃 0→ループバックで進行不能になるため、
> 根本（搭載）が直るまで保留。

### 1.1 次の作業（最優先）— スパロボ戦記「敵出撃」＝進行不能の解消

現状: 「次のステージへ」で **T1 味方フェーズに到達し、味方ユニット 1 体が表示される**ところまで来た
（進行不能の核は解消）。残る進行不能は **敵が出撃しない**（勝敗がつかない）こと。次セッションは
これを最優先で。

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
     `敵候補` は `特殊増援候補作成`（Main.eve L462〜520 で `Call 特殊増援候補作成 List(…)`）が構築。
  3. `Call(ランク算出, …)` の**式中ユーザ関数呼び出し**が Create 引数で未評価。
  4. `味方平均レベル` / `Info(マップ,幅/高さ)` / `配置場所[…]` が空 → 座標/カウントが壊れる。
- **診断手順**: 壊れた戦闘で `window.__srcVar("敵配置数")` `__srcVar("敵候補")` `__srcVar("配置場所[7]")`
  `__srcVar("味方平均レベル")` を確認。空のものが根本。`Info()` 由来か `特殊増援候補作成` 由来か
  ユーザ関数（`Call(...)`/`ランク算出`）由来かを切り分けて、該当の Info サブクエリ実装 or
  式中ユーザ関数実行（§3 参照）を進める。
- **関連（同根）**: マップ名空＝`Call SubTitle "…$(Lindex(Info(ユニットデータ,マップ情報,特殊能力データ,
  マップ決定),1))"` の Info 空。メイン/サブ選択が反映されない＝キャラメイキング搭載/選択ロジック。
  いずれも「**`Info()` 網羅・式中ユーザ関数実行・キャラメイキング**」という土台機能（§3）に帰着。

---

## 2. 設計の要点（コードを触る前に把握すべき箇所）

- **ユニット識別は uid**: `GameDatabase.pos_index: BTreeMap<(u32,u32),uid>`（serde skip、load 後
  `rebuild_pos_index`）が「どのマスに誰が居るか」の単一の真実源。座標変更は必ず
  `move_unit`/`remove_unit`/`set_off_map` 経由。`unit_instances` への直接 `.x=/.y=/.push/.remove`
  は禁止（db.rs 内のみ）。
- **フェイズ/ターン**: `turn.rs` の `Phase`＝Player/Enemy/Neutral/Npc。`Turn::end_phase()`
  (Npc→Player で +1)。ターンイベント発火は `app.rs::begin_phase`。
- **敵味方関係**: `Party::is_hostile_to`/`is_ally_of`（unit_instance.rs、内部 `camp()`）。
  {味方,ＮＰＣ}/{敵}/{中立}、異キャンプ＝敵対。combat/AI標的/援護/反撃/マップ攻撃が全てこれ経由。
- **逐次 AI**: `App.animate_ai`（フロントが起動・シナリオ読込時に true）。`end_phase` は
  `ai_runner` を起動し `tick`→`ai_runner_tick` が 1 体ずつ進める。`animate_ai=false`
  （テスト/ヘッドレス）は同期一括処理（全テスト互換）。演出再生中（`battle_anim`/`move_anim`）は
  ランナー待機。
- **反撃モード**: `ai_act_unit` が攻撃直前に対象を先読み、味方かつ手動なら `begin_reaction_prompt`
  →`PendingDialog::Menu`→`resolve_reaction`→`attack_resolve_and_run(def_mode)`。`def_mode`
  ("反撃"/"回避"/"防御"/"援護防御"/"") の補正は `attack_resolve_and_run` 内。
- **戦闘演出**: `battle_anim`（攻撃結果の視覚化）/`move_anim`（移動スライド）は共に `#[serde(skip)]`
  transient。フロントが `app.battle_anim()`/`move_anim()` を読んで描画。`tick` が move→battle の
  順で進める。
- **数値引数の式評価**: 座標等は `eval_coord_u32`（→`eval_int_expr_app`→`resolve_expr_atoms`）。
  裸のループ変数・script_var・**システム変数**（味方数/レベル平均値/ターン数 等）を解決する。
  直書き数値専用の `parse_u32`/`parse_i32_at` とは使い分け。

---

## 3. 残・後続課題

| # | 課題 | 状況 |
|---|------|------|
| 戦記-敵 | **スパロボ戦記「敵出撃」** | **最優先**（§1.1）。`Main.eve 敵配置/ボス配置` が `敵候補`/`敵配置数`/`Call(ランク算出)`/`Info` に依存。空のため敵 0→進行不能。要 `__srcVar` 切り分け |
| 戦記-関数 | **式中ユーザ定義関数の実行** | `表示用撃墜数格納(搭乗員[1,1])` / `Call(ランク算出,…)` 等、ラベルを式の中で実行して `Return` 値を使う基盤。`fn_arg_value` は `&App`（読み取り専用）でサブルーチンを実行できず生式が漏れる。`enter_call_args`（`&mut App`）経由で `Label(args)` を実行する設計が要る。敵生成・ステータス表示・多数の Info 派生に効く。再入注意 |
| 戦記-Info | **`Info()` サブクエリ網羅** | `Info(パイロット,…,累積経験値/性格/性別/最大ＳＰ/特殊能力所有,成長タイプX)` / `Info(ユニットデータ,…,特殊能力データ,マップ決定/全身画像)` 等、AlphaSecond ステータス・マップ名・敵候補が使うクエリが未対応で空。`event_runtime.rs L9213 "Info"` 周辺に追加 |
| 戦記-CM | **キャラメイキング搭載/選択** | 作ったパイロットがメイン/サブとして機体に乗らず、最初に選んだ機体が出る。`lib/CMaking.eve`（1608行）＋ `仮ユニット`→実機体の搭載/乗り換え。深い |
| GBA | **GBA クローズアップ戦闘アニメ移植** | 「専用バトルスプライト＋固定レイアウト」サブシステム。`BaseX/BaseY=0` の固定画面に `_GBA_GetUnitBmpFile(UID,"構え"/"基本"/…)` で解決したユニット個別スプライトを描く。状態は dict 変数（`戦闘アニメ変数[…]`/`_GBA[…]`）、数十の `_GBA_*` ヘルパ＋`Redraw`/`Keep` の画面クリア意味論（現状 `Redraw` は no-op）に依存。**複数セッション規模・要実シナリオ検証**。着手するなら dict 変数／`_GBA_*`／`_GBA_GetUnitBmpFile`／`Redraw` clear／固定レイアウト／`AttackDemo` 段階制御 を段階移植 |
| 演出 | エフェクトセットの見栄え調整・属性別 `EFFECT_` 選択の最適化、移動の経路アニメは実装済だが滑らかさ向上余地 | 小 |
| AI | **NPC/中立 AI の優先度分離** | 標的は `is_hostile_to` で正しく分離。優先度ロジックは敵と共通。SetRelation/友好度上書きは SRC 準拠で**意図的に非対応**。明確な SRC 差別化ルールが見当たらず実装余地は限定的 |
| 手動 | スパロボ戦記の乗せ換え→戦闘通し目視 | 84MB ロードが必要な手動タスク（自動化対象外） |

### 恒久的な制約（仕様・運用メモ）

- **プレビューの RAF スロットリング**: Claude Preview は offscreen で `requestAnimationFrame` が
  間引かれ逐次 AI が自動進行しない。検証時は `window.__srcTick(0.5)` で手動駆動。実ブラウザ
  （可視タブ）では自動進行する。
- **セーブ互換は破棄**: uid 再設計でセーブ形式が変化（方針: 互換不問）。`pos_index` は serde skip
  で load 後再構築。
- **素材パックは各自取得**: `crates/src-web/vendor-assets/` に `SRC_Graph101121.zip` /
  `SRC_BA110418.zip` / `SRC_Wave091207.zip` を配置すると起動時自動読込。再配布規約のため
  リポジトリ非同梱（.gitignore、`.gitkeep` のみ追跡）。

---

## 4. 開発環境

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

> verify-archive 系はネイティブビルド（`cargo build -p verify-archive --bins`）。

---

## 5. アーキテクチャ

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
docs/                          ← CURRENT_WORK.md（本書）/ ARCHIVE_SCAN_REPORT.md / SRC_SHARP_DIVERGENCE.md
```

---

## 6. 実装済み機能サマリ

- **シーン進行**: Title → Configuration →（Intermission）→ MapView（Briefing→Sortie→Battle→
  Victory/Defeat）↔ PilotList/UnitList。
- **`.eve` インタプリタ**: 制御フロー（Goto/If/For/ForEach/Switch/Do/Loop/Break/Continue/Call/
  Return/Exit）、変数（Set/Local/Unset/Incr/`$(name)`/`Args(N)`/`name[expr]`/`&`連結）、対話
  （Talk/Confirm/Menu/Ask/Input/Wait Click）、ユニット（Create/Place/Launch/Escape/Kill/
  Transform/Combine/Split/Join/Ride/Leave/ChangeParty）、育成・アイテム・精神・待機・ステージ・
  データ宣言・VFS ファイル I/O。
- **データパーサ**: pilot/unit/item/sp/terrain/.map/.eve/animation。全角コンマ正規化・バック
  クオート・行末コンマ耐性・レコード単位の寛容パース（warn+skip）。
- **戦闘**: 命中/ダメージ/クリティカル/回避/防御/反撃/援護攻撃/援護防御/行動不能ゲート。
- **演出**: ネイティブ戦闘演出（フラッシュ/ダメージ/lunge）＋ SRC_BA エフェクトスプライト、
  AI 移動スライド。`animation.txt` 戦闘アニメ実行配線（同梱シナリオ時）。

---

## 7. テスト / シナリオ動作状況

- `cargo test -p src-core` 全緑（**1730 件**）。統合テスト binary は `crates/src-core/tests/`
  （scenarios / unit_lifecycle / call_return / loops / misc_commands 他）＋各モジュールの
  `#[cfg(test)]`。
- **アーカイブ互換性**: 全 3 ディレクトリ計 1496 本を smoke スキャン済、**クラッシュ/ロード
  中断 0**。残警告は設計どおりの寛容スキップ・既知の偽陽性・壊れ/非対応アーカイブのみ。
  scan_eve でコマンド網羅性も確認済（未実装エンジンコマンド無し）。
- **スパロボ戦記.zip（84MB）**: ブラウザ実機でタイトル→機体選択→インターミッション→
  キャラメイキング→**「次のステージへ」で T1 味方フェーズに到達し味方ユニット表示**まで確認。
  残る進行不能は**敵が出撃しない**こと（§1.1）。マップ名空・メイン/サブ未反映も同根（§3 戦記-*）。

---

## 8. デバッグ / 動作確認の小技

```bash
# 単一 .eve 実行
cargo run -p verify-archive --bin run_eve -- <path.eve>
# アーカイブ smoke / ファイル内容ダンプ
VERIFY_SMOKE=1 target/debug/verify-archive <path.zip>
VERIFY_DUMP_PATH=ファイル名.eve target/debug/verify-archive <path.zip>
# 未登録コマンド洗い出し
target/debug/extract_text /tmp/out archive/SRCシナリオ_10K～99K/*
target/debug/scan_eve /tmp/out
```

ブラウザ（`just serve` 中）コンソール: `window.__srcDebug()` / `window.__srcVar("name")` /
`window.__srcImg()` / `window.__srcTick(0.5)`（RAF スロットリング回避の手動駆動）。

スパロボ戦記のブラウザロード手順:
```javascript
const res = await fetch('/sparobosenki.zip');
const file = new File([await res.arrayBuffer()], 'スパロボ戦記.zip', { type: 'application/zip' });
const dt = new DataTransfer(); dt.items.add(file);
const input = document.getElementById('src-file');
input.files = dt.files; input.dispatchEvent(new Event('change', { bubbles: true }));
// 60〜90 秒で完了 → window.__srcDebug()
```

---

## 9. 完了済みマイルストーン（履歴・要約）

詳細実装はコードと `git log` を参照。以下は「もう触らなくてよい」既消化項目の索引。

- **インターミッション制シナリオの進行修正＋UI**（本セッション・未コミット）: §1 に詳細。
  進行不能の核（`Continue` チェイン後の Briefing 停止）を解消、`Talk :/;`・`Wait Click` 右クリック/Esc
  脱出（`KeyState(2)` ワンショット）・単一行 `If Goto` の EndIf 修正・括弧算術の欠落オペランド 0 化・
  顔グラ解決・メニューバー・各種オーバーレイ修正。残るスパロボ戦記の深部は §3「戦記-*」へ。
- **戦闘演出 #1 一式**（`5f6322c`〜`308ec59`）: ネイティブ演出 / SRC_BA エフェクト / lunge /
  animation.txt 基盤 / スクリプト実行配線。GBA 本体は §3 へ。
- **§0(2) ゲームプレイ拡張**（`d1b0411`〜`d83f0d2`）: #2 援護防御選択肢 / #3 行動不能ゲート /
  #4 クリティカル機構 / #5 AI 移動スライド。
- **互換性スキャン＋式評価修正**（`df77e69`〜`8142a8d`）: 座標のループ変数/システム変数解決
  （`eval_coord_u32`/`resolve_expr_atoms` 拡張）＋ レベル平均値実装、scan_eve ノイズ除去。
- **ゲームプレイ系の SRC 準拠再構築**（`9401c06`〜`b38b14f`, 2026-06-03①）: uid 基準の状態
  管理（pos_index）/ ターン・フェイズ再構築（味方→敵→中立→ＮＰＣ）/ キャンプ判定一元化 /
  逐次 AI 演出＋反撃モード。
- **アーカイブ互換性**（`a5c021f`〜`ea9f32c`, 2026-06-02）: データロード堅牢化（warn+skip）/
  全角コンマ正規化 / 未終了クオート寛容化（ListSplit 互換）/ unit.txt 4 フィールド対応 /
  `Loop/Do While Call(cond)` / 戦闘イベント系システム変数 / 多数の `.eve` コマンド実装。
- **§7 旧課題 7.1〜7.5 はすべて完了**（7.6 BattleAnime 拡張は本セッションの戦闘演出で実質対応、
  GBA クローズアップは §3 へ）。

---

## 10. 参照

- 元実装: `SRC_20121125/`（VB6）／ C# 移植: `SRC.Sharp/SRC.NET/`
- SRC コマンド仕様: `SRC.Sharp/SRC.Sharp.Help/src/menu.md` をインデックスに使う
- アーカイブスキャン詳細: [`docs/ARCHIVE_SCAN_REPORT.md`](ARCHIVE_SCAN_REPORT.md)
- SRC.Sharp との乖離記録: [`docs/SRC_SHARP_DIVERGENCE.md`](SRC_SHARP_DIVERGENCE.md)
- フィクスチャ: `crates/src-web/tests/fixtures/`

---

最終更新: 2026-06-14（ユニットコマンド + 回復系 + アビリティ proper + インターミッション一式 + 精神3種 + 母艦/合体/分離 session）。  
`cargo test -p src-core` 全緑（**1771 件**）/ clippy clean / wasm `cargo check` OK。**本セッションの配線はコミット済**
（…`/ 895e65b / 12231d0 / 5f7b63f / 2597162`）。  
**完了**: Tier 1 精神コマンド完成・Tier 0 戦闘実効値・Tier 2 撃破報酬/インターミッション機体改造・データセーブ/
特殊効果攻撃属性・東方夢想伝01 の撃破/勝敗/敗北の進行不能根治・UI（デバッグコピー / Space 再割当）・
**修理 / 補給 / 変形 / チャージ のユニットコマンド配線**・**回復系特殊能力（HP/EN 回復・消費）**・
**アビリティ proper（パーサ + プレイヤー操作 + 効果、BLOCKER 解消）**。  
**次セッションへの引き継ぎ（残課題）**: 冒頭の **「★ 残課題サマリ（次セッション引き継ぎ）」**（A 精緻化 /
B クイックウィン / C 敵 AI / D スパロボ戦記土台 / E GBA 戦闘アニメ）を参照。各課題の commit ハッシュと
実装詳細は memory `project_gap_audit_roadmap` に集約。
