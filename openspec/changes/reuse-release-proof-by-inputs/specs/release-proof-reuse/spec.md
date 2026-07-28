## ADDED Requirements

### Requirement: Existing test coverage is preserved
ProjectAtlas SHALL retain its unit, integration, E2E, fault, concurrency, platform, security, installer, packaging, and release tests. Proof reuse SHALL skip only recomputation whose owning inputs and execution context are unchanged.

#### Scenario: Metadata-only change follows passed proof
- **WHEN** only IssueOps, OpenSpec checklist state, or other classified behavior-neutral metadata changes after all affected tests pass
- **THEN** the cheap current-state gates rerun and the unchanged expensive proof remains reusable

#### Scenario: Behavior-relevant input changes
- **WHEN** source, dependencies, lockfiles, toolchains, workflows, packaging, configuration, parser-pack inputs, or another owning input changes
- **THEN** every affected test or construction gate reruns before release

### Requirement: Reuse fails closed
Each reusable proof owner SHALL classify its behavior-relevant inputs explicitly and SHALL invalidate proof for an unknown change, changed execution platform or toolchain, partial or failed run, expired artifact, or unverifiable identity. Published benchmark campaigns SHALL bind their closed measurement-owning harness file set by content digest so methodology drift invalidates reuse without requiring current-commit equality.

#### Scenario: Unknown path changes
- **WHEN** a changed path cannot be proven behavior-neutral for the owning proof
- **THEN** that proof is invalidated

#### Scenario: Execution context changes
- **WHEN** the platform, toolchain, workflow contract, or required feature set differs
- **THEN** proof from the prior context is not reused

#### Scenario: A benchmark measurement owner changes
- **WHEN** a required harness input is missing, added, or has a different content digest
- **THEN** the campaign fails closed and requires a deliberate relock and rerun

### Requirement: Immutable artifacts are content-bound
Reusable release artifacts SHALL be validated by their existing version, content digest, receipt, size, platform, toolchain, workflow, and relevant-input contracts. Commit SHAs SHALL remain descriptive provenance but SHALL NOT be required to equal the current commit.

#### Scenario: Artifact inputs and identity are unchanged
- **WHEN** a successful unexpired artifact has matching relevant inputs and all immutable identity checks pass
- **THEN** the artifact may be promoted from its original provenance commit

#### Scenario: Matching artifact predates the first workflow-run page
- **WHEN** newer successful runs fill one or more API pages before an otherwise valid unexpired artifact
- **THEN** discovery searches every page before reporting that no reusable proof exists

#### Scenario: Artifact content is tampered
- **WHEN** any required digest, receipt, archive inventory, version, or size check fails
- **THEN** promotion fails

### Requirement: IssueOps tracks behavior proof without SHA ceremony
Mapped issues and OpenSpec tasks SHALL describe the owning behavior, test layers, reuse criteria, and invalidation conditions without requiring per-task commit SHA receipts or exact-commit reruns.

#### Scenario: Checklist-only reconciliation is committed
- **WHEN** a task-state-only commit follows passed behavior proof
- **THEN** IssueOps, review, topology, and policy checks rerun while unaffected expensive proof remains valid

#### Scenario: Issue text prohibits exact-head proof with a negative modal
- **WHEN** a mapped issue states that proof must not require, should not require, cannot require, or does not require exact-head identity
- **THEN** IssueOps accepts the prohibition while continuing to reject affirmative exact-head requirements

#### Scenario: Implemented behavior changes
- **WHEN** a task transition includes an owning implementation or test change
- **THEN** the affected behavior proof reruns before the task is completed
