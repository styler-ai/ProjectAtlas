## ADDED Requirements

### Requirement: Repository-authoritative reviewed purposes
ProjectAtlas SHALL store agent-reviewed purpose text and approval metadata once under the logical repository authority while keeping suggestions and source freshness worktree-specific.

#### Scenario: Agent approval is immediately reusable
- **WHEN** an agent approves a purpose through a supported purpose API in one worktree
- **THEN** the repository authority records one approved revision and a compatible sibling worktree can use it without a second approval pass

#### Scenario: Direct SQLite changes remain unsupported
- **WHEN** a caller attempts to bypass the purpose API or pairs an unvalidated continuity database
- **THEN** ProjectAtlas rejects or ignores the untrusted write path and preserves the authoritative revision

### Requirement: Accepted file purposes remain path-authoritative
ProjectAtlas SHALL keep an accepted purpose as durable authored responsibility for its logical repository and normalized path; source, summary, symbol, graph, scan, and watcher changes SHALL NOT demote it, while worktree-local path existence and source freshness remain separate facts.

#### Scenario: Identical file inherits approval
- **WHEN** two local worktrees share continuity authority and contain the same normalized accepted file path
- **THEN** both report the same approved agent purpose without recreating it

#### Scenario: Source changes after acceptance
- **WHEN** a sibling branch changes source, summary, symbols, graph facts, or content identity at an accepted path
- **THEN** ProjectAtlas retains the accepted purpose and separately reports derived freshness or an explicit repurposing review request

#### Scenario: Branch-only file does not leak
- **WHEN** an approved path exists only in one branch
- **THEN** sibling worktrees that lack the path keep the purpose dormant and do not expose it as indexed sibling source

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

### Requirement: Pull requests carry deterministic purpose promotions
ProjectAtlas SHALL export reviewed purpose promotions as durable mergeable semantic deltas keyed by portable logical repository identity, typed entity kind, normalized path, exact reviewed content identity, purpose revision, approval, and verifiable provenance rather than by local SQLite row identity.

#### Scenario: Pull request adds a reviewed file
- **WHEN** an agent approves the purpose of a new or changed file in a pull-request worktree
- **THEN** ProjectAtlas emits a deterministically ordered promotion record whose logical identity and payload produce stable conflict behavior in source control without committing the local database

#### Scenario: Different teammates produce compatible promotions
- **WHEN** separate clones or team identities approve the same repository/path/content/purpose under compatible trust policy
- **THEN** deterministic identity deduplicates the promotion while preserving sufficient approval provenance for CI to verify admission

#### Scenario: Promotion provenance is untrusted
- **WHEN** a promotion is malformed, forged, downgraded, from an untrusted actor/workflow, or fails repository policy
- **THEN** main CI rejects or quarantines it typed and does not project it into the main seed as approved

### Requirement: Main imports promotions against final merged source
ProjectAtlas SHALL admit a promotion to the main purpose projection only after validating its portable repository, normalized path, exact reviewed content identity, approval revision/provenance, and policy against final merged main.

#### Scenario: Promotion still matches main
- **WHEN** a merged promotion's path and exact content identity match final main and no incompatible approved revision conflicts
- **THEN** CI imports it once, preserves agent approval/provenance, and the next immutable main seed exposes the accepted purpose

#### Scenario: Merge changes promoted content
- **WHEN** overlap resolution, sequential merges, rebase, retarget, branch-only deletion, or later edits change or remove the promoted path/content before sealing
- **THEN** CI preserves the delta as stale or inconclusive promotion evidence, does not guess or silently apply it, and requests review against final main

#### Scenario: Rename or conflicting purpose is ambiguous
- **WHEN** promotions overlap, a path is renamed, or different approved texts target the same final identity without a deterministic compatible revision relation
- **THEN** CI preserves all provenance, refuses last-writer-wins transfer, and returns typed conflict work without merging branch graphs or databases

#### Scenario: Stacked pull request changes its base
- **WHEN** a stacked pull request is rebased, retargeted, or merged after its base
- **THEN** its promotion is handled by the same final-main identity checks and needs no special stacked-PR database implementation

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
- **THEN** ProjectAtlas preserves authenticated accepted path responsibility when its repository/path authority is proven, otherwise preserves it as historical/unbound context, and never fabricates a portable promotion without deliberate review

### Requirement: Purpose APIs remain exact-root and concurrency safe
ProjectAtlas SHALL resolve worktree source freshness and repository purpose authority from one captured validated binding for each API request.

#### Scenario: Interleaved MCP worktrees
- **WHEN** one MCP process interleaves purpose reads and reviews for two explicit `project_path` values
- **THEN** every response uses a request-captured exact-root binding with the shared local repository revision and never process-global sibling state

#### Scenario: Concurrent approval revision
- **WHEN** two agents conditionally review the same purpose revision concurrently
- **THEN** at most one matching conditional write succeeds and the other receives a typed stale-state conflict

#### Scenario: Missing index does not imply purpose loss
- **WHEN** a new compatible worktree has continuity state but no initialized derived atlas
- **THEN** read-only source navigation returns typed `init_required` while the approved repository purposes remain preserved and unmodified
