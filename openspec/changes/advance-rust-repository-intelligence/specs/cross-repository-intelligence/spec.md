## ADDED Requirements

### Requirement: Explicit Federation Scope
Cross-repository intelligence SHALL operate only over an explicit ordered set of valid indexed ProjectAtlas roots supplied in each approved relation, architecture, impact, or trace call. Federation roots SHALL NOT be stored as an ambient configuration/default or inherited from an earlier call. Federation SHALL NOT discover projects by scanning cache filenames, create a hidden global graph database, initialize or scan an unindexed root implicitly, or change the active default project as a side effect of a read query.

#### Scenario: Caller supplies indexed roots
- **WHEN** all roots have valid project-local databases and pass root/config binding checks
- **THEN** ProjectAtlas opens them with explicit identities and evaluates a bounded federated view

#### Scenario: Root lacks an index
- **WHEN** one addressed root has no valid `.projectatlas/projectatlas.db`
- **THEN** ProjectAtlas reports the missing index and performs no initialization or mutation

### Requirement: Stable External And Protocol Identities
ProjectAtlas SHALL canonicalize package, repository, HTTP, RPC, GraphQL, tRPC, async topic/channel, database, and configuration rendezvous through typed protocol-specific identities. Canonicalization SHALL account for language/package namespace conventions, path parameters/templates, prefixes, builders/constants, versions, and transport semantics and SHALL retain the original evidence on both sides.

#### Scenario: Cross-language RPC identities correspond
- **WHEN** two repositories express the same service/method under language-specific generated package prefixes
- **THEN** the canonical protocol identity can match them while preserving both original qualified names and the normalization explanation

#### Scenario: Similar strings are not the same endpoint
- **WHEN** a test, CI path, regex, documentation string, or unrelated constant resembles a route or topic without protocol evidence
- **THEN** ProjectAtlas does not create a resolved cross-repository relation

### Requirement: Explainable Cross-Repository Resolution
Every cross-repository relation SHALL include origin and target project/repository identities, relation kind, canonical rendezvous identity, source evidence, confidence, resolver version, ambiguity state, and captured active slot/epoch for each participating root. Exact, normalized, inferred, ambiguous, and unresolved matches SHALL remain distinct; ProjectAtlas SHALL NOT materialize a bidirectional resolved edge from exact-string similarity alone when protocol evidence is incomplete.

#### Scenario: Unique service match is resolved
- **WHEN** producer and consumer evidence normalize to one compatible typed protocol identity
- **THEN** the federated result reports the resolved relationship and a reproducible explanation

#### Scenario: Multiple service targets match
- **WHEN** more than one repository remains a valid target
- **THEN** the result is ambiguous with bounded candidates and no fabricated single target

### Requirement: Project Isolation And Read Safety
Federated queries SHALL preserve all existing repository-relative path validation, explicit `project_path` precedence, database/root binding, purpose/telemetry ownership, and no-cross-project-write guarantees. Every participating database SHALL be opened through a read-only/query-only connection for the entire call. Federation SHALL NOT run schema repair or migration, initialize/scan/watch a root, checkpoint or mutate WAL state, write telemetry/settings/purposes, persist links or caches, create a federation database, or copy authored/derived rows between roots. Any memoization SHALL be in-memory, call-scoped, bounded, and discarded when the call ends.

#### Scenario: Federated query addresses two projects
- **WHEN** one project contains a path with the same relative name as another
- **THEN** every entity remains namespaced by stable project/repository identity and no source read crosses its owning root

#### Scenario: One database is corrupt or wrong-root bound
- **WHEN** a selected database fails integrity or root verification
- **THEN** ProjectAtlas fails the whole federated call before returning graph rows and never omits or substitutes a selected project's data

#### Scenario: Federation would require a write
- **WHEN** a selected root needs migration, initialization, index refresh, cache persistence, WAL repair, or another mutation to answer the request
- **THEN** the whole call fails with that root's typed unavailable or stale reason, performs no write, and leaves every database and active project state unchanged

### Requirement: Incremental Federation Freshness
Cross-repository results SHALL capture and expose the active slot/epoch and repository revision/dirty-state evidence used for every participating project. Matches SHALL be computed from those call-captured snapshots. A bounded in-memory memo MAY be reused only within one call while the complete ordered root/identity/slot/epoch/query tuple is unchanged; it SHALL be discarded unconditionally when that call ends and sooner if any participating tuple changes. No memo SHALL be reused by a later call. Stale links SHALL be recomputed or reported stale rather than silently served as current, and no persistent federation invalidation state SHALL be required.

#### Scenario: One producer project changes an endpoint
- **WHEN** its new generation removes or changes a rendezvous identity
- **THEN** dependent consumer matches are invalidated and the next federated result reflects the new resolved/unresolved state

#### Scenario: A selected project is dirty
- **WHEN** repository bytes differ from the indexed generation
- **THEN** the result reports the freshness mismatch without repeatedly re-indexing on every poll/query

#### Scenario: Identical federation call runs again
- **WHEN** a later call supplies the same roots, snapshots, and query as a completed call
- **THEN** ProjectAtlas recomputes from the newly captured read-only snapshots and does not reuse a memo retained from the earlier call

### Requirement: Minimal Agent Surface
Federation SHALL extend the approved architecture, impact, trace, and relation surfaces through optional explicit root filters rather than adding a parallel family of cross-repository tool names. Existing single-project calls SHALL remain unchanged and SHALL never broaden to other roots implicitly.

#### Scenario: Existing relation call omits federation roots
- **WHEN** a client uses the old single-project request
- **THEN** ProjectAtlas queries only the selected project database

#### Scenario: Federated impact is requested
- **WHEN** a caller explicitly includes additional indexed roots in an approved analysis request
- **THEN** the same bounded response contract includes project-qualified paths and cross-repository evidence

### Requirement: Cross-Service Accuracy Suite
ProjectAtlas SHALL maintain positive, ambiguous, unresolved, and adversarial negative fixtures for every advertised cross-repository protocol family, including HTTP templates/builders/constants/framework prefixes, async topics/channels, gRPC, GraphQL, tRPC, packages, configuration, and database/schema links across different languages. Each family SHALL independently pass its declared precision/recall and confidence rule before it is advertised as resolved; aggregate accuracy, stronger families, or extra protocols SHALL NOT mask a failing family.

#### Scenario: Route extractor overfits a string
- **WHEN** adversarial fixtures contain route-like non-runtime strings
- **THEN** the protocol precision test detects the false edge and blocks the relevant tier/release claim
