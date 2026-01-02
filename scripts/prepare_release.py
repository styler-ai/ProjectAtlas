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
    build_version,
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


def compute_release_version(current_version: str, bump: str) -> str:
    """Return the release version derived from the current version."""
    if ".dev" in current_version:
        return current_version.split(".dev", maxsplit=1)[0]
    return bump_base_version(current_version, bump)


def compute_post_release_version(release_version: str, bump: str) -> str:
    """Return the next dev version after a release."""
    next_base = bump_base_version(release_version, bump)
    return build_version(next_base, dev=True)


def build_release_plan(
    release_version: str,
    issue: str,
    base_branch: str,
    head_branch: str,
) -> ReleasePlan:
    """Construct the release plan metadata for the given version bump."""
    issue_ref = normalize_issue(issue)
    commit_message = f"chore(release): prepare v{release_version} ({issue_ref})"
    pr_title = f"release: v{release_version} ({issue_ref})"
    pr_body = "\n".join(
        [
            f"## Summary",
            f"- Prepare v{release_version} release.",
            "",
            "## Checklist",
            "- [ ] CI green",
            "- [ ] Ready to merge to main",
        ]
    )
    return ReleasePlan(
        version=release_version,
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
    subprocess.run(command, check=True, cwd=ROOT)


def run_capture(command: list[str]) -> str:
    """Run a command and return stdout."""
    result = subprocess.run(
        command,
        cwd=ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return result.stdout.strip()


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


def current_branch() -> str:
    """Return the current git branch name."""
    return run_capture(["git", "rev-parse", "--abbrev-ref", "HEAD"])


def ensure_branch_available(branch: str) -> None:
    """Fail if a local or remote branch already exists."""
    local = run_capture(["git", "branch", "--list", branch])
    remote = run_capture(["git", "branch", "-r", "--list", f"origin/{branch}"])
    if local or remote:
        raise RuntimeError(f"Branch already exists: {branch}")


def ensure_source_contains_base(source: str, base: str, allow_divergence: bool) -> None:
    """Ensure source branch contains the base branch history."""
    run_command(["git", "fetch", "origin", "--prune"], dry_run=False)
    counts = run_capture(
        [
            "git",
            "rev-list",
            "--left-right",
            "--count",
            f"origin/{base}...origin/{source}",
        ]
    )
    base_ahead, _ = [int(value) for value in counts.split()]
    if base_ahead > 0 and not allow_divergence:
        raise RuntimeError(
            f"{source} is behind {base}. Sync {base} into {source} before releasing."
        )


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
        default=None,
        help="Head branch for the release PR (defaults to release/v<version>).",
    )
    parser.add_argument(
        "--source",
        default="dev",
        help="Source branch to release from (expected current branch).",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print commands without running them.",
    )
    parser.add_argument(
        "--post-release",
        action="store_true",
        help="Also open a dev PR that bumps to the next .dev version.",
    )
    parser.add_argument(
        "--allow-base-divergence",
        action="store_true",
        help="Skip the base sync check when the source is behind base.",
    )
    return parser.parse_args(argv)


def main(argv: list[str]) -> int:
    """Execute the release preparation flow."""
    args = parse_args(argv)
    active_branch = current_branch()
    if active_branch != args.source and not args.dry_run:
        raise RuntimeError(
            f"Expected to run on {args.source}, found {active_branch}."
        )
    if not args.dry_run:
        ensure_source_contains_base(
            source=args.source,
            base=args.base,
            allow_divergence=args.allow_base_divergence,
        )
    current_version = read_pyproject_version(ROOT / "pyproject.toml")
    release_version = compute_release_version(current_version, args.bump)
    head_branch = args.head or f"release/v{release_version}"
    if not args.dry_run:
        ensure_branch_available(head_branch)
    plan = build_release_plan(
        release_version=release_version,
        issue=args.issue,
        base_branch=args.base,
        head_branch=head_branch,
    )
    if not args.dry_run:
        ensure_clean_git(ROOT)
    if plan.head_branch != active_branch:
        run_command(["git", "checkout", "-b", plan.head_branch], args.dry_run)
    if not args.dry_run:
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
    if args.post_release:
        post_version = compute_post_release_version(plan.version, args.bump)
        post_branch = f"post-release/v{post_version}"
        if not args.dry_run:
            ensure_branch_available(post_branch)
        run_command(["git", "checkout", args.source], args.dry_run)
        run_command(["git", "checkout", "-b", post_branch], args.dry_run)
        run_command(["git", "merge", f"origin/{args.base}"], args.dry_run)
        if not args.dry_run:
            update_version_files(ROOT, post_version)
        run_command(
            ["git", "add", "pyproject.toml", "src/projectatlas/__init__.py"],
            args.dry_run,
        )
        run_command(
            ["git", "commit", "-m", f"chore(release): start v{post_version} ({plan.issue})"],
            args.dry_run,
        )
        run_command(["git", "push", "origin", post_branch], args.dry_run)
        run_command(
            [
                "gh",
                "pr",
                "create",
                "--base",
                args.source,
                "--head",
                post_branch,
                "--title",
                f"chore(release): start v{post_version} ({plan.issue})",
                "--body",
                "\n".join(
                    [
                        "## Summary",
                        f"- Bump dev to v{post_version} after release prep.",
                        "",
                        "## Checklist",
                        "- [ ] CI green",
                        "- [ ] Ready to merge to dev",
                    ]
                ),
            ],
            args.dry_run,
        )
    print(f"Release PR created for v{plan.version}.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
