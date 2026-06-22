---
title: "Android and Android TV final QA acceptance packet"
description: "LOW-379 Android and Android TV QA packet covering diagnostics, recovery, playback/watch-state, focus, evidence redaction, and hardware deferrals."
sidebar:
  order: 3
---

> **Evidence status:** This page preserves a dated QA packet. Treat rows marked blocked, pending hardware QA, or release-readiness follow-up as stale evidence until they are rerun on current devices and builds.

This is the fresh Android and Android TV QA evidence packet for the diagnostics-enabled lane. It recreates the useful LOW-154 / LOW-149 acceptance criteria as current documentation and evidence; LOW-154 and LOW-149 remain historical references only and were not modified in code, docs, or Linear state.

## Current disposition

| Area | Disposition | Evidence / follow-up |
| --- | --- | --- |
| Android mobile + TV build, unit, lint | Pass | Full Gradle assemble/unit/lint command below returned `BUILD SUCCESSFUL`; XML totals were `136 tests, 0 failures, 0 errors, 0 skipped` for each flavor. |
| Focused diagnostics tests | Pass | Diagnostics-focused mobile and TV unit command returned `BUILD SUCCESSFUL`; each flavor ran `DiagnosticsCoreTest` `7/7` and `DiagnosticsUiModelsTest` `3/3`. |
| Repository Rust baseline | Pass | `cargo fmt`, workspace `cargo check`, and `ferrex-core` lib tests passed; `cargo check` emitted existing `ferrexctl` missing-doc warnings only. |
| Device / emulator manual QA | Blocked in this workspace | `adb devices -l` returned no attached devices. The SDK emulator binary cannot start on this NixOS workspace due to the stub-ld dynamic-loader error, and no Android TV image/AVD is configured. Physical phone/TV evidence remains a release-readiness follow-up. |
| Screenshots / video / logcat | Not captured | No device was attached. Capture commands and redaction requirements are included below for the release-readiness run. |
| Codegen | Not run | No FlatBuffers schema, generated Kotlin, or Rust contract/codegen files were touched by this docs-only packet. |

## Exact validation commands and results

Run date: 2026-06-14 from `/home/lowband/dev/workspaces/ferrex/LOW-379`.

### Repository baseline

```bash
cargo fmt --all --check
```

Result: pass; command exited successfully with no output.

```bash
nix develop .#ferrex-player --command cargo check --workspace --all-targets
```

Result: pass; finished `dev` profile in `23.30s`. Existing `ferrexctl` missing-documentation warnings were emitted; no errors.

```bash
nix develop .#ferrex-player --command cargo test -p ferrex-core --lib
```

Result: pass; `160 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.89s`.

### Android mobile + TV assemble / unit / lint

```bash
cd mobile/android && ANDROID_HOME=/home/lowband/Android/Sdk ANDROID_SDK_ROOT=/home/lowband/Android/Sdk ./gradlew :app:assembleMobileDebug :app:assembleTvDebug :app:testMobileDebugUnitTest :app:testTvDebugUnitTest :app:lintMobileDebug :app:lintTvDebug --no-daemon --stacktrace -Pandroid.aapt2FromMavenOverride=/home/lowband/Android/Sdk/build-tools/35.0.0/aapt2
```

Result: pass; `BUILD SUCCESSFUL in 8s`; `105 actionable tasks: 49 executed, 56 from cache`. Lint reports were generated at:

- `mobile/android/app/build/reports/lint-results-mobileDebug.html`
- `mobile/android/app/build/reports/lint-results-tvDebug.html`

Unit XML totals after the run:

| Task | XML suites | Result |
| --- | ---: | --- |
| `testMobileDebugUnitTest` | 26 | `136 tests, 0 failures, 0 errors, 0 skipped` |
| `testTvDebugUnitTest` | 26 | `136 tests, 0 failures, 0 errors, 0 skipped` |

### Focused diagnostics tests

