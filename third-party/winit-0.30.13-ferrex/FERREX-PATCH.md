# Ferrex macOS foreign-view patch

This directory is a source copy of crates.io `winit` 0.30.13
(`a6755fa58a9f8350bd1e472d4c3fcc25f824ec358933bba33306d0b63df5978d`).
The upstream license is retained in `LICENSE`.

## Why it exists

Ferrex's macOS presenter keeps winit's logical window in an unordered donor
`NSWindow` while hosting its renderer `WinitView` inside an externally owned
`NSWindow`. Upstream winit 0.30.13 assumes that the renderer view remains the
donor's content view, leaving window-sensitive state tied to the wrong host
after reparenting.

The macOS-only patch:

- retains the actual `WinitView` instead of recovering it from the donor;
- preserves the donor `WindowId` for event routing while using the view's
  effective host for geometry, scale, focus, cursor, IME, and notifications;
- prevents donor window operations from mutating the foreign host; and
- removes foreign-host observations and restores donor ownership before close.

The changed implementation files are:

- `src/platform_impl/macos/view.rs`;
- `src/platform_impl/macos/window_delegate.rs`; and
- `src/platform_impl/macos/window.rs`.

Detailed notification ordering, event coalescing, and teardown behavior is
documented beside the implementation and its regression tests.

Although limited to macOS, this is not a small semantic delta: it spans view
retention, event identity, host metrics, focus, IME, notifications, scaling,
and teardown. Ferrex will not carry or expand that surface indefinitely.

## Validation

The foreign-view presenter passed functional one-window validation on Apple
Silicon. Intel macOS is legacy and outside the supported validation matrix.

## Exit contract

This is a temporary fork. The next winit/Iced upgrade must consume released
generic foreign-view support or select the Ferrex-owned AppKit host
contingency; rebasing or expanding this fork is not an accepted outcome. The
full upstream-or-delete decision is in the
[native mpv design](../../docs/specs/native-mpv-playback.md#winit-fork-exit).
