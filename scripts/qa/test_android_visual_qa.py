#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
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


if __name__ == "__main__":
    unittest.main()
