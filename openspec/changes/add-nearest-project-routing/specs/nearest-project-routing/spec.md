## ADDED Requirements

### Requirement: Opt-In Nearest Project Routing
ProjectAtlas MCP SHALL only route absolute addressed paths to a nearest indexed project when nearest-project routing is explicitly enabled at startup or on the call.

#### Scenario: Default-off absolute path outside selected project
- **WHEN** an MCP call addresses an absolute file or folder outside the selected project
- **AND** nearest-project routing is not enabled
- **THEN** ProjectAtlas SHALL reject the ProjectAtlas read and return guidance to use normal filesystem tools or an explicit project selection.

#### Scenario: Startup-enabled routing
- **WHEN** the MCP server starts with nearest-project routing enabled
- **AND** an addressed absolute path belongs to another valid indexed ProjectAtlas project
- **THEN** ProjectAtlas SHALL route that call to the nearest indexed project.

#### Scenario: Per-call override
- **WHEN** an MCP call supplies `nearest_project`
- **THEN** that value SHALL override the server startup default for that call.

### Requirement: Explicit Project Path Isolation
Explicit `project_path` SHALL take precedence over nearest-project routing.

#### Scenario: Explicit project path plus nearest project
- **WHEN** an MCP call supplies `project_path` for project A and an addressed absolute path inside project B
- **AND** nearest-project routing is enabled
- **THEN** ProjectAtlas SHALL keep the call isolated to project A and reject the project B path.

### Requirement: Read-Only Candidate Discovery
Nearest-project discovery SHALL inspect candidate databases without creating, initializing, or mutating them.

#### Scenario: Missing or partial index
- **WHEN** an addressed path has no ancestor with a valid `.projectatlas/projectatlas.db`
- **THEN** ProjectAtlas SHALL NOT create `.projectatlas` or a DB and SHALL return normal filesystem guidance.

#### Scenario: Invalid or WAL-mode candidate
- **WHEN** discovery probes a candidate DB
- **THEN** the probe SHALL be read-only and SHALL NOT create WAL/SHM sidecar files.

### Requirement: Ambiguity Rejection
ProjectAtlas SHALL reject ambiguous nearest-project routes instead of guessing.

#### Scenario: Config root mismatch
- **WHEN** a candidate DB or config root does not match the candidate project root
- **THEN** routing SHALL fail with a clear mismatch error.

#### Scenario: Symlink or junction ambiguity
- **WHEN** symlink or junction resolution creates multiple plausible indexed roots
- **THEN** routing SHALL fail and direct the agent to use explicit project selection or normal filesystem tools.

### Requirement: Selected Project Audit
Cross-project routed read responses SHALL identify the selected project before the normal payload.

#### Scenario: Rerouted read response
- **WHEN** a read tool routes to a project other than the active/default project
- **THEN** the response SHALL include selected root, DB, config, and status metadata.
