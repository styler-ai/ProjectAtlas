#!/usr/bin/env python3
"""Keep campaigns and original-artifact resource inspection explicitly selected."""

from __future__ import annotations

import re
import subprocess
import sys
import tempfile
from pathlib import Path, PurePosixPath

CAMPAIGN_ENTRYPOINTS = ("agent_navigation.py", "system_scale.py", "indexing_acceptance.py", "indexing_navigation_resources.py", "native_navigation_memory.py")
MAX_TRACKED_RAW_BENCHMARK_BYTES = 4 * 1024 * 1024


def is_oversized_tracked_raw_result(
    relative: str, size: int, *, tracked: bool = True
) -> bool:
    """Identify only oversized root-level JSONL benchmark traces."""

    path = PurePosixPath(relative)
    return (
        tracked
        and path.parts[:2] == ("docs", "benchmarks")
        and len(path.parts) == 3
        and path.suffix.lower() == ".jsonl"
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


def explicit_indexing_diagnostic(relative: str, lines: list[str], position: int) -> bool:
    """Admit separately guarded manual measurement or original-artifact inspection."""
    if "native_navigation_memory.py" in lines[position]:
        return explicit_navigation_memory_diagnostic(relative, lines, position)
    supplemental = "indexing_navigation_resources.py" in lines[position]
    if relative != ".github/workflows/release.yml" or not ("indexing_acceptance.py" in lines[position] or supplemental):
        return False
    if any(name in lines[position] for name in ("system_scale.py", "agent_navigation.py")):
        return False
    source = "\n".join(lines)
    event = re.search(r"^on:\n(.*?)(?=^\S)", source, re.MULTILINE | re.DOTALL)
    if event is None or re.findall(r"^  ([\w-]+):", event[1], re.MULTILINE) != ["workflow_dispatch"]:
        return False
    option = re.search(
        r"^      run_indexing_performance:\n((?:^        [^\n]*\n)+)",
        event[1] + "\n", re.MULTILINE,
    )
    if option is None or "        default: false\n" not in option[1] or "        type: boolean\n" not in option[1]:
        return False
    for start in range(position, -1, -1):
        if lines[start].startswith("      - name:"):
            guard = ("        if: ${{ always() && inputs.prepublish_only && inputs.run_indexing_performance && matrix.label == 'macos-arm64-posix' }}"
                     if supplemental else "        if: ${{ inputs.prepublish_only && inputs.run_indexing_performance }}")
            return guard in lines[start:position]
    return False


def explicit_navigation_memory_diagnostic(relative: str, lines: list[str], position: int) -> bool:
    """Keep original-artifact composition inspection off ordinary CI events."""
    if relative != ".github/workflows/ci.yml":
        return False
    source = "\n".join(lines)
    event = re.search(r"^on:\n(.*?)(?=^\S)", source, re.MULTILINE | re.DOTALL)
    if event is None or "  workflow_dispatch:\n" not in event[1]:
        return False
    option = re.search(r"^      navigation_memory_run_id:\n((?:^        [^\n]*\n)+)", event[1] + "\n", re.MULTILINE)
    if option is None or '        default: ""\n' not in option[1] or "        type: string\n" not in option[1]:
        return False
    isolation = "!(github.event_name == 'workflow_dispatch' && inputs.navigation_memory_run_id != '')"
    plan = re.search(r"^  plan:\n(.*?)(?=^  [\w-]+:|\Z)", source, re.MULTILINE | re.DOTALL)
    verify = re.search(r"^  verify:\n(.*?)(?=^  [\w-]+:|\Z)", source, re.MULTILINE | re.DOTALL)
    if (plan is None or f"&& {isolation}" not in plan[1] or verify is None
            or f"    if: ${{{{ always() && {isolation} }}}}" not in verify[1]):
        return False
    for start in range(position, -1, -1):
        if re.fullmatch(r"  [\w-]+:", lines[start]):
            body = lines[start:position]
            return (lines[start] == "  navigation-resource-diagnostics:"
                    and "    if: ${{ github.event_name == 'workflow_dispatch' && inputs.navigation_memory_run_id != '' }}" in body
                    and "    runs-on: ubuntu-latest" in body
                    and "    timeout-minutes: 10" in body)
    return False


def self_test() -> None:
    """Protect the narrow raw-trace boundary and its allowed false positives."""

    assert not is_oversized_tracked_raw_result(
        "docs/benchmarks/v0.4-agent-navigation-results.json",
        MAX_TRACKED_RAW_BENCHMARK_BYTES + 1,
    )
    diagnostic = [
        "on:", "  workflow_dispatch:", "    inputs:",
        "      run_indexing_performance:", "        default: false", "        type: boolean",
        "permissions:", "  contents: read", "jobs:", "  smoke:", "    steps:",
        "      - name: Explicit diagnostic",
        "        if: ${{ inputs.prepublish_only && inputs.run_indexing_performance }}",
        "        run: python docs/benchmarks/harness/indexing_acceptance.py",
    ]
    route = ".github/workflows/release.yml"
    assert explicit_indexing_diagnostic(route, diagnostic, len(diagnostic) - 1)
    for before, after in [
        ("        default: false", "        default: true"),
        ("        type: boolean", "        type: string"),
        ("  workflow_dispatch:", "  push:"),
        ("        if: ${{ inputs.prepublish_only && inputs.run_indexing_performance }}", "        if: ${{ always() }}"),
    ]:
        changed = [after if line == before else line for line in diagnostic]
        assert not explicit_indexing_diagnostic(route, changed, len(changed) - 1)
    native = [
        "on:", "  pull_request:", "  workflow_dispatch:", "    inputs:",
        "      navigation_memory_run_id:", '        default: ""', "        type: string",
        "permissions:", "  contents: read", "jobs:", "  navigation-resource-diagnostics:",
        "    if: ${{ github.event_name == 'workflow_dispatch' && inputs.navigation_memory_run_id != '' }}",
        "    runs-on: ubuntu-latest", "    timeout-minutes: 10", "    steps:",
        "      - run: python3 docs/benchmarks/harness/native_navigation_memory.py",
    ]
    native[10:10] = [
        "  plan:", "    if: ordinary_source && !(github.event_name == 'workflow_dispatch' && inputs.navigation_memory_run_id != '')",
        "  verify:", "    if: ${{ always() && !(github.event_name == 'workflow_dispatch' && inputs.navigation_memory_run_id != '') }}",
    ]
    assert campaign_entrypoint(native[-1])
    assert explicit_indexing_diagnostic(".github/workflows/ci.yml", native, len(native) - 1)
    assert not explicit_indexing_diagnostic(route, native, len(native) - 1)
    for before, after in [
        ('        default: ""', '        default: "17"'),
        ("        type: string", "        type: boolean"),
        ("  navigation-resource-diagnostics:", "  ordinary-job:"),
        ("    if: ${{ github.event_name == 'workflow_dispatch' && inputs.navigation_memory_run_id != '' }}", "    if: always()"),
        ("    runs-on: ubuntu-latest", "    runs-on: windows-latest"),
        ("    if: ${{ always() && !(github.event_name == 'workflow_dispatch' && inputs.navigation_memory_run_id != '') }}", "    if: always()"),
        ("    if: ordinary_source && !(github.event_name == 'workflow_dispatch' && inputs.navigation_memory_run_id != '')", "    if: ordinary_source"),
    ]:
        changed = [after if line == before else line for line in native]
        assert not explicit_indexing_diagnostic(".github/workflows/ci.yml", changed, len(changed) - 1)
    for entrypoint in ("indexing_acceptance.py", "indexing_navigation_resources.py", "system_scale.py", "agent_navigation.py"):
        changed = [line.replace("native_navigation_memory.py", entrypoint) for line in native]
        assert not explicit_indexing_diagnostic(".github/workflows/ci.yml", changed, len(changed) - 1)
    assert not explicit_indexing_diagnostic(".github/workflows/ci.yml", diagnostic, len(diagnostic) - 1)
    supplemental = diagnostic.copy()
    supplemental[-2] = "        if: ${{ always() && inputs.prepublish_only && inputs.run_indexing_performance && matrix.label == 'macos-arm64-posix' }}"
    supplemental[-1] = "        run: python docs/benchmarks/harness/indexing_navigation_resources.py"
    assert campaign_entrypoint(supplemental[-1])
    assert explicit_indexing_diagnostic(route, supplemental, len(supplemental) - 1)
    assert not explicit_indexing_diagnostic(".github/workflows/ci.yml", supplemental, len(supplemental) - 1)
    for before, after in [
        (" && inputs.prepublish_only", ""),
        (" && inputs.run_indexing_performance", ""),
        (" && matrix.label == 'macos-arm64-posix'", ""),
        ("        default: false", "        default: true"),
        ("  workflow_dispatch:", "  push:"),
    ]:
        changed = [line.replace(before, after) for line in supplemental]
        assert not explicit_indexing_diagnostic(route, changed, len(changed) - 1)
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
        compact = benchmark_root / "compact.jsonl"
        oversized.write_bytes(b"x" * (MAX_TRACKED_RAW_BENCHMARK_BYTES + 1))
        compact.write_bytes(b"x" * (MAX_TRACKED_RAW_BENCHMARK_BYTES - 1))
        for command in (
            ["git", "init", "--quiet"],
            [
                "git",
                "add",
                "--",
                "docs/benchmarks/oversized.jsonl",
                "docs/benchmarks/compact.jsonl",
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
            ("docs/benchmarks/oversized.jsonl", MAX_TRACKED_RAW_BENCHMARK_BYTES + 1)
        ]
        assert oversized_tracked_raw_results(root) == expected
        oversized.unlink()
        compact.write_bytes(b"x" * (MAX_TRACKED_RAW_BENCHMARK_BYTES + 1))
        assert oversized_tracked_raw_results(root) == expected


def main() -> int:
    self_test()
    if campaign_entrypoint("python3 -m unittest discover -s docs/benchmarks/harness -p test_indexing_acceptance.py"):
        raise RuntimeError("indexing arithmetic unit tests must remain allowed")
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
        lines = path.read_text(encoding="utf-8").splitlines()
        for line_number, line in enumerate(lines, start=1):
            if campaign_entrypoint(line) and not explicit_indexing_diagnostic(relative_path, lines, line_number - 1):
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
