//! Playback constants shared by controls, shortcuts, and update logic.

/// Seeking shortcut defaults.
pub mod seeking {
    use std::time::Duration;

    pub const SEEK_FORWARD_COURSE: f64 = 30.0;
    pub const SEEK_BACKWARD_COURSE: f64 = -15.0;
    pub const SEEK_FORWARD_FINE: f64 = 15.0;
    pub const SEEK_BACKWARD_FINE: f64 = -10.0;

    /// Minimum interval between seek-preview commands emitted while dragging.
    ///
    /// The mpv adapter adds a second bound by allowing only one absolute seek
    /// request in flight and retaining only the newest queued position.
    pub const SEEK_DRAG_THROTTLE: Duration = Duration::from_millis(100);
}

/// Player controls layout constants.
pub mod player_controls {
    /// Padding around control buttons container (all sides).
    pub const CONTROL_BUTTONS_PADDING: f32 = 40.0;

    /// Height of the control buttons row.
    pub const CONTROL_BUTTONS_HEIGHT: f32 = 36.0;

    /// Seek bar hit zone height (clickable area).
    pub const SEEK_BAR_HIT_ZONE_HEIGHT: f32 = 30.0;

    /// Total height of the control buttons container including padding.
    pub const CONTROL_CONTAINER_TOTAL_HEIGHT: f32 =
        CONTROL_BUTTONS_PADDING * 2.0 + CONTROL_BUTTONS_HEIGHT;

    /// Distance from bottom of screen to the visual center of the seek bar.
    pub const SEEK_BAR_CENTER_FROM_BOTTOM: f32 =
        CONTROL_CONTAINER_TOTAL_HEIGHT + SEEK_BAR_HIT_ZONE_HEIGHT / 2.0;

    /// Distance from bottom of screen to the bottom edge of the seek bar hit zone.
    pub const SEEK_BAR_BOTTOM_EDGE: f32 = CONTROL_CONTAINER_TOTAL_HEIGHT;

    /// Padding around the top bar (title and navigation).
    pub const TOP_BAR_PADDING: f32 = 15.0;
}
