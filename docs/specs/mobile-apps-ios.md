# Mobile Apps — iOS

> iOS-specific architecture, dependencies, and platform integration decisions.

## Status

| Field | Value |
|---|---|
| Created | 2025-07-15 |
| Depends on | `mobile-apps-strategy.md`, `mobile-apps-wire-format.md`, `mobile-apps-api-surface.md` |
| Target | iOS 17+ (SwiftUI lifecycle, Observation framework, async/await mature) |
| Dev environment | M4 MacBook Pro, Xcode, iOS Simulator (no physical device required for development) |

---

## Development Environment

### Confirmed Viable
- **Xcode + iOS Simulator on Apple Silicon:** The simulator runs app code as a
  native ARM process (not a VM). Full SwiftUI previews, AVPlayer testing, network
  debugging, and performance profiling are available.
- **No physical iOS device required** for v1 development. Simulator covers all
  v1 features (networking, video playback, UI rendering).
- **TestFlight** for distributing builds to testers with physical devices when
  ready for real-world validation.

### When a Physical Device Becomes Necessary
- Real-world performance profiling (thermal throttling, actual GPU behavior)
- Push notifications (Xcode can simulate basic pushes, but APNs requires device)
- Camera/sensors (not in v1 scope)
- Cellular network testing

---

## Project Structure

```
mobile/ios/
├── Ferrex.xcodeproj          # Or Ferrex.xcworkspace if SPM workspaces are used
├── Ferrex/                    # Main iOS app target
│   ├── App/
│   │   ├── FerrexApp.swift          # @main entry point
│   │   └── AppDelegate.swift        # UIKit lifecycle hooks if needed
│   ├── Core/                         # Business logic (no UI imports)
│   │   ├── API/
│   │   │   ├── FerrexAPIClient.swift       # HTTP client wrapping URLSession
│   │   │   ├── ContentNegotiation.swift    # Accept header, FlatBuffers encoding
│   │   │   └── Generated/                  # flatc-generated Swift types
│   │   ├── Auth/
│   │   │   ├── AuthManager.swift           # Token lifecycle, refresh, persistence
│   │   │   ├── KeychainStorage.swift       # Secure token storage
│   │   │   └── SessionState.swift          # Observable auth state
│   │   ├── Library/
│   │   │   ├── LibraryRepository.swift     # Batch sync, caching, sorted indices
│   │   │   ├── LibraryCache.swift          # Disk-backed FlatBuffer cache
│   │   │   └── MediaAccessor.swift         # Zero-copy field access from cached buffers
│   │   ├── Media/
│   │   │   └── WatchProgressTracker.swift  # Background progress reporting
│   │   ├── Search/
│   │   │   └── SearchService.swift         # Debounced search with cancellation
│   │   └── Image/
│   │       ├── ImagePipeline.swift         # Coordinates loading, uses Nuke under the hood
│   │       └── BlobURLBuilder.swift        # Constructs /images/blob/{token} URLs
│   ├── UI/
│   │   ├── Library/
│   │   │   ├── LibraryGridView.swift       # Poster grid (LazyVGrid)
│   │   │   ├── PosterCard.swift            # Individual poster with loading states
│   │   │   ├── SortFilterBar.swift         # Sort/filter controls
│   │   │   └── LibraryViewModel.swift      # Drives grid state
│   │   ├── Detail/
│   │   │   ├── MovieDetailView.swift
│   │   │   ├── SeriesDetailView.swift
│   │   │   ├── SeasonView.swift
│   │   │   └── DetailViewModel.swift
│   │   ├── Player/
│   │   │   ├── PlayerView.swift            # AVPlayerViewController wrapper
│   │   │   ├── PlayerControls.swift        # Custom overlay controls
│   │   │   └── PlayerViewModel.swift       # Playback state, progress sync
│   │   ├── Search/
│   │   │   ├── SearchView.swift
│   │   │   └── SearchViewModel.swift
│   │   ├── Auth/
│   │   │   ├── ServerConnectView.swift     # Manual server URL entry
│   │   │   ├── LoginView.swift
│   │   │   └── AuthViewModel.swift
│   │   ├── Home/
│   │   │   ├── HomeView.swift              # Continue watching + library tabs
│   │   │   └── HomeViewModel.swift
│   │   └── Components/
│   │       ├── AsyncPosterImage.swift      # Image loading with placeholder
│   │       └── LoadingState.swift          # Shared loading/error/empty states
│   └── Resources/
│       ├── Assets.xcassets
│       └── Info.plist
├── FerrexTests/
│   ├── Core/
│   │   ├── APIClientTests.swift
│   │   ├── AuthManagerTests.swift
│   │   └── LibraryRepositoryTests.swift
│   └── UI/
│       └── LibraryGridSnapshotTests.swift
└── FerrexUITests/
    └── LibraryBrowsingTests.swift
```

---

## Dependencies

### Decided

| Dependency | Purpose | Rationale |
|---|---|---|
| **URLSession** (system) | HTTP networking | No third-party networking library. URLSession is async/await native on iOS 15+, supports HTTP/2, streaming, background transfers. Keeps dependency count minimal. |
| **FlatBuffers** (Google, SPM) | Wire format deserialization | Generated Swift types for zero-copy access. See `mobile-apps-wire-format.md`. |
| **Nuke** (SPM) | Image loading + caching | Mature, async/await native, progressive loading, disk caching, memory caching with cost limits. Actively maintained, widely adopted. |
| **AVKit / AVFoundation** (system) | Video playback | System framework. No alternative considered. |
| **Security** (system, Keychain) | Token storage | System Keychain via Security framework. No third-party keychain wrapper initially. |

### Explicitly NOT Using

