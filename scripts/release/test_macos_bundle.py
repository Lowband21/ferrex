#!/usr/bin/env python3
"""Display-free tests for the macOS install-name policy."""

from __future__ import annotations

import importlib.util
import json
import sys
import tempfile
import unittest
from pathlib import Path


SCRIPT = Path(__file__).with_name("macos_bundle.py")
SPEC = importlib.util.spec_from_file_location("macos_bundle", SCRIPT)
assert SPEC is not None and SPEC.loader is not None
macos_bundle = importlib.util.module_from_spec(SPEC)
sys.modules[SPEC.name] = macos_bundle
SPEC.loader.exec_module(macos_bundle)


class MacOSBundlePolicyTests(unittest.TestCase):
    def record(
        self,
        name: str,
        *,
        install_id: str | None = None,
        dependencies: tuple[str, ...] = (),
        rpaths: tuple[str, ...] = (),
        architectures: tuple[str, ...] = ("arm64",),
        minimum_macos: str | None = "15.0",
        executable: bool = False,
    ) -> macos_bundle.MachORecord:
        return macos_bundle.MachORecord(
            path=Path(name),
            install_id=install_id,
            dependencies=dependencies,
            rpaths=rpaths,
            architectures=architectures,
            minimum_macos=minimum_macos,
            executable=executable,
            install_id_required=not executable,
        )

    def test_parses_otool_dependencies_and_rpaths(self) -> None:
        libraries = """/tmp/Ferrex Player:
\t@rpath/libmpv.2.dylib (compatibility version 2.0.0, current version 2.5.0)
\t/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit (compatibility version 45.0.0, current version 2575.0.0)
"""
        load_commands = """Load command 12
          cmd LC_RPATH
      cmdsize 48
         path @executable_path/../Frameworks (offset 12)
"""
        self.assertEqual(
            macos_bundle.parse_otool_libraries(libraries),
            (
                "@rpath/libmpv.2.dylib",
                "/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit",
            ),
        )
        self.assertEqual(
            macos_bundle.parse_otool_rpaths(load_commands),
            (macos_bundle.APP_RPATH,),
        )

        build_version = """Load command 10
      cmd LC_BUILD_VERSION
  cmdsize 32
 platform MACOS
    minos 15.0
      sdk 15.0
"""
        self.assertEqual(
            macos_bundle.parse_macos_deployment_target(build_version), "15.0"
        )
        self.assertEqual(
            macos_bundle.parse_macos_deployment_target(
                build_version + build_version.replace("15.0", "16.0")
            ),
            "16.0",
        )

    def test_relocatable_closure_passes(self) -> None:
        records = [
            self.record(
                "ferrex-player",
                dependencies=(
                    "@rpath/libmpv.2.dylib",
                    "/System/Library/Frameworks/AppKit.framework/Versions/C/AppKit",
                ),
                rpaths=(macos_bundle.APP_RPATH,),
                executable=True,
            ),
            self.record(
                "libmpv.2.dylib",
                install_id="@rpath/libmpv.2.dylib",
                dependencies=("@rpath/libavcodec.62.dylib",),
            ),
            self.record(
                "libavcodec.62.dylib",
                install_id="@rpath/libavcodec.62.dylib",
                dependencies=("/usr/lib/libSystem.B.dylib",),
            ),
        ]
        macos_bundle.audit_records(
            records,
            framework_names={"libmpv.2.dylib", "libavcodec.62.dylib"},
            required_architecture="arm64",
            executable_name="ferrex-player",
            maximum_deployment_target="15.0",
        )

    def test_rejects_homebrew_dependency_and_rpath(self) -> None:
        record = self.record(
            "ferrex-player",
            dependencies=("/opt/homebrew/opt/mpv/lib/libmpv.2.dylib",),
            rpaths=("/opt/homebrew/lib",),
            executable=True,
        )
        with self.assertRaisesRegex(
            macos_bundle.BundleError, "developer dependency"
        ):
            macos_bundle.audit_records(
                [record],
                framework_names=set(),
                required_architecture="arm64",
                executable_name="ferrex-player",
                maximum_deployment_target="15.0",
            )

    def test_rejects_missing_closure_member_and_noncanonical_id(self) -> None:
        records = [
            self.record(
                "ferrex-player",
                dependencies=("@rpath/libmpv.2.dylib",),
                rpaths=(macos_bundle.APP_RPATH,),
                executable=True,
            ),
            self.record(
                "libmpv.2.dylib",
                install_id="/tmp/build/libmpv.2.dylib",
                dependencies=("@rpath/libplacebo.349.dylib",),
            ),
        ]
        with self.assertRaises(macos_bundle.BundleError) as raised:
            macos_bundle.audit_records(
                records,
                framework_names={"libmpv.2.dylib"},
                required_architecture="arm64",
                executable_name="ferrex-player",
                maximum_deployment_target="15.0",
            )
        detail = str(raised.exception)
        self.assertIn("non-relocatable id", detail)
        self.assertIn("is not bundled", detail)

    def test_rejects_wrong_architecture(self) -> None:
        record = self.record(
            "ferrex-player",
            rpaths=(macos_bundle.APP_RPATH,),
            architectures=("x86_64",),
            executable=True,
        )
        with self.assertRaisesRegex(macos_bundle.BundleError, "lacks architecture"):
            macos_bundle.audit_records(
                [record],
                framework_names=set(),
                required_architecture="arm64",
                executable_name="ferrex-player",
                maximum_deployment_target="15.0",
            )

    def test_rejects_newer_or_missing_deployment_target(self) -> None:
        records = [
            self.record(
                "ferrex-player",
                rpaths=(macos_bundle.APP_RPATH,),
                minimum_macos="16.0",
                executable=True,
            ),
            self.record(
                "libmpv.2.dylib",
                install_id="@rpath/libmpv.2.dylib",
                minimum_macos=None,
            ),
        ]
        with self.assertRaises(macos_bundle.BundleError) as raised:
            macos_bundle.audit_records(
                records,
                framework_names={"libmpv.2.dylib"},
                required_architecture="arm64",
                executable_name="ferrex-player",
                maximum_deployment_target="15.0",
            )
        self.assertIn("requires macOS 16.0", str(raised.exception))
        self.assertIn("has no macOS deployment-target", str(raised.exception))

    def test_existing_dependency_must_be_in_declared_search_root(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            declared = root / "declared"
            declared.mkdir()
            allowed = declared / "liballowed.dylib"
            outside = root / "liboutside.dylib"
            allowed.touch()
            outside.touch()
            executable = root / "ferrex-player"
            executable.touch()
            resolver = macos_bundle.DependencyResolver([declared], executable)
            self.assertEqual(resolver.resolve(str(allowed), executable), allowed)
            with self.assertRaisesRegex(
                macos_bundle.BundleError, "outside declared search roots"
            ):
                resolver.resolve(str(outside), executable)

    def test_normalizes_prerelease_bundle_version(self) -> None:
        self.assertEqual(macos_bundle.apple_bundle_version("0.1.2-alpha.1"), "0.1.2")

    def test_rewrites_vulkan_icd_to_bundled_moltenvk(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "MoltenVK_icd.json"
            source.write_text(
                json.dumps(
                    {
                        "file_format_version": "1.0.0",
                        "ICD": {"library_path": "/opt/homebrew/lib/libMoltenVK.dylib"},
                    }
                ),
                encoding="utf-8",
            )
            target = macos_bundle.stage_vulkan_icd(
                source, root / "Resources", "libMoltenVK.dylib"
            )
            manifest = json.loads(target.read_text(encoding="utf-8"))
            self.assertEqual(
                manifest["ICD"]["library_path"],
                "../../../Frameworks/libMoltenVK.dylib",
            )

    def test_stages_hashed_ca_bundle(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            source = root / "source.pem"
            source.write_text(
                "-----BEGIN CERTIFICATE-----\nZmFrZQ==\n-----END CERTIFICATE-----\n",
                encoding="utf-8",
            )
            target = macos_bundle.stage_ca_bundle(source, root / "Resources")
            digest = (target.parent / "cacert.pem.sha256").read_text(
                encoding="utf-8"
            )
            self.assertIn("cacert.pem", digest)
            self.assertEqual(target.read_bytes(), source.read_bytes())

    def test_stages_and_audits_presenter_build_mode(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            app = Path(temporary) / "Ferrex Player.app"
            resources = app / "Contents/Resources"
            resources.mkdir(parents=True)
            target = macos_bundle.stage_presenter_build_mode("spike", resources)
            self.assertEqual(target.read_text(encoding="utf-8"), "spike\n")
            self.assertEqual(macos_bundle.load_presenter_build_mode(app), "spike")

            target.write_text("unknown\n", encoding="utf-8")
            with self.assertRaisesRegex(
                macos_bundle.BundleError, "invalid presenter build-mode"
            ):
                macos_bundle.load_presenter_build_mode(app)

    def test_rejects_invalid_presenter_mode_before_staging(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            with self.assertRaisesRegex(
                macos_bundle.BundleError, "invalid macOS presenter build mode"
            ):
                macos_bundle.stage_presenter_build_mode(
                    "production", Path(temporary)
                )

    def test_rejects_forbidden_gstreamer_dependency(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            app = Path(temporary) / "Ferrex Player.app"
            plugins = app / "Contents/PlugIns/gstreamer-1.0"
            resources = app / "Contents/Resources"
            plugins.mkdir(parents=True)
            resources.mkdir(parents=True)
            plugin = plugins / "libgstplayback.dylib"
            plugin.touch()
            (resources / "gstreamer-plugin-manifest.txt").write_text(
                f"{plugin.name}\n", encoding="utf-8"
            )
            record = self.record(
                str(plugin),
                install_id="@rpath/libgstplayback.dylib",
                dependencies=("@rpath/libavcodec.62.dylib",),
            )
            with self.assertRaisesRegex(
                macos_bundle.BundleError, "forbidden dependency"
            ):
                macos_bundle.audit_gstreamer_runtime(app, [record])

    def test_system_library_policy_is_narrow(self) -> None:
        self.assertTrue(macos_bundle.is_system_library("/usr/lib/libSystem.B.dylib"))
        self.assertTrue(
            macos_bundle.is_system_library(
                "/System/Library/Frameworks/Cocoa.framework/Versions/A/Cocoa"
            )
        )
        self.assertFalse(
            macos_bundle.is_system_library("/usr/local/lib/libmpv.2.dylib")
        )


if __name__ == "__main__":
    unittest.main()
