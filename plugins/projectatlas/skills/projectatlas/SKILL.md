---
name: projectatlas
description: Use ProjectAtlas as the atlas-first orientation layer before broad source reads, and run ProjectAtlas CLI/MCP scan, overview, folder/file, outline, symbols, slice, health, lint, and token commands.
---

# ProjectAtlas

## Goal

Use ProjectAtlas to open the agent's eyes in large repositories before expensive context operations. The workflow is:
scan, overview, folders, files, compressed outline or symbols, exact source.

ProjectAtlas is for the agent harness, not for human-facing documentation. Its job is to provide an atlas of the project so the agent knows where to look and does not repeatedly open every folder and file. Token reduction comes from using the atlas first and escalating only when needed.

The original ProjectAtlas goal has not changed: every important folder and file should have a one-line purpose so the agent can understand repository structure, navigate quickly, detect drift, and choose the right file before deep indexing. ProjectAtlas 3 keeps that goal and adds Rust speed, broader language support, a SQLite atlas database, MCP tools, and an improved deep code index.

ProjectAtlas exists to give the agent an atlas of the entire project:

1. First understand where things live, which folders exist, and what purpose each folder has.
2. Then understand which files live inside the selected folder, each file's purpose, and a one-line high-level content summary.
3. Then, only after the folder and file are known, go deeper into file outlines, classes, methods, functions, imports, calls, and exact slices.

The atlas update order is always:

1. Update folders and folder purposes.
2. Update files, file purposes, and one-line high-level file summaries.
3. Update the deep code index: outlines, symbols, relations, and exact slice metadata.

Purpose and summary are separate:

- Purpose answers why a folder or file exists.
- Summary answers what currently appears to be inside it.
- Generated summaries are acceptable as deterministic observed metadata.
- Generated purposes are only suggestions and must not be treated as correct until imported from trusted legacy metadata or approved by the agent after inspection.
- If lint or health reports missing purposes, the agent must inspect the folder/file enough to write a correct one-line purpose and call `atlas_purpose_set` or `projectatlas purpose set`; do not leave the purpose blank just because no human supplied it.

## Purpose Completion Loop

When `atlas_health`, `projectatlas health-check`, or `projectatlas lint` reports missing purposes:

1. Use `atlas_folders`/`atlas_files` to locate the missing path.
2. Inspect only enough context to understand the path's actual role.
3. Write a precise one-line purpose with `atlas_purpose_set` or `projectatlas purpose set`.
4. Run `atlas_watch_once`, `projectatlas watch --once`, or `projectatlas scan`.
5. Rerun health/lint.
6. Repeat until the ProjectAtlas database has purposes for all indexed folders/files and the deep index is refreshed.

This loop is an agent responsibility installed with the plugin. Do not wait for human purpose text during normal agent-harness operation.

If `atlas_health` reports a duplicate-purpose, repeated temporary folder, or similar deterministic conflict that is correct after inspection, resolve that exact finding with `atlas_health_resolve` or:

```bash
projectatlas health resolve <finding-id> <category> <path> --related-path <path> --rationale "<why this is intentionally correct>"
```

Do not resolve missing-purpose findings; fill the purpose instead.

ProjectAtlas MCP uses the standard MCP JSON-RPC envelope. Tool result text is TOON by default.

This skill is part of the ProjectAtlas plugin on purpose. Installing the plugin should give the agent:

- this skill as the workflow and decision manual,
- `plugins/projectatlas/.mcp.json` as the MCP server registration,
- native runtime installer scripts under `plugins/projectatlas/scripts/`,
- `projectatlas mcp-config` plus generated `.projectatlas/projectatlas.mcp.json` for absolute MCP paths,
- `projectatlas mcp` as the executable MCP server,
- TOON-first tool responses from all `atlas_*` tools.

## When To Use

