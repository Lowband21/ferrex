#!/usr/bin/env python3
"""Static guardrails for the reviewed macOS native playback build profile."""

from __future__ import annotations

import unittest
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
BUILD = (ROOT / "scripts/release/macos-build-libmpv.sh").read_text(encoding="utf-8")
PLUGINS = (ROOT / "scripts/release/macos-gstreamer-plugins.txt").read_text(
    encoding="utf-8"
)
SMOKE = (ROOT / "scripts/release/macos_gstreamer_bundle_smoke.c").read_text(
    encoding="utf-8"
)
WORKFLOW = (ROOT / ".github/workflows/ci.yml").read_text(encoding="utf-8")
RELEASE_WORKFLOW = (ROOT / ".github/workflows/release.yml").read_text(
    encoding="utf-8"
)
DIST_WORKSPACE = (ROOT / "dist-workspace.toml").read_text(encoding="utf-8")


class MacOSBuildProfileTests(unittest.TestCase):
    def test_ci_uses_default_apple_silicon_presenter_without_distribution(
        self,
    ) -> None:
        self.assertIn("target: aarch64-apple-darwin", WORKFLOW)
        self.assertNotIn("x86_64-apple-darwin", WORKFLOW)
        self.assertNotIn("FERREX_MPV_MACOS_PRESENTER", WORKFLOW)
        self.assertIn("--presenter-mode enabled", WORKFLOW)
        self.assertFalse((ROOT / ".github/workflows/macos-dist.yml").exists())
        self.assertNotIn("apple-darwin", RELEASE_WORKFLOW)
        self.assertNotIn("apple-darwin", DIST_WORKSPACE)

    def test_sources_and_revisions_are_pinned(self) -> None:
        for expected in (
            'MPV_VERSION="0.41.0"',
            'FFMPEG_COMMIT="38b88335f99e76ed89ff3c93f877fdefce736c13"',
            'LIBPLACEBO_COMMIT="cee9b076f2c63104ccfd497fa79c39a867293ec4"',
            'LIBASS_COMMIT="bbb3c7f1570a4a021e52683f3fbdf74fe492ae84"',
            'LUA_VERSION="5.2.4"',
            'LUA_ARCHIVE_SHA256="b9e2e4aad6789b3b63a056d442f7b39f0ecfca3ae0f1fc0ae4e9614401b69f4b"',
        ):
            self.assertIn(expected, BUILD)
        self.assertGreaterEqual(BUILD.count("--wrap-mode=nofallback"), 3)

    def test_ffmpeg_profile_is_source_built_and_lgpl(self) -> None:
        for flag in (
            "--disable-gpl",
            "--disable-nonfree",
            "--disable-version3",
            "--enable-videotoolbox",
            "--extra-libs=-liconv",
        ):
            self.assertIn(flag, BUILD)
        self.assertNotIn("brew install ffmpeg", BUILD)

    def test_mpv_profile_keeps_required_macos_paths(self) -> None:
        for flag in (
            "-Dcocoa=enabled",
            "-Dswift-build=enabled",
            "-Dmacos-cocoa-cb=enabled",
            "-Dvideotoolbox-pl=enabled",
            "-Dvideotoolbox-gl=enabled",
            "-Dgl-cocoa=enabled",
            "-Dvulkan=enabled",
            "-Dshaderc=disabled",
            "-Dlua=lua52",
        ):
            self.assertIn(flag, BUILD)
        self.assertNotIn("-Dlibass=", BUILD)

    def test_lua_is_pic_library_only_without_readline_or_libdl(self) -> None:
        self.assertIn('make -C "$lua_source/src"', BUILD)
        self.assertIn('MYCFLAGS="-fPIC -DLUA_USE_MACOSX"', BUILD)
        self.assertNotIn("macosx MYCFLAGS", BUILD)
        self.assertNotIn("-lreadline", BUILD)
        self.assertNotIn("-llua -lm -ldl", BUILD)
        self.assertIn("pkg-config --modversion lua52", BUILD)

    def test_libass_and_libplacebo_profiles_are_explicit(self) -> None:
        self.assertIn("-Dcoretext=enabled", BUILD)
        self.assertIn("-Dfontconfig=disabled", BUILD)
        self.assertIn("-Dvk-proc-addr=enabled", BUILD)
        self.assertIn("-Dglslang=disabled", BUILD)
        self.assertIn("-Dshaderc=enabled", BUILD)

    def test_homebrew_trust_is_transitive_but_not_prefix_wide(self) -> None:
        self.assertIn('brew deps --union "${homebrew_direct_formulae[@]}"', BUILD)
        self.assertIn("homebrew-build-inputs.json", BUILD)
        self.assertIn("homebrew-allowed-roots.txt", BUILD)
        self.assertNotIn('"$brew_prefix"/*', BUILD)

    def test_curated_plugins_avoid_conflicting_or_gpl_codec_surfaces(self) -> None:
        for required in (
            "libgstplayback.dylib",
            "libgstapp.dylib",
            "libgstvideoconvertscale.dylib",
            "libgstapplemedia.dylib",
            "libgstosxaudio.dylib",
            "libgstaudiofx.dylib",
            "libgstaom.dylib",
        ):
            self.assertIn(required, PLUGINS)
        for forbidden in (
            "libgstlibav.dylib",
            "libgstassrender.dylib",
            "libgstx264.dylib",
            "libgstx265.dylib",
            "libgstscaletempo.dylib",
        ):
            self.assertNotIn(forbidden, PLUGINS)
        self.assertNotIn("libgstdav1d.dylib", PLUGINS)

    def test_workflows_stage_moltenvk_and_clean_gstreamer_runtime(self) -> None:
        for expected in (
            "molten-vk shaderc vulkan-headers vulkan-loader",
            "--extra-library",
            "libMoltenVK.dylib",
            "libsoup-3.0.0.dylib",
            "--ca-bundle",
            "ca-certificates",
            "strict HTTPS smoke unexpectedly trusted",
            "MoltenVK_icd.json",
            "macos-gstreamer-plugins.txt",
            "gst-plugin-scanner",
            "GST_REGISTRY_1_0",
            "GIO_EXTRA_MODULES",
            "Contents/Resources/gstreamer-1.0",
            "Contents/Resources/gio/modules",
            "libgiognutls",
            "https_test_server.py",
        ):
            self.assertIn(expected, WORKFLOW)
        self.assertNotIn("export DYLD_LIBRARY_PATH", WORKFLOW)
        self.assertNotIn("Contents/PlugIns/gstreamer-1.0", WORKFLOW)
        self.assertNotIn("Contents/PlugIns/gio/modules", WORKFLOW)
        for expected in ("avdec_h264", "atdec", "g_tls_file_database_new"):
            self.assertIn(expected, SMOKE)

    def test_ca_bundle_is_immutable_and_provenance_recorded(self) -> None:
        immutable_path = "share/ca-certificates/cacert.pem"
        merged_path = "etc/ca-certificates/cert.pem"
        self.assertIn(immutable_path, BUILD)
        self.assertIn(immutable_path, WORKFLOW)
        self.assertNotIn(merged_path, WORKFLOW)
        for field in (
            "ca_certificates_version",
            "ca_certificates_source",
            "ca_certificates_source_sha256",
            "ca_certificates_bundle_sha256",
        ):
            self.assertIn(field, BUILD)

    def test_gstreamer_version_and_allowlist_are_exact(self) -> None:
        self.assertIn('GSTREAMER_VERSION="1.28.5"', BUILD)
        self.assertIn("pkg-config --exact-version=", BUILD)
        self.assertIn("gstreamer_plugins_sha256", BUILD)
        self.assertIn('MACOSX_DEPLOYMENT_TARGET:=15.0', BUILD)
        self.assertIn('MACOSX_DEPLOYMENT_TARGET="15.0"', WORKFLOW)

    def test_ci_pins_the_apple_silicon_runner_to_the_deployment_target(self) -> None:
        self.assertIn("os: [ubuntu-latest, macos-15, windows-latest]", WORKFLOW)
        self.assertNotIn("macos-latest", WORKFLOW)


if __name__ == "__main__":
    unittest.main()
