# Mobile Apps API Surface

> Defines the server API contract that mobile clients target.
> This is the shared interface — both iOS and Android consume the same endpoints
> with the same data shapes.

## Status

| Field | Value |
|---|---|
| Created | 2025-07-15 |
| Depends on | `mobile-apps-strategy.md`, `mobile-apps-wire-format.md` |

---

## Principle

The mobile API surface is NOT a new API. It is the existing ferrex-server
`/api/v1/*` route tree, consumed with a different wire format. Mobile clients
are peers to the desktop player.

Any endpoint the desktop player uses today is available to mobile. The v1 mobile
scope uses a **subset** of these endpoints. New endpoints are only added when
mobile has a genuinely different need (e.g., push notification token registration).

---

## Content Negotiation

Decided in `mobile-apps-wire-format.md`:

```
Accept: application/x-flatbuffers   →  FlatBuffers response
Accept: application/x-rkyv          →  rkyv response (desktop player)
Accept: application/json             →  JSON response (debug/tooling)
```

Request bodies use the same negotiation via `Content-Type`.

The server's serialization layer selects the encoder based on the `Accept`
header. If no recognized `Accept` is provided, the server returns JSON as
fallback (preserving backward compatibility with curl/browser debugging).

---

## v1 Endpoint Subset

These are the endpoints mobile v1 MUST support. Route constants reference
`ferrex-core/src/api/routes.rs`.

### Authentication

| Endpoint | Method | Purpose | Notes |
|---|---|---|---|
| `/api/v1/setup/status` | GET | Check if server needs initial setup | First-launch flow |
| `/api/v1/auth/register` | POST | Create account | Only if setup allows registration |
| `/api/v1/auth/login` | POST | Password login → access + refresh tokens | |
| `/api/v1/auth/refresh` | POST | Refresh expired access token | |
| `/api/v1/auth/logout` | POST | Invalidate session | |
| `/api/v1/auth/device/login` | POST | Device-based login | |
| `/api/v1/auth/device/pin` | POST | PIN login | |
| `/api/v1/users/me` | GET | Current user profile | |

### Libraries

| Endpoint | Method | Purpose | Notes |
|---|---|---|---|
| `/api/v1/libraries` | GET | List all libraries | Returns library metadata, not media |
| `/api/v1/libraries/{id}/movie-batches:sync` | POST | Sync movie batch versions | Client sends cached versions, server returns deltas |
| `/api/v1/libraries/{id}/movie-batches:fetch` | POST | Fetch specific movie batches | Returns full batch data for requested batch IDs |
| `/api/v1/libraries/{id}/movie-batches/{batch_id}` | GET | Single movie batch | Fallback for individual fetch |
| `/api/v1/libraries/{id}/series-bundles:sync` | POST | Sync series bundle versions | Same pattern as movie batches |
| `/api/v1/libraries/{id}/series-bundles:fetch` | POST | Fetch specific series bundles | |
| `/api/v1/libraries/{id}/indices/sorted` | GET | Sorted index for a library | Enables client-side sort without re-fetching all data |
| `/api/v1/libraries/{id}/indices/filter` | POST | Filtered index | Server-side filtering, returns matching indices |

### Media Details

| Endpoint | Method | Purpose | Notes |
|---|---|---|---|
| `/api/v1/media/query` | POST | Query media by criteria | Used for detail view data |

### Images

| Endpoint | Method | Purpose | Notes |
|---|---|---|---|
| `/api/v1/images/blob/{token}` | GET | Fetch image by content-addressed token | Immutable, cacheable forever |
| `/api/v1/images/manifest` | POST | Batch image readiness lookup | Returns which images are ready + their blob tokens |
| `/api/v1/images/events` | GET (SSE) | Image readiness notifications | Stream of newly-ready image events |

### Watch Progress

| Endpoint | Method | Purpose | Notes |
|---|---|---|---|
| `/api/v1/watch/progress` | POST | Update watch progress | Position, duration, timestamp |
| `/api/v1/watch/state` | GET | Full watch state for current user | All in-progress and completed items |
| `/api/v1/watch/continue` | GET | Continue watching list | Ordered by recency |
| `/api/v1/watch/series/{tmdb_series_id}` | GET | Series watch state | Per-season/episode completion |
| `/api/v1/watch/series/{tmdb_series_id}/next` | GET | Next episode to watch | |

### Search

| Endpoint | Method | Purpose | Notes |
|---|---|---|---|
| `/api/v1/media/query` | POST | Search with query string | Same endpoint as detail queries, different parameters |

### Streaming / Playback

| Endpoint | Method | Purpose | Notes |
|---|---|---|---|
| `/api/v1/stream/{id}` | GET | Stream media file | Direct play — server serves the file |
| `/api/v1/stream/{id}/ticket` | GET | Playback authorization ticket | Token-gated access for the stream URL |

### Real-Time Events

| Endpoint | Method | Purpose | Notes |
|---|---|---|---|
| `/api/v1/events/media` | GET (SSE) | Media library change events | New/updated/removed media notifications |
| `/api/v1/sync/ws` | WebSocket | Bidirectional sync channel | Watch progress sync, presence |

---

## Endpoints Explicitly NOT in v1 Mobile

