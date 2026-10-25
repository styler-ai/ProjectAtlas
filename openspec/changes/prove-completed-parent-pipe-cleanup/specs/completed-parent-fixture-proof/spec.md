## ADDED Requirements

### Requirement: Completed-parent cleanup proof establishes its process state

The installer lifecycle fixture SHALL establish a successful parent exit while an owned descendant still holds an inherited output pipe. The proof SHALL distinguish failure to reach that state from failure to retire the descendant and drain output. The fixture MUST use the existing process ownership and cleanup boundary and retain finite startup and cleanup bounds.

#### Scenario: Successful parent leaves an owned pipe holder
- **WHEN** the fixture parent exits successfully with a descendant retaining its output pipe
- **THEN** the observer retires that owned descendant, drains the pipe promptly, and preserves the successful parent exit status
- **AND** the proof establishes that the descendant retained the pipe before cleanup, so an empty process tree cannot pass as cleanup proof

#### Scenario: Fixture does not reach the completed-parent state
- **WHEN** the fixture fails to establish successful parent exit within its existing bounded startup allowance
- **THEN** the test fails with a diagnostic identifying the missing fixture state and cleans up only its owned process tree
- **AND** it does not claim completed-parent cleanup was exercised

### Requirement: Lifecycle fixture retains strict observer and containment contracts

The fixture correction MUST preserve strict observation deadlines, successful and late-completion classification, output draining, and Windows job or Unix process-group containment. It MUST NOT increase operation or workflow deadlines, suppress an existing route, or accept repeated retries as proof.

#### Scenario: Successful exit is first observed after the deadline
- **WHEN** a completed parent with an owned pipe-inheriting descendant is deliberately observed after its deadline
- **THEN** the observer reports completion after the deadline and retires the owned descendant without waiting for its natural lifetime

#### Scenario: Shared lifecycle gate runs on supported platforms
- **WHEN** the existing release asset lifecycle gate runs locally and in the affected hosted platform jobs
- **THEN** completed-parent cleanup and the existing live-parent timeout, late-observation, output-drain, and failure-cleanup cases pass under their unchanged contracts
