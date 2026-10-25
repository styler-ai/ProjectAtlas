## Why

ProjectAtlas 0.3.26 gives agents a fast atlas-first route from repository overview to exact source, but its index is still mostly file-local. Cross-file identity, dependency-aware freshness, language capability truth, and compact relationship context must become first-class if ProjectAtlas is to keep agents out of broad source reads as repositories grow.

Version 0.4 is an additive update to the successful 0.3.26 workflow and contracts, not a replacement. It preserves the purpose-led funnel, generated-versus-approved purpose distinction, parser behavior, exact retrieval, and compatible CLI/MCP requests while advancing the intelligence behind them. The goal is not a larger human-facing graph product. The goal is an agent reaching correct source context sooner, with fewer calls, fewer full-file reads, less backtracking, and lower total context use.

## What Changes

- Add a typed graph of the selected live local source state, with stable project, file, symbol, package, relation, resolution, coverage, and generation identities; current saved bytes remain authoritative over optional VCS context.
- Publish full and incremental structural data atomically while preserving authored purposes, settings, and telemetry.
- Keep reviewed folder/file purposes as durable authored responsibility state for navigation, then combine them with current content, parser trust, and graph proximity rather than replacing them with edge counts. Deterministic or heuristic text remains an unapproved suggestion until an agent accepts it; ordinary source and graph changes never invalidate an accepted purpose, while an explicit agent or user correction may replace one that is wrong or genuinely repurposed.
- Let supported agent hosts run a quiet bounded isolated subagent purpose curator beside the main task at the lowest reliable reasoning and cost tier the host supports, using a task-scoped queue without adding purpose-maintenance chatter to normal navigation responses; a fixed reliable tier still delegates without a selector, and only hosts without bounded isolated subagent execution use the main agent.
- Make normal index-backed reads, watch, and incremental refresh freshness-aware so results stay current after edits, offline changes, and restarts without repeated whole-repository work.
- After one exact post-start verification, let a healthy long-lived source-observation epoch make later unchanged agent reads proportional to their bounded query; any event, observation gap, overflow, uncertainty, or mid-query change invalidates the epoch before current facts are claimed.
- Treat huge local source trees as a first-class performance target: keep full work proportional to indexed input and emitted facts, bounded queries independent of repository-wide scans, incremental work proportional to the affected dependency closure, SQLite access indexed and batched, and CPU/memory concurrency within one host-wide resource envelope.
- Keep one explicit workload-driven database architecture decision covering SQLite operating assumptions, authored/derived authority, conceptual/logical/physical graph design, hot queries and indexes, publication/read transactions, WAL/concurrency/recovery, huge-source limits, focused diagrams, and measurable conditions that would force the engine choice to be revisited.
- Keep the single project database and its temporary work bounded over time: compact telemetry without losing supported token-report semantics, remove only ownership-proven obsolete derived/staging state, reuse or reclaim pages through a measured maintenance lifecycle, and never turn an ephemeral spill file into a second atlas.
- Generate one versioned language capability registry with an explicit accepted capability set, honest detected, parsed, symbols, semantic, and benchmarked tiers, and no silent loss of accepted language breadth.
- Enrich existing folder, file, search, summary, and relation calls automatically with compact graph context and next-call guidance without discarding or adding mandatory steps to the 0.3.26 funnel.
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
