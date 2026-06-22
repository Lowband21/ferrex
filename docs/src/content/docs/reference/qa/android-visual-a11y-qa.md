---
title: "Android visual QA and accessibility runbook"
description: "Android phone and Android TV visual/accessibility evidence runbook, automated contrast/tag tests, screenshot runner, and redaction requirements."
sidebar:
  order: 8
---

> **Evidence status:** This page preserves a dated QA packet. Treat rows marked blocked, pending hardware QA, or release-readiness follow-up as stale evidence until they are rerun on current devices and builds.

This runbook covers the Android redesign visual/accessibility evidence path for phone and Android TV. It is intentionally lightweight: source exposes stable Compose `testTag` hooks and content descriptions, while unit tests verify the shared color/status contracts without adding instrumented Compose UI-test dependencies.

## Automated evidence

Run from the repository root unless noted.

```bash
cd mobile/android
ANDROID_HOME=/home/lowband/Android/Sdk ANDROID_SDK_ROOT=/home/lowband/Android/Sdk ./gradlew :app:testMobileDebugUnitTest :app:testTvDebugUnitTest --tests 'com.ferrex.android.ui.theme.FerrexDesignTokensTest' --tests 'com.ferrex.android.ui.qa.FerrexVisualQaScenariosTest' --no-daemon --stacktrace -Pandroid.aapt2FromMavenOverride=/home/lowband/Android/Sdk/build-tools/35.0.0/aapt2
```

Expected result: `BUILD SUCCESSFUL`. The focused test verifies:

- tokenized text/action/status contrast pairs meet WCAG AA normal-text contrast (`4.5:1`) after alpha containers are composited over the dark Ferrex panel background;
- every `FerrexActionRole` maps to the expected `FerrexStatusTone`;
- deterministic visual-QA samples expose unique stable tags, descriptions, manual evidence paths, and synthetic media fixtures;
- phone and TV library-grid scenarios explicitly document compact controls plus dense grids, with enough synthetic poster cards to catch regressions back to vertical huge-card lists;
- dynamic phone/TV tag builders sanitize spaces, punctuation, and mixed case into predictable namespaces;
- phone and TV debug QA scenarios cover the migrated flat Home, Library, Search, Detail, Player, Recovery, and Diagnostics surfaces.

Full Android CI-compatible gate after source changes:

```bash
cd mobile/android
ANDROID_HOME=/home/lowband/Android/Sdk ANDROID_SDK_ROOT=/home/lowband/Android/Sdk ./gradlew :app:assembleMobileDebug :app:assembleTvDebug :app:testMobileDebugUnitTest :app:testTvDebugUnitTest :app:lintMobileDebug :app:lintTvDebug --no-daemon --stacktrace -Pandroid.aapt2FromMavenOverride=/home/lowband/Android/Sdk/build-tools/35.0.0/aapt2
```

No generated screenshots, videos, bugreports, logcats, or local Gradle artifacts should be committed.

## Host-side scenario screenshot runner

After building and installing the debug APKs with `just android-qa-build` and `just android-qa-install all`, capture the deterministic debug scenario/profile matrix from the repository root:

```bash
./scripts/qa/android-visual-qa.sh list --target all
./scripts/qa/android-visual-qa.sh gate --mode complete --output-dir target/android-visual-qa/theater-plate
./scripts/qa/android-visual-qa.sh accessibility --target all --scenario all --output-dir target/android-visual-qa/theater-plate-a11y
```

The runner reads the debug scenario registry in `FerrexVisualQa.kt`, launches each scenario through `com.ferrex.android.action.VISUAL_QA` on explicit phone/TV serials, applies the selected viewport profile with `wm size`/`wm density`, drives TV D-pad focus keys for the TV focus scenarios, captures prepared-run screenshots with direct `adb -s <serial> exec-out screencap -p` by default, and writes stable PNG paths plus `manifest.json` under the requested output directory:

