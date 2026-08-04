## ADDED Requirements

### Requirement: Stable repository and worktree identity
ProjectAtlas SHALL distinguish a logical repository from its checked-out worktrees using durable stored identities and SHALL treat filesystem paths, branches, and heads as mutable observed evidence rather than the identity itself.

#### Scenario: Primary and linked worktrees share repository identity
- **WHEN** a primary checkout and two registered linked worktrees belong to the same Git common repository
- **THEN** ProjectAtlas reports one repository identity and three distinct worktree identities without selecting a sibling as source

#### Scenario: Unrelated clone remains isolated
- **WHEN** an unrelated clone has matching remotes, paths, branches, or source bytes but a different local repository authority
- **THEN** ProjectAtlas assigns a distinct local repository/continuity identity and does not share private continuity state even if both clones independently verify and hydrate from the same portable team seed

#### Scenario: Worktree root relocates
- **WHEN** a registered worktree is moved and its continuity authority remains valid
- **THEN** reciprocal Git administrative identity or the validated non-Git registration nonce still matches, ProjectAtlas updates the locator without rotating worktree identity, and history is preserved

#### Scenario: Copied registration collides
- **WHEN** copied `.projectatlas` state, reused paths, or conflicting control directories claim an existing worktree nonce from more than one root
- **THEN** ProjectAtlas refuses automatic rebinding and returns typed collision or explicit reidentity guidance without merging history

#### Scenario: Deleted worktree is recreated
- **WHEN** a removed Git worktree is later recreated at the same path or branch without the reciprocal registered administrative identity and nonce
- **THEN** ProjectAtlas assigns a new worktree identity and retains the retired identity as history

### Requirement: Exact-root derived atlas isolation
ProjectAtlas SHALL retain worktree-local ownership of source nodes, summaries, symbols, relations, graph generations, freshness, watcher state, configuration, tasks, and mutable derived publication, and every source or graph response SHALL identify exactly one selected worktree root and generation.

#### Scenario: Branch-only source never enters sibling graph
- **WHEN** one linked worktree contains a branch-only file and resolved relationship
- **THEN** only that worktree's atlas contains the file, endpoints, relationship, and derived generation

#### Scenario: Bare manager is continuity-only
- **WHEN** a command addresses a bare/common Git manager that hosts repository continuity state
- **THEN** ProjectAtlas returns typed `worktree_required` for source operations and does not open the continuity database as a source atlas

#### Scenario: Wrong-root database is explicit failure
- **WHEN** a worktree is paired with another worktree's atlas database or an ambiguous continuity authority
- **THEN** ProjectAtlas fails typed without mutation, fallback, reset, migration, or sibling selection

#### Scenario: Simultaneous sibling operations remain isolated
- **WHEN** agents concurrently scan, watch, query, start tasks, record telemetry, or navigate graphs in sibling worktrees
- **THEN** each request reads and writes only its captured exact-root active database and continuity dimensions, and no default, generation, graph, task, telemetry attribution, or write bleeds between requests

### Requirement: One control plane routes all registered worktrees
ProjectAtlas SHALL discover registered worktrees from structural Git metadata and the continuity registry independently of directory names, and SHALL let one long-lived MCP server concurrently route them without manual server or command switching, using an exact project/worktree selection captured for each request.

#### Scenario: Root selection establishes the repository control plane
- **WHEN** a user or agent sets a bare/common manager, primary checkout, or linked checkout as the root
- **THEN** ProjectAtlas resolves and stores one repository control root, discovers its registered worktrees, and when the input was an exact checkout preserves compatibility by selecting that worktree

#### Scenario: Discovery does not depend on folder naming
- **WHEN** registered worktrees are nested under conventional or arbitrary directories, or live outside the manager directory
- **THEN** ProjectAtlas enumerates them from reciprocal Git/common-directory evidence and its validated continuity registry rather than path-name patterns

#### Scenario: Additional worktree paths are configured explicitly
- **WHEN** a legitimate same-repository worktree is not discoverable from the selected control root or Git evidence is unavailable
- **THEN** the user or agent may register its exact path, ProjectAtlas admits it only after reciprocal repository/worktree/root validation, and removing the registration does not delete source or mutate Git

#### Scenario: Descendant folders are not guessed to be worktrees
- **WHEN** the control root contains arbitrary source, build, cache, vendor, temporary, or unrelated project directories
- **THEN** ProjectAtlas indexes them only within the selected exact worktree's normal policy or ignores them accordingly, and never registers them as worktrees merely because they are descendants

