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

---

最終更新: 2026-05-20
