## Why

Released-schema tests embed captured SQL fixtures at compile time and mutate exact DDL fragments to prove incompatible drift is rejected. On Windows, Git checked those fixtures out with CRLF endings while the test mutations use LF, so the release gate failed before exercising schema validation.

## What Changes

- Pin repository SQL files to LF checkouts alongside the existing Rust, JSON, TOML, and documentation line-ending contracts.
- Keep the released schema fixtures byte-stable across Windows, Linux, and macOS checkouts.
- Verify the evolved released-schema drift test and the complete database test suite on Windows.

## Capabilities

### New Capabilities

- `released-schema-fixture-portability`: Defines platform-independent checkout behavior for embedded released-schema fixtures.

### Modified Capabilities

None.

## Impact

The change affects only Git checkout normalization for `*.sql` files and release-gate verification. It does not change production schema DDL, migration logic, database contents, dependencies, or public interfaces.

## Non-Goals

- Changing schema admission or migration behavior.
- Normalizing database contents at runtime.
- Adding a custom line-ending parser or test abstraction.
