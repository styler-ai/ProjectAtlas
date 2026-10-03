# Purpose: Document ProjectAtlas TOON and JSON map response formats.

# TOON Output Format

`projectatlas map` returns the current map with these sections on stdout in TOON by default, or JSON when global `--format json` is selected. MCP `atlas_map` returns the selected format in its `content` field (`json: true` selects JSON). The response is limited to 4 MiB; an oversized map returns a typed refusal instead of partial content. ProjectAtlas 3's durable source of truth is `.projectatlas/projectatlas.db`.

`generated_at` is the time of each render, not a content revision; identical map content can have different timestamps across calls. Compare `file_hash` and `folder_hash` to detect content changes.

Map rendering never creates, overwrites, or deletes `.projectatlas/projectatlas.toon` (or an alternate configured `map_path`). A pre-existing TOON file remains a read-only legacy purpose-import input. Map-local `--json` independently writes an adjacent JSON sidecar outside CI, or with `--force` in CI; global `--format json` does not imply a sidecar. `.projectatlas/projectatlas-nonsource-files.toon` remains a separate authored input. Agent-facing TOON fixtures are decoded with the `toon-format` crate in tests; map rows retain escaping/round-trip coverage.

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
  src/main.py,Python application entry point,header
folder_summary_duplicates[]:
  - Shared utils :: src/utils | app/utils
file_summary_duplicates[]:
  - Shared helpers :: src/helpers.py | app/helpers.py
folder_tree[]:
  - . - Project root
  - src/ - Application source
```

The section layout is stable so agents can scan quickly; tooling comparing content should use the hashes rather than `generated_at`.

## Overview fields

- `tracked_source_files` counts indexed source files.
- `tracked_nonsource_files` counts imported non-source summary entries and indexed non-source records.
- `tracked_files_total` is the combined total shown in `files[]`.
- The remaining fields (`tracked_folders`, `source_extensions`, `exclude_*`) describe the scan surface.

## Non-source list

The returned map can merge `.projectatlas/projectatlas-nonsource-files.toon` entries into `files[]`
for compatibility. New ProjectAtlas 3 workflows should prefer SQLite `folder_purpose` and
`file_purpose` records, plus deterministic `content_summary` values from `projectatlas summary`
or `atlas_file_summary`.
