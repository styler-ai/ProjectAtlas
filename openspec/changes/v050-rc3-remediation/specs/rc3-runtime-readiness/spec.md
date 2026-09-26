## ADDED Requirements

### Requirement: Marketplace integration readiness is separate from plugin installation
ProjectAtlas SHALL expose whether the installed plugin, directly resolved runtime, generated host config, and registered Codex MCP target are version-matched, and SHALL report a mismatch as incomplete with a supported packaged repair action. It MUST NOT imply that Codex marketplace installation atomically updates external runtime or MCP state.

#### Scenario: Older runtime and MCP after plugin update
- **WHEN** a new ProjectAtlas plugin is installed while a fresh shell resolves an older CLI and Codex MCP still targets an older runtime or another project database
- **THEN** the ProjectAtlas readiness route reports the mismatched identities and exact version-matched installer and verification commands without claiming integration readiness

#### Scenario: Version-matched repaired installation
- **WHEN** the packaged installer has converged the runtime, generated host configs, and registry for the selected project without replacing its database
- **THEN** a fresh child process verifies the matching identities and a live host restart boundary is stated separately if required

#### Scenario: Untrusted or disabled plugin hook
- **WHEN** the host does not run the trusted startup hook
- **THEN** ProjectAtlas makes no automatic mutation or readiness claim and provides a manual diagnostic/repair route

### Requirement: Existing project databases are preserved during integration repair
The version-matched installer MUST refuse incompatible schema or project binding rather than reset, downgrade, replace, or silently rebind another project's database.

#### Scenario: Wrong project or newer database
- **WHEN** a repair targets a registration bound to a different project or a schema not supported by the selected runtime
- **THEN** it returns typed actionable refusal while retaining the prior database and registration until an explicit supported selection is made
