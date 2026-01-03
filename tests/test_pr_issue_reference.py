"""
Purpose: Validate pull request issue reference enforcement.
"""

from __future__ import annotations

import json
import os
import tempfile
import unittest
from pathlib import Path

from scripts.check_pr_issue_reference import (
    extract_pr_text,
    extract_issue_numbers,
    has_issue_reference,
    issue_has_milestone,
    main,
)


class PullRequestIssueReferenceTests(unittest.TestCase):
    """Cover PR title/body issue reference checks."""

    def test_extract_pr_text(self) -> None:
        payload = {
            "pull_request": {"title": "Fix bug #12", "body": "Details"},
        }
        text = extract_pr_text(payload)
        self.assertEqual(text, "Fix bug #12\nDetails")

    def test_has_issue_reference(self) -> None:
        self.assertTrue(has_issue_reference("Handles #9."))
        self.assertFalse(has_issue_reference("No issue here."))

    def test_extract_issue_numbers(self) -> None:
        self.assertEqual(
            extract_issue_numbers("Fixes #12 and refs #7."),
            ["12", "7"],
        )
        self.assertEqual(extract_issue_numbers("No refs."), [])

    def test_issue_has_milestone(self) -> None:
        self.assertFalse(issue_has_milestone({"milestone": None}))
        self.assertFalse(issue_has_milestone({}))
        self.assertTrue(issue_has_milestone({"milestone": {"title": "v0.1.5"}}))

    def test_main_requires_issue_reference(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            event_path = Path(tmp_dir) / "event.json"
            payload = {
                "pull_request": {"title": "No issue", "body": ""},
            }
            event_path.write_text(
                json.dumps(payload), encoding="utf-8"
            )
            env_backup = os.environ.get("GITHUB_EVENT_PATH")
            os.environ["GITHUB_EVENT_PATH"] = str(event_path)
            try:
                self.assertEqual(main([]), 1)
            finally:
                if env_backup is None:
                    os.environ.pop("GITHUB_EVENT_PATH", None)
                else:
                    os.environ["GITHUB_EVENT_PATH"] = env_backup


if __name__ == "__main__":
    unittest.main()
