## ADDED Requirements

### Requirement: Lean OpenSpec checklist synchronization
Every active OpenSpec change with a task list SHALL be mapped in `openspec/issue-map.json`. Its authoritative GitHub issue checklist SHALL exactly mirror the local task text, order, ownership, and checked state. One issue SHALL own the full checklist unless a split is required; split ownership SHALL use ordered, disjoint, gap-free ranges that cover every local task exactly once. A closed issue SHALL NOT contain unchecked authoritative tasks.

The synchronizer SHALL parse only `OpenSpec Tasks` or `OpenSpec Task Checklist` and their numbered task subsections. Unrelated checkboxes SHALL NOT satisfy or alter the authoritative checklist.

#### Scenario: Local and GitHub checklists drift
- **WHEN** task text, order, owner, or checked state differs
- **THEN** IssueOps fails and identifies the issue/change mismatch

#### Scenario: An issue range has a gap or overlap
- **WHEN** multi-issue ownership does not cover the local checklist exactly once in order
- **THEN** IssueOps fails before trusting remote state

#### Scenario: An unrelated checkbox changes
- **WHEN** a checkbox outside the authoritative task section is added or changed
- **THEN** the OpenSpec task comparison is unaffected

### Requirement: Active issues keep a readable pre-mortem contract
Every open mapped issue SHALL contain the concise v0.3.26 planning structure used by #305: why, what changes, capabilities, release scope, non-goals, pre-mortem, and exactly one authoritative OpenSpec task section. The pre-mortem SHALL contain likely failure modes and one or more mitigation checkboxes. Each mitigation SHALL reference one or more task IDs owned by that issue, and its checked state SHALL equal whether all referenced tasks are checked.

This issue-quality contract SHALL NOT require task receipts, SHA or commit-permalink evidence, generated comments, unique tests, or separate mitigation tasks when existing OpenSpec tasks already own the behavior. Closed historical issues MAY retain their shipped presentation; open mapped issues and future work SHALL satisfy the current contract.

#### Scenario: API-created issue bypasses the form
- **WHEN** an open mapped issue omits a required #305 planning section or leaves its pre-mortem empty
- **THEN** IssueOps fails even if the authoritative OpenSpec checklist itself matches

#### Scenario: Mitigation references real implementation tasks
- **WHEN** a mitigation checkbox references owned OpenSpec task IDs
- **THEN** IssueOps accepts it only when its checked state equals the combined checked state of those tasks

#### Scenario: Mitigation references an unknown or foreign task
- **WHEN** a mitigation names a task ID outside the issue's authoritative task slice
- **THEN** IssueOps fails without treating the mitigation as task evidence

#### Scenario: Historical closed issue predates the contract
- **WHEN** a previously completed mapped issue lacks the newer body sections
- **THEN** IssueOps continues to validate its authoritative checklist without requiring a body rewrite

### Requirement: Behavior-focused task completion
A task MAY be checked after its implementation or artifact is complete and the smallest meaningful unit, integration, E2E, smoke, or validation check appropriate to the actual risk passes. Multiple related tasks MAY share one coherent test. Documentation and planning tasks SHALL NOT require artificial production tests.

Task completion SHALL NOT require a unique test identifier, task-level verification plan, task-evidence ledger, tested-commit digest, source permalink, hosted run link, exact-head receipt, OpenSpec commit-link block, rendered evidence comment, task-level PR path declaration, issue snapshot, or post-merge issue sealing.

#### Scenario: Several tasks share one behavior test
- **WHEN** one focused test covers a coherent behavior implemented by multiple checklist tasks
- **THEN** those tasks may complete without duplicate test wrappers or receipt rows

#### Scenario: Normal CI runs for a commit
- **WHEN** GitHub Actions runs the ordinary tests for a commit
- **THEN** IssueOps does not require a second repository-authored SHA receipt

### Requirement: Ordinary blocking Rust quality gates
Meaningful implementation and integration boundaries SHALL run formatting, locked all-target workspace check, locked strict Clippy, locked workspace tests, locked stable workspace doctests, warning-free rustdoc, ProjectAtlas source lints, dependency policy, and affected behavior/E2E checks. A failing required command SHALL remain failed; diagnostic or artifact handling SHALL NOT reinterpret it as success.

The workflow SHALL retain SHA-pinned GitHub Actions, least privilege, locked dependency commands, and applicable parser, package, signature, digest, checksum, and installer integrity checks. It SHALL NOT require nextest, LLVM coverage, changed-source mutation, full mutation, or a repository-wide coverage/mutation target as part of this change.

#### Scenario: One ordinary Rust gate fails
- **WHEN** formatting, check, Clippy, tests, doctests, rustdoc, source lint, dependency policy, or an affected behavior check fails
- **THEN** the implementation boundary remains failed

#### Scenario: A task receipt is absent
- **WHEN** the focused behavior tests and ordinary workspace gates pass without a task receipt
- **THEN** the implementation may progress normally

### Requirement: Ordinary PR and release boundaries
Pull requests SHALL reference an issue, carry the intended milestone, pass ordinary CI, and have no unresolved actionable review threads. Ordinary PR IssueOps SHALL synchronize mapped checklists without requiring every issue in the milestone to be complete. Full milestone checklist completion SHALL run only as a release gate.

Significant compiling v0.4 issue slices SHALL integrate into `dev`. `main` and release publication SHALL remain unchanged until the combined `dev` surface passes its required behavior and workspace checks. Release artifact integrity checks SHALL remain in the release path.

#### Scenario: An unrelated milestone issue is incomplete
- **WHEN** a pull request's own mapped checklist is synchronized but another issue in the milestone remains open
- **THEN** ordinary PR IssueOps may pass while the release milestone gate remains incomplete

#### Scenario: A compiling issue slice is ready
- **WHEN** its focused tests and ordinary Rust/workspace gates pass
- **THEN** it may integrate into `dev` without waiting for release proof

#### Scenario: The combined dev surface is not green
- **WHEN** a required implementation, test, documentation, or integration check fails
- **THEN** `main` and release publication remain untouched
