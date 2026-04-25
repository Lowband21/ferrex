# Android TV spike architecture

## Decision

Use a `tv` product flavor and source set inside the existing Android `:app` module for this spike.

## Rationale

The current Android app keeps reusable auth, API, library/cache, image loading, watch progress, player, Hilt, and generated FlatBuffers access under `app/src/main`. A separate TV application module is a cleaner long-term shape, but it first requires extracting those shared services into a core Android module. A flavor keeps the phone app buildable while adding a dedicated Leanback entry point and TV UI shell.

## Current shape

- `mobile` is the default flavor and keeps the phone launcher `MainActivity`.
- `tv` adds `applicationIdSuffix = ".tv"` and `versionNameSuffix = "-tv"`.
- `src/tv` contains the Leanback manifest overlay, `TvMainActivity`, TV navigation, TV home rows, and the TV player overlay hook.
- `src/main` remains shared by both flavors; TV reuses shared ViewModels/services but does not route into the phone home screen.

## Build and review notes

- Review both variants with `:app:assembleMobileDebug` and `:app:assembleTvDebug`.
- The `build.gradle.kts` Android block is expected to conflict with the `android-hardening` branch because both changes edit variant configuration. Do not resolve that in this TV spike branch.

## Follow-up direction

If Android TV moves beyond this shell, extract shared Android services into a `:core` module and split phone/TV into separate application modules (`:app` and `:tv`).
