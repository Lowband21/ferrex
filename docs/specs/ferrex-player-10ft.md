# Ferrex Player 10-Foot Mode

> Spec for a dedicated 10-foot / controller-driven interface in the Iced
> desktop player, targeting Steam Deck, HTPC, and console-like devices.

## Status

| Field | Value |
|---|---|
| Created | 2025-07-15 |
| Depends on | Existing ferrex-player architecture |
| Blocks | Mobile TV interfaces (tvOS, Android TV) — this is the UX reference |
| Phase | Specification |

---

## Vision

A full-screen, controller/remote-navigable interface mode within ferrex-player
that feels like a native console media app. This is NOT a separate binary — it's
a mode switch within the existing player, leveraging the same domain layer,
API client, caching, and video pipeline.

Primary targets:
- **Steam Deck** (handheld, gamepad, 800p/1280x800 display)
- **Steam Big Picture mode** on desktop/HTPC
- **Any Linux HTPC** with a wireless controller or remote
- **Couch use** with a Bluetooth controller or keyboard

This mode serves as the **reference implementation** for tvOS and Android TV
interfaces. UX patterns, navigation model, and information hierarchy validated
here will be ported to mobile TV apps.

A core motivation is **innovating on the traditional TV media library UX**.
Existing TV media apps (Plex, Jellyfin, Emby, Infuse) use similar row-of-rows
layouts with limited interactivity. Ferrex has unique leverage here: native
Iced rendering performance, zero-copy rkyv data access, custom wgpu shaders
for poster grids, and the existing motion controller system. This combination
enables UI patterns that aren't possible in web-based or cross-platform TV apps
— faster transitions, richer animations during browsing, more responsive
filtering, and novel navigation models that we can explore freely in the desktop
10-foot context before committing to mobile TV implementations.

---

## Decisions: Locked

### D-10FT-001: Mode within ferrex-player, not a separate binary
- Activated via: command-line flag (`--10ft`), environment variable
  (`FERREX_10FT=1`), or in-app toggle in settings.
- Shares: all domain state, API client, caching, video pipeline, auth.
- Replaces: the UI layer only. The `domains/ui/` views switch to 10-foot
  variants while `domains/library/`, `domains/player/`, etc. remain identical.

### D-10FT-002: Controller-first input model
- All navigation must work with a standard gamepad (D-pad, A/B/X/Y, triggers,
  bumpers, sticks).
- Keyboard arrow keys + Enter/Escape as fallback.
- Mouse/touch is NOT the primary input — it may work but is not optimized for.

### D-10FT-003: Existing Iced focus system as foundation
- ferrex-player already has `common/focus.rs` and keyboard-driven navigation.
- 10-foot mode extends this into a full spatial focus engine:
  directional focus movement (up/down/left/right) across a 2D grid of focusable
  elements.

---

## Decisions: Open

### O-10FT-001: How to detect Steam Deck / 10-foot context
- Options:
  - Check for `SteamDeck` in `/sys/devices/virtual/dmi/id/board_name`
  - Check `$SteamGamepadUI` environment variable (set by Steam Big Picture)
  - Pure opt-in via `--10ft` flag
- May auto-detect and offer a prompt: "Controller detected. Switch to 10-foot mode?"

### O-10FT-002: Resolution and scaling strategy
- Steam Deck: 1280x800, relatively small for 10-foot UI.
- HTPC: 1920x1080 or 3840x2160 on a TV.
- Need a scaling system that works across both.
- Options: fixed set of presets, or dynamic scaling based on detected resolution
  and DPI.

### O-10FT-003: Video player overlay in 10-foot mode
- Desktop player has custom controls.
- 10-foot mode needs larger, simpler controls (big seek bar, big play/pause,
  chapter markers).
- Should these be additional wgpu shader widgets or standard Iced widgets
  scaled up?

### O-10FT-004: How deeply to integrate with Steam Input
- Steam Input allows remapping any controller to any action.
- Ferrex could ship a default Steam Input configuration that maps media
  controls (play/pause = A, back = B, seek = bumpers).
