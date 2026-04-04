# Mobile Apps — Android

> Android-specific architecture, dependencies, and platform integration decisions.

## Status

| Field | Value |
|---|---|
| Created | 2025-07-15 |
| Depends on | `mobile-apps-strategy.md`, `mobile-apps-wire-format.md`, `mobile-apps-api-surface.md` |
| Target | Android API 28+ (Android 9, Pie) — covers ~95% of active devices |
| Language | Kotlin (no Java) |
| Build | Gradle with Kotlin DSL, version catalog |

---

## Development Environment

### Requirements
- **Android Studio** (latest stable, Ladybug or newer)
- **JDK 17+** (bundled with Android Studio)
- **Android Emulator** (ARM64 images on Apple Silicon, x86_64 on Intel)
- Physical Android device recommended but not required for v1 development

### Emulator Notes
- On Apple Silicon (M4 MacBook Pro): use ARM64 system images. Performance is
  excellent — no x86 translation layer.
- For performance profiling: physical device is preferred (emulator GPU
  behavior differs from real hardware).

---

## Project Structure

```
mobile/android/
├── build.gradle.kts                    # Root build file
├── settings.gradle.kts                 # Module declarations, version catalog
├── gradle/
│   └── libs.versions.toml              # Version catalog
├── app/                                 # Main application module
│   ├── build.gradle.kts
│   └── src/
│       ├── main/
│       │   ├── AndroidManifest.xml
│       │   ├── kotlin/com/ferrex/android/
│       │   │   ├── FerrexApplication.kt         # Application subclass, DI setup
│       │   │   ├── MainActivity.kt              # Single-activity Compose host
│       │   │   ├── navigation/
│       │   │   │   ├── FerrexNavGraph.kt        # Navigation graph definition
│       │   │   │   └── Routes.kt                # Sealed class route definitions
│       │   │   ├── core/
│       │   │   │   ├── api/
│       │   │   │   │   ├── FerrexApiClient.kt         # OkHttp + FlatBuffers
│       │   │   │   │   ├── ContentNegotiation.kt      # Accept header handling
│       │   │   │   │   ├── AuthInterceptor.kt         # Token injection interceptor
│       │   │   │   │   └── generated/                  # flatc-generated Kotlin types
│       │   │   │   ├── auth/
│       │   │   │   │   ├── AuthManager.kt             # Token lifecycle, refresh
│       │   │   │   │   ├── EncryptedStorage.kt        # EncryptedSharedPreferences
│       │   │   │   │   └── SessionState.kt            # StateFlow-based auth state
│       │   │   │   ├── library/
│       │   │   │   │   ├── LibraryRepository.kt       # Batch sync, caching
│       │   │   │   │   ├── LibraryCache.kt            # Disk-backed FlatBuffer cache
│       │   │   │   │   └── MediaAccessor.kt           # Zero-copy field access
│       │   │   │   ├── media/
│       │   │   │   │   └── WatchProgressTracker.kt    # Background progress reporting
│       │   │   │   ├── search/
│       │   │   │   │   └── SearchService.kt           # Debounced search
│       │   │   │   └── image/
│       │   │   │       ├── ImagePipeline.kt           # Coil integration
│       │   │   │       └── BlobUrlBuilder.kt          # /images/blob/{token} URLs
│       │   │   └── ui/
│       │   │       ├── library/
│       │   │       │   ├── LibraryGridScreen.kt       # Poster grid (LazyVerticalGrid)
│       │   │       │   ├── PosterCard.kt              # Individual poster composable
│       │   │       │   ├── SortFilterBar.kt
│       │   │       │   └── LibraryViewModel.kt
│       │   │       ├── detail/
│       │   │       │   ├── MovieDetailScreen.kt
│       │   │       │   ├── SeriesDetailScreen.kt
│       │   │       │   ├── SeasonScreen.kt
│       │   │       │   └── DetailViewModel.kt
│       │   │       ├── player/
│       │   │       │   ├── PlayerScreen.kt            # ExoPlayer Compose wrapper
│       │   │       │   ├── PlayerControls.kt          # Custom overlay
│       │   │       │   └── PlayerViewModel.kt
│       │   │       ├── search/
│       │   │       │   ├── SearchScreen.kt
│       │   │       │   └── SearchViewModel.kt
│       │   │       ├── auth/
│       │   │       │   ├── ServerConnectScreen.kt
│       │   │       │   ├── LoginScreen.kt
│       │   │       │   └── AuthViewModel.kt
│       │   │       ├── home/
│       │   │       │   ├── HomeScreen.kt
│       │   │       │   └── HomeViewModel.kt
│       │   │       └── components/
│       │   │           ├── AsyncPosterImage.kt
│       │   │           └── LoadingState.kt
│       │   └── res/
│       │       ├── values/
│       │       └── drawable/
│       └── test/                         # Unit tests
│           └── kotlin/com/ferrex/android/
│               ├── core/
│               │   ├── ApiClientTest.kt
│               │   ├── AuthManagerTest.kt
│               │   └── LibraryRepositoryTest.kt
│               └── ui/
└── tv/                                   # Android TV module (DEFERRED)
    └── ... (future)
```

