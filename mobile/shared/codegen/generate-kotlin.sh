#!/usr/bin/env bash
# Generate Kotlin FlatBuffers types from .fbs schemas.
# Output: mobile/android/app/src/main/java/generated/
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCHEMA_DIR="$SCRIPT_DIR/../schemas"
OUT_DIR="$SCRIPT_DIR/../../android/app/src/main/java"

if ! command -v flatc &>/dev/null; then
  if [[ -z "${FERREX_USE_NIX_FLATC:-}" ]] && command -v nix &>/dev/null; then
    echo "flatc not found in PATH; retrying via 'nix shell nixpkgs#flatbuffers'..."
    exec nix shell nixpkgs#flatbuffers -c env FERREX_USE_NIX_FLATC=1 bash "$0" "$@"
  fi

  echo "ERROR: flatc not found in PATH."
  exit 1
fi

echo "flatc version: $(flatc --version)"
echo "Schemas:       $SCHEMA_DIR"
echo "Output:        $OUT_DIR"

mkdir -p "$OUT_DIR"

flatc --kotlin \
  -o "$OUT_DIR" \
  -I "$SCHEMA_DIR" \
  --gen-all \
  "$SCHEMA_DIR/ids.fbs" \
  "$SCHEMA_DIR/common.fbs" \
  "$SCHEMA_DIR/files.fbs" \
  "$SCHEMA_DIR/details.fbs" \
  "$SCHEMA_DIR/media.fbs" \
  "$SCHEMA_DIR/library.fbs" \
  "$SCHEMA_DIR/watch.fbs" \
  "$SCHEMA_DIR/auth.fbs" \
  "$SCHEMA_DIR/image.fbs"

# Post-process: patch version constant to match the Maven-available runtime.
# flatc 25.12.19 (from nixpkgs) generates FLATBUFFERS_25_12_19, but Maven
# Central only has the flatbuffers-java 25.2.10 runtime. The validateVersion()
# method is never called automatically — it's an optional compile-time check.
MAVEN_VERSION="25_2_10"
find "$OUT_DIR/ferrex" -name "*.kt" -exec sed -i "s/FLATBUFFERS_[0-9_]*/FLATBUFFERS_${MAVEN_VERSION}/g" {} +

echo "✓ Kotlin FlatBuffers code generated in $OUT_DIR (patched for runtime $MAVEN_VERSION)"
