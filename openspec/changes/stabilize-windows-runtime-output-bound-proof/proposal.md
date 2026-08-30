## Why

The Windows installer E2E currently measures its output-flooding probe from before the nested writer process starts. Under ordinary hosted-runner contention, process startup can consume the test's four-second assertion budget even when the one-MiB live output bound stops the probe correctly, making a required release gate flaky and non-causal.

## What Changes

- Measure output-bound enforcement from one test-owned writer-readiness observation instead of charging nested-process startup delay to the byte-limit assertion.
- Distinguish delayed startup, live output-limit termination, and the existing true five-second timeout while preserving exact owned-process cleanup and fail-closed runtime validation.
- Keep installer/runtime behavior, the five-second product timeout, the one-MiB output ceiling, public contracts, dependencies, and suite parallelism unchanged unless the causal fixture proves a separate product defect.
- Treat this as backlog specification work until the packet and issue mirror are published on `main`; only then may #525 enter `v0.5.0-00` and implementation routing.

Non-goals:

- Increasing timeout or output limits, retrying, or serializing the Windows suite.
- Adding a process-test framework, dependency, persistent state, or cross-platform abstraction.
- Changing installer, PATH, runtime, MCP, database, or public CLI behavior without a separately reproduced defect.

## Capabilities

### New Capabilities

- `windows-runtime-output-bound-proof`: deterministic causal Windows proof that installer-owned runtime probes enforce their live output ceiling independently of nested-process startup delay.

### Modified Capabilities

None.

## Impact

- Windows-only CLI E2E fixture code in the existing delivery-test owner.
- Existing `Invoke-ProjectAtlasBoundedJsonCommand` behavior is observed but is not expected to change.
- OpenSpec/IssueOps mapping and the v0.5 release graph after published readiness.
- No Rust product source, SQLite schema, dependency, workflow, installer contract, or public payload change is expected.
