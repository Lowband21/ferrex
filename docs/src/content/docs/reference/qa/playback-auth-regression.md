---
title: "Playback auth regression QA"
description: "LOW-415 playback-auth regression evidence for server stream tickets, desktop GStreamer/MPV smoke, Android/TV auth-expiry units, and manual follow-up."
sidebar:
  order: 2
---

> **Evidence status:** This page preserves a dated QA packet. Treat rows marked blocked, pending hardware QA, or release-readiness follow-up as stale evidence until they are rerun on current devices and builds.

Run date: 2026-06-15 from `/home/lowband/dev/workspaces/ferrex/LOW-415`.

This packet records the playback-auth regression gate for the Ferrex server, desktop GStreamer path, desktop MPV hand-off, Android mobile, and Android TV. It focuses on short-lived playback tickets, fail-closed stream access, log redaction, and expiry/retry recovery evidence.

## Current disposition

| Area | Disposition | Evidence / gap |
| --- | --- | --- |
| Rust format/check/core | Pass | `cargo fmt`, workspace `cargo check --all-targets`, and `ferrex-core` lib tests passed. Workspace check still emits existing `ferrexctl` missing-doc warnings. |
| Server playback auth integration | Pass | `crates/ferrex-server/tests/playback_stream_failures.rs` passed against a local Nix Postgres with `pg_uuidv7`: ticket issue, range streaming, missing/invalid token rejection, account API scope rejection, and typed media recovery headers. |
| Desktop GStreamer smoke | Pass | `scripts/qa/playback-auth-smoke.sh` served a local protected WAV fixture, verified bad tickets return 401, then completed `gst-launch-1.0 playbin` with fakesinks using the ticketed URL. |
| Desktop MPV smoke | Pass | The same smoke completed `mpv --no-config --ao=null` against the ticketed URL, redacted the MPV log, and verified retained artifacts do not contain the raw ticket. |
| Desktop ticket URL / redaction unit coverage | Pass | `ferrex-player-playback` tests passed for ticketed URL resolution, fail-closed ticket errors, and access-token redaction in playback/MPV log lines. |
| Android mobile / TV auth-expiry retry unit coverage | Pass | Focused Gradle unit command for `PlaybackFoundationTest.ticketAuthFailuresRetryThenInvalidateSession` returned `BUILD SUCCESSFUL` for mobile and TV tasks. |
| Android mobile / TV manual playback | Blocked | `adb devices -l` returned no attached devices. The SDK emulator binary cannot start in this NixOS workspace because of the stub-ld dynamic-loader error. Physical phone/TV or a runnable emulator is still required for manual playback evidence. |
| Android full assemble/unit/lint | Not rerun for LOW-415 | No Android source code changed in this packet. The most recent full Android evidence is recorded in [Android / Android TV final QA acceptance packet](/reference/qa/android-final-qa/); rerun the full Gradle gate if Android code changes. |

## Exact validation commands and results

### Repository baseline

```bash
cargo fmt --all --check
```

Result: pass; command exited successfully with no output.

```bash
nix develop .#ferrex-player --command env cargo check --workspace --all-targets
```

Result: pass; finished `dev` profile in `23.61s`. Existing `ferrexctl` missing-documentation warnings were emitted; no errors.

```bash
nix develop .#ferrex-player --command env cargo test -p ferrex-core --lib
```

Result: pass; `170 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 4.89s`.

### Server playback auth integration

`sqlx::test` requires a live PostgreSQL with `pg_uuidv7`. This run used the Postgres binary supplied by the Nix dev shell and trust auth on loopback:

```bash
nix develop .#ferrex-player --command bash -lc '
set -euo pipefail
pg_dir="$(mktemp -d -t ferrex-low415-pg.XXXXXX)"
cleanup() {
  if [ -n "${PG_STARTED:-}" ]; then pg_ctl -D "$pg_dir/data" -m fast -w stop >/dev/null 2>&1 || true; fi
  rm -rf "$pg_dir"
}
trap cleanup EXIT
initdb -A trust -U postgres -D "$pg_dir/data" >/dev/null
pg_ctl -D "$pg_dir/data" -o "-F -p 55415 -k $pg_dir" -w start >/dev/null
PG_STARTED=1
DATABASE_URL="postgres://postgres@127.0.0.1:55415/postgres" cargo test -p ferrex-server --test playback_stream_failures
'
```

