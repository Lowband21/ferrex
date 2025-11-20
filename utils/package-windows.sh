#!/usr/bin/env bash
set -euo pipefail

# Package a Windows portable build of ferrex-player.
#
# This script stages dist-windows/ with:
# - ferrex-player.exe from target/<target>/<profile>
# - build-windows/run-ferrex.bat and run-ferrex.ps1 launchers
# - build-windows/README.txt
# - GStreamer runtime DLLs under bin/ and plugins under lib/gstreamer-1.0/
# Then creates a versioned zip: ferrex-player_windows_<version>_<timestamp>_<flavor>.zip
#
# Usage:
#   utils/package-windows.sh [--target <triple>] [--profile <name>] [--gst-root <path>] [--out <dir>]
#
# Environment overrides:
#   GST_MINGW_ROOT  When target contains "-gnu": path to MinGW GStreamer root ending in .../gstreamer/1.0/mingw_x86_64
#   GST_MSVC_ROOT   When target contains "-msvc": path to MSVC GStreamer root ending in .../gstreamer/1.0/msvc_x86_64
#

TARGET="x86_64-pc-windows-gnu"
PROFILE="release"
GST_ROOT_OVERRIDE=""
OUT_DIR="."

die() { echo "Error: $*" >&2; exit 1; }

usage() {
  cat <<EOF
Package ferrex-player for Windows.

Options:
  --target <triple>   Target triple (default: $TARGET)
  --profile <name>    Cargo profile (default: $PROFILE)
  --gst-root <path>   Override GStreamer root for packaging
  --out <dir>         Output directory for zip (default: current dir)
  -h|--help           Show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --target) TARGET=${2:-}; shift 2 ;;
    --profile) PROFILE=${2:-}; shift 2 ;;
    --gst-root) GST_ROOT_OVERRIDE=${2:-}; shift 2 ;;
    --out) OUT_DIR=${2:-}; shift 2 ;;
    -h|--help) usage; exit 0 ;;
    *) die "Unknown argument: $1" ;;
  esac
done

[[ -d ferrex-player ]] || die "Run from repo root; ferrex-player/ not found"

# Locate GStreamer root
FLAVOR=""
GST_ROOT=""
case "$TARGET" in
  *-gnu)
    FLAVOR="gnu"
    if [[ -n "$GST_ROOT_OVERRIDE" ]]; then
      GST_ROOT="$GST_ROOT_OVERRIDE"
    elif [[ -n "${GST_MINGW_ROOT:-}" ]]; then
      GST_ROOT="$GST_MINGW_ROOT"
    elif [[ -d "/home/lowband/gstreamer-windows/PFiles64/gstreamer/1.0/mingw_x86_64" ]]; then
      # Convenience default for this workspace
      GST_ROOT="/home/lowband/gstreamer-windows/PFiles64/gstreamer/1.0/mingw_x86_64"
    fi
    ;;
  *-msvc)
    FLAVOR="msvc"
    if [[ -n "$GST_ROOT_OVERRIDE" ]]; then
      GST_ROOT="$GST_ROOT_OVERRIDE"
    elif [[ -n "${GST_MSVC_ROOT:-}" ]]; then
      GST_ROOT="$GST_MSVC_ROOT"
    fi
    ;;
  *) die "Unknown target flavor for $TARGET (expected -gnu or -msvc)" ;;
esac

[[ -n "$GST_ROOT" ]] || die "GStreamer root not set. Provide --gst-root or set GST_MINGW_ROOT/GST_MSVC_ROOT"
[[ -d "$GST_ROOT/bin" ]] || die "Invalid GStreamer root (bin/ missing): $GST_ROOT"
[[ -d "$GST_ROOT/lib/gstreamer-1.0" ]] || die "Invalid GStreamer root (lib/gstreamer-1.0 missing): $GST_ROOT"

