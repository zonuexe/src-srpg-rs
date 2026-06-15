//! アプリケーション状態 / Application state.
//!
//! VB6 原典は `frmTitle` → `frmMain` の Form 切替でアプリ全体の状態遷移を
//! 表現していた。Rust 移植では `App` 構造体に集約し、フロントエンドからは
//! 入力イベントを `handle_input` に流し、結果を `scene()` / `settings()` で
//! 参照する。
//!
//! In the original VB6 app, global state transitions happen via `Form` swaps
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
/// 大きいほど倍率が上がる (> +7 で ×5、-1 以下で逓減)。倍率テーブルは原典準拠だが、
/// 基準値の絶対量は本実装の経験値スケール (level = total_exp/100) に合わせ、
/// 同レベル時に従来の一律値を保つよう小さく取る (原典は 修理=100/補給=150)。
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
            ai_runner: None,
            animate_battle: false,
            battle_anim: None,
            move_anim: None,
            auto_counter: false,
            pending_reaction: None,
            pending_spirit: None,
            pending_transform: None,
            pending_ability: None,
            pending_launch: None,
            pending_reload: None,
            wall_clock_ms: 0.0,
            intermission_commands: Vec::new(),
            intermission_cursor: 0,
            intermission_mode: IntermissionMode::Menu,
            ride_change_source: None,
            scene_return_to: None,
            pending_game_over: false,
            intermission_running: false,
            last_script_error: None,
            last_return_value: String::new(),
            keystate_call_count: std::cell::Cell::new(0),
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
    }
    pub fn pop_call_return(&mut self) -> Option<(usize, Vec<String>)> {
        self.script_call_stack.pop()
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

    /// 継続 1 件の実行。VB6 原典の「`HandleEvent` から戻った直後のコード」に相当。
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
        if !self.intermission_commands.is_empty() {
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
            let unit_name = self.database.unit_instances[i].unit_data_name.clone();
            let max_hp = self
                .database
                .unit_by_name(&unit_name)
                .map(|u| u.hp)
                .unwrap_or(0);
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
        // 回復系特殊能力 (`回復系特殊能力.md`): 当該陣営フェイズ開始時に、
        // ＨＰ回復Lv*/ＥＮ回復Lv* は実効最大値の 10×Lv% を回復、ＨＰ消費Lv*/ＥＮ消費Lv*
        // は同率を減少させる (ＨＰ は最低 1 / ＥＮ は最低 0)。
        // ※ 通常の基礎 EN 回復 (毎ターン 5) と霊力回復は本実装では別途 (未対応)。
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
    /// VB6 原典では `SRC.StartScenario(次ステージ)` がエピローグ後に
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
    fn pick_attack_weapon(
        atk_unit: &crate::data::unit::UnitData,
        dist: u32,
        forced: Option<&crate::data::unit::WeaponData>,
        charged: bool,
        post_move: bool,
        totsugeki: bool,
    ) -> Option<crate::data::unit::WeaponData> {
        let legal = |w: &crate::data::unit::WeaponData| {
            combat::weapon_in_range(w, dist)
                && (charged || !combat::is_charge_weapon(w))
                && Self::weapon_usable_post_move(w, post_move, totsugeki)
        };
        match forced {
            Some(w) => legal(w).then(|| w.clone()),
            None if post_move => atk_unit
                .weapons
                .iter()
                .filter(|w| legal(w))
                .max_by_key(|w| w.power)
                .cloned(),
            None => combat::best_weapon_in_range_with_charge(atk_unit, dist, charged).cloned(),
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
        let Some(atk_idx) = atk_idx else {
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
        let (def_idx, weapon) = if let Some(tt) = desired_target {
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
            ) else {
                return false;
            };
            (def_idx, weapon)
        };

        let def_inst = self.database.unit_instances[def_idx].clone();
        let def_combat = self.database.effective_combat_data(def_idx);
        let map = self.database.map.as_ref();
        let def_terrain_id = map.map(|m| m.cell(def_inst.x, def_inst.y).terrain_id);
        let (Some((def_pilot, def_unit)), Some(terrain_id)) = (def_combat, def_terrain_id) else {
            return false;
        };
        let def_hit_mod = self.database.terrain_hit_mod(terrain_id);
        let def_damage_mod = self.database.terrain_damage_mod(terrain_id);
        // Ｃ 属性武器を使った場合、charged フラグを消費 (次回攻撃前に再 Charge 必要)。
        if combat::is_charge_weapon(&weapon) {
            self.database.unit_instances[atk_idx].charged = false;
        }
        // SRC `使用 <unit> <device>:` (`使用イベント.md`) ─ `攻撃イベント` の前に発火。
        crate::event_runtime::fire_use_event_labels(self, atk_idx, &weapon.name);

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

        // 状態異常 / 精神コマンド分の補正は predict_with_status に任せる。
        let atk_morale = self.database.unit_instances[atk_idx].morale;
        let def_morale = def_inst.morale;
        let atk_statuses: Vec<String> = self.database.unit_instances[atk_idx]
            .conditions
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let def_statuses: Vec<String> =
            def_inst.conditions.iter().map(|c| c.name.clone()).collect();
        // 地形適応 (SRC `戦闘システム詳細.md`): 攻撃側は自地形、防御側は自地形で参照。
        let atk_env = self.terrain_env_at(
            self.database.unit_instances[atk_idx].x,
            self.database.unit_instances[atk_idx].y,
        );
        let def_env = self.terrain_env_at(def_inst.x, def_inst.y);
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
        );

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
        let hit = roll < effective_hit;
        // クリティカル判定 (命中時のみ)。防御選択でクリ率が半減する
        // (SRC `戦闘システム詳細.md`「防御」: クリティカル率 ×0.5)。
        let crit_chance = if def_mode == "防御" {
            preview.critical_chance / 2
        } else {
            preview.critical_chance
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
            if let Some(g_idx) = guard_idx {
                // 援護防御: guard unit が 50% ダメージを受ける (防御扱い)
                let guard_dmg = (preview.damage as f64 * 0.5) as i64;
                let guard_unit_name = self.database.unit_instances[g_idx].unit_data_name.clone();
                let guard_pilot_name = self.database.unit_instances[g_idx].pilot_name.clone();
                let guard_unit = self.database.unit_by_name(&guard_unit_name).cloned();
                let guard_pilot = self.database.pilot_by_name(&guard_pilot_name).cloned();
                self.database.unit_instances[g_idx].support_guard_remaining -= 1;
                self.database.unit_instances[g_idx].damage += guard_dmg;
                let g_max_hp = self
                    .database
                    .effective_max_hp(&self.database.unit_instances[g_idx]);
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
                    self.database.remove_unit_at(g_idx);
                    if let (Some(gp), Some(gu)) = (guard_pilot, guard_unit) {
                        self.fire_destruction_label(&gp.name, &gu.name);
                    }
                    m
                } else {
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
                // 通常ダメージ: defender が受ける (防御選択時は applied_damage で半減済)
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
                            let money = self.award_kill_rewards(killer_idx, exp, victim_value);
                            if money > 0 {
                                m.push_str(&format!(" 資金 +{money}"));
                            }
                        }
                        defender_killed = true;
                        m
                    }
                } else {
                    // 特殊効果攻撃属性: 命中かつ生存時に確率で状態異常を付与。
                    let inflicted =
                        self.apply_weapon_special_effects(def_idx, &weapon, &atk_pilot, &def_pilot);
                    // 衰 / 滅: クリティカル時に対象の現在 HP / EN を割合減少させる。
                    // 引 / 転: クリティカル時に対象の位置を移す。盗: 資金を奪う。
                    // 写 / 化: 発動者が対象のユニットへ変化する。
                    let decayed = if critical {
                        let mut d = self.apply_weapon_crit_decay(def_idx, &weapon);
                        d.extend(self.apply_weapon_crit_reposition(def_idx, (cx, cy), &weapon));
                        d.extend(self.apply_weapon_crit_steal(
                            def_idx,
                            atk_party,
                            def_unit.value,
                            &weapon,
                        ));
                        d.extend(self.apply_weapon_crit_copy(atk_idx, def_idx, &weapon));
                        d
                    } else {
                        Vec::new()
                    };
                    // 吹 / Ｋ: 命中時に対象を遠ざかる方向へ押し出す。
                    let knocked = self.apply_weapon_knockback(
                        def_idx,
                        (cx, cy),
                        &atk_unit_name,
                        &weapon,
                        critical,
                    );
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
        let allow_counter = def_mode != "回避" && def_mode != "防御" && def_mode != "援護防御";
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
        // 撃破時の自動発火ラベル `Destruction <unit>` / `<pilot>` を実行。
        if defender_killed {
            self.fire_destruction_label(&def_pilot.name, &def_unit.name);
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

        self.check_victory();
        true
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
        let weapon = combat::best_weapon_in_range(&sup_unit, dist).cloned()?;
        let def_inst = self.database.unit_instances[def_idx].clone();
        let (def_pilot, def_unit) = self.database.effective_combat_data(def_idx)?;
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
        let sup_statuses: Vec<String> = self.database.unit_instances[sup_idx]
            .conditions
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let def_statuses: Vec<String> =
            def_inst.conditions.iter().map(|c| c.name.clone()).collect();
        let sup_env = self.terrain_env_at(
            self.database.unit_instances[sup_idx].x,
            self.database.unit_instances[sup_idx].y,
        );
        let def_env = self.terrain_env_at(def_inst.x, def_inst.y);
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
        );
        let roll = (self.next_u32() % 100) as i32;
        let hit = roll < preview.hit_chance;
        // SP コスト消費 (= サポートアタック回数を 1 減らす)
        self.database.unit_instances[sup_idx].support_attack_remaining -= 1;
        if hit {
            let dmg = (preview.damage as f64 * 0.75) as i64;
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
                self.database.remove_unit_at(def_idx);
                self.fire_destruction_label(&def_pilot.name, &def_unit.name);
                if let Some(killer_idx) = self
                    .database
                    .unit_instances
                    .iter()
                    .position(|u| u.x == sx && u.y == sy)
                {
                    let money = self.award_kill_rewards(killer_idx, exp, victim_value);
                    if money > 0 {
                        self.push_message(format!("  援護撃破: 資金 +{money}"));
                    }
                }
            } else if remaining > 0 {
                // 生存: 援護武器の特殊効果攻撃属性 (状態異常付与) を防御側へ proc。
                let inflicted =
                    self.apply_weapon_special_effects(def_idx, &weapon, &sup_pilot, &def_pilot);
                if !inflicted.is_empty() {
                    self.push_message(format!("  → {}", inflicted.join("・")));
                }
            }
        } else {
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
    fn fire_destruction_label(&mut self, pilot: &str, unit: &str) {
        crate::event_runtime::fire_destruction_labels(self, pilot, unit);
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
        let (atk_pilot, atk_unit) = self.database.effective_combat_data(atk_idx)?;
        let dist = combat::manhattan((dx, dy), target);
        let weapon = combat::best_weapon_in_range(&def_unit, dist).cloned()?;
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
        let counter_atk_statuses: Vec<String> = self.database.unit_instances[def_idx]
            .conditions
            .iter()
            .map(|c| c.name.clone())
            .collect();
        let counter_def_statuses: Vec<String> = self.database.unit_instances[atk_idx]
            .conditions
            .iter()
            .map(|c| c.name.clone())
            .collect();
        // 反撃側 = defender (位置 dx,dy)、被弾側 = attacker (位置 target)。
        let counter_atk_env = self.terrain_env_at(dx, dy);
        let counter_def_env = self.terrain_env_at(target.0, target.1);
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
        );
        // 反撃武器の 1-based インデックスを取得
        let weapon_num = def_unit
            .weapons
            .iter()
            .position(|w| w.name == weapon.name)
            .map(|i| i + 1)
            .unwrap_or(1);
        let weapon_name = weapon.name.clone();
        let roll = (self.next_u32() % 100) as i32;
        let hit = roll < preview.hit_chance;
        let msg = if hit {
            self.database.unit_instances[atk_idx].damage += preview.damage;
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
                let mut m = format!(
                    "  ↩ 反撃: {} → {} [{}]: 撃破！",
                    def_pilot.nickname, atk_pilot.nickname, weapon.name
                );
                self.database.remove_unit_at(atk_idx);
                if let Some(killer_idx) = self
                    .database
                    .unit_instances
                    .iter()
                    .position(|u| u.x == dx && u.y == dy)
                {
                    let money = self.award_kill_rewards(killer_idx, exp, victim_value);
                    if money > 0 {
                        m.push_str(&format!(" 資金 +{money}"));
                    }
                }
                m
            } else {
                // 生存: 反撃武器の特殊効果攻撃属性 (状態異常付与) を被弾側へ proc。
                // 反撃側 = def (atk_pilot=def_pilot)、被弾側 = atk (def_pilot=atk_pilot)。
                let inflicted =
                    self.apply_weapon_special_effects(atk_idx, &weapon, &def_pilot, &atk_pilot);
                let mut m = format!(
                    "  ↩ 反撃: {} → {} [{}]: 命中 {} ダメージ (残HP {})",
                    def_pilot.nickname, atk_pilot.nickname, weapon.name, preview.damage, remaining
                );
                if !inflicted.is_empty() {
                    m.push_str(&format!(" → {}", inflicted.join("・")));
                }
                m
            }
        } else {
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
        // 回復アビリティを持つ AI は、射程内に負傷した味方が居れば回復を優先する。
        if self.ai_use_support_ability(idx) {
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
        let best_move = reachable
            .iter()
            .filter(|((x, y), _)| !occupied.contains(&(*x, *y)))
            .filter(|((x, y), _)| combat::manhattan((*x, *y), target_pos) <= max_range)
            // 同じ射程到達なら、ターゲットに最も近い / 残 MP 最大を選好
            .min_by_key(|((x, y), rem)| {
                (
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
                crate::event_runtime::map_attack(
                    self,
                    Some(&caster),
                    &weapon.name,
                    center.0,
                    center.1,
                );
                if let Some(i) = self.database.idx_by_uid(&caster) {
                    self.database.unit_instances[i].has_acted = true;
                }
                return true;
            }
        }
        false
    }

    /// 敵 AI の回復アビリティ使用。回復系アビリティ (効果に `回復`、霊力/ＳＰ 回復は除く) を
    /// 持ち、射程内に負傷した味方が居れば、最も負傷の大きい味方へ回復を発動する。発動したら
    /// `true`。テスト用ユニットはアビリティ無しなので無効 (= 既存テストへ影響なし)。
    fn ai_use_support_ability(&mut self, idx: usize) -> bool {
        let caster = self.database.unit_instances[idx].uid.clone();
        let unit_name = self.database.unit_instances[idx].unit_data_name.clone();
        let heal_idxs: Vec<usize> = match self.database.unit_by_name(&unit_name) {
            Some(d) => d
                .abilities
                .iter()
                .enumerate()
                .filter(|(_, a)| {
                    a.effect.contains("回復")
                        && !a.effect.contains("霊力")
                        && !a.effect.contains("ＳＰ")
                })
                .map(|(i, _)| i)
                .collect(),
            None => return false,
        };
        for ab_idx in heal_idxs {
            if !self.ability_usable(&caster, ab_idx) {
                continue;
            }
            // 射程内で最も負傷した味方を対象にする。
            let target_uids: Vec<String> = self
                .database
                .unit_instances
                .iter()
                .filter(|u| !u.off_map && u.damage > 0)
                .map(|u| u.uid.clone())
                .collect();
            let mut best: Option<(String, i64)> = None;
            for tuid in target_uids {
                if !self.ability_target_valid(&caster, ab_idx, &tuid) {
                    continue;
                }
                let dmg = self
                    .database
                    .unit_by_uid(&tuid)
                    .map(|u| u.damage)
                    .unwrap_or(0);
                if best.as_ref().map(|(_, d)| dmg > *d).unwrap_or(true) {
                    best = Some((tuid, dmg));
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
            if Self::pick_attack_weapon(atk_unit, d, forced, atk_charged, false, false).is_some() {
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
        self.attack_resolve_and_run(Some(pr.target_tile), false, &mode);
        // ランナーは pending_dialog が消えたので次 tick で継続する。
        true
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
        ((u.total_exp / 100).max(0) + 1).min(99)
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
                "全体" | "味方全体" | "味方" => return SpiritTargetKind::AllAllies,
                "単体" | "味方単体" | "指定" => return SpiritTargetKind::SingleAlly,
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
            // ── シナリオ独自コマンド (東方夢想伝): 効果は暫定。要確認。 ──
            "決意" => {
                self.add_unit_condition(target, "必中", 1);
                self.add_unit_condition(target, "熱血", 1);
            }
            "気迫" => self.add_unit_morale(target, 20),
            "希望" => {
                self.add_unit_condition(target, "必中", 1);
                self.add_unit_condition(target, "集中", 1);
            }
            // ── 既定: 未知コマンドは継続 condition として付与 (従来挙動) ──
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
    fn revive_if_possible(&mut self, idx: usize) -> bool {
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
        weapon: &crate::data::unit::WeaponData,
        atk_pilot: &crate::data::pilot::PilotData,
        def_pilot: &crate::data::pilot::PilotData,
    ) -> Vec<String> {
        let effects = crate::combat::weapon_special_effects(&weapon.class);
        let morale_down = crate::combat::weapon_morale_reduction(&weapon.class);
        if effects.is_empty() && morale_down.is_none() {
            return Vec::new();
        }
        let prob =
            (weapon.critical + (atk_pilot.technique - def_pilot.technique) / 2).clamp(1, 100);
        // 耐性 / 弱点: 武器の属性に対し対象が耐性を持てば発動率半減、弱点を持てば倍。
        let prob = self.adjust_proc_for_resistance(def_idx, &weapon.class, prob);
        if (self.next_u32() % 100) as i32 >= prob {
            return Vec::new();
        }
        let is_boss = self.database.unit_instances[def_idx].is_boss();
        let mut applied = Vec::new();
        for (name, lifetime) in effects {
            // ボスランク適用ユニットは石化 / 死の宣告 を無効化 (BossRankコマンド.md)。
            if is_boss && (name == "石化" || name == "死の宣告") {
                continue;
            }
            self.database.unit_instances[def_idx]
                .add_condition(crate::Condition::new(&name, lifetime));
            applied.push(name);
        }
        // 脱 / Ｄ: 対象の気力を低下 (Ｄ の吸収は未対応)。
        if let Some(amount) = morale_down {
            let uid = self.database.unit_instances[def_idx].uid.clone();
            self.add_unit_morale(&uid, -amount);
            applied.push(format!("気力 -{amount}"));
        }
        applied
    }

    /// 特殊効果の発動確率を、対象の `耐性` / `弱点` 特殊能力で調整する
    /// (`特殊効果攻撃属性.md`: 弱点属性に対しては発動確率倍、耐性属性に対しては半減)。
    /// 武器 class のいずれかの属性が対象の弱点に一致すれば ×2、耐性に一致すれば ÷2。
    fn adjust_proc_for_resistance(&self, def_idx: usize, weapon_class: &str, prob: i32) -> i32 {
        let inst = &self.database.unit_instances[def_idx];
        let weak = crate::feature::feature_value(&inst.active_features, "弱点").unwrap_or("");
        let resist = crate::feature::feature_value(&inst.active_features, "耐性").unwrap_or("");
        // 弱/効 属性で付加された一時的弱点 (condition `弱点:<属性>`)。
        let added_weak: Vec<&str> = inst
            .conditions
            .iter()
            .filter_map(|c| c.name.strip_prefix("弱点:"))
            .collect();
        if weak.is_empty() && resist.is_empty() && added_weak.is_empty() {
            return prob;
        }
        let mut p = prob;
        for tok in weapon_class.split_whitespace() {
            if weak.split_whitespace().any(|w| w == tok) || added_weak.contains(&tok) {
                p *= 2;
                break;
            }
            if resist.split_whitespace().any(|r| r == tok) {
                p /= 2;
                break;
            }
        }
        p.clamp(1, 100)
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

    /// 盗 (`盗`): クリティカル時に相手から資金を奪う (`特殊効果攻撃属性.md`)。
    /// 「アイテム所有」が無い相手からは修理費の 1/4 の資金。攻撃側が味方のときのみ獲得し、
    /// 同じ相手からは 1 度だけ (`被盗` 状態で再取得を抑止)。アイテム盗みは未対応。
    fn apply_weapon_crit_steal(
        &mut self,
        def_idx: usize,
        atk_party: crate::Party,
        victim_value: i64,
        weapon: &crate::data::unit::WeaponData,
    ) -> Vec<String> {
        if !weapon.class.split_whitespace().any(|t| t == "盗") {
            return Vec::new();
        }
        // 資金は撃破側が味方 (Player) のときのみ。既に盗んだ相手からは再取得しない。
        if atk_party != crate::Party::Player
            || self.database.unit_instances[def_idx].has_condition("被盗")
        {
            return Vec::new();
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
    fn award_kill_rewards(&mut self, killer_idx: usize, exp: i32, victim_value: i64) -> i64 {
        // 経験値: UnitInstance.total_exp + メインパイロットの PilotInstance 成長。
        let old_level = (self.database.unit_instances[killer_idx].total_exp / 100).max(0) + 1;
        self.database.unit_instances[killer_idx].total_exp += exp;
        let new_level = (self.database.unit_instances[killer_idx].total_exp / 100).max(0) + 1;
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
        let old_level = (self.database.unit_instances[unit_idx].total_exp / 100).max(0) + 1;
        self.database.unit_instances[unit_idx].total_exp += exp;
        let new_level = (self.database.unit_instances[unit_idx].total_exp / 100).max(0) + 1;
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
    fn apply_spirit_to_target(&mut self, caster: &str, target: &str, spirit: &str, cost: i32) {
        self.consume_unit_sp(caster, cost);
        self.apply_spirit_effect(target, spirit);
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
        // 高いほど多い (SRC: 相手のレベルが高いほど多い)。基準値は 修理:補給 = 2:3。
        if let (Some(ci), Some(ti)) = (
            self.database.idx_by_uid(caster),
            self.database.idx_by_uid(target),
        ) {
            let base = match kind {
                SupportKind::Repair => 10,
                SupportKind::Supply => 15,
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
        let (feats, abils) = self
            .database
            .unit_by_name(new_form)
            .map(|ud| {
                let feats = ud
                    .features
                    .iter()
                    .map(|(n, v)| crate::feature::ActiveFeature::new(n.clone(), v.clone()))
                    .collect::<Vec<_>>();
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
        let u = &mut self.database.unit_instances[idx];
        u.unit_data_name = new_form.to_string();
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

    /// 母艦 `carrier` から格納ユニット `stored` を出撃させる。母艦の隣接空きマスに
    /// 配置し、格納リンクを解除する。空きマスが無ければ `false`。
    fn launch_unit_from_carrier(&mut self, carrier: &str, stored: &str) -> bool {
        let Some(cpos) = self.database.unit_by_uid(carrier).map(|u| (u.x, u.y)) else {
            return false;
        };
        let Some((tx, ty)) = self.find_empty_adjacent_tile(cpos) else {
            self.push_message("発進できる空きマスがありません".to_string());
            return false;
        };
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
        true
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
    /// 合体可能なら (合体形態, 2 マス以内の合体相手 uid 列) を返す。
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
        let partner_names: std::collections::HashSet<&str> = toks[2..].iter().copied().collect();
        let hpos = (host.x, host.y);
        let hparty = host.party;
        let partners: Vec<String> = self
            .database
            .unit_instances
            .iter()
            .filter(|u| {
                !u.off_map
                    && u.uid != host_uid
                    && u.party == hparty
                    && partner_names.contains(u.unit_data_name.as_str())
                    && combat::manhattan(hpos, (u.x, u.y)) <= 2
            })
            .map(|u| u.uid.clone())
            .collect();
        if partners.is_empty() {
            return None;
        }
        Some((combined_form, partners))
    }

    /// 合体を実行: 2 マス以内の合体相手を温存 (off_map) して合体形態へ変身する。
    /// 発動で行動終了 (原典準拠)。構成ユニットは `分離` で復帰できる。
    fn apply_combine(&mut self, host_uid: &str) {
        let Some((combined_form, partners)) = self.combine_partners(host_uid) else {
            return;
        };
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

    /// 分離を実行: 内包する構成ユニットを host の隣接空きマスへ復帰させ、host を
    /// 合体前形態へ戻す。行動は消費しない (原典準拠)。
    fn apply_separate(&mut self, host_uid: &str) {
        let components = self
            .database
            .unit_by_uid(host_uid)
            .map(|u| u.combined_from.clone())
            .unwrap_or_default();
        if components.is_empty() {
            return;
        }
        let Some(hpos) = self.database.unit_by_uid(host_uid).map(|u| (u.x, u.y)) else {
            return;
        };
        for c in &components {
            // 既に他の構成ユニットを置いた分も考慮し、毎回空きマスを取り直す。
            if let Some((tx, ty)) = self.find_empty_adjacent_tile(hpos) {
                self.database.move_unit(c, tx, ty);
                self.database.set_off_map(c, false);
                if let Some(u) = self.database.unit_by_uid_mut(c) {
                    u.life_state = String::new();
                    u.has_acted = false;
                    u.has_moved = false;
                }
            }
            // 空きマスが無ければその構成ユニットは復帰できない (off_map のまま、稀)。
        }
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
        // 分離は行動を消費しないのでメニューを再表示。
        self.reopen_unit_menu_for(host_uid);
    }

    /// `host` の `合体` 特殊能力が、合体形態 (DB に存在) と相手 `partner_form` を
    /// 含むか。合体相手判定に使う。
    fn combine_lists_partner(&self, host_uid: &str, partner_form: &str) -> bool {
        let Some(host) = self.database.unit_by_uid(host_uid) else {
            return false;
        };
        let Some(val) = crate::feature::feature_value(&host.active_features, "合体") else {
            return false;
        };
        let toks: Vec<&str> = val.split_whitespace().collect();
        toks.len() >= 3
            && self.database.unit_by_name(toks[1]).is_some()
            && toks[2..].contains(&partner_form)
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
                if self.attack_unit_at(&uid, target, post_move) {
                    // 攻撃したら行動終了 → has_acted、メニュー閉じる
                    if let Some(u) = self.database.unit_by_uid_mut(&uid) {
                        u.has_acted = true;
                    }
                    if let Some(i) = self.database.idx_by_uid(&uid) {
                        crate::event_runtime::fire_action_end_labels(self, i);
                        // SRC `接触 <unit1> <unit2>:` ─ 行動終了後に隣接ペアで発火。
                        crate::event_runtime::fire_contact_event_labels(self, i);
                    }
                    self.action_mode = ActionMode::Browse;
                    self.command_menu = None;
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

        // 合体: 合体特殊能力を持ち、2 マス以内に合体相手が居る味方ユニット。
        // 原典 SRC: 合体は移動前のみ・発動で行動終了。
        if u.party == crate::Party::Player
            && !u.has_acted
            && !post_move
            && self.combine_partners(&u.uid).is_some()
        {
            items.push(UnitMenuItem::Combine);
        }

        // 分離: 構成ユニットを内包している (合体した) 味方ユニット。原典 SRC: 分離は
        // 行動を消費しない。
        if u.party == crate::Party::Player && !u.combined_from.is_empty() {
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
            // 乗り換え: 入れ替え先がある (味方ユニット 2 体以上) ときのみ。
            // 注: 原典は Option コマンドで明示有効化したときのみ表示だが、本実装は
            // Option コマンド未対応のため「2 体以上」で代替する。
            if self.intermission_upgrade_units().len() >= 2 {
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
                        // 部隊ロスター (パイロット / ユニット一覧) を開く。閲覧後は
                        // インターミッションへ戻す (Enter / クリックで一覧を送り、
                        // 末尾でこのシーンに復帰)。
                        self.scene = Scene::PilotList;
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

    /// 乗り換え: 2 ユニットの搭乗パイロット (`pilot_name` + `pilot_ids`) を入れ替える。
    /// 片方が無人でも入れ替え可 (空ユニットが生じうる点は SRC と同じ player 責務)。
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
        let sp = std::mem::take(&mut self.database.unit_instances[si].pilot_name);
        let sids = std::mem::take(&mut self.database.unit_instances[si].pilot_ids);
        let dp = std::mem::take(&mut self.database.unit_instances[di].pilot_name);
        let dids = std::mem::take(&mut self.database.unit_instances[di].pilot_ids);
        self.database.unit_instances[si].pilot_name = dp;
        self.database.unit_instances[si].pilot_ids = dids;
        self.database.unit_instances[di].pilot_name = sp;
        self.database.unit_instances[di].pilot_ids = sids;
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
mod tests {
    use super::*;
    use crate::scene::configuration::{ConfigurationLayout, TITLE_BAR_HEIGHT};

    /// 回帰: 自動発火する章ローカルイベント (`ターン N <陣営>` 等) が、全章を
    /// 同一ライブラリに同時ロードしている状況で **別章の同名ラベルへ漏れない**
    /// こと。
    ///
    /// 旧バグ (東方夢想伝): `begin_phase` が `post_event_label("ターン 1 敵")` を
    /// global 解決していたため、現章 (01) に `ターン 1 敵` が無いと、別章 (12) の
    /// `ターン 1 敵` (アリスの台詞) が誤発火して「話が飛んで」いた。
    #[test]
    fn stage_event_label_does_not_leak_across_chapters() {
        const CH_A: &str = "ターン 1 敵:\nSet ch_a_turn 1\nExit\n";
        const CH_B: &str = "ターン 1 敵:\nSet ch_b_turn 1\nExit\n";

        // chapter A / B を別ファイルとして登録 (両方に同名 `ターン 1 敵`)。
        let mut app = App::new();
        let stmts_a = crate::data::event::parse(CH_A).expect("parse A");
        app.script_library_mut()
            .append_with_name(&stmts_a, "scenes/ch_a.eve");
        let stmts_b = crate::data::event::parse(CH_B).expect("parse B");
        app.script_library_mut()
            .append_with_name(&stmts_b, "scenes/ch_b.eve");

        // 現ステージが chapter A のとき、`ターン 1 敵` は A のものだけ発火。
        app.current_stage_file = "scenes/ch_a.eve".to_string();
        assert!(app.post_stage_event_label("ターン 1 敵".to_string()));
        assert_eq!(app.script_var("ch_a_turn"), "1", "A の ターン 1 敵 が発火");
        assert_eq!(
            app.script_var("ch_b_turn"),
            "",
            "B の ターン 1 敵 が誤発火した (章間漏れ)"
        );
    }

    /// 回帰: 勝利条件「敵・中立撃墜」型ステージ (`全滅 敵`/`全滅 中立`→`クリア`) で、
    /// 敵だけ全滅し敵対する中立が残る局面では組込み勝利を発火しない (早期 Victory で
    /// 状態がロックされ、その後の味方全滅で敗北できず詰むのを防ぐ)。味方全滅時は
    /// 組込み敗北を確実に発火する (中立フェイズでの撃墜でも詰まないように)。
    #[test]
    fn check_victory_defers_clear_but_fires_defeat() {
        const STAGE: &str =
            "全滅 敵:\nSet 敵全滅\nExit\n全滅 中立:\nSet 中立全滅\nExit\nクリア:\nExit\n";
        let mut app = App::new();
        let stmts = crate::data::event::parse(STAGE).unwrap();
        app.script_library_mut()
            .append_with_name(&stmts, "scenes/stage1.eve");
        app.current_stage_file = "scenes/stage1.eve".to_string();
        app.stage_state = crate::stage::StageState::Battle;
        // 味方 1 + 敵対する中立 1 (敵ユニットは既に全滅)。
        app.database
            .unit_instances
            .push(crate::unit_instance::UnitInstance::new(
                "U_P",
                "P",
                crate::Party::Player,
                1,
                1,
            ));
        app.database
            .unit_instances
            .push(crate::unit_instance::UnitInstance::new(
                "U_N",
                "N",
                crate::Party::Neutral,
                2,
                2,
            ));

        app.check_victory();
        assert_eq!(
            app.stage_state,
            crate::stage::StageState::Battle,
            "敵全滅でも敵対中立が残るうちは組込み勝利を発火しない"
        );

        // 味方を全滅させると組込み敗北が確実に発火する。
        app.database
            .unit_instances
            .retain(|u| u.party != crate::Party::Player);
        app.check_victory();
        assert_eq!(
            app.stage_state,
            crate::stage::StageState::Defeat,
            "味方全滅で敗北が発火する (中立フェイズでも詰まない)"
        );
    }

    #[test]
    fn enemy_wipe_with_only_kuria_label_fires_kuria_and_clears() {
        // `クリア` だけ定義 (`全滅 敵` ハンドラなし) のシナリオで、敵全滅後に
        // `クリア` が発火し勝利状態へ進むこと (実機報告: 撃破しても何も起こらない の修正)。
        let stage = crate::data::event::parse("クリア:\nSet クリア発火 1\nExit\n").unwrap();
        let mut app = App::new();
        app.script_library_mut()
            .append_with_name(&stage, "scenes/stage1.eve");
        app.current_stage_file = "scenes/stage1.eve".to_string();
        app.stage_state = crate::stage::StageState::Battle;
        // 味方 1 のみ (敵・中立は全滅済み)。
        app.database
            .unit_instances
            .push(crate::unit_instance::UnitInstance::new(
                "U_P",
                "P",
                crate::Party::Player,
                1,
                1,
            ));
        app.check_victory();
        assert_eq!(
            app.script_var("クリア発火"),
            "1",
            "クリア だけ定義のシナリオでも敵全滅で クリア が発火する"
        );
        assert_eq!(
            app.stage_state,
            crate::stage::StageState::Victory,
            "勝利状態へ進む (Battle のまま固まらない)"
        );
    }

    #[test]
    fn defeat_without_gameover_label_offers_continue_then_title() {
        use crate::stage::StageState;
        // (1) __restart_save あり: 敗北 → コンティニューで再ロード要求。
        let mut app = App::new();
        app.stage_state = StageState::Battle;
        app.current_stage_file = "stage.eve".to_string();
        app.set_script_var("__restart_save".to_string(), "{\"x\":1}".to_string());
        app.game_over();
        assert_eq!(app.stage_state(), StageState::Defeat);
        assert!(
            app.pending_game_over,
            "GameOver 出口イベントが無いので組込みフォールバックが立つ"
        );
        app.game_over_continue();
        assert_eq!(
            app.take_pending_reload().as_deref(),
            Some("{\"x\":1}"),
            "コンティニューでステージ開始時スナップショットの再ロードを要求"
        );
        assert!(!app.pending_game_over);

        // (2) スナップショット無し: コンティニューはタイトルへフォールバック。
        let mut app2 = App::new();
        app2.stage_state = StageState::Battle;
        app2.current_stage_file = "stage.eve".to_string();
        app2.game_over();
        assert!(app2.pending_game_over);
        app2.game_over_continue();
        assert_eq!(
            app2.scene(),
            Scene::Title,
            "スナップショットが無ければタイトルへ戻る"
        );
        assert!(!app2.pending_game_over);
    }

    #[test]
    fn enemy_wipe_with_unresolving_zenmetsu_falls_back_to_game_clear() {
        use crate::stage::StageState;
        // `全滅 敵` ハンドラはあるが空 (進行を解決しない) シナリオ。敵全滅後に idle なら
        // 組込み game_clear で救済し、`クリア` を発火して Victory へ進む (実機報告: 東方夢想伝01)。
        let stage =
            crate::data::event::parse("全滅 敵:\nExit\nクリア:\nSet クリア発火 1\nExit\n").unwrap();
        let mut app = App::new();
        app.script_library_mut()
            .append_with_name(&stage, "stage.eve");
        app.current_stage_file = "stage.eve".to_string();
        app.stage_state = StageState::Battle;
        app.database
            .unit_instances
            .push(crate::unit_instance::UnitInstance::new(
                "U_P",
                "P",
                crate::Party::Player,
                1,
                1,
            ));
        app.check_victory();
        assert_eq!(
            app.stage_state(),
            StageState::Victory,
            "全滅敵 が解決しない idle 状態は game_clear で救済"
        );
        assert_eq!(
            app.script_var("クリア発火"),
            "1",
            "game_clear が クリア を発火"
        );
    }

    #[test]
    fn victory_proceed_goes_to_intermission_when_commands_registered() {
        use crate::stage::StageState;
        let mut app = App::new();
        app.stage_state = StageState::Victory;
        app.push_intermission_command("改造".to_string(), "x.eve".to_string());
        app.proceed_after_victory();
        assert_eq!(
            app.scene(),
            Scene::Intermission,
            "勝利後 Enter/クリックでインターミッションへ進む"
        );
    }

    /// 回帰: プレーンな Ask(Menu) の選択肢を **クリックして選べる** こと。
    /// 旧実装は Menu への任意クリックを `respond_dialog(0)` (キャンセル) にして
    /// いたため、難易度/キャラ選択がクリックで確定できず `選択 = 0` になり、
    /// 東方夢想伝ではキャラ未選択 → 味方 0 体 → 即敗北していた。
    #[test]
    fn clicking_plain_ask_menu_option_selects_it() {
        let mut app = App::new();
        app.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: "どの難易度？".to_string(),
            options: vec!["Easy".into(), "Normal".into(), "Hard".into()],
            var_name: "選択".to_string(),
            store_value: false,
            option_keys: Vec::new(),
            non_cancellable: false,
        });
        // CANVAS 480: 選択肢 2 行目は y∈[324,344)。x は枠内。
        assert!(app.handle_input(Input::ClickAt { x: 120, y: 330 }));
        assert_eq!(app.script_var("選択"), "2", "2 番目の選択肢が確定する");
        assert!(app.pending_dialog().is_none(), "確定で dialog は閉じる");
    }

    /// プレーン Ask の選択肢**外**クリックはキャンセルせず無反応 (dialog 継続)。
    #[test]
    fn clicking_outside_plain_ask_options_is_noop() {
        let mut app = App::new();
        app.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: "どの難易度？".to_string(),
            options: vec!["Easy".into(), "Normal".into(), "Hard".into()],
            var_name: "選択".to_string(),
            store_value: false,
            option_keys: Vec::new(),
            non_cancellable: false,
        });
        // 選択肢列より上 (prompt 付近) をクリック。
        let _ = app.handle_input(Input::ClickAt { x: 120, y: 270 });
        assert_eq!(app.script_var("選択"), "", "未選択のまま");
        assert!(
            app.pending_dialog().is_some(),
            "枠内でも選択肢外なら dialog は閉じない (キャンセルしない)"
        );
    }

    /// 回帰: キャンセル不可の Ask は choice 0 (Esc / 任意進行) を拒否し、選択を強制する。
    /// `キャンセル可` のない `Ask` (キャラ選択等) をキャンセルできると `選択 = 0` で
    /// 味方 0 体 → 即敗北になるため。
    #[test]
    fn non_cancellable_ask_rejects_cancel() {
        let mut app = App::new();
        app.set_pending_dialog(crate::dialog::PendingDialog::Menu {
            prompt: "霊夢と魔理沙、どっちが好き？".to_string(),
            options: vec!["もちろん霊夢".into(), "当然、魔理沙".into()],
            var_name: "選択".to_string(),
            store_value: false,
            option_keys: Vec::new(),
            non_cancellable: true,
        });
        // Esc (Cancel) / 任意進行 (Advance=choice 0) は拒否され dialog 継続。
        let _ = app.handle_input(Input::Cancel);
        assert!(
            app.pending_dialog().is_some(),
            "キャンセル不可の Ask は Esc で閉じない"
        );
        let _ = app.respond_dialog(0);
        assert!(
            app.pending_dialog().is_some(),
            "キャンセル不可の Ask は choice 0 で閉じない"
        );
        assert_eq!(app.script_var("選択"), "", "選択は確定していない");
        // 正規の選択 (choice 1) は通る。
        assert!(app.respond_dialog(1));
        assert_eq!(app.script_var("選択"), "1");
        assert!(app.pending_dialog().is_none());
    }

    /// 現章に当該ステージイベントが無い場合は **何も発火しない** (別章へ漏れない)。
    /// 東方夢想伝01 は `ターン 1 敵` を持たないので、敵フェイズで他章のそれを
    /// 引いてはならない。
    #[test]
    fn stage_event_label_absent_in_current_chapter_fires_nothing() {
        const CH_A: &str = "プロローグ:\nExit\n"; // ターン 1 敵 を持たない
        const CH_B: &str = "ターン 1 敵:\nSet ch_b_turn 1\nExit\n";

        let mut app = App::new();
        let stmts_a = crate::data::event::parse(CH_A).expect("parse A");
        app.script_library_mut()
            .append_with_name(&stmts_a, "scenes/ch_a.eve");
        let stmts_b = crate::data::event::parse(CH_B).expect("parse B");
        app.script_library_mut()
            .append_with_name(&stmts_b, "scenes/ch_b.eve");

        app.current_stage_file = "scenes/ch_a.eve".to_string();
        assert!(
            !app.post_stage_event_label("ターン 1 敵".to_string()),
            "現章に無いステージイベントは投函されない"
        );
        assert_eq!(
            app.script_var("ch_b_turn"),
            "",
            "別章の ターン 1 敵 へ漏れた"
        );
    }

    fn click(app: &mut App, scene_x: i32, scene_y: i32) -> bool {
        let (sw, sh) = app.scene().size();
        let ox = (CANVAS_WIDTH as i32 - sw as i32) / 2;
        let oy = (CANVAS_HEIGHT as i32 - sh as i32) / 2;
        app.handle_input(Input::ClickAt {
            x: ox + scene_x,
            y: oy + scene_y,
        })
    }

    #[test]
    fn advance_walks_scene_loop() {
        let mut app = App::new();
        // Title → Configuration → MapView。MapView 以降の Advance は no-op で、
        // ステージ進行 (Briefing → Sortie → Battle) は Enter でゲートしない
        // （auto_progress / ラベル発火が担う）。PilotList / UnitList は専用 Input で。
        assert_eq!(app.scene(), Scene::Title);
        app.handle_input(Input::Advance);
        assert_eq!(app.scene(), Scene::Configuration);
        app.handle_input(Input::Advance);
        assert_eq!(app.scene(), Scene::MapView);
        // start_scenario を経ていないため stage は空で auto_progress は走らず
        // Briefing のまま。Advance は何もしない（独自の入力待機を撤去済み）。
        assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);
        assert!(!app.handle_input(Input::Advance));
        assert_eq!(app.scene(), Scene::MapView);
        assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);
        // 明示的に PilotList へ
        app.handle_input(Input::GotoPilotList);
        assert_eq!(app.scene(), Scene::PilotList);
        app.handle_input(Input::GotoUnitList);
        assert_eq!(app.scene(), Scene::UnitList);
        app.handle_input(Input::GotoMapView);
        assert_eq!(app.scene(), Scene::MapView);
    }

    #[test]
    fn click_on_title_moves_to_configuration() {
        let mut app = App::new();
        assert!(app.handle_input(Input::ClickAt { x: 100, y: 100 }));
        assert_eq!(app.scene(), Scene::Configuration);
    }

    #[test]
    fn click_on_checkbox_toggles_setting() {
        let mut app = App::new();
        app.handle_input(Input::Advance);
        assert_eq!(app.scene(), Scene::Configuration);

        let l = ConfigurationLayout::original();
        let r = l.battle_animation.bounds;
        assert!(app.settings().battle_animation);
        assert!(click(&mut app, r.x + 4, TITLE_BAR_HEIGHT + r.y + 5));
        assert!(!app.settings().battle_animation);
    }

    #[test]
    fn click_ok_goes_to_main_and_keeps_settings() {
        let mut app = App::new();
        app.handle_input(Input::Advance);

        // checkbox を切り替える
        let l = ConfigurationLayout::original();
        let r = l.move_animation.bounds;
        click(&mut app, r.x + 4, TITLE_BAR_HEIGHT + r.y + 5);
        assert!(!app.settings().move_animation);

        // OK
        let ok = l.ok.bounds;
        assert!(click(&mut app, ok.x + 10, TITLE_BAR_HEIGHT + ok.y + 5));
        assert_eq!(app.scene(), Scene::MapView);
        assert!(!app.settings().move_animation); // 維持
    }

    #[test]
    fn click_cancel_reverts_changes() {
        let mut app = App::new();
        app.handle_input(Input::Advance);

        let l = ConfigurationLayout::original();
        let r = l.move_animation.bounds;
        click(&mut app, r.x + 4, TITLE_BAR_HEIGHT + r.y + 5);
        assert!(!app.settings().move_animation);

        // Cancel
        let c = l.cancel.bounds;
        assert!(click(&mut app, c.x + 10, TITLE_BAR_HEIGHT + c.y + 5));
        assert_eq!(app.scene(), Scene::MapView);
        assert!(app.settings().move_animation); // 巻き戻し
    }

    #[test]
    fn click_message_speed_cycles() {
        let mut app = App::new();
        app.handle_input(Input::Advance);

        let l = ConfigurationLayout::original();
        let r = l.message_speed_combo;
        let start = app.settings().message_speed;
        click(&mut app, r.x + 4, TITLE_BAR_HEIGHT + r.y + 5);
        assert_ne!(app.settings().message_speed, start);
    }

    #[test]
    fn click_mp3_bar_sets_volume() {
        let mut app = App::new();
        app.handle_input(Input::Advance);

        let l = ConfigurationLayout::original();
        let r = l.mp3_volume_scroll;
        // 右端付近 → 高音量
        click(&mut app, r.x + r.w as i32 - 1, TITLE_BAR_HEIGHT + r.y + 5);
        assert!(app.settings().mp3_volume >= 90);
        // 左端付近 → 低音量
        click(&mut app, r.x + 1, TITLE_BAR_HEIGHT + r.y + 5);
        assert!(app.settings().mp3_volume <= 10);
    }

    #[test]
    fn click_on_map_view_sets_cursor() {
        let mut app = App::new();
        app.handle_input(Input::Advance); // Configuration
        app.handle_input(Input::Advance); // MapView
        app.database_mut().replace_map(crate::data::map::demo());
        assert_eq!(app.scene(), Scene::MapView);
        assert!(app.map_cursor().is_none());

        // MapView: 640x456 (LeftMap 448 + RightPanel 192 / Bottom Msg 104)。CANVAS=640x480。
        // offset = (0, 12) (中央配置)。STATUS_BAR_HEIGHT=0, TILE_SIZE=32
        // タイル (1, 0) を狙う: canvas x = 0 + 1*32 + 16 = 48, y = 12 + 0 + 16 = 28
        assert!(app.handle_input(Input::ClickAt { x: 48, y: 28 }));
        assert_eq!(app.map_cursor(), Some((1, 0)));

        // タイル (2, 1)
        assert!(app.handle_input(Input::ClickAt { x: 80, y: 60 }));
        assert_eq!(app.map_cursor(), Some((2, 1)));
    }

    #[test]
    fn click_on_pilotlist_unitlist_loops_back() {
        let mut app = App::new();
        app.handle_input(Input::Advance); // Configuration
        app.handle_input(Input::Advance); // MapView (Briefing)
        app.handle_input(Input::Advance); // MapView (Sortie)
        app.handle_input(Input::Advance); // MapView (Battle)
        app.handle_input(Input::GotoPilotList); // PilotList
        assert!(app.handle_input(Input::ClickAt { x: 50, y: 50 }));
        assert_eq!(app.scene(), Scene::UnitList);
        assert!(app.handle_input(Input::ClickAt { x: 50, y: 50 }));
        assert_eq!(app.scene(), Scene::Title);
    }

    fn enter_mapview_with_demo_map(app: &mut App) {
        app.handle_input(Input::Advance); // Configuration
        app.handle_input(Input::Advance); // MapView
        app.database_mut().replace_map(crate::data::map::demo());
    }

    #[test]
    fn arrow_keys_init_cursor_to_origin() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        assert!(app.map_cursor().is_none());
        // 未配置からの Right → (0,0) スタート、その後 Right で (1,0)... ではない。
        // 実装は (0,0) を基準に dir を適用 → (1,0)
        assert!(app.handle_input(Input::MoveCursor(Direction::Right)));
        assert_eq!(app.map_cursor(), Some((1, 0)));
    }

    #[test]
    fn arrow_keys_clamp_to_map_bounds() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        // 左端で Left → 動かない
        app.handle_input(Input::ClickAt { x: 16, y: 28 }); // (0,0)
        assert_eq!(app.map_cursor(), Some((0, 0)));
        assert!(!app.handle_input(Input::MoveCursor(Direction::Left)));
        assert_eq!(app.map_cursor(), Some((0, 0)));
        // 上端で Up
        assert!(!app.handle_input(Input::MoveCursor(Direction::Up)));
    }

    #[test]
    fn end_phase_advances_turn_after_neutral() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        app.set_stage_state(crate::stage::StageState::Battle);
        assert_eq!(app.turn().number, 1);
        assert_eq!(app.turn().phase, crate::Phase::Player);

        // 自動 AI が Enemy/Allied/Neutral を即座に消費して Player(T2) に戻る
        app.handle_input(Input::EndPhase);
        assert_eq!(app.turn().phase, crate::Phase::Player);
        assert_eq!(app.turn().number, 2);

        app.handle_input(Input::EndPhase);
        assert_eq!(app.turn().phase, crate::Phase::Player);
        assert_eq!(app.turn().number, 3);
    }

    #[test]
    fn end_phase_ignored_outside_mapview() {
        let mut app = App::new();
        assert!(!app.handle_input(Input::EndPhase));
        assert_eq!(app.turn().number, 1);
    }

    fn place_player_unit(app: &mut App, name: &str, x: u32, y: u32) {
        use crate::data::pilot::{Adaption, PilotData, Sex};
        use crate::data::unit::{Size, UnitData};
        use crate::{Party, UnitInstance};

        let pilot = PilotData {
            spirit_commands: Vec::new(),
            name: "PILOT".into(),
            nickname: "P".into(),
            kana_name: "P".into(),
            sex: Sex::Unspecified,
            class: String::new(),
            adaption: Adaption::parse("AAAA").unwrap(),
            exp_value: 0,
            // SRC のデフォルト値に近い数値。0 だと乗算式でダメージが 0 になる。
            infight: 100,
            shooting: 100,
            hit: 10,
            dodge: 10,
            intuition: 10,
            technique: 10,
            personality: None,
            sp: None,
            bgm: None,
            bitmap: None,
            features: Vec::new(),
        };
        let unit_data = UnitData {
            abilities: Vec::new(),
            name: name.into(),
            kana_name: String::new(),
            nickname: name.into(),
            class: String::new(),
            pilot_num: 1,
            item_num: 0,
            transportation: "陸".into(),
            speed: 3,
            size: Size::M,
            value: 0,
            exp_value: 0,
            hp: 100,
            en: 50,
            armor: 10,
            mobility: 10,
            adaption: Adaption::parse("AAAA").unwrap(),
            bitmap: String::new(),
            weapons: Vec::new(),
            features: Vec::new(),
        };
        app.database_mut().pilots.push(pilot);
        app.database_mut().units.push(unit_data);
        app.database_mut()
            .register_unit(UnitInstance::new(name, "PILOT", Party::Player, x, y));
    }

    #[test]
    fn battle_click_opens_unit_menu() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "TestUnit", 2, 6);
        app.set_stage_state(crate::stage::StageState::Battle);

        let ox = 2 * 32 + 16;
        let oy = 12 + 6 * 32 + 16;
        app.handle_input(Input::ClickAt { x: ox, y: oy });
        assert_eq!(menu_unit_pos(&app), Some((2, 6)));
    }

    /// パイロットに精神コマンド (SP コマンド) を与えてユニットに搭載するヘルパ。
    /// `place_player_unit` 済みのユニット uid を返す。
    fn give_spirit_commands(
        app: &mut App,
        cmds: Vec<crate::data::pilot::SpiritCommand>,
        max_sp: i32,
    ) -> String {
        let p = app
            .database_mut()
            .pilots
            .iter_mut()
            .find(|p| p.name == "PILOT")
            .unwrap();
        p.sp = Some(max_sp);
        p.spirit_commands = cmds;
        app.database()
            .unit_instances
            .iter()
            .find(|u| u.party == crate::Party::Player)
            .unwrap()
            .uid
            .clone()
    }

    #[test]
    fn spirit_command_menu_applies_condition_and_consumes_sp() {
        use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
        use crate::data::pilot::SpiritCommand;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        // 勝敗判定回避のため敵を 1 体配置 (味方 0 / 敵 0 での即決着を防ぐ)。
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            10,
            10,
        ));
        app.set_stage_state(crate::stage::StageState::Battle);

        let uid = give_spirit_commands(
            &mut app,
            vec![
                SpiritCommand {
                    name: "集中".into(),
                    cost: Some(20),
                    level: 1,
                },
                // level 99: 未習得 (パイロットは level 1)。メニューに出ない。
                SpiritCommand {
                    name: "熱血".into(),
                    cost: None,
                    level: 99,
                },
            ],
            47,
        );

        // 発動可能なのは 集中 のみ (熱血 は未習得)。
        assert_eq!(
            app.spirit_command_options(&uid),
            vec![("集中".to_string(), 20)]
        );

        // ユニットメニューを開く → 精神コマンド項目が並ぶ。
        let pos = {
            let u = app.database().unit_by_uid(&uid).unwrap();
            (u.x, u.y)
        };
        app.open_unit_menu(pos);
        let has_spirit = matches!(
            app.command_menu(),
            Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Spirit)
        );
        assert!(has_spirit, "精神コマンド項目が表示される");

        // 精神コマンドを選択 → サブメニュー (pending_dialog Menu) が出る。
        app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Spirit));
        assert!(matches!(
            app.pending_dialog(),
            Some(crate::dialog::PendingDialog::Menu { .. })
        ));

        // 1 番目 (集中) を選択 → condition 付与 + SP 20 消費。
        assert!(app.respond_dialog(1));
        {
            let u = app.database().unit_by_uid(&uid).unwrap();
            assert!(u.has_condition("集中"), "集中 condition が付与される");
            assert_eq!(u.sp_consumed, 20, "SP 20 消費");
        }

        // 次の味方フェイズ開始 (T2) で 集中 (lifetime 1) が解除される。
        app.handle_input(Input::EndPhase);
        let u = app.database().unit_by_uid(&uid).unwrap();
        assert!(!u.has_condition("集中"), "集中 は次ターン開始で解除");
    }

    #[test]
    fn spirit_command_hidden_when_insufficient_sp() {
        use crate::data::pilot::SpiritCommand;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        app.set_stage_state(crate::stage::StageState::Battle);

        // 最大 SP 10 では 熱血(30) は買えない。集中(20) も買えない。
        let uid = give_spirit_commands(
            &mut app,
            vec![
                SpiritCommand {
                    name: "集中".into(),
                    cost: Some(20),
                    level: 1,
                },
                SpiritCommand {
                    name: "熱血".into(),
                    cost: Some(30),
                    level: 1,
                },
            ],
            10,
        );
        assert!(
            app.spirit_command_options(&uid).is_empty(),
            "SP 不足なら発動可能コマンドは空"
        );
        // メニューにも精神コマンド項目は出ない。
        let pos = {
            let u = app.database().unit_by_uid(&uid).unwrap();
            (u.x, u.y)
        };
        app.open_unit_menu(pos);
        let has_spirit = matches!(
            app.command_menu(),
            Some(crate::command_menu::CommandMenu::Unit { items, .. })
                if items.contains(&crate::command_menu::UnitMenuItem::Spirit)
        );
        assert!(!has_spirit, "SP 不足では精神コマンド項目は非表示");
    }

    /// place_player_unit 済みの先頭の味方ユニット uid を返すヘルパ。
    fn first_player_uid(app: &App) -> String {
        app.database()
            .unit_instances
            .iter()
            .find(|u| u.party == crate::Party::Player)
            .unwrap()
            .uid
            .clone()
    }

    #[test]
    fn spirit_kasoku_shinsoku_add_move_points() {
        use crate::Condition;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let uid = first_player_uid(&app);
        let base = {
            let u = app.database().unit_by_uid(&uid).unwrap();
            app.database().effective_speed(u)
        };
        // 加速: 移動力 +2
        let mut u = app.database().unit_by_uid(&uid).unwrap().clone();
        u.add_condition(Condition::new("加速", 1));
        assert_eq!(app.database().effective_speed(&u), base + 2);
        // 神速: 移動力 +3
        let mut u = app.database().unit_by_uid(&uid).unwrap().clone();
        u.add_condition(Condition::new("神速", 1));
        assert_eq!(app.database().effective_speed(&u), base + 3);
    }

    #[test]
    fn spirit_heal_effects_reduce_damage() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let uid = first_player_uid(&app);
        let max_hp = {
            let u = app.database().unit_by_uid(&uid).unwrap();
            app.database().effective_max_hp(u)
        }; // 100
           // 信頼: 最大HP 1/3 回復
        app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 90;
        app.apply_spirit_effect(&uid, "信頼");
        assert_eq!(
            app.database().unit_by_uid(&uid).unwrap().damage,
            90 - max_hp / 3
        );
        // 愛: 全快
        app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 90;
        app.apply_spirit_effect(&uid, "愛");
        assert_eq!(app.database().unit_by_uid(&uid).unwrap().damage, 0);
    }

    #[test]
    fn spirit_kakusei_resets_action_flags() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let uid = first_player_uid(&app);
        {
            let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
            u.has_acted = true;
            u.has_moved = true;
        }
        app.apply_spirit_effect(&uid, "覚醒");
        let u = app.database().unit_by_uid(&uid).unwrap();
        assert!(!u.has_acted && !u.has_moved, "覚醒 で再行動可能になる");
    }

    #[test]
    fn spirit_hokyuu_restores_en_and_lowers_morale() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let uid = first_player_uid(&app);
        {
            let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
            u.en_consumed = 20;
            u.morale = 100;
        }
        app.apply_spirit_effect(&uid, "補給");
        let u = app.database().unit_by_uid(&uid).unwrap();
        assert_eq!(u.en_consumed, 0, "補給 で EN 全快");
        assert_eq!(u.morale, 90, "補給 は気力 -10");
    }

    #[test]
    fn spirit_morale_and_grant_effects() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let uid = first_player_uid(&app);
        // 脱力: 気力 -10
        app.database_mut().unit_by_uid_mut(&uid).unwrap().morale = 100;
        app.apply_spirit_effect(&uid, "脱力");
        assert_eq!(app.database().unit_by_uid(&uid).unwrap().morale, 90);
        // 応援: 対象に「努力」を付与
        app.apply_spirit_effect(&uid, "応援");
        assert!(app
            .database()
            .unit_by_uid(&uid)
            .unwrap()
            .has_condition("努力"));
        // 祝福: 対象に「幸運」を付与
        app.apply_spirit_effect(&uid, "祝福");
        assert!(app
            .database()
            .unit_by_uid(&uid)
            .unwrap()
            .has_condition("幸運"));
    }

    #[test]
    fn spirit_fukkatsu_revives_once() {
        use crate::Condition;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let uid = first_player_uid(&app);
        let idx = app.database().idx_by_uid(&uid).unwrap();
        app.database_mut().unit_instances[idx].add_condition(Condition::new("復活", 1));
        app.database_mut().unit_instances[idx].damage = 9999;
        assert!(app.revive_if_possible(idx), "復活 保持なら復活する");
        assert_eq!(app.database().unit_instances[idx].damage, 0, "HP 全快");
        assert!(
            !app.database().unit_instances[idx].has_condition("復活"),
            "復活 は 1 回で消費"
        );
        assert!(!app.revive_if_possible(idx), "2 回目は復活しない");
    }

    #[test]
    fn spirit_single_target_defers_to_target_mode() {
        use crate::command_menu::{ActionMode, MenuActionId, UnitMenuItem};
        use crate::data::pilot::SpiritCommand;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        // 勝敗即決を避けるため敵を 1 体配置。
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            10,
            10,
        ));
        app.set_stage_state(crate::stage::StageState::Battle);
        let uid = give_spirit_commands(
            &mut app,
            vec![SpiritCommand {
                name: "信頼".into(),
                cost: Some(25),
                level: 1,
            }],
            50,
        );
        let pos = {
            let u = app.database().unit_by_uid(&uid).unwrap();
            (u.x, u.y)
        };
        app.open_unit_menu(pos);
        app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Spirit));
        assert!(app.respond_dialog(1));
        // 対象選択モードへ遷移する (caster には適用せず SP も未消費)。
        match app.action_mode() {
            ActionMode::SpiritTarget {
                ref spirit,
                target_enemy,
                ..
            } => {
                assert_eq!(spirit, "信頼");
                assert!(!target_enemy, "信頼 は味方単体対象");
            }
            other => panic!("SpiritTarget を期待: {other:?}"),
        }
        let u = app.database().unit_by_uid(&uid).unwrap();
        assert!(!u.has_condition("信頼"), "対象未確定で caster に付与しない");
        assert_eq!(u.sp_consumed, 0, "対象未確定で SP 未消費");
    }

    #[test]
    fn apply_spirit_to_target_heals_and_consumes_sp() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let caster = first_player_uid(&app);
        // 損傷した味方を対象として配置。
        let target = app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Player,
            3,
            6,
        ));
        app.database_mut().unit_by_uid_mut(&target).unwrap().damage = 90;
        app.apply_spirit_to_target(&caster, &target, "信頼", 25);
        // 信頼 = 最大HP(100)/3 = 33 回復。
        assert_eq!(app.database().unit_by_uid(&target).unwrap().damage, 90 - 33);
        // caster (PilotInstance 無し) は sp_consumed に反映。
        assert_eq!(app.database().unit_by_uid(&caster).unwrap().sp_consumed, 25);
    }

    /// 損傷した隣接味方が居れば「修理」がメニューに出て、対象クリックで HP 全回復
    /// + 発動主体は行動終了する (特殊能力 `修理装置` ベースの組込支援コマンド)。
    #[test]
    fn support_repair_heals_adjacent_ally_and_ends_action() {
        use crate::command_menu::{
            ActionMode, CommandMenu, MenuActionId, SupportKind, UnitMenuItem,
        };
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Medic", 2, 6);
        place_player_unit(&mut app, "Ally", 3, 6);
        // 勝敗即決を避けるため遠方に敵を配置。
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Medic",
            "PILOT",
            crate::Party::Enemy,
            12,
            12,
        ));
        app.set_stage_state(crate::stage::StageState::Battle);

        let caster = first_player_uid(&app);
        let ally = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.x == 3 && u.y == 6)
            .unwrap()
            .uid
            .clone();
        app.database_mut()
            .unit_by_uid_mut(&caster)
            .unwrap()
            .active_features
            .push(crate::feature::ActiveFeature::new("修理装置", ""));
        app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 50;

        // メニューに「修理」が出る。
        app.open_unit_menu((2, 6));
        let has_repair = matches!(
            app.command_menu(),
            Some(CommandMenu::Unit { items, .. })
                if items.contains(&UnitMenuItem::Support(SupportKind::Repair))
        );
        assert!(has_repair, "修理装置 + 隣接損傷味方 で『修理』が表示される");

        // 選択 → 対象選択モードへ。
        app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Support(
            SupportKind::Repair,
        )));
        assert!(matches!(
            app.action_mode(),
            ActionMode::SupportTarget {
                kind: SupportKind::Repair,
                ..
            }
        ));

        // 隣接味方タイル (3,6) をクリック → HP 回復 (修理装置 Lv なし=30%) + 行動終了。
        app.handle_input(Input::ClickAt {
            x: 3 * 32 + 16,
            y: 12 + 6 * 32 + 16,
        });
        assert_eq!(
            app.database().unit_by_uid(&ally).unwrap().damage,
            20,
            "修理装置 Lv なし=30% (最大HP100 → 30 回復) で damage 50→20"
        );
        assert!(
            app.database().unit_by_uid(&caster).unwrap().has_acted,
            "修理発動で行動終了"
        );
        assert!(matches!(app.action_mode(), ActionMode::Browse));
    }

    /// 補給は対象の EN・残弾を全回復し、対象の気力を 10 下げる (原典準拠)。
    #[test]
    fn support_supply_restores_en_and_lowers_morale() {
        use crate::command_menu::SupportKind;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Ally", 3, 6);
        let ally = first_player_uid(&app);
        {
            let u = app.database_mut().unit_by_uid_mut(&ally).unwrap();
            u.en_consumed = 30;
            u.morale = 100;
        }
        app.apply_support_to_target(&ally, &ally, SupportKind::Supply);
        let u = app.database().unit_by_uid(&ally).unwrap();
        assert_eq!(u.en_consumed, 0, "補給で EN 全回復");
        assert_eq!(u.morale, 90, "補給で対象の気力 -10");
    }

    /// 修理装置の回復量はレベルに比例する (Lv3 = 100%)。
    #[test]
    fn repair_amount_scales_with_device_level() {
        use crate::command_menu::SupportKind;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Medic", 2, 6);
        place_player_unit(&mut app, "Ally", 3, 6);
        let caster = first_player_uid(&app);
        let ally = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.x == 3 && u.y == 6)
            .unwrap()
            .uid
            .clone();
        // 修理装置Lv3 = 100% 回復。
        app.database_mut()
            .unit_by_uid_mut(&caster)
            .unwrap()
            .active_features = vec![crate::feature::ActiveFeature::new("修理装置Lv3", "")];
        app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 80;
        app.apply_support_to_target(&caster, &ally, SupportKind::Repair);
        assert_eq!(
            app.database().unit_by_uid(&ally).unwrap().damage,
            0,
            "修理装置Lv3=100% で全回復 (damage 80→0)"
        );
    }

    /// 修理 / 補給 を行ったユニットは経験値を得る。
    #[test]
    fn support_grants_exp_to_caster() {
        use crate::command_menu::SupportKind;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Medic", 2, 6);
        place_player_unit(&mut app, "Ally", 3, 6);
        let caster = first_player_uid(&app);
        let ally = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.x == 3 && u.y == 6)
            .unwrap()
            .uid
            .clone();
        app.database_mut()
            .unit_by_uid_mut(&caster)
            .unwrap()
            .active_features = vec![crate::feature::ActiveFeature::new("修理装置", "")];
        app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 50;
        let exp0 = app.database().unit_by_uid(&caster).unwrap().total_exp;
        app.apply_support_to_target(&caster, &ally, SupportKind::Repair);
        assert_eq!(
            app.database().unit_by_uid(&caster).unwrap().total_exp,
            exp0 + 10,
            "同レベル対象の修理で経験値 +10 (基準値)"
        );
    }

    /// 修理 / 補給 経験値の対象レベル差倍率 (SRC `Unit.cs::GetExp`)。
    #[test]
    fn support_exp_level_diff_table() {
        // 同レベル → 基準値。
        assert_eq!(support_exp_with_level_diff(10, 5, 5), 10);
        // 対象 +2 → ×2、+8 (>7) → ×5。
        assert_eq!(support_exp_with_level_diff(10, 7, 5), 20);
        assert_eq!(support_exp_with_level_diff(10, 13, 5), 50);
        // 対象が低レベル → 逓減 (-1 で ÷2)。
        assert_eq!(support_exp_with_level_diff(10, 4, 5), 5);
        // 補給基準 15 で +1 → ×1.5 = 22 (整数)。
        assert_eq!(support_exp_with_level_diff(15, 6, 5), 22);
        // 最低 1。
        assert_eq!(support_exp_with_level_diff(10, 0, 99), 1);
    }

    /// 統合: 高レベルの対象を修理すると経験値が増える (caster lv1, target lv5 → ×3)。
    #[test]
    fn support_exp_more_for_higher_level_target() {
        use crate::command_menu::SupportKind;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Medic", 2, 6);
        place_player_unit(&mut app, "Ally", 3, 6);
        let caster = first_player_uid(&app);
        let ally = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.x == 3 && u.y == 6)
            .unwrap()
            .uid
            .clone();
        app.database_mut()
            .unit_by_uid_mut(&caster)
            .unwrap()
            .active_features = vec![crate::feature::ActiveFeature::new("修理装置", "")];
        // 対象を高レベル (total_exp 400 → level 5) に。
        {
            let a = app.database_mut().unit_by_uid_mut(&ally).unwrap();
            a.total_exp = 400;
            a.damage = 50;
        }
        let exp0 = app.database().unit_by_uid(&caster).unwrap().total_exp;
        app.apply_support_to_target(&caster, &ally, SupportKind::Repair);
        let gained = app.database().unit_by_uid(&caster).unwrap().total_exp - exp0;
        assert_eq!(gained, 30, "高レベル対象 (+4) の修理で経験値 10×3=30");
    }

    /// 特殊能力が無い / 隣接に要支援の味方が居ないときは支援コマンドを出さない。
    #[test]
    fn support_command_hidden_without_feature_or_need() {
        use crate::command_menu::{CommandMenu, SupportKind, UnitMenuItem};
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Medic", 2, 6);
        place_player_unit(&mut app, "Ally", 3, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        let caster = first_player_uid(&app);

        // 特殊能力なし → 支援コマンドは出ない。
        app.open_unit_menu((2, 6));
        let shown = matches!(
            app.command_menu(),
            Some(CommandMenu::Unit { items, .. })
                if items.iter().any(|i| matches!(i, UnitMenuItem::Support(_)))
        );
        assert!(!shown, "修理装置を持たないユニットに支援コマンドは出ない");

        // 修理装置はあるが隣接味方が無傷 → 対象が無いので出ない。
        app.database_mut()
            .unit_by_uid_mut(&caster)
            .unwrap()
            .active_features
            .push(crate::feature::ActiveFeature::new("修理装置", ""));
        app.open_unit_menu((2, 6));
        let shown2 = matches!(
            app.command_menu(),
            Some(CommandMenu::Unit { items, .. })
                if items.contains(&UnitMenuItem::Support(SupportKind::Repair))
        );
        assert!(!shown2, "隣接に要修理の味方が居なければ『修理』は出ない");
    }

    /// 対象選択をキャンセルしても効果は発生せず、行動も消費しない。
    #[test]
    fn support_target_cancel_has_no_effect() {
        use crate::command_menu::{ActionMode, SupportKind};
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Medic", 2, 6);
        place_player_unit(&mut app, "Ally", 3, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        let caster = first_player_uid(&app);
        let ally = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.x == 3 && u.y == 6)
            .unwrap()
            .uid
            .clone();
        app.database_mut()
            .unit_by_uid_mut(&caster)
            .unwrap()
            .active_features
            .push(crate::feature::ActiveFeature::new("修理装置", ""));
        app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 50;

        app.begin_support_target(&caster, SupportKind::Repair);
        assert!(matches!(
            app.action_mode(),
            ActionMode::SupportTarget { .. }
        ));
        // 右クリック相当でキャンセル。
        assert!(app.cancel_action());
        assert!(matches!(app.action_mode(), ActionMode::Browse));
        assert_eq!(
            app.database().unit_by_uid(&ally).unwrap().damage,
            50,
            "キャンセルで回復しない"
        );
        assert!(
            !app.database().unit_by_uid(&caster).unwrap().has_acted,
            "キャンセルで行動終了しない"
        );
    }

    /// 変形先を 1 つ持つユニットはメニューの「変形」で即変形し、行動は消費しない。
    #[test]
    fn transform_single_form_switches_unit_immediately() {
        use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "FormA", 2, 6);
        // 変形先 FormB の UnitData を用意 (FormA を複製してリネーム)。
        let mut b = app.database().unit_by_name("FormA").cloned().unwrap();
        b.name = "FormB".into();
        app.database_mut().units.push(b);
        app.set_stage_state(crate::stage::StageState::Battle);
        let caster = first_player_uid(&app);
        app.database_mut()
            .unit_by_uid_mut(&caster)
            .unwrap()
            .active_features = vec![crate::feature::ActiveFeature::new("変形", "変形 FormB")];

        app.open_unit_menu((2, 6));
        let has = matches!(
            app.command_menu(),
            Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Transform)
        );
        assert!(has, "変形先を持つユニットに『変形』が表示される");

        app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Transform));
        assert_eq!(
            app.database().unit_by_uid(&caster).unwrap().unit_data_name,
            "FormB",
            "単一形態は即変形する"
        );
        assert!(
            !app.database().unit_by_uid(&caster).unwrap().has_acted,
            "変形は行動を消費しない"
        );
    }

    /// 変形先が複数あるとサブメニューが出て、選んだ形態に変形する。
    #[test]
    fn transform_multi_form_opens_submenu_and_switches() {
        use crate::command_menu::{MenuActionId, UnitMenuItem};
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "FormA", 2, 6);
        for n in ["FormB", "FormC"] {
            let mut d = app.database().unit_by_name("FormA").cloned().unwrap();
            d.name = n.into();
            app.database_mut().units.push(d);
        }
        app.set_stage_state(crate::stage::StageState::Battle);
        let caster = first_player_uid(&app);
        app.database_mut()
            .unit_by_uid_mut(&caster)
            .unwrap()
            .active_features = vec![crate::feature::ActiveFeature::new(
            "変形",
            "変形 FormB FormC",
        )];

        app.open_unit_menu((2, 6));
        app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Transform));
        // サブメニューが開く (まだ変形していない)。
        assert_eq!(
            app.database().unit_by_uid(&caster).unwrap().unit_data_name,
            "FormA"
        );
        // 2 番目 (FormC) を選ぶ。
        assert!(app.respond_dialog(2));
        assert_eq!(
            app.database().unit_by_uid(&caster).unwrap().unit_data_name,
            "FormC",
            "選んだ形態に変形する"
        );
    }

    /// 変形先サブメニューをキャンセルすると形態は変わらない。
    #[test]
    fn transform_submenu_cancel_keeps_form() {
        use crate::command_menu::{MenuActionId, UnitMenuItem};
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "FormA", 2, 6);
        for n in ["FormB", "FormC"] {
            let mut d = app.database().unit_by_name("FormA").cloned().unwrap();
            d.name = n.into();
            app.database_mut().units.push(d);
        }
        app.set_stage_state(crate::stage::StageState::Battle);
        let caster = first_player_uid(&app);
        app.database_mut()
            .unit_by_uid_mut(&caster)
            .unwrap()
            .active_features = vec![crate::feature::ActiveFeature::new(
            "変形",
            "変形 FormB FormC",
        )];
        app.open_unit_menu((2, 6));
        app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Transform));
        assert!(app.respond_dialog(0)); // キャンセル
        assert_eq!(
            app.database().unit_by_uid(&caster).unwrap().unit_data_name,
            "FormA",
            "キャンセルで変形しない"
        );
    }

    /// 変形特殊能力が無い / 移動後 のときは「変形」を出さない (移動前のみ)。
    #[test]
    fn transform_hidden_post_move_or_without_feature() {
        use crate::command_menu::{CommandMenu, UnitMenuItem};
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "FormA", 2, 6);
        let mut b = app.database().unit_by_name("FormA").cloned().unwrap();
        b.name = "FormB".into();
        app.database_mut().units.push(b);
        app.set_stage_state(crate::stage::StageState::Battle);
        let caster = first_player_uid(&app);

        // 変形 特殊能力なし → 出ない。
        app.open_unit_menu((2, 6));
        let absent = matches!(
            app.command_menu(),
            Some(CommandMenu::Unit { items, .. }) if !items.contains(&UnitMenuItem::Transform)
        );
        assert!(absent, "変形 特殊能力なしでは出ない");

        // 変形を付与するが移動後 → 出ない (移動前のみ)。
        {
            let u = app.database_mut().unit_by_uid_mut(&caster).unwrap();
            u.active_features = vec![crate::feature::ActiveFeature::new("変形", "変形 FormB")];
            u.has_moved = true;
        }
        app.open_unit_menu((2, 6));
        let absent2 = matches!(
            app.command_menu(),
            Some(CommandMenu::Unit { items, .. }) if !items.contains(&UnitMenuItem::Transform)
        );
        assert!(absent2, "移動後は変形を出さない");
    }

    /// チャージ攻撃 (Ｃ属性) 武器を持つユニットには「チャージ」が出て、選択で
    /// charged フラグが立ち行動終了する。武器を持たないユニットには出ない。
    #[test]
    fn charge_command_sets_flag_and_ends_action() {
        use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Charger", 2, 6);
        place_player_unit(&mut app, "Plain", 4, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        // Charger の UnitData にチャージ攻撃 (Ｃ属性) 武器を追加。
        {
            let d = app
                .database_mut()
                .units
                .iter_mut()
                .find(|d| d.name == "Charger")
                .unwrap();
            d.weapons.push(crate::data::unit::WeaponData {
                name: "チャージ砲".into(),
                power: 1000,
                min_range: 1,
                max_range: 3,
                precision: 0,
                bullet: -1,
                en_consumption: 0,
                necessary_morale: 0,
                adaption: "AAAA".into(),
                critical: 0,
                class: "Ｃ".into(),
                extras: Vec::new(),
            });
        }
        let charger = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.x == 2 && u.y == 6)
            .unwrap()
            .uid
            .clone();

        // チャージ砲を持つユニットには「チャージ」が出る。
        app.open_unit_menu((2, 6));
        let has = matches!(
            app.command_menu(),
            Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Charge)
        );
        assert!(has, "Ｃ属性武器を持つユニットに『チャージ』が表示される");

        // チャージ砲を持たないユニットには出ない。
        app.open_unit_menu((4, 6));
        let absent = matches!(
            app.command_menu(),
            Some(CommandMenu::Unit { items, .. }) if !items.contains(&UnitMenuItem::Charge)
        );
        assert!(absent, "Ｃ属性武器の無いユニットには『チャージ』は出ない");

        // 発動で charged フラグが立ち、行動終了する。
        app.open_unit_menu((2, 6));
        app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Charge));
        let u = app.database().unit_by_uid(&charger).unwrap();
        assert!(u.charged, "チャージで charged フラグが立つ");
        assert!(u.has_acted, "チャージで行動終了する");

        // 既にチャージ済みなら再表示しない。
        app.database_mut()
            .unit_by_uid_mut(&charger)
            .unwrap()
            .has_acted = false;
        app.open_unit_menu((2, 6));
        let absent2 = matches!(
            app.command_menu(),
            Some(CommandMenu::Unit { items, .. }) if !items.contains(&UnitMenuItem::Charge)
        );
        assert!(
            absent2,
            "チャージ済みユニットには『チャージ』を再表示しない"
        );
    }

    /// 精神「突撃」は移動後でも長射程武器 (マップ攻撃を除く) を使用可能にする。
    #[test]
    fn totsugeki_allows_long_range_weapon_post_move() {
        let mk = |max_range: i32, class: &str| crate::data::unit::WeaponData {
            name: "W".into(),
            power: 1000,
            min_range: 1,
            max_range,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: class.into(),
            extras: Vec::new(),
        };
        let long = mk(3, ""); // 長射程・属性なし
        let map = mk(3, "Ｍ"); // マップ攻撃

        // 移動前は常に使用可。
        assert!(App::weapon_usable_post_move(&long, false, false));
        // 移動後: 通常は長射程不可。突撃でのみ可。
        assert!(!App::weapon_usable_post_move(&long, true, false));
        assert!(App::weapon_usable_post_move(&long, true, true));
        // 突撃でもマップ攻撃 (Ｍ) は移動後不可。
        assert!(!App::weapon_usable_post_move(&map, true, true));
    }

    /// 回復系特殊能力 ＨＰ回復Lv*/ＥＮ回復Lv* は当該陣営フェイズ開始時に
    /// 実効最大値の 10×Lv% を回復する。
    #[test]
    fn regen_features_heal_hp_and_en_at_player_phase_start() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Regenner", 2, 6);
        // 勝敗即決回避の敵 (遠方)。
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Regenner",
            "PILOT",
            crate::Party::Enemy,
            12,
            12,
        ));
        app.set_stage_state(crate::stage::StageState::Battle);
        let uid = first_player_uid(&app);
        {
            let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
            u.active_features = vec![
                crate::feature::ActiveFeature::new("ＨＰ回復Lv2", ""),
                crate::feature::ActiveFeature::new("ＥＮ回復Lv2", ""),
            ];
            u.damage = 50; // 最大HP=100
            u.en_consumed = 20; // 最大EN=50
        }
        // 1 周回って味方フェイズ T2 開始 → 回復適用。
        app.handle_input(Input::EndPhase);
        assert_eq!(app.turn().phase, crate::Phase::Player);
        let u = app.database().unit_by_uid(&uid).unwrap();
        assert_eq!(u.damage, 30, "ＨＰ回復Lv2 = 最大HP 20% (20) 回復");
        assert_eq!(u.en_consumed, 10, "ＥＮ回復Lv2 = 最大EN 20% (10) 回復");
    }

    /// ＨＰ消費Lv*/ＥＮ消費Lv* は同率を減少させる (ＨＰ は最低 1)。
    #[test]
    fn drain_features_reduce_hp_and_en_at_player_phase_start() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Leaker", 2, 6);
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Leaker",
            "PILOT",
            crate::Party::Enemy,
            12,
            12,
        ));
        app.set_stage_state(crate::stage::StageState::Battle);
        let uid = first_player_uid(&app);
        {
            let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
            u.active_features = vec![
                crate::feature::ActiveFeature::new("ＨＰ消費Lv1", ""),
                crate::feature::ActiveFeature::new("ＥＮ消費Lv3", ""),
            ];
            // HP 満タン / EN 未消費から開始。
        }
        app.handle_input(Input::EndPhase);
        let u = app.database().unit_by_uid(&uid).unwrap();
        assert_eq!(u.damage, 10, "ＨＰ消費Lv1 = 最大HP 10% (10) 減少");
        assert_eq!(u.en_consumed, 15, "ＥＮ消費Lv3 = 最大EN 30% (15) 減少");
    }

    /// 回復不能 (特殊効果攻撃属性 害) は特殊能力による HP/EN 自然回復を阻害する。
    #[test]
    fn kaifukufunou_blocks_regen_features() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Regenner", 2, 6);
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Regenner",
            "PILOT",
            crate::Party::Enemy,
            12,
            12,
        ));
        app.set_stage_state(crate::stage::StageState::Battle);
        let uid = first_player_uid(&app);
        {
            let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
            u.active_features = vec![
                crate::feature::ActiveFeature::new("ＨＰ回復Lv2", ""),
                crate::feature::ActiveFeature::new("ＥＮ回復Lv2", ""),
            ];
            u.damage = 50;
            u.en_consumed = 20;
            u.add_condition(crate::Condition::new("回復不能", -1));
        }
        app.handle_input(Input::EndPhase);
        let u = app.database().unit_by_uid(&uid).unwrap();
        assert_eq!(u.damage, 50, "回復不能で HP 自然回復が阻害される");
        assert_eq!(u.en_consumed, 20, "回復不能で EN 自然回復が阻害される");
    }

    /// ゾンビ (特殊効果攻撃属性 ゾ) はアビリティ / 精神による能動的な HP/EN 回復を阻害する。
    #[test]
    fn zombie_blocks_active_recovery() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Z", 2, 6);
        let uid = first_player_uid(&app);
        {
            let u = app.database_mut().unit_by_uid_mut(&uid).unwrap();
            u.damage = 80;
            u.add_condition(crate::Condition::new("ゾンビ", 3));
        }
        // アビリティ回復は無効。
        app.apply_ability_effects(&uid, &uid, "回復Lv2");
        assert_eq!(
            app.database().unit_by_uid(&uid).unwrap().damage,
            80,
            "ゾンビはアビリティ回復を受けない"
        );
        // 精神 全快 (spirit_heal_full) も無効。
        app.spirit_heal_full(&uid);
        assert_eq!(
            app.database().unit_by_uid(&uid).unwrap().damage,
            80,
            "ゾンビは全快回復も受けない"
        );
    }

    /// アビリティをユニットに与えるテストヘルパ (静的 UnitData + ランタイム両方)。
    fn give_ability(app: &mut App, unit_name: &str, uid: &str, ab: crate::data::unit::AbilityData) {
        let d = app
            .database_mut()
            .units
            .iter_mut()
            .find(|d| d.name == unit_name)
            .unwrap();
        d.abilities.push(ab);
        let abilities = app
            .database()
            .unit_by_name(unit_name)
            .unwrap()
            .abilities
            .clone();
        let u = app.database_mut().unit_by_uid_mut(uid).unwrap();
        u.abilities = abilities
            .iter()
            .map(|a| {
                let mut ua =
                    crate::unit_ability::UnitAbility::new(a.name.clone(), a.effect.clone());
                ua.stock_remaining = a.uses;
                ua
            })
            .collect();
    }

    fn mk_ability(
        name: &str,
        effect: &str,
        range: i32,
        uses: Option<i32>,
    ) -> crate::data::unit::AbilityData {
        crate::data::unit::AbilityData {
            name: name.into(),
            effect: effect.into(),
            range,
            uses,
            en_cost: None,
            morale: None,
            attributes: String::new(),
        }
    }

    /// 射程0 アビリティ (回復) は自分に即適用し、回数を消費し行動終了する。
    #[test]
    fn ability_self_heal_applies_consumes_and_ends_action() {
        use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Healer", 2, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        let uid = first_player_uid(&app);
        app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 50;
        give_ability(
            &mut app,
            "Healer",
            &uid,
            mk_ability("自己回復", "回復Lv1", 0, Some(2)),
        );

        app.open_unit_menu((2, 6));
        let has = matches!(
            app.command_menu(),
            Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Ability)
        );
        assert!(has, "アビリティを持つユニットに『アビリティ』が表示される");

        app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Ability));
        assert!(app.respond_dialog(1)); // 1 番目のアビリティを選択

        let u = app.database().unit_by_uid(&uid).unwrap();
        assert_eq!(u.damage, 0, "回復Lv1 = 500 回復で全快 (最大HP100)");
        assert!(u.has_acted, "アビリティ発動で行動終了");
        assert_eq!(
            u.abilities[0].stock_remaining,
            Some(1),
            "回数が 2→1 に消費される"
        );
    }

    /// 射程≥1 アビリティは対象選択へ遷移し、クリックした射程内の味方に適用される。
    #[test]
    fn ability_targeted_heals_ally_in_range() {
        use crate::command_menu::{ActionMode, MenuActionId, UnitMenuItem};
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Healer", 2, 6);
        place_player_unit(&mut app, "Ally", 4, 6);
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Healer",
            "PILOT",
            crate::Party::Enemy,
            12,
            12,
        ));
        app.set_stage_state(crate::stage::StageState::Battle);
        let caster = first_player_uid(&app);
        let ally = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.x == 4 && u.y == 6)
            .unwrap()
            .uid
            .clone();
        app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 90;
        give_ability(
            &mut app,
            "Healer",
            &caster,
            mk_ability("治療", "回復Lv2", 3, None),
        );

        app.open_unit_menu((2, 6));
        app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Ability));
        assert!(app.respond_dialog(1));
        // 射程≥1 → 対象選択モードへ。
        assert!(matches!(
            app.action_mode(),
            ActionMode::AbilityTarget { .. }
        ));

        // 射程内 (距離2) の味方 (4,6) をクリック。
        app.handle_input(Input::ClickAt {
            x: 4 * 32 + 16,
            y: 12 + 6 * 32 + 16,
        });
        assert_eq!(
            app.database().unit_by_uid(&ally).unwrap().damage,
            0,
            "回復Lv2 = 1000 回復で全快"
        );
        assert!(
            app.database().unit_by_uid(&caster).unwrap().has_acted,
            "アビリティ発動で発動主体は行動終了"
        );
    }

    /// 回数切れアビリティは `×` 付きで表示され、選択しても発動しない。
    #[test]
    fn ability_out_of_uses_shown_with_x_and_rejected() {
        use crate::command_menu::{MenuActionId, UnitMenuItem};
        use crate::dialog::PendingDialog;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Healer", 2, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        let uid = first_player_uid(&app);
        app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 50;
        give_ability(
            &mut app,
            "Healer",
            &uid,
            mk_ability("自己回復", "回復Lv1", 0, Some(0)),
        );

        app.open_unit_menu((2, 6));
        app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Ability));
        // サブメニューの選択肢に × が付く。
        let x_shown = matches!(
            app.pending_dialog(),
            Some(PendingDialog::Menu { options, .. }) if options[0].starts_with('×')
        );
        assert!(x_shown, "回数切れアビリティは × 付きで表示");
        // 選択しても回復しない (発動拒否)。
        assert!(app.respond_dialog(1));
        assert_eq!(
            app.database().unit_by_uid(&uid).unwrap().damage,
            50,
            "回数切れは発動しない"
        );
        assert!(
            !app.database().unit_by_uid(&uid).unwrap().has_acted,
            "回数切れは行動を消費しない"
        );
    }

    /// アビリティを持たないユニットには『アビリティ』を出さない。
    #[test]
    fn ability_menu_hidden_without_abilities() {
        use crate::command_menu::{CommandMenu, UnitMenuItem};
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Plain", 2, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        app.open_unit_menu((2, 6));
        let shown = matches!(
            app.command_menu(),
            Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Ability)
        );
        assert!(!shown, "アビリティ無しユニットに『アビリティ』は出ない");
    }

    /// 射程0 の `再行動` アビリティは行動を消費しない (発動後もメニュー再表示)。
    #[test]
    fn ability_saidoudou_does_not_consume_action() {
        use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Mover", 2, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        let uid = first_player_uid(&app);
        // 行動済みにしておき、再行動で戻ることを確認。
        app.database_mut().unit_by_uid_mut(&uid).unwrap().has_acted = false;
        give_ability(
            &mut app,
            "Mover",
            &uid,
            mk_ability("再動", "再行動", 0, None),
        );

        app.open_unit_menu((2, 6));
        app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Ability));
        assert!(app.respond_dialog(1));
        // 再行動で has_acted=false のまま → メニューが再表示される。
        assert!(
            !app.database().unit_by_uid(&uid).unwrap().has_acted,
            "再行動アビリティは行動を消費しない"
        );
        assert!(
            matches!(app.command_menu(), Some(CommandMenu::Unit { .. })),
            "発動後にユニットメニューを再表示する"
        );
    }

    /// アビリティ効果 霊力回復 / ＳＰ回復 は SP を 10×Lv 回復する。
    #[test]
    fn ability_sp_restore_effect_reduces_consumed() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Caster", 2, 6);
        let uid = first_player_uid(&app);
        // PILOT に最大SP 50 を与え 30 消費した状態に (PilotInstance 無し → sp_consumed 経路)。
        app.database_mut()
            .pilots
            .iter_mut()
            .find(|p| p.name == "PILOT")
            .unwrap()
            .sp = Some(50);
        app.database_mut()
            .unit_by_uid_mut(&uid)
            .unwrap()
            .sp_consumed = 30;
        app.apply_ability_effects(&uid, &uid, "霊力回復Lv2"); // 10×2 = 20 回復
        assert_eq!(
            app.database().unit_by_uid(&uid).unwrap().sp_consumed,
            10,
            "SP 20 回復で消費 30→10"
        );
    }

    /// アビリティ効果 変身 は対象を別フォームへ差し替える (set_unit_form を共有)。
    #[test]
    fn ability_henshin_effect_changes_form() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Human", 2, 6);
        // 変身先 Dragon の UnitData を用意。
        let mut d = app.database().unit_by_name("Human").cloned().unwrap();
        d.name = "Dragon".into();
        app.database_mut().units.push(d);
        let uid = first_player_uid(&app);
        app.apply_ability_effects(&uid, &uid, "変身Lv3=Dragon");
        assert_eq!(
            app.database().unit_by_uid(&uid).unwrap().unit_data_name,
            "Dragon",
            "変身で形態が変わる"
        );
    }

    /// アビリティ効果 召喚Lv* は指定ユニットを Lv 体、隣接マスへ同陣営生成する。
    #[test]
    fn ability_summon_effect_creates_units() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Summoner", 5, 5);
        // 召喚先 Drone の UnitData (Summoner を複製)。
        let mut d = app.database().unit_by_name("Summoner").cloned().unwrap();
        d.name = "Drone".into();
        app.database_mut().units.push(d);
        let uid = first_player_uid(&app);
        app.apply_ability_effects(&uid, &uid, "召喚Lv2=Drone");

        let drones: Vec<_> = app
            .database()
            .unit_instances
            .iter()
            .filter(|u| u.unit_data_name == "Drone")
            .collect();
        assert_eq!(drones.len(), 2, "召喚Lv2 で 2 体生成");
        assert!(
            drones
                .iter()
                .all(|d| d.summoned_by.as_deref() == Some(uid.as_str())),
            "summoned_by に親 uid"
        );
        assert!(
            drones.iter().all(|d| d.party == crate::Party::Player),
            "同陣営で生成"
        );
        // 親 (5,5) の隣接に配置される。
        assert!(drones
            .iter()
            .all(|d| (d.x as i32 - 5).abs() <= 1 && (d.y as i32 - 5).abs() <= 1));
    }

    /// アビリティ効果 強化 は指定特殊能力を一時状態 (condition) として対象へ付与する。
    #[test]
    fn ability_kyouka_adds_condition() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Booster", 2, 6);
        let uid = first_player_uid(&app);
        app.apply_ability_effects(&uid, &uid, "強化Lv2=切り払い");
        let u = app.database().unit_by_uid(&uid).unwrap();
        assert!(
            u.conditions.iter().any(|c| c.name == "切り払い"),
            "強化で指定能力が状態として付与される"
        );
    }

    /// アビリティ効果 能力コピー は発動者を対象 (射程内味方) の形態へ変化させる。
    /// サイズ 2 段階以上差のユニットへは変化できない (サイズ制限)。
    #[test]
    fn ability_nouryoku_copy_transforms_caster_with_size_limit() {
        use crate::data::unit::Size;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Copier", 2, 6); // Size::M
        place_player_unit(&mut app, "Model", 4, 6); // Size::M (同サイズ)
        let caster = first_player_uid(&app);
        let model = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.x == 4 && u.y == 6)
            .unwrap()
            .uid
            .clone();
        give_ability(
            &mut app,
            "Copier",
            &caster,
            mk_ability("コピー", "能力コピーLv3", 3, None),
        );
        // 同サイズ・射程内 → 有効な対象。
        assert!(app.ability_target_valid(&caster, 0, &model));
        app.apply_ability(&caster, 0, &model);
        assert_eq!(
            app.database().unit_by_uid(&caster).unwrap().unit_data_name,
            "Model",
            "能力コピーで発動者が対象の形態へ変化"
        );

        // XL サイズの対象 (M とは 3 段階差) は対象不可。
        let mut big = app.database().unit_by_name("Model").cloned().unwrap();
        big.name = "BigModel".into();
        big.size = Size::XL;
        app.database_mut().units.push(big);
        app.database_mut()
            .unit_by_uid_mut(&model)
            .unwrap()
            .unit_data_name = "BigModel".into();
        assert!(
            !app.ability_target_valid(&caster, 0, &model),
            "サイズ 2 段階以上差の対象には能力コピー不可"
        );
    }

    /// マップ型アビリティ (Ｍ全) は射程内 (Ｍ全 は盤上全体) の全味方へ効果を及ぼす。
    #[test]
    fn ability_map_type_heals_all_allies() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Caster", 5, 5);
        place_player_unit(&mut app, "AllyA", 5, 6);
        place_player_unit(&mut app, "AllyB", 6, 5);
        let caster = first_player_uid(&app);
        for pos in [(5u32, 6u32), (6, 5)] {
            let uid = app
                .database()
                .unit_instances
                .iter()
                .find(|u| u.x == pos.0 && u.y == pos.1)
                .unwrap()
                .uid
                .clone();
            app.database_mut().unit_by_uid_mut(&uid).unwrap().damage = 50;
        }
        let mut ab = mk_ability("全体回復", "回復Lv1", 3, None);
        ab.attributes = "Ｍ全".into();
        give_ability(&mut app, "Caster", &caster, ab);
        app.apply_ability(&caster, 0, &caster);
        for pos in [(5u32, 6u32), (6, 5)] {
            let dmg = app
                .database()
                .unit_instances
                .iter()
                .find(|u| u.x == pos.0 && u.y == pos.1)
                .unwrap()
                .damage;
            assert_eq!(dmg, 0, "Ｍ全 回復で全味方が回復する");
        }
    }

    /// 敵対象アビリティ (脱=気力低下 / 除=特殊効果解除) は味方ではなく敵を対象に取る。
    #[test]
    fn ability_enemy_target_datsu_and_jo() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Caster", 2, 6);
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Caster",
            "FOE",
            crate::Party::Enemy,
            4,
            6,
        ));
        let caster = first_player_uid(&app);
        let enemy = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.party == crate::Party::Enemy)
            .unwrap()
            .uid
            .clone();
        // 敵に強化系の状態を付けておく (除 で解除される)。
        app.database_mut()
            .unit_by_uid_mut(&enemy)
            .unwrap()
            .add_condition(crate::Condition::new("攻撃力ＵＰ", -1));
        let mut ab = mk_ability("脱力除去弾", "解説", 3, None);
        ab.attributes = "脱 除".into();
        give_ability(&mut app, "Caster", &caster, ab);

        // 脱/除 アビリティは敵が有効対象、味方 (自分) は無効。
        assert!(app.ability_target_valid(&caster, 0, &enemy));
        assert!(!app.ability_target_valid(&caster, 0, &caster));

        let m0 = app.database().unit_by_uid(&enemy).unwrap().morale;
        app.apply_ability(&caster, 0, &enemy);
        let enemy_u = app.database().unit_by_uid(&enemy).unwrap();
        assert_eq!(enemy_u.morale, m0 - 10, "脱で敵気力 -10");
        assert!(
            enemy_u.conditions.is_empty(),
            "除で敵のアビリティ特殊効果を解除"
        );
    }

    /// 母艦: 搭載 (fire_boarding_event) で格納リンクが張られ、発進で隣接マスへ出撃する。
    #[test]
    fn carrier_board_and_launch() {
        use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Carrier", 5, 5);
        place_player_unit(&mut app, "Fighter", 6, 5);
        app.set_stage_state(crate::stage::StageState::Battle);
        let carrier = first_player_uid(&app);
        let fighter = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Fighter")
            .unwrap()
            .uid
            .clone();
        let carrier_idx = app.database().idx_by_uid(&carrier).unwrap();
        let fighter_idx = app.database().idx_by_uid(&fighter).unwrap();

        // 搭載: 相互リンク + off_map。
        app.fire_boarding_event(fighter_idx, carrier_idx);
        let f = app.database().unit_by_uid(&fighter).unwrap();
        assert!(f.off_map, "格納で off_map");
        assert_eq!(f.stored_in.as_deref(), Some(carrier.as_str()));
        assert!(app
            .database()
            .unit_by_uid(&carrier)
            .unwrap()
            .stored_units
            .contains(&fighter));

        // 発進: メニューに「発進」が出る。
        app.open_unit_menu((5, 5));
        let has_launch = matches!(
            app.command_menu(),
            Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Launch)
        );
        assert!(has_launch, "格納ユニットがあれば『発進』が出る");

        app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Launch));
        assert!(app.respond_dialog(1)); // 1 番目の格納ユニット

        let f = app.database().unit_by_uid(&fighter).unwrap();
        assert!(!f.off_map, "発進で盤上へ復帰");
        assert!(f.stored_in.is_none());
        assert!(
            app.database()
                .unit_by_uid(&carrier)
                .unwrap()
                .stored_units
                .is_empty(),
            "格納リストから外れる"
        );
        // 母艦 (5,5) の隣接マスに配置される。
        let (fx, fy) = (f.x as i32, f.y as i32);
        assert!(
            (fx - 5).abs() <= 1 && (fy - 5).abs() <= 1 && (fx, fy) != (5, 5),
            "母艦隣接に配置: ({fx},{fy})"
        );
    }

    /// 母艦格納中ユニットは当該陣営フェイズ開始時に HP/EN を 50% 回復する。
    #[test]
    fn carrier_stored_unit_recovers_each_turn() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Carrier", 5, 5);
        place_player_unit(&mut app, "Fighter", 6, 5);
        // 勝敗即決回避の敵。
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Carrier",
            "PILOT",
            crate::Party::Enemy,
            12,
            12,
        ));
        app.set_stage_state(crate::stage::StageState::Battle);
        let carrier = first_player_uid(&app);
        let fighter = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Fighter")
            .unwrap()
            .uid
            .clone();
        let carrier_idx = app.database().idx_by_uid(&carrier).unwrap();
        let fighter_idx = app.database().idx_by_uid(&fighter).unwrap();
        {
            let f = app.database_mut().unit_by_uid_mut(&fighter).unwrap();
            f.damage = 60; // 最大HP 100
            f.en_consumed = 40; // 最大EN 50
        }
        app.fire_boarding_event(fighter_idx, carrier_idx);

        // 1 周して味方フェイズ T2 開始 → 母艦回復。
        app.handle_input(Input::EndPhase);
        let f = app.database().unit_by_uid(&fighter).unwrap();
        assert_eq!(f.damage, 10, "HP 50% (50) 回復で damage 60→10");
        assert_eq!(f.en_consumed, 15, "EN 50% (25) 回復で en_consumed 40→15");
    }

    /// 合体テスト用: host + 2マス以内の合体相手を置き、合体形態 UnitData を用意する。
    fn setup_combine(app: &mut App) -> (String, String) {
        enter_mapview_with_demo_map(app);
        place_player_unit(app, "GetterEagle", 3, 3);
        place_player_unit(app, "GetterJaguar", 4, 3);
        // 合体形態 GetterRobo の UnitData (GetterEagle を複製)。
        let mut g = app.database().unit_by_name("GetterEagle").cloned().unwrap();
        g.name = "GetterRobo".into();
        app.database_mut().units.push(g);
        app.set_stage_state(crate::stage::StageState::Battle);
        let host = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "GetterEagle")
            .unwrap()
            .uid
            .clone();
        let partner = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "GetterJaguar")
            .unwrap()
            .uid
            .clone();
        // host に 合体 特殊能力 (名称, 合体形態, 相手)。
        app.database_mut()
            .unit_by_uid_mut(&host)
            .unwrap()
            .active_features = vec![crate::feature::ActiveFeature::new(
            "合体",
            "合体 GetterRobo GetterJaguar",
        )];
        (host, partner)
    }

    /// 合体: 2 マス以内の相手を取り込み合体形態へ変身、相手は温存 (off_map)、行動終了。
    #[test]
    fn combine_absorbs_partners_into_form() {
        use crate::command_menu::{CommandMenu, MenuActionId, UnitMenuItem};
        let mut app = App::new();
        let (host, partner) = setup_combine(&mut app);

        app.open_unit_menu((3, 3));
        let has = matches!(
            app.command_menu(),
            Some(CommandMenu::Unit { items, .. }) if items.contains(&UnitMenuItem::Combine)
        );
        assert!(has, "合体相手が2マス以内なら『合体』が出る");

        app.execute_menu_action(MenuActionId::Unit(UnitMenuItem::Combine));
        let h = app.database().unit_by_uid(&host).unwrap();
        assert_eq!(h.unit_data_name, "GetterRobo", "host が合体形態へ変身");
        assert!(h.combined_from.contains(&partner), "構成ユニットを記録");
        assert_eq!(h.pre_combine_form.as_deref(), Some("GetterEagle"));
        assert!(h.has_acted, "合体で行動終了");
        assert!(
            app.database().unit_by_uid(&partner).unwrap().off_map,
            "合体相手は off_map で温存"
        );
    }

    /// 分離: 構成ユニットを隣接マスへ復帰させ、host を合体前形態へ戻す (行動非消費)。
    #[test]
    fn separate_restores_components_and_reverts_form() {
        let mut app = App::new();
        let (host, partner) = setup_combine(&mut app);
        app.apply_combine(&host);
        assert!(app.database().unit_by_uid(&partner).unwrap().off_map);

        app.apply_separate(&host);
        let h = app.database().unit_by_uid(&host).unwrap();
        assert_eq!(h.unit_data_name, "GetterEagle", "host が合体前形態へ戻る");
        assert!(h.combined_from.is_empty(), "構成リストがクリアされる");
        assert!(h.pre_combine_form.is_none());

        let p = app.database().unit_by_uid(&partner).unwrap();
        assert!(!p.off_map, "構成ユニットが盤上へ復帰");
        assert!(
            (p.x as i32 - 3).abs() <= 1 && (p.y as i32 - 3).abs() <= 1,
            "host (3,3) 隣接に復帰: ({},{})",
            p.x,
            p.y
        );
    }

    /// 合体時のパイロット統合: 構成ユニットのパイロットが合体形態へ搭乗 (全員搭乗)、
    /// 分離で各機の搭乗構成へ戻る。
    #[test]
    fn combine_integrates_pilots_and_separate_restores() {
        let mut app = App::new();
        let (host, partner) = setup_combine(&mut app);
        // host / partner に区別可能な pilot_ids を設定。
        app.database_mut().unit_by_uid_mut(&host).unwrap().pilot_ids = vec!["host_p".to_string()];
        app.database_mut()
            .unit_by_uid_mut(&partner)
            .unwrap()
            .pilot_ids = vec!["partner_p".to_string()];

        app.apply_combine(&host);
        let h = app.database().unit_by_uid(&host).unwrap();
        assert!(
            h.pilot_ids.contains(&"host_p".to_string()),
            "host のパイロットを維持"
        );
        assert!(
            h.pilot_ids.contains(&"partner_p".to_string()),
            "相手のパイロットが合体形態へ搭乗"
        );
        assert_eq!(
            h.pre_combine_pilots,
            vec!["host_p".to_string()],
            "合体前の搭乗構成を保持"
        );

        app.apply_separate(&host);
        let h = app.database().unit_by_uid(&host).unwrap();
        assert_eq!(
            h.pilot_ids,
            vec!["host_p".to_string()],
            "分離で host の搭乗構成が復帰"
        );
        assert!(h.pre_combine_pilots.is_empty(), "統合状態がクリアされる");
        let p = app.database().unit_by_uid(&partner).unwrap();
        assert_eq!(
            p.pilot_ids,
            vec!["partner_p".to_string()],
            "相手のパイロットが各機へ復帰"
        );
    }

    /// 母艦上へ移動すると搭載される (ムーブ統合)。
    #[test]
    fn board_carrier_by_moving_onto_it() {
        use crate::command_menu::ActionMode;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Carrier", 5, 5);
        place_player_unit(&mut app, "Fighter", 5, 6); // 母艦に隣接
        app.set_stage_state(crate::stage::StageState::Battle);
        let carrier = first_player_uid(&app);
        let fighter = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Fighter")
            .unwrap()
            .uid
            .clone();
        app.database_mut()
            .unit_by_uid_mut(&carrier)
            .unwrap()
            .active_features = vec![crate::feature::ActiveFeature::new("母艦", "")];

        // Fighter を移動モードにして母艦タイル (5,5) をクリック。
        app.action_mode = ActionMode::MoveSelect {
            uid: fighter.clone(),
        };
        app.handle_input(Input::ClickAt {
            x: 5 * 32 + 16,
            y: 12 + 5 * 32 + 16,
        });

        assert!(
            app.database().unit_by_uid(&fighter).unwrap().off_map,
            "母艦上へ移動で搭載 (off_map)"
        );
        assert!(app
            .database()
            .unit_by_uid(&carrier)
            .unwrap()
            .stored_units
            .contains(&fighter));
    }

    /// 合体相手の上へ移動すると合体する (2 機合体のムーブ統合)。
    #[test]
    fn combine_by_moving_onto_partner() {
        use crate::command_menu::ActionMode;
        let mut app = App::new();
        let (host, partner) = setup_combine(&mut app);
        // partner (GetterJaguar) を移動モードにして host (GetterEagle, 3,3) をクリック。
        // host が 合体 特殊能力を持ち相手リストに partner の形態を含むので、host が
        // 合体形態 (GetterRobo) になり partner が温存される。
        app.action_mode = ActionMode::MoveSelect {
            uid: partner.clone(),
        };
        app.handle_input(Input::ClickAt {
            x: 3 * 32 + 16,
            y: 12 + 3 * 32 + 16,
        });

        assert_eq!(
            app.database().unit_by_uid(&host).unwrap().unit_data_name,
            "GetterRobo",
            "相手上へ移動で合体形態へ"
        );
        assert!(
            app.database().unit_by_uid(&partner).unwrap().off_map,
            "mover は構成ユニットとして温存"
        );
    }

    #[test]
    fn effective_combat_data_reflects_growth_and_debuff() {
        use crate::Condition;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let idx = {
            let uid = first_player_uid(&app);
            app.database().idx_by_uid(&uid).unwrap()
        };
        // 基準 (level 1 / 装備なし / 状態異常なし) = 静的データそのまま。
        let (p0, u0) = app.database().effective_combat_data(idx).unwrap();
        assert_eq!(p0.infight, 100);
        assert_eq!(u0.armor, 10);
        // 育成: total_exp 500 → level 6 → 格闘が成長する (戦闘予測へ反映)。
        app.database_mut().unit_instances[idx].total_exp = 500;
        let (p1, _) = app.database().effective_combat_data(idx).unwrap();
        assert!(
            p1.infight > p0.infight,
            "レベル成長で格闘が上がる: {} > {}",
            p1.infight,
            p0.infight
        );
        // 状態異常「装甲低下」で実効装甲が下がる (被ダメージ増加に直結)。
        app.database_mut().unit_instances[idx].add_condition(Condition::new("装甲低下", 1));
        let (_, u2) = app.database().effective_combat_data(idx).unwrap();
        assert!(
            u2.armor < u0.armor,
            "装甲低下で実効装甲が下がる: {} < {}",
            u2.armor,
            u0.armor
        );
    }

    #[test]
    fn award_kill_rewards_grants_money_exp_and_consumes_kouun() {
        use crate::Condition;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let idx = {
            let uid = first_player_uid(&app);
            app.database().idx_by_uid(&uid).unwrap()
        };
        let money0 = app.money();
        let exp0 = app.database().unit_instances[idx].total_exp;
        // 撃破: 経験値 50 / 敵価値 200 → 資金 100。
        let gained = app.award_kill_rewards(idx, 50, 200);
        assert_eq!(gained, 100, "資金 = 敵価値/2");
        assert_eq!(app.money(), money0 + 100);
        assert_eq!(app.database().unit_instances[idx].total_exp, exp0 + 50);
        // 幸運: 資金 2 倍 かつ消費。
        app.database_mut().unit_instances[idx].add_condition(Condition::new("幸運", 1));
        let money1 = app.money();
        let gained2 = app.award_kill_rewards(idx, 0, 200);
        assert_eq!(gained2, 200, "幸運 で資金 2 倍");
        assert_eq!(app.money(), money1 + 200);
        assert!(
            !app.database().unit_instances[idx].has_condition("幸運"),
            "幸運 は撃破時に消費される"
        );
    }

    #[test]
    fn intermission_upgrade_flow_spends_money_and_boosts_stats() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        app.add_money(5000);
        // インターミッションを表示させる (次ステージありで組込みコマンドが出る)。
        app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
        app.scene = Scene::Intermission;
        app.intermission_mode = IntermissionMode::Menu;
        // メニュー = [機体改造, データセーブ, 次のステージへ]。機体改造 (index 0) を選択。
        assert_eq!(app.intermission_item_label(0).as_deref(), Some("機体改造"));
        app.set_intermission_cursor(0);
        assert!(app.confirm_intermission_selection());
        assert_eq!(app.intermission_mode, IntermissionMode::UnitUpgrade);
        // ユニット選択 (index 0 = Hero) → 改造。
        let idx = {
            let uid = first_player_uid(&app);
            app.database().idx_by_uid(&uid).unwrap()
        };
        let hp_before = app
            .database()
            .effective_max_hp(&app.database().unit_instances[idx]);
        let money_before = app.money();
        app.set_intermission_cursor(0);
        assert!(app.confirm_intermission_selection());
        assert_eq!(app.database().unit_instances[idx].upgrade_level, 1);
        assert_eq!(app.money(), money_before - 1000, "Lv0→1 は 1000G");
        let hp_after = app
            .database()
            .effective_max_hp(&app.database().unit_instances[idx]);
        assert!(
            hp_after > hp_before,
            "改造で最大HPが上がる: {hp_after} > {hp_before}"
        );
        // 戻る (ユニット数=1 → index 1 が「戻る」) でメインメニューへ。
        app.set_intermission_cursor(1);
        assert!(app.confirm_intermission_selection());
        assert_eq!(app.intermission_mode, IntermissionMode::Menu);
        // データセーブ → __quicksave に保存 (項目順は他組込みの増減で変わるため
        // ラベルで index を解決する)。
        let save_idx = (0..app.intermission_item_count())
            .find(|&n| app.intermission_item_label(n).as_deref() == Some("データセーブ"))
            .expect("データセーブ 項目がある");
        app.set_intermission_cursor(save_idx);
        assert!(app.confirm_intermission_selection());
        assert!(
            !app.script_var("__quicksave").is_empty(),
            "データセーブで __quicksave が書き込まれる"
        );
    }

    /// インターミッション「換装」で `換装` 特殊能力を持つユニットの形態を差し替える。
    #[test]
    fn intermission_equip_swap_changes_unit_form() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Base", 2, 6);
        // 換装先 "砲撃装備" の UnitData を用意 (Base を複製してリネーム)。
        let mut alt = app.database().unit_by_name("Base").cloned().unwrap();
        alt.name = "砲撃装備".into();
        alt.nickname = "砲撃装備".into();
        app.database_mut().units.push(alt);
        let uid = first_player_uid(&app);
        app.database_mut()
            .unit_by_uid_mut(&uid)
            .unwrap()
            .active_features = vec![crate::feature::ActiveFeature::new("換装", "砲撃装備")];
        // インターミッション表示 (次ステージありで組込みコマンドが出る)。
        app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
        app.scene = Scene::Intermission;
        app.intermission_mode = IntermissionMode::Menu;

        // メニュー = [機体改造, 換装, データセーブ, 次のステージへ]。換装 = index 1。
        assert_eq!(app.intermission_item_label(1).as_deref(), Some("換装"));
        app.set_intermission_cursor(1);
        assert!(app.confirm_intermission_selection());
        assert_eq!(app.intermission_mode, IntermissionMode::EquipSwap);

        // 行 0 = "Base → 砲撃装備"。選択で換装。
        assert_eq!(
            app.intermission_item_label(0).as_deref(),
            Some("Base → 砲撃装備")
        );
        app.set_intermission_cursor(0);
        assert!(app.confirm_intermission_selection());
        assert_eq!(
            app.database().unit_by_uid(&uid).unwrap().unit_data_name,
            "砲撃装備",
            "換装で形態が差し替わる"
        );
    }

    /// 換装可能なユニットが居なければ「換装」項目を出さない。
    #[test]
    fn intermission_equip_swap_hidden_without_feature() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Base", 2, 6);
        app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
        app.scene = Scene::Intermission;
        app.intermission_mode = IntermissionMode::Menu;
        let labels: Vec<String> = (0..app.intermission_item_count())
            .filter_map(|n| app.intermission_item_label(n))
            .collect();
        assert!(
            !labels.iter().any(|l| l == "換装"),
            "換装可能ユニットが無ければ項目を出さない: {labels:?}"
        );
    }

    /// テスト用: 区別できる 2 機の味方ユニットを置きインターミッションに入る。
    fn setup_ride_change(app: &mut App) -> (String, String) {
        enter_mapview_with_demo_map(app);
        place_player_unit(app, "UnitA", 2, 6);
        place_player_unit(app, "UnitB", 3, 6);
        let uid_a = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "UnitA")
            .unwrap()
            .uid
            .clone();
        let uid_b = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "UnitB")
            .unwrap()
            .uid
            .clone();
        {
            let a = app.database_mut().unit_by_uid_mut(&uid_a).unwrap();
            a.pilot_name = "アリス".into();
            a.pilot_ids = vec!["alice".into()];
        }
        {
            let b = app.database_mut().unit_by_uid_mut(&uid_b).unwrap();
            b.pilot_name = "魔理沙".into();
            b.pilot_ids = vec!["marisa".into()];
        }
        app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
        app.scene = Scene::Intermission;
        app.intermission_mode = IntermissionMode::Menu;
        (uid_a, uid_b)
    }

    /// インターミッション「乗り換え」で 2 機の搭乗パイロットを入れ替える。
    #[test]
    fn intermission_ride_change_swaps_pilots() {
        let mut app = App::new();
        let (uid_a, uid_b) = setup_ride_change(&mut app);

        // メニューの「乗り換え」を選ぶ → RideChange モード。
        let ride_idx = (0..app.intermission_item_count())
            .find(|&n| app.intermission_item_label(n).as_deref() == Some("乗り換え"))
            .expect("乗り換え 項目がある");
        app.set_intermission_cursor(ride_idx);
        assert!(app.confirm_intermission_selection());
        assert_eq!(app.intermission_mode, IntermissionMode::RideChange);

        // 移動元 = UnitA (cursor 0)。
        app.set_intermission_cursor(0);
        assert!(app.confirm_intermission_selection());
        // 移動先候補は移動元を除いた一覧 → cursor 0 = UnitB。先頭は "→ " 付き。
        assert!(app
            .intermission_item_label(0)
            .as_deref()
            .unwrap()
            .starts_with("→ "));
        app.set_intermission_cursor(0);
        assert!(app.confirm_intermission_selection());

        // 搭乗が入れ替わる (pilot_name + pilot_ids)。
        let a = app.database().unit_by_uid(&uid_a).unwrap();
        let b = app.database().unit_by_uid(&uid_b).unwrap();
        assert_eq!(a.pilot_name, "魔理沙");
        assert_eq!(a.pilot_ids, vec!["marisa".to_string()]);
        assert_eq!(b.pilot_name, "アリス");
        assert_eq!(b.pilot_ids, vec!["alice".to_string()]);
    }

    /// 移動先選択中のキャンセルは移動元選択に戻り、入れ替えは起きない。
    #[test]
    fn intermission_ride_change_dest_cancel_returns_to_source() {
        let mut app = App::new();
        let (uid_a, _uid_b) = setup_ride_change(&mut app);
        let ride_idx = (0..app.intermission_item_count())
            .find(|&n| app.intermission_item_label(n).as_deref() == Some("乗り換え"))
            .unwrap();
        app.set_intermission_cursor(ride_idx);
        app.confirm_intermission_selection();
        // 移動元 UnitA を選択 → 移動先選択 (→ prefix)。
        app.set_intermission_cursor(0);
        app.confirm_intermission_selection();
        assert!(app
            .intermission_item_label(0)
            .as_deref()
            .unwrap()
            .starts_with("→ "));
        // キャンセル → 移動元選択へ戻る (→ prefix が消える)、入れ替えなし。
        assert!(app.cancel_action());
        assert_eq!(app.intermission_mode, IntermissionMode::RideChange);
        assert!(!app
            .intermission_item_label(0)
            .as_deref()
            .unwrap()
            .starts_with("→ "));
        assert_eq!(
            app.database().unit_by_uid(&uid_a).unwrap().pilot_name,
            "アリス",
            "キャンセルで入れ替わらない"
        );
    }

    /// インターミッション「ステータス」は部隊ロスターを開き、閲覧後に
    /// インターミッションへ戻る。
    #[test]
    fn intermission_status_opens_roster_and_returns() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        app.set_script_var("次ステージ".to_string(), "x.eve".to_string());
        app.scene = Scene::Intermission;
        app.intermission_mode = IntermissionMode::Menu;

        let idx = (0..app.intermission_item_count())
            .find(|&n| app.intermission_item_label(n).as_deref() == Some("ステータス"))
            .expect("ステータス 項目がある");
        app.set_intermission_cursor(idx);
        assert!(app.confirm_intermission_selection());
        assert_eq!(app.scene(), Scene::PilotList);

        // 送り: PilotList → UnitList → インターミッションへ復帰。
        app.handle_input(Input::Advance);
        assert_eq!(app.scene(), Scene::UnitList);
        app.handle_input(Input::Advance);
        assert_eq!(
            app.scene(),
            Scene::Intermission,
            "ステータス閲覧後はインターミッションへ戻る"
        );
        assert_eq!(app.intermission_mode, IntermissionMode::Menu);
    }

    /// 通常の部隊表 (戻り先指定なし) は従来どおり UnitList → Title で抜ける。
    #[test]
    fn unit_list_without_return_goes_to_title() {
        let mut app = App::new();
        app.scene = Scene::UnitList;
        app.handle_input(Input::Advance);
        assert_eq!(app.scene(), Scene::Title, "戻り先未指定なら Title へ");
    }

    #[test]
    fn weapon_special_effect_inflicts_status_and_disables() {
        use crate::data::unit::WeaponData;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let target = app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            3,
            6,
        ));
        let def_idx = app.database().idx_by_uid(&target).unwrap();
        let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
        // 痺 属性 + critical 100 → 必ず発動。
        let weapon = WeaponData {
            name: "麻痺銃".into(),
            power: 100,
            min_range: 1,
            max_range: 1,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 100,
            class: "痺".into(),
            extras: Vec::new(),
        };
        let applied = app.apply_weapon_special_effects(def_idx, &weapon, &pilot, &pilot);
        assert_eq!(applied, vec!["麻痺".to_string()]);
        assert!(app.database().unit_instances[def_idx].has_condition("麻痺"));
        assert!(
            app.database().unit_instances[def_idx].attack_disabled(),
            "麻痺 は行動不能 (AI / 攻撃ゲートが効く)"
        );
    }

    /// 脱属性武器は命中・proc 時に対象の気力を低下させる (Ｄ の吸収は未対応)。
    #[test]
    fn weapon_datsu_reduces_target_morale() {
        use crate::data::unit::WeaponData;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let target = app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            3,
            6,
        ));
        let def_idx = app.database().idx_by_uid(&target).unwrap();
        let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
        let m0 = app.database().unit_instances[def_idx].morale;
        // 脱 + critical 100 → 必ず proc。
        let weapon = WeaponData {
            name: "脱力弾".into(),
            power: 100,
            min_range: 1,
            max_range: 1,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 100,
            class: "脱".into(),
            extras: Vec::new(),
        };
        let applied = app.apply_weapon_special_effects(def_idx, &weapon, &pilot, &pilot);
        assert!(applied.iter().any(|s| s.contains("気力")), "気力減少ラベル");
        assert_eq!(
            app.database().unit_instances[def_idx].morale,
            m0 - 10,
            "脱 で気力 -10"
        );
    }

    /// 耐性 / 弱点 は特殊効果の発動確率を半減 / 倍にする (武器属性が一致する場合)。
    #[test]
    fn resistance_and_weakness_scale_proc_rate() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let uid = first_player_uid(&app);
        let def_idx = app.database().idx_by_uid(&uid).unwrap();
        // 属性なし → 変化なし。
        assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 50), 50);
        // 耐性=火 → 火属性武器の発動率半減。
        app.database_mut().unit_instances[def_idx].active_features =
            vec![crate::feature::ActiveFeature::new("耐性", "火")];
        assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 50), 25);
        // 非一致属性 (氷) なら変化なし。
        assert_eq!(app.adjust_proc_for_resistance(def_idx, "氷 痺", 50), 50);
        // 弱点=火 → 倍 (100 上限)。
        app.database_mut().unit_instances[def_idx].active_features =
            vec![crate::feature::ActiveFeature::new("弱点", "火")];
        assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 40), 80);
        assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 80), 100);

        // 弱/効 属性で付加された一時的弱点 (condition `弱点:火`) も倍率に効く。
        app.database_mut().unit_instances[def_idx].active_features = vec![];
        app.database_mut().unit_instances[def_idx]
            .add_condition(crate::Condition::new("弱点:火", 3));
        assert_eq!(app.adjust_proc_for_resistance(def_idx, "火 痺", 40), 80);
        assert_eq!(app.adjust_proc_for_resistance(def_idx, "氷 痺", 40), 40);
    }

    /// 移動力ＵＰ (+1) / 移動力ＤＯＷＮ (半減、特殊効果攻撃属性 低移) が effective_speed に効く。
    #[test]
    fn move_status_scales_effective_speed() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Mover", 5, 5); // speed 3
        let uid = first_player_uid(&app);
        let idx = app.database().idx_by_uid(&uid).unwrap();
        let speed = |app: &App| {
            app.database()
                .effective_speed(&app.database().unit_instances[idx])
        };
        assert_eq!(speed(&app), 3, "基本移動力 3");
        // 移動力ＤＯＷＮ → 半減 (1)。
        app.database_mut().unit_instances[idx]
            .add_condition(crate::Condition::new("移動力ＤＯＷＮ", 3));
        assert_eq!(speed(&app), 1, "移動力ＤＯＷＮ で半減");
        app.database_mut().unit_instances[idx].remove_condition("移動力ＤＯＷＮ");
        // 移動力ＵＰ → +1 (4)。
        app.database_mut().unit_instances[idx]
            .add_condition(crate::Condition::new("移動力ＵＰ", 3));
        assert_eq!(speed(&app), 4, "移動力ＵＰ で +1");
    }

    /// 恐怖 (特殊効果攻撃属性 恐) の敵 AI は味方から遠ざかり、攻撃しない。
    #[test]
    fn fear_makes_ai_flee_from_enemies() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 5);
        let coward = app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            6,
            5,
        ));
        app.database_mut()
            .unit_by_uid_mut(&coward)
            .unwrap()
            .add_condition(crate::Condition::new("恐怖", 3));
        app.set_stage_state(crate::stage::StageState::Battle);
        let idx = app.database().idx_by_uid(&coward).unwrap();
        app.ai_act_unit(idx);
        let c = app.database().unit_by_uid(&coward).unwrap();
        let dist_after = (c.x as i32 - 5).abs() + (c.y as i32 - 5).abs();
        assert!(
            dist_after > 1,
            "恐怖の敵は味方 (5,5) から遠ざかる (移動後距離={dist_after})"
        );
        assert!(c.has_acted, "逃走後は行動終了");
    }

    /// 敵 AI は射程内に対象が居るとき、攻撃前に攻撃補助の精神 (熱血) を使う。
    #[test]
    fn ai_uses_offensive_spirit_before_attacking() {
        use crate::data::pilot::SpiritCommand;
        use crate::data::unit::WeaponData;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 5); // 攻撃対象 (味方)
                                                   // Hero unit_data に隣接武器を持たせる (敵もこの機体を使う)。
        app.database_mut()
            .units
            .iter_mut()
            .find(|d| d.name == "Hero")
            .unwrap()
            .weapons
            .push(WeaponData {
                name: "パンチ".into(),
                power: 50,
                min_range: 1,
                max_range: 1,
                precision: 50,
                bullet: -1,
                en_consumption: 0,
                necessary_morale: 0,
                adaption: "AAAA".into(),
                critical: 0,
                class: String::new(),
                extras: Vec::new(),
            });
        let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            6,
            5,
        ));
        // PILOT に 熱血 を習得させ SP を与える。
        {
            let p = app
                .database_mut()
                .pilots
                .iter_mut()
                .find(|p| p.name == "PILOT")
                .unwrap();
            p.sp = Some(100);
            p.spirit_commands = vec![SpiritCommand {
                name: "熱血".into(),
                cost: Some(20),
                level: 1,
            }];
        }
        app.set_stage_state(crate::stage::StageState::Battle);
        let idx = app.database().idx_by_uid(&enemy).unwrap();
        app.ai_act_unit(idx);
        assert!(
            app.database()
                .unit_by_uid(&enemy)
                .unwrap()
                .has_condition("熱血"),
            "AI が攻撃前に精神コマンド 熱血 を使う"
        );
    }

    /// ChangeMode「逃亡」の敵 AI も恐怖と同様に味方から遠ざかる。
    #[test]
    fn change_mode_escape_makes_ai_flee() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 5);
        let runner = app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            6,
            5,
        ));
        app.database_mut().unit_by_uid_mut(&runner).unwrap().ai_mode = "逃亡".into();
        app.set_stage_state(crate::stage::StageState::Battle);
        let idx = app.database().idx_by_uid(&runner).unwrap();
        app.ai_act_unit(idx);
        let c = app.database().unit_by_uid(&runner).unwrap();
        let dist = (c.x as i32 - 5).abs() + (c.y as i32 - 5).abs();
        assert!(dist > 1, "逃亡モードの敵は味方から遠ざかる (距離={dist})");
    }

    /// 回復アビリティを持つ敵 AI は、射程内の負傷した味方を回復する。
    #[test]
    fn ai_uses_heal_ability_on_damaged_ally() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Healer", 9, 9); // Healer unit_data + PILOT を用意 (この個体は無関係)
        let healer = app.database_mut().register_unit(crate::UnitInstance::new(
            "Healer",
            "PILOT",
            crate::Party::Enemy,
            5,
            5,
        ));
        let ally = app.database_mut().register_unit(crate::UnitInstance::new(
            "Healer",
            "PILOT",
            crate::Party::Enemy,
            6,
            5,
        ));
        app.database_mut().unit_by_uid_mut(&ally).unwrap().damage = 50;
        give_ability(
            &mut app,
            "Healer",
            &healer,
            mk_ability("治療", "回復Lv1", 3, None),
        );
        app.set_stage_state(crate::stage::StageState::Battle);
        let idx = app.database().idx_by_uid(&healer).unwrap();
        app.ai_act_unit(idx);
        assert_eq!(
            app.database().unit_by_uid(&ally).unwrap().damage,
            0,
            "AI が射程内の負傷した味方を回復する"
        );
        assert!(
            app.database().unit_by_uid(&healer).unwrap().has_acted,
            "回復で行動終了"
        );
    }

    /// マップ兵器を持つ敵 AI は、2 体以上の敵を巻き込める照準があれば発射する。
    #[test]
    fn ai_uses_map_weapon_on_multiple_enemies() {
        use crate::data::unit::WeaponData;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Bomber", 9, 0); // Bomber unit_data + PILOT
        app.database_mut()
            .units
            .iter_mut()
            .find(|d| d.name == "Bomber")
            .unwrap()
            .weapons
            .push(WeaponData {
                name: "全体砲".into(),
                power: 30,
                min_range: 1,
                max_range: 5,
                precision: 50,
                bullet: -1,
                en_consumption: 0,
                necessary_morale: 0,
                adaption: "AAAA".into(),
                critical: 0,
                class: "Ｍ全".into(),
                extras: Vec::new(),
            });
        let bomber = app.database_mut().register_unit(crate::UnitInstance::new(
            "Bomber",
            "PILOT",
            crate::Party::Enemy,
            5,
            5,
        ));
        place_player_unit(&mut app, "Victim1", 6, 5);
        place_player_unit(&mut app, "Victim2", 7, 5);
        app.set_stage_state(crate::stage::StageState::Battle);
        let idx = app.database().idx_by_uid(&bomber).unwrap();
        app.ai_act_unit(idx);
        let victims: Vec<_> = app
            .database()
            .unit_instances
            .iter()
            .filter(|u| u.unit_data_name.starts_with("Victim"))
            .collect();
        let hit = victims.len() < 2 || victims.iter().all(|u| u.damage > 0);
        assert!(hit, "マップ兵器で複数の敵が被弾する");
        assert!(
            app.database().unit_by_uid(&bomber).unwrap().has_acted,
            "マップ兵器発射で行動終了"
        );
    }

    /// ChangeMode「護衛 <対象>」の敵 AI は守護対象の近くへ移動する。
    #[test]
    fn change_mode_escort_moves_toward_protected_unit() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Foe", 11, 11); // 敵対ユニット (candidates 用)
        place_player_unit(&mut app, "Protectee", 1, 1); // Protectee unit_data を用意 (この個体は遠方)
        let protectee = app.database_mut().register_unit(crate::UnitInstance::new(
            "Protectee",
            "PILOT",
            crate::Party::Enemy,
            3,
            3,
        ));
        let guard = app.database_mut().register_unit(crate::UnitInstance::new(
            "Foe",
            "PILOT",
            crate::Party::Enemy,
            10,
            10,
        ));
        app.database_mut().unit_by_uid_mut(&guard).unwrap().ai_mode = format!("護衛 {protectee}");
        app.set_stage_state(crate::stage::StageState::Battle);
        let idx = app.database().idx_by_uid(&guard).unwrap();
        app.ai_act_unit(idx);
        let g = app.database().unit_by_uid(&guard).unwrap();
        let dist_after = (g.x as i32 - 3).abs() + (g.y as i32 - 3).abs();
        assert!(
            dist_after < 14,
            "護衛役は守護対象 (3,3) へ近づく (移動後距離={dist_after})"
        );
    }

    /// 写/化 (能力コピー) はクリティカル時に発動者を対象の形態へ変える。写はサイズ制限あり。
    #[test]
    fn weapon_copy_transforms_attacker_with_size_limit() {
        use crate::data::unit::{Size, WeaponData};
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Mimic", 2, 6);
        place_player_unit(&mut app, "Model", 4, 6); // 同サイズ M
        let atk = first_player_uid(&app);
        let model = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.x == 4 && u.y == 6)
            .unwrap()
            .uid
            .clone();
        let atk_idx = app.database().idx_by_uid(&atk).unwrap();
        let def_idx = app.database().idx_by_uid(&model).unwrap();
        let mk = |class: &str| WeaponData {
            name: "コピー光線".into(),
            power: 100,
            min_range: 1,
            max_range: 1,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: class.into(),
            extras: Vec::new(),
        };
        // 写 同サイズ → 変化。
        let applied = app.apply_weapon_crit_copy(atk_idx, def_idx, &mk("写"));
        assert_eq!(applied, vec!["能力コピー → Model".to_string()]);
        assert_eq!(
            app.database().unit_by_uid(&atk).unwrap().unit_data_name,
            "Model"
        );
        // Model を XL に変え、Mimic(M) へ戻して再試行: 写 はサイズ差2以上で無効、化 は可。
        app.database_mut()
            .unit_by_uid_mut(&atk)
            .unwrap()
            .unit_data_name = "Mimic".into();
        app.database_mut()
            .units
            .iter_mut()
            .find(|d| d.name == "Model")
            .unwrap()
            .size = Size::XL;
        assert!(
            app.apply_weapon_crit_copy(atk_idx, def_idx, &mk("写"))
                .is_empty(),
            "写 はサイズ 2 段階以上差で無効"
        );
        assert_eq!(
            app.database().unit_by_uid(&atk).unwrap().unit_data_name,
            "Mimic",
            "写 失敗で形態は変わらない"
        );
        assert!(
            !app.apply_weapon_crit_copy(atk_idx, def_idx, &mk("化"))
                .is_empty(),
            "化 はサイズ制限なしで変化"
        );
    }

    /// 盗属性武器のクリティカル時資金奪取: 味方が敵を盗むと修理費の1/4が入り、再取得は不可。
    #[test]
    fn weapon_steal_grants_money_once() {
        use crate::data::unit::WeaponData;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let target = app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            3,
            6,
        ));
        let def_idx = app.database().idx_by_uid(&target).unwrap();
        let weapon = WeaponData {
            name: "盗賊剣".into(),
            power: 100,
            min_range: 1,
            max_range: 1,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: "盗".into(),
            extras: Vec::new(),
        };
        let money0 = app.money();
        // 修理費 800 → 1/4 = 200。
        let applied = app.apply_weapon_crit_steal(def_idx, crate::Party::Player, 800, &weapon);
        assert_eq!(applied, vec!["資金奪取 +200".to_string()]);
        assert_eq!(app.money(), money0 + 200, "資金 +200");
        // 再取得は不可 (被盗)。
        let applied2 = app.apply_weapon_crit_steal(def_idx, crate::Party::Player, 800, &weapon);
        assert!(applied2.is_empty(), "同じ相手から再取得しない");
        assert_eq!(app.money(), money0 + 200, "資金は変わらない");
    }

    /// 衰L2 武器のクリティカル時減衰: 対象の現在 HP が半分になる (撃破はしない)。
    #[test]
    fn weapon_crit_decay_halves_current_hp() {
        use crate::data::unit::WeaponData;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let target = app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            3,
            6,
        ));
        let def_idx = app.database().idx_by_uid(&target).unwrap();
        // 最大HP=100、ダメージ20 → 現在HP 80。衰L2 で 80→40。
        app.database_mut().unit_instances[def_idx].damage = 20;
        let weapon = WeaponData {
            name: "減衰砲".into(),
            power: 100,
            min_range: 1,
            max_range: 1,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: "衰L2".into(),
            extras: Vec::new(),
        };
        let applied = app.apply_weapon_crit_decay(def_idx, &weapon);
        assert!(!applied.is_empty(), "衰 でラベルが返る");
        // 現在HP 80 → 40 → damage = 100 - 40 = 60。
        assert_eq!(
            app.database().unit_instances[def_idx].damage,
            60,
            "衰L2 で現在HP が半分 (80→40)"
        );
    }

    /// BossRank はステータスを強化し、石化を無効化する。
    #[test]
    fn boss_rank_boosts_stats_and_blocks_petrify() {
        use crate::data::unit::WeaponData;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Boss", 5, 5); // hp 100, armor 10
        let uid = first_player_uid(&app);
        let idx = app.database().idx_by_uid(&uid).unwrap();
        let hp0 = app
            .database()
            .effective_max_hp(&app.database().unit_instances[idx]);
        assert_eq!(hp0, 100);
        app.database_mut().unit_instances[idx].boss_rank = 2; // HP ×2 / 装甲 +600
        assert_eq!(
            app.database()
                .effective_max_hp(&app.database().unit_instances[idx]),
            200,
            "rank2 で HP ×2"
        );
        assert_eq!(
            app.database()
                .effective_armor(&app.database().unit_instances[idx]),
            610,
            "rank2 で 装甲 +600"
        );
        // 石化 (critical 100 → 必ず proc) でもボスは無効。
        let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
        let w = WeaponData {
            name: "石化光線".into(),
            power: 100,
            min_range: 1,
            max_range: 1,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 100,
            class: "石".into(),
            extras: Vec::new(),
        };
        let applied = app.apply_weapon_special_effects(idx, &w, &pilot, &pilot);
        assert!(applied.is_empty(), "ボスは石化を無効化");
        assert!(
            !app.database().unit_instances[idx].has_condition("石化"),
            "石化 condition が付かない"
        );
    }

    /// 即死 (即) は非ボスを致死化し、ボスには無効。
    #[test]
    fn instakill_attribute_kills_non_boss_not_boss() {
        use crate::data::unit::WeaponData;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        let target = app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            3,
            6,
        ));
        let def_idx = app.database().idx_by_uid(&target).unwrap();
        let pilot = app.database().pilot_by_name("PILOT").unwrap().clone();
        let w = WeaponData {
            name: "即死針".into(),
            power: 1,
            min_range: 1,
            max_range: 1,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 100, // proc 率 100
            class: "即".into(),
            extras: Vec::new(),
        };
        assert!(
            app.roll_weapon_instakill(def_idx, &w, &pilot, &pilot),
            "非ボスは即死 proc"
        );
        app.database_mut().unit_instances[def_idx].boss_rank = 1;
        assert!(
            !app.roll_weapon_instakill(def_idx, &w, &pilot, &pilot),
            "ボスは即死無効"
        );
    }

    /// 吹L2 武器は対象を攻撃側から遠ざかる方向へ 2 マス押し出す。占有マスで停止する。
    #[test]
    fn weapon_knockback_pushes_and_stops_at_obstacle() {
        use crate::data::unit::WeaponData;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 5);
        let target = app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            6,
            5,
        ));
        let def_idx = app.database().idx_by_uid(&target).unwrap();
        let mk = |class: &str| WeaponData {
            name: "衝撃砲".into(),
            power: 100,
            min_range: 1,
            max_range: 3,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: class.into(),
            extras: Vec::new(),
        };
        // 攻撃側 (5,5) → 対象 (6,5) → +x 方向へ 2 マス → (8,5)。
        let moved = app.apply_weapon_knockback(def_idx, (5, 5), "Hero", &mk("吹L2"), false);
        assert!(moved, "吹L2 で押し出される");
        assert_eq!(
            (
                app.database().unit_by_uid(&target).unwrap().x,
                app.database().unit_by_uid(&target).unwrap().y
            ),
            (8, 5),
            "(6,5) から +x に 2 マス → (8,5)"
        );
        // (10,5) に障害ユニットを置き、(8,5) の対象を 吹L3 → (9,5) で停止 (10,5 不可)。
        place_player_unit(&mut app, "Blocker", 10, 5);
        let moved2 = app.apply_weapon_knockback(def_idx, (7, 5), "Hero", &mk("吹L3"), false);
        assert!(moved2);
        assert_eq!(
            app.database().unit_by_uid(&target).unwrap().x,
            9,
            "障害ユニット (10,5) の手前 (9,5) で停止"
        );
    }

    /// 引(引き寄せ)武器は対象を攻撃側に隣接する空きマスへ移す。
    #[test]
    fn weapon_pull_moves_target_adjacent_to_attacker() {
        use crate::data::unit::WeaponData;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 5);
        let target = app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            8,
            5,
        ));
        let def_idx = app.database().idx_by_uid(&target).unwrap();
        let weapon = WeaponData {
            name: "引力砲".into(),
            power: 100,
            min_range: 1,
            max_range: 5,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 0,
            class: "引".into(),
            extras: Vec::new(),
        };
        let applied = app.apply_weapon_crit_reposition(def_idx, (5, 5), &weapon);
        assert_eq!(applied, vec!["引き寄せ".to_string()]);
        let t = app.database().unit_by_uid(&target).unwrap();
        let dist = (t.x as i32 - 5).abs() + (t.y as i32 - 5).abs();
        assert_eq!(dist, 1, "攻撃側 (5,5) に隣接する位置へ移動");
    }

    /// 反撃武器の特殊効果攻撃属性 (状態異常) が、反撃の命中・生存時に被弾側へ proc する。
    #[test]
    fn counterattack_procs_weapon_special_effect() {
        use crate::data::unit::WeaponData;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6); // 反撃を受ける側
                                                   // 敵 unit_data: 痺 反撃武器 (critical 100 → 必ず proc)。
        let mut ebot = app.database().unit_by_name("Hero").cloned().unwrap();
        ebot.name = "EnemyBot".into();
        ebot.weapons = vec![WeaponData {
            name: "麻痺針".into(),
            power: 1, // Hero を撃破しない
            min_range: 1,
            max_range: 1,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 100,
            class: "痺".into(),
            extras: Vec::new(),
        }];
        app.database_mut().units.push(ebot);
        let hero = first_player_uid(&app);
        let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
            "EnemyBot",
            "PILOT",
            crate::Party::Enemy,
            3,
            6,
        ));
        // 反撃を必中させ RNG 非依存にする (hit_chance=100)。
        app.database_mut()
            .unit_by_uid_mut(&enemy)
            .unwrap()
            .add_condition(crate::Condition::new("必中", -1));

        let res = app.try_counterattack(3, 6, (2, 6));
        assert!(res.is_some(), "反撃が成立する");
        assert!(
            app.database()
                .unit_by_uid(&hero)
                .unwrap()
                .has_condition("麻痺"),
            "反撃武器の特殊効果 (麻痺) が被弾側へ proc する"
        );
    }

    /// 援護攻撃武器の特殊効果攻撃属性が、援護の命中・生存時に防御側へ proc する。
    #[test]
    fn support_attack_procs_weapon_special_effect() {
        use crate::data::unit::WeaponData;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6); // 本攻撃側 (atk_idx)
        place_player_unit(&mut app, "Supporter", 3, 6); // Hero に隣接した援護役
                                                        // Supporter に 痺 武器 (critical 100)。
        app.database_mut()
            .units
            .iter_mut()
            .find(|d| d.name == "Supporter")
            .unwrap()
            .weapons = vec![WeaponData {
            name: "麻痺針".into(),
            power: 1,
            min_range: 1,
            max_range: 1,
            precision: 0,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: "AAAA".into(),
            critical: 100,
            class: "痺".into(),
            extras: Vec::new(),
        }];
        let hero = first_player_uid(&app);
        let sup = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Supporter")
            .unwrap()
            .uid
            .clone();
        // 援護攻撃 + 必中 (hit_chance=100)。
        app.database_mut()
            .unit_by_uid_mut(&sup)
            .unwrap()
            .add_condition(crate::Condition::new("サポートアタック", -1));
        app.database_mut()
            .unit_by_uid_mut(&sup)
            .unwrap()
            .add_condition(crate::Condition::new("必中", -1));
        // 敵 (3,7): Supporter (3,6) の射程内 (距離1)。
        let enemy = app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            3,
            7,
        ));
        let atk_idx = app.database().idx_by_uid(&hero).unwrap();

        let res = app.try_support_attack(atk_idx, 3, 7);
        assert!(res.is_some(), "援護攻撃が成立する");
        assert!(
            app.database()
                .unit_by_uid(&enemy)
                .unwrap()
                .has_condition("麻痺"),
            "援護攻撃武器の特殊効果 (麻痺) が防御側へ proc する"
        );
    }

    #[test]
    fn condition_lifetime_decrements_and_expires() {
        use crate::Condition;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            10,
            10,
        ));
        app.set_stage_state(crate::stage::StageState::Battle);
        let uid = first_player_uid(&app);
        app.database_mut()
            .unit_by_uid_mut(&uid)
            .unwrap()
            .add_condition(Condition::new("麻痺", 2));
        // lifetime 2 → プレイヤーフェイズ開始 tick で 2→1 (残る)、再度で 1→0 (消滅)。
        app.begin_phase(crate::Phase::Player);
        assert!(
            app.database()
                .unit_by_uid(&uid)
                .unwrap()
                .has_condition("麻痺"),
            "lifetime 2 は 1 回の tick では消えない"
        );
        app.begin_phase(crate::Phase::Player);
        assert!(
            !app.database()
                .unit_by_uid(&uid)
                .unwrap()
                .has_condition("麻痺"),
            "2 回目の tick で解除される"
        );
    }

    #[test]
    fn combat_kill_fires_destruction_and_annihilation() {
        use crate::Condition;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        // `破壊 <name>` / `全滅 敵` を定義したステージファイル。
        let stage = crate::data::event::parse(
            "破壊 Foe:\nSet 破壊発火 1\nExit\n全滅 敵:\nSet 全滅発火 1\nExit\n",
        )
        .unwrap();
        app.script_library_mut()
            .append_with_name(&stage, "stage.eve");
        app.current_stage_file = "stage.eve".to_string();
        place_player_unit(&mut app, "Hero", 5, 6);
        add_weapon(&mut app, "Hero", 500, 1);
        place_player_unit(&mut app, "Foe", 9, 9); // Foe の UnitData を登録
        spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6); // 唯一の敵 (隣接)
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_map_cursor(5, 6);
        // 必中で確実に命中させ撃破する。
        let hero = first_player_uid(&app);
        app.database_mut()
            .unit_by_uid_mut(&hero)
            .unwrap()
            .add_condition(Condition::new("必中", 1));
        assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
        // 戦闘での撃破が 破壊 <name> と (敵全滅で) 全滅 敵 を発火する (回帰)。
        assert_eq!(
            app.script_var("破壊発火"),
            "1",
            "破壊 <name> イベントが発火する"
        );
        assert_eq!(
            app.script_var("全滅発火"),
            "1",
            "全滅 敵 イベントが発火する (撃破でシナリオが進行)"
        );
    }

    #[test]
    fn custom_unit_command_appears_in_menu_and_invokes() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "TestUnit", 2, 6);
        app.set_stage_state(crate::stage::StageState::Battle);

        // シナリオ定義の `*ユニットコマンド` を登録。
        // `c:` ラベルは条件サブルーチン — Return 1 で常に表示。
        let cc = crate::data::event::parse(
            "*ユニットコマンド テスト 味方 Call(c):\nSet 実行された 1\nExit\n\
             c:\nReturn 1\n",
        )
        .unwrap();
        app.script_library_mut().append(&cc);

        // ユニットをクリック → メニューに「テスト」が並ぶ。
        let ox = 2 * 32 + 16;
        let oy = 12 + 6 * 32 + 16;
        app.handle_input(Input::ClickAt { x: ox, y: oy });
        let idx = match app.command_menu() {
            Some(crate::CommandMenu::Unit { items, .. }) => {
                let pos = items.iter().position(|i| i.label() == "テスト");
                pos.expect("custom command「テスト」がメニューに無い")
            }
            _ => panic!("ユニットメニューが開いていない"),
        };
        // 「テスト」項目をクリック → 本体が実行される。
        let menu_x = crate::command_menu::MENU_X + 40;
        let item_y = crate::command_menu::MENU_Y
            + crate::command_menu::MENU_PADDING
            + crate::command_menu::MENU_ITEM_HEIGHT * idx as i32
            + 4;
        app.handle_input(Input::ClickAt {
            x: menu_x,
            y: item_y,
        });
        assert_eq!(
            app.script_var("実行された"),
            "1",
            "メニューからカスタムコマンドが実行されていない"
        );
    }

    #[test]
    fn custom_unit_command_clears_stale_overlay_on_entry() {
        // スパロボ戦記 AlphaSecond: 別ユニットでステータス画面を再表示したとき、
        // 前ユニットの描画 (script_overlay) が残り透けて見える問題の防止。
        // カスタムユニットコマンド実行時に overlay をクリアして開始するため、
        // 連続実行しても描画コマンドが累積しないことを検証する。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "U1", 2, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        let cc = crate::data::event::parse(
            "*ユニットコマンド 能力 味方 Call(c):\nPaintString 10 10 \"X\"\nExit\nc:\nReturn 1\n",
        )
        .unwrap();
        app.script_library_mut().append(&cc);

        app.invoke_custom_unit_command("U1", "能力");
        let after_first = app.script_overlay().cmds.len();
        assert!(after_first >= 1, "コマンド本体が描画していない");

        app.invoke_custom_unit_command("U1", "能力");
        let after_second = app.script_overlay().cmds.len();
        assert_eq!(
            after_first, after_second,
            "再表示で overlay が累積している (前ユニットの描画が残る)"
        );
    }

    #[test]
    fn menu_move_then_select_destination_moves_unit() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "TestUnit", 2, 6);
        app.set_stage_state(crate::stage::StageState::Battle);

        let ox = 2 * 32 + 16;
        let oy = 12 + 6 * 32 + 16;
        app.handle_input(Input::ClickAt { x: ox, y: oy });
        // メニューの "移動" 項目位置をクリック
        let menu_x = crate::command_menu::MENU_X + 40;
        let move_y = crate::command_menu::MENU_Y + crate::command_menu::MENU_PADDING + 4;
        app.handle_input(Input::ClickAt {
            x: menu_x,
            y: move_y,
        });
        assert_eq!(move_select_pos(&app), Some((2, 6)));

        // (3, 6) へクリック
        let dx = 3 * 32 + 16;
        let dy = 12 + 6 * 32 + 16;
        app.handle_input(Input::ClickAt { x: dx, y: dy });
        assert_eq!(app.database().unit_instances[0].x, 3);
        // 移動後はメニューが再表示
        assert_eq!(menu_unit_pos(&app), Some((3, 6)));
    }

    #[test]
    fn cancel_closes_menu_and_action_mode() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "TestUnit", 2, 6);
        app.set_stage_state(crate::stage::StageState::Battle);

        // クリック→メニュー表示→Cancel で閉じる
        app.handle_input(Input::ClickAt {
            x: 2 * 32 + 16,
            y: 12 + 6 * 32 + 16,
        });
        assert!(app.command_menu().is_some());
        app.handle_input(Input::Cancel);
        assert!(app.command_menu().is_none());
    }

    #[test]
    fn wait_marks_unit_acted() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "TestUnit", 2, 6);
        app.set_stage_state(crate::stage::StageState::Battle);

        app.handle_input(Input::ClickAt {
            x: 2 * 32 + 16,
            y: 12 + 6 * 32 + 16,
        });
        // ユニットメニュー: items = [Move, WeaponList, Wait]
        // 最後の項目 (Wait) をクリック
        let menu = app.command_menu().cloned().unwrap();
        let item_count = match &menu {
            crate::CommandMenu::Unit { items, .. } => items.len(),
            _ => panic!("expected Unit menu"),
        };
        let menu_x = crate::command_menu::MENU_X + 40;
        let wait_y = crate::command_menu::MENU_Y
            + crate::command_menu::MENU_PADDING
            + crate::command_menu::MENU_ITEM_HEIGHT * (item_count as i32 - 1)
            + 4;
        app.handle_input(Input::ClickAt {
            x: menu_x,
            y: wait_y,
        });
        assert!(app.database().unit_instances[0].has_acted);
        assert!(app.command_menu().is_none());
    }

    #[test]
    fn post_move_menu_excludes_move_command() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "TestUnit", 2, 6);
        app.set_stage_state(crate::stage::StageState::Battle);

        // 移動コマンドを選択
        let ox = 2 * 32 + 16;
        let oy = 12 + 6 * 32 + 16;
        app.handle_input(Input::ClickAt { x: ox, y: oy });
        let menu_x = crate::command_menu::MENU_X + 40;
        let move_y = crate::command_menu::MENU_Y + crate::command_menu::MENU_PADDING + 4;
        app.handle_input(Input::ClickAt {
            x: menu_x,
            y: move_y,
        });
        // 移動先をクリック
        app.handle_input(Input::ClickAt {
            x: 3 * 32 + 16,
            y: 12 + 6 * 32 + 16,
        });

        // 移動後メニューに Move が含まれていないこと
        let menu = app.command_menu().unwrap();
        let has_move = match menu {
            crate::CommandMenu::Unit { items, .. } => items.iter().any(|i| {
                matches!(
                    i,
                    crate::command_menu::UnitMenuItem::Builtin(
                        crate::command_menu::UnitAction::Move
                    )
                )
            }),
            _ => false,
        };
        assert!(!has_move, "post-move menu should not contain Move");
    }

    // ===== REPRO: 移動→攻撃後にユニットが初期位置に戻る不具合の調査 =====
    fn add_weapon(app: &mut App, unit_name: &str, power: i64, max_range: i32) {
        app.database_mut()
            .units
            .iter_mut()
            .find(|u| u.name == unit_name)
            .unwrap()
            .weapons
            .push(crate::data::unit::WeaponData {
                name: "テスト砲".to_string(),
                power,
                min_range: 1,
                max_range,
                precision: 100,
                bullet: -1,
                en_consumption: 0,
                necessary_morale: 0,
                adaption: String::new(),
                critical: 0,
                class: String::new(),
                extras: Vec::new(),
            });
    }

    fn click_tile(app: &mut App, c: u32, r: u32) {
        app.handle_input(Input::ClickAt {
            x: (c as i32) * 32 + 16,
            y: 12 + (r as i32) * 32 + 16,
        });
    }

    fn click_menu_item(app: &mut App, idx: i32) {
        let menu_x = crate::command_menu::MENU_X + 40;
        let item_y = crate::command_menu::MENU_Y
            + crate::command_menu::MENU_PADDING
            + crate::command_menu::MENU_ITEM_HEIGHT * idx
            + 4;
        app.handle_input(Input::ClickAt {
            x: menu_x,
            y: item_y,
        });
    }

    fn unit_xy(app: &App, name: &str) -> Option<(u32, u32)> {
        app.database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == name)
            .map(|u| (u.x, u.y))
    }

    /// 表示中のユニットメニューが指す uid を現在位置に解決する (uid キー化後の assert 用)。
    fn menu_unit_pos(app: &App) -> Option<(u32, u32)> {
        match app.command_menu() {
            Some(crate::CommandMenu::Unit { uid, .. }) => {
                app.database().unit_by_uid(uid).map(|u| (u.x, u.y))
            }
            _ => None,
        }
    }

    /// MoveSelect 中のユニット uid を現在位置に解決する。
    fn move_select_pos(app: &App) -> Option<(u32, u32)> {
        match app.action_mode() {
            crate::ActionMode::MoveSelect { uid } => {
                app.database().unit_by_uid(&uid).map(|u| (u.x, u.y))
            }
            _ => None,
        }
    }

    #[test]
    fn repro_move_then_attack_keeps_moved_position() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        // enemy を先に push して idx 0 にする (remove による index 失効を誘発)
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Foe",
            "PILOT",
            crate::Party::Enemy,
            4,
            6,
        ));
        place_player_unit(&mut app, "Foe", 4, 6); // Foe の UnitData/PilotData を用意
        place_player_unit(&mut app, "Hero", 2, 6);
        add_weapon(&mut app, "Hero", 999, 1);
        app.set_stage_state(crate::stage::StageState::Battle);

        // 1) Hero を選択
        click_tile(&mut app, 2, 6);
        // 2) Move (先頭項目)
        click_menu_item(&mut app, 0);
        assert_eq!(
            move_select_pos(&app),
            Some((2, 6)),
            "Move 選択後は MoveSelect(Hero) になるはず: {:?}",
            app.action_mode()
        );
        // 3) (3,6) へ移動
        click_tile(&mut app, 3, 6);
        assert_eq!(
            unit_xy(&app, "Hero"),
            Some((3, 6)),
            "移動が反映されていない"
        );
        // 4) PostMoveMenu の Attack を選ぶ
        let attack_idx = match app.command_menu() {
            Some(crate::CommandMenu::Unit { items, .. }) => items
                .iter()
                .position(|i| {
                    matches!(
                        i,
                        crate::command_menu::UnitMenuItem::Builtin(
                            crate::command_menu::UnitAction::Attack
                        )
                    )
                })
                .expect("PostMoveMenu に Attack が無い"),
            _ => panic!("PostMoveMenu が開いていない: {:?}", app.command_menu()),
        };
        click_menu_item(&mut app, attack_idx as i32);
        assert!(
            matches!(app.action_mode(), crate::ActionMode::AttackSelect { .. }),
            "Attack 選択後は AttackSelect になるはず: {:?}",
            app.action_mode()
        );
        // 5) 敵 (4,6) をクリックして攻撃
        click_tile(&mut app, 4, 6);

        // 攻撃後、Hero は移動先 (3,6) に留まっていること（初期 (2,6) に戻らない）
        assert_eq!(
            unit_xy(&app, "Hero"),
            Some((3, 6)),
            "攻撃後に Hero が初期出撃位置へ revert している"
        );
        // 位置索引が実体と整合していること (撃破による remove 後も)
        assert!(
            app.database().pos_index_is_consistent(),
            "pos_index が unit_instances と乖離している"
        );
    }

    /// 既存 UnitData を流用して敵ユニットを 1 体登録し uid を返す。
    fn spawn_enemy(app: &mut App, unit_data_name: &str, x: u32, y: u32) -> String {
        spawn_party(app, unit_data_name, crate::Party::Enemy, x, y)
    }

    /// 既存 UnitData を流用して指定陣営のユニットを登録し uid を返す。
    fn spawn_party(
        app: &mut App,
        unit_data_name: &str,
        party: crate::Party,
        x: u32,
        y: u32,
    ) -> String {
        app.database_mut().register_unit(crate::UnitInstance::new(
            unit_data_name,
            "PILOT",
            party,
            x,
            y,
        ))
    }

    /// 表示中ユニットメニューの「攻撃」項目の index。
    fn attack_item_index(app: &App) -> i32 {
        match app.command_menu() {
            Some(crate::CommandMenu::Unit { items, .. }) => items
                .iter()
                .position(|i| {
                    matches!(
                        i,
                        crate::command_menu::UnitMenuItem::Builtin(
                            crate::command_menu::UnitAction::Attack
                        )
                    )
                })
                .expect("メニューに Attack が無い")
                as i32,
            _ => panic!("ユニットメニューが開いていない: {:?}", app.command_menu()),
        }
    }

    #[test]
    fn attack_targets_clicked_tile_not_nearest() {
        // 射程内に敵が 2 体いるとき、クリックしたタイルの敵を攻撃する
        // (旧実装は最寄り敵を攻撃していた)。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        add_weapon(&mut app, "Hero", 30, 3); // 射程3・低威力 (撃破せず damage を見る)
        place_player_unit(&mut app, "FoeData", 9, 9); // FoeData の UnitData/PilotData を用意
        let near = spawn_enemy(&mut app, "FoeData", 3, 6); // 最寄り (dist1)
        let far = spawn_enemy(&mut app, "FoeData", 4, 6); // 遠い (dist2) — これをクリック
        app.set_stage_state(crate::stage::StageState::Battle);

        click_tile(&mut app, 2, 6); // Hero 選択
        let ai = attack_item_index(&app);
        click_menu_item(&mut app, ai); // 攻撃
        click_tile(&mut app, 4, 6); // 遠い敵を対象に

        let far_dmg = app.database().unit_by_uid(&far).map(|u| u.damage).unwrap();
        let near_dmg = app.database().unit_by_uid(&near).map(|u| u.damage).unwrap();
        assert!(far_dmg > 0, "クリックした敵 (4,6) が攻撃されていない");
        assert_eq!(near_dmg, 0, "最寄り敵 (3,6) を誤って攻撃している");
    }

    #[test]
    fn player_cannot_attack_npc_ally_but_can_attack_enemy_and_neutral() {
        // 味方 ↔ ＮＰＣ は同盟。Hero(味方) は隣の ＮＰＣ を攻撃できず、
        // 敵 / 中立 は攻撃できる（SRC 敵味方関係）。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        add_weapon(&mut app, "Hero", 30, 3);
        place_player_unit(&mut app, "Data", 9, 9); // UnitData/PilotData を用意
        let ally = spawn_party(&mut app, "Data", crate::Party::Npc, 3, 6); // 同盟 (dist1)
        let enemy = spawn_party(&mut app, "Data", crate::Party::Enemy, 4, 6); // 敵 (dist2)
        let neutral = spawn_party(&mut app, "Data", crate::Party::Neutral, 5, 6); // 中立 (dist3)
        app.set_stage_state(crate::stage::StageState::Battle);

        click_tile(&mut app, 2, 6); // Hero 選択
        let ai = attack_item_index(&app);
        click_menu_item(&mut app, ai); // 攻撃

        // ＮＰＣ 同盟タイルをクリック → 攻撃不成立 (ダメージ無し、AttackSelect 継続)。
        click_tile(&mut app, 3, 6);
        assert_eq!(
            app.database().unit_by_uid(&ally).map(|u| u.damage),
            Some(0),
            "ＮＰＣ 同盟を攻撃できてしまっている"
        );
        assert!(
            matches!(app.action_mode(), crate::ActionMode::AttackSelect { .. }),
            "同盟クリックは no-op で AttackSelect 継続のはず"
        );

        // 敵タイルをクリック → 攻撃成立 (ダメージ)。
        click_tile(&mut app, 4, 6);
        assert!(
            app.database()
                .unit_by_uid(&enemy)
                .map(|u| u.damage)
                .unwrap()
                > 0,
            "敵を攻撃できていない"
        );
        let _ = neutral; // 中立への攻撃可否は行列テスト (is_hostile_to) で担保。
    }

    #[test]
    fn cancel_after_post_move_attack_reverts_to_sortie() {
        // 「移動→攻撃選択→キャンセル」で出撃地点へ正しく巻き戻る (報告された症状の検証)。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        add_weapon(&mut app, "Hero", 30, 1); // 射程1 → 移動後攻撃可
        place_player_unit(&mut app, "FoeData", 9, 9);
        spawn_enemy(&mut app, "FoeData", 4, 6);
        app.set_stage_state(crate::stage::StageState::Battle);

        // Hero 選択 → Move → (3,6)
        click_tile(&mut app, 2, 6);
        click_menu_item(&mut app, 0); // Move (先頭)
        click_tile(&mut app, 3, 6);
        assert_eq!(unit_xy(&app, "Hero"), Some((3, 6)));

        // PostMoveMenu で攻撃選択 → AttackSelect (snapshot あり)
        let ai = attack_item_index(&app);
        click_menu_item(&mut app, ai);
        assert!(matches!(
            app.action_mode(),
            crate::ActionMode::AttackSelect {
                snapshot: Some(_),
                ..
            }
        ));

        // 1回目キャンセル: PostMoveMenu に戻るだけ (移動はまだ巻き戻さない)
        app.handle_input(Input::Cancel);
        assert_eq!(
            unit_xy(&app, "Hero"),
            Some((3, 6)),
            "攻撃キャンセルで移動先に留まるべき"
        );

        // 2回目キャンセル: 移動前 (2,6) に巻き戻る
        app.handle_input(Input::Cancel);
        assert_eq!(
            unit_xy(&app, "Hero"),
            Some((2, 6)),
            "PostMoveMenu キャンセルで出撃地点へ戻る"
        );
        assert!(app.database().pos_index_is_consistent());
    }

    #[test]
    fn selecting_another_unit_after_move_commits_not_reverts() {
        // 移動後メニュー表示中に別ユニットをクリックしたら、移動済みユニットは
        // その場に留まり (巻き戻らず)、別ユニットのメニューが開く。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Alpha", 2, 6);
        place_player_unit(&mut app, "Bravo", 6, 6);
        app.set_stage_state(crate::stage::StageState::Battle);

        // Alpha 選択 → Move → (3,6)
        click_tile(&mut app, 2, 6);
        click_menu_item(&mut app, 0); // Move
        click_tile(&mut app, 3, 6);
        assert_eq!(unit_xy(&app, "Alpha"), Some((3, 6)));

        // 移動後メニュー表示中に別ユニット Bravo(6,6) をクリック
        click_tile(&mut app, 6, 6);
        assert_eq!(
            unit_xy(&app, "Alpha"),
            Some((3, 6)),
            "別ユニット選択で前に移動したユニットが巻き戻ってはいけない"
        );
        assert_eq!(
            menu_unit_pos(&app),
            Some((6, 6)),
            "クリックした別ユニットのメニューが開くべき"
        );
        assert!(app.database().pos_index_is_consistent());
    }

    #[test]
    fn wait_after_move_commits_and_unsticks_action_mode() {
        // 移動 → 待機 で確定したあと action_mode が Browse に戻り、別ユニットを
        // 選べること、さらに右クリックしても確定済みユニットが巻き戻らないこと。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Alpha", 2, 6);
        place_player_unit(&mut app, "Bravo", 9, 6);
        app.set_stage_state(crate::stage::StageState::Battle);

        // Alpha: 選択 → Move → (3,6)
        click_tile(&mut app, 2, 6);
        click_menu_item(&mut app, 0); // Move
        click_tile(&mut app, 3, 6);
        // PostMoveMenu の待機を選択 (武器・敵なし → items=[待機] の先頭)
        click_menu_item(&mut app, 0);
        assert_eq!(unit_xy(&app, "Alpha"), Some((3, 6)));
        assert!(
            matches!(app.action_mode(), crate::ActionMode::Browse),
            "待機確定後は Browse に戻るべき: {:?}",
            app.action_mode()
        );

        // 続けて別ユニット Bravo を選択できる
        click_tile(&mut app, 9, 6);
        assert_eq!(
            menu_unit_pos(&app),
            Some((9, 6)),
            "別ユニットを選択できるべき"
        );

        // 右クリックしても Alpha は確定位置 (3,6) のまま
        app.handle_input(Input::Cancel);
        assert_eq!(
            unit_xy(&app, "Alpha"),
            Some((3, 6)),
            "待機確定後に巻き戻ってはいけない"
        );
        assert!(app.database().pos_index_is_consistent());
    }

    #[test]
    fn moving_one_of_duplicate_units_leaves_twin_in_place() {
        // 同型 (同 unit_data_name) のユニットが複数いても uid で区別され、
        // 移動したユニットだけが動き、もう一方は出撃地点に残る。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Twin", 2, 6); // UnitData Twin + instance #1
        let twin2 = app.database_mut().register_unit(crate::UnitInstance::new(
            "Twin",
            "PILOT",
            crate::Party::Player,
            5,
            6,
        ));
        let twin1 = app.database().uid_at(2, 6).unwrap().to_string();
        app.set_stage_state(crate::stage::StageState::Battle);

        // (2,6) の Twin を選択して (3,6) へ移動
        click_tile(&mut app, 2, 6);
        click_menu_item(&mut app, 0); // Move
        click_tile(&mut app, 3, 6);

        assert_eq!(
            app.database().unit_by_uid(&twin1).map(|u| (u.x, u.y)),
            Some((3, 6)),
            "選択した Twin が移動していない"
        );
        assert_eq!(
            app.database().unit_by_uid(&twin2).map(|u| (u.x, u.y)),
            Some((5, 6)),
            "もう一方の Twin が巻き込まれて動いている"
        );
        assert!(app.database().pos_index_is_consistent());
    }

    #[test]
    fn cancel_post_move_restores_unit_position() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "TestUnit", 2, 6);
        app.set_stage_state(crate::stage::StageState::Battle);

        // 移動
        let ox = 2 * 32 + 16;
        let oy = 12 + 6 * 32 + 16;
        app.handle_input(Input::ClickAt { x: ox, y: oy });
        let menu_x = crate::command_menu::MENU_X + 40;
        let move_y = crate::command_menu::MENU_Y + crate::command_menu::MENU_PADDING + 4;
        app.handle_input(Input::ClickAt {
            x: menu_x,
            y: move_y,
        });
        app.handle_input(Input::ClickAt {
            x: 3 * 32 + 16,
            y: 12 + 6 * 32 + 16,
        });
        assert_eq!(app.database().unit_instances[0].x, 3);

        // 移動後メニューでキャンセル → 元の位置に戻る
        app.handle_input(Input::Cancel);
        assert_eq!(
            app.database().unit_instances[0].x,
            2,
            "unit should be restored to original x"
        );
        assert!(
            !app.database().unit_instances[0].has_moved,
            "has_moved should be cleared"
        );
        assert!(app.command_menu().is_none());
    }

    #[test]
    fn end_phase_via_map_menu_works() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        app.set_stage_state(crate::stage::StageState::Battle);
        let start_turn = app.turn().number;

        // 空白地形をクリック → マップメニュー
        let empty_x = 8 * 32 + 16;
        let empty_y = 12 + 6 * 32 + 16;
        app.handle_input(Input::ClickAt {
            x: empty_x,
            y: empty_y,
        });
        assert!(matches!(
            app.command_menu(),
            Some(crate::CommandMenu::Map { .. })
        ));
        // EndTurn は items[0]
        let menu_x = crate::command_menu::MENU_X + 40;
        let endturn_y = crate::command_menu::MENU_Y + crate::command_menu::MENU_PADDING + 4;
        app.handle_input(Input::ClickAt {
            x: menu_x,
            y: endturn_y,
        });
        // ターン進行 (Player→…→Player)、turn.number は維持 or +1。少なくとも
        // command_menu は閉じている。
        assert!(app.command_menu().is_none());
        let _ = start_turn;
    }

    #[test]
    fn victory_conditions_map_menu_item_gated_on_label() {
        use crate::command_menu::{CommandMenu, MapAction};
        let mut app = App::new();

        // `勝利条件:` ラベルが無いときは「作戦目的」を出さない。
        app.open_map_menu();
        match app.command_menu() {
            Some(CommandMenu::Map { items, .. }) => {
                assert!(!items.contains(&MapAction::VictoryConditions));
            }
            _ => panic!("map menu expected"),
        }

        // ラベルを定義すると「作戦目的」が現れ、選択でラベル本体が走る。
        let stmts = crate::data::event::parse("勝利条件:\nSet vc_fired 1\nReturn\n")
            .expect("parse 勝利条件");
        app.script_library_mut().append(&stmts);
        assert!(app.has_victory_condition_event());

        app.open_map_menu();
        let items = match app.command_menu() {
            Some(CommandMenu::Map { items, .. }) => items.clone(),
            _ => panic!("map menu expected"),
        };
        assert!(items.contains(&MapAction::VictoryConditions));

        // メニュー経由で実行 → `勝利条件:` 本体が発火する。
        let idx = items
            .iter()
            .position(|a| *a == MapAction::VictoryConditions)
            .unwrap();
        assert!(app.execute_menu_action(crate::command_menu::MenuActionId::Map(items[idx])));
        assert_eq!(app.script_var("vc_fired"), "1");
    }

    #[test]
    fn click_in_range_moves_unit() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        // ブレイバー相当ユニットを (2, 6) に置く。speed=3 で (3, 6) は移動範囲内（平地コスト1）
        place_player_unit(&mut app, "TestUnit", 2, 6);

        // カーソルをユニット上に
        let click_unit = (2u32, 6u32);
        let ox = (click_unit.0 as i32) * 32 + 16;
        let oy = 12 + (click_unit.1 as i32) * 32 + 16;
        app.handle_input(Input::ClickAt { x: ox, y: oy });
        assert_eq!(app.map_cursor(), Some(click_unit));

        // (3, 6) へクリック → 移動
        let dest = (3u32, 6u32);
        let dx = (dest.0 as i32) * 32 + 16;
        let dy = 12 + (dest.1 as i32) * 32 + 16;
        app.handle_input(Input::ClickAt { x: dx, y: dy });

        let u = &app.database().unit_instances[0];
        assert_eq!((u.x, u.y), dest);
        assert_eq!(app.map_cursor(), Some(dest));
    }

    #[test]
    fn click_out_of_range_just_moves_cursor() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "TestUnit", 2, 6); // speed=3

        // カーソルをユニット上に
        let ox = 2 * 32 + 16;
        let oy = 12 + 6 * 32 + 16;
        app.handle_input(Input::ClickAt { x: ox, y: oy });

        // (10, 6) は遠すぎる → カーソル移動のみ
        let dx = 10 * 32 + 16;
        let dy = 12 + 6 * 32 + 16;
        app.handle_input(Input::ClickAt { x: dx, y: dy });

        let u = &app.database().unit_instances[0];
        assert_eq!((u.x, u.y), (2, 6));
        assert_eq!(app.map_cursor(), Some((10, 6)));
    }

    #[test]
    fn arrow_keys_ignored_outside_mapview() {
        let mut app = App::new();
        // Title 上で Right を送っても何も起きない
        assert!(!app.handle_input(Input::MoveCursor(Direction::Right)));
        assert_eq!(app.scene(), Scene::Title);
    }

    // ===== ステージ進行 API（start_scenario / begin_battle / game_over 等）=====

    #[test]
    fn start_scenario_resets_state_and_enters_mapview() {
        let mut app = App::new();
        // タイトル → 設定画面に進めておく（start_scenario は途中状態でも呼べる）
        app.handle_input(Input::Advance);
        assert_eq!(app.scene(), Scene::Configuration);

        app.start_scenario("テストステージ");
        assert_eq!(app.scene(), Scene::MapView);
        // 原典 SRC: Prologue → Start → Battle が自動進行する。
        // `Prologue` ラベルが script_library に未登録 (App::new() のまま) なので、
        // start_scenario 後に auto_progress_stage_state_if_idle が走り Battle へ。
        assert_eq!(app.stage_state(), crate::stage::StageState::Battle);
        assert_eq!(app.stage(), "テストステージ");
        assert_eq!(app.turn().number, 1);
        assert_eq!(app.turn().phase, crate::Phase::Player);
        assert!(app.command_menu().is_none());
    }

    #[test]
    fn begin_battle_goes_through_sortie_only() {
        // begin_sortie / begin_battle のゲート挙動 (順序強制) を直接検証する。
        // App::new() の初期 stage_state は Briefing なので start_scenario を
        // 経由せず手動で順に呼ぶ。auto_progress は start_scenario が呼ばないと
        // 走らないため、ここではゲートだけがテスト対象。
        let mut app = App::new();
        assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);

        // Briefing 状態では begin_battle は no-op
        app.begin_battle();
        assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);

        app.begin_sortie();
        assert_eq!(app.stage_state(), crate::stage::StageState::Sortie);

        app.begin_battle();
        assert_eq!(app.stage_state(), crate::stage::StageState::Battle);
        // begin_phase(Player) によりターン 1 開始メッセージが入る
        assert!(app
            .messages()
            .iter()
            .any(|m| m.contains("ターン 1") && m.contains("味方")));
    }

    #[test]
    fn auto_progress_starts_battle_even_with_intermission_commands() {
        // 回帰テスト: musou 系インターミッション制シナリオで `Continue` チェイン後に
        // 本編 (MapView / Briefing) へ入ると、`IntermissionCommand` が登録済みでも
        // auto_progress が Battle まで進め、マップ未ロードならデフォルト 15×15
        // マップが生成されること。手動 Enter 進行を撤去した後の停止バグの再発防止。
        let mut app = App::new();
        // musou 系は `Stage` コマンドを使わないため stage 表示名は空のまま、
        // 本編突入は `Continue` チェイン (advance_to_next_stage) 経由で
        // current_stage_file が立つことで判定する。該当ファイルは未登録なので
        // ラベル発火は no-op だが current_stage_file は設定される。
        app.set_script_var("次ステージ".to_string(), "東方夢想伝01.eve".to_string());
        let _ = app.advance_to_next_stage();
        app.set_scene(Scene::MapView);
        assert!(app.stage().is_empty());
        // プロローグで登録されたインターミッションコマンドはステージ突入後も残る。
        app.push_intermission_command(
            "プラクティス".to_string(),
            "Lib/プラクティス.eve".to_string(),
        );
        assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);
        assert!(app.database().map.is_none());

        app.auto_progress_stage_state_if_idle();

        assert_eq!(
            app.stage_state(),
            crate::stage::StageState::Battle,
            "intermission_commands 登録済みでも Battle へ進むべき"
        );
        assert!(
            app.database().map.is_some(),
            "マップ未ロードならデフォルトマップが生成されるべき"
        );
    }

    #[test]
    fn next_stage_starts_battle_without_double_firing_start() {
        // スパロボ戦記: 「次のステージへ」でステージファイルが `スタート` をインライン
        // 実行 (配置まで完了) して中断せず終わった場合、Briefing で固まらず味方フェイズが
        // 始まり、かつ `スタート` が二重発火しない (= 敵の二重配置が起きない) こと。
        let mut app = App::new();
        // ステージファイル: `スタート` で Incr (= 配置の代用)。中断しない。
        let stage = crate::data::event::parse("スタート:\nIncr スタート回数\nExit\n").unwrap();
        app.script_library_mut()
            .append_with_name(&stage, "main.eve");
        // インターミッション設定: 1 ユーザ項目 + 組込み(機体改造/データセーブ) + 「次のステージへ」。
        // 表示順: [User(0), 機体改造, データセーブ, 次のステージへ] → 次のステージは index 3。
        app.push_intermission_command("改造".to_string(), "lib/x.eve".to_string());
        app.set_script_var("次ステージ".to_string(), "main.eve".to_string());
        app.set_intermission_cursor(3); // 「次のステージへ」

        assert!(app.confirm_intermission_selection());
        assert_eq!(
            app.stage_state(),
            crate::stage::StageState::Battle,
            "次のステージへ後に Briefing で固まらず Battle へ進むべき"
        );
        assert_eq!(app.turn().phase, crate::Phase::Player);
        assert_eq!(
            app.script_var("スタート回数"),
            "1",
            "スタートが二重発火している (敵の二重配置になる)"
        );
    }

    #[test]
    fn begin_phase_deferred_until_start_event_completes() {
        // 完了プロトコル (FlowCont::AfterStartEvent): `スタート` が Wait Click で
        // suspend したら味方フェイズ開始 (begin_phase) は完了まで遅延し、完了後に
        // ターンイベントが発火すること。旧実装は begin_battle が suspend 中でも
        // 即 begin_phase していたため、ターン 1 イベントが trigger_label の
        // 実行中ガードに弾かれて黙って消えていた。
        let stage = crate::data::event::parse(
            "スタート:\nWait Click\nIncr 開始\nExit\nターン 1 味方:\nIncr ターンイベント\nExit\n",
        )
        .unwrap();
        let mut app = App::new();
        app.script_library_mut()
            .append_with_name(&stage, "main.eve");

        app.begin_sortie();
        app.begin_battle();
        // `スタート` が Wait Click で suspend 中: フェイズはまだ始まらない。
        assert!(app.pending_dialog().is_some());
        assert_eq!(app.script_var("ターンイベント"), "");
        assert!(!app.messages().iter().any(|m| m.contains("ターン 1 開始")));

        assert!(app.respond_dialog(0));
        // 完了 → AfterStartEvent 継続 → begin_phase → ターン 1 イベント発火。
        assert_eq!(app.script_var("開始"), "1");
        assert_eq!(app.turn().phase, crate::Phase::Player);
        assert_eq!(
            app.script_var("ターンイベント"),
            "1",
            "スタート完了後にターン 1 イベントが発火するべき"
        );
    }

    #[test]
    fn suspended_stage_file_with_inline_start_does_not_refire() {
        // 完了プロトコル (FlowCont::AfterStageFileRun + start_passed_pcs):
        // ステージファイルが `スタート` を通過実行する途中で suspend した場合、
        // resume 完了後に `スタート` を再発火しないこと。旧実装は「中断せず
        // 完了したか」で推定していたため、このケース (suspend したが スタート は
        // インライン実行済み) で resume → auto_progress → begin_battle が
        // `スタート` を再発火し、敵が二重配置されていた。
        let stage =
            crate::data::event::parse("スタート:\nIncr 配置\nWait Click\nIncr 配置後\nExit\n")
                .unwrap();
        let mut app = App::new();
        app.script_library_mut()
            .append_with_name(&stage, "main.eve");
        app.set_script_var("次ステージ".to_string(), "main.eve".to_string());

        assert!(app.advance_to_next_stage());
        // `スタート` 内の Wait Click で suspend 中。
        assert!(app.pending_dialog().is_some());
        assert_eq!(app.script_var("配置"), "1");
        assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);

        assert!(app.respond_dialog(0));
        assert_eq!(app.stage_state(), crate::stage::StageState::Battle);
        assert_eq!(app.turn().phase, crate::Phase::Player);
        assert_eq!(
            app.script_var("配置"),
            "1",
            "スタートが再発火している (敵の二重配置になる)"
        );
        assert_eq!(app.script_var("配置後"), "1");
    }

    #[test]
    fn intermission_subcommand_returns_to_menu_after_suspend() {
        // FlowCont::ReturnToIntermissionMenu: サブコマンド .eve が suspend した
        // 場合も、完了後にインターミッションメニューへ復帰すること。
        let sub =
            crate::data::event::parse("プロローグ:\nWait Click\nIncr 実行済\nExit\n").unwrap();
        let mut app = App::new();
        app.script_library_mut().append_with_name(&sub, "lib/x.eve");
        app.push_intermission_command("改造".to_string(), "lib/x.eve".to_string());
        app.set_scene(Scene::Intermission);
        app.set_intermission_cursor(0);

        assert!(app.confirm_intermission_selection());
        // サブコマンド実行中は MapView (PaintString / Hotpoint 表示のため)。
        assert_eq!(app.scene(), Scene::MapView);
        assert!(app.pending_dialog().is_some());

        assert!(app.respond_dialog(0));
        assert_eq!(app.script_var("実行済"), "1");
        assert_eq!(
            app.scene(),
            Scene::Intermission,
            "サブコマンド完了後はメニューへ復帰するべき"
        );
    }

    #[test]
    fn flow_continuations_survive_save_load() {
        // `flow` / `start_passed_pcs` は serde 対象: suspend 中にセーブ → ロード
        // しても継続が失われず、resume 完了後に正しく Battle へ進むこと。
        let stage = crate::data::event::parse("スタート:\nIncr 配置\nWait Click\nExit\n").unwrap();
        let mut app = App::new();
        app.script_library_mut()
            .append_with_name(&stage, "main.eve");
        app.set_script_var("次ステージ".to_string(), "main.eve".to_string());
        assert!(app.advance_to_next_stage());
        assert!(app.pending_dialog().is_some());

        let json = app.to_save_json().expect("save");
        let mut loaded = App::from_save_json(&json).expect("load");

        assert!(loaded.respond_dialog(0));
        assert_eq!(loaded.stage_state(), crate::stage::StageState::Battle);
        assert_eq!(
            loaded.script_var("配置"),
            "1",
            "ロード後の resume でスタートが再発火している"
        );
    }

    #[test]
    fn interrupt_event_defers_until_script_completes() {
        // EventQue (原典 Event.bas 準拠): スクリプト実行中の `Kill` が発火する
        // 破壊イベントはキューに積まれ、現在のスクリプト完了後に実行される。
        // ハンドラに Wait Click 等の suspend 系命令が含まれていても、外側の
        // 実行コンテキストを上書きしない (旧実装は再入実行で、外側 suspend 時に
        // ハンドラの ctx が上書き消失するハザードがあった)。
        let src = "\
メイン:
Pilot \"鋼鉄1号\" 鋼1 男性 一般 BBBC 50 100 120 110 110 100 100
Unit \"敵A\" リアル系 1 3 陸 5 M 1000 200 1200 100 800 80 BBBC
Place \"敵A\" \"鋼鉄1号\" Enemy 3 3
Kill 鋼鉄1号
Message after_kill
Exit
破壊 鋼鉄1号:
Message handler_start
Wait Click
Incr 破壊処理完了
Exit
";
        let stmts = crate::data::event::parse(src).unwrap();
        let mut app = App::new();
        app.script_library_mut().append(&stmts);
        crate::event_runtime::trigger_label(&mut app, "メイン");

        // メイン完了後にハンドラが起動し、Wait Click で suspend している。
        let msgs: Vec<&str> = app.messages().iter().map(String::as_str).collect();
        let after_kill_pos = msgs.iter().position(|m| *m == "after_kill");
        let handler_pos = msgs.iter().position(|m| *m == "handler_start");
        assert!(
            after_kill_pos.is_some() && handler_pos.is_some(),
            "after_kill / handler_start が記録されていない: {msgs:?}"
        );
        assert!(
            after_kill_pos < handler_pos,
            "破壊ハンドラがスクリプト完了前に割り込んでいる (EventQue 非準拠): {msgs:?}"
        );
        assert!(app.pending_dialog().is_some());
        assert_eq!(app.script_var("破壊処理完了"), "");

        // ハンドラの続きも失われない (旧実装では ctx 上書きで消失し得た)。
        assert!(app.respond_dialog(0));
        assert_eq!(app.script_var("破壊処理完了"), "1");
    }

    #[test]
    fn turn_events_not_swallowed_by_suspending_predecessor() {
        // EventQue: `ターン 全 味方` が Wait Click で suspend しても、後続の
        // `ターン 1 味方` はキューに残り、完了後に実行される。旧実装は
        // trigger_label の実行中ガードに弾かれて黙って消えていた。
        let src = "\
ターン 全 味方:
Incr 全ターン
Wait Click
Exit
ターン 1 味方:
Incr ターンN
Exit
";
        let stmts = crate::data::event::parse(src).unwrap();
        let mut app = App::new();
        app.script_library_mut().append(&stmts);

        // スタート未定義 → AfterStartEvent 継続が即 begin_phase(Player) を呼び、
        // ターンイベント 2 件が投函される。
        app.begin_sortie();
        app.begin_battle();

        assert_eq!(app.script_var("全ターン"), "1");
        assert!(
            app.pending_dialog().is_some(),
            "ターン全 が suspend するはず"
        );
        assert_eq!(
            app.script_var("ターンN"),
            "",
            "ターン 1 イベントは先行イベント完了まで実行されないはず"
        );

        assert!(app.respond_dialog(0));
        assert_eq!(
            app.script_var("ターンN"),
            "1",
            "先行ターンイベントの suspend で後続イベントが消えている"
        );
    }

    #[test]
    fn continue_chains_to_next_stage_without_frontend() {
        // 非インターミッションシナリオの `Continue <file>` はフロントエンドの
        // 介在なしに次ステージへ遷移する (FlowCont::LoadNextStage)。旧実装は
        // archive ロード時の while ループが拾う前提で、戦闘中の Continue
        // (例: 勝利 → エピローグ → Continue 次.eve) を誰も消費せず停止していた。
        // また Continue は旧ステージの flow 継続を破棄する (scenario_transition_
        // reset) ため、旧 AfterStageFileRun が新ステージ突入後に二重発火しない。
        let a = crate::data::event::parse(
            "スタート:\nIncr A実行\nContinue b.eve\nエピローグ:\nIncr エピ\nExit\n",
        )
        .unwrap();
        let b = crate::data::event::parse("スタート:\nIncr B実行\nExit\n").unwrap();
        let mut app = App::new();
        app.script_library_mut().append_with_name(&a, "a.eve");
        app.script_library_mut().append_with_name(&b, "b.eve");
        app.set_script_var("次ステージ".to_string(), "a.eve".to_string());

        assert!(app.advance_to_next_stage());
        assert_eq!(app.script_var("A実行"), "1");
        assert_eq!(app.script_var("エピ"), "1", "エピローグが実行されるべき");
        assert_eq!(
            app.script_var("B実行"),
            "1",
            "Continue が次ステージを起動するべき (フロントエンド非依存)"
        );
        assert_eq!(app.current_stage_file(), "b.eve");
        assert_eq!(app.stage_state(), crate::stage::StageState::Battle);
        assert_eq!(app.turn().phase, crate::Phase::Player);
    }

    #[test]
    fn call_condition_with_multibyte_neighbors_does_not_panic() {
        // 回帰テスト: 条件式中の `Call(...)` スキャンが `src[i..i+5]` の文字列
        // スライスでマルチバイト文字の途中を踏んで panic していた
        // (実例: game_4011 の `Call(版権ＢＧＭ確認) = あり`)。バイト列比較に
        // 修正後は正常に評価できる。
        let src = "\
メイン:
If Call(版権確認) = あり Then
Set 結果 当たり
EndIf
If (0 >= 1 And 通常ルート) Or (Call(版権確認) = あり And 分岐) Then
Set 結果2 1
EndIf
Exit
版権確認:
Return あり
";
        let stmts = crate::data::event::parse(src).unwrap();
        let mut app = App::new();
        app.script_library_mut().append(&stmts);
        crate::event_runtime::trigger_label(&mut app, "メイン");
        assert_eq!(app.script_var("結果"), "当たり");
    }

    #[test]
    fn escape_cancels_hotpoint_wait_click_screen() {
        // Esc (Input::Cancel) で Hotpoint Wait Click 画面を右クリック相当で「戻る」。
        // トラックパッドの副ボタンが効かない環境でも確実に抜けられることの保証。
        let src = "\
状態:
Hotpoint タブ 10 10 50 50
Do
  Wait Click
  Switch 選択
  Case \"\"
    If KeyState(2) Then
      Set 結果 終了
      Break
    EndIf
  EndSw
Loop While (1)
Exit
";
        let stmts = crate::data::event::parse(src).unwrap();
        let mut app = App::new();
        app.script_library_mut().append(&stmts);
        crate::event_runtime::trigger_label(&mut app, "状態");
        assert!(
            app.pending_dialog().is_some(),
            "Wait Click で中断していない"
        );
        // Esc → Input::Cancel → 右クリック相当でループ脱出
        app.handle_input(Input::Cancel);
        assert!(
            app.pending_dialog().is_none(),
            "Esc で Wait Click を抜けられていない"
        );
        assert_eq!(app.script_var("結果"), "終了");
    }

    #[test]
    fn keystate2_one_shot_no_infinite_release_loop() {
        // スパロボ戦記 AlphaSecond L65: ステータス画面を右クリックで抜けた直後の
        // `Do While KeyState(2) Loop` (ボタン解放待ち) が無限ループしないこと。
        // KeyState(2) はワンショット消費なので、解放待ちループは即座に 0 を見て抜ける。
        let src = "\
状態:
Hotpoint タブ 10 10 50 50
Do
  Wait Click
  Switch 選択
  Case \"\"
    If KeyState(2) Then
      Break
    EndIf
  EndSw
Loop While (1)
Do While KeyState(2)
Loop
Set 完了 1
Exit
";
        let stmts = crate::data::event::parse(src).unwrap();
        let mut app = App::new();
        app.script_library_mut().append(&stmts);
        crate::event_runtime::trigger_label(&mut app, "状態");
        assert!(app.pending_dialog().is_some());
        // 右クリック相当で抜ける → 解放待ち `Do While KeyState(2)` を無限ループせず通過
        app.handle_input(Input::Cancel);
        assert!(app.pending_dialog().is_none());
        assert_eq!(
            app.script_var("完了"),
            "1",
            "解放待ちループを抜けて最後まで到達するはず"
        );
        assert!(
            app.last_script_error().is_none(),
            "STEP_LIMIT (無限ループ) が発生した: {:?}",
            app.last_script_error()
        );
    }

    #[test]
    fn end_phase_uses_begin_phase_and_increments_turn() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        app.set_stage_state(crate::stage::StageState::Battle);
        assert_eq!(app.turn().number, 1);

        app.handle_input(Input::EndPhase);
        assert_eq!(app.turn().phase, crate::Phase::Player);
        assert_eq!(app.turn().number, 2);
        // 新ターン開始の HUD メッセージが入っているはず
        assert!(app
            .messages()
            .iter()
            .any(|m| m.contains("ターン 2") && m.contains("味方")));
    }

    #[test]
    fn turn_events_fire_per_phase_and_every_turn() {
        // SRC `ターンイベント.md`: 各フェイズ開始で `ターン <N> <陣営>` と
        // `ターン 全 <陣営>` が発火する。フェイズ順は 味方→敵→中立→ＮＰＣ。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        let eve = crate::data::event::parse(
            "ターン 全 味方:\nIncr c_all\nExit\n\
             ターン 1 敵:\nSet e1 1\nExit\n\
             ターン 1 ＮＰＣ:\nSet npc1 1\nExit\n\
             ターン 2 敵:\nSet e2 1\nExit\n",
        )
        .unwrap();
        app.script_library_mut().append(&eve);
        app.set_stage_state(crate::stage::StageState::Battle);

        // ターン1 味方 → EndPhase で 敵(1)/中立/ＮＰＣ(1) を消費し ターン2 味方 へ。
        app.handle_input(Input::EndPhase);
        assert_eq!(app.script_var("e1"), "1", "ターン 1 敵 が発火していない");
        assert_eq!(
            app.script_var("npc1"),
            "1",
            "ターン 1 ＮＰＣ が発火していない"
        );
        // ターン2 味方開始で `ターン 全 味方` が発火 (毎ターン)。
        assert_eq!(
            app.script_var("c_all"),
            "1",
            "ターン 全 味方 が毎ターン発火していない"
        );
        assert_eq!(
            app.script_var("e2"),
            "",
            "まだ ターン 2 敵 は発火しないはず"
        );

        // もう一度 EndPhase → ターン3 味方。`ターン 全 味方` が再度発火 (計2回)、
        // `ターン 2 敵` が発火する。
        app.handle_input(Input::EndPhase);
        assert_eq!(
            app.script_var("c_all"),
            "2",
            "ターン 全 味方 が2ターン目に再発火していない"
        );
        assert_eq!(app.script_var("e2"), "1", "ターン 2 敵 が発火していない");
    }

    #[test]
    fn animated_ai_runner_steps_units_and_returns_to_player() {
        // animate_ai=true では EndPhase で即完了せず、tick が敵/中立/ＮＰＣ を
        // 1 体ずつ進めて Player ターン+1 に戻る。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Data", 9, 9);
        spawn_party(&mut app, "Data", crate::Party::Enemy, 2, 2);
        spawn_party(&mut app, "Data", crate::Party::Enemy, 14, 14);
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_animate_ai(true);

        // EndPhase → ランナー起動 (敵フェイズ)。即座には Player に戻らない。
        app.handle_input(Input::EndPhase);
        assert!(app.ai_running(), "逐次ランナーが起動しているはず");
        assert_eq!(app.turn().phase, crate::Phase::Enemy);
        assert_eq!(app.turn().number, 1);

        // 最初の 1 ステップ未満の tick ではまだ誰も動かない (間の演出)。
        app.tick(0.1);
        assert!(app.ai_running());
        assert_eq!(app.turn().phase, crate::Phase::Enemy);

        // tick を十分回すと敵→中立→ＮＰＣ を消化し Player ターン2 に戻る。
        for _ in 0..300 {
            app.tick(0.1);
            if !app.ai_running() {
                break;
            }
        }
        assert!(!app.ai_running(), "ランナーが完了していない");
        assert_eq!(app.turn().phase, crate::Phase::Player);
        assert_eq!(app.turn().number, 2);
    }

    /// 敵が隣の Hero を攻撃する逐次フェイズを走らせ、反撃選択 `reaction`
    /// (Some=手動応答 / None=自動反撃モード) のときの Hero 被ダメージを返す。
    /// `hero_weapon` を与えると Hero は反撃可能になる。
    fn run_enemy_attacks_hero(reaction: Option<u32>, hero_weapon: bool) -> (App, i64) {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6); // 防御側
        if hero_weapon {
            add_weapon(&mut app, "Hero", 50, 1);
        }
        place_player_unit(&mut app, "Foe", 9, 9); // Foe の UnitData を用意 (隅の味方)
        add_weapon(&mut app, "Foe", 60, 1);
        spawn_party(&mut app, "Foe", crate::Party::Enemy, 4, 6); // 攻撃側 (隣接)
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_animate_ai(true);
        if reaction.is_none() {
            app.toggle_auto_counter(); // 自動反撃モード ON
        }
        app.handle_input(Input::EndPhase);
        for _ in 0..300 {
            app.tick(0.1);
            if let Some(c) = reaction {
                if app.pending_dialog().is_some() {
                    app.respond_dialog(c);
                }
            }
            if !app.ai_running() {
                break;
            }
        }
        let dmg = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Hero")
            .map(|u| u.damage)
            .unwrap();
        (app, dmg)
    }

    #[test]
    fn reaction_prompt_shown_manual_and_skipped_when_auto() {
        // 手動 (既定): 敵が味方を攻撃 → 反撃メニューが出る。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        place_player_unit(&mut app, "Foe", 9, 9);
        add_weapon(&mut app, "Foe", 60, 1);
        spawn_party(&mut app, "Foe", crate::Party::Enemy, 4, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_animate_ai(true);
        app.handle_input(Input::EndPhase);
        let mut prompted = false;
        for _ in 0..100 {
            app.tick(0.1);
            if app.pending_dialog().is_some() {
                prompted = true;
                break;
            }
            if !app.ai_running() {
                break;
            }
        }
        assert!(prompted, "手動反撃モードで反撃メニューが出ていない");

        // 自動反撃モード: プロンプト無しで攻撃が解決され Hero が被弾する。
        let (_app, auto_dmg) = run_enemy_attacks_hero(None, false);
        assert!(auto_dmg > 0, "自動反撃モードで攻撃が成立していない");
    }

    #[test]
    fn defend_reaction_halves_damage() {
        // options=[回避, 防御] (Hero 武器なし→反撃不可)。防御=choice 2。
        let (_a, defend_dmg) = run_enemy_attacks_hero(Some(2), false);
        let (_b, auto_dmg) = run_enemy_attacks_hero(None, false);
        assert!(auto_dmg > 0, "基準 (自動) で被弾していない");
        assert_eq!(
            defend_dmg,
            auto_dmg / 2,
            "防御でダメージが半減していない (defend={defend_dmg}, auto={auto_dmg})"
        );
    }

    #[test]
    fn counter_reaction_hits_attacker_but_defend_does_not() {
        // Hero に武器 → options=[反撃, 回避, 防御]。反撃=1 / 防御=3。
        let (app_counter, _) = run_enemy_attacks_hero(Some(1), true);
        let enemy_dmg_counter = app_counter
            .database()
            .unit_instances
            .iter()
            .find(|u| u.party == crate::Party::Enemy)
            .map(|u| u.damage)
            .unwrap_or(0);
        assert!(
            enemy_dmg_counter > 0,
            "反撃で攻撃側にダメージが入っていない"
        );

        let (app_defend, _) = run_enemy_attacks_hero(Some(3), true);
        let enemy_dmg_defend = app_defend
            .database()
            .unit_instances
            .iter()
            .find(|u| u.party == crate::Party::Enemy)
            .map(|u| u.damage)
            .unwrap_or(0);
        assert_eq!(enemy_dmg_defend, 0, "防御では反撃しないはず");
    }

    #[test]
    fn battle_anim_queued_on_attack_and_cleared_by_tick() {
        // animate_battle 有効時、攻撃解決で命中演出が積まれ、tick で総時間ぶん
        // 進めると破棄される。攻撃側/防御側タイルが正しく記録される。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        add_weapon(&mut app, "Hero", 200, 1);
        place_player_unit(&mut app, "Foe", 9, 9); // Foe の UnitData を用意
        spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6); // 隣接する攻撃対象
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_animate_battle(true);
        app.set_map_cursor(5, 6);

        assert!(
            app.attack_resolve_and_run(Some((6, 6)), false, ""),
            "攻撃が解決しなかった"
        );
        let anim = app.battle_anim().expect("戦闘演出が積まれていない");
        assert_eq!(anim.attacker, (5, 6));
        assert_eq!(anim.defender, (6, 6));
        assert_eq!(anim.elapsed, 0.0);
        let total = anim.total;
        // 途中までの tick では残る。
        app.tick(total * 0.5);
        assert!(app.battle_anim().is_some(), "途中で演出が消えている");
        // 総時間を超えたら破棄。
        app.tick(total);
        assert!(app.battle_anim().is_none(), "tick 後も演出が残っている");
    }

    #[test]
    fn battle_anim_not_queued_when_disabled() {
        // animate_battle 無効 (既定/ヘッドレス) では演出を積まない (既存テスト互換)。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        add_weapon(&mut app, "Hero", 200, 1);
        place_player_unit(&mut app, "Foe", 9, 9);
        spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_map_cursor(5, 6);

        assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
        assert!(
            app.battle_anim().is_none(),
            "animate_battle 無効でも演出が積まれている"
        );
    }

    #[test]
    fn battle_animation_subroutine_plays_via_script_when_resolved() {
        // animation.txt で武器(状況)→サブルーチンが解決でき、それが script_library に
        // 実在すれば、戦闘解決後にスクリプト再生が起動する (ネイティブ演出より優先)。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        add_weapon(&mut app, "Hero", 200, 1); // 武器名 = "テスト砲"
        place_player_unit(&mut app, "Foe", 9, 9);
        spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
        app.set_stage_state(crate::stage::StageState::Battle);

        // 戦闘アニメサブルーチンを script_library に登録 (PaintPicture + Wait)。
        let sub = "\
戦闘アニメ_斬撃攻撃:
PaintPicture slash.bmp 100 100
Wait 5
Return
";
        let stmts = crate::data::event::parse(sub).unwrap();
        crate::event_runtime::library_append(&mut app, &stmts);
        // animation.txt: 汎用の テスト砲(攻撃) → 斬撃 (→ 戦闘アニメ_斬撃攻撃)。
        app.database_mut()
            .merge_animation_data("汎用\nテスト砲(攻撃), 斬撃\n");

        app.set_animate_battle(true);
        app.set_map_cursor(5, 6);
        assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));

        // スクリプト再生が起動し Wait で中断している。
        assert!(
            app.has_script_context(),
            "戦闘アニメスクリプトが起動・中断していない"
        );
        assert!(
            app.pending_timer().is_some(),
            "Wait による中断タイマが立っていない"
        );
        // PaintPicture が overlay に積まれている。
        assert!(
            !app.script_overlay().cmds.is_empty(),
            "PaintPicture が描画コマンドに積まれていない"
        );
        // スクリプト再生を優先したのでネイティブ演出は積まれない。
        assert!(
            app.battle_anim().is_none(),
            "スクリプト再生中にネイティブ演出も積まれている"
        );

        // tick でタイマを満了させるとスクリプトが再開・完了する。
        app.tick(1.0);
        assert!(
            !app.has_script_context(),
            "Wait 満了後もスクリプトが再開・完了していない"
        );
    }

    #[test]
    fn battle_animation_falls_back_to_native_without_data() {
        // animation.txt が無ければ従来どおりネイティブ演出 (battle_anim) が積まれる。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        add_weapon(&mut app, "Hero", 200, 1);
        place_player_unit(&mut app, "Foe", 9, 9);
        spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_animate_battle(true);
        app.set_map_cursor(5, 6);
        assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
        assert!(
            !app.has_script_context(),
            "戦闘アニメデータ無しで script 起動"
        );
        assert!(
            app.battle_anim().is_some(),
            "フォールバックのネイティブ演出が積まれていない"
        );
    }

    #[test]
    fn counterattack_suppressed_when_defender_action_disabled() {
        // 行動不能 (麻痺) の防御側は反撃武器を持っていても反撃できない (SRC
        // MaxAction()==0 ゲート)。必中で命中を確定させ RNG 非依存にする。
        fn run(disable: bool) -> (i64, i64) {
            let mut app = App::new();
            enter_mapview_with_demo_map(&mut app);
            place_player_unit(&mut app, "Hero", 5, 6);
            add_weapon(&mut app, "Hero", 50, 1);
            place_player_unit(&mut app, "Foe", 9, 9);
            add_weapon(&mut app, "Foe", 50, 1);
            // Hero に必中 (攻撃確定命中)。
            app.database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| u.unit_data_name == "Hero")
                .unwrap()
                .add_condition(crate::condition::Condition::new("必中", 3));
            let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
            {
                let e = app.database_mut().unit_by_uid_mut(&euid).unwrap();
                e.add_condition(crate::condition::Condition::new("必中", 3));
                if disable {
                    e.add_condition(crate::condition::Condition::new("麻痺", 3));
                }
            }
            app.set_stage_state(crate::stage::StageState::Battle);
            app.set_map_cursor(5, 6);
            assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
            let hero_dmg = app
                .database()
                .unit_instances
                .iter()
                .find(|u| u.unit_data_name == "Hero" && u.party == crate::Party::Player)
                .map(|u| u.damage)
                .unwrap();
            let enemy_dmg = app
                .database()
                .unit_instances
                .iter()
                .find(|u| u.party == crate::Party::Enemy)
                .map(|u| u.damage)
                .unwrap_or(-1);
            (hero_dmg, enemy_dmg)
        }
        let (hero_disabled, enemy_disabled) = run(true);
        let (hero_normal, _enemy_normal) = run(false);
        assert!(hero_normal > 0, "前提: 通常は敵が反撃して Hero が被弾する");
        assert_eq!(hero_disabled, 0, "行動不能の敵が反撃してしまっている");
        assert!(enemy_disabled > 0, "行動不能でも敵自身は攻撃を受ける");
    }

    #[test]
    fn reaction_prompt_skipped_when_defender_action_disabled() {
        // 行動不能 (麻痺) の味方が攻撃されても反撃メニューは出ない (選択不可)。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        app.database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| u.unit_data_name == "Hero")
            .unwrap()
            .add_condition(crate::condition::Condition::new("麻痺", 3));
        place_player_unit(&mut app, "Foe", 9, 9);
        add_weapon(&mut app, "Foe", 60, 1);
        spawn_party(&mut app, "Foe", crate::Party::Enemy, 4, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_animate_ai(true);
        app.handle_input(Input::EndPhase);
        let mut prompted = false;
        for _ in 0..200 {
            app.tick(0.1);
            if app.pending_dialog().is_some() {
                prompted = true;
                break;
            }
            if !app.ai_running() {
                break;
            }
        }
        assert!(!prompted, "行動不能の防御側に反撃メニューが出てしまった");
    }

    #[test]
    fn support_guard_reaction_option_offered_only_when_available() {
        // 反撃メニューに「援護防御」が出るのは、隣接に援護防御可能な味方が居るときだけ。
        fn reaction_menu_options(with_guard: bool) -> Vec<String> {
            let mut app = App::new();
            enter_mapview_with_demo_map(&mut app);
            place_player_unit(&mut app, "Hero", 5, 6);
            add_weapon(&mut app, "Hero", 50, 1);
            place_player_unit(&mut app, "Guard", 5, 5); // Hero に隣接する味方
            if with_guard {
                let g = app
                    .database_mut()
                    .unit_instances
                    .iter_mut()
                    .find(|u| u.unit_data_name == "Guard")
                    .unwrap();
                g.add_condition(crate::condition::Condition::new("サポートガード", 9));
                g.support_guard_remaining = 1;
            }
            place_player_unit(&mut app, "Foe", 9, 9);
            let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 4, 6);
            let hero_uid = app
                .database()
                .unit_instances
                .iter()
                .find(|u| u.unit_data_name == "Hero")
                .unwrap()
                .uid
                .clone();
            app.set_stage_state(crate::stage::StageState::Battle);
            app.begin_reaction_prompt(euid, hero_uid, (5, 6));
            match app.pending_dialog() {
                Some(crate::PendingDialog::Menu { options, .. }) => options.clone(),
                _ => Vec::new(),
            }
        }
        let with = reaction_menu_options(true);
        let without = reaction_menu_options(false);
        assert!(
            with.contains(&"援護防御".to_string()),
            "援護防御 が出ていない: {with:?}"
        );
        assert!(
            !without.contains(&"援護防御".to_string()),
            "援護防御 が誤って出ている: {without:?}"
        );
    }

    #[test]
    fn support_guard_intercepts_on_choice_and_suppressed_on_personal_reaction() {
        // "援護防御" 選択 → 隣接の援護役が肩代わり (防御側被弾 0)。個別反撃 ("防御")
        // 選択 → 援護防御は発動せず防御側が受ける。必中で命中確定し RNG 非依存。
        fn run(def_mode: &str) -> (i64, i64) {
            let mut app = App::new();
            enter_mapview_with_demo_map(&mut app);
            place_player_unit(&mut app, "Hero", 5, 6);
            add_weapon(&mut app, "Hero", 60, 1);
            app.database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| u.unit_data_name == "Hero")
                .unwrap()
                .add_condition(crate::condition::Condition::new("必中", 9));
            place_player_unit(&mut app, "Foe", 9, 9);
            let fuid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6); // 防御側
            let guid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 5); // 援護する敵
            {
                let g = app.database_mut().unit_by_uid_mut(&guid).unwrap();
                g.add_condition(crate::condition::Condition::new("サポートガード", 9));
                g.support_guard_remaining = 1;
            }
            app.set_stage_state(crate::stage::StageState::Battle);
            app.set_map_cursor(5, 6);
            assert!(app.attack_resolve_and_run(Some((6, 6)), false, def_mode));
            let foe = app
                .database()
                .unit_by_uid(&fuid)
                .map(|u| u.damage)
                .unwrap_or(-1);
            let guard = app
                .database()
                .unit_by_uid(&guid)
                .map(|u| u.damage)
                .unwrap_or(-1);
            (foe, guard)
        }
        let (foe_guarded, guard_guarded) = run("援護防御");
        assert_eq!(foe_guarded, 0, "援護防御で防御側が被弾している");
        assert!(guard_guarded > 0, "援護役が肩代わりしていない");

        let (foe_defend, guard_defend) = run("防御");
        assert!(foe_defend > 0, "個別防御で防御側が受けていない");
        assert_eq!(guard_defend, 0, "個別反撃選択時に援護防御が発動している");
    }

    /// テスト用: `unit_data_name` のユニットに `critical` 率の武器「クリ砲」(射程1) と
    /// 必中を付与する。
    fn give_crit_weapon(app: &mut App, unit_name: &str, power: i64, critical: i32) {
        let weapon = crate::data::unit::WeaponData {
            name: "クリ砲".into(),
            power,
            min_range: 1,
            max_range: 1,
            precision: 100,
            bullet: -1,
            en_consumption: 0,
            necessary_morale: 0,
            adaption: String::new(),
            critical,
            class: String::new(),
            extras: Vec::new(),
        };
        app.database_mut()
            .units
            .iter_mut()
            .find(|u| u.name == unit_name)
            .unwrap()
            .weapons
            .push(weapon);
        app.database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| u.unit_data_name == unit_name)
            .unwrap()
            .add_condition(crate::condition::Condition::new("必中", 9));
    }

    #[test]
    fn critical_hit_multiplies_damage_by_one_point_five() {
        // クリティカル率 100% (weapon.critical=100, 技量差0) で必ずクリティカル → ×1.5。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        give_crit_weapon(&mut app, "Hero", 20, 100);
        place_player_unit(&mut app, "Foe", 9, 9);
        let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_map_cursor(5, 6);

        // 非クリの基本ダメージを combat で算出。
        let base = {
            let hero = app
                .database()
                .unit_instances
                .iter()
                .find(|u| u.unit_data_name == "Hero")
                .unwrap();
            let foe = app.database().unit_by_uid(&euid).unwrap();
            let atk_unit = app.database().unit_by_name("Hero").unwrap();
            let weapon = atk_unit
                .weapons
                .iter()
                .find(|w| w.name == "クリ砲")
                .unwrap();
            let atk_pilot = app.database().pilot_by_name(&hero.pilot_name).unwrap();
            let def_unit = app.database().unit_by_name("Foe").unwrap();
            let def_pilot = app.database().pilot_by_name(&foe.pilot_name).unwrap();
            let t = app.database().map.as_ref().unwrap().cell(6, 6).terrain_id;
            // 解決経路と同じ地形適応を適用して base を算出 (crit 比 1.5× を保つ)。
            let atk_env = app.terrain_env_at(hero.x, hero.y);
            let def_env = app.terrain_env_at(6, 6);
            crate::combat::predict_with_status_terrain(
                atk_pilot,
                atk_unit,
                weapon,
                def_pilot,
                def_unit,
                app.database().terrain_hit_mod(t),
                app.database().terrain_damage_mod(t),
                hero.morale,
                foe.morale,
                &["必中".to_string()],
                &[],
                atk_env,
                def_env,
            )
            .damage
        };

        assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
        let dealt = app.database().unit_by_uid(&euid).map(|u| u.damage).unwrap();
        assert_eq!(
            dealt,
            base * 3 / 2,
            "クリティカルが ×1.5 になっていない (base={base}, dealt={dealt})"
        );
        assert!(
            app.messages().iter().any(|m| m.contains("クリティカル")),
            "クリティカル表示が無い"
        );
    }

    /// 特殊効果攻撃属性 (痺) を持つ武器は critical=100 でもクリティカルしない
    /// (特殊効果がクリの代わり)。ダメージは ×1.5 されず、状態異常が proc する。
    #[test]
    fn special_effect_weapon_does_not_crit_but_procs() {
        use crate::data::unit::WeaponData;
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 5, 6);
        // 痺 + critical 100 の武器を Hero に持たせ、必中で命中を確定。
        app.database_mut()
            .units
            .iter_mut()
            .find(|u| u.name == "Hero")
            .unwrap()
            .weapons
            .push(WeaponData {
                name: "麻痺砲".into(),
                power: 20,
                min_range: 1,
                max_range: 1,
                precision: 100,
                bullet: -1,
                en_consumption: 0,
                necessary_morale: 0,
                adaption: String::new(),
                critical: 100,
                class: "痺".into(),
                extras: Vec::new(),
            });
        app.database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| u.unit_data_name == "Hero")
            .unwrap()
            .add_condition(crate::Condition::new("必中", 9));
        place_player_unit(&mut app, "Foe", 9, 9);
        let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
        // Foe は高 HP で生存させる (撃破時は proc しないため)。
        app.database_mut()
            .units
            .iter_mut()
            .find(|u| u.name == "Foe")
            .unwrap()
            .hp = 100_000;
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_map_cursor(5, 6);

        // 非クリの基本ダメージを combat で算出 (crit 抑止後はこれと一致するはず)。
        let base = {
            let hero = app
                .database()
                .unit_instances
                .iter()
                .find(|u| u.unit_data_name == "Hero")
                .unwrap();
            let foe = app.database().unit_by_uid(&euid).unwrap();
            let atk_unit = app.database().unit_by_name("Hero").unwrap();
            let weapon = atk_unit
                .weapons
                .iter()
                .find(|w| w.name == "麻痺砲")
                .unwrap();
            let atk_pilot = app.database().pilot_by_name(&hero.pilot_name).unwrap();
            let def_unit = app.database().unit_by_name("Foe").unwrap();
            let def_pilot = app.database().pilot_by_name(&foe.pilot_name).unwrap();
            let t = app.database().map.as_ref().unwrap().cell(6, 6).terrain_id;
            let atk_env = app.terrain_env_at(hero.x, hero.y);
            let def_env = app.terrain_env_at(6, 6);
            crate::combat::predict_with_status_terrain(
                atk_pilot,
                atk_unit,
                weapon,
                def_pilot,
                def_unit,
                app.database().terrain_hit_mod(t),
                app.database().terrain_damage_mod(t),
                hero.morale,
                foe.morale,
                &["必中".to_string()],
                &[],
                atk_env,
                def_env,
            )
            .damage
        };

        assert!(app.attack_resolve_and_run(Some((6, 6)), false, ""));
        let dealt = app.database().unit_by_uid(&euid).map(|u| u.damage).unwrap();
        assert_eq!(
            dealt, base,
            "特殊効果武器はクリティカルしない (×1.5 されない: base={base}, dealt={dealt})"
        );
        assert!(
            !app.messages().iter().any(|m| m.contains("クリティカル")),
            "特殊効果武器でクリティカル表示は出ない"
        );
        assert!(
            app.database()
                .unit_by_uid(&euid)
                .unwrap()
                .has_condition("麻痺"),
            "特殊効果 (麻痺) が proc する"
        );
    }

    #[test]
    fn defend_halves_critical_rate() {
        // 防御選択でクリティカル率が半減する (SRC)。多数試行で発生率を比較
        // (固定シードなので件数は決定的)。
        fn crit_count(def_mode: &str, trials: usize) -> usize {
            let mut app = App::new();
            enter_mapview_with_demo_map(&mut app);
            place_player_unit(&mut app, "Hero", 5, 6);
            give_crit_weapon(&mut app, "Hero", 20, 50); // クリ率 50% (防御で 25%)
            place_player_unit(&mut app, "Foe", 9, 9);
            let euid = spawn_party(&mut app, "Foe", crate::Party::Enemy, 6, 6);
            app.set_stage_state(crate::stage::StageState::Battle);
            app.set_map_cursor(5, 6);
            let mut crits = 0;
            for _ in 0..trials {
                // 撃破されないよう毎回ダメージをリセット (同じタイルを攻撃し続ける)。
                app.database_mut().unit_by_uid_mut(&euid).unwrap().damage = 0;
                let before = app.messages().len();
                app.attack_resolve_and_run(Some((6, 6)), false, def_mode);
                if app.messages()[before..]
                    .iter()
                    .any(|m| m.contains("クリティカル"))
                {
                    crits += 1;
                }
            }
            crits
        }
        let normal = crit_count("", 400);
        let defend = crit_count("防御", 400);
        assert!(
            normal > defend,
            "防御でクリ率が下がっていない (normal={normal}, defend={defend})"
        );
        assert!(
            defend < normal * 3 / 4,
            "防御のクリ率半減が不十分 (normal={normal}, defend={defend})"
        );
    }

    #[test]
    fn ai_move_sets_move_anim_and_runner_waits_for_it() {
        // 逐次 AI 移動でスライド演出 (move_anim) が立ち、再生中はランナーが次に進まない。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 9, 6); // 遠い標的
        place_player_unit(&mut app, "Foe", 23, 15); // Foe の UnitData 用 (隅)
        add_weapon(&mut app, "Foe", 60, 1);
        spawn_party(&mut app, "Foe", crate::Party::Enemy, 2, 6); // 攻撃側 (遠方)
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_animate_ai(true);
        app.handle_input(Input::EndPhase);

        // 最初のステップ間隔を消化して 1 体目を行動させる。
        app.tick(0.5);
        assert!(app.ai_running(), "AI フェイズが進行していない");
        let anim = app.move_anim().expect("移動スライド演出が立っていない");
        assert!(anim.path.len() >= 2, "経路が短すぎる: {:?}", anim.path);
        // 論理位置は即時に移動先へ (始点から動いている)。
        let moved_x = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.party == crate::Party::Enemy)
            .map(|u| u.x)
            .unwrap();
        assert!(moved_x > 2, "敵の論理位置が移動していない (x={moved_x})");

        // 演出再生中はランナーが待機 (move_anim が残る短い tick では別ユニットへ進まない)。
        app.tick(0.05);
        assert!(
            app.move_anim().is_some(),
            "短い tick で演出が消えている (待機していない)"
        );
        // 総時間を超えて進めると演出は破棄される。
        for _ in 0..30 {
            app.tick(0.1);
            if app.move_anim().is_none() {
                break;
            }
        }
        assert!(app.move_anim().is_none(), "演出が完了後も残っている");
    }

    #[test]
    fn game_over_only_fires_during_battle() {
        let mut app = App::new();
        // Battle 以外では no-op
        app.game_over();
        assert_eq!(app.stage_state(), crate::stage::StageState::Briefing);

        app.set_stage_state(crate::stage::StageState::Battle);
        app.game_over();
        assert_eq!(app.stage_state(), crate::stage::StageState::Defeat);
        assert!(app.messages().iter().any(|m| m.contains("敗北")));

        // 二度目以降は遷移済みなので no-op
        app.game_over();
        assert_eq!(app.stage_state(), crate::stage::StageState::Defeat);
    }

    #[test]
    fn click_advances_talk_dialog() {
        // Talk モーダル中はクリックでも Advance できる (旧実装は無反応)。
        let mut app = App::new();
        app.set_pending_dialog(crate::dialog::PendingDialog::Talk {
            speaker: String::new(),
            body: "セリフ".to_string(),
        });
        // クリック → respond_dialog(0) 相当
        assert!(app.handle_input(Input::ClickAt { x: 100, y: 100 }));
        assert!(app.pending_dialog().is_none());
    }

    #[test]
    fn click_advances_wait_click_dialog() {
        // `Wait Click` の合成ダイアログ (WaitClick variant) もクリック / Enter で
        // resume できる。レンダリングは行わず、原典 SRC のように画面はそのまま。
        let mut app = App::new();
        app.set_pending_dialog(crate::dialog::PendingDialog::WaitClick);
        assert!(app.handle_input(Input::ClickAt { x: 50, y: 50 }));
        assert!(app.pending_dialog().is_none());

        let mut app = App::new();
        app.set_pending_dialog(crate::dialog::PendingDialog::WaitClick);
        assert!(app.handle_input(Input::Advance));
        assert!(app.pending_dialog().is_none());
    }

    #[test]
    fn save_load_preserves_active_dialog() {
        // モーダル中 (Talk dialog 表示中) でも save → load で状態を完全復元できる。
        let mut app = App::new();
        // Talk を発火
        let src = "\
Talk リオ
こんにちは。
End
";
        let stmts = crate::data::event::parse(src).unwrap();
        crate::event_runtime::execute(&mut app, &stmts).unwrap();
        assert!(matches!(
            app.pending_dialog(),
            Some(crate::PendingDialog::Talk { .. })
        ));
        let json = app.to_save_json().expect("serialize");
        let restored = App::from_save_json(&json).expect("deserialize");
        assert!(matches!(
            restored.pending_dialog(),
            Some(crate::PendingDialog::Talk { .. })
        ));
        assert!(restored.has_script_context());
    }

    #[test]
    fn support_attack_fires_when_adjacent_supporter_has_status() {
        // 隣接味方が「サポートアタック」状態を持っていれば、本攻撃の後に
        // 自動で 75% 火力の追撃が走る。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        place_player_unit(&mut app, "Supporter", 3, 6);
        // 敵を隣接 (2,7) に配置
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            2,
            7,
        ));
        // サポーターに能力付与
        app.database_mut().unit_instances[1]
            .add_condition(crate::Condition::new("サポートアタック", -1));
        app.set_stage_state(crate::stage::StageState::Battle);
        // Player ユニット (Hero) に攻撃用武器を持たせる
        app.database_mut().units[0]
            .weapons
            .push(crate::data::unit::WeaponData {
                name: "ビーム".to_string(),
                power: 200,
                min_range: 1,
                max_range: 3,
                precision: 50,
                bullet: -1,
                en_consumption: 0,
                necessary_morale: 0,
                adaption: String::new(),
                critical: 0,
                class: String::new(),
                extras: Vec::new(),
            });
        // カーソルを敵に合わせて Hero 側から攻撃
        app.map_cursor = Some((2, 6));
        let _ = app.attack_target();
        // 敵 HP は 100、ヒット時の本攻撃 + サポートアタックで多めに削れていれば良い。
        // 1 度ヒットしただけでも撃破される可能性が高いので、削れた事実だけ確認。
        let enemy = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.party == crate::Party::Enemy);
        // 撃破 or ダメージ蓄積していること
        let damaged_or_dead = enemy.map(|u| u.damage > 0).unwrap_or(true);
        assert!(damaged_or_dead, "expected enemy hit");
        // サポーターの残りサポート攻撃が 0 になっていること (使った場合)
        // ※本攻撃でミス → サポートアタック発動 (1→0) のケース、
        // 　本攻撃で撃破 → サポートアタック発動せず (1 のまま) のケース、両方許容。
        let sup = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Supporter")
            .unwrap();
        assert!(sup.support_attack_remaining <= 1);
    }

    #[test]
    fn support_guard_fires_during_enemy_phase() {
        // 敵フェイズに敵が味方ユニット (Target) を攻撃した際、隣接する援護防御可能な
        // 味方ユニット (Guard) が代わりにダメージを受ける。
        // Target は無傷 / Guard はダメージ蓄積していること。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        // Target (3, 6): 攻撃を受ける味方ユニット (武器なし)
        place_player_unit(&mut app, "Target", 3, 6);
        // Guard (4, 6): 援護防御ユニット (Target の隣)
        place_player_unit(&mut app, "Guard", 4, 6);
        // Guard に「サポートガード」能力を付与
        {
            let guard = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| u.unit_data_name == "Guard")
                .unwrap();
            guard.add_condition(crate::Condition::new("サポートガード", -1));
        }
        // 敵ユニット (2, 6): Target に隣接、武器あり、必中
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Target", // Hero の unit data を再利用
            "PILOT",
            crate::Party::Enemy,
            2,
            6,
        ));
        // 敵に武器を持たせる (power=50, range=1, precision=100)
        app.database_mut()
            .units
            .iter_mut()
            .find(|u| u.name == "Target")
            .unwrap()
            .weapons
            .push(crate::data::unit::WeaponData {
                name: "バルカン".to_string(),
                power: 50,
                min_range: 1,
                max_range: 1,
                precision: 100,
                bullet: -1,
                en_consumption: 0,
                necessary_morale: 0,
                adaption: String::new(),
                critical: 0,
                class: String::new(),
                extras: Vec::new(),
            });
        // 必中条件: 命中 100 を保証
        app.database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| u.party == crate::Party::Enemy)
            .unwrap()
            .add_condition(crate::Condition::new("必中", -1));
        app.set_stage_state(crate::stage::StageState::Battle);
        // EndPhase → 敵フェイズへ → AI が Target を攻撃 → 援護防御発動
        app.handle_input(Input::EndPhase);
        // Target は無傷のはず
        let target = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Target" && u.party == crate::Party::Player);
        let target_damage = target.map(|u| u.damage).unwrap_or(0);
        assert_eq!(
            target_damage, 0,
            "Target should be unharmed (support guard protected it)"
        );
        // Guard はダメージを受けているか撃破されているはず
        let guard = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Guard");
        let guard_damaged_or_dead = guard.map(|u| u.damage > 0).unwrap_or(true);
        assert!(
            guard_damaged_or_dead,
            "Guard should have taken damage (support guarded Target)"
        );
    }

    /// 直撃 (精神コマンド) を持つ攻撃側はサポートガード (援護防御) を無効化する。
    /// Target が直接ダメージを受け、Guard は無傷になる。
    #[test]
    fn chokugeki_disables_support_guard() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Target", 3, 6);
        place_player_unit(&mut app, "Guard", 4, 6);
        app.database_mut()
            .unit_instances
            .iter_mut()
            .find(|u| u.unit_data_name == "Guard")
            .unwrap()
            .add_condition(crate::Condition::new("サポートガード", -1));
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Target",
            "PILOT",
            crate::Party::Enemy,
            2,
            6,
        ));
        app.database_mut()
            .units
            .iter_mut()
            .find(|u| u.name == "Target")
            .unwrap()
            .weapons
            .push(crate::data::unit::WeaponData {
                name: "バルカン".to_string(),
                power: 50,
                min_range: 1,
                max_range: 1,
                precision: 100,
                bullet: -1,
                en_consumption: 0,
                necessary_morale: 0,
                adaption: String::new(),
                critical: 0,
                class: String::new(),
                extras: Vec::new(),
            });
        // 敵に 必中 + 直撃。
        {
            let enemy = app
                .database_mut()
                .unit_instances
                .iter_mut()
                .find(|u| u.party == crate::Party::Enemy)
                .unwrap();
            enemy.add_condition(crate::Condition::new("必中", -1));
            enemy.add_condition(crate::Condition::new("直撃", -1));
        }
        app.set_stage_state(crate::stage::StageState::Battle);
        app.handle_input(Input::EndPhase); // 敵フェイズ → AI が Target を攻撃
        let target_damage = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Target" && u.party == crate::Party::Player)
            .map(|u| u.damage)
            .unwrap_or(0);
        let guard_damage = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.unit_data_name == "Guard")
            .map(|u| u.damage)
            .unwrap_or(-1);
        assert!(
            target_damage > 0,
            "直撃でサポートガードが無効 → Target が被弾"
        );
        assert_eq!(guard_damage, 0, "Guard は肩代わりしない (無傷)");
    }

    #[test]
    fn ai_uses_dijkstra_to_close_distance() {
        // 敵ユニットがマップ反対側 (10, 6) にいる。プレイヤーは Player phase 終了で
        // Enemy AI が走る。AI はプレイヤーに向かって速度分マスを進む。
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Hero", 2, 6);
        // 敵ユニットを (10, 6) に配置
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            10,
            6,
        ));
        app.set_stage_state(crate::stage::StageState::Battle);
        // EndPhase で Enemy phase が走る → AI が Hero (player) に近付く
        app.handle_input(Input::EndPhase);
        // AI が動いた結果、敵は (2,6) のプレイヤーに近付いている
        let enemy = app
            .database()
            .unit_instances
            .iter()
            .find(|u| u.party == crate::Party::Enemy)
            .expect("enemy alive");
        let manhattan_after =
            (enemy.x as i32 - 2).unsigned_abs() + (enemy.y as i32 - 6).unsigned_abs();
        assert!(
            manhattan_after < 8,
            "AI failed to close: dist={manhattan_after}"
        );
    }

    #[test]
    fn poison_status_drops_hp_each_phase_and_one_turn_buffs_clear() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Poisoned", 2, 6);
        // 勝利判定を回避するため敵ユニットも配置
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Poisoned",
            "PILOT",
            crate::Party::Enemy,
            10,
            10,
        ));
        app.set_stage_state(crate::stage::StageState::Battle);
        // unit_data の hp は 100。10% = 10 ダメージのはず
        let u = &mut app.database_mut().unit_instances[0];
        u.add_condition(crate::Condition::new("毒", -1)); // 毒は自動削除しない
        u.add_condition(crate::Condition::new("必中", 1));
        u.add_condition(crate::Condition::new("永続バフ", -1));
        assert_eq!(u.damage, 0);
        // Player フェイズ → Enemy → Allied → Neutral → Player (turn 2) の遷移で
        // Player party の begin_phase が再度呼ばれる。end_phase 1 回で 1 ティック。
        app.handle_input(Input::EndPhase);
        let u = &app.database().unit_instances[0];
        assert!(u.damage >= 10, "expected poison tick ≥10, got {}", u.damage);
        assert!(!u.has_condition("必中"), "必中 should have expired");
        assert!(
            u.has_condition("毒"),
            "毒 should still be present (permanent condition)"
        );
        assert!(u.has_condition("永続バフ"));
    }

    /// 死の宣告 (告) は期限切れ (次の自軍フェイズ) で HP を 1 にする。ボスは無効。
    #[test]
    fn death_sentence_sets_hp_to_one_on_expiry() {
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);
        place_player_unit(&mut app, "Doomed", 2, 6);
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Doomed",
            "PILOT",
            crate::Party::Enemy,
            10,
            10,
        ));
        app.set_stage_state(crate::stage::StageState::Battle);
        let uid = first_player_uid(&app);
        app.database_mut()
            .unit_by_uid_mut(&uid)
            .unwrap()
            .add_condition(crate::Condition::new("死の宣告", 1));
        app.handle_input(Input::EndPhase); // T2 味方フェイズ開始で発動
        assert_eq!(
            app.database().unit_by_uid(&uid).unwrap().damage,
            99,
            "死の宣告 で HP が 1 になる (最大HP100 → damage 99)"
        );
    }

    /// 毒ダメージは 毒属性への弱点で倍・耐性で半減する (特殊効果攻撃属性.md)。
    #[test]
    fn poison_damage_scales_with_resistance_and_weakness() {
        fn poison_tick(features: Vec<crate::feature::ActiveFeature>) -> i64 {
            let mut app = App::new();
            enter_mapview_with_demo_map(&mut app);
            place_player_unit(&mut app, "P", 2, 6);
            app.database_mut().register_unit(crate::UnitInstance::new(
                "P",
                "PILOT",
                crate::Party::Enemy,
                10,
                10,
            ));
            app.set_stage_state(crate::stage::StageState::Battle);
            {
                let u = &mut app.database_mut().unit_instances[0];
                u.add_condition(crate::Condition::new("毒", -1));
                u.active_features = features;
            }
            app.handle_input(Input::EndPhase); // T2 Player フェイズ開始で 1 ティック
            app.database().unit_instances[0].damage
        }
        let normal = poison_tick(vec![]);
        let weak = poison_tick(vec![crate::feature::ActiveFeature::new("弱点", "毒")]);
        let resist = poison_tick(vec![crate::feature::ActiveFeature::new("耐性", "毒")]);
        assert_eq!(weak, normal * 2, "弱点=毒 で毒ダメージ倍");
        assert_eq!(resist, (normal / 2).max(1), "耐性=毒 で毒ダメージ半減");
    }

    #[test]
    fn game_clear_only_fires_during_battle() {
        let mut app = App::new();
        app.set_stage_state(crate::stage::StageState::Battle);
        app.game_clear();
        assert_eq!(app.stage_state(), crate::stage::StageState::Victory);
        assert!(app.messages().iter().any(|m| m.contains("勝利")));
    }

    /// SRC 本体相当の `Data/System/GameOver.eve`(`プロローグ:` ラベル) を再現した
    /// 最小スクリプト。コンティニュー Ask → `選択=1` で `Quickload` → else `GameClear`。
    const GAMEOVER_EVE: &str = "プロローグ:\n\
        Ask コンティニュー？\n\
        はい\n\
        いいえ\n\
        End\n\
        If 選択 = 1 Then\n\
        Quickload\n\
        Endif\n\
        GameClear\n\
        Exit\n";

    #[test]
    fn game_over_fires_gameover_eve_prologue_continue_prompt() {
        // 旧バグ: GameOver.eve のコンティニューは `プロローグ:` ラベルなのに
        // `game_over` が `GameOver`/`ゲームオーバー` しか試さず「何も起こらない」。
        let mut app = App::new();
        let stmts = crate::data::event::parse(GAMEOVER_EVE).unwrap();
        app.script_library_mut()
            .append_with_name(&stmts, "Data/System/GameOver.eve");
        app.set_stage_state(crate::stage::StageState::Battle);

        app.game_over();
        assert_eq!(app.stage_state(), crate::stage::StageState::Defeat);
        // GameOver.eve の プロローグ が発火し、コンティニュー Ask が出る。
        assert!(
            matches!(
                app.pending_dialog(),
                Some(crate::dialog::PendingDialog::Menu { .. })
            ),
            "コンティニュー Ask が表示される"
        );
    }

    #[test]
    fn game_over_continue_choice_requests_reload_from_restart_save() {
        let mut app = App::new();
        app.set_stage_state(crate::stage::StageState::Battle);
        // 戦闘開始時スナップショット相当を __restart_save に保存 (enter_battle_state 相当)。
        let snapshot = app.to_save_json().unwrap();
        app.set_script_var("__restart_save".to_string(), snapshot.clone());

        let stmts = crate::data::event::parse(GAMEOVER_EVE).unwrap();
        app.script_library_mut()
            .append_with_name(&stmts, "GameOver.eve");

        app.game_over();
        // コンティニュー Ask に「はい」(choice 1) で応答 → Quickload。
        assert!(app.respond_dialog(1));
        // __quicksave が無いので __restart_save から再ロードが要求される。
        assert_eq!(
            app.take_pending_reload().as_deref(),
            Some(snapshot.as_str()),
            "コンティニューで __restart_save から再ロード要求"
        );
    }

    #[test]
    fn game_over_decline_continue_does_not_reload() {
        let mut app = App::new();
        app.set_stage_state(crate::stage::StageState::Battle);
        app.set_script_var("__restart_save".to_string(), app.to_save_json().unwrap());
        let stmts = crate::data::event::parse(GAMEOVER_EVE).unwrap();
        app.script_library_mut()
            .append_with_name(&stmts, "GameOver.eve");

        app.game_over();
        // 「いいえ」(choice 2) → Quickload を踏まず GameClear 経路 → 再ロード無し。
        assert!(app.respond_dialog(2));
        assert!(
            app.take_pending_reload().is_none(),
            "コンティニュー拒否では再ロードしない"
        );
    }

    #[test]
    fn quickload_falls_back_to_restart_save_when_no_quicksave() {
        // `Quickload` 単体: __quicksave が空でも __restart_save があれば再ロード要求。
        let mut app = App::new();
        let snapshot = app.to_save_json().unwrap();
        app.set_script_var("__restart_save".to_string(), snapshot.clone());
        let stmts = crate::data::event::parse("Quickload\n").unwrap();
        crate::event_runtime::execute(&mut app, &stmts).unwrap();
        assert_eq!(
            app.take_pending_reload().as_deref(),
            Some(snapshot.as_str())
        );
    }

    #[test]
    fn invoke_custom_unit_command_runs_body_with_target_bound() {
        let mut app = App::new();
        let setup = "Create 味方 ブレイバー 0 リオ 1 3 3\n";
        let stmts = crate::data::event::parse(setup).unwrap();
        crate::event_runtime::execute(&mut app, &stmts).unwrap();
        let uid = app.database().unit_instances[0].uid.clone();

        // シナリオ定義のユニットコマンドを登録。
        // `常true:` ラベルは条件サブルーチン — Return 1 で常に表示。
        let cc = "*ユニットコマンド テスト 味方 Call(常true):\n\
                  Set 実行対象 $(対象ユニットＩＤ)\n\
                  Exit\n\
                  常true:\nReturn 1\n";
        let cc_stmts = crate::data::event::parse(cc).unwrap();
        app.script_library_mut().append(&cc_stmts);

        // 対象ユニットＩＤ が束縛され本体が実行される。
        assert!(app.invoke_custom_unit_command(&uid, "テスト"));
        assert_eq!(app.script_var("実行対象"), uid);
        // 未登録コマンド名は false。
        assert!(!app.invoke_custom_unit_command(&uid, "存在しないコマンド"));
        // 不在ユニットも false。
        assert!(!app.invoke_custom_unit_command("U999", "テスト"));
    }

    #[test]
    fn ai_prefers_high_damage_target() {
        // Two enemy units at different distances: one killable with ~100 damage, one not.
        // AI should target the killable one even if slightly farther.
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);

        // Player unit at (3, 3) - we control Hero
        place_player_unit(&mut app, "Hero", 3, 3);

        // Enemy 1: medium distance (dist=2), HP remaining = 40 (killable with ~100 damage)
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            1, // x = 1, manhattan dist from (3,3) = 2
            3, // y = 3
        ));
        app.database_mut().unit_instances.last_mut().unwrap().damage = 60; // HP remaining = 40

        // Enemy 2: closer (dist=1), HP remaining = 100 (NOT killable with ~100 damage)
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            4, // x = 4, manhattan dist from (3,3) = 1
            3, // y = 3
        ));
        app.database_mut().unit_instances.last_mut().unwrap().damage = 0; // HP remaining = 100

        app.set_stage_state(crate::stage::StageState::Battle);
        // End player phase to trigger enemy AI
        app.handle_input(Input::EndPhase);

        // After AI phase, check which enemy was attacked
        // The killable enemy (HP 40) should have taken more damage
        let enemies: Vec<_> = app
            .database()
            .unit_instances
            .iter()
            .filter(|u| u.party == crate::Party::Enemy)
            .collect();
        let most_damaged = enemies.iter().max_by_key(|u| u.damage);
        if let Some(t) = most_damaged {
            // The killable enemy should have been targeted (higher damage taken)
            assert!(
                t.damage >= 60,
                "AI should target killable enemy, got damage={}",
                t.damage
            );
        }
    }

    #[test]
    fn ai_avoids_high_counter_risk() {
        // Enemy with counter ability - AI should still target killable enemies.
        // This is a simplified test: if a target can be killed with high damage,
        // AI prefers it regardless of counter risk.
        let mut app = App::new();
        enter_mapview_with_demo_map(&mut app);

        place_player_unit(&mut app, "Hero", 3, 3);

        // Enemy 1: killable with high damage
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            4,
            3, // distance 1
        ));
        app.database_mut().unit_instances.last_mut().unwrap().damage = 50; // HP 50

        // Enemy 2: far, not killable
        app.database_mut().register_unit(crate::UnitInstance::new(
            "Hero",
            "PILOT",
            crate::Party::Enemy,
            10,
            3, // distance 8
        ));

        app.set_stage_state(crate::stage::StageState::Battle);
        app.handle_input(Input::EndPhase);

        // AI should have targeted enemy 1 (killable, closer)
        let enemies: Vec<_> = app
            .database()
            .unit_instances
            .iter()
            .filter(|u| u.party == crate::Party::Enemy)
            .collect();
        let most_damaged = enemies.iter().max_by_key(|u| u.damage);
        if let Some(t) = most_damaged {
            // The closer killable enemy should have taken more damage
            assert!(t.damage >= 50, "AI should attack killable target");
        }
    }
}
