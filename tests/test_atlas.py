"""
Purpose: Validate basic ProjectAtlas scanning behavior.
"""

from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from projectatlas.atlas import build_file_records, build_folder_records
from projectatlas.config import AtlasConfig


def make_config(root: Path) -> AtlasConfig:
    """Create a minimal config for unit tests."""
    return AtlasConfig(
        root=root,
        map_path=root / ".projectatlas" / "atlas.toon",
        manual_files_path=None,
        purpose_filename=".purpose",
        source_extensions={".py"},
        exclude_dir_names=set(),
        exclude_path_prefixes=set(),
        non_source_path_prefixes=set(),
        allowed_untracked_filenames={".purpose"},
        untracked_allowlist_dir_prefixes=set(),
        untracked_allowlist_files=set(),
        asset_allowed_prefixes=set(),
        asset_extensions=set(),
        max_scan_lines=80,
        summary_max_length=140,
        summary_ascii_only=True,
        summary_no_commas=True,
    )


class AtlasScanTests(unittest.TestCase):
    """Cover baseline Purpose extraction for files and folders."""

    def test_python_purpose_header(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            path = root / "sample.py"
            path.write_text(
                '"""\nPurpose: Sample module.\n"""\n',
                encoding="utf-8",
            )
            config = make_config(root)
            records, missing, invalid = build_file_records([path], config)
            self.assertEqual(missing, [])
            self.assertEqual(invalid, {})
            self.assertEqual(records[0].summary, "Sample module.")

    def test_folder_purpose(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            (root / ".purpose").write_text("Purpose: Root folder.\n", encoding="utf-8")
            config = make_config(root)
            records, missing, invalid = build_folder_records([root], config)
            self.assertEqual(missing, [])
            self.assertEqual(invalid, {})
            self.assertEqual(records[0].summary, "Root folder.")


if __name__ == "__main__":
    unittest.main()