```bash
cd mobile/android && ANDROID_HOME=/home/lowband/Android/Sdk ANDROID_SDK_ROOT=/home/lowband/Android/Sdk ./gradlew :app:testMobileDebugUnitTest :app:testTvDebugUnitTest --tests 'com.ferrex.android.core.diagnostics.DiagnosticsCoreTest' --tests 'com.ferrex.android.core.diagnostics.DiagnosticsUiModelsTest' --no-daemon --stacktrace -Pandroid.aapt2FromMavenOverride=/home/lowband/Android/Sdk/build-tools/35.0.0/aapt2
```

Result: pass; `BUILD SUCCESSFUL in 5s`; `48 actionable tasks: 1 executed, 47 up-to-date`.

Focused XML totals:

| Flavor task | Focused diagnostics tests | Result |
| --- | --- | --- |
| `testMobileDebugUnitTest` | `DiagnosticsCoreTest` `7/7`, `DiagnosticsUiModelsTest` `3/3` | `10 tests, 0 failures, 0 errors, 0 skipped` |
| `testTvDebugUnitTest` | `DiagnosticsCoreTest` `7/7`, `DiagnosticsUiModelsTest` `3/3` | `10 tests, 0 failures, 0 errors, 0 skipped` |

### Device discovery commands

```bash
adb devices -l
adb shell input keyevent KEYCODE_DPAD_CENTER
/home/lowband/Android/Sdk/cmdline-tools/latest/bin/sdkmanager --list_installed | rg '^  system-images|^  emulator|^  platform-tools|^  build-tools;35'
/home/lowband/Android/Sdk/emulator/emulator -list-avds
```

Results:

- `adb devices -l`: only the header `List of devices attached`; no phone, emulator, or TV target.
- D-pad key event: `adb: no devices/emulators found`.
- Installed SDK entries include `build-tools;35.0.0`, `platform-tools 35.0.2`, `emulator 35.2.10`, and only `system-images;android-35;google_apis_playstore;x86_64`; no Android TV / Google TV system image is installed.
- Emulator listing failed before AVD enumeration: NixOS stub-ld dynamic executable error for `/home/lowband/Android/Sdk/emulator/emulator`.

### Codegen / contract commands

Not run for this packet because no FlatBuffers schema, shared mobile codegen script, generated Kotlin under `mobile/android/app/src/main/java/ferrex/`, or Rust contract code was changed. If a later change touches those files, rerun:

```bash
nix shell nixpkgs#flatbuffers -c ./mobile/shared/codegen/generate-kotlin.sh
```

## Device matrix

| Target | Status in this workspace | Required release-readiness follow-up |
| --- | --- | --- |
| Phone emulator or physical Android phone | Blocked: no ADB device/emulator attached. Build/unit/lint evidence passed for `mobileDebug`; no manual phone screenshots/video/logcat captured. | Install `app/build/outputs/apk/mobile/debug/app-mobile-debug.apk` on a phone-class API 28+ emulator/device, run recovery, diagnostics/export, and playback rows below, and attach redacted evidence. |
| Android TV emulator | Blocked: no Android TV/Google TV AVD configured, only a non-TV Android 35 Play Store image is installed, and the emulator binary fails on this NixOS workspace with stub-ld. Build/unit/lint evidence passed for `tvDebug`. | Provision a runnable Android TV / Google TV AVD, install `app/build/outputs/apk/tv/debug/app-tv-debug.apk`, run D-pad/OK/Back focus rows below, and attach redacted evidence. |
| Physical Android TV + remote | Blocked: `adb devices -l` returned no attached physical TV/remote target. | Run the TV matrix on physical hardware before promoting Android TV beyond `dev`; record model, API level, Android release, resolution, remote type, and redacted captures. |

## Recovery scenario matrix

Manual result status for every row is **ready / blocked**: the recovery behavior is documented, build/unit coverage passed where available, and the exact manual pass criteria are defined, but this workspace had no attached device/emulator for interactive execution.

