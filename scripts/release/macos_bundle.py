#!/usr/bin/env python3
"""Stage and verify a relocatable Ferrex macOS application bundle.

The bundler copies the complete non-system Mach-O dependency closure into
``Contents/Frameworks``, rewrites install names to ``@rpath``, removes package
manager/developer rpaths, and signs only after all binary mutation is complete.
It is intentionally independent of Homebrew layout: search roots are build
inputs, never runtime lookup paths.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import plistlib
import re
import shutil
import subprocess
import sys
from dataclasses import dataclass, replace
from pathlib import Path
from typing import Iterable, Sequence


APP_RPATH = "@executable_path/../Frameworks"
GSTREAMER_PLUGIN_RPATH = "@loader_path/../../Frameworks"
GIO_MODULE_RPATH = "@loader_path/../../../Frameworks"
PRESENTER_BUILD_MODES = frozenset({"disabled", "spike"})
FORBIDDEN_RUNTIME_PREFIXES = (
    "/opt/homebrew/",
    "/usr/local/",
    "/nix/store/",
)
SYSTEM_LIBRARY_PREFIXES = (
    "/System/Library/",
    "/usr/lib/",
)
FORBIDDEN_GSTREAMER_PLUGIN_NAMES = {
    "libgstassrender.dylib",
    "libgstlibav.dylib",
    "libgstx264.dylib",
    "libgstx265.dylib",
}
FORBIDDEN_GSTREAMER_DEPENDENCY_PREFIXES = (
    "libass.",
    "libavcodec.",
    "libavdevice.",
    "libavfilter.",
    "libavformat.",
    "libavutil.",
    "libfaac.",
    "libfdk-aac.",
    "libswresample.",
    "libswscale.",
    "libx264.",
    "libx265.",
)


class BundleError(RuntimeError):
    """A bundle cannot satisfy the relocatable install-name policy."""


@dataclass(frozen=True)
class MachORecord:
    path: Path
    install_id: str | None
    dependencies: tuple[str, ...]
    rpaths: tuple[str, ...]
    architectures: tuple[str, ...]
    minimum_macos: str | None
    executable: bool = False
    install_id_required: bool = True


@dataclass
class StagedMachO:
    source: Path
    staged: Path
    record: MachORecord
    dependency_targets: dict[str, str]


def run(
    arguments: Sequence[str | os.PathLike[str]],
    *,
    check: bool = True,
) -> subprocess.CompletedProcess[str]:
    command = [os.fspath(argument) for argument in arguments]
    completed = subprocess.run(
        command,
        check=False,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    if check and completed.returncode != 0:
        detail = completed.stderr.strip() or completed.stdout.strip()
        raise BundleError(f"command failed ({' '.join(command)}): {detail}")
    return completed


def parse_otool_libraries(output: str) -> tuple[str, ...]:
    dependencies: list[str] = []
    for line in output.splitlines()[1:]:
        match = re.match(r"^\s+(.+?)\s+\(compatibility version ", line)
        if match:
            dependencies.append(match.group(1))
    return tuple(dependencies)


def parse_otool_rpaths(output: str) -> tuple[str, ...]:
    lines = output.splitlines()
    rpaths: list[str] = []
    for index, line in enumerate(lines):
        if line.strip() != "cmd LC_RPATH":
            continue
        for candidate in lines[index + 1 : index + 5]:
            match = re.match(r"^\s*path (.+?) \(offset \d+\)$", candidate)
            if match:
                rpaths.append(match.group(1))
                break
    return tuple(rpaths)


def parse_architectures(output: str) -> tuple[str, ...]:
    return tuple(part for part in output.strip().split() if part)


def parse_macos_deployment_target(output: str) -> str | None:
    """Return the Mach-O's LC_BUILD_VERSION/LC_VERSION_MIN_MACOSX target."""
    lines = output.splitlines()
    targets: list[str] = []
    for index, line in enumerate(lines):
        command = line.strip()
        if command not in {"cmd LC_BUILD_VERSION", "cmd LC_VERSION_MIN_MACOSX"}:
            continue
        field = "minos" if command == "cmd LC_BUILD_VERSION" else "version"
        for candidate in lines[index + 1 : index + 8]:
            match = re.match(rf"^\s*{field}\s+(\d+(?:\.\d+)*)\s*$", candidate)
            if match:
                targets.append(match.group(1))
                break
    if not targets:
        return None
    # Fat inputs can contain one load-command block per architecture. Audit
    # the strictest slice so a lower first slice cannot hide a newer minimum.
    return max(targets, key=version_tuple)


def version_tuple(value: str) -> tuple[int, ...]:
    if not re.fullmatch(r"\d+(?:\.\d+)*", value):
        raise BundleError(f"invalid numeric version: {value!r}")
    return tuple(int(component) for component in value.split("."))


