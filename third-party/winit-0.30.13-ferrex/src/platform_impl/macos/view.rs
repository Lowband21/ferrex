#![allow(clippy::unnecessary_cast)]
use std::cell::{Cell, RefCell};
use std::collections::{HashMap, VecDeque};
use std::ptr;
use std::sync::{Arc, Mutex};

use objc2::rc::{Retained, WeakId};
use objc2::runtime::{AnyObject, Sel};
use objc2::{
    declare_class, msg_send_id, mutability, sel, ClassType, DeclaredClass,
};
use objc2_app_kit::{
    NSApplication, NSCursor, NSEvent, NSEventPhase, NSResponder,
    NSTextInputClient, NSTrackingRectTag, NSView,
    NSViewFrameDidChangeNotification, NSWindow,
    NSWindowDidBecomeKeyNotification,
    NSWindowDidChangeBackingPropertiesNotification,
    NSWindowDidChangeOcclusionStateNotification,
    NSWindowDidChangeScreenNotification, NSWindowDidDeminiaturizeNotification,
    NSWindowDidEnterFullScreenNotification,
    NSWindowDidExitFullScreenNotification, NSWindowDidMiniaturizeNotification,
    NSWindowDidMoveNotification, NSWindowDidResignKeyNotification,
    NSWindowDidResizeNotification, NSWindowOcclusionState,
    NSWindowWillCloseNotification,
};
use objc2_foundation::{
    MainThreadMarker, NSArray, NSAttributedString, NSAttributedStringKey,
    NSCopying, NSMutableAttributedString, NSNotFound, NSNotification,
    NSNotificationCenter, NSObject, NSObjectProtocol, NSPoint, NSRange, NSRect,
    NSSize, NSString, NSUInteger,
};

use super::app_state::ApplicationDelegate;
use super::cursor::{default_cursor, invisible_cursor};
use super::event::{
    code_to_key, code_to_location, create_key_event, event_mods, lalt_pressed,
    ralt_pressed, scancode_to_physicalkey, KeyEventExtra,
};
use super::monitor::flip_window_screen_coordinates;
use super::observer::RunLoop;
use super::window::{WindowId, WinitWindow};
use super::DEVICE_ID;
use crate::dpi::{
    LogicalPosition, LogicalSize, PhysicalPosition, PhysicalSize,
};
use crate::event::{
    DeviceEvent, ElementState, Ime, InnerSizeWriter, KeyEvent, Modifiers,
    MouseButton, MouseScrollDelta, TouchPhase, WindowEvent,
};
use crate::keyboard::{Key, KeyCode, KeyLocation, ModifiersState, NamedKey};
use crate::platform::macos::OptionAsAlt;

#[derive(Debug)]
struct CursorState {
    visible: bool,
    cursor: Retained<NSCursor>,
}

impl Default for CursorState {
    fn default() -> Self {
        Self {
            visible: true,
            cursor: default_cursor(),
        }
    }
}

#[derive(Debug, Eq, PartialEq, Clone, Copy, Default)]
enum ImeState {
    #[default]
    /// The IME events are disabled, so only `ReceivedCharacter` is being sent to the user.
    Disabled,

    /// The ground state of enabled IME input. It means that both Preedit and regular keyboard
    /// input could be start from it.
    Ground,

    /// The IME is in preedit.
    Preedit,

    /// The text was just committed, so the next input from the keyboard must be ignored.
    Committed,
}

bitflags::bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct ModLocationMask: u8 {
        const LEFT     = 0b0001;
        const RIGHT    = 0b0010;
    }
}
impl ModLocationMask {
    fn from_location(loc: KeyLocation) -> ModLocationMask {
        match loc {
            KeyLocation::Left => ModLocationMask::LEFT,
            KeyLocation::Right => ModLocationMask::RIGHT,
            _ => unreachable!(),
        }
    }
}

fn key_to_modifier(key: &Key) -> Option<ModifiersState> {
    match key {
        Key::Named(NamedKey::Alt) => Some(ModifiersState::ALT),
        Key::Named(NamedKey::Control) => Some(ModifiersState::CONTROL),
        Key::Named(NamedKey::Super) => Some(ModifiersState::SUPER),
        Key::Named(NamedKey::Shift) => Some(ModifiersState::SHIFT),
        _ => None,
    }
}

fn get_right_modifier_code(key: &Key) -> KeyCode {
    match key {
        Key::Named(NamedKey::Alt) => KeyCode::AltRight,
        Key::Named(NamedKey::Control) => KeyCode::ControlRight,
        Key::Named(NamedKey::Shift) => KeyCode::ShiftRight,
        Key::Named(NamedKey::Super) => KeyCode::SuperRight,
        _ => unreachable!(),
    }
}

fn get_left_modifier_code(key: &Key) -> KeyCode {
    match key {
        Key::Named(NamedKey::Alt) => KeyCode::AltLeft,
        Key::Named(NamedKey::Control) => KeyCode::ControlLeft,
        Key::Named(NamedKey::Shift) => KeyCode::ShiftLeft,
        Key::Named(NamedKey::Super) => KeyCode::SuperLeft,
        _ => unreachable!(),
    }
}

#[derive(Clone, Copy, Debug)]
struct ForeignHostMetrics {
    position: PhysicalPosition<i32>,
    scale_factor: f64,
    inner_size: PhysicalSize<u32>,
}

#[derive(Debug)]
pub struct ViewState {
    /// Strong reference to the global application state.
    app_delegate: Retained<ApplicationDelegate>,

    cursor_state: RefCell<CursorState>,
    ime_position: Cell<NSPoint>,
    ime_size: Cell<NSSize>,
    modifiers: Cell<Modifiers>,
    phys_modifiers: RefCell<HashMap<Key, ModLocationMask>>,
    tracking_rect: Cell<Option<NSTrackingRectTag>>,
    ime_state: Cell<ImeState>,
    input_source: RefCell<String>,

    /// True iff the application wants IME events.
    ///
    /// Can be set using `set_ime_allowed`
    ime_allowed: Cell<bool>,

    /// True if the current key event should be forwarded
    /// to the application, even during IME
    forward_key_to_app: Cell<bool>,

    marked_text: RefCell<Retained<NSMutableAttributedString>>,
    accepts_first_mouse: bool,

    /// Stable winit identity. AppKit may temporarily install this view in a
    /// foreign window, but events must keep their original logical WindowId.
    event_window_id: WindowId,
    /// Weak reference because the donor window normally keeps a strong
    /// reference to the view. The platform window state retains the view
    /// independently for foreign hosting.
    donor_window: WeakId<WinitWindow>,
    foreign_hosted: Cell<bool>,
    foreign_host_closing: Cell<bool>,
    previous_scale_factor: Cell<f64>,
    previous_host_position: Cell<Option<PhysicalPosition<i32>>>,
    pending_host_metrics: Cell<Option<ForeignHostMetrics>>,
    focused: Cell<Option<bool>>,
    occluded: Cell<Option<bool>>,
    cursor_hittest: Cell<bool>,
    host_metrics_scheduled: Cell<bool>,
    host_metrics_force_scale: Cell<bool>,

    /// The state of the `Option` as `Alt`.
    option_as_alt: Cell<OptionAsAlt>,
}

