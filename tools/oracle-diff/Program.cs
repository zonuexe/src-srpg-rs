using System;
using System.Collections.Generic;
using SRCCore;
using SRCCore.CmdDatas;
using SRCCore.Events;
using SRCCore.Expressions;
using SRCCore.Filesystem;
using SRCCore.Models;
using SRCCore.TestLib;
using SRCCore.Units;

namespace OracleDiff
{
    /// <summary>
    /// 原典 SRCCore を standalone 駆動する差分オラクル。2 モード:
    ///
    /// (既定) 式モード: 標準入力の式を 1 行ずつ `GetValueAsString` で評価して出力。
    ///
    /// `scenario` 引数: コマンド列モード。標準入力を `===PROBES===` で分け、上段の
    /// コマンドを順に parse+Exec し、下段の probe 式を `GetValueAsString` で評価して出力。
    /// (逐次実行のみ。If/For 等の制御フローは PC 管理が要るため非対応。)
    ///
    /// 空行・`#` 始まりはスキップ。評価例外は `&lt;ERR:型名&gt;`。
    /// </summary>
    internal static class Program
    {
        private static int Main(string[] args)
        {
            if (args.Length > 0 && args[0] == "scenario")
            {
                return RunScenario();
            }
            if (args.Length > 1 && args[0] == "loaddata")
            {
                return RunLoadData(args[1]);
            }
            if (args.Length > 1 && args[0] == "placeunit")
            {
                return RunPlaceUnit(args[1]);
            }
            if (args.Length > 1 && args[0] == "placeattack")
            {
                return RunPlaceAttack(args[1]);
            }
            if (args.Length > 1 && args[0] == "moverange")
            {
                return RunMoveRange(args[1]);
            }
            return RunExpressions();
        }

        private static int RunExpressions()
        {
            var src = new SRC { GUI = new MockGUI() };
            var exp = new Expression(src);
            string line;
            while ((line = Console.In.ReadLine()) != null)
            {
                if (line.Length == 0 || line[0] == '#')
                {
                    continue;
                }
                Console.WriteLine(Eval(exp, line));
            }
            return 0;
        }

        private static int RunScenario()
        {
            var src = new SRC { GUI = new MockGUI() };
            src.Event.EventData = new List<EventDataLine>();
            src.Event.EventCmd = new List<CmdData>();
            src.Event.EventFileNames = new List<string>();
            src.Event.AdditionalEventFileNames = new List<string>();
            src.Event.EventQue = new Queue<string>();
            var parser = new CmdParser();

            var probes = new List<string>();
            var inProbes = false;
            var id = 0;
            string line;
            while ((line = Console.In.ReadLine()) != null)
            {
                if (line == "===PROBES===")
                {
                    inProbes = true;
                    continue;
                }
                if (line.Length == 0 || line[0] == '#')
                {
                    continue;
                }
                if (inProbes)
                {
                    probes.Add(line);
                    continue;
                }
                try
                {
                    var dl = new EventDataLine(id, EventDataSource.Scenario, "test", id, line);
                    src.Event.EventData.Add(dl);
                    var cmd = parser.Parse(src, dl);
                    src.Event.EventCmd.Add(cmd);
                    cmd.Exec();
                }
                catch (Exception e)
                {
                    Console.Error.WriteLine("cmd err [" + line + "]: " + e.Message);
                }
                id++;
            }

            foreach (var p in probes)
            {
                Console.WriteLine(Eval(src.Expression, p));
            }
            return 0;
        }

        // データロード検証モード: pilot.txt/unit.txt をロードし、標準入力の probe を評価。
        // 調査結果 (2026-06-17): LoadDataDirectory は headless で動き PDList/UDList を populate するが、
        // (a) Info/HP/MaxHP 等のユニットクエリは **PLACED ユニット (UList)** を読むため、データを
        //     ロードしただけでは解決しない (Create/Place + map 初期化が要る)。
        // (b) UDList の件数が少ないのは IncludeData 機構 (scenario の Include 指定で複数 data dir を
        //     合成) を通していないため。フル単体検証には scenario ロード経路が要る。
        // → ユニット/combat 状態の cross-engine diff はこの map 初期化 + 配置 (+combat は RNG 一致) を
        //    要する大きめの統合。本モードは「データロード API が headless で動く」ことの実証用。
        private static int RunLoadData(string dir)
        {
            var src = new SRC { GUI = new MockGUI(), FileSystem = new LocalFileSystem() };
            try
            {
                src.LoadDataDirectory(dir);
            }
            catch (Exception e)
            {
                Console.Error.WriteLine("LoadDataDirectory failed: " + e);
                return 1;
            }
            Console.Error.WriteLine("loaded: PDList=" + src.PDList.Count() + " UDList=" + src.UDList.Count());
            string line;
            while ((line = Console.In.ReadLine()) != null)
            {
                if (line.Length == 0 || line[0] == '#')
                {
                    continue;
                }
                Console.WriteLine(Eval(src.Expression, line));
            }
            return 0;
        }

