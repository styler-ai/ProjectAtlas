## Why

ProjectAtlas 0.3.26 gives agents a fast atlas-first route from repository overview to exact source, but its index is still mostly file-local. Cross-file identity, dependency-aware freshness, language capability truth, and compact relationship context must become first-class if ProjectAtlas is to keep agents out of broad source reads as repositories grow.

Version 0.4 keeps the successful 0.3.26 workflow and advances the intelligence behind it. The goal is not a larger human-facing graph product. The goal is an agent reaching correct source context sooner, with fewer calls, fewer full-file reads, less backtracking, and lower total context use.

## What Changes

- Add a typed graph of the selected live local source state, with stable project, file, symbol, package, relation, resolution, coverage, and generation identities; current saved bytes remain authoritative over optional VCS context.
- Publish full and incremental structural data atomically while preserving authored purposes, settings, and telemetry.
- Keep reviewed folder/file purposes as the responsibility layer for navigation, then combine them with current content, parser trust, and graph proximity rather than replacing them with edge counts.
- Let supported agent hosts run a quiet bounded low-reasoning purpose curator beside the main task, using a task-scoped queue without adding purpose-maintenance chatter to normal navigation responses.
- Make normal index-backed reads, watch, and incremental refresh freshness-aware so results stay current after edits, offline changes, and restarts without repeated whole-repository work.
- Generate one versioned language capability registry with an explicit accepted capability set, honest detected, parsed, symbols, semantic, and benchmarked tiers, and no silent loss of accepted language breadth.
- Enrich existing folder, file, search, summary, and relation calls automatically with compact graph context and next-call guidance.
- Keep deterministic lexical search as the always-available baseline; optional acceleration or semantic capabilities must preserve explicit behavior and remain outside the default core when unused.
- Add a versioned accepted relation-family inventory plus bounded coverage, architecture, complexity/bottleneck candidate, VCS-aware impact, and trace inspection where existing calls cannot express the result cleanly.
- Allow cross-repository analysis only for explicit indexed roots supplied to one read-only call.
- Evaluate agent workflows against 0.3.26 using correctness, time to useful context, tool calls, file reads, backtracking, output bytes, and context tokens.
- Keep implementation slices lean: one meaningful behavior test may cover several tasks, ordinary locked workspace gates remain authoritative, and no task receipts, SHA ledgers, rendered evidence, or unique test-per-task scheme is introduced.

## Capabilities

### New Capabilities

- `repository-knowledge-graph`: Typed stable repository identities, relationships, source context, resolution state, coverage, and generation ownership.
- `incremental-index-integrity`: Atomic publication, dependency-aware refresh, full/incremental equivalence, and authored-data preservation.
- `language-intelligence-registry`: Generated capability truth, deterministic language selection, parser ownership, and independently gated semantic support.
- `graph-retrieval-and-analysis`: Automatic graph-aware navigation plus bounded relations, architecture, impact, and trace services.
- `optional-intelligence-packs`: Explicit parser and semantic capability lifecycles that do not burden default-core startup or indexing.
- `cross-repository-intelligence`: Explicit-root, call-only, read-only federation with project-qualified identities.
- `repository-intelligence-benchmarks`: Reproducible compatibility, correctness, resource, and agent-efficiency decisions.

## Impact

- `projectatlas-core` owns shared typed graph and capability contracts.
- `projectatlas-db` owns migrations, generation publication, indexed graph storage, and integrity checks.
- `projectatlas-symbols` owns registry-driven extraction and language-specific resolution.
- `projectatlas-fs` and runtime composition own safe discovery, freshness planning, and bounded work.
- `projectatlas-service` owns ranking, summaries, relations, analysis, cursors, and optional federation.
- CLI and MCP remain thin compatible adapters over the same services.
- No new crate is added without a real independently consumed ownership boundary.
