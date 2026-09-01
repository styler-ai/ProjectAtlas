#!/usr/bin/env python3
"""Keep full benchmark campaigns out of automated validation and release paths."""

from __future__ import annotations

import re
import subprocess
import sys
import tempfile
from pathlib import Path, PurePosixPath

CAMPAIGN_ENTRYPOINTS = ("agent_navigation.py", "system_scale.py")
MAX_TRACKED_RAW_BENCHMARK_BYTES = 4 * 1024 * 1024


def is_oversized_tracked_raw_result(
    relative: str, size: int, *, tracked: bool = True
) -> bool:
    """Identify oversized root-level JSON benchmark result formats."""

    path = PurePosixPath(relative)
    return (
        tracked
        and path.parts[:2] == ("docs", "benchmarks")
        and len(path.parts) == 3
        and path.suffix.lower() in {".json", ".jsonl"}
        and size > MAX_TRACKED_RAW_BENCHMARK_BYTES
    )


def oversized_tracked_raw_results(root: Path) -> list[tuple[str, int]]:
    """Return oversized tracked raw benchmark blobs selected by ``HEAD``."""

    process = subprocess.run(
        ["git", "ls-tree", "-r", "-l", "-z", "HEAD", "--", "docs/benchmarks"],
        cwd=root,
        check=False,
        capture_output=True,
        timeout=120,
    )
    if process.returncode != 0:
        raise RuntimeError("could not inspect committed benchmark blobs")

    findings: list[tuple[str, int]] = []
    for encoded in process.stdout.split(b"\0"):
        if not encoded:
            continue
        header, separator, encoded_path = encoded.partition(b"\t")
        fields = header.split()
        if not separator or len(fields) != 4 or fields[1] != b"blob":
            continue
        relative = encoded_path.decode("utf-8")
        size = int(fields[3])
        if is_oversized_tracked_raw_result(relative, size):
            findings.append((relative, size))
    return findings


def campaign_entrypoint(line: str) -> bool:
    normalized = line.replace("\\", "/")
    return any(
        f"docs/benchmarks/harness/{entrypoint}" in normalized
        or re.search(
            rf"\b(?:python(?:3(?:\.\d+)?)?|py)\b[^#\n]*\b{re.escape(entrypoint)}\b",
            normalized,
        )
        for entrypoint in CAMPAIGN_ENTRYPOINTS
    )


