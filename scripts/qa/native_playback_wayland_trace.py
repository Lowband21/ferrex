#!/usr/bin/env python3
"""Capture a redacted, operation-correlated mpv Wayland protocol trace.

This is the reproducible W0 research fixture for the native playback migration.
It exercises mpv's ordinary ``gpu-next`` Wayland VO; it does not implement or
validate the Ferrex protocol bridge.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import secrets
import shutil
import socket
import subprocess
import sys
import tempfile
import time
from dataclasses import dataclass, field
from datetime import UTC, datetime
from pathlib import Path
from typing import Any, Callable, Sequence

SCHEMA_VERSION = 1
PINNED_MPV_VERSION = "0.41.0"
DEFAULT_FIXTURE_ROOT = Path("target/native-playback-fixtures")
DEFAULT_FIXTURE = "h264-sdr-8bit.mkv"
DEFAULT_RESULTS_ROOT = Path("target/native-playback-results")
_ENVIRONMENT_ID = re.compile(r"^[a-z0-9][a-z0-9._-]{0,63}$")
_RUN_ID = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._-]{0,95}$")
_TRACE_LINE = re.compile(
    r"^\[\s*(?P<timestamp>[0-9.]+)\]\s+(?:\{[^}]+\}\s+)?"
    r"(?P<request>->\s+)?(?P<interface>[A-Za-z0-9_]+)[#@]\d+\."
    r"(?P<method>[A-Za-z0-9_]+)\((?P<arguments>.*)\)$"
)
_GLOBAL = re.compile(r'^\d+, "(?P<interface>[A-Za-z0-9_]+)", (?P<version>\d+)$')
_BIND = re.compile(
    r'^\d+, "(?P<interface>[A-Za-z0-9_]+)", (?P<version>\d+),'
)
_QUOTED_ARGUMENT = r'"(?:\\.|[^"\\])*"'
_TITLE = re.compile(
    rf"(?P<prefix>xdg_toplevel[#@]\d+\.set_title\(){_QUOTED_ARGUMENT}(?P<suffix>\))"
)
_OUTPUT_IDENTITY = re.compile(
    rf"(?P<prefix>(?:wl_output|zxdg_output_v1)[#@]\d+\."
    rf"(?:name|description)\(){_QUOTED_ARGUMENT}(?P<suffix>\))"
)
_OUTPUT_GEOMETRY = re.compile(
    r"(?P<prefix>wl_output[#@]\d+\.geometry\()(?P<arguments>.*)(?P<suffix>\))"
)
_SEAT_IDENTITY = re.compile(
    rf"(?P<prefix>wl_seat[#@]\d+\.name\(){_QUOTED_ARGUMENT}(?P<suffix>\))"
)


class TraceError(RuntimeError):
    """An actionable trace-capture failure."""


@dataclass
class ProtocolInventory:
    """Protocol interfaces and methods observed in a WAYLAND_DEBUG trace."""

    advertised_globals: dict[str, int] = field(default_factory=dict)
    bound_globals: dict[str, int] = field(default_factory=dict)
    requests: dict[str, set[str]] = field(default_factory=dict)
    events: dict[str, set[str]] = field(default_factory=dict)
    message_count: int = 0
    registry_request_count: int = 0
    xdg_toplevel_candidate_count: int = 0

    def as_dict(self) -> dict[str, Any]:
        interfaces = sorted(set(self.requests) | set(self.events))
        return {
            "advertised_globals": dict(sorted(self.advertised_globals.items())),
            "bound_globals": dict(sorted(self.bound_globals.items())),
            "interfaces": interfaces,
            "requests": {
                interface: sorted(methods)
                for interface, methods in sorted(self.requests.items())
            },
            "events": {
                interface: sorted(methods)
                for interface, methods in sorted(self.events.items())
            },
            "message_count": self.message_count,
            "topology": {
                "registry_request_count": self.registry_request_count,
                "xdg_toplevel_candidate_count": self.xdg_toplevel_candidate_count,
            },
        }


class MpvIpc:
    """Small synchronous client for the private mpv JSON IPC socket."""

    def __init__(self, path: Path, timeout: float = 5.0) -> None:
        self.socket = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        self.socket.settimeout(timeout)
        self.socket.connect(os.fspath(path))
        self.buffer = bytearray()
        self.request_id = 0
        self.timeout = timeout

    def close(self) -> None:
        self.socket.close()

    def request(
        self, command: Sequence[Any], *, allow_error: bool = False
    ) -> dict[str, Any]:
        self.request_id += 1
        request_id = self.request_id
        payload = json.dumps(
            {"command": list(command), "request_id": request_id},
            ensure_ascii=True,
            separators=(",", ":"),
        ).encode("utf-8") + b"\n"
        self.socket.sendall(payload)

        deadline = time.monotonic() + self.timeout
        while True:
            while b"\n" not in self.buffer:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise TraceError("timed out waiting for an mpv IPC reply")
                self.socket.settimeout(remaining)
                chunk = self.socket.recv(64 * 1024)
                if not chunk:
                    raise TraceError("mpv closed its IPC socket before replying")
                self.buffer.extend(chunk)

            line, _, tail = self.buffer.partition(b"\n")
            self.buffer = bytearray(tail)
            if not line:
                continue
            try:
                message = json.loads(line)
            except (UnicodeDecodeError, json.JSONDecodeError) as error:
                raise TraceError("mpv returned malformed JSON over IPC") from error
            if message.get("request_id") != request_id:
                continue
            error_name = message.get("error", "unknown")
            if error_name != "success" and not allow_error:
                raise TraceError(f"mpv IPC command failed: {error_name}")
            return message

    def property(self, name: str) -> Any | None:
        reply = self.request(["get_property", name], allow_error=True)
        return reply.get("data") if reply.get("error") == "success" else None


def validate_environment_id(value: str) -> str:
    """Validate a stable, non-path environment identifier."""

    if value in (".", "..") or _ENVIRONMENT_ID.fullmatch(value) is None:
        raise TraceError(
            "environment ID must be 1-64 lowercase letters, digits, '.', '_', or '-'"
        )
    return value


def validate_run_id(value: str) -> str:
    """Validate a UTC run identifier as one safe path segment."""

    if value in (".", "..") or _RUN_ID.fullmatch(value) is None:
        raise TraceError(
            "run ID must be 1-96 letters, digits, '.', '_', or '-'"
        )
    return value


def parse_mpv_version(output: str) -> str:
    """Extract the release from mpv's first version line."""

    match = re.search(r"(?m)^mpv v([0-9]+(?:\.[0-9]+){2})(?:\s|$)", output)
    if match is None:
        raise TraceError("could not parse mpv --version output")
    return match.group(1)


