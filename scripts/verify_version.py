"""
Purpose: Validate that ProjectAtlas versions match the requested tag.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

import tomllib


ROOT = Path(__file__).resolve().parents[1]
PYPROJECT_PATH = ROOT / "pyproject.toml"
INIT_PATH = ROOT / "src" / "projectatlas" / "__init__.py"


def read_pyproject_version() -> str:
    """Read the version from pyproject.toml."""
    payload = tomllib.loads(PYPROJECT_PATH.read_text(encoding="utf-8"))
    return str(payload["project"]["version"])


def read_package_version() -> str:
    """Read the __version__ value from the package __init__."""
    text = INIT_PATH.read_text(encoding="utf-8")
    match = re.search(r'__version__\s*=\s*"([^"]+)"', text)
    if not match:
        raise ValueError("Could not find __version__ in __init__.py")
    return match.group(1)


def normalize_version(version: str) -> str:
    """Normalize a version or tag (strip leading v)."""
    return version.lstrip("v")


def main(argv: list[str]) -> int:
    """Run the version validation."""
    if len(argv) != 1:
        raise SystemExit("Usage: python scripts/verify_version.py <version>")
    requested = normalize_version(argv[0])
    pyproject_version = read_pyproject_version()
    package_version = read_package_version()
    errors: list[str] = []
    if pyproject_version != requested:
        errors.append(
            f"pyproject.toml version {pyproject_version} != {requested}"
        )
    if package_version != requested:
        errors.append(
            f"__init__.py version {package_version} != {requested}"
        )
    if errors:
        raise SystemExit("\n".join(errors))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