        // ユニット実体状態 diff モード: データロード後、`@unit <name> <rank> <party>` 指令で
        // ユニットを生成 (UList.Add + FullRecover; GUI 依存の CreateCmd を経ず Units/ テストと
        // 同じ低レベル API)、`===PROBES===` 不要で `@`/`#` 以外の行を probe として評価。
        // map 配置 (StandBy) は不要 — MaxHP/HP/装甲/運動性 の getter は Map を参照しない。
        private static int RunPlaceUnit(string dir)
        {
            var src = new SRC { GUI = new MockGUI(), FileSystem = new LocalFileSystem() };
            try
            {
                src.LoadDataDirectory(dir);
            }
            catch (Exception e)
            {
                Console.Error.WriteLine("LoadDataDirectory failed: " + e);
                return 1;
            }

            var probes = new List<string>();
            var created = 0;
            string line;
            while ((line = Console.In.ReadLine()) != null)
            {
                if (line.Length == 0 || line[0] == '#')
                {
                    continue;
                }
                if (line.StartsWith("@unit "))
                {
                    // `@unit <name> <rank> <party>` (無人) または
                    // `@unit <name> <rank> <party> <pilot> <level>` (有人)。
                    var parts = line.Substring(6)
                        .Split(' ', StringSplitOptions.RemoveEmptyEntries);
                    if (parts.Length >= 3)
                    {
                        var name = parts[0];
                        var rank = int.TryParse(parts[1], out var r) ? r : 0;
                        var party = parts[2];
                        var u = src.UList.Add(name, rank, party);
                        if (u != null)
                        {
                            if (parts.Length >= 5)
                            {
                                var pname = parts[3];
                                var plevel = int.TryParse(parts[4], out var l) ? l : 1;
                                var p = src.PList.Add(pname, plevel, party);
                                if (p != null)
                                {
                                    p.Ride(u);
                                }
                                else
                                {
                                    Console.Error.WriteLine("PList.Add returned null: " + pname);
                                }
                            }
                            u.FullRecover();
                            created++;
                        }
                        else
                        {
                            Console.Error.WriteLine("UList.Add returned null: " + name);
                        }
                    }
                    continue;
                }
                probes.Add(line);
            }
            Console.Error.WriteLine("created=" + created + " UList=" + src.UList.Count());
            foreach (var p in probes)
            {
                Console.WriteLine(Eval(src.Expression, p));
            }
            return 0;
        }

