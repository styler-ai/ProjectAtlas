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
IssueOps SHALL enforce acceptance as a final ordered state transition. No acceptance task may be checked while any implementation task is unchecked; acceptance checks SHALL form a prefix in canonical order; and closed mapped issues plus release completion SHALL require every implementation and acceptance task checked.

#### Scenario: Implementation remains incomplete
- **WHEN** any implementation task is unchecked
- **THEN** every acceptance and review task must remain unchecked
- **AND** a checked acceptance task fails validation.

#### Scenario: Acceptance review is in progress
- **WHEN** all implementation tasks are checked and acceptance review has begun
- **THEN** IssueOps accepts only a checked prefix of the five acceptance tasks
- **AND** it rejects any checked task after an unchecked acceptance task.

#### Scenario: Issue closes or release completes
- **WHEN** a mapped issue is closed or its milestone is evaluated for release completion
- **THEN** IssueOps requires both authoritative task lists to be fully checked
- **AND** no unresolved implementation or acceptance state can be hidden in another section.

#### Scenario: Incremental pull request remains in progress
- **WHEN** an ordinary pull request contributes part of an issue whose implementation and acceptance tasks remain unchecked
- **THEN** the repository checker may validate the truthful synchronized incomplete state
- **AND** it does not treat pull-request existence as issue closure or final acceptance.

### Requirement: Issue complexity is explicit and singular
IssueOps SHALL require every open mapped issue to carry exactly one label from `complexity:low`, `complexity:medium`, `complexity:high`, or `complexity:very-high`. It SHALL NOT infer complexity or encode model-routing behavior.

#### Scenario: Valid complexity label
- **WHEN** an open mapped issue has exactly one accepted complexity label
- **THEN** IssueOps accepts the label contract independently of priority, milestone, or task state.

#### Scenario: Missing duplicate or unknown complexity
- **WHEN** no accepted complexity label exists, more than one exists, or a `complexity:` label uses an unknown value
- **THEN** IssueOps fails with the observed label state
- **AND** it does not choose, invoke, or validate any agent model.

### Requirement: Existing issue strength and historical truth are preserved
The migration SHALL preserve every existing open issue section, substantive byte, link, task, checked state, mitigation owner ID, milestone fact, and native relationship except the specified task-heading, mitigation-terminology, acceptance-checklist, and complexity-label additions. Closed mapped issues SHALL retain historical bodies and remain readable through their legacy OpenSpec task headings.

#### Scenario: Open issue migration
- **WHEN** an open mapped issue is migrated
- **THEN** its `OpenSpec Tasks` heading becomes `Implementation Tasks`, its mitigation references become `Implementation tasks`, the canonical acceptance checklist is added, and one specification-owned complexity label is applied
- **AND** every other issue fact remains unchanged.

#### Scenario: Closed historical issue
- **WHEN** IssueOps validates a closed mapped issue created under the prior contract
- **THEN** it accepts exactly one legacy `OpenSpec Tasks` or `OpenSpec Task Checklist` section for implementation history
- **AND** it does not require new acceptance fields or complexity labels retroactively
- **AND** it still rejects unchecked historical implementation tasks.

### Requirement: Activation fails closed across live state
The new contract SHALL become authoritative only after the accepted checker and every open mapped live issue converge. The migration SHALL validate exact live readback and provide restoration of prior bodies, labels, milestone/status, and native relationships before merge.

#### Scenario: Complete activation
- **WHEN** the accepted implementation is ready to publish
- **THEN** every open mapped issue is migrated and read back against the new checker
- **AND** the accepted checker head is merged only when the complete live set passes
- **AND** IssueOps is rerun from accepted current `main` after merge.

#### Scenario: Partial or raced migration
- **WHEN** an issue edit, label update, readback, branch validation, or hosted check is partial, stale, or mismatched
- **THEN** no issue, merge, release, or acceptance transition is authorized
- **AND** the prior live bodies, labels, milestone/status, and native relationships are restored when the checker has not merged.

### Requirement: Repository guidance and tests share the contract
The IssueOps self-test, behavior-focused repository E2E, issue templates, pull-request template, repository workflow guidance, repository agent guidance, and applicable version-matched ProjectAtlas plugin guidance SHALL describe and verify the same two-list, completion, complexity, and historical-compatibility rules without adding a dependency or model policy to IssueOps.

#### Scenario: Contract implementation changes
- **WHEN** the checker or issue contract changes
- **THEN** focused positive and negative tests cover heading isolation, exact task mirroring, canonical acceptance text, state ordering, complexity cardinality, migration compatibility, closed history, and closure/release completion
- **AND** guidance and templates use the same exact field names and lifecycle.
