---
name: projectatlas
description: Use ProjectAtlas as the atlas-first orientation layer before broad source reads, and run ProjectAtlas CLI/MCP scan, overview, folder/file, outline, symbols, slice, health, lint, and token commands.
---

# Purpose: Guide agents through ProjectAtlas atlas-first repository orientation and MCP workflows.

# ProjectAtlas

## Goal

Use ProjectAtlas to open the agent's eyes in large repositories before expensive context operations. The workflow is:
scan, overview, folders, files, compressed outline or symbols, exact source.

ProjectAtlas is for the agent harness, not for human-facing documentation. Its job is to provide an atlas of the project so the agent knows where to look and does not repeatedly open every folder and file. Token reduction comes from using the atlas first and escalating only when needed.

The original ProjectAtlas goal has not changed: every important folder and file should have a one-line purpose so the agent can understand repository structure, navigate quickly, detect drift, and choose the right file before deep indexing. ProjectAtlas 3 keeps that goal and adds Rust speed, broader language support, a SQLite atlas database, MCP tools, and an improved deep code index.

ProjectAtlas exists to give the agent an atlas of the entire project:

1. First understand where things live: use `overview` and `folders` to get a folder-purpose overview and choose the right folder.
2. Use that folder-purpose overview as housekeeping signal too: missing, duplicate, stale, or inconsistent folder intent should be fixed or surfaced before new code lands in the wrong place.
3. Then understand which files live inside the selected folder: use `files --folder` to compare each file's `file_purpose` and `content_summary`.
4. Then read the detailed file intelligence with `summary`: `file_purpose`, `content_summary`, parser status, functions, methods, types, imports, calls, and counts.
5. Only after the folder, file, and needed range are known, open exact source with `slice` or `symbols slice`.

The atlas update order is always:

1. Update folders and folder purposes.
2. Update files, file purposes, and one-line high-level file summaries.
3. Update the deep code index: outlines, symbols, relations, and exact slice metadata.

Purpose and summary are separate:

- `folder_purpose` / `file_purpose` answer why a folder or file exists.
- `content_summary` answers what currently appears to be inside a file.
- `stale` applies to approved purpose metadata that needs review after a meaningful change; it does not mean the refreshed `content_summary` is stale.
- Generated summaries are acceptable as deterministic content metadata.
- Generated purposes are only suggestions and must not be treated as correct until imported from trusted legacy metadata or explicitly reviewed by the agent after inspection.
- Folder purposes are high-value navigation metadata and should be curated broadly.
- File purposes are curated selectively. Fill them when they affect current work, public API, build/config/workflow/test/runtime behavior, routes, migrations, commands, MCP surfaces, or stale trusted metadata. Do not review every file in a large repository unless the user explicitly asks for broad file-purpose cleanup.
- If lint, health, or the curation queue reports missing folder or high-impact file purposes, the agent must inspect the path enough to write a correct one-line purpose and call `atlas_purpose_set` or `projectatlas purpose set`; do not leave the purpose blank just because no human supplied it.
- If the agent notices a wrong, stale, vague, or generic purpose while working, fix it immediately with `atlas_purpose_set` or `projectatlas purpose set` after inspecting enough context. Purpose quality should improve during normal work.

## Purpose Completion Loop

When `atlas_purpose_queue`, `projectatlas purpose queue`, `atlas_health`, `projectatlas health-check`, or `projectatlas lint` reports missing purposes:

1. Start with `atlas_purpose_queue` or `projectatlas purpose queue --limit <n>` for a source-focused next-action list.
2. Use `atlas_folders`/`atlas_files` to locate the missing path.
3. Inspect only enough context to understand the path's actual role.
4. Write a precise one-line purpose with `atlas_purpose_set` or `projectatlas purpose set`.
5. For a reviewed batch, use `atlas_purpose_review` or `projectatlas purpose review --from-file <json> --apply`; do not edit SQLite directly.
6. Run `atlas_watch_once`, `projectatlas watch --once`, or `projectatlas scan`.
7. Rerun health/lint.
8. Repeat until the ProjectAtlas database has reviewed folder purposes, selected high-value file purposes, and the deep index is refreshed.

`atlas_purpose_queue` and `projectatlas purpose queue` default to all folders and high-impact files. Use `projectatlas purpose queue --include-low-priority-files` or MCP `include_low_priority_files: true` only when intentionally doing broad file-purpose cleanup. Use `projectatlas purpose queue --include-assets`, MCP `include_assets: true`, raw `atlas_health`, or bare `projectatlas health-check` only when intentionally curating assets or generated outputs.
Read `folder_scope` and `file_scope` in queue metadata before deciding how broad the current curation pass is.

