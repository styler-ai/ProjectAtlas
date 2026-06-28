# Concepts

ProjectAtlas is a Rust-native way to keep structural intent and source intelligence visible to coding agents without polluting product folders or source files.

## SQLite Purpose Records

ProjectAtlas 3 stores folder and file purposes in `.projectatlas/projectatlas.db`.
Each project has its own database under the project root. Folder purpose and file purpose are different records:

- A folder purpose describes the folder's structural responsibility.
- A file purpose describes why that file exists inside its folder.

Missing purposes are health/lint findings. Agents should inspect enough context to set a correct one-line purpose with `projectatlas purpose set` or the MCP `atlas_purpose_set` tool.

## Summaries

Summaries are not purposes. A summary describes what the index observes in a file: language, line count, dependencies, imports, functions, methods, classes/types, calls, and line ranges where available.
Use `projectatlas summary <file> --limit 25` or `atlas_file_summary` before opening full source.

Generated file-purpose guesses may be stored as suggestions, but they remain review-required until an agent approves or corrects them.

## Legacy metadata

Legacy `.purpose` files, source `Purpose:` headers, and `.projectatlas/projectatlas-nonsource-files.toon` remain import/migration sources. They are not the final ProjectAtlas 3 storage model.

The compatibility map at `.projectatlas/projectatlas.toon` is an exported snapshot for older workflows and quick diffs; the SQLite database is the durable source of truth.

## Health signals

ProjectAtlas surfaces:

- missing or suggested-but-unapproved purposes
- duplicate or overlapping approved purposes across files or folders
- untracked assets outside approved roots
- repeated temporary/generated folder roles
- stale index or structure drift signals

These signals are meant to prompt cleanup before the structure drifts.
