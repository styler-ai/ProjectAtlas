## ADDED Requirements

### Requirement: Typed Repository Graph Contract
ProjectAtlas SHALL represent repository, folder, file, package/module, declaration, reference, endpoint, route, channel, configuration, environment, infrastructure, test, and external identities through typed Rust domain models. It SHALL represent approved containment, declaration, import/export, call/reference, inheritance/implementation/override, test, dependency, route/RPC/channel, configuration, deployment, generation, similarity, history-coupling, direct read/write, and cross-repository relationships through a versioned `#[non_exhaustive]` `GraphRelationKind`. The initial four legacy-compatible relation families SHALL carry no generic discriminator or generic payload. A future relation family MAY introduce a relation-owned typed payload only after its single traceability row is ratified; ProjectAtlas SHALL NOT introduce a generic discriminator, property bag, or payload framework. The existing public `RelationKind` enum and its four variants SHALL remain source/serialization compatible; compatibility conversions SHALL map legacy facts into the richer graph without adding variants to the exhaustive legacy enum.

ProjectAtlas SHALL persist its typed Rust identities, relation kinds, evidence, confidence, coverage, and limits as typed SQLite rows under one schema owner. Generic JSON property bags and a second graph schema owner are prohibited. Arena allocation, string interning, compact numeric IDs, adjacency representation, and other physical hot-path layouts remain internal implementation details that MAY optimize storage or execution but SHALL NOT change graph semantics, serialized identities, ownership, or public contracts.

#### Scenario: Graph records are serialized
- **WHEN** an extractor or resolver persists a graph entity or relation
- **THEN** the database receives a schema-versioned typed record rather than adapter-authored protocol/status string literals

#### Scenario: An adapter needs private detail
- **WHEN** a language or framework adapter emits detail not shared by other adapters
- **THEN** the typed adapter-local payload remains namespaced and does not expand the cross-crate public contract prematurely

#### Scenario: Existing Rust caller matches every legacy relation
- **WHEN** downstream code exhaustively matches the four existing `RelationKind` variants after this program is released
- **THEN** it continues to compile and receive the same legacy serialized values while new graph APIs use `GraphRelationKind`

#### Scenario: Compact graph layout changes
- **WHEN** ProjectAtlas changes an internal allocation, interning, numeric-identity, or adjacency representation
- **THEN** canonical graph equivalence, serialized identities, schema ownership, and public compatibility remain exact

### Requirement: Stable Identity Across Generations
Every ProjectAtlas database SHALL persist a generated `ProjectInstanceId` that survives an explicit verified root move and same-project snapshot round trips but differs for independently initialized clones/worktrees. This change SHALL NOT provide an ambient or implicit identity-adoption operation. It MAY also record a normalized VCS-derived `RepositoryIdentity` for federation; forks/worktrees/clones SHALL remain distinguishable by the pair of identities. Each persisted entity kind SHALL define a versioned kind-specific canonical `EntityKey` encoding rather than requiring fields that do not apply to that kind. Database row identifiers MAY change, but entity keys SHALL survive unchanged-content full scans, incremental scans, process restarts, and supported snapshot round trips.

Repository-relative identity SHALL normalize separators and dot components while preserving case and the exact Unicode codepoint sequence recorded by Git. ProjectAtlas SHALL NOT case-fold or Unicode-normalize two distinct Git paths into one identity.

A normal open at a root different from the stored binding SHALL remain read-only and return typed `RebindRequired`. Root movement SHALL occur only through an explicit additive mode on the existing root-set surface with the prior canonical root, current database identity/digest, candidate new root, and operator intent. Read-only preflight SHALL verify that the database is project-local at the new root, the old binding and `ProjectInstanceId` match, the new root is not already bound to another ProjectAtlas database, and an accessible old root does not still contain a database owning the same instance identity. After a verified backup, one transaction SHALL move the binding through `Rebinding` to `Bound` and append an audit record; failure leaves the old binding and identity unchanged. A copied/independent checkout SHALL instead initialize normally or use an explicit detach operation that generates a new `ProjectInstanceId` while retaining any compatible `RepositoryIdentity`. Snapshot import SHALL never overwrite the destination instance identity.

