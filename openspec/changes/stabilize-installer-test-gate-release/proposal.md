## Why

The Windows installer concurrency fixture intermittently aborts when PowerShell's
provider-backed gate lookup races removal of the gate. This shared test failure
blocks normal validation even when the owning feature is unrelated.

## What Changes

- Use native exact-file existence checks for the three test-only pause gates.
- Preserve ready signals, hold/release ordering, lifecycle assertions, and budgets.
- Prove release behavior and run the existing real opposite-migration E2E.

## Capabilities

### New Capabilities

- `installer-test-gate-release`: reliable Windows test-file gate release.

### Modified Capabilities

None.

## Impact

Issue #572 is ready for implementation and is a shared validation prerequisite of
#465 under release owner #492. The affected owners are the Windows plugin installer
test hooks and delivery E2E. No API, dependency, database, or production lock changes.

## Non-Goals

No timeout increase, retry-based acceptance, test suppression, or CI routing change.