declare_class!(
    #[derive(Debug)]
    pub(super) struct WinitView;

    unsafe impl ClassType for WinitView {
        #[inherits(NSResponder, NSObject)]
        type Super = NSView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "WinitView";
    }

    impl DeclaredClass for WinitView {
        type Ivars = ViewState;
    }

    unsafe impl WinitView {
        #[method(isFlipped)]
        fn is_flipped(&self) -> bool {
            // `winit` uses the upper-left corner as the origin.
            true
        }

        #[method(viewDidMoveToWindow)]
        fn view_did_move_to_window(&self) {
            trace_scope!("viewDidMoveToWindow");
            if let Some(tracking_rect) = self.ivars().tracking_rect.take() {
                self.removeTrackingRect(tracking_rect);
            }

            let rect = self.frame();
            let tracking_rect = unsafe {
                self.addTrackingRect_owner_userData_assumeInside(rect, self, ptr::null_mut(), false)
            };
            assert_ne!(tracking_rect, 0, "failed adding tracking rect");
            self.ivars().tracking_rect.set(Some(tracking_rect));

            self.clear_foreign_window_observers();
            let actual_window = self.as_super().window();
            let donor_window = self.donor_window();
            let was_foreign_hosted = self.ivars().foreign_hosted.get();

            // Reparenting passes through a windowless edge. Preserve the
            // previous host mode until AppKit installs the view in its next
            // window so the final edge can synchronize donor/foreign state.
            if actual_window.is_none() {
                self.set_focused(false);
                return;
            }

            let foreign_hosted = match (&actual_window, &donor_window) {
                (Some(actual), Some(donor)) => {
                    Retained::as_ptr(actual).cast::<()>()
                        != Retained::as_ptr(donor).cast::<()>()
                },
                // If the weak donor unexpectedly disappeared while the view
                // still has an actual host, fail closed: external-root
                // mutators must remain suppressed.
                (Some(_), None) => true,
                _ => false,
            };
            self.ivars().foreign_hosted.set(foreign_hosted);
            self.ivars().foreign_host_closing.set(false);

            if foreign_hosted {
                let host = actual_window
                    .as_deref()
                    .expect("foreign-hosted WinitView to have an NSWindow");
                self.observe_foreign_window(host);
                self.capture_foreign_host_metrics();
            } else if let Some(donor) = donor_window.as_deref() {
                // set_cursor_hittest is view-local while foreign-hosted so it
                // cannot make the hidden donor affect mpv's root. Reconcile
                // the donor flag when the view returns.
                donor.setIgnoresMouseEvents(
                    !self.ivars().cursor_hittest.get(),
                );
            }

            if was_foreign_hosted != foreign_hosted {
                self.reset_ime_for_host_transition();
            }

            // Initial donor setup retains upstream winit's event ordering.
            // Only a real foreign-host transition needs an immediate resync.
            if was_foreign_hosted || foreign_hosted {
                // Reparenting is initiated from Iced's update path, while
                // winit's event handler can still be mutably borrowed.
                // Dispatching ScaleFactorChanged synchronously here would
                // re-enter that handler and panic, so resynchronize on the
                // next main-run-loop turn.
                self.schedule_host_metrics(true);
            }
        }

        #[method(frameDidChange:)]
        fn frame_did_change(&self, _event: &NSEvent) {
            trace_scope!("frameDidChange:");
            if let Some(tracking_rect) = self.ivars().tracking_rect.take() {
                self.removeTrackingRect(tracking_rect);
            }

            let rect = self.frame();
            let tracking_rect = unsafe {
                self.addTrackingRect_owner_userData_assumeInside(rect, self, ptr::null_mut(), false)
            };
            assert_ne!(tracking_rect, 0, "failed adding tracking rect");
            self.ivars().tracking_rect.set(Some(tracking_rect));

            // Reparenting may resize the view after it has reached the donor
            // but before the deferred final foreign move is delivered. A raw
            // donor Resized would make Iced adopt the donor scale too early,
            // so fold that resize into the ordered host synchronization.
            if self.has_detached_pending_host_metrics() {
                self.schedule_host_metrics(true);
                return;
            }

            // Emit resize event here rather than from windowDidResize because:
            // 1. When a new window is created as a tab, the frame size may change without a window resize occurring.
            // 2. Even when a window resize does occur on a new tabbed window, it contains the wrong size (includes tab height).
            let logical_size = LogicalSize::new(rect.size.width as f64, rect.size.height as f64);
            let size = logical_size.to_physical::<u32>(self.scale_factor());
            self.queue_event(WindowEvent::Resized(size));
        }

        #[method(drawRect:)]
        fn draw_rect(&self, _rect: NSRect) {
            trace_scope!("drawRect:");

            // It's a workaround for https://github.com/rust-windowing/winit/issues/2640, don't replace with `self.window_id()`.
            self.ivars()
                .app_delegate
                .handle_redraw(self.ivars().event_window_id);

            // This is a direct subclass of NSView, no need to call superclass' drawRect:
        }

        #[method(acceptsFirstResponder)]
        fn accepts_first_responder(&self) -> bool {
            trace_scope!("acceptsFirstResponder");
            true
        }

        // This is necessary to prevent a beefy terminal error on MacBook Pros:
        // IMKInputSession [0x7fc573576ff0 presentFunctionRowItemTextInputViewWithEndpoint:completionHandler:] : [self textInputContext]=0x7fc573558e10 *NO* NSRemoteViewController to client, NSError=Error Domain=NSCocoaErrorDomain Code=4099 "The connection from pid 0 was invalidated from this process." UserInfo={NSDebugDescription=The connection from pid 0 was invalidated from this process.}, com.apple.inputmethod.EmojiFunctionRowItem
        // TODO: Add an API extension for using `NSTouchBar`
        #[method_id(touchBar)]
        fn touch_bar(&self) -> Option<Retained<NSObject>> {
            trace_scope!("touchBar");
            None
        }

        #[method(resetCursorRects)]
        fn reset_cursor_rects(&self) {
            trace_scope!("resetCursorRects");
            let bounds = self.bounds();
            let cursor_state = self.ivars().cursor_state.borrow();
            // We correctly invoke `addCursorRect` only from inside `resetCursorRects`
            if cursor_state.visible {
                self.addCursorRect_cursor(bounds, &cursor_state.cursor);
            } else {
                self.addCursorRect_cursor(bounds, &invisible_cursor());
            }
        }

        #[method_id(hitTest:)]
        fn hit_test(&self, point: NSPoint) -> Option<Retained<NSView>> {
            if self.ivars().cursor_hittest.get() {
                unsafe { msg_send_id![super(self), hitTest: point] }
            } else {
                None
            }
        }

        #[method(winitForeignWindowDidChangeBackingProperties:)]
        fn foreign_backing_changed(&self, _notification: &NSNotification) {
            trace_scope!("winitForeignWindowDidChangeBackingProperties:");
            self.capture_foreign_host_metrics();
            self.schedule_host_metrics(false);
        }

        #[method(winitForeignWindowDidBecomeKey:)]
        fn foreign_became_key(&self, _notification: &NSNotification) {
            trace_scope!("winitForeignWindowDidBecomeKey:");
            if self.is_foreign_hosted() {
                self.set_focused(true);
            }
        }

        #[method(winitForeignWindowDidResignKey:)]
        fn foreign_resigned_key(&self, _notification: &NSNotification) {
            trace_scope!("winitForeignWindowDidResignKey:");
            if self.is_foreign_hosted() {
                self.set_focused(false);
            }
        }

        #[method(winitForeignWindowDidChangeOcclusionState:)]
        fn foreign_occlusion_changed(&self, _notification: &NSNotification) {
            trace_scope!("winitForeignWindowDidChangeOcclusionState:");
            if let Some(host) = self.actual_host_window() {
                self.set_occluded(
                    host.isMiniaturized()
                        || !host
                            .occlusionState()
                            .contains(NSWindowOcclusionState::Visible),
                );
            }
        }

        #[method(winitForeignWindowMetricsChanged:)]
        fn foreign_metrics_changed(&self, _notification: &NSNotification) {
            trace_scope!("winitForeignWindowMetricsChanged:");
            // Root changes can be caused by an mpv command dispatched from an
            // Iced update. Snapshot the root position while the notification
            // still identifies that host, then defer event delivery so an
            // AppKit notification cannot re-enter winit's event handler. The
            // snapshot survives a detach before the queued callback runs.
            self.capture_foreign_host_metrics();
            self.schedule_host_metrics(false);
        }

        #[method(winitForeignWindowWillClose:)]
        fn foreign_will_close(&self, _notification: &NSNotification) {
            trace_scope!("winitForeignWindowWillClose:");
            if !self.is_foreign_hosted()
                || self.ivars().foreign_host_closing.replace(true)
            {
                return;
            }

            self.capture_foreign_host_metrics();
            self.clear_foreign_window_observers();
            self.reset_modifiers();
            self.reset_ime_for_host_transition();

            // The externally owned root is closing. Move the retained view
            // back to its hidden donor immediately so neither AppKit nor the
            // renderer can retain an orphan under the closing mpv window.
            unsafe { self.removeFromSuperview() };
            if let Some(donor) = self.donor_window() {
                donor.setContentView(Some(self));
            } else {
                self.ivars().foreign_hosted.set(false);
                self.set_focused(false);
            }

            // Application code may synchronously react to CloseRequested.
            // Publish it only after the renderer view is no longer retained
            // beneath the external window that is closing.
            self.queue_event(WindowEvent::CloseRequested);
        }
    }

    unsafe impl NSTextInputClient for WinitView {
        #[method(hasMarkedText)]
        fn has_marked_text(&self) -> bool {
            trace_scope!("hasMarkedText");
            self.ivars().marked_text.borrow().length() > 0
        }

        #[method(markedRange)]
        fn marked_range(&self) -> NSRange {
            trace_scope!("markedRange");
            let length = self.ivars().marked_text.borrow().length();
            if length > 0 {
                NSRange::new(0, length)
            } else {
                // Documented to return `{NSNotFound, 0}` if there is no marked range.
                NSRange::new(NSNotFound as NSUInteger, 0)
            }
        }

        #[method(selectedRange)]
        fn selected_range(&self) -> NSRange {
            trace_scope!("selectedRange");
            // Documented to return `{NSNotFound, 0}` if there is no selection.
            NSRange::new(NSNotFound as NSUInteger, 0)
        }

        #[method(setMarkedText:selectedRange:replacementRange:)]
        fn set_marked_text(
            &self,
            string: &NSObject,
            selected_range: NSRange,
            _replacement_range: NSRange,
        ) {
            // TODO: Use _replacement_range, requires changing the event to report surrounding text.
            trace_scope!("setMarkedText:selectedRange:replacementRange:");

            // SAFETY: This method is guaranteed to get either a `NSString` or a `NSAttributedString`.
            let (marked_text, string) = if string.is_kind_of::<NSAttributedString>() {
                let string: *const NSObject = string;
                let string: *const NSAttributedString = string.cast();
                let string = unsafe { &*string };
                (
                    NSMutableAttributedString::from_attributed_nsstring(string),
                    string.string(),
                )
            } else {
                let string: *const NSObject = string;
                let string: *const NSString = string.cast();
                let string = unsafe { &*string };
                (
                    NSMutableAttributedString::from_nsstring(string),
                    string.copy(),
                )
            };

            // Update marked text.
            *self.ivars().marked_text.borrow_mut() = marked_text;

            // Notify IME is active if application still doesn't know it.
            if self.ivars().ime_state.get() == ImeState::Disabled {
                *self.ivars().input_source.borrow_mut() = self.current_input_source();
                self.queue_event(WindowEvent::Ime(Ime::Enabled));
            }

            if unsafe { self.hasMarkedText() } {
                self.ivars().ime_state.set(ImeState::Preedit);
            } else {
                // In case the preedit was cleared, set IME into the Ground state.
                self.ivars().ime_state.set(ImeState::Ground);
            }

            let cursor_range = if string.is_empty() {
                // An empty string basically means that there's no preedit, so indicate that by
                // sending a `None` cursor range.
                None
            } else {
                // Clamp to string length to avoid NSRangeException from out-of-bounds
                // indices sent by macOS IME (e.g. native Pinyin, see
                // https://github.com/alacritty/alacritty/issues/8791).
                let len = string.length();
                let location = selected_range.location.min(len);
                let end = selected_range.end().min(len);
                // Convert the selected range from UTF-16 indices to UTF-8 indices.
                let sub_string_a = unsafe { string.substringToIndex(location) };
                let sub_string_b = unsafe { string.substringToIndex(end) };
                let lowerbound_utf8 = sub_string_a.len();
                let upperbound_utf8 = sub_string_b.len();
                Some((lowerbound_utf8, upperbound_utf8))
            };

            // Send WindowEvent for updating marked text
            self.queue_event(WindowEvent::Ime(Ime::Preedit(string.to_string(), cursor_range)));
        }

        #[method(unmarkText)]
        fn unmark_text(&self) {
            trace_scope!("unmarkText");
            *self.ivars().marked_text.borrow_mut() = NSMutableAttributedString::new();

            let input_context = self.inputContext().expect("input context");
            input_context.discardMarkedText();

            self.queue_event(WindowEvent::Ime(Ime::Preedit(String::new(), None)));
            if self.is_ime_enabled() {
                // Leave the Preedit self.ivars()
                self.ivars().ime_state.set(ImeState::Ground);
            } else {
                tracing::warn!("Expected to have IME enabled when receiving unmarkText");
            }
        }

        #[method_id(validAttributesForMarkedText)]
        fn valid_attributes_for_marked_text(&self) -> Retained<NSArray<NSAttributedStringKey>> {
            trace_scope!("validAttributesForMarkedText");
            NSArray::new()
        }

        #[method_id(attributedSubstringForProposedRange:actualRange:)]
        fn attributed_substring_for_proposed_range(
            &self,
            _range: NSRange,
            _actual_range: *mut NSRange,
        ) -> Option<Retained<NSAttributedString>> {
            trace_scope!("attributedSubstringForProposedRange:actualRange:");
            None
        }

        #[method(characterIndexForPoint:)]
        fn character_index_for_point(&self, _point: NSPoint) -> NSUInteger {
            trace_scope!("characterIndexForPoint:");
            0
        }

        #[method(firstRectForCharacterRange:actualRange:)]
        fn first_rect_for_character_range(
            &self,
            _range: NSRange,
            _actual_range: *mut NSRange,
        ) -> NSRect {
            trace_scope!("firstRectForCharacterRange:actualRange:");
            let rect = NSRect::new(
                self.ivars().ime_position.get(),
                self.ivars().ime_size.get()
            );
            // Return value is expected to be in screen coordinates, so we need a conversion here
            self.host_window()
                .convertRectToScreen(self.convertRect_toView(rect, None))
        }

        #[method(insertText:replacementRange:)]
        fn insert_text(&self, string: &NSObject, _replacement_range: NSRange) {
            // TODO: Use _replacement_range, requires changing the event to report surrounding text.
            trace_scope!("insertText:replacementRange:");

            // SAFETY: This method is guaranteed to get either a `NSString` or a `NSAttributedString`.
            let string = if string.is_kind_of::<NSAttributedString>() {
                let string: *const NSObject = string;
                let string: *const NSAttributedString = string.cast();
                unsafe { &*string }.string().to_string()
            } else {
                let string: *const NSObject = string;
                let string: *const NSString = string.cast();
                unsafe { &*string }.to_string()
            };

            let is_control = string.chars().next().is_some_and(|c| c.is_control());

            // Commit only if we have marked text.
            if unsafe { self.hasMarkedText() } && self.is_ime_enabled() && !is_control {
                self.queue_event(WindowEvent::Ime(Ime::Preedit(String::new(), None)));
                self.queue_event(WindowEvent::Ime(Ime::Commit(string)));
                self.ivars().ime_state.set(ImeState::Committed);
            }
        }

        // Basically, we're sent this message whenever a keyboard event that doesn't generate a "human
        // readable" character happens, i.e. newlines, tabs, and Ctrl+C.
        #[method(doCommandBySelector:)]
        fn do_command_by_selector(&self, _command: Sel) {
            trace_scope!("doCommandBySelector:");
            // We shouldn't forward any character from just committed text, since we'll end up sending
            // it twice with some IMEs like Korean one. We'll also always send `Enter` in that case,
            // which is not desired given it was used to confirm IME input.
            if self.ivars().ime_state.get() == ImeState::Committed {
                return;
            }

            self.ivars().forward_key_to_app.set(true);

            if unsafe { self.hasMarkedText() } && self.ivars().ime_state.get() == ImeState::Preedit
            {
                // Leave preedit so that we also report the key-up for this key.
                self.ivars().ime_state.set(ImeState::Ground);
            }
        }
    }

    unsafe impl WinitView {
        #[method(keyDown:)]
        fn key_down(&self, event: &NSEvent) {
            trace_scope!("keyDown:");
            {
                let mut prev_input_source = self.ivars().input_source.borrow_mut();
                let current_input_source = self.current_input_source();
                if *prev_input_source != current_input_source && self.is_ime_enabled() {
                    *prev_input_source = current_input_source;
                    drop(prev_input_source);
                    self.ivars().ime_state.set(ImeState::Disabled);
                    self.queue_event(WindowEvent::Ime(Ime::Disabled));
                }
            }

            // Get the characters from the event.
            let old_ime_state = self.ivars().ime_state.get();
            self.ivars().forward_key_to_app.set(false);
            let event = replace_event(event, self.option_as_alt());

            // The `interpretKeyEvents` function might call
            // `setMarkedText`, `insertText`, and `doCommandBySelector`.
            // It's important that we call this before queuing the KeyboardInput, because
            // we must send the `KeyboardInput` event during IME if it triggered
            // `doCommandBySelector`. (doCommandBySelector means that the keyboard input
            // is not handled by IME and should be handled by the application)
            if self.ivars().ime_allowed.get() {
                let events_for_nsview = NSArray::from_slice(&[&*event]);
                unsafe { self.interpretKeyEvents(&events_for_nsview) };

                // If the text was committed we must treat the next keyboard event as IME related.
                if self.ivars().ime_state.get() == ImeState::Committed {
                    // Remove any marked text, so normal input can continue.
                    *self.ivars().marked_text.borrow_mut() = NSMutableAttributedString::new();
                }
            }

            self.update_modifiers(&event, false);

            let had_ime_input = match self.ivars().ime_state.get() {
                ImeState::Committed => {
                    // Allow normal input after the commit.
                    self.ivars().ime_state.set(ImeState::Ground);
                    true
                }
                ImeState::Preedit => true,
                // `key_down` could result in preedit clear, so compare old and current state.
                _ => old_ime_state != self.ivars().ime_state.get(),
            };

            if !had_ime_input || self.ivars().forward_key_to_app.get() {
                let key_event = create_key_event(&event, true, unsafe { event.isARepeat() });
                self.queue_event(WindowEvent::KeyboardInput {
                    device_id: DEVICE_ID,
                    event: key_event,
                    is_synthetic: false,
                });
            }
        }

        #[method(keyUp:)]
        fn key_up(&self, event: &NSEvent) {
            trace_scope!("keyUp:");

            let event = replace_event(event, self.option_as_alt());
            self.update_modifiers(&event, false);

            // We want to send keyboard input when we are currently in the ground state.
            if matches!(
                self.ivars().ime_state.get(),
                ImeState::Ground | ImeState::Disabled
            ) {
                self.queue_event(WindowEvent::KeyboardInput {
                    device_id: DEVICE_ID,
                    event: create_key_event(&event, false, false),
                    is_synthetic: false,
                });
            }
        }

        #[method(flagsChanged:)]
        fn flags_changed(&self, event: &NSEvent) {
            trace_scope!("flagsChanged:");

            self.update_modifiers(event, true);
        }

        #[method(insertTab:)]
        fn insert_tab(&self, _sender: Option<&AnyObject>) {
            trace_scope!("insertTab:");
            let window = self.host_window();
            if let Some(first_responder) = window.firstResponder() {
                if *first_responder == ***self {
                    window.selectNextKeyView(Some(self))
                }
            }
        }

        #[method(insertBackTab:)]
        fn insert_back_tab(&self, _sender: Option<&AnyObject>) {
            trace_scope!("insertBackTab:");
            let window = self.host_window();
            if let Some(first_responder) = window.firstResponder() {
                if *first_responder == ***self {
                    window.selectPreviousKeyView(Some(self))
                }
            }
        }

        // Allows us to receive Cmd-. (the shortcut for closing a dialog)
        // https://bugs.eclipse.org/bugs/show_bug.cgi?id=300620#c6
        #[method(cancelOperation:)]
        fn cancel_operation(&self, _sender: Option<&AnyObject>) {
            let mtm = MainThreadMarker::from(self);
            trace_scope!("cancelOperation:");

            let event = NSApplication::sharedApplication(mtm)
                .currentEvent()
                .expect("could not find current event");

            self.update_modifiers(&event, false);
            let event = create_key_event(&event, true, unsafe { event.isARepeat() });

            self.queue_event(WindowEvent::KeyboardInput {
                device_id: DEVICE_ID,
                event,
                is_synthetic: false,
            });
        }

        // In the past (?), `mouseMoved:` events were not generated when the
        // user hovered over a window from a separate window, and as such the
        // application might not know the location of the mouse in the event.
        //
        // To fix this, we emit `mouse_motion` inside of mouse click, mouse
        // scroll, magnify and other gesture event handlers, to ensure that
        // the application's state of where the mouse click was located is up
        // to date.
        //
        // See https://github.com/rust-windowing/winit/pull/1490 for history.

        #[method(mouseDown:)]
        fn mouse_down(&self, event: &NSEvent) {
            trace_scope!("mouseDown:");
            self.mouse_motion(event);
            self.mouse_click(event, ElementState::Pressed);
        }

        #[method(mouseUp:)]
        fn mouse_up(&self, event: &NSEvent) {
            trace_scope!("mouseUp:");
            self.mouse_motion(event);
            self.mouse_click(event, ElementState::Released);
        }

        #[method(rightMouseDown:)]
        fn right_mouse_down(&self, event: &NSEvent) {
            trace_scope!("rightMouseDown:");
            self.mouse_motion(event);
            self.mouse_click(event, ElementState::Pressed);
        }

        #[method(rightMouseUp:)]
        fn right_mouse_up(&self, event: &NSEvent) {
            trace_scope!("rightMouseUp:");
            self.mouse_motion(event);
            self.mouse_click(event, ElementState::Released);
        }

        #[method(otherMouseDown:)]
        fn other_mouse_down(&self, event: &NSEvent) {
            trace_scope!("otherMouseDown:");
            self.mouse_motion(event);
            self.mouse_click(event, ElementState::Pressed);
        }

        #[method(otherMouseUp:)]
        fn other_mouse_up(&self, event: &NSEvent) {
            trace_scope!("otherMouseUp:");
            self.mouse_motion(event);
            self.mouse_click(event, ElementState::Released);
        }

        // No tracing on these because that would be overly verbose

        #[method(mouseMoved:)]
        fn mouse_moved(&self, event: &NSEvent) {
            self.mouse_motion(event);
        }

        #[method(mouseDragged:)]
        fn mouse_dragged(&self, event: &NSEvent) {
            self.mouse_motion(event);
        }

        #[method(rightMouseDragged:)]
        fn right_mouse_dragged(&self, event: &NSEvent) {
            self.mouse_motion(event);
        }

        #[method(otherMouseDragged:)]
        fn other_mouse_dragged(&self, event: &NSEvent) {
            self.mouse_motion(event);
        }

        #[method(mouseEntered:)]
        fn mouse_entered(&self, _event: &NSEvent) {
            trace_scope!("mouseEntered:");
            self.queue_event(WindowEvent::CursorEntered {
                device_id: DEVICE_ID,
            });
        }

        #[method(mouseExited:)]
        fn mouse_exited(&self, _event: &NSEvent) {
            trace_scope!("mouseExited:");

            self.queue_event(WindowEvent::CursorLeft {
                device_id: DEVICE_ID,
            });
        }

        #[method(scrollWheel:)]
        fn scroll_wheel(&self, event: &NSEvent) {
            trace_scope!("scrollWheel:");

            self.mouse_motion(event);

            let delta = {
                let (x, y) = unsafe { (event.scrollingDeltaX(), event.scrollingDeltaY()) };
                if unsafe { event.hasPreciseScrollingDeltas() } {
                    let delta = LogicalPosition::new(x, y).to_physical(self.scale_factor());
                    MouseScrollDelta::PixelDelta(delta)
                } else {
                    MouseScrollDelta::LineDelta(x as f32, y as f32)
                }
            };

            // The "momentum phase," if any, has higher priority than touch phase (the two should
            // be mutually exclusive anyhow, which is why the API is rather incoherent). If no momentum
            // phase is recorded (or rather, the started/ended cases of the momentum phase) then we
            // report the touch phase.
            #[allow(non_upper_case_globals)]
            let phase = match unsafe { event.momentumPhase() } {
                NSEventPhase::MayBegin | NSEventPhase::Began => TouchPhase::Started,
                NSEventPhase::Ended | NSEventPhase::Cancelled => TouchPhase::Ended,
                _ => match unsafe { event.phase() } {
                    NSEventPhase::MayBegin | NSEventPhase::Began => TouchPhase::Started,
                    NSEventPhase::Ended | NSEventPhase::Cancelled => TouchPhase::Ended,
                    _ => TouchPhase::Moved,
                },
            };

            self.update_modifiers(event, false);

            self.ivars().app_delegate.maybe_queue_device_event(DeviceEvent::MouseWheel { delta });
            self.queue_event(WindowEvent::MouseWheel {
                device_id: DEVICE_ID,
                delta,
                phase,
            });
        }

        #[method(magnifyWithEvent:)]
        fn magnify_with_event(&self, event: &NSEvent) {
            trace_scope!("magnifyWithEvent:");

            self.mouse_motion(event);

            #[allow(non_upper_case_globals)]
            let phase = match unsafe { event.phase() } {
                NSEventPhase::Began => TouchPhase::Started,
                NSEventPhase::Changed => TouchPhase::Moved,
                NSEventPhase::Cancelled => TouchPhase::Cancelled,
                NSEventPhase::Ended => TouchPhase::Ended,
                _ => return,
            };

            self.queue_event(WindowEvent::PinchGesture {
                device_id: DEVICE_ID,
                delta: unsafe { event.magnification() },
                phase,
            });
        }

        #[method(smartMagnifyWithEvent:)]
        fn smart_magnify_with_event(&self, event: &NSEvent) {
            trace_scope!("smartMagnifyWithEvent:");

            self.mouse_motion(event);

            self.queue_event(WindowEvent::DoubleTapGesture {
                device_id: DEVICE_ID,
            });
        }

        #[method(rotateWithEvent:)]
        fn rotate_with_event(&self, event: &NSEvent) {
            trace_scope!("rotateWithEvent:");

            self.mouse_motion(event);

            #[allow(non_upper_case_globals)]
            let phase = match unsafe { event.phase() } {
                NSEventPhase::Began => TouchPhase::Started,
                NSEventPhase::Changed => TouchPhase::Moved,
                NSEventPhase::Cancelled => TouchPhase::Cancelled,
                NSEventPhase::Ended => TouchPhase::Ended,
                _ => return,
            };

            self.queue_event(WindowEvent::RotationGesture {
                device_id: DEVICE_ID,
                delta: unsafe { event.rotation() },
                phase,
            });
        }

        #[method(pressureChangeWithEvent:)]
        fn pressure_change_with_event(&self, event: &NSEvent) {
            trace_scope!("pressureChangeWithEvent:");

            self.queue_event(WindowEvent::TouchpadPressure {
                device_id: DEVICE_ID,
                pressure: unsafe { event.pressure() },
                stage: unsafe { event.stage() } as i64,
            });
        }

        // Allows us to receive Ctrl-Tab and Ctrl-Esc.
        // Note that this *doesn't* help with any missing Cmd inputs.
        // https://github.com/chromium/chromium/blob/a86a8a6bcfa438fa3ac2eba6f02b3ad1f8e0756f/ui/views/cocoa/bridged_content_view.mm#L816
        #[method(_wantsKeyDownForEvent:)]
        fn wants_key_down_for_event(&self, _event: &NSEvent) -> bool {
            trace_scope!("_wantsKeyDownForEvent:");
            true
        }

        #[method(acceptsFirstMouse:)]
        fn accepts_first_mouse(&self, _event: &NSEvent) -> bool {
            trace_scope!("acceptsFirstMouse:");
            self.ivars().accepts_first_mouse
        }
    }
);

