//! Core GUI operations.

/// Core GUI interface for dialogs, messages, and general UI operations.
pub trait IGUI: Send + Sync {
    /// Show a message dialog with OK button.
    fn show_message(&self, message: &str);

    /// Show a confirmation dialog (Yes/No). Returns true if Yes.
    fn show_confirm(&self, message: &str) -> bool;

    /// Show an input dialog. Returns the entered text.
    fn show_input(&self, prompt: &str) -> String;

    /// Show a selection menu. Returns the selected index (0-based).
    fn show_menu(&self, title: &str, items: &[&str]) -> usize;

    /// Play a sound effect.
    fn play_sound(&self, name: &str);

    /// Play BGM.
    fn play_bgm(&self, name: &str, loop_: bool);

    /// Stop BGM.
    fn stop_bgm(&self);
}