- `phone-portrait` validates screenshots as `1080x2400`.
- `phone-landscape-foldable` validates screenshots as `1800x1200`.
- `tv-1080p` validates screenshots as `1920x1080`.
- `tv-4k-scaled` applies a `3840x2160` logical viewport/density override while validating the emulator framebuffer as `1920x1080`.
- `<output>/<profile>/<scenario-id>.png` records the profile name, logical `wm size` override, and expected screenshot dimensions in `manifest.json` for every capture.
- `manifest.json` records command/tool versions, APK/package metadata, serial metadata, scenario IDs, viewport profiles, dimensions, paths, timestamps, screenshot method, command category, output path, capture timing, and whether helper compatibility mode was requested or used.
- Smoke gates attempt one helper-compatible screenshot comparison for a default emulator profile when the helper is available; otherwise the manifest records why the helper comparison was unavailable while keeping the main evidence on the fast path.
- Transient PNG dimension mismatches are retried after reapplying the viewport; invalid attempts are preserved as `<scenario-id>.attempt-<n>.invalid.png` and recorded in `screenshot_validation_attempts`.
- Failed captures include bounded redacted logcat snippets under `<output>/logs/`; redaction removes authorization values, bearer/basic tokens, token/ticket/password fields, URL origins, and private LAN origins.

Use `./scripts/qa/android-visual-qa.sh list --target all` to list the current matrix. The flat UI coverage scenarios include `phone-home`, `phone-browse-grid`, `phone-search`, `phone-movie-detail`, `phone-playback-entry`, `phone-recovery-offline-stale`, `phone-diagnostics`, `tv-home-focus`, `tv-grid-focus`, `tv-search-focus`, `tv-detail-focus`, `tv-recovery-focus`, `tv-diagnostics-focus`, and the phone/TV Theater Plate states including `*-theater-plate-playback-entry`. `--profile` can narrow the default all-profile matrix, for example `--profile phone-portrait --profile tv-1080p`. `--screenshot-mode fast` is the default prepared-run path; use `--screenshot-mode helper-compatible` only when explicitly checking the legacy Nix screenshot helper behavior for default emulator profiles. Hardware confirmation is opt-in only and requires an explicit serial, for example `--hardware --hardware-serial "$SERIAL"` or `FERREX_ANDROID_HARDWARE_SERIAL`; no physical device serials, IPs, or server origins are committed as defaults.

The accessibility subcommand writes `accessibility-manifest.json` plus UI Automator XML dumps under `<output>/accessibility/`. It fails when required recovery/action/status/focus/media nodes are missing stable tags, content descriptions, button roles, onClick/clickable affordances, TV focusability, or disabled semantics for playback-entry actions that cannot launch without a ticket.

Expected local artifact paths (all ignored by source control) are:

- `target/android-visual-qa/theater-plate/manifest.json`
- `target/android-visual-qa/theater-plate/<profile>/<scenario-id>.png`
- `target/android-visual-qa/theater-plate/logs/<profile>-<scenario-id>-failure-logcat.txt` only on failed captures
- `target/android-visual-qa/theater-plate-a11y/accessibility-manifest.json`
- `target/android-visual-qa/theater-plate-a11y/accessibility/<profile>/<scenario-id>.xml`
- `target/android-visual-qa/theater-plate-a11y/logs/<profile>-<scenario-id>-accessibility-logcat.txt` only on failed accessibility checks

If a profile cannot be captured in a given workspace, the manifest verifier only accepts an explicit human deferral recorded in `profile_deferrals`, for example:

```json
{
  "target": "tv",
  "profile": "tv-4k-scaled",
  "human_deferred": true,
  "reason": "No 4K-scaled Android TV emulator was attached in this workspace."
}
```

## Stable tag map for UI tests/manual QA

The shared tag constants live in `mobile/android/app/src/main/kotlin/com/ferrex/android/ui/qa/FerrexVisualQa.kt`.

Key phone tags:

- `phone.shell`, `phone.shell.nav`, `phone.shell.nav.<destination>`
- `phone.home`, `phone.home.header`, `phone.home.continue-watching`, `phone.home.browse-find`, `phone.home.server-recovery`
- `phone.libraries`, `phone.libraries.tabs`, `phone.libraries.chooser`, `phone.libraries.controls`, `phone.libraries.index-status`, `phone.libraries.grid`, `phone.library.recovery`
- `phone.search`, `phone.search.panel`, `phone.search.field`, `phone.search.actions`, `phone.search.results`
- `phone.detail.movie`, `phone.detail.series`, `phone.detail.season-episode`, `phone.playback-entry`
- `phone.recovery.offline-stale`, `phone.diagnostics`, `phone.diagnostics.actions`, `phone.diagnostics.action.export`, `phone.diagnostics.action.clear`
- `phone.account-server`, `phone.account-server.summary`

