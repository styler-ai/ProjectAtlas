## ADDED Requirements

### Requirement: Release-asset fixture lifetime follows the owned installer operation
The shared installer E2E release-asset server SHALL remain available while its owned installer operation can still issue the expected archive and checksum requests, and SHALL terminate through the same bounded owner lifecycle when that operation succeeds or fails. The owner SHALL create one four-minute absolute deadline before listener and installer launch, SHALL observe the child through the existing bounded installer helper using only the remaining duration, and SHALL reserve the remaining minute of the existing five-minute CI step for termination, join, diagnostics, and harness cleanup.

#### Scenario: Delayed installer reaches both downloads
- **WHEN** ordinary parallel scheduling delays the owned installer before it requests the archive and checksum
- **THEN** the local server remains available, validates and serves both exact requests, and terminates cleanly without deciding from an independent pre-request clock

#### Scenario: Installer completes without both requests
- **WHEN** the owned installer fails or completes before the archive and checksum contract is satisfied
- **THEN** the server terminates within the owner's overall bound and reports the missing or invalid request state without hanging or hiding the installer result

#### Scenario: Overall owner deadline expires
- **WHEN** the installer remains live until the shared four-minute absolute deadline
- **THEN** the existing installer observer terminates and reaps the owned child, the owner signals and joins the server, both failure causes remain visible, and cleanup finishes within the outer five-minute step

### Requirement: Request validation and product behavior remain unchanged
The fixture SHALL continue to accept only the exact archive and `SHA256SUMS` paths and payloads required by the test. The shared helper SHALL preserve `posix_release_binary_installer_rejects_checksum_mismatch` on Linux and macOS. The change SHALL NOT alter installer, runtime, PATH, MCP, database, release, or public CLI behavior.

#### Scenario: Unexpected or partial request sequence
- **WHEN** the local server receives an unexpected path, empty request, invalid transfer, or only one required asset request
- **THEN** the test fails with bounded causal diagnostics and leaves no server thread or owned installer process behind

#### Scenario: Valid installer flow
- **WHEN** the installer requests both assets and completes successfully
- **THEN** existing payload validation, installed-runtime checks, readiness output, and compatibility assertions remain unchanged

#### Scenario: POSIX checksum mismatch compatibility
- **WHEN** the POSIX installer uses the shared helper with an invalid checksum on Linux or macOS
- **THEN** the existing checksum-mismatch refusal and bounded helper cleanup remain unchanged

### Requirement: The release gate is reliable under ordinary parallel load
The focused lifecycle fixture, affected Windows installer fixtures, POSIX checksum-mismatch fixture, and ordinary parallel locked workspace gate SHALL pass without retry-only acceptance, suite serialization, an independent pre-request timeout, global locks, or a new process/server framework.

#### Scenario: Parallel workspace execution
- **WHEN** the fixture runs with the repository's ordinary parallel test workload
- **THEN** completion depends on the owned installer and exact request contract rather than an unrelated local-server deadline
