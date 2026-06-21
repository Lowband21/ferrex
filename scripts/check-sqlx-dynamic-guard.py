#!/usr/bin/env python3
"""SQLx dynamic-query guard for Ferrex.

Enforces Ferrex policy that non-test Rust code uses compile-checked SQLx macros
(`query!`, `query_as!`, `query_scalar!`, `query_file!`, `migrate!`) instead of
SQLx's dynamic (non-macro) query APIs and `QueryBuilder`.

Forbidden dynamic APIs (in non-test code):
  - function-call forms: `query`, `query_as`, `query_scalar`, `query_with`,
    `raw_sql` -- forbidden when called (`(... )`) and NOT used as a macro (`!`).
  - `QueryBuilder` -- forbidden whenever referenced.

The guard resolves imports and aliases so that `use sqlx::query as dq; dq(...)`,
`use sqlx::{query, QueryBuilder}`, and `use sqlx as s; s::query(...)` are caught.

Test code is excluded by design:
  - files under any `tests/` directory (integration tests)
  - `#[cfg(test)]` / `#[cfg(any(test, ...))]` modules (inline or external `mod x;`)
  - `#[test]`, `#[tokio::test]`, and equivalent `::test`-suffixed attributes on
    functions/modules

A reviewed TOML allowlist (scripts/sqlx-dynamic-allowlist.toml) permits narrowly
scoped non-preparable admin/DDL exceptions. The guard fails on:
  - unallowlisted forbidden usages
  - stale allowlist entries (no matching usage) so the allowlist stays tight
  - expired `scope = "temporary"` exceptions

Usage:
  scripts/check-sqlx-dynamic-guard.py              # scan repo + enforce allowlist
  scripts/check-sqlx-dynamic-guard.py --self-test  # run built-in fixture tests
  scripts/check-sqlx-dynamic-guard.py --root PATH --allowlist PATH

Exit code: 0 if clean, 1 if violations / stale allowlist / expired exceptions.
"""

from __future__ import annotations

import argparse
import datetime as _dt
import os
import re
import sys
from dataclasses import dataclass, field
from pathlib import Path
from typing import Iterator

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------

GUARD_DIR = Path(__file__).resolve().parent
DEFAULT_ROOT = GUARD_DIR.parent
DEFAULT_ALLOWLIST = GUARD_DIR / "sqlx-dynamic-allowlist.toml"

# Dynamic SQLx APIs that are forbidden in non-test code.
CALL_APIS = {"query", "query_as", "query_scalar", "query_with", "raw_sql"}
TYPE_APIS = {"QueryBuilder"}
FORBIDDEN_SYMBOLS = CALL_APIS | TYPE_APIS

# Directory segments that mark integration-test code.
TEST_DIR_SEGMENTS = {"tests"}

# Build/output directories that never contain tracked source.
EXCLUDE_DIR_NAMES = {"target", "target-nix", ".jj", ".git", ".sqlx", "dist-windows"}

# Attributes that scope an item as test code.
_RE_ATTR_TEST_BARE = re.compile(r"(^|::)\s*test\s*(\(|$)")
_RE_ATTR_CFG_TEST = re.compile(r"\bcfg\s*\(")


def _attr_is_test_scoping(attr_inner: str) -> bool:
    """Return True for `#[test]`, `#[tokio::test]`, `#[cfg(test)]`, etc."""
    # Strip string-literal contents so `cfg(feature = "test")` is not mistaken.
    stripped = re.sub(r'"(\\.|[^"\\])*"', '""', attr_inner)
    if _RE_ATTR_TEST_BARE.search(stripped):
        return True
    if _RE_ATTR_CFG_TEST.search(stripped) and re.search(r"\btest\b", stripped):
        return True
    return False


# ---------------------------------------------------------------------------
# Tokenizer
# ---------------------------------------------------------------------------


@dataclass(frozen=True)
class Tok:
    kind: str  # "ident" | "punct" | "attr" | "other"
    text: str
    line: int
    col: int


def _is_ident_start(ch: str) -> bool:
    return ch == "_" or ch.isalpha()


def _is_ident_cont(ch: str) -> bool:
    return ch == "_" or ch.isalnum()