def apple_bundle_version(version: str) -> str:
    """Normalize a Cargo semver to Apple's numeric bundle-version grammar."""
    match = re.match(r"^(\d+)(?:\.(\d+))?(?:\.(\d+))?", version)
    if match is None:
        raise BundleError(f"cannot derive Apple bundle version from {version!r}")
    return ".".join(component for component in match.groups(default="0"))


def is_system_library(install_name: str) -> bool:
    return install_name.startswith(SYSTEM_LIBRARY_PREFIXES)


def has_forbidden_runtime_prefix(value: str) -> bool:
    return value.startswith(FORBIDDEN_RUNTIME_PREFIXES)


def macho_record(
    path: Path,
    *,
    executable: bool = False,
    install_id_required: bool | None = None,
) -> MachORecord:
    libraries = parse_otool_libraries(run(["otool", "-L", path]).stdout)
    id_result = run(["otool", "-D", path], check=False)
    install_id = None
    if id_result.returncode == 0:
        id_lines = [line.strip() for line in id_result.stdout.splitlines()[1:]]
        install_id = next((line for line in id_lines if line), None)
    dependencies = tuple(
        dependency for dependency in libraries if dependency != install_id
    )
    load_commands = run(["otool", "-l", path]).stdout
    architectures = parse_architectures(run(["lipo", "-archs", path]).stdout)
    return MachORecord(
        path=path,
        install_id=install_id,
        dependencies=dependencies,
        rpaths=parse_otool_rpaths(load_commands),
        architectures=architectures,
        minimum_macos=parse_macos_deployment_target(load_commands),
        executable=executable,
        install_id_required=(not executable)
        if install_id_required is None
        else install_id_required,
    )


def expand_special_path(
    value: str,
    *,
    loader: Path,
    executable: Path,
) -> Path | None:
    replacements = {
        "@loader_path": loader.parent,
        "@executable_path": executable.parent,
    }
    for marker, root in replacements.items():
        if value == marker:
            return root
        if value.startswith(f"{marker}/"):
            return root / value[len(marker) + 1 :]
    if value.startswith("@"):
        return None
    return Path(value)


class DependencyResolver:
    def __init__(self, search_roots: Iterable[Path], executable: Path):
        self.search_roots = tuple(root.resolve() for root in search_roots)
        self.executable = executable.resolve()
        self._basename_index: dict[str, list[Path]] | None = None
        self._allowed_sources: set[Path] | None = None
        self._record_cache: dict[Path, MachORecord] = {}

    def record(
        self,
        path: Path,
        *,
        executable: bool = False,
        install_id_required: bool | None = None,
    ) -> MachORecord:
        resolved = path.resolve()
        cached = self._record_cache.get(resolved)
        if cached is None:
            cached = macho_record(resolved)
            self._record_cache[resolved] = cached
        return replace(
            cached,
            executable=executable,
            install_id_required=(not executable)
            if install_id_required is None
            else install_id_required,
        )

    def resolve(self, install_name: str, loader: Path) -> Path:
        if is_system_library(install_name):
            return Path(install_name)

        expanded = expand_special_path(
            install_name,
            loader=loader,
            executable=self.executable,
        )
        if expanded is not None and expanded.exists():
            return self._require_declared_source(expanded.resolve(), install_name, loader)

        if install_name.startswith("@rpath/"):
            suffix = install_name[len("@rpath/") :]
            records = [self.record(loader)]
            if loader.resolve() != self.executable:
                records.append(self.record(self.executable, executable=True))
            for record in records:
                for rpath in record.rpaths:
                    root = expand_special_path(
                        rpath,
                        loader=loader,
                        executable=self.executable,
                    )
                    if root is not None:
                        candidate = root / suffix
                        if candidate.exists():
                            return self._require_declared_source(
                                candidate.resolve(), install_name, loader
                            )

        basename = Path(install_name).name
        candidates = self._index().get(basename, [])
        if not candidates:
            raise BundleError(
                f"cannot resolve {install_name!r} required by {loader}"
            )
        if len(candidates) != 1:
            choices = ", ".join(str(candidate) for candidate in candidates)
            raise BundleError(
                f"ambiguous dependency {install_name!r} required by {loader}: "
                f"{choices}"
            )
        return candidates[0]

    def _require_declared_source(
        self,
        candidate: Path,
        install_name: str,
        loader: Path,
    ) -> Path:
        # An existing absolute install name is not sufficient evidence that a
        # dependency belongs in a release. It must also be reachable through a
        # caller-declared search root, including through a symlink in that root.
        self._index()
        assert self._allowed_sources is not None
        if candidate not in self._allowed_sources:
            raise BundleError(
                f"dependency {install_name!r} required by {loader} resolves outside "
                f"declared search roots: {candidate}"
            )
        return candidate

    def _index(self) -> dict[str, list[Path]]:
        if self._basename_index is None:
            index: dict[str, list[Path]] = {}
            allowed_sources: set[Path] = set()
            for root in self.search_roots:
                if not root.is_dir():
                    raise BundleError(f"library search root is not a directory: {root}")
                for candidate in root.rglob("*"):
                    if candidate.is_file() and (
                        candidate.suffix == ".dylib"
                        or ".framework/" in candidate.as_posix()
                    ):
                        resolved = candidate.resolve()
                        allowed_sources.add(resolved)
                        bucket = index.setdefault(candidate.name, [])
                        if resolved not in bucket:
                            bucket.append(resolved)
            self._basename_index = index
            self._allowed_sources = allowed_sources
        return self._basename_index


