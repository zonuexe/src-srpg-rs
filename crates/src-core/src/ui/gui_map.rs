//! Map-related UI operations.

/// Map UI interface for rendering and interacting with the game map.
pub trait IGUIMap: Send + Sync {
    /// Render the map to the canvas.
    fn render_map(&self, width: u32, height: u32, tiles: &[(u32, u32, u32)]);

    /// Highlight a cell on the map.
    fn highlight_cell(&self, x: u32, y: u32, color: &str);

    /// Clear all highlights.
    fn clear_highlights(&self);

    /// Show unit at position.
    fn show_unit(&self, x: u32, y: u32, unit_name: &str, party: &str);

    /// Hide unit at position.
    fn hide_unit(&self, x: u32, y: u32);

    /// Show movement range overlay.
    fn show_movement_range(&self, cells: &[(u32, u32)], cost: &[(u32, u32, i32)]);

    /// Clear movement range overlay.
    fn clear_movement_range(&self);
}
