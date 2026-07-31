## ADDED Requirements

### Requirement: Accepted MCP test requests complete before client shutdown
An MCP E2E client that requires responses from accepted requests SHALL keep its input transport open until every required response is received or its bounded request deadline fails, and SHALL attempt explicit bounded session shutdown afterward.

#### Scenario: Multiple required responses
- **WHEN** one persistent test session issues several accepted tool requests
- **THEN** the client receives and validates each corresponding response before closing stdin

#### Scenario: Explicit bounded shutdown after success or failure
- **WHEN** the final response is validated or a request or assertion fails
- **THEN** the client attempts to close and reap the one persistent MCP child within the existing bound before returning the primary result

### Requirement: Stale-read adapter parity uses one live session
The stale-index MCP E2E SHALL exercise summary, search, relations, files, slice, and deleted absolute-selector reads through one project-bound session and SHALL preserve the exact typed `refresh_required` response and no-stale-content assertions for every adapter.

#### Scenario: Every normal read refuses stale state
- **WHEN** source or ignore policy differs from the published index
- **THEN** each normal-read adapter returns the typed refresh guidance without serving stale indexed content

#### Scenario: Test repair has no product mutation
- **WHEN** response-before-shutdown lifecycle is applied
- **THEN** production MCP, freshness, cancellation, database, dependency, and public contract behavior remains unchanged
