## Why

Agents often address an absolute file or folder path inside another already-indexed repository. Requiring a manual `atlas_set_project_path` or `project_path` switch for every such call is noisy, but silently reading a different project by default is unsafe.

## What Changes

- Add an opt-in nearest-project routing policy for MCP startup and per-call overrides.
- Discover the nearest ancestor that contains a valid `.projectatlas/projectatlas.db` without creating or mutating candidate databases.
- Preserve explicit `project_path` isolation ahead of nearest-project routing.
- Return normal filesystem guidance when no valid indexed ProjectAtlas database is found.
- Include selected-project audit metadata on cross-project read responses so agents can see which root and DB served the call.

## Capabilities

### New Capabilities
- `nearest-project-routing`: Defines opt-in nearest indexed ProjectAtlas DB discovery for MCP path-bearing tools.

### Modified Capabilities

## Impact

- Expected code touch points: `crates/projectatlas-cli/src/mcp.rs`, `crates/projectatlas-cli/src/main.rs`, `crates/projectatlas-cli/tests/e2e.rs`, and `crates/projectatlas-db/src/lib.rs`.
- Expected documentation touch points: `AGENTS.md`, `templates/AGENTS.md`, `docs/agent-integration.md`, and `plugins/projectatlas/skills/projectatlas/SKILL.md`.
- Expected verification: focused MCP routing tests, read-only DB-probe tests, full Rust gates, and ProjectAtlas lint.
