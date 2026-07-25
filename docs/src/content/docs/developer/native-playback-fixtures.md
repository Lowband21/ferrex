---
title: "Native playback fixtures and test matrix"
description: "Reproducible synthetic media, authenticated transport, and environment inventory for desktop playback migration testing."
sidebar:
  order: 10
---

This page defines the P0 media and environment inputs for the [native mpv integration specification](https://github.com/Lowband21/ferrex/blob/dev/docs/specs/native-mpv-playback.md). Generated media and run results stay outside the repository; the generator, validator, and transport server are versioned.

## Generate and verify

The complete set requires Python 3, `ffmpeg`, `ffprobe`, Fontconfig/DejaVu Sans (or an explicitly supplied redistributable TrueType font), and these FFmpeg encoders:

- `libx264`;
- `libx265`;
- `libvpx-vp9`;
- `libaom-av1`;
- AAC; and
- ASS.

From the repository root:

```bash
./scripts/qa/native_playback_fixtures.py generate
./scripts/qa/native_playback_fixtures.py verify
```

The default output is `target/native-playback-fixtures/`, which is already ignored with the rest of `target/`. Regeneration is explicit:

```bash
./scripts/qa/native_playback_fixtures.py generate --force
```

The generator refuses to replace a directory without its Ferrex fixture marker. It writes a schema-versioned `manifest.json` and `SHA256SUMS`; verification checks the hashes, codecs, color signaling, HDR side data, tracks, chapters, attachments, subtitles, HLS structure, and expected malformed-input rejection.

Use `--font /path/to/font.ttf` when DejaVu Sans is unavailable. The selected font is embedded into generated Matroska files, not copied into the repository. Confirm that a replacement font permits this use.

## Fixture inventory

All fixtures are four-second, 640×360 synthetic patterns with a low-level synthetic audio tone unless noted otherwise.

| Path | Coverage and required observation |
|---|---|
| `h264-sdr-8bit.mkv` | H.264, `yuv420p`, BT.709 SDR baseline. Keyframes are spaced for one-second HLS segmentation. |
| `hevc-main10-sdr.mkv` | HEVC Main10, `yuv420p10le`, BT.709 SDR. This distinguishes bit depth from HDR. |
| `hdr10-pq.mkv` | HEVC Main10 with BT.2020/PQ, mastering-display metadata, MaxCLL 1000, and MaxFALL 400. |
| `hlg.mkv` | HEVC Main10 with BT.2020 and ARIB STD-B67/HLG transfer signaling. |
| `vp9-sdr.mkv` | VP9 SDR decode path. |
| `av1-sdr.mkv` | AV1 SDR decode path. |
| `ass-animation-fonts.mkv` | Two animated/karaoke ASS events and an attached DejaVu Sans font. |
| `pgs-bitmap.mkv` | A locally constructed HDMV PGS bitmap object, palette, show, and clear sequence. No external PGS encoder or copyrighted subtitle is required. |
| `multitrack-structure.mkv` | Two named/language-tagged audio tracks, two text subtitle tracks (default and forced), two chapters, and a font attachment. |
| `transcoded-hls/index.m3u8` | VOD HLS playlist and MPEG-TS segments derived from the H.264 baseline. This is a deterministic transport-format fixture, not evidence of Ferrex transcoding parity. |
| `malformed-truncated.mkv` | Truncated EBML input that `ffprobe` must reject. |
| `unsupported.txt` | Non-media input that `ffprobe` must reject. |
| `sources/` | Generated ASS, SRT, chapter metadata, and raw SUP inputs for inspection and external-subtitle tests. |

The HDR10 and HLG files test metadata handling and native-output behavior. Their synthetic pattern is **not** a mastering-quality visual reference and cannot establish display accuracy by itself.

## Authenticated HTTP range and HLS transport

Generate the fixtures, choose an ephemeral secret, and start the loopback-only server. The token is read from the environment rather than a command-line argument.

```bash
export FERREX_FIXTURE_TOKEN="$(python3 -c 'import secrets; print(secrets.token_urlsafe(32))')"
./scripts/qa/native_playback_fixture_server.py --port 8000
```

The default is bearer authentication. It supports `HEAD`, `GET`, and a single standards-style byte range, including open-ended and suffix ranges. It does not list directories and refuses to serve an unmarked root.

Verify authorization and range handling:

```bash
curl --fail \
  -H "Authorization: Bearer ${FERREX_FIXTURE_TOKEN}" \
  -H 'Range: bytes=0-1023' \
  http://127.0.0.1:8000/h264-sdr-8bit.mkv \
  --output /tmp/ferrex-range.bin
```

The server selects an ephemeral port by default. For automation, use a private ready file:

```bash
ready_dir="$(mktemp -d)"
chmod 700 "$ready_dir"
port_file="$ready_dir/port"
FERREX_FIXTURE_TOKEN="$FERREX_FIXTURE_TOKEN" \
  ./scripts/qa/native_playback_fixture_server.py --port-file "$port_file" &
server_pid=$!
cleanup_fixture_server() {
  kill "$server_pid" 2>/dev/null || true
  wait "$server_pid" 2>/dev/null || true
  rm -rf "$ready_dir"
}
trap cleanup_fixture_server EXIT
for _ in {1..100}; do
  [[ -s "$port_file" ]] && break
  if ! kill -0 "$server_pid" 2>/dev/null; then
    wait "$server_pid" 2>/dev/null || true
    echo "fixture server exited before publishing its port" >&2
    exit 1
  fi
  sleep 0.1
done
[[ -s "$port_file" ]] || {
  echo "fixture server did not publish its port within 10 seconds" >&2
  exit 1
}
port="$(<"$port_file")"
[[ "$port" =~ ^[0-9]+$ ]] || {
  echo "fixture server published an invalid port" >&2
  exit 1
}
```

For the real libmpv native-window smoke (requires a working desktop VO):

```bash
FERREX_MPV_SMOKE_URL="http://127.0.0.1:${port}/h264-sdr-8bit.mkv" \
FERREX_MPV_SMOKE_AUTHORIZATION="Bearer ${FERREX_FIXTURE_TOKEN}" \
cargo test -p ferrex-player-playback --features mpv \
  mpv_adapter::tests::linked_native_window_load_control_fullscreen_stop_and_close_smoke \
  -- --ignored --exact --nocapture
```

Use `transcoded-hls/index.m3u8` as the URL to exercise bearer propagation
across playlist and segment requests. To exercise the complete local
track/chapter/edition control path, run the same ignored test with
`FERREX_MPV_SMOKE_MEDIA=target/native-playback-fixtures/multitrack-structure.mkv`;
selectors are tested only when the loaded fixture advertises the corresponding
catalog. The smoke also applies an identity native-VO shader, confirms its
redacted observed count, writes/removes a screenshot, and clears the shader.

Run subtitle formats separately with `ass-animation-fonts.mkv` and
`pgs-bitmap.mkv`. To add and select a local sidecar through the public contract,
use:

```bash
FERREX_MPV_SMOKE_MEDIA=target/native-playback-fixtures/h264-sdr-8bit.mkv \
FERREX_MPV_SMOKE_EXTERNAL_SUBTITLE=target/native-playback-fixtures/sources/english.srt \
cargo test -p ferrex-player-playback --features mpv \
  mpv_adapter::tests::linked_native_window_load_control_fullscreen_stop_and_close_smoke \
  -- --ignored --exact --nocapture
```

The test requires the new track to be selected, text-kind, and marked external;
local paths and temporary screenshot/shader paths are filtered from copied mpv
logs. `--auth query` exists only for the retained legacy compatibility path;
query credentials do not automatically propagate into relative HLS segment
URLs. Logs omit query strings and redact the configured token.

## Local Ferrex server acceptance

The loopback fixture server isolates client transport behavior. It does **not**
satisfy the Ferrex-server direct-play or transcode acceptance gates.

For server-backed acceptance:

1. mount or copy `target/native-playback-fixtures/` into a local test library;
2. scan the library and record the resulting media IDs;
3. request the normal playback ticket/source through the Ferrex API;
4. test direct play with `h264-sdr-8bit.mkv`, `multitrack-structure.mkv`, and
   both HDR-signaled files;
5. force the local server's transcode profile and verify its returned manifest
   and every segment use the same playback-scoped authorization policy; and
6. run the UI/native-window path through next episode, stop, EOF, and navigation
   while retaining redacted client and server diagnostics.

Feature-gated ignored tests provide reproducible direct-play,
generated-transcode, and protected-HLS transport paths through isolated
PostgreSQL databases and real network-bound Ferrex routers. The direct test
issues the normal account session and playback-scoped ticket, opens the
protected stream through the
backend-neutral native-mpv session, and confirms metadata/resume, pause,
authenticated range seek, shader and screenshot commands, diagnostics
redaction, and ordered stop:

```bash
./scripts/dev/sqlx-db.sh start
set -a
source .env.sqlx
set +a
DATABASE_URL="$DATABASE_URL_ADMIN" \
  cargo test -p ferrex-server --features native-mpv-e2e \
  --test playback_stream_failures \
  playback_ticket_drives_display_backed_native_mpv_through_ferrex_router \
  -- --ignored --exact --nocapture --test-threads=1
```

The default input is `h264-sdr-8bit.mkv`; set
`FERREX_MPV_SERVER_SMOKE_MEDIA` to another generated file.

The HLS transport test rewrites only the generated fixture's local segment
references to credential-free protected Ferrex stream routes. It requires one
header-carried playback ticket on the manifest and every segment, checks
unauthenticated rejection and HLS MIME types, and then runs the same real
native-mpv lifecycle:

```bash
./scripts/dev/sqlx-db.sh start
set -a
source .env.sqlx
set +a
DATABASE_URL="$DATABASE_URL_ADMIN" \
  cargo test -p ferrex-server --features native-mpv-e2e \
  --test playback_stream_failures \
  playback_ticket_propagates_to_every_router_backed_hls_segment \
  -- --ignored --exact --nocapture --test-threads=1
```

Set `FERREX_MPV_SERVER_SMOKE_HLS` to use another local VOD manifest. Relative
media references must remain beneath its directory; remote, absolute, and
traversal references fail closed.

The generated-transcode test submits the `360p` profile to the real bounded
FFmpeg job provider, waits for atomic publication, checks unauthenticated
rejection and the playback ticket on the manifest and every segment, verifies
cached reuse, and loads the resulting protected playlist through native mpv:

```bash
./scripts/dev/sqlx-db.sh start
set -a
source .env.sqlx
set +a
DATABASE_URL="$DATABASE_URL_ADMIN" \
  cargo test -p ferrex-server --features native-mpv-e2e \
  --test playback_stream_failures \
  server_generated_transcode_plays_through_display_backed_native_mpv \
  -- --ignored --exact --nocapture --test-threads=1
```

Set `FERREX_MPV_SERVER_TRANSCODE_MEDIA` to transcode another generated local
fixture. The test requires FFmpeg with `libx264` and AAC encoders in addition
to the direct-play test's requirements.

These tests use the production router, ticket service, transcode manager,
stream handlers, and real HTTP/libmpv boundary. The pre-generated HLS run
isolates protected manifest/segment transport, while the generated run covers
bounded FFmpeg generation, atomic publication, cached reuse, protected assets,
and display-backed loading as one lifecycle. The normal display-free route and
manager tests continue to cover job ownership and failure behavior. A manual
player quality-picker run and UI episode-transition acceptance remain in the
live-server gate above.

Follow the [playback authentication regression procedure](/reference/qa/playback-auth-regression/)
for ticket lifecycle and retained-artifact checks. Record server revision,
profile, source media ID, returned container/codecs, and whether playback was
direct or transcoded. Never retain the raw ticket, authorization header, cookie,
or unredacted mpv log.

## Native-window episode replacement

The ignored playback-domain smoke lets the first synthetic episode reach real
native-mpv EOF, verifies final progress and the backend-preserving next-episode
request, then drives the normal `SetStreamSource` close/reopen path and requires
the second episode to start in a newer mpv session generation:

```bash
FERREX_MPV_SMOKE_MEDIA="$PWD/target/native-playback-fixtures/h264-sdr-8bit.mkv" \
  cargo test -p ferrex-player-playback --features mpv \
  update::tests::linked_native_window_eof_reloads_next_episode_with_same_backend \
  -- --ignored --exact --nocapture --test-threads=1
```

This combines the real native-VO lifecycle with the backend-neutral episode
reducer. The outer repository selection, ticket-resolution task, and visible
player-shell transition remain part of the manual live-server UI gate.

## Native-window lifecycle stress

The ignored linked-libmpv stress test creates a fresh in-process mpv core and
native window for every cycle, waits for playback and native-VO configuration,
issues an ordered stop, confirms the terminal event, and tears the owner down.
It defaults to the 100 cycles required by the native-presentation gate:

```bash
FERREX_MPV_SMOKE_MEDIA="$PWD/target/native-playback-fixtures/h264-sdr-8bit.mkv" \
FERREX_MPV_STRESS_CYCLES=100 \
FERREX_MPV_STRESS_MAX_RSS_GROWTH_MIB=64 \
FERREX_MPV_STRESS_MAX_FD_GROWTH=4 \
  cargo test -p ferrex-player-playback --features mpv \
  mpv_adapter::tests::linked_native_window_load_stop_lifecycle_stress \
  -- --ignored --exact --nocapture --test-threads=1
```

On Linux the test reports first-cycle baseline, final, and peak resident memory
and open file descriptors. The two optional limit variables make excessive
final growth fail the run; choose and record a reviewed platform budget rather
than silently loosening it. Use a smaller `FERREX_MPV_STRESS_CYCLES` only to
validate the harness. A platform gate still requires one uninterrupted
100-cycle run while separately monitoring native/GPU memory and window-system
resources; save its redacted output and resource samples under the environment's ignored
`target/native-playback-results/` run directory. This generic native-window
job does not replace the Windows HWND or macOS AppKit presenter-specific
100-cycle gates.

## Windows and macOS integrated-presenter handoff

The Windows and macOS implementations have an explicit `spike` build gate so
they can be exercised on representative systems without changing the Auto
backend policy. In a spike build, **Play in MPV** requests
`mpv-integrated`: mpv owns the native video window and Iced attaches a hidden,
transparent controls window after both native handles are ready. A failed
preflight or attachment records a structured reason and returns to
`mpv-native-window` while GStreamer remains available for rollback.

"Handoff ready" here means the target code, deterministic fallback, source
builders, package staging, and display-free tests are present. It does **not**
mean the production/Auto gate has passed. Keep that gate closed until the
hardware observations below and the clean-package smoke have been recorded.

### Windows test build

The canonical, provenance-recorded path is the `Windows Dist` workflow. It
builds the pinned LGPL libmpv SDK, generates the MSVC import library, compiles
the selected presenter mode, stages only the reviewed GStreamer 1.28.4 plugin
roots plus their recursive PE/GIO/TLS closure, and audits H.264/AAC HLS and
strict HTTPS from the clean stage before uploading the zip. Runtime DLL owners,
versions, hashes, and notices are recorded; the floating Rust/MSYS2 build-tool
selection remains visible in workflow logs rather than pinned as a bit-for-bit
reproducible toolchain.

Use the uploaded artifact for the clean-host handoff run. From a PowerShell
shell with GitHub CLI authentication:

```powershell
$ref = '<branch-or-tag-containing-tested-revision>'
$expectedSha = (git rev-parse $ref).Trim()
gh workflow run windows-dist.yml --ref $ref `
  -f profile=release -f presenter_mode=spike
gh run list --workflow windows-dist.yml --event workflow_dispatch `
  --commit $expectedSha --limit 5
# Select the spike run dispatched above, not merely the newest repository run.
$run = '<run-id>'
$actualSha = (gh run view $run --json headSha | ConvertFrom-Json).headSha
if ($actualSha -ne $expectedSha) { throw "Run revision mismatch: $actualSha" }
gh run watch $run --exit-status
$spikeDir = Join-Path 'target\windows-handoff' $run
if (Test-Path -LiteralPath $spikeDir) {
  throw "Refusing to reuse handoff directory: $spikeDir"
}
$null = New-Item -ItemType Directory -Path $spikeDir
gh run download $run --name ferrex-player-windows-spike `
  --dir $spikeDir
$zips = @(Get-ChildItem -LiteralPath $spikeDir -File -Filter '*.zip')
if ($zips.Count -ne 1) {
  throw "Expected exactly one zip in $spikeDir, found $($zips.Count)"
}
$zip = $zips[0]
Get-FileHash $zip.FullName -Algorithm SHA256
$appDir = Join-Path $spikeDir 'app'
Expand-Archive -LiteralPath $zip.FullName -DestinationPath $appDir
$localAppData = if ([string]::IsNullOrWhiteSpace($env:LOCALAPPDATA)) {
  $env:TEMP
} else {
  $env:LOCALAPPDATA
}
$registry = Join-Path $localAppData 'Ferrex\gstreamer-1.0\registry.bin'
if (Test-Path -LiteralPath $registry) {
  throw "Clean-user prerequisite failed; archive or remove $registry first"
}
cmd /c "$appDir\run-ferrex.bat" "http://your-ferrex-server:3000"
```

Run that launcher, rather than the executable or downloaded PowerShell script
directly: the batch launcher is not subject to PowerShell execution policy and
selects only the bundled plugins, GIO TLS modules, registry, CA bundle, and
`gst-plugin-scanner`. Perform the artifact run from a clean Windows user or VM
without another GStreamer or mpv directory on `PATH`, and retain the displayed
zip hash, run ID, and verified `headSha` with the result record.

For the required disabled-presenter control, dispatch the same revision again
and retain its separately closure-audited artifact:

```powershell
gh workflow run windows-dist.yml --ref $ref `
  -f profile=release -f presenter_mode=disabled
gh run list --workflow windows-dist.yml --event workflow_dispatch `
  --commit $expectedSha --limit 5
$disabledRun = '<disabled-run-id>'
$disabledSha = (gh run view $disabledRun --json headSha | ConvertFrom-Json).headSha
if ($disabledSha -ne $expectedSha) { throw "Run revision mismatch: $disabledSha" }
gh run watch $disabledRun --exit-status
$disabledDir = Join-Path 'target\windows-disabled-handoff' $disabledRun
if (Test-Path -LiteralPath $disabledDir) {
  throw "Refusing to reuse handoff directory: $disabledDir"
}
$null = New-Item -ItemType Directory -Path $disabledDir
gh run download $disabledRun --name ferrex-player-windows-disabled `
  --dir $disabledDir
$disabledZips = @(
  Get-ChildItem -LiteralPath $disabledDir -File -Filter '*.zip'
)
if ($disabledZips.Count -ne 1) {
  throw "Expected exactly one zip in $disabledDir, found $($disabledZips.Count)"
}
$disabledZip = $disabledZips[0]
Get-FileHash $disabledZip.FullName -Algorithm SHA256
$disabledAppDir = Join-Path $disabledDir 'app'
Expand-Archive -LiteralPath $disabledZip.FullName `
  -DestinationPath $disabledAppDir
$modeFile = Join-Path $disabledAppDir `
  'share\ferrex-player\PRESENTER_BUILD_MODE'
$mode = (Get-Content -LiteralPath $modeFile -Raw).Trim()
if ($mode -ne 'disabled') { throw "Unexpected presenter mode: $mode" }
if (Test-Path -LiteralPath $registry) {
  $evidenceDir = Join-Path $disabledDir 'evidence'
  $null = New-Item -ItemType Directory -Path $evidenceDir
  Move-Item -LiteralPath $registry `
    -Destination (Join-Path $evidenceDir 'registry-after-spike.bin')
}
if (Test-Path -LiteralPath $registry) {
  throw "Spike GStreamer registry was not isolated: $registry"
}
cmd /c "$disabledAppDir\run-ferrex.bat" "http://your-ferrex-server:3000"
```

Complete the spike cases below and quit the app before running the disabled
block. The commands refuse stale download/extraction directories, require one
archive, and move the spike GStreamer registry out of the fixed per-user path
before the disabled launch. A separate clean user is also acceptable. Do not
reuse either extraction tree or registry evidence. Tag-triggered Windows
artifacts force `disabled`; the unapproved spike cannot become a release
attachment.

For a local MSVC test, first build the SDK from an MSYS2 UCRT64 shell as
documented in `utils/build-windows/build-libmpv-lgpl.sh`, then run this from a
PowerShell developer shell. The installer helper downloads only the pinned
official GStreamer 1.28.4 SDK and verifies its recorded SHA-256:

```powershell
$root = 'C:\ferrex-libmpv-sdk'
& .\utils\build-windows\new-libmpv-import-library.ps1 -SdkRoot $root
$dll = Get-ChildItem (Join-Path $root 'bin') -File |
  Where-Object { $_.Name -in @('libmpv-2.dll', 'mpv-2.dll', 'mpv.dll') } |
  Select-Object -First 1
