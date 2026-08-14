## ADDED Requirements

### Requirement: Repository-lifetime token telemetry
ProjectAtlas SHALL maintain one exact deduplicated lifetime savings total for the logical repository across all of its worktrees while retaining per-worktree and per-session dimensions.

#### Scenario: Two worktrees contribute to one total
- **WHEN** legitimate telemetry-producing calls execute in `main` and an issue worktree
- **THEN** the repository report equals the exact sum of admitted distinct events and exposes each worktree contribution

#### Scenario: Worktree retirement preserves total
- **WHEN** a reconciled worktree is retired and its local derived database is later removed
- **THEN** the repository-lifetime calls, tokens avoided, likely reads avoided, and accounting components remain unchanged

#### Scenario: Unrelated repositories stay isolated
- **WHEN** two repositories use ProjectAtlas concurrently
- **THEN** neither repository's report includes the other's events or aggregates

#### Scenario: Team seed remains telemetry-free
- **WHEN** CI seals or publishes a main seed and purpose promotions
- **THEN** no repository-lifetime, worktree, session, process, task, event, aggregate, or private telemetry state is copied into the seed, manifest, promotion delta, Git, LFS, release, or cache artifact

### Requirement: Event admission is exactly-once under retries and concurrency
ProjectAtlas SHALL allocate repository-ordered usage-instance identities, require contiguous per-instance event sequences, and retain compact admission high-water/closed-range state independently of evictable event payload so a retry cannot inflate totals.

#### Scenario: Same event is retried
- **WHEN** a runtime retries an event after an uncertain response
- **THEN** an equal or lower sequence is a deterministic duplicate and the repository transaction does not change aggregates even when raw detail has been evicted

#### Scenario: Event sequence has a gap
- **WHEN** an active usage instance submits a sequence above the next expected value
- **THEN** ProjectAtlas returns typed retry guidance without admitting the event or advancing aggregates

#### Scenario: Concurrent distinct events
- **WHEN** multiple CLI/MCP processes in several worktrees publish distinct events concurrently
- **THEN** every valid distinct event is attributed through its request-captured exact repository/worktree binding and counted once with bounded busy handling, no sibling bleed, and no application-wide hidden lock

#### Scenario: Process dies during publication
- **WHEN** the writer process terminates before commit
- **THEN** SQLite recovery exposes either the entire event-plus-aggregate transaction or none of it and the retry remains idempotent

#### Scenario: Usage instance is sealed
- **WHEN** an instance closes normally or recovery proves its owner absent and resolves its pending sequence
- **THEN** ProjectAtlas compacts it into durable closed-instance range state and rejects every later event for that instance without retaining all event identities

### Requirement: Reports preserve accounting truth and bounded detail
ProjectAtlas SHALL preserve the current conservative accounting layers, estimator labels, confidence, baseline kinds, deduplication scopes, exact totals, retained-detail state, and bounded output while adding repository/worktree/session scope.

#### Scenario: Repository total reconciles
- **WHEN** a token report is requested without a worktree filter
- **THEN** headline and component totals reconcile exactly across admitted worktree aggregates without materializing all raw events

#### Scenario: Worktree and session filters reconcile
- **WHEN** a caller filters by known worktree or session identity
- **THEN** the report returns only that bounded dimension and its totals reconcile with the repository total

#### Scenario: Raw detail is evicted
- **WHEN** retention removes old raw event detail
- **THEN** exact all-time aggregate components remain and the report states that detail is unavailable rather than inventing rows

#### Scenario: Human TUI shows scope
- **WHEN** the token TUI renders repository-lifetime and selected-worktree values
- **THEN** labels make both scopes unambiguous, version identity and selected root/generation are exact, repository/worktree overview remains complete within declared bounds, and narrow layouts do not silently substitute or blend one scope or graph for another

### Requirement: Existing telemetry migration preserves truth without double counting
ProjectAtlas SHALL import compatible existing telemetry through consistent snapshots, source fingerprints, component reconciliation, atomic receipts, preserved originals, and explicit completeness. It SHALL NOT claim an exact combined lifetime total when aggregate-only predecessor histories may overlap.

#### Scenario: Compatible worktree history imports
- **WHEN** an existing worktree database contains retained events and exact aggregates
- **THEN** ProjectAtlas imports each source component once, records provenance/receipt, reconciles the destination, and leaves the source unchanged

#### Scenario: Same historical event appears in copied databases
- **WHEN** two source databases contain events with the same stable runtime/event identity
- **THEN** uniqueness admits one event and migration reports the deduplicated source overlap

#### Scenario: Aggregate-only predecessor has provably disjoint authority
- **WHEN** a supported predecessor retained exact aggregates but no raw events and its authority epoch or instance provenance proves disjointness
- **THEN** ProjectAtlas imports typed aggregate provenance without fabricating raw events and records the disjoint authority range

#### Scenario: Aggregate-only predecessors may overlap
- **WHEN** copied or partially overlapping aggregate-only databases cannot prove disjoint authority epochs or instance ownership
- **THEN** ProjectAtlas preserves every source, selects one explicit canonical source only under deterministic policy or user authorization, otherwise reports typed incomplete/lower-bound history, never silently sums the uncertain totals, and does not block source navigation or the product upgrade

#### Scenario: Incompatible source is preserved
- **WHEN** a database is malformed, corrupt, wrong-root, or newer than the installed migration contract
- **THEN** ProjectAtlas refuses before source or destination mutation and returns typed recovery guidance

### Requirement: Telemetry remains local, private, and available without Git or network
ProjectAtlas SHALL estimate and persist telemetry locally and SHALL not require Git, GitHub, network tokenizers, or external billing APIs for normal reporting.

#### Scenario: Git executable is missing
- **WHEN** ProjectAtlas runs in a structurally valid checkout without a resolvable Git executable
- **THEN** CLI/MCP navigation and telemetry publication continue while Git-specific lifecycle fields remain typed unavailable

#### Scenario: Network is unavailable
- **WHEN** the host has no GitHub or internet connectivity
- **THEN** an already installed runtime records and reports local telemetry without requiring seed retrieval or destructive installer or marketplace mutation

#### Scenario: Telemetry is disabled
- **WHEN** the documented no-telemetry control is active
- **THEN** no repository or worktree telemetry event, aggregate, instance, or import state is created by ordinary read calls

### Requirement: Telemetry storage is bounded and observable
ProjectAtlas SHALL use prepared bounded queries, short caller-owned transactions, indexed repository/worktree/session access, explicit WAL/checkpoint behavior, bounded retention, and typed storage diagnostics.

#### Scenario: High-event many-worktree repository
- **WHEN** event volume and worktree count reach supported representative limits
- **THEN** report latency, lock duration, WAL growth, checkpoint behavior, active-instance and closed-range counts, retained-detail bytes, RSS, I/O, persistent bytes, and output remain within measured release limits

#### Scenario: Busy continuity writer
- **WHEN** another valid process holds the SQLite writer temporarily
- **THEN** publication uses bounded busy behavior, never spins indefinitely, and returns typed retry state without losing or duplicating the event

#### Scenario: Continuity database is corrupt
- **WHEN** integrity or row conversion fails
- **THEN** ProjectAtlas returns the failure for the whole operation, preserves last-valid files and backups, and never returns a partial successful total