- At the start of work in a repo that already has `.projectatlas/config.toml`.
- When adopting ProjectAtlas in a new repository.
- After creating, moving, or deleting folders.
- After adding new source files.
- Before large refactors or cleanup decisions where folder/file intent matters.
- When the user asks for token savings from ProjectAtlas.

## First-Time Setup

1. Establish the project root first. If the workspace root is unambiguous, use it; otherwise ask the user once. Do not use one global ProjectAtlas database for unrelated projects.
2. Run all setup commands from that root so the default index is `<project-root>/.projectatlas/projectatlas.db`.
3. Confirm the native runtime is ProjectAtlas 3 with `projectatlas --format json runtime-info`; the report must identify project `ProjectAtlas`, major version 3 or newer, capability `mcp`, and text format `TOON`.
4. If the command is missing or resolves to an older non-ProjectAtlas-3 wrapper, run the plugin runtime installer from the target project root or pass the project root explicitly. The installer verifies the stable `runtime-info` contract, uses a local ProjectAtlas source checkout when present, otherwise downloads the release tag derived from the plugin manifest for the platform, then falls back to `cargo install --git https://github.com/styler-ai/ProjectAtlas --tag <plugin-release-tag> --package projectatlas-cli --locked`. It writes `.projectatlas/projectatlas.mcp.json` with absolute MCP paths:
   - Windows: `plugins/projectatlas/scripts/install-runtime.ps1`
   - Linux/macOS: `plugins/projectatlas/scripts/install-runtime.sh`
5. Confirm MCP registration uses the generated `.projectatlas/projectatlas.mcp.json` whenever possible. It contains absolute runtime, DB, and config paths plus a `cwd` project-root hint. `mcp-config` discovers `.projectatlas/config.toml` and flat `projectatlas.toml` from the selected DB/project root. The MCP server also resolves path-less root-sensitive tools from config, indexed DB metadata, or the default `.projectatlas/projectatlas.db` parent, so hosts that ignore `cwd` still use the intended project. The fallback plugin `.mcp.json` starts `projectatlas --db .projectatlas/projectatlas.db mcp` from PATH and should only be used from the project root when PATH resolves to the verified ProjectAtlas 3 binary.
6. Initialize the target repo with `projectatlas init --seed-purpose`.
7. Check `.projectatlas/config.toml` and add generated/vendor/build-heavy directories to `[scan].exclude_dir_names` before large-repo indexing.
8. Run `projectatlas scan`.
9. Add or import one-line purpose records for important folders and files.
10. Add summaries for non-source files to `.projectatlas/projectatlas-nonsource-files.toon` when needed.
11. Run `projectatlas map --force`.
12. Run `projectatlas lint --strict-folders --report-untracked` and fix every reported issue.

## MCP Tool Workflow

Use the MCP tools when the harness exposes them. They are preferred over shell commands because they keep the agent in the atlas-first path and return TOON text payloads directly.

1. `atlas_scan` when the index may be stale or after file/folder changes.
2. `atlas_overview` at startup to understand repository size and purpose coverage.
3. `atlas_folders` with the task query to choose the right work area.
4. `atlas_files` with the task query and selected folder to pick target files; add `file_pattern` when you already know the filename/path glob.
5. `atlas_file_summary` for structured file facts: purpose state, observed summary, imports/dependencies, functions, methods, classes/types, calls, and line ranges.
6. `atlas_outline` for a compact line-level file outline when the summary is not enough.
7. `atlas_symbols` and `atlas_symbol_relations` when function/class/import/call context is needed.
8. `atlas_search` for filtered literal, regex, or fuzzy matches inside indexed files.
9. `atlas_slice` for exact line or symbol source after folder/file/symbol selection.
10. `atlas_health` before cleanup, refactor, or DRY decisions.
11. `atlas_watch_once` after file changes when a continuous watcher is not running.
12. `atlas_token_report` when the user asks how many tokens ProjectAtlas saved.
13. `atlas_settings` and `atlas_watch_status` for diagnostics.
14. `atlas_reset_index` dry-run first when the local SQLite/cache state is corrupt or intentionally discarded.
15. `atlas_strip_legacy_purpose` only after migrated `.purpose` metadata is safely stored in SQLite.
16. `atlas_purpose_set` when an agent-approved purpose should be written to the durable index.
17. `atlas_health_resolve` when a deterministic conflict is intentionally correct and should not be repeated.

