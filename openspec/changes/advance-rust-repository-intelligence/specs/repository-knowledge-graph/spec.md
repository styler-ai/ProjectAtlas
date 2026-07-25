## ADDED Requirements

### Requirement: Typed Stable Repository Graph

ProjectAtlas SHALL represent projects, files, packages, declarations, external targets, relationships, source context, resolution, confidence, coverage, and generation identity through typed Rust contracts with one smallest owning module. Stable keys SHALL survive unchanged-content rescans and line movement while independent project instances, scopes, and overloads remain distinct. Legacy 0.3.26 public relation values and normal requests SHALL remain compatible; graph-only families SHALL use additive typed fields rather than changing the old exhaustive enum.

#### Scenario: Repeated scan keeps identity
- **WHEN** unchanged declarations move only by formatting or line position
- **THEN** their stable identities remain unchanged and inbound relationships can be reconciled without name-only matching

#### Scenario: Stable key collides
- **WHEN** two distinct canonical identities produce the same compact stable key
- **THEN** ProjectAtlas detects the conflicting canonical material and fails or keeps the identities separate instead of silently merging entities

#### Scenario: Reference cannot be resolved uniquely
- **WHEN** several targets remain valid or no target is supported by static source context
- **THEN** ProjectAtlas records typed ambiguous or unresolved context and does not fabricate a traversable edge

### Requirement: Logical Relations Retain Every Source Occurrence

ProjectAtlas SHALL represent one logical source-kind-target relationship separately from its distinct source/evidence occurrences. Traversal, ranking, and impact SHALL deduplicate the logical edge, while detailed relation output MAY return bounded call sites, import sites, or other occurrences with exact source spans plus returned/truncated/continuation and typed exact/at-least/unknown total state. An exact total is optional unless it is already known or computable inside the same bound.

#### Scenario: One caller invokes the same target twice
- **WHEN** two source spans support the same logical call relationship
- **THEN** traversal follows one logical edge and detailed output retains both bounded call-site occurrences

### Requirement: One Indexed Storage Owner

Typed entities, deduplicated logical relationships, separate source occurrences, coverage, and generation fields SHALL be persisted as typed SQLite columns under one schema owner. The physical model SHALL support bounded indexed outbound and inbound adjacency, exact stable-key/path/kind/generation access, and affected-dependent invalidation without decoding per-edge JSON, scanning or materializing the whole graph, or relinking by display labels. Index order, coverage, and query plans SHALL be validated against representative high-cardinality and high-degree graph workloads. SQLite SHALL execute direct one-hop and affected-candidate set operations and batch-load unique endpoint selectors and accepted purposes; Rust service code SHALL control multi-hop traversal, ranking, aggregate limits, and result composition without loading the complete graph. Graph publication SHALL not own, duplicate, overwrite, invalidate, or silently approve folder/file purposes; local node and path results SHALL project purpose from the authoritative owning file/folder at query time.

#### Scenario: Row iteration fails after returning rows
- **WHEN** SQLite reports corruption, I/O failure, cancellation, or schema mismatch during iteration
- **THEN** the entire query fails and partial rows are not returned as successful output

#### Scenario: Huge graph returns one bounded neighborhood
- **WHEN** an agent requests bounded inbound or outbound connections for a high-degree local entity
- **THEN** SQLite uses the owning indexed adjacency path, returns explicit returned/truncated/continuation and typed at-least or exact total state, and does not scan the full high-degree adjacency or materialize unrelated graph rows solely to compute display metadata

#### Scenario: Purpose changes without structural graph change
- **WHEN** an owning file or folder purpose is explicitly approved or corrected while entity and relation identities remain unchanged
- **THEN** later graph-backed results project the current authoritative accepted purpose without rewriting or duplicating graph rows

#### Scenario: Source changes without purpose correction
- **WHEN** source bytes, hashes, symbols, summaries, entities, relations, or generations change without an explicit purpose correction
- **THEN** later graph-backed results retain the same accepted purpose while projecting the newly published derived facts

### Requirement: Database Architecture Remains Workload-Fit

ProjectAtlas SHALL maintain one versioned database architecture decision for its current local-first workload. The decision SHALL identify the supported SQLite library/driver and local-filesystem operating profile; authored, derived, cached, and rebuildable authority; conceptual, logical, and physical data models; stable keys and integrity constraints; hot read/write paths and owning indexes; prepared/batched operations; publication and read-snapshot transaction boundaries; WAL, busy, checkpoint, concurrency, migration, backup, recovery, and corruption behavior; huge-source scale/resource assumptions; and measurable conditions that require the engine decision to be revisited. SQLite SHALL remain the default only while it satisfies the accepted offline, cross-platform, bounded-read, atomic-publication, and packaging contracts better than realistic alternatives. Database design SHALL remain owned by `projectatlas-db` inside the accepted seven-crate architecture unless a durable independent owner and consumer are demonstrated.

