## Why

When Windows locks the stable LocalAppData mirror, the installer safely uses the versioned runtime and absolute MCP paths but then validates bare `projectatlas` only inside its own mutated child process. The already-running Codex parent can still launch later shells with stale PATH, so the installer must report partial readiness honestly.

## What Changes

- Distinguish verified runtime/MCP readiness, installer-process CLI readiness, and parent-host restart requirement.
- Emit an unambiguous, testable restart-required state only when the effective fresh Machine-plus-User PATH resolves the verified runtime; otherwise report that restart alone cannot repair the stale parent command.
- Preserve versioned runtime installation, absolute generated MCP configs, Codex MCP registration, and fresh-host PATH precedence.
- Add a Windows parent/installer-child/later-sibling regression test plus unlocked and no-process-termination controls.

## Capabilities

### New Capabilities

- `installer-host-restart-state`: Defines truthful Windows installer readiness when a persistent parent host cannot inherit the installer's process environment.

### Modified Capabilities

None.

## Impact

- The shipped PowerShell installer, its Windows installer integration tests, and matching operator guidance.
- Installer messages/state only; Rust runtime, SQLite, MCP request schemas, and generated config shapes remain unchanged.

## Non-Goals

- Mutating another process's environment or terminating any pre-existing ProjectAtlas, Codex, terminal, or unrelated process. The installer may terminate only the exact runtime-info probe child it started after that probe exceeds its time or output bound.
- Treating a locked stable mirror as failure when the verified runtime and MCP integration are ready.
- Replacing absolute version-guarded MCP paths with bare command resolution.
- Adding a cross-platform installer result framework for this Windows-specific parent-process fact.

## Status

Ready for implementation in the v0.4.1 bugfix-only stabilization release.
