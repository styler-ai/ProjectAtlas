# Purpose: Document agent startup and MCP integration workflows for ProjectAtlas.

# Agent Integration

ProjectAtlas is designed to be read at agent startup so you can:

- Get a repository atlas before broad search.
- Pick the correct file quickly.
- Spot duplicated folder roles early.
- Keep structure clean as the repo grows.
- Track token savings caused by the atlas-first workflow.

ProjectAtlas is an atlas of the entire project, not a shortcut to full-file reads:

1. Bind the intended project and refresh only when the index may be stale.
2. Call `atlas_session_brief` once with the task and compact output.
3. Follow its returned summary, search, relation, health, or slice request without repeating discovery.
4. Inspect the smallest exact source slice only after the target is known.

Use overview, folders, and files as a manual or MCP fallback when the brief is unavailable, returns no actionable candidate, or broader repository structure is itself the task.

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

When init, session brief, or the purpose queue returns an actionable `low`-scope handoff, a capable host delegates that exact batch to an isolated purpose curator at the lowest reasoning tier it can enforce while the main task continues. A host that cannot enforce subagent execution or reasoning selection lets the main agent process the same bounded batch. The worker receives only queue rows and bounded current summary, graph, outline, or exact-slice context. It copies `task`, `work_key`, and `state_token` into conditional purpose review, never edits SQLite directly, and emits no successful-maintenance chatter in the normal conversation. ProjectAtlas reports a handoff and never claims the Rust server spawned a host agent. Automatic handoffs stay at `low`; `medium` and `strict` remain explicit.

The actionable queue contains only missing or suggested rows. Accepted intent is skipped unless an agent or user explicitly assigns a known inconsistency or genuine repurposing; that deliberate correction uses `atlas_purpose_set` or `projectatlas purpose set`. Source changes alone never demote or invalidate accepted purposes.

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
0.1. When a GitHub issue has an OpenSpec change, use the concise v0.3.26 #305 shape: `Why`, `What Changes`, `Capabilities`, `Release Scope`, `Non-Goals`, `Pre-Mortem` with likely failures and mitigation checkboxes, then one `OpenSpec Tasks` or `OpenSpec Task Checklist` section. Map each mitigation checkbox to owned task IDs with `(OpenSpec tasks: 1.2, 3.4)` and check it exactly when all referenced tasks are checked. Mirror `openspec/changes/<id>/tasks.md` exactly, keep `openspec/issue-map.json` current, and run `.github/scripts/issue-checklists.py` before check-in, status updates, closure, or release. Do not add issue-level commit/SHA evidence, task receipts, rendered evidence comments, hosted links per checkbox, or per-task test IDs.
1. Establish the project root. Run ProjectAtlas from that root so `.projectatlas/projectatlas.db` belongs to this project only.
2. For first-run setup, run `atlas_init` or `projectatlas init`; it creates the local DB/config, runs the initial scan/index by default, writes generated MCP configs, and returns a purpose handoff.
2.1. When init returns an actionable purpose handoff, delegate that exact `low`-scope batch to an isolated purpose curator at the lowest reasoning tier the host can enforce while the main task continues; otherwise process it in the main agent. Apply with the returned tokens through ProjectAtlas purpose APIs only.
3. Refresh with `atlas_watch_once`, `atlas_scan`, `projectatlas watch --once`, or `projectatlas scan` only when the SQLite index may be stale after later edits.
4. For task-directed MCP work, call `atlas_session_brief` once with `query`, `project_path` when needed, and `compact: true`; follow its typed next call directly.
5. Use returned compact summaries and crisp connections for ordinary direct callers or dependencies. Use detailed relations only when resolution, completeness, ambiguity, omitted connections, or exact occurrences matter.
6. Copy returned selectors and continuations into `atlas_slice`, `atlas_symbol_relations`, or the recommended next tool instead of guessing or repeating discovery.
7. Fall back to `atlas_overview`, then `atlas_folders` and `atlas_files`, only when the brief is unavailable, has no actionable candidate, or broader repository structure is itself the task. The manual CLI equivalents remain `projectatlas overview`, `projectatlas folders <query>`, and `projectatlas files <query> --folder <path>`.
8. Run `projectatlas outline <file>` or bounded `projectatlas search <pattern> --file-pattern <glob>` when selected-file context is insufficient.
9. Run `atlas_slice`, `projectatlas slice <file> --start-line <n> --end-line <m>`, or `projectatlas symbols slice <file> <symbol> --symbol-parent <parent> --symbol-kind <kind> --symbol-line <line>` for exact source; copy disambiguators from ProjectAtlas results.
10. Run `atlas_health` or `projectatlas health-check --source-only --limit 50` when planning cleanup or refactors.
11. Run `atlas_lint` or `projectatlas lint --report-untracked --purpose-level low`.
12. Run `atlas_token_report` or `projectatlas token` when the user asks how many tokens ProjectAtlas saved.
13. Only then run language-server lookups or broad file reads on selected files.

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

