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

## 戦闘予測モード (placeattack) — 命中/ダメージ/クリティカル率の差分

ユニットを 2 体生成・配置し、**combat 予測** (命中率 / ダメージ / クリティカル率) を両エンジンで
評価して diff する。これらは式関数で露出しないため、C# は map を初期化して攻撃側/守備側を `StandBy`
で配置し `UnitWeapon.HitProbability/Damage/CriticalProbability` を直接呼ぶ専用モード `placeattack`、
Rust は `oracle_loaddata` が `effective_combat_data` から `combat::predict_with_status_terrain` を
中立条件で呼ぶ。地形は EmptyTerrain (HitMod=0/DamageMod=0) で中立化する。

コーパスは [`combat_prediction.txt`](combat_prediction.txt)。`@unit` でユニットを生成し、
`@predict <attacker> <defender> <weapon_index(1-based)> <field>` (field=命中率/ダメージ/クリティカル率)
を 1 行 1 予測で評価する。

```sh
DIR="$PWD/crates/src-web/tests/fixtures/スパロボ戦記/data/スパロボ戦記"
# C#
$NIX develop .#dotnet --command bash -c \
  "dotnet run --project tools/oracle-diff/oracle-diff.csproj -c Release placeattack '$DIR' < tools/oracle-diff/combat_prediction.txt > /tmp/cs.txt 2>/dev/null"
# Rust
$NIX develop --command bash -c \
  "cargo run -q -p verify-archive --bin oracle_loaddata -- '$DIR' < tools/oracle-diff/combat_prediction.txt > /tmp/rs.txt 2>/dev/null"
grep '^@predict' tools/oracle-diff/combat_prediction.txt > /tmp/p.txt
paste -d'~' /tmp/p.txt /tmp/cs.txt /tmp/rs.txt | awk -F'~' '$2!=$3{print}'
```

> 命中率/クリティカル率 (combat_prediction.txt) は地形適応非依存。ダメージ (combat_damage.txt) は
> 両エンジンを地上に置き env=陸 で地形適応を揃えて突合する (`戦闘システム詳細.md` のダメージ式は
> 攻撃力/防御力ともに地形適応が乗る)。`@option <name>` 指令で C# 側のオプションも定義できる。

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
- **戦闘予測モード (combat_prediction.txt, placeattack): 命中率/クリティカル率 18/18 完全一致 (2026-06-18)。**
  実 fixture のユニット (マジンガーＺ/マジンカイザー/ガンダム/ゲッター１ × 人工知能 lv10/20) で命中率・
  クリティカル率を cross-engine 突合。effective_combat_data の全経路 (レベル成長 +2*lv 命中/回避・改造・
  武器命中補正・サイズ補正) が原典 C# と一致。**pervasive 実バグを検出・是正**: 命中率クランプが
  `clamp(5,95)` (他 SRPG 慣習) だったのを VB6 `Unit.cls:6694-6696` 準拠の**上限なし・最低 0** へ
  (`combat.rs`、>100=必中。旧実装は高命中でも 5% 外し/低命中でも 5% 当たる非原典挙動だった)。
  表示は描画側で `min(100)`。
- **戦闘予測モード ダメージ (combat_damage.txt, placeattack): 14/14 完全一致 (2026-06-18)。**
  両エンジンを中立地形の地上に配置し、実データの地形適応 (S/A/B/C/D) を素直に効かせて
  ダメージ式 `(攻撃力−防御力)×地形ダメージ修正` を突合 (Rust は env=陸、C# は EmptyTerrain→地上)。
  **pervasive 実バグを検出・是正**: 最低ダメージが `max(1)` だったのを VB6 `Unit.cls:7460` 準拠の
  **既定 10** (`max(10)`) へ。SRC ダメージ式は Rust と構造同一で、装甲＞攻撃力の 1 ケースの下限差が
  唯一の乖離だった。攻撃力にも武器/ユニットの地形適応が乗る (`戦闘システム詳細.md`) ことも実数で確認。
- **戦闘予測モード 地形 (combat_terrain.txt, placeattack + `@terrain <id>`): 命中率/ダメージ 13/13 一致 (2026-06-18)。**
  防御側を実地形 (平地/林/山/洞窟/砂地) に配置し命中率・ダメージを突合 (C#=`TDList.Load`+防御側セルに `UnderTerrain`
  を敷き live 参照 / Rust=`terrain_file` ロード＋`@terrain` 紐づけで `db.terrain_hit_mod/damage_mod`+env を予測へ)。
  **地形の命中修正符号の是正を cross-engine で確認**: 林 ×0.85=123 / 山 ×0.70=101 / 洞窟 ×0.75 / 砂地 (負modで攻撃側有利) ×1.10=159、
  ダメージ修正も 機械獣ガラダＫ７ on 山 = 1020×0.70=714 で一致。丸め差・符号反転なし。`@terrain <id>` は以降の `@predict` の防御側地形を指定。
- 副次発見: `Set var "x" & y` を C# は「引数の数が違う」と拒否、Rust は受理 (Rust が寛容、
  `docs/SRC_SHARP_DIVERGENCE.md` の乖離候補参照)。正規の SRC 形式は `Set var "x$(y)"`。
  C# のリスト初期化は組込みダミー (`AddDummyData`: パイロット不在 / ユニット無し) を 1 件ずつ
  足すため、件数は C#=ファイル件数+1 (PDList/UDList とも) になる。

