## ADDED Requirements

### Requirement: Generic subprocess observers classify deadline expiry before completion

Every generic E2E child-process observer covered by this change MUST classify an observation made at or after its absolute deadline as timed out before accepting child completion. The timeout path MUST terminate the exact child only when still running, reap it in all cases, preserve available output and diagnostics, and MUST NOT report successful completion.

#### Scenario: Completed child is first observed after the deadline

- **WHEN** a child completes promptly but the owning observer first resumes at or after its absolute deadline
- **THEN** the observer SHALL return its timeout classification, SHALL reap the exact child, and SHALL NOT accept the completed status or output as an in-time success

#### Scenario: Running child reaches the deadline

- **WHEN** the observer reaches its absolute deadline while the exact child remains running
- **THEN** the observer SHALL terminate and reap that child and return the existing bounded timeout diagnostic with all safely available output

#### Scenario: Child completion is observed before the deadline

- **WHEN** the observer establishes child completion before its absolute deadline
- **THEN** it SHALL retain the existing output collection, exit-status validation, diagnostics, and success or failure behavior

### Requirement: Deadline-ordering proof is causal and scheduler independent

Owning regressions MUST control observer delay independently from prompt child completion for `McpContractSession::shutdown`, `run_mcp_stdio_with_env`, and `wait_for_plugin_installer_output`. Proof MUST cover late observation and compatible in-time completion without retries, suite serialization, global locks, or enlarged timeout slack.

#### Scenario: Observer delay crosses the deadline after prompt completion

- **WHEN** a test fixture completes within its allowance and a test-only seam delays the observer until the deadline has expired
- **THEN** the regression SHALL causally exercise the timeout-before-completion branch and verify exact child cleanup

#### Scenario: Normal observer scheduling remains compatible

- **WHEN** the same helpers observe valid completion within their existing deadlines under the normal parallel test schedule
- **THEN** focused and workspace proof SHALL retain their prior successful behavior and bounded resource cleanup