`atlas_session_brief` is the compact task-start probe for agents. Call it once with `compact: true`,
follow its returned next call directly, and do not repeat folder/file discovery. It returns selected project identity,
index availability, overview counts when present, bounded ranked folder/file candidates, health
blockers, and typed next-call recommendations without scanning, writing telemetry, or reading source
content. `atlas_settings` includes an additive `mcp_session` capability block with nearest-project
startup policy, selected DB/config roots, path scope, telemetry mode, scan policy, runtime identity,
and no-secret guarantees. The same settings response includes content-free language registry and
accepted-set versions, digests, the complete per-row detector/owner/tier/provenance matrix, pinned
optional-catalog identity, and independently derived detected, parsed, symbol, semantic, and
benchmarked counts; aliases and extensions never inflate those capability totals, and catalog
membership never implies an installed or accepted grammar. `atlas_task_status`
and `atlas_task_cancel` expose the bounded MCP
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
`PROJECTATLAS_VERSION` is set. Because this mode does not persist PATH, a stale
parent bare command is reported as requiring unlock/removal and an installer
rerun; restarting that parent alone cannot repair it.

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
bare command, follow its reported remedy: restart when the verified runtime was
persisted and resolves first in the effective fresh Machine-plus-User PATH, or
unlock/remove the stale command and rerun when persistence was skipped or a
Machine PATH entry still shadows it. Generated MCP configs remain usable through
their verified absolute runtime. On Windows, restart the environment-owning
launcher or terminal session before starting a new Codex or shell; restarting
only a child of an unchanged launcher can retain its stale process PATH.

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

- `atlas_set_project_path` or per-call `project_path`: select the repository when one MCP server may serve multiple projects; prefer per-call `project_path` for shared or concurrent hosts.
- `atlas_watch_once` or `atlas_scan`: refresh only when the selected index may be stale.
- `atlas_session_brief`: call once with the task and compact output, then follow its returned summary, search, relation, health, or slice request directly.
- `atlas_overview`, `atlas_folders`, and `atlas_files`: fallback when the brief is unavailable, returns no actionable candidate, or broad repository structure is itself the task.
- `atlas_file_summary`: read structured file facts and purpose state before opening source.
- `atlas_outline`: read compressed line-level context for a selected file.
- `atlas_symbols`: inspect functions/classes/methods/packages/dependencies.
- `atlas_symbol_relations`: inspect imports, calls, dependencies, and containment.
- `atlas_search`: search indexed files with filters and pagination.
- `atlas_slice`: fetch exact line or symbol source only after selection.
- `atlas_health`: find cleanup/refactor/DRY structure issues. Use `limit`, `start_index`, `category`, `severity`, `path_prefix`, `summary_only`, or `source_only` for large health surfaces.
- `atlas_watch_once`: bounded refresh after local file changes when no continuous watcher is running.
- `atlas_token_report`: report estimated token savings; optionally pass one repository-relative `benchmark_results` artifact for validated publication evidence.
- `atlas_settings` and `atlas_watch_status`: diagnose runtime/index/cache state.
- `atlas_reset_index`: preview or clear local SQLite/cache files when the index is corrupt or intentionally being rebuilt.
- `atlas_strip_legacy_purpose`: remove migrated `.purpose` files when explicitly requested.
- `atlas_purpose_queue`: return the folder-first queue of missing, suggested, stale, and structural purpose work for agent curation.
- `atlas_purpose_set`: write agent-approved purpose metadata into SQLite.
- `atlas_purpose_review`: preview or apply a reviewed purpose batch into SQLite.
- `atlas_health_resolve`: mark an intentional deterministic health finding resolved with rationale.
- `atlas_init`: initialize ProjectAtlas files for first-time setup.
- `atlas_runtime_info`: inspect runtime identity and capabilities.
- `atlas_root` and `atlas_root_set`: inspect, verify, or bind project root identity.
- `atlas_config`: inspect effective scan, purpose, and exclusion policy.
- `atlas_ignore_list`, `atlas_ignore_init_gitignore`, `atlas_ignore_add`, and `atlas_ignore_remove`: manage the stricter ProjectAtlas manual ignore layer.
- `atlas_lint`: run structure, purpose, and untracked-file lint gates.
- `atlas_mcp_config`: generate harness MCP config with absolute runtime, DB, and config paths.
- `atlas_map`: write an explicit legacy TOON compatibility map when needed.

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
| Start a non-trivial repo task | Refresh only if stale, then call `atlas_session_brief` once with `compact: true` and follow its returned call | Use the manual overview/folders/files funnel when session brief is unavailable |
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
| Compare the controlled navigation benchmark with live token context | `atlas_token_report` with repository-relative `benchmark_results` | `projectatlas token --benchmark-results <path>` |
| Human asks for a terminal token dashboard | `atlas_token_report` first for agent state | `projectatlas token --view tui` |
| Runtime/index diagnostics | `atlas_settings`, `atlas_watch_status`, `atlas_runtime_info` | `projectatlas settings`, `projectatlas watch-status`, `projectatlas runtime-info` |
| Generate harness MCP config | `atlas_mcp_config` | `projectatlas --format json --db .projectatlas/projectatlas.db mcp-config` |
| Write explicit legacy TOON map | `atlas_map` | `projectatlas map --force` |
| Corrupt or intentionally discarded local index | `atlas_reset_index` dry-run first | `projectatlas reset-index --dry-run`, then `projectatlas reset-index --apply` |
| Migrating old `.purpose` files | `atlas_strip_legacy_purpose` dry-run first | `projectatlas strip-legacy-purpose --dry-run` |

