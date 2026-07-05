# Purpose: Document agent startup and MCP integration workflows for ProjectAtlas.

# Agent Integration

ProjectAtlas is designed to be read at agent startup so you can:

- Get a repository atlas before broad search.
- Pick the correct file quickly.
- Spot duplicated folder roles early.
- Keep structure clean as the repo grows.
- Track token savings caused by the atlas-first workflow.

ProjectAtlas is an atlas of the entire project, not a shortcut to full-file reads:

1. First, learn where things live: folder structure and `folder_purpose`.
2. Second, learn what each selected folder contains: `file_purpose` and one-line `content_summary`.
3. Third, after the folder and file are known, read the detailed summary report.
4. Fourth, inspect exact source slices only after the right folder and file are selected.

The atlas update order is always folder index first, file purpose and one-line file summaries second, and deep code index last. Do not treat symbol indexing as the first gate.

Purpose correctness matters. `folder_purpose` and `file_purpose` explain why a folder or file exists; `content_summary` explains what the index currently observes inside a file. Generated summaries can be deterministic metadata, but generated purposes are only suggestions. Treat a purpose as correct only when it is imported from trusted metadata or explicitly agent-reviewed in the SQLite index. Folder purposes should be curated broadly because they are the navigation backbone. File purposes should be curated selectively when they affect navigation, current work, public/build/test/runtime behavior, or stale trusted metadata. When lint or health reports a missing folder or high-impact file purpose, the agent should inspect enough context to write the correct one-line purpose and set it with `atlas_purpose_set` or `projectatlas purpose set`; ProjectAtlas is for the agent harness, so there should be no human approval bottleneck in normal operation. If the agent sees a wrong, stale, vague, or generic purpose during normal work, it should correct it immediately after inspecting enough context.

Purpose completion loop:

1. Read the focused curation queue with `atlas_purpose_queue` or `projectatlas purpose queue --limit <n>`.
2. Use the atlas sequence to inspect the path: folders, files, outline or symbols as needed.
3. Set the correct purpose with `atlas_purpose_set` or `projectatlas purpose set`.
4. For a reviewed batch, use `atlas_purpose_review` or `projectatlas purpose review --from-file <json> --apply`
   instead of raw SQLite edits.
5. Refresh with `atlas_watch_once`, `projectatlas watch --once`, or `projectatlas scan`.
6. Rerun health/lint.
7. Continue until the database has complete reviewed folder purposes, selected high-value file purposes, and the deep index is current.

For planned folder and file purpose creation or correction, including initial creation and broad refreshes, delegate the work to a low-reasoning subagent when the host supports subagents. The delegated task should receive bounded ProjectAtlas queue rows, summaries, outlines, or exact snippets, not an instruction to browse the whole repository. The subagent may write purposes through `atlas_purpose_set`, `atlas_purpose_review`, `projectatlas purpose set`, or `projectatlas purpose review`, then report changed paths and commands. A purpose written by an agent or subagent through those ProjectAtlas APIs is agent-approved and does not need a second approval pass. If any agent notices a wrong, stale, vague, or generic purpose during normal work, correct it along the way with the same ProjectAtlas APIs. Agents and subagents must not edit SQLite directly.

`atlas_purpose_queue` and `projectatlas purpose queue` default to all folders and high-impact files. Low-priority source files stay out of the default queue so agents are not pushed through every file in a large repository. Pass `projectatlas purpose queue --include-low-priority-files` or MCP `include_low_priority_files: true` only for explicit broad file-purpose cleanup. Use `projectatlas purpose queue --include-assets`, MCP `include_assets: true`, raw `atlas_health`, or bare `projectatlas health-check` only when intentionally curating assets or generated outputs; non-source files should usually inherit purpose from an approved asset root instead of becoming one-by-one queue noise.
Queue metadata includes `folder_scope` and `file_scope`; agents should use those fields to understand whether files are limited to high-impact entries, all source files, or asset-inclusive mode.

