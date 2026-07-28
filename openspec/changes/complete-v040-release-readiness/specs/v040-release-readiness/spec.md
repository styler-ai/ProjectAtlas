## ADDED Requirements

### Requirement: Release readiness has one truthful mapped owner
Issue #311 SHALL mirror the local `complete-v040-release-readiness` tasks exactly, SHALL be mapped in `openspec/issue-map.json`, and SHALL contain only prepublication work that can be completed before the v0.4.0 milestone gate runs.

#### Scenario: Obsolete initiative tasks are reconciled
- **WHEN** release readiness is prepared
- **THEN** the stale split of the completed #308 program is replaced by the concise local readiness checklist and IssueOps reports matching local and GitHub task text and state

#### Scenario: Readiness remains incomplete
- **WHEN** any mapped readiness task is unchecked or any other v0.4.0 milestone issue remains open
- **THEN** #311 remains open and milestone completion is not claimed

### Requirement: Prepublication proof follows behavior-relevant inputs
ProjectAtlas SHALL prove one clean release candidate after every v0.4.0 implementation, readiness artifact, and other milestone issue is reconciled. Commit SHAs SHALL remain provenance only. After checklist or other behavior-neutral metadata changes, strict OpenSpec, IssueOps, ProjectAtlas low lint, review, topology, and release-policy checks SHALL rerun while unaffected expensive proof remains valid. Source, dependency, lockfile, toolchain, workflow, packaging, configuration, parser-pack, platform, artifact-identity, and unknown changes SHALL invalidate the affected proof.

#### Scenario: Release-content proof is ready to reconcile
- **WHEN** all required local gates and hosted runs complete successfully
- **THEN** their owning behavior-relevant inputs and execution context match the locked release candidate and checklist reconciliation may begin

#### Scenario: Checklist state is reconciled
- **WHEN** the reconciliation commit and mirrored issue state change only behavior-neutral task metadata and the protected `dev` merge preserves the candidate tree
- **THEN** unaffected clean-construction and prepublish proof remains valid while cheap current-state closure gates rerun

#### Scenario: Promotion head changes after proof
- **WHEN** reconciliation or a later commit changes an owning behavior-relevant input or an unknown path
- **THEN** the affected proof is rerun before completion

#### Scenario: A required gate fails or is skipped
- **WHEN** a required local check, hosted job, platform row, review thread, or release preflight is failed, cancelled, skipped, stale, or unresolved
- **THEN** release readiness remains incomplete

### Requirement: Existing workflows prove the real release boundary
Release readiness SHALL use the existing `01-CI`, `optional-parser-pack`, and `02-Release` workflows. It SHALL require cross-platform source-built CLI/MCP E2E, explicit empty-cache Linux and Windows optional-parser construction, and `02-Release` package, optional-parser release-asset, and installer proof with `prepublish_only=true`.

#### Scenario: Clean optional-parser proof succeeds
- **WHEN** the release candidate's optional-parser inputs require a fresh dispatch with `clean_construction=true` and `target=all`
- **THEN** Linux and Windows bypass cache restore and save, complete construction plus fresh-runner and runtime proof, and produce the complete aggregate result

#### Scenario: Prepublish release proof succeeds
- **WHEN** `02-Release` runs for version `v0.4.0` with `prepublish_only=true` and an input-compatible clean optional-parser handoff
- **THEN** every required package, optional-parser release-asset, and installer smoke job succeeds without creating a tag or GitHub release

#### Scenario: Optional-parser handoff is stale or incomplete
- **WHEN** the referenced run, repository, workflow, event, behavior-relevant inputs, aggregate proof, clean receipt, archive size, archive digest, version, or supported target set differs
- **THEN** `02-Release` fails before publication and does not stage the optional-parser archives

### Requirement: Fresh review findings reopen affected release proof
Any review finding that changes release behavior or another proof input SHALL reopen the affected candidate, local, hosted, review, prepublish, and reconciliation tasks. Unaffected success SHALL remain reusable.

