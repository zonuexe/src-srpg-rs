//! アプリケーション状態 / Application state.
//!
//! VB5 原典は `frmTitle` → `frmMain` の Form 切替でアプリ全体の状態遷移を
//! 表現していた。Rust 移植では `App` 構造体に集約し、フロントエンドからは
//! 入力イベントを `handle_input` に流し、結果を `scene()` / `settings()` で
//! 参照する。
//!
//! In the original VB5 app, global state transitions happen via `Form` swaps
//! (`frmTitle` → `frmMain`). Here we collapse that into a single `App` value
//! that the frontend drives via `handle_input`.

use crate::combat;
use crate::scene::{configuration, map_view, Scene};
use crate::GameDatabase;
use crate::Settings;
use crate::Turn;
use crate::{CANVAS_HEIGHT, CANVAS_WIDTH};

/// 4 方向 / 4-direction enum used by `Input::MoveCursor`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// プラットフォーム非依存の入力イベント。
/// Platform-agnostic input event.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Input {
    /// 「次へ」操作: タイトルなど受動的なシーンを抜けるための汎用トリガ。
    Advance,
    /// マウスクリック / タップ。座標は canvas 論理ピクセル基準
    /// (0..`CANVAS_WIDTH`, 0..`CANVAS_HEIGHT`)。
    ClickAt { x: i32, y: i32 },
    /// 4 方向キーによるカーソル移動。MapView のみで意味を持つ。
    MoveCursor(Direction),
    /// 現フェーズを終了して次フェーズへ。MapView のみで意味を持つ。
    EndPhase,
    /// カーソル上の自勢力ユニットで隣接 / 射程内の最寄り敵対ユニットを攻撃。
    AttackTarget,
    /// カーソル上のユニットの装備武器を次へ循環。
    CycleWeapon,
    /// 対話 UI への応答: Yes / OK (Talk 進行)。
    DialogYes,
    /// 対話 UI への応答: No (Confirm 専用)。Talk では Yes 扱い。
    DialogNo,
    /// 対話 UI への応答: 数値 (Menu の 1-indexed 選択)。0 はキャンセル。
    DialogChoice(u32),
    /// MapView から PilotList へ遷移（戻りはもう一度押す or Advance）。
    GotoPilotList,
    /// MapView から UnitList へ遷移（同上）。
    GotoUnitList,
    /// MapView へ戻る（PilotList / UnitList から）。
    GotoMapView,
    /// キャンセル（右クリック / Esc）。コマンドメニュー / 行動モードを抜ける。
    Cancel,
    /// 右クリック位置。コマンドメニュー閉じ + 該当タイル上での選択。
    RightClickAt { x: i32, y: i32 },
}

/// 逐次 AI 実行ランナー。`animate_ai=true` の敵/中立/ＮＰＣ フェイズで、
/// `tick` が `step_timer` 間隔ごとに `queue` から 1 体取り出して行動させる。
/// SRC.cs `StartTurn` の CPU ループ (1 体ずつ RedrawScreen + 間) に相当。
#[derive(Debug, Clone, Default)]
struct AiRunner {
    /// このフェイズで未行動の AI ユニット uid (行動順)。
    queue: std::collections::VecDeque<String>,
    /// 次の 1 体を動かすまでの残り秒 (「間」の演出)。
    step_timer: f64,
}

/// 逐次 AI の 1 体ごとの「間」(秒)。
const AI_STEP_SECS: f64 = 0.45;

/// 反撃モード (SRC) の待機文脈。味方ユニットが AI に攻撃されたとき、プレイヤーの
/// 反撃手段選択 (反撃/回避/防御) を待つ間 `pending_reaction` に保持する。応答時に
/// `target_tile` の防御側へ選んだ `def_mode` で攻撃を解決する。
#[derive(Debug, Clone)]
struct PendingReaction {
    /// 攻撃側 (AI) ユニット uid。
    atk_uid: String,
    /// 防御側 (味方) の居るタイル。
    target_tile: (u32, u32),
    /// メニュー選択 index(1-based) → def_mode のマップ ("反撃"/"回避"/"防御")。
    modes: Vec<String>,
}

/// 武器選択を待つ間 `pending_weapon_select` に保持する。通常攻撃と、反撃手段
/// 「反撃」選択後の反撃武器選択の両方に使う (`counter` で区別)。
#[derive(Debug, Clone)]
struct PendingWeaponSelect {
    /// 武器を撃つ側 uid (通常攻撃=攻撃側 / 反撃=防御側=反撃する側)。
    atk_uid: String,
    /// 撃つ目標タイル (通常攻撃=防御側 / 反撃=元攻撃側の位置)。
    target: (u32, u32),
    /// 移動後攻撃か (キャンセル復帰用フラグ)。
    post_move: bool,
    /// `Some` なら反撃武器選択。選択後に主攻撃を `def_mode="反撃"` で解決する。
    counter: Option<CounterCtx>,
}

/// 反撃武器選択の文脈 (主攻撃の解決に必要)。
#[derive(Debug, Clone)]
struct CounterCtx {
    /// 主攻撃の攻撃側 (AI) uid。
    main_atk_uid: String,
    /// 主攻撃の防御側 (反撃する側) タイル。
    main_target: (u32, u32),
}

/// 精神コマンド (SP コマンド) のサブメニュー選択を待つ間 `pending_spirit` に保持。
/// メニュー選択 (1-based) → `commands[idx-1]` の (コマンド名, 消費SP) を発動する。
#[derive(Debug, Clone)]
struct PendingSpirit {
    /// 発動主体のユニット uid。
    uid: String,
    /// メニュー項目に対応する (コマンド名, 消費SP) の列 (表示順)。
    commands: Vec<(String, i32)>,
}

/// 変形（特殊能力 `変形`）の変形先サブメニュー選択を待つ間 `pending_transform`
/// に保持。メニュー選択 (1-based) → `forms[idx-1]` の形態へ変形する。
#[derive(Debug, Clone)]
struct PendingTransform {
    /// 変形するユニットの uid。
    uid: String,
    /// メニュー項目に対応する変形先フォーム名の列（表示順）。
    forms: Vec<String>,
}

/// アビリティ一覧サブメニュー選択を待つ間 `pending_ability` に保持。
/// メニュー選択 (1-based) → `entries[idx-1]` の (ability_idx, 使用可否) を解決する。
#[derive(Debug, Clone)]
struct PendingAbility {
    /// 発動主体のユニット uid。
    uid: String,
    /// メニュー項目に対応する (`UnitData.abilities` のインデックス, 使用可否)。
    entries: Vec<(usize, bool)>,
}

/// 発進サブメニュー (母艦の格納ユニット選択) を待つ間 `pending_launch` に保持。
/// メニュー選択 (1-based) → `stored[idx-1]` のユニットを出撃させる。
#[derive(Debug, Clone)]
struct PendingLaunch {
    /// 母艦ユニットの uid。
    carrier: String,
    /// 格納ユニットの uid 列 (表示順)。
    stored: Vec<String>,
}

/// インターミッション画面のサブモード。`Menu` はメインメニュー、`UnitUpgrade` は
/// 「機体改造」で資金を払って強化するユニットを選ぶリスト。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum IntermissionMode {
    /// メインメニュー (ユーザ定義項目 + 機体改造 / 換装 / データセーブ / 次のステージへ)。
    #[default]
    Menu,
    /// 機体改造の対象ユニット選択。
    UnitUpgrade,
    /// 換装の対象 (ユニット → 換装先) 選択。`換装` 特殊能力を持つ味方ユニットの
    /// 各換装先を平坦化したリスト。
    EquipSwap,
    /// 乗り換え。`ride_change_source` が None なら移動元ユニット選択、Some なら
    /// 移動先ユニット選択 (確定で 2 ユニットの搭乗パイロットを入れ替える)。
    RideChange,
}

/// インターミッションのメインメニュー項目 (表示順を index で解決するため列挙)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterItem {
    /// ユーザ定義 `IntermissionCommand` (intermission_commands への index)。
    User(usize),
    /// 組込み「機体改造」。
    Upgrade,
    /// 組込み「換装」。
    EquipSwap,
    /// 組込み「乗り換え」。
    RideChange,
    /// 組込み「ステータス」（部隊ロスター閲覧）。
    Status,
    /// 組込み「データセーブ」。
    Save,
    /// 「次のステージへ」。
    NextStage,
}

/// 精神コマンドの対象種別。sp.txt の `TargetType` (`自分`/`単体`/`全体`/`敵単体`)
/// を優先し、未定義なら組込みの名前→種別テーブルにフォールバックする。
/// 対象選択が要る種別 (`SingleAlly` / `SingleEnemy`) は `ActionMode::SpiritTarget`
/// へ遷移し、対象確定時に効果と SP 消費を行う (キャンセル時は SP を消費しない)。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SpiritTargetKind {
    /// 発動主体自身に作用 (必中 / 集中 / 覚醒 / 奇跡 等)。
    SelfOnly,
    /// 出撃中の味方全体に作用 (友情 / 愛)。
    AllAllies,
    /// 任意の味方単体に作用 (信頼 / 補給 / 祝福 / 応援 / 再動)。
    SingleAlly,
    /// 任意の敵単体に作用 (脱力)。
    SingleEnemy,
}

