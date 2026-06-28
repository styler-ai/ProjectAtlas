# Agent Integration

ProjectAtlas is designed to be read at agent startup so you can:

- Get a repository atlas before broad search.
- Pick the correct file quickly.
- Spot duplicated folder roles early.
- Keep structure clean as the repo grows.
- Track token savings caused by the atlas-first workflow.

ProjectAtlas is an atlas of the entire project, not a shortcut to full-file reads:

1. First, learn where things live: folder structure and folder purpose.
2. Second, learn what each selected folder contains: file purpose and one-line high-level file content.
3. Third, after the folder and file are known, inspect outlines, classes, methods, functions, imports, calls, and exact source slices.

The atlas update order is always folder index first, file purpose and one-line file summaries second, and deep code index last. Do not treat symbol indexing as the first gate.

Purpose correctness matters. A purpose explains why a folder or file exists; a summary explains what the index observes inside it. Generated summaries can be deterministic metadata, but generated purposes are only suggestions. Treat a purpose as correct only when it is imported from trusted metadata or agent-approved in the SQLite index. When lint or health reports a missing purpose, the agent should inspect the folder/file enough to write the correct one-line purpose and set it with `atlas_purpose_set` or `projectatlas purpose set`; ProjectAtlas is for the agent harness, so there should be no human approval bottleneck in normal operation.

Purpose completion loop:

1. Read missing-purpose findings from `atlas_health` or `projectatlas health-check`.
2. Use the atlas sequence to inspect the path: folders, files, outline or symbols as needed.
3. Set the correct purpose with `atlas_purpose_set` or `projectatlas purpose set`.
4. Refresh with `atlas_watch_once`, `projectatlas watch --once`, or `projectatlas scan`.
5. Rerun health/lint.
6. Continue until the database has complete folder/file purposes and the deep index is current.

If a duplicate-purpose, repeated temporary folder, or similar deterministic finding is intentional after inspection, resolve that exact finding with `atlas_health_resolve` or `projectatlas health resolve <finding-id> <category> <path> --related-path <path> --rationale "<why>"`. Do not resolve missing-purpose findings; fill the purpose instead.

ProjectAtlas MCP uses the standard MCP stdio JSON-RPC transport. Per the MCP transport spec, stdio messages are
newline-delimited JSON-RPC messages; ProjectAtlas does not use LSP-style `Content-Length` framing on stdio. Tool
result text is TOON by default, so agents get compact structured payloads without changing the MCP envelope.

## Codex / AGENTS.md snippet

```
## Startup
1. Establish the project root. Run ProjectAtlas from that root so `.projectatlas/projectatlas.db` belongs to this project only.
2. Run `projectatlas scan` or `projectatlas map --force` when the index may be stale.
3. Run `projectatlas overview`.
4. Run `projectatlas folders <query>` to choose where to work.
5. Run `projectatlas files <query> --folder <path>` to pick targets; use `projectatlas files --file-pattern <glob>` when the filename/path pattern is already known.
6. Run `projectatlas summary <file> --limit 25` before opening full source; inspect `parser_kind` and `summary_status` before trusting the observed summary.
7. Run `projectatlas outline <file>` if the structured summary is not enough.
8. Run `projectatlas symbols list --file <file>` and `projectatlas symbols relations --file <file>` when symbol context is needed.
9. Run `projectatlas search <pattern> --file-pattern <glob>` for bounded, glob-filtered text search in selected areas; search is intentionally case-insensitive by default for agent discovery, add `--case-sensitive` only when exact casing matters, add `--fuzzy` when the name is approximate, and check returned, searched file, searched byte, and truncated counters before widening the search.
10. Run `projectatlas slice <file> --start-line <n> --end-line <m>` or `projectatlas symbols slice <file> <symbol> --symbol-parent <parent> --symbol-kind <kind> --symbol-line <line>` for exact source slices; add symbol disambiguators when duplicate names exist.
11. Run `projectatlas health-check` when planning cleanup or refactors.
12. Run `projectatlas lint --strict-folders --report-untracked`.
13. Run `projectatlas token` when the user asks how many tokens ProjectAtlas saved.
14. Only then run language-server lookups or broad file reads on the selected files.

Note: the non-source file list (`.projectatlas/projectatlas-nonsource-files.toon`) is agent-maintained input for
non-source summaries and is merged into the atlas. Agents still read only the generated atlas.
```

## MCP Server

Prefer the installer-generated project-local MCP config at `.projectatlas/projectatlas.mcp.json`.
It contains an absolute native `projectatlas` binary path plus explicit project-local `--db` and
`--config` arguments plus a `cwd` project-root hint, so Codex/OpenCode/Claude Code do not attach to
an old PATH wrapper or the wrong current working directory. `mcp-config` discovers both
`.projectatlas/config.toml` and `projectatlas.toml` from the selected DB/project root. The MCP server
also resolves path-less root-sensitive tools from config, indexed DB metadata, or the default
`.projectatlas/projectatlas.db` location so clients that ignore `cwd` still use the intended project
root.

MCP root-changing tool arguments such as `atlas_scan.path` and `atlas_watch_once.path` are constrained
to that bound project root. Start a separate project-local MCP server instead of pointing one
project's DB/config at another repository.

