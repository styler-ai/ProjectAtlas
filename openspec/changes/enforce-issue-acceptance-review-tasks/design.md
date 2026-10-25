## Context

The current checker extracts one exact `Implementation Tasks` section for each open mapped issue and compares it with the mapped local `tasks.md`. It also validates the full #305 issue shape, pre-mortem mitigation ownership/state, architecture links, native closed state, and release-milestone completion. That makes implementation bookkeeping reliable, but it does not expose an independent issue-acceptance decision.

The abandoned #500/#501 lane proposed ordinary non-checkbox acceptance bullets, Sol-owned semantic review, body manifests, and a staged future release graph. This change does not revive that design. The user has selected two visible task fields, model-blind structural enforcement, one required complexity label, Terra code/intent review for every issue, and an additional fresh Sol holistic acceptance only for externally routed `complexity:very-high` work.

Hosted validation of two independent v0.5 branches exposed one concurrency defect in the existing global mirror: checking completed implementation tasks on issue A immediately made issue B's older branch fail because CI compared every candidate-local task slice with mutable live issue state. Immediate truthful task progress and independent delivery lanes therefore require pull-request validation to distinguish the owning issue from unrelated task authority.

## Goals / Non-Goals

**Goals:**

- Preserve the complete existing issue packet and every current IssueOps gate.
- Rename the exact OpenSpec mirror to `Implementation Tasks` without changing its local `tasks.md` authority.
- Add one exact `Acceptance and Review Tasks` checklist whose five tasks cover issue intent/outcome, implementation/source, specification/architecture, test/proof, and final readiness.
- Make acceptance state impossible to advance while implementation is incomplete, and require both lists before closure/release.
- Require exactly one valid complexity label on every open issue without encoding model routing in IssueOps or inventing implementation tasks for unmapped backlog issues.
- Make the current two-list contract authoritative for every open mapped issue; already closed mapped issues remain inert historical state.
- Preserve immediate task progress and parallel pull requests without allowing a candidate branch to alter another issue's task slice.

**Non-Goals:**

- Semantic scoring, reviewer identity, model selection, agent receipts, commit/SHA evidence, or an LLM CI gate.
- Removing, shortening, or replacing existing issue intent, capability, architecture, non-goal, pre-mortem, OpenSpec, test, or release obligations.
- Rewriting closed issue history or changing Rust product/runtime and SQLite behavior.
- Adding a second local acceptance artifact, generated evidence ledger, or per-issue schema framework.

## Decisions

### 1. Keep local OpenSpec tasks authoritative and rename only their issue field

`openspec/changes/<change>/tasks.md` remains the implementation authority. For an open mapped issue, the checker extracts exactly one visible `Implementation Tasks` section and compares text, order, owner slice, and checked state exactly as it does today. An already closed mapped issue is inert: global and release state gates use its native CLOSED state without parsing or migrating its body. If it is reopened, its current body must satisfy the open two-list contract before work proceeds.

This avoids renaming every local OpenSpec file or introducing another task store. Treating the GitHub issue as a second free-form implementation plan was rejected because it would lose exact synchronization.

Contract selection follows native issue state. Every open mapped issue uses the current two-list contract; no registry, cutoff, timestamp, issue-number heuristic, manifest, or mutable heading can grant a legacy exception. Closed history is not part of task/body validation. Pull-request validation obtains the bounded open issue set to exclude closed unrelated slices, then checks the one open owner against live state and all unrelated open slices against the accepted base.

The migration preserves any existing review-oriented OpenSpec row, including the historical final architecture-reconciliation task, because deleting it would weaken an accepted issue packet. The new checker no longer requires every future implementation list to end with that review-only row: the canonical specification/architecture acceptance task now owns the holistic reconciliation gate.

### 2. Use one fixed, model-blind acceptance checklist

Every open mapped issue contains exactly one `Acceptance and Review Tasks` section with these ordered tasks:

