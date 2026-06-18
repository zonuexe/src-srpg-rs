//! `.eve` コマンドの単一カタログ / Single source of truth for `.eve` commands.
//!
//! 「パーサに追加したが executor に到達しない」「executor を no-op にしたが
//! 構文受理する」「typo で silent に落ちる」といった配線不足を検出するため、
//! 全コマンドを 1 箇所で宣言する。
//!
//! - **Implemented**: `event_runtime::exec_command_pc` に対応 match arm がある。
//! - **Stub**: 構文受理のみ (VB6 互換の no-op)。dispatcher が即 `Ok(pc+1)` で返す。
//! - **ControlFlow**: 制御フロー (If/For/Goto 等)。dispatcher 前段で処理。
//!
//! このカタログは VB6 `Event.bas::CmdData` および SRC-Sharp `SRC.Sharp.Event` の
//! コマンド集合と比較するためのインデックスとしても使う (#4 — VB6 coverage)。

/// コマンドの実装ステータス。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandKind {
    /// `exec_command_pc` の match に対応 arm がある実装済コマンド。
    Implemented,
    /// 構文受理のみ。実機能は未実装 (VB6 互換のため受理しないとシナリオが
    /// 全停止するもの)。
    Stub,
    /// 制御フロー (`Goto` / `If` / `For` 等)。dispatcher 前段で特別扱い。
    ControlFlow,
}

/// カタログ 1 エントリ。
#[derive(Debug, Clone, Copy)]
pub struct CommandSpec {
    /// 表示用の標準綴り (なるべく CamelCase で VB6 / SRC.Sharp に合わせる)。
    pub name: &'static str,
    /// 追加で受理する小文字 alias。`name` 自体の lowercase は自動で含まれる
    /// ので、ここには `gameclear` ↔ `win` のような別綴りだけ書く。
    pub aliases: &'static [&'static str],
    pub kind: CommandKind,
    /// 1 行の説明。VB6 リファレンスに対応する概要を書く。
    pub summary: &'static str,
}

impl CommandSpec {
    /// `name` (case-insensitive) がこのエントリに一致するか。
    pub fn matches(&self, name: &str) -> bool {
        if name.eq_ignore_ascii_case(self.name) {
            return true;
        }
        for a in self.aliases {
            if name.eq_ignore_ascii_case(a) {
                return true;
            }
        }
        false
    }
}