`projectatlas lint` defaults to `--purpose-level low`: stale, duplicate, and repeated temporary-folder findings fail the gate, while first-pass missing/suggested/agent-review purpose curation for folders plus high-impact files stays advisory so new installs can bootstrap. Use `projectatlas purpose queue` for the actionable low-scope curation list, `--purpose-level medium` when all source files must be agent-reviewed, and `--purpose-level strict` only when a user explicitly wants every indexed file and folder reviewed.

If a duplicate-purpose, repeated temporary folder, or similar deterministic finding is intentional after inspection, resolve that exact finding with `atlas_health_resolve` or `projectatlas health resolve <finding-id> <category> <path> --related-path <path> --rationale "<why>"`. Do not resolve missing-purpose findings; fill the purpose instead.

ProjectAtlas MCP uses the standard MCP stdio JSON-RPC transport. Per the MCP transport spec, stdio messages are
newline-delimited JSON-RPC messages; ProjectAtlas does not use LSP-style `Content-Length` framing on stdio. Tool
result text is TOON by default, so agents get compact structured payloads without changing the MCP envelope.

## Codex / AGENTS.md snippet

```
## Startup
0. If ProjectAtlas MCP tools are available, use `atlas_*` tools for normal ProjectAtlas command families before shelling out. Expected parity tools include `atlas_init`, `atlas_config`, `atlas_root`/`atlas_root_set`, `atlas_ignore_list`/`atlas_ignore_init_gitignore`/`atlas_ignore_add`/`atlas_ignore_remove`, `atlas_lint`, `atlas_runtime_info`, `atlas_mcp_config`, `atlas_session_brief`, `atlas_task_status`/`atlas_task_cancel`, and `atlas_map`, plus the existing scan, overview, folder, file, summary, search, slice, health, purpose, token, settings, and watcher-status tools. Use the CLI for plugin install/update/release/CI workflows, MCP server startup/debugging, continuous `watch`, terminal TUI views, or when an MCP tool is unavailable.
0.1. When a GitHub issue has an OpenSpec change, mirror `openspec/changes/<id>/tasks.md` into the issue as a visible checklist under an `OpenSpec Tasks` or `OpenSpec Task Checklist` heading, and update checked items before status updates, closure, or release. Keep `openspec/issue-map.json` current and run `.github/scripts/issue-checklists.py` for release/check-in validation. Treat local/GitHub checklist drift as a check-in blocker; random checkboxes outside explicit task-checklist sections must not satisfy the gate.
1. Establish the project root. Run ProjectAtlas from that root so `.projectatlas/projectatlas.db` belongs to this project only.
2. For first-run setup, run `atlas_init` or `projectatlas init`; it creates the local DB/config, runs the initial scan/index by default, writes generated MCP configs, and returns a purpose handoff.
2.1. When init returns a purpose handoff, delegate initial folder/file purpose creation or correction to a subagent with low reasoning and apply reviewed purposes through ProjectAtlas purpose APIs.
3. Run `projectatlas scan` or `projectatlas watch --once` when the SQLite index may be stale after later edits.
4. Run `projectatlas overview`.
5. Run `projectatlas folders <query>` to choose where to work from `folder_purpose` overviews.
6. Run `projectatlas files <query> --folder <path>` to pick targets from `file_purpose` and `content_summary`; use `projectatlas files --file-pattern <glob>` when the filename/path pattern is already known.
7. Run `projectatlas summary <file> --limit 25` for detailed file intelligence before opening full source; inspect `parser_kind` and `summary_status` before trusting the `content_summary`.
8. Run `projectatlas outline <file>` if the structured summary is not enough.
9. Run `projectatlas symbols list --file <file>` and `projectatlas symbols relations --file <file>` when symbol context is needed.
10. Run `projectatlas search <pattern> --file-pattern <glob>` for bounded, glob-filtered text search in selected areas; search is intentionally case-insensitive by default for agent discovery, add `--case-sensitive` only when exact casing matters, add `--fuzzy` when the name is approximate, and check returned, searched file, searched byte, and truncated counters before widening the search.
11. Run `projectatlas slice <file> --start-line <n> --end-line <m>` or `projectatlas symbols slice <file> <symbol> --symbol-parent <parent> --symbol-kind <kind> --symbol-line <line>` for exact source slices; add symbol disambiguators when duplicate names exist.
12. Run `projectatlas health-check --source-only --limit 50` when planning cleanup or refactors.
13. Run `atlas_lint` or `projectatlas lint --report-untracked --purpose-level low`.
14. Run `projectatlas token` when the user asks how many tokens ProjectAtlas saved.
15. Only then run language-server lookups or broad file reads on the selected files.

Note: the non-source file list (`.projectatlas/projectatlas-nonsource-files.toon`) is agent-maintained input for
non-source summaries. Agents should read current repository intelligence from the SQLite-backed CLI/MCP
surfaces, not from a checked-in static map snapshot. Purpose review batch files are replay inputs for
`projectatlas purpose review`; SQLite remains authoritative after the ProjectAtlas command applies them.
```

