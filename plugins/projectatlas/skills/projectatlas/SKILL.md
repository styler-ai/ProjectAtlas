---
name: projectatlas
description: Use ProjectAtlas as the atlas-first orientation layer before broad source reads, with MCP-first task startup, ranked navigation, exact evidence, purpose curation, health, lint, and token reporting.
---

# ProjectAtlas

## Goal

For task-directed work in an existing indexed repository, use ProjectAtlas to reach the correct live source with the least navigation context:

`atlas_session_brief(compact: true) -> returned typed next call -> atlas_file_summary(compact: true) or relations/search -> atlas_slice`

ProjectAtlas is an agent orientation layer. It combines reviewed folder/file responsibility, current source summaries, parser trust, and bounded graph connections so agents select the right source before opening it. TOON is the default agent format; current saved bytes are authoritative, including dirty and non-Git trees.

## Task Startup

1. On first use in each distinct project root, call `atlas_init` when exposed or run `projectatlas init` from that root. If a read-only call returns `init_required`, execute its exact `atlas_init` next call for the returned `project_path`; do not choose a database filename or reuse another root's state. Every project root owns its own `.projectatlas/projectatlas.db`, config, generated host configs, and initial index. Do not treat initialization of another repository as setup for the current one, and do not substitute a scan, symbol build, or hand-written MCP config for init.
2. Bind the intended project. In shared or concurrent MCP hosts, pass `project_path` on every call; use `atlas_set_project_path` only as a single-client process default.
3. Refresh only when needed. Prefer `atlas_watch_once` for ordinary changed-file updates. Use `atlas_scan` only when the index is absent or typed ProjectAtlas guidance requires a full refresh; never scan merely because a session started.
4. Call `atlas_session_brief` once at task-oriented startup with `query`, `project_path` when needed, and `compact: true`. For a focused code question, start with `file_limit: 3`, `folder_limit: 3`, `blocker_limit: 1`, and `purpose_limit: 1`; widen only when no actionable candidate is returned. Follow its typed next call directly; do not restart the brief or repeat folder/file discovery for a later caller, source, or public-boundary check.
5. Call a returned `atlas_file_summary` recommendation with `compact: true`. Use legacy/default summary output or an explicit `limit` only when full totals, empty sections, and complete coverage state are needed.
6. Use the compact summary's crisp connections for an ordinary direct caller or dependency already shown there. Do not add a relation call merely to reconfirm a trusted `called_by` or call row, and do not inspect a plausible sibling once another ranked summary contains the exact requested behavior. Use `atlas_outline` or `atlas_symbols` only when summary context is insufficient. Use `atlas_symbol_relations` with `view: "detailed"` and `compact: true` when resolution/completeness matters, a connection sample is truncated, ambiguity or external/unresolved state matters, or a required path is not explicit in the summaries. Request occurrences only when the call-site span itself is needed. When the compact result returns a top-level `next_call`, submit its tool and arguments unchanged; do not reconstruct the cursor or rediscover the file first.
7. Use `atlas_slice` for the smallest exact line or symbol range that answers the task. Do not guess a symbol line or other disambiguator; copy the returned selector fields.
8. Stop once exact evidence answers the task. For external reachability, verify the owning module, re-export, package, or route boundary once; a public nested declaration alone is not proof, but do not repeat a boundary already established by a trusted export or exact declaration. Public exposure is not an inbound-caller question: use the trusted export or a bounded module/re-export declaration, not a relation query on the entrypoint.
9. Fall back to `atlas_overview` only when the session-brief MCP tool is unavailable, the brief has no actionable candidate, or broader repository structure is itself the task. Then use `atlas_folders` before `atlas_files`.

`connections_truncated` describes the compact sample. It means more relationships exist, not that the selected next call is wrong. Use the returned detailed-relations call only when those additional relationships matter.

## Indexing Strategy

