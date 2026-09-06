## ADDED Requirements

### Requirement: Release-asset completion respects the absolute deadline

Every release-asset test-server completion decision SHALL reject an observation at or after the existing absolute deadline with `TimedOut`, irrespective of whether both expected assets were served. Request and response operations SHALL retain their respective timeout diagnostics. Idle waits MUST be bounded by the remaining deadline, and expiry MUST be checked before more listener work is admitted.

#### Scenario: Completion is observed after expiry
- **WHEN** any completion receive observes completion at or after the deadline, with complete or incomplete served-asset state
- **THEN** the shared result is `TimedOut` with the current operation's timeout diagnostic
- **AND** success or missing-request classification cannot override expiry

#### Scenario: Timely completion preserves served-asset classification
- **WHEN** completion is observed before the deadline
- **THEN** both required assets served yields success and missing required requests yields the existing missing-request error

#### Scenario: Stalled socket reaches its deadline
- **WHEN** a connected client provides no request and completion is delivered after the server deadline
- **THEN** the real lifecycle test observes the request timeout and includes the causal error if that assertion fails
- **AND** existing socket, thread, process cleanup and operation timeout values are preserved

### Requirement: Shared deadline proof is causal and portable

The existing lifecycle test SHALL exercise deterministic expired/timely complete/incomplete decisions and retain its real socket and process scenarios. Required local and affected hosted platform checks MUST pass without test suppression or increased operation deadlines.

#### Scenario: Correction is accepted for the release baseline
- **WHEN** the shared fix is reviewed for release acceptance
- **THEN** deterministic regression coverage distinguishes the old helper behavior and the existing macOS ARM, Linux, and Windows lifecycle gates execute successfully