`projectatlas lint` defaults to `--purpose-level low`: stale, duplicate, and repeated temporary-folder findings fail the gate, while first-pass missing/suggested/agent-review purpose curation for folders plus high-impact files stays advisory so new installs can bootstrap. Use `projectatlas purpose queue` for the actionable low-scope curation list, `--purpose-level medium` when all source files must be agent-reviewed, and `--purpose-level strict` only when the user explicitly wants every indexed file and folder reviewed. Strict is intentionally expensive on large repositories.

This loop is an agent responsibility installed with the plugin. Do not wait for human purpose text during normal agent-harness operation.

If `atlas_health` reports a duplicate-purpose, repeated temporary folder, or similar deterministic conflict that is correct after inspection, resolve that exact finding with `atlas_health_resolve` or:

```bash
projectatlas health resolve <finding-id> <category> <path> --related-path <path> --rationale "<why this is intentionally correct>"
```

Do not resolve missing-purpose findings; fill the purpose instead.

ProjectAtlas MCP uses the standard MCP stdio JSON-RPC transport. Stdio messages are newline-delimited JSON-RPC
messages, not LSP-style `Content-Length` framed messages. Tool result text is TOON by default.

This skill is part of the ProjectAtlas plugin on purpose. Installing the plugin should give the agent:

- this skill as the workflow and decision manual,
- native runtime installer scripts under `plugins/projectatlas/scripts/`,
- `projectatlas mcp-config` plus generated `.projectatlas/projectatlas.mcp.json`, `.projectatlas/projectatlas.claude.mcp.json`, and `.projectatlas/projectatlas.opencode.json` for absolute MCP paths,
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
3. Confirm the native runtime with `projectatlas --format json runtime-info`; the report must identify project `ProjectAtlas`, major version 3 or newer, capability `mcp`, text format `TOON`, and the plugin manifest version when a plugin release tag is known.
4. If the command is missing, resolves to an older non-ProjectAtlas wrapper, or reports a stale runtime version for the installed plugin, run the plugin runtime installer from the target project root or pass the project root explicitly. The installer verifies the stable `runtime-info` contract and matching release version, uses a local ProjectAtlas source checkout when present, otherwise downloads the release tag derived from the plugin manifest for the platform, then falls back to `cargo install --git https://github.com/styler-ai/ProjectAtlas --tag <plugin-release-tag> projectatlas-cli --locked`. It writes `.projectatlas/projectatlas.mcp.json` with absolute MCP paths:
   - Windows: `plugins/projectatlas/scripts/install-runtime.ps1`
   - Linux/macOS: `bash plugins/projectatlas/scripts/install-runtime.sh`
   When validating a newly released Codex plugin, refresh the configured Git marketplace snapshot if `codex plugin add` keeps installing an older ProjectAtlas version: `codex plugin marketplace upgrade projectatlas --json`, `codex plugin remove projectatlas --marketplace projectatlas`, `codex plugin add projectatlas --marketplace projectatlas`, then verify with `codex plugin list --marketplace projectatlas --available --json`. If `codex plugin marketplace list --json` shows the `projectatlas` marketplace is pinned to an older release tag, replace that marketplace source only after confirming it is the dedicated `styler-ai/ProjectAtlas` source and no unrelated plugin depends on it: `codex plugin marketplace remove projectatlas`, then `codex plugin marketplace add styler-ai/ProjectAtlas --ref <new-release-tag>`.
