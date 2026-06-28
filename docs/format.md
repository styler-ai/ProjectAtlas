# TOON Output Format

ProjectAtlas writes a compatibility TOON snapshot at `.projectatlas/projectatlas.toon` with these sections. ProjectAtlas 3's durable source of truth is `.projectatlas/projectatlas.db`; TOON is the compact agent/export format.

The map snapshot is a stable ProjectAtlas compatibility artifact used by legacy map/lint/import workflows. Agent-facing CLI/MCP payloads use official TOON-compatible text, and TOON fixtures are decoded with the `toon-format` crate in tests. The compatibility map writer keeps a local row reader/writer only to preserve backward-compatible atlas snapshots and must keep escaping/round-trip coverage when the row schema changes.

```
version: 1
generated_at: 2026-01-01T12:00:00Z
file_hash: "..."
folder_hash: "..."
root: .
overview: tracked_source_files=12 tracked_nonsource_files=4 tracked_files_total=16 tracked_folders=8 source_extensions=9 exclude_dir_names=6 exclude_path_prefixes=0
source_extensions[]:
  - .py
exclude_dir_names[]:
  - .git
exclude_path_prefixes[]:
  - docs/generated
folders[3]{path,summary,source}:
  .,Project root,purpose
files[4]{path,summary,source}:
  src/main.py,Main entry,header
folder_summary_duplicates[]:
  - Shared utils :: src/utils | app/utils
file_summary_duplicates[]:
  - Shared helpers :: src/helpers.py | app/helpers.py
folder_tree[]:
  - . - Project root
  - src/ - Application source
```

Sections are stable so agents can scan quickly and tooling can diff.

## Overview fields

- `tracked_source_files` counts indexed source files.
- `tracked_nonsource_files` counts imported non-source summary entries and indexed non-source records.
- `tracked_files_total` is the combined total shown in `files[]`.
- The remaining fields (`tracked_folders`, `source_extensions`, `exclude_*`) describe the scan surface.

## Non-source list

The generated atlas can merge `.projectatlas/projectatlas-nonsource-files.toon` entries into `files[]`
for compatibility. New ProjectAtlas 3 workflows should prefer SQLite purpose records and structured summaries from `projectatlas summary` / `atlas_file_summary`.
