## ADDED Requirements

### Requirement: MCP payload fixture uses one project database

The real MCP payload fixture SHALL use the canonical project-local database for
setup, scanning, purpose seeding, explicit initialization, and implicit navigation.
It SHALL compare initialized and selected database identity against the expected
fixture database before accepting successful payloads. Existing payload assertions,
process cleanup, deadlines, and product freshness behavior SHALL remain intact.

#### Scenario: Initialization and navigation share the seeded index

- **WHEN** the real CLI setup and MCP phases execute the fixture
- **THEN** the initialization and selected-runtime reports identify the expected project-local database
- **AND** the existing purpose, graph, navigation, and compatibility assertions pass

#### Scenario: The fixture selects a different database

- **WHEN** explicit initialization and implicit navigation identify different databases
- **THEN** the identity assertion fails with the actual and expected binding
- **AND** no fallback database or implicit refresh can satisfy acceptance

#### Scenario: The selected index is unavailable or freshness is refused

- **WHEN** the coherent fixture cannot provide the required successful payload
- **THEN** the existing assertion remains a failure requiring diagnosis
- **AND** no increased deadline, retry-based acceptance, or unproven observer change hides it
