# 現在の作業状況 (Session Handoff)

VB6 製 SRC (Simulation RPG Construction) を Rust + WebAssembly に移植中。
本ドキュメントは作業継続のための要約。**解決済み課題は §9 に 1 行で要約**し、本文は
「現状・残課題・恒久リファレンス」に絞る。各課題の commit ハッシュ・実装詳細は memory
`project_gap_audit_roadmap`（穴埋めロードマップ）/ `project_spirit_commands_status`（精神コマンド）に集約。

---

## 現在地（2026-06-21）— サンプル戦闘系特殊能力を VB6 原典で網羅実装（セッション区切り）

**ブランチ**: `feat/sample-scenario-smoke`（`master` ではない。**push 未指示**）。
**テスト**: `cargo test -p src-core` 全緑（約 2018 件）／ clippy clean（`-D warnings`）／ wasm `cargo check` OK。作業ツリーはクリーン（全コミット済み）。

**★ このセッションの最大の収穫＝VB6 原典ソースの発見**: 非再配布パッケージ `srcall-2_2_33-111106/Source/Src/`
に **VB6 原典ソース一式（`*.cls`/`*.bas`）** が同梱されている。C#（SRC.Sharp/SRCCore）より上流の最終 ground truth。
**今後の SRC 仕様確認はここを引く**（例: C# オラクルでは検証不能だった防御特性・未文書の `マップ攻撃破壊` を VB6 で確定できた）。

**この区切りで完了した実装**（すべて VB6 を ground truth に・各段ゲート緑・コミット済み。詳細は下記 §現在地(2026-06-20) の続き節）:
- **マップ兵器の発火イベント**: プレイヤー/AI 使用で 使用/攻撃/使用後/攻撃後/損傷率 を発火（通常攻撃と対称化）。撃破は原典どおり `マップ攻撃破壊`（スクリプト `MapAttack` は `破壊`）。
- **攻撃系精神の `スペシャルパワー無効化` 免疫** / **`Not` 演算子の優先順位是正**（記録済み乖離 §3 解消）。
- **自作SP「生贄→みがわり」**（`イベント=` SP効果種別＋身代わり戦闘リダイレクト、通常/反撃/援護 全経路）。
- **貫（貫通）属性**（VB6 突合監査で発見した未実装の実バグ。サンプル使用武器が装甲半減せず過少ダメージだった）。
- **防御特性監査**（弱点/吸収/耐性/無効化 が VB6 と一致を確認）。
- **再攻撃**（大鳥霞・確率的2回目攻撃）／**カウンター＋先読み**（ロイ・先制反撃）。

**到達点＝サンプルの戦闘系特殊能力は網羅完了**: 霊力/術（既存）＋ 再攻撃/カウンター/みがわり（本セッション）。
**連続行動・連続ターゲットはサンプル data にも VB6 Source にも不在＝N/A**（旧メモは別 fixture の混同と判明）。

**残（次セッション候補）**:
1. **みがわり肩代わりダメージの再計算**（低優先・近似）: VB6 は身代わりを新被弾対象として Damage を再算出するが、ポートは
   防御側向け予測値を流用（身代わりの バリア/シールド/不死身 は適用済み・差は核式の基礎装甲のみ）。完全再算出は
   `predict_with_status_terrain`(~13引数) の redirect 内重複構築が要り限界利得に不相応のため据え置き。
2. **非戦闘の sample 機能**: Welcome.eve メニュー導線の駆動 / 1ターン実プレイ通し / 画像(PaintPicture)・戦闘アニメ(GBA)・
   共有 Lib の VFS 同梱 / ブラウザ実レンダリング確認（要実機）。
3. **VB6 を ground truth にした横断監査の継続**（"原典/fixture の多様性が新種バグの鉱脈" の踏襲。貫 はこの手法で発見）。
> 詳細・VB6 機構・コミットハッシュは memory [[project_sample_scenario_goal]] に集約。

---

## 現在地（2026-06-20）— 公式サンプルシナリオ互換 + 防御能力の方針決定

**ブランチ**: `feat/sample-scenario-smoke`（`master` ではなくフィーチャーブランチ。push 未指示）。
**テスト**: `cargo test -p src-core` 全緑（約 2018 件）／ clippy clean（`-D warnings`）／ wasm `cargo check` OK。
**主題**: 非再配布パッケージ `srcall-2_2_33-111106/サンプルシナリオ`（公式サンプル）を Rust 移植で
動作させる。テストは実フォルダを **参照のみ**（無ければ skip・本文 embed なし・`srcall-*/` は `.gitignore`）。

**★ 最新の追加（2026-06-20・続き）= マップ兵器が「使用/攻撃イベント」を発火するよう是正**:
- **背景**: サンプルの目玉「マップ兵器 鳳翼天翔 のとどめカットイン」は自動インクルード
  `data/龍神機/Include.eve` の `*使用 大鳥霞 鳳翼天翔:` で使用直前の各陣営数を記録するが、
  ポートの `event_runtime::map_attack` は 破壊イベントしか発火せず **使用/攻撃イベントを一切
  発火していなかった**（プレイヤー/AI のマップ兵器使用で `*使用`/`攻撃` が不発）。
- **是正**: SRC `Unit.MapAttack(w,tx,ty,is_event)`（Unit.cs:27012/27320）準拠で `map_attack` に
  `is_event` 引数を追加。`is_event=false`（プレイヤー/AI のマップ攻撃コマンド）のときダメージ
  適用**前**に 使用イベント（武器 1 回）→ 各対象への 攻撃イベント を発火する。スクリプトの素の
  `MapAttack` コマンドは既定 `is_event=true`（非発火）で、末尾に `通常戦闘` を付けたときだけ
  `is_event=false` になる（CmdData.cs:14865）。AI 経路（`app.rs::ai_use_map_weapon`）は false で配線。
  発火で index がずれるため攻撃側/対象を uid で都度引き直す。
- **検証**: 単体 `map_attack_fires_use_and_attack_events_only_in_normal_battle_mode`（is_event 切替で
  使用/攻撃 の発火有無）＋ 実データ統合 `sample_map_weapon_use_fires_houyokutenshou_use_event`
  （鳳神機=大鳥霞 で 鳳翼天翔 マップ攻撃→`*使用 大鳥霞 鳳翼天翔:` が発火し 味方数 を記録）。
- **ダメージ後の後段イベントも配線済（続き）**: C# `Unit.MapAttack`（is_event=false, Unit.cs:28560-28688）
  準拠で、ダメージ確定後に 損傷率（生存対象）→ 使用後（攻撃側）→ 攻撃後（生存対象ごと）も発火し、
  通常攻撃経路と対称化（生存対象をループ中にスナップショット→check_victory 後に昇順発火）。
  破壊/全滅 は撃破ループ内で既発火。**マップ攻撃破壊**（Help 未記載の内部イベント）は仕様不明のため
  未実装（推測実装回避）。テスト `map_attack_fires_after_damage_events_only_in_normal_battle_mode`。
  → **マップ兵器の発火イベントは 使用/攻撃/使用後/攻撃後/損傷率/破壊/全滅 が通常攻撃と対称**。

**★ 追加（2026-06-20・続き）= 攻撃系精神の `スペシャルパワー無効化` 免疫を実装**:
- サンプル決戦1話は全敵に `SetStatus スペシャルパワー無効化` を付与し、挑発/脱力等の攻撃系精神から
  ザコを守る（1話コメント「スペシャルパワー無効化／特殊効果無効化 は必須」）。ポートは `apply_spirit_effect`
  が無効化を一切見ておらず、攻撃系精神が無効化持ちにも効いていた。
- **是正**: SRC `SpecialPowerData.cs:523-540`（TargetType 敵/全敵/任意/全 の SP は `スペシャルパワー
  無効化`/`精神コマンド無効化` 保持ユニットに効果なし）準拠で、`apply_spirit_effect` 冒頭に免疫判定を
  追加。敵対象（`SpiritTargetKind::SingleEnemy`）の精神を、対象が当該 condition を持つときは効果のみ
  無効化（SP コストは発動側で消費済み）。テスト `special_power_nullification_blocks_offensive_spirit`
  （脱力 で無効化あり=気力不変・なし=気力-10）。

**★ 追加（2026-06-20・続き）= `Not` 演算子の優先順位をオラクルと整合**（記録済み乖離 §3 を是正）:
- 式評価器の `Not` は最高優先（`parse_factor`）で束縛していたが、VB6/SRC.Sharp は比較より緩く
  `And`/`Or` より固い。`Not 1 = 2` がポート 0／オラクル 1 と乖離していた（`docs/SRC_SHARP_DIVERGENCE.md` §3）。
- **是正**: 比較（`parse_comparison`）と論理（`parse_logical`）の間に `parse_not` レベルを挿入し
  `parse_factor` から `Not` を外す。`Not 1 = 2`=1・`Not 0 And 1`=`(Not 0) And 1`=1、単項/括弧付きは不変。
  非 `Not` 式は素通りで挙動不変。括弧無しオペランド位置の `Not`（`a = Not b`）は実シナリオ/テストで
  未使用と確認（全て括弧付き）。テスト `not_binds_looser_than_comparison`（旧 characterization を更新）。

**★ 追加（2026-06-20・続き）= 自作SP「生贄→みがわり（身代わり）」の戦闘リダイレクト（A1+B1+C1）**:
- サンプルの目玉「自作スペシャルパワー」。`生贄`(SP, `イベント=生贄ルーチン`)→`data/include.eve` の
  `生贄ルーチン`→`SpecialPower 相手ユニットＩＤ みがわり 対象ユニットＩＤ`。意味は「選んだ味方が
  使用者の身代わりになり、使用者が攻撃されると1回だけ代わりに受ける」（SRC `Unit.cls:14855`）。
- **A1（身代わり関係の保持）**: `SpecialPower X みがわり Y` を `specialpower` ハンドラで特例化。保護対象 Y の
  `みがわり` condition の `data` に身代わり X の uid を格納（消費型 lifetime=-1）。SP は使用者から消費。
- **B1（戦闘リダイレクト）**: 通常攻撃のダメージ適用直前に最優先で `apply_migawari` を挿入。保護対象が
  `みがわり` を持てば身代わりへ100%肩代わり（身代わりの バリア/フィールド/シールド/不死身 を適用、HP0で
  撃破・破壊イベント・勝敗判定）し condition 消費。援護防御の先例に倣う。
- **反撃/援護経路へ展開済（ユーザ決定）**: 通常攻撃に加え、**反撃**（`try_counterattack`）・**援護攻撃**
  （`try_support_attack`）の被弾経路にも `apply_migawari` を最優先で配線（被ダメージ→身代わりへ、防御側 0）。
  援護防御は元々 みがわり が防御側で最優先のため二重肩代わりにならない。検証: `migawari_redirects_attack_damage_to_substitute_once`
  ＋`migawari_redirects_counterattack_damage_to_substitute`。
