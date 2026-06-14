# 実アーカイブ網羅スキャン レポート

対象: `archive/SRCシナリオ_10K～99K/` 配下の全アーカイブ (626 件)
手法: `tools/verify-archive`（`verify-archive` / `extract_text` / `scan_eve`）でネイティブに
展開 → 各データ/イベントファイルを `src-core` のパーサ・ランタイムに通し、
パースエラー・式展開・未実装命令・想定外用法を集計。
日付: 2026-06-01

---

## 0. サマリ

| 指標 | 値 |
|------|----|
| 走査アーカイブ総数 | 626 |
| シナリオ実体を含むアーカイブ | 177 |
| 抽出テキストファイル | 2093 |
| 抽出 `.eve` | 635 |
| 総 command statement 数 | 195,838 |
| ユニーク command 名 | 575 |
| `.eve` パースエラー | 1 ファイル（バッククオート未対応） |
| データファイル致命的パースエラー（**ロード全体を中断**） | 6 アーカイブ |
| ランタイム警告（実行時 `⚠`） | 約 30 件 / 7 カテゴリ |

結論として **大半のシナリオは正しくパース・起動できている**。一方で
少数だが影響度の高い欠陥が 5 件確認できた（詳細は §1〜§5）。再現に使った
ツールと手順は §6 に残す。

---

## 1. 【高】データファイルが全角コンマ `，` でパース失敗 → シナリオ全体ロード中断

### 症状
`pilot.txt` / `unit.txt` / `item.txt` で「設定に抜けがあります。」等が発生。

例: `Dat_Nekronorm.lzh / 秘神黙示ネクロノーム/pilot.txt` 5 行目
```
光頼，男性，魔術機, AABB, 190
```
`，`（全角コンマ U+FF0C）と `,`（半角）が混在。`src-core` のデータパーサ
（`crates/src-core/src/data/pilot.rs:144` 等）は **半角 `,` でしか分割しない**ため、
`光頼，男性，魔術機` が 1 フィールド扱いになり「フィールド不足」と判定される。

### 原典の挙動（要対応）
SRC.NET の `GeneralLib.GetLine`（`SRC.Sharp/SRC.NET/GeneralLib.cs:1393-1395`）は
**全データ行で `，` を `, ` に置換してから**パースしている:
```csharp
string args2 = "，";
string args3 = ", ";
ReplaceString(ref line_buf, ref args2, ref args3);
```
つまり SRC 本体は全角コンマを正規化して受理する。**我々の `loader::read_data_lines`
にこの正規化が無い**のが根本原因。

### 影響
- `Dat_Nekronorm.lzh`, `eod050618.zip` 他、全角コンマを含むデータが全滅。
- §5 と複合し、**1 ファイルでも失敗するとシナリオ全体のロードが中断**される
  （`crates/src-web/src/archive.rs:101` 他が `?` で即 return）。

### 対応
`loader::read_data_lines` に `，`→`, ` 置換を追加（GetLine と等価）。✅ 本コミットで実装。

---

## 2. 【高】`Wait Until <time>` / `Wait Start` が引数エラーになる

### 症状
最頻出のランタイム警告。例:
```
Karin.zip 可鈴/include.eve:14  Wait Until 1.6   → 「Waitコマンドの引数の数が違います」
BattleAnimeKeru / rubi_text / LockOnInclude / VCInclude / usagi / BR_Regiment ...
```

`crates/src-core/src/event_runtime.rs:3534` が `xargs.len() > 1` を一律エラーにしている。

### 原典の書式（`Waitコマンド.md`）
```
書式1  Wait time          (1 引数)
書式2  Wait Start         (1 引数, 基準時刻記録)
書式3  Wait Until time    (2 引数, 基準時刻から 0.1×time 秒まで待機)
書式4  Wait Click         (1 引数)
```
`Wait Until <time>` は 2 引数で正当。戦闘アニメ・カラオケ同期で多用される。

