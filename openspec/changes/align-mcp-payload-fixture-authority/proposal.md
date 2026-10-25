## Why

The real MCP payload fixture claims one initialization/navigation database but
mixes an external selected database with explicit project-local initialization.
This invalidates the fixture's stated lifecycle and complicates a hosted purpose
freshness failure whose exact observer cause is not established.

## What Changes

- Select the canonical project-local database throughout the existing fixture.
- Assert initialization and selected-runtime database identity before payload proof.
- Preserve all payload assertions and investigate any remaining freshness refusal.

## Capabilities

### New Capabilities

- `mcp-payload-fixture-authority`: one database for the real MCP payload fixture.

### Modified Capabilities

None.

## Impact

Issue #574 is ready for implementation under #492 and unblocks shared validation
for #572 and #465. Only the existing delivery fixture and its source-contract
digest mirrors change; product routing, storage, and observer owners do not.

## Non-Goals

No extra refresh, timeout increase, retry-based acceptance, weakened assertion,
database replacement, or unproven product observer change.
