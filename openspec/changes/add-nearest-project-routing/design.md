## Routing Model

Nearest-project routing is disabled by default. It can be enabled for a server process with `projectatlas mcp --nearest-project`, generated into MCP configs with `projectatlas mcp-config --nearest-project`, or overridden per call with `nearest_project`.

Explicit `project_path` always wins. If a caller supplies `project_path`, ProjectAtlas stays inside that selected project even when `nearest_project` is true.

## Discovery Rules

For path-bearing tools, ProjectAtlas walks upward from the addressed absolute path to find the nearest ancestor with `.projectatlas/projectatlas.db`. A candidate is valid only when:

- the DB exists;
- read-only metadata can be opened without creating WAL/SHM sidecars;
- stored DB root/config root agrees with the candidate root;
- symlink or junction paths do not create multiple plausible roots.

When no valid DB exists, the MCP response explains that the agent should use normal filesystem tools such as `Get-Content`, `rg`, or targeted shell reads.

## Audit Metadata

When a read response is routed to a project other than the active/default project, the response starts with a `selected_project` block containing the selected root, DB path, config path, and status before the normal payload.
