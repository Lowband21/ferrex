# Mobile Apps Wire Format

> Research and decision spec for the binary serialization format used between
> ferrex-server and mobile clients.

## Status

| Field | Value |
|---|---|
| Created | 2025-07-15 |
| Decision | **PENDING** — recommendation below, awaiting confirmation |
| Depends on | `mobile-apps-strategy.md` D-004 (no JSON) |

---

## Problem Statement

ferrex-player uses rkyv for zero-copy deserialization of server responses. This
is a core performance advantage — the player can memory-map cached library data
and access thousands of movie records without a deserialization step.

rkyv is Rust-only. Mobile clients (Swift, Kotlin) cannot consume rkyv archives.
We need a binary serialization format that:

1. **Works across Rust, Swift, and Kotlin** with code generation (not hand-written parsers)
2. **Preserves the performance philosophy** — ideally zero-copy or near-zero-copy
3. **Is compact on the wire** — mobile networks have real bandwidth constraints
4. **Has mature, maintained tooling** for all three languages
5. **Supports schema evolution** — fields can be added without breaking old clients

---

## Candidates Evaluated

### FlatBuffers

| Property | Assessment |
|---|---|
| Zero-copy | ✅ Yes — accessor objects read directly from the buffer, no deserialization step |
| Rust support | ✅ `flatbuffers` crate (Google-maintained, stable) |
| Swift support | ✅ `flatbuffers-swift` (Google-maintained, ships with FlatBuffers repo) |
| Kotlin support | ✅ `flatbuffers-kotlin` (Google-maintained, ships with FlatBuffers repo) |
| Schema evolution | ✅ Fields can be added/deprecated without breaking; designed for this |
| Wire size | ✅ Compact — alignment padding adds ~5-15% overhead vs optimal, but no field names on wire |
| Adoption | Google (Android internals, gRPC FlatBuffers), Facebook Messenger, game industry |
| Schema language | `.fbs` files — typed, supports nested tables, unions, enums, vectors |
| Trade-offs | Schema language is more constrained than Rust structs. Accessor pattern is different from native structs (field access via methods, not properties). Building messages requires a FlatBufferBuilder. |

**How it works:** You define a `.fbs` schema → run `flatc` → get generated code
per language. The generated code provides accessor types that read directly from
the underlying byte buffer. No deserialization step. On mobile, you receive the
bytes from the network and immediately start reading fields.

### Cap'n Proto

| Property | Assessment |
|---|---|
| Zero-copy | ✅ Yes — similar accessor pattern to FlatBuffers |
| Rust support | ✅ `capnp` crate (active, well-maintained) |
| Swift support | ⚠️ Community-maintained (`capnproto-swift`), less active, uncertain iOS production readiness |
| Kotlin support | ⚠️ `capnproto-java` exists but less polished; no dedicated Kotlin generator |
| Schema evolution | ✅ Excellent — designed for evolution from day one |
| Wire size | ✅ Comparable to FlatBuffers |
| Trade-offs | Weaker mobile ecosystem. Swift/Kotlin generators are not first-party. Risk of maintenance gaps on the mobile side. |

**Verdict:** Technically excellent but the mobile tooling is a real risk. If the
Swift or Kotlin generator falls behind or has bugs, we're blocked with no
first-party support to escalate to.

### Protocol Buffers (Protobuf)

| Property | Assessment |
|---|---|
| Zero-copy | ❌ No — requires full deserialization into structs |
| Rust support | ✅ `prost` (excellent, widely used) |
| Swift support | ✅ `swift-protobuf` (**Apple-maintained**) |
| Kotlin support | ✅ `protobuf-kotlin` (**Google-maintained**) |
| Schema evolution | ✅ Excellent — the original schema evolution format |
| Wire size | ✅ Very compact (varint encoding, no field names) |
| Adoption | Industry standard. Used everywhere. |
| Trade-offs | Deserialization is fast but not zero-copy. For large library responses (thousands of movies), there IS a deserialization cost. Proto3 has no concept of required fields (everything is optional with defaults). |

**Note on performance:** Protobuf deserialization is highly optimized in all
three languages. For a 1000-movie library response, we're talking ~1-5ms on a
modern phone. This is fast, but it's NOT zero-copy — you pay the cost on every
fetch, and the deserialized structs consume heap memory proportional to the data.

### MessagePack

| Property | Assessment |
|---|---|
| Zero-copy | ❌ No |
| Rust support | ✅ `rmp-serde` (uses serde, so existing `ferrex-model` derives work) |
| Swift support | ⚠️ `MessagePack.swift` — community maintained |
| Kotlin support | ⚠️ `msgpack-kotlin` — less mature than Protobuf/FlatBuffers |
| Schema evolution | ❌ No schema — it's binary JSON. Same fragility as JSON but smaller. |
| Wire size | ✅ ~30-50% smaller than JSON |
| Trade-offs | No schema = no codegen = hand-maintained parsing on mobile. No zero-copy. Barely better than JSON for our purposes. |

