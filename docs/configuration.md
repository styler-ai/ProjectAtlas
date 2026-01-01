# ProjectAtlas Configuration

ProjectAtlas reads `projectatlas.toml` or `.projectatlas/config.toml`. All paths are relative to the config file.

```toml
[project]
root = "."
map_path = ".projectatlas/atlas.toon"
manual_files_path = ".projectatlas/manual-files.toon"
purpose_filename = ".purpose"

[scan]
source_extensions = [".py", ".js", ".ts", ".tsx", ".jsx", ".vue", ".css", ".mjs", ".cjs", ".d.ts"]
exclude_dir_names = [".git", ".projectatlas", ".venv", "__pycache__", "node_modules", "dist", "build"]
exclude_path_prefixes = []
non_source_path_prefixes = []
max_scan_lines = 80

[summary_rules]
ascii_only = true
no_commas = true
max_length = 140

[untracked]
allowed_filenames = [".purpose"]
allowlist_dir_prefixes = []
allowlist_files = []
asset_allowed_prefixes = []
asset_extensions = [".png", ".jpg", ".jpeg", ".svg", ".gif", ".webp", ".ico", ".pdf"]
```

### Manual files

If you set `project.manual_files_path`, ProjectAtlas reads a TOON file with a `manual_files[]:` section:

```
manual_files[]:
  path/to/file.txt,One line purpose summary
```

These entries are merged into the file list for non-source or config files that cannot carry headers.
