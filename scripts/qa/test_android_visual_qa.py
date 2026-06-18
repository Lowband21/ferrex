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

    def write_manifest(self, path: Path, captures: list[dict[str, object]]) -> None:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(
            json.dumps(
                {
                    "schema_version": 1,
                    "status": "passed",
                    "output_dir": str(path.parent),
                    "captures": captures,
                    "failures": [],
                }
            ),
            encoding="utf-8",
        )

    def test_registry_parses_required_phone_and_tv_scenarios(self) -> None:
        registry = android_visual_qa.ScenarioRegistry.load(self.repo_root())

        self.assertIn("phone-home", registry.by_id)
        self.assertIn("tv-home-focus", registry.by_id)
        self.assertEqual("phone", registry.by_id["phone-home"].target)
        self.assertEqual("tv", registry.by_id["tv-home-focus"].target)
        self.assertEqual(registry.scenarios[0].id, "phone-home")
        self.assertEqual(len(registry.scenarios), len({scenario.id for scenario in registry.scenarios}))

    def test_scenario_selection_rejects_cross_target_request(self) -> None:
        registry = android_visual_qa.ScenarioRegistry.load(self.repo_root())

        with self.assertRaises(android_visual_qa.VisualQaError):
            registry.select("phone", "tv-home-focus")

        phone_ids = [scenario.id for scenario in registry.select("phone", "all")]
        self.assertTrue(phone_ids)
        self.assertTrue(all(scenario_id.startswith("phone-") for scenario_id in phone_ids))

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

    def test_verify_manifest_accepts_smoke_phone_and_tv_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            phone = root / "phone" / "phone-home.png"
            tv = root / "tv" / "tv-home-focus.png"
            self.write_png(phone, 1080, 2400)
            self.write_png(tv, 1920, 1080)
            manifest = root / "manifest.json"
            self.write_manifest(
                manifest,
                [
                    {
                        "status": "passed",
                        "target": "phone",
                        "scenario_id": "phone-home",
                        "screenshot_path": str(phone),
                        "expected_dimensions": {"width": 1080, "height": 2400},
                        "dimensions": {"width": 1080, "height": 2400},
                    },
                    {
                        "status": "passed",
                        "target": "tv",
                        "scenario_id": "tv-home-focus",
                        "screenshot_path": str(tv),
                        "expected_dimensions": {"width": 1920, "height": 1080},
                        "dimensions": {"width": 1920, "height": 1080},
                    },
                ],
            )

            summary = android_visual_qa.verify_manifest(
                manifest,
                mode="smoke",
                repo_root=self.repo_root(),
            )

            self.assertEqual(summary.capture_count, 2)
            self.assertEqual(summary.target_counts["phone"], 1)
            self.assertEqual(summary.target_counts["tv"], 1)

    def test_verify_manifest_rejects_incomplete_smoke_artifacts(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            phone = root / "phone" / "phone-home.png"
            self.write_png(phone, 1080, 2400)
            manifest = root / "manifest.json"
            self.write_manifest(
                manifest,
                [
                    {
                        "status": "passed",
                        "target": "phone",
                        "scenario_id": "phone-home",
                        "screenshot_path": str(phone),
                        "expected_dimensions": {"width": 1080, "height": 2400},
                        "dimensions": {"width": 1080, "height": 2400},
                    }
                ],
            )

            with self.assertRaises(android_visual_qa.VisualQaError):
                android_visual_qa.verify_manifest(manifest, mode="smoke", repo_root=self.repo_root())

    def test_gate_runs_primitives_capture_and_verify_in_order(self) -> None:
        calls: list[str] = []

        def fake_gate_command(command: tuple[object, ...]) -> None:
            command_parts = [os.fspath(part) for part in command]
            calls.append(f"primitive:{Path(command_parts[0]).name}:{':'.join(command_parts[1:])}")

        def fake_capture_plan(**kwargs: object) -> int:
            calls.append(f"capture:{kwargs['mode']}")
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


if __name__ == "__main__":
    unittest.main()