Result: pass; `4 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 2.76s`.

Covered server scenarios:

- Authenticated ticket issue for an available media file.
- Full and byte-range streaming with a playback ticket.
- Missing, invalid query-token, and invalid bearer-token stream access rejects without serving media bytes or stream headers.
- Playback-scope tickets are rejected by account/admin APIs and are not reflected in response bodies or headers.
- Missing media, unavailable media, and missing files return typed recovery headers.

### Desktop playback auth smoke

The smoke script is intentionally local and self-contained: it generates a short WAV fixture, starts a loopback HTTP server that serves the fixture only when `access_token` matches a disposable ticket, verifies bad-ticket 401 behavior, then exercises the desktop GStreamer and MPV playback dependencies with headless/null sinks.

```bash
nix develop .#ferrex-player --command shellcheck scripts/qa/playback-auth-smoke.sh
```

Result: pass; no shellcheck findings.

```bash
nix develop .#ferrex-player --command bash scripts/qa/playback-auth-smoke.sh
```

Result: pass:

```text
Playback auth smoke passed
 - unauthorized ticket rejected with HTTP 401
 - ticketed bytes fetched and matched fixture
 - GStreamer playbin completed with fakesinks
 - MPV completed with --no-config --ao=null
 - retained logs are redacted
```

To retain redacted smoke artifacts for a reviewer-requested packet:

```bash
FERREX_QA_KEEP_ARTIFACTS=1 nix develop .#ferrex-player --command bash scripts/qa/playback-auth-smoke.sh
```

Do not attach `mpv.raw.log`; the script deletes it after producing `mpv.redacted.log`.

### Desktop ticket URL and redaction units

```bash
nix develop .#ferrex-player --command env cargo test -p ferrex-player-playback resolve_playback_stream_url
```

Result: pass; `2 passed; 0 failed; 4 filtered out`.

```bash
nix develop .#ferrex-player --command env cargo test -p ferrex-player-playback redacts_access_token
```

Result: pass; `2 passed; 0 failed; 4 filtered out`.

### Android mobile / Android TV focused auth-expiry unit check

No Android source code changed in LOW-415, so the full assemble/unit/lint gate was not rerun. A focused playback auth-expiry check was rerun for mobile and TV unit tasks:

```bash
cd mobile/android && \
ANDROID_HOME=/home/lowband/Android/Sdk \
ANDROID_SDK_ROOT=/home/lowband/Android/Sdk \
./gradlew :app:testMobileDebugUnitTest :app:testTvDebugUnitTest \
  --tests 'com.ferrex.android.core.playback.PlaybackFoundationTest.ticketAuthFailuresRetryThenInvalidateSession' \
  --no-daemon --stacktrace \
  -Pandroid.aapt2FromMavenOverride=/home/lowband/Android/Sdk/build-tools/35.0.0/aapt2
```

Result: pass; `BUILD SUCCESSFUL in 8s`. Mobile reused cached test output that includes the full `PlaybackFoundationTest` class with `18 tests, 0 failures, 0 errors, 0 skipped`; TV produced focused output with `1 test, 0 failures, 0 errors, 0 skipped` for `ticketAuthFailuresRetryThenInvalidateSession`.

### Android device / emulator availability

```bash
adb devices -l
/home/lowband/Android/Sdk/emulator/emulator -list-avds
```

Results:

- `adb devices -l`: only the `List of devices attached` header; no phone, Android TV, or emulator target was attached.
- Emulator listing failed before AVD enumeration: NixOS stub-ld dynamic executable error for `/home/lowband/Android/Sdk/emulator/emulator`.

## Regression scenario matrix