def tokenize(src: str) -> list[Tok]:
    """Tokenize Rust source into idents, punctuation, and attributes.

    Comments, string literals, raw strings, byte strings, char literals, and
    lifetime labels are skipped so identifiers inside them are never matched.
    """
    toks: list[Tok] = []
    i = 0
    n = len(src)
    line = 1
    col = 1

    def adv(k: int = 1) -> None:
        nonlocal i, line, col
        for _ in range(k):
            if i >= n:
                return
            ch = src[i]
            if ch == "\n":
                line += 1
                col = 1
            else:
                col += 1
            i += 1

    def at(offset: int = 0) -> str:
        j = i + offset
        return src[j] if j < n else ""

    def skip_quoted_string() -> None:
        """Skip a normal Rust string/byte string at the opening quote."""
        if at(0) != '"':
            return
        adv()  # opening quote
        while i < n:
            if src[i] == "\\":
                adv(2)
            elif src[i] == '"':
                adv()
                break
            else:
                adv()

    def skip_raw_string() -> bool:
        """Skip a Rust raw string (`r#"..."#` or `br#"..."#`)."""
        if at(0) == "b" and at(1) == "r":
            adv()
        if at(0) != "r":
            return False
        adv()  # r
        hashes = 0
        while at(0) == "#":
            hashes += 1
            adv()
        if at(0) != '"':
            return False
        adv()  # opening quote
        while i < n:
            if src[i] == '"':
                j = i + 1
                matched = True
                for _ in range(hashes):
                    if j >= n or src[j] != "#":
                        matched = False
                        break
                    j += 1
                if matched:
                    adv(1 + hashes)
                    break
            adv()
        return True

    def skip_string_literal() -> bool:
        """Skip any Rust string/byte-string starting at the current cursor."""
        if at(0) == '"':
            skip_quoted_string()
            return True
        if at(0) == "b" and at(1) == '"':
            adv()  # byte-string prefix
            skip_quoted_string()
            return True
        if (at(0) == "r") or (at(0) == "b" and at(1) == "r"):
            return skip_raw_string()
        return False

    while i < n:
        ch = src[i]

        # Whitespace
        if ch.isspace():
            adv()
            continue

        # Line comment
        if ch == "/" and at(1) == "/":
            while i < n and src[i] != "\n":
                adv()
            continue

        # Block comment (Rust block comments nest)
        if ch == "/" and at(1) == "*":
            depth = 1
            adv(2)
            while i < n and depth > 0:
                if src[i] == "/" and at(1) == "*":
                    depth += 1
                    adv(2)
                elif src[i] == "*" and at(1) == "/":
                    depth -= 1
                    adv(2)
                else:
                    adv()
            continue

        # Attribute: # or #! then [ ... ]
        if ch == "#":
            start_line, start_col = line, col
            adv()
            if at(0) == "!":
                adv()
            if at(0) != "[":
                # Stray '#'; emit as other.
                toks.append(Tok("other", "#", start_line, start_col))
                continue
            # Read balanced [ ... ] capturing inner text.
            adv()  # consume '['
            depth = 1
            inner_start = i
            while i < n and depth > 0:
                c = src[i]
                if c == "[":
                    depth += 1
                    adv()
                elif c == "]":
                    depth -= 1
                    adv()
                elif c == "/" and at(1) == "/":
                    while i < n and src[i] != "\n":
                        adv()
                elif c == "/" and at(1) == "*":
                    bdepth = 1
                    adv(2)
                    while i < n and bdepth > 0:
                        if src[i] == "/" and at(1) == "*":
                            bdepth += 1
                            adv(2)
                        elif src[i] == "*" and at(1) == "/":
                            bdepth -= 1
                            adv(2)
                        else:
                            adv()
                elif c == '"' or _string_ahead(src, i):
                    if not skip_string_literal():
                        adv()
                else:
                    adv()
            inner = src[inner_start : i - 1] if i > inner_start else ""
            toks.append(Tok("attr", inner, start_line, start_col))
            continue

        # String / raw string / byte string
        if ch == '"' or (ch in "rb" and _string_ahead(src, i)):
            if skip_string_literal():
                continue

        # Char literal or lifetime label
        if ch == "'":
            # Lifetime: 'ident not followed by closing quote.
            if _is_ident_start(at(1)):
                j = i + 1
                while j < n and _is_ident_cont(src[j]):
                    j += 1
                if j < n and src[j] == "'":
                    # Char literal like 'a' -- treat as char, skip to closing.
                    adv()
                    while i < n and src[i] != "'":
                        if src[i] == "\\":
                            adv(2)
                        else:
                            adv()
                    if i < n:
                        adv()
                else:
                    # Lifetime label -- skip 'ident as a single token.
                    adv()
                    while i < n and _is_ident_cont(src[i]):
                        adv()
            else:
                # Char literal with escape or symbol: '\n', '\u{..}', ' '
                adv()
                while i < n and src[i] != "'":
                    if src[i] == "\\":
                        adv(2)
                    else:
                        adv()
                if i < n:
                    adv()
            continue

        # Identifier / keyword
        if _is_ident_start(ch):
            start_line, start_col = line, col
            start = i
            adv()
            while i < n and _is_ident_cont(src[i]):
                adv()
            toks.append(Tok("ident", src[start:i], start_line, start_col))
            continue

        # Numbers (skip; never matched)
        if ch.isdigit():
            adv()
            while i < n and (src[i].isalnum() or src[i] in "._"):
                adv()
            continue

        # Punctuation we care about (multi-char first)
        if ch == ":" and at(1) == ":":
            toks.append(Tok("punct", "::", line, col))
            adv(2)
            continue
        if ch in "!(){}[];,<>":
            toks.append(Tok("punct", ch, line, col))
            adv()
            continue

        # Everything else
        adv()

    return toks