### 付随する忠実度バグ
書式1 は仕様上 **0.1×time 秒**待つが、現実装は `time` をそのまま秒として
扱い `min(5.0)` でクランプしている（`event_runtime.rs:3563`）。`Wait 10`＝1秒の
ところを 5 秒上限の別挙動になっている。

### 対応
`Wait Start`（基準時刻記録）/ `Wait Until time`（2 引数）を受理し、`0.1×time`
係数を適用。✅ 本コミットで実装（基準時刻は近似的に pending_timer で表現）。

---

## 3. 【中】`.eve` トークナイザがバッククオート文字列を扱わず誤パース

### 症状
`.eve` パースエラー 1 件:
```
Ori-fan20060513.lzh .../Lib/STModule/ST_Func.eve:110  「クオートが閉じていません。」
```
該当行:
```
Case "、" "。" "」" "』" "－" "！" "？" ")" "）" `“` `”` `"`
```
`` `"` `` は **バッククオートで囲んだダブルクオート文字**。現トークナイザ
（`crates/src-core/src/data/event.rs:166`）は `"` のみをクオート境界として
トグルし、`` ` `` を無視するため、`` `"` `` 内の `"` が未対応の開きクオートとして
数えられて破綻する。

### 原典の挙動
SRC の `ListSplit`（`GeneralLib.cs:704,754`）は **Asc 96（`` ` ``）をシングルクオート
境界**として扱う（`"` を内包する文字列を書くためのエスケープ手段）。

### 対応
event.rs トークナイザと `inside_unclosed_quote` でバッククオートをクオート境界
として扱う。✅ 本コミットで実装。

---

## 4. 【中】`Pilot name level`（インスタンス書式）が未対応

### 症状
```
eria.lzh eria/Test.eve:7   Pilot セシル＝ローレン 30  → 「Pilot 命令は 12 引数必要。」
SRWAL / MoF_FixedSabun / TukabarkInclude / YY_TestBattle / Ori-fan ...
```

### 原典の書式（`Pilotコマンド.md`）
```
Pilot name level [ID]
```
ロード済みデータから `name` を `level` で **味方として作成**するイベント命令。
`Unit name [rank]` と対になる隊列構築イディオム:
```
Unit サイキックバスター 0
Pilot ジェイ 10
Ride ジェイ
```

ところが `event_runtime.rs:5468` は `Pilot` を **12 引数のデータ定義**（`pilot.txt`
相当）として実装しており、本来のイベント命令書式と食い違っていた。

> ⚠ **重要**: コマンドのエラーは `run_loop_inner`（`event_runtime.rs:759`）で `?`
> により伝播し、**そのスクリプトの実行全体を中断**する。つまり `Pilot リオ 10`
> がエラーになると、それ以降のセットアップ処理が丸ごと走らない。単なる警告では
> なく「シナリオ初期化の中断」を引き起こすため重大。

### 対応
`Pilot` を「12 引数以上 = 旧データ定義（後方互換）」「それ未満 = インスタンス書式
`name level [ID]`」で分岐。短形式ではロード済み `PilotData` を解決して
`create_pilot_instance` で runtime インスタンスを作り、`level` を設定する。
後続の `Ride` がカレントユニットに搭乗させる流れ（Unit / Pilot / Ride）に整合。
✅ 本コミットで実装。`eria` / `Ori-fan` / `DUInclude` 他で Pilot 起因の中断が解消。

---

## 5. 【高】データファイル 1 つの失敗でシナリオ全体のロードが中断される

### 症状
`crates/src-web/src/archive.rs`（および smoke harness）の
`pilot::parse(...).map_err(...)?` 系（行 101 / 112 / 116 / 120 / 124）が、
**どれか 1 ファイルでもパース失敗すると `?` で全体を即 Err** にする。
`.map` ファイルだけは警告ログで握り潰している（行 136）。

