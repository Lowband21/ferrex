#!/usr/bin/env python3
from __future__ import annotations

import contextlib
import importlib.util
import io
import os
import stat
import struct
import sys
import tempfile
import threading
import unittest
import urllib.error
import urllib.request
from pathlib import Path


def load_module(name: str, filename: str):
    path = Path(__file__).with_name(filename)
    spec = importlib.util.spec_from_file_location(name, path)
    assert spec is not None and spec.loader is not None
    module = importlib.util.module_from_spec(spec)
    sys.modules[name] = module
    spec.loader.exec_module(module)
    return module


fixture_server = load_module(
    "native_playback_fixture_server", "native_playback_fixture_server.py"
)
fixture_generator = load_module(
    "native_playback_fixtures", "native_playback_fixtures.py"
)


class NativePlaybackFixtureTest(unittest.TestCase):
    def test_byte_ranges_cover_media_client_forms(self) -> None:
        self.assertEqual(
            fixture_server.ByteRange(10, 29),
            fixture_server.parse_byte_range("bytes=10-29", 100),
        )
        self.assertEqual(
            fixture_server.ByteRange(90, 99),
            fixture_server.parse_byte_range("bytes=-10", 100),
        )
        self.assertEqual(
            fixture_server.ByteRange(95, 99),
            fixture_server.parse_byte_range("bytes=95-", 100),
        )
        self.assertEqual(
            fixture_server.ByteRange(95, 99),
            fixture_server.parse_byte_range("bytes=95-200", 100),
        )

    def test_byte_ranges_reject_ambiguous_or_unsatisfiable_input(self) -> None:
        for value in (
            "bytes=",
            "bytes=100-101",
            "bytes=20-10",
            "bytes=0-1,4-5",
            "items=0-1",
            "bytes=-0",
        ):
            with self.subTest(value=value):
                with self.assertRaises(ValueError):
                    fixture_server.parse_byte_range(value, 100)

    def test_port_file_is_private_and_replaced_atomically(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "ready/port"
            fixture_server.write_port_file(path, 43210)

            self.assertEqual("43210\n", path.read_text(encoding="utf-8"))
            if os.name != "nt":
                self.assertEqual(0o600, stat.S_IMODE(path.stat().st_mode))
            self.assertEqual([], list(path.parent.glob(".*.tmp-*")))

    def test_fixture_server_enforces_auth_and_exact_ranges(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            payload = bytes(range(256))
            (root / "fixture.bin").write_bytes(payload)
            server = fixture_server.FixtureServer(
                ("127.0.0.1", 0), root, "test-secret", "bearer"
            )
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            log = io.StringIO()
            with contextlib.redirect_stderr(log):
                thread.start()
                base = f"http://127.0.0.1:{server.server_port}/fixture.bin"
                try:
                    with self.assertRaises(urllib.error.HTTPError) as error:
                        urllib.request.urlopen(base, timeout=2)
                    self.assertEqual(401, error.exception.code)
                    error.exception.close()

                    request = urllib.request.Request(
                        base,
                        headers={
                            "Authorization": "Bearer test-secret",
                            "Range": "bytes=10-29",
                        },
                    )
                    with urllib.request.urlopen(
                        request, timeout=2
                    ) as response:
                        self.assertEqual(206, response.status)
                        self.assertEqual("bytes 10-29/256", response.headers["Content-Range"])
                        self.assertEqual(payload[10:30], response.read())
                finally:
                    server.shutdown()
                    server.server_close()
                    thread.join(timeout=2)

            self.assertNotIn("test-secret", log.getvalue())

    def test_query_ticket_is_not_retained_in_server_log(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "fixture.bin").write_bytes(b"fixture")
            server = fixture_server.FixtureServer(
                ("127.0.0.1", 0), root, "query-secret", "query"
            )
            thread = threading.Thread(target=server.serve_forever, daemon=True)
            log = io.StringIO()
            with contextlib.redirect_stderr(log):
                thread.start()
                url = (
                    f"http://127.0.0.1:{server.server_port}/fixture.bin"
                    "?access_token=query-secret"
                )
                try:
                    with urllib.request.urlopen(url, timeout=2) as response:
                        self.assertEqual(b"fixture", response.read())
                finally:
                    server.shutdown()
                    server.server_close()
                    thread.join(timeout=2)

            retained_log = log.getvalue()
            self.assertNotIn("query-secret", retained_log)
            self.assertNotIn("access_token", retained_log)

    def test_generated_pgs_has_owned_epoch_and_clear_sequences(self) -> None:
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "bitmap.sup"
            fixture_generator.write_pgs(path)
            data = path.read_bytes()

        segments: list[tuple[int, int, bytes]] = []
        offset = 0
        while offset < len(data):
            self.assertEqual(b"PG", data[offset : offset + 2])
            pts, _dts, kind, length = struct.unpack(
                ">IIBH", data[offset + 2 : offset + 13]
            )
            start = offset + 13
            end = start + length
            self.assertLessEqual(end, len(data))
            segments.append((pts, kind, data[start:end]))
            offset = end

        self.assertEqual(len(data), offset)
        self.assertEqual(
            [0x16, 0x17, 0x14, 0x15, 0x80, 0x16, 0x80],
            [kind for _pts, kind, _payload in segments],
        )
        self.assertEqual(90_000, segments[0][0])
        self.assertEqual(270_000, segments[-1][0])
        self.assertEqual(1, segments[0][2][10])  # one composition object
        self.assertEqual(0, segments[-2][2][-1])  # clear composition


if __name__ == "__main__":
    unittest.main()
