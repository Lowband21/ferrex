pub mod controller;
pub mod focus;
pub mod subscriptions;

use ferrex_player_playback::messages::PlaybackRequestId;
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
    /// A source has resolved and the retained main window is being hidden
    /// before the integrated mpv backend may open its native root.
    Launching,
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
    player_overlay_launch_request: Option<PlaybackRequestId>,
    deferred_player_overlay_launch: Option<PlaybackRequestId>,
    deferred_external_playback_launch: Option<PlaybackRequestId>,
    shell_hidden_for_playback: Option<PlaybackRequestId>,
    focused: Option<window::Id>,
}

impl WindowManager {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&mut self, kind: WindowKind, id: window::Id) {
        if let Some(previous_id) = self.by_kind.remove(&kind) {
            self.by_id.remove(&previous_id);
            if previous_id != id && self.focused == Some(previous_id) {
                self.focused = None;
            }
        }
        if let Some(previous_kind) = self.by_id.remove(&id) {
            self.by_kind.remove(&previous_kind);
            if previous_kind == WindowKind::PlayerOverlay {
                self.player_overlay = PlayerOverlayWindowState::Closed;
                self.player_overlay_size = None;
                self.player_overlay_launch_request = None;
            }
        }

        self.by_kind.insert(kind, id);
        self.by_id.insert(id, kind);
        if kind == WindowKind::PlayerOverlay {
            self.player_overlay = PlayerOverlayWindowState::Hidden;
            self.player_overlay_size = None;
            self.player_overlay_launch_request = None;
            // A newly allocated donor is hidden. Focus must be confirmed by a
            // later platform event from its actual hosted root.
            if self.focused == Some(id) {
                self.focused = None;
            }
        }
    }

    pub fn get(&self, kind: WindowKind) -> Option<window::Id> {
        self.by_kind.get(&kind).copied()
    }

    pub fn get_kind(&self, id: window::Id) -> Option<WindowKind> {
        self.by_id.get(&id).copied()
    }

    /// Record platform-confirmed focus for a registered window.
    pub fn record_focus(&mut self, id: window::Id) -> bool {
        if !self.by_id.contains_key(&id) {
            return false;
        }
        let changed = self.focused != Some(id);
        self.focused = Some(id);
        changed
    }

    /// Clear focus only when the matching platform window loses it.
    pub fn record_unfocus(&mut self, id: window::Id) -> bool {
        if self.focused != Some(id) {
            return false;
        }
        self.focused = None;
        true
    }

    pub const fn focused_window(&self) -> Option<window::Id> {
        self.focused
    }

    /// Whether the active integrated controls view's actual host owns focus.
    ///
    /// The vendored winit AppKit path reports root focus under the retained
    /// donor id, so this remains pointer-free and does not infer focus from
    /// route or lifecycle state.
    pub fn is_player_overlay_focused(&self) -> bool {
        self.player_overlay == PlayerOverlayWindowState::Active
            && self.get(WindowKind::PlayerOverlay) == self.focused
    }

    /// Whether the currently rendered Ferrex player surface owns focus.
    ///
    /// Integrated native playback routes focus from mpv's root under the
    /// hosted overlay id. Ordinary in-shell playback continues to use the main
    /// window, so adding the native controller backend does not regress it.
    pub fn is_player_surface_focused(&self) -> bool {
        let surface = if self.player_overlay == PlayerOverlayWindowState::Active
        {
            WindowKind::PlayerOverlay
        } else {
            WindowKind::Main
        };
        self.get(surface) == self.focused
    }

    pub fn remove_by_id(&mut self, id: window::Id) -> Option<WindowKind> {
        if let Some(kind) = self.by_id.remove(&id) {
            let _ = self.by_kind.remove(&kind);
            if kind == WindowKind::PlayerOverlay {
                self.player_overlay = PlayerOverlayWindowState::Closed;
                self.player_overlay_size = None;
                self.player_overlay_launch_request = None;
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

    /// Begin the source-level single-window handoff.
    ///
    /// Active or activating hosts can re-enter this state after their previous
    /// presenter detached for an episode or rendition replacement.
    pub fn begin_player_overlay_launch(
        &mut self,
        request: PlaybackRequestId,
    ) -> bool {
        if self.player_overlay_launch_request == Some(request) {
            return false;
        }
        if self.get(WindowKind::PlayerOverlay).is_some()
            && matches!(
                self.player_overlay,
                PlayerOverlayWindowState::Hidden
                    | PlayerOverlayWindowState::Launching
                    | PlayerOverlayWindowState::Activating
                    | PlayerOverlayWindowState::Active
            )
        {
            self.player_overlay = PlayerOverlayWindowState::Launching;
            self.player_overlay_launch_request = Some(request);
            self.deferred_player_overlay_launch = None;
            self.shell_hidden_for_playback = Some(request);
            // The current shell/root is being withdrawn. The replacement
            // surface must earn focus through a fresh platform event.
            self.focused = None;
            true
        } else {
            false
        }
    }

    pub const fn player_overlay_launch_request(
        &self,
    ) -> Option<PlaybackRequestId> {
        self.player_overlay_launch_request
    }

    pub const fn shell_hidden_for_playback(&self) -> Option<PlaybackRequestId> {
        self.shell_hidden_for_playback
    }

    pub fn defer_player_overlay_launch(&mut self, request: PlaybackRequestId) {
        self.deferred_external_playback_launch = None;
        self.deferred_player_overlay_launch = Some(request);
        // The old backend was synchronously withdrawn before this request was
        // deferred. Transfer the durable restore obligation now so a failure
        // that beats RawWindowClosed cannot strand the retained shell hidden.
        self.shell_hidden_for_playback = Some(request);
    }

    pub fn take_deferred_player_overlay_launch(
        &mut self,
    ) -> Option<PlaybackRequestId> {
        self.deferred_player_overlay_launch.take()
    }

    /// Transfer the durable retained-shell hide obligation to an external
    /// process request before any donor close or process spawn is queued.
    pub fn begin_external_shell_handoff(&mut self, request: PlaybackRequestId) {
        self.deferred_player_overlay_launch = None;
        self.shell_hidden_for_playback = Some(request);
        self.focused = None;
        if self.get(WindowKind::PlayerOverlay).is_some() {
            self.player_overlay_launch_request = Some(request);
        }
    }

    /// Defer process spawn until the integrated donor's raw close completion.
    pub fn defer_external_playback_launch(
        &mut self,
        request: PlaybackRequestId,
    ) {
        self.deferred_player_overlay_launch = None;
        self.deferred_external_playback_launch = Some(request);
        self.shell_hidden_for_playback = Some(request);
    }

    pub fn take_deferred_external_playback_launch(
        &mut self,
    ) -> Option<PlaybackRequestId> {
        self.deferred_external_playback_launch.take()
    }

    /// Release shell-hide ownership only for the matching playback request.
    pub fn finish_shell_handoff(
        &mut self,
        request: Option<PlaybackRequestId>,
    ) -> bool {
        match request {
            Some(request)
                if self.shell_hidden_for_playback == Some(request) =>
            {
                self.shell_hidden_for_playback = None;
                true
            }
            Some(_) => false,
            None => {
                self.shell_hidden_for_playback = None;
                true
            }
        }
    }

    /// Confirm native attachment and begin the hidden-to-visible handoff.
    pub fn activate_player_overlay(
        &mut self,
        request: PlaybackRequestId,
    ) -> bool {
        if self.get(WindowKind::PlayerOverlay).is_some()
            && self.player_overlay_launch_request == Some(request)
            && matches!(
                self.player_overlay,
                PlayerOverlayWindowState::Hidden
                    | PlayerOverlayWindowState::Launching
            )
        {
            self.player_overlay = PlayerOverlayWindowState::Activating;
            true
        } else {
            false
        }
    }

    /// Record that the native presenter has synchronously revealed the host.
    pub fn finish_player_overlay_activation(
        &mut self,
        request: PlaybackRequestId,
    ) -> bool {
        if self.get(WindowKind::PlayerOverlay).is_some()
            && self.player_overlay_launch_request == Some(request)
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
            if self
                .get(WindowKind::PlayerOverlay)
                .is_some_and(|id| self.focused == Some(id))
            {
                self.focused = None;
            }
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
        let request = PlaybackRequestId::new(1);

        assert_eq!(
            windows.player_overlay_state(),
            PlayerOverlayWindowState::Closed
        );
        assert!(!windows.activate_player_overlay(request));

        windows.set(WindowKind::PlayerOverlay, overlay);
        assert_eq!(
            windows.player_overlay_state(),
            PlayerOverlayWindowState::Hidden,
        );
        assert!(windows.begin_player_overlay_launch(request));
        assert!(!windows.begin_player_overlay_launch(request));
        assert_eq!(
            windows.player_overlay_state(),
            PlayerOverlayWindowState::Launching,
        );
        assert!(windows.activate_player_overlay(request));
        assert!(!windows.activate_player_overlay(request));
        assert_eq!(
            windows.player_overlay_state(),
            PlayerOverlayWindowState::Activating,
        );
        assert!(windows.finish_player_overlay_activation(request));
        assert!(!windows.finish_player_overlay_activation(request));
        assert!(!windows.is_player_overlay_focused());
        assert!(windows.record_focus(overlay));
        assert!(windows.is_player_overlay_focused());
        assert_eq!(
            windows.begin_player_overlay_close(),
            PlayerOverlayWindowState::Active
        );
        assert_eq!(
            windows.player_overlay_state(),
            PlayerOverlayWindowState::Closing
        );
        assert!(!windows.is_player_overlay_focused());
        assert_eq!(windows.focused_window(), None);

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
    fn newer_request_supersedes_launching_while_same_request_is_deduplicated() {
        let mut windows = WindowManager::new();
        let overlay = window::Id::unique();
        let first = PlaybackRequestId::new(1);
        let second = PlaybackRequestId::new(2);
        windows.set(WindowKind::PlayerOverlay, overlay);

        assert!(windows.begin_player_overlay_launch(first));
        assert!(!windows.begin_player_overlay_launch(first));
        assert!(windows.begin_player_overlay_launch(second));
        assert_eq!(windows.player_overlay_launch_request(), Some(second));
        assert_eq!(windows.shell_hidden_for_playback(), Some(second));
        assert_eq!(
            windows.player_overlay_state(),
            PlayerOverlayWindowState::Launching
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
        let request = PlaybackRequestId::new(1);
        windows.set(WindowKind::PlayerOverlay, overlay);

        assert!(windows.begin_player_overlay_launch(request));
        assert!(windows.activate_player_overlay(request));
        assert_eq!(
            windows.begin_player_overlay_close(),
            PlayerOverlayWindowState::Activating
        );
        assert_eq!(
            windows.player_overlay_state(),
            PlayerOverlayWindowState::Closing
        );
        assert!(!windows.finish_player_overlay_activation(request));
    }

    #[test]
    fn player_focus_requires_platform_confirmation_and_clears_on_unfocus() {
        let mut windows = WindowManager::new();
        let main = window::Id::unique();
        let overlay = window::Id::unique();
        let unknown = window::Id::unique();
        let request = PlaybackRequestId::new(1);
        windows.set(WindowKind::Main, main);
        windows.set(WindowKind::PlayerOverlay, overlay);

        assert!(!windows.record_focus(unknown));
        assert!(windows.record_focus(main));
        assert!(windows.is_player_surface_focused());
        assert!(windows.record_focus(overlay));
        assert_eq!(windows.focused_window(), Some(overlay));
        assert!(!windows.is_player_surface_focused());
        assert!(!windows.is_player_overlay_focused());

        assert!(windows.begin_player_overlay_launch(request));
        assert_eq!(windows.focused_window(), None);
        assert!(windows.activate_player_overlay(request));
        assert!(windows.finish_player_overlay_activation(request));
        assert!(!windows.is_player_overlay_focused());

        assert!(windows.record_focus(overlay));
        assert!(windows.is_player_overlay_focused());
        assert!(windows.is_player_surface_focused());
        assert!(windows.record_unfocus(overlay));
        assert!(!windows.is_player_overlay_focused());
        assert!(!windows.is_player_surface_focused());
        assert_eq!(windows.focused_window(), None);

        assert!(windows.record_focus(main));
        assert!(!windows.record_unfocus(overlay));
        assert_eq!(windows.focused_window(), Some(main));
        assert!(!windows.is_player_surface_focused());
    }
}
