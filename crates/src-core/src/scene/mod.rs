//! シーン（画面状態）モジュール / Scene (screen state) module.
//!
//! VB6 原典は Form を切り替えてシーン遷移していた（`frmTitle` → `frmMain`）。
//! 移植先では Form を持たず、シーンを enum で表現し、フロントエンドはその enum
//! を見て描画を切り替える。
//!
//! Original VB6 switches `Form`s for scene transitions
//! (`frmTitle` → `frmMain`). Here we model scenes as an enum so the
//! frontend can render based on the current variant.

pub mod configuration;
pub mod intermission;
pub mod title;

pub mod map_view;
pub mod pilot_list;
pub mod unit_detail;
pub mod unit_list;

/// 現在表示中のシーン / Currently rendered scene.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum Scene {
    /// 元: `frmTitle`
    #[default]
    Title,
    /// 元: `frmConfiguration` — 設定変更ダイアログ
    Configuration,
    /// SRC.Sharp `Intermission` 相当: 戦闘外メニュー画面。
    /// シナリオが `IntermissionCommand` で登録した項目 + 「次のステージへ」を
    /// リスト表示し、Up/Down/Enter またはクリックで選択する。
    Intermission,
    /// 元 `frmMain` の主領域: マップタイル描画。
    MapView,
    /// 移植版独自: GameDatabase の pilot 一覧（元 SRC には対応シーンなし、
    /// 移植中のデータ確認用）。
    PilotList,
    /// 移植版独自: GameDatabase の unit 一覧（同上）。
    UnitList,
    /// インターミッション「ステータス」から開く単機ステータス詳細画面。
    /// 味方ロスター 1 機ぶんの実効ステータス (機体 + 搭乗パイロット + 武器) を
    /// 表示し、`◀ / ▶` で巡回する。`App::status_detail_index` が表示中の機体を指す。
    UnitDetail,
}

/// 描画が読む `App` 状態の種類。
///
/// `Scene::render_reads()` で「各シーンがどのフィールド種を読むか」を
/// const 宣言する。render.rs 側 `match scene` の各 arm でこの宣言と
/// 実装が齟齬を起こさないか視覚 review し、漏れがあれば追加する。
///
/// 例: `script_overlay` が MapView 以外で誤って描画される不具合
/// (commit 414285e で修正) のような「シーン外フィールド誤参照」は、
/// この宣言と render の実装を突き合わせれば気付きやすい。
///
/// `Overlay` 系は scene に依らず常時描画する点に注意。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneRead {
    /// 画像 / 音声アセット (`Assets`)。
    Assets,
    /// `Settings`。
    Settings,
    /// `GameDatabase` (units / pilots / items / map …)。
    Database,
    /// `App::map_cursor`。
    Cursor,
    /// `App::turn`。
    Turn,
    /// `App::map_scroll`。
    MapScroll,
    /// `App::stage`。
    Stage,
    /// `App::messages` / `last_message`。
    Messages,
    /// `App::selected_weapon_idx`。
    SelectedWeapon,
    /// `App::action_mode`。
    ActionMode,
    /// `App::script_overlay`。MapView 以外で読むと描画漏出の元になる。
    ScriptOverlay,
    /// `App::command_menu`。
    CommandMenu,
    /// `App::stage_state`。
    StageState,
    /// `App::briefing`。
    Briefing,
    /// `App::intermission_commands` / `App::intermission_cursor`。
    Intermission,
}

/// シーン非依存のオーバーレイ。`Scene::overlays()` で宣言する。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Overlay {
    /// `Hotpoint` クリック領域の可視化 (`App::hotpoints`)。
    Hotpoints,
    /// `Talk` / `Confirm` / `Menu` / `Input` の対話 UI
    /// (`App::pending_dialog`)。
    PendingDialog,
}