## MCP Server

Prefer the `projectatlas init` or installer-generated project-local MCP config at `.projectatlas/projectatlas.mcp.json`
for `.mcp.json`-compatible hosts. Init and the installer also write
`.projectatlas/projectatlas.claude.mcp.json` for Claude Code and
`.projectatlas/projectatlas.opencode.json` for OpenCode. These files contain an absolute native
`projectatlas` binary path plus explicit project-local `--db` and `--config` arguments, and the
Codex/OpenCode configs include a `cwd` project-root hint where the host supports it. This prevents
agents from attaching to an old PATH wrapper or the wrong current working directory. `mcp-config` discovers both
`.projectatlas/config.toml` and `projectatlas.toml` from the selected DB/project root. The MCP server
also resolves path-less root-sensitive tools from config, indexed DB metadata, or the default
`.projectatlas/projectatlas.db` location so clients that ignore `cwd` still use the intended project
root.

One MCP server can serve multiple repositories. Use `atlas_set_project_path` to change the active
process default for later calls in a single-client stdio session, or pass per-call `project_path`
on normal `atlas_*` tools when a host knows the workspace root and needs request-level or shared
server isolation. Do not use the active default as a concurrency boundary. Root-level compatibility arguments
such as `atlas_scan.path` and `atlas_watch_once.path` are selected-root assertions first. If they
resolve outside the active project and the addressed root already has
`.projectatlas/projectatlas.db`, ProjectAtlas may route that call to the addressed indexed project.
If the addressed root is not already indexed, the tool returns a clear error and the agent must
switch with `atlas_set_project_path`, pass the correct `project_path`, or use ordinary filesystem
tools instead of ProjectAtlas for out-of-project files. File, folder, slice, purpose, health, and
search paths remain repository-relative inside the selected project.

`atlas_session_brief` is the compact startup probe for agents. It returns selected project identity,
index availability, overview counts when present, bounded ranked folder/file candidates, health
blockers, and typed next-call recommendations without scanning, writing telemetry, or reading source
content. `atlas_settings` includes an additive `mcp_session` capability block with nearest-project
startup policy, selected DB/config roots, path scope, telemetry mode, scan policy, runtime identity,
and no-secret guarantees. `atlas_task_status` and `atlas_task_cancel` expose the bounded MCP
task-progress contract; scan, watch, search, summary, slice, and CLI commands remain synchronous in
this release.

The plugin no longer ships a PATH-based fallback `.mcp.json`. Registering a plugin-level MCP file with
`command = "projectatlas"` is not portable across Windows, Linux, and macOS because an already-running
host process may not see PATH changes made by the runtime installer. Use the generated project-local
configs instead; they are version-guarded and point at the verified runtime by absolute path.

Use `projectatlas --format json runtime-info` as the compatibility probe. It reports runtime identity
and capabilities without creating `.projectatlas` or touching the project-local database.

The plugin installation must install or invoke the native `projectatlas` runtime before any server
is registered. From a source checkout, use:

