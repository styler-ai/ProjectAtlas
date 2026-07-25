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

## Derived graph snapshots

`projectatlas snapshot export <archive.tar.zst>` creates a portable graph accelerator for the current exact source state. The exported archive is rebuilt from an explicit derived-only allowlist; it does not copy the live SQLite pages or carry project identity, purposes, health resolutions, telemetry, settings, future Memory Atlas rows, machine-local roots, or deleted/free-page remnants.

`projectatlas snapshot import <archive.tar.zst>` validates the archive root, entry types and paths, compression and expansion limits, inventory, schema/runtime, content digests, source-state identity, and capability fingerprint before replacing graph rows through the normal atomic projection publication. The destination must already be a current index of the same source state. Its project identity and authored state remain authoritative.

Local export/import is unsigned by default. `--require-digest <blake3>` provides an explicit content trust pin. Builds that opt into the `derived-snapshot-signatures` feature also support `--signing-key <secret-key-file>` on export and `--trusted-public-key <public-key-file>` on import; trusted imports reject missing, invalid, or differently signed archives before publication.

## Health signals

ProjectAtlas surfaces:

- missing or suggested-but-unapproved purposes
- duplicate or overlapping approved purposes across files or folders
- untracked assets outside approved roots
- repeated temporary/generated folder roles
- legacy stale-purpose records, stale index, or structure drift signals

These signals are meant to prompt cleanup before the structure drifts.
