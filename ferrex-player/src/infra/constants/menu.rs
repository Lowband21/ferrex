//! Menu constants for poster backface menu.
//!
//! These constants define the normalized (0.0-1.0) coordinates for menu buttons.
//! They MUST be kept in sync with `ferrex-player/src/infra/shaders/poster_back.wgsl`.
//!
//! Run `cargo test shader_menu_constants_sync` to verify synchronization.

pub const MENU_AUTO_CLOSE_MS: u64 = 1000;
pub const MENU_KEEPALIVE_MS: u64 = 1200;

/// Horizontal padding from poster edges (normalized 0-1).
/// Set to 0.0 for full-width buttons.
pub const BUTTON_X_PADDING: f32 = 0.0;

/// Y position where first button starts (normalized 0-1, from top).
/// Set to 0.0 to start at top edge.
pub const BUTTON_Y_START: f32 = 0.0;

/// Height of each button (normalized 0-1).
/// With 5 buttons and no gaps: 1.0 / 5 = 0.2
pub const BUTTON_HEIGHT: f32 = 0.2;

/// Vertical gap between buttons (normalized 0-1).
/// Set to 0.0 for no gaps.
pub const BUTTON_GAP: f32 = 0.0;

/// Corner radius of buttons (normalized).
/// Set to 0.0 for sharp corners on full-coverage buttons.
pub const BUTTON_RADIUS: f32 = 0.0;

/// Total number of menu buttons.
pub const NUM_BUTTONS: usize = 5;

/// Button index constants.
pub mod button_index {
    pub const PLAY: usize = 0;
    pub const DETAILS: usize = 1;
    pub const WATCHED: usize = 2;
    pub const WATCHLIST: usize = 3;
    pub const EDIT: usize = 4;
}

/// Calculate the Y start position for a button at the given index.
#[inline]
pub fn button_y_start(index: usize) -> f32 {
    BUTTON_Y_START + (index as f32) * (BUTTON_HEIGHT + BUTTON_GAP)
}

/// Calculate the Y end position for a button at the given index.
#[inline]
pub fn button_y_end(index: usize) -> f32 {
    button_y_start(index) + BUTTON_HEIGHT
}

/// Check if an x coordinate is within button x bounds.
#[inline]
pub fn in_x_bounds(x: f32) -> bool {
    (BUTTON_X_PADDING..=(1.0 - BUTTON_X_PADDING)).contains(&x)
}

/// Get button index from normalized y position, or None if not on a button.
pub fn button_from_y(y: f32) -> Option<usize> {
    if !(0.0..=1.0).contains(&y) {
        return None;
    }
    for i in 0..NUM_BUTTONS {
        if y >= button_y_start(i) && y < button_y_end(i) {
            return Some(i);
        }
    }
    // Handle edge case: y == 1.0 exactly belongs to last button
    if (y - 1.0).abs() < f32::EPSILON {
        return Some(NUM_BUTTONS - 1);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Validates that shader constants in poster_back.wgsl match Rust constants.
    #[test]
    fn shader_menu_constants_sync() {
        let shader_src = include_str!("../shaders/poster_back.wgsl");

        fn extract_f32_const(src: &str, name: &str) -> Option<f32> {
            let pattern = format!("const {}: f32 = ", name);
            src.lines()
                .find(|line| line.contains(&pattern))
                .and_then(|line| {
                    let start = line.find(&pattern)? + pattern.len();
                    let end = line[start..].find(';')?;
                    line[start..start + end].trim().parse().ok()
                })
        }

        fn extract_i32_const(src: &str, name: &str) -> Option<i32> {
            let pattern = format!("const {}: i32 = ", name);
            src.lines()
                .find(|line| line.contains(&pattern))
                .and_then(|line| {
                    let start = line.find(&pattern)? + pattern.len();
                    let end = line[start..].find(';')?;
                    line[start..start + end].trim().parse().ok()
                })
        }

        assert_eq!(
            extract_f32_const(shader_src, "BUTTON_X_PADDING"),
            Some(BUTTON_X_PADDING),
            "BUTTON_X_PADDING mismatch between Rust and WGSL"
        );

        assert_eq!(
            extract_f32_const(shader_src, "BUTTON_Y_START"),
            Some(BUTTON_Y_START),
            "BUTTON_Y_START mismatch between Rust and WGSL"
        );

        assert_eq!(
            extract_f32_const(shader_src, "BUTTON_HEIGHT"),
            Some(BUTTON_HEIGHT),
            "BUTTON_HEIGHT mismatch between Rust and WGSL"
        );

        assert_eq!(
            extract_f32_const(shader_src, "BUTTON_GAP"),
            Some(BUTTON_GAP),
            "BUTTON_GAP mismatch between Rust and WGSL"
        );

        assert_eq!(
            extract_f32_const(shader_src, "BUTTON_RADIUS"),
            Some(BUTTON_RADIUS),
            "BUTTON_RADIUS mismatch between Rust and WGSL"
        );

        assert_eq!(
            extract_i32_const(shader_src, "NUM_BUTTONS"),
            Some(NUM_BUTTONS as i32),
            "NUM_BUTTONS mismatch between Rust and WGSL"
        );
    }

    #[test]
    fn test_button_positions() {
        // With HEIGHT=0.2, GAP=0, START=0:
        // Button 0: 0.0 - 0.2
        // Button 1: 0.2 - 0.4
        // Button 2: 0.4 - 0.6
        // Button 3: 0.6 - 0.8
        // Button 4: 0.8 - 1.0
        assert_eq!(button_from_y(0.1), Some(0));
        assert_eq!(button_from_y(0.3), Some(1));
        assert_eq!(button_from_y(0.5), Some(2));
        assert_eq!(button_from_y(0.7), Some(3));
        assert_eq!(button_from_y(0.9), Some(4));
        assert_eq!(button_from_y(1.0), Some(4)); // Edge case
        assert_eq!(button_from_y(-0.1), None);
        assert_eq!(button_from_y(1.1), None);
    }

    #[test]
    fn test_x_bounds() {
        // With PADDING=0, full width is valid
        assert!(in_x_bounds(0.0));
        assert!(in_x_bounds(0.5));
        assert!(in_x_bounds(1.0));
    }
}