def _string_ahead(src: str, i: int) -> bool:
    """Return True if a string/raw-string starts at i (optional b/r prefixes)."""
    n = len(src)
    j = i
    # optional byte prefix 'b'
    if j < n and src[j] == "b":
        j += 1
    # optional raw prefix 'r' -- but only if zero or more hashes are followed
    # by an opening quote. This avoids treating raw identifiers (`r#type`) as
    # raw strings.
    if j < n and src[j] == "r":
        k = j + 1
        while k < n and src[k] == "#":
            k += 1
        return k < n and src[k] == '"'
    # plain string after optional 'b'
    if j < n and src[j] == '"':
        return True
    return False


# ---------------------------------------------------------------------------
# Import resolution + scope tracking + scan
# ---------------------------------------------------------------------------


@dataclass
class Finding:
    path: str
    line: int
    symbol: str
    source: str


@dataclass
class Exception:
    path: str
    symbol: str
    reason: str
    reviewer: str
    scope: str
    expires: str | None


@dataclass
class Report:
    unallowlisted: list[Finding] = field(default_factory=list)
    stale: list[Exception] = field(default_factory=list)
    expired: list[Exception] = field(default_factory=list)
    scanned_files: int = 0
    findings: list[Finding] = field(default_factory=list)

    @property
    def ok(self) -> bool:
        return not self.unallowlisted and not self.stale and not self.expired


def _parse_use_sqlx(toks: list[Tok], start: int) -> tuple[dict[str, str], int]:
    """Parse a `use sqlx::...;` tree starting at the `use` keyword index.

    Returns (local_name -> sqlx_symbol_or_module_alias, end_index) where the
    mapped value is one of the forbidden symbols or the literal "sqlx" for a
    module alias (`use sqlx as s;`). Only `sqlx`-rooted paths are mapped.
    """
    mapping: dict[str, str] = {}
    # Expect ident "use" then "sqlx"
    j = start + 1
    if j >= len(toks) or toks[j].kind != "ident" or toks[j].text != "sqlx":
        return mapping, start + 1
    j += 1

    def resolve_path(path_segments: list[str]) -> str | None:
        # path_segments is the full path after `sqlx::`
        if not path_segments:
            return "sqlx"  # `use sqlx;` -> module alias
        last = path_segments[-1]
        if last in FORBIDDEN_SYMBOLS:
            return last
        return None

    # Now parse the use tree after `sqlx`.
    # Forms:
    #   use sqlx;
    #   use sqlx as s;
    #   use sqlx::query;
    #   use sqlx::query as q;
    #   use sqlx::{query, query_as as qa, QueryBuilder};
    #   use sqlx::types::Uuid;   (ignored)
    def add_mapping(local: str, target: str | None) -> None:
        if target is not None:
            mapping[local] = target

    if j < len(toks) and toks[j].kind == "punct" and toks[j].text == ";":
        # `use sqlx;` -> module alias named sqlx (trivial, already in scope)
        return mapping, j + 1

    if j < len(toks) and toks[j].kind == "ident" and toks[j].text == "as":
        # `use sqlx as s;`
        j += 1
        if j < len(toks) and toks[j].kind == "ident":
            add_mapping(toks[j].text, "sqlx")
            j += 1
        return mapping, j

    # Expect `::`
    if not (j < len(toks) and toks[j].kind == "punct" and toks[j].text == "::"):
        return mapping, j
    j += 1

    def parse_tree(j: int, prefix: list[str]) -> int:
        # Parse one tree node. prefix is the path consumed so far.
        if j >= len(toks):
            return j
        t = toks[j]
        if t.kind == "ident":
            name = t.text
            j += 1
            # `name as alias` ?
            if j < len(toks) and toks[j].kind == "ident" and toks[j].text == "as":
                j += 1
                if j < len(toks) and toks[j].kind == "ident":
                    local = toks[j].text
                    add_mapping(local, resolve_path(prefix + [name]))
                    j += 1
                # expect , or }
                return j
            # `name::` -> nested path, or `name` leaf, or `name::{}`
            if j < len(toks) and toks[j].kind == "punct" and toks[j].text == "::":
                j += 1
                if j < len(toks) and toks[j].kind == "punct" and toks[j].text == "{":
                    j += 1
                    while j < len(toks) and not (
                        toks[j].kind == "punct" and toks[j].text == "}"
                    ):
                        j = parse_tree(j, prefix + [name])
                        if j < len(toks) and toks[j].kind == "punct" and toks[j].text == ",":
                            j += 1
                        else:
                            break
                    if j < len(toks) and toks[j].kind == "punct" and tojs_text(toks[j]) == "}":
                        j += 1
                return j
            # leaf
            add_mapping(name, resolve_path(prefix + [name]))
            return j
        if t.kind == "punct" and t.text == "{":
            j += 1
            while j < len(toks) and not (toks[j].kind == "punct" and toks[j].text == "}"):
                j = parse_tree(j, prefix)
                if j < len(toks) and toks[j].kind == "punct" and toks[j].text == ",":
                    j += 1
                else:
                    break
            if j < len(toks) and toks[j].kind == "punct" and toks[j].text == "}":
                j += 1
            return j
        return j

    j = parse_tree(j, [])
    return mapping, j


