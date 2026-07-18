## Context

Claude Code integration is currently file-based from ProjectAtlas' side: the installer writes `.projectatlas/projectatlas.claude.mcp.json`, and the Rust CLI owns the harness-specific JSON shape. The active session may still cache older tool instructions until restarted, so installer convergence can verify project-local config but cannot promise in-process Claude state has changed.

## Contract

The installer SHALL validate the generated Claude Code config after writing it:

- `mcpServers.projectatlas.command` is an absolute path to the verified runtime.
- `mcpServers.projectatlas.args` includes `--require-version <runtime-version>`.
- `mcpServers.projectatlas.args` includes `--db <project-root>/.projectatlas/projectatlas.db`.
- If `.projectatlas/config.toml` or `projectatlas.toml` exists, args include `--config <effective-config-path>`.
- The last argument is `mcp`.
- Claude Code config does not require a `cwd` field because the absolute DB/config args bind the project.

Validation failure SHALL fail the installer before it reports host convergence. The error should name the mismatched field so the user can rerun with a repaired runtime or inspect the generated config.

## Implementation Notes

- PowerShell should use `ConvertFrom-Json` to parse the generated file, then compare normalized paths with the existing runtime/path helpers where possible.
- POSIX should prefer `jq` when available and fall back to `python3` JSON parsing; it must fail closed if neither parser is available.
- Both installers should call host-specific verification after `Write-ProjectAtlasMcpConfig $claudeMcpConfigPath "claude-code"` / `write_mcp_config "$claude_mcp_config_path" claude-code`.
- The verification function should be host-agnostic in behavior and host-specific only in parsing syntax.

## Edge Cases

- Project config absent: do not require `--config`.
- Flat project config present: require the flat config path when nested config is absent.
- Paths with spaces: compare parsed JSON array elements, not string-split command lines.
- Case differences on Windows: compare through existing normalized absolute path handling.
- Running Claude Code process: print restart guidance without claiming live session refresh.

## Pre-Mortem

Risk: generated config verification drifts from Rust output.
Mitigation: use the real generated file in installer tests and keep config generation centralized in `mcp-config`.

Risk: validating command text misses equivalent Windows paths.
Mitigation: use canonical/normalized comparisons rather than raw string equality for paths.

Risk: future host-specific repair gets bolted on without official-source proof.
Mitigation: keep this change's non-goals explicit and require a separate issue/spec for any native Claude mutation.
