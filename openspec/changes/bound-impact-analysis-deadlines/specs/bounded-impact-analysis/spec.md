## ADDED Requirements

### Requirement: One aggregate control bounds complete impact analysis
ProjectAtlas SHALL apply one captured absolute deadline, cancellation source, and aggregate resource budget across impact and optional dead-code candidate discovery, SQLite reads, graph traversal, hydration, composition, and output encoding.

#### Scenario: Deadline expires during dead-code discovery
- **WHEN** `include_dead_code` is true and the absolute deadline expires while discovering or reading candidate usage
- **THEN** ProjectAtlas stops the phase and returns typed deadline or bounded-partial state without waiting for the host timeout

#### Scenario: Deadline expires during traversal or hydration
- **WHEN** the absolute deadline expires after discovery while traversing relationships or hydrating symbols, files, purposes, or selectors
- **THEN** ProjectAtlas stops using the same deadline and does not start another unbounded phase

#### Scenario: Cancellation races a database batch
- **WHEN** cancellation becomes terminal before or during a bounded SQLite batch
- **THEN** ProjectAtlas returns typed cancellation state and does not continue iteration, traversal, or rendering

#### Scenario: Successful control
- **WHEN** impact and dead-code analysis completes inside every declared budget
- **THEN** ProjectAtlas returns the same deterministic candidates, selectors, confidence, coverage, and total-state contract as a successful compatible request

### Requirement: Expiry releases read and task resources
ProjectAtlas SHALL release SQLite statements and snapshots, service task records, and intermediate collections before returning deadline, cancellation, truncation, or failure.

#### Scenario: Immediate follow-up after expiry
- **WHEN** a bounded impact request expires
- **THEN** an immediate harmless MCP status/read call completes promptly and a subsequent database write is not blocked by the expired request

#### Scenario: No authoritative partial publication
- **WHEN** impact analysis expires after producing intermediate candidates
- **THEN** ProjectAtlas publishes no source, graph, purpose, or analysis state and labels any permitted returned candidates with non-exact coverage and total state

### Requirement: CLI and MCP expose equivalent bounded behavior
ProjectAtlas SHALL enforce the same service deadline and cancellation behavior through the CLI and `atlas_symbol_relations` MCP analysis route.

#### Scenario: One-second MCP request
- **WHEN** an MCP impact request with `include_dead_code: true` declares a one-second deadline and small traversal budgets
- **THEN** it returns a typed ProjectAtlas result within a bounded tolerance rather than reaching the host's 300-second timeout

#### Scenario: Equivalent CLI request
- **WHEN** the equivalent CLI analysis declares the same deadline and budgets
- **THEN** it terminates through the same bounded service result and does not perform additional unbounded work

#### Scenario: Larger entrypoint remains bounded
- **WHEN** a larger entrypoint produces more impact and dead-code candidates than a leaf file
- **THEN** declared node, edge, visited-state, intermediate-byte, output-byte, deadline, and cancellation limits remain authoritative
