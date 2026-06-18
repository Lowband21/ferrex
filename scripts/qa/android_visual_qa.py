#!/usr/bin/env python3
"""Host-side Android visual QA scenario screenshot runner.

The runner launches debug-only Ferrex visual QA scenarios on explicit ADB
serials, captures PNG screenshots, validates dimensions, and writes a run
manifest plus redacted failure logcat snippets under target/android-visual-qa.
"""

from __future__ import annotations

import argparse
from collections import Counter
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
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path
from typing import Mapping, Sequence

ACTION_VISUAL_QA = "com.ferrex.android.action.VISUAL_QA"
EXTRA_SCENARIO_ID = "com.ferrex.android.extra.QA_SCENARIO_ID"
VISUAL_QA_ACTIVITY = "com.ferrex.android.qa.FerrexVisualQaActivity"
DEFAULT_OUTPUT_DIR = Path("target/android-visual-qa")
DEFAULT_SETTLE_MS = 1500
DEFAULT_LOG_LINES = 240
ACCESSIBILITY_REACHABILITY_STEPS = 6
GATE_MODES = ("smoke", "complete")
SMOKE_SCENARIO_IDS = ("phone-home", "tv-home-focus")

PHONE_EXPECTED_SIZE = (1080, 2400)
PHONE_LANDSCAPE_FOLDABLE_SIZE = (1800, 1200)
TV_EXPECTED_SIZE = (1920, 1080)
TV_4K_SCALED_SIZE = (3840, 2160)

DEFAULT_VIEWPORT_PROFILE_NAMES = (
    "phone-portrait",
    "phone-landscape-foldable",
    "tv-1080p",
    "tv-4k-scaled",
)

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
class ViewportProfile:
    name: str
    target: str
    expected_size: tuple[int, int]
    wm_size: tuple[int, int] | None
    wm_density: int | None
    description: str

    def to_json(self) -> dict[str, object]:
        data: dict[str, object] = {
            "name": self.name,
            "target": self.target,
            "expected_dimensions": {"width": self.expected_size[0], "height": self.expected_size[1]},
            "description": self.description,
        }
        if self.wm_size is not None:
            data["wm_size"] = f"{self.wm_size[0]}x{self.wm_size[1]}"
        if self.wm_density is not None:
            data["wm_density"] = self.wm_density
        return data


@dataclass(frozen=True)
class WmOverrideSnapshot:
    raw_size: str
    raw_density: str
    override_size: str | None
    override_density: str | None

    def to_json(self) -> dict[str, object]:
        return {
            "raw_size": self.raw_size,
            "raw_density": self.raw_density,
            "override_size": self.override_size,
            "override_density": self.override_density,
        }


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


@dataclass(frozen=True)
class AccessibilityRequirement:
    key: str
    kind: str
    tag: str | None = None
    content_description: str | None = None
    content_description_contains: str | None = None
    require_content_description: bool = False
    require_clickable: bool = False
    require_focusable: bool = False

    def to_json(self) -> dict[str, object]:
        data: dict[str, object] = {"key": self.key, "kind": self.kind}
        if self.tag is not None:
            data["tag"] = self.tag
        if self.content_description is not None:
            data["content_description"] = self.content_description
        if self.content_description_contains is not None:
            data["content_description_contains"] = self.content_description_contains
        if self.require_content_description:
            data["require_content_description"] = True
        if self.require_clickable:
            data["require_clickable"] = True
        if self.require_focusable:
            data["require_focusable"] = True
        return data


@dataclass(frozen=True)
class VerifiedCapture:
    target: str
    scenario_id: str
    screenshot_path: Path
    dimensions: PngDimensions
    profile_name: str | None = None


@dataclass(frozen=True)
class ManifestSummary:
    manifest_path: Path
    output_dir: Path
    mode: str | None
    captures: tuple[VerifiedCapture, ...]

    @property
    def capture_count(self) -> int:
        return len(self.captures)

    @property
    def target_counts(self) -> Counter[str]:
        return Counter(capture.target for capture in self.captures)


VIEWPORT_PROFILES: Mapping[str, ViewportProfile] = {
    "phone-portrait": ViewportProfile(
        name="phone-portrait",
        target="phone",
        expected_size=PHONE_EXPECTED_SIZE,
        wm_size=PHONE_EXPECTED_SIZE,
        wm_density=440,
        description="Phone portrait evidence viewport matching the default Lowband phone emulator.",
    ),
    "phone-landscape-foldable": ViewportProfile(
        name="phone-landscape-foldable",
        target="phone",
        expected_size=PHONE_LANDSCAPE_FOLDABLE_SIZE,
        wm_size=PHONE_LANDSCAPE_FOLDABLE_SIZE,
        wm_density=420,
        description="Phone landscape/foldable-ish viewport for wide compact and two-pane visual QA.",
    ),
    "tv-1080p": ViewportProfile(
        name="tv-1080p",
        target="tv",
        expected_size=TV_EXPECTED_SIZE,
        wm_size=TV_EXPECTED_SIZE,
        wm_density=320,
        description="Android TV 1080p evidence viewport matching the default Lowband TV emulator.",
    ),
    "tv-4k-scaled": ViewportProfile(
        name="tv-4k-scaled",
        target="tv",
        expected_size=TV_EXPECTED_SIZE,
        wm_size=TV_4K_SCALED_SIZE,
        wm_density=640,
        description="Android TV 4K logical viewport scaled onto the 1080p emulator framebuffer for 10-foot Theater Plate review.",
    ),
}

THEATER_PLATE_STATE_LABELS: Mapping[str, str] = {
    "bright": "Bright backdrop",
    "dark": "Dark backdrop",
    "busy": "Busy backdrop",
    "missing-backdrop": "Missing backdrop",
    "long-title": "Long title",
    "missing-artwork": "Missing artwork",
    "stale-offline": "Stale/offline",
    "recovery": "Recovery",
    "search": "Search",
    "browse": "Browse",
    "detail": "Detail",
    "rails": "Rails",
    "playback-entry": "Playback entry",
}

THEATER_PLATE_PRIMARY_ACTIONS: Mapping[str, str] = {
    "bright": "Review bright contrast",
    "dark": "Review dark contrast",
    "busy": "Reduce backdrop noise",
    "missing-backdrop": "Use fallback backdrop",
    "long-title": "Open long-title detail",
    "missing-artwork": "Use artwork fallback",
    "stale-offline": "Retry offline data",
    "recovery": "Retry",
    "search": "Search again",
    "browse": "Browse related titles",
    "detail": "Open detail actions",
    "rails": "Browse rail item",
    "playback-entry": "Resume playback",
}

