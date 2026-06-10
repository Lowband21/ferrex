#!/usr/bin/env bash
# Generate Kotlin FlatBuffers bindings from the shared mobile schemas.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCHEMA_DIR="$SCRIPT_DIR/../schemas"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../.." && pwd)"
OUT_DIR="$REPO_ROOT/mobile/android/app/src/main/java"
GENERATED_DIR="$OUT_DIR/ferrex"
FLATBUFFERS_JAVA_VERSION="${FERREX_FLATBUFFERS_JAVA_VERSION:-25.2.10}"
FLATBUFFERS_JAVA_CONSTANT="${FLATBUFFERS_JAVA_VERSION//./_}"

if ! command -v flatc >/dev/null 2>&1; then
  echo "ERROR: flatc not found in PATH." >&2
  echo "Run with: nix shell nixpkgs#flatbuffers -c $0" >&2
  exit 1
fi

schemas=(
  "$SCHEMA_DIR/ids.fbs"
  "$SCHEMA_DIR/common.fbs"
  "$SCHEMA_DIR/files.fbs"
  "$SCHEMA_DIR/details.fbs"
  "$SCHEMA_DIR/media.fbs"
  "$SCHEMA_DIR/library.fbs"
  "$SCHEMA_DIR/auth.fbs"
  "$SCHEMA_DIR/image.fbs"
)

echo "flatc version:              $(flatc --version)"
echo "Schemas:                    $SCHEMA_DIR"
echo "Output:                     $OUT_DIR"
echo "FlatBuffers Java runtime:   $FLATBUFFERS_JAVA_VERSION"

rm -rf "$GENERATED_DIR"
mkdir -p "$OUT_DIR"

flatc --kotlin \
  -o "$OUT_DIR" \
  -I "$SCHEMA_DIR" \
  --gen-all \
  "${schemas[@]}"

# Nixpkgs may ship a newer flatc than the newest flatbuffers-java artifact used
# by Gradle. The generated validateVersion() helpers are optional compatibility
# checks, but they still need to reference a runtime constant that exists.
find "$GENERATED_DIR" -type f -name "*.kt" \
  -exec sed -i "s/FLATBUFFERS_[0-9_]*/FLATBUFFERS_${FLATBUFFERS_JAVA_CONSTANT}/g" {} +

echo "✓ Kotlin FlatBuffers code generated in $GENERATED_DIR"
