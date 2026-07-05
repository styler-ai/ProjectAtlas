## Why

Agents need to know the MCP server's startup policy before trusting path-sensitive calls. Today settings are available, but startup flags such as nearest-project routing, telemetry mode, selected DB/config roots, and scan policy are not exposed as one typed capability contract.

## What Changes

- Add a typed MCP capability/settings response that reports server startup flags and effective project identity.
- Include `nearest_project` startup policy, selected project root, DB path, config path, scan policy, telemetry mode, and MCP/runtime version fields.
- Make the response structured and snapshot-testable so harnesses can detect drift before reading files.
- Backlog status: this proposal is for review only and is not planned for the current release until approved.

## Capabilities

### New Capabilities
- `mcp-capability-settings`: Defines a typed MCP capability/settings contract for startup flags, selected project identity, scan policy, telemetry mode, and runtime identity.

### Modified Capabilities

## Impact

- Expected code touch points: `crates/projectatlas-cli/src/mcp.rs`, settings/runtime-info structs, TOON rendering helpers, and MCP tests.
- Expected docs touch points: MCP setup instructions, ProjectAtlas plugin skill, and agent integration docs if approved.
- No external dependency is expected; Rust `Serialize` structs/enums should model the contract.
