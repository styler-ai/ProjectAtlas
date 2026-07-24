## ADDED Requirements

### Requirement: Compatibility And Correctness Precede Performance Claims

Representative comparisons SHALL pin repository bytes, configuration, capabilities, commands, limits, runtime versions, and result schemas. A run with missing capability, incomplete coverage, timeout, crash, invalid output, or undeclared fallback SHALL remain a failure or ineligible result. Faster incomplete output SHALL not support a performance claim.

#### Scenario: Faster candidate omits relationships
- **WHEN** the candidate is faster but fails graph or language correctness
- **THEN** the performance cell is ineligible and the correctness deficit remains visible

### Requirement: Agent Workflow Improvement Is Observable

Representative paired tasks SHALL compare the current candidate with ProjectAtlas 0.3.26 using the same local source bytes, configuration, prompt, model, permissions, and budgets, including dirty-worktree and non-Git cases. Results SHALL include source-selection correctness, purpose-plus-connection usefulness before summary, calls/time to first useful context, wrong/redundant choices, backtracking, full-file reads, broad-read escapes, total tool calls, emitted bytes, conservative context tokens, next-call usefulness, and task completion. Every normal startup, locate, inspect/summary, relations, and exact-slice workflow SHALL keep the same or fewer mandatory calls and SHALL not regress reads or total context. The candidate SHALL also meet the stronger predefined navigation targets used to ensure it is not merely equal to the prior release.

After the complete v0.4 feature behavior is stable, one preregistered MCP composition evaluation SHALL cover six canonical questions: where behavior is implemented; which callers a symbol change affects; why a file is relevant; which relations are resolved, ambiguous, external, or unresolved; whether an edit invalidated dependent graph state; and the smallest trustworthy source slice after following a relationship. The evaluation SHALL compare the current compatible routes, additive request/payload/default/next-call changes, and the smallest credible dedicated or additional tool designs under identical clean, dirty-worktree, and non-Git fixtures and agent conditions. It SHALL measure answer and reusable-selector correctness, freshness/trust evidence, required calls, tool-discovery/schema and total context bytes, latency, backtracking, and bounded failure behavior. The current composition SHALL remain when it is best or tied; otherwise the smallest compatible additive v0.4 change or call set that materially improves the agent path SHALL be implemented and retested before the final release benchmark. No pre-v0.4 route may be removed or broken, and broader inventory removal, renaming, consolidation, or compact/default selection remains owned by issue #310.

#### Scenario: Extra summary context avoids no later work
- **WHEN** an enriched response grows but does not improve correctness or reduce later calls/reads/context
- **THEN** the enrichment is simplified, made opt-in, or removed

#### Scenario: Candidate improves navigation
- **WHEN** paired tasks reach correct source with fewer wrong selections, reads, calls, or context and no correctness regression
- **THEN** the candidate may be accepted as the preferred first repository tool for issue #308

#### Scenario: Graph feature weakens the existing sieve
- **WHEN** graph enrichment displaces a stronger purpose match, requires an extra mandatory call, trusts stale committed state over current local bytes, or adds context that prevents no later work
- **THEN** the enrichment fails acceptance and is simplified, moved behind an opt-in view, or removed

#### Scenario: Current MCP composition is only expressible, not effective
- **WHEN** the existing calls can technically answer a canonical question but require avoidable discovery, schemas, calls, context, backtracking, or incomplete trust/freshness reconstruction compared with a credible compatible additive design
- **THEN** the existing composition does not pass merely because it is expressible, and the smallest materially better additive schema or call surface is implemented and retested

#### Scenario: Proposed MCP call adds no measured agent value
- **WHEN** a dedicated or additional call does not improve correctness or the bounded agent path over the best existing-call composition
- **THEN** the call is not added and the existing compatible surface remains authoritative

### Requirement: Resource Dimensions Remain Separate

Index time, incremental time, query latency, process-tree memory, database/WAL/staging writes, persistent bytes, package/install bytes, startup, and dependency/public-surface growth SHALL be reported and decided separately. Persistent database size SHALL remain proportional to current indexed facts plus declared authored-retention budgets rather than lifetime tool-call or failed-staging count. Correctness or agent value MAY justify a trade-off but SHALL NOT convert a regressed resource into a superiority claim.

#### Scenario: Agent value improves while package size regresses
- **WHEN** behavior is accepted despite a larger package
- **THEN** documentation reports the trade-off and does not claim package-size superiority

### Requirement: Huge Local Source Trees Remain Responsive

