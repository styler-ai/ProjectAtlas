#!/usr/bin/env python3
"""Unit test for optional-parser release-proof input classification."""

import runpy
import subprocess
import unittest
from pathlib import Path


class OptionalParserProofInputsTests(unittest.TestCase):
    def test_metadata_reuses_proof_and_every_other_input_invalidates(self) -> None:
        classify = runpy.run_path(
            Path(__file__).with_name("optional-parser-proof-inputs.py")
        )["classify_paths"]
        self.assertEqual(
            classify(
                [
                    ".github/scripts/issue-checklists.py",
                    "openspec/changes/release/tasks.md",
                    "docs/workflow.md",
                    "AGENTS.md",
                ]
            ),
            {
                "reusable": True,
                "metadata_only": [
                    ".github/scripts/issue-checklists.py",
                    "AGENTS.md",
                    "docs/workflow.md",
                    "openspec/changes/release/tasks.md",
                ],
                "invalidating": [],
            },
        )
        for path in (
            "Cargo.lock",
            "rust-toolchain.toml",
            "crates/projectatlas-cli/src/parser_worker.rs",
            "packaging/parser-pack/manifest.json",
            ".github/workflows/optional-parser-pack.yml",
            ".github/scripts/issue-checklists-helper.py",
            ".github/scripts/unknown-construction-step.py",
            "unknown-release-input",
            r"docs\workflow.md",
        ):
            with self.subTest(path=path):
                self.assertEqual(classify([path])["invalidating"], [path])

    def test_handoff_selection_reaches_an_eligible_run_on_a_later_page(self) -> None:
        select = runpy.run_path(
            Path(__file__).with_name("resolve-optional-parser-handoff.py")
        )["select_reusable_run"]
        newest = "a" * 40
        newer = "b" * 40
        eligible = "c" * 40
        promotion = "d" * 40
        pages = [
            {
                "workflow_runs": [
                    {"id": 301, "run_number": 301, "head_sha": newest},
                    {"id": 300, "run_number": 300, "head_sha": newer},
                ]
            },
            {
                "workflow_runs": [
                    {"id": 199, "run_number": 199, "head_sha": eligible}
                ]
            },
        ]

        def runner(arguments: list[str]) -> subprocess.CompletedProcess[str]:
            if arguments[:3] == ["git", "fetch", "--no-tags"]:
                return subprocess.CompletedProcess(arguments, 0, "", "")
            if "--base" in arguments:
                base = arguments[arguments.index("--base") + 1]
                return subprocess.CompletedProcess(
                    arguments, 0 if base == eligible else 1, "", ""
                )
            if "/actions/runs/199/artifacts" in arguments[-1]:
                return subprocess.CompletedProcess(
                    arguments,
                    0,
                    '{"artifacts":[{"name":"optional-parser-pack-release-assets",'
                    '"expired":false}]}',
                    "",
                )
            return subprocess.CompletedProcess(arguments, 0, '{"artifacts":[]}', "")

        self.assertEqual(select(pages, promotion, "owner/repo", runner), "199")

    def test_handoff_selection_rejects_malformed_commit_identity(self) -> None:
        select = runpy.run_path(
            Path(__file__).with_name("resolve-optional-parser-handoff.py")
        )["select_reusable_run"]
        with self.assertRaisesRegex(ValueError, "commit identity is malformed"):
            select([], "not-a-commit", "owner/repo", lambda _: None)


if __name__ == "__main__":
    unittest.main()
