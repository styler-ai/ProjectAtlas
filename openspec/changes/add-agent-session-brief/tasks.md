## 1. Spec and Issue Setup

- [x] 1.1 Update the session brief OpenSpec proposal, design, spec delta, and task list for v0.3.25 implementation.
- [x] 1.2 Mirror this task list into #292 under `OpenSpec Tasks` and assign the release milestone.

## 2. MCP Implementation

- [x] 2.1 Add `atlas_session_brief` to the MCP tool surface and runtime tool list.
- [x] 2.2 Add typed request/response structs and enum-backed status, blocker, and recommendation fields.
- [x] 2.3 Compose selected project state, settings/index state, overview, ranking, and health data without calling telemetry-recording MCP handlers.
- [x] 2.4 Return typed missing-index guidance without creating `.projectatlas` or a database.

## 3. Tests and Documentation

- [x] 3.1 Add tests for healthy project brief, query-guided ranking, missing index, and blocker output.
- [x] 3.2 Update ProjectAtlas skill and agent integration docs for `atlas_session_brief`.
- [x] 3.3 Run OpenSpec, issue-checklist, MCP, and Rust verification gates.
