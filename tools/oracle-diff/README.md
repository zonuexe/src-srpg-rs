# oracle-diff — C#↔Rust 差分オラクル

原典 SRC エンジン (C# `SRCCore`) と本実装 (Rust) に**同一の式を通して結果を diff** し、
式評価の挙動を自動突合するハーネス。手動でのテスト移植 (mining) と違い、原典エンジンが
期待値を**計算**するため転記ミスが無く、コーパスを増やすほどカバレッジが上がる。

## 構成

- `oracle-diff.csproj` / `Program.cs` — SRCCore の `Expression` を standalone 駆動する
  net10.0 コンソール。標準入力の式を 1 行ずつ `GetValueAsString` で評価し標準出力へ。
- Rust 側評価器: [`tools/verify-archive/src/bin/oracle_eval.rs`](../verify-archive/src/bin/oracle_eval.rs)。
  各式を `Set z Eval(<式>)` として実行し `z` を読む。
- `corpus.txt` — 突合する式の集合 (`#`/空行スキップ)。

## 実行

```sh
NIX="nix --extra-experimental-features 'nix-command flakes'"

# C# (原典) — 初回は NuGet 復元のためネットワークが必要
$NIX develop .#dotnet --command bash -c \
  'dotnet run --project tools/oracle-diff/oracle-diff.csproj -c Release < tools/oracle-diff/corpus.txt > /tmp/cs.txt 2>/dev/null'

# Rust (本実装)
$NIX develop --command bash -c \
  'cargo run -q -p verify-archive --bin oracle_eval < tools/oracle-diff/corpus.txt > /tmp/rs.txt 2>/dev/null'

# 差分 (expr | C# | Rust、不一致のみ)
grep -vE '^#|^$' tools/oracle-diff/corpus.txt > /tmp/exprs.txt
paste -d'~' /tmp/exprs.txt /tmp/cs.txt /tmp/rs.txt | awk -F'~' '$2 != $3 {print}'
```

## スコープと制約

- **対象**: 数値・論理を返す式と数値関数 (算術 / 比較 / 論理 / Mod / ゼロ除算 /
  Int・Round・Abs・Min・Max・Sqr / Len・InStr・Asc 等)。
- **対象外**: トップレベルの文字列連結 `&` と文字列を返す関数 (Mid/Left/Replace/Format 等)。
  本実装には C# `GetValueAsString` 相当の**単一の式評価入口が無く**、評価形式が文脈依存
  (算術=`(…)` / 関数=裸 / `Eval(…)`)。代用の `Eval(…)` はトップレベル `&` を正しく
  扱えないため、文字列系は [`string_function_oracle.rs`](../../crates/src-core/tests/string_function_oracle.rs)
  で直接検証する。→ 将来 `eval_to_string(expr)` 統一入口を設ければ harness を全式へ拡張可能。

## コマンド列モード (scenario) — Commands 層の差分

式 1 つではなく**コマンド列**を両エンジンで実行し、状態 (変数/配列) を probe して diff する。
C# のユニットテストは Commands を内部構造として検証するため mining できないが、両エンジンを
駆動して diff すればコマンド実行の fidelity を直接検証できる。

入力は `===PROBES===` でコマンド列と probe 式に分ける ([`scenario_vars.txt`](scenario_vars.txt) 参照)。
逐次実行のみ (C# は per-command `Exec()` で PC 管理が無いため If/For 等の制御フローは非対応)。

```sh
# C#
$NIX develop .#dotnet --command bash -c \
  'dotnet run --project tools/oracle-diff/oracle-diff.csproj -c Release scenario < tools/oracle-diff/scenario_vars.txt > /tmp/cs.txt 2>/dev/null'
# Rust
$NIX develop --command bash -c \
  'cargo run -q -p verify-archive --bin oracle_scenario < tools/oracle-diff/scenario_vars.txt > /tmp/rs.txt 2>/dev/null'
# probe を抽出して diff
sed -n '/^===PROBES===$/,$p' tools/oracle-diff/scenario_vars.txt | grep -vE '^===|^#|^$' > /tmp/p.txt
paste -d'~' /tmp/p.txt /tmp/cs.txt /tmp/rs.txt | awk -F'~' '$2!=$3{print}'
```

## 最新の結果 (2026-06-17)

- **式モード (corpus 76 式): 75/76 が SRCCore と完全一致。** 唯一の差分 `Round(-2.5, 0)`:
  C#=-3 (SRC.Sharp の `AwayFromZero`) / Rust=-2 (VB6 原典に忠実で正しい、`docs/SRC_SHARP_DIVERGENCE.md` §2)。
- **コマンド列モード (scenario_vars.txt 9 probe): 9/9 完全一致** (Set / bareword 算術代入 / Array /
  文字列補間)。Commands 層の fidelity を実証。
- 副次発見: `Set var "x" & y` を C# は「引数の数が違う」と拒否、Rust は受理 (Rust が寛容、
  `docs/SRC_SHARP_DIVERGENCE.md` の乖離候補参照)。正規の SRC 形式は `Set var "x$(y)"`。

## 拡張

同じ machinery を**ゲーム状態**の diff へ拡張できる: 実シナリオを両エンジンで駆動し、
ユニット HP/EN/気力/レベル・スクリプト変数を JSON ダンプして突合する (combat/イベントの
fidelity を実シナリオ全体で検証)。本ハーネスはその foundation。