def parse_protocol_inventory(trace: str) -> ProtocolInventory:
    """Reduce WAYLAND_DEBUG output to a deterministic protocol inventory."""

    inventory = ProtocolInventory()
    for line in trace.splitlines():
        match = _TRACE_LINE.match(line)
        if match is None:
            continue

        interface = match.group("interface")
        method = match.group("method")
        arguments = match.group("arguments")
        destination = inventory.requests if match.group("request") else inventory.events
        destination.setdefault(interface, set()).add(method)
        inventory.message_count += 1
        if match.group("request") and interface == "wl_display" and method == "get_registry":
            inventory.registry_request_count += 1
        elif match.group("request") and interface == "xdg_surface" and method == "get_toplevel":
            inventory.xdg_toplevel_candidate_count += 1

        if interface == "wl_registry" and method == "global" and not match.group("request"):
            global_match = _GLOBAL.fullmatch(arguments)
            if global_match is not None:
                name = global_match.group("interface")
                version = int(global_match.group("version"))
                inventory.advertised_globals[name] = max(
                    version, inventory.advertised_globals.get(name, 0)
                )
        elif interface == "wl_registry" and method == "bind" and match.group("request"):
            bind_match = _BIND.match(arguments)
            if bind_match is not None:
                name = bind_match.group("interface")
                version = int(bind_match.group("version"))
                inventory.bound_globals[name] = max(
                    version, inventory.bound_globals.get(name, 0)
                )
    return inventory


def sanitize_trace(trace: str, replacements: Sequence[str | os.PathLike[str]]) -> str:
    """Remove controlled paths plus window and output identity strings."""

    sanitized = trace
    values = {
        os.fspath(value)
        for value in replacements
        if os.fspath(value) not in ("", "/")
    }
    for value in sorted(values, key=len, reverse=True):
        sanitized = sanitized.replace(value, "<redacted-path>")
    sanitized = _TITLE.sub(
        lambda match: f'{match.group("prefix")}\"<redacted-title>\"{match.group("suffix")}',
        sanitized,
    )
    sanitized = _OUTPUT_IDENTITY.sub(
        lambda match: f'{match.group("prefix")}\"<redacted-output>\"{match.group("suffix")}',
        sanitized,
    )
    sanitized = _OUTPUT_GEOMETRY.sub(
        lambda match: (
            match.group("prefix")
            + re.sub(_QUOTED_ARGUMENT, '\"<redacted-output>\"', match.group("arguments"))
            + match.group("suffix")
        ),
        sanitized,
    )
    sanitized = _SEAT_IDENTITY.sub(
        lambda match: f'{match.group("prefix")}\"<redacted-seat>\"{match.group("suffix")}',
        sanitized,
    )
    return sanitized


