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

## データロードモード (loaddata) — 静的ユニット/パイロットデータの差分

実シナリオの**データディレクトリ** (`pilot.txt` / `unit.txt` / …) を両エンジンにロードし、
同一の `Info(ユニットデータ, …)` / `Info(パイロットデータ, …)` probe を評価して diff する。
パーサと Info 照会の fidelity を cross-engine で突合する層 (ユニット/combat 状態 diff の
foundation = 静的データ層)。C# は `SRC.LoadDataDirectory(dir)`、Rust は新設の
[`oracle_loaddata`](../verify-archive/src/bin/oracle_loaddata.rs) が同じファイル群を同順でロードする。

コーパスは [`unit_data.txt`](unit_data.txt)。対象データは
`crates/src-web/tests/fixtures/スパロボ戦記/data/スパロボ戦記`。

```sh
DIR="$PWD/crates/src-web/tests/fixtures/スパロボ戦記/data/スパロボ戦記"
# C#
$NIX develop .#dotnet --command bash -c \
  "dotnet run --project tools/oracle-diff/oracle-diff.csproj -c Release loaddata '$DIR' < tools/oracle-diff/unit_data.txt > /tmp/cs.txt 2>/dev/null"
# Rust
$NIX develop --command bash -c \
  "cargo run -q -p verify-archive --bin oracle_loaddata -- '$DIR' < tools/oracle-diff/unit_data.txt > /tmp/rs.txt 2>/dev/null"
# probe を抽出して diff
grep -vE '^#|^$' tools/oracle-diff/unit_data.txt > /tmp/p.txt
paste -d'~' /tmp/p.txt /tmp/cs.txt /tmp/rs.txt | awk -F'~' '$2!=$3{print}'
```

## ユニット実体モード (placeunit) — UnitInstance 状態の差分

データロード後に**ユニット実体を生成**し、`Info(ユニット, …)` で実効値を diff する
(静的データ層 loaddata の次フロンティア = stage a-2)。`@unit <name> <rank> <party>` 指令で
両エンジンが同一ユニットを作る:
- **C#** (`placeunit <dir>`): `UList.Add(name, rank, party)` + `FullRecover()` (GUI 依存の
  `CreateCmd` を経ず `Units/` テストと同じ低レベル API。`Unit.MaxHP`/`HP`/`装甲` getter は Map を
  参照しないため map 配置 (`StandBy`) 不要)。
- **Rust** (`oracle_loaddata` の `@unit` 拡張): `Create <party> <name> <rank> - 0 <x> 1`。

`rank` は改造段階 (1 段ごとに HP+200/装甲+100/EN+10/運動性+5)。コーパスは
[`unit_instance.txt`](unit_instance.txt)。

```sh
DIR="$PWD/crates/src-web/tests/fixtures/スパロボ戦記/data/スパロボ戦記"
$NIX develop .#dotnet --command bash -c \
  "dotnet run --project tools/oracle-diff/oracle-diff.csproj -c Release placeunit '$DIR' < tools/oracle-diff/unit_instance.txt > /tmp/cs.txt 2>/dev/null"
$NIX develop --command bash -c \
  "cargo run -q -p verify-archive --bin oracle_loaddata -- '$DIR' < tools/oracle-diff/unit_instance.txt > /tmp/rs.txt 2>/dev/null"
grep -vE '^#|^$|^@' tools/oracle-diff/unit_instance.txt > /tmp/p.txt
paste -d'~' /tmp/p.txt /tmp/cs.txt /tmp/rs.txt | awk -F'~' '$2!=$3{print}'
```

## 最新の結果 (2026-06-17)

- **式モード (corpus 76 式): 75/76 が SRCCore と完全一致。** 唯一の差分 `Round(-2.5, 0)`:
  C#=-3 (SRC.Sharp の `AwayFromZero`) / Rust=-2 (VB6 原典に忠実で正しい、`docs/SRC_SHARP_DIVERGENCE.md` §2)。
- **コマンド列モード (scenario_vars.txt 9 probe): 9/9 完全一致** (Set / bareword 算術代入 / Array /
  文字列補間)。Commands 層の fidelity を実証。
- **データロードモード (unit_data.txt 61 probe): 58/61 一致。** 残 3 件は既知乖離として記録済
  (`docs/SRC_SHARP_DIVERGENCE.md`): ① ユニット `特殊能力数` 13(C#)/12(Rust)・② 同 `特殊能力名称,1`
  (C#=`全ユニット共通`/Rust=`ＢＧＭ`) = unit パーサが bare marker 行を捨てる差・③ パイロット `性別`
  (C#=`-`/Rust=空) = `Sex` enum 正規化差。**実バグ 1 件を検出・是正**: pilot.txt 能力値行の 5/6 番目
  (技量/反応) 取り違え (`Info(…,技量)` C#=135/旧 Rust=80 → `803e13d` で VB6 順に是正)。
- **ユニット実体モード (unit_instance.txt 25 probe): 24/25 一致。** 残 1 件は `気力` (無人ユニット):
  C# はパイロット属性で空・Rust は UnitInstance 既定 100 (有人なら一致、既知乖離)。**実バグ 1 件を
  検出・是正**: `Create party unit rank …` の rank(改造段階) を無視していた (rank2 で C#=MaxHP+400 に対し
  旧 Rust=素の値)。`upgrade_level` へ配線して rank 0/2/3/5 の HP/EN/装甲/運動性が cross-engine 一致。
- **有人ユニットモード (unit_pilot.txt 13 probe): 13/13 一致。** Create level を初期 exp へ配線 (level/累積
  経験値が一致)。**pervasive 実バグを検出・是正**: パイロットのレベル成長式が class ベース過大成長だった
  → VB6 `Pilot.cls:582-593` 準拠 (`lv=Level`・格闘等 +lv・命中/回避 +2*lv) へ。併せて `Info(パイロットデータ)` の
  成長 conflation も是正。
- **武器フィールド (unit_weapon.txt 38 probe): 38/38 一致** (乖離なし＝武器パーサ堅牢)。
- **パイロット SP/特殊能力 (pilot_feature.txt 13 probe): 11/13 一致。** 残 2 件は `特殊能力名称` 列挙の
  既知乖離 (C# は別名 RHS・Rust は key LHS。`特殊能力所有` 所有判定は一致＝表示のみ、`docs/SRC_SHARP_DIVERGENCE.md`)。
- 副次発見: `Set var "x" & y` を C# は「引数の数が違う」と拒否、Rust は受理 (Rust が寛容、
  `docs/SRC_SHARP_DIVERGENCE.md` の乖離候補参照)。正規の SRC 形式は `Set var "x$(y)"`。
  C# のリスト初期化は組込みダミー (`AddDummyData`: パイロット不在 / ユニット無し) を 1 件ずつ
  足すため、件数は C#=ファイル件数+1 (PDList/UDList とも) になる。

## 拡張

同じ machinery を**ゲーム状態**の diff へ拡張できる: 実シナリオを両エンジンで駆動し、
ユニット HP/EN/気力/レベル・スクリプト変数を JSON ダンプして突合する (combat/イベントの
fidelity を実シナリオ全体で検証)。本ハーネスはその foundation。
