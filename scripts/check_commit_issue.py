"""
Purpose: Enforce GitHub issue references in commit messages.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path


ISSUE_RE = re.compile(r"#\d+")


def commit_has_issue(commit_message: str) -> bool:
    """Return True if the commit message contains a #NNN reference."""
    return bool(ISSUE_RE.search(commit_message))


def read_commit_message(path: Path) -> str:
    """Read the commit message file."""
    return path.read_text(encoding="utf-8")


def main(argv: list[str]) -> int:
    """Exit non-zero when the commit message lacks an issue reference."""
    if len(argv) != 1:
        sys.stderr.write("Usage: check_commit_issue.py <commit-msg-file>\n")
        return 1
    path = Path(argv[0])
    message = read_commit_message(path)
    if not commit_has_issue(message):
        sys.stderr.write(
            "Commit message must reference a GitHub issue (example: #123).\n"
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
