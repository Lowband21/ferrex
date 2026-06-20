# Scanner layout contract

Ferrex's manifest scanner classifies every path under a configured library root as supported, ignored, or unsupported with a stable diagnostic code. The domain source of truth is `ferrex_core::domain::scan::manifest`.

## Movies libraries

Supported:

- Flat movie media directly under the Movies root: `/Movies/Alien.mkv`.
- Movie folders directly under the Movies root: `/Movies/Alien (1979)/Alien.mkv`.

Reported as diagnostics:

- Nested folders below a movie folder: `scanner.layout.movie_nested_folder_unsupported`.
- Extras folders such as `Extras`, `Trailers`, `Featurettes`, or `Deleted Scenes`: `scanner.layout.movie_extras_unsupported`.

## Series libraries

Supported:

- Series folders directly under the Series root: `/Series/Fringe`.
- Season folders directly under a series folder, including `Specials`: `/Series/Fringe/Season 01/S01E01.mkv` and `/Series/Fringe/Specials/S00E01.mkv`.
- Parseable episode files directly under a series folder: `/Series/Fringe/S01E01.mkv`.

Reported as diagnostics:

- Video files directly under the Series library root: `scanner.layout.series_library_root_media_unsupported`.
- Direct series-root episode files that do not parse: `scanner.layout.series_direct_episode_parse_failed`.
- Episode files in a Season/Specials folder that do not parse: `scanner.layout.series_episode_parse_failed`.
- Episode file season number mismatches: `scanner.layout.series_season_mismatch`.
- Nested folders below a series or season folder: `scanner.layout.series_nested_folder_unsupported`.
- Extras folders: `scanner.layout.series_extras_unsupported`.

## Shared filtering

- Hidden/system paths are ignored with `scanner.layout.hidden_system_path`.
- Configured `ignored_extensions` are ignored with `scanner.layout.ignored_extension`.
- Configured `ignored_path_patterns` are ignored with `scanner.layout.ignored_path_pattern`.
- Non-media files are ignored with `scanner.layout.non_media_file`.

Each diagnostic includes remediation text from `ManifestDiagnosticReason::remediation()` so UI and operator tooling can show the same recovery guidance without string matching on logs.

## Recovery and diagnostics surfaces

Manifest runs persist root/partition coverage in `manifest_runs`, latest path state in `manifest_entries`, per-code operator diagnostics in `manifest_diagnostics`, stale partition cursors in `manifest_partition_cursors`, and watch-event recovery hints in `manifest_deferred_watch_hints`.

The scan config endpoint exposes manifest walker bounds (batch size, partition size, max depth), supported layout examples, and the stable diagnostic code list. Scan metrics/status payloads expose manifest run counts, diagnostics grouped by code, deferred watch hints by status, stale partition count, stuck run/library indicators, and `oldest_manifest_lag_ms` so admins can tell whether recovery has fallen behind.

Recovery behavior is conservative: successful root or prefix-partition manifest runs may mark missing entries and tombstone unavailable media, but failed/stalled/canceled runs never delete or tombstone media. Filesystem watcher overflows enqueue a root manifest scan, and file-level watch bursts enqueue bounded partition scans; pending deferred watch hints are replayed during stuck-scan recovery.

## DB-backed validation caveats

End-to-end manifest tests use temporary media trees and PostgreSQL-backed validation so they can assert durable run, entry, diagnostic, move, delete, overflow, and active-run watch-event behavior. These tests require a `DATABASE_URL` with permission to create/drop isolated test databases; when no database URL is available, report the DB-backed manifest tests as skipped rather than treating them as Rust unit-test failures.
