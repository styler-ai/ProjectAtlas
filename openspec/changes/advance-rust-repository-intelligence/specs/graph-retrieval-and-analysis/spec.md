## ADDED Requirements

### Requirement: Existing Calls Gain Automatic Bounded Graph Context

Existing folder, file, symbol, search, summary, and relation calls SHALL preserve the progressive narrowing funnel of folder purpose plus crisp graph role, file purpose plus crisp relevant connections, selected-file summary and trust state, then exact slice. Reviewed purposes plus current graph context SHALL be consumed automatically when available and fall back deterministically when unavailable. Exact path/name and strong reviewed-purpose matches SHALL keep priority over weaker popularity/proximity signals. Folder/file rows SHALL expose only compact reason codes, relationship counts, a bounded high-value connection sample, truncation, and the next recommended call; agents SHALL NOT need a new mandatory call or learn a graph-query language. Generated purpose suggestions SHALL not be treated as reviewed truth.

#### Scenario: Several lexical matches exist
- **WHEN** one candidate has strong current package/import/call/test context
- **THEN** it may rank higher with a compact deterministic reason while exact path matches remain dominant

#### Scenario: Purpose and graph popularity disagree
- **WHEN** a reviewed responsibility purpose strongly matches the task but another file merely has higher graph degree
- **THEN** the responsibility match remains ahead and graph proximity is exposed only as a bounded secondary reason

#### Scenario: File has many relationships
- **WHEN** the default summary limit is exceeded
- **THEN** the response reports counts/truncation and recommends a detailed relation call instead of dumping edges

### Requirement: Purpose Curation Can Run Quietly Beside Navigation

The purpose queue SHALL support bounded task/generation/path-scoped selection of folders and high-impact files whose purposes are missing, stale, suggested, vague, or generic. When the host supports isolated bounded subagents, packaged guidance SHALL direct a low-reasoning curator to process the default `low` scope beside the main task at startup and relevant transitions using only queue rows, bounded current summary/graph/outline/slice context, and ProjectAtlas purpose APIs. Navigation SHALL not wait for successful low-scope curation. Duplicate project/generation/path work SHALL coalesce, and conflicting or ambiguous proposals SHALL not overwrite reviewed intent.

Successful low-scope maintenance SHALL NOT add per-path progress, approval, or completion prose to normal session, folder, file, summary, or conversational output. Later navigation SHALL simply consume approved purposes. A host-required terminal result SHALL be minimal and machine-facing. Only task-relevant unsafe conflicts or repeated degraded/failure state may surface as compact blockers or explicit health/settings diagnostics.

`medium` and `strict` purpose enforcement SHALL remain explicit choices and SHALL NOT start implicitly. A host without isolated subagents SHALL keep the task-scoped queue available without a false claim that ProjectAtlas spawned background work.

#### Scenario: Task candidates contain stale purposes
- **WHEN** startup ranks files whose reviewed responsibilities may have changed
- **THEN** the main response remains focused on the coding task while the supported curator independently claims the bounded task-scoped rows and later rankings consume approved updates

#### Scenario: Background curation succeeds
- **WHEN** the low-reasoning curator approves several bounded purpose updates
- **THEN** normal navigation emits no purpose-maintenance chatter and explicit diagnostics expose only aggregate state on request

#### Scenario: Strict purpose enforcement exists
- **WHEN** a normal coding session uses default purpose policy
- **THEN** ProjectAtlas does not silently expand background work from task-relevant low scope to every indexed file and folder

### Requirement: Lexical Correctness Is Always Available

