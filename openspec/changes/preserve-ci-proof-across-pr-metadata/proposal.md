## Why

A body-only edit changes a merge-ready pull request to blocked when the new CI
run omits the required `verify` job, even when the previous source verification
and current PR-state checks pass. Correcting the skipped job's dynamic name does
not repair that transition. Metadata must retain native protected readiness
without rebuilding source or accepting incomplete base-retarget proof.

## What Changes

- Keep issue validation in PR-state. Let the existing required `verify` job
  revalidate a real successful source run for metadata events, without planning,
  building, testing, or rerunning the source aggregate.
- Capture the source event's PR number and base in its native run name; combine
  that immutable binding with the native run's head, workflow, and conclusion.
  Never use the mutable PR association on an old workflow run as historical proof.
- Reuse existing source proof for a base retarget, binding its result to the new
  base and unchanged head before protected merge readiness can succeed.
- Preserve required `verify` and `pr-state` checks, their GitHub Actions identity,
  native conversation resolution, selected proof, and source cancellation rules.
- Add causal routing checks and hosted protected-PR validation.

## Capabilities

### New Capabilities

- `pr-source-event-routing`: Distinguish source, metadata, and base-retarget events
  at the existing CI and PR-state owners without generating metadata source checks.

### Modified Capabilities

None. The existing affected-proof requirements remain authoritative; this change
repairs their event delivery and protected-check integration.

## Impact

Issue #560 owns the CI and PR-state workflows, their existing delivery contract
tests, workflow guidance, and its OpenSpec/release mapping under #492. No product
API, database, dependency, planner, or selected test inventory changes are needed.

## Non-Goals

No new workflow, artifact ledger, planner, fabricated statuses, skipped-as-success
source proof, branch-protection relaxation, repeat source proof for metadata
edits, stable release promotion, or revival of the declined CI proposal.

## Readiness

Ready for implementation after executable IssueOps confirms the specified routing,
task mapping, and live release relationships. Hosted protection remains required
acceptance proof.