致命的中断が起きたアーカイブ:

| アーカイブ | ファイル | エラー |
|-----------|---------|--------|
| Dat_Nekronorm.lzh | pilot.txt:5 | 設定に抜けがあります（§1 全角コンマ） |
| eod050618.zip | Pilot.txt:2 / Unit.txt:2 | 設定に抜けがあります |
| TTGL.lzh | robot.txt:743 | 移動性能の項目が不足しています |
| RDCD.zip | unit.txt:10 | 移動性能の項目が不足しています |
| mva06.zip | unit.txt:344 | アイテム数が数値ではありません（行末コンマ） |
| SECTemplate.zip | pilot/robot/unit.txt:3 | 性別/パイロット数（テンプレート placeholder） |

### 問題
CLAUDE.md の方針「実行時エラーは握り潰してフロントエンド堅牢性重視」と矛盾。
SRC.NET も個別データのエラーはメッセージ表示しつつ可能な範囲で続行する。
**1 つの不正データで全シナリオが起動不能**は過剰に脆い。

### 対応方針
データファイルのパースエラーを**致命的にせず**、警告ログに残してそのファイル
（できればそのレコード単位）をスキップし、残りを取り込む。§1 修正で
Dat_Nekronorm 等は解消するが、TTGL/RDCD/mva06 のような個別データ起因の
ものはこの堅牢化で救済する。

> 注: `mva06` の `対人級, ＢＥＴＡ, 1 ,`（行末コンマ）や TTGL の移動性能行ずれは
> パーサ側のトリム/空フィールド許容の追加検討余地あり（後続課題）。

---

## 6. 未登録（＝未実装または catalog 未掲載）の命令

`scan_eve` が拾った catalog 未登録命令 110 種のうち、**会話本文の誤検出**
（`OKか？`、`Yes`、`BETA接近！！` 等、Talk ブロック内テキストが命令の
ように見えるもの）を除いた「実コマンドらしき」ものは以下。いずれも
ランタイムでは「未知の命令は無視」（`event_runtime.rs:37`）され実行は
止まらないが、演出・状態変化が欠落する。

| 命令 | 出現 | 種別 | 影響 |
|------|------|------|------|
| `Display` | 345 | 描画/演出 | 画像・文字の即時表示が出ない |
| `ETalkL` / `ETalkR` / `ETalkEnd` | 100/104/15 | 会話演出 | 立ち絵付き会話の欠落 |
| `SetAbility` / `ClearAbility` | 65/29 | **状態変化** | 特殊能力の付与/解除が効かない |
| `Mind` / `ClearMind` / `MindAnime` | 40/28/8 | **状態変化** | 精神コマンド付与/解除が効かない |
| `PlayEffect` | 112 | 演出 | エフェクト再生なし |
| `AttackDemo` | 37 | 演出 | 戦闘デモ省略 |
| `ShowCharacter` | 34 | 演出 | キャラ表示なし |
| `String` | 20 | 変数宣言 | 文字列変数宣言（`Local`/`Global` 同系） |
| `MapWeapon` / `OnMapItemChanger` / `NonPilotInfo` / `StratBGM` | 各数件 | 各種 | 個別機能欠落 |
| `Ta.` | 433 | 1 シナリオ独自 | `ayanami.lzh` 専用の非標準記法（無視で可） |
| `DU_*` / `VisualMap*` / `ITW_*` | 各数件 | ユーザ定義 lib | インクルードライブラリ独自命令 |

**優先実装推奨**: `SetAbility`/`ClearAbility`/`Mind`/`ClearMind`（ゲーム状態に影響）、
次に `String`（変数宣言）。`Display`/`PlayEffect`/`AttackDemo`/`ShowCharacter`/
`ETalk*` は演出系なので catalog に Stub 登録して警告ノイズを消すだけでも可。

---

## 7. その他のランタイム警告（個別・低頻度）