impl WinitView {
    pub(super) fn new(
        app_delegate: &ApplicationDelegate,
        window: &WinitWindow,
        accepts_first_mouse: bool,
        option_as_alt: OptionAsAlt,
    ) -> Retained<Self> {
        let mtm = MainThreadMarker::from(window);
        let this =
            mtm.alloc().set_ivars(ViewState {
                app_delegate: app_delegate.retain(),
                cursor_state: Default::default(),
                ime_position: Default::default(),
                ime_size: Default::default(),
                modifiers: Default::default(),
                phys_modifiers: Default::default(),
                tracking_rect: Default::default(),
                ime_state: Default::default(),
                input_source: Default::default(),
                ime_allowed: Default::default(),
                forward_key_to_app: Default::default(),
                marked_text: Default::default(),
                accepts_first_mouse,
                event_window_id: window.id(),
                donor_window: WeakId::new(&window.retain()),
                foreign_hosted: Cell::new(false),
                foreign_host_closing: Cell::new(false),
                previous_scale_factor: Cell::new(
                    window.backingScaleFactor() as f64
                ),
                previous_host_position: Cell::new(None),
                pending_host_metrics: Cell::new(None),
                focused: Cell::new(None),
                occluded: Cell::new(None),
                cursor_hittest: Cell::new(true),
                host_metrics_scheduled: Cell::new(false),
                host_metrics_force_scale: Cell::new(false),
                option_as_alt: Cell::new(option_as_alt),
            });
        let this: Retained<Self> = unsafe { msg_send_id![super(this), init] };

        this.setPostsFrameChangedNotifications(true);
        let notification_center =
            unsafe { NSNotificationCenter::defaultCenter() };
        unsafe {
            notification_center.addObserver_selector_name_object(
                &this,
                sel!(frameDidChange:),
                Some(NSViewFrameDidChangeNotification),
                Some(&this),
            )
        }

        *this.ivars().input_source.borrow_mut() = this.current_input_source();

        this
    }

