//! UI abstraction layer.
//!
//! Traits define the interface between the engine core (`src-core`) and
//! the frontend (`src-web`). This allows the core to be truly platform-agnostic.

mod gui;
mod gui_map;
mod gui_screen;
mod gui_status;

pub use gui::IGUI;
pub use gui_map::IGUIMap;
pub use gui_screen::IGUIScreen;
pub use gui_status::IGUIStatus;

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock implementation of IGUI for testing.
    struct MockGUI;

    impl IGUI for MockGUI {
        fn show_message(&self, _message: &str) {}
        fn show_confirm(&self, _message: &str) -> bool {
            false
        }
        fn show_input(&self, _prompt: &str) -> String {
            String::new()
        }
        fn show_menu(&self, _title: &str, _items: &[&str]) -> usize {
            0
        }
        fn play_sound(&self, _name: &str) {}
        fn play_bgm(&self, _name: &str, _loop_: bool) {}
        fn stop_bgm(&self) {}
    }

    /// Mock implementation of IGUIMap for testing.
    struct MockGUIMap;

    impl IGUIMap for MockGUIMap {
        fn render_map(&self, _width: u32, _height: u32, _tiles: &[(u32, u32, u32)]) {}
        fn highlight_cell(&self, _x: u32, _y: u32, _color: &str) {}
        fn clear_highlights(&self) {}
        fn show_unit(&self, _x: u32, _y: u32, _unit_name: &str, _party: &str) {}
        fn hide_unit(&self, _x: u32, _y: u32) {}
        fn show_movement_range(&self, _cells: &[(u32, u32)], _cost: &[(u32, u32, i32)]) {}
        fn clear_movement_range(&self) {}
    }

    /// Mock implementation of IGUIScreen for testing.
    struct MockGUIScreen;

    impl IGUIScreen for MockGUIScreen {
        fn draw_string(&self, _x: i32, _y: i32, _text: &str, _color: &str) {}
        fn draw_line(&self, _x1: i32, _y1: i32, _x2: i32, _y2: i32, _color: &str, _width: u32) {}
        fn draw_rect(&self, _x: i32, _y: i32, _w: i32, _h: i32, _color: &str) {}
        fn draw_image(&self, _x: i32, _y: i32, _name: &str) {}
        fn clear(&self) {}
        fn apply_sepia(&self) {}
        fn apply_monotone(&self) {}
        fn fade_in(&self, _duration_ms: u32) {}
        fn fade_out(&self, _duration_ms: u32) {}
    }

    /// Mock implementation of IGUIStatus for testing.
    struct MockGUIStatus;

    impl IGUIStatus for MockGUIStatus {
        fn show_unit_status(
            &self,
            _unit_name: &str,
            _pilot_name: &str,
            _hp: i64,
            _max_hp: i64,
            _en: i32,
            _max_en: i32,
            _armor: i64,
            _mobility: i32,
        ) {
        }
        fn hide_unit_status(&self) {}
        fn show_pilot_status(
            &self,
            _pilot_name: &str,
            _level: i32,
            _exp: i32,
            _sp: i32,
            _morale: i32,
        ) {
        }
        fn hide_pilot_status(&self) {}
        fn show_weapon_list(&self, _unit_name: &str, _weapons: &[(&str, i64, i32, i32)]) {}
        fn hide_weapon_list(&self) {}
    }

    #[test]
    fn ui_trait_object_creation() {
        // Verify traits can be used as trait objects (dyn IGUI)
        let _gui: Box<dyn IGUI> = Box::new(MockGUI);
        let _gui_map: Box<dyn IGUIMap> = Box::new(MockGUIMap);
        let _gui_screen: Box<dyn IGUIScreen> = Box::new(MockGUIScreen);
        let _gui_status: Box<dyn IGUIStatus> = Box::new(MockGUIStatus);

        // Verify trait objects can hold different implementations
        fn use_gui(_g: &dyn IGUI) {}
        fn use_gui_map(_g: &dyn IGUIMap) {}
        fn use_gui_screen(_g: &dyn IGUIScreen) {}
        fn use_gui_status(_g: &dyn IGUIStatus) {}

        let mock_gui = MockGUI;
        use_gui(&mock_gui);
        use_gui_map(&MockGUIMap);
        use_gui_screen(&MockGUIScreen);
        use_gui_status(&MockGUIStatus);
    }

    #[test]
    fn ui_trait_bounds_satisfied() {
        // Verify Send + Sync bounds are satisfied by mock implementations
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MockGUI>();
        assert_send_sync::<MockGUIMap>();
        assert_send_sync::<MockGUIScreen>();
        assert_send_sync::<MockGUIStatus>();
    }
}