/// 上位ロジックの状態 / Top-level engine state.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct App {
    scene: Scene,
    settings: Settings,
    /// Configuration 入場時の Settings スナップショット。Cancel で巻き戻すため。
    settings_snapshot: Option<Settings>,
    database: GameDatabase,
    /// MapView でのカーソル位置（タイル単位）。`None` ならカーソル未配置。
    map_cursor: Option<(u32, u32)>,
    /// 現在のターン / フェーズ。
    turn: Turn,
    /// マップビューポートの左上タイル座標。scroll で動かす。
    map_scroll: (u32, u32),
    /// 元 `SRC.bas::Stage` — 現ステージの文字列名（イベント `Stage` 命令で設定）。
    stage: String,
    /// イベント `Message` 命令や戦闘結果でつもる HUD ログ。
    messages: Vec<String>,
    /// 戦闘ロール用 PRNG (splitmix64 状態)。
    rng_state: u64,
    /// 現在カーソル上のユニットで選択中の武器 index。
    /// 0 = 自動選択（射程内で最強）、1 以降 = `UnitData.weapons[i-1]` 固定。
    selected_weapon_idx: usize,
    /// シナリオ進行状態（Briefing/Sortie/Battle/Victory/Defeat）。
    stage_state: crate::stage::StageState,
    /// ステージ簡易説明（元 SRC `Briefing` 命令で設定）。
    briefing: String,
    /// `.eve` の `Set` / `Local` で代入されたシナリオ変数。
    /// 元 SRC では `Event.bas::SetVariable` 等で扱う動的シンボル表。
    script_vars: std::collections::BTreeMap<String, String>,
    /// 中断可能な実行コンテキスト。`Talk` / `Confirm` などで一時停止して
    /// `pending_dialog` を表示し、ユーザ応答後に `event_runtime::resume` で再開する。
    /// モーダル中のセーブを許容するため save/load 経路でも保存・復元する。
    #[serde(default)]
    script_ctx: Option<crate::event_runtime::ScriptContext>,
    /// 表示中の対話 UI（Talk / Confirm）。`None` ならモーダル無し。
    #[serde(default)]
    pending_dialog: Option<crate::dialog::PendingDialog>,
    /// `For` / `ForEach` ループのフレームスタック（ネスト対応）。
    #[serde(default)]
    script_for_stack: Vec<crate::event_runtime::LoopFrame>,
    /// `Call` のコールスタック。各フレームは (リターン PC, 呼び出し元の
    /// `Args(1..9)` スナップショット)。`Return` で Args を復元する。
    ///
    /// saved_args の layout: [Args(1)..Args(9), ArgNum, upvar_level, upvar_base_argnum]  (長さ 12)
    #[serde(default)]
    script_call_stack: Vec<(usize, Vec<String>)>,
    /// 各コールフレームで `Local` により宣言されたサブルーチンローカル変数の
    /// 復元情報。`script_call_stack` と必ず同じ深さで push/pop される。
    ///
    /// 要素は (変数名, 宣言前の値)。`None` は「宣言前は未定義」を表し、
    /// フレーム離脱時に変数ごと削除する。
    ///
    /// 原典は `VarStack` + `VarIndex` でサブルーチンローカル変数を管理し、
    /// `Return` で `VarIndex` を復帰させることで自動的に破棄する
    /// (`CmdData.cls::ExecLocalCmd` / `Expression.bas`)。本実装は
    /// `script_vars` 1 枚に載せる構造なので、宣言時に旧値を退避して
    /// フレーム離脱時に巻き戻すことで同じ可視性を得る。
    #[serde(default)]
    script_sublocal_stack: Vec<Vec<(String, Option<String>)>>,
    /// 現フレームで `UpVar` を呼び出した回数。`Call` 時に 0 へリセット、
    /// `Return` 時に呼び出し元の値へ復元する。
    #[serde(default)]
    upvar_level: usize,
    /// 現フレームで最初の `UpVar` を呼ぶ直前の ArgNum。`UpVar` を複数回
    /// 呼び出す際に何段目の祖先の引数を追加するか判断するために使う。
    #[serde(default)]
    upvar_base_argnum: usize,
    /// 全ロード済み .eve を集約したスクリプトライブラリ。
    /// `Start:` / `Turn N:` 等の自動発火に使用。
    #[serde(default)]
    script_library: crate::event_runtime::ScriptLibrary,
    /// `.eve` の `PaintString` / `Line` / `Font` 等で蓄積された描画コマンド列。
    /// `Refresh` 命令で `clear()` される。シーン描画の最後に上書きする。
    #[serde(default)]
    script_overlay: crate::script_overlay::ScriptOverlay,
    /// 所持金（元 SRC `Money`）。`Money n` / `Money +n` / `Money -n` で変動。
    #[serde(default)]
    money: i64,
    /// 表示中のコマンドメニュー（ユニット / マップ）。
    #[serde(default)]
    command_menu: Option<crate::command_menu::CommandMenu>,
    /// 現在の行動モード（Browse / MoveSelect / AttackSelect）。
    #[serde(default)]
    action_mode: crate::command_menu::ActionMode,
    /// `Hotpoint` 命令で登録されたクリック領域。`Wait Click` の応答や
    /// 描画ヒントに使う。`Refresh` でクリア。
    #[serde(default)]
    hotpoints: Vec<crate::event_runtime::HotpointEntry>,
    /// `Startbgm` / `Playsound` 等で積まれるオーディオリクエスト。
    /// フロントエンド側で `take_pending_audio()` を呼んで実再生する。
    #[serde(skip)]
    pending_audio: Vec<crate::audio::AudioRequest>,
    /// `Wait <duration>` で設定されるタイマ秒数。> 0 のあいだスクリプトは
    /// 中断し、`tick(dt)` で減算、0 以下になったら `resume()` で再開する。
    ///
    /// save/load を生き残る (`#[serde(default)]`)。Wait 中にユーザが save
    /// した場合、ロード後の現実時間で再カウントダウンを継続する。これを
    /// 落とすと `script_ctx` だけ復元されて pending_timer 無しの状態に
    /// なり、`tick` が resume を呼ばずスクリプトが永久停止する。
    #[serde(default)]
    pending_timer: Option<f64>,
    /// Talk メッセージ内の `:` (半角) による段階表示の残りページ。
    /// SRC `Talkコマンド.md`: `:` で区切った箇所はメッセージを一部ずつ段階的に
    /// 表示する。最初のページを `pending_dialog` の Talk に出し、残りをここに
    /// 積む。クリック/Enter 応答ごとに 1 ページずつ消費し、空になったら
    /// スクリプトを再開する。`talk_pages_speaker` は継続ページ共通の話者名。
    #[serde(default)]
    talk_pages: Vec<String>,
    #[serde(default)]
    talk_pages_speaker: String,
    /// `Wait Start` を基準としたスクリプト・タイムライン上の現在位置 (秒)。
    /// `Wait Until time` は基準時刻から `0.1×time` 秒経過までの待機なので、
    /// 直前の `Wait Start`/`Wait Until` からの増分だけを `pending_timer` に
    /// 積む。`Wait Start` で 0 にリセット。transient (save/load 不要)。
    #[serde(skip)]
    wait_clock: f64,
    /// 敵/中立/ＮＰＣ フェイズの逐次演出を有効にするか。`true` のとき `end_phase` は
    /// `ai_runner` を初期化し、`tick` が 1 体ずつ AI を実行する。`false`(既定/ヘッドレス)
    /// では従来どおり同期一括処理する。transient (フロントが起動時に true へ設定)。
    #[serde(skip)]
    animate_ai: bool,
    /// 再攻撃 (`attack_resolve_and_run` の再帰呼び出し) 中フラグ。transient。
    /// true の呼び出しは使用イベント再発火と 3回目の再攻撃を抑止する。
    #[serde(skip)]
    reattack_in_progress: bool,
    /// 逐次 AI 実行の進行状態 (animate_ai=true 時のみ)。transient。
    #[serde(skip)]
    ai_runner: Option<AiRunner>,
    /// ネイティブ戦闘演出を有効にするか。`true` のとき `attack_resolve_and_run` は
    /// 攻撃解決時に `battle_anim` を積み、`tick` が再生する。`false`(既定/ヘッドレス)
    /// では演出を生成しない (既存テスト互換)。transient (フロントが起動時に設定)。
    #[serde(skip)]
    animate_battle: bool,
    /// 進行中のネイティブ戦闘演出 (`animate_battle=true` 時のみ)。表示はフロント。
    /// transient。
    #[serde(skip)]
    battle_anim: Option<crate::battle_anim::BattleAnim>,
    /// 進行中の移動スライド演出 (`animate_ai=true` の逐次 AI 移動時のみ)。表示はフロント。
    /// 論理位置は移動先に即時更新済みで、これは表示位置の補間用。transient。
    #[serde(skip)]
    move_anim: Option<crate::battle_anim::MoveAnim>,
    /// SRC「自動反撃モード」。`true` で味方が攻撃されても反撃選択を出さず自動反撃。
    /// `false`(既定) で手動 (反撃/回避/防御 を選択)。マップコマンドで切替。transient。
    #[serde(skip)]
    auto_counter: bool,
    /// 反撃モード待機中の戦闘文脈 (味方が攻撃され選択待ちのとき Some)。transient。
    #[serde(skip)]
    pending_reaction: Option<PendingReaction>,
    /// 武器選択ウィンドウ (UI モード時の攻撃フロー)。`true`(既定) で目標確定後に
    /// 武器選択窓を出す。`false` で従来の自動 / W キー武器による即解決。transient。
    #[serde(skip)]
    weapon_window_enabled: bool,
    /// 武器選択待機中の攻撃文脈 (Some のとき武器選択待ち)。transient。
    #[serde(skip)]
    pending_weapon_select: Option<PendingWeaponSelect>,
    /// プレイヤーが反撃武器選択で選んだ反撃武器 (0-based)。`try_counterattack` が
    /// `take()` して 1 回だけ使い、無効/未設定なら自動選定にフォールバック。transient。
    #[serde(skip)]
    forced_counter_weapon: Option<usize>,
    /// 精神コマンドのサブメニュー選択待ち文脈 (Some のとき発動待ち)。transient。
    #[serde(skip)]
    pending_spirit: Option<PendingSpirit>,
    /// 変形先サブメニュー選択待ち文脈 (Some のとき変形先選択待ち)。transient。
    #[serde(skip)]
    pending_transform: Option<PendingTransform>,
    /// アビリティ一覧サブメニュー選択待ち文脈 (Some のとき選択待ち)。transient。
    #[serde(skip)]
    pending_ability: Option<PendingAbility>,
    /// 発進サブメニュー選択待ち文脈 (Some のとき格納ユニット選択待ち)。transient。
    #[serde(skip)]
    pending_launch: Option<PendingLaunch>,
    /// スクリプト (`Quickload` / `Restart` / ゲームオーバー コンティニュー) が要求した
    /// 再ロード用 JSON。`core` は `self` を置換できないため、フロントエンドが
    /// [`App::take_pending_reload`] で取り出し `from_save_json` で置換 + `fire_resume_event`
    /// する責務を持つ。transient。
    #[serde(skip)]
    pending_reload: Option<String>,
    /// `Now()` / `Year()` / `Month()` 等 SRC 時間関数の参照する Unix epoch
    /// ミリ秒 (UTC)。プラットフォーム依存の時計取得は src-web 側で
    /// `Date::now()` を呼んで毎フレーム更新する。テスト / src-core 単独
    /// 実行時は 0 (= 1970-01-01 00:00:00 UTC) のまま。
    #[serde(skip)]
    wall_clock_ms: f64,
    /// シナリオが `IntermissionCommand <name> <file>` で登録した
    /// インターミッションメニュー項目。SRC.Sharp の `Intermission` シーンで
    /// 表示される「キャラクターメイキング / 改造 / ショップ / ...」相当。
    /// 表示順は登録順を維持する。
    #[serde(default)]
    intermission_commands: Vec<IntermissionCommandEntry>,
    /// `Scene::Intermission` での選択カーソル位置 (0-indexed)。
    #[serde(default)]
    intermission_cursor: usize,
    /// インターミッションのサブモード (メインメニュー / 機体改造のユニット選択)。
    #[serde(default)]
    intermission_mode: IntermissionMode,
    /// 乗り換えの移動元ユニット uid (移動先選択待ちのとき Some)。transient。
    #[serde(skip)]
    ride_change_source: Option<String>,
    /// 一覧シーン (PilotList / UnitList) を抜けたときに戻るシーン。インターミッション
    /// 「ステータス」から開いた場合に Intermission へ戻すために使う。None なら既定
    /// (Title)。transient。
    #[serde(skip)]
    scene_return_to: Option<Scene>,
    /// 単機ステータス詳細 (`Scene::UnitDetail`) で表示中の機体の、味方ロスター内
    /// 0 始まり位置。`◀ / ▶` で更新する。transient。
    #[serde(skip)]
    status_detail_index: usize,
    /// 分離 (合体形態) の着地点選択キュー: まだ配置していない構成ユニットの uid 列。
    /// `LandingSelect` 確定ごとに先頭から取り出して次の選択へ進む。transient。
    #[serde(skip)]
    landing_queue: Vec<String>,
    /// 着地点選択キューの起点 (合体元 host の位置)。候補算出に使う。transient。
    #[serde(skip)]
    landing_source: Option<(u32, u32)>,
    /// 着地点選択キュー完了後にメニューを再表示する対象 uid (分離の host)。transient。
    #[serde(skip)]
    landing_menu_uid: Option<String>,
    /// 敗北 (`game_over`) でシナリオ / `GameOver.eve` の出口イベントが無く、組込みの
    /// コンティニュー/タイトル選択を待っている状態。soft-lock 防止のフォールバック。
    /// この間 Enter/クリック = コンティニュー、右クリック/Esc = タイトルへ。
    #[serde(default)]
    pending_game_over: bool,
    /// インターミッション項目から起動したサブコマンド (.eve) を実行中か。
    /// true のあいだ scene は MapView (サブコマンドの描画用)。サブコマンドの
    /// スクリプトが完了したら `Scene::Intermission` へ戻す。
    #[serde(default)]
    intermission_running: bool,
    /// 直近の `.eve` スクリプト実行エラー (デバッグ用)。`run_loop` が
    /// Err を返したときに `L<行>: <メッセージ>` 形式で記録する。
    #[serde(skip)]
    last_script_error: Option<String>,
    /// `Return <value>` で返された直近の値。
    /// `Call(<label>)` 形式の条件式評価（`evaluate_command_condition`）で
    /// サブルーチンが返す値を受け取るために使用する。
    /// スクリプト実行中にのみ意味を持つ一時値なので save/load しない。
    #[serde(skip)]
    last_return_value: String,
    /// `Unit <name> <rank>` の短い形式で生成した「カレントユニット」の `uid`。
    /// SRC の `Event.SelectedUnitForEvent` 相当。後続の `Ride <pilot>`
    /// (unit 省略形) はこの uid のユニットに搭乗員を載せる。空文字は未設定。
    #[serde(default)]
    selected_unit_for_event: String,
    /// 現在進行中ステージの `.eve` ファイル (`advance_to_next_stage` が
    /// `次ステージ` から設定)。`begin_battle` の `スタート` / `Start`
    /// auto-fire をこのファイルスコープで行い、同名ラベルが多数の `.eve`
    /// に存在するときに別ステージのものを誤発火しないようにする。
    #[serde(default)]
    current_stage_file: String,
    /// スクリプト完了後に実行する継続のスタック (docs/FLOW_REDESIGN.md §2.2)。
    /// イベントを起動する側が「完了後にやること」を push し、
    /// `event_runtime::run_loop` がスクリプト完了 (インライン完了 / resume 後の
    /// 完了 / エラー終了) を検知すると [`Self::on_script_completed`] が idle な間
    /// pop して実行する。suspend 中のセーブを生き残る (`serde(default)`)。
    #[serde(default)]
    flow: Vec<crate::flow::FlowCont>,
    /// [`Self::on_script_completed`] の再入ガード。継続の実行中に起動した
    /// スクリプトがインライン完了したとき、内側の完了通知では drain せず
    /// 外側のループに続きを任せる (再帰防止)。
    #[serde(skip)]
    flow_draining: bool,
    /// 割込みイベントのキュー (原典 `Event.bas::EventQue` 相当)。
    /// スクリプト実行中に発生した自動発火イベント (`破壊` / `全滅` / `損傷率`
    /// 等) はここに積まれ、現在のスクリプト完了後に
    /// [`Self::on_script_completed`] が FIFO で実行する。実行中でなければ
    /// 投函と同時に実行される。
    ///
    /// 各要素は `(ラベル名, 発火スコープ)`。スコープが `Some(file)` なら当該
    /// ファイル内のラベルとして発火し (`ターン` / `破壊` / `全滅` 等の章ローカル
    /// イベント)、`None` なら global 解決する (`GameOver` 等のシステムイベント)。
    /// 全 22 章を 1 ライブラリに同時ロードする本実装では、章ローカルイベントを
    /// global 解決すると同名ラベル (各章の `ターン 1 敵` 等) が別章へ漏れて
    /// 「話が飛ぶ」ため、現ステージファイルにスコープする。
    #[serde(default)]
    event_queue: std::collections::VecDeque<(String, Option<String>)>,
    /// スクリプト実行ループ (`event_runtime::run_loop`) のネスト深さ。
    /// 0 でないあいだは「スクリプト実行中」であり、完了通知 (drain) は
    /// 最外殻の run_loop が完了したときだけ行う。実行中の ctx はローカル変数
    /// なので `script_ctx.is_none()` では実行中を判別できないことに注意。
    #[serde(skip)]
    script_depth: usize,
    /// 直近のステージ実行が **通過** した `スタート` / `Start` ラベル行の PC 一覧
    /// (= スタートイベントの中身がインライン実行された事実の記録)。
    /// `advance_to_next_stage` / `start_scenario` が実行前にクリアし、
    /// event_runtime がラベル行通過時に積む。`FlowCont::AfterStageFileRun` が
    /// 「現ステージファイルの `スタート` を既に実行したか」を**ファイルの
    /// PC 範囲**で判定する (旧実装の「中断せず完了したら実行済みとみなす」
    /// 推定の置き換え)。bool でなく PC で持つのは、ロード時に lib .eve の
    /// top-level 実行が他ファイルの `スタート` を通過しても混同しないため。
    #[serde(default)]
    start_passed_pcs: Vec<usize>,
    /// `Open` / `Print` / `Read` 用のインメモリ仮想ファイルシステム。
    /// キー = 正規化パス (小文字 / `/` 区切り)、値 = 行リスト。
    /// キャラメイキングの `Data\一時フォルダ\Pilot.txt` 書き出し等で使う。
    /// save/load を生き残る。
    #[serde(default)]
    virtual_files: std::collections::BTreeMap<String, Vec<String>>,
    /// 開いているファイルハンドル。スクリプト実行中のみ有効なので
    /// save/load では持ち越さない。
    #[serde(skip)]
    open_files: std::collections::BTreeMap<String, OpenFileHandle>,
    #[serde(skip)]
    next_file_handle: u32,
    /// `KeyState()` 関数の呼び出し回数。`run_loop` の開始時にリセットされ、
    /// Do While (KeyState()=0) ループが STEP_LIMIT まで走り続けるのを防ぐ。
    /// 一定回数を超えると `KeyState()` は「押された」を返してループを脱出させる。
    ///
    /// `eval_script_function` が `&App` しか持てないため `Cell` で内部可変にする。
    #[serde(skip)]
    keystate_call_count: std::cell::Cell<usize>,
    /// 直近に実行した .eve 文の pc (script_library インデックス)。診断用 transient。
    /// `run_loop_inner` が各文の実行前に更新する。対話 (Ask/Menu) が中断したとき
    /// `current_exec_pc()` でその発生元 pc を取得し、`script_library().labels` の逆引きで
    /// 発生元ラベルを特定できる (動的構築されたメニューの源を追う triage 用)。
    #[serde(skip)]
    exec_pc: std::cell::Cell<usize>,
    /// 現在実行中の自動発火イベントラベルの pc (`trigger_label*` で起動したもの)。
    /// 引数無し `ClearEvent` が「いま実行中のイベントラベルを消す」(SRC
    /// `Event.CurrentLabel` 相当) ために使う。trigger 時に設定し、最外殻 run_loop の
    /// **完了時**にクリア (Talk 中断→resume を跨いで保持)。非イベント実行中は None
    /// なので、その場合の引数無し ClearEvent は no-op (SRC 準拠)。transient。
    #[serde(skip)]
    current_event_label_pc: Option<usize>,
    /// 直近の `Wait Click` 系の中断を **右クリックで解除した** か。SRC の
    /// `Wait Click` → `If KeyState(2) Then` (右ボタン = キャンセル/戻る) を実現する。
    /// 右クリック応答で true、`KeyState(2)` が読むと **その場で false に消費** する
    /// ワンショット。Web には「ボタン押下中」状態が無いため、`Do While KeyState(2)`
    /// (ボタン解放待ちビジーループ) が無限化しないよう一度きりで返す必要がある。
    /// `KeyState(2)` の評価が `&App` なので `Cell` で内部可変にする。transient。
    #[serde(skip)]
    wait_click_right: std::cell::Cell<bool>,
    /// マップの時間帯。`Sunset` / `Noon` / `Night` コマンドで設定。
    /// `Info(マップ, 時間帯)` が返す値。省略時は "昼"。
    #[serde(default = "default_time_of_day")]
    pub time_of_day: String,
    /// `Load title` コマンドで登録されたタイトル (作品) 名リスト。
    /// `Forget title` でリストから削除する。次回プレイ開始時のロード対象管理。
    /// SRC.Sharp の `SRC.Titles` に相当。
    #[serde(default)]
    titles: Vec<String>,
    /// 総ターン数 (累計)。SRC.Sharp の `SRC.TotalTurn` 相当。
    /// `Set 総ターン数 N` / `Incr 総ターン数` で変更可能。
    #[serde(default)]
    total_turn: u32,
}