    pub(super) fn donor_window(&self) -> Option<Retained<WinitWindow>> {
        self.ivars().donor_window.load()
    }

    fn actual_host_window(&self) -> Option<Retained<NSWindow>> {
        self.as_super().window()
    }

    pub(super) fn host_window(&self) -> Retained<NSWindow> {
        self.actual_host_window()
            .or_else(|| self.donor_window().map(Retained::into_super))
            .expect("WinitView to have an actual host or retained donor window")
    }

    pub(super) fn is_foreign_hosted(&self) -> bool {
        self.ivars().foreign_hosted.get()
    }

    fn queue_event(&self, event: WindowEvent) {
        self.ivars()
            .app_delegate
            .maybe_queue_window_event(self.ivars().event_window_id, event);
    }

    pub(super) fn set_focused(&self, focused: bool) {
        if self.ivars().focused.replace(Some(focused)) == Some(focused) {
            return;
        }
        if !focused {
            self.reset_modifiers();
        }
        self.queue_event(WindowEvent::Focused(focused));
    }

    pub(super) fn set_occluded(&self, occluded: bool) {
        if self.ivars().occluded.replace(Some(occluded)) == Some(occluded) {
            return;
        }
        self.queue_event(WindowEvent::Occluded(occluded));
    }

    fn scale_factor(&self) -> f64 {
        self.host_window().backingScaleFactor() as f64
    }

