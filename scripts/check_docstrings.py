"""
Purpose: Enforce module and public symbol docstrings for ProjectAtlas code.
"""

from __future__ import annotations

import argparse
import ast
import sys
from pathlib import Path


DEFAULT_TARGETS = ("src/projectatlas", "scripts")
EXCLUDE_PARTS = {"__pycache__", ".venv", ".projectatlas"}


def iter_python_files(root: Path, targets: tuple[str, ...]) -> list[Path]:
    """Collect Python files under the target paths."""
    files: list[Path] = []
    for target in targets:
        path = root / target
        if not path.exists():
            continue
        for file_path in path.rglob("*.py"):
            if any(part in EXCLUDE_PARTS for part in file_path.parts):
                continue
            files.append(file_path)
    return sorted(files)


def is_public(name: str) -> bool:
    """Return True when a symbol should require a docstring."""
    return not name.startswith("_")


def collect_docstring_issues(root: Path, targets: tuple[str, ...]) -> list[str]:
    """Return a list of missing docstring issues."""
    issues: list[str] = []
    for file_path in iter_python_files(root, targets):
        module = ast.parse(file_path.read_text(encoding="utf-8"))
        module_doc = ast.get_docstring(module)
        if not module_doc:
            issues.append(f"{file_path.relative_to(root)}: missing module docstring")
        for node in module.body:
            if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)):
                if is_public(node.name) and not ast.get_docstring(node):
                    issues.append(
                        f"{file_path.relative_to(root)}: function {node.name} missing docstring"
                    )
            if isinstance(node, ast.ClassDef):
                if is_public(node.name) and not ast.get_docstring(node):
                    issues.append(
                        f"{file_path.relative_to(root)}: class {node.name} missing docstring"
                    )
                for member in node.body:
                    if isinstance(member, (ast.FunctionDef, ast.AsyncFunctionDef)):
                        if is_public(member.name) and not ast.get_docstring(member):
                            issues.append(
                                f"{file_path.relative_to(root)}: method {node.name}.{member.name} missing docstring"
                            )
    return issues


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse CLI arguments."""
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--root",
        type=Path,
        default=Path.cwd(),
        help="Project root to scan.",
    )
    parser.add_argument(
        "--targets",
        nargs="*",
        default=list(DEFAULT_TARGETS),
        help="Relative paths to scan for docstrings.",
    )
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    """Run the docstring enforcement check."""
    args = parse_args(argv)
    root = args.root.resolve()
    targets = tuple(args.targets)
    issues = collect_docstring_issues(root, targets)
    if issues:
        sys.stderr.write("\n".join(issues) + "\n")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