/// 仮想ファイルシステムの開いているハンドル 1 件。
#[derive(Debug, Clone)]
struct OpenFileHandle {
    /// 正規化パス。
    path: String,
    /// 書き込みモード (`出力` / `追加`) なら `true`、読み込みモードなら `false`。
    write: bool,
    /// 読み込みカーソル (次に読む行 index)。
    read_cursor: usize,
}

/// `IntermissionCommand <name> <file>` の登録 1 件。
///
/// SRC.Sharp では `Expression.DefineGlobalVariable("IntermissionCommand(file)")`
/// に表示名を格納する形だが、本実装では明示的な構造体で持つ。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct IntermissionCommandEntry {
    /// 画面表示用の項目名 (例: "キャラクターメイキング")
    pub name: String,
    /// 選択時に実行する .eve ファイル (例: "Lib\\CMaking.eve")
    pub file: String,
}

/// `*ユニットコマンド` の対象指定 (`味方` / `敵` / 空 等) がこのユニットの
/// 勢力に合致するか。ユニット名指定 (`ナデシコ` 等) は本実装では非対応 (false)。
fn custom_command_targets_party(target: &str, party: crate::Party) -> bool {
    use crate::Party;
    match target.trim() {
        "" | "全" | "全て" => true,
        "味方" | "Player" => party == Party::Player,
        "敵" | "Enemy" => party == Party::Enemy,
        "中立" | "Neutral" => party == Party::Neutral,
        "ＮＰＣ" | "NPC" | "友軍" | "Allied" => party == Party::Npc,
        _ => false,
    }
}