**Verdict:** Eliminated. No schema, no codegen, weaker tooling. Gains over JSON
don't justify the non-standard nature.

### Bebop

| Property | Assessment |
|---|---|
| Zero-copy | ❌ No (fast serialization, but requires deserialization) |
| Rust support | ✅ `bebop` crate |
| Swift support | ❌ No official support |
| Kotlin support | ⚠️ Experimental |

**Verdict:** Eliminated. No Swift support.

---

## Comparison Matrix

| Property | FlatBuffers | Cap'n Proto | Protobuf | MessagePack |
|---|---|---|---|---|
| Zero-copy | ✅ | ✅ | ❌ | ❌ |
| Rust (production) | ✅ | ✅ | ✅ | ✅ |
| Swift (1st-party) | ✅ Google | ⚠️ Community | ✅ Apple | ⚠️ Community |
| Kotlin (1st-party) | ✅ Google | ⚠️ Community | ✅ Google | ⚠️ Community |
| Schema + codegen | ✅ | ✅ | ✅ | ❌ |
| Schema evolution | ✅ | ✅ | ✅ | ❌ |
| Wire compactness | Good | Good | Best | Good |
| Ecosystem maturity | High | Medium | Highest | Medium |

---

## Benchmark Validation

Data from [rust_serialization_benchmark](https://github.com/djkoloski/rust_serialization_benchmark)
(last updated 2026-04-03, rkyv 0.8.10, flatbuffers 25.12.19, prost 0.14.1).

### Zero-copy access + read (the metrics that matter for Ferrex)

**`log` dataset** — small records, many strings (closest to movie/series metadata):

| Crate | Access (unvalidated) | Read (unvalidated) |
|---|---|---|
| rkyv | 1.41 ns (100%) | 10.75 µs (100%) |
| flatbuffers | 2.81 ns (50%) | 55.73 µs (19%) |
| capnp | 92.59 ns (1.5%) | 131.61 µs (8%) |

**`minecraft_savedata`** — highly structured nested data (closest to library with seasons/episodes):

| Crate | Access (unvalidated) | Read (unvalidated) |
|---|---|---|
| rkyv | 1.41 ns (100%) | 176.47 ns (100%) |
| flatbuffers | 2.81 ns (50%) | 1.65 µs (10.7%) |
| capnp | 92.18 ns (1.5%) | 478.16 ns (37%) |

**nibblecode** (0.1.0) matches rkyv's zero-copy performance almost exactly, but
is Rust-only with no Swift/Kotlin codegen. Eliminated for our use case.

### Serialization speed (server-side cost)

| Crate | log | minecraft_savedata | mesh |
|---|---|---|---|
| rkyv | 235 µs | 273 µs | 180 µs |
| flatbuffers | 965 µs (4.1x slower) | 3400 µs (12.5x slower) | 471 µs (2.6x slower) |
| prost (protobuf) | 935 µs (4x slower) | 1332 µs (4.9x slower) | 7020 µs (39x slower) |

### Wire size

| Crate | log | minecraft_savedata |
|---|---|---|
| rkyv | 1,011 KB | 604 KB |
| flatbuffers | 1,276 KB (+26%) | 849 KB (+41%) |
| prost (protobuf) | 885 KB (−12%) | 597 KB (−1%) |

### What the benchmarks tell us

1. **rkyv is unmatched in Rust.** No cross-language format comes close for
   zero-copy performance. The desktop player should absolutely keep rkyv.

2. **FlatBuffers is 2x slower on access, 5–9x slower on reads vs rkyv.** This
   is meaningful — but context matters. FlatBuffers reading through an entire
   string-heavy dataset in 55µs is still sub-millisecond. For scrolling through
   a 2000-movie library, 55µs is imperceptible. rkyv does it in 10µs, which is
   *also* imperceptible. Both are ~100x faster than Protobuf deserialization.

3. **FlatBuffers serialization is 4–12x slower than rkyv on the server.** This
   is the real cost. For the `minecraft_savedata`-like structured library data,
   FlatBuffers takes 3.4ms vs rkyv's 273µs. Mitigation: the server can cache
   pre-serialized FlatBuffers responses alongside rkyv responses.

4. **FlatBuffers wire size is 26–41% larger than rkyv** for string-heavy data.
   On mobile networks this matters. Mitigation: zstd compression on the wire
   brings both formats to comparable sizes (rkyv+zstd: 326KB, flatbuffers+zstd:
   388KB for the `log` dataset — the gap narrows to ~19%).

5. **Cap'n Proto is dramatically slower** than both rkyv and FlatBuffers for
   zero-copy operations. Combined with weaker mobile tooling, it's eliminated.

6. **Protobuf is NOT zero-copy** but deserializes fast (~1–5ms for typical
   payloads). It has the best tooling ecosystem. It's the fallback recommendation
   if FlatBuffers proves unworkable in practice.

---

## Recommendation: FlatBuffers (for mobile wire format)

**FlatBuffers is the only format that is zero-copy AND has first-party
maintained codegen for all three target languages (Rust, Swift, Kotlin).**

It is NOT as fast as rkyv. The benchmarks make that clear. But it preserves the
*philosophy* that makes rkyv valuable: receive bytes → access fields immediately
→ cache raw bytes to disk → memory-map on next launch → instant. No
deserialization step. No heap allocation per record. No GC pressure on Android.

The alternative — Protobuf — would mean deserializing every response into
thousands of native objects, managing their lifecycle, and re-serializing for
disk caching. This is exactly the pattern Ferrex was built to avoid.

### Architecture: dual wire format with content negotiation

The server keeps rkyv for the desktop player and adds FlatBuffers for mobile.
This is NOT a compromise — it's the optimal design. Each client gets the best
format for its platform.

1. Define `.fbs` schemas in `mobile/shared/schemas/` mirroring `ferrex-model` types.
2. Server gains a FlatBuffers serialization layer alongside rkyv and serde.
3. Content negotiation via `Accept` header:
   - `application/x-flatbuffers` → FlatBuffers (mobile clients)
   - `application/x-rkyv` → rkyv (desktop player, existing behavior)
   - `application/json` → JSON (debugging, curl, future web client)
4. Server caches pre-serialized FlatBuffers responses to amortize the
   serialization cost (4–12x slower than rkyv per the benchmarks).
5. Desktop player does NOT migrate from rkyv. rkyv remains the fastest path.

### Schema workflow

```
mobile/shared/schemas/
├── media.fbs          # Movie, Series, Episode, Season references
├── library.fbs        # Library, LibraryType
├── details.fbs        # EnhancedMovieDetails, EnhancedSeriesDetails, cast, genres
├── watch.fbs          # WatchProgress, SeriesWatchStatus, SeasonWatchStatus
├── auth.fbs           # AuthToken, DeviceRegistration, LoginRequest
├── image.fbs          # ImageManifest, ImageQuery, ImageReadyEvent
├── scan.fbs           # ScanProgressEvent, ScanConfig (for future use)
├── ids.fbs            # UUID types, MediaID, LibraryId, etc.
└── common.fbs         # Shared enums, timestamps, pagination
```

`flatc` generates:
- Rust types into a `ferrex-flatbuffers` crate (workspace member)
- Swift types into `mobile/ios/FerrexAPI/Generated/`
- Kotlin types into `mobile/android/app/.../generated/`

A `just` recipe or build script ensures codegen stays in sync.

---

## Risks and Mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| FlatBuffers accessor pattern is unfamiliar | Medium | Wrapper types in each platform's API client that expose native-feeling interfaces. The generated accessors are an implementation detail. |
| Schema maintenance burden (`.fbs` + `ferrex-model` must stay in sync) | Medium | CI check that compares `.fbs` field lists against `ferrex-model` struct fields. Consider generating `.fbs` from Rust types long-term. |
| FlatBuffers Swift codegen quality | Low-Medium | Google maintains it and Android/iOS teams use it internally. Pin to a known-good `flatc` version. |
| Performance delta vs rkyv on desktop | Low | FlatBuffers and rkyv have comparable zero-copy access patterns. Benchmark before any desktop migration. |

---

## Open Questions

### OQ-001: Does the desktop player also migrate to FlatBuffers?
- **No.** The benchmarks are conclusive: rkyv is 2–9x faster for zero-copy
  operations in Rust. The desktop player stays on rkyv. The server serves both
  formats via content negotiation. This is the intended long-term architecture,
  not a compromise.

### OQ-002: gRPC or plain HTTP?
- FlatBuffers has a gRPC integration (`grpc-flatbuffers`).
- Current server is Axum (HTTP). Adding gRPC is a significant change.
- **Recommendation:** Stay with HTTP REST. Use FlatBuffers as the body encoding. gRPC adds complexity (HTTP/2 framing, bidirectional streams) that isn't needed when the existing REST API is well-structured. WebSocket remains the real-time channel.

### OQ-003: How to handle SSE/WebSocket events?
- SSE events are currently JSON-encoded.
- Options: keep events as JSON (small payloads, infrequent), or encode event payloads in FlatBuffers too.
- **Recommendation:** Events can stay JSON initially. They're small (scan progress, image readiness) and infrequent. Migrate to FlatBuffers if profiling shows it matters.
