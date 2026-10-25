## Why

The earlier #518 repair established a named 30-second readiness deadline and exact owned-process validation. The RC3 required parallel delivery gate now exposes two remaining fixture failures: its positive delayed-publication/identity case exhausts that deadline, and ordinary final teardown propagates a PowerShell stop-helper timeout even though exact fallback cleanup completed. The earlier startup and helper-creation costs are not separately measured, so the precise lost headroom is unconfirmed.

Routine readiness still starts PowerShell through its deadline-specific identity path although the fixture already owns native exact identity capture. Ordinary teardown also uses the deliberately diagnostic helper-failure chain. Reuse the existing native owners for routine fixture work; preserve that failure chain for tests which intentionally exercise it. Retrying the gate or increasing its budgets would not repair these ownership boundaries.

## What Changes

- Reuse native exact identity capture and stop for ordinary Windows fixture readiness and teardown, retaining the named readiness and cleanup bounds.
- Retain the existing bounded readiness-failure recovery and deliberate helper-fault cleanup paths; native teardown applies to the accepted-owner routine path.
- Preserve early parent-exit detection, atomic PID publication, exact PID/start-time/executable-path validation, bounded failure, and complete owned-process cleanup.
- Preserve deterministic delayed-publication/capture, true-deadline and helper-failure proof, including original helper errors after successful recovery and unrelated-process protection.
- Retain the single delivery-test owner established by historical #487; accept this repair before refreshing dependent #666 native-upgrade proof.

## Capabilities

### New Capabilities

- `windows-mcp-owner-fixture-readiness`: Deterministic, bounded Codex-owner/ProjectAtlas-child fixture startup, identity validation, diagnostics, and cleanup under full-suite contention.

### Modified Capabilities

None.

## Impact

- Affects only the Windows CLI integration-test fixture owner and its focused architecture/test proof.
- Unblocks #666's required broad Windows gate; #487's accepted split remains historical.
- Adds no dependency, product configuration, public payload, installer/MCP behavior, schema, database path, or production timeout change.
- Ready for implementation in v0.5.0 after the specification and architecture view are accepted.

## Non-Goals

- Masking product deadlocks or process leaks with an unbounded wait or repeated retry policy.
- Serializing the complete workspace suite or weakening exact process identity and ownership checks.
- Changing installer handoff semantics, MCP runtime behavior, or production timeout policy.
- Adding a general test framework, dependency, product setting, or cross-platform abstraction.