- **First use for each project root:** `atlas_init` or `projectatlas init`. This is the normal per-project setup and initial-index path.
- **Fresh existing index:** make no indexing call. Start with `atlas_session_brief`.
- **Changed files:** use `atlas_watch_once`; it incrementally refreshes affected source, summaries, symbols, graph facts, and freshness state.
- **Full refresh:** use `atlas_scan` only for a missing index, an intentional repository-wide rebuild, or typed full-refresh guidance after continuity, root, policy, or index uncertainty.
- **Deep symbol/graph rebuild:** use `atlas_symbols_build` only when ProjectAtlas reports the symbol/graph projection missing, stale, or incomplete, or when the user explicitly requests that rebuild. Do not run it at ordinary startup or before every relation query.
- **Continuous editing:** a human may keep `projectatlas watch` running; agents use `atlas_watch_once` for bounded refreshes.

Never reset or replace an incompatible database as an orientation shortcut. Follow its typed recovery guidance and preserve authored purpose state.

## Task-to-Tool Routing

| Task | Primary route | Follow-up |
| --- | --- | --- |
| Startup, project state, ranked candidates | `atlas_session_brief` with `compact: true` | Execute the returned summary, search, relations, slice, health, or scan request |
| First use in a project | `atlas_init` | Honor the returned initial-index and purpose-curation handoff |
| Changed files since the last verified index | `atlas_watch_once` | Continue only from the new complete generation |
| Missing index or typed full-refresh requirement | `atlas_scan` | Do not use for routine session startup |
| Missing, stale, or explicitly requested deep symbol/graph projection | `atlas_symbols_build` | Then use `atlas_symbols` or `atlas_symbol_relations`; do not rebuild repeatedly |
| Broad work-area selection | `atlas_overview`, `atlas_folders`, `atlas_files` | Summary for the selected file |
| One-file intelligence or direct impact already shown by crisp connections | `atlas_file_summary` with `compact: true` | Follow the selected connection to another compact summary or exact slice; use relations only when its stronger trust/path facts are material |
| Declaration lookup | `atlas_symbols` | Slice the returned exact selector |
| Inbound/outbound relations or bounded graph projection | `atlas_symbol_relations` with `view: "detailed"` and `compact: true` | Submit a top-level continuation `next_call` unchanged; otherwise copy a row `next_call` selector into summary, relations, or slice; do not restart discovery |
| Public reachability | Trusted owning-file export or bounded search for the module/re-export declaration | Slice the exact declaration when source proof is required; a reviewed purpose and nested `pub` declaration are selection evidence, not exposure proof |
| Architecture, impact, dead-code, cycle, or static path review | `atlas_symbol_relations` with its closed analysis view/mode | Treat candidate/inconclusive output as review evidence, then inspect returned source selectors |
| Indexed text discovery | `atlas_search` with a bounded `file_pattern` when possible | Slice the returned range; narrow before paging when truncated |
| Exact source | `atlas_slice` | Stop when sufficient |
| Missing, suggested, stale, or wrong purposes | `atlas_purpose_queue`, then `atlas_purpose_review` or `atlas_purpose_set` | Delegate one bounded `low` batch to a low-reasoning subagent when supported; never edit SQLite |
| Cleanup, coverage, or purpose diagnostics | `atlas_health` / `atlas_purpose_queue` | Resolve a confirmed conflict or curate through purpose APIs |
| Manual ProjectAtlas ignore policy | `atlas_ignore_list`, then `atlas_ignore_add` / `atlas_ignore_remove` | Keep `.gitignore` authoritative and add only stricter atlas-specific rules |
| Runtime/config/index diagnostics | `atlas_runtime_info`, `atlas_root`, `atlas_config`, `atlas_settings`, `atlas_watch_status` | Use typed recovery guidance |
| CLI/MCP compatibility audit | `atlas_parity_report` | Use for explicit diagnostics or release/CI proof, not normal navigation |
| Legacy manual next-step ranking | `atlas_next` | Use only when session brief is unavailable or the manual overview/folders/files route is intentional |