    fn emit_resized(&self) {
        let bounds = self.bounds();
        let logical_size = LogicalSize::new(
            bounds.size.width as f64,
            bounds.size.height as f64,
        );
        self.queue_event(WindowEvent::Resized(
            logical_size.to_physical::<u32>(self.scale_factor()),
        ));
    }

    fn foreign_host_metrics(&self) -> Option<ForeignHostMetrics> {
        if !self.is_foreign_hosted() {
            return None;
        }
        let host = self.actual_host_window()?;
        let scale_factor = host.backingScaleFactor() as f64;
        let position = flip_window_screen_coordinates(host.frame());
        let position = LogicalPosition::new(position.x, position.y)
            .to_physical(scale_factor);
        let bounds = self.bounds();
        let inner_size = LogicalSize::new(
            bounds.size.width as f64,
            bounds.size.height as f64,
        )
        .to_physical(scale_factor);
        Some(ForeignHostMetrics {
            position,
            scale_factor,
            inner_size,
        })
    }

    fn capture_foreign_host_metrics(&self) {
        if let Some(metrics) = self.foreign_host_metrics() {
            self.ivars().pending_host_metrics.set(Some(metrics));
        }
    }

    fn emit_moved_position(&self, position: PhysicalPosition<i32>) {
        if self.ivars().previous_host_position.replace(Some(position))
            == Some(position)
        {
            return;
        }
        self.queue_event(WindowEvent::Moved(position));
    }

