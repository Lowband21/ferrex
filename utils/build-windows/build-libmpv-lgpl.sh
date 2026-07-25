#!/usr/bin/env bash
# Build the pinned LGPL-only Windows libmpv SDK used by Ferrex CI/releases.
#
# Run this inside an MSYS2 UCRT64 shell after installing the packages listed in
# .github/workflows/ci.yml. The output is self-describing and contains the
# runtime DLL closure, headers, GNU import library, exact build profile, hashes,
# and notices. A separate PowerShell step derives mpv.lib for the MSVC Rust
# target from the same DLL exports.

set -euo pipefail

PREFIX=${1:-/c/ferrex-libmpv-sdk}
WORK=${FERREX_LIBMPV_BUILD_ROOT:-/c/ferrex-libmpv-build}
JOBS=${NUMBER_OF_PROCESSORS:-4}

MPV_COMMIT=41f6a645068483470267271e1d09966ca3b9f413
FFMPEG_COMMIT=38b88335f99e76ed89ff3c93f877fdefce736c13
LIBASS_COMMIT=bbb3c7f1570a4a021e52683f3fbdf74fe492ae84
LIBPLACEBO_COMMIT=cee9b076f2c63104ccfd497fa79c39a867293ec4
LUAJIT_COMMIT=b411bec3ce550ef9968fc83bca094455cf812c1f

if [[ ${MSYSTEM:-} != UCRT64 ]]; then
	echo "error: run this script in an MSYS2 UCRT64 shell" >&2
	exit 1
fi

mkdir -p "$PREFIX" "$WORK/src" "$WORK/build"
export PATH="$PREFIX/bin:/ucrt64/bin:$PATH"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:/ucrt64/lib/pkgconfig"
export PKG_CONFIG=/ucrt64/bin/pkg-config
export CFLAGS="${CFLAGS:-} -O2"

checkout_exact() {
	local name=$1
	local repository=$2
	local commit=$3
	local directory="$WORK/src/$name"
	if [[ ! -d $directory/.git ]]; then
		mkdir -p "$directory"
		git -C "$directory" init
		git -C "$directory" remote add origin "$repository"
	fi
	git -C "$directory" fetch --depth 1 origin "$commit"
	git -C "$directory" checkout --detach --force FETCH_HEAD
	[[ $(git -C "$directory" rev-parse HEAD) == "$commit" ]] || {
		echo "error: $name did not resolve pinned commit $commit" >&2
		exit 1
	}
	git -C "$directory" submodule update --init --recursive --depth 1
}

checkout_exact libass https://github.com/libass/libass.git "$LIBASS_COMMIT"
checkout_exact ffmpeg https://github.com/FFmpeg/FFmpeg.git "$FFMPEG_COMMIT"
checkout_exact luajit https://github.com/openresty/luajit2.git "$LUAJIT_COMMIT"
checkout_exact libplacebo https://github.com/haasn/libplacebo.git "$LIBPLACEBO_COMMIT"
checkout_exact mpv https://github.com/mpv-player/mpv.git "$MPV_COMMIT"

# libass 0.17.4: exact shared build used for text/ASS subtitle rendering.
pushd "$WORK/src/libass" >/dev/null
./autogen.sh \
	--prefix="$PREFIX" \
	--disable-static \
	--enable-shared \
	--enable-asm \
	--enable-harfbuzz \
	--disable-fontconfig
make -j"$JOBS"
make install
popd >/dev/null

# FFmpeg 8.1.2: keep the closure LGPLv2.1 by explicitly rejecting GPL,
# nonfree, and LGPLv3/version3 options. Windows native TLS and hardware decode
# APIs do not require an external restricted library.
mkdir -p "$WORK/build/ffmpeg"
pushd "$WORK/build/ffmpeg" >/dev/null
"$WORK/src/ffmpeg/configure" \
	--prefix="$PREFIX" \
	--disable-debug \
	--disable-doc \
	--disable-programs \
	--disable-static \
	--disable-autodetect \
	--disable-gpl \
	--disable-nonfree \
	--disable-version3 \
	--enable-shared \
	--enable-libass \
	--enable-schannel \
	--enable-d3d11va \
	--enable-dxva2
make -j"$JOBS"
make install
if grep -Eq '^CONFIG_(GPL|NONFREE|VERSION3)=yes$' ffbuild/config.mak; then
	echo "error: FFmpeg resolved a restricted license option" >&2
	exit 1
fi
for required in CONFIG_LIBASS CONFIG_SCHANNEL CONFIG_D3D11VA CONFIG_DXVA2; do
	if ! grep -q "^${required}=yes$" ffbuild/config.mak; then
		echo "error: FFmpeg did not enable required Windows feature $required" >&2
		exit 1
	fi
