# ProjectAtlas Configuration

ProjectAtlas reads `projectatlas.toml` or `.projectatlas/config.toml`. All paths are relative to the config file.

```toml
[project]
root = "."
map_path = ".projectatlas/projectatlas.toon"
nonsource_files_path = ".projectatlas/projectatlas-nonsource-files.toon"
purpose_filename = ".purpose"

[scan]
source_extensions = [
  ".py", ".pyw", ".js", ".jsx", ".ts", ".tsx", ".mjs", ".cjs", ".d.ts", ".java",
  ".c", ".cpp", ".h", ".hpp", ".cxx", ".cc", ".hxx", ".hh", ".cs", ".go",
  ".m", ".mm", ".rb", ".php", ".swift", ".kt", ".kts", ".rs", ".scala",
  ".sh", ".bash", ".zsh", ".ps1", ".psm1", ".psd1", ".bat", ".cmd", ".r",
  ".pl", ".pm", ".lua", ".dart", ".hs", ".ml", ".mli", ".fs", ".fsx",
  ".clj", ".cljs", ".vim", ".zig", ".zon", ".html", ".htm", ".css", ".scss",
  ".sass", ".less", ".stylus", ".styl", ".md", ".mdx", ".json", ".jsonc",
  ".xml", ".yml", ".yaml", ".toml", ".toon", ".txt", ".ini", ".cfg", ".conf", ".vue",
  ".svelte", ".astro", ".jsp", ".jspx", ".jspf", ".tag", ".tagx", ".gsp",
  ".properties", ".gradle", ".groovy", ".proto", ".hbs", ".handlebars", ".ejs",
  ".pug", ".ftl", ".mustache", ".liquid", ".erb", ".sql", ".ddl", ".dml",
  ".mysql", ".postgresql", ".psql", ".sqlite", ".mssql", ".oracle", ".ora",
  ".db2", ".proc", ".procedure", ".func", ".function", ".view", ".trigger",
  ".index", ".migration", ".seed", ".fixture", ".schema", ".cql", ".cypher",
  ".sparql", ".gql", ".liquibase", ".flyway"
]
exclude_dir_names = [".git", ".projectatlas", ".venv", "__pycache__", "node_modules", "dist", "build", "target"]
exclude_dir_suffixes = [".egg-info"]
exclude_path_prefixes = []
non_source_path_prefixes = []
max_scan_lines = 80
text_index_max_bytes = 2000000

[purpose]
default_style = "javadoc"
line_comment_prefixes = ["//", "#", "--", ";"]
# styles_by_extension = { ".go" = "line-comment", ".c" = "block-comment" }

[summary_rules]
ascii_only = true
no_commas = true
max_length = 140

[untracked]
allowed_filenames = [".purpose"]
allowlist_dir_prefixes = []
allowlist_files = []
asset_allowed_prefixes = []
asset_extensions = [".png", ".jpg", ".jpeg", ".svg", ".gif", ".webp", ".ico", ".pdf", ".ttf", ".woff", ".woff2"]
```

`projectatlas init` writes the Rust configuration template. Adjust `scan.source_extensions` only when a project needs a narrower or broader compatibility-map surface.

`scan.exclude_dir_names` and `scan.exclude_path_prefixes` are used by `projectatlas scan`, `projectatlas map`,
`projectatlas lint`, MCP `atlas_scan`, watcher refresh, and `strip-legacy-purpose`. Use directory-name excludes for
broad generated/vendor/build folders such as `node_modules` or `target`; use path-prefix excludes for exact
repository subtrees such as `docs/api` or `app/public/generated`. Search then operates over the indexed file set
and can use literal, regex, or fuzzy matching.

During migration from legacy TOON maps, `projectatlas scan` imports purpose records only for paths still present in
the freshly indexed file set. Stale or newly excluded map rows are counted as skipped stale imports instead of
failing the first scan with a low-level SQLite no-row error.

`scan.text_index_max_bytes` caps the size of each UTF-8 file stored in SQLite for indexed text search. Oversized
files remain indexed as repository nodes, but their full text is skipped for search to keep large repositories fast
and memory bounded. Use a higher value only when the repository needs indexed search inside large generated or data
files.

Path-like entries in scan and untracked configuration are repository-relative. Absolute paths, drive-prefixed
paths, root paths, and `..` traversal are rejected before ProjectAtlas performs existence checks or lint probes.

### Purpose styles

- `purpose.default_style` controls the fallback header style (`javadoc`, `block-comment`, or `line-comment`).
- `purpose.styles_by_extension` maps specific extensions to a style.
- `purpose.line_comment_prefixes` controls which line-comment prefixes are recognized.

Example:

```toml
[purpose]
default_style = "javadoc"
line_comment_prefixes = ["//", "#", "--", ";"]

[purpose.styles_by_extension]
".go" = "line-comment"
".rs" = "line-comment"
".c" = "block-comment"
```

### Non-source file list

If you set `project.nonsource_files_path`, ProjectAtlas reads a TOON file with a `nonsource_files[]:` section. This
file is agent-maintained input for non-source summaries (configs, docs, assets) and is merged into the generated
atlas. The legacy `project.manual_files_path` key is still accepted for backward compatibility.

```
nonsource_files[]:
  path/to/file.txt,One line purpose summary
```

These entries are merged into the file list for non-source or config files that cannot carry headers.
