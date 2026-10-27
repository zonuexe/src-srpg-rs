//! ユニット / マップコマンドメニューのレイアウトとクリック当たり判定。描画は src-web
//! `render::draw_command_menu` が同じ定数を使う。

use crate::command_menu::{CommandMenu, MenuActionId};

/// メニュー描画 / クリック判定の共通レイアウト定数。
pub const MENU_X: i32 = 200;
pub const MENU_Y: i32 = 12;
pub const MENU_WIDTH: i32 = 180;
pub const MENU_ITEM_HEIGHT: i32 = 26;
pub const MENU_PADDING: i32 = 6;

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
