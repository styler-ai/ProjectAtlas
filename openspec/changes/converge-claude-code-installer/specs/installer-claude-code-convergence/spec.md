## ADDED Requirements

### Requirement: Claude Code Generated Config Verification
The ProjectAtlas installer SHALL verify `.projectatlas/projectatlas.claude.mcp.json` after generating it from the verified runtime.

#### Scenario: Generated config points at verified runtime
- **WHEN** the installer completes runtime verification and writes the Claude Code MCP config
- **THEN** the config SHALL contain an absolute `mcpServers.projectatlas.command` path equivalent to the verified ProjectAtlas runtime.

#### Scenario: Generated config carries the version guard
- **WHEN** the installer writes the Claude Code MCP config for a known runtime version
- **THEN** the config args SHALL include `--require-version <runtime-version>` before `mcp`.

#### Scenario: Generated config binds the selected project
- **WHEN** the installer writes the Claude Code MCP config
- **THEN** the config args SHALL include the selected project DB path and the effective project config path when one exists.

#### Scenario: Generated config fails validation
- **WHEN** the generated Claude Code config does not match the verified runtime, version, DB, config, or `mcp` command contract
- **THEN** the installer SHALL fail with a clear field-specific error before reporting Claude Code convergence.

### Requirement: Claude Code Non-Mutation Boundary
The ProjectAtlas installer SHALL NOT mutate Claude Code user settings, plugin caches, or unrelated Claude state unless a future official ProjectAtlas Claude integration surface is positively identified.

#### Scenario: No official repairable Claude state exists
- **WHEN** no official Claude Code ProjectAtlas plugin/cache surface is available
- **THEN** the installer SHALL limit convergence to generated config verification and documentation.

#### Scenario: Running Claude Code may cache old instructions
- **WHEN** generated config verification succeeds
- **THEN** the installer SHALL tell users to restart Claude Code if an older session cached previous instructions.
