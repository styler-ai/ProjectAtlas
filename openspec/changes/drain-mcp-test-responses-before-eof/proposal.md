## Why

The MCP freshness E2E closes stdin immediately after sending six accepted read requests, so RMCP's bounded post-EOF drain can discard a varying unfinished response under load. The v0.4.2 gate must test stale-read behavior deterministically instead of depending on shutdown timing.

## What Changes

- Reuse the existing persistent MCP contract session for the six stale-read adapter checks.
- Keep the session input open until each accepted request receives its response, then attempt explicit bounded shutdown on both success and failure.
- Preserve one MCP server and project binding plus every exact typed `refresh_required` assertion.
- Keep production MCP, freshness, cancellation, storage, dependencies, and public contracts unchanged.

## Non-Goals

- Increasing RMCP's post-EOF drain timeout or changing production shutdown behavior.
- Adding a concurrent-read contract to this stale-state test.
- Adding another session abstraction or launching one server per tool.

This bugfix is ready for implementation in v0.4.2.

## Capabilities

### New Capabilities

- `mcp-test-session-lifecycle`: Defines response-before-shutdown and bounded cleanup behavior for persistent MCP E2E clients that verify multiple accepted requests.

### Modified Capabilities

None.

## Impact

The change is limited to `crates/projectatlas-cli/tests/e2e.rs` plus issue/OpenSpec routing. It changes no production crate, API, dependency, database schema/query/transaction path, runtime resource budget, or packaged artifact.