#### Scenario: Review finds a release blocker after reconciliation
- **WHEN** an actionable finding requires a workflow, runtime, spec, test, or published documentation change
- **THEN** #311 remains open, the finding receives an explicit local task owner, and all proof affected by the changed inputs is rerun

#### Scenario: Corrected head is ready to reconcile
- **WHEN** every actionable Codex and Dependabot finding is fixed or explicitly dispositioned and the corrected candidate passes its required local and hosted gates
- **THEN** the finite task-state-only reconciliation may begin

#### Scenario: Ordinary candidate behavior succeeds
- **WHEN** required `01-CI` completes
- **THEN** Rust verification and source-built CLI/MCP E2E smoke succeed on Linux, Windows, macOS x64, and macOS arm64

### Requirement: Compatible upgrades preserve cumulative token impact
The normal v0.4.0 database-open path SHALL preserve all compatible cumulative token-usage events and derived overview and trend totals from a released v0.3.26 project database. Later supported migrations SHALL preserve the same authored telemetry authority. A migration SHALL either commit the complete compatible state or leave the last-valid database unchanged.

#### Scenario: A released v0.3.26 project upgrades
- **WHEN** v0.4.0 opens a released-schema database containing token-impact history
- **THEN** the cumulative overview and every retained trend total match before migration, after migration, and after reopening the upgraded database

#### Scenario: Compatible telemetry contains an invalid predecessor row
- **WHEN** migration validation encounters malformed telemetry
- **THEN** the migration rolls back atomically and the last-valid predecessor database remains retryable without lost history

### Requirement: The token dashboard is truthful and focused
The human token-impact dashboard SHALL derive every live numeric field from the active project's persisted token report. Its normal no-artifact view SHALL focus on tokens, lookups performed, source-reconciled file reads avoided with their observed and modeled split, and modeled folder walks avoided without release-version, frozen-baseline, plain-control, or repeated-work benchmark comparisons. An explicitly supplied `--benchmark-results` artifact MAY add one bounded, visually separate comparison panel without changing any live total. At wide terminal sizes the dashboard MAY show a bounded connected, clustered, and depth-cued non-interactive constellation drawn only from resolved logical relations in the active project database; it SHALL NOT invent graph rows or imply complete graph analysis.

#### Scenario: Persisted impact is rendered
- **WHEN** the dashboard renders an overview without a benchmark artifact
- **THEN** its lookups, token equation, file-read total and split, persisted directory-walk steps, source rows, and composition reconcile exactly with that overview
- **AND** it omits frozen-baseline, plain-control, and repeated-work comparison rows

#### Scenario: Benchmark evidence is explicitly requested
- **WHEN** a caller supplies `--benchmark-results`
- **THEN** one bounded separate panel renders only the typed comparison state and values
- **AND** those values do not alter persisted tokens, file reads, folder walks, source rows, or composition.

#### Scenario: A wide project graph is available
- **WHEN** the active database returns resolved logical relations within the preview bounds
- **THEN** the right panel renders one deterministic connected static atlas from only those rows, gives graph-derived cluster and depth cues, keeps every node and edge inside the panel, and labels it as a bounded live snapshot

#### Scenario: The graph is empty, unavailable, or the terminal is narrow
- **WHEN** no resolved preview rows are available, the optional preview read fails, or the terminal cannot fit both columns
- **THEN** the dashboard remains usable, shows an explicit empty or unavailable state where applicable, and never substitutes demo data

### Requirement: Promotion remains reversible until readiness is complete
The release process SHALL leave `main`, version tags, and GitHub releases untouched until every readiness task is complete. It SHALL prepare a merge-commit `dev`-to-`main` promotion only after review feedback, OpenSpec, IssueOps, ProjectAtlas, package, installer, and platform evidence is reconciled. `main` SHALL be an ancestor of the promotion head, squash and rebase SHALL be refused, and the resulting merge commit SHALL have the promotion head as a parent with an identical Git tree.

#### Scenario: Readiness is incomplete
- **WHEN** any readiness condition is not proven
- **THEN** `main`, tags, and published releases remain unchanged

#### Scenario: Readiness is complete
- **WHEN** every v0.4.0 milestone issue is checked and closed and the promotion is ready
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
