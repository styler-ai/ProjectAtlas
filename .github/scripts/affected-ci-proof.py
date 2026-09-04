#!/usr/bin/env python3
"""Plan the smallest closed ProjectAtlas CI proof for one exact Git change."""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
from collections.abc import Callable
from dataclasses import dataclass
from pathlib import Path


MAX_DIFF_BYTES = 1_000_000
MAX_PATHS = 2_048
MAX_PATH_BYTES = 4_096
MAX_REPORTED_CHANGES = 64
MAX_PLAN_BYTES = 900_000
MAX_SUMMARY_BYTES = 64_000

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
    "linux": ("worktree", "process", "navigation", "tui", "btrfs", "plugin", "mcp"),
    "windows": ("worktree", "process", "plugin", "windows", "mcp"),
    "macos-x64": ("mac-quality", "parser"),
    "macos-arm64": (
        "mac-quality",
        "parser",
        "worktree",
        "process",
        "tui",
        "plugin",
        "mcp",
    ),
}
PLATFORM_OS = {
    "linux": "ubuntu-latest",
    "windows": "windows-latest",
    "macos-x64": "macos-15-intel",
    "macos-arm64": "macos-14",
}
OS_PLATFORM_LABELS = ("linux", "windows", "macos-arm64")
UNIX_PLATFORM_LABELS = ("linux", "macos-arm64")
MAC_PLATFORM_LABELS = ("macos-x64", "macos-arm64")
AUTHORITY_PATHS = (
    ".cargo/",
    ".github/workflows/",
    ".github/scripts/affected-ci-proof.py",
    ".githooks/pre-push",
    "Cargo.lock",
    "Cargo.toml",
    "rust-toolchain.toml",
    "openspec/issue-map.json",
    "crates/projectatlas-db/src/schema.rs",
)
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


def exact_commit(value: str, root: Path) -> str:
    resolved = git("rev-parse", "--verify", f"{value}^{{commit}}", root=root)
    commit = resolved.decode("ascii", "strict").strip()
    if len(commit) != 40:
        raise RuntimeError(f"unreadable commit {value!r}")
    return commit


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


def platform_owners(path: str, platforms: dict[str, set[str]]) -> None:
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
        test_targets=set(),
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
) -> dict[str, object]:
    paths = [path for change in changes for path in change.paths]
    dependency_audit = any(
        path in {".github/mermaid-parser/package.json", ".github/mermaid-parser/package-lock.json"}
        for path in paths
    )
    cargo_dependency = any(path == "Cargo.lock" or path.endswith("Cargo.toml") for path in paths)
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

    repository = {"issueops", "mermaid"}
    packages: set[str] = set()
    production_packages: set[str] = set()
    test_targets: set[str] = set()
    platforms = {label: set() for label in PLATFORM_CONTRACTS}
    reasons: list[str] = []
    unknown: str | None = None
    for change in changes:
        for path in change.paths:
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
                    repository.add("source-policy")
                    platform_owners(path, platforms)
                    reasons.append(f"Cargo package {owner} owns {path}")
                else:
                    unknown = path
                    break
                continue
            if path.startswith("plugins/") or path.startswith("install"):
                repository.add("source-policy")
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
                cargo_dependency=any(
                    path == "Cargo.lock" or path.endswith("Cargo.toml") for path in paths
                ),
            )
    else:
        graph = CargoGraph(
            {name: f"crates/{name}" for name in PACKAGE_NAMES},
            {name: set() for name in PACKAGE_NAMES},
        )
    return plan_changes(
        base=base,
        head=head,
        event=event,
        changes=changes,
        graph=graph,
        force_full=force_full,
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
        "source-policy": "affected production source or installer owns source policy",
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
    assert plan["jobs"] == {"repository": True, "rust": False, "platform": False}
    assert plan["repository_contracts"] == ["issueops", "mermaid"]
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

    cli_domain = plan_changes(
        base="a" * 40,
        head="b" * 40,
        event="pull_request",
        changes=[Change("M", ("crates/projectatlas-cli/tests/e2e_navigation.rs",))],
        graph=graph,
    )
    assert cli_domain["test_only"] is True
    assert cli_domain["test_targets"] == ["e2e_navigation"]
    assert cli_domain["platform_matrix"]["include"] == [
        {"label": "linux", "os": "ubuntu-latest", "contracts": ["navigation"]}
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
        {"repository": "success", "rust": "skipped", "platform": "skipped"},
    )
    for name, result in (
        ("repository", "failure"),
        ("repository", "cancelled"),
        ("repository", "skipped"),
        ("repository", "missing"),
        ("rust", "success"),
        ("platform", "success"),
    ):
        failing = {"repository": "success", "rust": "skipped", "platform": "skipped"}
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
            {"repository": "success", "rust": "skipped", "platform": "skipped"},
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
            {"repository": "success", "rust": "skipped", "platform": "skipped"},
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
            {"repository": "success", "rust": "skipped", "platform": "skipped"},
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
    base = exact_commit(args.base, root)
    head = exact_commit(args.head, root)
    changes = parse_name_status(
        git("diff", "--name-status", "-z", "--find-renames", base, head, "--", root=root)
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
