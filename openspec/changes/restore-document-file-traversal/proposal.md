## Why

ProjectAtlas 0.4.5-rc1 can persist exact Markdown reference relations on heading entities while a file-anchored `documents` traversal reports an exact zero result. The release-candidate graph therefore fails its primary documentation-to-source navigation contract, and a document with no admitted static reference has no distinct typed coverage disposition.

## What Changes

- Make a documentation file anchor expose the canonical `documents` relations proven anywhere in that document while retaining exact occurrence spans and heading context.
- Preserve one stored outbound relation and derive `documented_by` only as its inbound view.
- Represent a successfully inspected document with no admitted static target through an explicit typed coverage state instead of ordinary complete-zero coverage.
- Add production-shaped full-scan, incremental, service, CLI, MCP, and packaged E2E regressions to mandatory CI.
- Keep the change ready for immediate RC2 implementation.

Non-goals:

- Do not infer implementation ownership from prose, names, similarity, embeddings, or an LLM.
- Do not connect a document to every source file it mentions conversationally.
- Do not duplicate inverse relations, add a new graph store, add a crate or dependency, or add later-version graph visualization and semantic-analysis features.

## Capabilities

### New Capabilities

- `document-file-graph-coverage`: File-scoped exact documentation traversal, canonical inverse navigation, occurrence provenance, and explicit no-static-target coverage.

### Modified Capabilities

None.

## Impact

- Affected code: Markdown graph projection, graph coverage contracts and persistence, detailed relation service projection, CLI/MCP rendering, and release E2E fixtures.
- Affected data: additive `no_candidates` domain/wire state derived from the existing schema-18 trusted zero-count coverage row; no schema migration or new persistent column is required.
- Compatibility: existing resolved, ambiguous, unresolved, ignored, outside-root, case-conflict, unsupported, pagination, selection, and omitted-family behavior remains compatible; the projection-contract fingerprint forces one typed full refresh of RC1-derived graph rows without migrating schema 18 or authored purpose.
- Dependencies: no new crate or package.
