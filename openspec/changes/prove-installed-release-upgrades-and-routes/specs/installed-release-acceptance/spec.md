## ADDED Requirements

### Requirement: Every installed CLI route has executable proof
The packaged CLI contract SHALL reconcile every supported top-level command, nested subcommand, and action with the frozen current inventory and execute each route against the exact installed candidate on every supported native tuple. Each case SHALL assert its real output, typed refusal, or filesystem/SQLite effect with isolated state. Help and schema checks SHALL remain supporting evidence only.

#### Scenario: A nested route loses behavioral coverage
- **WHEN** the frozen inventory contains a nested route absent from the executable case set
- **THEN** the packaged contract fails before reporting complete coverage

#### Scenario: A route succeeds or refuses within its owning state
- **WHEN** a nested route executes with its fixture prerequisites or deliberately invalid input
- **THEN** its output and owning state match the documented success or typed failure and unrelated canaries remain unchanged

#### Scenario: Wrong root or missing index
- **WHEN** a route targets a different project or lacks required index state
- **THEN** it returns the documented typed selection/recovery outcome without implicitly mutating the wrong project

### Requirement: Actual published predecessor state gates release
Every supported prepublish tuple SHALL obtain and checksum-verify the published v0.4.5 runtime, exercise that executable to author a real project database, and update that same installation/state through the existing candidate installer. A Cargo build, ambient executable, or synthetic SQL fixture SHALL NOT replace the released predecessor.

#### Scenario: Verified upgrade preserves authority
- **WHEN** the exact candidate updates the exercised predecessor installation
- **THEN** project identity, database/root selection, authored purposes, telemetry, and worktree registrations survive; generation and source evidence remain valid or explicitly require a supported refresh, and candidate CLI/MCP/host bindings agree after recovery

#### Scenario: Candidate admission fails and is retried
- **WHEN** an injected candidate checksum or migration failure refuses the update
- **THEN** the prior valid runtime/configuration and authored database state remain intact, and retry with valid inputs completes without destructive reinitialization

#### Scenario: Rollback respects schema compatibility
- **WHEN** the retained predecessor is invoked against candidate state it cannot interpret
- **THEN** it refuses before database or sidecar mutation and the accepted candidate remains usable; compatible retained-state recovery preserves its authored state and does not silently discard later changes

#### Scenario: Required predecessor proof is unavailable
- **WHEN** the required released executable, checksum, or explicit artifact binding is missing or invalid
- **THEN** publication acceptance fails instead of skipping the contract or substituting a local build

### Requirement: Existing packaged matrix owns complete acceptance
The existing release workflow SHALL execute the complete CLI/MCP route and predecessor-upgrade contracts against installed packages on Linux x64, Windows x64, macOS x64, and macOS arm64 before publication. The existing live MCP inventory/effect equality SHALL remain authoritative.

#### Scenario: One platform or contract fails
- **WHEN** any required installed contract fails or executes no intended test
- **THEN** publication remains blocked and no passing sibling platform substitutes for that proof
