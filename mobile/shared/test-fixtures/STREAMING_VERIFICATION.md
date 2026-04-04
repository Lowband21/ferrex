# Streaming Endpoint Verification (Step 0.5)

## Endpoint: `GET /api/v1/stream/{id}`

### Headers Verified ✓

The server (`ferrex-server/src/handlers/stream/stream_handlers.rs`) correctly implements:

1. **`Accept-Ranges: bytes`** — present on both full and partial responses
2. **`Content-Range: bytes {start}-{end}/{total}`** — present on 206 Partial Content
3. **`Content-Type`** — correctly maps file extensions:
   - `video/mp4` for `.mp4`
   - `video/x-matroska` for `.mkv`
   - `video/quicktime` for `.mov`
   - `video/webm` for `.webm`
   - Plus 9 additional video formats
4. **`Content-Length`** — correct for both full file and range requests
5. **Range request parsing** — handles `bytes=start-end`, `bytes=start-`, and `bytes=-suffix`

### Response Status Codes

- **200 OK** — full file, streamed via `ReaderStream`
- **206 Partial Content** — range request, with `file.seek()` + `file.take()`

### Mobile Compatibility Assessment

| Feature | AVPlayer (iOS) | ExoPlayer (Android) | Server Support |
|---|---|---|---|
| Range requests | Required | Required | ✓ |
| Accept-Ranges header | Expected | Expected | ✓ |
| Content-Range header | Required for 206 | Required for 206 | ✓ |
| Content-Type: video/mp4 | Required | Required | ✓ |
| Content-Type: video/x-matroska | Partial (codec-dependent) | ✓ | ✓ |
| Connection: keep-alive | Preferred | Preferred | ✓ |
| Chunked streaming | Supported | Supported | ✓ (ReaderStream) |

### Authorization

Stream access requires authentication via the standard auth middleware.
A `GET /api/v1/stream/{id}/ticket` endpoint exists for issuing short-lived
playback tokens, suitable for query-string embedding in player URLs.

### No Server Changes Needed for v1

The existing streaming implementation is compatible with both AVPlayer and
ExoPlayer's default HTTP loading behavior. Direct play (no transcoding) is
the v1 scope per the strategy spec.