#### Scenario: Graph database is proposed because the data is graph-shaped
- **WHEN** an alternative engine is proposed without a measured failure of bounded indexed SQLite traversal, publication, concurrency, recovery, packaging, or scale contracts
- **THEN** ProjectAtlas retains the simpler embedded design and fixes the logical/physical model or query path at its owning boundary

#### Scenario: SQLite operating assumptions stop matching the product
- **WHEN** a required workload needs shared remote multi-writer state, an unsupported live network filesystem, unbounded distributed graph computation, or still misses declared scale/query/publication thresholds after correct schema, index, query, and transaction tuning
- **THEN** the engine decision is explicitly reopened with representative measurements rather than hidden behind another storage abstraction

#### Scenario: Storage behavior is verified
- **WHEN** graph, purpose projection, publication, migration, or recovery behavior is claimed complete
- **THEN** owning tests write through the real transaction API to a temporary SQLite database and read or reopen it through the real bounded query API, with risk-required constraint, rollback, read-snapshot, query-plan, corruption, and concurrency cases instead of a mocked repository or SQL-text-only assertion

#### Scenario: Writable WAL placement is unsupported or uncertain
- **WHEN** connection preflight proves a live network filesystem unsupported by SQLite WAL or cannot establish the required local locking/shared-memory profile
- **THEN** ProjectAtlas returns typed unsupported or uncertain state before mutation and does not silently weaken durability or continue with the selected WAL profile

### Requirement: Project-Local Storage Growth Is Bounded

Each resolved runtime/request source-state binding SHALL select exactly one authoritative project database containing its authored atlas state and active complete derived generation. Normal root, `project_path`, and nearest-project discovery SHALL select `<root>/.projectatlas/projectatlas.db`; an explicit startup database MAY select a noncanonical path for an isolated host, test, migration/verification, or protected runtime binding, but it SHALL NOT be auto-discovered, attached, merged, substituted, used as a fallback, or combined with another database in one read, publication, authored mutation, telemetry write, or result. ProjectAtlas SHALL NOT split purposes, future Memory Atlas records, graph/index projections, settings, health resolutions, or telemetry for one binding into separate product databases. ProjectAtlas MAY spill one admitted full projection into one bounded private SQLite-backed staging directory. The stage SHALL NOT be a second authoritative project database. It SHALL contain only rebuildable scan/graph rows and internal schema, ownership, and copy-validation metadata, including the exact root, selected project identity, staging-only marker, and target graph generation; it SHALL contain no authored rows, SHALL NOT be selected by normal root/project discovery, and SHALL expose no supported CLI/MCP result surface. ProjectAtlas SHALL checkpoint and close the staging store before ownership-validated removal after publish, cancellation, failure, or restart; incomplete or uncertain creation SHALL be retained fail-closed rather than recursively deleted. Recent raw telemetry and retained aggregates SHALL have explicit row, age, and byte budgets; compaction SHALL preserve supported all-time token-report totals and declared trend semantics while reporting honestly when expired session-level detail is unavailable. Obsolete derived rows, abandoned owned staging artifacts, WAL growth, free pages, and planner statistics SHALL have measured maintenance owners. Normal agent reads SHALL NOT run an unbounded purge, blocking truncate checkpoint, or blind database rebuild/`VACUUM`, and cleanup SHALL NOT delete project identity, reviewed purposes, health resolutions, or future separately capped Memory Atlas state.

Implicit telemetry SHALL use a bounded internal runtime or invocation session instance rather than one eternal default deduplication scope. An optional caller-visible session label SHALL remain distinct from that internal instance. Session-scoped modeled accounting SHALL preserve the established per-baseline `max(baseline without ProjectAtlas) - sum(emitted with ProjectAtlas)` contribution while an instance is active; sealing or expiry SHALL NOT silently reopen discarded baseline state, and later reuse of a label SHALL start a new instance. Global supported totals SHALL remain exact after compaction while session detail and trend availability SHALL report retained, partial, expired, or unavailable truth explicitly.

#### Scenario: Explicit database binding remains isolated
- **WHEN** a runtime starts with an explicit noncanonical database while the same source root also has a conventional project database
- **THEN** every request uses exactly its captured root/database/configuration binding and ProjectAtlas does not discover, merge, fall back to, or mutate the other database

