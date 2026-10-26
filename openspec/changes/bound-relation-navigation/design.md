# Design

## Context

See `proposal.md` for the installed oversized-response reproduction and deliberate compatibility changes. Current CLI/MCP defaults select legacy arrays with 50 requested rows. `AtlasStore::load_symbol_relations` projects all requested text columns before returning a vector; optional substring filters and ordering can inspect or sort much more than the returned rows. The existing detailed service already owns normalized adjacency, multiple independent budgets, coverage, exact continuation and adapter-envelope composition.

The issue packet, OpenSpec ownership, sole release parent and native blocker graph
agree; independent specification review, strict OpenSpec and scoped IssueOps passed.

## Goals / Non-Goals

Keep one existing detailed traversal/continuation owner and one storage-owned bounded compatibility reader. Bound work before large row decoding as well as output afterward. Preserve successful small legacy arrays and existing graph facts. Do not change storage schema, relation identity, indexing, normal summary sampling, parser output or unrelated database accessors.

## Decisions

### Default to existing anchored detailed navigation

Use ten rows and 32 KiB as the shared CLI/MCP default. Keep existing occurrence, traversal, evidence and absolute detailed-request limits unless a focused test demonstrates a required correction. Require the existing exact file anchor; optional symbol selectors refine it. Missing-file guidance names a bounded file-discovery route and an example anchored relation request. Preserve explicit detailed/analysis semantics and returned continuations.

This changes an intentional public default, so help, schemas, examples, plugin instructions and compatibility tests change together. Keeping legacy as the normal route would leave useful continuation and completeness opt-in, while building another compact graph route would duplicate existing behavior.

### Make explicit legacy output complete or refuse it

Admit requested rows from 1 through 200 and retain 50 when explicit legacy view omits the limit; use default encoded output 32 KiB and hard output ceiling 256 KiB. Detailed view defaults to ten rows/32 KiB; explicit analysis keeps its existing 50-row/256-KiB defaults and semantics. Resolve omitted values after selecting the view, without confusing omission with an explicit ten or fifty. Observe at most one sentinel row beyond the requested count; more eligible data causes a typed refusal that names detailed navigation. Do not clamp an oversized caller request silently. Successful responses keep the established complete array fields and order. The array length is then the known complete eligible count; partial arrays cannot masquerade as complete results.

Caller `output_bytes` is effective in both views. Legacy additionally honors tighter applicable deadline/intermediate bounds, while graph traversal/continuation controls unsupported by legacy receive errors instead of being ignored. Existing small complete legacy clients retain their shape. Previously partial or oversized legacy consumers must explicitly use bounded detailed continuation; this is preferable to adding an incompatible metadata envelope around the old array or discarding unreported rows.

### Bound the SQLite path before wide materialization

Add a concrete storage-owned compatibility budget/read method using existing
cancellation/progress-hook and bounded-field facilities. Candidate discovery selects
only stable row identifiers with the existing file/LIKE filters and a requested-plus-one
limit, without SQL ordering. A sentinel refuses before hydration. Otherwise the set
is complete: hydrate at most 200 rows through one prepared lookup, check raw value
sizes before owned decoding, then sort that complete bounded set by
path/line/source/target/row identity. Keep candidate selection and hydration in one
read snapshot. This avoids wide temporary SQL sorting entirely.

The operation shares a 100,000 SQLite instruction ceiling, a maximum 1,000 ms
cooperative deadline and 1 MiB decoded/intermediate state, including candidate IDs,
result structures and pattern storage. Use an instruction-level progress callback
so many short prepared operations cannot evade the cumulative budget. Enable the
existing rusqlite `limits` feature, apply a temporary native value-length ceiling
and tighten it to remaining bytes before hydration. Restore native limits and the
progress hook through the storage-owned RAII guard.

Legacy query text is limited to 1,024 UTF-8 bytes before allocating `%query%`;
set a temporary SQLite LIKE-pattern ceiling of 1,026 bytes. Preserve existing `%`/`_`
wildcards and SQLite case semantics within the cap. Longer patterns receive typed
narrowing guidance. This bounds work inside a single LIKE instruction, which the
progress callback cannot preempt; time limits remain cooperative. No unrestricted
exact count or new index is required. Inspect exact-file and broad query plans on
the high-degree fixture and verify neither candidate query uses a temporary sort.

