# Android visual QA and accessibility evidence runbook

This runbook covers the Android redesign visual/accessibility evidence path for phone and Android TV. It is intentionally lightweight: source exposes stable Compose `testTag` hooks and content descriptions, while unit tests verify the shared color/status contracts without adding instrumented Compose UI-test dependencies.

## Automated evidence

Run from the repository root unless noted.

```bash
cd mobile/android
ANDROID_HOME=/home/lowband/Android/Sdk ANDROID_SDK_ROOT=/home/lowband/Android/Sdk ./gradlew :app:testMobileDebugUnitTest :app:testTvDebugUnitTest --tests 'com.ferrex.android.ui.theme.FerrexDesignTokensTest' --no-daemon --stacktrace -Pandroid.aapt2FromMavenOverride=/home/lowband/Android/Sdk/build-tools/35.0.0/aapt2
```

Expected result: `BUILD SUCCESSFUL`. The focused test verifies:

- tokenized text/action/status contrast pairs meet WCAG AA normal-text contrast (`4.5:1`) after alpha containers are composited over the dark Ferrex panel background;
- every `FerrexActionRole` maps to the expected `FerrexStatusTone`;
- deterministic visual-QA samples expose unique stable tags, descriptions, and manual evidence paths;
- dynamic phone/TV tag builders sanitize spaces, punctuation, and mixed case into predictable namespaces.

Full Android CI-compatible gate after source changes:

```bash
cd mobile/android
ANDROID_HOME=/home/lowband/Android/Sdk ANDROID_SDK_ROOT=/home/lowband/Android/Sdk ./gradlew :app:assembleMobileDebug :app:assembleTvDebug :app:testMobileDebugUnitTest :app:testTvDebugUnitTest :app:lintMobileDebug :app:lintTvDebug --no-daemon --stacktrace -Pandroid.aapt2FromMavenOverride=/home/lowband/Android/Sdk/build-tools/35.0.0/aapt2
```

No generated screenshots, videos, bugreports, logcats, or local Gradle artifacts should be committed.

## Stable tag map for UI tests/manual QA

The shared tag constants live in `mobile/android/app/src/main/kotlin/com/ferrex/android/ui/qa/FerrexVisualQa.kt`.

Key phone tags:

- `phone.shell`, `phone.shell.nav`, `phone.shell.nav.<destination>`
- `phone.home`, `phone.home.header`, `phone.home.continue-watching`, `phone.home.browse-find`, `phone.home.server-recovery`
- `phone.libraries`, `phone.libraries.tabs`, `phone.libraries.chooser`, `phone.libraries.grid`, `phone.library.recovery`
- `phone.search`, `phone.search.panel`, `phone.search.field`, `phone.search.actions`, `phone.search.results`
- `phone.account-server`, `phone.account-server.summary`

Key TV tags:

- top-level: `tv.home`, `tv.search`, `tv.search.field`, `tv.search.results`, `tv.detail`
- focus surfaces: `tv.surface.<surface-key>`
- focus actions: `tv.action.<surface-key>.<action-key>`
- poster targets: `tv.poster.<surface-key>.<stable-item-key>`

TV focusable surfaces also set explicit semantic content descriptions through `TvFocusableSurface`/`TvFocusableButton`; shared phone action buttons set button content descriptions from their labels.

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
| Libraries | Movie/Series tabs, library chooser, full grid, status copy, and library recovery actions are reachable without hidden first-page caps. |
| Search | Query field, Retry/Clear actions, result rows, stale/cache-miss/error cards, and diagnostics action remain visible and tagged. |
| Account & Server | Retry, Change server, Sign out, Reset connection, Diagnostics, and cache recovery exits remain visible; reset/change paths do not require OS app-data wipe. |
| Detail/Player if available | Playback/watch actions and error recovery copy preserve token contrast and expose labeled actions. |

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
| Library tabs/chooser/actions | Tabs, library chips, Browse all, Retry selected library, and recovery controls expose `tv.action.<surface>.<key>` tags and restore focus. |
| Search | Search field is focusable, Back/Retry/Clear are tagged, result rows expose `Open <title>` descriptions, and cache-miss recovery actions remain reachable. |
| Detail | Back starts as a safe focus target; playback/watch actions and missing-detail recovery actions are tagged and have visible focus. |
| Recovery/errors | Retry, clear cache, Change server, Reset connection, Sign out, and Diagnostics actions stay reachable without OS app-data wipe. |

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
