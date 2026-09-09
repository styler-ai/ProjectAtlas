## Why

The required Windows workspace gate can reject an otherwise accepted head when the Codex-owned obsolete-MCP fixture does not publish its child PID within a hard-coded five-second startup window under parallel suite contention. The exact test passes in isolation, so retry-only acceptance would hide a release-gate race rather than prove that the broad run is reliable for v0.5.0.

## What Changes

- Define one named, bounded readiness contract for the Codex MCP owner fixture that remains reliable under supported parallel workspace test load.
- Preserve early parent-exit detection, atomic PID publication, exact PID/start-time/executable-path validation, bounded failure, and complete owned-process cleanup.
- Add deterministic delayed-publication and true-failure proof so the repair cannot become an unbounded wait or a weakened process-identity assertion.
- Reconcile the current monolithic test owner with the accepted #487 delivery-test split without duplicating the helper or changing product behavior.

## Capabilities

### New Capabilities

- `windows-mcp-owner-fixture-readiness`: Deterministic, bounded Codex-owner/ProjectAtlas-child fixture startup, identity validation, diagnostics, and cleanup under full-suite contention.

### Modified Capabilities

None.

## Impact

- Affects only the Windows CLI integration-test fixture owner and its focused architecture/test proof.
- Unblocks the final delivery-test owner in #487.
- Adds no dependency, product configuration, public payload, installer/MCP behavior, schema, database path, or production timeout change.
- Ready for implementation in v0.5.0 after the specification and architecture view are accepted.

## Non-Goals

- Masking product deadlocks or process leaks with an unbounded wait or repeated retry policy.
- Serializing the complete workspace suite or weakening exact process identity and ownership checks.
- Changing installer handoff semantics, MCP runtime behavior, or production timeout policy.
- Adding a general test framework, dependency, product setting, or cross-platform abstraction.
