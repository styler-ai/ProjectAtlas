## Why

The shared installer E2E release-asset server starts a one-minute deadline before the owned Windows installer process reaches its download phase. Under ordinary parallel workspace load, the server can expire first and report `timed out waiting for release asset request` even though the focused Windows installer test succeeds. This makes the release gate depend on unrelated scheduling delay instead of the owned installer lifecycle; changing the shared helper must also preserve its POSIX checksum-mismatch caller.

## What Changes

- Bind the shared local release-asset fixture to one four-minute absolute operation deadline created before listener and installer launch; keep the existing five-minute workflow step as an independent outer kill boundary without claiming a fixed cleanup reserve.
- Observe the installer through the existing bounded installer helper with the remaining operation budget, give the server one bounded standard-library completion signal, and preserve both owner and request failures on every terminal path.
- Preserve the current archive/checksum suffix routing, exact payload validation, two-request completion rule, deterministic cleanup, and existing installer/product behavior.
- Cover delayed download, installer failure, missing or invalid requests, the shared POSIX checksum caller, and ordinary parallel execution without retry, a new fixture lock, or broader suite serialization.
- Keep this as backlog specification work until the packet is published on `main`; after publication #533 is one direct child and blocker of #492, has no direct blocker, unlocks only #492, and is operationally sequenced after the active shared-file owner without a native dependency edge.

Non-goals:

- Changing installer download, runtime, PATH, MCP, database, or release behavior.
- Raising or retaining an independent pre-request helper deadline, or relying on the five-minute CI step as the only bound.
- Hardening the current suffix-based fixture routing into full-path validation.
- Adding a process-test framework, dependency, new global lock, or generic server abstraction.

## Capabilities

### New Capabilities

- `windows-release-asset-fixture-lifecycle`: causal, bounded Windows installer proof under ordinary parallel test load.

### Modified Capabilities

None.

## Impact

- The shared CLI E2E release-asset helper in the existing delivery-test owner, including four Windows installer callers and POSIX checksum-mismatch compatibility.
- OpenSpec/IssueOps mapping and the v0.5 release graph after published readiness.
- No Rust product source, SQLite schema, dependency, workflow, installer contract, or public payload change is expected.
