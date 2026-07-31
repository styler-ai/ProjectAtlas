## ADDED Requirements

### Requirement: Implicit bare roots are classified before database access
ProjectAtlas SHALL validate the source-root role before opening an implicitly selected conventional database.

#### Scenario: Bare root has a future-schema database
- **WHEN** a CLI command uses the implicit default database from a bare/common Git root whose database schema is newer than the runtime
- **THEN** ProjectAtlas returns typed `worktree_required` without opening or changing that database

#### Scenario: Bare root has absent or compatible state
- **WHEN** a CLI command uses the implicit default database from a bare/common Git root whose database is absent, current, older-supported, or malformed
- **THEN** ProjectAtlas returns the same typed `worktree_required` source-selection contract before database admission

### Requirement: Explicit database selection retains database diagnostics
ProjectAtlas SHALL preserve compatibility diagnostics when a caller explicitly selects a database.

#### Scenario: Explicit future-schema database
- **WHEN** a caller passes `--db` for a future-schema database located beneath a bare/common Git root
- **THEN** ProjectAtlas returns the truthful unsupported-schema error instead of replacing it with implicit worktree guidance

### Requirement: Bare-root refusal is non-mutating
ProjectAtlas MUST NOT mutate unselected repository or database state while producing worktree guidance.

#### Scenario: Preserved database and SQLite sidecars
- **WHEN** implicit bare-root classification refuses a command
- **THEN** database, existing WAL/SHM, config, backup, purpose, and telemetry state remain unchanged and no SQLite sidecar is created

### Requirement: Existing project selection remains isolated
ProjectAtlas SHALL retain existing checked-out worktree, explicit config, and explicit MCP project-selection behavior.

#### Scenario: Checked-out linked worktree
- **WHEN** the same command runs from an initialized checked-out worktree
- **THEN** ProjectAtlas uses only that worktree's project-local database and does not select a sibling

#### Scenario: Explicit config or MCP database
- **WHEN** a caller provides an explicit ProjectAtlas config, MCP `project_path`, or generated absolute database/config pair
- **THEN** ProjectAtlas preserves that explicit selection and its existing wrong-root and missing-index diagnostics
