#!/usr/bin/env python3
"""Host-side Android visual QA scenario screenshot runner.

The runner launches debug-only Ferrex visual QA scenarios on explicit ADB
serials, captures PNG screenshots, validates dimensions, and writes a run
manifest plus redacted failure logcat snippets under target/android-visual-qa.
"""

from __future__ import annotations

import argparse
from collections import Counter
from concurrent.futures import ThreadPoolExecutor, as_completed
from contextlib import contextmanager
from contextvars import ContextVar
import copy
import datetime as dt
import hashlib
import json
import math
import os
import re
import shlex
import shutil
import struct
import subprocess
import sys
import time
import xml.etree.ElementTree as ET
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Iterator, Mapping, Sequence

ACTION_VISUAL_QA = "com.ferrex.android.action.VISUAL_QA"
EXTRA_SCENARIO_ID = "com.ferrex.android.extra.QA_SCENARIO_ID"
VISUAL_QA_ACTIVITY = "com.ferrex.android.qa.FerrexVisualQaActivity"
DEFAULT_OUTPUT_DIR = Path("target/android-visual-qa")
DEFAULT_SETTLE_MS = 1500
DEFAULT_WARM_SETTLE_MS = 100
DEFAULT_LOG_LINES = 240
DEFAULT_ACCESSIBILITY_MAX_STEPS = 16
VALIDATED_BATCHED_ACCESSIBILITY_DUMP_SDK: Mapping[str, str] = {"phone": "35", "tv": "34"}
ACCESSIBILITY_DUMP_ATTEMPTS = 8
ACCESSIBILITY_DUMP_RELAUNCH_ATTEMPTS = 2
ACCESSIBILITY_DUMP_RETRY_DELAY_SECONDS = 0.75
ACCESSIBILITY_NO_PROGRESS_STOP_STREAK = 4
TV_ACCESSIBILITY_SETTLE_MS = 8000
VIEWPORT_SETTLE_TIMEOUT_SECONDS = 20.0
VIEWPORT_SETTLE_RETRY_DELAY_SECONDS = 0.25
GATE_MODES = ("smoke", "complete")
SCREENSHOT_MODE_FAST = "fast"
SCREENSHOT_MODE_HELPER_COMPATIBLE = "helper-compatible"
SCREENSHOT_MODES = (SCREENSHOT_MODE_FAST, SCREENSHOT_MODE_HELPER_COMPATIBLE)
SCREENSHOT_VALIDATION_ATTEMPTS = 3
SCREENSHOT_VALIDATION_RETRY_DELAY_SECONDS = 0.5
SMOKE_SCENARIO_IDS = ("phone-home", "tv-home-focus")
WARM_FRESHNESS_STEP_NAME = "warm_freshness"
WARM_FRESHNESS_POLICY = "conservative"
WARM_PREPARATION_STEP_NAMES = ("build", "start", "doctor", "install")
WARM_PREPARATION_PHASE = "preparation"

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
REQUIRED_VIEWPORT_PROFILE_NAMES_BY_TARGET: Mapping[str, tuple[str, ...]] = {
    "phone": ("phone-portrait", "phone-landscape-foldable"),
    "tv": ("tv-1080p", "tv-4k-scaled"),
}

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
    accelerometer_rotation: str | None = None
    user_rotation: str | None = None
    raw_user_rotation: str | None = None
    user_rotation_mode: str | None = None

    def to_json(self) -> dict[str, object]:
        data: dict[str, object] = {
            "raw_size": self.raw_size,
            "raw_density": self.raw_density,
            "override_size": self.override_size,
            "override_density": self.override_density,
            "accelerometer_rotation": self.accelerometer_rotation,
            "user_rotation": self.user_rotation,
        }
        if self.raw_user_rotation is not None:
            data["raw_user_rotation"] = self.raw_user_rotation
            data["user_rotation_mode"] = self.user_rotation_mode
        return data


@dataclass(frozen=True)
class ViewportApplyEvidence:
    before: WmOverrideSnapshot
    after: WmOverrideSnapshot
    actions: tuple[dict[str, str], ...]

    @property
    def skipped(self) -> bool:
        return not self.actions

    def to_json(self) -> dict[str, object]:
        return {
            "before": self.before.to_json(),
            "after": self.after.to_json(),
            "actions": list(self.actions),
            "skipped": self.skipped,
        }


@dataclass(frozen=True)
class CacheLookup:
    value: dict[str, object]
    provenance: dict[str, object]


@dataclass(frozen=True)
class TargetViewportPlan:
    target: str
    profile: ViewportProfile
    scenarios: tuple[Scenario, ...]


@dataclass(frozen=True)
class TargetRunResult:
    target: str
    records: tuple[dict[str, object], ...]
    viewport_events: tuple[dict[str, object], ...]
    cache_summary: dict[str, object]
    worker_timing: dict[str, object]
    screenshot_method_comparison: dict[str, object] | None = None


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


ADB_OPTIONS_WITH_VALUE = frozenset(("-s", "-t", "-H", "-P", "-L"))


def duration_ms_since(start: float, clock: Callable[[], float] | None = None) -> int:
    now = (clock or time.monotonic)()
    return max(0, int(round((now - start) * 1000)))


def sanitized_category_token(value: str) -> str:
    token = re.sub(r"[^A-Za-z0-9_.-]+", "-", value.strip())
    return (token[:48] or "unknown").strip("-") or "unknown"


def adb_command_category(args: Sequence[str | os.PathLike[str]]) -> str | None:
    string_args = [os.fspath(arg) for arg in args]
    if not string_args or Path(string_args[0]).name not in {"adb", "adb.exe"}:
        return None

    index = 1
    while index < len(string_args):
        arg = string_args[index]
        if arg in ADB_OPTIONS_WITH_VALUE:
            index += 2
        elif arg.startswith("-"):
            index += 1
        else:
            break

    if index >= len(string_args):
        return "adb"

    verb = sanitized_category_token(string_args[index])
    if verb in {"shell", "exec-out"} and index + 1 < len(string_args):
        return f"{verb}:{sanitized_category_token(string_args[index + 1])}"
    return verb


def command_category(args: Sequence[str | os.PathLike[str]]) -> str:
    adb_category = adb_command_category(args)
    if adb_category is not None:
        return adb_category
    string_args = [os.fspath(arg) for arg in args]
    if not string_args:
        return "unknown"
    return f"helper:{sanitized_category_token(Path(string_args[0]).name)}"


def effective_screenshot_mode(args: argparse.Namespace) -> str:
    mode = getattr(args, "screenshot_mode", None) or SCREENSHOT_MODE_FAST
    if mode not in SCREENSHOT_MODES:
        expected = ", ".join(SCREENSHOT_MODES)
        raise VisualQaError(f"unknown screenshot mode {mode!r}; expected one of: {expected}")
    if getattr(args, "no_nix_screenshot", False) and mode != SCREENSHOT_MODE_FAST:
        raise VisualQaError("--no-nix-screenshot cannot be combined with --screenshot-mode helper-compatible")
    return mode


def screenshot_manifest_config(args: argparse.Namespace) -> dict[str, object]:
    mode = effective_screenshot_mode(args)
    return {
        "mode": mode,
        "default_mode": SCREENSHOT_MODE_FAST,
        "helper_compatibility_mode": mode == SCREENSHOT_MODE_HELPER_COMPATIBLE,
        "no_nix_screenshot_alias": bool(getattr(args, "no_nix_screenshot", False)),
    }


class TimingRecorder:
    def __init__(self, clock: Callable[[], float] | None = None):
        self._clock = clock or time.monotonic
        self._timings_ms: Counter[str] = Counter()
        self._adb_categories: dict[str, dict[str, int]] = {}

    def now(self) -> float:
        return self._clock()

    def elapsed_ms_since(self, start: float) -> int:
        return duration_ms_since(start, self._clock)

    def add_duration(self, name: str, duration_ms: int) -> None:
        self._timings_ms[name] += max(0, int(duration_ms))

    @contextmanager
    def step(self, name: str) -> Iterator[None]:
        started = self.now()
        try:
            yield
        finally:
            self.add_duration(name, self.elapsed_ms_since(started))

    def record_adb_command(self, args: Sequence[str | os.PathLike[str]], duration_ms: int) -> None:
        category = adb_command_category(args)
        if category is None:
            return
        bucket = self._adb_categories.setdefault(category, {"count": 0, "duration_ms": 0})
        bucket["count"] += 1
        bucket["duration_ms"] += max(0, int(duration_ms))

    def timings_json(self, total_ms: int | None = None) -> dict[str, int]:
        data = {key: int(value) for key, value in sorted(self._timings_ms.items())}
        if total_ms is not None:
            data["total"] = max(0, int(total_ms))
        return data

    def adb_command_summary(self) -> dict[str, object]:
        categories = {
            category: {"count": values["count"], "duration_ms": values["duration_ms"]}
            for category, values in sorted(self._adb_categories.items())
        }
        return {
            "total_count": sum(values["count"] for values in categories.values()),
            "total_duration_ms": sum(values["duration_ms"] for values in categories.values()),
            "categories": categories,
        }


_CURRENT_TIMING_RECORDER: ContextVar[TimingRecorder | None] = ContextVar(
    "android_visual_qa_timing_recorder",
    default=None,
)


@contextmanager
def active_timing_recorder(recorder: TimingRecorder) -> Iterator[None]:
    token = _CURRENT_TIMING_RECORDER.set(recorder)
    try:
        yield
    finally:
        _CURRENT_TIMING_RECORDER.reset(token)


def current_timing_recorder() -> TimingRecorder | None:
    return _CURRENT_TIMING_RECORDER.get()


@contextmanager
def current_timing_step(name: str) -> Iterator[None]:
    recorder = current_timing_recorder()
    if recorder is None:
        yield
    else:
        with recorder.step(name):
            yield


def record_current_adb_timing(args: Sequence[str | os.PathLike[str]], started: float, recorder: TimingRecorder | None) -> None:
    if recorder is not None:
        recorder.record_adb_command(args, recorder.elapsed_ms_since(started))


def non_negative_int(value: object) -> int | None:
    if isinstance(value, bool):
        return None
    if isinstance(value, int):
        return max(0, value)
    if isinstance(value, float):
        return max(0, int(round(value)))
    return None


def duration_distribution(values: Sequence[int]) -> dict[str, int]:
    sorted_values = sorted(max(0, int(value)) for value in values)
    if not sorted_values:
        return {"count": 0, "total": 0, "p50": 0, "p95": 0, "max": 0}

    def percentile(percent: int) -> int:
        rank = max(0, math.ceil((percent / 100) * len(sorted_values)) - 1)
        return sorted_values[min(rank, len(sorted_values) - 1)]

    return {
        "count": len(sorted_values),
        "total": sum(sorted_values),
        "p50": percentile(50),
        "p95": percentile(95),
        "max": sorted_values[-1],
    }


def record_total_ms(record: Mapping[str, object]) -> int | None:
    timings = record.get("timings_ms")
    if not isinstance(timings, Mapping):
        return None
    return non_negative_int(timings.get("total"))


def record_method_name(record: Mapping[str, object]) -> str:
    screenshot_capture = record.get("screenshot_capture")
    if isinstance(screenshot_capture, Mapping):
        method = screenshot_capture.get("method")
        if isinstance(method, str) and method:
            return method
    if record.get("dump_paths") is not None or record.get("requirements") is not None:
        return "uiautomator-accessibility"
    return "unknown"


def aggregate_adb_summaries(records: Sequence[Mapping[str, object]]) -> dict[str, object]:
    categories: dict[str, dict[str, int]] = {}
    for record in records:
        summary = record.get("adb_command_summary")
        if not isinstance(summary, Mapping):
            continue
        raw_categories = summary.get("categories")
        if not isinstance(raw_categories, Mapping):
            continue
        for category, raw_values in raw_categories.items():
            if not isinstance(category, str) or not isinstance(raw_values, Mapping):
                continue
            count = non_negative_int(raw_values.get("count")) or 0
            duration_ms = non_negative_int(raw_values.get("duration_ms")) or 0
            bucket = categories.setdefault(category, {"count": 0, "duration_ms": 0})
            bucket["count"] += count
            bucket["duration_ms"] += duration_ms
    return {
        "total_count": sum(values["count"] for values in categories.values()),
        "total_duration_ms": sum(values["duration_ms"] for values in categories.values()),
        "categories": dict(sorted(categories.items())),
    }


def aggregate_viewport_events(events: Sequence[Mapping[str, object]]) -> dict[str, object]:
    durations_by_operation: dict[str, list[int]] = {}
    statuses: Counter[str] = Counter()
    skipped_apply_count = 0
    for event in events:
        operation = event.get("operation")
        if not isinstance(operation, str) or not operation:
            operation = "unknown"
        duration_ms = non_negative_int(event.get("duration_ms")) or 0
        durations_by_operation.setdefault(operation, []).append(duration_ms)
        status = event.get("status")
        statuses[str(status or "unknown")] += 1
        if operation == "apply" and event.get("skipped") is True:
            skipped_apply_count += 1
    return {
        "event_count": len(events),
        "total_duration_ms": sum(sum(values) for values in durations_by_operation.values()),
        "operations": {
            operation: duration_distribution(values)
            for operation, values in sorted(durations_by_operation.items())
        },
        "statuses": dict(sorted(statuses.items())),
        "skipped_apply_count": skipped_apply_count,
        "failed_count": statuses.get("failed", 0),
    }


def breakdown_by(records: Sequence[Mapping[str, object]], key_func: Callable[[Mapping[str, object]], str]) -> dict[str, dict[str, int]]:
    values_by_key: dict[str, list[int]] = {}
    for record in records:
        total = record_total_ms(record)
        if total is None:
            continue
        key = key_func(record) or "unknown"
        values_by_key.setdefault(key, []).append(total)
    return {key: duration_distribution(values) for key, values in sorted(values_by_key.items())}


def build_timing_summary(
    records: Sequence[Mapping[str, object]],
    *,
    gate_primitives: Sequence[Mapping[str, object]] | None = None,
    manifest_write_ms: int | None = None,
    viewport_events: Sequence[Mapping[str, object]] | None = None,
) -> dict[str, object]:
    timed_records = [record for record in records if record_total_ms(record) is not None]
    record_totals = [record_total_ms(record) or 0 for record in timed_records]
    record_distribution = duration_distribution(record_totals)
    gate_primitive_records = [dict(primitive) for primitive in (gate_primitives or ())]
    gate_primitive_durations: dict[str, int] = {}
    for primitive in gate_primitive_records:
        name = primitive.get("name")
        duration_ms = non_negative_int(primitive.get("duration_ms")) or 0
        if isinstance(name, str) and name:
            gate_primitive_durations[name] = gate_primitive_durations.get(name, 0) + duration_ms
    manifest_write_duration = non_negative_int(manifest_write_ms) or 0
    gate_total = sum(gate_primitive_durations.values())
    has_capture_gate = any(primitive.get("name") == "capture" for primitive in gate_primitive_records)
    total = gate_total if has_capture_gate else record_distribution["total"] + gate_total

    return {
        "total": total + manifest_write_duration,
        "p50": record_distribution["p50"],
        "p95": record_distribution["p95"],
        "max": record_distribution["max"],
        "record_count": record_distribution["count"],
        "record_total": record_distribution["total"],
        "gate_total": gate_total,
        "records": record_distribution,
        "target_breakdown": breakdown_by(
            timed_records,
            lambda record: str(record.get("target") or "unknown"),
        ),
        "profile_breakdown": breakdown_by(
            timed_records,
            lambda record: str(record.get("profile") or "unknown"),
        ),
        "method_breakdown": breakdown_by(timed_records, record_method_name),
        "adb_commands": aggregate_adb_summaries(timed_records),
        "viewport": aggregate_viewport_events(viewport_events or ()),
        "gate_primitives": gate_primitive_records,
        "gate_primitive_durations": gate_primitive_durations,
        "manifest_write_ms": manifest_write_duration,
    }


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
    require_button_role: bool = False
    require_enabled: bool = False
    require_disabled: bool = False

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
        if self.require_button_role:
            data["require_button_role"] = True
        if self.require_enabled:
            data["require_enabled"] = True
        if self.require_disabled:
            data["require_disabled"] = True
        return data


@dataclass(frozen=True)
class AccessibilityDumpStrategy:
    name: str
    batched: bool
    reason: str

    def to_json(self) -> dict[str, object]:
        return {"name": self.name, "batched": self.batched, "reason": self.reason}


@dataclass(frozen=True)
class AccessibilityXmlDump:
    xml_text: str
    command_strategy: str
    fallback_reason: str | None = None


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
    timing_summary: Mapping[str, object] | None = None

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

THEATER_PLATE_RECOVERY_ACTIONS: tuple[tuple[str, str], ...] = (
    ("retry", "Retry"),
    ("change-server", "Change server"),
    ("clear-cache", "Clear cache"),
    ("reset-connection", "Reset connection"),
    ("diagnostics", "Diagnostics / Export diagnostics"),
)

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


def redact_xml_text(text: str) -> str:
    return redact_text(text).replace("<redacted-origin>", "redacted-origin").replace("<redacted>", "redacted")


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
    recorder = current_timing_recorder()
    timing_started = recorder.now() if recorder is not None else time.monotonic()
    try:
        completed = subprocess.run(
            string_args,
            input=input_bytes,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            check=False,
            timeout=timeout,
        )
    except Exception:
        record_current_adb_timing(string_args, timing_started, recorder)
        raise
    record_current_adb_timing(string_args, timing_started, recorder)
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
    recorder = current_timing_recorder()
    timing_started = recorder.now() if recorder is not None else time.monotonic()
    output.parent.mkdir(parents=True, exist_ok=True)
    try:
        with output.open("wb") as handle:
            completed = subprocess.run(
                string_args,
                stdout=handle,
                stderr=subprocess.PIPE,
                check=False,
                timeout=timeout,
            )
    except Exception:
        record_current_adb_timing(string_args, timing_started, recorder)
        raise
    record_current_adb_timing(string_args, timing_started, recorder)
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


def parse_datetime_utc(value: object) -> dt.datetime | None:
    if not isinstance(value, str):
        return None
    text = value.strip()
    if not text:
        return None
    normalized = text.removesuffix("Z") + "+00:00" if text.endswith("Z") else text
    try:
        parsed = dt.datetime.fromisoformat(normalized)
    except ValueError:
        parsed = None
    if parsed is None:
        for fmt in ("%Y-%m-%d %H:%M:%S", "%Y-%m-%d %H:%M:%S.%f", "%Y-%m-%d"):
            try:
                parsed = dt.datetime.strptime(text, fmt).replace(tzinfo=dt.UTC)
                break
            except ValueError:
                continue
    if parsed is None:
        return None
    if parsed.tzinfo is None:
        return parsed.replace(tzinfo=dt.UTC)
    return parsed.astimezone(dt.UTC)


