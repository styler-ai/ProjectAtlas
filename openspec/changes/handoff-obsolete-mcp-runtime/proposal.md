## Why

A Windows Codex host can keep the stable ProjectAtlas MCP executable locked after a versioned runtime has been installed, leaving the host on obsolete code until a broad restart. The installer needs a narrow, fail-closed handoff that converges only when every replacement integration is already exact and never treats a mutation-skip flag as proof of readiness.

## What Changes

- Verify Codex plugin and MCP registry readiness independently from whether their mutation steps are enabled.
- Compare the global Codex MCP registration as structured JSON using the exact runtime command and ordered arguments.
- Retire only one unambiguous obsolete stable-mirror MCP process after final handle-bound creation, path, command-mode, version, and image-identity checks.
- Retry the stable-mirror replacement once and report typed complete or partial convergence without terminating Codex or unrelated ProjectAtlas processes.
- Document the safe handoff, retry, and external host-integration release gate.

## Capabilities

### New Capabilities

- `windows-installer-convergence`: Safe convergence of a locked obsolete Windows MCP runtime to a verified versioned runtime and exact Codex integration state.

### Modified Capabilities

None.

## Impact

The Windows plugin runtime installer, its Rust E2E/static contract tests, Codex host-integration documentation, and installer architecture view are affected. No Rust runtime API, SQLite schema, MCP tool schema, dependency, or non-Windows installer behavior changes.

This bugfix is ready for implementation in the v0.4.1 stabilization scope. Non-goals are broad process termination, automatic Codex restart, retiring current/unrelated/ambiguous processes, changing database ownership, and claiming real-host success without external exact-version verification.