def tojs_text(t: Tok) -> str:
    return t.text


def _is_test_path(path: Path) -> bool:
    return any(seg in TEST_DIR_SEGMENTS for seg in path.parts)


def scan_file(path: Path, root: Path, external_test_files: set[Path]) -> list[Finding]:
    """Scan one Rust file for forbidden dynamic SQLx usages in non-test code."""
    rel = path.relative_to(root).as_posix() if path.is_absolute() else path.as_posix()

    # Path-based test exclusion (integration tests).
    if _is_test_path(path):
        return []
    # External test module file (included via `#[cfg(test)] mod x;`).
    if path in external_test_files:
        return []

    try:
        src = path.read_text(encoding="utf-8")
    except (OSError, UnicodeDecodeError):
        return []

    toks = tokenize(src)
    lines = src.splitlines()

    # 1) Build import alias map from `use sqlx::...` statements.
    alias_map: dict[str, str] = {}  # local ident -> sqlx symbol or "sqlx" module alias
    k = 0
    while k < len(toks):
        t = toks[k]
        if t.kind == "ident" and t.text == "use":
            mapping, end = _parse_use_sqlx(toks, k)
            alias_map.update(mapping)
            k = end
            continue
        k += 1

    module_aliases = {name for name, tgt in alias_map.items() if tgt == "sqlx"}

    # 2) Walk tokens tracking brace scope + pending test attribute.
    findings: list[Finding] = []
    brace_stack: list[bool] = []  # per-brace is_test flags
    pending_test_attr = False

    def in_test() -> bool:
        return any(brace_stack)

    def token_after_optional_turbofish(idx: int) -> Tok | None:
        """Return the token that determines macro/call usage after a symbol."""
        m = idx + 1
        if m >= len(toks):
            return None
        if toks[m].kind == "punct" and toks[m].text == "!":
            return toks[m]
        # Dynamic functions can be called as `query_as::<_, T>(...)`; skip the
        # turbofish before checking for the call paren. Qualified paths such as
        # `sqlx::query` are resolved to the `query` token, so this only applies
        # to the optional generic arguments after the forbidden symbol itself.
        if (
            m + 1 < len(toks)
            and toks[m].kind == "punct"
            and toks[m].text == "::"
            and toks[m + 1].kind == "punct"
            and toks[m + 1].text == "<"
        ):
            m += 1
            depth = 0
            while m < len(toks):
                if toks[m].kind == "punct" and toks[m].text == "<":
                    depth += 1
                elif toks[m].kind == "punct" and toks[m].text == ">":
                    depth -= 1
                    if depth == 0:
                        m += 1
                        break
                m += 1
        return toks[m] if m < len(toks) else None

    i = 0
    while i < len(toks):
        t = toks[i]

        if t.kind == "attr":
            pending_test_attr = pending_test_attr or _attr_is_test_scoping(t.text)
            i += 1
            continue

        if t.kind == "punct" and t.text == "{":
            is_test = pending_test_attr or in_test()
            brace_stack.append(is_test)
            pending_test_attr = False
            i += 1
            continue

        if t.kind == "punct" and t.text == "}":
            if brace_stack:
                brace_stack.pop()
            pending_test_attr = False
            i += 1
            continue

        if t.kind == "punct" and t.text == ";":
            # External test mod detection: a test-scoping attribute followed by
            # `mod <ident>;` declares an external test module file.
            pending_test_attr = False
            i += 1
            continue

        if t.kind == "ident" and t.text == "mod" and pending_test_attr and not in_test():
            # `#[cfg(test)] mod <name>;` -> record external test file.
            j = i + 1
            if j < len(toks) and toks[j].kind == "ident":
                mod_name = toks[j].text
                k2 = j + 1
                if k2 < len(toks) and toks[k2].kind == "punct" and toks[k2].text == ";":
                    base = path.parent
                    cand_file = base / f"{mod_name}.rs"
                    cand_mod = base / mod_name / "mod.rs"
                    if cand_file.exists():
                        external_test_files.add(cand_file.resolve())
                    if cand_mod.exists():
                        external_test_files.add(cand_mod.resolve())
                    pending_test_attr = False
                    i = k2 + 1
                    continue

        if t.kind == "ident" and t.text == "use":
            # Imports only establish aliases. Do not report the import token as
            # a dynamic SQLx usage; report the eventual call/type use instead.
            while i < len(toks) and not (
                toks[i].kind == "punct" and toks[i].text == ";"
            ):
                i += 1
            if i < len(toks):
                i += 1
            pending_test_attr = False
            continue

        if t.kind == "ident" and not in_test():
            resolved = _resolve_symbol(t, toks, i, alias_map, module_aliases)
            if resolved is not None:
                symbol, symbol_tok, symbol_idx = resolved
                following = token_after_optional_turbofish(symbol_idx)
                ftext = following.text if following else ""
                is_macro = ftext == "!"
                is_call = ftext == "("
                forbidden = False
                if symbol in TYPE_APIS:
                    forbidden = True  # QueryBuilder: any reference is forbidden
                elif symbol in CALL_APIS:
                    forbidden = is_call and not is_macro
                if forbidden:
                    src_line = (
                        lines[symbol_tok.line - 1].strip()
                        if 0 < symbol_tok.line <= len(lines)
                        else ""
                    )
                    findings.append(Finding(rel, symbol_tok.line, symbol, src_line))
            # consume a qualified path we already matched so we don't re-scan
            # its tail (e.g. `sqlx::query`) as separate idents.
            if t.text == "sqlx" or t.text in module_aliases:
                j = i + 1
                if j < len(toks) and toks[j].kind == "punct" and toks[j].text == "::":
                    j += 1
                    if j < len(toks) and toks[j].kind == "ident":
                        i = j + 1
                        continue
            i += 1
            continue

        i += 1

    return findings


