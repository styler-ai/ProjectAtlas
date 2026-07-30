## ADDED Requirements

### Requirement: Windows installer reports distinct readiness scopes
The Windows installer SHALL distinguish verified versioned runtime plus generated absolute MCP-config readiness, installer-process bare CLI readiness, unchanged parent bare CLI readiness, and persistent parent-host restart requirement. Optional global Codex registry repair SHALL retain its separate success, skip, or warning result rather than being hidden inside generated-config readiness.

#### Scenario: Stable mirror is locked and parent PATH is stale
- **WHEN** a stale stable mirror is locked, the installer uses the verified versioned runtime, and the command environment inherited from the parent resolves the stale mirror
- **THEN** installation keeps runtime and absolute MCP integration ready, reports installer and parent CLI readiness separately, and emits `host_restart_required=true` only when the verified runtime was persisted for future processes

#### Scenario: Stable mirror is updated in place
- **WHEN** the stable mirror is not locked and is synchronized to the verified runtime
- **THEN** later siblings that inherited resolution of that same path are current and the installer emits `host_restart_required=false`

#### Scenario: Locked stable mirror is already current
- **WHEN** the inherited parent resolves the stable mirror and that locked mirror already satisfies the requested runtime version
- **THEN** the installer reuses it without replacement and emits `host_restart_required=false`

#### Scenario: Synchronized mirror is absent from inherited resolution
- **WHEN** the stable mirror is synchronized but the unchanged parent resolves a different stale command or no bare command
- **THEN** installation keeps runtime and absolute MCP integration ready and emits `host_restart_required=true` only when the verified runtime was persisted for future processes

#### Scenario: Parent already resolves the versioned runtime
- **WHEN** the stable mirror is locked but the command environment inherited from the parent already resolves the verified versioned runtime and version
- **THEN** the installer emits `host_restart_required=false`

#### Scenario: Quarantine exposes a current inherited fallback
- **WHEN** the parent PATH first resolves a known stale shim that installation quarantines and next resolves the verified runtime or a synchronized stable mirror
- **THEN** the installer derives readiness from that post-quarantine sibling resolution and emits `host_restart_required=false`

#### Scenario: Supplied runtime does not persist future PATH
- **WHEN** `-RuntimePath` is supplied or User PATH persistence is intentionally skipped and the unchanged parent still resolves a stale command
- **THEN** the installer emits `parent_cli_ready=false` and `host_restart_required=false`, explains that restart alone cannot repair the parent, and requires the stale command to be unlocked or removed before rerunning the installer

### Requirement: Future and absolute host integration remains correct
The installer SHALL keep the verified versioned runtime first in persisted User PATH for genuinely fresh processes and SHALL keep Codex MCP plus generated Codex, Claude Code, and OpenCode configs pinned to the verified absolute runtime, database, config, and version guard.

#### Scenario: Fresh host after partial readiness
- **WHEN** a new host starts from the User and Machine environment after a locked-mirror installation that persisted the verified runtime
- **THEN** bare `projectatlas` resolves the verified runtime and no restart requirement remains

#### Scenario: Existing host uses MCP before restart
- **WHEN** the parent host has not restarted after a locked-mirror installation
- **THEN** the generated and registered MCP integration still launches the verified absolute runtime even though parent-host bare CLI remains stale

### Requirement: Installer never repairs readiness by terminating unrelated processes
The installer SHALL NOT terminate, suspend, mutate, or replace the environment of running ProjectAtlas, Codex, terminal, or unrelated processes.

#### Scenario: Locked process remains alive
- **WHEN** an owned stale runtime process keeps the stable mirror locked during installation
- **THEN** the installer reports restart-required state without terminating that process or any other running process

#### Scenario: Parent-child-sibling regression topology
- **WHEN** a persistent parent launches the installer child and then launches a sibling bare-CLI probe from its unchanged environment
- **THEN** the installer result matches the sibling's actual stale-or-current resolution and does not infer parent readiness from the child's mutated PATH
