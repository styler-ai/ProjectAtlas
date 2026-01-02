"""
Purpose: Validate the release preparation helper.
"""

from __future__ import annotations

import unittest

from scripts.prepare_release import build_release_plan, normalize_issue


class PrepareReleaseTests(unittest.TestCase):
    """Cover release plan metadata helpers."""

    def test_normalize_issue(self) -> None:
        self.assertEqual(normalize_issue("23"), "#23")
        self.assertEqual(normalize_issue("#42"), "#42")

    def test_build_release_plan(self) -> None:
        plan = build_release_plan(
            current_version="0.1.0.dev0",
            bump="patch",
            issue="23",
            base_branch="main",
            head_branch="dev",
        )
        self.assertEqual(plan.version, "0.1.0")
        self.assertEqual(plan.issue, "#23")
        self.assertIn("v0.1.0", plan.commit_message)
        self.assertIn("v0.1.0", plan.pr_title)
        self.assertIn("Prepare v0.1.0", plan.pr_body)

        plan_release = build_release_plan(
            current_version="0.1.0",
            bump="patch",
            issue="23",
            base_branch="main",
            head_branch="dev",
        )
        self.assertEqual(plan_release.version, "0.1.1")


if __name__ == "__main__":
    unittest.main()