5. Confirm MCP registration uses the generated project-local config whenever possible:
   - Codex and generic `.mcp.json` hosts: `.projectatlas/projectatlas.mcp.json`
   - Claude Code: `.projectatlas/projectatlas.claude.mcp.json`
   - OpenCode: `.projectatlas/projectatlas.opencode.json`

   These generated files contain absolute runtime, DB, and config paths. Codex/OpenCode configs include a `cwd` project-root hint where supported; Claude Code config does not rely on `cwd` because the absolute DB/config arguments bind the project root. `mcp-config` discovers `.projectatlas/config.toml` and flat `projectatlas.toml` from the selected DB/project root. The MCP server also resolves path-less root-sensitive tools from config, indexed DB metadata, or the default `.projectatlas/projectatlas.db` parent, so hosts that ignore `cwd` still use the intended project. The plugin does not ship a PATH-based fallback `.mcp.json`; generated project-local configs are the supported registration path on Windows, Linux, and macOS.
   Installer tests or release validation may pass an already-built runtime with PowerShell `-RuntimePath <path>` or POSIX `PROJECTATLAS_RUNTIME_PATH=<path>`. The installer must still verify that runtime with `projectatlas --format json runtime-info` before writing configs.
   The installers make their own active process prefer the verified runtime, repair the Windows LocalAppData stable mirror for bare `projectatlas` PATH use, remove verified stale ProjectAtlas shims only from known user-local Cargo/npm-style locations, and report unknown PATH shadows with an actionable warning. MCP configs and Codex registry entries stay pinned to the verified runtime path instead of the mutable stable mirror. When `codex` is available, the installers also repair a stale official `projectatlas` Codex marketplace/plugin cache to the runtime release tag, verify the installed ProjectAtlas skill artifact when Codex exposes its plugin source path, and inspect `codex mcp get projectatlas`; they automatically replace a stale global Codex MCP registry entry with the verified runtime, current project DB, current config, and matching `--require-version`. The installers report Claude Code/OpenCode generated-config status and warn on stale official ProjectAtlas release pins in downstream `.github/workflows` files. Set `PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE=1` only when a managed environment intentionally owns the Codex ProjectAtlas plugin marketplace, and set `PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE=1` only when it intentionally owns that global registry. After plugin/runtime updates, agents must verify `codex plugin list --marketplace projectatlas --json` and `codex mcp get projectatlas` or `codex mcp list` point to the verified runtime and version, and rerun the installer if they do not. A parent Codex, Claude Code, or OpenCode process may still need a restart before bare `projectatlas` lookup or in-process skill metadata sees a newly installed runtime/skill, so generated absolute MCP configs remain the supported registration path.
   One MCP server can serve multiple repositories. Use `atlas_set_project_path` to change the active process default project for later calls in a single-client stdio session, or pass per-call `project_path` on normal `atlas_*` tools when a host knows the workspace root and needs request-level or shared-server isolation. Do not use the active default as a concurrency boundary. Root-level compatibility arguments such as `atlas_scan.path` and `atlas_watch_once.path` are selected-root assertions first; if they resolve outside the active project and the addressed root already has `.projectatlas/projectatlas.db`, ProjectAtlas may route that one call to the addressed indexed project. If the addressed root is not already indexed, the tool returns a clear error and the agent must switch with `atlas_set_project_path`, pass the correct `project_path`, or use ordinary filesystem tools instead of ProjectAtlas for out-of-project files. File, folder, slice, purpose, health, and search paths remain repository-relative inside the selected project.
6. Initialize the target repo with `projectatlas init`.
7. Run `projectatlas ignore list` before large-repo indexing. ProjectAtlas inherits `.gitignore` dynamically and then applies the manual ProjectAtlas ignore layer as stricter atlas-only exclusions. If the project has no `.gitignore` and needs one for local runtime state, run `projectatlas ignore init-gitignore`. Add broad generated/vendor/build directory names with `projectatlas ignore add --kind dir-name <name>`, and add exact generated or published subtrees with `projectatlas ignore add --kind path-prefix <path>`.
8. Run `projectatlas scan`.
9. Add or import one-line purpose records for important folders and files.
10. Add summaries for non-source files to `.projectatlas/projectatlas-nonsource-files.toon` when needed.
11. Run `projectatlas lint --report-untracked --purpose-level low`; fix blocking lint output and use `projectatlas purpose queue` for advisory low-scope purpose curation.
12. Run `projectatlas map --force` only when an explicit legacy TOON map export is needed.

## MCP Tool Workflow

Use the MCP tools when the harness exposes them. Normal ProjectAtlas command families should be MCP-first because they keep the agent in the atlas-first path and return TOON text payloads directly. Use the CLI for plugin install/update/release/CI workflows, MCP server startup/debugging, continuous `watch`, terminal TUI views, or when an MCP tool is unavailable.

