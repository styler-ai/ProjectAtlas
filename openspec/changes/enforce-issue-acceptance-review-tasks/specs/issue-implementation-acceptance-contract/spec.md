## ADDED Requirements

### Requirement: Open issues separate implementation from acceptance
IssueOps SHALL require every open mapped GitHub issue to contain exactly one visible `Implementation Tasks` section and exactly one visible `Acceptance and Review Tasks` section after the complete existing issue packet. The implementation section SHALL exactly mirror the mapped local OpenSpec task owner slice in text, order, and checked state.

#### Scenario: Complete open issue packet
- **WHEN** IssueOps validates an open issue mapped by `openspec/issue-map.json`
- **THEN** it requires every existing substantive issue section in canonical order followed by `Implementation Tasks` and `Acceptance and Review Tasks`
- **AND** it compares only the implementation section with the local `tasks.md` owner slice
- **AND** acceptance tasks cannot satisfy, pollute, or replace the implementation mirror.

#### Scenario: Missing duplicate hidden or legacy task field
- **WHEN** an open mapped issue omits either new task field, duplicates a field, hides it in a comment or fence, or retains only `OpenSpec Tasks` or `OpenSpec Task Checklist`
- **THEN** IssueOps fails with a bounded structural diagnostic
- **AND** it does not infer task state from unrelated checkboxes or comments.

#### Scenario: Architecture reconciliation moves to acceptance
- **WHEN** a new implementation task list does not end with the historical architecture-review task
- **THEN** IssueOps does not reject the implementation mirror for that reason
- **AND** the canonical specification and architecture acceptance task remains mandatory
- **AND** migration preserves any existing architecture-review implementation row unchanged.

### Requirement: Acceptance and review tasks are strong and fixed
The `Acceptance and Review Tasks` section SHALL contain exactly five ordered outcome-oriented checkboxes covering intent/outcome, implementation/source, specification/architecture, test/proof, and final readiness. Their canonical text SHALL require holistic review of the complete issue and applicable behavior boundary without naming an agent model.

#### Scenario: Canonical acceptance checklist
- **WHEN** an open mapped issue contains the five canonical acceptance tasks once and in order
- **THEN** IssueOps accepts their structure
- **AND** the tasks require complete issue intent, source and ownership quality, specification and diagram truth, sound layered tests, and final review/gate readiness.

#### Scenario: Weakened or expanded acceptance checklist
- **WHEN** a canonical task is missing, renamed, reordered, duplicated, or replaced, or the section contains an additional checkbox
- **THEN** IssueOps rejects the issue contract
- **AND** the issue cannot substitute implementation steps, evidence receipts, or generic completion claims for the canonical review gates.

### Requirement: Acceptance follows complete implementation
IssueOps SHALL enforce acceptance as a final ordered state transition for active mapped issues. No acceptance task may be checked while any implementation task is unchecked; acceptance checks SHALL form a prefix in canonical order; and an active issue plus release completion SHALL require every implementation and acceptance task checked. A mapped issue already in native CLOSED state is inert for body/task validation; closure and release still require the repository-native closed state.

#### Scenario: Implementation remains incomplete
- **WHEN** any implementation task is unchecked
- **THEN** every acceptance and review task must remain unchecked
- **AND** a checked acceptance task fails validation.

#### Scenario: Acceptance review is in progress
- **WHEN** all implementation tasks are checked and acceptance review has begun
- **THEN** IssueOps accepts only a checked prefix of the five acceptance tasks
- **AND** it rejects any checked task after an unchecked acceptance task.

#### Scenario: Issue closes or release completes
- **WHEN** a release milestone is evaluated, or an active mapped issue is being prepared for closure
- **THEN** IssueOps requires each active issue's authoritative task lists to be fully checked and each mapped issue to be natively CLOSED for release completion
- **AND** an already CLOSED mapped issue is accepted as inert history without revalidating its body.

#### Scenario: Incremental pull request remains in progress
- **WHEN** an ordinary pull request contributes part of an issue whose implementation and acceptance tasks remain unchecked
- **THEN** the repository checker may validate the truthful synchronized incomplete state
- **AND** it does not treat pull-request existence as issue closure or final acceptance.

### Requirement: Issue complexity is explicit and singular
IssueOps SHALL require every open issue to carry exactly one label from `complexity:low`, `complexity:medium`, `complexity:high`, or `complexity:very-high`. It SHALL NOT infer complexity, encode model-routing behavior, or require unmapped backlog issues to fabricate OpenSpec-backed task fields.

#### Scenario: Valid complexity label
- **WHEN** an open issue has exactly one accepted complexity label
- **THEN** IssueOps accepts the label contract independently of priority, milestone, or task state.

#### Scenario: Missing duplicate or unknown complexity
- **WHEN** no accepted complexity label exists, more than one exists, or a `complexity:` label uses an unknown value
- **THEN** IssueOps fails with the observed label state
- **AND** it does not choose, invoke, or validate any agent model.

#### Scenario: Unmapped backlog issue
- **WHEN** an open issue has no OpenSpec mapping and is not implementation-ready
- **THEN** IssueOps still requires exactly one valid complexity label
- **AND** it does not require `Implementation Tasks` or `Acceptance and Review Tasks` until a real mapping supplies task authority.

