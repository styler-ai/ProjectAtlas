## Why

The native-watch E2E writes its fixture after a fixed 750 ms sleep, so a loaded hosted runner can emit the change before the child installs its watcher. The initial refresh then absorbs the write as cycle one and the test waits forever for a second event, blocking the v0.4.2 release despite correct production behavior.

## What Changes

- Replace elapsed-time startup guessing with a bounded observable readiness check.
- Require the fixture's exact initial symbol in the selected database while the watch child remains live before writing the changed source.
- Preserve the native-event cycle-two exit and changed-symbol assertions with specific early-exit and deadline diagnostics.
- Keep production watcher behavior, timeouts, retries, storage, dependencies, and public surfaces unchanged.

## Capabilities

### New Capabilities

- `notify-watch-test-readiness`: Defines the observable publication and child-liveness boundary required before the native-watch E2E emits its change event.

### Modified Capabilities

None.

## Impact

The ready-for-implementation bugfix is confined to `crates/projectatlas-cli/tests/e2e.rs` plus its OpenSpec and issue routing. It adds no production code, API, crate, dependency, schema, query, transaction, or persistent-format change.

## Release Scope

Bugfix-only v0.4.2 release reliability.

## Non-Goals

- Changing production watcher timing or event handling.
- Serializing the E2E suite or retrying a missed native event.
- Adding a reusable readiness framework for one owning test.
