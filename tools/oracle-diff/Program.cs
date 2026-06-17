using System;
using SRCCore;
using SRCCore.Expressions;
using SRCCore.TestLib;

namespace OracleDiff
{
    /// <summary>
    /// 原典 SRCCore の Expression エンジンを standalone 駆動する差分オラクル。
    /// 標準入力から式を 1 行ずつ読み、`GetValueAsString` で評価して結果を出力する。
    /// 空行・`#` 始まりはスキップ。評価例外は `&lt;ERR:型名&gt;` として出力。
    /// </summary>
    internal static class Program
    {
        private static int Main(string[] args)
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
                string result;
                try
                {
                    result = exp.GetValueAsString(line);
                }
                catch (Exception e)
                {
                    result = "<ERR:" + e.GetType().Name + ">";
                }
                Console.WriteLine(result);
            }
            return 0;
        }
    }
}
