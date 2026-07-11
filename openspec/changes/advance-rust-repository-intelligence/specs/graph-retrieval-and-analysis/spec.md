## ADDED Requirements

### Requirement: Existing MCP And CLI Compatibility
Before implementation, ProjectAtlas SHALL generate and accept a machine-readable compatibility inventory of every current public CLI command, subcommand, alias, flag, default, output mode, exit-status/error contract, and every registered MCP tool name, request field/default, required response field, error shape, task behavior, and capability/settings field. That complete inventory, rather than a hand-selected navigation subset, SHALL be the compatibility floor. Every previously valid invocation/request SHALL remain valid with the same default scope and core semantics; new request fields SHALL be optional, and new response fields SHALL be additive and bounded unless a separate approved specification explicitly versions a surface.

This includes navigation, search, symbol, scan/watch, settings/config/root, ignore, init/map/lint/health/purpose, runtime/MCP config, task progress/cancellation, token/telemetry, and all other surfaces present in the accepted inventory. Graph enrichment SHALL be maintained by existing scan/watch flows and consumed automatically; a normal agent SHALL NOT need to orchestrate extraction or resolution passes. Existing CLI command names and the overview-to-folder-to-file-to-summary/outline-to-slice navigation order SHALL remain valid.

#### Scenario: Existing client repeats its startup flow
- **WHEN** a client built against the pre-change MCP schemas calls overview, folders, files, summary, and slice
- **THEN** every request deserializes and completes with backward-compatible core fields and no additional mandatory calls

#### Scenario: Graph enrichment is unavailable
- **WHEN** an older index or disabled optional pass lacks an enrichment
- **THEN** existing navigation calls still work and report the missing capability without demanding a different tool name

#### Scenario: Non-navigation compatibility corpus runs
- **WHEN** old accepted payloads and CLI invocations exercise settings, root/config, ignore, health/purpose, task, token, runtime, scan/watch, and output/error paths
- **THEN** every surface still deserializes/parses, preserves its old defaults and required fields/error semantics, and requires no new capability or mandatory call

#### Scenario: Registered surface is missing from the inventory
- **WHEN** the generated current tool/command registry contains a public surface absent from compatibility fixtures
- **THEN** compatibility validation fails before implementation or release rather than treating the untested surface as out of scope

### Requirement: Automatic Graph-Aware Ranking
`atlas_folders`, `atlas_files`, and non-exact ranked symbol search SHALL automatically use bounded graph evidence such as package/module proximity, import/call/reference relationships, test-source pairing, routes, configuration, and architecture boundaries when those signals are available. Ranking SHALL remain deterministic, expose compact reason codes, preserve exact path priority, and avoid per-candidate unbounded traversals.

#### Scenario: Strong graph evidence exists
- **WHEN** a query matches several files lexically but only one is directly related to the selected package, symbol, or test context
- **THEN** the related file ranks higher and the result includes a bounded graph reason

#### Scenario: No graph evidence exists
- **WHEN** the active generation has only current path/purpose/summary/text signals
- **THEN** ranking falls back to the existing deterministic behavior without an error or schema change

### Requirement: Lexical Search Correctness And FTS Acceleration
ProjectAtlas SHALL retain an always-available deterministic non-FTS lexical path while conditionally adding FTS5 candidate indexes for source text and identifiers. Literal, regex, fuzzy, file-pattern, case, context-line, pagination, ordering, and completeness semantics SHALL remain unchanged. FTS MAY narrow candidates only when it can produce a complete candidate superset for the requested mode; every candidate SHALL be verified by the existing exact line matcher. Unsupported short, punctuation-only, regex, fuzzy, case/tokenizer/Unicode-sensitive, unavailable-FTS, or uncertain cases SHALL use the deterministic fallback. FTS availability SHALL affect performance and reported execution path, never logical correctness or completeness.

#### Scenario: Literal candidate narrowing is safe
- **WHEN** an FTS tokenizer can produce a complete candidate set for a literal search
- **THEN** ProjectAtlas verifies those files with the exact matcher and returns the same logical matches/order contract as the non-FTS path

