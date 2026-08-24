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

### Requirement: The tracked body manifest is the sole publication source
The planning change SHALL track one `candidate-issue-bodies.json` with schema version 1, SHA-256, UTF-8/LF normalization with one trailing LF, and exactly one unique entry for each of issue #500 plus the twenty-nine accepted/current-candidate v0.5 issues. Every entry SHALL contain its issue number, complete sanitized Markdown as `body_lines`, and the SHA-256 of the exact bytes reconstructed by joining those lines with LF. The issue set SHALL be exact and unique. Ignored `.tmp` body copies MAY exist for author convenience but SHALL match the reconstructed body bytes exactly and SHALL NOT authorize review, publication, repair, or readiness.

After independent Sol and hosted Codex review accept an exact planning-PR head, primary Sol SHALL validate the manifest schema, hash algorithm, normalization, exact issue set, uniqueness, complete content, and every digest. It SHALL publish only the exact body bytes reconstructed from the manifest and immediately read back every live body as the same normalized UTF-8/LF bytes and digest. A malformed entry, missing or extra issue, hash mismatch, concurrent mutation, partial write, stale readback, or any byte difference SHALL fail closed. The temporary manifest SHALL remain tracked through exact body and planning-`main` readback and SHALL be removed with the candidate graph manifest only at the later atomic graph-promotion cleanup after hosted relationship bootstrap also agrees; it SHALL NOT become a permanent body store or evidence ledger.

#### Scenario: Every public body byte is reviewable at the accepted head
- **WHEN** independent Sol or hosted Codex reviews the exact planning-PR head
- **THEN** the tracked manifest exposes the complete normalized Markdown and digest for the exact thirty-issue set without depending on ignored workspace files

#### Scenario: Publication input or live readback differs
- **WHEN** manifest validation, ignored-copy comparison, write-time state, or normalized live readback finds a malformed schema, wrong issue set, stale body, byte difference, or digest mismatch
- **THEN** primary Sol performs no downstream transition and repairs or rereviews the tracked publication source before retrying

### Requirement: Candidate graph staging does not become live authority
The planning change SHALL keep #497-#500 task ownership mapped under `openspec/issue-map.json.changes`, as required for every immediate OpenSpec `tasks.md`, while keeping authoritative `release_graphs.v0.5.0-00` aligned with the current hosted twenty-five-child v0.5 graph. Task mapping alone SHALL NOT assign milestone membership, hierarchy, blockers, or release readiness. The separate tracked change-local `candidate-issue-map.json` SHALL contain only #499's future campaign declaration and the complete twenty-nine-child replacement graph. IssueOps SHALL ignore that graph manifest for live reconciliation.

After independent Sol review and a new hosted Codex review accept the corrected exact planning-PR head, primary Sol SHALL validate the tracked body manifest, publish only the exact body bytes reconstructed from it, and read back the same normalized bytes and hashes while that PR remains open and the authoritative/live graph remains at twenty-five children. The temporary body-to-`main` architecture-link gap SHALL authorize no readiness or downstream transition. Normal unfiltered IssueOps/CI SHALL then pass. Trusted-default-branch repository dispatch SHALL remain the sole normal merge route and SHALL be attempted first. Only the one proven self-repair whose old default-branch authorizer cannot consume its own accepted correction MAY use the bounded bootstrap defined below. After exact planning-main and repaired-authorizer readback, #497, the shared #498/#499 change, and #500 SHALL expose only accepted checked contract/specification work in section 1; every implementation, test, campaign/GitHub synchronization, and final implementation-versus-diagram task SHALL live in section 2 or later, with every incomplete or not-yet-accepted task unchecked. #500 task 2.1 SHALL remain unchecked while independent shared task 1.4 accepts and reads back its exact handoff; tasks 2.2-2.3 SHALL remain unchecked for post-activation delivery. Exact issue-map owner slices and published body task mirrors SHALL agree. Only then SHALL the independent shared task apply and read back #497-#500's v0.5 milestone/status and the complete hosted twenty-nine-child native bootstrap from `candidate-issue-map.json`. After exact body, planning-main, task-classification, and hosted relationship readback agree, a separate narrow promotion PR SHALL atomically replace only active `release_graphs.v0.5.0-00` with that graph manifest's graph/campaign and remove both candidate manifests. Exact promoted-main/IssueOps/hosted/dependency/semantic readback SHALL accept the task 2.1 handoff and precede shared-task synchronization and every dependency-permitted delivery handoff; activation SHALL NOT mark incomplete delivery complete or close an implementation-bearing issue. No readiness, handoff, merge authorization for implementation, closure, or release transition SHALL proceed while any required PR, manifest, body, task classification, issue state, dependency, hosted, authoritative-graph, or published-main state differs.

#### Scenario: Exact planning head is reviewed before body bootstrap
- **WHEN** the corrected planning-PR head has not received both an independent Sol review and a new hosted Codex review for that exact head
- **THEN** no body, milestone, native relationship, or authoritative graph mutation proceeds

