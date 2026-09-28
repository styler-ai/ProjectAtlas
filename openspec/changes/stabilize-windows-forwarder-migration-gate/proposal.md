## Why

The Windows opposite-forwarder migration E2E fails under a full concurrent suite at observer and aggregate wall-time assertions even when the installer lock refusal is bounded. Its unstable signal blocks unrelated RC3 acceptance, so the gate must distinguish real installer failure from host scheduling and output collection.

## What Changes

- Measure the migration and held-lock phases at their causal boundaries, with finite process observation and cleanup bounds.
- Keep real installer children, state/ownership checks, the 250 ms held-lock refusal, and negative deadlock/late-exit proof.
- Make failure output identify the phase and child status before attributing an elapsed-time failure to lock ordering.

## Capabilities

### New Capabilities

- `installer-concurrency-gate-proof`: causal, bounded cross-process proof for opposite Atlas forwarder migrations under Windows suite load.

### Modified Capabilities

None. Production installation and forwarder-lock requirements do not change.

## Impact

Issue #633 owns the Windows installer delivery E2E and its narrow process observer seam under release owner #492. No product lock deadline, CLI/MCP API, database, dependency, or package format changes are planned. The change is ready for implementation and blocks reliable #624 and RC3 validation.

## Non-Goals

No test suppression, whole-suite serialization, unbounded timeout, or relaxation of the product's held-lock deadline.
