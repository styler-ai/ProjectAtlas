#!/usr/bin/env python3
"""Keep full benchmark campaigns out of automated validation and release paths."""

from __future__ import annotations

import sys
from pathlib import Path

CAMPAIGN_ENTRYPOINTS = ("agent_navigation.py", "system_scale.py")


def campaign_entrypoint(line: str) -> bool:
    normalized = line.replace("\\", "/")
    return any(
        f"docs/benchmarks/harness/{entrypoint}" in normalized
        for entrypoint in CAMPAIGN_ENTRYPOINTS
    )


def main() -> int:
    if campaign_entrypoint("python docs/benchmarks/harness/test_agent_navigation.py"):
        raise RuntimeError("benchmark unit tests must remain allowed")
    if not campaign_entrypoint(
        r"python docs\benchmarks\harness\agent_navigation.py --repeats 3"
    ):
        raise RuntimeError("full campaign invocation must be detected")

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
    print("manual-only benchmark routing policy passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
