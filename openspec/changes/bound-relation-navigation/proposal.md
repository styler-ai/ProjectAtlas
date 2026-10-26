# Proposal

## Why

Relation navigation should help an agent select its next source read. The default legacy route currently accepts more than 10,000 rows, ignores the detailed-view output budget and materializes the requested list; an installed reproduction returned 10,001 rows and 2,835,153 JSON bytes despite a 4,096-byte request.

## What Changes

- **BREAKING:** make CLI and MCP ordinary relation navigation use the existing detailed view, with an exact file/symbol anchor, ten rows and a 32 KiB output default. Missing anchors receive actionable narrowing guidance. Existing detailed continuation and explicit larger bounded requests remain available.
- Keep explicit analysis opt-in and preserve its existing defaults (50 rows, 256 KiB output), profile/mode semantics and bounds.
- Keep explicit `--view legacy` / `view: legacy` as a compatibility route for complete small arrays, retaining its omitted row limit of 50. Enforce at most 200 requested rows, a 32 KiB default and 256 KiB hard encoded-output ceiling, bounded decoded/intermediate state and cancellable SQLite work. Refuse oversized or incomplete compatibility results with guidance to the detailed route instead of silently truncating them.
- Bound candidate discovery, ordering, row decoding and response composition separately. Preserve filters and exact relation facts; use the existing SQLite/read-budget and detailed traversal owners rather than a second graph engine.
- Distinguish returned rows, known totals, unknown totals, reached budgets and resumable continuation. Do not compute an unbounded exact total to decorate a bounded page.
- Update help, MCP schemas, plugin/user guidance and real CLI/MCP regression coverage together.

## Capabilities

### New Capabilities

- `bounded-relation-navigation`: useful default relation pages and explicitly bounded compatibility reads, including work, bytes, truthful completeness, continuation and root-safe failure.

### Modified Capabilities

None. The current published capability inventory contains token-impact reporting; existing relation implementation and in-flight graph changes remain the mechanisms reused here.

## Impact

Owning issue: #721, complexity high, scheduled for `v0.5.1-00` under release acceptance #715. Its specification, task mapping, live issue packet and native release graph have
passed independent review, strict OpenSpec and scoped IssueOps. #708 and #721 have no direct implementation dependency; #715 requires both.

Affected owners: CLI argument/routing and exact output composition, MCP schemas/routing and selected-project envelopes, the existing detailed relation service, and SQLite legacy relation reads. Enable the existing rusqlite dependency's `limits` feature for native value-length admission; no new crate, database schema or stored relation identity change is proposed. Small explicit legacy consumers retain the complete array shape; default-view and over-budget legacy behavior changes are deliberate and documented.

## Non-Goals

No relation deletion, hidden filtering or truncation, index/build changes, new database, graph-query language, speculative cache, unbounded totals, or replacement continuation framework. Full release/platform acceptance remains owned by #715; this issue supplies its focused high-degree, compatibility and query-plan proof.
