# SRC.Sharp との互換性・意図的な乖離

本プロジェクトは VB6 オリジナル SRC を一次資料、SRC.Sharp (C# 移植) を二次資料
として参考にしている。両者がセマンティクスで食い違っていたり、SRC.Sharp の
実装に明確な FIXME / 設計バグがあると判断した場合は、Rust 実装は **VB6
オリジナルに寄せて再設計** する。本ドキュメントはそうした「意図的な乖離」を
記録する。

各エントリは次のフォーマットを持つ:

- **対象**: 関数 / コマンド名
- **Rust 実装**: 採用したセマンティクス
- **SRC.Sharp**: 比較対象の挙動
- **VB6**: 原典の挙動 (参考)
- **乖離の理由**: なぜ SRC.Sharp に追従しないか
- **検証**: 関連するテスト

---

## 1. `Asc(s)` / `Chr(n)` — SJIS (CP932) 一貫性を優先

### 対象
[`Asc(s)`](../crates/src-core/src/event_runtime.rs) / [`Chr(n)`](../crates/src-core/src/event_runtime.rs)

### Rust 実装
両方とも **SJIS (CP932) ベース**。

- `Asc("あ")` → 33440 (= 0x82A0、SJIS 2 バイト連結値)
- `Chr(33440)` → "あ"
- `Chr(Asc(s))` が round-trip する

実装は `encoding_rs::SHIFT_JIS` 経由で encode / decode し、SJIS に
マップできない文字 (例: 絵文字) は Unicode codepoint に fallback。

### SRC.Sharp
- `Asc`: `Microsoft.VisualBasic.Strings.Asc()` を呼ぶため SJIS 経由 (Windows 環境で
  CP932 動作時)。`Asc("あ")` → 33440。
- `Chr`: `(char)SRC.Expression.GetValueAsLong(...)` で **Unicode コードポイントとして
  キャスト**。`Chr(33440)` → U+82A0 = '蠀' (漢字、別文字)。
- 結果として **`Chr(Asc("あ"))` が round-trip しない**。SRC.Sharp の `String.cs`
  には `// XXX 文字コード` という FIXME コメントが残っており、この非対称が
  未解決の既知問題と認識されている。

### VB6
SJIS で一貫。`Asc("あ")` = 33440、`Chr(33440)` = "あ" で round-trip する。

### 乖離の理由
- VB6 SRC のシナリオは SJIS 文字列を前提に書かれている (実 .eve ファイルも
  SJIS で保存されており、loader で UTF-8 に変換している)。
- SRC.Sharp の不整合は同プロジェクトでも FIXME 扱い。
- `Chr(Asc(s)) == s` が成り立たないと、文字列を分解・再構築する処理 (例:
  SetWindow タイトル整形、ファイル名生成) が破綻する可能性がある。
- 実シナリオでの非 ASCII Asc/Chr 使用は 0 件と確認済 (定量影響無し) だが、
  将来のシナリオが SJIS 値を直接書いてもよう、VB6 互換を取った方が安全。

### 検証
- [`crates/src-core/tests/string_function.rs`](../crates/src-core/tests/string_function.rs)
  の `asc_japanese_returns_sjis_code` / `chr_sjis_double_byte_decodes_japanese`
  / `asc_chr_round_trip_japanese` / `asc_halfwidth_katakana_returns_single_byte`
  / `chr_halfwidth_katakana_byte_decodes_to_kana`
- ASCII 範囲 (0..=0x7F) は SJIS と Unicode で同値なので、既存テスト
  (`Asc("A") = 65`、`Chr(65) = "A"` 等) はそのまま通る。

### コミット
`79ab0f2` — Asc / Chr を SJIS (CP932) 互換に変更 + round-trip 検証テスト 5 ケース

---

## 2. `Round(x [, n])` — 負の半数は VB6 (+∞方向丸め) に従う

### 対象
[`Round`](../crates/src-core/src/event_runtime.rs)（数値関数）

### Rust 実装
VB6 原典 `Expression.bas:2991-2996` 準拠: `num = Int(x*10^n)`（Int=floor）して
小数部 ≥ 0.5 なら +1。すなわち **+∞方向への半数丸め**。
- `Round(2.5)` = 3 / `Round(-2.5)` = **-2** / `Round(1.5)` = 2

### SRC.Sharp
`Math.Round(x, n, MidpointRounding.AwayFromZero)`（**ゼロから遠ざける半数丸め**）。
- `Round(2.5)` = 3（正は一致） / `Round(-2.5)` = **-3**（VB6 と乖離）

### VB6
`Round(-2.5)` = -2（`Int(-2.5)=-3`、`-2.5-(-3)=0.5≥0.5` → +1 → -2）。

### 乖離の理由
オラクル (SRC.Sharp) が VB6 原典から乖離しているケース。負の半数値の丸めは
VB6 が一次資料のため VB6 に従う。正の値は三者一致。

### 検証
[`crates/src-core/tests/math_function_oracle.rs`](../crates/src-core/tests/math_function_oracle.rs)
の `round_negative_half_goes_toward_positive_infinity`。

### コミット
`a171af3`（Math オラクルテスト追加時に VB6 で裏取り、Rust が正しいと確認）

---

## 3. `Not` 演算子の優先順位 — 既知の未整合 (低影響・未是正)

### 対象
[`parse_factor`](../crates/src-core/src/event_runtime.rs)（式評価器の `Not`）

### Rust 実装
`Not` を最高優先 (`parse_factor`) で束縛する。`Not 1 = 2` → `(Not 1) = 2` → `0 = 2` → 0。

### SRC.Sharp / VB6
`Not` は比較より緩く束縛する。`Not 1 = 2` → `Not (1 = 2)` → `Not 0` → 1。

### 判断
括弧付き (`Not (a = b)`) では三者一致し、実シナリオは括弧付きが通例のため低影響。
characterization test で挙動を明示し据え置き（将来 `Not` を比較と論理の間の優先度
レベルへ移せば整合する）。

### 検証
[`crates/src-core/tests/expression_oracle.rs`](../crates/src-core/tests/expression_oracle.rs)
の `known_divergence_not_precedence_without_parens`。

---

## 4. 是正済みの旧乖離 (2026-06-17 オラクル監査で発見・整合)

C# オラクル × VB6 原典の突合で、本実装が原典から乖離していた箇所を是正した記録。
いずれも C# = VB6 で、Rust が異端だった (上記 1〜3 とは逆向き)。

| 項目 | 旧 Rust | SRC/VB6 原典 | コミット |
|------|---------|--------------|----------|
| 式のゼロ除算 | `5/0 == 5`（分子残し） | `0`（`EvalExpr` の rnum≠0 ガード） | `b994e08` |
| 改造 1 段の HP | +100 | +200（`Unit.cls:1719`） | `0458a0e` |
| 改造 1 段の装甲 | +30 | +100（`Unit.cls:1721`） | `0458a0e` |
| exp→level | 100 exp/level | 500 exp/level（`Pilot.cls:1183`） | `06366f3` |
| pilot.txt 能力値 5/6 番目 | 反応・技量（取り違え） | 技量・反応（`PilotDataList.cls:677-692`） | `803e13d` |
| `Create` の rank 引数 | 無視（常に素のステータス） | 改造段階として反映（`UList.Add`→`Unit.Rank`） | `135b5da` |
| `Create` の level 引数 | 無視（常にレベル 1） | 主パイロット初期レベルへ反映（`PList.Add`） | `ab67269` |
| パイロットのレベル成長式 | class ベース `(level-1)*rate`（過大成長） | `lv=Level`・格闘等 +level・命中/回避 +2*level（`Pilot.cls:582-593`） | （本セッション） |
| `Info(パイロットデータ,…)` の成長 | 配置済みだと成長後を返す | 静的データ（成長前）を返す（`Info.cs` PDList） | （本セッション） |
| 命中率のクランプ | `clamp(5, 95)`（他 SRPG 慣習） | 上限なし・最低 0（`Unit.cls:6694-6696` / C# `Math.Max(0,prob)`。100 超は必中、表示のみ `min(100)`） | （本セッション） |
| 最低ダメージ | `max(1)` | `max(10)`（`Unit.cls:7460-7474` / C# `UnitWeapon.cs:3567`。既定 10。オプション「ダメージ下限解除」=0/「ダメージ下限１」=1） | （本セッション） |
| 地形の命中修正の符号 | combat `(100 + hit_mod)`＋ビルトイン地形が負値（内部独自規約） | **正=防御地形**で `(100 - hit_mod)`（`Unit.cls:6295` / `マップデータ.md`。terrain.txt の命中修正列は正格納） | （本セッション） |
| 防御側パイロットの Defense（耐久 技能） | 未モデル（防御力に Defense 係数なし） | `防御力 = 装甲 × 気力/100 × Defense/100`、`Defense = 100 + 5*耐久Lv`（`Pilot.cls:402` / C# `UnitWeapon.cs` `arm *= Defense/100`） | （本セッション） |

pilot.txt 能力値行の 5 番目=技量(Technique)・6 番目=反応(Intuition) を取り違えていた。
差分オラクル `oracle_loaddata`（後述）で `Info(パイロットデータ, 人工知能, 技量)` が
C#=135 / 旧 Rust=80 と食い違うことで検出。戦闘式 (`combat.rs`) は反応→命中/回避・
技量→クリティカルと正しい意味で参照していたため、パーサ是正で実効値が正される
（combat コードは変更不要）。

exp→level は併せて、実装中に 16 箇所重複していた `total_exp/100` を正典関数
`pilot_instance::level_from_exp` に集約した（1 箇所修正漏れでレベル不整合になる罠の除去）。

命中率クランプは差分オラクルの**戦闘予測モード**（`placeattack` / `combat_prediction.txt`）で検出した。
実 fixture のユニットで `Info` 経由ではなく `HitProbability` を直接突合したところ、C# は 115/155/170 等
（上限なし）を返すのに対し旧 Rust は一律 95 へ丸めていた。VB6 `Unit.cls:6694-6696`（`If prob < 0 Then
HitProbability = 0 Else HitProbability = prob`）は**最低 0 のみ**で上限がなく、C# も `Math.Max(0, prob)`
で一致。クランプ撤去後は命中率・クリティカル率とも **18/18 cross-engine 一致**＝旧クランプが唯一の乖離で、
その下に隠れた式の不整合は無かった（命中=`100+命中+直感+運動性+命中補正 − (回避+直感+運動性)`×地形×サイズ
の全項が原典と一致）。上限 95 は高命中でも 5% 外す・下限 5 は低命中でも 5% 当たる非原典挙動だった。

最低ダメージは同じ戦闘予測オラクルを `ダメージ` field へ拡張して検出（`combat_damage.txt`）。両エンジンを
中立地形の地上に配置し実データの地形適応（S/A/B/C/D）を効かせて突合した結果、装甲＞攻撃力の 1 ケースのみ
C#=10 / 旧 Rust=1 と乖離。`戦闘システム詳細.md` のダメージ式は `(攻撃力−防御力)×地形ダメージ修正`
（攻撃力=武器攻撃力×ﾊﾟｲﾛｯﾄ攻撃力/100×気力/100×地形適応、防御力=装甲×気力/100×地形適応）で**構造は
Rust と同一**＝唯一の差は下限（VB6 `Unit.cls:7460` 既定 10、Rust は 1）。是正後は **14/14 cross-engine 一致**。
（教訓: 当初 C# `Damage` の静的読みでは「攻撃力に地形適応は乗らない」と誤読したが、実数突合で攻撃力にも
武器/ユニット地形適応が乗ると確定＝Rust の `atk_adapt` 適用は原典準拠で正しい。spec 裏取りが決定的。）

地形の命中修正の符号は combat 予測オラクルの terrain 拡張を設計する過程で発見（静的解析＋VB6/help 裏取り）。
SRC は terrain.txt の「命中修正」列を**正=防御地形**で格納し（`マップデータ.md`、森林 10/山 30 等）、combat は
VB6 `Unit.cls:6295` `ed_aradap *= (100 - TerrainEffectForHit)/100`・C# 同様に**引き算**で適用する。
`Info(マップ,x,y,回避修正)` もこの正値を返す。一方旧 Rust は combat を `(100 + hit_mod)` とし、辻褄合わせで
ビルトイン地形カタログに**負値**を格納する独自規約だった。terrain.txt パーサは（正しく）正値を格納するため、
**実シナリオ（terrain.txt をロードするほぼ全シナリオ）で防御地形の被命中が逆転**（森林・山・都市で当たりやすくなる）
していた＝pervasive。加えて敵 AI の防御地形選好 `tile_defensive_value` も `damage_mod - hit_mod` が `30-30=0` になり
山の防御価値を見失っていた。是正＝原典の**正規約**へ統一: combat `(100 - hit_mod)`・ビルトインカタログを正値・AI を
`damage_mod + hit_mod`・`回避修正` Info も正値（パーサは既に正で無改）。ビルトイン地形の combat 挙動は規約反転が透過的で
不変（既存テスト全緑）、変わるのは terrain.txt 由来の防御地形のみ＝是正。回帰テスト `positive_terrain_hit_mod_reduces_hit`。

防御側パイロットの Defense（耐久 技能）は damage オラクル（cut 2a）の representative 突合で C# が `Info(パイロット,防御)=100`
を返すのを見て C# `Damage` の `arm *= Defense/100` を確認し、Rust 側に Defense 係数が**全く無い**ことから発掘。VB6
`Pilot.cls:402` の既定 Defense = `100 + 5*SkillLevel("耐久")`（オプション下の Level 加算項は既定オフ＝未モデル）。
人工知能 は耐久を持たず Defense=100 のため cut 2 の 14/14 は一致していた（基底ケースは健全）が、**耐久 技能持ちの防御側は
被ダメージが過大**だった。`combat.rs` の `def_power` に `Defense/100` 係数を追加（耐久 Lv は features の `耐久Lv<n>` から
ハンター技能と同じ要領で抽出）。in-repo fixture に 耐久 パイロットが無いため synthetic 回帰テスト
`endurance_skill_raises_defense_and_reduces_damage`（魅了/憑依 と同方針）。残: 耐久 の実データ格納形式（名称接尾辞 vs
データ列）の最終確認と、オプション下の Level 成長項は未モデル。

---

## 乖離候補 (まだ着手していない / 暫定の判断保留)

以下は SRC.Sharp と Rust 実装で挙動が異なるが、本ドキュメントに正式エントリと
してまとめる前段階。各項目は今後の調査で「VB6 寄りに揃える」「SRC.Sharp に
合わせる」「現行 Rust 実装を維持する」のいずれかに決定される予定。

### `Ride pilot unit`
- **Rust 実装**: `unit_a` の座標を `unit_b` の座標に移動する (= 「キャリアに搭乗」
  をマップ位置だけで表現)。
- **SRC.Sharp / VB6**: パイロットを別ユニットに乗せ換え (`pilot_name` 差替え +
  party 同期)。空き席探索や副パイロット降車を伴う多段ロジック。
- **判断保留の理由**: 本実装の `Unit` / `Pilot` コマンドは VB6 の「データ定義
  形式」(2 引数 + level) と「全フィールド指定」(12〜14 引数) を別物として
  受理しており、前者で declare → `Ride` で実体化、というフローがまだ
  通っていない。`Ride` だけ直しても破綻するので、後段の `Unit name level`
  / `Pilot name level` の実体化処理を実装してから揃える。

### `Plana(pilot)` / `Relation(p1, p2)` / `SP(pilot)` 等
- **Rust 実装**: 霊力 / 関係値は modeling していないためスタブで 0 を返す。
  `SP` は `PilotData.sp - sp_consumed` で近似。
- **SRC.Sharp / VB6**: 独立した `PilotInstance` を持ち、関係値や霊力を
  per-instance で保持する。
- **判断保留の理由**: `PilotInstance` の独立化は CURRENT_WORK.md §6.1 で
  整理されている大規模リファクタの一部。実シナリオで `Plana` / `Relation`
  の戻り値に依存した条件式が動かないと困るような流れは未確認。

### `.eve Pilot` / `Unit` コマンドの inline 定義形式 — Rust 独自拡張 (test 便宜)

- **Rust 実装**: `Pilot "リオ" リオ 男性 超能力者 AAAA 100 160 ...` のような **12〜14 フィールド
  inline 定義形式**を受理し、パイロット/ユニットをその場で定義する (テストの利便性のための拡張)。
  SRC 正規の参照形式 (3〜4 引数) も受理する superset。
- **SRC.Sharp / VB6**: `Pilot` コマンドは **3〜4 引数**のみ (`PilotCmd.cs:18` `ArgNum != 3 && != 4`)、
  arg2 は **pilot.txt で事前定義済みの名前** (`PDList.IsDefined` 必須)。inline 12 フィールド形式は
  「Pilotコマンドの引数の数が違います」で拒否。`Unit` も同様。
- **判断保留の理由**: Rust の inline 形式は superset (SRC 正規の参照形式も動く) なので実シナリオの
  fidelity には影響しない。ただし**差分オラクルでユニット状態を diff する際の制約**: 両エンジンで
  同一ユニットを作るには inline コマンドではなく **pilot.txt/unit.txt データを両方にロード**する
  必要がある (combat 状態 diff はさらに RNG シード一致が要る)。差分オラクルの「ユニット/combat
  状態」拡張の前提条件。

### `Set var "x" & y` (Set 値の `&` 連結) — Rust が寛容

- **Rust 実装**: `Set msg "HP:" & $(hp)` を受理し連結して代入する。
- **SRC.Sharp / VB6**: 「Setコマンドの引数の数が違います」エラーで拒否する (Set は
  値を 1 トークン/式として取り、空白区切りの `& ` を余分な引数と見なす)。正規の SRC
  形式は補間 `Set msg "HP:$(hp)"`。
- **判断保留の理由**: Rust の寛容化は無害 (正規形式も動く) だが、原典では実行時エラーに
  なる入力を黙って受理する点が乖離。差分オラクル (`tools/oracle-diff` scenario モード) で
  検出。低優先。

### `Info(パイロットデータ, …, 性別)` で性別 `-` を空文字へ正規化 (差分オラクル loaddata で検出)

- **Rust 実装**: `Sex` enum を {Male, Female, Unspecified} の 3 値で持ち、pilot.txt の
  性別フィールド `-` を **Unspecified** に畳む。`Info(…, 性別)` は Unspecified で空文字を返す。
- **SRC.Sharp / VB6**: `.Sex` を生文字列で保持 (`PilotDataList.cls:223/247` で `Case "-"` を
  そのまま格納)。`Info(…, 性別)` は格納値 `"-"` を返す。
- **判断保留の理由**: 性別限定の判定 (男性/女性) では `-`・空とも「いずれでもない」で等価。
  観測差は Info 文字列のみ (`-` vs 空)。`Sex` enum 拡張は save 形式・combat/condition への
  波及があり、表示文字列のためだけの変更は見送り。差分オラクル (`oracle_loaddata`) で検出。低優先。

### `Info(パイロットデータ, …, クラス)` 別名 — Rust が寛容 (機能差なし)

- **Rust 実装**: パイロットの機体クラス照会で `クラス` / `ユニットクラス` 両別名を受理する
  (`info_pilot`)。両者とも `PilotData.Class`（= `汎用` 等、VB6 準拠値）を返す。
- **SRC.Sharp**: `Info.cs` のパイロット分岐は `ユニットクラス` / `機体クラス` のみ受理し、
  `クラス` キーには未対応 (空文字を返す)。
- **判断保留の理由**: Rust が別名を 1 つ多く受ける superset 差で、返す値自体は両エンジン一致。
  差分コーパスは曖昧を避けるため `ユニットクラス` で突合する。無害・低優先。

### `Info(パイロットデータ, …, 特殊能力名称, N)` — C# は別名(RHS)・Rust は key(LHS) を返す

差分オラクル loaddata (`pilot_feature.txt`) で検出。パイロット特殊能力 `名前=別名` 形式の列挙。

- **Rust 実装**: `feature_name` は `(name, value)` の **name (LHS)** を返す。`メッセージ=無口(ザコ)` →
  `メッセージ`、`ザコパイロット=非表示` → `ザコパイロット`。
- **SRC.Sharp / VB6**: パイロットは `pd.SkillName(…)` 経由で **別名 (RHS)** を返す (`Info.cs:1016-1026`)。
  `メッセージ=無口(ザコ)` → `無口(ザコ)`、`ザコパイロット=非表示` → `非表示`。`=` 無しの `成長タイプB` は両者一致。
- **判断保留の理由**: `特殊能力所有(名前)` (所有判定) は両エンジンとも **key(LHS)** で一致するため機能差は無く、
  乖離は `特殊能力名称` 列挙 (表示用) のみ。是正は pilot 用 `特殊能力名称` が value(別名・末尾 `,N` 除去) を
  返すよう分岐する必要があるが、`feature_name` は unit/item と共有のため波及に注意 (pilot 限定の分岐が要る)。
  表示用途・低優先。`特殊能力所有`/`特殊能力レベル`/`最大ＳＰ`/`特殊能力数` は cross-engine 一致を確認済。

### ✅ 武器フィールドのパース — cross-engine 一致を確認 (乖離なし)

差分オラクル loaddata (`unit_weapon.txt`) でマジンガーＺの 7 武器 (弾数武器/EN 武器/各属性) を
攻撃力/最小射程/最大射程/命中率/最大弾数/消費ＥＮ/クリティカル率/属性/地形適応 で全 sweep し、
**38/38 一致**。複雑な武器 CSV のパース (能力値行の技量/反応 と同種の field-order bug の懸念) は
堅牢と実証。記録のみ (是正不要)。

### ユニット `特殊能力` の section marker 行 (`全ユニット共通` 等) — Rust が取り込まない

- **Rust 実装**: unit パーサは `名前=値` 形の特殊能力行のみ取り込み、`=` を持たない
  `全ユニット共通` のような marker/ディレクティブ行を捨てる (`data/unit.rs`)。マジンガーＺの
  `特殊能力数` は 12・先頭は `ＢＧＭ`。
- **SRC.Sharp / VB6**: `全ユニット共通` を含む bare 行も特殊能力として保持する。`特殊能力数`
  は 13・先頭は `全ユニット共通`。
- **判断保留の理由**: 「全ユニット共通能力の継承」機構自体が Rust 未実装のため、marker を
  捨てても現状の機能影響は無い (継承を実装するときに併せて取り込み方を揃える)。差分オラクル
  (`oracle_loaddata`) で検出。中優先 (穴埋めロードマップ側の課題)。

### ✅ パイロットのレベル成長式 — 是正済 (§4 表参照)

差分オラクル placeunit (有人モード `unit_pilot.txt`) で発掘し是正した pervasive bug。旧 Rust は
class ベース `base+(level-1)*rate` で過大成長していた。VB6 `Pilot.cls:582-593` 準拠の
**`lv=Level` (レベル 1 でも成長)・格闘/射撃/技量/反応 +=lv・命中/回避 +=2*lv** へ `db::grown_pilot` /
`pilot_instance::apply_stat_growth` を是正 (成長スキル/`追加レベル`/`攻撃力低成長` Option は未モデル)。
併せて `Info(パイロットデータ,…)` が配置済みパイロットで成長後を返していた conflation も是正 (静的データを返す)。
`unit_pilot.txt` の 13/13 一致で cross-engine 検証 (人工知能 lv10 格闘=110・命中=165、超人工知能 lv30 格闘=155)。

### `Info(ユニット, …, 気力)` — 無人ユニットの気力 (差分オラクル placeunit で検出)

- **Rust 実装**: 気力 (morale) を `UnitInstance` に持ち、生成直後の既定値 100 を返す
  (パイロットの有無に依らない)。
- **SRC.Sharp / VB6**: 気力はパイロット属性で、`Info(ユニット, name, 気力)` は乗っているパイロットの
  気力を読む。**無人ユニット (CountPilot()==0) では空文字**を返す。
- **判断保留の理由**: 有人ユニットでは両者一致 (既定気力 100)。差が出るのは無人ユニットのみで、
  Rust が morale を instance に持つ設計上の選択。実プレイでユニットは原則有人なので影響は限定的。
  差分オラクル (`placeunit` モード) で検出。低優先。

---

最終更新: 2026-06-18（差分オラクル placeunit 有人モードでパイロットのレベル成長式の大乖離を発掘・
VB6 準拠へ是正＋Create level 配線＋パイロットデータ成長 conflation 是正）