/// 修理 / 補給 の獲得経験値を、対象パイロットと実行者のレベル差で増減させる
/// (SRC `Unit.cs::GetExp`)。`base` は 修理 / 補給 の基準値で、差 (target - actor) が
/// 大きいほど倍率が上がる (> +7 で ×5、-1 以下で逓減)。倍率テーブル・基準値とも
/// 原典準拠 (修理=100 / 補給=150、exp スケールは 500/level)。
/// 素質 / 遅成長 等の技能補正は未対応。
fn support_exp_with_level_diff(base: i32, target_level: i32, actor_level: i32) -> i32 {
    let xp = match target_level - actor_level {
        d if d > 7 => base * 5,
        7 => base * 9 / 2,
        6 => base * 4,
        5 => base * 7 / 2,
        4 => base * 3,
        3 => base * 5 / 2,
        2 => base * 2,
        1 => base * 3 / 2,
        0 => base,
        -1 => base / 2,
        -2 => base / 4,
        -3 => base / 6,
        -4 => base / 8,
        -5 => base / 10,
        _ => base / 12,
    };
    xp.max(1)
}

/// アビリティ効果トークンを (基底名, レベル) に分割する。`回復Lv2` → (`回復`, 2)、
/// `治癒` → (`治癒`, 1)。`Lv` は半角。レベル省略・解析失敗時は 1。
fn split_effect_level(s: &str) -> (&str, i32) {
    if let Some(pos) = s.find("Lv") {
        let lv = s[pos + 2..].trim().parse().unwrap_or(1);
        (&s[..pos], lv)
    } else {
        (s, 1)
    }
}

fn default_time_of_day() -> String {
    "昼".to_string()
}

/// 戦闘判定 (`predict_*`) は status 名スライスのみを参照するため、特徴由来の戦闘修正
/// 特殊能力 (分身 / ステルス) を condition 由来の status 一覧へ合流させる。これが無いと
/// ユニットデータの特殊能力 (分身/ステルス) が戦闘の命中判定に反映されない
/// (`predict_with_status_terrain` は条件名のみを見るため)。
fn push_combat_feature_statuses(unit: &crate::UnitInstance, statuses: &mut Vec<String>) {
    use crate::feature::feature_name_matches_base as base_match;
    for f in &unit.active_features {
        if !f.is_active {
            continue;
        }
        let name = f.name.as_str();
        if name == "ステルス" {
            // ステルス は予測の hit% 低下 (-30) として扱う。
            // 分身/超回避 は (B) で実行段の完全回避ロール (check_dodge_feature) へ移行した
            // ため、ここでは積まない (active_features から直接判定する)。
            statuses.push(f.name.clone());
        } else if base_match(name, "盾") || name.contains("バリアシールド") {
            // (A) 盾Lv*・バリアシールド(非標準名) のみ既存の `バリア` status (÷2 近似) に合流。
            // **(B) へ移行済で除外**: シールド系→apply_shield_defense / バリア・フィールド系→
            // apply_barrier_absorb / アクティブフィールド→apply_active_field_absorb (確率) /
            // 分身・超回避→check_dodge_feature。単独 `プロテクト・フィールド` は値 `解説…` の
            // ヘルプ文＝非機能 (どの分岐にも該当せず inert)。`バリア中和` は攻撃属性で除外。
            statuses.push("バリア".to_string());
        }
    }
}

