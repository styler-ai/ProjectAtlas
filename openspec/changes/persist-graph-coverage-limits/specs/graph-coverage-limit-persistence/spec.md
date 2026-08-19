## ADDED Requirements

### Requirement: Every graph limit kind is durably admissible
The system SHALL persist and read `rows`, `nodes`, `edges`, `occurrences`, `visited`, `intermediate_bytes`, `deadline`, `depth`, and `output_bytes` as distinct `graph_coverage.reached_limit` values.

#### Scenario: Complete closed-domain round trip
- **WHEN** one valid partial coverage record for each `GraphLimitKind` is published
- **THEN** every record is committed and read back with the same distinct limit kind

#### Scenario: Unknown limit spelling
- **WHEN** a coverage row contains a reached-limit spelling outside the closed domain
- **THEN** SQLite rejects the row and the publication transaction does not expose partial state

### Requirement: Reached limits preserve repository publication
The system SHALL treat a valid reached graph limit as partial coverage rather than as a repository-wide publication failure, independent of source language or fact provider.

#### Scenario: Formerly rejected producer limit
- **WHEN** graph projection reaches `nodes`, `edges`, `visited`, `intermediate_bytes`, or `deadline`
- **THEN** the scan publishes one complete generation with the affected coverage row marked partial and the exact reached limit retained

#### Scenario: Language-independent persistence
- **WHEN** any parser, Markdown extractor, traversal, or future graph producer emits a valid limit kind
- **THEN** the shared repository graph writer applies the same persistence contract without language-specific mapping

### Requirement: Released databases migrate without authored-state loss
The system SHALL upgrade a valid schema-18 database transactionally to the complete graph-limit domain while preserving authored state and project identity and invalidating only rebuildable derived graph publication.

#### Scenario: Successful schema-18 upgrade
- **WHEN** the current runtime opens a valid schema-18 database
- **THEN** it reaches schema 19, preserves purposes and project identity, clears stale derived graph publication, and requests the normal full refresh

#### Scenario: Failed schema-18 upgrade
- **WHEN** any migration step fails before commit
- **THEN** SQLite rolls back the schema version and all migration-owned changes together

### Requirement: Existing graph budgets remain unchanged
The system MUST NOT raise graph limits or broaden indexed scopes as part of admitting all reached-limit values.

#### Scenario: Coverage admission after upgrade
- **WHEN** a workload reaches its existing configured graph budget
- **THEN** work stops at the same boundary and only the durable partial-coverage reason differs from the failed schema-18 behavior
