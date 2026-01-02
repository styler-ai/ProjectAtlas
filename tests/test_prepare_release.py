"""
Purpose: Validate the release preparation helper.
"""

from __future__ import annotations

import unittest

from scripts.prepare_release import (
    build_release_plan,
    compute_release_version,
    normalize_issue,
)


class PrepareReleaseTests(unittest.TestCase):
    """Cover release plan metadata helpers."""

    def test_normalize_issue(self) -> None:
        self.assertEqual(normalize_issue("23"), "#23")
        self.assertEqual(normalize_issue("#42"), "#42")

    def test_build_release_plan(self) -> None:
        release_version = compute_release_version("0.1.0.dev0", "patch")
        plan = build_release_plan(
            release_version=release_version,
            issue="23",
            base_branch="main",
            head_branch="release/v0.1.0",
        )
        self.assertEqual(plan.version, "0.1.0")
        self.assertEqual(plan.issue, "#23")
        self.assertIn("v0.1.0", plan.commit_message)
        self.assertIn("v0.1.0", plan.pr_title)
        self.assertIn("Prepare v0.1.0", plan.pr_body)

    def test_compute_release_version(self) -> None:
        self.assertEqual(compute_release_version("0.1.0.dev0", "patch"), "0.1.0")
        self.assertEqual(compute_release_version("0.1.0", "patch"), "0.1.1")


if __name__ == "__main__":
    unittest.main()
