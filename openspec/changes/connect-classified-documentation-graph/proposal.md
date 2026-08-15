## Why

ProjectAtlas already discovers and summarizes repository Markdown, but it does not classify every indexed file for agent-facing use or connect explicit documentation references to the source they describe. Agents can therefore read documentation as if it were source evidence, miss the inverse path from source to its specification, or broaden a search because graph navigation has no trusted document relation.

Issue #440 is planned for v0.4.5 beside #430. Its parsing, classification, graph, SQLite, compatibility, performance, failure, and exact-worktree routing contracts must remain explicit.

## What Changes

- Add one closed content classification to every indexed file: `source`, `documentation`, `configuration_data`, `other_text`, or `opaque`.
- Derive known classifications from the existing language registry; classify otherwise eligible valid UTF-8 as `other_text` and invalid/binary content as `opaque` without guessing generated-file status from names.
- Parse bounded Markdown/MDX headings and explicit repository-local references with the already installed `pulldown-cmark` parser and exact byte/line selectors.
- Add `heading` symbols and one canonical `documents` graph relation; expose `documented_by` only as the inbound view of the same stored fact.
- Retain typed unresolved evidence for missing, ignored, outside-root, case-conflicting, unsupported, or non-static targets without leaking absolute/private paths.
- Add source, documentation, and both selection to affected service/CLI/MCP paths while preserving the exact legacy candidate universe and ranking when selection is omitted.
- Store classification in one constrained, indexed active-atlas table and publish it transactionally with the existing complete derived generation and graph relations.
- Keep every checkout's classification/document facts in its own ignored writable database selected by exact `project_path`; never merge sibling graph generations.
- Preserve a seamless v0.4.4 upgrade for ordinary checkouts and linked worktrees, including offline local rebuild.
- Update the shipped ProjectAtlas skill, user guidance, architecture documentation, and real multi-platform/worktree tests.

## Capabilities

### New Capabilities

- `repository-content-classification`: Closed registry-owned file roles, persistence, projections, and backwards-compatible selection.
- `documentation-graph-relations`: Bounded Markdown heading/link extraction, exact local resolution, canonical document relations, typed unresolved evidence, and incremental invalidation.
- `classified-agent-navigation`: Classified files/search/summary/purpose/relation/analysis results with CLI/MCP parity and exact next calls.
- `worktree-classified-navigation`: Branch-local classified document refresh, structural worktree status, exact-root isolation, and v0.4.4 upgrade compatibility.

### Modified Capabilities

- Existing repository graph publication gains `heading` symbols and `documents` relations without changing legacy relation results when the new family is not requested.
- Existing text search, file ranking, summaries, purposes, relation traversal, and graph analysis gain additive classification fields and optional content selection.

## Impact

- `projectatlas-core`: content classification/selection, heading symbol, document relation, typed resolution contracts, and registry projection.
- `projectatlas-db`: one append-only active-atlas migration, constrained classification storage/indexes, prepared/batched access, generation publication, reverse relation queries, and upgrade/recovery coverage.
- `projectatlas-symbols`: bounded Markdown/MDX heading and explicit-reference extraction using the existing parser dependency.
- `projectatlas-service`: shared selection semantics, compatibility-preserving ranking/search/traversal, summaries, relations, analysis, and next-call projection.
- `projectatlas-cli`: scan/publication integration plus CLI/MCP schemas and output classification.
- #430 structural worktree discovery, exact-root routing/status, shipped-skill guidance, and the joint holistic worktree E2E.
- `docs/projectatlas-3-architecture.md`, the shipped `projectatlas` skill, user/upgrade guidance, and issue #440.
- No new crate, database file, runtime service, network dependency, vector store, or generic document framework.
