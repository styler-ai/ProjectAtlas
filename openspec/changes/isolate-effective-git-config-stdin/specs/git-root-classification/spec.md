## ADDED Requirements

### Requirement: Effective Git configuration is independent of caller input
ProjectAtlas SHALL run its effective local Git `core.bare` query with a closed child stdin so root classification never waits for CLI or MCP transport input.

#### Scenario: Persistent MCP transport remains open
- **WHEN** an MCP client keeps the server stdin open while calling `atlas_session_brief`, `atlas_root`, or `atlas_init`
- **THEN** each tool completes its Git-root classification without waiting for transport shutdown

#### Scenario: Session remains reusable
- **WHEN** the client calls another root-sensitive tool immediately after the startup probes
- **THEN** the same MCP session returns the correct root result without a stale or blocked Git child

### Requirement: Existing Git-root outcomes remain compatible
The stdin isolation MUST preserve existing effective-config inclusion, missing-key, linked-worktree, bare-root, wrong-root, missing-index, and no-implicit-mutation behavior.

#### Scenario: Effective value comes from an included local config
- **WHEN** local Git configuration includes a file that sets `core.bare`
- **THEN** ProjectAtlas classifies the root from Git's effective included value

#### Scenario: Local key is absent
- **WHEN** `git config --get` returns its standard missing-key status
- **THEN** ProjectAtlas treats `core.bare` as unset rather than as a probe failure

#### Scenario: Selected root is not an initialized atlas
- **WHEN** a read-only MCP tool addresses a wrong root or a root without a ProjectAtlas index
- **THEN** ProjectAtlas returns the existing typed guidance and does not create project state implicitly

#### Scenario: Linked and bare control roots retain their guidance
- **WHEN** root selection encounters a linked worktree or a bare/common Git control root
- **THEN** ProjectAtlas preserves the existing worktree-local routing or typed refusal before database access
