#!/usr/bin/env python3
"""Guard against non-test dynamic SQLx APIs.

This scanner intentionally favors a cheap, deterministic text pass over a full Rust
parser. It tracks imports and aliases for SQLx dynamic query APIs, skips Rust test
fixtures, and requires every remaining production use to be reviewed in a precise
TOML allowlist.
"""

from __future__ import annotations

import argparse
import bisect
import dataclasses
import re
import sys
import tomllib
from pathlib import Path
from typing import Iterable, Iterator, Sequence

FORBIDDEN_FUNCTIONS = frozenset(
    {"query", "query_as", "query_scalar", "query_with", "raw_sql"}
)
FORBIDDEN_TYPE = "QueryBuilder"
DEFAULT_ALLOWLIST = Path("scripts/sqlx_dynamic_allowlist.toml")
SKIPPED_DIRS = frozenset(
    {
        ".git",
        ".jj",
        ".sqlx",
        ".symphony",
        "target",
        "cache",
        "dist-windows",
        "tests",
    }
)
REQUIRED_ALLOWLIST_FIELDS = frozenset(
    {"path", "symbol", "reason", "reviewer", "expiration", "removal_target"}
)


@dataclasses.dataclass(frozen=True, order=True)
class Finding:
    """One forbidden SQLx API use found in non-test Rust code."""

    path: str
    line: int
    column: int
    symbol: str
    selector: str

    def display(self) -> str:
        return f"{self.path}:{self.line}:{self.column}: {self.symbol}: {self.selector}"


@dataclasses.dataclass(frozen=True)
class AllowlistEntry:
    """Reviewed exception for one or more precise findings."""

    index: int
    path: str
    symbol: str
    lines: frozenset[int]
    selectors: frozenset[str]
    reason: str
    reviewer: str
    expiration: str
    removal_target: str

    def match_keys(self, finding: Finding) -> set[tuple[str, int | str]]:
        if finding.path != self.path or finding.symbol != self.symbol:
            return set()

        keys: set[tuple[str, int | str]] = set()
        if finding.line in self.lines:
            keys.add(("line", finding.line))
        if finding.selector in self.selectors:
            keys.add(("selector", finding.selector))
        return keys

    @property
    def keys(self) -> frozenset[tuple[str, int | str]]:
        return frozenset(
            [("line", line) for line in self.lines]
            + [("selector", selector) for selector in self.selectors]
        )


@dataclasses.dataclass(frozen=True)
class Allowlist:
    entries: tuple[AllowlistEntry, ...]

    @classmethod
    def from_toml_path(cls, path: Path) -> "Allowlist":
        if not path.exists():
            return cls(())
        with path.open("rb") as handle:
            data = tomllib.load(handle)
        return cls.from_mapping(data, source=str(path))

    @classmethod
    def from_mapping(cls, data: object, *, source: str = "allowlist") -> "Allowlist":
        if not isinstance(data, dict):
            raise ValueError(f"{source}: allowlist root must be a TOML table")
        raw_entries = data.get("exceptions", [])
        if not isinstance(raw_entries, list):
            raise ValueError(f"{source}: exceptions must be an array of tables")

        entries: list[AllowlistEntry] = []
        for index, raw in enumerate(raw_entries, start=1):
            if not isinstance(raw, dict):
                raise ValueError(f"{source}: exception #{index} must be a table")
            missing = sorted(REQUIRED_ALLOWLIST_FIELDS - raw.keys())
            if missing:
                raise ValueError(
                    f"{source}: exception #{index} is missing required fields: "
                    + ", ".join(missing)
                )

            lines = _coerce_int_set(raw, "line", "lines", source, index)
            selectors = _coerce_string_set(raw, "selector", "selectors", source, index)
            if not lines and not selectors:
                raise ValueError(
                    f"{source}: exception #{index} must include line/lines or selector/selectors"
                )

            entries.append(
                AllowlistEntry(
                    index=index,
                    path=_coerce_string(raw["path"], source, index, "path"),
                    symbol=_coerce_string(raw["symbol"], source, index, "symbol"),
                    lines=frozenset(lines),
                    selectors=frozenset(_normalize_selector(s) for s in selectors),
                    reason=_coerce_string(raw["reason"], source, index, "reason"),
                    reviewer=_coerce_string(raw["reviewer"], source, index, "reviewer"),
                    expiration=_coerce_string(raw["expiration"], source, index, "expiration"),
                    removal_target=_coerce_string(
                        raw["removal_target"], source, index, "removal_target"
                    ),
                )
            )
        return cls(tuple(entries))


