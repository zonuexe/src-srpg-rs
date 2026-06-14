# SRC.Sharp Help (menu.md) ⇄ Rust 実装カバレッジレポート

本レポートは [`SRC.Sharp/SRC.Sharp.Help/src/menu.md`](../SRC.Sharp/SRC.Sharp.Help/src/menu.md)
の目次カテゴリを軸に、

- 公式仕様文書 (`SRC.Sharp.Help/src/*.md`)
- C# 移植実装 (`SRC.Sharp/SRC.NET/*.cs`)
- 現行 Rust 実装 (`crates/src-core/src/**`)

の対応関係をまとめ、Rust 側の実装状況を概観する。

評価記号:

| 記号 | 意味 |
|------|------|
| ✅ | ほぼ完全実装 (実シナリオ動作確認 + テスト網羅) |
| 🟡 | 部分実装 / 制約あり (一部引数未対応、副作用不完全 など) |
| 🔴 | スタブ / 未実装 / プレースホルダのみ |
| ⚪ | UI / 仕様文書専用で Rust 実装の対象外 (将来 UI 実装で対応) |
| ❓ | 未調査 / 評価保留 |

> NOTE: 「Rust 実装」列は対応する **可能性が高い** 場所であり、その時点で
> 実装されていなくてもパス候補が示される。「評価」列が未記入 / ❓ の
> 行は、後続の評価パスで埋められる。

---

## 0. ナビゲーション