def parse_gradle_output_metadata(apk_path: Path) -> dict[str, object]:
    metadata_path = apk_path.with_name("output-metadata.json")
    try:
        parsed = json.loads(metadata_path.read_text(encoding="utf-8"))
    except OSError as exc:
        raise VisualQaError(f"Gradle output metadata is unavailable for {apk_path}: {exc}") from exc
    except json.JSONDecodeError as exc:
        raise VisualQaError(f"Gradle output metadata is not valid JSON: {metadata_path}: {exc}") from exc
    if not isinstance(parsed, dict):
        raise VisualQaError(f"Gradle output metadata root must be an object: {metadata_path}")
    application_id = parsed.get("applicationId")
    elements = parsed.get("elements")
    if not isinstance(application_id, str) or not application_id:
        raise VisualQaError(f"Gradle output metadata is missing applicationId: {metadata_path}")
    if not isinstance(elements, list) or not elements:
        raise VisualQaError(f"Gradle output metadata is missing elements: {metadata_path}")
    candidates = [element for element in elements if isinstance(element, Mapping)]
    element = next((candidate for candidate in candidates if candidate.get("outputFile") == apk_path.name), None)
    if element is None and len(candidates) == 1:
        element = candidates[0]
    if element is None:
        raise VisualQaError(f"Gradle output metadata does not describe {apk_path.name}: {metadata_path}")
    version_code = element.get("versionCode")
    version_name = element.get("versionName")
    if version_code is None or version_name is None:
        raise VisualQaError(f"Gradle output metadata is missing versionCode/versionName for {apk_path.name}: {metadata_path}")
    return {
        "source": "gradle-output-metadata",
        "metadata_path": str(metadata_path),
        "package_name": application_id,
        "version_code": str(version_code),
        "version_name": str(version_name),
        "output_file": str(element.get("outputFile") or apk_path.name),
    }


def parse_aapt2_badging(output: str) -> dict[str, object]:
    package_line = next((line for line in output.splitlines() if line.startswith("package: ")), "")
    if not package_line:
        raise VisualQaError("aapt2 badging output did not contain a package line")

    def field(name: str) -> str | None:
        match = re.search(rf"\b{name}='([^']*)'", package_line)
        return match.group(1) if match else None

    package_name = field("name")
    version_code = field("versionCode")
    version_name = field("versionName")
    if not package_name or version_code is None or version_name is None:
        raise VisualQaError("aapt2 badging output is missing package name/version fields")
    return {
        "source": "aapt2-badging",
        "package_name": package_name,
        "version_code": version_code,
        "version_name": version_name,
    }


def inferred_android_app_dir(config: TargetConfig) -> Path | None:
    for parent in config.apk_path.parents:
        if parent.name == "app" and (parent / "src").is_dir():
            return parent
    return None


def iter_freshness_input_files(paths: Sequence[Path]) -> Iterator[Path]:
    ignored_dir_names = {".gradle", ".idea", "build"}
    for path in paths:
        if path.is_file():
            yield path
        elif path.is_dir():
            for root, dirs, files in os.walk(path):
                dirs[:] = [name for name in dirs if name not in ignored_dir_names]
                root_path = Path(root)
                for file_name in files:
                    yield root_path / file_name


def collect_host_apk_input_freshness(config: TargetConfig, host_apk: Mapping[str, object]) -> dict[str, object]:
    host_mtime = parse_datetime_utc(host_apk.get("mtime"))
    if host_mtime is None:
        return {"status": "unavailable", "reason": "host APK mtime is unavailable"}
    app_dir = inferred_android_app_dir(config)
    if app_dir is None:
        return {"status": "unavailable", "reason": "could not infer Android app source directory from APK path"}
    android_dir = app_dir.parent
    candidate_paths = (
        app_dir / "src",
        app_dir / "build.gradle.kts",
        app_dir / "lint.xml",
        app_dir / "proguard-rules.pro",
        android_dir / "build.gradle.kts",
        android_dir / "settings.gradle.kts",
        android_dir / "gradle.properties",
        android_dir / "gradle",
        android_dir / "gradlew",
        android_dir / "gradlew.bat",
    )
    latest_path: Path | None = None
    latest_mtime: dt.datetime | None = None
    checked_count = 0
    for path in iter_freshness_input_files(candidate_paths):
        try:
            stat = path.stat()
        except OSError:
            continue
        checked_count += 1
        mtime = dt.datetime.fromtimestamp(stat.st_mtime, dt.UTC)
        if latest_mtime is None or mtime > latest_mtime:
            latest_mtime = mtime
            latest_path = path
    if latest_mtime is None or latest_path is None:
        return {"status": "unavailable", "reason": "no Android source/build input mtimes were readable"}
    latest_iso = latest_mtime.replace(microsecond=0).isoformat().replace("+00:00", "Z")
    data: dict[str, object] = {
        "status": "fresh" if host_mtime >= latest_mtime else "stale",
        "checked_file_count": checked_count,
        "latest_input_path": str(latest_path),
        "latest_input_mtime": latest_iso,
        "host_apk_mtime": host_apk.get("mtime"),
    }
    if data["status"] == "stale":
        data["reason"] = f"host APK is older than Android input {latest_path}"
    else:
        data["reason"] = "host APK mtime is not older than checked Android source/build inputs"
    return data


