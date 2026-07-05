## Why

OpenCode integration ships through `plugins/projectatlas/opencode/opencode.json` and generated `.projectatlas/projectatlas.opencode.json`. After fixing Codex plugin/runtime drift, OpenCode needs an equivalent convergence check that verifies the generated project-local config is actually bound to the runtime the installer just verified, without assuming OpenCode has a Codex-like marketplace/cache.

## What Changes

- Add an OpenCode installer convergence check after the generated OpenCode MCP config is written.
- Validate the generated config against the verified runtime path, `--require-version`, selected DB path, optional config path, final `mcp` subcommand, enabled state, local type, and project `cwd` hint.
- Keep OpenCode config generation delegated to `projectatlas mcp-config --harness opencode`.
- Emit explicit restart guidance because a running OpenCode process may cache earlier plugin instructions or MCP config.
- Document that no OpenCode marketplace/cache repair is performed unless a future host API exposes a positively identifiable official ProjectAtlas state.

## Capabilities

### New Capabilities
- `installer-opencode-convergence`: Defines the generated-config verification and non-mutation contract for OpenCode integration.

### Modified Capabilities
- ProjectAtlas plugin installer behavior on Windows, Linux, and macOS.
- OpenCode setup and agent integration documentation.

## Release Scope

This change is scheduled for the next version. It is intentionally bounded to generated MCP config verification plus docs. Native OpenCode plugin/cache mutation remains out of scope until a real official host API exists.

## Non-Goals

- Do not invent an OpenCode marketplace or plugin-cache repair path.
- Do not mutate user-managed OpenCode settings or unrelated OpenCode files.
- Do not add opt-out environment variables before a mutation path exists.
- Do not claim a running OpenCode session has refreshed its cached instructions.

## Pre-Mortem

Likely failure modes:
- The installer writes OpenCode config but does not catch a stale command path.
- The check misses OpenCode-only fields such as `type`, `enabled`, or `cwd`.
- OpenCode config generation drifts from the plugin template.
- Tests encode a fake marketplace model that OpenCode does not actually support.
- PowerShell and POSIX installers validate different fields.

Mitigations:
- Validate parsed generated JSON arrays and OpenCode-specific fields.
- Keep JSON generation in Rust `mcp-config` and only validate in installer scripts.
- Test real installer output rather than a handcrafted fixture.
- Keep documentation explicit that OpenCode convergence is generated-config based.
