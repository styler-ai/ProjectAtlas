## ADDED Requirements

### Requirement: CLI-to-MCP Parity Inventory
ProjectAtlas SHALL maintain a central reviewed inventory that maps official safe CLI command families to MCP `atlas_*` tools or to explicit reviewed exceptions.

#### Scenario: Safe CLI family lacks MCP coverage
- **WHEN** a safe ProjectAtlas CLI command family is added or changed
- **THEN** the parity inventory SHALL require a corresponding MCP tool unless the family is listed as a reviewed exception with a reason.

#### Scenario: Tests detect drift
- **WHEN** MCP parity tests run
- **THEN** missing MCP command-family coverage SHALL fail unless the gap is listed as a reviewed exception.

### Requirement: Safe Admin and Reporting MCP Tools
The MCP server SHALL expose safe admin and reporting parity tools for `init`, root diagnostics/binding, effective config, manual ignore policy, lint, runtime identity, MCP config generation, and compatibility map export.

#### Scenario: Project initialization
- **WHEN** an agent can reach the MCP server and needs first-time ProjectAtlas setup
- **THEN** `atlas_init` SHALL provide the safe MCP equivalent of `projectatlas init`.

#### Scenario: Root diagnostics and binding
- **WHEN** an agent needs root identity, verification, or binding
- **THEN** `atlas_root` with `verify` when needed and `atlas_root_set` SHALL provide MCP equivalents for `projectatlas root show`, `projectatlas root verify`, and `projectatlas root set`.

#### Scenario: Config diagnostics
- **WHEN** an agent needs effective scan, purpose, or exclusion policy
- **THEN** `atlas_config` SHALL return the normalized effective config equivalent to `projectatlas config --print`.

#### Scenario: Manual ignore policy
- **WHEN** an agent needs to inspect or mutate the ProjectAtlas manual ignore layer
- **THEN** `atlas_ignore_list`, `atlas_ignore_init_gitignore`, `atlas_ignore_add`, and `atlas_ignore_remove` SHALL cover the matching `projectatlas ignore` subcommands.

#### Scenario: Lint and runtime diagnostics
- **WHEN** an agent needs lint or runtime identity information
- **THEN** `atlas_lint` and `atlas_runtime_info` SHALL provide MCP equivalents for `projectatlas lint` and `projectatlas runtime-info`.

#### Scenario: MCP config and compatibility map
- **WHEN** an agent needs generated harness config or an explicit legacy TOON map export
- **THEN** `atlas_mcp_config` and `atlas_map` SHALL provide MCP equivalents for `projectatlas mcp-config` and `projectatlas map`.

### Requirement: Project Path Isolation
Root-sensitive parity tools SHALL accept `project_path` and SHALL bind all file, config, DB, and generated-output behavior to that selected project.

#### Scenario: Shared MCP server
- **WHEN** one MCP server is shared across repositories
- **THEN** a parity tool call with `project_path` SHALL operate on that project without relying on active process-global project state.

#### Scenario: Wrong-root request
- **WHEN** a parity tool receives a path that does not belong to the selected project
- **THEN** it SHALL return a clear error instead of mutating or reporting against an unintended repository.

### Requirement: Reviewed MCP Exceptions
ProjectAtlas SHALL document true CLI-only exceptions for command families that are unsafe or unsuitable to expose as ordinary MCP calls.

#### Scenario: MCP server startup
- **WHEN** parity is evaluated for `projectatlas mcp`
- **THEN** it SHALL be treated as a reviewed exception because it starts the MCP server process and must not recursively start a server inside an MCP request.

#### Scenario: Continuous watch
- **WHEN** parity is evaluated for continuous `projectatlas watch`
- **THEN** it SHALL be treated as a reviewed exception until an explicit MCP lifecycle contract exists, and agents SHALL use `atlas_watch_once` or `atlas_watch_status` for safe MCP workflows.

#### Scenario: Terminal TUI
- **WHEN** parity is evaluated for `projectatlas token --view tui`
- **THEN** it SHALL be treated as a reviewed exception because terminal UI rendering is not an agent payload, and agents SHALL use `atlas_token_report`.

### Requirement: Documentation and Plugin Guidance
ProjectAtlas agent documentation and plugin skill guidance SHALL prefer MCP parity tools when available and list CLI fallbacks only for unavailable tools or reviewed exceptions.

#### Scenario: Agent startup snippet
- **WHEN** an agent reads `AGENTS.md`, templates, docs, or the plugin skill
- **THEN** the guidance SHALL name the MCP parity tools for normal ProjectAtlas command families and SHALL identify MCP startup, continuous watch, and terminal TUI as reviewed CLI-only exceptions.
