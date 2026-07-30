## Why

The Windows adversarial parser gate applied its 500 ms hostile-stall allowance to the healthy recovery probe, so normal launch scheduling could fail after correct hostile cleanup. The v0.4.1 test-harness correction needs durable issue ownership so release checklist validation covers the completed fix.

## What Changes

- Give only the post-failure healthy recovery probe a platform-tolerant no-progress allowance.
- Preserve the short hostile-case budgets, exact typed failures, single launch attempt, and existing deadline for each attempt.
- Keep all production parser deadlines, protocol, containment, cancellation, and cleanup behavior unchanged.
- Require repeated Windows adversarial success plus ordinary cross-platform CI.

## Capabilities

### New Capabilities

- `parser-recovery-harness`: Defines the bounded distinction between hostile parser scenarios and the healthy recovery probe used by release tests.

### Modified Capabilities

None.

## Impact

The completed change affects only the adversarial parser-supervisor harness in `crates/projectatlas-cli/src/parser_supervisor.rs` and its release verification. Production runtime behavior, storage, dependencies, and public interfaces are unchanged.

## Non-Goals

- No production timeout or protocol change.
- No relaxation of hostile-stall assertions.
- No retry that could mask leaked process state.

This release-scoped test-harness correction is implemented and ready for v0.4.1 verification.
