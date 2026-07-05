## Why

Some MCP operations can be long-running in large repositories, but ProjectAtlas should not jump straight to a daemon or persistent job queue. The next version should provide a small typed task-progress model that future task-backed MCP operations can use, while keeping existing CLI commands and direct MCP source reads synchronous.

## What Changes

- Add a bounded MCP task-progress contract with typed task states, operation kinds, status payloads, and cancel semantics.
- Expose minimal MCP status/cancel tools for task ids.
- Keep task state in memory only and bounded by a small capacity.
- Do not move scan, watch refresh, search, summary, or slice behind async polling in this release.
- Preserve existing CLI behavior.

## Capabilities

### New Capabilities
- `mcp-task-progress`: Defines task states, task status payloads, cancellation responses, and bounded in-memory task registry behavior for future long-running MCP operations.

### Modified Capabilities
- MCP runtime tool surface and ProjectAtlas agent setup documentation.

## Release Scope

This change is scheduled for the next version as a contract and minimal MCP surface. It deliberately stops before async scan/watch/search because those need operation-level cancellation checkpoints and SQLite write contention tests.

## Non-Goals

- Do not add persistent task queues, cross-process ownership, daemons, or background scan/watch/search execution.
- Do not change CLI scan/watch/search behavior.
- Do not make direct file summary or slice require polling.
- Do not claim cancellation interrupts synchronous operations that do not have cancellation checkpoints.

## Pre-Mortem

Likely failure modes:
- The model grows into a generic job system.
- Cancellation lies by claiming to interrupt running synchronous work.
- Status tools are unbounded and leak old task records forever.
- The contract uses prose states instead of stable enum values.
- Future async work contends on SQLite without a clear status/failure path.

Mitigations:
- Keep the registry in-memory, bounded, and MCP-session local.
- Return `not_found` or typed cancel results for unknown/non-cancelable tasks.
- Use enum-backed `pending`, `running`, `complete`, `failed`, and `canceled` values.
- Add tests for serialization, unknown task status, cancellation response, and CLI/direct-call preservation.
