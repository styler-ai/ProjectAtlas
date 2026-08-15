## ADDED Requirements

### Requirement: Structural worktree discovery is bounded and read-only

ProjectAtlas SHALL classify a supplied directory as a true non-Git root, an exact primary or linked worktree, a Git common manager, or invalid Git evidence by reading bounded structural metadata without starting Git or mutating the filesystem.

#### Scenario: Primary and linked worktrees are discovered

- **WHEN** a repository has a primary checkout and one or more valid registered linked worktrees at arbitrary paths
- **THEN** discovery returns their canonical roots, primary/linked roles, Git administrative directories, common control root, and active state in deterministic order

#### Scenario: Missing registration remains status evidence

- **WHEN** Git retains a registered worktree whose reciprocal checkout control path is absent
- **THEN** status reports it as missing and never selects it as source

#### Scenario: Unsafe control metadata fails closed

- **WHEN** a selected control path is a symlink/junction, oversized, malformed, outside the common directory, missing its required target, or disagrees with reciprocal evidence
- **THEN** ProjectAtlas returns typed invalid Git evidence without following it, guessing a source root, or mutating Git

#### Scenario: Registration inventory exceeds the bound

- **WHEN** structural registrations exceed the fixed discovery ceiling
- **THEN** discovery fails with a bounded resource error rather than returning a partial authoritative inventory

#### Scenario: Git executable is unavailable

- **WHEN** Git is absent from `PATH`
- **THEN** structural discovery and exact-root ProjectAtlas operation remain available because no Git process is required

### Requirement: Source selection never guesses among worktrees

ProjectAtlas SHALL resolve each source operation to one exact canonical worktree or return the existing `worktree_required` failure.

#### Scenario: Addressed worktree stays exact

- **WHEN** a caller starts inside a primary or linked worktree, including a nested directory
- **THEN** source selection resolves to that worktree and uses only its configured database and generation

#### Scenario: One manager worktree is unambiguous

- **WHEN** a Git common manager has exactly one structurally active worktree
- **THEN** source selection may resolve to that exact worktree without another user choice

#### Scenario: Manager has several worktrees

- **WHEN** a common manager has multiple structurally active worktrees
- **THEN** source operations return `worktree_required` and direct the agent to supply an exact worktree path

#### Scenario: True non-Git project remains zero ceremony

- **WHEN** no structural Git boundary contains the supplied path
- **THEN** ProjectAtlas preserves the existing non-Git exact-root behavior and local atlas path

### Requirement: Agent worktree status uses existing root surfaces

ProjectAtlas SHALL expose content-free structural worktree state through CLI `root status` and MCP `atlas_root` with `control_root`, without adding a new tool family or UI.

#### Scenario: CLI and MCP status agree

- **WHEN** CLI and MCP inspect the same checkout or Git common directory
- **THEN** both return the same control root, selection state, selected root when unambiguous, worktree-required flag, role/state rows, truncation flag, and blockers

#### Scenario: Public status is bounded

- **WHEN** more rows exist than the public status ceiling
- **THEN** ProjectAtlas returns the deterministic prefix and marks the inventory truncated

#### Scenario: Control status is not database verification

- **WHEN** MCP supplies `control_root`
- **THEN** it cannot combine that request with `project_path` or database verification and performs no atlas write or Git mutation

### Requirement: One MCP process preserves exact worktree isolation

ProjectAtlas SHALL retain request-captured exact-root authority for every per-call `project_path` in a long-lived shared MCP process.

#### Scenario: Interleaved sibling reads do not bleed

- **WHEN** one MCP process receives interleaved search, summary, slice, graph, task, purpose, or status calls for initialized sibling worktrees
- **THEN** every response uses only the requested worktree's database, source, generation, and state

#### Scenario: Worktree-local writes do not alter a sibling

- **WHEN** init, scan, watch, purpose, or other mutation runs for one worktree
- **THEN** the sibling atlas identity, publication generation, authored purposes, graph, tasks, and source state remain unchanged

#### Scenario: Current TUI remains selected-root only

- **WHEN** the existing token TUI is opened from a worktree
- **THEN** it continues to show only that selected worktree's existing token and graph data and does not become a repository manager UI