def dependency_destination_name(
    preferred_name: str,
    source: Path,
    occupied: dict[str, Path],
) -> str:
    """Return a deterministic non-colliding name for an exact dependency."""
    resolved = source.resolve()
    previous = occupied.get(preferred_name)
    if previous is None or previous == resolved:
        return preferred_name

    digest = hashlib.sha256(resolved.read_bytes()).hexdigest()
    suffix = Path(preferred_name).suffix
    stem = preferred_name[: -len(suffix)] if suffix else preferred_name
    for length in range(12, len(digest) + 1, 4):
        candidate = f"{stem}-{digest[:length]}{suffix}"
        previous = occupied.get(candidate)
        if previous is None or previous == resolved:
            return candidate
    raise BundleError(
        f"could not allocate a unique bundle name for {preferred_name}: {resolved}"
    )


def copy_dependency_closure(
    binary: Path,
    staged_binary: Path,
    frameworks: Path,
    search_roots: Iterable[Path],
    additional_seeds: Iterable[tuple[Path, Path, bool, bool]] = (),
) -> list[StagedMachO]:
    resolver = DependencyResolver(search_roots, binary)
    queue: list[tuple[Path, Path, bool, bool]] = [
        (binary.resolve(), staged_binary, True, False)
    ]
    staged_by_source: dict[Path, Path] = {binary.resolve(): staged_binary}
    source_by_basename: dict[str, Path] = {staged_binary.name: binary.resolve()}
    for (
        seed_source,
        seed_target,
        seed_executable,
        seed_install_id_required,
    ) in additional_seeds:
        resolved_seed = seed_source.resolve()
        previous_source = source_by_basename.get(seed_target.name)
        if previous_source is not None and previous_source != resolved_seed:
            raise BundleError(
                "bundle contains two different Mach-O files named "
                f"{seed_target.name}: {previous_source} and {resolved_seed}"
            )
        previous_target = staged_by_source.get(resolved_seed)
        if previous_target is not None and previous_target != seed_target:
            raise BundleError(
                f"Mach-O seed {resolved_seed} was staged twice as "
                f"{previous_target} and {seed_target}"
            )
        source_by_basename[seed_target.name] = resolved_seed
        staged_by_source[resolved_seed] = seed_target
        queue.append(
            (
                resolved_seed,
                seed_target,
                seed_executable,
                seed_install_id_required,
            )
        )
    staged_records: list[StagedMachO] = []

    while queue:
        source, staged, executable, install_id_required = queue.pop(0)
        record = resolver.record(
            source,
            executable=executable,
            install_id_required=install_id_required,
        )
        targets: dict[str, str] = {}
        for dependency in record.dependencies:
            if is_system_library(dependency):
                continue
            resolved = resolver.resolve(dependency, source)
            # Preserve the dependency's install-name basename (its SONAME),
            # even when resolving a symlink yields a more-versioned filename.
            # Callers and --require-library intentionally refer to this name.
            basename = Path(dependency).name or resolved.name
            existing_target = staged_by_source.get(resolved)
            if existing_target is not None:
                basename = existing_target.name
            else:
                # Nix closures can legitimately contain ABI-distinct libraries
                # with the same SONAME. Their load commands identify exact store
                # paths, so give colliding transitive files deterministic names
                # and rewrite each caller to the corresponding bundled target.
                basename = dependency_destination_name(
                    basename, resolved, source_by_basename
                )
            source_by_basename[basename] = resolved
            target = frameworks / basename
            targets[dependency] = basename
            if existing_target is None:
                shutil.copy2(resolved, target)
                target.chmod(target.stat().st_mode | 0o200)
                staged_by_source[resolved] = target
                queue.append((resolved, target, False, True))
        staged_records.append(
            StagedMachO(
                source=source,
                staged=staged,
                record=record,
                dependency_targets=targets,
            )
        )

    return staged_records


