## Naming

Use predictable `atlas_*` names that mirror CLI nouns and subcommands:

- `projectatlas init` -> `atlas_init`
- `projectatlas root show|verify|set` -> `atlas_root` with `verify` when needed, and `atlas_root_set`
- `projectatlas config --print` -> `atlas_config`
- `projectatlas ignore list|init-gitignore|add|remove` -> `atlas_ignore_list`, `atlas_ignore_init_gitignore`, `atlas_ignore_add`, `atlas_ignore_remove`
- `projectatlas lint` -> `atlas_lint`
- `projectatlas runtime-info` -> `atlas_runtime_info`
- `projectatlas mcp-config` -> `atlas_mcp_config`
- `projectatlas map` -> `atlas_map`

## Behavior

Parity tools should call the same service helpers as CLI handlers and return the same stable fields in bounded TOON-first payloads. Mutating tools must make mutation explicit through required arguments or clearly named flags, not implicit diagnostics.

Root-sensitive tools accept `project_path`. One MCP server may serve multiple repositories, so handlers must not rely on process-global active state when the caller supplies `project_path`.

## Reviewed Exceptions

`projectatlas mcp` remains CLI-only because it starts the MCP server process. `projectatlas watch` continuous mode remains CLI-only until a separate MCP lifecycle contract exists; agents use `atlas_watch_once` and `atlas_watch_status` instead. `projectatlas token --view tui` remains terminal-only; agents use `atlas_token_report` and optional chart payloads instead.
