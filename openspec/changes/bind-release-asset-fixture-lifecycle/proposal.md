## Why

The Windows installer E2E release-asset server starts a one-minute deadline before the owned installer process reaches its download phase. Under ordinary parallel workspace load, the server can expire first and report `timed out waiting for release asset request` even though the focused installer test succeeds. This makes the release gate depend on unrelated scheduling delay instead of the owned installer lifecycle.

## What Changes

- Bind the local release-asset fixture lifetime to the owned installer operation and one explicit overall test bound.
- Preserve exact archive and checksum request validation, deterministic cleanup, and existing installer/product behavior.
- Cover delayed download, installer failure, missing request, and ordinary parallel execution without another timeout increase, retry, or suite serialization.
- Keep this as backlog specification work until the packet is published on `main`; only then may #533 enter `v0.5.0-00` and implementation routing.

Non-goals:

- Changing installer download, runtime, PATH, MCP, database, or release behavior.
- Raising the one-minute helper deadline again.
- Adding a process-test framework, dependency, global lock, or generic server abstraction.

## Capabilities

### New Capabilities

- `windows-release-asset-fixture-lifecycle`: causal, bounded Windows installer proof under ordinary parallel test load.

### Modified Capabilities

None.

## Impact

- Windows-only CLI E2E fixture code in the existing delivery-test owner.
- OpenSpec/IssueOps mapping and the v0.5 release graph after published readiness.
- No Rust product source, SQLite schema, dependency, workflow, installer contract, or public payload change is expected.