A `LogicalEdgeKey` SHALL identify one source entity, target entity or typed external identity, graph relation kind, and, only when a ratified relation-family traceability row defines them, relation-owned stable identity fields without mutable source spans. The initial four legacy-compatible relation families SHALL use no additional discriminator, and ProjectAtlas SHALL NOT add a generic relation discriminator. One logical edge MAY own many source occurrences. Each occurrence SHALL have an `EvidenceOccurrenceKey` derived from logical edge, origin identity, resolver/version, content-anchored span fingerprint, and occurrence discriminator. Hash rows SHALL store encoding version and enough canonical identity material to detect a collision; a collision SHALL fail publication rather than silently alias entities or edges.

#### Scenario: Unchanged symbol is re-indexed
- **WHEN** a full or incremental generation re-extracts a symbol whose identity inputs did not change
- **THEN** the symbol retains the same stable key and inbound relationships can be reconciled without name-only matching

#### Scenario: Duplicate names exist
- **WHEN** two symbols share a short name in different scopes or overload positions
- **THEN** their stable keys remain distinct and query results expose enough scope/signature evidence to disambiguate them

#### Scenario: One call target has multiple call sites
- **WHEN** several source occurrences resolve to the same logical source/target/relation identity
- **THEN** ProjectAtlas stores one logical edge with distinct evidence occurrences and impact/traversal deduplicates the edge without losing call-site evidence

#### Scenario: Repository is cloned or moved
- **WHEN** a project root moves or an independent clone/worktree is initialized
- **THEN** root moves preserve `ProjectInstanceId`, independent initializations receive distinct instance IDs, and optional `RepositoryIdentity` can express shared VCS lineage without collapsing project instances

#### Scenario: Copied database still exists at the old root
- **WHEN** a caller requests move-rebind but an accessible old-root database still owns the same `ProjectInstanceId`
- **THEN** ProjectAtlas rejects identity-preserving rebind and directs the caller to detach the copy into a new instance identity

#### Scenario: Verified root move fails during update
- **WHEN** backup, binding update, audit append, or post-write verification fails
- **THEN** the transaction rolls back and the database remains bound to the prior root and instance identity

### Requirement: Evidence, Confidence, And Resolution State
Every non-containment semantic reference SHALL record its producing resolver, source file/span or other evidence, confidence class, active index slot/epoch, and optional candidate/explanation metadata. The state SHALL use orthogonal typed axes: `ResolutionStatus { Resolved, Ambiguous, Unresolved }`, `TargetScope { Internal, External }` when a target exists, `EvidenceClass { Direct, Inferred }`, `Completeness { Complete, Partial, Truncated }`, and a finite `ConfidenceClass`. Only a resolved reference with a typed internal or external target becomes a traversable logical edge. Ambiguous and unresolved references SHALL remain first-class non-traversable occurrences with bounded candidates/reasons; query truncation SHALL NOT mutate stored resolution state.

#### Scenario: A semantic edge is inspected
- **WHEN** an agent requests relation evidence
- **THEN** ProjectAtlas returns the origin span, resolver, confidence, resolution state, and target identity within the caller's limits

#### Scenario: An inferred relationship is displayed
- **WHEN** a relation comes from similarity, history, naming, or framework inference rather than direct syntax
- **THEN** output labels it inferred and does not merge it with direct resolved relationships

#### Scenario: Ambiguous reference is queried
- **WHEN** a source occurrence has multiple viable targets
- **THEN** it is returned as non-traversable `Ambiguous` resolution with bounded candidates while evidence class, completeness, and confidence remain independently inspectable

### Requirement: Protocol Boundary Argument Evidence
For each accepted HTTP, RPC, GraphQL, tRPC, broker, topic, or channel adapter whose caller and handler boundaries are both statically visible, ProjectAtlas SHALL represent typed caller argument or request-field to handler parameter or request-field evidence. Each occurrence SHALL carry normalized protocol and endpoint identities, bounded field/parameter identifiers and types, source and target spans, resolver/version, confidence, resolution/ambiguity state, coverage, and incremental dependency keys. Persistence SHALL retain identifiers, types, and field paths needed for resolution but SHALL NOT retain secret literal values; agent output SHALL be bounded and redact secret-bearing evidence. This capability is protocol-boundary mapping, not a claim of general interprocedural def-use or taint analysis.