ProjectAtlas SHALL treat large-source performance as an end-to-end product contract. Full indexing work SHALL remain proportional to included entries and bytes plus emitted facts. After the first exact post-start verification, normal unchanged reads in a healthy source-observation epoch SHALL not repeat a whole-source walk or full indexed-node load. Normal bounded file, summary, relation, coverage, and graph queries SHALL use bounded indexed access rather than whole-repository or whole-graph scans. Indexable search shapes SHALL use a complete indexed candidate path; arbitrary correctness-fallback search SHALL remain proportional to its explicitly bounded selected persisted-text bytes and report that work honestly. Incremental refresh SHALL be proportional to the changed paths plus the bounded affected dependency closure and SHALL explicitly require a full refresh when that safe closure exceeds configured limits instead of silently truncating or repeatedly rebuilding everything.

SQLite storage SHALL use typed normalized columns, responsibility-owned indexes, reusable prepared operations, and batched atomic publication; representative query plans SHALL reject accidental table scans on hot bounded lookups. Rust hot paths SHALL avoid unnecessary allocation, cloning, owned conversion, serialization, and unbounded intermediate collections. Filesystem/database I/O and parsing SHALL batch or stream where correctness permits. Parallel work SHALL derive from one effective host/process CPU and memory envelope, prevent cross-task oversubscription, keep lock/transaction hold times bounded, and preserve cancellation, backpressure, and responsiveness.

A versioned representative scale matrix SHALL preregister at least three scale points and exercise small, medium, and huge local source corpora, including dirty and non-Git trees, high file/symbol/relation cardinality, high-degree graph nodes, concurrent isolated projects, and incremental changes with both narrow and expanded inbound closures. The huge point SHALL include one pinned real-source corpus beyond ordinary regression-fixture scale; synthetic graph stress MAY isolate topology and skew but SHALL NOT by itself support an end-to-end source-scale claim. The matrix SHALL record corpus facts and the SQLite version/compile options, journal/synchronous/busy/checkpoint/statistics profile. It SHALL separately measure full scan, filesystem entries/bytes read, parsed bytes, incremental refresh and affected-closure rows, bounded query latency and plans, startup, process-tree CPU/worker utilization and parallel efficiency, queue/backpressure and database busy/lock time, cancellation-to-quiescence, memory/RSS before and after repeated work, database/WAL/staging writes and checkpoint behavior, persistent bytes, and output size. Thresholds and regression decisions SHALL be explicit for the measured environment; static review or a green correctness test alone SHALL NOT support a performance claim.

#### Scenario: Bounded query runs on a huge graph
- **WHEN** a file, relation, path, coverage, or search request asks for a bounded page from a huge indexed source tree
- **THEN** query-plan and runtime checks show bounded indexed access, bounded output and memory, and no repository-wide graph materialization

#### Scenario: Several unchanged reads share a healthy source epoch
- **WHEN** a long-lived agent runtime has exactly verified the huge local source tree and no relevant event or observation gap occurred
- **THEN** later bounded navigation calls avoid another full tree walk and full node-table decode while remaining invalidatable by a concurrent edit

#### Scenario: Arbitrary search requires exact fallback
- **WHEN** regex, fuzzy, short, punctuation-sensitive, or tokenizer/Unicode-unsafe semantics cannot use a complete indexed candidate set
- **THEN** ProjectAtlas scans only the declared selected persisted-text scope within byte/time/cancellation bounds and reports searched bytes plus truncation instead of making an indexed-complexity claim

#### Scenario: Several projects index concurrently
- **WHEN** isolated projects perform concurrent scan, parse, or enrichment work
- **THEN** their combined workers and memory stay within the effective host envelope without one project monopolizing the process or multiplying per-task CPU budgets

#### Scenario: Planner or operating-profile drift changes a hot query
- **WHEN** SQLite version, statistics, schema, indexes, pragmas, or query text changes for a bounded hot path
- **THEN** representative query-plan and runtime checks reject an accidental scan, sort, temporary structure, busy amplification, or checkpoint regression before a performance claim is accepted

#### Scenario: Repeated or canceled work releases resources
- **WHEN** indexing or traversal is repeated or canceled at a bounded checkpoint
- **THEN** admitted workers drain, queues quiesce, database transactions release, and retained process memory returns within the declared steady-state bound

#### Scenario: Small edit has a large inbound closure
- **WHEN** an exported identity or dependency key change affects many unchanged dependents
- **THEN** ProjectAtlas processes the complete bounded closure once or returns typed full-refresh guidance without partial publication, silent truncation, or repeated write amplification

### Requirement: Lean Verification Uses Normal Gates

Each coherent behavior slice SHALL run the smallest meaningful owning test plus risk-required integration, real CLI/MCP, concurrency, corruption, Unicode, cancellation, or affected-platform checks. One test MAY cover several tasks. Ordinary locked Rust/workspace, source, dependency, OpenSpec, and IssueOps gates remain authoritative. No unique test-per-task, task receipt, SHA ledger, evidence renderer, issue sealing, or repository-wide mutation/coverage campaign SHALL be required.

#### Scenario: One behavior check covers related tasks
- **WHEN** one focused integration test proves atomic refresh, freshness, and compatible query results together
- **THEN** the related tasks may complete without duplicate test wrappers or receipt rows