def _resolve_symbol(
    t: Tok,
    toks: list[Tok],
    i: int,
    alias_map: dict[str, str],
    module_aliases: set[str],
) -> tuple[str, Tok, int] | None:
    """Resolve an ident token to (forbidden SQLx symbol, token, index)."""
    # Fully-qualified: sqlx::SYMBOL or ALIAS::SYMBOL. Return the SYMBOL token so
    # call/macro detection examines `query(`, `query!`, or `query_as::<...>(`.
    if t.text == "sqlx" or t.text in module_aliases:
        j = i + 1
        if j < len(toks) and toks[j].kind == "punct" and tojs_text(toks[j]) == "::":
            k = j + 1
            if k < len(toks) and toks[k].kind == "ident" and toks[k].text in FORBIDDEN_SYMBOLS:
                return toks[k].text, toks[k], k
        return None
    # Imported/aliased bare name
    if t.text in alias_map:
        mapped = alias_map[t.text]
        if mapped in FORBIDDEN_SYMBOLS:
            return mapped, t, i
    return None


# ---------------------------------------------------------------------------
# Allowlist + checking
# ---------------------------------------------------------------------------


def _load_toml(path: Path) -> dict:
    try:
        import tomllib  # Python 3.11+
    except ModuleNotFoundError:  # pragma: no cover
        try:
            import tomli as tomllib  # type: ignore
        except ModuleNotFoundError:
            sys.exit(
                "error: this guard needs Python 3.11+ (tomllib) or the 'tomli' "
                "package to read the allowlist."
            )
    with path.open("rb") as fh:
        return tomllib.load(fh)


def load_allowlist(path: Path) -> list[Exception]:
    data = _load_toml(path)
    out: list[Exception] = []
    for raw in data.get("exceptions", []):
        out.append(
            Exception(
                path=raw["path"],
                symbol=raw["symbol"],
                reason=raw.get("reason", ""),
                reviewer=raw.get("reviewer", ""),
                scope=raw.get("scope", "permanent"),
                expires=raw.get("expires"),
            )
        )
    return out


def _iter_rust_files(root: Path) -> Iterator[Path]:
    for dirpath, dirnames, filenames in os.walk(root):
        # prune excluded dirs in place
        dirnames[:] = [d for d in dirnames if d not in EXCLUDE_DIR_NAMES]
        for fn in filenames:
            if fn.endswith(".rs"):
                yield Path(dirpath) / fn