The plugin-provided `plugins/projectatlas/.mcp.json` is only a fallback for harnesses that register
the plugin file directly from the project root. It includes `--require-version` so a stale PATH runtime
fails closed instead of starting an older MCP server:

```json
{
  "mcpServers": {
    "projectatlas": {
      "command": "projectatlas",
      "args": ["--require-version", "0.3.4", "--db", ".projectatlas/projectatlas.db", "mcp"]
    }
  }
}
```

Use `projectatlas --format json runtime-info` as the compatibility probe. It reports runtime identity
and capabilities without creating `.projectatlas` or touching the project-local database.

The plugin installation must install or invoke the native `projectatlas` runtime before any server
is registered. From a source checkout, use:

```powershell
plugins/projectatlas/scripts/install-runtime.ps1
```

On Linux/macOS:

```bash
plugins/projectatlas/scripts/install-runtime.sh
```

Installer verification uses the stable runtime contract:

```bash
projectatlas --format json runtime-info
```

The response must identify project `ProjectAtlas`, major version 3 or newer, capability `mcp`, text format `TOON`,
and the expected release `version` when a plugin manifest or `PROJECTATLAS_VERSION` pins the runtime.

## MCP Tool Sequence

Prefer MCP tools when the harness exposes them:

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
11. `atlas_health`: find cleanup/refactor/DRY structure issues. Use `limit`, `start_index`, `category`, `severity`, `path_prefix`, or `summary_only` for large health surfaces.
12. `atlas_watch_once`: bounded refresh after local file changes when no continuous watcher is running.
13. `atlas_token_report`: report estimated token savings.
14. `atlas_settings` and `atlas_watch_status`: diagnose runtime/index/cache state.
15. `atlas_reset_index`: preview or clear local SQLite/cache files when the index is corrupt or intentionally being rebuilt.
16. `atlas_strip_legacy_purpose`: remove migrated `.purpose` files when explicitly requested.
17. `atlas_purpose_set`: write agent-approved purpose metadata into SQLite.
18. `atlas_health_resolve`: mark an intentional deterministic health finding resolved with rationale.

For read-only reviews or diagnostics, set `PROJECTATLAS_NO_TELEMETRY=1` before running CLI commands or the MCP server.
This preserves normal atlas reads while preventing usage telemetry writes to `.projectatlas/projectatlas.db`.

## When To Call What

| Situation | Preferred MCP tool | CLI fallback |
| --- | --- | --- |
| Start a non-trivial repo task | `atlas_scan` if stale, then `atlas_overview` | `projectatlas scan`, then `projectatlas overview` |
| Choose the work area | `atlas_folders` | `projectatlas folders <query>` |
| Choose files inside a work area | `atlas_files` | `projectatlas files <query> --folder <path>` |
| Direct glob file discovery | `atlas_files` with `file_pattern` | `projectatlas files --file-pattern <glob>` |
| Need structured file facts | `atlas_file_summary` | `projectatlas summary <file> --limit <n>` |
| Need effective scan/config policy | `atlas_settings` | `projectatlas config --print` |
| Need compressed file context | `atlas_outline` | `projectatlas outline <file>` |
| Need functions/classes/methods/packages | `atlas_symbols` | `projectatlas symbols list --file <file>` |
| Need imports/calls/dependencies/containment | `atlas_symbol_relations` | `projectatlas symbols relations --file <file>` |
| Need filtered text matches | `atlas_search` | `projectatlas search <pattern> --file-pattern <glob>` or `projectatlas search <pattern> --fuzzy --file-pattern <glob>` |
| Need exact source | `atlas_slice` | `projectatlas slice ...` or `projectatlas symbols slice ... --symbol-parent <parent>` |
| Files changed locally | `atlas_watch_once` | `projectatlas watch --once` |
| Long local editing session | `atlas_watch_status` for diagnostics | `projectatlas watch` |
| Planning cleanup/refactor/DRY work | `atlas_health` with filters/paging when needed | `projectatlas health-check` |
| Intentional health conflict | `atlas_health_resolve` | `projectatlas health resolve ... --rationale <why>` |
| User asks for saved tokens | `atlas_token_report` | `projectatlas token` |
| Human asks for a terminal token dashboard | `atlas_token_report` first for agent state | `projectatlas token --view tui` |
| Runtime/index diagnostics | `atlas_settings`, `atlas_watch_status` | `projectatlas settings`, `projectatlas watch-status` |
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
-> exact-slice funnel. Agent and MCP surfaces stay structured TOON; terminal decoration belongs only to the
explicit `projectatlas token --view tui` view.

## Codex skills

ProjectAtlas ships public agent guidance through `AGENTS.md`, repository docs, and the packaged plugin skill.
Personal workspace memory is local state and should stay ignored/untracked through `.gitignore`.

## Claude / skills

Drop the `skills/claude/ProjectAtlas.md` into your Claude skills folder and reference it in your agent setup.

## Lint and CI

ProjectAtlas `lint` is meant for local workflows. Many teams skip map generation in CI, but still run
lint on PRs to surface missing or unapproved SQLite purpose records. Use `projectatlas map --force` if CI must regenerate the compatibility map.
