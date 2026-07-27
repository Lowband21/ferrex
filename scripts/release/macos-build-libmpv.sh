#!/usr/bin/env bash
# Build the reviewed macOS libmpv profile into an isolated prefix.

set -euo pipefail

: "${MACOSX_DEPLOYMENT_TARGET:=15.0}"
export MACOSX_DEPLOYMENT_TARGET
if [[ "$MACOSX_DEPLOYMENT_TARGET" != "15.0" ]]; then
    echo "The validated Ferrex macOS build profile targets macOS 15.0, not $MACOSX_DEPLOYMENT_TARGET" >&2
    exit 2
fi

readonly MPV_VERSION="0.41.0"
readonly MPV_CLIENT_API="2.5.0"
readonly MPV_ARCHIVE_SHA256="ee21092a5ee427353392360929dc64645c54479aefdb5babc5cfbb5fad626209"
readonly MPV_ARCHIVE_URL="https://github.com/mpv-player/mpv/archive/refs/tags/v${MPV_VERSION}.tar.gz"
readonly FFMPEG_COMMIT="38b88335f99e76ed89ff3c93f877fdefce736c13"
readonly LIBPLACEBO_COMMIT="cee9b076f2c63104ccfd497fa79c39a867293ec4"
readonly LIBASS_COMMIT="bbb3c7f1570a4a021e52683f3fbdf74fe492ae84"
readonly LUA_VERSION="5.2.4"
readonly LUA_ARCHIVE_SHA256="b9e2e4aad6789b3b63a056d442f7b39f0ecfca3ae0f1fc0ae4e9614401b69f4b"
readonly LUA_ARCHIVE_URL="https://www.lua.org/ftp/lua-${LUA_VERSION}.tar.gz"
readonly FFMPEG_REPOSITORY="https://github.com/FFmpeg/FFmpeg.git"
readonly LIBPLACEBO_REPOSITORY="https://github.com/haasn/libplacebo.git"
readonly LIBASS_REPOSITORY="https://github.com/libass/libass.git"