1. `Intent and outcome review:` the delivered result fulfills the complete issue intent and respects non-goals at the real boundary.
2. `Implementation review:` the complete source and implementation artifacts satisfy correctness, ownership, applicable Rust/database discipline, security, resource, compatibility, and simplicity requirements.
3. `Specification and architecture review:` issue, OpenSpec, source, documentation, and necessary architecture views agree, including an explicit reasoned N/A where no new view is required.
4. `Test and proof review:` appropriate unit, integration, E2E, fault, concurrency, performance, and platform proof is sound, causal, and covers positive, negative, failure, and compatibility behavior.
5. `Final readiness review:` implementation tasks, review feedback, and required local/hosted gates are complete with no partial behavior or proof.

The exact task wording is a repository constant and does not name a reviewer model. The complete issue and OpenSpec packet supply issue-specific intent; the checklist prevents those details from being replaced by generic completion claims. Additional checkboxes inside the acceptance section are rejected so it cannot become a second implementation plan or evidence ledger.

### 3. Enforce acceptance as a final state transition

Acceptance tasks must be unchecked while any implementation task is unchecked. Once implementation is complete, acceptance tasks may progress only as a checked prefix in their fixed order. A later implementation reopening therefore requires all acceptance tasks to return to unchecked. The active issue path requires both task sets before closure or release completion. Already closed mapped issues are checked only for native repository state by global and release gates; reopening them returns them to the active contract.

Ordinary in-progress pull requests remain valid with unchecked implementation and acceptance tasks. The checker enforces truthful task state, not a rule that every incremental PR closes its issue.

### 4. Require one explicit complexity label

Every open issue has exactly one of `complexity:low`, `complexity:medium`, `complexity:high`, or `complexity:very-high`. IssueOps checks cardinality and vocabulary only. It does not infer complexity, select models, or require an unmapped backlog issue to fabricate OpenSpec-backed implementation tasks.

Sol assigns the label during specification using delivery-boundary complexity rather than priority, duration, checklist length, or changed-line count. Routing policy lives in the external model-router: low/medium normally select Luna High, high/very-high Luna XHigh, Terra High always reviews, and very-high receives an additional fresh Sol XHigh holistic acceptance after Terra.

### 5. Activate through a bounded live migration

Before the new checker becomes authoritative, prepare exact body edits for every open mapped issue and validate the complete live complexity-label inventory for every open issue. The body migration SHALL:

- preserve all existing prose, links, tasks, checked state, relationships, and milestone facts;
- rename only the authoritative task heading;
- add the fixed acceptance checklist unchecked unless the issue already has complete independently accepted proof that is read back live;
- rename pre-mortem ownership text from `(OpenSpec tasks: ...)` to `(Implementation tasks: ...)` without changing IDs or state;

The user-authorized complexity classification is already live and independently useful to the external router. Unmapped backlog issues receive only that label until they gain a real OpenSpec mapping and implementation task authority. Checker activation treats the labels as validated input; a body/checker rollback does not remove them.

Publish the issue edits only after the implementation has passed local independent review. The old main checker may fail closed during the short heading-transition window; no issue or release transition is authorized from that state. Push the accepted checker immediately, validate all live bodies with the new branch checker, merge only on complete readback, and rerun IssueOps on current `main`. If complete convergence cannot be achieved, restore the preserved prior bodies/labels and do not merge the checker.

A compatibility mode that accepts both task headings indefinitely was rejected because it would not enforce the rename for open work. Rewriting closed issue bodies was rejected because closed history is inert and body mutation adds no safety.

### 6. Compare pull requests at the correct authority boundaries

Pull-request CI resolves exactly one owning issue from the repository's existing pull-request issue relationship. The checker compares that issue's candidate-local implementation task slice with mutable live issue state, because those checkboxes are the owning lane's immediate progress. Every other mapped issue is compared with the pull-request base instead of live state. A candidate that changes an unrelated task slice, has missing or ambiguous ownership, or cannot read the accepted base fails closed.

Pushes to `main`, milestone completion, and release validation retain the native closed-state gate and the complete active open-issue contract check. Ordinary issue events retain their existing affected-issue `--planned-issue` scope. Closed mapped history is skipped after state read, while a reopened issue is validated as open. This is not eventual consistency and does not permit stale owner progress: it separates candidate ownership from unrelated open mutable state while preserving the existing global convergence boundary after merge.

