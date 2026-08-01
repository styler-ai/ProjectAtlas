## ADDED Requirements

### Requirement: Schema compatibility is classified before writable access
ProjectAtlas SHALL inspect an existing database through the database-owned read-only preflight before opening it writable, enabling or changing WAL, configuring write pragmas, executing DDL or repair, creating indexes, or applying migrations. Every normal CLI and MCP path that can acquire a writable store SHALL use this shared open boundary.

#### Scenario: Current database opens normally
- **WHEN** the selected project database has the exact schema version and valid ownership supported by the runtime
- **THEN** preflight admits it and the existing writable-open behavior continues without schema-specific adapter logic

#### Scenario: Supported predecessor is upgraded through the existing owner
- **WHEN** preflight classifies a valid released predecessor represented by the centralized migration inventory
- **THEN** ProjectAtlas SHALL defer establishing the write profile and applying the existing ordered migrations to the subsequent database-owned writable transaction

#### Scenario: Newer database is refused before writable open
- **WHEN** the durable schema version is newer than the runtime supports
- **THEN** ProjectAtlas returns a typed version mismatch before any writable connection, WAL change, DDL, repair, index creation, or migration

#### Scenario: Existing database has missing or malformed version metadata
- **WHEN** read-only preflight cannot classify the durable schema version safely
- **THEN** ProjectAtlas fails closed without creating, repairing, migrating, or otherwise mutating the database

#### Scenario: Database is genuinely absent
- **WHEN** preflight proves the selected database does not exist
- **THEN** ProjectAtlas SHALL classify it as fresh and SHALL create it only through the subsequent existing fresh-database path

#### Scenario: Selected project root does not own the database
- **WHEN** preflight finds a valid schema whose durable project identity belongs to another root
- **THEN** ProjectAtlas returns the existing typed project mismatch and does not migrate, rebind, create, or mutate either project's index

### Requirement: Newer-schema refusal preserves complete durable state
ProjectAtlas SHALL leave the main database, WAL contents, schema objects, schema metadata, project identity, authored state, telemetry, and derived rows unchanged after refusing a newer schema. The refusal SHALL remain non-mutating when the newer schema and committed fixture writes are present in an active uncheckpointed WAL retained by another live connection.

#### Scenario: Newer schema is present in an active WAL
- **WHEN** a live SQLite connection retains a non-empty uncheckpointed WAL containing a valid fixture stamped one version newer than the runtime
- **THEN** a normal writable ProjectAtlas open returns the version mismatch without checkpointing or changing the main database bytes, WAL bytes, or sidecar inventory

#### Scenario: Newer database contains durable and derived rows
- **WHEN** the incompatible database contains project identity, authored purposes, health resolutions, telemetry, metadata, and representative derived rows
- **THEN** refusal leaves every captured row and table, index, view, and trigger definition unchanged

#### Scenario: Concurrent owner remains usable
- **WHEN** the live fixture connection remains open while ProjectAtlas refuses the incompatible database
- **THEN** that owner can still read its unchanged committed state and ProjectAtlas does not acquire a writer transaction or alter checkpoint state

### Requirement: CLI and MCP share a privacy-safe mismatch contract
ProjectAtlas SHALL serialize incompatible schema errors from the shared typed database error as `schema_version_mismatch` in both CLI agent output and MCP tool errors. The payload SHALL include `found_schema_version`, `supported_schema_version`, and `runtime_version`, and SHALL NOT include database paths, project roots, SQL, metadata values, or authored content.

#### Scenario: CLI agent output encounters a newer schema
- **WHEN** a JSON or TOON CLI command opens a newer project database
- **THEN** it returns `schema_version_mismatch` with the exact found, supported, and runtime versions and no private database content

#### Scenario: MCP tool encounters a newer schema
- **WHEN** a real stdio MCP session initializes and a tool addresses a newer project database through explicit `project_path`
- **THEN** the tool error returns the same kind and version fields as the CLI while the MCP session remains protocol-correct

#### Scenario: Current-only adapter encounters an admitted predecessor
- **WHEN** a CLI or MCP read requires the current schema but the database has a released predecessor represented by the centralized migration inventory
- **THEN** ProjectAtlas returns a content-free `schema_migration_required` handoff with the found version, supported version, remaining migration steps, and the existing safe `init` migration action through the same CLI `--db`/`--config` selection or MCP server/database binding instead of calling the schema unsupported or creating the default database

#### Scenario: MCP addresses a missing index
- **WHEN** an MCP tool addresses a project root with no project-local database
- **THEN** it returns the existing typed initialization handoff and does not create an index implicitly

#### Scenario: MCP addresses another project's index
- **WHEN** an MCP tool's explicit `project_path` would select an index owned by another root
- **THEN** it returns the existing typed project mismatch rather than a schema mismatch and performs no implicit migration, rebind, or mutation

### Requirement: Packaged adapters preserve incompatible-schema refusal
The official packaged ProjectAtlas runtime SHALL preserve the same preflight ordering and typed refusal as workspace code. Release verification SHALL invoke the isolated packaged executable by absolute path, verify its exact runtime version, and exercise both a representative CLI command and real stdio MCP tool call against a newer-schema active-WAL fixture.

#### Scenario: Packaged CLI refuses a newer database
- **WHEN** the exact packaged runtime executes a representative CLI command against the fixture
- **THEN** it returns the shared typed mismatch and leaves the complete database/WAL snapshot unchanged

#### Scenario: Packaged MCP refuses a newer database
- **WHEN** the exact packaged runtime serves stdio MCP and a tool addresses the fixture
- **THEN** it returns the shared typed mismatch without adapter fallback, implicit initialization, or database mutation

### Requirement: Incompatible preflight remains bounded by schema metadata
Newer-schema classification SHALL remain a bounded schema/metadata operation whose CPU, memory, SQLite reads, and elapsed work do not scale with authored or derived row cardinality. It SHALL perform no database or WAL writes.

#### Scenario: Small and representative large databases are refused
- **WHEN** focused profiling compares the same newer schema with small and representative large row populations
- **THEN** observed work remains bounded by schema metadata, performs no table-sized scan or row decode, and produces no database/WAL growth
