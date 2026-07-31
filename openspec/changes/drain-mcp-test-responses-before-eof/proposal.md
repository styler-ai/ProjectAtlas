## Why

Two release-mandatory MCP E2E paths close stdin immediately after sending accepted requests, so RMCP's bounded post-EOF drain can discard a varying unfinished response under load. The stale-read gate exposed the race first; the v0.4.2 Windows release gate then lost the all-tools inventory response after the #409 persistent-session regression had passed. Release tests must observe their required responses deterministically instead of depending on shutdown timing.

## What Changes

- Reuse the existing persistent MCP contract session for the six stale-read adapter checks, the all-tools inventory, and each advertised-tool contract call.
- Complete the MCP initialization handshake and keep session input open until every required response arrives.
- Attempt explicit bounded shutdown after both successful validation and request/assertion failure.
- Preserve one MCP server and project binding plus every exact typed `refresh_required` assertion.
- Keep the release-mandatory persistent #409 and all-advertised-tool contracts deterministic.
- Keep production MCP, freshness, cancellation, storage, dependencies, and public contracts unchanged.

## Non-Goals

- Increasing RMCP's post-EOF drain timeout or changing production shutdown behavior.
- Changing the generic EOF-bounded batch helper or adding a concurrent-read contract to sequential release checks.
- Adding another session abstraction or changing the existing one-process-per-contract isolation.

This bugfix is ready for implementation in v0.4.2.

## Capabilities

### New Capabilities

- `mcp-test-session-lifecycle`: Defines response-before-shutdown and bounded cleanup behavior for persistent MCP E2E clients that verify multiple accepted requests.

### Modified Capabilities

None.

## Impact

The change is limited to `crates/projectatlas-cli/tests/e2e.rs` plus issue/OpenSpec routing. It changes no production crate, API, dependency, database schema/query/transaction path, runtime resource budget, or packaged artifact.
