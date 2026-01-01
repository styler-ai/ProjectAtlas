"""
Purpose: Validate commit message issue reference checks.
"""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scripts.check_commit_issue import commit_has_issue, main


class CommitIssueTests(unittest.TestCase):
    """Cover commit message issue reference enforcement."""

    def test_commit_has_issue(self) -> None:
        self.assertTrue(commit_has_issue("Fix: handle edge case (#12)"))
        self.assertTrue(commit_has_issue("Closes #123"))
        self.assertFalse(commit_has_issue("No issue reference"))

    def test_main_requires_issue(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            path = Path(tmp_dir) / "msg.txt"
            path.write_text("No issue reference", encoding="utf-8")
            self.assertEqual(main([str(path)]), 1)


if __name__ == "__main__":
    unittest.main()