def check_repo(root: Path, allowlist_path: Path) -> Report:
    allowlist = load_allowlist(allowlist_path)
    report = Report()

    # Pass 1: collect external test module files (from `#[cfg(test)] mod x;`).
    external_test_files: set[Path] = set()
    all_files = list(_iter_rust_files(root))
    for f in all_files:
        if _is_test_path(f):
            continue
        try:
            src = f.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        _collect_external_test_mods(f, src, external_test_files)

    # Pass 2: scan every file.
    for f in all_files:
        report.scanned_files += 1
        report.findings.extend(scan_file(f, root, external_test_files))

    # Match findings against allowlist by (path, symbol) count.
    findings_by_key: dict[tuple[str, str], list[Finding]] = {}
    for fnd in report.findings:
        findings_by_key.setdefault((fnd.path, fnd.symbol), []).append(fnd)

    allow_by_key: dict[tuple[str, str], list[Exception]] = {}
    for exc in allowlist:
        allow_by_key.setdefault((exc.path, exc.symbol), []).append(exc)

    keys = set(findings_by_key) | set(allow_by_key)
    for key in keys:
        found = findings_by_key.get(key, [])
        allowed = allow_by_key.get(key, [])
        matched = min(len(found), len(allowed))
        for extra in found[matched:]:
            report.unallowlisted.append(extra)
        for extra in allowed[matched:]:
            report.stale.append(extra)

    # Expired temporary exceptions.
    today = _dt.date.today()
    for exc in allowlist:
        if exc.scope == "temporary" and exc.expires:
            try:
                exp_date = _dt.date.fromisoformat(exc.expires)
            except ValueError:
                report.expired.append(exc)
                continue
            if exp_date < today:
                report.expired.append(exc)

    return report


def _collect_external_test_mods(
    path: Path, src: str, out: set[Path]
) -> None:
    """Find `#[cfg(...test...)] mod <name>;` and record external test files."""
    toks = tokenize(src)
    pending_test = False
    i = 0
    while i < len(toks):
        t = toks[i]
        if t.kind == "attr":
            if _attr_is_test_scoping(t.text):
                pending_test = True
            i += 1
            continue
        if t.kind == "punct" and t.text in ";{}":
            pending_test = False
            i += 1
            continue
        if (
            t.kind == "ident"
            and t.text == "mod"
            and pending_test
        ):
            j = i + 1
            if j < len(toks) and toks[j].kind == "ident":
                name = toks[j].text
                k = j + 1
                if k < len(toks) and toks[k].kind == "punct" and toks[k].text == ";":
                    base = path.parent
                    cf = base / f"{name}.rs"
                    cm = base / name / "mod.rs"
                    if cf.exists():
                        out.add(cf.resolve())
                    if cm.exists():
                        out.add(cm.resolve())
                    i = k + 1
                    pending_test = False
                    continue
        i += 1


# ---------------------------------------------------------------------------
# Reporting
# ---------------------------------------------------------------------------


def format_report(report: Report, allowlist_path: Path) -> str:
    if report.ok:
        return (
            f"ok: no unallowlisted dynamic SQLx usages; "
            f"{report.scanned_files} files scanned; allowlist "
            f"{allowlist_path.name} has no stale or expired entries."
        )

    parts: list[str] = []
    if report.unallowlisted:
        parts.append("ERROR: unallowlisted dynamic SQLx usages in non-test code:")
        for fnd in report.unallowlisted:
            parts.append(
                f"  {fnd.path}:{fnd.line}: sqlx::{fnd.symbol}  ->  {fnd.source}"
            )
        parts.append(
            "  Use a compile-checked SQLx macro (query!/query_as!/query_scalar!/"
            "query_file!/migrate!) or, for a non-preparable admin/DDL path, add a "
            "reviewed exception to scripts/sqlx-dynamic-allowlist.toml."
        )
    if report.stale:
        parts.append("ERROR: stale allowlist entries (no matching usage):")
        for exc in report.stale:
            parts.append(f"  {exc.path}: sqlx::{exc.symbol} ({exc.reason})")
        parts.append("  Remove stale entries so the allowlist stays tight.")
    if report.expired:
        parts.append("ERROR: expired temporary exceptions:")
        for exc in report.expired:
            parts.append(
                f"  {exc.path}: sqlx::{exc.symbol} expired {exc.expires} ({exc.reason})"
            )
        parts.append("  Convert, remove, or re-review and extend the expiration.")
    return "\n".join(parts)


# ---------------------------------------------------------------------------
# Self-test
# ---------------------------------------------------------------------------


