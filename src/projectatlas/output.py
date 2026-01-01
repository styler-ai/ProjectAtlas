"""
Purpose: Write ProjectAtlas snapshots to TOON or JSON outputs.
"""

from __future__ import annotations

import json

from projectatlas.config import AtlasConfig
from projectatlas.models import AtlasSnapshot
from projectatlas.atlas import format_overview


def to_toon(snapshot: AtlasSnapshot, config: AtlasConfig) -> str:
    """Serialize the snapshot to TOON."""
    lines: list[str] = []
    lines.append("version: 1")
    lines.append(f"generated_at: {snapshot.generated_at}")
    lines.append(f'file_hash: "{snapshot.file_hash}"')
    lines.append(f'folder_hash: "{snapshot.folder_hash}"')
    lines.append("root: .")
    lines.append(format_overview(snapshot.overview))
    lines.append("source_extensions[]:")
    for ext in sorted(config.source_extensions):
        lines.append(f"  - {ext}")
    lines.append("exclude_dir_names[]:")
    for name in sorted(config.exclude_dir_names):
        lines.append(f"  - {name}")
    lines.append("exclude_path_prefixes[]:")
    for prefix in sorted(config.exclude_path_prefixes):
        lines.append(f"  - {prefix}")
    lines.append(
        f"folders[{len(snapshot.folder_records)}]{{path,summary,source}}:"
    )
    for record in snapshot.folder_records:
        lines.append(f"  {record.path},{record.summary},{record.source}")
    lines.append(
        f"files[{len(snapshot.file_records)}]{{path,summary,source}}:"
    )
    for record in snapshot.file_records:
        lines.append(f"  {record.path},{record.summary},{record.source}")
    lines.append("folder_summary_duplicates[]:")
    for entry in snapshot.folder_duplicates:
        lines.append(f"  - {entry}")
    lines.append("file_summary_duplicates[]:")
    for entry in snapshot.file_duplicates:
        lines.append(f"  - {entry}")
    lines.append("folder_tree[]:")
    for entry in snapshot.folder_tree:
        lines.append(f"  - {entry}")
    return "\n".join(lines) + "\n"


def write_toon(snapshot: AtlasSnapshot, config: AtlasConfig) -> None:
    """Write the snapshot to the configured TOON path."""
    config.map_path.parent.mkdir(parents=True, exist_ok=True)
    config.map_path.write_text(to_toon(snapshot, config), encoding="utf-8")


def write_json(snapshot: AtlasSnapshot, config: AtlasConfig) -> None:
    """Write the snapshot to a JSON file next to the TOON output."""
    payload = {
        "version": 1,
        "generated_at": snapshot.generated_at,
        "file_hash": snapshot.file_hash,
        "folder_hash": snapshot.folder_hash,
        "root": ".",
        "overview": snapshot.overview,
        "source_extensions": sorted(config.source_extensions),
        "exclude_dir_names": sorted(config.exclude_dir_names),
        "exclude_path_prefixes": sorted(config.exclude_path_prefixes),
        "folders": [
            {"path": rec.path, "summary": rec.summary, "source": rec.source}
            for rec in snapshot.folder_records
        ],
        "files": [
            {"path": rec.path, "summary": rec.summary, "source": rec.source}
            for rec in snapshot.file_records
        ],
        "folder_summary_duplicates": list(snapshot.folder_duplicates),
        "file_summary_duplicates": list(snapshot.file_duplicates),
        "folder_tree": list(snapshot.folder_tree),
    }
    json_path = config.map_path.with_suffix(".json")
    json_path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")