#### Scenario: Bodies are published while the reviewed planning PR remains open
- **WHEN** both exact-head reviews accept the corrected planning PR while #497-#500 remain unmilestoned and without native release relationships
- **THEN** primary Sol validates the tracked body manifest, publishes only its reconstructed exact thirty bodies, and reads back every normalized byte and hash while the active release graph continues to validate the current twenty-five-child hosted graph
- **AND** the temporary body-to-`main` architecture-link gap remains fail-closed for readiness while the separate graph manifest preserves the future graph without activating live relationship checks

#### Scenario: Body bootstrap makes the planning PR executable
- **WHEN** the tracked body manifest passes schema, exact-set, content, digest, ignored-copy, task-mirror, and normalized live-readback validation while the active graph still matches live hosted state
- **THEN** normal unfiltered IssueOps/CI passes before primary Sol attempts the trusted-default-branch repository-dispatch merge and reads back its exact `main` artifacts

### Requirement: Trusted-authorizer self-repair has one bounded bootstrap exception
Repository dispatch from the trusted default branch SHALL remain the sole normal merge route. The planning PR MAY use one non-reusable administrator bootstrap only when its accepted immutable head is itself the exact repair for a proven old-default-branch authorizer failure and `issueops-merge-authorized` is the sole authorization blocker. Before mutation, primary Sol SHALL pin the reviewed head, base branch and exact current `main`, expected squash-result tree, complete branch-protection state, and every required context/app binding; all other required checks SHALL be green and every review finding SHALL be resolved.

The exception SHALL temporarily remove only administrator enforcement, admin-squash only the pinned head, and restore administrator enforcement in a mandatory finally path regardless of merge-command outcome. It SHALL NOT remove or rebind any required check, forge a status, weaken strictness, change another protection field, or create another merge route. Downstream work SHALL require a successful merge command plus exact readback that the full protection state is restored with administrator enforcement active, the squash commit has the pinned base/`main` as its sole parent and the pinned expected tree, `main` points to that commit, and an `issueops_merge` dispatch executed from repaired `main` returns the idempotent `already-satisfied` outcome for the merged exact head without state drift.

#### Scenario: Normal authorizer can consume the current implementation
- **WHEN** trusted-default-branch repository dispatch passes and confirms the exact planning merge
- **THEN** primary Sol uses that sole normal path and does not change branch protection or invoke the bootstrap exception

#### Scenario: The accepted planning PR is the proven authorizer self-repair
- **WHEN** every immutable precondition is pinned, every ordinary required check is green, all review findings are resolved, and only `issueops-merge-authorized` fails because trusted default-branch execution cannot consume this PR's accepted authorizer repair
- **THEN** primary Sol may remove only administrator enforcement, admin-squash the pinned head inside mandatory finally restoration, prove exact restored protection and parent/tree/`main`, and obtain repaired-main idempotent-dispatch readback before any downstream transition

#### Scenario: Bootstrap eligibility, restoration, or readback differs
- **WHEN** any precondition, merge command, finally restoration, protection/check-app binding, parent, tree, `main`, or repaired-dispatch outcome is missing, stale, failed, or mismatched
- **THEN** primary Sol terminates at restoration/recovery with no downstream handoff, graph activation, required-check removal or rebind, forged status, or reusable exception

#### Scenario: Candidate specification tasks are complete before activation
- **WHEN** exact planning-main readback proves checked section 1 specification work for #497, #498/#499, and #500 while every owner-specific delivery task lives in section 2 or later, each checked state matches substantive completion, every incomplete delivery task remains unchecked, and every issue-map/body mirror agrees
- **THEN** the unchanged planned-issue contract gate admits all four open issues for milestone planning without representing their delivery as complete

#### Scenario: Candidate task classification is incomplete or false
- **WHEN** a candidate change has no checked section 1 specification task, has an unchecked section 1 task, keeps implementation or final review in section 1, falsely checks incomplete delivery, or differs from its owner slice/body mirror
- **THEN** IssueOps and semantic readiness reject milestone/native activation and graph promotion

#### Scenario: Hosted bootstrap is partial, premature, or raced
- **WHEN** before exact checked specification-task classification any candidate milestone/native graph activation is attempted, or afterward any candidate body, task state, milestone, parent, blocker, mapping, graph node, or published revision is missing, stale, or mismatched during controlled reconciliation
- **THEN** IssueOps and semantic readiness remain fail-closed and no downstream transition proceeds

#### Scenario: Checked-contract candidates enter the release graph before promotion
- **WHEN** the independent shared task applies #497-#500's v0.5 milestone/status plus every native relationship from the accepted graph manifest
- **THEN** complete hosted readback proves all four are open executable children of #492, every section 1 specification task is checked, every incomplete delivery task remains truthfully unchecked, and the declared blocker edges still constrain handoff

#### Scenario: Candidate graph is promoted
- **WHEN** exact body/planning-main/task-classification readback agrees and every hosted #497-#500 milestone/native relationship matches the accepted graph manifest
- **THEN** a separate narrow PR atomically replaces only `openspec/issue-map.json.release_graphs.v0.5.0-00` with the graph manifest's campaign declaration and replacement graph, removes both candidate manifests, and requires exact merged-main plus live IssueOps/hosted/dependency readback before the independent shared task completes or any delivery handoff proceeds

