## ADDED Requirements

### Requirement: ProjectAtlas ships bounded trusted lifecycle guidance
The version-matched ProjectAtlas plugin SHALL ship a discoverable Codex lifecycle hook that emits concise ProjectAtlas-first navigation guidance at supported session starts. The hook SHALL direct the agent to the bundled version-matched skill and SHALL NOT initialize, scan, mutate project state, replace repository instructions, or emit the full skill text.

#### Scenario: Trusted startup hook emits guidance
- **WHEN** an enabled ProjectAtlas plugin hook is reviewed, trusted, and receives a supported startup event
- **THEN** Codex receives bounded ProjectAtlas-first guidance and the hook makes no project mutation

#### Scenario: Untrusted hook remains distinguishable
- **WHEN** the bundled hook has not been trusted or hooks are disabled
- **THEN** the host skips its command according to host policy and the plugin remains distinguishable from a plugin with no hook artifact

### Requirement: Lifecycle continuity respects host event semantics
The hook SHALL support the documented startup, resume, and compaction session-start forms supported by the host and SHALL use the host's plugin-root path contract without a hard-coded installation path.

#### Scenario: Resumed session receives the same bounded route
- **WHEN** Codex resumes or starts a session after compaction with the trusted plugin hook enabled
- **THEN** the hook emits the same bounded route without an index scan or duplicate plugin installation
