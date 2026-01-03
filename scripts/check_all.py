"""
Purpose: Run the full local ProjectAtlas verification workflow in one command.
"""

from __future__ import annotations

import subprocess
import sys
from dataclasses import dataclass


@dataclass(frozen=True)
class Step:
    """Represent a check step in the local verification run."""

    label: str
    command: list[str]


def run_step(step: Step) -> None:
    """Run a single verification step."""
    print(f"==> {step.label}")
    subprocess.run(step.command, check=True)


def ensure_build_module() -> None:
    """Ensure the Python build module is available."""
    try:
        import build  # noqa: F401
    except ImportError as exc:
        raise SystemExit(
            "Missing build module. Install with: python -m pip install build"
        ) from exc


def main() -> int:
    """Run the full local verification sequence."""
    ensure_build_module()
    python = sys.executable
    steps = [
        Step("ProjectAtlas map", [python, "-m", "projectatlas", "map"]),
        Step(
            "ProjectAtlas lint",
            [
                python,
                "-m",
                "projectatlas",
                "lint",
                "--strict-folders",
                "--report-untracked",
            ],
        ),
        Step("Docstring check", [python, "scripts/check_docstrings.py"]),
        Step("API docs", [python, "scripts/generate_api_docs.py"]),
        Step("Unit tests", [python, "-m", "unittest", "discover", "-s", "tests"]),
        Step("Build package", [python, "-m", "build", "--sdist", "--wheel"]),
    ]
    for step in steps:
        run_step(step)
    print("All ProjectAtlas checks succeeded.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
