## ADDED Requirements

### Requirement: One-Call First-Run ProjectAtlas Init
`projectatlas init` SHALL provide a safe one-call first-run bootstrap that prepares a repository for high-quality ProjectAtlas use through the ProjectAtlas plugin/runtime mechanism.

#### Scenario: New repository bootstrap
- **WHEN** a user runs `projectatlas init` in a repository without `.projectatlas/`
- **THEN** ProjectAtlas SHALL create `.projectatlas/`
- **AND** it SHALL create default `config.toml`
- **AND** it SHALL create the non-source TOON scaffold
- **AND** it SHALL create or initialize `.projectatlas/projectatlas.db`
- **AND** it SHALL create or refresh generated host MCP configs for Codex/generic MCP, Claude Code, and OpenCode
- **AND** it SHALL run the deep scan/index by default
- **AND** it SHALL return a setup report with created/existing/verified/skipped phase statuses.

#### Scenario: Existing ProjectAtlas surface is extended safely
- **WHEN** a user runs `projectatlas init` in a repository that already has `.projectatlas/config.toml`
- **THEN** ProjectAtlas SHALL preserve the existing config content
- **AND** it SHALL verify or create missing required files
- **AND** it SHALL NOT overwrite approved purposes or existing DB content unless an explicit future migration requires it.

#### Scenario: Init is idempotent
- **WHEN** a user runs `projectatlas init` twice in the same repository
- **THEN** the second run SHALL report existing or verified resources
- **AND** it SHALL NOT duplicate, delete, or rewrite user-owned state unnecessarily.

### Requirement: Init Scan Control
The first-run init flow SHALL run scan/index by default and provide explicit controls for scripts and slow repositories.

#### Scenario: Default init scans
- **WHEN** `projectatlas init` runs without scan-disabling options
- **THEN** it SHALL run the existing scan/index pipeline
- **AND** it SHALL report file, folder, purpose, symbol, and warning counts where available.

#### Scenario: Scan can be skipped
- **WHEN** `projectatlas init --no-scan` runs
- **THEN** ProjectAtlas SHALL create/verify the project surface
- **AND** it SHALL NOT run the scan pipeline
- **AND** the setup report SHALL mark scan status as `skipped`.

#### Scenario: Existing DB can be refreshed
- **WHEN** `projectatlas init --force-rescan` runs in a repository with an existing DB
- **THEN** ProjectAtlas SHALL run the scan/index pipeline again
- **AND** it SHALL report the refreshed scan status.

### Requirement: Purpose Curation Handoff
The first-run init flow SHALL prepare purpose curation for agent harnesses without making the Rust binary directly spawn subagents.

#### Scenario: Agent harness receives a reliable low-cost purpose handoff
- **WHEN** `projectatlas init` completes in an agent/plugin harness
- **THEN** the report SHALL include guidance to delegate initial folder and file purpose creation/correction to bounded isolated subagents at the lowest reliable reasoning and cost tier the host supports
- **AND** a fixed reliable subagent tier SHALL still delegate at that tier without a selector; reasoning selection is optional, and only absence of bounded isolated subagent execution SHALL select the main-agent fallback
- **AND** it SHALL include purpose queue counts or batches that the harness can assign
- **AND** it SHALL instruct the harness to apply purposes through ProjectAtlas purpose APIs.

#### Scenario: CLI outside an agent harness remains useful
- **WHEN** a human runs `projectatlas init` outside an agent harness
- **THEN** ProjectAtlas SHALL print the purpose handoff and next commands
- **AND** it SHALL NOT fail merely because no subagent mechanism is available.

#### Scenario: Approved API path
- **WHEN** an agent or subagent applies purposes during the init handoff
- **THEN** purposes SHALL be written through `atlas_purpose_review` or `projectatlas purpose review --apply`
- **AND** the resulting purposes SHALL be marked agent-reviewed.

### Requirement: MCP Init Parity
`atlas_init` SHALL expose the same first-run bootstrap behavior and structured setup report as `projectatlas init`.

#### Scenario: MCP init accepts explicit project path
- **WHEN** an agent calls `atlas_init` with `project_path`
- **THEN** ProjectAtlas SHALL initialize that explicit project root
- **AND** it SHALL NOT silently route to a different nearest project root
- **AND** it SHALL NOT change the active default MCP project used by later calls that omit `project_path`.

#### Scenario: MCP init supports scan controls
- **WHEN** an agent calls `atlas_init` with scan disabled or forced rescan parameters
- **THEN** the MCP result SHALL reflect the same behavior as the matching CLI flags.

#### Scenario: Missing-index path-sensitive behavior
- **WHEN** `atlas_init` is called for a root that has no `.projectatlas/projectatlas.db`
- **THEN** ProjectAtlas SHALL create or initialize the DB for that root
- **AND** subsequent ProjectAtlas calls for that root SHALL use the newly initialized local DB.

#### Scenario: Generated host config parity
- **WHEN** `projectatlas init` or `atlas_init` completes successfully
- **THEN** the setup report SHALL include generated host config statuses
- **AND** `.projectatlas/projectatlas.mcp.json`, `.projectatlas/projectatlas.claude.mcp.json`, and `.projectatlas/projectatlas.opencode.json` SHALL point at the selected project DB/config and current runtime version.

### Requirement: Init Does Not Mutate Wrong Roots
The init flow SHALL avoid implicit cross-root mutation.

#### Scenario: Wrong-root protection
- **WHEN** a user or agent targets a path outside the intended repository
- **THEN** ProjectAtlas SHALL require an explicit root/path parameter or current working directory
- **AND** it SHALL report the root it is initializing before or as part of mutation.

#### Scenario: Nearest project routing is not implicit init behavior
- **WHEN** nearest-project discovery is enabled elsewhere
- **THEN** `projectatlas init` and `atlas_init` SHALL NOT initialize a nearest ancestor/sibling root unless that root was explicitly selected as the init target.

### Requirement: OpenSpec-Inspired Product Shape Without Runtime Dependency
ProjectAtlas SHALL take product inspiration from OpenSpec's one-command initialization while remaining Rust/plugin-native.

#### Scenario: No npm/OpenSpec runtime dependency
- **WHEN** first-run ProjectAtlas init is implemented
- **THEN** it SHALL NOT require npm
- **AND** it SHALL NOT require OpenSpec to be installed
- **AND** it SHALL run through the ProjectAtlas binary/plugin runtime.

#### Scenario: Phase-level report
- **WHEN** init completes or partially fails
- **THEN** the report SHALL include phase statuses for surface, config, DB, scan, purpose handoff, and next steps
- **AND** failures SHALL include enough detail to resume safely.