if [[ $# -lt 1 || $# -gt 2 ]]; then
    echo "usage: $0 OUTPUT_PREFIX [WORK_DIRECTORY]" >&2
    exit 2
fi

if [[ "$(uname -s)" != "Darwin" ]]; then
    echo "macos-build-libmpv.sh must run on macOS" >&2
    exit 2
fi

prefix="$(mkdir -p "$1" && cd "$1" && pwd)"
if find "$prefix" -mindepth 1 -print -quit | grep -q .; then
    echo "refusing to install over a non-empty prefix: $prefix" >&2
    exit 2
fi

if [[ $# -eq 2 ]]; then
    work_directory="$(mkdir -p "$2" && cd "$2" && pwd)"
    cleanup_work_directory=false
else
    work_directory="$(mktemp -d "${TMPDIR:-/tmp}/ferrex-libmpv.XXXXXX")"
    cleanup_work_directory=true
fi

cleanup() {
    if [[ "$cleanup_work_directory" == true ]]; then
        rm -rf "$work_directory"
    fi
}
trap cleanup EXIT

archive="$work_directory/mpv-v${MPV_VERSION}.tar.gz"
source_directory="$work_directory/mpv-${MPV_VERSION}"
dependency_source_directory="$work_directory/dependencies"
dependency_build_directory="$work_directory/dependency-build"
build_directory="$work_directory/mpv-build"
jobs="$(sysctl -n hw.logicalcpu 2>/dev/null || printf '4')"

mkdir -p "$dependency_source_directory" "$dependency_build_directory"

checkout_exact() {
    local name="$1"
    local repository="$2"
    local commit="$3"
    local directory="$dependency_source_directory/$name"

    if [[ ! -d "$directory/.git" ]]; then
        mkdir -p "$directory"
        git -C "$directory" init --quiet
        git -C "$directory" remote add origin "$repository"
    fi
    git -C "$directory" fetch --quiet --depth 1 origin "$commit"
    git -C "$directory" checkout --quiet --detach --force FETCH_HEAD
    if [[ "$(git -C "$directory" rev-parse HEAD)" != "$commit" ]]; then
        echo "$name did not resolve pinned commit $commit" >&2
        exit 1
    fi
    git -C "$directory" submodule update --init --recursive --depth 1
}

checkout_exact ffmpeg "$FFMPEG_REPOSITORY" "$FFMPEG_COMMIT"
checkout_exact libplacebo "$LIBPLACEBO_REPOSITORY" "$LIBPLACEBO_COMMIT"
checkout_exact libass "$LIBASS_REPOSITORY" "$LIBASS_COMMIT"

lua_archive="$work_directory/lua-${LUA_VERSION}.tar.gz"
curl --fail --location --silent --show-error \
    "$LUA_ARCHIVE_URL" --output "$lua_archive"
printf '%s  %s\n' "$LUA_ARCHIVE_SHA256" "$lua_archive" | shasum -a 256 --check
tar -xzf "$lua_archive" -C "$dependency_source_directory"

curl --fail --location --silent --show-error \
    "$MPV_ARCHIVE_URL" --output "$archive"
printf '%s  %s\n' "$MPV_ARCHIVE_SHA256" "$archive" | shasum -a 256 --check
tar -xzf "$archive" -C "$work_directory"

# Do not inherit Homebrew's default FFmpeg/libplacebo feature or license
# choices. Build both reviewed dependencies into the isolated prefix first;
# their pkg-config files then take precedence for the mpv configuration.
export PATH="$prefix/bin:$PATH"
export PKG_CONFIG_PATH="$prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
export DYLD_FALLBACK_LIBRARY_PATH="$prefix/lib${DYLD_FALLBACK_LIBRARY_PATH:+:$DYLD_FALLBACK_LIBRARY_PATH}"

for module in freetype2 fribidi harfbuzz vulkan shaderc; do
    if ! pkg-config --exists "$module"; then
        echo "required version-recorded Homebrew module is missing: $module" >&2
        exit 1
    fi
done
ffmpeg_build="$dependency_build_directory/ffmpeg"
mkdir -p "$ffmpeg_build"
(
    cd "$ffmpeg_build"
    "$dependency_source_directory/ffmpeg/configure" \
        --prefix="$prefix" \
        --disable-debug \
        --disable-doc \
        --disable-programs \
        --disable-static \
        --disable-autodetect \
        --disable-gpl \
        --disable-nonfree \
        --disable-version3 \
        --enable-shared \
        --enable-avfoundation \
        --enable-audiotoolbox \
        --enable-videotoolbox \
        --enable-securetransport \
        --enable-iconv \
        --extra-libs=-liconv \
        --enable-zlib
    make -j"$jobs"
    make install
)

if grep -Eq '^CONFIG_(GPL|NONFREE|VERSION3)=yes$' \
    "$ffmpeg_build/ffbuild/config.mak"; then
    echo "the pinned FFmpeg build resolved a restricted license option" >&2
    exit 1
fi
for required in AVFOUNDATION AUDIOTOOLBOX VIDEOTOOLBOX SECURETRANSPORT; do
    if ! grep -q "^CONFIG_${required}=yes$" "$ffmpeg_build/ffbuild/config.mak"; then
        echo "pinned FFmpeg did not enable required macOS feature $required" >&2
        exit 1
    fi
done

libass_build="$dependency_build_directory/libass"
meson setup "$libass_build" "$dependency_source_directory/libass" \
    --wrap-mode=nofallback \
    --prefix="$prefix" \
    --libdir=lib \
    --buildtype=release \
    -Ddefault_library=shared \
    -Dfontconfig=disabled \
    -Dcoretext=enabled \
    -Ddirectwrite=disabled \
    -Dlibunibreak=disabled \
    -Dtest=disabled \
    -Dcompare=disabled \
    -Dprofile=disabled \
    -Dfuzz=disabled \
    -Dcheckasm=disabled
meson compile -C "$libass_build" -j "$jobs"
meson install -C "$libass_build"

python3 - "$libass_build/meson-info/intro-buildoptions.json" <<'PY'
import json
import sys

values = {item["name"]: item["value"] for item in json.load(open(sys.argv[1], encoding="utf-8"))}
required = {
    "default_library": "shared",
    "fontconfig": "disabled",
    "coretext": "enabled",
    "directwrite": "disabled",
    "libunibreak": "disabled",
}
wrong = {name: (values.get(name), expected) for name, expected in required.items() if values.get(name) != expected}
if wrong:
    raise SystemExit(f"unexpected pinned libass build options: {wrong}")
PY

# Build only the static PIC library. Upstream's `macosx` aggregate also links
# the unused `lua` CLI against readline, which is not part of this runtime.
lua_source="$dependency_source_directory/lua-${LUA_VERSION}"
make -C "$lua_source/src" -j"$jobs" liblua.a \
    MYCFLAGS="-fPIC -DLUA_USE_MACOSX"
mkdir -p "$prefix/lib/pkgconfig" "$prefix/include"
cp "$lua_source/src/liblua.a" "$prefix/lib/"
ranlib "$prefix/lib/liblua.a"
cp "$lua_source/src/lua.h" \
    "$lua_source/src/luaconf.h" \
    "$lua_source/src/lualib.h" \
    "$lua_source/src/lauxlib.h" \
    "$lua_source/src/lua.hpp" \
    "$prefix/include/"
{
    printf 'prefix=%s\n' "$prefix"
    printf 'exec_prefix=${prefix}\nlibdir=${exec_prefix}/lib\nincludedir=${prefix}/include\n\n'
    printf 'Name: Lua\nDescription: Lua interpreter library\nVersion: %s\n' "$LUA_VERSION"
    printf 'Libs: -L${libdir} -llua -lm\nCflags: -I${includedir}\n'
} >"$prefix/lib/pkgconfig/lua52.pc"
if [[ "$(pkg-config --modversion lua52)" != "$LUA_VERSION" ]]; then
    echo "pinned Lua pkg-config metadata did not resolve version $LUA_VERSION" >&2
    exit 1
fi

libplacebo_build="$dependency_build_directory/libplacebo"
meson setup "$libplacebo_build" "$dependency_source_directory/libplacebo" \
    --wrap-mode=nofallback \
    --prefix="$prefix" \
    --libdir=lib \
    --buildtype=release \
    -Ddefault_library=shared \
    -Dvulkan=enabled \
    -Dvk-proc-addr=enabled \
    -Dopengl=enabled \
    -Dd3d11=disabled \
    -Dglslang=disabled \
    -Dshaderc=enabled \
    -Dlcms=disabled \
    -Ddovi=disabled \
    -Ddemos=false \
    -Dtests=false \
    -Dbench=false \
    -Dunwind=disabled
meson compile -C "$libplacebo_build" -j "$jobs"
meson install -C "$libplacebo_build"

python3 - "$libplacebo_build/meson-info/intro-buildoptions.json" <<'PY'
import json
import sys

values = {item["name"]: item["value"] for item in json.load(open(sys.argv[1], encoding="utf-8"))}
required = {
    "default_library": "shared",
    "vulkan": "enabled",
    "vk-proc-addr": "enabled",
    "opengl": "enabled",
    "d3d11": "disabled",
    "glslang": "disabled",
    "shaderc": "enabled",
    "lcms": "disabled",
    "dovi": "disabled",
}
wrong = {name: (values.get(name), expected) for name, expected in required.items() if values.get(name) != expected}
if wrong:
    raise SystemExit(f"unexpected pinned libplacebo build options: {wrong}")
PY

# mpv's native macOS VO and VideoToolbox/libplacebo path are required. The
# cplayer is not shipped: Ferrex links the shared client library and owns the
# application bundle. Shader compilation stays in libplacebo; mpv's shaderc
# option is for its Win32/D3D11 path. Optional GPL surfaces stay disabled.
meson setup "$build_directory" "$source_directory" \
    --wrap-mode=nofallback \
    --buildtype=release \
    --prefix="$prefix" \
    --libdir=lib \
    -Dgpl=false \
    -Dcplayer=false \
    -Dlibmpv=true \
    -Dbuild-date=false \
    -Dtests=false \
    -Dcocoa=enabled \
    -Dswift-build=enabled \
    -Dmacos-cocoa-cb=enabled \
    -Dcoreaudio=enabled \
    -Davfoundation=enabled \
    -Dvideotoolbox-pl=enabled \
    -Dvideotoolbox-gl=enabled \
    -Dgl=enabled \
    -Dgl-cocoa=enabled \
    -Dwayland=disabled \
    -Dx11=disabled \
    -Dvulkan=enabled \
    -Dshaderc=disabled \
    -Dlcms2=disabled \
    -Dlibarchive=disabled \
    -Dlibbluray=disabled \
    -Djpeg=disabled \
    -Drubberband=disabled \
    -Duchardet=disabled \
    -Dzimg=disabled \
    -Dcplugins=disabled \
    -Djavascript=disabled \
    -Dlua=lua52 \
    -Dmanpage-build=disabled \
    -Dhtml-build=disabled \
    -Dpdf-build=disabled

meson compile -C "$build_directory"
meson install -C "$build_directory"

export PKG_CONFIG_PATH="$prefix/lib/pkgconfig${PKG_CONFIG_PATH:+:$PKG_CONFIG_PATH}"
actual_client_api="$(pkg-config --modversion mpv)"
if [[ "$actual_client_api" != "$MPV_CLIENT_API" ]]; then
    echo "expected libmpv client API $MPV_CLIENT_API, found $actual_client_api" >&2
    exit 1
fi

libmpv="$prefix/lib/libmpv.2.dylib"
if [[ ! -f "$libmpv" ]]; then
    echo "pinned build did not install $libmpv" >&2
    exit 1
fi

build_options="$build_directory/meson-info/intro-buildoptions.json"
python3 -c '
import json, sys
values = {item["name"]: item["value"] for item in json.load(open(sys.argv[1], encoding="utf-8"))}
required = {
    "gpl": False,
    "cplayer": False,
    "libmpv": True,
    "cocoa": "enabled",
    "swift-build": "enabled",
    "macos-cocoa-cb": "enabled",
    "videotoolbox-pl": "enabled",
    "videotoolbox-gl": "enabled",
    "gl": "enabled",
    "gl-cocoa": "enabled",
    "wayland": "disabled",
    "x11": "disabled",
    "vulkan": "enabled",
    "shaderc": "disabled",
    "lua": "lua52",
}
wrong = {name: (values.get(name), expected) for name, expected in required.items() if values.get(name) != expected}
if wrong:
    raise SystemExit(f"unexpected mpv build options: {wrong}")
' "$build_options"

ffmpeg_configuration="$(sed -n '/^FFMPEG_CONFIGURATION=/p' "$ffmpeg_build/ffbuild/config.mak")"

profile_directory="$prefix/share/ferrex/native-mpv"
mkdir -p "$profile_directory"
homebrew_direct_formulae=(
    freetype fribidi harfbuzz molten-vk shaderc vulkan-headers vulkan-loader
)
homebrew_formulae="$profile_directory/homebrew-formulae.txt"
{
    printf '%s\n' "${homebrew_direct_formulae[@]}"
    brew deps --union "${homebrew_direct_formulae[@]}"
} | LC_ALL=C sort -u >"$homebrew_formulae"
xargs brew list --versions <"$homebrew_formulae" \
    >"$profile_directory/homebrew-build-inputs.txt"
xargs brew info --json=v2 <"$homebrew_formulae" \
    >"$profile_directory/homebrew-build-inputs.json"
{
    printf 'mpv_version=%s\n' "$MPV_VERSION"
    printf 'mpv_client_api=%s\n' "$actual_client_api"
    printf 'mpv_source=%s\n' "$MPV_ARCHIVE_URL"
    printf 'mpv_source_sha256=%s\n' "$MPV_ARCHIVE_SHA256"
    printf 'mpv_gpl=false\n'
    printf 'mpv_cocoa=enabled\n'
    printf 'mpv_swift_build=enabled\n'
    printf 'mpv_macos_cocoa_cb=enabled\n'
    printf 'mpv_videotoolbox_pl=enabled\n'
    printf 'mpv_gl=enabled\n'
    printf 'mpv_wayland=disabled\n'
    printf 'mpv_x11=disabled\n'
    printf 'mpv_vulkan=enabled\n'
    printf 'mpv_shaderc=disabled\n'
    printf 'mpv_lua=lua52\n'
    printf 'ffmpeg_commit=%s\n' "$FFMPEG_COMMIT"
    printf 'ffmpeg_version=%s\n' "$(pkg-config --modversion libavcodec)"
    printf 'ffmpeg_gpl=false\nffmpeg_nonfree=false\nffmpeg_version3=false\n'
    printf 'libplacebo_commit=%s\n' "$LIBPLACEBO_COMMIT"
    printf 'libplacebo_version=%s\n' "$(pkg-config --modversion libplacebo)"
    printf 'libplacebo_opengl=enabled\nlibplacebo_vulkan=enabled\nlibplacebo_shaderc=enabled\n'
    printf 'libass_commit=%s\n' "$LIBASS_COMMIT"
    printf 'libass_version=%s\n' "$(pkg-config --modversion libass)"
    printf 'libass_coretext=enabled\n'
    printf 'freetype_version=%s\n' "$(pkg-config --modversion freetype2)"
    printf 'fribidi_version=%s\n' "$(pkg-config --modversion fribidi)"
    printf 'harfbuzz_version=%s\n' "$(pkg-config --modversion harfbuzz)"
    printf 'vulkan_loader_version=%s\n' "$(pkg-config --modversion vulkan)"
    printf 'shaderc_version=%s\n' "$(pkg-config --modversion shaderc)"
    printf 'lua_version=%s\n' "$LUA_VERSION"
    printf 'lua_source=%s\n' "$LUA_ARCHIVE_URL"
    printf 'lua_source_sha256=%s\n' "$LUA_ARCHIVE_SHA256"
    printf 'macos_deployment_target=%s\n' "$MACOSX_DEPLOYMENT_TARGET"
} >"$profile_directory/build-profile.txt"
printf '%s\n' "$ffmpeg_configuration" \
    >"$profile_directory/ffmpeg-build-configuration.txt"
cp "$source_directory/Copyright" \
    "$source_directory/LICENSE.LGPL" \
    "$profile_directory/"
mkdir -p "$profile_directory/licenses/ffmpeg" \
    "$profile_directory/licenses/libplacebo" \
    "$profile_directory/licenses/libass" \
    "$profile_directory/licenses/lua"
cp "$dependency_source_directory/ffmpeg/COPYING.LGPLv2.1" \
    "$dependency_source_directory/ffmpeg/LICENSE.md" \
    "$profile_directory/licenses/ffmpeg/"
cp "$dependency_source_directory/libplacebo/LICENSE" \
    "$profile_directory/licenses/libplacebo/"
cp "$dependency_source_directory/libass/COPYING" \
    "$profile_directory/licenses/libass/"
cp "$dependency_source_directory/lua-${LUA_VERSION}/doc/readme.html" \
    "$profile_directory/licenses/lua/"

# Preserve any installed formula notices in addition to Homebrew's complete
# version/license JSON. Keep relative paths so identically named notices from
# nested packages cannot overwrite one another.
while IFS= read -r formula; do
    formula_prefix="$(brew --prefix "$formula")"
    formula_notice_directory="$profile_directory/licenses/homebrew/$formula"
    python3 - "$formula_prefix" "$formula_notice_directory" <<'PY'
import pathlib
import shutil
import sys

source = pathlib.Path(sys.argv[1])
destination = pathlib.Path(sys.argv[2])
for notice in source.rglob("*"):
    name = notice.name.upper()
    if notice.is_file() and name.startswith(("LICENSE", "COPYING", "NOTICE")):
        target = destination / notice.relative_to(source)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(notice, target)
PY
done <"$homebrew_formulae"

# The reviewed SDK may use only isolated-prefix dylibs, Apple system libraries,
# and the complete version/license-recorded transitive Homebrew formula set.
homebrew_roots="$profile_directory/homebrew-allowed-roots.txt"
while IFS= read -r formula; do
    formula_prefix="$(brew --prefix "$formula")"
    (cd "$formula_prefix" && pwd -P)
done <"$homebrew_formulae" | LC_ALL=C sort -u >"$homebrew_roots"
while IFS= read -r binary; do
    while IFS= read -r dependency; do
        case "$dependency" in
            "$prefix"/* | /System/Library/* | /usr/lib/* | @rpath/* | @loader_path/*)
                continue
                ;;
        esac
        resolved_dependency="$(python3 -c 'import os,sys; print(os.path.realpath(sys.argv[1]))' "$dependency")"
        declared=false
        while IFS= read -r allowed_root; do
            case "$resolved_dependency" in
                "$allowed_root"/*)
                    declared=true
                    break
                    ;;
            esac
        done <"$homebrew_roots"
        if [[ "$declared" != true ]]; then
            echo "undeclared dependency in macOS libmpv closure: $binary -> $dependency" >&2
            exit 1
        fi
    done < <(otool -L "$binary" | awk 'NR > 1 { print $1 }')
done < <(find "$prefix/lib" -type f -name '*.dylib' -print)

otool -D "$libmpv"
otool -L "$libmpv"
echo "built Ferrex macOS libmpv $MPV_VERSION (client API $actual_client_api) at $prefix"
