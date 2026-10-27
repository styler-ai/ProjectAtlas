# Proposal

## Why

Indexing and refreshing a large project can take too long and hit a deadline. Version 0.5.1 must improve the complete path: initial indexing, full reindexing, symbol/graph rebuilding and refreshes that update the actual changes and affected data.

## What Changes

- Set separate, measured improvement targets for initial indexing, full `scan`, explicit symbol/graph rebuilding, cold CLI refresh, warm MCP, unchanged projects and small or large change sets. A faster watch-once path alone does not complete this issue.
- Settle one shared operation plan: trusted no-op, affected-data update or required full work. Use lightweight ordered source comparison, reuse valid source/parse evidence and remove repeated whole-project work in the existing owners. Focused measurements verify this architecture; they are not an open-ended search for it.
- Reuse the existing watcher and source observer safely. With trustworthy change evidence, update all changed sources and affected data, and make an exactly reconciled unchanged refresh avoid derived work. Zero-source-work warm admission is explicitly deferred to unscheduled #725 by the accepted RC1 scope.
- Reconcile uncertain history, offline edits and relevant configuration changes exactly. Preserve the complete results and force/rebuild semantics of initial and full indexing.
- Choose bounded default work budgets from the work required. Preserve explicit caller limits, cancellation, atomic publication, authored purposes, native identity and safe retry.
- Prove each improvement with the optimized installed product, normal telemetry, realistic large-project fixtures, work counts and resource measurements on supported platforms.

## Capabilities

### New Capabilities

- `incremental-refresh`: complete large-project indexing and rebuild performance, trustworthy change-set refresh, and bounded work budgets through the existing CLI/MCP routes.

### Modified Capabilities

None. Existing freshness, root isolation, publication and caller-limit guarantees remain constraints.

## Impact

The existing init, scan, symbol-build and watch adapters; filesystem and source-observation owners; parsing; SQLite comparison/staging/publication; and performance/E2E tests. Continuous watching shares the improved owners. Keep the seven crates and current dependencies as the starting point. Any new dependency, persistent component, protocol or schema needs measured justification and a reviewed compatibility design before adoption.

## Release Scope

#708 is a high-priority 0.5.1 must-have under release owner #715. Move it out of #493's 0.6.0 child/blocker set. This change publishes the complete plan; implementation and measured speedups remain pending. Start with the baseline and cold-CLI design gate after specification review and native milestone/parent/blocker admission.

## Non-Goals

- A one-file-only or watch-only optimization, larger timeouts as the sole fix, or speculative extra parallelism.
- Git-only change discovery, size/mtime-only freshness, or an unannounced background service.
- Weaker source evidence, reduced parser/document/graph coverage, cancellation, resource bounds, root isolation or atomic publication.
- Treating a skipped required rebuild as a speedup, or claiming measured gains or a settled cross-invocation algorithm before the planned experiments.
