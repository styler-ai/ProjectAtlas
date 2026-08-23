## ADDED Requirements

### Requirement: #492 is the feature-free v0.5 release hierarchy root
#492 SHALL have no native parent and SHALL be the sole direct parent of every other accepted `v0.5.0-00` issue. It SHALL be directly blocked by every child and SHALL implement no feature or bug.

#### Scenario: Milestone progress is inspected
- **WHEN** the native hierarchy is read
- **THEN** #492 exposes every accepted child once while each issue's direct blocker list independently communicates execution order

#### Scenario: A child is incomplete
- **WHEN** any child issue, mapped task, required proof, or review remains open
- **THEN** #492 cannot freeze, publish, promote, or close

### Requirement: Accepted issue evidence is published before implementation
Every issue assigned to `v0.5.0-00` with `status:ready` SHALL resolve its mapped OpenSpec task source and every architecture URL, heading, and Mermaid block from an exact clean checkout of the live default-branch revision. Candidate-local validation SHALL remain required for proposed artifacts but SHALL NOT authorize readiness, milestone assignment, native release relationships, implementation handoff, merge, or release.

#### Scenario: A planning slice publishes new evidence
- **WHEN** a planning pull request has no native closing issue and its candidate OpenSpec and Mermaid checks pass
- **THEN** it may publish the proposed specification and architecture artifacts without claiming that an implementation issue is ready

#### Scenario: Candidate-only evidence is presented as published
- **WHEN** a planned, implementation, merge-authorization, milestone, or release check resolves an artifact only from a candidate, stale, or dirty checkout
- **THEN** readiness fails until a planning pull request lands and the exact live default-branch artifact is read back successfully

#### Scenario: The local checkout is not the exact published root
- **WHEN** Git reports another top-level root, a malformed local HEAD, a well-formed but different HEAD, or tracked modifications
- **THEN** published readiness fails before the checkout's OpenSpec or architecture artifacts authorize state

#### Scenario: Only ignored untracked notes exist
- **WHEN** the exact live default-branch checkout differs only by untracked files excluded from tracked publication identity
- **THEN** published readiness may continue without treating those notes as repository evidence

#### Scenario: Published identity cannot be established
- **WHEN** Git inspection times out or raises an OS/process error, or GitHub returns a malformed default-branch identity, ref, or SHA
- **THEN** the check fails closed with no fallback to candidate or cached identity

#### Scenario: The default branch moves during merge authorization
- **WHEN** the live default-branch SHA differs between published-snapshot admission, merge preflight, or final authorization reread
- **THEN** authorization fails and cannot arm or preserve a merge decision based on the earlier snapshot

### Requirement: Native relationship changes are prevalidated and reverse drift is repaired
IssueOps SHALL derive a bounded transition plan from the declared release graph and current native state before any relationship mutation. The requested relation kind, orientation, issue, related issue, and operation SHALL match exactly one missing-or-extra transition toward the declaration. Post-mutation readback and complete graph reconciliation SHALL remain mandatory. Issue events SHALL repair invalid closed state in both blocker directions within the declared graph and SHALL validate a declared issue even when a `demilestoned` event removed its live milestone.

#### Scenario: A relationship request does not match the declared transition
- **WHEN** the requested tuple is unknown, ambiguous, reversed, graph-widening, or does not repair one exact missing-or-extra relation
- **THEN** IssueOps rejects it before any GitHub mutation and does not rely on rollback after reconciliation failure

#### Scenario: A blocker reopens
- **WHEN** a declared blocker becomes open while one or more graph-bounded reverse dependents are closed
- **THEN** IssueOps derives reverse direct-blocker adjacency, reopens or fails every invalid closed dependent through a bounded queue, and invalidates affected implementation or merge readiness

#### Scenario: A declared issue is demilestoned
- **WHEN** an issue event removes the live milestone from an issue still owned by a release graph
- **THEN** IssueOps selects the graph from the issue map, reports targeted milestone drift, and does not skip validation because the event payload milestone is null

### Requirement: Release input is exact and complete
#492 SHALL freeze one exact `main` revision only after every accepted child, task, owning proof, document/diagram, dependency, release note, and actionable human/automated review finding is complete and the published-default-branch IssueOps milestone gate has read back every accepted issue's OpenSpec task source and architecture target. Technical disposition MAY satisfy only reproducible no-change work or a genuinely non-actionable observation; it SHALL NOT convert partial accepted work into readiness.

#### Scenario: Evidence-led no-change issue
- **WHEN** a measurement or reproduction task proves existing behavior already satisfies its contract
- **THEN** the issue may close with reproducible no-product-change evidence and its required review/gates

