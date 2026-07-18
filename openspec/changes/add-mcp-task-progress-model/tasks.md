## 1. Spec and Issue Setup

- [x] 1.1 Update the task-progress OpenSpec proposal, design, spec delta, and task list for v0.3.25 implementation.
- [x] 1.2 Mirror this task list into #295 under `OpenSpec Tasks` and assign the release milestone.

## 2. MCP Implementation

- [x] 2.1 Add typed task status, operation, progress, cancellation, and lookup result structs/enums.
- [x] 2.2 Add a bounded MCP-session-local task registry without persistent queues or background workers.
- [x] 2.3 Expose `atlas_task_status` and `atlas_task_cancel` MCP tools with typed unknown-task and non-cancelable responses.
- [x] 2.4 Keep scan, watch, search, summary, slice, and CLI commands synchronous in this release.

## 3. Tests and Documentation

- [x] 3.1 Add tests for task status serialization, unknown task status, cancellation response, and direct summary/slice preservation.
- [x] 3.2 Update ProjectAtlas skill and agent integration docs for the task-progress contract and current non-async scope.
- [x] 3.3 Run OpenSpec, issue-checklist, MCP, and Rust verification gates.