- [はじめに / お使いになる前に / インストール](#1-はじめに--お使いになる前に--インストール)
- [操作方法](#2-操作方法)
  - [画面の説明](#21-画面の説明)
  - [ユニットコマンド](#22-ユニットコマンド)
  - [マップコマンド](#23-マップコマンド)
  - [インターミッションコマンド](#24-インターミッションコマンド)
- [データの作成](#3-データの作成)
  - [基本データ](#31-基本データ)
  - [パイロット用特殊能力リファレンス](#32-パイロット用特殊能力リファレンス)
  - [ユニット用特殊能力リファレンス](#33-ユニット用特殊能力リファレンス)
  - [武器属性リファレンス](#34-武器属性リファレンス)
- [シナリオの作成](#4-シナリオの作成)
  - [基本概念](#41-基本概念)
  - [イベントラベルリファレンス](#42-イベントラベルリファレンス)
  - [イベントコマンドリファレンス](#43-イベントコマンドリファレンス)
  - [関数リファレンス](#44-関数リファレンス)
- [最後に](#5-最後に)

---

## 1. はじめに / お使いになる前に / インストール

ランタイム挙動には関与しないドキュメント類。`src-web` の UI/初回起動ハンドリング
で部分的に対応するが、Rust 実装の評価対象外。

| 項目 | Help (`SRC.Sharp.Help/src/`) | SRC.NET (`SRC.Sharp/SRC.NET/`) | Rust 実装 (`crates/src-core/` 他) | 評価 |
|------|---|---|---|------|
| はじめに | はじめに.md | — | — | ⚪ |
| 動作環境 | 動作環境.md | — | `README.md` / `flake.nix` で代替 | ⚪ |
| 使用条件 | 使用条件.md | — | `gpl-3.0.jp.txt` / `gpl-3.0.txt` | ⚪ |
| SRCとの互換性 | SRCとの互換性.md | — | `docs/SRC_SHARP_DIVERGENCE.md` | ⚪ |
| インストール手順 / アンインストール / MIDI 等 | インストールの手順.md ほか 9 ファイル | — | ZIP アーカイブを `src-web/archive.rs` が展開、`assets/` を fetch | ⚪ |

---

## 2. 操作方法

### 2.1 画面の説明

| 項目 | Help | SRC.NET | Rust 実装 | 評価 | コメント |
|------|---|---|---|------|---|
| 画面の説明 | 画面の説明.md | `Main.cs` / `Map.cs` | `crates/src-web/src/render.rs` + `crates/src-core/src/scene/` | 🟡 | scene/ enum と render フレーム有、StatusWindow 等は未実装 |
| マップウインドウ | マップウインドウ.md | `Map.cs` | `crates/src-core/src/scene/map_view.rs` | 🟡 | 480x480/15x15 タイル/カーソル/スクロール実装済 |
| ステータスウインドウ | ステータスウインドウ.md | `Status.cs` | (未実装 / プレースホルダ) | 🔴 | 専用シーン無し。右パネルに簡易表示のみで原典の詳細項目欠 |
| メッセージウインドウ | メッセージウインドウ.md | `Message.cs` | `crates/src-core/src/dialog.rs` + `crates/src-core/src/scene/map_view.rs` | 🟡 | Talk/Confirm/Menu/Input ダイアログ動作。Telop 未実装 |
| SRCの起動 | SRCの起動.md | `SafeMain.cs` / `Main.cs` | `crates/src-core/src/app.rs::App::new` + `crates/src-core/src/entrypoint.rs` | 🟡 | App::new / entrypoint 経路有、初期化フロー簡略化 |
| ゲームの流れ | ゲームの流れ.md | `Main.cs` | `crates/src-core/src/stage.rs` + `crates/src-core/src/turn.rs` | 🟡 | stage.rs StageState (Briefing/Sortie/Battle/Victory) + turn.rs |
| 基本操作 | 基本操作.md | `Main.cs` / `Map.cs` | `crates/src-core/src/app.rs::Input` + `crates/src-web/src/input.rs` | 🟡 | Input enum で主要操作実装。右クリック等は限定的 |

### 2.2 ユニットコマンド

参照: [`crates/src-core/src/command_menu.rs`](../crates/src-core/src/command_menu.rs)
(`UnitAction` enum), [`crates/src-core/src/app.rs::invoke_custom_unit_command`].

| 項目 | Help | SRC.NET | Rust 実装 | 評価 | コメント |
|------|---|---|---|------|---|
| 移動 | 移動.md | `Command.cs::Move_Click` | `command_menu.rs::UnitAction::Move` / `movement.rs` | ✅ | UnitAction::Move + movement.rs Dijkstra、テスト有 |
| 攻撃 | 攻撃.md | `Command.cs::Attack_Click` | `command_menu.rs::UnitAction::Attack` / `combat.rs` | ✅ | UnitAction::Attack + combat.rs、ダメージ/命中テスト網羅 |
| アビリティ | アビリティ.md | `Command.cs` (Ability) | `unit_ability.rs` (実行時状態のみ。メニュー未実装) | 🔴 | unit_ability.rs 実行時状態のみ。メニュー項目も発動経路も無し |
| 会話 | 会話.md | `Command.cs::Talk_Click` | `event_runtime.rs` の `Talk` / 会話イベントトリガ | 🟡 | Talk スクリプトのみ。メニュー「会話」項目欠落 |
| 修理 | 修理.md | `Command.cs::Repair_Click` | `event_runtime.rs::"fix"` (スクリプト用)、メニュー未実装 | 🟡 | fix スクリプトのみ。UnitAction メニュー項目欠落 |
| 補給 | 補給.md | `Command.cs::Supply_Click` | `event_runtime.rs::"supply"` (スクリプト用)、メニュー未実装 | 🟡 | supply スクリプトのみ。UnitAction メニュー項目欠落 |
| チャージ | チャージ.md | `Command.cs::Charge_Click` | (未実装) | 🔴 | 未実装。スクリプト/メニュー双方欠落 |
| スペシャルパワー | スペシャルパワー.md | `Command.cs::SP_Click` | `event_runtime.rs::"specialpower"` (スクリプト用)、メニュー未実装 | 🟡 | specialpower スクリプト + SP コスト計算有。メニュー欠落 |
| 変形 | 変形.md | `Command.cs::Transform_Click` | `event_runtime.rs::"transform"` | 🟡 | transform スクリプト有 (unit_data_name 差替えのみ) |
| 分離 | 分離.md | `Command.cs::Split_Click` | `event_runtime.rs::"split"` | 🟡 | split スクリプト有 (features 参照)。メニュー欠落 |
| ハイパーモード | ハイパーモード.md | `Command.cs` | (未実装) | 🔴 | 未実装 |
| 合体 | 合体.md | `Command.cs::Combine_Click` | `event_runtime.rs::"combine"` | 🟡 | combine スクリプト有 (unit_data_name 差替えのみ) |
| 地上 / 空中 / 地中 / 水上 / 水中 | 地上.md ほか | `Command.cs::Land/Air/...` | `event_runtime.rs::"land"`/`"air"`/`"water"`/`"sea"`/`"cosmos"`/`"diving"` + 日本語 alias + `UnitInstance.current_area` | ✅ | 全 6 領域コマンド実装、`Area()` 関数も current_area 優先で連動 |
| 発進 | 発進.md | `Command.cs::Launch_Click` | `event_runtime.rs::"launch"` | 🟡 | launch スクリプト有。メニュー欠落 |
| アイテム | アイテム.md | `Command.cs::Item_Click` | `item_slot.rs` + `event_runtime.rs::"exchangeitem"` | 🟡 | item_slot.rs + exchangeitem 有。Item コマンド未実装 |
| 召喚解除 | 召喚解除.md | `Command.cs::StopSummoning_Click` | (未実装) | 🔴 | stopsummoning は no-op スルー |
| 命令 | 命令.md | `Command.cs::Order_Click` | (未実装) | 🔴 | 未実装 |
| 特殊能力一覧 | 特殊能力一覧.md | `Command.cs` (Status 経由) | (未実装) | 🔴 | 表示ロジックも UI も無し |
| 武装一覧 | 武装一覧.md | `Command.cs` | `command_menu.rs::UnitAction::WeaponList` | 🟡 | UnitAction::WeaponList でメッセージ列挙のみ |
| アビリティ一覧 | アビリティ一覧.md | `Command.cs` | (未実装) | 🔴 | 未実装 |
| 待機 | 待機.md | `Command.cs::Wait_Click` | `command_menu.rs::UnitAction::Wait` | ✅ | UnitAction::Wait で has_acted 設定、テスト有 |

### 2.3 マップコマンド

参照: [`crates/src-core/src/command_menu.rs`](../crates/src-core/src/command_menu.rs) `MapAction` enum.

| 項目 | Help | SRC.NET | Rust 実装 | 評価 | コメント |
|------|---|---|---|------|---|
| ターン終了 | ターン終了.md | `Map.cs::EndTurn_Click` | `MapAction::EndTurn` / `turn.rs::end_phase` | ✅ | MapAction::EndTurn + turn.rs::end_phase、テスト有 |
| 中断 | 中断.md | `Map.cs::Suspend_Click` | (未実装) | 🔴 | Suspend 未実装 |
| 部隊表 | 部隊表.md | `Map.cs::UnitList_Click` | `MapAction::UnitList` → `scene/unit_list.rs` | 🟡 | MapAction::UnitList → PilotList シーン (一覧のみ) |
| スペシャルパワー検索 | スペシャルパワー検索.md | `Map.cs` | (未実装) | 🔴 | 未実装 |
| 全体マップ | 全体マップ.md | `Map.cs::Overview_Click` | (未実装) | 🔴 | 未実装 (overview 無し) |
| 作戦目的 | 作戦目的.md | `Map.cs::Mission_Click` | (未実装) | 🔴 | 未実装 (briefing 文字列のみ) |
| 自動反撃モード | 自動反撃モード.md | `Map.cs` | (未実装) | 🔴 | 未実装 |
| 設定変更 | 設定変更.md | `Configuration.cs` | `MapAction::Settings` / `scene/configuration.rs` | 🟡 | Configuration シーン有・レイアウトと hit_test 実装 |
| リスタート | リスタート.md | `Map.cs::Restart_Click` | (未実装) | 🔴 | 未実装 |
| クイックロード | クイックロード.md | `Map.cs::QuickLoad_Click` | `MapAction::QuickLoad` (実装の有無は要確認) | 🔴 | MapAction 有、push_message のみのスタブ |
| クイックセーブ | クイックセーブ.md | `Map.cs::QuickSave_Click` | `MapAction::QuickSave` | 🔴 | MapAction 有、push_message のみのスタブ |

### 2.4 インターミッションコマンド

参照: [`crates/src-core/src/scene/intermission.rs`](../crates/src-core/src/scene/intermission.rs),
[`crates/src-core/src/app.rs::IntermissionCommandEntry`].

| 項目 | Help | SRC.NET | Rust 実装 | 評価 | コメント |
|------|---|---|---|------|---|
| SRCを終了 | SRCを終了.md | `Intermission.cs::ExitGame_Click` | (未実装) | 🔴 | ExitGame 未実装 |
| 次のマップへ | 次のマップへ.md | `Intermission.cs::NextStage_Click` | `stage.rs::StageState` 遷移 | 🟡 | advance_to_next_stage で「次ステージ」変数遷移 |
| データーセーブ | データーセーブ.md | `Intermission.cs::SaveData_Click` | `App::to_save_json` / `event_runtime.rs::"savedata"` | 🟡 | savedata は script_var 格納のみ。実保存はフロント側 |
| 機体改造 | 機体改造.md | `Intermission.cs::Upgrade_Click` | `event_runtime.rs::"upgrade"` (スクリプト用)、UI 未実装 | 🟡 | upgrade スクリプトのみ (hp/en/armor 等)。UI 未実装 |
| 乗り換え | 乗り換え.md | `Intermission.cs::Replace_Click` | `event_runtime.rs::"replacepilot"` (スクリプト用) | 🟡 | replacepilot スクリプトのみ。空席/降車ロジック無 |
| アイテム交換 | アイテム交換.md | `Intermission.cs::ItemExchange_Click` | `event_runtime.rs::"exchangeitem"` | 🟡 | exchangeitem スクリプトのみ。UI 未実装 |
| 換装 | 換装.md | `Intermission.cs::Equip_Click` | (未実装) | 🔴 | Equip コマンド未実装 |
| パイロットステータス | パイロットステータス.md | `Status.cs` (Pilot) | `scene/pilot_list.rs` (一覧のみ) | 🟡 | pilot_list.rs カラム表示のみ。詳細画面無し |
| ユニットステータス | ユニットステータス.md | `Status.cs` (Unit) | `event_runtime.rs::"showunitstatus"` | 🟡 | showunitstatus スクリプトのみ。専用シーン無し |

---

## 3. データの作成

### 3.1 基本データ

| 項目 | Help | SRC.NET | Rust 実装 | 評価 | コメント |
|------|---|---|---|------|---|
| データを作成する前に | データを作成する前に.md | — | — | ⚪ | ドキュメントのみ |
| データ形式 | データ形式.md | (各 `*DataList.cs`) | `crates/src-core/src/data/loader.rs` | 🟡 | loader.rs: 行/レコード/コメント/SJIS 対応済 |
| パイロットデータ | パイロットデータ.md | `PilotDataList.cs` / `PilotData.cs` | `crates/src-core/src/data/pilot.rs` | 🟡 | 基本+stats+SP+BGM+特殊能力 KV のみ、武器/SP習得Lv 未連動 |
| パイロット用特殊能力 (概論) | パイロット用特殊能力.md | `SkillData.cs` | `crates/src-core/src/feature.rs` (汎用) | 🟡 | features は KV 保持のみ、効果未連動 (3.2 参照) |
| 非戦闘員データ | 非戦闘員データ.md | `NonPilotData.cs` / `NonPilotDataList.cs` | (未実装) | 🔴 | NonPilotData パーサ・実装なし (Info の enum 名のみ) |
| ユニットデータ | ユニットデータ.md | `UnitDataList.cs` / `UnitData.cs` | `crates/src-core/src/data/unit.rs` | 🟡 | 基本+stats+adaption+武器+features を heuristic で取込 |
| ユニット用特殊能力 (概論) | ユニット用特殊能力.md | `FeatureData.cs` | `crates/src-core/src/feature.rs` | 🟡 | UnitData.features に KV のみ。効果は combat 未連動 |
| 武器属性 | 武器属性.md | `WeaponData.cs` | `crates/src-core/src/data/unit.rs` (武装パース部) + `unit_weapon.rs` | 🟡 | WeaponData 主要フィールドあり (3.4 参照)、必要技能/条件は extras 行き |
| 必要技能 | 必要技能.md | `WeaponData.cs` | (未実装) | 🔴 | extras に未解釈で保持、判定ロジックなし |
| アビリティ効果 | アビリティ効果.md | `AbilityEffect.cs` / `AbilityData.cs` | `crates/src-core/src/unit_ability.rs` | 🔴 | UnitAbility 構造体は名前/値のみ、回復/補給等の効果未実装 |
| メッセージデータ | メッセージデータ.md | `MessageData.cs` / `MessageDataList.cs` | (未実装) | 🔴 | MessageData パーサ・型なし |
| ダイアログデータ | ダイアログデータ.md | `DialogData.cs` / `DialogDataList.cs` | `crates/src-core/src/dialog.rs` | 🟡 | PendingDialog (UI モーダル) のみ。pilot_dialog.txt パース未実装 |
| 戦闘アニメデータ | 戦闘アニメデータ.md | `Effect.cs` / `Graphics.cs` | (未実装) | 🔴 | animation.txt パーサ・実装なし |
| 特殊効果データ | 特殊効果データ.md | `Effect.cs` | (未実装) | 🔴 | effect.txt パーサ・実装なし |
| アイテムデータ | アイテムデータ.md | `ItemDataList.cs` / `ItemData.cs` | `crates/src-core/src/data/item.rs` + `item_slot.rs` | 🟡 | name/class/part/stat修正+features 解釈、武器/特殊効果未対応 |
| スペシャルパワーデータ | スペシャルパワーデータ.md | `SpecialPowerData.cs` / `SpecialPowerDataList.cs` | `crates/src-core/src/data/special_power.rs` | 🟡 | name/short/SP/target/duration のみ、Cond/Anim 未対応 |
| エリアスデータ | エリアスデータ.md | `AliasData.cs` / `AliasDataList.cs` | (未実装) | 🔴 | alias.txt パーサ・実装なし |
| 地形データ | 地形データ.md | `TerrainData.cs` / `TerrainDataList.cs` | `crates/src-core/src/data/terrain.rs` + `terrain_file.rs` | 🟡 | terrain_file パーサ+移動コスト連動、features 列は未解釈 |
| 戦闘システム詳細 | 戦闘システム詳細.md | `Command.cs` (Attack) | `crates/src-core/src/combat.rs` | 🟡 | combat.rs に命中/ダメージ式あり、底力/サイズ補正等は近似/欠落 |
| バトルコンフィグデータ | バトルコンフィグデータ.md | `BattleConfigData.cs` / `BattleConfigDataList.cs` | (未実装) | 🔴 | battle.txt パーサ・実装なし |
| 行動パターン | 行動パターン.md | `Map.cs` (CPU AI) | (未実装) | 🔴 | CPU AI 未実装 |

### 3.2 パイロット用特殊能力リファレンス

`SRC.NET/SkillData.cs` および各 Help md が原典。Rust 側は
[`crates/src-core/src/feature.rs`](../crates/src-core/src/feature.rs) と
スクリプト関数 `Skill(...)` / `SetSkill` / `ClearSkill` で扱う。
個別特殊能力ごとの効果実装は `combat.rs` / `condition.rs` などに分散。

| カテゴリ (md) | Rust 関連実装 | 評価 | コメント |
|---|---|---|---|
| 防御・回避に関する特殊能力.md | `combat.rs` (回避・装甲補正) | 🔴 | feature 文字列保持のみ、Ｓ防御/切り払い/分身/耐久いずれも未実装 |
| 攻撃に関する特殊能力.md | `combat.rs` (与ダメ補正) | 🔴 | 潜在力/得意技/ハンター/カウンター等 全て参照箇所なし |
| 特異資質に関する特殊能力.md | (未実装) | 🔴 | 超感覚/念力/オーラ等 命中・回避修正に未連動 |
| 瀕死時に発動する特殊能力.md | (未実装) | 🔴 | 底力/超底力/覚悟/起死回生 いずれも HP 連動効果なし |
| 援護行動に関する特殊能力.md | (未実装) | 🟡 | サポートアタックは実装、援護防御は未実装 |
| サポート系特殊能力.md | (未実装) | 🔴 | 命中/格闘/射撃サポート 等の補正処理が存在しない |
| パイロット成長に関する特殊能力.md | `event_runtime.rs::"expup"`, `"levelup"` 周辺 | 🟡 | levelup/expup コマンドはあるが素質/成長系 feature は未連動 |
| スペシャルパワーに関する特殊能力.md | `data/special_power.rs` + 関連 SP コマンド | 🟡 | SP消費減少のみ反映、自動発動/集中力/精神統一 未実装 |
| 気力に関する特殊能力.md | `pilot_instance.rs::morale` + `event_runtime.rs::"increasemorale"` | 🟡 | morale 値と increasemorale はあるが闘争本能/上限下限 未実装 |
| その他の特殊能力 (パイロット).md | `feature.rs` + 個別 | 🔴 | 術/技/英雄/再生/資金獲得 等は参照されない |

### 3.3 ユニット用特殊能力リファレンス

`FeatureData.cs` 由来。Rust では `feature.rs::ActiveFeature` がキャッチオール。

| カテゴリ (md) | Rust 関連実装 | 評価 | コメント |
|---|---|---|---|
| 防御特性に関する特殊能力.md | `combat.rs` | 🔴 | 吸収/無効化/耐性/弱点 いずれも combat.rs に分岐なし |
| 防御系特殊能力.md | `combat.rs` | 🔴 | シールド/エネルギーシールド 自動発動なし |
| 回避系特殊能力.md | `combat.rs` | 🔴 | 分身/超回避/緊急テレポート 全て未実装 |
| 回復系特殊能力.md | `event_runtime.rs::"recoverhp"`, `"recoveren"` + ターンエンド処理 | 🔴 | 修理装置/補給装置/母艦 自動回復・搭載 未実装 (fix/supply スクリプトのみ) |
| コンバータ系特殊能力.md | (未実装) | 🔴 | 霊力変換器/オーラ変換器/サイキックドライブ 全て未実装 |
| 移動系特殊能力.md | `movement.rs` | 🟡 | 基本 Dijkstra のみ、テレポート/ジャンプ/透過/すり抜け 未連動 |
| 変形系特殊能力.md | `event_runtime.rs::"transform"` | 🟡 | transform コマンド経由で形態切替可、換装/パーツ分離は未対応 |
| パイロット関連特殊能力.md | `pilot_instance.rs` | 🔴 | 追加パイロット/暴走時/追加サポート いずれも未実装 |
| アイテム関連特殊能力.md | `item_slot.rs` | 🟡 | ItemSlot/装備個所 string 取得は可、武器クラス/ハードポイント 未連動 |
| 武器関連特殊能力.md | `unit_weapon.rs` + `combat.rs` | 🔴 | 合体技/変形技/追加攻撃 ユニット特殊能力としては未実装 |
| ＢＧＭ関連特殊能力.md | `event_runtime.rs::"startbgm"` 周辺 | 🟡 | startbgm/stopbgm コマンドのみ、武器/合体/勝利 ＢＧＭ 未対応 |
| ユニット改造関連特殊能力.md | `event_runtime.rs::"upgrade"` | 🟡 | upgrade コマンド有、最大改造数/改造費修正/ランクアップ 未実装 |
| ユニット強化関連特殊能力.md | (未実装) | 🟡 | 装甲強化のみ部分参照、HP/EN/運動性/移動力強化 未連動 |
| その他の特殊能力 (ユニット).md | `feature.rs` | 🔴 | 制御不可/不安定/暴走/部隊ユニット 等 全て未実装 |

### 3.4 武器属性リファレンス

Rust 側武器: `crates/src-core/src/data/unit.rs` の武器パース + `unit_weapon.rs`。
属性ごとの効果は `combat.rs` 内で散在解釈される。

| カテゴリ (md) | Rust 関連実装 | 評価 | コメント |
|---|---|---|---|
| ダメージ算出方法に関する属性.md | `combat.rs` | 🔴 | 格/射/複/連属性 combat.rs で分岐なし、weapon.power 一律 |
| 使用可能時に関する属性.md | `combat.rs` (発動条件) | 🟡 | Ｐ/Ｑ のみ app.rs で参照、攻/反/瀕 未対応 |
| 攻撃種類に関する属性.md | `combat.rs` | 🔴 | 武/突/接/Ｊ 等 切り払い対象判定なし |
| 攻撃の実行手順に関する属性.md | `combat.rs` | 🔴 | 先/後/再 反撃順制御や再攻撃判定なし |
| マップ攻撃に関する属性.md | `event_runtime.rs::"mapattack"` + `combat.rs` | 🟡 | Ｍ全/投/直/拡/扇/線 形状判定 + テストあり、反撃/経験値処理省略 |
| パイロット能力と連動する属性.md | `combat.rs` | 🔴 | オ/超/シ/サ 攻撃力修正・射程拡張 未連動 |
| 攻撃対象を特定する属性.md | `combat.rs` (filter) | 🔴 | 封/限/♂/♀/対 フィルタ未実装 |
| 使用時のペナルティに関する属性.md | (未実装) | 🔴 | 気L/霊L/失L/銭L 消費処理なし |
| ＥＮ消費量に関する属性.md | `unit_weapon.rs` | 🟡 | 武器 en_consumption フィールドは消費反映、術/技/尽属性連動なし |
| 弾数に関する属性.md | `unit_weapon.rs` | 🟡 | UnitWeapon.bullet_remaining + テスト有、共/斉/永 未対応 |
| チャージ攻撃に関する属性.md | (未実装) | 🔴 | Ｃ/Ａ チャージ式・自動チャージ 未実装 |
| 合体技に関する属性.md | `event_runtime.rs::"combine"` | 🟡 | combine コマンドで形態合体は可、合属性発動条件チェックなし |
| 変形技に関する属性.md | `event_runtime.rs::"transform"` | 🟡 | transform コマンドはあるが 変属性 自動発動なし |
| 特殊効果攻撃属性.md | `condition.rs` | 🔴 | Ｓ/縛/麻/凍/毒 等 命中時の状態異常付与なし |
| 防御能力を無効化する属性.md | `combat.rs` | 🔴 | 貫/固/殺 装甲無効/ダメージ固定 combat.rs で未分岐 |
| 吸収攻撃に関する属性.md | (未実装) | 🔴 | 吸/減/奪 HP/EN 吸収未実装 |
| 攻撃力変動に関する属性.md | `combat.rs` | 🔴 | Ｒ/改/体 ユニットランク連動なし |
| 命中率変動に関する属性.md | `combat.rs` | 🔴 | Ｈ/追/有/誘/空/散 命中補正分岐なし |
| クリティカル率変動に関する属性.md | `combat.rs` | 🔴 | 忍/暗殺技 等 CT 率補正なし (critical 自体未使用) |
| クリティカル時のダメージ増加量に関する属性.md | `combat.rs` | 🔴 | 痛属性 ダメージ増加倍率の処理なし |

---

## 4. シナリオの作成

### 4.1 基本概念

| 項目 | Help | SRC.NET | Rust 実装 | 評価 | コメント |
|------|---|---|---|------|---|
| シナリオを作成する前に | シナリオを作成する前に.md | — | — | ⚪ | ドキュメントのみ、Rust 対象外 |
| シナリオの構成 | シナリオの構成.md | `Event.cs` (entry) | `crates/src-core/src/app.rs` + `event_runtime.rs::ScriptLibrary` | 🟡 | App + ScriptLibrary で .eve 集約、複数ファイル対応 |
| イベントデータ | イベントデータ.md | `LabelData.cs` / `Event.cs` | `crates/src-core/src/data/event.rs` | ✅ | data/event.rs で .eve パーサ、line continuation 対応 |
| イベントラベル | イベントラベル.md | `LabelData.cs` | `event_runtime.rs::label_pc_*` | 🟡 | canonical_label/_full でラベル収集、`*`/`@`/`:` 受理 |
| イベントコマンド (概論) | イベントコマンド.md | `Event.cs` / `Command.cs` | `event_runtime.rs::exec_command_pc` | 🟡 | exec_command_pc で約 70/170 コマンド dispatch |
| 式 | 式.md | `Expression.cs` | `crates/src-core/src/expression/` + `event_runtime.rs::expand_arg` | 🟡 | expression/eval.rs + 演算子, 関数登録あり、未統合経路 |
| 変数 | 変数.md | `VarData.cs` / `BCVariable.cs` | `app.rs::script_vars` + Local/Global/Args | 🔴 | script_vars は単一 BTreeMap、Local/Global/Sub-local 未分離 |
| 関数 | 関数.md | `Expression.cs` | `event_runtime.rs::eval_function_call` 周辺 | 🟡 | eval_script_function + expression/functions/ で多数対応 |
| Systemフォルダ | Systemフォルダ.md | (loader系) | `crates/src-web/src/archive.rs` (System 探索) | 🔴 | entrypoint.rs で減点ヒント程度、System/ 解決ロジック無 |
| マップデータ | マップデータ.md | (mapパーサ) | `crates/src-core/src/data/map.rs` | ✅ | data/map.rs でパース + MapData、Layer 対応 |

### 4.2 イベントラベルリファレンス

ラベルトリガは `event_runtime.rs::trigger_label*` と各シーン遷移点で発火する。

| ラベル (md) | SRC.NET 発火点 | Rust 発火点 | 評価 | コメント |
|---|---|---|---|---|
| プロローグイベント.md | `Event.cs::Prologue` | `stage.rs` 遷移 → `trigger_label("プロローグ")` | ✅ | start_scenario で発火、stage_state_pipeline で検証 |
| スタートイベント.md | `Event.cs::Start` | `stage.rs` Briefing/Sortie 移行時 | ✅ | begin_battle で trigger_label_in_file → 全域 fallback、テスト有 |
| エピローグイベント.md | `Event.cs::Epilogue` | `stage.rs` 遷移 | 🟡 | Continue 命令経由で jump_to のみ。自動発火点なし |
| ターンイベント.md | `Event.cs` ターン進行 | `turn.rs::end_phase` 周辺 | ✅ | begin_phase で Turn N / ターン N / Turn N <party> 発火 |
| 損傷率イベント.md | `Event.cs` ダメージ後 | `event_runtime.rs::fire_damage_threshold_labels` | 🟡 | apply_damage 内で閾値跨ぎ判定 + 発火、fixture 19 で検証。Attack/MapAttack 区別 (発火しない条件) は未対応 |
| 破壊イベント.md | `Event.cs` | `event_runtime.rs::fire_destruction_labels` | ✅ | pilot/unit × 日英、fixture 16 で検証 |
| 全滅イベント.md | `Event.cs` | `event_runtime.rs::fire_total_annihilation_if_any` | ✅ | 4 陣営チェック、fixture 18 で検証 |
| 攻撃イベント.md | `Event.cs` | `event_runtime.rs::fire_attack_event_labels` | 🟡 | `app.rs::attack_target` 開戦直前で発火、lib テストで pilot×unit×party 交差検証 |
| 攻撃後イベント.md | `Event.cs` | `event_runtime.rs::fire_after_attack_event_labels` | 🟡 | `app.rs::attack_target` 末尾で発火、双方生存判定 + lib テスト有 |
| 会話イベント.md | `Event.cs::Talk` | `event_runtime.rs::"talk"` | 🔴 | whitelist 登録のみ、Conversation 自動発火点無 (Talk はコマンド) |
| 接触イベント.md | `Event.cs` | `event_runtime.rs::fire_contact_event_labels` | 🟡 | UI 移動/Wait/Attack 完了後、4 近傍ペアで `接触 <unit1> <unit2>` を試行発火、lib test 有 |
| 進入イベント.md | `Event.cs` | `event_runtime.rs::fire_entry_event_labels` | 🟡 | `app.rs::try_move_unit_to` 完了時に `進入 <unit> <x> <y>` + 地形名形式を試行、lib test 有。0-based 座標 (SRC は 1-based) で乖離 |
| 脱出イベント.md | `Event.cs` | `event_runtime.rs::fire_entry_event_labels` (連鎖) | 🟡 | 進入完了直後にマップ端方位 N/S/E/W を判定して `脱出 <unit> <dir>` 連鎖発火、lib test 有 |
| 収納イベント.md | `Event.cs` | `App::fire_boarding_event` + `"stow"` arm | 🟡 | UI/スクリプトから明示的に発火可能。母艦着艦 (Land 経由) の自動 hook は未実装 |
| 使用イベント.md | `Event.cs` | `event_runtime.rs::fire_use_event_labels` | 🟡 | `app.rs::attack_target` 開戦直前 (攻撃イベント前) に `使用 <unit> <weapon>` を発火、lib test 有 |
| 使用後イベント.md | `Event.cs` | `event_runtime.rs::fire_after_use_event_labels` | 🟡 | `攻撃後` の前、attacker 生存時のみ `使用後 <unit> <weapon>` を発火、lib test 有 |
| 変形イベント.md | `Event.cs` | `event_runtime.rs::"transform"` | ✅ | Transform 完了後 `変形 <unit> <new>:` 発火、fixture 21 で検証 |
| 合体イベント.md | `Event.cs` | `event_runtime.rs::"combine"` | ✅ | Combine 完了後 `合体 <unit> <new>:` 発火、fixture 21 で検証 |
| 分離イベント.md | `Event.cs` | `event_runtime.rs::"split"` | ✅ | Split 完了後 `分離 <unit> <old>:` 発火、lib テスト (`split_fires_separation_event_label`) で検証 |
| 行動終了イベント.md | `Event.cs` | `event_runtime.rs::fire_action_end_labels` | 🟡 | `app.rs` の UI 経路 (Wait/Attack) で発火、lib テスト有。unit_instance.rs 内部の has_acted セットは未連動 |
| レベルアップイベント.md | `Event.cs` | `event_runtime.rs::"expup"` | 🟡 | ExpUp 経由でレベル繰り上がり時 `LevelUp <unit>:` 発火、fixture 20 で検証。LevelUp 命令経由は原典準拠で発火しない |
| 勝利条件イベント.md | `Event.cs` | `App::fire_victory_condition_event` + `App::has_victory_condition_event` | 🟡 | API は整備済、マップコマンド「作戦目的」UI 連動は別途 (`has_*` で表示可否判定可能) |
| 再開イベント.md | `Event.cs` | `App::fire_resume_event` + `App::from_save_json` | 🟡 | from_save_json は副作用無し、フロントエンド側で `fire_resume_event()` を明示的に呼ぶ規約。lib test で round-trip 検証済 |
| マップコマンドイベント.md | `Event.cs` | `app.rs::invoke_custom_unit_command` の map 版 (要確認) | 🟡 | CustomCommandDef で収集、マップメニュー側未完 |
| ユニットコマンドイベント.md | `Event.cs` | `app.rs::invoke_custom_unit_command` + `command_menu.rs::Custom` | ✅ | UnitMenuItem::Custom で実行・条件付き表示、テスト有 |

### 4.3 イベントコマンドリファレンス

主たる dispatch: [`event_runtime.rs::exec_command_pc`](../crates/src-core/src/event_runtime.rs)。
対応する SRC.NET 実装は基本的に `SRC.NET/Command.cs` または `Event.cs` 内の同名サブルーチン。
表は menu.md の出現順を維持する (約 170 件)。

| コマンド (md) | SRC.NET (`Command.cs` ほか) | Rust 実装 (`event_runtime.rs` の dispatch arm 行 / 別ファイル) | 評価 | コメント |
|---|---|---|---|---|
| Arcコマンド | `Command.cs::Cmd_Arc` | (未実装) | 🔴 | dispatch 無し (描画系) |
| Arrayコマンド | `Command.cs::Cmd_Array` | `"array"` arm | ✅ | `Array var string sep` で配列分解、`リスト` 区切り対応 |
| Askコマンド | `Command.cs::Cmd_Ask` | `menu/ask` 共有 arm (L2681) | ✅ | Format1/2 両対応 |
| Attackコマンド | `Command.cs::Cmd_Attack` | `"attack"` arm + `apply_damage_no_event` | 🟡 | `Attack unit1 weapon1 unit2 weapon2` 4 引数フォーマット、`自動`/武器名選択、`防御`/`回避`/`無抵抗`/武器名 反撃、SRC 仕様準拠で 攻撃/攻撃後/損傷率/破壊 ラベルは発火させない。命中判定・気力連動は省略 |
| AutoTalkコマンド | `Command.cs::Cmd_AutoTalk` | `"autotalk"` arm + `fire_pair_event_labels` 経由 | 🟡 | 隣接ペアを走査し `会話 <a> <b>` ラベルを発火。引数 0 で全ユニット、1+ で起点指定 |
| BossRankコマンド | `Command.cs::Cmd_BossRank` | `"bossrank"` arm | 🟡 | `__rank_<unit>` script_var に保存。Rank() 関数で参照可能。ランク別装甲補正等の戦闘連動は未実装 |
| Breakコマンド | `Command.cs::Cmd_Break` | `"break"` arm (line 1547) | ✅ | Loop/Next 脱出 |
| Callコマンド | `Command.cs::Cmd_Call` | `"call"` arm (line 885) | ✅ | Args 束縛と Return 対応 |
| Cancelコマンド | `Command.cs::Cmd_Cancel` | `"cancel"` arm + `App::cancel_pending_dialog` | ✅ | 現在の pending dialog を破棄、`選択` = 0 にセット |
| Centerコマンド | `Command.cs::Cmd_Center` | `"center"` arm + `App::set_map_cursor` | ✅ | `Center x y [option]` / `Center unit [option]` 両形式対応、option (`非同期`) は無視 |
| ChangeAreaコマンド | `Command.cs::Cmd_ChangeArea` | `"changearea"` arm + `UnitInstance.current_area` | ✅ | `ChangeArea [unit] area` で current_area 直接設定。Land/Air/Water コマンドの上位 |
| ChangeMapコマンド | `Command.cs::Cmd_ChangeMap` | `"changemap"` arm (line 2203) | 🟡 | 切替のみ、非同期/エフェクト未対応 |
| ChangeModeコマンド | `Command.cs::Cmd_ChangeMode` | `"changemode"` arm + `UnitInstance.ai_mode` + `App::ai_act_unit` 連動 | ✅ | `ChangeMode [unit] mode` で `ai_mode` 更新。AI ターンで `固定` → 完全静止、`待機` → 攻撃のみ。Pass 12 で AI 連動完了 |
| ChangePartyコマンド | `Command.cs::Cmd_ChangeParty` | `"changeparty"` arm (line 1729) | ✅ | 実装済 |
| ChangeTerrainコマンド | `Command.cs::Cmd_ChangeTerrain` | `"changeterrain"` arm | ✅ | `ChangeTerrain X Y name bitmap` で terrain_id を上書き。`(ローカル)` 接尾も剥がして同名 lookup、`DEFAULT_TERRAINS` + シナリオ terrain.txt の両方を検索 |
| ChangeUnitBitmapコマンド | `Command.cs::Cmd_ChangeUnitBitmap` | (未実装) | 🔴 | dispatch 無し |
| Chargeコマンド | `Command.cs::Cmd_Charge` | `"charge"` arm + `UnitInstance.charged` + `combat::best_weapon_in_range_with_charge` | ✅ | charged フラグで Ｃ 属性武器を解禁。`combat::is_charge_weapon` で武器側 class 判定 |
| Circleコマンド | `Command.cs::Cmd_Circle` | (未実装) | 🔴 | dispatch 無し (描画系) |
| ClearEventコマンド | `Command.cs::Cmd_ClearEvent` | `"clearevent"` arm | 🟡 | `ClearEvent <label>` でラベル削除。引数省略 (現在実行中ラベルを消す) は未対応 — 呼出側で `ClearEvent "攻撃 A B"` のように明示する必要あり |
| ~~ClearFlashコマンド~~ | (廃止) | — | ⚪ | 廃止 |
| ClearObjコマンド | `Command.cs::Cmd_ClearObj` | `"clearobj"` arm (line 2290) | ✅ | overlay+hotpoint クリア |
| ClearPictureコマンド | `Command.cs::Cmd_ClearPicture` | `"clearpicture"` arm (line 2284) | ✅ | overlay クリア |
| ClearSkillコマンド | `Command.cs::Cmd_ClearSkill` | `"clearskill"` arm | ✅ | `ClearSkill unit skill` で condition 削除 (SetSkill の逆操作) |
| ClearSpecialPowerコマンド | `Command.cs::Cmd_ClearSpecialPower` | `"clearspecialpower"` arm | 🟡 | `ClearSpecialPower unit [sp]` で condition 削除。sp 省略時は全 condition クリア (SP buff と他 status の区別なし — 簡略化) |
| ClearStatusコマンド | `Command.cs::Cmd_ClearStatus` | `"clearstatus"` arm (UnsetStatus と共有) | ✅ | unit 省略時は selected_unit、status 指定で remove_condition |
| Closeコマンド | `Command.cs::Cmd_Close` | `"close"` arm (line 1660) | ✅ | VFS ハンドルクローズ |
| Clsコマンド | `Command.cs::Cmd_Cls` | `"cls"` arm (line 2480) | ✅ | 実装済 |
| Colorコマンド | `Command.cs::Cmd_Color` | `"color"` arm (line 2460) | ✅ | 実装済 |
| ColorFilterコマンド | `Command.cs::Cmd_ColorFilter` | `"colorfilter"` arm + `DrawCmd::Fade` | 🟡 | 引数色でフェード描画 (Sepia/Monotone と統合) |
| Combineコマンド | `Command.cs::Cmd_Combine` | `"combine"` arm (line 1214) | 🟡 | mode 切替のみ、合体先構成簡略 |
| Confirmコマンド | `Command.cs::Cmd_Confirm` | `"confirm"` arm (line 2667) | ✅ | PendingDialog::Confirm 連携 |
| Continueコマンド | `Command.cs::Cmd_Continue` | `"continue"` arm (line 1551) | ✅ | ループ脱出 + 次シナリオ遷移両対応 |
| Execコマンド | `Command.cs::Cmd_Exec` | (未実装) | 🔴 | dispatch 無し (外部プロセス起動) |
| CopyArrayコマンド | `Command.cs::Cmd_CopyArray` | `"copyarray"` arm | ✅ | `CopyArray src dst` で `src[*]` の全要素を `dst[*]` にコピー (上書き、完全置換ではない) |
| CopyFileコマンド | `Command.cs::Cmd_CopyFile` | (未実装) | 🔴 | dispatch 無し |
| Createコマンド | `Command.cs::Cmd_Create` | `"create"` arm (line 2818) | 🟡 | rank/level/ID/option 無視 |
| CreateFolderコマンド | `Command.cs::Cmd_CreateFolder` | (未実装) | 🔴 | dispatch 無し |
| Debugコマンド | `Command.cs::Cmd_Debug` | (未実装) | 🔴 | dispatch 無し |
| CallIntermissionCommandコマンド | `Command.cs::Cmd_CallIntermissionCommand` | `"callintermissioncommand"` arm (line 2019) | 🟡 | log のみ、実 UI 未連携 |
| Destroyコマンド | `Command.cs::Cmd_Destroy` | `kill/destroy` 共有 arm (L2796) | ✅ | 撃破ラベル発火 |
| Disableコマンド | `Command.cs::Cmd_Disable` | `"disable"` arm | 🟡 | 1 引数 (グローバル) / 2 引数 (ユニット個別) 両形式対応、`__cmd_enabled_<key>` script_var に保存。UI 連動は別途 |
| Doコマンド | `Command.cs::Cmd_Do` | `"do"` arm (line 1505) | ✅ | While/Until 両対応 |
| DrawOptionコマンド | `Command.cs::Cmd_DrawOption` | (未実装) | 🔴 | dispatch 無し |
| DrawWidthコマンド | `Command.cs::Cmd_DrawWidth` | `"drawwidth"` arm (line 2471) | ✅ | 実装済 |
| Enableコマンド | `Command.cs::Cmd_Enable` | `"enable"` arm | 🟡 | Disable と共通実装、フラグ反転 |
| Equipコマンド | `Command.cs::Cmd_Equip` | `item/equip` 共有 arm (L2990) | ✅ | 実装済 |
| Escapeコマンド | `Command.cs::Cmd_Escape` | `"escape"` arm (line 1763) | ✅ | off_map=true 退避 |
| ExchangeItemコマンド | `Command.cs::Cmd_ExchangeItem` | `"exchangeitem"` arm (line 3029) | ✅ | 実装済 |
| Exitコマンド | `Command.cs::Cmd_Exit` | `"exit"` arm (line 871) | ✅ | 実装済 |
| Explodeコマンド | `Command.cs::Cmd_Explode` | `"explode"` arm | 🟡 | `Explode size [X Y]` で size 別の正方形 FillRect を ScriptOverlay に push (爆発演出近似)。X/Y 省略時は画面中央 |
| ExpUpコマンド | `Command.cs::Cmd_ExpUp` | `"expup"` arm (line 2956) | ✅ | total_exp 加算 + 成長 |
| FadeInコマンド | `Command.cs::Cmd_FadeIn` | `fadein/fadeout` 共有 arm (L2633) | 🟡 | 共有 arm 存在、効果簡略 |
| FadeOutコマンド | `Command.cs::Cmd_FadeOut` | `fadein/fadeout` 共有 arm (L2633) | 🟡 | 共有 arm 存在、効果簡略 |
| FillColorコマンド | `Command.cs::Cmd_FillColor` | `"fillcolor"` arm (line 2627) | ✅ | 実装済 |
| FillStyleコマンド | `Command.cs::Cmd_FillStyle` | (未実装) | 🔴 | dispatch 無し |
| Finishコマンド | `Command.cs::Cmd_Finish` | `"finish"` arm (line 1136) | ✅ | 実装済 |
| Fixコマンド | `Command.cs::Cmd_Fix` | `"fix"` arm (line 1675) | ✅ | HP 完全回復 |
| Fontコマンド | `Command.cs::Cmd_Font` | `"font"` arm (line 2297) | ✅ | family/size/style/color 全パース |
| Forコマンド | `Command.cs::Cmd_For` | `"for"` arm (line 913) | ✅ | Step 込み正常実装 |
| ForEachコマンド | `Command.cs::Cmd_ForEach` | `"foreach"` arm (line 1003) | ✅ | 配列要素列挙 |
| Forgetコマンド | `Command.cs::Cmd_Forget` | `"forget"` arm (line 1308) | ✅ | labels から削除 |
| FreeMemoryコマンド | `Command.cs::Cmd_FreeMemory` | (未実装) | 🔴 | dispatch 無し |
| GameClearコマンド | `Command.cs::Cmd_GameClear` | `win/gameclear` 共有 arm (L1121) | ✅ | 勝利ラベル発火 |
| GameOverコマンド | `Command.cs::Cmd_GameOver` | `lose/gameover` 共有 arm (L1130) | ✅ | 敗北ラベル発火 |
| GetOffコマンド | `Command.cs::Cmd_GetOff` | `"getoff"` arm (line 1428) | ✅ | pilot_name クリア |
| Globalコマンド | `Command.cs::Cmd_Global` | `"global"` arm (no-op stub) | 🟡 | 本実装は Local/Global 未分離のため宣言は副作用無し。スコープ分離は別タスク |
| GoToコマンド | `Command.cs::Cmd_GoTo` | `"goto"` arm (line 727) | ✅ | 実装済 |
| Hideコマンド | `Command.cs::Cmd_Hide` | `"hide"` arm | 🟡 | `script_overlay.clear()` で擬似的に「メインウィンドウ消去」状態を作る (Cls 相当)。SRC.NET の真の hidden-window 状態とは差分有り |
| HotPointコマンド | `Command.cs::Cmd_HotPoint` | `"hotpoint"` arm (line 2256) + `"hotpointstring"` | ✅ | name/x/y/w/h/非表示 全対応 |
| Ifコマンド | `Command.cs::Cmd_If` | `"if"` arm (line 731) | ✅ | 単行/ブロック両形式 + ElseIf 対応 |
| Incrコマンド | `Command.cs::Cmd_Incr` | `"incr"` arm (line 1086) | ✅ | 実装済 |
| IncreaseMoraleコマンド | `Command.cs::Cmd_IncreaseMorale` | `"increasemorale"` arm (line 2936) | ✅ | morale 加算 (clamp 付き) |
| Inputコマンド | `Command.cs::Cmd_Input` | `"input"` arm (line 2649) | ✅ | PendingDialog::Input 連携 |
| IntermissionCommandコマンド | `Command.cs::Cmd_IntermissionCommand` | `"intermissioncommand"` arm (line 2002) | ✅ | name/file 登録 + 削除対応 |
| Itemコマンド | `Command.cs::Cmd_Item` | `item/equip` 共有 arm (L2990) | ✅ | 実装済 |
| Joinコマンド | `Command.cs::Cmd_Join` | `"join"` arm (line 1359) | ✅ | pilot_name 上書き |
| KeepBGMコマンド | `Command.cs::Cmd_KeepBGM` | `"keepbgm"` arm (line 2167) | ✅ | AudioRequest::KeepBgm |
| Landコマンド | `Command.cs::Cmd_Land` | (未実装) | 🔴 | dispatch 無し |
| Launchコマンド | `Command.cs::Cmd_Launch` | `"launch"` arm (line 1405) | ✅ | off_map=false 再配置 |
| Leaveコマンド | `Command.cs::Cmd_Leave` | `"leave"` arm (line 1445) | ✅ | off_map=true (Escape 同等) |
| LevelUpコマンド | `Command.cs::Cmd_LevelUp` | `"levelup"` arm (line 3056) | ✅ | 100exp 換算 + 成長 |
| Lineコマンド | `Command.cs::Cmd_Line` | `"line"` arm (line 2552) | ✅ | B/BF box mode と色解決対応 |
| LineReadコマンド | `Command.cs::Cmd_LineRead` | `read/lineread` 共有 arm (L1650) | ✅ | 実装済 |
| Loadコマンド | `Command.cs::Cmd_Load` | `"load"` arm (line 1298) | 🟡 | メッセージ出力のみ、実ロード未対応 |
| Localコマンド | `Command.cs::Cmd_Local` | `"local"` arm (line ≈Set と共有) | ✅ | 空文字代入 |
| MakePilotListコマンド | `Command.cs::Cmd_MakePilotList` | `"makepilotlist"` arm | 🟡 | `mode` (レベル/SP/格闘 等) でソートし `パイロットリスト[1..N]` + `パイロットリスト数` に格納。UI 描画は別レイヤ |
| MakeUnitListコマンド | `Command.cs::Cmd_MakeUnitList` | `"makeunitlist"` arm | 🟡 | `mode` (HP/EN/装甲/移動力/最大攻撃力 等) でソートし `ユニットリスト[1..N]` + `ユニットリスト数` に格納 |
| MapAbilityコマンド | `Command.cs::Cmd_MapAbility` | `"mapability"` arm (line 1881) | 🟡 | MapAttack と同実装、汎用アビ用処理欠 |
| MapAttackコマンド | `Command.cs::Cmd_MapAttack` | `"mapattack"` arm (line 1861) | 🟡 | 中心(X,Y) + weapon のみ、反撃/経験値仕様簡略 |
| Moneyコマンド | `Command.cs::Cmd_Money` | `"money"` arm (line 1146) | ✅ | 絶対/+/- 全対応 |
| Monotoneコマンド | `Command.cs::Cmd_Monotone` | `"monotone"` arm + `DrawCmd::Fade` | 🟡 | グレーフェード (alpha 0.35) で擬似モノトーン |
| Moveコマンド | `Command.cs::Cmd_Move` | `"move"` arm | ✅ | `Move [unit] x y [option]` 全形式、unit 省略時 selected_unit、option (非同期/アニメ表示) は無視で即時テレポート |
| Nightコマンド | `Command.cs::Cmd_Night` | (未実装) | 🔴 | 包括 no-op アーム (L2544) のみ |
| Noonコマンド | `Command.cs::Cmd_Noon` | (未実装) | 🔴 | 包括 no-op アーム (L2544) のみ |
| Openコマンド | `Command.cs::Cmd_Open` | `"open"` arm (line 1617) | ✅ | For/As/モード対応、VFS 連携 |
| Optionコマンド | `Command.cs::Cmd_Option` | (未実装) | 🔴 | hide/changeterrain/option no-op (L2199) |
| Organizeコマンド | `Command.cs::Cmd_Organize` | `"organize"` arm (line 1898) | 🟡 | UI 無、off_map 螺旋配置の最小実装 |
| Ovalコマンド | `Command.cs::Cmd_Oval` | (未実装) | 🔴 | dispatch 無し |
| PaintPictureコマンド | `Command.cs::Cmd_PaintPicture` | `"paintpicture"` arm (line 2395) | ✅ | "-" 中央寄せ含む描画コマンド |
| PaintStringコマンド | `Command.cs::Cmd_PaintString` | (未実装) | 🟡 | paintstring/r/sysstring 同一実装、色/font 引数簡略 |
| PaintSysStringコマンド | `Command.cs::Cmd_PaintSysString` | (未実装) | 🟡 | PaintString と同枝、System 文字列差分なし |
| Pilotコマンド | `Command.cs::Cmd_Pilot` | `"Pilot"` arm (line 3124) | ✅ | データ定義 12 引数フル |
| ~~PlayFlashコマンド~~ | (廃止) | — | ⚪ | 廃止 |
| PlayMIDIコマンド | `Command.cs::Cmd_PlayMIDI` | `"playmidi"` arm + `AudioRequest::PlayMidi` | ✅ | 専用 AudioRequest 変種を src-web 側で MIDI 再生に振り分け |
| PlaySoundコマンド | `Command.cs::Cmd_PlaySound` | `"playsound"` arm (line 2171) | ✅ | AudioRequest::PlaySound 連携 |
| Polygonコマンド | `Command.cs::Cmd_Polygon` | (未実装) | 🔴 | dispatch 無し |
| Printコマンド | `Command.cs::Cmd_Print` | (未実装 — `Talk` で代替?) | 🟡 | ファイルハンドル書込のみ、メッセージは push_message 経由 |
| PSetコマンド | `Command.cs::Cmd_PSet` | `"pset"` arm (line 2618) | ✅ | drawcmd 経由で実装 |
| Questionコマンド | `Command.cs::Cmd_Question` | `"question"` arm | 🟡 | Menu と同等の選択肢ダイアログ。タイマ (制限時間) は未実装で即時応答 |
| Quitコマンド | `Command.cs::Cmd_Quit` | (未実装) | 🔴 | 包括 no-op アーム (L2541) のみ |
| QuickLoadコマンド | `Command.cs::Cmd_QuickLoad` | `"quickload"` arm + `MapAction::QuickLoad` | 🟡 | `__quicksave` script_var の有無を確認しメッセージ表示。実復元は self 置換不可のためフロントエンドが `from_save_json` + `fire_resume_event` を呼ぶ責務 |
| RankUpコマンド | `Command.cs::Cmd_RankUp` | `"rankup"` arm + `Rank()` 関数 | ✅ | `__rank_<unit>` に increment 蓄積、`Rank()` 関数で読出可。`BossRank` とも統合 |
| RecoverENコマンド | `Command.cs::Cmd_RecoverEN` | `"recoveren"` arm (line 2889) | ✅ | %/数値/Full 全対応 |
| RecoverHPコマンド | `Command.cs::Cmd_RecoverHP` | `"recoverhp"` arm (line 2879) | ✅ | %/数値/Full 全対応 |
| RecoverPlanaコマンド | `Command.cs::Cmd_RecoverPlana` | (未実装) | 🟡 | recoversp/recoverplana 共有、Plana 独立性無し |
| RecoverSPコマンド | `Command.cs::Cmd_RecoverSP` | (未実装) | ✅ | %/数値/Full/PilotInstance 連携 |
| Redrawコマンド | `Command.cs::Cmd_Redraw` | (未実装) | 🔴 | 包括 no-op アーム (L2547) のみ |
| Refreshコマンド | `Command.cs::Cmd_Refresh` | `"refresh"` arm (line 2214) | ✅ | 毎フレーム描画前提で実質 no-op |
| Releaseコマンド | `Command.cs::Cmd_Release` | (未実装) | 🔴 | dispatch 無し |
| RemoveFileコマンド | `Command.cs::Cmd_RemoveFile` | (未実装) | 🔴 | 包括 no-op アーム (L2539) のみ |
| RemoveFolderコマンド | `Command.cs::Cmd_RemoveFolder` | (未実装) | 🔴 | 包括 no-op アーム (L2539) のみ |
| RemoveItemコマンド | `Command.cs::Cmd_RemoveItem` | (未実装) | 🟡 | removeitem/unequip 共有、装備外し中心 |
| RemovePilotコマンド | `Command.cs::Cmd_RemovePilot` | `"removepilot"` arm (line 2864) | 🟡 | pilots からのみ削除、unit_instance 連動無 |
| RemoveUnitコマンド | `Command.cs::Cmd_RemoveUnit` | `"removeunit"` arm (line 2854) | ✅ | 名前 LU 全削除 |
| RenameBGMコマンド | `Command.cs::Cmd_RenameBGM` | (未実装) | 🔴 | 包括 no-op アーム (L2538) のみ |
| RenameFileコマンド | `Command.cs::Cmd_RenameFile` | (未実装) | 🔴 | 包括 no-op アーム (L2539) のみ |
| RenameTermコマンド | `Command.cs::Cmd_RenameTerm` | (未実装) | 🔴 | 包括 no-op アーム (L2538) のみ |
| ReplacePilotコマンド | `Command.cs::Cmd_ReplacePilot` | `"replacepilot"` arm (line 1746) | 🟡 | pilot 入れ替え基本のみ |
| Requireコマンド | `Command.cs::Cmd_Require` | `"require"` arm (line 2524) | ✅ | .ini 取り込み + script_var 反映 |
| SaveDataコマンド | `Command.cs::Cmd_SaveData` | `"savedata"` arm (line 1282) | 🟡 | JSON 化 → script_var 記録、永続化はフロント任 |
| RestoreEventコマンド | `Command.cs::Cmd_RestoreEvent` | (未実装) | 🟡 | restore/restoreevent ラベル再登録 |
| Returnコマンド | `Command.cs::Cmd_Return` | `"return"` arm (line 876) | ✅ | サブルーチン復帰 |
| Rideコマンド | `Command.cs::Cmd_Ride` | `"ride"` arm (line 1376) — 既知乖離 (`SRC_SHARP_DIVERGENCE.md` 参照) | 🟡 | 既知乖離 (DIVERGENCE 記載) |
| Selectコマンド | `Command.cs::Cmd_Select` | `"select"` arm | ✅ | `Select prompt var` + 選択肢列 → Menu ダイアログ。結果を var に格納 |
| SelectTargetコマンド | `Command.cs::Cmd_SelectTarget` | `"selecttarget"` arm | ✅ | `相手パイロット` / `相手ユニットＩＤ` システム変数を更新 |
| Sepiaコマンド | `Command.cs::Cmd_Sepia` | `"sepia"` arm + `DrawCmd::Fade` | 🟡 | セピアフェード (alpha 0.35) |
| Setコマンド | `Command.cs::Cmd_Set` | `"set"` arm (line 783) | ✅ | set/local 共通、配列/式評価対応 |
| SetBulletコマンド | `Command.cs::Cmd_SetBullet` | `"setbullet"` arm (line 1464) | 🟡 | UnitData 側に書込 (インスタンス別管理無) |
| SetMessageコマンド | `Command.cs::Cmd_SetMessage` | `"setmessage"` arm | 🟡 | `次戦闘メッセージ` / `次戦闘メッセージ_<type>` script_var に保存。battle 演出側で参照する想定 (現状は battle 描画が無いため文字列保管のみ) |
| SetRelationコマンド | `Command.cs::Cmd_SetRelation` | `"setrelation"` arm + `Relation()` 関数 | 🟡 | `__rel_<a>_<b>` script_var に対称値を保存。`Relation(a, b)` 関数で読出可。値域 (-100..=100) は無検証 |
| SetSkillコマンド | `Command.cs::Cmd_SetSkill` | `"setskill"` arm (line 1339) | ✅ | conditions テーブル流用で skill 付与 |
| SetStatusコマンド | `Command.cs::Cmd_SetStatus` | `"setstatus"` arm (line 1164) | ✅ | Condition 永続付与 |
| SetStockコマンド | `Command.cs::Cmd_SetStock` | (未実装) | 🔴 | 包括 no-op アーム (L2061) のみ |
| Showコマンド | `Command.cs::Cmd_Show` | `"show"` arm (line 2187) | 🟡 | Title/Configuration → MapView のみ |
| ShowUnitStatusコマンド | `Command.cs::Cmd_ShowUnitStatus` | `"showunitstatus"` arm (line 2066) | ✅ | HP/EN/装甲/運動性/移動力/士気表示 |
| Skipコマンド | `Command.cs::Cmd_Skip` | `"skip"` arm (line 1075) | ✅ | カウンタ式分岐 |
| Sortコマンド | `Command.cs::Cmd_Sort` | `"sort"` arm (line 2485) | ✅ | 配列 prefix の昇/降順 |
| SpecialPowerコマンド | `Command.cs::Cmd_SpecialPower` | `"specialpower"` arm (line 1781) | ✅ | PilotInstance SP 消費 + condition 付与 |
| Splitコマンド | `Command.cs::Cmd_Split` | `"split"` arm (line 1244) | ✅ | dispatch 有 |
| StartBGMコマンド | `Command.cs::Cmd_StartBGM` | `"startbgm"` arm (line 2155) | ✅ | AudioRequest::StartBgm |
| StopBGMコマンド | `Command.cs::Cmd_StopBGM` | `"stopbgm"` arm (line 2163) | ✅ | AudioRequest::StopBgm |
| StopSummoningコマンド | `Command.cs::Cmd_StopSummoning` | (未実装) | 🔴 | 包括 no-op アーム (L2062) のみ |
| Sunsetコマンド | `Command.cs::Cmd_Sunset` | (未実装) | 🔴 | 包括 no-op アーム (L2544) のみ |
| Supplyコマンド | `Command.cs::Cmd_Supply` | `"supply"` arm (line 1667) | 🟡 | dispatch 有、副作用最小 |
| Swapコマンド | `Command.cs::Cmd_Swap` | `"swap"` arm | ✅ | `Swap a b` で 2 変数の値交換 (LHS 解決経由で配列要素も可) |
| Switchコマンド | `Command.cs::Cmd_Switch` | `"switch"` arm (line 894) | ✅ | Case/CaseElse/EndSw 構文対応 |
| Talkコマンド | `Command.cs::Cmd_Talk` | `"talk"` arm (line 842) | ✅ | End まで本文集約 + Talk 対話 |
| Telopコマンド | `Command.cs::Cmd_Telop` | `"telop"` arm | 🟡 | 接頭辞 `【テロップ】` + `.` を改行に変換した文字列を push_message。Subtitle.mid 自動演奏や 1 秒タイマは未実装 |
| Transformコマンド | `Command.cs::Cmd_Transform` | `"transform"` arm (line 1197) | ✅ | dispatch 有 |
| Unitコマンド | `Command.cs::Cmd_Unit` | `"Unit"` arm (line 3152) | ✅ | データ定義 14 引数 + 短形式 |
| UnSetコマンド | `Command.cs::Cmd_UnSet` | `"unset"` arm (line 1590) | ✅ | LHS 解決して script_var 削除 |
| Upgradeコマンド | `Command.cs::Cmd_Upgrade` | `"upgrade"` arm (line 1695) | 🟡 | hp/en/armor/mob/speed のみ、attribute 5 種限定 |
| UpVarコマンド | `Command.cs::Cmd_UpVar` | (未実装) | 🔴 | 包括 no-op アーム (L2541) のみ |
| UseAbilityコマンド | `Command.cs::Cmd_UseAbility` | `"useability"` arm | 🟡 | アビリティ名を `直前使用アビリティ` script_var に保存。効果本体 (AbilityEffect dispatch) は未連動 |
| Waitコマンド | `Command.cs::Cmd_Wait` | `"wait"` arm (line 2114) | ✅ | 秒/Click/Press/Key + Hotpoint 連携 |
| Waterコマンド | `Command.cs::Cmd_Water` | (未実装) | 🔴 | 包括 no-op アーム (L2547) のみ |
| WhiteInコマンド | `Command.cs::Cmd_WhiteIn` | `"whitein"` arm + `DrawCmd::Fade` | 🟡 | 白フェード (引数 n/255 を alpha に換算) |
| WhiteOutコマンド | `Command.cs::Cmd_WhiteOut` | `"whiteout"` arm + `DrawCmd::Fade` | 🟡 | 白フェード (WhiteIn と共通実装) |

### 4.4 関数リファレンス

dispatch: [`event_runtime.rs::eval_function_call`](../crates/src-core/src/event_runtime.rs)
近辺の `match name`、および [`expression/functions/`](../crates/src-core/src/expression/functions/) の
math/string モジュール。

| カテゴリ (md) | SRC.NET | Rust 関連 | 評価 | コメント |
|---|---|---|---|---|
| ユニット情報関数.md | `Expression.cs` (Unit info) | `event_runtime.rs` の `HP`/`MaxHP`/`EN`/`MaxEN`/`Armor`/`Mobility`/`Speed`/`X`/`Y`/`Area`/`Action`/`Damage`/`Condition`/`Status`/`Bullet`/`MaxBullet`/`CountItem`/`WX`/`WY` ほか | 🟡 | 18 関数実装。`Status` は出撃/待機/破棄の 3 種 (格納/離脱/破壊は未モデル化)。`WX`/`WY` は 1 タイル=32px の固定換算。Partner/CountPartner/IsEquiped/SpecialPower 等は未実装 |
| パイロット情報関数.md | `Expression.cs` (Pilot info) | `event_runtime.rs` の `Pilot`/`PilotID`/`Morale`/`Exp`/`Level`/`SP`/`Plana`/`Relation`/`Nickname`/`Skill` | ✅ | Level/Morale/Plana/Relation/Skill/SP すべて実装 |
| Info関数.md | `Expression.cs::Info` | `event_runtime.rs::eval_info` (line 5435 周辺) | 🟡 | 9 種データ区分対応、能力修正/専用機/技能網羅は要追検証 |
| 文字列処理関数.md | `Expression.cs` (String) | `expression/functions/string.rs` + `event_runtime.rs` の `Left`/`Right`/`Mid`/`LCase`/`UCase`/`Trim`/`InStr`/`InStrRev`/`Replace`/`String`/`Wide`/`Asc`/`Chr`/`Format`/`Len` | ✅ | LSet/RSet も実装。Format は #,##0 等パターン非対応 |
| リスト処理関数.md | `Expression.cs` (List) | `event_runtime.rs` の `List`/`Llength`/`Lindex`/`Lsearch`/`Lsplit`/`Lremove` | ✅ | 6 関数すべて実装 |
| 算術処理関数.md | `Expression.cs` (Math) | `expression/functions/math.rs` + `event_runtime.rs` の `Min`/`Max`/`Abs`/`Int`/`Eval`/`Round`/`RoundUp`/`RoundDown`/`Sqr`/`Sin`/`Cos`/`Tan`/`Atn`/`Random` + `Sgn`/`Mod`/`Hex`/`Oct`/`Atan2` | ✅ | Sgn/Mod/Hex/Oct/Atan2 を追加実装。Mod は整数剰余、Hex は大文字 prefix 無し |
| ファイル処理関数.md | `Expression.cs` (File) | `event_runtime.rs` の `Dir`/`FileExists`/`FolderExists`/`FileLen`/`Loc`/`EOF`/`LOF` | 🟡 | VFS ベースで 7 関数実装、ネイティブ FS アクセスは行わない (`FolderExists` は常に 0) |
| 描画処理関数.md | `Expression.cs` (Graphics) | `event_runtime.rs` の `RGB`/`TextWidth`/`TextHeight`/`PointX`/`PointY`/`BaseX`/`BaseY` + `script_overlay.rs` cursor_x/cursor_y | ✅ | PointX/PointY/BaseX/BaseY は ScriptOverlay の描画カーソルを返す。`Line`/`PSet`/`PaintString` 実行で更新される (SRC.NET `picMain.CurrentX/Y` 同等) |
| 時間データ処理関数.md | `Expression.cs` (Time) | `event_runtime.rs` の `Now`/`Year`/`Month`/`Day`/`Hour`/`Minute`/`Second`/`Weekday`/`DiffTime`/`GetTime` + `time_util.rs` (Hinnant Gregorian) + `src-web/src/lib.rs` で `Date::now()` を毎フレーム反映 | ✅ | 全 10 関数実装 + 実時刻バインド完了。WASM 上で `Now()` が現在時刻を返す |
| 正規表現関数.md | `Expression.cs` (Regex) | `event_runtime.rs` の `RegExp`/`RegExpMatch`/`RegExpReplace` + `regex` crate | 🟡 | RegexBuilder ベースで `大小区別あり`/`大小区別なし` 対応。後方参照等は regex crate 制約 (default-features off + unicode-case/perl 限定) |
| その他の関数.md | `Expression.cs` (misc) | `event_runtime.rs` の `Iif`/`Not`/`IsDefined`/`IsAvailable`/`IsVarDefined`/`IsNumeric`/`Exists`/`Count`/`CountPilot`/`HasItem`/`HasStatus`/`Term`/`KeyState`/`PlayingMidi`/`PlayingSound` | ✅ | PlayingMidi/PlayingSound は "0" 固定 stub |

---

## 5. 最後に

| 項目 | Help | Rust 対応 | 評価 |
|---|---|---|---|
| 更新履歴 (1997〜移植中) | 更新履歴(*).md | — | ⚪ |
| サポート情報 | サポート情報.md | `README.md` | ⚪ |
| スペシャルサンクス | スペシャルサンクス.md | — | ⚪ |

---

## 6. 評価セッションログ

各カテゴリは独立した調査エージェントが並列で担当した結果を統合 (2026-05-25)。

### 6.1 操作方法 (2.1〜2.4)

- **対象**: 画面 7・ユニットコマンド 21・マップコマンド 11・インターミッション 9 = 計 48 項目。
- **主要所見**: UI フレーム (scene/, command_menu) と script ドメイン (event_runtime) で実装が二極化。
  メニュー操作と script コマンドを繋ぐ「メニュー項目」自体が大半欠落 (会話/修理/補給/特殊能力/SP/変形/合体/分離/発進/アイテム等)。
  戦闘・移動・待機・ターン終了は本格実装かつテスト網羅で ✅。
- **残課題**: (1) `UnitAction` enum 拡張と各コマンドの target 選択 UI 結線、
  (2) `QuickSave/QuickLoad` の永続化、(3) `StatusWindow` と詳細画面の設計、
  (4) 換装/SRC終了/リスタート等の基盤実装。

### 6.2 基本データ (3.1)

- **対象**: 21 項目 (パイロット/ユニット/アイテム/SP/地形/マップ/イベント 等のパース)。
- **主要所見**: pilot/unit/item/sp/terrain_file/map/event の 7 ファイルが揃いコメント・SJIS・行継続まで対応、
  `crates/src-web/src/archive.rs` 経由で実シナリオから取り込めている。
  ただしいずれも **「KV または extras 文字列に押し込んで実行系では未利用」** の段階。
- **残課題**: 非戦闘員/メッセージ/戦闘アニメ/特殊効果/エリアス/バトルコンフィグ/行動パターン の 7 種が完全未実装。
  優先度は (a) feature/skill 値の実行系反映 → (b) 必要技能の判定組み込み →
  (c) AbilityEffect 種別 dispatch → (d) MessageData/DialogData の連動 → (e) 行動パターン (AI)。

### 6.3 特殊能力/武器属性リファレンス (3.2〜3.4)

- **対象**: パイロット 10 / ユニット 14 / 武器属性 20 = 計 44 カテゴリ。
- **主要所見**: 3.2 はほぼ全カテゴリが 🔴。`feature.rs` は `(name, value)` 保持コンテナで、
  `combat.rs` は基本ステータスと精神コマンドしか参照しない。
  3.3 も多くが 🔴/🟡 で、ユニット側 feature 文字列を自動発動する経路は実装されていない。
  3.4 はマップ攻撃形状解析だけ実装され、それ以外はほぼ全滅。
- **残課題**: (a) `feature.rs` を効果 enum + 実体クエリへ昇格、
  (b) `WeaponData.class` を `Vec<WeaponAttribute>` に構造化、
  (c) 戦闘ループの分岐点に特殊能力フックを挿入。

### 6.4 シナリオ基本概念 + イベントラベル (4.1〜4.2)

- **対象**: 基本概念 10 + ラベル 25 = 35 項目。
- **主要所見** (2026-05-25 セッションで大幅更新): 発火点を持つラベルが
  6/25 → **19/25** に。プロローグ・スタート・ターン・破壊・全滅・ユニットコマンド
  に加え、**損傷率 / 攻撃 / 攻撃後 / LevelUp / 変形 / 合体 / 分離 / 行動終了 /
  進入 / 接触 / 脱出 / 使用 / 使用後** を実装。`script_vars` は依然単一 BTreeMap
  で Local/Global/Sub-local 未分離。
- **本セッションで追加した発火点 (`event_runtime.rs`)** ─ 2 pass:
  - **Pass 1 (戦闘・成長系)**:
    - `fire_damage_threshold_labels` ─ `apply_damage` 内で `損傷率 <name> <pct>` 閾値跨ぎ発火 (fixture 19)
    - `fire_unit_event_labels` ─ pilot/unit/party 識別子 3 種 × 日英 prefix 抽象。変形/合体/分離/LevelUp/行動終了 で共用
    - `fire_attack_event_labels` / `fire_after_attack_event_labels` ─ `app.rs::attack_target` 経路で発火 (UnitEventId 構造体経由、3x3 識別子マッチ)
    - `fire_action_end_labels` ─ `app.rs` UI 経路 (Wait/Attack) で発火
    - ExpUp arm の改修: `total_exp / 100` 変化で LevelUp 発火 (PilotInstance リンク不要)
  - **Pass 2 (移動・使用系)**:
    - `fire_entry_event_labels` ─ `app.rs::try_move_unit_to` 完了時に `進入 <unit> <x> <y>` (0-based) + `進入 <unit> <terrain>` + マップ端到達なら `脱出 <unit> <N/S/W/E>` 連鎖発火
    - `fire_contact_event_labels` ─ 行動終了直後、4 近傍ユニットと `接触 <unit1> <unit2>` 発火 (`fire_pair_event_labels` を流用)
    - `fire_use_event_labels` / `fire_after_use_event_labels` ─ `攻撃イベント` / `攻撃後イベント` 直前に `使用 <unit> <weapon>` / `使用後 <unit> <weapon>` を発火 (attacker 生存時のみ)
    - `AUTOFIRE_KEYWORDS` に `脱出` / `Escape` を追加 (multi-token label 認識のため)
- **残課題**: (1) `会話` (Conversation) 自動発火 (隣接判定ベース、味方-敵の出会い)、
  (2) `収納` イベント (母艦収納時)、
  (3) `特殊効果` / `再開` / `マップ攻撃破壊` ラベル、
  (4) Local/Global/Sub-local 変数スコープ分離、
  (5) `攻撃` / `攻撃後` / `損傷率` で `Attack` / `MapAttack` 命令経由を発火しない条件分岐、
  (6) `Move` 命令経由のスクリプト移動で `進入` を発火しない条件分岐 (現状: UI 経路のみ呼ぶので自然に満たす)。

### 6.4.1 Pass 5 イベントコマンド拡張 (2026-05-25)

- **追加**: `Move` / `ClearStatus` / `Question` / `Select` / `Array` / `Swap` / `Global` (Global は意図的に no-op)
- **副次的修正**:
  - `resolve_expr_atoms` で trailing `,` を吸収 → `Line 50, 40, ...` 等 SRC 標準のカンマ区切り構文を全 draw 系コマンドで受け入れ可能に
  - `pset` arm を `eval_int_expr_app` 経由に変更し、変数式 (例: `PSet X(unit) + 1, Y(unit)`) 対応強化
- **回帰防止**: `Global` の挙動は意図的に no-op stub のまま (定義済み判定を変えるとシナリオの IsVarDefined ベース分岐に副作用)

### 6.4.2 Pass 6 (2026-05-25)

- **イベントラベル発火**: `再開:` (`App::fire_resume_event` 経由)
- **イベントコマンド**: `Hide` / `Center` / `ChangeTerrain` (no-op stub → 実装)
- **関数**: `Format()` の `#,##0` パターン (既に動作していたものをテスト追加で確定)
- **App API 追加**:
  - `fire_resume_event` ─ from_save_json 後のロード再開ラベル
  - `set_map_cursor(x, y)` ─ Center コマンドおよび UI からの中心点設定
- **発火点 (4.2)**: 19/25 → 20/25 (再開を 🔴→🟡)

### 6.4.12 Pass 16 (2026-05-25)

- **PaintPicture オプション完全化 (+4)**:
  - `右回転 N` / `左回転 N` ─ `DrawCmd::Picture.rotation_deg` 設定 (左回転 → 負値)
  - `背景` ─ `as_background` フラグ (フロントエンドが render persistence
    を別レイヤ化する想定)
  - `保持` ─ `persist` フラグ (`ClearPicture` で消えない想定)
  - render.rs `draw_overlay_picture` を回転+反転対応に書き直し
    (画像中央を原点とした transform で `rotate` → `scale` → `draw`)
- **収納イベント発火 (4.2 +1)**:
  - `App::fire_boarding_event(unit_idx, carrier_idx)` API 追加
  - 格納時に `life_state="格納"` + `off_map=true` セット
  - `相手パイロット` / `相手ユニットＩＤ` システム変数に carrier 情報を反映
  - スクリプトから明示できる `Stow unit carrier` 命令を新設 (本実装独自)
  - `event_runtime::fire_unit_event_labels_public` を新規公開
- **発火点 (4.2)**: 21/25 → 22/25 (収納を 🔴→🟡)

### 6.4.11 Pass 15 (2026-05-25)

- **イベントコマンド (+2)**:
  - `Restart` ─ ステージ開始時 (`begin_battle` で自動 snapshot) からやり直し。
    実復元はフロントエンドが `__restart_save` JSON から `from_save_json` +
    `fire_resume_event` を呼ぶ責務。QuickSave 後の Restart で QuickLoad 無効化
    (`__quicksave` クリア)
  - `ChangeArea [unit] area` ─ `UnitInstance.current_area` を直接設定する
    上位 API (Land/Air/Water 個別コマンドの代替)
- **PaintPicture 半分系オプション (+8)**:
  - `DrawCmd::Picture.half_mode: String` 追加
  - `上半分` / `下半分` / `左半分` / `右半分` ─ 反対側 1/2 を背景色で塗りつぶし
  - `右上` / `左上` / `右下` / `左下` ─ 対角線で三角形塗りつぶし
  - src-web `apply_half_mask` ─ Canvas2D の fillRect / Polygon で実装
- **test_harness の `vars` 表示で内部 `__` 変数を除外**:
  - `__quicksave` / `__restart_save` / `__save_slot_*` 等の serialize 済 JSON が
    snapshot ノイズになるのを抑止
  - 既存 fixture (09 save_load_roundtrip 等) はそのまま動作

### 6.4.10 Pass 14 (2026-05-25)

- **Charge × attack_target 連動**:
  - `app.rs::attack_target` を `best_weapon_in_range_with_charge` 経由に変更、
    `UnitInstance.charged` フラグで Ｃ 武器候補を切替
  - Ｃ 武器使用時に `charged = false` で消費 (次回攻撃前に Charge 命令再実行が必要)
- **PaintPicture オプション拡張 (+3)**:
  - `DrawCmd::Picture` に `flip_y` / `monochrome` / `sepia` フィールド追加
  - `上下反転` / `白黒` / `セピア` 引数を arm でパース
  - src-web 側で `draw_overlay_picture` が flip_x + flip_y を結合、`build_css_filter`
    が `grayscale(1)` / `sepia(1)` を Canvas2D filter プロパティで適用
- **UseAbility 追加効果 (+3)**:
  - `憑依` ─ pilot を別 unit へ転送 (元 unit は無人化)
  - `精神感応` ─ SP を半分 target に転送
  - `気力増加` / `気力上昇` ─ 対象の morale を +10

### 6.4.9 Pass 13 (2026-05-25)

- **condition × combat 連動 (`combat.rs::predict_with_status`)**:
  - `バリア` → ダメージ 1/2 軽減
  - `分身` → 命中率 -40
  - `ステルス` → 命中率 -30
- **Status 状態モデル化**:
  - `UnitInstance.life_state: String` (空=自動判定 / `格納` / `離脱` / `破壊`)
  - `Leave` コマンドで `life_state = "離脱"` セット
  - `Status()` 関数が `life_state` を優先返却 (5 状態 + 破棄 の 6 種に対応)
- **Charge 武器属性 (`Ｃ`) 連動**:
  - `combat::is_charge_weapon` ─ WeaponData.class の `Ｃ` / `C` 判定
  - `combat::best_weapon_in_range_with_charge(unit, dist, charged)` 新設、
    `charged=false` なら Ｃ 武器を候補から除外
  - 既存 `best_weapon_in_range` も Ｃ 武器を常に除外する仕様に変更
    (チャージ前は呼び出されない前提)

### 6.4.8 Pass 12 (2026-05-25)

- **UseAbility 効果展開 (+5)**:
  - `バリア`/`バリア展開`/`シールド` → `バリア` condition 付与
  - `分身`/`ホログラム` → `分身` condition 付与
  - `集中`/`精神統一` → `集中` condition 付与
  - `ステルス`/`隠れ身` → `ステルス` condition 付与
  - `合体技`/`援護攻撃` → `直前合体技ユニット` / `直前合体技パートナー[N]` /
    `直前合体技パートナー数` script_var を更新 → `Partner()` / `CountPartner()`
    関数で参照可能に
- **AI 連動 (`ChangeMode`)**:
  - `固定` モード → AI ターンで完全静止 (移動・攻撃共に no-op)
  - `待機` モード → 移動せず現在位置から攻撃のみ試行
  - `通常` (空文字含む) → 既存の Dijkstra + 攻撃ロジック

### 6.4.7 Pass 11 (2026-05-25)

- **イベントコマンド (+5)**:
  - `QuickSave` / `QuickLoad` ─ `__quicksave` script_var に JSON 保存、復元はフロント
  - `Disable` / `Enable` 改善 ─ 1/2 引数フォーム両対応で `__cmd_enabled_<key>` 階層
  - `SaveScreen` / `LoadScreen` ─ ScriptOverlay 全体を JSON で round-trip
  - `PlayFlash` / `StopFlash` / `ClearFlash` ─ `__playing_flash` フラグ
- **イベントラベル発火 (+1)**:
  - `勝利条件:` (`App::fire_victory_condition_event` API)
- **UseAbility 効果 dispatch (+4 abilities)**:
  - `修理装置`/`修理` → HP 全回復、`補給装置`/`補給` → EN 全回復、`状態異常回復` → 全 condition クリア、`自爆` → 発動 unit 撃破 (Destruction 発火)
- **App API 追加**:
  - `fire_victory_condition_event` / `has_victory_condition_event` ─ 「作戦目的」メニュー連動の準備

### 6.4.6 Pass 10 (2026-05-25)

- **イベントコマンド (+5)**:
  - `SelectTarget` ─ `相手パイロット`/`相手ユニットＩＤ` システム変数を設定
  - `Explode` ─ size 別 FillRect を ScriptOverlay に push (爆発演出)
  - `SetRelation` ─ `__rel_<a>_<b>` script_var に対称値保存
  - `Sepia` / `Monotone` / `ColorFilter` ─ `DrawCmd::Fade` で擬似カラーフィルタ
  - `WhiteIn` / `WhiteOut` ─ n/255 → alpha 換算で白フェード
- **関数強化**:
  - `Relation(a, b)` ─ `__rel_<a>_<b>` 読出 (旧実装の常時 0 から修正)
  - `Rank(unit)` ─ fn_arg_value 経由の引数解決、`__rank_<unit>` 読出 (`RankUp`/`BossRank` 連動完了)

### 6.4.5 Pass 9 (2026-05-25)

- **イベントコマンド (+4)**:
  - `MakePilotList` / `MakeUnitList` ─ 能力値ソート → `パイロットリスト[*]` / `ユニットリスト[*]` 配列に格納
  - `PlayMIDI` ─ `AudioRequest::PlayMidi` 専用変種を新設、src-web 側でも対応
  - `Attack` ─ 4 引数戦闘実行、`apply_damage_no_event` ヘルパで Destruction 抑止
- **関数 (+3)**:
  - `RegExp` / `RegExpMatch` / `RegExpReplace` ─ `regex` クレート依存追加 (default-features off + std/perf/unicode-case/unicode-perl)
- **データモデル / 共通基盤**:
  - `AudioRequest::PlayMidi { name }` 追加 (BGM とは別チャネル)
  - `apply_damage_no_event(app, key, amount)` ─ Attack 命令等の "イベント上の戦闘" 用 (損傷率/破壊 ラベルを発火させない)
- **依存追加**:
  - `regex = "1"` (default-features off で WASM サイズ抑制、unicode-case + unicode-perl のみ有効)

### 6.4.4 Pass 8 (2026-05-25)

- **イベントコマンド (+11)**:
  - 移動領域系: `Land` / `Air` / `Water` / `Sea` / `Cosmos` / `Diving` + 日本語 alias (地上/空中/水中/水上/宇宙/地中)
  - 攻撃補助: `Charge` (charged フラグ)、`UseAbility` (アビリティ名保存)、`AutoTalk` (自動 会話 発火)
  - 表示・制御: `Telop` (`.` 改行+接頭辞、push_message 連動)、`Suspend` (Title 復帰)、`BossRank` (rank script_var)
- **関数 (+5)**:
  - `Partner(num)` / `CountPartner()` ─ 合体技履歴 (`直前合体技パートナー[*]` 規約)
  - `IsEquiped`/`IsEquipped` ─ 装備品確認 (HasItem 同等の正典名)
  - `SpecialPower(unit, sp)` ─ SP buff 影響下チェック (`has_condition` 経由)
  - `Area()` の current_area 優先化 (地上/空中 コマンドと連動)
- **データモデル拡張**:
  - `UnitInstance.current_area: String` (領域上書き)、`UnitInstance.charged: bool`
- **fixture 更新**: `14_mini_campaign.expected` — Telop 接頭辞を反映

### 6.4.3 Pass 7 (2026-05-25)

- **イベントコマンド (+7)**:
  - `ClearEvent` ─ ラベル削除 (引数指定形式のみ、省略形は未対応)
  - `ClearSkill` ─ SetSkill の逆操作
  - `ClearSpecialPower` ─ SP buff 解除 (sp 省略時は全 condition クリア)
  - `CopyArray` ─ 配列要素コピー
  - `ChangeMode` ─ `ai_mode` フィールド更新 (陣営一括対応)
  - `SetMessage` ─ `次戦闘メッセージ_<type>` script_var への保存
  - `Cancel` ─ pending_dialog のキャンセル
- **データモデル拡張**:
  - `UnitInstance.ai_mode: String` 追加 (思考モード文字列)
  - `App::cancel_pending_dialog()` 追加 (応答せず破棄)
- **テスト追加**: 7 件 ([expression_functions.rs](crates/src-core/tests/expression_functions.rs))

### 6.5 イベントコマンドリファレンス (4.3)

- **対象**: 約 170 コマンド (A-L / M-W の 2 班並列)。
- **主要所見**: スクリプト基本要素 (If/Goto/Call/Return/For/ForEach/Do/Loop/Break/Continue/Switch/Set/Local/Incr 等) は
  テスト付きで実装され、シナリオ骨格に支障少。
  描画系で `Arc`/`Circle`/`Oval`/`Polygon`/`FillStyle`/`DrawOption`/`ColorFilter` 等の基本図形と
  画面エフェクトの多くが未実装。
  多数の no-op キャッチオール arm (L2535-2548, L2061-2065) が `playmidi`/`renamebgm`/`whitein`/`whiteout`/
  `makepilotlist`/`makeunitlist`/`swap`/`upvar`/`quit`/`useability`/`stopsummoning` 等を吸収しており、
  **エラーは出さないがゲーム的副作用が一切ない** 状態。
- **残課題**: 描画 API 追加 dispatch、`Array`/`CopyArray`/`Swap`/`Global`/`UpVar` などデータ操作系、
  `Select`/`SelectTarget`/`Question`/`QuickLoad`/`Option` などフロントエンド連携。

### 6.6 関数リファレンス (4.4)

- **対象**: 11 カテゴリ。
- **主要所見**: 文字列/リスト/算術/その他/パイロット情報関数は概ね ✅。
  ユニット情報関数は半分、Info は 9 種類対応で 🟡。
  正規表現 関数群は丸ごと欠落、ファイル/時間 は本セッションで追加。
- **本セッションで追加した関数 (2026-05-25 Pass 3+4+5)**:
  - **算術 (Pass 3)**: `Sgn`/`Mod`/`Hex`/`Oct`/`Atan2` (event_runtime.rs dispatcher)
  - **時間 (Pass 3)**: `Now`/`Year`/`Month`/`Day`/`Hour`/`Minute`/`Second`/`Weekday`/`DiffTime`/`GetTime`
    + `time_util.rs` (Howard Hinnant Gregorian アルゴリズム移植、外部依存なし)
  - **ファイル (Pass 3)**: `FileExists`/`FolderExists`/`FileLen`/`Loc`/`EOF`/`LOF` (App VFS ベース)
  - **ユニット情報 (Pass 4)**: `Action`/`Damage`/`Condition`/`Status`/`Bullet`/`MaxBullet`
  - **描画 (Pass 4)**: `PointX`/`PointY`/`BaseX`/`BaseY` を `ScriptOverlay.cursor_x/y` に
    バックエンドして実値返却。`Line`/`PSet`/`PaintString` 実行で更新される
  - **時間配線 (Pass 4)**: src-web で `App::set_wall_clock_ms(Date::now())` を
    毎フレーム呼出 → `Now()` が WASM 実行時の実時刻を返す
  - **副次的修正 (Pass 3+4)**:
    - `Print #<handle>,` の tokenizer 出力 `#1,` の trailing `,` 剥がし
    - `Line`/`PSet` 引数 `50, 40` の trailing `,` を `resolve_expr_atoms` で吸収
- **Pass 5 追加 (2026-05-25)**:
  - **ユニット情報**: `CountItem`/`WX`/`WY` (タイル32px換算)
  - **イベントコマンド**: `Move`/`ClearStatus`/`Question`/`Select`/`Array`/`Swap`/`Global` (Global は no-op stub)
- **残課題**: (1) `RegExpMatch()` 等正規表現対応、
  (2) `Format()` の `#,##0` パターン対応、
  (3) `Status` の 格納/離脱/破壊 状態モデル化、
  (4) `CountPartner`/`Partner`/`IsEquiped`/`SpecialPower` 関数、
  (5) Local/Global スコープ分離 (Global は現状 no-op)、
  (6) Question コマンドのタイマ実装。

---

最終更新: 2026-05-25 (4.2 イベントラベル発火点 6/25 → 22/25,
4.3 イベントコマンド +28,
4.4 関数 +35 (算術 5 + 時間 10 + ファイル 7 + ユニット情報 14 + 描画 4) + src-web Date::now() 配線:
Pass 1 で 損傷率/攻撃/攻撃後/LevelUp/変形/合体/分離/行動終了,
Pass 2 で 進入/接触/脱出/使用/使用後,
Pass 3 で Sgn/Mod/Hex/Oct/Atan2 + Now/Year/Month/Day/Hour/Minute/Second/Weekday/DiffTime/GetTime + FileExists/FolderExists/FileLen/Loc/EOF/LOF,
Pass 4 で Action/Damage/Condition/Status/Bullet/MaxBullet + PointX/PointY/BaseX/BaseY 実値化 + src-web Date::now() 配線,
Pass 5 で Move/ClearStatus/Question/Select/Array/Swap/Global コマンド + CountItem/WX/WY 関数,
Pass 6 で 再開 ラベル + Hide/Center/ChangeTerrain コマンド + Format #,##0 確認,
Pass 7 で ClearEvent/ClearSkill/ClearSpecialPower/CopyArray/ChangeMode/SetMessage/Cancel コマンド,
Pass 8 で 地上/空中/水中/水上/宇宙/地中 + Charge/UseAbility/AutoTalk/Telop/Suspend/BossRank コマンド + Partner/CountPartner/IsEquiped/SpecialPower 関数,
Pass 9 で MakePilotList/MakeUnitList/PlayMIDI/Attack コマンド + RegExp/RegExpMatch/RegExpReplace 関数 (regex crate 依存追加),
Pass 10 で SelectTarget/Explode/SetRelation/Sepia/Monotone/ColorFilter/WhiteIn/WhiteOut コマンド + Relation/Rank 関数強化,
Pass 11 で QuickSave/QuickLoad/Disable/Enable/SaveScreen/LoadScreen/PlayFlash/StopFlash コマンド + 勝利条件 ラベル発火 + UseAbility 4 効果 dispatch,
Pass 12 で UseAbility 5 効果追加 (バリア/分身/集中/ステルス/合体技) + ChangeMode AI 連動 (固定/待機),
Pass 13 で バリア/分身/ステルス × combat 連動 + Status 状態モデル化 (life_state) + Charge 武器属性 (Ｃ) 連動,
Pass 14 で Charge × attack_target 注入 + PaintPicture 拡張 (flip_y/白黒/セピア) + UseAbility 憑依/精神感応/気力増加,
Pass 15 で Restart/ChangeArea コマンド + PaintPicture 半分/対角線塗り (8 種) + test_harness 内部変数除外,
Pass 16 で PaintPicture 右/左回転/背景/保持 (+4) + 収納 ラベル発火 + Stow コマンド)