Key TV tags:

- top-level: `tv.home`, `tv.search`, `tv.search.field`, `tv.search.results`, `tv.detail`, `tv.diagnostics`
- compact full-grid controls: `tv.surface.grid-top-controls`, `tv.action.grid-top-controls.media-type`, `tv.action.grid-top-controls.library`, `tv.action.grid-top-controls.sort-filter`, `tv.action.grid-top-controls.status-more`
- dense full-grid posters: `tv.surface.grid-cards`, `tv.poster.grid-cards.<stable-item-key>`
- modal grid panels: `tv.surface.grid-movie-controls-panel`, `tv.surface.grid-status-panel`, plus `tv.action.<panel>.<action-key>` for sort/filter/retry/cache/diagnostics exits
- generic focus surfaces: `tv.surface.<surface-key>`
- generic focus actions: `tv.action.<surface-key>.<action-key>`
- poster targets: `tv.poster.<surface-key>.<stable-item-key>`
- diagnostics actions: `tv.action.diagnostics-actions.export`, `tv.action.diagnostics-actions.clear`, `tv.action.diagnostics-actions.back`

Theater Plate debug tags are generated for both `phone` and `tv` targets:

- root: `<target>.theater-plate.<state>`
- status: `<target>.theater-plate.status.<state>`
- actions: `<target>.theater-plate.action.<state>.<action-key>`
- playback-entry disabled ticket action: `<target>.theater-plate.action.playback-entry.network-required`
- media: `<target>.theater-plate.media.<state>.hero`
- rails: `<target>.theater-plate.rail.<state>.primary`
- search field: `<target>.theater-plate.search.search.field`

The required Theater Plate state IDs are `bright`, `dark`, `busy`, `missing-backdrop`, `long-title`, `missing-artwork`, `stale-offline`, `recovery`, `search`, `browse`, `detail`, `rails`, and `playback-entry`. TV focusable surfaces also set explicit semantic content descriptions through `TvFocusableSurface`/`TvFocusableButton`; shared phone action buttons set button content descriptions from their labels.

## Library-grid density contract

The phone and TV library-grid debug scenarios are the visual canaries for the compact-control rewrite:

- `phone-browse-grid` renders compact horizontally scrolling control/status shelves for media type, library, sort/filter, status, More/Recovery, retry, reset, and diagnostics above a `FerrexMobileMediaGrid` dense poster grid. The grid owns vertical scroll below the controls; do not restore first-page caps or full-width card lists.
- `tv-grid-focus` renders compact D-pad top controls (`grid-top-controls`), a dense synthetic 12-card poster grid (`grid-cards`), and modal sort/filter plus status/recovery panels. The cards use dense grid token spacing/copy and stable `tv.poster.grid-cards.*` tags; TV home rails keep their larger 10-foot rail dimensions and must not inherit the dense full-grid treatment.
- Recovery controls remain explicit, tagged, and no-wipe: Retry selected/all, Clear selected/all cache, Change server, Reset connection, and Diagnostics / Export diagnostics stay reachable from the status/More panel.
- Screenshots, UI Automator dumps, logcat snippets, and videos for these scenarios must follow the redaction rules below before attachment.

## Flat UI surface contract

Use the following matrix when reviewing screenshots, UI Automator dumps, or manual device sessions. Decorative cards/rounded panels should not reappear on layout-only sections; status/recovery bands may use flat tinted slabs and dividers, while functional buttons, dialogs, focus rings, and TV D-pad targets keep their interactive shape/focus treatment.

