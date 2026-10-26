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

    /// 検証/デバッグ専用: オンマップ各ユニットの武器の発射可否を要約する (verify-archive が
    /// 「交戦が成立しない」原因 — 武器なし／必要技能 NG／必要気力不足／EN・残弾切れ — を切り分ける)。
    pub fn debug_firable_report(&self) -> Vec<String> {
        let mut out = Vec::new();
        for (idx, u) in self.database.unit_instances.iter().enumerate() {
            if u.off_map {
                continue;
            }
            let weapons = self
                .database
                .unit_by_name(&u.unit_data_name)
                .map(|d| d.weapons.clone())
                .unwrap_or_default();
            let parts: Vec<String> = weapons
                .iter()
                .map(|w| {
                    let mark = if self.weapon_firable(idx, w) {
                        "✓"
                    } else {
                        "✗"
                    };
                    format!(
                        "{mark}{}(r{} 気{} EN{} 技{:?})",
                        w.name,
                        w.max_range,
                        w.necessary_morale,
                        w.en_consumption,
                        w.necessary_skill()
                    )
                })
                .collect();
            let max_en = self.database.effective_max_en(u);
            let pilot_ok = self.database.pilot_by_name(&u.pilot_name).is_some();
            let combat_ok = self.database.effective_combat_data(idx).is_some();
            out.push(format!(
                "{}[{:?}]@({},{}) pilot={:?}{} combat_data={} 気力{} EN{}/{}: {}",
                u.unit_data_name,
                u.party,
                u.x,
                u.y,
                u.pilot_name,
                if pilot_ok { "(✓DB)" } else { "(✗DB欠)" },
                if combat_ok { "✓" } else { "✗None" },
                u.morale,
                max_en.saturating_sub(u.en_consumed),
                max_en,
                if parts.is_empty() {
                    "(武器なし)".to_string()
                } else {
                    parts.join(" ")
                }
            ));
        }
        out
    }

    /// 検証/デバッグ専用: パイロット不在 (`pilot_name` が DB に無い) の味方機に DB の
    /// 任意パイロットを乗せ、`effective_combat_data` を解決可能にする。キャラメイキングの
    /// 出撃導線 (exit/搭乗) をヘッドレスで完走できないシナリオ (D スパロボ戦記) で、
    /// 「piloted な味方が実マップで戦闘成立するか」を検証するための affordance。乗せた機数を返す。
    /// 通常プレイ経路からは呼ばれない。
    pub fn debug_seat_db_pilot(&mut self) -> usize {
        let Some(pilot) = self.database.pilots.first().map(|p| p.name.clone()) else {
            return 0;
        };
        let valid: std::collections::HashSet<String> = self
            .database
            .pilots
            .iter()
            .map(|p| p.name.clone())
            .collect();
        let mut n = 0;
        for u in self.database.unit_instances.iter_mut() {
            if u.party == crate::Party::Player && !u.off_map && !valid.contains(&u.pilot_name) {
                u.pilot_name = pilot.clone();
                n += 1;
            }
        }
        n
    }

    /// 検証/デバッグ専用: 現在のフェイズ党派のユニットを AI ロジックで一括行動させる。
    /// `run_ai_phase` を党派非依存に再利用 (ターゲットは `is_hostile_to` で解決されるため、
    /// 味方フェイズに呼べば味方が敵へ前進・攻撃する)。verify-archive のヘッドレス drive が
    /// 「味方フェイズに味方を自動行動させて戦闘を勝敗まで通す」ために使う。通常プレイの
    /// 手動操作には一切影響しない (このメソッドはエンジン内部から呼ばれない)。
    /// Battle 中の MapView でのみ作用し、行動後に勝敗判定を回す。
    pub fn debug_run_phase_ai(&mut self) {
        if self.scene == Scene::MapView && self.stage_state == crate::stage::StageState::Battle {
            self.run_ai_phase();
            self.check_victory();
        }
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

    /// 表示中の対話 UI（`Talk` / `Confirm`）。
    pub fn pending_dialog(&self) -> Option<&crate::dialog::PendingDialog> {
        self.pending_dialog.as_ref()
    }

    pub fn set_pending_dialog(&mut self, d: crate::dialog::PendingDialog) {
        self.pending_dialog = Some(d);
    }

    /// Talk の段階表示 (`:`) の残りページを設定する。`speaker` は継続ページ共通の
    /// 話者名。先頭ページは別途 `set_pending_dialog` で出す想定。
    pub fn set_talk_pages(&mut self, speaker: String, pages: Vec<String>) {
        self.talk_pages_speaker = speaker;
        self.talk_pages = pages;
    }

    /// 現在の pending dialog を捨てる (応答せず取り消す)。
    /// SRC `Cancel` コマンドや、シナリオ強制中断時に使う。`respond_dialog` と
    /// 違い script_var への結果代入は行わない。戻り値はキャンセル対象が
    /// 存在したか。
    pub fn cancel_pending_dialog(&mut self) -> bool {
        self.pending_dialog.take().is_some()
    }

    /// 対話 UI に応答してスクリプトを再開する。
    ///
    /// - `Talk` の場合: `choice` は無視。任意キーで進行。
    /// - `Confirm` の場合: 0=Yes / 1=No を `var_name` に代入。
    ///
    /// 戻り値: 応答した dialog がある場合 true。
    pub fn respond_dialog(&mut self, choice: u32) -> bool {
        // 精神コマンドのサブメニューは SP 消費 / condition 付与へ委譲。
        if self.pending_spirit.is_some() {
            return self.resolve_spirit(choice);
        }
        // 変形先サブメニューは形態変更へ委譲。
        if self.pending_transform.is_some() {
            return self.resolve_transform(choice);
        }
        // アビリティ一覧サブメニューは効果適用へ委譲。
        if self.pending_ability.is_some() {
            return self.resolve_ability(choice);
        }
        // 発進サブメニューは格納ユニットの出撃へ委譲。
        if self.pending_launch.is_some() {
            return self.resolve_launch(choice);
        }
        // 反撃モードのメニューは戦闘解決へ委譲 (script_var ではなく攻撃を実行)。
        if self.pending_reaction.is_some() {
            return self.resolve_reaction(choice);
        }
        // 武器選択メニューは選んだ武器で攻撃解決へ委譲。
        if self.pending_weapon_select.is_some() {
            return self.resolve_weapon_select(choice);
        }
        let Some(d) = self.pending_dialog.take() else {
            return false;
        };
        // キャンセル不可の Menu (キャンセル可 のない `Ask`) は choice 0 (Esc / 右クリック
        // / 任意進行) を受け付けず、ダイアログを維持して選択を強制する。
        // (キャンセルで 選択=0 になりキャラ未選択 → 味方0体 → 即敗北 を防ぐ。)
        if let crate::dialog::PendingDialog::Menu {
            non_cancellable: true,
            ..
        } = &d
        {
            if choice == 0 {
                self.pending_dialog = Some(d);
                return false;
            }
        }
        // Talk の段階表示 (`:`): 残りページがあれば次ページを出し、スクリプトは
        // 再開しない。最終ページまで送り切ってから通常の resume に進む。
        if matches!(d, crate::dialog::PendingDialog::Talk { .. }) && !self.talk_pages.is_empty() {
            let next = self.talk_pages.remove(0);
            let speaker = self.talk_pages_speaker.clone();
            self.pending_dialog = Some(crate::dialog::PendingDialog::Talk {
                speaker,
                body: next,
            });
            return true;
        }
        match &d {
            crate::dialog::PendingDialog::Confirm { var_name, .. } => {
                // SRC `Confirm` 仕様: `選択 = 1` で「はい」、`選択 = 0` で「いいえ」。
                // 本実装は内部で DialogYes → choice=0、DialogNo → choice=1 を
                // 取るので、SRC 流儀に合わせて反転する。`DialogChoice(n)` で
                // n>=2 が来た場合 (Talk から流用された誤入力など) は 0 として扱う。
                let v = match choice {
                    0 => "1", // Yes
                    1 => "0", // No
                    _ => "0",
                };
                self.set_script_var(var_name.clone(), v.to_string());
            }
            crate::dialog::PendingDialog::Menu {
                var_name,
                options,
                store_value,
                option_keys,
                ..
            } => {
                let picked = choice as usize;
                let val = if !option_keys.is_empty() && choice > 0 && picked <= option_keys.len() {
                    // SRC `Ask` Format 2: 選んだ要素の配列添字を格納する。
                    option_keys[picked - 1].clone()
                } else if *store_value && choice > 0 && picked <= options.len() {
                    options[picked - 1].clone()
                } else {
                    choice.to_string()
                };
                self.set_script_var(var_name.clone(), val);
            }
            crate::dialog::PendingDialog::Talk { .. } => {}
            crate::dialog::PendingDialog::WaitClick => {}
            crate::dialog::PendingDialog::Input {
                var_name, default, ..
            } => {
                // 数値応答だけが来た場合は default を残す
                self.set_script_var(var_name.clone(), default.clone());
            }
        }
        // スクリプト再開（失敗しても無視: シナリオ次第）。完了していれば
        // run_loop が `on_script_completed` で flow 継続を消化する。
        let _ = crate::event_runtime::resume(self);
        self.on_script_completed();
        self.return_from_intermission_subcommand_if_idle();
        self.auto_progress_stage_state_if_idle();
        true
    }

    /// `Wait Click` 系を **右クリック** で解除する（SRC の「右ボタン = キャンセル」）。
    /// Menu (Hotpoint) なら選択変数を空 (`選択 = ""`) にし、`KeyState(2)` が `1` を
    /// 返すフラグを立ててスクリプトを再開する。これによりステータス画面の
    /// `Wait Click` → `Case "" → If KeyState(2) Then Break` 等で画面を抜けられる。
    /// 対象は `Menu(store_value)` / `WaitClick` のみ。Talk / Confirm / Input は
    /// 右クリックを無視 (誤って進めない)。
    pub fn respond_dialog_right_click(&mut self) -> bool {
        // 武器選択窓は右クリックで取り消し (目標を選び直せる AttackSelect へ戻る)。
        if self.cancel_weapon_select() {
            return true;
        }
        // 対象は Hotpoint 付き `Wait Click` (store_value Menu + hotpoints) と、
        // 描画なしの `WaitClick` のみ。Talk / Confirm / Ask / Input は右クリックを
        // 無視する (左クリック判定 `is_hotpoint_menu` と同じ条件で線引き)。
        let is_hotpoint_menu = matches!(
            self.pending_dialog,
            Some(crate::dialog::PendingDialog::Menu {
                store_value: true,
                ..
            })
        ) && !self.hotpoints.is_empty();
        let is_wait_click = matches!(
            self.pending_dialog,
            Some(crate::dialog::PendingDialog::WaitClick)
        );
        if !is_hotpoint_menu && !is_wait_click {
            return false;
        }
        let Some(d) = self.pending_dialog.take() else {
            return false;
        };
        if let crate::dialog::PendingDialog::Menu { var_name, .. } = &d {
            // 選択 = "" (どの Hotpoint も選んでいない = キャンセル)
            self.set_script_var(var_name.clone(), String::new());
        }
        // KeyState(2) (右ボタン) を立てる。スクリプト再開後に評価される。
        self.set_wait_click_right(true);
        let _ = crate::event_runtime::resume(self);
        self.on_script_completed();
        self.return_from_intermission_subcommand_if_idle();
        self.auto_progress_stage_state_if_idle();
        true
    }

    /// `Input` モーダルへのテキスト応答。`var_name` に `text` を格納し再開。
    /// 他種の dialog だった場合は無視して `false`。
    pub fn respond_dialog_text(&mut self, text: String) -> bool {
        let Some(d) = self.pending_dialog.take() else {
            return false;
        };
        match &d {
            crate::dialog::PendingDialog::Input { var_name, .. } => {
                self.set_script_var(var_name.clone(), text);
            }
            // Input 以外の場合は元に戻して取り下げ
            other => {
                self.pending_dialog = Some(other.clone());
                return false;
            }
        }
        let _ = crate::event_runtime::resume(self);
        self.on_script_completed();
        self.return_from_intermission_subcommand_if_idle();
        self.auto_progress_stage_state_if_idle();
        true
    }

    /// 内部用: 中断中のスクリプトを保存 / 取り出し。
    pub fn take_script_context(&mut self) -> Option<crate::event_runtime::ScriptContext> {
        self.script_ctx.take()
    }
    pub fn set_script_context(&mut self, ctx: crate::event_runtime::ScriptContext) {
        self.script_ctx = Some(ctx);
    }
    pub fn has_script_context(&self) -> bool {
        self.script_ctx.is_some()
    }
    /// 中断中スクリプトの再開 PC。`Wait` / 対話命令でサスペンド中のみ `Some`。
    /// 同一 PC で何度もサスペンドし続ける = スクリプトが進行不能ループに
    /// 陥っている指標になる (test_harness の `Drain` が利用)。
    pub fn script_resume_pc(&self) -> Option<usize> {
        self.script_ctx.as_ref().map(|c| c.pc)
    }

    /// スクリプト完了通知 (docs/FLOW_REDESIGN.md §2.1)。
    ///
    /// `event_runtime::run_loop` がスクリプトの完了 (インライン完了 / resume
    /// 後の完了 / エラー終了) を検知するたびに呼ぶ。idle (スクリプト/対話/
    /// タイマのいずれも無い) な間、`flow` スタックの継続を pop して実行する。
    /// 継続の実行が新たなスクリプトを起動して suspend したら drain は止まり、
    /// そのスクリプトの完了時に再開される。
    ///
    /// 継続実行中に内側で完了したスクリプトからの再帰呼び出しは
    /// `flow_draining` ガードで弾く (外側のループが続きを処理する)。
    pub fn on_script_completed(&mut self) {
        if self.flow_draining {
            return;
        }
        // スクリプト実行中 (ネストした run_loop の完了など) は drain しない。
        // 最外殻の run_loop が完了したときにまとめて処理する。
        if self.script_depth > 0 {
            return;
        }
        self.flow_draining = true;
        // 暴走防止: `Continue` 同士のループバック (A→B→A…) のような
        // 終わらないチェインで WASM がハングしないよう、1 回の drain で
        // 処理する継続/イベント数に上限を置く (正常系では到達しない)。
        let mut steps: usize = 0;
        const DRAIN_STEP_LIMIT: usize = 256;
        while self.script_ctx.is_none()
            && self.pending_dialog.is_none()
            && self.pending_timer.is_none()
        {
            steps += 1;
            if steps > DRAIN_STEP_LIMIT {
                self.push_message(
                    "進行継続の処理が上限を超えました (Continue ループの可能性)".to_string(),
                );
                break;
            }
            // 割込みイベント (EventQue) を継続より先に消化する。原典の
            // 「HandleEvent はキューを処理し切ってから呼び出し元へ戻る」
            // (Event.bas) と同じ順序。
            if let Some((label, scope)) = self.event_queue.pop_front() {
                match scope {
                    // 章ローカルイベント: 当該ファイル内のラベルとして発火する。
                    // ファイル内に無ければ何もしない (他章へ漏らさない)。
                    Some(file) => {
                        crate::event_runtime::trigger_label_in_file(self, &file, &label);
                    }
                    None => {
                        crate::event_runtime::trigger_label(self, &label);
                    }
                }
                continue;
            }
            let Some(cont) = self.flow.pop() else {
                break;
            };
            self.run_flow_cont(cont);
        }
        self.flow_draining = false;
    }

    /// 割込みイベントを投函する (原典 `Event.bas::EventQue` への追加に相当)。
    ///
    /// `label` が script_library に定義されていればキューに積んで `true`。
    /// 未定義なら何もせず `false` (呼び出し側の「候補を順に試して最初の 1 件
    /// だけ発火」ロジックのため、存在判定は投函時に行う)。
    ///
    /// スクリプト実行中でなければ投函と同時に drain して即実行する
    /// (従来の即時 `trigger_label` と同じタイミング)。実行中なら現在の
    /// スクリプト完了後に FIFO で実行される (再入による ctx 上書きを防ぐ)。
    pub(crate) fn post_event_label(&mut self, label: String) -> bool {
        if self.script_library.label_pc(&label).is_none() {
            return false;
        }
        self.event_queue.push_back((label, None));
        if self.script_depth == 0 {
            self.on_script_completed();
        }
        true
    }

    /// 章ローカルな自動発火イベント (`ターン` / `破壊` / `全滅` / `損傷率` /
    /// `会話` / `勝利条件` 等) を **現ステージファイルにスコープして** 投函する。
    ///
    /// `post_event_label` (global) と違い、現ステージファイル内に `label` が
    /// 定義されていなければ投函しない (`false`)。全 22 章を 1 ライブラリに同時
    /// ロードする本実装では、章ローカルイベントを global 解決すると、現章に
    /// 当該イベントが無いとき別章の同名ラベル (例: 01 章の敵フェイズで `ターン
    /// 1 敵` を引くと 12 章のそれにヒット) が誤発火して「話が飛ぶ」。原典 SRC は
    /// シナリオを 1 本ずつロードするため衝突しない挙動を、ファイルスコープで再現する。
    ///
    /// `current_stage_file` が未設定 (単一ファイル / テスト) の場合は従来どおり
    /// global 解決にフォールバックする。
    pub(crate) fn post_stage_event_label(&mut self, label: String) -> bool {
        if self.current_stage_file.is_empty() {
            // ステージファイル概念が無い経路: 従来どおり global。
            return self.post_event_label(label);
        }
        if self
            .script_library
            .label_pc_in_file(&self.current_stage_file, &label)
            .is_none()
        {
            return false;
        }
        let file = self.current_stage_file.clone();
        self.event_queue.push_back((label, Some(file)));
        if self.script_depth == 0 {
            self.on_script_completed();
        }
        true
    }

    /// 指定ファイル (`file` = basename / パス。例: `GameOver.eve`) 内の `label` を
    /// EventQue にスコープ投函する。[`post_stage_event_label`] の任意ファイル版で、
    /// `current_stage_file` に依らず特定の .eve のラベルを発火したいとき (SRC 本体の
    /// `Data/System/GameOver.eve::プロローグ` 等) に使う。
    ///
    /// 当該ファイルに `label` が無ければ投函せず `false`。
    pub(crate) fn post_event_label_in_file(&mut self, file: &str, label: String) -> bool {
        if self.script_library.label_pc_in_file(file, &label).is_none() {
            return false;
        }
        self.event_queue.push_back((label, Some(file.to_string())));
        if self.script_depth == 0 {
            self.on_script_completed();
        }
        true
    }

    /// スクリプトが要求した再ロード用 JSON を登録する (`Quickload` / `Restart` 等)。
    pub(crate) fn request_reload(&mut self, json: String) {
        self.pending_reload = Some(json);
    }

    /// フロントエンドが再ロード要求を取り出す。`Some(json)` なら
    /// [`App::from_save_json`] で `self` を置換し [`App::fire_resume_event`] を呼ぶこと。
    pub fn take_pending_reload(&mut self) -> Option<String> {
        self.pending_reload.take()
    }

    /// `event_runtime::run_loop` の実行ネスト深さの増減。
    pub(crate) fn enter_script_run(&mut self) {
        self.script_depth += 1;
    }
    pub(crate) fn exit_script_run(&mut self) {
        self.script_depth = self.script_depth.saturating_sub(1);
    }
    pub(crate) fn script_run_depth(&self) -> usize {
        self.script_depth
    }

    /// 継続 1 件の実行。VB5 原典の「`HandleEvent` から戻った直後のコード」に相当。
    fn run_flow_cont(&mut self, cont: crate::flow::FlowCont) {
        use crate::flow::FlowCont;
        match cont {
            FlowCont::AfterStartEvent => {
                // 原典 SRC.bas `StartScenario` 末尾: HandleEvent "スタート" の
                // 完了後に StartTurn "味方"。
                self.begin_phase(crate::Phase::Player);
                self.center_view_on_first_player_unit();
            }
            FlowCont::ReturnToIntermissionMenu => {
                self.intermission_running = false;
                self.scene = Scene::Intermission;
                self.intermission_mode = IntermissionMode::Menu;
                self.script_overlay.clear();
                self.hotpoints.clear();
            }
            FlowCont::AfterStageFileRun => {
                // `Continue` ループバック (ステージファイルが `次ステージ` を
                // 再予約した) 場合はインターミッションに留まる。
                if !self.script_var("次ステージ").is_empty() {
                    return;
                }
                if !matches!(
                    self.stage_state,
                    crate::stage::StageState::Briefing | crate::stage::StageState::Sortie
                ) {
                    return;
                }
                if self.consume_stage_start_ran() {
                    // ステージファイルが `スタート` ラベルを通過実行済み →
                    // 再発火せず (敵の二重配置防止) Battle 状態に入って
                    // 味方フェイズを開始する。
                    self.enter_battle_state();
                    self.begin_phase(crate::Phase::Player);
                    self.center_view_on_first_player_unit();
                } else {
                    // `スタート` 未実行 → 通常経路で発火する (`begin_battle` が
                    // `current_stage_file` スコープで `スタート` を起動し、
                    // 完了後に AfterStartEvent 継続が味方フェイズを開始する)。
                    self.begin_sortie();
                    self.begin_battle();
                }
            }
            FlowCont::LoadNextStage => {
                // 原典: IsScenarioFinished → StartScenario(次ステージ)。
                // 予約が無ければ no-op (シナリオ完結)。
                let _ = self.advance_to_next_stage();
            }
        }
    }

    /// シナリオ遷移 (`Continue <file>` / 大域中断) に伴う進行状態の巻き戻し。
    /// 原典の `IsScenarioFinished` によるコールスタック巻き戻しに相当し、
    /// 旧ステージの継続 (AfterStartEvent 等) と未消化の割込みイベントを
    /// 破棄する (docs/FLOW_REDESIGN.md §2.2 規則 3)。
    pub(crate) fn scenario_transition_reset(&mut self) {
        self.flow.clear();
        self.event_queue.clear();
    }

    /// flow 継続を積む (event_runtime / オーケストレーション用)。
    pub fn push_flow_cont(&mut self, cont: crate::flow::FlowCont) {
        self.flow.push(cont);
    }

    /// `スタート` / `Start` ラベル行 (PC) をスクリプト実行が通過した
    /// (= スタートイベントの中身がインライン実行された) ことを記録する。
    /// event_runtime::run_loop_inner から呼ばれる。
    pub(crate) fn mark_start_label_passed(&mut self, pc: usize) {
        self.start_passed_pcs.push(pc);
    }

    /// 現ステージ (`current_stage_file`) の `スタート` セクションがインライン
    /// 実行済みかを判定し、記録を消費 (クリア) する。
    /// `current_stage_file` が未設定 / library 未登録の場合は「いずれかの
    /// `スタート` を通過したか」にフォールバックする (start_scenario 等の
    /// ファイル概念が無い経路)。
    fn consume_stage_start_ran(&mut self) -> bool {
        let ran = if self.current_stage_file.is_empty() {
            !self.start_passed_pcs.is_empty()
        } else if let Some((s, e)) = self
            .script_library
            .find_file(&self.current_stage_file)
            .map(|f| (f.start_pc, f.end_pc))
        {
            self.start_passed_pcs.iter().any(|&pc| pc >= s && pc < e)
        } else {
            !self.start_passed_pcs.is_empty()
        };
        self.start_passed_pcs.clear();
        ran
    }

    /// アーカイブロード完了後のステージブートストラップ (フロントエンド /
    /// verify-archive のロードオーケストレーションから呼ぶ)。
    ///
    /// `Stage` コマンドも `Continue` チェインも使わないシナリオ (エントリ
    /// .eve のプロローグ実行だけで終わる型。例: 東中無双２) は、原典
    /// `StartScenario` の後半「プロローグ完了 → `スタート` 発火 → 味方
    /// フェイズ」を駆動する者が居らず Briefing で停止していた。エントリ
    /// .eve を現ステージファイルとして `AfterStageFileRun` 継続を積み、
    /// idle なら即進行する (プロローグが suspend 中なら完了時に進行)。
    pub fn bootstrap_stage_after_load(&mut self, entry_file: &str) {
        // インターミッション制はメニュー操作 (「次のステージへ」) で進行する。
        //
        // ただし `IntermissionCommand` は「インターミッションのメニューに項目を
        // 追加する」だけの命令であり (`IntermissionCommandコマンド.md`)、
        // シナリオがインターミッション制であることを意味しない。ステージ制の
        // シナリオもプロローグで辞典・ショップ等を登録するのに使う
        // (例: 多次元世界エニバース の `IntermissionCommand "エニバース辞典" …`)。
        // 起動ファイルが `スタート` / `Start` を持つならステージ制と判断して
        // ブートストラップする。
        if !self.intermission_commands.is_empty() && !self.file_has_start_label(entry_file) {
            return;
        }
        if self.stage_state != crate::stage::StageState::Briefing {
            return;
        }
        // 既にチェイン進行中 (`Continue` が LoadNextStage を予約 / advance が
        // AfterStageFileRun を積載) なら二重に積まない。
        if !self.flow.is_empty() {
            return;
        }
        // ユニット/パイロット定義がなければ素材パック (ライブラリ .eve のみ収録)
        // であり、シナリオとしてブートストラップする必要がない。
        if self.database.units.is_empty() && self.database.pilots.is_empty() {
            return;
        }
        if self.current_stage_file.is_empty() && !entry_file.is_empty() {
            self.current_stage_file = entry_file.to_string();
        }
        self.flow.push(crate::flow::FlowCont::AfterStageFileRun);
        self.on_script_completed();
    }

    /// 指定ファイルが `スタート` / `Start` ラベルを定義しているか。
    fn file_has_start_label(&self, file_path: &str) -> bool {
        if file_path.is_empty() {
            return false;
        }
        let lib = self.script_library();
        lib.label_pc_in_file(file_path, "スタート").is_some()
            || lib.label_pc_in_file(file_path, "Start").is_some()
    }

    /// デバッグ用: App の主要状態を 1 行 JSON 風文字列で返す。
    /// フロントエンドが `window.__srcDebug()` 経由で吸い出して
    /// シナリオ進行の詰まり箇所を診断する。
    pub fn debug_summary(&self) -> String {
        let dialog = match &self.pending_dialog {
            None => "none".to_string(),
            Some(crate::dialog::PendingDialog::Talk { speaker, .. }) => {
                format!("Talk({speaker})")
            }
            Some(crate::dialog::PendingDialog::WaitClick) => "WaitClick".to_string(),
            Some(crate::dialog::PendingDialog::Confirm { .. }) => "Confirm".to_string(),
            Some(crate::dialog::PendingDialog::Menu { options, .. }) => {
                format!("Menu({})", options.len())
            }
            Some(crate::dialog::PendingDialog::Input { .. }) => "Input".to_string(),
        };
        let hp_names: Vec<&str> = self
            .hotpoints
            .iter()
            .take(12)
            .map(|h| h.name.as_str())
            .collect();
        let pcount = |p: crate::Party| {
            self.database
                .unit_instances
                .iter()
                .filter(|u| u.party == p && !u.off_map)
                .count()
        };
        format!(
            "scene={:?} stage_state={:?} dialog={dialog} script_ctx={} \
             flow={:?} pending_timer={:?} hotpoints={}{:?} overlay_cmds={} \
             intermission_cmds={} 次ステージ={:?} units={} \
             parties=[味:{} 敵:{} 中:{} Ｎ:{}] file={:?} \
             victory[全滅敵={} 全滅中立={} クリア={}] messages={} \
             last_msg={:?} script_err={:?}",
            self.scene,
            self.stage_state,
            self.script_ctx.is_some(),
            self.flow,
            self.pending_timer,
            self.hotpoints.len(),
            hp_names,
            self.script_overlay.cmds.len(),
            self.intermission_commands.len(),
            self.script_var("次ステージ"),
            self.database.unit_instances.len(),
            pcount(crate::Party::Player),
            pcount(crate::Party::Enemy),
            pcount(crate::Party::Neutral),
            pcount(crate::Party::Npc),
            self.current_stage_file,
            self.stage_defines_label("全滅 敵"),
            self.stage_defines_label("全滅 中立"),
            self.stage_defines_label("クリア"),
            self.messages.len(),
            self.messages.last().map(String::as_str).unwrap_or(""),
            self.last_script_error,
        )
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

    /// パスを正規化する (`\` → `/`、小文字化、クォート / 前後空白除去)。
    fn vfs_normalize(path: &str) -> String {
        path.trim()
            .trim_matches('"')
            .replace('\\', "/")
            .to_lowercase()
    }

    /// ファイルを開いてハンドル文字列を返す。
    /// `mode`: `入力`/`Input` = 読込、`出力`/`Output` = 書込 (切詰)、
    /// `追加`/`追加出力`/`Append` = 追記。
    pub fn vfs_open(&mut self, path: &str, mode: &str) -> String {
        let norm = Self::vfs_normalize(path);
        let m = mode.trim().trim_matches('"');
        let write = !matches!(m, "入力" | "Input" | "input");
        let append = matches!(m, "追加" | "追加出力" | "Append" | "append");
        if write && !append {
            self.virtual_files.insert(norm.clone(), Vec::new());
        } else {
            self.virtual_files.entry(norm.clone()).or_default();
        }
        let handle = format!("__fh{}", self.next_file_handle);
        self.next_file_handle = self.next_file_handle.wrapping_add(1);
        self.open_files.insert(
            handle.clone(),
            OpenFileHandle {
                path: norm,
                write,
                read_cursor: 0,
            },
        );
        handle
    }

    /// `s` が開いているファイルハンドルか。
    pub fn vfs_is_handle(&self, s: &str) -> bool {
        self.open_files.contains_key(s)
    }

    /// 開いているファイルに 1 行追記する。
    pub fn vfs_print(&mut self, handle: &str, line: String) {
        if let Some(h) = self.open_files.get(handle) {
            let path = h.path.clone();
            self.virtual_files.entry(path).or_default().push(line);
        }
    }

    /// 開いているファイルから次の 1 行を読む。末尾 / 不正ハンドルなら `None`。
    pub fn vfs_read_line(&mut self, handle: &str) -> Option<String> {
        let (path, cursor) = {
            let h = self.open_files.get(handle)?;
            (h.path.clone(), h.read_cursor)
        };
        let line = self.virtual_files.get(&path)?.get(cursor).cloned();
        if line.is_some() {
            if let Some(h) = self.open_files.get_mut(handle) {
                h.read_cursor += 1;
            }
        }
        line
    }

    /// ファイルを閉じる。書き込みモードで閉じたファイルが pilot/unit 等の
    /// データファイルなら内容を再パースして GameDatabase に加法マージする
    /// (キャラメイキングが書き出したパイロットを使用可能にする)。
    pub fn vfs_close(&mut self, handle: &str) {
        let Some(h) = self.open_files.remove(handle) else {
            return;
        };
        if h.write {
            self.vfs_reload_data_file(&h.path);
        }
    }

    /// 仮想ファイルがデータファイル (`pilot.txt` 等) ならパースして DB に
    /// 加法マージする。パース失敗は黙って無視する。
    fn vfs_reload_data_file(&mut self, path: &str) {
        let basename = path.rsplit('/').next().unwrap_or(path);
        let Some(lines) = self.virtual_files.get(path) else {
            return;
        };
        let text = lines.join("\n");
        match basename {
            "pilot.txt" => {
                // 壊れた 1 レコードがあっても残りのパイロットは取り込む。
                let (pilots, _errors) = crate::data::pilot::parse_lenient(&text);
                self.database.extend_pilots(pilots);
            }
            "unit.txt" | "robot.txt" => {
                // 壊れた 1 レコードがあっても残りのユニットは取り込む。
                let (units, _errors) = crate::data::unit::parse_lenient(&text);
                self.database.extend_units(units);
            }
            "item.txt" => {
                let (items, _errors) = crate::data::item::parse_lenient(&text);
                self.database.extend_items(items);
            }
            _ => {}
        }
    }

    /// 仮想ファイルの全行を返す (テスト / デバッグ用)。
    pub fn virtual_file_lines(&self, path: &str) -> Option<&[String]> {
        self.virtual_files
            .get(&Self::vfs_normalize(path))
            .map(Vec::as_slice)
    }

    /// 仮想ファイルが存在すれば basename を返す。`Dir(path, …)` 用。
    pub fn virtual_file_basename_if_exists(&self, path: &str) -> Option<String> {
        let norm = Self::vfs_normalize(path);
        if self.virtual_files.contains_key(&norm) {
            Some(norm.rsplit('/').next().unwrap_or(&norm).to_string())
        } else {
            None
        }
    }

    /// `FileExists(path)` — VFS に該当ファイルがあるなら true。
    pub fn virtual_file_exists(&self, path: &str) -> bool {
        let norm = Self::vfs_normalize(path);
        self.virtual_files.contains_key(&norm)
    }

    /// `FileLen(path)` — VFS 上のファイルサイズ (バイト数 = 行内 UTF-8 バイト
    /// + 改行 1 バイト/行)。未登録なら 0。
    pub fn virtual_file_len(&self, path: &str) -> u64 {
        let norm = Self::vfs_normalize(path);
        match self.virtual_files.get(&norm) {
            Some(lines) => lines.iter().map(|l| l.len() as u64 + 1).sum::<u64>(),
            None => 0,
        }
    }

    /// `Loc(handle)` — 開いているファイルの読み取りカーソル (行番号、0 起点)。
    /// 不正ハンドルなら 0。
    pub fn vfs_loc(&self, handle: &str) -> u64 {
        self.open_files
            .get(handle)
            .map(|h| h.read_cursor as u64)
            .unwrap_or(0)
    }

    /// `EOF(handle)` — 末尾 (= 読取可能な行が無い) なら true。
    pub fn vfs_eof(&self, handle: &str) -> bool {
        let Some(h) = self.open_files.get(handle) else {
            return true;
        };
        let Some(lines) = self.virtual_files.get(&h.path) else {
            return true;
        };
        h.read_cursor >= lines.len()
    }

    /// `LOF(handle)` — 開いているファイルの総行数。不正ハンドルなら 0。
    pub fn vfs_lof(&self, handle: &str) -> u64 {
        let Some(h) = self.open_files.get(handle) else {
            return 0;
        };
        self.virtual_files
            .get(&h.path)
            .map(|lines| lines.len() as u64)
            .unwrap_or(0)
    }

    /// `CreateFolder` — VFS に空フォルダエントリを作る。
    /// 実際の BTreeMap には `path/` キーで空 Vec を挿入する。
    pub fn vfs_ensure_folder(&mut self, norm_path: &str) {
        self.virtual_files.entry(norm_path.to_string()).or_default();
    }

    /// `RemoveFolder` — 指定パス以下のファイルを全削除する。
    pub fn vfs_remove_folder(&mut self, prefix: &str) {
        let prefix_slash = if prefix.ends_with('/') {
            prefix.to_string()
        } else {
            format!("{prefix}/")
        };
        self.virtual_files
            .retain(|k, _| !k.starts_with(&prefix_slash) && k != prefix);
    }

    /// `RemoveFile` — 仮想ファイルを削除する。
    pub fn vfs_remove_file(&mut self, path: &str) {
        let norm = Self::vfs_normalize(path);
        self.virtual_files.remove(&norm);
    }

    /// `RenameFile` — 仮想ファイルのパスを変更する。
    pub fn vfs_rename_file(&mut self, old_path: &str, new_path: &str) {
        let old_norm = Self::vfs_normalize(old_path);
        let new_norm = Self::vfs_normalize(new_path);
        if let Some(content) = self.virtual_files.remove(&old_norm) {
            self.virtual_files.insert(new_norm, content);
        }
    }

    /// `CopyFile` — 仮想ファイルをコピーする。
    pub fn vfs_copy_file(&mut self, src_path: &str, dst_path: &str) {
        let src_norm = Self::vfs_normalize(src_path);
        let dst_norm = Self::vfs_normalize(dst_path);
        if let Some(content) = self.virtual_files.get(&src_norm).cloned() {
            self.virtual_files.insert(dst_norm, content);
        }
    }

    pub const fn stage_state(&self) -> crate::stage::StageState {
        self.stage_state
    }

    pub fn briefing(&self) -> &str {
        &self.briefing
    }

    pub fn set_briefing(&mut self, text: String) {
        self.briefing = text;
    }

    pub fn set_stage_state(&mut self, s: crate::stage::StageState) {
        self.stage_state = s;
    }

    // ===== ステージ進行 API（元 SRC `SRC.play.cs::StartScenario` / `StartTurn` 系）=====
    //
    // 上位のシーン進行を以下のメソッドに集約する:
    //
    //   start_scenario(name)         // Title/Configuration から呼ぶ
    //     │ Prologue / プロローグ ラベル発火
    //     ▼
    //   StageState::Briefing
    //     │ begin_sortie()
    //     ▼
    //   StageState::Sortie
    //     │ begin_battle()
    //     │   Start / スタート ラベル発火
    //     │   begin_phase(Player) → "ターン 1" + "ターン 1 味方" 発火
    //     ▼
    //   StageState::Battle  (end_phase で begin_phase(Enemy)→AI→…→begin_phase(Player(T+1)) を回す)
    //     │ game_clear()  ─▶ Victory  (Victory / 勝利 ラベル発火)
    //     │ game_over()   ─▶ Defeat   (GameOver / ゲームオーバー ラベル発火)
    //     ▼
    //   終了オーバーレイ → Title へ戻す

    /// 元 SRC `StartScenario(fname)` の Rust 版エントリ。
    ///
    /// `name` は表示用のシナリオ名（HUD 表示や save 用のキーになる文字列）。
    /// `.eve` のロード自体は呼び出し側で済ませておく前提で、ここでは:
    ///   1. ステージ進行状態を `Briefing` にリセット
    ///   2. ターン / メニュー / アクションモードを初期化
    ///   3. シーンを MapView に遷移
    ///   4. 自動発火: `Prologue` / `プロローグ` ラベル
    ///
    /// が走る。SRC では `Stage = "プロローグ"` 相当。
    pub fn start_scenario(&mut self, name: impl Into<String>) {
        self.stage = name.into();
        self.stage_state = crate::stage::StageState::Briefing;
        self.turn = Turn::new();
        self.command_menu = None;
        self.action_mode = crate::command_menu::ActionMode::Browse;
        self.scene = Scene::MapView;
        self.settings_snapshot = None;
        if !self.stage.is_empty() {
            self.push_message(format!("シナリオ「{}」開始", self.stage));
        }
        // 元 SRC: Event.HandleEvent("プロローグ")。通過実行の観測のため
        // 発火前に通過記録をリセットする。
        self.start_passed_pcs.clear();
        crate::event_runtime::trigger_label(self, "Prologue");
        crate::event_runtime::trigger_label(self, "プロローグ");
        // 原典 SRC `StartScenario`: プロローグ完了後に `スタート` を発火して
        // Battle へ (SRC.bas L1208-1262)。「完了後」を継続として積む。プロローグ
        // が `スタート` ラベルを通過実行していた場合 (start_passed_pcs) は
        // AfterStageFileRun が再発火を抑止する。
        self.flow.push(crate::flow::FlowCont::AfterStageFileRun);
        self.on_script_completed();
    }

    /// 元 SRC: Briefing → Sortie（出撃ユニット確認）。重複呼び出しは無視。
    pub fn begin_sortie(&mut self) {
        if self.stage_state != crate::stage::StageState::Briefing {
            return;
        }
        self.stage_state = crate::stage::StageState::Sortie;
        self.push_message("出撃準備フェーズ".to_string());
    }

    /// 元 SRC `StartTurn("味方")` 相当: Sortie → Battle に遷移して `Start` ラベル
    /// を発火する。`スタート` の **完了後** (インライン完了でも suspend 後の
    /// 完了でも) `FlowCont::AfterStartEvent` 継続が Player フェーズを開始する。
    /// Sortie 以外からの呼び出しは無視。
    pub fn begin_battle(&mut self) {
        if self.stage_state != crate::stage::StageState::Sortie {
            return;
        }
        self.enter_battle_state();
        // 元 SRC: Event.HandleEvent("スタート") → 完了後に StartTurn "味方"
        // (SRC.bas `StartScenario` 末尾)。「完了後」を継続として積んでから発火。
        self.flow.push(crate::flow::FlowCont::AfterStartEvent);
        // `スタート` / `Start` ラベルは各ステージ .eve に個別に存在するため、
        // 現ステージファイルが分かるならそのファイルスコープで発火する。
        // グローバル発火だと先頭登録ファイルの `スタート` を誤って実行し、
        // 当該ステージの敵配置 (`Call 敵配置` 等) が走らない。
        let stage_file = self.current_stage_file.clone();
        let fired = !stage_file.is_empty()
            && (crate::event_runtime::trigger_label_in_file(self, &stage_file, "Start")
                || crate::event_runtime::trigger_label_in_file(self, &stage_file, "スタート"));
        if !fired {
            crate::event_runtime::trigger_label(self, "Start");
            crate::event_runtime::trigger_label(self, "スタート");
        }
        // `スタート` が未定義で何も起動しなかった場合はここで継続を消化して
        // 味方フェイズを開始する (起動した場合は run_loop の完了通知が消化
        // 済み / suspend 中なら resume 完了時に消化される)。
        self.on_script_completed();
    }

    /// Battle 状態への突入処理 (`スタート` 発火と味方フェイズ開始は含まない)。
    /// `begin_battle` と `FlowCont::AfterStageFileRun` (スタート通過実行済みの
    /// ステージファイル) の共通部。
    fn enter_battle_state(&mut self) {
        // Briefing/Sortie フェーズでスクリプトが PaintString した描画は
        // 戦闘開始時にクリア。scene は MapView のままなので `set_scene` の
        // cleared_on_exit では拾われない。
        self.script_overlay.clear();
        self.hotpoints.clear();
        self.stage_state = crate::stage::StageState::Battle;
        // 原典 SRC `SRC.cs::StartScenario`: マップ未ロードのままステージを開始する
        // 場合はデフォルト 15×15 マップを生成する (`if Map.MapWidth == 1:
        // SetMapSize(15, 15)`)。`ChangeMap` を持たないステージ (例: 東方夢想伝
        // 01〜11) はこの暗黙マップ上で `Create` 配置・戦闘を行う。`スタート`
        // 内で `ChangeMap` / `MapSize` する場合は後勝ちで差し替わる。
        if self.database.map.is_none() {
            self.database
                .replace_map(crate::data::map::MapData::new(15, 15));
        }
        // SRC `Restartコマンド` 用に、戦闘開始時点のスナップショットを
        // `__restart_save` script_var に保存しておく。`_リスタート.src` 相当。
        // 失敗時は無視 (Restart コマンドが no-op になるだけ)。
        if let Ok(json) = self.to_save_json() {
            self.set_script_var("__restart_save".to_string(), json);
        }
    }

    /// 元 SRC `StartTurn(uparty)` の冒頭処理: フェイズに入ったときの初期化と
    /// ターンラベル発火。呼び出し側は事前に `turn.number` を確定させておくこと
    /// （Neutral → Player のラップで +1 する責務は `end_phase` 側にある）。
    ///
    /// - `has_acted` をリセット
    /// - コマンドメニュー / 行動モードを Browse に戻す
    /// - Player フェイズなら "ターン N 開始" HUD メッセージ
    /// - 自動発火: `Turn N` / `ターン N`（Player のみ）と
    ///   `Turn N <stage>` / `ターン N <stage>`（全フェイズ）
    fn begin_phase(&mut self, phase: crate::Phase) {
        self.turn.phase = phase;
        let party = phase.party();
        // フェイズ開始時の状態異常処理:
        // - 毒: 最大 HP の 10% ダメージ（撃破は防ぐため最低 HP 1）
        // - 1 回限定 (lifetime==1) の精神コマンド系 (必中 / 集中 / ひらめき / 熱血 /
        //   魂 / 気合 / 不屈 / 鉄壁 等) は当該陣営フェイズ開始時に解除する。
        //   `clear_one_turn_conditions` と同じ「lifetime==1 = 発動ターン限り」の規約に
        //   従い、ユニットコマンド「精神」で付与した全コマンドを一律に解除する。
        for i in 0..self.database.unit_instances.len() {
            if self.database.unit_instances[i].party != party {
                continue;
            }
            // 改造・強化パーツ・ボスランクを反映した実効最大 HP を基準にする
            // (毒ダメージ / 死の宣告 が育成後の HP に追従する)。
            let max_hp = self
                .database
                .effective_max_hp(&self.database.unit_instances[i]);
            let u = &mut self.database.unit_instances[i];
            // 毒: 最大 HP の 10%。毒属性への弱点で倍・耐性で半減 (特殊効果攻撃属性.md)。
            if u.has_condition("毒") && max_hp > 0 {
                let weak_poison = crate::feature::feature_value(&u.active_features, "弱点")
                    .is_some_and(|v| v.split_whitespace().any(|t| t == "毒"));
                let resist_poison = crate::feature::feature_value(&u.active_features, "耐性")
                    .is_some_and(|v| v.split_whitespace().any(|t| t == "毒"));
                let base = (max_hp / 10).max(1);
                let damage = if weak_poison {
                    base * 2
                } else if resist_poison {
                    (base / 2).max(1)
                } else {
                    base
                };
                let new_dmg = (u.damage + damage).min(max_hp - 1);
                u.damage = new_dmg;
            }
            // 死の宣告 (告): 期限切れ (lifetime≤1=この自軍フェイズで解ける) になると
            // HP が 1 になる (特殊効果攻撃属性.md)。tick_conditions より前に判定する。
            if max_hp > 0
                && u.conditions
                    .iter()
                    .any(|c| c.name == "死の宣告" && !c.is_permanent() && c.lifetime <= 1)
            {
                u.damage = (max_hp - 1).max(0);
            }
            // 状態異常の経過: 有限 lifetime を 1 減らし、0 以下で解除 (永続=-1 は保持)。
            // 旧実装は lifetime==1 のみ除去し 2 以上を永久に残していたため、特殊効果
            // 攻撃属性 (縛/痺/凍 等) の複数ターン状態が解除されなかった。当該陣営の
            // フェイズが来るたびに 1 ターン経過 (SRC `特殊効果攻撃属性.md`)。
            u.tick_conditions();
        }
        // 魅了 (特殊効果攻撃属性 魅) の期限切れ復帰: `魅了` condition が上の tick で消えた
        // ユニットは元の勢力 (charm_revert_party) へ戻し、設定した護衛 ai_mode と退避
        // フィールドをクリアする。魅了ユニットの勢力は攻撃側 (= 現フェイズ party になり得る)
        // へ移っているため、tick の直後にここで判定する。憑依 (恒久支配) は
        // charm_revert_party を使わないため対象外 (復帰しない)。
        for u in &mut self.database.unit_instances {
            if u.charm_revert_party.is_some() && !u.has_condition("魅了") {
                if let Some(orig) = u.charm_revert_party.take() {
                    u.party = orig;
                }
                if u.ai_mode.starts_with("護衛") {
                    u.ai_mode.clear();
                }
            }
        }
        // 基礎 EN 自然回復 (VB5 `Unit.cls:28182`「ＥＮは毎ターン5回復」): 当該陣営の
        // 全ユニットは、特殊能力 ＥＮ回復 の有無に関わらずフェイズ開始時に EN +5 される
        // (MaxEN 超過分は en_consumed の `.max(0)` クランプで自動的に切り捨てられる)。
        // `回復不能` (特殊効果攻撃属性 害) のユニットは対象外。VB5 の
        // `ＥＮ自然回復無効` オプションや `IsSpecialPowerInEffect("回復不能")` 判定、
        // 味方最初のターンのスキップは本実装では対象外 (下の特殊能力回復と同様に
        // `回復不能` condition のみを見る)。霊力回復は本実装では別途 (未対応)。
        for i in 0..self.database.unit_instances.len() {
            if self.database.unit_instances[i].party != party {
                continue;
            }
            let inst = &self.database.unit_instances[i];
            if inst.has_condition("回復不能") {
                continue;
            }
            self.database.unit_instances[i].en_consumed =
                (self.database.unit_instances[i].en_consumed - 5).max(0);
        }
        // 回復系特殊能力 (`回復系特殊能力.md`): 当該陣営フェイズ開始時に、
        // ＨＰ回復Lv*/ＥＮ回復Lv* は実効最大値の 10×Lv% を回復、ＨＰ消費Lv*/ＥＮ消費Lv*
        // は同率を減少させる (ＨＰ は最低 1 / ＥＮ は最低 0)。
        // ※ 基礎 EN 回復 (毎ターン 5) は上のブロックで、霊力 (プラーナ) 回復は下のブロックで実施。
        for i in 0..self.database.unit_instances.len() {
            if self.database.unit_instances[i].party != party {
                continue;
            }
            let inst = &self.database.unit_instances[i];
            let feats = &inst.active_features;
            let hp_heal = crate::feature::feature_level(feats, "ＨＰ回復");
            let hp_drain = crate::feature::feature_level(feats, "ＨＰ消費");
            let en_heal = crate::feature::feature_level(feats, "ＥＮ回復");
            let en_drain = crate::feature::feature_level(feats, "ＥＮ消費");
            if hp_heal.is_none() && hp_drain.is_none() && en_heal.is_none() && en_drain.is_none() {
                continue;
            }
            // 回復不能 (特殊効果攻撃属性 害): 特殊能力・地形による HP/EN 自然回復を
            // 阻害する (アビリティ/精神による回復は別経路なので影響しない)。消費系は継続。
            let no_regen = inst.has_condition("回復不能");
            let eff_max_hp = self.database.effective_max_hp(inst);
            let eff_max_en = self.database.effective_max_en(inst);
            let u = &mut self.database.unit_instances[i];
            if let Some(lv) = hp_heal {
                if !no_regen {
                    let heal = eff_max_hp * i64::from(10 * lv) / 100;
                    u.damage = (u.damage - heal).max(0);
                }
            }
            if let Some(lv) = hp_drain {
                let dmg = eff_max_hp * i64::from(10 * lv) / 100;
                u.damage = (u.damage + dmg).min((eff_max_hp - 1).max(0));
            }
            if let Some(lv) = en_heal {
                if !no_regen {
                    let rec = eff_max_en * (10 * lv) / 100;
                    u.en_consumed = (u.en_consumed - rec).max(0);
                }
            }
            if let Some(lv) = en_drain {
                let drn = eff_max_en * (10 * lv) / 100;
                u.en_consumed = (u.en_consumed + drn).min(eff_max_en);
            }
        }
        // 霊力 (プラーナ) 自然回復 (VB5 `Unit.cls:27374`): 主パイロットが霊力を持つ
        // (MaxPlana>0) ユニットは、フェイズ開始時に
        // `plana += MaxPlana/16 + MaxPlana*霊力回復Lv/10 - MaxPlana*霊力消費Lv/10`
        // し、[0, MaxPlana] にクランプする。HP/EN と異なり `回復不能` でも回復する
        // (VB5 では回復不能ガードの外に置かれている)。
        for i in 0..self.database.unit_instances.len() {
            if self.database.unit_instances[i].party != party {
                continue;
            }
            let inst = &self.database.unit_instances[i];
            let max_plana = self.database.effective_max_plana(inst);
            if max_plana <= 0 {
                continue;
            }
            let heal =
                crate::feature::feature_level(&inst.active_features, "霊力回復").unwrap_or(0);
            let drain =
                crate::feature::feature_level(&inst.active_features, "霊力消費").unwrap_or(0);
            let rec = max_plana / 16 + max_plana * heal / 10 - max_plana * drain / 10;
            let u = &mut self.database.unit_instances[i];
            u.plana = (u.plana + rec).clamp(0, max_plana);
        }
        // 母艦格納中ユニットの毎ターン回復 (回復系特殊能力.md 母艦): HP/EN を実効最大値の
        // 50% 回復、弾薬・アビリティ使用回数を全快。当該陣営フェイズ開始時。
        for i in 0..self.database.unit_instances.len() {
            if self.database.unit_instances[i].party != party
                || self.database.unit_instances[i].stored_in.is_none()
            {
                continue;
            }
            let inst = &self.database.unit_instances[i];
            let max_hp = self.database.effective_max_hp(inst);
            let max_en = self.database.effective_max_en(inst);
            let unit_name = inst.unit_data_name.clone();
            let (max_bullets, max_uses): (Vec<i32>, Vec<Option<i32>>) = self
                .database
                .unit_by_name(&unit_name)
                .map(|d| {
                    (
                        d.weapons.iter().map(|w| w.bullet).collect(),
                        d.abilities.iter().map(|a| a.uses).collect(),
                    )
                })
                .unwrap_or_default();
            let u = &mut self.database.unit_instances[i];
            u.damage = (u.damage - max_hp / 2).max(0);
            u.en_consumed = (u.en_consumed - max_en / 2).max(0);
            for w in &mut u.weapons {
                if let Some(b) = max_bullets.get(w.weapon_index) {
                    w.bullet_remaining = *b;
                }
            }
            for (j, a) in u.abilities.iter_mut().enumerate() {
                if let Some(uses) = max_uses.get(j) {
                    a.stock_remaining = *uses;
                }
            }
        }
        for u in &mut self.database.unit_instances {
            if u.party == party {
                u.has_acted = false;
                u.has_moved = false;
                // サポートアタック / サポートガード回数は毎フェイズ 1 リセット
                u.support_attack_remaining = 1;
                u.support_guard_remaining = 1;
                // カウンター (先制反撃) 使用回数も毎フェイズ 0 リセット (SRC UsedCounterAttack)。
                u.used_counter_attack = 0;
            }
        }
        self.command_menu = None;
        self.action_mode = crate::command_menu::ActionMode::Browse;
        let n = self.turn.number;
        let stage_name = phase.stage_name();
        if phase == crate::Phase::Player {
            self.push_message(format!("ターン {n} 開始: {}", phase.label()));
        }
        // SRC `Event.HandleEvent("ターン", Turn, party)` 相当。原典 (`Event.cs`) は
        // フェイズ開始ごとに `ターン 全 <陣営>`（毎ターン）と `ターン <N> <陣営>`
        // の 2 つを発火する。全フェイズ (味方/敵/中立/ＮＰＣ) が対象。
        // 英語エイリアス `Turn ...` も併せて発火し、後方互換を保つ。
        //
        // 発火は post_event_label (EventQue) 経由: 先行のターンイベントが Talk
        // 等で suspend しても後続イベントはキューに残り、完了後に順次実行される
        // (旧実装は trigger_label の実行中ガードに弾かれて黙って消えていた)。
        // 章ローカルイベントなので現ステージファイルにスコープして発火する
        // (全章同時ロード下で別章の同名 `ターン N <陣営>` へ漏れるのを防ぐ)。
        self.post_stage_event_label(format!("ターン 全 {stage_name}"));
        self.post_stage_event_label(format!("ターン {n} {stage_name}"));
        self.post_stage_event_label(format!("Turn All {stage_name}"));
        self.post_stage_event_label(format!("Turn {n} {stage_name}"));
        // 旧移植が発火していた陣営なしラベル (Player フェイズのみ) も後方互換で残す。
        if phase == crate::Phase::Player {
            self.post_stage_event_label(format!("ターン {n}"));
            self.post_stage_event_label(format!("Turn {n}"));
        }
        // フェイズ開始時点で勝敗が確定しているか判定する。`スタート` が味方を
        // 1 体も配置しなかった場合 (キャラ選択をキャンセルした等)、味方フェイズ
        // 開始直後に味方全滅=敗北を確定させ、「操作不能な味方フェイズ」を防ぐ。
        self.check_victory();
    }

    /// 元 SRC `GameClear`: 勝利演出に入る。Victory / 勝利 / Ending ラベルを試行。
    /// Battle 以外（既に Victory/Defeat 等）からの呼び出しは無視。
    pub fn game_clear(&mut self) {
        use crate::stage::StageState;
        if self.stage_state != StageState::Battle {
            return;
        }
        self.stage_state = StageState::Victory;
        self.command_menu = None;
        self.action_mode = crate::command_menu::ActionMode::Browse;
        self.push_message("【勝利】敵を全滅させました".to_string());
        // SRC では GameClear は単に TerminateSRC するが、シナリオ側で Ending を
        // 用意していることが多いので慣例的に発火する。スクリプト実行中
        // (GameClear/Win コマンド経由) はキューに積まれ完了後に実行される。
        // 勝利 / エンディング は章ローカルなので現ステージファイルにスコープ。
        //
        // `クリア` は原典の自動発火ラベルではないが、本移植のシナリオ (東方夢想伝等)
        // が勝利エントリとして使う。エンジンは他に `クリア` を発火する経路を持たない
        // ため (旧実装は check_victory が `クリア` 定義時に委譲するだけで誰も発火せず、
        // 敵全滅後に Battle のまま進行不能だった) ここで先頭候補として発火する。
        // game_clear は冒頭で stage_state=Victory にするため再入は冪等 (二重発火しない)。
        for lab in ["クリア", "Victory", "勝利", "Ending", "エンディング"] {
            if self.post_stage_event_label(lab.to_string()) {
                break;
            }
        }
    }

    /// 元 SRC `GameOver`: 敗北演出に入る。GameOver / ゲームオーバー ラベルを試行。
    /// Battle 以外からの呼び出しは無視。
    pub fn game_over(&mut self) {
        use crate::stage::StageState;
        if self.stage_state != StageState::Battle {
            return;
        }
        self.stage_state = StageState::Defeat;
        self.command_menu = None;
        self.action_mode = crate::command_menu::ActionMode::Browse;
        self.push_message("【敗北】味方全滅".to_string());
        // シナリオ独自の `GameOver` / `ゲームオーバー` ラベルを最優先で発火。
        for lab in ["GameOver", "ゲームオーバー"] {
            if self.post_event_label(lab.to_string()) {
                return;
            }
        }
        // SRC 本体相当の `Data/System/GameOver.eve` は **`プロローグ:` ラベル**で
        // コンティニュー Ask を出す (ラベル名が `GameOver` ではない)。そのため上の
        // 候補に当たらず「何も起こらない」状態だった。当該ファイル内の `プロローグ`
        // をファイルスコープ発火する (シナリオ本編の `プロローグ` とは別物なので、
        // global ではなく GameOver.eve に限定して誤ってオープニングへ飛ぶのを防ぐ)。
        // コンティニュー (`選択=1` → `Quickload`) は `__restart_save` からの再ロードに
        // 橋渡しされる (event_runtime の `quickload` ハンドラ)。
        if self.post_event_label_in_file("GameOver.eve", "プロローグ".to_string()) {
            return;
        }
        // シナリオも GameOver.eve も出口を持たない場合の組込みフォールバック。
        // これが無いと敗北画面 (「敗北」) のまま操作不能で詰む (実機報告: 東方夢想伝01)。
        // Enter/クリック = コンティニュー (ステージ開始時スナップショットから再開)、
        // 右クリック/Esc = タイトルへ。`advance` / `cancel_action` がこのフラグを見る。
        self.pending_game_over = true;
        let has_continue = !self.script_var("__restart_save").is_empty()
            || !self.script_var("__quicksave").is_empty();
        if has_continue {
            self.push_message(
                "Enter / クリックでコンティニュー、右クリック / Esc でタイトルへ".to_string(),
            );
        } else {
            self.push_message("右クリック / Esc でタイトルへ".to_string());
        }
    }

    /// 敗北フォールバックのコンティニュー: ステージ開始時のスナップショット
    /// (`__restart_save`、無ければ `__quicksave`) から再開を要求する。スナップ
    /// ショットが無ければタイトルへ戻る。
    fn game_over_continue(&mut self) {
        self.pending_game_over = false;
        let snap = {
            let restart = self.script_var("__restart_save");
            if restart.is_empty() {
                self.script_var("__quicksave").to_string()
            } else {
                restart.to_string()
            }
        };
        if snap.is_empty() {
            self.return_to_title();
        } else {
            self.request_reload(snap);
        }
    }

    /// タイトル画面へ戻る (敗北フォールバック / 汎用の終了)。
    fn return_to_title(&mut self) {
        self.pending_game_over = false;
        self.scene = Scene::Title;
        self.stage_state = crate::stage::StageState::Briefing;
        self.command_menu = None;
        self.action_mode = crate::command_menu::ActionMode::Browse;
    }

    /// 勝利状態 (Victory) で `クリア` 等が進行を解決しなかった場合に前進する
    /// (Enter / クリックから呼ばれる)。`次ステージ` が予約されていれば次ステージへ、
    /// なければインターミッション (登録があれば) かタイトルへ。
    fn proceed_after_victory(&mut self) {
        if !self.script_var("次ステージ").is_empty() && self.advance_to_next_stage() {
            if self.script_var("次ステージ").is_empty() {
                self.scene = Scene::MapView;
            }
            return;
        }
        if !self.intermission_commands.is_empty() {
            self.scene = Scene::Intermission;
            self.intermission_cursor = 0;
            self.intermission_mode = IntermissionMode::Menu;
            self.stage_state = crate::stage::StageState::Briefing;
        } else {
            self.return_to_title();
        }
    }

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

    /// SRC `Continue <filename>` で予約された「次ステージ」をクリアし、
    /// 対応するエントリへ遷移する。
    ///
    /// VB5 原典では `SRC.StartScenario(次ステージ)` がエピローグ後に
    /// 呼ばれて新しいシナリオファイルをロードする。本実装は次の優先順位で
    /// 解決する:
    ///
    /// 1. **ラベル名一致**: `次ステージ` の値が `script_library.label_pc`
    ///    に存在すればそのラベルから `trigger_label` で起動。
    /// 2. **ファイル名一致** (basename, case-insensitive): `Continue
    ///    Eve\onsen.eve` のようにパス指定された場合、library 内の
    ///    対応 .eve の start_pc から `run_from_pc` で起動。
    ///
    /// フロントエンド (`archive.rs` / scenario chain orchestrator) から
    /// script 完了後に呼び出すこと。
    ///
    /// 戻り値: 次ステージが起動できれば `true`。未セット / 該当無し /
    /// 既に script 中断中の場合は `false`。
    pub fn advance_to_next_stage(&mut self) -> bool {
        let next = self.script_var("次ステージ").to_string();
        if next.is_empty() {
            return false;
        }
        // 再呼び出しで同じラベル / ファイルに無限ループしないよう先にクリア。
        self.set_script_var("次ステージ".to_string(), String::new());
        // 現ステージファイルとして記録。`begin_battle` の `スタート` 発火を
        // このファイルスコープで行うため (同名ラベル誤発火の防止)。
        self.current_stage_file = next.clone();
        // 新しいステージは Briefing から始める。原典 `StartScenario` は
        // ステージを最初から立ち上げ直す。前ステージが Victory / Defeat の
        // まま残っていると `FlowCont::AfterStageFileRun` のガード
        // (Briefing / Sortie のみ通す) に弾かれ、次ステージが永久に開始
        // しなくなる (実コーパス: ＰＴＡにようこそ！ が 1 話クリア後に停止)。
        if matches!(
            self.stage_state,
            crate::stage::StageState::Victory | crate::stage::StageState::Defeat
        ) {
            self.stage_state = crate::stage::StageState::Briefing;
        }
        // ステージファイル実行の完了後処理 (Battle 突入 or ループバック尊重) を
        // 継続として積む。`スタート` 通過の事実は `start_passed_pcs` で観測する。
        self.start_passed_pcs.clear();
        self.flow.push(crate::flow::FlowCont::AfterStageFileRun);
        // 1) ラベル名として試す
        if crate::event_runtime::trigger_label(self, &next) {
            return true;
        }
        // 2) ファイル名 (basename 一致) として試す。
        //    原典 SRC `StartScenario` 準拠: ステージファイルは「プロローグ →
        //    (begin_battle 経由で) スタート」の順で起動する。`マップコマンド X:`
        //    / `進入 ...:` / `ターン N:` 等はゲームイベントで発火する **ハンドラ
        //    定義** であり、ロード時に本体を実行してはいけない。
        //
        //    ファイル先頭から run_from_pc すると、先頭がこれらハンドラ定義の
        //    ときにその本体を開幕にインライン実行してしまう (東方夢想伝01:
        //    冒頭の `マップコマンド ボーナス条件確認` 本体 = ボーダー説明 Talk が
        //    ステージ開始前に誤再生され、本来の `プロローグ` 幻想郷ナレーションも
        //    飛ばされる)。プロローグがあればそれを起動し、無ければ従来どおり
        //    ファイル先頭から実行する。スタート発火は AfterStageFileRun 継続が担う。
        if self.script_library().find_file(&next).is_some() {
            if crate::event_runtime::trigger_label_in_file(self, &next, "プロローグ")
                || crate::event_runtime::trigger_label_in_file(self, &next, "Prologue")
            {
                return true;
            }
            // プロローグ無し: ラベルを持たない薄いステージ / top-level コード型は
            // ファイル先頭から実行する (従来挙動)。
            let file_start_pc = self.script_library().find_file(&next).map(|e| e.start_pc);
            if let Some(pc) = file_start_pc {
                let _ = crate::event_runtime::run_from_pc(self, pc);
                return true;
            }
        }
        // 何も起動しなかった → 積んだ継続を取り下げ (stale 防止)、予約を復元
        // する。解決失敗 (ラベル/ファイル未登録 = 壊れたシナリオや外部ファイル
        // 参照) を握り潰して予約ごと消すと、呼び出し側/フロントエンドが後から
        // 再試行・診断できなくなるため。
        if self.flow.last() == Some(&crate::flow::FlowCont::AfterStageFileRun) {
            self.flow.pop();
        }
        self.set_script_var("次ステージ".to_string(), next);
        false
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

    /// 入力を消費して状態を進める。redraw が必要なら `true` を返す。
    /// Consume an input. Returns `true` when a redraw is required.
    pub fn handle_input(&mut self, input: Input) -> bool {
        // 対話 UI が出ているうちは、それ以外の入力は喰わず dialog 操作のみ。
        if self.pending_dialog.is_some() {
            return match input {
                // Talk: 任意キーで進行。Confirm: Yes (0)。
                Input::Advance | Input::DialogYes | Input::AttackTarget | Input::EndPhase => {
                    self.respond_dialog(0)
                }
                Input::DialogNo => self.respond_dialog(1),
                Input::DialogChoice(n) => self.respond_dialog(n),
                // クリック処理: Hotpoint 付きの Menu はヒット判定で確定、
                // それ以外 (Talk / Confirm / 普通の Menu) はクリックで Advance。
                Input::ClickAt { x, y } => {
                    // 反撃ウィンドウ表示中は専用ジオメトリで選択肢行をクリック判定する
                    // (プレーン Menu とは別配置のため menu_choice_at は使えない)。
                    if self.pending_reaction.is_some() {
                        let n =
                            if let Some(crate::dialog::PendingDialog::Menu { options, .. }) =
                                &self.pending_dialog
                            {
                                options.len()
                            } else {
                                0
                            };
                        return match crate::dialog::reaction_choice_at(n, x, y) {
                            Some(c) => self.respond_dialog(c),
                            None => false,
                        };
                    }
                    // 武器選択窓も専用ジオメトリで武器行をクリック判定する。
                    if self.pending_weapon_select.is_some() {
                        let n =
                            if let Some(crate::dialog::PendingDialog::Menu { options, .. }) =
                                &self.pending_dialog
                            {
                                options.len()
                            } else {
                                0
                            };
                        return match crate::dialog::weapon_select_choice_at(n, x, y) {
                            Some(c) => self.respond_dialog(c),
                            None => false,
                        };
                    }
                    let is_hotpoint_menu = matches!(
                        self.pending_dialog,
                        Some(crate::dialog::PendingDialog::Menu {
                            store_value: true,
                            ..
                        })
                    ) && !self.hotpoints.is_empty();
                    if is_hotpoint_menu {
                        // ヒットすれば確定、外したら無反応 (キャンセルではない)
                        self.try_hotpoint_click(x, y)
                    } else {
                        // プレーンな Menu(Ask) は選択肢行をクリック確定する。
                        // 旧実装は任意クリックで respond_dialog(0)=キャンセルしており、
                        // 難易度/キャラ選択を「クリックで選べない」状態だった
                        // (選択が 0 になり、東方夢想伝では味方 0 体で即敗北)。
                        // Talk / Confirm / WaitClick は従来どおり任意クリックで Advance。
                        let plain_menu_choice = if let Some(crate::dialog::PendingDialog::Menu {
                            prompt,
                            options,
                            ..
                        }) = &self.pending_dialog
                        {
                            Some(crate::dialog::menu_choice_at(prompt, options.len(), x, y))
                        } else {
                            None
                        };
                        match plain_menu_choice {
                            // 選択肢行をクリック → 確定。
                            Some(Some(n)) => self.respond_dialog(n),
                            // プレーン Menu の選択肢外クリック → 無反応
                            // (キャンセルは右クリック / Esc)。
                            Some(None) => false,
                            // 非 Menu (Talk / Confirm 等) → Advance(Yes)。
                            None => self.respond_dialog(0),
                        }
                    }
                }
                // 右クリック: SRC `Wait Click` の「右ボタン = キャンセル/戻る」。
                // 選択を空にして KeyState(2)=1 でスクリプトを再開し、ステータス画面等の
                // `Case "" → If KeyState(2) Then Break` で抜けられるようにする。
                Input::RightClickAt { .. } => self.respond_dialog_right_click(),
                // Esc: Hotpoint Wait Click 画面は右クリック相当で「戻る」。トラックパッドの
                // 副ボタン設定に依存せず確実に抜けられるようにする。通常の Menu(Ask) は
                // 従来どおり choice 0 でキャンセル。
                Input::Cancel => {
                    if self.respond_dialog_right_click() {
                        true
                    } else if matches!(
                        self.pending_dialog,
                        Some(crate::dialog::PendingDialog::Menu { .. })
                    ) {
                        self.respond_dialog(0)
                    } else {
                        false
                    }
                }
                _ => false,
            };
        }
        match input {
            Input::Advance => self.advance(),
            Input::ClickAt { x, y } => self.handle_click(x, y),
            Input::RightClickAt { x, y } => self.handle_right_click(x, y),
            Input::MoveCursor(dir) => self.move_cursor(dir),
            Input::EndPhase => self.end_phase(),
            Input::AttackTarget => self.player_attack_shortcut(),
            Input::CycleWeapon => self.cycle_weapon(),
            Input::DialogYes | Input::DialogNo | Input::DialogChoice(_) => false,
            Input::Cancel => self.cancel_action(),
            Input::GotoPilotList => {
                if matches!(self.scene, Scene::MapView | Scene::UnitList) {
                    self.scene = Scene::PilotList;
                    true
                } else {
                    false
                }
            }
            Input::GotoUnitList => {
                if matches!(self.scene, Scene::MapView | Scene::PilotList) {
                    self.scene = Scene::UnitList;
                    true
                } else {
                    false
                }
            }
            Input::GotoMapView => {
                if matches!(self.scene, Scene::PilotList | Scene::UnitList) {
                    self.scene = Scene::MapView;
                    true
                } else {
                    false
                }
            }
        }
    }

    /// 右クリック / Esc によるキャンセル。`MoveSelect` / `AttackSelect` →
    /// Browse へ、`command_menu` → 閉じる、それ以外は no-op。
    fn cancel_action(&mut self) -> bool {
        use crate::command_menu::ActionMode;
        // 敗北フォールバック中のキャンセル (右クリック/Esc) → タイトルへ。
        if self.pending_game_over {
            self.return_to_title();
            return true;
        }
        // 単機ステータス詳細のキャンセル (右クリック/Esc) → インターミッションへ戻る
        // (元 SRC: パイロット/ユニットステータスは右クリックで戻る)。
        if self.scene == Scene::UnitDetail {
            self.exit_list_scene();
            return true;
        }
        // 乗り換えの移動先選択中のキャンセル → 移動元選択へ戻る (メニューには戻らない)。
        if self.scene == Scene::Intermission
            && self.intermission_mode == IntermissionMode::RideChange
            && self.ride_change_source.take().is_some()
        {
            self.intermission_cursor = 0;
            return true;
        }
        // インターミッションのサブモード (機体改造 / 換装 / 乗り換え) 中のキャンセル → メインメニューへ。
        if self.scene == Scene::Intermission && self.intermission_mode != IntermissionMode::Menu {
            self.intermission_mode = IntermissionMode::Menu;
            self.intermission_cursor = 0;
            return true;
        }
        // PostMoveMenu 表示中のキャンセル: 移動を巻き戻してブラウズに戻る
        if self.command_menu.is_some() {
            if let ActionMode::PostMoveMenu { uid, snapshot } = self.action_mode.clone() {
                self.rollback_move(&uid, &snapshot);
            }
            self.command_menu = None;
            self.action_mode = ActionMode::Browse;
            return true;
        }
        match self.action_mode.clone() {
            ActionMode::Browse => false,
            ActionMode::PostMoveMenu { uid, snapshot } => {
                self.rollback_move(&uid, &snapshot);
                self.action_mode = ActionMode::Browse;
                true
            }
            ActionMode::AttackSelect {
                uid,
                snapshot: Some(snapshot),
            } => {
                // 攻撃目標選択キャンセル → 移動後メニューに戻る (移動はまだ確定のまま)
                let pos = self.database.unit_by_uid(&uid).map(|u| (u.x, u.y));
                self.action_mode = ActionMode::PostMoveMenu { uid, snapshot };
                if let Some(pos) = pos {
                    self.open_unit_menu(pos);
                }
                true
            }
            ActionMode::SpiritTarget { caster, .. } => {
                // 精神コマンドの対象選択キャンセル: SP は未消費。発動主体の
                // ユニットメニューへ戻り、別コマンドや行動を選び直せるようにする。
                self.action_mode = ActionMode::Browse;
                if let Some(pos) = self.database.unit_by_uid(&caster).map(|u| (u.x, u.y)) {
                    self.open_unit_menu(pos);
                }
                true
            }
            ActionMode::SupportTarget { caster, .. } => {
                // 修理 / 補給 の対象選択キャンセル: 効果なし。発動主体の
                // ユニットメニューへ戻り、別コマンドや行動を選び直せるようにする。
                self.action_mode = ActionMode::Browse;
                if let Some(pos) = self.database.unit_by_uid(&caster).map(|u| (u.x, u.y)) {
                    self.open_unit_menu(pos);
                }
                true
            }
            ActionMode::AbilityTarget { caster, .. } => {
                // アビリティ対象選択キャンセル: 効果なし。発動主体のメニューへ戻る。
                self.action_mode = ActionMode::Browse;
                if let Some(pos) = self.database.unit_by_uid(&caster).map(|u| (u.x, u.y)) {
                    self.open_unit_menu(pos);
                }
                true
            }
            ActionMode::MoveSelect { .. } | ActionMode::AttackSelect { snapshot: None, .. } => {
                self.action_mode = ActionMode::Browse;
                true
            }
            ActionMode::LandingSelect { uid, carrier, .. } => {
                self.action_mode = ActionMode::Browse;
                if let Some(carrier) = carrier {
                    // 発進: 配置せず格納のまま、母艦メニューへ戻す。
                    self.reopen_unit_menu_for(&carrier);
                } else {
                    // 分離: 分離自体は成立済なので、残り全機を隣接自動配置で確定する。
                    let source = self.landing_source.unwrap_or((0, 0));
                    if let Some((tx, ty)) = self.find_empty_adjacent_tile(source) {
                        self.place_separated_unit_at(&uid, tx, ty);
                    }
                    let remaining = std::mem::take(&mut self.landing_queue);
                    for ru in remaining {
                        if let Some((tx, ty)) = self.find_empty_adjacent_tile(source) {
                            self.place_separated_unit_at(&ru, tx, ty);
                        }
                    }
                    self.finish_landing_queue();
                }
                true
            }
        }
    }

    fn handle_right_click(&mut self, _x: i32, _y: i32) -> bool {
        self.cancel_action()
    }

    fn cycle_weapon(&mut self) -> bool {
        if self.scene != Scene::MapView {
            return false;
        }
        let Some((cx, cy)) = self.map_cursor else {
            return false;
        };
        let Some(u) = self.database.units_at(cx, cy).next() else {
            return false;
        };
        let Some(d) = self.database.unit_by_name(&u.unit_data_name) else {
            return false;
        };
        // 0 = 自動、1..=N = 武器固定
        let modulus = d.weapons.len() + 1;
        if modulus == 0 {
            return false;
        }
        self.selected_weapon_idx = (self.selected_weapon_idx + 1) % modulus;
        true
    }

    /// 武器が「移動後攻撃」で使用可能か。`post_move=false` なら常に true。
    /// SRC: 移動後は近距離武器のみデフォルト可、遠距離 (max_range>1) は Ｐ 属性、
    /// Ｑ 属性 (移動後使用不可) は除外。メニュー可否判定と攻撃実行で共有し、
    /// 「攻撃を選べるが押しても無反応」の齟齬を防ぐ。
    fn weapon_usable_post_move(
        w: &crate::data::unit::WeaponData,
        post_move: bool,
        totsugeki: bool,
    ) -> bool {
        if !post_move {
            return true;
        }
        if w.max_range > 1 {
            // 通常、長射程武器は移動後使用不可 (Ｐ 属性を除く)。精神「突撃」発動中は
            // マップ攻撃 (Ｍ) 以外の長射程武器も移動後使用可能 (スペシャルパワー.md)。
            w.class.contains('Ｐ') || (totsugeki && !w.class.contains('Ｍ'))
        } else {
            !w.class.contains('Ｑ')
        }
    }

    /// 距離 `dist` の敵に対し使用する武器を選ぶ。`forced` 指定時はその武器が
    /// 合法 (射程内・チャージ・移動後制限) なら採用、不可なら `None`。自動選択時は
    /// 合法武器のうち威力最大 (移動後制限なしは従来の `best_weapon_in_range_with_charge`)。
    #[allow(clippy::too_many_arguments)] // 武器選択の制約 (射程/チャージ/移動後/突撃/資源/必要技能) を素直に列挙。
    fn pick_attack_weapon(
        atk_unit: &crate::data::unit::UnitData,
        dist: u32,
        forced: Option<&crate::data::unit::WeaponData>,
        charged: bool,
        post_move: bool,
        totsugeki: bool,
        firable: impl Fn(&crate::data::unit::WeaponData) -> bool,
        skill_ok: impl Fn(&crate::data::unit::WeaponData) -> bool,
    ) -> Option<crate::data::unit::WeaponData> {
        let legal = |w: &crate::data::unit::WeaponData| {
            combat::weapon_in_range(w, dist)
                && (charged || !combat::is_charge_weapon(w))
                && Self::weapon_usable_post_move(w, post_move, totsugeki)
                && firable(w)
        };
        match forced {
            // 強制武器 (プレイヤーが武器選択した場合) は資源 (EN/弾/気力) を満たさなくても
            // 発射を許す (UI 側で選択可否を制御する想定)。ただし必要技能/条件は資源ではなく
            // 使用資格そのものなので強制でも必須 (満たさない武器は撃てない)。
            Some(w) => (combat::weapon_in_range(w, dist)
                && (charged || !combat::is_charge_weapon(w))
                && Self::weapon_usable_post_move(w, post_move, totsugeki)
                && skill_ok(w))
            .then(|| w.clone()),
            None => atk_unit
                .weapons
                .iter()
                .filter(|w| legal(w))
                .max_by_key(|w| w.power)
                .cloned(),
        }
    }

    /// カーソル上の自勢力ユニットで最寄りの敵対ユニットを攻撃。
    /// `hit_chance` に対して PRNG を回し、命中時はダメージを加算。
    /// 撃破時はマップから除去。HUD ログに結果を流す。
    /// **AI / 内部専用**。プレイヤーのクリック攻撃は `attack_unit_at` (状態機械経由) を使う。
    fn attack_target(&mut self) -> bool {
        // 自動 def_mode ("") = 防御側は自動反撃 (従来挙動)。
        self.attack_resolve_and_run(None, false, "")
    }

    /// `a` キー (Input::AttackTarget) のプレイヤー用挙動。旧来は最寄り敵を直接攻撃して
    /// メニュー状態機械を迂回していた (位置ずれ・選択不整合の原因)。現在はユニット
    /// コマンドメニューに「攻撃」があればそれを選択し、`AttackSelect` 状態へ遷移させる
    /// だけにする (クリックと同一フロー)。メニュー未表示なら no-op。
    fn player_attack_shortcut(&mut self) -> bool {
        use crate::command_menu::{CommandMenu, MenuActionId, UnitAction, UnitMenuItem};
        let has_attack = matches!(
            &self.command_menu,
            Some(CommandMenu::Unit { items, .. })
                if items.iter().any(|i| matches!(i, UnitMenuItem::Builtin(UnitAction::Attack)))
        );
        if has_attack {
            self.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Builtin(
                UnitAction::Attack,
            )))
        } else {
            false
        }
    }

    /// `atk_uid` のユニットが `target` タイルのユニットを攻撃する。
    /// クリックされたタイルの敵を厳密に対象とし、最寄り敵ヒューリスティックは使わない。
    /// 対象が敵でない / 射程内に合法武器が無い場合は false (別ユニットを攻撃しない)。
    fn attack_unit_at(&mut self, atk_uid: &str, target: (u32, u32), post_move: bool) -> bool {
        let Some((ax, ay)) = self.database.unit_by_uid(atk_uid).map(|u| (u.x, u.y)) else {
            return false;
        };
        self.map_cursor = Some((ax, ay));
        self.attack_resolve_and_run(Some(target), post_move, "")
    }

    /// 攻撃の本体。`desired_target` が `Some(tile)` ならそのタイルの敵を対象に、
    /// `None` なら射程内最寄り敵を対象に攻撃を実行する。`post_move` は移動後攻撃
    /// (Ｐ属性制限) かどうか。
    ///
    /// `def_mode` は防御側の反撃手段 (SRC 反撃モード):
    /// - `""` / `"反撃"`: 通常命中・ダメージ＋反撃 (自動反撃と同じ)。
    /// - `"回避"`: 命中率 ×0.5、反撃なし。
    /// - `"防御"`: ダメージ ×0.5、反撃なし。
    fn attack_resolve_and_run(
        &mut self,
        desired_target: Option<(u32, u32)>,
        post_move: bool,
        def_mode: &str,
    ) -> bool {
        // 再攻撃 (VB5 begin ループ) の再帰呼び出しか。`reattack_in_progress` で受け渡し、
        // この呼び出し内では使用イベント再発火と 3回目の再攻撃を抑止する。
        let is_reattack = self.reattack_in_progress;
        if self.scene != Scene::MapView {
            return false;
        }
        if self.stage_state != crate::stage::StageState::Battle {
            return false;
        }
        let Some((cx, cy)) = self.map_cursor else {
            return false;
        };

        // カーソル上のユニット
        let atk_idx = self
            .database
            .unit_instances
            .iter()
            .position(|u| u.x == cx && u.y == cy);
        let Some(mut atk_idx) = atk_idx else {
            return false;
        };

        let atk_party = self.database.unit_instances[atk_idx].party;
        if atk_party != self.turn.phase.party() {
            return false;
        }

        let atk_pilot_name = self.database.unit_instances[atk_idx].pilot_name.clone();
        let atk_unit_name = self.database.unit_instances[atk_idx].unit_data_name.clone();
        // 戦闘予測には実行時実効値 (育成 / 強化パーツ / 状態異常) を反映した複製を使う。
        let Some((atk_pilot, atk_unit)) = self.database.effective_combat_data(atk_idx) else {
            return false;
        };

        // 攻撃武器の固定指定 (selected_weapon_idx: 0=自動)。所有権ある複製にする。
        let forced_weapon = if self.selected_weapon_idx == 0 {
            None
        } else {
            atk_unit.weapons.get(self.selected_weapon_idx - 1).cloned()
        };
        // SRC `Charge` フラグで Ｃ 属性武器が解禁される。
        let atk_charged = self.database.unit_instances[atk_idx].charged;
        // 精神「突撃」で移動後でも長射程武器が使える。
        let atk_totsugeki = self.database.unit_instances[atk_idx].has_condition("突撃");

        // 対象ユニット (def_idx) と使用武器を確定する。
        let (mut def_idx, weapon) = if let Some(tt) = desired_target {
            // クリックされたタイルの敵を厳密に対象とする (最寄り敵には流れない)。
            let def_idx = self.database.unit_instances.iter().position(|u| {
                !u.off_map && u.x == tt.0 && u.y == tt.1 && u.party.is_hostile_to(atk_party)
            });
            let Some(def_idx) = def_idx else {
                return false;
            };
            let d = combat::manhattan((cx, cy), tt);
            let Some(weapon) = Self::pick_attack_weapon(
                &atk_unit,
                d,
                forced_weapon.as_ref(),
                atk_charged,
                post_move,
                atk_totsugeki,
                |w| self.weapon_firable(atk_idx, w),
                |w| self.weapon_skill_ok(atk_idx, w),
            ) else {
                self.push_message(format!(
                    "{}: 射程内に使用可能な武器がありません.",
                    atk_pilot.nickname
                ));
                return false;
            };
            (def_idx, weapon)
        } else {
            // 射程内最寄り敵を探索 (AI / 旧 a キー)。
            let mut best: Option<(usize, u32)> = None;
            for (i, def) in self.database.unit_instances.iter().enumerate() {
                if i == atk_idx || !def.party.is_hostile_to(atk_party) || def.off_map {
                    continue;
                }
                let d = combat::manhattan((cx, cy), (def.x, def.y));
                if Self::pick_attack_weapon(
                    &atk_unit,
                    d,
                    forced_weapon.as_ref(),
                    atk_charged,
                    post_move,
                    atk_totsugeki,
                    |w| self.weapon_firable(atk_idx, w),
                    |w| self.weapon_skill_ok(atk_idx, w),
                )
                .is_some()
                {
                    match best {
                        None => best = Some((i, d)),
                        Some((_, bd)) if d < bd => best = Some((i, d)),
                        _ => {}
                    }
                }
            }
            let Some((def_idx, dist)) = best else {
                // 武器固定中で対象なしなら情報メッセージを出す
                if let Some(w) = forced_weapon.as_ref() {
                    self.push_message(format!(
                        "{}: 射程内に対象なし ({}).",
                        atk_pilot.nickname, w.name
                    ));
                }
                return false;
            };
            let Some(weapon) = Self::pick_attack_weapon(
                &atk_unit,
                dist,
                forced_weapon.as_ref(),
                atk_charged,
                post_move,
                atk_totsugeki,
                |w| self.weapon_firable(atk_idx, w),
                |w| self.weapon_skill_ok(atk_idx, w),
            ) else {
                return false;
            };
            (def_idx, weapon)
        };

        let def_inst = self.database.unit_instances[def_idx].clone();
        let def_combat = self.database.effective_combat_data(def_idx);
        let map = self.database.map.as_ref();
        let def_terrain_id = map.map(|m| m.cell(def_inst.x, def_inst.y).terrain_id);
        let (Some((def_pilot, mut def_unit)), Some(terrain_id)) = (def_combat, def_terrain_id)
        else {
            return false;
        };
        // 防御特性による装甲調整 (弱点→半減 / 吸収→無視)。VB5 `Unit.cls:6949-6951`。
        self.apply_defense_armor_mod(def_idx, &weapon.class, &mut def_unit);
        let def_hit_mod = self.database.terrain_hit_mod(terrain_id);
        let def_damage_mod = self.database.terrain_damage_mod(terrain_id);
        // Ｃ 属性武器を使った場合、charged フラグを消費 (次回攻撃前に再 Charge 必要)。
        if combat::is_charge_weapon(&weapon) {
            self.database.unit_instances[atk_idx].charged = false;
        }
        // SRC `使用 <unit> <device>:` (`使用イベント.md`) ─ `攻撃イベント` の前に発火。
        // 再攻撃 (VB5 `begin` ラベルは使用イベントより後ろ) では再発火しない。
        if !is_reattack {
            crate::event_runtime::fire_use_event_labels(self, atk_idx, &weapon.name);
        }

        // 戦闘イベント系システム変数を設定 (`変数.md`: 対象ユニット使用武器 等)。
        // 攻撃イベント発火前に攻撃側武器を登録し、相手武器は後で更新する。
        let atk_weapon_num = atk_unit
            .weapons
            .iter()
            .position(|w| w.name == weapon.name)
            .map(|i| i + 1)
            .unwrap_or(1);
        self.set_script_var("対象ユニット使用武器".to_string(), weapon.name.clone());
        self.set_script_var(
            "対象ユニット使用武器番号".to_string(),
            atk_weapon_num.to_string(),
        );
        // 武器発射による EN・残弾消費 (撃破前=index 失効前に消費。命中の有無に依らない)。
        self.consume_weapon_resources(atk_idx, atk_weapon_num - 1);
        // 攻撃元 = 対象 / 攻撃先 = 相手。`攻撃イベント.md` の解説どおり、ユニットを
        // 陣営名で指定した場合に実ユニットを識別できるよう、パイロット名と
        // 一意 uid (`対象ユニットＩＤ` / `相手ユニットＩＤ`) を発火前に設定する。
        // これらは `攻撃後イベント` にもそのまま引き継がれる (script_vars 永続)。
        let atk_uid = self.database.unit_instances[atk_idx].uid.clone();
        self.set_script_var("対象ユニットＩＤ".to_string(), atk_uid.clone());
        self.set_script_var("対象パイロット".to_string(), atk_pilot_name.clone());
        self.set_script_var("相手ユニットＩＤ".to_string(), def_inst.uid.clone());
        self.set_script_var("相手パイロット".to_string(), def_inst.pilot_name.clone());
        self.set_script_var("相手ユニット使用武器".to_string(), String::new());
        self.set_script_var("相手ユニット使用武器番号".to_string(), "0".to_string());
        self.set_script_var("対象ユニット使用アビリティ".to_string(), String::new());
        self.set_script_var(
            "対象ユニット使用アビリティ番号".to_string(),
            "0".to_string(),
        );
        self.set_script_var(
            "対象ユニット使用スペシャルパワー".to_string(),
            String::new(),
        );
        self.set_script_var("サポートアタックユニットＩＤ".to_string(), String::new());
        self.set_script_var("サポートガードユニットＩＤ".to_string(), String::new());

        // SRC `攻撃 <atk> <def>:` (`攻撃イベント.md`) ─ ダメージ適用前に発火。
        // 発火中に Talk 等で UI が中断するとダメージは保留されるが、本実装は
        // Destruction 同様 sync 完結する Set/Message/Goto/ChangeMode を想定。
        crate::event_runtime::fire_attack_event_labels(self, atk_idx, def_idx);

        // 攻撃後ラベル発火用に attacker/defender 識別子を退避。
        // damage 適用で defender が unit_instances から消えたあとに名前が必要。
        let after_atk = crate::event_runtime::UnitEventId::from_unit_instance(
            &self.database.unit_instances[atk_idx],
        );
        let after_def = crate::event_runtime::UnitEventId::from_unit_instance(
            &self.database.unit_instances[def_idx],
        );

        // 先制攻撃 (カウンター/先読み, SRC `COM.bas:1040-1058`): 防御側が反撃武器を射程内に
        // 持ち先手を取れるなら、主攻撃の**前に**防御側が反撃する。`reattack` (再攻撃) では
        // 先制反撃しない (二重反撃を避ける)。先制反撃で攻撃側が撃破されたら主攻撃は不発。
        let preempted = if is_reattack {
            false
        } else {
            self.try_preemptive_counter(def_idx, (cx, cy), (def_inst.x, def_inst.y), &weapon.class)
        };
        if preempted {
            let atk_alive = self
                .database
                .unit_instances
                .iter()
                .any(|u| !u.off_map && u.x == cx && u.y == cy);
            if !atk_alive {
                self.check_victory();
                return true;
            }
            // 先制反撃で index がずれ得る (撃破/みがわり) ため位置で引き直す。
            if let Some(i) = self
                .database
                .unit_instances
                .iter()
                .position(|u| u.x == cx && u.y == cy)
            {
                atk_idx = i;
            }
            if let Some(i) = self
                .database
                .unit_instances
                .iter()
                .position(|u| u.x == def_inst.x && u.y == def_inst.y)
            {
                def_idx = i;
            }
        }

        // 状態異常 / 精神コマンド分の補正は predict_with_status に任せる。
        let atk_morale = self.database.unit_instances[atk_idx].morale;
        let def_morale = def_inst.morale;
        let mut atk_statuses: Vec<String> = self.database.unit_instances[atk_idx]
            .conditions
            .iter()
            .map(|c| c.name.clone())
            .collect();
        push_combat_feature_statuses(&self.database.unit_instances[atk_idx], &mut atk_statuses);
        let mut def_statuses: Vec<String> =
            def_inst.conditions.iter().map(|c| c.name.clone()).collect();
        push_combat_feature_statuses(&def_inst, &mut def_statuses);
        // 地形適応 (SRC `戦闘システム詳細.md`): 攻撃側は自地形、防御側は自地形で参照。
        let atk_env = self.terrain_env_at(
            self.database.unit_instances[atk_idx].x,
            self.database.unit_instances[atk_idx].y,
        );
        let def_env = self.terrain_env_at(def_inst.x, def_inst.y);
        // 与/被ダメージ修正 (ダメージ増加/被ダメージ増加/ダメージ低下/被ダメージ低下) を
        // 攻撃側/防御側それぞれの精神効果から sp.txt 経由で解決 (シナリオ固有値を反映)。
        let dmg_levels = self
            .database
            .damage_spirit_levels(&atk_statuses, &def_statuses);
        let preview = combat::predict_with_status_terrain(
            &atk_pilot,
            &atk_unit,
            &weapon,
            &def_pilot,
            &def_unit,
            def_hit_mod,
            def_damage_mod,
            atk_morale,
            def_morale,
            &atk_statuses,
            &def_statuses,
            atk_env,
            def_env,
            dmg_levels,
            self.ecm_hit_mult(def_idx, atk_idx, &weapon.class),
        );
        // 散 (散布) 属性武器の距離補正 (命中アップ・ダメージダウン)。
        let atk_def_dist = combat::manhattan(
            (
                self.database.unit_instances[atk_idx].x,
                self.database.unit_instances[atk_idx].y,
            ),
            (
                self.database.unit_instances[def_idx].x,
                self.database.unit_instances[def_idx].y,
            ),
        );
        let preview = preview.apply_scatter(&weapon.class, atk_def_dist);

        // 命中判定。回避を選んだ防御側は命中率が半減する (SRC 反撃モード)。
        let effective_hit = if def_mode == "回避" {
            preview.hit_chance / 2
        } else {
            preview.hit_chance
        };
        // 1 つの乱数から命中(下位)とクリティカル(上位)を取り出す。下位 2 桁は従来の
        // 命中ロールと同一なので、命中可否・後続の乱数列 (反撃/援護) は一切変わらない。
        let r = self.next_u32();
        let roll = (r % 100) as i32;
        // 回避系特殊能力 (B): 命中するはずでも 分身/超回避 で完全回避され得る。
        let hit =
            roll < effective_hit && !self.check_dodge_feature(def_idx, atk_idx, &weapon.class);
        // クリティカル判定 (命中時のみ)。技能補正 (超反応/超能力/底力) を base へ加算した後、
        // 防御選択でクリ率が半減する (SRC `戦闘システム詳細.md`「防御」: クリティカル率 ×0.5)。
        let crit_base = preview.critical_chance + self.crit_skill_bonus(atk_idx, def_idx);
        let crit_chance = if def_mode == "防御" {
            crit_base / 2
        } else {
            crit_base
        };
        let crit_roll = ((r / 100) % 100) as i32;
        // 特殊効果攻撃属性 (CC 属性) を持つ武器では通常のクリティカルは発生しない。
        // 特殊効果の発動がクリティカルの代わりとみなされる (SRC 特殊効果攻撃属性.md)。
        // 状態異常付与 (weapon_special_effects) と 気力減少 (脱/Ｄ) の双方が該当。
        let has_special_effect = !crate::combat::weapon_special_effects(&weapon.class).is_empty()
            || crate::combat::weapon_morale_reduction(&weapon.class).is_some()
            || weapon.class.split_whitespace().any(|t| t == "即");
        let critical = hit && crit_roll < crit_chance && !has_special_effect;
        let mut defender_killed = false;
        // クリティカルはダメージ 1.5 倍 (SRC)。その後、防御選択でさらに半減する。
        let crit_damage = if critical {
            (preview.damage * 3 / 2).max(0)
        } else {
            preview.damage
        };
        let applied_damage = if def_mode == "防御" {
            (crit_damage / 2).max(0)
        } else {
            crit_damage.max(0)
        };
        // 即死 (即): 命中時に特殊効果発動率で proc し、ボス以外を致死化する
        // (`特殊効果攻撃属性.md`)。proc すると致死ダメージにして撃破サイトへ流す。
        // 非 即 武器は RNG を消費しない (既存の乱数列を保つ)。
        let applied_damage =
            if hit && self.roll_weapon_instakill(def_idx, &weapon, &atk_pilot, &def_pilot) {
                def_unit.hp.max(applied_damage)
            } else {
                applied_damage
            };
        // 切り払い: 格闘系の攻撃を防御側が確率で無効化する (攻撃側 直撃 で無効化されない)。
        let atk_has_chokugeki = atk_statuses.iter().any(|s| s.contains("直撃"));
        let parried = hit && !atk_has_chokugeki && self.roll_parry(atk_idx, def_idx, &weapon.class);
        if parried {
            self.push_message(format!(
                "{} は {} の [{}] を切り払った！",
                def_pilot.nickname, atk_pilot.nickname, weapon.name
            ));
        }
        let applied_damage = if parried { 0 } else { applied_damage };
        // 防御特性 (耐性 ÷2 / 無効化 0 / 吸収 回復) を最終ダメージへ適用する。
        let dmg_before_def = applied_damage;
        let applied_damage = self.defense_attribute_damage(def_idx, &weapon.class, applied_damage);
        if applied_damage != dmg_before_def {
            if applied_damage < 0 {
                self.push_message(format!("{} は攻撃を吸収した！", def_pilot.nickname));
            } else if applied_damage == 0 {
                self.push_message(format!("{} には効かない (無効化)", def_pilot.nickname));
            }
        }

        // 援護防御チェック。`def_mode` で挙動を切替える (SRC 反撃モードの「援護防御
        // ON/OFF」):
        //  - "援護防御": プレイヤーが明示選択 → フェイズに依らず強制発動。
        //  - "反撃"/"回避"/"防御": 個別反撃を選んだ → 援護防御は発動しない。
        //  - ""(自動/ヘッドレス/行動不能の素通り): 従来どおり 敵/中立/ＮＰＣ フェイズの
        //    攻撃時のみ自動発動 (味方が自分から攻撃した Player フェイズは無効)。
        // 直撃 (精神コマンド): サポートガード (援護防御) を無効化する (§0.8 / SRC)。
        let atk_chokugeki = atk_statuses.iter().any(|s| s.contains("直撃"));
        let guard_idx = if !hit || atk_chokugeki {
            None
        } else {
            match def_mode {
                "援護防御" => self.try_find_support_guard(def_idx, preview.damage),
                "反撃" | "回避" | "防御" => None,
                _ if self.turn.phase != crate::turn::Phase::Player => {
                    self.try_find_support_guard(def_idx, preview.damage)
                }
                _ => None,
            }
        };

        let msg = if hit {
            if let Some(sub) = self.apply_migawari(def_idx, atk_idx, &weapon.class, applied_damage)
            {
                // みがわり (身代わり, 最優先・SRC Unit.cls:14855): 指定された身代わりユニットが
                // 防御側のダメージを 1 回肩代わりする。防御側は無傷、condition は消費される。
                format!(
                    "みがわり: {} が {} の身代わりになった！ [{}]",
                    sub, def_pilot.nickname, weapon.name
                )
            } else if let Some(g_idx) = guard_idx {
                // 援護防御: guard unit が 50% ダメージを受ける (防御扱い)
                let guard_dmg = (preview.damage as f64 * 0.5) as i64;
                let guard_unit_name = self.database.unit_instances[g_idx].unit_data_name.clone();
                let guard_pilot_name = self.database.unit_instances[g_idx].pilot_name.clone();
                let guard_unit = self.database.unit_by_name(&guard_unit_name).cloned();
                let guard_pilot = self.database.pilot_by_name(&guard_pilot_name).cloned();
                self.database.unit_instances[g_idx].support_guard_remaining -= 1;
                let g_max_hp = self
                    .database
                    .effective_max_hp(&self.database.unit_instances[g_idx]);
                // バリア吸収 (B): guard 役が バリアLv* を持てば 1000×Lv を吸収。
                let guard_dmg = self.apply_barrier_absorb(g_idx, atk_idx, &weapon.class, guard_dmg);
                // アクティブフィールド (B): 確率発動の 500×Lv 吸収。
                let guard_dmg =
                    self.apply_active_field_absorb(g_idx, atk_idx, &weapon.class, guard_dmg);
                // シールド防御 (B): guard 役が Ｓ防御+シールドを持てば被ダメージ半減。
                let guard_dmg = self.apply_shield_defense(g_idx, &weapon.class, guard_dmg);
                // 不死身: 援護防御で身代わりになった guard も撃破させない。
                let guard_dmg = self.cap_damage_for_immortal(g_idx, g_max_hp, guard_dmg);
                let guard_old_dmg = self.database.unit_instances[g_idx].damage;
                self.database.unit_instances[g_idx].damage += guard_dmg;
                let remaining = g_max_hp - self.database.unit_instances[g_idx].damage;
                if remaining <= 0 && self.revive_if_possible(g_idx) {
                    format!(
                        "援護防御: {} が {} の代わりに受けた → 撃破… だが【復活】！",
                        guard_pilot
                            .as_ref()
                            .map(|p| p.nickname.as_str())
                            .unwrap_or("?"),
                        def_pilot.nickname
                    )
                } else if remaining <= 0 {
                    let m = format!(
                        "援護防御: {} が {} の代わりに受けた → 撃破！",
                        guard_pilot
                            .as_ref()
                            .map(|p| p.nickname.as_str())
                            .unwrap_or("?"),
                        def_pilot.nickname
                    );
                    let guard_party = self.database.unit_instances[g_idx].party;
                    self.database.remove_unit_at(g_idx);
                    if let (Some(gp), Some(gu)) = (guard_pilot, guard_unit) {
                        self.fire_destruction_label(&gp.name, &gu.name, guard_party);
                    }
                    m
                } else {
                    // 損傷率イベント (援護防御で guard が閾値を割った場合)。
                    self.fire_damage_threshold_for_unit(g_idx, g_max_hp, guard_old_dmg);
                    format!(
                        "援護防御: {} が {} の代わりに {} ダメージ (残HP {})",
                        guard_pilot
                            .as_ref()
                            .map(|p| p.nickname.as_str())
                            .unwrap_or("?"),
                        def_pilot.nickname,
                        guard_dmg,
                        remaining
                    )
                }
            } else {
                // 通常ダメージ: defender が受ける (防御選択時は applied_damage で半減済)。
                // 不死身: 撃破させないよう被ダメージをキャップ (サンプルの「不死身で
                // 耐える→損傷率 50% で回復＆不死身解除」演出を成立させる)。
                // バリア吸収 (B): 防御側が バリアLv* を持てば 1000×Lv を吸収。
                let applied_damage =
                    self.apply_barrier_absorb(def_idx, atk_idx, &weapon.class, applied_damage);
                // アクティブフィールド (B): 確率発動の 500×Lv 吸収。
                let applied_damage =
                    self.apply_active_field_absorb(def_idx, atk_idx, &weapon.class, applied_damage);
                // シールド防御 (B): 防御側が Ｓ防御+シールドを持てば被ダメージ半減。
                let applied_damage =
                    self.apply_shield_defense(def_idx, &weapon.class, applied_damage);
                let applied_damage =
                    self.cap_damage_for_immortal(def_idx, def_unit.hp, applied_damage);
                let def_old_dmg = self.database.unit_instances[def_idx].damage;
                self.database.unit_instances[def_idx].damage += applied_damage;
                let remaining = def_unit.hp - self.database.unit_instances[def_idx].damage;
                if remaining <= 0 {
                    if self.revive_if_possible(def_idx) {
                        // 精神コマンド「復活」: HP0 でも HP 全快で立ち上がる (1 回で消費)。
                        format!(
                            "{} ({}) → {} ({}) [{}]: 撃破… だが【復活】で立ち上がった！",
                            atk_pilot.nickname,
                            atk_unit.name,
                            def_pilot.nickname,
                            def_unit.name,
                            weapon.name,
                        )
                    } else {
                        // 精神コマンド「努力」: 次の戦闘で得る経験値 2 倍 (1 回で消費)。
                        let doubled = self
                            .database
                            .unit_instances
                            .iter()
                            .find(|u| u.x == cx && u.y == cy)
                            .map(|u| u.has_condition("努力"))
                            .unwrap_or(false);
                        let exp = def_pilot.exp_value * if doubled { 2 } else { 1 };
                        let victim_value = def_unit.value;
                        let mut m = format!(
                            "{} ({}) → {} ({}) [{}]: {}撃破！ EXP +{}{}",
                            atk_pilot.nickname,
                            atk_unit.name,
                            def_pilot.nickname,
                            def_unit.name,
                            weapon.name,
                            if critical {
                                "クリティカル！ "
                            } else {
                                ""
                            },
                            exp,
                            if doubled { " (努力)" } else { "" },
                        );
                        self.database.remove_unit_at(def_idx);
                        // 撃破側ユニットを位置で再特定し、経験値 / 育成 / 資金を付与する
                        // (PilotInstance 成長 + レベルアップイベント + 資金 + 努力/幸運 消費)。
                        if let Some(killer_idx) = self
                            .database
                            .unit_instances
                            .iter()
                            .position(|u| u.x == cx && u.y == cy)
                        {
                            let money = self.award_kill_rewards(
                                killer_idx,
                                exp,
                                victim_value,
                                def_inst.party,
                            );
                            if money > 0 {
                                m.push_str(&format!(" 資金 +{money}"));
                            }
                        }
                        defender_killed = true;
                        m
                    }
                } else {
                    // 損傷率イベント (SRC `損傷率 <unit> <pct>:`): 戦闘ダメージで HP が
                    // 閾値 (例 50%) を割ったら発火する。旧実装は `Damage` コマンド経由
                    // (apply_damage) でしか発火せず、実戦闘では発火していなかった。
                    // サンプルシナリオの「不死身で耐える→損傷率 50% で HP 回復＋不死身
                    // 解除」演出はこの発火に依存する。
                    self.fire_damage_threshold_for_unit(def_idx, def_unit.hp, def_old_dmg);
                    // 攻撃を受けて生存したユニットは気力 +1 (SRC `Unit.cs`: 損傷による気力上昇)。
                    {
                        let dm = &mut self.database.unit_instances[def_idx];
                        dm.morale = (dm.morale + 1).clamp(0, 150);
                    }
                    // 命中時/損傷時 気力増加スキル。
                    self.apply_combat_morale_skills(atk_idx, def_idx, true);
                    // 特殊効果攻撃属性: 命中かつ生存時に確率で状態異常を付与。
                    // 切り払いで無効化された攻撃は特殊効果も発動しない。
                    let inflicted = if parried {
                        Vec::new()
                    } else {
                        self.apply_weapon_special_effects(
                            def_idx, atk_idx, &weapon, &atk_pilot, &def_pilot,
                        )
                    };
                    // 衰 / 滅: クリティカル時に対象の現在 HP / EN を割合減少させる。
                    // 引 / 転: クリティカル時に対象の位置を移す。盗: 資金を奪う。
                    // 写 / 化: 発動者が対象のユニットへ変化する。
                    let decayed = if critical && !parried {
                        let mut d = self.apply_weapon_crit_decay(def_idx, &weapon);
                        d.extend(self.apply_weapon_crit_reposition(def_idx, (cx, cy), &weapon));
                        d.extend(self.apply_weapon_crit_steal(
                            def_idx,
                            atk_idx,
                            atk_party,
                            def_unit.value,
                            &weapon,
                        ));
                        d.extend(self.apply_weapon_crit_copy(atk_idx, def_idx, &weapon));
                        // 習: クリティカル時に対象の「ラーニング可能技」を習得する。
                        d.extend(self.apply_weapon_learning(atk_idx, def_idx, &weapon));
                        d
                    } else {
                        Vec::new()
                    };
                    // 吹 / Ｋ: 命中時に対象を遠ざかる方向へ押し出す。
                    let knocked = if parried {
                        false
                    } else {
                        self.apply_weapon_knockback(
                            def_idx,
                            (cx, cy),
                            &atk_unit_name,
                            &weapon,
                            critical,
                        )
                    };
                    let mut m = format!(
                        "{} → {} [{}]: 命中 {} ダメージ{} (残HP {})",
                        atk_pilot.nickname,
                        def_pilot.nickname,
                        weapon.name,
                        applied_damage,
                        if critical {
                            " クリティカル！"
                        } else {
                            ""
                        },
                        remaining
                    );
                    if !inflicted.is_empty() {
                        m.push_str(&format!(" → {}", inflicted.join("・")));
                    }
                    if !decayed.is_empty() {
                        m.push_str(&format!(" → {}", decayed.join("・")));
                    }
                    if knocked {
                        m.push_str(" → 吹き飛ばし");
                    }
                    m
                }
            }
        } else {
            // 失敗時/回避時 気力増加スキル (攻撃を外した側 / 回避した側)。
            self.apply_combat_morale_skills(atk_idx, def_idx, false);
            format!(
                "{} → {} [{}]: ミス (要 {}%, ロール {})",
                atk_pilot.nickname, def_pilot.nickname, weapon.name, effective_hit, roll
            )
        };
        self.push_message(msg);

        // 戦闘演出 (animate_battle 時のみ)。`animation.txt` で戦闘アニメサブルーチンが
        // 解決でき、かつそれが script_library に存在すればスクリプト再生を優先する
        // (SRC 汎用戦闘アニメ経路)。再生できなければネイティブ演出にフォールバック。
        if self.animate_battle {
            let played = self.try_play_battle_animation(&atk_unit_name, &weapon.name, hit);
            if !played {
                // ネイティブ演出: 攻撃側→防御側タイルへの命中フラッシュ・着弾・ダメージ
                // 数字を `tick` 駆動で短く再生。援護防御の肩代わりは防御側被弾 0 とする。
                let (anim_hit, anim_damage) = if hit && guard_idx.is_none() {
                    (true, applied_damage)
                } else if hit {
                    (true, 0)
                } else {
                    (false, 0)
                };
                self.battle_anim = Some(crate::battle_anim::BattleAnim::new(
                    (cx, cy),
                    (def_inst.x, def_inst.y),
                    anim_hit,
                    anim_damage,
                    defender_killed,
                    crate::battle_anim::AttackKind::from_weapon(&weapon),
                ));
            }
        }

        // 援護防御が発動した場合、サポートガードユニットの UID をシステム変数に記録。
        if let Some(g_idx) = guard_idx {
            // g_idx はダメージ後 (remove の可能性あり) — まだ存在するか確認
            if let Some(gu) = self.database.unit_instances.get(g_idx) {
                let uid = gu.uid.clone();
                self.set_script_var("サポートガードユニットＩＤ".to_string(), uid);
            }
        }

        // 援護攻撃: 攻撃側と同じ陣営の隣接ユニットで `support_attack_remaining > 0`
        // かつ "サポートアタック" / "援護攻撃" 特殊能力を持つユニットが、防御側を
        // 武器射程内に収めていれば 1 回 75% ダメージで追撃。
        // 援護防御でガード機が除去された場合 atk_idx がずれ得るため uid で引き直す。
        let support_uid = match (defender_killed, self.database.idx_by_uid(&atk_uid)) {
            (false, Some(ai)) => self.try_support_attack(ai, def_inst.x, def_inst.y),
            _ => None,
        };
        // 反撃: 防御側が生存しており、攻撃側まで射程内の武器を持っていれば 1 回反撃。
        // ただし回避/防御を選んだ場合は反撃しない (SRC 反撃モード)。反撃 or 自動("")のみ。
        // 先制反撃 (preempted) 済みなら主攻撃後の通常反撃は行わない (SRC: def_mode="先制攻撃")。
        let allow_counter =
            !preempted && def_mode != "回避" && def_mode != "防御" && def_mode != "援護防御";
        let still_alive_def = self
            .database
            .unit_instances
            .iter()
            .any(|u| u.x == def_inst.x && u.y == def_inst.y);
        let counter_weapon = if allow_counter && !defender_killed && still_alive_def {
            self.try_counterattack(def_inst.x, def_inst.y, (cx, cy))
        } else {
            None
        };
        // 撃破時の自動発火ラベル `Destruction <unit>` / `<pilot>` / `破壊 <陣営>` を実行。
        if defender_killed {
            self.fire_destruction_label(&def_pilot.name, &def_unit.name, def_inst.party);
        }

        // 攻撃後イベント発火前に残り戦闘系システム変数を更新。
        // 相手ユニット使用武器 = 反撃武器 (反撃なしなら空文字)。
        if let Some((cname, cnum)) = counter_weapon {
            self.set_script_var("相手ユニット使用武器".to_string(), cname);
            self.set_script_var("相手ユニット使用武器番号".to_string(), cnum.to_string());
        }
        if let Some(uid) = support_uid {
            self.set_script_var("サポートアタックユニットＩＤ".to_string(), uid);
        }

        // SRC `使用後 <unit> <device>:` (`使用後イベント.md`) ─ `攻撃後` の前、
        // attacker 生存時のみ発火。
        crate::event_runtime::fire_after_use_event_labels(self, &after_atk, &weapon.name);

        // SRC `攻撃後 <atk> <def>:` (`攻撃後イベント.md`) ─ 損傷率/破壊
        // ラベル発火後、双方が生存している場合に限り発火。
        crate::event_runtime::fire_after_attack_event_labels(self, &after_atk, &after_def);

        // 再攻撃 (VB5 `Unit.cls:10239-10270`): 主攻撃側が 再攻撃 技能 / 再属性 を持ち、
        // 防御側が生存していれば確率で同じ攻撃をもう一度実行する (1回限り)。武器の EN/弾が
        // 尽きていれば 2回目の武器選択で自然に不発になる。`reattack_in_progress` で使用
        // イベント再発火と 3回目の再攻撃を抑止する。
        if !is_reattack && !defender_killed {
            let def_here = self.database.unit_instances.iter().any(|u| {
                !u.off_map && u.x == def_inst.x && u.y == def_inst.y && u.party == def_inst.party
            });
            if let (Some(ai), true) = (self.database.idx_by_uid(&atk_uid), def_here) {
                if self.rolls_reattack(ai, &atk_pilot, &def_pilot, &weapon) {
                    self.push_message(format!("{} の再攻撃！", atk_pilot.nickname));
                    self.map_cursor = Some((
                        self.database.unit_instances[ai].x,
                        self.database.unit_instances[ai].y,
                    ));
                    self.reattack_in_progress = true;
                    self.attack_resolve_and_run(
                        Some((def_inst.x, def_inst.y)),
                        post_move,
                        def_mode,
                    );
                    self.reattack_in_progress = false;
                }
            }
        }

        self.check_victory();
        true
    }

    /// 再攻撃 (VB5 `Unit.cls:10251-10270`) の発動判定。攻撃側が 再攻撃 技能を持てば
    /// `slevel = (直感 >= 相手直感 ? 2*Lv : Lv)` が `Dice(32)` 以上で発動。武器 `再Ln`
    /// 属性は `n` (レベル無しは 1) が `Dice(16)` 以上で発動。SP効果「再攻撃」condition は
    /// 無条件発動。能力を持たない攻撃側では乱数を消費しない (既存 RNG 列を保つ)。
    fn rolls_reattack(
        &mut self,
        atk_idx: usize,
        atk_pilot: &crate::data::pilot::PilotData,
        def_pilot: &crate::data::pilot::PilotData,
        weapon: &crate::data::unit::WeaponData,
    ) -> bool {
        // SP効果「再攻撃」: 無条件で発動 (SRC IsUnderSpecialPowerEffect("再攻撃"))。
        if self.database.unit_instances[atk_idx].has_condition("再攻撃") {
            return true;
        }
        // 再攻撃 技能。
        let lv = self.unit_pilot_skill_level(atk_idx, "再攻撃");
        if lv > 0 {
            let slevel = if atk_pilot.intuition >= def_pilot.intuition {
                2 * lv
            } else {
                lv
            };
            let dice = (self.next_u32() % 32) as i32 + 1; // 1..=32
            if slevel >= dice {
                return true;
            }
        }
        // 再 武器属性 (`再Ln`、レベル無しは 1)。
        if weapon.class.contains('再') {
            let n = crate::db::weapon_class_level(&weapon.class, '再')
                .map(|f| f as i32)
                .unwrap_or(1);
            let dice = (self.next_u32() % 16) as i32 + 1; // 1..=16
            if n >= dice {
                return true;
            }
        }
        false
    }

    /// `animation.txt` から戦闘アニメサブルーチンを解決し、script_library に**実在する**
    /// ものだけを (準備→攻撃→[命中]) の順に返す。SRC 原典の段階順に対応。
    fn resolve_battle_anim_calls(
        &self,
        atk_unit_data_name: &str,
        weapon: &str,
        hit: bool,
    ) -> Vec<crate::data::animation::ResolvedAnim> {
        use crate::data::animation::WeaponPhase;
        let phases: &[(&str, WeaponPhase)] = if hit {
            &[
                ("準備", WeaponPhase::Prep),
                ("攻撃", WeaponPhase::Attack),
                ("命中", WeaponPhase::Hit),
            ]
        } else {
            &[("準備", WeaponPhase::Prep), ("攻撃", WeaponPhase::Attack)]
        };
        let mut calls = Vec::new();
        for (label, phase) in phases {
            for r in
                self.database
                    .resolve_weapon_animation(atk_unit_data_name, weapon, label, *phase)
            {
                // 実在するラベルのみ再生対象 (未ロードの GBA サブルーチンは飛ばす)。
                if self.script_library().label_pc(&r.subroutine).is_some() {
                    calls.push(r);
                }
            }
        }
        calls
    }

    /// 戦闘アニメをイベントスクリプトとして再生できるなら起動して `true` を返す。
    ///
    /// SRC 汎用戦闘アニメ経路の最小実装: `animation.txt` で解決した `戦闘アニメ_*`
    /// サブルーチンを (準備→攻撃→[命中]) の順に `Call` する合成ドライバを生成し、
    /// イベントエンジンで実行する。サブルーチン内の `PaintPicture` は script_overlay に
    /// 積まれ、`Wait` は `pending_timer` で中断 → `tick` が再開する。
    ///
    /// 起動条件: `animate_battle` ＋ 設定「戦闘アニメを表示する」＋ 解決サブルーチンが
    /// script_library に実在 ＋ 他スクリプト/ダイアログ非実行中。いずれも満たさなければ
    /// `false` を返し、呼び出し側はネイティブ演出にフォールバックする。
    fn try_play_battle_animation(
        &mut self,
        atk_unit_data_name: &str,
        weapon: &str,
        hit: bool,
    ) -> bool {
        if !self.animate_battle || !self.settings.battle_animation {
            return false;
        }
        if self.has_script_context() || self.pending_dialog.is_some() {
            return false;
        }
        let calls = self.resolve_battle_anim_calls(atk_unit_data_name, weapon, hit);
        if calls.is_empty() {
            return false;
        }
        // 合成ドライバ .eve を生成 (対象/相手ユニットＩＤ は呼び出し側で設定済み)。
        let mut src = String::new();
        for c in &calls {
            if c.params.is_empty() {
                src.push_str(&format!("Call {}\n", c.subroutine));
            } else {
                src.push_str(&format!("Call {} {}\n", c.subroutine, c.params));
            }
        }
        let Ok(stmts) = crate::data::event::parse(&src) else {
            return false;
        };
        let pc = crate::event_runtime::library_append(self, &stmts);
        let _ = crate::event_runtime::run_from_pc(self, pc);
        true
    }

    /// `不死身` (撃破阻止): `target_idx` が不死身なら、与ダメージを HP が 10 未満に
    /// ならないようキャップして返す (C# `Unit.attack.cs` の てかげん/不死身 と同一)。
    /// 通常攻撃・援護攻撃・援護防御・反撃の全ダメージ経路で共通に使う。
    fn cap_damage_for_immortal(&self, target_idx: usize, max_hp: i64, dmg: i64) -> i64 {
        if self.database.unit_instances[target_idx].has_condition("不死身") {
            let cur_hp = max_hp - self.database.unit_instances[target_idx].damage;
            if cur_hp <= 10 {
                0
            } else {
                dmg.min(cur_hp - 10)
            }
        } else {
            dmg
        }
    }

    /// シールド防御 (防御能力 (B) 段階・SRC.NET `Unit.cs::CheckShieldFeature` 準拠):
    /// 防御側 `def_idx` が **Ｓ防御技能** + シールド系特殊能力 (シールド/小型/大型/アクティブ
    /// シールド) を持ち、行動可能 (行動不能/麻痺/石化/凍結/睡眠 でない) で武器が 精/殺/浸 で
    /// ないとき、`Ｓ防御Lv(+大型1/アクティブ2) >= Dice(16)` で発動し被ダメージを半減する
    /// (小型=2/3、破属性は 3/4・小型破は 5/6)。発動時は最低 10。実行段 (予測ではなく) で
    /// ロールするため、プレビューは確率的シールドを反映しない (SRC と同じ)。
    ///
    /// 発動しなければ `dmg` をそのまま返す。`バリア`/`フィールド` 系 (S防御非依存の吸収/÷2) は
    /// 対象外で、現状は予測の ÷2 近似 ((A)) のまま。
    /// みがわり (身代わり, SRC `Unit.cls:14855`): 防御側 `def_idx` が `みがわり` condition
    /// (data=身代わりユニット uid) を持つとき、攻撃ダメージ `damage` を身代わりユニットへ
    /// **1 回だけ**肩代わりさせる。肩代わりが成立したら身代わりユニットの愛称を返し
    /// condition を消費する。身代わりが盤上に居ない / 防御側自身なら `None`（通常処理）。
    ///
    /// 身代わりユニットは自身の バリア/フィールド/シールド/不死身 を適用してダメージを
    /// 受け、HP0 なら（復活が無ければ）撃破・破壊イベント発火・勝敗判定まで処理する。
    /// 近似: 肩代わりダメージは防御側に向かっていた値をそのまま用い、身代わりの装甲で
    /// 再計算はしない（SRC は再計算する）。Ｓ防御依存シールド等は身代わり基準で適用。
    fn apply_migawari(
        &mut self,
        def_idx: usize,
        atk_idx: usize,
        weapon_class: &str,
        damage: i64,
    ) -> Option<String> {
        let sub_uid = self.database.unit_instances[def_idx]
            .conditions
            .iter()
            .find(|c| c.name == "みがわり" && !c.data.is_empty())
            .map(|c| c.data.clone())?;
        let sub_idx = self
            .database
            .idx_by_uid(&sub_uid)
            .filter(|&i| i != def_idx && !self.database.unit_instances[i].off_map)?;
        // condition を消費（1 回限り）。
        self.database.unit_instances[def_idx]
            .conditions
            .retain(|c| c.name != "みがわり");
        // 身代わりユニットがダメージを受ける（自身の バリア/フィールド/シールド/不死身）。
        let sub_max_hp = self
            .database
            .effective_max_hp(&self.database.unit_instances[sub_idx]);
        let dmg = self.apply_barrier_absorb(sub_idx, atk_idx, weapon_class, damage);
        let dmg = self.apply_active_field_absorb(sub_idx, atk_idx, weapon_class, dmg);
        let dmg = self.apply_shield_defense(sub_idx, weapon_class, dmg);
        let dmg = self.cap_damage_for_immortal(sub_idx, sub_max_hp, dmg);
        let sub_name = self.database.unit_instances[sub_idx].unit_data_name.clone();
        let sub_nick = self
            .database
            .pilot_by_name(&self.database.unit_instances[sub_idx].pilot_name)
            .map(|p| p.nickname.clone())
            .unwrap_or_else(|| sub_name.clone());
        self.database.unit_instances[sub_idx].damage += dmg;
        if sub_max_hp - self.database.unit_instances[sub_idx].damage <= 0
            && !self.revive_if_possible(sub_idx)
        {
            let p = self.database.unit_instances[sub_idx].pilot_name.clone();
            let party = self.database.unit_instances[sub_idx].party;
            self.database.remove_unit_at(sub_idx);
            crate::event_runtime::fire_destruction_labels(self, &p, &sub_name, party);
            self.check_victory();
        }
        Some(sub_nick)
    }

    fn apply_shield_defense(&mut self, def_idx: usize, weapon_class: &str, dmg: i64) -> i64 {
        if dmg <= 0 || self.unit_pilot_skill_level(def_idx, "Ｓ防御") <= 0 {
            return dmg;
        }
        // 精 / 殺 / 浸 属性にはシールド防御不可。
        if weapon_class.contains('精') || weapon_class.contains('殺') || weapon_class.contains('浸')
        {
            return dmg;
        }
        // C# 探索順 (シールド→小型→大型→アクティブ) で最初に持つものを使い、発動率と半減率を決める。
        // 行動不能系の状態異常下では発動しない。
        let (prob, is_small, fname) = {
            let u = &self.database.unit_instances[def_idx];
            if u.has_condition("行動不能")
                || u.has_condition("麻痺")
                || u.has_condition("石化")
                || u.has_condition("凍結")
                || u.has_condition("睡眠")
            {
                return dmg;
            }
            let sdef = self.unit_pilot_skill_level(def_idx, "Ｓ防御");
            let h = |name: &str| crate::feature::has_feature(&u.active_features, name);
            if h("シールド") {
                (sdef, false, "シールド")
            } else if h("小型シールド") {
                (sdef, true, "小型シールド")
            } else if h("大型シールド") {
                (sdef + 1, false, "大型シールド")
            } else if h("アクティブシールド") {
                (sdef + 2, false, "アクティブシールド")
            } else {
                return dmg;
            }
        };
        // Ｓ防御Lv(+補正) >= Dice(16) で発動。
        let roll = (self.next_u32() % 16) as i32 + 1;
        if prob < roll {
            return dmg;
        }
        let is_break = weapon_class.contains('破');
        let reduced = match (is_break, is_small) {
            (true, true) => 5 * dmg / 6,
            (true, false) => 3 * dmg / 4,
            (false, true) => 2 * dmg / 3,
            (false, false) => dmg / 2,
        };
        let reduced = if (1..10).contains(&reduced) {
            10
        } else {
            reduced
        };
        self.push_message(format!("シールド防御({fname}): {dmg} → {reduced} ダメージ"));
        reduced
    }

    /// バリア吸収強度 (防御能力 (B)): 防御側 `def_idx` の `バリア`/`バリアLv*` 特殊能力が
    /// 武器属性に対し有効なとき `1000×Lv` を返す (複数は最大、該当無しは 0)。対象属性
    /// (`バリアLv*=別名 対象属性 …` の 2 番目のトークン) は `defense_attr_matches` で
    /// 相殺/中和 (バリア・フィールドのオプション・防御系特殊能力.md): 攻撃側 `attacker_idx` が
    /// 防御側に **隣接 (マンハッタン1)** していて、防御側のバリア/フィールドと **同名 (別名=値の
    /// 1 番目トークンが一致)** のバリア/フィールドを持つとき、その Lv を返す (複数は最大)。
    /// 相殺=完全無効 / 中和=この Lv 分だけ効果減少、に使う。条件を満たさなければ None。
    /// SRC.NET `Unit.cs::IsSameCategory` (別名一致) + 隣接判定 準拠。
    fn same_named_adjacent_barrier_level(
        &self,
        attacker_idx: usize,
        def_idx: usize,
        alias: &str,
    ) -> Option<i32> {
        if alias.is_empty() {
            return None;
        }
        let a = &self.database.unit_instances[attacker_idx];
        let d = &self.database.unit_instances[def_idx];
        if a.off_map || d.off_map {
            return None;
        }
        let dist =
            (i64::from(a.x) - i64::from(d.x)).abs() + (i64::from(a.y) - i64::from(d.y)).abs();
        if dist != 1 {
            return None;
        }
        let mut best: Option<i32> = None;
        for f in &a.active_features {
            if !f.is_active {
                continue;
            }
            let lv = crate::feature::feature_level(std::slice::from_ref(f), "バリア")
                .or_else(|| crate::feature::feature_level(std::slice::from_ref(f), "フィールド"))
                .or_else(|| {
                    crate::feature::feature_level(std::slice::from_ref(f), "アクティブフィールド")
                });
            if let Some(lv) = lv {
                if f.value.split_whitespace().next().unwrap_or("") == alias {
                    best = Some(best.map_or(lv, |b| b.max(lv)));
                }
            }
        }
        best
    }

    /// バリア吸収 (防御能力 (B)・SRC `バリア`/`フィールド` 特殊能力): 防御側の バリア=1000×Lv /
    /// フィールド=500×Lv が、対象属性に該当する攻撃を吸収する (吸収後の残ダメージを返す。
    /// 完全吸収は 0)。確率ではなく**確定**だが、予測シグネチャ変更を避け実行段で適用する。
    /// 攻撃側 `直撃` / 防御側 `バリア中和` で無効。`attacker_idx` はダメージを与える側。
    ///
    /// **knobs (忠実性)**: 各特殊能力の値 `…=別名 対象属性 ＥＮ消費 必要気力 …` から ＥＮ消費
    /// (3 番目, 既定 0) と 必要気力 (4 番目, 既定 0) を読み、`気力≧必要気力` かつ `残EN≧ＥＮ消費`
    /// のときのみ発動し、発動した能力ぶんの ＥＮ を消費する (`&mut self`)。対象属性は 2 番目
    /// (省略=全)。`アクティブフィールド` は確率発動なので別経路、`解説…` 値はヘルプ文で対象外。
    fn apply_barrier_absorb(
        &mut self,
        def_idx: usize,
        attacker_idx: usize,
        weapon_class: &str,
        dmg: i64,
    ) -> i64 {
        if dmg <= 0 {
            return dmg;
        }
        if self.database.unit_instances[attacker_idx].has_condition("直撃")
            || self.database.unit_instances[def_idx].has_condition("バリア中和")
        {
            return dmg;
        }
        let morale = self.database.unit_instances[def_idx].morale;
        let max_en = self
            .database
            .effective_max_en(&self.database.unit_instances[def_idx]);
        // 発動した能力の吸収強度合計と消費 ＥＮ 合計を、features を借りる scope 内で算出。
        let (total_strength, total_en) = {
            let u = &self.database.unit_instances[def_idx];
            let mut avail_en = max_en - u.en_consumed;
            let mut strength = 0i64;
            let mut en_used = 0i32;
            for f in &u.active_features {
                if !f.is_active || f.value.trim_start().starts_with("解説") {
                    continue;
                }
                if crate::feature::feature_name_matches_base(&f.name, "アクティブフィールド")
                {
                    continue; // 確率発動は apply_active_field_absorb
                }
                let (base_mult, lv) = if let Some(lv) =
                    crate::feature::feature_level(std::slice::from_ref(f), "バリア")
                {
                    (1000i64, lv)
                } else if let Some(lv) =
                    crate::feature::feature_level(std::slice::from_ref(f), "フィールド")
                {
                    (500i64, lv)
                } else {
                    continue;
                };
                let toks: Vec<&str> = f.value.split_whitespace().collect();
                let target = toks.get(1).copied().unwrap_or("");
                if !(target.is_empty() || Self::defense_attr_matches(weapon_class, target)) {
                    continue;
                }
                let en_cost: i32 = toks.get(2).and_then(|t| t.parse().ok()).unwrap_or(0);
                let morale_req: i32 = toks.get(3).and_then(|t| t.parse().ok()).unwrap_or(0);
                if morale < morale_req || avail_en < en_cost {
                    continue; // 気力不足 / EN 不足では発動しない
                }
                // 相殺/中和 (オプション・トークン 5 番目以降): 隣接する同名バリア/フィールドを
                // 持つ攻撃側で、相殺=完全無効 / 中和=攻撃側 Lv 分だけ効果減少。
                let mut eff_lv = lv;
                let sousai = toks.iter().skip(4).any(|t| t.contains("相殺"));
                let chuwa = toks.iter().skip(4).any(|t| t.contains("中和"));
                if sousai || chuwa {
                    let alias = toks.first().copied().unwrap_or("");
                    if let Some(atk_lv) =
                        self.same_named_adjacent_barrier_level(attacker_idx, def_idx, alias)
                    {
                        if sousai {
                            continue; // 相殺: 完全無効
                        }
                        eff_lv = (lv - atk_lv).max(0);
                        if eff_lv <= 0 {
                            continue;
                        }
                    }
                }
                avail_en -= en_cost;
                en_used += en_cost;
                strength += base_mult * i64::from(eff_lv);
            }
            (strength, en_used)
        };
        if total_strength <= 0 {
            return dmg;
        }
        self.database.unit_instances[def_idx].en_consumed += total_en;
        (dmg - total_strength).max(0)
    }

    /// アクティブフィールド吸収 (防御能力 (B)・防御系特殊能力.md): フィールドの**確率発動版**。
    /// 防御側が `アクティブフィールドLv*` + Ｓ防御技能を持ち、`Ｓ防御Lv >= Dice(16)` で発動し
    /// 対象属性に該当する攻撃の `500×Lv` を吸収する。攻撃側 `直撃` / 防御側 `バリア中和` で無効。
    /// 確率発動なので予測には乗せず実行段で適用する (`&mut self`)。
    fn apply_active_field_absorb(
        &mut self,
        def_idx: usize,
        attacker_idx: usize,
        weapon_class: &str,
        dmg: i64,
    ) -> i64 {
        if dmg <= 0 || self.unit_pilot_skill_level(def_idx, "Ｓ防御") <= 0 {
            return dmg;
        }
        if self.database.unit_instances[attacker_idx].has_condition("直撃")
            || self.database.unit_instances[def_idx].has_condition("バリア中和")
        {
            return dmg;
        }
        // 吸収強度 (500×Lv) + ＥＮ消費 を、対象属性一致 + 気力/EN ゲートを満たす中から最大で選ぶ。
        // 値: `…=別名 対象属性 ＥＮ消費 必要気力 …` (既定 ＥＮ消費0 / 必要気力0)。解説は除外。
        let morale = self.database.unit_instances[def_idx].morale;
        let max_en = self
            .database
            .effective_max_en(&self.database.unit_instances[def_idx]);
        let (strength, en_cost) = {
            let u = &self.database.unit_instances[def_idx];
            let avail_en = max_en - u.en_consumed;
            let mut best = (0i64, 0i32);
            for f in &u.active_features {
                if !f.is_active || f.value.trim_start().starts_with("解説") {
                    continue;
                }
                let Some(lv) =
                    crate::feature::feature_level(std::slice::from_ref(f), "アクティブフィールド")
                else {
                    continue;
                };
                let toks: Vec<&str> = f.value.split_whitespace().collect();
                let target = toks.get(1).copied().unwrap_or("");
                if !(target.is_empty() || Self::defense_attr_matches(weapon_class, target)) {
                    continue;
                }
                let ec: i32 = toks.get(2).and_then(|t| t.parse().ok()).unwrap_or(0);
                let mr: i32 = toks.get(3).and_then(|t| t.parse().ok()).unwrap_or(0);
                if morale < mr || avail_en < ec {
                    continue;
                }
                let s = 500 * i64::from(lv);
                if s > best.0 {
                    best = (s, ec);
                }
            }
            best
        };
        if strength <= 0 {
            return dmg;
        }
        // Ｓ防御Lv >= Dice(16) で発動。発動時に ＥＮ を消費。
        let sdef = self.unit_pilot_skill_level(def_idx, "Ｓ防御");
        let roll = (self.next_u32() % 16) as i32 + 1;
        if sdef < roll {
            return dmg;
        }
        self.database.unit_instances[def_idx].en_consumed += en_cost;
        let reduced = (dmg - strength).max(0);
        self.push_message(format!("アクティブフィールド: {dmg} → {reduced} ダメージ"));
        reduced
    }

    /// ＥＣＭ エリア補正 (防御能力 (B)・SRC.NET `Unit.cs:10419`): 防御側 `def_idx` の周囲
    /// (中心からマンハッタン 3 以内かつ各軸 ±2 マス) を走査し、**防御側の味方**の最大
    /// ＥＣＭ Lv (ecm) と **攻撃側の味方**の最大 ＥＣＭ Lv (eccm) を求め、命中率に掛ける
    /// 係数 `(100 − f×max(ecm−eccm,0)) / 100` を返す (f は ホーミング `Ｈ` 武器で 10、他 5)。
    /// 補正が無ければ 1.0。`predict_with_status_terrain` の `ecm_hit_mult` 引数へ渡す。
    fn ecm_hit_mult(&self, def_idx: usize, attacker_idx: usize, weapon_class: &str) -> f64 {
        let def = &self.database.unit_instances[def_idx];
        let (cx, cy, def_party) = (i64::from(def.x), i64::from(def.y), def.party);
        let atk_party = self.database.unit_instances[attacker_idx].party;
        let mut ecm = 0i32;
        let mut eccm = 0i32;
        for u in &self.database.unit_instances {
            if u.off_map {
                continue;
            }
            let ddx = (i64::from(u.x) - cx).abs();
            let ddy = (i64::from(u.y) - cy).abs();
            if ddx > 2 || ddy > 2 || ddx + ddy > 3 {
                continue;
            }
            let lv = crate::feature::feature_level(&u.active_features, "ＥＣＭ").unwrap_or(0);
            if lv <= 0 {
                continue;
            }
            if !u.party.is_hostile_to(def_party) {
                ecm = ecm.max(lv);
            }
            if !u.party.is_hostile_to(atk_party) {
                eccm = eccm.max(lv);
            }
        }
        let diff = (ecm - eccm).max(0);
        if diff == 0 {
            return 1.0;
        }
        let f = if weapon_class.contains('Ｈ') { 10 } else { 5 };
        f64::from((100 - f * diff).max(0)) / 100.0
    }

    /// 回避系特殊能力 (防御能力 (B)・SRC.NET `Unit.cs::CheckDodgeFeature` 準拠): 命中する
    /// はずの攻撃を完全回避するか判定する (`true` なら攻撃はミス扱い)。
    /// 分身 は気力 130 以上で 50% (`Dice(2)`)、超回避Lv「n」 は `n >= Dice(10)` ≈ 10n%。
    /// 攻撃側 `直撃` / 防御側の行動不能系状態では発動しない。命中ロールが当たった
    /// (would-hit) ときだけ呼ぶこと (SRC 同様・無駄な乱数消費を避ける)。
    /// 予測 (プレビュー) には反映しない＝SRC の HitProbability も 分身/超回避 を畳み込まない。
    fn check_dodge_feature(
        &mut self,
        def_idx: usize,
        attacker_idx: usize,
        _weapon_class: &str,
    ) -> bool {
        // 直撃 (攻撃側) で回避無効。
        if self.database.unit_instances[attacker_idx].has_condition("直撃") {
            return false;
        }
        let (morale, has_bunshin, chougaihi_lv) = {
            let u = &self.database.unit_instances[def_idx];
            // 能動防御は行動可能でないと発動しない。
            if u.has_condition("行動不能")
                || u.has_condition("麻痺")
                || u.has_condition("石化")
                || u.has_condition("凍結")
                || u.has_condition("睡眠")
            {
                return false;
            }
            (
                u.morale,
                crate::feature::has_feature(&u.active_features, "分身"),
                crate::feature::feature_level(&u.active_features, "超回避").unwrap_or(0),
            )
        };
        // 分身: 気力130以上で 50% (Dice(2))。
        if has_bunshin && morale >= 130 && (self.next_u32() % 2) == 0 {
            self.push_message("分身: 攻撃を回避！".to_string());
            return true;
        }
        // 超回避Lv*: Lv >= Dice(10) ≈ 10×Lv%。
        if chougaihi_lv > 0 {
            let roll = (self.next_u32() % 10) as i32 + 1;
            if chougaihi_lv >= roll {
                self.push_message("超回避: 攻撃を回避！".to_string());
                return true;
            }
        }
        false
    }

    /// 被弾して生存した `target_idx` に対し `損傷率 <unit> <pct>:` 閾値イベントを
    /// 発火する。`old_dmg` はこの被弾前の累積ダメージ。撃破された (除去済み) 場合は
    /// 呼ばない。通常攻撃・援護攻撃・援護防御・反撃の被弾生存経路で共通に使う。
    fn fire_damage_threshold_for_unit(&mut self, target_idx: usize, max_hp: i64, old_dmg: i64) {
        let (pilot_name, unit_data_name, party) = {
            let u = &self.database.unit_instances[target_idx];
            (u.pilot_name.clone(), u.unit_data_name.clone(), u.party)
        };
        let new_dmg = self.database.unit_instances[target_idx].damage;
        crate::event_runtime::fire_damage_threshold_labels(
            self,
            &pilot_name,
            &unit_data_name,
            party,
            old_dmg,
            new_dmg,
            max_hp,
        );
    }

    /// `atk_idx` のユニットが (def_x, def_y) を攻撃した直後、同陣営の隣接ユニットで
    /// サポートアタック特殊能力を持つものから 1 体追撃。残り回数を 1 消費。
    /// 命中・ダメージは `predict_with_status` のダメージを 75% に減衰させる。
    /// 戻り値: 援護攻撃を実施したユニットの UID / サポートアタック不能なら `None`。
    fn try_support_attack(&mut self, atk_idx: usize, def_x: u32, def_y: u32) -> Option<String> {
        let atk_party = self.database.unit_instances[atk_idx].party;
        let atk_pos = (
            self.database.unit_instances[atk_idx].x,
            self.database.unit_instances[atk_idx].y,
        );
        // 隣接した同盟陣営 (味方↔ＮＰＣ 含む) でサポートアタック可能なユニットを探す
        let supporter_idx = self
            .database
            .unit_instances
            .iter()
            .enumerate()
            .find(|(i, u)| {
                *i != atk_idx
                    && u.party.is_ally_of(atk_party)
                    && u.support_attack_remaining > 0
                    && Self::is_adjacent((u.x, u.y), atk_pos)
                    && u.conditions
                        .iter()
                        .any(|c| c.name.contains("サポートアタック") || c.name.contains("援護攻撃"))
            })
            .map(|(i, _)| i);
        let sup_idx = supporter_idx?;

        let def_idx = self
            .database
            .unit_instances
            .iter()
            .position(|u| u.x == def_x && u.y == def_y)?;

        // 援護攻撃を行うユニットの UID を退避 (戻り値用)
        let sup_uid = self.database.unit_instances[sup_idx].uid.clone();

        let (sup_pilot, sup_unit) = self.database.effective_combat_data(sup_idx)?;
        let dist = combat::manhattan(
            (
                self.database.unit_instances[sup_idx].x,
                self.database.unit_instances[sup_idx].y,
            ),
            (def_x, def_y),
        );
        let weapon = self.best_firable_weapon_in_range(sup_idx, &sup_unit, dist)?;
        // 援護武器の EN・残弾消費 (撃破前=index 失効前に消費)。
        if let Some(wi) = sup_unit.weapons.iter().position(|w| w.name == weapon.name) {
            self.consume_weapon_resources(sup_idx, wi);
        }
        let def_inst = self.database.unit_instances[def_idx].clone();
        let (def_pilot, mut def_unit) = self.database.effective_combat_data(def_idx)?;
        // 防御特性 (弱点→装甲半減 / 吸収→無視) を援護攻撃でも一貫適用。
        self.apply_defense_armor_mod(def_idx, &weapon.class, &mut def_unit);
        let terrain_id = self
            .database
            .map
            .as_ref()
            .map(|m| m.cell(def_inst.x, def_inst.y).terrain_id)
            .unwrap_or(0);
        let def_hit_mod = self.database.terrain_hit_mod(terrain_id);
        let def_damage_mod = self.database.terrain_damage_mod(terrain_id);
        let sup_morale = self.database.unit_instances[sup_idx].morale;
        let def_morale_sup = def_inst.morale;
        let mut sup_statuses: Vec<String> = self.database.unit_instances[sup_idx]
            .conditions
            .iter()
            .map(|c| c.name.clone())
            .collect();
        push_combat_feature_statuses(&self.database.unit_instances[sup_idx], &mut sup_statuses);
        let mut def_statuses: Vec<String> =
            def_inst.conditions.iter().map(|c| c.name.clone()).collect();
        push_combat_feature_statuses(&def_inst, &mut def_statuses);
        let sup_env = self.terrain_env_at(
            self.database.unit_instances[sup_idx].x,
            self.database.unit_instances[sup_idx].y,
        );
        let def_env = self.terrain_env_at(def_inst.x, def_inst.y);
        // 援護攻撃側/防御側の与・被ダメージ修正を sp.txt から解決。
        let dmg_levels = self
            .database
            .damage_spirit_levels(&sup_statuses, &def_statuses);
        let preview = combat::predict_with_status_terrain(
            &sup_pilot,
            &sup_unit,
            &weapon,
            &def_pilot,
            &def_unit,
            def_hit_mod,
            def_damage_mod,
            sup_morale,
            def_morale_sup,
            &sup_statuses,
            &def_statuses,
            sup_env,
            def_env,
            dmg_levels,
            self.ecm_hit_mult(def_idx, sup_idx, &weapon.class),
        );
        // 散 属性武器の距離補正 (援護攻撃側 → 防御側)。
        let sup_def_dist = combat::manhattan(
            (
                self.database.unit_instances[sup_idx].x,
                self.database.unit_instances[sup_idx].y,
            ),
            (
                self.database.unit_instances[def_idx].x,
                self.database.unit_instances[def_idx].y,
            ),
        );
        let preview = preview.apply_scatter(&weapon.class, sup_def_dist);
        let roll = (self.next_u32() % 100) as i32;
        // 回避系特殊能力 (B): 援護攻撃も 分身/超回避 で完全回避され得る。
        let hit =
            roll < preview.hit_chance && !self.check_dodge_feature(def_idx, sup_idx, &weapon.class);
        // SP コスト消費 (= サポートアタック回数を 1 減らす)
        self.database.unit_instances[sup_idx].support_attack_remaining -= 1;
        if hit {
            let base = (preview.damage as f64 * 0.75) as i64;
            // 防御特性 (耐性÷2 / 無効化0 / 吸収=回復)。弱点の装甲半減は予測前に適用済み。
            let dmg = self.defense_attribute_damage(def_idx, &weapon.class, base);
            // みがわり (身代わり, 最優先・SRC Unit.cls:14855): 防御側が みがわり を持てば
            // 身代わりが肩代わりし、防御側への被ダメージは 0 になる。
            let dmg = if let Some(sub) = self.apply_migawari(def_idx, sup_idx, &weapon.class, dmg) {
                self.push_message(format!(
                    "みがわり: {} が {} の身代わりになった！",
                    sub, def_pilot.nickname
                ));
                0
            } else {
                // バリア/アクティブフィールド/シールド吸収 → 不死身キャップ。
                let dmg = self.apply_barrier_absorb(def_idx, sup_idx, &weapon.class, dmg);
                let dmg = self.apply_active_field_absorb(def_idx, sup_idx, &weapon.class, dmg);
                let dmg = self.apply_shield_defense(def_idx, &weapon.class, dmg);
                self.cap_damage_for_immortal(def_idx, def_unit.hp, dmg)
            };
            let def_old_dmg = self.database.unit_instances[def_idx].damage;
            self.database.unit_instances[def_idx].damage += dmg;
            self.push_message(format!(
                "{} のサポートアタック [{}] → {}: {} ダメージ",
                sup_pilot.nickname, weapon.name, def_pilot.nickname, dmg
            ));
            let remaining = def_unit.hp - self.database.unit_instances[def_idx].damage;
            if remaining <= 0 && !self.revive_if_possible(def_idx) {
                // 援護攻撃側 (sup) が撃破 → 報酬は援護側へ。除去前に位置を退避。
                let (sx, sy) = (
                    self.database.unit_instances[sup_idx].x,
                    self.database.unit_instances[sup_idx].y,
                );
                let doubled = self.database.unit_instances[sup_idx].has_condition("努力");
                let exp = def_pilot.exp_value * if doubled { 2 } else { 1 };
                let victim_value = def_unit.value;
                let victim_party = self.database.unit_instances[def_idx].party;
                self.database.remove_unit_at(def_idx);
                self.fire_destruction_label(&def_pilot.name, &def_unit.name, victim_party);
                if let Some(killer_idx) = self
                    .database
                    .unit_instances
                    .iter()
                    .position(|u| u.x == sx && u.y == sy)
                {
                    let money =
                        self.award_kill_rewards(killer_idx, exp, victim_value, victim_party);
                    if money > 0 {
                        self.push_message(format!("  援護撃破: 資金 +{money}"));
                    }
                }
            } else if remaining > 0 {
                // 損傷率イベント (援護攻撃で HP が閾値を割った場合)。
                self.fire_damage_threshold_for_unit(def_idx, def_unit.hp, def_old_dmg);
                // 攻撃を受けて生存したユニットは気力 +1 (SRC: 損傷による気力上昇)。
                {
                    let dm = &mut self.database.unit_instances[def_idx];
                    dm.morale = (dm.morale + 1).clamp(0, 150);
                }
                // 命中時/損傷時 気力増加スキル。
                self.apply_combat_morale_skills(sup_idx, def_idx, true);
                // 生存: 援護武器の特殊効果攻撃属性 (状態異常付与) を防御側へ proc。
                let inflicted = self.apply_weapon_special_effects(
                    def_idx, sup_idx, &weapon, &sup_pilot, &def_pilot,
                );
                if !inflicted.is_empty() {
                    self.push_message(format!("  → {}", inflicted.join("・")));
                }
            }
        } else {
            // 失敗時/回避時 気力増加スキル。
            self.apply_combat_morale_skills(sup_idx, def_idx, false);
            self.push_message(format!("{} のサポートアタックはミス", sup_pilot.nickname));
        }
        Some(sup_uid)
    }

    fn is_adjacent(a: (u32, u32), b: (u32, u32)) -> bool {
        let dx = (a.0 as i64 - b.0 as i64).abs();
        let dy = (a.1 as i64 - b.1 as i64).abs();
        dx + dy == 1
    }

    /// 援護防御ユニット検索: `def_idx` のユニットが `expected_damage` のダメージを
    /// 受けようとしている際、代わりにダメージを受けてくれる隣接ユニットを探す。
    ///
    /// SRC `Unit.cs::LookForSupportGuard` に準拠。
    /// 戻り値: 援護防御を行うユニットのインデックス / 該当なしなら `None`。
    fn try_find_support_guard(&self, def_idx: usize, expected_damage: i64) -> Option<usize> {
        let def_inst = &self.database.unit_instances[def_idx];
        let def_pos = (def_inst.x, def_inst.y);
        let def_party = def_inst.party;
        let def_unit = self.database.unit_by_name(&def_inst.unit_data_name)?;
        let def_max_hp = def_unit.hp;
        let def_current_hp = def_max_hp - def_inst.damage;

        // ダメージが最小閾値 (MaxHP の 5% or 現 HP の 20%) を下回る場合は発動しない。
        // (SRC: `my_dmg < MaxHP / 20 & my_dmg < HP / 5` → 両方以下なら skip)
        if expected_damage < def_max_hp / 20 && expected_damage < def_current_hp / 5 {
            return None;
        }

        let mut best: Option<(usize, i64)> = None; // (index, current_hp)
        for (i, u) in self.database.unit_instances.iter().enumerate() {
            if i == def_idx {
                continue;
            }
            // 援護できるのは同盟陣営 (同陣営 + 味方↔ＮＰＣ)。
            if !u.party.is_ally_of(def_party) {
                continue;
            }
            // 隣接していること
            if !Self::is_adjacent((u.x, u.y), def_pos) {
                continue;
            }
            // 援護防御能力を持つこと ("サポートガード"/"援護防御"/"援護" のいずれか)
            let has_guard_ability = u.conditions.iter().any(|c| {
                c.name.contains("サポートガード") || c.name.contains("援護防御") || c.name == "援護"
            }) || u.active_features.iter().any(|f| {
                f.is_active
                    && (f.name.contains("サポートガード")
                        || f.name.contains("援護防御")
                        || f.name == "援護")
            });
            if !has_guard_ability {
                continue;
            }
            // 援護防御回数が残っていること
            if u.support_guard_remaining <= 0 {
                continue;
            }
            // 混乱/暴走/恐怖/狂戦士状態でないこと
            if u.has_condition("混乱")
                || u.has_condition("暴走")
                || u.has_condition("恐怖")
                || u.has_condition("狂戦士")
            {
                continue;
            }
            // 代わりに受けても撃破されないこと (50% ダメージで HP が残る)
            let guard_unit_data = match self.database.unit_by_name(&u.unit_data_name) {
                Some(d) => d,
                None => continue,
            };
            let guard_max_hp = guard_unit_data.hp;
            let guard_current_hp = guard_max_hp - u.damage;
            let guard_dmg = expected_damage / 2; // 防御扱い = 50%
            if guard_dmg >= guard_current_hp {
                // 倒れてしまうのでかばわない
                continue;
            }
            // HP が最も高いユニットを優先
            if let Some((_, best_hp)) = best {
                if guard_current_hp <= best_hp {
                    continue;
                }
            }
            best = Some((i, guard_current_hp));
        }
        best.map(|(i, _)| i)
    }

    /// 反撃メニューに「援護防御」選択肢を出すべきか (read-only)。`def_uid` の隣接に
    /// 援護防御可能な同盟ユニット (能力あり・残回数 > 0・混乱/暴走/恐怖/狂戦士でない) が
    /// いれば真。ダメージ依存の閾値/生存判定は解決時 (`try_find_support_guard`) に委ねる
    /// ため、ここでは省く (選んでも条件を満たさなければ不発になるだけ)。
    fn support_guard_available(&self, def_uid: &str) -> bool {
        let Some(def) = self.database.unit_by_uid(def_uid) else {
            return false;
        };
        let def_pos = (def.x, def.y);
        let def_party = def.party;
        self.database.unit_instances.iter().any(|u| {
            u.uid != def_uid
                && u.party.is_ally_of(def_party)
                && !u.off_map
                && Self::is_adjacent((u.x, u.y), def_pos)
                && u.support_guard_remaining > 0
                && (u.conditions.iter().any(|c| {
                    c.name.contains("サポートガード")
                        || c.name.contains("援護防御")
                        || c.name == "援護"
                }) || u.active_features.iter().any(|f| {
                    f.is_active
                        && (f.name.contains("サポートガード")
                            || f.name.contains("援護防御")
                            || f.name == "援護")
                }))
                && !u.has_condition("混乱")
                && !u.has_condition("暴走")
                && !u.has_condition("恐怖")
                && !u.has_condition("狂戦士")
        })
    }

    /// 撃破時に自動発火するラベルを試行。最初に見つかったものを 1 つだけ投函。
    /// 章ローカルイベントなので現ステージファイルにスコープする
    /// (`post_stage_event_label`)。スクリプト実行中なら完了後に実行される。
    /// 戦闘でユニットを撃破したときの破壊イベント発火。SRC `破壊 <name>` (破壊
    /// イベント) と、該当陣営が全滅していれば `全滅 <party>` (全滅イベント) を発火する。
    ///
    /// 旧実装は誤って `撃破 <name>` ラベル (原典に存在しない綴) を発火し、かつ全滅
    /// イベントを一切発火しなかったため、**戦闘で最後の敵を撃破してもシナリオの
    /// `破壊 <name>` / `全滅 敵 → クリア` が走らず進行不能** だった (実機報告: ターンだけ
    /// 増えて撃破しても何も起こらない)。`.eve` `Kill`/`Damage` 経路で使われている
    /// 正しい `fire_destruction_labels` (破壊 + 全滅) に委譲して一本化する。
    fn fire_destruction_label(&mut self, pilot: &str, unit: &str, party: crate::Party) {
        crate::event_runtime::fire_destruction_labels(self, pilot, unit, party);
    }

    /// 先制攻撃 (SRC `COM.bas:1040-1058`): 攻撃側が `def_pos` の防御側を攻撃する際、
    /// 防御側が反撃武器を射程内に持ち、以下のいずれかで先手を取れるなら主攻撃の**前に**
    /// 防御側が反撃する。成立条件 (いずれか): 攻撃側武器が `後`(後攻) / 防御側反撃武器が
    /// `先`(先制) 属性、`カウンター` SP (condition)、`先読み` 技能 `Lv>=Dice(16)`、`カウンター`
    /// 技能の使用回数残あり (`used<Lv`、使うたび加算)。成立したら `true` (= 主攻撃後の通常反撃を
    /// 抑止)。乱数は `先読み` 技能保持時のみ消費する (能力非保持ユニットの RNG 列を保つ)。
    fn try_preemptive_counter(
        &mut self,
        def_idx: usize,
        atk_pos: (u32, u32),
        def_pos: (u32, u32),
        atk_weapon_class: &str,
    ) -> bool {
        if self.database.unit_instances[def_idx].attack_disabled() {
            return false;
        }
        let Some(atk_idx) = self
            .database
            .unit_instances
            .iter()
            .position(|u| u.x == atk_pos.0 && u.y == atk_pos.1)
        else {
            return false;
        };
        if !self.database.unit_instances[def_idx]
            .party
            .is_hostile_to(self.database.unit_instances[atk_idx].party)
        {
            return false;
        }
        // 反撃武器が射程内にあるか。
        let dist = combat::manhattan(def_pos, atk_pos);
        let Some((_, def_unit)) = self.database.effective_combat_data(def_idx) else {
            return false;
        };
        let Some(cw) = self.best_firable_weapon_in_range(def_idx, &def_unit, dist) else {
            return false;
        };
        let counter_lv = self.unit_pilot_skill_level(def_idx, "カウンター");
        let foresight_lv = self.unit_pilot_skill_level(def_idx, "先読み");
        let used = self.database.unit_instances[def_idx].used_counter_attack;
        let has_counter_sp = self.database.unit_instances[def_idx].has_condition("カウンター");
        // SRC COM.bas の判定順: 後/先/カウンターSP/先読み(Dice) → カウンター技能(回数消費)。
        // 先読み の Dice は技能保持時のみ評価 (`||` 短絡で RNG 列を保つ)。
        let preempt = if atk_weapon_class.contains('後')
            || cw.class.contains('先')
            || has_counter_sp
            || (foresight_lv > 0 && foresight_lv > (self.next_u32() % 16) as i32)
        {
            true
        } else if counter_lv > used {
            self.database.unit_instances[def_idx].used_counter_attack = used + 1;
            true
        } else {
            false
        };
        if !preempt {
            return false;
        }
        self.push_message(format!(
            "{} の先制攻撃！",
            self.database
                .pilot_by_name(&self.database.unit_instances[def_idx].pilot_name)
                .map(|p| p.nickname.clone())
                .unwrap_or_else(|| self.database.unit_instances[def_idx].pilot_name.clone())
        ));
        self.try_counterattack(def_pos.0, def_pos.1, atk_pos);
        true
    }

    /// `(dx, dy)` の防御側ユニットが `target` へ反撃可能なら 1 度だけ実行。
    /// 命中処理は通常攻撃と同じだが、反撃で defender を撃破した場合の経験値は
    /// 与えない（簡略化）。
    /// 戻り値: 反撃に使用した武器の `(名前, 1-based 番号)` / 反撃不能なら `None`。
    fn try_counterattack(
        &mut self,
        dx: u32,
        dy: u32,
        target: (u32, u32),
    ) -> Option<(String, usize)> {
        let def_idx = self
            .database
            .unit_instances
            .iter()
            .position(|u| u.x == dx && u.y == dy)?;
        let atk_idx = self
            .database
            .unit_instances
            .iter()
            .position(|u| u.x == target.0 && u.y == target.1)?;
        // 反撃が成立するのは敵対関係のときのみ (同盟同士は反撃しない)。
        if !self.database.unit_instances[def_idx]
            .party
            .is_hostile_to(self.database.unit_instances[atk_idx].party)
        {
            return None;
        }
        // 行動不能 (麻痺/混乱/睡眠/捕縛 等) のユニットは反撃できない。SRC
        // `MaxAction()==0` ゲート相当。手動/自動/ヘッドレス全経路で一元的に抑止する。
        if self.database.unit_instances[def_idx].attack_disabled() {
            return None;
        }
        // 反撃側 = defender、被弾側 = attacker。双方とも実効値込みのデータを使う。
        let (def_pilot, def_unit) = self.database.effective_combat_data(def_idx)?;
        let (atk_pilot, mut atk_unit) = self.database.effective_combat_data(atk_idx)?;
        let dist = combat::manhattan((dx, dy), target);
        // プレイヤーが反撃武器選択で武器を指定していれば優先 (1 回限り。射程内 + 発射可能
        // でなければ自動選定へフォールバック)。
        let forced = self.forced_counter_weapon.take().and_then(|idx| {
            def_unit
                .weapons
                .get(idx)
                .filter(|w| self.weapon_usable_vs(def_idx, w, dist, false))
                .cloned()
        });
        let weapon = match forced {
            Some(w) => w,
            None => self.best_firable_weapon_in_range(def_idx, &def_unit, dist)?,
        };
        // 反撃の被弾側 (= 元攻撃者 atk_idx) の防御特性 (弱点→装甲半減 / 吸収→無視) を適用。
        self.apply_defense_armor_mod(atk_idx, &weapon.class, &mut atk_unit);
        let t_id = self
            .database
            .map
            .as_ref()
            .map(|m| m.cell(target.0, target.1).terrain_id)?;
        let atk_hit_mod = self.database.terrain_hit_mod(t_id);
        let atk_damage_mod = self.database.terrain_damage_mod(t_id);
        // 反撃側 = defender、被弾側 = attacker。状態異常はそれぞれの
        // UnitInstance.statuses から取り出す。
        let counter_atk_morale = self.database.unit_instances[def_idx].morale;
        let counter_def_morale = self.database.unit_instances[atk_idx].morale;
        // 反撃側の状態。捨て身 は反撃時には無効 (§0.8 / SRC: 自分から攻撃したときのみ
        // ×3。反撃では適用しない) なので除外する。
        let mut counter_atk_statuses: Vec<String> = self.database.unit_instances[def_idx]
            .conditions
            .iter()
            .map(|c| c.name.clone())
            .filter(|n| n != "捨て身")
            .collect();
        push_combat_feature_statuses(
            &self.database.unit_instances[def_idx],
            &mut counter_atk_statuses,
        );
        let mut counter_def_statuses: Vec<String> = self.database.unit_instances[atk_idx]
            .conditions
            .iter()
            .map(|c| c.name.clone())
            .collect();
        push_combat_feature_statuses(
            &self.database.unit_instances[atk_idx],
            &mut counter_def_statuses,
        );
        // 反撃側 = defender (位置 dx,dy)、被弾側 = attacker (位置 target)。
        let counter_atk_env = self.terrain_env_at(dx, dy);
        let counter_def_env = self.terrain_env_at(target.0, target.1);
        // 反撃側 (= 攻撃側) / 被弾側 (= 防御側) の与・被ダメージ修正を sp.txt から解決。
        let dmg_levels = self
            .database
            .damage_spirit_levels(&counter_atk_statuses, &counter_def_statuses);
        let preview = combat::predict_with_status_terrain(
            &def_pilot,
            &def_unit,
            &weapon,
            &atk_pilot,
            &atk_unit,
            atk_hit_mod,
            atk_damage_mod,
            counter_atk_morale,
            counter_def_morale,
            &counter_atk_statuses,
            &counter_def_statuses,
            counter_atk_env,
            counter_def_env,
            dmg_levels,
            // 反撃の被弾側は atk_idx、反撃の攻撃側は def_idx。
            self.ecm_hit_mult(atk_idx, def_idx, &weapon.class),
        );
        // 散 属性武器の距離補正 (反撃側 dx,dy → 被弾側 target)。
        let counter_dist = combat::manhattan((dx, dy), target);
        let preview = preview.apply_scatter(&weapon.class, counter_dist);
        // 反撃武器の 1-based インデックスを取得
        let weapon_num = def_unit
            .weapons
            .iter()
            .position(|w| w.name == weapon.name)
            .map(|i| i + 1)
            .unwrap_or(1);
        let weapon_name = weapon.name.clone();
        // 反撃武器の EN・残弾消費 (撃破前=index 失効前に消費)。
        self.consume_weapon_resources(def_idx, weapon_num - 1);
        let roll = (self.next_u32() % 100) as i32;
        // 回避系特殊能力 (B): 反撃を受ける側 (atk_idx) が 分身/超回避 で完全回避し得る
        // (反撃の攻撃側は def_idx)。
        let hit =
            roll < preview.hit_chance && !self.check_dodge_feature(atk_idx, def_idx, &weapon.class);
        let msg = if hit {
            // 防御特性 (耐性÷2 / 無効化0 / 吸収=回復)。弱点の装甲半減は予測前に適用済み。
            let counter_dmg = self.defense_attribute_damage(atk_idx, &weapon.class, preview.damage);
            // みがわり (身代わり, 最優先・SRC Unit.cls:14855): 反撃を受ける側 (atk_idx) が
            // みがわり を持てば身代わりが肩代わりし、被ダメージは 0 になる。
            let counter_dmg = if let Some(sub) =
                self.apply_migawari(atk_idx, def_idx, &weapon.class, counter_dmg)
            {
                self.push_message(format!(
                    "みがわり: {} が {} の身代わりになった！",
                    sub, atk_unit.name
                ));
                0
            } else {
                // バリア/アクティブフィールド/シールド吸収 → 不死身キャップ
                // (反撃の攻撃側は def_idx)。
                let d = self.apply_barrier_absorb(atk_idx, def_idx, &weapon.class, counter_dmg);
                let d = self.apply_active_field_absorb(atk_idx, def_idx, &weapon.class, d);
                let d = self.apply_shield_defense(atk_idx, &weapon.class, d);
                self.cap_damage_for_immortal(atk_idx, atk_unit.hp, d)
            };
            let atk_old_dmg = self.database.unit_instances[atk_idx].damage;
            self.database.unit_instances[atk_idx].damage += counter_dmg;
            let remaining = atk_unit.hp - self.database.unit_instances[atk_idx].damage;
            if remaining <= 0 && self.revive_if_possible(atk_idx) {
                format!(
                    "  ↩ 反撃: {} → {} [{}]: 撃破… だが【復活】！",
                    def_pilot.nickname, atk_pilot.nickname, weapon.name
                )
            } else if remaining <= 0 {
                // 反撃側 (def, 位置 dx,dy) が撃破 → 報酬は反撃側へ。
                let doubled = self.database.unit_instances[def_idx].has_condition("努力");
                let exp = atk_pilot.exp_value * if doubled { 2 } else { 1 };
                let victim_value = atk_unit.value;
                let victim_party = self.database.unit_instances[atk_idx].party;
                let mut m = format!(
                    "  ↩ 反撃: {} → {} [{}]: 撃破！",
                    def_pilot.nickname, atk_pilot.nickname, weapon.name
                );
                self.database.remove_unit_at(atk_idx);
                self.fire_destruction_label(&atk_pilot.name, &atk_unit.name, victim_party);
                if let Some(killer_idx) = self
                    .database
                    .unit_instances
                    .iter()
                    .position(|u| u.x == dx && u.y == dy)
                {
                    let money =
                        self.award_kill_rewards(killer_idx, exp, victim_value, victim_party);
                    if money > 0 {
                        m.push_str(&format!(" 資金 +{money}"));
                    }
                }
                m
            } else {
                // 損傷率イベント (反撃で HP が閾値を割った場合)。
                self.fire_damage_threshold_for_unit(atk_idx, atk_unit.hp, atk_old_dmg);
                // 反撃を受けて生存した攻撃側は気力 +1 (SRC: 損傷による気力上昇)。
                {
                    let am = &mut self.database.unit_instances[atk_idx];
                    am.morale = (am.morale + 1).clamp(0, 150);
                }
                // 命中時/損傷時 気力増加スキル (反撃側=def が命中、被弾側=atk が損傷)。
                self.apply_combat_morale_skills(def_idx, atk_idx, true);
                // 生存: 反撃武器の特殊効果攻撃属性 (状態異常付与) を被弾側へ proc。
                // 反撃側 = def (atk_pilot=def_pilot)、被弾側 = atk (def_pilot=atk_pilot)。
                let inflicted = self.apply_weapon_special_effects(
                    atk_idx, def_idx, &weapon, &def_pilot, &atk_pilot,
                );
                let mut m = format!(
                    "  ↩ 反撃: {} → {} [{}]: 命中 {} ダメージ (残HP {})",
                    def_pilot.nickname, atk_pilot.nickname, weapon.name, counter_dmg, remaining
                );
                if !inflicted.is_empty() {
                    m.push_str(&format!(" → {}", inflicted.join("・")));
                }
                m
            }
        } else {
            // 失敗時/回避時 気力増加スキル (反撃側=def が失敗、被弾側=atk が回避)。
            self.apply_combat_morale_skills(def_idx, atk_idx, false);
            format!(
                "  ↩ 反撃: {} → {} [{}]: ミス",
                def_pilot.nickname, atk_pilot.nickname, weapon.name
            )
        };
        self.push_message(msg);
        Some((weapon_name, weapon_num))
    }

    fn end_phase(&mut self) -> bool {
        if self.scene != Scene::MapView {
            return false;
        }
        if self.stage_state != crate::stage::StageState::Battle {
            return false; // ブリーフィング中などはフェーズを進めない
        }
        // 逐次演出モード: 敵フェイズを開始してランナーを初期化し、以降は `tick` が
        // 1 体ずつ駆動する (SRC.cs `StartTurn` の CPU ループ相当)。
        if self.animate_ai {
            self.begin_ai_phase(crate::Phase::Enemy);
            return true;
        }
        // 同期モード (ヘッドレス/テスト): 元 SRC `StartTurn("味方")` の do/loop に対応。
        //   Player の "ターン終了" 後、敵 → 中立 → ＮＰＣ を順に処理し
        //   それぞれ AI を回したら、ターン数を +1 して Player に戻す。
        // 無限ループ防止のため最大 4 フェーズ進める。
        for _ in 0..4 {
            let next = self.turn.phase.next();
            // ＮＰＣ → Player でラップするときに turn 数を進める
            if next == crate::Phase::Player {
                self.turn.number = self.turn.number.saturating_add(1);
            }
            self.begin_phase(next);
            if self.turn.phase == crate::Phase::Player {
                break;
            }
            // 元 SRC `CPUOperation(uparty)` 相当
            self.run_ai_phase();
            self.check_victory();
            if matches!(
                self.stage_state,
                crate::stage::StageState::Victory | crate::stage::StageState::Defeat
            ) {
                return true;
            }
        }
        true
    }

    /// 逐次演出モードで `phase`(敵/中立/ＮＰＣ) を開始し、`ai_runner` の行動キューを
    /// 構築する。`begin_phase` がターンイベント(Talk 等)を立てた場合は modal で
    /// ブロックされ、`tick` 側がそれを待ってからステップする。
    fn begin_ai_phase(&mut self, phase: crate::Phase) {
        // 勝敗確定後は新しいフェイズを開始しない (敗北後に中立/敵フェイズが
        // 走り続けるのを防ぐ)。
        if matches!(
            self.stage_state,
            crate::stage::StageState::Victory | crate::stage::StageState::Defeat
        ) {
            self.ai_runner = None;
            return;
        }
        self.begin_phase(phase);
        let party = phase.party();
        let queue: std::collections::VecDeque<String> = self
            .database
            .unit_instances
            .iter()
            .filter(|u| u.party == party && !u.off_map)
            .map(|u| u.uid.clone())
            .collect();
        self.ai_runner = Some(AiRunner {
            queue,
            step_timer: AI_STEP_SECS,
        });
    }

    /// 逐次 AI ランナーを `dt` 秒分進める。`tick` から呼ばれる。返り値は再描画要否。
    /// modal(ダイアログ/反撃プロンプト)・スクリプト Wait 中は進めない。
    fn ai_runner_tick(&mut self, dt: f64) -> bool {
        if self.ai_runner.is_none() {
            return false;
        }
        // 勝敗が確定したら AI / フェイズ進行を即停止する。Defeat/Victory は
        // ユニット撃破 (check_victory) だけでなく、シナリオの `全滅 味方` →
        // `GameOver` イベント (戦闘演出後に EventQue 経由で発火) など別経路でも
        // 立つため、各 tick 冒頭で必ず確認する。これをしないと味方全滅後も
        // 中立/敵フェイズが動き続け「敗北したのに進行する」状態になる。
        if matches!(
            self.stage_state,
            crate::stage::StageState::Victory | crate::stage::StageState::Defeat
        ) {
            self.ai_runner = None;
            return false;
        }
        // modal (ターンイベントの Talk / 反撃プロンプト) や Wait タイマ中は待機。
        if self.pending_dialog.is_some() || self.pending_timer.is_some() {
            return false;
        }
        // 移動/戦闘演出の再生中は次の 1 体を動かさず、演出を見せきってから進める。
        if self.battle_anim.is_some() || self.move_anim.is_some() {
            return false;
        }
        // 「間」のカウントダウン。
        {
            let runner = self.ai_runner.as_mut().unwrap();
            runner.step_timer -= dt;
            if runner.step_timer > 0.0 {
                return false;
            }
            runner.step_timer = AI_STEP_SECS;
        }
        // 次に行動する有効なユニットを探す (撃破/退避/行動済みはスキップ)。
        let phase_party = self.turn.phase.party();
        let mut act_uid: Option<String> = None;
        while let Some(uid) = self.ai_runner.as_mut().unwrap().queue.pop_front() {
            if let Some(u) = self.database.unit_by_uid(&uid) {
                if !u.off_map && u.party == phase_party && !u.has_acted {
                    act_uid = Some(uid);
                    break;
                }
            }
        }
        if let Some(uid) = act_uid {
            // カメラを行動ユニットへ寄せて 1 体実行 (移動→攻撃)。
            if let Some(u) = self.database.unit_by_uid(&uid) {
                self.map_cursor = Some((u.x, u.y));
                self.ensure_cursor_visible();
            }
            if let Some(idx) = self.database.idx_by_uid(&uid) {
                self.ai_act_unit(idx);
            }
            self.check_victory();
            if matches!(
                self.stage_state,
                crate::stage::StageState::Victory | crate::stage::StageState::Defeat
            ) {
                self.ai_runner = None;
            }
            return true;
        }
        // このフェイズのキューが空 → 次フェイズへ。ＮＰＣ→Player でラップ時 turn++。
        let next = self.turn.phase.next();
        if next == crate::Phase::Player {
            self.turn.number = self.turn.number.saturating_add(1);
            self.begin_phase(crate::Phase::Player);
            self.ai_runner = None; // プレイヤーのターンへ。
        } else {
            self.begin_ai_phase(next);
        }
        true
    }

    /// 現フェーズの所属ユニットを順に動かす AI:
    ///
    /// 各ユニットについて
    /// 1. ターゲット候補: 対立勢力ユニットを「HP 残量 (低い方優先)」「マンハッタン
    ///    距離 (近い方優先)」「経験値 (高い方優先)」の複合スコアでランク付け。
    /// 2. movement::compute_range_with でユニットの移動範囲を Dijkstra で計算。
    /// 3. ベストターゲットへ「攻撃可能な距離 = 任意武器の最大射程」を満たす
    ///    位置を移動範囲から選び、最短経路の一手を実際の位置として採用。
    /// 4. 移動後、AttackTarget を呼んで攻撃。
    fn run_ai_phase(&mut self) {
        let party = self.turn.phase.party();
        let positions: Vec<(u32, u32)> = self
            .database
            .unit_instances
            .iter()
            .filter(|u| u.party == party && !u.off_map)
            .map(|u| (u.x, u.y))
            .collect();
        let map_dims = match self.database.map.as_ref() {
            Some(m) => Some((m.width, m.height)),
            None => return,
        };
        let _ = map_dims;

        for (sx, sy) in positions {
            // 最新のインデックスを引き直す（前手の攻撃で削除されている可能性）
            let idx = self
                .database
                .unit_instances
                .iter()
                .position(|u| u.x == sx && u.y == sy && u.party == party);
            let Some(idx) = idx else {
                continue;
            };

            self.ai_act_unit(idx);
        }
    }

    /// 単一ユニットの AI 行動 1 手。run_ai_phase の内部ループから呼ばれる。
    fn ai_act_unit(&mut self, idx: usize) {
        // 行動不能 (麻痺/睡眠/混乱/捕縛/凍結/石化 等、AttackDisabled を持つ状態異常) の
        // ユニットは行動せずフェイズを消費する。特殊効果攻撃属性の状態異常が AI に効く。
        if self.database.unit_instances[idx].attack_disabled() {
            self.database.unit_instances[idx].has_acted = true;
            return;
        }
        let party = self.database.unit_instances[idx].party;
        let unit_pos = (
            self.database.unit_instances[idx].x,
            self.database.unit_instances[idx].y,
        );
        let unit_data_name = self.database.unit_instances[idx].unit_data_name.clone();
        // SRC `ChangeMode` で設定された思考モードを反映。
        // `待機` / `固定` のユニットは敵が射程内に来るまで動かない。
        // 本実装はターン中に再評価される最小ロジック:
        //   - `固定` → 完全静止 (移動なし、攻撃なし)
        //   - `待機` → 移動なし、ただし攻撃可能ターゲットがいれば攻撃
        let ai_mode = self.database.unit_instances[idx].ai_mode.clone();
        let mode_keeps_position = matches!(ai_mode.as_str(), "待機" | "固定");
        let mode_no_attack = ai_mode == "固定";
        if mode_no_attack {
            self.database.unit_instances[idx].has_acted = true;
            return;
        }
        // 支援精神 (回復系) を持つ AI は、半分以下まで負傷した味方が居れば回復する。
        // 精神は行動を消費しないため、この後も通常の移動・攻撃を続ける。
        self.ai_use_support_spirit(idx);
        // 瀕死の自ユニットは撃破に備えて「復活」精神を事前発動する (free action)。
        self.ai_use_revive_spirit(idx);
        // 回復/補給アビリティを持つ AI は、射程内に支援を要する味方が居れば優先する。
        if self.ai_use_support_ability(idx) {
            return;
        }
        // 敵対象アビリティ (脱=気力減少 等) を持つ AI は、射程内に敵が居れば使う。
        if self.ai_use_offensive_ability(idx) {
            return;
        }
        // 召喚アビリティを持つ AI は、数的に劣勢なら増援を召喚する。
        if self.ai_use_summon_ability(idx) {
            return;
        }
        // マップ兵器を持つ AI は、2 体以上の敵を巻き込める照準があれば優先発射する。
        if self.ai_use_map_weapon(idx) {
            return;
        }
        // 自ユニットの最大射程 (= 攻撃移動目標距離)
        let max_range = self
            .database
            .unit_by_name(&unit_data_name)
            .map(|u| u.weapons.iter().map(|w| w.max_range).max().unwrap_or(1))
            .unwrap_or(1) as u32;
        let speed = self
            .database
            .unit_by_name(&unit_data_name)
            .map(|u| u.speed)
            .unwrap_or(3);

        let unit_data = self.database.unit_by_name(&unit_data_name).unwrap();
        // 攻撃側の実効戦闘データ (育成 / 強化パーツ / 状態異常 込み) を一度だけ算出。
        let atk_combat = self.database.effective_combat_data(idx);

        // ターゲット候補をスコア付けして並べ替え
        let mut candidates: Vec<((u32, u32), i64, i64)> = self
            .database
            .unit_instances
            .iter()
            .filter(|u| u.party.is_hostile_to(party) && !u.off_map)
            .map(|u| {
                let dist = combat::manhattan(unit_pos, (u.x, u.y));
                // 残 HP は強化パーツ込みの実効最大 HP を基準にする。
                let hp_remaining = self.database.effective_max_hp(u) - u.damage;
                let exp_value = self
                    .database
                    .pilot_by_name(&u.pilot_name)
                    .map(|p| i64::from(p.exp_value))
                    .unwrap_or(0);

                // Use combat prediction to estimate damage against this target
                let best_weapon = combat::best_weapon_in_range(unit_data, dist);

                // 攻守とも実効値込みデータで与ダメージを推定する (静的データではなく
                // 育成 / 強化パーツ / 状態異常を反映)。攻撃側データが無い場合は 0。
                let estimated_damage = match (best_weapon, &atk_combat) {
                    (Some(weapon), Some((ap, au))) => self
                        .database
                        .idx_by_uid(&u.uid)
                        .and_then(|i| self.database.effective_combat_data(i))
                        .map(|(dp, du)| {
                            crate::combat::predict(ap, au, weapon, &dp, &du, 0, 0).damage
                        })
                        .unwrap_or(0),
                    _ => 0,
                };

                // Score: prefer targets that can be killed (damage >= hp_remaining)
                // Then prefer closer targets, then higher exp value
                let can_kill = estimated_damage >= hp_remaining;
                let score = if can_kill {
                    // High priority for killable targets (negative score = sorted first)
                    -10000 + i64::from(dist)
                } else {
                    // Otherwise: prefer closer targets with less HP and higher exp
                    i64::from(dist) * 10 + hp_remaining.max(0) / 100 - exp_value / 100
                };
                ((u.x, u.y), score, hp_remaining)
            })
            .collect();
        // 撃破可能 (= 残 HP が小さい) ターゲットを優先するため score 昇順。
        candidates.sort_by_key(|(_, score, _)| *score);
        let Some(&(target_pos, _, _)) = candidates.first() else {
            return;
        };
        let mut target_pos = target_pos;
        // ChangeMode「護衛 <対象>」: 指定ユニットの近くへ移動して守る (移動目標を
        // 守護対象に置換)。攻撃自体は移動後に射程内の敵を狙う (attack_target)。
        if let Some(name) = ai_mode
            .strip_prefix("護衛")
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
        {
            if let Some(gpos) = self
                .database
                .unit_instances
                .iter()
                .find(|u| {
                    !u.off_map
                        && (u.unit_data_name == name || u.pilot_name == name || u.uid == name)
                })
                .map(|u| (u.x, u.y))
            {
                target_pos = gpos;
            }
        }
        // ChangeMode による思考誘導 (決戦！宇宙怪獣2話 の NPC 制御):
        //  - `ChangeMode <unit> <x> <y>` (座標モード): 指定座標へ向かう (ボスを釣る)。
        //  - `ChangeMode <unit> <対象ユニット名>` (ユニット狙い): 指定ユニットを
        //    優先ターゲットにする (NPC をボスへ集中させる)。
        // (`通常`/`待機`/`固定`/`逃亡`/`護衛 …` は別途処理済み。)
        {
            let mode = ai_mode.trim();
            let toks: Vec<&str> = mode.split_whitespace().collect();
            let coord = match toks.as_slice() {
                [a, b] => match (a.parse::<u32>(), b.parse::<u32>()) {
                    (Ok(x), Ok(y)) => Some((x, y)),
                    _ => None,
                },
                _ => None,
            };
            if let Some(c) = coord {
                target_pos = c;
            } else if !mode.is_empty()
                && !matches!(mode, "通常" | "待機" | "固定" | "逃亡")
                && !mode.starts_with("護衛")
            {
                // ユニット狙い: 名前一致する敵対ユニットを優先ターゲットに。
                if let Some(tpos) = self
                    .database
                    .unit_instances
                    .iter()
                    .find(|u| {
                        !u.off_map
                            && u.party.is_hostile_to(party)
                            && (u.unit_data_name == mode || u.pilot_name == mode || u.uid == mode)
                    })
                    .map(|u| (u.x, u.y))
                {
                    target_pos = tpos;
                }
            }
        }

        // 移動範囲を Dijkstra で計算 (地形適応・特殊能力を考慮)。
        let move_cost_fn = {
            let terrain_table = self.database.terrains.clone();
            let transportation = unit_data.transportation.clone();
            let adaption = unit_data.adaption.0;
            let current_area = self.database.unit_instances[idx].current_area.clone();
            let active_feature_names: Vec<String> = self.database.unit_instances[idx]
                .active_features
                .iter()
                .map(|f| f.name.clone())
                .collect();
            let terrain_adapt_names: Vec<String> = self.database.unit_instances[idx]
                .active_features
                .iter()
                .filter(|f| f.is_active && f.name == "地形適応")
                .flat_map(|f| {
                    // value = "別名 地形名称1 地形名称2..." (first token is alias)
                    f.value
                        .split_whitespace()
                        .skip(1)
                        .map(|s| s.to_string())
                        .collect::<Vec<_>>()
                })
                .collect();
            crate::movement::make_unit_cost_fn(
                terrain_table,
                transportation,
                adaption,
                current_area,
                active_feature_names,
                terrain_adapt_names,
            )
        };
        let reachable = match self.database.map.as_ref() {
            Some(m) => crate::movement::compute_range_with(m, unit_pos, speed, move_cost_fn),
            None => std::collections::HashMap::new(),
        };

        // 恐怖 (特殊効果攻撃属性 恐) / ChangeMode「逃亡」: 敵から逃げ続ける。
        // 到達マスのうち敵への最小距離が最大のマスへ移動し、攻撃はしない。
        if self.database.unit_instances[idx].has_condition("恐怖") || ai_mode == "逃亡" {
            let enemies: Vec<(u32, u32)> = candidates.iter().map(|(p, _, _)| *p).collect();
            let occ: std::collections::HashSet<(u32, u32)> = self
                .database
                .unit_instances
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != idx)
                .map(|(_, u)| (u.x, u.y))
                .collect();
            let flee = reachable
                .iter()
                .filter(|((x, y), _)| !occ.contains(&(*x, *y)))
                .max_by_key(|((x, y), _)| {
                    enemies
                        .iter()
                        .map(|e| combat::manhattan((*x, *y), *e))
                        .min()
                        .unwrap_or(0)
                })
                .map(|((x, y), _)| (*x, *y));
            if let Some(dest) = flee {
                if dest != unit_pos {
                    let uid = self.database.unit_instances[idx].uid.clone();
                    self.begin_move_anim(&uid, &reachable, unit_pos, dest);
                    self.database.move_unit(&uid, dest.0, dest.1);
                }
            }
            self.database.unit_instances[idx].has_acted = true;
            return;
        }

        // ターゲットを射程内に収める最良の到達マス: ターゲット直近 ≤ max_range
        // のマスのうち、占有されていない / 自分の現在位置 OK を選ぶ。
        let occupied: std::collections::HashSet<(u32, u32)> = self
            .database
            .unit_instances
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != idx)
            .map(|(_, u)| (u.x, u.y))
            .collect();
        // マップ兵器回避: 敵対勢力がマップ兵器を持つとき、同陣営ユニットとの密集を避けて
        // 攻撃マスを選ぶ (1 発で複数巻き込まれないよう散開する)。脅威が無ければ密集度は
        // 常に 0 で従来挙動と同じ (= 既存テストへ影響なし)。
        let map_threat = self.hostile_has_map_weapon(party);
        let ally_positions: Vec<(u32, u32)> = if map_threat {
            self.database
                .unit_instances
                .iter()
                .enumerate()
                .filter(|(i, u)| *i != idx && !u.off_map && u.party == party)
                .map(|(_, u)| (u.x, u.y))
                .collect()
        } else {
            Vec::new()
        };
        let cluster_at = |x: u32, y: u32| -> i64 {
            ally_positions
                .iter()
                .filter(|(ax, ay)| combat::manhattan((x, y), (*ax, *ay)) <= 2)
                .count() as i64
        };
        let best_move = reachable
            .iter()
            .filter(|((x, y), _)| !occupied.contains(&(*x, *y)))
            .filter(|((x, y), _)| combat::manhattan((*x, *y), target_pos) <= max_range)
            // 攻撃可能 (射程内) なマスのうち、防御地形 (森/山/都市など) を選好し、次に
            // マップ兵器脅威下では密集を避ける。同条件ならターゲットに最も近い / 残 MP 最大を選好。
            .min_by_key(|((x, y), rem)| {
                (
                    -self.tile_defensive_value(*x, *y), // 防御価値が高いほど優先 (負で最小化)
                    cluster_at(*x, *y), // マップ兵器脅威下は密集を回避 (脅威無し=常に0)
                    combat::manhattan((*x, *y), target_pos),
                    -*rem, // BinaryHeap と同じ符号反転で大きい方を最小に
                )
            })
            .map(|((x, y), _)| (*x, *y));
        if mode_keeps_position {
            // `待機` モード: 移動せず、攻撃のみ試行 (現在位置から射程内に
            // 敵がいる場合のみ attack_target で命中する)。
        } else if let Some(dest) = best_move {
            if dest != unit_pos {
                let uid = self.database.unit_instances[idx].uid.clone();
                self.database.move_unit(&uid, dest.0, dest.1);
            }
        } else {
            // 射程に届く到達マスが無い: ターゲットに最も近い到達マスへ移動。
            let approach = reachable
                .iter()
                .filter(|((x, y), _)| !occupied.contains(&(*x, *y)))
                .min_by_key(|((x, y), _)| combat::manhattan((*x, *y), target_pos))
                .map(|((x, y), _)| (*x, *y));
            if let Some(dest) = approach {
                if dest != unit_pos {
                    let uid = self.database.unit_instances[idx].uid.clone();
                    self.begin_move_anim(&uid, &reachable, unit_pos, dest);
                    self.database.move_unit(&uid, dest.0, dest.1);
                }
            }
        }

        // 攻撃試行 (attack_target はカーソル位置のユニットを使う)
        let new_pos = (
            self.database.unit_instances[idx].x,
            self.database.unit_instances[idx].y,
        );
        self.map_cursor = Some(new_pos);
        // 敵 AI: 射程内に攻撃対象が居れば、攻撃前に攻撃補助の精神コマンドを使う。
        if self.ai_nearest_target_tile(idx, new_pos).is_some() {
            self.ai_use_offensive_spirit(idx);
        }
        // 逐次演出かつ手動反撃モードのとき、攻撃対象が味方なら反撃手段の選択を出して
        // 中断する (応答後に attack を解決)。対象が味方以外/自動反撃モードなら自動解決。
        if self.animate_ai && !self.auto_counter {
            if let Some((def_uid, tile)) = self.ai_nearest_target_tile(idx, new_pos) {
                if self.reaction_applies(&def_uid) {
                    let atk_uid = self.database.unit_instances[idx].uid.clone();
                    self.begin_reaction_prompt(atk_uid, def_uid, tile);
                    return;
                }
            }
        }
        let _ = self.attack_target();
    }

    /// 敵 AI のマップ兵器使用。`Ｍ` 属性の武器を持ち、現在位置からの照準で 2 体以上の
    /// 敵対ユニットを巻き込める場合、最も多く巻き込む座標へマップ攻撃を発射する。発射したら
    /// `true`。マップ兵器は移動前提でないため現在位置から評価する。テスト用ユニットは
    /// マップ武器を持たないので無効 (= 既存テストへ影響なし)。
    fn ai_use_map_weapon(&mut self, idx: usize) -> bool {
        let pos = (
            self.database.unit_instances[idx].x,
            self.database.unit_instances[idx].y,
        );
        let party = self.database.unit_instances[idx].party;
        let unit_name = self.database.unit_instances[idx].unit_data_name.clone();
        let caster = self.database.unit_instances[idx].uid.clone();
        // マップ兵器 (class に全角 Ｍ) の WeaponData を収集。
        let map_weapons: Vec<crate::data::unit::WeaponData> =
            match self.database.unit_by_name(&unit_name) {
                Some(d) => d
                    .weapons
                    .iter()
                    .filter(|w| w.class.contains('Ｍ'))
                    .cloned()
                    .collect(),
                None => return false,
            };
        if map_weapons.is_empty() {
            return false;
        }
        let enemies: Vec<(u32, u32)> = self
            .database
            .unit_instances
            .iter()
            .filter(|u| !u.off_map && u.party.is_hostile_to(party))
            .map(|u| (u.x, u.y))
            .collect();
        if enemies.len() < 2 {
            return false;
        }
        for weapon in &map_weapons {
            // 各敵位置を照準候補に、最も多く敵を巻き込む中心を探す。
            let mut best: Option<((u32, u32), usize)> = None;
            for &center in &enemies {
                let area = crate::event_runtime::map_attack_area(weapon, pos, center);
                let hits = enemies.iter().filter(|e| area.contains(e)).count();
                if hits >= 2 && best.as_ref().map(|(_, h)| hits > *h).unwrap_or(true) {
                    best = Some((center, hits));
                }
            }
            if let Some((center, _)) = best {
                // AI 発のマップ攻撃コマンド = 通常戦闘扱い (is_event=false)。
                // SRC `Unit.MapAttack` 同様、ダメージ適用前に 使用/攻撃 イベントを発火。
                crate::event_runtime::map_attack(
                    self,
                    Some(&caster),
                    &weapon.name,
                    center.0,
                    center.1,
                    false,
                );
                if let Some(i) = self.database.idx_by_uid(&caster) {
                    self.database.unit_instances[i].has_acted = true;
                }
                return true;
            }
        }
        false
    }

    /// 敵 AI の支援アビリティ使用。回復 (HP、効果に `回復`、霊力/ＳＰ は除く) または
    /// 補給 (EN、効果に `補給`) を提供するアビリティを持ち、射程内にその支援を必要とする
    /// 味方が居れば、最も必要度の高い味方へ発動する。発動したら `true`。回復対象は最大
    /// ダメージ、補給対象は最大 EN 消費で選ぶ。テスト用ユニットはアビリティ無しなので
    /// 無効 (= 既存テストへ影響なし)。
    fn ai_use_support_ability(&mut self, idx: usize) -> bool {
        let caster = self.database.unit_instances[idx].uid.clone();
        let unit_name = self.database.unit_instances[idx].unit_data_name.clone();
        // (アビリティ index, 回復するか, 補給するか) を収集。
        let support_idxs: Vec<(usize, bool, bool)> = match self.database.unit_by_name(&unit_name) {
            Some(d) => d
                .abilities
                .iter()
                .enumerate()
                .filter_map(|(i, a)| {
                    let heals = a.effect.contains("回復")
                        && !a.effect.contains("霊力")
                        && !a.effect.contains("ＳＰ");
                    let supplies = a.effect.contains("補給");
                    (heals || supplies).then_some((i, heals, supplies))
                })
                .collect(),
            None => return false,
        };
        for (ab_idx, heals, supplies) in support_idxs {
            if !self.ability_usable(&caster, ab_idx) {
                continue;
            }
            // 射程内で最も支援を必要とする味方を対象にする
            // (回復=最大ダメージ / 補給=最大 EN 消費)。
            let target_uids: Vec<String> = self
                .database
                .unit_instances
                .iter()
                .filter(|u| {
                    !u.off_map && ((heals && u.damage > 0) || (supplies && u.en_consumed > 0))
                })
                .map(|u| u.uid.clone())
                .collect();
            let mut best: Option<(String, i64)> = None;
            for tuid in target_uids {
                if !self.ability_target_valid(&caster, ab_idx, &tuid) {
                    continue;
                }
                let need = self
                    .database
                    .unit_by_uid(&tuid)
                    .map(|u| {
                        let h = if heals { u.damage } else { 0 };
                        let s = if supplies {
                            i64::from(u.en_consumed)
                        } else {
                            0
                        };
                        h.max(s)
                    })
                    .unwrap_or(0);
                if best.as_ref().map(|(_, d)| need > *d).unwrap_or(true) {
                    best = Some((tuid, need));
                }
            }
            if let Some((tuid, _)) = best {
                self.apply_ability(&caster, ab_idx, &tuid);
                if let Some(i) = self.database.idx_by_uid(&caster) {
                    if self.database.unit_instances[i].has_acted {
                        crate::event_runtime::fire_action_end_labels(self, i);
                    }
                }
                return true;
            }
        }
        false
    }

    /// 敵 AI の攻撃アビリティ使用。敵対勢力を対象とするアビリティ (`脱`=気力減少 /
    /// `除`=状態解除 等の属性) を持ち、射程内に有効な敵が居れば、最も気力の高い敵へ
    /// 発動する (`脱` で気力を削ぐ価値が高い相手を優先)。発動したら `true`。テスト用
    /// ユニットはアビリティ無しなので無効 (= 既存テストへ影響なし)。
    fn ai_use_offensive_ability(&mut self, idx: usize) -> bool {
        let caster = self.database.unit_instances[idx].uid.clone();
        let unit_name = self.database.unit_instances[idx].unit_data_name.clone();
        let atk_idxs: Vec<usize> = match self.database.unit_by_name(&unit_name) {
            Some(d) => d
                .abilities
                .iter()
                .enumerate()
                .filter(|(_, a)| a.targets_enemy())
                .map(|(i, _)| i)
                .collect(),
            None => return false,
        };
        for ab_idx in atk_idxs {
            if !self.ability_usable(&caster, ab_idx) {
                continue;
            }
            // 射程内で最も気力の高い敵を対象にする。
            let target_uids: Vec<String> = self
                .database
                .unit_instances
                .iter()
                .filter(|u| !u.off_map)
                .map(|u| u.uid.clone())
                .collect();
            let mut best: Option<(String, i32)> = None;
            for tuid in target_uids {
                if !self.ability_target_valid(&caster, ab_idx, &tuid) {
                    continue;
                }
                let morale = self
                    .database
                    .unit_by_uid(&tuid)
                    .map(|u| u.morale)
                    .unwrap_or(0);
                if best.as_ref().map(|(_, m)| morale > *m).unwrap_or(true) {
                    best = Some((tuid, morale));
                }
            }
            if let Some((tuid, _)) = best {
                self.apply_ability(&caster, ab_idx, &tuid);
                if let Some(i) = self.database.idx_by_uid(&caster) {
                    if self.database.unit_instances[i].has_acted {
                        crate::event_runtime::fire_action_end_labels(self, i);
                    }
                }
                return true;
            }
        }
        false
    }

    /// 敵 AI の召喚アビリティ使用。`召喚` 効果のアビリティを持ち、自陣営がオンマップ数で
    /// 敵対勢力に劣勢 (味方数 ≤ 敵数) のとき、増援を召喚する。発動したら `true`。劣勢
    /// ゲートで数的に拮抗すれば止まるため乱発しない (召喚アビリティは多くが回数制限付き
    /// でさらに有限)。テスト用ユニットはアビリティ無しなので無効 (= 既存テストへ影響なし)。
    fn ai_use_summon_ability(&mut self, idx: usize) -> bool {
        let caster = self.database.unit_instances[idx].uid.clone();
        let party = self.database.unit_instances[idx].party;
        let unit_name = self.database.unit_instances[idx].unit_data_name.clone();
        let summon_idxs: Vec<usize> = match self.database.unit_by_name(&unit_name) {
            Some(d) => d
                .abilities
                .iter()
                .enumerate()
                .filter(|(_, a)| a.effect.contains("召喚"))
                .map(|(i, _)| i)
                .collect(),
            None => return false,
        };
        if summon_idxs.is_empty() {
            return false;
        }
        // 劣勢判定: オンマップの自陣営数 ≤ 敵対勢力数 のときのみ召喚する。
        let allies = self
            .database
            .unit_instances
            .iter()
            .filter(|u| !u.off_map && u.party == party)
            .count();
        let enemies = self
            .database
            .unit_instances
            .iter()
            .filter(|u| !u.off_map && u.party.is_hostile_to(party))
            .count();
        if allies > enemies {
            return false;
        }
        for ab_idx in summon_idxs {
            if !self.ability_usable(&caster, ab_idx) {
                continue;
            }
            // 召喚は発動者自身 (隣接空きマスに生成) を対象にする。
            if !self.ability_target_valid(&caster, ab_idx, &caster) {
                continue;
            }
            self.apply_ability(&caster, ab_idx, &caster);
            if let Some(i) = self.database.idx_by_uid(&caster) {
                if self.database.unit_instances[i].has_acted {
                    crate::event_runtime::fire_action_end_labels(self, i);
                }
            }
            return true;
        }
        false
    }

    /// 敵 AI の攻撃補助精神コマンド使用。パイロットが習得済み (SP/気力/レベル充足) の
    /// 攻撃補助系精神 (魂 > 熱血 > 必中 の優先順) を 1 つ発動する。既に攻撃補助状態を
    /// 持つ場合や該当精神を習得していない場合は何もしない。テスト用ユニットは
    /// `spirit_commands` が空なので無効 (= 既存テストへ影響なし)。
    fn ai_use_offensive_spirit(&mut self, idx: usize) {
        if self.database.unit_instances[idx].has_condition("魂")
            || self.database.unit_instances[idx].has_condition("熱血")
            || self.database.unit_instances[idx].has_condition("必中")
        {
            return;
        }
        let uid = self.database.unit_instances[idx].uid.clone();
        let opts = self.spirit_command_options(&uid);
        if opts.is_empty() {
            return;
        }
        for name in ["魂", "熱血", "必中"] {
            if let Some((_, cost)) = opts.iter().find(|(n, _)| n == name) {
                // 気力条件 (魂=120 / 熱血=80 等) を満たすこと。
                if !self.database.unit_instances[idx].morale_sufficient_for_power(name) {
                    continue;
                }
                let cost = *cost;
                self.consume_unit_sp(&uid, cost);
                self.database.unit_instances[idx].add_condition(crate::Condition::new(name, 1));
                let nick = self.database.unit_instances[idx].pilot_name.clone();
                self.push_message(format!("{nick} は精神コマンド【{name}】を使用！"));
                return;
            }
        }
    }

    /// 敵 AI の支援精神コマンド使用。HP 回復系精神 (信頼/友情/愛/根性/ド根性) を習得し
    /// (SP/レベル充足)、最大 HP の半分以下まで負傷した同陣営ユニットが居れば 1 つ発動して
    /// 回復する。精神は行動を消費しないため発動後も移動・攻撃を続ける。発動したら `true`。
    /// 安価な単体 (信頼) → 全体 (友情/愛) → 自分のみ (根性/ド根性) の優先順で試す。テスト用
    /// ユニットは `spirit_commands` が空なので無効 (= 既存テストへ影響なし)。
    fn ai_use_support_spirit(&mut self, idx: usize) -> bool {
        let uid = self.database.unit_instances[idx].uid.clone();
        let party = self.database.unit_instances[idx].party;
        let opts = self.spirit_command_options(&uid);
        if opts.is_empty() {
            return false;
        }
        // 最も負傷した同陣営ユニット (自分含む) を探す。半分以下まで減った味方が
        // 居なければ SP を温存する。
        let mut worst: Option<(String, i64)> = None;
        let mut any_below_half = false;
        for u in self.database.unit_instances.iter() {
            if u.off_map || u.party != party || u.damage <= 0 {
                continue;
            }
            let max_hp = self.database.effective_max_hp(u);
            if u.damage * 2 >= max_hp {
                any_below_half = true;
            }
            if worst.as_ref().map(|(_, d)| u.damage > *d).unwrap_or(true) {
                worst = Some((u.uid.clone(), u.damage));
            }
        }
        if !any_below_half {
            return false;
        }
        let Some((worst_uid, _)) = worst else {
            return false;
        };
        for name in ["信頼", "友情", "愛", "根性", "ド根性"] {
            let Some((_, cost)) = opts.iter().find(|(n, _)| n == name) else {
                continue;
            };
            let cost = *cost;
            match self.spirit_target_kind(name) {
                SpiritTargetKind::SingleAlly => {
                    self.consume_unit_sp(&uid, cost);
                    self.apply_spirit_effect(&worst_uid, name);
                }
                SpiritTargetKind::AllAllies => {
                    self.consume_unit_sp(&uid, cost);
                    let targets: Vec<String> = self
                        .database
                        .unit_instances
                        .iter()
                        .filter(|u| !u.off_map && u.party == party)
                        .map(|u| u.uid.clone())
                        .collect();
                    for t in &targets {
                        self.apply_spirit_effect(t, name);
                    }
                }
                SpiritTargetKind::SelfOnly => {
                    // 根性 / ド根性 は自分のみ。自分が最も負傷しているときだけ使う。
                    if worst_uid != uid {
                        continue;
                    }
                    self.consume_unit_sp(&uid, cost);
                    self.apply_spirit_effect(&uid, name);
                }
                SpiritTargetKind::SingleEnemy => continue,
            }
            let nick = self.database.unit_instances[idx].pilot_name.clone();
            self.push_message(format!("{nick} は精神コマンド【{name}】を使用！"));
            return true;
        }
        false
    }

    /// 復活精神の能動 pre-buff: 瀕死 (実効最大 HP の 1/4 以下) の自ユニットが「復活」精神を
    /// 持ち SP が足りていれば、撃破される前に自分へ「復活」を掛ける。撃破予測の近似として
    /// 低 HP を用いる (次の敵フェイズ前に死にやすい)。精神は行動を消費しない free action。
    /// 既に「復活」を保持しているなら再発動しない (SP 温存)。発動したら `true`。テスト用
    /// ユニットは `spirit_commands` が空なので無効 (= 既存テストへ影響なし)。
    fn ai_use_revive_spirit(&mut self, idx: usize) -> bool {
        // 既に復活を保持していれば不要。
        if self.database.unit_instances[idx].has_condition("復活") {
            return false;
        }
        // 瀕死判定: 実効最大 HP の 1/4 以下。
        let max_hp = self
            .database
            .effective_max_hp(&self.database.unit_instances[idx]);
        let cur_hp = max_hp - self.database.unit_instances[idx].damage;
        if cur_hp * 4 > max_hp {
            return false; // まだ余裕がある
        }
        // 復活精神が発動可能か (level / 残 SP)。
        let uid = self.database.unit_instances[idx].uid.clone();
        let opts = self.spirit_command_options(&uid);
        let Some((_, cost)) = opts.iter().find(|(n, _)| n == "復活") else {
            return false;
        };
        let cost = *cost;
        self.consume_unit_sp(&uid, cost);
        self.apply_spirit_effect(&uid, "復活");
        let nick = self.database.unit_instances[idx].pilot_name.clone();
        self.push_message(format!("{nick} は精神コマンド【復活】を使用！"));
        true
    }

    /// 移動スライド演出を仕込む (逐次演出時のみ)。`reachable` から `start`→`dest` の
    /// 経路を復元して `move_anim` にセットする。`animate_ai=false` (ヘッドレス) では何もしない。
    fn begin_move_anim(
        &mut self,
        uid: &str,
        reachable: &std::collections::HashMap<(u32, u32), i32>,
        start: (u32, u32),
        dest: (u32, u32),
    ) {
        if !self.animate_ai {
            return;
        }
        let path = crate::movement::reconstruct_path(reachable, start, dest);
        if path.len() > 1 {
            self.move_anim = Some(crate::battle_anim::MoveAnim::new(uid.to_string(), path));
        }
    }

    /// AI ユニット (atk_idx, カーソル `cursor`) が狙う射程内最寄り敵対ユニットの
    /// (uid, タイル) を返す (read-only)。`attack_resolve_and_run` の None 分岐と同じ選択。
    fn ai_nearest_target_tile(
        &self,
        atk_idx: usize,
        cursor: (u32, u32),
    ) -> Option<(String, (u32, u32))> {
        let atk = &self.database.unit_instances[atk_idx];
        let atk_party = atk.party;
        let atk_charged = atk.charged;
        let atk_unit = self.database.unit_by_name(&atk.unit_data_name)?;
        let forced = if self.selected_weapon_idx == 0 {
            None
        } else {
            atk_unit.weapons.get(self.selected_weapon_idx - 1)
        };
        let mut best: Option<(usize, u32)> = None;
        for (i, def) in self.database.unit_instances.iter().enumerate() {
            if i == atk_idx || !def.party.is_hostile_to(atk_party) || def.off_map {
                continue;
            }
            let d = combat::manhattan(cursor, (def.x, def.y));
            // post_move=false なので 突撃 (totsugeki) は影響しない。
            if Self::pick_attack_weapon(
                atk_unit,
                d,
                forced,
                atk_charged,
                false,
                false,
                |w| self.weapon_firable(atk_idx, w),
                |w| self.weapon_skill_ok(atk_idx, w),
            )
            .is_some()
            {
                match best {
                    None => best = Some((i, d)),
                    Some((_, bd)) if d < bd => best = Some((i, d)),
                    _ => {}
                }
            }
        }
        let (di, _) = best?;
        let d = &self.database.unit_instances[di];
        Some((d.uid.clone(), (d.x, d.y)))
    }

    /// 反撃モードのプロンプトを出すべき防御側か (味方ユニットのみ)。
    fn reaction_applies(&self, def_uid: &str) -> bool {
        self.database
            .unit_by_uid(def_uid)
            .map(|u| u.party == crate::Party::Player)
            .unwrap_or(false)
    }

    /// 防御側が攻撃側まで射程内の武器を持ち反撃できるか (read-only)。
    fn defender_can_counter(&self, def_uid: &str, atk_uid: &str) -> bool {
        let (Some(def), Some(atk)) = (
            self.database.unit_by_uid(def_uid),
            self.database.unit_by_uid(atk_uid),
        ) else {
            return false;
        };
        // 行動不能のユニットは反撃できない。
        if def.attack_disabled() {
            return false;
        }
        let dist = combat::manhattan((def.x, def.y), (atk.x, atk.y));
        self.database
            .unit_by_name(&def.unit_data_name)
            .map(|du| du.weapons.iter().any(|w| combat::weapon_in_range(w, dist)))
            .unwrap_or(false)
    }

    /// 反撃手段の選択メニュー (反撃/回避/防御) を提示する。チャージ中の防御側は
    /// 強制「防御」で即解決 (SRC 準拠)。`PendingDialog::Menu` を流用して UI を出す。
    fn begin_reaction_prompt(&mut self, atk_uid: String, def_uid: String, tile: (u32, u32)) {
        // 行動不能 (麻痺/混乱/睡眠/捕縛 等) は反撃も能動的な回避/防御も選べない
        // (SRC `MaxAction()==0`: 反撃不可・選択不可)。プロンプト無しで素通り解決する
        // (def_mode 空 = 命中率/ダメージ補正なし、反撃は try_counterattack 側で抑止)。
        let disabled = self
            .database
            .unit_by_uid(&def_uid)
            .map(|u| u.attack_disabled())
            .unwrap_or(false);
        if disabled {
            self.set_cursor_to_uid(&atk_uid);
            self.attack_resolve_and_run(Some(tile), false, "");
            return;
        }
        // チャージ中は強制防御 (プロンプト無し)。
        let charging = self
            .database
            .unit_by_uid(&def_uid)
            .map(|u| u.charged)
            .unwrap_or(false);
        if charging {
            self.set_cursor_to_uid(&atk_uid);
            self.attack_resolve_and_run(Some(tile), false, "防御");
            return;
        }
        let can_counter = self.defender_can_counter(&def_uid, &atk_uid);
        let mut options: Vec<String> = Vec::new();
        let mut modes: Vec<String> = Vec::new();
        if can_counter {
            options.push("反撃".to_string());
            modes.push("反撃".to_string());
        }
        options.push("回避".to_string());
        modes.push("回避".to_string());
        options.push("防御".to_string());
        modes.push("防御".to_string());
        // 隣接に援護防御可能な味方がいれば「援護防御」を選べる (SRC 反撃モード)。
        if self.support_guard_available(&def_uid) {
            options.push("援護防御".to_string());
            modes.push("援護防御".to_string());
        }
        let nick = self
            .database
            .unit_by_uid(&def_uid)
            .map(|u| u.pilot_name.clone())
            .unwrap_or_default();
        self.pending_reaction = Some(PendingReaction {
            atk_uid,
            target_tile: tile,
            modes,
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: format!("{nick} 反撃手段を選択"),
            options,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            // 反撃メニューは pending_reaction が別途処理する (キャンセルは右クリック)。
            non_cancellable: false,
        });
    }

    /// カーソルを uid のユニット位置へ移す内部ヘルパ。
    fn set_cursor_to_uid(&mut self, uid: &str) {
        if let Some(u) = self.database.unit_by_uid(uid) {
            self.map_cursor = Some((u.x, u.y));
        }
    }

    /// 反撃モードのメニュー応答を処理する。`respond_dialog` から委譲される。
    /// `choice` は 1-based のメニュー選択 (0/範囲外は先頭=反撃/回避にフォールバック)。
    fn resolve_reaction(&mut self, choice: u32) -> bool {
        let Some(pr) = self.pending_reaction.take() else {
            return false;
        };
        self.pending_dialog = None; // 反撃メニューを閉じる
        let idx = choice as usize;
        let mode = if idx >= 1 && idx <= pr.modes.len() {
            pr.modes[idx - 1].clone()
        } else {
            pr.modes.first().cloned().unwrap_or_default()
        };
        self.set_cursor_to_uid(&pr.atk_uid);
        // 「反撃」選択時、反撃武器が複数あり武器選択ウィンドウが有効なら、どの反撃
        // 武器で反撃するかを選ばせる (使用可能が 1 本以下なら自動選定のまま解決)。
        if mode == "反撃" && self.weapon_window_enabled {
            if let (Some(atk_idx), Some(def_idx)) = (
                self.database.idx_by_uid(&pr.atk_uid),
                self.database
                    .unit_instances
                    .iter()
                    .position(|u| !u.off_map && (u.x, u.y) == pr.target_tile),
            ) {
                let usable_cnt = self
                    .build_weapon_rows(def_idx, atk_idx, false)
                    .iter()
                    .filter(|r| r.usable)
                    .count();
                if usable_cnt >= 2 {
                    let def_uid = self.database.unit_instances[def_idx].uid.clone();
                    let atk_tile = {
                        let a = &self.database.unit_instances[atk_idx];
                        (a.x, a.y)
                    };
                    self.begin_counter_weapon_select(
                        def_uid,
                        atk_tile,
                        pr.atk_uid.clone(),
                        pr.target_tile,
                    );
                    return true;
                }
            }
        }
        self.attack_resolve_and_run(Some(pr.target_tile), false, &mode);
        // ランナーは pending_dialog が消えたので次 tick で継続する。
        true
    }

    /// `atk_idx` が `def_idx` を攻撃するときの使用武器・攻撃力・基準命中率 (%) を
    /// 予測する。`attack_resolve_and_run` と同じ武器選定 (`pick_attack_weapon`) と
    /// 命中予測 (`predict_with_status_terrain` + 散布補正) を使い、反撃ウィンドウの
    /// 命中率表示に用いる (読み取り専用、盤面は変更しない)。射程内に使用可能武器が
    /// 無ければ `None`。
    fn attack_hit_forecast(&self, atk_idx: usize, def_idx: usize) -> Option<(String, i64, i32)> {
        let (atk_pilot, atk_unit) = self.database.effective_combat_data(atk_idx)?;
        let (def_pilot, def_unit) = self.database.effective_combat_data(def_idx)?;
        // 借用を解放するため戦闘文脈をローカルへ複製。
        let (ax, ay, atk_morale, atk_charged, atk_totsugeki, atk_statuses) = {
            let a = &self.database.unit_instances[atk_idx];
            let mut s: Vec<String> = a.conditions.iter().map(|c| c.name.clone()).collect();
            push_combat_feature_statuses(a, &mut s);
            (a.x, a.y, a.morale, a.charged, a.has_condition("突撃"), s)
        };
        let (dx, dy, def_morale, def_statuses) = {
            let d = &self.database.unit_instances[def_idx];
            let mut s: Vec<String> = d.conditions.iter().map(|c| c.name.clone()).collect();
            push_combat_feature_statuses(d, &mut s);
            (d.x, d.y, d.morale, s)
        };
        let dist = combat::manhattan((ax, ay), (dx, dy));
        let weapon = Self::pick_attack_weapon(
            &atk_unit,
            dist,
            None,
            atk_charged,
            false,
            atk_totsugeki,
            |w| self.weapon_firable(atk_idx, w),
            |w| self.weapon_skill_ok(atk_idx, w),
        )?;
        let terrain_id = self.database.map.as_ref()?.cell(dx, dy).terrain_id;
        let def_hit_mod = self.database.terrain_hit_mod(terrain_id);
        let def_damage_mod = self.database.terrain_damage_mod(terrain_id);
        let dmg_levels = self
            .database
            .damage_spirit_levels(&atk_statuses, &def_statuses);
        let preview = combat::predict_with_status_terrain(
            &atk_pilot,
            &atk_unit,
            &weapon,
            &def_pilot,
            &def_unit,
            def_hit_mod,
            def_damage_mod,
            atk_morale,
            def_morale,
            &atk_statuses,
            &def_statuses,
            self.terrain_env_at(ax, ay),
            self.terrain_env_at(dx, dy),
            dmg_levels,
            self.ecm_hit_mult(def_idx, atk_idx, &weapon.class),
        )
        .apply_scatter(&weapon.class, dist);
        Some((
            weapon.name.clone(),
            weapon.power,
            preview.hit_chance.clamp(0, 100),
        ))
    }

    /// 反撃手段選択中なら、ウィンドウ表示用データを構築する (front が描画)。
    /// `pending_reaction` が無い / 攻防ユニットを解決できない場合は `None`。
    pub fn reaction_window_data(&self) -> Option<crate::scene::reaction::ReactionWindowData> {
        use crate::scene::reaction::{ReactionOption, ReactionWindowData};
        let pr = self.pending_reaction.as_ref()?;
        let atk_idx = self.database.idx_by_uid(&pr.atk_uid)?;
        let def_idx = self
            .database
            .unit_instances
            .iter()
            .position(|u| !u.off_map && (u.x, u.y) == pr.target_tile)?;
        let (weapon, power, base_hit) = self.attack_hit_forecast(atk_idx, def_idx)?;
        let attacker = {
            let a = &self.database.unit_instances[atk_idx];
            (a.x, a.y)
        };
        let options = pr
            .modes
            .iter()
            .map(|m| {
                // 回避は命中半減 (SRC 反撃モード / attack_resolve_and_run の def_mode="回避")。
                let hit = if m == "回避" {
                    (base_hit / 2).max(0)
                } else {
                    base_hit
                };
                ReactionOption {
                    label: m.clone(),
                    hit_pct: hit,
                }
            })
            .collect();
        Some(ReactionWindowData {
            attacker,
            defender: pr.target_tile,
            weapon,
            power,
            base_hit,
            options,
        })
    }

    // ── 武器選択ウィンドウ (攻撃側) ───────────────────────────────────────────

    /// `weapon` を `dist` の目標へ `post_move` 状況で使用可能か (射程 / チャージ /
    /// 移動後 / 突撃 / 資源 / 必要技能)。武器選択窓の行表示・選択可否に使う。
    fn weapon_usable_vs(
        &self,
        atk_idx: usize,
        weapon: &crate::data::unit::WeaponData,
        dist: u32,
        post_move: bool,
    ) -> bool {
        let u = &self.database.unit_instances[atk_idx];
        let totsugeki = u.has_condition("突撃");
        combat::weapon_in_range(weapon, dist)
            && (u.charged || !combat::is_charge_weapon(weapon))
            && Self::weapon_usable_post_move(weapon, post_move, totsugeki)
            && self.weapon_firable(atk_idx, weapon)
            && self.weapon_skill_ok(atk_idx, weapon)
    }

    /// `atk_uid` がタイル `target` の敵対ユニットを `post_move` 状況で攻撃可能か
    /// (射程内に使用可能な武器が 1 つでもあるか)。武器選択窓を出すか即解決するかの判定。
    fn can_attack_target(&self, atk_uid: &str, target: (u32, u32), post_move: bool) -> bool {
        let Some(atk_idx) = self.database.idx_by_uid(atk_uid) else {
            return false;
        };
        let atk = &self.database.unit_instances[atk_idx];
        let (atk_party, ax, ay) = (atk.party, atk.x, atk.y);
        let hostile = self
            .database
            .unit_instances
            .iter()
            .any(|u| !u.off_map && (u.x, u.y) == target && u.party.is_hostile_to(atk_party));
        if !hostile {
            return false;
        }
        let Some((_, atk_unit)) = self.database.effective_combat_data(atk_idx) else {
            return false;
        };
        let dist = combat::manhattan((ax, ay), target);
        atk_unit
            .weapons
            .iter()
            .any(|w| self.weapon_usable_vs(atk_idx, w, dist, post_move))
    }

    /// プレイヤー攻撃の後始末 (行動終了 / 行動終了・接触イベント / Browse 復帰)。
    /// 即解決経路と武器選択経路の両方から呼ぶ。
    fn finish_player_attack(&mut self, uid: &str) {
        if let Some(u) = self.database.unit_by_uid_mut(uid) {
            u.has_acted = true;
        }
        if let Some(i) = self.database.idx_by_uid(uid) {
            crate::event_runtime::fire_action_end_labels(self, i);
            crate::event_runtime::fire_contact_event_labels(self, i);
        }
        self.action_mode = crate::command_menu::ActionMode::Browse;
        self.command_menu = None;
    }

    /// 武器選択ウィンドウを開始する。武器名一覧を `PendingDialog::Menu` に積み
    /// (番号キー / クリックのルーティング用)、`pending_weapon_select` を保持する。
    /// 武装が無ければ即解決へフォールバック。
    fn begin_weapon_select(&mut self, atk_uid: String, target: (u32, u32), post_move: bool) {
        let names: Vec<String> = self
            .database
            .idx_by_uid(&atk_uid)
            .and_then(|i| self.database.unit_instances.get(i))
            .and_then(|u| self.database.unit_by_name(&u.unit_data_name))
            .map(|d| d.weapons.iter().map(|w| w.name.clone()).collect())
            .unwrap_or_default();
        if names.is_empty() {
            if self.attack_unit_at(&atk_uid, target, post_move) {
                self.finish_player_attack(&atk_uid);
            }
            return;
        }
        self.pending_weapon_select = Some(PendingWeaponSelect {
            atk_uid,
            target,
            post_move,
            counter: None,
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: "武器選択".to_string(),
            options: names,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            non_cancellable: false,
        });
    }

    /// 反撃武器選択を開始する。`def_uid` (反撃する側) の反撃武器一覧を Menu に積み、
    /// 解決時に選んだ武器を `forced_counter_weapon` に設定して主攻撃を `反撃` で解決する。
    /// `main_atk_uid` / `main_target` は主攻撃 (元の攻撃) の文脈。
    fn begin_counter_weapon_select(
        &mut self,
        def_uid: String,
        atk_tile: (u32, u32),
        main_atk_uid: String,
        main_target: (u32, u32),
    ) {
        let names: Vec<String> = self
            .database
            .unit_by_uid(&def_uid)
            .and_then(|u| self.database.unit_by_name(&u.unit_data_name))
            .map(|d| d.weapons.iter().map(|w| w.name.clone()).collect())
            .unwrap_or_default();
        self.pending_weapon_select = Some(PendingWeaponSelect {
            atk_uid: def_uid,
            target: atk_tile,
            post_move: false,
            counter: Some(CounterCtx {
                main_atk_uid,
                main_target,
            }),
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: "反撃武器選択".to_string(),
            options: names,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            non_cancellable: false,
        });
    }

    /// 武器選択窓の行データを構築する (機体の武装順)。命中率は
    /// `predict_with_status_terrain` (+散布補正) を読み取り専用で評価する。
    fn build_weapon_rows(
        &self,
        atk_idx: usize,
        def_idx: usize,
        post_move: bool,
    ) -> Vec<crate::scene::weapon_select::WeaponSelectRow> {
        use crate::scene::weapon_select::WeaponSelectRow;
        let (Some((atk_pilot, atk_unit)), Some((def_pilot, def_unit))) = (
            self.database.effective_combat_data(atk_idx),
            self.database.effective_combat_data(def_idx),
        ) else {
            return Vec::new();
        };
        let (ax, ay, atk_morale, atk_statuses) = {
            let a = &self.database.unit_instances[atk_idx];
            let mut s: Vec<String> = a.conditions.iter().map(|c| c.name.clone()).collect();
            push_combat_feature_statuses(a, &mut s);
            (a.x, a.y, a.morale, s)
        };
        let (dx, dy, def_morale, def_statuses) = {
            let d = &self.database.unit_instances[def_idx];
            let mut s: Vec<String> = d.conditions.iter().map(|c| c.name.clone()).collect();
            push_combat_feature_statuses(d, &mut s);
            (d.x, d.y, d.morale, s)
        };
        let dist = combat::manhattan((ax, ay), (dx, dy));
        let terrain_id = self
            .database
            .map
            .as_ref()
            .map(|m| m.cell(dx, dy).terrain_id);
        let def_hit_mod = terrain_id.map_or(0, |t| self.database.terrain_hit_mod(t));
        let def_damage_mod = terrain_id.map_or(0, |t| self.database.terrain_damage_mod(t));
        let atk_env = self.terrain_env_at(ax, ay);
        let def_env = self.terrain_env_at(dx, dy);
        let dmg_levels = self
            .database
            .damage_spirit_levels(&atk_statuses, &def_statuses);
        let atk_inst = &self.database.unit_instances[atk_idx];
        atk_unit
            .weapons
            .iter()
            .enumerate()
            .map(|(i, w)| {
                let usable = self.weapon_usable_vs(atk_idx, w, dist, post_move);
                let hit_pct = if usable {
                    combat::predict_with_status_terrain(
                        &atk_pilot,
                        &atk_unit,
                        w,
                        &def_pilot,
                        &def_unit,
                        def_hit_mod,
                        def_damage_mod,
                        atk_morale,
                        def_morale,
                        &atk_statuses,
                        &def_statuses,
                        atk_env,
                        def_env,
                        dmg_levels,
                        self.ecm_hit_mult(def_idx, atk_idx, &w.class),
                    )
                    .apply_scatter(&w.class, dist)
                    .hit_chance
                    .clamp(0, 100)
                } else {
                    0
                };
                let uw = atk_inst.weapons.iter().find(|uw| uw.weapon_index == i);
                let ammo = if w.bullet > 0 {
                    let rem = uw.map_or(w.bullet, |x| x.bullet_remaining.max(0));
                    format!("残{}/{}", rem, w.bullet)
                } else if w.en_consumption > 0 {
                    format!("EN{}", w.en_consumption)
                } else {
                    "-".to_string()
                };
                WeaponSelectRow {
                    name: w.name.clone(),
                    power: w.power,
                    hit_pct,
                    critical: w.critical,
                    ammo,
                    adaption: w.adaption.clone(),
                    class: w.class.clone(),
                    usable,
                }
            })
            .collect()
    }

    /// 武器選択中なら、ウィンドウ表示用データを構築する (front が描画)。
    pub fn weapon_select_window_data(
        &self,
    ) -> Option<crate::scene::weapon_select::WeaponSelectWindowData> {
        use crate::scene::weapon_select::WeaponSelectWindowData;
        let pws = self.pending_weapon_select.as_ref()?;
        let atk_idx = self.database.idx_by_uid(&pws.atk_uid)?;
        let atk_party = self.database.unit_instances[atk_idx].party;
        let def_idx = self.database.unit_instances.iter().position(|u| {
            !u.off_map && (u.x, u.y) == pws.target && u.party.is_hostile_to(atk_party)
        })?;
        let attacker = {
            let a = &self.database.unit_instances[atk_idx];
            (a.x, a.y)
        };
        let rows = self.build_weapon_rows(atk_idx, def_idx, pws.post_move);
        Some(WeaponSelectWindowData {
            attacker,
            defender: pws.target,
            rows,
            is_counter: pws.counter.is_some(),
        })
    }

    /// 武器選択メニュー応答を処理する (`respond_dialog` から委譲)。`choice` は
    /// 1-based。選んだ武器が使用不可なら無視 (窓継続)。使用可能なら固定して攻撃・解決。
    fn resolve_weapon_select(&mut self, choice: u32) -> bool {
        let Some(pws) = self.pending_weapon_select.clone() else {
            return false;
        };
        let Some(atk_idx) = self.database.idx_by_uid(&pws.atk_uid) else {
            self.pending_weapon_select = None;
            self.pending_dialog = None;
            return false;
        };
        let widx = choice as usize; // 1-based 武器番号
                                    // 選んだ武器の使用可否を検証 (× 行は無視)。
        let usable = self
            .database
            .effective_combat_data(atk_idx)
            .and_then(|(_, u)| u.weapons.get(widx.wrapping_sub(1)).cloned())
            .map(|w| {
                let dist = {
                    let a = &self.database.unit_instances[atk_idx];
                    combat::manhattan((a.x, a.y), pws.target)
                };
                self.weapon_usable_vs(atk_idx, &w, dist, pws.post_move)
            })
            .unwrap_or(false);
        if widx == 0 || !usable {
            return true; // 無効選択 → 窓を継続
        }
        self.pending_weapon_select = None;
        self.pending_dialog = None;
        if let Some(cc) = pws.counter {
            // 反撃武器選択: 選んだ武器を反撃武器に固定し、主攻撃を「反撃」で解決する
            // (try_counterattack が forced_counter_weapon を take して使う)。
            self.forced_counter_weapon = Some(widx - 1);
            self.set_cursor_to_uid(&cc.main_atk_uid);
            self.attack_resolve_and_run(Some(cc.main_target), false, "反撃");
            self.forced_counter_weapon = None;
            return true;
        }
        // 通常攻撃: 選んだ武器を固定して攻撃 (1回限り; 終了後に W キー設定へ戻す)。
        let prev = self.selected_weapon_idx;
        self.selected_weapon_idx = widx;
        let attacked = self.attack_unit_at(&pws.atk_uid, pws.target, pws.post_move);
        self.selected_weapon_idx = prev;
        if attacked {
            self.finish_player_attack(&pws.atk_uid);
        }
        true
    }

    /// 武器選択をキャンセルする (右クリック / Esc)。AttackSelect 状態は維持し、
    /// 目標を選び直せるようにする。
    fn cancel_weapon_select(&mut self) -> bool {
        if self.pending_weapon_select.take().is_some() {
            self.pending_dialog = None;
            true
        } else {
            false
        }
    }

    // ── 精神コマンド (SP コマンド) ───────────────────────────────────────────
    //
    // SRC ([精神コマンド.md] / [SPコマンド]) のユニットコマンド「精神」。パイロットが
    // 習得済み (`level <= パイロットレベル`) の SP コマンドを、残り SP の範囲で発動する。
    // 戦闘側の効果 (集中/熱血/魂/必中/ひらめき/不屈/鉄壁/気合 等) は `combat.rs`
    // `predict_with_status` が `UnitInstance.conditions` を読んで反映するため、ここでは
    // SP を消費して 1 ターン (lifetime=1) の condition を付与する。`begin_phase` が当該陣営の
    // 次フェイズ開始で lifetime==1 の condition を解除する。

    /// ユニットの主パイロットのレベルを返す。`PilotInstance` があればその level、
    /// 無ければ `UnitInstance.total_exp` から算出 (`db::effective_pilot_data` と同式)。
    fn unit_pilot_level(&self, u: &crate::UnitInstance) -> i32 {
        let pilot_id = u
            .pilot_ids
            .first()
            .map(|s| s.as_str())
            .unwrap_or(u.pilot_name.as_str());
        if let Some(pi) = self
            .database
            .pilot_instances
            .iter()
            .find(|p| p.id == pilot_id || p.pilot_data_name == pilot_id)
        {
            return pi.level;
        }
        crate::pilot_instance::level_from_exp(u.total_exp)
    }

    /// ユニットの主パイロットの (最大 SP, 残り SP) を返す。`PilotInstance` があれば
    /// その `sp_remaining`、無ければ `PilotData.sp - UnitInstance.sp_consumed`。
    fn unit_sp(&self, u: &crate::UnitInstance) -> (i32, i32) {
        let pilot_id = u
            .pilot_ids
            .first()
            .map(|s| s.as_str())
            .unwrap_or(u.pilot_name.as_str());
        if let Some(pi) = self
            .database
            .pilot_instances
            .iter()
            .find(|p| p.id == pilot_id || p.pilot_data_name == pilot_id)
        {
            let max = self
                .database
                .pilot_by_name(&pi.pilot_data_name)
                .and_then(|p| p.sp)
                .unwrap_or(0);
            return (max, pi.sp_remaining.max(0));
        }
        let max = self
            .database
            .pilot_by_name(&u.pilot_name)
            .and_then(|p| p.sp)
            .unwrap_or(0);
        (max, (max - u.sp_consumed).max(0))
    }

    /// 精神コマンド `name` の消費 SP を解決する。コマンド定義のコスト指定 (`override_cost`)
    /// を最優先、次に sp.txt (`special_powers`) の既定、最後に組込みテーブル。
    fn spirit_cost(&self, name: &str, override_cost: Option<i32>) -> i32 {
        if let Some(c) = override_cost {
            return c;
        }
        if let Some(sp) = self
            .database
            .special_powers
            .iter()
            .find(|sp| sp.name == name)
        {
            if sp.sp_consumption > 0 {
                return sp.sp_consumption;
            }
        }
        crate::event_runtime::sp_cost_for(name)
    }

    /// 当該ユニットが現在発動可能な精神コマンドの (名前, 消費SP) 列を返す。
    /// 条件: パイロットが習得済み (`level <= パイロットレベル`) かつ 消費 SP が残り SP 以内。
    fn spirit_command_options(&self, uid: &str) -> Vec<(String, i32)> {
        let Some(u) = self.database.unit_by_uid(uid) else {
            return Vec::new();
        };
        // 憑依 (特殊効果攻撃属性 憑): 支配されたユニットはスペシャルパワーを使用できない。
        // プレイヤー UI (`open_spirit_menu`) と AI 精神 (`ai_use_*_spirit`) はいずれも
        // ここを唯一の関門にしているため、空を返せば全経路で封じられる。
        if u.has_condition("憑依") {
            return Vec::new();
        }
        let Some(pdata) = self.database.pilot_by_name(&u.pilot_name) else {
            return Vec::new();
        };
        if pdata.spirit_commands.is_empty() {
            return Vec::new();
        }
        let level = self.unit_pilot_level(u);
        let (_max, remaining) = self.unit_sp(u);
        let mut out = Vec::new();
        for sc in &pdata.spirit_commands {
            if sc.level > level {
                continue;
            }
            let cost = self.spirit_cost(&sc.name, sc.cost);
            if cost > remaining {
                continue;
            }
            // 同名コマンドの重複や既発動中 (condition 保持) は除外。
            if out.iter().any(|(n, _): &(String, i32)| n == &sc.name) {
                continue;
            }
            out.push((sc.name.clone(), cost));
        }
        out
    }

    /// 精神コマンドのサブメニュー (SP コマンド一覧) を開く。
    fn open_spirit_menu(&mut self, uid: &str) {
        let commands = self.spirit_command_options(uid);
        if commands.is_empty() {
            self.push_message("発動できる精神コマンドがありません".to_string());
            return;
        }
        let (nick, remaining) = self
            .database
            .unit_by_uid(uid)
            .map(|u| (u.pilot_name.clone(), self.unit_sp(u).1))
            .unwrap_or_default();
        let options: Vec<String> = commands
            .iter()
            .map(|(name, cost)| format!("{name} ({cost})"))
            .collect();
        self.pending_spirit = Some(PendingSpirit {
            uid: uid.to_string(),
            commands,
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: format!("{nick} 精神コマンド (SP {remaining})"),
            options,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            // キャンセルは Esc / choice 0 (pending_spirit が別途処理する)。
            non_cancellable: false,
        });
    }

    /// 精神コマンドのサブメニュー応答を処理する。`respond_dialog` から委譲される。
    /// `choice` は 1-based (0 / 範囲外はキャンセル)。発動後はユニットメニューを再表示し、
    /// 続けて別の精神コマンドや移動 / 攻撃を選べるようにする (SRC: 精神は行動を消費しない)。
    fn resolve_spirit(&mut self, choice: u32) -> bool {
        let Some(ps) = self.pending_spirit.take() else {
            return false;
        };
        self.pending_dialog = None; // サブメニューを閉じる
        let idx = choice as usize;
        if idx == 0 || idx > ps.commands.len() {
            // キャンセル: ユニットメニューを再表示。
            self.reopen_unit_menu_for(&ps.uid);
            return true;
        }
        let (name, cost) = ps.commands[idx - 1].clone();
        // 念のため残り SP を再確認 (連続発動で枯渇している場合)。
        let remaining = self
            .database
            .unit_by_uid(&ps.uid)
            .map(|u| self.unit_sp(u).1)
            .unwrap_or(0);
        if cost > remaining {
            self.push_message(format!("SP が足りません ({name}: {cost})"));
            self.reopen_unit_menu_for(&ps.uid);
            return true;
        }
        match self.spirit_target_kind(&name) {
            // 対象選択が必要なコマンドは AttackSelect と同様の選択モードへ遷移し、
            // 対象確定時 (`apply_spirit_to_target`) に SP を消費する。キャンセル
            // (右クリック / Esc) では SP を消費しないため、ここでは消費しない。
            kind @ (SpiritTargetKind::SingleAlly | SpiritTargetKind::SingleEnemy) => {
                self.begin_spirit_target(&ps.uid, &name, cost, kind);
                return true;
            }
            SpiritTargetKind::SelfOnly => {
                self.consume_unit_sp(&ps.uid, cost);
                let caster = ps.uid.clone();
                self.apply_spirit_effect(&caster, &name);
            }
            SpiritTargetKind::AllAllies => {
                self.consume_unit_sp(&ps.uid, cost);
                let party = self.database.unit_by_uid(&ps.uid).map(|u| u.party);
                if let Some(party) = party {
                    let targets: Vec<String> = self
                        .database
                        .unit_instances
                        .iter()
                        .filter(|u| u.party == party)
                        .map(|u| u.uid.clone())
                        .collect();
                    for t in &targets {
                        self.apply_spirit_effect(t, &name);
                    }
                }
            }
        }
        self.push_message(format!("精神コマンド【{name}】発動 (SP -{cost})"));
        // 続けて発動 / 行動できるようユニットメニューを再表示。
        self.reopen_unit_menu_for(&ps.uid);
        true
    }

    /// 精神コマンド `name` の対象種別を解決する。sp.txt の `TargetType` を優先。
    fn spirit_target_kind(&self, name: &str) -> SpiritTargetKind {
        if let Some(sp) = self.database.special_powers.iter().find(|s| s.name == name) {
            match sp.target_type.as_str() {
                "自分" | "自分のみ" => return SpiritTargetKind::SelfOnly,
                // SRC `SpecialPowerData.cls` Execute 準拠: `全味方`/`全体` のみ全体、
                // `味方`/`単体`/`指定` は単体選択 (旧実装は `味方` を全体と誤判定していた)。
                "全体" | "味方全体" | "全味方" => return SpiritTargetKind::AllAllies,
                "単体" | "味方単体" | "指定" | "味方" => {
                    return SpiritTargetKind::SingleAlly
                }
                "敵単体" | "敵" | "敵指定" => return SpiritTargetKind::SingleEnemy,
                _ => {}
            }
        }
        // 組込み既定 (sp.txt 未定義時)。
        match name {
            "友情" | "愛" | "鼓舞" => SpiritTargetKind::AllAllies,
            "信頼" | "補給" | "祝福" | "応援" | "激励" | "再動" | "癒し" => {
                SpiritTargetKind::SingleAlly
            }
            "脱力" | "戦慄" | "かく乱" | "威圧" | "魅惑" | "足かせ" => {
                SpiritTargetKind::SingleEnemy
            }
            _ => SpiritTargetKind::SelfOnly,
        }
    }

    /// 精神コマンドの効果を `target` ユニットへ適用する。継続効果は `lifetime=1`
    /// の condition として付与し (戦闘 / 移動 / 撃破報酬 / 復活判定が名前で参照)、
    /// 瞬間効果 (HP/EN/弾/気力/再行動) は即時に反映する。
    fn apply_spirit_effect(&mut self, target: &str, name: &str) {
        // 敵対象 (攻撃系) の精神コマンドは、対象が `スペシャルパワー無効化` /
        // `精神コマンド無効化` を持つと効果が無効化される (SRC `SpecialPowerData.cs:523-540`:
        // TargetType 敵/全敵/任意/全 の SP は当該 condition を持つユニットに効果なし)。
        // SP コストは発動側で既に消費済みで、対象への効果だけを無効化する。
        if matches!(self.spirit_target_kind(name), SpiritTargetKind::SingleEnemy) {
            let immune = self.database.unit_by_uid(target).map(|u| {
                (
                    u.has_condition("スペシャルパワー無効化")
                        || u.has_condition("精神コマンド無効化"),
                    u.unit_data_name.clone(),
                )
            });
            if let Some((true, nm)) = immune {
                self.push_message(format!("{nm} には精神コマンド【{name}】が効かない！"));
                return;
            }
        }
        match name {
            // ── 継続効果: combat.rs / movement / 撃破報酬 / 復活判定が名前で参照 ──
            // (必中/集中/ひらめき/熱血/魂/気合/不屈/鉄壁 は既に combat.rs が解釈。
            //  加速/神速→movement, 復活→撃破時, 幸運/努力→撃破報酬, 突撃→攻撃判定。)
            "必中" | "集中" | "ひらめき" | "熱血" | "魂" | "気合" | "不屈" | "鉄壁" | "加速"
            | "神速" | "復活" | "幸運" | "努力" | "突撃" | "捨て身" | "直撃" => {
                self.add_unit_condition(target, name, 1);
            }
            // ── 瞬間: 再行動 (行動済みフラグを戻す) ──
            "覚醒" | "再動" => self.reset_unit_action(target),
            // ── 瞬間: HP 回復 ──
            "信頼" => self.spirit_heal_fraction(target, 3), // 最大HPの 1/3
            "友情" => self.spirit_heal_fraction(target, 2), // 最大HPの 1/2
            "根性" => self.spirit_heal_fraction(target, 3),
            "愛" | "ド根性" => self.spirit_heal_full(target),
            // ── 瞬間: EN・弾数全快 (補給は気力 -10) ──
            "補給" => {
                self.spirit_resupply(target);
                self.add_unit_morale(target, -10);
            }
            "瞑想" => self.spirit_resupply(target),
            // ── 瞬間: 気力増減 ──
            "脱力" => self.add_unit_morale(target, -10),
            "激励" => self.add_unit_morale(target, 10),
            "鼓舞" => self.add_unit_morale(target, 5),
            // ── 効果付与: 対象に別コマンドの継続効果を与える ──
            "応援" => self.add_unit_condition(target, "努力", 1),
            "祝福" => self.add_unit_condition(target, "幸運", 1),
            // ── 複合 (奇跡 = 加速+魂+必中+ひらめき+幸運+気力+全快) ──
            "奇跡" => {
                for n in ["加速", "魂", "必中", "ひらめき", "幸運"] {
                    self.add_unit_condition(target, n, 1);
                }
                self.add_unit_morale(target, 30);
                self.spirit_heal_full(target);
            }
            "奇襲" => {
                for n in ["加速", "熱血", "必中", "ひらめき"] {
                    self.add_unit_condition(target, n, 1);
                }
            }
            // ── 既定: SRC 標準以外 (シナリオ独自定義) の精神コマンドは継続 condition として
            //   名前のまま付与するに留める。効果はシナリオ側 (sp.txt の効果種別 / イベント) で
            //   解決すべきであり、原典 (SRC_20121125 / SRC.Sharp) に定義の無いコマンドの効果を
            //   エンジンへ独自実装 (推測) しない。例: 東方夢想伝の 決意/気迫/希望 は原典に無く、
            //   ここでは無害な named condition となる (combat.rs は解釈しない)。 ──
            _ => self.add_unit_condition(target, name, 1),
        }
    }

    /// `target` に lifetime=1 の condition を付与する (発動陣営の次フェイズ開始で解除)。
    fn add_unit_condition(&mut self, uid: &str, name: &str, lifetime: i32) {
        if let Some(u) = self.database.unit_by_uid_mut(uid) {
            u.add_condition(crate::Condition::new(name, lifetime));
        }
    }

    /// `target` の気力を `delta` 増減する (0..=150 にクランプ)。
    fn add_unit_morale(&mut self, uid: &str, delta: i32) {
        if let Some(u) = self.database.unit_by_uid_mut(uid) {
            u.morale = (u.morale + delta).clamp(0, 150);
        }
    }

    /// `target` の行動済み / 移動済みフラグを解除し、再行動可能にする。
    fn reset_unit_action(&mut self, uid: &str) {
        if let Some(u) = self.database.unit_by_uid_mut(uid) {
            u.has_acted = false;
            u.has_moved = false;
        }
    }

    /// `target` の HP を 最大HP/`denom` だけ回復する (撃破はしない / 既に最大なら無効)。
    /// `uid` がゾンビ状態 (特殊効果攻撃属性 ゾ) で、アビリティ / 精神 / 修理補給 等の
    /// 能動的な HP/EN 回復を受けられないか。地形・特殊能力による自然回復は別 (回復可)。
    fn recovery_blocked(&self, uid: &str) -> bool {
        self.database
            .unit_by_uid(uid)
            .is_some_and(|u| u.has_condition("ゾンビ"))
    }

    fn spirit_heal_fraction(&mut self, uid: &str, denom: i64) {
        if self.recovery_blocked(uid) {
            return;
        }
        let Some(idx) = self.database.idx_by_uid(uid) else {
            return;
        };
        let max_hp = self
            .database
            .effective_max_hp(&self.database.unit_instances[idx]);
        let heal = (max_hp / denom).max(1);
        let u = &mut self.database.unit_instances[idx];
        u.damage = (u.damage - heal).max(0);
    }

    /// `target` の HP を全快する。
    fn spirit_heal_full(&mut self, uid: &str) {
        if self.recovery_blocked(uid) {
            return;
        }
        if let Some(u) = self.database.unit_by_uid_mut(uid) {
            u.damage = 0;
        }
    }

    /// 精神コマンド「復活」判定: `idx` のユニットが「復活」を保持していれば HP を
    /// 全快させ condition を消費して `true` を返す (= 撃破は成立しない)。保持して
    /// いなければ `false` (= 撃破成立) を返す。各撃破サイトで `remove_unit_at` の
    /// 直前に呼ぶ。
    pub(crate) fn revive_if_possible(&mut self, idx: usize) -> bool {
        if self.database.unit_instances[idx].has_condition("復活") {
            let u = &mut self.database.unit_instances[idx];
            u.remove_condition("復活");
            u.damage = 0;
            true
        } else {
            false
        }
    }

    /// 特殊効果攻撃属性 (`特殊効果攻撃属性.md`): 命中時に確率で防御側へ状態異常を
    /// 付与する。発生確率はクリティカルと同様 (CT率 + (技量差)/2)。付与した状態異常名の
    /// 列を返す (メッセージ用)。武器に該当属性が無ければ何もしない。
    fn apply_weapon_special_effects(
        &mut self,
        def_idx: usize,
        firer_idx: usize,
        weapon: &crate::data::unit::WeaponData,
        atk_pilot: &crate::data::pilot::PilotData,
        def_pilot: &crate::data::pilot::PilotData,
    ) -> Vec<String> {
        let effects = crate::combat::weapon_special_effects(&weapon.class);
        let morale_down = crate::combat::weapon_morale_reduction(&weapon.class);
        let possession = crate::combat::weapon_possession(&weapon.class);
        if effects.is_empty() && morale_down.is_none() && possession.is_none() {
            return Vec::new();
        }
        // 特殊効果無効化 (防御特性に関する特殊能力.md): 防御側が指定特殊効果を無効化する。
        // `特殊効果無効化=全` (または値省略) は全特殊効果を無効化 (サンプルは全て `=全`)。
        // 個別指定は値に一致する効果のみ無効化する (下のループで部分一致判定)。
        let nullify_vals: Vec<String> = self.database.unit_instances[def_idx]
            .active_features
            .iter()
            .filter(|f| f.is_active && f.name == "特殊効果無効化")
            .map(|f| f.value.trim().to_string())
            .collect();
        let nullify_all = nullify_vals
            .iter()
            .any(|v| v.is_empty() || v.split_whitespace().any(|t| t == "全"));
        if nullify_all {
            // 全特殊効果 (状態異常・気力低下・支配) を無効化。ダメージ自体は別経路。
            return Vec::new();
        }
        let prob =
            (weapon.critical + (atk_pilot.technique - def_pilot.technique) / 2).clamp(1, 100);
        // 耐性 / 弱点: 武器の属性に対し対象が耐性を持てば発動率半減、弱点を持てば倍。
        let prob = self.adjust_proc_for_resistance(def_idx, &weapon.class, prob);
        // 抵抗力Lv* (回避系特殊能力.md): 特殊効果発生確率を 10×Lv% 減少
        // (C# Unit.cs:12275 `prob - 10 * FeatureLevel("抵抗力")`)。
        let resist_lv = crate::feature::feature_level(
            &self.database.unit_instances[def_idx].active_features,
            "抵抗力",
        )
        .unwrap_or(0);
        let prob = prob - 10 * resist_lv;
        if prob <= 0 || (self.next_u32() % 100) as i32 >= prob {
            return Vec::new();
        }
        let is_boss = self.database.unit_instances[def_idx].is_boss();
        let mut applied = Vec::new();
        for (name, lifetime) in effects {
            // ボスランク適用ユニットは石化 / 死の宣告 を無効化 (BossRankコマンド.md)。
            if is_boss && (name == "石化" || name == "死の宣告") {
                continue;
            }
            // 特殊効果無効化 (個別指定): 値が状態異常名に部分一致するものは無効化。
            if nullify_vals
                .iter()
                .any(|v| !v.is_empty() && (name.contains(v.as_str()) || v.contains(name.as_str())))
            {
                continue;
            }
            self.database.unit_instances[def_idx]
                .add_condition(crate::Condition::new(&name, lifetime));
            applied.push(name);
        }
        // 脱 / Ｄ: 対象の気力を低下。Ｄ は低下分の半分を攻撃側 (firer) へ吸収する。
        if let Some(amount) = morale_down {
            let uid = self.database.unit_instances[def_idx].uid.clone();
            self.add_unit_morale(&uid, -amount);
            applied.push(format!("気力 -{amount}"));
            if crate::combat::weapon_morale_absorbs(&weapon.class) {
                let gain = amount / 2;
                if gain > 0 {
                    let fuid = self.database.unit_instances[firer_idx].uid.clone();
                    self.add_unit_morale(&fuid, gain);
                    applied.push(format!("気力吸収 +{gain}"));
                }
            }
        }
        // 憑 (憑依) / 魅 (魅了): 相手の勢力を攻撃側へ奪う支配系属性 (特殊効果攻撃属性.md)。
        // BossRank 適用ユニットには無効。攻撃側自身 (firer_idx==def_idx の自爆等) は対象外。
        if let Some(kind) = possession {
            if !is_boss && firer_idx != def_idx {
                let firer_party = self.database.unit_instances[firer_idx].party;
                let firer_uid = self.database.unit_instances[firer_idx].uid.clone();
                let def = &mut self.database.unit_instances[def_idx];
                match kind {
                    // 憑依: 相手を乗っ取り**恒久支配**。勢力を攻撃側へ移し、SpecialPower の
                    // 使用を封じる condition `憑依` (永続=-1) を付与する。復帰しない
                    // (charm_revert_party は使わない)。
                    "憑依" => {
                        def.party = firer_party;
                        def.add_condition(crate::Condition::new("憑依", -1));
                        applied.push("憑依".to_string());
                    }
                    // 魅了: 相手を **3 ターン魅了**。元勢力を charm_revert_party に退避し
                    // 勢力を攻撃側へ移して「魅了主を護衛する味方」として行動させる
                    // (ai_mode=護衛 <firer_uid>)。condition `魅了` lifetime=4 (3 ターン +
                    // フェイズ開始 tick 吸収) が期限切れになると begin_phase が元勢力へ復帰。
                    "魅了" => {
                        if def.charm_revert_party.is_none() {
                            def.charm_revert_party = Some(def.party);
                        }
                        def.party = firer_party;
                        def.ai_mode = format!("護衛 {firer_uid}");
                        def.add_condition(crate::Condition::new("魅了", 4));
                        applied.push("魅了".to_string());
                    }
                    _ => {}
                }
            }
        }
        applied
    }

    /// 武器 class が防御特性の属性リスト `attr_list` に該当するか
    /// (VB5 `Unit.cls::Weakness/Resist/Absorb` 準拠の字単位部分文字列照合)。
    /// `attr_list` の各属性 (空白は無視) が weapon_class に**部分文字列**として含まれれば該当する
    /// (例: 弱点 "火" は複合 class "格実火" に該当)。`全`=常に該当 / `物`=武器に 魔・精 が無ければ該当。
    /// 旧実装は `split_whitespace` の完全トークン一致で、無空白複合 class ("格実火" 等) が
    /// 弱点 "火" に一致せず防御特性が事実上機能していなかった (VB5 は `InStrNotNest`=部分一致)。
    fn defense_attr_matches(weapon_class: &str, attr_list: &str) -> bool {
        for c in attr_list.chars() {
            if c.is_whitespace() {
                continue;
            }
            match c {
                '全' => return true,
                '物' => {
                    if !weapon_class.contains('魔') && !weapon_class.contains('精') {
                        return true;
                    }
                }
                '魔' => {
                    // 魔武/魔突/魔接/魔銃/魔実 武器には「魔」属性への防御特性が効かない
                    // (spec `防御特性に関する特殊能力.md`「魔法武器には魔法への防御能力が効かない」)。
                    // 逆順 (武魔 等) は対象 = `魔X` 並びのときだけ免除する。
                    const MAGIC_COMBAT: [&str; 5] = ["魔武", "魔突", "魔接", "魔銃", "魔実"];
                    if weapon_class.contains('魔')
                        && !MAGIC_COMBAT.iter().any(|m| weapon_class.contains(m))
                    {
                        return true;
                    }
                }
                _ => {
                    if weapon_class.contains(c) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// 武器属性が防御側の `弱点` (静的特殊能力 + 一時付加 condition `弱点:<属性>`) に一致するか。
    /// 一致時は VB5 `Unit.cls:6949` 準拠で被ダメージ計算の装甲を半減させる
    /// (`attack_resolve_and_run` が予測前に `def_unit.armor` を ÷2 する)。
    fn weapon_hits_weakness(&self, def_idx: usize, weapon_class: &str) -> bool {
        let inst = &self.database.unit_instances[def_idx];
        let weak = crate::feature::feature_value(&inst.active_features, "弱点").unwrap_or("");
        if Self::defense_attr_matches(weapon_class, weak) {
            return true;
        }
        inst.conditions
            .iter()
            .filter_map(|c| c.name.strip_prefix("弱点:"))
            .any(|el| weapon_class.contains(el))
    }

    /// 武器属性が防御側の `吸収` に一致し、かつ `有効` で打ち消されていないか。
    /// 一致時は VB5 `Unit.cls:6951` 準拠で被ダメージ計算の装甲を無視 (arm=0) する
    /// (`attack_resolve_and_run` が予測前に `def_unit.armor=0` にする)。これにより吸収量が
    /// spec「装甲値を無視して計算されたダメージを半減させた値」基準になり、
    /// `defense_attribute_damage` の吸収分岐 `-(damage/2)` が正しい回復量へ反転する。
    fn weapon_hits_absorb(&self, def_idx: usize, weapon_class: &str) -> bool {
        let inst = &self.database.unit_instances[def_idx];
        let absorb = crate::feature::feature_value(&inst.active_features, "吸収").unwrap_or("");
        let effective = crate::feature::feature_value(&inst.active_features, "有効").unwrap_or("");
        Self::defense_attr_matches(weapon_class, absorb)
            && !Self::defense_attr_matches(weapon_class, effective)
    }

    /// 防御特性による装甲調整を予測前に `def_unit.armor` へ適用する (VB5 `Unit.cls:6949-6951`)。
    /// 弱点 → 装甲半減 (被ダメージ増) / 吸収 (かつ非有効) → 装甲無視 (arm=0)。弱点 > 吸収 の優先。
    /// 通常攻撃 / 援護 / 反撃の全経路で一貫適用するための共有ヘルパ。
    pub(crate) fn apply_defense_armor_mod(
        &self,
        def_idx: usize,
        weapon_class: &str,
        def_unit: &mut crate::data::unit::UnitData,
    ) {
        if self.weapon_hits_weakness(def_idx, weapon_class) {
            def_unit.armor /= 2;
        } else if self.weapon_hits_absorb(def_idx, weapon_class) {
            def_unit.armor = 0;
        }
    }

    /// 特殊効果の発動確率を、対象の `耐性` / `弱点` 特殊能力で調整する
    /// (VB5 `Unit.cls::CriticalProbability`: 弱点属性に対しては発動確率 +10、耐性属性には半減)。
    fn adjust_proc_for_resistance(&self, def_idx: usize, weapon_class: &str, prob: i32) -> i32 {
        let inst = &self.database.unit_instances[def_idx];
        let weak = crate::feature::feature_value(&inst.active_features, "弱点").unwrap_or("");
        let resist = crate::feature::feature_value(&inst.active_features, "耐性").unwrap_or("");
        // 弱/効 属性で付加された一時的弱点 (condition `弱点:<属性>`)。
        let added_weak_match = inst
            .conditions
            .iter()
            .filter_map(|c| c.name.strip_prefix("弱点:"))
            .any(|el| weapon_class.contains(el));
        if weak.is_empty() && resist.is_empty() && !added_weak_match {
            return prob;
        }
        let mut p = prob;
        if Self::defense_attr_matches(weapon_class, weak) || added_weak_match {
            p += 10;
        } else if Self::defense_attr_matches(weapon_class, resist) {
            p /= 2;
        }
        p.clamp(1, 100)
    }

    /// 防御特性 (`吸収` / `無効化` / `耐性`) を与ダメージへ適用する (VB5 `Unit.cls::Damage`)。
    /// 武器属性が一致するとき: 吸収→`-damage/2`(回復)、無効化→0、耐性→÷2。優先順は
    /// spec `防御特性に関する特殊能力.md`「弱点 > 有効 > 吸収 > 無効化 > 耐性」。
    /// `弱点` の装甲半減は `weapon_hits_weakness` で予測前に適用済みのためここでは無処理
    /// (`有効` は防御特性を打ち消すため、`弱点`/`有効` 一致時は吸収/無効化/耐性を適用しない)。
    /// 対象が該当特性を持たなければ `damage` をそのまま返す。
    /// 注: 吸収は spec では「装甲無視ダメージ÷2」だが、本実装は最終ダメージ基準の近似 (要精緻化)。
    pub(crate) fn defense_attribute_damage(
        &self,
        def_idx: usize,
        weapon_class: &str,
        damage: i64,
    ) -> i64 {
        let inst = &self.database.unit_instances[def_idx];
        let m = |name: &str| -> bool {
            let list = crate::feature::feature_value(&inst.active_features, name).unwrap_or("");
            Self::defense_attr_matches(weapon_class, list)
        };
        // 弱点 / 有効 が一致するときは後段の防御特性 (吸収/無効化/耐性) を適用しない。
        if m("弱点") || m("有効") {
            return damage;
        }
        if damage > 0 && m("吸収") {
            return -(damage / 2);
        }
        if m("無効化") {
            return 0;
        }
        if m("耐性") {
            return damage / 2;
        }
        damage
    }

    /// ユニットの主パイロットの「切り払い」技能レベル (PilotInstance 優先、無ければ
    /// `PilotData.features` の「切り払いLv<n>」から)。0 = 切り払い不可。
    fn unit_parry_level(&self, idx: usize) -> i32 {
        self.unit_pilot_skill_level(idx, "切り払い")
    }

    /// 主パイロット (`pilot_ids` 先頭、無ければ `pilot_name`) の指定技能レベルを返す。
    /// `PilotInstance` を優先し、無ければ静的 `PilotData.features` から末尾の数字を抽出する
    /// (レベル表記が無ければ 1、技能が無ければ 0)。技能名は部分一致 (`skill_level` 同様)。
    fn unit_pilot_skill_level(&self, idx: usize, skill: &str) -> i32 {
        let u = &self.database.unit_instances[idx];
        let from_inst = u
            .pilot_ids
            .first()
            .and_then(|id| self.database.pilot_instance_by_id(id))
            .map(|p| p.skill_level(skill))
            .unwrap_or(0);
        if from_inst > 0 {
            return from_inst;
        }
        self.database
            .pilot_by_name(&u.pilot_name)
            .and_then(|pd| pd.features.iter().find(|(n, _)| n.contains(skill)))
            .map(|(n, _)| {
                n.rfind(|c: char| c.is_ascii_digit())
                    .and_then(|pos| {
                        let start = n[..=pos]
                            .rfind(|c: char| !c.is_ascii_digit())
                            .map(|i| i + 1)
                            .unwrap_or(0);
                        n[start..=pos].parse::<i32>().ok()
                    })
                    .unwrap_or(1)
            })
            .unwrap_or(0)
    }

    /// 撃破者のメインパイロットの撃墜数を +1 する (SRC `Pilot.Kills`)。撃墜数は
    /// パイロット用特殊能力として扱われ (`撃墜数LvN`)、`(撃墜数Lv20)` 等の必要技能
    /// ゲートが読む。これにより「規定数撃墜でエース専用武器が解禁される」原典の挙動を再現。
    /// 現値 (静的 features ＋ 実体 skills の最大) を基準に +1 し `PilotInstance.skills` へ
    /// `撃墜数 N` 形式で書き戻す (SetSkill と同形式)。
    fn increment_kill_count(&mut self, killer_idx: usize) {
        let cur = self.unit_pilot_skill_level(killer_idx, "撃墜数");
        let Some(pid) = self.database.unit_instances[killer_idx]
            .pilot_ids
            .first()
            .cloned()
        else {
            return;
        };
        if let Some(pi) = self.database.pilot_instance_by_id_mut(&pid) {
            let entry = format!("撃墜数 {}", cur + 1);
            if let Some(pos) = pi.skills.iter().position(|s| s.contains("撃墜数")) {
                pi.skills[pos] = entry;
            } else {
                pi.skills.push(entry);
            }
        }
    }

    /// クリティカル発生率へのパイロット技能補正 (`Unit.cs` クリティカル発生率計算準拠)。
    /// 超反応 (攻撃側 +2×Lv / 防御側 −2×Lv)・超能力 (攻撃側 +5)・底力/超底力/覚悟
    /// (攻撃側 HP が最大の 1/4 以下で +50) の合計を返す。これらの補正は `weapon.critical`
    /// が特殊効果攻撃属性の発動率計算と共有されるため `combat::critical_probability` には
    /// 含めず、命中/クリティカルロール直前に `crit_chance` へ加算する (proc 率への漏れ防止)。
    fn crit_skill_bonus(&self, atk_idx: usize, def_idx: usize) -> i32 {
        let mut bonus = 0;
        // 超反応: 攻撃側 +2×Lv / 防御側 −2×Lv。
        bonus += 2 * self.unit_pilot_skill_level(atk_idx, "超反応");
        bonus -= 2 * self.unit_pilot_skill_level(def_idx, "超反応");
        // 超能力: 攻撃側が持てば +5。
        if self.unit_pilot_skill_level(atk_idx, "超能力") > 0 {
            bonus += 5;
        }
        // 底力 / 超底力 / 覚悟: 攻撃側 HP が最大の 1/4 以下なら +50。
        let atk = &self.database.unit_instances[atk_idx];
        let max_hp = self.database.effective_max_hp(atk);
        if max_hp > 0
            && (max_hp - atk.damage) * 4 <= max_hp
            && (self.unit_pilot_skill_level(atk_idx, "底力") > 0
                || self.unit_pilot_skill_level(atk_idx, "覚悟") > 0)
        {
            bonus += 50;
        }
        bonus
    }

    /// 切り払い判定 (SRC `Unit.cs::CheckParryFeature`)。格闘系 (武/格/突/接) の攻撃を、
    /// 防御側が確率で無効化する。攻撃側が `直撃` を持つときは無効化されない。
    /// 確率: `2×防御側Lv − 攻撃側Lv` が `Dice(1..32)` 以上で発動。防御側が切り払いを
    /// 持たない (Lv0) ときは乱数を消費しない (既存 RNG 列を保つ)。
    /// 防御側が `見切り` (特殊防御発動の SpecialPower) を持つときは確率を 32 (必ず成功) に
    /// 上書きする (`Unit.cs::CheckParryFeature`「見切りがあれば必ず発動」、prob=32)。
    fn roll_parry(&mut self, atk_idx: usize, def_idx: usize, weapon_class: &str) -> bool {
        let is_melee = weapon_class.contains('武')
            || weapon_class.contains('格')
            || weapon_class.contains('突')
            || weapon_class.contains('接');
        if !is_melee {
            return false;
        }
        let def_parry = self.unit_parry_level(def_idx);
        if def_parry <= 0 {
            return false;
        }
        let atk_parry = self.unit_parry_level(atk_idx);
        // 見切り: 切り払いを必ず成功させる (prob=32)。乱数は通常どおり消費して RNG 列を保つ。
        let prob = if self.database.unit_instances[def_idx].has_condition("見切り") {
            32
        } else {
            2 * def_parry - atk_parry
        };
        let dice = (self.next_u32() % 32) as i32 + 1; // 1..=32
        prob >= dice
    }

    /// 即死 (`即`) の発動判定。武器が `即` 属性を持ち対象がボスでないとき、特殊効果
    /// 発動率 (CT率 + 技量差/2、耐性/弱点補正込み) で proc すれば `true` (= 致死化)。
    /// `即` を持たない武器では乱数を消費しない (既存の RNG 列を保つため)。
    fn roll_weapon_instakill(
        &mut self,
        def_idx: usize,
        weapon: &crate::data::unit::WeaponData,
        atk_pilot: &crate::data::pilot::PilotData,
        def_pilot: &crate::data::pilot::PilotData,
    ) -> bool {
        if !weapon.class.split_whitespace().any(|t| t == "即") {
            return false;
        }
        // ボスランク適用ユニットには無効 (BossRankコマンド.md)。
        if self.database.unit_instances[def_idx].is_boss() {
            return false;
        }
        let prob =
            (weapon.critical + (atk_pilot.technique - def_pilot.technique) / 2).clamp(1, 100);
        let prob = self.adjust_proc_for_resistance(def_idx, &weapon.class, prob);
        ((self.next_u32() % 100) as i32) < prob
    }

    /// 減衰系属性 (`衰`=HP / `滅`=EN) をクリティカル時に適用する。対象の現在 HP / EN を
    /// 属性レベルに応じた割合 (Lv1=3/4・Lv2=1/2・Lv3=1/4) に減らす (`特殊効果攻撃属性.md`)。
    /// 減衰は撃破せず (常に 1 以上残す)。適用したラベル列を返す (メッセージ用)。
    fn apply_weapon_crit_decay(
        &mut self,
        def_idx: usize,
        weapon: &crate::data::unit::WeaponData,
    ) -> Vec<String> {
        let (hp_lv, en_lv) = crate::combat::weapon_crit_decay_levels(&weapon.class);
        let is_boss = self.database.unit_instances[def_idx].is_boss();
        // 残す割合 (分子, 分母)。ボスは減少率が半減するため残す割合が増える ((4+keep)/8)。
        let keep_frac = |lv: i32| -> (i64, i64) {
            let keep = crate::combat::crit_decay_keep_numer(lv);
            if is_boss {
                (4 + keep, 8)
            } else {
                (keep, 4)
            }
        };
        let mut applied = Vec::new();
        if let Some(lv) = hp_lv {
            let (num, den) = keep_frac(lv);
            let max_hp = self
                .database
                .effective_max_hp(&self.database.unit_instances[def_idx]);
            let cur_hp = (max_hp - self.database.unit_instances[def_idx].damage).max(1);
            let new_hp = (cur_hp * num / den).max(1);
            self.database.unit_instances[def_idx].damage = max_hp - new_hp;
            applied.push(format!("ＨＰ減衰 (残{new_hp})"));
        }
        if let Some(lv) = en_lv {
            let (num, den) = keep_frac(lv);
            let max_en = self
                .database
                .effective_max_en(&self.database.unit_instances[def_idx]);
            let cur_en = (max_en - self.database.unit_instances[def_idx].en_consumed).max(0);
            let new_en = ((i64::from(cur_en) * num / den) as i32).max(0);
            self.database.unit_instances[def_idx].en_consumed = max_en - new_en;
            applied.push(format!("ＥＮ減衰 (残{new_en})"));
        }
        applied
    }

    /// 吹き飛ばし / ノックバック (`吹` / `Ｋ`): 命中時に対象を攻撃側から見て遠ざかる
    /// 方向へ最大 `マス数` (`critical` 時 +1) だけ押し出す。盤外・占有マスで停止する。
    /// 対象サイズ XL / 移動力 0 では不発 (`特殊効果攻撃属性.md`)。Ｋ は攻撃側サイズが
    /// 標的より 2 段階以上小さいと不発。衝突ダメージは未モデル。押し出したら `true`。
    fn apply_weapon_knockback(
        &mut self,
        def_idx: usize,
        atk_pos: (u32, u32),
        atk_unit_name: &str,
        weapon: &crate::data::unit::WeaponData,
        critical: bool,
    ) -> bool {
        use crate::data::unit::Size;
        let Some((mut tiles, is_k)) = crate::combat::weapon_knockback(&weapon.class) else {
            return false;
        };
        if critical {
            tiles += 1;
        }
        let (tx, ty, def_name) = {
            let u = &self.database.unit_instances[def_idx];
            (u.x, u.y, u.unit_data_name.clone())
        };
        // 対象サイズ XL / 移動力 0 は固定扱いで不発。
        let (def_size, def_speed) = self
            .database
            .unit_by_name(&def_name)
            .map(|d| (d.size, d.speed))
            .unwrap_or((Size::M, 1));
        if def_size == Size::XL || def_speed <= 0 {
            return false;
        }
        // Ｋ: 攻撃側サイズが標的より 2 段階以上小さいと不発。
        if is_k {
            let atk_size = self
                .database
                .unit_by_name(atk_unit_name)
                .map(|d| d.size)
                .unwrap_or(Size::M);
            // rank が大きいほど小さいサイズ。攻撃側 rank - 標的 rank >= 2 で「2段階以上小さい」。
            if atk_size.rank() - def_size.rank() >= 2 {
                return false;
            }
        }
        // 押し出し方向 (攻撃側→対象の優勢軸)。
        let ddx = tx as i32 - atk_pos.0 as i32;
        let ddy = ty as i32 - atk_pos.1 as i32;
        let (sx, sy) = if ddx.abs() >= ddy.abs() {
            (ddx.signum(), 0)
        } else {
            (0, ddy.signum())
        };
        if sx == 0 && sy == 0 {
            return false;
        }
        let (mw, mh) = match self.database.map.as_ref() {
            Some(m) => (m.width as i32, m.height as i32),
            None => return false,
        };
        let uid = self.database.unit_instances[def_idx].uid.clone();
        let (mut cx, mut cy) = (tx as i32, ty as i32);
        let mut moved = false;
        for _ in 0..tiles {
            let (nx, ny) = (cx + sx, cy + sy);
            if nx < 0 || ny < 0 || nx >= mw || ny >= mh {
                break; // 盤外で停止
            }
            if self.database.uid_at(nx as u32, ny as u32).is_some() {
                break; // 他ユニットで停止
            }
            self.database.move_unit(&uid, nx as u32, ny as u32);
            cx = nx;
            cy = ny;
            moved = true;
        }
        moved
    }

    /// 盗 (`盗`): クリティカル時に相手から資金またはアイテムを奪う (`特殊効果攻撃属性.md`)。
    /// 相手が `アイテム所有=<item>` を持てばそのアイテムを (`レアアイテム所有` があれば 1/8 で
    /// 代わりにレアアイテムを) 攻撃側へ移す。持たなければ修理費の 1/4 の資金。攻撃側が味方
    /// (Player) のときのみ。同じ相手からは 1 度だけ (`被盗` 状態で再取得を抑止)。
    fn apply_weapon_crit_steal(
        &mut self,
        def_idx: usize,
        atk_idx: usize,
        atk_party: crate::Party,
        victim_value: i64,
        weapon: &crate::data::unit::WeaponData,
    ) -> Vec<String> {
        if !weapon.class.split_whitespace().any(|t| t == "盗") {
            return Vec::new();
        }
        // 撃破側が味方 (Player) のときのみ。既に盗んだ相手からは再取得しない。
        if atk_party != crate::Party::Player
            || self.database.unit_instances[def_idx].has_condition("被盗")
        {
            return Vec::new();
        }
        // アイテム所有 / レアアイテム所有 (相手が持つ盗めるアイテム)。
        let feats = &self.database.unit_instances[def_idx].active_features;
        let rare = crate::feature::feature_value(feats, "レアアイテム所有")
            .map(str::to_string)
            .filter(|s| !s.is_empty());
        let normal = crate::feature::feature_value(feats, "アイテム所有")
            .map(str::to_string)
            .filter(|s| !s.is_empty());
        // レアアイテム所有があれば 1/8 で代わりにレアを盗む (`&&` 短絡で非保有時は RNG 不消費)。
        let stolen_item = if rare.is_some() && self.next_u32() % 8 == 0 {
            rare
        } else {
            normal
        };

        if let Some(item) = stolen_item {
            self.database.unit_instances[atk_idx]
                .equipped_items
                .push(item.clone());
            self.database.unit_instances[def_idx].add_condition(crate::Condition::new("被盗", -1));
            return vec![format!("アイテム奪取: {item}")];
        }
        let amount = (victim_value / 4).max(0);
        if amount <= 0 {
            return Vec::new();
        }
        self.add_money(amount);
        self.database.unit_instances[def_idx].add_condition(crate::Condition::new("被盗", -1));
        vec![format!("資金奪取 +{amount}")]
    }

    /// 写 / 化 (能力コピー): クリティカル時に発動者を対象のユニットへ変化させる
    /// (`特殊効果攻撃属性.md`)。`写` はサイズ 2 段階以上差で無効、`化` は制限なし。
    /// パイロットは `set_unit_form` が保持する。適用したラベル列を返す。
    fn apply_weapon_crit_copy(
        &mut self,
        atk_idx: usize,
        def_idx: usize,
        weapon: &crate::data::unit::WeaponData,
    ) -> Vec<String> {
        use crate::data::unit::Size;
        let has_sha = weapon.class.split_whitespace().any(|t| t == "写");
        let has_ka = weapon.class.split_whitespace().any(|t| t == "化");
        if !has_sha && !has_ka {
            return Vec::new();
        }
        let atk_name = self.database.unit_instances[atk_idx].unit_data_name.clone();
        let def_name = self.database.unit_instances[def_idx].unit_data_name.clone();
        if def_name == atk_name {
            return Vec::new();
        }
        // 写 (化 でない) はサイズ 2 段階以上差で無効。
        if has_sha && !has_ka {
            let asize = self
                .database
                .unit_by_name(&atk_name)
                .map(|d| d.size)
                .unwrap_or(Size::M);
            let dsize = self
                .database
                .unit_by_name(&def_name)
                .map(|d| d.size)
                .unwrap_or(Size::M);
            if asize.step_diff(dsize) >= 2 {
                return Vec::new();
            }
        }
        let atk_uid = self.database.unit_instances[atk_idx].uid.clone();
        if self.set_unit_form(&atk_uid, &def_name) {
            vec![format!("能力コピー → {def_name}")]
        } else {
            Vec::new()
        }
    }

    /// ラーニング技 (`習`): クリティカル時に対象 (`def_idx`) が持つユニット用特殊能力
    /// `ラーニング可能技=<技名称> [表示]` を攻撃側 (`atk_idx`) のメインパイロットが習得する
    /// (`特殊効果攻撃属性.md` 習 / `その他の特殊能力 ラーニング可能技`)。習得した技は
    /// レベル無しのパイロット用特殊能力として `PilotInstance.skills` に追加され、`(技名称)`
    /// を必要技能とする武器/アビリティのゲートが解禁される。
    ///
    /// `習` は class 内で他属性と連結されうる (`無習` 等) ため文字単位で検出する。
    /// 既に保有 (静的/習得済) の技は再習得しない。適用メッセージを返す。
    fn apply_weapon_learning(
        &mut self,
        atk_idx: usize,
        def_idx: usize,
        weapon: &crate::data::unit::WeaponData,
    ) -> Vec<String> {
        if !weapon.class.contains('習') {
            return Vec::new();
        }
        // 対象ユニットの「ラーニング可能技」を列挙 (value の先頭トークンが技名称、
        // 続く "表示" は表示フラグ。本実装は技名のみ使用)。
        let def_name = self.database.unit_instances[def_idx].unit_data_name.clone();
        let learnable: Vec<String> = self
            .database
            .unit_by_name(&def_name)
            .map(|d| {
                d.features
                    .iter()
                    .filter(|(n, _)| n == "ラーニング可能技")
                    .filter_map(|(_, v)| v.split_whitespace().next())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        if learnable.is_empty() {
            return Vec::new();
        }
        let Some(pid) = self.database.unit_instances[atk_idx]
            .pilot_ids
            .first()
            .cloned()
        else {
            return Vec::new();
        };
        let mut msgs = Vec::new();
        for skill in learnable {
            // 既に保有 (静的 features / 習得済 skills) していれば再習得しない。
            if self.unit_pilot_skill_level(atk_idx, &skill) > 0 {
                continue;
            }
            if let Some(pi) = self.database.pilot_instance_by_id_mut(&pid) {
                pi.skills.push(skill.clone());
                msgs.push(format!("ラーニング！ {skill} を習得"));
            }
        }
        msgs
    }

    /// 引き寄せ / 強制転移 (`引` / `転`): クリティカル時に対象の位置を移す
    /// (`特殊効果攻撃属性.md`)。`引`=攻撃側に隣接する空きマスへ、`転`=現在地から
    /// 属性レベル距離内のランダムな空きマスへ。対象 XL / 移動力 0 では不発。
    /// 適用したラベル列を返す (メッセージ用)。
    fn apply_weapon_crit_reposition(
        &mut self,
        def_idx: usize,
        atk_pos: (u32, u32),
        weapon: &crate::data::unit::WeaponData,
    ) -> Vec<String> {
        use crate::data::unit::Size;
        let (pull, teleport) = crate::combat::weapon_crit_reposition(&weapon.class);
        if !pull && teleport.is_none() {
            return Vec::new();
        }
        let def_name = self.database.unit_instances[def_idx].unit_data_name.clone();
        let (def_size, def_speed) = self
            .database
            .unit_by_name(&def_name)
            .map(|d| (d.size, d.speed))
            .unwrap_or((Size::M, 1));
        if def_size == Size::XL || def_speed <= 0 {
            return Vec::new();
        }
        let uid = self.database.unit_instances[def_idx].uid.clone();
        let mut applied = Vec::new();
        // 引き寄せ: 攻撃側の隣接空きマスへ。
        if pull {
            if let Some((nx, ny)) = self.find_empty_adjacent_tile(atk_pos) {
                self.database.move_unit(&uid, nx, ny);
                applied.push("引き寄せ".to_string());
            }
        }
        // 強制転移: 現在地から dist マス内のランダムな空きマスへ。
        if let Some(dist) = teleport {
            let (px, py) = {
                let u = &self.database.unit_instances[def_idx];
                (u.x as i32, u.y as i32)
            };
            let (mw, mh) = match self.database.map.as_ref() {
                Some(m) => (m.width as i32, m.height as i32),
                None => return applied,
            };
            let mut candidates: Vec<(u32, u32)> = Vec::new();
            for dx in -dist..=dist {
                for dy in -dist..=dist {
                    if dx == 0 && dy == 0 || dx.abs() + dy.abs() > dist {
                        continue;
                    }
                    let (nx, ny) = (px + dx, py + dy);
                    if nx < 0 || ny < 0 || nx >= mw || ny >= mh {
                        continue;
                    }
                    if self.database.uid_at(nx as u32, ny as u32).is_none() {
                        candidates.push((nx as u32, ny as u32));
                    }
                }
            }
            if !candidates.is_empty() {
                let pick = (self.next_u32() as usize) % candidates.len();
                let (nx, ny) = candidates[pick];
                self.database.move_unit(&uid, nx, ny);
                applied.push("強制転移".to_string());
            }
        }
        applied
    }

    /// 撃破報酬を撃破側ユニット (`killer_idx`) に付与する。`exp` は 努力 反映済みの最終
    /// 経験値。`UnitInstance.total_exp` への加算に加え、メインパイロットの `PilotInstance`
    /// を成長させ、レベルが上がれば `レベルアップ / LevelUp` イベントを発火する。資金は
    /// `victim_value / 2` を基準に、撃破側の「獲得資金増加」技能で +10%/Lv、「幸運」で
    /// 2 倍 (消費) して `App.money` に加算する。獲得資金額を返す (0 なら無し)。
    /// `idx` のユニットの主パイロット (静的 `PilotData`) の性格を返す。
    fn main_pilot_personality(&self, idx: usize) -> Option<String> {
        self.database
            .unit_instances
            .get(idx)
            .and_then(|u| self.database.pilot_by_name(&u.pilot_name))
            .and_then(|p| p.personality.clone())
    }

    /// 主パイロットの性格が `機械` か (気力が変動しない)。
    fn main_pilot_is_mecha(&self, idx: usize) -> bool {
        self.main_pilot_personality(idx).as_deref() == Some("機械")
    }

    /// 撃墜による気力変動 (`Unit.cs`)。撃破陣営: 撃破者トータル +4 (機械は +3)、同陣営の他
    /// 出撃中ユニット +1 (機械は不動)。被撃破陣営: 性格別 (超強気 +2 / 強気 +1 / 弱気 -1 /
    /// その他・機械 0)。マップ攻撃の撃破は `award_kill_rewards` を経由しないため対象外
    /// (原典どおり)。`victim_party` は撃破された側の陣営。
    fn apply_kill_morale(&mut self, killer_idx: usize, victim_party: crate::Party) {
        let killer_party = self.database.unit_instances[killer_idx].party;
        for i in 0..self.database.unit_instances.len() {
            if self.database.unit_instances[i].off_map {
                continue;
            }
            let party = self.database.unit_instances[i].party;
            let delta = if party == killer_party {
                // 撃破者本人: +3 (常時) ＋ 陣営加算 +1 (機械以外) = 機械 +3 / それ以外 +4。
                // 同陣営の他ユニット: +1 (機械以外)。
                let mecha = self.main_pilot_is_mecha(i);
                if i == killer_idx {
                    if mecha {
                        3
                    } else {
                        4
                    }
                } else if mecha {
                    0
                } else {
                    1
                }
            } else if party == victim_party {
                // 被撃破陣営: 性格別 (機械・その他は default 0)。
                match self.main_pilot_personality(i).as_deref() {
                    Some("超強気") => 2,
                    Some("強気") => 1,
                    Some("弱気") => -1,
                    _ => 0,
                }
            } else {
                0
            };
            if delta != 0 {
                let m = &mut self.database.unit_instances[i];
                m.morale = (m.morale + delta).clamp(0, 150);
            }
        }
    }

    /// `unit_idx` が `wd` (武器データ) を今発射できるか (残弾・EN・必要気力)。実行時資源を見る。
    /// 該当 UnitWeapon が無い (未 populate) 場合は弾無制限とみなす (= 既存テストへ無影響)。
    /// 武器の必要技能 / 必要条件 ((念力Lv3) 形式の括弧条件) を満たすか。資源 (EN/弾/
    /// 気力) は判定しない使用「資格」のみのゲート。強制武器選択時にも必須。
    fn weapon_skill_ok(&self, unit_idx: usize, wd: &crate::data::unit::WeaponData) -> bool {
        let u = &self.database.unit_instances[unit_idx];
        crate::necessary_skill::weapon_usable_by(wd, u, &self.database)
    }

    fn weapon_firable(&self, unit_idx: usize, wd: &crate::data::unit::WeaponData) -> bool {
        // 必要技能 / 必要条件 (使用資格)。
        if !self.weapon_skill_ok(unit_idx, wd) {
            return false;
        }
        let u = &self.database.unit_instances[unit_idx];
        // 必要気力。
        if wd.necessary_morale > 0 && u.morale < wd.necessary_morale {
            return false;
        }
        // EN。
        let max_en = self.database.effective_max_en(u);
        if max_en - u.en_consumed < wd.en_consumption {
            return false;
        }
        // 残弾 (有限弾のみ)。武器を名前で unit_data → index 解決し、該当 UnitWeapon を見る。
        if let Some(di) = self
            .database
            .unit_by_name(&u.unit_data_name)
            .and_then(|d| d.weapons.iter().position(|w| w.name == wd.name))
        {
            if let Some(uw) = u.weapons.iter().find(|w| w.weapon_index == di) {
                if !uw.has_ammo() {
                    return false;
                }
            }
        }
        true
    }

    /// 射程内かつ発射可能 (残弾/EN/気力) な最高威力武器を選ぶ。`combat::best_weapon_in_range`
    /// の実行時資源を考慮する版 (反撃 / 援護 / AI 自動選択用)。チャージ武器は除外。
    fn best_firable_weapon_in_range(
        &self,
        unit_idx: usize,
        unit: &crate::data::unit::UnitData,
        dist: u32,
    ) -> Option<crate::data::unit::WeaponData> {
        unit.weapons
            .iter()
            .filter(|w| combat::weapon_in_range(w, dist))
            .filter(|w| !combat::is_charge_weapon(w))
            .filter(|w| self.weapon_firable(unit_idx, w))
            .max_by_key(|w| w.power)
            .cloned()
    }

    /// 武器発射時の資源消費 (EN・残弾)。`Unit.cs` 同様、命中の有無に関わらず発射で消費する。
    /// `weapon_di` は `unit_data.weapons` でのインデックス (= `UnitWeapon.weapon_index`)。
    /// EN コスト 0・無限弾 (bullet ≤ 0) の武器は実質無消費 (= テスト用武器へ無影響)。
    pub(crate) fn consume_weapon_resources(&mut self, unit_idx: usize, weapon_di: usize) {
        let (en_cost, has_ammo) = {
            let u = &self.database.unit_instances[unit_idx];
            match self
                .database
                .unit_by_name(&u.unit_data_name)
                .and_then(|d| d.weapons.get(weapon_di))
            {
                Some(wd) => (wd.en_consumption, wd.bullet > 0),
                None => return,
            }
        };
        let u = &mut self.database.unit_instances[unit_idx];
        if en_cost > 0 {
            u.en_consumed += en_cost;
        }
        if has_ammo {
            if let Some(uw) = u.weapons.iter_mut().find(|w| w.weapon_index == weapon_di) {
                uw.consume_bullet();
            }
        }
    }

    /// 命中/損傷/失敗/回避 時の気力増加スキルを適用する (`Unit.cs` 17078-17138)。
    /// `hit=true`: 攻撃側に 命中時気力増加 / 被弾側に 損傷時気力増加 をスキルLv分加算。
    /// `hit=false`: 攻撃側に 失敗時気力増加 / 回避側に 回避時気力増加 を加算。テスト用
    /// ユニットは該当スキルを持たないため無影響 (= 既存テストへ影響なし)。
    fn apply_combat_morale_skills(&mut self, atk_idx: usize, def_idx: usize, hit: bool) {
        let (atk_skill, def_skill) = if hit {
            ("命中時気力増加", "損傷時気力増加")
        } else {
            ("失敗時気力増加", "回避時気力増加")
        };
        let atk_lv = self.unit_pilot_skill_level(atk_idx, atk_skill);
        if atk_lv > 0 {
            let m = &mut self.database.unit_instances[atk_idx];
            m.morale = (m.morale + atk_lv).clamp(0, 150);
        }
        let def_lv = self.unit_pilot_skill_level(def_idx, def_skill);
        if def_lv > 0 {
            let m = &mut self.database.unit_instances[def_idx];
            m.morale = (m.morale + def_lv).clamp(0, 150);
        }
    }

    fn award_kill_rewards(
        &mut self,
        killer_idx: usize,
        exp: i32,
        victim_value: i64,
        victim_party: crate::Party,
    ) -> i64 {
        // 撃墜による気力変動 (撃破者 +4 / 同陣営 +1、被撃破陣営は性格別、機械は不動)。
        self.apply_kill_morale(killer_idx, victim_party);
        // 撃墜数 +1 (撃破者メインパイロット)。撃墜数Lv* の必要技能ゲートが読む。
        self.increment_kill_count(killer_idx);
        // 経験値: UnitInstance.total_exp + メインパイロットの PilotInstance 成長。
        let old_level = crate::pilot_instance::level_from_exp(
            self.database.unit_instances[killer_idx].total_exp,
        );
        self.database.unit_instances[killer_idx].total_exp += exp;
        let new_level = crate::pilot_instance::level_from_exp(
            self.database.unit_instances[killer_idx].total_exp,
        );
        self.database.unit_instances[killer_idx].remove_condition("努力");
        let pilot_ids = self.database.unit_instances[killer_idx].pilot_ids.clone();
        for pilot_id in &pilot_ids {
            let pdata = self
                .database
                .pilot_instance_by_id(pilot_id)
                .map(|p| p.pilot_data_name.clone())
                .and_then(|n| self.database.pilot_by_name(&n).cloned());
            if let Some(pdata) = pdata {
                if let Some(pi) = self.database.pilot_instance_by_id_mut(pilot_id) {
                    if pi.add_exp(exp) {
                        pi.apply_stat_growth(&pdata);
                    }
                }
            }
        }
        // 資金: 撃破側が味方 (Player) のときのみ獲得する。敵/中立 の撃破で味方の
        // 資金プールが増えるのは誤り (実機報告: 敵が味方を撃破して「資金 +N」)。
        // victim_value/2 を基準に 獲得資金増加 (+10%/Lv) と 幸運 (×2, 消費) を適用。
        let mut money = if self.database.unit_instances[killer_idx].party == crate::Party::Player {
            (victim_value / 2).max(0)
        } else {
            0
        };
        let gain_lv = pilot_ids
            .iter()
            .filter_map(|id| self.database.pilot_instance_by_id(id))
            .map(|p| p.skill_level("獲得資金増加"))
            .max()
            .unwrap_or(0);
        if gain_lv > 0 {
            money += money * i64::from(gain_lv) / 10;
        }
        if self.database.unit_instances[killer_idx].has_condition("幸運") {
            money *= 2;
            self.database.unit_instances[killer_idx].remove_condition("幸運");
        }
        if money > 0 {
            self.add_money(money);
        }
        // レベルアップイベント (メインパイロットのレベルが上がった場合のみ発火)。
        if new_level > old_level {
            let (pn, un, party) = {
                let u = &self.database.unit_instances[killer_idx];
                (u.pilot_name.clone(), u.unit_data_name.clone(), u.party)
            };
            crate::event_runtime::fire_unit_event_labels_public(
                self,
                &["レベルアップ", "LevelUp"],
                &pn,
                &un,
                party,
            );
        }
        money
    }

    /// 撃破以外の行動 (修理 / 補給 等) の経験値付与。`total_exp` 加算 + メイン
    /// パイロット成長 + レベルアップイベント (資金は無し)。
    fn award_support_exp(&mut self, unit_idx: usize, exp: i32) {
        if exp <= 0 {
            return;
        }
        let old_level =
            crate::pilot_instance::level_from_exp(self.database.unit_instances[unit_idx].total_exp);
        self.database.unit_instances[unit_idx].total_exp += exp;
        let new_level =
            crate::pilot_instance::level_from_exp(self.database.unit_instances[unit_idx].total_exp);
        let pilot_ids = self.database.unit_instances[unit_idx].pilot_ids.clone();
        for pilot_id in &pilot_ids {
            let pdata = self
                .database
                .pilot_instance_by_id(pilot_id)
                .map(|p| p.pilot_data_name.clone())
                .and_then(|n| self.database.pilot_by_name(&n).cloned());
            if let Some(pdata) = pdata {
                if let Some(pi) = self.database.pilot_instance_by_id_mut(pilot_id) {
                    if pi.add_exp(exp) {
                        pi.apply_stat_growth(&pdata);
                    }
                }
            }
        }
        if new_level > old_level {
            let (pn, un, party) = {
                let u = &self.database.unit_instances[unit_idx];
                (u.pilot_name.clone(), u.unit_data_name.clone(), u.party)
            };
            crate::event_runtime::fire_unit_event_labels_public(
                self,
                &["レベルアップ", "LevelUp"],
                &pn,
                &un,
                party,
            );
        }
    }

    /// `target` の EN を全快し、全武器の残弾を初期値へ戻す (補給 / 瞑想)。
    fn spirit_resupply(&mut self, uid: &str) {
        let Some(idx) = self.database.idx_by_uid(uid) else {
            return;
        };
        // ゾンビは EN 回復を受けられない (弾薬補給は可)。
        let en_blocked = self.recovery_blocked(uid);
        let unit_name = self.database.unit_instances[idx].unit_data_name.clone();
        let max_bullets: Vec<i32> = self
            .database
            .unit_by_name(&unit_name)
            .map(|d| d.weapons.iter().map(|w| w.bullet).collect())
            .unwrap_or_default();
        let u = &mut self.database.unit_instances[idx];
        if !en_blocked {
            u.en_consumed = 0;
        }
        for w in &mut u.weapons {
            if let Some(b) = max_bullets.get(w.weapon_index) {
                w.reset_ammo(*b);
            }
        }
    }

    /// 対象選択が要る精神コマンドの対象選択モードを開始する。メニュー / サブ
    /// メニューを閉じ、`ActionMode::SpiritTarget` へ遷移する。SP の消費は対象
    /// 確定時 (`apply_spirit_to_target`) に行う。
    fn begin_spirit_target(
        &mut self,
        caster: &str,
        spirit: &str,
        cost: i32,
        kind: SpiritTargetKind,
    ) {
        use crate::command_menu::ActionMode;
        self.pending_dialog = None;
        self.command_menu = None;
        let target_enemy = kind == SpiritTargetKind::SingleEnemy;
        self.action_mode = ActionMode::SpiritTarget {
            caster: caster.to_string(),
            spirit: spirit.to_string(),
            cost,
            target_enemy,
        };
        let who = if target_enemy { "敵" } else { "味方" };
        self.push_message(format!(
            "精神【{spirit}】対象の{who}ユニットを選択 (右クリックでキャンセル)"
        ));
    }

    /// 精神コマンドの対象が確定したときに SP を消費し効果を適用する。
    /// 精神コマンド `name` が `イベント=<routine>` 効果を持つならサブルーチン名を返す
    /// (自作SP `生贄`→`生贄ルーチン`)。SRC `SpecialPowerData.cls` Execute の "イベント"。
    fn spirit_event_routine(&self, name: &str) -> Option<String> {
        self.database
            .special_powers
            .iter()
            .find(|s| s.name == name)
            .and_then(|s| s.event_routine())
            .map(|s| s.to_string())
    }

    fn apply_spirit_to_target(&mut self, caster: &str, target: &str, spirit: &str, cost: i32) {
        self.consume_unit_sp(caster, cost);
        self.apply_spirit_effect(target, spirit);
        // イベント= SP効果種別 (自作SP): 対象/相手ユニットＩＤ を束縛してサブルーチンを実行
        // (SRC `SpecialPowerData.cls` Execute の "イベント" 効果)。`生贄`→`生贄ルーチン` が
        // `SpecialPower 相手ユニットＩＤ みがわり 対象ユニットＩＤ` で選んだ味方を身代わりにする。
        // 対象ユニットＩＤ = 使用者, 相手ユニットＩＤ = 選んだ対象 (combat の束縛と同じ向き)。
        if let Some(routine) = self.spirit_event_routine(spirit) {
            self.set_script_var("対象ユニットＩＤ".to_string(), caster.to_string());
            self.set_script_var("相手ユニットＩＤ".to_string(), target.to_string());
            crate::event_runtime::trigger_label(self, &routine);
        }
        let target_nick = self
            .database
            .unit_by_uid(target)
            .map(|u| u.pilot_name.clone())
            .unwrap_or_default();
        self.push_message(format!(
            "精神コマンド【{spirit}】→ {target_nick} (SP -{cost})"
        ));
    }

    /// 修理 / 補給 (特殊能力 `修理装置` / `補給装置`) の対象になり得る隣接味方
    /// ユニットの uid 一覧を返す。原典 SRC: 対象は隣接 (距離1) の味方/ＮＰＣで、
    /// 修理は HP 減、補給は EN または残弾が減っているユニットに限る。
    fn support_target_uids(
        &self,
        caster_uid: &str,
        kind: crate::command_menu::SupportKind,
    ) -> Vec<String> {
        let Some(c) = self.database.unit_by_uid(caster_uid) else {
            return Vec::new();
        };
        if c.off_map {
            return Vec::new();
        }
        let cpos = (c.x, c.y);
        let cparty = c.party;
        self.database
            .unit_instances
            .iter()
            .filter(|t| {
                !t.off_map
                    && t.uid != caster_uid
                    && cparty.is_ally_of(t.party)
                    && combat::manhattan(cpos, (t.x, t.y)) == 1
                    && self.support_needs(t, kind)
            })
            .map(|t| t.uid.clone())
            .collect()
    }

    /// `t` が `kind` の支援を必要としているか (修理=HP 減 / 補給=EN または残弾減)。
    fn support_needs(
        &self,
        t: &crate::UnitInstance,
        kind: crate::command_menu::SupportKind,
    ) -> bool {
        use crate::command_menu::SupportKind;
        match kind {
            SupportKind::Repair => t.damage > 0,
            SupportKind::Supply => {
                if t.en_consumed > 0 {
                    return true;
                }
                let Some(d) = self.database.unit_by_name(&t.unit_data_name) else {
                    return false;
                };
                t.weapons.iter().any(|w| {
                    d.weapons
                        .get(w.weapon_index)
                        .is_some_and(|wd| wd.bullet >= 0 && w.bullet_remaining < wd.bullet)
                })
            }
        }
    }

    /// 修理 / 補給 コマンドの対象選択モードを開始する。メニューを閉じ
    /// `ActionMode::SupportTarget` へ遷移する。効果適用は対象確定時。
    fn begin_support_target(&mut self, caster: &str, kind: crate::command_menu::SupportKind) {
        use crate::command_menu::ActionMode;
        self.pending_dialog = None;
        self.command_menu = None;
        self.action_mode = ActionMode::SupportTarget {
            caster: caster.to_string(),
            kind,
        };
        self.push_message(format!(
            "{} 対象の隣接ユニットを選択 (右クリックでキャンセル)",
            kind.label()
        ));
    }

    /// 修理 / 補給 の対象が確定したときに効果を適用する。
    /// 修理: HP 全回復 / 補給: EN・残弾 全回復 + 対象の気力 -10 (原典準拠)。
    fn apply_support_to_target(
        &mut self,
        caster: &str,
        target: &str,
        kind: crate::command_menu::SupportKind,
    ) {
        use crate::command_menu::SupportKind;
        let tnick = self
            .database
            .unit_by_uid(target)
            .map(|u| u.pilot_name.clone())
            .unwrap_or_default();
        match kind {
            SupportKind::Repair => {
                // 修理装置 Lv 別回復率 (回復系特殊能力.md): なし/1=30% / 2=50% / 3+=100%。
                let lv = self
                    .database
                    .unit_by_uid(caster)
                    .and_then(|u| crate::feature::feature_level(&u.active_features, "修理装置"))
                    .unwrap_or(1);
                let pct: i64 = match lv {
                    ..=1 => 30,
                    2 => 50,
                    _ => 100,
                };
                // ゾンビ状態の対象は能動的な HP 回復を受けられない。
                let blocked = self.recovery_blocked(target);
                if let Some(idx) = self.database.idx_by_uid(target).filter(|_| !blocked) {
                    let max_hp = self
                        .database
                        .effective_max_hp(&self.database.unit_instances[idx]);
                    let heal = max_hp * pct / 100;
                    let u = &mut self.database.unit_instances[idx];
                    u.damage = (u.damage - heal).max(0);
                }
                self.push_message(format!("修理 → {tnick} (HP +{pct}%)"));
            }
            SupportKind::Supply => {
                self.spirit_resupply(target);
                self.add_unit_morale(target, -10);
                self.push_message(format!("補給 → {tnick} (EN・残弾 補給 / 気力 -10)"));
            }
        }
        // 修理 / 補給 を行った側は経験値を得る。獲得量は対象パイロットのレベルが
        // 高いほど多い (SRC: 相手のレベルが高いほど多い)。基準値は SRC 原典準拠で
        // 修理=100 / 補給=150 (exp スケールが 500/level になったため原典値へ復帰)。
        if let (Some(ci), Some(ti)) = (
            self.database.idx_by_uid(caster),
            self.database.idx_by_uid(target),
        ) {
            let base = match kind {
                SupportKind::Repair => 100,
                SupportKind::Supply => 150,
            };
            let actor_lv = self.unit_pilot_level(&self.database.unit_instances[ci]);
            let target_lv = self.unit_pilot_level(&self.database.unit_instances[ti]);
            let xp = support_exp_with_level_diff(base, target_lv, actor_lv);
            self.award_support_exp(ci, xp);
        }
    }

    /// ユニット `u` の特殊能力 `変形` から (表示ラベル, 変形先フォーム列) を返す。
    /// 原典書式: `変形=<変形の名称> <変形先1> <変形先2> …`。第 1 トークンが
    /// コマンド表示名、以降が変形先。DB に存在する形態のみ採用し、1 つも無ければ
    /// `None`（メニューに出さない）。
    fn transform_forms(&self, u: &crate::UnitInstance) -> Option<(String, Vec<String>)> {
        let val = crate::feature::feature_value(&u.active_features, "変形")?;
        let mut toks = val.split_whitespace();
        let label = toks.next()?.to_string();
        let forms: Vec<String> = toks
            .filter(|f| self.database.unit_by_name(f).is_some())
            .map(|f| f.to_string())
            .collect();
        if forms.is_empty() {
            return None;
        }
        Some((label, forms))
    }

    /// 変形コマンドを開く。変形先が 1 つなら即変形、複数ならサブメニューを出す。
    fn open_transform_menu(&mut self, uid: &str) {
        let info = self.database.unit_by_uid(uid).and_then(|u| {
            self.transform_forms(u)
                .map(|(_label, forms)| (u.pilot_name.clone(), forms))
        });
        let Some((nick, forms)) = info else {
            self.push_message("変形できる形態がありません".to_string());
            self.reopen_unit_menu_for(uid);
            return;
        };
        if forms.len() == 1 {
            // 変形先が 1 つ → 即変形。発動後もユニットメニューへ戻る（行動非消費）。
            let form = forms[0].clone();
            self.transform_unit_by_uid(uid, &form);
            self.reopen_unit_menu_for(uid);
            return;
        }
        // 複数形態はサブメニューで選択させる。
        self.pending_transform = Some(PendingTransform {
            uid: uid.to_string(),
            forms: forms.clone(),
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: format!("{nick} 変形先を選択"),
            options: forms,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            non_cancellable: false,
        });
    }

    /// 変形先サブメニューの選択を解決する。`choice` は 1-based。0 / 範囲外は
    /// キャンセルでユニットメニューへ戻る。
    /// 形態 (`form_name`) の必要技能 / 不必要技能 (`UnitData` の `必要技能=` / `不必要技能=`
    /// 特殊能力、`必要技能.md` §4) を `unit_idx` の搭乗員が満たすか。変形 / 換装 のゲート。
    /// 形態側にこれらの宣言が無ければ常に許可 (= 制限なし)。`必要技能` は満たさないと不可、
    /// `不必要技能` は満たすと不可 (原典: ちょうど逆)。乗り換え / 合体 のクロス搭乗評価は別途。
    fn form_skill_ok(&self, unit_idx: usize, form_name: &str) -> bool {
        let Some(ud) = self.database.unit_by_name(form_name) else {
            return true;
        };
        let reqs: Vec<(bool, String)> = ud
            .features
            .iter()
            .filter_map(|(n, v)| match n.as_str() {
                "必要技能" => Some((true, v.clone())),
                "不必要技能" => Some((false, v.clone())),
                _ => None,
            })
            .collect();
        if reqs.is_empty() {
            return true;
        }
        let u = &self.database.unit_instances[unit_idx];
        for (require, expr) in &reqs {
            let satisfied = crate::necessary_skill::is_satisfied(expr, u, &self.database);
            if *require != satisfied {
                return false;
            }
        }
        true
    }

    fn resolve_transform(&mut self, choice: u32) -> bool {
        let Some(pt) = self.pending_transform.take() else {
            return false;
        };
        self.pending_dialog = None;
        let idx = choice as usize;
        if idx == 0 || idx > pt.forms.len() {
            self.reopen_unit_menu_for(&pt.uid);
            return true;
        }
        let form = pt.forms[idx - 1].clone();
        // 形態の必要技能 (搭乗員が満たさないと変形不可)。
        if let Some(uidx) = self.database.idx_by_uid(&pt.uid) {
            if !self.form_skill_ok(uidx, &form) {
                self.push_message(format!("必要技能を満たさないため {form} に変形できません"));
                self.reopen_unit_menu_for(&pt.uid);
                return true;
            }
        }
        self.transform_unit_by_uid(&pt.uid, &form);
        self.reopen_unit_menu_for(&pt.uid);
        true
    }

    /// ユニット `uid` の形態 (`unit_data_name`) を `new_form` に差し替え、新形態の
    /// `active_features` / `abilities` を反映する（イベント発火・メッセージ無し）。
    /// 変形 / 換装 の共通コア。形態が存在し差し替えたら `true`。
    fn set_unit_form(&mut self, uid: &str, new_form: &str) -> bool {
        let Some(idx) = self.database.idx_by_uid(uid) else {
            return false;
        };
        let (raw_feats, abils) = self
            .database
            .unit_by_name(new_form)
            .map(|ud| {
                let feats = ud.features.clone();
                let abils = ud
                    .abilities
                    .iter()
                    .map(|a| {
                        let mut ua =
                            crate::unit_ability::UnitAbility::new(a.name.clone(), a.effect.clone());
                        ua.stock_remaining = a.uses;
                        ua
                    })
                    .collect::<Vec<_>>();
                (feats, abils)
            })
            .unwrap_or_default();
        self.database.unit_instances[idx].unit_data_name = new_form.to_string();
        // §3 必要技能ゲート込みで新形態の特殊能力を有効化する。
        let feats = crate::necessary_skill::gated_active_features(
            &raw_feats,
            &self.database.unit_instances[idx],
            &self.database,
        );
        let u = &mut self.database.unit_instances[idx];
        u.active_features = feats;
        u.abilities = abils;
        true
    }

    /// ユニット `uid` を `new_form` へ変形させる。`変形`/`Transform` イベントを
    /// 発火する（`.eve Transform` と同じ本体処理）。行動は消費しない。
    fn transform_unit_by_uid(&mut self, uid: &str, new_form: &str) {
        if !self.set_unit_form(uid, new_form) {
            return;
        }
        let Some(idx) = self.database.idx_by_uid(uid) else {
            return;
        };
        let (pn, un, party) = {
            let u = &self.database.unit_instances[idx];
            (u.pilot_name.clone(), u.unit_data_name.clone(), u.party)
        };
        self.push_message(format!("{pn} 変形 → {new_form}"));
        crate::event_runtime::fire_unit_event_labels_public(
            self,
            &["変形", "Transform"],
            &pn,
            &un,
            party,
        );
    }

    // ───────────────────────── アビリティ ─────────────────────────

    /// `caster` の `idx` 番目のアビリティ静的データ (`UnitData.abilities`) を返す。
    fn ability_at(&self, caster_uid: &str, idx: usize) -> Option<crate::data::unit::AbilityData> {
        let u = self.database.unit_by_uid(caster_uid)?;
        let d = self.database.unit_by_name(&u.unit_data_name)?;
        d.abilities.get(idx).cloned()
    }

    /// アビリティが使用可能か (回数 / EN / 気力)。`アビリティ.md` の × 判定。
    fn ability_usable(&self, caster_uid: &str, idx: usize) -> bool {
        let Some(u) = self.database.unit_by_uid(caster_uid) else {
            return false;
        };
        let Some(ab) = self.ability_at(caster_uid, idx) else {
            return false;
        };
        // 回数切れ (stock_remaining=Some(0))。無制限 (None) は常に可。
        if let Some(stock) = u.abilities.get(idx).and_then(|ua| ua.stock_remaining) {
            if stock <= 0 {
                return false;
            }
        }
        // EN 不足。
        if let Some(en) = ab.en_cost {
            let cur_en = self.database.effective_max_en(u) - u.en_consumed;
            if cur_en < en {
                return false;
            }
        }
        // 気力不足。
        if let Some(m) = ab.morale {
            if u.morale < m {
                return false;
            }
        }
        // 沈黙 (特殊効果攻撃属性 黙): 術 / 音 属性のアビリティは使用不能。
        if u.has_condition("沈黙") && (ab.attributes.contains('術') || ab.attributes.contains('音'))
        {
            return false;
        }
        // 必要技能 / 必要条件 (属性末尾の括弧条件)。武器同様、満たさないと使用不可。
        let (_class, ns, nc) = crate::necessary_skill::split_necessary(&ab.attributes);
        if !ns.is_empty() && !crate::necessary_skill::is_satisfied(&ns, u, &self.database) {
            return false;
        }
        if !nc.is_empty() && !crate::necessary_skill::is_satisfied(&nc, u, &self.database) {
            return false;
        }
        true
    }

    /// アビリティ一覧サブメニューを開く。使用不可なものは先頭に `×` を付ける。
    fn open_ability_menu(&mut self, uid: &str) {
        let info: Option<(String, Vec<(usize, String)>)> =
            self.database.unit_by_uid(uid).and_then(|u| {
                let d = self.database.unit_by_name(&u.unit_data_name)?;
                if d.abilities.is_empty() {
                    return None;
                }
                let list = d
                    .abilities
                    .iter()
                    .enumerate()
                    .map(|(i, a)| (i, a.display_name().to_string()))
                    .collect();
                Some((u.pilot_name.clone(), list))
            });
        let Some((nick, list)) = info else {
            self.push_message("アビリティがありません".to_string());
            self.reopen_unit_menu_for(uid);
            return;
        };
        let mut entries = Vec::new();
        let mut options = Vec::new();
        for (idx, name) in list {
            let usable = self.ability_usable(uid, idx);
            options.push(format!("{}{name}", if usable { "" } else { "×" }));
            entries.push((idx, usable));
        }
        self.pending_ability = Some(PendingAbility {
            uid: uid.to_string(),
            entries,
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: format!("{nick} アビリティ"),
            options,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            non_cancellable: false,
        });
    }

    /// アビリティ一覧サブメニューの選択を解決する。`choice` は 1-based。
    fn resolve_ability(&mut self, choice: u32) -> bool {
        let Some(pa) = self.pending_ability.take() else {
            return false;
        };
        self.pending_dialog = None;
        let i = choice as usize;
        if i == 0 || i > pa.entries.len() {
            self.reopen_unit_menu_for(&pa.uid);
            return true;
        }
        let (ability_idx, usable) = pa.entries[i - 1];
        if !usable {
            self.push_message("そのアビリティは使用できません (回数 / EN / 気力)".to_string());
            self.reopen_unit_menu_for(&pa.uid);
            return true;
        }
        let Some(ab) = self.ability_at(&pa.uid, ability_idx) else {
            self.reopen_unit_menu_for(&pa.uid);
            return true;
        };
        if ab.is_map_type() {
            // マップ型 (Ｍ全/Ｍ投/…): 座標選択を省略し射程内の全有効対象へ即適用。
            let caster = pa.uid.clone();
            self.apply_ability(&caster, ability_idx, &caster);
            self.finish_ability(&caster);
        } else if ab.is_self_only() {
            // 射程0: 自分に即適用 (召喚も射程0)。
            let caster = pa.uid.clone();
            self.apply_ability(&caster, ability_idx, &caster);
            self.finish_ability(&caster);
        } else {
            self.begin_ability_target(&pa.uid, ability_idx);
        }
        true
    }

    /// 射程≥1 アビリティの対象選択モードを開始する。
    fn begin_ability_target(&mut self, caster: &str, ability_idx: usize) {
        use crate::command_menu::ActionMode;
        self.pending_dialog = None;
        self.command_menu = None;
        self.action_mode = ActionMode::AbilityTarget {
            caster: caster.to_string(),
            ability_idx,
        };
        let name = self
            .ability_at(caster, ability_idx)
            .map(|a| a.display_name().to_string())
            .unwrap_or_default();
        self.push_message(format!(
            "アビリティ【{name}】対象を選択 (右クリックでキャンセル)"
        ));
    }

    /// アビリティの対象が有効か (射程内・盤上、対象勢力、`援`/サイズ制限)。
    /// 対象勢力は属性で決まる: `脱`/`除` (敵対象) は敵、それ以外は味方。
    fn ability_target_valid(&self, caster: &str, idx: usize, target: &str) -> bool {
        let Some(ab) = self.ability_at(caster, idx) else {
            return false;
        };
        let (Some(c), Some(t)) = (
            self.database.unit_by_uid(caster),
            self.database.unit_by_uid(target),
        ) else {
            return false;
        };
        if t.off_map {
            return false;
        }
        if ab.attributes.contains('援') && target == caster {
            return false; // 援: 自分に使用不可
        }
        // 対象勢力: 脱/除 アビリティは敵、それ以外は味方。
        if ab.targets_enemy() {
            if !c.party.is_hostile_to(t.party) {
                return false;
            }
        } else if !c.party.is_ally_of(t.party) {
            return false;
        }
        // 能力コピー: 自分以外の味方が対象。サイズ制限 (既定で 2 段階以上差は不可、
        // 追加設定 サイズ制限無し/強 で変化) を満たす相手のみ。
        if ab.has_copy_effect() {
            if target == caster {
                return false;
            }
            if !self.copy_size_ok(c, t, &ab) {
                return false;
            }
        }
        combat::manhattan((c.x, c.y), (t.x, t.y)) <= ab.range.max(0) as u32
    }

    /// `能力コピー` のサイズ制限判定。既定は 2 段階以上のサイズ差を禁止
    /// (`アビリティ効果.md`)。`サイズ制限無し` で無制限、`サイズ制限強` で同サイズのみ。
    fn copy_size_ok(
        &self,
        caster: &crate::UnitInstance,
        target: &crate::UnitInstance,
        ab: &crate::data::unit::AbilityData,
    ) -> bool {
        if ab.effect.contains("サイズ制限無し") {
            return true;
        }
        let (Some(cd), Some(td)) = (
            self.database.unit_by_name(&caster.unit_data_name),
            self.database.unit_by_name(&target.unit_data_name),
        ) else {
            return true;
        };
        if ab.effect.contains("サイズ制限強") {
            cd.size == td.size
        } else {
            cd.size.step_diff(td.size) < 2
        }
    }

    /// アビリティを発動: 回数 / EN を消費し効果を `target` に適用する。
    /// 行動消費 (has_acted) は適用前に true をセットする (`再行動` 効果が解除しうる)。
    fn apply_ability(&mut self, caster: &str, ability_idx: usize, target: &str) {
        let Some(ab) = self.ability_at(caster, ability_idx) else {
            return;
        };
        // 回数を消費 (有限のみ)。
        if let Some(ci) = self.database.idx_by_uid(caster) {
            if let Some(ua) = self.database.unit_instances[ci]
                .abilities
                .get_mut(ability_idx)
            {
                if let Some(s) = ua.stock_remaining.as_mut() {
                    *s = (*s - 1).max(0);
                }
            }
        }
        // EN を消費。
        if let Some(en) = ab.en_cost {
            if let Some(u) = self.database.unit_by_uid_mut(caster) {
                u.en_consumed += en;
            }
        }
        // 行動消費を先に確定 (`再行動` 効果が後で解除しうる)。
        if let Some(u) = self.database.unit_by_uid_mut(caster) {
            u.has_acted = true;
        }
        if ab.is_map_type() {
            // マップ型: 射程内の全有効対象へ効果と属性を適用する。
            self.apply_ability_area(caster, &ab);
        } else {
            self.apply_ability_effects(caster, target, &ab.effect);
            self.apply_ability_attributes(target, &ab);
            let tnick = self
                .database
                .unit_by_uid(target)
                .map(|u| u.pilot_name.clone())
                .unwrap_or_default();
            self.push_message(format!("アビリティ【{}】→ {tnick}", ab.display_name()));
        }
    }

    /// マップ型アビリティの効果を、射程内 (`Ｍ全` は盤上全体) の全有効対象へ
    /// 適用する。対象勢力は属性で決まる (脱/除=敵、それ以外=味方)。
    fn apply_ability_area(&mut self, caster: &str, ab: &crate::data::unit::AbilityData) {
        let Some(c) = self.database.unit_by_uid(caster) else {
            return;
        };
        let (cx, cy, cparty) = (c.x, c.y, c.party);
        let range = ab.range.max(0) as u32;
        let targets: Vec<String> = self
            .database
            .unit_instances
            .iter()
            .filter(|u| !u.off_map)
            .filter(|u| {
                if ab.targets_enemy() {
                    cparty.is_hostile_to(u.party)
                } else {
                    cparty.is_ally_of(u.party)
                }
            })
            // 援: 自分自身は対象外。
            .filter(|u| !(ab.attributes.contains('援') && u.uid == caster))
            // Ｍ全 は射程・座標を無視。それ以外は発動者からの射程内。
            .filter(|u| ab.is_map_all() || combat::manhattan((cx, cy), (u.x, u.y)) <= range)
            .map(|u| u.uid.clone())
            .collect();
        for t in &targets {
            self.apply_ability_effects(caster, t, &ab.effect);
            self.apply_ability_attributes(t, ab);
        }
        self.push_message(format!(
            "アビリティ【{}】→ {} 体",
            ab.display_name(),
            targets.len()
        ));
    }

    /// アビリティ属性 (`脱`=気力低下 / `除`=特殊効果解除) を対象へ適用する。
    /// いずれも発動確率 100% (`ユニットデータ.md` アビリティ属性)。
    fn apply_ability_attributes(&mut self, target: &str, ab: &crate::data::unit::AbilityData) {
        if ab.attributes.contains('脱') {
            self.add_unit_morale(target, -10);
        }
        if ab.attributes.contains('除') {
            // 相手にかかっているアビリティ由来の特殊効果を解除。MVP: 状態異常を
            // 全消去 (アビリティ付与分とそれ以外を区別しない簡易版)。
            if let Some(u) = self.database.unit_by_uid_mut(target) {
                u.conditions.clear();
            }
        }
    }

    /// アビリティ発動後の後処理: 行動が残っていればメニュー再表示、消費済みなら
    /// 行動終了イベントを発火してメニューを閉じる。
    fn finish_ability(&mut self, caster: &str) {
        use crate::command_menu::ActionMode;
        self.action_mode = ActionMode::Browse;
        let acted = self
            .database
            .unit_by_uid(caster)
            .map(|u| u.has_acted)
            .unwrap_or(false);
        if acted {
            if let Some(i) = self.database.idx_by_uid(caster) {
                crate::event_runtime::fire_action_end_labels(self, i);
                crate::event_runtime::fire_contact_event_labels(self, i);
            }
            self.command_menu = None;
        } else {
            // 再行動等で行動が戻った → 連続使用できるようメニューを再表示。
            self.reopen_unit_menu_for(caster);
        }
    }

    /// アビリティ効果文字列 (`回復Lv2 治癒` 等、半角スペース区切り) を適用する。
    /// `アビリティ効果.md` の主要効果に対応。未対応効果は無視 (無害)。
    /// `caster` は発動者 (`能力コピー` で発動者自身が変化するため必要)。
    fn apply_ability_effects(&mut self, caster: &str, target: &str, effect: &str) {
        // ゾンビ状態の対象は能動的な HP/EN 回復 (回復/補給) を受けられない。
        let recovery_blocked = self.recovery_blocked(target);
        for tok in effect.split_whitespace() {
            let (head, arg) = match tok.split_once('=') {
                Some((h, a)) => (h, Some(a.trim_matches('"'))),
                None => (tok, None),
            };
            let (base, lv) = split_effect_level(head);
            match base {
                // 回復: HP 500×Lv。ゾンビ対象は回復不可。
                "回復" => {
                    if let Some(ci) = self
                        .database
                        .idx_by_uid(target)
                        .filter(|_| !recovery_blocked)
                    {
                        let heal = i64::from((500 * lv).max(0));
                        let u = &mut self.database.unit_instances[ci];
                        u.damage = (u.damage - heal).max(0);
                    }
                }
                // 補給: EN 50×Lv。ゾンビ対象は回復不可。
                "補給" => {
                    if let Some(ci) = self
                        .database
                        .idx_by_uid(target)
                        .filter(|_| !recovery_blocked)
                    {
                        let u = &mut self.database.unit_instances[ci];
                        u.en_consumed = (u.en_consumed - (50 * lv).max(0)).max(0);
                    }
                }
                // 気力増加: 10×Lv。
                "気力増加" => self.add_unit_morale(target, 10 * lv),
                // 治癒: 状態異常回復 (引数指定なら該当のみ)。
                "治癒" => {
                    if let Some(u) = self.database.unit_by_uid_mut(target) {
                        match arg {
                            Some(names) => {
                                for n in names.split_whitespace() {
                                    u.remove_condition(n);
                                }
                            }
                            None => u.conditions.clear(),
                        }
                    }
                }
                // 装填: 残弾を最大まで回復 (EN は戻さない)。
                "装填" => self.ability_reload(target),
                // 再行動: 行動済みを解除。
                "再行動" => self.reset_unit_action(target),
                // 霊力回復 / ＳＰ回復: メインパイロットの SP を 10×Lv 回復。
                "霊力回復" | "ＳＰ回復" => self.restore_unit_sp(target, 10 * lv),
                // 変身: 対象を別フォームへ (set_unit_form を変形/換装と共有)。
                // 注: 持続ターン (Lv) の自動解除は未モデル (永続変身扱い)。
                "変身" => {
                    if let Some(form) = arg {
                        self.set_unit_form(target, form);
                    }
                }
                // 状態 / 付加: 特殊状態・特殊能力を付与。Lv 指定で持続ターン、無指定で永続。
                "状態" | "付加" => {
                    if let Some(name) = arg {
                        let lifetime = if head.contains("Lv") { lv } else { -1 };
                        self.add_unit_condition(target, name, lifetime);
                    }
                }
                // 召喚: 指定ユニットを Lv 体、対象 (=発動者) の隣接空きマスへ生成する。
                "召喚" => {
                    if let Some(unit_name) = arg {
                        for _ in 0..lv.max(1) {
                            if !self.summon_unit_adjacent(target, unit_name) {
                                break; // 空きマスが尽きたら打ち切り
                            }
                        }
                    }
                }
                // 強化: メインパイロットの特殊能力レベルを一定時間増加。MVP では
                // 指定能力を一時的な状態 (condition) として対象へ付与する (付加と同じ
                // 機構で名前ベースに参照可能化。既存能力へのレベル加算は未モデル)。
                "強化" => {
                    if let Some(name) = arg {
                        let lifetime = if head.contains("Lv") { lv } else { -1 };
                        self.add_unit_condition(target, name, lifetime);
                    }
                }
                // 能力コピー: 発動者自身を対象 (射程内味方) のユニットへ変化させる。
                // 対象のユニットデータをコピー (サイズ制限は target 選択時に担保)。
                // パイロット能力は変化しない (set_unit_form は pilot を保持)。
                "能力コピー" => {
                    let form = self
                        .database
                        .unit_by_uid(target)
                        .map(|u| u.unit_data_name.clone());
                    if let Some(form) = form {
                        if caster != target {
                            self.set_unit_form(caster, &form);
                        }
                    }
                }
                // 解説: 効果なし (表示用)。
                "解説" => {}
                _ => {}
            }
        }
    }

    /// 装填アビリティ: 対象の全武器の残弾を最大値へ戻す (EN は変えない)。
    fn ability_reload(&mut self, target: &str) {
        let Some(idx) = self.database.idx_by_uid(target) else {
            return;
        };
        let unit_name = self.database.unit_instances[idx].unit_data_name.clone();
        let max_bullets: Vec<i32> = self
            .database
            .unit_by_name(&unit_name)
            .map(|d| d.weapons.iter().map(|w| w.bullet).collect())
            .unwrap_or_default();
        let u = &mut self.database.unit_instances[idx];
        for w in &mut u.weapons {
            if let Some(b) = max_bullets.get(w.weapon_index) {
                w.bullet_remaining = *b;
            }
        }
    }

    /// 召喚アビリティ: `parent` の隣接空きマスに `unit_name` のユニットを生成する。
    /// 同陣営・無人で生成し、`summoned_by=parent` を記録 (親破壊で消滅させるため)。
    /// 空きマスが無い / ユニット定義が無いときは `false`。
    fn summon_unit_adjacent(&mut self, parent_uid: &str, unit_name: &str) -> bool {
        if self.database.unit_by_name(unit_name).is_none() {
            return false;
        }
        let Some((ppos, party)) = self
            .database
            .unit_by_uid(parent_uid)
            .map(|u| ((u.x, u.y), u.party))
        else {
            return false;
        };
        let Some((tx, ty)) = self.find_empty_adjacent_tile(ppos) else {
            return false;
        };
        let mut inst = crate::UnitInstance::new(unit_name, String::new(), party, tx, ty);
        inst.summoned_by = Some(parent_uid.to_string());
        let uid = self.database.register_unit(inst);
        // 召喚ユニットの active_features / abilities を UnitData から初期化。
        self.set_unit_form(&uid, unit_name);
        self.push_message(format!("{unit_name} を召喚！"));
        true
    }

    // ───────────────────────── 母艦 / 発進 ─────────────────────────

    /// 発進サブメニューを開く。母艦が格納しているユニットを一覧表示する。
    fn open_launch_menu(&mut self, carrier_uid: &str) {
        let stored: Vec<String> = self
            .database
            .unit_by_uid(carrier_uid)
            .map(|u| u.stored_units.clone())
            .unwrap_or_default();
        if stored.is_empty() {
            self.push_message("格納しているユニットがありません".to_string());
            self.reopen_unit_menu_for(carrier_uid);
            return;
        }
        let options: Vec<String> = stored
            .iter()
            .map(|uid| {
                self.database
                    .unit_by_uid(uid)
                    .map(|u| {
                        let unit = self.unit_display_name(u);
                        let pilot = if u.pilot_name.is_empty() {
                            "無人".to_string()
                        } else {
                            u.pilot_name.clone()
                        };
                        format!("{unit} ({pilot})")
                    })
                    .unwrap_or_else(|| uid.clone())
            })
            .collect();
        self.pending_launch = Some(PendingLaunch {
            carrier: carrier_uid.to_string(),
            stored,
        });
        self.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: "発進させるユニットを選択".to_string(),
            options,
            var_name: String::new(),
            store_value: false,
            option_keys: Vec::new(),
            non_cancellable: false,
        });
    }

    /// 発進サブメニューの選択を解決する。`choice` は 1-based。発進は行動を消費しない。
    fn resolve_launch(&mut self, choice: u32) -> bool {
        let Some(pl) = self.pending_launch.take() else {
            return false;
        };
        self.pending_dialog = None;
        let i = choice as usize;
        if i == 0 || i > pl.stored.len() {
            self.reopen_unit_menu_for(&pl.carrier);
            return true;
        }
        let stored = pl.stored[i - 1].clone();
        // 着地点選択 (原典): 母艦位置を起点とする移動範囲の空きマスから対話選択する。
        // 候補があればクリック選択へ移行し、無ければ従来の隣接自動配置にフォールバック。
        if let Some(cpos) = self.database.unit_by_uid(&pl.carrier).map(|u| (u.x, u.y)) {
            let candidates = self.landing_candidates(&stored, cpos);
            if !candidates.is_empty() {
                self.map_cursor = Some(cpos);
                self.action_mode = crate::command_menu::ActionMode::LandingSelect {
                    uid: stored,
                    candidates,
                    carrier: Some(pl.carrier.clone()),
                };
                return true;
            }
        }
        if self.launch_unit_from_carrier(&pl.carrier, &stored) {
            let nick = self
                .database
                .unit_by_uid(&stored)
                .map(|u| u.pilot_name.clone())
                .unwrap_or_default();
            self.push_message(format!("{nick} 発進！"));
        }
        // 発進は母艦の行動を消費しないのでメニューを再表示。
        self.reopen_unit_menu_for(&pl.carrier);
        true
    }

    /// 着地点選択 (`LandingSelect`) で候補マス `(tx, ty)` を確定する。ユニットを配置し、
    /// 母艦経路なら母艦メニューを再表示する。候補外なら何もしない (`false`)。
    fn confirm_landing(&mut self, tx: u32, ty: u32) -> bool {
        let crate::command_menu::ActionMode::LandingSelect {
            uid,
            candidates,
            carrier,
        } = self.action_mode.clone()
        else {
            return false;
        };
        if !candidates.contains(&(tx, ty)) {
            return false; // 候補外クリックは無反応 (キャンセルは右クリック)。
        }
        if let Some(carrier) = carrier {
            self.place_launched_unit_at(&carrier, &uid, tx, ty);
            let nick = self
                .database
                .unit_by_uid(&uid)
                .map(|u| u.pilot_name.clone())
                .unwrap_or_default();
            self.push_message(format!("{nick} 発進！"));
            self.action_mode = crate::command_menu::ActionMode::Browse;
            self.reopen_unit_menu_for(&carrier);
        } else {
            // 分離経路 (carrier なし): 構成ユニットを盤上へ配置し、キューの次へ進む
            // (残りが無ければ Browse へ戻し host メニューを再表示)。
            self.place_separated_unit_at(&uid, tx, ty);
            self.advance_landing_queue();
        }
        true
    }

    /// 母艦 `carrier` から格納ユニット `stored` を出撃させる。母艦の隣接空きマスに
    /// 配置し、格納リンクを解除する。空きマスが無ければ `false`。
    /// (スクリプト/AI 経路・着地点選択候補が無いときのフォールバック。プレイヤーの
    /// メニュー経路は `resolve_launch` が移動範囲からの着地点選択 (`LandingSelect`) を挟む。)
    fn launch_unit_from_carrier(&mut self, carrier: &str, stored: &str) -> bool {
        let Some(cpos) = self.database.unit_by_uid(carrier).map(|u| (u.x, u.y)) else {
            return false;
        };
        let Some((tx, ty)) = self.find_empty_adjacent_tile(cpos) else {
            self.push_message("発進できる空きマスがありません".to_string());
            return false;
        };
        self.place_launched_unit_at(carrier, stored, tx, ty);
        true
    }

    /// 格納ユニット `stored` を `(tx, ty)` へ配置し、格納リンクを解除する。
    /// `launch_unit_from_carrier` (隣接自動) と `LandingSelect` (移動範囲から選択) の共通配置。
    fn place_launched_unit_at(&mut self, carrier: &str, stored: &str, tx: u32, ty: u32) {
        // off_map ユニットは move_unit で座標だけ更新し、set_off_map(false) で
        // pos_index に載せる (`.eve Launch` と同順)。
        self.database.move_unit(stored, tx, ty);
        self.database.set_off_map(stored, false);
        if let Some(u) = self.database.unit_by_uid_mut(stored) {
            u.life_state = String::new();
            u.stored_in = None;
            u.has_acted = false;
            u.has_moved = false;
        }
        if let Some(c) = self.database.unit_by_uid_mut(carrier) {
            c.stored_units.retain(|s| s != stored);
        }
    }

    /// 発進/分離ユニット `uid` が `source` (母艦/合体元の位置) から着地可能なマス一覧。
    /// ユニットの移動範囲内 (Dijkstra) かつ空き (他ユニット不在) のマス。`source` が母艦で
    /// 占有されていても起点としては有効 (そこから移動して空きマスへ着地する)。
    fn landing_candidates(&self, uid: &str, source: (u32, u32)) -> Vec<(u32, u32)> {
        self.database
            .unit_move_range_from(uid, source)
            .into_keys()
            .filter(|&(x, y)| self.database.uid_at(x, y).is_none())
            .collect()
    }

    /// 分離の構成ユニット `uid` を `(tx, ty)` へ盤上配置する (off_map 解除 + 状態リセット)。
    fn place_separated_unit_at(&mut self, uid: &str, tx: u32, ty: u32) {
        self.database.move_unit(uid, tx, ty);
        self.database.set_off_map(uid, false);
        if let Some(u) = self.database.unit_by_uid_mut(uid) {
            u.life_state = String::new();
            u.has_acted = false;
            u.has_moved = false;
        }
    }

    /// 分離の構成ユニット列 `units` を `source` (host 位置) 起点で順次着地選択させる。
    /// 最初に着地候補のあるユニットで `LandingSelect` へ移行し残りを `landing_queue` へ積む。
    /// 候補の無いユニットは隣接自動配置でスキップ。全機 auto-place / 候補皆無なら `false`
    /// (対話に入らない＝呼出側が従来どおりメニュー再表示する)。
    fn begin_landing_queue(&mut self, units: Vec<String>, source: (u32, u32)) -> bool {
        self.landing_source = Some(source);
        let mut remaining: std::collections::VecDeque<String> = units.into();
        while let Some(uid) = remaining.pop_front() {
            let candidates = self.landing_candidates(&uid, source);
            if candidates.is_empty() {
                if let Some((tx, ty)) = self.find_empty_adjacent_tile(source) {
                    self.place_separated_unit_at(&uid, tx, ty);
                }
                continue;
            }
            self.landing_queue = remaining.into_iter().collect();
            self.action_mode = crate::command_menu::ActionMode::LandingSelect {
                uid,
                candidates,
                carrier: None,
            };
            return true;
        }
        self.landing_source = None;
        false
    }

    /// 分離キューの次のユニットへ進む。候補があれば `LandingSelect`、無ければ隣接自動で
    /// スキップ。キューが尽きたら `finish_landing_queue` で Browse へ戻し host メニューを再表示。
    fn advance_landing_queue(&mut self) {
        let source = self.landing_source.unwrap_or((0, 0));
        while !self.landing_queue.is_empty() {
            let uid = self.landing_queue.remove(0);
            let candidates = self.landing_candidates(&uid, source);
            if candidates.is_empty() {
                if let Some((tx, ty)) = self.find_empty_adjacent_tile(source) {
                    self.place_separated_unit_at(&uid, tx, ty);
                }
                continue;
            }
            self.action_mode = crate::command_menu::ActionMode::LandingSelect {
                uid,
                candidates,
                carrier: None,
            };
            return;
        }
        self.finish_landing_queue();
    }

    /// 分離キュー完了: Browse へ戻し、host のユニットメニューを再表示する。
    fn finish_landing_queue(&mut self) {
        self.action_mode = crate::command_menu::ActionMode::Browse;
        self.landing_source = None;
        if let Some(host) = self.landing_menu_uid.take() {
            self.reopen_unit_menu_for(&host);
        }
    }

    /// `pos` の 8 近傍からユニットの居ない盤内マスを 1 つ返す。発進の着地点用。
    fn find_empty_adjacent_tile(&self, pos: (u32, u32)) -> Option<(u32, u32)> {
        let (w, h) = match self.database.map.as_ref() {
            Some(m) => (m.width, m.height),
            None => return None,
        };
        const DELTAS: [(i32, i32); 8] = [
            (0, -1),
            (0, 1),
            (-1, 0),
            (1, 0),
            (-1, -1),
            (1, -1),
            (-1, 1),
            (1, 1),
        ];
        for (dx, dy) in DELTAS {
            let nx = pos.0 as i32 + dx;
            let ny = pos.1 as i32 + dy;
            if nx < 0 || ny < 0 || nx as u32 >= w || ny as u32 >= h {
                continue;
            }
            let (tx, ty) = (nx as u32, ny as u32);
            if self.database.uid_at(tx, ty).is_none() {
                return Some((tx, ty));
            }
        }
        None
    }

    // ───────────────────────── 合体 / 分離 ─────────────────────────

    /// `host` の `合体` 特殊能力 (`合体=<名称> <合体形態> <相手1> <相手2> …`) を解析し、
    /// 合体可能なら (合体形態, 合体相手 uid 列) を返す。原典 (合体.md): 合体に参加する
    /// ユニット全てが 2 マス以内に居る場合にのみ合体可能なので、列挙された各パートナー
    /// 形態がそれぞれ 2 マス以内に揃っていなければ `None` を返す。
    fn combine_partners(&self, host_uid: &str) -> Option<(String, Vec<String>)> {
        let host = self.database.unit_by_uid(host_uid)?;
        let val = crate::feature::feature_value(&host.active_features, "合体")?;
        let toks: Vec<&str> = val.split_whitespace().collect();
        if toks.len() < 3 {
            return None; // 名称 + 合体形態 + 相手 が最低限必要
        }
        let combined_form = toks[1].to_string();
        // 合体形態が DB に存在しなければ不可。
        self.database.unit_by_name(&combined_form)?;
        let hpos = (host.x, host.y);
        let hparty = host.party;
        // 列挙されたパートナー形態ごとに、2 マス以内・同陣営・盤上の味方を 1 機ずつ探す。
        // 1 形態でも揃わなければ合体不可。
        let mut partners: Vec<String> = Vec::new();
        for pform in &toks[2..] {
            let found = self.database.unit_instances.iter().find(|u| {
                !u.off_map
                    && u.uid != host_uid
                    && u.party == hparty
                    && u.unit_data_name.as_str() == *pform
                    && !partners.contains(&u.uid)
                    && combat::manhattan(hpos, (u.x, u.y)) <= 2
            });
            partners.push(found?.uid.clone());
        }
        Some((combined_form, partners))
    }

    /// `host` の `合体` 特殊能力に列挙されたパートナー形態数を返す (合体能力が無ければ 0)。
    /// 1 = 2 体合体 (相手ユニット上への移動で合体)、2 以上 = 3 体以上合体 (合体コマンドで合体)。
    fn combine_partner_count(&self, host_uid: &str) -> usize {
        self.database
            .unit_by_uid(host_uid)
            .and_then(|h| crate::feature::feature_value(&h.active_features, "合体"))
            .map(|v| v.split_whitespace().skip(2).count())
            .unwrap_or(0)
    }

    /// 合体を実行: 2 マス以内の合体相手を温存 (off_map) して合体形態へ変身する。
    /// 発動で行動終了 (原典準拠)。構成ユニットは `分離` で復帰できる。
    fn apply_combine(&mut self, host_uid: &str) {
        let Some((combined_form, partners)) = self.combine_partners(host_uid) else {
            return;
        };
        // 合体形態の必要技能 (`必要技能.md` §4) を構成全員のパイロットで判定する。満たさなければ
        // 合体しない。判定は merged パイロットを host へ一時設定して評価し、即座に復元する
        // (以降の本処理が merged を再計算するため、ここでは破壊しない非破壊チェック)。
        if let Some(host_idx) = self.database.idx_by_uid(host_uid) {
            let mut merged = self.database.unit_instances[host_idx].pilot_ids.clone();
            for p in &partners {
                if let Some(pu) = self.database.unit_by_uid(p) {
                    for pid in &pu.pilot_ids {
                        if !merged.contains(pid) {
                            merged.push(pid.clone());
                        }
                    }
                }
            }
            let saved = std::mem::replace(
                &mut self.database.unit_instances[host_idx].pilot_ids,
                merged,
            );
            let ok = self.form_skill_ok(host_idx, &combined_form);
            self.database.unit_instances[host_idx].pilot_ids = saved;
            if !ok {
                self.push_message(format!(
                    "必要技能を満たさないため {combined_form} に合体できません"
                ));
                return;
            }
        }
        // 構成ユニット (相手) を温存: off_map + life_state="合体"。
        for p in &partners {
            self.database.set_off_map(p, true);
            if let Some(u) = self.database.unit_by_uid_mut(p) {
                u.life_state = "合体".to_string();
            }
        }
        // host の合体前形態を記録し、合体形態へ変身。
        let pre_form = self
            .database
            .unit_by_uid(host_uid)
            .map(|u| u.unit_data_name.clone());
        // パイロット統合 (全員搭乗): host の元搭乗者を温存しつつ、構成ユニットの
        // パイロットを合体形態へ集約する。構成ユニット側の pilot_ids は温存され
        // (off_map のまま)、分離時に各自へ戻る。
        let host_pilots = self
            .database
            .unit_by_uid(host_uid)
            .map(|u| u.pilot_ids.clone())
            .unwrap_or_default();
        let mut merged_pilots = host_pilots.clone();
        for p in &partners {
            if let Some(pu) = self.database.unit_by_uid(p) {
                for pid in &pu.pilot_ids {
                    if !merged_pilots.contains(pid) {
                        merged_pilots.push(pid.clone());
                    }
                }
            }
        }
        self.set_unit_form(host_uid, &combined_form);
        if let Some(u) = self.database.unit_by_uid_mut(host_uid) {
            u.pre_combine_form = pre_form;
            u.pre_combine_pilots = host_pilots;
            u.pilot_ids = merged_pilots;
            u.combined_from = partners.clone();
            u.has_acted = true;
        }
        self.push_message(format!(
            "合体！ → {combined_form} (構成 {} 機)",
            partners.len() + 1
        ));
        if let Some(i) = self.database.idx_by_uid(host_uid) {
            let (pn, un, party) = {
                let u = &self.database.unit_instances[i];
                (u.pilot_name.clone(), u.unit_data_name.clone(), u.party)
            };
            crate::event_runtime::fire_unit_event_labels_public(
                self,
                &["合体", "Combine"],
                &pn,
                &un,
                party,
            );
            if let Some(i2) = self.database.idx_by_uid(host_uid) {
                crate::event_runtime::fire_action_end_labels(self, i2);
            }
        }
        self.action_mode = crate::command_menu::ActionMode::Browse;
        self.command_menu = None;
    }

    /// 分離を実行する。行動は消費しない (原典準拠)。2 つの経路がある:
    /// - **実行時合体** (`combined_from` 非空): 温存していた構成ユニットを隣接へ復帰し、
    ///   host を合体前形態へ戻す。
    /// - **最初から合体形態** (`combined_from` 空で `分離` 特殊能力あり): `分離` 特殊能力の
    ///   形態へ分離し、形態 2 以降を新規生成する。
    fn apply_separate(&mut self, host_uid: &str) {
        let components = self
            .database
            .unit_by_uid(host_uid)
            .map(|u| u.combined_from.clone())
            .unwrap_or_default();
        let split_forms: Vec<String> = self
            .database
            .unit_by_uid(host_uid)
            .and_then(|u| crate::feature::feature_value(&u.active_features, "分離"))
            // `分離=<名称> <形態1> <形態2> …` の先頭 (名称) を除いた形態列。
            .map(|v| {
                v.split_whitespace()
                    .skip(1)
                    .map(|s| s.to_string())
                    .collect()
            })
            .unwrap_or_default();

        let staged = if !components.is_empty() {
            self.separate_preserved_components(host_uid, &components)
        } else if !split_forms.is_empty() {
            self.separate_into_forms(host_uid, &split_forms)
        } else {
            return; // 分離できない
        };

        self.push_message("分離！".to_string());
        if let Some(i) = self.database.idx_by_uid(host_uid) {
            let (pn, un, party) = {
                let u = &self.database.unit_instances[i];
                (u.pilot_name.clone(), u.unit_data_name.clone(), u.party)
            };
            crate::event_runtime::fire_unit_event_labels_public(
                self,
                &["分離", "Split"],
                &pn,
                &un,
                party,
            );
        }
        // 構成ユニットの着地点を移動範囲から対話選択させる (host 位置を起点)。
        // 候補があればキュー化し、完了後に host メニューを再表示。候補が無い (狭所/map 無し)
        // ときは begin_landing_queue 内で隣接自動配置し false を返すので従来どおりメニュー再表示。
        if let Some(hpos) = self.database.unit_by_uid(host_uid).map(|u| (u.x, u.y)) {
            self.landing_menu_uid = Some(host_uid.to_string());
            if self.begin_landing_queue(staged, hpos) {
                return; // 対話配置中 (完了で finish_landing_queue がメニュー再表示)。
            }
        }
        self.landing_menu_uid = None;
        // 分離は行動を消費しないのでメニューを再表示。
        self.reopen_unit_menu_for(host_uid);
    }

    /// 実行時合体の分離: 温存していた構成ユニット (off_map) の着地点を呼出側が選択させる。
    /// host を合体前形態・合体前の搭乗構成へ戻し、構成 uid 列を返す (placement は
    /// `apply_separate` が移動範囲から対話選択させる。候補無し時は隣接自動)。
    fn separate_preserved_components(
        &mut self,
        host_uid: &str,
        components: &[String],
    ) -> Vec<String> {
        // 構成ユニットは合体中ずっと off_map で温存されているため、ここでは盤上配置せず
        // uid 列だけ返す (着地は landing queue が担う)。
        let staged: Vec<String> = components.to_vec();
        // host を合体前形態へ戻す。
        let pre = self
            .database
            .unit_by_uid(host_uid)
            .and_then(|u| u.pre_combine_form.clone());
        if let Some(form) = pre {
            self.set_unit_form(host_uid, &form);
        }
        // host の搭乗者を合体前の構成へ戻す (構成ユニットのパイロットは各機へ復帰)。
        let pre_pilots = self
            .database
            .unit_by_uid(host_uid)
            .map(|u| u.pre_combine_pilots.clone())
            .unwrap_or_default();
        if let Some(u) = self.database.unit_by_uid_mut(host_uid) {
            u.pilot_ids = pre_pilots;
            u.combined_from.clear();
            u.pre_combine_form = None;
            u.pre_combine_pilots.clear();
        }
        staged
    }

    /// 最初から合体形態で配置されたユニットの分離: `分離` 特殊能力
    /// (`分離=<名称> <形態1> <形態2> …`) の形態へ分離する。host は形態 1 になり、
    /// 形態 2 以降は隣接空きマスへ新規生成する。原典 (分離.md / 変形系特殊能力.md):
    /// パイロットは形態の順に乗り移り、サポートパイロット (形態数を超える分) は全て
    /// 1 号機 (形態 1) に乗せられる。MVP: 形態順の主パイロット割当のみ、複数形態への
    /// 主/副の細かい配分は形態 1 へ集約する単純化。
    fn separate_into_forms(&mut self, host_uid: &str, forms: &[String]) -> Vec<String> {
        let Some((hpos, party, pilots)) = self
            .database
            .unit_by_uid(host_uid)
            .map(|u| ((u.x, u.y), u.party, u.pilot_ids.clone()))
        else {
            return Vec::new();
        };
        // 形態 1 = host。形態 2 以降を host 位置に off_map で生成し (staging)、着地点は
        // 呼出側 (`apply_separate`) が移動範囲から対話選択させる (候補無し時は隣接自動)。
        // 形態 i (0 基点) の主パイロット = pilots[i] (あれば)。形態数を超えるパイロットは
        // サポートとして形態 1 (host) に集約する。
        let mut staged = Vec::new();
        for (i, form) in forms.iter().enumerate().skip(1) {
            // 生成先の形態が DB に無ければスキップ。
            if self.database.unit_by_name(form).is_none() {
                continue;
            }
            let pilot_id = pilots.get(i).cloned();
            let pilot_name = pilot_id
                .as_deref()
                .map(|id| self.pilot_name_for_id(id))
                .unwrap_or_default();
            let mut inst = crate::UnitInstance::new(form, pilot_name, party, hpos.0, hpos.1);
            if let Some(pid) = pilot_id {
                inst.pilot_ids = vec![pid];
            }
            inst.off_map = true; // staging (pos_index に載せない)
            let uid = self.database.register_unit(inst);
            self.set_unit_form(&uid, form);
            staged.push(uid);
        }
        // host を形態 1 へ変身させ、形態 1 のパイロット (主 pilots[0] + サポート余り) を残す。
        self.set_unit_form(host_uid, &forms[0]);
        let mut host_pilots: Vec<String> = Vec::new();
        if let Some(p0) = pilots.first() {
            host_pilots.push(p0.clone());
        }
        for p in pilots.iter().skip(forms.len()) {
            host_pilots.push(p.clone());
        }
        if let Some(u) = self.database.unit_by_uid_mut(host_uid) {
            if !host_pilots.is_empty() {
                u.pilot_ids = host_pilots;
            }
        }
        staged
    }

    /// パイロット ID (PilotInstance.id または pilot_data_name) から表示用のパイロット
    /// データ名を解決する。見つからなければ ID をそのまま返す。
    fn pilot_name_for_id(&self, pilot_id: &str) -> String {
        self.database
            .pilot_instances
            .iter()
            .find(|p| p.id == pilot_id || p.pilot_data_name == pilot_id)
            .map(|p| p.pilot_data_name.clone())
            .unwrap_or_else(|| pilot_id.to_string())
    }

    /// `host` の `合体` 特殊能力が **2 体合体** (相手 1 機) で、合体形態 (DB に存在) と
    /// 相手 `partner_form` を持つか。相手ユニット上への移動による合体の判定に使う。
    /// 原典 (合体.md): 3 体以上合体は合体コマンド専用で、移動では合体しない。
    fn combine_lists_partner(&self, host_uid: &str, partner_form: &str) -> bool {
        let Some(host) = self.database.unit_by_uid(host_uid) else {
            return false;
        };
        let Some(val) = crate::feature::feature_value(&host.active_features, "合体") else {
            return false;
        };
        let toks: Vec<&str> = val.split_whitespace().collect();
        // 名称 + 合体形態 + 相手 1 機 = ちょうど 3 トークン (2 体合体) のみ。
        toks.len() == 3 && self.database.unit_by_name(toks[1]).is_some() && toks[2] == partner_form
    }

    /// `mover` が `target_pos` の隣接マスへ到達できるなら、その (空き) マスを返す。
    /// 既に隣接していれば現在位置を返す。搭載 / 合体の到達判定に使う。
    fn reachable_adjacent_to(&self, mover_uid: &str, target_pos: (u32, u32)) -> Option<(u32, u32)> {
        if let Some(mp) = self.database.unit_by_uid(mover_uid).map(|u| (u.x, u.y)) {
            if combat::manhattan(mp, target_pos) == 1 {
                return Some(mp);
            }
        }
        let range = self.database.unit_move_range(mover_uid);
        for (dx, dy) in [(0i32, -1), (0, 1), (-1, 0), (1, 0)] {
            let nx = target_pos.0 as i32 + dx;
            let ny = target_pos.1 as i32 + dy;
            if nx < 0 || ny < 0 {
                continue;
            }
            let adj = (nx as u32, ny as u32);
            if range.contains_key(&adj) {
                return Some(adj);
            }
        }
        None
    }

    /// 占有マス (味方の母艦 or 合体相手) へ移動しようとしたときの 搭載 / 合体 統合。
    /// `mover` が `occupant` に隣接到達でき、母艦搭載 or 合体が成立すれば実行して
    /// `true`。該当しなければ `false` (通常移動の占有判定に委ねる = 移動不可)。
    fn try_board_or_combine_by_move(&mut self, mover_uid: &str, occupant_uid: &str) -> bool {
        let (Some(m), Some(o)) = (
            self.database.unit_by_uid(mover_uid),
            self.database.unit_by_uid(occupant_uid),
        ) else {
            return false;
        };
        if m.party != o.party || o.off_map {
            return false;
        }
        let opos = (o.x, o.y);
        let mover_form = m.unit_data_name.clone();
        let occ_form = o.unit_data_name.clone();
        let occ_is_carrier = crate::feature::has_feature(&o.active_features, "母艦");
        let mover_no_store = crate::feature::has_feature(&m.active_features, "格納不可");
        let combine_into_occ = self.combine_lists_partner(occupant_uid, &mover_form);
        let combine_into_mover = self.combine_lists_partner(mover_uid, &occ_form);
        if !occ_is_carrier && !combine_into_occ && !combine_into_mover {
            return false; // 母艦でも合体相手でもない
        }
        // 到達判定 (隣接マスへ行けるか)。
        let Some(adj) = self.reachable_adjacent_to(mover_uid, opos) else {
            return false;
        };
        if occ_is_carrier && !mover_no_store {
            // 搭載: mover は off_map になるので位置は問わない。
            let (Some(mi), Some(ci)) = (
                self.database.idx_by_uid(mover_uid),
                self.database.idx_by_uid(occupant_uid),
            ) else {
                return false;
            };
            self.fire_boarding_event(mi, ci);
            return true;
        }
        // 合体: mover を隣接マスへ動かしてから host を合体させる。
        if combine_into_occ {
            self.database.move_unit(mover_uid, adj.0, adj.1);
            self.apply_combine(occupant_uid);
            return true;
        }
        if combine_into_mover {
            self.database.move_unit(mover_uid, adj.0, adj.1);
            self.apply_combine(mover_uid);
            return true;
        }
        false
    }

    /// 精神コマンドの SP 消費を反映する。`PilotInstance` があれば `sp_remaining` を、
    /// 無ければ `UnitInstance.sp_consumed` を更新する (`SpecialPower` 命令と同方針)。
    fn consume_unit_sp(&mut self, uid: &str, cost: i32) {
        let Some(unit_idx) = self.database.idx_by_uid(uid) else {
            return;
        };
        let pilot_id = {
            let u = &self.database.unit_instances[unit_idx];
            u.pilot_ids
                .first()
                .cloned()
                .unwrap_or_else(|| u.pilot_name.clone())
        };
        let pilot_idx = self
            .database
            .pilot_instances
            .iter()
            .position(|p| p.id == pilot_id || p.pilot_data_name == pilot_id);
        if let Some(pi) = pilot_idx {
            let p = &mut self.database.pilot_instances[pi];
            p.sp_remaining = (p.sp_remaining - cost).max(0);
        } else {
            self.database.unit_instances[unit_idx].sp_consumed += cost;
        }
    }

    /// SP を `amount` 回復する (最大値でクランプ)。`consume_unit_sp` の逆。
    /// アビリティ効果 霊力回復 / ＳＰ回復 用。
    fn restore_unit_sp(&mut self, uid: &str, amount: i32) {
        let Some(unit_idx) = self.database.idx_by_uid(uid) else {
            return;
        };
        let max = self.unit_sp(&self.database.unit_instances[unit_idx]).0;
        let pilot_id = {
            let u = &self.database.unit_instances[unit_idx];
            u.pilot_ids
                .first()
                .cloned()
                .unwrap_or_else(|| u.pilot_name.clone())
        };
        let pilot_idx = self
            .database
            .pilot_instances
            .iter()
            .position(|p| p.id == pilot_id || p.pilot_data_name == pilot_id);
        if let Some(pi) = pilot_idx {
            let p = &mut self.database.pilot_instances[pi];
            p.sp_remaining = (p.sp_remaining + amount).min(max);
        } else {
            let u = &mut self.database.unit_instances[unit_idx];
            u.sp_consumed = (u.sp_consumed - amount).max(0);
        }
    }

    /// uid のユニットの居る位置でユニットコマンドメニューを再表示する。
    fn reopen_unit_menu_for(&mut self, uid: &str) {
        if let Some(u) = self.database.unit_by_uid(uid) {
            let pos = (u.x, u.y);
            self.open_unit_menu(pos);
        }
    }

    fn move_cursor(&mut self, dir: Direction) -> bool {
        // 単機ステータス詳細: Left/Up=前の機体, Right/Down=次の機体 (ラップ)。
        if self.scene == Scene::UnitDetail {
            return match dir {
                Direction::Left | Direction::Up => self.cycle_status_detail(-1),
                Direction::Right | Direction::Down => self.cycle_status_detail(1),
            };
        }
        if self.scene == Scene::Intermission {
            // インターミッション画面では Up/Down が項目選択カーソル移動。
            // Left/Right は無視。
            let item_count = self.intermission_item_count();
            if item_count == 0 {
                return false;
            }
            match dir {
                Direction::Up => {
                    if self.intermission_cursor == 0 {
                        self.intermission_cursor = item_count - 1;
                    } else {
                        self.intermission_cursor -= 1;
                    }
                    return true;
                }
                Direction::Down => {
                    self.intermission_cursor = (self.intermission_cursor + 1) % item_count;
                    return true;
                }
                _ => return false,
            }
        }
        if self.scene != Scene::MapView {
            return false;
        }
        let Some(map) = self.database.map.as_ref() else {
            return false;
        };
        if map.width == 0 || map.height == 0 {
            return false;
        }
        let (mut x, mut y) = self.map_cursor.unwrap_or((0, 0));
        match dir {
            Direction::Left if x > 0 => x -= 1,
            Direction::Right if x + 1 < map.width => x += 1,
            Direction::Up if y > 0 => y -= 1,
            Direction::Down if y + 1 < map.height => y += 1,
            _ => return false,
        }
        let next = Some((x, y));
        if self.map_cursor == next {
            return false;
        }
        self.map_cursor = next;
        self.ensure_cursor_visible();
        true
    }

    /// 「次へ」操作。Title / Configuration / Intermission の遷移のみを担う。
    ///
    /// MapView では **何もしない**。原典 SRC にはステージ進行をユーザの Enter
    /// 押下でゲートする仕様が無く（[`スタートイベント.md`] 参照）、
    /// `Briefing → Sortie → Battle` は [`Self::auto_progress_stage_state_if_idle`]
    /// が idle 時に自動進行させる。`Victory` / `Defeat` も独自オーバーレイと
    /// Enter→タイトルのゲートを持たず、勝利/敗北/Ending ラベルの発火に委ねる。
    fn advance(&mut self) -> bool {
        match self.scene {
            Scene::Title => {
                self.enter_configuration();
                true
            }
            Scene::Configuration => {
                self.settings_snapshot = None;
                // インターミッションコマンドが登録されていれば、戦闘前に
                // メニュー画面を挟む。無ければ従来通り MapView へ直行。
                if !self.intermission_commands.is_empty() {
                    self.scene = Scene::Intermission;
                    self.intermission_cursor = 0;
                    self.intermission_mode = IntermissionMode::Menu;
                } else {
                    self.scene = Scene::MapView;
                }
                true
            }
            Scene::Intermission => {
                self.confirm_intermission_selection();
                true
            }
            // 戦闘画面では Enter で進行しない。ステージ進行は auto_progress と
            // ラベル発火が担う（誤押下でのシーン遷移も防ぐ）。
            // 例外: 敗北フォールバック中は Enter/クリック = コンティニュー、勝利状態で
            // クリア が進行を解決しない場合は Enter/クリック = 次へ (soft-lock 脱出)。
            Scene::MapView => {
                if self.pending_game_over {
                    self.game_over_continue();
                    true
                } else if self.stage_state == crate::stage::StageState::Victory {
                    self.proceed_after_victory();
                    true
                } else {
                    false
                }
            }
            Scene::PilotList => {
                self.scene = Scene::UnitList;
                true
            }
            Scene::UnitList => {
                self.exit_list_scene();
                true
            }
            // 単機ステータス詳細: Enter で閉じてインターミッションへ戻る
            // (巡回は ◀ / ▶ ボタンか矢印キー)。
            Scene::UnitDetail => {
                self.exit_list_scene();
                true
            }
        }
    }

    /// 一覧シーン (UnitList) を抜けるときの遷移。`scene_return_to` があればそこへ
    /// (インターミッション「ステータス」からの復帰)、無ければ既定の Title へ。
    fn exit_list_scene(&mut self) {
        match self.scene_return_to.take() {
            Some(Scene::Intermission) => {
                self.scene = Scene::Intermission;
                self.intermission_mode = IntermissionMode::Menu;
                self.intermission_cursor = 0;
            }
            Some(s) => self.scene = s,
            None => self.scene = Scene::Title,
        }
    }

    /// 単機ステータス詳細で巡回対象になる味方ロスター (off_map 含む)。
    /// `intermission_upgrade_units` と同じく Player 陣営の `unit_instances` index 列。
    fn status_roster(&self) -> Vec<usize> {
        self.intermission_upgrade_units()
    }

    /// 単機ステータス詳細で次/前の機体へ巡回する (ロスター内をラップ)。
    /// `delta` は +1 (次) / -1 (前)。ロスターが空なら何もしない。
    fn cycle_status_detail(&mut self, delta: isize) -> bool {
        let total = self.status_roster().len();
        if total == 0 {
            return false;
        }
        let cur = self.status_detail_index.min(total - 1) as isize;
        let next = (cur + delta).rem_euclid(total as isize) as usize;
        self.status_detail_index = next;
        true
    }

    /// `Scene::UnitDetail` で表示する単機ステータスのビューモデルを構築する。
    /// `roster_pos` は味方ロスター (`status_roster`) 内の 0 始まり位置。範囲外 /
    /// ロスター空なら `None`。改造段階・装備・レベル成長・状態異常込みの実効値を
    /// 反映する (`GameDatabase::effective_*` / `effective_pilot_data`)。
    pub fn build_status_detail(
        &self,
        roster_pos: usize,
    ) -> Option<crate::scene::unit_detail::StatusDetail> {
        use crate::scene::unit_detail::{StatusDetail, WeaponRow};

        let roster = self.status_roster();
        let total = roster.len();
        let &unit_idx = roster.get(roster_pos)?;
        let u = self.database.unit_instances.get(unit_idx)?;
        let unit_data = self.database.unit_by_name(&u.unit_data_name)?;

        let hp_max = self.database.effective_max_hp(u);
        let hp_cur = (hp_max - u.damage).max(0);
        let en_max = self.database.effective_max_en(u);
        let en_cur = (en_max - u.en_consumed).max(0);

        // 搭乗パイロット (メイン)。無人なら has_pilot=false。
        let main_pilot = u.main_pilot_name();
        let pilot_data = if main_pilot.is_empty() {
            None
        } else {
            self.database.effective_pilot_data(main_pilot)
        };
        let pilot_inst = u
            .pilot_ids
            .first()
            .and_then(|id| self.database.pilot_instance_by_id(id));

        // 武器一覧 (runtime 状態を反映: 残弾 / EN)。
        let weapons: Vec<WeaponRow> = u
            .weapons
            .iter()
            .filter_map(|uw| {
                let wd = unit_data.weapons.get(uw.weapon_index)?;
                let range = if wd.min_range == wd.max_range {
                    wd.max_range.to_string()
                } else {
                    format!("{}-{}", wd.min_range, wd.max_range)
                };
                let ammo = if wd.bullet > 0 {
                    format!("残{}/{}", uw.bullet_remaining.max(0), wd.bullet)
                } else if wd.en_consumption > 0 {
                    format!("EN{}", wd.en_consumption)
                } else {
                    "-".to_string()
                };
                // 必要技能 / 必要条件のゲートのみで使用可否を判定する (has_acted / EN / 残弾
                // 等の一時的状態は閲覧画面では除外し、技能による恒久的な解禁状況のみ示す)。
                let ns = wd.necessary_skill();
                let nc = wd.necessary_condition();
                let usable = (ns.is_empty()
                    || crate::necessary_skill::is_satisfied(ns, u, &self.database))
                    && (nc.is_empty()
                        || crate::necessary_skill::is_satisfied(nc, u, &self.database));
                Some(WeaponRow {
                    name: wd.name.clone(),
                    power: wd.power,
                    range,
                    ammo,
                    usable,
                })
            })
            .collect();

        let conditions: Vec<String> = u.conditions.iter().map(|c| c.name.clone()).collect();

        // パイロット項目。
        let (
            has_pilot,
            pilot_name,
            pilot_nickname,
            level,
            exp,
            sp_cur,
            sp_max,
            infight,
            shooting,
            hit,
            dodge,
            intuition,
            technique,
            pilot_adaption,
            spirit_commands,
            skills,
        ) = if let Some(pd) = pilot_data.as_ref() {
            let level = pilot_inst.map(|p| p.level).unwrap_or(1);
            let exp = pilot_inst.map(|p| p.total_exp).unwrap_or(u.total_exp);
            let sp_max = pd.sp.unwrap_or(0);
            let sp_cur = pilot_inst.map(|p| p.sp_remaining).unwrap_or(sp_max);
            let spirit_commands: Vec<String> = pd
                .spirit_commands
                .iter()
                .map(|sc| format!("{} Lv{}", sc.name, sc.level))
                .collect();
            // 特殊技能: runtime PilotInstance.skills を優先し、無ければ静的特殊能力名。
            let skills: Vec<String> = if let Some(pi) = pilot_inst {
                if !pi.skills.is_empty() {
                    pi.skills.clone()
                } else {
                    pd.features.iter().map(|(n, _)| n.clone()).collect()
                }
            } else {
                pd.features.iter().map(|(n, _)| n.clone()).collect()
            };
            (
                true,
                pd.name.clone(),
                pd.nickname.clone(),
                level,
                exp,
                sp_cur,
                sp_max,
                pd.infight,
                pd.shooting,
                pd.hit,
                pd.dodge,
                pd.intuition,
                pd.technique,
                pd.adaption.as_str().to_string(),
                spirit_commands,
                skills,
            )
        } else {
            (
                false,
                String::new(),
                String::new(),
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                0,
                String::new(),
                Vec::new(),
                Vec::new(),
            )
        };

        Some(StatusDetail {
            index: roster_pos,
            total,
            unit_name: unit_data.name.clone(),
            unit_nickname: unit_data.nickname.clone(),
            party_label: u.party.short_label().to_string(),
            class: u
                .class_override
                .clone()
                .unwrap_or_else(|| unit_data.class.clone()),
            size: unit_data.size.label().to_string(),
            unit_adaption: unit_data.adaption.as_str().to_string(),
            hp_cur,
            hp_max,
            en_cur,
            en_max,
            armor: self.database.effective_armor(u),
            mobility: self.database.effective_mobility(u),
            speed: self.database.effective_speed(u),
            upgrade_level: u.upgrade_level,
            unit_morale: u.morale,
            conditions,
            has_pilot,
            pilot_name,
            pilot_nickname,
            level,
            exp,
            sp_cur,
            sp_max,
            infight,
            shooting,
            hit,
            dodge,
            intuition,
            technique,
            pilot_adaption,
            spirit_commands,
            skills,
            weapons,
        })
    }

    /// 戦闘中に勝敗を判定。Player 側ユニットが居なくなれば敗北、
    /// 対立勢力 (Enemy) が居なくなれば勝利。両方とも 0 件の場合は判定しない
    /// （まだユニットが配置されていない / すでに判定済み等）。
    ///
    /// 勝敗確定時は `game_clear()` / `game_over()` を呼び、それぞれ
    /// `Victory`/`勝利` / `GameOver`/`ゲームオーバー` ラベルを自動発火する。
    /// `(x, y)` の地形に対応する地形適応の環境インデックス (0=空/1=陸/2=海/3=宇)。
    /// 戦闘ダメージの地形適応 ([`crate::combat::predict_with_status_terrain`]) に渡す。
    /// マップ未ロード / 範囲外は -1 (地形適応なし=×1.0)。地形未定義は陸 (1) 扱い。
    pub fn terrain_env_at(&self, x: u32, y: u32) -> i32 {
        let Some(map) = self.database.map.as_ref() else {
            return -1;
        };
        if x >= map.width || y >= map.height {
            return -1;
        }
        let tid = map.cell(x, y).terrain_id;
        crate::data::terrain::lookup(tid)
            .map(|t| crate::combat::terrain_env(t.class))
            .unwrap_or(1)
    }

    /// `(x, y)` の地形の防御価値。値が大きいほど守りやすい (被ダメージ軽減 + 回避上昇)。
    /// 戦闘では `damage_mod` が正値ほど被ダメージを減らし (`(100 - damage_mod)/100`)、
    /// `hit_mod` も**正値ほど被命中を下げる** (`(100 - hit_mod)/100`、SRC 規約) ため、両者を
    /// 合わせた `damage_mod + hit_mod` を指標とする (平地=0 / 森林=15 / 山=25 / 都市=35)。
    /// マップ未ロード / 範囲外は 0 (中立)。敵 AI の防御地形選好に使う。
    fn tile_defensive_value(&self, x: u32, y: u32) -> i32 {
        let Some(map) = self.database.map.as_ref() else {
            return 0;
        };
        if x >= map.width || y >= map.height {
            return 0;
        }
        let tid = map.cell(x, y).terrain_id;
        self.database.terrain_damage_mod(tid) + self.database.terrain_hit_mod(tid)
    }

    /// `party` に敵対する勢力のオンマップユニットがマップ兵器 (武器 class に全角Ｍ) を
    /// 1 つでも持つか。敵 AI の散開判断 (マップ兵器回避) に使う。マップ兵器を持つ敵が
    /// 居なければ散開しない (= 通常の攻撃位置選択)。
    fn hostile_has_map_weapon(&self, party: crate::Party) -> bool {
        self.database.unit_instances.iter().any(|u| {
            !u.off_map
                && u.party.is_hostile_to(party)
                && self
                    .database
                    .unit_by_name(&u.unit_data_name)
                    .is_some_and(|d| d.weapons.iter().any(|w| w.class.contains('Ｍ')))
        })
    }

    /// 現ステージファイルが `label` を定義しているか (戦闘終了イベントの委譲判定用)。
    /// `current_stage_file` 未設定時は global にフォールバック。
    fn stage_defines_label(&self, label: &str) -> bool {
        if self.current_stage_file.is_empty() {
            return self.script_library.label_pc(label).is_some();
        }
        self.script_library
            .label_pc_in_file(&self.current_stage_file, label)
            .is_some()
    }

    pub(crate) fn check_victory(&mut self) {
        use crate::stage::StageState;
        if self.stage_state != StageState::Battle {
            return;
        }
        if self.database.unit_instances.is_empty() {
            return;
        }
        // off_map (Escape 退避中) のユニットは勝敗判定から除外。
        let player_alive = self
            .database
            .unit_instances
            .iter()
            .any(|u| u.party == crate::Party::Player && !u.off_map);
        // 味方全滅 = 敗北。敵/中立の生死に関わらず確実に発火する。
        //
        // 旧実装は「味方も敵も全滅 → 引き分けで無処理」で early return しており、
        // 敵だけ先に全滅して敵対する中立が残る局面で味方全滅すると `!味方 && !敵`
        // が真になり敗北を発火できず詰んでいた (ユーザ報告: 中立フェイズで味方が
        // 倒されても敗北せず進行不能)。中立は SRC のキャンプ仕様で味方に敵対する
        // ため、味方が居なくなれば一律敗北とする。
        if !player_alive {
            self.game_over();
            return;
        }
        // 勝利判定: 味方に敵対する陣営 (敵 + 中立) が全滅したか。中立を勝利条件に
        // 含めないと、敵対中立が残るのに早期勝利してしまう。
        let hostile_alive =
            self.database.unit_instances.iter().any(|u| {
                !u.off_map && matches!(u.party, crate::Party::Enemy | crate::Party::Neutral)
            });
        if !hostile_alive {
            // シナリオの `全滅 敵` / `全滅 中立` ハンドラは破壊イベント経由
            // (`fire_destruction_labels` → `fire_total_annihilation_if_any`) で発火する。
            // それが勝利演出 (Talk) や次ステージ (Continue) を出していれば「idle ではない」
            // ので組込み勝利は発火しない (委譲)。
            //
            // 逆に、ハンドラが無い / 空 / 進行を解決しないために **敵全滅 & Battle のまま
            // idle** (dialog / script / flow / event_queue いずれも無し) になった場合は、
            // 組込み `game_clear` (= `クリア` / `Victory` 発火) を呼んで soft-lock を防ぐ。
            // 旧実装は `全滅 敵` 定義時に無条件委譲し、ハンドラが進行させないと敵全滅後も
            // Battle のまま詰んでいた (実機報告: 東方夢想伝01)。idle 判定で進行中の勝利
            // 演出は中断しない。
            let scenario_handles_victory =
                self.stage_defines_label("全滅 敵") || self.stage_defines_label("全滅 中立");
            let idle = self.pending_dialog.is_none()
                && !self.has_script_context()
                && self.flow.is_empty()
                && self.event_queue.is_empty();
            if !scenario_handles_victory || idle {
                self.game_clear();
            }
        }
    }

    /// 設定画面 (Configuration) へ遷移し、キャンセル復元用に現在の設定を
    /// スナップショットする。メニューバー「設定変更」/ タイトルクリックから呼ぶ。
    ///
    /// 対話 (`pending_dialog`) やスクリプト (`script_ctx`) が動作中は遷移しない。
    /// 原典 SRC でも設定変更はイベント再生中には実行できず、また遷移すると Talk
    /// 等が Configuration 画面に重なって操作不能になる (クリックは pending_dialog に
    /// 吸われる) ため。
    pub fn enter_configuration(&mut self) {
        if self.pending_dialog.is_some() || self.has_script_context() {
            return;
        }
        self.scene = Scene::Configuration;
        self.settings_snapshot = Some(self.settings.clone());
    }

    /// canvas-local 座標でクリックを処理。
    fn handle_click(&mut self, x: i32, y: i32) -> bool {
        let (sw, sh) = self.scene.size();
        let ox = (CANVAS_WIDTH as i32 - sw as i32) / 2;
        let oy = (CANVAS_HEIGHT as i32 - sh as i32) / 2;
        let lx = x - ox;
        let ly = y - oy;

        match self.scene {
            Scene::Title => {
                self.enter_configuration();
                true
            }
            Scene::Configuration => self.handle_click_configuration(lx, ly),
            Scene::Intermission => self.handle_click_intermission(lx, ly),
            // MapView は メニュー判定で canvas 絶対座標も使うので (x, y) 双方を渡す。
            Scene::MapView => self.handle_click_map_view(x, y, lx, ly),
            Scene::PilotList => {
                self.scene = Scene::UnitList;
                true
            }
            Scene::UnitList => {
                self.exit_list_scene();
                true
            }
            Scene::UnitDetail => self.handle_click_unit_detail(lx, ly),
        }
    }

    /// 単機ステータス詳細でのクリック: フッタの ◀ / ▶ / 閉じる ボタンのみ反応する
    /// (ボタン外クリックは無反応＝誤操作で閉じない)。
    fn handle_click_unit_detail(&mut self, lx: i32, ly: i32) -> bool {
        use crate::scene::unit_detail::{hit_button, DetailButton};
        match hit_button(lx, ly) {
            Some(DetailButton::Prev) => self.cycle_status_detail(-1),
            Some(DetailButton::Next) => self.cycle_status_detail(1),
            Some(DetailButton::Close) => {
                self.exit_list_scene();
                true
            }
            None => false,
        }
    }

    /// MapView でのクリック処理:
    /// - カーソル位置に現フェーズの所属ユニットがいて、クリック先が
    ///   そのユニットの移動範囲内の空マスなら、ユニットを移動。
    /// - そうでなければカーソルを移動。
    ///
    /// 入力 `(lx, ly)` は MapView 内のローカル座標。ビューポート相対の
    /// タイル位置に変換した後、`map_scroll` を足してマップ絶対座標にする。
    fn handle_click_map_view(&mut self, abs_x: i32, abs_y: i32, lx: i32, ly: i32) -> bool {
        // 敗北フォールバック中の左クリック = コンティニュー (soft-lock 脱出)。
        if self.pending_game_over {
            self.game_over_continue();
            return true;
        }
        // 勝利状態でクリア等が進行を解決しない場合の左クリック = 次へ (soft-lock 脱出)。
        // 勝利演出 (Talk) 中はクリックが pending_dialog へ行くのでここには来ない。
        if self.stage_state == crate::stage::StageState::Victory {
            self.proceed_after_victory();
            return true;
        }
        // 敗北確定後はマップ操作を一切受け付けない (クリックでユニットが動く等の破綻防止)。
        if self.stage_state == crate::stage::StageState::Defeat {
            return false;
        }
        // 1) コマンドメニューが表示中なら、まずメニュー項目クリックを試みる。
        //    メニューは canvas 絶対座標で描画されるので abs_x/abs_y で判定。
        if self.command_menu.is_some() {
            if let Some(action) =
                crate::command_menu::hit_test_menu_item(self.command_menu.as_ref(), abs_x, abs_y)
            {
                return self.execute_menu_action(action);
            }
            // メニュー外をクリックしたらメニューを閉じる
            self.command_menu = None;
            // 移動後メニューを外クリックで閉じた場合は「移動を確定」してブラウズに戻す
            // (巻き戻さない)。これで続けて別ユニットを選択でき、移動済みユニットは
            // その場に留まる。移動の取り消しは右クリック (cancel_action) のみが行う。
            if matches!(
                self.action_mode,
                crate::command_menu::ActionMode::PostMoveMenu { .. }
            ) {
                self.action_mode = crate::command_menu::ActionMode::Browse;
            }
        }

        let Some((vtx, vty)) = map_view::pixel_to_tile(lx, ly) else {
            return false;
        };
        let target = (vtx + self.map_scroll.0, vty + self.map_scroll.1);
        if let Some(map) = self.database.map.as_ref() {
            if target.0 >= map.width || target.1 >= map.height {
                return false;
            }
        } else {
            return false;
        }
        let prev_cursor = self.map_cursor;
        self.map_cursor = Some(target);
        self.ensure_cursor_visible();

        // Battle 以外ではメニューフローを使わず、旧来の「cursor 上ユニットを直接移動」を維持。
        if self.stage_state != crate::stage::StageState::Battle {
            if let Some(prev) = prev_cursor {
                if prev != target && self.try_move_unit_to(prev, target) {
                    return true;
                }
            }
            return true;
        }

        use crate::command_menu::ActionMode;
        // 2) ActionMode に応じてクリック処理を分岐 (Battle 時のみ)
        match self.action_mode.clone() {
            ActionMode::MoveSelect { uid } => {
                // 占有マス (味方の母艦 / 合体相手) へのクリックは 搭載 / 合体 を試みる
                // (原典: 母艦上へ移動で搭載・2 機合体は相手上へ移動)。成立すれば確定。
                if let Some(occ) = self
                    .database
                    .uid_at(target.0, target.1)
                    .map(|s| s.to_string())
                {
                    if occ != uid && self.try_board_or_combine_by_move(&uid, &occ) {
                        self.action_mode = ActionMode::Browse;
                        self.command_menu = None;
                        return true;
                    }
                }
                // 移動前スナップショットを取得してから移動を確定 (キャンセル復帰用)。
                let snap =
                    self.database
                        .unit_by_uid(&uid)
                        .map(|u| crate::command_menu::MoveSnapshot {
                            x: u.x,
                            y: u.y,
                            en_consumed: u.en_consumed,
                            current_area: u.current_area.clone(),
                        });
                if let Some(snapshot) = snap {
                    let from = (snapshot.x, snapshot.y);
                    if self.try_move_unit_to(from, target) {
                        // 移動成功 → 移動後メニューを表示 (同一 uid を追従)
                        self.action_mode = ActionMode::PostMoveMenu {
                            uid: uid.clone(),
                            snapshot,
                        };
                        self.open_unit_menu(target);
                    }
                }
                // 移動失敗 (範囲外等) はそのまま (mode 継続)
                true
            }
            ActionMode::PostMoveMenu { .. } => true,
            ActionMode::AttackSelect { uid, snapshot } => {
                // クリックしたタイルのユニットを攻撃 (Phase D で対象解決を厳密化)。
                let post_move = snapshot.is_some();
                // UI モード + 武器選択ウィンドウ有効時は、攻撃可能な目標なら武器選択窓を
                // 出して解決を保留する (応答後に resolve_weapon_select が解決)。
                if self.animate_battle
                    && self.weapon_window_enabled
                    && self.can_attack_target(&uid, target, post_move)
                {
                    self.begin_weapon_select(uid.clone(), target, post_move);
                } else if self.attack_unit_at(&uid, target, post_move) {
                    // 攻撃したら行動終了 → has_acted、メニュー閉じる。
                    self.finish_player_attack(&uid);
                }
                true
            }
            ActionMode::SpiritTarget {
                caster,
                spirit,
                cost,
                target_enemy,
            } => {
                // クリックしたタイルのユニットを精神コマンドの対象に取る。
                let clicked = self
                    .database
                    .uid_at(target.0, target.1)
                    .and_then(|uid| self.database.unit_by_uid(uid))
                    .map(|u| (u.uid.clone(), u.party));
                let caster_party = self.database.unit_by_uid(&caster).map(|u| u.party);
                if let (Some((target_uid, tparty)), Some(cparty)) = (clicked, caster_party) {
                    let valid = if target_enemy {
                        cparty.is_hostile_to(tparty)
                    } else {
                        cparty.is_ally_of(tparty)
                    };
                    if valid {
                        self.apply_spirit_to_target(&caster, &target_uid, &spirit, cost);
                        self.action_mode = ActionMode::Browse;
                        // 発動主体のメニューを再表示し連続発動 / 行動を可能にする。
                        if let Some(pos) = self.database.unit_by_uid(&caster).map(|u| (u.x, u.y)) {
                            self.open_unit_menu(pos);
                        }
                    }
                    // 無効な対象 (陣営違い) のクリックは no-op で選択モード継続。
                }
                true
            }
            ActionMode::SupportTarget { caster, kind } => {
                // クリックしたタイルのユニットを修理 / 補給の対象に取る。
                // 候補 (隣接・味方・要支援) に含まれるときのみ適用し、行動終了する。
                let clicked = self
                    .database
                    .uid_at(target.0, target.1)
                    .map(|s| s.to_string());
                if let Some(tuid) = clicked {
                    if self.support_target_uids(&caster, kind).contains(&tuid) {
                        self.apply_support_to_target(&caster, &tuid, kind);
                        // 行動終了 (修理 / 補給 は発動で行動を消費する)。
                        if let Some(u) = self.database.unit_by_uid_mut(&caster) {
                            u.has_acted = true;
                        }
                        if let Some(i) = self.database.idx_by_uid(&caster) {
                            crate::event_runtime::fire_action_end_labels(self, i);
                            // SRC `接触 <unit1> <unit2>:` ─ 行動終了後に隣接ペアで発火。
                            crate::event_runtime::fire_contact_event_labels(self, i);
                        }
                        self.action_mode = ActionMode::Browse;
                        self.command_menu = None;
                    }
                    // 候補外 (非隣接 / 陣営違い / 支援不要) のクリックは no-op で継続。
                }
                true
            }
            ActionMode::AbilityTarget {
                caster,
                ability_idx,
            } => {
                // クリックしたタイルのユニットをアビリティの対象に取る。
                // 有効 (味方・射程内) のときのみ適用し、消費・行動終了処理を行う。
                let clicked = self
                    .database
                    .uid_at(target.0, target.1)
                    .map(|s| s.to_string());
                if let Some(tuid) = clicked {
                    if self.ability_target_valid(&caster, ability_idx, &tuid) {
                        self.apply_ability(&caster, ability_idx, &tuid);
                        self.finish_ability(&caster);
                    }
                    // 無効な対象 (射程外 / 陣営違い) のクリックは no-op で継続。
                }
                true
            }
            ActionMode::LandingSelect { .. } => {
                // 着地候補マスのクリックで配置を確定する。候補外は no-op (継続)。
                self.confirm_landing(target.0, target.1);
                true
            }
            ActionMode::Browse => {
                // 通常: クリック先にユニットがあればメニュー、無ければマップメニュー
                let on_unit = self
                    .database
                    .uid_at(target.0, target.1)
                    .and_then(|uid| self.database.unit_by_uid(uid))
                    .map(|u| (u.party, u.has_acted));
                if let Some((party, has_acted)) = on_unit {
                    let is_player_active = party == self.turn.phase.party() && !has_acted;
                    if is_player_active && self.stage_state == crate::stage::StageState::Battle {
                        self.open_unit_menu(target);
                    }
                } else if self.stage_state == crate::stage::StageState::Battle {
                    self.open_map_menu();
                }
                true
            }
        }
    }

    /// ユニットコマンドメニューを開く（指定タイルにユニットがあること前提）。
    fn open_unit_menu(&mut self, pos: (u32, u32)) {
        use crate::command_menu::{CommandMenu, UnitAction, UnitMenuItem};
        let Some(u) = self
            .database
            .unit_instances
            .iter()
            .find(|u| u.x == pos.0 && u.y == pos.1)
            .cloned()
        else {
            return;
        };
        let post_move = u.has_moved;
        let mut items = Vec::new();

        // 移動コマンドは移動前のみ
        if !post_move {
            items.push(UnitMenuItem::Builtin(UnitAction::Move));
        }

        // 攻撃コマンド: 射程内に敵がいるか（移動後は武器属性で絞り込み）
        let unit_def = self.database.unit_by_name(&u.unit_data_name).cloned();
        let totsugeki = u.has_condition("突撃");
        if let Some(def) = unit_def.as_ref() {
            let any_in_range = self.database.unit_instances.iter().any(|other| {
                if !other.party.is_hostile_to(u.party) || other.off_map {
                    return false;
                }
                let d = combat::manhattan(pos, (other.x, other.y));
                // メニュー可否と攻撃実行で同一述語を使い齟齬を防ぐ。
                def.weapons.iter().any(|w| {
                    combat::weapon_in_range(w, d)
                        && Self::weapon_usable_post_move(w, post_move, totsugeki)
                })
            });
            if any_in_range {
                items.push(UnitMenuItem::Builtin(UnitAction::Attack));
            }
        }

        // 武装一覧は移動前のみ
        if !post_move {
            items.push(UnitMenuItem::Builtin(UnitAction::WeaponList));
        }

        // 精神コマンド: パイロットが習得済み (level<=パイロットレベル) の SP コマンドを
        // 持つ味方ユニットで、まだ行動を確定していない (移動後でも可) ときに表示する。
        if u.party == crate::Party::Player
            && !u.has_acted
            && !self.spirit_command_options(&u.uid).is_empty()
        {
            items.push(UnitMenuItem::Spirit);
        }

        // 修理 / 補給（特殊能力 修理装置 / 補給装置）: 隣接に要支援の味方が居る
        // 味方ユニットで、まだ行動を確定していない（移動後も可）ときに表示する。
        // 原典 SRC: 修理/補給は発動で行動終了する（精神コマンドと異なり継続不可）。
        if u.party == crate::Party::Player && !u.has_acted {
            for kind in [
                crate::command_menu::SupportKind::Repair,
                crate::command_menu::SupportKind::Supply,
            ] {
                // `修理装置Lv*` のようにレベル接尾辞付きでも拾えるよう feature_level で判定。
                if crate::feature::feature_level(&u.active_features, kind.feature_name()).is_some()
                    && !self.support_target_uids(&u.uid, kind).is_empty()
                {
                    items.push(UnitMenuItem::Support(kind));
                }
            }
        }

        // 変形（特殊能力 変形）: 変形先を持つ味方ユニットで、移動前かつ未行動の
        // ときに表示する。原典 SRC: 変形は移動前のみ・行動を消費しない。
        if u.party == crate::Party::Player
            && !post_move
            && !u.has_acted
            && self.transform_forms(&u).is_some()
        {
            items.push(UnitMenuItem::Transform);
        }

        // チャージ: チャージ攻撃 (Ｃ 属性) 武器を持つ味方ユニットで、未チャージ・
        // 未行動のときに表示する。原典 SRC: チャージで行動終了し、次ターンに
        // チャージ攻撃が解禁される (charged フラグは攻撃使用まで持続)。
        if u.party == crate::Party::Player && !u.has_acted && !u.charged {
            if let Some(def) = unit_def.as_ref() {
                if def.weapons.iter().any(combat::is_charge_weapon) {
                    items.push(UnitMenuItem::Charge);
                }
            }
        }

        // アビリティ: アビリティを持つ未行動の味方ユニットで表示する。使用可否
        // (回数 / EN / 気力) はサブメニュー側で × 表示・判定する (原典準拠)。
        if u.party == crate::Party::Player
            && !u.has_acted
            && unit_def.as_ref().is_some_and(|d| !d.abilities.is_empty())
        {
            items.push(UnitMenuItem::Ability);
        }

        // 発進: 母艦にユニットを格納している味方ユニット。原典 SRC: 発進は行動を
        // 消費しない (メニューが開くのは未行動ユニットなので !has_acted は不問)。
        if u.party == crate::Party::Player && !u.stored_units.is_empty() {
            items.push(UnitMenuItem::Launch);
        }

        // 合体: 合体特殊能力を持ち、合体相手が全て 2 マス以内に居る味方ユニット。
        // 原典 SRC (合体.md): 合体コマンドで合体するのは 3 体以上合体 (相手 2 機以上) の
        // 場合のみ。2 体合体は相手ユニット上への移動で行う (move 経路)。合体は移動前のみ・
        // 発動で行動終了。
        if u.party == crate::Party::Player
            && !u.has_acted
            && !post_move
            && self.combine_partner_count(&u.uid) >= 2
            && self.combine_partners(&u.uid).is_some()
        {
            items.push(UnitMenuItem::Combine);
        }

        // 分離: 構成ユニットを内包している (実行時に合体した) 味方ユニット、または `分離`
        // 特殊能力を持つ味方ユニット (最初から合体形態で `.eve` 配置されたユニット)。
        // 原典 SRC: 分離は行動を消費しない。
        if u.party == crate::Party::Player
            && (!u.combined_from.is_empty()
                || crate::feature::has_feature(&u.active_features, "分離"))
        {
            items.push(UnitMenuItem::Separate);
        }

        items.push(UnitMenuItem::Builtin(UnitAction::Wait));

        {
            // シナリオ定義の `[*|-]ユニットコマンド` で、対象勢力がこのユニットに
            // 合致するものを追記する。
            //
            // 原典 SRC ([ユニットコマンドイベント.md](../../../SRC.Sharp/SRC.Sharp.Help/src/ユニットコマンドイベント.md)):
            //  - `condition` 式の値が 0 でないときだけ表示
            //  - 同時に表示される項目は最大 10 件
            //  - タイミングフラグ:
            //      - `ユニットコマンド`   → 移動前のみ (post_move_ok=false, post_act_ok=false)
            //      - `*ユニットコマンド`  → 移動後も表示 (post_move_ok=true)
            //      - `*-ユニットコマンド` → 同上
            //      - `-*ユニットコマンド` → 行動終了後も表示 (post_act_ok=true)
            //      - `**ユニットコマンド` → 移動後・行動終了後どちらでも表示
            //
            // 加えて、組込コマンド (移動 / 攻撃 / 武装一覧 / 待機) と同名の
            // カスタムコマンドは builtin が優先 (重複表示を避ける)。
            let has_acted = u.has_acted;
            let builtin_names: std::collections::HashSet<&str> = items
                .iter()
                .filter_map(|it| match it {
                    UnitMenuItem::Builtin(a) => Some(a.label()),
                    UnitMenuItem::Custom { .. }
                    | UnitMenuItem::Spirit
                    | UnitMenuItem::Support(_)
                    | UnitMenuItem::Transform
                    | UnitMenuItem::Charge
                    | UnitMenuItem::Ability
                    | UnitMenuItem::Launch
                    | UnitMenuItem::Combine
                    | UnitMenuItem::Separate => None,
                })
                .collect();
            let mut seen: std::collections::HashSet<String> =
                builtin_names.iter().map(|s| s.to_string()).collect();
            const MAX_CUSTOM: usize = 10;
            let mut added = 0usize;
            // condition 評価には `&mut App` が要る (`Call(<label>)` 形式の同期実行のため)。
            // まず候補を収集してから評価する (script_library への借用を回避)。
            let candidates: Vec<(String, Option<String>)> = self
                .script_library()
                .custom_commands
                .iter()
                .filter(|c| {
                    if !c.is_unit || !custom_command_targets_party(&c.target, u.party) {
                        return false;
                    }
                    // タイミングフィルタ (SRC.NET の AsterNum チェックに相当)
                    if has_acted {
                        c.post_act_ok
                    } else if post_move {
                        c.post_move_ok
                    } else {
                        true // 移動前: 全コマンドを表示
                    }
                })
                .map(|c| (c.name.clone(), c.condition.clone()))
                .collect();
            for (name, condition) in candidates {
                if !seen.insert(name.clone()) {
                    continue;
                }
                if !crate::event_runtime::evaluate_command_condition(self, condition.as_deref()) {
                    continue;
                }
                items.push(UnitMenuItem::Custom { name });
                added += 1;
                if added >= MAX_CUSTOM {
                    break;
                }
            }
        }

        self.command_menu = Some(CommandMenu::Unit {
            uid: u.uid.clone(),
            items,
            cursor: 0,
        });
    }

    /// マップコマンドメニューを開く。
    fn open_map_menu(&mut self) {
        use crate::command_menu::{CommandMenu, MapAction};
        let mut items = vec![
            MapAction::EndTurn,
            MapAction::UnitList,
            MapAction::Settings,
            MapAction::ToggleAutoCounter,
            MapAction::ToggleWeaponWindow,
        ];
        // 「作戦目的」はシナリオが `勝利条件:` ラベルを定義している場合のみ表示する。
        if self.has_victory_condition_event() {
            items.push(MapAction::VictoryConditions);
        }
        items.push(MapAction::QuickSave);
        items.push(MapAction::QuickLoad);
        self.command_menu = Some(CommandMenu::Map { items, cursor: 0 });
    }

    /// メニュー項目が選ばれた時のアクション解決。
    fn execute_menu_action(&mut self, action: crate::command_menu::MenuActionId) -> bool {
        use crate::command_menu::{
            ActionMode, CommandMenu, MapAction, MenuActionId, UnitAction, UnitMenuItem,
        };
        match action {
            MenuActionId::Unit(a) => {
                // メニューを閉じてからモード遷移 (対象は uid で束縛)
                let uid = match &self.command_menu {
                    Some(CommandMenu::Unit { uid, .. }) => uid.clone(),
                    _ => return false,
                };
                self.command_menu = None;
                // 移動後メニューのスナップショットを退避してから action_mode をリセット。
                // 端アクション (待機 / 武装一覧 / カスタム) 後に PostMoveMenu が残ると、
                // 続くクリックが no-op になり右クリックで確定済みユニットが巻き戻るため、
                // 既定で Browse に戻し、Move/Attack のみがモード遷移で上書きする。
                let post_move_snapshot =
                    if let ActionMode::PostMoveMenu { snapshot, .. } = &self.action_mode {
                        Some(snapshot.clone())
                    } else {
                        None
                    };
                self.action_mode = ActionMode::Browse;
                // シナリオ定義コマンドは対象ユニットを束縛して本体を実行。
                let a = match a {
                    UnitMenuItem::Builtin(a) => a,
                    UnitMenuItem::Custom { name } => {
                        self.invoke_custom_unit_command(&uid, &name);
                        return true;
                    }
                    UnitMenuItem::Spirit => {
                        // 精神コマンドのサブメニューを開く (SP コマンド一覧)。
                        self.open_spirit_menu(&uid);
                        return true;
                    }
                    UnitMenuItem::Support(kind) => {
                        // 修理 / 補給: 隣接味方の対象選択へ遷移する。
                        self.begin_support_target(&uid, kind);
                        return true;
                    }
                    UnitMenuItem::Transform => {
                        // 変形: 変形先が 1 つなら即変形、複数ならサブメニューを開く。
                        self.open_transform_menu(&uid);
                        return true;
                    }
                    UnitMenuItem::Charge => {
                        // チャージ: charged フラグを立てて行動終了 (完了は次ターン)。
                        if let Some(u) = self.database.unit_by_uid_mut(&uid) {
                            u.charged = true;
                            u.has_acted = true;
                        }
                        self.push_message(
                            "チャージ開始 — 次ターンにチャージ攻撃が可能".to_string(),
                        );
                        if let Some(i) = self.database.idx_by_uid(&uid) {
                            crate::event_runtime::fire_action_end_labels(self, i);
                            // SRC `接触 <unit1> <unit2>:` ─ 行動終了後に隣接ペアで発火。
                            crate::event_runtime::fire_contact_event_labels(self, i);
                        }
                        return true;
                    }
                    UnitMenuItem::Ability => {
                        // アビリティ: 一覧サブメニューを開く。
                        self.open_ability_menu(&uid);
                        return true;
                    }
                    UnitMenuItem::Launch => {
                        // 発進: 格納ユニット選択サブメニューを開く。
                        self.open_launch_menu(&uid);
                        return true;
                    }
                    UnitMenuItem::Combine => {
                        // 合体: 合体相手を取り込み合体形態へ。発動で行動終了。
                        self.apply_combine(&uid);
                        return true;
                    }
                    UnitMenuItem::Separate => {
                        // 分離: 構成ユニットを盤上へ戻す。行動は消費しない。
                        self.apply_separate(&uid);
                        return true;
                    }
                };
                match a {
                    UnitAction::Move => {
                        self.action_mode = ActionMode::MoveSelect { uid };
                        self.push_message("移動先を選択 (右クリックでキャンセル)".to_string());
                    }
                    UnitAction::Attack => {
                        // 移動後メニューからの攻撃なら、キャンセル復帰用にスナップショットを引き継ぐ。
                        self.action_mode = ActionMode::AttackSelect {
                            uid,
                            snapshot: post_move_snapshot,
                        };
                        self.push_message("攻撃目標を選択 (右クリックでキャンセル)".to_string());
                    }
                    UnitAction::WeaponList => {
                        // 武装一覧: 情報表示のみ - 武装名をメッセージに列挙
                        if let Some(u) = self.database.unit_by_uid(&uid) {
                            if let Some(def) = self.database.unit_by_name(&u.unit_data_name) {
                                let names: Vec<String> =
                                    def.weapons.iter().map(|w| w.name.clone()).collect();
                                self.push_message(format!(
                                    "武装: {}",
                                    if names.is_empty() {
                                        "(なし)".to_string()
                                    } else {
                                        names.join(" / ")
                                    }
                                ));
                            }
                        }
                    }
                    UnitAction::Wait => {
                        // 行動終了
                        if let Some(u) = self.database.unit_by_uid_mut(&uid) {
                            u.has_acted = true;
                        }
                        if let Some(i) = self.database.idx_by_uid(&uid) {
                            crate::event_runtime::fire_action_end_labels(self, i);
                            // SRC `接触 <unit1> <unit2>:` ─ 行動終了後に隣接ペアで発火。
                            crate::event_runtime::fire_contact_event_labels(self, i);
                        }
                    }
                }
                true
            }
            MenuActionId::Map(a) => {
                self.command_menu = None;
                match a {
                    MapAction::EndTurn => self.end_phase(),
                    MapAction::UnitList => {
                        self.scene = Scene::PilotList;
                        true
                    }
                    MapAction::Settings => {
                        self.enter_configuration();
                        true
                    }
                    MapAction::ToggleAutoCounter => {
                        let on = self.toggle_auto_counter();
                        self.push_message(format!(
                            "自動反撃モード: {}",
                            if on {
                                "ＯＮ (自動反撃)"
                            } else {
                                "ＯＦＦ (手動選択)"
                            }
                        ));
                        true
                    }
                    MapAction::ToggleWeaponWindow => {
                        let on = self.toggle_weapon_window();
                        self.push_message(format!(
                            "武器選択ウィンドウ: {}",
                            if on {
                                "ＯＮ (攻撃時に選択)"
                            } else {
                                "ＯＦＦ (自動/Wキー)"
                            }
                        ));
                        true
                    }
                    MapAction::VictoryConditions => {
                        // シナリオ定義の `勝利条件:` ラベルを発火する。メニューには
                        // ラベル定義時のみ現れるが、発火に失敗した場合の保険も置く。
                        if !self.fire_victory_condition_event() {
                            self.push_message("勝利条件は設定されていません".to_string());
                        }
                        true
                    }
                    MapAction::QuickSave => {
                        // 現在の状態を JSON 化して script_var `__quicksave` に
                        // 保存。フロントエンド (src-web) は同 var を localStorage
                        // に永続化する責務を持つ。
                        if let Ok(json) = self.to_save_json() {
                            self.set_script_var("__quicksave".to_string(), json);
                            self.push_message("【クイックセーブ】".to_string());
                        }
                        true
                    }
                    MapAction::QuickLoad => {
                        // 直前のクイックセーブを復元。本実装は self を置換できない
                        // ため、フロントエンドが `script_var("__quicksave")` から
                        // JSON を取り出し `App::from_save_json` で復元 + `fire_resume_event`
                        // を呼ぶ責務を持つ。ここでは表示のみ。
                        if !self.script_var("__quicksave").is_empty() {
                            self.push_message("【クイックロード要求】".to_string());
                        } else {
                            self.push_message("クイックセーブデータがありません".to_string());
                        }
                        true
                    }
                }
            }
        }
    }

    /// `from` のユニットを `to` に移動できれば実施。返り値は実行有無。
    /// 条件: 同マス (`from` -> `to`) で別、from にユニット、to にユニットなし、
    /// to が from から MP 内、ユニットの所属が現フェーズと一致。
    fn try_move_unit_to(&mut self, from: (u32, u32), to: (u32, u32)) -> bool {
        let unit_idx = self
            .database
            .unit_instances
            .iter()
            .position(|u| u.x == from.0 && u.y == from.1);
        let Some(idx) = unit_idx else {
            return false;
        };

        // 現フェーズの所属ユニットのみ移動可能
        if self.database.unit_instances[idx].party != self.turn.phase.party() {
            return false;
        }

        // to に既に別ユニットがいるなら移動不可
        let occupied = self
            .database
            .unit_instances
            .iter()
            .enumerate()
            .any(|(i, u)| i != idx && u.x == to.0 && u.y == to.1);
        if occupied {
            return false;
        }

        // 地形適応・搭乗種別・特殊能力・装備込み移動力を考慮した到達範囲。
        // 描画 (move range overlay) と同一の GameDatabase::unit_move_range を使う。
        let uid = self.database.unit_instances[idx].uid.clone();
        let range = self.database.unit_move_range(&uid);
        if !range.contains_key(&to) {
            return false;
        }

        // 移動を反映 (pos_index 同期は move_unit に一元化)
        self.database.move_unit(&uid, to.0, to.1);
        self.database.unit_instances[idx].has_moved = true;
        // SRC `進入 <unit> <x> <y>:` (`進入イベント.md`) + 端到達なら `脱出 <unit> <dir>`
        crate::event_runtime::fire_entry_event_labels(self, idx);
        true
    }

    /// `uid` のユニットを移動前スナップショットへ巻き戻し、`has_moved` をクリアする。
    /// 移動前の座標・EN 消費・現在エリアを復元する (SRC.Sharp の cancel 復帰相当)。
    fn rollback_move(&mut self, uid: &str, snapshot: &crate::command_menu::MoveSnapshot) {
        self.database.move_unit(uid, snapshot.x, snapshot.y);
        if let Some(u) = self.database.unit_by_uid_mut(uid) {
            u.en_consumed = snapshot.en_consumed;
            u.current_area = snapshot.current_area.clone();
            u.has_moved = false;
        }
    }

    /// インターミッション画面に表示する項目数。ユーザ定義 + 「次のステージへ」。
    /// 「次のステージへ」はシナリオが `Continue <file>` で `次ステージ` を
    /// セットしている時のみ末尾に追加される (SRC.Sharp 同等)。
    pub fn intermission_item_count(&self) -> usize {
        match self.intermission_mode {
            IntermissionMode::Menu => self.intermission_menu_items().len(),
            // 機体改造: 対象ユニット + 末尾「戻る」。
            IntermissionMode::UnitUpgrade => self.intermission_upgrade_units().len() + 1,
            // 換装: (ユニット → 換装先) 行 + 末尾「戻る」。
            IntermissionMode::EquipSwap => self.intermission_swap_rows().len() + 1,
            // 乗り換え: 移動元選択は全味方、移動先選択は移動元を除く + 末尾「戻る」。
            IntermissionMode::RideChange => self.ride_change_units().len() + 1,
        }
    }

    /// メインメニューの表示項目を順序付きで構築する。組込みコマンド (機体改造 /
    /// データセーブ) は、表示すべきインターミッション (ユーザ定義項目あり、または
    /// 「次のステージへ」あり) のときだけ追加する。空メニューには足さない。
    fn intermission_menu_items(&self) -> Vec<InterItem> {
        let mut items: Vec<InterItem> = (0..self.intermission_commands.len())
            .map(InterItem::User)
            .collect();
        let has_next = !self.script_var("次ステージ").is_empty();
        if !self.intermission_commands.is_empty() || has_next {
            items.push(InterItem::Upgrade);
            // 換装: 換装可能な味方ユニットが居るときのみ。
            if !self.intermission_swap_rows().is_empty() {
                items.push(InterItem::EquipSwap);
            }
            // 乗り換え: シナリオが `Option 乗り換え` で有効化し、かつ入れ替え先がある
            // (味方ユニット 2 体以上) ときのみ表示 (`乗り換え.md` / `Optionコマンド.md`)。
            if !self.script_var("Option(乗り換え)").is_empty()
                && self.intermission_upgrade_units().len() >= 2
            {
                items.push(InterItem::RideChange);
            }
            // ステータス: 味方ユニットが居れば部隊ロスター閲覧を出す。
            if !self.intermission_upgrade_units().is_empty() {
                items.push(InterItem::Status);
            }
            items.push(InterItem::Save);
        }
        if has_next {
            items.push(InterItem::NextStage);
        }
        items
    }

    /// 機体改造で対象に取れる味方ユニットの `unit_instances` index 列。
    fn intermission_upgrade_units(&self) -> Vec<usize> {
        self.database
            .unit_instances
            .iter()
            .enumerate()
            .filter(|(_, u)| u.party == crate::Party::Player)
            .map(|(i, _)| i)
            .collect()
    }

    /// 0..intermission_item_count() の n に対応する表示文字列。
    pub fn intermission_item_label(&self, n: usize) -> Option<String> {
        match self.intermission_mode {
            IntermissionMode::Menu => {
                let items = self.intermission_menu_items();
                items.get(n).map(|it| match it {
                    InterItem::User(i) => self.intermission_commands[*i].name.clone(),
                    InterItem::Upgrade => "機体改造".to_string(),
                    InterItem::EquipSwap => "換装".to_string(),
                    InterItem::RideChange => "乗り換え".to_string(),
                    InterItem::Status => "ステータス".to_string(),
                    InterItem::Save => "データセーブ".to_string(),
                    InterItem::NextStage => {
                        crate::scene::intermission::NEXT_STAGE_LABEL.to_string()
                    }
                })
            }
            IntermissionMode::UnitUpgrade => {
                let units = self.intermission_upgrade_units();
                if let Some(&idx) = units.get(n) {
                    let u = &self.database.unit_instances[idx];
                    let name = self.unit_display_name(u);
                    let lv = u.upgrade_level;
                    if lv >= crate::db::UPGRADE_MAX_LEVEL {
                        Some(format!("{name}  改造Lv{lv} (最大)"))
                    } else {
                        Some(format!(
                            "{name}  改造Lv{lv} → 次 {}G",
                            Self::upgrade_cost(lv)
                        ))
                    }
                } else if n == units.len() {
                    Some("← 戻る".to_string())
                } else {
                    None
                }
            }
            IntermissionMode::EquipSwap => {
                let rows = self.intermission_swap_rows();
                if let Some((uid, form)) = rows.get(n) {
                    let cur = self
                        .database
                        .unit_by_uid(uid)
                        .map(|u| self.unit_display_name(u))
                        .unwrap_or_default();
                    Some(format!("{cur} → {form}"))
                } else if n == rows.len() {
                    Some("← 戻る".to_string())
                } else {
                    None
                }
            }
            IntermissionMode::RideChange => {
                let uids = self.ride_change_units();
                if let Some(uid) = uids.get(n) {
                    let label = self.ride_unit_label(uid);
                    // 移動元未選択なら一覧、選択済みなら移動先候補 (→ prefix)。
                    if self.ride_change_source.is_some() {
                        Some(format!("→ {label}"))
                    } else {
                        Some(label)
                    }
                } else if n == uids.len() {
                    Some("← 戻る".to_string())
                } else {
                    None
                }
            }
        }
    }

    /// 乗り換えの選択対象 uid 列。移動元未選択なら全味方、選択済みなら移動元を除く。
    fn ride_change_units(&self) -> Vec<String> {
        self.database
            .unit_instances
            .iter()
            .filter(|u| u.party == crate::Party::Player)
            .filter(|u| self.ride_change_source.as_deref() != Some(u.uid.as_str()))
            .map(|u| u.uid.clone())
            .collect()
    }

    /// 乗り換えリストの 1 行表示「ユニット名 (搭乗: パイロット)」。
    fn ride_unit_label(&self, uid: &str) -> String {
        let Some(u) = self.database.unit_by_uid(uid) else {
            return String::new();
        };
        let unit = self.unit_display_name(u);
        let pilot = if u.pilot_name.is_empty() {
            "無人".to_string()
        } else {
            u.pilot_name.clone()
        };
        format!("{unit} (搭乗: {pilot})")
    }

    /// 機体改造 Lv `level` → `level+1` への必要資金。
    fn upgrade_cost(level: i32) -> i64 {
        i64::from(level + 1) * 1000
    }

    /// ユニットの表示名 (nickname があればそれ、無ければ unit_data_name)。
    fn unit_display_name(&self, u: &crate::UnitInstance) -> String {
        self.database
            .unit_by_name(&u.unit_data_name)
            .map(|d| {
                if d.nickname.is_empty() {
                    d.name.clone()
                } else {
                    d.nickname.clone()
                }
            })
            .unwrap_or_else(|| u.unit_data_name.clone())
    }

    /// 機体改造を 1 段階適用する (資金チェック + 消費 + `upgrade_level` 加算)。
    fn apply_unit_upgrade(&mut self, idx: usize) {
        let lv = self.database.unit_instances[idx].upgrade_level;
        let name = self.unit_display_name(&self.database.unit_instances[idx]);
        if lv >= crate::db::UPGRADE_MAX_LEVEL {
            self.push_message(format!("{name} はこれ以上改造できません (最大 Lv{lv})"));
            return;
        }
        let cost = Self::upgrade_cost(lv);
        if self.money() < cost {
            self.push_message(format!(
                "資金が足りません ({name} 改造: 必要 {cost}G / 所持 {}G)",
                self.money()
            ));
            return;
        }
        self.add_money(-cost);
        self.database.unit_instances[idx].upgrade_level = lv + 1;
        self.push_message(format!("{name} を改造しました (Lv{} / -{cost}G)", lv + 1));
    }

    /// ユニット `u` の特殊能力「換装」が持つ換装先フォーム名の列を返す。
    /// 書式 `換装=<換装先1> <換装先2> …`（`換装.md`）。`active_features` を優先し、
    /// 空なら現フォームの `UnitData.features` から引く。DB に存在する形態のみ採用し、
    /// `非表示` トークンは除外する。
    fn swap_forms_for(&self, u: &crate::UnitInstance) -> Vec<String> {
        let val = crate::feature::feature_value(&u.active_features, "換装")
            .map(str::to_string)
            .or_else(|| {
                self.database.unit_by_name(&u.unit_data_name).and_then(|d| {
                    d.features
                        .iter()
                        .find(|(n, _)| n == "換装")
                        .map(|(_, v)| v.clone())
                })
            });
        let Some(val) = val else {
            return Vec::new();
        };
        val.split_whitespace()
            .filter(|t| *t != "非表示" && self.database.unit_by_name(t).is_some())
            .map(str::to_string)
            .collect()
    }

    /// 換装サブモードの行: 換装可能な味方ユニットの各換装先を平坦化した
    /// `(uid, 換装先フォーム名)` の列。
    fn intermission_swap_rows(&self) -> Vec<(String, String)> {
        let mut rows = Vec::new();
        for u in &self.database.unit_instances {
            if u.party != crate::Party::Player {
                continue;
            }
            for f in self.swap_forms_for(u) {
                rows.push((u.uid.clone(), f));
            }
        }
        rows
    }

    /// 換装を適用する。`set_unit_form` で形態を差し替え（資金消費なし、イベント無し）。
    fn apply_equip_swap(&mut self, uid: &str, new_form: &str) {
        let name = self
            .database
            .unit_by_uid(uid)
            .map(|u| self.unit_display_name(u))
            .unwrap_or_default();
        // 換装先の形態の必要技能 (搭乗員が満たさないと換装不可)。
        if let Some(uidx) = self.database.idx_by_uid(uid) {
            if !self.form_skill_ok(uidx, new_form) {
                self.push_message(format!(
                    "必要技能を満たさないため {new_form} に換装できません"
                ));
                return;
            }
        }
        if self.set_unit_form(uid, new_form) {
            let new_name = self
                .database
                .unit_by_name(new_form)
                .map(|d| {
                    if d.nickname.is_empty() {
                        d.name.clone()
                    } else {
                        d.nickname.clone()
                    }
                })
                .unwrap_or_else(|| new_form.to_string());
            self.push_message(format!("{name} を {new_name} に換装しました"));
        }
    }

    /// インターミッションの「データセーブ」。`to_save_json()` を `__quicksave` に
    /// 格納する (マップコマンドの QuickSave と同経路)。フロントエンドが永続化する。
    /// データセーブを実行する。現在の状態を JSON 化し `__quicksave` script_var に
    /// 保存する (フロントエンドが localStorage に永続化する責務を持つ)。インター
    /// ミッションメニューの「データセーブ」と `.eve CallIntermissionCommand データセーブ`
    /// の両経路から共有する。
    pub(crate) fn intermission_data_save(&mut self) {
        match self.to_save_json() {
            Ok(json) => {
                self.set_script_var("__quicksave".to_string(), json);
                self.push_message("データをセーブしました".to_string());
            }
            Err(e) => self.push_message(format!("セーブに失敗しました: {e}")),
        }
    }

    /// ユーザ定義インターミッションコマンドの `.eve` (`プロローグ`) を起動する。
    /// サブコマンド実行中は MapView に切り替え、完了で Intermission に戻る。
    fn run_user_intermission_command(&mut self, cmd_idx: usize) {
        let file = self.intermission_commands[cmd_idx].file.clone();
        self.intermission_running = true;
        self.scene = Scene::MapView;
        self.script_overlay.clear();
        self.hotpoints.clear();
        self.flow
            .push(crate::flow::FlowCont::ReturnToIntermissionMenu);
        let mut started = crate::event_runtime::trigger_label_in_file(self, &file, "プロローグ");
        if !started {
            if let Some(entry) = self.script_library.find_file(&file) {
                let pc = entry.start_pc;
                let _ = crate::event_runtime::run_from_pc(self, pc);
                started = true;
            }
        }
        if !started && self.flow.last() == Some(&crate::flow::FlowCont::ReturnToIntermissionMenu) {
            self.flow.pop();
            self.return_from_intermission_subcommand_if_idle();
        }
    }

    /// 「次のステージへ」を確定する。`advance_to_next_stage` で次シナリオへ進む。
    /// `Continue` で次ステージを再予約した場合は scene=Intermission を尊重する。
    fn select_next_stage(&mut self) -> bool {
        self.intermission_running = false;
        if self.advance_to_next_stage() {
            if !self.script_var("次ステージ").is_empty() {
                return true;
            }
            self.scene = Scene::MapView;
            return true;
        }
        false
    }

    /// 現在カーソル位置の項目を実行。モードに応じて分岐する。
    fn confirm_intermission_selection(&mut self) -> bool {
        match self.intermission_mode {
            IntermissionMode::Menu => {
                let items = self.intermission_menu_items();
                let Some(&item) = items.get(self.intermission_cursor) else {
                    return false;
                };
                match item {
                    InterItem::User(i) => {
                        self.run_user_intermission_command(i);
                        true
                    }
                    InterItem::Upgrade => {
                        self.intermission_mode = IntermissionMode::UnitUpgrade;
                        self.intermission_cursor = 0;
                        true
                    }
                    InterItem::EquipSwap => {
                        self.intermission_mode = IntermissionMode::EquipSwap;
                        self.intermission_cursor = 0;
                        true
                    }
                    InterItem::RideChange => {
                        self.intermission_mode = IntermissionMode::RideChange;
                        self.ride_change_source = None;
                        self.intermission_cursor = 0;
                        true
                    }
                    InterItem::Status => {
                        // 味方ロスター先頭機の単機ステータス詳細を開く。`◀ / ▶` で
                        // 巡回し、閉じる / Esc / 右クリックでインターミッションへ戻る。
                        self.status_detail_index = 0;
                        self.scene = Scene::UnitDetail;
                        self.scene_return_to = Some(Scene::Intermission);
                        true
                    }
                    InterItem::Save => {
                        self.intermission_data_save();
                        true
                    }
                    InterItem::NextStage => self.select_next_stage(),
                }
            }
            IntermissionMode::UnitUpgrade => {
                let units = self.intermission_upgrade_units();
                if let Some(&idx) = units.get(self.intermission_cursor) {
                    self.apply_unit_upgrade(idx);
                    true
                } else {
                    // 末尾「戻る」: メインメニューへ。
                    self.intermission_mode = IntermissionMode::Menu;
                    self.intermission_cursor = 0;
                    true
                }
            }
            IntermissionMode::EquipSwap => {
                let rows = self.intermission_swap_rows();
                if let Some((uid, form)) = rows.get(self.intermission_cursor).cloned() {
                    self.apply_equip_swap(&uid, &form);
                    // カーソルが範囲外にならないようクランプ (換装で行数が変わりうる)。
                    let count = self.intermission_item_count();
                    if self.intermission_cursor >= count {
                        self.intermission_cursor = count.saturating_sub(1);
                    }
                    true
                } else {
                    self.intermission_mode = IntermissionMode::Menu;
                    self.intermission_cursor = 0;
                    true
                }
            }
            IntermissionMode::RideChange => {
                let uids = self.ride_change_units();
                let Some(uid) = uids.get(self.intermission_cursor).cloned() else {
                    // 末尾「戻る」: 移動元選択済みなら解除して移動元選択へ、
                    // そうでなければメインメニューへ。
                    if self.ride_change_source.take().is_some() {
                        self.intermission_cursor = 0;
                    } else {
                        self.intermission_mode = IntermissionMode::Menu;
                        self.intermission_cursor = 0;
                    }
                    return true;
                };
                match self.ride_change_source.take() {
                    None => {
                        // 移動元を確定 → 移動先選択へ。
                        self.ride_change_source = Some(uid);
                        self.intermission_cursor = 0;
                    }
                    Some(src) => {
                        // 移動先を確定 → 搭乗パイロットを入れ替え。
                        self.apply_ride_change(&src, &uid);
                        self.intermission_cursor = 0;
                    }
                }
                true
            }
        }
    }

    /// 2 ユニットの搭乗パイロット (`pilot_name` + `pilot_ids`) を入れ替える。
    fn swap_unit_pilots(&mut self, a: usize, b: usize) {
        let an = std::mem::take(&mut self.database.unit_instances[a].pilot_name);
        let ai = std::mem::take(&mut self.database.unit_instances[a].pilot_ids);
        let bn = std::mem::take(&mut self.database.unit_instances[b].pilot_name);
        let bi = std::mem::take(&mut self.database.unit_instances[b].pilot_ids);
        self.database.unit_instances[a].pilot_name = bn;
        self.database.unit_instances[a].pilot_ids = bi;
        self.database.unit_instances[b].pilot_name = an;
        self.database.unit_instances[b].pilot_ids = ai;
    }

    /// 乗り換え: 2 ユニットの搭乗パイロットを入れ替える。
    /// 片方が無人でも入れ替え可 (空ユニットが生じうる点は SRC と同じ player 責務)。
    /// 形態の必要技能 (`必要技能.md` §4: 乗り換え先のパイロット制限) を満たさない組合せは
    /// 入れ替え後にチェックして revert する。インターミッション (off_map) では地形/隣接
    /// 条件は fail-open となり、実質パイロット技能条件のみが効く (原典の「常に満たす」に整合)。
    fn apply_ride_change(&mut self, src_uid: &str, dst_uid: &str) {
        if src_uid == dst_uid {
            return;
        }
        let (Some(si), Some(di)) = (
            self.database.idx_by_uid(src_uid),
            self.database.idx_by_uid(dst_uid),
        ) else {
            return;
        };
        let src_name = self.unit_display_name(&self.database.unit_instances[si]);
        let dst_name = self.unit_display_name(&self.database.unit_instances[di]);
        self.swap_unit_pilots(si, di);
        // 入れ替え後の搭乗員が双方の形態の必要技能を満たすか。満たさなければ revert。
        let src_form = self.database.unit_instances[si].unit_data_name.clone();
        let dst_form = self.database.unit_instances[di].unit_data_name.clone();
        if !self.form_skill_ok(si, &src_form) || !self.form_skill_ok(di, &dst_form) {
            self.swap_unit_pilots(si, di);
            self.push_message(format!(
                "必要技能を満たさないため {src_name} と {dst_name} を乗り換えできません"
            ));
            return;
        }
        self.push_message(format!("{src_name} と {dst_name} の搭乗を入れ替えました"));
    }

    /// 配置済み (on_map) の最初の味方ユニットへカーソルを置き、ビューをスクロール
    /// して画面内に収める。戦闘開始時に味方が画面外に配置されていても見えるように。
    fn center_view_on_first_player_unit(&mut self) {
        if let Some((x, y)) = self
            .database
            .unit_instances
            .iter()
            .find(|u| u.party == crate::Party::Player && !u.off_map)
            .map(|u| (u.x, u.y))
        {
            self.map_cursor = Some((x, y));
            self.ensure_cursor_visible();
        }
    }

    /// インターミッション項目から起動したサブコマンド (.eve) のスクリプトが
    /// 完了したら `Scene::Intermission` に戻す。スクリプトがまだ pending
    /// 原典 SRC 互換: Prologue が完了したら、ユーザ入力を待たずに
    /// `スタート` ラベルを発火して Battle 状態へ移行する。
    ///
    /// 原典 [`スタートイベント.md`](../../SRC.Sharp/SRC.Sharp.Help/src/スタートイベント.md):
    /// 「プロローグイベントの後にメインウィンドウが表示され、スタートイベントが
    /// 発生します。スタートイベントが終了するとプレイヤーの操作が可能になり、
    /// 戦闘が開始されます」— ユーザに `Enter で出撃準備` を押させる仕様は無い。
    ///
    /// idle (`script_ctx` 無し / `pending_dialog` 無し / `pending_timer` 無し /
    /// インターミッション中でない) のときに Briefing → Sortie → Battle を
    /// 一気に進める。
    /// `respond_dialog` / `respond_dialog_text` / `tick` の resume 後 と
    /// `start_scenario` 末尾から呼ばれる。
    pub fn auto_progress_stage_state_if_idle(&mut self) {
        // flow 継続が残っていれば on_script_completed が処理する。
        // ここで begin_battle を呼ぶと スタート ラベルが二重発火する。
        if !self.flow.is_empty() {
            return;
        }
        // シナリオが実際に起動しているかの判定:
        //  - `stage` 非空: `Stage` コマンド / `start_scenario` で表示名が付いている。
        //  - `current_stage_file` 非空: `Continue` チェインで本編ステージファイルに
        //    入っている (`advance_to_next_stage` が設定)。
        // どちらも空 = テストで `App::new()` 直後に直接スクリプトを動かす類の合成
        // ケースなので auto-progress しない (期待を壊さない)。
        // 旧実装は `stage` のみで判定していたが、`Stage` コマンドを使わず `Continue`
        // チェインだけで本編に入るシナリオ (musou 系) は `stage` が空のままで、本編
        // 突入後も自動進行できず「マップが読み込まれていません」で詰んでいた。
        if self.stage.is_empty() && self.current_stage_file.is_empty() {
            return;
        }
        // 「インターミッション中」の判定は `intermission_running` (サブコマンド実行中)
        // と `scene == Intermission` (メニュー表示中) で行う。
        // `intermission_commands` の登録有無は判定に使わない: インターミッション制
        // シナリオでは登録がシナリオ全体で残り続けるため、「登録あり = 進めない」に
        // すると本編 (MapView) に入った後も Briefing のまま `スタート` が発火せず
        // 詰む (musou 系の `Continue` チェイン後の症状)。
        if self.has_script_context()
            || self.pending_dialog.is_some()
            || self.pending_timer.is_some()
            || self.intermission_running
            || matches!(self.scene, Scene::Intermission)
        {
            return;
        }
        if self.stage_state == crate::stage::StageState::Briefing {
            self.begin_sortie();
        }
        // begin_battle は `スタート` ラベルを発火する。それが Talk 等で
        // suspend したら以降の自動進行は止まり、ユーザが応答 → resume → 再度
        // ここに戻ってくる経路で進む。
        if self.stage_state == crate::stage::StageState::Sortie {
            self.begin_battle();
        }
    }

    /// (dialog / timer / 中断コンテキスト) なら何もしない。
    ///
    /// `respond_dialog` / `respond_dialog_text` / `tick` の resume 後に
    /// 呼ばれ、サブコマンドの Exit を検出してメニューに復帰させる。
    pub fn return_from_intermission_subcommand_if_idle(&mut self) {
        if !self.intermission_running {
            return;
        }
        let idle = self.script_ctx.is_none()
            && self.pending_dialog.is_none()
            && self.pending_timer.is_none();
        if idle {
            self.intermission_running = false;
            self.scene = Scene::Intermission;
            self.intermission_mode = IntermissionMode::Menu;
            self.script_overlay.clear();
            self.hotpoints.clear();
        }
    }

    fn handle_click_intermission(&mut self, x: i32, y: i32) -> bool {
        let count = self.intermission_item_count();
        if count == 0 {
            return false;
        }
        let Some(idx) = crate::scene::intermission::hit_item(x, y, count) else {
            return false;
        };
        self.intermission_cursor = idx;
        self.confirm_intermission_selection()
    }

    fn handle_click_configuration(&mut self, x: i32, y: i32) -> bool {
        let layout = configuration::ConfigurationLayout::original();
        let Some(hit) = configuration::hit_test(&layout, x, y) else {
            return false;
        };
        use configuration::HitTarget::*;
        match hit {
            Checkbox(field) => {
                field.toggle(&mut self.settings);
                true
            }
            MessageSpeedCombo => {
                self.settings.message_speed = self.settings.message_speed.next();
                true
            }
            MidiResetCombo => {
                self.settings.midi_reset = self.settings.midi_reset.next();
                true
            }
            Mp3VolumeBar { ratio } => {
                self.settings.mp3_volume = (ratio * 100.0).round().clamp(0.0, 100.0) as u8;
                true
            }
            OkButton => {
                self.scene = Scene::MapView;
                self.settings_snapshot = None;
                true
            }
            CancelButton => {
                if let Some(prev) = self.settings_snapshot.take() {
                    self.settings = prev;
                }
                self.scene = Scene::MapView;
                true
            }
        }
    }
}

#[cfg(test)]
mod tests;
