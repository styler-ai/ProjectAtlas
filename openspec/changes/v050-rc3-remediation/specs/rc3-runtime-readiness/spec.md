## ADDED Requirements

### Requirement: Marketplace integration readiness is separate from plugin installation

An MCP registration with nonempty or malformed environment overrides, inherited environment-variable names, or a working-directory override SHALL NOT be accepted as exact, even when its command and ordered arguments match; the installer SHALL repair or report it incomplete before attestation. Missing and null overrides, and empty environment maps or name arrays, are equivalent to Codex's default registration. A readiness receipt SHALL bind a Codex config digest unchanged across registration validation and immediately before publication. The hook's shell fallback SHALL reject JSON paths containing control or format characters before raw extraction and filesystem comparison.

ProjectAtlas SHALL expose whether the installed plugin, directly resolved runtime, generated host config, and registered Codex MCP target are version-matched, and SHALL report a mismatch as incomplete with a supported packaged repair action. It MUST NOT imply that Codex marketplace installation atomically updates external runtime or MCP state.
The installer SHALL add a missing Codex MCP entry only after a well-formed inventory confirms absence, and SHALL issue a bounded user-state readiness receipt only after verifying the runtime, generated host config, plugin, and exact MCP entry. The receipt SHALL bind the selected root, version, absolute registered runtime, installer-owned direct CLI, and config identities with hashes, and retain only the MCP fields needed for readiness, never MCP environment values or unrelated config. On Windows, the stable direct CLI mirror may differ in path from the versioned MCP runtime only when its bytes match the verified runtime. The recurring startup hook SHALL verify that receipt and the selected database schema/root binding through a bounded read-only probe, not repeat a full database integrity scan or execute a PATH-resolved Codex/ProjectAtlas command before its trust checks. Missing, stale, malformed, overridden, or unverifiable host state SHALL be incomplete with an installer repair route; full integrity verification remains an explicit command.

This RC3 requirement supersedes RC2's blanket no-runtime-execution rule for any shadow entry anywhere in PATH: a competing executable that wins direct resolution SHALL prevent all ProjectAtlas runtime execution, while a later, non-selected PATH entry does not prevent an already receipt-matched exact runtime from undergoing read-only verification.

#### Scenario: Older runtime and MCP after plugin update
- **WHEN** a new ProjectAtlas plugin is installed while a fresh shell resolves an older CLI and Codex MCP still targets an older runtime or another project database
- **THEN** the ProjectAtlas readiness route reports the mismatched identities and exact version-matched installer and verification commands without claiming integration readiness

#### Scenario: Version-matched repaired installation
- **WHEN** the packaged installer has converged the runtime, generated host configs, and registry for the selected project without replacing its database
- **THEN** a fresh child process verifies the matching identities and a live host restart boundary is stated separately if required

#### Scenario: First install has no MCP entry
- **WHEN** Codex reports no ProjectAtlas MCP entry and a well-formed registry inventory confirms its absence
- **THEN** the installer adds the exact version-matched entry without removing any unrelated registration and records readiness only after readback

#### Scenario: Host state or PATH changes after installation
- **WHEN** a PATH-shadowed CLI or Codex command appears, the runtime or host config hash changes, the receipt is absent or malformed, or a project config can override the global MCP entry
- **THEN** the startup hook runs neither shadow command nor a database mutation and reports integration incomplete with the selected project's repair command

#### Scenario: Untrusted or disabled plugin hook
- **WHEN** the host does not run the trusted startup hook
- **THEN** ProjectAtlas makes no automatic mutation or readiness claim and provides a manual diagnostic/repair route

Packaged hook and installer fixtures own the causal #620 checks. Actual Codex hook trust/disable behavior, an already-running host restart, and installed platform convergence are final #492 release acceptance, not claims inferred from a fresh child process.

### Requirement: Packaged agent guidance routes the short CLI and MCP truthfully
The packaged skill SHALL identify the installed `atlas` short command as the preferred route for ordinary operations in one exact checkout, describe every public CLI command family's function and trigger, and reserve MCP for registered alias routing, compact session briefs/typed continuations, and cross-worktree federation. The installer SHALL verify the hook instruction asset, main skill, language-support reference, and short-command guide in the source and cache and bind each digest into the readiness receipt. The trusted startup hook SHALL remind the agent to read the version-matched skill again after compaction, but SHALL NOT emit mutable hook guidance before validating its receipt digest. A missing, unreadable, or stale skill/guidance asset MUST be reported as integration incomplete; the hook cannot certify that a model internally read the skill.

The plugin source, installed cache, and readiness hook SHALL require the manifest's `projectatlas` name and skill route to select the shipped `./skills/` directory. On POSIX hosts, the hook's curated utility path SHALL include system-managed non-FHS locations needed by supported Linux installations without restoring arbitrary project-influenced PATH entries.

#### Scenario: Startup or compaction with complete guidance
- **WHEN** a trusted plugin hook runs at startup or after compaction with packaged guidance and a validated readiness receipt for the selected project
- **THEN** it injects the skill path and concise CLI/MCP routing guidance without initializing, scanning, or changing project state

#### Scenario: Guidance asset is damaged
- **WHEN** the packaged instruction asset is missing, unreadable, empty, or oversized
- **THEN** the hook reports integration incomplete with bounded reinstall guidance and does not report readiness

#### Scenario: Packaged skill or hook guidance asset is missing or stale
- **WHEN** a plugin source or cache lacks the shipped hook instructions, main skill, language-support reference, or short-command guide, or any of those assets changes after the installer recorded readiness
- **THEN** the installer refuses to attest that plugin artifact or the hook reports integration incomplete until the matching asset is restored

#### Scenario: Plugin identity, skill route, version, or system utility path differs
- **WHEN** a manifest changes the plugin name, redirects the skill directory, or differs from the expected version only by prerelease letter case, or a supported Linux host provides required utilities only through its system-managed non-FHS path
- **THEN** the mismatched manifest is incomplete, while the trusted non-FHS utilities can verify an otherwise matching installation without admitting project PATH shadows

### Requirement: Existing project databases are preserved during integration repair
The version-matched installer MUST refuse incompatible schema or project binding rather than reset, downgrade, replace, or silently rebind another project's database.

#### Scenario: Wrong project or newer database
- **WHEN** a repair targets a registration bound to a different project or a schema not supported by the selected runtime
- **THEN** it returns typed actionable refusal while retaining the prior database and registration until an explicit supported selection is made
