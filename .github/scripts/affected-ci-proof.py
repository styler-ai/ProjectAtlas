#!/usr/bin/env python3
"""Plan the smallest closed ProjectAtlas CI proof for one exact Git change."""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from collections import Counter
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path


MAX_DIFF_BYTES = 1_000_000
MAX_PATHS = 2_048
MAX_PATH_BYTES = 4_096
MAX_REPORTED_CHANGES = 64
MAX_PLAN_BYTES = 900_000
MAX_SUMMARY_BYTES = 64_000
MAX_SOURCE_BYTES = 2_000_000
MAX_SOURCE_BLOB_LOOKUPS = 256
MAX_SOURCE_BLOBS = 128
MAX_TOTAL_SOURCE_BYTES = 32_000_000

PACKAGE_NAMES = (
    "projectatlas-core",
    "projectatlas-db",
    "projectatlas-fs",
    "projectatlas-lints",
    "projectatlas-service",
    "projectatlas-symbols",
    "projectatlas-cli",
)
CLI_TEST_TARGETS = (
    "e2e_delivery",
    "e2e_lifecycle",
    "e2e_maintenance",
    "e2e_navigation",
    "e2e_worktrees",
    "installer_trust_boundaries",
    "language_runtime_compatibility",
    "lint_diagnostics",
    "optional_parser_worker_failure",
    "optional_parser_worker_platform",
    "parser_launch_authority",
    "parser_supervisor_adversarial",
)
PACKAGE_TEST_TARGETS = {
    "projectatlas-db": (
        "e2e_lifecycle",
        "e2e_maintenance",
        "e2e_navigation",
        "e2e_worktrees",
    ),
    "projectatlas-fs": ("e2e_lifecycle", "e2e_worktrees"),
    "projectatlas-lints": ("lint_diagnostics",),
    "projectatlas-service": ("e2e_lifecycle", "e2e_maintenance", "e2e_navigation"),
    "projectatlas-symbols": ("e2e_navigation", "language_runtime_compatibility"),
}
REPOSITORY_CONTRACTS = (
    "issueops",
    "source-policy",
    "mermaid",
    "dependency-audit",
    "cargo-dependency",
    "benchmark-policy",
    "optional-parser-inputs",
)
PLATFORM_CONTRACTS = {
    "linux": ("compile", "worktree", "process", "navigation", "tui", "btrfs", "plugin", "mcp"),
    "windows": ("compile", "worktree", "process", "plugin", "windows", "mcp"),
    "macos-x64": ("compile", "mac-quality", "parser"),
    "macos-arm64": (
        "compile",
        "mac-quality",
        "parser",
        "worktree",
        "process",
        "tui",
        "plugin",
        "mcp",
    ),
}
TARGET_OS_PATTERN = re.compile(rb'\btarget_os\s*=\s*"([^"\r\n]{1,64})"')
TARGET_FAMILY_PATTERN = re.compile(rb'\btarget_family\s*=\s*"([^"\r\n]{1,64})"')
TARGET_ARCH_PATTERN = re.compile(rb'\btarget_arch\s*=\s*"([^"\r\n]{1,64})"')
TARGET_KEY_PATTERN = re.compile(rb"\btarget_[a-z_]+\b")
TARGET_NOT_PATTERN = re.compile(rb"\bnot\s*\(")
TARGET_ALL_PATTERN = re.compile(rb"\ball\s*\(")
CFG_START_PATTERN = re.compile(rb"#\s*!?\s*\[\s*cfg(?:_attr)?\s*\(|\bcfg!\s*\(")
CFG_ATTRIBUTE_PATTERN = re.compile(rb"#\s*!?\s*\[\s*cfg(?:_attr)?\s*\(([^]]{0,4096})]", re.DOTALL)
CFG_MACRO_PATTERN = re.compile(rb"\bcfg!\s*\((.{0,4096}?)\)", re.DOTALL)
CFG_IDENTIFIER_PATTERN = re.compile(rb"\b[A-Za-z_][A-Za-z0-9_]*\b")
SUPPORTED_TARGET_CFG_IDENTIFIERS = {
    b"any",
    b"target_os",
    b"target_family",
    b"target_arch",
    b"linux",
    b"windows",
    b"macos",
    b"unix",
    b"x86_64",
    b"aarch64",
}
PLATFORM_OS = {
    "linux": "ubuntu-latest",
    "windows": "windows-latest",
    "macos-x64": "macos-15-intel",
    "macos-arm64": "macos-14",
}
OS_PLATFORM_LABELS = ("linux", "windows", "macos-arm64")
UNIX_PLATFORM_LABELS = ("linux", "macos-arm64")
SOURCE_UNIX_PLATFORM_LABELS = ("linux", "macos-x64", "macos-arm64")
MAC_PLATFORM_LABELS = ("macos-x64", "macos-arm64")
CLI_E2E_INVENTORY_PATH = "docs/v050-cli-e2e-inventory.json"
AUTHORITY_PATHS = (
    ".cargo/",
    ".github/workflows/",
    ".github/scripts/affected-ci-proof.py",
    ".githooks/pre-push",
    "Cargo.lock",
    "Cargo.toml",
    "deny.toml",
    "rust-toolchain.toml",
    "openspec/issue-map.json",
    "crates/projectatlas-db/src/schema.rs",
)
INSTALLER_SCRIPT_PATHS = {
    "plugins/projectatlas/scripts/install-runtime.ps1",
    "plugins/projectatlas/scripts/install-runtime.sh",
}
OBJECT_ID_LENGTHS = {"sha1": 40, "sha256": 64}
KNOWN_REPOSITORY_FILES = {
    "AGENTS.md",
    "CHANGELOG.md",
    "CONTRIBUTING.md",
    "LICENSE",
    "README.md",
}


@dataclass(frozen=True)
class Change:
    status: str
    paths: tuple[str, ...]


@dataclass(frozen=True)
class CargoGraph:
    roots: dict[str, str]
    reverse: dict[str, set[str]]

    def owner(self, path: str) -> str | None:
        matches = [
            (root, package)
            for package, root in self.roots.items()
            if path == root or path.startswith(f"{root}/")
        ]
        return max(matches, default=("", None), key=lambda item: len(item[0]))[1]

    def closure(self, packages: set[str]) -> set[str]:
        selected = set(packages)
        pending = list(packages)
        while pending:
            package = pending.pop()
            for dependent in self.reverse.get(package, set()):
                if dependent not in selected:
                    selected.add(dependent)
                    pending.append(dependent)
        return selected


def normalize_path(raw: str) -> str:
    return raw.replace("\\", "/").removeprefix("./")


def parse_name_status(data: bytes) -> list[Change]:
    if len(data) > MAX_DIFF_BYTES:
        raise ValueError("diff exceeds byte bound")
    fields = data.split(b"\0")
    if fields and fields[-1] == b"":
        fields.pop()
    changes: list[Change] = []
    index = 0
    while index < len(fields):
        status = fields[index].decode("ascii", "strict")
        index += 1
        if status[:1] in {"R", "C"}:
            if not status[1:].isdigit():
                raise ValueError("malformed NUL name-status diff")
        elif status not in {"A", "D", "M", "T", "U", "X", "B"}:
            raise ValueError("malformed NUL name-status diff")
        path_count = 2 if status[:1] in {"R", "C"} else 1
        if not status or index + path_count > len(fields):
            raise ValueError("malformed NUL name-status diff")
        paths = []
        for raw_path in fields[index : index + path_count]:
            if len(raw_path) > MAX_PATH_BYTES:
                raise ValueError("diff path exceeds byte bound")
            paths.append(normalize_path(os.fsdecode(raw_path)))
        changes.append(Change(status, tuple(paths)))
        index += path_count
        if len(changes) > MAX_PATHS:
            raise ValueError("diff exceeds path-count bound")
    if not changes:
        raise ValueError("diff is empty")
    return changes


