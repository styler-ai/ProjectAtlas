## Context

`projectatlas init` currently calls `init_project(root)` in `crates/projectatlas-cli/src/atlas_map.rs`. That function creates `.projectatlas/`, writes `.projectatlas/config.toml` if missing, writes `.projectatlas/projectatlas-nonsource-files.toon` if missing, and returns an empty string. MCP `atlas_init` exposes the same config-only behavior and currently describes itself as "without scanning source."

The desired first-run experience is broader:

```powershell
cd your-project
projectatlas init
```

That one command should leave the repository ready for high-quality ProjectAtlas use: local config, DB, scan/index, symbol/text search, and a structured purpose-curation handoff. Because ProjectAtlas is normally used through the plugin mechanism, an agent harness will usually be present. The Rust binary should still remain deterministic and harness-agnostic: it prepares and reports the work, while the agent harness spawns low-reasoning subagents to create/review folder and file purposes through ProjectAtlas APIs.

OpenSpec's `init` is a useful product pattern: validate the target, determine new-vs-extend mode, guard against wrong root, create/verify directories idempotently, generate tool artifacts, write config only if missing, and print next steps. ProjectAtlas should adapt that shape in Rust without depending on OpenSpec or npm.

## CLI Shape

Keep `projectatlas init` as the main command and expand options conservatively:

- `projectatlas init`: default first-run bootstrap.
- `projectatlas init --no-scan`: create/verify structure and config but skip scan/index.
- `projectatlas init --force-rescan`: run scan even when a compatible DB already exists.
- `projectatlas init --max-workers <n>`, `--timeout-seconds <n>`, and `--text-index-max-bytes <n>` may reuse scan options if the implementation can do so without duplicating parser configuration.

The default should be safe and idempotent. It must not overwrite config, delete DBs, or replace approved purposes.

## Phases

The init implementation should be a typed orchestration over existing primitives:

1. Resolve and validate root:
   - canonicalize target root,
   - verify write permission for `.projectatlas`,
   - reject invalid non-directory targets,
   - avoid accidentally initializing an unintended parent/child root.
2. Create/verify project surface:
   - `.projectatlas/`,
   - `.projectatlas/config.toml`,
   - `.projectatlas/projectatlas-nonsource-files.toon`,
   - `.projectatlas/projectatlas.db` schema if missing,
   - generated host MCP configs: `.projectatlas/projectatlas.mcp.json`, `.projectatlas/projectatlas.claude.mcp.json`, and `.projectatlas/projectatlas.opencode.json`.
3. Load effective config:
   - preserve existing config,
   - report config path and whether it was created or already existed.
4. Scan/index:
   - run existing scan pipeline unless skipped or already verified fresh enough by a future freshness check,
   - build symbols/text index using existing runtime code,
   - report counts, duration, warnings, and truncated/timeout state.
5. Purpose handoff:
   - produce folder/file purpose queue counts,
   - return clear harness instructions: spawn low-reasoning subagents for initial purpose creation and correction, apply via `atlas_purpose_review`/`projectatlas purpose review --apply`,
   - include machine-readable batches or next commands when possible.
6. Success/next steps:
   - report statuses as `created`, `exists`, `verified`, `skipped`, or `failed`,
   - print concise next commands for humans and structured JSON/TOON for agents.

## MCP Shape

`atlas_init` should expose parity with CLI init:

- accept options equivalent to the CLI flags,
- return the same typed setup report,
- preserve current project routing rules,
- allow `project_path` to explicitly target the project,
- never change the active default project when `project_path` is supplied for one init call,
- never use nearest-project discovery to silently initialize a different root.

The tool description should change from config-only initialization to first-run bootstrap. If `--no-scan`/equivalent is used, the report should clearly say scan was skipped.

## Agent Harness Contract

The plugin skill/guidance should say:

- After `projectatlas init` returns a purpose handoff, spawn a subagent with low reasoning to create folder and file purposes for the initial queue.
- Subagents should apply purposes only through ProjectAtlas APIs/CLI, not by editing SQLite.
- If an agent or subagent writes the purposes through the approved ProjectAtlas purpose API, those purposes are considered agent-reviewed.
- During normal work, if an agent sees a purpose that is wrong, stale, vague, or generic, it can correct it opportunistically.

The CLI itself should not try to spawn subagents. Outside an agent harness, it should print the handoff and next commands.

## Report Types

Prefer typed Rust structs shared by CLI and MCP:

- `InitSetupReport`
- `InitSurfaceReport`
- `InitConfigReport`
- `InitDbReport`
- `InitScanReport`
- `InitPurposeHandoff`
- `InitNextStep`

Use enums for status values instead of scattered strings:

- `created`
- `exists`
- `verified`
- `skipped`
- `failed`

## OpenSpec-Inspired Behaviors

Adapt these OpenSpec implementation ideas:

- validate permissions before mutation,
- distinguish first-time setup from extend/refresh mode,
- create directories recursively but do not overwrite user-owned files,
- support non-interactive flags,
- print created/refreshed/existing statuses,
- return clear getting-started next steps.

Do not copy OpenSpec code or add OpenSpec/npm as a dependency.

## Tests

Required tests:

- new temp repo: init creates config, nonsource TOON, DB, and scan/index report,
- existing `.projectatlas` without DB: init creates DB without overwriting config,
- existing DB/config: init verifies and preserves content,
- `--no-scan`: no scan happens and report says skipped,
- `--force-rescan`: scan runs even when DB exists,
- invalid target / unwritable target returns a clear error,
- MCP `atlas_init` returns matching structured fields,
- purpose handoff reports queue counts and subagent instructions without mutating purposes by default,
- running init twice is idempotent.

## Pre-Mortem

Risk: init becomes too slow for scripts.
Mitigation: provide `--no-scan` and report scan duration/timeouts.

Risk: init overwrites config or approved purposes.
Mitigation: preserve existing files by default and test sentinel config/purpose values.

Risk: CLI/MCP drift.
Mitigation: shared report types and parity tests.

Risk: harness purpose delegation is confused with Rust runtime behavior.
Mitigation: keep a structured handoff field and document that subagent spawning belongs to the plugin/agent harness.

Risk: first-run partial failure leaves unclear state.
Mitigation: phase-level status report with next commands to resume.
