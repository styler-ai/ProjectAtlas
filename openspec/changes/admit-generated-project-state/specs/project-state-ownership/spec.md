## Purpose

Keep automatic initialization and generated project state within its admitted owner
while preserving explicit operator-selected filesystem capabilities.

## ADDED Requirements

### Requirement: Automatic state destinations are admitted before writes
Automatic project-state access SHALL establish the conventional database and WAL/SHM
ownership before SQLite opens, including root discovery and read-only navigation.
Initialization SHALL establish its output ownership before creating or editing default configuration, nonsource metadata,
database/WAL, hydration state or generated host files. Unsupported automatic
destinations SHALL be refused without outside-owner side effects.

#### Scenario: Unsupported project-state directory
- **WHEN** ordinary CLI/MCP or registered-worktree initialization selects an unsupported automatic state directory
- **THEN** it reports a causal refusal before generated writes and leaves external fixture files, prior identity and authored state unchanged

#### Scenario: Read-only navigation can initialize SQLite sidecars
- **WHEN** CLI/MCP navigation, settings/root discovery or another managed database consumer opens a conventional project database with WAL/SHM sidecars
- **THEN** automatic path ownership is checked before SQLite access, even with telemetry disabled, and refused aliases cannot create or modify outside-owned state
- **AND** valid navigation remains available when generated host files are absent or invalid because database access does not require generated-output admission

#### Scenario: Automatic file aliases and replaced destinations
- **WHEN** an automatic generated output has an unsafe existing leaf alias or its owning directory changes before publication
- **THEN** the writer preserves ownership at its actual mutation boundary, refuses unsupported state or publishes only its own entry, and does not follow the alias to alter outside-owned bytes

#### Scenario: Manual helpers mutate managed documents
- **WHEN** ignore edits change a managed default or legacy config, Gitignore initialization finds an existing or racing directory entry, or a map request writes the default JSON sidecar
- **THEN** the shared CLI/MCP writer protects only the exact managed destination, preserves outside alias targets, valid permissions and ordinary creation modes, and reports Gitignore creation truthfully without clobbering an existing entry
- **AND** no-change config operations, existing-entry Gitignore no-ops, explicit custom/external config and map destinations, and the legacy TOON alias refusal retain their supported behavior

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