def parse_planning_changes(
    data: bytes, *, base: str, head: str, force_full: bool
) -> list[Change]:
    if force_full and base == head and not data:
        return []
    return parse_name_status(data)


def git(*args: str, root: Path, check: bool = True) -> bytes:
    environment = dict(os.environ)
    environment["GIT_NO_REPLACE_OBJECTS"] = "1"
    result = subprocess.run(
        ["git", *args],
        cwd=root,
        env=environment,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if check and result.returncode:
        raise RuntimeError(result.stderr.decode("utf-8", "replace").strip())
    return result.stdout


def object_id_length(object_format: str) -> int:
    try:
        return OBJECT_ID_LENGTHS[object_format]
    except KeyError as error:
        raise RuntimeError(f"unsupported Git object format {object_format!r}") from error


def repository_object_id_length(root: Path) -> int:
    object_format = git("rev-parse", "--show-object-format=storage", root=root).decode(
        "ascii", "strict"
    ).strip()
    return object_id_length(object_format)


def validate_object_id(value: str, expected_length: int) -> None:
    if len(value) != expected_length or any(
        character not in "0123456789abcdefABCDEF" for character in value
    ):
        raise RuntimeError(f"unreadable commit {value!r}")


def exact_commit(value: str, root: Path, expected_length: int) -> str:
    validate_object_id(value, expected_length)
    resolved = git("rev-parse", "--verify", f"{value}^{{commit}}", root=root)
    commit = resolved.decode("ascii", "strict").strip()
    if len(commit) != expected_length or commit.lower() != value.lower():
        raise RuntimeError(f"unreadable commit {value!r}")
    return commit


def exact_tree_blob(root: Path, revision: str, path: str) -> bytes | None:
    tree = git(
        "ls-tree",
        "-z",
        "--full-tree",
        revision,
        "--",
        f":(literal){path}",
        root=root,
    )
    if not tree:
        return None
    records = [record for record in tree.split(b"\0") if record]
    if len(records) != 1 or b"\t" not in records[0]:
        raise RuntimeError(f"ambiguous submitted-tree source path: {path}")
    header, _ = records[0].split(b"\t", 1)
    fields = header.split()
    if len(fields) != 3 or fields[0] not in {b"100644", b"100755"} or fields[1] != b"blob":
        raise RuntimeError(f"unsupported submitted-tree source mode: {path}")
    oid = fields[2].decode("ascii", "strict")
    source = git("cat-file", "blob", oid, root=root)
    if len(source) > MAX_SOURCE_BYTES:
        raise RuntimeError(f"submitted-tree source exceeds byte bound: {path}")
    return source


def source_platform_labels(source: bytes) -> tuple[str, ...]:
    if len(source) > MAX_SOURCE_BYTES:
        raise RuntimeError("submitted-tree source exceeds byte bound")
    cfg_matches = list(CFG_ATTRIBUTE_PATTERN.finditer(source)) + list(
        CFG_MACRO_PATTERN.finditer(source)
    )
    if {match.start() for match in CFG_START_PATTERN.finditer(source)} != {
        match.start() for match in cfg_matches
    }:
        raise RuntimeError("unbounded or malformed cfg expression requires complete proof")
    cfg_bodies = [match.group(1) for match in cfg_matches]
    operating_systems: set[bytes] = set()
    families: set[bytes] = set()
    architectures: set[bytes] = set()
    for body in cfg_bodies:
        os_values = set(TARGET_OS_PATTERN.findall(body))
        family_values = set(TARGET_FAMILY_PATTERN.findall(body))
        arch_values = set(TARGET_ARCH_PATTERN.findall(body))
        shorthand_windows = re.search(rb"\bwindows\b", body) is not None
        shorthand_unix = re.search(rb"\bunix\b", body) is not None
        target_bearing = bool(
            TARGET_KEY_PATTERN.search(body) or shorthand_windows or shorthand_unix
        )
        if not target_bearing:
            continue
        if TARGET_NOT_PATTERN.search(body):
            raise RuntimeError("negated target predicate requires complete proof")
        if TARGET_ALL_PATTERN.search(body):
            raise RuntimeError("conjunctive target predicate requires complete proof")
        if set(TARGET_KEY_PATTERN.findall(body)) - {
            b"target_os",
            b"target_family",
            b"target_arch",
        }:
            raise RuntimeError("unsupported target predicate requires complete proof")
        if os_values - {b"linux", b"windows", b"macos"}:
            raise RuntimeError("unsupported target operating system requires complete proof")
        if family_values - {b"unix", b"windows"}:
            raise RuntimeError("unsupported target family requires complete proof")
        if arch_values - {b"x86_64", b"aarch64"}:
            raise RuntimeError("unsupported target architecture requires complete proof")
        if (
            set(CFG_IDENTIFIER_PATTERN.findall(body))
            - SUPPORTED_TARGET_CFG_IDENTIFIERS
        ):
            raise RuntimeError("mixed target and non-target predicate requires complete proof")
        operating_systems.update(os_values)
        families.update(family_values)
        architectures.update(arch_values)
        if shorthand_windows:
            operating_systems.add(b"windows")
        if shorthand_unix:
            families.add(b"unix")

    labels: set[str] = set()
    if b"linux" in operating_systems or b"unix" in families:
        labels.add("linux")
    if b"windows" in operating_systems or b"windows" in families:
        labels.add("windows")
    if b"macos" in operating_systems:
        labels.update(MAC_PLATFORM_LABELS)
    if b"unix" in families:
        labels.update(SOURCE_UNIX_PLATFORM_LABELS)

    if b"x86_64" in architectures:
        labels.update(("linux", "windows", "macos-x64"))
    if b"aarch64" in architectures:
        labels.add("macos-arm64")
    return tuple(label for label in PLATFORM_CONTRACTS if label in labels)


def source_target_predicates(source: bytes) -> Counter[bytes]:
    matches = list(CFG_ATTRIBUTE_PATTERN.finditer(source)) + list(
        CFG_MACRO_PATTERN.finditer(source)
    )
    return Counter(
        body
        for match in matches
        if TARGET_KEY_PATTERN.search(body := match.group(1))
        or re.search(rb"\b(?:windows|unix)\b", body)
    )


def changed_source_platforms(
    root: Path,
    base: str,
    head: str,
    changes: list[Change],
    blob_loader: Callable[[Path, str, str], bytes | None] = exact_tree_blob,
) -> dict[str, tuple[str, ...]]:
    result: dict[str, tuple[str, ...]] = {}
    added_sources: list[tuple[str, Counter[bytes]]] = []
    deleted_sources: list[tuple[str, Counter[bytes]]] = []
    lookups = 0
    blobs = 0
    total_bytes = 0
    for change in changes:
        change_predicates: dict[tuple[str, str], Counter[bytes]] = {}
        for path in change.paths:
            if not path.endswith(".rs"):
                continue
            labels: set[str] = set()
            revision_predicates: dict[str, Counter[bytes]] = {}
            found = False
            for revision in (base, head):
                lookups += 1
                if lookups > MAX_SOURCE_BLOB_LOOKUPS:
                    raise RuntimeError("submitted-tree source lookup bound exceeded")
                source = blob_loader(root, revision, path)
                if source is None:
                    continue
                found = True
                blobs += 1
                total_bytes += len(source)
                if blobs > MAX_SOURCE_BLOBS or total_bytes > MAX_TOTAL_SOURCE_BYTES:
                    raise RuntimeError("submitted-tree source aggregate bound exceeded")
                current_labels = set(source_platform_labels(source))
                predicates = source_target_predicates(source)
                revision_predicates[revision] = predicates
                change_predicates[(revision, path)] = predicates
                labels.update(current_labels)
            if not found:
                raise RuntimeError(f"submitted-tree source is unavailable: {path}")
            if base in revision_predicates and head in revision_predicates and not (
                revision_predicates[base] <= revision_predicates[head]
            ):
                labels.update(PLATFORM_CONTRACTS)
            result[path] = tuple(label for label in PLATFORM_CONTRACTS if label in labels)
            if change.status == "A" and head in revision_predicates:
                added_sources.append((path, revision_predicates[head]))
            elif change.status == "D" and base in revision_predicates:
                deleted_sources.append((path, revision_predicates[base]))
        if change.status.startswith("R") and len(change.paths) == 2:
            old_path, new_path = change.paths
            old_predicates = change_predicates.get((base, old_path))
            new_predicates = change_predicates.get((head, new_path))
            selected = set(result.get(old_path, ())) | set(result.get(new_path, ()))
            if (
                old_predicates is not None
                and new_predicates is not None
                and not old_predicates <= new_predicates
                and selected != set(PLATFORM_CONTRACTS)
            ):
                result[new_path] = tuple(PLATFORM_CONTRACTS)
    if added_sources and any(predicates for _, predicates in deleted_sources):
        for path, _ in added_sources + deleted_sources:
            result[path] = tuple(PLATFORM_CONTRACTS)
    return result


def validate_package_inventory(packages: list[dict[str, object]]) -> None:
    if {str(package["name"]) for package in packages} != set(PACKAGE_NAMES):
        raise RuntimeError("Cargo workspace package inventory drifted")


def load_cargo_graph(root: Path) -> CargoGraph:
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--locked"],
        cwd=root,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(result.stderr.decode("utf-8", "replace").strip())
    metadata = json.loads(result.stdout)
    workspace_ids = set(metadata["workspace_members"])
    packages = [package for package in metadata["packages"] if package["id"] in workspace_ids]
    validate_package_inventory(packages)
    root_path = root.resolve()
    roots: dict[str, str] = {}
    manifest_owners: dict[str, str] = {}
    for package in packages:
        package_root = Path(package["manifest_path"]).resolve().parent
        relative = package_root.relative_to(root_path).as_posix()
        roots[package["name"]] = relative
        manifest_owners[str(package_root)] = package["name"]
    reverse = {name: set() for name in roots}
    for package in packages:
        for dependency in package["dependencies"]:
            dependency_path = dependency.get("path")
            if not dependency_path:
                continue
            owner = manifest_owners.get(str(Path(dependency_path).resolve()))
            if owner:
                reverse[owner].add(package["name"])
    return CargoGraph(roots, reverse)


def add_platform(platforms: dict[str, set[str]], labels: tuple[str, ...], *contracts: str) -> None:
    for label in labels:
        platforms[label].update(contracts)


def platform_owners(
    path: str,
    platforms: dict[str, set[str]],
    source_labels: tuple[str, ...] = (),
) -> None:
    add_platform(platforms, source_labels, "compile")
    if path.startswith("crates/projectatlas-fs/") or any(
        token in path for token in ("project_root", "project_identity", "worktree_registry")
    ):
        add_platform(platforms, OS_PLATFORM_LABELS, "worktree")
        add_platform(platforms, ("linux",), "btrfs")
    if path.startswith("crates/projectatlas-symbols/") or any(
        token in path for token in ("graph", "structural", "atlas_map", "relation")
    ):
        add_platform(platforms, ("linux",), "navigation")
    if "token_tui" in path:
        add_platform(platforms, UNIX_PLATFORM_LABELS, "tui")
    if any(token in path for token in ("optional_parser", "parser_worker")):
        add_platform(platforms, MAC_PLATFORM_LABELS, "parser", "mac-quality")
        add_platform(platforms, OS_PLATFORM_LABELS, "process")
    if "parser_supervisor" in path or "source_observation" in path:
        add_platform(platforms, OS_PLATFORM_LABELS, "process")
    if "/mcp" in path or path.endswith("/mcp.rs"):
        add_platform(platforms, OS_PLATFORM_LABELS, "mcp")
    if path.startswith("plugins/") or path.startswith("install") or "plugin" in path:
        add_platform(platforms, OS_PLATFORM_LABELS, "plugin")
    if path.endswith(".ps1"):
        add_platform(platforms, ("windows",), "windows")


def test_platform_owners(target: str, platforms: dict[str, set[str]]) -> None:
    if target == "e2e_worktrees":
        add_platform(platforms, OS_PLATFORM_LABELS, "worktree")
    elif target == "e2e_delivery":
        add_platform(platforms, OS_PLATFORM_LABELS, "process", "plugin")
        add_platform(platforms, ("windows",), "windows")
    elif target == "e2e_lifecycle":
        add_platform(platforms, OS_PLATFORM_LABELS, "mcp")
        add_platform(platforms, ("linux",), "btrfs")
        add_platform(platforms, MAC_PLATFORM_LABELS, "parser")
    elif target == "e2e_navigation":
        add_platform(platforms, ("linux",), "navigation")
    elif target == "e2e_maintenance":
        add_platform(platforms, ("linux",), "navigation")
        add_platform(platforms, UNIX_PLATFORM_LABELS, "tui")
    elif target == "installer_trust_boundaries":
        add_platform(platforms, ("windows",), "windows")
    elif target == "optional_parser_worker_platform":
        add_platform(platforms, MAC_PLATFORM_LABELS, "parser", "mac-quality")
    elif target == "parser_supervisor_adversarial":
        add_platform(platforms, OS_PLATFORM_LABELS, "process")


def production_test_targets(owner: str, path: str) -> tuple[str, ...] | None:
    if owner != "projectatlas-cli":
        return PACKAGE_TEST_TARGETS.get(owner, ())
    if any(token in path for token in ("plugin", "installer")):
        return ("e2e_delivery", "installer_trust_boundaries")
    if "token_tui" in path:
        return ("e2e_maintenance",)
    if any(token in path for token in ("optional_parser", "parser_worker")):
        return (
            "e2e_lifecycle",
            "optional_parser_worker_failure",
            "optional_parser_worker_platform",
        )
    if "parser_supervisor" in path or "source_observation" in path:
        return ("e2e_delivery", "e2e_lifecycle", "parser_supervisor_adversarial")
    if "worktree" in path or "project_root" in path or "project_identity" in path:
        return ("e2e_lifecycle", "e2e_worktrees")
    if any(token in path for token in ("graph", "structural", "relation", "module_resolution")):
        return ("e2e_navigation", "language_runtime_compatibility")
    if any(token in path for token in ("purpose", "lint", "telemetry")):
        return ("e2e_maintenance",)
    if "/mcp" in path or path.endswith("/mcp.rs"):
        return (
            "e2e_delivery",
            "e2e_lifecycle",
            "e2e_maintenance",
            "e2e_navigation",
            "e2e_worktrees",
        )
    return None


def is_authority(path: str) -> bool:
    return any(path == item or path.startswith(item) for item in AUTHORITY_PATHS) or (
        path.endswith("Cargo.toml")
    )


def is_cargo_dependency(path: str) -> bool:
    return path in {"Cargo.lock", "deny.toml"} or path.endswith("Cargo.toml")


def is_known_repository_path(path: str) -> bool:
    return (
        path in KNOWN_REPOSITORY_FILES
        or path.startswith("docs/")
        or path.startswith("openspec/")
        or path.startswith(".github/ISSUE_TEMPLATE/")
        or path == ".github/pull_request_template.md"
    )


def full_plan(
    *,
    base: str,
    head: str,
    event: str,
    changes: list[Change],
    reason: str,
    dependency_audit: bool,
    cargo_dependency: bool,
) -> dict[str, object]:
    repository = set(REPOSITORY_CONTRACTS) - {"dependency-audit", "cargo-dependency"}
    if dependency_audit or event in {"schedule", "workflow_dispatch"}:
        repository.add("dependency-audit")
    if cargo_dependency or event in {"schedule", "workflow_dispatch"}:
        repository.add("cargo-dependency")
    return build_plan(
        base=base,
        head=head,
        event=event,
        changes=changes,
        mode="full",
        reasons=[reason],
        repository=repository,
        packages=set(PACKAGE_NAMES),
        test_targets=set(CLI_TEST_TARGETS),
        test_only=False,
        platforms={label: set(contracts) for label, contracts in PLATFORM_CONTRACTS.items()},
    )


def build_plan(
    *,
    base: str,
    head: str,
    event: str,
    changes: list[Change],
    mode: str,
    reasons: list[str],
    repository: set[str],
    packages: set[str],
    test_targets: set[str],
    test_only: bool,
    platforms: dict[str, set[str]],
) -> dict[str, object]:
    for label, contracts in platforms.items():
        undeclared = contracts - set(PLATFORM_CONTRACTS[label])
        if undeclared:
            raise RuntimeError(f"undeclared {label} contracts: {sorted(undeclared)}")
    matrix = {
        "include": [
            {
                "label": label,
                "os": PLATFORM_OS[label],
                "contracts": sorted(contracts),
            }
            for label, contracts in platforms.items()
            if contracts
        ]
    }
    selected = set(repository)
    selected.update(f"rust:{package}" for package in packages)
    selected.update(f"test:{target}" for target in test_targets)
    selected.update(
        f"platform:{label}:{contract}"
        for label, contracts in platforms.items()
        for contract in contracts
    )
    all_contracts = set(REPOSITORY_CONTRACTS)
    all_contracts.update(f"rust:{package}" for package in PACKAGE_NAMES)
    all_contracts.update(f"test:{target}" for target in CLI_TEST_TARGETS)
    all_contracts.update(
        f"platform:{label}:{contract}"
        for label, contracts in PLATFORM_CONTRACTS.items()
        for contract in contracts
    )
    return {
        "schema": 1,
        "binding": {"base": base, "head": head, "event": event},
        "mode": mode,
        "reasons": reasons[:64],
        "changes": [
            {"status": change.status, "paths": list(change.paths)}
            for change in changes[:MAX_REPORTED_CHANGES]
        ],
        "repository_contracts": sorted(repository),
        "rust_packages": sorted(packages),
        "test_targets": sorted(test_targets),
        "test_only": test_only,
        "platform_matrix": matrix,
        "jobs": {
            "repository": bool(repository),
            "rust": bool(
                packages
                or test_targets
                or "source-policy" in repository
                or "cargo-dependency" in repository
            ),
            "platform": bool(matrix["include"]),
        },
        "selected": sorted(selected),
        "omitted": sorted(all_contracts - selected),
    }


def plan_changes(
    *,
    base: str,
    head: str,
    event: str,
    changes: list[Change],
    graph: CargoGraph,
    force_full: bool = False,
    source_platforms: dict[str, tuple[str, ...]] | None = None,
) -> dict[str, object]:
    source_platforms = source_platforms or {}
    paths = [path for change in changes for path in change.paths]
    dependency_audit = force_full and not changes or any(
        path in {".github/mermaid-parser/package.json", ".github/mermaid-parser/package-lock.json"}
        for path in paths
    )
    cargo_dependency = force_full and not changes or any(
        is_cargo_dependency(path) for path in paths
    )
    if force_full or event in {"schedule", "workflow_dispatch", "merge_group"}:
        return full_plan(
            base=base,
            head=head,
            event=event,
            changes=changes,
            reason=f"{event} requires complete drift proof",
            dependency_audit=dependency_audit,
            cargo_dependency=cargo_dependency,
        )
    structural = next(
        (change for change in changes if change.status in {"T", "U", "X", "B"}),
        None,
    )
    if structural:
        return full_plan(
            base=base,
            head=head,
            event=event,
            changes=changes,
            reason=f"ambiguous Git status requires complete proof: {structural.status}",
            dependency_audit=dependency_audit,
            cargo_dependency=cargo_dependency,
        )
    authority = next((path for path in paths if is_authority(path)), None)
    if authority:
        return full_plan(
            base=base,
            head=head,
            event=event,
            changes=changes,
            reason=f"shared proof authority changed: {authority}",
            dependency_audit=dependency_audit,
            cargo_dependency=cargo_dependency,
        )

    repository = {"issueops", "mermaid", "source-policy"}
    packages: set[str] = set()
    production_packages: set[str] = set()
    test_targets: set[str] = set()
    platforms = {label: set() for label in PLATFORM_CONTRACTS}
    reasons: list[str] = []
    unknown: str | None = None
    for change in changes:
        for path in change.paths:
            if path == CLI_E2E_INVENTORY_PATH:
                packages.add("projectatlas-cli")
                test_targets.add("e2e_delivery")
                reasons.append("CLI E2E inventory requires its executable validator")
                continue
            if is_known_repository_path(path):
                reasons.append(f"repository documentation/policy owns {path}")
                if path.startswith("docs/benchmarks/"):
                    repository.add("benchmark-policy")
                continue
            if path.startswith(".github/mermaid-parser/"):
                repository.add("mermaid")
                if dependency_audit:
                    repository.add("dependency-audit")
                reasons.append(f"Mermaid parser owns {path}")
                continue
            owner = graph.owner(path)
            if owner and path.endswith(".rs"):
                relative = path.removeprefix(f"{graph.roots[owner]}/")
                if owner == "projectatlas-core" and relative.startswith("src/"):
                    return full_plan(
                        base=base,
                        head=head,
                        event=event,
                        changes=changes,
                        reason=f"shared core changed: {path}",
                        dependency_audit=dependency_audit,
                        cargo_dependency=cargo_dependency,
                    )
                if relative.startswith("tests/"):
                    parts = relative.split("/")
                    if len(parts) == 2 and parts[1].endswith(".rs"):
                        target = parts[1][:-3]
                        if target not in CLI_TEST_TARGETS or owner != "projectatlas-cli":
                            unknown = path
                            break
                        test_targets.add(target)
                        packages.add(owner)
                        reasons.append(f"owning CLI test target {target} changed")
                        test_platform_owners(target, platforms)
                        add_platform(
                            platforms, source_platforms.get(path, ()), "compile"
                        )
                    elif relative.startswith("tests/support/"):
                        return full_plan(
                            base=base,
                            head=head,
                            event=event,
                            changes=changes,
                            reason=f"shared CLI test support changed: {path}",
                            dependency_audit=dependency_audit,
                            cargo_dependency=cargo_dependency,
                        )
                    else:
                        unknown = path
                        break
                elif relative.startswith("src/") or relative.startswith("examples/"):
                    owning_tests = production_test_targets(owner, path)
                    if owning_tests is None:
                        return full_plan(
                            base=base,
                            head=head,
                            event=event,
                            changes=changes,
                            reason=f"shared CLI source changed: {path}",
                            dependency_audit=dependency_audit,
                            cargo_dependency=cargo_dependency,
                        )
                    packages.add(owner)
                    production_packages.add(owner)
                    test_targets.update(owning_tests)
                    for target in owning_tests:
                        test_platform_owners(target, platforms)
                    platform_owners(path, platforms, source_platforms.get(path, ()))
                    reasons.append(f"Cargo package {owner} owns {path}")
                else:
                    unknown = path
                    break
                continue
            if path.startswith("plugins/"):
                owning_tests = ["e2e_delivery"]
                if path in INSTALLER_SCRIPT_PATHS:
                    owning_tests.append("installer_trust_boundaries")
                test_targets.update(owning_tests)
                for target in owning_tests:
                    test_platform_owners(target, platforms)
                platform_owners(path, platforms)
                reasons.append(f"installer/plugin contract owns {path}")
                continue
            unknown = path
            break
        if unknown:
            break
    if unknown:
        return full_plan(
            base=base,
            head=head,
            event=event,
            changes=changes,
            reason=f"unknown or incompletely owned path: {unknown}",
            dependency_audit=dependency_audit,
            cargo_dependency=cargo_dependency,
        )
    packages = graph.closure(packages)
    if packages:
        reasons.append("Cargo reverse dependencies were included")
    return build_plan(
        base=base,
        head=head,
        event=event,
        changes=changes,
        mode="narrow",
        reasons=reasons,
        repository=repository,
        packages=packages,
        test_targets=test_targets,
        test_only=bool(test_targets) and not production_packages,
        platforms=platforms,
    )


def plan_with_cargo_graph(
    *,
    root: Path,
    base: str,
    head: str,
    event: str,
    changes: list[Change],
    force_full: bool = False,
    graph_loader: Callable[[Path], CargoGraph] = load_cargo_graph,
) -> dict[str, object]:
    paths = [path for change in changes for path in change.paths]
    requires_graph = (
        not force_full
        and event not in {"schedule", "workflow_dispatch", "merge_group"}
        and any(path.startswith("crates/") for path in paths)
        and not any(is_authority(path) for path in paths)
    )
    if requires_graph:
        try:
            graph = graph_loader(root)
        except (AttributeError, KeyError, OSError, RuntimeError, TypeError, ValueError):
            return full_plan(
                base=base,
                head=head,
                event=event,
                changes=changes,
                reason="Cargo metadata unavailable or invalid",
                dependency_audit=any(
                    path
                    in {
                        ".github/mermaid-parser/package.json",
                        ".github/mermaid-parser/package-lock.json",
                    }
                    for path in paths
                ),
                cargo_dependency=any(is_cargo_dependency(path) for path in paths),
            )
        try:
            source_platforms = changed_source_platforms(root, base, head, changes)
        except (OSError, RuntimeError, UnicodeError, ValueError):
            return full_plan(
                base=base,
                head=head,
                event=event,
                changes=changes,
                reason="target-specific source ownership unavailable or invalid",
                dependency_audit=any(
                    path
                    in {
                        ".github/mermaid-parser/package.json",
                        ".github/mermaid-parser/package-lock.json",
                    }
                    for path in paths
                ),
                cargo_dependency=any(is_cargo_dependency(path) for path in paths),
            )
    else:
        graph = CargoGraph(
            {name: f"crates/{name}" for name in PACKAGE_NAMES},
            {name: set() for name in PACKAGE_NAMES},
        )
        source_platforms = {}
    return plan_changes(
        base=base,
        head=head,
        event=event,
        changes=changes,
        graph=graph,
        force_full=force_full,
        source_platforms=source_platforms,
    )


def decision_reason(plan: dict[str, object], contract: str, selected: bool) -> str:
    if selected and plan["mode"] == "full":
        return f"complete fallback: {plan['reasons'][0]}"
    if contract.startswith("rust:"):
        return (
            "owning Cargo package or reverse dependency selected"
            if selected
            else "outside the owning package and reverse-dependency closure"
        )
    if contract.startswith("test:"):
        return (
            "changed test or production owner maps to this test domain"
            if selected
            else "no changed test or production owner maps to this test domain"
        )
    if contract.startswith("platform:"):
        return (
            "affected path or test domain declares this platform owner"
            if selected
            else "no affected path or test domain declares this platform owner"
        )
    repository_rules = {
        "issueops": "ordinary change requires mapped issue-state consistency",
        "mermaid": "IssueOps architecture links require the locked Mermaid parser",
        "source-policy": "every affected tracked change owns repository-wide source policy",
        "dependency-audit": "Mermaid dependency manifest changed or drift proof was requested",
        "cargo-dependency": "Cargo dependency manifest changed or drift proof was requested",
        "benchmark-policy": "benchmark documentation or complete fallback owns benchmark policy",
        "optional-parser-inputs": "complete fallback owns optional-parser proof inputs",
    }
    if selected:
        return repository_rules[contract]
    if contract == "dependency-audit":
        return "Mermaid dependency manifests are unchanged and this is not drift proof"
    if contract == "cargo-dependency":
        return "Cargo dependency manifests are unchanged and this is not drift proof"
    return "no affected path declares this repository owner"


def summary(plan: dict[str, object]) -> str:
    lines = [
        "## Affected CI proof",
        "",
        f"Mode: **{plan['mode']}**",
        "",
        "Reasons:",
        *[f"- {reason}" for reason in plan["reasons"]],
        "",
        "Selected contracts:",
        *[
            f"- `{contract}` — {decision_reason(plan, contract, True)}"
            for contract in plan["selected"]
        ],
        "",
        "Omitted contracts:",
        *[
            f"- `{contract}` — {decision_reason(plan, contract, False)}"
            for contract in plan["omitted"]
        ],
    ]
    rendered = "\n".join(lines) + "\n"
    if len(rendered.encode("utf-8")) > MAX_SUMMARY_BYTES:
        raise RuntimeError("plan summary exceeds byte bound")
    return rendered


def write_outputs(plan: dict[str, object], output: Path | None, summary_path: Path | None) -> None:
    compact = json.dumps(plan, separators=(",", ":"), sort_keys=True)
    if len(compact.encode("utf-8")) > MAX_PLAN_BYTES:
        raise RuntimeError("proof plan exceeds byte bound")
    if output:
        jobs = plan["jobs"]
        with output.open("a", encoding="utf-8", newline="\n") as stream:
            stream.write(f"plan={compact}\n")
            stream.write(f"repository={str(jobs['repository']).lower()}\n")
            stream.write(f"rust={str(jobs['rust']).lower()}\n")
            stream.write(f"platform={str(jobs['platform']).lower()}\n")
            stream.write(
                "platform_matrix="
                + json.dumps(plan["platform_matrix"], separators=(",", ":"))
                + "\n"
            )
    if summary_path:
        with summary_path.open("a", encoding="utf-8", newline="\n") as stream:
            stream.write(summary(plan))


def aggregate(
    plan: dict[str, object],
    base: str,
    head: str,
    event: str,
    results: dict[str, str],
) -> None:
    binding = plan.get("binding", {})
    if (
        binding.get("base") != base
        or binding.get("head") != head
        or binding.get("event") != event
    ):
        raise RuntimeError("proof plan binding is stale")
    jobs = plan.get("jobs", {})
    for name in ("repository", "rust", "platform"):
        selected = jobs.get(name) is True
        result = results[name]
        if selected and result != "success":
            raise RuntimeError(f"selected {name} job concluded {result}")
        if not selected and result != "skipped":
            raise RuntimeError(f"omitted {name} job concluded {result}")


def fake_graph() -> CargoGraph:
    return CargoGraph(
        roots={name: f"crates/{name}" for name in PACKAGE_NAMES},
        reverse={
            "projectatlas-core": {
                "projectatlas-db",
                "projectatlas-fs",
                "projectatlas-service",
                "projectatlas-symbols",
                "projectatlas-cli",
            },
            "projectatlas-db": {"projectatlas-service", "projectatlas-cli"},
            "projectatlas-symbols": {"projectatlas-service", "projectatlas-cli"},
            "projectatlas-service": {"projectatlas-cli"},
            "projectatlas-fs": {"projectatlas-cli"},
            "projectatlas-lints": set(),
            "projectatlas-cli": set(),
        },
    )


def self_test() -> None:
    changes = parse_name_status(b"M\0docs/readme.md\0R100\0old.md\0new.md\0")
    assert changes == [Change("M", ("docs/readme.md",)), Change("R100", ("old.md", "new.md"))]
    for invalid in (
        b"",
        b"M\0",
        b"Z\0a\0",
        b"R\0a\0b\0",
        b"M\0" + b"a" * (MAX_PATH_BYTES + 1) + b"\0",
        b"M\0a\0" * (MAX_PATHS + 1),
    ):
        try:
            parse_name_status(invalid)
        except ValueError:
            pass
        else:
            raise AssertionError("invalid diff was accepted")
    graph = fake_graph()
    try:
        validate_package_inventory([{"name": "unexpected-package"}])
    except RuntimeError:
        pass
    else:
        raise AssertionError("Cargo metadata inventory drift was accepted")
    assert graph.closure({"projectatlas-core"}) == {
        "projectatlas-core",
        "projectatlas-db",
        "projectatlas-fs",
        "projectatlas-service",
        "projectatlas-symbols",
        "projectatlas-cli",
    }
    plan = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", ("docs/readme.md",))],
        graph=graph,
    )
    assert plan["jobs"] == {"repository": True, "rust": True, "platform": False}
    assert plan["repository_contracts"] == ["issueops", "mermaid", "source-policy"]
    assert "actor" not in plan["binding"]
    report = summary(plan)
    assert "ordinary change requires mapped issue-state consistency" in report
    assert "outside the owning package and reverse-dependency closure" in report

    leaf = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", ("crates/projectatlas-lints/src/lib.rs",))],
        graph=graph,
    )
    assert leaf["rust_packages"] == ["projectatlas-lints"]
    assert leaf["test_targets"] == ["lint_diagnostics"]
    assert leaf["platform_matrix"] == {"include": []}

    database = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", ("crates/projectatlas-db/src/queries.rs",))],
        graph=graph,
    )
    assert database["test_targets"] == list(PACKAGE_TEST_TARGETS["projectatlas-db"])
    assert {
        row["label"]: row["contracts"] for row in database["platform_matrix"]["include"]
    } == {
        "linux": ["btrfs", "mcp", "navigation", "tui", "worktree"],
        "windows": ["mcp", "worktree"],
        "macos-x64": ["parser"],
        "macos-arm64": ["mcp", "parser", "tui", "worktree"],
    }

    platform_source = "crates/projectatlas-service/src/agent_efficiency.rs"

    def platform_blob(_: Path, revision: str, path: str) -> bytes | None:
        assert path == platform_source
        return b"#[cfg(windows)]\n" if revision == "a" * 40 else b"#[cfg(unix)]\n"

    source_platforms = changed_source_platforms(
        Path.cwd(),
        "a" * 40,
        "b" * 40,
        [Change("M", (platform_source,))],
        blob_loader=platform_blob,
    )
    assert source_platforms[platform_source] == (
        "linux",
        "windows",
        "macos-x64",
        "macos-arm64",
    )

    def relaxed_platform_blob(_: Path, revision: str, path: str) -> bytes | None:
        assert path == platform_source
        retained = b"#[cfg(windows)]\nfn retained() {}\n"
        changed = b"#[cfg(windows)]\nfn changed() {}\n"
        return retained + changed if revision == "a" * 40 else retained + b"fn changed() {}\n"

    assert changed_source_platforms(
        Path.cwd(),
        "a" * 40,
        "b" * 40,
        [Change("M", (platform_source,))],
        blob_loader=relaxed_platform_blob,
    )[platform_source] == tuple(PLATFORM_CONTRACTS)
    target_specific = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", (platform_source,))],
        graph=graph,
        source_platforms=source_platforms,
    )
    assert target_specific["mode"] == "narrow"
    assert target_specific["rust_packages"] == ["projectatlas-cli", "projectatlas-service"]
    assert {
        row["label"]: row["contracts"]
        for row in target_specific["platform_matrix"]["include"]
    } == {
        "linux": ["btrfs", "compile", "mcp", "navigation", "tui"],
        "windows": ["compile", "mcp"],
        "macos-x64": ["compile", "parser"],
        "macos-arm64": ["compile", "mcp", "parser", "tui"],
    }
    assert source_platform_labels(b'let note = "target_os = \\"windows\\"";') == ()
    assert source_platform_labels(b"#[cfg(unix)]") == SOURCE_UNIX_PLATFORM_LABELS
    assert source_platform_labels(b'cfg!(\n  target_os = "windows"\n)') == ("windows",)
    for ambiguous_source in (
        b"#[cfg(not(windows))]",
        b"#[cfg(not(unix))]",
        b'#[cfg(not(target_arch = "x86_64"))]',
        b'#[cfg(not(any(target_os = "windows", target_os = "linux")))]',
        b'#[cfg(not(all(unix, target_arch = "aarch64")))]',
        b'#[cfg(all(target_os = "linux", target_arch = "x86_64"))]',
        b'#[cfg(all(target_os = "windows", target_arch = "x86_64"))]',
        b'#[cfg(all(target_os = "macos", target_arch = "aarch64"))]',
        b'#[cfg(any(target_arch = "x86_64", debug_assertions))]',
        b'#[cfg(any(target_os = "windows", feature = "optional"))]',
        (
            b'#[cfg(any(all(target_os = "linux", target_arch = "x86_64"), '
            b'all(target_os = "macos", target_arch = "aarch64")))]'
        ),
        b"#[cfg(" + b" " * 4097 + b'target_os = "windows")]',
    ):
        try:
            source_platform_labels(ambiguous_source)
        except RuntimeError:
            pass
        else:
            raise AssertionError("ambiguous target predicate was accepted as narrow proof")

    old_source = "crates/projectatlas-service/src/old.rs"
    new_source = "crates/projectatlas-service/src/new.rs"

    def renamed_blob(_: Path, revision: str, path: str) -> bytes | None:
        retained = b"#[cfg(windows)]\nfn retained() {}\n"
        changed = b"#[cfg(windows)]\nfn changed() {}\n"
        return {
            ("a" * 40, old_source): retained + changed,
            ("b" * 40, new_source): retained + b"fn changed() {}\n",
        }.get((revision, path))

    renamed_platforms = changed_source_platforms(
        Path.cwd(),
        "a" * 40,
        "b" * 40,
        [Change("R100", (old_source, new_source))],
        blob_loader=renamed_blob,
    )
    assert renamed_platforms[old_source] == ("windows",)
    assert renamed_platforms[new_source] == tuple(PLATFORM_CONTRACTS)

    def split_move_blob(_: Path, revision: str, path: str) -> bytes | None:
        return {
            ("a" * 40, old_source): b"#[cfg(windows)]\nfn moved() {}\n",
            ("b" * 40, new_source): (
                b"#[cfg(windows)]\nfn unrelated() {}\nfn moved() {}\n"
            ),
        }.get((revision, path))

    split_move_platforms = changed_source_platforms(
        Path.cwd(),
        "a" * 40,
        "b" * 40,
        [Change("D", (old_source,)), Change("A", (new_source,))],
        blob_loader=split_move_blob,
    )
    assert split_move_platforms[old_source] == tuple(PLATFORM_CONTRACTS)
    assert split_move_platforms[new_source] == tuple(PLATFORM_CONTRACTS)

    def unchanged_rename_blob(_: Path, revision: str, path: str) -> bytes | None:
        return {
            ("a" * 40, old_source): b"#[cfg(windows)]\nfn retained() {}\n",
            ("b" * 40, new_source): b"#[cfg(windows)]\nfn retained() {}\n",
        }.get((revision, path))

    unchanged_rename = Change("R100", (old_source, new_source))
    unchanged_platforms = changed_source_platforms(
        Path.cwd(),
        "a" * 40,
        "b" * 40,
        [unchanged_rename],
        blob_loader=unchanged_rename_blob,
    )
    unchanged_plan = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[unchanged_rename],
        graph=graph,
        source_platforms=unchanged_platforms,
    )
    assert {
        row["label"]
        for row in unchanged_plan["platform_matrix"]["include"]
        if "compile" in row["contracts"]
    } == {"windows"}

    try:
        changed_source_platforms(
            Path.cwd(),
            "a" * 40,
            "b" * 40,
            [
                Change("M", (f"crates/projectatlas-service/src/{index}.rs",))
                for index in range(MAX_SOURCE_BLOBS // 2 + 1)
            ],
            blob_loader=lambda _root, _revision, _path: b"",
        )
    except RuntimeError:
        pass
    else:
        raise AssertionError("aggregate submitted-tree source bound was not enforced")

    cli_domain = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", ("crates/projectatlas-cli/tests/e2e_navigation.rs",))],
        graph=graph,
    )
    assert cli_domain["test_only"] is True
    assert cli_domain["test_targets"] == ["e2e_navigation"]
    assert "source-policy" in cli_domain["repository_contracts"]
    assert cli_domain["platform_matrix"]["include"] == [
        {"label": "linux", "os": "ubuntu-latest", "contracts": ["navigation"]}
    ]

    inventory = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", (CLI_E2E_INVENTORY_PATH,))],
        graph=graph,
    )
    assert inventory["test_only"] is True
    assert inventory["rust_packages"] == ["projectatlas-cli"]
    assert inventory["test_targets"] == ["e2e_delivery"]
    assert inventory["platform_matrix"] == {"include": []}

    target_specific_test = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", ("crates/projectatlas-cli/tests/lint_diagnostics.rs",))],
        graph=graph,
        source_platforms={
            "crates/projectatlas-cli/tests/lint_diagnostics.rs": MAC_PLATFORM_LABELS
        },
    )
    assert target_specific_test["test_targets"] == ["lint_diagnostics"]
    assert target_specific_test["platform_matrix"]["include"] == [
        {"label": label, "os": PLATFORM_OS[label], "contracts": ["compile"]}
        for label in MAC_PLATFORM_LABELS
    ]

    shared = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", ("crates/projectatlas-core/src/lib.rs",))],
        graph=graph,
    )
    assert shared["mode"] == "full"

    platform = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", ("crates/projectatlas-cli/src/plugin_installer.rs",))],
        graph=graph,
    )
    assert [item["label"] for item in platform["platform_matrix"]["include"]] == list(
        OS_PLATFORM_LABELS
    )
    assert all(
        "plugin" in item["contracts"] for item in platform["platform_matrix"]["include"]
    )
    assert platform["test_targets"] == ["e2e_delivery", "installer_trust_boundaries"]

    for installer_script in INSTALLER_SCRIPT_PATHS:
        plugin_script = plan_changes(
            base="a" * 40,
            head="b" * 40,
            event="pull_request",
            changes=[Change("M", (installer_script,))],
            graph=graph,
        )
        assert plugin_script["test_only"] is True
        assert plugin_script["test_targets"] == [
            "e2e_delivery",
            "installer_trust_boundaries",
        ]

    for plugin_asset in (
        "plugins/projectatlas/.codex-plugin/plugin.json",
        "plugins/projectatlas/scripts/install-runtime.md",
    ):
        plugin_plan = plan_changes(
            base="a" * 40,
            head="b" * 40,
            event="pull_request",
            changes=[Change("M", (plugin_asset,))],
            graph=graph,
        )
        assert plugin_plan["test_targets"] == ["e2e_delivery"]

    unknown_install = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", ("installation-notes.md",))],
        graph=graph,
    )
    assert unknown_install["mode"] == "full"

    try:
        parse_planning_changes(b"", base="a" * 40, head="a" * 40, force_full=False)
    except ValueError:
        pass
    else:
        raise AssertionError("ordinary empty diff did not fail closed")
    forced_changes = parse_planning_changes(
        b"", base="a" * 40, head="a" * 40, force_full=True
    )
    forced_empty = plan_changes(
        base="a" * 40,
        head="a" * 40,
        event="push",
        changes=forced_changes,
        graph=graph,
        force_full=True,
    )
    assert forced_empty["mode"] == "full"
    assert set(forced_empty["repository_contracts"]) == set(REPOSITORY_CONTRACTS)
    assert set(forced_empty["rust_packages"]) == set(PACKAGE_NAMES)
    assert len(forced_empty["platform_matrix"]["include"]) == len(PLATFORM_CONTRACTS)
    assert set(forced_empty["test_targets"]) == set(CLI_TEST_TARGETS)
    assert forced_empty["omitted"] == []

    assert OBJECT_ID_LENGTHS == {"sha1": 40, "sha256": 64}
    assert object_id_length("sha1") == 40
    assert object_id_length("sha256") == 64
    validate_object_id("a" * 40, object_id_length("sha1"))
    validate_object_id("a" * 64, object_id_length("sha256"))
    for invalid_oid, expected_length in (("a" * 40, 64), ("g" * 64, 64)):
        try:
            validate_object_id(invalid_oid, expected_length)
        except RuntimeError:
            pass
        else:
            raise AssertionError("invalid native object ID was accepted")

    shared_cli = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", ("crates/projectatlas-cli/src/main.rs",))],
        graph=graph,
    )
    assert shared_cli["mode"] == "full"

    renamed = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[
            Change(
                "R100",
                (
                    "crates/projectatlas-lints/src/renamed.rs",
                    "crates/projectatlas-cli/tests/e2e_navigation.rs",
                ),
            )
        ],
        graph=graph,
    )
    assert renamed["mode"] == "narrow"
    assert renamed["rust_packages"] == ["projectatlas-cli", "projectatlas-lints"]
    assert renamed["test_targets"] == ["e2e_navigation", "lint_diagnostics"]
    assert renamed["platform_matrix"]["include"] == [
        {"label": "linux", "os": "ubuntu-latest", "contracts": ["navigation"]}
    ]

    deleted = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("D", ("crates/projectatlas-lints/src/removed.rs",))],
        graph=graph,
    )
    assert deleted["mode"] == "narrow"
    assert deleted["rust_packages"] == ["projectatlas-lints"]

    unknown = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("D", ("unmapped.file",))],
        graph=graph,
    )
    assert unknown["mode"] == "full"

    ignored_policy = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", (".gitignore",))],
        graph=graph,
    )
    assert ignored_policy["mode"] == "full"

    def fail_metadata(_: Path) -> CargoGraph:
        raise RuntimeError("deterministic metadata failure")

    metadata_failure = plan_with_cargo_graph(
        root=Path.cwd(),
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", ("crates/projectatlas-lints/src/lib.rs",))],
        graph_loader=fail_metadata,
    )
    assert metadata_failure["mode"] == "full"
    assert metadata_failure["reasons"] == ["Cargo metadata unavailable or invalid"]

    bounded = build_plan(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", (f"docs/{index}.md",)) for index in range(MAX_PATHS)],
        mode="narrow",
        reasons=["documentation"],
        repository={"issueops", "mermaid"},
        packages=set(),
        test_targets=set(),
        test_only=False,
        platforms={label: set() for label in PLATFORM_CONTRACTS},
    )
    assert len(bounded["changes"]) == MAX_REPORTED_CHANGES
    assert len(summary(bounded).encode("utf-8")) <= MAX_SUMMARY_BYTES
    assert (
        len(json.dumps(bounded, separators=(",", ":"), sort_keys=True).encode("utf-8"))
        <= MAX_PLAN_BYTES
    )
    oversized_summary = dict(bounded)
    oversized_summary["reasons"] = ["x" * MAX_SUMMARY_BYTES]
    try:
        summary(oversized_summary)
    except RuntimeError:
        pass
    else:
        raise AssertionError("oversized summary was accepted")
    oversized_plan = dict(bounded)
    oversized_plan["reasons"] = ["x" * MAX_PLAN_BYTES]
    try:
        write_outputs(oversized_plan, None, None)
    except RuntimeError:
        pass
    else:
        raise AssertionError("oversized plan was accepted")
    aggregate(
        plan,
        "a" * 40,
        "b" * 40,
        "pull_request",
        {"repository": "success", "rust": "success", "platform": "skipped"},
    )
    for name, result in (
        ("repository", "failure"),
        ("repository", "cancelled"),
        ("repository", "skipped"),
        ("repository", "missing"),
        ("rust", "failure"),
        ("rust", "cancelled"),
        ("rust", "skipped"),
        ("rust", "missing"),
        ("platform", "success"),
    ):
        failing = {"repository": "success", "rust": "success", "platform": "skipped"}
        failing[name] = result
        try:
            aggregate(plan, "a" * 40, "b" * 40, "pull_request", failing)
        except RuntimeError:
            pass
        else:
            raise AssertionError("selected failed job was accepted")
    try:
        aggregate(
            plan,
            "c" * 40,
            "b" * 40,
            "pull_request",
            {"repository": "success", "rust": "success", "platform": "skipped"},
        )
    except RuntimeError:
        pass
    else:
        raise AssertionError("stale binding was accepted")
    try:
        aggregate(
            plan,
            "a" * 40,
            "c" * 40,
            "pull_request",
            {"repository": "success", "rust": "success", "platform": "skipped"},
        )
    except RuntimeError:
        pass
    else:
        raise AssertionError("stale head binding was accepted")
    try:
        aggregate(
            plan,
            "a" * 40,
            "b" * 40,
            "push",
            {"repository": "success", "rust": "success", "platform": "skipped"},
        )
    except RuntimeError:
        pass
    else:
        raise AssertionError("stale event binding was accepted")
    full = full_plan(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", ("unknown",))],
        reason="unknown",
        dependency_audit=False,
        cargo_dependency=False,
    )
    assert full["mode"] == "full" and len(full["platform_matrix"]["include"]) == 4
    assert "dependency-audit" not in full["repository_contracts"]
    assert "cargo-dependency" not in full["repository_contracts"]
    deny_policy = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", ("deny.toml",))],
        graph=graph,
    )
    assert deny_policy["mode"] == "full"
    assert "cargo-dependency" in deny_policy["repository_contracts"]
    drift = full_plan(
        base="a" * 40,
        head="b" * 40,
        event="schedule",
        changes=[Change("M", ("unknown",))],
        reason="drift",
        dependency_audit=False,
        cargo_dependency=False,
    )
    assert "dependency-audit" in drift["repository_contracts"]
    assert "cargo-dependency" in drift["repository_contracts"]
    print("affected CI proof self-test passed")


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser()
    result.add_argument("--self-test", action="store_true")
    subparsers = result.add_subparsers(dest="command")
    plan = subparsers.add_parser("plan")
    plan.add_argument("--base", required=True)
    plan.add_argument("--head", required=True)
    plan.add_argument("--event", required=True)
    plan.add_argument("--force-full", action="store_true")
    plan.add_argument("--github-output", type=Path)
    plan.add_argument("--summary", type=Path)
    aggregate_parser = subparsers.add_parser("aggregate")
    aggregate_parser.add_argument("--plan", required=True)
    aggregate_parser.add_argument("--base", required=True)
    aggregate_parser.add_argument("--head", required=True)
    aggregate_parser.add_argument("--event", required=True)
    aggregate_parser.add_argument("--repository-result", required=True)
    aggregate_parser.add_argument("--rust-result", required=True)
    aggregate_parser.add_argument("--platform-result", required=True)
    return result


