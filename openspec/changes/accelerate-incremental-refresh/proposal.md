# Proposal

## Why

Refreshing a large project can take too long and hit its deadline even when little changed. v0.6.0 must make `watch --once` and `atlas_watch_once` process the actual changes and affected index data, with reliable results and a time budget that fits the work.

## What Changes

- Reuse existing change tracking and incremental publication before adding new machinery. Continuous watching already receives `WatchChangeSet`; one-shot refresh currently performs exact whole-project discovery, and MCP watch-once bypasses `SourceObservationRegistry`.
- Make verified unchanged refreshes cheap. Hash, parse and publish only the source and derived data that require an update when trustworthy change evidence is available.
- Detect lost or unavailable change history, offline changes and policy changes explicitly. Reconcile the wider project when necessary rather than silently returning stale results.
- Profile the current optimized runtime, select the smallest safe algorithm, and set reproducible speed and resource targets before implementation. Cover change sets of any size and supported platforms.
- Choose the default cooperative work budget from estimated work, within documented limits. Preserve explicit caller limits, cancellation, atomic failure, safe retry and current project identity.

## Capabilities

### New Capabilities

- `incremental-refresh`: trustworthy change-set refresh, bounded adaptive work budgets, and measurable large-project performance through CLI and MCP.

### Modified Capabilities

None. Existing freshness, root isolation, publication and caller-limit guarantees remain constraints.

## Impact

The existing CLI/MCP refresh adapters, source-observation and filesystem scan owners, SQLite source comparison and graph publication, and performance/E2E tests. Existing Rust dependencies and seven crates remain the starting point. A new dependency, long-lived process, protocol, schema or persistence layer needs measured justification and a reviewed design before adoption.

## Release Scope

#708 is a v0.6.0 must-have under release owner #493, with no implementation in RC4. This proposal is for planning review. Implementation starts only after published specification, exact issue-task mapping and native milestone/parent/blocker admission.

## Non-Goals

- A one-file-only optimization, larger timeouts as the sole fix, or speculative extra parallelism.
- Git-only change discovery, size/mtime-only freshness, or an unannounced background service.
- Weaker source evidence, cancellation, resource bounds, root isolation or atomic publication.
- Claiming a measured watch-once speedup or a settled cross-invocation algorithm before the planned experiments.
