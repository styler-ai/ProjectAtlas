## Why

Three generic E2E subprocess observers can accept a child that completed before polling resumed even when the observation occurs after the caller's explicit deadline. This can turn a late, invalid test observation into apparent success and weaken v0.5.0-rc1 release proof under ordinary scheduler contention.

## What Changes

- Classify deadline expiry before accepting child completion in the shared MCP-session, MCP-process, and installer-output E2E observers.
- Preserve bounded output collection, status validation, child termination/reaping, diagnostics, and all existing successful in-deadline behavior.
- Add causal delayed-observer regressions that distinguish completion time from observation time without retries, global locks, suite serialization, or extra scheduler slack.

## Capabilities

### New Capabilities

- `e2e-process-observer-deadlines`: Require generic subprocess test observers to reject completion first observed at or after their deadline while preserving exact cleanup and compatible in-time success.

### Modified Capabilities

None.

## Impact

- Affects only generic process-observer helpers and their tests in `crates/projectatlas-cli/tests/e2e.rs`.
- Adds no product CLI/MCP behavior, public schema, crate, dependency, workflow, database, migration, or persistent state.
- The change is ready for implementation after its issue/OpenSpec packet is accepted, but its shared-file implementation lane must not overlap the active #518 amendment.

## Non-Goals

- Changing subprocess timeout durations, adding retries, or serializing the E2E suite.
- Refactoring all process helpers into a new framework or abstraction.
- Expanding #518's Windows Codex-owner fixture boundary into this issue.
- Claiming that a process completed within its deadline when the observer did not establish that fact in time.
