## Context

The release path already verifies Rust tests, platform E2E, parser construction, packaging, installers, checksums, receipts, and artifact digests. Its avoidable cost comes from treating commit identity as proof identity after checklist-only or other behavior-neutral changes.

## Goals / Non-Goals

**Goals:**

- Reuse passed proof when the inputs that can affect that proof are unchanged.
- Keep all existing test suites and fail closed on unknown or relevant changes.
- Use the existing workflows, Git path comparison, and artifact digests.

**Non-Goals:**

- Reusing failed, partial, expired, unverifiable, cross-platform, or cross-toolchain proof.
- Removing provenance SHAs, content checksums, signatures, branch protection, or review gates.
- Adding another build, release, or evidence framework.

## Decisions

### Separate provenance from invalidation

Commit SHAs remain recorded as provenance. Test and artifact reuse is decided by the relevant input paths and the immutable artifact, platform, toolchain, workflow, and contract digests already checked by the owning gate.

Commit equality was rejected because metadata-only commits change provenance without changing executable behavior.

### Keep classification local to the owning proof

The optional-parser handoff compares only its source, dependency, toolchain, packaging, workflow, and construction-script inputs. Benchmark preregistration validates the runtime, MCP inventory, and packaged-skill identities it actually measures without requiring the current Git commit. Unknown changed inputs invalidate proof.

A generalized dependency graph was rejected because the existing proof boundaries are few and already have explicit owners.

### Keep cheap current-state gates current

OpenSpec, IssueOps, live review threads, branch topology, version policy, and release eligibility rerun after metadata-only changes. Expensive construction and behavior proof is reused only after those cheap gates show that no owning input changed.

## Risks / Trade-offs

- [A relevant path is omitted] → Use conservative owned path sets and treat unknown changes as invalidating.
- [An artifact is substituted] → Keep version, digest, receipt, archive-size, platform, toolchain, and workflow validation.
- [Issue and workflow rules drift] → Cover both safe reuse and required invalidation in the existing workflow-policy and IssueOps tests.
- [Historical provenance becomes unclear] → Record source SHAs as descriptive provenance without using equality as the reuse decision.

## Migration Plan

Land the classifier and policy tests, update #311 and #366 in the same IssueOps-synchronized change, then reuse only proof that passes the new input/artifact checks. Roll back by restoring exact-commit matching; no product data or schema migration is involved.

## Open Questions

None.
