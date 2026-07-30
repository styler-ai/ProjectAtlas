## Context

Impact analysis already receives an aggregate request control with deadline, cancellation, row, node, edge, visited-state, intermediate-byte, and output-byte limits. The control is captured at the service boundary, but the optional dead-code path performs candidate discovery and hydration through helpers that do not consistently check or forward it. A small request can therefore retain a SQLite read snapshot and CPU work until the MCP host's unrelated timeout.

The graph is rebuildable SQLite state. Analysis is read-only and candidate-labeled; it must never publish partial authoritative data. CLI and MCP compose the same service result.

## Goals / Non-Goals

**Goals:**

- Use one captured deadline and cancellation source across every impact/dead-code phase.
- Check control before and during bounded database reads, traversal, hydration, composition, and rendering.
- Return the existing typed deadline, cancellation, or truncation result within a bounded tolerance.
- Drop statements, snapshots, task records, and intermediate collections before return.
- Preserve successful analysis behavior and deterministic bounded output.

**Non-Goals:**

- Creating a background analysis executor or another task registry.
- Returning unbounded partial candidates after expiry.
- Changing graph schema, indexes, publication, or MCP request fields; the CLI only exposes aggregate limits the service and MCP route already accept.
- Converting synchronous SQLite reads to async work.

## Decisions

### Thread the existing concrete request control

Pass the existing analysis/request control by shared reference into dead-code candidate discovery, usage indegree reads, admitted-symbol hydration, node composition, and final accounting. Each loop and each database batch checks the same absolute deadline and cancellation state.

A new trait, token type, or nested timeout was rejected because the existing control already owns all required limits and typed errors. Relative per-phase timers were rejected because they could extend the caller's total deadline.

### Bound database work at the storage boundary

Reuse controlled, prepared, paged database queries where available. If a current helper materializes a complete symbol or usage set, add the smallest controlled page/batch boundary and check between batches. The service owns orchestration; SQL and statement lifetime remain in `projectatlas-db`.

### Fail through existing typed result composition

Deadline and cancellation propagate through the normal analysis result mapping. Safe completed rows may be retained only when the existing result contract marks coverage and total state accurately; no failure becomes an exact or authoritative answer. RAII drops the snapshot and statements before adapter serialization.

### Test elapsed behavior without millisecond brittleness

Deterministic test hooks expire control at named analysis stages. Wall-clock integration tests use a generous upper tolerance relative to the requested deadline and require an immediate harmless follow-up call. Both CLI and MCP tests exercise the real adapter route.

## Risks / Trade-offs

- **A helper remains unbounded between checks** → Cover expiry during candidate discovery, SQLite iteration, traversal, hydration, and rendering with stage-specific tests.
- **Cancellation returns but retains a read snapshot or task record** → Reopen/write after expiry and require immediate status/read responsiveness.
- **Tiny deadline tests are flaky on loaded CI** → Use deterministic stage hooks for phase coverage and tolerant elapsed assertions only at adapter boundaries.
- **Checks add hot-loop overhead** → Reuse the existing cheap control check at batch/loop boundaries; work remains bounded by requested nodes, edges, rows, bytes, and deadline.

## Migration Plan

No durable migration is required. Ship the service and storage-boundary fix in v0.4.1 with CLI/MCP compatibility tests.

## Open Questions

None.
