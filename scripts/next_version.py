"""
Purpose: Compute the next ProjectAtlas version and optionally update files.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

import tomllib


VERSION_RE = re.compile(
    r"^(?P<major>\d+)\.(?P<minor>\d+)\.(?P<patch>\d+)(?P<suffix>\.dev\d+|\.post\d+)?$"
)


def parse_version(version: str) -> tuple[int, int, int, str | None]:
    """Parse a PEP 440-compatible version string."""
    match = VERSION_RE.match(version.strip())
    if not match:
        raise ValueError(f"Unsupported version format: {version}")
    major = int(match.group("major"))
    minor = int(match.group("minor"))
    patch = int(match.group("patch"))
    suffix = match.group("suffix")
    return major, minor, patch, suffix


def bump_base_version(version: str, bump: str) -> str:
    """Return the next base version string without suffix."""
    major, minor, patch, _ = parse_version(version)
    if bump == "major":
        return f"{major + 1}.0.0"
    if bump == "minor":
        return f"{major}.{minor + 1}.0"
    if bump == "patch":
        return f"{major}.{minor}.{patch + 1}"
    raise ValueError(f"Unsupported bump: {bump}")


def build_version(base: str, dev: bool) -> str:
    """Return the final version string with optional dev suffix."""
    if dev:
        return f"{base}.dev0"
    return base


def read_pyproject_version(pyproject_path: Path) -> str:
    """Read the version from pyproject.toml."""
    payload = tomllib.loads(pyproject_path.read_text(encoding="utf-8"))
    return str(payload["project"]["version"])


def update_version_files(root: Path, version: str) -> None:
    """Update version strings in pyproject.toml and package __init__."""
    pyproject_path = root / "pyproject.toml"
    init_path = root / "src" / "projectatlas" / "__init__.py"
    pyproject_text = pyproject_path.read_text(encoding="utf-8")
    pyproject_text = re.sub(
        r'(^version\s*=\s*")[^"]+(")',
        rf'\g<1>{version}\2',
        pyproject_text,
        flags=re.MULTILINE,
    )
    pyproject_path.write_text(pyproject_text, encoding="utf-8")
    init_text = init_path.read_text(encoding="utf-8")
    init_text = re.sub(
        r'(__version__\s*=\s*")[^"]+(")',
        rf'\g<1>{version}\2',
        init_text,
    )
    init_path.write_text(init_text, encoding="utf-8")


def main(argv: list[str]) -> int:
    """Compute the next version and optionally write it."""
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--bump",
        choices=("major", "minor", "patch"),
        default="patch",
        help="Which component to bump.",
    )
    parser.add_argument(
        "--dev",
        action="store_true",
        help="Append .dev0 to the computed version.",
    )
    parser.add_argument(
        "--apply",
        action="store_true",
        help="Write the new version to pyproject.toml and __init__.py.",
    )
    args = parser.parse_args(argv)
    root = Path(__file__).resolve().parents[1]
    current = read_pyproject_version(root / "pyproject.toml")
    base = bump_base_version(current, args.bump)
    version = build_version(base, args.dev)
    if args.apply:
        update_version_files(root, version)
    print(version)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