These exist on the server but are not consumed by mobile v1:

- **Scan management:** `/api/v1/libraries/{id}/scans:*`, `/api/v1/scan/*`
- **Library CRUD:** POST/PUT/DELETE on `/api/v1/libraries/*`
- **Admin:** `/api/v1/admin/*`
- **User management:** `/api/v1/users` (collection), `/api/v1/admin/users/*`
- **Role management:** `/api/v1/roles/*`, `/api/v1/permissions/*`
- **Device trust management:** `/api/v1/auth/device/validate`, `revoke-trust`, `extend-trust`
- **Security settings:** `/api/v1/admin/security/*`
- **Demo mode:** `/api/v1/admin/demo/*`
- **Dev tools:** `/api/v1/admin/dev/*`

---

## Data Flow Patterns

### Library Loading (the critical path)

This is the most performance-sensitive flow. It must be fast enough to enable
60fps poster grid scrolling on first load.

```
Mobile Client                          ferrex-server
     │                                       │
     │  GET /libraries                       │
     │  Accept: application/x-flatbuffers    │
     │──────────────────────────────────────>│
     │                                       │
     │  [Library list: id, name, type, counts]│
     │<──────────────────────────────────────│
     │                                       │
     │  POST /libraries/{id}/movie-batches:sync
     │  Body: { cached_versions: [...] }     │
     │──────────────────────────────────────>│
     │                                       │
     │  [Delta: which batches are stale]     │
     │<──────────────────────────────────────│
     │                                       │
     │  POST /libraries/{id}/movie-batches:fetch
     │  Body: { batch_ids: [stale ones] }    │
     │──────────────────────────────────────>│
     │                                       │
     │  [Full batch data for requested IDs]  │
     │  (FlatBuffers — zero-copy on arrival) │
     │<──────────────────────────────────────│
     │                                       │
     │  (Same pattern for series-bundles)    │
```

On subsequent launches, the client sends its cached batch versions. If nothing
changed, the server returns an empty delta and the client displays from cache
instantly.

### Image Loading

```
Mobile Client                          ferrex-server
     │                                       │
     │  POST /images/manifest                │
     │  Body: [list of needed image keys]    │
     │──────────────────────────────────────>│
     │                                       │
     │  [Manifest: key → blob_token, or PENDING]
     │<──────────────────────────────────────│
     │                                       │
     │  GET /images/blob/{token}             │
     │  (for each ready image)               │
     │──────────────────────────────────────>│
     │                                       │
     │  [Raw image bytes, Cache-Control: immutable]
     │<──────────────────────────────────────│
```

Initial v1 simplification: skip the manifest step, construct blob URLs from
known image tokens embedded in the media data, and let Nuke/Coil handle caching.
The manifest-aware prefetch layer is added later.

### Watch Progress Sync

```
Mobile Client                          ferrex-server
     │                                       │
     │  (During playback, every ~10s)        │
     │  POST /watch/progress                 │
     │  Body: { media_id, position, duration }│
     │──────────────────────────────────────>│
     │                                       │
     │  (On app launch)                      │
     │  GET /watch/state                     │
     │──────────────────────────────────────>│
     │                                       │
     │  [Full watch state for user]          │
     │<──────────────────────────────────────│
```

---

## Server-Side Changes Required

### Mandatory for v1

1. **FlatBuffers serialization layer.** The server must be able to serialize
   responses in FlatBuffers format based on `Accept` header negotiation.
   - Affects: response serialization in handlers, likely via an Axum extractor
     or middleware.
   - Does NOT affect: business logic, database queries, or existing rkyv path.

2. **FlatBuffers request deserialization.** For POST endpoints where the client
   sends structured data (sync requests, progress updates, etc.).

### Desirable for v1

3. **Streaming endpoint compatibility check.** Verify that `/api/v1/stream/{id}`
   works correctly with AVPlayer's HTTP range request pattern and ExoPlayer's
   adaptive loading. May need `Accept-Ranges`, `Content-Range`, and proper
   `Content-Type` headers for video files.

4. **CORS/connectivity from mobile simulators.** Ensure the server is reachable
   from iOS Simulator and Android Emulator networking contexts (typically
   localhost mapping or LAN IP).

### Not Required for v1

5. Push notification token registration endpoint.
6. Download/offline token endpoint.
7. Mobile-specific analytics or telemetry endpoints.

---

## Open Questions

### OQ-001: API versioning strategy for mobile
- Desktop player and server are versioned together in the monorepo.
- Mobile apps will have independent release cycles (App Store/Play Store).
- How do we handle API version compatibility? Options:
  - Server maintains backward compatibility within `/api/v1/`.
  - Mobile client sends its version in a header; server adapts.
  - Version the FlatBuffers schemas independently from the API version.

### OQ-002: Rate limiting for mobile clients
- The server has rate limiting infrastructure (Redis-based).
- Mobile clients on cellular may have different IP characteristics than
  desktop clients on LAN.
- Should mobile have different rate limit tiers?

### OQ-003: Batch size tuning for mobile
- Movie batches and series bundles were sized for desktop (fast LAN, large RAM).
- Mobile may benefit from smaller batch sizes or paginated fetching.
- Open: profile on real devices and tune.