#### Scenario: Agent auto-binds on worktree entry
- **WHEN** an agent starts from a nested cwd inside a registered worktree or loads its generated local config
- **THEN** MCP and CLI resolve that containing exact root, capture the selection for the request, and return the root, worktree identity, selection provenance, and generation in every source/graph result

#### Scenario: Explicit per-call selection wins safely
- **WHEN** one MCP connection interleaves calls with explicit project/worktree selections for sibling roots
- **THEN** each call uses its own validated selection and a mutable process or prior-call default cannot redirect another concurrent request

#### Scenario: Manager-root caller selects deliberately
- **WHEN** CLI, MCP, or TUI starts at a bare/common manager with more than one worktree
- **THEN** repository-level status is available, source/graph commands require an explicit or persisted valid worktree selection, and ProjectAtlas never guesses a sibling from recency, branch name, or database presence

#### Scenario: One unambiguous worktree needs no ceremony
- **WHEN** a manager or control plane discovers exactly one valid active worktree and the caller provides no conflicting path or selection
- **THEN** ProjectAtlas selects that worktree automatically and reports the discovery provenance with its exact root and generation

#### Scenario: CLI and MCP expose the same routing contract
- **WHEN** an advertised worktree, source, graph, purpose, task, lifecycle, seed, or telemetry operation is available through both adapters
- **THEN** CLI and MCP return equivalent exact-root selection, generation, completeness, bounds, failure classification, and next-action semantics

### Requirement: Manager TUI is a complete repository/worktree overview
ProjectAtlas SHALL let the root token TUI aggregate repository lifetime and per-worktree state while scoping every source/graph map to one visibly labeled selected worktree.

#### Scenario: Manager overview renders multiple worktrees
- **WHEN** the TUI opens at a repository manager
- **THEN** it shows repository identity and lifetime totals, seed state, registered active/retired worktrees, branch/head/dirty evidence, atlas/runtime/schema/freshness, purposes, telemetry contribution, processes, blockers, and bounded completeness for each worktree

#### Scenario: Map selection changes
- **WHEN** a user explicitly selects another worktree in the manager TUI
- **THEN** only the map/navigation pane changes to that labeled exact root and generation while repository aggregates remain repository-scoped and no sibling graph is blended into the map

#### Scenario: Ordinary root retains zero-ceremony TUI
- **WHEN** the TUI opens inside a normal single-checkout Git root
- **THEN** that checkout is selected automatically and existing local token and map behavior remains available without manager setup or seed access

### Requirement: Bounded worktree lifecycle status
ProjectAtlas SHALL expose a bounded typed CLI/MCP status for known worktrees, continuity, derived atlas state, runtime/schema/freshness, available Git evidence, blockers, completeness, and safe next actions.

#### Scenario: Git evidence is available
- **WHEN** native Git status and registration evidence can be obtained within configured limits
- **THEN** the report includes exact root, branch, head, dirty/merge evidence, atlas and continuity state, typed blockers, and a deterministic next action

#### Scenario: Git executable is unavailable
- **WHEN** the Git executable cannot start but structural root and local databases are valid
- **THEN** ordinary ProjectAtlas operations remain available and lifecycle status marks only Git-dependent fields and cleanup proof unavailable

#### Scenario: Status has many worktrees
- **WHEN** registered worktrees exceed the response bound
- **THEN** the report preserves exact returned counts, total/completeness state, continuation, deadline/cancellation, and bounded output

#### Scenario: Branch topology changes in one checkout
- **WHEN** a registered checkout switches branches, enters detached HEAD, renames or deletes a branch, rebases, retargets, or becomes older than/diverged from the current main seed
- **THEN** the worktree identity remains stable, branch/head evidence updates independently, derived freshness becomes typed refresh-required until complete, and no prior branch graph is reported as the selected generation

#### Scenario: Externally removed worktree leaves the active list
- **WHEN** startup, status, or watcher reconciliation proves that Git or the user removed a registered worktree directory
- **THEN** ProjectAtlas automatically removes it from active navigation and selection, retains a bounded retired identity plus durable purpose/telemetry continuity, and never reassigns its identity to a recreated path

#### Scenario: Pull request merge is advisory evidence only
- **WHEN** a worktree's branch is merged or its remote branch is deleted but the local checkout still exists
- **THEN** ProjectAtlas reports retirement readiness or blockers and never deletes, unregisters, or mutates the local worktree solely from merge evidence