## Command Decision Rules

- Start of any non-trivial repo task: call `atlas_scan` if the index may be stale, otherwise call `atlas_overview`.
- New session after scan: call `atlas_overview`, then `atlas_folders` with the task terms.
- Choosing where to work: call `atlas_folders` before `atlas_files`; do not jump directly to broad source reads.
- Choosing source targets: call `atlas_files` with the selected folder and task terms; add `file_pattern` for exact glob discovery such as `*.rs` or `src/**/*.ts`.
- Need structured file-level context: call `atlas_file_summary` before opening a full file.
- Need compact line-level context: call `atlas_outline` after `atlas_file_summary` when the summary is not enough.
- Need API/function/class/module context: call `atlas_symbols` for declarations and `atlas_symbol_relations` for imports, calls, dependencies, and containment.
- Need exact code: call `atlas_slice` only after the folder, file, and range or symbol are known; pass symbol parent, kind, or line when duplicate symbol names exist.
- Need text occurrences: call `atlas_search` with `file_pattern`, `context_lines`, and `limit` rather than broad shell search; search is intentionally case-insensitive by default for agent discovery, set `case_sensitive` only when exact casing matters, set `fuzzy` when the name is approximate, and treat `truncated`, searched file count, and searched byte count as the signal for whether to narrow or widen the glob.
- After creating, moving, deleting, or editing files: call `atlas_watch_once`, `projectatlas watch --once`, or `atlas_scan` before trusting old results.
- During a long local editing session: prefer a single continuous `projectatlas watch` process from the project root, then use MCP reads against the refreshed SQLite index. File edits refresh incrementally; directory/root/ignore-rule changes may trigger a full scan for correctness.
- Planning cleanup/refactor/DRY work: call `atlas_health` after overview/folder/file orientation and before proposing moves/merges.
- Intentional health conflict after inspection: call `atlas_health_resolve` with a rationale.
- User asks about saved tokens: call `atlas_token_report`.
- Runtime looks wrong: call `projectatlas --format json runtime-info`, then `atlas_settings` and `atlas_watch_status`.
- Local index/cache is corrupt or intentionally discarded: call `atlas_reset_index` dry-run first; apply only when rebuilding from source is acceptable.
- Read-only review or CI smoke must not mutate telemetry: set `PROJECTATLAS_NO_TELEMETRY=1` before running ProjectAtlas CLI commands or launching the MCP server.
- Migrating old metadata: call `atlas_scan` first, then `atlas_strip_legacy_purpose` with dry-run; apply only on explicit user request.

When MCP registration files are needed from the CLI, generate them with:

```bash
projectatlas --format json --db .projectatlas/projectatlas.db mcp-config
```

This emits a `.mcp.json`-compatible document with the absolute `projectatlas` executable path, selected project database, optional config path, and project-root `cwd` hint. `projectatlas --format json runtime-info` is the read-only compatibility probe; it must not create `.projectatlas` by itself.

If MCP tools are unavailable, use the equivalent CLI sequence:

