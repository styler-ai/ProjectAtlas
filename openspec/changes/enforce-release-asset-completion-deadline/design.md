## Context

Four receive branches in `e2e_delivery.rs` call `release_asset_server_completion_result`: response writes, the outer listener loop, stalled request reads, and idle listener waits. Pre-I/O expiry checks cannot classify a completion observed after those checks. The hosted macOS ARM stalled-request regression reached this race.

## Goals / Non-Goals

**Goals:** Preserve the server's absolute deadline at every completion decision, operation-specific diagnostics, timely completion behavior, and existing resource ownership.

**Non-Goals:** Timeout changes, scheduler control, new server or process abstractions, database work, installed-product behavior, and CI event routing.

## Decisions

- Extend the existing completion-result helper with the deadline and operation-specific timeout message. Check expiry before served-asset booleans. A guard only in the outer loop misses the other receive branches; a new state machine or synchronization framework is unnecessary.
- Keep pre-I/O checks for prompt expiry, check the outer loop before accepting work, and cap idle receive waiting at the smaller of remaining time and the existing polling interval.
- Extend the existing lifecycle test with expired/timely and complete/incomplete decision assertions. Use an already expired `Instant` rather than scheduler sleeps for this decision table. Preserve real stalled socket, response, and process cleanup cases; report the actual stalled error on failure.
- Reuse standard `Instant`, `Result`, `io::ErrorKind::TimedOut`, and the existing channel and RAII thread/process owners. Each completion adds constant work and no allocation on success; there is no persistent state, lock, pool, or public compatibility change.

## Risks / Trade-offs

- A sibling receive bypasses the decision -> inspect all four callers and cover the shared decision table.
- Expiry masks timely missing-request behavior -> retain future incomplete and successful cases.
- Local scheduling conceals the hosted race -> require deterministic causal failure before the fix and actual macOS ARM, Linux, and Windows lifecycle readback afterward.

## Migration Plan

Apply the focused helper correction and required frozen test-source identity updates, run existing required local gates, obtain independent review, and publish the accepted shared baseline before refreshing dependent work. No data or runtime migration is needed. Architecture diagrams are N/A because owners and flow are unchanged.

## Dependencies / Cross-Issue Impact

#564 belongs to release owner #492 and has no open implementation prerequisite. Its shared gate blocks acceptance of preserved #477 and #390 operationally without creating an artificial source dependency. Publish and reconcile this accepted baseline before their final delivery. #560 owns the independent metadata-event routing defect.

## Open Questions

None.
