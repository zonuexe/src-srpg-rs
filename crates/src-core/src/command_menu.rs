//! 戦闘モードのユニット / マップコマンドメニュー定義。
//!
//! 元 SRC では `Command.bas` で実装される操作系の入口。
//! https://github.com/7474/SRC/blob/master/SRC.Sharp.Help/src/基本操作.md
//!
//! - ユニット上を左クリック → ユニットコマンドメニュー
//! - 空白地形を左クリック → マップコマンドメニュー
//! - 右クリック → キャンセル
//! - 移動 / 攻撃を選んだ場合は ActionMode が遷移し、続く左クリックで
//!   目的地 / 攻撃目標を確定する。

use serde::{Deserialize, Serialize};

/// ユニットコマンド（左クリック → ユニットコマンドメニュー）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitAction {
    /// 移動
    Move,
    /// 攻撃
    Attack,
    /// 武装一覧（情報表示のみ）
    WeaponList,
    /// 待機: 行動済みにして終了
    Wait,
}

impl UnitAction {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Move => "移動",
            Self::Attack => "攻撃",
            Self::WeaponList => "武装一覧",
            Self::Wait => "待機",
        }
    }
}

/// マップコマンド（空白地形を左クリック → マップコマンドメニュー）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MapAction {
    /// ターン終了: 味方フェイズを終了
    EndTurn,
    /// 部隊表: PilotList シーンへ
    UnitList,
    /// 設定変更: Configuration シーンへ
    Settings,
    /// 自動反撃モード切替: 味方が攻撃された際の反撃手段選択を自動化する。
    ToggleAutoCounter,
    /// クイックセーブ
    QuickSave,
    /// クイックロード
    QuickLoad,
}

impl MapAction {
    pub const fn label(self) -> &'static str {
        match self {
            Self::EndTurn => "ターン終了",
            Self::UnitList => "部隊表",
            Self::Settings => "設定変更",
            Self::ToggleAutoCounter => "自動反撃モード",
            Self::QuickSave => "クイックセーブ",
            Self::QuickLoad => "クイックロード",
        }
    }
}

/// ユニットコマンドメニューの 1 項目。組込コマンドとシナリオ定義の
/// `*ユニットコマンド` の両方を表す。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum UnitMenuItem {
    /// 組込コマンド (移動 / 攻撃 / 武装一覧 / 待機)。
    Builtin(UnitAction),
    /// シナリオ定義の `*ユニットコマンド`。`name` は表示名 = コマンド名で、
    /// 選択時に `App::invoke_custom_unit_command` に渡す。
    Custom { name: String },
    /// 精神コマンド（SP コマンド）サブメニューを開く。パイロットが習得済みの
    /// 精神コマンドを持つときのみ表示される。選択で SP コマンド一覧を出す。
    Spirit,
    /// 特殊能力ベースの組込支援コマンド（修理 / 補給）。該当特殊能力を持ち、
    /// 隣接に要支援の味方が居るときのみ表示される。選択で対象選択へ遷移する。
    Support(SupportKind),
    /// 変形（特殊能力 `変形`）。変形先が 1 つなら即変形、複数ならサブメニューで
    /// 形態を選ぶ。移動前のみ表示され、発動しても行動は消費しない（原典準拠）。
    Transform,
    /// チャージ（チャージ攻撃 `Ｃ` 属性武器を持つユニット）。発動で行動終了し、
    /// 次ターンにチャージ攻撃が解禁される（`charged` フラグを立てる）。
    Charge,
    /// アビリティ（`===` 区切り以降のアビリティを持つユニット）。選択でアビリティ
    /// 一覧サブメニューを開く。射程0は即時、射程≥1は対象選択へ遷移する。
    Ability,
    /// 発進（母艦にユニットを格納している場合）。格納ユニットを選んで出撃させる。
    /// 行動は消費しない（原典準拠）。
    Launch,
    /// 合体（`合体` 特殊能力。2 マス以内に合体相手が居るとき）。発動で合体形態へ。
    /// 移動前のみ・発動で行動終了（原典準拠）。
    Combine,
    /// 分離（合体形態＝構成ユニットを内包している場合）。構成ユニットを盤上へ戻す。
    /// 行動は消費しない（原典準拠）。
    Separate,
}

/// 特殊能力（`修理装置` / `補給装置`）に紐づく組込支援コマンドの種別。
/// 原典 SRC の `FixCmdID`(修理) / `SupplyCmdID`(補給) に相当する。隣接する
/// 味方ユニットを対象に取り、発動すると行動を終了する（精神コマンドと異なり
/// 行動消費する）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SupportKind {
    /// 修理（特殊能力 `修理装置`）: 対象の HP を全回復する。
    Repair,
    /// 補給（特殊能力 `補給装置`）: 対象の EN・残弾を全回復し気力を 10 下げる。
    Supply,
}

impl SupportKind {
    /// メニュー表示ラベル。
    pub fn label(self) -> &'static str {
        match self {
            Self::Repair => "修理",
            Self::Supply => "補給",
        }
    }

    /// 発動条件となる特殊能力名（`UnitData.features` / `active_features` のキー）。
    pub fn feature_name(self) -> &'static str {
        match self {
            Self::Repair => "修理装置",
            Self::Supply => "補給装置",
        }
    }
}