Search is lexical by default. Literal/token acceleration must preserve exact results; regex, fuzzy, short, punctuation-sensitive, or Unicode-unsafe queries may use bounded persisted-text fallback. Inspect searched files/bytes, completeness, and truncation before widening. Semantic or hybrid retrieval is explicit and may return typed unavailable/stale lifecycle state.

## Freshness, Trust, and Bounds

- Exact slices read current source bytes. Indexed selectors and summaries must be fresh or return typed `refresh_required` guidance.
- `summary_status: fallback` means generated summary prose needs deeper inspection; it is not full parser trust.
- Purpose suggestions are not reviewed truth. Exact paths/names and reviewed responsibility outrank popularity.
- Relation and analysis results are static indexed source facts, not runtime traces.
- Preserve typed coverage, resolution, confidence, total-state, continuation, cancellation, and truncation fields. Never turn partial or ambiguous coverage into certainty.
- Keep every query bounded by the available row/depth/edge/time/output controls. Prefer a returned continuation over broad source reads.
- After edits, moves, deletes, ignore/config changes, or offline changes, refresh before trusting prior indexed results.
- For a long local session, a continuous `projectatlas watch` may keep the index current; use `atlas_watch_once` when continuous watch is not running.

## Purpose Curation

`folder_purpose` and `file_purpose` explain why a path exists; `content_summary` explains what is currently in it. Generated purposes remain `suggested` until an agent approves them. A purpose written through `atlas_purpose_set` or successfully applied through `atlas_purpose_review` becomes `approved`, `source: agent`, and `agent_reviewed: true` immediately; it is no longer a generated suggestion and needs no second approval pass. Approved purposes are durable authored responsibility: source, summary, symbol, graph, scan, and watcher changes do not demote them. Deleted/excluded paths leave purposes dormant; renames do not transfer approval.

When init, session brief, or `atlas_purpose_queue` returns an actionable `low`-scope handoff:

1. Keep the main task moving.
2. If the host supports bounded isolated subagents, partition a large queue into bounded, non-overlapping batches and delegate them to one or more agents at the lowest reasoning tier the host can enforce. Respect the host's subagent and ProjectAtlas worker budget, assign each path to exactly one curator, and curate folders before files whose responsibility depends on them. Otherwise process the queue in the main agent.
3. Give each curator only its queue rows and bounded summary/graph/outline/slice context.
4. Copy each batch's `task`, `work_key`, and `state_token` into `atlas_purpose_review`. Write only through `atlas_purpose_set` / `atlas_purpose_review` or their CLI equivalents; never edit SQLite. A successful agent write is already approved and agent-reviewed.
5. Skip accepted purposes unless an agent or user explicitly assigns a correction.
6. Keep successful maintenance out of normal conversation. ProjectAtlas exposes the handoff; the Rust server does not spawn an agent.
7. Never expand automatic work to `medium` or `strict`.

For a single known wrong or genuinely repurposed accepted purpose, inspect enough current context and use `atlas_purpose_set` deliberately. For missing/suggested rows, use the bounded queue, review, refresh, and rerun health/lint. Do not resolve a missing-purpose finding; fill the purpose. Use `atlas_health_resolve` only for an inspected deterministic conflict that is intentionally correct.

`projectatlas lint` defaults to `--purpose-level low`. Use `medium` only when all source files must be reviewed and `strict` only when every indexed file and folder must be reviewed.

## Root, Ignore, and Isolation Rules