- Or: handle raw gamepad events directly via gilrs/SDL2.

---

## UI Structure (10-Foot Mode)

### Navigation Model

```
[Home]
  ├── Continue Watching (horizontal row, auto-focused)
  ├── Library 1: Movies (horizontal row of posters)
  ├── Library 2: TV Shows (horizontal row of posters)
  └── Library N...

[Library View] (entered by selecting a library row header or "See All")
  └── Full poster grid, vertical scrolling, horizontal wrapping

[Detail View] (entered by selecting a poster)
  ├── Backdrop + metadata
  ├── Play button (auto-focused)
  ├── Episodes list (for series)
  └── Related / recommendations (future)

[Player] (entered by activating Play)
  ├── Fullscreen video
  ├── Overlay controls (appear on any input, auto-hide after 3s)
  └── Back exits to Detail View

[Search] (triggered by dedicated button, e.g., Y on gamepad)
  ├── On-screen keyboard (controller-navigable)
  └── Results grid
```

### Focus Behavior

- **D-pad / left stick:** Spatial navigation. Focus moves in the pressed
  direction to the nearest focusable element.
- **A / Enter:** Activate (open detail, start playback, select item).
- **B / Escape:** Back (pop navigation stack).
- **Bumpers (L1/R1):** Page through library rows or jump between sections.
- **Triggers (L2/R2):** Seek forward/backward in player (10s / 30s increments).
- **Y:** Open search.
- **X:** Context menu (future: add to playlist, mark watched, etc.).
- **Start:** Settings / menu overlay.

### Visual Design Principles

- **Large text:** Minimum 24pt effective size at 10-foot viewing distance.
- **High contrast:** Poster titles readable over dark backgrounds.
- **Focus indicators:** Large, animated, high-contrast border or glow on the
  focused element. Must be instantly visible from across the room.
- **Reduced density:** Fewer items visible per screen compared to desktop mode.
  Posters are larger, spacing is generous.
- **Smooth scrolling:** Row scrolling and grid scrolling use the existing
  motion controller system (`domains/ui/motion_controller/`) with spring
  physics tuned for controller input (heavier damping, longer deceleration).

---

## Implementation Approach

### Phase 1: Navigation Shell
- New `domains/ui/views/tenfoot/` directory (or `10ft/`).
- Home screen with horizontal rows.
- Spatial focus engine extending `common/focus.rs`.
- Gamepad input handling (gilrs crate or direct evdev).
- Mode switch plumbing (shared state, different view tree).

### Phase 2: Library + Detail
- Full poster grid in 10-foot layout.
- Detail view with backdrop, metadata, episode list.
- Play button → existing video pipeline.

### Phase 3: Player Overlay
- Large, controller-friendly seek bar.
- Auto-hide/show on input.
- Seek with triggers.
- Subtitle/audio track selection (accessible via shoulder buttons).

### Phase 4: Polish
- Animations and transitions tuned for TV/controller feel.
- Steam Input default configuration.
- Auto-detection and mode switching.

---

## Relationship to Mobile TV

This spec exists because mobile TV interfaces (tvOS, Android TV) share the
same fundamental UX challenges:

- Controller/remote-first navigation
- 10-foot viewing distance assumptions
- Horizontal row browsing → grid → detail → player flow
- Large focus indicators, reduced density

The desktop 10-foot mode validates these patterns in the environment where
iteration is fastest (same codebase, same tooling, instant rebuild). Once the
UX is proven here, it informs:

- **tvOS:** SwiftUI with focus engine (maps to this spec's spatial focus model)
- **Android TV:** Compose for TV with D-pad handling (maps to this spec's
  gamepad navigation model)

Discoveries made during 10-foot mode development (what row height feels right,
how many posters per row, focus transition timing, seek increment UX) become
concrete design parameters for mobile TV specs.

---

## Related Specs

| Spec | Relationship |
|---|---|
| `mobile-apps-strategy.md` D-006 | TV deferred until this ships |
| `mobile-apps-ios.md` | tvOS section references this as UX source |
| `mobile-apps-android.md` | Android TV section references this as UX source |
