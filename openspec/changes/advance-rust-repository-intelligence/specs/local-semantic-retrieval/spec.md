## ADDED Requirements

### Requirement: Always-Available Lexical Baseline
Core ProjectAtlas releases SHALL provide deterministic lexical identifier and source retrieval without FTS, an embedding model, or any optional pack. Lexical ranking SHALL normalize camelCase, PascalCase, snake_case, qualified names, and language-specific separators while preserving exact literal/regex/fuzzy matching, filters, context, pagination, and candidate/total semantics. FTS5/BM25 MAY accelerate complete lexical candidate selection only when the runtime proves the requested query class is safely supported; final matching and deterministic tie/order rules SHALL remain authoritative and SHALL produce the same logical result contract with or without FTS.

#### Scenario: No model pack is installed
- **WHEN** a caller performs normal file, symbol, or code discovery
- **THEN** lexical and graph ranking remains fully functional with no network access or degraded error state

#### Scenario: FTS is unavailable or unsafe for a query
- **WHEN** FTS is absent or cannot guarantee complete candidates for a short, punctuation-sensitive, Unicode, regex, fuzzy, case-sensitive, or tokenizer-incompatible request
- **THEN** the deterministic non-FTS lexical path runs and preserves the request's completeness, ordering, filters, context, and pagination contract

### Requirement: Explicit Optional Semantic Pack
Local embedding retrieval SHALL be an optional install/configuration choice, not a core scan requirement. ProjectAtlas SHALL NOT implicitly download a model, contact an inference service, execute repository code, or silently enable semantic indexing. Every model pack SHALL declare model/version, source, digest, license inventory, dimensions, tokenizer/preprocessing identity, runtime, disk/RSS/startup/query budgets, supported platforms, offline source behavior, and lifecycle commands.

The lifecycle SHALL expose one typed state contract through settings and inspection: `Absent`, `InstalledDisabled`, `EnabledIndexMissing`, `Building`, `Ready`, `Stale`, `Updating`, `RollbackReady`, `Incompatible`, `Failed`, and `Removing`. Each state SHALL define allowed operations, next states, active generation if any, rollback generation if any, and capability readiness. Install SHALL verify provenance, digest, license, ABI/runtime, platform, and resource policy before entering `InstalledDisabled`. Enable SHALL enter `EnabledIndexMissing` and then `Building`; only reconciled vectors and quality identity checks publish `Ready`. A structural or model-input identity change makes the selected generation `Stale` until a bounded rebuild publishes. Update SHALL retain the prior ready generation as rollback data but SHALL NOT expose it as the requested new generation; successful activation enters `RollbackReady`, from which explicit rollback or retention cleanup is valid. Disable SHALL enter `InstalledDisabled` immediately without harming structural/lexical data. Remove SHALL enter `Removing`, cancel or drain pack-owned tasks, delete only pack-owned model/vector/index artifacts, clear its settings, and finish at `Absent`. Failure SHALL preserve any separately identified rollback generation but SHALL expose semantic capability as unavailable until an explicit recovery or rollback reaches `Ready` or `RollbackReady`. Offline install, inspect, enable, disable, rollback, update, and removal behavior SHALL be deterministic and documented.

#### Scenario: User enables a verified model pack
- **WHEN** an installed pack passes digest, compatibility, and resource-policy checks
- **THEN** ProjectAtlas records the selected pack in visible settings and builds semantic records through a bounded task

#### Scenario: Model asset is absent or invalid
- **WHEN** semantic mode is requested without a valid configured model pack
- **THEN** ProjectAtlas returns a typed semantic-capability error with setup or recovery guidance and never downloads the asset or silently executes another retrieval mode

#### Scenario: Pack upgrade fails during vector build
- **WHEN** a replacement model/runtime fails, is cancelled, or misses reconciliation before activation
- **THEN** the replacement enters `Failed`, any prior generation remains rollback data rather than masquerading as the requested generation, and structural/lexical search remains complete

#### Scenario: Enabled pack is removed
- **WHEN** the explicit removal command is confirmed while semantic tasks or records exist
- **THEN** ProjectAtlas disables selection, drains or cancels owned tasks, removes only pack-owned artifacts, clears capability/settings state, and passes a no-pack lexical/graph smoke

### Requirement: Existing Search Retrieval Modes And Completion
The existing `atlas_search` surface and its CLI equivalent SHALL add one optional typed `retrieval_mode` with values `lexical`, `semantic`, and `hybrid`; omission SHALL preserve the pre-change lexical behavior. Literal, regex, fuzzy, file-pattern, case, context, limit, and pagination fields SHALL retain their existing meaning. Semantic and hybrid modes SHALL be accepted only for ranked natural-language/identifier retrieval combinations defined by the schema; contradictory regex/fuzzy/exact-only combinations SHALL return a validation error rather than silently changing meaning.

