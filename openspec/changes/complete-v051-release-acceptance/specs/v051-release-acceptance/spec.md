## Purpose

Accept the complete large-project indexing performance release through real installed behavior and independently verified publication, while preserving existing compatibility and project state.

## ADDED Requirements

### Requirement: The release has one feature-free acceptance owner
#715 SHALL own `v0.5.1-00` without implementing product changes. Every accepted implementation issue SHALL have #715 as its sole direct parent and directly block release acceptance. The complete #708 scope SHALL be admitted and accepted before candidate publication.

#### Scenario: Performance work enters the release
- **WHEN** #708's complete 0.5.1 specification is synchronized
- **THEN** its milestone, parent, blockers and declared graph agree, and the old 0.6.0 membership is removed

#### Scenario: Initial planning graph
- **WHEN** only the release-owner plan has been admitted
- **THEN** the singleton graph records planning ownership and does not count the performance work as implemented or authorize candidate acceptance

### Requirement: Acceptance binds the exact complete candidate
The release owner SHALL freeze accepted main and exact artifacts only after every child, required review and compatibility disposition is complete. Changed behavior-relevant inputs SHALL invalidate the affected acceptance proof.

#### Scenario: Unfinished performance work
- **WHEN** any required indexing path, performance target, correctness case or review remains incomplete
- **THEN** candidate acceptance and publication remain blocked

### Requirement: Every supported installed route is exercised
The candidate SHALL reconcile and safely execute the complete CLI command/nested-command and MCP tool inventory, including unchanged routes, on Windows x64, Linux x64, macOS Intel and Apple Silicon as supported. Mutations SHALL use isolated project and host state.

#### Scenario: Installed navigation and administration
- **WHEN** native CLI, MCP, plugin hooks and generated host configurations are exercised
- **THEN** versions, root/database bindings, actual behavior, typed refusals, output formats and bounds match their accepted contracts without ambient mutations

#### Scenario: Wrong root or missing index
- **WHEN** a selected root/database mismatches or initialized state is missing
- **THEN** the installed candidate returns typed recovery guidance without substituting or resetting a database

### Requirement: Performance proof covers the complete indexing topic
Acceptance SHALL verify #708's preregistered speed, work-count and resource targets using the exact optimized installed candidate with normal telemetry. Initial indexing, full reindexing, cold CLI refresh, warm MCP, no-op, sparse/dense changes and required reconciliation SHALL have distinct measured cases.

#### Scenario: Complete performance result
- **WHEN** representative large repositories are tested on supported platforms
- **THEN** every required path meets its own target with correct complete results; faster warm MCP or a longer timeout cannot substitute for another path's missing improvement

### Requirement: Upgrades and recovery preserve authored state
The installed candidate SHALL preserve native identity, approved purposes and the last valid publication through supported upgrades, concurrent operations, cancellation, deadlines and injected failures, with safe retry and compatible rollback proof.

#### Scenario: Interrupted refresh or upgrade
- **WHEN** the operation fails or is cancelled at an owning failure boundary
- **THEN** no partial index or wrong-root state is exposed and recovery preserves authored data

### Requirement: Publication and closure follow verified acceptance
An authorized 0.5.1 RC SHALL be a non-draft prerelease with Latest excluded. Later authorized stable promotion SHALL repeat exact hosted and installed proof and independently verify assets, checksums, identities, Latest and downstream pins. #715 SHALL close last after every child and stable readback pass.

#### Scenario: Candidate publication
- **WHEN** complete acceptance and candidate publication authority exist
- **THEN** tag/head, assets and installed behavior are independently read back while the preceding stable remains Latest

#### Scenario: Final stable closeout
- **WHEN** stable promotion and complete readback succeed
- **THEN** 0.5.1 is Latest, current feedback and task mirrors are reconciled, and #715 closes before the milestone is finalized
