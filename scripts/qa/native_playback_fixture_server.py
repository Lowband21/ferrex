#!/usr/bin/env python3
"""Serve generated playback fixtures with redacted auth and HTTP ranges.

The server binds to loopback and reads its bearer/query token from an
environment variable so the credential does not appear in its argument vector.
It is a deterministic transport fixture, not a substitute for the Ferrex-server
acceptance gate.
"""

from __future__ import annotations

import argparse
import hmac
import http.server
import mimetypes
import os
import re
import signal
import sys
import threading
import urllib.parse
from dataclasses import dataclass
from pathlib import Path
from typing import BinaryIO, Sequence

MARKER_NAME = ".ferrex-native-playback-fixtures"
DEFAULT_ROOT = Path("target/native-playback-fixtures")
DEFAULT_TOKEN_ENV = "FERREX_FIXTURE_TOKEN"
_RANGE_PATTERN = re.compile(r"^bytes=(\d*)-(\d*)$")


class ServerError(RuntimeError):
    """Invalid server configuration."""


@dataclass(frozen=True)
class ByteRange:
    start: int
    end: int

    @property
    def length(self) -> int:
        return self.end - self.start + 1


def parse_byte_range(value: str, size: int) -> ByteRange:
    """Parse the single byte-range form used by media clients."""

    match = _RANGE_PATTERN.fullmatch(value.strip())
    if match is None or size <= 0:
        raise ValueError("invalid byte range")

    start_text, end_text = match.groups()
    if not start_text and not end_text:
        raise ValueError("empty byte range")

    if not start_text:
        suffix = int(end_text)
        if suffix <= 0:
            raise ValueError("invalid suffix byte range")
        start = max(0, size - suffix)
        return ByteRange(start, size - 1)

    start = int(start_text)
    end = int(end_text) if end_text else size - 1
    if start >= size or end < start:
        raise ValueError("unsatisfiable byte range")
    return ByteRange(start, min(end, size - 1))


