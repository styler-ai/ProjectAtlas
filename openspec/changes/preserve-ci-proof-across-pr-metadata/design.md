## Context

The existing source workflow already distinguishes base retargets from title/body
edits. However, skipping its dynamically named result job prevents GitHub from
evaluating that name: the hosted check exposes the literal expression instead of
`metadata-edit`. Earlier blocked merge readiness was observed alongside that
defect; the check-name defect has not been established as its cause. Hosted
acceptance must independently inspect source-check identity and native protection
instead of treating any blocked merge state as a source-check failure.

## Goals / Non-Goals

Keep source proof and PR-state validation independent. Preserve exact retarget
binding, existing planner/aggregate semantics, protected check names and app
identity, and cancellation isolation. Do not add a workflow, proof ledger,
dependency, status writer, or additional source execution for metadata activity.

## Decisions

Schedule the existing result job with `always()` so its dynamic name can resolve
even when all dependencies are skipped. Move the existing source-event condition
to both source-owning steps: checkout and aggregate. For ordinary metadata the job
is named `metadata-edit` and executes no source steps; for base retargets and other
source events it is named `verify` and executes the unchanged aggregate.

Retain the direct `pull_request.edited` subscription and source concurrency key.
Moving retarget proof into a reusable PR-state job would let a later metadata run
overwrite failed retarget readiness and would replay source CI when issue events
rerun PR-state. A dispatcher or stored proof lookup would introduce another owner
without fixing the demonstrated skipped-name boundary.

## Risks / Trade-offs

- GitHub may still associate checks unexpectedly: require real title/body edits,
  exact-base retarget execution, and protected readiness before acceptance.
- A misplaced step guard could execute or satisfy source proof on metadata:
  existing workflow-contract tests must inspect both guards and actual job names.
- Metadata overlapping failed or running retarget work must not replace `verify`:
  exercise this negative case at the native hosted check boundary.
- Scheduling the metadata result incurs a small hosted job startup cost; it
  performs no checkout, planner, build, or test work.

## Migration Plan

Deliver the minimal workflow/test change through one normally protected PR.
Read back source checks, metadata checks, and base/head bindings on actual events.
No data migration or branch-protection update is needed. A source revert restores
the prior event behavior; do not compensate with bypass statuses or relaxed gates.

## Architecture

N/A: Existing CI planner, selected proof jobs, aggregate, PR-state validation, and
native protection owners are unchanged. Only job-versus-step condition placement
changes within the existing aggregate.

## Dependencies / Cross-Issue Impact

Issue #560 is a direct child and blocker of release owner #492. The accepted #555
affected-proof implementation remains the source authority on main. There is no
open implementation prerequisite; other release issues retain separate scopes.

## Open Questions

None.
