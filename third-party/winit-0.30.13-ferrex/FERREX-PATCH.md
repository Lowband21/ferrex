# Ferrex macOS foreign-view patch

This directory is a source copy of crates.io `winit` 0.30.13
(`a6755fa58a9f8350bd1e472d4c3fcc25f824ec358933bba33306d0b63df5978d`).
The upstream license is retained in `LICENSE`.

Ferrex carries a narrow AppKit patch because its macOS native-mpv presenter
keeps winit's window identity in an unordered staging `NSWindow` while moving
the associated `WinitView` into mpv's externally owned root `NSWindow`.
Upstream 0.30.13 assumes that `WinitView` always remains the staging window's
content view. That assumption makes raw-handle recreation unsafe and leaves
move, scale, focus, cursor, IME, and resize behavior tied to the wrong window
after reparenting.

The Ferrex delta is intentionally limited to macOS implementation files. It
must:

- retain the `WinitView` directly instead of recovering it through an unsafe
  cast of the staging window's current content view;
- preserve the staging `WindowId` for event routing;
- use the view's actual host `NSWindow` for host-sensitive position, geometry,
  scale, focus, cursor, IME, and drag behavior;
- defer host-transition scale/resize delivery to the next main-run-loop turn
  so AppKit reparent callbacks cannot re-enter winit's borrowed event handler;
- snapshot foreign-root movement in physical coordinates at notification time
  and deliver the newest position exactly once even if detach completes before
  the deferred callback, before any donor scale-factor transition can change
  how Iced converts that physical position;
- retain the captured foreign scale and view size so a coalesced backing-scale
  change is replayed before the final move, keep its size writer valid without
  applying that request to the donor, and defer `Resized` until authoritative
  donor reconciliation;
- mirror relevant host-window notifications while the view is foreign-hosted;
- remove those observations automatically when the view returns to staging;
- leave staging-window visibility, destruction, and application identity under
  normal winit ownership.

Changed source files:

- `src/platform_impl/macos/view.rs` splits stable event identity from the
  effective AppKit host, binds external-root notifications, preserves a final
  host move across notification-to-detach races, reports view-local metrics,
  and owns observer/IME/focus cleanup;
- `src/platform_impl/macos/window_delegate.rs` retains the exact view, returns
  effective-host queries, and prevents window/lifecycle setters from mutating
  either mpv's root or the hidden donor while foreign-hosted;
- `src/platform_impl/macos/window.rs` detaches a foreign-hosted view before the
  donor closes.

Notification registration is object-scoped to the current external root.
Cleanup is deliberately name-scoped with `object: nil`: this removes a stale
registration even when the previous root has already deallocated, while
leaving the independent view-frame notification untouched.

The donor remains a winit-owned, unordered staging object so Iced keeps its
normal logical window and renderer lifecycle. It is not a presented overlay;
all externally visible window operations remain owned by mpv's root. Ferrex
must route player fullscreen/lifecycle commands through libmpv and native
background drag through the retained root, never through generic donor-window
actions.

Apple Silicon and Intel acceptance evidence is required before this patch can
be treated as production-qualified.
