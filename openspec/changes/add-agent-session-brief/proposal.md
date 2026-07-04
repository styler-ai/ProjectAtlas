## Why

ProjectAtlas gives agents strong low-level MCP tools, but startup still requires several separate calls before an agent knows whether the root, index freshness, relevant folders, relevant files, and health state are safe. A first-class session brief would reduce startup ceremony and make wrong-root or stale-index mistakes easier to catch before source reads.

## What Changes

- Add an `atlas_session_brief` MCP tool that composes existing settings, overview, ranking, and health signals into one compact agent startup payload.
- Accept an optional task query and optional `project_path` so the brief can recommend relevant folders/files without changing active project state.
- Return typed TOON/JSON-compatible fields for project identity, index state, ranked candidates, blockers, and recommended next calls.
- Keep the tool read-only and bounded; it must not scan, create indexes, or read source file contents directly.
- Backlog status: this proposal is for review only and is not planned for the current release until approved.

## Capabilities

### New Capabilities
- `agent-session-brief`: Defines the MCP startup brief contract for active project identity, index freshness, relevant navigation candidates, blockers, and next-call recommendations.

### Modified Capabilities

## Impact

- Expected code touch points: `crates/projectatlas-cli/src/mcp.rs`, ranking/health/runtime helper modules, runtime-info/settings response structs, and MCP contract tests.
- Expected docs touch points: ProjectAtlas skill instructions and agent integration docs if the contract is approved.
- No dependency change is expected; the feature should compose existing Rust services.