- Run from the project root; the normal database is `<root>/.projectatlas/projectatlas.db`.
- One MCP server may serve several indexed roots. Per-call `project_path` is the concurrency-safe choice.
- Never route a path outside the selected root unless that addressed root is already indexed and explicitly selected.
- If the selected DB is incompatible or belongs to another project, do not reset, migrate, attach, merge, substitute, or fall back silently. Use typed recovery guidance or an explicit isolated DB.
- `.gitignore` is dynamically authoritative. ProjectAtlas manual ignores are a stricter atlas-only layer applied afterward.
- Use `atlas_ignore_list` before adding excludes. Use `atlas_ignore_init_gitignore` only when the project root genuinely lacks the needed `.gitignore`.
- Keep local agent/editor/cache state in `.gitignore`; do not encode personal tool folders as product invariants.
- Run `atlas_reset_index` as a dry run first and apply only when rebuilding derived state is acceptable.
- Remove legacy `.purpose` files only after scanning/importing and a successful `atlas_strip_legacy_purpose` dry run.

## Setup and Runtime Repair

For every new project root, run `atlas_init` when exposed or `projectatlas init` from that root. It creates or verifies that project's local config, DB, host configs, and initial index. Initialization does not carry across roots. Honor any returned purpose handoff.

After installing ProjectAtlas, read and follow this shipped skill before broad source reads. If the harness does not load plugin skills automatically, preserve the repository's existing guidance and add one durable pointer to the nearest harness instruction file: `AGENTS.md` for Codex, `CLAUDE.md` for Claude Code, or the host's equivalent. The pointer should tell future agents to use the installed/version-matched ProjectAtlas skill and MCP tools, run init only when project-local state is absent, and follow the skill's incremental freshness policy. Do not replace unrelated project instructions or paste a duplicate copy of the full skill.

Use `atlas_runtime_info` first. CLI fallback:

`projectatlas --format json runtime-info`

The runtime must report ProjectAtlas major version 3+, MCP, SQLite, TOON, and a runtime `version` matching the selected plugin release. Verify the installed plugin version and shipped skill artifact separately through the installer and harness plugin inventory. If the runtime is missing or stale, resolve the installer from the installed, version-matched ProjectAtlas plugin root or a checked-out matching ProjectAtlas release, then pass the target project root separately:

- Windows: `& "<projectatlas-plugin-root>\scripts\install-runtime.ps1" -ProjectRoot "<target-project-root>"`
- Linux/macOS: `bash "<projectatlas-plugin-root>/scripts/install-runtime.sh" "<target-project-root>"`

The command path belongs to the ProjectAtlas plugin/release artifact; the working/project-root argument belongs to the repository being initialized. Do not assume an unrelated target repository contains `plugins/projectatlas/scripts`.

Prefer installer-generated absolute host configs:

- generic/Codex: `.projectatlas/projectatlas.mcp.json`
- Claude Code: `.projectatlas/projectatlas.claude.mcp.json`
- OpenCode: `.projectatlas/projectatlas.opencode.json`

After plugin/runtime updates, verify `codex plugin list --marketplace projectatlas --json` and `codex mcp get projectatlas` (or `codex mcp list`). Rerun the installer if the official plugin cache, skill, MCP registry, runtime version, DB/config binding, or downstream release pin is stale. Use `PROJECTATLAS_SKIP_CODEX_PLUGIN_UPDATE=1` or `PROJECTATLAS_SKIP_CODEX_MCP_REGISTRY_UPDATE=1` only in intentionally managed environments. A parent host may need restart; absolute generated configs remain authoritative.

For a stale official plugin snapshot, run `codex plugin marketplace upgrade projectatlas --json`, `codex plugin remove projectatlas --marketplace projectatlas`, `codex plugin add projectatlas --marketplace projectatlas`, then `codex plugin list --marketplace projectatlas --available --json`. If that source is pinned to an older release tag, replace only the dedicated `styler-ai/ProjectAtlas` source after confirming it has no unrelated consumers: `codex plugin marketplace remove projectatlas`, then `codex plugin marketplace add styler-ai/ProjectAtlas --ref v0.4.1`.

MCP stdio uses newline-delimited JSON-RPC, not `Content-Length` framing.

## MCP-First Operations

Use MCP for normal ProjectAtlas command families. Use CLI for installer/release/CI work, MCP startup/debugging, continuous watch, terminal TUI, or when a tool is unavailable.

