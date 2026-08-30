---
name: projectatlas
description: Use ProjectAtlas as the atlas-first orientation layer before broad source reads, with MCP-first task startup, short-alias worktree registration and routing, safe targeted initialization, ranked navigation, exact or federated graph evidence, purpose curation, health, lint, and repository-wide token reporting.
---

# ProjectAtlas

## Goal

For task-directed work in an existing indexed repository, use ProjectAtlas to reach the correct live source with the least navigation context:

`atlas_session_brief(compact: true) -> returned typed next call -> atlas_file_summary(compact: true) or relations/search -> atlas_slice`

ProjectAtlas is an agent orientation layer. It combines reviewed folder/file responsibility, current source summaries, parser trust, and bounded graph connections so agents select the right source before opening it. TOON is the default agent format; current saved bytes are authoritative, including dirty and non-Git trees.

## Task Startup

1. On first use in each distinct project root, call `atlas_init` when exposed or run `projectatlas init` from that root. If a read-only call returns `init_required`, execute its exact `atlas_init` next call using the returned `worktree` alias or `project_path`; do not choose a database filename or reuse another root's writable state. Every project root owns its own `.projectatlas/projectatlas.db`, config, generated host configs, and exact index. Do not substitute a scan, symbol build, or hand-written MCP config for init.
2. Bind the intended control project once. For a registered worktree, keep the agent in the control checkout and pass `worktree` on each root-scoped call. For unrelated or unregistered roots, pass `project_path`; use `atlas_set_project_path` only as a single-client process default. Never send both selectors.
3. Refresh only when needed. Prefer `atlas_watch_once` for ordinary changed-file updates. Use `atlas_scan` only when the index is absent or typed ProjectAtlas guidance requires a full refresh; never scan merely because a session started.
4. Call `atlas_session_brief` once at task-oriented startup with `query`, `project_path` when needed, and `compact: true`. For a focused code question, start with `file_limit: 3`, `folder_limit: 3`, `blocker_limit: 1`, and `purpose_limit: 1`; widen only when no actionable candidate is returned. Follow its typed next call directly; do not restart the brief or repeat folder/file discovery for a later caller, source, or public-boundary check.
   When the task has a content role, carry `content_selection: "source"`, `"documentation"`, or `"both"` on the returned files, search, summary, slice, symbol, and detailed-relation calls that expose it. Use `source` for ordinary implementation work, `documentation` for specification or guidance discovery, and `both` only when the task crosses the two. Omit the field only when the legacy candidate universe, including configuration/data and other text, is intentionally required.
5. Call a returned `atlas_file_summary` recommendation with `compact: true`. Use legacy/default summary output or an explicit `limit` only when full totals, empty sections, and complete coverage state are needed.
6. Use the compact summary's crisp connections for an ordinary direct caller or dependency already shown there. Do not add a relation call merely to reconfirm a trusted `called_by` or call row, and do not inspect a plausible sibling once another ranked summary contains the exact requested behavior. Use `atlas_outline` or `atlas_symbols` only when summary context is insufficient. Use `atlas_symbol_relations` with `view: "detailed"` and `compact: true` when resolution/completeness matters, a connection sample is truncated, ambiguity or external/unresolved state matters, or a required path is not explicit in the summaries. Request occurrences only when the call-site span itself is needed. When the compact result returns a top-level `next_call`, submit its tool and arguments unchanged; do not reconstruct the cursor or rediscover the file first.
7. Use `atlas_slice` for the smallest exact line or symbol range that answers the task. Do not guess a symbol line or other disambiguator; copy the returned selector fields.
8. Stop once exact evidence answers the task. For external reachability, verify the owning module, re-export, package, or route boundary once; a public nested declaration alone is not proof, but do not repeat a boundary already established by a trusted export or exact declaration. Public exposure is not an inbound-caller question: use the trusted export or a bounded module/re-export declaration, not a relation query on the entrypoint.
9. Fall back to `atlas_overview` only when the session-brief MCP tool is unavailable, the brief has no actionable candidate, or broader repository structure is itself the task. Then use `atlas_folders` before `atlas_files`.

`connections_truncated` describes the compact sample. It means more relationships exist, not that the selected next call is wrong. Use the returned detailed-relations call only when those additional relationships matter.

## Worktree MCP Workflow

