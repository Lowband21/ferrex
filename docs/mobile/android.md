# Ferrex Android

Native Android client for Ferrex, built with Kotlin, Jetpack Compose, OkHttp,
Media3 ExoPlayer, Hilt, Coil, and FlatBuffers.

## Requirements

- JDK 17+
- Android SDK platform 35 and build-tools 35.0.0
- `flatc` 25.12.19 for generated Kotlin FlatBuffers sources

On Nix-based systems, `flatc` can be supplied for a single command with:

```bash
nix shell nixpkgs#flatbuffers -c ./mobile/shared/codegen/generate-kotlin.sh
```

## Generated FlatBuffers sources

Kotlin FlatBuffers types are generated into:

```text
mobile/android/app/src/main/java/ferrex/
```

That directory is intentionally ignored by git. Do not hand-edit generated
sources. Regenerate them from a clean checkout before running Android Gradle
builds:

```bash
./mobile/shared/codegen/generate-kotlin.sh
```

The generator currently uses schemas from `mobile/shared/schemas/` and patches
the generated runtime version constant to match the Maven Central
`flatbuffers-java` runtime declared in `gradle/libs.versions.toml`.

## Local validation

From the repository root:

```bash
./mobile/shared/codegen/generate-kotlin.sh
cd mobile/android
./gradlew :app:assembleDebug :app:testDebugUnitTest :app:lintDebug --no-daemon --stacktrace
./gradlew :app:assembleRelease --no-daemon --stacktrace
```

If your local environment needs machine-specific Gradle properties, keep them in
`~/.gradle/gradle.properties` rather than this repository. For example, NixOS
may require an `android.aapt2FromMavenOverride` pointing at a patched Android SDK
`aapt2` binary.

## Lint policy

Android lint is expected to run cleanly for app code. Generated FlatBuffers
Kotlin can trigger `SuspiciousIndentation` false positives from `flatc --kotlin`;
`app/lint.xml` scopes that suppression to `src/main/java/ferrex` only.

## CI

`.github/workflows/android.yml` is the clean-checkout Android quality gate. It:

1. checks out the repository,
2. installs JDK/Android SDK/Gradle cache,
3. installs `flatc` 25.12.19,
4. runs Kotlin FlatBuffers codegen, and
5. runs `:app:assembleDebug :app:testDebugUnitTest :app:lintDebug`.
