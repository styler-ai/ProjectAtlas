## ADDED Requirements

### Requirement: MCP Capability Settings
The MCP server SHALL expose a typed capability/settings payload that reports runtime identity, selected project identity, and route-affecting startup policy.

#### Scenario: Nearest-project policy visible
- **WHEN** the MCP server starts with nearest-project routing enabled or disabled
- **THEN** the capability/settings payload SHALL report the effective nearest-project startup policy.

#### Scenario: Project identity visible
- **WHEN** an agent requests MCP capability/settings
- **THEN** the response SHALL include selected project root, DB path, config path when present, and runtime version.

### Requirement: Machine-Readable Policy
Capability/settings output SHALL use typed fields and stable enum values for policies that agents need to inspect.

#### Scenario: Harness validates policy before reading
- **WHEN** a harness parses the capability/settings payload
- **THEN** it SHALL be able to determine whether absolute path calls are selected-project-only or may route to nearest indexed projects without parsing prose.

#### Scenario: Missing index represented
- **WHEN** the selected ProjectAtlas database is missing or unavailable
- **THEN** the capability/settings payload SHALL represent that state with typed status fields and SHALL NOT create an index.

#### Scenario: Wrong-root policy visible
- **WHEN** a server has route-affecting startup policy such as nearest-project routing
- **THEN** the capability/settings payload SHALL expose the policy before the harness makes summary, slice, search, or file-ranking calls.

#### Scenario: Read-only settings inspection
- **WHEN** an agent requests capability/settings
- **THEN** ProjectAtlas SHALL NOT scan, repair config, mutate selected project state, or otherwise change repository state.

### Requirement: No Secret Exposure
Capability/settings output SHALL avoid exposing secrets, arbitrary environment variables, or host-private data beyond paths already used for ProjectAtlas runtime configuration.

#### Scenario: Settings request in a configured project
- **WHEN** the server returns capability/settings
- **THEN** the response SHALL not include token values, full environment dumps, or unrelated user profile data.
