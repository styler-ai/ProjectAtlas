## ADDED Requirements

### Requirement: Native-watch E2E mutation follows observable readiness
The native-watch E2E client SHALL keep the fixture unchanged until the selected database contains the fixture's exact initial symbol while the spawned watch child remains live. Readiness waiting SHALL be bounded, SHALL retain a useful last observation, and SHALL preserve child cleanup plus the existing native-event refresh assertions.

#### Scenario: Initial publication establishes readiness
- **WHEN** the spawned watch child has installed its watcher and published the exact `src/lib.rs::initial` symbol to the selected database
- **THEN** the E2E may write the changed fixture and continue waiting for the native-event cycle

#### Scenario: Database state is not ready yet
- **WHEN** the selected database is absent, temporarily unreadable, incomplete, or does not contain the exact initial symbol
- **THEN** the E2E keeps the fixture unchanged and retries only until the bounded readiness deadline

#### Scenario: Watch child exits before readiness
- **WHEN** the spawned watch child exits before exact initial publication is observed
- **THEN** the E2E fails with the child status, captured output, and last database observation instead of emitting the change

#### Scenario: Readiness deadline expires
- **WHEN** exact initial publication is not observed before the bounded deadline
- **THEN** the E2E kills and reaps any live child and fails with the last database observation plus captured output

#### Scenario: Native event refresh remains the tested behavior
- **WHEN** the changed fixture is written after readiness
- **THEN** the watch child exits cleanly at cycle two, reports notify mode, and the selected database contains the changed symbol
