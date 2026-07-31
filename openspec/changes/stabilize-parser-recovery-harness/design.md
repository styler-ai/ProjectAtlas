## Context

The adversarial parser-supervisor harness uses a short no-progress budget to keep hostile stalls bounded. Reusing that same budget for the healthy post-failure recovery probe made the Windows release gate sensitive to ordinary launch scheduling.

## Goals / Non-Goals

**Goals:**

- Separate the healthy recovery allowance from hostile-case stall budgets.
- Preserve one healthy launch attempt and its existing two-second deadline.
- Retain exact hostile failure classification, containment, and cleanup coverage.

**Non-Goals:**

- Change production parser deadlines, protocol, cancellation, containment, or cleanup.
- Add retries or widen hostile-case bounds.

## Decisions

- Override only the existing healthy recovery case's no-progress value.
- Set the recovery allowance to the healthy attempt's existing deadline, so the premature cutoff is removed without lengthening that attempt.
- Keep the existing adversarial suite as the owning verification surface.

## Risks / Trade-offs

- [A broad timeout change weakens hostile assertions] → Leave the shared hostile default unchanged.
- [A retry masks leaked state] → Keep exactly one healthy recovery attempt.
- [Loaded Windows hosts remain variable] → Verify repeatedly on Windows and retain ordinary cross-platform CI.