Literal, regex, fuzzy, case, context, pagination, ordering, punctuation, short-string, and Unicode behavior SHALL remain deterministic without FTS or semantic capability. For indexable literal/token shapes, FTS/BM25 MAY narrow only safe complete candidate supersets and every result SHALL be exact-verified. Arbitrary regex, fuzzy, short, punctuation-sensitive, or tokenizer/Unicode-unsafe shapes MAY require the correctness-authoritative persisted-text fallback; that fallback SHALL be bounded by selected paths, inspected bytes, elapsed time, cancellation, and output, and SHALL expose searched-file/searched-byte/truncation state rather than claim repository-size-independent indexed access. Omitted retrieval mode SHALL remain lexical. Explicit `semantic` or `hybrid` mode SHALL return a typed capability error without a compatible ready generation, and hybrid mode SHALL retain lexical completeness with bounded versioned scoring reasons.

#### Scenario: Acceleration cannot guarantee completeness
- **WHEN** a query is regex, fuzzy, short, punctuation-sensitive, or tokenizer-sensitive
- **THEN** ProjectAtlas runs the correctness-authoritative fallback and preserves result semantics

#### Scenario: Optional semantic generation is unavailable
- **WHEN** a caller explicitly requests semantic or hybrid retrieval without a compatible ready generation
- **THEN** ProjectAtlas returns the pack state and recovery guidance rather than silently relabeling lexical results

### Requirement: Coverage Is Discoverable Without A New Scan

Existing summary and health surfaces SHALL expose bounded relationship/parse coverage digests and opt-in project-wide pages for complete, partial, failed, ignored, oversized, quarantined, and stale states. Every coverage row SHALL include an actionable path or bounded range, extraction pass or relation family, reason, reached limit when applicable, active generation, and trust state. Pages SHALL include returned, truncated, continuation, typed exact/at-least/unknown total state, and output-byte metadata and SHALL use current indexed rows without a new tool or implicit scan. An exact total SHALL appear only when already known, proved by the bounded page, or computable within the same declared database/row/time/cancellation budget.

Opt-in coverage discovery SHALL support bounded filters for path, parser/provider pass, relation family, coverage state, and reason. It SHALL distinguish complete, partial, failed, ignored, oversized, quarantined, and stale records when those states apply, and SHALL report returned, truncated, continuation, and typed total state without starting a scan or unbounded count.

#### Scenario: Agent investigates missing relationships
- **WHEN** a summary reports partial relationship coverage
- **THEN** the agent can request the bounded affected coverage page and distinguish parser, limit, quarantine, ignore, and stale causes

#### Scenario: Agent narrows incomplete coverage
- **WHEN** an agent filters a current generation to partial or failed route relationships under one path
- **THEN** only matching bounded rows are returned with reason, trust state, exact path/span selectors where available, returned/truncated/continuation and typed total state, and the next safe call

### Requirement: Detailed Relations And Analysis Are Typed And Bounded

Existing relation requests SHALL retain legacy defaults, rows, ordering, and relation kinds. Additive closed view, direction, extended family, depth, confidence, resolution, source occurrence, exact reusable target selector/next call, cursor, and hard-limit fields MAY expose richer graph detail without a separate jump tool. Every returned node or path step backed by local source SHALL project the authoritative reviewed purpose of its owning file or applicable folder plus review/stale state; symbols SHALL inherit that owning-file projection, external or unresolved nodes SHALL report purpose as not applicable or unavailable, and derived graph rows SHALL NOT duplicate or own authored purpose text. Architecture, relationship-derived component/community candidate, dependency-cycle or strongly connected component candidate, reviewed-purpose alignment/drift, language-valid complexity/bottleneck candidate, VCS-aware impact, and trace SHALL begin as typed views of existing relation/summary/health services. Trace means bounded static relationship/path inspection over indexed source facts; runtime execution-trace ingestion is outside this change. Inferred architecture SHALL remain deterministic and candidate-labeled, retain exact reusable source selectors plus coverage/resolution/trust state, and expose returned/truncated/continuation plus typed total state under row, edge, time, memory, output, and cancellation budgets; it SHALL NOT scan an otherwise unvisited high-degree adjacency solely to compute display metadata. Expensive community or cycle analysis SHALL remain opt-in. Trace paths SHALL be node-simple, and inferred component, cycle, complexity, bottleneck, and dead-code output SHALL NOT be presented as architectural truth. VCS selectors SHALL be closed typed working-tree/index/revision-range values, use a maintained Git crate or shell-free argument-vector boundary, and SHALL NOT mutate, replace local source truth, or implicitly scan the source tree.

