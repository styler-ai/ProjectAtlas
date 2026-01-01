"""
Purpose: Enforce issue references in pull request titles or bodies.
"""

from __future__ import annotations

import json
import os
import re
import sys
from pathlib import Path
from typing import Any


ISSUE_RE = re.compile(r"#\d+")


def load_event_payload(event_path: Path) -> dict[str, Any]:
    """Load the GitHub event payload JSON."""
    return json.loads(event_path.read_text(encoding="utf-8"))


def extract_pr_text(payload: dict[str, Any]) -> str | None:
    """Return PR title/body text if present."""
    pr = payload.get("pull_request")
    if not pr:
        return None
    title = str(pr.get("title", ""))
    body = str(pr.get("body", ""))
    return f"{title}\n{body}"


def has_issue_reference(text: str) -> bool:
    """Check whether the text contains a #NNN issue reference."""
    return bool(ISSUE_RE.search(text))


def main(argv: list[str]) -> int:
    """Exit non-zero if a PR event lacks an issue reference."""
    _ = argv
    event_path = os.environ.get("GITHUB_EVENT_PATH")
    if not event_path:
        return 0
    payload = load_event_payload(Path(event_path))
    text = extract_pr_text(payload)
    if text is None:
        return 0
    if not has_issue_reference(text):
        sys.stderr.write(
            "PR title/body must reference an issue (example: #123).\n"
        )
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
