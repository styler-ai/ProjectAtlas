## ADDED Requirements

### Requirement: Planned issues use one acceptance-oriented section order
Every planned ProjectAtlas issue SHALL contain exactly one visible non-empty section in this order: `Why`, `What Changes`, `Capabilities`, `Architecture Diagrams`, `Release Scope`, `Acceptance Criteria`, `Non-Goals`, `Pre-Mortem`, then exactly one `OpenSpec Tasks` or `OpenSpec Task Checklist` section. `Acceptance Criteria` SHALL contain two to five top-level ordinary unordered bullets and SHALL NOT contain checkboxes, OpenSpec task IDs, evidence receipts, or another implementation checklist.

#### Scenario: Complete planned issue
- **WHEN** one planned issue contains the required sections in order, two to five ordinary acceptance bullets, and one exact OpenSpec task mirror
- **THEN** its structural acceptance contract may pass while semantic readiness remains a separate Sol decision

#### Scenario: Acceptance is missing or becomes tasks
- **WHEN** the acceptance section is missing, duplicate, empty, misordered, contains fewer than two or more than five ordinary bullets, or contains a checkbox, OpenSpec task ID, or receipt
- **THEN** IssueOps fails the planned issue without treating its OpenSpec task mirror as acceptance evidence

### Requirement: Acceptance states observable behavior rather than work
Acceptance bullets SHALL express observable truths across the applicable positive, negative or fail-closed, compatibility or unchanged, and legitimate no-change boundaries. The bullets SHALL remain distinct from implementation steps, proof receipts, and task completion state.

#### Scenario: Behavior can close with no product change
- **WHEN** a measurement, reproduction, or warning-classification issue permits retaining current behavior when representative evidence already satisfies the contract
- **THEN** its acceptance criteria include the observable no-change outcome and the proof boundary without inventing implementation work

#### Scenario: Failure behavior is material
- **WHEN** a malformed, stale, wrong-root, unsupported, over-budget, canceled, raced, or otherwise unsafe input is part of the issue boundary
- **THEN** the acceptance criteria state the observable refusal or last-valid-state behavior instead of only the happy path

### Requirement: Planned prose preserves applicable intake context
A planned bug SHALL retain enough sanitized trigger, environment, actual, and expected behavior to reproduce the defect, or SHALL state truthfully that it hardens a missing proof boundary without claiming a reproduced product failure. A planned improvement SHALL identify the affected actor or surface and the before-to-after agent workflow. Inapplicable raw issue-form fields SHALL NOT be required as empty ceremony.

#### Scenario: Reproducible bug
- **WHEN** a bug has a known trigger and environment
- **THEN** `Why` and `What Changes` preserve concise sanitized trigger, actual, expected, and relevant environment facts through planning

#### Scenario: Proof-gap hardening
- **WHEN** an issue such as real host configuration consumption closes a missing verification boundary rather than a reproduced product failure
- **THEN** the body names the unproven boundary and expected proof without fabricating reproduction or failure evidence

#### Scenario: Agent workflow improvement
- **WHEN** an improvement changes how an agent reaches trustworthy source or another ProjectAtlas result
- **THEN** the body names the actor or affected surface and explains the current-to-intended workflow

### Requirement: IssueOps proves objective structure only
IssueOps SHALL check exactly one visible non-empty `Acceptance Criteria` section in the canonical order, two to five top-level ordinary unordered bullets, and absence of checkboxes and OpenSpec task-ID references. It SHALL preserve the current exact task mirror, architecture-link/Mermaid, pre-mortem mitigation, milestone, release-graph, native-relationship, publication, and readiness gates. It SHALL NOT use word counts, keyword or jargon rules, semantic scores, an LLM gate, or an evidence ledger.

#### Scenario: Structurally valid but weak prose
- **WHEN** the section shape is valid but the explanation does not establish actor, consequence, intended behavior, ownership, or release role
- **THEN** IssueOps reports only structural success and Sol keeps readiness or handoff blocked after semantic review

#### Scenario: Existing structural gate fails
- **WHEN** acceptance is structurally valid but the task mirror, diagram, pre-mortem, graph, publication, or readiness evidence fails
- **THEN** the existing owning gate still fails without being weakened by the new section

