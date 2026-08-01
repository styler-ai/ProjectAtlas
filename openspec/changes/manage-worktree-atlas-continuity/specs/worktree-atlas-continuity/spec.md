## ADDED Requirements

### Requirement: Stable repository and worktree identity
ProjectAtlas SHALL distinguish a logical repository from its checked-out worktrees using durable stored identities and SHALL treat filesystem paths, branches, and heads as mutable observed evidence rather than the identity itself.

#### Scenario: Primary and linked worktrees share repository identity
- **WHEN** a primary checkout and two registered linked worktrees belong to the same Git common repository
- **THEN** ProjectAtlas reports one repository identity and three distinct worktree identities without selecting a sibling as source

#### Scenario: Unrelated clone remains isolated
- **WHEN** an unrelated clone has matching remotes, paths, branches, or source bytes but a different local repository authority
- **THEN** ProjectAtlas assigns a distinct repository identity and does not share continuity state

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
ProjectAtlas SHALL retain worktree-local ownership of source nodes, summaries, symbols, relations, graph generations, freshness, watcher state, configuration, and derived publication.

#### Scenario: Branch-only source never enters sibling graph
- **WHEN** one linked worktree contains a branch-only file and resolved relationship
- **THEN** only that worktree's atlas contains the file, endpoints, relationship, and derived generation

#### Scenario: Bare manager is continuity-only
- **WHEN** a command addresses a bare/common Git manager that hosts repository continuity state
- **THEN** ProjectAtlas returns typed `worktree_required` for source operations and does not open the continuity database as a source atlas

#### Scenario: Wrong-root database is explicit failure
- **WHEN** a worktree is paired with another worktree's atlas database or an ambiguous continuity authority
- **THEN** ProjectAtlas fails typed without mutation, fallback, reset, migration, or sibling selection

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

### Requirement: Dry-run-first retirement
ProjectAtlas SHALL plan retirement without mutation, SHALL revalidate exact identity and blockers before apply, SHALL preserve continuity plus a bounded recovery manifest, and SHALL not implicitly mutate Git branches or worktrees.

#### Scenario: Clean merged worktree can be sealed
- **WHEN** an exact clean worktree has no live owned process or unique state and all compatible purposes/telemetry are durably reconciled
- **THEN** ProjectAtlas seals the target registration/contribution epoch, records a verified bounded retirement manifest, does not copy the rebuildable atlas, and returns explicit Git-authority removal guidance

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
- **THEN** ProjectAtlas uses a local continuity identity, supports local purpose/telemetry behavior, and reports Git-worktree management as not applicable
