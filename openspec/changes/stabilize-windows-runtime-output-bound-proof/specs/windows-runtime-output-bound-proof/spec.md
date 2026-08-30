## ADDED Requirements

### Requirement: Windows runtime-probe output limits have causal test proof
The existing private Windows installer runtime probe SHALL expose enough optional test-only observation to distinguish live output-limit termination from true timeout without changing its production payload, timeout, output ceiling, or process lifecycle.

#### Scenario: Delayed writer exceeds the live ceiling
- **WHEN** an owned runtime writer starts after an injected fixture delay, emits more than the existing one-MiB ceiling, and remains alive
- **THEN** the test observes output-limit termination, a rejected payload, exact owned-process reaping, and complete probe-file cleanup without using total pre-launch elapsed time as the branch oracle

#### Scenario: Writer never exceeds the ceiling
- **WHEN** an owned runtime remains alive without producing a valid payload or exceeding the byte ceiling
- **THEN** the test observes the existing five-second timeout disposition and the same exact cleanup instead of accepting output-limit success

### Requirement: Production runtime validation remains unchanged
Production installer callers SHALL continue to receive the existing nullable validated JSON result and SHALL retain the five-second timeout, one-MiB per-probe-file ceiling, strict UTF-8/JSON validation, fail-closed errors, and exact owned-process cleanup. No test observation SHALL be emitted through CLI, installer, MCP, or machine-readable product output.

#### Scenario: Valid bounded runtime
- **WHEN** a runtime exits successfully inside the existing limits with valid strict JSON
- **THEN** the same validated payload is returned with no additional output or persistent state

#### Scenario: Invalid, flooding, hanging, or cleanup-failing runtime
- **WHEN** runtime execution violates validation, resource, exit, or cleanup requirements
- **THEN** the same null/fail-closed production result is returned and only the exact installer-owned process and temporary probe files are cleaned

### Requirement: The release gate is reliable under ordinary parallel load
The focused Windows fixture and the ordinary parallel locked workspace gate SHALL pass without retries, suite serialization, timeout inflation, global locks, or test-only acceptance of a true timeout.

#### Scenario: Hosted runner contention
- **WHEN** nested-process startup is delayed by ordinary scheduler contention
- **THEN** the test decides from the actual probe disposition and cleanup outcome rather than an arbitrary shorter wall-clock threshold
