## ADDED Requirements

### Requirement: The installed stable v0.5 surface is frozen
ProjectAtlas SHALL record one installed stable v0.5 baseline containing the complete CLI inventory, MCP tool names and concrete schemas, serialized discovery bytes, runtime/plugin/skill identity, generated host configuration, and representative workflow results.

#### Scenario: Baseline capture
- **WHEN** the v0.6 evaluation begins
- **THEN** every compared route uses the same released v0.5 product, repository revisions, task prompts, project roots, warm or cold state, timeout, and accounting policy

#### Scenario: Baseline cannot be reproduced
- **WHEN** the installed runtime, plugin, skill, host configuration, or inventory does not match the selected v0.5 release
- **THEN** evaluation stops without classifying the drifted surface

### Requirement: CLI-first, MCP-first, and mixed routes are compared identically
ProjectAtlas SHALL compare the three route policies on the same startup, orientation, search, exact retrieval, graph, health, purpose, token, administration, routing, freshness, and recovery tasks.

#### Scenario: Comparative task result
- **WHEN** a task completes or fails under a route policy
- **THEN** the evaluation records correctness, freshness, elapsed time, subprocess or session startup, calls, serialized discovery, context, wrong reads, backtracking, recovery, and bounded output without discarding failures

#### Scenario: Workload-specific result
- **WHEN** one route wins the accepted task set
- **THEN** ProjectAtlas reports the tested boundary and limitations rather than claiming universal transport superiority

### Requirement: Ordinary one-shot shell work uses the concise CLI route
The version-matched ProjectAtlas skill SHALL prefer `atlas ...` for an ordinary local operation that completes in one shell command after the installed v0.5 contract is accepted.

#### Scenario: Ordinary local navigation
- **WHEN** an agent with shell access needs one scan, overview, folder, file, summary, outline, search, graph, slice, health, lint, token, configuration, or comparable completed operation
- **THEN** the skill teaches the concise CLI route unless a capability-specific MCP requirement applies

#### Scenario: CLI failure or absence
- **WHEN** the verified CLI is unavailable or cannot express the accepted capability safely
- **THEN** the skill reports or selects the supported alternative without inventing a command or silently using a stale runtime

### Requirement: MCP remains first-class where its session capability helps
ProjectAtlas SHALL retain MCP as the preferred route for no-shell hosts, typed tool discovery, persistent multi-client project routing, real session-local task status or cancellation, and other cases whose evaluation proves a material benefit.

#### Scenario: No-shell host
- **WHEN** an agent host cannot invoke a local shell but supports the installed ProjectAtlas MCP server
- **THEN** the version-matched skill exposes the retained typed repository workflow through MCP

#### Scenario: Concurrent repositories
- **WHEN** several clients or repositories share one MCP host
- **THEN** per-call `project_path` or an equivalent explicit isolation boundary routes each request independently

### Requirement: Every public route has one reviewed disposition
Every frozen MCP tool, CLI command or alias, skill instruction, generated-host route, and compatibility expectation SHALL be classified as retained, merged into an existing typed response, CLI-owned, automatic backend behavior, or intentionally removed.

#### Scenario: Inventory reconciliation
- **WHEN** implementation begins
- **THEN** no frozen route, alias, schema, fixture, skill reference, or host expectation remains unclassified

#### Scenario: Unobserved route
- **WHEN** local telemetry does not contain a route
- **THEN** absence alone cannot justify removal without contract, workflow, and compatibility review

### Requirement: Retained public contracts stay concrete and bounded
Retained MCP tools SHALL expose concrete typed input schemas and retained CLI commands SHALL preserve documented arguments, help, formats, stdout, stderr, exit codes, errors, confirmation, and output bounds.

#### Scenario: Typed batch input
- **WHEN** an MCP tool accepts a collection of structured items
- **THEN** `tools/list` exposes the actual item fields and constraints rather than an `unknown` array or unrelated action string