if (-not $dll) { throw "libmpv runtime DLL is missing from $root\bin" }

$env:LIBMPV_ROOT = $root
$env:LIBMPV_LIB_DIR = Join-Path $root 'lib'
$env:LIBMPV_INCLUDE_DIR = Join-Path $root 'include'
$env:LIBMPV_DLL_DIR = Join-Path $root 'bin'
$env:LIBMPV_DLL = $dll.FullName
$gst = Join-Path (Get-Location) 'target\gstreamer-msvc-x86_64'
& .\utils\build-windows\install-gstreamer.ps1 -Destination $gst
$env:GSTREAMER_1_0_ROOT_MSVC_X86_64 = $gst
$env:PKG_CONFIG = Join-Path $gst 'bin\pkg-config.exe'
$env:PKG_CONFIG_PATH = Join-Path $gst 'lib\pkgconfig'
$env:FERREX_MPV_WINDOWS_PRESENTER = 'spike'
$env:PATH = "$(Join-Path $root 'bin');$(Join-Path $gst 'bin');$env:PATH"
cargo run -p ferrex-player --features mpv
```

The environment value is consumed at compile time. Rebuild after changing it.
Do not substitute a locally installed default mpv build for release evidence.

### macOS test build

The canonical, provenance-recorded path is the `macOS App Bundle` workflow,
which builds both Apple Silicon and Intel artifacts. mpv, FFmpeg, libplacebo,
libass, and Lua 5.2 sources are pinned; Homebrew build/runtime inputs are
version/hash recorded and rejected if their expected GStreamer profile drifts.
The workflow rewrites the complete dylib/GIO/trust closure to bundle-relative
paths, signs nested code, and performs strict clean-bundle HTTP/HTTPS HLS
playback. This engineering handoff artifact explicitly requires macOS 15 or
newer. Use the uploaded app—not a raw Cargo binary—for clean-bundle, Dock,
fullscreen, GStreamer rollback, and runtime-path evidence:

```bash
ref='<branch-or-tag-containing-tested-revision>'
expected_sha="$(git rev-parse "$ref")"
gh workflow run macos-dist.yml --ref "$ref" -f presenter_mode=spike
gh run list --workflow macos-dist.yml --event workflow_dispatch \
  --commit "$expected_sha" --limit 5
