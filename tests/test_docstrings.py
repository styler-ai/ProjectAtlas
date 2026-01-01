"""
Purpose: Validate the ProjectAtlas docstring enforcement script.
"""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scripts.check_docstrings import collect_docstring_issues


class DocstringCheckTests(unittest.TestCase):
    """Cover docstring enforcement for public symbols."""

    def test_all_docstrings_present(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            src = root / "src" / "projectatlas"
            src.mkdir(parents=True, exist_ok=True)
            file_path = src / "sample.py"
            file_path.write_text(
                '"""\nPurpose: Sample module.\n"""\n\n'
                "class Public:\n"
                '    """Public class."""\n'
                "    def method(self):\n"
                '        """Docstring."""\n'
                "        return 1\n\n"
                "def public_fn():\n"
                '    """Docstring."""\n'
                "    return 2\n",
                encoding="utf-8",
            )
            issues = collect_docstring_issues(root, ("src/projectatlas",))
            self.assertEqual(issues, [])

    def test_missing_docstrings(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            src = root / "src" / "projectatlas"
            src.mkdir(parents=True, exist_ok=True)
            file_path = src / "sample.py"
            file_path.write_text(
                '"""\nPurpose: Sample module.\n"""\n\n'
                "class Public:\n"
                "    def method(self):\n"
                "        return 1\n\n"
                "def public_fn():\n"
                "    return 2\n",
                encoding="utf-8",
            )
            issues = collect_docstring_issues(root, ("src/projectatlas",))
            self.assertTrue(issues)


if __name__ == "__main__":
    unittest.main()
