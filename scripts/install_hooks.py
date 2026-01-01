"""
Purpose: Install ProjectAtlas git hooks by setting core.hooksPath.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path


def main(argv: list[str]) -> int:
    """Configure git hooks path for the current repository."""
    _ = argv
    root = Path(__file__).resolve().parents[1]
    hooks_path = root / ".githooks"
    if not hooks_path.exists():
        sys.stderr.write("Missing .githooks directory.\n")
        return 1
    subprocess.run(
        ["git", "config", "core.hooksPath", str(hooks_path)],
        check=True,
        cwd=root,
    )
    sys.stdout.write(f"Configured hooks path: {hooks_path}\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