| Dependency | Why not |
|---|---|
| Alamofire | URLSession is sufficient. Extra abstraction layer adds no value for our use case. |
| SwiftUI Navigation libraries (TCA, etc.) | NavigationStack is sufficient for v1. Avoid architectural framework lock-in. |
| Realm / CoreData / SwiftData | FlatBuffers cached to disk IS the persistence layer. No ORM needed. |
| Combine | Using structured concurrency (async/await) and Observation framework instead. |

---

## Key Technical Decisions

### State Management: Observation Framework

iOS 17's `@Observable` macro replaces `ObservableObject` + `@Published`. It's
simpler, more performant (fine-grained change tracking), and maps cleanly to the
desktop player's domain-based state model.

```swift
@Observable
final class LibraryViewModel {
    var libraries: [Library] = []
    var selectedLibrary: Library?
    var sortOrder: SortOrder = .title
    var isLoading = false
    
    // Backed by zero-copy FlatBuffer cache
    private let repository: LibraryRepository
}
```

### Data Layer: FlatBuffers as Cache

The critical insight from the desktop player is that the wire format IS the
cache format. FlatBuffers enables the same pattern:

1. Receive batch response bytes from server.
2. Validate the FlatBuffer (one-time verification pass).
3. Write raw bytes to disk cache (keyed by library_id + batch_id + version).
4. On next launch: read bytes from disk → access fields directly. No deserialization.

This means `LibraryCache` is essentially a content-addressed byte store, not
a database. Swift's `FileManager` + memory-mapped file access (`Data(contentsOf:options:.mappedIfSafe)`) provides the mechanism.

### Video Playback: AVPlayer

```swift
// Simplified flow
let ticket = try await api.getPlaybackTicket(mediaId: id)
let url = api.streamURL(mediaId: id, ticket: ticket)
let player = AVPlayer(url: url)

// Progress tracking
let interval = CMTime(seconds: 10, preferredTimescale: 1)
player.addPeriodicTimeObserver(forInterval: interval, queue: .main) { time in
    Task {
        try await api.updateProgress(mediaId: id, position: time.seconds, duration: duration)
    }
}
```

Key requirements for the server's `/stream/{id}` endpoint:
- Must support HTTP range requests (`Accept-Ranges: bytes`)
- Must return correct `Content-Type` for the media container
- Must handle AVPlayer's characteristic request pattern (initial small range,
  then larger sequential reads)

### Image Loading: Nuke with Custom Pipeline

```swift
// Poster images use content-addressed blob URLs — immutable, cache forever
let request = ImageRequest(
    url: api.imageBlobURL(token: movie.posterToken),
    processors: [.resize(width: posterWidth)],
    priority: .high,
    options: [.returnCacheDataDontLoad] // Use cache if available, don't re-fetch
)
```

Nuke's disk cache respects `Cache-Control: immutable` headers. Since blob URLs
are content-addressed, a cached image never goes stale — matching the server's
design.

---

## Performance Targets

These are non-negotiable per `mobile-apps-strategy.md`:

| Metric | Target | How measured |
|---|---|---|
| Grid scroll FPS | 60fps on iPhone 12 (A14, 2020) | Instruments → Core Animation FPS |
| Time to first poster | < 500ms from library selection (warm cache) | Custom trace |
| Video start latency | < 2s tap-to-first-frame on LAN | Custom trace |
| Search response | < 100ms keystroke-to-results on LAN | Custom trace |
| Memory (1000 movie library) | < 100MB resident | Instruments → Memory |
| Cold launch to browsable | < 3s with warm disk cache | Custom trace |

### How to achieve grid performance

The desktop player achieves its grid performance through custom wgpu shaders
that batch-render posters. On iOS, the path is different but the principle is
the same — minimize per-frame work:

1. **LazyVGrid** handles view recycling (iOS's equivalent of viewport culling).
2. **Nuke** handles async image loading with memory/disk cache tiers.
3. **FlatBuffer zero-copy** means no deserialization cost when scrolling through
   data — field access is a pointer offset, same as rkyv on desktop.
4. **Prefetch API** (`ScrollView` with `.onAppear` / `List` prefetching) triggers
   image loads ahead of scroll position.
5. If `LazyVGrid` proves insufficient, drop to `UICollectionView` with
   `UICollectionViewCompositionalLayout` via `UIViewControllerRepresentable`.

---

## Open Questions (iOS-specific)

### OQ-iOS-001: Minimum iOS version
- Recommended: iOS 17 (Observation framework, improved SwiftUI navigation).
- Alternative: iOS 16 (wider device reach, but requires `ObservableObject` instead of `@Observable`).
- Trade-off: iOS 17 adoption is ~85%+ as of mid-2025.

### OQ-iOS-002: Swift Package Manager vs. CocoaPods/Carthage
- Recommendation: SPM exclusively. It's Apple's first-party dependency manager,
  FlatBuffers and Nuke both support it.
- No reason to introduce CocoaPods/Carthage complexity.

### OQ-iOS-003: App architecture pattern
- Recommendation: MVVM with Observation framework. Simple, idiomatic SwiftUI.
- NOT using TCA (The Composable Architecture) — too much ceremony for v1.
- If needed later, the Core/ layer is already separated from UI/, making
  architectural migration feasible.

---

## tvOS Notes (Deferred)

Per `mobile-apps-strategy.md` D-006, tvOS is deferred. When it arrives:

- `Ferrex/Core/` is shared entirely (API client, auth, library, caching).
- `Ferrex/UI/` is NOT shared — tvOS needs focus-engine-aware views.
- A new `FerrexTV/` target is added to the Xcode project.
- The desktop 10-foot mode (`ferrex-player-10ft.md`) provides UX reference.
