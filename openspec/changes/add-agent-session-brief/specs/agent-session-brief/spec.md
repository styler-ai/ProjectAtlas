## ADDED Requirements

### Requirement: Agent Session Brief
The MCP server SHALL expose a read-only `atlas_session_brief` tool that returns a compact typed payload for agent startup orientation.

#### Scenario: Healthy project brief
- **WHEN** an indexed project is selected and the agent calls `atlas_session_brief` with a task query
- **THEN** the response SHALL include selected project identity, overview counts, bounded relevant folders/files, health blocker summary, and recommended next calls.

#### Scenario: Missing index brief
- **WHEN** the selected project index is missing and the agent calls `atlas_session_brief`
- **THEN** the response SHALL not create a database and SHALL recommend `atlas_scan` or explicit project selection before ProjectAtlas reads.

### Requirement: Brief Output Contract
The session brief payload SHALL use typed serializable fields for status, recommendation kind, path scope, and blocker severity instead of prose-only diagnostics.

#### Scenario: Machine-readable recommendations
- **WHEN** a brief recommends a next action
- **THEN** the recommendation SHALL include a stable kind, a concise reason, and enough arguments for the agent to call the next ProjectAtlas tool safely.

### Requirement: Brief Scope Boundaries
The session brief SHALL compose existing ProjectAtlas index, ranking, settings, and health data without scanning or opening source files directly.

#### Scenario: Read-only operation
- **WHEN** an agent calls `atlas_session_brief`
- **THEN** ProjectAtlas SHALL not run a scan, create an index, or read arbitrary source content outside already indexed summaries and metadata.
