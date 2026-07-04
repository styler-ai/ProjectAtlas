## Why

Issue #281 requires ProjectAtlas agents to use MCP for normal ProjectAtlas command families instead of shelling out whenever the server is already available. The current MCP code inspection shows the new admin/reporting parity tools are not exposed yet, so this change defines the minimal contract before implementation lands.

## What Changes

- Add MCP parity coverage for safe CLI families: `init`, root show/verify/set, `config --print`, ignore list/init-gitignore/add/remove, `lint`, `runtime-info`, `mcp-config`, and `map`.
- Require project-root-sensitive parity tools to accept per-call `project_path`.
- Keep TOON-first, bounded agent responses and reuse the same services as CLI handlers.
- Define reviewed exceptions for MCP server startup, continuous watch, and terminal TUI output.
- Ready for implementation under #281.

## Non-goals

- Do not expose installer internals, release automation, or CI orchestration as MCP tools.
- Do not start a nested MCP server from an MCP request.
- Do not make continuous watch block an MCP request without a separate lifecycle contract.
- Do not expose terminal UI rendering as an MCP payload.

## Capabilities

### New Capabilities

- `mcp-cli-parity`: Defines safe CLI command-family parity, reviewed exceptions, project path behavior, and documentation expectations.

### Modified Capabilities

## Impact

- Expected code touch points after approval: `crates/projectatlas-cli/src/mcp.rs`, parity inventory/tests, and existing CLI service helpers only.
- Expected docs touch points: `AGENTS.md`, `templates/AGENTS.md`, `docs/agent-integration.md`, and the ProjectAtlas plugin skill.