class FixtureServer(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def __init__(
        self,
        address: tuple[str, int],
        root: Path,
        token: str,
        auth_mode: str,
    ) -> None:
        super().__init__(address, FixtureHandler)
        self.root = root
        self.token = token
        self.auth_mode = auth_mode


class FixtureHandler(http.server.BaseHTTPRequestHandler):
    server: FixtureServer
    protocol_version = "HTTP/1.1"

    def log_message(self, format_string: str, *arguments: object) -> None:
        message = format_string % arguments
        message = message.replace(self.server.token, "<redacted>")
        sys.stderr.write(
            f"fixture-server {self.client_address[0]} {message}\n"
        )

    def log_request(
        self, code: int | str = "-", size: int | str = "-"
    ) -> None:
        # BaseHTTPRequestHandler logs the complete request target, including a
        # query ticket. Keep only the decoded path and never retain the query.
        path = urllib.parse.urlsplit(self.path).path
        self.log_message(
            '"%s %s %s" %s %s',
            self.command,
            path,
            self.request_version,
            code,
            size,
        )

    def do_HEAD(self) -> None:  # noqa: N802 - stdlib handler API
        self._serve(send_body=False)

    def do_GET(self) -> None:  # noqa: N802 - stdlib handler API
        self._serve(send_body=True)

    def _serve(self, *, send_body: bool) -> None:
        parsed = urllib.parse.urlsplit(self.path)
        if not self._authorized(parsed):
            self._plain_error(401, "authorization required")
            return

        try:
            path = self._resolve_path(parsed.path)
        except ValueError:
            self._plain_error(404, "fixture not found")
            return

        if not path.is_file():
            self._plain_error(404, "fixture not found")
            return

        size = path.stat().st_size
        selected_range: ByteRange | None = None
        range_header = self.headers.get("Range")
        if range_header is not None:
            try:
                selected_range = parse_byte_range(range_header, size)
            except (ValueError, OverflowError):
                self.send_response(416)
                self.send_header("Content-Range", f"bytes */{size}")
                self.send_header("Content-Length", "0")
                self.send_header("Connection", "close")
                self.end_headers()
                return

        content_type = mimetypes.guess_type(path.name)[0]
        if path.suffix.lower() == ".mkv":
            content_type = "video/x-matroska"
        elif path.suffix.lower() == ".m3u8":
            content_type = "application/vnd.apple.mpegurl"
        elif path.suffix.lower() == ".ts":
            content_type = "video/mp2t"

        if selected_range is None:
            self.send_response(200)
            content_length = size
        else:
            self.send_response(206)
            content_length = selected_range.length
            self.send_header(
                "Content-Range",
                f"bytes {selected_range.start}-{selected_range.end}/{size}",
            )

        self.send_header("Content-Type", content_type or "application/octet-stream")
        self.send_header("Content-Length", str(content_length))
        self.send_header("Accept-Ranges", "bytes")
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.end_headers()

        if not send_body:
            return

        with path.open("rb") as source:
            if selected_range is not None:
                source.seek(selected_range.start)
            self._copy_exact(source, content_length)

    def _authorized(self, parsed: urllib.parse.SplitResult) -> bool:
        bearer_allowed = self.server.auth_mode in ("bearer", "either")
        query_allowed = self.server.auth_mode in ("query", "either")

        if bearer_allowed:
            authorization = self.headers.get("Authorization", "")
            expected = f"Bearer {self.server.token}"
            if hmac.compare_digest(authorization, expected):
                return True

        if query_allowed:
            values = urllib.parse.parse_qs(
                parsed.query, keep_blank_values=True
            ).get("access_token", [])
            if len(values) == 1 and hmac.compare_digest(
                values[0], self.server.token
            ):
                return True

        return False

    def _resolve_path(self, request_path: str) -> Path:
        decoded = urllib.parse.unquote(request_path)
        relative = Path(decoded.lstrip("/"))
        if not relative.parts or any(part in ("", ".", "..") for part in relative.parts):
            raise ValueError("invalid fixture path")

        candidate = (self.server.root / relative).resolve()
        try:
            candidate.relative_to(self.server.root)
        except ValueError as error:
            raise ValueError("fixture path escapes root") from error
        return candidate

    def _plain_error(self, status: int, message: str) -> None:
        body = f"{message}\n".encode("utf-8")
        self.send_response(status)
        if status == 401:
            self.send_header("WWW-Authenticate", 'Bearer realm="ferrex-fixtures"')
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("Connection", "close")
        self.end_headers()
        if self.command != "HEAD":
            self.wfile.write(body)

    def _copy_exact(self, source: BinaryIO, remaining: int) -> None:
        while remaining > 0:
            block = source.read(min(64 * 1024, remaining))
            if not block:
                break
            self.wfile.write(block)
            remaining -= len(block)


def write_port_file(path: Path, port: int) -> None:
    path = path.expanduser().resolve()
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_name(f".{path.name}.tmp-{os.getpid()}")
    temporary.write_text(f"{port}\n", encoding="utf-8")
    if os.name != "nt":
        temporary.chmod(0o600)
    temporary.replace(path)


def parse_args(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT)
    parser.add_argument("--bind", default="127.0.0.1")
    parser.add_argument("--port", type=int, default=0)
    parser.add_argument(
        "--auth",
        choices=("bearer", "query", "either"),
        default="bearer",
        help="accepted credential transport (default: bearer)",
    )
    parser.add_argument(
        "--token-env",
        default=DEFAULT_TOKEN_ENV,
        help=f"environment variable containing the token (default: {DEFAULT_TOKEN_ENV})",
    )
    parser.add_argument(
        "--port-file",
        type=Path,
        help="atomically write the selected port for smoke-test automation",
    )
    return parser.parse_args(argv)


def main(argv: Sequence[str] | None = None) -> int:
    args = parse_args(argv if argv is not None else sys.argv[1:])
    try:
        root = args.root.expanduser().resolve()
        if not root.is_dir() or not (root / MARKER_NAME).is_file():
            raise ServerError(
                f"not a generated Ferrex fixture directory: {root}"
            )
        if args.bind not in ("127.0.0.1", "localhost"):
            raise ServerError("fixture server must bind to loopback")
        if not 0 <= args.port <= 65535:
            raise ServerError("port must be between 0 and 65535")

        token = os.environ.get(args.token_env, "")
        if not token or "\r" in token or "\n" in token:
            raise ServerError(
                f"set {args.token_env} to a non-empty, single-line token"
            )

        server = FixtureServer(
            (args.bind, args.port), root, token, args.auth
        )
        if args.port_file is not None:
            write_port_file(args.port_file, server.server_port)

        stopping = threading.Event()

        def request_shutdown(_signum: int, _frame: object) -> None:
            if not stopping.is_set():
                stopping.set()
                threading.Thread(target=server.shutdown, daemon=True).start()

        for signal_number in (signal.SIGINT, signal.SIGTERM):
            signal.signal(signal_number, request_shutdown)

        print(
            f"Serving {root} at http://{args.bind}:{server.server_port}/ "
            f"with {args.auth} authentication",
            flush=True,
        )
        try:
            server.serve_forever(poll_interval=0.2)
        finally:
            server.server_close()
    except (OSError, ServerError) as error:
        print(f"native playback fixture server error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