0. `atlas_set_project_path` or per-call `project_path` when one MCP server may serve multiple repositories; prefer per-call `project_path` for shared or concurrent hosts.
1. `atlas_scan` when the index may be stale or after file/folder changes.
2. `atlas_overview` at startup to understand repository size and purpose coverage.
3. `atlas_folders` with the task query to choose the right work area.
4. `atlas_files` with the task query and selected folder to pick target files; add `file_pattern` when you already know the filename/path glob.
5. `atlas_file_summary` for detailed file intelligence: `file_purpose`, `content_summary`, `parser_kind`, `summary_status`, imports/dependencies, functions, methods, classes/types, calls, and line ranges. Treat `summary_status: fallback` as a signal to inspect deeper or improve the parser.
6. `atlas_outline` for a compact line-level file outline when the summary is not enough.
7. `atlas_symbols` and `atlas_symbol_relations` when function/class/import/call context is needed.
8. `atlas_search` for filtered literal, regex, or fuzzy matches inside indexed files.
9. `atlas_slice` for exact line or symbol source after folder/file/symbol selection.
10. `atlas_health` before cleanup, refactor, or DRY decisions. On large repositories, pass `limit`, `start_index`, `category`, `severity`, `path_prefix`, `summary_only`, or `source_only` so health review stays bounded.
11. `atlas_watch_once` after file changes when a continuous watcher is not running.
12. `atlas_token_report` when the user asks how many tokens ProjectAtlas saved.
13. `atlas_settings` and `atlas_watch_status` for diagnostics.
14. `atlas_reset_index` dry-run first when the local SQLite/cache state is corrupt or intentionally discarded.
15. `atlas_strip_legacy_purpose` only after migrated `.purpose` metadata is safely stored in SQLite.
16. `atlas_purpose_queue` when an agent needs a folder-first purpose curation queue before approving or correcting generated purposes.
17. `atlas_purpose_set` when an agent-approved purpose should be written to the durable index.
18. `atlas_purpose_review` when a reviewed batch should be previewed or applied to SQLite through the ProjectAtlas MCP surface.
19. `atlas_health_resolve` when a deterministic conflict is intentionally correct and should not be repeated.
20. `atlas_init`, `atlas_runtime_info`, `atlas_root`/`atlas_root_set`, `atlas_config`, `atlas_ignore_list`/`atlas_ignore_init_gitignore`/`atlas_ignore_add`/`atlas_ignore_remove`, `atlas_lint`, `atlas_mcp_config`, and `atlas_map` for CLI parity admin/reporting workflows when those tools are exposed.

## Command Decision Rules

- If MCP tools are available, use `atlas_*` tools for normal ProjectAtlas calls. Do not shell out to `projectatlas` for routine setup, overview, folders, files, search, summary, slice, health, purpose, config, ignore, lint, runtime-info, MCP config, or map work unless you are testing the CLI itself, using a reviewed CLI-only exception, or the MCP surface is unavailable.
- When a GitHub issue has an OpenSpec change, mirror `openspec/changes/<id>/tasks.md` into the issue as a visible checklist and update checked items before status updates, closure, or release. Treat local/GitHub checklist drift as a check-in blocker; do not leave issue task state only in local OpenSpec files.
- Start of any non-trivial repo task: call `atlas_scan` if the index may be stale, otherwise call `atlas_overview`.
- New session after scan: call `atlas_overview`, then `atlas_folders` with the task terms.
- Choosing where to work: call `atlas_folders` before `atlas_files`; do not jump directly to broad source reads.
- Choosing source targets: call `atlas_files` with the selected folder and task terms; add `file_pattern` for exact glob discovery such as `*.rs` or `src/**/*.ts`.
- Need structured file-level context: call `atlas_file_summary` before opening a full file, and verify `summary_status` is not `fallback` before relying on `content_summary`.
- Need compact line-level context: call `atlas_outline` after `atlas_file_summary` when the summary is not enough.
- Need API/function/class/module context: call `atlas_symbols` for declarations and `atlas_symbol_relations` for imports, calls, dependencies, and containment.
- Need exact code: call `atlas_slice` only after the folder, file, and range or symbol are known; pass symbol parent, kind, or line when duplicate symbol names exist.
- Exact line slices validate the file against the indexed project and then read current file content from disk. Symbol slices also read current disk content, but their line ranges come from the deep symbol index.
- Need text occurrences: call `atlas_search` with `file_pattern`, `context_lines`, and `limit` rather than broad shell search; search is intentionally case-insensitive by default for agent discovery, set `case_sensitive` only when exact casing matters, set `fuzzy` when the name is approximate, and treat `truncated`, searched file count, and searched byte count as the signal for whether to narrow or widen the glob.
- After creating, moving, deleting, or editing files: call `atlas_watch_once`, `projectatlas watch --once`, or `atlas_scan` before trusting old results.
- During a long local editing session: prefer a single continuous `projectatlas watch` process from the project root, then use MCP reads against the refreshed SQLite index. File edits refresh incrementally; directory/root/ignore-rule changes may trigger a full scan for correctness.
- Planning cleanup/refactor/DRY work: call `atlas_health` after overview/folder/file orientation and before proposing moves/merges; use `summary_only`, `source_only`, `category`, `severity`, `path_prefix`, `limit`, and `start_index` when the health surface is large.
- Purpose curation: call `atlas_purpose_queue` before writing purposes; then inspect enough context and call `atlas_purpose_set` for one path or `atlas_purpose_review` for a reviewed batch.
- Intentional health conflict after inspection: call `atlas_health_resolve` with a rationale.
- User asks about saved tokens: call `atlas_token_report`.
- Runtime looks wrong: call `atlas_runtime_info` when exposed, then `atlas_settings` and `atlas_watch_status`; fall back to `projectatlas --format json runtime-info` when MCP parity tools are unavailable.
- Local index/cache is corrupt or intentionally discarded: call `atlas_reset_index` dry-run first; apply only when rebuilding from source is acceptable.
- Read-only review or CI smoke must not mutate telemetry: set `PROJECTATLAS_NO_TELEMETRY=1` before running ProjectAtlas CLI commands or launching the MCP server.
- Migrating old metadata: call `atlas_scan` first, then `atlas_strip_legacy_purpose` with dry-run; apply only on explicit user request.
- If scan reports skipped stale purpose imports, treat them as legacy TOON rows for paths that are deleted or excluded by current scan policy. Confirm the current index is correct with `atlas_overview`, `atlas_files`, and `atlas_health`; do not recreate stale paths just to preserve old map rows.