LEGACY_SCENARIO_ROOT_TAGS: Mapping[str, str] = {
    "phone-home": "phone.home",
    "phone-search": "phone.search",
    "phone-browse-grid": "phone.libraries.grid",
    "phone-movie-detail": "phone.detail.movie",
    "phone-series-detail": "phone.detail.series",
    "phone-season-episode": "phone.detail.season-episode",
    "phone-playback-entry": "phone.playback-entry",
    "phone-recovery-offline-stale": "phone.recovery.offline-stale",
    "tv-home-focus": "tv.surface.home-actions",
    "tv-grid-focus": "tv.surface.grid-cards",
    "tv-detail-focus": "tv.detail",
    "tv-search-focus": "tv.search",
    "tv-recovery-focus": "tv.surface.recovery-actions",
}


def utc_now() -> str:
    return dt.datetime.now(dt.UTC).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def repo_root_from_script() -> Path:
    return Path(__file__).resolve().parents[2]


def default_gate_output_dir(mode: str) -> Path:
    return DEFAULT_OUTPUT_DIR / mode


def resolve_output_dir(
    repo_root: Path,
    output_dir: str | os.PathLike[str] | None,
    mode: str | None = None,
) -> Path:
    default = default_gate_output_dir(mode) if mode else DEFAULT_OUTPUT_DIR
    resolved = Path(output_dir) if output_dir else default
    if not resolved.is_absolute():
        resolved = repo_root / resolved
    return resolved


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


def run_gate_command(args: Sequence[str | os.PathLike[str]], *, timeout: int | float | None = None) -> None:
    string_args = [os.fspath(arg) for arg in args]
    completed = subprocess.run(string_args, check=False, timeout=timeout)
    if completed.returncode != 0:
        raise CommandError(string_args, completed.returncode, "", "")


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


def scenarios_for_gate_mode(registry: ScenarioRegistry, mode: str) -> list[Scenario]:
    if mode == "smoke":
        scenarios = []
        for scenario_id in SMOKE_SCENARIO_IDS:
            scenario = registry.by_id.get(scenario_id)
            if scenario is None:
                raise VisualQaError(f"smoke gate scenario is missing from registry: {scenario_id}")
            scenarios.append(scenario)
        targets = {scenario.target for scenario in scenarios}
        if targets != {"phone", "tv"}:
            raise VisualQaError("smoke gate must include at least one phone and one TV scenario")
        return scenarios
    if mode == "complete":
        return list(registry.scenarios)
    raise VisualQaError(f"unknown gate mode {mode!r}; expected smoke or complete")


