## Purpose

Make large-project refreshes update the actual changes and affected index data quickly, while preserving trustworthy source evidence, safe publication and caller control over resource use.

## ADDED Requirements

### Requirement: Refresh applies the actual change set
`watch --once` and `atlas_watch_once` SHALL update added, modified, deleted and renamed sources and the derived data they affect. With trustworthy change evidence, they SHALL avoid unnecessary whole-project reads, parsing and publication. Change sets SHALL not be restricted to a particular file count.

#### Scenario: Mixed change set
- **WHEN** a project contains any supported mixture of additions, edits, deletions and renames
- **THEN** the refresh publishes their correct affected state without reprocessing unaffected data unnecessarily

#### Scenario: Dependency or policy change
- **WHEN** a change invalidates dependent data or a broader compiler, ignore, parser or publication policy
- **THEN** refresh includes that invalidated scope while preserving unrelated authored state

### Requirement: Verified unchanged refresh is a cheap no-op
When root, policy, publication identity and reliable change-history continuity prove no relevant change, refresh SHALL avoid project-tree traversal, content hashing, full-node decoding and parsing, and SHALL create no new index generation.

#### Scenario: Continuously observed unchanged MCP project
- **WHEN** a long-lived MCP process has valid trusted observation and matching publication state for an unchanged project
- **THEN** MCP completes through the verified no-op path with the required zero work counts and unchanged generation

#### Scenario: CLI has reviewed retained change evidence
- **WHEN** a CLI fast path uses trustworthy retained evidence supplied by an implemented and reviewed cross-invocation design
- **THEN** it proves the same zero-work bounds without treating a fresh empty event queue as history

### Requirement: Standalone CLI has its own performance gate
Before production changes, the plan SHALL fix and evaluate a cold-CLI latency target. If exact cold discovery cannot meet it, a trustworthy cross-invocation evidence design SHALL be published and reviewed before that implementation proceeds. A fast MCP path alone SHALL not satisfy this capability.

#### Scenario: Exact cold discovery meets the target
- **WHEN** the measured exact-discovery approach meets the preregistered cold-CLI target on representative large projects
- **THEN** the CLI may retain that approach while reducing unnecessary comparison and affected-update work

#### Scenario: Exact cold discovery misses the target
- **WHEN** the measured exact-discovery approach cannot meet the preregistered cold-CLI target
- **THEN** implementation of the proposed fast path waits for a reviewed reliable-history design, including continuity loss, fallback and any required process lifetime or deployment behavior

### Requirement: Missing history triggers trustworthy reconciliation
Refresh SHALL detect unavailable, lost or invalid change evidence and perform the exact reconciliation needed for correctness. A new process SHALL not treat an empty newly created event queue as evidence about past edits. Equal file size and timestamps alone SHALL not prove unchanged source.

#### Scenario: Restart or missed events
- **WHEN** observation restarts, loses events, overflows or cannot cover offline changes
- **THEN** refresh reconciles the uncertain scope exactly before returning a current result

#### Scenario: Content changes without a size or timestamp change
- **WHEN** source content changes while size and modification time remain equal
- **THEN** refresh detects the edit or refuses uncertain freshness rather than silently returning stale data

### Requirement: Source changes and failures preserve valid publication
Refresh SHALL preserve native project identity, authored purposes and atomic publication under source races, concurrent operations, cancellation, deadlines and injected failure. It SHALL expose no partial new index and SHALL support safe retry.

#### Scenario: Change during refresh
- **WHEN** source changes after the initial evidence is captured
- **THEN** refresh revalidates or retries the affected work, or refuses publication without exposing stale or partial new state

#### Scenario: Cancellation or failed write
- **WHEN** refresh is cancelled, reaches its deadline or fails while staging or publishing
- **THEN** the prior valid index, native identity and approved purposes remain intact and an adequate retry can succeed

### Requirement: Refresh remains root safe without implicit initialization
CLI and MCP SHALL use the explicitly selected project/worktree and its existing private database. Wrong-root, incompatible or missing state SHALL produce typed recovery guidance without substituting another database or implicitly creating/resetting project state.

#### Scenario: Wrong root
- **WHEN** the selected database identity does not match the addressed root or worktree
- **THEN** refresh refuses before an update and leaves both projects unchanged

#### Scenario: Missing index
- **WHEN** project-local initialized state is absent or a required publication is missing
- **THEN** refresh returns the applicable typed initialization or refresh guidance without silently replacing state

#### Scenario: Plain directory
- **WHEN** a valid selected project has no Git repository or Git is unavailable
- **THEN** refresh retains its ordinary filesystem and index behavior without making Git a prerequisite

### Requirement: Default work budget fits planned work
Without an explicit caller timeout, refresh SHALL select a documented bounded cooperative budget from estimated required work. Explicit limits SHALL take precedence. Cancellation and existing resource bounds SHALL remain effective, and retries SHALL not reset the operation clock.

#### Scenario: Different work estimates
- **WHEN** a verified no-op, a delta refresh and a required full reconciliation are admitted
- **THEN** their default budgets reflect their different discovery, source-byte and affected-data work within the documented bounds

#### Scenario: Explicit or unrepresentable limit
- **WHEN** the caller supplies a limit, including zero or an unrepresentable deadline
- **THEN** refresh honors the explicit policy, refuses zero immediately and fails closed on an unrepresentable deadline without corrupting the index

### Requirement: Large-project performance is proven
Acceptance SHALL use reproducible optimized installed CLI/MCP measurements and causal work-count checks on supported platforms. Baselines and numeric latency/resource targets SHALL be fixed before implementation. Longer timeouts alone SHALL not satisfy acceptance.

#### Scenario: Representative acceptance
- **WHEN** the completed change is evaluated on unchanged, sparse/dense changed and full-reconciliation large-project fixtures
- **THEN** it meets the preregistered speed/resource targets, required work-count bounds and correctness checks, with cold and warm results distinguished

#### Scenario: Unaffected behavior
- **WHEN** full indexing, normal navigation and supported installation/database upgrades are exercised
- **THEN** no material regression beyond the preregistered limits is accepted
