pub mod controller;
pub mod focus;
pub mod subscriptions;

use iced::{Size, window};
use std::collections::HashMap;

#[derive(Debug, Copy, Clone, PartialEq, Eq, Hash)]
pub enum WindowKind {
    Main,
    Search,
    /// Transparent Iced controls hosted above a native-root video window.
    PlayerOverlay,
}

/// Lifecycle of the dedicated player overlay window.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq)]
pub enum PlayerOverlayWindowState {
    #[default]
    Closed,
    /// Allocated with `visible = false`; native attachment may proceed safely.
    Hidden,
    /// Native attachment is ready and the retained main window is being
    /// hidden; the overlay itself is still invisible.
    Activating,
    /// Native attachment was confirmed and the overlay is visible.
    Active,
    /// Native slots were detached and a close action has been queued.
    Closing,
}

#[derive(Debug, Default)]
pub struct WindowManager {
    by_kind: HashMap<WindowKind, window::Id>,
    by_id: HashMap<window::Id, WindowKind>,
    player_overlay: PlayerOverlayWindowState,
    player_overlay_size: Option<Size>,
    pub focused: Option<window::Id>,
}

impl WindowManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, kind: WindowKind, id: window::Id) {
        if let Some(previous_id) = self.by_kind.remove(&kind) {
            self.by_id.remove(&previous_id);
        }
        if let Some(previous_kind) = self.by_id.remove(&id) {
            self.by_kind.remove(&previous_kind);
            if previous_kind == WindowKind::PlayerOverlay {
                self.player_overlay = PlayerOverlayWindowState::Closed;
                self.player_overlay_size = None;
            }
        }

