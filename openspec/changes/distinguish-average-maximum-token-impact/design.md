## Context

Token telemetry already separates observed compression from modeled navigation, preserves denominator and dedupe semantics, and derives `TokenOverview` from either raw events or bounded SQLite aggregate components. The current `tokens_avoided` headline adds measured savings to the full deduped modeled estimate, so a `directory_walk` baseline represents every file in the avoided scope.

The change crosses core aggregation, SQLite report composition, structured serialization, and Ratatui presentation, but it does not require new persistence. Existing aggregate dimensions retain `denominator_kind`, and existing counters retain deduped modeled baseline and payload components.

## Goals / Non-Goals

**Goals:**

- Preserve the existing all-files calculation as an explicit maximum.
- Add a transparent primary average estimate that applies a fixed 50% policy only to modeled `directory_walk` baselines.
- Keep measured compression, non-folder modeled categories, deduplication, buckets, trends, read counts, and stored events unchanged.
- Preserve old structured fields while adding explicit average and maximum fields.
- Keep the existing dashboard composition and add only one compact comparison bar.

**Non-Goals:**

- Inferring Codex behavior, claiming benchmark evidence, or configuring the percentage.
- Changing ordinary event admission, pruning, SQLite schema, migrations, query plans, tokenizer estimates, or search policy beyond one reserved directory-walk overflow discriminator required for correct reporting.
- Reworking the token TUI or trend model.

## Decisions

### Derive both values at the shared reporting boundary

`TokenOverview` will expose `average_tokens_avoided` and `maximum_tokens_avoided`. The existing `tokens_avoided` field remains present and becomes the compatibility alias for the primary average value. The existing deduped modeled value remains the all-files modeled component; one derived average modeled component is added so the formula is reproducible.

This keeps every CLI, MCP, JSON, TOON, and TUI caller on one calculation. Computing only inside the TUI was rejected because structured reports would remain ambiguous. Rewriting stored events was rejected because the policy is a report interpretation, not new evidence.

### Apply 50% only to modeled directory-walk baselines

For the deduped aggregate modeled folder baseline `without` and aggregate emitted payload `with`:

```text
average folder contribution = floor(without / 2) - with
maximum folder contribution = without - with
non-folder contribution      = without - with
```

Measured compression is added unchanged to both totals. Repeated baseline deduplication happens before the retained folder baselines are aggregated, so the aggregate folder scope is halved once and every emitted Atlas payload remains charged. Integer division rounds down to avoid overstating the average and matches both raw-event and bounded SQLite aggregate reports without adding persisted policy state.

Using half of the already-computed savings was rejected because that would also discount the Atlas payload. Applying 50% to `selected_candidates` was rejected because the requested adjustment is limited to all-files folder navigation and search remains unchanged.

### Reuse existing SQLite aggregates and preserve the bounded discriminator

The report query already groups rows by dimensions including `denominator_kind`, retains each signed deduped modeled delta, and retains the aggregate emitted modeled payload. The database adapter can reconstruct the aggregate retained folder baseline as `deduped_delta + emitted_payload`, apply the 50% policy, and compose `TokenAccountingTotals`; no column, table, index, migration, transaction, WAL, pruning, or recovery behavior changes.

The existing generic overflow dimension erases `denominator_kind` at dimension capacity. New directory-walk overflow is therefore routed to one additional reserved dimension that retains only this policy discriminator; ordinary dimensions and every other counter remain bounded as before. Predecessor generic overflow rows cannot be reconstructed exactly, so their modeled contribution remains at maximum and the structured `average_policy.evidence` label reports `fixed_policy_estimate_unclassified_overflow_uses_maximum` instead of implying an exact 50% classification.

### Preserve the dashboard and make the distinction visible

The current hero will be relabeled for average tokens avoided and use the average value throughout its equation. One compact maximum bar immediately below the primary comparison will show the current all-files result and explain that average uses 50% of folder scope while maximum uses all files. Existing sections, themes, atlas side panel, narrow fallback, and non-folder source rows remain intact.

## Risks / Trade-offs

- [A fixed 50% policy can be mistaken for measured behavior] → Label it as a policy estimate in structured metadata and visible TUI copy; do not connect it to benchmark evidence.
- [Average savings can be negative when the unchanged Atlas payload exceeds half the baseline] → Preserve signed arithmetic and the existing negative-state styling instead of clamping.
- [Raw-event and SQLite-derived reports can drift] → Use the same denominator predicate and component formula at both report boundaries, with a real SQLite round-trip test.
- [Existing generic overflow rows have already lost their denominator] → Preserve their numeric maximum fallback, label the policy evidence as unclassified, and retain the discriminator in one bounded overflow row for future writes.
- [A new bar can crowd narrow terminals] → Reuse the existing panel and responsive layout, assert text and styles with `TestBackend`, and inspect real screenshots at desktop, common, and narrow widths in dark, light, and terminal themes.

## Migration Plan

Ship the derived fields and presentation in v0.4.4 without a database migration. Existing databases immediately produce both values from their retained aggregates; predecessor generic overflow remains an explicitly labeled maximum fallback because its denominator is no longer recoverable. New writes may add one reserved directory-walk overflow dimension using the existing table and uniqueness contract. Older consumers continue to receive all existing fields; new fields use deserialization defaults for compatibility with older payload fixtures. Rollback is a binary downgrade because no schema or stored-event rewrite occurs.

## Open Questions

None. The v0.4.4 policy is fixed at 50% and intentionally described as modeled rather than benchmark-derived.
