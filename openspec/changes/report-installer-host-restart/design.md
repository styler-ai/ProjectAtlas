## Context

On Windows, an old ProjectAtlas process can lock `%LOCALAPPDATA%\ProjectAtlas\bin\projectatlas.exe`. The installer correctly falls back to the checksum-verified versioned runtime, writes absolute MCP configs, updates Codex MCP registration, prepends the runtime directory to its own process PATH, and persists User PATH for future processes. It then validates bare command resolution inside that child process and can imply that later shells from the already-running parent host are ready, although a child cannot update its parent's environment.

## Goals / Non-Goals

**Goals:**

- Report runtime/generated-config readiness separately from inherited parent-host bare CLI readiness.
- Require a restart only when the unchanged parent environment will not resolve either the verified versioned runtime or a stable mirror synchronized during installation.
- Keep future User PATH and absolute MCP behavior correct.
- Model parent, installer child, and later sibling process behavior on Windows.

**Non-Goals:**

- Mutating or terminating the parent host, terminals, Codex, or ProjectAtlas processes.
- Failing a usable absolute-runtime/MCP installation solely because the parent must restart.
- Adding a general installer result protocol or changing Rust/MCP schemas.

## Decisions

### Capture inherited bare-command state before PATH mutation

Record the parent PATH and bare `projectatlas` command path before the installer prepends its versioned runtime. After installer-owned stale-shim quarantine, reuse the captured path when it still exists; if quarantine removed it, resolve the next command from the captured parent PATH without executing it. Compare that effective sibling path with the verified runtime and synchronized stable mirror. Path identity is sufficient because those two destinations are verified by the installer; the inherited command is not executed and therefore cannot hang installation.

Checking the installer's final `Get-Command` alone is insufficient. Attempting to inspect or mutate the parent process was rejected as unsafe and platform-fragile.

### Return stable-mirror synchronization state from the existing helper

Make `Sync-ProjectAtlasRuntimeToLocalAppData` report whether the stable mirror is current. Both existing call sites retain the selected verified runtime separately, so the helper need not return a path. The final restart requirement is false only when the effective sibling command after installer-owned quarantine is the verified runtime or the stable mirror that installation synchronized; merely updating a path the parent does not resolve cannot make a later sibling ready.

### Emit one explicit final readiness record

After the runtime and generated absolute MCP configs are verified, emit a deterministic line containing `runtime_mcp_configs_ready`, `installer_cli_ready`, and `host_restart_required`. Optional global Codex registry repair keeps its existing separate success, skip, or warning output and is not folded into the generated-config readiness field. When restart is required, also emit one clear warning explaining that future bare CLI calls from the existing host remain stale until restart. Normal unlocked installation reports `host_restart_required=false`.

This avoids a new object/schema while remaining machine-testable in PowerShell and release logs.

## Risks / Trade-offs

- **Restart is reported when the parent was already correct** → Compare the effective post-quarantine sibling path from the captured parent PATH, not merely the mirror outcome.
- **Locked mirror becomes a hard install failure** → Keep absolute runtime/MCP success authoritative and report partial host readiness with exit code zero.
- **Tests accidentally mutate the developer environment** → Use temporary LocalAppData/User-profile state, fake Codex, scoped PATH, and an owned lock process.
- **A process is killed to make the test pass** → The test owns and closes only its fixture lock process; production installer contains no termination path.

## Migration Plan

Ship the PowerShell message/state correction in v0.4.1. Existing generated configs and registry entries remain compatible. Users with a locked stale mirror restart the host once; fresh hosts then inherit the persisted User PATH.

## Open Questions

None.