Treat `main` as the reserved alias for the explicitly selected control atlas, not as a branch or directory name. The control checkout may itself be linked, live under `.worktrees`, or live anywhere else on the filesystem. ProjectAtlas discovers existing Git structure but never creates, switches, moves, prunes, or deletes a Git worktree or branch.

1. Call `atlas_worktree_list(include_retired: false)` from the control MCP process. Select a returned stable `selector`; do not reconstruct a full path or guess from a colliding directory or branch name. A row blocked because its common, administrative, or source path is not lossless UTF-8 has no registrable selector or root and must not be addressed through replacement-character display text.
2. Register an existing checkout with `atlas_worktree_add(worktree: "<selector>", alias: "issue-430")`. The alias is optional only when the target directory name is already a valid unique alias. Registration changes only the control atlas catalog; it does not create the target `.projectatlas` directory or mutate Git or source files.
3. Initialize an absent target with `atlas_init(worktree: "issue-430")`. A complete compatible control atlas seeds a private candidate, removes control identity, telemetry, task, and runtime state, reconciles the candidate against the target branch and dirty files, and publishes it atomically. A valid existing target database is preserved. An unsuitable baseline produces an explicit ordinary-init fallback; cancellation or integrity failure leaves the destination absent or unchanged.
4. Route ordinary calls without changing directory or mutable process selection: `atlas_session_brief(worktree: "issue-430", compact: true)`, `atlas_watch_once(worktree: "issue-430")`, `atlas_file_summary(worktree: "issue-430", file: "src/lib.rs", compact: true)`, or a purpose/health call with the same alias. Each admitted call captures its exact root, database, project identity, registration identity, and alias, and refuses a recreated database whose project identity differs, so interleaved `main` and worktree calls cannot retarget one another.
5. For a cross-worktree graph question, use one detailed or analysis `atlas_symbol_relations` call with `worktrees: ["main", "issue-430"]`. The first alias is primary. ProjectAtlas opens two to eight exact databases read-only, labels every participant/result/blocker/continuation, and never persists or merges sibling graphs. Do not also send legacy `roots`.
6. Request repository totals with `atlas_token_report(worktree: "main")` or run `projectatlas token` / `projectatlas token --view tui` from the control checkout. The existing TUI layout combines native-control plus active and retired registered-worktree aggregates. `atlas_token_report(worktree: "issue-430")` stays exact to that target's local detail. Alias-routed MCP usage is recorded once in control; independent local usage synchronizes monotonically without copying raw per-session events.
7. Retire only the ProjectAtlas registration with `atlas_worktree_remove(worktree: "issue-430")`. ProjectAtlas holds a short local SQLite writer-exclusion scope while it atomically final-syncs and retires the control registration, retains the accepted aggregate, and leaves the checkout, Git registration, branch, files, `.projectatlas`, and SQLite database untouched.

Use the returned typed recovery state rather than switching to paths: `ambiguous` returns bounded selectors; `init_required` returns the exact alias init call; `refresh_required` names the stale alias; invalid or mismatched Git evidence fails closed; `worktree_required` asks for an exact active checkout when a bare/common manager cannot select one. If Git externally removes a registered checkout, `atlas_worktree_list` retains its active alias, last validated root, accepted telemetry revision, and typed missing state so `atlas_worktree_remove(worktree: "<alias>")` can retire it without reconstructing a path. A manager with `core.worktree`, an enabled `config.worktree` override, or unresolved config includes never guesses its parent. Add revalidates the selected root and lifecycle immediately before registration, and a moved-root refresh cannot reactivate an alias retired by a concurrent remove. If Git recreates a worktree administrative directory at a previously registered path, remove the stale ProjectAtlas alias and add the replacement explicitly; ProjectAtlas will not let the replacement inherit the alias or touch its atlas through that stale mapping. Lifecycle identity requires creation time plus platform file-object identity (Unix device/inode or Windows retained-handle volume/128-bit file ID), and every persisted common, administrative, and source path must be lossless UTF-8 for SQLite metadata and MCP JSON. If the filesystem or path cannot provide the complete evidence, alias registration fails closed; never reuse a replacement-character display path as `project_path`. `project_path` remains the compatibility route for unregistered and older workflows when the path is lossless UTF-8, while alias routing is the normal concurrent-agent path.

## Classified Documentation Navigation

