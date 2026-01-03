"""
Purpose: Validate automatic language detection for config generation.
"""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from projectatlas.config import build_config_text, detect_language_extensions


class LanguageDetectionTests(unittest.TestCase):
    """Cover language detection and config rendering."""

    def test_detects_extensions_and_skips_excluded_dirs(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            (root / "src").mkdir(parents=True)
            (root / "node_modules").mkdir(parents=True)
            (root / "src" / "main.go").write_text("package main\n", encoding="utf-8")
            (root / "node_modules" / "ignore.js").write_text(
                "console.log('skip');\n",
                encoding="utf-8",
            )
            detected = detect_language_extensions(root)
            self.assertIn(".go", detected)
            self.assertNotIn(".js", detected)

    def test_build_config_text_includes_styles(self) -> None:
        config_text = build_config_text(
            source_extensions=[".go"],
            purpose_styles={".go": "line-comment"},
        )
        self.assertIn('source_extensions = [".go"]', config_text)
        self.assertIn("[purpose.styles_by_extension]", config_text)
        self.assertIn('"\\.go" = "line-comment"'.replace("\\", ""), config_text)


if __name__ == "__main__":
    unittest.main()