impl Scene {
    /// このシーン本体の論理サイズ（pixel）。
    /// Logical size of this scene in pixels (without surrounding letterbox).
    pub const fn size(self) -> (u32, u32) {
        match self {
            Scene::Title => (title::TITLE_WIDTH, title::TITLE_HEIGHT),
            Scene::Configuration => (configuration::CONFIG_WIDTH, configuration::CONFIG_HEIGHT),
            Scene::Intermission => (
                intermission::INTERMISSION_WIDTH,
                intermission::INTERMISSION_HEIGHT,
            ),
            Scene::PilotList => (pilot_list::PILOT_LIST_WIDTH, pilot_list::PILOT_LIST_HEIGHT),
            Scene::UnitList => (unit_list::UNIT_LIST_WIDTH, unit_list::UNIT_LIST_HEIGHT),
            Scene::UnitDetail => (
                unit_detail::UNIT_DETAIL_WIDTH,
                unit_detail::UNIT_DETAIL_HEIGHT,
            ),
            Scene::MapView => (map_view::MAP_VIEW_WIDTH, map_view::MAP_VIEW_HEIGHT),
        }
    }

    /// このシーンの描画が読むべき `App` フィールド種別。
    ///
    /// `render.rs` の `match scene` で実際にアクセスしているフィールドと
    /// 突合し、宣言と実装が一致するよう保つ。新フィールドを App に追加
    /// したときは、どの Scene で読むのか明確にしてここに反映する。
    pub const fn render_reads(self) -> &'static [SceneRead] {
        match self {
            Scene::Title => &[SceneRead::Assets],
            Scene::Configuration => &[SceneRead::Settings],
            Scene::Intermission => &[SceneRead::Intermission],
            Scene::MapView => &[
                SceneRead::Assets,
                SceneRead::Database,
                SceneRead::Cursor,
                SceneRead::Turn,
                SceneRead::MapScroll,
                SceneRead::Stage,
                SceneRead::Messages,
                SceneRead::SelectedWeapon,
                SceneRead::ActionMode,
                SceneRead::ScriptOverlay,
                SceneRead::CommandMenu,
                SceneRead::StageState,
                SceneRead::Briefing,
            ],
            Scene::PilotList => &[SceneRead::Assets, SceneRead::Database],
            Scene::UnitList => &[SceneRead::Assets, SceneRead::Database],
            Scene::UnitDetail => &[SceneRead::Assets, SceneRead::Database],
        }
    }

    /// シーンに依らず常時描画する overlay の宣言。
    /// 現状: Hotpoint 可視化 + 対話 UI モーダル。
    pub const fn overlays() -> &'static [Overlay] {
        &[Overlay::Hotpoints, Overlay::PendingDialog]
    }

    /// `MapView` から外れる scene 遷移で「クリアすべき」 transient state の宣言。
    /// 現状: `script_overlay` は MapView 専有なので、Title/Configuration へ
    /// 遷移する際は呼び出し側で明示的にクリアする (commit 414285e 参照)。
    pub const fn cleared_on_exit(self) -> &'static [SceneRead] {
        match self {
            Scene::MapView => &[SceneRead::ScriptOverlay, SceneRead::CommandMenu],
            _ => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_scene_has_reads_declared() {
        // 新シーン追加時に「読むフィールドの宣言を忘れた」を検出。
        // 「読まない」明示なら空 slice を返すが、現状は全 Scene に何か割当てている。
        for scene in [
            Scene::Title,
            Scene::Configuration,
            Scene::Intermission,
            Scene::MapView,
            Scene::PilotList,
            Scene::UnitList,
            Scene::UnitDetail,
        ] {
            let reads = scene.render_reads();
            assert!(!reads.is_empty(), "{scene:?} の render_reads が空");
        }
    }

    #[test]
    fn script_overlay_only_in_mapview() {
        // ScriptOverlay は MapView 専有。他 scene の reads に紛れ込ませない
        // (Title 残留バグの再発防止チェック)。
        for scene in [
            Scene::Title,
            Scene::Configuration,
            Scene::Intermission,
            Scene::PilotList,
            Scene::UnitList,
            Scene::UnitDetail,
        ] {
            assert!(
                !scene.render_reads().contains(&SceneRead::ScriptOverlay),
                "{scene:?} は ScriptOverlay を読むべきではない"
            );
        }
        assert!(Scene::MapView
            .render_reads()
            .contains(&SceneRead::ScriptOverlay));
    }

    #[test]
    fn map_view_cleared_includes_script_overlay() {
        // MapView 退場時に script_overlay をクリアする宣言が落ちていないか。
        assert!(Scene::MapView
            .cleared_on_exit()
            .contains(&SceneRead::ScriptOverlay));
    }
}