### Requirement: Existing issue strength and closed history are preserved
The migration SHALL preserve every existing open mapped issue section, substantive byte, link, task, checked state, mitigation owner ID, milestone fact, and native relationship except the specified task-heading, mitigation-terminology, acceptance-checklist, and complexity-label additions. Already closed mapped issues remain untouched and inert; their native repository state is authoritative and their historical body is not repeatedly parsed or migrated.

#### Scenario: Open issue migration
- **WHEN** an open mapped issue is migrated
- **THEN** its `OpenSpec Tasks` heading becomes `Implementation Tasks`, its mitigation references become `Implementation tasks`, the canonical acceptance checklist is added, and one specification-owned complexity label is applied
- **AND** every other issue fact remains unchanged.

#### Scenario: Closed mapped issue is inert
- **WHEN** IssueOps encounters a mapped issue whose authenticated native state is CLOSED
- **THEN** global and release checks use that state for closure/release gating without parsing, migrating, or validating its body or historical task heading
- **AND** malformed or unchecked historical text does not re-enter active task validation.

#### Scenario: Reopened historical issue uses the current contract
- **WHEN** an issue with a prior `OpenSpec Tasks` or `OpenSpec Task Checklist` body is reopened
- **THEN** IssueOps treats it as an open mapped issue and rejects the legacy heading until it is migrated to exactly one current `Implementation Tasks` and `Acceptance and Review Tasks` section
- **AND** after migration it must mirror its local owner slice and follow the active acceptance state rules.

### Requirement: Activation fails closed across live state
The current contract SHALL become authoritative for open mapped issues only after the accepted checker, every open mapped live issue body, and every open issue complexity label converge. Already closed mapped issues are not part of that body migration. The migration SHALL validate exact live readback and provide restoration of prior bodies, #517 milestone/status, and native relationships before merge. The independently authorized complexity classification SHALL remain intact across checker/body rollback.

#### Scenario: Complete activation
- **WHEN** the accepted implementation is ready to publish
- **THEN** every open mapped issue body and every open issue complexity label is migrated and read back against the new checker
- **AND** the accepted checker head is merged only when the complete live set passes
- **AND** IssueOps is rerun from accepted current `main` after merge.

#### Scenario: Partial or raced migration
- **WHEN** an issue edit, label update, readback, branch validation, or hosted check is partial, stale, or mismatched
- **THEN** no issue, merge, release, or acceptance transition is authorized
- **AND** the prior live bodies, #517 milestone/status, and native relationships are restored when the checker has not merged
- **AND** the independently authorized complexity labels remain intact.

### Requirement: Pull-request validation isolates mutable issue progress
Pull-request IssueOps SHALL validate the open owning issue's candidate-local implementation task slice against live issue state while requiring every unrelated open mapped issue task slice to remain identical to the accepted pull-request base. Closed unrelated history SHALL not participate. Missing or ambiguous owning-issue identity, unreadable base authority, or an unrelated task-slice edit SHALL fail closed. Pushes to `main`, milestone completion, and release validation SHALL retain complete active live-state validation plus native closed-state gating, while ordinary issue events SHALL retain their existing affected-issue `--planned-issue` scope.

#### Scenario: Unrelated issue advances while a pull request is open
- **WHEN** issue A's implementation tasks advance immediately in live GitHub state while an independent pull request for issue B retains issue A's accepted base task slice
- **THEN** pull-request validation compares issue B's candidate slice with issue B's live state
- **AND** it compares issue A's candidate slice with the pull-request base rather than mutable live state
- **AND** issue A's truthful progress does not fail issue B's unchanged branch.

#### Scenario: Pull request changes an unrelated task slice
- **WHEN** a pull request for issue B changes issue A's local implementation tasks relative to the accepted base
- **THEN** IssueOps rejects the candidate even if issue A's live state happens to match
- **AND** no unrelated local task authority can ride through the owner-only live comparison.

#### Scenario: Ownership or base authority is unavailable
- **WHEN** pull-request CI cannot resolve exactly one owning issue or cannot read the accepted base task authority
- **THEN** IssueOps fails with a bounded ownership or base diagnostic
- **AND** it does not fall back to accepting an owner-only or candidate-only comparison.

#### Scenario: Accepted branch reaches a global boundary
- **WHEN** IssueOps runs for a push to `main`, milestone completion, or release validation
- **THEN** it validates every open mapped local task slice against complete live issue state and requires every mapped issue to be natively CLOSED for milestone/release completion
- **AND** already closed bodies and unrelated closed history remain outside task validation while branch isolation cannot hide repository-wide drift at the convergence boundary.

#### Scenario: An issue event targets one planned issue
- **WHEN** IssueOps runs for an ordinary issue event
- **THEN** it retains the existing `--planned-issue` validation scope for that affected issue
- **AND** it does not broaden an issue edit into an unrelated repository-wide task comparison.

### Requirement: Repository guidance and tests share the contract
The IssueOps self-test, behavior-focused repository E2E, issue templates, pull-request template, repository workflow guidance, repository agent guidance, and applicable version-matched ProjectAtlas plugin guidance SHALL describe and verify the same two-list, completion, complexity, and historical-compatibility rules without adding a dependency or model policy to IssueOps.

#### Scenario: Contract implementation changes
- **WHEN** the checker or issue contract changes
- **THEN** focused positive and negative tests cover heading isolation, exact task mirroring, canonical acceptance text, state ordering, complexity cardinality, pull-request ownership and base isolation, concurrent progress, migration compatibility, closed history, and closure/release completion
- **AND** guidance and templates use the same exact field names and lifecycle.
