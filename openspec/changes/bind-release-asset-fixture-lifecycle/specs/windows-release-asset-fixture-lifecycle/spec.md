## ADDED Requirements

### Requirement: Release-asset fixture lifetime follows the owned installer operation
The Windows installer E2E release-asset server SHALL remain available while its owned installer operation can still issue the expected archive and checksum requests, and SHALL terminate through the same bounded owner lifecycle when that operation succeeds or fails.

#### Scenario: Delayed installer reaches both downloads
- **WHEN** ordinary parallel scheduling delays the owned installer before it requests the archive and checksum
- **THEN** the local server remains available, validates and serves both exact requests, and terminates cleanly without deciding from an independent pre-request clock

#### Scenario: Installer completes without both requests
- **WHEN** the owned installer fails or completes before the archive and checksum contract is satisfied
- **THEN** the server terminates within the owner's overall bound and reports the missing or invalid request state without hanging or hiding the installer result

### Requirement: Request validation and product behavior remain unchanged
The fixture SHALL continue to accept only the exact archive and `SHA256SUMS` paths and payloads required by the test. The change SHALL NOT alter installer, runtime, PATH, MCP, database, release, or public CLI behavior.

#### Scenario: Unexpected or partial request sequence
- **WHEN** the local server receives an unexpected path, empty request, invalid transfer, or only one required asset request
- **THEN** the test fails with bounded causal diagnostics and leaves no server thread or owned installer process behind

#### Scenario: Valid installer flow
- **WHEN** the installer requests both assets and completes successfully
- **THEN** existing payload validation, installed-runtime checks, readiness output, and compatibility assertions remain unchanged

### Requirement: The release gate is reliable under ordinary parallel load
The focused fixture and ordinary parallel locked workspace gate SHALL pass without retry-only acceptance, suite serialization, timeout inflation, global locks, or a new process/server framework.

#### Scenario: Parallel workspace execution
- **WHEN** the fixture runs with the repository's ordinary parallel test workload
- **THEN** completion depends on the owned installer and exact request contract rather than an unrelated local-server deadline