| Scenario | Current automated/source evidence | Manual pass criteria | Manual result |
| --- | --- | --- | --- |
| First install | `FerrexNavGraph` and `TvFerrexNavGraph` route no-server state to server URL entry; `TvAuthRecoveryPolicyTest` covers initial TV server focus. | Fresh install opens explicit server setup, no blank screen/spinner, Back behavior is safe, diagnostics/recovery exits appear when serious failures occur. | Blocked: no device. |
| Bad server URL | Recovery models and auth policy tests cover failed connection focus/retry; phone and TV recovery panels expose retry/change/reset. | Invalid URL or failed validation returns actionable copy with Retry/Connect, Change server or Reset connection; no OS app-data wipe required. | Blocked: no device. |
| Offline server | Recovery action panels are wired on phone home/recovery/detail/search/player and TV home/grid/detail/search/player surfaces. | Previously configured offline server shows retry plus sign-out/change-server/reset diagnostics paths; cached browse data remains usable where present. | Blocked: no device. |
| Expired session | `TokenRefreshAuthenticatorTest`, `AuthManagerTest`, and playback auth-failure tests run in the full Android unit gate. | 401/403 refresh failure returns to login/recovery with sign in, sign out, change server, reset connection, and diagnostics; no trapped authenticated state. | Blocked: no device. |
| Revoked session | Auth/session invalidation paths are covered by auth and playback session-invalidated tests. | Revoked tokens are cleared or invalidated locally; login/recovery copy explains the session cannot be refreshed and does not require OS app-data wipe. | Blocked: no device. |
| Setup / registration copy | Phone login reason copy documents setup-required/registration-closed states; TV policy starts focus on recovery actions for fatal login states. | Setup-required and registration-closed states show explanatory copy and recovery actions, not fake setup/PIN flows. | Blocked: no device. |
| Sign out | Phone and TV recovery/action panels expose Sign out; auth tests cover token clearing. | Sign out clears local auth/session, preserves server URL when expected, returns to login/recovery, and keeps change/reset available. | Blocked: no device. |
| Change server | Phone and TV panels expose Change server from auth, home, cache, detail, search, and playback errors. | Change server returns to server URL entry and only adopts a new URL after successful validation. | Blocked: no device. |
| Reset connection | Phone and TV panels expose Reset connection; cache/recovery docs and tests cover server/user scoped cache clearing. | Reset clears saved connection and scoped local caches, returns to server URL entry with explanatory copy, and avoids Android OS app-data wipe. | Blocked: no device. |
| Clear selected cache | Phone and TV library recovery panels expose selected-cache clear when a library is selected. | Selected library cache can be cleared and resynced without clearing auth/server/all app data. | Blocked: no device. |
| Clear all cache | Home/grid recovery supports all-cache clear actions where state allows. | All library/image cache can be cleared from in-app recovery and resynced; auth/session and diagnostics are not silently deleted. | Blocked: no device. |
| Clear diagnostics only | `DiagnosticsCoreTest.clearDiagnosticsLeavesAuthAndAppCachesUntouched` and `DiagnosticsUiModelsTest.reducerKeepsExportFailuresRetryableAndSeparatesClearConfirmation` passed for mobile and TV. | Clear diagnostics confirmation states that only diagnostic logs, retained crashes, and prior exports are deleted; auth, server, library cache, image cache, and playback state remain intact. | Blocked: no device. |

## Diagnostics / export scenario matrix