---

## Dependencies

### Decided

| Dependency | Purpose | Rationale |
|---|---|---|
| **OkHttp** | HTTP client | Industry standard for Android. Interceptor chain for auth. HTTP/2 support. |
| **FlatBuffers** (Google, Gradle) | Wire format | Generated Kotlin types for zero-copy access. See `mobile-apps-wire-format.md`. |
| **Coil** (Compose) | Image loading + caching | Compose-native, Kotlin coroutines-based, disk/memory caching. The idiomatic choice for Compose. |
| **Media3 ExoPlayer** | Video playback | Google's official media player. Adaptive streaming, HDR support, subtitle rendering, Cast extension. |
| **Hilt** | Dependency injection | Standard for Android. Provides `@HiltViewModel`, scoped component lifecycle. |
| **Jetpack Navigation (Compose)** | Screen navigation | Type-safe navigation with Compose integration. |
| **EncryptedSharedPreferences** (Jetpack Security) | Token storage | Hardware-backed keystore encryption for auth tokens. |
| **Kotlin Coroutines + Flow** | Async / reactive state | Standard Kotlin concurrency. `StateFlow` for UI state, `Flow` for streams. |

### Explicitly NOT Using

| Dependency | Why not |
|---|---|
| Retrofit | OkHttp is sufficient when using FlatBuffers (Retrofit's value is in JSON/Protobuf converters). |
| Room / SQLite | FlatBuffers cached to disk IS the persistence layer. No ORM needed. |
| RxJava | Kotlin coroutines/Flow are the standard. RxJava adds no value for new Compose apps. |
| Ktor (client) | OkHttp is more mature on Android, better interceptor ecosystem. |
| Moshi / Gson / kotlinx.serialization | Not using JSON. FlatBuffers codegen handles serialization. |

---

## Key Technical Decisions

### State Management: ViewModel + StateFlow

```kotlin
@HiltViewModel
class LibraryViewModel @Inject constructor(
    private val repository: LibraryRepository,
) : ViewModel() {
    
    private val _uiState = MutableStateFlow(LibraryUiState())
    val uiState: StateFlow<LibraryUiState> = _uiState.asStateFlow()
    
    fun selectLibrary(libraryId: LibraryId) {
        viewModelScope.launch {
            _uiState.update { it.copy(isLoading = true) }
            val batches = repository.syncAndFetch(libraryId)
            _uiState.update { it.copy(
                isLoading = false,
                mediaAccessor = batches, // Zero-copy FlatBuffer accessor
            )}
        }
    }
}
```

### Data Layer: FlatBuffers as Cache

Same principle as iOS — the wire format is the cache format:

1. Receive batch response bytes from server via OkHttp.
2. Validate the FlatBuffer.
3. Write raw `ByteArray` to disk cache (app internal storage, keyed by library + batch + version).
4. On next launch: memory-map the file (`FileChannel.map(MapMode.READ_ONLY)`) → access fields directly.

Kotlin/JVM's `java.nio.MappedByteBuffer` provides zero-copy file access, and
FlatBuffers' `ByteBuffer`-based API integrates directly with it.

```kotlin
class LibraryCache(private val cacheDir: File) {
    
    fun getCachedBatch(libraryId: LibraryId, batchId: MovieBatchId): ByteBuffer? {
        val file = cacheFile(libraryId, batchId)
        if (!file.exists()) return null
        
        val channel = FileInputStream(file).channel
        return channel.map(FileChannel.MapMode.READ_ONLY, 0, channel.size())
        // FlatBuffers reads directly from this mapped buffer — no copy
    }
    
    fun writeBatch(libraryId: LibraryId, batchId: MovieBatchId, data: ByteArray) {
        cacheFile(libraryId, batchId).writeBytes(data)
    }
}
```

### Video Playback: Media3 ExoPlayer

```kotlin
@Composable
fun PlayerScreen(mediaId: String, viewModel: PlayerViewModel = hiltViewModel()) {
    val context = LocalContext.current
    
    val exoPlayer = remember {
        ExoPlayer.Builder(context)
            .build()
            .apply {
                val ticket = viewModel.playbackTicket.value
                val uri = viewModel.buildStreamUri(mediaId, ticket)
                setMediaItem(MediaItem.fromUri(uri))
                prepare()
                playWhenReady = true
            }
    }
    
    // Progress tracking
    LaunchedEffect(exoPlayer) {
        while (true) {
            delay(10_000)
            viewModel.reportProgress(
                mediaId = mediaId,
                position = exoPlayer.currentPosition / 1000.0,
                duration = exoPlayer.duration / 1000.0,
            )
        }
    }
    
    AndroidView(factory = { PlayerView(it).apply { player = exoPlayer } })
    
    DisposableEffect(Unit) {
        onDispose { exoPlayer.release() }
    }
}
```

ExoPlayer requirements for the server's `/stream/{id}` endpoint:
- HTTP range request support (`Accept-Ranges: bytes`)
- Correct `Content-Type` for the media container
- ExoPlayer will probe the container format and adapt — it handles mkv, mp4, etc.

### Image Loading: Coil with Compose Integration

```kotlin
@Composable
fun PosterCard(movie: MovieAccessor, apiClient: FerrexApiClient) {
    AsyncImage(
        model = ImageRequest.Builder(LocalContext.current)
            .data(apiClient.imageBlobUrl(movie.posterToken))
            .crossfade(true)
            .size(posterWidth, posterHeight)
            .memoryCachePolicy(CachePolicy.ENABLED)
            .diskCachePolicy(CachePolicy.ENABLED)
            .build(),
        contentDescription = movie.title,
        contentScale = ContentScale.Crop,
    )
}
```

Coil's disk cache respects HTTP cache headers. Content-addressed blob URLs
with `Cache-Control: immutable` are cached permanently.

---

## Performance Targets

Same bar as iOS, per `mobile-apps-strategy.md`:

| Metric | Target | How measured |
|---|---|---|
| Grid scroll FPS | 60fps on Pixel 6 (Tensor G1, 2021) | Android Studio Profiler → Frame timing |
| Time to first poster | < 500ms from library selection (warm cache) | Custom trace |
| Video start latency | < 2s tap-to-first-frame on LAN | Custom trace |
| Search response | < 100ms keystroke-to-results on LAN | Custom trace |
| Memory (1000 movie library) | < 120MB resident | Android Studio Profiler → Memory |
| Cold launch to browsable | < 3s with warm disk cache | Custom trace |

### How to achieve grid performance

1. **`LazyVerticalGrid`** handles Compose view recycling.
2. **Coil** handles async image loading with tiered caching.
3. **FlatBuffer zero-copy** — field access from `MappedByteBuffer` is a pointer
   offset computation, no GC pressure.
4. **Compose `@Stable` annotations** on data types to prevent unnecessary
   recomposition.
5. If `LazyVerticalGrid` proves insufficient, drop to `RecyclerView` with
   `GridLayoutManager` via `AndroidView`.

### Android-Specific Performance Concerns

- **GC pressure:** FlatBuffers' zero-copy access avoids allocating Kotlin objects
  per movie when scrolling. This is critical on Android where GC pauses cause
  frame drops. The generated accessor types read from the underlying `ByteBuffer`
  without creating intermediate objects.
- **Bitmap memory:** Coil manages a bitmap pool. Configure `ImageLoader` with
  appropriate memory cache size limits (e.g., 25% of available heap).
- **Main thread discipline:** All network and disk I/O on coroutine dispatchers.
  `Dispatchers.IO` for network/disk, `Dispatchers.Default` for computation.

---

## Open Questions (Android-specific)

### OQ-AND-001: Minimum API level
- Recommended: API 28 (Android 9, Pie) — ~95% device coverage.
- Alternative: API 26 (Android 8, Oreo) for wider reach, but loses some
  security features (StrongBox Keymaster).
- ExoPlayer supports API 21+, so no blocker there.

### OQ-AND-002: Hilt vs. manual DI vs. Koin
- Recommendation: Hilt (standard, best ViewModel integration).
- Koin is simpler but less type-safe.
- Manual DI is viable for a small app but doesn't scale.

### OQ-AND-003: Compose Navigation vs. alternatives
- Recommendation: Jetpack Navigation Compose (type-safe routes in latest versions).
- Alternatives: Voyager, Decompose — more powerful but add dependency weight.
- For v1's simple navigation graph (home → library → detail → player), Jetpack
  Navigation is sufficient.

### OQ-AND-004: ProGuard/R8 rules for FlatBuffers
- FlatBuffers generated code may need keep rules to survive minification.
- Need to verify during release build testing.

---

## Android TV Notes (Deferred)

Per `mobile-apps-strategy.md` D-006, Android TV is deferred. When it arrives:

- `core/` package is shared entirely (API client, auth, library, caching).
- `ui/` package is NOT shared — TV needs Leanback / Compose for TV components.
- A new `tv/` module is added to the Gradle project.
- Focus management and D-pad navigation are the primary TV-specific concerns.
- The desktop 10-foot mode (`ferrex-player-10ft.md`) provides UX reference.