    fn has_detached_pending_host_metrics(&self) -> bool {
        self.ivars().pending_host_metrics.get().is_some()
            && (!self.is_foreign_hosted()
                || self.actual_host_window().is_none())
    }

    fn emit_pending_host_moved(&self) -> bool {
        let Some(metrics) = self.ivars().pending_host_metrics.take() else {
            return false;
        };
        self.emit_moved_position(metrics.position);
        true
    }

    fn emit_captured_host_scale_factor_changed(
        &self,
        metrics: ForeignHostMetrics,
    ) {
        if metrics.scale_factor == self.ivars().previous_scale_factor.get() {
            return;
        }
        self.ivars().previous_scale_factor.set(metrics.scale_factor);

        let captured_inner_size = Arc::new(Mutex::new(metrics.inner_size));
        self.ivars().app_delegate.handle_window_event(
            self.ivars().event_window_id,
            WindowEvent::ScaleFactorChanged {
                scale_factor: metrics.scale_factor,
                inner_size_writer: InnerSizeWriter::new(Arc::downgrade(
                    &captured_inner_size,
                )),
            },
        );
        // The captured foreign root is already detached and was the sole
        // geometry authority. Keep the writer alive through dispatch, but
        // never apply its request to the donor. Do not publish a captured
        // Resized here: Iced resolves that event against the raw window's
        // current (donor) scale, which would undo the captured scale before
        // the pending Moved is converted. Normal donor reconciliation below
        // publishes the next authoritative resize.
        drop(captured_inner_size);
    }

    fn emit_detached_pending_host_moved(&self) -> bool {
        let Some(metrics) = self.ivars().pending_host_metrics.take() else {
            return false;
        };
        self.emit_captured_host_scale_factor_changed(metrics);
        self.emit_moved_position(metrics.position);
        true
    }

    fn emit_host_moved(&self) {
        if self.emit_pending_host_moved() {
            return;
        }
        let metrics = self.foreign_host_metrics();
        let Some(metrics) = metrics else {
            return;
        };
        self.emit_moved_position(metrics.position);
    }

    pub(super) fn handle_scale_factor_changed(&self, force: bool) -> bool {
        let scale_factor = self.scale_factor();
        if !force && scale_factor == self.ivars().previous_scale_factor.get() {
            return false;
        }
        self.ivars().previous_scale_factor.set(scale_factor);

        let bounds = self.bounds();
        let logical_size = LogicalSize::new(
            bounds.size.width as f64,
            bounds.size.height as f64,
        );
        let suggested_size = logical_size.to_physical::<u32>(scale_factor);
        let new_inner_size = Arc::new(Mutex::new(suggested_size));
        self.ivars().app_delegate.handle_window_event(
            self.ivars().event_window_id,
            WindowEvent::ScaleFactorChanged {
                scale_factor,
                inner_size_writer: InnerSizeWriter::new(Arc::downgrade(
                    &new_inner_size,
                )),
            },
        );
        let requested_size = *new_inner_size.lock().unwrap();
        drop(new_inner_size);

        let actual_size = if !self.is_foreign_hosted()
            && requested_size != suggested_size
        {
            if let Some(donor) = self.donor_window() {
                let logical = requested_size.to_logical::<f64>(scale_factor);
                donor
                    .setContentSize(NSSize::new(logical.width, logical.height));
                requested_size
            } else {
                suggested_size
            }
        } else {
            // The external mpv root is the sole geometry authority. Ignore an
            // InnerSizeWriter request while hosted and report actual view size.
            suggested_size
        };
        self.ivars().app_delegate.handle_window_event(
            self.ivars().event_window_id,
            WindowEvent::Resized(actual_size),
        );
        true
    }

    fn schedule_host_metrics(&self, force_scale: bool) {
        if force_scale {
            self.ivars().host_metrics_force_scale.set(true);
        }
        if self.ivars().host_metrics_scheduled.replace(true) {
            return;
        }

        let mtm = MainThreadMarker::from(self);
        let this = self.retain();
        RunLoop::main(mtm).queue_closure(move || {
            this.ivars().host_metrics_scheduled.set(false);
            let force_scale =
                this.ivars().host_metrics_force_scale.replace(false);
            this.synchronize_host_state(force_scale);
        });
    }

    fn synchronize_host_state(&self, force_scale: bool) {
        // A detached view must publish the captured foreign-root position
        // before any donor ScaleFactorChanged event. Iced converts Moved from
        // physical coordinates using its current scale, so the detached
        // helper first replays a coalesced captured foreign scale when needed.
        if self.has_detached_pending_host_metrics() {
            self.emit_detached_pending_host_moved();
        }
        let Some(host) = self.actual_host_window() else {
            // A pending foreign-root move is independent of the view's
            // current host. Deliver it even if detach completed before this
            // deferred callback.
            self.emit_detached_pending_host_moved();
            return;
        };
        self.set_focused(host.isKeyWindow());
        self.set_occluded(
            host.isMiniaturized()
                || !host
                    .occlusionState()
                    .contains(NSWindowOcclusionState::Visible),
        );
        if !self.handle_scale_factor_changed(force_scale) {
            self.emit_resized();
        }
        self.emit_host_moved();
        host.invalidateCursorRectsForView(self);
        self.ivars()
            .app_delegate
            .queue_redraw(self.ivars().event_window_id);
    }