### Requirement: Sol performs the eight-question semantic audit
Before readiness, implementation handoff, candidate acceptance, and stable acceptance, Sol SHALL reconcile the body, OpenSpec, diagrams, and live release graph by answering the eight design-owned questions for `Why`, `What Changes`, capabilities, release scope, acceptance, non-goals/pre-mortem, diagram meaning, and the exact task mirror. The audit SHALL remain a semantic judgment and SHALL NOT become a score, checklist ledger, generated receipt, or automated model gate.

#### Scenario: Per-issue handoff is coherent
- **WHEN** the eight questions show that one issue's explanation, behavior/capability, acceptance, release role, non-goals/failures, diagram meaning, and tasks agree
- **THEN** Sol may allow the normal published-readiness path to proceed to Luna handoff if every objective gate also passes

#### Scenario: Semantic sources disagree
- **WHEN** any answer reveals missing actor/current state, an overstated capability, an artificial dependency, incomplete acceptance, a misleading diagram, or task/prose drift
- **THEN** Sol keeps the issue unready and corrects the specification packet without asking IssueOps to score prose

### Requirement: The restored contract does not invent product dependencies
#500 SHALL be one direct native child of release owner #492 with `blocked_by: []`, and #492 SHALL be directly blocked by #500. #500 SHALL NOT be added as a native blocker of every product issue. Per-issue implementation handoff SHALL remain blocked by the published-readiness contract until that packet and the shared mechanism it depends on are accepted.

#### Scenario: Independent product lane
- **WHEN** one product issue has no genuine schema, identity, interface, platform, structural, or proof dependency on #500
- **THEN** the release graph contains no #500 blocker edge and the issue remains independently orderable after its own restored packet passes readiness

#### Scenario: CI issue needs the shared mechanism
- **WHEN** #497 is otherwise ready before the #500 structural mechanism and order are accepted
- **THEN** handoff waits through the readiness gate without misrepresenting #500 as a native product dependency

### Requirement: The complete v0.5 set is migrated and reread semantically
Sol SHALL migrate issue #500 plus the twenty-nine accepted/current-candidate v0.5.0 bodies from exact live bodies or the latest candidate drafts, preserving authoritative task text and checked state while applying only required explanatory, acceptance, graph, and campaign corrections. #492 candidate and stable gates SHALL freshly reconcile every accepted issue's explanation, behavior/capability, acceptance, release role, non-goals/failures, diagram meaning, and task mirror in addition to objective IssueOps and published-main readback.

#### Scenario: Candidate semantic readback
- **WHEN** the exact RC input is evaluated
- **THEN** #492 reads the full twenty-nine-child hierarchy and every accepted issue packet from exact published `main`, requires every implementation-bearing child except release-governance #499 to be closed, and separately requires #499 exact `candidate_ready`

#### Scenario: Stable semantic readback
- **WHEN** accepted RC intake and the pre-stable audit finish
- **THEN** #492 repeats full-set semantic and objective readback, requires #499 exact `stable_ready` and closure, and closes last only after every child and hosted stable proof agree

#### Scenario: Migration is partial or raced
- **WHEN** any body, task mirror, acceptance section, milestone, parent, blocker, diagram, or published-main identity is missing, stale, or inconsistent
- **THEN** shared planning task 1.4 remains unchecked and no affected readiness, handoff, candidate, or stable transition proceeds

### Requirement: Guidance has one cross-project semantic owner
The global `issue-spec-writing` guidance SHALL define `Why` as actor/current state/consequence/outcome, `What Changes` as observable behavior plus ownership, truthful release-role and diagram-boundary prose, and the eight-question Sol audit. ProjectAtlas repository guidance and issue forms SHALL carry its exact planned order and objective constraints. Any version-matched ProjectAtlas plugin update SHALL be limited to `plugins/projectatlas/skills/projectatlas/SKILL.md` and its generated/package mirrors and SHALL NOT duplicate the contract into repository-root model folklore or a new guidance framework.

#### Scenario: Global and ProjectAtlas guidance agree
- **WHEN** an agent plans a ProjectAtlas issue after the guidance integration lands
- **THEN** the cross-project semantic definitions and ProjectAtlas structural order lead to one coherent packet and one implementation checklist

#### Scenario: No ProjectAtlas-specific plugin change is needed
- **WHEN** the global skill plus repository guidance already communicate the complete contract to the installed ProjectAtlas workflow
- **THEN** the version-matched plugin may remain unchanged after an explicit ownership review rather than receiving duplicate prose
