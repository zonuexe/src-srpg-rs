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
        // 地形は EmptyTerrain (HitMod=0/DamageMod=0/Class="" → StandBy で Area="地上") で中立化。
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
            var predicts = new List<string>();
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
                if (line.StartsWith("@predict "))
                {
                    predicts.Add(line);
                    continue;
                }
                // それ以外の行は本モードでは無視 (combat corpus は @unit + @predict のみ)。
            }
            Console.Error.WriteLine("created=" + created + " UList=" + src.UList.Count());

            foreach (var pr in predicts)
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