#### Scenario: Query cannot be narrowed safely
- **WHEN** a query is shorter than the tokenizer minimum, punctuation-sensitive, regex, fuzzy, or FTS5 is unavailable
- **THEN** ProjectAtlas uses the compatible fallback and records which search path ran

### Requirement: Detailed Relationship Surface
`atlas_symbol_relations` and its CLI equivalent SHALL remain the primary detailed graph relation surface. They SHALL accept optional direction, extended graph-relation-kind, depth, file/symbol identity, confidence, resolution-state, evidence, and pagination bounds while preserving old default behavior. A request that uses only pre-change fields SHALL preserve the legacy row set, ordering, and four serialized `RelationKind` values. Richer graph-only relation families SHALL require an explicit new extended-kind filter/mode and SHALL use a separate typed extended-kind field; they SHALL NOT place a new value in the legacy `kind` field. Compatible richer evidence MAY be additive and bounded without changing legacy row identity. Traversal depth, returned rows, visited nodes, expanded edges, wall time, and memory SHALL have hard limits and explicit truncation counters.

#### Scenario: Old relation request is sent
- **WHEN** a client supplies only a previously supported query, file, and limit
- **THEN** ProjectAtlas returns the same default one-hop legacy projection, row semantics/order, and four legacy `kind` values, with only bounded compatible metadata added

#### Scenario: Extended relation families are requested
- **WHEN** a caller opts into the new extended-kind filter or mode
- **THEN** ProjectAtlas may return graph-only families through the separate typed extended-kind contract without changing the serialized legacy enum

#### Scenario: Bounded traversal reaches a limit
- **WHEN** a relation expansion reaches its depth, row, edge, time, cancellation, or memory limit
- **THEN** the result stops deterministically and reports the reached limit, returned/available counters where known, and a resumable cursor when safe

### Requirement: Project-Wide Coverage Discovery
The existing `atlas_health` surface and its CLI equivalent SHALL accept additive opt-in coverage-state, extraction-pass, relation-family, and path filters while preserving every old request and default. Coverage discovery SHALL use indexed active-generation records rather than rescanning source and SHALL return bounded deterministic pages across `complete`, `partial`, `failed`, `ignored`, `oversized`, `quarantined`, and `stale` states. Each row SHALL identify the affected file or bounded path range, exact pass/relation family, reason, reached limit where applicable, active slot/epoch, and actionable capability state. Each page SHALL reconcile returned, available, and truncated counters.

#### Scenario: Agent locates incomplete graph coverage
- **WHEN** an agent filters health results for partial or failed call-resolution coverage under a path
- **THEN** ProjectAtlas returns an index-backed bounded page of every matching known coverage row with exact reasons and generation identity, without running a scan or requiring a new tool

#### Scenario: Existing health request is repeated
- **WHEN** a pre-change client calls health without coverage filters
- **THEN** its existing default finding scope and response contract remain valid and additive coverage fields do not require another call

### Requirement: Small Specialized Analysis Surface
The change MAY add no more than three optional MCP analysis tools without a separate approved specification: `atlas_architecture`, `atlas_impact`, and `atlas_trace`, each with CLI parity. They are explicit expert or informational surfaces for focused architecture, impact, and evidence-path inspection; they SHALL NOT be prerequisites for indexing, graph freshness, startup, normal navigation, search, file selection, summary, or slice workflows. Tool schemas SHALL use typed bounded filters and SHALL NOT require raw Cypher or another compatibility query language. A fourth tool requires benchmark evidence that existing tools cannot express a recurring agent workflow within the same or fewer calls/tokens.

#### Scenario: MCP tool inventory is compared
- **WHEN** the feature is released
- **THEN** all existing tool names remain and no more than the three approved analysis tools have been added by this program

#### Scenario: Normal agent never invokes an analysis tool
- **WHEN** an agent uses only the pre-change scan, overview, folders, files, search, summary, relations, outline, and slice surfaces
- **THEN** graph extraction, resolution, freshness, ranking, compact relationship context, and bounded next-step guidance still work automatically without a missing-capability prompt or mandatory extra call

