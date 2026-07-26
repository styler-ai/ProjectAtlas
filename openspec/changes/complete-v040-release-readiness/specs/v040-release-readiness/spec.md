## ADDED Requirements

### Requirement: Release readiness has one truthful mapped owner
Issue #311 SHALL mirror the local `complete-v040-release-readiness` tasks exactly, SHALL be mapped in `openspec/issue-map.json`, and SHALL contain only prepublication work that can be completed before the v0.4.0 milestone gate runs.

#### Scenario: Obsolete initiative tasks are reconciled
- **WHEN** release readiness is prepared
- **THEN** the stale split of the completed #308 program is replaced by the concise local readiness checklist and IssueOps reports matching local and GitHub task text and state

#### Scenario: Readiness remains incomplete
- **WHEN** any mapped readiness task is unchecked or any other v0.4.0 milestone issue remains open
- **THEN** #311 remains open and milestone completion is not claimed

### Requirement: One exact promotion head owns all prepublication proof
ProjectAtlas SHALL first prove one locked release-content head after every v0.4.0 implementation, readiness artifact, and other milestone issue is reconciled. It SHALL then permit exactly one commit that changes only #311 OpenSpec task checkbox state and mirror that state to the GitHub issue. The protected `dev` landing SHALL use a merge commit that contains the task-state commit as a parent with an identical Git tree, and the resulting `dev` merge SHA SHALL be the exact promotion head. Clean optional-parser and prepublish evidence MAY carry forward only across that verified non-release-impacting tree change. Ordinary exact-head CI, strict OpenSpec, IssueOps, ProjectAtlas low lint, and review checks SHALL run as post-commit closure gates on the resulting `dev` SHA outside the reconciled checklist, #311 SHALL remain open until they pass, and any other tree change SHALL invalidate the affected evidence.

#### Scenario: Release-content proof is ready to reconcile
- **WHEN** all required local gates and hosted runs complete successfully
- **THEN** their identity matches the locked release-content head and one task-state-only reconciliation commit may be prepared

#### Scenario: Checklist state is reconciled
- **WHEN** the bounded reconciliation commit and mirrored issue state contain no change beyond completed #311 task checkboxes and the protected `dev` merge preserves that exact tree
- **THEN** the resulting `dev` merge SHA becomes the exact promotion head, unaffected clean-construction and prepublish proof remains valid, and #311 remains open while ordinary exact-head closure gates run

#### Scenario: Promotion head changes after proof
- **WHEN** reconciliation changes any other path or a later commit changes the exact promotion tree
- **THEN** readiness returns to the release-content lock and affected proof is rerun before completion

#### Scenario: A required gate fails or is skipped
- **WHEN** a required local check, hosted job, platform row, review thread, or release preflight is failed, cancelled, skipped, stale, or unresolved
- **THEN** release readiness remains incomplete

### Requirement: Existing workflows prove the real release boundary
Release readiness SHALL use the existing `01-CI`, `optional-parser-pack`, and `02-Release` workflows. It SHALL require cross-platform source-built CLI/MCP E2E, explicit empty-cache Linux and Windows optional-parser construction, and `02-Release` package and installer proof with `prepublish_only=true`.

#### Scenario: Clean optional-parser proof succeeds
- **WHEN** the exact release-content head is dispatched with `clean_construction=true` and `target=all`
- **THEN** Linux and Windows bypass cache restore and save, complete construction plus fresh-runner and runtime proof, and produce the complete aggregate result

#### Scenario: Prepublish release proof succeeds
- **WHEN** `02-Release` runs on the exact release-content head with version `v0.4.0` and `prepublish_only=true`
- **THEN** every required package and installer smoke job succeeds without creating a tag or GitHub release

#### Scenario: Ordinary candidate behavior succeeds
- **WHEN** exact-head `01-CI` completes
- **THEN** Rust verification and source-built CLI/MCP E2E smoke succeed on Linux, Windows, macOS x64, and macOS arm64

### Requirement: Promotion remains reversible until readiness is complete
The release process SHALL leave `main`, version tags, and GitHub releases untouched until every readiness task is complete. It SHALL prepare a merge-commit `dev`-to-`main` promotion only after review feedback, OpenSpec, IssueOps, ProjectAtlas, package, installer, and platform evidence is reconciled. `main` SHALL be an ancestor of the promotion head, squash and rebase SHALL be refused, and the resulting merge commit SHALL have the promotion head as a parent with an identical Git tree.

#### Scenario: Readiness is incomplete
- **WHEN** any readiness condition is not proven
- **THEN** `main`, tags, and published releases remain unchanged

#### Scenario: Readiness is complete
- **WHEN** every v0.4.0 milestone issue is checked and closed and the exact promotion is ready
- **THEN** milestone IssueOps passes and the existing main-triggered release path may publish the candidate

#### Scenario: Promotion preserves verified content
- **WHEN** the prepared promotion is merged after readiness completes
- **THEN** the resulting `main` SHA has the verified promotion head as a parent and the same Git tree, and `02-Release` repeats verification, packaging, and installer smoke on that `main` SHA before publication

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
