## ADDED Requirements

### Requirement: One immutable portable main seed
ProjectAtlas SHALL let CI publish one content-addressed SQLite seed and digest-bound manifest as exact-version GitHub release assets for a clean complete main atlas, SHALL keep that seed physically separate from every ignored writable active database, and SHALL never open the seed writable.

#### Scenario: CI publishes a clean main generation
- **WHEN** a clean main checkout has a complete verified atlas generation and all compatible purpose promotions are resolved
- **THEN** CI seals one portable seed plus a manifest that identifies the portable repository, exact included-source fingerprint or external exact commit artifact, schema, runtime, parser, policy, config, artifact size, digest, and attestation

#### Scenario: Seed and active database are separate
- **WHEN** a worktree hydrates from a verified seed
- **THEN** ProjectAtlas copies or safely reflinks the seed into that worktree's ignored active database, never hardlinks or writes the seed, and all later writes target only the exact-root active copy

#### Scenario: Local-only state is excluded
- **WHEN** CI constructs the portable seed
- **THEN** an explicit portable allowlist excludes or resets absolute roots, local repository/worktree identities, telemetry, sessions, processes, tasks, leases, watcher state, transient generations, WAL/SHM state, host paths, caches, and other host-local or private rows

#### Scenario: Telemetry cannot enter publication
- **WHEN** local repository continuity contains lifetime token telemetry or session detail
- **THEN** sealing neither reads it as seed input nor writes it to the seed, manifest, purpose delta, or Git-hosted artifact

#### Scenario: Local seed and active state remain ignored
- **WHEN** ProjectAtlas downloads, stages, hydrates, or caches a seed in a checkout
- **THEN** root ignore policy excludes active databases and sidecars, continuity, seed caches, staging paths, locks, generated host configs, and private runtime state while a structural scan exclusion prevents them from entering the source fingerprint

### Requirement: SQLite-safe deterministic sealing
ProjectAtlas SHALL seal only from a quiescent complete publication using an engine-supported consistent snapshot, explicit portable normalization, and fail-closed validation before immutable publication.

#### Scenario: Active WAL is checkpointed safely
- **WHEN** the candidate main atlas has WAL state
- **THEN** the sealer proves all owned writers quiescent, performs a bounded checkpoint under the owning connection policy, snapshots through the SQLite backup API or `VACUUM INTO`-style flow, and refuses publication if a writer, busy state, or WAL uncertainty remains

#### Scenario: Portable snapshot passes database checks
- **WHEN** the staged seed has been normalized
- **THEN** CI verifies expected application/schema identity, allowed tables and columns, integrity, foreign keys, row conversions, complete-generation markers, no excluded local state, and a real OS-level and SQLite-query-only read-only CLI/MCP smoke before computing its final digest

#### Scenario: Publication input avoids self-reference
- **WHEN** seed material or its manifest is source-hosted
- **THEN** ProjectAtlas structurally excludes all publication material from indexed input and binds the seed to a deterministic included-source tree fingerprint or an external exact source-commit artifact so neither commit nor digest must contain itself

#### Scenario: RC seed uses its exact release tag
- **WHEN** CI publishes an accepted `vMAJOR.MINOR.PATCH-rcN` release
- **THEN** the release contains the candidate's seed and manifest under deterministic versioned names, their checksums enter the release inventory, hydration addresses that exact tag, and GitHub Latest remains unchanged

#### Scenario: Stable seed uses its exact release tag
- **WHEN** CI publishes final `vMAJOR.MINOR.PATCH`
- **THEN** it regenerates the seed from final merged main, publishes it with that exact stable tag, and the release verifier proves the stable release and seed are current

#### Scenario: Seed payloads stay out of Git history
- **WHEN** release seed assets and a bounded local cache exist
- **THEN** neither payload nor downloaded manifest is committed through normal Git or Git LFS, and source-controlled purpose promotions remain the only portable authored state

### Requirement: Automatic exact-root hydration
ProjectAtlas SHALL automatically discover the nearest compatible seed for a new worktree or teammate clone, verify it before use, activate only an exact-root private copy, and incrementally refresh that copy to the current selected source.