#### Scenario: Generic query language is requested
- **WHEN** a desired workflow can be represented by typed architecture, impact, trace, or relation parameters
- **THEN** ProjectAtlas uses those bounded parameters rather than adding a custom Cypher parser

### Requirement: Architecture Analysis
`atlas_architecture` SHALL produce bounded evidence-backed facets for languages, packages/modules, entry points, public APIs, routes/channels, tests, configuration/infrastructure, fan-in/fan-out hotspots, dependency boundaries, cycles, and optional measured communities. It SHALL separate direct graph facts from inferred layers/communities, include coverage, generation, and truncation metadata, and SHALL NOT persist invented architecture prose as source truth.

#### Scenario: Project architecture is requested
- **WHEN** a caller requests architecture for a project or bounded path/package scope
- **THEN** ProjectAtlas returns typed facets backed by active-generation entities/relations and includes partial-coverage warnings

#### Scenario: Community inference is disabled
- **WHEN** the optional deterministic clustering pass is unavailable or below its quality gate
- **THEN** direct packages, dependencies, entry points, routes, and hotspots remain available without fabricated layers

### Requirement: Complexity And Bottleneck Candidates
ProjectAtlas SHALL compute only versioned, language-valid complexity and bottleneck candidates with explicit formula identities and source evidence. Accepted metrics MAY include per-language decision/branch counts and nesting, direct fan-in/fan-out, recursion/cycle membership, and bounded deterministic call-graph propagation. Every result SHALL identify source spans or graph evidence, coverage, calculation version, scope, and reached limits. These results SHALL be labeled heuristic candidates rather than asymptotic Big-O proofs, SHALL abstain when a language construct or partial graph cannot support the metric, and SHALL be exposed through existing bounded summary, relation, architecture, or impact responses without adding a fourth analysis tool.

#### Scenario: Bounded hotspot analysis has sufficient evidence
- **WHEN** a supported function has valid branch/nesting facts and resolved call-graph neighbors within the configured propagation budget
- **THEN** ProjectAtlas returns deterministic candidate metrics with formula, evidence, coverage, and limits instead of an unqualified complexity claim

#### Scenario: Language or graph evidence is incomplete
- **WHEN** a construct is unsupported, generated, malformed, dynamic, or beyond a coverage limit
- **THEN** the affected metric is omitted or marked partial with an abstention reason and cannot be ranked as complete evidence

### Requirement: Impact And Dead-Code Candidate Analysis
`atlas_impact` SHALL traverse typed inbound/outbound relationships from explicit stable identities and report affected files/symbols/packages/tests/routes/configuration with path evidence, confidence, coverage, and limits. Dead-code output SHALL be labeled candidate evidence, SHALL account for entry points, exports, tests, framework hooks, dynamic dispatch, generated code, reflection, unresolved references, and partial coverage, and SHALL never claim proof of safety from zero inbound edges alone.

#### Scenario: Symbol impact is requested
- **WHEN** a resolved symbol has bounded incoming callers, implementers, tests, or routes
- **THEN** ProjectAtlas returns deduplicated affected identities with shortest or strongest evidence paths and no repeated-edge path inflation

#### Scenario: Apparently unused symbol has incomplete evidence
- **WHEN** a symbol has zero resolved inbound edges but its language/framework coverage is partial or dynamic
- **THEN** it is either omitted or reported as a low-confidence dead-code candidate with the limiting evidence stated

### Requirement: VCS-Aware Change Impact
`atlas_impact` SHALL optionally accept one closed typed change selector: working tree, index, or revision range. ProjectAtlas SHALL parse VCS state through a maintained canonical Rust crate or direct argument-vector process calls without a shell, resolve the exact revisions plus working-tree/index signatures, and produce bounded changed path/hunk records for additions, modifications, renames, deletions, binary changes, and untracked files where the selected mode permits them. It SHALL map those changes to affected graph identities, packages, tests, routes, and configuration using the captured active slot/epoch and SHALL expose stale-index evidence, coverage, limits, and truncation. The request SHALL NOT mutate VCS state, start an implicit scan, or hide a mismatch between indexed content and the selected change state.

