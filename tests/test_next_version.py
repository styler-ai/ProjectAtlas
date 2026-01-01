"""
Purpose: Validate ProjectAtlas version helper utilities.
"""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scripts.next_version import (
    build_version,
    bump_base_version,
    update_version_files,
)


class NextVersionTests(unittest.TestCase):
    """Cover version bump helpers and file updates."""

    def test_bump_base_version(self) -> None:
        self.assertEqual(bump_base_version("0.1.0.dev0", "patch"), "0.1.1")
        self.assertEqual(bump_base_version("0.1.0", "minor"), "0.2.0")
        self.assertEqual(bump_base_version("0.1.0", "major"), "1.0.0")

    def test_build_version(self) -> None:
        self.assertEqual(build_version("0.2.0", False), "0.2.0")
        self.assertEqual(build_version("0.2.0", True), "0.2.0.dev0")

    def test_update_version_files(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            pyproject = root / "pyproject.toml"
            init_path = root / "src" / "projectatlas" / "__init__.py"
            init_path.parent.mkdir(parents=True, exist_ok=True)
            pyproject.write_text(
                "[project]\nversion = \"0.1.0\"\n",
                encoding="utf-8",
            )
            init_path.write_text(
                '__version__ = "0.1.0"\n',
                encoding="utf-8",
            )
            update_version_files(root, "0.2.0")
            self.assertIn("0.2.0", pyproject.read_text(encoding="utf-8"))
            self.assertIn("0.2.0", init_path.read_text(encoding="utf-8"))


if __name__ == "__main__":
    unittest.main()
