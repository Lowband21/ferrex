//! Strongly-typed pixel offsets used by the background shader.

/// Content-space offset in logical pixels.
///
/// This is intentionally modeled as its own type (instead of reusing `iced::Vector`)
/// because these values are not velocities; they represent an absolute scroll offset
/// that should be fed into the shader deterministically.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ContentOffsetPx {
    pub x: f32,
    pub y: f32,
}

impl ContentOffsetPx {
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}