Every successful response SHALL report requested mode, executed mode, captured structural slot/epoch, lexical completion, semantic pack/model/vector generation and completion where requested, coverage, returned/available/truncated counters, and cursor identity. `lexical` SHALL be complete whenever the core index and requested scope are complete. Explicit `semantic` and `hybrid` requests SHALL execute only against a `Ready` or `RollbackReady` compatible generation; every other lifecycle state SHALL return a typed semantic-capability error with the exact state and recovery guidance. `hybrid` SHALL always execute lexical retrieval in addition to the ready semantic generation and SHALL report overall `complete` only when every requested signal is complete. ProjectAtlas SHALL NOT silently reinterpret an explicit retrieval mode or expose an implicit fallback option. Omission of `retrieval_mode` remains the backward-compatible lexical path.

#### Scenario: Existing search request omits retrieval mode
- **WHEN** a previously valid `atlas_search` request is sent without `retrieval_mode`
- **THEN** it executes deterministic lexical behavior with the same exact/regex/fuzzy/filter/context contract and no model-pack requirement

#### Scenario: Semantic mode lacks a ready vector generation
- **WHEN** `retrieval_mode=semantic` addresses a missing, stale, building, failed, disabled, or incompatible pack generation
- **THEN** the call returns a typed semantic-capability error naming the lifecycle state and recovery action, executes no fallback mode, and never reports semantic completion

#### Scenario: Hybrid mode lacks a ready vector generation
- **WHEN** `retrieval_mode=hybrid` addresses a missing, stale, building, failed, disabled, or incompatible pack generation
- **THEN** the call returns the same typed semantic-capability error rather than returning lexical results under a hybrid label

#### Scenario: Hybrid mode has partial vector coverage
- **WHEN** lexical retrieval completes but eligible semantic vectors cover only part of the requested scope
- **THEN** lexical results remain valid, semantic contribution and coverage are labeled partial, and the combined response is not labeled complete

### Requirement: Evaluated ANN Retrieval
Semantic retrieval SHALL use a maintained Rust-compatible approximate-nearest-neighbor or SQLite vector backend selected by benchmark, not an unindexed full-table `O(N * dimensions)` scan for normal repositories. The index SHALL support deterministic normalized model input bytes, model/tokenizer/preprocessing identity, bounded query candidates, cancellation, generation identity, and changed-row updates. Vector determinism SHALL be evaluated by byte equality for deterministic quantized output or a predeclared per-component tolerance, plus eligible-vector membership and Recall@K/top-K overlap on pinned queries. Backend-private ANN node IDs, graph topology, insertion layout, or serialized topology SHALL NOT be canonical equality requirements.

#### Scenario: Semantic query runs at scale
- **WHEN** the benchmark graph contains at least the declared million-vector scale
- **THEN** query p50/p95, recall, RSS, index bytes, and build/update time meet the published gate without scanning every stored vector

#### Scenario: Vector backend is unavailable
- **WHEN** an unsupported platform or disabled feature cannot load the ANN backend
- **THEN** ProjectAtlas remains functional through lexical/graph retrieval and reports semantic capability unavailable

### Requirement: Explainable Hybrid Ranking
Hybrid retrieval SHALL combine versioned lexical, graph, and semantic scores through a typed scoring contract whose active weights reconcile to 1.0 or whose non-additive composition is precisely documented. Results SHALL expose bounded per-signal reasons and SHALL exclude vectorless or invalid-vector rows from winning through sentinel/default scores.

#### Scenario: Hybrid result is inspected
- **WHEN** a caller requests ranking reasons
- **THEN** ProjectAtlas returns the lexical, graph, semantic, and final contributions needed to reproduce the ordering within rounding rules

#### Scenario: Candidate has no vector
- **WHEN** a graph entity has no valid embedding for the active model generation
- **THEN** it is ranked by declared lexical/graph fallback rules and cannot receive an artificial best semantic distance

### Requirement: Semantic And Clone Quality Gates
Semantic retrieval SHALL publish MRR, nDCG@10, Recall@10, precision at declared cutoffs, repeated-run determinism, latency, and resource results on a pinned labeled corpus. Every advertised semantic query family and every ready model/scoring profile SHALL independently meet its accepted accuracy and confidence gate; aggregate retrieval scores SHALL NOT mask a failing family. Similarity/clone edges SHALL use a labeled clone corpus, distinguish estimated signatures from exact similarity, publish precision/recall by clone type, cap candidate/edge counts, and remain disabled by default until each advertised clone family meets its quality threshold.

#### Scenario: Semantic weights change
- **WHEN** a model, tokenizer, feature, threshold, or weight changes
- **THEN** the full labeled retrieval suite reruns and the model/scoring version invalidates affected vectors or scores

#### Scenario: Similarity is unstable
- **WHEN** identical inputs or worker counts produce different semantic/similarity edges beyond documented tie ordering
- **THEN** the deterministic release gate fails and no new active enrichment generation is published

### Requirement: Local Privacy And Resource Controls
Embedding text, vectors, query text, and scores SHALL remain project-local unless an explicit future network capability is separately specified. Index and query operations SHALL enforce model input, file, vector, time, worker, memory, and disk budgets and SHALL publish progress/cancellation through the existing task model.

#### Scenario: Semantic indexing exceeds its budget
- **WHEN** a model task crosses a configured safe resource limit
- **THEN** it is cancelled or failed without affecting the active structural/lexical graph generation

#### Scenario: Network is denied
- **WHEN** scan and query run in a network-blocked environment
- **THEN** installed local semantic retrieval behaves identically and makes no outbound attempt