def capture_plan_json(
    mode: str | None,
    selected: Sequence[Scenario],
    profiles_by_target: Mapping[str, Sequence[ViewportProfile]],
) -> dict[str, object]:
    capture_count = sum(len(profiles_by_target.get(scenario.target, ())) for scenario in selected)
    return {
        "mode": mode,
        "scenario_count": len(selected),
        "scenario_ids": [scenario.id for scenario in selected],
        "target_counts": dict(Counter(scenario.target for scenario in selected)),
        "viewport_profiles": {
            target: [profile.to_json() for profile in profiles]
            for target, profiles in sorted(profiles_by_target.items())
            if profiles
        },
        "capture_count": capture_count,
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


def size_to_string(size: tuple[int, int]) -> str:
    return f"{size[0]}x{size[1]}"


def expand_profile_names(raw_values: Sequence[str] | None) -> tuple[str, ...]:
    if not raw_values:
        return DEFAULT_VIEWPORT_PROFILE_NAMES
    requested = [part.strip() for raw_value in raw_values for part in raw_value.split(",") if part.strip()]
    if not requested:
        raise VisualQaError("at least one viewport profile must be selected")
    if "all" in requested:
        if len(requested) > 1:
            raise VisualQaError("viewport profile 'all' cannot be combined with explicit profiles")
        return DEFAULT_VIEWPORT_PROFILE_NAMES
    names: list[str] = []
    for name in requested:
        if name not in VIEWPORT_PROFILES:
            available = ", ".join(["all", *VIEWPORT_PROFILES.keys()])
            raise VisualQaError(f"unknown viewport profile {name!r}; available: {available}")
        if name not in names:
            names.append(name)
    return tuple(names)


def hardware_profile(config: TargetConfig) -> ViewportProfile:
    return ViewportProfile(
        name=f"hardware-{config.target}",
        target=config.target,
        expected_size=config.expected_size,
        wm_size=None,
        wm_density=None,
        description="Explicit hardware viewport; no wm size/density override is applied.",
    )


def selected_viewport_profiles(
    args: argparse.Namespace,
    configs: Mapping[str, TargetConfig],
    selected: Sequence[Scenario],
) -> dict[str, tuple[ViewportProfile, ...]]:
    selected_targets = {scenario.target for scenario in selected}
    explicit_profiles = getattr(args, "profile", None)
    if getattr(args, "hardware", False):
        if explicit_profiles:
            raise VisualQaError("--profile is not valid with --hardware; use --expected-size for explicit hardware captures")
        return {
            target: (hardware_profile(configs[target]),)
            for target in sorted(selected_targets)
        }

    profile_names = expand_profile_names(explicit_profiles)
    profiles = [VIEWPORT_PROFILES[name] for name in profile_names]
    by_target = {
        target: tuple(profile for profile in profiles if profile.target == target)
        for target in sorted(selected_targets)
    }
    missing = sorted(target for target, target_profiles in by_target.items() if not target_profiles)
    if missing:
        requested = ", ".join(profile_names)
        raise VisualQaError(f"no selected viewport profile covers target(s) {', '.join(missing)}; requested profiles: {requested}")
    return by_target


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


def wm_snapshot(adb: str, serial: str) -> WmOverrideSnapshot:
    raw_size = adb_shell(adb, serial, "wm", "size", timeout=30).stdout.strip()
    raw_density = adb_shell(adb, serial, "wm", "density", timeout=30).stdout.strip()
    size_match = re.search(r"(?im)^\s*Override size:\s*(\d+x\d+)\s*$", raw_size)
    density_match = re.search(r"(?im)^\s*Override density:\s*(\d+)\s*$", raw_density)
    return WmOverrideSnapshot(
        raw_size=raw_size,
        raw_density=raw_density,
        override_size=size_match.group(1) if size_match else None,
        override_density=density_match.group(1) if density_match else None,
    )


def apply_viewport_profile(adb: str, config: TargetConfig, profile: ViewportProfile) -> WmOverrideSnapshot:
    before = wm_snapshot(adb, config.serial)
    try:
        if profile.wm_size is not None:
            adb_shell(adb, config.serial, "wm", "size", size_to_string(profile.wm_size), timeout=30)
        if profile.wm_density is not None:
            adb_shell(adb, config.serial, "wm", "density", str(profile.wm_density), timeout=30)
    except Exception:
        restore_viewport_profile(adb, config, before)
        raise
    return before


def restore_viewport_profile(adb: str, config: TargetConfig, before: WmOverrideSnapshot) -> dict[str, object]:
    if before.override_size:
        adb_shell(adb, config.serial, "wm", "size", before.override_size, timeout=30)
    else:
        adb_shell(adb, config.serial, "wm", "size", "reset", timeout=30)
    if before.override_density:
        adb_shell(adb, config.serial, "wm", "density", before.override_density, timeout=30)
    else:
        adb_shell(adb, config.serial, "wm", "density", "reset", timeout=30)
    return {"restored_to": wm_snapshot(adb, config.serial).to_json()}


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


def focused_window_snapshot(adb: str, config: TargetConfig) -> dict[str, object]:
    output = adb_shell(adb, config.serial, "dumpsys", "window", timeout=30).stdout
    focus_lines = [line.strip() for line in output.splitlines() if "mCurrentFocus=" in line or "mFocusedApp=" in line]
    return {
        "package_foreground": any(config.package in line and VISUAL_QA_ACTIVITY in line for line in focus_lines),
        "focus_lines": focus_lines[-4:],
    }


def wait_for_visual_qa_foreground(
    adb: str,
    config: TargetConfig,
    scenario: Scenario,
    timeout_seconds: float = 8.0,
) -> dict[str, object]:
    deadline = time.monotonic() + timeout_seconds
    last_snapshot: dict[str, object] | None = None
    while True:
        last_snapshot = focused_window_snapshot(adb, config)
        if last_snapshot["package_foreground"]:
            waited_ms = int((timeout_seconds - max(deadline - time.monotonic(), 0)) * 1000)
            return {"scenario_id": scenario.id, "waited_ms": waited_ms, **last_snapshot}
        if time.monotonic() >= deadline:
            break
        time.sleep(0.25)
    raise VisualQaError(
        f"{config.package}/{VISUAL_QA_ACTIVITY} did not become foreground for {scenario.id}: {last_snapshot}"
    )


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
    attempts: list[dict[str, object]] = []
    for attempt in range(1, 3):
        result = run_command(command, timeout=60)
        output = (result.stdout + "\n" + result.stderr).strip()
        if re.search(r"(?im)^\s*(Error|Exception):", output):
            raise VisualQaError(f"am start reported an error launching {scenario.id}: {redact_text(output)}")
        attempt_record: dict[str, object] = {
            "attempt": attempt,
            "action": ACTION_VISUAL_QA,
            "extra_scenario_id": EXTRA_SCENARIO_ID,
            "component": config.component,
            "stdout": output,
        }
        try:
            attempt_record["foreground"] = wait_for_visual_qa_foreground(adb, config, scenario)
            attempts.append(attempt_record)
            return {**attempt_record, "attempts": attempts}
        except VisualQaError as exc:
            attempt_record["foreground_error"] = redact_text(str(exc))
            attempts.append(attempt_record)
            if attempt == 1:
                force_stop_package(adb, config)
                time.sleep(0.5)
    raise VisualQaError(f"visual QA activity did not become foreground after launch attempts for {scenario.id}: {attempts}")


def drive_scenario(adb: str, config: TargetConfig, scenario: Scenario) -> list[str]:
    default_tv_keys: tuple[str, ...] = ("KEYCODE_DPAD_DOWN", "KEYCODE_DPAD_RIGHT")
    keys = list(TV_DPAD_SEQUENCES.get(scenario.id, default_tv_keys if scenario.id.startswith("tv-theater-plate-") else ()))
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


def read_json_object(path: Path) -> dict[str, object]:
    try:
        parsed = json.loads(path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise VisualQaError(f"failed to read manifest {path}: {exc}") from exc
    except json.JSONDecodeError as exc:
        raise VisualQaError(f"manifest is not valid JSON: {path}: {exc}") from exc
    if not isinstance(parsed, dict):
        raise VisualQaError(f"manifest root must be a JSON object: {path}")
    return parsed


def dimension_tuple(raw: object, field_name: str) -> tuple[int, int]:
    if not isinstance(raw, dict):
        raise VisualQaError(f"capture {field_name} must be an object")
    width = raw.get("width")
    height = raw.get("height")
    if not isinstance(width, int) or not isinstance(height, int):
        raise VisualQaError(f"capture {field_name} must contain integer width and height")
    if width <= 0 or height <= 0:
        raise VisualQaError(f"capture {field_name} dimensions must be positive")
    return (width, height)


def verify_manifest(
    manifest_path: Path,
    *,
    mode: str | None = None,
    repo_root: Path | None = None,
) -> ManifestSummary:
    data = read_json_object(manifest_path)
    if data.get("status") != "passed":
        raise VisualQaError(f"manifest did not pass: {manifest_path} status={data.get('status')!r}")
    failures = data.get("failures", [])
    if failures:
        raise VisualQaError(f"manifest contains {len(failures)} failure record(s): {manifest_path}")
    captures_raw = data.get("captures")
    if not isinstance(captures_raw, list) or not captures_raw:
        raise VisualQaError(f"manifest contains no capture records: {manifest_path}")

    verified: list[VerifiedCapture] = []
    seen_ids: set[str] = set()
    for index, capture in enumerate(captures_raw):
        if not isinstance(capture, dict):
            raise VisualQaError(f"capture record {index} must be an object")
        if capture.get("status") != "passed":
            raise VisualQaError(
                f"capture record {index} did not pass: scenario={capture.get('scenario_id')!r} status={capture.get('status')!r}"
            )
        target = capture.get("target")
        profile_name = capture.get("profile")
        scenario_id = capture.get("scenario_id")
        screenshot_raw = capture.get("screenshot_path")
        if not isinstance(target, str) or target not in {"phone", "tv"}:
            raise VisualQaError(f"capture record {index} has invalid target: {target!r}")
        if profile_name is not None and not isinstance(profile_name, str):
            raise VisualQaError(f"capture record {index} has invalid profile: {profile_name!r}")
        if not isinstance(scenario_id, str) or not scenario_id:
            raise VisualQaError(f"capture record {index} has invalid scenario_id: {scenario_id!r}")
        if not isinstance(screenshot_raw, str) or not screenshot_raw:
            raise VisualQaError(f"capture record {index} has invalid screenshot_path: {screenshot_raw!r}")
        expected_size = dimension_tuple(capture.get("expected_dimensions"), "expected_dimensions")
        screenshot_path = Path(screenshot_raw)
        if not screenshot_path.is_absolute():
            screenshot_path = manifest_path.parent / screenshot_path
        dimensions = validate_png(screenshot_path, expected_size)
        manifest_dimensions = capture.get("dimensions")
        if manifest_dimensions is not None and dimension_tuple(manifest_dimensions, "dimensions") != (
            dimensions.width,
            dimensions.height,
        ):
            raise VisualQaError(f"capture dimensions disagree with PNG header for {screenshot_path}")
        verified.append(
            VerifiedCapture(
                target=target,
                scenario_id=scenario_id,
                screenshot_path=screenshot_path,
                dimensions=dimensions,
                profile_name=profile_name,
            )
        )
        seen_ids.add(scenario_id)

    if mode is not None:
        if mode not in GATE_MODES:
            raise VisualQaError(f"unknown verify mode {mode!r}; expected smoke or complete")
        registry = ScenarioRegistry.load(repo_root or repo_root_from_script())
        required_ids = {scenario.id for scenario in scenarios_for_gate_mode(registry, mode)}
        missing = sorted(required_ids - seen_ids)
        if missing:
            raise VisualQaError(f"{mode} manifest is missing required scenario(s): {', '.join(missing)}")
        if mode == "smoke":
            targets = {capture.target for capture in verified if capture.scenario_id in required_ids}
            if targets != {"phone", "tv"}:
                raise VisualQaError("smoke manifest must include at least one phone and one TV capture")
        elif mode == "complete":
            unexpected = sorted(seen_ids - required_ids)
            if unexpected:
                raise VisualQaError(
                    f"complete manifest contains scenario(s) outside the required registry: {', '.join(unexpected)}"
                )

    output_raw = data.get("output_dir")
    output_dir = Path(output_raw) if isinstance(output_raw, str) and output_raw else manifest_path.parent
    return ManifestSummary(
        manifest_path=manifest_path,
        output_dir=output_dir,
        mode=mode,
        captures=tuple(verified),
    )


def print_artifact_summary(summary: ManifestSummary) -> None:
    mode_label = f" {summary.mode}" if summary.mode else ""
    counts = summary.target_counts
    print(
        f"android-visual-qa:{mode_label} passed; captures={summary.capture_count} "
        f"phone={counts.get('phone', 0)} tv={counts.get('tv', 0)}",
        file=sys.stderr,
    )
    print(f"android-visual-qa: artifacts {summary.output_dir}", file=sys.stderr)
    print(f"android-visual-qa: manifest {summary.manifest_path}", file=sys.stderr)
    for capture in summary.captures:
        profile = f"/{capture.profile_name}" if capture.profile_name else ""
        print(
            "android-visual-qa: "
            f"{capture.target}{profile}/{capture.scenario_id} "
            f"{capture.dimensions.width}x{capture.dimensions.height} "
            f"{capture.screenshot_path}",
            file=sys.stderr,
        )


def capture_one(
    *,
    adb: str,
    config: TargetConfig,
    scenario: Scenario,
    profile: ViewportProfile,
    output_dir: Path,
    settle_ms: int,
    log_lines: int,
    prefer_nix_helper: bool,
) -> dict[str, object]:
    started_at = utc_now()
    screenshot_path = output_dir / profile.name / f"{scenario.id}.png"
    failure_log_path = output_dir / "logs" / f"{profile.name}-{scenario.id}-failure-logcat.txt"
    record: dict[str, object] = {
        "target": config.target,
        "profile": profile.name,
        "scenario_id": scenario.id,
        "serial": config.serial,
        "package_name": config.package,
        "viewport_profile": profile.to_json(),
        "expected_dimensions": {"width": profile.expected_size[0], "height": profile.expected_size[1]},
        "screenshot_path": str(screenshot_path),
        "started_at": started_at,
        "status": "running",
    }
    viewport_before: WmOverrideSnapshot | None = None

    try:
        require_serial_present(adb, config.serial)
        viewport_before = apply_viewport_profile(adb, config, profile)
        record["viewport_before"] = viewport_before.to_json()
        record["serial_metadata"] = collect_serial_metadata(adb, config)
        record["package_metadata"] = collect_package_metadata(adb, config)
        force_stop_package(adb, config)
        record["launch"] = launch_scenario(adb, config, scenario)
        record["dpad_key_events"] = drive_scenario(adb, config, scenario)
        time.sleep(settle_ms / 1000.0)
        prefer_helper_for_profile = prefer_nix_helper and profile.expected_size == config.expected_size
        record["screenshot_capture"] = capture_screenshot(adb, config, screenshot_path, prefer_helper_for_profile)
        dimensions = validate_png(screenshot_path, profile.expected_size)
        record["dimensions"] = dimensions.to_json()
        record["ended_at"] = utc_now()
        record["status"] = "passed"
    except Exception as exc:  # noqa: BLE001 - capture must emit failure artifacts for any failure.
        record["ended_at"] = utc_now()
        record["status"] = "failed"
        record["error"] = redact_text(str(exc))
        record["failure_logcat"] = capture_failure_logcat(adb, config, failure_log_path, log_lines)
    finally:
        if viewport_before is not None:
            try:
                record["viewport_restore"] = restore_viewport_profile(adb, config, viewport_before)
            except Exception as exc:  # noqa: BLE001 - never leave viewport restore failures silent.
                restore_error = redact_text(str(exc))
                record["viewport_restore_error"] = restore_error
                if record.get("status") == "passed":
                    record["status"] = "failed"
                    record["error"] = restore_error
    return record


def theater_plate_state_key(scenario_id: str) -> str | None:
    for prefix in ("phone-theater-plate-", "tv-theater-plate-"):
        if scenario_id.startswith(prefix):
            state_key = scenario_id.removeprefix(prefix)
            return state_key if state_key in THEATER_PLATE_STATE_LABELS else None
    return None


def theater_plate_tag(target: str, node_kind: str, state_key: str, leaf: str | None = None) -> str:
    parts = [target, "theater-plate"]
    if node_kind != "root":
        parts.append(node_kind)
    parts.append(state_key)
    if leaf:
        parts.append(leaf)
    return ".".join(parts)


def legacy_accessibility_requirements(scenario: Scenario) -> list[AccessibilityRequirement]:
    requirements: list[AccessibilityRequirement] = []
    root_tag = LEGACY_SCENARIO_ROOT_TAGS.get(scenario.id)
    if root_tag:
        requirements.append(AccessibilityRequirement(key="root-tag", kind="tag", tag=root_tag))

    if scenario.id == "phone-search":
        requirements.extend(
            [
                AccessibilityRequirement(key="search-field", kind="tag", tag="phone.search.field"),
                AccessibilityRequirement(key="search-actions", kind="tag", tag="phone.search.actions"),
                AccessibilityRequirement(key="search-results", kind="tag", tag="phone.search.results"),
            ]
        )
    elif scenario.id == "phone-browse-grid":
        requirements.extend(
            [
                AccessibilityRequirement(key="library-grid", kind="tag", tag="phone.libraries.grid"),
                AccessibilityRequirement(
                    key="library-recovery-status",
                    kind="status",
                    tag="phone.library.recovery",
                    require_content_description=True,
                ),
                AccessibilityRequirement(
                    key="reset-connection-action",
                    kind="action",
                    content_description="Reset connection",
                    require_clickable=True,
                ),
            ]
        )
    elif scenario.id == "phone-recovery-offline-stale":
        for label in ("Retry", "Sign out", "Change server", "Reset connection", "Diagnostics / Export diagnostics"):
            requirements.append(
                AccessibilityRequirement(
                    key=f"recovery-{label.lower().replace(' ', '-')}",
                    kind="recovery-action",
                    content_description=label,
                    require_clickable=True,
                )
            )
    elif scenario.id.startswith("tv-"):
        surface = root_tag
        if surface:
            requirements.append(
                AccessibilityRequirement(key="tv-focus-surface", kind="focus", tag=surface, require_content_description=False)
            )
        if scenario.id == "tv-grid-focus":
            requirements.append(
                AccessibilityRequirement(
                    key="tv-media-card",
                    kind="media",
                    tag="tv.poster.grid-cards.movie-aurora-station",
                    content_description_contains="Aurora Station",
                    require_content_description=True,
                    require_focusable=True,
                )
            )
        else:
            action_tag = {
                "tv-home-focus": "tv.action.home-actions.search",
                "tv-detail-focus": "tv.action.detail-actions.play",
                "tv-search-focus": "tv.action.search-results.field",
                "tv-recovery-focus": "tv.action.recovery-actions.retry",
            }.get(scenario.id)
            if action_tag:
                requirements.append(
                    AccessibilityRequirement(
                        key="tv-primary-action",
                        kind="action",
                        tag=action_tag,
                        require_content_description=True,
                        require_focusable=True,
                    )
                )
    return requirements


def theater_plate_accessibility_requirements(scenario: Scenario, state_key: str) -> list[AccessibilityRequirement]:
    target = scenario.target
    label = THEATER_PLATE_STATE_LABELS[state_key]
    primary_action = THEATER_PLATE_PRIMARY_ACTIONS[state_key]
    focus_required = target == "tv"
    requirements = [
        AccessibilityRequirement(
            key="theater-root",
            kind="tag",
            tag=theater_plate_tag(target, "root", state_key),
            require_content_description=True,
        ),
        AccessibilityRequirement(
            key="theater-status",
            kind="status",
            tag=theater_plate_tag(target, "status", state_key),
            content_description_contains=label,
            require_content_description=True,
        ),
        AccessibilityRequirement(
            key="theater-primary-action",
            kind="action",
            tag=theater_plate_tag(target, "action", state_key, "primary"),
            content_description=primary_action,
            require_clickable=target == "phone",
            require_focusable=focus_required,
        ),
        AccessibilityRequirement(
            key="theater-media-hero",
            kind="media",
            tag=theater_plate_tag(target, "media", state_key, "hero"),
            content_description_contains="Theater Plate media",
            require_content_description=True,
            require_focusable=focus_required,
        ),
        AccessibilityRequirement(
            key="theater-rail",
            kind="media-rail",
            tag=theater_plate_tag(target, "rail", state_key, "primary"),
            require_content_description=True,
        ),
    ]
    if target == "phone" and state_key in {"stale-offline", "recovery"}:
        for label in ("Retry", "Change server", "Reset connection", "Diagnostics / Export diagnostics"):
            requirements.append(
                AccessibilityRequirement(
                    key=f"theater-recovery-{label.lower().replace(' ', '-')}",
                    kind="recovery-action",
                    content_description=label,
                    require_clickable=True,
                )
            )
    if state_key == "search":
        requirements.append(
            AccessibilityRequirement(
                key="theater-search-field",
                kind="action",
                tag=theater_plate_tag(target, "search", state_key, "field"),
                content_description="Search Theater Plate",
                require_clickable=target == "phone",
                require_focusable=focus_required,
            )
        )
    if state_key == "playback-entry":
        requirements.append(
            AccessibilityRequirement(
                key="theater-playback-entry",
                kind="action",
                content_description="Resume playback",
                require_clickable=target == "phone",
                require_focusable=focus_required,
            )
        )
    return requirements


def accessibility_requirements_for_scenario(scenario: Scenario) -> list[AccessibilityRequirement]:
    state_key = theater_plate_state_key(scenario.id)
    if state_key is not None:
        return theater_plate_accessibility_requirements(scenario, state_key)
    return legacy_accessibility_requirements(scenario)


def dump_accessibility_xml(adb: str, config: TargetConfig) -> str:
    remote_path = f"/sdcard/ferrex-visual-qa-accessibility-{os.getpid()}.xml"
    try:
        adb_shell(adb, config.serial, "uiautomator", "dump", "--compressed", remote_path, timeout=90)
        return adb_shell(adb, config.serial, "cat", remote_path, timeout=30).stdout
    finally:
        adb_shell(adb, config.serial, "rm", "-f", remote_path, check=False, timeout=30)


def drive_accessibility_reachability_step(adb: str, config: TargetConfig, profile: ViewportProfile) -> None:
    width, height = profile.expected_size
    if config.target == "tv":
        for _ in range(5):
            adb_shell(adb, config.serial, "input", "keyevent", "KEYCODE_DPAD_DOWN", timeout=30)
            time.sleep(0.05)
    adb_shell(
        adb,
        config.serial,
        "input",
        "swipe",
        str(width // 2),
        str(int(height * 0.82)),
        str(width // 2),
        str(int(height * 0.25)),
        "250",
        timeout=30,
    )
    time.sleep(0.25)


def accessibility_dump_path(base_path: Path, step: int) -> Path:
    if step == 0:
        return base_path
    return base_path.with_name(f"{base_path.stem}-step-{step}{base_path.suffix}")


def parse_accessibility_nodes(xml_text: str) -> list[dict[str, str]]:
    try:
        root = ET.fromstring(xml_text)
    except ET.ParseError as exc:
        raise VisualQaError(f"accessibility dump is not valid XML: {exc}") from exc

    records: list[dict[str, str]] = []

    def walk(element: ET.Element, ancestors: Sequence[Mapping[str, str]]) -> list[str]:
        attrs = dict(element.attrib)
        child_descs: list[str] = []
        for child in element:
            if child.tag == "node":
                child_descs.extend(walk(child, (*ancestors, attrs)))
        own_desc = attrs.get("content-desc", "")
        subtree_descs = [desc for desc in (own_desc, *child_descs) if desc]
        if element.tag == "node":
            attrs["_subtree_content_descs"] = "\n".join(subtree_descs)
            attrs["_ancestor_clickable"] = "true" if attrs.get("clickable") == "true" or any(
                ancestor.get("clickable") == "true" for ancestor in ancestors
            ) else "false"
            attrs["_ancestor_focusable"] = "true" if attrs.get("focusable") == "true" or any(
                ancestor.get("focusable") == "true" for ancestor in ancestors
            ) else "false"
            records.append(attrs)
        return subtree_descs

    for child in root:
        if child.tag == "node":
            walk(child, ())
    return records


def node_has_tag(node: Mapping[str, str], tag: str) -> bool:
    resource_id = node.get("resource-id", "")
    return resource_id == tag or resource_id.endswith(f":id/{tag}") or resource_id.endswith(tag)


def node_matches_requirement(node: Mapping[str, str], requirement: AccessibilityRequirement) -> bool:
    if requirement.tag is not None and not node_has_tag(node, requirement.tag):
        return False
    content_description = node.get("content-desc", "")
    subtree_descriptions = node.get("_subtree_content_descs", content_description)
    if requirement.content_description is not None:
        if requirement.tag is not None:
            if requirement.content_description not in subtree_descriptions.splitlines():
                return False
        elif content_description != requirement.content_description:
            return False
    if requirement.content_description_contains is not None:
        searchable_description = subtree_descriptions if requirement.tag is not None else content_description
        if requirement.content_description_contains not in searchable_description:
            return False
    if requirement.require_content_description and not subtree_descriptions:
        return False
    if requirement.require_clickable and node.get("clickable") != "true" and node.get("_ancestor_clickable") != "true":
        return False
    if requirement.require_focusable and node.get("focusable") != "true" and node.get("_ancestor_focusable") != "true":
        return False
    return True


def verify_accessibility_requirements(
    nodes: Sequence[Mapping[str, str]],
    requirements: Sequence[AccessibilityRequirement],
) -> list[dict[str, object]]:
    checks: list[dict[str, object]] = []
    for requirement in requirements:
        matching = [node for node in nodes if node_matches_requirement(node, requirement)]
        checks.append(
            {
                "requirement": requirement.to_json(),
                "status": "passed" if matching else "failed",
                "matching_nodes": len(matching),
            }
        )
    return checks


def accessibility_one(
    *,
    adb: str,
    config: TargetConfig,
    scenario: Scenario,
    profile: ViewportProfile,
    output_dir: Path,
    settle_ms: int,
    log_lines: int,
) -> dict[str, object]:
    started_at = utc_now()
    dump_path = output_dir / "accessibility" / profile.name / f"{scenario.id}.xml"
    failure_log_path = output_dir / "logs" / f"{profile.name}-{scenario.id}-accessibility-logcat.txt"
    requirements = accessibility_requirements_for_scenario(scenario)
    record: dict[str, object] = {
        "target": config.target,
        "profile": profile.name,
        "scenario_id": scenario.id,
        "serial": config.serial,
        "package_name": config.package,
        "viewport_profile": profile.to_json(),
        "expected_dimensions": {"width": profile.expected_size[0], "height": profile.expected_size[1]},
        "requirements": [requirement.to_json() for requirement in requirements],
        "dump_path": str(dump_path),
        "started_at": started_at,
        "status": "running",
    }
    viewport_before: WmOverrideSnapshot | None = None
    try:
        if not requirements:
            raise VisualQaError(f"no accessibility requirements registered for scenario {scenario.id}")
        require_serial_present(adb, config.serial)
        viewport_before = apply_viewport_profile(adb, config, profile)
        record["viewport_before"] = viewport_before.to_json()
        record["serial_metadata"] = collect_serial_metadata(adb, config)
        record["package_metadata"] = collect_package_metadata(adb, config)
        force_stop_package(adb, config)
        record["launch"] = launch_scenario(adb, config, scenario)
        record["dpad_key_events"] = drive_scenario(adb, config, scenario)
        time.sleep(settle_ms / 1000.0)
        dump_path.parent.mkdir(parents=True, exist_ok=True)
        nodes: list[dict[str, str]] = []
        dump_paths: list[str] = []
        for step in range(ACCESSIBILITY_REACHABILITY_STEPS):
            xml_text = dump_accessibility_xml(adb, config)
            step_path = accessibility_dump_path(dump_path, step)
            step_path.write_text(redact_text(xml_text), encoding="utf-8")
            dump_paths.append(str(step_path))
            nodes.extend(parse_accessibility_nodes(xml_text))
            if step < ACCESSIBILITY_REACHABILITY_STEPS - 1:
                drive_accessibility_reachability_step(adb, config, profile)
        record["dump_paths"] = dump_paths
        checks = verify_accessibility_requirements(nodes, requirements)
        failures = [check for check in checks if check["status"] != "passed"]
        record["node_count"] = len(nodes)
        record["checks"] = checks
        record["ended_at"] = utc_now()
        if failures:
            missing = ", ".join(str(check["requirement"]["key"]) for check in failures)
            raise VisualQaError(f"missing accessibility requirement(s): {missing}")
        record["status"] = "passed"
    except Exception as exc:  # noqa: BLE001 - accessibility gate must emit diagnostics.
        record["ended_at"] = utc_now()
        record["status"] = "failed"
        record["error"] = redact_text(str(exc))
        record["failure_logcat"] = capture_failure_logcat(adb, config, failure_log_path, log_lines)
    finally:
        if viewport_before is not None:
            try:
                record["viewport_restore"] = restore_viewport_profile(adb, config, viewport_before)
            except Exception as exc:  # noqa: BLE001
                restore_error = redact_text(str(exc))
                record["viewport_restore_error"] = restore_error
                if record.get("status") == "passed":
                    record["status"] = "failed"
                    record["error"] = restore_error
    return record


def run_accessibility(args: argparse.Namespace) -> int:
    repo_root = repo_root_from_script()
    registry = ScenarioRegistry.load(repo_root)
    selected = registry.select(args.target, args.scenario)
    output_dir = resolve_output_dir(repo_root, args.output_dir)
    configs = target_configs(repo_root, args)
    profiles_by_target = selected_viewport_profiles(args, configs, selected)
    adb = resolve_executable(args.adb)
    output_dir.mkdir(parents=True, exist_ok=True)

    manifest: dict[str, object] = {
        "schema_version": 1,
        "command": "android-visual-qa accessibility",
        "argv": getattr(args, "effective_argv", sys.argv[1:]),
        "started_at": utc_now(),
        "output_dir": str(output_dir),
        "hardware_confirmation": bool(args.hardware),
        "registry": registry.to_json(),
        "accessibility_plan": capture_plan_json(None, selected, profiles_by_target),
        "command_versions": collect_command_versions(adb, Path(__file__).resolve()),
        "checks": [],
        "failures": [],
    }

    records: list[dict[str, object]] = []
    for scenario in selected:
        config = configs[scenario.target]
        for profile in profiles_by_target[scenario.target]:
            print(
                f"android-visual-qa: accessibility {scenario.id} on {scenario.target}/{profile.name} ({config.serial})",
                file=sys.stderr,
            )
            record = accessibility_one(
                adb=adb,
                config=config,
                scenario=scenario,
                profile=profile,
                output_dir=output_dir,
                settle_ms=args.settle_ms,
                log_lines=args.log_lines,
            )
            records.append(record)
            if record["status"] == "passed":
                print(f"android-visual-qa: accessibility passed {profile.name}/{scenario.id}", file=sys.stderr)
            else:
                print(
                    f"android-visual-qa: accessibility FAILED {profile.name}/{scenario.id}: {record['error']}",
                    file=sys.stderr,
                )

    failures = [record for record in records if record.get("status") != "passed"]
    manifest["checks"] = records
    manifest["failures"] = failures
    manifest["ended_at"] = utc_now()
    manifest["status"] = "failed" if failures else "passed"
    manifest_path = output_dir / "accessibility-manifest.json"
    write_json(manifest_path, manifest)
    print(f"android-visual-qa: accessibility manifest {manifest_path}", file=sys.stderr)
    return 1 if failures else 0


def run_capture_plan(
    *,
    args: argparse.Namespace,
    repo_root: Path,
    registry: ScenarioRegistry,
    selected: Sequence[Scenario],
    output_dir: Path,
    command_name: str,
    mode: str | None = None,
) -> int:
    configs = target_configs(repo_root, args)
    profiles_by_target = selected_viewport_profiles(args, configs, selected)
    adb = resolve_executable(args.adb)
    output_dir.mkdir(parents=True, exist_ok=True)

    manifest: dict[str, object] = {
        "schema_version": 1,
        "command": command_name,
        "argv": getattr(args, "effective_argv", sys.argv[1:]),
        "started_at": utc_now(),
        "output_dir": str(output_dir),
        "hardware_confirmation": bool(args.hardware),
        "registry": registry.to_json(),
        "capture_plan": capture_plan_json(mode, selected, profiles_by_target),
        "command_versions": collect_command_versions(adb, Path(__file__).resolve()),
        "captures": [],
        "failures": [],
    }

    captures: list[dict[str, object]] = []
    for scenario in selected:
        config = configs[scenario.target]
        for profile in profiles_by_target[scenario.target]:
            print(
                f"android-visual-qa: capture {scenario.id} on {scenario.target}/{profile.name} ({config.serial})",
                file=sys.stderr,
            )
            record = capture_one(
                adb=adb,
                config=config,
                scenario=scenario,
                profile=profile,
                output_dir=output_dir,
                settle_ms=args.settle_ms,
                log_lines=args.log_lines,
                prefer_nix_helper=not args.no_nix_screenshot,
            )
            captures.append(record)
            if record["status"] == "passed":
                print(f"android-visual-qa: wrote {record['screenshot_path']}", file=sys.stderr)
            else:
                print(f"android-visual-qa: FAILED {profile.name}/{scenario.id}: {record['error']}", file=sys.stderr)

    failures = [record for record in captures if record.get("status") != "passed"]
    manifest["captures"] = captures
    manifest["failures"] = failures
    manifest["ended_at"] = utc_now()
    manifest["status"] = "failed" if failures else "passed"
    manifest_path = output_dir / "manifest.json"
    write_json(manifest_path, manifest)
    print(f"android-visual-qa: manifest {manifest_path}", file=sys.stderr)
    return 1 if failures else 0


def run_capture(args: argparse.Namespace) -> int:
    repo_root = repo_root_from_script()
    registry = ScenarioRegistry.load(repo_root)
    selected = registry.select(args.target, args.scenario)
    output_dir = resolve_output_dir(repo_root, args.output_dir)
    return run_capture_plan(
        args=args,
        repo_root=repo_root,
        registry=registry,
        selected=selected,
        output_dir=output_dir,
        command_name="android-visual-qa capture",
    )


def run_gate(args: argparse.Namespace) -> int:
    repo_root = repo_root_from_script()
    registry = ScenarioRegistry.load(repo_root)
    selected = scenarios_for_gate_mode(registry, args.mode)
    output_dir = resolve_output_dir(repo_root, args.output_dir, args.mode)
    qa_script = repo_root / "scripts/qa/android-emulator-qa.sh"

    steps: tuple[tuple[str, tuple[str | Path, ...]], ...] = (
        ("build", (qa_script, "build")),
        ("start", (qa_script, "start")),
        ("doctor", (qa_script, "doctor")),
        ("install", (qa_script, "install", "all")),
    )
    for label, command in steps:
        print(f"android-visual-qa: step {label}", file=sys.stderr)
        run_gate_command(command)

    capture_args = argparse.Namespace(
        target="all",
        scenario="all",
        output_dir=str(output_dir),
        settle_ms=args.settle_ms,
        log_lines=args.log_lines,
        adb=args.adb,
        no_nix_screenshot=args.no_nix_screenshot,
        hardware=False,
        hardware_serial=None,
        expected_size=None,
        profile=getattr(args, "profile", None),
        effective_argv=getattr(args, "effective_argv", sys.argv[1:]),
    )
    print(f"android-visual-qa: step capture ({args.mode})", file=sys.stderr)
    capture_status = run_capture_plan(
        args=capture_args,
        repo_root=repo_root,
        registry=registry,
        selected=selected,
        output_dir=output_dir,
        command_name="android-visual-qa gate",
        mode=args.mode,
    )
    if capture_status != 0:
        return capture_status

    print("android-visual-qa: step verify", file=sys.stderr)
    run_gate_command((qa_script, "check", "all"))
    summary = verify_manifest(output_dir / "manifest.json", mode=args.mode, repo_root=repo_root)
    print_artifact_summary(summary)
    return 0


def run_verify(args: argparse.Namespace) -> int:
    repo_root = repo_root_from_script()
    output_dir = resolve_output_dir(repo_root, args.output_dir, args.mode)
    manifest_path = Path(args.manifest) if args.manifest else output_dir / "manifest.json"
    if not manifest_path.is_absolute():
        manifest_path = repo_root / manifest_path
    summary = verify_manifest(manifest_path, mode=args.mode, repo_root=repo_root)
    print_artifact_summary(summary)
    return 0


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
        description=(
            "Run the Ferrex Android visual QA gate or capture debug scenario screenshots with metadata. "
            "No arguments defaults to the smoke gate."
        ),
    )
    subparsers = parser.add_subparsers(dest="command", required=True)

    gate = subparsers.add_parser(
        "gate",
        help="Run build/start/doctor/install/capture/verify acceptance gate (default mode: smoke)",
    )
    gate.add_argument(
        "--mode",
        choices=GATE_MODES,
        default="smoke",
        help="smoke captures phone-home and tv-home-focus; complete captures the full required registry",
    )
    gate.add_argument(
        "--output-dir",
        help="Output directory for PNGs and manifest; defaults to target/android-visual-qa/<mode>",
    )
    gate.add_argument("--settle-ms", type=positive_int, default=DEFAULT_SETTLE_MS)
    gate.add_argument("--log-lines", type=positive_int, default=DEFAULT_LOG_LINES)
    gate.add_argument(
        "--adb",
        default=os.environ.get("ADB", "adb"),
        help="adb executable to use for screenshot capture",
    )
    gate.add_argument(
        "--no-nix-screenshot",
        action="store_true",
        help="Do not use ferrex-android-screenshot-phone/tv helpers; always use adb -s exec-out screencap",
    )
    gate.add_argument(
        "--profile",
        action="append",
        help="Viewport profile(s) to capture: all, phone-portrait, phone-landscape-foldable, tv-1080p, tv-4k-scaled. May be repeated or comma-separated; defaults to all.",
    )
    gate.set_defaults(func=run_gate, hardware=False)

    verify = subparsers.add_parser("verify", help="Verify a captured manifest and print a concise artifact summary")
    verify.add_argument("--mode", choices=GATE_MODES, help="Enforce the smoke or complete acceptance matrix")
    verify.add_argument("--manifest", help="Manifest path; defaults to <output-dir>/manifest.json")
    verify.add_argument(
        "--output-dir",
        help="Output directory containing manifest; defaults to target/android-visual-qa[/<mode>]",
    )
    verify.set_defaults(func=run_verify)

    list_parser = subparsers.add_parser("list", help="List scenario IDs from the debug registry")
    list_parser.add_argument("--target", choices=("phone", "mobile", "tv", "all"), default="all")
    list_parser.add_argument("--json", action="store_true", help="Print JSON instead of tab-separated text")
    list_parser.set_defaults(func=run_list)

    accessibility = subparsers.add_parser(
        "accessibility",
        help="Launch scenarios and verify host-side UI Automator tags, labels, actions, focus, and media semantics",
    )
    accessibility.add_argument("--target", choices=("phone", "mobile", "tv", "all"), required=True)
    accessibility.add_argument(
        "--scenario",
        required=True,
        help="Scenario ID from android-visual-qa list, or 'all' for the required target matrix",
    )
    accessibility.add_argument(
        "--output-dir",
        default=str(DEFAULT_OUTPUT_DIR),
        help="Output directory for accessibility dumps and manifest",
    )
    accessibility.add_argument("--settle-ms", type=positive_int, default=DEFAULT_SETTLE_MS)
    accessibility.add_argument("--log-lines", type=positive_int, default=DEFAULT_LOG_LINES)
    accessibility.add_argument("--adb", default=os.environ.get("ADB", "adb"), help="adb executable to use")
    accessibility.add_argument(
        "--profile",
        action="append",
        help="Viewport profile(s) to check: all, phone-portrait, phone-landscape-foldable, tv-1080p, tv-4k-scaled. May be repeated or comma-separated; defaults to all.",
    )
    accessibility.add_argument(
        "--hardware",
        action="store_true",
        help="Use an explicitly supplied hardware serial instead of the emulator serial for the requested target",
    )
    accessibility.add_argument(
        "--hardware-serial",
        help="Hardware ADB serial; equivalent env: FERREX_ANDROID_HARDWARE_SERIAL. No default is provided.",
    )
    accessibility.add_argument(
        "--expected-size",
        help="Expected hardware dimensions only with --hardware, e.g. 1080x2400; equivalent env: FERREX_ANDROID_HARDWARE_EXPECTED_SIZE",
    )
    accessibility.set_defaults(func=run_accessibility, no_nix_screenshot=True)

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
        "--profile",
        action="append",
        help="Viewport profile(s) to capture: all, phone-portrait, phone-landscape-foldable, tv-1080p, tv-4k-scaled. May be repeated or comma-separated; defaults to all.",
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


def effective_argv(argv: Sequence[str] | None) -> list[str]:
    provided = list(sys.argv[1:] if argv is None else argv)
    if not provided:
        return ["gate", "--mode", "smoke"]
    if len(provided) == 1 and provided[0] in GATE_MODES:
        return ["gate", "--mode", provided[0]]
    return provided


def main(argv: Sequence[str] | None = None) -> int:
    parser = build_parser()
    parsed_argv = effective_argv(argv)
    args = parser.parse_args(parsed_argv)
    args.effective_argv = parsed_argv
    try:
        return args.func(args)
    except VisualQaError as exc:
        parser.exit(2, f"android-visual-qa: ERROR: {redact_text(str(exc))}\n")


if __name__ == "__main__":
    raise SystemExit(main())