def _coerce_string(value: object, source: str, index: int, field: str) -> str:
    if not isinstance(value, str) or not value.strip():
        raise ValueError(f"{source}: exception #{index} field {field!r} must be a string")
    return value.strip()


def _coerce_int_set(
    raw: dict[str, object], singular: str, plural: str, source: str, index: int
) -> set[int]:
    values: set[int] = set()
    if singular in raw:
        value = raw[singular]
        if not isinstance(value, int):
            raise ValueError(
                f"{source}: exception #{index} field {singular!r} must be an integer"
            )
        values.add(value)
    if plural in raw:
        value = raw[plural]
        if not isinstance(value, list) or not all(isinstance(item, int) for item in value):
            raise ValueError(
                f"{source}: exception #{index} field {plural!r} must be a list of integers"
            )
        values.update(value)
    if any(value <= 0 for value in values):
        raise ValueError(f"{source}: exception #{index} line numbers must be positive")
    return values


def _coerce_string_set(
    raw: dict[str, object], singular: str, plural: str, source: str, index: int
) -> set[str]:
    values: set[str] = set()
    if singular in raw:
        value = raw[singular]
        if not isinstance(value, str) or not value.strip():
            raise ValueError(
                f"{source}: exception #{index} field {singular!r} must be a string"
            )
        values.add(value)
    if plural in raw:
        value = raw[plural]
        if not isinstance(value, list) or not all(
            isinstance(item, str) and item.strip() for item in value
        ):
            raise ValueError(
                f"{source}: exception #{index} field {plural!r} must be a list of strings"
            )
        values.update(item.strip() for item in value)
    return values


def should_scan_path(path: Path) -> bool:
    if path.suffix != ".rs":
        return False
    return not any(part in SKIPPED_DIRS for part in path.parts)


def iter_rust_files(root: Path) -> Iterator[Path]:
    for path in sorted(root.rglob("*.rs")):
        rel = path.relative_to(root)
        if should_scan_path(rel):
            yield path


def scan_root(root: Path) -> list[Finding]:
    findings: list[Finding] = []
    for path in iter_rust_files(root):
        rel = path.relative_to(root).as_posix()
        findings.extend(scan_source(rel, path.read_text(encoding="utf-8")))
    return sorted(_dedupe_findings(findings))


def scan_source(path: str, source: str) -> list[Finding]:
    rel = Path(path)
    if not should_scan_path(rel):
        return []

    source_without_tests = mask_cfg_test_items(source)
    code = mask_comments_and_literals(source_without_tests)
    line_starts = _line_starts(code)
    original_lines = source.splitlines()
    use_ranges = tuple(_sqlx_use_ranges(code))
    aliases = _aliases_from_use_statements(code)

    findings: list[Finding] = []
    findings.extend(
        _find_qualified_function_calls(path, code, line_starts, original_lines, use_ranges, aliases)
    )
    findings.extend(
        _find_imported_function_calls(path, code, line_starts, original_lines, use_ranges, aliases)
    )
    findings.extend(
        _find_query_builder_uses(path, code, line_starts, original_lines, use_ranges, aliases)
    )
    return sorted(_dedupe_findings(findings))


def _dedupe_findings(findings: Iterable[Finding]) -> set[Finding]:
    return set(findings)