## 移動範囲モード (moverange) — Dijkstra 移動範囲の差分

ユニットを小マップに配置し、**移動可能セルと到達コスト**を両エンジンで突合する。C# は
`Map.AreaInSpeed(u)` が `TotalMoveCost[x,y]` (1始まり・**2倍スケール**・到達コスト) を埋める、
Rust は `movement::compute_range_with` (`cell→残MP`)。両者を「2倍スケールの到達コスト」へ正規化して
比較する (Rust=`(speed-残MP)*2` / C#=`TotalMoveCost` をそのまま、座標は 0 始まりへ)。

C# モード `moverange <dir>` ＋ Rust bin `oracle_move`。指令: `@map <w> <h>` / `@cell <x> <y> <id>` /
`@unit <name> <rank> <party> <pilot> <level> <x> <y>` / `@move <name>`。コーパスは
[`move_flat.txt`](move_flat.txt)(平地で正規化検証＝完全一致)・[`move_terrain.txt`](move_terrain.txt)(地形・移動タイプ)。

```sh
DIR="$PWD/crates/src-web/tests/fixtures/スパロボ戦記/data/スパロボ戦記"
$NIX develop --command bash -c "cargo run -q -p verify-archive --bin oracle_move -- '$DIR' < tools/oracle-diff/move_terrain.txt > /tmp/rs.txt"
$NIX develop .#dotnet --command bash -c "dotnet run --project tools/oracle-diff/oracle-diff.csproj -c Release moverange '$DIR' < tools/oracle-diff/move_terrain.txt > /tmp/cs.txt"
diff <(sort /tmp/cs.txt) <(sort /tmp/rs.txt)
```

> **注**: 地上ユニットの森林/山 (move_cost 1.5/2.5) の差は **整数移動の設計上の既知乖離**
> (Rust は ceil・C# は 2倍スケールで半コスト保持。`CLAUDE.md`「移動範囲計算は整数に統一」)。
> 飛行/水中/宇宙移動の**特殊コストのスケール移植バグ**は別途是正対象 (下記)。

## 気力/精神モード (placeattack + @morale/@spirit) — 気力スケーリングと精神倍率の差分

`@morale <unit> <value>` でパイロット気力を、`@spirit <unit> <name>` で精神/状態 (熱血/魂/鉄壁 等) を
設定してダメージを突合する。C# は `pilot.Morale=`/`unit.MakeSpecialPowerInEffect(name)`＋`SPDList.Load(system/sp.txt)`、
Rust は `predict_with_status_terrain` の morale 引数と status スライスへ配線。コーパス [`combat_morale.txt`](combat_morale.txt)。

> **判明した乖離**:
> 1. **気力スケーリング自体は一致** (×気力/100 を両エンジン整数で適用、120/50/150 等で一致)。
> 2. ✅ **ブースト (ユニット特殊能力) の高気力 ×1.25 を是正済**（`9f89196`）。気力 150 で C# のみ ×1.25 していた真因は
>    予測が攻撃側ユニットの `ブースト` を未参照だったこと（`全ユニット共通` 継承ではない＝共通能力は各ユニットに明示列挙され
>    Rust も `ブースト=…` を features へ取り込む）。配線後は気力 150 のダメージが cross-engine 一致 (1206→1507)。
> 3. ✅ **攻撃側ダメージ増加精神を sp.txt データ駆動・MaxDbl 非加算へ是正済**（`6dcdad8`）。`SpecialPowerData.effects` に `ダメージ増加Lv` をパースし、
>    `db::sp_damage_increase_level`（active 精神の最大 Lv）で解決して `predict` が `1 + 0.1*Lv`（MaxDbl）を適用。熱血+魂=×2.5(210)・
>    気合=×1.0(気力増加で与ダメ非増) で **morale corpus 10/10 一致**。防御側 被ダメージ低下（鉄壁/不屈/バリア）・捨て身/攻撃力ＵＰ は範囲外（従来どおり名前ベース）。

## 拡張

同じ machinery を**ゲーム状態**の diff へ拡張できる: 実シナリオを両エンジンで駆動し、
ユニット HP/EN/気力/レベル・スクリプト変数を JSON ダンプして突合する (combat/イベントの
fidelity を実シナリオ全体で検証)。本ハーネスはその foundation。