| 種別 | 例 | 備考 |
|------|----|----|
| `Party 値が不正: "ＮＰＣ"` | DUInclude / Ori-fan | 全角 `ＮＰＣ` 陣営。SRC 4 陣営は 味方/ＮＰＣ/敵/中立。`parse_party` 未対応（§8 で対応） |
| `Goto 先ラベルが見つかりません` | SLG15 / BR_Regiment / maskman / SRPGW 他 | インクルード/別ファイルのラベルを smoke harness が解決できないだけのものが多い（要個別精査・多くは harness 由来の偽陽性） |
| `ForEach 命令は 2 引数必要 (var collection)` | 方向付き演習 / 聖火リレー | `ForEach var In collection` の特定書式 |
| `Join コマンドの引数の数が違います` | sabun | 引数形バリエーション |
| `Item/Equip 命令は N 引数必要 (unit item)` | shop_01 / DUInclude | 引数形バリエーション |
| `実行ステップ数が上限を超えました` | meiro | 無限ループ防止上限。長尺スクリプトの誤検出の可能性 |

---

## 8. 本コミットで着手した改善と効果測定

原典仕様が明確で `cargo test -p src-core` で検証可能なものから着手:

1. **§1** `loader::read_data_lines` に全角コンマ正規化 `，`→`, ` を追加。
2. **§2** `Wait Start` / `Wait Until time` を受理し `0.1×time` 係数を適用。
3. **§3** `.eve` トークナイザでバッククオート `` ` `` をクオート境界として扱う。
4. **§4** `Pilot name level [ID]`（インスタンス書式）を実装。
5. **§7** `parse_party` に `ＮＰＣ`/`NPC` を追加。

177 シナリオアーカイブに対する起動 smoke の効果測定:

| 指標 | 改善前 | 改善後 |
|------|-------:|-------:|
| データ致命的失敗（ロード全体中断）アーカイブ | 6 | **5** |
| ランタイム警告（＝スクリプト中断要因）行数 | 約 30 | **16** |
| `Wait*` 引数エラー | 約 11 | **0** |
| `Pilot 命令` 中断 | 9 | **0** |
| `Party 値が不正 (ＮＰＣ)` | 2 | **0** |
| `.eve` パースエラー | 1 | **0** |

`cargo test -p src-core`: 全 43 スイート緑（新規テスト: 全角コンマ正規化 /
バッククオート / 未終端バッククオート）。

### 残課題（後続）
- **§5** データロード堅牢化（1 ファイル失敗で全体中断しない）。残り 5 件の
  致命的失敗（TTGL/RDCD の移動性能行ずれ、mva06 の行末コンマ、SECTemplate の
  テンプレ placeholder）はこれで救済できる。
- **§6** 未実装命令の実装/Stub 登録（`SetAbility`/`Mind` 系を優先）。
- 残ランタイム警告 16 件の内訳: `Goto ラベル未検出` 9（多くは smoke harness が
  複数 .eve のラベルを単一 script_library に統合する都合の偽陽性。要個別精査）、
  `Item/Equip` 引数 3、`ForEach`/`Join` 引数 3、ステップ上限 1。

---

## 9. 再現手順

```sh
# ビルド
nix ... develop --command cargo build -p verify-archive --bins

# 1 アーカイブの内訳 + 起動 smoke
VERIFY_SMOKE=1 target/debug/verify-archive "archive/SRCシナリオ_10K～99K/1k.lzh"

# テキストを一括抽出してツリー化
target/debug/extract_text /tmp/va_extract archive/SRCシナリオ_10K～99K/*.lzh ...

# catalog 未登録命令スキャン
target/debug/scan_eve /tmp/va_extract
```

`tools/verify-archive/src/bin/extract_text.rs` を本作業で追加（アーカイブ群から
`.eve`/`.txt`/`.map`/`.ini`/`.dat` をデコード展開する解析補助）。