        // 戦闘予測 diff モード: データロード後、map を初期化して各 `@unit` を配置 (StandBy)、
        // `@predict <attacker> <defender> <weapon_index(1-based)> <field>` 指令を順に評価し、
        // 結果整数を 1 行ずつ出力する。field は 命中率 / ダメージ / クリティカル率。
        //   命中率 → UnitWeapon.HitProbability(defender, true)
        //   ダメージ → UnitWeapon.Damage(defender, true)
        //   クリティカル率 → UnitWeapon.CriticalProbability(defender, "")
        // 地形は既定で EmptyTerrain (HitMod=0/DamageMod=0/Class="" → StandBy で Area="地上") で
        // 中立化。`@terrain <id>` 指令で以降の `@predict` の防御側セル地形を切り替える
        // (下記参照)。
        private static int RunPlaceAttack(string dir)
        {
            var src = new SRC { GUI = new MockGUI(), GUIMap = new MockGUIMap(), FileSystem = new LocalFileSystem() };
            try
            {
                src.LoadDataDirectory(dir);
            }
            catch (Exception e)
            {
                Console.Error.WriteLine("LoadDataDirectory failed: " + e);
                return 1;
            }

            // terrain.txt は LoadDataDirectory が読まないため明示ロードする。原典 SRC は
            // `<ScenarioPath>/Data/System/terrain.txt` 等を `TDList.Load` で読むが、本ハーネスでは
            // シナリオ data dir の `../system/terrain.txt` (= <dir>/../system/terrain.txt) を直接渡す。
            var terrainPath = System.IO.Path.Combine(dir, "..", "system", "terrain.txt");
            if (System.IO.File.Exists(terrainPath))
            {
                try
                {
                    src.TDList.Load(terrainPath);
                    Console.Error.WriteLine("terrain loaded: " + System.IO.Path.GetFullPath(terrainPath));
                }
                catch (Exception e)
                {
                    Console.Error.WriteLine("TDList.Load failed: " + e.Message);
                }
            }
            else
            {
                Console.Error.WriteLine("terrain.txt not found: " + System.IO.Path.GetFullPath(terrainPath));
            }

            // sp.txt (スペシャルパワーデータ) は LoadDataDirectory が `<dir>/sp.txt` のみ見るが、本
            // フィクスチャでは `<dir>/../system/sp.txt` に置かれるため、`@spirit` を解決するには明示
            // ロードが要る (terrain.txt と同経路)。`MakeSpecialPowerInEffect` が付与する Condition は
            // `IsUnderSpecialPowerEffect`/`SpecialPowerEffectLevel` で `SRC.SPDList.Item(name)` を引く
            // ため、ここで SPDList を populate しないと付与しても効果レベル 0 (= 倍率 1.0) の no-op になる。
            var spPath = System.IO.Path.Combine(dir, "..", "system", "sp.txt");
            if (System.IO.File.Exists(spPath))
            {
                try
                {
                    src.SPDList.Load(spPath);
                    Console.Error.WriteLine("sp loaded: " + System.IO.Path.GetFullPath(spPath) + " (SPDList=" + src.SPDList.Count() + ")");
                }
                catch (Exception e)
                {
                    Console.Error.WriteLine("SPDList.Load failed: " + e.Message);
                }
            }
            else
            {
                Console.Error.WriteLine("sp.txt not found: " + System.IO.Path.GetFullPath(spPath));
            }

            // map を初期化し、全セルを中立地形 (EmptyTerrain: HitMod=0/DamageMod=0) で埋める。
            // SetMapSize は末尾で GUIMap.SetMapSize を呼ぶため headless 用 no-op を渡す。
            src.Map.SetMapSize(20, 20);
            foreach (var x in src.Map.MapData.XRange)
            {
                foreach (var y in src.Map.MapData.YRange)
                {
                    src.Map.MapData[x, y].UnderTerrain = TerrainData.EmptyTerrain;
                }
            }

            var units = new Dictionary<string, Unit>();
            // predicts は (predict 行, 地形 id, ユニット別気力スナップショット, ユニット別精神名スナップ
            // ショット) の組で保持する。気力/精神は corpus 上で `@morale`/`@spirit` により累積的に変化
            // するが、C# は predict をまとめて末尾で評価するため、各 `@predict` を読んだ時点の状態を
            // **スナップショット**して紐づける (terrain と同じ遅延適用方式)。
            var predicts = new List<(string Line, int Terrain,
                Dictionary<string, int> Morale, Dictionary<string, List<string>> Spirits)>();
            var curTerrain = -1; // -1 = 中立 (EmptyTerrain)
            // ユニットデータ名 → 気力 (既定 100) / アクティブ精神名リスト。corpus 順に更新する。
            var moraleMap = new Dictionary<string, int>();
            var spiritMap = new Dictionary<string, List<string>>();
            var created = 0;
            string line;
            while ((line = Console.In.ReadLine()) != null)
            {
                if (line.Length == 0 || line[0] == '#')
                {
                    continue;
                }
                if (line.StartsWith("@terrain "))
                {
                    // `@terrain <id>` → 以降の `@predict` で防御側セルに敷く地形 id を設定。
                    // id が TDList に未定義なら中立 (-1) 扱いに戻す。
                    var ts = line.Substring(9).Trim();
                    if (int.TryParse(ts, out var tid))
                    {
                        curTerrain = tid;
                    }
                    continue;
                }
                if (line.StartsWith("@unit "))
                {
                    // `@unit <name> <rank> <party>` (無人) / `@unit <name> <rank> <party> <pilot> <level>`
                    var parts = line.Substring(6)
                        .Split(' ', StringSplitOptions.RemoveEmptyEntries);
                    if (parts.Length >= 3)
                    {
                        var name = parts[0];
                        var rank = int.TryParse(parts[1], out var r) ? r : 0;
                        var party = parts[2];
                        var u = src.UList.Add(name, rank, party);
                        if (u != null)
                        {
                            if (parts.Length >= 5)
                            {
                                var pname = parts[3];
                                var plevel = int.TryParse(parts[4], out var l) ? l : 1;
                                var p = src.PList.Add(pname, plevel, party);
                                if (p != null)
                                {
                                    p.Ride(u);
                                }
                                else
                                {
                                    Console.Error.WriteLine("PList.Add returned null: " + pname);
                                }
                            }
                            u.FullRecover();
                            // 配置: ユニット同士が隣接しない位置 (1-indexed) へ。指令順カウンタで散らす。
                            u.StandBy(1 + 2 * created, 5);
                            // 名前 (ユニットデータ名) で引けるよう辞書登録。重複名は最後勝ち。
                            units[name] = u;
                            created++;
                        }
                        else
                        {
                            Console.Error.WriteLine("UList.Add returned null: " + name);
                        }
                    }
                    continue;
                }
                if (line.StartsWith("@morale "))
                {
                    // `@morale <unitName> <value>` → 以降の `@predict` でそのユニットのメインパイロット
                    // 気力に value を設定する (corpus 順に累積)。値は SetMorale で [MinMorale,MaxMorale]
                    // (既定 50..150) にクランプされる点に注意。
                    var parts = line.Substring(8)
                        .Split(' ', StringSplitOptions.RemoveEmptyEntries);
                    if (parts.Length >= 2 && int.TryParse(parts[1], out var mv))
                    {
                        moraleMap[parts[0]] = mv;
                    }
                    continue;
                }
                if (line.StartsWith("@spirit "))
                {
                    // `@spirit <unitName> <spiritName>` → 以降の `@predict` でそのユニットに精神コマンド
                    // <spiritName> をアクティブにする (Unit.MakeSpecialPowerInEffect、SP消費/GUI なし)。
                    // 1 ユニットに複数指定可。同名は冪等。
                    var parts = line.Substring(8)
                        .Split(' ', StringSplitOptions.RemoveEmptyEntries);
                    if (parts.Length >= 2)
                    {
                        if (!spiritMap.TryGetValue(parts[0], out var lst))
                        {
                            lst = new List<string>();
                            spiritMap[parts[0]] = lst;
                        }
                        if (!lst.Contains(parts[1]))
                        {
                            lst.Add(parts[1]);
                        }
                    }
                    continue;
                }
                if (line.StartsWith("@predict "))
                {
                    // 現在アクティブな地形 id (@terrain で設定) と、気力/精神の現在スナップショットを
                    // 予測行に紐づける (深いコピー: 後続の @morale/@spirit に影響されないよう複製)。
                    var moraleSnap = new Dictionary<string, int>(moraleMap);
                    var spiritSnap = new Dictionary<string, List<string>>();
                    foreach (var kv in spiritMap)
                    {
                        spiritSnap[kv.Key] = new List<string>(kv.Value);
                    }
                    predicts.Add((line, curTerrain, moraleSnap, spiritSnap));
                    continue;
                }
                if (line.StartsWith("@option "))
                {
                    // `@option <name>` → グローバル変数 `Option(<name>)` を 1 で定義し、
                    // `Expression.IsOptionDefined(<name>)` を true にする (原典 OptionCmd と同経路:
                    // CmdDatas/Commands/Other/OptionCmd.cs)。予測実行前に効くよう即時定義する。
                    // 用途: `地形適応命中率修正` を立て UnitWeapon.Damage の uadaption を 1.0 へ強制
                    // (UnitWeapon.cs:2774-2836)、Rust 側 env=-1 (適応 ×1.0) と整合させる。
                    var oname = line.Substring(8).Trim();
                    if (oname.Length > 0)
                    {
                        var vname = "Option(" + oname + ")";
                        if (!src.Expression.IsGlobalVariableDefined(vname))
                        {
                            src.Expression.DefineGlobalVariable(vname);
                        }
                        src.Expression.SetVariableAsLong(vname, 1);
                    }
                    continue;
                }
                // それ以外の行は本モードでは無視 (combat corpus は @unit + @predict + @option のみ)。
            }
            Console.Error.WriteLine("created=" + created + " UList=" + src.UList.Count());

            foreach (var (pr, terrainId, moraleSnap, spiritSnap) in predicts)
            {
                // `@predict <attacker> <defender> <weapon_index> <field>`
                var parts = pr.Substring(9)
                    .Split(' ', StringSplitOptions.RemoveEmptyEntries);
                if (parts.Length < 4)
                {
                    Console.WriteLine("<ERR:args>");
                    continue;
                }
                var aname = parts[0];
                var dname = parts[1];
                var widx = int.TryParse(parts[2], out var wi) ? wi : 0;
                var field = parts[3];
                if (!units.TryGetValue(aname, out var attacker)
                    || !units.TryGetValue(dname, out var defender))
                {
                    Console.WriteLine("<ERR:lookup>");
                    continue;
                }
                // この予測のスナップショットへ攻撃側/防御側の状態を確定させる。各 predict を独立に
                // 扱うため、関与する全ユニットの気力を snapshot 値 (未指定は 100) へ、精神を snapshot
                // のリストへ「リセット → 再付与」する (後続 predict の累積が前の predict に漏れない)。
                // MoraleMod は除外して気力をそのまま倍率に乗せるため 0 に固定する
                // (Damage は (Morale + MoraleMod)/100 を用いる: UnitWeapon.cs:259-267 / 2953-2961)。
                foreach (var kv in units)
                {
                    // 辞書キー (= @unit/@morale/@spirit が使うユニットデータ名) で snapshot を引く。
                    var uname = kv.Key;
                    var u = kv.Value;
                    var p = u.MainPilot();
                    if (p == null)
                    {
                        continue;
                    }
                    p.MoraleMod = 0;
                    p.Morale = moraleSnap.TryGetValue(uname, out var mv) ? mv : 100;
                    u.RemoveAllSpecialPowerInEffect();
                    if (spiritSnap.TryGetValue(uname, out var spirits))
                    {
                        foreach (var sp in spirits)
                        {
                            // Unit へ精神を付与 (SP消費/GUI なし、データ駆動)。SPDList 未定義名は
                            // 効果レベル 0 → 倍率 1.0 の no-op になる (上の SPDList ロードで回避)。
                            u.MakeSpecialPowerInEffect(sp);
                        }
                    }
                }
                // 防御側が立つセルの地形を @terrain で指定された id に設定する。
                // HitProbability/Damage は `Map.Terrain(defender.x, defender.y)` を毎回 live に
                // 読むため、StandBy 後の本変更も即反映される (UnitWeapon.cs:2056-2059 / 2988-3026)。
                // 採用地形 (林=11/山=15/洞窟=58/砂地=1) は全て Class="陸" のため StandBy 時に
                // キャッシュ済みの defender.Area="地上" と整合し、命中/ダメージ修正が正しく適用される。
                // id<0 は中立 (EmptyTerrain) に戻す。
                var defTerrain = terrainId >= 0 ? src.TDList.Item(terrainId) : TerrainData.EmptyTerrain;
                src.Map.MapData[defender.x, defender.y].UnderTerrain = defTerrain;
                try
                {
                    var w = attacker.Weapon(widx);
                    if (w == null)
                    {
                        Console.WriteLine("<ERR:weapon>");
                        continue;
                    }
                    int val;
                    switch (field)
                    {
                        case "命中率":
                            val = w.HitProbability(defender, true);
                            break;
                        case "ダメージ":
                            val = w.Damage(defender, true);
                            break;
                        case "クリティカル率":
                            val = w.CriticalProbability(defender, "");
                            break;
                        default:
                            Console.WriteLine("<ERR:field>");
                            continue;
                    }
                    Console.WriteLine(val);
                }
                catch (Exception e)
                {
                    Console.WriteLine("<ERR:" + e.GetType().Name + ">");
                }
            }
            return 0;
        }