When MCP registration files are needed, prefer `atlas_mcp_config` when exposed. If MCP config generation is unavailable from the current server or the server itself is being bootstrapped, generate them with:

```bash
projectatlas --format json --db .projectatlas/projectatlas.db mcp-config
projectatlas --format json --db .projectatlas/projectatlas.db mcp-config --harness claude-code
projectatlas --format json --db .projectatlas/projectatlas.db mcp-config --harness opencode
```

This emits harness-specific MCP config with the absolute `projectatlas` executable path, selected project database, and optional config path. `atlas_runtime_info` is the preferred read-only compatibility probe when exposed; `projectatlas --format json runtime-info` is the CLI fallback and must not create `.projectatlas` by itself.

If MCP tools are unavailable, use the equivalent CLI sequence:

| Situation | CLI command |
| --- | --- |
| First-time setup | `projectatlas init` |
| Runtime identity | `projectatlas --format json runtime-info` |
| Root diagnostics/binding | `projectatlas root show`, `projectatlas root verify`, or `projectatlas root set <path>` |
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
| Cleanup/refactor signals | `projectatlas health-check --source-only --limit <n>` |
| Purpose curation queue | `projectatlas purpose queue --limit <n>` |
| Purpose review batch | `projectatlas purpose review --from-file <json> --apply` |
| Purpose lint | `projectatlas lint --purpose-level low`, `projectatlas lint --purpose-level medium`, or `projectatlas lint --purpose-level strict` |
| Token savings | `projectatlas token` |
| Human token dashboard | `projectatlas token --view tui` |
| Diagnostics | `projectatlas settings`, `projectatlas config --print`, and `projectatlas watch-status` |
| Ignore policy | `projectatlas ignore list`, `projectatlas ignore init-gitignore`, `projectatlas ignore add --kind dir-name <name>`, and `projectatlas ignore add --kind path-prefix <path>` |
| Harness MCP config | `projectatlas --format json --db .projectatlas/projectatlas.db mcp-config` |
| Legacy TOON map export | `projectatlas map --force` |
| Reset local index/cache | `projectatlas reset-index --dry-run` then `projectatlas reset-index --apply` |

## Startup Workflow

