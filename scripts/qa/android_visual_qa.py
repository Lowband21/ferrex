#!/usr/bin/env python3
"""Host-side Android visual QA scenario screenshot runner.

The runner launches debug-only Ferrex visual QA scenarios on explicit ADB
serials, captures PNG screenshots, validates dimensions, and writes a run
manifest plus redacted failure logcat snippets under target/android-visual-qa.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
import re
import shutil
import struct
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping, Sequence

ACTION_VISUAL_QA = "com.ferrex.android.action.VISUAL_QA"
EXTRA_SCENARIO_ID = "com.ferrex.android.extra.QA_SCENARIO_ID"
VISUAL_QA_ACTIVITY = "com.ferrex.android.qa.FerrexVisualQaActivity"
DEFAULT_OUTPUT_DIR = Path("target/android-visual-qa")
DEFAULT_SETTLE_MS = 1500
DEFAULT_LOG_LINES = 240

PHONE_EXPECTED_SIZE = (1080, 2400)
TV_EXPECTED_SIZE = (1920, 1080)

PNG_SIGNATURE = b"\x89PNG\r\n\x1a\n"

TV_DPAD_SEQUENCES: Mapping[str, tuple[str, ...]] = {
    "tv-home-focus": ("KEYCODE_DPAD_DOWN", "KEYCODE_DPAD_RIGHT"),
    "tv-grid-focus": ("KEYCODE_DPAD_DOWN", "KEYCODE_DPAD_RIGHT"),
    "tv-detail-focus": ("KEYCODE_DPAD_DOWN", "KEYCODE_DPAD_RIGHT"),
    "tv-search-focus": ("KEYCODE_DPAD_DOWN", "KEYCODE_DPAD_RIGHT"),
    "tv-recovery-focus": ("KEYCODE_DPAD_DOWN", "KEYCODE_DPAD_RIGHT"),
}

REDACTION_PATTERNS: tuple[tuple[re.Pattern[str], str], ...] = (
    (
        re.compile(r"(?i)(authorization\s*[:=]\s*)(?:bearer|basic)?\s*[^\s,;]+"),
        r"\1<redacted>",
    ),
    (
        re.compile(r"(?i)\b(bearer|basic)\s+[A-Za-z0-9._~+/=-]+"),
        r"\1 <redacted>",
    ),
    (
        re.compile(
            r"(?i)([?&](?:access[_-]?token|refresh[_-]?token|id[_-]?token|session[_-]?id|token|ticket|api[_-]?key|secret|password)=)[^\s&#]+"
        ),
        r"\1<redacted>",
    ),
    (
        re.compile(
            r"(?i)([\"']?(?:access[_-]?token|refresh[_-]?token|id[_-]?token|session[_-]?token|session[_-]?id|device[_-]?session[_-]?id|playback[_-]?ticket|ticket|api[_-]?key|token|secret|password)[\"']?\s*[:=]\s*[\"']?)[^\"'\s,}]+"
        ),
        r"\1<redacted>",
    ),
    (
        re.compile(r"https?://(?:[A-Za-z0-9.-]+|\[[0-9A-Fa-f:]+\])(?::\d+)?"),
        "https://<redacted-origin>",
    ),
    (
        re.compile(
            r"\b(?:10|127|172\.(?:1[6-9]|2\d|3[01])|192\.168)\.\d{1,3}\.\d{1,3}(?::\d+)?\b"
        ),
        "<redacted-origin>",
    ),
)


class VisualQaError(RuntimeError):
    """Base error for expected runner failures."""


class CommandError(VisualQaError):
    def __init__(self, args: Sequence[str], returncode: int, stdout: str, stderr: str):
        self.args_for_command = list(args)
        self.returncode = returncode
        self.stdout = stdout
        self.stderr = stderr
        super().__init__(self._message())

    def _message(self) -> str:
        command = " ".join(self.args_for_command)
        output = (self.stderr or self.stdout).strip()
        if output:
            output = redact_text(output)
            output = "\n" + "\n".join(output.splitlines()[-12:])
        return f"command failed ({self.returncode}): {command}{output}"


@dataclass(frozen=True)
class Scenario:
    id: str
    target: str


@dataclass(frozen=True)
class TargetConfig:
    target: str
    serial: str
    default_serial: str
    package: str
    apk_path: Path
    expected_size: tuple[int, int]
    screenshot_helper: str

    @property
    def component(self) -> str:
        return f"{self.package}/{VISUAL_QA_ACTIVITY}"


@dataclass(frozen=True)
class PngDimensions:
    width: int
    height: int

    def to_json(self) -> dict[str, int]:
        return {"width": self.width, "height": self.height}


@dataclass(frozen=True)
class RunResult:
    stdout: str
    stderr: str
    returncode: int


def utc_now() -> str:
    return dt.datetime.now(dt.UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def repo_root_from_script() -> Path:
    return Path(__file__).resolve().parents[2]


def redact_text(text: str) -> str:
    redacted = text
    for pattern, replacement in REDACTION_PATTERNS:
        redacted = pattern.sub(replacement, redacted)
    return redacted


def bounded_lines(text: str, max_lines: int) -> str:
    if max_lines <= 0:
        return ""
    lines = text.splitlines()
    return "\n".join(lines[-max_lines:])


def run_command(
    args: Sequence[str | os.PathLike[str]],
    *,
    check: bool = True,
    timeout: int | float | None = 120,
    input_bytes: bytes | None = None,
) -> RunResult:
    string_args = [os.fspath(arg) for arg in args]
    completed = subprocess.run(
        string_args,
        input=input_bytes,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=timeout,
    )
    stdout = completed.stdout.decode("utf-8", errors="replace").replace("\r", "")
    stderr = completed.stderr.decode("utf-8", errors="replace").replace("\r", "")
    if check and completed.returncode != 0:
        raise CommandError(string_args, completed.returncode, stdout, stderr)
    return RunResult(stdout=stdout, stderr=stderr, returncode=completed.returncode)


def run_command_to_file(
    args: Sequence[str | os.PathLike[str]],
    output: Path,
    *,
    timeout: int | float | None = 120,
) -> RunResult:
    string_args = [os.fspath(arg) for arg in args]
    output.parent.mkdir(parents=True, exist_ok=True)
    with output.open("wb") as handle:
        completed = subprocess.run(
            string_args,
            stdout=handle,
            stderr=subprocess.PIPE,
            check=False,
            timeout=timeout,
        )
    stderr = completed.stderr.decode("utf-8", errors="replace").replace("\r", "")
    if completed.returncode != 0:
        raise CommandError(string_args, completed.returncode, "", stderr)
    return RunResult(stdout="", stderr=stderr, returncode=completed.returncode)


def command_output_or_none(args: Sequence[str | os.PathLike[str]], timeout: int = 15) -> str | None:
    try:
        result = run_command(args, check=True, timeout=timeout)
    except (OSError, subprocess.SubprocessError, VisualQaError):
        return None
    output = (result.stdout or result.stderr).strip()
    return output or None


def resolve_executable(name: str) -> str:
    path = shutil.which(name)
    if path is None:
        raise VisualQaError(f"required command is missing from PATH: {name}")
    return path


def command_identity(name: str, version_args: Sequence[str] = ("--version",)) -> dict[str, object]:
    path = shutil.which(name)
    if path is None:
        return {"path": None, "available": False, "version": None}
    output = command_output_or_none([path, *version_args])
    return {"path": path, "available": True, "version": output}


def helper_identity(name: str) -> dict[str, object]:
    path = shutil.which(name)
    return {"path": path, "available": path is not None, "version": None}


def file_identity(path: Path) -> dict[str, object]:
    if not path.exists():
        return {"path": str(path), "exists": False}
    hasher = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(65536), b""):
            hasher.update(chunk)
    stat = path.stat()
    return {
        "path": str(path),
        "exists": True,
        "bytes": stat.st_size,
        "mtime": dt.datetime.fromtimestamp(stat.st_mtime, dt.UTC)
        .replace(microsecond=0)
        .isoformat()
        .replace("+00:00", "Z"),
        "sha256": hasher.hexdigest(),
    }


def collect_command_versions(adb: str, script_path: Path) -> dict[str, object]:
    versions: dict[str, object] = {
        "script": file_identity(script_path),
        "python": {"path": sys.executable, "version": sys.version.split()[0]},
        "adb": {
            "path": adb,
            "version": command_output_or_none([adb, "version"]),
        },
        "aapt2": command_identity("aapt2", ("version",)),
        "ferrex_android_emulators": helper_identity("ferrex-android-emulators"),
        # Screenshot convenience aliases intentionally do not have a version
        # command; invoking them would try to capture from a device. Record
        # availability/path only so manifest collection remains side-effect-free.
        "ferrex_android_screenshot_phone": helper_identity("ferrex-android-screenshot-phone"),
        "ferrex_android_screenshot_tv": helper_identity("ferrex-android-screenshot-tv"),
    }
    return versions


class ScenarioRegistry:
    def __init__(self, scenarios: Sequence[Scenario], source_path: Path):
        self.scenarios = list(scenarios)
        self.source_path = source_path
        self.by_id = {scenario.id: scenario for scenario in self.scenarios}
        if len(self.by_id) != len(self.scenarios):
            raise VisualQaError("visual QA registry contains duplicate scenario IDs")

    @classmethod
    def load(cls, repo_root: Path) -> "ScenarioRegistry":
        source_path = repo_root / "mobile/android/app/src/main/kotlin/com/ferrex/android/ui/qa/FerrexVisualQa.kt"
        try:
            content = source_path.read_text(encoding="utf-8")
        except OSError as exc:
            raise VisualQaError(f"failed to read visual QA scenario registry: {source_path}: {exc}") from exc

        constants = dict(re.findall(r"const\s+val\s+(\w+)\s*=\s*\"([^\"]+)\"", content))
        match = re.search(r"val\s+requiredScenarioIds\s*=\s*listOf\((.*?)\)", content, re.DOTALL)
        if match is None:
            raise VisualQaError(f"could not find FerrexVisualQaScenarios.requiredScenarioIds in {source_path}")

        ordered_constant_names = re.findall(r"FerrexQaScenarioIds\.(\w+)", match.group(1))
        if not ordered_constant_names:
            raise VisualQaError(f"visual QA requiredScenarioIds is empty in {source_path}")

        scenarios: list[Scenario] = []
        for constant_name in ordered_constant_names:
            scenario_id = constants.get(constant_name)
            if scenario_id is None:
                raise VisualQaError(
                    f"requiredScenarioIds references missing FerrexQaScenarioIds.{constant_name} in {source_path}"
                )
            target = target_for_scenario_id(scenario_id)
            scenarios.append(Scenario(id=scenario_id, target=target))
        return cls(scenarios, source_path)

    def select(self, target: str, scenario_id: str) -> list[Scenario]:
        normalized_target = normalize_target(target)
        requested = scenario_id.strip()
        if requested == "all":
            return [
                scenario
                for scenario in self.scenarios
                if normalized_target == "all" or scenario.target == normalized_target
            ]

        scenario = self.by_id.get(requested)
        if scenario is None:
            available = ", ".join(scenario.id for scenario in self.scenarios)
            raise VisualQaError(f"unknown visual QA scenario {requested!r}; available: {available}")
        if normalized_target != "all" and scenario.target != normalized_target:
            raise VisualQaError(
                f"scenario {requested!r} belongs to {scenario.target}, not requested target {normalized_target}"
            )
        return [scenario]

    def to_json(self) -> dict[str, object]:
        return {
            "source_path": str(self.source_path),
            "required_scenario_ids": [scenario.id for scenario in self.scenarios],
            "scenario_count": len(self.scenarios),
        }


def target_for_scenario_id(scenario_id: str) -> str:
    if scenario_id.startswith("phone-"):
        return "phone"
    if scenario_id.startswith("tv-"):
        return "tv"
    raise VisualQaError(f"scenario ID does not declare a phone/tv target by prefix: {scenario_id}")


def normalize_target(target: str) -> str:
    normalized = target.strip().lower()
    if normalized in {"phone", "mobile"}:
        return "phone"
    if normalized in {"tv", "television"}:
        return "tv"
    if normalized in {"all", "both"}:
        return "all"
    raise VisualQaError(f"unknown target {target!r}; expected phone, tv, or all")


def parse_expected_size(value: str) -> tuple[int, int]:
    match = re.fullmatch(r"(\d+)x(\d+)", value.strip())
    if match is None:
        raise VisualQaError(f"invalid expected size {value!r}; expected WIDTHxHEIGHT")
    width = int(match.group(1))
    height = int(match.group(2))
    if width <= 0 or height <= 0:
        raise VisualQaError(f"invalid expected size {value!r}; dimensions must be positive")
    return (width, height)


def target_configs(repo_root: Path, args: argparse.Namespace) -> dict[str, TargetConfig]:
    phone_serial = os.environ.get("FERREX_ANDROID_PHONE_SERIAL", "emulator-5554")
    tv_serial = os.environ.get("FERREX_ANDROID_TV_SERIAL", "emulator-5556")

    phone_expected = PHONE_EXPECTED_SIZE
    tv_expected = TV_EXPECTED_SIZE

    normalized_target = normalize_target(args.target)
    if args.hardware:
        if normalized_target == "all":
            raise VisualQaError("--hardware requires --target phone or --target tv")
        hardware_serial = args.hardware_serial or os.environ.get("FERREX_ANDROID_HARDWARE_SERIAL")
        if not hardware_serial:
            raise VisualQaError(
                "--hardware requires --hardware-serial or FERREX_ANDROID_HARDWARE_SERIAL; no hardware serial defaults are committed"
            )
        hardware_expected_raw = args.expected_size or os.environ.get("FERREX_ANDROID_HARDWARE_EXPECTED_SIZE")
        if hardware_expected_raw:
            expected_size = parse_expected_size(hardware_expected_raw)
            if normalized_target == "phone":
                phone_expected = expected_size
            else:
                tv_expected = expected_size
        if normalized_target == "phone":
            phone_serial = hardware_serial
        else:
            tv_serial = hardware_serial
    elif args.hardware_serial or args.expected_size:
        raise VisualQaError("--hardware-serial and --expected-size are only valid with --hardware")

    return {
        "phone": TargetConfig(
            target="phone",
            serial=phone_serial,
            default_serial="emulator-5554",
            package="com.ferrex.android.debug",
            apk_path=repo_root / "mobile/android/app/build/outputs/apk/mobile/debug/app-mobile-debug.apk",
            expected_size=phone_expected,
            screenshot_helper="ferrex-android-screenshot-phone",
        ),
        "tv": TargetConfig(
            target="tv",
            serial=tv_serial,
            default_serial="emulator-5556",
            package="com.ferrex.android.tv.debug",
            apk_path=repo_root / "mobile/android/app/build/outputs/apk/tv/debug/app-tv-debug.apk",
            expected_size=tv_expected,
            screenshot_helper="ferrex-android-screenshot-tv",
        ),
    }


def adb_shell(adb: str, serial: str, *shell_args: str, check: bool = True, timeout: int = 60) -> RunResult:
    return run_command([adb, "-s", serial, "shell", *shell_args], check=check, timeout=timeout)


def require_serial_present(adb: str, serial: str) -> None:
    devices = run_command([adb, "devices"], timeout=15).stdout.splitlines()
    state = ""
    for line in devices:
        parts = line.split()
        if len(parts) >= 2 and parts[0] == serial:
            state = parts[1]
            break
    if state != "device":
        raise VisualQaError(f"ADB serial {serial} is not ready (state: {state or 'missing'})")


def getprop(adb: str, serial: str, name: str) -> str:
    return adb_shell(adb, serial, "getprop", name, timeout=30).stdout.strip()


def collect_serial_metadata(adb: str, config: TargetConfig) -> dict[str, object]:
    serial = config.serial
    props = {
        "sdk": getprop(adb, serial, "ro.build.version.sdk"),
        "release": getprop(adb, serial, "ro.build.version.release"),
        "model": getprop(adb, serial, "ro.product.model"),
        "device": getprop(adb, serial, "ro.product.device"),
        "manufacturer": getprop(adb, serial, "ro.product.manufacturer"),
        "brand": getprop(adb, serial, "ro.product.brand"),
        "product": getprop(adb, serial, "ro.product.name"),
        "abi": getprop(adb, serial, "ro.product.cpu.abi"),
    }
    wm_size = adb_shell(adb, serial, "wm", "size", timeout=30).stdout.strip()
    wm_density = adb_shell(adb, serial, "wm", "density", timeout=30).stdout.strip()
    features = adb_shell(adb, serial, "pm", "list", "features", timeout=30).stdout.splitlines()
    leanback = "feature:android.software.leanback" in {line.strip() for line in features}
    return {
        "target": config.target,
        "serial": serial,
        "expected_serial_default": config.default_serial,
        "is_default_emulator_serial": serial == config.default_serial,
        "properties": props,
        "wm_size": wm_size,
        "wm_density": wm_density,
        "leanback": leanback,
    }


def parse_package_dump(dump: str) -> dict[str, str | None]:
    def first(pattern: str) -> str | None:
        match = re.search(pattern, dump, re.MULTILINE)
        return match.group(1).strip() if match else None

    return {
        "version_code": first(r"^\s*versionCode=([^\s]+)"),
        "version_name": first(r"^\s*versionName=(.*)$"),
        "first_install_time": first(r"^\s*firstInstallTime=(.*)$"),
        "last_update_time": first(r"^\s*lastUpdateTime=(.*)$"),
    }


def collect_package_metadata(adb: str, config: TargetConfig) -> dict[str, object]:
    package_path = adb_shell(adb, config.serial, "pm", "path", config.package, timeout=30).stdout.strip()
    if not package_path:
        raise VisualQaError(
            f"{config.target} package {config.package} is not installed on {config.serial}; run scripts/qa/android-emulator-qa.sh install {config.target} first"
        )
    dump = adb_shell(adb, config.serial, "dumpsys", "package", config.package, timeout=60).stdout
    return {
        "package_name": config.package,
        "installed_package_path": package_path.removeprefix("package:"),
        "host_apk": file_identity(config.apk_path),
        **parse_package_dump(dump),
    }


def force_stop_package(adb: str, config: TargetConfig) -> None:
    adb_shell(adb, config.serial, "am", "force-stop", config.package, check=False, timeout=30)


def launch_scenario(adb: str, config: TargetConfig, scenario: Scenario) -> dict[str, object]:
    command = [
        adb,
        "-s",
        config.serial,
        "shell",
        "am",
        "start",
        "-W",
        "-a",
        ACTION_VISUAL_QA,
        "-c",
        "android.intent.category.DEFAULT",
        "-n",
        config.component,
        "--es",
        EXTRA_SCENARIO_ID,
        scenario.id,
    ]
    result = run_command(command, timeout=60)
    output = (result.stdout + "\n" + result.stderr).strip()
    if re.search(r"(?im)^\s*(Error|Exception):", output):
        raise VisualQaError(f"am start reported an error launching {scenario.id}: {redact_text(output)}")
    return {
        "action": ACTION_VISUAL_QA,
        "extra_scenario_id": EXTRA_SCENARIO_ID,
        "component": config.component,
        "stdout": output,
    }


def drive_scenario(adb: str, config: TargetConfig, scenario: Scenario) -> list[str]:
    keys = list(TV_DPAD_SEQUENCES.get(scenario.id, ()))
    for key in keys:
        adb_shell(adb, config.serial, "input", "keyevent", key, timeout=30)
        time.sleep(0.15)
    return keys


def capture_screenshot(
    adb: str,
    config: TargetConfig,
    output_path: Path,
    prefer_nix_helper: bool,
) -> dict[str, object]:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    helper_path = shutil.which(config.screenshot_helper)
    if prefer_nix_helper and helper_path and config.serial == config.default_serial:
        run_command([helper_path, output_path], timeout=120)
        return {
            "method": "nix-screenshot-helper",
            "command": [helper_path, str(output_path)],
            "serial": config.serial,
        }

    run_command_to_file([adb, "-s", config.serial, "exec-out", "screencap", "-p"], output_path, timeout=120)
    return {
        "method": "adb-exec-out-screencap",
        "command": [adb, "-s", config.serial, "exec-out", "screencap", "-p"],
        "serial": config.serial,
    }


def validate_png(path: Path, expected_size: tuple[int, int]) -> PngDimensions:
    if not path.exists():
        raise VisualQaError(f"screenshot PNG is missing: {path}")
    size = path.stat().st_size
    if size <= 0:
        raise VisualQaError(f"screenshot PNG is zero bytes: {path}")
    with path.open("rb") as handle:
        header = handle.read(24)
    if len(header) < 24 or not header.startswith(PNG_SIGNATURE) or header[12:16] != b"IHDR":
        raise VisualQaError(f"screenshot artifact is not a valid PNG with IHDR header: {path}")
    width, height = struct.unpack(">II", header[16:24])
    expected_width, expected_height = expected_size
    if (width, height) != (expected_width, expected_height):
        raise VisualQaError(
            f"screenshot dimensions for {path} were {width}x{height}; expected {expected_width}x{expected_height}"
        )
    return PngDimensions(width=width, height=height)


def capture_failure_logcat(adb: str, config: TargetConfig, output_path: Path, max_lines: int) -> dict[str, object]:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    try:
        result = run_command(
            [adb, "-s", config.serial, "logcat", "-d", "-v", "threadtime", "-t", str(max_lines)],
            check=False,
            timeout=45,
        )
        raw = result.stdout or result.stderr
        log = redact_text(bounded_lines(raw, max_lines))
        output_path.write_text(log + ("\n" if log else ""), encoding="utf-8")
        return {
            "path": str(output_path),
            "lines": len(log.splitlines()) if log else 0,
            "max_lines": max_lines,
            "redacted": True,
            "capture_returncode": result.returncode,
        }
    except Exception as exc:  # noqa: BLE001 - failure artifacts should not hide primary failure.
        message = redact_text(str(exc))
        output_path.write_text(message + "\n", encoding="utf-8")
        return {
            "path": str(output_path),
            "lines": 1,
            "max_lines": max_lines,
            "redacted": True,
            "error": message,
        }


def write_json(path: Path, data: Mapping[str, object]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    tmp = path.with_name(path.name + ".tmp")
    tmp.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    tmp.replace(path)


def capture_one(
    *,
    adb: str,
    config: TargetConfig,
    scenario: Scenario,
    output_dir: Path,
    settle_ms: int,
    log_lines: int,
    prefer_nix_helper: bool,
) -> dict[str, object]:
    started_at = utc_now()
    screenshot_path = output_dir / config.target / f"{scenario.id}.png"
    failure_log_path = output_dir / "logs" / f"{config.target}-{scenario.id}-failure-logcat.txt"
    record: dict[str, object] = {
        "target": config.target,
        "scenario_id": scenario.id,
        "serial": config.serial,
        "package_name": config.package,
        "expected_dimensions": {"width": config.expected_size[0], "height": config.expected_size[1]},
        "screenshot_path": str(screenshot_path),
        "started_at": started_at,
        "status": "running",
    }

    try:
        require_serial_present(adb, config.serial)
        record["serial_metadata"] = collect_serial_metadata(adb, config)
        record["package_metadata"] = collect_package_metadata(adb, config)
        force_stop_package(adb, config)
        record["launch"] = launch_scenario(adb, config, scenario)
        record["dpad_key_events"] = drive_scenario(adb, config, scenario)
        time.sleep(settle_ms / 1000.0)
        record["screenshot_capture"] = capture_screenshot(adb, config, screenshot_path, prefer_nix_helper)
        dimensions = validate_png(screenshot_path, config.expected_size)
        record["dimensions"] = dimensions.to_json()
        record["ended_at"] = utc_now()
        record["status"] = "passed"
    except Exception as exc:  # noqa: BLE001 - capture must emit failure artifacts for any failure.
        record["ended_at"] = utc_now()
        record["status"] = "failed"
        record["error"] = redact_text(str(exc))
        record["failure_logcat"] = capture_failure_logcat(adb, config, failure_log_path, log_lines)
    return record


def run_capture(args: argparse.Namespace) -> int:
    repo_root = repo_root_from_script()
    registry = ScenarioRegistry.load(repo_root)
    selected = registry.select(args.target, args.scenario)
    configs = target_configs(repo_root, args)
    adb = resolve_executable(args.adb)
    output_dir = Path(args.output_dir)
    if not output_dir.is_absolute():
        output_dir = repo_root / output_dir
    output_dir.mkdir(parents=True, exist_ok=True)

    manifest: dict[str, object] = {
        "schema_version": 1,
        "command": "android-visual-qa capture",
        "argv": sys.argv[1:],
        "started_at": utc_now(),
        "output_dir": str(output_dir),
        "hardware_confirmation": bool(args.hardware),
        "registry": registry.to_json(),
        "command_versions": collect_command_versions(adb, Path(__file__).resolve()),
        "captures": [],
        "failures": [],
    }

    captures: list[dict[str, object]] = []
    for scenario in selected:
        config = configs[scenario.target]
        print(f"android-visual-qa: capture {scenario.id} on {scenario.target} ({config.serial})", file=sys.stderr)
        record = capture_one(
            adb=adb,
            config=config,
            scenario=scenario,
            output_dir=output_dir,
            settle_ms=args.settle_ms,
            log_lines=args.log_lines,
            prefer_nix_helper=not args.no_nix_screenshot,
        )
        captures.append(record)
        if record["status"] == "passed":
            print(f"android-visual-qa: wrote {record['screenshot_path']}", file=sys.stderr)
        else:
            print(f"android-visual-qa: FAILED {scenario.id}: {record['error']}", file=sys.stderr)

    failures = [record for record in captures if record.get("status") != "passed"]
    manifest["captures"] = captures
    manifest["failures"] = failures
    manifest["ended_at"] = utc_now()
    manifest["status"] = "failed" if failures else "passed"
    manifest_path = output_dir / "manifest.json"
    write_json(manifest_path, manifest)
    print(f"android-visual-qa: manifest {manifest_path}", file=sys.stderr)
    return 1 if failures else 0


def run_list(args: argparse.Namespace) -> int:
    repo_root = repo_root_from_script()
    registry = ScenarioRegistry.load(repo_root)
    selected = registry.select(args.target, "all")
    if args.json:
        print(json.dumps([scenario.__dict__ for scenario in selected], indent=2, sort_keys=True))
    else:
        for scenario in selected:
            print(f"{scenario.target}\t{scenario.id}")
    return 0


def positive_int(value: str) -> int:
    parsed = int(value)
    if parsed <= 0:
        raise argparse.ArgumentTypeError("must be positive")
    return parsed


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        prog="android-visual-qa",
        description="Capture Ferrex Android debug visual QA scenario screenshots with metadata.",
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    list_parser = subparsers.add_parser("list", help="List scenario IDs from the debug registry")
    list_parser.add_argument("--target", choices=("phone", "mobile", "tv", "all"), default="all")
    list_parser.add_argument("--json", action="store_true", help="Print JSON instead of tab-separated text")
    list_parser.set_defaults(func=run_list)

    capture = subparsers.add_parser("capture", help="Launch scenarios and capture validated PNGs")
    capture.add_argument("--target", choices=("phone", "mobile", "tv", "all"), required=True)
    capture.add_argument(
        "--scenario",
        required=True,
        help="Scenario ID from android-visual-qa list, or 'all' for the required target matrix",
    )
    capture.add_argument("--output-dir", default=str(DEFAULT_OUTPUT_DIR), help="Output directory for PNGs and manifest")
    capture.add_argument("--settle-ms", type=positive_int, default=DEFAULT_SETTLE_MS)
    capture.add_argument("--log-lines", type=positive_int, default=DEFAULT_LOG_LINES)
    capture.add_argument("--adb", default=os.environ.get("ADB", "adb"), help="adb executable to use")
    capture.add_argument(
        "--no-nix-screenshot",
        action="store_true",
        help="Do not use ferrex-android-screenshot-phone/tv helpers; always use adb -s exec-out screencap",
    )
    capture.add_argument(
        "--hardware",
        action="store_true",
        help="Use an explicitly supplied hardware serial instead of the emulator serial for the requested target",
    )
    capture.add_argument(
        "--hardware-serial",
        help="Hardware ADB serial; equivalent env: FERREX_ANDROID_HARDWARE_SERIAL. No default is provided.",
    )
    capture.add_argument(
        "--expected-size",
        help="Override expected PNG dimensions only with --hardware, e.g. 1080x2400; equivalent env: FERREX_ANDROID_HARDWARE_EXPECTED_SIZE",
    )
    capture.set_defaults(func=run_capture)
    return parser


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)
    try:
        return args.func(args)
    except VisualQaError as exc:
        parser.exit(2, f"android-visual-qa: ERROR: {redact_text(str(exc))}\n")


if __name__ == "__main__":
    raise SystemExit(main())
