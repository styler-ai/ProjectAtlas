## Context

Current MCP calls are synchronous. That simplicity is valuable, but future scans, watch refreshes, and broad searches need a standard way to report progress if they become task-backed. This change defines the model and minimal tool surface first.

## Contract

The task model includes:

- `task_id`: opaque MCP-session-local id.
- `operation`: enum such as `scan`, `watch_once`, `search`, or `contract`.
- `state`: `pending`, `running`, `complete`, `failed`, or `canceled`.
- `created_at_ms` and `updated_at_ms`: monotonic-enough wall-clock timestamps for status ordering.
- `progress`: optional bounded progress fields (`current`, `total`, `message`).
- `error`: optional concise failure diagnostic.
- `result_ref`: optional text reference for completed task output location or follow-up call.
- `cancelable`: boolean.

The MCP surface includes:

- `atlas_task_status`: accepts `task_id` and returns a typed status. Unknown ids return a typed `not_found` status rather than a transport failure.
- `atlas_task_cancel`: accepts `task_id` and returns typed cancellation state. Unknown ids return `not_found`; running non-cancelable tasks return `not_cancelable`.

The initial registry may be empty because no long operation is moved async in this release. That is intentional: the stable contract lands before operation migration.

## Implementation Notes

- Keep the registry on `ProjectAtlasMcpServer` as a bounded in-memory collection protected by `RwLock` or `Mutex`.
- Keep the capacity small and evict oldest completed records when needed.
- Add one contract/status record only if useful for testing; do not create fake background work.
- Encode responses through the existing TOON payload helpers.
- Do not add fields to CLI runtime-info.

## Edge Cases

- Unknown task id: return `state: not_found` or a typed not-found result without panicking.
- Cancel unknown id: return `not_found`.
- Cancel non-cancelable task: return `not_cancelable`.
- No task-backed operations yet: task tools still advertise the model and return clear typed states.
- Summary/slice calls: remain direct.

## Pre-Mortem

Risk: users expect cancellation to interrupt scans immediately.
Mitigation: document that no current operation is task-backed in this release and that future cancellation is best-effort unless checkpoints are added.

Risk: empty registry feels useless.
Mitigation: the value is the stable contract and harness readiness; operation migration follows only when backed by tests.

Risk: registry locking complicates the MCP server.
Mitigation: keep it a tiny helper with bounded records and no background workers.