Keep the existing low-level accessor and unrelated summary sampling semantics intact; both user-facing relation adapters select the bounded compatibility method. Verify that every legacy spelling/route reaches that method. Do not add an index speculatively: inspect file-scoped and broad `EXPLAIN QUERY PLAN` behavior on the high-degree fixture. ID-only candidate admission plus bounded complete-set sorting is the design; a measured need for a covering index would require its write/storage trade-off and migration to be reviewed before implementation.

The expected model is O(admitted query instructions) inspection, at most requested-plus-one candidate identifiers and O(admitted raw/decoded bytes) composition, not O(all stored relation widths). Detailed requests retain their existing bounded adjacency model.

### Account for the actual selected adapter payload

Reuse existing detailed exact-envelope composition and byte accounting. Legacy first bounds decoded storage state, then encodes the complete successful payload once and checks its actual UTF-8 encoded bytes at the CLI JSON/TOON or MCP tool-text boundary, including selected-project/audit wrappers. Do not treat a byte estimate or a pre-wrapper array length as the emitted size. Small typed errors retain the normal bounded diagnostic contract even when an invalid caller budget is too small to encode an error.

Keep returned-row counts, total knowledge, coverage and budget exhaustion distinct. Detailed continuation remains owned by the existing service and bound to project, publication generation, anchor, filters and budgets. A refusal never changes stored relations or invents a continuation from incomplete legacy state.

### Preserve current project and read authority

Use the existing root/worktree admission and verified read snapshots. Relation calls do not initialize, reset or rebuild state implicitly. Cancellation and failed row conversion terminate the entire result, release resources and permit a subsequent request; no partial successful array escapes after an error. Existing wrong-root, stale-generation, missing-index and alias-isolation contracts remain active.

## Architecture Diagrams

N/A: request, graph traversal and storage ownership remain in the existing CLI/MCP, detailed relation service and SQLite modules. This change adds admission/default/output policy inside those boundaries and reuses their existing continuation and verified-read flows; it introduces no new architecture owner or communication path.

## Verification

- A high-degree fixture independently checks candidate rows/instructions, bytes decoded/retained and final adapter bytes. Include the exact 10,001-row reproduction, sparse matches that require excess scan/sort work, oversized text cells and incomplete legacy results.
- Real CLI JSON/TOON and MCP tests cover ordinary defaults, unanchored guidance, complete legacy shape, explicit limits, a smaller byte budget, zero/oversized values, filters, repeated continuation, cycles/dense hubs, cancellation and hook cleanup.
- Existing evidence, coverage, ambiguity, source freshness, generation/root mismatch and worktree tests protect the reused detailed owner. Complete results and deterministic refusal are stronger proof than a small output snapshot alone.
- Use locked workspace, OpenSpec, IssueOps, affected-check and existing release routes. Preserve the E2E inventory/source-contract artifacts when adding tests. #715 owns the broader installed platform/surface acceptance; do not invent another benchmark campaign.

## Risks / Trade-offs

- Changed defaults break consumers expecting implicit arrays → label the change, retain explicit small legacy arrays, update help/schema/examples and test both adapters.
- Small output hides a large sort or substring scan → shared SQLite instruction/deadline limits, ID-only candidates, raw byte admission and causal high-degree/query-plan checks.
- Refusing a legacy result hides useful data → include exact anchored detailed guidance; existing continuation keeps remaining eligible data reachable.
- Budget state leaks into the next SQLite query → storage-owned RAII restoration and a succeeding query after each injected stop/failure.
- Metadata/cursor/audit wrappers exceed a nominal limit → reuse exact adapter-envelope accounting, including Unicode/escaping and minimal-result refusal tests.
- A shared default change drifts between adapters → one owning policy, real CLI/MCP behavior checks and complete guidance reconciliation.

## Migration Plan

No persistent migration or graph rewrite. Ship the deliberate default and compatibility-policy changes in 0.5.1-rc1 for testing, with explicit legacy migration guidance. Rollback restores adapter admission/default behavior without altering stored graph facts. #708 is an independent build/refresh issue; #715 is blocked on both accepted children and owns integrated publication. Stable promotion remains outside this issue.

## Dependencies / Cross-Issue Impact

#721 is a direct child of release acceptance #715 in milestone `v0.5.1-00`; #715 is directly blocked by #708, #718 and #721. #708 and #721 have no direct blocker relationship: their implementation owners are separate, although integration must reconcile shared CLI/MCP source, test inventories and public guidance. #718 is accepted historical documentation work. No 0.6.0 issue is a prerequisite. The #721 issue-map slice owns tasks 1.1 through 3.1 of this change.

## Open Questions

None.