done
popd >/dev/null

# LuaJIT supplies mpv's controlled OSC/native-window fallback without reading
# a user's standalone mpv configuration.
pushd "$WORK/src/luajit" >/dev/null
make -j"$JOBS" BUILDMODE=dynamic TARGET_SYS=Windows
make install BUILDMODE=dynamic TARGET_SYS=Windows PREFIX="$PREFIX"
popd >/dev/null

# libplacebo 7.360.1 provides gpu-next's D3D11 renderer. Vulkan/OpenGL are not
# needed for the Windows release path and are disabled to keep the closure
# narrow; shaderc is retained for the D3D11 shader pipeline.
meson setup "$WORK/build/libplacebo" "$WORK/src/libplacebo" \
	--prefix="$PREFIX" \
	--buildtype=release \
	-Ddefault_library=shared \
	-Dvulkan=disabled \
	-Dopengl=disabled \
	-Dd3d11=enabled \
	-Dglslang=disabled \
	-Dshaderc=enabled \
	-Dlcms=enabled \
	-Ddovi=disabled \
	-Ddemos=false \
	-Dtests=false \
	-Dbench=false \
	-Dunwind=disabled
meson compile -C "$WORK/build/libplacebo" -j "$JOBS"
meson install -C "$WORK/build/libplacebo"

python - "$WORK/build/libplacebo/meson-info/intro-buildoptions.json" <<'PY'
import json
import sys

options = {item["name"]: item["value"] for item in json.load(open(sys.argv[1], encoding="utf-8"))}
required = {
    "vulkan": "disabled",
    "opengl": "disabled",
    "d3d11": "enabled",
    "glslang": "disabled",
    "shaderc": "enabled",
    "lcms": "enabled",
}
wrong = {key: (options.get(key), value) for key, value in required.items() if options.get(key) != value}
if wrong:
    raise SystemExit(f"unexpected libplacebo Windows build options: {wrong}")
PY

# mpv 0.41.0 / client API 2.5. The LGPL-compatible gpu-next/D3D11 path,
# WASAPI, D3D11VA/DXVA2, and shared libmpv are mandatory. The legacy
# `direct3d` VO is GPL-gated upstream and therefore explicitly disabled.
meson setup "$WORK/build/mpv" "$WORK/src/mpv" \
	--prefix="$PREFIX" \
	--buildtype=release \
	-Dgpl=false \
	-Dbuild-date=false \
	-Dcplayer=false \
	-Dlibmpv=true \
	-Dtests=false \
	-Dfuzzers=false \
	-Dmanpage-build=disabled \
	-Dhtml-build=disabled \
	-Dpdf-build=disabled \
	-Dcdda=disabled \
	-Ddvbin=disabled \
	-Ddvdnav=disabled \
	-Djavascript=disabled \
	-Dlua=luajit \
	-Drubberband=disabled \
	-Duchardet=disabled \
	-Dvapoursynth=disabled \
	-Dzimg=disabled \
	-Dlibarchive=disabled \
	-Dlibbluray=disabled \
	-Dopenal=disabled \
	-Dsdl2-audio=disabled \
	-Dsdl2-video=disabled \
	-Dwasapi=enabled \
	-Dd3d11=enabled \
	-Ddirect3d=disabled \
	-Dd3d-hwaccel=enabled \
	-Dd3d9-hwaccel=enabled \
	-Dgl=disabled \
	-Dvulkan=disabled \
	-Dwin32-threads=enabled \
	-Dwin32-smtc=disabled
meson compile -C "$WORK/build/mpv" -j "$JOBS"
meson install -C "$WORK/build/mpv"

python - "$WORK/build/mpv/meson-info/intro-buildoptions.json" <<'PY'
import json
import sys

options = {item["name"]: item["value"] for item in json.load(open(sys.argv[1], encoding="utf-8"))}
required = {
    "gpl": False,
    "libmpv": True,
    "cplayer": False,
    "d3d11": "enabled",
    "direct3d": "disabled",
    "d3d-hwaccel": "enabled",
    "d3d9-hwaccel": "enabled",
    "wasapi": "enabled",
    "gl": "disabled",
    "vulkan": "disabled",
    "lua": "luajit",
    "win32-threads": "enabled",
}
wrong = {key: (options.get(key), value) for key, value in required.items() if options.get(key) != value}
if wrong:
    raise SystemExit(f"unexpected mpv Windows build options: {wrong}")
PY