# Prepare pkg-config shim for cross build
PC_CACHE_DIR="$PWD/cache/pkgconfig/${TARGET}"
mkdir -p "$PC_CACHE_DIR"
PC_SOURCE_DIR="$GST_ROOT/lib/pkgconfig"
if [[ -d "$PC_SOURCE_DIR" ]]; then
  for pc_path in "$PC_SOURCE_DIR"/*.pc; do
    [[ -f "$pc_path" ]] || continue
    pc_file=$(basename "$pc_path")
    awk -v ROOT="$GST_ROOT" '
      BEGIN{FS=OFS="="}
      /^prefix=/ {print "prefix=" ROOT; next}
      {print}
    ' "$pc_path" > "$PC_CACHE_DIR/$pc_file"
  done
else
  die "pkg-config directory missing under $GST_ROOT"
fi

# Configure cross-compilation env before building
TARGET_ENV=${TARGET//-/_}
export PKG_CONFIG_ALLOW_CROSS=1
eval "export PKG_CONFIG_ALLOW_CROSS_${TARGET_ENV}=1"
eval "export PKG_CONFIG_${TARGET_ENV}=pkg-config"
export PKG_CONFIG_PATH="$PC_CACHE_DIR"
eval "export PKG_CONFIG_PATH_${TARGET_ENV}=$PC_CACHE_DIR"
export PKG_CONFIG_LIBDIR="$PC_CACHE_DIR"
eval "export PKG_CONFIG_LIBDIR_${TARGET_ENV}=$PC_CACHE_DIR"
export PKG_CONFIG_SYSROOT_DIR="$GST_ROOT"
eval "export PKG_CONFIG_SYSROOT_DIR_${TARGET_ENV}=$GST_ROOT"

if [[ "$TARGET" == "x86_64-pc-windows-gnu" ]]; then
  command -v x86_64-w64-mingw32-gcc >/dev/null || die "x86_64-w64-mingw32-gcc not found in PATH"
  eval "export CC_${TARGET_ENV}=x86_64-w64-mingw32-gcc"
  eval "export CXX_${TARGET_ENV}=x86_64-w64-mingw32-g++"
  eval "export AR_${TARGET_ENV}=x86_64-w64-mingw32-ar"
  eval "export RANLIB_${TARGET_ENV}=x86_64-w64-mingw32-ranlib"
  eval "export WINDRES_${TARGET_ENV}=x86_64-w64-mingw32-windres"
fi

# Build the executable after env is configured
EXE_PATH="target/${TARGET}/${PROFILE}/ferrex-player.exe"
echo "Building ferrex-player for ${TARGET} (${PROFILE})..."
cargo build -p ferrex-player --target "$TARGET" --profile "$PROFILE"

[[ -f "$EXE_PATH" ]] || die "Built exe not found: $EXE_PATH"

# Stage distribution
STAGE="dist-windows"
BIN_DIR="$STAGE/bin"
PLUGINS_DIR="$STAGE/lib/gstreamer-1.0"
rm -rf "$STAGE"
mkdir -p "$BIN_DIR" "$PLUGINS_DIR"

cp -v utils/build-windows/run-ferrex.bat "$STAGE/"
cp -v utils/build-windows/run-ferrex.ps1 "$STAGE/"
cp -v utils/build-windows/README.txt "$STAGE/"
cp -v "$EXE_PATH" "$STAGE/"

echo "Copying GStreamer runtime from: $GST_ROOT"
cp -v "$GST_ROOT"/bin/*.dll "$BIN_DIR/"
cp -v "$GST_ROOT"/lib/gstreamer-1.0/*.dll "$PLUGINS_DIR/"

# Create versioned zip
STAMP=$(date +%Y%m%d-%H%M%S)
VER=$(awk -F'"' '/^version\s*=\s*"[0-9A-Za-z\.-]+"/{print $2; exit}' Cargo.toml)
ZIP_NAME="ferrex-player_windows_${VER:-0.1.0}_${STAMP}_${FLAVOR}.zip"
(
  cd "$OUT_DIR"
  rm -f "$ZIP_NAME"
  # Use relative path to stage so the zip contains dist-windows/*
  if [[ "$OUT_DIR" == "." ]]; then
    zip -r "$ZIP_NAME" "$STAGE" >/dev/null
  else
    zip -r "$ZIP_NAME" "$PWD/$STAGE" >/dev/null
  fi
  echo "Created: $(pwd)/$ZIP_NAME"
  sha256sum "$ZIP_NAME" || true
)

echo "Done. Artifact: $OUT_DIR/$ZIP_NAME"
