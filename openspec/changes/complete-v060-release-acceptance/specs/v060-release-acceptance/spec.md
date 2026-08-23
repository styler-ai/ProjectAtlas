## ADDED Requirements

### Requirement: The release hierarchy exposes complete accepted scope
Issue #493 SHALL be the sole native parent of every other accepted `v0.6.0-00` issue and SHALL have no parent. Every child SHALL appear once in the declared release graph; direct blockers SHALL independently express genuine execution prerequisites.

#### Scenario: Initial hierarchy
- **WHEN** v0.6.0 contains #310 and #314
- **THEN** both are direct sub-issues of #493, #314 is blocked by #310, and #493 is blocked by both

#### Scenario: Later issue is accepted
- **WHEN** another issue enters the milestone
- **THEN** it becomes one direct #493 sub-issue and blocker while its own `blocked_by` list contains only genuine prerequisites

### Requirement: Release acceptance freezes exact complete inputs
#493 SHALL freeze one exact revision/artifact set only after every child issue, mapped task, required test/document/diagram, dependency, and actionable review finding is complete.

#### Scenario: Incomplete child or review
- **WHEN** any child, task, required proof, or actionable review remains open
- **THEN** candidate acceptance and publication are blocked

#### Scenario: Input changes
- **WHEN** the revision, artifact, dependency graph, or compatibility disposition changes
- **THEN** prior integrated proof is invalid and the complete candidate proof restarts

### Requirement: Every installed public route executes safely
The release gate SHALL reconcile the complete installed CLI command/nested-command and MCP tool inventory, including unchanged routes, and SHALL execute every supported route against isolated fixtures.

#### Scenario: Read-only route
- **WHEN** a navigation, source-evidence, format, freshness, or status route is exercised
- **THEN** its installed behavior, root identity, output/error schema, bounds, and compatibility match the accepted contract

#### Scenario: Mutating or administrative route
- **WHEN** a route can change purpose, task, worktree, Memory Atlas, database, or host state
- **THEN** it executes only against disposable isolated state and proves refusal, cleanup, and no ambient mutation

### Requirement: Holistic proof spans #310 and #314
The exact installed candidate SHALL pass one end-to-end workflow composing the accepted agent surface with Memory Atlas storage, reflection, recovery, and host boundaries.

#### Scenario: Clean installed workflow
- **WHEN** the candidate is installed into isolated home/config/cache/project state
- **THEN** install or upgrade, init, scan, navigation, graph/source evidence, CLI/MCP routing, authored-context write/read/recovery, host fallback, concurrency/pressure, privacy, failure recovery, uninstall, and compatible rollback all pass

### Requirement: Confirmed defects return to their owners
#493 SHALL implement no feature or bug. A confirmed candidate defect SHALL return to an existing or new sanitized issue with its own specification, implementation, tests, and review.

#### Scenario: Candidate defect
- **WHEN** integrated proof discovers a contract failure
- **THEN** release work stops, the owner fixes and merges it, and the complete proof restarts on the changed exact input

### Requirement: Prerelease and stable state are independently read back
`v0.6.0-rc1` SHALL be a non-draft prerelease and SHALL NOT replace stable v0.5.0 as Latest. Stable v0.6.0 SHALL repeat installed and hosted proof before Latest/downstream/milestone finalization.

#### Scenario: RC1 publication
- **WHEN** explicit authorization exists and all candidate proof passes
- **THEN** independent readback verifies tag/revision, metadata, assets, checksums, installers, runtime/plugin/skill/MCP identity, E2E results, and Latest protection

#### Scenario: Stable promotion
- **WHEN** the accepted candidate has no unresolved blocker and stable proof passes
- **THEN** v0.6.0 becomes Latest only after exact installed/hosted/downstream/issue/review/milestone state is verified

### Requirement: The release-acceptance issue closes last
#493 SHALL remain open until every child and required review is closed successfully and stable v0.6.0 readback is complete.

#### Scenario: Milestone finalization
- **WHEN** all acceptance tasks and final architecture reconciliation pass
- **THEN** #493 closes last and milestone progress reaches completion
