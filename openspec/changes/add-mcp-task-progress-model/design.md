## Context

ProjectAtlas currently exposes synchronous CLI and MCP operations. That is simple and reliable, but long operations can block an MCP request. A task-progress model should be small, typed, and explicit before ProjectAtlas moves any long-running MCP operation to asynchronous execution.

## Goals / Non-Goals

**Goals:**
- Define typed task states and status payloads for long MCP operations.
- Preserve existing CLI behavior.
- Provide a path to status polling and cancellation for MCP clients.

**Non-Goals:**
- Do not implement persistent job queues.
- Do not add cross-process task state.
- Do not make scan/watch/search non-blocking until the task contract is approved.
- Do not couple task progress to nearest-project routing.

## Decisions

- Start with a typed contract before implementation. The minimum useful states are `pending`, `running`, `complete`, `failed`, and `canceled`.
- Keep task records in memory if implemented; persistence can be a later proposal.
- Scope initial candidates to MCP scan, watch refresh, and broad search. Normal file summary and slice calls should remain direct.

## Risks / Trade-offs

- A task model can become a full job system -> defer persistence and cross-process ownership.
- Polling can add MCP surface area -> keep status/cancel tools minimal and typed.
- Async execution can hide failures -> status payloads must include typed failure state and concise diagnostics.
