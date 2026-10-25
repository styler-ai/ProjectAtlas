## Purpose

Give agents reliable Unicode case-insensitive source search without silent candidate loss or changes to source evidence and bounded retrieval.

## ADDED Requirements

### Requirement: Unicode case-insensitive search preserves matches

Literal and fuzzy case-insensitive searches SHALL use locale-independent Unicode simple case folding, agreeing with regex without canonical normalization. Storage and acceleration SHALL preserve a conservative candidate superset. Results SHALL retain original source lines, deterministic pagination, case-sensitive behavior and existing resource limits without database reset.

#### Scenario: Accented Latin and Cyrillic persisted text

- **WHEN** CLI or MCP searches for FÜR, CAFÉ, GRÜßE or русский against für, café, Grüße or Русский with case sensitivity disabled
- **THEN** literal and fuzzy modes return the original matching line and regex provides a matching control
- **AND** case-sensitive variants remain distinct and accelerated/fallback results agree where acceleration applies

#### Scenario: Case folding differs from lowercase conversion

- **WHEN** a caller searches for ΟΣ against οσ, kelvin against Kelvin or source against ſource
- **THEN** literal and fuzzy modes agree with the Unicode case-insensitive regex control
- **AND** an ASCII query cannot lose its Unicode source candidate in storage or acceleration

#### Scenario: Repeated persisted-text reads remain current

- **WHEN** a connection repeatedly hydrates persisted text and the text is replaced, a write rolls back or a row is deleted
- **THEN** later reads observe the current committed text or absence, preserve typed failures and maintain the existing search outputs and resource limits
- **AND** reuse of a prepared query does not retain source results between executions

#### Scenario: Bounded or invalid search

- **WHEN** selection or output reaches an existing limit, or a caller selects a wrong root or missing index
- **THEN** existing typed refusal or truthful truncation remains intact and search does not mutate source, approved purposes or project identity
