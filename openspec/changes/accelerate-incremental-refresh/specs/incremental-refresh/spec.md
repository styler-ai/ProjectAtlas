## Purpose

Make initial indexing, full reindexing, symbol/graph rebuilding and large-project refreshes fast while preserving complete trustworthy results, safe publication and caller control over resources.

## ADDED Requirements

### Requirement: Performance covers all indexing entry points
The capability SHALL cover CLI/MCP initialization, full scan, explicit symbol/graph rebuild and one-shot refresh, plus continuous watch through shared owners. Each path SHALL meet its own preregistered speed, work-count and resource targets. Improving only watch-once or warm MCP SHALL not complete the capability.

#### Scenario: Initial and full indexing
- **WHEN** a representative large project is initially indexed or fully reindexed
- **THEN** the optimized installed product meets its measured improvement targets with complete current results, supported parser/document coverage and preserved authored state

#### Scenario: Explicit symbol or graph rebuild
- **WHEN** a rebuild is requested through a supported CLI or MCP route
- **THEN** it meets its own target and preserves the existing selection, force/rebuild, provenance and complete-projection contract rather than skipping required work

### Requirement: Algorithms avoid demonstrated unnecessary work
Indexing SHALL reduce measured redundant traversal, reads/hashing, parsing, decoding, comparison and database/publication work in the existing shared owners. Worker, memory, file and output bounds SHALL remain effective; retaining complete and race-safe source evidence SHALL take precedence over a fast incomplete result.

#### Scenario: Full-work benchmark
- **WHEN** an initial/full/rebuild performance case is measured
- **THEN** work counts and expected results prove the required indexing actually occurred, and a cached no-op cannot be reported as full-rebuild throughput

#### Scenario: Wider invalidation
- **WHEN** correctness requires wider source or dependent-data processing
- **THEN** the operation includes that work under its measured full/reconciliation case without an unsupported constant-time claim

### Requirement: Refresh applies the actual change set
`watch --once` and `atlas_watch_once` SHALL update added, modified, deleted and renamed sources and the derived data they affect. With trustworthy change evidence, they SHALL avoid unnecessary whole-project reads, parsing and publication. Change sets SHALL not be restricted to a particular file count.

#### Scenario: Mixed change set
- **WHEN** a project contains any supported mixture of additions, edits, deletions and renames
- **THEN** the refresh publishes their correct affected state without reprocessing unaffected data unnecessarily

#### Scenario: Dependency or policy change
- **WHEN** a change invalidates dependent data or a broader compiler, ignore, parser or publication policy
- **THEN** refresh includes that invalidated scope while preserving unrelated authored state

### Requirement: Unchanged refresh reconciles exactly without derived work
For 0.5.1, CLI and MCP one-shot refresh SHALL perform exact source discovery and lightweight ordered source comparison before declaring current state. They SHALL avoid full-node decoding, parsing and publication for unchanged current-contract sources. Native callback queues, equal metadata and retained read epochs alone SHALL NOT authorize bypassing this reconciliation. Zero-source-work admission is deferred to unscheduled #725.

#### Scenario: Unchanged current-contract project
- **WHEN** CLI or long-lived MCP refreshes an unchanged project with matching publication policy
- **THEN** it performs one detection discovery/comparison, no parse or full-node decode and no new publication
- **AND** it meets the registered unchanged latency/resource targets

#### Scenario: Delayed observation or same-metadata edit
- **WHEN** a saved edit has not reached a watcher callback, or preserves size and modification time
- **THEN** exact comparison detects it and the existing affected-work owner repairs it or refuses unsafe publication

#### Scenario: Policy restored after selecting a different source set
- **WHEN** Git configuration, global/repository excludes, root/nested `.gitignore`/`.ignore` inputs, `.git`/`commondir` selectors or registered-worktree inventory temporarily change during discovery and are restored before admission finishes, with watcher delivery delayed
- **THEN** admission binds its source assessment to the policy inputs actually consumed and reconciles or returns typed refresh guidance instead of declaring a stale publication current
- **AND** this includes restoration of the original policy file object and policy files excluded by their own rules, while unchanged policy and supported local exceptions retain their normal behavior

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

