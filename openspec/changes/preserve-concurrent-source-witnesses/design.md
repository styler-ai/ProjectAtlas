## Context

Observed reads and purpose mutations share one process-local source epoch per exact root/database/config binding. Mutation admission deliberately performs exact source reconciliation. An older read can then reject its superseded epoch and unconditionally clear the newer mutation witness. The deterministic interleaving reproduces the hosted purpose-review refusal without changing source.

## Goals / Non-Goals

**Goals:** Preserve newer valid evidence when an older reader retries, while retaining exact-source mutation admission, source-change refusal, transaction rollback, cancellation, and bounded work.

**Non-Goals:** No persisted state, public wire change, timeout or retry-budget increase, implicit refresh, or observer bypass.

## Decisions

Keep acceptance classification inside the existing source-observation owner. A small private closed enum distinguishes acceptance, supersession, and invalidated source evidence. Callers discard superseded results without clearing another operation's valid epoch. Actual source/policy/continuity invalidation still invalidates the shared evidence and rejects mutation publication.

Conditional invalidation based only on epoch equality is insufficient: a stale reader may drain a genuine event that also invalidates the newer witness. Classification must preserve the reason for rejection, and event consumption must not leave invalid evidence reusable. The implementation must also preserve lock ordering and avoid clearing a subsequently reconciled epoch with stale cleanup.

Exact preparation alone holds the reconciliation mutex because repair may need a SQLite writer. Mutation acceptance runs inside the purpose write transaction and must never acquire that mutex. Event drain and final epoch installation instead share the existing receiver-to-state lock order, with SQLite reads and policy sampling outside that final gate. Before each acceptance drain, the sampled epoch must still be current; otherwise its old selection policy could discard a source event relevant to a successor. A consumed relevant event retains continuity invalidation until the next exact verification starts, so another consumer cannot hide it from a verification already in flight. If reconciliation replaces an epoch during policy sampling, acceptance resamples within the existing attempt limit.

Always using exact-only mutation witnesses would bypass the established observed path and change its performance behavior. Keep the existing exact fallback for its existing admission conditions. No new worker, long-held mutex, or persistent table is justified.

## Risks / Trade-offs

- Confusing supersession with source change could permit stale publication. Pair the no-change interleaving with source-event and policy/continuity invalidation cases and real purpose rollback.
- Unconditional cleanup after a stale result could still clear a newer epoch. Inspect every acceptance consumer and preserve generation/identity checks.
- Concurrency tests based on sleeping would be nondeterministic. Arrange the interleaving through existing read closures and observer test seams.

## Migration Plan

No migration. Land the shared fix before refreshing dependent #339. Run the complete existing MCP payload fixture without added refreshes, serializing its requests, or disabling telemetry, then obtain required hosted platform proof. Reverting the code requires no data conversion.

## Architecture

N/A for a new diagram: root/database/config ownership, source observation, and purpose transaction boundaries are unchanged. This corrects acceptance and invalidation inside the existing owner.

## Dependencies / Cross-Issue Impact

No implementation prerequisite. #586 is a direct child and blocker of #492 and blocks #339. Land the shared observer fix before refreshing the PHP guidance branch. #574 remains closed because its database identity correction is intact.

## Open Questions

None.