impl UnitMenuItem {
    /// メニュー表示ラベル。
    pub fn label(&self) -> &str {
        match self {
            Self::Builtin(a) => a.label(),
            Self::Custom { name } => name,
            Self::Spirit => "精神コマンド",
            Self::Support(k) => k.label(),
            Self::Transform => "変形",
            Self::Charge => "チャージ",
            Self::Ability => "アビリティ",
            Self::Launch => "発進",
            Self::Combine => "合体",
            Self::Separate => "分離",
        }
    }
}

/// 表示中のコマンドメニュー。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CommandMenu {
    /// ユニットコマンドメニュー。`uid` はメニュー対象ユニットの一意 ID。
    /// 位置ではなく uid で束縛することで、移動後も同一ユニットを追従する。
    Unit {
        uid: String,
        items: Vec<UnitMenuItem>,
        cursor: usize,
    },
    /// マップコマンドメニュー。
    Map {
        items: Vec<MapAction>,
        cursor: usize,
    },
}

/// 移動前のユニット状態スナップショット。移動後コマンドのキャンセルで
/// 巻き戻すために保持する。SRC.Sharp `PrevUnitX/PrevUnitY/PrevUnitArea/PrevUnitEN`
/// (`Command.cs`) に相当。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveSnapshot {
    pub x: u32,
    pub y: u32,
    pub en_consumed: i32,
    pub current_area: String,
}

/// ユニット 1 体の行動フェイズ内モード。識別は一意 `uid`。
/// SRC.Sharp `SelectedUnit` + `CommandState` 相当の状態機械。
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ActionMode {
    /// 通常: カーソルが自由に動く
    #[default]
    Browse,
    /// 移動先選択: `uid` のユニットの移動先を選ぶ
    MoveSelect { uid: String },
    /// 移動確定後・行動未選択。`snapshot` はキャンセル時の復帰元。
    PostMoveMenu { uid: String, snapshot: MoveSnapshot },
    /// 攻撃目標選択中。移動後からの遷移なら `snapshot` に移動前状態。
    /// 移動せずその場から攻撃する場合は `None`。
    AttackSelect {
        uid: String,
        snapshot: Option<MoveSnapshot>,
    },
    /// 精神コマンドの対象選択中 (信頼 / 補給 / 再動 / 応援 / 祝福 / 脱力 等)。
    /// `caster` が発動主体、`spirit` がコマンド名、`cost` が消費 SP。
    /// `target_enemy` が真なら敵単体、偽なら味方単体を対象に取る。
    /// 対象確定時に効果適用 + SP 消費。キャンセル時は SP を消費しない。
    SpiritTarget {
        caster: String,
        spirit: String,
        cost: i32,
        target_enemy: bool,
    },
    /// 修理 / 補給（特殊能力ベースの支援コマンド）の対象選択中。
    /// `caster` が発動主体、`kind` が支援種別。隣接する味方ユニットを対象に取り、
    /// 確定すると効果適用 + 行動終了。キャンセル時は何も起きない。
    SupportTarget { caster: String, kind: SupportKind },
    /// アビリティ（射程≥1）の対象選択中。`caster` が発動主体、`ability_idx` は
    /// `UnitData.abilities` のインデックス。射程内の味方ユニットを対象に取り、
    /// 確定すると効果適用 + 消費。キャンセル時は何も起きない。
    AbilityTarget { caster: String, ability_idx: usize },
}

/// メニュー描画 / クリック判定の共通レイアウト定数。
pub const MENU_X: i32 = 200;
pub const MENU_Y: i32 = 12;
pub const MENU_WIDTH: i32 = 180;
pub const MENU_ITEM_HEIGHT: i32 = 26;
pub const MENU_PADDING: i32 = 6;

/// メニュー項目を選んだ際に App 側で処理する一意キー。
/// `command_menu` の variants を 1 つに統合し、`execute_menu_action` に渡す。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MenuActionId {
    Unit(UnitMenuItem),
    Map(MapAction),
}

/// 与えられた canvas-local 座標 `(x, y)` が現在のメニューのどの項目に
/// ヒットしているかを返す。ヒット無しなら `None`。
pub fn hit_test_menu_item(menu: Option<&CommandMenu>, x: i32, y: i32) -> Option<MenuActionId> {
    let menu = menu?;
    let count = match menu {
        CommandMenu::Unit { items, .. } => items.len(),
        CommandMenu::Map { items, .. } => items.len(),
    };
    if count == 0 {
        return None;
    }
    let height = MENU_ITEM_HEIGHT * count as i32 + MENU_PADDING * 2;
    if !(MENU_X..MENU_X + MENU_WIDTH).contains(&x) || !(MENU_Y..MENU_Y + height).contains(&y) {
        return None;
    }
    let py = y - MENU_Y - MENU_PADDING;
    if py < 0 {
        return None;
    }
    let idx = (py / MENU_ITEM_HEIGHT) as usize;
    if idx >= count {
        return None;
    }
    match menu {
        CommandMenu::Unit { items, .. } => Some(MenuActionId::Unit(items.get(idx)?.clone())),
        CommandMenu::Map { items, .. } => Some(MenuActionId::Map(*items.get(idx)?)),
    }
}
