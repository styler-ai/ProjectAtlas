"""
Purpose: Define data models used across ProjectAtlas.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Sequence


@dataclass(frozen=True)
class Record:
    """Represent a ProjectAtlas record entry."""

    path: str
    summary: str
    source: str


@dataclass(frozen=True)
class AtlasSnapshot:
    """Capture the full ProjectAtlas snapshot to write."""

    folder_records: Sequence[Record]
    file_records: Sequence[Record]
    folder_tree: Sequence[str]
    folder_duplicates: Sequence[str]
    file_duplicates: Sequence[str]
    file_hash: str
    folder_hash: str
    generated_at: str
    overview: dict[str, int]