- **`イベント=<routine>` SP効果種別＋生贄発動フロー 実装済（2026-06-21）**: ① **対象種別是正**（SRC
  `SpecialPowerData.cls` Execute 準拠で `味方`=単体味方選択／`全味方`=全体。旧実装は `味方` を全体と誤判定）。
  ② **`SpecialPowerData::event_routine()`**（効果行 `イベント=<ラベル>` を解析）。③ `apply_spirit_to_target` で
  `対象ユニットＩＤ`=使用者・`相手ユニットＩＤ`=選んだ対象 を束縛しサブルーチンを `trigger_label` 実行。
  ④ **コマンド引数のシステム変数解決**（`resolve_handle_var`：実ユニットに一致しない裸の handle は同名
  script_var 値へフォールバック＝生贄ルーチンの `SpecialPower 相手ユニットＩＤ みがわり 対象ユニットＩＤ` が
  uid へ解決）。検証 `sacrifice_special_power_makes_selected_ally_a_substitute`（生贄→みがわり end-to-end）。
- **残（次段）**: 近似（肩代わりダメージは身代わりの装甲で再計算せず防御側向け値を流用）。プレイヤー UI の
  SP メニューからの実発動（対象種別是正で導線は通る）はブラウザ目視のみ。

**★ 追加（2026-06-21）= 貫（貫通）属性を実装（VB6 突合監査で発見した実バグ）**:
- VB6 原典で **防御特性（弱点/吸収/耐性/無効化）の Rust 実装が正しい**ことを確認した監査中に、**`貫`
  （貫通）属性が完全未実装**と判明。VB6 `Unit.cls:6812-6819`／C# SRC.NET `Unit.cs:11173`: `貫`=防御側装甲
  1/2・`貫Ln`=×(10-n)/10。`is_true_value` ゲートの外＝常時適用（予測にも反映）。
- **サンプルに 貫 武器が存在**（`クウィンテセンス`=`貫間`／`超高速蹴`=`突貫Ｃ`）＝これらが装甲半減せず
  過少ダメージだった実バグ。`combat::pierce_armor` を新設し `predict_with_status_terrain` の def_power
  算出（装甲×気力×Defense×適応 の前）に配線。テスト `pierce_weapon_reduces_armor_and_increases_damage`。
- **残**: `貫通攻撃` SP（攻撃側 condition も装甲半減）は未対応（サンプル未使用）。防御特性の核は VB6 と一致を確認。
- **残**: `貫通攻撃` SP は未対応（サンプル未使用）。

**★ 追加（2026-06-21）= 再攻撃（パイロット特殊能力）を実装**:
- 大鳥霞（龍神機）が `再攻撃Lv1-3` を所持。VB6 `Unit.cls:10239-10270` 準拠で、主攻撃後に
  `slevel=(直感>=相手直感?2*Lv:Lv)` が `Dice(32)` 以上なら同じ攻撃をもう一度（武器 `再Ln` 属性は
  `n>=Dice(16)`、SP効果「再攻撃」condition は無条件）。`attack_resolve_and_run` を `reattack_in_progress`
  フラグ付きで**再帰**させ、再攻撃側は使用イベント再発火と3回目の再攻撃を抑止（VB6 `begin` ラベルは
  使用イベントより後ろ＝再攻撃で 使用 は再発火しないが 攻撃/反撃 は再交戦する）。能力非保持ユニットは
  乱数非消費＝既存 RNG 列不変。テスト `reattack_skill_strikes_twice`。
**★ 追加（2026-06-21）= カウンター（先制反撃・パイロット特殊能力）を実装**:
- ロイ（キャリバーン）が `カウンターLv1-4` を所持。VB6 `COM.bas:1040-1058` 準拠で、攻撃を受ける際に
  防御側が反撃武器を射程内に持ち先手を取れるなら、**主攻撃の前に**先制反撃する。発動条件（VB6 判定順）:
  ① 攻撃側武器が `後`（後攻）／防御側反撃武器が `先`（先制）属性、② `カウンター` SP、③ `先読み` 技能
  `Lv>=Dice(16)`、④ `カウンター` 技能で使用回数残あり（`used<Lv`、使うたび `used_counter_attack` 加算・
  `begin_phase` で 0 リセット）。先制反撃で攻撃側を撃破すれば主攻撃は不発、主攻撃後の通常反撃は抑止。
  `try_preemptive_counter`＋`attack_resolve_and_run` の予測前に配線、先制で index がずれ得るため位置で
  引き直す。能力非保持ユニットは乱数非消費＝既存 RNG 列不変。テスト `counter_skill_strikes_pre_emptively`
  （先読み の base 機構も同時に実装）。
- **残（次の sample-used 特殊能力）**: **連続行動・連続ターゲット**。VB6 機構は要調査。
  みがわり肩代わりダメージの身代わり装甲再計算も残（近似）。

**★ 追加（2026-06-20・続き）= マップ攻撃の撃破を `マップ攻撃破壊` で発火（原典忠実・ユーザ決定）**:
- **VB6 原典ソース発見**: `srcall-2_2_33-111106/Source/Src/`（C# より上流の ground truth）。`Event.bas:1744` で
  `マップ攻撃破壊` は `破壊` と同列の `DestructionEventLabel`、`Unit.cls:17214` がマップ撃破された**対象**に発火。
- **是正**: プレイヤー/AI 発のマップ攻撃（`is_event=false`）の撃破は原典どおり `破壊` ではなく
  **`マップ攻撃破壊 <対象>`** を発火（`fire_map_attack_destruction_labels`、`全滅`/対象ユニット設定は共通）。
  スクリプトの素の `MapAttack`（`is_event=true`）は進行保証のため従来どおり `破壊` を発火（別個の意図的緩和）。
  テスト `map_attack_kill_fires_map_destruction_for_player_but_normal_for_script`。

**このセッションで修正した実バグ / 実装**（feature 由来戦闘修正・イベント発火基盤）:
- **特殊能力パーサが値無し裸名を全捨て**（`data/unit.rs`）: `水上移動`/`ＨＰ回復Lv1`/`分身` 等の
  `=` 無し裸名特殊能力を全ユニットで取りこぼしていた → `特殊能力` セクション配下の裸名を取り込む。
- **`Enable`/`Disable` が特殊能力(active_features)を切替えていなかった** → `feature_name_matches_base`
  で基底名一致し is_active を反転（2話 ＨＰ回復 水上ON/陸上OFF ギミックが成立）。
- **特徴由来 `分身`/`ステルス`/`バリア` が戦闘に未反映** → `push_combat_feature_statuses` で
  condition 由来 status に合流（命中-40 / ÷2 近似は既存のまま流用）。
- **`*`接頭辞 multi-token ラベル登録バグ** → `*攻撃 ＮＰＣ 敵:` が "攻撃" に潰れて発火しなかった
  のを `*` 剥がして multi-token 登録に是正。**引数無し `ClearEvent`** 実装（`disabled_pcs`＋
  `current_event_label_pc` 追跡、file-scoped 発火も停止）。
- **`特殊効果無効化=<対象>`**（防御特性）実装（`=全` で状態異常/気力低下/支配を無効化。サンプルの
  ザコは全機 `=全`）。

**★★ 防御能力の実装方針（ユーザ決定 2026-06-20）= 段階的に (A) → 将来 (B)**:
- **背景**: ポート combat は「単一 hit%／単一 damage を *予測* し 1 回ロール」モデル
  （`predict_with_status_terrain`）。`分身`=命中-40 / `バリア`=÷2 のように、SRC の「確率的完全回避」
  「属性別 1000×Lv 吸収」を **別物の近似へ翻訳**して収めている。`predict_with_defense`/`DefenseMode`
  (Barrier{strength}/Shield{chance}) は土台はあるが **dormant（本番未配線）**。
- **(A) 既存モデルへの近似畳み込み（今やる）**: 未実装の防御能力を hit%/÷2 近似へ寄せる。
  - 超回避Lv* → 回避系として命中ペナルティ（`-(10×Lv)`、SRC の 10×Lv% 完全回避を近似）。
  - シールド系（シールド/小型/アクティブ/大型）・フィールド系（フィールド/プロテクト・フィールド/
    アクティブフィールド）・バリアシールド → 既存の `バリア` ÷2 近似に合流（バリアと同じ防御クラス扱い）。
  - **既知の妥協**: 確率/属性/S防御依存を無視＝常時発動の過大評価。Lv も÷2では無視。これは
    既存の `バリア=÷2`（SRC は属性別吸収）と同じ割り切り。(B) で精緻化する。
- **(B) 本番配線 = 実装済（2026-06-20、3 増分）**: 戦闘実行段に確率/確定ロールを差し込み。
  - ① **シールド防御**（`3d26666`）: Ｓ防御技能 (`unit_pilot_skill_level(idx,"Ｓ防御")`) 依存の
    確率発動 `Ｓ防御Lv(+大型1/アクティブ2)>=Dice(16)` → 半減（小型2/3・破3/4）。全ダメージ適用
    4経路の `damage +=` 直前 `apply_shield_defense`。精/殺/浸・行動不能で不可。予測には乗せない。
  - ② **バリア吸収**（`5cee6bd`）: バリア/バリアLv* を ÷2 から外し属性別 1000×Lv 確定吸収
    （`apply_barrier_absorb`、defense_attr_matches 流用、直撃/バリア中和 で無効、完全吸収=0）。
  - ③ **分身/超回避**（`af43d74`）: 命中ペナルティ ((A)) を撤去し実行段の完全回避ロール
    （`check_dodge_feature`、分身=気力130以上50%、超回避Lvn=`n>=Dice10`≈10n%、直撃/行動不能で不発）。
  - ④ **ＥＣＭ**（`23107ff`）: ユーザ承認で予測関数に `ecm_hit_mult` を追加。App::ecm_hit_mult が
    防御側周囲を走査し 防御側味方ＥＣＭ−攻撃側味方ＥＣＭ で命中 ×(100−f×diff)% (f=Ｈ:10/他:5)。
  - ⑤ **フィールド系**（`6393623`/`9e7e744`）: フィールド=500×Lv確定吸収・アクティブフィールド=
    Ｓ防御/16確率の500×Lv吸収。解説エントリ(値「解説…」)を防御から除外 (プロテクト・フィールド単独は
    ヘルプ文＝非機能と判明)。
  - ⑥ **knobs/忠実性**（`3b3b992`）: バリア/フィールド/アクティブフィールド吸収に ＥＮ消費(値3番目)・
    必要気力(4番目) ゲート追加。既定0/0で素の能力は不変。
  - → **サンプルに在る防御能力は全て本番化完了** (シールド/バリア/フィールド/分身/超回避/ＥＣＭ/
    特殊効果無効化/抵抗力)。
