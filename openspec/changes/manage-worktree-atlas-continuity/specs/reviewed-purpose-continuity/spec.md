## ADDED Requirements

### Requirement: Repository-authoritative reviewed purposes
ProjectAtlas SHALL store agent-reviewed purpose text and approval metadata once under the logical repository authority while keeping suggestions and source freshness worktree-specific.

#### Scenario: Agent approval is immediately reusable
- **WHEN** an agent approves a purpose through a supported purpose API in one worktree
- **THEN** the repository authority records one approved revision and a compatible sibling worktree can use it without a second approval pass

#### Scenario: Direct SQLite changes remain unsupported
- **WHEN** a caller attempts to bypass the purpose API or pairs an unvalidated continuity database
- **THEN** ProjectAtlas rejects or ignores the untrusted write path and preserves the authoritative revision

### Requirement: File purposes are content-aware
ProjectAtlas SHALL project an approved file purpose as current only when repository identity, normalized path, and approved content identity match the selected worktree.

#### Scenario: Identical file inherits approval
- **WHEN** two worktrees contain the same normalized file path with identical approved source content
- **THEN** both report the same approved agent purpose without recreating it

#### Scenario: Changed file becomes stale
- **WHEN** a sibling branch changes the approved file's source content
- **THEN** ProjectAtlas retains the reviewed text as stale context and requires deliberate review before treating it as current

#### Scenario: Branch-only file does not leak
- **WHEN** an approved path exists only in one branch
- **THEN** sibling worktrees that lack the path do not expose it as an indexed or current purpose row

### Requirement: Folder purposes preserve path responsibility without sibling leakage
ProjectAtlas SHALL reuse an approved folder purpose when the normalized folder path exists in the same logical repository and SHALL keep branch-local existence and repurposing evidence explicit.

#### Scenario: Common folder inherits approval
- **WHEN** the same folder path exists in compatible worktrees
- **THEN** ProjectAtlas reuses its approved responsibility without re-creating the purpose

#### Scenario: Folder path is absent
- **WHEN** a branch deletes or never contains the approved folder path
- **THEN** the selected worktree does not expose a current folder purpose while repository history remains preserved

#### Scenario: Rename is ambiguous
- **WHEN** a file or folder may have been renamed but exact bounded identity evidence is incomplete or conflicting
- **THEN** ProjectAtlas does not transfer approval automatically and returns typed review guidance

### Requirement: Purpose migration is idempotent and non-destructive
ProjectAtlas SHALL import compatible approved purposes from existing worktree databases through consistent read-only snapshots, stable source fingerprints, atomic receipts, and preserved originals.

#### Scenario: First compatible import
- **WHEN** a compatible source database contains approved purposes not present in continuity authority
- **THEN** ProjectAtlas imports validated revisions atomically, reconciles counts, records a unique receipt, and leaves the source unchanged

#### Scenario: Repeated import
- **WHEN** the same source state is imported again after retry or restart
- **THEN** the existing receipt prevents duplicate revisions and returns the prior result

#### Scenario: Conflicting approved text
- **WHEN** two compatible sources contain different approved purposes for the same repository/path/content identity
- **THEN** ProjectAtlas preserves both source databases, refuses silent last-writer selection, and returns typed conflict resolution work

#### Scenario: Active WAL or newer schema
- **WHEN** a source has active WAL state or an unsupported newer schema
- **THEN** ProjectAtlas uses an engine-supported consistent snapshot when compatible or refuses typed before mutation when compatibility cannot be proven

#### Scenario: Legacy approval lacks current content proof
- **WHEN** an approved legacy purpose has a missing node, stale generation, deleted path, absent content hash, changed current bytes, or otherwise cannot bind path/content identity from one consistent snapshot
- **THEN** ProjectAtlas preserves it as historical/unbound review context and never projects it as a current approved purpose without deliberate review

### Requirement: Purpose APIs remain exact-root and concurrency safe
ProjectAtlas SHALL resolve worktree source freshness and repository purpose authority from one captured validated binding for each API request.

#### Scenario: Interleaved MCP worktrees
- **WHEN** one MCP process interleaves purpose reads and reviews for two explicit `project_path` values
- **THEN** every response uses the addressed worktree's freshness with the shared repository revision and never process-global sibling state

#### Scenario: Concurrent approval revision
- **WHEN** two agents conditionally review the same purpose revision concurrently
- **THEN** at most one matching conditional write succeeds and the other receives a typed stale-state conflict

#### Scenario: Missing index does not imply purpose loss
- **WHEN** a new compatible worktree has continuity state but no initialized derived atlas
- **THEN** read-only source navigation returns typed `init_required` while the approved repository purposes remain preserved and unmodified