#### Scenario: New worktree receives immediate atlas value
- **WHEN** an agent enters a newly created linked worktree whose nearest seed is compatible
- **THEN** cwd/config discovery selects that exact root, verifies and activates a private seed copy, rebinds local repository/worktree/root identity, refreshes only differences from the seed source fingerprint, and returns the selected exact root and complete generation without manual database or MCP-server switching

#### Scenario: Teammate clone consumes the team baseline
- **WHEN** a separate clone with its own local continuity identity obtains the repository's compatible seed and manifest
- **THEN** it verifies the portable repository/source binding, hydrates a new clone-local active database, preserves clone-local purposes and telemetry authority, and never imports another host's local identity or private state

#### Scenario: Current source is older or diverged
- **WHEN** a branch, detached HEAD, branch switch, rebase, retarget, or older checkout differs from the seed in either direction
- **THEN** incremental refresh adds, changes, and removes exact content as required, reuses only exact-content facts, recomputes the bounded affected dependency closure, and publishes one complete generation for the selected source without retaining seed-main-only graph rows

#### Scenario: Hydration races or crashes
- **WHEN** processes concurrently hydrate the same worktree or one terminates before activation
- **THEN** one bounded owner stages and atomically activates a verified copy, peers receive deterministic wait/retry/current-state results, and restart preserves the previous valid active database or safely resumes/discards the incomplete staging file

### Requirement: Seed use is optional and fails safe
ProjectAtlas SHALL keep ordinary local initialization and navigation fully functional when seed discovery, verification, compatibility, Git, manager state, or network access is unavailable.

#### Scenario: Seed is missing or host is offline
- **WHEN** no local compatible seed exists and a remote artifact cannot be fetched
- **THEN** ProjectAtlas performs the existing local init/scan path, reports typed seed unavailability as optimization state, and retains full source, purpose, token, and graph behavior

#### Scenario: Seed is corrupt or tampered
- **WHEN** the payload is truncated, corrupt, digest-mismatched, attestation-invalid, publication paths leak into indexed input, or excluded private state is present
- **THEN** ProjectAtlas quarantines or ignores the candidate without opening it writable, preserves every valid active database, and falls back locally with typed recovery guidance

#### Scenario: Compatibility contract changed
- **WHEN** seed schema, runtime, parser, policy, config, source binding, or portable repository identity is incompatible
- **THEN** ProjectAtlas neither silently migrates nor partially imports the seed and instead chooses a proven compatible seed or the ordinary clean local build path

#### Scenario: Single checkout has no manager or seed
- **WHEN** an ordinary Git checkout is used without a common-manager control plane or seed publication
- **THEN** existing zero-ceremony CLI, MCP, TUI, init, scan, purpose, token, and graph behavior remains unchanged

#### Scenario: Existing valid atlas wins over seed hydration
- **WHEN** an upgraded checkout already has a compatible exact-root active database
- **THEN** ProjectAtlas preflights and migrates that local authority as required and does not download or activate a seed over it

### Requirement: Main CI rebuilds final relationships instead of merging branch atlases
ProjectAtlas SHALL update the main baseline from final merged source and compatible semantic purpose promotions, reuse only facts keyed by exact content identity, recompute every affected cross-file relation, and seal a new immutable seed only after the final main atlas is complete.

#### Scenario: Sequential pull requests merge
- **WHEN** pull requests with new files, changed purposes, and overlapping dependencies land sequentially
- **THEN** main CI applies only promotions compatible with each final-main path/content identity, incrementally reuses exact-content summaries and symbols, recomputes affected relations against final main, and seals one replacement seed generation

#### Scenario: Branch databases disagree
- **WHEN** contributors produced different local SQLite databases, WAL state, graph generations, row identifiers, or branch-only relations
- **THEN** CI ignores those binary publications as merge input and derives the main generation from final source plus trusted purpose deltas

#### Scenario: Stacked pull request is rebased or retargeted
- **WHEN** a dependent pull request is rebased, retargeted, or merged after its base pull request
- **THEN** the same path/content/provenance validation classifies its promotion against final main without any stacked-PR-specific database merge or graph composition
