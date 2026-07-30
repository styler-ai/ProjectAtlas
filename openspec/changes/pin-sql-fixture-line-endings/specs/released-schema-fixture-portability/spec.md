## ADDED Requirements

### Requirement: Embedded released-schema fixtures are platform independent

The repository MUST check out SQL fixtures with LF line endings on every supported platform so compile-time embedded DDL has deterministic bytes.

#### Scenario: Windows checkout embeds a released schema fixture

- **WHEN** a released-schema SQL fixture is checked out and embedded during a Windows build
- **THEN** its line endings match the canonical LF bytes used by the schema-drift tests

### Requirement: Portability does not weaken schema validation

The fixture portability correction MUST preserve production schema DDL, migration behavior, and strict rejection of incompatible released-schema drift.

#### Scenario: A captured predecessor schema is changed semantically

- **WHEN** the drift test removes, renames, adds, or changes a captured schema object
- **THEN** preflight rejects the lookalike without mutating the database

### Requirement: Database release gate covers the checkout contract

Release verification MUST confirm SQL checkout normalization and run the owning drift test plus the complete database test suite on Windows.

#### Scenario: The database release gate runs

- **WHEN** the v0.4.1 database gate evaluates the portability correction
- **THEN** LF checkout state, focused drift coverage, and all database tests pass
