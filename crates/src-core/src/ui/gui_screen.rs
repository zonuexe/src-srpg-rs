//! Screen/drawing operations.

/// Screen UI interface for drawing commands.
pub trait IGUIScreen: Send + Sync {
    /// Draw a string at position.
    fn draw_string(&self, x: i32, y: i32, text: &str, color: &str);

    /// Draw a line from (x1,y1) to (x2,y2).
    fn draw_line(&self, x1: i32, y1: i32, x2: i32, y2: i32, color: &str, width: u32);

    /// Draw a filled rectangle.
    fn draw_rect(&self, x: i32, y: i32, w: i32, h: i32, color: &str);

    /// Draw an image at position.
    fn draw_image(&self, x: i32, y: i32, name: &str);

    /// Clear the screen.
    fn clear(&self);

    /// Apply a sepia filter.
    fn apply_sepia(&self);

    /// Apply a monochrome filter.
    fn apply_monotone(&self);

    /// Fade in.
    fn fade_in(&self, duration_ms: u32);

    /// Fade out.
    fn fade_out(&self, duration_ms: u32);
}