- Select/setup: `atlas_set_project_path`, `atlas_init`
- Refresh: `atlas_scan`, `atlas_watch_once`, `atlas_symbols_build`
- Navigate: `atlas_session_brief`, `atlas_overview`, `atlas_folders`, `atlas_files`, `atlas_next`, `atlas_file_summary`, `atlas_outline`, `atlas_symbols`, `atlas_symbol_relations`, `atlas_search`, `atlas_slice`
- Maintain: `atlas_health`, `atlas_health_resolve`, `atlas_purpose_queue`, `atlas_purpose_set`, `atlas_purpose_review`, `atlas_lint`
- Diagnose/admin: `atlas_root`, `atlas_root_set`, `atlas_config`, `atlas_settings`, `atlas_watch_status`, `atlas_runtime_info`, `atlas_ignore_list`, `atlas_ignore_init_gitignore`, `atlas_ignore_add`, `atlas_ignore_remove`, `atlas_mcp_config`, `atlas_reset_index`, `atlas_strip_legacy_purpose`, `atlas_parity_report`
- Bounded task model: `atlas_task_status`, `atlas_task_cancel`
- Telemetry: `atlas_token_report`
- Compatibility export only: `atlas_map`

Read-only review or CI smoke must set `PROJECTATLAS_NO_TELEMETRY=1`.

## CLI Fallback

| Need | Command |
| --- | --- |
| Initialize | `projectatlas init` |
| Refresh | `projectatlas scan` or `projectatlas watch --once` |
| Broad orientation | `projectatlas overview`; `projectatlas folders <query>`; `projectatlas files <query> --folder <path>` |
| Exact file discovery | `projectatlas files --file-pattern <glob>` |
| Summary/outline | `projectatlas summary <file> --limit <n>`; `projectatlas outline <file>` |
| Symbols/relations | `projectatlas symbols list --file <file>`; `projectatlas symbols relations --file <file>` |
| Search | `projectatlas search <pattern> --file-pattern <glob> --context-lines <n>` |
| Exact source | `projectatlas slice <file> --start-line <n> --end-line <m>`; `projectatlas symbols slice <file> <symbol> --symbol-parent <parent>` |
| Health/purpose/lint | `projectatlas health-check --source-only --limit <n>`; `projectatlas purpose queue --limit <n>`; `projectatlas purpose review --from-file <json> --apply`; `projectatlas lint --report-untracked --purpose-level low` |
| Runtime/root/config | `projectatlas --format json runtime-info`; `projectatlas root verify`; `projectatlas config --print` |
| Token report | `projectatlas token` |
| Human dashboard | `projectatlas token --view tui` |
| Continuous watch | `projectatlas watch` |
| Legacy export | `projectatlas map --force` |

## Token Reporting

Use `atlas_token_report` or `projectatlas token` when asked. Treat `tokens_avoided` as the conservative headline. Default accounting is offline `ceil(chars_or_bytes / 4)` heuristic, not model billing. Distinguish observed summary/slice replacement from modeled navigation narrowing; inspect accounting layer, baseline, confidence, provider/model/backend, and accuracy labels. Search-modeled file reads avoided are weaker than observed summary/slice replacements. Tokenizer calibration is explicit; normal reporting never calls network APIs.

## Repository Gates

For ProjectAtlas itself:

```bash
cargo fmt --check
cargo check --workspace --all-targets --all-features
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo test --doc --all-features
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features
cargo run -p projectatlas-cli -- lint --report-untracked
```

When an issue has OpenSpec tasks, keep the issue checklist exactly synchronized and run the repository IssueOps checker before transitions.

## References

- <https://github.com/styler-ai/ProjectAtlas>
- <https://styler-ai.github.io/ProjectAtlas/>
- `docs/projectatlas-3-architecture.md`
- `docs/agent-integration.md`
- `docs/format.md`
- `docs/workflow.md`