def rewrite_install_names(records: Iterable[StagedMachO]) -> None:
    for staged in records:
        for old_name, basename in staged.dependency_targets.items():
            run(
                [
                    "install_name_tool",
                    "-change",
                    old_name,
                    f"@rpath/{basename}",
                    staged.staged,
                ]
            )
        if staged.record.install_id_required:
            run(
                [
                    "install_name_tool",
                    "-id",
                    f"@rpath/{staged.staged.name}",
                    staged.staged,
                ]
            )
        for rpath in staged.record.rpaths:
            if rpath in {APP_RPATH, GSTREAMER_PLUGIN_RPATH, GIO_MODULE_RPATH} or rpath.startswith(
                SYSTEM_LIBRARY_PREFIXES
            ):
                continue
            run(["install_name_tool", "-delete_rpath", rpath, staged.staged])
        required_rpath = None
        if staged.record.executable:
            required_rpath = APP_RPATH
        elif "Contents/Resources/gstreamer-1.0" in staged.staged.as_posix():
            # GStreamer's soup loader opens libsoup by leaf name rather than a
            # Mach-O import. Give dyld a bundle-local search path for that load.
            required_rpath = GSTREAMER_PLUGIN_RPATH
        elif "Contents/Resources/gio/modules" in staged.staged.as_posix():
            required_rpath = GIO_MODULE_RPATH
        if required_rpath is not None:
            current = macho_record(staged.staged, executable=staged.record.executable)
            if required_rpath not in current.rpaths:
                run(
                    [
                        "install_name_tool",
                        "-add_rpath",
                        required_rpath,
                        staged.staged,
                    ]
                )


def write_info_plist(
    contents: Path,
    *,
    executable_name: str,
    bundle_name: str,
    bundle_identifier: str,
    version: str,
    minimum_macos: str,
) -> None:
    normalized_version = apple_bundle_version(version)
    info = {
        "CFBundleDevelopmentRegion": "en",
        "CFBundleExecutable": executable_name,
        "CFBundleIdentifier": bundle_identifier,
        "CFBundleInfoDictionaryVersion": "6.0",
        "CFBundleName": bundle_name,
        "CFBundleDisplayName": bundle_name,
        "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": normalized_version,
        "CFBundleVersion": normalized_version,
        "LSMinimumSystemVersion": minimum_macos,
        "NSHighResolutionCapable": True,
        "NSSupportsAutomaticGraphicsSwitching": True,
    }
    with (contents / "Info.plist").open("wb") as plist:
        plistlib.dump(info, plist, sort_keys=True)


def copy_resources(resources: Iterable[Path], destination: Path) -> None:
    for resource in resources:
        if not resource.exists():
            raise BundleError(f"bundle resource does not exist: {resource}")
        target = destination / resource.name
        if resource.is_dir():
            shutil.copytree(resource, target)
        else:
            shutil.copy2(resource, target)


def copy_macho_seed(
    source: Path,
    target: Path,
    *,
    executable: bool,
    install_id_required: bool | None = None,
) -> tuple[Path, Path, bool, bool]:
    if not source.is_file():
        raise BundleError(f"Mach-O bundle input does not exist: {source}")
    target.parent.mkdir(parents=True, exist_ok=True)
    if target.exists():
        raise BundleError(f"duplicate Mach-O bundle destination: {target}")
    shutil.copy2(source, target)
    mode = target.stat().st_mode | 0o200
    if executable:
        mode |= 0o111
    target.chmod(mode)
    return (
        source,
        target,
        executable,
        (not executable) if install_id_required is None else install_id_required,
    )


def stage_gstreamer_runtime(
    plugin_directories: Iterable[Path],
    plugin_files: Iterable[Path],
    scanner: Path | None,
    contents: Path,
    excluded_plugins: Iterable[str] = (),
) -> list[tuple[Path, Path, bool, bool]]:
    directories = tuple(plugin_directories)
    files = tuple(plugin_files)
    if not directories and not files and scanner is None:
        return []
    if (not directories and not files) or scanner is None:
        raise BundleError(
            "GStreamer packaging requires plugin inputs and --gstreamer-scanner"
        )

    seeds: list[tuple[Path, Path, bool, bool]] = []
    exclusions = set(excluded_plugins)
    plugin_target = contents / "Resources/gstreamer-1.0"
    seen_names: dict[str, Path] = {}
    candidates = list(files)
    for directory in directories:
        if not directory.is_dir():
            raise BundleError(f"GStreamer plugin directory does not exist: {directory}")
        candidates.extend(sorted(directory.rglob("*.dylib")))
    for plugin in candidates:
        if not plugin.is_file():
            raise BundleError(f"GStreamer plugin does not exist: {plugin}")
        if plugin.suffix != ".dylib":
            raise BundleError(f"GStreamer plugin is not a .dylib: {plugin}")
        if plugin.name in exclusions:
            continue
        resolved = plugin.resolve()
        previous = seen_names.get(plugin.name)
        if previous is not None and previous != resolved:
            raise BundleError(
                f"two GStreamer plugins are named {plugin.name}: "
                f"{previous} and {resolved}"
            )
        if previous is None:
            seen_names[plugin.name] = resolved
            seeds.append(
                copy_macho_seed(
                    plugin,
                    plugin_target / plugin.name,
                    executable=False,
                )
            )
    if not seeds:
        raise BundleError("no GStreamer .dylib plugins were found")
    manifest = contents / "Resources/gstreamer-plugin-manifest.txt"
    manifest.write_text(
        "".join(f"{name}\n" for name in sorted(seen_names)),
        encoding="utf-8",
    )
    seeds.append(
        copy_macho_seed(
            scanner,
            contents / "Helpers/gst-plugin-scanner",
            executable=True,
        )
    )
    return seeds