/// 全カタログ。**新規コマンド追加時は必ずここにも 1 行加える**。
///
/// 順序は VB6 `Event.bas` のおおよそのカテゴリ順に揃える。
pub const COMMAND_CATALOG: &[CommandSpec] = &[
    // ---- 制御フロー -----------------------------------------------------
    sp("Goto", &[], CommandKind::ControlFlow, "ラベルへジャンプ"),
    sp(
        "If",
        &[],
        CommandKind::ControlFlow,
        "条件分岐 (Then/EndIf/ElseIf/Else)",
    ),
    sp("ElseIf", &[], CommandKind::ControlFlow, "If の追加条件"),
    sp("Else", &[], CommandKind::ControlFlow, "If の else 節"),
    sp("EndIf", &[], CommandKind::ControlFlow, "If 終端"),
    sp("Switch", &[], CommandKind::ControlFlow, "値による分岐"),
    sp("Case", &[], CommandKind::ControlFlow, "Switch の case"),
    sp(
        "CaseElse",
        &[],
        CommandKind::ControlFlow,
        "Switch の default",
    ),
    sp("EndSw", &[], CommandKind::ControlFlow, "Switch 終端"),
    sp(
        "For",
        &[],
        CommandKind::ControlFlow,
        "For var = start To end [Step n]",
    ),
    sp("Next", &[], CommandKind::ControlFlow, "For 終端"),
    sp(
        "ForEach",
        &[],
        CommandKind::ControlFlow,
        "リスト/連想配列の反復",
    ),
    sp("Do", &[], CommandKind::ControlFlow, "Do-Loop ループ開始"),
    sp(
        "Loop",
        &[],
        CommandKind::ControlFlow,
        "Do-Loop ループ終端 (While/Until 可)",
    ),
    sp("Break", &[], CommandKind::ControlFlow, "ループ脱出"),
    sp("Continue", &[], CommandKind::ControlFlow, "ループ次反復"),
    sp("Skip", &[], CommandKind::ControlFlow, "n 命令スキップ"),
    sp(
        "Call",
        &[],
        CommandKind::ControlFlow,
        "ラベルをサブルーチン呼出",
    ),
    sp("Return", &[], CommandKind::ControlFlow, "Call から復帰"),
    sp("Exit", &[], CommandKind::ControlFlow, "イベント即終了"),
    sp(
        "End",
        &[],
        CommandKind::ControlFlow,
        "Talk/Menu 等のブロック終端",
    ),
    // ---- 変数 -----------------------------------------------------------
    sp("Set", &[], CommandKind::Implemented, "変数代入"),
    sp("Local", &[], CommandKind::Implemented, "Set の別名"),
    sp("Unset", &[], CommandKind::Implemented, "変数削除"),
    sp(
        "Incr",
        &[],
        CommandKind::Implemented,
        "変数を加算 (1 か delta)",
    ),
    sp("Print", &[], CommandKind::Implemented, "デバッグ出力"),
    // ---- ステージ / シーン制御 ------------------------------------------
    sp("Stage", &[], CommandKind::Implemented, "現ステージ名を設定"),
    sp(
        "Briefing",
        &[],
        CommandKind::Implemented,
        "簡易説明文を設定",
    ),
    sp("Start", &[], CommandKind::Implemented, "Battle へ即遷移"),
    sp("Win", &[], CommandKind::Implemented, "勝利確定"),
    sp("Lose", &[], CommandKind::Implemented, "敗北確定"),
    sp("GameClear", &[], CommandKind::Implemented, "Win と同等"),
    sp("GameOver", &[], CommandKind::Implemented, "Lose と同等"),
    sp("Finish", &[], CommandKind::Implemented, "ステージ終了"),
    sp(
        "Telop",
        &["displaymessage"],
        CommandKind::Implemented,
        "テロップ表示 (messages に push)",
    ),
    // ---- データ宣言 -----------------------------------------------------
    sp("MapSize", &[], CommandKind::Implemented, "マップサイズ宣言"),
    sp(
        "SetTile",
        &[],
        CommandKind::Implemented,
        "1 タイルの terrain_id 設定",
    ),
    sp("Pilot", &[], CommandKind::Implemented, "PilotData 追加"),
    sp("Unit", &[], CommandKind::Implemented, "UnitData 追加"),
    sp(
        "Weapon",
        &[],
        CommandKind::Implemented,
        "UnitData に武器追加",
    ),
    sp("Place", &[], CommandKind::Implemented, "ユニット配置"),
    sp("Turn", &[], CommandKind::Implemented, "現ターン数設定"),
    sp(
        "Message",
        &[],
        CommandKind::Implemented,
        "メッセージログ追加",
    ),
    sp("Money", &[], CommandKind::Implemented, "所持金操作"),
    // ---- ユニット ライフサイクル ----------------------------------------
    sp("Create", &[], CommandKind::Implemented, "ユニット新規生成"),
    sp("RemoveUnit", &[], CommandKind::Implemented, "ユニット削除"),
    sp(
        "RemovePilot",
        &[],
        CommandKind::Implemented,
        "パイロット定義削除",
    ),
    sp(
        "MoveUnit",
        &["movunit"],
        CommandKind::Implemented,
        "ユニット位置変更",
    ),
    sp(
        "Launch",
        &[],
        CommandKind::Implemented,
        "オフマップから再投入",
    ),
    sp("Escape", &[], CommandKind::Implemented, "オフマップへ退避"),
    sp("Damage", &[], CommandKind::Implemented, "HP ダメージ"),
    sp("Heal", &[], CommandKind::Implemented, "HP 回復"),
    sp(
        "Kill",
        &["destroy"],
        CommandKind::Implemented,
        "ユニット撃破",
    ),
    sp("Transform", &[], CommandKind::Implemented, "機体変形"),
    sp("Combine", &[], CommandKind::Implemented, "合体"),
    sp("Split", &[], CommandKind::Implemented, "分離"),
    sp("ChangeParty", &[], CommandKind::Implemented, "陣営変更"),
    sp("Join", &[], CommandKind::Implemented, "ユニット参戦"),
    sp("Ride", &[], CommandKind::Implemented, "パイロット乗換"),
    sp(
        "Getoff",
        &[],
        CommandKind::Implemented,
        "降機 (pilot_name クリア)",
    ),
    sp(
        "Leave",
        &[],
        CommandKind::Implemented,
        "戦線離脱 (off_map=true)",
    ),
    sp(
        "ReplacePilot",
        &[],
        CommandKind::Implemented,
        "パイロット差替",
    ),
    sp(
        "SetRelation",
        &[],
        CommandKind::Implemented,
        "パイロット間好感度設定 (__rel_x_y 変数)",
    ),
    // ---- ステータス / SP / 育成 -----------------------------------------
    sp("SetStatus", &[], CommandKind::Implemented, "状態異常付与"),
    sp("UnsetStatus", &[], CommandKind::Implemented, "状態異常解除"),
    sp("SetSkill", &[], CommandKind::Implemented, "特殊能力付与"),
    sp(
        "SpecialPower",
        &[],
        CommandKind::Implemented,
        "精神コマンド発動",
    ),
    sp("IncreaseMorale", &[], CommandKind::Implemented, "士気増加"),
    sp("ExpUp", &[], CommandKind::Implemented, "経験値加算"),
    sp("LevelUp", &[], CommandKind::Implemented, "レベルアップ"),
    sp("RecoverHP", &[], CommandKind::Implemented, "HP 回復"),
    sp("RecoverEN", &[], CommandKind::Implemented, "EN 回復"),
    sp(
        "RecoverSP",
        &["recoverplana"],
        CommandKind::Implemented,
        "SP 回復",
    ),
    sp("Supply", &[], CommandKind::Implemented, "補給"),
    sp("Fix", &[], CommandKind::Implemented, "修理"),
    sp("Upgrade", &[], CommandKind::Implemented, "改造"),
    sp("RankUp", &[], CommandKind::Implemented, "階級アップ"),
    sp("SetBullet", &[], CommandKind::Implemented, "弾数設定"),
    // ---- アイテム -------------------------------------------------------
    sp(
        "Item",
        &["equip"],
        CommandKind::Implemented,
        "アイテム作成/装備 (1引数:在庫追加, 2引数:ユニット装備)",
    ),
    sp(
        "RemoveItem",
        &["unequip"],
        CommandKind::Implemented,
        "アイテム除去",
    ),
    sp(
        "ExchangeItem",
        &[],
        CommandKind::Implemented,
        "アイテム交換",
    ),
    // ---- セーブ / ロード ------------------------------------------------
    sp("SaveData", &[], CommandKind::Implemented, "セーブ"),
    sp("Load", &[], CommandKind::Implemented, "ロード"),
    sp("Forget", &[], CommandKind::Implemented, "永続変数削除"),
    sp(
        "Restore",
        &["restoreevent"],
        CommandKind::Implemented,
        "イベント再活性化",
    ),
    sp("Disable", &[], CommandKind::Implemented, "イベント無効化"),
    sp("Enable", &[], CommandKind::Implemented, "イベント有効化"),
    // ---- 対話 -----------------------------------------------------------
    sp(
        "Talk",
        &[],
        CommandKind::Implemented,
        "話者付きメッセージ表示",
    ),
    sp("Confirm", &[], CommandKind::Implemented, "Yes/No 質問"),
    sp(
        "Menu",
        &["ask"],
        CommandKind::Implemented,
        "番号選択メニュー",
    ),
    sp("Input", &[], CommandKind::Implemented, "テキスト入力"),
    // ---- 描画 -----------------------------------------------------------
    sp("PaintString", &[], CommandKind::Implemented, "文字列描画"),
    sp(
        "PaintStringR",
        &[],
        CommandKind::Implemented,
        "右寄せ文字列描画",
    ),
    sp(
        "PaintSysString",
        &[],
        CommandKind::Implemented,
        "システム文字列描画",
    ),
    sp("PaintPicture", &[], CommandKind::Implemented, "画像描画"),
    sp("Line", &[], CommandKind::Implemented, "直線/箱描画"),
    sp("PSet", &[], CommandKind::Implemented, "1 ピクセル描画"),
    sp("Color", &[], CommandKind::Implemented, "描画色設定"),
    sp("FillColor", &[], CommandKind::Implemented, "塗りつぶし色"),
    sp("DrawWidth", &[], CommandKind::Implemented, "線太さ"),
    sp("Font", &[], CommandKind::Implemented, "フォント設定"),
    sp("FadeIn", &[], CommandKind::Implemented, "フェードイン"),
    sp("FadeOut", &[], CommandKind::Implemented, "フェードアウト"),
    sp(
        "Refresh",
        &[],
        CommandKind::Implemented,
        "描画コマンドのクリア",
    ),
    sp("ClearPicture", &[], CommandKind::Implemented, "画像クリア"),
    sp(
        "ClearObj",
        &[],
        CommandKind::Implemented,
        "オブジェクトクリア",
    ),
    sp("Cls", &[], CommandKind::Implemented, "画面クリア"),
    sp(
        "Hotpoint",
        &[],
        CommandKind::Implemented,
        "クリック領域登録",
    ),
    sp(
        "HotpointString",
        &[],
        CommandKind::Implemented,
        "文字 + クリック領域",
    ),
    // ---- マップ / 戦闘 --------------------------------------------------
    sp("MapAttack", &[], CommandKind::Implemented, "マップ攻撃"),
    sp(
        "MapAbility",
        &[],
        CommandKind::Implemented,
        "マップ特殊能力",
    ),
    sp("ChangeMap", &[], CommandKind::Implemented, "マップ差替"),
    // ---- 配列 / その他 --------------------------------------------------
    sp("Sort", &[], CommandKind::Implemented, "配列ソート"),
    sp(
        "Wait",
        &[],
        CommandKind::Implemented,
        "時間待機 (秒) / Wait Click 等",
    ),
    sp("Show", &[], CommandKind::Implemented, "オブジェクト表示"),
    // ---- 音声 -----------------------------------------------------------
    sp("Startbgm", &[], CommandKind::Implemented, "BGM 再生開始"),
    sp("Stopbgm", &[], CommandKind::Implemented, "BGM 停止"),
    sp("Keepbgm", &[], CommandKind::Implemented, "BGM 継続"),
    sp("Playsound", &[], CommandKind::Implemented, "効果音再生"),
    sp("PlayVoice", &[], CommandKind::Implemented, "ボイス再生"),
    sp(
        "IntermissionCommand",
        &[],
        CommandKind::Implemented,
        "中間メニュー項目登録",
    ),
    // ---- 実装済み (元 Stub から昇格) --------------------------------------
    sp("ShowImage", &[], CommandKind::Implemented, "画像表示"),
    sp(
        "ShowUnitStatus",
        &[],
        CommandKind::Implemented,
        "ユニット詳細表示",
    ),
    sp(
        "SetMessage",
        &[],
        CommandKind::Implemented,
        "メッセージ設定",
    ),
    sp(
        "SetStock",
        &[],
        CommandKind::Implemented,
        "アビリティ残使用回数変更",
    ),
    sp("UseAbility", &[], CommandKind::Implemented, "特殊能力使用"),
    sp("StopSummoning", &[], CommandKind::Implemented, "召喚停止"),
    sp(
        "Hide",
        &[],
        CommandKind::Implemented,
        "メインウィンドウ非表示",
    ),
    sp("ChangeTerrain", &[], CommandKind::Implemented, "地形変更"),
    sp("Option", &[], CommandKind::Implemented, "オプション処理"),
    sp("Organize", &[], CommandKind::Implemented, "編成"),
    // 描画 / 効果系
    sp("FillStyle", &[], CommandKind::Implemented, "塗りつぶし様式"),
    sp("Background", &[], CommandKind::Implemented, "背景"),
    sp("Effect", &[], CommandKind::Implemented, "視覚効果"),
    sp("Sepia", &[], CommandKind::Implemented, "セピア"),
    sp("Monotone", &[], CommandKind::Implemented, "モノトーン"),
    sp("WhiteIn", &[], CommandKind::Implemented, "ホワイトイン"),
    sp("WhiteOut", &[], CommandKind::Implemented, "ホワイトアウト"),
    sp(
        "DrawOption",
        &[],
        CommandKind::Implemented,
        "描画オプション",
    ),
    sp(
        "SetStatusStringColor",
        &[],
        CommandKind::Implemented,
        "ステータス文字色",
    ),
    sp(
        "SetWindowColor",
        &[],
        CommandKind::Implemented,
        "ウィンドウ色",
    ),
    sp(
        "SetWindowFrameWidth",
        &[],
        CommandKind::Implemented,
        "ウィンドウ枠太さ",
    ),
    sp("PlayFlash", &[], CommandKind::Implemented, "Flash 再生"),
    sp("ClearFlash", &[], CommandKind::Implemented, "Flash クリア"),
    sp("StopFlash", &[], CommandKind::Implemented, "Flash 停止"),
    sp("RenameTitle", &[], CommandKind::Implemented, "タイトル改名"),
    sp("PlayMidi", &[], CommandKind::Implemented, "MIDI 再生"),
    sp("RenameTerm", &[], CommandKind::Implemented, "用語改名"),
    sp("RenameBgm", &[], CommandKind::Implemented, "BGM 改名"),
    sp(
        "FreeMemory",
        &[],
        CommandKind::Implemented,
        "メモリ開放 (no-op)",
    ),
    sp(
        "CreateFolder",
        &[],
        CommandKind::Implemented,
        "フォルダ作成 (VFS)",
    ),
    sp(
        "RemoveFolder",
        &[],
        CommandKind::Implemented,
        "フォルダ削除 (VFS)",
    ),
    sp(
        "RemoveFile",
        &[],
        CommandKind::Implemented,
        "ファイル削除 (VFS)",
    ),
    sp(
        "RenameFile",
        &[],
        CommandKind::Implemented,
        "ファイル改名 (VFS)",
    ),
    sp(
        "CopyFile",
        &[],
        CommandKind::Implemented,
        "ファイル複写 (VFS)",
    ),
    sp("Open", &[], CommandKind::Implemented, "ファイルオープン"),
    sp("Read", &[], CommandKind::Implemented, "ファイル読み込み"),
    sp("Write", &[], CommandKind::Implemented, "ファイル書き込み"),
    sp("LineRead", &[], CommandKind::Implemented, "1 行読み込み"),
    sp(
        "Global",
        &[],
        CommandKind::Implemented,
        "グローバル変数宣言",
    ),
    sp(
        "UpVar",
        &[],
        CommandKind::Implemented,
        "上位スコープ変数参照",
    ),
    sp("Require", &[], CommandKind::Implemented, "他 .eve 取込"),
    sp("Exec", &[], CommandKind::Implemented, "外部実行 (no-op)"),
    sp("Debug", &[], CommandKind::Implemented, "デバッグ出力"),
    sp("Quit", &[], CommandKind::Implemented, "終了 (Suspend 相当)"),
    sp(
        "Suspend",
        &[],
        CommandKind::Implemented,
        "中断 (タイトル復帰)",
    ),
    sp("Explode", &[], CommandKind::Implemented, "爆発エフェクト"),
    sp("Make", &[], CommandKind::Implemented, "汎用生成"),
    sp(
        "MakePilotList",
        &[],
        CommandKind::Implemented,
        "パイロット一覧生成",
    ),
    sp(
        "MakeUnitList",
        &[],
        CommandKind::Implemented,
        "ユニット一覧生成",
    ),
    sp("CopyArray", &[], CommandKind::Implemented, "配列複写"),
    sp("Swap", &[], CommandKind::Implemented, "値交換"),
    sp("DecreaseMorale", &[], CommandKind::Implemented, "士気減少"),
    sp(
        "Sunset",
        &[],
        CommandKind::Implemented,
        "夕方化 (状態フラグ)",
    ),
    sp("Noon", &[], CommandKind::Implemented, "昼化 (状態フラグ)"),
    sp("Night", &[], CommandKind::Implemented, "夜化 (状態フラグ)"),
    sp(
        "Center",
        &[],
        CommandKind::Implemented,
        "スクロール中心設定",
    ),
    sp(
        "ChangeUnitBitmap",
        &[],
        CommandKind::Implemented,
        "ユニット画像差替",
    ),
    sp(
        "ChangePilotBitmap",
        &[],
        CommandKind::Implemented,
        "パイロット画像差替",
    ),
    sp(
        "ChangeUnitClass",
        &[],
        CommandKind::Implemented,
        "ユニット分類差替",
    ),
    sp(
        "Pause",
        &[],
        CommandKind::Implemented,
        "一時停止 (Wait 相当)",
    ),
    sp("Close", &[], CommandKind::Implemented, "ファイルクローズ"),
    sp(
        "PlayMovie",
        &[],
        CommandKind::Implemented,
        "ムービー再生 (no-op)",
    ),
    sp("Redraw", &[], CommandKind::Implemented, "マップ再描画"),
    sp("Water", &[], CommandKind::Implemented, "水中状態切替"),
    sp("ColorFilter", &[], CommandKind::Implemented, "色フィルタ"),
    sp("SetBackground", &[], CommandKind::Implemented, "背景設定"),
    sp(
        "SaveScreen",
        &[],
        CommandKind::Implemented,
        "画面保存 (no-op)",
    ),
    sp(
        "LoadScreen",
        &[],
        CommandKind::Implemented,
        "画面読込 (no-op)",
    ),
    // ---- SRC.Sharp 由来の追補 ---------------------------------------------
    sp(
        "ABGM",
        &[],
        CommandKind::Stub,
        "自動 BGM 切替 (auto-prefix)",
    ),
    sp("AIf", &[], CommandKind::Stub, "自動分岐 (auto-prefix If)"),
    sp("ATalk", &[], CommandKind::Stub, "自動 Talk (auto-prefix)"),
    sp("Arc", &[], CommandKind::Implemented, "弧描画 (Graphics)"),
    sp("Array", &[], CommandKind::Implemented, "配列宣言"),
    sp("Attack", &[], CommandKind::Implemented, "ユニット攻撃指示"),
    sp(
        "AutoTalk",
        &[],
        CommandKind::Implemented,
        "隣接ユニット自動会話発火",
    ),
    sp("BossRank", &[], CommandKind::Implemented, "ボス階級設定"),
    sp(
        "CallIntermissionCommand",
        &[],
        CommandKind::Implemented,
        "中間メニューコマンド呼出",
    ),
    sp(
        "Cancel",
        &[],
        CommandKind::Implemented,
        "現操作のキャンセル",
    ),
    sp("ChangeArea", &[], CommandKind::Implemented, "エリア切替"),
    sp("ChangeLayer", &[], CommandKind::Stub, "レイヤー切替"),
    sp("ChangeMode", &[], CommandKind::Implemented, "モード切替"),
    sp(
        "Charge",
        &[],
        CommandKind::Implemented,
        "チャージフラグ設定",
    ),
    sp("Circle", &[], CommandKind::Implemented, "円描画 (Graphics)"),
    sp(
        "ClearEvent",
        &[],
        CommandKind::Implemented,
        "イベント定義削除",
    ),
    sp("ClearImage", &[], CommandKind::Stub, "画像クリア"),
    sp("ClearLayer", &[], CommandKind::Stub, "レイヤークリア"),
    sp(
        "ClearSkill",
        &[],
        CommandKind::Implemented,
        "特殊能力クリア",
    ),
    sp(
        "ClearSpecialPower",
        &[],
        CommandKind::Implemented,
        "精神コマンドクリア",
    ),
    sp(
        "ClearStatus",
        &[],
        CommandKind::Implemented,
        "状態異常クリア",
    ),
    sp("Land", &[], CommandKind::Implemented, "着地命令"),
    sp("Move", &[], CommandKind::Implemented, "ユニット移動"),
    sp("Nop", &[], CommandKind::Stub, "No operation"),
    sp(
        "NotImplemented",
        &[],
        CommandKind::Stub,
        "SRC.Sharp 未実装マーカ",
    ),
    sp(
        "NotSupported",
        &[],
        CommandKind::Stub,
        "SRC.Sharp 非対応マーカ",
    ),
    sp("Oval", &[], CommandKind::Implemented, "楕円描画 (Graphics)"),
    sp(
        "Polygon",
        &[],
        CommandKind::Implemented,
        "多角形描画 (Graphics)",
    ),
    sp(
        "Question",
        &[],
        CommandKind::Implemented,
        "制限時間付き選択ダイアログ",
    ),
    sp("QuickLoad", &[], CommandKind::Implemented, "クイックロード"),
    sp("Release", &[], CommandKind::Implemented, "Fix 固定解除"),
    sp("Select", &[], CommandKind::Implemented, "選択入力"),
    sp(
        "SelectTarget",
        &[],
        CommandKind::Implemented,
        "対象選択 (戦闘 UI)",
    ),
    // 実シナリオ scan で検出された SRC 拡張命令 (Stub)
    sp(
        "SpecialPowerAnime",
        &[],
        CommandKind::Stub,
        "精神コマンド発動アニメ",
    ),
    // ---- 実アーカイブスキャン (2026-06) 追補 --------------------------------
    // runtime に実装済みのコマンドを catalog にも登録 (scan_eve ノイズ除去)。
    sp(
        "Mind",
        &[],
        CommandKind::Implemented,
        "精神コマンド強制発動 (SP消費なし)",
    ),
    sp(
        "ClearMind",
        &[],
        CommandKind::Implemented,
        "精神コマンド解除",
    ),
    sp(
        "SetAbility",
        &[],
        CommandKind::Implemented,
        "特殊能力付与 (SetSkill 旧名)",
    ),
    sp(
        "ClearAbility",
        &[],
        CommandKind::Implemented,
        "特殊能力削除 (ClearSkill 旧名)",
    ),
    // 描画・演出系: 現状は視覚効果なしの Stub として登録しノイズを消す。
    sp("Display", &[], CommandKind::Stub, "キャラクタ表示"),
    sp(
        "ShowCharacter",
        &[],
        CommandKind::Stub,
        "キャラクタ画像表示",
    ),
    sp("PlayEffect", &[], CommandKind::Stub, "エフェクト再生"),
    sp("AttackDemo", &[], CommandKind::Stub, "攻撃デモ再生"),
    sp("MindAnime", &[], CommandKind::Stub, "精神コマンドアニメ"),
    sp("ETalkL", &[], CommandKind::Stub, "立ち絵付き左会話"),
    sp("ETalkR", &[], CommandKind::Stub, "立ち絵付き右会話"),
    sp("ETalkEnd", &[], CommandKind::Stub, "立ち絵会話終了"),
    sp(
        "String",
        &[],
        CommandKind::Stub,
        "文字列変数宣言 (Local と同義)",
    ),
    sp(
        "MapWeapon",
        &[],
        CommandKind::Implemented,
        "マップ攻撃 (MapAttack の旧名称)",
    ),
    sp(
        "OnMapItemChanger",
        &[],
        CommandKind::Stub,
        "マップ上アイテム変換",
    ),
    sp("StratBGM", &[], CommandKind::Stub, "戦略マップ BGM 設定"),
    sp("NonPilotInfo", &[], CommandKind::Stub, "非戦闘員情報表示"),
    sp("PPaintString", &[], CommandKind::Stub, "位置指定文字列描画"),
    sp(
        "BBSR_TypeWrite",
        &[],
        CommandKind::Stub,
        "タイプライター演出",
    ),
    sp(
        "VisualMapInitialize",
        &[],
        CommandKind::Stub,
        "ビジュアルマップ初期化",
    ),
    sp(
        "VisualMapDrawButtons",
        &[],
        CommandKind::Stub,
        "ビジュアルマップボタン描画",
    ),
    sp(
        "VisualMapSetItem",
        &[],
        CommandKind::Stub,
        "ビジュアルマップアイテム設定",
    ),
    sp(
        "ITW_ClearUnknown",
        &[],
        CommandKind::Stub,
        "ITW ライブラリ: 未知情報クリア",
    ),
    sp("EquipShuraMode", &[], CommandKind::Stub, "修羅モード装備"),
    sp("EquipHardMode", &[], CommandKind::Stub, "ハードモード装備"),
    sp(
        "DU_LoadUnitName1",
        &[],
        CommandKind::Stub,
        "ユニット開発 lib: 名前ロード1",
    ),
    sp(
        "DU_LoadUnitName2",
        &[],
        CommandKind::Stub,
        "ユニット開発 lib: 名前ロード2",
    ),
    sp(
        "DU_ItemDescription",
        &[],
        CommandKind::Stub,
        "ユニット開発 lib: アイテム説明",
    ),
    sp(
        "DU_OptionSet",
        &[],
        CommandKind::Stub,
        "ユニット開発 lib: オプション設定",
    ),
];