def collect_host_apk_metadata(config: TargetConfig) -> dict[str, object]:
    host_apk = file_identity(config.apk_path)
    if not host_apk.get("exists"):
        raise VisualQaError(f"{config.target} host APK is missing at {config.apk_path}; run the build primitive first")

    input_freshness = collect_host_apk_input_freshness(config, host_apk)
    if input_freshness.get("status") == "stale":
        raise VisualQaError(str(input_freshness.get("reason") or "host APK is older than Android source/build inputs"))

    metadata_errors: list[str] = []
    metadata: dict[str, object] | None = None
    try:
        metadata = parse_gradle_output_metadata(config.apk_path)
    except VisualQaError as exc:
        metadata_errors.append(str(exc))

    if metadata is None:
        aapt2 = shutil.which("aapt2")
        if aapt2:
            try:
                metadata = parse_aapt2_badging(run_command([aapt2, "dump", "badging", config.apk_path], timeout=30).stdout)
            except (OSError, subprocess.SubprocessError, VisualQaError) as exc:
                metadata_errors.append(str(exc))
        else:
            metadata_errors.append("aapt2 is unavailable on PATH")

    if metadata is None:
        raise VisualQaError(
            f"{config.target} host APK metadata could not be proven for {config.apk_path}: "
            + "; ".join(redact_text(error) for error in metadata_errors)
        )
    package_name = metadata.get("package_name")
    if package_name != config.package:
        raise VisualQaError(
            f"{config.target} host APK package mismatch for {config.apk_path}: expected {config.package}, got {package_name or 'empty'}"
        )
    return {
        "package_name": package_name,
        "version_code": metadata.get("version_code"),
        "version_name": metadata.get("version_name"),
        "metadata_source": metadata.get("source"),
        "metadata": metadata,
        "host_apk": host_apk,
        "input_freshness": input_freshness,
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
        requested_ids = [part.strip() for part in scenario_id.split(",") if part.strip()]
        if not requested_ids:
            raise VisualQaError("at least one scenario ID must be selected")
        if "all" in requested_ids:
            if len(requested_ids) > 1:
                raise VisualQaError("scenario 'all' cannot be combined with explicit scenario IDs")
            return [
                scenario
                for scenario in self.scenarios
                if normalized_target == "all" or scenario.target == normalized_target
            ]

        selected: list[Scenario] = []
        seen: set[str] = set()
        for requested in requested_ids:
            scenario = self.by_id.get(requested)
            if scenario is None:
                available = ", ".join(scenario.id for scenario in self.scenarios)
                raise VisualQaError(f"unknown visual QA scenario {requested!r}; available: {available}")
            if normalized_target != "all" and scenario.target != normalized_target:
                raise VisualQaError(
                    f"scenario {requested!r} belongs to {scenario.target}, not requested target {normalized_target}"
                )
            if scenario.id not in seen:
                selected.append(scenario)
                seen.add(scenario.id)
        return selected

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


def shell_command(*args: str) -> str:
    return " ".join(shlex.quote(arg) for arg in args)


def parse_adb_shell_sections(output: str, section_names: Sequence[str]) -> dict[str, str]:
    begin_markers = {f"__FERREX_VISUAL_QA_BEGIN_{name}__": name for name in section_names}
    end_markers = {f"__FERREX_VISUAL_QA_END_{name}__": name for name in section_names}
    sections: dict[str, list[str]] = {name: [] for name in section_names}
    current: str | None = None
    for line in output.splitlines():
        if line in begin_markers:
            current = begin_markers[line]
            sections[current] = []
        elif line in end_markers:
            if current == end_markers[line]:
                current = None
        elif current is not None:
            sections[current].append(line)
    return {name: "\n".join(lines).strip() for name, lines in sections.items()}


def adb_shell_sections(
    adb: str,
    serial: str,
    sections: Sequence[tuple[str, str]],
    *,
    timeout: int = 60,
) -> dict[str, str]:
    names = [name for name, _ in sections]
    invalid = [name for name in names if re.fullmatch(r"[A-Za-z0-9_]+", name) is None]
    if invalid:
        raise VisualQaError(f"invalid adb shell section name(s): {', '.join(invalid)}")
    script_lines: list[str] = []
    for name, command in sections:
        begin = f"__FERREX_VISUAL_QA_BEGIN_{name}__"
        end = f"__FERREX_VISUAL_QA_END_{name}__"
        script_lines.append(f"printf '%s\\n' {shlex.quote(begin)}")
        script_lines.append(command)
        script_lines.append("printf '\\n'")
        script_lines.append(f"printf '%s\\n' {shlex.quote(end)}")
    # `adb shell` joins argv on the host before the device shell sees it, so
    # pass one quoted remote command string; otherwise `sh -c` receives only the
    # first token as its script and the first section marker is lost.
    script = "\n".join(script_lines)
    output = adb_shell(adb, serial, f"sh -c {shlex.quote(script)}", timeout=timeout).stdout
    return parse_adb_shell_sections(output, names)


def getprop(adb: str, serial: str, name: str) -> str:
    return adb_shell(adb, serial, "getprop", name, timeout=30).stdout.strip()


def parse_user_rotation(raw: str) -> tuple[str | None, str | None]:
    parts = raw.split()
    if not parts:
        return None, None
    mode = parts[0]
    rotation = parts[1] if len(parts) > 1 else None
    return mode, rotation


def _setting_value(result: RunResult) -> str | None:
    value = result.stdout.strip() if result.returncode == 0 else ""
    return value if value and value != "null" else None


def _normalized_setting_output(value: str | None) -> str | None:
    normalized = (value or "").strip()
    return normalized if normalized and normalized != "null" else None


def wm_snapshot(adb: str, serial: str) -> WmOverrideSnapshot:
    sections = adb_shell_sections(
        adb,
        serial,
        (
            ("wm_size", shell_command("wm", "size")),
            ("wm_density", shell_command("wm", "density")),
            ("accelerometer_rotation", shell_command("settings", "get", "system", "accelerometer_rotation")),
            ("user_rotation", shell_command("settings", "get", "system", "user_rotation")),
        ),
        timeout=30,
    )
    raw_size = sections["wm_size"]
    raw_density = sections["wm_density"]
    raw_rotation_result = adb_shell(adb, serial, "cmd", "window", "user-rotation", check=False, timeout=30)
    raw_user_rotation = raw_rotation_result.stdout.strip() if raw_rotation_result.returncode == 0 else None
    user_rotation_mode, parsed_user_rotation = parse_user_rotation(raw_user_rotation or "")
    size_match = re.search(r"(?im)^\s*Override size:\s*(\d+x\d+)\s*$", raw_size)
    density_match = re.search(r"(?im)^\s*Override density:\s*(\d+)\s*$", raw_density)
    user_rotation = _normalized_setting_output(sections["user_rotation"]) or parsed_user_rotation
    return WmOverrideSnapshot(
        raw_size=raw_size,
        raw_density=raw_density,
        override_size=size_match.group(1) if size_match else None,
        override_density=density_match.group(1) if density_match else None,
        accelerometer_rotation=_normalized_setting_output(sections["accelerometer_rotation"]),
        user_rotation=user_rotation,
        raw_user_rotation=raw_user_rotation,
        user_rotation_mode=user_rotation_mode,
    )


def wm_effective_size(snapshot: WmOverrideSnapshot) -> str | None:
    if snapshot.override_size:
        return snapshot.override_size
    match = re.search(r"(?im)^\s*Physical size:\s*(\d+x\d+)\s*$", snapshot.raw_size)
    return match.group(1) if match else None


def wm_effective_density(snapshot: WmOverrideSnapshot) -> str | None:
    if snapshot.override_density:
        return snapshot.override_density
    match = re.search(r"(?im)^\s*Physical density:\s*(\d+)\s*$", snapshot.raw_density)
    return match.group(1) if match else None


def viewport_user_rotation(profile: ViewportProfile) -> str | None:
    if profile.wm_size is None and profile.wm_density is None:
        return None
    # Keep emulator framebuffer orientation stable and let wm size/density define
    # the logical QA viewport. Auto-rotation can remap 1800x1200 foldable captures
    # into portrait 1200x1800 frames while the activity is settling.
    return "0"


def set_viewport_profile(
    adb: str,
    config: TargetConfig,
    profile: ViewportProfile,
    *,
    include_snapshot: bool = True,
) -> dict[str, object]:
    applied: dict[str, object] = {}
    has_viewport_override = profile.wm_size is not None or profile.wm_density is not None
    desired_rotation = viewport_user_rotation(profile)
    if not has_viewport_override and desired_rotation is None:
        return applied
    if config.target == "phone" and has_viewport_override:
        adb_shell(adb, config.serial, "cmd", "window", "user-rotation", "lock", "0", check=False, timeout=30)
        applied["window_user_rotation"] = "lock 0"
    if desired_rotation is not None:
        adb_shell(adb, config.serial, "settings", "put", "system", "accelerometer_rotation", "0", timeout=30)
        adb_shell(adb, config.serial, "settings", "put", "system", "user_rotation", desired_rotation, timeout=30)
        applied["accelerometer_rotation"] = "0"
        applied["user_rotation"] = desired_rotation
    if has_viewport_override:
        adb_shell(adb, config.serial, "wm", "size", "reset", timeout=30)
        adb_shell(adb, config.serial, "wm", "density", "reset", timeout=30)
    if profile.wm_size is not None:
        wm_size = size_to_string(profile.wm_size)
        adb_shell(adb, config.serial, "wm", "size", wm_size, timeout=30)
        applied["wm_size"] = wm_size
    if profile.wm_density is not None:
        adb_shell(adb, config.serial, "wm", "density", str(profile.wm_density), timeout=30)
        applied["wm_density"] = profile.wm_density
    if config.target == "phone" and has_viewport_override:
        adb_shell(adb, config.serial, "cmd", "window", "user-rotation", "lock", "0", check=False, timeout=30)
    if include_snapshot:
        applied["snapshot"] = wm_snapshot(adb, config.serial).to_json()
    return applied


def apply_viewport_profile_for_group(
    adb: str,
    config: TargetConfig,
    profile: ViewportProfile,
    *,
    force: bool = False,
) -> ViewportApplyEvidence:
    before = wm_snapshot(adb, config.serial)
    actions: list[dict[str, str]] = []
    has_viewport_override = profile.wm_size is not None or profile.wm_density is not None
    try:
        desired_rotation = viewport_user_rotation(profile)
        if desired_rotation is not None:
            if force or before.accelerometer_rotation != "0":
                adb_shell(adb, config.serial, "settings", "put", "system", "accelerometer_rotation", "0", timeout=30)
                actions.append({"kind": "accelerometer_rotation", "value": "0"})
            if force or before.user_rotation != desired_rotation:
                adb_shell(adb, config.serial, "settings", "put", "system", "user_rotation", desired_rotation, timeout=30)
                actions.append({"kind": "user_rotation", "value": desired_rotation})
        if profile.wm_size is not None:
            desired_size = size_to_string(profile.wm_size)
            if force or wm_effective_size(before) != desired_size:
                adb_shell(adb, config.serial, "wm", "size", desired_size, timeout=30)
                actions.append({"kind": "wm_size", "value": desired_size})
        if profile.wm_density is not None:
            desired_density = str(profile.wm_density)
            if force or wm_effective_density(before) != desired_density:
                adb_shell(adb, config.serial, "wm", "density", desired_density, timeout=30)
                actions.append({"kind": "wm_density", "value": desired_density})
        if config.target == "phone" and has_viewport_override and (force or actions):
            adb_shell(adb, config.serial, "cmd", "window", "user-rotation", "lock", "0", check=False, timeout=30)
            if force:
                actions.append({"kind": "window_user_rotation", "value": "lock 0"})
    except Exception:
        restore_viewport_profile(adb, config, before)
        raise
    after = wm_snapshot(adb, config.serial) if actions else before
    return ViewportApplyEvidence(before=before, after=after, actions=tuple(actions))


def apply_viewport_profile(adb: str, config: TargetConfig, profile: ViewportProfile) -> WmOverrideSnapshot:
    return apply_viewport_profile_for_group(adb, config, profile, force=True).before


def restore_viewport_profile(adb: str, config: TargetConfig, before: WmOverrideSnapshot) -> dict[str, object]:
    if before.override_size:
        adb_shell(adb, config.serial, "wm", "size", before.override_size, timeout=30)
    else:
        adb_shell(adb, config.serial, "wm", "size", "reset", timeout=30)
    if before.override_density:
        adb_shell(adb, config.serial, "wm", "density", before.override_density, timeout=30)
    else:
        adb_shell(adb, config.serial, "wm", "density", "reset", timeout=30)
    if before.accelerometer_rotation in {"0", "1"}:
        adb_shell(
            adb,
            config.serial,
            "settings",
            "put",
            "system",
            "accelerometer_rotation",
            before.accelerometer_rotation,
            timeout=30,
        )
    if before.user_rotation in {"0", "1", "2", "3"}:
        adb_shell(adb, config.serial, "settings", "put", "system", "user_rotation", before.user_rotation, timeout=30)
    if before.user_rotation_mode == "lock" and before.user_rotation is not None:
        adb_shell(
            adb,
            config.serial,
            "cmd",
            "window",
            "user-rotation",
            "lock",
            before.user_rotation,
            check=False,
            timeout=30,
        )
    elif before.user_rotation_mode == "free":
        adb_shell(adb, config.serial, "cmd", "window", "user-rotation", "free", check=False, timeout=30)
    return {"restored_to": wm_snapshot(adb, config.serial).to_json()}


def collect_serial_metadata(adb: str, config: TargetConfig) -> dict[str, object]:
    serial = config.serial
    prop_commands = {
        "sdk": "ro.build.version.sdk",
        "release": "ro.build.version.release",
        "model": "ro.product.model",
        "device": "ro.product.device",
        "manufacturer": "ro.product.manufacturer",
        "brand": "ro.product.brand",
        "product": "ro.product.name",
        "abi": "ro.product.cpu.abi",
    }
    sections = adb_shell_sections(
        adb,
        serial,
        (
            *((key, shell_command("getprop", prop_name)) for key, prop_name in prop_commands.items()),
            ("wm_size", shell_command("wm", "size")),
            ("wm_density", shell_command("wm", "density")),
            ("features", shell_command("pm", "list", "features")),
        ),
        timeout=60,
    )
    props = {key: sections[key] for key in prop_commands}
    features = sections["features"].splitlines()
    leanback = "feature:android.software.leanback" in {line.strip() for line in features}
    return {
        "target": config.target,
        "serial": serial,
        "expected_serial_default": config.default_serial,
        "is_default_emulator_serial": serial == config.default_serial,
        "properties": props,
        "wm_size": sections["wm_size"],
        "wm_density": sections["wm_density"],
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
    sections = adb_shell_sections(
        adb,
        config.serial,
        (
            ("package_path", shell_command("pm", "path", config.package)),
            ("package_dump", shell_command("dumpsys", "package", config.package)),
        ),
        timeout=90,
    )
    package_path = sections["package_path"]
    if not package_path:
        raise VisualQaError(
            f"{config.target} package {config.package} is not installed on {config.serial}; run scripts/qa/android-emulator-qa.sh install {config.target} first"
        )
    return {
        "package_name": config.package,
        "installed_package_path": package_path.removeprefix("package:"),
        "host_apk": file_identity(config.apk_path),
        **parse_package_dump(sections["package_dump"]),
    }


def warm_check_record(name: str, status: str, reason: str, **fields: object) -> dict[str, object]:
    record: dict[str, object] = {"name": name, "status": status, "reason": redact_text(reason)}
    record.update(fields)
    return record


def expected_warm_target_metadata(config: TargetConfig) -> dict[str, object]:
    if config.target == "phone":
        return {
            "serial": config.serial,
            "sdk": os.environ.get("FERREX_ANDROID_PHONE_API", "35"),
            "model": os.environ.get("FERREX_ANDROID_PHONE_MODEL", "sdk_gphone64_x86_64"),
            "device": os.environ.get("FERREX_ANDROID_PHONE_DEVICE", "emu64xa"),
            "leanback": False,
        }
    if config.target == "tv":
        return {
            "serial": config.serial,
            "sdk": os.environ.get("FERREX_ANDROID_TV_API", "34"),
            "model": os.environ.get("FERREX_ANDROID_TV_MODEL", "AOSP TV on x86"),
            "device": os.environ.get("FERREX_ANDROID_TV_DEVICE", "emulator_x86_arm"),
            "leanback": True,
        }
    raise VisualQaError(f"unknown warm target {config.target!r}")


def collect_device_apk_identity(adb: str, config: TargetConfig, installed_path: str) -> dict[str, object]:
    if not installed_path:
        return {"available": False, "path": installed_path, "unavailable_reason": "installed package path is empty"}
    quoted_path = shlex.quote(installed_path)
    try:
        sections = adb_shell_sections(
            adb,
            config.serial,
            (
                (
                    "bytes",
                    f"if [ -r {quoted_path} ]; then wc -c < {quoted_path} 2>/dev/null | tr -d ' '; fi",
                ),
                (
                    "sha256",
                    f"if command -v sha256sum >/dev/null 2>&1 && [ -r {quoted_path} ]; then sha256sum {quoted_path} 2>/dev/null | awk '{{print $1}}'; fi",
                ),
                (
                    "mtime_epoch",
                    f"if command -v stat >/dev/null 2>&1 && [ -r {quoted_path} ]; then stat -c %Y {quoted_path} 2>/dev/null; fi",
                ),
            ),
            timeout=90,
        )
    except Exception as exc:  # noqa: BLE001 - identity probing should degrade to package timestamp evidence.
        return {
            "available": False,
            "path": installed_path,
            "unavailable_reason": redact_text(str(exc)),
        }

    identity: dict[str, object] = {"available": False, "path": installed_path}
    bytes_text = sections.get("bytes", "").strip()
    if bytes_text.isdigit():
        identity["bytes"] = int(bytes_text)
    sha256_text = sections.get("sha256", "").strip().splitlines()[0:1]
    if sha256_text and re.fullmatch(r"[0-9a-fA-F]{64}", sha256_text[0]):
        identity["sha256"] = sha256_text[0].lower()
    mtime_text = sections.get("mtime_epoch", "").strip()
    if re.fullmatch(r"\d+", mtime_text):
        identity["mtime_epoch"] = int(mtime_text)
    identity["available"] = any(key in identity for key in ("bytes", "sha256", "mtime_epoch"))
    if not identity["available"]:
        identity["unavailable_reason"] = "device APK bytes/hash/stat probes returned no data"
    return identity


def apk_freshness_check(
    *,
    adb: str,
    config: TargetConfig,
    host_metadata: Mapping[str, object],
    package_metadata: Mapping[str, object],
) -> dict[str, object]:
    host_apk = host_metadata.get("host_apk")
    if not isinstance(host_apk, Mapping):
        return warm_check_record("freshness", "failed", "host APK identity is missing from warm evidence")
    installed_path = package_metadata.get("installed_package_path")
    if not isinstance(installed_path, str) or not installed_path:
        return warm_check_record("freshness", "failed", "installed package path is missing from package metadata")

    device_identity = collect_device_apk_identity(adb, config, installed_path)
    host_bytes = non_negative_int(host_apk.get("bytes"))
    host_sha256 = host_apk.get("sha256") if isinstance(host_apk.get("sha256"), str) else None
    device_bytes = non_negative_int(device_identity.get("bytes"))
    device_sha256 = device_identity.get("sha256") if isinstance(device_identity.get("sha256"), str) else None

    if host_bytes is not None and device_bytes is not None and host_bytes != device_bytes:
        return warm_check_record(
            "freshness",
            "failed",
            f"installed APK size {device_bytes} does not match host APK size {host_bytes}",
            host_apk=host_apk,
            device_apk=device_identity,
        )
    if host_sha256 and device_sha256:
        if host_sha256.lower() != device_sha256.lower():
            return warm_check_record(
                "freshness",
                "failed",
                "installed APK sha256 does not match host APK sha256",
                host_apk=host_apk,
                device_apk=device_identity,
            )
        return warm_check_record(
            "freshness",
            "passed",
            "installed APK sha256 and size match the host APK",
            proof="device_sha256_size",
            host_apk=host_apk,
            device_apk=device_identity,
        )

    host_mtime = parse_datetime_utc(host_apk.get("mtime"))
    device_mtime_epoch = non_negative_int(device_identity.get("mtime_epoch"))
    if host_mtime is not None and device_mtime_epoch is not None:
        device_mtime = dt.datetime.fromtimestamp(device_mtime_epoch, dt.UTC)
        if device_mtime >= host_mtime:
            return warm_check_record(
                "freshness",
                "passed",
                "installed APK device mtime is not older than the host APK mtime",
                proof="device_apk_mtime_vs_host_mtime",
                host_apk=host_apk,
                device_apk=device_identity,
            )

    installed_update_time = parse_datetime_utc(package_metadata.get("last_update_time"))
    if host_mtime is not None and installed_update_time is not None:
        if installed_update_time >= host_mtime:
            return warm_check_record(
                "freshness",
                "passed",
                "installed package lastUpdateTime is not older than the host APK mtime",
                proof="last_update_time_vs_host_mtime",
                host_apk=host_apk,
                device_apk=device_identity,
                installed_last_update_time=package_metadata.get("last_update_time"),
            )
        return warm_check_record(
            "freshness",
            "failed",
            "installed package lastUpdateTime is older than the host APK mtime",
            host_apk=host_apk,
            device_apk=device_identity,
            installed_last_update_time=package_metadata.get("last_update_time"),
        )

    return warm_check_record(
        "freshness",
        "failed",
        "freshness could not be proven from device hash/size or installed lastUpdateTime",
        host_apk=host_apk,
        device_apk=device_identity,
        installed_last_update_time=package_metadata.get("last_update_time"),
    )


def evaluate_warm_target(adb: str, config: TargetConfig) -> dict[str, object]:
    checks: list[dict[str, object]] = []

    def failed(name: str, reason: str, **fields: object) -> dict[str, object]:
        checks.append(warm_check_record(name, "failed", reason, **fields))
        return {
            "target": config.target,
            "serial": config.serial,
            "status": "fallback",
            "reason": redact_text(reason),
            "checks": checks,
        }

    try:
        require_serial_present(adb, config.serial)
    except Exception as exc:  # noqa: BLE001 - warm preflight falls back to the full preparation path.
        return failed("serial_ready", f"ADB serial {config.serial} is not ready: {exc}")
    checks.append(warm_check_record("serial_ready", "passed", f"ADB serial {config.serial} is in device state"))

    try:
        boot_completed = getprop(adb, config.serial, "sys.boot_completed")
    except Exception as exc:  # noqa: BLE001 - warm preflight falls back to the full preparation path.
        return failed("boot_completed", f"could not read sys.boot_completed on {config.serial}: {exc}")
    if boot_completed != "1":
        return failed("boot_completed", f"{config.serial} has sys.boot_completed={boot_completed or 'empty'}", value=boot_completed)
    checks.append(warm_check_record("boot_completed", "passed", "sys.boot_completed=1", value=boot_completed))

    try:
        serial_metadata = collect_serial_metadata(adb, config)
    except Exception as exc:  # noqa: BLE001 - warm preflight falls back to the full preparation path.
        return failed("target_metadata", f"could not collect target metadata for {config.serial}: {exc}")
    props_raw = serial_metadata.get("properties")
    props = props_raw if isinstance(props_raw, Mapping) else {}
    expected = expected_warm_target_metadata(config)
    actual = {
        "serial": config.serial,
        "sdk": props.get("sdk"),
        "model": props.get("model"),
        "device": props.get("device"),
        "leanback": serial_metadata.get("leanback"),
    }
    mismatches = [key for key, expected_value in expected.items() if actual.get(key) != expected_value]
    if mismatches:
        return failed(
            "target_metadata",
            f"{config.target} target metadata mismatch on {config.serial}: {', '.join(mismatches)}",
            expected=expected,
            actual=actual,
        )
    checks.append(
        warm_check_record(
            "target_metadata",
            "passed",
            "serial target metadata matches the expected emulator profile",
            expected=expected,
            actual=actual,
        )
    )

    try:
        host_metadata = collect_host_apk_metadata(config)
    except Exception as exc:  # noqa: BLE001 - warm preflight falls back to the full preparation path.
        return failed("host_apk", f"host APK metadata is not fresh/provable for {config.target}: {exc}")
    checks.append(
        warm_check_record(
            "host_apk",
            "passed",
            "host APK exists and declares the expected package/version metadata",
            metadata=host_metadata,
        )
    )

    try:
        package_metadata = collect_package_metadata(adb, config)
    except Exception as exc:  # noqa: BLE001 - warm preflight falls back to the full preparation path.
        return failed("installed_package", f"installed package metadata is unavailable for {config.target}: {exc}")
    version_mismatches: list[str] = []
    for key in ("package_name", "version_code", "version_name"):
        if str(package_metadata.get(key)) != str(host_metadata.get(key)):
            version_mismatches.append(key)
    if version_mismatches:
        return failed(
            "installed_package",
            f"installed package metadata does not match host APK for {config.target}: {', '.join(version_mismatches)}",
            expected={key: host_metadata.get(key) for key in ("package_name", "version_code", "version_name")},
            actual={key: package_metadata.get(key) for key in ("package_name", "version_code", "version_name")},
        )
    checks.append(
        warm_check_record(
            "installed_package",
            "passed",
            "installed package name/version matches the host APK metadata",
            metadata={key: package_metadata.get(key) for key in ("package_name", "version_code", "version_name", "installed_package_path", "last_update_time")},
        )
    )

    freshness = apk_freshness_check(
        adb=adb,
        config=config,
        host_metadata=host_metadata,
        package_metadata=package_metadata,
    )
    checks.append(freshness)
    if freshness.get("status") != "passed":
        return {
            "target": config.target,
            "serial": config.serial,
            "status": "fallback",
            "reason": str(freshness.get("reason") or "freshness could not be proven"),
            "checks": checks,
        }

    return {
        "target": config.target,
        "serial": config.serial,
        "status": "passed",
        "reason": "serial, boot, target metadata, package metadata, and APK freshness all passed",
        "checks": checks,
    }


def warm_target_config_args() -> argparse.Namespace:
    return argparse.Namespace(target="all", hardware=False, hardware_serial=None, expected_size=None)


def evaluate_warm_gate(args: argparse.Namespace, repo_root: Path) -> dict[str, object]:
    evidence: dict[str, object] = {
        "policy": WARM_FRESHNESS_POLICY,
        "requested": True,
        "status": "fallback",
        "reason": "warm freshness was not evaluated",
        "targets": {},
    }
    try:
        adb = resolve_executable(args.adb)
        configs = target_configs(repo_root, warm_target_config_args())
    except Exception as exc:  # noqa: BLE001 - warm preflight falls back to the full preparation path.
        evidence["reason"] = redact_text(str(exc))
        return evidence

    target_results: dict[str, object] = {}
    fallback_reasons: list[str] = []
    for target in ("phone", "tv"):
        result = evaluate_warm_target(adb, configs[target])
        target_results[target] = result
        if result.get("status") != "passed":
            fallback_reasons.append(f"{target}: {result.get('reason')}")
    evidence["targets"] = target_results
    if fallback_reasons:
        evidence["status"] = "fallback"
        evidence["reason"] = "; ".join(redact_text(reason) for reason in fallback_reasons)
    else:
        evidence["status"] = "passed"
        evidence["reason"] = "all warm freshness checks passed for phone and TV targets"
    return evidence


class RunCache:
    def __init__(self) -> None:
        self._entries: dict[str, dict[str, dict[str, object]]] = {
            "serial_readiness": {},
            "serial_metadata": {},
            "package_metadata": {},
            "command_versions": {},
        }
        self._stats: dict[str, Counter[str]] = {category: Counter() for category in self._entries}
        self._sequence = 0

    def _target_serial_key(self, config: TargetConfig) -> str:
        return f"{config.target}:{config.serial}"

    def _package_key(self, config: TargetConfig) -> str:
        return f"{config.serial}:{config.package}:{config.apk_path}"

    def _store_entry(self, category: str, key: str, value: Mapping[str, object]) -> dict[str, object]:
        self._sequence += 1
        entry: dict[str, object] = {
            "value": copy.deepcopy(dict(value)),
            "collected_at": utc_now(),
            "sequence": self._sequence,
            "access_count": 0,
        }
        self._entries[category][key] = entry
        return entry

    def _lookup(self, category: str, key: str, loader: Callable[[], Mapping[str, object]]) -> CacheLookup:
        entry = self._entries[category].get(key)
        hit = entry is not None
        if hit:
            self._stats[category]["hits"] += 1
        else:
            self._stats[category]["misses"] += 1
            entry = self._store_entry(category, key, loader())
        entry["access_count"] = int(entry.get("access_count", 0)) + 1
        provenance = {
            "category": category,
            "key": key,
            "hit": hit,
            "collected_at": entry["collected_at"],
            "sequence": entry["sequence"],
            "access_count": entry["access_count"],
        }
        value = entry["value"]
        return CacheLookup(value=copy.deepcopy(value) if isinstance(value, dict) else {}, provenance=provenance)

    def require_serial_present(self, adb: str, config: TargetConfig) -> CacheLookup:
        key = self._target_serial_key(config)

        def load() -> Mapping[str, object]:
            require_serial_present(adb, config.serial)
            return {
                "target": config.target,
                "serial": config.serial,
                "ready": True,
                "state": "device",
            }

        return self._lookup("serial_readiness", key, load)

    def serial_metadata(self, adb: str, config: TargetConfig) -> CacheLookup:
        return self._lookup(
            "serial_metadata",
            self._target_serial_key(config),
            lambda: collect_serial_metadata(adb, config),
        )

    def package_metadata(self, adb: str, config: TargetConfig) -> CacheLookup:
        return self._lookup(
            "package_metadata",
            self._package_key(config),
            lambda: collect_package_metadata(adb, config),
        )

    def command_versions(self, adb: str, script_path: Path) -> CacheLookup:
        return self._lookup(
            "command_versions",
            f"{adb}:{script_path}",
            lambda: collect_command_versions(adb, script_path),
        )

    def invalidate_target(self, config: TargetConfig, reason: str) -> None:
        targets = {
            "serial_readiness": (self._target_serial_key(config),),
            "serial_metadata": (self._target_serial_key(config),),
            "package_metadata": (self._package_key(config),),
        }
        for category, keys in targets.items():
            invalidated = False
            for key in keys:
                if self._entries[category].pop(key, None) is not None:
                    invalidated = True
            if invalidated:
                self._stats[category]["invalidations"] += 1
                self._stats[category][f"invalidated_by:{sanitized_category_token(reason)}"] += 1

    def summary(self) -> dict[str, object]:
        summary: dict[str, object] = {}
        for category, entries in sorted(self._entries.items()):
            stats = self._stats[category]
            hits = int(stats.get("hits", 0))
            misses = int(stats.get("misses", 0))
            invalidations = int(stats.get("invalidations", 0))
            invalidation_reasons = {
                key.removeprefix("invalidated_by:"): int(value)
                for key, value in sorted(stats.items())
                if key.startswith("invalidated_by:")
            }
            summary[category] = {
                "hits": hits,
                "misses": misses,
                "lookups": hits + misses,
                "invalidations": invalidations,
                "invalidation_reasons": invalidation_reasons,
                "entries": len(entries),
            }
        return summary


def target_worker_count(args: argparse.Namespace) -> int:
    raw_value = getattr(args, "target_workers", 1)
    try:
        count = int(raw_value)
    except (TypeError, ValueError) as exc:
        raise VisualQaError(f"--target-workers must be a positive integer, got {raw_value!r}") from exc
    if count <= 0:
        raise VisualQaError(f"--target-workers must be positive, got {count}")
    return count


def target_order_for_plans(plans: Sequence[TargetViewportPlan]) -> tuple[str, ...]:
    return tuple(dict.fromkeys(plan.target for plan in plans))


def effective_target_worker_count(
    args: argparse.Namespace,
    target_order: Sequence[str],
    configs: Mapping[str, TargetConfig],
) -> tuple[int, int]:
    requested = target_worker_count(args)
    effective = min(requested, len(target_order)) if target_order else 1
    if effective > 1:
        targets_by_serial: dict[str, list[str]] = {}
        for target in target_order:
            targets_by_serial.setdefault(configs[target].serial, []).append(target)
        duplicate_serials = {
            serial: targets
            for serial, targets in targets_by_serial.items()
            if len(targets) > 1
        }
        if duplicate_serials:
            details = ", ".join(
                f"{serial} ({'/'.join(targets)})"
                for serial, targets in sorted(duplicate_serials.items())
            )
            raise VisualQaError(
                "--target-workers requires unique ADB serials per concurrent target; "
                f"duplicate serial assignment: {details}"
            )
    return requested, effective


def target_worker_manifest_config(
    *,
    requested: int,
    effective: int,
    target_order: Sequence[str],
    configs: Mapping[str, TargetConfig],
    profiles_by_target: Mapping[str, Sequence[ViewportProfile]],
    artifact_kind: str,
) -> dict[str, object]:
    return {
        "requested_worker_count": requested,
        "worker_count": effective,
        "enabled": effective > 1,
        "strategy": "one-worker-per-selected-target; each worker runs its target serially",
        "artifact_kind": artifact_kind,
        "deterministic_record_order": "target order from the scenario registry, then selected viewport profile order, then scenario registry order",
        "targets": [
            {
                "target": target,
                "serial": configs[target].serial,
                "worker_index": index if effective > 1 else 0,
                "viewport_profiles": [profile.name for profile in profiles_by_target.get(target, ())],
                "output_subtrees": [
                    f"accessibility/{profile.name}" if artifact_kind == "accessibility" else profile.name
                    for profile in profiles_by_target.get(target, ())
                ],
                "failure_log_path_pattern": "logs/<profile>-<scenario>-*-logcat.txt",
            }
            for index, target in enumerate(target_order)
        ],
        "timings": [],
    }


def merge_cache_summaries(*summaries: Mapping[str, object]) -> dict[str, object]:
    categories = sorted({category for summary in summaries for category in summary.keys()})
    merged: dict[str, object] = {}
    for category in categories:
        hits = misses = invalidations = entries = 0
        invalidation_reasons: Counter[str] = Counter()
        for summary in summaries:
            raw_category = summary.get(category)
            if not isinstance(raw_category, Mapping):
                continue
            hits += non_negative_int(raw_category.get("hits")) or 0
            misses += non_negative_int(raw_category.get("misses")) or 0
            invalidations += non_negative_int(raw_category.get("invalidations")) or 0
            entries += non_negative_int(raw_category.get("entries")) or 0
            raw_reasons = raw_category.get("invalidation_reasons")
            if isinstance(raw_reasons, Mapping):
                for reason, value in raw_reasons.items():
                    invalidation_reasons[str(reason)] += non_negative_int(value) or 0
        merged[category] = {
            "hits": hits,
            "misses": misses,
            "lookups": hits + misses,
            "invalidations": invalidations,
            "invalidation_reasons": dict(sorted(invalidation_reasons.items())),
            "entries": entries,
        }
    return merged


def offset_viewport_event_indices(
    records: Sequence[Mapping[str, object]],
    viewport_events: Sequence[Mapping[str, object]],
    offset: int,
) -> tuple[list[dict[str, object]], list[dict[str, object]]]:
    adjusted_records = [copy.deepcopy(dict(record)) for record in records]
    adjusted_events = [copy.deepcopy(dict(event)) for event in viewport_events]
    if offset <= 0:
        return adjusted_records, adjusted_events

    for event in adjusted_events:
        index = event.get("index")
        if isinstance(index, int):
            event["index"] = index + offset
    for record in adjusted_records:
        for key in ("viewport_apply_event_index", "final_viewport_restore_event_index"):
            index = record.get(key)
            if isinstance(index, int):
                record[key] = index + offset
        indices = record.get("viewport_apply_event_indices")
        if isinstance(indices, list):
            record["viewport_apply_event_indices"] = [
                index + offset if isinstance(index, int) else index
                for index in indices
            ]
    return adjusted_records, adjusted_events


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
    with current_timing_step("foreground_polling"):
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


def launch_scenario(
    adb: str,
    config: TargetConfig,
    scenario: Scenario,
    *,
    stop_app: bool = True,
) -> dict[str, object]:
    command = [
        adb,
        "-s",
        config.serial,
        "shell",
        "am",
        "start",
        "-W",
    ]
    if stop_app:
        command.append("-S")
    command += [
        "-f",
        "0x10008000",
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
            "stop_app": stop_app,
            "stdout": redact_text(output),
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


def is_helper_compatible_profile(config: TargetConfig, profile: ViewportProfile) -> bool:
    return profile.expected_size == config.expected_size


def helper_compatibility_unavailable_reason(
    config: TargetConfig,
    helper_path: str | None,
    helper_compatible_profile: bool,
) -> str | None:
    if not helper_compatible_profile:
        return "viewport profile dimensions do not match the screenshot helper default framebuffer"
    if config.serial != config.default_serial:
        return f"serial {config.serial} does not match helper default serial {config.default_serial}"
    if helper_path is None:
        return f"{config.screenshot_helper} is not available on PATH"
    return None


def capture_screenshot(
    adb: str,
    config: TargetConfig,
    output_path: Path,
    screenshot_mode: str,
    *,
    helper_compatible_profile: bool,
) -> dict[str, object]:
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.unlink(missing_ok=True)
    recorder = current_timing_recorder()
    started = recorder.now() if recorder is not None else time.monotonic()
    helper_path = shutil.which(config.screenshot_helper)
    helper_requested = screenshot_mode == SCREENSHOT_MODE_HELPER_COMPATIBLE
    helper_unavailable_reason = helper_compatibility_unavailable_reason(
        config,
        helper_path,
        helper_compatible_profile,
    )

    if helper_requested and helper_unavailable_reason is None and helper_path is not None:
        command = [helper_path, str(output_path)]
        run_command(command, timeout=120)
        duration_ms = recorder.elapsed_ms_since(started) if recorder is not None else duration_ms_since(started)
        return {
            "method": "nix-screenshot-helper",
            "requested_mode": screenshot_mode,
            "helper_compatibility_mode": True,
            "helper_used": True,
            "command": command,
            "command_category": command_category(command),
            "serial": config.serial,
            "output_path": str(output_path),
            "duration_ms": duration_ms,
        }

    command = [adb, "-s", config.serial, "exec-out", "screencap", "-p"]
    run_command_to_file(command, output_path, timeout=120)
    duration_ms = recorder.elapsed_ms_since(started) if recorder is not None else duration_ms_since(started)
    record: dict[str, object] = {
        "method": "adb-exec-out-screencap",
        "requested_mode": screenshot_mode,
        "helper_compatibility_mode": helper_requested,
        "helper_used": False,
        "command": command,
        "command_category": command_category(command),
        "serial": config.serial,
        "output_path": str(output_path),
        "duration_ms": duration_ms,
    }
    if helper_requested and helper_unavailable_reason is not None:
        record["helper_unavailable_reason"] = helper_unavailable_reason
        record["fallback_method"] = "adb-exec-out-screencap"
    return record


def png_dimensions_from_bytes(data: bytes, context: str) -> PngDimensions:
    header = data[:24]
    if len(header) < 24 or not header.startswith(PNG_SIGNATURE) or header[12:16] != b"IHDR":
        raise VisualQaError(f"{context} is not a valid PNG with IHDR header")
    width, height = struct.unpack(">II", header[16:24])
    return PngDimensions(width=width, height=height)


def screencap_dimensions(adb: str, config: TargetConfig) -> PngDimensions:
    command = [adb, "-s", config.serial, "exec-out", "screencap", "-p"]
    completed = subprocess.run(
        command,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
        timeout=120,
    )
    if completed.returncode != 0:
        stdout = completed.stdout.decode("utf-8", errors="replace").replace("\r", "")
        stderr = completed.stderr.decode("utf-8", errors="replace").replace("\r", "")
        raise CommandError(command, completed.returncode, stdout, stderr)
    return png_dimensions_from_bytes(completed.stdout, f"screencap from {config.serial}")


def wait_for_viewport_profile(adb: str, config: TargetConfig, profile: ViewportProfile) -> dict[str, object]:
    expected = profile.expected_size
    deadline = time.monotonic() + VIEWPORT_SETTLE_TIMEOUT_SECONDS
    attempts: list[dict[str, object]] = []
    while True:
        attempt: dict[str, object] = {"attempt": len(attempts) + 1}
        try:
            dimensions = screencap_dimensions(adb, config)
            attempt["observed_dimensions"] = dimensions.to_json()
            attempts.append(attempt)
            if (dimensions.width, dimensions.height) == expected:
                waited_ms = int((VIEWPORT_SETTLE_TIMEOUT_SECONDS - max(deadline - time.monotonic(), 0)) * 1000)
                return {
                    "profile": profile.name,
                    "expected_dimensions": {"width": expected[0], "height": expected[1]},
                    "waited_ms": waited_ms,
                    "attempts": attempts,
                }
        except Exception as exc:  # noqa: BLE001 - retry transient screencap failures while WM settles.
            attempt["error"] = redact_text(str(exc))
            attempts.append(attempt)
        if time.monotonic() >= deadline:
            break
        if len(attempts) % 4 == 0:
            try:
                set_viewport_profile(adb, config, profile)
                attempt["reapplied_profile"] = True
            except Exception as exc:  # noqa: BLE001 - keep waiting and report recent reapply failures.
                attempt["reapply_error"] = redact_text(str(exc))
        time.sleep(VIEWPORT_SETTLE_RETRY_DELAY_SECONDS)

    recent_attempts = attempts[-8:]
    raise VisualQaError(
        f"viewport {profile.name} did not settle to {expected[0]}x{expected[1]} on {config.serial}: {recent_attempts}"
    )


def validate_png(path: Path, expected_size: tuple[int, int]) -> PngDimensions:
    if not path.exists():
        raise VisualQaError(f"screenshot PNG is missing: {path}")
    size = path.stat().st_size
    if size <= 0:
        raise VisualQaError(f"screenshot PNG is zero bytes: {path}")
    with path.open("rb") as handle:
        header = handle.read(24)
    dimensions = png_dimensions_from_bytes(header, f"screenshot artifact {path}")
    width, height = dimensions.width, dimensions.height
    expected_width, expected_height = expected_size
    if (width, height) != (expected_width, expected_height):
        raise VisualQaError(
            f"screenshot dimensions for {path} were {width}x{height}; expected {expected_width}x{expected_height}"
        )
    return dimensions


def screenshot_attempt_record(
    capture: Mapping[str, object],
    *,
    attempt: int,
    status: str,
    error: str | None = None,
    dimensions: PngDimensions | None = None,
    preserved_path: Path | None = None,
) -> dict[str, object]:
    record = dict(capture)
    record["attempt"] = attempt
    record["status"] = status
    if preserved_path is not None:
        record["original_output_path"] = str(capture.get("output_path") or "")
        record["output_path"] = str(preserved_path)
        record["preserved"] = True
    if dimensions is not None:
        record["dimensions"] = dimensions.to_json()
    if error is not None:
        record["error"] = error
    return record


def preserve_invalid_screenshot_attempt(output_path: Path, attempt: int) -> Path | None:
    if not output_path.exists():
        return None
    preserved_path = output_path.with_name(f"{output_path.stem}.attempt-{attempt}.invalid{output_path.suffix}")
    output_path.replace(preserved_path)
    return preserved_path


def record_viewport_reapply(
    record: dict[str, object],
    *,
    adb: str,
    config: TargetConfig,
    profile: ViewportProfile,
    next_attempt: int,
) -> None:
    reapply_records = record.setdefault("viewport_reapply", [])
    if isinstance(reapply_records, list):
        reapply_records.append(
            {
                "next_attempt": next_attempt,
                **set_viewport_profile(adb, config, profile),
            }
        )


def capture_validated_screenshot(
    *,
    adb: str,
    config: TargetConfig,
    profile: ViewportProfile,
    output_path: Path,
    screenshot_mode: str,
    timing: TimingRecorder,
    record: dict[str, object],
) -> PngDimensions:
    helper_compatible_profile = is_helper_compatible_profile(config, profile)
    validation_attempts: list[dict[str, object]] = []
    for attempt in range(1, SCREENSHOT_VALIDATION_ATTEMPTS + 1):
        with timing.step("screenshot"):
            capture = capture_screenshot(
                adb,
                config,
                output_path,
                screenshot_mode,
                helper_compatible_profile=helper_compatible_profile,
            )
        capture["attempt"] = attempt
        record["screenshot_capture"] = capture
        try:
            with timing.step("png_validation"):
                dimensions = validate_png(output_path, profile.expected_size)
        except VisualQaError as exc:
            error = redact_text(str(exc))
            should_retry = attempt < SCREENSHOT_VALIDATION_ATTEMPTS
            preserved_path = preserve_invalid_screenshot_attempt(output_path, attempt) if should_retry else None
            validation_attempts.append(
                screenshot_attempt_record(
                    capture,
                    attempt=attempt,
                    status="failed",
                    error=error,
                    preserved_path=preserved_path,
                )
            )
            record["screenshot_validation_attempts"] = validation_attempts
            if not should_retry:
                raise
            with timing.step("viewport_reapply"):
                record_viewport_reapply(
                    record,
                    adb=adb,
                    config=config,
                    profile=profile,
                    next_attempt=attempt + 1,
                )
            with timing.step("screenshot_retry_delay"):
                time.sleep(SCREENSHOT_VALIDATION_RETRY_DELAY_SECONDS)
            continue
        validation_attempts.append(
            screenshot_attempt_record(
                capture,
                attempt=attempt,
                status="passed",
                dimensions=dimensions,
            )
        )
        if attempt > 1:
            record["screenshot_validation_attempts"] = validation_attempts
        return dimensions
    raise VisualQaError(f"failed to validate screenshot after {SCREENSHOT_VALIDATION_ATTEMPTS} attempts: {output_path}")


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


def write_json_with_timing(path: Path, data: Mapping[str, object]) -> int:
    started = time.monotonic()
    write_json(path, data)
    return duration_ms_since(started)


def manifest_record_list(manifest: Mapping[str, object], record_key: str) -> list[Mapping[str, object]]:
    raw_records = manifest.get(record_key)
    if not isinstance(raw_records, list):
        return []
    return [record for record in raw_records if isinstance(record, Mapping)]


def write_manifest_with_timing_summary(
    manifest_path: Path,
    manifest: dict[str, object],
    *,
    record_key: str,
    gate_primitives: Sequence[Mapping[str, object]] | None = None,
) -> dict[str, object]:
    records = manifest_record_list(manifest, record_key)
    viewport_events = manifest_record_list(manifest, "viewport_events")
    manifest["viewport_summary"] = aggregate_viewport_events(viewport_events)
    manifest["timing_summary"] = build_timing_summary(
        records,
        gate_primitives=gate_primitives,
        viewport_events=viewport_events,
    )
    manifest_write_ms = write_json_with_timing(manifest_path, manifest)
    manifest["timing_summary"] = build_timing_summary(
        records,
        gate_primitives=gate_primitives,
        manifest_write_ms=manifest_write_ms,
        viewport_events=viewport_events,
    )
    write_json(manifest_path, manifest)
    timing_summary = manifest.get("timing_summary")
    return timing_summary if isinstance(timing_summary, dict) else {}


def print_timing_summary(timing_summary: Mapping[str, object] | None) -> None:
    if not isinstance(timing_summary, Mapping):
        return
    total = non_negative_int(timing_summary.get("total")) or 0
    record_count = non_negative_int(timing_summary.get("record_count")) or 0
    p50 = non_negative_int(timing_summary.get("p50")) or 0
    p95 = non_negative_int(timing_summary.get("p95")) or 0
    max_ms = non_negative_int(timing_summary.get("max")) or 0
    gate_primitives = timing_summary.get("gate_primitives")
    gate_count = len(gate_primitives) if isinstance(gate_primitives, list) else 0
    adb_summary = timing_summary.get("adb_commands")
    adb_count = 0
    if isinstance(adb_summary, Mapping):
        adb_count = non_negative_int(adb_summary.get("total_count")) or 0
    print(
        "android-visual-qa: timings "
        f"total={total}ms records={record_count} p50={p50}ms p95={p95}ms max={max_ms}ms "
        f"gate_steps={gate_count} adb_commands={adb_count}",
        file=sys.stderr,
    )


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


def parse_profile_deferrals(data: Mapping[str, object]) -> set[tuple[str, str]]:
    raw = data.get("profile_deferrals", [])
    if raw in (None, []):
        return set()
    if not isinstance(raw, list):
        raise VisualQaError("profile_deferrals must be a list when provided")

    deferrals: set[tuple[str, str]] = set()
    for index, item in enumerate(raw):
        if not isinstance(item, dict):
            raise VisualQaError(f"profile_deferrals[{index}] must be an object")
        target = item.get("target")
        profile = item.get("profile") or item.get("profile_name")
        reason = item.get("reason") or item.get("rationale")
        human_deferred = item.get("human_deferred") is True or item.get("status") == "human-deferred"
        if target not in REQUIRED_VIEWPORT_PROFILE_NAMES_BY_TARGET:
            raise VisualQaError(f"profile_deferrals[{index}] has invalid target: {target!r}")
        if profile not in REQUIRED_VIEWPORT_PROFILE_NAMES_BY_TARGET[target]:
            raise VisualQaError(f"profile_deferrals[{index}] has invalid profile for {target}: {profile!r}")
        if not human_deferred:
            raise VisualQaError(f"profile_deferrals[{index}] must set human_deferred=true or status='human-deferred'")
        if not isinstance(reason, str) or not reason.strip():
            raise VisualQaError(f"profile_deferrals[{index}] must include a non-empty reason")
        deferrals.add((target, profile))
    return deferrals


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
    seen_profile_pairs: set[tuple[str, str, str]] = set()
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
        if isinstance(profile_name, str):
            profile = VIEWPORT_PROFILES.get(profile_name)
            if profile is None or profile.target != target:
                raise VisualQaError(f"capture record {index} has invalid profile for {target}: {profile_name!r}")
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
        if profile_name is not None:
            seen_profile_pairs.add((target, scenario_id, profile_name))

    if mode is not None:
        if mode not in GATE_MODES:
            raise VisualQaError(f"unknown verify mode {mode!r}; expected smoke or complete")
        registry = ScenarioRegistry.load(repo_root or repo_root_from_script())
        required_scenarios = scenarios_for_gate_mode(registry, mode)
        required_ids = {scenario.id for scenario in required_scenarios}
        missing = sorted(required_ids - seen_ids)
        if missing:
            raise VisualQaError(f"{mode} manifest is missing required scenario(s): {', '.join(missing)}")
        deferrals = parse_profile_deferrals(data)
        missing_profile_captures = []
        for scenario in required_scenarios:
            for profile in REQUIRED_VIEWPORT_PROFILE_NAMES_BY_TARGET[scenario.target]:
                if (scenario.target, scenario.id, profile) not in seen_profile_pairs and (scenario.target, profile) not in deferrals:
                    missing_profile_captures.append(f"{scenario.target}/{profile}/{scenario.id}")
        if missing_profile_captures:
            raise VisualQaError(
                f"{mode} manifest is missing required viewport profile capture(s): "
                + ", ".join(missing_profile_captures[:12])
                + (" ..." if len(missing_profile_captures) > 12 else "")
            )
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
    timing_summary = data.get("timing_summary")
    return ManifestSummary(
        manifest_path=manifest_path,
        output_dir=output_dir,
        mode=mode,
        captures=tuple(verified),
        timing_summary=timing_summary if isinstance(timing_summary, Mapping) else None,
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
    print_timing_summary(summary.timing_summary)


def add_cache_provenance(record: dict[str, object], name: str, provenance: Mapping[str, object]) -> None:
    cache_provenance = record.setdefault("cache_provenance", {})
    if isinstance(cache_provenance, dict):
        cache_provenance[name] = dict(provenance)


def should_invalidate_target_cache(exc: BaseException) -> bool:
    return isinstance(exc, (CommandError, OSError, subprocess.SubprocessError))


def capture_base_record(
    config: TargetConfig,
    scenario: Scenario,
    profile: ViewportProfile,
    output_dir: Path,
    started_at: str,
) -> dict[str, object]:
    screenshot_path = output_dir / profile.name / f"{scenario.id}.png"
    return {
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


def accessibility_base_record(
    config: TargetConfig,
    scenario: Scenario,
    profile: ViewportProfile,
    output_dir: Path,
    started_at: str,
    *,
    max_steps: int,
    exhaustive_dumps: bool,
) -> dict[str, object]:
    dump_path = output_dir / "accessibility" / profile.name / f"{scenario.id}.xml"
    requirements = accessibility_requirements_for_scenario(scenario)
    return {
        "target": config.target,
        "profile": profile.name,
        "scenario_id": scenario.id,
        "serial": config.serial,
        "package_name": config.package,
        "viewport_profile": profile.to_json(),
        "expected_dimensions": {"width": profile.expected_size[0], "height": profile.expected_size[1]},
        "requirements": [requirement.to_json() for requirement in requirements],
        "dump_path": str(dump_path),
        "max_accessibility_steps": max_steps,
        "exhaustive_accessibility_dumps": bool(exhaustive_dumps),
        "started_at": started_at,
        "status": "running",
    }


def ordered_viewport_plans(
    selected: Sequence[Scenario],
    profiles_by_target: Mapping[str, Sequence[ViewportProfile]],
) -> tuple[TargetViewportPlan, ...]:
    targets: list[str] = []
    for scenario in selected:
        if scenario.target in profiles_by_target and scenario.target not in targets:
            targets.append(scenario.target)
    plans: list[TargetViewportPlan] = []
    for target in targets:
        scenarios = tuple(scenario for scenario in selected if scenario.target == target)
        for profile in profiles_by_target.get(target, ()):  # preserve selected profile order per target.
            plans.append(TargetViewportPlan(target=target, profile=profile, scenarios=scenarios))
    return tuple(plans)


def append_viewport_apply_event(
    *,
    adb: str,
    config: TargetConfig,
    profile: ViewportProfile,
    viewport_events: list[dict[str, object]],
    force: bool = False,
) -> tuple[int, dict[str, object]]:
    event_index = len(viewport_events)
    started_at = utc_now()
    started = time.monotonic()
    event: dict[str, object] = {
        "index": event_index,
        "operation": "apply",
        "target": config.target,
        "serial": config.serial,
        "profile": profile.name,
        "started_at": started_at,
        "force": force,
    }
    try:
        evidence = apply_viewport_profile_for_group(adb, config, profile, force=force)
        event.update(evidence.to_json())
        event["status"] = "passed"
    except Exception as exc:  # noqa: BLE001 - caller records affected scenario failures.
        event["status"] = "failed"
        event["error"] = redact_text(str(exc))
        raise
    finally:
        event["ended_at"] = utc_now()
        event["duration_ms"] = duration_ms_since(started)
        viewport_events.append(event)
    return event_index, event


def append_final_viewport_restore_event(
    *,
    adb: str,
    config: TargetConfig,
    initial_snapshot: WmOverrideSnapshot,
    viewport_events: list[dict[str, object]],
) -> dict[str, object]:
    event_index = len(viewport_events)
    started_at = utc_now()
    started = time.monotonic()
    event: dict[str, object] = {
        "index": event_index,
        "operation": "final_restore",
        "target": config.target,
        "serial": config.serial,
        "started_at": started_at,
        "restore_source": "target_initial_snapshot",
        "restore_to": initial_snapshot.to_json(),
    }
    try:
        event.update(restore_viewport_profile(adb, config, initial_snapshot))
        event["status"] = "passed"
    except Exception as exc:  # noqa: BLE001 - manifest must preserve restore failure evidence.
        event["status"] = "failed"
        event["error"] = redact_text(str(exc))
    finally:
        event["ended_at"] = utc_now()
        event["duration_ms"] = duration_ms_since(started)
        viewport_events.append(event)
    return event


def appended_viewport_event_index(viewport_events: Sequence[Mapping[str, object]], event_position: int) -> int | None:
    if event_position >= len(viewport_events):
        return None
    raw_index = viewport_events[event_position].get("index")
    return raw_index if isinstance(raw_index, int) else None


def append_unavailable_final_viewport_restore_event(
    *,
    config: TargetConfig,
    viewport_events: list[dict[str, object]],
    error: BaseException,
) -> dict[str, object]:
    event: dict[str, object] = {
        "index": len(viewport_events),
        "operation": "final_restore",
        "target": config.target,
        "serial": config.serial,
        "started_at": utc_now(),
        "ended_at": utc_now(),
        "duration_ms": 0,
        "restore_source": "unavailable",
        "status": "failed",
        "error": redact_text(f"initial viewport snapshot unavailable; restore not attempted: {error}"),
    }
    viewport_events.append(event)
    return event


def setup_failure_record(
    *,
    adb: str,
    config: TargetConfig,
    scenario: Scenario,
    profile: ViewportProfile,
    output_dir: Path,
    log_lines: int,
    stage: str,
    error: BaseException,
    kind: str,
    viewport_event_index: int | None = None,
    max_steps: int = DEFAULT_ACCESSIBILITY_MAX_STEPS,
    exhaustive_dumps: bool = False,
) -> dict[str, object]:
    started_at = utc_now()
    timing = TimingRecorder()
    total_started = timing.now()
    record = (
        capture_base_record(config, scenario, profile, output_dir, started_at)
        if kind == "capture"
        else accessibility_base_record(
            config,
            scenario,
            profile,
            output_dir,
            started_at,
            max_steps=max_steps,
            exhaustive_dumps=exhaustive_dumps,
        )
    )
    if viewport_event_index is not None:
        record["viewport_apply_event_index"] = viewport_event_index
        record["viewport_apply_event_indices"] = [viewport_event_index]
    failure_log_path = output_dir / "logs" / f"{profile.name}-{scenario.id}-{stage}-logcat.txt"
    with active_timing_recorder(timing):
        record["status"] = "failed"
        record["error"] = redact_text(f"{stage}: {error}")
        with timing.step("failure_logcat"):
            record["failure_logcat"] = capture_failure_logcat(adb, config, failure_log_path, log_lines)
        record["ended_at"] = utc_now()
        record["timings_ms"] = timing.timings_json(timing.elapsed_ms_since(total_started))
        record["adb_command_summary"] = timing.adb_command_summary()
    return record


def mark_target_restore_failure(records: Sequence[dict[str, object]], target: str, restore_event: Mapping[str, object]) -> None:
    if restore_event.get("status") != "failed":
        return
    error = restore_event.get("error")
    message = f"final viewport restore failed: {error}" if isinstance(error, str) else "final viewport restore failed"
    event_index = restore_event.get("index")
    for record in records:
        if record.get("target") != target:
            continue
        record["final_viewport_restore_event_index"] = event_index
        record["viewport_restore_error"] = message
        if record.get("status") == "passed":
            record["status"] = "failed"
            record["error"] = message


def capture_one(
    *,
    adb: str,
    config: TargetConfig,
    scenario: Scenario,
    profile: ViewportProfile,
    output_dir: Path,
    settle_ms: int,
    log_lines: int,
    screenshot_mode: str,
    fast_launch: bool = False,
    run_cache: RunCache | None = None,
    viewport_events: list[dict[str, object]] | None = None,
    viewport_event_index: int | None = None,
) -> dict[str, object]:
    started_at = utc_now()
    timing = TimingRecorder()
    total_started = timing.now()
    screenshot_path = output_dir / profile.name / f"{scenario.id}.png"
    failure_log_path = output_dir / "logs" / f"{profile.name}-{scenario.id}-failure-logcat.txt"
    record = capture_base_record(config, scenario, profile, output_dir, started_at)
    if viewport_event_index is not None:
        record["viewport_apply_event_index"] = viewport_event_index
        record["viewport_apply_event_indices"] = [viewport_event_index]
    viewport_before: WmOverrideSnapshot | None = None
    cache = run_cache or RunCache()

    with active_timing_recorder(timing):
        try:
            with timing.step("serial_readiness"):
                readiness = cache.require_serial_present(adb, config)
            record["serial_readiness"] = readiness.value
            add_cache_provenance(record, "serial_readiness", readiness.provenance)
            if viewport_event_index is None:
                with timing.step("viewport_apply"):
                    viewport_before = apply_viewport_profile(adb, config, profile)
                record["viewport_before"] = viewport_before.to_json()
            with timing.step("viewport_settle"):
                record["viewport_settle"] = wait_for_viewport_profile(adb, config, profile)
            with timing.step("metadata"):
                serial_metadata = cache.serial_metadata(adb, config)
            record["serial_metadata"] = serial_metadata.value
            add_cache_provenance(record, "serial_metadata", serial_metadata.provenance)
            with timing.step("package_metadata"):
                package_metadata = cache.package_metadata(adb, config)
            record["package_metadata"] = package_metadata.value
            add_cache_provenance(record, "package_metadata", package_metadata.provenance)
            record["fast_launch"] = bool(fast_launch)
            if fast_launch:
                record["force_stop_skipped"] = "warm fast launch uses am start without -S while retaining NEW_TASK|CLEAR_TASK"
            else:
                with timing.step("force_stop"):
                    force_stop_package(adb, config)
            with timing.step("launch"):
                record["launch"] = launch_scenario(adb, config, scenario, stop_app=not fast_launch)
            with timing.step("drive"):
                record["dpad_key_events"] = drive_scenario(adb, config, scenario)
            with timing.step("settle"):
                time.sleep(settle_ms / 1000.0)
            with timing.step("viewport_capture_settle"):
                record["viewport_capture_settle"] = wait_for_viewport_profile(adb, config, profile)
            dimensions = capture_validated_screenshot(
                adb=adb,
                config=config,
                profile=profile,
                output_path=screenshot_path,
                screenshot_mode=screenshot_mode,
                timing=timing,
                record=record,
            )
            record["dimensions"] = dimensions.to_json()
            record["status"] = "passed"
        except Exception as exc:  # noqa: BLE001 - capture must emit failure artifacts for any failure.
            if should_invalidate_target_cache(exc):
                cache.invalidate_target(config, "record_failure")
            record["status"] = "failed"
            record["error"] = redact_text(str(exc))
            with timing.step("failure_logcat"):
                record["failure_logcat"] = capture_failure_logcat(adb, config, failure_log_path, log_lines)
        finally:
            if viewport_before is not None:
                try:
                    with timing.step("viewport_restore"):
                        record["viewport_restore"] = restore_viewport_profile(adb, config, viewport_before)
                except Exception as exc:  # noqa: BLE001 - never leave viewport restore failures silent.
                    restore_error = redact_text(str(exc))
                    record["viewport_restore_error"] = restore_error
                    if record.get("status") == "passed":
                        record["status"] = "failed"
                        record["error"] = restore_error
            record["ended_at"] = utc_now()
            record["timings_ms"] = timing.timings_json(timing.elapsed_ms_since(total_started))
            record["adb_command_summary"] = timing.adb_command_summary()
    return record


def capture_reference_for_comparison(record: Mapping[str, object]) -> dict[str, object]:
    screenshot_capture = record.get("screenshot_capture")
    capture_data = dict(screenshot_capture) if isinstance(screenshot_capture, Mapping) else {}
    screenshot_path = str(capture_data.get("output_path") or record.get("screenshot_path") or "")
    reference: dict[str, object] = {
        "method": capture_data.get("method", "unknown"),
        "serial": capture_data.get("serial", record.get("serial", "unknown")),
        "output_path": screenshot_path,
        "screenshot_path": str(record.get("screenshot_path") or screenshot_path),
        "helper_compatibility_mode": bool(capture_data.get("helper_compatibility_mode", False)),
        "helper_used": bool(capture_data.get("helper_used", False)),
    }
    for key in ("requested_mode", "command_category", "duration_ms"):
        if key in capture_data:
            reference[key] = capture_data[key]
    dimensions = record.get("dimensions")
    if isinstance(dimensions, Mapping):
        reference["dimensions"] = dict(dimensions)
    timings = record.get("timings_ms")
    if isinstance(timings, Mapping) and "screenshot" in timings:
        reference["timing_ms"] = timings["screenshot"]
    return reference


def compare_screenshot_methods(
    *,
    adb: str,
    config: TargetConfig,
    scenario: Scenario,
    profile: ViewportProfile,
    output_dir: Path,
    fast_record: Mapping[str, object],
) -> dict[str, object]:
    helper_output = output_dir / "screenshot-method-comparison" / profile.name / f"{scenario.id}-helper-compatible.png"
    helper_path = shutil.which(config.screenshot_helper)
    unavailable_reason = helper_compatibility_unavailable_reason(
        config,
        helper_path,
        is_helper_compatible_profile(config, profile),
    )
    comparison: dict[str, object] = {
        "status": "running",
        "target": config.target,
        "profile": profile.name,
        "scenario_id": scenario.id,
        "serial": config.serial,
        "started_at": utc_now(),
        "fast_capture": capture_reference_for_comparison(fast_record),
        "helper_compatible_output_path": str(helper_output),
    }
    if unavailable_reason is not None:
        comparison["status"] = "unavailable"
        comparison["unavailable_reason"] = unavailable_reason
        comparison["ended_at"] = utc_now()
        return comparison

    timing = TimingRecorder()
    total_started = timing.now()
    viewport_before: WmOverrideSnapshot | None = None
    helper_record: dict[str, object] = {}
    with active_timing_recorder(timing):
        try:
            with timing.step("viewport_apply"):
                viewport_before = apply_viewport_profile(adb, config, profile)
            comparison["viewport_before"] = viewport_before.to_json()
            with timing.step("viewport_settle"):
                comparison["viewport_settle"] = wait_for_viewport_profile(adb, config, profile)
            dimensions = capture_validated_screenshot(
                adb=adb,
                config=config,
                profile=profile,
                output_path=helper_output,
                screenshot_mode=SCREENSHOT_MODE_HELPER_COMPATIBLE,
                timing=timing,
                record=helper_record,
            )
            helper_capture = helper_record.get("screenshot_capture")
            comparison["status"] = "passed"
            comparison["helper_compatible_capture"] = {
                **(dict(helper_capture) if isinstance(helper_capture, Mapping) else {}),
                "dimensions": dimensions.to_json(),
            }
            for key in ("screenshot_validation_attempts", "viewport_reapply"):
                if key in helper_record:
                    comparison[f"helper_compatible_{key}"] = helper_record[key]
        except Exception as exc:  # noqa: BLE001 - comparison evidence should be reported in the manifest.
            comparison["status"] = "failed"
            comparison["error"] = redact_text(str(exc))
            for key in ("screenshot_capture", "screenshot_validation_attempts", "viewport_reapply"):
                if key in helper_record:
                    comparison[f"helper_compatible_{key}"] = helper_record[key]
        finally:
            if viewport_before is not None:
                try:
                    with timing.step("viewport_restore"):
                        comparison["viewport_restore"] = restore_viewport_profile(adb, config, viewport_before)
                except Exception as exc:  # noqa: BLE001 - comparison evidence should include cleanup failures.
                    comparison["viewport_restore_error"] = redact_text(str(exc))
                    if comparison.get("status") == "passed":
                        comparison["status"] = "failed"
                        comparison["error"] = comparison["viewport_restore_error"]
            comparison["ended_at"] = utc_now()
            comparison["timings_ms"] = timing.timings_json(timing.elapsed_ms_since(total_started))
            comparison["adb_command_summary"] = timing.adb_command_summary()
    return comparison


def should_compare_screenshot_methods(mode: str | None, args: argparse.Namespace) -> bool:
    return (
        mode == "smoke"
        and effective_screenshot_mode(args) == SCREENSHOT_MODE_FAST
        and not getattr(args, "no_nix_screenshot", False)
    )


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
                    require_button_role=True,
                    require_enabled=True,
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
                    require_button_role=True,
                    require_enabled=True,
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
                    require_button_role=True,
                    require_enabled=True,
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
                        require_button_role=True,
                        require_enabled=True,
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
            require_clickable=True,
            require_focusable=focus_required,
            require_button_role=True,
            require_enabled=True,
        ),
        AccessibilityRequirement(
            key="theater-media-hero",
            kind="media",
            tag=theater_plate_tag(target, "media", state_key, "hero"),
            content_description_contains="Theater Plate media",
            require_content_description=True,
            require_clickable=True,
            require_focusable=focus_required,
            require_button_role=True,
            require_enabled=True,
        ),
        AccessibilityRequirement(
            key="theater-rail",
            kind="media-rail",
            tag=theater_plate_tag(target, "rail", state_key, "primary"),
            require_content_description=True,
        ),
    ]
    if state_key in {"stale-offline", "recovery"}:
        for action_key, action_label in THEATER_PLATE_RECOVERY_ACTIONS:
            if target == "phone" and action_key == "retry":
                # Retry is already exposed as the primary action on phone Theater Plate recovery
                # states; keep the supporting action set free of duplicate labels.
                continue
            requirements.append(
                AccessibilityRequirement(
                    key=f"theater-recovery-{action_key}",
                    kind="recovery-action",
                    tag=theater_plate_tag(target, "action", state_key, action_key),
                    content_description=action_label,
                    require_clickable=True,
                    require_focusable=focus_required,
                    require_button_role=True,
                    require_enabled=True,
                )
            )
    if state_key == "search":
        requirements.append(
            AccessibilityRequirement(
                key="theater-search-field",
                kind="action",
                tag=theater_plate_tag(target, "search", state_key, "field"),
                content_description="Search Theater Plate",
                require_clickable=True,
                require_focusable=focus_required,
                require_button_role=True,
                require_enabled=True,
            )
        )
    if state_key == "playback-entry":
        requirements.extend(
            [
                AccessibilityRequirement(
                    key="theater-playback-entry",
                    kind="action",
                    tag=theater_plate_tag(target, "action", state_key, "primary"),
                    content_description="Resume playback",
                    require_clickable=True,
                    require_focusable=focus_required,
                    require_button_role=True,
                    require_enabled=True,
                ),
                AccessibilityRequirement(
                    key="theater-playback-disabled-network",
                    kind="disabled-action",
                    tag=theater_plate_tag(target, "action", state_key, "network-required"),
                    content_description="Network playback requires a playback ticket",
                    require_button_role=True,
                    require_disabled=True,
                ),
            ]
        )
    return requirements


def accessibility_requirements_for_scenario(scenario: Scenario) -> list[AccessibilityRequirement]:
    state_key = theater_plate_state_key(scenario.id)
    if state_key is not None:
        return theater_plate_accessibility_requirements(scenario, state_key)
    return legacy_accessibility_requirements(scenario)


def scenario_root_tag(requirements: Sequence[AccessibilityRequirement]) -> str | None:
    for requirement in requirements:
        if requirement.key in {"root-tag", "theater-root"} and requirement.tag:
            return requirement.tag
    return None


def accessibility_dump_strategy(
    config: TargetConfig,
    serial_metadata: Mapping[str, object] | None,
) -> AccessibilityDumpStrategy:
    expected_sdk = VALIDATED_BATCHED_ACCESSIBILITY_DUMP_SDK.get(config.target)
    if config.serial != config.default_serial:
        return AccessibilityDumpStrategy(
            name="safe-sequence",
            batched=False,
            reason="non-default serial; using the proven dump/cat/rm sequence",
        )

    properties = serial_metadata.get("properties") if isinstance(serial_metadata, Mapping) else None
    sdk = properties.get("sdk") if isinstance(properties, Mapping) else None
    normalized_sdk = str(sdk).strip() if sdk is not None else ""
    if expected_sdk is None or normalized_sdk != expected_sdk:
        return AccessibilityDumpStrategy(
            name="safe-sequence",
            batched=False,
            reason=(
                f"default {config.target} serial reported API {normalized_sdk or 'unknown'}; "
                f"batched dumps are validated for API {expected_sdk or 'unknown'}"
            ),
        )

    return AccessibilityDumpStrategy(
        name="batched-shell",
        batched=True,
        reason=f"validated default {config.target} serial on API {normalized_sdk}",
    )


def accessibility_remote_dump_path(attempt: int | None = None) -> str:
    suffix = f"{os.getpid()}-{time.monotonic_ns()}"
    if attempt is not None:
        suffix = f"{suffix}-{attempt}"
    return f"/sdcard/ferrex-visual-qa-accessibility-{suffix}.xml"


def accessibility_dump_validation_error(xml_text: str, expected_text: str | None) -> str | None:
    if "<hierarchy" not in xml_text:
        return f"dump did not contain a UI hierarchy: {bounded_lines(redact_text(xml_text), 4)}"
    if expected_text is not None and expected_text not in xml_text:
        marker_match = re.search(r'Ferrex visual QA • [^"<]+', xml_text)
        observed_marker = marker_match.group(0) if marker_match else "<missing>"
        return f"dump did not contain expected scenario marker {expected_text!r}; observed {observed_marker!r}"
    return None


def should_relaunch_after_accessibility_dump_error(message: str) -> bool:
    return any(
        marker in message
        for marker in (
            "expected scenario marker",
            "uiautomator dump exited 137",
            "dump file missing/unreadable",
            "accessibility dump did not stabilize",
        )
    )


def dump_accessibility_xml_safe(
    adb: str,
    config: TargetConfig,
    remote_path: str | None = None,
    *,
    expected_text: str | None = None,
) -> str:
    errors: list[str] = []
    for attempt in range(1, ACCESSIBILITY_DUMP_ATTEMPTS + 1):
        attempt_remote_path = remote_path if remote_path is not None and attempt == 1 else accessibility_remote_dump_path(attempt)
        dump_output = ""
        try:
            adb_shell(adb, config.serial, "rm", "-f", attempt_remote_path, check=False, timeout=30)
            dump = adb_shell(
                adb,
                config.serial,
                "uiautomator",
                "dump",
                "--compressed",
                attempt_remote_path,
                check=False,
                timeout=90,
            )
            dump_output = (dump.stdout + "\n" + dump.stderr).strip()
            if dump.returncode != 0:
                errors.append(
                    f"attempt {attempt}: uiautomator dump exited {dump.returncode}: "
                    f"{bounded_lines(redact_text(dump_output), 4)}"
                )
            else:
                xml_result = adb_shell(adb, config.serial, "cat", attempt_remote_path, check=False, timeout=30)
                xml_text = xml_result.stdout
                if xml_result.returncode != 0:
                    cat_output = (xml_result.stdout + "\n" + xml_result.stderr).strip()
                    errors.append(
                        f"attempt {attempt}: dump file missing/unreadable: "
                        f"{bounded_lines(redact_text(cat_output or dump_output), 4)}"
                    )
                else:
                    validation_error = accessibility_dump_validation_error(xml_text, expected_text)
                    if validation_error is None:
                        return xml_text
                    errors.append(f"attempt {attempt}: {validation_error}")
        except CommandError as exc:
            errors.append(f"attempt {attempt}: {bounded_lines(redact_text(str(exc)), 4)}")
        finally:
            adb_shell(adb, config.serial, "rm", "-f", attempt_remote_path, check=False, timeout=30)
        if attempt < ACCESSIBILITY_DUMP_ATTEMPTS:
            time.sleep(ACCESSIBILITY_DUMP_RETRY_DELAY_SECONDS)

    recent_errors = "\n".join(errors[-ACCESSIBILITY_DUMP_ATTEMPTS:]) or "no dump attempts completed"
    raise VisualQaError(f"accessibility dump did not stabilize for {config.serial}:\n{recent_errors}")


def dump_accessibility_xml_batched(adb: str, config: TargetConfig, remote_path: str) -> str:
    script = (
        f"remote='{remote_path}'; "
        'uiautomator dump --compressed "$remote" >/dev/null 2>&1; '
        "dump_status=$?; "
        'if [ "$dump_status" -eq 0 ]; then '
        'cat "$remote"; cat_status=$?; rm -f "$remote"; exit "$cat_status"; '
        'else rm -f "$remote"; exit "$dump_status"; fi'
    )
    return adb_shell(adb, config.serial, f"sh -c {shlex.quote(script)}", timeout=120).stdout


def dump_accessibility_xml_result(
    adb: str,
    config: TargetConfig,
    strategy: AccessibilityDumpStrategy | None = None,
    *,
    expected_text: str | None = None,
) -> AccessibilityXmlDump:
    remote_path = accessibility_remote_dump_path()
    selected = strategy or AccessibilityDumpStrategy(
        name="safe-sequence",
        batched=False,
        reason="default safe sequence",
    )
    if selected.batched:
        try:
            xml_text = dump_accessibility_xml_batched(adb, config, remote_path)
            validation_error = accessibility_dump_validation_error(xml_text, expected_text)
            if validation_error is not None:
                raise VisualQaError(validation_error)
            return AccessibilityXmlDump(xml_text=xml_text, command_strategy=selected.name)
        except Exception as exc:  # noqa: BLE001 - fall back to the long-standing safe sequence.
            return AccessibilityXmlDump(
                xml_text=dump_accessibility_xml_safe(adb, config, remote_path, expected_text=expected_text),
                command_strategy="safe-sequence",
                fallback_reason=f"{selected.name} failed; retried with safe sequence: {redact_text(str(exc))}",
            )
    return AccessibilityXmlDump(
        xml_text=dump_accessibility_xml_safe(adb, config, remote_path, expected_text=expected_text),
        command_strategy=selected.name,
    )


def dump_accessibility_xml(adb: str, config: TargetConfig, expected_text: str | None = None) -> str:
    return dump_accessibility_xml_result(adb, config, expected_text=expected_text).xml_text


def dump_accessibility_nodes(
    adb: str,
    config: TargetConfig,
    root_tag: str | None,
    strategy: AccessibilityDumpStrategy | None = None,
    *,
    expected_text: str | None = None,
) -> tuple[AccessibilityXmlDump, list[dict[str, str]], int]:
    attempts = 5
    last_dump: AccessibilityXmlDump | None = None
    last_nodes: list[dict[str, str]] = []
    for attempt in range(1, attempts + 1):
        dump_result = dump_accessibility_xml_result(adb, config, strategy, expected_text=expected_text)
        nodes = parse_accessibility_nodes(dump_result.xml_text)
        if root_tag is None or any(node_has_tag(node, root_tag) for node in nodes):
            return dump_result, nodes, attempt
        last_dump = dump_result
        last_nodes = nodes
        time.sleep(0.5)
    if root_tag is not None:
        raise VisualQaError(f"accessibility dump did not include scenario root tag after {attempts} attempt(s): {root_tag}")
    if last_dump is not None:
        return last_dump, last_nodes, attempts
    raise VisualQaError("accessibility dump did not return any XML")


def drive_accessibility_reachability_step(adb: str, config: TargetConfig, profile: ViewportProfile) -> None:
    width, height = profile.expected_size
    if config.target == "tv":
        for _ in range(2):
            adb_shell(adb, config.serial, "input", "keyevent", "KEYCODE_DPAD_DOWN", timeout=30)
            time.sleep(0.05)
        time.sleep(0.25)
        return

    for _ in range(2):
        adb_shell(
            adb,
            config.serial,
            "input",
            "swipe",
            str(width // 2),
            str(int(height * 0.88)),
            str(width // 2),
            str(int(height * 0.12)),
            "350",
            timeout=30,
        )
        time.sleep(0.15)
    adb_shell(adb, config.serial, "input", "keyevent", "KEYCODE_PAGE_DOWN", check=False, timeout=30)
    time.sleep(0.25)


def hide_soft_keyboard_if_visible(adb: str, config: TargetConfig) -> list[str]:
    if config.target != "phone":
        return []
    dump = adb_shell(adb, config.serial, "dumpsys", "input_method", check=False, timeout=30)
    if dump.returncode != 0:
        return ["input-method-dumpsys-unavailable"]
    if not re.search(r"(?m)\bm(?:InputShown|IsInputViewShown)=true\b|\binputShown=true\b", dump.stdout):
        return []
    adb_shell(adb, config.serial, "input", "keyevent", "KEYCODE_BACK", timeout=30)
    time.sleep(0.25)
    return ["KEYCODE_BACK"]


def refresh_accessibility_focus(adb: str, config: TargetConfig) -> list[str]:
    if config.target == "phone":
        adb_shell(adb, config.serial, "input", "tap", "8", "80", timeout=30)
        time.sleep(0.1)
        return ["tap:8,80"]
    if config.target != "tv":
        return []
    keys = ["KEYCODE_DPAD_UP", "KEYCODE_DPAD_LEFT"]
    for key in keys:
        adb_shell(adb, config.serial, "input", "keyevent", key, timeout=30)
        time.sleep(0.1)
    return keys


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
    if requirement.require_button_role:
        class_name = node.get("class", "") or node.get("className", "")
        if (
            "Button" not in class_name
            and node.get("clickable") != "true"
            and node.get("_ancestor_clickable") != "true"
            and node.get("focusable") != "true"
            and node.get("enabled") != "false"
        ):
            return False
    if requirement.require_clickable and node.get("clickable") != "true" and node.get("_ancestor_clickable") != "true":
        if not (requirement.require_button_role and node.get("focusable") == "true"):
            return False
    if requirement.require_focusable and node.get("focusable") != "true" and node.get("_ancestor_focusable") != "true":
        return False
    if requirement.require_enabled and node.get("enabled") == "false":
        return False
    if requirement.require_disabled and node.get("enabled") != "false":
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


def accessibility_requirement_key(check: Mapping[str, object]) -> str:
    requirement = check.get("requirement")
    if isinstance(requirement, Mapping):
        key = requirement.get("key")
        if isinstance(key, str):
            return key
    return "unknown"


def accessibility_requirement_status(checks: Sequence[Mapping[str, object]]) -> dict[str, str]:
    return {accessibility_requirement_key(check): str(check.get("status") or "unknown") for check in checks}


def passed_accessibility_requirement_keys(checks: Sequence[Mapping[str, object]]) -> set[str]:
    return {accessibility_requirement_key(check) for check in checks if check.get("status") == "passed"}


def failed_accessibility_requirement_keys(checks: Sequence[Mapping[str, object]]) -> list[str]:
    return [accessibility_requirement_key(check) for check in checks if check.get("status") != "passed"]


def all_accessibility_requirements_passed(checks: Sequence[Mapping[str, object]]) -> bool:
    return bool(checks) and not failed_accessibility_requirement_keys(checks)


def accessibility_node_fingerprint(node: Mapping[str, str]) -> tuple[tuple[str, str], ...]:
    fields = (
        "resource-id",
        "class",
        "text",
        "content-desc",
        "clickable",
        "focusable",
        "enabled",
        "selected",
        "scrollable",
        "bounds",
    )
    return tuple((field, node.get(field, "")) for field in fields)


def accessibility_one(
    *,
    adb: str,
    config: TargetConfig,
    scenario: Scenario,
    profile: ViewportProfile,
    output_dir: Path,
    settle_ms: int,
    log_lines: int,
    max_steps: int = DEFAULT_ACCESSIBILITY_MAX_STEPS,
    exhaustive_dumps: bool = False,
    run_cache: RunCache | None = None,
    viewport_events: list[dict[str, object]] | None = None,
    viewport_event_index: int | None = None,
) -> dict[str, object]:
    started_at = utc_now()
    timing = TimingRecorder()
    total_started = timing.now()
    dump_path = output_dir / "accessibility" / profile.name / f"{scenario.id}.xml"
    failure_log_path = output_dir / "logs" / f"{profile.name}-{scenario.id}-accessibility-logcat.txt"
    requirements = accessibility_requirements_for_scenario(scenario)
    max_steps = max(1, int(max_steps))
    record = accessibility_base_record(
        config,
        scenario,
        profile,
        output_dir,
        started_at,
        max_steps=max_steps,
        exhaustive_dumps=exhaustive_dumps,
    )
    if viewport_event_index is not None:
        record["viewport_apply_event_index"] = viewport_event_index
        record["viewport_apply_event_indices"] = [viewport_event_index]
    viewport_before: WmOverrideSnapshot | None = None
    cache = run_cache or RunCache()
    with active_timing_recorder(timing):
        try:
            if not requirements:
                raise VisualQaError(f"no accessibility requirements registered for scenario {scenario.id}")
            with timing.step("serial_readiness"):
                readiness = cache.require_serial_present(adb, config)
            record["serial_readiness"] = readiness.value
            add_cache_provenance(record, "serial_readiness", readiness.provenance)
            if viewport_event_index is None:
                with timing.step("viewport_apply"):
                    viewport_before = apply_viewport_profile(adb, config, profile)
                record["viewport_before"] = viewport_before.to_json()
            with timing.step("viewport_settle"):
                record["viewport_settle"] = wait_for_viewport_profile(adb, config, profile)
            with timing.step("metadata"):
                serial_metadata = cache.serial_metadata(adb, config)
            record["serial_metadata"] = serial_metadata.value
            add_cache_provenance(record, "serial_metadata", serial_metadata.provenance)
            dump_strategy = accessibility_dump_strategy(config, serial_metadata.value)
            record["accessibility_dump_strategy"] = dump_strategy.to_json()
            with timing.step("package_metadata"):
                package_metadata = cache.package_metadata(adb, config)
            record["package_metadata"] = package_metadata.value
            add_cache_provenance(record, "package_metadata", package_metadata.provenance)
            with timing.step("force_stop"):
                force_stop_package(adb, config)
            with timing.step("prelaunch_settle"):
                time.sleep(1.0 if config.target == "tv" else 0.25)
            with timing.step("launch"):
                record["launch"] = launch_scenario(adb, config, scenario)
            with timing.step("drive"):
                record["dpad_key_events"] = []
            target_settle_ms = TV_ACCESSIBILITY_SETTLE_MS if config.target == "tv" else 2500
            with timing.step("settle"):
                time.sleep(max(settle_ms, target_settle_ms) / 1000.0)
            try:
                with timing.step("viewport_accessibility_settle"):
                    record["viewport_accessibility_settle"] = wait_for_viewport_profile(adb, config, profile)
            except VisualQaError as exc:
                record["viewport_accessibility_initial_settle_error"] = redact_text(str(exc))
                with timing.step("viewport_reapply"):
                    reapply_record = {"next_attempt": 1, **set_viewport_profile(adb, config, profile)}
                record["viewport_reapply"] = [reapply_record]
                with timing.step("viewport_accessibility_settle"):
                    record["viewport_accessibility_settle"] = wait_for_viewport_profile(adb, config, profile)
            with timing.step("soft_keyboard"):
                keyboard_events = hide_soft_keyboard_if_visible(adb, config)
            if keyboard_events:
                record["soft_keyboard_events"] = keyboard_events
            with timing.step("accessibility_focus_refresh"):
                record["dpad_key_events"] = refresh_accessibility_focus(adb, config)

            dump_path.parent.mkdir(parents=True, exist_ok=True)
            nodes: list[dict[str, str]] = []
            dump_paths: list[str] = []
            dump_attempts: list[int] = []
            root_tag = scenario_root_tag(requirements)
            expected_marker = f"Ferrex visual QA • {scenario.id}"
            steps: list[dict[str, object]] = []
            seen_node_fingerprints: set[tuple[tuple[str, str], ...]] = set()
            previous_passed_requirement_keys: set[str] = set()
            no_progress_streak = 0
            final_checks: list[dict[str, object]] = []
            record["dump_paths"] = dump_paths
            record["dump_attempts"] = dump_attempts
            record["accessibility_steps"] = steps
            record["dump_command_strategies_used"] = []
            for step in range(max_steps):
                step_path = accessibility_dump_path(dump_path, step)
                step_timings: dict[str, int] = {}
                step_record: dict[str, object] = {
                    "step": step,
                    "dump_path": str(step_path),
                    "timings_ms": step_timings,
                }
                step_started = timing.now()
                steps.append(step_record)

                dump_result: AccessibilityXmlDump | None = None
                step_nodes: list[dict[str, str]] = []
                attempts = 0
                dump_relaunches: list[dict[str, object]] = []
                dump_started = timing.now()
                with timing.step("accessibility_dump"):
                    for relaunch_attempt in range(ACCESSIBILITY_DUMP_RELAUNCH_ATTEMPTS + 1):
                        try:
                            dump_result, step_nodes, attempts = dump_accessibility_nodes(
                                adb,
                                config,
                                root_tag,
                                dump_strategy,
                                expected_text=expected_marker,
                            )
                            break
                        except VisualQaError as exc:
                            error = redact_text(str(exc))
                            if (
                                relaunch_attempt >= ACCESSIBILITY_DUMP_RELAUNCH_ATTEMPTS
                                or not should_relaunch_after_accessibility_dump_error(error)
                            ):
                                raise
                            relaunch_record: dict[str, object] = {
                                "attempt": relaunch_attempt + 1,
                                "reason": error,
                            }
                            force_stop_package(adb, config)
                            time.sleep(1.0 if config.target == "tv" else 0.5)
                            relaunch_record["launch"] = launch_scenario(adb, config, scenario)
                            time.sleep((TV_ACCESSIBILITY_SETTLE_MS if config.target == "tv" else 2500) / 1000.0)
                            dump_relaunches.append(relaunch_record)
                            step_record["dump_relaunches"] = dump_relaunches
                step_timings["dump"] = timing.elapsed_ms_since(dump_started)
                if dump_result is None:
                    raise VisualQaError("accessibility dump did not return a result")
                xml_text = dump_result.xml_text
                step_path.write_text(redact_xml_text(xml_text), encoding="utf-8")
                dump_paths.append(str(step_path))
                dump_attempts.append(attempts)
                step_record["dump_attempts"] = attempts
                step_record["dump_command_strategy"] = dump_result.command_strategy
                if dump_result.fallback_reason is not None:
                    step_record["dump_fallback_reason"] = dump_result.fallback_reason
                record["dump_command_strategies_used"] = sorted(
                    {
                        str(recorded_step["dump_command_strategy"])
                        for recorded_step in steps
                        if "dump_command_strategy" in recorded_step
                    }
                )

                step_fingerprints = {accessibility_node_fingerprint(node) for node in step_nodes}
                new_node_fingerprints = step_fingerprints - seen_node_fingerprints
                seen_node_fingerprints.update(step_fingerprints)
                nodes.extend(step_nodes)
                record["node_count"] = len(nodes)
                record["unique_node_count"] = len(seen_node_fingerprints)
                step_record["node_count"] = len(step_nodes)
                step_record["unique_node_count"] = len(step_fingerprints)
                step_record["new_node_count"] = len(new_node_fingerprints)
                step_record["cumulative_node_count"] = len(nodes)
                step_record["cumulative_unique_node_count"] = len(seen_node_fingerprints)

                verify_started = timing.now()
                with timing.step("accessibility_verify"):
                    checks = verify_accessibility_requirements(nodes, requirements)
                step_timings["verify"] = timing.elapsed_ms_since(verify_started)
                final_checks = checks
                record["checks"] = checks
                passed_keys = passed_accessibility_requirement_keys(checks)
                failed_keys = failed_accessibility_requirement_keys(checks)
                newly_passed_keys = sorted(passed_keys - previous_passed_requirement_keys)
                previous_passed_requirement_keys = set(passed_keys)
                step_record["requirement_status"] = accessibility_requirement_status(checks)
                step_record["checks"] = checks
                step_record["passed_requirement_count"] = len(passed_keys)
                step_record["failed_requirement_keys"] = failed_keys
                step_record["newly_passed_requirement_keys"] = newly_passed_keys
                step_record["all_requirements_passed"] = all_accessibility_requirements_passed(checks)

                no_new_nodes = step > 0 and not new_node_fingerprints
                no_requirement_progress = step > 0 and not newly_passed_keys
                no_progress = no_new_nodes and no_requirement_progress
                no_progress_streak = no_progress_streak + 1 if no_progress else 0
                step_record["no_new_nodes"] = no_new_nodes
                step_record["no_requirement_progress"] = no_requirement_progress
                step_record["no_progress_streak"] = no_progress_streak

                if all_accessibility_requirements_passed(checks) and "first_all_requirements_passed_step" not in record:
                    record["first_all_requirements_passed_step"] = step

                stop_reason: str | None = None
                if all_accessibility_requirements_passed(checks) and not exhaustive_dumps:
                    stop_reason = "all_requirements_passed"
                    if step < max_steps - 1:
                        record["early_stop_reason"] = stop_reason
                        step_record["early_stop_reason"] = stop_reason
                elif no_progress_streak >= ACCESSIBILITY_NO_PROGRESS_STOP_STREAK and not exhaustive_dumps and step < max_steps - 1:
                    stop_reason = "no_progress"
                    record["early_stop_reason"] = stop_reason
                    step_record["early_stop_reason"] = stop_reason
                elif step >= max_steps - 1:
                    stop_reason = "max_steps"

                if stop_reason is not None:
                    record["stop_reason"] = stop_reason
                    step_record["stop_reason"] = stop_reason
                    step_timings["total"] = timing.elapsed_ms_since(step_started)
                    break

                drive_started = timing.now()
                with timing.step("accessibility_drive"):
                    drive_accessibility_reachability_step(adb, config, profile)
                step_timings["drive"] = timing.elapsed_ms_since(drive_started)
                step_timings["total"] = timing.elapsed_ms_since(step_started)

            if not final_checks:
                raise VisualQaError("no accessibility dumps captured")
            failures = [check for check in final_checks if check["status"] != "passed"]
            if failures:
                missing = ", ".join(failed_accessibility_requirement_keys(final_checks))
                raise VisualQaError(f"missing accessibility requirement(s): {missing}")
            record["status"] = "passed"
        except Exception as exc:  # noqa: BLE001 - accessibility gate must emit diagnostics.
            if should_invalidate_target_cache(exc):
                cache.invalidate_target(config, "record_failure")
            record["status"] = "failed"
            record["error"] = redact_text(str(exc))
            with timing.step("failure_logcat"):
                record["failure_logcat"] = capture_failure_logcat(adb, config, failure_log_path, log_lines)
        finally:
            if viewport_before is not None:
                try:
                    with timing.step("viewport_restore"):
                        record["viewport_restore"] = restore_viewport_profile(adb, config, viewport_before)
                except Exception as exc:  # noqa: BLE001
                    restore_error = redact_text(str(exc))
                    record["viewport_restore_error"] = restore_error
                    if record.get("status") == "passed":
                        record["status"] = "failed"
                        record["error"] = restore_error
            record["ended_at"] = utc_now()
            record["timings_ms"] = timing.timings_json(timing.elapsed_ms_since(total_started))
            record["adb_command_summary"] = timing.adb_command_summary()
    return record



def run_accessibility_target_plans(
    *,
    adb: str,
    config: TargetConfig,
    target_plans: Sequence[TargetViewportPlan],
    args: argparse.Namespace,
    output_dir: Path,
) -> TargetRunResult:
    target = config.target
    worker_started_at = utc_now()
    worker_started = time.monotonic()
    run_cache = RunCache()
    records: list[dict[str, object]] = []
    viewport_events: list[dict[str, object]] = []
    initial_snapshot: WmOverrideSnapshot | None = None
    try:
        run_cache.require_serial_present(adb, config)
        initial_snapshot = wm_snapshot(adb, config.serial)
    except Exception as exc:  # noqa: BLE001 - emit per-scenario diagnostics instead of aborting the manifest.
        if should_invalidate_target_cache(exc):
            run_cache.invalidate_target(config, "viewport_setup")
        for plan in target_plans:
            for scenario in plan.scenarios:
                records.append(
                    setup_failure_record(
                        adb=adb,
                        config=config,
                        scenario=scenario,
                        profile=plan.profile,
                        output_dir=output_dir,
                        log_lines=args.log_lines,
                        stage="viewport_snapshot",
                        error=exc,
                        kind="accessibility",
                        max_steps=args.max_steps,
                        exhaustive_dumps=args.exhaustive_dumps,
                    )
                )
        restore_event = append_unavailable_final_viewport_restore_event(
            config=config,
            viewport_events=viewport_events,
            error=exc,
        )
        for record in records:
            record["final_viewport_restore_event_index"] = restore_event.get("index")
        mark_target_restore_failure(records, target, restore_event)
    else:
        try:
            for plan in target_plans:
                apply_event_position = len(viewport_events)
                try:
                    viewport_event_index, viewport_event = append_viewport_apply_event(
                        adb=adb,
                        config=config,
                        profile=plan.profile,
                        viewport_events=viewport_events,
                    )
                    viewport_event["record_count"] = len(plan.scenarios)
                except Exception as exc:  # noqa: BLE001 - emit one failure record for every skipped check.
                    if should_invalidate_target_cache(exc):
                        run_cache.invalidate_target(config, "viewport_apply")
                    failed_event_index = appended_viewport_event_index(viewport_events, apply_event_position)
                    for scenario in plan.scenarios:
                        records.append(
                            setup_failure_record(
                                adb=adb,
                                config=config,
                                scenario=scenario,
                                profile=plan.profile,
                                output_dir=output_dir,
                                log_lines=args.log_lines,
                                stage="viewport_apply",
                                error=exc,
                                kind="accessibility",
                                viewport_event_index=failed_event_index,
                                max_steps=args.max_steps,
                                exhaustive_dumps=args.exhaustive_dumps,
                            )
                        )
                    continue
                for scenario in plan.scenarios:
                    print(
                        f"android-visual-qa: accessibility {scenario.id} on {scenario.target}/{plan.profile.name} ({config.serial})",
                        file=sys.stderr,
                    )
                    record = accessibility_one(
                        adb=adb,
                        config=config,
                        scenario=scenario,
                        profile=plan.profile,
                        output_dir=output_dir,
                        settle_ms=args.settle_ms,
                        log_lines=args.log_lines,
                        max_steps=args.max_steps,
                        exhaustive_dumps=args.exhaustive_dumps,
                        run_cache=run_cache,
                        viewport_events=viewport_events,
                        viewport_event_index=viewport_event_index,
                    )
                    records.append(record)
                    if record["status"] == "passed":
                        print(f"android-visual-qa: accessibility passed {plan.profile.name}/{scenario.id}", file=sys.stderr)
                    else:
                        print(
                            f"android-visual-qa: accessibility FAILED {plan.profile.name}/{scenario.id}: {record['error']}",
                            file=sys.stderr,
                        )
        finally:
            restore_event = append_final_viewport_restore_event(
                adb=adb,
                config=config,
                initial_snapshot=initial_snapshot,
                viewport_events=viewport_events,
            )
            for record in records:
                record["final_viewport_restore_event_index"] = restore_event.get("index")
            mark_target_restore_failure(records, target, restore_event)

    failure_count = sum(1 for record in records if record.get("status") != "passed")
    worker_timing: dict[str, object] = {
        "target": target,
        "serial": config.serial,
        "started_at": worker_started_at,
        "ended_at": utc_now(),
        "duration_ms": duration_ms_since(worker_started),
        "status": "failed" if failure_count else "passed",
        "record_count": len(records),
        "failure_count": failure_count,
        "viewport_event_count": len(viewport_events),
    }
    return TargetRunResult(
        target=target,
        records=tuple(records),
        viewport_events=tuple(viewport_events),
        cache_summary=run_cache.summary(),
        worker_timing=worker_timing,
    )


def run_capture_target_plans(
    *,
    adb: str,
    config: TargetConfig,
    target_plans: Sequence[TargetViewportPlan],
    args: argparse.Namespace,
    output_dir: Path,
    screenshot_mode: str,
    compare_methods: bool,
) -> TargetRunResult:
    target = config.target
    worker_started_at = utc_now()
    worker_started = time.monotonic()
    run_cache = RunCache()
    captures: list[dict[str, object]] = []
    viewport_events: list[dict[str, object]] = []
    screenshot_method_comparison: dict[str, object] | None = None
    initial_snapshot: WmOverrideSnapshot | None = None
    try:
        run_cache.require_serial_present(adb, config)
        initial_snapshot = wm_snapshot(adb, config.serial)
    except Exception as exc:  # noqa: BLE001 - emit per-scenario diagnostics instead of aborting the manifest.
        if should_invalidate_target_cache(exc):
            run_cache.invalidate_target(config, "viewport_setup")
        for plan in target_plans:
            for scenario in plan.scenarios:
                captures.append(
                    setup_failure_record(
                        adb=adb,
                        config=config,
                        scenario=scenario,
                        profile=plan.profile,
                        output_dir=output_dir,
                        log_lines=args.log_lines,
                        stage="viewport_snapshot",
                        error=exc,
                        kind="capture",
                    )
                )
        restore_event = append_unavailable_final_viewport_restore_event(
            config=config,
            viewport_events=viewport_events,
            error=exc,
        )
        for record in captures:
            record["final_viewport_restore_event_index"] = restore_event.get("index")
        mark_target_restore_failure(captures, target, restore_event)
    else:
        try:
            for plan in target_plans:
                apply_event_position = len(viewport_events)
                try:
                    viewport_event_index, viewport_event = append_viewport_apply_event(
                        adb=adb,
                        config=config,
                        profile=plan.profile,
                        viewport_events=viewport_events,
                    )
                    viewport_event["record_count"] = len(plan.scenarios)
                except Exception as exc:  # noqa: BLE001 - emit one failure record for every skipped capture.
                    if should_invalidate_target_cache(exc):
                        run_cache.invalidate_target(config, "viewport_apply")
                    failed_event_index = appended_viewport_event_index(viewport_events, apply_event_position)
                    for scenario in plan.scenarios:
                        captures.append(
                            setup_failure_record(
                                adb=adb,
                                config=config,
                                scenario=scenario,
                                profile=plan.profile,
                                output_dir=output_dir,
                                log_lines=args.log_lines,
                                stage="viewport_apply",
                                error=exc,
                                kind="capture",
                                viewport_event_index=failed_event_index,
                            )
                        )
                    continue
                for scenario in plan.scenarios:
                    print(
                        f"android-visual-qa: capture {scenario.id} on {scenario.target}/{plan.profile.name} ({config.serial})",
                        file=sys.stderr,
                    )
                    record = capture_one(
                        adb=adb,
                        config=config,
                        scenario=scenario,
                        profile=plan.profile,
                        output_dir=output_dir,
                        settle_ms=args.settle_ms,
                        log_lines=args.log_lines,
                        screenshot_mode=screenshot_mode,
                        fast_launch=bool(getattr(args, "warm_fast_launch", False)),
                        run_cache=run_cache,
                        viewport_events=viewport_events,
                        viewport_event_index=viewport_event_index,
                    )
                    captures.append(record)
                    if record["status"] == "passed":
                        print(f"android-visual-qa: wrote {record['screenshot_path']}", file=sys.stderr)
                        if (
                            compare_methods
                            and screenshot_method_comparison is None
                            and is_helper_compatible_profile(config, plan.profile)
                        ):
                            screenshot_method_comparison = compare_screenshot_methods(
                                adb=adb,
                                config=config,
                                scenario=scenario,
                                profile=plan.profile,
                                output_dir=output_dir,
                                fast_record=record,
                            )
                    else:
                        print(f"android-visual-qa: FAILED {plan.profile.name}/{scenario.id}: {record['error']}", file=sys.stderr)
        finally:
            restore_event = append_final_viewport_restore_event(
                adb=adb,
                config=config,
                initial_snapshot=initial_snapshot,
                viewport_events=viewport_events,
            )
            for record in captures:
                record["final_viewport_restore_event_index"] = restore_event.get("index")
            mark_target_restore_failure(captures, target, restore_event)

    failure_count = sum(1 for record in captures if record.get("status") != "passed")
    if isinstance(screenshot_method_comparison, Mapping) and screenshot_method_comparison.get("status") == "failed":
        failure_count += 1
    worker_timing: dict[str, object] = {
        "target": target,
        "serial": config.serial,
        "started_at": worker_started_at,
        "ended_at": utc_now(),
        "duration_ms": duration_ms_since(worker_started),
        "status": "failed" if failure_count else "passed",
        "record_count": len(captures),
        "failure_count": failure_count,
        "viewport_event_count": len(viewport_events),
    }
    return TargetRunResult(
        target=target,
        records=tuple(captures),
        viewport_events=tuple(viewport_events),
        cache_summary=run_cache.summary(),
        worker_timing=worker_timing,
        screenshot_method_comparison=screenshot_method_comparison,
    )


def collect_target_worker_results(
    *,
    target_order: Sequence[str],
    worker_count: int,
    runner: Callable[[str], TargetRunResult],
) -> dict[str, TargetRunResult]:
    if worker_count <= 1 or len(target_order) <= 1:
        return {target: runner(target) for target in target_order}

    results: dict[str, TargetRunResult] = {}
    with ThreadPoolExecutor(max_workers=worker_count, thread_name_prefix="ferrex-android-visual-qa") as executor:
        futures = {executor.submit(runner, target): target for target in target_order}
        for future in as_completed(futures):
            target = futures[future]
            results[target] = future.result()
    return results


def merge_target_run_results(
    *,
    results_by_target: Mapping[str, TargetRunResult],
    target_order: Sequence[str],
) -> tuple[list[dict[str, object]], list[dict[str, object]], list[dict[str, object]], list[dict[str, object]], dict[str, object] | None]:
    records: list[dict[str, object]] = []
    viewport_events: list[dict[str, object]] = []
    cache_summaries: list[dict[str, object]] = []
    worker_timings: list[dict[str, object]] = []
    screenshot_method_comparison: dict[str, object] | None = None
    for target in target_order:
        result = results_by_target[target]
        adjusted_records, adjusted_events = offset_viewport_event_indices(
            result.records,
            result.viewport_events,
            len(viewport_events),
        )
        records.extend(adjusted_records)
        viewport_events.extend(adjusted_events)
        cache_summaries.append(result.cache_summary)
        worker_timings.append(dict(result.worker_timing))
        if screenshot_method_comparison is None and result.screenshot_method_comparison is not None:
            screenshot_method_comparison = copy.deepcopy(result.screenshot_method_comparison)
    return records, viewport_events, cache_summaries, worker_timings, screenshot_method_comparison


def run_accessibility(args: argparse.Namespace) -> int:
    repo_root = repo_root_from_script()
    registry = ScenarioRegistry.load(repo_root)
    selected = registry.select(args.target, args.scenario)
    output_dir = resolve_output_dir(repo_root, args.output_dir)
    return run_accessibility_plan(
        args=args,
        repo_root=repo_root,
        registry=registry,
        selected=selected,
        output_dir=output_dir,
    )


def run_accessibility_plan(
    *,
    args: argparse.Namespace,
    repo_root: Path,
    registry: ScenarioRegistry,
    selected: Sequence[Scenario],
    output_dir: Path,
) -> int:
    configs = target_configs(repo_root, args)
    profiles_by_target = selected_viewport_profiles(args, configs, selected)
    adb = resolve_executable(args.adb)
    output_dir.mkdir(parents=True, exist_ok=True)

    base_cache = RunCache()
    command_versions = base_cache.command_versions(adb, Path(__file__).resolve())
    plans = ordered_viewport_plans(selected, profiles_by_target)
    target_order = target_order_for_plans(plans)
    requested_workers, effective_workers = effective_target_worker_count(args, target_order, configs)
    target_workers = target_worker_manifest_config(
        requested=requested_workers,
        effective=effective_workers,
        target_order=target_order,
        configs=configs,
        profiles_by_target=profiles_by_target,
        artifact_kind="accessibility",
    )
    manifest: dict[str, object] = {
        "schema_version": 1,
        "command": "android-visual-qa accessibility",
        "argv": getattr(args, "effective_argv", sys.argv[1:]),
        "started_at": utc_now(),
        "output_dir": str(output_dir),
        "hardware_confirmation": bool(args.hardware),
        "max_accessibility_steps": args.max_steps,
        "exhaustive_accessibility_dumps": bool(args.exhaustive_dumps),
        "registry": registry.to_json(),
        "accessibility_plan": capture_plan_json(None, selected, profiles_by_target),
        "target_workers": target_workers,
        "command_versions": command_versions.value,
        "command_versions_cache": command_versions.provenance,
        "checks": [],
        "failures": [],
        "viewport_events": [],
    }

    def runner(target: str) -> TargetRunResult:
        return run_accessibility_target_plans(
            adb=adb,
            config=configs[target],
            target_plans=[plan for plan in plans if plan.target == target],
            args=args,
            output_dir=output_dir,
        )

    results_by_target = collect_target_worker_results(
        target_order=target_order,
        worker_count=effective_workers,
        runner=runner,
    )
    records, viewport_events, cache_summaries, worker_timings, _ = merge_target_run_results(
        results_by_target=results_by_target,
        target_order=target_order,
    )
    manifest["target_workers"]["timings"] = worker_timings

    failures = [record for record in records if record.get("status") != "passed"]
    manifest["checks"] = records
    manifest["failures"] = failures
    manifest["viewport_events"] = viewport_events
    manifest["cache_summary"] = merge_cache_summaries(base_cache.summary(), *cache_summaries)
    manifest["ended_at"] = utc_now()
    manifest["status"] = "failed" if failures else "passed"
    manifest_path = output_dir / "accessibility-manifest.json"
    timing_summary = write_manifest_with_timing_summary(manifest_path, manifest, record_key="checks")
    print(f"android-visual-qa: accessibility manifest {manifest_path}", file=sys.stderr)
    print_timing_summary(timing_summary)
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
    screenshot_mode = effective_screenshot_mode(args)
    compare_methods = should_compare_screenshot_methods(mode, args)
    configs = target_configs(repo_root, args)
    profiles_by_target = selected_viewport_profiles(args, configs, selected)
    adb = resolve_executable(args.adb)
    output_dir.mkdir(parents=True, exist_ok=True)

    base_cache = RunCache()
    command_versions = base_cache.command_versions(adb, Path(__file__).resolve())
    plans = ordered_viewport_plans(selected, profiles_by_target)
    target_order = target_order_for_plans(plans)
    requested_workers, effective_workers = effective_target_worker_count(args, target_order, configs)
    target_workers = target_worker_manifest_config(
        requested=requested_workers,
        effective=effective_workers,
        target_order=target_order,
        configs=configs,
        profiles_by_target=profiles_by_target,
        artifact_kind="capture",
    )
    manifest: dict[str, object] = {
        "schema_version": 1,
        "command": command_name,
        "argv": getattr(args, "effective_argv", sys.argv[1:]),
        "started_at": utc_now(),
        "output_dir": str(output_dir),
        "hardware_confirmation": bool(args.hardware),
        "registry": registry.to_json(),
        "capture_plan": capture_plan_json(mode, selected, profiles_by_target),
        "target_workers": target_workers,
        "warm_gate": bool(getattr(args, "warm", False)),
        "screenshot": screenshot_manifest_config(args),
        "command_versions": command_versions.value,
        "command_versions_cache": command_versions.provenance,
        "profile_deferrals": [],
        "captures": [],
        "failures": [],
        "viewport_events": [],
    }

    def runner(target: str) -> TargetRunResult:
        return run_capture_target_plans(
            adb=adb,
            config=configs[target],
            target_plans=[plan for plan in plans if plan.target == target],
            args=args,
            output_dir=output_dir,
            screenshot_mode=screenshot_mode,
            compare_methods=compare_methods,
        )

    results_by_target = collect_target_worker_results(
        target_order=target_order,
        worker_count=effective_workers,
        runner=runner,
    )
    captures, viewport_events, cache_summaries, worker_timings, screenshot_method_comparison = merge_target_run_results(
        results_by_target=results_by_target,
        target_order=target_order,
    )
    manifest["target_workers"]["timings"] = worker_timings

    if compare_methods and screenshot_method_comparison is None:
        screenshot_method_comparison = {
            "status": "unavailable",
            "unavailable_reason": "no passed default-emulator capture was available for helper-compatible comparison",
            "started_at": utc_now(),
            "ended_at": utc_now(),
        }
    if screenshot_method_comparison is not None:
        manifest["screenshot_method_comparison"] = screenshot_method_comparison

    failures = [record for record in captures if record.get("status") != "passed"]
    if isinstance(screenshot_method_comparison, Mapping) and screenshot_method_comparison.get("status") == "failed":
        failures.append(
            {
                "status": "failed",
                "step": "screenshot_method_comparison",
                "error": screenshot_method_comparison.get("error", "screenshot method comparison failed"),
            }
        )
    manifest["captures"] = captures
    manifest["failures"] = failures
    manifest["viewport_events"] = viewport_events
    manifest["cache_summary"] = merge_cache_summaries(base_cache.summary(), *cache_summaries)
    manifest["ended_at"] = utc_now()
    manifest["status"] = "failed" if failures else "passed"
    manifest_path = output_dir / "manifest.json"
    timing_summary = write_manifest_with_timing_summary(manifest_path, manifest, record_key="captures")
    print(f"android-visual-qa: manifest {manifest_path}", file=sys.stderr)
    print_timing_summary(timing_summary)
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


def effective_gate_settle_ms(args: argparse.Namespace) -> int:
    requested = getattr(args, "settle_ms", None)
    if requested is not None:
        return int(requested)
    if bool(getattr(args, "warm", False)):
        return DEFAULT_WARM_SETTLE_MS
    return DEFAULT_SETTLE_MS


def gate_capture_args(args: argparse.Namespace, output_dir: Path) -> argparse.Namespace:
    return argparse.Namespace(
        target="all",
        scenario="all",
        output_dir=str(output_dir),
        settle_ms=effective_gate_settle_ms(args),
        log_lines=args.log_lines,
        adb=args.adb,
        no_nix_screenshot=args.no_nix_screenshot,
        screenshot_mode=effective_screenshot_mode(args),
        hardware=False,
        hardware_serial=None,
        expected_size=None,
        profile=getattr(args, "profile", None),
        target_workers=target_worker_count(args),
        warm=bool(getattr(args, "warm", False)),
        warm_fast_launch=bool(getattr(args, "warm_fast_launch", False)),
        effective_argv=getattr(args, "effective_argv", sys.argv[1:]),
    )


def record_gate_primitive(
    name: str,
    gate_primitives: list[dict[str, object]],
    action: Callable[[], object],
    *,
    reason: str | None = None,
    phase: str | None = None,
) -> object:
    started_at = utc_now()
    started = time.monotonic()
    try:
        result = action()
    except Exception as exc:
        record: dict[str, object] = {
            "name": name,
            "status": "failed",
            "duration_ms": duration_ms_since(started),
            "started_at": started_at,
            "ended_at": utc_now(),
            "error": redact_text(str(exc)),
        }
        if reason:
            record["reason"] = redact_text(reason)
        if phase:
            record["phase"] = phase
        if isinstance(exc, CommandError):
            record["returncode"] = exc.returncode
        gate_primitives.append(record)
        raise

    record = {
        "name": name,
        "status": "passed",
        "duration_ms": duration_ms_since(started),
        "started_at": started_at,
        "ended_at": utc_now(),
    }
    if reason:
        record["reason"] = redact_text(reason)
    if phase:
        record["phase"] = phase
    if type(result) is int and result != 0:
        record["status"] = "failed"
        record["exit_status"] = result
    gate_primitives.append(record)
    return result


def record_gate_skip(
    name: str,
    gate_primitives: list[dict[str, object]],
    *,
    reason: str,
    phase: str = WARM_PREPARATION_PHASE,
) -> None:
    now = utc_now()
    gate_primitives.append(
        {
            "name": name,
            "status": "skipped",
            "duration_ms": 0,
            "started_at": now,
            "ended_at": now,
            "reason": redact_text(reason),
            "phase": phase,
        }
    )


def record_warm_freshness_probe(
    args: argparse.Namespace,
    repo_root: Path,
    gate_primitives: list[dict[str, object]],
) -> dict[str, object]:
    started_at = utc_now()
    started = time.monotonic()
    evidence = evaluate_warm_gate(args, repo_root)
    status = "passed" if evidence.get("status") == "passed" else "fallback"
    gate_primitives.append(
        {
            "name": WARM_FRESHNESS_STEP_NAME,
            "status": status,
            "duration_ms": duration_ms_since(started),
            "started_at": started_at,
            "ended_at": utc_now(),
            "reason": redact_text(str(evidence.get("reason") or "warm freshness evaluated")),
            "phase": WARM_PREPARATION_PHASE,
            "freshness_policy": WARM_FRESHNESS_POLICY,
            "evidence": evidence,
        }
    )
    return evidence


def gate_step_for_summary(record: Mapping[str, object]) -> dict[str, object]:
    keys = (
        "name",
        "status",
        "duration_ms",
        "started_at",
        "ended_at",
        "reason",
        "error",
        "phase",
        "freshness_policy",
        "returncode",
        "exit_status",
    )
    summarized = {key: copy.deepcopy(record[key]) for key in keys if key in record}
    if record.get("name") == WARM_FRESHNESS_STEP_NAME and isinstance(record.get("evidence"), Mapping):
        summarized["evidence"] = copy.deepcopy(record["evidence"])
    return summarized


def gate_preparation_summary(args: argparse.Namespace, gate_primitives: Sequence[Mapping[str, object]]) -> dict[str, object]:
    preparation_steps = [
        gate_step_for_summary(record)
        for record in gate_primitives
        if record.get("phase") == WARM_PREPARATION_PHASE or record.get("name") in WARM_PREPARATION_STEP_NAMES
    ]
    status_counts = Counter(str(step.get("status") or "unknown") for step in preparation_steps)
    warm_step = next((step for step in preparation_steps if step.get("name") == WARM_FRESHNESS_STEP_NAME), None)
    return {
        "mode": "warm" if bool(getattr(args, "warm", False)) else "full",
        "warm_requested": bool(getattr(args, "warm", False)),
        "freshness_policy": WARM_FRESHNESS_POLICY,
        "default_full_preparation": not bool(getattr(args, "warm", False)),
        "argv": list(getattr(args, "effective_argv", sys.argv[1:])),
        "capture_settle_ms": {
            "requested": int(getattr(args, "settle_ms")) if getattr(args, "settle_ms", None) is not None else None,
            "effective": effective_gate_settle_ms(args),
            "full_default": DEFAULT_SETTLE_MS,
            "warm_default": DEFAULT_WARM_SETTLE_MS if bool(getattr(args, "warm", False)) else None,
        },
        "warm_fast_launch": bool(getattr(args, "warm_fast_launch", False)),
        "fallback": bool(warm_step and warm_step.get("status") == "fallback"),
        "fallback_reason": warm_step.get("reason") if isinstance(warm_step, Mapping) and warm_step.get("status") == "fallback" else None,
        "steps": preparation_steps,
        "status_counts": dict(sorted(status_counts.items())),
        "executed_steps": [
            str(step.get("name"))
            for step in preparation_steps
            if step.get("status") in {"passed", "failed", "fallback"}
        ],
        "skipped_steps": [str(step.get("name")) for step in preparation_steps if step.get("status") == "skipped"],
    }


def gate_failure_record(gate_primitives: Sequence[Mapping[str, object]], error: Exception) -> dict[str, object]:
    failed = next((primitive for primitive in reversed(gate_primitives) if primitive.get("status") == "failed"), None)
    step = failed.get("name") if isinstance(failed, Mapping) and isinstance(failed.get("name"), str) else "gate"
    return {
        "status": "failed",
        "step": step,
        "error": redact_text(str(error)),
    }


def write_gate_failure_manifest(
    *,
    args: argparse.Namespace,
    repo_root: Path,
    registry: ScenarioRegistry,
    selected: Sequence[Scenario],
    output_dir: Path,
    gate_primitives: Sequence[Mapping[str, object]],
    error: Exception,
) -> dict[str, object]:
    output_dir.mkdir(parents=True, exist_ok=True)
    capture_args = gate_capture_args(args, output_dir)
    try:
        configs = target_configs(repo_root, capture_args)
        profiles_by_target = selected_viewport_profiles(capture_args, configs, selected)
        capture_plan: object = capture_plan_json(args.mode, selected, profiles_by_target)
    except Exception as plan_error:  # noqa: BLE001 - failure manifests should preserve primary errors.
        capture_plan = {"mode": args.mode, "error": redact_text(str(plan_error))}
    manifest: dict[str, object] = {
        "schema_version": 1,
        "command": "android-visual-qa gate",
        "argv": getattr(args, "effective_argv", sys.argv[1:]),
        "started_at": utc_now(),
        "ended_at": utc_now(),
        "output_dir": str(output_dir),
        "hardware_confirmation": False,
        "registry": registry.to_json(),
        "capture_plan": capture_plan,
        "warm_gate": bool(getattr(args, "warm", False)),
        "screenshot": screenshot_manifest_config(capture_args),
        "command_versions": collect_command_versions(args.adb, Path(__file__).resolve()),
        "profile_deferrals": [],
        "preparation": gate_preparation_summary(args, gate_primitives),
        "captures": [],
        "failures": [gate_failure_record(gate_primitives, error)],
        "status": "failed",
    }
    manifest_path = output_dir / "manifest.json"
    timing_summary = write_manifest_with_timing_summary(
        manifest_path,
        manifest,
        record_key="captures",
        gate_primitives=gate_primitives,
    )
    print(f"android-visual-qa: manifest {manifest_path}", file=sys.stderr)
    print_timing_summary(timing_summary)
    return manifest


def update_gate_manifest_timing(
    manifest_path: Path,
    gate_primitives: Sequence[Mapping[str, object]],
    *,
    args: argparse.Namespace | None = None,
) -> dict[str, object]:
    manifest = read_json_object(manifest_path)
    if args is not None:
        manifest["preparation"] = gate_preparation_summary(args, gate_primitives)
    record_key = "captures" if isinstance(manifest.get("captures"), list) else "checks"
    return write_manifest_with_timing_summary(
        manifest_path,
        manifest,
        record_key=record_key,
        gate_primitives=gate_primitives,
    )


def run_gate(args: argparse.Namespace) -> int:
    effective_screenshot_mode(args)
    repo_root = repo_root_from_script()
    registry = ScenarioRegistry.load(repo_root)
    selected = scenarios_for_gate_mode(registry, args.mode)
    output_dir = resolve_output_dir(repo_root, args.output_dir, args.mode)
    manifest_path = output_dir / "manifest.json"
    qa_script = repo_root / "scripts/qa/android-emulator-qa.sh"
    gate_primitives: list[dict[str, object]] = []

    steps: tuple[tuple[str, tuple[str | Path, ...]], ...] = (
        ("build", (qa_script, "build")),
        ("start", (qa_script, "start")),
        ("doctor", (qa_script, "doctor")),
        ("install", (qa_script, "install", "all")),
    )
    warm_requested = bool(getattr(args, "warm", False))
    setattr(args, "warm_fast_launch", False)
    run_full_preparation = True
    preparation_reason = "full preparation path (warm mode not requested)"
    if warm_requested:
        print("android-visual-qa: step warm freshness", file=sys.stderr)
        warm_freshness = record_warm_freshness_probe(args, repo_root, gate_primitives)
        if warm_freshness.get("status") == "passed":
            setattr(args, "warm_fast_launch", True)
            run_full_preparation = False
            skip_reasons = {
                "build": "warm freshness proved host APK identities and installed APK freshness",
                "start": "warm freshness proved both deterministic serials are present and boot_completed=1",
                "doctor": "warm freshness proved serial target metadata matches the expected emulator profiles",
                "install": "warm freshness proved installed package names/versions and APK identities match the host APKs",
            }
            for label, _command in steps:
                print(f"android-visual-qa: step {label} skipped (warm)", file=sys.stderr)
                record_gate_skip(label, gate_primitives, reason=skip_reasons[label])
        else:
            preparation_reason = f"warm freshness fallback: {warm_freshness.get('reason') or 'freshness could not be proven'}"

    if run_full_preparation:
        try:
            for label, command in steps:
                print(f"android-visual-qa: step {label}", file=sys.stderr)
                record_gate_primitive(
                    label,
                    gate_primitives,
                    lambda command=command: run_gate_command(command),
                    reason=preparation_reason,
                    phase=WARM_PREPARATION_PHASE,
                )
        except Exception as exc:
            write_gate_failure_manifest(
                args=args,
                repo_root=repo_root,
                registry=registry,
                selected=selected,
                output_dir=output_dir,
                gate_primitives=gate_primitives,
                error=exc,
            )
            raise

    capture_args = gate_capture_args(args, output_dir)
    print(f"android-visual-qa: step capture ({args.mode})", file=sys.stderr)
    try:
        capture_status = record_gate_primitive(
            "capture",
            gate_primitives,
            lambda: run_capture_plan(
                args=capture_args,
                repo_root=repo_root,
                registry=registry,
                selected=selected,
                output_dir=output_dir,
                command_name="android-visual-qa gate",
                mode=args.mode,
            ),
            reason=f"capture {args.mode} visual QA screenshots after gate preparation",
            phase="capture",
        )
    except Exception as exc:
        write_gate_failure_manifest(
            args=args,
            repo_root=repo_root,
            registry=registry,
            selected=selected,
            output_dir=output_dir,
            gate_primitives=gate_primitives,
            error=exc,
        )
        raise
    if type(capture_status) is int and capture_status != 0:
        timing_summary = update_gate_manifest_timing(manifest_path, gate_primitives, args=args)
        print_timing_summary(timing_summary)
        return capture_status

    print("android-visual-qa: step check", file=sys.stderr)
    try:
        record_gate_primitive(
            "check",
            gate_primitives,
            lambda: run_gate_command((qa_script, "check", "all")),
            reason="post-capture installed target metadata validation",
            phase="validation",
        )
    except Exception as exc:
        if manifest_path.exists():
            timing_summary = update_gate_manifest_timing(manifest_path, gate_primitives, args=args)
            print_timing_summary(timing_summary)
        else:
            write_gate_failure_manifest(
                args=args,
                repo_root=repo_root,
                registry=registry,
                selected=selected,
                output_dir=output_dir,
                gate_primitives=gate_primitives,
                error=exc,
            )
        raise

    print("android-visual-qa: step verify", file=sys.stderr)
    try:
        record_gate_primitive(
            "verify",
            gate_primitives,
            lambda: verify_manifest(manifest_path, mode=args.mode, repo_root=repo_root),
            reason="verify manifest captures and required scenario coverage",
            phase="validation",
        )
    except Exception as exc:
        timing_summary = update_gate_manifest_timing(manifest_path, gate_primitives, args=args)
        print_timing_summary(timing_summary)
        raise

    update_gate_manifest_timing(manifest_path, gate_primitives, args=args)
    summary = verify_manifest(manifest_path, mode=args.mode, repo_root=repo_root)
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
    gate.add_argument(
        "--settle-ms",
        type=positive_int,
        default=None,
        help=(
            f"Milliseconds to wait after scenario launch before each screenshot; default {DEFAULT_SETTLE_MS}. "
            f"Warm gates use {DEFAULT_WARM_SETTLE_MS} unless this flag is explicitly set."
        ),
    )
    gate.add_argument("--log-lines", type=positive_int, default=DEFAULT_LOG_LINES)
    gate.add_argument(
        "--warm",
        action="store_true",
        help=(
            "Opt in to conservative warm preparation: prove deterministic serial readiness, boot_completed, "
            "target metadata, installed package version, and APK freshness before skipping build/start/doctor/install; "
            "uses fast per-scenario relaunch only after freshness passes, keeps the warm settle default, "
            "and falls back to the full preparation path when evidence is missing or stale."
        ),
    )
    gate.add_argument(
        "--adb",
        default=os.environ.get("ADB", "adb"),
        help="adb executable to use for screenshot capture",
    )
    gate.add_argument(
        "--screenshot-mode",
        choices=SCREENSHOT_MODES,
        default=SCREENSHOT_MODE_FAST,
        help=(
            "Screenshot path to use during capture: fast uses adb -s <serial> exec-out screencap -p "
            "after the gate has prepared devices; helper-compatible invokes the Nix screenshot helper "
            "for default emulator profiles when available. Defaults to fast."
        ),
    )
    gate.add_argument(
        "--no-nix-screenshot",
        action="store_true",
        help="Deprecated alias for --screenshot-mode fast; also disables the smoke helper comparison.",
    )
    gate.add_argument(
        "--profile",
        action="append",
        help="Viewport profile(s) to capture: all, phone-portrait, phone-landscape-foldable, tv-1080p, tv-4k-scaled. May be repeated or comma-separated; defaults to all.",
    )
    gate.add_argument(
        "--target-workers",
        type=positive_int,
        default=1,
        help="Opt-in per-target concurrency; 2 runs phone and TV on separate serial-owning workers while each target remains serial.",
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
        help="Scenario ID from android-visual-qa list, a comma-separated list, or 'all' for the required target matrix",
    )
    accessibility.add_argument(
        "--output-dir",
        default=str(DEFAULT_OUTPUT_DIR),
        help="Output directory for accessibility dumps and manifest",
    )
    accessibility.add_argument("--settle-ms", type=positive_int, default=DEFAULT_SETTLE_MS)
    accessibility.add_argument("--log-lines", type=positive_int, default=DEFAULT_LOG_LINES)
    accessibility.add_argument(
        "--max-steps",
        "--accessibility-max-steps",
        type=positive_int,
        default=DEFAULT_ACCESSIBILITY_MAX_STEPS,
        help="Maximum accessibility dump/drive steps before failing unresolved requirements",
    )
    accessibility.add_argument(
        "--exhaustive-dumps",
        "--exhaustive-accessibility-dumps",
        action="store_true",
        help="Capture all accessibility steps even after requirements pass or no-progress is detected",
    )
    accessibility.add_argument("--adb", default=os.environ.get("ADB", "adb"), help="adb executable to use")
    accessibility.add_argument(
        "--profile",
        action="append",
        help="Viewport profile(s) to check: all, phone-portrait, phone-landscape-foldable, tv-1080p, tv-4k-scaled. May be repeated or comma-separated; defaults to all.",
    )
    accessibility.add_argument(
        "--target-workers",
        type=positive_int,
        default=1,
        help="Opt-in per-target concurrency; 2 runs phone and TV accessibility checks on separate serial-owning workers while each target remains serial.",
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
        help="Scenario ID from android-visual-qa list, a comma-separated list, or 'all' for the required target matrix",
    )
    capture.add_argument("--output-dir", default=str(DEFAULT_OUTPUT_DIR), help="Output directory for PNGs and manifest")
    capture.add_argument("--settle-ms", type=positive_int, default=DEFAULT_SETTLE_MS)
    capture.add_argument("--log-lines", type=positive_int, default=DEFAULT_LOG_LINES)
    capture.add_argument("--adb", default=os.environ.get("ADB", "adb"), help="adb executable to use")
    capture.add_argument(
        "--screenshot-mode",
        choices=SCREENSHOT_MODES,
        default=SCREENSHOT_MODE_FAST,
        help=(
            "Screenshot path to use: fast uses adb -s <serial> exec-out screencap -p; "
            "helper-compatible invokes the Nix screenshot helper for default emulator profiles when available. "
            "Defaults to fast."
        ),
    )
    capture.add_argument(
        "--no-nix-screenshot",
        action="store_true",
        help="Deprecated alias for --screenshot-mode fast.",
    )
    capture.add_argument(
        "--profile",
        action="append",
        help="Viewport profile(s) to capture: all, phone-portrait, phone-landscape-foldable, tv-1080p, tv-4k-scaled. May be repeated or comma-separated; defaults to all.",
    )
    capture.add_argument(
        "--target-workers",
        type=positive_int,
        default=1,
        help="Opt-in per-target concurrency; 2 runs phone and TV on separate serial-owning workers while each target remains serial.",
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