| Scenario | Current evidence | Manual pass criteria | Manual result |
| --- | --- | --- | --- |
| Reachability from settings/home | Phone home exposes `Settings & Diagnostics`; TV home exposes `Settings & Diagnostics` with focus key `settings-diagnostics`. | From authenticated home, open diagnostics with touch/keyboard on phone and D-pad/OK on TV. | Blocked: no device. |
| Reachability from serious error screens | Phone recovery/detail/search/player and TV home/grid/detail/search/player error/recovery panels expose `Diagnostics / Export diagnostics`. | From bad server, cache miss, detail failure, search failure, and playback error panels, diagnostics are reachable without OS app-data wipe. | Blocked: no device. |
| Diagnostics screen content | `DiagnosticsSummaryPresenter` renders app/build, device/display, server, auth, cache, playback, crash, and privacy rows; `TvDiagnosticsScreen` and `PhoneDiagnosticsScreen` share the same reducer/core. | Summary loads on phone and TV; Back remains available; TV action panel autofocus does not strand D-pad focus. | Blocked: no device. |
| Export artifact contents | `DiagnosticsCoreTest.exportBundleIncludesManifestLogsAndCrashFilesWithRedaction` passed. `DiagnosticsExportBuilder` writes `manifest.json`, `diagnostics.txt`, `logs/diagnostic-log.txt`, and `crashes/crash-*.txt` entries. | Export creates a `.zip`; unpack and verify manifest, human summary, diagnostic log, and retained crash files are present as applicable. | Blocked: no device. |
| Redaction spot-checks | `DiagnosticsCoreTest.redactorCoversHeadersQueriesJsonAndBodySecrets`, `throwableRedactionRemovesSecretsFromMessagesAndStackTraceText`, `authAndCacheSummariesAvoidRawIdentityAndCountCheapCacheFacts`, and `playbackLogBridgeUsesSharedRedactorAndFeedsExportableLog` passed. | Search exported files and screen text for raw server secrets, auth headers, tokens, passwords, PIN/private material, session IDs, local device IDs, usernames/display names where sensitive, and playback ticket URLs before attaching. | Blocked: no device. |
| Clear diagnostics | Focused diagnostics tests passed for clear confirmation and preservation. | Clear diagnostics removes prior exports/logs/crashes and then refreshes summary; server URL, auth/session, library/image cache, and playback state still work. | Blocked: no device. |
| Crash-file retention | `DiagnosticsCoreTest.crashRetentionRedactsBoundsAndPrunesOldFiles` passed. Crash files are bounded/pruned and included in exports. | If a retained crash exists, diagnostics shows the count and export includes redacted crash files; clearing diagnostics removes them. | Blocked: no device. |
| Share sheet behavior | `DiagnosticsExportShare` restricts sharing to `.zip` files under `diagnostics/exports` with `application/zip` and read URI permission; UI reports `ActivityNotFoundException` as retryable copy. | Share sheet opens for an export target; if no target exists, the screen remains recoverable and reports the failure. | Blocked: no device. |

## Playback / watch-state scenario matrix

Source matrix: [Android playback QA matrix](/reference/qa/android-playback-matrix/). Manual phone/TV playback remains blocked by the absent device/emulator; automated evidence listed below passed in the full Android unit gate.