Default sequence for coding tasks:

1. Bind the intended project and refresh if stale.
2. Call one compact task-oriented session brief.
3. Follow its returned summary, search, relation, health, or slice call.
4. Continue from returned selectors without rediscovery.
5. Read the smallest exact slice.
6. Edit.
7. Watch once or scan.
8. Run health/lint/tests.
9. Report tokens only when requested.

Token savings estimate context that ProjectAtlas prevented the agent from wasting: wrong-folder exploration,
wrong-file opens, and unnecessary full-code reads avoided by the session-brief -> returned next call
-> exact-slice funnel. Agent and MCP surfaces stay structured TOON; Ratatui terminal charts belong only to the
explicit `projectatlas token --view tui` view.

The default token report is a fast offline heuristic, not provider billing telemetry. It estimates emitted
ProjectAtlas payload text with `ceil(chars / 4)` and file-size baselines with `ceil(bytes / 4)`. Reports expose
bucket, baseline kind, confidence, accounting layer, provider, model, tokenizer backend, and accuracy labels so agents can separate
observed full-file compression from modeled navigation savings. Use `tokens_avoided` for the conservative headline because repeated modeled baselines are deduped there; `estimated_saved` remains the legacy gross compatibility value. Local tokenizer calibration is explicit with `projectatlas token --tokenizer o200k_base` or `projectatlas token --tokenizer cl100k_base`; normal orientation and `atlas_token_report` must stay local and fast.

To attach the controlled v0.4 navigation benchmark to the existing overview,
pass its repository-relative path:

```bash
projectatlas token --benchmark-results docs/benchmarks/v0.4-agent-navigation-results.json
projectatlas --format json token --benchmark-results docs/benchmarks/v0.4-agent-navigation-results.json
```

The equivalent MCP request adds:

```json
{
  "benchmark_results": "docs/benchmarks/v0.4-agent-navigation-results.json"
}
```

The path is optional and applies only to token overviews, not trend reports.
The human TUI always remains the focused live token-impact dashboard and
reserves no space for release or plain-control comparisons. Supplying a
benchmark path populates only the structured CLI JSON/TOON and MCP report; even
an explicit `--view tui --benchmark-results <path>` combination leaves the
human layout and all non-clock cells unchanged.
ProjectAtlas accepts only a direct regular file below the selected project root
and reads at most 8 MiB. No path yields `unavailable`; safe read/decode failures
yield `failed`; a decoded but unsupported contract yields `incompatible`;
retained unmatched failures yield `partial`; and fully matched evidence yields
`compatible`. Absolute, parent-escaping, symlink, reparse-point, and non-file
paths are rejected at the request boundary.

The comparison is read-only publication evidence. It is attached once as
`TokenOverview.agent_efficiency` and rendered identically by CLI JSON/TOON and
`atlas_token_report`; the Ratatui overview never renders it. The benchmark is
never written to SQLite, added to live `tokens_avoided` or file-read estimates,
or cached.
Provider token counters remain descriptive-only; capability rows report calls
and emitted bytes without claiming per-tool token causality.

Read-avoidance counters are also local workflow estimates. Observed
summary/outline/slice replacements are stronger evidence than search-modeled
file reads avoided; aggregate bucket-only reports must stay `not_recorded`
instead of inventing whole-file-read counts.

The TUI keeps those file-read sources as separate proportional bars. Beneath
them, broad folder walks skipped and candidate files not opened each use two
different denominators: activity share is the row's persisted source steps
divided by all reconciled source steps, while token impact is the row's avoided
tokens divided by reconciled `tokens_avoided`. The exact source ledger below the
charts uses the same rows, counts, and token allocation.

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