- Treat `classification` as a derived file role, not parser trust, purpose authority, or runtime truth. `documentation` is guidance; confirm implementation claims in current `source` summaries, symbols, or exact slices.
- Start with classified files or search. Use `source` for code-only work, `documentation` for docs-only discovery, and `both` when finding an explicit bridge. The closed selections exclude configuration/data, other text, and opaque rows; omission preserves the broader legacy behavior.
- Follow an explicit document bridge with `atlas_symbol_relations` using `view: "detailed"`, `relation: "documents"`, and the exact file or heading anchor. Outbound traversal moves from documentation to its validated repository target. Inbound traversal from source returns the same stored relation under the read-only `documented_by` view; no inverse fact is stored.
- Inspect parser provenance, coverage, completeness, resolution, and typed unresolved reason together with classification. Missing, ignored, outside-root, case-conflicting, unsupported, and non-static targets are evidence to narrow or repair the navigation request, never permission to guess.
- Submit the returned `next_call` unchanged. It preserves the exact file or heading selector, content selection, generation, and bounds; finish at current source evidence before making an implementation claim.
- In linked-worktree or shared-host sessions, pass the exact checkout `project_path` on every call. Each checkout owns its ignored writable database and classified graph; never substitute a sibling database or combine sibling graph generations.

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
| Existing Git worktree inventory and registration | `atlas_worktree_list`, then `atlas_worktree_add` with its stable selector | Use the short alias on all subsequent root-scoped calls |
| Registered worktree first use | `atlas_init` with `worktree` | Accept safe hydration or the explicit ordinary-init fallback; never copy a live DB manually |
| Registered worktree retirement | `atlas_worktree_remove` with `worktree` | Retained token totals remain in control; Git and target files remain untouched |
| First use in a project | `atlas_init` | Honor the returned initial-index and purpose-curation handoff |
| Changed files since the last verified index | `atlas_watch_once` | Continue only from the new complete generation |
| Missing index or typed full-refresh requirement | `atlas_scan` | Do not use for routine session startup |
| Missing, stale, or explicitly requested deep symbol/graph projection | `atlas_symbols_build` | Then use `atlas_symbols` or `atlas_symbol_relations`; do not rebuild repeatedly |
| Broad work-area selection | `atlas_overview`, `atlas_folders`, `atlas_files` | Summary for the selected file |
| One-file intelligence or direct impact already shown by crisp connections | `atlas_file_summary` with `compact: true` and task-appropriate `content_selection` | Follow the selected connection to another compact summary or exact slice; use relations only when its stronger trust/path facts are material |
| Declaration lookup | `atlas_symbols` | Slice the returned exact selector |
| Inbound/outbound relations or bounded graph projection | `atlas_symbol_relations` with `view: "detailed"`, `compact: true`, and task-appropriate `content_selection` | Request `relation: "documents"` explicitly for documentation bridges; use `worktrees` only for an explicit labelled read-only federation; submit a continuation or row `next_call` unchanged |
| Public reachability | Trusted owning-file export or bounded search for the module/re-export declaration | Slice the exact declaration when source proof is required; a reviewed purpose and nested `pub` declaration are selection evidence, not exposure proof |
| Architecture, impact, dead-code, cycle, or static path review | `atlas_symbol_relations` with its closed analysis view/mode | Treat candidate/inconclusive output as review evidence, then inspect returned source selectors |
| Indexed text discovery | `atlas_search` with a bounded `file_pattern` and task-appropriate `content_selection` when possible | Slice the returned range; narrow before paging when truncated |
| Exact source | `atlas_slice` with `content_selection: "source"` when supported by the returned call | Stop when sufficient |
| Missing, suggested, stale, or wrong purposes | `atlas_purpose_queue`, then `atlas_purpose_review` or `atlas_purpose_set` | Delegate one bounded `low` batch through isolated subagent execution at the lowest reliable reasoning and cost tier the host supports; otherwise process it in the main agent; never edit SQLite |
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
2. If the host supports bounded isolated subagents, partition a large queue into bounded, non-overlapping batches and delegate them to one or more agents at the lowest reliable reasoning and cost tier the host supports. Examples when available: Codex `gpt-5.6-luna` with `low` reasoning, or Claude Code `haiku`; otherwise use the host's lowest reliable equivalent as model names and availability change. Respect the host's subagent and ProjectAtlas worker budget, assign each path to exactly one curator, and curate folders before files whose responsibility depends on them. Otherwise process the queue in the main agent.
3. Give each curator only its queue rows and bounded summary/graph/outline/slice context.
4. Copy each batch's `task`, `work_key`, and `state_token` into `atlas_purpose_review`. Write only through `atlas_purpose_set` / `atlas_purpose_review` or their CLI equivalents; never edit SQLite. A successful agent write is already approved and agent-reviewed.
5. Skip accepted purposes unless an agent or user explicitly assigns a correction.
6. Keep successful maintenance out of normal conversation. ProjectAtlas exposes the handoff; the Rust server does not spawn an agent.
7. Never expand automatic work to `medium` or `strict`.