| Scenario | Server | Desktop GStreamer | Desktop MPV | Android mobile | Android TV |
| --- | --- | --- | --- | --- | --- |
| Ticket acquisition / ticketed stream | Pass: integration test issues a playback ticket and streams media bytes. | Pass: local ticketed URL plays through `gst-launch-1.0 playbin`. | Pass: local ticketed URL plays through MPV with null audio output. | Unit evidence: `fetchTicketUsesAuthenticatedTicketRouteAndBuildsTicketedStreamUrl`; manual blocked. | Same shared unit evidence; manual blocked. |
| Missing / invalid ticket | Pass: missing and invalid tokens return 401 without media bytes or streaming headers. | Pass: smoke verifies bad ticket returns HTTP 401 before playback. | Pass: smoke verifies bad ticket returns HTTP 401 before playback. | Unit evidence covers ticket auth failures and recovery copy; manual blocked. | Same shared unit evidence; manual blocked. |
| Ticket scope separation | Pass: playback ticket is forbidden on account/admin APIs and unauthorized as an account query token. | Not applicable; desktop clients consume stream URLs only. | Not applicable; desktop clients consume stream URLs only. | Covered by server contract plus mobile ticket transport route tests. | Covered by server contract plus shared ticket transport route tests. |
| Expiry / retry recovery | Server evidence: tickets have bounded positive expiry and invalid tokens fail closed. | Desktop unit evidence: ticket endpoint errors fail closed instead of returning a bare stream URL; live short-TTL retry was not manually exercised. | MPV log redaction passed; live short-TTL retry was not manually exercised. | Pass: `ticketAuthFailuresRetryThenInvalidateSession` rerun; broader `PlaybackFoundationTest` covers stream auth retry with fresh tickets. Manual playback blocked. | Pass: focused TV unit rerun for ticket auth retry/invalidation. Manual playback blocked. |
| Log / diagnostics redaction | Integration tests assert raw tokens are not echoed in auth failure responses. | Smoke retains only redacted logs; `redacts_access_token` unit tests pass. | Smoke redacts MPV log lines and verifies retained artifacts do not contain the disposable ticket. | Unit diagnostics redaction coverage exists in the Android QA packet; focused retry command did not produce device logs. | Same shared unit/QA-packet evidence; no device logs. |
| Hardware/emulator playback | Not applicable. | Headless dependency smoke passed; full interactive desktop UI playback was not captured. | Headless dependency smoke passed; full interactive desktop UI/MPV hand-off was not captured. | Blocked: no attached phone/emulator. | Blocked: no attached TV/emulator. |

## Required manual follow-up when hardware is available

Record device model, OS/API level, display mode, server build, media fixture, and redacted logs/screenshots for each manual run.

1. **Desktop UI, GStreamer path:** launch the player against a QA server, start playback from a detail page, verify the resolved stream URL is ticketed, pause/seek/exit, and confirm no raw ticket appears in Ferrex logs.
2. **Desktop UI, MPV hand-off:** use the same media item, launch "Play in MPV", confirm MPV opens the ticketed stream, close MPV, and confirm Ferrex/MPV retained logs are redacted.
3. **Short ticket expiry:** configure a short playback-ticket TTL or use an expired ticket fixture. Verify server returns 401/403, clients retry where supported, and recovery remains actionable after retry limits without clearing app data.
4. **Android phone:** install `app-mobile-debug.apk`, sign in, play/resume/start-over, force ticket/session expiry, and verify retry then sign-in/change-server recovery without OS app-data wipe.
5. **Android TV:** install `app-tv-debug.apk`, repeat the phone playback/expiry cases using D-pad/OK/Back, and verify focus remains reachable on playback recovery actions.
6. **Evidence redaction:** before attaching logs, screenshots, videos, bugreports, or diagnostics exports, remove server URLs if sensitive, auth headers, bearer/session/refresh/access tokens, playback ticket URLs/query values, local device IDs, usernames when sensitive, PIN/setup material, and private paths.

## Files added by this packet

- `scripts/qa/playback-auth-smoke.sh` — reusable headless local ticketed-stream smoke for desktop GStreamer and MPV dependencies.
- [Playback auth regression QA](/reference/qa/playback-auth-regression/) — this cross-platform QA evidence packet.

## Source evidence consulted

- `crates/ferrex-server/tests/playback_stream_failures.rs`
- `crates/ferrex-player-playback/src/update.rs`
- `crates/ferrex-player-playback/src/diagnostics.rs`
- `mobile/android/app/src/test/kotlin/com/ferrex/android/core/playback/PlaybackFoundationTest.kt`
- [Android / Android TV final QA acceptance packet](/reference/qa/android-final-qa/)
- [Android playback QA matrix](/reference/qa/android-playback-matrix/)
- [Android TV 10-foot QA matrix](/reference/qa/android-tv-10ft-matrix/)