Selecting only one live issue without checking unrelated candidate task files was rejected because it would let a pull request silently modify another issue's local authority. Serializing task updates until every concurrent branch rebases was rejected because it contradicts immediate task progress and independent delivery.

### 7. Extend existing tests and guidance without new machinery

The standard-library self-test owns parser/state-transition coverage: a closed mapped issue with a legacy heading is skipped, reopening that same body is rejected until migrated, planned checks remain issue-scoped, and release checks use native state without reading closed bodies. Existing Rust workflow-policy E2E assertions execute that self-test against the repository fixture while also confirming that CI, release, issue templates, PR guidance, repository guidance, and the checker carry the same contract, including removal of the legacy mandatory-final-review-row rule for new implementation lists. No dependency, service, schema, generated manifest, or new workflow is needed. Parsing remains linear in bounded issue-body size; state-only closed checks avoid historical body I/O, while each active issue body is fetched once per validation boundary.

## Risks / Trade-offs

- **Migration temporarily makes old-main IssueOps reject renamed live bodies.** → Publish only after local acceptance, keep the window bounded, block transitions, merge only after branch-checker readback, and restore prior bodies on failure.
- **Generic review boxes become empty ceremony.** → Use exact outcome-oriented wording, require sequential state, retain the complete issue/OpenSpec detail, and keep semantic reviewer judgment outside CI.
- **The new section weakens or replaces issue-specific content.** → Preserve every existing section and task byte except the mechanical heading/mitigation terminology changes; fail migration readback on any other drift.
- **Complexity labels become disguised priority or model logic leaks into IssueOps.** → Validate only one vocabulary value; document semantic classification and model routing separately.
- **A reopened historical issue bypasses the current contract.** → Skip bodies only while native state is CLOSED; once reopened, require the current two-list contract and exact active task mirror.
- **A checked implementation task later reopens after acceptance.** → Fail closed unless the entire acceptance list is reset.
- **Immediate progress on one issue makes an unrelated pull request stale, or a pull request edits another issue's tasks.** → Validate the owner against live state, compare unrelated slices with the accepted base, reject missing or ambiguous ownership, and keep global checks on `main` and release.
- **Checklist growth becomes a receipt ledger.** → Fix the acceptance section to five tasks and reject additional acceptance checkboxes.

## Migration Plan

1. Create and map one sanitized v0.5 IssueOps owner with `complexity:high`, and declare its queued direct-child/blocker relationship to #492. Keep the live relationship, milestone, and readiness activation for the bounded publication step so accepted `main` and its existing release graph remain coherent during implementation.
2. Implement and locally validate the checker, branch-aware pull-request workflow call, templates, guidance, and contract tests without changing live issue bodies.
3. Obtain Terra High acceptance of the immutable implementation head; this issue does not require the additional Sol gate unless its final specification is reclassified `complexity:very-high`.
4. Prepare, diff, and validate every open mapped issue body, read back every open issue complexity label, and prepare the queued #517/#492 relationship activation against the accepted checker while preserving the mutable activation state for rollback.
5. Publish the live body/relationship migration, validate the exact body/label/relationship state, push the Terra-accepted implementation head, run hosted checks, and reach exact merge-ready convergence only when the new checker sees the complete live set.
6. Synchronize both task lists at that accepted boundary, merge, rerun IssueOps from accepted `main`, verify this issue and #492 relationships/tasks, and then allow ordinary release work to continue.

Rollback before merge restores the prior live issue bodies, #517 milestone/status, and native relationships and abandons the unmerged checker head. The independently authorized complexity labels remain classified unless the user separately changes that policy. After merge, any defect is fixed forward under the same owner; closed historical bodies remain untouched throughout.

## Dependencies / Cross-Issue Impact

- #517 has no product-code prerequisite and can be implemented on the accepted release baseline while #464 completes its independent product lane.
- The live #517 milestone, readiness state, direct-child relationship to #492, and #492 blocker edge activate only with the accepted body/label migration; before that bounded window, the current release graph remains authoritative.
- The accepted #517 contract becomes a release-wide workflow gate for every remaining open mapped issue, while preserving each issue's existing product scope and real dependency edges.

## Open Questions

None.