#### Scenario: Telemetry exceeds the raw retention budget
- **WHEN** successful local agent use creates more raw usage events than the declared row, age, or byte budget
- **THEN** ProjectAtlas atomically compacts or expires raw detail under its retention contract, keeps supported totals and trends correct, reports raw-detail availability honestly, and stops database growth from remaining proportional to lifetime tool-call count

#### Scenario: An expired session label is reused
- **WHEN** a caller reuses a public session label after its prior internal instance was sealed and its baseline state expired
- **THEN** ProjectAtlas creates a new bounded session instance, preserves exact global totals, and reports prior label history as partial or expired instead of reusing or fabricating the old deduplication scope

#### Scenario: Restart encounters an owned disposable graph stage
- **WHEN** restart cleanup encounters a direct staging directory whose SQLite marker binds the exact root and selected project
- **THEN** ProjectAtlas closes the validating connection, removes only that non-authoritative stage with its marker last, and preserves the authoritative project database and its last complete generation

#### Scenario: Staging ownership is incomplete or uncertain
- **WHEN** restart cleanup cannot prove the exact root, selected project, and direct staging marker for a non-empty stage, or encounters a linked or lookalike stage path
- **THEN** it retains that state fail-closed and never follows or recursively deletes it; only an empty direct stage shell may be removed

#### Scenario: Indexed source shrinks substantially
- **WHEN** obsolete derived rows and free pages accumulate after a large local source tree becomes smaller
- **THEN** the measured maintenance lifecycle makes those pages reusable or reclaimable without blocking a normal agent read or deleting authored atlas state

### Requirement: Advertised Relation Families Are End-To-End Live

Every advertised relation family SHALL have one responsibility owner, producer/provider, typed persistence projection, invalidation rule, settings/capability row, query consumer, current coverage state, positive fixture, and negative or ambiguity fixture. Validation SHALL reject a family that is registered but cannot be produced, persisted, invalidated, queried, reported, or meaningfully tested.

#### Scenario: Registry entry has no query consumer
- **WHEN** a relation family appears in capability state but no current query path can return its persisted rows
- **THEN** validation fails and the family is not advertised

### Requirement: Accepted Relation Families Cannot Shrink Silently

One versioned accepted relation-family inventory SHALL own required direct structural/type, package/manifest, test, route/protocol, configuration/environment, deployment/infrastructure, and bounded static read/write families plus separately gated inferred similarity/co-change families. Persistence, invalidation, settings/capability rows, query consumers, positive/negative/ambiguity fixtures, and generated documentation SHALL derive from the inventory. Removing or weakening an accepted row SHALL require an explicit compatibility decision and inventory-version change; validation SHALL fail when an accepted row is omitted from any required end-to-end owner. Product Rust and tests SHALL NOT duplicate mutable family totals as literals.

#### Scenario: An implementation stops advertising an accepted relation
- **WHEN** an accepted family is removed from generated capability state or lacks a producer, persistence projection, invalidation rule, query consumer, fixture, or documentation row
- **THEN** validation fails against the accepted inventory rather than treating the smaller advertised set as complete

### Requirement: Existing Settings Report Graph Truth

The existing settings surface SHALL report schema compatibility and migration state, active generation, optional physical-slot details only when slotting is selected, registry/provider/relation capability digests, actionable coverage state, lexical/FTS/search modes, and optional parser/semantic lifecycle readiness. It SHALL not expose secret-bearing values or new machine-local paths.

#### Scenario: Optional pack is stale
- **WHEN** the selected graph generation no longer matches the optional pack generation
- **THEN** settings reports structural state as valid and the pack/search mode as stale rather than collapsing them into one readiness value

### Requirement: Authored Data And Project Identity Are Preserved

Migrations, full publication, incremental updates, cleanup, rollback, and snapshot operations SHALL preserve project identity, accepted purposes, approval/provenance state, settings, and telemetry. Deterministic or heuristic suggestions remain unapproved until an agent accepts them. Automation SHALL NOT invalidate an accepted purpose because source or graph state changed; explicit agent/user correction remains allowed. An absent node SHALL keep its path-owned accepted purpose dormant and excluded from current navigation; exact-path recreation MAY reactivate it, while a rename SHALL leave the old path dormant and SHALL NOT transfer approval automatically to the new path. Verified root moves preserve one project instance; independent copies/worktrees initialize or explicitly detach to a different instance. Snapshot import SHALL never overwrite destination identity.

#### Scenario: Independent checkout copies an index
- **WHEN** an accessible old root still owns the copied project identity
- **THEN** identity-preserving rebind is rejected and the copy must detach or initialize independently