#### Scenario: Working-tree impact is requested against a stale index
- **WHEN** changed paths or hunks do not match the active generation's indexed fingerprints
- **THEN** ProjectAtlas returns bounded change evidence and an explicit stale-index limitation rather than silently rescanning or presenting graph impact as current

#### Scenario: Revision or path text contains metacharacters
- **WHEN** a caller supplies a valid adversarial revision, path, rename, deletion, or binary change fixture
- **THEN** ProjectAtlas treats it as typed data, never invokes a shell, enforces cancellation and output limits, and returns deterministic impact or a typed validation error

### Requirement: Bounded Path Tracing
`atlas_trace` SHALL find deterministic bounded node-simple paths between stable graph identities or filtered entity sets. Except for an explicitly returned zero-hop path whose source equals its target, no stable entity identity or logical edge SHALL occur more than once in one returned path. Traversal SHALL enforce configured hop, returned-path, visited-node, expanded-edge, wall-time, memory, and cancellation limits and SHALL not allow one request to block unrelated MCP reads. Results SHALL include every node identity and every edge's kind, evidence/confidence, captured slot/epoch, and truncation state.

#### Scenario: A valid dependency path exists
- **WHEN** a caller traces between two indexed identities within the configured limits
- **THEN** ProjectAtlas returns deterministic ranked paths with typed edges and evidence

#### Scenario: Adversarial cyclic graph is traced
- **WHEN** the graph contains cycles or a request asks for excessive depth
- **THEN** the traversal emits only node-simple paths, enforces every budget, supports cancellation, and leaves the MCP server responsive

#### Scenario: Cycle would revisit a node through different edges
- **WHEN** a candidate walk can reach an already visited entity by a different relation
- **THEN** that candidate is not returned as a path and does not inflate path count, score, or impact evidence

### Requirement: Cursor Snapshot And Query Binding
Every cursor emitted by relation, search, architecture, impact, or trace pagination SHALL be an opaque versioned value whose server-validated binding covers the tool and schema version, ordered project/root identities, captured active slot/epoch for every root, accepted capability/registry and relation-family inventory identities, optional model/vector generation, normalized query and filters, direction/kinds/depth, deterministic ordering/tie version, and all semantic traversal/response budgets that affect membership or order. The server SHALL revalidate decoded fields and current root authorization; cursor opacity SHALL NOT be treated as authorization.

A cursor SHALL be accepted only with the same bound request and unchanged captured snapshots. A changed root order, project identity, slot/epoch, pack/model generation, query/filter, ordering version, or membership-affecting limit SHALL return a typed stale/mismatched-cursor error instead of continuing against different data. A cursor SHALL be omitted when the implementation cannot produce a deterministic continuation within known safety limits. Page size MAY change only when the cursor schema explicitly separates it from membership/order budgets and revalidation proves the continuation unchanged.

#### Scenario: Cursor is resumed after graph publication
- **WHEN** any participating root has a different active slot/epoch from the cursor binding
- **THEN** the request fails as stale and returns restart guidance rather than mixing pages from two graph snapshots

#### Scenario: Cursor is replayed with wider filters or roots
- **WHEN** a caller changes a path, relation kind, direction, federation root/order, depth, or another membership-affecting field
- **THEN** binding validation rejects the cursor before traversal or row return

### Requirement: Compact And Valid Output
Every analysis response SHALL use typed Rust serialization to valid TOON/JSON, reconcile returned/available/truncated counters, preserve UTF-8 scalar boundaries during truncation, and include active project root, generation, coverage, and deterministic ordering. Human diagnostics MAY remain prose, but graph properties SHALL NOT be assembled through duplicated string-built JSON paths.

#### Scenario: Non-ASCII evidence is truncated
- **WHEN** a docstring, path, or snippet containing multi-byte Unicode exceeds a response limit
- **THEN** truncation occurs at a valid character boundary and strict decoders can consume the result

#### Scenario: Parallel and serial execution are compared
- **WHEN** the same analysis runs with one worker and multiple workers
- **THEN** canonical serialized entities, relations, evidence, and ordering are logically identical