| Scenario | Automated/fake evidence | Phone manual result | TV manual result | Pass criteria |
| --- | --- | --- | --- | --- |
| First play | `PlaybackFoundationTest.fetchTicketUsesAuthenticatedTicketRouteAndBuildsTicketedStreamUrl` and ready-state tests | Blocked | Blocked | Detail/cache entry starts playback without exposing session tokens. |
| Resume | `serverResumeProgressLoadsBeforeTicketedPlaybackWhenNoExplicitStartWasChosen` | Blocked | Blocked | Resume starts from server watch position. |
| Start over | Start-over skip-resume tests in `PlaybackFoundationTest` | Blocked | Blocked | Playback starts at 0s and does not reuse stale progress. |
| Pause | Progress-write tests in `PlaybackFoundationTest` | Blocked | Blocked | Pause keeps UI recoverable and commits current progress. |
| Exit / Back | Exit progress write tests and `TvPlaybackOverlayReducerTest` | Blocked | Blocked | Back/exit returns to detail/home, saves progress when duration is known, and avoids app-data wipe. |
| Completion | `onPlaybackEnded` progress test | Blocked | Blocked | Completion commits ended position and returns to usable detail/home state. |
| Progress refresh | Progress commit callbacks and watch-state tests in full unit gate | Blocked | Blocked | Detail/home watch badges update after progress or watched/unwatched mutation. |
| Ticket/session expiry | Ticket 401/403 retry/invalidation and resume/progress auth-failure tests | Blocked | Blocked | Expiry retries within limits, then returns to sign-in/change-server recovery instead of trapping the player. |
| Server unreachable | Ticket network retry-limit tests | Blocked | Blocked | Shows retry/change-server/sign-out recovery after bounded retries. |
| Missing file / offline library | 404/503 playback failure tests | Blocked | Blocked | Shows retry/change-server/sign-out recovery and does not clear app data. |
| Audio track cases | `PlaybackTrackOptionsTest.audioOptionsFormatLabelsLanguagesAndCapabilityWarningsAcrossGroups` | Blocked | Blocked | Multiple/unsupported audio streams show labels, support warnings, and safe selectable/disabled states. |
| Subtitle cases | `PlaybackTrackOptionsTest.subtitleOptionsAlwaysOfferOffAndFormatRolesAndFallbacks` and `subtitleOffIsSelectedWhenTextDisabledOrNoTextTracksExist` | Blocked | Blocked | Subtitles offer Off, selectable text tracks can be enabled, and empty/unsupported tracks remain graceful. |
| Track diagnostics | `PlaybackTrackOptionsTest.diagnosticsSummarizeTrackGroupsWithoutUrls` | Blocked | Blocked | Diagnostics summarize track groups without URLs or playback tickets. |

## Android TV 10-foot focus scenario matrix

Source matrix: [Android TV 10-foot QA matrix](/reference/qa/android-tv-10ft-matrix/). Manual TV D-pad execution remains blocked by the absent TV emulator/device; source/unit evidence listed below passed in the full Android unit gate.

| Surface / scenario | Automated/source evidence | Manual TV result | Pass criteria |
| --- | --- | --- | --- |
| Server / login / recovery | `TvAuthRecoveryPolicyTest`; `TvDiagnosticsScreen` and TV recovery panels share `TvActionPanel` focus restoration. | Blocked | Initial focus is deterministic; Retry, Sign out, Change server, Reset connection, Back, and diagnostics remain reachable. |
| Home | `TvHomeFocusPolicy`, `TvFocusRestoreModelTest`, and TV home source expose Settings & Diagnostics and recovery panels. | Blocked | D-pad traverses actions, shelves, library chooser, rows/grids, and recovery controls with visible focus. |
| Library grids | TV grid/detail source plus full build/lint. | Blocked | Grid cards, filters/sorts where available, retry/cache-clear/change/reset actions, and Back-to-Home behavior remain reachable. |
| Search | TV search rows and cache-miss diagnostics actions; full build/lint. | Blocked | Search field, retry, clear, result rows, cache-miss retry, diagnostics, and Back-to-Home are reachable. |
| Detail | TV detail source plus playback/watch-state tests. | Blocked | Detail starts with safe Back focus, launches Resume/Start over, exposes watch actions, recovery, diagnostics, and origin restore. |
| Player controls | `TvPlaybackOverlayReducerTest` and playback source. | Blocked | Hidden controls show on D-pad, OK toggles, Back first restores controls then exits, and recovery actions appear on errors. |
| Tracks | `PlaybackTrackOptionsTest` and `TvPlaybackOverlayReducerTest`. | Blocked | Audio/subtitle pickers have bounded D-pad traversal, selected/disabled states, Back dismissal, and safe focus restoration. |
| Dialogs / confirmations | Diagnostics clear confirmation and TV action panel source. | Blocked | Confirmation panels keep focus in reachable actions and always include cancel/back. |
| Errors | Recovery panels in TV auth/home/grid/detail/search/player source. | Blocked | Error panels focus the first useful recovery action and never require OS app-data wipe. |
| Back behavior | `TvAuthRecoveryPolicyTest`, `TvFocusRestoreModelTest`, `TvPlaybackOverlayReducerTest`. | Blocked | Auth/recovery consumes Back, grid/search/detail return to parent, player uses two-step Back, Home uses system Back. |
| Post-reset navigation | TV auth/recovery source and 10-foot matrix. | Blocked | Reset returns to server URL field, login/Home focus recovers after reconnect, and reset can be repeated. |
| Diagnostics screen | `TvDiagnosticsScreen`, `DiagnosticsUiModelsTest`, focused diagnostics tests. | Blocked | Diagnostics action panel autofocuses Export/Clear/Back, share failures are recoverable, clear confirmation preserves non-diagnostic state. |

