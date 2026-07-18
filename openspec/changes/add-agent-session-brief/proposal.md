## Why

ProjectAtlas gives agents the right low-level MCP tools, but a normal startup still takes several calls before an agent can see the selected root, DB/config binding, index availability, likely work areas, health blockers, and safe next steps. That ceremony is useful but repetitive, and it leaves room for wrong-root or stale-index mistakes before source reads begin.

## What Changes

- Add a read-only MCP `atlas_session_brief` tool for compact startup orientation.
- Accept optional `project_path`, task `query`, and bounded limits for folders/files/blockers.
- Return typed, TOON/JSON-compatible fields for selected project identity, index state, overview counts, ranked candidates, blocker summary, and next-call recommendations.
- Compose existing settings, overview, ranking, and health data directly from store/service helpers.
- Avoid calling other MCP handlers from the brief because those handlers record token telemetry and would make a read-only brief mutate usage state.

## Capabilities

### New Capabilities
- `agent-session-brief`: Defines the MCP startup brief contract for selected project identity, index freshness, relevant navigation candidates, health blockers, and next-call recommendations.

### Modified Capabilities
- MCP runtime tool surface and ProjectAtlas agent setup documentation.

## Release Scope

This change is scheduled for the next version. It does not replace the atlas-first workflow; it gives agents a typed first snapshot and then recommends the existing lower-level calls.

## Non-Goals

- Do not run scans, create indexes, or repair configuration from `atlas_session_brief`.
- Do not read arbitrary source contents or emit slices/summaries inline.
- Do not hide existing `atlas_overview`, `atlas_folders`, `atlas_files`, `atlas_health`, or `atlas_next`.
- Do not add human marketing prose to the contract.

## Pre-Mortem

Likely failure modes:
- The brief grows so large that it erases token savings.
- The brief calls MCP handlers and accidentally records telemetry.
- The brief reimplements ranking or health logic and drifts from lower-level tools.
- Missing-index state is hidden behind optimistic recommendations.
- Wrong-root guidance is vague, causing agents to use ProjectAtlas outside the selected DB.

Mitigations:
- Hard-limit folder, file, blocker, and recommendation counts and include truncation metadata.
- Use store/service helpers directly; no internal `atlas_*` handler calls.
- Represent index state, path scope, blocker severity, and recommendation kind as serialized enums.
- Add tests for healthy, missing-index, query-guided, and blocker-producing projects.