| Surface | Phone flat expectation | TV flat expectation |
| --- | --- | --- |
| Home | Header, Continue Watching, Browse and find, and Server & recovery read as flat sections with stable tags; connection/cache recovery stays inline. | Home actions and shelves use Theater Plate/flat focus surfaces, visible focus scale/border, and no stranded retry/diagnostics exits. |
| Library | Tabs, chooser, sort/filter/status/More controls collapse into compact flat controls; one dense grid owns scroll and media cards keep accessible descriptions. | Compact top controls open modal panels; dense poster grid shows at least the tested 1080p density, stable poster tags, and focused media-art grounding. |
| Search | Field, Retry/Clear, result rows, stale/cache-miss/error cards, and Diagnostics action are tagged and contrast-safe. | Search field, result/retry/clear actions, result rows, and cache-miss recovery are D-pad reachable with `tv.action.search-results.*` tags. |
| Detail | Movie, series, and episode details use flat Theater Plate primitives; phone omits redundant visible Back while keeping playback/cache recovery inline. | Detail starts with a safe Back focus target, preserves play/watch/cache/diagnostics actions, and uses focus-only outlines instead of resting card chrome. |
| Player | Playback-entry/debug state preserves resume/start-over/network-ticket recovery actions and disabled semantics where a ticket is unavailable. | Theater Plate playback-entry and TV playback recovery actions expose disabled network-required semantics, D-pad focus, and Back/change-server/sign-out/diagnostics exits. |
| Recovery | Retry, Sign out, Change server, Reset connection, Diagnostics, and cache repair exits remain visible without asking for OS app-data wipes. | Recovery panels keep all no-wipe exits focusable and tagged; destructive reset uses destructive tone, not hidden platform wipe instructions. |
| Diagnostics | Settings & Diagnostics uses flat status bands, export/share and clear diagnostics/logs actions, no visible Back button, and redacted/safe summaries only. | TV diagnostics keeps export, clear, and Back D-pad actions in `diagnostics-actions`, flat status slabs, and redacted/safe summary copy. |

## Theater Plate migration seams

Route-level redesign remains out of scope for this QA lane. The source-level migration notes in `TheaterPlateComponentMigrationNotes` document the safe seams that LOW-447/LOW-448/LOW-449 should consume later:

- `FerrexStatusCard` can be replaced by `FerrexStageSurface(StatusSlab)` + Theater Plate text roles while preserving `FerrexStatusAction.onClick`, loading, tags, and content descriptions.
- `FerrexActionButton` can be restyled into Theater Plate control shelves while preserving `onClick`, enabled, subtitle, tag, and content-description callbacks for retry/reset/cache/diagnostics/playback actions.
- `FerrexPosterCard` and rail/detail successors can migrate to projection/rail stage surfaces while preserving media-open callbacks and fallback artwork labels for LOW-447 data/rail parity.
- TV focus actions should keep existing D-pad focus restoration keys, action callbacks, tags, and content descriptions while LOW-449 adopts the ten-foot Theater Plate surfaces.

## Manual phone runbook

Prerequisites:

```bash
cd mobile/android
ANDROID_HOME=/home/lowband/Android/Sdk ANDROID_SDK_ROOT=/home/lowband/Android/Sdk ./gradlew :app:assembleMobileDebug --no-daemon --stacktrace -Pandroid.aapt2FromMavenOverride=/home/lowband/Android/Sdk/build-tools/35.0.0/aapt2
adb devices -l
adb install -r app/build/outputs/apk/mobile/debug/app-mobile-debug.apk
adb shell monkey -p com.ferrex.android.debug -c android.intent.category.LAUNCHER 1
```

Record device model, Android/API version, and resolution:

```bash
adb shell getprop ro.product.model
adb shell getprop ro.build.version.release
adb shell getprop ro.build.version.sdk
adb shell wm size
```

Phone paths to capture/verify:

| Path | Expected visual/accessibility evidence |
| --- | --- |
| Home | Header, Continue Watching, Browse and find, and Server & recovery sections are visible; primary cyan and secondary violet copy remain legible on slate surfaces. |
| Libraries | Compact top controls expose Movie/Series, library chooser, movie sort/filter, status, More/Recovery, retry/reset, and diagnostics; one dense grid owns the scroll below the controls without hidden first-page caps or full-width huge-card lists. |
| Search | Query field, Retry/Clear actions, result rows, stale/cache-miss/error cards, and diagnostics action remain visible and tagged. |
| Account & Server | Retry, Change server, Sign out, Reset connection, Diagnostics, and cache recovery exits remain visible; reset/change paths do not require OS app-data wipe. |
| Detail/Player if available | Playback/watch actions and error recovery copy preserve token contrast and expose labeled actions. |
| Diagnostics | Settings & Diagnostics shows flat status bands plus Export / Share diagnostics and Clear diagnostics/logs; no visible Back button is present, Android back remains available, and summaries avoid raw credentials/device IDs. |