```powershell
plugins/projectatlas/scripts/install-runtime.ps1
```

On Linux/macOS:

```bash
bash plugins/projectatlas/scripts/install-runtime.sh
```

When testing a newly released ProjectAtlas plugin through Codex, refresh the
configured Git marketplace snapshot if `codex plugin add` keeps installing an
older cached plugin version:

```bash
codex plugin marketplace upgrade projectatlas --json
codex plugin remove projectatlas --marketplace projectatlas
codex plugin add projectatlas --marketplace projectatlas
codex plugin list --marketplace projectatlas --available --json
```

If `codex plugin marketplace list --json` shows that the `projectatlas`
marketplace source is pinned to an older release tag, upgrade correctly keeps
that pinned ref. Replace the marketplace source only after confirming it is the
dedicated `styler-ai/ProjectAtlas` source and no unrelated plugin depends on it:

```bash
codex plugin marketplace remove projectatlas
codex plugin marketplace add styler-ai/ProjectAtlas --ref <new-release-tag>
```

Installer and release tests can provide an already-built runtime without
downloading a release or mutating PATH: use `-RuntimePath <path-to-projectatlas>`
on PowerShell or `PROJECTATLAS_RUNTIME_PATH=<path-to-projectatlas>` with the
POSIX installer. The supplied binary is still verified through
`projectatlas --format json runtime-info`, including version pinning when
`PROJECTATLAS_VERSION` is set.

Installer updates preserve project-local atlas state by default. They rewrite
generated MCP configs and managed runtime binaries, but they do not delete
`.projectatlas/projectatlas.db`, SQLite sidecars, token telemetry, approved
purposes, health resolutions, project config, or nonsource metadata. Use
`projectatlas reset-index --apply` only when you explicitly want local atlas
state removed.

Installers also prune verified stale ProjectAtlas shims from known user-local
locations such as Cargo and npm shim folders. Unknown PATH shadows are reported
with an actionable warning instead of being deleted automatically.

Installers also report obsolete `projectatlas` binaries or shims that remain on
PATH. Generated MCP configs use absolute, version-guarded runtime paths, but a
stale Python, npm, or Cargo shim can still affect bare `projectatlas` commands
in another shell until PATH order is fixed or the obsolete shim is removed.
The installer makes its own active process prefer the verified runtime on
Windows, Linux, and macOS; if a parent host process still cannot resolve the
bare command, restart that host or use the generated absolute MCP config.

When `codex` is available, installers also inspect the official
`projectatlas` Codex marketplace and `codex mcp get projectatlas`. If the
official marketplace is stale, the installer replaces that marketplace ref with
the runtime release tag and reinstalls the `projectatlas` Codex plugin. When
Codex exposes the installed plugin source path, the installer verifies the
ProjectAtlas skill artifact and manifest version; if the running host still has
older in-process skill metadata, restart the host after installation. If a
global Codex MCP server named `projectatlas` exists but points to a stale runtime
version or another project's DB/config, the installer removes and re-adds that
registry entry with the verified absolute runtime, current project database,
current config, and matching `--require-version`. On Windows, the LocalAppData
stable mirror is repaired for bare `projectatlas` PATH use, but MCP configs and
Codex registry entries stay pinned to the verified runtime path. Set
`PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE=1` only when a managed environment
intentionally owns the Codex ProjectAtlas plugin marketplace, and set
`PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE=1` only when it intentionally owns
the global Codex MCP registry. After plugin/runtime updates, agents should
verify `codex plugin list --marketplace projectatlas --json` and
`codex mcp get projectatlas` or `codex mcp list`; stale entries should be
repaired by rerunning the ProjectAtlas installer instead of left for the next
Codex restart.

