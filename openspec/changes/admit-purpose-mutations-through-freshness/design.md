## Context

Queue reads already pass through the saved-source freshness boundary. Conditional purpose work keys bind project identity, indexed generation, task, path, and purpose-row state. SQLite then applies a batch atomically and rejects generation or row conflicts.

The CLI and MCP apply paths bypass saved-source admission and open the existing database writer directly. If repository source changed without a publication, the database still matches the old token and accepts it. A later scan preserves the result as authored intent, which is correct storage behavior but now protects an outdated purpose.

## Goals / Non-Goals

Goals:

- reconcile exact current saved source once per mutation batch and verify it again at the final precommit point;
- let the existing generation check reject old conditional work;
- retain explicit current corrections and unchanged approvals;
- prove the existing same-binding queue, no-op watch, and queue path remains convergent across CLI and MCP;
- preserve atomicity, cancellation, root, policy, and output contracts.

Non-goals:

- source hashes in purpose tokens or SQLite;
- automatic demotion of approved authored purpose after every edit;
- a new mutation framework, background reconciler, watcher, trait, crate, or dependency;
- stronger filesystem transaction claims than the existing scan publication boundary.

## Dependencies / Cross-Issue Impact

This change reuses schema-18 purpose transactions, source reconciliation, native observation, and adapter cancellation. It does not depend on #461 or #462 behavior and introduces no cross-issue schema, crate, or package change; the three fixes share only RC2 release proof.

## Migration Plan

No database or configuration migration is required. The derivation, purpose-token, and MCP payload formats remain unchanged; RC2 replaces the mutation admission behavior in place.

## Risks / Trade-offs

- Exact admission and the final detect-only witness hash repository source twice per mutation batch and hold the SQLite writer during the second pass.
- Arbitrary filesystem writers cannot share SQLite atomicity, so the final exact witness is the declared linearization point and observer ingress is an additional fail-closed signal.
- Explicit rollback can surface a storage failure in addition to the initiating mutation error; preserving both is preferable to silently hiding possible integrity loss.

## Open Questions

None for RC2. A future measured optimization may replace the second full-source comparison only if it supplies an equally strong saved-source fence without trusting asynchronous notification timing.

## Decisions

### Retain exact admission through the final precommit witness

Each purpose set or applied review forces one exact freshness admission for the selected project and complete batch; an empty warm watcher queue is not mutation authority. The adapter retains that source, policy, identity, continuity, and cancellation witness while it opens the existing writer, begins one purpose transaction, applies the batch, performs one exact detect-only saved-source comparison as the final precommit linearization point, synchronizes request cancellation, and explicitly commits. Any failed revalidation explicitly rolls the transaction back and changes no purpose row or authored revision. Persistent MCP reuses its `SourceObservationRegistry`, while a CLI invocation owns one process-local registry for the same contract.

Preview-only review remains a fresh read and does not open a writer. Mutation admission is never repeated per purpose row.

### Keep the existing conditional transaction authoritative

When admission discovers source changes, the existing bounded incremental or full repair publishes a current generation before the purpose transaction begins. Old queue work then conflicts on its existing generation-bound token. If exact saved source, policy, observer continuity, or cancellation differs at the final precommit witness, the adapter refuses the explicit commit and rolls the transaction back. Both paths return typed state, change no purpose row, and do not advance authored-purpose revision. Current explicit set/review and requeued conditional work use the same transaction.

SQLite cannot transact with arbitrary external filesystem writers. The declared mutation linearization point is therefore the final exact saved-source witness immediately before commit, followed by a synchronous request-cancellation fence; observer ingress remains an additional fail-closed signal rather than the filesystem authority.

No source hash is added to the token: a hash stored in an unrefreshed database cannot prove current saved source.

### Preserve authored-purpose durability

Approved purpose remains authored responsibility and survives unchanged full and incremental scans. A file edit does not automatically erase or demote it. The fix prevents stale source-derived approval at admission; semantic correction after a later intentional edit remains an explicit curator action.

### Protect no-op watch and queue verification without an unproven observer change

The exact persistent-MCP queue, zero-candidate watch, and queue sequence converges under the same root/database binding in RC1 and current source, with and without telemetry. This change therefore does not alter source-observer acknowledgement, event filtering, or target routing. A focused regression follows the returned target binding and proves that unchanged work retains the same generation-bound queue identity across the no-op watch.

When a native observer cannot start or the bounded process registry is full, mutation admission uses the existing exact-per-call compatibility path instead of making watcher availability a product requirement. It retains the admitted generation and project identity, then requires exact saved-source agreement plus the same durable identity immediately before commit. The observer capacity, eviction policy, and routing remain unchanged.

## Rust Pattern Fit

The existing closed purpose apply state, freshness classifier, observer registry, and a concrete RAII SQLite transaction guard remain the correct mechanisms. A private sum type retains either an observed epoch or an exact-per-call generation/project witness without optional-state ambiguity. CLI and MCP keep thin adapter-owned closures because MCP owns worktree aliases and request cancellation while CLI owns process-local project selection. No cross-adapter trait, worker, channel, or dependency is justified.

## Database And SQLite Fit

No schema or query change is needed. SQLite continues to own project identity, generation checks, purpose rows, atomic batches, and authored revision. The caller begins one immediate transaction through the existing connection, performs cached purpose statements, revalidates the retained source witness, and commits explicitly; rejected operations roll back explicitly and drop remains the last-resort fallback. The runtime continues to own saved-source inspection and derived publication. Busy, read-only, cancellation, continuity, policy, and post-admission source failures leave the transaction uncommitted.

## Performance And Bounds

Freshness work is one exact repair-capable admission plus one exact detect-only precommit witness per batch, never per purpose row. Persistent MCP retains its observer epoch for continuity and policy evidence but does not use an empty event queue as mutation authority; observer-unavailable calls perform the same bounded exact work without allocating another watcher. Separate CLI invocations establish the same process-local contract. Changed source uses existing incremental repair and established full-scan fallback. The purpose writer holds SQLite's single-writer slot for the bounded batch and final witness check; there is no new scanner, per-row filesystem I/O, WAL state, or persistent data.

## Failure And Concurrency

- A changed generation makes the complete old conditional batch stale with no partial writes.
- Deleted, renamed, newly ignored, and wrong-root paths retain the existing reconciliation and refusal behavior before explicit mutation.
- Busy/read-only repair, cancellation, policy drift, exact-source drift, observer continuity loss, or exact-fallback identity drift returns typed recovery and never certifies a purpose.
- Existing concurrent-curator winner, replay, root identity, and authored-revision semantics remain unchanged.
- Deterministic unobserved edits and synchronous request cancellation prove that the final precommit witness rolls the complete purpose batch back.

## Verification

Owning tests edit saved source between queue and apply and prove generation advance, stale result, unchanged purpose/revision, successful requeue, replay, scan retention, and final convergence. A saturated observer registry additionally proves exact-fallback success, cancellation, unpublished drift rollback, and concurrent-publication generation rejection without changing registry occupancy. CLI subprocess and persistent MCP E2E repeat the workflow on real saved files. The persistent session separately proves same-binding queue, zero-candidate watch, and queue convergence without claiming an observer repair. Existing negative, failure, and concurrency suites remain the compatibility boundary. The focused E2E runs in the existing Linux, Windows, and macOS matrix and the packaged RC agent workflow.
