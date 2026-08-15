## ADDED Requirements

### Requirement: Structural worktree discovery is bounded, location-independent, and read-only

ProjectAtlas SHALL classify a supplied directory as a true non-Git root, an exact primary or linked worktree, a Git common manager, or invalid Git evidence by reading bounded structural metadata without starting Git, mutating the filesystem, or assuming a folder/branch naming convention.

#### Scenario: Primary and linked worktrees live anywhere

- **WHEN** one Git repository has a selected primary/control checkout and linked worktrees at arbitrary filesystem paths, including a bare common manager with every checkout stored elsewhere
- **THEN** discovery returns canonical roots, primary/linked structural roles, administrative identities, the common control root, and active states in deterministic order without requiring `.worktrees/` or a branch named `main`

#### Scenario: Missing registration remains status evidence

- **WHEN** Git retains a registered worktree whose reciprocal checkout control path is absent
- **THEN** status reports it as missing and never selects or initializes it as source

#### Scenario: Unsafe control metadata fails closed

- **WHEN** a selected control path is a symlink/junction, oversized, malformed, outside the common directory, missing its required target, or disagrees with reciprocal evidence
- **THEN** ProjectAtlas returns typed invalid Git evidence without following it, guessing a source root, or mutating Git

#### Scenario: Registration inventory exceeds the bound

- **WHEN** structural registrations exceed the fixed discovery ceiling
- **THEN** discovery fails with a bounded resource error rather than returning a partial authoritative inventory

#### Scenario: Git executable is unavailable

- **WHEN** Git is absent from `PATH`
- **THEN** structural discovery, registration status, and exact-root ProjectAtlas operation remain available because no Git process is required

### Requirement: The control atlas owns a durable short-alias registry

ProjectAtlas SHALL let one explicitly selected primary/control atlas register structurally discovered worktrees by short alias while Git remains the sole lifecycle authority.

#### Scenario: Main is an authority alias rather than a path assumption

- **WHEN** the selected control checkout is an ordinary checkout or a linked checkout anywhere on disk
- **THEN** ProjectAtlas exposes it as reserved alias `main` without inferring its role from its directory name or current Git branch name

#### Scenario: List joins structural and ProjectAtlas state

- **WHEN** an agent calls `atlas_worktree_list` from the control MCP session
- **THEN** the bounded response labels every structural candidate with a stable selector, exact current path, Git state, registered alias when present, atlas initialization state, telemetry synchronization state, and typed blockers without writing either Git or ProjectAtlas state

#### Scenario: Add registers one unambiguous discovered worktree

- **WHEN** an agent calls `atlas_worktree_add` with one stable or uniquely matching short selector and an optional valid alias
- **THEN** ProjectAtlas validates reciprocal same-repository evidence and durably registers the worktree without requiring the agent to repeat a full absolute path or creating/moving any Git worktree

#### Scenario: Ambiguous selector does not guess

- **WHEN** a human-friendly selector matches multiple structural candidates
- **THEN** add returns a typed bounded candidate list and creates no registration

#### Scenario: Alias and identity conflicts fail atomically

- **WHEN** an alias is invalid/reserved/in use or the Git administrative identity is already registered
- **THEN** ProjectAtlas returns a typed conflict and leaves the catalog unchanged

#### Scenario: Git-authorized move preserves registration

- **WHEN** Git moves a registered worktree while retaining its validated administrative identity
- **THEN** the next resolution updates the cached canonical path and preserves the alias, database identity, and token origin

#### Scenario: Reused administrative path does not inherit an alias

- **WHEN** Git removes a registered worktree and later creates a different administrative-directory lifecycle at the same path
- **THEN** ProjectAtlas rejects alias routing without reading or writing the replacement atlas and permits the stale ProjectAtlas registration to be retired before the replacement is registered explicitly

#### Scenario: Remove unregisters ProjectAtlas only

- **WHEN** final local telemetry synchronization succeeds and an agent calls `atlas_worktree_remove` with an active alias
- **THEN** ProjectAtlas retires the selectable mapping while preserving historical aggregate telemetry and leaving the Git worktree, files, branch, `.projectatlas`, and database untouched

#### Scenario: Remove fails before retirement when final sync fails

- **WHEN** ProjectAtlas cannot establish a last-valid aggregate snapshot for an otherwise available registered worktree
- **THEN** remove returns a typed synchronization failure and keeps the active registration unchanged