    fn observe_foreign_window(&self, host: &NSWindow) {
        let center = unsafe { NSNotificationCenter::defaultCenter() };
        unsafe {
            center.addObserver_selector_name_object(
                self,
                sel!(winitForeignWindowDidBecomeKey:),
                Some(NSWindowDidBecomeKeyNotification),
                Some(host),
            );
            center.addObserver_selector_name_object(
                self,
                sel!(winitForeignWindowDidResignKey:),
                Some(NSWindowDidResignKeyNotification),
                Some(host),
            );
            center.addObserver_selector_name_object(
                self,
                sel!(winitForeignWindowDidChangeBackingProperties:),
                Some(NSWindowDidChangeBackingPropertiesNotification),
                Some(host),
            );
            center.addObserver_selector_name_object(
                self,
                sel!(winitForeignWindowDidChangeOcclusionState:),
                Some(NSWindowDidChangeOcclusionStateNotification),
                Some(host),
            );
            center.addObserver_selector_name_object(
                self,
                sel!(winitForeignWindowMetricsChanged:),
                Some(NSWindowDidResizeNotification),
                Some(host),
            );
            center.addObserver_selector_name_object(
                self,
                sel!(winitForeignWindowMetricsChanged:),
                Some(NSWindowDidMoveNotification),
                Some(host),
            );
            center.addObserver_selector_name_object(
                self,
                sel!(winitForeignWindowMetricsChanged:),
                Some(NSWindowDidChangeScreenNotification),
                Some(host),
            );
            center.addObserver_selector_name_object(
                self,
                sel!(winitForeignWindowMetricsChanged:),
                Some(NSWindowDidMiniaturizeNotification),
                Some(host),
            );
            center.addObserver_selector_name_object(
                self,
                sel!(winitForeignWindowMetricsChanged:),
                Some(NSWindowDidDeminiaturizeNotification),
                Some(host),
            );
            center.addObserver_selector_name_object(
                self,
                sel!(winitForeignWindowMetricsChanged:),
                Some(NSWindowDidEnterFullScreenNotification),
                Some(host),
            );
            center.addObserver_selector_name_object(
                self,
                sel!(winitForeignWindowMetricsChanged:),
                Some(NSWindowDidExitFullScreenNotification),
                Some(host),
            );
            center.addObserver_selector_name_object(
                self,
                sel!(winitForeignWindowWillClose:),
                Some(NSWindowWillCloseNotification),
                Some(host),
            );
        }
    }

    fn clear_foreign_window_observers(&self) {
        let center = unsafe { NSNotificationCenter::defaultCenter() };
        unsafe {
            center.removeObserver_name_object(
                self,
                Some(NSWindowDidBecomeKeyNotification),
                None,
            );
            center.removeObserver_name_object(
                self,
                Some(NSWindowDidResignKeyNotification),
                None,
            );
            center.removeObserver_name_object(
                self,
                Some(NSWindowDidChangeBackingPropertiesNotification),
                None,
            );
            center.removeObserver_name_object(
                self,
                Some(NSWindowDidChangeOcclusionStateNotification),
                None,
            );
            center.removeObserver_name_object(
                self,
                Some(NSWindowDidResizeNotification),
                None,
            );
            center.removeObserver_name_object(
                self,
                Some(NSWindowDidMoveNotification),
                None,
            );
            center.removeObserver_name_object(
                self,
                Some(NSWindowDidChangeScreenNotification),
                None,
            );
            center.removeObserver_name_object(
                self,
                Some(NSWindowDidMiniaturizeNotification),
                None,
            );
            center.removeObserver_name_object(
                self,
                Some(NSWindowDidDeminiaturizeNotification),
                None,
            );
            center.removeObserver_name_object(
                self,
                Some(NSWindowDidEnterFullScreenNotification),
                None,
            );
            center.removeObserver_name_object(
                self,
                Some(NSWindowDidExitFullScreenNotification),
                None,
            );
            center.removeObserver_name_object(
                self,
                Some(NSWindowWillCloseNotification),
                None,
            );
        }
    }

    fn reset_ime_for_host_transition(&self) {
        *self.ivars().marked_text.borrow_mut() =
            NSMutableAttributedString::new();
        if let Some(input_context) = self.inputContext() {
            input_context.discardMarkedText();
        }
        if self.ivars().ime_state.replace(ImeState::Disabled)
            != ImeState::Disabled
        {
            self.queue_event(WindowEvent::Ime(Ime::Disabled));
        }
    }

    pub(super) fn set_cursor_hittest(&self, hittest: bool) {
        self.ivars().cursor_hittest.set(hittest);
    }

    pub(super) fn prepare_for_donor_close(&self) {
        self.capture_foreign_host_metrics();
        self.clear_foreign_window_observers();
        if self.is_foreign_hosted() {
            unsafe { self.removeFromSuperview() };
        }
        self.ivars().foreign_hosted.set(false);
        self.ivars().foreign_host_closing.set(false);
        self.set_focused(false);
        self.reset_ime_for_host_transition();
    }

    fn is_ime_enabled(&self) -> bool {
        !matches!(self.ivars().ime_state.get(), ImeState::Disabled)
    }

    fn current_input_source(&self) -> String {
        self.inputContext()
            .expect("input context")
            .selectedKeyboardInputSource()
            .map(|input_source| input_source.to_string())
            .unwrap_or_default()
    }

    pub(super) fn cursor_icon(&self) -> Retained<NSCursor> {
        self.ivars().cursor_state.borrow().cursor.clone()
    }

    pub(super) fn set_cursor_icon(&self, icon: Retained<NSCursor>) {
        let mut cursor_state = self.ivars().cursor_state.borrow_mut();
        cursor_state.cursor = icon;
    }

    /// Set whether the cursor should be visible or not.
    ///
    /// Returns whether the state changed.
    pub(super) fn set_cursor_visible(&self, visible: bool) -> bool {
        let mut cursor_state = self.ivars().cursor_state.borrow_mut();
        if visible != cursor_state.visible {
            cursor_state.visible = visible;
            true
        } else {
            false
        }
    }

    pub(super) fn set_ime_allowed(&self, ime_allowed: bool) {
        if self.ivars().ime_allowed.get() == ime_allowed {
            return;
        }
        self.ivars().ime_allowed.set(ime_allowed);
        if self.ivars().ime_allowed.get() {
            return;
        }

        // Clear markedText
        *self.ivars().marked_text.borrow_mut() =
            NSMutableAttributedString::new();

        if self.ivars().ime_state.get() != ImeState::Disabled {
            self.ivars().ime_state.set(ImeState::Disabled);
            self.queue_event(WindowEvent::Ime(Ime::Disabled));
        }
    }

    pub(super) fn set_ime_cursor_area(&self, position: NSPoint, size: NSSize) {
        self.ivars().ime_position.set(position);
        self.ivars().ime_size.set(size);
        let input_context = self.inputContext().expect("input context");
        input_context.invalidateCharacterCoordinates();
    }

    /// Reset modifiers and emit a synthetic ModifiersChanged event if deemed necessary.
    pub(super) fn reset_modifiers(&self) {
        if !self.ivars().modifiers.get().state().is_empty() {
            self.ivars().modifiers.set(Modifiers::default());
            self.queue_event(WindowEvent::ModifiersChanged(
                self.ivars().modifiers.get(),
            ));
        }
    }

    pub(super) fn set_option_as_alt(&self, value: OptionAsAlt) {
        self.ivars().option_as_alt.set(value)
    }