        // 移動範囲 diff モード: データロード後、指令言語でマップ・ユニットを組み立て、
        // 各ユニットの到達可能マス集合を共通正規化 `<x> <y> <cost2x>` で出力する。
        //
        // 原典 SRC は `Map.AreaInSpeed(u)` が `Map.TotalMoveCost[x,y]` (2× game scale,
        // start=0、不到達=1000000) を埋める。到達可能判定は `TotalMoveCost <= 2*Speed`。
        // 注意: `MaskData` は AreaInSpeed 末尾で「到達可能かつ空きマス → false」「不到達 or
        // ユニット存在 → true」と **反転** して使われる (Map.cs:1737-1766)。start セルには
        // currentUnit がいるため MaskData[start]=true となり「不到達」と区別できない。よって
        // ここでは MaskData ではなく `TotalMoveCost[x,y] <= 2*Speed` を到達可能の基準にする
        // (start=0 も含まれ、Rust `compute_range_with` の戻り (start 込み) と整合する)。
        //
        // 指令言語 (stdin):
        //   @map <w> <h>                         w×h の平地マップ (terrain id 0) を生成。
        //   @cell <x> <y> <terrain_id>           セル (0-indexed) の地形 id を上書き。
        //   @unit <name> <rank> <party> <pilot> <level> <x> <y>   ユニットを (x,y) 0-indexed に配置。
        //   @move <name>                         そのユニットの到達マスを出力 (ヘッダ付き)。
        //
        // 座標正規化: C# は 1-indexed のため出力時に x-1/y-1 する。配置は StandBy(x+1,y+1)。
        private static int RunMoveRange(string dir)
        {
            var src = new SRC { GUI = new MockGUI(), GUIMap = new MockGUIMap(), FileSystem = new LocalFileSystem() };
            try
            {
                src.LoadDataDirectory(dir);
            }
            catch (Exception e)
            {
                Console.Error.WriteLine("LoadDataDirectory failed: " + e);
                return 1;
            }

            // terrain.txt は LoadDataDirectory が読まないため明示ロードする
            // (placeattack モードと同経路: <dir>/../system/terrain.txt)。
            var terrainPath = System.IO.Path.Combine(dir, "..", "system", "terrain.txt");
            if (System.IO.File.Exists(terrainPath))
            {
                try
                {
                    src.TDList.Load(terrainPath);
                    Console.Error.WriteLine("terrain loaded: " + System.IO.Path.GetFullPath(terrainPath));
                }
                catch (Exception e)
                {
                    Console.Error.WriteLine("TDList.Load failed: " + e.Message);
                }
            }
            else
            {
                Console.Error.WriteLine("terrain.txt not found: " + System.IO.Path.GetFullPath(terrainPath));
            }

            // 指令を順に処理: @map → @cell → @unit → @move。読みながら状態を更新する。
            var units = new Dictionary<string, Unit>();
            var mapReady = false;
            var output = new System.Text.StringBuilder();
            string line;
            while ((line = Console.In.ReadLine()) != null)
            {
                if (line.Length == 0 || line[0] == '#')
                {
                    continue;
                }
                // 行末コメント (" #...") を剥がす (corpus 可読性のため)。
                var hash = line.IndexOf(" #", StringComparison.Ordinal);
                if (hash >= 0)
                {
                    line = line.Substring(0, hash);
                }
                line = line.TrimEnd();
                if (line.Length == 0)
                {
                    continue;
                }

                if (line.StartsWith("@map "))
                {
                    var parts = line.Substring(5)
                        .Split(' ', StringSplitOptions.RemoveEmptyEntries);
                    if (parts.Length >= 2
                        && int.TryParse(parts[0], out var w)
                        && int.TryParse(parts[1], out var h))
                    {
                        src.Map.SetMapSize(w, h);
                        foreach (var x in src.Map.MapData.XRange)
                        {
                            foreach (var y in src.Map.MapData.YRange)
                            {
                                // 全セルを平地 (terrain id 0) に初期化する。
                                src.Map.MapData[x, y].TerrainType = 0;
                                src.Map.MapData[x, y].UnderTerrain = src.TDList.Item(0);
                            }
                        }
                        mapReady = true;
                    }
                    continue;
                }
                if (line.StartsWith("@cell "))
                {
                    // `@cell <x> <y> <terrain_id>` (x,y は 0-indexed → 内部 1-indexed)。
                    var parts = line.Substring(6)
                        .Split(' ', StringSplitOptions.RemoveEmptyEntries);
                    if (mapReady && parts.Length >= 3
                        && int.TryParse(parts[0], out var x0)
                        && int.TryParse(parts[1], out var y0)
                        && int.TryParse(parts[2], out var tid))
                    {
                        var x = x0 + 1;
                        var y = y0 + 1;
                        if (x >= 1 && x <= src.Map.MapWidth && y >= 1 && y <= src.Map.MapHeight)
                        {
                            src.Map.MapData[x, y].TerrainType = tid;
                            src.Map.MapData[x, y].UnderTerrain = src.TDList.Item(tid);
                        }
                    }
                    continue;
                }
                if (line.StartsWith("@unit "))
                {
                    // `@unit <name> <rank> <party> <pilot> <level> <x> <y>` (x,y は 0-indexed)。
                    var parts = line.Substring(6)
                        .Split(' ', StringSplitOptions.RemoveEmptyEntries);
                    if (parts.Length >= 7)
                    {
                        var name = parts[0];
                        var rank = int.TryParse(parts[1], out var r) ? r : 0;
                        var party = parts[2];
                        var pname = parts[3];
                        var plevel = int.TryParse(parts[4], out var l) ? l : 1;
                        var x0 = int.TryParse(parts[5], out var px) ? px : 0;
                        var y0 = int.TryParse(parts[6], out var py) ? py : 0;
                        var u = src.UList.Add(name, rank, party);
                        if (u != null)
                        {
                            if (pname != "-")
                            {
                                var p = src.PList.Add(pname, plevel, party);
                                if (p != null)
                                {
                                    p.Ride(u);
                                }
                                else
                                {
                                    Console.Error.WriteLine("PList.Add returned null: " + pname);
                                }
                            }
                            u.FullRecover();
                            // 0-indexed → 1-indexed で配置。StandBy は地形/占有を考慮し
                            // 実際の着地セルを u.x/u.y に確定させる (= 真の start)。
                            u.StandBy(x0 + 1, y0 + 1);
                            units[name] = u;
                        }
                        else
                        {
                            Console.Error.WriteLine("UList.Add returned null: " + name);
                        }
                    }
                    continue;
                }
                if (line.StartsWith("@move "))
                {
                    var name = line.Substring(6).Trim();
                    output.Append("=== move " + name + " ===\n");
                    if (!units.TryGetValue(name, out var u))
                    {
                        output.Append("<ERR:nounit>\n");
                        continue;
                    }
                    // 2× scale の移動力 (AreaInSpeed 内部と同じ uspeed)。
                    var uspeed = 2 * u.Speed;
                    Console.Error.WriteLine(
                        "move " + name + ": speed=" + u.Speed + " area=" + u.Area
                        + " start=(" + (u.x - 1) + "," + (u.y - 1) + ")");
                    src.Map.AreaInSpeed(u);
                    // 到達可能 = TotalMoveCost <= uspeed。共通正規化 `<x> <y> <cost2x>`、
                    // 座標は 0-indexed、(x,y) 昇順で出力する。
                    var rows = new List<(int X, int Y, int C)>();
                    for (var x = 1; x <= src.Map.MapWidth; x++)
                    {
                        for (var y = 1; y <= src.Map.MapHeight; y++)
                        {
                            var c = src.Map.TotalMoveCost[x, y];
                            if (c <= uspeed)
                            {
                                rows.Add((x - 1, y - 1, c));
                            }
                        }
                    }
                    rows.Sort((a, b) => a.X != b.X ? a.X.CompareTo(b.X) : a.Y.CompareTo(b.Y));
                    foreach (var (x, y, c) in rows)
                    {
                        output.Append(x + " " + y + " " + c + "\n");
                    }
                    continue;
                }
                // それ以外は無視。
            }
            Console.Error.WriteLine("placed=" + units.Count);
            Console.Out.Write(output.ToString());
            return 0;
        }

        private static string Eval(Expression exp, string expr)
        {
            try
            {
                return exp.GetValueAsString(expr);
            }
            catch (Exception e)
            {
                return "<ERR:" + e.GetType().Name + ">";
            }
        }

        // headless 用の no-op GUIMap。SetMapSize が末尾で呼ぶため最小実装を渡す。
        private sealed class MockGUIMap : IGUIMap
        {
            public void InitMapSize(int w, int h) { }
            public void SetMapSize(int w, int h) { }
        }
    }
}