@dataclasses.dataclass(frozen=True)
class SqlxAliases:
    crate_aliases: frozenset[str]
    function_aliases: tuple[tuple[str, str], ...]
    query_builder_aliases: frozenset[str]


SQLX_USE_RE = re.compile(r"(?m)^\s*use\s+sqlx(?P<body>\s+(?:as\s+\w+)|\s*::[^;]*);", re.S)
IDENT_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*$")


def _sqlx_use_ranges(code: str) -> Iterator[tuple[int, int]]:
    for match in SQLX_USE_RE.finditer(code):
        yield match.span()


def _aliases_from_use_statements(code: str) -> SqlxAliases:
    crate_aliases = {"sqlx"}
    function_aliases: dict[str, str] = {}
    query_builder_aliases: set[str] = set()

    for match in SQLX_USE_RE.finditer(code):
        body = match.group("body").strip()
        if body.startswith("as "):
            alias = body.removeprefix("as ").strip()
            if IDENT_RE.match(alias):
                crate_aliases.add(alias)
            continue

        if not body.startswith("::"):
            continue
        path = body[2:].strip()
        if path.startswith("{") and path.endswith("}"):
            for item in _split_top_level_commas(path[1:-1]):
                _record_use_item(
                    item,
                    crate_aliases=crate_aliases,
                    function_aliases=function_aliases,
                    query_builder_aliases=query_builder_aliases,
                )
        else:
            _record_use_item(
                path,
                crate_aliases=crate_aliases,
                function_aliases=function_aliases,
                query_builder_aliases=query_builder_aliases,
            )

    return SqlxAliases(
        crate_aliases=frozenset(crate_aliases),
        function_aliases=tuple(sorted(function_aliases.items())),
        query_builder_aliases=frozenset(query_builder_aliases),
    )


def _record_use_item(
    item: str,
    *,
    crate_aliases: set[str],
    function_aliases: dict[str, str],
    query_builder_aliases: set[str],
) -> None:
    item = item.strip()
    if not item:
        return

    first_segment = item.split("::", 1)[0].strip()
    symbol, alias = _parse_alias(first_segment)
    if symbol == "self":
        if alias is not None:
            crate_aliases.add(alias)
        return

    if symbol in FORBIDDEN_FUNCTIONS:
        function_aliases[alias or symbol] = symbol
    elif symbol == FORBIDDEN_TYPE:
        query_builder_aliases.add(alias or symbol)


def _parse_alias(segment: str) -> tuple[str, str | None]:
    parts = re.split(r"\s+as\s+", segment, maxsplit=1)
    symbol = parts[0].strip()
    alias = parts[1].strip() if len(parts) == 2 else None
    if alias is not None and not IDENT_RE.match(alias):
        alias = None
    return symbol, alias


def _split_top_level_commas(items: str) -> list[str]:
    result: list[str] = []
    start = 0
    depth = 0
    for index, char in enumerate(items):
        if char == "{":
            depth += 1
        elif char == "}":
            depth = max(0, depth - 1)
        elif char == "," and depth == 0:
            result.append(items[start:index])
            start = index + 1
    result.append(items[start:])
    return result


def _find_qualified_function_calls(
    path: str,
    code: str,
    line_starts: Sequence[int],
    original_lines: Sequence[str],
    use_ranges: Sequence[tuple[int, int]],
    aliases: SqlxAliases,
) -> list[Finding]:
    findings: list[Finding] = []
    funcs = "|".join(sorted(FORBIDDEN_FUNCTIONS, key=len, reverse=True))
    for crate in sorted(aliases.crate_aliases, key=len, reverse=True):
        pattern = re.compile(
            rf"(?<![A-Za-z0-9_:]){re.escape(crate)}\s*::\s*(?P<symbol>{funcs})\b"
            rf"(?!\s*!)"
            rf"(?:\s*::\s*<[^;\n{{}}]*>|\s*<[^;\n{{}}]*>)?\s*\(",
            re.S,
        )
        for match in pattern.finditer(code):
            if _in_ranges(match.start(), use_ranges):
                continue
            findings.append(
                _finding(path, match.start("symbol"), match.group("symbol"), line_starts, original_lines)
            )
    return findings