/// const コンストラクタ (記述短縮)。
const fn sp(
    name: &'static str,
    aliases: &'static [&'static str],
    kind: CommandKind,
    summary: &'static str,
) -> CommandSpec {
    CommandSpec {
        name,
        aliases,
        kind,
        summary,
    }
}

/// `name` を case-insensitive で検索する。見つからなければ `None`。
pub fn lookup(name: &str) -> Option<&'static CommandSpec> {
    COMMAND_CATALOG.iter().find(|s| s.matches(name))
}

/// 「dispatcher の match で受理されなかった」コマンドのフォールバック処理。
///
/// - Stub なら何もせず Ok を返す (受理のみ)。
/// - Implemented なら「match arm が無い (実装漏れ)」として警告。
/// - カタログに無いなら「未登録コマンド (typo か新規)」として警告。
///
/// いずれもエラーにはしない。シナリオ進行は止めない。
///
/// # VB6 原典との対応
///
/// `SRC_20121125/CmdData.cls::Parse` の `Case Else` (line 524-580) では、
/// 未知の name は **すべて自動的に `CallCmd` に書き換え** られ、ラベルが
/// 未定義であっても `ArgsType = UndefinedType` で記録するだけで parse は
/// 成功扱いとなる (実行時に silent skip)。 fuzzy match や typo 補正は
/// 原典にも無いので本実装でも導入しない。`Retunr` のような typo は
/// 警告ログを出すだけで silent OK (= VB6 と挙動一致) で済ませる。
pub fn handle_unrecognized(name: &str, line: usize) {
    match lookup(name) {
        Some(spec) if spec.kind == CommandKind::Stub => {
            // 想定どおりの受理: 黙って通す。
        }
        Some(spec) => {
            log::warn!(
                "[command-catalog] {} 行目: 「{}」はカタログ上 {:?} だが \
                 dispatcher match に届いていません (実装漏れの可能性)。",
                line,
                spec.name,
                spec.kind,
            );
        }
        None => {
            log::warn!(
                "[command-catalog] {} 行目: 「{}」はカタログ未登録。\
                 typo か新規コマンドの可能性 — 必要なら crate::command_catalog \
                 に追加してください。",
                line,
                name,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn no_duplicate_names_or_aliases() {
        let mut seen: HashSet<String> = HashSet::new();
        for spec in COMMAND_CATALOG {
            let key = spec.name.to_ascii_lowercase();
            assert!(
                seen.insert(key.clone()),
                "重複したコマンド名: {}",
                spec.name
            );
            for a in spec.aliases {
                let akey = a.to_ascii_lowercase();
                assert!(
                    seen.insert(akey.clone()),
                    "{} の alias {} が他と衝突",
                    spec.name,
                    a
                );
            }
        }
    }

    #[test]
    fn lookup_is_case_insensitive() {
        assert!(lookup("Stage").is_some());
        assert!(lookup("STAGE").is_some());
        assert!(lookup("stage").is_some());
        assert!(lookup("StAgE").is_some());
        assert!(lookup("nonexistent_command_xyz").is_none());
    }

    #[test]
    fn alias_lookup_resolves_to_canonical() {
        // `gameclear` is an alias of `Win`? No — they're separate entries.
        // Confirm `restoreevent` resolves to Restore.
        let spec = lookup("restoreevent").expect("restoreevent should resolve");
        assert_eq!(spec.name, "Restore");
        let spec = lookup("destroy").expect("destroy should resolve");
        assert_eq!(spec.name, "Kill");
    }

    #[test]
    fn control_flow_and_implemented_cover_basic_set() {
        // よく使う命令が漏れていないかの sanity check。
        for must in [
            "Goto",
            "If",
            "EndIf",
            "Set",
            "Stage",
            "Message",
            "Talk",
            "Confirm",
            "Menu",
            "Input",
            "Damage",
            "Heal",
            "Place",
            "Pilot",
            "Unit",
            "Weapon",
            "MapAttack",
            "Wait",
            "Startbgm",
        ] {
            let spec = lookup(must).unwrap_or_else(|| panic!("{must} missing from catalog"));
            assert_ne!(
                spec.kind,
                CommandKind::Stub,
                "{must} は Stub であってはならない"
            );
        }
    }

    #[test]
    fn known_stubs_are_marked_stub() {
        // Nop / SpecialPowerAnime は未対応系の典型的な真の Stub。
        // FillStyle / PlayMovie は no-op arm あり → Implemented に昇格済み。
        // グラフィクス系 Circle/Oval/Polygon/Arc は描画 primitive 実装済み (下記参照)。
        for stub in ["Nop", "SpecialPowerAnime"] {
            let spec = lookup(stub).unwrap_or_else(|| panic!("{} missing", stub));
            assert_eq!(spec.kind, CommandKind::Stub, "{stub} は Stub のはず");
        }
    }

    #[test]
    fn graphics_primitives_are_implemented() {
        // 汎用戦闘アニメ Lib (GBA クローズアップ) が使う図形描画命令は実装済み。
        for must in ["Circle", "Oval", "Polygon", "Arc", "Line", "PSet"] {
            let spec = lookup(must).unwrap_or_else(|| panic!("{must} missing from catalog"));
            assert_eq!(
                spec.kind,
                CommandKind::Implemented,
                "{must} は Implemented のはず"
            );
        }
    }
}