## Manual Android TV runbook

Prerequisites:

```bash
cd mobile/android
ANDROID_HOME=/home/lowband/Android/Sdk ANDROID_SDK_ROOT=/home/lowband/Android/Sdk ./gradlew :app:assembleTvDebug --no-daemon --stacktrace -Pandroid.aapt2FromMavenOverride=/home/lowband/Android/Sdk/build-tools/35.0.0/aapt2
adb devices -l
adb install -r app/build/outputs/apk/tv/debug/app-tv-debug.apk
adb shell monkey -p com.ferrex.android.tv.debug -c android.intent.category.LEANBACK_LAUNCHER 1
```

D-pad smoke path:

```bash
adb shell input keyevent KEYCODE_DPAD_DOWN
adb shell input keyevent KEYCODE_DPAD_RIGHT
adb shell input keyevent KEYCODE_DPAD_CENTER
adb shell input keyevent KEYCODE_BACK
```

TV paths to capture/verify:

| Surface | Expected focus/accessibility evidence |
| --- | --- |
| Home actions | Focus ring/scale is visible on Search, Settings & Diagnostics, and Retry when present; semantic labels match button copy. |
| Continue Watching / shelves | Poster cards expose `tv.poster.<surface>.<item>` tags, content descriptions from media titles, and visible focus border/scale. |
| Library grid controls/posters | `grid-top-controls` exposes compact Back, Media, Library, Sort/filter, and Status/More actions; `grid-cards` shows dense poster cards tagged as `tv.poster.grid-cards.<item>` rather than vertical huge-card rows; modal sort/filter and status/recovery panels restore focus and keep no-wipe exits reachable. |
| Search | Search field is focusable, Back/Retry/Clear are tagged, result rows expose `Open <title>` descriptions, and cache-miss recovery actions remain reachable. |
| Detail | Back starts as a safe focus target; playback/watch actions and missing-detail recovery actions are tagged and have visible focus. |
| Recovery/errors | Retry, clear cache, Change server, Reset connection, Sign out, and Diagnostics actions stay reachable without OS app-data wipe. |
| Diagnostics | Export / Share diagnostics, Clear diagnostics/logs, and Back focus targets live in `tv.action.diagnostics-actions.*`; status slabs remain flat and safe summaries avoid raw credentials/device IDs. |

## Screenshot, video, logcat, and redaction requirements

Capture commands when a device is available:

```bash
adb exec-out screencap -p > ferrex-android-visual-qa.png
adb shell screenrecord /sdcard/ferrex-android-visual-qa.mp4
adb pull /sdcard/ferrex-android-visual-qa.mp4 ./ferrex-android-visual-qa.mp4
adb shell rm /sdcard/ferrex-android-visual-qa.mp4
adb logcat -c
# reproduce the scenario
adb logcat -d -v threadtime > ferrex-android-visual-qa-logcat.txt
```

Before attaching any artifact, redact or replace:

- private server URLs, hostnames, LAN IPs, reverse-proxy paths, and library names if sensitive;
- usernames, display names, account/device identifiers, avatars, and local filesystem paths that identify a person or machine;
- authorization headers, cookies, bearer/basic tokens, refresh/access tokens, session IDs, device-session IDs, playback ticket URLs, query parameters such as `access_token`, `ticket`, and private setup/PIN material;
- media titles or artwork if the capture itself should not disclose a private library.

## Current workspace deferral template

If `adb devices -l` returns no phone/TV target, record manual evidence as blocked in the PR/testing notes with this shape:

> Manual phone/TV visual QA deferred: `adb devices -l` returned no attached device/emulator in this workspace. Automated contrast/status/tag unit tests and Gradle assemble/unit/lint gates passed; screenshots/video/logcat remain release-readiness follow-up and must follow the redaction rules above.