#### Scenario: Old relation request is repeated
- **WHEN** a client supplies only pre-change fields
- **THEN** the legacy projection remains compatible with only bounded additive metadata

#### Scenario: Cyclic graph is traced
- **WHEN** traversal encounters cycles or a high-degree node
- **THEN** returned paths do not repeat nodes and all row/depth/edge/time/memory/output/cancellation limits are enforced

#### Scenario: Agent follows a relationship target
- **WHEN** a relation resolves to a file or symbol in the selected local source generation
- **THEN** the result includes an exact selector accepted directly by summary, relation, or slice plus the supporting source span, trust state, and authoritative owning-purpose projection with review/stale state

#### Scenario: Agent already has a source anchor
- **WHEN** an agent starts from a selected file or symbol and asks which inbound or outbound connection to follow
- **THEN** ProjectAtlas ranks a bounded relevant connection set with relation reason, exact target selector, owning-purpose projection, coverage, resolution, trust, and next call without requiring overview, folder, or file discovery to run again

#### Scenario: Relationship target has no local purpose owner
- **WHEN** a returned node is external, unresolved, or otherwise not backed by a local file or applicable folder
- **THEN** the result reports purpose as not applicable or unavailable instead of fabricating, inheriting, or persisting purpose text

#### Scenario: Topology component crosses responsibility folders
- **WHEN** bounded relationship topology forms a deterministic component across folder or package boundaries
- **THEN** ProjectAtlas returns the component as an inferred candidate with exact selectors, coverage/trust state, and reviewed-purpose agreement or drift reasons rather than silently redefining responsibility

#### Scenario: Dependency cycle is present or absent
- **WHEN** opt-in bounded cycle analysis inspects cyclic and acyclic fixtures
- **THEN** the real strongly connected component is returned as a candidate, the acyclic fixture produces no false cycle, and any reached budget is reported as explicit truncation rather than a complete negative result

### Requirement: MCP Discovery Is Compact With Full Compatibility Available

Installer-generated agent configurations SHALL advertise only the compact inventory documented in `docs/agent-navigation.md`; its complete `tools/list` response SHALL remain within 16 KiB. A closed full surface SHALL preserve every pre-change tool name, request schema, default, and payload behavior. Compatibility aliases SHALL delegate to the same service implementation. No new mandatory graph or jump tool SHALL be added.

`atlas_session_brief` SHALL perform startup overview and purpose/graph candidate ranking once, then recommend the best ready-to-use summary, search, relation, or slice call. It SHALL NOT recommend rerunning folder/file ranking already included in the brief.

#### Scenario: New installer config starts an agent session
- **WHEN** the MCP host requests tools and then asks for a task-oriented session brief
- **THEN** only the compact agent inventory is advertised and the brief advances to the next unresolved navigation step without duplicate ranking calls

#### Scenario: Existing client selects full compatibility
- **WHEN** a pre-change MCP request is replayed through the full surface
- **THEN** its old route, schema, defaults, and compatible payload remain available through the shared implementation

### Requirement: Cursors Bind Result-Defining State

Every resumable cursor SHALL bind project/root identity, active generation, capability/algorithm version, normalized query/filters/order, and membership-affecting budgets. Any change SHALL return a typed stale or mismatched-cursor error. Uniform rows SHALL remain compact TOON by default, exact slices SHALL remain verbatim source, and topology-oriented results SHALL use typed bounded aggregate/node/edge/path records rather than flattening away relationships. Supported TOON/JSON SHALL remain valid, deterministic, bounded, and UTF-8 safe.

#### Scenario: Publication occurs between pages
- **WHEN** a cursor is resumed after the active generation changes
- **THEN** continuation is rejected with restart guidance rather than mixing generations