mod ai;
mod battle;
mod combat_rules;
mod debug;
mod dialog;
mod input;
mod intermission;
mod map_attack;
mod reaction;
mod script_flow;
mod spirit;
mod stage_flow;
mod status_detail;
mod unit_commands;
mod vfs;

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

impl App {
    pub fn new() -> Self {
        Self::with_rng_seed(0xDEADBEEF_CAFEBABE)
    }

    pub fn with_rng_seed(seed: u64) -> Self {
        Self {
            scene: Scene::Title,
            settings: Settings::default(),
            settings_snapshot: None,
            database: GameDatabase::new(),
            map_cursor: None,
            turn: Turn::new(),
            map_scroll: (0, 0),
            stage: String::new(),
            messages: Vec::new(),
            rng_state: seed,
            selected_weapon_idx: 0,
            stage_state: crate::stage::StageState::Briefing,
            briefing: String::new(),
            script_vars: std::collections::BTreeMap::new(),
            script_ctx: None,
            pending_dialog: None,
            script_for_stack: Vec::new(),
            script_call_stack: Vec::new(),
            script_sublocal_stack: Vec::new(),
            upvar_level: 0,
            upvar_base_argnum: 0,
            script_library: crate::event_runtime::ScriptLibrary::default(),
            script_overlay: crate::script_overlay::ScriptOverlay::default(),
            money: 0,
            command_menu: None,
            action_mode: crate::command_menu::ActionMode::Browse,
            hotpoints: Vec::new(),
            pending_audio: Vec::new(),
            pending_timer: None,
            talk_pages: Vec::new(),
            talk_pages_speaker: String::new(),
            wait_clock: 0.0,
            animate_ai: false,
            reattack_in_progress: false,
            ai_runner: None,
            animate_battle: false,
            battle_anim: None,
            move_anim: None,
            auto_counter: false,
            pending_reaction: None,
            weapon_window_enabled: true,
            pending_weapon_select: None,
            forced_counter_weapon: None,
            pending_spirit: None,
            pending_transform: None,
            pending_ability: None,
            pending_launch: None,
            pending_reload: None,
            wall_clock_ms: 0.0,
            intermission_commands: Vec::new(),
            intermission_cursor: 0,
            status_detail_index: 0,
            landing_queue: Vec::new(),
            landing_source: None,
            landing_menu_uid: None,
            intermission_mode: IntermissionMode::Menu,
            ride_change_source: None,
            scene_return_to: None,
            pending_game_over: false,
            intermission_running: false,
            last_script_error: None,
            last_return_value: String::new(),
            keystate_call_count: std::cell::Cell::new(0),
            exec_pc: std::cell::Cell::new(0),
            current_event_label_pc: None,
            wait_click_right: std::cell::Cell::new(false),
            selected_unit_for_event: String::new(),
            current_stage_file: String::new(),
            flow: Vec::new(),
            flow_draining: false,
            event_queue: std::collections::VecDeque::<(String, Option<String>)>::new(),
            script_depth: 0,
            start_passed_pcs: Vec::new(),
            virtual_files: std::collections::BTreeMap::new(),
            open_files: std::collections::BTreeMap::new(),
            next_file_handle: 0,
            time_of_day: "昼".to_string(),
            titles: Vec::new(),
            total_turn: 0,
        }
    }

    /// `.eve` 実行エラーを記録 (event_runtime から呼ばれる)。
    pub fn set_last_script_error(&mut self, msg: String) {
        self.last_script_error = Some(msg);
    }
    pub fn last_script_error(&self) -> Option<&str> {
        self.last_script_error.as_deref()
    }

    /// `KeyState()` 呼び出し回数をインクリメントして返す。
    /// `Cell` による内部可変なので `&self` で呼べる（`eval_script_function` 対応）。
    pub fn increment_keystate_call_count(&self) -> usize {
        let next = self.keystate_call_count.get() + 1;
        self.keystate_call_count.set(next);
        next
    }

    /// `KeyState()` 呼び出し回数をリセット（スクリプト実行開始時に呼ぶ）。
    pub fn reset_keystate_call_count(&self) {
        self.keystate_call_count.set(0);
    }

    /// 実行中の .eve 文 pc を記録する (診断用、`run_loop_inner` が各文前に呼ぶ)。
    pub fn set_exec_pc(&self, pc: usize) {
        self.exec_pc.set(pc);
    }

    /// 直近に実行した .eve 文の pc を返す (対話の発生元特定用)。
    pub fn current_exec_pc(&self) -> usize {
        self.exec_pc.get()
    }

    /// 現在実行中の自動発火イベントラベルの pc を設定する (`trigger_label*` が起動時に呼ぶ)。
    pub(crate) fn set_current_event_label_pc(&mut self, pc: Option<usize>) {
        self.current_event_label_pc = pc;
    }

    /// 現在実行中の自動発火イベントラベルの pc (引数無し `ClearEvent` 用)。
    pub(crate) fn current_event_label_pc(&self) -> Option<usize> {
        self.current_event_label_pc
    }

    /// 直近の `Wait Click` を右クリックで解除したフラグを設定する。
    pub fn set_wait_click_right(&self, v: bool) {
        self.wait_click_right.set(v);
    }
    /// `KeyState(2)` 用: 右クリック解除フラグを **読んで消費** する (ワンショット)。
    /// 押下中状態の無い Web 環境で `Do While KeyState(2) Loop` (解放待ち) が
    /// 無限化しないよう、一度 true を返したら以降は false にする。
    pub fn take_wait_click_right(&self) -> bool {
        let v = self.wait_click_right.get();
        if v {
            self.wait_click_right.set(false);
        }
        v
    }

    pub fn intermission_commands(&self) -> &[IntermissionCommandEntry] {
        &self.intermission_commands
    }

    pub fn push_intermission_command(&mut self, name: String, file: String) {
        // 同名の既存項目は上書き (再登録に対応)。
        if let Some(e) = self
            .intermission_commands
            .iter_mut()
            .find(|e| e.name == name)
        {
            e.file = file;
            return;
        }
        self.intermission_commands
            .push(IntermissionCommandEntry { name, file });
    }

    pub fn remove_intermission_command(&mut self, name: &str) {
        self.intermission_commands.retain(|e| e.name != name);
    }

    pub fn clear_intermission_commands(&mut self) {
        self.intermission_commands.clear();
    }

    pub fn intermission_cursor(&self) -> usize {
        self.intermission_cursor
    }

    pub fn set_intermission_cursor(&mut self, n: usize) {
        self.intermission_cursor = n;
    }

    /// 単機ステータス詳細 (`Scene::UnitDetail`) で表示中の機体のロスター内位置。
    pub fn status_detail_index(&self) -> usize {
        self.status_detail_index
    }

    pub fn push_audio_request(&mut self, req: crate::audio::AudioRequest) {
        self.pending_audio.push(req);
    }

    pub fn pending_timer(&self) -> Option<f64> {
        self.pending_timer
    }

    /// `Now()` / `Year()` 等 SRC 時間関数が参照する Unix epoch ミリ秒 (UTC)。
    /// src-web 側が `Date::now()` を呼んで `set_wall_clock_ms` で更新する。
    pub fn wall_clock_ms(&self) -> f64 {
        self.wall_clock_ms
    }

    pub fn set_wall_clock_ms(&mut self, ms: f64) {
        self.wall_clock_ms = ms;
    }

    pub fn set_pending_timer(&mut self, seconds: f64) {
        self.pending_timer = if seconds > 0.0 { Some(seconds) } else { None };
    }

    /// `Wait Start`: スクリプト・タイムラインの基準時刻を現在位置にリセット。
    pub fn reset_wait_clock(&mut self) {
        self.wait_clock = 0.0;
    }

    /// 敵/中立/ＮＰＣ フェイズの逐次演出を有効化する (フロントエンドが起動時に呼ぶ)。
    /// 有効時、`end_phase` は AI を `tick` 駆動で 1 体ずつ実行する。
    pub fn set_animate_ai(&mut self, on: bool) {
        self.animate_ai = on;
    }

    /// 逐次 AI 実行が進行中か (フロントの描画/入力ガード用)。
    pub fn ai_running(&self) -> bool {
        self.ai_runner.is_some()
    }

    /// ネイティブ戦闘演出を有効化する (フロントエンドが起動時に呼ぶ)。
    /// 有効時、攻撃解決ごとに短い命中演出を `tick` 駆動で再生する。
    pub fn set_animate_battle(&mut self, on: bool) {
        self.animate_battle = on;
        if !on {
            self.battle_anim = None;
        }
    }