## Accepted deferrals and follow-ups

- **Physical Android TV / remote run:** unavailable in this workspace; release-readiness follow-up before promoting Android TV beyond `dev`.
- **Phone emulator / physical phone run:** unavailable in this workspace; release-readiness follow-up before claiming manual phone UX evidence.
- **Android TV emulator run:** unavailable because no TV system image/AVD is configured and the SDK emulator binary fails under NixOS stub-ld in this workspace.
- **Screenshots / video / logcat captures:** not obtained because no device/emulator was attached. Capture and redact evidence using the guidance below.
- **External subtitle discovery:** out of scope for this QA packet. Current evidence covers Media3 subtitle track selection/toggle once tracks are present, not discovery of sidecar/external subtitle files.
- **Release-readiness media fixtures:** hardware playback still needs real media fixtures for unsupported/exceeds-capability audio, subtitles, missing-file/offline-library recovery, and server expiry cases.

## Screenshot, video, logcat, and export redaction guidance

When a phone, emulator, or TV is available, record the target details before capturing evidence:

```bash
adb devices -l
adb shell getprop ro.build.version.sdk
adb shell getprop ro.build.version.release
adb shell getprop ro.product.model
adb shell wm size
```

Useful capture commands:

```bash
# Screenshot
adb exec-out screencap -p > ferrex-android-qa-screenshot.png

# Short video; stop with Ctrl-C or after the scenario completes.
adb shell screenrecord /sdcard/ferrex-android-qa.mp4
adb pull /sdcard/ferrex-android-qa.mp4 ./ferrex-android-qa.mp4
adb shell rm /sdcard/ferrex-android-qa.mp4

# Logcat snapshot
adb logcat -c
# Reproduce the scenario, then:
adb logcat -d -v threadtime > ferrex-android-qa-logcat.txt

# Optional full bugreport, only if the reviewer requests it.
adb bugreport ferrex-android-qa-bugreport.zip
```

Before attaching any screenshot, video, logcat, bugreport, or diagnostics export, redact or replace:

- Server URLs when they are sensitive, including hostnames, base paths, LAN IPs, and reverse-proxy paths.
- Usernames, display names, avatars, account names, and any other identifying profile data.
- `Authorization`, `Proxy-Authorization`, cookie, bearer/basic auth, refresh/access token, API key, session, device-session, local-device, and account/device identifier values.
- Playback ticket URLs and query parameters such as `access_token`, `ticket`, `device_session_id`, and stream IDs when they identify private media.
- Passwords, PINs, PIN proofs, setup tokens, private keys, signatures, and registration/private material.
- Local filesystem paths if they reveal private usernames, library names, or hostnames.

For exported diagnostics zips, unpack into a scratch directory and spot-check before attaching:

```bash
mkdir -p /tmp/ferrex-diagnostics-review
unzip ferrex-diagnostics-*.zip -d /tmp/ferrex-diagnostics-review
rg -n "Authorization|Bearer|refresh_token|access_token|password|pin|ticket|session|device|private_key|BEGIN PRIVATE" /tmp/ferrex-diagnostics-review
```

The expected result is either no matches or matches whose values are `<redacted>` / hashed safe summaries.

