using System;
using System.Collections.Generic;
using SRCCore;
using SRCCore.CmdDatas;
using SRCCore.Events;
using SRCCore.Expressions;
using SRCCore.Filesystem;
using SRCCore.TestLib;

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
    }
}