- **(B) 残り＝設計判断/別調査が要る境界 (将来)**: **相殺/中和** (隣接同名バリア/フィールドの位置依存・
  ＥＣＭ同様の盤面スキャン要) ／ **盾Lv*** (回数式・別機構) ／ **特殊効果無効化の即死(即)例外** (即死経路
  特定要) ／ **エネルギーシールド** (doc:5EN/+100×Lv が C# コードに無い乖離・サンプル該当機無し) ／
  **バリアシールド** (非標準名・(A) ÷2 据置) ／ **HUD プレビュー行の ＥＣＭ 反映** (GameDatabase 化要)。
- **未着手の細部**: `Set var (式)` の未設定変数→非評価（文字列保護のための意図的設計・SRC unset=0 との
  トレードオフ）。

## 現在地（2026-06-19）

**テスト**: `cargo test -p src-core` 全緑（**1974 件**）／ clippy clean（`-D warnings`）／ wasm `cargo check` OK。  
**最新セッション（2026-06-19・`master` 直接コミット）= GBA 着手 Phase 1〜3 完了（図形 primitive＋画面意味論＋実データ検証）。残は Phase 4 実機見栄えのみ**:
★★ **前提ブロッカーの誤認を是正**: 前セッションは「in-repo に GBA シナリオが無い」と結論したが、これは grep を `_GBA_` という狭い語に
限定したための誤り。実際には **in-repo の スパロボ戦記 fixture に汎用戦闘アニメ Lib 相当の `lib/BattleAnime*.eve`（無印/G/O/R/S）が既に存在**
（`設定[全身戦闘アニメ]`＝クローズアップを描画 primitive で実現）。**GBA は当初からブロックされていなかった**。エンジンも `try_play_battle_animation`
（`animation.txt` 解決→script_library 実在サブルーチンを再生）で再生経路を持つ。
① ✅ **図形描画 primitive を実装（`dc6bdcd`）**: 戦闘アニメ Lib が多用する `Circle`/`Polygon`/`Oval`/`Arc` が **no-op だった**
（catalog Stub）。SRC 原典構文どおり実装し、`FillStyle`/`FillColor` を状態分離（旧 fillcolor が SetColor に潰す配線を是正）。Canvas2D で描画。
② ✅ **画面クリア意味論を是正（`5e58a0b`）**: フレームループ `Paint; Refresh; ClearPicture; Wait`（Lib に 1989 箇所）で **ClearPicture が Wait 前に
overlay を即クリア＝毎フレーム空表示**だった。SRC immediate-mode に倣い ClearPicture を**遅延クリア**化（次の描画 push か Refresh まで保持）＝Wait 中もフレームが見える。
③ ✅ **描画ペン状態を ClearPicture 跨ぎで永続化（`8ab258a`）**: Color/FillStyle/FillColor/DrawWidth はループ外で 1 度設定し毎フレーム ClearPicture するため、
保持しないと図形が既定色（白）に。SRC の ObjColor 永続性を再現（永続フィールド＋レンダラ seed・`clear()` シーン遷移はリセット）。
④ ✅ **実 fixture で解決パイプライン＋GBA 分岐を検証（`596f7bd`）**: animation.txt の武器→`resolve_weapon`→Lib ラベル実在を突合する統合テスト。
GBA クローズアップが **`設定[全身戦闘アニメ]=オン` で分岐**することを実データで固定（Phase 3 配線の前提）。**Phase 1 gap 監査の結論**: 戦闘アニメ Lib が使う命令動詞に
**未実装/Stub は 0 件**（図形 primitive 実装後。残ノイズは `#`/`//` コメント行と `List(...)` 継続断片のみ）。VFS file I/O（Open/Print/Close/Load）も駆動中に正常実行を確認。
**配線確認**: combat が `対象ユニットＩＤ`/`相手ユニットＩＤ` を `try_play_battle_animation` 前に束縛済（`app.rs:3282-3284`→`:3654`）。
⑤ ✅ **実 combat 経路でフレームループを検証（`11b9429`）**: `attack_resolve_and_run`→戦闘アニメ起動の実経路で `Paint; Refresh; ClearPicture; Wait` 多フレームが
各フレーム overlay に残る（毎フレーム空にならない・累積しない）ことを確認＝GBA フレーム描画の combat 統合 capstone。
⑥ ✅ **実 fixture のクローズアップ本体が headless で完走（`3d87adc`）**: `設定[全身戦闘アニメ]=オン`（スパロボ戦記は `スパロボ戦記.eve:48` で**既定 ON**）で
実サブルーチン `戦闘アニメ_拡大小ビーム照射攻撃` が**未対応命令/欠落ラベルなく Wait まで完走**（ヘルパ・VFS file I/O 込み）。実 D 戦闘を `VERIFY_ANIMATE=1` で駆動し、
実戦闘武器（スティンガン/ビームライフル/破壊光線＝全て animation.txt でクローズアップ sub に解決）の戦闘が ScriptError/panic なく完走することも確認。
native test 11 件＋統合 2 件。**Phase 1/2 完了・Phase 3 headless 完了**。⑦ ✅ **Phase 4＝ブラウザ到達確認済（2026-06-19）**: in-repo 素材から組んだ
**最小検証シナリオ**（§4 レシピ・著作権で非コミット）を `just serve` で読み込み、**マップ・ガンダム個別スプライト・戦闘（反撃手段選択）が実機描画**されることを
ユーザがスクショで確認。攻撃実行でクローズアップ本体が再生（図形/ClearPicture/ペン状態の実機検証）。
**⏸ クローズアップ各フレームの目視確認はユーザ判断で保留（Windows 動作環境を確保済み・そちらで実施予定）**＝GBA はエンジン側の検証可能範囲を完了し残は実機の見栄え微調整のみ。詳細は §4「GBA 着手準備」。

**★★ 次セッション＝「オリジナル SRC との互換性向上」（ユーザ決定 2026-06-19）**: GBA は一区切り。次は VB6 `SRC_20121125` / C# `SRC.Sharp` を ground truth とした
**差分検証の拡張と乖離の是正**を主題にする。起点は **§1.1「次フロンティア」** ＋ 既存差分 harness `tools/oracle-diff`（式層/Commands/データ層/combat 予測/移動/気力・精神を
カバー済）＋ 記録済み乖離 `docs/SRC_SHARP_DIVERGENCE.md` ＋ memory [[reference_csharp_oracle]]（C# は macOS で 7490 tests 緑・`nix develop .#dotnet`）。
**有望な着手候補**（詳細・優先度は §1.1 / §4 テーブル参照）: ① **未カバー領域の差分 corpus 化**（状態異常/SetStatus の実効、ZoC・遮蔽、水中/宇宙 passability＝該当ユニットを `@unit` で合成、
クリティカル技能項の C# 突合）② **記録済み乖離の解消**（`SRC_SHARP_DIVERGENCE.md` の継承/性別正規化/`特殊能力名称` 列挙差）③ **別 fixture 駆動で新種バグ発掘**（"fixture の多様性が鉱脈" の踏襲）。
**手法**: 「実 fixture or 合成入力を両エンジンで評価して diff → 乖離は VB6/C# で裏取り → 是正 → synthetic＋oracle 回帰」を継続（推測実装はしない）。

**前セッション（2026-06-18・`master` 直接コミット）**: ① ✅ **B 単機ステータス詳細（`Scene::UnitDetail`）完了**（§1.2 B）。
② ✅ **差分オラクルを combat 予測（c）＋移動（d）＋気力/精神（e）＋改造/極端 level（f）＋別 fixture/サイズ差（g）へ拡張**。combat `placeattack` 45/45・移動 `moverange` 平地一致・
気力/精神 10/10・改造/level `combat_rank_level` **20/20**・サイズ差 `combat_size_tales` **7/7**。過程で **実バグ 11 件**を発掘・是正（全て VB6/C# 裏取り）: ①命中率クランプ→上限なし・最低0
②最低ダメージ→既定10 ③地形命中修正の符号（正=防御地形）④防御側パイロット Defense（耐久）⑤飛行/水中等の特殊移動コスト（2→1 game MP）
⑥ブースト の高気力 ×1.25 配線 ⑦攻撃側ダメージ増加精神を sp.txt データ駆動・MaxDbl 非加算へ ⑧防御側 被ダメージ低下も data 駆動・C# up/down-mod へ（**不屈 1→×0.1**）
⑨ **機体改造の武器攻撃力を乗算 `+base×10%×Rank` → C# 加算（通常+100×Rank/固据置/Ｒ・改 +50 or +10×n×Rank）へ是正**（改造ユニットの攻撃が過大だった pervasive bug）。
⑩ **散（散布）属性武器の距離補正を実装**（manhattan 距離 1-5+ で 命中 +0/+5/+10/+15/+20・ダメージ ×1.0/0.95/0.90/0.85/0.80。ライブ戦闘 3 経路＋プレビューに配線。Rust は 散 を一切未実装だった）。
⑪ **防御特性「弱点」の装甲半減を実装＋属性照合を字単位部分一致へ是正**（VB6 `Unit.cls:6949` `arm \ 2`。旧 Rust は弱点＝ダメージ変化なし＋照合が完全トークン一致で複合 class "格実火" が弱点 "火" に不一致＝耐性/吸収/無効化も事実上機能せず。**oracle は SRCCore の未完成ポート＝検証不能で VB6 が唯一の ground truth**）。詳細は §1.1 stage c-g＋防御特性節。  
**本セッション後半の追加完了**: ✅ **A2 着地点選択 UI 完了**（§1.2 A・発進 `09480eb`＋分離 `a533da3`）／✅ **防御特性を全 3 経路（通常/援護/反撃）で完成**（弱点装甲半減＋字単位照合＋吸収装甲無視＋魔例外、`cb4ce83`/`960e35e`/`d13bfe9`）／
✅ **単機ステータス詳細に必要技能の武器使用可否を反映**（`b1504b3`）／✅ **空 passability をオラクル実証**（`018f4bd`）。  
**★★ GBA クローズアップ戦闘アニメ＝Phase 1〜3 完了・Phase 4(実機目視)保留（ユーザ決定 2026-06-18／2026-06-19 完了・保留）**: 偵察結果・段階計画は **§4「GBA 着手準備」**に集約。
最重要: ① ~~前提ブロッカー＝in-repo に GBA シナリオが無い~~ **誤認を是正（2026-06-19）**: スパロボ戦記 fixture の `lib/BattleAnime*.eve` が汎用戦闘アニメ Lib 相当（`設定[全身戦闘アニメ]`）＝**シナリオは在った**。② エンジンは描画（PaintString/PaintPicture/Line/PSet/Color/Cls/**Circle/Oval/Polygon/Arc**）/配列(dict)変数/Redraw/Call/script_overlay 独自画面 の素材が揃う（図形 primitive を本セッションで充足）。
③ GBA クローズアップは**エンジン機能ではなくシナリオ側 汎用戦闘アニメ `.eve` サブルーチン**で、engine 側は primitives（Lib が使う命令＋Redraw/Keep clear 意味論＋固定レイアウト画面）を満たす形。
**オラクル尾部（任意・低優先）**: バリア強度吸収の精緻化／捨て身・攻撃力ＵＰ の効果統合／クリティカル技能項（プレビュー非露出＝影響軽微）／サイズ SS/XL・Ｒ/改 武器（**別 fixture が要**）。  
**ブランチ／コミット**: `master` で直接コミット（ユーザ指示）。以前の `feat/oracle-unit-diff`（差分オラクルのデータ層＋ユニット/パイロット実体層拡張）は `master` に取り込み済。
過去の `feat/necessary-skill-gate` は `master` へ取り込み済（ローカル `master` が `origin/master` から大きく先行）。
本セッションで実エンジンバグ **6 件**修正（pilot.txt カンマ形式特殊能力・`Input` 配列 lvalue 値展開・`expand_vars` クオート内 `name[expr]` 展開・マップ範囲外クラッシュ・**括弧無し `var = expr` 算術代入が未評価**・**数値関数引数の裸変数算術が未評価**）。
push はユーザの明示指示で行う（no-auto-push）。**D スパロボ戦記の「進行不能」は §2 で解決済**（エンジンは戦闘まで完走、原因は harness）。
次セッションの残課題は §1。**★ ユーザ決定（2026-06-16）**: ① ✅ **魅了/憑依は「spec 準拠で実装」完了**（憑依=恒久支配・ボス免疫・SpecialPower 封じ /
魅了=3T 一時・護衛行動・期限切れ復帰。synthetic test 4 件。実装詳細は §1.1）。② 並行方針 **「更にバグ探索を継続」**: in-repo の**未駆動 fixture（温泉旅館＝非戦闘の経営シム）を駆動**して
**式評価エンジンの 2 バグを発見・修正**（`d1d2c85` 括弧無し `var = expr` 算術代入が未評価 / `bd90843` 数値関数引数の裸変数算術が未評価）。両修正で温泉旅館の経営計算 cascade
（`Round(...)`→収入→収支→資本金）が**ヘッドレスドライブで end-to-end 評価**されることを実証（§4「温泉旅館」）。他の残（GBA 大規模移植・A2/演出/詳細 UI の検証制約）は従来どおり。

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
  **残（軽微・意図的）**: ① ✅ **必要技能未達武器のステータス画面表示を実装（2026-06-18、`b1504b3`）**: 単機ステータス詳細（`Scene::UnitDetail`）の `WeaponRow` に `usable` を追加し、必要技能/必要条件未達の武器をグレー＋「(技能不足)」併記で表示（非表示でなく標識化＝閲覧画面では全武器を示しつつ解禁状況を明示）。synthetic 2 件。
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
- **E GBA クローズアップ戦闘アニメ（✅ Phase 1〜3 完了・Phase 4 実機目視のみ保留）**: 専用バトルスプライト＋固定レイアウトの段階移植。
  ✅ 前提ブロッカー誤認是正＋図形 primitive＋画面クリア意味論＋ペン状態永続化＋実 fixture 解決/クローズアップ headless 完走/実 combat 経路を全て実装・検証（`dc6bdcd`〜`3d87adc`）。
  Phase 4 はブラウザ到達確認済で、各フレームの**目視のみ Windows 環境で実施予定＝保留**。**詳細・レシピは §4「GBA 着手準備」に集約**。
- **差分 harness のユニット/combat 状態拡張（2026-06-17）**: `tools/oracle-diff` は式層・Commands 層（逐次）に加え、
  **✅ 静的データ層（stage a-1）を構築・実バグ 1 件を是正**。新設 `oracle_loaddata` bin（Rust）が C# `LoadDataDirectory` と
  同じ pilot.txt/unit.txt を同順でロードし、同一の `Info(ユニットデータ/パイロットデータ,…)` probe を両エンジンで評価して diff
  （コーパス `unit_data.txt`、対象 `fixtures/スパロボ戦記/data/スパロボ戦記`）。**結果 58/61 一致**。
  - **✅ 実バグ是正（`803e13d`）**: pilot.txt 能力値行の 5/6 番目（技量/反応）を取り違えていた（`Info(…,技量)` C#=135/旧 Rust=80）。
    VB6 `PilotDataList.cls:677-692` 準拠（技量→反応の順）に是正。combat 式は意味的に正しく参照していたため**パーサ是正で実効値が正される**。
  - **残 3 件は既知乖離として記録**（`docs/SRC_SHARP_DIVERGENCE.md`）: ① unit `特殊能力数`/`特殊能力名称,1`（Rust が bare marker 行
    `全ユニット共通` を捨てる差・継承未実装で機能影響無）② パイロット `性別`（`Sex` enum で `-`→空に正規化）。C# はリスト初期化で
    組込みダミー（パイロット不在/ユニット無し）を 1 件足すため件数が +1。
  - **教訓**: データ層 diff は新種パーサバグの発掘源（能力値順の取り違えは combat に波及していた）。**非戦闘の経営シム同様、戦闘外の
    視点で初めて露見する**。
  - **✅ stage a-2（配置ユニットの実体状態 diff）も完了**: `oracle-diff placeunit` モード＋ Rust `@unit` 指令を新設。`@unit <name> <rank> <party>`
    で両エンジンが同一ユニットを生成（C#=`UList.Add(name,rank,party)`+`FullRecover`＝GUI 依存 `CreateCmd` を経ず `Units/` テストと同パターン・
    map 不要／Rust=`Create <party> <name> <rank> - 0 <x> 1`）し、`Info(ユニット,…)` で実効値を diff（コーパス `unit_instance.txt`、**24/25 一致**）。
    **✅ 実バグ是正（`135b5da`）**: `Create` の rank（改造段階）を捨てており、`Create 敵 ザコ 2 …` の増援が改造ボーナスを得られなかった
    （rank2 で C#=MaxHP+400/装甲+200 に対し旧 Rust=素の値）。`upgrade_level` へ配線し rank 0/2/3/5 の HP/EN/装甲/運動性が cross-engine 一致。
    残 1 件は既知乖離（`気力`: 無人ユニットは C# 空・Rust 既定 100、有人なら一致）。
  - **✅ stage b（有人ユニット＝パイロット実体状態）も着手・実バグ 1 件是正＋大物 1 件を発掘**: `@unit` を 5 フィールド有人形式
    （`<unit> <rank> <party> <pilot> <level>`）へ拡張（C#=`PList.Add(pilot,level,party)`+`Ride`／Rust=`Create` の pilot/level）。コーパス
    `unit_pilot.txt`。**✅ 実バグ是正（Create の level 配線）**: `Create` が level 引数（主パイロット初期レベル）を捨ててレベル 1 固定だった→
    `exp_for_level` で初期 total_exp へ配線。**レベル/累積経験値は cross-engine 完全一致**（lv10/20/30）。
  - **★★ ✅ pervasive バグを発掘・是正: パイロットのレベル成長式**。旧 Rust `grown_pilot`/`apply_stat_growth` は class ベース
    `base+(level-1)*rate`（過大成長）。VB6 `Pilot.cls:582-593` 準拠の **`lv=Level`（レベル 1 でも成長）・格闘/射撃/技量/反応 +=lv・
    命中/回避 +=2*lv** へ是正。**全レベルアップ済みパイロットの戦闘実効値に波及する pervasive bug**だった（人工知能 lv10 格闘 旧190→110、
    超人工知能 lv30 415→155）。併せて `Info(パイロットデータ,…)` が配置済みパイロットで成長後を返す conflation も是正（静的データを返す）。
    `unit_pilot.txt` **13/13 一致**で cross-engine 検証。「level 1 でも成長」へ伴い成長系テスト 5 件の期待値を VB6 値へ更新
    （`from_data` は level 1 = base+成長、成長は class 非依存に）。
  - **✅ パーサ層の追加 sweep**: 武器フィールド（`unit_weapon.txt`、マジンガーＺ 7 武器×全フィールド）**38/38 一致＝武器パーサ堅牢**。
    パイロット SP/特殊能力（`pilot_feature.txt`）**11/13 一致**。残 2 件は `特殊能力名称` 列挙の既知乖離（C#=別名 RHS / Rust=key LHS。
    `特殊能力所有` 所有判定は一致＝表示のみ、`docs/SRC_SHARP_DIVERGENCE.md`）。
  - **✅ stage c = combat 予測（命中率/ダメージ/クリティカル率）diff も構築・実バグ 2 件是正（2026-06-18）**: 専用モード `placeattack`
    を新設（C#=map 初期化＋攻撃側/守備側を `StandBy` 配置し `UnitWeapon.HitProbability/Damage/CriticalProbability` を直接呼ぶ・
    Rust=`effective_combat_data`→`combat::predict_with_status_terrain` を中立条件で呼ぶ）。コーパス `combat_prediction.txt`（命中/クリティカル、
    地形非依存）＋ `combat_damage.txt`（ダメージ、両者を地上に置き env=陸 で地形適応を整合）。**命中 18/18 ＋ ダメージ 14/14 = cross-engine 全一致**。
    **★★ 実バグ 4 件を発掘・是正（全て VB6 裏取り）**: ① **命中率クランプ**＝旧 `clamp(5,95)`（他 SRPG 慣習）を VB6 `Unit.cls:6694-6696` 準拠の
    **上限なし・最低 0**（>100=必中、表示のみ `min(100)`）。② **最低ダメージ**＝旧 `max(1)` を VB6 `Unit.cls:7460` 準拠の**既定 10**。①②は全戦闘の命中/ダメージに波及。
    ③ **地形の命中修正の符号**＝旧 combat `(100+hit_mod)`＋ビルトイン地形が負値の独自規約を、VB6 `Unit.cls:6295`/`マップデータ.md` 準拠の
    **正=防御地形・`(100-hit_mod)`** へ統一（terrain.txt は正格納。terrain.txt をロードする実シナリオで防御地形の被命中が逆転＝pervasive。AI の防御地形選好も是正）。
    ④ **防御側パイロット Defense（耐久 技能）**＝旧 `def_power` に Defense 係数が無く耐久持ちが過大被ダメージ。VB6 `Pilot.cls:402` `Defense=100+5*耐久Lv` を
    `防御力×Defense/100` へ反映（基底 Defense=100 の人工知能では露見せず、cut2 representative の `Info(防御)=100` から発掘）。
    SRC ダメージ式 `(攻撃力−防御力)×地形ダメージ修正` は**構造が Rust と同一**（攻撃力にも地形適応が乗ることも実数確認＝`戦闘システム詳細.md`）。
    回帰テスト 4 件（`hit_chance_has_no_upper_cap`/`minimum_damage_is_ten`/`positive_terrain_hit_mod_reduces_hit`/`endurance_skill_raises_defense_and_reduces_damage`。③④は synthetic＝fixture に該当データ無し）。
    地形を変えた hit/ダメージ diff（terrain.txt ロード＋`@terrain`）も別コーパス `combat_terrain.txt` で **13/13 一致**＝③の符号是正を cross-engine 実証。
  - **✅ stage d = 移動範囲（Dijkstra）diff も構築・実バグ 1 件是正（2026-06-18）**: 新モード `moverange`（C#=`Map.AreaInSpeed`→`TotalMoveCost`）＋
    Rust bin `oracle_move`（`movement::compute_range_with`）。指令 `@map`/`@cell`/`@unit <…> <x> <y>`/`@move`、コーパス `move_flat.txt`/`move_terrain.txt`。
    両者を **2 倍スケールの到達コスト**へ正規化（Rust=`(speed-残MP)*2` / C#=`TotalMoveCost`、0 始まり）。**平地で完全一致**。
    **★ 実バグ 1 件是正**: 飛行/水上/水中/宇宙の特殊移動（地形を乗り越える）コストが 2 game MP だった→C# 内部 2 倍スケールの `move_cost=2`＝
    1 game MP のスケール取り違え（`movement.rs` の `TRAVERSE`=1 へ集約。飛行/水中/宇宙ユニットが地形上で移動範囲半減していた pervasive バグ）。
    残る地上ユニットの森林/山（1.5/2.5）の ceil 差は**整数移動の既知設計乖離**（`CLAUDE.md`）。**注**: C# `IsAdopted`（地形適応 特殊能力）のコスト cap は Rust 未モデル。
  - **✅ stage e = 気力/精神（morale/spirit）diff も構築・実バグ 1 件是正（2026-06-18）**: `placeattack` に `@morale <unit> <value>`/`@spirit <unit> <name>` を追加
    （C#=`pilot.Morale=`/`MakeSpecialPowerInEffect`+`SPDList.Load(system/sp.txt)` / Rust=predict の morale 引数・status スライス）。コーパス `combat_morale.txt`。
    **気力スケーリング自体は一致**。**★ 実バグ 2 件是正**: ⑥ **ブースト（ユニット特殊能力）の高気力 ×1.25** を予測へ配線（気力 150 が 1206→1507。
    `全ユニット共通` 継承ではなく予測が `atk_unit.features` の ブースト を未参照だったのが真因＝共通能力は各ユニットに明示列挙）。
    ⑦ **攻撃側ダメージ増加精神を sp.txt データ駆動・MaxDbl 非加算へ**（`SpecialPowerData.effects`＋`db::sp_effect_level`）。熱血+魂=×2.5(MaxDbl)・
    気合=×1.0で morale corpus 一致。⑧ **防御側 被ダメージ低下/増加も data 駆動・C# 完全 up/down-mod へ**（`DamageSpiritLevels` 構造体）。
    **不屈 の重大乖離を是正**（旧 `min(1)`→`被ダメージ低下Lv9`=×0.1、例 1572→157）。鉄壁=Lv7.5(÷4) は同値で挙動不変。バリア（強度吸収シールド）は範囲外。
    防御側 鉄壁/不屈 ケースを corpus へ追加し **morale 13/13 一致**。
  - **✅ stage f = combat corpus を rank>0 / 極端 level へ拡張・実バグ 1 件是正（2026-06-18、9 件目）**: 新コーパス
    `combat_rank_level.txt`（改造段階 rank3/5 ＋ level 1/50/99 で命中/ダメージ/クリティカルを突合）。**改造ユニットの攻撃ダメージが C# と乖離**（6/20 不一致）。
    真因＝`effective_combat_data` の**機体改造の武器攻撃力上昇が乗算** `+base×10%×Rank` で、SRC.NET `Unit.cs:4089-4551` UpdateWeaponPower は
    **加算**だった（通常 +100×Rank ／ 固 据え置き ／ Ｒ・改 は `<attr>L<n>` で +10×n×Rank・無指定 +50×Rank ／ 攻撃力 0 据え置き）。乗算式は
    base 攻撃力が大きい武器ほど過大（ブレストファイヤー 2800 で Rust +1400 vs C# +500）で**固定ダメージ武器も誤増加**＝改造ユニットの全戦闘に波及。
    `weapon_class_level` で `ＲL<n>`/`改L<n>` を解析し加算ルールへ是正→ **combat_rank_level 20/20 一致**（baseline 18/18・damage rank0 14/14 不変）。
    synthetic 回帰 `upgrade_weapon_power_matches_src_additive_rule` 追加。**注**: 全 fixture ユニットは M サイズ・固/Ｒ/改 武器無しのため SS/XL とＲ/改は corpus 未到達（synthetic で検証）。
  - **✅ stage g = 別 fixture（テイルズオブファンタジア）でサイズ差/技能パイロットを突合・実バグ 1 件是正（2026-06-18、10 件目）**: スパロボ戦記は全 M サイズ・技能無パイロットのため、
    S/M/L サイズと技能（切り払い/魔力所有/Ｓ防御）を持つ別 fixture で combat 予測を突合（`combat_size_tales.txt`）。**クレスの武器 1「虚空蒼破斬」(class 格魔散) だけが乖離**（命中 -5・ダメージ過大）。
    真因＝**散（散布）属性**の距離補正を Rust が**完全未実装**だった（ライブ戦闘でも散武器が距離スケールせず）。SRC.NET `Unit.cs` 準拠で manhattan 距離 1/2/3/4/5+ → 命中 +0/+5/+10/+15/+20・
    ダメージ ×1.0/0.95/0.90/0.85/0.80 を `combat::scatter_*`＋`CombatPreview::apply_scatter` で実装し、ライブ戦闘 3 経路（通常/援護/反撃）＋プレビュー行に配線。
    harness は C# `StandBy(1+2*created,5)`（2 マス間隔）に合わせ Create x を 1,3,5,… にし距離依存補正を cross-engine 化→ **7/7 一致**（既存 corpus 不変）。
    **教訓**: 全 M サイズ・技能無の fixture では到達不能な属性（散）が、別 fixture を当てて初めて露見。**fixture の多様性が新種バグの鉱脈**（経営シム・データ層・別シナリオと同じパターン）。
  - **✅ stage h = 防御特性「弱点」是正済（2026-06-18、11 件目）＋ oracle はこの領域を検証不能と判明**:
    別 fixture（テイルズ モンスター、`弱点=火` 等）で防御特性を突合する過程で Rust の 2 バグを是正（commit `cb4ce83`）。**VB6 が ground truth**:
    - ① **弱点=装甲半減 未実装**→ `weapon_hits_weakness` ＋ `attack_resolve_and_run` で予測前に `def_unit.armor /= 2`（VB6 `Unit.cls:6949` `arm \ 2`、静的特殊能力＋一時付加 condition `弱点:<属性>`）。
    - ② **属性照合が完全トークン一致**（無空白複合 class "格実火" が弱点 "火" に不一致＝耐性/吸収/無効化も事実上死んでいた）→ `defense_attr_matches` で **VB6 `InStrNotNest`＝字単位部分一致**へ（全=常に / 物=魔・精以外）。優先順も spec「弱点>有効>吸収>無効化>耐性」へ。synthetic 回帰 `defense_attr_substring_matching_and_weakness_detection`（既存 defensive test も後方互換）。
    - **★ 検証手段の重大知見**: 差分オラクル（SRCCore）は**防御特性を検証できない**。実機 SRCCore は `strWeakness`/`strResist`/`strAbsorb` の populate が**コメントアウトされた未完成ポート**（`Unit.status.cs:1082` 全コメント・`Unit.ref.cs` は `SRCCore.csproj` で `<Compile Remove>`）で `Weakness()` が常に false＝防御特性が無効。harness が `UList.Add`＋`Update()` を経ても strWeakness は空（debug 実測）。**oracle も移植であり ground truth ではない**（[[reference_csharp_oracle]] の教訓）の顕著例。**今後この領域は VB6＋synthetic で検証する**（harness 先行案は SRCCore 未完成のため無効と判明し撤回）。
    - **✅ 追加是正（commit `960e35e`）**: 吸収を**装甲無視基準**へ（`weapon_hits_absorb`＋予測前 `armor=0`、弱点>吸収 優先・有効で打ち消し）＋**魔属性例外**（魔武/魔突/魔接/魔銃/魔実 武器に「魔」防御特性が効かない）を `defense_attr_matches` に実装。synthetic 拡張。
    - **✅ 援護/反撃にも防御特性を適用（commit `d13bfe9`）**: 装甲調整を `apply_defense_armor_mod` へ共有化し通常攻撃/援護/反撃の予測前に一貫適用＋援護/反撃のダメージを `defense_attribute_damage` でラップ（プレイヤーが弱点持ちの敵を反撃/援護する頻出ケースの過少ダメージを是正）。**防御特性は全 3 経路で完成**。
    - **残（要精緻化・任意・極低優先）**: 吸収×クリティカルの適用順のみ（VB6 は Damage 内で反転→外で ×2＝稀）。
  - **（参考・解決済の経緯）当初の調査記録**:
    別 fixture（テイルズ モンスター、`弱点=火`/`吸収=水`/`耐性=闇` 多数）で防御特性を突合する過程で、**Rust の防御特性ダメージ計算に複数の確定バグ**を発見。
    **重要な訂正**: 当初 `SRC.Sharp/SRC.NET/` を読んでいたが、**oracle harness が実際にビルドするのは `SRC.Sharp/SRC.Sharp/SRCCore/`**（`oracle-diff.csproj` の ProjectReference）。
    実機エンジン source（SRCCore）で挙動を**確定**した:
    - **spec（`防御特性に関する特殊能力.md`、権威）**: 弱点=**装甲半減**（被ダメ増）/吸収=装甲無視ダメージ÷2 回復/耐性=÷2/無効化=0/有効=吸収・無効化・耐性を打ち消す。優先 **弱点>有効>吸収>無効化>耐性**。魔武/魔突/魔接/魔銃/魔実 武器には「魔」防御特性が効かない。
    - **実機 C#（`SRCCore/Units/UnitWeapon.cs:2925`）**: `if (is_true_value || mpskill>=140) { if (t.Weakness(wclass)) arm/=2; else if (!t.Effective && t.Absorb) arm=0; }`＝**弱点で装甲半減**（spec どおり）。harness は `is_true_value=true` を渡す。属性照合 `Unit.attribute.cs::Weakness` は `InStrNotNest`＝**部分文字列/字単位**（"火"⊂"格実火"）。
    - **Rust の確定バグ（複数）**: ① `app.rs::defense_attribute_damage` は **弱点＝ダメージ変化なし**（装甲半減 未実装。ほぼ全モンスターが弱点持ち＝pervasive）。② 照合が `split_whitespace` の**完全トークン一致**で、複合 class "格実火" vs 弱点"火" が**一致しない**（C# は部分文字列＝字単位。実データの class は "格実火" のように無空白複合が常態）。③ 吸収が「最終ダメージ÷2」で spec の「装甲無視ダメージ÷2」と乖離。④ 援護/反撃経路は `defense_attribute_damage` 未呼び＝防御特性が一切効かない。
    - **oracle 検証のブロッカー（2 段）**: ⓐ harness は `WeaponData.Damage(defender,true)` を呼ぶが、**実戦闘（`Unit.attack.cs:124-126` の `Update(); t.Update();`）を経ない**ため防御側の `strWeakness` 等が populate されず 弱点 が常に no-op（→ harness に `attacker.Update(); defender.Update();` 追加で解消可・既存 corpus 不変を確認済）。ⓑ **さらに `UList.Add(name,rank,party)` がモンスター個別の `弱点=火` を colFeature へロードしない**（debug 出力で `feats=[=][らくらく愛称設定][敵専用補正]`＝共通能力のみ・個別弱点は空 `[=]`）。**ⓑが本丸の未解決**：正規生成（CreateUnit 相当）か feature ロード経路の調査が要る。
    - **次手（順序）**: (1) harness の feature ロードを修正（ⓑ）→ Update() 追加（ⓐ）→ モンスター弱点 corpus で C# が arm/2 を出すこと確認。(2) Rust を実機準拠へ是正：照合を**字/部分文字列**化、**弱点＝予測前に def 装甲半減**（live 3 経路）、吸収＝装甲無視基準、援護/反撃へも配線。(3) synthetic＋oracle で検証。**core ダメージ式の複数箇所変更＋oracle 検証ブロッカー（ⓑ）未解消につき、harness 修正を先行させてから着手**（推測実装はしない方針を堅持）。
  - **次フロンティア**: ① バリア強度吸収の精緻化・捨て身/攻撃力ＵＰ の効果統合（任意・低優先）。**Ｓ防御＝シールド防御**（`Unit.cs:20519`、シールド特殊能力持ちが確率で被ダメージブロック）も
    この**シールド/バリア系（確率的・静的 predict 非露出）**に属し ① に含む。② テイルズ fixture の追加活用余地（**サイズ HIT 補正は検証済で一致**）: 魔力所有 + 高威力 魔 武器（ミントの攻撃武器は威力 0/低のため不可・別 mage 要）、
    切り払い（防御側の確率反撃＝静的 predict 非露出）。サイズ差の命中は `combat_size_tales` で実証済。
    ③ **クリティカル率の技能項**（超反応/超能力/底力。Rust `critical_probability` は素のみで技能は `App::crit_skill_bonus` に分離。**プレビュー行は命中/ダメージのみ表示で crit 非露出＝影響軽微・fixture に技能持ち不在**）。
    ④ 移動 corpus 拡張: ✅ **空 (sky) passability を実証**（`move_sky_block.txt`、地上ユニット 空適応 '-' は空マス id 81 に進入不可＝両エンジン一致）。残: 水中/宇宙 passability は `水/宇適応 '-'` ユニットが in-repo fixture に不在で未カバー／ZoC・遮蔽。
    ⑤ SetStatus/状態異常の Info diff は**式層では低価値**。設計は memory `reference_csharp_oracle`。

### 1.2 検証制約・MVP 拡張（小〜中）

- **A2 着地点選択**: ✅ **発進・分離とも実装済（2026-06-18、`09480eb`＋`a533da3`）＝A2 完了**: `ActionMode::LandingSelect` を新設し、
  **発進**（`resolve_launch`）は母艦位置を起点とする移動範囲の空きマスから対話選択、**分離**（`apply_separate`）は構成ユニットを off_map で staging し
  `begin_landing_queue` で 1 機ずつ順次選択（`landing_queue`/`advance_landing_queue`/`finish_landing_queue`）。両者とも `confirm_landing` が候補クリックで配置・
  候補外 no-op、候補無し（狭所/map 無し）は従来の隣接自動へフォールバック。`db::unit_move_range_from`＋`landing_candidates` で候補算出、`draw_action_overlays` が候補を黄ハイライト。
  native test 5 件（発進 配置/キャンセル・単機分離・3 体合体の順次配置）。**クリック選択の見栄えは実機確認**（対話 UI）。スクリプト/AI の `Launch`/script 分離 は非対話のまま。
  **残（任意）**: 初期合体形態の分離後の再合体トラッキング・分離パイロットの主/副の細分配分（現状は形態 1 へ集約）。
- **B インターミッション**: ✅ **ステータスの単機詳細化 完了（2026-06-18）**。インターミッション「ステータス」を、従来の
  静的データ閲覧 (`PilotList`→`UnitList`、`GameDatabase::pilots/units` の一覧 MVP) から、**味方ロスター実体 (`unit_instances`) の
  単機詳細画面 `Scene::UnitDetail`** へ差し替えた。1 機ぶんの**実効ステータス**（改造段階・装備・レベル成長・状態異常込みの HP/EN/装甲/
  運動性/移動力 ＋ 搭乗パイロットの Lv/Exp/SP/能力値/精神コマンド/技能 ＋ 武器の攻撃力/射程/残弾・EN）を 2 カラム＋武器表で表示し、
  `◀ / ▶`（ボタン or 矢印キー）でロスターを巡回、Enter / 右クリック / 閉じる で復帰する。実装: ロジック層 `scene/unit_detail.rs`
  （レイアウト定数＋ビューモデル `StatusDetail`＋ボタン hit-test）／ビューモデル構築 `App::build_status_detail`（`GameDatabase::effective_*`
  ／`effective_pilot_data` で実効値解決・テスト可能）／巡回・入力配線（`status_detail_index`・`cycle_status_detail`・
  `handle_click_unit_detail`・`cancel_action`/`move_cursor`/`advance` 分岐）／描画 `src-web::render::draw_unit_detail`。
  **検証**: native test 6 件（ステータス→詳細遷移・◀▶巡回ラップ・閉じる/右クリック復帰・ビューモデルの実効値マッピング＝改造込み HP/
  残弾/EN/状態/精神/レベル成長）。**注**: 元 SRC のステータスは右クリックで戻る対話画面のため、実画面の見栄え確認は要シナリオ＝実機
  （A2 と同じ検証制約）。従来の P/U キーの静的データブラウザ (`PilotList`/`UnitList`) は MapView 側に従来どおり残置。
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
| GBA | **GBA クローズアップ戦闘アニメ移植（✅ Phase 1〜3 完了・Phase 4 実機目視のみ保留）** | ✅ 前提ブロッカー誤認是正（`lib/BattleAnime*.eve`＝汎用戦闘アニメ Lib・`try_play_battle_animation` 再生経路）／✅ 図形 primitive（Circle/Oval/Polygon/Arc＋FillStyle/FillColor、`dc6bdcd`）／✅ ClearPicture 遅延クリア（`5e58a0b`）／✅ ペン状態永続化（`8ab258a`）／✅ 実 fixture 解決・クローズアップ headless 完走・実 combat 経路検証（`596f7bd`〜`3d87adc`）。**未実装/Stub 命令 0 件**。Phase 4 はブラウザ到達確認済、各フレーム目視は **Windows 環境で実施予定＝保留**。**詳細・最小検証シナリオの再現レシピは下記「GBA 着手準備」を参照** |
| 演出 | エフェクトセットの見栄え調整・属性別 `EFFECT_` 選択の最適化。移動経路アニメは実装済だが滑らかさ向上余地 | 小 |
| AI | **NPC/中立 AI の優先度分離** | 標的は `is_hostile_to` で正しく分離。優先度ロジックは敵と共通。SetRelation/友好度上書きは SRC 準拠で**意図的に非対応**。明確な差別化ルールが見当たらず実装余地は限定的 |
| 手動 | スパロボ戦記の乗せ換え→戦闘通し目視 | 84MB ロードが必要な手動タスク（自動化対象外） |
| 温泉旅館 | **経営シム（非戦闘）の経営計算** | ✅✅ **2 件の式評価バグを発見・修正（end-to-end 実証済）**。未駆動 fixture `温泉旅館.zip`（非戦闘の経営シム＝式評価エンジンを深く突く）を駆動して発見。① ✅ **`d1d2c85`**: 括弧無し `var = expr` 算術代入（`資本金 = 資本金 - 営業収支`）が未評価＝SRC `ExecSetCmd`/`EvalTerm` 準拠で `=` 形のみ括弧無し算術を数値化（`Set` 形は引用符/Format 誤数値化回避のため括弧付きのみ。回帰7件）。② ✅ **`bd90843`**: 数値関数引数の**裸変数算術が未解決**（`Round(温泉宿１収入 + 温泉宿２収入)` が未評価＝従来は `$(...)` 補間のみ）。`numeric_arg` を `eval_numeric_atoms`（全アトム数値時のみ評価し非数値文字列は None＝LSet/RSet の契約維持）で裸変数解決。数値関数は既に numeric_arg 経由・文字列関数は fn_arg_value 直呼びで非影響・ネスト関数は expand_vars 事前展開。回帰4件。**両修正で収入計算 cascade（`温泉宿収入=Round(...)`→`営業収入`→`営業収支`→`資本金`）が end-to-end 評価**（`温泉宿１収入=25.2`/`営業収支=-38`/`資本金=500` を実測）。✅ **`VERIFY_MENU_CYCLE` drive 強化（`58a4f36`、§5）で更にターン 1→10 まで駆動**し全アクションハンドラ（整備/増築/雇用/掘削）＋経済 cascade を exercise（`資本金 57→-58`・`温泉宿 70→99`・`労働力 2→3` の動的変化を実測、parse/runtime errors=0）＝**経営エンジン全体が健全**と実証。残: 完走（53 ターン）は step 上限 400 で turn 10 まで＝engine 非ブロック|

### GBA 着手準備（✅ Phase 1〜3 完了・2026-06-18 偵察 / 2026-06-19 実装・Phase 4 実機目視保留）

**目的**: GBA クローズアップ戦闘アニメ（専用バトルスプライト＋固定レイアウトの戦闘画面）を移植する。**複数セッション規模**。

**偵察で判明した構造（重要）**:
- GBA クローズアップは **エンジンの組込み機能ではなく、シナリオ側の「汎用戦闘アニメ」Lib（`.eve` サブルーチン集）** が、
  エンジンの描画/変数/制御命令を使って実現する。SRC 本体の `Lib\汎用戦闘アニメ` に格納（`戦闘アニメデータ.md` 参照）。
  すなわち engine 側のゴールは「**Lib が使う primitives を満たす**」こと（GBA 画面そのものを engine が描くのではない）。

**★ 前提ブロッカーの誤認を是正（2026-06-19）**: 前回は「in-repo に GBA シナリオ無し」（`grep _GBA_` が 0 件）と結論したが**誤り**。
スパロボ戦記 fixture の **`lib/BattleAnime{,G,O,R,S}.eve` が汎用戦闘アニメ Lib 相当**（`戦闘アニメ_<武器>攻撃:` ラベル群＋`設定[全身戦闘アニメ]`
＝クローズアップ全身戦闘アニメ）。`_GBA_` という綴りでないだけで、Lib は最初から在った。エンジンも **`try_play_battle_animation`**
（`animation.txt` で `戦闘アニメ_*` を解決→**script_library に実在するもの**を 準備→攻撃→命中 の順に `Call` 再生、`app.rs:3766`）で再生経路を持つ。
→ **ヘッドレス/実機どちらでも駆動可能**。「シナリオ入手」は不要。次の一手はそのまま Phase 1 gap 監査（下記）。

**engine 側の現状（素材は概ね揃う）**:
- ✅ 描画命令: `PaintString`/`SystemPaintString`/`PaintPicture`/`Line`/`PSet`/`Color`/`Cls` = Implemented。
  ✅ **`Circle`/`Oval`/`Polygon`/`Arc` = Implemented（2026-06-19、commit `dc6bdcd`）**＋`FillStyle`/`FillColor` 状態分離。
- ✅ 配列(dict)変数: `prefix[key]` 形式をサポート（`Count(prefix)`/`Sort`/`Input` 配列・`script_vars: BTreeMap`）。`戦闘アニメ変数[…]`/`_GBA[…]` はこれで表現可。
- ✅ `Redraw` = Implemented（マップ再描画、`command_catalog.rs:576`）。ただし **GBA が依存する「Redraw/Keep の画面クリア意味論」は要確認**
  （`Keep` は現状 BGM 用 `KeepBgm` のみ＝画面 Keep 未確認）。
- ✅ 独自画面描画: `script_overlay`（`.eve` の蓄積描画コマンド）を MapView で表示する経路は実装済（タイトル/OP/キャラメイキング等の独自 click UI で実証済）。
  GBA 固定レイアウト（`BaseX/BaseY=0`）もこの script_overlay 経路に乗る見込み。
- ✅ `Call`（式中ユーザ関数/サブルーチン呼出）機構あり（`enter_call_args`/`call_label_sync_for_condition`）。
- ⚠ engine ネイティブの `battle_anim`（命中フラッシュ/ダメージ数字、`battle_anim.rs`）は **SRC 汎用戦闘アニメとは別物**。GBA は後者（シナリオ駆動）。

**段階計画**:
1. **Phase 1 — gap 監査**: ✅ **完了**。`BattleAnime*.eve` の使用命令を catalog と case-insensitive 突合（命令は `to_ascii_lowercase`・
   関数は正規化マップ）。**戦闘アニメ Lib が使う命令動詞に未実装/Stub は 0 件**（図形 primitive 実装後）。残「absent」は全て `#`/`//` コメント行
   （.eve パーサは `#` 始まり・`//` 以降を非命令として除外）と `List(...)` の継続断片＝ノイズ。実 fixture 駆動で **VFS file I/O（Open/Print/Close/Load）も正常実行**を確認。
2. **Phase 2 — primitives 充足**: ✅ **完了**。① 図形 `Circle`/`Oval`/`Polygon`/`Arc`＋`FillStyle`/`FillColor`（`dc6bdcd`）。
   ② **画面クリア意味論**＝`ClearPicture` を遅延クリア化（`5e58a0b`）。フレームループ `Paint; Refresh; ClearPicture; Wait`（Lib に 1989 箇所）で
   ClearPicture が Wait 前に overlay を即クリア＝毎フレーム空表示だった真因を是正（SRC immediate-mode のバックバッファ消去意味論を retained-overlay で再現）。
   ③ 描画ペン状態（色/塗り/線幅/フォント）を ClearPicture 跨ぎで永続化（`8ab258a`、SRC ObjColor 準拠）。
   **注**: `Keep` は依然 BGM 用 `KeepBgm` のみ＝画面 Keep は戦闘アニメ Lib で未使用（gap 監査で出現せず）＝対応不要。
3. **Phase 3 — 固定レイアウト描画の配線**（src-web）: ✅ **headless 検証可能範囲を完了**。① GBA 分岐の前提を実データで固定（`596f7bd`）＝クローズアップは
   **`設定[全身戦闘アニメ]=オン`** で開く（スパロボ戦記は `スパロボ戦記.eve:48` で**既定 ON**＝この scenario の既定戦闘表示が GBA クローズアップ）。
   ② 配線確認: combat が `対象ユニットＩＤ`/`相手ユニットＩＤ` を `try_play_battle_animation` 前に束縛（`app.rs:3282-3284`→`:3654`）、`animate_battle`＋`settings.battle_animation`(既定 true) で起動。
   ③ ✅ 実 fixture のクローズアップ本体が headless で Wait まで完走（`3d87adc`）・実 D 戦闘を `VERIFY_ANIMATE=1` で駆動し ScriptError なく完走。図形は Canvas2D で描画済。
   **残（Phase 4 と一体・要ブラウザ）**: 固定画面に `戦闘アニメ[対象ユニット画像]`/`Info(…,全身画像)` でユニット個別スプライトを置く**見栄え**（実アセットパックが要る）。
4. **Phase 4 — 実機検証**（対話/描画ゆえ headless 不可の見栄え確認）: ⏳ **ブラウザ到達確認済・各フレーム目視は保留（2026-06-19）**。最小検証シナリオ（下記レシピ）を
   `just serve` で読み込み、**マップ・ガンダム個別スプライト・戦闘（攻撃目標選択/反撃手段選択）が実機で正しく描画**されることをユーザがスクリーンショットで確認。
   攻撃を実行するとクローズアップ本体が再生される（図形描画/ClearPicture 遅延クリア/ペン状態の実機確認）。**クローズアップ各フレームの目視確認は Windows 動作環境を確保したユーザがそちらで実施予定＝保留**。
   - **観測メモ**: 攻撃前プレビューの「顔グラ枠」がグレー placeholder になるのは**正常**（`人工知能(ザコ)` は pilot.txt の顔フィールドが `-`＝顔グラ無しの無口ザコ AI。フル素材でも同じ）。
     実際の顔を出すには顔グラ持ちの**名前付きパイロット**を `Create` で使い `Bitmap/Pilot/` を同梱する（最小シナリオでは省いている）。
> **方針メモ**: GBA は「engine に GBA 画面を作り込む」のではなく「**汎用戦闘アニメ Lib が要求する primitives を engine が満たす**」のが正道。
> 推測で GBA 画面を実装せず、Lib スクリプトを駆動して gap を埋める（温泉旅館/スパロボ戦記で実証した「fixture を駆動して未対応を洗い出す」手法を踏襲）。

#### Phase 4 最小検証シナリオ（再現レシピ・著作権により非コミット）

実機の見栄え確認用に、in-repo の gitignore 済み素材 `crates/src-web/tests/fixtures/スパロボ戦記/`（84M・非コミット）から
**最小構成シナリオ**を組み立てる。**生成物（zip）は著作権上コミットしない**（`/tmp` で運用）。要点:
- **エントリ .eve（自作）**: キャラメイキングを回避し `Create 味方/敵 ガンダム 0 人工知能(ザコ) 20 x y` で 2vs2 を直接配置・`Set 設定[全身戦闘アニメ] オン`・
  `ChangeMap "map\map-1.map"`。`@スパロボ戦記` でデータ＋（`data/スパロボ戦記/Include.eve` 経由で）汎用戦闘アニメ Lib を読む。`プロローグ`/`スタート` はエンジンが自動発火。
- **同梱（不可分）**: `data/` ＋ `lib/` ＋ `map/map-1.map` ＋ `src.ini`（計 ~7M）。
- **スプライト（最小）**: ガンダム個別（`Bitmap/Anime/Unit/EC_G0079_Gundam*.bmp` 全身・`Bitmap/Unit/G0079_Gundam*.bmp` マップ・Shield 等の小ディレクトリ、~1.9M）のみ。
  **共通エフェクト**（`EFFECT_BeamRifle01` 等）は **vendor BA パック `vendor-assets/SRC_BA110418.zip`**（エンジン起動時自動ロード）が供給＝シナリオに含めない（"scenario=固有ユニット / vendor=共通" の最小化）。
- **検証**: `VERIFY_SMOKE=1 VERIFY_DRIVE=1 VERIFY_AUTOSTART=1 VERIFY_AUTOPLAY=1 VERIFY_ANIMATE=1` で entry 自動選択・parse/runtime errors=0・Battle 到達・combat 成立を headless 確認 → `just serve` で実機読込。
- **生成 8.9M（zip 1.2M）**。前提: `vendor-assets/` に 3 パック（特に `SRC_BA110418.zip`）が配置済みであること。

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
> `App::debug_firable_report`（pilot 解決/combat_data/武器発射可否）を出力。
> **2026-06-17 追加**: `VERIFY_MENU_CYCLE=1`＝同一 Menu/Ask が連続提示される（選択肢が no-op でループ）とき選択肢を順送りしてループを破る
> （経営シム等で「上限到達で無効化される選択肢」を越えて先へ進める。既定 OFF＝確立済み smoke ドライブ非影響。cmaking は対象外）。
> **env は `export` で渡す**（インライン `VAR=x cmd` は nix shell 経由で届かないことがある）。

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

### 2026-06-19 セッション（GBA 着手 Phase 1〜3 — 図形 primitive＋画面意味論＋実データ検証）

- **GBA 前提ブロッカーの誤認を是正**: 前セッションは「in-repo に GBA シナリオ無し」と結論したが、grep を `_GBA_` に限定したための誤り。
  スパロボ戦記 fixture の `lib/BattleAnime{,G,O,R,S}.eve` が汎用戦闘アニメ Lib 相当（`戦闘アニメ_<武器>攻撃:`＋`設定[全身戦闘アニメ]`）＝シナリオは在った。
  エンジンも `try_play_battle_animation`（`animation.txt`→script_library 実在サブルーチン再生）で再生経路を持つ。GBA は当初から駆動可能だった。
- **図形描画 primitive 実装（`dc6bdcd`）**: 戦闘アニメ Lib が多用する `Circle`/`Polygon`/`Arc` が no-op・`Oval` 未対応だったのを SRC 原典構文どおり実装。
  `script_overlay::DrawCmd` に `Circle`/`Oval`/`Polygon`/`Arc`＋`SetFillSolid`/`SetFillColor` を追加。`FillStyle`(塗り/透明)・`FillColor`(線色と独立)
  を状態分離（旧 fillcolor が SetColor に潰す配線を是正）。src-web は Canvas2D ellipse/arc_with_anticlockwise/path で描画（Arc は SRC の CCW 角度規約を
  y 下向き canvas へ `arc_with_anticlockwise` で変換、C# DrawArc 準拠）。catalog で Stub→Implemented へ昇格。native test 7 件。
- **ClearPicture 遅延クリア（`5e58a0b`）**: フレームループ `Paint; Refresh; ClearPicture; Wait`（Lib に 1989 箇所）で ClearPicture が Wait 前に
  overlay を即クリアし**毎フレーム空表示**＝アニメ不可視だった。SRC immediate-mode（ClearPicture はバックバッファのみ消し画面は次 present まで保持）を
  retained-overlay で再現: `ScriptOverlay.pending_clear`＋`defer_clear()`/`present()`、push() が新フレーム最初の描画で適用、`clear()`(シーン遷移) は即時。
  回帰 `battle_anim_frame_visible_during_wait`。
- **描画ペン状態の永続化（`8ab258a`）**: Color/FillStyle/FillColor/DrawWidth はループ外で 1 度設定し毎フレーム ClearPicture するため、cmds から消えると
  図形が既定色（白）に。SRC の ObjColor 等の永続性を再現＝`current_fill_solid`/`current_fill_color`/`current_line_width` 永続フィールド＋レンダラ seed・
  `clear()`(シーン遷移) でリセット・`defer_clear()`(ClearPicture) で保持。回帰 `pen_state_persists_across_clearpicture`。
- **実 fixture 解決パイプライン＋GBA 分岐の検証（`596f7bd`/`3d87adc`、`tests/battle_anim_lib.rs`）**: animation.txt の武器→`resolve_weapon`→Lib ラベル実在を突合。
  GBA クローズアップが `設定[全身戦闘アニメ]=オン` で分岐（スパロボ戦記は `スパロボ戦記.eve:48` で既定 ON）。実サブルーチン `戦闘アニメ_拡大小ビーム照射攻撃` が
  headless で **Wait まで完走**（ヘルパ・VFS file I/O 込み・ScriptError 無し）。**Phase 1 gap 監査の結論**: 戦闘アニメ Lib が使う命令動詞に未実装/Stub は 0 件。
  配線確認: combat が `対象ユニットＩＤ`/`相手ユニットＩＤ` を `try_play_battle_animation`（`app.rs:3654`）前に束縛（`:3282-3284`）。
- **実 combat 経路の検証（`11b9429`/`3d87adc`）**: 実 `attack_resolve_and_run`→戦闘アニメ起動でフレームループが正しく表示（ClearPicture 遅延クリア）。
  実 D 戦闘を `VERIFY_ANIMATE=1` で駆動し、実戦闘武器（animation.txt でクローズアップ sub に解決）の戦闘が ScriptError/panic なく完走することも確認。

### 2026-06-17 セッション（C# オラクル監査・差分 harness）

原典 C# `SRCCore` を macOS で動かし、式・コマンドの挙動を Rust と突合した監査セッション。
- **オラクル基盤**: `flake.nix` に `devShells.dotnet`（`nix develop .#dotnet`）を追加。`SRCCore`（netstandard2.1）＋
  `SRCCoreTests`（net10.0, **7490 テスト**）が macOS でビルド/実行可能（`43ac490`）。
- **式言語層の網羅監査（mining）**: 算術/演算子/Math/String/Format/時刻/正規表現の input→expected を C# テストから抽出し
  Rust に移植。**堅牢を確認**し、オラクルテスト計 51 件追加（`expression_oracle`/`math_function_oracle`/`string_function_oracle`/
  `function_oracle`/`combat_stat_oracle`）。**実バグ 1 件修正**: 式のゼロ除算が分子を残していた（`5/0==5`）→ SRC 準拠で 0（`b994e08`）。
- **ゲームロジック層の VB6 裏取り是正（実バグ 3 件）**: ① 機体改造 HP +100→**+200** / 装甲 +30→**+100**（`Unit.cls:1719-21`、`0458a0e`）。
  ② **exp→level 100→500/level**（`Pilot.cls:1183`、`06366f3`）。後者は実装中に **16 箇所重複**していた `total_exp/100` を正典関数
  `pilot_instance::level_from_exp` に集約（1 箇所漏れでレベル不整合になる罠を除去）、LevelUp コマンド n*500・修理/補給 exp 基準 100/150 も連動是正。
- **死にコード除去**: 未使用かつ壊れた `crates/src-core/src/expression/` モジュールを削除（`f83e237`）。
- **差分 harness 構築**（`tools/oracle-diff/` ＋ `verify-archive` の `oracle_eval`/`oracle_scenario` bin）: 同一式/コマンド列を
  C# SRCCore と Rust 両エンジンに通して自動 diff。**式層 75/76 一致**（Round 乖離を自動検出）・**Commands 層 9/9 一致**（`4427366`/`011e81e`）。
- **差分 harness をデータ層へ拡張（静的ユニット/パイロットデータ diff）**: 新設 `oracle_loaddata` bin（Rust）＋ C# `loaddata` モードが
  同一データディレクトリをロードし、`Info(ユニットデータ/パイロットデータ,…)` probe を両エンジンで diff（コーパス `unit_data.txt`、**58/61 一致**）。
  **実バグ 1 件を検出・是正（`803e13d`）**: pilot.txt 能力値行 5/6 番目の技量/反応 取り違え（VB6 `PilotDataList.cls:677-692` 準拠に是正、
  combat に波及していた）。残 3 件は既知乖離として記録（unit bare marker `全ユニット共通`・`性別` の `-`→空正規化・C# 組込みダミー件数 +1）。
- **差分 harness をユニット実体層へ拡張（stage a-2、`placeunit` モード）**: `@unit <name> <rank> <party>` で両エンジンが同一ユニットを生成
  （C#=`UList.Add`+`FullRecover`／Rust=`Create`）し `Info(ユニット,…)` を diff（コーパス `unit_instance.txt`、**24/25 一致**）。**実バグ 1 件を
  検出・是正（`135b5da`）**: `Create` の rank（改造段階）を無視していた→`upgrade_level` へ配線（rank 0/2/3/5 の HP/EN/装甲/運動性が cross-engine 一致）。
  残 1 件は既知乖離（`気力`: 無人ユニットで C# 空・Rust 既定 100）。
- **差分 harness を有人ユニット（パイロット実体）へ拡張（stage b、`@unit` 5 フィールド有人形式）**: コーパス `unit_pilot.txt`。**実バグ 1 件を
  検出・是正**: `Create` の level（主パイロット初期レベル）を無視していた→`exp_for_level` で初期 total_exp へ配線（レベル/累積経験値が cross-engine 一致）。
  **★★ pervasive バグを発掘・是正**: パイロットのレベル成長式が SRC と大きく乖離（旧 Rust=class ベース過大成長 / SRC=VB6 `Pilot.cls:582-593`
  `lv=Level`・格闘/射撃/技量/反応 +=lv・命中/回避 +=2*lv）。`grown_pilot`/`apply_stat_growth` を VB6 式へ是正（人工知能 lv10 格闘 旧190→110、
  超人工知能 lv30 415→155）＝全レベルアップ済みパイロットに波及していた。併せて `Info(パイロットデータ,…)` の成長 conflation も是正（静的データを返す）。
  `unit_pilot.txt` 13/13 一致。「level 1 でも成長」化に伴い成長系テスト 5 件を VB6 値へ更新。
- **乖離記録**: `docs/SRC_SHARP_DIVERGENCE.md` §4（是正済）に 技量/反応 取り違え・Create rank/level・**パイロット成長式**・パイロットデータ成長 conflation を、
  乖離候補に Round・Not 優先順位・Set-& 寛容差・Pilot/Unit inline 形式・性別/クラス別名/全ユニット共通/気力 を記録。
- **テスト**: `cargo test -p src-core` **1937 件**全緑 / clippy `-D warnings` / wasm check OK。
- **教訓**: 式層は mining で堅牢確認（収穫逓減）／ゲームロジックに pitfall 集中だが**オラクル自身も移植で VB6 裏取りが決定的**／
  Commands 層は mining 不可だが差分 harness で検証可能。詳細は memory `reference_csharp_oracle`。

### 2026-06-16 セッション（`feat/necessary-skill-gate`）

- **式評価エンジンの 2 バグを修正（実エンジンバグ、未駆動 fixture `温泉旅館.zip`＝非戦闘の経営シムを駆動して発見）**:
  ① **括弧無し `var = expr` 算術代入の式評価（`d1d2c85`）**: `資本金 = 資本金 - 営業収支` / `HP = HP - 10` / `カウンタ = カウンタ + 1` のような
  括弧無し算術を持つ `=` 代入が生の式文字列のまま格納されていた（従来は括弧付き `(a - b)` のみ評価）。SRC `ExecSetCmd` は値を `EvalTerm` で式評価し
  数値型なら数値代入するため準拠。`eval_arith_value`（括弧の有無に依らず算術式を数値化、共有ガード `eval_numeric_atoms`）を新設し、適用は `var = expr` 形
  （assign sugar が生成する内部コマンド `__assign`）に限定。`Set var value` 形は従来どおり括弧付きのみ評価（パーサが引用符を剥がすため形では区別できず、
  引用符付き文字列 `"$(a)-$(b)-$(c)"`→`1-2-3` を `-4` に潰す／Format 出力 `-05` を `-5` に潰す誤数値化を `=`/`Set` の形で防ぐ）。回帰 7 件。
  ② **数値関数引数の裸変数算術の解決（`bd90843`）**: `Round(温泉宿１収入 + 温泉宿２収入)` のように数値関数の引数が裸変数を含む算術式のとき、`numeric_arg` が
  裸変数を解決できず評価に失敗していた（従来は `$(...)` 補間のみ）。`numeric_arg` を `eval_numeric_atoms`（全アトムが数値/数値変数のときのみ評価し、非数値文字列は
  None＝LSet/RSet の「numeric なら数値・else 文字列」契約を維持）で解決するよう修正。数値関数は既に numeric_arg 経由・文字列関数は fn_arg_value 直呼びで非影響・
  ネスト関数は expand_vars 事前展開。回帰 4 件。**両修正で温泉旅館の収入計算 cascade（`温泉宿収入=Round(...)`→`営業収入`→`営業収支`→`資本金`）が
  ヘッドレスドライブで end-to-end 評価されることを実証**（`温泉宿１収入=25.2`/`営業収支=-38`/`資本金=500`）。教訓: **非戦闘シナリオ（経営/AVG 系）は式評価エンジンを
  深く突くので新種バグの発掘源**（combat シナリオは関数引数に `$(...)`/リテラルを使うため露見しにくい）。
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
  シナリオの救済）。⑦ **3 本目の実シナリオ `スーパーヒーロー伝説`（らんま系）を Briefing→Battle まで完走確認**（バグ無し・debug bypass 無し。
  撃破/反撃/クリティカル/資金/EXP すべて正常）＝**最もクリーンな end-to-end combat 検証**。
  > **注（2026-06-17 追記）**: 現在の既定ドライブ（VERIFY_ASK=1 の総当たりではない単純駆動）では `校長→乱馬` 撃破で **Defeat** に終わる
  > （`乱馬→九能 撃破` 等、味方も交戦は成立＝combat 健全。勝敗はドライブの非戦略性＋必要技能ゲートでの武器選択変化による正当な帰結で、`ce9e104`＝本会話の作業前から同一。
  > 本セッションの式評価/魅了憑依の変更は worktree 比較で Defeat の原因でないと確認済＝**回帰ではない**）。当初メモの「Victory」は必要技能ゲート配線前の状態の記録。
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