1. Establish the project root and run ProjectAtlas from that root.
2. Run `projectatlas scan` when the SQLite index may be stale.
3. Run `projectatlas overview`.
4. Run `projectatlas folders <query>` to choose the right part of the repo.
5. Run `projectatlas files <query> --folder <path>` to select targets; use `projectatlas files --file-pattern <glob>` when the file/path pattern is already known.
6. Run `projectatlas summary <file> --limit 25` before opening full source; inspect `parser_kind` and `summary_status`.
7. Run `projectatlas outline <file>` if the structured summary is not enough.
8. Run `projectatlas symbols list --file <file>` and `projectatlas symbols relations --file <file>` when symbol context is needed.
9. Run `projectatlas search <pattern> --file-pattern <glob>` for bounded filtered text matches; add `--fuzzy` when the name is approximate, and inspect returned, searched file, searched byte, and truncated counters before widening the search.
10. Run `projectatlas slice <file> --start-line <n> --end-line <m>` or `projectatlas symbols slice <file> <symbol> --symbol-parent <parent> --symbol-kind <kind> --symbol-line <line>` for exact source; add disambiguators when duplicate names exist.
11. Run `projectatlas health-check --source-only --limit 50` before cleanup/refactor decisions.
12. Only then use language servers or broad file reads on selected targets.
13. Run `projectatlas token` when token-savings reporting is requested; use `projectatlas token --view tui` only for a human terminal dashboard.
14. Run `projectatlas lint --report-untracked --purpose-level low` before finishing structural changes. Low is the nonblocking first-pass purpose curation scope; use `projectatlas purpose queue` for the next curation actions and `--purpose-level medium` or `--purpose-level strict` only for intentionally broader enforced purpose-curation passes.

Token savings estimate avoided wrong-folder exploration, wrong-file opens, and unnecessary full-code reads caused by the atlas-first workflow. Agent and MCP surfaces should remain structured TOON by default; the TUI view is an explicit Ratatui terminal dashboard with headline `tokens_avoided`, measured summary/slice savings, navigation narrowing, a vertical with/without ProjectAtlas token comparison, a file-read-avoidance mix, bucket rows, and optional calibration.

Token reports are offline by default. The current heuristic is `ceil(chars / 4)` for emitted ProjectAtlas text and `ceil(bytes / 4)` for file-size baselines, labeled as `heuristic_estimate`, not model billing tokens. Inspect bucket metadata before making claims: `full_file_compression` with `observed_delta` accounting and `observed` confidence is stronger than modeled `navigation_avoidance` with `modeled_avoidance` accounting and `inferred` or `policy_estimate` confidence. Use `tokens_avoided` for the conservative headline because repeated modeled baselines are deduped there; `estimated_saved` and `legacy_gross_estimated_saved` remain compatibility gross numbers. Read-avoidance counters are derived from raw eligible summary/outline/slice/search events only: observed summary/slice replacements are stronger than search-modeled file reads avoided, and aggregate bucket-only reports must stay `not_recorded`. Local tokenizer calibration is explicit via `projectatlas token --tokenizer o200k_base` or `projectatlas token --tokenizer cl100k_base`; normal `projectatlas token` and `atlas_token_report` must not call network APIs.

## Local Gates

For ProjectAtlas itself, run:

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --doc --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo run -p projectatlas-cli -- lint --report-untracked
```

## Map Interpretation

The SQLite database and MCP/CLI query surfaces are the primary agent source of truth. `projectatlas map --force` is an explicit compatibility export for older workflows, not a normal startup or CI requirement.

- `overview` shows repository scale and purpose coverage.
- `folders` chooses a work area by path and `folder_purpose`, and helps agents spot structural housekeeping issues.
- `files` narrows the file set inside a folder.
- `summary` gives detailed deterministic file intelligence, including `file_purpose` and `content_summary`, before full source reads.
- `outline` gives compressed source context and token estimates.
- `symbols` lists functions, classes, methods, imports, calls, dependencies, and manifest-level Rust/Cargo context.
- `search` finds literal, regex, or fuzzy text matches inside indexed files with optional path filters.
- `slice` returns exact source ranges after a file is selected.
- `health-check` flags missing purposes, duplicate purposes, repeated temp/generated folders, and cleanup signals.
- `settings` and `watch-status` report local index/config state.
- `token` reports estimated ProjectAtlas token savings and likely file reads avoided. The default report is a fast offline `chars/bytes / 4` workflow heuristic, not provider/model billing-token accounting. Token output includes measured/gross/deduped/headline totals, read-avoidance counters, plus bucket, baseline, confidence, accounting layer, provider/model/backend, and accuracy labels.
- `mcp` starts the native MCP server. MCP tool text content is TOON.

## References

- ProjectAtlas repository: https://github.com/styler-ai/ProjectAtlas
- Live documentation: https://styler-ai.github.io/ProjectAtlas/
- MCP stdio transport spec: https://modelcontextprotocol.io/specification/2025-06-18/basic/transports#stdio
- `docs/projectatlas-3-architecture.md` for the target architecture.
- `docs/agent-integration.md` for AGENTS.md startup snippets.
- `docs/format.md` for TOON schema.
- `docs/workflow.md` for troubleshooting.
