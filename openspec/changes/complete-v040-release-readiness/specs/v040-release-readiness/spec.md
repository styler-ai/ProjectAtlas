## ADDED Requirements

### Requirement: Release readiness has one truthful mapped owner
Issue #311 SHALL mirror the local `complete-v040-release-readiness` tasks exactly, SHALL be mapped in `openspec/issue-map.json`, and SHALL contain only prepublication work that can be completed before the v0.4.0 milestone gate runs.

#### Scenario: Obsolete initiative tasks are reconciled
- **WHEN** release readiness is prepared
- **THEN** the stale split of the completed #308 program is replaced by the concise local readiness checklist and IssueOps reports matching local and GitHub task text and state

#### Scenario: Readiness remains incomplete
- **WHEN** any mapped readiness task is unchecked or any other v0.4.0 milestone issue remains open
- **THEN** #311 remains open and milestone completion is not claimed

### Requirement: One exact final candidate owns all prepublication proof
ProjectAtlas SHALL bind final release readiness to one exact `dev` candidate after every v0.4.0 implementation and readiness artifact is merged. Required local and hosted evidence SHALL apply to that candidate, and a later candidate change SHALL invalidate the affected evidence.

#### Scenario: Candidate proof is current
- **WHEN** all required local gates and hosted runs complete successfully
- **THEN** their candidate identity matches the locked `dev` head and the release promotion uses that exact content

#### Scenario: Candidate changes after proof
- **WHEN** any commit changes the locked candidate
- **THEN** affected hosted proof is rerun on the new head before readiness can complete

#### Scenario: A required gate fails or is skipped
- **WHEN** a required local check, hosted job, platform row, review thread, or release preflight is failed, cancelled, skipped, stale, or unresolved
- **THEN** release readiness remains incomplete

### Requirement: Existing workflows prove the real release boundary
Release readiness SHALL use the existing `01-CI`, `optional-parser-pack`, and `02-Release` workflows. It SHALL require cross-platform packaged CLI/MCP smoke, explicit empty-cache Linux and Windows optional-parser construction, and `02-Release` package and installer proof with `prepublish_only=true`.

#### Scenario: Clean optional-parser proof succeeds
- **WHEN** the final candidate is dispatched with `clean_construction=true` and `target=all`
- **THEN** Linux and Windows bypass cache restore and save, complete construction plus fresh-runner and runtime proof, and produce the complete aggregate result

#### Scenario: Prepublish release proof succeeds
- **WHEN** `02-Release` runs on the exact candidate with version `v0.4.0` and `prepublish_only=true`
- **THEN** every required package and installer smoke job succeeds without creating a tag or GitHub release

#### Scenario: Ordinary candidate behavior succeeds
- **WHEN** exact-head `01-CI` completes
- **THEN** Rust verification and packaged E2E smoke succeed on Linux, Windows, macOS x64, and macOS arm64

### Requirement: Promotion remains reversible until readiness is complete
The release process SHALL leave `main`, version tags, and GitHub releases untouched until every readiness task is complete. It SHALL prepare an exact `dev`-to-`main` promotion only after review feedback, OpenSpec, IssueOps, ProjectAtlas, package, installer, and platform evidence is reconciled.

#### Scenario: Readiness is incomplete
- **WHEN** any readiness condition is not proven
- **THEN** `main`, tags, and published releases remain unchanged

#### Scenario: Readiness is complete
- **WHEN** every v0.4.0 milestone issue is checked and closed and the exact promotion is ready
- **THEN** milestone IssueOps passes and the existing main-triggered release path may publish the candidate

### Requirement: Post-release verification and cleanup have a durable owner
Before #311 closes, a dedicated non-milestone v0.4.0 post-release issue SHALL own independent publication verification and workspace consolidation. The release goal SHALL remain active until that issue is completed.

#### Scenario: Post-release work is handed off
- **WHEN** prepublication readiness is complete
- **THEN** the follow-up issue visibly requires tag, release, asset, checksum, installer, runtime, plugin, MCP, CLI, and representative E2E verification before cleanup

#### Scenario: Release is not independently verified
- **WHEN** v0.4.0 is absent, incomplete, inconsistent, or failing installed smoke
- **THEN** no ProjectAtlas branch, worktree, or external checkout is removed

#### Scenario: Cleanup is safe to begin
- **WHEN** v0.4.0 is published and independently verified
- **THEN** every ProjectAtlas branch and worktree is inventoried for dirtiness, unique commits, PR ownership, and merged or superseded state before removal

#### Scenario: Cleanup encounters uncertain work
- **WHEN** a branch, worktree, or checkout contains dirty, unique, unmerged, or unclassified work
- **THEN** it is retained until that work is preserved, merged, or explicitly superseded

#### Scenario: Cleanup completes
- **WHEN** all obsolete ProjectAtlas lanes are safely removed
- **THEN** worktree records are pruned, active checkouts live only under the primary repository's ignored `.worktrees` directory, and no external sibling ProjectAtlas checkout remains