For a single known wrong or genuinely repurposed accepted purpose, inspect enough current context and use `atlas_purpose_set` deliberately. For missing/suggested rows, use the bounded queue, review, refresh, and rerun health/lint. Do not resolve a missing-purpose finding; fill the purpose. Use `atlas_health_resolve` only for an inspected deterministic conflict that is intentionally correct.

`projectatlas lint` defaults to `--purpose-level low`. Use `medium` only when all source files must be reviewed and `strict` only when every indexed file and folder must be reviewed.

## Root, Ignore, and Isolation Rules

- Run from the project root; the normal database is `<root>/.projectatlas/projectatlas.db`.
- Every ordinary checkout and linked worktree owns a private ignored writable database. Hydration copies reusable baseline state only into an unpublished candidate; after activation, every checkout remains an independent graph, purpose, source, task, and generation authority.
- One MCP server may serve several registered worktrees from the control checkout. Per-call `worktree` is the concurrency-safe normal route; per-call `project_path` remains available for an exact unregistered root.
- Use `atlas_root` with `control_root` (or `projectatlas root status <path>`) for bounded mutation-free structural worktree status. A common manager with one active worktree may select it; zero or several active worktrees require an exact worktree path.
- ProjectAtlas reports active, missing, and invalid Git structure and manages only its own alias registrations. It never creates, moves, prunes, removes, or switches Git worktrees. Git remains lifecycle authority.
- The token TUI opened from the control checkout shows durable repository-wide totals across control plus active and retired registrations without adding a worktree selector UI. An exact worktree report remains origin-scoped.
- Never route a path outside the selected root unless that addressed root is already indexed and explicitly selected.
- If the selected DB is incompatible or belongs to another project, do not reset, migrate, attach, merge, substitute, or fall back silently. Use typed recovery guidance or an explicit isolated DB.
- `.gitignore` is dynamically authoritative. ProjectAtlas manual ignores are a stricter atlas-only layer applied afterward.
- Use `atlas_ignore_list` before adding excludes. Use `atlas_ignore_init_gitignore` only when the project root genuinely lacks the needed `.gitignore`.
- Keep local agent/editor/cache state in `.gitignore`; do not encode personal tool folders as product invariants.
- Run `atlas_reset_index` as a dry run first and apply only when rebuilding derived state is acceptable.
- Remove legacy `.purpose` files only after scanning/importing and a successful `atlas_strip_legacy_purpose` dry run.

## Setup and Runtime Repair

For an unregistered project root, run `atlas_init` when exposed or `projectatlas init` from that root. For a registered worktree, prefer `atlas_init(worktree: "<alias>")` from the control process so a valid control baseline can be reused safely. Both paths create or verify the target's local config, database, host configs, and exact index. Honor any returned hydration/fallback and purpose handoff.

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

For a stale official plugin snapshot, run `codex plugin marketplace upgrade projectatlas --json`, `codex plugin remove projectatlas --marketplace projectatlas`, `codex plugin add projectatlas --marketplace projectatlas`, then `codex plugin list --marketplace projectatlas --available --json`. If that source is pinned to an older release tag, replace only the dedicated `styler-ai/ProjectAtlas` source after confirming it has no unrelated consumers: `codex plugin marketplace remove projectatlas`, then `codex plugin marketplace add styler-ai/ProjectAtlas --ref <matching-release-tag>`.

MCP stdio uses newline-delimited JSON-RPC, not `Content-Length` framing.

## MCP-First Operations

Use MCP for normal ProjectAtlas command families. Use CLI for installer/release/CI work, MCP startup/debugging, continuous watch, terminal TUI, or when a tool is unavailable.

