## 1. Contract And Architecture

- [x] 1.1 Map issue #461 to this change, synchronize its objective checklist, and keep RC2 scope limited to exact file-scoped document traversal plus explicit zero-candidate coverage.
- [x] 1.2 Update the focused classified-documentation graph and publication/failure diagrams only where file ownership or normalized zero-candidate coverage changes the durable flow; render every changed Mermaid block and inspect it visually and semantically.

## 2. Core And SQLite Coverage Foundation

- [x] 2.1 Add the closed `no_candidates` core coverage state with zero-count invariants, trusted semantics, serialization, health counts, and negative contract tests.
- [x] 2.2 Normalize `no_candidates` through the existing schema-18 complete-zero row, reconstruct it only from exact zero-count invariants, and distinguish positive-complete versus zero-candidate discovery filters without changing relation storage.
- [x] 2.3 Prove schema-18 write/read/reopen, positive-complete and zero-candidate filter separation, constraint and corruption refusal, rollback/recovery, query-plan ownership, cancellation, and bounded SQLite work.

## 3. Canonical Document Projection And Navigation

- [x] 3.1 Make the owning document file the canonical source of every admitted static `documents` relation while retaining exact heading entities, occurrence spans, resolution dependencies, typed unresolved reasons, and self-edge rejection.
- [x] 3.2 Emit `no_candidates` only for complete zero-candidate Markdown extraction and preserve partial, failed, ignored, oversized, quarantined, stale, and admitted-but-unresolved behavior.
- [x] 3.3 Update detailed relation composition so file-outbound `documents` and target-inbound `documented_by` remain one canonical bounded fact with classified endpoints, exact next calls, pagination, occurrence, selection, cancellation, and output compatibility.
- [x] 3.4 Preserve full/incremental equivalence after add, edit, rename, delete, case, fragment, ignore, and failed-publication transitions without speculative prose or similarity edges.

## 4. Mandatory Regression And Release Proof

- [x] 4.1 Add production-shaped projection, core, SQLite, and service tests for heading-contained file traversal, repeated-target deduplication, exact occurrences, inbound symmetry, unresolved outcomes, no-static-target coverage, and long-prose negative behavior.
- [x] 4.2 Add real CLI/MCP full-scan regressions for exact-root file-outbound, source-inbound, unresolved, no-candidate, wrong-root, missing-index, and no-implicit-mutation behavior; wire them into mandatory CI and the packaged holistic RC E2E.
- [x] 4.3 Run focused tests, `cargo fmt --check`, `cargo check --workspace --all-targets --all-features`, warnings-denied workspace Clippy, full workspace/all-feature and doc tests, warnings-denied docs, strict OpenSpec, IssueOps checklist parity, ProjectAtlas lint, and representative high-reference performance/query-plan checks with explicit timeouts.
- [ ] 4.4 Update durable graph, database, upgrade, agent-integration, release, privacy, and failure guidance; resolve or disposition every live review/automated finding and verify the exact packaged RC2 behavior without displacing v0.4.4 Latest.
- [x] 4.5 Review the final implementation against the architecture diagrams, update the diagrams or implementation until they agree, or reconfirm the reasoned N/A.
