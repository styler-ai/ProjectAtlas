## Why

The Windows adversarial parser harness applies its 500 ms deliberate-stall allowance to process launch and containment admission for every hostile fixture. On loaded hosts, non-stall fixtures can therefore fail with an unrelated admission `NoProgress` result before reaching the exact protocol, I/O, limit, cancellation, or deadline behavior they own.

## What Changes

- Classify hostile fixtures by whether their expected behavior intentionally depends on a short launch/admission no-progress bound.
- Give non-stall fixtures one separate bounded launch/admission allowance while preserving their existing operation bounds, single attempt, exact typed assertions, and mandatory cleanup.
- Add deterministic harness-policy coverage and repeatedly run the complete Windows adversarial suite without changing production parser budgets.

## Capabilities

### New Capabilities

- `hostile-parser-fixture-admission`: Defines phase-specific adversarial-harness allowances, exact failure expectations, and cleanup proof for hostile parser fixtures.

### Modified Capabilities

None.

## Impact

The change is ready for implementation and is limited to the test-only adversarial policy in `crates/projectatlas-cli/src/parser_supervisor.rs`, its focused coverage, and OpenSpec routing. Production parser deadlines, no-progress limits, containment, protocol validation, resource limits, cancellation, and public APIs remain unchanged; no dependency or storage change is required.

## Non-Goals

- Retrying a hostile fixture after launch or admission failure.
- Accepting an unrelated admission timeout in place of a fixture's exact typed result.
- Changing the healthy-only recovery allowance from #391 or any production supervisor constant.
