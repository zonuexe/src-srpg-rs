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

exp→level は併せて、実装中に 16 箇所重複していた `total_exp/100` を正典関数
`pilot_instance::level_from_exp` に集約した（1 箇所修正漏れでレベル不整合になる罠の除去）。

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

---

最終更新: 2026-06-17
