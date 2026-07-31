## ADDED Requirements

### Requirement: Hostile fixture allowances are phase-specific
The adversarial parser harness MUST retain the short no-progress allowance for the phase intentionally stalled by pre-ready, containment-admission, and progress-stall fixtures. A hostile fixture that does not intentionally stall launch or admission SHALL receive one separate bounded launch/admission no-progress allowance before its operation uses the fixture's existing no-progress and deadline policy. A fixture-specific operation deadline MUST begin only after launch/admission succeeds.

#### Scenario: Launch or admission stall remains short
- **WHEN** a pre-ready or containment-admission fixture intentionally makes no launch progress
- **THEN** the harness returns the exact typed no-progress failure within the short hostile allowance

#### Scenario: Progress stall remains short after admission
- **WHEN** a progress-stall fixture launches and admits successfully but then makes no meaningful parse progress
- **THEN** the harness applies the short operation allowance and returns the exact typed no-progress failure

#### Scenario: Non-stall hostile fixture reaches its behavior
- **WHEN** a non-stall hostile fixture runs on a loaded supported host
- **THEN** one bounded launch/admission attempt reaches and proves the fixture's exact protocol, I/O, limit, cancellation, or deadline result without accepting an unrelated admission timeout

#### Scenario: Response deadline is independent from delayed launch
- **WHEN** the endless-progress fixture spends longer than its operation deadline in launch/admission before successfully reaching request processing
- **THEN** its post-launch operation expires with `DeadlineExceeded` during `request response`, and a launch/admission deadline does not satisfy that expectation

### Requirement: Fixture isolation remains strict
The adversarial parser harness MUST NOT retry hostile cases, MUST reject weakened or mismatched typed results, and MUST complete mandatory process, pipe, reap, and thread cleanup before continuing. Test-only allowances MUST NOT change production parser deadlines, no-progress limits, containment, cancellation, resource limits, or protocol validation.

#### Scenario: Hostile case fails and cleans up
- **WHEN** a hostile fixture returns its expected failure
- **THEN** the harness verifies no mandatory cleanup failure or active process-spawn ownership remains before one healthy restart succeeds

#### Scenario: Production budgets remain independent
- **WHEN** the test-only launch/admission policy is compiled or executed
- **THEN** the production artifact-admission timeout, aggregate timeout, and no-progress timeout retain their existing values

### Requirement: Windows scheduling tolerance is verified
The complete process-owning adversarial harness SHALL pass repeatedly on Windows, and focused ordinary cross-platform Rust gates SHALL pass without production behavior changes.

#### Scenario: Repeated supported-host execution
- **WHEN** the complete adversarial test is run repeatedly with hard command timeouts on Windows
- **THEN** every run reaches each exact hostile result and completes without leaked child ownership