| Situation | CLI command |
| --- | --- |
| Refresh state | `projectatlas scan` |
| Overview | `projectatlas overview` |
| Folder selection | `projectatlas folders <query>` |
| File selection | `projectatlas files <query> --folder <path>` |
| Glob file discovery | `projectatlas files --file-pattern <glob>` |
| Structured file summary | `projectatlas summary <file> --limit <n>` |
| File context | `projectatlas outline <file>` |
| Symbols | `projectatlas symbols list --file <file>` |
| Relations | `projectatlas symbols relations --file <file>` |
| Search | `projectatlas search <pattern> --file-pattern <glob> --context-lines <n>` or `projectatlas search <pattern> --fuzzy --file-pattern <glob>` |
| Exact lines | `projectatlas slice <file> --start-line <n> --end-line <m>` |
| Exact symbol | `projectatlas symbols slice <file> <symbol> --symbol-parent <parent>` |
| Refresh after edits | `projectatlas watch --once` |
| Continuous local refresh | `projectatlas watch` |
| Cleanup/refactor signals | `projectatlas health-check` |
| Token savings | `projectatlas token` |
| Human token dashboard | `projectatlas token --view tui` |
| Diagnostics | `projectatlas settings` and `projectatlas watch-status` |
| Reset local index/cache | `projectatlas reset-index --dry-run` then `projectatlas reset-index --apply` |

## Startup Workflow

1. Establish the project root and run ProjectAtlas from that root.
2. Run `projectatlas scan` or `projectatlas map --force` when the index may be stale.
3. Run `projectatlas overview`.
4. Run `projectatlas folders <query>` to choose the right part of the repo.
5. Run `projectatlas files <query> --folder <path>` to select targets; use `projectatlas files --file-pattern <glob>` when the file/path pattern is already known.
6. Run `projectatlas summary <file> --limit 25` before opening full source.
7. Run `projectatlas outline <file>` if the structured summary is not enough.
8. Run `projectatlas symbols list --file <file>` and `projectatlas symbols relations --file <file>` when symbol context is needed.
9. Run `projectatlas search <pattern> --file-pattern <glob>` for bounded filtered text matches; add `--fuzzy` when the name is approximate, and inspect returned, searched file, searched byte, and truncated counters before widening the search.
10. Run `projectatlas slice <file> --start-line <n> --end-line <m>` or `projectatlas symbols slice <file> <symbol> --symbol-parent <parent> --symbol-kind <kind> --symbol-line <line>` for exact source; add disambiguators when duplicate names exist.
11. Run `projectatlas health-check` before cleanup/refactor decisions.
12. Only then use language servers or broad file reads on selected targets.
13. Run `projectatlas token` when token-savings reporting is requested; use `projectatlas token --view tui` only for a human terminal dashboard.
14. Run `projectatlas lint --strict-folders --report-untracked` before finishing structural changes.

Token savings estimate avoided wrong-folder exploration, wrong-file opens, and unnecessary full-code reads caused by the atlas-first workflow. Agent and MCP surfaces should remain structured TOON by default; the TUI view is explicit terminal UI.

## Local Gates

For ProjectAtlas itself, run:

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --doc --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo run -p projectatlas-cli -- map --force
cargo run -p projectatlas-cli -- lint --strict-folders --report-untracked
```

## Map Interpretation

- `overview` shows repository scale and purpose coverage.
- `folders` chooses a work area by path and purpose.
- `files` narrows the file set inside a folder.
- `summary` gives structured deterministic file facts and purpose state before full source reads.
- `outline` gives compressed source context and token estimates.
- `symbols` lists functions, classes, methods, imports, calls, dependencies, and manifest-level Rust/Cargo context.
- `search` finds literal, regex, or fuzzy text matches inside indexed files with optional path filters.
- `slice` returns exact source ranges after a file is selected.
- `health-check` flags missing purposes, duplicate purposes, repeated temp/generated folders, and cleanup signals.
- `settings` and `watch-status` report local index/config state.
- `token` reports estimated ProjectAtlas token savings.
- `mcp` starts the native MCP server. MCP tool text content is TOON.

## References

- ProjectAtlas repository: https://github.com/styler-ai/ProjectAtlas
- Live documentation: https://styler-ai.github.io/ProjectAtlas/
- `docs/projectatlas-3-architecture.md` for the target architecture.
- `docs/agent-integration.md` for AGENTS.md startup snippets.
- `docs/format.md` for TOON schema.
- `docs/workflow.md` for troubleshooting.