    /// 進行中のネイティブ戦闘演出 (なければ `None`)。フロントが読んで描画する。
    pub fn battle_anim(&self) -> Option<&crate::battle_anim::BattleAnim> {
        self.battle_anim.as_ref()
    }

    /// 進行中の移動スライド演出 (なければ `None`)。フロントが該当ユニットの
    /// 表示位置を `position()` で補間して描画する。
    pub fn move_anim(&self) -> Option<&crate::battle_anim::MoveAnim> {
        self.move_anim.as_ref()
    }

    /// SRC「自動反撃モード」の現在値 (true=自動反撃 / false=手動で反撃選択)。
    pub fn auto_counter(&self) -> bool {
        self.auto_counter
    }

    /// 「自動反撃モード」を切り替える (マップコマンド用)。戻り値は新しい値。
    pub fn toggle_auto_counter(&mut self) -> bool {
        self.auto_counter = !self.auto_counter;
        self.auto_counter
    }

    /// 「武器選択ウィンドウ」の現在値 (true=攻撃時に武器選択窓を出す)。
    pub fn weapon_window(&self) -> bool {
        self.weapon_window_enabled
    }

    /// 「武器選択ウィンドウ」を切り替える (マップコマンド用)。戻り値は新しい値。
    pub fn toggle_weapon_window(&mut self) -> bool {
        self.weapon_window_enabled = !self.weapon_window_enabled;
        self.weapon_window_enabled
    }

    /// `Wait Until target_secs`: 基準時刻から `target_secs` 秒経過時まで待つ。
    /// 直前の位置 (`wait_clock`) からの増分を返し、タイムラインを進める。
    /// 既に過ぎている場合は 0 を返す。
    pub fn advance_wait_clock(&mut self, target_secs: f64) -> f64 {
        let delta = (target_secs - self.wait_clock).max(0.0);
        if target_secs > self.wait_clock {
            self.wait_clock = target_secs;
        }
        delta
    }

    pub fn clear_pending_timer(&mut self) {
        self.pending_timer = None;
    }

    /// 蓄積された AudioRequest 一覧を取り出して内部をクリア。
    pub fn take_pending_audio(&mut self) -> Vec<crate::audio::AudioRequest> {
        std::mem::take(&mut self.pending_audio)
    }

    /// テスト・デバッグ用: 取り出さずに参照。
    pub fn pending_audio(&self) -> &[crate::audio::AudioRequest] {
        &self.pending_audio
    }

    pub fn hotpoints(&self) -> &[crate::event_runtime::HotpointEntry] {
        &self.hotpoints
    }
    pub fn push_hotpoint(&mut self, h: crate::event_runtime::HotpointEntry) {
        self.hotpoints.push(h);
    }
    pub fn clear_hotpoints(&mut self) {
        self.hotpoints.clear();
    }

    /// `pending_dialog` が Menu（Hotpoint 経由）の場合に canvas クリック座標を
    /// Hotpoint 矩形と当たり判定し、ヒットしたら対応するメニュー番号で確定する。
    /// ヒットしなかった場合は `false` を返して入力を消費しない。
    fn try_hotpoint_click(&mut self, x: i32, y: i32) -> bool {
        if !matches!(
            self.pending_dialog,
            Some(crate::dialog::PendingDialog::Menu { .. })
        ) {
            return false;
        }
        if self.hotpoints.is_empty() {
            return false;
        }
        // 後から登録された Hotpoint が手前にあると見做し、逆順で hit-test。
        let hit = self
            .hotpoints
            .iter()
            .enumerate()
            .rev()
            .find(|(_, h)| x >= h.x && x < h.x + h.w && y >= h.y && y < h.y + h.h)
            .map(|(i, _)| i);
        match hit {
            Some(i) => self.respond_dialog((i + 1) as u32),
            None => false,
        }
    }

    pub fn command_menu(&self) -> Option<&crate::command_menu::CommandMenu> {
        self.command_menu.as_ref()
    }
    pub fn action_mode(&self) -> crate::command_menu::ActionMode {
        self.action_mode.clone()
    }

    pub const fn money(&self) -> i64 {
        self.money
    }
    pub fn set_money(&mut self, n: i64) {
        self.money = n.clamp(0, 999_999_999);
    }
    pub fn add_money(&mut self, delta: i64) {
        self.money = self.money.saturating_add(delta).clamp(0, 999_999_999);
    }

    pub fn script_library(&self) -> &crate::event_runtime::ScriptLibrary {
        &self.script_library
    }
    pub fn script_library_mut(&mut self) -> &mut crate::event_runtime::ScriptLibrary {
        &mut self.script_library
    }

    pub fn script_overlay(&self) -> &crate::script_overlay::ScriptOverlay {
        &self.script_overlay
    }
    pub fn script_overlay_mut(&mut self) -> &mut crate::script_overlay::ScriptOverlay {
        &mut self.script_overlay
    }

    pub fn push_for_frame(&mut self, f: impl Into<crate::event_runtime::LoopFrame>) {
        self.script_for_stack.push(f.into());
    }
    pub fn pop_for_frame(&mut self) -> Option<crate::event_runtime::LoopFrame> {
        self.script_for_stack.pop()
    }
    pub fn last_for_frame(&self) -> Option<&crate::event_runtime::LoopFrame> {
        self.script_for_stack.last()
    }
    /// 末尾フレームに対し可変アクセス（ForEach の index 更新等で使用）。
    pub fn last_for_frame_mut(&mut self) -> Option<&mut crate::event_runtime::LoopFrame> {
        self.script_for_stack.last_mut()
    }
    /// `Call` のリターンアドレスと、呼び出し元の `Args(1..9)` スナップショットを
    /// 積む。ネストした `Call` が `Args` を破壊しても `Return` で復元するため。
    pub fn push_call_return(&mut self, pc: usize, saved_args: Vec<String>) {
        self.script_call_stack.push((pc, saved_args));
        // サブルーチンローカル変数のフレームを同時に積む (深さを揃える)。
        self.script_sublocal_stack.push(Vec::new());
    }
    pub fn pop_call_return(&mut self) -> Option<(usize, Vec<String>)> {
        // フレーム内で `Local` 宣言された変数を宣言前の状態へ巻き戻す。
        // 逆順に戻すことで、同名を複数回宣言した場合も最初の値に復帰する。
        if let Some(frame) = self.script_sublocal_stack.pop() {
            for (name, prev) in frame.into_iter().rev() {
                match prev {
                    Some(v) => {
                        self.script_vars.insert(name, v);
                    }
                    None => {
                        self.script_vars.remove(&name);
                    }
                }
            }
        }
        self.script_call_stack.pop()
    }
    /// `Local` による宣言を現在のコールフレームに記録する。
    ///
    /// フレームが無い (トップレベルの `Local`) 場合は何もしない — 原典でも
    /// イベント終了まで残るため、従来どおり通常の変数として振る舞う。
    /// 同一フレームで同名を再宣言しても、退避するのは **最初の 1 回だけ**。
    pub fn record_sublocal(&mut self, name: &str) {
        let Some(frame) = self.script_sublocal_stack.last_mut() else {
            return;
        };
        if frame.iter().any(|(n, _)| n == name) {
            return;
        }
        let prev = self.script_vars.get(name).cloned();
        frame.push((name.to_string(), prev));
    }
    /// `UpVar` コマンドが使う: 現フレームの UpVar 呼び出し回数と、最初の
    /// UpVar 直前の ArgNum を取得・変更する。
    pub fn upvar_level(&self) -> usize {
        self.upvar_level
    }
    pub fn upvar_base_argnum(&self) -> usize {
        self.upvar_base_argnum
    }
    pub fn set_upvar_level(&mut self, v: usize) {
        self.upvar_level = v;
    }
    pub fn set_upvar_base_argnum(&mut self, v: usize) {
        self.upvar_base_argnum = v;
    }

    /// `Return <value>` コマンドで返された最後の値を設定する。
    pub fn set_last_return_value(&mut self, v: String) {
        self.last_return_value = v;
    }
    /// 直近の Return 値を取り出してフィールドをリセットする。
    pub fn last_return_value_str(&self) -> &str {
        &self.last_return_value
    }

    pub fn take_last_return_value(&mut self) -> String {
        std::mem::take(&mut self.last_return_value)
    }

    /// `script_for_stack` の現在の深さ (条件評価のクリーンアップ用)。
    pub fn for_stack_len(&self) -> usize {
        self.script_for_stack.len()
    }
    /// `script_for_stack` を `len` まで切り詰める (条件評価後のクリーンアップ用)。
    pub fn truncate_for_stack(&mut self, len: usize) {
        self.script_for_stack.truncate(len);
    }
    /// コールスタックの N 番目エントリ (0 = 最初に積まれた) の saved_args を参照する。
    /// `UpVar` が祖先フレームの引数を読むために使う。
    pub fn call_stack_saved_args(&self, idx: usize) -> Option<&Vec<String>> {
        self.script_call_stack.get(idx).map(|(_, args)| args)
    }
    /// コールスタックの深さ (積まれているフレーム数)。
    pub fn call_stack_depth(&self) -> usize {
        self.script_call_stack.len()
    }

