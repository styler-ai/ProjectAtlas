## ADDED Requirements

### Requirement: Failed official Codex plugin updates preserve working state
The Windows and POSIX installers SHALL capture validated locally restorable official marketplace/plugin/config state before destructive replacement and SHALL restore it without network access when replacement fails.

#### Scenario: Every replacement and rollback add fails
- **WHEN** a working official ProjectAtlas marketplace/plugin is updated while every marketplace or plugin add attempt fails
- **THEN** the prior validated marketplace source, installed plugin, Codex config bytes, runtime selection, and generated ProjectAtlas state remain byte-for-byte or structurally identical and the installer reports failure

#### Scenario: Replacement succeeds
- **WHEN** the official replacement is acquired, validated, and installed successfully
- **THEN** the installer retains the new exact version and does not restore the snapshot

#### Scenario: Missing prior official integration
- **WHEN** no validated official marketplace/plugin exists before update
- **THEN** failure reports the missing replacement without fabricating or restoring state that was never present

### Requirement: Offline restoration retains installer trust boundaries
The installer SHALL snapshot and restore only validated official Codex-owned paths and exact configuration bytes and SHALL not follow links, cross owned containment, or trust unofficial/ambiguous sources.

#### Scenario: Source or destination is redirected
- **WHEN** a marketplace/plugin/config path is a symlink, junction, reparse point, traversal, hard-link hazard, or otherwise outside validated ownership
- **THEN** snapshot or restore fails before destructive mutation and preserves the existing target

#### Scenario: Marketplace is intentionally managed or unofficial
- **WHEN** the configured source is non-official or documented skip controls declare external management
- **THEN** the installer retains existing skip/non-mutation behavior and does not capture or replace that marketplace

### Requirement: Host handoff and platform behavior remain compatible
Offline restoration SHALL compose with generated configs, stable/versioned runtime convergence, and obsolete MCP handoff on Windows and SHALL have equivalent failure semantics on POSIX.

#### Scenario: Obsolete MCP handoff follows plugin failure
- **WHEN** plugin replacement fails while an obsolete MCP process or locked stable mirror is present
- **THEN** restored plugin/config state is not overwritten, unrelated or ambiguous processes survive, and typed handoff state remains truthful

#### Scenario: Windows and POSIX total-offline faults
- **WHEN** owning platform tests force every add/replacement acquisition to fail
- **THEN** both installers prove local restoration, exact previous state, and no network-dependent rollback requirement

### Requirement: Concurrent replacement serialization
The installers SHALL serialize inventory, snapshot, mutation, validation, and restore for each selected Codex root so one failed updater cannot overwrite another updater's newer successful state.

#### Scenario: A second installer starts during failed replacement
- **WHEN** one installer holds destructively mutated state and a second installer targets the same Codex root
- **THEN** the second installer reads plugin state only after the first installer has either validated success or completed local restoration

#### Scenario: A POSIX lock owner terminated
- **WHEN** a direct contained update lock records a process that no longer exists
- **THEN** the installer reclaims that exact stale lock safely before entering the serialized operation

#### Scenario: A terminated updater left recovery state
- **WHEN** a later installer acquires the Codex-root lock and finds retained ProjectAtlas plugin recovery state
- **THEN** it refuses further plugin mutation, retains the recovery state for inspection, and reports the recovery requirement truthfully
