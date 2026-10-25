## Why

Windows MCP server teardown can leave native directory notifications active while temporary repositories are removed, hanging otherwise completed tests. This shared runtime defect blocks normal release validation.

## What Changes

- Complete Windows unwatch operations within the source-observer entry lifetime.
- Preserve cleanup ownership when watch registration fails partway through startup.
- Prove directory cleanup after server teardown and failed registration.

## Capabilities

### New Capabilities

- `native-source-observer-teardown`: Complete owned Windows directory-watch cancellation before releasing the observer.

### Modified Capabilities

None.

## Impact

The existing CLI source-observation entry and MCP identity regression test. No public API, dependency, database, or non-Windows observation change. Issue #570 is ready for implementation and blocks #465 validation under release owner #492.

## Non-Goals

No timeouts, retries, watcher framework, test suppression, or CI redesign.
