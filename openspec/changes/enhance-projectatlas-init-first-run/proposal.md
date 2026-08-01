## Why

`projectatlas init` exists today, but it is a light config-surface initializer: it creates `.projectatlas/`, writes `config.toml` when missing, and writes the non-source TOON scaffold. First-time ProjectAtlas adoption still requires several separate steps before the repo is at peak usefulness: scan, DB creation, symbol/text indexing, purpose queue review, and agent-approved folder/file purposes.

OpenSpec's `openspec init` is a useful product pattern for this: one adoption command validates the target, detects whether the project is new or already configured, creates the required structure idempotently, generates tool/plugin artifacts, writes config only when missing, and prints next steps. ProjectAtlas should offer the same first-run feeling through the ProjectAtlas plugin/runtime, not through npm.

The target experience is:

```powershell
cd your-project
projectatlas init
```

After that one command, the project should have the local `.projectatlas` structure, DB, config, deep index, and a clear agent-facing purpose-curation handoff. In an agent/plugin harness, the harness can then spawn bounded isolated subagents at the lowest reliable reasoning and cost tier the host supports to create and apply reviewed folder/file purposes; a fixed reliable tier still delegates without a selector, and only hosts without bounded isolated subagent execution use the main agent.

## What Changes

- Strengthen `projectatlas init` into a one-call first-run setup flow.
- Keep it idempotent:
  - if `.projectatlas/` already exists, extend/verify it rather than overwrite it,
  - if `.projectatlas/projectatlas.db` already exists, verify schema/root compatibility rather than recreate it,
  - if config already exists, preserve user settings and report `exists`.
- Add explicit scan/index behavior:
  - create the DB if missing,
  - run a deep scan/symbol/text index on first setup unless disabled by a flag,
  - report scan summary and health/purpose queue counts.
- Add an agent-harness handoff contract:
  - CLI/MCP returns a structured setup report and purpose-curation work plan,
  - plugin/agent guidance says to delegate initial folder/file purpose creation and correction under the reliable-tier handoff rule,
  - applied purposes are marked agent-reviewed through ProjectAtlas purpose APIs.
- Keep the workflow native to ProjectAtlas's plugin/runtime mechanism; do not introduce npm or OpenSpec as a runtime dependency.
- Add MCP parity so `atlas_init` can perform or request the same first-run setup flow.

## Capabilities

### New Capabilities

- `projectatlas-first-run-init`: a one-call setup flow that bootstraps the ProjectAtlas local project surface and prepares an agent-driven purpose curation pass.

### Modified Capabilities

- `atlas_init` / `projectatlas init`: move from config-only initialization to an idempotent first-run bootstrap with explicit flags and structured output.

## Release Scope

This is promoted into the next-version release scope alongside the token TUI visual-regression fix. It targets the v0.3.26 milestone.

Expected future implementation scope:

- CLI command options and report types.
- MCP `atlas_init` parameter/result expansion.
- scan/index orchestration using existing runtime scan pipeline.
- purpose queue/report generation using existing purpose APIs.
- plugin/skill/harness documentation for reliable low-cost purpose curation.
- tests for new repo, existing `.projectatlas`, existing DB, stale config, scan disabled, and harness handoff output.

## Non-Goals

- Do not add npm/OpenSpec as a ProjectAtlas runtime dependency.
- Do not make the Rust binary directly spawn Codex/OpenCode/Claude subagents. The CLI returns a plan; the agent harness executes delegation.
- Do not overwrite existing config, DB, or approved purposes.
- Do not require purpose curation to block deterministic CLI initialization when no agent harness is present.
- Do not make nearest-project cross-root routing part of first-run setup.

## OpenSpec Inspiration

Use OpenSpec as a design reference, not a dependency:

- OpenSpec README quick start shows the adoption shape: `cd your-project` then `openspec init`.
- OpenSpec `InitCommand` validates the target path, detects extend mode, checks write permissions, detects legacy artifacts, detects tools, creates the `openspec/` structure idempotently, generates tool artifacts, writes config if missing, and prints clear next steps.
- ProjectAtlas should adapt the same product shape to Rust/plugin-native setup: validate root, classify existing `.projectatlas`, create/verify structure, run index, produce structured setup report, and hand off purpose curation to the agent harness.

References:

- https://github.com/Fission-AI/OpenSpec
- https://raw.githubusercontent.com/Fission-AI/OpenSpec/main/src/core/init.ts

## Pre-Mortem

Likely failure modes:

- `projectatlas init` becomes destructive by overwriting existing config, DB, or approved purpose metadata.
- The command tries to directly own subagent creation, which only makes sense inside an agent harness.
- First-run scan makes init slow or surprising in CI/scripted contexts.
- Purpose generation is treated as deterministic product output rather than agent-reviewed metadata.
- Partial failures leave `.projectatlas` in an ambiguous state.
- MCP and CLI init drift apart.

Mitigations:

- Use idempotent create/verify phases with explicit statuses: `created`, `exists`, `verified`, `skipped`, `failed`.
- Keep subagent delegation in plugin/agent instructions and structured handoff output, not inside the Rust binary process.
- Add flags such as `--no-scan`, `--force-rescan`, scan tuning controls, and JSON/TOON reports for automation.
- Preserve approved purposes and mark newly applied purposes through existing purpose review APIs.
- Make partial failures explicit in the setup report with next commands to resume.
- Require shared typed report structs for CLI/MCP parity.