def _find_imported_function_calls(
    path: str,
    code: str,
    line_starts: Sequence[int],
    original_lines: Sequence[str],
    use_ranges: Sequence[tuple[int, int]],
    aliases: SqlxAliases,
) -> list[Finding]:
    findings: list[Finding] = []
    for alias, symbol in aliases.function_aliases:
        pattern = re.compile(
            rf"(?<![A-Za-z0-9_:.]){re.escape(alias)}\b"
            rf"(?!\s*!)"
            rf"(?:\s*::\s*<[^;\n{{}}]*>|\s*<[^;\n{{}}]*>)?\s*\(",
            re.S,
        )
        for match in pattern.finditer(code):
            if _in_ranges(match.start(), use_ranges):
                continue
            findings.append(_finding(path, match.start(), symbol, line_starts, original_lines))
    return findings


def _find_query_builder_uses(
    path: str,
    code: str,
    line_starts: Sequence[int],
    original_lines: Sequence[str],
    use_ranges: Sequence[tuple[int, int]],
    aliases: SqlxAliases,
) -> list[Finding]:
    findings: list[Finding] = []
    for crate in sorted(aliases.crate_aliases, key=len, reverse=True):
        pattern = re.compile(
            rf"(?<![A-Za-z0-9_:]){re.escape(crate)}\s*::\s*(?P<symbol>{FORBIDDEN_TYPE})\b"
        )
        for match in pattern.finditer(code):
            if _in_ranges(match.start(), use_ranges):
                continue
            findings.append(
                _finding(path, match.start("symbol"), FORBIDDEN_TYPE, line_starts, original_lines)
            )

    for alias in sorted(aliases.query_builder_aliases, key=len, reverse=True):
        pattern = re.compile(rf"(?<![A-Za-z0-9_:.]){re.escape(alias)}\b")
        for match in pattern.finditer(code):
            if _in_ranges(match.start(), use_ranges):
                continue
            findings.append(_finding(path, match.start(), FORBIDDEN_TYPE, line_starts, original_lines))
    return findings


def _in_ranges(offset: int, ranges: Sequence[tuple[int, int]]) -> bool:
    return any(start <= offset < end for start, end in ranges)


def _finding(
    path: str,
    offset: int,
    symbol: str,
    line_starts: Sequence[int],
    original_lines: Sequence[str],
) -> Finding:
    line_index = bisect.bisect_right(line_starts, offset) - 1
    line = line_index + 1
    column = offset - line_starts[line_index] + 1
    selector = _normalize_selector(original_lines[line_index] if line_index < len(original_lines) else "")
    return Finding(path=path, line=line, column=column, symbol=symbol, selector=selector)


def _line_starts(text: str) -> list[int]:
    starts = [0]
    starts.extend(match.end() for match in re.finditer("\n", text))
    return starts


def _normalize_selector(text: str) -> str:
    return " ".join(text.strip().split())


def mask_cfg_test_items(source: str) -> str:
    """Blank out #[cfg(test)] items while preserving offsets and line numbers."""

    cfg_attr = re.compile(r"#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]")
    lines = source.splitlines(keepends=True)
    output = list(lines)
    index = 0
    while index < len(lines):
        if not cfg_attr.search(lines[index]):
            index += 1
            continue

        start = index
        index += 1
        while index < len(lines) and _is_following_attribute_or_blank(lines[index]):
            index += 1
        if index >= len(lines):
            end = len(lines)
        else:
            end = _end_of_rust_item(lines, index)

        for blank_index in range(start, end):
            output[blank_index] = _blank_preserving_newlines(output[blank_index])
        index = max(end, start + 1)
    return "".join(output)


def _is_following_attribute_or_blank(line: str) -> bool:
    stripped = line.strip()
    return not stripped or stripped.startswith("#")


