## Context

The opposite-migration fixture releases two real installers after their discovery gates, then waits for each child with an existing 35-second bounded observer. It also rejects total wall time at 30 seconds before checking the two exit statuses. Under a full Windows suite, scheduling and PowerShell work can cross that wall threshold without proving a full lock wait. In the held-second case, a 250 ms product lock timeout is tested using an outer five-second process observer; hosted load once exhausted that observer while the child reported the intended lock refusal.

## Goals / Non-Goals

**Goals:** Keep real-process lock-order, held-lock refusal, state preservation, recovery, and bounded cleanup proof, with diagnostics that distinguish product failure from observer delay.

**Non-Goals:** Change production lock budgets, serialize the suite, add retries or sleeps, or expand installer behavior.

## Decisions

- Remove the redundant aggregate 30-second migration assertion. Both installers must exit successfully inside their existing finite per-child observer limits, and the following state/config assertions remain the causal lock-order proof. The aggregate clock includes unrelated process work and has no distinct product contract.
- Give the held-second contender the same finite installer-process observation envelope already used for its held owner. Its configured 250 ms lock timeout and non-success/status/state assertions remain unchanged; an outer host-process budget is not the lock deadline.
- Keep the shared observer implementation and production scripts untouched unless further reproduction proves a defect there. Avoid a new timing helper or global test serialization.

## Risks / Trade-offs

- [Risk] A slow true deadlock is hidden by removing the 30-second check. → The per-child bounded observers still fail and reap exact owned processes, while success requires both completed migrations and correct managed state.
- [Risk] A longer outer observer hides a broken 250 ms refusal. → Assert the child failure and lock-specific output/trace, plus unchanged state and recovery; the outer envelope only permits startup/collection scheduling.

## Migration Plan

Test-only change; no user state or schema migration. Revert the narrow test edit if a causal product lock fault emerges, then fix the product owner separately.

## Dependencies / Cross-Issue Impact

#633 is an RC3 child of #492. Its gate failure blocks the required #624 push, but it changes neither #624's DOCX implementation nor the independent #625 PDF and #627 map scopes.

## Open Questions

None.