### Requirement: Every root-scoped MCP operation accepts a concurrency-safe short selector

ProjectAtlas SHALL route root-scoped MCP tools through one captured target resolver that accepts a registered `worktree` alias and preserves legacy exact `project_path` compatibility.

#### Scenario: Alias routes normal agent operations

- **WHEN** an agent calls init, scan, watch-once, startup brief, navigation, source slice, graph, purpose, token, settings, health, lint, root, ignore, map, parity, or another root-scoped tool with `worktree: "issue-430"`
- **THEN** the tool operates on only the exact registered worktree root/database and labels the selected alias and identity in applicable diagnostics

#### Scenario: Init can target an absent worktree atlas

- **WHEN** a structurally valid registered worktree has no `.projectatlas/projectatlas.db` and the agent calls `atlas_init({ worktree: "issue-430" })`
- **THEN** ProjectAtlas initializes that exact worktree without changing the process directory, switching Git state, or requiring `project_path`

#### Scenario: Missing atlas returns alias-preserving guidance

- **WHEN** a non-init tool targets a registered worktree without a database
- **THEN** it returns typed `init_required` guidance whose next call uses the same short alias

#### Scenario: Alias and exact path are mutually exclusive

- **WHEN** one request supplies both `worktree` and `project_path`
- **THEN** ProjectAtlas rejects the request before opening a database or source root

#### Scenario: Interleaved sibling requests do not bleed

- **WHEN** one long-lived MCP process receives concurrent or interleaved calls for `main` and multiple registered aliases
- **THEN** every admitted request and background task keeps its captured root, database, project identity, generation, control authority, and alias regardless of later requests or catalog changes

#### Scenario: Legacy exact path remains compatible

- **WHEN** an existing client supplies only `project_path`
- **THEN** current exact-root behavior remains available as a low-level compatibility and diagnostic escape hatch

### Requirement: Worktree initialization safely hydrates from a valid primary atlas

ProjectAtlas SHALL prefer a safe local hydration from the selected control atlas for a new registered worktree and SHALL publish only a worktree-bound database reconciled to the target's exact source state.

#### Scenario: Compatible complete main atlas hydrates a new worktree

- **WHEN** `atlas_init(worktree=...)` targets an absent atlas and `main` has a compatible, healthy, complete same-repository atlas
- **THEN** ProjectAtlas creates a consistent SQLite-backup candidate, assigns a new target identity/root, preserves allowlisted source/summary/graph data and purpose records, clears telemetry and transient/private runtime state, reconciles the target branch/dirty delta, validates the candidate, and atomically activates it

#### Scenario: Hydration never raw-copies live SQLite files

- **WHEN** main uses WAL or has concurrent readers/writers
- **THEN** hydration uses SQLite's supported backup/snapshot boundary and never copies live `.db`, `-wal`, or `-shm` files directly

#### Scenario: Target branch differs from main

- **WHEN** the registered worktree contains added, changed, deleted, renamed, or dirty files relative to the captured main atlas
- **THEN** incremental reconciliation publishes only the target's exact current source, summaries, classifications, relationships, and generation without changing main

#### Scenario: Applicable approved purposes survive hydration

- **WHEN** main contains approved purpose records for paths present in or temporarily absent from the target branch
- **THEN** the new worktree owns independent copies, present paths remain approved where content freshness permits, absent paths follow existing dormant-purpose behavior, and later worktree edits never promote back to main automatically

#### Scenario: Telemetry is not cloned

- **WHEN** main has existing usage and token-savings history
- **THEN** hydration does not copy that history into the worktree database and aggregate reporting does not double count it

#### Scenario: No safe hydration source falls back visibly

- **WHEN** main is absent, incomplete, incompatible, corrupt, unrelated, unsupported by the filesystem, or otherwise unsafe as a source
- **THEN** init runs the existing full initialization path and returns a typed hydration status and reason instead of resetting/downgrading main or silently claiming hydration

#### Scenario: Existing valid worktree database is preserved

- **WHEN** init targets a worktree that already has a valid compatible atlas
- **THEN** idempotent initialization preserves that database and does not overwrite it with main

#### Scenario: Failure leaves last-valid state