def main() -> int:
    args = parser().parse_args()
    if args.self_test:
        self_test()
        return 0
    if args.command == "aggregate":
        aggregate(
            json.loads(args.plan),
            args.base,
            args.head,
            args.event,
            {
                "repository": args.repository_result,
                "rust": args.rust_result,
                "platform": args.platform_result,
            },
        )
        print("affected CI proof aggregate passed")
        return 0
    if args.command != "plan":
        raise RuntimeError("plan or aggregate command is required")
    root = Path.cwd()
    object_id_length = repository_object_id_length(root)
    base = exact_commit(args.base, root, object_id_length)
    head = exact_commit(args.head, root, object_id_length)
    changes = parse_planning_changes(
        git("diff", "--name-status", "-z", "--find-renames", base, head, "--", root=root),
        base=base,
        head=head,
        force_full=args.force_full,
    )
    plan = plan_with_cargo_graph(
        root=root,
        base=base,
        head=head,
        event=args.event,
        changes=changes,
        force_full=args.force_full,
    )
    write_outputs(plan, args.github_output, args.summary)
    print(json.dumps(plan, separators=(",", ":"), sort_keys=True))
    return 0


if __name__ == "__main__":
    try:
        raise SystemExit(main())
    except (OSError, RuntimeError, ValueError, json.JSONDecodeError) as error:
        print(f"affected-ci-proof: {error}", file=sys.stderr)
        raise SystemExit(1) from error