def validate_capture_inventory(inventory: ProtocolInventory) -> None:
    """Require the basic W0 map/frame/fullscreen/teardown evidence."""

    required = (
        (inventory.requests, "wl_surface", "attach"),
        (inventory.requests, "wl_surface", "commit"),
        (inventory.requests, "wl_surface", "destroy"),
        (inventory.requests, "wp_presentation", "feedback"),
        (inventory.events, "wp_presentation_feedback", "presented"),
        (inventory.requests, "xdg_toplevel", "set_fullscreen"),
        (inventory.requests, "xdg_toplevel", "unset_fullscreen"),
        (inventory.requests, "xdg_toplevel", "destroy"),
    )
    missing = [
        f"{interface}.{method}"
        for mapping, interface, method in required
        if method not in mapping.get(interface, set())
    ]
    if inventory.xdg_toplevel_candidate_count != 1:
        missing.append(
            "exactly one xdg_surface.get_toplevel candidate "
            f"(observed {inventory.xdg_toplevel_candidate_count})"
        )
    if missing:
        raise TraceError(
            "trace is missing required W0 protocol evidence: " + ", ".join(missing)
        )


def sha256_file(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def wait_for_socket(path: Path, process: subprocess.Popen[bytes], timeout: float) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if path.exists():
            return
        status = process.poll()
        if status is not None:
            raise TraceError(f"mpv exited before IPC became ready (status {status})")
        time.sleep(0.025)
    raise TraceError("timed out waiting for mpv's IPC socket")


def wait_for_property(
    ipc: MpvIpc, name: str, expected: Any, timeout: float = 10.0
) -> None:
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if ipc.property(name) == expected:
            return
        time.sleep(0.05)
    raise TraceError(f"timed out waiting for mpv property {name}")


def safe_mpv_diagnostics(ipc: MpvIpc) -> dict[str, Any]:
    """Read only non-identifying VO/runtime properties."""

    properties = (
        "mpv-version",
        "ffmpeg-version",
        "libplacebo-version",
        "current-vo",
        "gpu-api",
        "gpu-context",
        "hwdec-current",
        "video-codec",
        "video-format",
        "video-params/primaries",
        "video-params/gamma",
        "video-params/colormatrix",
    )
    result: dict[str, Any] = {}
    for name in properties:
        value = ipc.property(name)
        if isinstance(value, (str, int, float, bool)) or value is None:
            result[name] = value
    return result


def marker(raw_fd: int, started: float, phase: str, operation: str) -> None:
    elapsed_ms = int((time.monotonic() - started) * 1000)
    line = f"# FERREX_OPERATION +{elapsed_ms}ms {phase} {operation}\n"
    os.write(raw_fd, line.encode("ascii"))


def terminate_process(process: subprocess.Popen[bytes]) -> None:
    if process.poll() is not None:
        return
    process.terminate()
    try:
        process.wait(timeout=3)
    except subprocess.TimeoutExpired:
        process.kill()
        process.wait(timeout=3)


def run_operation(
    operations: list[dict[str, Any]],
    raw_fd: int,
    started: float,
    name: str,
    action: Callable[[], Any],
) -> Any:
    marker(raw_fd, started, "BEGIN", name)
    operation = {
        "name": name,
        "started_ms": int((time.monotonic() - started) * 1000),
    }
    try:
        result = action()
    except Exception:
        operation["result"] = "failed"
        operation["finished_ms"] = int((time.monotonic() - started) * 1000)
        operations.append(operation)
        marker(raw_fd, started, "FAIL", name)
        raise
    operation["result"] = "passed"
    operation["finished_ms"] = int((time.monotonic() - started) * 1000)
    operations.append(operation)
    marker(raw_fd, started, "END", name)
    return result


def capture(args: argparse.Namespace) -> Path:
    environment_id = validate_environment_id(args.environment_id)
    if not os.environ.get("WAYLAND_DISPLAY"):
        raise TraceError("WAYLAND_DISPLAY is not set; run capture in a Wayland session")

    mpv = Path(args.mpv).expanduser() if args.mpv else None
    if mpv is None:
        discovered = shutil.which("mpv")
        if discovered is None:
            raise TraceError("mpv was not found; pass --mpv /path/to/mpv")
        mpv = Path(discovered)
    mpv = mpv.resolve()
    version_output = subprocess.run(
        [os.fspath(mpv), "--no-config", "--version"],
        check=True,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
    ).stdout
    mpv_version = parse_mpv_version(version_output)
    if mpv_version != PINNED_MPV_VERSION:
        raise TraceError(
            f"W0 is pinned to mpv {PINNED_MPV_VERSION}, found {mpv_version}"
        )

    fixture_root = Path(args.fixture_root).expanduser().resolve()
    fixture = (fixture_root / args.fixture).resolve()
    try:
        fixture_name = fixture.relative_to(fixture_root).as_posix()
    except ValueError as error:
        raise TraceError("fixture path must remain below --fixture-root") from error
    if not fixture.is_file():
        raise TraceError("fixture is missing; generate native playback fixtures first")

    run_id = args.run_id or (
        datetime.now(UTC).strftime("%Y%m%dT%H%M%S.%fZ")
        + f"-{secrets.token_hex(3)}"
    )
    validate_run_id(run_id)
    result_dir = (
        Path(args.results_root).expanduser().resolve() / environment_id / run_id
    )
    result_dir.mkdir(mode=0o700, parents=True, exist_ok=False)

    runtime_root_value = os.environ.get("XDG_RUNTIME_DIR")
    runtime_root = Path(runtime_root_value) if runtime_root_value else None
    if runtime_root is not None and not runtime_root.is_dir():
        runtime_root = None

    raw_fd, raw_name = tempfile.mkstemp(
        prefix="ferrex-wayland-trace-", suffix=".raw", dir=runtime_root
    )
    os.fchmod(raw_fd, 0o600)
    raw_path = Path(raw_name)
    process: subprocess.Popen[bytes] | None = None
    ipc: MpvIpc | None = None
    operations: list[dict[str, Any]] = []
    diagnostics: dict[str, Any] = {}
    outcome = "failed"
    failure: Exception | None = None
    started = time.monotonic()

    try:
        with tempfile.TemporaryDirectory(
            prefix="ferrex-mpv-wayland-", dir=runtime_root
        ) as runtime_directory:
            ipc_path = Path(runtime_directory) / "mpv.sock"
            command = [
                os.fspath(mpv),
                "--no-config",
                "--terminal=no",
                "--msg-level=all=warn",
                "--idle=no",
                "--force-window=yes",
                "--title=Ferrex Wayland trace fixture",
                "--audio=no",
                "--osc=no",
                "--input-default-bindings=no",
                "--input-vo-keyboard=no",
                "--keep-open=yes",
                "--loop-file=inf",
                "--vo=gpu-next",
                "--gpu-api=vulkan",
                "--gpu-context=waylandvk",
                "--hwdec=auto-safe",
                f"--input-ipc-server={ipc_path}",
                "--",
                os.fspath(fixture),
            ]
            environment = os.environ.copy()
            environment["WAYLAND_DEBUG"] = "client"
            marker(raw_fd, started, "BEGIN", "process-start")
            process = subprocess.Popen(
                command,
                stdin=subprocess.DEVNULL,
                stdout=subprocess.DEVNULL,
                stderr=raw_fd,
                env=environment,
            )
            wait_for_socket(ipc_path, process, args.startup_timeout)
            ipc = MpvIpc(ipc_path)
            run_operation(
                operations,
                raw_fd,
                started,
                "initial-map",
                lambda: wait_for_property(ipc, "vo-configured", True),
            )
            time.sleep(args.settle_seconds)
            diagnostics = safe_mpv_diagnostics(ipc)

            run_operation(
                operations,
                raw_fd,
                started,
                "pause",
                lambda: ipc.request(["set_property", "pause", True]),
            )
            time.sleep(args.settle_seconds)
            run_operation(
                operations,
                raw_fd,
                started,
                "seek",
                lambda: ipc.request(["seek", 2.0, "absolute+exact"]),
            )
            time.sleep(args.settle_seconds)
            run_operation(
                operations,
                raw_fd,
                started,
                "resume",
                lambda: ipc.request(["set_property", "pause", False]),
            )
            run_operation(
                operations,
                raw_fd,
                started,
                "resize",
                lambda: ipc.request(["set_property", "window-scale", 0.8]),
            )
            time.sleep(args.settle_seconds)
            run_operation(
                operations,
                raw_fd,
                started,
                "fullscreen-enter",
                lambda: ipc.request(["set_property", "fullscreen", True]),
            )
            wait_for_property(ipc, "fullscreen", True)
            time.sleep(args.settle_seconds)
            run_operation(
                operations,
                raw_fd,
                started,
                "fullscreen-exit",
                lambda: ipc.request(["set_property", "fullscreen", False]),
            )
            wait_for_property(ipc, "fullscreen", False)
            time.sleep(args.settle_seconds)
            run_operation(
                operations,
                raw_fd,
                started,
                "stop",
                lambda: ipc.request(["stop"]),
            )
            run_operation(
                operations,
                raw_fd,
                started,
                "vo-reload",
                lambda: ipc.request(["loadfile", os.fspath(fixture), "replace"]),
            )
            wait_for_property(ipc, "vo-configured", True)
            time.sleep(args.settle_seconds)
            run_operation(
                operations,
                raw_fd,
                started,
                "quit",
                lambda: ipc.request(["quit"], allow_error=True),
            )
            ipc.close()
            ipc = None
            try:
                process.wait(timeout=5)
            except subprocess.TimeoutExpired as error:
                raise TraceError("mpv did not terminate after quit") from error
            marker(raw_fd, started, "END", "process-stop")
            if process.returncode not in (0, None):
                raise TraceError(f"mpv exited with status {process.returncode}")
            outcome = "passed"
    except Exception as error:  # retain a redacted partial trace for diagnosis
        failure = error
    finally:
        if ipc is not None:
            try:
                ipc.request(["quit"], allow_error=True)
            except Exception:
                pass
            ipc.close()
        if process is not None:
            terminate_process(process)
        os.close(raw_fd)

    raw_trace = raw_path.read_text(encoding="utf-8", errors="replace")
    raw_path.unlink(missing_ok=True)
    sanitized_trace = sanitize_trace(
        raw_trace,
        (
            fixture,
            fixture_root,
            Path.cwd().resolve(),
            Path.home().resolve(),
            runtime_root or "",
        ),
    )
    trace_path = result_dir / "wayland-client.log"
    trace_path.write_text(sanitized_trace, encoding="utf-8")
    trace_path.chmod(0o600)

    parsed_inventory = parse_protocol_inventory(sanitized_trace)
    if failure is None:
        try:
            validate_capture_inventory(parsed_inventory)
        except TraceError as error:
            failure = error
            outcome = "failed"
    inventory = parsed_inventory.as_dict()
    summary: dict[str, Any] = {
        "schema_version": SCHEMA_VERSION,
        "outcome": outcome,
        "environment_id": environment_id,
        "run_id": run_id,
        "captured_at_utc": datetime.now(UTC).isoformat(),
        "fixture": {
            "name": fixture_name,
            "sha256": sha256_file(fixture),
        },
        "mpv": {
            "pinned_version": PINNED_MPV_VERSION,
            "observed_version": mpv_version,
            "diagnostics": diagnostics,
        },
        "operations": operations,
        "protocol_inventory": inventory,
        "artifacts": {"wayland_client_trace": trace_path.name},
    }
    if failure is not None:
        failure_text = sanitize_trace(str(failure), (fixture, fixture_root, Path.home()))
        summary["failure"] = failure_text
    summary_path = result_dir / "summary.json"
    summary_path.write_text(
        json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8"
    )
    summary_path.chmod(0o600)

    print(result_dir)
    if failure is not None:
        if isinstance(failure, TraceError):
            raise failure
        raise TraceError("Wayland trace capture failed; inspect the redacted result") from failure
    return result_dir


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--environment-id", required=True)
    parser.add_argument("--fixture-root", default=DEFAULT_FIXTURE_ROOT)
    parser.add_argument("--fixture", default=DEFAULT_FIXTURE)
    parser.add_argument("--results-root", default=DEFAULT_RESULTS_ROOT)
    parser.add_argument("--run-id")
    parser.add_argument("--mpv")
    parser.add_argument("--startup-timeout", type=float, default=10.0)
    parser.add_argument("--settle-seconds", type=float, default=0.4)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        capture(args)
    except (TraceError, OSError, subprocess.SubprocessError) as error:
        parser.exit(1, f"error: {error}\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