#### Scenario: Administrative CLI route
- **WHEN** a routine MCP tool is reclassified as CLI-owned administration
- **THEN** the CLI retains typed validation, containment, dry-run or confirmation where required, errors, and real compatibility tests

### Requirement: Project-root failures never mutate implicitly
Every retained CLI and MCP path-sensitive route SHALL preserve explicit root isolation and typed wrong-root and missing-index behavior without implicit initialization, scan, database replacement, or selected-root mutation.

#### Scenario: Wrong explicit root
- **WHEN** a request addresses a database through a filesystem-distinct project root
- **THEN** ProjectAtlas returns the typed wrong-root result and changes no project, database, session, watcher, or worktree state

#### Scenario: Missing addressed index
- **WHEN** an explicit project path has no ProjectAtlas database
- **THEN** ProjectAtlas returns typed missing-index guidance and does not initialize or scan implicitly

### Requirement: Task status and cancellation require a real producer
Session-local task status and cancellation SHALL remain public only when an installed normal operation produces a bounded task identifier with observable progress and exact cancellation ownership.

#### Scenario: Real asynchronous task
- **WHEN** a retained operation returns a task identifier
- **THEN** status observes its state and cancellation stops only its owned work while preserving database and process integrity

#### Scenario: No task-producing workflow
- **WHEN** no normal packaged operation can produce such an identifier
- **THEN** task status and cancellation are removed together from the public inventory, skill, fixtures, and compatibility expectations

### Requirement: Breaking surface changes migrate atomically
Any removed tool or incompatible schema SHALL ship only through one versioned runtime, plugin, skill, generated-host, fixture, documentation, migration-note, and installer-validation boundary with a tested rollback.

#### Scenario: Clean v0.6 installation
- **WHEN** a supported host installs the changed release
- **THEN** its runtime, CLI, MCP inventory, plugin skill, and generated configuration agree and no instruction names an unavailable route

#### Scenario: Rollback
- **WHEN** a user returns to the last compatible release
- **THEN** the installer restores the complete compatible runtime/plugin contract without mixing old schemas and new guidance

### Requirement: Route classification does not invent persistence work
CLI/MCP disposition SHALL preserve the existing SQLite schema, authored/derived authority, and freshness model unless an unavoidable accepted compatibility delta is specified and reviewed database-first.

#### Scenario: Adapter-only disposition
- **WHEN** a route is retained, merged, CLI-owned, automatic, or removed without changing stored semantics
- **THEN** no migration, table, index, cache, repository abstraction, or second state authority is added

#### Scenario: Persistence compatibility delta is unavoidable
- **WHEN** an accepted public contract cannot remain compatible on the existing storage boundary
- **THEN** its exact authority/key/query/transaction/migration/rollback/recovery implications are specified and proven before service or adapter work

### Requirement: Installed-product proof covers retained workflows
The accepted v0.6 surface SHALL pass real CLI, stdio MCP, generated-host, concurrent-root, migration, rollback, unknown-tool, freshness, and supported-platform E2E for every retained normal workflow and every moved administrative behavior.

#### Scenario: Retained workflow
- **WHEN** an installed agent completes startup, orientation, bounded search, exact retrieval, graph inspection, health or lint, purpose correction, token reporting, and relevant administration
- **THEN** the selected routes preserve task success, current evidence, exact selectors, typed errors, bounded output, and compatibility on Windows, Linux, macOS x64, and macOS Apple Silicon

### Requirement: #310 is accepted before dependent Memory Atlas and release proof
#310 SHALL remain the sole owner of the route disposition. #314 SHALL consume only the accepted surface, and #493 SHALL provide final feature-free installed release acceptance.

#### Scenario: Dependency transition
- **WHEN** #310 is accepted and merged
- **THEN** #314 refreshes or rebases onto that baseline and #493 remains blocked until both children and their required reviews are complete
