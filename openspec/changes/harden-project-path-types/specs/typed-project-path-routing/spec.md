## ADDED Requirements

### Requirement: Typed Routing Boundaries
ProjectAtlas SHALL represent selected project roots, indexed project roots, absolute filesystem paths, and repository-relative keys with distinct Rust types or equivalent strongly typed boundaries in MCP/runtime routing code.

#### Scenario: Explicit project isolation
- **WHEN** a caller supplies `project_path` for one project and an absolute file path in another project
- **THEN** routing helpers SHALL reject the call instead of nearest-routing to the other project.

#### Scenario: Selected project absolute path
- **WHEN** a caller supplies an absolute path inside the selected project
- **THEN** routing helpers SHALL normalize it to a repository-relative key without requiring nearest-project routing.

#### Scenario: Nested indexed child under active parent
- **WHEN** nearest-project routing is enabled and an absolute path is inside a nested child project under the active selected root
- **THEN** routing helpers SHALL select the nested child DB when it is the nearest valid indexed ancestor.

### Requirement: Nearest Indexed Project Validation
Nearest-project routing SHALL only select an ancestor that contains `.projectatlas/projectatlas.db` whose stored project root matches that ancestor.

#### Scenario: Partial ProjectAtlas folder
- **WHEN** an absolute path is inside a folder containing `.projectatlas/` but no `projectatlas.db`
- **THEN** ProjectAtlas SHALL reject the call with filesystem-tool guidance and SHALL NOT create a database.

#### Scenario: Invalid candidate database
- **WHEN** an absolute path is inside a folder containing an empty, corrupt, or schema-invalid `.projectatlas/projectatlas.db`
- **THEN** nearest-project validation SHALL reject that candidate with filesystem-tool guidance and SHALL NOT initialize, migrate, repair, or create WAL/SHM files for that candidate.

#### Scenario: Config or DB root mismatch
- **WHEN** an ancestor DB or config records a different project root than the candidate root
- **THEN** nearest-project routing SHALL reject that candidate and SHALL NOT route through it.

### Requirement: Cross-Platform Path Tests
ProjectAtlas SHALL include automated tests for routing and normalization edge cases across Windows-style and Unix-style path inputs.

#### Scenario: Windows-style path coverage
- **WHEN** tests exercise drive-prefixed paths, backslash separators, and extended-prefix diagnostics
- **THEN** path normalization and routing behavior SHALL remain deterministic and safe.

#### Scenario: Unix-style path coverage
- **WHEN** tests exercise absolute slash paths, parent traversal attempts, and nested roots
- **THEN** path normalization and routing behavior SHALL remain deterministic and safe.