    /// `.eve` シナリオ変数の値を取得（未定義は空文字）。
    pub fn script_var(&self, name: &str) -> &str {
        self.script_vars.get(name).map(String::as_str).unwrap_or("")
    }

    /// 変数が **キーとして** 定義されているか (空文字代入でも `true`)。
    /// SRC.Sharp `IsVariableDefined` 同等: `Set var ""` は defined 扱い。
    pub fn is_script_var_defined(&self, name: &str) -> bool {
        self.script_vars.contains_key(name)
    }

    /// `.eve` シナリオ変数を設定（既存値があれば上書き）。
    pub fn set_script_var(&mut self, name: String, value: String) {
        self.script_vars.insert(name, value);
    }

    /// シナリオ変数を **完全に削除**。`Unset` コマンドから呼ぶ。
    /// SRC.Sharp `Expression.UndefineVariable` 同等: 削除後の
    /// `IsVarDefined` は 0 を返す (空文字代入とは異なる)。
    pub fn unset_script_var(&mut self, name: &str) {
        self.script_vars.remove(name);
    }

    /// 全シナリオ変数（テスト / デバッグ表示用）。
    pub fn script_vars(&self) -> &std::collections::BTreeMap<String, String> {
        &self.script_vars
    }

    /// シナリオ定義のユニットコマンド (`*ユニットコマンド <名> 味方 …`) を
    /// `unit_uid` のユニットを対象に実行する。
    ///
    /// `対象ユニットＩＤ` / `対象パイロット` を束縛してからコマンド本体
    /// ラベルを実行する (`Create` と同じシステム変数規約)。本体が `Ask` 等で
    /// 中断した場合は `pending_dialog` がセットされ、呼び出し側が応答を
    /// 駆動する。該当コマンドが無い / 対象ユニット不在 / 既にモーダル中なら
    /// `false` を返す。
    pub fn invoke_custom_unit_command(&mut self, unit_uid: &str, command_name: &str) -> bool {
        let body_pc = self
            .script_library()
            .custom_commands
            .iter()
            .find(|c| c.is_unit && c.name == command_name)
            .map(|c| c.body_pc);
        let Some(body_pc) = body_pc else {
            return false;
        };
        let pilot = self
            .database
            .unit_instances
            .iter()
            .find(|u| u.uid == unit_uid)
            .map(|u| u.pilot_name.clone());
        let Some(pilot) = pilot else {
            return false;
        };
        self.set_script_var("対象ユニットＩＤ".to_string(), unit_uid.to_string());
        self.set_script_var("対象パイロット".to_string(), pilot);
        // 独自画面 (ステータス表示等) を描く前に前回の描画と Hotpoint をクリアし、
        // 残留を防ぐ。スパロボ戦記 AlphaSecond のステータス画面は終了時の
        // `ClearObj` が `Wait 1` タイマ越しで、別ユニットを続けて開くと前ユニットの
        // 描画が透けて見える。コマンド画面はエントリで一旦クリアして開始する。
        self.script_overlay.clear();
        self.hotpoints.clear();
        // body_pc はラベル行。本体はその次の文から始まる。
        crate::event_runtime::run_from_pc(self, body_pc + 1).is_ok()
    }

    // --- 仮想ファイルシステム (`Open` / `Print` / `Read` / `Close`) ---

    pub const fn selected_weapon_idx(&self) -> usize {
        self.selected_weapon_idx
    }

    /// 現状を JSON にシリアライズ（セーブデータ）。
    pub fn to_save_json(&self) -> Result<String, String> {
        serde_json::to_string(self).map_err(|e| e.to_string())
    }

    /// JSON からデシリアライズして App を復元。
    ///
    /// SRC `再開イベント` (`再開イベント.md`) は本関数では発火させない:
    /// シリアライザ的な API として副作用無しを保つため。
    /// 中断セーブから再開した場合は別途 [`App::fire_resume_event`] を呼ぶこと
    /// (画面再描画用の `PaintPicture` 等が `再開:` ラベル経由で復元される)。
    pub fn from_save_json(s: &str) -> Result<Self, String> {
        let mut app: Self = serde_json::from_str(s).map_err(|e| e.to_string())?;
        // pos_index は `#[serde(skip)]` のためロード後に再構築する。
        app.database.rebuild_pos_index();
        Ok(app)
    }

    /// SRC `再開:` ラベル (`再開イベント.md`) を auto-fire する。
    /// `from_save_json` でロード完了後、フロントエンド (src-web) が呼ぶ想定。
    /// 既に script_ctx / pending_dialog が立っているなら no-op で no-op を返す。
    pub fn fire_resume_event(&mut self) -> bool {
        crate::event_runtime::trigger_label(self, "再開")
    }

    /// SRC `収納 <unit>:` ラベル (`収納イベント.md`) を auto-fire する。
    /// プレイヤーがユニットコマンド「搭載」で母艦に格納したときに UI から呼ぶ規約。
    /// `相手パイロット` / `相手ユニットＩＤ` システム変数に carrier 情報をセット
    /// してから発火する。`unit_idx` は格納されたユニット、`carrier_idx` は母艦。
    /// 戻り値は発火したか (該当ラベル未定義なら false)。
    pub fn fire_boarding_event(&mut self, unit_idx: usize, carrier_idx: usize) -> bool {
        let Some(u) = self.database.unit_instances.get(unit_idx) else {
            return false;
        };
        let pilot_name = u.pilot_name.clone();
        let unit_data_name = u.unit_data_name.clone();
        let party = u.party;
        let Some(c) = self.database.unit_instances.get(carrier_idx) else {
            return false;
        };
        let carrier_pilot = c.pilot_name.clone();
        let carrier_uid = c.uid.clone();
        // システム変数を SRC 規約に従ってセット
        self.set_script_var("相手パイロット".to_string(), carrier_pilot);
        self.set_script_var("相手ユニットＩＤ".to_string(), carrier_uid.clone());
        // 格納されたユニットの life_state を更新
        self.database.unit_instances[unit_idx].life_state = "格納".to_string();
        let stored_uid = self.database.unit_instances[unit_idx].uid.clone();
        self.database.set_off_map(&stored_uid, true);
        // 母艦 ↔ 格納ユニットの相互リンクを張る (発進 / 毎ターン回復で使う)。
        // uid が無いユニット (一部の .eve 配置) はリンクを張れないので skip。
        if !carrier_uid.is_empty() && !stored_uid.is_empty() {
            self.database.unit_instances[unit_idx].stored_in = Some(carrier_uid.clone());
            if let Some(c) = self.database.unit_by_uid_mut(&carrier_uid) {
                if !c.stored_units.contains(&stored_uid) {
                    c.stored_units.push(stored_uid.clone());
                }
            }
        }
        // `収納 <unit>:` を pilot/unit/party の順に試行
        crate::event_runtime::fire_unit_event_labels_public(
            self,
            &["収納"],
            &pilot_name,
            &unit_data_name,
            party,
        )
    }

    /// SRC `勝利条件:` ラベル (`勝利条件イベント.md`) を auto-fire する。
    /// プレイヤーがマップコマンド「作戦目的」を実行したときに UI から呼ぶ規約。
    /// ラベルが未定義なら `false` を返す (= 「作戦目的」メニュー項目を非表示にする
    /// シグナル)。本実装の UI は当面メニュー連動を未実装だが、API は先行整備。
    pub fn fire_victory_condition_event(&mut self) -> bool {
        crate::event_runtime::trigger_label(self, "勝利条件")
    }

    /// `勝利条件:` ラベルが定義されているか (= マップメニューに「作戦目的」を
    /// 表示するべきか)。`fire_victory_condition_event` を呼ぶ前のチェックに使う。
    pub fn has_victory_condition_event(&self) -> bool {
        self.script_library.label_pc("勝利条件").is_some()
    }

    /// 現在のモーダルゲート状態 (`ModalGate`) を読み出す。
    /// `pending_dialog` / `pending_timer` の組合せから純粋関数で導出する。
    pub fn modal_gate(&self) -> crate::modal::ModalGate {
        crate::modal::classify(self.pending_dialog.as_ref(), self.pending_timer)
    }

