## ADDED Requirements

### Requirement: Compatibility And Correctness Precede Performance Claims

Representative comparisons SHALL pin repository bytes, configuration, capabilities, commands, limits, runtime versions, and result schemas. A run with missing capability, incomplete coverage, timeout, crash, invalid output, or undeclared fallback SHALL remain a failure or ineligible result. Faster incomplete output SHALL not support a performance claim.

#### Scenario: Faster candidate omits relationships
- **WHEN** the candidate is faster but fails graph or language correctness
- **THEN** the performance cell is ineligible and the correctness deficit remains visible

### Requirement: Agent Workflow Improvement Is Observable

Representative paired tasks SHALL compare the current candidate with ProjectAtlas 0.3.26 using the same local source bytes, configuration, prompt, model, permissions, and budgets, including dirty-worktree and non-Git cases. Results SHALL include source-selection correctness, purpose-plus-connection usefulness before summary, calls/time to first useful context, wrong/redundant choices, backtracking, full-file reads, broad-read escapes, total tool calls, emitted bytes, conservative context tokens, next-call usefulness, and task completion. Every normal startup, locate, inspect/summary, relations, and exact-slice workflow SHALL keep the same or fewer mandatory calls and SHALL not regress reads or total context. The candidate SHALL also meet the stronger predefined navigation targets used to ensure it is not merely equal to the prior release.

#### Scenario: Extra summary context avoids no later work
- **WHEN** an enriched response grows but does not improve correctness or reduce later calls/reads/context
- **THEN** the enrichment is simplified, made opt-in, or removed

#### Scenario: Candidate improves navigation
- **WHEN** paired tasks reach correct source with fewer wrong selections, reads, calls, or context and no correctness regression
- **THEN** the candidate may be accepted as the preferred first repository tool for issue #308

#### Scenario: Graph feature weakens the existing sieve
- **WHEN** graph enrichment displaces a stronger purpose match, requires an extra mandatory call, trusts stale committed state over current local bytes, or adds context that prevents no later work
- **THEN** the enrichment fails acceptance and is simplified, moved behind an opt-in view, or removed

### Requirement: Resource Dimensions Remain Separate

Index time, incremental time, query latency, process-tree memory, database/WAL/staging writes, persistent bytes, package/install bytes, startup, and dependency/public-surface growth SHALL be reported and decided separately. Correctness or agent value MAY justify a trade-off but SHALL NOT convert a regressed resource into a superiority claim.

#### Scenario: Agent value improves while package size regresses
- **WHEN** behavior is accepted despite a larger package
- **THEN** documentation reports the trade-off and does not claim package-size superiority

### Requirement: Lean Verification Uses Normal Gates

Each coherent behavior slice SHALL run the smallest meaningful owning test plus risk-required integration, real CLI/MCP, concurrency, corruption, Unicode, cancellation, or affected-platform checks. One test MAY cover several tasks. Ordinary locked Rust/workspace, source, dependency, OpenSpec, and IssueOps gates remain authoritative. No unique test-per-task, task receipt, SHA ledger, evidence renderer, issue sealing, or repository-wide mutation/coverage campaign SHALL be required.

#### Scenario: One behavior check covers related tasks
- **WHEN** one focused integration test proves atomic refresh, freshness, and compatible query results together
- **THEN** the related tasks may complete without duplicate test wrappers or receipt rows
