## ADDED Requirements

### Requirement: Separate healthy recovery tolerance
The adversarial parser harness SHALL give the healthy probe after hostile cleanup a no-progress allowance distinct from the hostile-case allowance and MUST keep the existing absolute harness deadline.

#### Scenario: Healthy recovery follows hostile cleanup
- **WHEN** a hostile parser peer produces its expected typed failure and cleanup completes
- **THEN** the single healthy recovery probe can use the platform-tolerant allowance without extending the absolute test deadline

### Requirement: Hostile cases retain strict bounds
The adversarial parser harness MUST retain the short hostile no-progress bounds, exact typed failure expectations, containment checks, and cleanup requirements.

#### Scenario: Hostile peer stalls
- **WHEN** a hostile parser fixture makes no progress
- **THEN** the harness rejects it within the existing hostile bound and reports the expected typed failure

### Requirement: Recovery verification remains release-grade
The correction MUST be verified by repeated Windows adversarial runs and ordinary cross-platform workspace gates without changing production parser behavior.

#### Scenario: Release verification runs
- **WHEN** the parser recovery correction is evaluated for release
- **THEN** repeated Windows adversarial coverage and ordinary cross-platform CI pass with production behavior unchanged
