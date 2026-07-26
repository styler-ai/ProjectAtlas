## Context

`TokenOverview` in `projectatlas-core` is the authoritative serialized token report. `projectatlas-db` supplies bounded observed and modeled telemetry, `projectatlas-service` selects the report through one captured project binding, and the CLI, MCP, TOON/JSON, and Ratatui paths only adapt that typed result. The final v0.4 navigation benchmark is a versioned JSON artifact with 45 retained scheduled trials, matched v0.4, frozen-v0.3.26, and plain arms, per-run navigation audits, aggregate distributions, explicit failed setup runs, and non-causal provider counters.

The benchmark is publication evidence, not project data. Importing it into SQLite would create a second authority plus schema, migration, retention, and drift responsibilities without improving normal telemetry.

## Goals / Non-Goals

**Goals:**

- Validate one optional repository-relative benchmark artifact under fixed size, count, identity, schema, and numeric bounds.
- Keep observed telemetry, modeled avoidance, and benchmark comparison as separate typed accounting layers.
- Expose identical comparison values through existing CLI JSON/TOON, MCP, and Ratatui paths.
- Preserve failed and unmatched trials and distinguish unavailable, failed, incompatible, partial, and compatible evidence.
- Keep the existing dashboard readable at normal and compact widths.

**Non-Goals:**

- Persisting benchmark data, changing the SQLite schema, or recording comparison reads as telemetry.
- Claiming provider-token causality or deriving benchmark-only navigation counters from live events.
- Adding another MCP tool, dashboard framework, background loader, or dependency.
- Re-running or rewriting the immutable task-7.6 result artifact.

## Decisions

### Attach a closed comparison state to the existing token overview

`projectatlas-core` will own closed serializable comparison-state, baseline-row, metric, and capability-contribution types. `TokenOverview` gains one backward-compatible defaulted comparison field. Adapters continue to serialize the same authoritative report rather than maintaining separate arithmetic.

A trait hierarchy or generic analytics model was rejected because the accepted artifact schema and report variants are closed for v0.4.

### Validate read-only evidence in the service

The overview request accepts an optional repository-relative benchmark path. The service resolves it beneath the already captured project root, rejects absolute, parent-escaping, non-file, and path-indirection inputs through the existing repository path boundary, then reads at most 8 MiB. Missing input produces `unavailable`; unreadable or malformed content produces `failed`; semantic mismatch produces `incompatible`; compatible evidence with retained failed or unmatched trials produces `partial`; fully matched evidence produces `compatible`. Path-boundary violations remain hard request errors.

The loader deserializes only required fields with private typed structs and ignores unrelated answer text. It validates:

- schema version `1`, exact v0.4 and frozen-v0.3.26 semantic identities, and a plain arm with ProjectAtlas disabled;
- bounded run, schedule, comparison, distribution-value, and MCP-call counts;
- unique and exactly retained scheduled run IDs, at least three repeats, and zero excluded trials;
- finite, nonnegative counts/timings/bytes and required matched candidate/baseline distributions, with exact-integer samples for count/byte/token fields, lossless JSON-integer bounds, and a seven-day wall-time ceiling;
- explicit failed-run accounting rather than converting failures to zero;
- non-causal provider metadata when provider counters are present.

The artifact digest and bounded public identities are retained; local filesystem paths, prompts, answers, and raw traces are not copied into the report.

### Derive two matched baseline rows and bounded capability rows

The service combines only workload groups that have completed trials in both compared arms. It reports medians and observed maxima for total/ProjectAtlas tool calls, productive and wrong folder/file/relation visits, broad/full reads, backtracks, gross and net navigation context, setup/runtime cost, persistent bytes, and wall-time break-even. Frozen-v0.3.26's failed huge-corpus setup remains an unmatched failure and makes that row partial; it is never treated as a successful zero.

Capability rows classify trace-completed v0.4 MCP calls into the existing durable navigation responsibilities:

- initial purpose and connection discovery;
- summary and exact slice compression;
- lexical search narrowing;
- symbol and relation navigation.

Rows report calls and emitted bytes and reconcile to the classified trace-completed MCP-call total. A completed trace status is not relabeled as semantic success, and the rows do not attribute token savings to individual tools because the artifact does not establish that causal split.

### Keep comparison rendering subordinate to the accepted token dashboard

The conservative token headline, file-read strip, observed/modeled composition, source table, calibration notes, and footer stay unchanged. A bounded agent-efficiency panel follows the existing accounting sections and shows comparison state plus two compact baseline rows. Normal width shows the principal call/read/navigation/context/runtime fields; compact width uses shorter labels and fewer columns without hiding state or failed trials. Provider counters remain in the typed payload under a descriptive-only label and are not added to the causal savings panel.

## Risks / Trade-offs

- **A large or hostile JSON artifact consumes excessive resources** → bound bytes and every repeated collection, reject non-finite or negative numeric values, and keep parsing synchronous and optional.
- **Same-version evidence is mistaken for exact-binary proof** → retain the benchmark runtime digest and source identity in the typed result and label compatibility as release-contract compatibility, not current-executable identity.
- **Failed frozen-baseline trials disappear from aggregates** → require scheduled/run retention and carry unmatched failures into row and top-level state.
- **Dashboard density regresses readability** → use two bounded baseline rows, compact fallbacks, deterministic buffer tests, and real visual review.
- **Adapters drift** → calculate and classify only in core/service types; CLI, MCP, JSON, TOON, and TUI render that result.

## Migration Plan

1. Add defaulted core report types so existing databases and serialized payload consumers remain compatible.
2. Add the optional CLI/MCP path and read-only service loader.
3. Add the dashboard panel, documentation, smoke/E2E coverage, and visual proof.
4. Rollback removes the optional loader and panel; no database or authored state requires migration.

## Open Questions

None.
