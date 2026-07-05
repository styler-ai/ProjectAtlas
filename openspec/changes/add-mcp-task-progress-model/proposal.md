## Why

Some MCP operations can become long-running as repositories grow, especially scan, watch refresh, and broad search. Agents would benefit from a bounded progress model before any operation is made non-blocking, but this should be designed separately from path-routing hardening.

## What Changes

- Define a minimal MCP task-progress model for long ProjectAtlas operations.
- Model task states such as pending, running, complete, failed, and canceled with typed serialized fields.
- Keep CLI command behavior unchanged.
- Keep persistence and cross-process job management out of scope until a concrete need is proven.
- Backlog status: this proposal is for review only and is not planned for the current release until approved.

## Capabilities

### New Capabilities
- `mcp-task-progress`: Defines a bounded task-progress contract for long-running MCP operations without changing CLI behavior.

### Modified Capabilities

## Impact

- Expected code touch points after approval: a small MCP task/status module, `crates/projectatlas-cli/src/mcp.rs`, and serialization tests.
- Expected operations affected after approval: scan, watch refresh, and broad search only if they are explicitly moved to task-backed execution.
- This is intentionally separate from `harden-project-path-types` so routing safety remains small and release-ready.