The installers verify Claude Code and OpenCode generated MCP configs after writing them. The checks
parse the generated JSON and require the verified runtime path, `--require-version`, selected DB
path, effective config path when present, and final `mcp` command; OpenCode also requires
`type = "local"`, `enabled = true`, and the project `cwd`. Claude Code/OpenCode do not currently
have a ProjectAtlas-managed marketplace/cache repair path in the installer, so convergence is
generated-config verification plus explicit restart guidance for any running host session that cached
older instructions. The installers also warn on stale official ProjectAtlas release URLs in
downstream `.github/workflows` files. These workflow-pin warnings are intentionally non-mutating:
update the workflow pin deliberately or keep the warning as an explicit migration decision.

Harness-specific config can also be generated directly:

```bash
projectatlas --format json --db .projectatlas/projectatlas.db mcp-config --harness codex
projectatlas --format json --db .projectatlas/projectatlas.db mcp-config --harness claude-code
projectatlas --format json --db .projectatlas/projectatlas.db mcp-config --harness opencode
```

OpenCode uses the generated `opencode.json` shape with `mcp.projectatlas.type = "local"` and a
command array. Claude Code uses a plugin-compatible `.mcp.json` shape under `mcpServers`; ProjectAtlas
does not rely on Claude Code `cwd` support because the generated arguments bind the absolute DB/config
paths.

Installer verification uses the stable runtime contract:

```bash
projectatlas --format json runtime-info
```

The response must identify project `ProjectAtlas`, major version 3 or newer, capability `mcp`, text format `TOON`,
and the expected release `version` when a plugin manifest or `PROJECTATLAS_VERSION` pins the runtime.

## MCP Tool Sequence

Prefer MCP tools when the harness exposes them. Use CLI fallbacks only when the matching MCP tool is unavailable or the command is a reviewed exception.

0. `atlas_session_brief`: get a compact startup snapshot of selected project identity, index state, top candidates, blockers, and next calls.
1. `atlas_set_project_path` or per-call `project_path`: select the repository when one MCP server may serve multiple projects; prefer per-call `project_path` for shared or concurrent hosts.
1. `atlas_scan`: refresh repository, purpose, and symbol state.
2. `atlas_overview`: inspect repository scale and purpose coverage.
3. `atlas_folders`: choose the work area from folder purpose and path.
4. `atlas_files`: choose files inside the selected folder; add `file_pattern` for direct glob file discovery.
5. `atlas_file_summary`: read structured file facts and purpose state before opening source.
6. `atlas_outline`: read compressed line-level context for a selected file.
7. `atlas_symbols`: inspect functions/classes/methods/packages/dependencies.
8. `atlas_symbol_relations`: inspect imports, calls, dependencies, and containment.
9. `atlas_search`: search indexed files with filters and pagination.
10. `atlas_slice`: fetch exact line or symbol source only after selection.
11. `atlas_health`: find cleanup/refactor/DRY structure issues. Use `limit`, `start_index`, `category`, `severity`, `path_prefix`, `summary_only`, or `source_only` for large health surfaces.
12. `atlas_watch_once`: bounded refresh after local file changes when no continuous watcher is running.
13. `atlas_token_report`: report estimated token savings.
14. `atlas_settings` and `atlas_watch_status`: diagnose runtime/index/cache state.
15. `atlas_reset_index`: preview or clear local SQLite/cache files when the index is corrupt or intentionally being rebuilt.
16. `atlas_strip_legacy_purpose`: remove migrated `.purpose` files when explicitly requested.
17. `atlas_purpose_queue`: return the folder-first queue of missing, suggested, stale, and structural purpose work for agent curation.
18. `atlas_purpose_set`: write agent-approved purpose metadata into SQLite.
19. `atlas_purpose_review`: preview or apply a reviewed purpose batch into SQLite.
20. `atlas_health_resolve`: mark an intentional deterministic health finding resolved with rationale.
21. `atlas_init`: initialize ProjectAtlas files for first-time setup.
22. `atlas_runtime_info`: inspect runtime identity and capabilities.
23. `atlas_root` and `atlas_root_set`: inspect, verify, or bind project root identity.
24. `atlas_config`: inspect effective scan, purpose, and exclusion policy.
25. `atlas_ignore_list`, `atlas_ignore_init_gitignore`, `atlas_ignore_add`, and `atlas_ignore_remove`: manage the stricter ProjectAtlas manual ignore layer.
26. `atlas_lint`: run structure, purpose, and untracked-file lint gates.
27. `atlas_mcp_config`: generate harness MCP config with absolute runtime, DB, and config paths.
28. `atlas_map`: write an explicit legacy TOON compatibility map when needed.

