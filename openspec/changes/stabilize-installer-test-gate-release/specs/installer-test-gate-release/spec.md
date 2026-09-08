## ADDED Requirements

### Requirement: Windows installer test gates preserve release semantics

The Windows installer SHALL use an exact-file existence predicate for its
test-only state-publication, lock-discovery, and lock-acquired pause gates. A
transient PowerShell provider lookup failure during gate removal SHALL NOT abort
the fixture. Ready markers, hold/release ordering, polling intervals, production
lock behavior, and lifecycle assertions SHALL remain unchanged.

#### Scenario: A held gate releases after parent coordination

- **WHEN** a fixture starts an installer with one of the three test gates present
- **THEN** the installer writes its ready marker and remains held until release
- **AND** removal of the gate lets the installer finish its existing lifecycle work

#### Scenario: No test gate is requested

- **WHEN** the matching test environment variable is absent or blank
- **THEN** the helper returns without creating files or pausing normal installation

#### Scenario: Opposite migrations preserve ownership through release

- **WHEN** real installer children execute opposite migration, interruption, and repair races
- **THEN** their existing managed-pair, unrelated-state, and shared-deadline assertions pass without increased budgets or suppressed failures
