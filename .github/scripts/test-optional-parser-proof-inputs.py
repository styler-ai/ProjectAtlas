#!/usr/bin/env python3
"""Unit test for optional-parser release-proof input classification."""

import runpy
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
                    "openspec/changes/release/tasks.md",
                    "docs/workflow.md",
                    "AGENTS.md",
                ]
            ),
            {
                "reusable": True,
                "metadata_only": [
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
            ".github/scripts/unknown-construction-step.py",
            "unknown-release-input",
            r"docs\workflow.md",
        ):
            with self.subTest(path=path):
                self.assertEqual(classify([path])["invalidating"], [path])


if __name__ == "__main__":
    unittest.main()
