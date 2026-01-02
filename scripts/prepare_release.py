"""
Purpose: Bump the ProjectAtlas version and open a release PR.
"""

from __future__ import annotations

import argparse
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from scripts.next_version import (  # noqa: E402
    bump_base_version,
    read_pyproject_version,
    update_version_files,
)


@dataclass(frozen=True)
class ReleasePlan:
    """Capture the release version bump and PR metadata."""

    version: str
    issue: str
    base_branch: str
    head_branch: str
    commit_message: str
    pr_title: str
    pr_body: str


def normalize_issue(issue: str) -> str:
    """Normalize an issue reference into #NNN form."""
    trimmed = issue.strip()
    if trimmed.startswith("#"):
        return trimmed
    return f"#{trimmed}"


def build_release_plan(
    current_version: str,
    bump: str,
    issue: str,
    base_branch: str,
    head_branch: str,
) -> ReleasePlan:
    """Construct the release plan metadata for the given version bump."""
    if ".dev" in current_version:
        next_version = current_version.split(".dev", maxsplit=1)[0]
    else:
        next_version = bump_base_version(current_version, bump)
    issue_ref = normalize_issue(issue)
    commit_message = f"chore(release): prepare v{next_version} ({issue_ref})"
    pr_title = f"release: v{next_version} ({issue_ref})"
    pr_body = "\n".join(
        [
            f"## Summary",
            f"- Prepare v{next_version} release.",
            "",
            "## Checklist",
            "- [ ] CI green",
            "- [ ] Ready to merge to main",
        ]
    )
    return ReleasePlan(
        version=next_version,
        issue=issue_ref,
        base_branch=base_branch,
        head_branch=head_branch,
        commit_message=commit_message,
        pr_title=pr_title,
        pr_body=pr_body,
    )


def run_command(command: list[str], dry_run: bool) -> None:
    """Run a shell command or print it in dry-run mode."""
    if dry_run:
        print("DRY-RUN:", " ".join(command))
        return
    subprocess.run(command, check=True)


def ensure_clean_git(root: Path) -> None:
    """Abort if the git worktree is not clean."""
    result = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=root,
        check=True,
        capture_output=True,
        text=True,
    )
    if result.stdout.strip():
        raise RuntimeError("Git worktree is not clean; commit or stash changes.")


def parse_args(argv: list[str]) -> argparse.Namespace:
    """Parse CLI arguments."""
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--bump",
        choices=("major", "minor", "patch"),
        default="patch",
        help="Which version component to bump.",
    )
    parser.add_argument(
        "--issue",
        required=True,
        help="GitHub issue number for the release PR.",
    )
    parser.add_argument(
        "--base",
        default="main",
        help="Base branch for the release PR.",
    )
    parser.add_argument(
        "--head",
        default="dev",
        help="Head branch for the release PR.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print commands without running them.",
    )
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    """Execute the release preparation flow."""
    args = parse_args(argv)
    current_version = read_pyproject_version(ROOT / "pyproject.toml")
    plan = build_release_plan(
        current_version=current_version,
        bump=args.bump,
        issue=args.issue,
        base_branch=args.base,
        head_branch=args.head,
    )
    if not args.dry_run:
        ensure_clean_git(ROOT)
        update_version_files(ROOT, plan.version)
    run_command(
        ["git", "add", "pyproject.toml", "src/projectatlas/__init__.py"],
        args.dry_run,
    )
    run_command(["git", "commit", "-m", plan.commit_message], args.dry_run)
    run_command(["git", "push", "origin", plan.head_branch], args.dry_run)
    run_command(
        [
            "gh",
            "pr",
            "create",
            "--base",
            plan.base_branch,
            "--head",
            plan.head_branch,
            "--title",
            plan.pr_title,
            "--body",
            plan.pr_body,
        ],
        args.dry_run,
    )
    print(f"Release PR created for v{plan.version}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
