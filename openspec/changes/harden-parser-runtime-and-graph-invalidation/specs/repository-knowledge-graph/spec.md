## ADDED Requirements

### Requirement: Incremental invalidation discovers external endpoints in bounded sets
Incremental graph publication SHALL discover external relation endpoints for affected local entities through bounded set-oriented, parameterized SQLite queries that use the existing source-first and target-first adjacency indexes. It SHALL NOT execute one source and one target adjacency statement per affected entity. Candidate identity, both relation directions, orphan cleanup, error propagation, savepoint rollback, and atomic publication semantics SHALL remain unchanged.

#### Scenario: Affected entities have external endpoints in both directions
- **WHEN** one bounded affected-key batch contains local entities with inbound and outbound relations to external entities
- **THEN** one compound statement per admitted chunk uses both owning adjacency indexes and returns the exact union of external endpoint keys

#### Scenario: Candidate discovery fails
- **WHEN** row decoding, statement execution, cancellation, or another terminal SQLite error occurs during external-endpoint discovery
- **THEN** the publication owner propagates the error and rolls back without publishing partial graph state or losing the last complete generation

#### Scenario: Affected closure grows
- **WHEN** the affected local-key set exceeds one admitted statement chunk
- **THEN** discovery executes a bounded number of parameterized chunks, bounds in-memory keys and candidates, and preserves exact results without adding schema or persistent writes
