# Mobile Apps Strategy

> Source-of-truth spec for the Ferrex mobile initiative.
> Covers high-level goals, locked decisions, scope boundaries, and phasing.

## Status

| Field | Value |
|---|---|
| Created | 2025-07-15 |
| Branch | `feat/mobile-apps` |
| Phase | Specification |

---

## Vision

Deliver first-class native mobile apps (iOS, Android) that provide the same
philosophy as ferrex-player on desktop: **performance as a feature**, native
platform integration, and a fluid browsing-to-playback experience. Mobile apps
are peers to the desktop player — they talk to the same ferrex-server over the
same API, not a dumbed-down subset.

TV interfaces (tvOS, Android TV) are **deferred** until ferrex-player ships its
own 10-foot mode as a reference implementation (see `ferrex-player-10ft.md`).

---

## Decisions: Locked

These decisions are final and should not be revisited without a new spec.

### D-001: Pure native UI per platform
- **iOS:** SwiftUI (UIKit bridging only where SwiftUI gaps demand it)
- **Android:** Jetpack Compose (View-system bridging only where Compose gaps demand it)
- **Rationale:** Ferrex exists because generic UI frameworks aren't fast enough.
  The same principle applies on mobile. Native UI is non-negotiable for the
  poster grid scrolling performance and platform-correct feel that define the
  project.

### D-002: Native video players
- **iOS:** AVPlayer / AVKit
- **Android:** Media3 ExoPlayer
- **Rationale:** Each has thousands of engineer-years behind it — HDR metadata,
  adaptive streaming, PiP, AirPlay/Cast, audio routing, background playback.
  Building on anything else would be years of catch-up for a worse result.

### D-003: Monorepo, mobile/ subdirectory
- All mobile code lives under `mobile/` at the workspace root.
- Structure: `mobile/ios/`, `mobile/android/`, `mobile/shared/` (for schema
  definitions, codegen tooling, shared test fixtures).
- CI and code ownership are scoped to `mobile/**` paths.
- **Rationale:** Keeps API spec, schema definitions, and server code in the same
  repo. Changes to server endpoints or data models that affect mobile are
  visible in the same PR.

### D-004: No JSON wire format
- Mobile clients will NOT use JSON for server communication.
- A binary serialization format with cross-language code generation will be used.
- The specific format is covered in `mobile-apps-wire-format.md`.
- **Rationale:** rkyv's zero-copy performance is core to ferrex-player's
  identity. Mobile must not regress to the lowest-common-denominator format.
  JSON is neither compact nor fast — it's just common.

### D-005: Server is unchanged (API-first)
- Mobile apps are new clients against the existing ferrex-server API.
- The server MAY gain new content-negotiation or dedicated endpoints for the
  chosen wire format, but the existing REST/WS route structure is the contract.
- Mobile does NOT get a separate backend or BFF (backend-for-frontend).

### D-006: TV interfaces deferred
- tvOS and Android TV apps will NOT be built until ferrex-player has a working
  10-foot mode (see `ferrex-player-10ft.md`).
- The desktop 10-foot mode serves as the UX reference implementation.
- Mobile TV apps will share their respective platform's core modules (API
  client, auth, data layer) but will have dedicated UI targets.

### D-007: Parallel platform development
- iOS and Android development proceed in parallel, not sequenced.
- Each platform will surface different challenges/blockers; cross-pollination
  of discoveries is expected and encouraged.
- **Rationale:** Code is cheap in the age of agentic models. Decisions about
  what to build are expensive. Parallel development maximizes learning velocity.

---

## Decisions: Open

These decisions need to be resolved during implementation. They are documented
here to prevent accidental lock-in.

### O-001: Wire format (FlatBuffers vs. Protobuf vs. others)
- See `mobile-apps-wire-format.md` for research and analysis.
- Decision affects: schema definition workflow, codegen tooling, server
  serialization path, and whether desktop player also migrates off rkyv.

### O-002: Image loading strategy details
- Initial approach: direct blob URL loading via Nuke (iOS) / Coil (Android).
- Deferred: manifest-aware prefetch layer (matching desktop behavior).
- Open question: when does the prefetch layer become necessary? What library
  size triggers it?

### O-003: Auth consolidation scope
- Existing desktop auth is "sophisticated in code, limited in practice."
- Mobile needs: device trust, session management, password + PIN flows.
- Open: how much server-side auth refactoring happens in this initiative vs.
  a separate auth-focused effort? Mobile-specific auth (biometric unlock,
  Keychain/Keystore secure storage) is deferred past v1.

### O-004: Offline capability
- v1: online-only (requires server connectivity).
- Open: when does offline library cache / download-to-device become a priority?
  This requires server-side download token API and significant client-side
  storage management.

### O-005: CI/CD pipeline for mobile
- Needs: Xcode Cloud or GitHub Actions with macOS runners for iOS. Standard
  GitHub Actions for Android.
- Open: TestFlight / Play Store internal testing distribution automation.

---

## v1 Scope

### Must Have (ship-blocking)
- **Library browsing**: poster grid with smooth scrolling, sorting, filtering.
  Must feel as responsive as ferrex-player on a mid-range phone.
- **Video playback**: direct play with full hardware decode. Progress tracking
  synced to server. Seek, pause, resume. Resume-from-position on re-launch.
- **Search**: fast, debounced, results update as you type.
- **Basic auth**: connect to server URL, register/login, session persistence.
- **Server connection**: manual server URL entry, connection status indicator.

### Should Have (v1 if time permits)
- Movie and series detail views (cast, overview, seasons/episodes).
- Watch progress "continue watching" section on home screen.
- Sort and filter controls matching desktop (genre, year, resolution, watch status).

### Deferred (explicitly not v1)
- Scan management / admin controls
- Library creation / management
- User administration
- Settings beyond server URL and basic preferences
- HLS transcoding (direct play only for v1)
- Offline / download support
- TV interfaces (see D-006)
- Push notifications
- Chromecast / AirPlay integration
- Picture-in-Picture
- Widgets

---

## Quality Bar

This is the most important section of this spec.

v1 mobile apps will NOT ship with "it works but it's rough" quality. The
explicit standard:

- **Poster grid scrolling**: 60fps on a 3-year-old mid-range device. No jank,
  no placeholder flicker on scroll-back. Image loading must be progressive and
  non-blocking.
- **Video playback start**: < 2 seconds from tap to first frame on local
  network. Seek must be frame-accurate and responsive.
- **Search**: results update within 100ms of keystroke on local network.
- **Navigation**: screen transitions must be animated and immediate. No loading
  spinners for cached data.

If these can't be met, the feature doesn't ship — it gets fixed first.

---

## Related Specs

| Spec | Scope |
|---|---|
| `mobile-apps-wire-format.md` | Wire format research, decision, and migration path |
| `mobile-apps-api-surface.md` | API contract: endpoints, auth flow, data shapes |
| `mobile-apps-ios.md` | iOS-specific architecture, dependencies, platform integration |
| `mobile-apps-android.md` | Android-specific architecture, dependencies, platform integration |
| `ferrex-player-10ft.md` | Desktop 10-foot mode (Steam Deck / Big Picture reference) |