### Requirement: Dry-run-first retirement
ProjectAtlas SHALL plan retirement without mutation, SHALL revalidate exact identity and blockers before apply, SHALL preserve continuity plus a bounded recovery manifest, and SHALL not implicitly mutate Git branches or worktrees.

#### Scenario: Clean merged worktree can be sealed
- **WHEN** an exact clean worktree has no live owned process or unique state and all compatible purposes/telemetry are durably reconciled
- **THEN** ProjectAtlas seals the target registration/contribution epoch, removes it from the active list and default selection, records a verified bounded retirement manifest, does not copy the rebuildable atlas, and returns explicit Git-authority removal guidance

#### Scenario: Missing worktree registration is retired explicitly
- **WHEN** a user or agent applies retirement to an already absent worktree after exact retired/missing identity and continuity revalidation
- **THEN** ProjectAtlas removes the stale active registration idempotently, retains bounded history and lifetime totals, and does not require the deleted source database to exist

#### Scenario: Dirty or unique worktree is blocked
- **WHEN** the target is dirty, unmerged, identity-changed, process-owned, database-uncertain, or continuity-incomplete
- **THEN** retirement apply fails closed and preserves the worktree, branch, databases, WAL files, purposes, and telemetry

#### Scenario: Apply evidence changed after dry run
- **WHEN** path, head, dirty state, process identity, schema, or continuity state changes after the plan
- **THEN** ProjectAtlas rejects the stale plan and requires a fresh dry run

#### Scenario: Process ownership is incomplete
- **WHEN** a lease is stale, a PID is reused, process creation identity or root/database arguments do not match, or platform access prevents identity proof
- **THEN** ProjectAtlas returns typed incomplete evidence, blocks retirement, and does not signal or kill any process

### Requirement: Deterministic lifecycle recovery
ProjectAtlas SHALL preserve last-valid derived and continuity state across interrupted initialization, relocation, migration, retirement, cancellation, process death, corruption, and incompatible schemas.

#### Scenario: Interrupted registration retries
- **WHEN** a process terminates between continuity preparation and worktree registration completion
- **THEN** restart either completes the same idempotent transition or returns typed recovery without duplicate identities

#### Scenario: Newer continuity schema is encountered
- **WHEN** the installed runtime opens a continuity database created by a newer unsupported runtime
- **THEN** it refuses before write and preserves database, WAL, SHM, metadata, and source atlas bytes

#### Scenario: Older writer spans migration cutover
- **WHEN** an owned CLI, MCP, or watcher writer remains live or the source database cannot enforce the new authority epoch against a supported predecessor runtime
- **THEN** ProjectAtlas preserves the source and destination, refuses cutover, and reports the quiescence or compatibility blocker

#### Scenario: Cutover crashes between databases
- **WHEN** migration terminates before destination prepare, after destination prepare but before source fence, after source fence but before registration switch, or after registration switch
- **THEN** recovery follows the recorded saga state so the source is never fenced before a verified destination exists, a pre-fence prepared import is refreshed after possible late writes, and no state exposes two authoritative writers

#### Scenario: Recovery follows a continuity write
- **WHEN** continuity authority accepted a new purpose or telemetry write after cutover and recovery becomes necessary
- **THEN** ProjectAtlas reconciles forward into a new authority epoch and never restores an older snapshot as authority

#### Scenario: Non-Git project uses local lifecycle
- **WHEN** a valid non-Git project is initialized
- **THEN** ProjectAtlas uses a local continuity identity, supports full local source, purpose, telemetry, task, map, graph, CLI, MCP, and TUI behavior, and reports only Git-worktree evidence and management as not applicable

#### Scenario: Git executable disappears from a Git checkout
- **WHEN** a structurally valid Git checkout remains readable but Git cannot be resolved from PATH
- **THEN** full local source, purpose, token, task, map, graph, CLI, MCP, and TUI behavior continues while branch, dirty, merge, and Git-removal evidence is typed unavailable

#### Scenario: Relocated or retired root is encountered
- **WHEN** a registration points at a relocated, removed, recreated, nested-cwd, or partially retired worktree
- **THEN** ProjectAtlas proves reciprocal identity before rebinding, refuses stale or duplicate locators, preserves prior history, and returns deterministic rebind, reidentity, retirement, or local-init guidance without selecting a sibling