# Select the spike run dispatched above and use arm64 or x86_64 for this Mac.
run='<run-id>'
arch="$(uname -m)"
actual_sha="$(gh run view "$run" --json headSha --jq .headSha)"
test "$actual_sha" = "$expected_sha"
gh run watch "$run" --exit-status
spike_dir="target/macos-handoff/$run"
test ! -e "$spike_dir" || {
  echo "refusing to reuse handoff directory: $spike_dir" >&2
  exit 1
}
mkdir -p "$spike_dir"
gh run download "$run" --name "ferrex-player-macos-$arch-spike" \
  --dir "$spike_dir"
(cd "$spike_dir" && shasum -a 256 --check ./*.sha256)
archive_count="$(find "$spike_dir" -maxdepth 1 -type f -name '*.zip' |
  wc -l | tr -d '[:space:]')"
test "$archive_count" -eq 1 || {
  echo "expected exactly one zip in $spike_dir, found $archive_count" >&2
  exit 1
}
archive="$(find "$spike_dir" -maxdepth 1 -type f -name '*.zip' -print)"
ditto -x -k "$archive" "$spike_dir/app"
registry="$HOME/Library/Caches/io.github.lowband21.FerrexPlayer/gstreamer-registry-1.0.bin"
test ! -e "$registry" || {
  echo "clean-user prerequisite failed; archive or remove $registry first" >&2
  exit 1
}
open "$spike_dir/app/Ferrex Player.app"
```

Use a clean macOS user with Homebrew library paths, `DYLD_*`, `GST_*`, and
`VK_*` overrides unset. At least one representative run must use a host or VM
without Homebrew installed, rather than relying only on a clean account on the
build host. The app must obtain libmpv, MoltenVK, the GStreamer
plugins/scanner, GIO TLS module, CA trust database, and their closure from
`Contents` only. An unsigned workflow run is ad-hoc signed for engineering
handoff rather than notarized for public distribution; preserve the verified
archive hash, run ID, `headSha`, signing output, and closure/HLS audit output.

Complete the spike cases below and quit the app before continuing. Dispatch
`presenter_mode=disabled` at the same `ref` for the fallback control, verify
that run's `headSha`, and download `ferrex-player-macos-$arch-disabled`.
Tag-triggered artifacts force the disabled mode but remain Actions artifacts;
this engineering workflow never attaches macOS artifacts to a GitHub Release.
Public distribution requires a separate Developer ID signing and notarization
workflow. Verify and launch the disabled artifact from its own directory:

```bash
gh workflow run macos-dist.yml --ref "$ref" -f presenter_mode=disabled
gh run list --workflow macos-dist.yml --event workflow_dispatch \
  --commit "$expected_sha" --limit 5
# Select the disabled run dispatched above.
disabled_run='<disabled-run-id>'
disabled_sha="$(gh run view "$disabled_run" --json headSha --jq .headSha)"
test "$disabled_sha" = "$expected_sha"
gh run watch "$disabled_run" --exit-status
disabled_dir="target/macos-disabled-handoff/$disabled_run"
test ! -e "$disabled_dir" || {
  echo "refusing to reuse handoff directory: $disabled_dir" >&2
  exit 1
}
mkdir -p "$disabled_dir"
gh run download "$disabled_run" \
  --name "ferrex-player-macos-$arch-disabled" --dir "$disabled_dir"
(cd "$disabled_dir" && shasum -a 256 --check ./*.sha256)
disabled_archive_count="$(find "$disabled_dir" -maxdepth 1 -type f \
  -name '*.zip' | wc -l | tr -d '[:space:]')"
test "$disabled_archive_count" -eq 1 || {
  echo "expected exactly one zip in $disabled_dir, found $disabled_archive_count" >&2
  exit 1
}
disabled_archive="$(find "$disabled_dir" -maxdepth 1 -type f \
  -name '*.zip' -print)"
ditto -x -k "$disabled_archive" "$disabled_dir/app"
test "$(cat "$disabled_dir/app/Ferrex Player.app/Contents/Resources/presenter-build-mode.txt")" = disabled
if test -e "$registry"; then
  mkdir -p "$disabled_dir/evidence"
  mv "$registry" "$disabled_dir/evidence/registry-after-spike.bin"
fi
test ! -e "$registry"
open "$disabled_dir/app/Ferrex Player.app"
```

Run the same fallback and Auto cases against this copy. The commands refuse
stale directories, require one archive, and archive the spike registry before
the disabled launch. A separate clean user is also acceptable; never reuse the
spike bundle or registry evidence.

For a local development run, install the native build prerequisites listed in
that workflow and choose a new empty prefix:

```bash
prefix="$PWD/target/ferrex-libmpv-macos"
export MACOSX_DEPLOYMENT_TARGET=15.0
bash scripts/release/macos-build-libmpv.sh "$prefix"

export PKG_CONFIG_PATH="$prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export LIBRARY_PATH="$prefix/lib${LIBRARY_PATH:+:$LIBRARY_PATH}"
export DYLD_FALLBACK_LIBRARY_PATH="$prefix/lib${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"
export FERREX_MPV_MACOS_PRESENTER=spike
cargo run -p ferrex-player --features mpv
```

The prefix builder refuses to install over a non-empty directory. The
presenter build value is compile-time state, so rebuild after changing it.
Normal AppKit object access stays on the Iced/AppKit main thread; blocking
libmpv teardown is handed to the named background reaper only after the child
window has detached.

### Representative-system procedure

Generate and verify the fixtures first. Test a normal direct Ferrex stream and
at least `h264-sdr-8bit.mkv`, `multitrack-structure.mkv`,
`ass-animation-fonts.mkv`, `pgs-bitmap.mkv`, `hdr10-pq.mkv`, and `hlg.mkv`.
Run the SDR cases on every environment and the HDR/EDR cases only on a capable
display with the OS HDR state recorded.

For every run:

1. Start Ferrex with the spike compiled in and use **Play in MPV**. Confirm
   diagnostics report requested/selected `mpv-integrated`, then
   `presenter_state=attached`, with no fallback reason. Retain the pointer-free
   `native player overlay handoff:` debug transition log plus a native-window
   trace or screen recording proving this order: hidden controls host
   allocated; presenter attached and positioned to mpv's content rectangle;
   retained main window hide completed; presenter host made visible; overlay
   focus confirmed. The serialized diagnostic snapshot supplies presenter
   state/geometry; it does not by itself prove Iced hide/focus delivery. Any early
   `ShowWindow`/`orderFront`, stale main-window resize/move after attachment, or
   visible flash fails the run.
2. Confirm there is one active player identity in the Windows taskbar and
   Alt-Tab list, or one Ferrex application identity in the macOS Dock and app
   switcher. No second blank or permanently hidden window may remain after
   stop.
3. Move and resize continuously, cross monitors, change Windows per-monitor
   DPI or macOS backing scale, minimize/restore, hide/unhide the app, and cover
   then uncover it. The overlay must follow the video content rectangle without
   drift, stale controls, focus theft, or visible startup flash. Include one
   independent-viewport case: retain a 1280×720 main-window snapshot while the
   native content/overlay becomes a materially different size and aspect (for
   example 1024×768) and scale. Control layout, focus geometry, progress-bar
   bounds, and pointer hit targets must follow the live overlay viewport rather
   than the retained main size.
4. Exercise mouse, keyboard, seek, pause, volume, track/subtitle selection,
   settings, next/previous episode, and Back/Home. In integrated mode Iced is
   the input owner; mpv OSC/default bindings must not compete with it.
5. Enter and leave native fullscreen repeatedly. mpv owns the transition and
   Ferrex must wait for the observed fullscreen property before changing its
   snapshot. On macOS, repeat across Spaces and during the native animation.
6. Compare the HDR/EDR fixture with controls continuously visible, controls
   hidden, and repeated overlay show/hide. Record the reported input/output
   color parameters rather than inferring HDR from the filename or backend.
7. Record `current-vo`, GPU context/API, adapter, `hwdec-current`, frame/drop
   counters, and the presenter geometry/scale. VideoToolbox on macOS and
   D3D11VA/DXVA2 on Windows are observations, not assumptions.
8. Test normal stop, EOF, native window close, overlay close, app quit, reload,
   and an immediate second playback. Native relationships must detach before
   either host is destroyed, and shutdown must not hang the AppKit main loop.
9. Validate fallback with a build whose presenter gate is `disabled`, and
   capture the structured transition to `mpv-native-window`. Playback must
   remain controllable and the hidden Iced overlay must be dismissed. From the
   same spike and disabled artifacts, select Auto and actually play the direct
   Ferrex SDR stream and `h264-sdr-8bit.mkv` through EOF with seek, pause,
   audio, and stop working. Then select a non-original quality profile so the
   local Ferrex server generates its protected HLS rendition; wait for the job
   to complete and verify Auto reloads the credential-free manifest URI with
   its playback-scoped header and plays every protected segment. Repeat that
   application path through an HTTPS Ferrex base URL whose hostname and
   certificate chain are trusted by the bundled Mozilla CA set (a loopback
   self-signed certificate is not this test). Keep the authenticated-HTTP and
   strict-HTTPS results separate. Confirm Auto selects GStreamer, and correlate
   the run with the same artifact's closure-audit output proving its packaged
   H.264/AAC, demux, network/TLS, audio-sink, and appsink factories; runtime
   diagnostics do not enumerate factories. Selection or the helper-only HTTPS
   smoke alone is not a rollback pass.
10. After exploratory checks pass, complete one uninterrupted 100-cycle
    load/attach/fullscreen/stop/close run. Monitor process, native-window, and
    GPU resources against the provisional budget below; the lower-level
    native-window stress test above does not exercise the attached overlay.

### Presenter stress budget

Declare the following provisional budget before the 100-cycle run. Establish
the baseline after cycle 10 and 30 seconds of quiescence. Sample again after
cycles 20–80 in ten-cycle increments, after every cycle from 81–100, and after
a final 30-second quiescence. Do not restart the process between samples.

- Final working set/RSS, private committed bytes, and virtual address-space
  size must each be no more than 64 MiB above the stabilized baseline; the
  post-baseline peak must be no more than 128 MiB above it. Windows
  `VirtualMemorySize64` and macOS VSZ are address-space measurements, not
  committed-memory measurements.
- Windows handle count must finish at no more than baseline +8. macOS open file
  descriptors must finish at no more than baseline +4.
- GPU process memory must return to no more than baseline +64 MiB after final
  quiescence, with no live decoder, swapchain, or video texture from a stopped
  generation.
- Native player/overlay window count and ownership/child relationships must
  return exactly to baseline after every stop. Any orphan window, second
  taskbar/Dock identity, or stale native relationship fails immediately.
- The final 20 quiescent samples must not be monotonically non-decreasing with
  a net increase of at least 1 MiB, one handle/FD, or one native/GPU object.
  Any limit breach, crash, hang, fallback, or diagnostics from a stale
  generation fails the run.

On Windows, capture the process counters at each sample with this PowerShell
snippet and save the objects as CSV; use Task Manager, Process Explorer, PIX,
or an equivalent reviewed tool for GPU and HWND relationship evidence:

```powershell
$p = Get-Process ferrex-player -ErrorAction Stop
[pscustomobject]@{
  Utc = [DateTime]::UtcNow.ToString('o')
  WorkingSet64 = $p.WorkingSet64
  PrivateMemorySize64 = $p.PrivateMemorySize64
  VirtualMemorySize64 = $p.VirtualMemorySize64
  HandleCount = $p.HandleCount
  MainWindowHandle = $p.MainWindowHandle
}
```

On macOS, record RSS/VSZ and open descriptors at the same cadence, and use
Activity Monitor plus Instruments/Quartz Debug (or an equivalent reviewed
tool) for GPU and AppKit child-window evidence:

```bash
pid="$(pgrep -n ferrex-player)"
date -u +%Y-%m-%dT%H:%M:%SZ
ps -o pid=,rss=,vsz= -p "$pid"
lsof -a -p "$pid" -Ff | awk '/^f[0-9]+$/ { count++ } END { print count+0 }'
```

### Result record and pass boundary

Create one directory per run beneath
`target/native-playback-results/<environment-id>/<UTC-run-id>/`. Save a short
`summary.md`, redacted Ferrex output, package/fixture hashes, resource samples,
and screenshots or screen recordings only when they contain no private media,
server address, account name, token, header, cookie, or machine-identifying
path. A useful diagnostic launch is:

```bash
FERREX_MPV_LOG_LEVEL=trace \
RUST_LOG=ferrex_player_playback=trace,ferrex_player_mpv=trace,ferrex_player_ui=debug \
  cargo run -p ferrex-player --features mpv
```

Use the equivalent PowerShell environment variables on Windows. The run passes
the **handoff validation** when all required operations succeed, the package
closure audit passes on the same artifact, no pointer/handle or credential is
present in retained diagnostics, and every predeclared 100-cycle resource
budget above passes. Only then update the P5/P6 production decision; HDR/EDR,
hardware-decoding, and Auto capability flags must reflect the recorded
evidence instead of the build target.

## Initial platform and protocol inventory

Use stable environment IDs in results instead of hostnames or user names. The initial inventory requires at least these classes before an Auto rollout decision:

| Environment ID | Window system/compositor | GPU/driver class | Display gate | Primary purpose |
|---|---|---|---|---|
| `wl-wlroots-amd` | wlroots/Hyprland family | AMD Mesa | SDR + HDR-capable output when available | Wayland bridge, dmabuf, explicit sync, color management |
| `wl-kde-intel` | KDE Wayland | Intel Mesa | SDR and fractional scale | configure/scale/output transitions |
| `wl-gnome-intel` | GNOME Wayland | Intel Mesa | SDR | protocol compatibility and fallback |
| `wl-nvidia` | supported Wayland compositor | NVIDIA proprietary | SDR + HDR when available | interop and explicit-sync behavior |
| `x11-composited` | X11 with compositor | any supported GPU | SDR | transparent overlay presenter |
| `x11-uncomposited` | X11 without compositor | any supported GPU | SDR | deterministic `wid`/native-window fallback |
| `windows-sdr` | supported Windows | Intel/AMD/NVIDIA | SDR | HWND, DPI, focus, taskbar, gpu-next |
| `windows-hdr` | supported Windows | HDR-capable adapter | HDR enabled | overlay-visible/hidden HDR gate |
| `macos-apple` | macOS 15+ | Apple Silicon | SDR + EDR when available | AppKit, Spaces, fullscreen, VideoToolbox |
| `macos-intel` | macOS 15+ | Intel | SDR | fallback and teardown compatibility |

A run record must include:

- Ferrex revision and fixture `manifest.json` plus `SHA256SUMS` hashes;
- mpv/client API, FFmpeg, libplacebo, VO, GPU context/API, adapter, and `hwdec-current`;
- OS, kernel/build, window system, compositor, GPU, driver, monitor, refresh rate, scale, and HDR state;
- direct versus transcoded source and selected backend/presenter;
- pass/fail for load, first frame, pause, seek, tracks, subtitles, chapters, resize, hide/show, fullscreen, stop, close, and fallback; and
- links to redacted logs, performance samples, and protocol traces.

Store local run artifacts under `target/native-playback-results/<environment-id>/<UTC-run-id>/`. Keep a small redacted summary in review documentation when it supports a rollout decision; do not commit large media, traces, or machine-identifying dumps.

### Wayland trace matrix

Capture only on a dedicated test environment and strip sensitive titles/paths. The P7 spike must correlate these fixture operations with protocol traffic:

| Operation | Required protocol evidence |
|---|---|
| Initial map and first frame | registry bindings, surface creation, shell-role virtualization, configure/ack, buffer attach, frame/presentation callback |
| Resize and fractional scale | parent geometry revision, synthetic configure, viewport/buffer scale, output enter/leave |
| HDR10/PQ and HLG | mpv-selected VO/hwdec plus compositor color-management/color-representation traffic |
| Pause/seek | independent native-VO cadence without Iced frame polling; explicit-sync/dmabuf release remains live |
| Fullscreen | Iced top-level transition and synthesized mpv state/configure, with no second real toplevel |
| Stop/VO reload | child role/object teardown before parent destruction and clean generation replacement |
| Missing optional global | explicit capability/fallback reason; no silent CPU frame path |

A single successful compositor run is spike evidence only. D-022 currently selects HYBRID, so W1–W5 are deferred; any future P7 GO still requires the complete matrix and release packaging.

Use the versioned `native_playback_wayland_trace.py` harness to capture the
initial map/control/fullscreen/VO-reload sequence against pinned mpv 0.41.0.
The [native mpv Wayland spike record](/developer/native-mpv-wayland-spike/)
documents the command, redacted artifact schema, initial protocol inventory,
and the current connection-redirection blocker.
