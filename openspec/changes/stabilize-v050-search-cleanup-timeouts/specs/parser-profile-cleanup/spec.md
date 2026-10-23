## Purpose

Bound the cleanup of one owned parser artifact profile and distinguish operational cleanup failures from archive validity.

## ADDED Requirements

### Requirement: One owner controls the cleanup attempt

An explicit temporary-profile cleanup SHALL consume its single attempt even on failure, return that failure and prevent an accidental destructor retry. Early-error fallback SHALL remain bounded and artifact-scoped. Cleanup supervision SHALL preserve termination, terminal child reaping and bounded pipe completion.

#### Scenario: Injected explicit cleanup timeout

- **WHEN** cleanup times out after successful archive verification
- **THEN** the complete operation includes only one cleanup attempt and its reap/drain allowance
- **AND** the result identifies an operational cleanup failure without implying archive corruption or publishing partial lifecycle state

#### Scenario: Fallback and retry

- **WHEN** an early error leaves a temporary profile armed or a caller retries a failed lifecycle operation
- **THEN** fallback cleans only its owned profile once and a subsequent safe isolated lifecycle can succeed
