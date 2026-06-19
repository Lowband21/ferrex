#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import json
import os
import struct
import sys
import tempfile
import unittest
from pathlib import Path
from types import SimpleNamespace
from unittest import mock

MODULE_PATH = Path(__file__).with_name("android_visual_qa.py")
SPEC = importlib.util.spec_from_file_location("android_visual_qa", MODULE_PATH)
assert SPEC is not None and SPEC.loader is not None
android_visual_qa = importlib.util.module_from_spec(SPEC)
sys.modules["android_visual_qa"] = android_visual_qa
SPEC.loader.exec_module(android_visual_qa)


class AndroidVisualQaTest(unittest.TestCase):
    def repo_root(self) -> Path:
        return Path(__file__).resolve().parents[2]

    def write_png(self, path: Path, width: int, height: int) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(
            android_visual_qa.PNG_SIGNATURE
            + b"\x00\x00\x00\rIHDR"
            + struct.pack(">II", width, height)
            + b"\x08\x06\x00\x00\x00"
        )

    def write_manifest(
        self,
        path: Path,
        captures: list[dict[str, object]],
        profile_deferrals: list[dict[str, object]] | None = None,
    ) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "status": "passed",
                    "output_dir": str(path.parent),
                    "captures": captures,
                    "profile_deferrals": profile_deferrals or [],
                    "failures": [],
                }
            ),
            encoding="utf-8",
        )

    def target_config(self, target: str = "phone", serial: str = "serial-1") -> android_visual_qa.TargetConfig:
        package = "com.ferrex.android.debug" if target == "phone" else "com.ferrex.android.tv.debug"
        return android_visual_qa.TargetConfig(
            target=target,
            serial=serial,
            default_serial=serial,
            package=package,
            apk_path=self.repo_root() / f"build/{target}.apk",
            expected_size=(1080, 2400) if target == "phone" else (1920, 1080),
            screenshot_helper=f"ferrex-android-screenshot-{target}",
        )

    def test_registry_parses_required_phone_and_tv_scenarios(self) -> None:
        registry = android_visual_qa.ScenarioRegistry.load(self.repo_root())

        self.assertIn("phone-home", registry.by_id)
        self.assertIn("tv-home-focus", registry.by_id)
        self.assertIn("phone-theater-plate-bright", registry.by_id)
        self.assertIn("tv-theater-plate-playback-entry", registry.by_id)
        self.assertEqual("phone", registry.by_id["phone-home"].target)
        self.assertEqual("tv", registry.by_id["tv-home-focus"].target)
        self.assertEqual("phone", registry.by_id["phone-theater-plate-bright"].target)
        self.assertEqual("tv", registry.by_id["tv-theater-plate-playback-entry"].target)
        self.assertEqual(registry.scenarios[0].id, "phone-home")
        self.assertEqual(len(registry.scenarios), len({scenario.id for scenario in registry.scenarios}))

    def test_scenario_selection_rejects_cross_target_request(self) -> None:
        registry = android_visual_qa.ScenarioRegistry.load(self.repo_root())

        with self.assertRaises(android_visual_qa.VisualQaError):
            registry.select("phone", "tv-home-focus")

        phone_ids = [scenario.id for scenario in registry.select("phone", "all")]
        self.assertTrue(phone_ids)
        self.assertTrue(all(scenario_id.startswith("phone-") for scenario_id in phone_ids))

    def test_scenario_selection_accepts_comma_separated_phone_and_tv_ids(self) -> None:
        registry = android_visual_qa.ScenarioRegistry.load(self.repo_root())

        selected = registry.select("all", "phone-home,tv-home-focus,phone-home")

        self.assertEqual([scenario.id for scenario in selected], ["phone-home", "tv-home-focus"])
        with self.assertRaises(android_visual_qa.VisualQaError):
            registry.select("all", "all,phone-home")

    def test_png_validation_reads_ihdr_dimensions(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            png = Path(tmp) / "capture.png"
            png.write_bytes(
                android_visual_qa.PNG_SIGNATURE
                + b"\x00\x00\x00\rIHDR"
                + struct.pack(">II", 1080, 2400)
                + b"\x08\x06\x00\x00\x00"
            )

            dimensions = android_visual_qa.validate_png(png, (1080, 2400))

            self.assertEqual(dimensions.width, 1080)
            self.assertEqual(dimensions.height, 2400)
            with self.assertRaises(android_visual_qa.VisualQaError):
                android_visual_qa.validate_png(png, (1920, 1080))

    def test_capture_screenshot_removes_stale_helper_output(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "capture.png"
            self.write_png(output, 1800, 1200)
            config = SimpleNamespace(
                serial="emulator-5554",
                default_serial="emulator-5554",
                screenshot_helper="ferrex-android-screenshot-phone",
            )

            with mock.patch.object(android_visual_qa.shutil, "which", return_value="/bin/helper"), mock.patch.object(
                android_visual_qa,
                "run_command",
                return_value=android_visual_qa.RunResult(stdout="", stderr="", returncode=0),
            ):
                result = android_visual_qa.capture_screenshot(
                    "adb",
                    config,
                    output,
                    android_visual_qa.SCREENSHOT_MODE_HELPER_COMPATIBLE,
                    helper_compatible_profile=True,
                )

            self.assertFalse(output.exists())
            self.assertEqual(result["method"], "nix-screenshot-helper")

    def test_redaction_removes_tokens_authorization_and_origins(self) -> None:
        sample = (
            "Authorization: Bearer secret-token\n"
            "GET http://192.168.1.20:8096/play?access_token=abc123&ticket=ticket123\n"
            "refresh_token=refresh-secret password=hunter2 https://media.example.com:443/path\n"
        )

        redacted = android_visual_qa.redact_text(sample)

        self.assertNotIn("secret-token", redacted)
        self.assertNotIn("abc123", redacted)
        self.assertNotIn("ticket123", redacted)
        self.assertNotIn("refresh-secret", redacted)
        self.assertNotIn("hunter2", redacted)
        self.assertNotIn("192.168.1.20", redacted)
        self.assertNotIn("media.example.com", redacted)
        self.assertIn("<redacted>", redacted)
        self.assertIn("<redacted-origin>", redacted)
        self.assertEqual('password="redacted"', android_visual_qa.redact_xml_text('password="hunter2"'))

    def test_timing_summary_aggregates_records_breakdowns_and_adb(self) -> None:
        records = [
            {
                "target": "phone",
                "profile": "phone-portrait",
                "timings_ms": {"total": 100},
                "screenshot_capture": {"method": "adb-exec-out-screencap"},
                "adb_command_summary": {
                    "total_count": 2,
                    "total_duration_ms": 30,
                    "categories": {"shell:wm": {"count": 2, "duration_ms": 30}},
                },
            },
            {
                "target": "phone",
                "profile": "phone-landscape-foldable",
                "timings_ms": {"total": 200},
                "screenshot_capture": {"method": "nix-screenshot-helper"},
                "adb_command_summary": {
                    "total_count": 1,
                    "total_duration_ms": 10,
                    "categories": {"shell:am": {"count": 1, "duration_ms": 10}},
                },
            },
            {
                "target": "tv",
                "profile": "tv-1080p",
                "timings_ms": {"total": 300},
                "screenshot_capture": {"method": "adb-exec-out-screencap"},
                "adb_command_summary": {
                    "total_count": 1,
                    "total_duration_ms": 50,
                    "categories": {"exec-out:screencap": {"count": 1, "duration_ms": 50}},
                },
            },
        ]

        summary = android_visual_qa.build_timing_summary(
            records,
            gate_primitives=[{"name": "build", "status": "passed", "duration_ms": 50}],
            manifest_write_ms=7,
        )

        self.assertEqual(summary["record_count"], 3)
        self.assertEqual(summary["total"], 657)
        self.assertEqual(summary["p50"], 200)
        self.assertEqual(summary["p95"], 300)
        self.assertEqual(summary["max"], 300)
        self.assertEqual(summary["target_breakdown"]["phone"]["total"], 300)
        self.assertEqual(summary["profile_breakdown"]["tv-1080p"]["max"], 300)
        self.assertEqual(summary["method_breakdown"]["adb-exec-out-screencap"]["count"], 2)
        self.assertEqual(summary["gate_primitive_durations"], {"build": 50})
        self.assertEqual(summary["adb_commands"]["total_count"], 4)
        self.assertEqual(summary["adb_commands"]["categories"]["shell:wm"]["duration_ms"], 30)

    def test_adb_timing_summary_records_categories_without_sensitive_args(self) -> None:
        recorder = android_visual_qa.TimingRecorder()
        recorder.record_adb_command(
            [
                "adb",
                "-s",
                "emulator-5554",
                "shell",
                "am",
                "start",
                "--es",
                "access_token",
                "secret-token",
                "https://media.example.com/private",
            ],
            12,
        )

        serialized = json.dumps(recorder.adb_command_summary(), sort_keys=True)

        self.assertIn("shell:am", serialized)
        self.assertIn("duration_ms", serialized)
        self.assertNotIn("secret-token", serialized)
        self.assertNotIn("access_token", serialized)
        self.assertNotIn("media.example.com", serialized)
        self.assertNotIn("emulator-5554", serialized)

    def test_adb_shell_sections_quotes_remote_script_and_parses_sections(self) -> None:
        output = "\n".join(
            [
                "__FERREX_VISUAL_QA_BEGIN_first__",
                "one",
                "__FERREX_VISUAL_QA_END_first__",
                "__FERREX_VISUAL_QA_BEGIN_second__",
                "two",
                "__FERREX_VISUAL_QA_END_second__",
            ]
        )
        with mock.patch.object(
            android_visual_qa,
            "adb_shell",
            return_value=android_visual_qa.RunResult(output, "", 0),
        ) as adb_shell:
            sections = android_visual_qa.adb_shell_sections(
                "adb",
                "serial-1",
                (("first", "printf one"), ("second", "printf two")),
                timeout=12,
            )

        adb_shell.assert_called_once()
        call_args = adb_shell.call_args.args
        self.assertEqual(call_args[:2], ("adb", "serial-1"))
        self.assertEqual(len(call_args), 3)
        self.assertTrue(call_args[2].startswith("sh -c "))
        self.assertIn("__FERREX_VISUAL_QA_BEGIN_first__", call_args[2])
        self.assertEqual(sections, {"first": "one", "second": "two"})

    def test_metadata_collectors_batch_shell_probes(self) -> None:
        config = self.target_config()
        calls: list[tuple[tuple[str, ...], int]] = []

        def fake_sections(adb: str, serial: str, sections: object, *, timeout: int = 60) -> dict[str, str]:
            del adb, serial
            names = tuple(name for name, _ in sections)
            calls.append((names, timeout))
            if "package_path" in names:
                return {
                    "package_path": "package:/data/app/com.ferrex/base.apk",
                    "package_dump": "  versionCode=42 minSdk=23\n  versionName=1.2.3\n  firstInstallTime=2026-01-01\n  lastUpdateTime=2026-01-02\n",
                }
            return {
                "sdk": "35",
                "release": "15",
                "model": "Pixel",
                "device": "emu",
                "manufacturer": "Google",
                "brand": "google",
                "product": "sdk_phone64",
                "abi": "arm64-v8a",
                "wm_size": "Physical size: 1080x2400\nOverride size: 1080x2400",
                "wm_density": "Physical density: 440\nOverride density: 440",
                "features": "feature:android.software.leanback\nfeature:android.hardware.touchscreen",
            }

        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            android_visual_qa,
            "adb_shell_sections",
            side_effect=fake_sections,
        ):
            config = android_visual_qa.TargetConfig(
                **{**config.__dict__, "apk_path": Path(tmp) / "app.apk"}
            )
            config.apk_path.write_bytes(b"apk")
            serial_metadata = android_visual_qa.collect_serial_metadata("adb", config)
            package_metadata = android_visual_qa.collect_package_metadata("adb", config)

        self.assertEqual(len(calls), 2)
        self.assertEqual(
            calls[0][0],
            (
                "sdk",
                "release",
                "model",
                "device",
                "manufacturer",
                "brand",
                "product",
                "abi",
                "wm_size",
                "wm_density",
                "features",
            ),
        )
        self.assertEqual(calls[0][1], 60)
        self.assertEqual(calls[1][0], ("package_path", "package_dump"))
        self.assertEqual(calls[1][1], 90)
        self.assertTrue(serial_metadata["leanback"])
        self.assertEqual(serial_metadata["properties"]["sdk"], "35")
        self.assertEqual(package_metadata["installed_package_path"], "/data/app/com.ferrex/base.apk")
        self.assertEqual(package_metadata["version_code"], "42")
        self.assertTrue(package_metadata["host_apk"]["exists"])

    def test_run_cache_reuses_metadata_and_reports_provenance(self) -> None:
        config = self.target_config()
        cache = android_visual_qa.RunCache()

        with mock.patch.object(android_visual_qa, "require_serial_present") as require_serial, mock.patch.object(
            android_visual_qa,
            "collect_serial_metadata",
            return_value={"serial": config.serial, "properties": {"sdk": "35"}},
        ) as collect_serial, mock.patch.object(
            android_visual_qa,
            "collect_package_metadata",
            return_value={"package_name": config.package, "host_apk": {"path": "app.apk"}},
        ) as collect_package:
            first_ready = cache.require_serial_present("adb", config)
            second_ready = cache.require_serial_present("adb", config)
            first_serial = cache.serial_metadata("adb", config)
            first_serial.value["properties"] = {"sdk": "mutated"}
            second_serial = cache.serial_metadata("adb", config)
            first_package = cache.package_metadata("adb", config)
            second_package = cache.package_metadata("adb", config)

        require_serial.assert_called_once_with("adb", config.serial)
        collect_serial.assert_called_once_with("adb", config)
        collect_package.assert_called_once_with("adb", config)
        self.assertFalse(first_ready.provenance["hit"])
        self.assertTrue(second_ready.provenance["hit"])
        self.assertFalse(first_serial.provenance["hit"])
        self.assertTrue(second_serial.provenance["hit"])
        self.assertEqual(second_serial.value["properties"], {"sdk": "35"})
        self.assertFalse(first_package.provenance["hit"])
        self.assertTrue(second_package.provenance["hit"])
        summary = cache.summary()
        self.assertEqual(summary["serial_readiness"]["lookups"], 2)
        self.assertEqual(summary["serial_metadata"]["hits"], 1)
        self.assertEqual(summary["serial_metadata"]["misses"], 1)
        self.assertEqual(summary["package_metadata"]["entries"], 1)

    def test_capture_command_failure_invalidates_cached_target_metadata(self) -> None:
        config = self.target_config()
        cache = android_visual_qa.RunCache()
        scenario = android_visual_qa.Scenario("phone-home", "phone")
        profile = android_visual_qa.VIEWPORT_PROFILES["phone-portrait"]
        failure = android_visual_qa.CommandError(["adb", "shell", "am"], 1, "", "device offline")

        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            android_visual_qa,
            "require_serial_present",
        ), mock.patch.object(
            android_visual_qa,
            "collect_serial_metadata",
            return_value={"serial": config.serial},
        ), mock.patch.object(
            android_visual_qa,
            "collect_package_metadata",
            return_value={"package_name": config.package},
        ), mock.patch.object(
            android_visual_qa,
            "force_stop_package",
        ), mock.patch.object(
            android_visual_qa,
            "launch_scenario",
            side_effect=failure,
        ), mock.patch.object(
            android_visual_qa,
            "capture_failure_logcat",
            return_value={"path": "failure-logcat.txt", "redacted": True},
        ):
            record = android_visual_qa.capture_one(
                adb="adb",
                config=config,
                scenario=scenario,
                profile=profile,
                output_dir=Path(tmp),
                settle_ms=1,
                log_lines=1,
                screenshot_mode=android_visual_qa.SCREENSHOT_MODE_FAST,
                run_cache=cache,
                viewport_event_index=0,
            )

        self.assertEqual(record["status"], "failed")
        self.assertEqual(record["failure_logcat"], {"path": "failure-logcat.txt", "redacted": True})
        summary = cache.summary()
        for category in ("serial_readiness", "serial_metadata", "package_metadata"):
            self.assertEqual(summary[category]["entries"], 0)
            self.assertEqual(summary[category]["invalidations"], 1)
            self.assertEqual(summary[category]["invalidation_reasons"], {"record_failure": 1})

    def test_viewport_apply_restores_previous_snapshot_on_partial_failure(self) -> None:
        config = self.target_config()
        profile = android_visual_qa.ViewportProfile(
            name="test-profile",
            target="phone",
            expected_size=(1080, 2400),
            wm_size=(1080, 2400),
            wm_density=440,
            description="test",
        )
        before = android_visual_qa.WmOverrideSnapshot(
            raw_size="Physical size: 1080x2400",
            raw_density="Physical density: 420",
            override_size="1080x2400",
            override_density=None,
            accelerometer_rotation="0",
            user_rotation="0",
        )
        failure = android_visual_qa.CommandError(["adb", "shell", "wm", "density"], 1, "", "wm failed")

        with mock.patch.object(android_visual_qa, "wm_snapshot", return_value=before), mock.patch.object(
            android_visual_qa,
            "adb_shell",
            side_effect=[failure],
        ), mock.patch.object(android_visual_qa, "restore_viewport_profile") as restore:
            with self.assertRaises(android_visual_qa.CommandError):
                android_visual_qa.apply_viewport_profile_for_group("adb", config, profile)

        restore.assert_called_once_with("adb", config, before)

    def test_viewport_apply_skips_when_profile_is_already_active(self) -> None:
        config = self.target_config()
        profile = android_visual_qa.ViewportProfile(
            name="active-profile",
            target="phone",
            expected_size=(1080, 2400),
            wm_size=(1080, 2400),
            wm_density=440,
            description="active",
        )
        snapshot = android_visual_qa.WmOverrideSnapshot(
            raw_size="Physical size: 1080x2400\nOverride size: 1080x2400",
            raw_density="Physical density: 440\nOverride density: 440",
            override_size="1080x2400",
            override_density="440",
            accelerometer_rotation="0",
            user_rotation="0",
        )

        with mock.patch.object(android_visual_qa, "wm_snapshot", return_value=snapshot), mock.patch.object(
            android_visual_qa,
            "adb_shell",
        ) as adb_shell:
            evidence = android_visual_qa.apply_viewport_profile_for_group("adb", config, profile)

        self.assertTrue(evidence.skipped)
        self.assertEqual(evidence.actions, ())
        self.assertEqual(evidence.before, snapshot)
        self.assertEqual(evidence.after, snapshot)
        adb_shell.assert_not_called()

    def test_capture_plan_groups_profiles_and_preserves_record_order(self) -> None:
        phone_config = self.target_config("phone", "phone-serial")
        tv_config = self.target_config("tv", "tv-serial")
        selected = [
            android_visual_qa.Scenario("phone-a", "phone"),
            android_visual_qa.Scenario("tv-a", "tv"),
            android_visual_qa.Scenario("phone-b", "phone"),
        ]
        registry = android_visual_qa.ScenarioRegistry(selected, Path("registry.kt"))
        phone_profiles = (
            android_visual_qa.ViewportProfile("phone-p1", "phone", (1080, 2400), (1080, 2400), 440, "phone p1"),
            android_visual_qa.ViewportProfile("phone-p2", "phone", (1800, 1200), (1800, 1200), 420, "phone p2"),
        )
        tv_profiles = (
            android_visual_qa.ViewportProfile("tv-p1", "tv", (1920, 1080), (1920, 1080), 320, "tv p1"),
        )
        profiles_by_target = {"phone": phone_profiles, "tv": tv_profiles}
        snapshot = android_visual_qa.WmOverrideSnapshot("size", "density", None, None)
        args = SimpleNamespace(
            hardware=False,
            no_nix_screenshot=False,
            screenshot_mode=android_visual_qa.SCREENSHOT_MODE_FAST,
            log_lines=1,
            settle_ms=1,
            adb="adb",
            effective_argv=["capture"],
        )

        def fake_apply(
            *,
            adb: str,
            config: object,
            profile: object,
            viewport_events: list[dict[str, object]],
            force: bool = False,
        ) -> tuple[int, dict[str, object]]:
            del adb, force
            event = {
                "index": len(viewport_events),
                "operation": "apply",
                "target": config.target,
                "serial": config.serial,
                "profile": profile.name,
                "status": "passed",
                "duration_ms": 1,
            }
            viewport_events.append(event)
            return event["index"], event

        def fake_restore(*, adb: str, config: object, initial_snapshot: object, viewport_events: list[dict[str, object]]) -> dict[str, object]:
            del adb, initial_snapshot
            event = {
                "index": len(viewport_events),
                "operation": "final_restore",
                "target": config.target,
                "serial": config.serial,
                "status": "passed",
                "duration_ms": 1,
            }
            viewport_events.append(event)
            return event

        def fake_capture_one(**kwargs: object) -> dict[str, object]:
            scenario = kwargs["scenario"]
            profile = kwargs["profile"]
            config = kwargs["config"]
            event_index = kwargs["viewport_event_index"]
            return {
                "target": config.target,
                "profile": profile.name,
                "scenario_id": scenario.id,
                "serial": config.serial,
                "status": "passed",
                "screenshot_path": str(Path(kwargs["output_dir"]) / profile.name / f"{scenario.id}.png"),
                "viewport_apply_event_index": event_index,
                "timings_ms": {"total": 1},
                "adb_command_summary": {"total_count": 0, "total_duration_ms": 0, "categories": {}},
            }

        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            android_visual_qa,
            "target_configs",
            return_value={"phone": phone_config, "tv": tv_config},
        ), mock.patch.object(
            android_visual_qa,
            "selected_viewport_profiles",
            return_value=profiles_by_target,
        ), mock.patch.object(
            android_visual_qa,
            "resolve_executable",
            return_value="adb",
        ), mock.patch.object(
            android_visual_qa,
            "collect_command_versions",
            return_value={"adb": {"path": "adb"}},
        ), mock.patch.object(
            android_visual_qa,
            "require_serial_present",
        ), mock.patch.object(
            android_visual_qa,
            "wm_snapshot",
            return_value=snapshot,
        ), mock.patch.object(
            android_visual_qa,
            "append_viewport_apply_event",
            side_effect=fake_apply,
        ), mock.patch.object(
            android_visual_qa,
            "append_final_viewport_restore_event",
            side_effect=fake_restore,
        ), mock.patch.object(
            android_visual_qa,
            "capture_one",
            side_effect=fake_capture_one,
        ):
            status = android_visual_qa.run_capture_plan(
                args=args,
                repo_root=self.repo_root(),
                registry=registry,
                selected=selected,
                output_dir=Path(tmp),
                command_name="android-visual-qa capture",
            )
            manifest = json.loads((Path(tmp) / "manifest.json").read_text(encoding="utf-8"))

        self.assertEqual(status, 0)
        self.assertEqual(
            [(record["profile"], record["scenario_id"]) for record in manifest["captures"]],
            [
                ("phone-p1", "phone-a"),
                ("phone-p1", "phone-b"),
                ("phone-p2", "phone-a"),
                ("phone-p2", "phone-b"),
                ("tv-p1", "tv-a"),
            ],
        )
        self.assertEqual(
            [(event["operation"], event.get("profile"), event["target"]) for event in manifest["viewport_events"]],
            [
                ("apply", "phone-p1", "phone"),
                ("apply", "phone-p2", "phone"),
                ("final_restore", None, "phone"),
                ("apply", "tv-p1", "tv"),
                ("final_restore", None, "tv"),
            ],
        )
        self.assertEqual(
            [(record["viewport_apply_event_index"], record["final_viewport_restore_event_index"]) for record in manifest["captures"]],
            [(0, 2), (0, 2), (1, 2), (1, 2), (3, 4)],
        )
        self.assertEqual(
            [event.get("record_count") for event in manifest["viewport_events"] if event["operation"] == "apply"],
            [2, 2, 1],
        )
        self.assertEqual(manifest["cache_summary"]["serial_readiness"]["misses"], 2)
        self.assertEqual(manifest["viewport_summary"]["event_count"], 5)

    def test_target_workers_merge_phone_and_tv_results_deterministically(self) -> None:
        phone_config = self.target_config("phone", "phone-serial")
        tv_config = self.target_config("tv", "tv-serial")
        selected = [
            android_visual_qa.Scenario("phone-a", "phone"),
            android_visual_qa.Scenario("tv-a", "tv"),
            android_visual_qa.Scenario("phone-b", "phone"),
        ]
        registry = android_visual_qa.ScenarioRegistry(selected, Path("registry.kt"))
        phone_profiles = (
            android_visual_qa.ViewportProfile("phone-p1", "phone", (1080, 2400), (1080, 2400), 440, "phone p1"),
        )
        tv_profiles = (
            android_visual_qa.ViewportProfile("tv-p1", "tv", (1920, 1080), (1920, 1080), 320, "tv p1"),
        )
        profiles_by_target = {"phone": phone_profiles, "tv": tv_profiles}
        snapshot = android_visual_qa.WmOverrideSnapshot("size", "density", None, None)
        args = SimpleNamespace(
            hardware=False,
            no_nix_screenshot=False,
            screenshot_mode=android_visual_qa.SCREENSHOT_MODE_FAST,
            log_lines=1,
            settle_ms=1,
            adb="adb",
            target_workers=2,
            effective_argv=["capture", "--target-workers", "2"],
        )

        def fake_apply(
            *,
            adb: str,
            config: object,
            profile: object,
            viewport_events: list[dict[str, object]],
            force: bool = False,
        ) -> tuple[int, dict[str, object]]:
            del adb, force
            event = {
                "index": len(viewport_events),
                "operation": "apply",
                "target": config.target,
                "serial": config.serial,
                "profile": profile.name,
                "status": "passed",
                "duration_ms": 1,
            }
            viewport_events.append(event)
            return event["index"], event

        def fake_restore(*, adb: str, config: object, initial_snapshot: object, viewport_events: list[dict[str, object]]) -> dict[str, object]:
            del adb, initial_snapshot
            event = {
                "index": len(viewport_events),
                "operation": "final_restore",
                "target": config.target,
                "serial": config.serial,
                "status": "passed",
                "duration_ms": 1,
            }
            viewport_events.append(event)
            return event

        def fake_capture_one(**kwargs: object) -> dict[str, object]:
            scenario = kwargs["scenario"]
            profile = kwargs["profile"]
            config = kwargs["config"]
            event_index = kwargs["viewport_event_index"]
            return {
                "target": config.target,
                "profile": profile.name,
                "scenario_id": scenario.id,
                "serial": config.serial,
                "status": "passed",
                "screenshot_path": str(Path(kwargs["output_dir"]) / profile.name / f"{scenario.id}.png"),
                "viewport_apply_event_index": event_index,
                "timings_ms": {"total": 1},
                "adb_command_summary": {"total_count": 0, "total_duration_ms": 0, "categories": {}},
            }

        with tempfile.TemporaryDirectory() as tmp, mock.patch.object(
            android_visual_qa,
            "target_configs",
            return_value={"phone": phone_config, "tv": tv_config},
        ), mock.patch.object(
            android_visual_qa,
            "selected_viewport_profiles",
            return_value=profiles_by_target,
        ), mock.patch.object(
            android_visual_qa,
            "resolve_executable",
            return_value="adb",
        ), mock.patch.object(
            android_visual_qa,
            "collect_command_versions",
            return_value={"adb": {"path": "adb"}},
        ), mock.patch.object(
            android_visual_qa,
            "require_serial_present",
        ), mock.patch.object(
            android_visual_qa,
            "wm_snapshot",
            return_value=snapshot,
        ), mock.patch.object(
            android_visual_qa,
            "append_viewport_apply_event",
            side_effect=fake_apply,
        ), mock.patch.object(
            android_visual_qa,
            "append_final_viewport_restore_event",
            side_effect=fake_restore,
        ), mock.patch.object(
            android_visual_qa,
            "capture_one",
            side_effect=fake_capture_one,
        ):
            status = android_visual_qa.run_capture_plan(
                args=args,
                repo_root=self.repo_root(),
                registry=registry,
                selected=selected,
                output_dir=Path(tmp),
                command_name="android-visual-qa capture",
            )
            manifest = json.loads((Path(tmp) / "manifest.json").read_text(encoding="utf-8"))

        self.assertEqual(status, 0)
        self.assertEqual(manifest["target_workers"]["worker_count"], 2)
        self.assertTrue(manifest["target_workers"]["enabled"])
        self.assertEqual([target["target"] for target in manifest["target_workers"]["targets"]], ["phone", "tv"])
        self.assertEqual([timing["target"] for timing in manifest["target_workers"]["timings"]], ["phone", "tv"])
        self.assertEqual(
            [(record["profile"], record["scenario_id"]) for record in manifest["captures"]],
            [("phone-p1", "phone-a"), ("phone-p1", "phone-b"), ("tv-p1", "tv-a")],
        )
        self.assertEqual(
            [(event["operation"], event.get("profile"), event["target"], event["index"]) for event in manifest["viewport_events"]],
            [
                ("apply", "phone-p1", "phone", 0),
                ("final_restore", None, "phone", 1),
                ("apply", "tv-p1", "tv", 2),
                ("final_restore", None, "tv", 3),
            ],
        )
        self.assertEqual(
            [(record["viewport_apply_event_index"], record["final_viewport_restore_event_index"]) for record in manifest["captures"]],
            [(0, 1), (0, 1), (2, 3)],
        )

    def test_target_workers_reject_duplicate_concurrent_serials(self) -> None:
        configs = {
            "phone": self.target_config("phone", "shared-serial"),
            "tv": self.target_config("tv", "shared-serial"),
        }

        with self.assertRaises(android_visual_qa.VisualQaError):
            android_visual_qa.effective_target_worker_count(
                SimpleNamespace(target_workers=2),
                ["phone", "tv"],
                configs,
            )

        self.assertEqual(
            android_visual_qa.effective_target_worker_count(
                SimpleNamespace(target_workers=1),
                ["phone", "tv"],
                configs,
            ),
            (1, 1),
        )

    def test_fast_screenshot_path_uses_adb_with_evidence_metadata(self) -> None:
        config = android_visual_qa.TargetConfig(
            target="phone",
            serial="emulator-5554",
            default_serial="emulator-5554",
            package="com.ferrex.android.debug",
            apk_path=Path("app.apk"),
            expected_size=android_visual_qa.PHONE_EXPECTED_SIZE,
            screenshot_helper="ferrex-android-screenshot-phone",
        )

        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "fast.png"

            def fake_run_to_file(command: list[str], path: Path, *, timeout: int) -> android_visual_qa.RunResult:
                self.write_png(path, 1080, 2400)
                return android_visual_qa.RunResult(stdout="", stderr="", returncode=0)

            with mock.patch.object(android_visual_qa.shutil, "which", return_value="/bin/ferrex-android-screenshot-phone"), mock.patch.object(
                android_visual_qa,
                "run_command_to_file",
                side_effect=fake_run_to_file,
            ) as run_fast, mock.patch.object(android_visual_qa, "run_command") as run_helper:
                capture = android_visual_qa.capture_screenshot(
                    "adb",
                    config,
                    output,
                    android_visual_qa.SCREENSHOT_MODE_FAST,
                    helper_compatible_profile=True,
                )

            self.assertEqual(capture["method"], "adb-exec-out-screencap")
            self.assertEqual(capture["requested_mode"], "fast")
            self.assertEqual(capture["serial"], "emulator-5554")
            self.assertEqual(capture["output_path"], str(output))
            self.assertEqual(capture["command_category"], "exec-out:screencap")
            self.assertFalse(capture["helper_compatibility_mode"])
            self.assertFalse(capture["helper_used"])
            self.assertIsInstance(capture["duration_ms"], int)
            run_fast.assert_called_once()
            run_helper.assert_not_called()
            self.assertEqual(android_visual_qa.validate_png(output, (1080, 2400)).width, 1080)

    def test_helper_compatible_screenshot_mode_invokes_helper_when_available(self) -> None:
        config = android_visual_qa.TargetConfig(
            target="phone",
            serial="emulator-5554",
            default_serial="emulator-5554",
            package="com.ferrex.android.debug",
            apk_path=Path("app.apk"),
            expected_size=android_visual_qa.PHONE_EXPECTED_SIZE,
            screenshot_helper="ferrex-android-screenshot-phone",
        )

        with tempfile.TemporaryDirectory() as tmp:
            output = Path(tmp) / "helper.png"

            def fake_run_command(command: list[str], *, timeout: int) -> android_visual_qa.RunResult:
                self.write_png(Path(command[1]), 1080, 2400)
                return android_visual_qa.RunResult(stdout="", stderr="", returncode=0)

            with mock.patch.object(
                android_visual_qa.shutil,
                "which",
                return_value="/nix/store/helper/bin/ferrex-android-screenshot-phone",
            ), mock.patch.object(
                android_visual_qa,
                "run_command",
                side_effect=fake_run_command,
            ) as run_helper, mock.patch.object(android_visual_qa, "run_command_to_file") as run_fast:
                capture = android_visual_qa.capture_screenshot(
                    "adb",
                    config,
                    output,
                    android_visual_qa.SCREENSHOT_MODE_HELPER_COMPATIBLE,
                    helper_compatible_profile=True,
                )

            self.assertEqual(capture["method"], "nix-screenshot-helper")
            self.assertEqual(capture["requested_mode"], "helper-compatible")
            self.assertEqual(capture["command_category"], "helper:ferrex-android-screenshot-phone")
            self.assertTrue(capture["helper_compatibility_mode"])
            self.assertTrue(capture["helper_used"])
            run_helper.assert_called_once()
            run_fast.assert_not_called()
            self.assertEqual(android_visual_qa.validate_png(output, (1080, 2400)).height, 2400)

    def test_capture_one_keeps_png_dimension_validation_mandatory(self) -> None:
        config = android_visual_qa.TargetConfig(
            target="phone",
            serial="emulator-5554",
            default_serial="emulator-5554",
            package="com.ferrex.android.debug",
            apk_path=Path("app.apk"),
            expected_size=android_visual_qa.PHONE_EXPECTED_SIZE,
            screenshot_helper="ferrex-android-screenshot-phone",
        )
        scenario = android_visual_qa.Scenario("phone-home", "phone")
        profile = android_visual_qa.VIEWPORT_PROFILES["phone-portrait"]
        snapshot = android_visual_qa.WmOverrideSnapshot(
            raw_size="Physical size: 1080x2400",
            raw_density="Physical density: 440",
            override_size=None,
            override_density=None,
        )

        with tempfile.TemporaryDirectory() as tmp:

            def fake_capture(
                adb: str,
                target_config: android_visual_qa.TargetConfig,
                output_path: Path,
                screenshot_mode: str,
                *,
                helper_compatible_profile: bool,
            ) -> dict[str, object]:
                self.write_png(output_path, 100, 100)
                return {
                    "method": "adb-exec-out-screencap",
                    "requested_mode": screenshot_mode,
                    "serial": target_config.serial,
                    "output_path": str(output_path),
                    "command_category": "exec-out:screencap",
                    "duration_ms": 1,
                    "helper_compatibility_mode": False,
                    "helper_used": False,
                }

            with mock.patch.object(android_visual_qa, "require_serial_present"), mock.patch.object(
                android_visual_qa,
                "apply_viewport_profile",
                return_value=snapshot,
            ), mock.patch.object(
                android_visual_qa,
                "collect_serial_metadata",
                return_value={},
            ), mock.patch.object(
                android_visual_qa,
                "collect_package_metadata",
                return_value={},
            ), mock.patch.object(android_visual_qa, "force_stop_package"), mock.patch.object(
                android_visual_qa,
                "launch_scenario",
                return_value={},
            ), mock.patch.object(
                android_visual_qa,
                "drive_scenario",
                return_value=[],
            ), mock.patch.object(
                android_visual_qa,
                "capture_screenshot",
                side_effect=fake_capture,
            ), mock.patch.object(
                android_visual_qa,
                "set_viewport_profile",
                return_value={},
            ), mock.patch.object(android_visual_qa.time, "sleep"), mock.patch.object(
                android_visual_qa,
                "capture_failure_logcat",
                return_value={},
            ), mock.patch.object(
                android_visual_qa,
                "restore_viewport_profile",
                return_value={},
            ):
                record = android_visual_qa.capture_one(
                    adb="adb",
                    config=config,
                    scenario=scenario,
                    profile=profile,
                    output_dir=Path(tmp),
                    settle_ms=1,
                    log_lines=1,
                    screenshot_mode=android_visual_qa.SCREENSHOT_MODE_FAST,
                )

        self.assertEqual(record["status"], "failed")
        self.assertIn("screenshot dimensions", record["error"])
        self.assertIn("png_validation", record["timings_ms"])
        self.assertEqual(len(record["screenshot_validation_attempts"]), android_visual_qa.SCREENSHOT_VALIDATION_ATTEMPTS)

    def test_capture_one_retries_transient_dimension_mismatch_and_preserves_invalid_attempt(self) -> None:
        config = android_visual_qa.TargetConfig(
            target="phone",
            serial="emulator-5554",
            default_serial="emulator-5554",
            package="com.ferrex.android.debug",
            apk_path=Path("app.apk"),
            expected_size=android_visual_qa.PHONE_EXPECTED_SIZE,
            screenshot_helper="ferrex-android-screenshot-phone",
        )
        scenario = android_visual_qa.Scenario("phone-home", "phone")
        profile = android_visual_qa.VIEWPORT_PROFILES["phone-portrait"]
        snapshot = android_visual_qa.WmOverrideSnapshot(
            raw_size="Physical size: 1080x2400",
            raw_density="Physical density: 440",
            override_size=None,
            override_density=None,
        )
        capture_calls = 0

        with tempfile.TemporaryDirectory() as tmp:

            def fake_capture(
                adb: str,
                target_config: android_visual_qa.TargetConfig,
                output_path: Path,
                screenshot_mode: str,
                *,
                helper_compatible_profile: bool,
            ) -> dict[str, object]:
                nonlocal capture_calls
                capture_calls += 1
                dimensions = (100, 100) if capture_calls == 1 else profile.expected_size
                self.write_png(output_path, dimensions[0], dimensions[1])
                return {
                    "method": "adb-exec-out-screencap",
                    "requested_mode": screenshot_mode,
                    "serial": target_config.serial,
                    "output_path": str(output_path),
                    "command_category": "exec-out:screencap",
                    "duration_ms": 1,
                    "helper_compatibility_mode": False,
                    "helper_used": False,
                }

            with mock.patch.object(android_visual_qa, "require_serial_present"), mock.patch.object(
                android_visual_qa,
                "apply_viewport_profile",
                return_value=snapshot,
            ), mock.patch.object(
                android_visual_qa,
                "collect_serial_metadata",
                return_value={},
            ), mock.patch.object(
                android_visual_qa,
                "collect_package_metadata",
                return_value={},
            ), mock.patch.object(android_visual_qa, "force_stop_package"), mock.patch.object(
                android_visual_qa,
                "launch_scenario",
                return_value={},
            ), mock.patch.object(
                android_visual_qa,
                "drive_scenario",
                return_value=[],
            ), mock.patch.object(
                android_visual_qa,
                "capture_screenshot",
                side_effect=fake_capture,
            ), mock.patch.object(
                android_visual_qa,
                "set_viewport_profile",
                return_value={"snapshot": {}},
            ) as reapply, mock.patch.object(android_visual_qa.time, "sleep"), mock.patch.object(
                android_visual_qa,
                "restore_viewport_profile",
                return_value={},
            ):
                record = android_visual_qa.capture_one(
                    adb="adb",
                    config=config,
                    scenario=scenario,
                    profile=profile,
                    output_dir=Path(tmp),
                    settle_ms=1,
                    log_lines=1,
                    screenshot_mode=android_visual_qa.SCREENSHOT_MODE_FAST,
                )

            invalid_path = Path(tmp) / "phone-portrait" / "phone-home.attempt-1.invalid.png"
            invalid_preserved = invalid_path.exists()

        self.assertEqual(record["status"], "passed")
        self.assertEqual(record["dimensions"], {"width": 1080, "height": 2400})
        self.assertEqual(capture_calls, 2)
        reapply.assert_called_once()
        attempts = record["screenshot_validation_attempts"]
        self.assertEqual([attempt["status"] for attempt in attempts], ["failed", "passed"])
        self.assertEqual(attempts[0]["output_path"], str(invalid_path))
        self.assertTrue(invalid_preserved)

    def test_hardware_mode_requires_explicit_serial(self) -> None:
        with mock.patch.dict(os.environ, {}, clear=True):
            args = SimpleNamespace(
                target="phone",
                hardware=True,
                hardware_serial=None,
                expected_size=None,
            )
            with self.assertRaises(android_visual_qa.VisualQaError):
                android_visual_qa.target_configs(self.repo_root(), args)

    def test_hardware_mode_can_override_expected_size_explicitly(self) -> None:
        args = SimpleNamespace(
            target="tv",
            hardware=True,
            hardware_serial="ABC123",
            expected_size="1280x720",
        )

        configs = android_visual_qa.target_configs(self.repo_root(), args)

        self.assertEqual(configs["tv"].serial, "ABC123")
        self.assertEqual(configs["tv"].expected_size, (1280, 720))
        self.assertEqual(configs["phone"].serial, "emulator-5554")

    def test_default_viewport_profiles_cover_phone_tv_and_scaled_dimensions(self) -> None:
        registry = android_visual_qa.ScenarioRegistry.load(self.repo_root())
        selected = [registry.by_id["phone-home"], registry.by_id["tv-home-focus"]]
        args = SimpleNamespace(
            target="all",
            hardware=False,
            hardware_serial=None,
            expected_size=None,
            profile=None,
        )
        configs = android_visual_qa.target_configs(self.repo_root(), args)

        profiles = android_visual_qa.selected_viewport_profiles(args, configs, selected)
        plan = android_visual_qa.capture_plan_json("smoke", selected, profiles)

        self.assertEqual([profile.name for profile in profiles["phone"]], ["phone-portrait", "phone-landscape-foldable"])
        self.assertEqual(
            android_visual_qa.REQUIRED_VIEWPORT_PROFILE_NAMES_BY_TARGET,
            {"phone": ("phone-portrait", "phone-landscape-foldable"), "tv": ("tv-1080p", "tv-4k-scaled")},
        )
        self.assertEqual([profile.expected_size for profile in profiles["tv"]], [(1920, 1080), (1920, 1080)])
        self.assertEqual(profiles["tv"][1].wm_size, (3840, 2160))
        self.assertEqual(plan["capture_count"], 4)
        self.assertEqual(
            plan["viewport_profiles"]["phone"][1]["expected_dimensions"],
            {"width": 1800, "height": 1200},
        )
        self.assertEqual(plan["viewport_profiles"]["tv"][1]["wm_size"], "3840x2160")

    def test_accessibility_requirements_match_tags_labels_and_actions(self) -> None:
        scenario = android_visual_qa.Scenario("phone-theater-plate-recovery", "phone")
        requirements = android_visual_qa.accessibility_requirements_for_scenario(scenario)
        xml = """
        <hierarchy>
          <node resource-id="phone.theater-plate.recovery" content-desc="Recovery root" clickable="false" focusable="false" />
          <node resource-id="phone.theater-plate.status.recovery" content-desc="Recovery status: Recovery paths remain visible without clearing app data." clickable="false" focusable="false" />
          <node resource-id="phone.theater-plate.action.recovery.primary" content-desc="Retry" clickable="true" focusable="false" />
          <node resource-id="phone.theater-plate.media.recovery.hero" content-desc="Theater Plate media Recovery Queue: Server unreachable" clickable="true" focusable="false" />
          <node resource-id="phone.theater-plate.rail.recovery.primary" content-desc="Recovery rail" clickable="false" focusable="false" />
          <node resource-id="phone.theater-plate.action.recovery.retry" content-desc="Retry" clickable="true" focusable="false" />
          <node resource-id="phone.theater-plate.action.recovery.change-server" content-desc="Change server" clickable="true" focusable="false" />
          <node resource-id="phone.theater-plate.action.recovery.clear-cache" content-desc="Clear cache" clickable="true" focusable="false" />
          <node resource-id="phone.theater-plate.action.recovery.reset-connection" content-desc="Reset connection" clickable="true" focusable="false" />
          <node resource-id="phone.theater-plate.action.recovery.diagnostics" content-desc="Diagnostics / Export diagnostics" clickable="true" focusable="false" />
        </hierarchy>
        """
        nodes = android_visual_qa.parse_accessibility_nodes(xml)

        checks = android_visual_qa.verify_accessibility_requirements(nodes, requirements)

        self.assertTrue(all(check["status"] == "passed" for check in checks), checks)
        missing_label_nodes = [node for node in nodes if node.get("content-desc") != "Reset connection"]
        missing_label_checks = android_visual_qa.verify_accessibility_requirements(missing_label_nodes, requirements)
        self.assertTrue(any(check["status"] == "failed" for check in missing_label_checks))

    def test_accessibility_dump_strategy_batches_only_validated_default_emulators(self) -> None:
        phone_config = android_visual_qa.TargetConfig(
            target="phone",
            serial="emulator-5554",
            default_serial="emulator-5554",
            package="com.ferrex.android.debug",
            apk_path=Path("phone.apk"),
            expected_size=android_visual_qa.PHONE_EXPECTED_SIZE,
            screenshot_helper="phone-helper",
        )
        tv_config = android_visual_qa.TargetConfig(
            target="tv",
            serial="emulator-5556",
            default_serial="emulator-5556",
            package="com.ferrex.android.tv.debug",
            apk_path=Path("tv.apk"),
            expected_size=android_visual_qa.TV_EXPECTED_SIZE,
            screenshot_helper="tv-helper",
        )

        phone_strategy = android_visual_qa.accessibility_dump_strategy(phone_config, {"properties": {"sdk": "35"}})
        tv_strategy = android_visual_qa.accessibility_dump_strategy(tv_config, {"properties": {"sdk": "34"}})
        wrong_api_strategy = android_visual_qa.accessibility_dump_strategy(phone_config, {"properties": {"sdk": "34"}})
        hardware_strategy = android_visual_qa.accessibility_dump_strategy(
            android_visual_qa.TargetConfig(
                target="phone",
                serial="hardware-serial",
                default_serial="emulator-5554",
                package="com.ferrex.android.debug",
                apk_path=Path("phone.apk"),
                expected_size=android_visual_qa.PHONE_EXPECTED_SIZE,
                screenshot_helper="phone-helper",
            ),
            {"properties": {"sdk": "35"}},
        )

        self.assertTrue(phone_strategy.batched)
        self.assertTrue(tv_strategy.batched)
        self.assertFalse(wrong_api_strategy.batched)
        self.assertFalse(hardware_strategy.batched)

    def test_accessibility_batched_dump_quotes_script_for_remote_shell(self) -> None:
        config = android_visual_qa.TargetConfig(
            target="phone",
            serial="emulator-5554",
            default_serial="emulator-5554",
            package="com.ferrex.android.debug",
            apk_path=Path("phone.apk"),
            expected_size=android_visual_qa.PHONE_EXPECTED_SIZE,
            screenshot_helper="phone-helper",
        )

        with mock.patch.object(
            android_visual_qa,
            "adb_shell",
            return_value=android_visual_qa.RunResult("<hierarchy />", "", 0),
        ) as adb_shell:
            xml_text = android_visual_qa.dump_accessibility_xml_batched("adb", config, "/sdcard/ferrex-a11y.xml")

        self.assertEqual(xml_text, "<hierarchy />")
        adb_shell.assert_called_once()
        self.assertEqual(len(adb_shell.call_args.args), 3)
        self.assertEqual(adb_shell.call_args.args[:2], ("adb", "emulator-5554"))
        remote_command = adb_shell.call_args.args[2]
        self.assertTrue(remote_command.startswith("sh -c "))
        self.assertIn("/sdcard/ferrex-a11y.xml", remote_command)
        self.assertEqual(adb_shell.call_args.kwargs["timeout"], 120)

    def test_accessibility_one_stops_after_requirements_pass_and_records_steps(self) -> None:
        config = android_visual_qa.TargetConfig(
            target="phone",
            serial="emulator-5554",
            default_serial="emulator-5554",
            package="com.ferrex.android.debug",
            apk_path=Path("phone.apk"),
            expected_size=android_visual_qa.PHONE_EXPECTED_SIZE,
            screenshot_helper="phone-helper",
        )
        profile = android_visual_qa.VIEWPORT_PROFILES["phone-portrait"]
        viewport = android_visual_qa.WmOverrideSnapshot("Physical size: 1080x2400", "Physical density: 440", None, None)
        xml = '<hierarchy><node resource-id="phone.home" content-desc="Home" /></hierarchy>'

        with tempfile.TemporaryDirectory() as tmp:
            dump_mock = mock.Mock(
                return_value=android_visual_qa.AccessibilityXmlDump(xml, command_strategy="batched-shell")
            )
            reachability_mock = mock.Mock()
            with mock.patch.object(android_visual_qa, "require_serial_present"), mock.patch.object(
                android_visual_qa,
                "apply_viewport_profile",
                return_value=viewport,
            ), mock.patch.object(
                android_visual_qa,
                "collect_serial_metadata",
                return_value={"properties": {"sdk": "35"}},
            ), mock.patch.object(
                android_visual_qa,
                "collect_package_metadata",
                return_value={},
            ), mock.patch.object(
                android_visual_qa,
                "force_stop_package",
            ), mock.patch.object(
                android_visual_qa,
                "launch_scenario",
                return_value={"status": "started"},
            ), mock.patch.object(
                android_visual_qa,
                "drive_scenario",
                return_value=[],
            ), mock.patch.object(
                android_visual_qa,
                "dump_accessibility_xml_result",
                dump_mock,
            ), mock.patch.object(
                android_visual_qa,
                "drive_accessibility_reachability_step",
                reachability_mock,
            ), mock.patch.object(
                android_visual_qa,
                "restore_viewport_profile",
                return_value={"restored": True},
            ), mock.patch.object(
                android_visual_qa.time,
                "sleep",
                return_value=None,
            ):
                record = android_visual_qa.accessibility_one(
                    adb="adb",
                    config=config,
                    scenario=android_visual_qa.Scenario("phone-home", "phone"),
                    profile=profile,
                    output_dir=Path(tmp),
                    settle_ms=1,
                    log_lines=1,
                    max_steps=6,
                    exhaustive_dumps=False,
                )

            self.assertEqual(record["status"], "passed")
            self.assertEqual(record["early_stop_reason"], "all_requirements_passed")
            self.assertEqual(record["first_all_requirements_passed_step"], 0)
            self.assertEqual(len(record["dump_paths"]), 1)
            self.assertEqual(record["dump_attempts"], [1])
            self.assertTrue(Path(record["dump_paths"][0]).exists())
            self.assertEqual(record["node_count"], 1)
            step = record["accessibility_steps"][0]
            self.assertEqual(step["requirement_status"], {"root-tag": "passed"})
            self.assertEqual(step["dump_path"], record["dump_paths"][0])
            self.assertEqual(step["dump_attempts"], 1)
            self.assertEqual(step["node_count"], 1)
            self.assertIn("dump", step["timings_ms"])
            self.assertIn("verify", step["timings_ms"])
            self.assertEqual(record["dump_command_strategies_used"], ["batched-shell"])
            dump_mock.assert_called_once()
            reachability_mock.assert_not_called()

    def test_accessibility_one_failed_checks_continue_until_no_progress_and_logcat(self) -> None:
        config = android_visual_qa.TargetConfig(
            target="phone",
            serial="emulator-5554",
            default_serial="emulator-5554",
            package="com.ferrex.android.debug",
            apk_path=Path("phone.apk"),
            expected_size=android_visual_qa.PHONE_EXPECTED_SIZE,
            screenshot_helper="phone-helper",
        )
        profile = android_visual_qa.VIEWPORT_PROFILES["phone-portrait"]
        viewport = android_visual_qa.WmOverrideSnapshot("Physical size: 1080x2400", "Physical density: 440", None, None)
        xml = '<hierarchy><node resource-id="phone.search" content-desc="Search" /></hierarchy>'

        with tempfile.TemporaryDirectory() as tmp:
            dump_mock = mock.Mock(
                return_value=android_visual_qa.AccessibilityXmlDump(xml, command_strategy="safe-sequence")
            )
            reachability_mock = mock.Mock()
            logcat_mock = mock.Mock(return_value={"path": "logcat.txt", "max_lines": 1, "status": "captured"})
            with mock.patch.object(android_visual_qa, "require_serial_present"), mock.patch.object(
                android_visual_qa,
                "apply_viewport_profile",
                return_value=viewport,
            ), mock.patch.object(
                android_visual_qa,
                "collect_serial_metadata",
                return_value={"properties": {"sdk": "35"}},
            ), mock.patch.object(
                android_visual_qa,
                "collect_package_metadata",
                return_value={},
            ), mock.patch.object(
                android_visual_qa,
                "force_stop_package",
            ), mock.patch.object(
                android_visual_qa,
                "launch_scenario",
                return_value={"status": "started"},
            ), mock.patch.object(
                android_visual_qa,
                "drive_scenario",
                return_value=[],
            ), mock.patch.object(
                android_visual_qa,
                "dump_accessibility_xml_result",
                dump_mock,
            ), mock.patch.object(
                android_visual_qa,
                "drive_accessibility_reachability_step",
                reachability_mock,
            ), mock.patch.object(
                android_visual_qa,
                "capture_failure_logcat",
                logcat_mock,
            ), mock.patch.object(
                android_visual_qa,
                "restore_viewport_profile",
                return_value={"restored": True},
            ), mock.patch.object(
                android_visual_qa.time,
                "sleep",
                return_value=None,
            ):
                record = android_visual_qa.accessibility_one(
                    adb="adb",
                    config=config,
                    scenario=android_visual_qa.Scenario("phone-search", "phone"),
                    profile=profile,
                    output_dir=Path(tmp),
                    settle_ms=1,
                    log_lines=1,
                    max_steps=6,
                    exhaustive_dumps=False,
                )

            self.assertEqual(record["status"], "failed")
            self.assertEqual(record["early_stop_reason"], "no_progress")
            self.assertEqual(record["stop_reason"], "no_progress")
            self.assertEqual(len(record["dump_paths"]), 2)
            self.assertEqual(record["dump_attempts"], [1, 1])
            self.assertTrue(all(Path(path).exists() for path in record["dump_paths"]))
            self.assertIn("search-field", record["accessibility_steps"][1]["failed_requirement_keys"])
            self.assertTrue(record["accessibility_steps"][1]["no_new_nodes"])
            self.assertEqual(record["accessibility_steps"][1]["no_progress_streak"], 1)
            self.assertEqual(record["failure_logcat"], {"path": "logcat.txt", "max_lines": 1, "status": "captured"})
            self.assertEqual(dump_mock.call_count, 2)
            reachability_mock.assert_called_once()
            logcat_mock.assert_called_once()

    def test_accessibility_exhaustive_dumps_ignore_pass_and_no_progress_until_max_steps(self) -> None:
        config = android_visual_qa.TargetConfig(
            target="phone",
            serial="emulator-5554",
            default_serial="emulator-5554",
            package="com.ferrex.android.debug",
            apk_path=Path("phone.apk"),
            expected_size=android_visual_qa.PHONE_EXPECTED_SIZE,
            screenshot_helper="phone-helper",
        )
        profile = android_visual_qa.VIEWPORT_PROFILES["phone-portrait"]
        viewport = android_visual_qa.WmOverrideSnapshot("Physical size: 1080x2400", "Physical density: 440", None, None)
        xml = '<hierarchy><node resource-id="phone.home" content-desc="Home" /></hierarchy>'

        with tempfile.TemporaryDirectory() as tmp:
            dump_mock = mock.Mock(
                return_value=android_visual_qa.AccessibilityXmlDump(xml, command_strategy="safe-sequence")
            )
            reachability_mock = mock.Mock()
            with mock.patch.object(android_visual_qa, "require_serial_present"), mock.patch.object(
                android_visual_qa,
                "apply_viewport_profile",
                return_value=viewport,
            ), mock.patch.object(
                android_visual_qa,
                "collect_serial_metadata",
                return_value={"properties": {"sdk": "35"}},
            ), mock.patch.object(
                android_visual_qa,
                "collect_package_metadata",
                return_value={},
            ), mock.patch.object(
                android_visual_qa,
                "force_stop_package",
            ), mock.patch.object(
                android_visual_qa,
                "launch_scenario",
                return_value={"status": "started"},
            ), mock.patch.object(
                android_visual_qa,
                "drive_scenario",
                return_value=[],
            ), mock.patch.object(
                android_visual_qa,
                "dump_accessibility_xml_result",
                dump_mock,
            ), mock.patch.object(
                android_visual_qa,
                "drive_accessibility_reachability_step",
                reachability_mock,
            ), mock.patch.object(
                android_visual_qa,
                "restore_viewport_profile",
                return_value={"restored": True},
            ), mock.patch.object(
                android_visual_qa.time,
                "sleep",
                return_value=None,
            ):
                record = android_visual_qa.accessibility_one(
                    adb="adb",
                    config=config,
                    scenario=android_visual_qa.Scenario("phone-home", "phone"),
                    profile=profile,
                    output_dir=Path(tmp),
                    settle_ms=1,
                    log_lines=1,
                    max_steps=3,
                    exhaustive_dumps=True,
                )

            self.assertEqual(record["status"], "passed")
            self.assertEqual(record["stop_reason"], "max_steps")
            self.assertNotIn("early_stop_reason", record)
            self.assertEqual(record["first_all_requirements_passed_step"], 0)
            self.assertEqual(len(record["dump_paths"]), 3)
            self.assertEqual(record["dump_attempts"], [1, 1, 1])
            self.assertEqual(dump_mock.call_count, 3)
            self.assertEqual(reachability_mock.call_count, 2)

    def test_tv_recovery_accessibility_requirements_use_stable_focus_action_tags(self) -> None:
        scenario = android_visual_qa.Scenario("tv-theater-plate-recovery", "tv")
        requirements = android_visual_qa.accessibility_requirements_for_scenario(scenario)
        recovery = {requirement.key: requirement for requirement in requirements if requirement.kind == "recovery-action"}

        self.assertEqual(
            {
                "theater-recovery-retry",
                "theater-recovery-change-server",
                "theater-recovery-clear-cache",
                "theater-recovery-reset-connection",
                "theater-recovery-diagnostics",
            },
            set(recovery),
        )
        for action_key, action_label in android_visual_qa.THEATER_PLATE_RECOVERY_ACTIONS:
            requirement = recovery[f"theater-recovery-{action_key}"]
            self.assertEqual(requirement.tag, f"tv.theater-plate.action.recovery.{action_key}")
            self.assertEqual(requirement.content_description, action_label)
            self.assertTrue(requirement.require_clickable)
            self.assertTrue(requirement.require_focusable)
            self.assertTrue(requirement.require_button_role)
            self.assertTrue(requirement.require_enabled)

    def test_playback_accessibility_requirements_verify_disabled_semantics(self) -> None:
        scenario = android_visual_qa.Scenario("tv-theater-plate-playback-entry", "tv")
        requirements = android_visual_qa.accessibility_requirements_for_scenario(scenario)
        xml = """
        <hierarchy>
          <node resource-id="tv.theater-plate.playback-entry" content-desc="Playback entry root" enabled="true" />
          <node resource-id="tv.theater-plate.status.playback-entry" content-desc="Playback entry status: Playback entry uses a prepared route while preserving no-wipe exits." enabled="true" />
          <node resource-id="tv.theater-plate.action.playback-entry.primary" class="android.widget.Button" content-desc="Resume playback" clickable="true" focusable="true" enabled="true" />
          <node resource-id="tv.theater-plate.media.playback-entry.hero" class="android.widget.Button" content-desc="Theater Plate media Aurora Station: Resume at 30:42" clickable="true" focusable="true" enabled="true" />
          <node resource-id="tv.theater-plate.rail.playback-entry.primary" content-desc="Playback entry rail" enabled="true" />
          <node resource-id="tv.theater-plate.action.playback-entry.network-required" class="android.widget.Button" content-desc="Network playback requires a playback ticket" clickable="false" focusable="false" enabled="false" />
        </hierarchy>
        """
        nodes = android_visual_qa.parse_accessibility_nodes(xml)

        checks = android_visual_qa.verify_accessibility_requirements(nodes, requirements)

        self.assertTrue(all(check["status"] == "passed" for check in checks), checks)
        enabled_disabled_node = [
            {**node, "enabled": "true"}
            if node.get("resource-id") == "tv.theater-plate.action.playback-entry.network-required"
            else node
            for node in nodes
        ]
        failed_checks = android_visual_qa.verify_accessibility_requirements(enabled_disabled_node, requirements)
        self.assertTrue(
            any(
                check["requirement"]["key"] == "theater-playback-disabled-network" and check["status"] == "failed"
                for check in failed_checks
            )
        )

    def test_accessibility_dump_retries_missing_remote_file(self) -> None:
        cat_paths: list[str] = []

        def fake_adb_shell(adb: str, serial: str, *shell_args: str, **kwargs: object) -> android_visual_qa.RunResult:
            if shell_args[:2] == ("uiautomator", "dump"):
                return android_visual_qa.RunResult(stdout="", stderr="", returncode=0)
            if shell_args and shell_args[0] == "cat":
                cat_paths.append(shell_args[1])
                if len(cat_paths) == 1:
                    raise android_visual_qa.CommandError([adb, "-s", serial, "shell", *shell_args], 1, "", "No such file")
                return android_visual_qa.RunResult(stdout="<hierarchy />", stderr="", returncode=0)
            return android_visual_qa.RunResult(stdout="", stderr="", returncode=0)

        with mock.patch.object(android_visual_qa, "adb_shell", side_effect=fake_adb_shell):
            xml = android_visual_qa.dump_accessibility_xml("adb", SimpleNamespace(serial="emulator-5556"))

        self.assertEqual(xml, "<hierarchy />")
        self.assertEqual(len(cat_paths), 2)
        self.assertEqual(len(set(cat_paths)), 2)

    def test_tv_accessibility_reachability_uses_dpad_not_touch_swipe(self) -> None:
        calls: list[tuple[str, ...]] = []

        def fake_adb_shell(adb: str, serial: str, *shell_args: str, **kwargs: object) -> android_visual_qa.RunResult:
            calls.append(shell_args)
            return android_visual_qa.RunResult(stdout="", stderr="", returncode=0)

        profile = android_visual_qa.VIEWPORT_PROFILES["tv-1080p"]
        with mock.patch.object(android_visual_qa, "adb_shell", side_effect=fake_adb_shell):
            android_visual_qa.drive_accessibility_reachability_step(
                "adb",
                SimpleNamespace(serial="emulator-5556", target="tv"),
                profile,
            )

        self.assertTrue(calls)
        self.assertTrue(all(call[:2] == ("input", "keyevent") for call in calls), calls)


    def test_gate_smoke_selection_includes_phone_and_tv(self) -> None:
        registry = android_visual_qa.ScenarioRegistry.load(self.repo_root())

        selected = android_visual_qa.scenarios_for_gate_mode(registry, "smoke")

        self.assertEqual([scenario.id for scenario in selected], ["phone-home", "tv-home-focus"])
        self.assertEqual({scenario.target for scenario in selected}, {"phone", "tv"})

    def test_gate_complete_selection_matches_required_registry(self) -> None:
        registry = android_visual_qa.ScenarioRegistry.load(self.repo_root())

        selected = android_visual_qa.scenarios_for_gate_mode(registry, "complete")

        self.assertEqual(
            [scenario.id for scenario in selected],
            [scenario.id for scenario in registry.scenarios],
        )

    def test_default_complete_capture_plan_emits_78_captures(self) -> None:
        registry = android_visual_qa.ScenarioRegistry.load(self.repo_root())
        selected = android_visual_qa.scenarios_for_gate_mode(registry, "complete")
        args = SimpleNamespace(
            target="all",
            hardware=False,
            hardware_serial=None,
            expected_size=None,
            profile=None,
        )
        configs = android_visual_qa.target_configs(self.repo_root(), args)
        profiles = android_visual_qa.selected_viewport_profiles(args, configs, selected)
        plan = android_visual_qa.capture_plan_json("complete", selected, profiles)

        self.assertEqual(plan["scenario_count"], 39)
        self.assertEqual(plan["capture_count"], 78)

    def test_verify_manifest_accepts_smoke_phone_and_tv_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            captures: list[dict[str, object]] = []
            for profile, width, height in (
                ("phone-portrait", 1080, 2400),
                ("phone-landscape-foldable", 1800, 1200),
            ):
                png = root / profile / "phone-home.png"
                self.write_png(png, width, height)
                captures.append(
                    {
                        "status": "passed",
                        "target": "phone",
                        "profile": profile,
                        "scenario_id": "phone-home",
                        "screenshot_path": str(png),
                        "expected_dimensions": {"width": width, "height": height},
                        "dimensions": {"width": width, "height": height},
                    }
                )
            for profile in ("tv-1080p", "tv-4k-scaled"):
                png = root / profile / "tv-home-focus.png"
                self.write_png(png, 1920, 1080)
                captures.append(
                    {
                        "status": "passed",
                        "target": "tv",
                        "profile": profile,
                        "scenario_id": "tv-home-focus",
                        "screenshot_path": str(png),
                        "expected_dimensions": {"width": 1920, "height": 1080},
                        "dimensions": {"width": 1920, "height": 1080},
                    }
                )
            manifest = root / "manifest.json"
            self.write_manifest(manifest, captures)

            summary = android_visual_qa.verify_manifest(
                manifest,
                mode="smoke",
                repo_root=self.repo_root(),
            )

            self.assertEqual(summary.capture_count, 4)
            self.assertEqual(summary.target_counts["phone"], 2)
            self.assertEqual(summary.target_counts["tv"], 2)

    def test_verify_manifest_rejects_incomplete_smoke_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            phone = root / "phone-portrait" / "phone-home.png"
            self.write_png(phone, 1080, 2400)
            manifest = root / "manifest.json"
            self.write_manifest(
                manifest,
                [
                    {
                        "status": "passed",
                        "target": "phone",
                        "profile": "phone-portrait",
                        "scenario_id": "phone-home",
                        "screenshot_path": str(phone),
                        "expected_dimensions": {"width": 1080, "height": 2400},
                        "dimensions": {"width": 1080, "height": 2400},
                    }
                ],
            )

            with self.assertRaises(android_visual_qa.VisualQaError):
                android_visual_qa.verify_manifest(manifest, mode="smoke", repo_root=self.repo_root())

    def test_smoke_capture_plan_records_helper_comparison_unavailable(self) -> None:
        registry = android_visual_qa.ScenarioRegistry(
            [android_visual_qa.Scenario("phone-home", "phone")],
            self.repo_root() / "mobile/android/app/src/main/kotlin/com/ferrex/android/ui/qa/FerrexVisualQa.kt",
        )
        selected = [registry.by_id["phone-home"]]
        args = SimpleNamespace(
            target="all",
            scenario="all",
            settle_ms=1,
            log_lines=1,
            adb="adb",
            no_nix_screenshot=False,
            screenshot_mode=android_visual_qa.SCREENSHOT_MODE_FAST,
            hardware=False,
            hardware_serial=None,
            expected_size=None,
            profile=["phone-portrait"],
            effective_argv=["gate", "--mode", "smoke"],
        )

        with tempfile.TemporaryDirectory() as tmp:
            output_dir = Path(tmp)

            def fake_capture_one(**kwargs: object) -> dict[str, object]:
                screenshot_path = output_dir / "phone-portrait" / "phone-home.png"
                self.write_png(screenshot_path, 1080, 2400)
                return {
                    "status": "passed",
                    "target": "phone",
                    "profile": "phone-portrait",
                    "scenario_id": "phone-home",
                    "serial": "emulator-5554",
                    "screenshot_path": str(screenshot_path),
                    "expected_dimensions": {"width": 1080, "height": 2400},
                    "dimensions": {"width": 1080, "height": 2400},
                    "screenshot_capture": {
                        "method": "adb-exec-out-screencap",
                        "requested_mode": "fast",
                        "serial": "emulator-5554",
                        "output_path": str(screenshot_path),
                        "command_category": "exec-out:screencap",
                        "duration_ms": 3,
                        "helper_compatibility_mode": False,
                        "helper_used": False,
                    },
                    "timings_ms": {"screenshot": 3, "total": 5},
                    "adb_command_summary": {
                        "total_count": 1,
                        "total_duration_ms": 3,
                        "categories": {"exec-out:screencap": {"count": 1, "duration_ms": 3}},
                    },
                }

            with mock.patch.object(android_visual_qa, "resolve_executable", return_value="adb"), mock.patch.object(
                android_visual_qa,
                "collect_command_versions",
                return_value={},
            ), mock.patch.object(
                android_visual_qa,
                "capture_one",
                side_effect=fake_capture_one,
            ), mock.patch.object(android_visual_qa.shutil, "which", return_value=None):
                status = android_visual_qa.run_capture_plan(
                    args=args,
                    repo_root=self.repo_root(),
                    registry=registry,
                    selected=selected,
                    output_dir=output_dir,
                    command_name="android-visual-qa gate",
                    mode="smoke",
                )
            manifest = json.loads((output_dir / "manifest.json").read_text(encoding="utf-8"))

        self.assertEqual(status, 0)
        self.assertEqual(manifest["screenshot"]["mode"], "fast")
        self.assertEqual(manifest["captures"][0]["screenshot_capture"]["command_category"], "exec-out:screencap")
        comparison = manifest["screenshot_method_comparison"]
        self.assertEqual(comparison["status"], "unavailable")
        self.assertIn("not available", comparison["unavailable_reason"])
        self.assertEqual(comparison["fast_capture"]["dimensions"], {"width": 1080, "height": 2400})

    def test_verify_manifest_accepts_explicit_profile_deferrals(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            phone = root / "phone-portrait" / "phone-home.png"
            tv = root / "tv-1080p" / "tv-home-focus.png"
            self.write_png(phone, 1080, 2400)
            self.write_png(tv, 1920, 1080)
            manifest = root / "manifest.json"
            self.write_manifest(
                manifest,
                [
                    {
                        "status": "passed",
                        "target": "phone",
                        "profile": "phone-portrait",
                        "scenario_id": "phone-home",
                        "screenshot_path": str(phone),
                        "expected_dimensions": {"width": 1080, "height": 2400},
                        "dimensions": {"width": 1080, "height": 2400},
                    },
                    {
                        "status": "passed",
                        "target": "tv",
                        "profile": "tv-1080p",
                        "scenario_id": "tv-home-focus",
                        "screenshot_path": str(tv),
                        "expected_dimensions": {"width": 1920, "height": 1080},
                        "dimensions": {"width": 1920, "height": 1080},
                    },
                ],
                profile_deferrals=[
                    {
                        "target": "phone",
                        "profile": "phone-landscape-foldable",
                        "human_deferred": True,
                        "reason": "No foldable/landscape phone emulator attached in this workspace.",
                    },
                    {
                        "target": "tv",
                        "profile": "tv-4k-scaled",
                        "status": "human-deferred",
                        "reason": "4K-scaled framebuffer confirmation requires a local TV emulator run.",
                    },
                ],
            )

            summary = android_visual_qa.verify_manifest(manifest, mode="smoke", repo_root=self.repo_root())

            self.assertEqual(summary.capture_count, 2)

    def test_verify_manifest_rejects_invalid_capture_profile_names(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            phone = root / "tv-1080p" / "phone-home.png"
            self.write_png(phone, 1080, 2400)
            manifest = root / "manifest.json"
            self.write_manifest(
                manifest,
                [
                    {
                        "status": "passed",
                        "target": "phone",
                        "profile": "tv-1080p",
                        "scenario_id": "phone-home",
                        "screenshot_path": str(phone),
                        "expected_dimensions": {"width": 1080, "height": 2400},
                        "dimensions": {"width": 1080, "height": 2400},
                    }
                ],
            )

            with self.assertRaises(android_visual_qa.VisualQaError):
                android_visual_qa.verify_manifest(manifest, repo_root=self.repo_root())

    def test_parser_accepts_target_workers_on_gate_capture_and_accessibility(self) -> None:
        parser = android_visual_qa.build_parser()

        gate = parser.parse_args(["gate", "--warm", "--target-workers", "2"])
        capture = parser.parse_args(["capture", "--target", "all", "--scenario", "all", "--target-workers", "2"])
        accessibility = parser.parse_args(
            ["accessibility", "--target", "all", "--scenario", "all", "--target-workers", "2"]
        )

        self.assertTrue(gate.warm)
        self.assertEqual(gate.target_workers, 2)
        self.assertEqual(capture.target_workers, 2)
        self.assertEqual(accessibility.target_workers, 2)

    def test_warm_gate_skips_setup_before_capture(self) -> None:
        calls: list[str] = []

        def fake_gate_command(command: tuple[object, ...]) -> None:
            command_parts = [os.fspath(part) for part in command]
            calls.append(f"primitive:{Path(command_parts[0]).name}:{':'.join(command_parts[1:])}")

        def fake_capture_plan(**kwargs: object) -> int:
            capture_args = kwargs["args"]
            calls.append(f"capture:{kwargs['mode']}:warm={capture_args.warm}:workers={capture_args.target_workers}")
            output_dir = Path(kwargs["output_dir"])
            output_dir.mkdir(parents=True, exist_ok=True)
            (output_dir / "manifest.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "status": "passed",
                        "output_dir": str(output_dir),
                        "captures": [],
                        "failures": [],
                    }
                ),
                encoding="utf-8",
            )
            return 0

        with tempfile.TemporaryDirectory() as tmp:
            summary = android_visual_qa.ManifestSummary(
                manifest_path=Path(tmp) / "manifest.json",
                output_dir=Path(tmp),
                mode="smoke",
                captures=(),
            )
            args = SimpleNamespace(
                mode="smoke",
                output_dir=tmp,
                settle_ms=1,
                log_lines=1,
                adb="adb",
                no_nix_screenshot=True,
                warm=True,
                target_workers=2,
                profile=None,
                effective_argv=["gate", "--mode", "smoke", "--warm", "--target-workers", "2"],
            )
            with mock.patch.object(
                android_visual_qa,
                "run_gate_command",
                side_effect=fake_gate_command,
            ), mock.patch.object(
                android_visual_qa,
                "run_capture_plan",
                side_effect=fake_capture_plan,
            ), mock.patch.object(
                android_visual_qa,
                "verify_manifest",
                return_value=summary,
            ), mock.patch.object(
                android_visual_qa,
                "print_artifact_summary",
            ):
                status = android_visual_qa.run_gate(args)

        self.assertEqual(status, 0)
        self.assertEqual(
            calls,
            [
                "capture:smoke:warm=True:workers=2",
                "primitive:android-emulator-qa.sh:check:all",
            ],
        )

    def test_gate_runs_primitives_capture_and_verify_in_order(self) -> None:
        calls: list[str] = []

        def fake_gate_command(command: tuple[object, ...]) -> None:
            command_parts = [os.fspath(part) for part in command]
            calls.append(f"primitive:{Path(command_parts[0]).name}:{':'.join(command_parts[1:])}")

        def fake_capture_plan(**kwargs: object) -> int:
            calls.append(f"capture:{kwargs['mode']}")
            output_dir = Path(kwargs["output_dir"])
            output_dir.mkdir(parents=True, exist_ok=True)
            (output_dir / "manifest.json").write_text(
                json.dumps(
                    {
                        "schema_version": 1,
                        "status": "passed",
                        "output_dir": str(output_dir),
                        "captures": [],
                        "failures": [],
                    }
                ),
                encoding="utf-8",
            )
            return 0

        with tempfile.TemporaryDirectory() as tmp:
            summary = android_visual_qa.ManifestSummary(
                manifest_path=Path(tmp) / "manifest.json",
                output_dir=Path(tmp),
                mode="smoke",
                captures=(),
            )
            args = SimpleNamespace(
                mode="smoke",
                output_dir=tmp,
                settle_ms=1,
                log_lines=1,
                adb="adb",
                no_nix_screenshot=True,
                effective_argv=["gate", "--mode", "smoke"],
            )
            with mock.patch.object(
                android_visual_qa,
                "run_gate_command",
                side_effect=fake_gate_command,
            ), mock.patch.object(
                android_visual_qa,
                "run_capture_plan",
                side_effect=fake_capture_plan,
            ), mock.patch.object(
                android_visual_qa,
                "verify_manifest",
                return_value=summary,
            ), mock.patch.object(
                android_visual_qa,
                "print_artifact_summary",
            ):
                status = android_visual_qa.run_gate(args)
            manifest = json.loads((Path(tmp) / "manifest.json").read_text(encoding="utf-8"))
            gate_primitives = manifest["timing_summary"]["gate_primitives"]

        self.assertEqual(status, 0)
        self.assertEqual(
            calls,
            [
                "primitive:android-emulator-qa.sh:build",
                "primitive:android-emulator-qa.sh:start",
                "primitive:android-emulator-qa.sh:doctor",
                "primitive:android-emulator-qa.sh:install:all",
                "capture:smoke",
                "primitive:android-emulator-qa.sh:check:all",
            ],
        )
        self.assertEqual(
            [primitive["name"] for primitive in gate_primitives],
            ["build", "start", "doctor", "install", "capture", "check", "verify"],
        )
        self.assertTrue(all(primitive["status"] == "passed" for primitive in gate_primitives))


if __name__ == "__main__":
    unittest.main()
