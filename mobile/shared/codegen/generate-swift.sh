#!/usr/bin/env bash
# Generate Swift FlatBuffers types from .fbs schemas.
# Output: mobile/ios/FerrexAPI/Generated/
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCHEMA_DIR="$SCRIPT_DIR/../schemas"
OUT_DIR="$SCRIPT_DIR/../../ios/FerrexAPI/Generated"

if ! command -v flatc &>/dev/null; then
  echo "ERROR: flatc not found in PATH."
  exit 1
fi

echo "flatc version: $(flatc --version)"
echo "Schemas:       $SCHEMA_DIR"
echo "Output:        $OUT_DIR"

mkdir -p "$OUT_DIR"

flatc --swift \
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

echo "✓ Swift FlatBuffers code generated in $OUT_DIR"