#### Scenario: Static client and handler fields align
- **WHEN** an accepted adapter can statically resolve a caller request field to one handler request field
- **THEN** ProjectAtlas persists one evidence-bearing protocol-boundary relation with both spans and the normalized endpoint identity

#### Scenario: Dynamic or secret-bearing argument is encountered
- **WHEN** a field is computed dynamically, maps to multiple handler parameters, or contains a secret literal
- **THEN** ProjectAtlas records partial, ambiguous, or unresolved evidence as appropriate and excludes or redacts the secret value without fabricating a resolved data-flow edge

### Requirement: Versioned Graph Schema And Migrations
The SQLite graph schema SHALL have a runtime-owned monotonically ordered schema identifier, an append-only forward migration ledger, compatibility ranges, and an explicit rollback/rebuild policy. This specification SHALL NOT reserve numeric schema versions. Each implementation migration SHALL receive a unique immutable ledger identifier when accepted; released identifiers SHALL never be reused, reordered, or silently rewritten. Ledger rows SHALL record the migration identity and integrity digest needed to detect a database whose declared version and applied migration history disagree.

Before enabling write-capable pragmas, creating a table, running DDL/DML, repairing metadata, or opening a migration transaction, the runtime SHALL perform a read-only preflight of file identity, SQLite header/integrity availability, project/root binding, schema identifier, migration-ledger shape/digest, required authored tables, and runtime read/write compatibility. Unknown future schemas, ledger gaps or rewrites, incompatible optional-pack schemas, and failed preflight SHALL return a typed compatibility error without modifying the database. A supported migration SHALL run transactionally only after disk/backup preflight and SHALL preserve project identity, approved purposes, purpose review state, telemetry, settings, and other authored metadata. Unknown future schemas and failed migrations SHALL be rejected without destructive auto-rebuild.

#### Scenario: Supported old schema is opened
- **WHEN** ProjectAtlas opens a database with a supported older graph schema
- **THEN** it migrates transactionally, validates the result, and reports the old and new versions

#### Scenario: Newer unknown schema is opened
- **WHEN** the runtime encounters a graph schema newer than it understands
- **THEN** it refuses writes and gives upgrade guidance without deleting or replacing the database

#### Scenario: Unknown schema is preflighted
- **WHEN** a database reports a schema version newer than the runtime understands
- **THEN** no table, column, index, trigger, pragma that writes, or metadata repair is attempted before the runtime returns the compatibility error

#### Scenario: Migration ledger disagrees with declared schema
- **WHEN** the ledger has a missing, reordered, unknown, or digest-mismatched released migration entry
- **THEN** the runtime refuses migration and writes, reports the exact ledger incompatibility, and leaves the database byte-for-byte unmodified

### Requirement: Relation Family Traceability
Every public `GraphRelationKind` family SHALL have exactly one owning module and one machine-readable traceability row. That row SHALL identify its relation-owned typed payload/schema or explicitly record that the family has no payload, define every relation-owned payload field that participates in stable identity, and identify all enabled producers, at least one consuming query or agent workflow, positive fixtures, ambiguous/unresolved fixtures where applicable, adversarial negative fixtures, required precision/recall and coverage metrics, capability/pack prerequisites, evidence and confidence rules, invalidation input keys, incremental recomputation/removal behavior, and the tests and benchmark artifacts that prove those contracts. Generated capability/settings output, persistence schema, query filters, documentation, and release claims SHALL be derived from this accepted traceability inventory.

A relation family SHALL remain adapter-private or unavailable until its owner, producer, consumer, fixtures, metrics, and invalidation contract all pass. An aggregate graph score or another relation family SHALL NOT compensate for a failing advertised family. Direct read/write relationships SHALL be limited to evidence-backed configuration, environment, database/schema, state, or provider-defined source/sink facts; they SHALL NOT be advertised as general interprocedural data flow without a separately accepted and benchmarked def-use/source-sink specification.

#### Scenario: New relation kind is proposed
- **WHEN** any owner, producer, consumer, positive/negative fixture, coverage rule, precision threshold, or invalidation rule is missing
- **THEN** generation/review fails and the relation kind remains adapter-private or deferred

#### Scenario: Advertised relation family regresses
- **WHEN** any required producer, consumer, fixture class, per-family accuracy metric, or incremental invalidation test falls below its accepted contract
- **THEN** that family is removed from advertised capabilities or the release is blocked, and persisted/query output cannot report the family as complete

