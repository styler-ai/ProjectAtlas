## Why

An ordinary pull-request title or body edit can leave an unchanged verified head
blocked by required-check association after source CI emits a skipped aggregate.
Release work needs metadata validation without disturbing source proof, while a
base retarget must still verify the new exact comparison.

## What Changes

- Keep metadata validation in PR-state and schedule the existing CI result job
  so GitHub resolves its metadata-only name without executing any source step.
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

No new proof ledger or planner, fabricated statuses, skipped-as-success source
proof, branch-protection relaxation, repeat source proof for metadata edits,
stable release promotion, or revival of the declined CI proposal.

## Readiness

Ready for implementation after executable IssueOps confirms the specified routing,
task mapping, and live release relationships. Hosted protection remains required
acceptance proof.
