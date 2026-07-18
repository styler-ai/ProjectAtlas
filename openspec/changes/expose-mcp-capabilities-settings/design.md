## Context

ProjectAtlas MCP now supports selected-project routing and optional nearest indexed project discovery. Those are server-session policies, not static binary capabilities. Agents need to inspect them before summary, slice, search, or file-ranking calls.

## Contract

`atlas_settings` SHALL add a nested MCP capability/session payload without removing existing settings fields. The payload includes:

- `runtime`: project, version, major version, executable, repository, capabilities, text format, output formats, and compiled MCP tool names from `build_runtime_info()`.
- `selected_project`: root, DB path, config path, and index status.
- `startup_policy`: nearest-project policy as `enabled` or `disabled`.
- `path_scope`: `selected_project` when nearest-project routing is disabled, `nearest_indexed_project` when enabled.
- `scan_policy`: effective config-root text index size and no implicit scan from settings.
- `telemetry`: `enabled` or `disabled` based on the same no-telemetry environment gate used elsewhere.
- `privacy`: stable booleans indicating that no environment dump, token values, or unrelated profile data are included.

## Implementation Notes

- Keep `atlas_settings` return shape additive to avoid another MCP tool when one diagnostic call already exists.
- The existing `SettingsReport` can be embedded or wrapped in an MCP-specific payload.
- Use typed enums for nearest-project, path scope, telemetry, index status, and scan mutation policy.
- `build_settings_report` may inspect path existence and index stats but must not create an index.
- Do not add `mcp_nearest_project` or similar to CLI `runtime-info`; an existing e2e test protects that boundary.

## Edge Cases

- Missing DB: return selected DB path plus `index_status: missing`.
- Config absent: return `config: null` and keep scan policy based on discovered/default config.
- Nearest-project disabled: `path_scope` must make absolute out-of-root paths selected-project-only.
- Nearest-project enabled: `path_scope` must tell harnesses that absolute path calls may route to nearest indexed roots.
- No-secret output: assert common token env names do not appear.

## Pre-Mortem

Risk: settings becomes too noisy.
Mitigation: put new fields under one nested MCP session/capability key.

Risk: capability values drift from runtime and server construction.
Mitigation: build the payload inside `ProjectAtlasMcpServer` from its actual state.

Risk: privacy test becomes brittle by rejecting allowed Windows user paths.
Mitigation: test for secret keys/token-looking values rather than any user path; selected ProjectAtlas paths are allowed.