MPV_DLL=$(find "$PREFIX/bin" -maxdepth 1 -type f \( -iname 'libmpv-2.dll' -o -iname 'mpv-2.dll' \) -print -quit)
[[ -n $MPV_DLL ]] || {
	echo "error: mpv install did not produce libmpv-2.dll" >&2
	exit 1
}
[[ -f "$PREFIX/lib/libmpv.dll.a" ]] || {
	echo "error: mpv install did not produce libmpv.dll.a" >&2
	exit 1
}
[[ -f "$PREFIX/include/mpv/client.h" ]] || {
	echo "error: mpv install did not produce mpv/client.h" >&2
	exit 1
}

# Everything present before closure materialization came from the exact source
# builds above. Record that boundary so copied MSYS2 runtime DLLs cannot be
# mislabeled merely because they now live under the isolated prefix.
SOURCE_BUILT_DLLS="$WORK/source-built-dlls.txt"
COPIED_RUNTIME_ORIGINS="$WORK/copied-runtime-origins.tsv"
find "$PREFIX/bin" -maxdepth 1 -type f -iname '*.dll' -printf '%f\n' |
	LC_ALL=C sort -u >"$SOURCE_BUILT_DLLS"
: >"$COPIED_RUNTIME_ORIGINS"

# Materialize the complete non-system DLL closure next to libmpv. ldd returns
# the transitive graph; repeat after copying so dependencies discovered through
# a newly staged DLL are also checked. System32/API-set DLLs are never copied.
copy_runtime_closure() {
	local changed=1
	while ((changed)); do
		changed=0
		while IFS= read -r binary; do
			while IFS= read -r dependency; do
				case "$dependency" in
				/c/Windows/* | /C/Windows/* | "$PREFIX"/*) continue ;;
				esac
				if [[ -f $dependency ]]; then
					local destination basename
					basename=$(basename "$dependency")
					destination="$PREFIX/bin/$(basename "$dependency")"
					if [[ ! -f $destination ]]; then
						cp "$dependency" "$destination"
						printf '%s\t%s\n' "$basename" "$dependency" \
							>>"$COPIED_RUNTIME_ORIGINS"
						changed=1
					elif ! cmp -s "$dependency" "$destination"; then
						echo "error: conflicting runtime DLL basename $basename" >&2
						echo "  staged: $destination" >&2
						echo "  import: $dependency" >&2
						exit 1
					fi
				fi
			done < <(
				ldd "$binary" | awk '
                    /=> \/.*\.dll/ { print $3 }
                    /^[[:space:]]*\/.*\.dll/ { print $1 }
                '
			)
		done < <(find "$PREFIX/bin" -maxdepth 1 -type f -iname '*.dll' -print)
	done
}
copy_runtime_closure

while IFS= read -r binary; do
	if ldd "$binary" | grep -q 'not found'; then
		echo "error: unresolved runtime dependency for $binary" >&2
		ldd "$binary" >&2
		exit 1
	fi
done < <(find "$PREFIX/bin" -maxdepth 1 -type f -iname '*.dll' -print)

LICENSE_ROOT="$PREFIX/share/licenses/ferrex-libmpv"
mkdir -p "$LICENSE_ROOT/mpv" "$LICENSE_ROOT/ffmpeg" \
	"$LICENSE_ROOT/libass" "$LICENSE_ROOT/libplacebo" \
	"$LICENSE_ROOT/luajit" "$LICENSE_ROOT/runtime-packages"
cp "$WORK/src/mpv/LICENSE.LGPL" "$LICENSE_ROOT/mpv/"
cp "$WORK/src/ffmpeg/COPYING.LGPLv2.1" "$LICENSE_ROOT/ffmpeg/"
cp "$WORK/src/ffmpeg/LICENSE.md" "$LICENSE_ROOT/ffmpeg/"
cp "$WORK/src/libass/COPYING" "$LICENSE_ROOT/libass/"
cp "$WORK/src/libplacebo/LICENSE" "$LICENSE_ROOT/libplacebo/"
cp "$WORK/src/luajit/COPYRIGHT" "$LICENSE_ROOT/luajit/"

printf '%s\n' \
	'mpv=0.41.0' \
	"mpv_commit=$MPV_COMMIT" \
	'client_api=2.5' \
	'gpl=false' \
	'libmpv=true' \
	'gpu_next=true' \
	'd3d11=true' \
	'direct3d=false' \
	'd3d_hwaccel=true' \
	'd3d9_hwaccel=true' \
	'wasapi=true' \
	'gl=false' \
	'vulkan=false' \
	'lua=luajit' \
	'ffmpeg=8.1.2' \
	"ffmpeg_commit=$FFMPEG_COMMIT" \
	'ffmpeg_gpl=false' \
	'ffmpeg_nonfree=false' \
	'ffmpeg_version3=false' \
	'libass=0.17.4' \
	"libass_commit=$LIBASS_COMMIT" \
	'libplacebo=7.360.1' \
	"libplacebo_commit=$LIBPLACEBO_COMMIT" \
	"luajit_commit=$LUAJIT_COMMIT" \
	'toolchain=msys2-ucrt64' \
	>"$LICENSE_ROOT/BUILD_PROFILE"

# Preserve an exhaustive provenance row for every hashed DLL. Exact source
# builds are tied to the profile above; copied runtime DLLs are resolved back to
# their original MSYS2-owned path before querying pacman.
runtime_manifest="$LICENSE_ROOT/runtime-packages/MANIFEST"
: >"$runtime_manifest"

source_component_metadata() {
	local filename="$1"
	case "$filename" in
		libmpv-*.dll | mpv-*.dll)
			printf 'mpv\t0.41.0@%s\tLGPL-2.1-or-later\n' "$MPV_COMMIT"
			;;
		avcodec-*.dll | libavcodec-*.dll | avdevice-*.dll | libavdevice-*.dll | \
		avfilter-*.dll | libavfilter-*.dll | avformat-*.dll | libavformat-*.dll | \
		avutil-*.dll | libavutil-*.dll | swresample-*.dll | libswresample-*.dll | \
		swscale-*.dll | libswscale-*.dll)
			printf 'ffmpeg\t8.1.2@%s\tLGPL-2.1-or-later\n' "$FFMPEG_COMMIT"
			;;
		libass-*.dll)
			printf 'libass\t0.17.4@%s\tISC\n' "$LIBASS_COMMIT"
			;;
		libplacebo-*.dll)
			printf 'libplacebo\t7.360.1@%s\tLGPL-2.1-or-later\n' "$LIBPLACEBO_COMMIT"
			;;
		lua*.dll | libluajit-*.dll)
			printf 'luajit\t%s\tMIT\n' "$LUAJIT_COMMIT"
			;;
		*)
			return 1
			;;
	esac
}

while IFS= read -r dll; do
	filename=$(basename "$dll")
	if grep -Fxq "$filename" "$SOURCE_BUILT_DLLS"; then
		metadata=$(source_component_metadata "$filename") || {
			echo "error: unclassified source-built runtime DLL: $filename" >&2
			exit 1
		}
		IFS=$'\t' read -r component version licenses <<<"$metadata"
		printf '%s\tsource\t%s\t%s\t%s\n' \
			"$filename" "$component" "$version" "$licenses" >>"$runtime_manifest"
		continue
	fi

	origin=$(awk -F '\t' -v filename="$filename" \
		'$1 == filename { print $2; exit }' "$COPIED_RUNTIME_ORIGINS")
	[[ -n $origin && -f $origin ]] || {
		echo "error: copied runtime DLL has no recorded origin: $filename" >&2
		exit 1
	}
	owner=$(pacman -Qqo "$origin" 2>/dev/null || true)
	[[ -n $owner ]] || {
		echo "error: no MSYS2 package owns runtime DLL origin: $origin" >&2
		exit 1
	}
	version=$(pacman -Q "$owner" | awk '{print $2}')
	licenses=$(pacman -Qi "$owner" |
		sed -n 's/^Licenses[[:space:]]*:[[:space:]]*//p')
	[[ -n $version && -n $licenses && $licenses != None ]] || {
		echo "error: incomplete package license metadata for $owner" >&2
		exit 1
	}
	printf '%s\tmsys2\t%s\t%s\t%s\n' \
		"$filename" "$owner" "$version" "$licenses" >>"$runtime_manifest"

	package_directory="$LICENSE_ROOT/runtime-packages/$owner"
	mkdir -p "$package_directory"
	pacman -Qi "$owner" >"$package_directory/PACKAGE_INFO"
	notice_count=0
	while IFS= read -r notice; do
		[[ -f $notice ]] || continue
		destination="$package_directory$notice"
		mkdir -p "$(dirname "$destination")"
		cp "$notice" "$destination"
		notice_count=$((notice_count + 1))
	done < <(pacman -Ql "$owner" |
		awk '$2 ~ /\/share\/licenses\// { print $2 }')
	((notice_count > 0)) || {
		echo "error: package $owner ships no installed license notice" >&2
		exit 1
	}
done < <(find "$PREFIX/bin" -maxdepth 1 -type f -iname '*.dll' -print | LC_ALL=C sort)

(
	cd "$PREFIX/bin"
	sha256sum ./*.dll | sed 's#  \./#  #' >"$LICENSE_ROOT/RUNTIME_DLLS.sha256"
)

echo "validated Ferrex Windows LGPL libmpv SDK: $PREFIX"
