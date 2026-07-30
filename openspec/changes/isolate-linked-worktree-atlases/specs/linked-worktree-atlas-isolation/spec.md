## ADDED Requirements

### Requirement: Selected roots are scanned recursively without boundary escape
ProjectAtlas SHALL recursively index every eligible nested folder beneath the selected project or linked-worktree root while pruning Git-ignored and ProjectAtlas-ignored directories before descendant file, text, purpose, summary, symbol, or graph work.

#### Scenario: Deep eligible source
- **WHEN** eligible source files exist at several nested depths beneath the selected root
- **THEN** scan indexes their folders, saved bytes, text, summaries, symbols, purposes, and graph facts subject to declared resource limits

#### Scenario: Ignored nested source
- **WHEN** a nested parent is excluded by `.gitignore` or stricter ProjectAtlas ignore policy
- **THEN** the parent subtree contributes no indexed folders, files, text, purposes, summaries, symbols, relationships, or graph rows

#### Scenario: Main repository contains ignored worktrees
- **WHEN** the main ProjectAtlas repository is scanned with `/.worktrees/` in its root `.gitignore`
- **THEN** sources and ProjectAtlas databases beneath `.worktrees` are pruned before descent and cannot enter the main database

#### Scenario: Registered linked worktree is not ignored
- **WHEN** another linked worktree registered to the same common Git repository is physically beneath the selected root and its container has no ignore rule
- **THEN** ProjectAtlas identifies its canonical registered root before traversal and excludes that complete checkout from the selected root's index

#### Scenario: Unrelated nested repository
- **WHEN** an unrelated nested Git repository is beneath the selected root and is not ignored
- **THEN** ProjectAtlas does not exclude it merely for containing Git metadata because it is not a registered worktree of the same common repository

#### Scenario: Worktree boundary cannot be validated
- **WHEN** ProjectAtlas detects relevant linked-worktree metadata but cannot read or safely normalize the registered boundary
- **THEN** scan returns typed root-policy guidance without publishing a potentially contaminated generation

### Requirement: First use creates one worktree-local atlas through explicit initialization
ProjectAtlas SHALL recognize a selected linked-worktree directory as its own project root and SHALL let the agent invoke `atlas_init` automatically for that exact root without choosing a database filename.

#### Scenario: Agent selects an uninitialized linked worktree
- **WHEN** the shipped agent workflow selects a linked-worktree root with no ProjectAtlas state
- **THEN** it invokes `atlas_init` for that `project_path` and receives a verified local database, config, host configs, project identity, and initial recursive scan

#### Scenario: Read-only use before initialization
- **WHEN** an ordinary read-only CLI or MCP command addresses an uninitialized worktree
- **THEN** ProjectAtlas returns typed `init_required` guidance with `atlas_init` as the exact next call and creates no project state

#### Scenario: First write scan
- **WHEN** `scan` is intentionally used as the first write command for a selected root
- **THEN** it uses the same local create-or-supported-migrate boundary and never selects a sibling or common Git directory database

### Requirement: Linked worktrees keep independent root, database, and source identity
Each linked worktree SHALL own its `.projectatlas` database, config, generated host working directory, project identity, source hashes, reviewed purposes, summaries, symbols, graph generation, and refresh lifecycle.

#### Scenario: Branch-only source
- **WHEN** two linked worktrees contain different branch-only files or saved dirty bytes
- **THEN** each atlas reports only the source beneath its selected worktree root and preserves independent project identity

#### Scenario: Common Git directory and sibling worktree
- **WHEN** a worktree's `.git` control file points into a common Git directory and another linked worktree exists
- **THEN** neither the common Git directory nor sibling source becomes a source root, database owner, or indexed descendant

#### Scenario: Per-call MCP routing
- **WHEN** one MCP server receives interleaved calls with two initialized worktree `project_path` values
- **THEN** every call uses the addressed worktree database without changing a process-global root or crossing identity

#### Scenario: Bare repository root
- **WHEN** a bare/common Git repository root with no checked-out source is selected
- **THEN** ProjectAtlas returns typed worktree-selection guidance and does not create a source database there

#### Scenario: Incremental worktree refresh
- **WHEN** nested files are added, edited, deleted, or changed by a branch switch and `watch --once` or scan refreshes the selected worktree
- **THEN** the refresh preserves the same root and identity boundary and publishes only that worktree's complete affected state