Reviewed CLI-only exceptions: `projectatlas mcp` starts the server process, continuous `projectatlas watch` needs a terminal/lifecycle contract, and `projectatlas token --view tui` renders a terminal dashboard. Use `atlas_watch_once`, `atlas_watch_status`, and `atlas_token_report` for MCP-safe equivalents.

For read-only reviews or diagnostics, set `PROJECTATLAS_NO_TELEMETRY=1` before running CLI commands or the MCP server.
This preserves normal atlas reads while preventing usage telemetry writes to `.projectatlas/projectatlas.db`.

## When To Call What

| Situation | Preferred MCP tool | CLI fallback |
| --- | --- | --- |
| One MCP server may serve another repository | `atlas_set_project_path` or per-call `project_path` | `projectatlas --db <repo>/.projectatlas/projectatlas.db ...` |
| First-time ProjectAtlas setup | `atlas_init` | `projectatlas init` |
| Runtime identity and capabilities | `atlas_runtime_info` | `projectatlas --format json runtime-info` |
| Root diagnostics or binding | `atlas_root` with `verify` when needed, `atlas_root_set` | `projectatlas root show`, `projectatlas root verify`, `projectatlas root set <path>` |
| Start a non-trivial repo task | `atlas_scan` if stale, then `atlas_overview` | `projectatlas scan`, then `projectatlas overview` |
| Choose the work area | `atlas_folders` | `projectatlas folders <query>` |
| Choose files inside a work area | `atlas_files` | `projectatlas files <query> --folder <path>` |
| Direct glob file discovery | `atlas_files` with `file_pattern` | `projectatlas files --file-pattern <glob>` |
| Need structured file facts | `atlas_file_summary` | `projectatlas summary <file> --limit <n>` |
| Need effective scan/config policy | `atlas_config` | `projectatlas config --print` |
| Manage manual ProjectAtlas ignores | `atlas_ignore_list`, `atlas_ignore_init_gitignore`, `atlas_ignore_add`, `atlas_ignore_remove` | `projectatlas ignore list`, `projectatlas ignore init-gitignore`, `projectatlas ignore add ...`, `projectatlas ignore remove ...` |
| Need compressed file context | `atlas_outline` | `projectatlas outline <file>` |
| Need functions/classes/methods/packages | `atlas_symbols` | `projectatlas symbols list --file <file>` |
| Need imports/calls/dependencies/containment | `atlas_symbol_relations` | `projectatlas symbols relations --file <file>` |
| Need filtered text matches | `atlas_search` | `projectatlas search <pattern> --file-pattern <glob>` or `projectatlas search <pattern> --fuzzy --file-pattern <glob>` |
| Need exact source | `atlas_slice` | `projectatlas slice ...` or `projectatlas symbols slice ... --symbol-parent <parent>` |
| Files changed locally | `atlas_watch_once` | `projectatlas watch --once` |
| Long local editing session | `atlas_watch_status` for diagnostics | `projectatlas watch` |
| Planning cleanup/refactor/DRY work | `atlas_health` with filters/paging when needed | `projectatlas health-check --source-only --limit <n>` |
| Lint structure and purpose state | `atlas_lint` | `projectatlas lint --report-untracked --purpose-level low` |
| Curating missing or generated purposes | `atlas_purpose_queue`, then `atlas_purpose_set` or `atlas_purpose_review` | `projectatlas purpose queue --limit <n>`, then `projectatlas purpose set ...` or `projectatlas purpose review --from-file <json> --apply` |
| Intentional health conflict | `atlas_health_resolve` | `projectatlas health resolve ... --rationale <why>` |
| User asks for saved tokens | `atlas_token_report` | `projectatlas token` |
| Human asks for a terminal token dashboard | `atlas_token_report` first for agent state | `projectatlas token --view tui` |
| Runtime/index diagnostics | `atlas_settings`, `atlas_watch_status`, `atlas_runtime_info` | `projectatlas settings`, `projectatlas watch-status`, `projectatlas runtime-info` |
| Generate harness MCP config | `atlas_mcp_config` | `projectatlas --format json --db .projectatlas/projectatlas.db mcp-config` |
| Write explicit legacy TOON map | `atlas_map` | `projectatlas map --force` |
| Corrupt or intentionally discarded local index | `atlas_reset_index` dry-run first | `projectatlas reset-index --dry-run`, then `projectatlas reset-index --apply` |
| Migrating old `.purpose` files | `atlas_strip_legacy_purpose` dry-run first | `projectatlas strip-legacy-purpose --dry-run` |

