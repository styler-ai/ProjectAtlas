## 1. Input Classification

- [x] 1.1 Define the minimal behavior-relevant release-input contract and the metadata-only paths that cannot affect built or packaged behavior.
- [x] 1.2 Implement one fail-closed change classifier with existing Git/path tooling; unknown inputs invalidate affected proof.

## 2. Proof Reuse

- [x] 2.1 Bind reusable immutable artifacts to their relevant input content, digest, platform, toolchain, and workflow contract.
- [x] 2.2 Reuse valid passed artifacts and test proof across commit-only or metadata-only changes, while rerunning cheap OpenSpec, IssueOps, review, topology, and policy gates.
- [x] 2.3 Invalidate and rerun every affected test or build gate when source, dependency, toolchain, workflow, packaging, configuration, or parser-pack inputs change.

## 3. Process Contract

- [x] 3.1 Retain commit SHAs for provenance and supply-chain controls, but remove exact commit equality as a general test/build reuse requirement.
- [x] 3.2 Adapt IssueOps and release guidance to validate the classification, recognize ordinary negative-modal prohibitions, paginate reusable handoff discovery, and prevent SHA receipts or per-task proof ceremony.

## 4. Verification

- [x] 4.1 Add focused positive, negative, mixed-clause, negative-modal, pagination, unknown-input, cross-platform/toolchain, and artifact-tampering tests for classification and reuse.
- [ ] 4.2 Add hosted end-to-end proof that a checklist-only promotion reuses valid expensive artifacts and a behavior-relevant change invalidates them.
- [ ] 4.3 Update the release architecture view and runbook, then verify the simpler path on a non-production release candidate.
