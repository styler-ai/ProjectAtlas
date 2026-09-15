## Why

Release acceptance currently executes every top-level CLI command and advertised MCP tool, but some nested CLI routes are only covered by help checks. Its predecessor test constructs a historical SQL schema instead of exercising a published v0.4.5 installation, so green packages do not establish the complete installed-route and real upgrade proof required by #492.

## What Changes

- Extend the existing packaged CLI behavior contract to reconcile and execute every supported nested route with isolated state and typed positive or refusal outcomes.
- Require the actual checksum-verified v0.4.5 platform executable to create and exercise a predecessor project before the exact candidate installer updates that same state.
- Exercise injected installer failure, atomic preservation, repair/retry, authored and derived state continuity, generated host binding, and compatibility-safe rollback/refusal on every supported platform.
- Repair the demonstrated Unix upgrade gap with an explicit `root set <path> --transition adopt-legacy` operation for schema-19 databases, preserving ordinary fail-closed identity admission and reusing the existing atomic migration sequence.
- Keep the existing synthetic schema tests as focused compatibility checks and the existing dynamic MCP inventory as the tool behavior authority.

## Capabilities

### New Capabilities

- `installed-release-acceptance`: Complete nested CLI behavior and actual published-predecessor upgrade proof in the existing packaged release matrix.

### Modified Capabilities

- `project-path-and-graph-integrity`: Explicit operator-owned legacy root recovery without weakening ordinary native identity admission.

## Impact

The existing CLI integration tests, root transitions, database migration owner, frozen source contract, release workflow, and release architecture own this change. The demonstrated Unix predecessor refusal is repaired in this same issue and PR through one explicit transition. No new runtime framework, database schema, or external dependency is proposed. The database migration reuses the existing filesystem crate for native Git lifecycle proof.

## Non-Goals

This change is ready for implementation. It does not publish the RC, authorize stable promotion, add npm delivery, or alter unrelated host installations and project state.
