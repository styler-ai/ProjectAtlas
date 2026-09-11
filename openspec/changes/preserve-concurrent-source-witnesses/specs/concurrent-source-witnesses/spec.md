## ADDED Requirements

### Requirement: Superseded reads preserve newer valid source evidence

The source-observation owner SHALL distinguish a superseded read from invalid source evidence. Retrying an older read MUST NOT invalidate a newer mutation witness when source, policy, continuity, project identity, and publication remain valid.

#### Scenario: Mutation admission supersedes an in-flight read
- **WHEN** a read captures an epoch, a mutation exactly reconciles unchanged source and obtains a newer witness, and the old read reaches acceptance
- **THEN** the old read discards its provisional result and retries within existing bounds while the mutation witness remains valid

### Requirement: Actual invalidation prevents purpose publication

Real source changes, policy changes, observer continuity loss, identity changes, cancellation, and invalid publication state MUST retain their existing refusal and rollback behavior. An event consumed by a superseded reader MUST NOT leave a newer invalid witness usable.

#### Scenario: Stale reader observes a genuine source event
- **WHEN** an old read reaches acceptance after mutation admission and a relevant source event is observed
- **THEN** invalid evidence cannot authorize the mutation and the purpose transaction rolls back without authored-state changes

#### Scenario: Cancellation after mutation admission
- **WHEN** cancellation occurs after admission but before purpose commit
- **THEN** the existing typed cancellation is returned and no purpose mutation commits

### Requirement: Existing isolation and runtime contracts remain intact

The fix SHALL retain exact root/database/config binding and existing bounded retries, output, CLI/MCP payloads, and observer fallback. It MUST NOT add implicit refresh, database initialization, telemetry suppression, or serialized fixture requests to manufacture success.

#### Scenario: Wrong root or absent index
- **WHEN** a purpose request selects a wrong root or missing index
- **THEN** the existing typed refusal occurs without creating or changing project state

#### Scenario: Concurrent real MCP payload fixture
- **WHEN** existing navigation and purpose requests execute in the same MCP phase against unchanged indexed source
- **THEN** the expected purpose and navigation payloads are returned with their existing assertions and source identity intact