        self.by_kind.insert(kind, id);
        self.by_id.insert(id, kind);
        if kind == WindowKind::PlayerOverlay {
            self.player_overlay = PlayerOverlayWindowState::Hidden;
            self.player_overlay_size = None;
        }
    }

    pub fn get(&self, kind: WindowKind) -> Option<window::Id> {
        self.by_kind.get(&kind).copied()
    }

    pub fn get_kind(&self, id: window::Id) -> Option<WindowKind> {
        self.by_id.get(&id).copied()
    }

    pub fn remove_by_id(&mut self, id: window::Id) -> Option<WindowKind> {
        if let Some(kind) = self.by_id.remove(&id) {
            let _ = self.by_kind.remove(&kind);
            if kind == WindowKind::PlayerOverlay {
                self.player_overlay = PlayerOverlayWindowState::Closed;
                self.player_overlay_size = None;
            }
            if self.focused == Some(id) {
                self.focused = None;
            }
            Some(kind)
        } else {
            None
        }
    }

    pub const fn player_overlay_state(&self) -> PlayerOverlayWindowState {
        self.player_overlay
    }

    pub const fn player_overlay_size(&self) -> Option<Size> {
        self.player_overlay_size
    }

    /// Record the actual native overlay viewport without overwriting the
    /// retained main-window geometry used when playback exits.
    pub fn set_player_overlay_size(
        &mut self,
        id: window::Id,
        size: Size,
    ) -> bool {
        if self.get(WindowKind::PlayerOverlay) != Some(id)
            || !size.width.is_finite()
            || !size.height.is_finite()
            || size.width <= 0.0
            || size.height <= 0.0
        {
            return false;
        }
        self.player_overlay_size = Some(size);
        true
    }

    /// Confirm native attachment and begin the hidden-to-visible handoff.
    pub fn activate_player_overlay(&mut self) -> bool {
        if self.get(WindowKind::PlayerOverlay).is_some()
            && self.player_overlay == PlayerOverlayWindowState::Hidden
        {
            self.player_overlay = PlayerOverlayWindowState::Activating;
            true
        } else {
            false
        }
    }

    /// Record that the native presenter has synchronously revealed the host.
    pub fn finish_player_overlay_activation(&mut self) -> bool {
        if self.get(WindowKind::PlayerOverlay).is_some()
            && self.player_overlay == PlayerOverlayWindowState::Activating
        {
            self.player_overlay = PlayerOverlayWindowState::Active;
            true
        } else {
            false
        }
    }

    /// Record deterministic pre-close teardown and return the prior lifecycle.
    pub fn begin_player_overlay_close(&mut self) -> PlayerOverlayWindowState {
        let previous = self.player_overlay;
        if self.get(WindowKind::PlayerOverlay).is_some()
            && previous != PlayerOverlayWindowState::Closing
        {
            self.player_overlay = PlayerOverlayWindowState::Closing;
        }
        previous
    }

    pub fn is_search_window(&self, id: window::Id) -> bool {
        matches!(self.get_kind(id), Some(WindowKind::Search))
    }

    pub fn is_main_window(&self, id: window::Id) -> bool {
        matches!(self.get_kind(id), Some(WindowKind::Main))
    }

    pub fn is_player_overlay_window(&self, id: window::Id) -> bool {
        matches!(self.get_kind(id), Some(WindowKind::PlayerOverlay))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kind_and_id_maps_remain_bijective_when_windows_are_replaced() {
        let mut windows = WindowManager::new();
        let first = window::Id::unique();
        let replacement = window::Id::unique();

        windows.set(WindowKind::Search, first);
        windows.set(WindowKind::Search, replacement);

        assert_eq!(windows.get(WindowKind::Search), Some(replacement));
        assert_eq!(windows.get_kind(first), None);
        assert_eq!(windows.get_kind(replacement), Some(WindowKind::Search));

        windows.set(WindowKind::Main, replacement);
        assert_eq!(windows.get(WindowKind::Search), None);
        assert_eq!(windows.get(WindowKind::Main), Some(replacement));
        assert_eq!(windows.get_kind(replacement), Some(WindowKind::Main));
    }

    #[test]
    fn player_overlay_requires_hidden_attach_reveal_active_close_order() {
        let mut windows = WindowManager::new();
        let overlay = window::Id::unique();

        assert_eq!(
            windows.player_overlay_state(),
            PlayerOverlayWindowState::Closed
        );
        assert!(!windows.activate_player_overlay());

        windows.set(WindowKind::PlayerOverlay, overlay);
        assert_eq!(
            windows.player_overlay_state(),
            PlayerOverlayWindowState::Hidden,
        );
        assert!(windows.activate_player_overlay());
        assert!(!windows.activate_player_overlay());
        assert_eq!(
            windows.player_overlay_state(),
            PlayerOverlayWindowState::Activating,
        );
        assert!(windows.finish_player_overlay_activation());
        assert!(!windows.finish_player_overlay_activation());
        assert_eq!(
            windows.begin_player_overlay_close(),
            PlayerOverlayWindowState::Active
        );
        assert_eq!(
            windows.player_overlay_state(),
            PlayerOverlayWindowState::Closing
        );

        assert_eq!(
            windows.remove_by_id(overlay),
            Some(WindowKind::PlayerOverlay)
        );
        assert_eq!(
            windows.player_overlay_state(),
            PlayerOverlayWindowState::Closed
        );
    }

    #[test]
    fn player_overlay_size_is_scoped_validated_and_cleared() {
        let mut windows = WindowManager::new();
        let overlay = window::Id::unique();
        let unrelated = window::Id::unique();

        windows.set(WindowKind::PlayerOverlay, overlay);
        assert_eq!(windows.player_overlay_size(), None);
        assert!(
            !windows.set_player_overlay_size(
                unrelated,
                Size::new(1_920.0, 1_080.0)
            )
        );
        assert!(
            !windows
                .set_player_overlay_size(overlay, Size::new(f32::NAN, 1_080.0))
        );
        assert!(
            !windows.set_player_overlay_size(overlay, Size::new(1_920.0, 0.0))
        );

        let actual = Size::new(1_920.0, 1_080.0);
        assert!(windows.set_player_overlay_size(overlay, actual));
        assert_eq!(windows.player_overlay_size(), Some(actual));

        windows.remove_by_id(overlay);
        assert_eq!(windows.player_overlay_size(), None);
    }

    #[test]
    fn activating_overlay_can_close_before_reveal_completion() {
        let mut windows = WindowManager::new();
        let overlay = window::Id::unique();
        windows.set(WindowKind::PlayerOverlay, overlay);

        assert!(windows.activate_player_overlay());
        assert_eq!(
            windows.begin_player_overlay_close(),
            PlayerOverlayWindowState::Activating
        );
        assert_eq!(
            windows.player_overlay_state(),
            PlayerOverlayWindowState::Closing
        );
        assert!(!windows.finish_player_overlay_activation());
    }
}
