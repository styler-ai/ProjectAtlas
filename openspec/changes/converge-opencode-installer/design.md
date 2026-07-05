## Context

OpenCode support has two ProjectAtlas-owned surfaces: the plugin metadata under `plugins/projectatlas/opencode/opencode.json` and the generated project-local MCP config `.projectatlas/projectatlas.opencode.json`. The installer can safely verify the generated config because it is written by ProjectAtlas. It must not mutate arbitrary OpenCode user state unless a future official ProjectAtlas OpenCode surface exists.

## Contract

The installer SHALL validate the generated OpenCode config after writing it:

- `mcp.projectatlas.command` is an absolute path to the verified runtime.
- `mcp.projectatlas.args` includes `--require-version <runtime-version>`.
- `mcp.projectatlas.args` includes `--db <project-root>/.projectatlas/projectatlas.db`.
- If `.projectatlas/config.toml` or `projectatlas.toml` exists, args include `--config <effective-config-path>`.
- The last argument is `mcp`.
- `mcp.projectatlas.type` is `local`.
- `mcp.projectatlas.enabled` is `true`.
- `mcp.projectatlas.cwd` is the selected project root.

Validation failure SHALL fail the installer before it reports OpenCode convergence.

## Implementation Notes

- Reuse the existing `mcp-config --harness opencode` output; do not duplicate JSON construction in installer scripts.
- PowerShell should parse the generated JSON with `ConvertFrom-Json`.
- POSIX should prefer `jq` and fall back to `python3` JSON parsing; it must fail closed if neither parser is available.
- The validation helpers should share as much structure as is reasonable with the Claude Code generated-config check while keeping host-specific field names clear.

## Edge Cases

- Project root paths with spaces: validate JSON array elements, not shell command strings.
- Project config absent: require only DB and runtime/version args.
- Nested and flat configs: prefer nested `.projectatlas/config.toml` when present, otherwise flat `projectatlas.toml`.
- Running OpenCode process: print restart guidance and do not claim live session refresh.

## Pre-Mortem

Risk: the installer validates the generic MCP fields but misses OpenCode's `cwd` hint.
Mitigation: add explicit test assertions for `type`, `enabled`, and `cwd`.

Risk: OpenCode docs change and a future repair path appears.
Mitigation: this release documents the current generated-config contract and leaves native repair for a separate audited issue.

Risk: POSIX fallback parser silently accepts a bad config.
Mitigation: fail closed when a required field cannot be extracted.