def self_test() -> None:
    """Protect the narrow raw-trace boundary and its allowed false positives."""

    assert not is_oversized_tracked_raw_result(
        "docs/benchmarks/v0.4-agent-navigation-results.json",
        MAX_TRACKED_RAW_BENCHMARK_BYTES,
    )
    assert is_oversized_tracked_raw_result(
        "docs/benchmarks/v0.5-reverse-caller-performance-results.json",
        MAX_TRACKED_RAW_BENCHMARK_BYTES + 1,
    )
    assert not is_oversized_tracked_raw_result(
        "docs/benchmarks/fixtures/generated.jsonl",
        MAX_TRACKED_RAW_BENCHMARK_BYTES + 1,
    )
    assert not is_oversized_tracked_raw_result(
        "docs/benchmarks/local-output.jsonl",
        MAX_TRACKED_RAW_BENCHMARK_BYTES + 1,
        tracked=False,
    )
    assert not is_oversized_tracked_raw_result(
        "release/benchmark.jsonl.gz", MAX_TRACKED_RAW_BENCHMARK_BYTES + 1
    )
    assert is_oversized_tracked_raw_result(
        "docs/benchmarks/v0.4-agent-navigation-failed-binary-init-29a4863.jsonl",
        MAX_TRACKED_RAW_BENCHMARK_BYTES + 1,
    )
    assert is_oversized_tracked_raw_result(
        r"docs/benchmarks/trace\part.jsonl",
        MAX_TRACKED_RAW_BENCHMARK_BYTES + 1,
    )
    assert not is_oversized_tracked_raw_result(
        "docs/benchmarks/v0.4-agent-navigation-failed-binary-init-29a4863.jsonl",
        MAX_TRACKED_RAW_BENCHMARK_BYTES,
    )

    temporary_root = Path(__file__).resolve().parents[2] / ".tmp"
    temporary_root.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(
        prefix="benchmark-policy-", dir=temporary_root
    ) as temporary:
        root = Path(temporary)
        benchmark_root = root / "docs" / "benchmarks"
        benchmark_root.mkdir(parents=True)
        oversized = benchmark_root / "oversized.jsonl"
        oversized_json = benchmark_root / "oversized.json"
        compact = benchmark_root / "compact.jsonl"
        compact_json = benchmark_root / "compact.json"
        oversized.write_bytes(b"x" * (MAX_TRACKED_RAW_BENCHMARK_BYTES + 1))
        oversized_json.write_bytes(b"x" * (MAX_TRACKED_RAW_BENCHMARK_BYTES + 1))
        compact.write_bytes(b"x" * (MAX_TRACKED_RAW_BENCHMARK_BYTES - 1))
        compact_json.write_bytes(b"x" * (MAX_TRACKED_RAW_BENCHMARK_BYTES - 1))
        for command in (
            ["git", "init", "--quiet"],
            [
                "git",
                "add",
                "--",
                "docs/benchmarks/oversized.jsonl",
                "docs/benchmarks/oversized.json",
                "docs/benchmarks/compact.jsonl",
                "docs/benchmarks/compact.json",
            ],
            [
                "git",
                "-c",
                "user.name=benchmark-policy-self-test",
                "-c",
                "user.email=benchmark-policy-self-test@example.invalid",
                "commit",
                "--quiet",
                "-m",
                "fixture",
            ],
        ):
            subprocess.run(
                command,
                cwd=root,
                check=True,
                capture_output=True,
                timeout=30,
            )

        expected = [
            ("docs/benchmarks/oversized.json", MAX_TRACKED_RAW_BENCHMARK_BYTES + 1),
            ("docs/benchmarks/oversized.jsonl", MAX_TRACKED_RAW_BENCHMARK_BYTES + 1),
        ]
        assert oversized_tracked_raw_results(root) == expected
        oversized.unlink()
        compact.write_bytes(b"x" * (MAX_TRACKED_RAW_BENCHMARK_BYTES + 1))
        assert oversized_tracked_raw_results(root) == expected


def main() -> int:
    self_test()
    if campaign_entrypoint("python docs/benchmarks/harness/test_agent_navigation.py"):
        raise RuntimeError("benchmark unit tests must remain allowed")
    if not campaign_entrypoint(
        r"python docs\benchmarks\harness\agent_navigation.py --repeats 3"
    ):
        raise RuntimeError("full campaign invocation must be detected")
    if not campaign_entrypoint("python3 system_scale.py"):
        raise RuntimeError("basename campaign invocation must be detected")
    if not campaign_entrypoint(
        "cd docs/benchmarks/harness && python agent_navigation.py"
    ):
        raise RuntimeError("changed-directory campaign invocation must be detected")

    root = Path(__file__).resolve().parents[2]
    workflow_root = root / ".github/workflows"
    automated_routes = [
        root / ".githooks/pre-push",
        *sorted(workflow_root.glob("*.yml")),
        *sorted(workflow_root.glob("*.yaml")),
    ]
    violations: list[str] = []
    for path in automated_routes:
        relative_path = path.relative_to(root).as_posix()
        for line_number, line in enumerate(
            path.read_text(encoding="utf-8").splitlines(), start=1
        ):
            if campaign_entrypoint(line):
                violations.append(f"{relative_path}:{line_number}")

    if violations:
        print(
            "full system-scale and agent-navigation campaigns are manual-only; "
            "remove automated invocations from " + ", ".join(violations),
            file=sys.stderr,
        )
        return 1
    oversized = oversized_tracked_raw_results(root)
    if oversized:
        shown = oversized[:8]
        details = ", ".join(f"{path} ({size} bytes)" for path, size in shown)
        if len(oversized) > len(shown):
            details += f", ... and {len(oversized) - len(shown)} more"
        print(
            "tracked raw benchmark traces exceed the 4 MiB source bound; "
            "retain compact sanitized evidence or use ignored local output: " + details,
            file=sys.stderr,
        )
        return 1
    print("manual-only benchmark routing policy passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