- Select/setup: `atlas_set_project_path`, `atlas_init`, `atlas_worktree_list`, `atlas_worktree_add`, `atlas_worktree_remove`
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
| Broad orientation | `projectatlas overview`; `projectatlas folders <query>`; `projectatlas files <query> --folder <path> --content-selection source|documentation|both` |
| Exact file discovery | `projectatlas files --file-pattern <glob>` |
| Summary/outline | `projectatlas summary <file> --content-selection source|documentation|both --limit <n>`; `projectatlas outline <file>` |
| Symbols/relations | `projectatlas symbols list --file <file> --content-selection source|documentation|both`; `projectatlas symbols relations --view detailed --file <file> --relation documents --direction outbound|inbound --content-selection source|documentation|both` |
| Search | `projectatlas search <pattern> --file-pattern <glob> --content-selection source|documentation|both --context-lines <n>` |
| Exact source | `projectatlas slice <file> --content-selection source --start-line <n> --end-line <m>`; `projectatlas symbols slice <file> <symbol> --content-selection source|documentation|both --symbol-parent <parent>` |
| Health/purpose/lint | `projectatlas health-check --source-only --limit <n>`; `projectatlas purpose queue --limit <n>`; `projectatlas purpose review --from-file <json> --apply`; `projectatlas lint --report-untracked --purpose-level low` |
| Runtime/root/config | `projectatlas --format json runtime-info`; `projectatlas root verify`; `projectatlas config --print` |
| Token report | `projectatlas token` |
| Human dashboard | `projectatlas token --view tui` |
| Continuous watch | `projectatlas watch` |
| Legacy export | `projectatlas map --force` |

## Token Reporting

Use `atlas_token_report` or `projectatlas token` when asked. Control/main scope combines native control events with monotonically synchronized aggregates for active and retired registrations; an explicit worktree alias remains exact to that origin and does not present sibling detail as local. Treat `tokens_avoided` as the compatibility alias for the primary `average_tokens_avoided`: measured compression plus unchanged non-folder savings plus 50% of the deduped aggregate folder-navigation baseline, minus the complete Atlas payload. `maximum_tokens_avoided` retains the all-files folder-scope calculation. Inspect `average_policy`; 50% is a fixed policy estimate, not a benchmark-derived Codex average or provider-billing value. Default accounting is offline `ceil(chars_or_bytes / 4)` heuristic. Distinguish observed summary/slice replacement from modeled navigation narrowing; inspect accounting layer, baseline, confidence, provider/model/backend, accuracy, origin synchronization, and detail-availability labels. Search-modeled file reads avoided are weaker than observed summary/slice replacements. Tokenizer calibration is explicit; normal reporting never calls network APIs.

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

When an issue has OpenSpec tasks, keep exactly one visible `Implementation Tasks` section synchronized with its mapped local owner slice and exactly one canonical five-row `Acceptance and Review Tasks` section. Implementation tasks are live progress: check each row immediately after its behavior and required task-level proof pass, and reopen it immediately when review finds the implementation partial, resetting all acceptance/review rows. Keep acceptance unchecked until implementation is complete, then use a checked prefix; closed historical issues may retain one legacy OpenSpec task heading, while new-contract closed issues require both lists. Every open issue carries exactly one accepted `complexity:*` label, including unmapped backlog issues, without fabricating task fields. Use `(Implementation tasks: <task IDs>)` for open mapped pre-mortem mitigations, preserve existing implementation rows, and run the repository IssueOps checker before transitions. Pull-request validation resolves exactly one referenced owner, checks that owner against live state, checks unrelated slices against the accepted base, and fails closed without ownership or base authority; `main` and release validation remain global.

Contract provenance is repository-controlled per issue in `openspec/issue-map.json` under `legacy_closed_issues`: mapped issues outside that explicit set default to the new contract, and every exception must remain mapped, closed, legacy-only, an exact local owner-slice mirror, and complete with no unchecked historical task. The initial pull-request boundary validates each derived/listed exception using cached authenticated payloads before freezing the set against the accepted base; later pull requests cannot mutate it.

## References

- <https://github.com/styler-ai/ProjectAtlas>
- <https://styler-ai.github.io/ProjectAtlas/>
- `docs/projectatlas-3-architecture.md`
- `docs/agent-integration.md`
- `docs/format.md`
- `docs/workflow.md`
