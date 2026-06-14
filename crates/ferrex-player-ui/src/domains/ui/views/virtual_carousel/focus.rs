//! Global carousel focus controller
//!
//! Tracks which carousel should receive keyboard navigation events based on
//! hover state and explicit focus commands.

use super::types::CarouselKey;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FocusSource {
    #[default]
    None,
    Mouse,
    Keyboard,
}

/// Global carousel focus controller
///
/// Manages focus state for all virtual carousels in the application.
/// Focus determines which carousel receives keyboard navigation events.
///
/// Resolution order for keyboard target:
/// 1. `hovered_key` - if mouse is over a carousel, it takes priority
/// 2. `keyboard_active_key` - explicit focus from chevron clicks or programmatic focus
/// 3. Fallback to view-specific defaults (handled by caller)
#[derive(Debug, Default, Clone)]
pub struct CarouselFocus {
    /// The carousel currently hovered by the mouse (takes priority for keyboard events)
    pub hovered_key: Option<CarouselKey>,

    /// The carousel that should receive keyboard events when no carousel is hovered.
    /// Set by chevron button presses or explicit focus commands.
    pub keyboard_active_key: Option<CarouselKey>,

    /// Timestamp of the last mouse movement observed. Used to gate hover-driven
    /// focus switches so entering a carousel without moving the mouse does not
    /// steal focus.
    pub last_mouse_move_at: Option<Instant>,

    /// Which input source last set the active focus target
    pub last_source: FocusSource,
}

impl CarouselFocus {
    /// Create a new carousel focus controller
    pub fn new() -> Self {
        Self::default()
    }

    /// Set the hovered carousel (called on mouse enter)
    pub fn set_hovered(&mut self, key: Option<CarouselKey>) {
        self.hovered_key = key;
    }

    /// Set the keyboard-active carousel (called on chevron press or explicit focus)
    pub fn set_keyboard_active(&mut self, key: Option<CarouselKey>) {
        self.keyboard_active_key = key;
        self.last_source = FocusSource::Keyboard;
    }

    /// Get the currently active carousel key for keyboard navigation
    /// Returns hovered carousel if present, otherwise keyboard-active carousel
    pub fn get_active_key(&self) -> Option<&CarouselKey> {
        self.hovered_key
            .as_ref()
            .or(self.keyboard_active_key.as_ref())
    }

    /// Check if a specific carousel is the active keyboard target
    pub fn is_active(&self, key: &CarouselKey) -> bool {
        self.get_active_key() == Some(key)
    }

    /// Clear hover state (typically called when mouse leaves window)
    pub fn clear_hover(&mut self) {
        self.hovered_key = None;
    }

    /// Clear all focus state
    pub fn clear_all(&mut self) {
        self.hovered_key = None;
        self.keyboard_active_key = None;
        self.last_source = FocusSource::None;
    }

    /// Record a mouse movement timestamp
    pub fn record_mouse_move(&mut self, when: Instant) {
        self.last_mouse_move_at = Some(when);
    }

    /// Return true if a recent mouse movement occurred within the given window (ms)
    pub fn has_recent_mouse_move(&self, now: Instant, window_ms: u64) -> bool {
        match self.last_mouse_move_at {
            Some(t) => {
                now.saturating_duration_since(t).as_millis() as u64 <= window_ms
            }
            None => false,
        }
    }

    /// Activate hover focus explicitly (used when a FocusKey is accepted)
    pub fn activate_hovered(&mut self, key: CarouselKey) {
        self.hovered_key = Some(key);
        self.last_source = FocusSource::Mouse;
    }

    /// Decide if hover should be preferred given current state and time
    pub fn should_prefer_hover(&self, now: Instant, window_ms: u64) -> bool {
        if self.hovered_key.is_none() {
            return false;
        }
        // If the last active source was mouse, keep preferring hover while hovered_key is set
        if self.last_source == FocusSource::Mouse {
            return true;
        }
        // Otherwise require recent mouse movement to switch to hover
        self.has_recent_mouse_move(now, window_ms)
    }
}