Default sequence for coding tasks:

1. Refresh if stale.
2. Overview.
3. Folders.
4. Files.
5. Structured file summary.
6. Outline.
7. Symbols/relations or search as needed.
8. Exact slice.
9. Edit.
10. Watch once or scan.
11. Health/lint/tests.
12. Token report when requested.

Token savings estimate context that ProjectAtlas prevented the agent from wasting: wrong-folder exploration,
wrong-file opens, and unnecessary full-code reads avoided by the overview -> folders -> files -> summary/outline
-> exact-slice funnel. Agent and MCP surfaces stay structured TOON; Ratatui terminal charts belong only to the
explicit `projectatlas token --view tui` view.

The default token report is a fast offline heuristic, not provider billing telemetry. It estimates emitted
ProjectAtlas payload text with `ceil(chars / 4)` and file-size baselines with `ceil(bytes / 4)`. Reports expose
bucket, baseline kind, confidence, accounting layer, provider, model, tokenizer backend, and accuracy labels so agents can separate
observed full-file compression from modeled navigation savings. Use `tokens_avoided` for the conservative headline because repeated modeled baselines are deduped there; `estimated_saved` remains the legacy gross compatibility value. Local tokenizer calibration is explicit with `projectatlas token --tokenizer o200k_base` or `projectatlas token --tokenizer cl100k_base`; normal orientation and `atlas_token_report` must stay local and fast.

Read-avoidance counters are also local workflow estimates. Observed
summary/outline/slice replacements are stronger evidence than search-modeled
file reads avoided; aggregate bucket-only reports must stay `not_recorded`
instead of inventing whole-file-read counts.

For freshness, treat `projectatlas watch` as the steady-state updater for local editing sessions. Line slices
validate against SQLite and then read the current file from disk. Symbol slices also read current disk content,
but their line ranges come from the deep symbol index and should be kept fresh by the watcher or `atlas_watch_once`.

## Codex skills

ProjectAtlas ships public agent guidance through `AGENTS.md`, repository docs, and the packaged plugin skill.
Personal workspace memory is local state and should stay ignored/untracked through `.gitignore`.

## Claude Code Plugin And OpenCode MCP Config

The ProjectAtlas plugin package includes:

- `.codex-plugin/plugin.json` for Codex plugin metadata.
- `.claude-plugin/plugin.json` plus the root `skills/` folder for Claude Code plugin packaging.
- `opencode/opencode.json` as a disabled OpenCode MCP config template with absolute-path placeholders.
- Installer scripts that generate project-local Codex-compatible, Claude Code, and OpenCode config files after runtime verification.

The generated project-local files are the supported MCP registration path because they contain absolute runtime and project paths.
Checked-in templates must not be enabled with a bare `projectatlas` command.
ProjectAtlas does not ship a native OpenCode JavaScript/TypeScript plugin; OpenCode integration is the local MCP server config shape.

## Lint and CI

ProjectAtlas `lint` should run in local and CI workflows to surface missing or unapproved SQLite purpose records.
The static `.projectatlas/projectatlas.toon` map is an optional compatibility export only; normal CI should not
require a committed map diff.
