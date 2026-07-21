# Purpose: Explain ProjectAtlas purpose metadata source of truth and health concepts.

# Concepts

ProjectAtlas is a Rust-native way to keep structural intent and source intelligence visible to coding agents without polluting product folders or source files.

## SQLite Purpose Records

ProjectAtlas 3 stores folder and file purposes in `.projectatlas/projectatlas.db`.
Each project has its own database under the project root. Folder purpose and file purpose are different records:

- A folder purpose describes the folder's structural responsibility.
- A file purpose describes why that file exists inside its folder.

Missing purposes are health/lint findings. Agents should inspect enough context to set a correct one-line purpose with `projectatlas purpose set` or the MCP `atlas_purpose_set` tool. Folder purposes should be curated broadly; file purposes should be curated selectively for current-task, public API, build/config, workflow, test, runtime, route, migration, command, MCP, or trusted metadata paths whose recorded responsibility is inconsistent.

## Summaries

Summaries are not purposes. A summary describes what the index observes in a file: language, line count, dependencies, imports, functions, methods, classes/types, calls, and line ranges where available.
Use `projectatlas summary <file> --limit 25` or `atlas_file_summary` before opening full source.

Generated file-purpose guesses may be stored as suggestions, but they remain `agent_reviewed=false` until an agent approves or corrects them.

An accepted purpose is durable authored responsibility state. Scans and watch refreshes update `content_summary` and other derived facts without demoting, invalidating, or overwriting that purpose. The legacy `stale` purpose status remains readable for wire/schema compatibility and is normalized to `approved` during migration; normal source, hash, summary, symbol, and graph changes do not create it. An agent or user may still correct an accepted purpose explicitly after finding a mistake, inconsistency, or genuine repurposing.

## Legacy metadata

Legacy `.purpose` files, source `Purpose:` headers, and `.projectatlas/projectatlas-nonsource-files.toon` remain import/migration sources. They are not the final ProjectAtlas 3 storage model.

The compatibility map at `.projectatlas/projectatlas.toon` is an optional exported snapshot for older workflows; it should not be committed as the agent source of truth. The SQLite database is the durable source of truth.

## Health signals

ProjectAtlas surfaces:

- missing or suggested-but-unapproved purposes
- duplicate or overlapping approved purposes across files or folders
- untracked assets outside approved roots
- repeated temporary/generated folder roles
- legacy stale-purpose records, stale index, or structure drift signals

These signals are meant to prompt cleanup before the structure drifts.
