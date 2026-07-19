## ADDED Requirements

### Requirement: Federation Is Explicit, Call-Only, And Read-Only

Federated relation or analysis calls SHALL receive their complete ordered set of already indexed roots explicitly. They SHALL enforce hard participating-root and simultaneously open connection/read-snapshot limits plus aggregate database/input bytes, rows, entities, edges, decoded/intermediate bytes, elapsed time, output bytes, and cancellation-to-close budgets. Participating databases SHALL be opened read-only/query-only and all validated before rows are returned. Federation SHALL not discover roots, initialize or scan a root, change the active project, persist roots/edges/cache, retain open handles, write telemetry/settings, or mutate any project.

#### Scenario: One selected root is invalid
- **WHEN** a root is missing, stale beyond the request contract, corrupt, unsupported, or bound to another project
- **THEN** the whole call fails without returning a plausible partial result or mutating any root

#### Scenario: Existing single-project request omits roots
- **WHEN** a client uses the pre-change request shape
- **THEN** only the selected project is queried and behavior remains compatible

#### Scenario: Many roots include one invalid late root
- **WHEN** a bounded ordered root set contains a stale, corrupt, or incompatible database after several valid roots
- **THEN** the call closes every captured snapshot and returns no partial rows, cache, telemetry, settings, or active-project mutation

#### Scenario: Federation is canceled with several roots open
- **WHEN** cancellation or an aggregate resource limit is reached during validation or bounded resolution
- **THEN** every connection closes within the cancellation budget and no cross-root state survives the call

### Requirement: Cross-Root Results Remain Project-Qualified And Fresh

Every result SHALL retain original project identity and source context plus the captured generation for each root. Typed package/protocol/configuration rendezvous MAY be normalized in bounded call memory, including statically supported route/client/handler, RPC, schema, topic/channel, and configuration identities, but ambiguity SHALL remain explicit and the derived state SHALL be discarded when the call ends. Similar strings alone SHALL not establish a cross-root relationship.

#### Scenario: Two projects contain the same relative path
- **WHEN** both roots participate in one query
- **THEN** their entities remain distinct through project-qualified identity

#### Scenario: A root changes between pages
- **WHEN** a federated cursor no longer matches one root's captured generation
- **THEN** continuation fails stale rather than serving mixed-root generations

### Requirement: Static Protocol Boundary Fields Remain Traceable

Within one selected source tree or explicit federated roots, statically supported protocol relationships MAY map caller argument/request fields to handler parameter/request fields. Each mapping SHALL retain normalized protocol and endpoint identity, source and target field/parameter paths and types, exact spans, resolution or ambiguity, coverage, invalidation identity, active generation, bounded output metadata, and secret-value exclusion. This capability SHALL NOT claim general interprocedural def-use or taint analysis.

#### Scenario: Request fields align statically
- **WHEN** a client and handler expose compatible statically visible protocol identities and field paths
- **THEN** ProjectAtlas returns the bounded mapping with both source spans and a reusable handler target selector

#### Scenario: Dynamic or secret-bearing field cannot be mapped safely
- **WHEN** identity or field flow depends on runtime behavior or contains a secret literal/value
- **THEN** ProjectAtlas reports partial, ambiguous, or unsupported coverage and does not persist or return the secret value