### Requirement: Graph Schema And Capability Settings
The existing `atlas_settings` surface and its CLI equivalent SHALL expose one backward-compatible bounded graph-capability section derived from live metadata rather than duplicated constants. It SHALL report the current schema identifier, read/write compatibility state, migration-required state, migration-ledger digest, captured `active_slot`/`active_epoch`, accepted language-capability-set identity/digest and achieved tiers, relation-family inventory identity/digest, structural/lexical coverage, FTS availability/activation, and optional grammar/model/vector pack lifecycle and capability state. It SHALL distinguish unavailable, disabled, pending, partial, stale, ready, incompatible, and failed states where applicable and SHALL NOT report a capability as ready from installed files alone.

Existing settings fields and previously valid requests SHALL remain valid. The settings response SHALL not expose secrets, model inputs, repository source, or new machine-local paths beyond existing path-reporting policy. Schema/capability fields used by CLI, MCP, documentation, migration diagnostics, and release manifests SHALL share one typed source.

#### Scenario: Current graph settings are inspected
- **WHEN** a caller invokes `atlas_settings` after a successful publication
- **THEN** the response identifies the compatible schema/ledger, captured active slot/epoch, accepted capability set, achieved relation/language tiers, coverage, and optional-pack state that queries will use

#### Scenario: Database needs migration or a pack is incomplete
- **WHEN** read-only preflight finds a supported older schema or an enabled pack lacks compatible generated records
- **THEN** settings reports `migration-required` or the pack's non-ready state without claiming the affected graph capability is ready or mutating the database

### Requirement: Coverage-Preserving Graph Limits
Symbol, relation, traversal, file-size, and resolver budgets SHALL be explicit and configurable within safe maxima. Hitting a budget SHALL record coverage and truncation evidence at the affected file/pass/relation level; it SHALL NOT silently produce a graph reported as complete.

#### Scenario: Relation cap is reached
- **WHEN** an extraction or resolver pass reaches a configured relation cap
- **THEN** ProjectAtlas preserves the bounded results, records the omitted category and reason, marks coverage partial, and prevents a complete-ready claim

#### Scenario: Graph remains within budget
- **WHEN** all required passes finish within their limits
- **THEN** the active generation reports complete coverage for those passes with counts that reconcile to persisted rows

### Requirement: Compact Existing-Call Integration
The active repository graph SHALL enrich existing `atlas_files`, `atlas_file_summary`, `atlas_search`, `atlas_symbols`, and `atlas_symbol_relations` behavior without requiring agents to learn a new default workflow. Default file summaries SHALL include only bounded relation counts, high-value related identities, coverage state, and typed next-call hints; full edge evidence SHALL require a relation or analysis call.

#### Scenario: Existing summary call runs after graph enrichment
- **WHEN** a client sends a previously valid `atlas_file_summary` request
- **THEN** it receives a backward-compatible compact response whose additional graph digest stays within the documented row/token limits

#### Scenario: Full evidence would be large
- **WHEN** a file has more relationships than the default summary limit
- **THEN** the summary reports total/returned/truncated counters and recommends the existing relation surface instead of expanding unbounded output

### Requirement: Memory-Efficient Graph Representation
The active graph construction and query path SHALL use benchmark-selected compact typed storage such as contiguous vectors/arenas, numeric internal identifiers, string/path interning, bounded worker-local batches, and relation-kind-owned identity fields. Normal extraction, persistence, adjacency lookup, and traversal SHALL NOT require per-edge JSON parsing or one independent heap allocation per entity/relation. Source, target, kind, package/module, stable identity, and generation indexes SHALL support bounded queries without whole-graph scans.

#### Scenario: Million-node graph is constructed
- **WHEN** the scale harness builds the declared million-node/multi-million-edge graph with N workers
- **THEN** peak RSS, allocations, merge time, persistence throughput, and index bytes meet the published resource gate while every retained fact reconciles

#### Scenario: Direct relation query runs
- **WHEN** a caller requests indexed inbound/outbound relations by stable identity and kind
- **THEN** ProjectAtlas uses typed adjacency/index lookups and does not deserialize unrelated JSON property bags or scan the full edge table
