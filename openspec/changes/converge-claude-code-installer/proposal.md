## Why

ProjectAtlas already writes `.projectatlas/projectatlas.claude.mcp.json` from the verified runtime, DB, config, and version guard. After the Codex cache-drift work, Claude Code needs the same convergence discipline: the installer must prove the generated Claude Code MCP config points at the runtime it just verified, and it must not pretend to repair a Claude marketplace/cache surface unless that surface is positively identified.

## What Changes

- Add a Claude Code installer convergence check after the generated Claude MCP config is written.
- Validate the generated config against the verified runtime path, `--require-version`, selected DB path, optional config path, and final `mcp` subcommand.
- Keep Claude Code config generation delegated to `projectatlas mcp-config --harness claude-code`; the installer must not hand-build host JSON.
- Emit explicit restart guidance because a running Claude Code process can keep stale in-process instructions even after the project-local config is repaired.
- Document that no Claude Code marketplace/cache repair is performed unless a future host API exposes a positively identifiable official ProjectAtlas state.

## Capabilities

### New Capabilities
- `installer-claude-code-convergence`: Defines the generated-config verification and non-mutation contract for Claude Code integration.

### Modified Capabilities
- ProjectAtlas plugin installer behavior on Windows, Linux, and macOS.
- Agent setup documentation that describes ProjectAtlas host convergence.

## Release Scope

This change is scheduled for the next version. It is intentionally bounded to generated MCP config verification and documentation. Native Claude Code plugin/cache mutation remains out of scope unless a real, official, reversible API is found during implementation.

## Non-Goals

- Do not copy Codex marketplace/plugin commands into Claude Code.
- Do not mutate user-managed Claude settings or generic Claude files.
- Do not add an opt-out environment variable before there is a mutation path to opt out of.
- Do not claim a running Claude Code session has refreshed its cached instructions.

## Pre-Mortem

Likely failure modes:
- The installer reports success after writing a config that still points at an old runtime.
- The check accepts a command path that is not absolute or not the verified executable.
- The generated config is current, but the message implies Claude Code's running process has already reloaded it.
- A future fake test encourages non-existent Claude marketplace behavior.
- PowerShell and POSIX installers drift in the fields they verify.

Mitigations:
- Parse the generated JSON and compare exact command, DB, config, version guard, and final `mcp` arguments.
- Keep the restart wording explicit and conservative.
- Test the generated-config contract through the real installer on the current host.
- Keep all JSON generation in the Rust `mcp-config` path and only validate in installer scripts.
