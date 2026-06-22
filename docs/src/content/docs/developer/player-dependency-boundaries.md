---
title: "Player dependency boundaries"
description: "Allowed dependency direction, compatibility shims, and guard commands for the decomposed Ferrex desktop player crates."
sidebar:
  order: 3
---

> **Drift-prevention:** This Starlight page is the canonical docs-site version. The legacy `docs/*.md` path now points here instead of carrying a second copy.

Ferrex’s desktop player is split into a small app/facade layer, a UI/runtime
layer, data-domain crates, an API adapter crate, and dependency-light foundation
contracts. Imports must stay acyclic and point from product/runtime code toward
stable lower layers.

## Final crate graph

```text
ferrex-player (binary/facade)
  -> ferrex-player-app (desktop runtime shell)
    -> ferrex-player-ui (Iced views, widgets, subscriptions, presentation adapters)
      -> ferrex-player-auth
      -> ferrex-player-repository
      -> ferrex-player-library
      -> ferrex-player-media
      -> ferrex-player-metadata
      -> ferrex-player-playback (Iced/subwave playback runtime)
      -> ferrex-player-search
      -> ferrex-player-settings
      -> ferrex-player-user-admin
      -> ferrex-player-api
        -> ferrex-player-foundation
```

Supporting crates may depend on `ferrex-player-foundation` and, when they own a
wire/data contract, on `ferrex-core`, `ferrex-model`, or `ferrex-contracts`.
Only `ferrex-player-app`, `ferrex-player-ui`, and the extracted
`ferrex-player-playback` video domain may depend on Iced widgets, Iced runtime
tasks/subscriptions, subwave, or concrete desktop runtime/video presentation
code. `ferrex-player-settings` may use `iced_core` color/point DTOs for shared
accent-color state, but it must not import Iced widgets, tasks, subscriptions,
or subwave runtime crates.

`ferrex-player-foundation` is the bottom layer. It contains primitives that are
independent of UI frameworks, video backends, and Ferrex domain models:
repository result/error types, unit helpers, stable auth/setup policy DTOs,
generic update/event containers, and dependency-light domain tasks that the UI
shell translates into concrete runtime tasks.

## Crate rules

| Crate/layer | May depend on | Must not depend on |
| --- | --- | --- |
| `ferrex-player-foundation` | `std`, small ecosystem support crates (`serde`, `thiserror`) | `ferrex-player*`, `iced*`, `iced_aw`, `lucide-icons`, `subwave_*`, `ferrex-core`, `ferrex-model`, `ferrex-contracts`, server/domain crates |
| `ferrex-player-api` | `ferrex-player-foundation`, schema/model crates, HTTP/runtime support | `ferrex-player`, `ferrex-player-app`, `ferrex-player-ui`, Iced, subwave |
| `ferrex-player-auth` | API/foundation crates plus auth storage/crypto support | Iced widgets/tasks/subscriptions, subwave, app/UI modules |
| `ferrex-player-repository` | Foundation/core/model crates plus repository snapshots and disk cache support | Iced widgets/tasks/subscriptions, subwave, app/UI modules |
| `ferrex-player-library` | API/foundation/repository crates, library state machine, SSE stream builders | Iced subscriptions/tasks/widgets, subwave, app/UI modules |
| `ferrex-player-media` | API/foundation/library crates and watch-state selectors | Iced tasks/subscriptions/widgets, subwave, app/UI modules |
| `ferrex-player-metadata` | Metadata-domain contracts that are independent of UI image handles | Iced image handles/tasks/subscriptions, subwave, app/UI modules |
| `ferrex-player-playback` | API/foundation crates plus Iced/subwave playback runtime and overlay helpers | App bootstrap, root state composition, or UI shell modules |
| `ferrex-player-search` | API/foundation/library crates and search data-domain logic | Iced event/key types, tasks/subscriptions/widgets, subwave, app/UI modules |
| `ferrex-player-settings` | Foundation/domain state, settings validation, section reducers, color utilities, `iced_core` color/point DTOs | Iced widgets/tasks/subscriptions, subwave, app/UI modules |
| `ferrex-player-user-admin` | User-admin state, sanitized messages, reducer helpers | Iced widgets/tasks/subscriptions, subwave, app/UI modules |
| `ferrex-player-ui` | Supporting player data/API crates, Iced/subwave, presentation adapters | App bootstrap/daemon construction; lower crates must not import UI/shell code |
| `ferrex-player-app` | `ferrex-player-ui`, extracted player crates, runtime-only shell dependencies (`iced`, logging/profiling hooks) | New reusable domain primitives that belong in lower crates |
| `ferrex-player` binary/facade | `ferrex-player-app` only for runtime code, plus dev-only test helpers as needed | New shell composition, shared primitives, or UI implementation modules |

## Intentional compatibility shims

Existing `ferrex-player` import paths remain available while downstream code
migrates to the extracted crates. The package re-exports `ferrex-player-app`,
which exposes the app shell and compatibility re-exports for UI/domain surfaces.
These lower-level compatibility shims remain intentional:

- `ferrex-player-ui::infra::repository::*` re-exports
  `ferrex_player_repository::*`; repository result/error types continue to come
  from `ferrex_player_foundation::repository` through that crate.
- `ferrex-player-ui::infra::units::ByteSize` re-exports
  `ferrex_player_foundation::units::ByteSize`.
- `ferrex-player-ui::domains::auth::pin_policy::*` re-exports the shared PIN
  helpers.
- `ferrex-player-ui::domains::auth::manager::{PinPolicyResponse,
  DeviceTrustPolicyResponse, DeviceAuthStatus}` and
  `ferrex-player-ui::infra::api_client::SetupStatus` re-export shared auth DTOs.
- `ferrex-player-ui::domains::library::messages` re-exports library messages but
  owns the Iced subscription wrappers around dependency-light library streams.

Runtime bootstrap/config, root update/view/subscription composition, Iced daemon
construction, presets, and logger/profiling hooks live under `ferrex-player-app`.
UI-bound auth handlers, the unified image service, Iced image handles, and image
loading subscriptions live under `ferrex-player-ui`; playback/video runtime code
lives under `ferrex-player-playback`. Do not add `#[path = ...]` includes from
lower crates or move Iced widget/image-handle code back into
`ferrex-player-auth`, `ferrex-player-repository`, `ferrex-player-library`,
`ferrex-player-media`, `ferrex-player-metadata`, `ferrex-player-search`,
`ferrex-player-settings`, or `ferrex-player-user-admin`.

When new player crates are created, import from the owning supporting crate
(e.g. `ferrex-player-app` for app-shell wiring, `ferrex-player-ui` for
presentation code, or `ferrex-player-foundation` for foundation primitives)
instead of adding new users of temporary facade shims.

## Guard

Run the lightweight boundary guard after dependency changes:

```bash
./scripts/check-player-crate-boundaries.sh
```

The guard fails if non-UI player crates gain direct source imports, direct
manifest dependencies, or normal transitive dependency edges to Iced/subwave UI
runtime crates. For `ferrex-player-settings`, it permits only the documented
`iced_core` color/point DTO dependency and rejects all other Iced, subwave, or
app/UI-layer edges. It also keeps lower crates from depending upward on the
`ferrex-player` facade, `ferrex-player-app`, or `ferrex-player-ui`, and keeps
`ferrex-player-foundation` independent of Ferrex core/model/contracts.

The older command remains as a compatibility wrapper:

```bash
./scripts/check-player-foundation-boundaries.sh
```