#### Scenario: Continuous watch crosses a source-root replacement gap
- **WHEN** the selected source directory briefly disappears during replacement while continuous native or polling watch retains its external database and configuration
- **THEN** initial and later reconciliation retry only the selected root's typed absence, at most three attempts with the existing watch interval and original cancellation/deadline control
- **AND** native watch retains uncertainty, rebinds recursive coverage and performs full reconciliation before counting a successful cycle, preserving project identity, authored purposes and the last complete publication during absence
- **AND** persistent absence terminates with the existing typed failure; unrelated configuration/database I/O, permission errors, project mismatches and explicit one-shot/MCP calls retain their existing failure behavior

#### Scenario: Source indirection substituted after admission
- **WHEN** a direct source leaf or parent becomes a symlink after metadata inspection, whether its target is inside or outside the selected root
- **THEN** discovery refuses before hashing redirected source bytes and preserves the prior valid publication
- **AND** direct source reads, hardlinks, selected-root aliases, ordinary file replacement and supported native reparse compatibility remain functional

#### Scenario: Targeted source alias within the selected root
- **WHEN** a targeted refresh receives a symlink leaf or ancestor whose target is otherwise an admitted source inside the selected root
- **THEN** it excludes the source alias without canonicalizing it into an admitted direct path

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

#### Scenario: Dependency closure requires an explicit scan
- **WHEN** a read refuses because the dependency-aware incremental closure exceeded its safe limit
- **THEN** CLI JSON/TOON and MCP typed recovery select `scan` / `atlas_scan`, consistently with the human diagnostic, and preserve the exact project or registered worktree selector
- **AND** source, path and policy changes that the ordinary one-shot refresh can reconcile continue to select `watch --once` / `atlas_watch_once`, including a full reconciliation scope

### Requirement: Default work budget fits planned work
Without an explicit caller timeout, indexing/rebuilding/refresh SHALL select a documented bounded cooperative budget from reliable work estimates, including required discovery. Explicit limits SHALL take precedence. Cancellation and resource bounds SHALL remain effective; estimating work SHALL not add an unbudgeted full walk, and retries SHALL not reset the clock.

#### Scenario: Different work estimates
- **WHEN** initial/full/rebuild work, a verified no-op, a delta refresh or required reconciliation is admitted
- **THEN** their default budgets reflect their different discovery, source-byte and affected-data work within the documented bounds

#### Scenario: Explicit or unrepresentable limit
- **WHEN** the caller supplies a limit, including zero or an unrepresentable deadline
- **THEN** refresh honors the explicit policy, refuses zero immediately and fails closed on an unrepresentable deadline without corrupting the index

### Requirement: Adaptive admission preserves the established ceiling
0.5.1 SHALL replace only the omitted-limit default with work-based selection. The 1,800-second ceiling and larger-request clamp SHALL remain. With no reliable work estimate, automatic admission SHALL use 1,800 seconds without a preflight walk. Explicit limits up to the ceiling, zero-budget refusal and unrepresentable-deadline refusal SHALL retain their existing behavior.

#### Scenario: Unknown cold initial workload
- **WHEN** a cold initial index has no reliable prior file, byte or affected-work estimate and the caller omits a limit
- **THEN** admission uses the existing 1,800-second budget and includes discovery within that single operation clock

#### Scenario: Explicit and larger limits
- **WHEN** the caller explicitly supplies a limit whose deadline is representable
- **THEN** limits up to 1,800 seconds are used instead of the estimator, including an explicit 1,800; larger requests are clamped to 1,800 seconds

#### Scenario: Published automatic-default policy
- **WHEN** the adaptive default is implemented
- **THEN** affected help, schemas and skill/user guidance describe the changed omitted-limit behavior, unchanged ceiling/clamp, unknown-work fallback and preserved explicit limits

### Requirement: Large-project performance is proven
Acceptance SHALL use reproducible optimized installed CLI/MCP measurements and causal work-count checks on supported platforms. Baselines and numeric latency/resource targets SHALL be fixed before implementation. Longer timeouts alone SHALL not satisfy acceptance.

#### Scenario: Representative acceptance
- **WHEN** the completed change is evaluated on initial/full/rebuild, unchanged, sparse/dense changed and reconciliation large-project fixtures
- **THEN** every path meets its own preregistered speed/throughput/resource and work-count targets with complete correct results, cold/warm starts and caches distinguished, and normal cases finishing within their selected budgets

#### Scenario: Unaffected behavior
- **WHEN** normal navigation and supported installation/database upgrades are exercised
- **THEN** no material regression beyond the preregistered limits is accepted