    /// 1 フレーム分の時間を進める（アニメーション補間）。`dt_secs` は秒数。
    /// 各ユニットの `displayed_damage` を `damage` に向けて指数減衰補間する。
    /// 補間係数は HP 1000 / 0.5 秒で 60% 進む程度の感覚で設定。
    /// 戻り値: 補間が進んで再描画が必要なら `true`。
    pub fn tick(&mut self, dt_secs: f64) -> bool {
        let mut dirty = false;
        let factor = 1.0 - (-dt_secs * 8.0).exp(); // 0..1
        for u in &mut self.database.unit_instances {
            let target = u.damage as f64;
            let diff = target - u.displayed_damage;
            if diff.abs() > 0.5 {
                u.displayed_damage += diff * factor;
                dirty = true;
            } else if (u.displayed_damage - target).abs() > 0.0 {
                u.displayed_damage = target;
                dirty = true;
            }
        }
        // タイマ進行は `ModalGate::awaits_timer_tick()` が真のときのみ。
        // 満了後に resume を呼ぶのは dialog が立っていない場合に限る
        // (Dialog/DialogOverTimer のうち DialogOverTimer は Timer 部分だけ
        //  満了させて Dialog だけ残し、ユーザ応答後に通常の resume 経路で
        //  処理する)。
        let gate = self.modal_gate();
        if gate.awaits_timer_tick() {
            if let Some(remaining) = self.pending_timer {
                let new_remaining = remaining - dt_secs;
                if new_remaining <= 0.0 {
                    self.pending_timer = None;
                    if !gate.awaits_dialog_response() {
                        // タイマだけが原因で中断していたスクリプトを再開。
                        let _ = crate::event_runtime::resume(self);
                        self.on_script_completed();
                        self.return_from_intermission_subcommand_if_idle();
                        self.auto_progress_stage_state_if_idle();
                    }
                    dirty = true;
                } else {
                    self.pending_timer = Some(new_remaining);
                }
            }
        }
        // 移動スライド演出を先に進める (移動 → 完了後に戦闘演出 の順で見せる)。
        if let Some(m) = self.move_anim.as_mut() {
            m.elapsed += dt_secs;
            if m.finished() {
                self.move_anim = None;
            }
            dirty = true;
        }
        // ネイティブ戦闘演出を進める。移動演出の再生中は据え置き (移動を見せきってから)。
        if self.move_anim.is_none() {
            if let Some(anim) = self.battle_anim.as_mut() {
                anim.elapsed += dt_secs;
                if anim.finished() {
                    self.battle_anim = None;
                }
                dirty = true;
            }
        }
        // 逐次 AI ランナー (敵/中立/ＮＰＣ フェイズの 1 体ずつ実行) を進める。
        if self.ai_runner.is_some() {
            dirty |= self.ai_runner_tick(dt_secs);
        }
        dirty
    }

    /// 新規 `UnitInstance.uid` を発行 (単調増加)。採番は `GameDatabase` の
    /// カウンタに一元化し、`register_unit` と同じ供給源を使う。
    /// SRC の `グループＩＤ` 相当の一意 ID 採番に使う。
    pub fn next_unit_id(&mut self) -> String {
        self.database.mint_uid()
    }

    /// `Unit <name> <rank>` で生成したカレントユニットの `uid`。未設定なら空文字。
    pub fn selected_unit_for_event(&self) -> &str {
        &self.selected_unit_for_event
    }

    /// カレントユニットの `uid` を設定 (`Unit` 短い形式が呼ぶ)。
    pub fn set_selected_unit_for_event(&mut self, uid: String) {
        self.selected_unit_for_event = uid;
    }

    /// splitmix64 ベースの簡易 PRNG。
    fn next_u32(&mut self) -> u32 {
        self.rng_state = self.rng_state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.rng_state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        (z ^ (z >> 31)) as u32
    }

    pub fn stage(&self) -> &str {
        &self.stage
    }

    pub fn set_stage(&mut self, name: String) {
        self.stage = name;
    }

    /// 現在進行中ステージの `.eve` ファイル名 (`advance_to_next_stage` が設定)。
    pub fn current_stage_file(&self) -> &str {
        &self.current_stage_file
    }

    /// 現在の時間帯 ("昼" / "夕" / "夜")。`Noon` / `Sunset` / `Night` コマンドで変更。
    pub fn time_of_day(&self) -> &str {
        &self.time_of_day
    }

    /// 時間帯を設定する。
    pub fn set_time_of_day(&mut self, s: &str) {
        self.time_of_day = s.to_string();
    }

    /// `Load` コマンドで登録されたタイトルリストを返す。
    pub fn titles(&self) -> &[String] {
        &self.titles
    }

    /// タイトルリストへの変更可能参照を返す。
    pub fn titles_mut(&mut self) -> &mut Vec<String> {
        &mut self.titles
    }

    /// 総ターン数を返す。
    pub const fn total_turn(&self) -> u32 {
        self.total_turn
    }

    /// 総ターン数を設定する。
    pub fn set_total_turn(&mut self, n: u32) {
        self.total_turn = n;
    }

    pub fn messages(&self) -> &[String] {
        &self.messages
    }

    pub fn push_message(&mut self, msg: String) {
        self.messages.push(msg);
        // 直近 50 件で打ち切り（HUD 用）
        if self.messages.len() > 50 {
            let drop = self.messages.len() - 50;
            self.messages.drain(0..drop);
        }
    }

    pub fn set_turn_number(&mut self, n: u32) {
        self.turn.number = n;
    }

    /// MapView 上の現在カーソル位置（タイル）。
    pub const fn map_cursor(&self) -> Option<(u32, u32)> {
        self.map_cursor
    }

    /// SRC `Center` コマンド / シナリオロジック向け: マップカーソル (= 表示中心)
    /// を `(x, y)` にセット。マップ範囲外は no-op。
    pub fn set_map_cursor(&mut self, x: u32, y: u32) {
        if let Some(m) = self.database.map.as_ref() {
            if x < m.width && y < m.height {
                self.map_cursor = Some((x, y));
            }
        } else {
            self.map_cursor = Some((x, y));
        }
    }

    pub const fn turn(&self) -> Turn {
        self.turn
    }

    /// マップビューの左上タイル座標（スクロール offset）。
    pub const fn map_scroll(&self) -> (u32, u32) {
        self.map_scroll
    }

    /// カーソルが視界外に出たら自動スクロール。
    fn ensure_cursor_visible(&mut self) {
        let Some((cx, cy)) = self.map_cursor else {
            return;
        };
        let (ox, oy) = self.map_scroll;
        let vx = map_view::VIEW_TILES_X;
        let vy = map_view::VIEW_TILES_Y;

        if cx < ox {
            self.map_scroll.0 = cx;
        } else if cx >= ox + vx {
            self.map_scroll.0 = cx + 1 - vx;
        }
        if cy < oy {
            self.map_scroll.1 = cy;
        } else if cy >= oy + vy {
            self.map_scroll.1 = cy + 1 - vy;
        }

        // マップ範囲外に行かないようクランプ
        if let Some(map) = self.database.map.as_ref() {
            let max_ox = map.width.saturating_sub(vx);
            let max_oy = map.height.saturating_sub(vy);
            self.map_scroll.0 = self.map_scroll.0.min(max_ox);
            self.map_scroll.1 = self.map_scroll.1.min(max_oy);
        }
    }

    pub const fn scene(&self) -> Scene {
        self.scene
    }

    /// 外部からシーンを設定 (シナリオロード時の自動遷移など用)。
    ///
    /// 退場 scene の `cleared_on_exit()` に挙げられた transient state を
    /// このタイミングでクリアし、別 scene への描画漏出 (`script_overlay`
    /// が Title に残留する不具合 etc.) を仕組みで防ぐ。
    pub fn set_scene(&mut self, scene: Scene) {
        if self.scene != scene {
            for cleanup in self.scene.cleared_on_exit() {
                match cleanup {
                    crate::scene::SceneRead::ScriptOverlay => {
                        self.script_overlay.clear();
                    }
                    crate::scene::SceneRead::CommandMenu => {
                        self.command_menu = None;
                        self.action_mode = crate::command_menu::ActionMode::default();
                    }
                    // 現状の `cleared_on_exit` 宣言はこの 2 種のみ。
                    // 他種を追加した場合は対応 arm を足す (default で握り潰さない)。
                    other => log::warn!(
                        "[scene] cleared_on_exit に未対応の SceneRead variant: {other:?}"
                    ),
                }
            }
        }
        self.scene = scene;
    }

    pub const fn settings(&self) -> &Settings {
        &self.settings
    }

    pub const fn database(&self) -> &GameDatabase {
        &self.database
    }

    pub fn database_mut(&mut self) -> &mut GameDatabase {
        &mut self.database
    }

    // ── 武器選択ウィンドウ (攻撃側) ───────────────────────────────────────────

    // ── 精神コマンド (SP コマンド) ───────────────────────────────────────────
    //
    // SRC ([精神コマンド.md] / [SPコマンド]) のユニットコマンド「精神」。パイロットが
    // 習得済み (`level <= パイロットレベル`) の SP コマンドを、残り SP の範囲で発動する。
    // 戦闘側の効果 (集中/熱血/魂/必中/ひらめき/不屈/鉄壁/気合 等) は `combat.rs`
    // `predict_with_status` が `UnitInstance.conditions` を読んで反映するため、ここでは
    // SP を消費して 1 ターン (lifetime=1) の condition を付与する。`begin_phase` が当該陣営の
    // 次フェイズ開始で lifetime==1 の condition を解除する。

    // ───────────────────────── アビリティ ─────────────────────────

    // ───────────────────────── 母艦 / 発進 ─────────────────────────

    // ───────────────────────── 合体 / 分離 ─────────────────────────
}

#[cfg(test)]
mod tests;