- **WHEN** backup, disk, cancellation, reconciliation, integrity, identity, publication, or atomic activation fails
- **THEN** ProjectAtlas removes only the unpublished candidate, preserves an existing target database or the clean uninitialized state, and returns typed recovery guidance

### Requirement: Graph and purpose writes remain exact while federation is explicit

ProjectAtlas SHALL keep every writable graph and purpose operation bound to one exact atlas and SHALL use registered aliases only to open bounded read-only federation.

#### Scenario: Worktree-local writes do not alter siblings

- **WHEN** init, scan, watch, purpose, health, task, or another mutation runs for one alias
- **THEN** main and every sibling atlas retain their own identities, publications, purposes, graphs, tasks, telemetry-local state, and source bytes

#### Scenario: Main graph remains the complete current main graph

- **WHEN** changes are merged into main and its normal scan/watch path completes
- **THEN** `worktree: "main"` returns the complete graph of the current main checkout without importing contradictory unmerged branch state

#### Scenario: Federated aliases remain labelled

- **WHEN** a supported graph operation receives `worktrees: ["main", "issue-430"]`
- **THEN** ProjectAtlas resolves them to exact read-only participants and labels every result, coverage record, blocker, and continuation with the owning alias/root/generation

#### Scenario: Federation never persists a merged graph

- **WHEN** registered worktrees contain contradictory versions of the same path or symbol
- **THEN** results preserve participant identities and no combined graph rows are written into main or either worktree

### Requirement: The released v0.4.5 skill and public documentation teach the complete workflow

ProjectAtlas SHALL ship version-matched, detailed agent and human guidance for the complete v0.4.5-rc1 worktree contract and SHALL keep public GitHub/Pages architecture links browser-native and correct.

#### Scenario: Shipped skill covers the complete agent workflow

- **WHEN** an agent loads the ProjectAtlas skill shipped with v0.4.5-rc1
- **THEN** it documents list/add/remove, `main`, location-independent discovery, targeted init and hydration/fallback, alias use across normal tools, explicit graph federation, aggregate token reporting, unregister retention, exact isolation, legacy path compatibility, typed failures, recovery, and concise valid MCP examples

#### Scenario: Runtime and skill do not drift

- **WHEN** packaging/plugin/runtime contract checks inspect v0.4.5-rc1
- **THEN** the documented tool names and parameter fields match the compiled MCP surface and obsolete path-only guidance is rejected

#### Scenario: Public GitHub and Pages guidance is complete

- **WHEN** a human reads the repository README/agent integration where applicable, lifecycle guide, architecture guide, GitHub Pages documentation, release notes, or mapped issue architecture links
- **THEN** the public material consistently explains the new workflow, boundaries, examples, current TUI scope, and #456 non-goal without implying Git management or a shared writable graph

#### Scenario: Architecture links render the requested diagrams

- **WHEN** a user follows an issue's Architecture Diagrams link in GitHub
- **THEN** the browser lands on the exact Markdown heading containing a directly rendered non-empty Mermaid diagram through the durable `main/docs` link contract

### Requirement: One holistic E2E proves the released agent workflow

ProjectAtlas SHALL own one named holistic E2E that exercises the complete worktree workflow through real CLI/MCP/database boundaries and is scheduled on all supported hosted RC lanes.

#### Scenario: Holistic workflow succeeds across arbitrary locations

- **WHEN** the E2E creates one selected main checkout and at least two linked worktrees in unrelated temporary paths
- **THEN** it proves structural list/add, short aliases, targeted init, safe main hydration, released-schema migration, branch/dirty reconciliation, scan/watch, purpose preservation/isolation, exact and federated graph reads, combined token totals, interleaved one-process MCP routing, final-sync unregister, retained retired totals, and no Git/source/write bleed

#### Scenario: Holistic workflow exercises negative and recovery paths

- **WHEN** the E2E supplies ambiguous aliases, missing/invalid Git evidence, incompatible or incomplete main atlas state, concurrent stale telemetry sync, cancellation/failure before activation, and a legacy exact `project_path` call
- **THEN** every path returns the specified typed behavior, preserves last-valid databases/totals, and leaves Git lifecycle state untouched

#### Scenario: RC verification owns platform proof

- **WHEN** v0.4.5-rc1 is prepared
- **THEN** the holistic E2E runs in the supported Ubuntu, Windows, macOS Intel, and macOS ARM hosted lanes and release verification retains exact-head/runtime/skill/package evidence