def stage_vulkan_icd(source: Path, resources: Path, library_name: str) -> Path:
    if not source.is_file():
        raise BundleError(f"Vulkan ICD manifest does not exist: {source}")
    try:
        manifest = json.loads(source.read_text(encoding="utf-8"))
        icd = manifest["ICD"]
    except (OSError, json.JSONDecodeError, KeyError, TypeError) as error:
        raise BundleError(f"invalid Vulkan ICD manifest {source}: {error}") from error
    if not isinstance(icd, dict):
        raise BundleError(f"invalid Vulkan ICD object in {source}")
    icd["library_path"] = f"../../../Frameworks/{library_name}"
    target = resources / "vulkan/icd.d/MoltenVK_icd.json"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_text(json.dumps(manifest, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    return target


def stage_ca_bundle(source: Path, resources: Path) -> Path:
    if not source.is_file():
        raise BundleError(f"CA bundle does not exist: {source}")
    data = source.read_bytes()
    if b"-----BEGIN CERTIFICATE-----" not in data:
        raise BundleError(f"CA bundle contains no PEM certificates: {source}")
    target = resources / "tls/cacert.pem"
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(data)
    (target.parent / "cacert.pem.sha256").write_text(
        hashlib.sha256(data).hexdigest() + "  cacert.pem\n",
        encoding="utf-8",
    )
    return target


def stage_presenter_build_mode(mode: str, resources: Path) -> Path:
    if mode not in PRESENTER_BUILD_MODES:
        raise BundleError(f"invalid macOS presenter build mode: {mode!r}")
    target = resources / "presenter-build-mode.txt"
    target.write_text(f"{mode}\n", encoding="utf-8")
    return target


def load_presenter_build_mode(app: Path) -> str:
    path = app / "Contents/Resources/presenter-build-mode.txt"
    try:
        mode = path.read_text(encoding="utf-8").strip()
    except OSError as error:
        raise BundleError(f"presenter build-mode evidence is missing: {path}") from error
    if mode not in PRESENTER_BUILD_MODES:
        raise BundleError(f"invalid presenter build-mode evidence: {mode!r}")
    return mode


def stage_gio_modules(
    modules: Iterable[Path], contents: Path
) -> list[tuple[Path, Path, bool, bool]]:
    seeds: list[tuple[Path, Path, bool, bool]] = []
    destination = contents / "Resources/gio/modules"
    for module in modules:
        seeds.append(
            copy_macho_seed(
                module,
                destination / module.name,
                executable=False,
                # GIO modules are Mach-O bundles rather than dylibs and do not
                # carry LC_ID_DYLIB.
                install_id_required=False,
            )
        )
    return seeds


def sign_bundle(app: Path, identity: str, nested_machos: Iterable[Path]) -> None:
    # Sign every nested Mach-O explicitly after install-name mutation. The app
    # signature is last so its sealed-resource envelope includes final bytes.
    for library in sorted(set(nested_machos), key=lambda path: (-len(path.parts), str(path))):
        arguments = ["codesign", "--force", "--sign", identity]
        if identity == "-":
            arguments.append("--timestamp=none")
        else:
            arguments.extend(["--options", "runtime", "--timestamp"])
        run([*arguments, library])

    arguments = ["codesign", "--force", "--sign", identity]
    if identity == "-":
        arguments.append("--timestamp=none")
    else:
        arguments.extend(["--options", "runtime", "--timestamp"])
    run([*arguments, app])


def audit_records(
    records: Iterable[MachORecord],
    *,
    framework_names: set[str],
    required_architecture: str | None,
    executable_name: str,
    maximum_deployment_target: str,
) -> None:
    records = tuple(records)
    errors: list[str] = []
    maximum_target = version_tuple(maximum_deployment_target)
    for record in records:
        if required_architecture and required_architecture not in record.architectures:
            errors.append(
                f"{record.path.name} lacks architecture {required_architecture}: "
                f"{','.join(record.architectures)}"
            )
        if record.minimum_macos is None:
            errors.append(f"{record.path.name} has no macOS deployment-target load command")
        elif version_tuple(record.minimum_macos) > maximum_target:
            errors.append(
                f"{record.path.name} requires macOS {record.minimum_macos}, newer than "
                f"bundle minimum {maximum_deployment_target}"
            )
        if (
            record.install_id_required
            and record.install_id != f"@rpath/{record.path.name}"
        ):
            errors.append(
                f"{record.path.name} has non-relocatable id {record.install_id!r}"
            )
        for dependency in record.dependencies:
            if is_system_library(dependency):
                continue
            if has_forbidden_runtime_prefix(dependency):
                errors.append(
                    f"{record.path.name} retains developer dependency {dependency}"
                )
                continue
            if not dependency.startswith("@rpath/"):
                errors.append(
                    f"{record.path.name} has non-canonical dependency {dependency}"
                )
                continue
            basename = dependency[len("@rpath/") :]
            if basename not in framework_names:
                errors.append(
                    f"{record.path.name} dependency {dependency} is not bundled"
                )
        for rpath in record.rpaths:
            if has_forbidden_runtime_prefix(rpath):
                errors.append(f"{record.path.name} retains developer rpath {rpath}")
        if record.executable and APP_RPATH not in record.rpaths:
            errors.append(f"{executable_name} lacks bundle rpath {APP_RPATH}")
    if errors:
        raise BundleError("invalid Mach-O bundle closure:\n- " + "\n- ".join(errors))


def audit_gstreamer_runtime(app: Path, records: Iterable[MachORecord]) -> None:
    plugins = app / "Contents/Resources/gstreamer-1.0"
    if not plugins.is_dir():
        return
    actual_names = {path.name for path in plugins.glob("*.dylib") if path.is_file()}
    forbidden_names = sorted(actual_names & FORBIDDEN_GSTREAMER_PLUGIN_NAMES)
    if forbidden_names:
        raise BundleError(
            "forbidden GStreamer plugins are bundled: " + ", ".join(forbidden_names)
        )
    manifest_path = app / "Contents/Resources/gstreamer-plugin-manifest.txt"
    if not manifest_path.is_file():
        raise BundleError("bundled GStreamer plugin manifest is missing")
    manifest_names = {
        line.strip()
        for line in manifest_path.read_text(encoding="utf-8").splitlines()
        if line.strip()
    }
    if manifest_names != actual_names:
        raise BundleError("bundled GStreamer plugin manifest does not match staged files")

    errors: list[str] = []
    for record in records:
        if plugins not in record.path.parents:
            continue
        if GSTREAMER_PLUGIN_RPATH not in record.rpaths:
            errors.append(
                f"{record.path.name} lacks bundle dynamic-loader rpath "
                f"{GSTREAMER_PLUGIN_RPATH}"
            )
        for dependency in record.dependencies:
            basename = Path(dependency).name
            if basename.startswith(FORBIDDEN_GSTREAMER_DEPENDENCY_PREFIXES):
                errors.append(f"{record.path.name} links forbidden dependency {basename}")
    if errors:
        raise BundleError(
            "invalid GStreamer license/closure surface:\n- " + "\n- ".join(errors)
        )


def load_bundle_records(app: Path) -> tuple[list[MachORecord], str, str]:
    info_path = app / "Contents/Info.plist"
    if not info_path.is_file():
        raise BundleError(f"missing bundle metadata: {info_path}")
    with info_path.open("rb") as plist:
        info = plistlib.load(plist)
    executable_name = info.get("CFBundleExecutable")
    if not isinstance(executable_name, str) or not executable_name:
        raise BundleError("Info.plist does not define CFBundleExecutable")
    executable = app / "Contents/MacOS" / executable_name
    if not executable.is_file():
        raise BundleError(f"missing application executable: {executable}")
    minimum_macos = info.get("LSMinimumSystemVersion")
    if not isinstance(minimum_macos, str):
        raise BundleError("Info.plist does not define LSMinimumSystemVersion")
    version_tuple(minimum_macos)
    frameworks = app / "Contents/Frameworks"
    records = [macho_record(executable, executable=True)]
    records.extend(macho_record(path) for path in sorted(frameworks.rglob("*")) if path.is_file())
    plugins = app / "Contents/Resources/gstreamer-1.0"
    if plugins.is_dir():
        records.extend(
            macho_record(path) for path in sorted(plugins.rglob("*.dylib")) if path.is_file()
        )
    scanner = app / "Contents/Helpers/gst-plugin-scanner"
    if scanner.is_file():
        records.append(macho_record(scanner, executable=True))
    gio_modules = app / "Contents/Resources/gio/modules"
    if gio_modules.is_dir():
        records.extend(
            macho_record(path, install_id_required=False)
            for path in sorted(gio_modules.iterdir())
            if path.is_file()
        )
    return records, executable_name, minimum_macos


def verify_bundle(
    app: Path,
    *,
    required_architecture: str | None,
    required_libraries: Iterable[str],
    verify_signature: bool,
    require_gstreamer_runtime: bool = False,
    require_vulkan_icd: bool = False,
    expected_presenter_mode: str | None = None,
) -> None:
    records, executable_name, minimum_macos = load_bundle_records(app)
    presenter_mode = load_presenter_build_mode(app)
    if expected_presenter_mode is not None and presenter_mode != expected_presenter_mode:
        raise BundleError(
            f"presenter build mode is {presenter_mode!r}, expected "
            f"{expected_presenter_mode!r}"
        )
    frameworks = app / "Contents/Frameworks"
    framework_names = {path.name for path in frameworks.iterdir() if path.is_file()}
    missing = sorted(set(required_libraries) - framework_names)
    if missing:
        raise BundleError(f"required bundled libraries are missing: {', '.join(missing)}")
    if require_gstreamer_runtime:
        plugins = app / "Contents/Resources/gstreamer-1.0"
        scanner = app / "Contents/Helpers/gst-plugin-scanner"
        if not plugins.is_dir() or not any(plugins.glob("*.dylib")):
            raise BundleError("required bundled GStreamer plugins are missing")
        if not scanner.is_file() or not os.access(scanner, os.X_OK):
            raise BundleError("required bundled GStreamer plugin scanner is missing")
        gio_modules = app / "Contents/Resources/gio/modules"
        if not gio_modules.is_dir() or not any(gio_modules.iterdir()):
            raise BundleError("required bundled GIO TLS modules are missing")
        for record in records:
            if gio_modules in record.path.parents and GIO_MODULE_RPATH not in record.rpaths:
                raise BundleError(
                    f"{record.path.name} lacks bundle GIO module rpath {GIO_MODULE_RPATH}"
                )
        ca_bundle = app / "Contents/Resources/tls/cacert.pem"
        ca_hash = app / "Contents/Resources/tls/cacert.pem.sha256"
        if not ca_bundle.is_file() or not ca_hash.is_file():
            raise BundleError("required bundled CA trust store is missing")
        data = ca_bundle.read_bytes()
        expected_hash = ca_hash.read_text(encoding="utf-8").split()[0]
        if (
            b"-----BEGIN CERTIFICATE-----" not in data
            or hashlib.sha256(data).hexdigest() != expected_hash
        ):
            raise BundleError("bundled CA trust store hash/content is invalid")
    if require_vulkan_icd:
        manifest_path = app / "Contents/Resources/vulkan/icd.d/MoltenVK_icd.json"
        try:
            manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
            library_path = manifest["ICD"]["library_path"]
        except (OSError, json.JSONDecodeError, KeyError, TypeError) as error:
            raise BundleError(f"required bundled Vulkan ICD is invalid: {error}") from error
        expected = "../../../Frameworks/libMoltenVK.dylib"
        if library_path != expected:
            raise BundleError(
                f"bundled Vulkan ICD uses {library_path!r}, expected {expected!r}"
            )
        if not (frameworks / "libMoltenVK.dylib").is_file():
            raise BundleError("bundled Vulkan ICD library is missing")
    audit_gstreamer_runtime(app, records)
    audit_records(
        records,
        framework_names=framework_names,
        required_architecture=required_architecture,
        executable_name=executable_name,
        maximum_deployment_target=minimum_macos,
    )
    run(["plutil", "-lint", app / "Contents/Info.plist"])
    if verify_signature:
        run(["codesign", "--verify", "--deep", "--strict", "--verbose=2", app])


def stage_bundle(arguments: argparse.Namespace) -> None:
    binary = arguments.binary.resolve()
    app = arguments.app.resolve()
    if not binary.is_file():
        raise BundleError(f"application binary does not exist: {binary}")
    if app.exists():
        raise BundleError(f"refusing to replace existing application bundle: {app}")

    contents = app / "Contents"
    macos = contents / "MacOS"
    frameworks = contents / "Frameworks"
    resources = contents / "Resources"
    macos.mkdir(parents=True)
    frameworks.mkdir()
    resources.mkdir()
    staged_binary = macos / arguments.executable_name
    shutil.copy2(binary, staged_binary)
    staged_binary.chmod(staged_binary.stat().st_mode | 0o111 | 0o200)
    write_info_plist(
        contents,
        executable_name=arguments.executable_name,
        bundle_name=arguments.bundle_name,
        bundle_identifier=arguments.bundle_identifier,
        version=arguments.version,
        minimum_macos=arguments.minimum_macos,
    )
    copy_resources(arguments.resource, resources)
    additional_seeds = stage_gstreamer_runtime(
        arguments.gstreamer_plugin_dir,
        arguments.gstreamer_plugin,
        arguments.gstreamer_scanner,
        contents,
        arguments.exclude_gstreamer_plugin,
    )
    additional_seeds.extend(stage_gio_modules(arguments.gio_module, contents))
    for library in arguments.extra_library:
        additional_seeds.append(
            copy_macho_seed(
                library,
                frameworks / library.name,
                executable=False,
            )
        )
    if arguments.vulkan_icd:
        stage_vulkan_icd(arguments.vulkan_icd, resources, "libMoltenVK.dylib")
    if arguments.ca_bundle:
        stage_ca_bundle(arguments.ca_bundle, resources)
    stage_presenter_build_mode(arguments.presenter_mode, resources)
    records = copy_dependency_closure(
        binary,
        staged_binary,
        frameworks,
        arguments.search_root,
        additional_seeds,
    )
    rewrite_install_names(records)
    if arguments.sign_identity:
        sign_bundle(
            app,
            arguments.sign_identity,
            (record.staged for record in records if record.staged != staged_binary),
        )
    verify_bundle(
        app,
        required_architecture=arguments.architecture,
        required_libraries=arguments.require_library,
        verify_signature=bool(arguments.sign_identity),
        require_gstreamer_runtime=bool(
            arguments.gstreamer_plugin_dir or arguments.gstreamer_plugin
        ),
        require_vulkan_icd=bool(arguments.vulkan_icd),
        expected_presenter_mode=arguments.presenter_mode,
    )


def parser() -> argparse.ArgumentParser:
    root = argparse.ArgumentParser(description=__doc__)
    commands = root.add_subparsers(dest="command", required=True)

    stage = commands.add_parser("stage", help="create and verify an .app bundle")
    stage.add_argument("--binary", type=Path, required=True)
    stage.add_argument("--app", type=Path, required=True)
    stage.add_argument("--search-root", type=Path, action="append", default=[], required=True)
    stage.add_argument("--resource", type=Path, action="append", default=[])
    stage.add_argument("--extra-library", type=Path, action="append", default=[])
    stage.add_argument(
        "--gstreamer-plugin-dir", type=Path, action="append", default=[]
    )
    stage.add_argument("--gstreamer-plugin", type=Path, action="append", default=[])
    stage.add_argument("--gstreamer-scanner", type=Path)
    stage.add_argument("--exclude-gstreamer-plugin", action="append", default=[])
    stage.add_argument("--gio-module", type=Path, action="append", default=[])
    stage.add_argument("--ca-bundle", type=Path)
    stage.add_argument("--vulkan-icd", type=Path)
    stage.add_argument("--require-library", action="append", default=["libmpv.2.dylib"])
    stage.add_argument("--architecture", choices=("arm64", "x86_64"), required=True)
    stage.add_argument("--executable-name", default="ferrex-player")
    stage.add_argument("--bundle-name", default="Ferrex Player")
    stage.add_argument("--bundle-identifier", default="io.github.lowband21.FerrexPlayer")
    stage.add_argument("--version", required=True)
    stage.add_argument("--minimum-macos", default="15.0")
    stage.add_argument(
        "--presenter-mode", choices=sorted(PRESENTER_BUILD_MODES), required=True
    )
    stage.add_argument("--sign-identity", default="-")
    stage.set_defaults(handler=stage_bundle)

    verify = commands.add_parser("verify", help="audit an existing .app bundle")
    verify.add_argument("--app", type=Path, required=True)
    verify.add_argument("--require-library", action="append", default=["libmpv.2.dylib"])
    verify.add_argument("--architecture", choices=("arm64", "x86_64"))
    verify.add_argument("--require-gstreamer-runtime", action="store_true")
    verify.add_argument("--require-vulkan-icd", action="store_true")
    verify.add_argument("--presenter-mode", choices=sorted(PRESENTER_BUILD_MODES))
    verify.add_argument("--skip-signature", action="store_true")
    verify.set_defaults(
        handler=lambda arguments: verify_bundle(
            arguments.app.resolve(),
            required_architecture=arguments.architecture,
            required_libraries=arguments.require_library,
            verify_signature=not arguments.skip_signature,
            require_gstreamer_runtime=arguments.require_gstreamer_runtime,
            require_vulkan_icd=arguments.require_vulkan_icd,
            expected_presenter_mode=arguments.presenter_mode,
        )
    )
    return root


def main() -> int:
    arguments = parser().parse_args()
    try:
        arguments.handler(arguments)
    except BundleError as error:
        print(f"macOS bundle error: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
