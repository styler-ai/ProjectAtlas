## 1. Specification and Issue Boundary

- [x] 1.1 Create and map one sanitized v0.5 IssueOps owner, assign its accepted complexity, declare its queued direct #492 child and blocker activation, synchronize the complete issue packet with this OpenSpec task list, and pass strict OpenSpec plus current IssueOps validation.
- [ ] 1.2 Add focused issue-task authority and acceptance-state architecture views to the existing v0.5 release architecture document; render every changed Mermaid block and inspect both visual communication and semantic truth.

## 2. IssueOps Contract

- [x] 2.1 Replace the open mapped-issue `OpenSpec Tasks` authority with exactly one `Implementation Tasks` field that retains exact local `tasks.md` text/order/ownership/state mirroring, isolates unrelated or hidden checkboxes, keeps legacy task-heading support only for explicitly grandfathered mapped CLOSED issues, and retires the mandatory final architecture-review implementation row without deleting any existing task.
- [x] 2.2 Require exactly one canonical five-row `Acceptance and Review Tasks` field, reject weakened/reordered/extra acceptance checkboxes, enforce all-unchecked acceptance until implementation completes and prefix-only review progress afterward, and require both task lists complete before closure or release.
- [x] 2.3 Require exactly one accepted complexity label on every open issue, report missing/duplicate/unknown values, leave unmapped backlog issues free of fabricated task fields, and keep the checker free of model names, model routing, semantic scoring, and inferred complexity.
- [x] 2.4 Extend the standard-library IssueOps self-test for positive, negative, hidden, duplicate, state-transition, complexity, closure/release, migration, concurrent pull-request progress, closed-history compatibility, and immutable per-issue contract-provenance cases without adding a dependency or evidence ledger.
- [x] 2.5 Make pull-request validation branch-aware: validate the owning issue's candidate implementation tasks against live issue state, require every unrelated mapped task slice to remain identical to the pull-request base, freeze the repository-owned per-issue legacy provenance after its initial accepted-base introduction, fail closed when ownership is missing or ambiguous, and retain complete global live validation on `main` and release gates.

## 3. Repository Surfaces and Proof

- [x] 3.1 Update applicable issue templates, the pull-request template, repository workflow and agent guidance, and version-matched ProjectAtlas plugin guidance so they preserve every existing substantive issue field; use the exact two-list, mitigation-terminology, completion, complexity, explicit legacy-provenance, and historical rules; and document branch-aware pull-request validation without weakening global `main` or release checks.
- [x] 3.2 Extend the owning behavior-focused repository E2E assertions and run the IssueOps self-test, Python compilation, focused Rust E2E, formatting/diff checks, strict OpenSpec validation, concurrent-branch regression proof, contract-downgrade and provenance-freeze proof, and complete affected repository gates with explicit timeouts.

## 4. Live Migration and Acceptance

- [ ] 4.1 Prepare and independently diff every open mapped issue body, read back every open issue complexity label, and prepare the queued #517 milestone/status/relationship activation; preserve all prior prose, links, task text/state, milestone facts, and relationships; validate the complete candidate set with the new checker; and retain exact mutable-state rollback inputs without committing a body manifest or receipt ledger.
- [ ] 4.2 Obtain Terra High review of the immutable implementation and migration boundary, return every finding to the implementation owner, and keep all acceptance tasks and publication blocked until Terra accepts the complete issue intent, source, specifications, architecture, and tests.
- [ ] 4.3 Publish the bounded live issue-body/relationship migration, validate the complete body/label/relationship set, push only the Terra-accepted implementation head, pass hosted checks and live IssueOps, reach exact merge-ready convergence, and restore prior bodies/relationships if convergence fails before merge.
- [ ] 4.4 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
