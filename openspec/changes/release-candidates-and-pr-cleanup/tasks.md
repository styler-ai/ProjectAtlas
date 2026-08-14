## 1. Contract

- [ ] 1.1 Map `release-candidates-and-pr-cleanup` to GitHub issue #448 in `openspec/issue-map.json` and keep the issue checklist synchronized with this file.
- [x] 1.2 Specify version-agnostic stable/RC syntax, stable milestone derivation, prerelease/Latest behavior, recovery refusal, cumulative release-note history, parser asset identity, installer pin compatibility, automatic dispatch, dependency cleanup, and IssueOps architecture/readiness boundaries.

## 2. Implementation

- [x] 2.1 Add the smallest shared standard-library, version-agnostic stable/RC classifier used by workflows and Python release tooling without a new dependency or generic versioning framework.
- [x] 2.2 Publish and repair GitHub releases with exact stable/prerelease classification, exact-head validation, and post-publication Latest verification.
- [x] 2.3 Preserve automatic stable and RC dispatch, development-version exclusion, existing-tag exclusion, exact optional-parser handoff, and cumulative notes from the preceding stable tag through every RC and the final release.
- [x] 2.4 Preserve complete RC identity in optional-parser assets and POSIX/PowerShell installer downstream-pin checks.
- [x] 2.5 Retarget Dependabot to `main` and close obsolete or superseded dependency PRs against `dev` with explicit dispositions and no dependency-version changes.
- [x] 2.6 Hard-gate IssueOps architecture evidence to a real Mermaid diagram in every linked section or a reasoned `N/A`, and require the standard final architecture-reconciliation task.
- [x] 2.7 Add an issue-event IssueOps gate that admits an open issue to a release milestone only after its mapped OpenSpec, contract tasks, dependencies, open questions, issue contract, and `status:ready` state are implementation-ready.

## 3. Verification and Documentation

- [x] 3.1 Add and run focused positive, negative, failure, and stable-compatibility checks for generic stable, RC, malformed, and development versions, milestone derivation/readiness, cumulative release-note history, and workflow routing.
- [ ] 3.2 Strengthen and run the existing installer E2E coverage for exact stable and RC downstream pins, stale pins, and unrelated repositories.
- [ ] 3.3 Update release guidance and the stable-versus-RC architecture diagram, render and visually inspect the Mermaid output, and synchronize issue #448.
- [x] 3.4 Verify the generic post-publication contract covers exact tag head, stable/prerelease metadata, new-release and repair Latest behavior, and immutable optional-asset repair without making any gated implementation issue depend on the release it blocks.
- [ ] 3.5 Run OpenSpec, IssueOps, release, Rust, and repository gates and reconcile every live review thread before closure.
- [ ] 3.6 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
