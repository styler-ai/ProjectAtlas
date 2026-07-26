## ADDED Requirements

### Requirement: Optional Benchmark Evidence Is Bounded And Read-Only
The token overview SHALL accept an optional repository-relative navigation benchmark artifact, SHALL validate it without persisting or mutating project state, and SHALL read it only within fixed byte and collection bounds.

#### Scenario: No benchmark artifact is supplied
- **WHEN** a caller requests the existing token overview without a benchmark path
- **THEN** the report SHALL return comparison state `unavailable`
- **AND** existing token accounting and trend behavior SHALL remain unchanged.

#### Scenario: A valid in-project artifact is supplied
- **WHEN** a caller supplies a regular benchmark-result file beneath the selected project root
- **THEN** the service SHALL read and validate it through the captured project binding
- **AND** the benchmark read SHALL create no SQLite row, telemetry event, benchmark-owned sidecar, or rewritten artifact beyond the existing read-only SQLite connection lifecycle.

#### Scenario: The path escapes the selected project
- **WHEN** an MCP or CLI caller supplies an absolute path, parent traversal, path indirection, or a file outside the selected project root
- **THEN** the request SHALL fail at the repository path boundary
- **AND** it SHALL NOT read the outside target or mutate either project.

#### Scenario: The artifact exceeds a bound
- **WHEN** the artifact exceeds the byte limit, any retained run, schedule, comparison, distribution, or MCP-call count exceeds its limit, a count/byte/token sample is fractional or exceeds the lossless JSON-integer bound, or a wall-time sample exceeds seven days
- **THEN** the report SHALL return an explicit failed or incompatible comparison state
- **AND** it SHALL NOT partially expose unvalidated comparison values.

### Requirement: Comparison Compatibility Is Typed And Honest
The report SHALL distinguish unavailable, failed, incompatible, partial, and compatible benchmark evidence and SHALL preserve candidate, baseline, schema, workload, failure, and digest identity needed to interpret the result.

#### Scenario: The supported final benchmark is loaded
- **WHEN** schema version, v0.4 candidate version, frozen-v0.3.26 version, plain control, schedule, repeat, run-retention, comparison, and metric contracts validate
- **THEN** the report SHALL expose the artifact digest and bounded public identities
- **AND** it SHALL classify each baseline row from its actual matched and failed trials.

#### Scenario: A required identity or metric is stale
- **WHEN** the schema, candidate/baseline semantic identity, arm contract, required workload, or required metric differs from the supported contract
- **THEN** the report SHALL return `incompatible` with a bounded reason
- **AND** it SHALL expose no fabricated comparison zero or savings percentage.

#### Scenario: Retained setup failures prevent one complete match
- **WHEN** a baseline has explicit failed trials for a workload while other workloads remain exactly matched
- **THEN** that baseline row and the overall evidence SHALL be marked `partial`
- **AND** matched metrics MAY remain visible with the unmatched failure count
- **AND** failed trials SHALL NOT enter completed-trial denominators as zero.

#### Scenario: Provider counters are present
- **WHEN** the artifact contains provider input, cached-input, output, or reasoning counters
- **THEN** those counters SHALL remain labeled descriptive-only and non-causal
- **AND** they SHALL NOT contribute to navigation savings or break-even arithmetic.

### Requirement: Matched Navigation Metrics Have One Typed Representation
The token overview SHALL expose matched v0.4-versus-frozen-v0.3.26 and v0.4-versus-plain rows from the validated artifact, using the same typed values for CLI JSON/TOON, MCP, and TUI rendering.

#### Scenario: Matched groups contain navigation evidence
- **WHEN** both arms completed the same workload trials
- **THEN** the row SHALL report matched and failed trial counts
- **AND** it SHALL report total and ProjectAtlas tool calls, productive and wrong folder/file/relation visits, broad/full reads, backtracks, gross and net navigation context, setup/runtime cost, persistent bytes, and break-even state where available.

#### Scenario: A ratio denominator is zero
- **WHEN** a baseline metric is zero or a break-even saving is not positive
- **THEN** the typed result SHALL report the percentage or break-even value as unavailable
- **AND** it SHALL NOT emit infinity, NaN, or a fabricated percentage.

#### Scenario: Adapters render the report
- **WHEN** the same project, session, and benchmark path are requested through CLI JSON, CLI TOON, and `atlas_token_report`
- **THEN** all adapters SHALL expose identical comparison state, identities, counts, values, and unavailable fields.

### Requirement: Capability Contributions Are Bounded And Non-Causal
The report SHALL group trace-completed v0.4 MCP calls into bounded navigation-capability rows and SHALL reconcile their calls and emitted bytes without relabeling trace status as semantic success or claiming per-capability token causality.

#### Scenario: Supported v0.4 MCP calls are present
- **WHEN** validated v0.4 traces contain discovery, summary/slice, search, and symbol/relation tools
- **THEN** capability rows SHALL classify calls by those durable responsibilities
- **AND** visible row calls SHALL sum to the classified trace-completed ProjectAtlas MCP-call total.

#### Scenario: An unknown tool name is present
- **WHEN** a compatible artifact contains a trace-completed ProjectAtlas MCP tool outside the supported classification
- **THEN** the report SHALL place it in one bounded `other` contribution row or mark the evidence incompatible
- **AND** it SHALL NOT silently drop the call or assign unsupported savings.
