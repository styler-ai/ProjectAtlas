## Why

The release workflow requires every milestone issue to be closed before publication, while the declared release owner must publish and verify the candidate before it can close. With all accepted children closed, publication still fails on the open owner. Separate publication readiness from milestone closeout so the release can execute its accepted tasks.

## What Changes

- Keep strict all-closed milestone validation for closeout.
- Add a publication milestone check that derives the permitted release owner from the existing issue-map declaration and requires every other member to be closed.
- Validate the declaration and exact milestone membership; retain normal OpenSpec contract validation and exact-main publication checks.
- Permit an already closed owner only during repair of an existing correctly classified release at the exact candidate commit, and retain all-closed behavior for historical milestones without a declared release graph.
- Update the workflow caller and release guidance, with focused positive, negative, malformed-input, and repair regression checks.

## Capabilities

### New Capabilities

- `release-publication-readiness`: Distinguish release-owner publication from all-closed milestone closeout using declared membership.

### Modified Capabilities

None.

## Impact

The IssueOps checker, release workflow invocation, existing issue-map release declaration, and workflow documentation. No runtime, database, package identity, release metadata, dependency, or public product API changes. This is an implementation-ready defect fix once its issue mapping and contract are synchronized.

## Non-Goals

Publishing a release as part of this defect, closing the release owner, authorizing stable promotion, bypassing child acceptance or reviews, changing automatic publication policy, or restoring a general issue mutation framework.
