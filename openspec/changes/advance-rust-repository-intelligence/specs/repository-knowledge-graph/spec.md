## ADDED Requirements

### Requirement: Typed Stable Repository Graph

ProjectAtlas SHALL represent projects, files, packages, declarations, external targets, relationships, source context, resolution, confidence, coverage, and generation identity through typed Rust contracts with one smallest owning module. Stable keys SHALL survive unchanged-content rescans and line movement while independent project instances, scopes, and overloads remain distinct. Legacy public relation values SHALL remain compatible; graph-only families SHALL use additive typed fields rather than changing the old exhaustive enum.

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

ProjectAtlas SHALL represent one logical source-kind-target relationship separately from its distinct source/evidence occurrences. Traversal, ranking, and impact SHALL deduplicate the logical edge, while detailed relation output MAY return bounded call sites, import sites, or other occurrences with exact source spans and total/returned/truncated metadata.

#### Scenario: One caller invokes the same target twice
- **WHEN** two source spans support the same logical call relationship
- **THEN** traversal follows one logical edge and detailed output retains both bounded call-site occurrences

### Requirement: One Indexed Storage Owner

Typed entities, relationships, source occurrences, coverage, and generation fields SHALL be persisted as typed SQLite columns under one schema owner. Queries SHALL use bounded indexed source/target/kind/path/stable-key access, propagate SQLite terminal failures, and SHALL NOT depend on per-edge JSON decoding or whole-graph scans. Graph publication SHALL not own, overwrite, or silently approve folder/file purposes.

#### Scenario: Row iteration fails after returning rows
- **WHEN** SQLite reports corruption, I/O failure, cancellation, or schema mismatch during iteration
- **THEN** the entire query fails and partial rows are not returned as successful output

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

Migrations, full publication, incremental updates, cleanup, rollback, and snapshot operations SHALL preserve project identity, purposes, review state, settings, and telemetry. Verified root moves preserve one project instance; independent copies/worktrees initialize or explicitly detach to a different instance. Snapshot import SHALL never overwrite destination identity.

#### Scenario: Independent checkout copies an index
- **WHEN** an accessible old root still owns the copied project identity
- **THEN** identity-preserving rebind is rejected and the copy must detach or initialize independently
