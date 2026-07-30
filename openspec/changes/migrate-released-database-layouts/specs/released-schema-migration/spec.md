## ADDED Requirements

### Requirement: Released schema-8 layouts are admitted semantically
ProjectAtlas SHALL admit a schema-8 database accepted by released v0.3.26 when its named tables, columns, declared types, nullability, defaults, keys, foreign keys, and required indexes are semantically equivalent to the released contract, regardless of compatible physical table-column order.

#### Scenario: Earlier database evolved through released migrations
- **WHEN** `init` or `scan` opens a schema-8 database created by v0.3.11 and accepted by v0.3.26 whose telemetry columns were appended after `created_at`
- **THEN** ProjectAtlas admits the database to the supported migration path instead of rejecting its physical column ordinals

#### Scenario: Fresh v0.3.26 database
- **WHEN** `init` or `scan` opens a fresh schema-8 database created by released v0.3.26
- **THEN** ProjectAtlas admits the existing canonical predecessor layout without changing its compatibility behavior

#### Scenario: Incompatible predecessor drift
- **WHEN** a schema-8 database has a missing, renamed, extra, or semantically changed required column, key, foreign key, or index
- **THEN** ProjectAtlas fails closed with a bounded schema diagnostic and performs no migration write

### Requirement: Migration is atomic and preserves durable state
On writable `init` or `scan`, ProjectAtlas SHALL automatically detect any admitted supported predecessor represented by the centralized migration inventory and migrate it to the current schema in the existing ordered transaction. The released schema-8 layouts SHALL use that same path rather than a version-specific CLI conversion. Migration SHALL preserve approved purposes, health resolutions, telemetry, project identity, root binding, and durable source rows. Successful migration SHALL invalidate only predecessor publication markers incompatible with the current derived-index contract; failed migration SHALL leave the original last-complete publication state unchanged.

#### Scenario: Any admitted migration-inventory predecessor
- **WHEN** `init` or `scan` opens a database whose version and semantic contract are admitted by the database-owned migration inventory
- **THEN** ProjectAtlas applies every ordered transition to the current schema automatically before continuing, without CLI-owned version logic or manual state replay

#### Scenario: Successful released-layout migration
- **WHEN** an admitted evolved schema-8 database contains approved purposes, health resolutions, telemetry rows, project identity, and a complete predecessor source generation
- **THEN** migration reaches schema 16 with those durable rows and identities unchanged, removes the incompatible predecessor publication markers, accepts a new complete publication, and passes `PRAGMA quick_check`, `PRAGMA foreign_key_check`, reopen, and root verification

#### Scenario: Migration failure after admission
- **WHEN** an injected failure occurs after semantic admission but before the migration transaction commits
- **THEN** reopening observes the original schema version, durable rows, root binding, and complete publication state with no partial schema or generation

#### Scenario: Independent project databases
- **WHEN** two project-local databases with different roots and identities are migrated
- **THEN** each migration remains bound to its own root and identity and no authored or derived row crosses between projects

### Requirement: Read-only compatibility remains non-mutating
ProjectAtlas SHALL distinguish an admitted upgrade-required database from a current database during read-only preflight without applying DDL, advancing metadata, creating sidecars beyond SQLite read behavior, or publishing source state.

#### Scenario: Read-only preflight of an evolved released database
- **WHEN** read-only preflight inspects a semantically compatible evolved schema-8 database
- **THEN** it reports upgrade-required state and leaves the database contents and schema version unchanged

#### Scenario: Wrong-root read-only preflight
- **WHEN** a compatible schema-8 database is addressed with a different expected project root
- **THEN** ProjectAtlas returns the typed root mismatch and does not migrate or rebind the database