def _end_of_rust_item(lines: Sequence[str], start: int) -> int:
    depth = 0
    saw_brace = False
    for index in range(start, len(lines)):
        masked = mask_comments_and_literals(lines[index])
        for char in masked:
            if char == "{":
                depth += 1
                saw_brace = True
            elif char == "}":
                depth = max(0, depth - 1)
                if saw_brace and depth == 0:
                    return index + 1
            elif char == ";" and not saw_brace:
                return index + 1
        if not saw_brace and lines[index].strip().endswith(";"):
            return index + 1
    return len(lines)


def _blank_preserving_newlines(text: str) -> str:
    return "".join("\n" if char == "\n" else " " for char in text)


def mask_comments_and_literals(source: str) -> str:
    """Replace comments and Rust string/char literals with spaces.

    The scanner only needs identifiers and punctuation. Blanking literal/comment
    contents keeps positions stable while preventing matches inside SQL strings,
    documentation, or comments.
    """

    result: list[str] = []
    index = 0
    state = "code"
    raw_hashes = 0
    block_depth = 0

    while index < len(source):
        char = source[index]
        next_char = source[index + 1] if index + 1 < len(source) else ""

        if state == "code":
            if char == "/" and next_char == "/":
                result.extend("  ")
                index += 2
                state = "line_comment"
            elif char == "/" and next_char == "*":
                result.extend("  ")
                index += 2
                state = "block_comment"
                block_depth = 1
            elif char == '"':
                result.append(" ")
                index += 1
                state = "string"
            elif char == "r":
                raw = _raw_string_prefix(source, index)
                if raw is not None:
                    hashes, prefix_len = raw
                    result.extend(" " * prefix_len)
                    index += prefix_len
                    state = "raw_string"
                    raw_hashes = hashes
                else:
                    result.append(char)
                    index += 1
            elif char == "'" and _looks_like_char_literal(source, index):
                result.append(" ")
                index += 1
                state = "char"
            else:
                result.append(char)
                index += 1
        elif state == "line_comment":
            result.append("\n" if char == "\n" else " ")
            index += 1
            if char == "\n":
                state = "code"
        elif state == "block_comment":
            if char == "/" and next_char == "*":
                result.extend("  ")
                index += 2
                block_depth += 1
            elif char == "*" and next_char == "/":
                result.extend("  ")
                index += 2
                block_depth -= 1
                if block_depth == 0:
                    state = "code"
            else:
                result.append("\n" if char == "\n" else " ")
                index += 1
        elif state == "string":
            if char == "\\":
                result.append(" ")
                if index + 1 < len(source):
                    result.append("\n" if source[index + 1] == "\n" else " ")
                    index += 2
                else:
                    index += 1
            elif char == '"':
                result.append(" ")
                index += 1
                state = "code"
            else:
                result.append("\n" if char == "\n" else " ")
                index += 1
        elif state == "raw_string":
            terminator = '"' + ("#" * raw_hashes)
            if source.startswith(terminator, index):
                result.extend(" " * len(terminator))
                index += len(terminator)
                state = "code"
            else:
                result.append("\n" if char == "\n" else " ")
                index += 1
        elif state == "char":
            if char == "\\":
                result.append(" ")
                if index + 1 < len(source):
                    result.append("\n" if source[index + 1] == "\n" else " ")
                    index += 2
                else:
                    index += 1
            elif char == "'":
                result.append(" ")
                index += 1
                state = "code"
            else:
                result.append("\n" if char == "\n" else " ")
                index += 1
        else:  # pragma: no cover - defensive state machine guard
            raise AssertionError(f"unknown lexer state: {state}")

    return "".join(result)


def _raw_string_prefix(source: str, index: int) -> tuple[int, int] | None:
    cursor = index + 1
    hashes = 0
    while cursor < len(source) and source[cursor] == "#":
        hashes += 1
        cursor += 1
    if cursor < len(source) and source[cursor] == '"':
        return hashes, cursor - index + 1
    return None