### Requirement: The complete v0.5 set is migrated and reread semantically
Sol SHALL derive the tracked body-manifest entries for issue #500 plus the twenty-nine accepted/current-candidate v0.5.0 bodies from exact live bodies or the latest candidate drafts, preserving authoritative task text and checked state while applying only required explanatory, acceptance, graph, campaign, and task-classification corrections; after review, that manifest SHALL be the sole publication source. #500 task 2.1 SHALL own exact-head reviews, strict tracked-manifest validation, open-PR exact-byte publication and normalized hash readback, green unfiltered planning CI, planning merge/main readback, and the exact handoff to independent shared `complete-v050-release-readiness` task 1.4, and SHALL remain unchecked until that shared task accepts and reads the complete handoff. The shared task SHALL own checked-contract verification for #497-#500, truthful later-section task-state readback, milestone/status/native bootstrap, atomic graph promotion and two-manifest cleanup, and authoritative/native/live readback without marking incomplete delivery complete or closing an implementation issue. After promotion, task 2.1 MAY be checked through normal synchronization, #500 task 2.2 SHALL own Luna's objective repository integration, and task 2.3 SHALL own final implementation-versus-diagram review; #497, #498, and #499 SHALL likewise retain their incomplete later-section delivery unchecked. Each issue SHALL close only after its own delivery tasks complete. #492 candidate and stable gates SHALL freshly reconcile every accepted issue's explanation, behavior/capability, acceptance, release role, non-goals/failures, diagram meaning, and task mirror in addition to objective IssueOps and published-main readback.

#### Scenario: Candidate semantic readback
- **WHEN** the exact RC input is evaluated
- **THEN** #492 reads the full twenty-nine-child hierarchy and every accepted issue packet from exact published `main`, requires every implementation-bearing child except release-governance #499 to be closed, and separately requires #499 exact `candidate_ready`

#### Scenario: Stable semantic readback
- **WHEN** accepted RC intake and the pre-stable audit finish
- **THEN** #492 repeats full-set semantic and objective readback, requires #499 exact `stable_ready` and closure, and closes last only after every child and hosted stable proof agree

#### Scenario: Migration is partial or raced
- **WHEN** any manifest entry or digest, body, task mirror, acceptance section, milestone, parent, blocker, diagram, or published-main identity is missing, stale, or inconsistent
- **THEN** the independent shared planning task remains unchecked and no affected readiness, handoff, candidate, or stable transition proceeds

#### Scenario: Activation and delivery-task completion do not form a cycle
- **WHEN** exact-head review, strict tracked-manifest validation, open-PR exact-byte body publication/hash readback, and green planning merge/readback prove every candidate change's checked section 1 specification contract while #500 publication/bootstrap task 2.1 remains unchecked pending independent shared-task acceptance of its handoff
- **THEN** the independent shared task may activate/promote all four open issues and synchronize itself while their incomplete later-section delivery tasks remain unchecked, after which task 2.1 may be checked from exact accepted-handoff readback
- **AND** each issue can subsequently complete and check its implementation, tests, GitHub synchronization, and final review through normal validation before closure

### Requirement: Guidance has one cross-project semantic owner
The primary Sol-owned personal global `issue-spec-writing` guidance SHALL remain the cross-project semantic owner for `Why` actor/current trigger/consequence/outcome, `What Changes` observable before-to-after behavior plus the smallest owner, conditional reproduced-bug facts versus truthful proof-gap hardening, the canonical acceptance order, diagram-boundary prose, and the eight-question Sol audit without a score, LLM gate, or ledger. Primary Sol has completed that global contract. Luna SHALL treat it as read-only compatibility authority and SHALL own only the repository checker/self-test, ProjectAtlas issue forms and repository guidance, and any version-matched ProjectAtlas plugin update proven necessary by ownership review. Such plugin updates SHALL be limited to `plugins/projectatlas/skills/projectatlas/SKILL.md` and its generated/package mirrors and SHALL NOT duplicate the contract into repository-root model folklore or a new guidance framework.

#### Scenario: Repository implementation consumes the completed global contract
- **WHEN** Luna implements the ProjectAtlas checker, forms, repository guidance, or a proven necessary version-matched plugin update
- **THEN** it verifies compatibility against the exact primary Sol-owned personal global skill without mutating that global source

#### Scenario: Global and ProjectAtlas guidance agree
- **WHEN** an agent plans a ProjectAtlas issue after the guidance integration lands
- **THEN** the cross-project semantic definitions and ProjectAtlas structural order lead to one coherent packet and one implementation checklist

#### Scenario: No ProjectAtlas-specific plugin change is needed
- **WHEN** the global skill plus repository guidance already communicate the complete contract to the installed ProjectAtlas workflow
- **THEN** the version-matched plugin may remain unchanged after an explicit ownership review rather than receiving duplicate prose
