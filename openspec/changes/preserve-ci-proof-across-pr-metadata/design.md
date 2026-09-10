## Context

The source workflow distinguishes base retargets from title/body edits. A native
body-only edit nevertheless changes a clean PR to blocked when its new CI run has
no `verify` job. Scheduling a correctly named `metadata-edit` and rerunning the
previous source aggregate both leave it blocked. Required-check lists alone do
not expose this failure; hosted acceptance must inspect protected readiness.

## Goals / Non-Goals

Keep source proof and PR-state validation independent. Preserve exact retarget
binding, existing planner/aggregate semantics, protected check names and app
identity, and cancellation isolation. Do not add a workflow, proof ledger,
dependency, status writer, or additional source execution for metadata activity.

## Decisions

Keep `verify` as the native required job on every CI event. Source events execute
the existing exact planner, selected jobs, and aggregate. Metadata only fetches
the verification helper at the captured accepted base and reads native Actions results. It does not execute a
source plan, build, test, or aggregate, or rerun an earlier job.

For PR source events, `run-name` captures the PR number and full base/head SHAs from
the event. The native run also records its head and workflow identity.
Metadata selects the latest source run for that PR/head in the same workflow;
its captured base must match both the metadata event and the live PR. Require a
successful terminal source run. Pending source work may be observed with bounded
waiting; failed, cancelled, skipped, missing, stale, malformed, inaccessible, or
ambiguous evidence cannot pass. Recheck the live comparison and latest source run
before acceptance. Never borrow an older success over a newer incomplete run.

Use the existing proof script, Python standard library, and authenticated `gh`.
Bound run discovery to 100 matching-head runs and waiting to 120 minutes. These
limits fail closed; there is no new database, artifact, status API, or dispatcher.
Native run-name binding must be verified across real title/body edits, base
retargets, and reruns; an API run's `pull_requests` base/head association is mutable
and is explicitly excluded from historical proof.

Retain the direct `pull_request.edited` subscription and source concurrency key.
Moving retarget proof into PR-state would replay source CI when issue events rerun
PR-state and complicate required-check ownership. Revalidate the native source
run in the existing CI owner instead.

## Risks / Trade-offs

- GitHub may still associate checks unexpectedly: require real title/body edits,
  exact-base retarget execution, and protected readiness before acceptance.
- A misplaced guard or stale lookup could execute source work or accept unrelated
  proof on metadata: causal tests must cover routing, binding, latest-run choice,
  failure, bounded waiting, API refusal, and changing live comparisons.
- Metadata overlapping failed or running retarget work must not replace `verify`:
  exercise this negative case at the native hosted check boundary.
- A metadata edit during source CI waits for that existing run; it incurs a small
  helper fetch and read-only API work, without replaying builds or tests.

## Migration Plan

First deliver the verifier command and its failure-path tests through normal
protected source CI, keeping the owning issue open. Then refresh the existing
worktree onto that accepted base and activate metadata verification in a second
PR for the same issue. The activation must fetch only the captured base helper;
missing support fails closed without executing the submitted head helper.
This ordering establishes trusted code before it decides metadata readiness.
Read back source checks, metadata checks, and base/head bindings on actual events.
No data migration or branch-protection update is needed. A source revert restores
the prior event behavior; do not compensate with bypass statuses or relaxed gates.

## Architecture

N/A: Existing CI, PR-state, and native protection retain their ownership. The
required CI job gains a read-only path to its own native source-run history; no
new workflow, persistent store, or product architecture is introduced.

## Dependencies / Cross-Issue Impact

Issue #560 is a direct child and blocker of release owner #492. The accepted #555
affected-proof implementation remains the source authority on main. There is no
open implementation prerequisite; other release issues retain separate scopes.

## Open Questions

None.
