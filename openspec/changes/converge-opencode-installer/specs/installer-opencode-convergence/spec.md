## ADDED Requirements

### Requirement: OpenCode Generated Config Verification
The ProjectAtlas installer SHALL verify `.projectatlas/projectatlas.opencode.json` after generating it from the verified runtime.

#### Scenario: Generated config points at verified runtime
- **WHEN** the installer completes runtime verification and writes the OpenCode MCP config
- **THEN** the config SHALL contain an absolute `mcp.projectatlas.command` path equivalent to the verified ProjectAtlas runtime.

#### Scenario: Generated config carries the version guard
- **WHEN** the installer writes the OpenCode MCP config for a known runtime version
- **THEN** the config args SHALL include `--require-version <runtime-version>` before `mcp`.

#### Scenario: Generated config binds the selected project
- **WHEN** the installer writes the OpenCode MCP config
- **THEN** the config args SHALL include the selected project DB path, the effective config path when one exists, and the selected project root as `cwd`.

#### Scenario: OpenCode local MCP flags are present
- **WHEN** the installer writes the OpenCode MCP config
- **THEN** the config SHALL mark `mcp.projectatlas.type` as `local` and `mcp.projectatlas.enabled` as `true`.

#### Scenario: Generated config fails validation
- **WHEN** the generated OpenCode config does not match the verified runtime, version, DB, config, `cwd`, local type, enabled flag, or `mcp` command contract
- **THEN** the installer SHALL fail with a clear field-specific error before reporting OpenCode convergence.

### Requirement: OpenCode Non-Mutation Boundary
The ProjectAtlas installer SHALL NOT mutate OpenCode user settings, plugin caches, or unrelated OpenCode state unless a future official ProjectAtlas OpenCode integration surface is positively identified.

#### Scenario: No official repairable OpenCode state exists
- **WHEN** no official OpenCode ProjectAtlas plugin/cache surface is available
- **THEN** the installer SHALL limit convergence to generated config verification and documentation.

#### Scenario: Running OpenCode may cache old instructions
- **WHEN** generated config verification succeeds
- **THEN** the installer SHALL tell users to restart OpenCode if an older session cached previous instructions.