def _self_test() -> int:
    import tempfile

    failures: list[str] = []
    tmp = Path(tempfile.mkdtemp(prefix="sqlx-guard-selftest-"))

    def write(rel: str, content: str) -> Path:
        p = tmp / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content, encoding="utf-8")
        return p

    def expect_findings(label: str, content: str, expected: int) -> None:
        p = write(f"crates/demo/src/{label.replace(' ', '_')}.rs", content)
        # Scan with empty external set; path is not under tests/.
        findings = scan_file(p, tmp, set())
        syms = sorted({f.symbol for f in findings})
        if len(findings) != expected:
            failures.append(
                f"[{label}] expected {expected} finding(s), got {len(findings)} "
                f"(symbols={syms})"
            )

    def expect_test_excluded(label: str, content: str) -> None:
        p = write(f"crates/demo/src/{label.replace(' ', '_')}.rs", content)
        findings = scan_file(p, tmp, set())
        if findings:
            failures.append(
                f"[{label}] expected test exclusion, got {len(findings)} finding(s): "
                f"{[(f.line, f.symbol) for f in findings]}"
            )

    # 1. Allowed macros -> no findings.
    expect_findings(
        "allowed_macros",
        """\
fn main() {
    let _ = sqlx::query!("SELECT 1");
    let _ = sqlx::query_as!(Foo, "SELECT * FROM t");
    let _ = sqlx::query_scalar!("SELECT 1");
}
""",
        0,
    )

    # 2. Forbidden dynamic calls.
    expect_findings(
        "forbidden_query",
        """\
async fn f() { sqlx::query("SELECT 1").execute(&pool).await; }
""",
        1,
    )
    expect_findings(
        "forbidden_query_as",
        """\
async fn f() { sqlx::query_as::<_, Foo>("SELECT 1").fetch_one(&pool).await; }
""",
        1,
    )
    expect_findings(
        "forbidden_raw_sql",
        """\
async fn f() { sqlx::raw_sql("SELECT 1").fetch_all(&pool).await; }
""",
        1,
    )

    # 3. Forbidden QueryBuilder (any reference).
    expect_findings(
        "forbidden_query_builder",
        """\
fn f() { let mut q = sqlx::QueryBuilder::new("SELECT"); q.push(" FROM t"); }
""",
        1,
    )

    # 4. Alias: use sqlx::query as dq; dq(...)
    expect_findings(
        "alias_query",
        """\
use sqlx::query as dq;
async fn f() { dq("SELECT 1").execute(&pool).await; }
""",
        1,
    )

    # 4b. Group import with QueryBuilder alias.
    expect_findings(
        "group_import",
        """\
use sqlx::{query as dq, QueryBuilder};
fn f() { dq("SELECT 1"); let mut b = QueryBuilder::new("SELECT"); }
""",
        2,
    )

    # 4c. Module alias: use sqlx as s; s::query(...)
    expect_findings(
        "module_alias",
        """\
use sqlx as s;
async fn f() { s::query("SELECT 1").execute(&pool).await; }
""",
        1,
    )

    # 4d. Macro through alias stays allowed.
    expect_findings(
        "alias_macro_allowed",
        """\
use sqlx::query;
fn f() { let _ = query!("SELECT 1"); }
""",
        0,
    )

    # 5. Test exclusions.
    expect_test_excluded(
        "inline_cfg_test_mod",
        """\
pub fn real() { sqlx::query!("SELECT 1"); }
#[cfg(test)]
mod tests {
    fn helper() { sqlx::query("SELECT 1"); }
}
""",
    )
    expect_test_excluded(
        "test_fn",
        """\
#[test]
fn t() { sqlx::query("SELECT 1"); }
""",
    )
    expect_test_excluded(
        "tokio_test_fn",
        """\
#[tokio::test]
async fn t() { sqlx::query("SELECT 1").execute(&pool).await; }
""",
    )

    # 6. String/comment exclusion.
    expect_findings(
        "string_comment",
        """\
fn f() {
    let s = "sqlx::query(\\"SELECT 1\\")";
    // sqlx::query("SELECT 1")
    /* sqlx::QueryBuilder::new("x") */
}
""",
        0,
    )

    # 7. Non-test module named tests with braces is still scanned unless cfg(test).
    expect_findings(
        "non_cfg_tests_mod",
        """\
pub mod tests {
    pub fn helper() { sqlx::query("SELECT 1"); }
}
""",
        1,
    )

    # 8. Allowlist matching via check_repo. Use a clean repo fixture so the
    # single-file scanner fixtures above do not count as repository findings.
    allow_tmp = Path(tempfile.mkdtemp(prefix="sqlx-guard-repo-"))

    def write_repo(rel: str, content: str) -> Path:
        p = allow_tmp / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(content, encoding="utf-8")
        return p

    allow = write_repo(
        "scripts/sqlx-dynamic-allowlist.toml",
        """\
[[exceptions]]
path = "crates/demo/src/allowlisted.rs"
symbol = "query"
reason = "test fixture"
reviewer = "self-test"
scope = "permanent"
""",
    )
    write_repo(
        "crates/demo/src/allowlisted.rs",
        """\
async fn f() { sqlx::query("SELECT 1").execute(&pool).await; }
""",
    )
    write_repo(
        "crates/demo/src/extra.rs",
        """\
async fn f() { sqlx::query("SELECT 1").execute(&pool).await; }
""",
    )
    # Stale entry
    stale_allow = write_repo(
        "scripts/sqlx-dynamic-allowlist-stale.toml",
        """\
[[exceptions]]
path = "crates/demo/src/does_not_exist.rs"
symbol = "query"
reason = "stale"
reviewer = "self-test"
scope = "permanent"
""",
    )
    # Expired temporary entry
    expired_allow = write_repo(
        "scripts/sqlx-dynamic-allowlist-expired.toml",
        f"""\
[[exceptions]]
path = "crates/demo/src/allowlisted.rs"
symbol = "query"
reason = "expired"
reviewer = "self-test"
scope = "temporary"
expires = "2000-01-01"
""",
    )

    # 8a. With the matching allowlist, the allowlisted finding is OK; extra is not.
    rep = check_repo(allow_tmp, allow)
    unallow_paths = sorted({f.path for f in rep.unallowlisted})
    if unallow_paths != ["crates/demo/src/extra.rs"]:
        failures.append(
            f"[allowlist] expected only extra.rs unallowlisted, got {unallow_paths}"
        )
    if rep.stale:
        failures.append(f"[allowlist] expected no stale, got {[(e.path, e.symbol) for e in rep.stale]}")
    if rep.expired:
        failures.append(f"[allowlist] expected no expired, got {rep.expired}")

    # 8b. Stale allowlist -> reported.
    rep = check_repo(allow_tmp, stale_allow)
    # allowlisted.rs + extra.rs both unallowlisted (no matching allow); plus stale entry.
    if not rep.stale:
        failures.append("[stale] expected a stale allowlist entry")
    if not rep.unallowlisted:
        failures.append("[stale] expected unallowlisted findings for the real usages")

    # 8c. Expired temporary exception -> reported.
    rep = check_repo(allow_tmp, expired_allow)
    if not rep.expired:
        failures.append("[expired] expected an expired exception")

    # 8d. External test mod exclusion (#[cfg(test)] mod tests; -> tests.rs).
    extmod = write_repo(
        "crates/demo/src/extmod.rs",
        """\
pub fn real() { sqlx::query!("SELECT 1"); }
#[cfg(test)]
mod tests;
""",
    )
    write_repo(
        "crates/demo/src/tests.rs",
        """\
#[test]
fn t() { sqlx::query("SELECT 1"); }
""",
    )
    ext_rep = check_repo(allow_tmp, allow)
    # No finding should come from extmod's tests.rs.
    bad = [f for f in ext_rep.findings if f.path == "crates/demo/src/tests.rs"]
    if bad:
        failures.append(f"[external-mod] tests.rs should be excluded, got {bad}")

    # Cleanup.
    import shutil

    shutil.rmtree(tmp, ignore_errors=True)
    shutil.rmtree(allow_tmp, ignore_errors=True)

    if failures:
        print("self-test FAILED:")
        for f_ in failures:
            print(f"  - {f_}")
        return 1
    print("self-test OK: all fixture cases passed.")
    return 0


# ---------------------------------------------------------------------------
# Main
# ---------------------------------------------------------------------------


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="SQLx dynamic-query guard.")
    parser.add_argument("--root", type=Path, default=DEFAULT_ROOT, help="repo root")
    parser.add_argument(
        "--allowlist",
        type=Path,
        default=DEFAULT_ALLOWLIST,
        help="path to the TOML allowlist",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run built-in fixture tests and exit",
    )
    args = parser.parse_args(argv)

    if args.self_test:
        return _self_test()

    if not args.allowlist.exists():
        print(f"error: allowlist not found: {args.allowlist}", file=sys.stderr)
        return 1
    if not args.root.exists():
        print(f"error: root not found: {args.root}", file=sys.stderr)
        return 1

    report = check_repo(args.root, args.allowlist)
    print(format_report(report, args.allowlist))
    return 0 if report.ok else 1


if __name__ == "__main__":
    sys.exit(main())
