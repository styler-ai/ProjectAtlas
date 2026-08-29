## ADDED Requirements

### Requirement: Fixture readiness is bounded and contention tolerant

The Windows Codex MCP owner integration fixture SHALL use one named readiness budget that admits valid atomic child-identity publication beyond the former five-second ceiling under supported parallel workspace load and SHALL still terminate a missing publication within a finite bound.

#### Scenario: Valid delayed publication exceeds the former ceiling

- **WHEN** the real compiled owner fixture starts its ProjectAtlas MCP child and deliberately delays atomic identity publication for more than five seconds but less than the named readiness budget
- **THEN** the owning helper accepts the exact published child identity without retrying the test or serializing unrelated workspace tests

#### Scenario: Publication never arrives

- **WHEN** the owner remains alive but does not publish a child identity before the named readiness budget expires
- **THEN** the helper fails within that bound with elapsed-budget, owner, identity-file, and expected-runtime diagnostics and cleans up only its owned processes

#### Scenario: Owner exits before publication

- **WHEN** the owner fixture exits before publishing the child identity
- **THEN** the helper reports the exit immediately instead of waiting for the readiness deadline or guessing a child process

### Requirement: Exact process identity and cleanup remain fail closed

The readiness repair SHALL retain atomic publication plus exact child PID, start-time, and executable-path validation, and SHALL NOT terminate or adopt a process whose identity or ownership is unproven.

#### Scenario: Exact identity is published

- **WHEN** the fixture atomically publishes a PID, matching start time, and canonical expected runtime path
- **THEN** the helper returns that exact typed process identity and the owning installer E2E preserves its existing positive behavior assertions

#### Scenario: Published identity is malformed or mismatched

- **WHEN** the identity file is incomplete, malformed, refers to a reused PID/start time, or names a different executable path
- **THEN** the helper fails closed with a typed diagnostic and performs no guessed or unrelated process termination

#### Scenario: Failure cleanup runs

- **WHEN** publication, identity validation, or the later installer assertion fails
- **THEN** the test cleanup stops and waits only for the fixture-owned parent and child processes and leaves unrelated processes untouched

### Requirement: Release and split ownership retain causal proof

The accepted change SHALL pass the focused Windows fixture/installer E2E and the required parallel workspace gate without retry-only acceptance, and #487 SHALL retain exactly one final helper owner after refreshing onto the accepted change.

#### Scenario: Required workspace gate runs under parallel load

- **WHEN** the unchanged required Windows workspace test command executes the owning fixture alongside the broader suite
- **THEN** valid delayed startup does not fail at the former five-second boundary and all true failure paths remain bounded

#### Scenario: Delivery suite is refreshed

- **WHEN** #487 refreshes its split delivery-test owner onto the accepted #518 main revision
- **THEN** the readiness helper, delay seam, diagnostics, and causal tests exist exactly once in the final delivery owner and its frozen inventory reflects the real source

#### Scenario: Product surfaces remain unchanged

- **WHEN** the test-only change is built and exercised
- **THEN** CLI/MCP payloads, installer handoff behavior, project-root selection, generated configuration, schemas, persistent state, and supported non-Windows behavior remain unchanged
