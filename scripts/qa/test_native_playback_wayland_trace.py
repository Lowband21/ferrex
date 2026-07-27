#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
import sys
import unittest
from pathlib import Path


def load_module(name: str, filename: str):
    path = Path(__file__).with_name(filename)
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


wayland_trace = load_module(
    "native_playback_wayland_trace", "native_playback_wayland_trace.py"
)


class NativePlaybackWaylandTraceTest(unittest.TestCase):
    def test_protocol_inventory_separates_advertised_bound_and_used(self) -> None:
        trace = """\
[  1.000] {Default Queue} wl_registry#2.global(3, "wl_compositor", 6)
[  1.001] {Default Queue} wl_registry#2.global(7, "wp_viewporter", 1)
[  1.002] {Default Queue}  -> wl_display#1.get_registry(new id wl_registry#2)
[  1.003] {Default Queue}  -> wl_registry#2.bind(3, "wl_compositor", 6, new id [unknown]#4)
[  1.004] {Default Queue}  -> wl_compositor#4.create_surface(new id wl_surface#5)
[  1.005] {Default Queue}  -> xdg_surface#9.get_toplevel(new id xdg_toplevel#10)
[  1.006] {Default Queue} wl_surface#5.enter(wl_output#8)
[  1.007] {Default Queue}  -> wl_surface#5.commit()
"""
        inventory = wayland_trace.parse_protocol_inventory(trace).as_dict()

        self.assertEqual(
            {"wl_compositor": 6, "wp_viewporter": 1},
            inventory["advertised_globals"],
        )
        self.assertEqual({"wl_compositor": 6}, inventory["bound_globals"])
        self.assertEqual(
            [
                "wl_compositor",
                "wl_display",
                "wl_registry",
                "wl_surface",
                "xdg_surface",
            ],
            inventory["interfaces"],
        )
        self.assertEqual(["create_surface"], inventory["requests"]["wl_compositor"])
        self.assertEqual(["commit"], inventory["requests"]["wl_surface"])
        self.assertEqual(["enter"], inventory["events"]["wl_surface"])
        self.assertEqual(8, inventory["message_count"])
        self.assertEqual(
            {"registry_request_count": 1, "xdg_toplevel_candidate_count": 1},
            inventory["topology"],
        )

    def test_protocol_inventory_accepts_pre_queue_at_object_format(self) -> None:
        inventory = wayland_trace.parse_protocol_inventory(
            '[ 2.000]  -> wl_display@1.get_registry(new id wl_registry@2)\n'
        )
        self.assertEqual(1, inventory.registry_request_count)
        self.assertEqual({"wl_display": {"get_registry"}}, inventory.requests)

    def test_capture_inventory_requires_frame_fullscreen_and_teardown(self) -> None:
        inventory = wayland_trace.ProtocolInventory(
            requests={
                "wl_surface": {"attach", "commit", "destroy"},
                "wp_presentation": {"feedback"},
                "xdg_toplevel": {"set_fullscreen", "unset_fullscreen", "destroy"},
            },
            events={"wp_presentation_feedback": {"presented"}},
            xdg_toplevel_candidate_count=1,
        )
        wayland_trace.validate_capture_inventory(inventory)

        inventory.events.clear()
        with self.assertRaises(wayland_trace.TraceError):
            wayland_trace.validate_capture_inventory(inventory)

    def test_trace_sanitization_removes_paths_titles_and_output_identity(self) -> None:
        trace = """\
[ 1.0] {Default Queue}  -> xdg_toplevel#7.set_title("private title")
[ 1.1] {Default Queue} wl_output#8.name("DP-3")
[ 1.2] {Default Queue} zxdg_output_v1#9.description("serial 123")
[ 1.3] {Default Queue} wl_output#8.geometry(0, 0, 600, 340, 0, "Private Vendor", "Private Model", 0)
[ 1.4] {Default Queue} wl_seat#4.name("private-seat")
/home/example/media/video.mkv
"""
        sanitized = wayland_trace.sanitize_trace(
            trace, ["/home/example/media/video.mkv", "/home/example"]
        )

        for secret in (
            "private title",
            "DP-3",
            "serial 123",
            "Private Vendor",
            "Private Model",
            "private-seat",
            "/home/example",
        ):
            self.assertNotIn(secret, sanitized)
        self.assertIn('set_title("<redacted-title>")', sanitized)
        self.assertIn('name("<redacted-output>")', sanitized)
        self.assertIn('geometry(0, 0, 600, 340, 0, "<redacted-output>"', sanitized)
        self.assertIn('name("<redacted-seat>")', sanitized)
        self.assertIn("<redacted-path>", sanitized)

    def test_environment_ids_are_single_safe_path_segments(self) -> None:
        for value in ("wl-wlroots-amd", "wl.nvidia_1", "run-20260712t120000z"):
            with self.subTest(value=value):
                self.assertEqual(value, wayland_trace.validate_environment_id(value))

        for value in ("", ".", "..", "../escape", "host/name", "UPPERCASE", "a" * 65):
            with self.subTest(value=value):
                with self.assertRaises(wayland_trace.TraceError):
                    wayland_trace.validate_environment_id(value)

    def test_run_ids_allow_utc_form_but_not_paths(self) -> None:
        run_id = "20260713T003507.399434Z-89e48c"
        self.assertEqual(run_id, wayland_trace.validate_run_id(run_id))
        for value in ("", ".", "..", "../escape", "run/name", "a" * 97):
            with self.subTest(value=value):
                with self.assertRaises(wayland_trace.TraceError):
                    wayland_trace.validate_run_id(value)

    def test_mpv_version_parser_requires_release_triplet(self) -> None:
        self.assertEqual(
            "0.41.0",
            wayland_trace.parse_mpv_version(
                "mpv v0.41.0 Copyright mpv project\nlibplacebo version: v7.360.1\n"
            ),
        )
        with self.assertRaises(wayland_trace.TraceError):
            wayland_trace.parse_mpv_version("mpv development build\n")


if __name__ == "__main__":
    unittest.main()