    pub(super) fn option_as_alt(&self) -> OptionAsAlt {
        self.ivars().option_as_alt.get()
    }

    /// Update modifiers if `event` has something different
    fn update_modifiers(
        &self,
        ns_event: &NSEvent,
        is_flags_changed_event: bool,
    ) {
        use ElementState::{Pressed, Released};

        let current_modifiers = event_mods(ns_event);
        let prev_modifiers = self.ivars().modifiers.get();
        self.ivars().modifiers.set(current_modifiers);

        // This function was called form the flagsChanged event, which is triggered
        // when the user presses/releases a modifier even if the same kind of modifier
        // has already been pressed.
        //
        // When flags changed event has key code of zero it means that event doesn't carry any key
        // event, thus we can't generate regular presses based on that. The `ModifiersChanged`
        // later will work though, since the flags are attached to the event and contain valid
        // information.
        'send_event: {
            if is_flags_changed_event && unsafe { ns_event.keyCode() } != 0 {
                let scancode = unsafe { ns_event.keyCode() };
                let physical_key = scancode_to_physicalkey(scancode as u32);

                let logical_key = code_to_key(physical_key, scancode);
                // Ignore processing of unknown modifiers because we can't determine whether
                // it was pressed or release reliably.
                //
                // Furthermore, sometimes normal keys are reported inside flagsChanged:, such as
                // when holding Caps Lock while pressing another key, see:
                // https://github.com/alacritty/alacritty/issues/8268
                let Some(event_modifier) = key_to_modifier(&logical_key) else {
                    break 'send_event;
                };

                let mut event = KeyEvent {
                    location: code_to_location(physical_key),
                    logical_key: logical_key.clone(),
                    physical_key,
                    repeat: false,
                    // We'll correct this later.
                    state: Pressed,
                    text: None,
                    platform_specific: KeyEventExtra {
                        text_with_all_modifiers: None,
                        key_without_modifiers: logical_key.clone(),
                    },
                };

                let location_mask =
                    ModLocationMask::from_location(event.location);

                let mut phys_mod_state =
                    self.ivars().phys_modifiers.borrow_mut();
                let phys_mod = phys_mod_state
                    .entry(logical_key)
                    .or_insert(ModLocationMask::empty());

                let is_active =
                    current_modifiers.state().contains(event_modifier);
                let mut events = VecDeque::with_capacity(2);

                // There is no API for getting whether the button was pressed or released
                // during this event. For this reason we have to do a bit of magic below
                // to come up with a good guess whether this key was pressed or released.
                // (This is not trivial because there are multiple buttons that may affect
                // the same modifier)
                if !is_active {
                    event.state = Released;
                    if phys_mod.contains(ModLocationMask::LEFT) {
                        let mut event = event.clone();
                        event.location = KeyLocation::Left;
                        event.physical_key =
                            get_left_modifier_code(&event.logical_key).into();
                        events.push_back(WindowEvent::KeyboardInput {
                            device_id: DEVICE_ID,
                            event,
                            is_synthetic: false,
                        });
                    }
                    if phys_mod.contains(ModLocationMask::RIGHT) {
                        event.location = KeyLocation::Right;
                        event.physical_key =
                            get_right_modifier_code(&event.logical_key).into();
                        events.push_back(WindowEvent::KeyboardInput {
                            device_id: DEVICE_ID,
                            event,
                            is_synthetic: false,
                        });
                    }
                    *phys_mod = ModLocationMask::empty();
                } else {
                    if *phys_mod == location_mask {
                        // Here we hit a contradiction:
                        // The modifier state was "changed" to active,
                        // yet the only pressed modifier key was the one that we
                        // just got a change event for.
                        // This seemingly means that the only pressed modifier is now released,
                        // but at the same time the modifier became active.
                        //
                        // But this scenario is possible if we released modifiers
                        // while the application was not in focus. (Because we don't
                        // get informed of modifier key events while the application
                        // is not focused)

                        // In this case we prioritize the information
                        // about the current modifier state which means
                        // that the button was pressed.
                        event.state = Pressed;
                    } else {
                        phys_mod.toggle(location_mask);
                        let is_pressed = phys_mod.contains(location_mask);
                        event.state =
                            if is_pressed { Pressed } else { Released };
                    }

                    events.push_back(WindowEvent::KeyboardInput {
                        device_id: DEVICE_ID,
                        event,
                        is_synthetic: false,
                    });
                }

                drop(phys_mod_state);

                for event in events {
                    self.queue_event(event);
                }
            }
        }

        if prev_modifiers == current_modifiers {
            return;
        }

        self.queue_event(WindowEvent::ModifiersChanged(
            self.ivars().modifiers.get(),
        ));
    }

    fn mouse_click(&self, event: &NSEvent, button_state: ElementState) {
        let button = mouse_button(event);

        self.update_modifiers(event, false);

        self.queue_event(WindowEvent::MouseInput {
            device_id: DEVICE_ID,
            state: button_state,
            button,
        });
    }

    fn mouse_motion(&self, event: &NSEvent) {
        let window_point = unsafe { event.locationInWindow() };
        let view_point = self.convertPoint_fromView(window_point, None);
        let frame = self.frame();

        if view_point.x.is_sign_negative()
            || view_point.y.is_sign_negative()
            || view_point.x > frame.size.width
            || view_point.y > frame.size.height
        {
            let mouse_buttons_down = unsafe { NSEvent::pressedMouseButtons() };
            if mouse_buttons_down == 0 {
                // Point is outside of the client area (view) and no buttons are pressed
                return;
            }
        }

        let view_point = LogicalPosition::new(view_point.x, view_point.y);

        self.update_modifiers(event, false);

        self.queue_event(WindowEvent::CursorMoved {
            device_id: DEVICE_ID,
            position: view_point.to_physical(self.scale_factor()),
        });
    }
}

/// Get the mouse button from the NSEvent.
fn mouse_button(event: &NSEvent) -> MouseButton {
    // The buttonNumber property only makes sense for the mouse events:
    // NSLeftMouse.../NSRightMouse.../NSOtherMouse...
    // For the other events, it's always set to 0.
    // MacOS only defines the left, right and middle buttons, 3..=31 are left as generic buttons,
    // but 3 and 4 are very commonly used as Back and Forward by hardware vendors and applications.
    match unsafe { event.buttonNumber() } {
        0 => MouseButton::Left,
        1 => MouseButton::Right,
        2 => MouseButton::Middle,
        3 => MouseButton::Back,
        4 => MouseButton::Forward,
        n => MouseButton::Other(n as u16),
    }
}

// NOTE: to get option as alt working we need to rewrite events
// we're getting from the operating system, which makes it
// impossible to provide such events as extra in `KeyEvent`.
fn replace_event(
    event: &NSEvent,
    option_as_alt: OptionAsAlt,
) -> Retained<NSEvent> {
    let ev_mods = event_mods(event).state;
    let ignore_alt_characters = match option_as_alt {
        OptionAsAlt::OnlyLeft if lalt_pressed(event) => true,
        OptionAsAlt::OnlyRight if ralt_pressed(event) => true,
        OptionAsAlt::Both if ev_mods.alt_key() => true,
        _ => false,
    } && !ev_mods.control_key()
        && !ev_mods.super_key();

    if ignore_alt_characters {
        let ns_chars = unsafe {
            event
                .charactersIgnoringModifiers()
                .expect("expected characters to be non-null")
        };

        unsafe {
            NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
                event.r#type(),
                event.locationInWindow(),
                event.modifierFlags(),
                event.timestamp(),
                event.windowNumber(),
                None,
                &ns_chars,
                &ns_chars,
                event.isARepeat(),
                event.keyCode(),
            )
            .unwrap()
        }
    } else {
        event.copy()
    }
}