def _looks_like_char_literal(source: str, index: int) -> bool:
    if index + 1 >= len(source):
        return False
    if source[index + 1].isalpha() or source[index + 1] == "_":
        return False
    cursor = index + 1
    if source[cursor] == "\\":
        cursor += 2
    else:
        cursor += 1
    return cursor < len(source) and source[cursor] == "'"


@dataclasses.dataclass(frozen=True)
class Evaluation:
    unallowlisted: tuple[Finding, ...]
    stale: tuple[str, ...]
    ambiguous: tuple[str, ...]

    @property
    def ok(self) -> bool:
        return not self.unallowlisted and not self.stale and not self.ambiguous


def evaluate_allowlist(findings: Sequence[Finding], allowlist: Allowlist) -> Evaluation:
    used_keys: dict[tuple[int, tuple[str, int | str]], Finding] = {}
    unallowlisted: list[Finding] = []
    ambiguous: list[str] = []

    for finding in findings:
        matches: list[tuple[AllowlistEntry, set[tuple[str, int | str]]]] = []
        for entry in allowlist.entries:
            keys = entry.match_keys(finding)
            if keys:
                matches.append((entry, keys))

        if not matches:
            unallowlisted.append(finding)
            continue
        if len(matches) > 1:
            ambiguous.append(
                f"{finding.display()} matches multiple allowlist entries: "
                + ", ".join(f"#{entry.index}" for entry, _ in matches)
            )
            continue

        entry, keys = matches[0]
        for key in keys:
            used_keys[(entry.index, key)] = finding

    stale: list[str] = []
    for entry in allowlist.entries:
        for key in sorted(entry.keys):
            if (entry.index, key) not in used_keys:
                stale.append(
                    f"exception #{entry.index} {entry.path} {entry.symbol} {key[0]}={key[1]!r} no longer matches a finding"
                )

    return Evaluation(
        unallowlisted=tuple(sorted(unallowlisted)),
        stale=tuple(stale),
        ambiguous=tuple(ambiguous),
    )


def print_report(findings: Sequence[Finding], evaluation: Evaluation) -> None:
    if evaluation.ok:
        print(
            f"sqlx dynamic guard passed: {len(findings)} reviewed exception(s), "
            "0 new violations."
        )
        return

    if evaluation.unallowlisted:
        print("Forbidden dynamic SQLx usage found outside tests:", file=sys.stderr)
        for finding in evaluation.unallowlisted:
            print(f"  {finding.display()}", file=sys.stderr)
        print(
            "Use SQLx compile-checked macros where possible. If a dynamic query is "
            "temporarily unavoidable, add a reviewed exception with path, symbol, "
            "line or selector, reason, reviewer, expiration, and removal_target.",
            file=sys.stderr,
        )

    if evaluation.stale:
        print("Stale SQLx dynamic allowlist entries:", file=sys.stderr)
        for item in evaluation.stale:
            print(f"  {item}", file=sys.stderr)

    if evaluation.ambiguous:
        print("Ambiguous SQLx dynamic allowlist entries:", file=sys.stderr)
        for item in evaluation.ambiguous:
            print(f"  {item}", file=sys.stderr)


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path.cwd(), help="repository root")
    parser.add_argument(
        "--allowlist",
        type=Path,
        default=DEFAULT_ALLOWLIST,
        help="TOML file containing reviewed dynamic SQLx exceptions",
    )
    args = parser.parse_args(argv)

    root = args.root.resolve()
    allowlist_path = args.allowlist
    if not allowlist_path.is_absolute():
        allowlist_path = root / allowlist_path

    findings = scan_root(root)
    allowlist = Allowlist.from_toml_path(allowlist_path)
    evaluation = evaluate_allowlist(findings, allowlist)
    print_report(findings, evaluation)
    return 0 if evaluation.ok else 1


if __name__ == "__main__":
    raise SystemExit(main())