## Source mapping

### Files changed by this packet

- [Android / Android TV final QA acceptance packet](/reference/qa/android-final-qa/) — new QA evidence packet.

### Current repository files consulted

- [Android playback QA matrix](/reference/qa/android-playback-matrix/) — existing playback/watch-state matrix incorporated into the final packet.
- [Android TV 10-foot QA matrix](/reference/qa/android-tv-10ft-matrix/) — existing 10-foot focus matrix incorporated into the final packet.
- `mobile/android/app/src/main/kotlin/com/ferrex/android/core/diagnostics/AndroidDiagnostics.kt`
- `mobile/android/app/src/main/kotlin/com/ferrex/android/core/diagnostics/DiagnosticsExportBuilder.kt`
- `mobile/android/app/src/main/kotlin/com/ferrex/android/core/diagnostics/DiagnosticsExportShare.kt`
- `mobile/android/app/src/main/kotlin/com/ferrex/android/core/diagnostics/DiagnosticsFiles.kt`
- `mobile/android/app/src/main/kotlin/com/ferrex/android/core/diagnostics/DiagnosticsRedactor.kt`
- `mobile/android/app/src/main/kotlin/com/ferrex/android/core/diagnostics/DiagnosticsUiModels.kt`
- `mobile/android/app/src/main/kotlin/com/ferrex/android/ui/diagnostics/PhoneDiagnosticsScreen.kt`
- `mobile/android/app/src/tv/kotlin/com/ferrex/android/tv/ui/TvDiagnosticsScreen.kt`
- `mobile/android/app/src/main/kotlin/com/ferrex/android/ui/home/PhoneHomeScreen.kt`
- `mobile/android/app/src/main/kotlin/com/ferrex/android/ui/recovery/PhoneRecoveryScreens.kt`
- `mobile/android/app/src/tv/kotlin/com/ferrex/android/tv/ui/TvHomeScreen.kt`
- `mobile/android/app/src/main/kotlin/com/ferrex/android/ui/player/PlayerScreen.kt`
- `mobile/android/app/src/test/kotlin/com/ferrex/android/core/diagnostics/DiagnosticsCoreTest.kt`
- `mobile/android/app/src/test/kotlin/com/ferrex/android/core/diagnostics/DiagnosticsUiModelsTest.kt`
- `mobile/android/app/src/test/kotlin/com/ferrex/android/core/playback/PlaybackFoundationTest.kt`
- `mobile/android/app/src/test/kotlin/com/ferrex/android/core/playback/PlaybackTrackOptionsTest.kt`
- `mobile/android/app/src/test/kotlin/com/ferrex/android/core/playback/TvPlaybackOverlayReducerTest.kt`
- `mobile/android/app/src/test/kotlin/com/ferrex/android/core/tvfocus/TvAuthRecoveryPolicyTest.kt`
- `mobile/android/app/src/test/kotlin/com/ferrex/android/core/tvfocus/TvFocusRestoreModelTest.kt`
- `mobile/android/app/build.gradle.kts` and Android manifests for package/flavor/launcher details.

### Intake / historical reference files consulted

- `feat/android@intake:docs/specs/mobile-apps-android.md` — Android architecture/build/codegen baseline reference.
- `feat/android@intake:docs/specs/android-tv-spike.md` — TV product-flavor and Leanback shell reference.
- `feat/android@intake:mobile/android/app/src/main/kotlin/com/ferrex/android/core/diagnostics/DiagnosticLog.kt` — historical diagnostics log reference only; current diagnostics implementation was reviewed from the active workspace files above.
- `symphony/issue/LOW-149:docs/specs/android-tv-dev-acceptance.md` — historical Android/TV dev acceptance checklist used to recreate criteria here as fresh documentation.

LOW-154 and LOW-149 were treated as historical references only. No LOW-154/LOW-149 artifact was revived, edited, or used to mutate Linear issue state.
