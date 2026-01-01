# TOON Output Format

ProjectAtlas writes a `atlas.toon` snapshot with these sections:

```
version: 1
generated_at: 2026-01-01T12:00:00Z
file_hash: "..."
folder_hash: "..."
root: .
overview: tracked_files=12 tracked_folders=8 source_extensions=9 exclude_dir_names=6 exclude_path_prefixes=0
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
