## ADDED Requirements

### Requirement: Legacy root recovery requires explicit authority
The CLI and MCP root-transition API SHALL provide explicit `adopt-legacy` recovery for an intact schema-19 database at the selected canonical root's conventional project-local location. The operator's explicit selection SHALL supply native root authority; lossy predecessor text SHALL only reject contradictions and SHALL NOT authorize ordinary opens. The existing migration sequence, native root publication, and project-identity validation SHALL complete in one transaction without changing authored purposes, telemetry, worktree authority, or source files.

#### Scenario: Ordinary legacy access refuses safely
- **WHEN** a Unix schema-19 database lacks native root identity and an ordinary init, open, MCP configuration, bind, move, or detach is requested
- **THEN** the operation refuses without implicit adoption or database mutation

#### Scenario: Explicit recovery and installer retry
- **WHEN** an operator explicitly adopts the matching existing root through a checksum-verified candidate and retries installation
- **THEN** migration publishes the native root atomically, preserves project identity and authored state, and candidate CLI/MCP/host bindings converge

#### Scenario: Invalid or failed adoption
- **WHEN** adoption addresses a missing, current, malformed, already-native, foreign-root, or raced predecessor, or migration fails
- **THEN** it refuses or rolls back without partial schema, root identity, or generated configuration publication, and an intact predecessor remains usable for a corrected retry

#### Scenario: Explicit recovery preserves non-UTF-8 root bytes
- **WHEN** a Unix predecessor root contains bytes that cannot be represented as UTF-8
- **THEN** explicit adoption preserves those bytes in native root identity, uses the predecessor's lossy text only to reject contradictions, and removes unrepresentable compatibility metadata

#### Scenario: Database pathname is replaced after opening
- **WHEN** the conventional database pathname stops identifying the database opened for adoption
- **THEN** adoption refuses before migration or rolls back before commit instead of reporting success for the displaced database
