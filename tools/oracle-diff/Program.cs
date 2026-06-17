using System;
using System.Collections.Generic;
using SRCCore;
using SRCCore.CmdDatas;
using SRCCore.Events;
using SRCCore.Expressions;
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
