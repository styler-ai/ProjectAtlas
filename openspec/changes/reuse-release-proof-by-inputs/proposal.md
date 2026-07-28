## Why

ProjectAtlas currently reruns expensive release proof when only the commit SHA or checklist metadata changes, even though the tested runtime and behavior-relevant inputs are unchanged. This delays v0.4.0 without increasing confidence.

## What Changes

- Keep every existing unit, integration, E2E, platform, security, installer, packaging, and release test.
- Reuse passed proof when its behavior-relevant inputs, platform, toolchain, workflow contract, and immutable artifact digests are unchanged.
- Treat source, lockfile, toolchain, workflow, packaging, configuration, and parser-pack input changes as invalidating; unknown changes fail closed.
- Search every page of successful reusable-artifact runs before declaring that no matching proof exists.
- Rerun cheap OpenSpec, IssueOps, review-thread, topology, and release-policy checks after metadata-only changes.
- Let IssueOps distinguish ordinary negative-modal prohibitions from affirmative exact-head requirements.
- Retain commit SHAs as provenance without using commit equality as a test or artifact-reuse requirement.

## Capabilities

### New Capabilities

- `release-proof-reuse`: Define safe input-based reuse and invalidation for existing release proof.

### Modified Capabilities

None.

## Impact

- Existing CI, optional-parser, release, benchmark, IssueOps, and readiness contracts.
- No product runtime, crate, dependency, schema, migration, CLI/MCP contract, or test suite is added or removed.

## Non-Goals

- Weakening test coverage, branch protection, review gates, checksums, signatures, SHA-pinned Actions, or provenance.
- Adding a generalized build system, evidence ledger, or per-task receipt scheme.
- Reusing proof when any relevant input or artifact identity is unknown or changed.

This change is ready for implementation in v0.4.0.
