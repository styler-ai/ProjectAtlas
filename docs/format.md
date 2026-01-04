# TOON Output Format

ProjectAtlas writes a `atlas.toon` snapshot with these sections:

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

- `tracked_source_files` counts files scanned for Purpose headers (source extensions).
- `tracked_nonsource_files` counts entries supplied by `projectatlas-nonsource-files.toon`.
- `tracked_files_total` is the combined total shown in `files[]`.
- The remaining fields (`tracked_folders`, `source_extensions`, `exclude_*`) describe the scan surface.

## Non-source list

The generated atlas merges the manual `.projectatlas/projectatlas-nonsource-files.toon` entries into `files[]`
so the snapshot stays complete without forcing headers into formats that cannot safely accept them.
