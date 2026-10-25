## ADDED Requirements

### Requirement: Locked stale command diagnostics identify both runtimes
When Windows cannot synchronize an obsolete locked stable mirror, the ProjectAtlas installer SHALL report the exact bare-command executable selected by the inherited environment, its observed runtime version, the verified absolute versioned runtime, and the requested target version. It SHALL keep stable-mirror, versioned-runtime/config, installer-CLI, parent-CLI, and restart readiness truthful and distinct.

#### Scenario: Locked stable mirror remains obsolete
- **WHEN** the inherited bare command resolves to a locked stable mirror whose bounded version probe reports an older ProjectAtlas version
- **THEN** installer output names that exact stale path/version and the verified absolute path/target version, reports the bare command unready, and preserves usable versioned-runtime and generated-config readiness

#### Scenario: Stale version cannot be observed safely
- **WHEN** the inherited executable is locked or present but its bounded version probe cannot produce a trustworthy version
- **THEN** the installer reports the exact path with version unavailable, does not claim the command current, and keeps convergence partial

#### Scenario: Same-version foreign PATH command is not ready
- **WHEN** inherited PATH selects an executable at neither the verified runtime path nor the synchronized stable-mirror path, even though its bounded probe reports the target version
- **THEN** the installer reports that exact command and version as unready, emits verified-runtime recovery guidance, and derives restart applicability from exact fresh-process command resolution

#### Scenario: Bare command already resolves to the verified target
- **WHEN** the inherited command and version exactly match the verified runtime target
- **THEN** the installer does not emit stale-command recovery guidance

### Requirement: Recovery uses the verified runtime until mirror convergence
Locked-mirror guidance SHALL give an exact absolute-runtime verification/use command that does not depend on inherited PATH, state whether a fresh host restart can repair command resolution, and require unlock or observed exit followed by installer rerun and bare-command verification before claiming stable-mirror convergence.

#### Scenario: Versioned runtime is ready while mirror is locked
- **WHEN** the stable mirror remains obsolete but the versioned runtime and generated Codex, Claude Code, and OpenCode MCP configs pass final verification
- **THEN** the installer directs the operator to the exact absolute runtime and keeps those integrations usable without claiming the stale bare command is ready

#### Scenario: Fresh environment will resolve the verified runtime
- **WHEN** the persisted effective fresh-process PATH selects the verified runtime but the unchanged parent still resolves the stale mirror
- **THEN** guidance requires restarting the environment-owning Windows host, not merely one child shell, and gives the post-restart bare-command version gate

#### Scenario: Restart alone cannot repair command resolution
- **WHEN** the effective fresh-process PATH still selects a stale command
- **THEN** guidance requires correcting or unlocking that exact command and rerunning the installer instead of promising restart recovery

#### Scenario: Mirror unlocks and installer reruns
- **WHEN** the lock owner exits or releases the stable mirror and the installer is rerun
- **THEN** the installer synchronizes and verifies the mirror, the bare command reports the target version, and the bare token TUI opens the existing compatible database without reset or replacement

### Requirement: Diagnostic recovery does not broaden handoff authority
The #410 diagnostic and recovery path SHALL reuse the existing bounded probes, mirror synchronization, readiness classification, and #411 process-handoff authority. It SHALL NOT introduce name-wide termination, terminate Codex or unrelated processes, or count local locked-mirror diagnostics as completion of the separate real installed-Codex handoff release gate.

#### Scenario: Lock owner is not authorized for exact handoff
- **WHEN** the stale mirror remains locked by a process outside the existing exact handoff authority
- **THEN** that process remains alive and the installer emits path/version recovery guidance with partial convergence

#### Scenario: Local recovery regression passes
- **WHEN** an isolated Windows test proves stale diagnostics, absolute-runtime use, unlock, rerun, and bare-command convergence
- **THEN** #410 platform coverage passes without marking `handoff-obsolete-mcp-runtime` task 4.1 complete
