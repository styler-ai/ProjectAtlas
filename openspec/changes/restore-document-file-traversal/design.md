## Context

The Markdown fact extractor already admits only parser destinations and complete repository-path code spans, resolves them through exact indexed identities, and records occurrence spans. RC1 then chooses the nearest heading entity as the canonical relation source. Detailed file traversal starts from the file entity and filters for `documents` before expanding, so it cannot cross a `contains` edge to discover a heading-owned relation. The service test avoided that real shape by constructing a file-owned relation directly.

Coverage has a second ambiguity: `complete` with zero covered and zero omitted means “the supported extraction finished with no candidates,” but the domain and wire layers expose it only as an ordinary complete row. The existing constrained schema already retains every fact needed to derive a distinct trusted zero-candidate state.

The affected ownership boundaries are the existing core graph types, prepared graph persistence, CLI graph projection, shared detailed-relation service, and CLI/MCP/release E2E adapters. No schema change, new crate, or dependency is justified.

## Goals / Non-Goals

**Goals:**

- Make a document file the stable graph source for its exact static documentation references.
- Preserve heading-level explainability through existing heading entities and exact occurrence spans.
- Deduplicate repeated document-to-target references without losing bounded occurrences.
- Expose a trusted `no_candidates` coverage state from the existing durable zero-count fact.
- Preserve full/incremental equivalence, typed unresolved outcomes, exact-root isolation, and bounded indexed reads.

**Non-Goals:**

- Infer implementation relationships from prose, names, similarity, embeddings, or an LLM.
- Store both file-owned and heading-owned copies of the same relation.
- Add a query-time recursive heading projection, inverse relation table, graph service, visualization, or semantic layer.
- Change source-derived call/import/configuration truth or the default legacy relation-family universe.

## Decisions

### Canonical document edges start at the document file

`project_document_rows` will use the already-staged file entity as the source of every admitted candidate. The existing relation occurrence retains the exact file/span evidence, and existing heading symbols still provide the enclosing document structure for follow-up slices.

This follows note-level graph semantics: the stable navigable object is the document, while headings locate evidence inside it. It also causes repeated references from several headings to the same target to share one logical key and accumulate bounded occurrences.

Alternatives rejected:

- Duplicating file and heading edges would inflate degree, split pagination, and violate the single-canonical-fact contract.
- Teaching every adjacency query to expand a file into all same-path headings would complicate continuation identity, query plans, work accounting, and multi-depth traversal for behavior that can be represented once at publication.
- Keeping heading ownership and traversing `contains` before applying the requested family would make exact-family queries depend on unrelated relation families and depth budgets.

### Add a document-specific trusted zero-candidate coverage state

Core `CoverageState` gains `NoCandidates`, serialized publicly as `no_candidates`. It is valid only with `covered = 0`, `omitted = 0`, no reason, and no reached limit. It is trusted because extraction completed and omitted nothing; it remains distinct from positive `Complete`, `Partial`, `Failed`, `Ignored`, `Oversized`, `Quarantined`, and `Stale`.

The state currently requires the `documents` relation. That matches the proved
product need and prevents schema-18 normalization from silently reclassifying
unrelated complete-zero parser or relation coverage.

Document coverage selects `NoCandidates` only when Markdown fact extraction is complete and admitted zero candidates. A document containing an admitted candidate that cannot resolve still emits a typed unresolved `documents` relation and uses normal complete/partial coverage for the candidate set.

SQLite keeps the existing schema-18 representation: `relation = documents`, `state = 'complete'`, `covered = 0`, `omitted = 0`, `total = 0`, with no reason or limit. Prepared writes normalize `NoCandidates` to that admitted durable row; reads reconstruct `NoCandidates` only from the exact count invariant on a `documents` row. State filters use the same relation-and-count predicate, so `complete` excludes only document zero-candidate rows while unrelated complete-zero coverage remains `complete`. This avoids a table rebuild and preserves every key, index, relation, purpose, generation, and compatibility boundary.

The derived-index projection contract advances independently of schema 18. An RC1 database whose stored fingerprint still describes heading-owned document rows returns the existing typed publication-contract mismatch instead of serving those stale rows as current. One full refresh republishes file-owned relations and the current fingerprint while preserving authored purpose and project identity.

Coverage discovery remains a bounded prepared read. Because schema 18 has no dedicated public-state column and changing an existing index in place would require a migration, the service reuses SQLite progress cancellation and applies a two-second discovery ceiling instead of adding speculative storage. The scale regression cancels a worst-case zero-candidate lookup after bounded virtual-machine work.

### Test the shipped shape at each real boundary

The projection unit test will prove file ownership, deduplication, occurrences, and `no_candidates`. SQLite write/read/reopen and filtered-discovery tests will prove the normalized state without a schema migration. The service integration fixture will prove exact zero-edge traversal still returns anchor coverage. CLI/MCP tests will execute full scan and exact file-anchored outbound/inbound calls. The mandatory hosted release E2E will repeat the behavior from the packaged candidate.

## Risks / Trade-offs

- [Heading-owned relation consumers observe a file source after re-scan] -> Preserve exact occurrence spans and heading entities; document the file as the canonical navigation anchor and test inbound/outbound next calls.
- [Normalized storage conflates document zero-candidate and unrelated complete-zero rows] -> Require the `documents` relation plus exact zero counts, apply the same predicate to state filters, and prove write/read/reopen plus both filter paths.
- [A long document or worst-case coverage filter produces excessive work] -> Admit only existing static candidates, deduplicate by canonical relation key, retain existing candidate/occurrence/page/output ceilings, apply the two-second coverage deadline, and assert indexed query plans plus progress cancellation.
- [Incremental publication retains stale heading-owned rows] -> Reuse the existing affected-path replacement transaction and compare full versus incremental graph snapshots after add/change/rename/delete/case/ignore transitions.
- [No-candidate coverage is mistaken for untrusted absence] -> Map it to trusted health semantics and expose it explicitly in CLI/MCP coverage projections.

## Publication Plan

1. Keep schema 18 and its constrained trusted zero-count coverage representation.
2. Publish file-owned document relations and normalized `no_candidates` coverage only in a complete new graph generation.
3. On publication failure, roll back and retain the prior complete generation without partial rows.
4. RC1 databases remain schema-compatible, but their older projection fingerprint requires one typed full refresh before graph reads; that refresh replaces the derived heading-owned facts without changing authored state.

## Open Questions

None. Heading-level semantic relationships beyond exact static link evidence remain later-version backlog work.
