"""
Purpose: Enforce issue references in pull request titles or bodies.
"""

from __future__ import annotations

import json
import os
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path
from typing import Any, Iterable


ISSUE_RE = re.compile(r"#(\d+)")
TRUTHY = {"1", "true", "yes", "on"}


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


def extract_issue_numbers(text: str) -> list[str]:
    """Return distinct issue numbers referenced in the text."""
    matches = ISSUE_RE.findall(text)
    return sorted({match for match in matches})


def requires_milestone() -> bool:
    """Return True when milestone enforcement is enabled."""
    raw = os.environ.get("PROJECTATLAS_REQUIRE_ISSUE_MILESTONE")
    if raw is None:
        return False
    return raw.strip().lower() in TRUTHY


def issue_has_milestone(payload: dict[str, Any]) -> bool:
    """Return True when an issue payload includes a milestone."""
    milestone = payload.get("milestone")
    if milestone is None:
        return False
    return bool(milestone.get("title"))


def fetch_issue(
    repository: str, issue_number: str, token: str
) -> dict[str, Any]:
    """Fetch issue metadata from the GitHub API."""
    url = f"https://api.github.com/repos/{repository}/issues/{issue_number}"
    request = urllib.request.Request(
        url,
        headers={
            "Accept": "application/vnd.github+json",
            "Authorization": f"Bearer {token}",
            "User-Agent": "projectatlas-ci",
        },
    )
    with urllib.request.urlopen(request) as response:
        payload = response.read().decode("utf-8")
    return json.loads(payload)


def ensure_issue_milestones(
    issue_numbers: Iterable[str], repository: str, token: str
) -> bool:
    """Return True if any referenced issue has a milestone."""
    for issue_number in issue_numbers:
        payload = fetch_issue(repository, issue_number, token)
        if issue_has_milestone(payload):
            return True
    return False


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
    if requires_milestone():
        repository = os.environ.get("GITHUB_REPOSITORY")
        token = os.environ.get("GITHUB_TOKEN")
        if not repository or not token:
            sys.stderr.write(
                "Milestone enforcement requires GITHUB_REPOSITORY and GITHUB_TOKEN.\n"
            )
            return 1
        issue_numbers = extract_issue_numbers(text)
        if not issue_numbers:
            sys.stderr.write(
                "Milestone enforcement enabled but no issue numbers found.\n"
            )
            return 1
        try:
            if not ensure_issue_milestones(issue_numbers, repository, token):
                sys.stderr.write(
                    "Referenced issues must have a milestone for the current release.\n"
                )
                return 1
        except urllib.error.HTTPError as exc:
            sys.stderr.write(
                f"Failed to fetch issue milestone data ({exc.code}).\n"
            )
            return 1
        except urllib.error.URLError as exc:
            sys.stderr.write(
                f"Failed to fetch issue milestone data ({exc.reason}).\n"
            )
            return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
