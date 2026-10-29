## Purpose

Keep automatic initialization and generated project state within its admitted owner
while preserving explicit operator-selected filesystem capabilities.

## ADDED Requirements

### Requirement: Automatic state destinations are admitted before writes
Initialization SHALL establish the automatic project-state directory and output
ownership before creating or editing default configuration, nonsource metadata,
database/WAL, hydration state or generated host files. Unsupported automatic
destinations SHALL be refused without outside-owner side effects.

#### Scenario: Unsupported project-state directory
- **WHEN** ordinary CLI/MCP or registered-worktree initialization selects an unsupported automatic state directory
- **THEN** it reports a causal refusal before generated writes and leaves external fixture files, prior identity and authored state unchanged

#### Scenario: Automatic file aliases and replaced destinations
- **WHEN** an automatic generated output has an unsafe existing leaf alias or its owning directory changes before publication
- **THEN** the writer preserves ownership at its actual mutation boundary, refuses unsupported state or publishes only its own entry, and does not follow the alias to alter outside-owned bytes

### Requirement: Supported explicit paths remain available
Explicit operator-selected database/config paths and canonical selected-root aliases
SHALL retain their supported behavior and existing identity/configuration safeguards.
Automatic output admission SHALL NOT become a blanket ban on external paths.

#### Scenario: Explicit external database and configuration
- **WHEN** a caller intentionally selects a supported external database or config path with a valid project root
- **THEN** CLI/MCP initialization and subsequent navigation/refresh operate normally and generated project files retain their admitted owner

#### Scenario: Valid existing projects and root aliases
- **WHEN** an existing valid project, linked worktree or supported canonical root alias is initialized again
- **THEN** its identity, authored configuration, all approved purposes and valid publication remain intact while normal generated host configuration remains available

### Requirement: Initialization adapters share the write invariant
Ordinary CLI/MCP, registered-worktree hydration, root transitions and direct generated-host writers
SHALL preserve the same automatic output ownership and failure semantics using the
existing bootstrap/filesystem owners. Public schema and wire shapes remain compatible.

#### Scenario: Native installed paths agree
- **WHEN** the same supported or refused fixture is exercised through real installed CLI and MCP initialization, including registered initialization where applicable
- **THEN** they agree on admission and side effects; normal host configs and public navigation work after success and refusal preserves the prior valid state

#### Scenario: Failure and compatible upgrade
- **WHEN** creation, activation or generation fails or a compatible existing installation is upgraded/reopened
- **THEN** only owned operation artifacts are cleaned up, prior authored state is preserved and a normal supported retry remains usable; a committed root transition retains its existing honest follow-up-error contract for subsequent unrelated I/O failure
