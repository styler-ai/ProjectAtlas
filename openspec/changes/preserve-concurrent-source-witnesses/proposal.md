## Why

Concurrent MCP navigation and purpose mutation share a source observer. An older read can reject its superseded epoch and clear the newer mutation witness, causing `refresh_required` even when source is unchanged. Hosted payload validation exposed this boundary; it blocks release acceptance.

## What Changes

- Distinguish accepted evidence, superseded evidence, and actual source invalidation at the existing observer acceptance boundary.
- Retry superseded reads without clearing newer valid evidence; retain fail-closed source, policy, continuity, cancellation, and identity checks.
- Reject syntactically invalid or confirmed absent purpose-set targets through read-only preflight before they can supersede a valid mutation witness; preserve exact repair for newly saved source and transactional indexed-existence checks.
- Prove the interleaving deterministically and pair it with a real-invalidation rollback case before full CLI/MCP and platform validation.

## Capabilities

### New Capabilities

- `concurrent-source-witnesses`: preserve valid newer source witnesses while rejecting stale readers and actual invalidation safely.

### Modified Capabilities

None.

## Impact

The existing CLI runtime source-observation owner, MCP purpose preflight, and their owning tests change. No public schema, persistence migration, dependency, or crate is added. MCP purpose transactions retain exact-source verification before commit.

## Non-Goals

No fixture refresh, retry-until-green, timeout increase, telemetry suppression, observer bypass, or change to PHP guidance. Do not replace observed mutation admission with an unconditional exact-only path.

## Readiness

Release-blocking repair in causal reproduction and specification. Implementation begins after the deterministic failure and owning issue are established.