#### Scenario: Revision or artifact changes
- **WHEN** any candidate input changes after proof
- **THEN** the complete public-surface and holistic proof restarts for the new exact input

#### Scenario: Published issue evidence drifts
- **WHEN** a mapped task source, architecture document, heading, Mermaid block, issue mirror, or default-branch identity is missing or inconsistent
- **THEN** #492 stops acceptance, returns the gap to its specification owner, and performs no feature or bug repair

### Requirement: Every installed CLI and MCP route executes
The release gate SHALL derive and reconcile the complete installed CLI command/nested-command and MCP tool inventory, including unchanged routes, and SHALL safely execute each supported route on every supported platform.

#### Scenario: Read-only route
- **WHEN** a navigation, source-evidence, format, freshness, health, status, or settings route is selected
- **THEN** the installed candidate proves root/worktree identity, output/error schema, bounds, compatibility, and actual behavior rather than help/schema presence

#### Scenario: Mutation or administration
- **WHEN** purpose, worktree, task, repair, resolve, reset, strip, or another mutating/administrative route executes
- **THEN** it uses isolated disposable fixtures, proves confirmation/refusal/cleanup, and leaves unrelated state unchanged

### Requirement: Holistic proof uses packaged installed products
One clean E2E SHALL compose binary/npm/plugin/host installation, init/database, scan, purpose-led navigation, graph/source evidence, PHP, PDF/DOCX, analysis, worktree/watcher/telemetry, parser capability, update/repair/uninstall, concurrency, cancellation, failure recovery, and compatible rollback using exact candidate artifacts.

#### Scenario: Supported installed workflow
- **WHEN** the candidate is installed into isolated homes/config/cache/repositories/databases/host roots
- **THEN** every composed boundary returns consistent runtime/plugin/skill/MCP/CLI/host identity and exact source evidence

#### Scenario: Ambient checkout or database is present
- **WHEN** developer state could satisfy a route accidentally
- **THEN** the harness proves the packaged path and isolated selected database are used or fails

### Requirement: Updating from v0.4.5 is a publication hard gate
Before RC or stable publication, the release gate SHALL install `v0.4.5`, create and exercise a real project database with durable authored and runtime state, update that same installation and database to the exact candidate on every supported platform, and prove schema/runtime/plugin/skill/MCP/CLI/host convergence. Publication SHALL fail when migration, state preservation, interrupted-update recovery, safe retry, or compatible rollback/refusal behavior is incomplete.

#### Scenario: Supported in-place update
- **WHEN** an exercised `v0.4.5` installation and database update to the exact candidate
- **THEN** project identity, authored purposes, telemetry, registered worktrees, selected roots, current generation, and source evidence remain correct without destructive reinitialization

#### Scenario: Update or migration is interrupted
- **WHEN** installer activation or database migration fails at an injected boundary
- **THEN** no partial candidate becomes active, the prior state remains usable or fails closed without corruption, repair/retry succeeds, and unrelated host or project state is unchanged

### Requirement: Confirmed defects return to owning issues
#492 SHALL classify each candidate observation. A confirmed defect SHALL return to an existing or new sanitized v0.5 IssueOps/OpenSpec owner for implementation, tests, and review; #492 SHALL NOT patch it.

#### Scenario: Candidate blocker
- **WHEN** installed or hosted proof finds a contract failure
- **THEN** publication/promotion stops, the owning fix lands on `main`, and complete proof restarts

#### Scenario: Non-defect observation
- **WHEN** evidence proves expected, unsupported-by-contract, duplicate, already-correct, or non-actionable behavior
- **THEN** it is recorded without weakening an accepted task

### Requirement: v0.5.0 begins with an independently read-back prerelease
With explicit authorization, `v0.5.0-rc1` SHALL publish as a non-draft prerelease from the exact accepted revision. Independent readback SHALL verify tag/revision, metadata, assets, checksums/integrity records, installers, npm, runtime/plugin/skill/MCP/CLI/host identity, and acceptance results. v0.4.5 SHALL remain Latest.

#### Scenario: Missing or mismatched release artifact
- **WHEN** any required tuple/asset/digest/version/readback is absent or inconsistent
- **THEN** RC acceptance fails and stable promotion is blocked

### Requirement: Stable promotion repeats complete proof and closes last
After an accepted candidate and explicit authorization, stable v0.5.0 SHALL repeat installed and hosted proof before becoming Latest. #492 SHALL remain open until downstream pins and final IssueOps/OpenSpec/review/milestone/workflow state agree and SHALL close last.

#### Scenario: Stable promotion
- **WHEN** no blocker remains and stable artifacts pass the complete proof
- **THEN** v0.5.0 becomes Latest only after exact readback and #492/milestone finalization
