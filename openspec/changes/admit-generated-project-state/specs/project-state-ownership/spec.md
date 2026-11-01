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

#### Scenario: Managed paths include unresolved parent reductions
- **WHEN** a selected database or fixed managed document is spelled through an existing or missing parent followed by `..`
- **THEN** existing parents use their actual filesystem resolution, and an unresolved spelling that could reduce to the managed parent is refused before creating directories or writing files
- **AND** ownership uncertainty never silently selects an external writer; selected-root aliases and physically external symlink-parent paths retain their supported behavior

#### Scenario: Managed case aliases follow macOS volume behavior
- **WHEN** a selected fixed filename or private-directory component is a case alias on macOS, including native Unicode spellings and APFS expanding folds
- **THEN** classification uses the actual containing volume's case behavior, protects case aliases before database/document writes even when the main file is absent, and propagates unresolved ownership rather than choosing an external writer
- **AND** case-distinct custom names on case-sensitive volumes remain available, including separate hardlink entries and unrelated external paths

#### Scenario: Windows managed names respect directory-local case sensitivity
- **WHEN** a fixed database, sidecar, document or private-directory component differs in case on Windows
- **THEN** ownership follows the actual containing directory's native naming mode, including correctly resolved missing parents and inherited case flags
- **AND** distinct selected custom entries on sensitive directories do not redirect writes into lowercase managed entries; insensitive aliases remain protected, exact names remain usable, and uncertain native queries refuse before side effects

#### Scenario: Replacement preserves existing access controls
- **WHEN** an admitted generated host config, managed TOML config or JSON sidecar is replaced
- **THEN** publication preserves its existing access ACL/DACL and effective ownership, including applicable inheritance protection, and does not broaden access through the staging directory's defaults
- **AND** unsupported or failed metadata preservation refuses before replacing valid output; ordinary regeneration, outside-hardlink preservation and owned staging cleanup remain available

#### Scenario: Linux managed aliases follow directory casefolding
- **WHEN** a selected fixed component aliases a managed name in a Linux casefolded directory, including inherited ext4/f2fs Unicode 12.1 behavior
- **THEN** directory-native classification protects database/sidecar and document operations before side effects, including absent leaves and parent reductions
- **AND** distinct sensitive-directory custom entries and external paths remain usable; a potential alias with an unrecognized native comparison model reports ownership uncertainty instead of selecting an external writer

#### Scenario: Linux content-sealed outputs remain protected
- **WHEN** a managed replacement destination is fs-verity-protected initially or becomes sealed before final publication
- **THEN** retained-inode admission refuses replacement and preserves its bytes, protection, outside hardlinks and owned cleanup
- **AND** ordinary replacement, genuinely new outputs and unrelated explicit custom paths remain available without re-sealing or privilege changes

#### Scenario: Mandatory labels and pre-copy access remain protected
- **WHEN** an existing Linux output has SELinux or Smack file-access labels, or a replacement stage could inherit broader native access than its destination
- **THEN** the writer captures the admitted file's policy without following an alias and never writes generated bytes into a named stage whose initial access policy is broader or different
- **AND** Linux changed-policy stages receive ownership, ACLs, mode and applicable file labels and complete bytes while anonymous, with exact readback before naming; ordinary equivalent-policy regeneration does not require relabel privilege
- **AND** macOS replacement payload creation occurs only after any private staging directory's inherited ACLs and permissions are made restrictive and verified; genuinely new outputs retain normal parent inheritance
- **AND** unsupported policy preservation or cleanup reports a causal failure while retaining valid output and foreign entries; a post-publication cleanup error explicitly identifies the already committed publication

#### Scenario: Native ACL operations retain object identity across name substitution
- **WHEN** another local principal replaces an admitted macOS file/directory or Windows staging name before a native ACL operation
- **THEN** ACL reads and writes remain bound to the retained original descriptor/handle and preserve the foreign object's native policy and bytes
- **AND** metadata preservation does not reopen a mutable project pathname or fall back to a following setter; ordinary same-owner Windows regeneration does not demand unused take-ownership rights
- **AND** Windows releases its destination snapshot only after the final identity/policy checks so the retained query handles do not prevent normal atomic replacement
- **AND** Windows publication renames the retained stage handle, preserves no-clobber collisions and never cleans a substituted former stage name after success

#### Scenario: Windows system access policy survives replacement
- **WHEN** an existing generated output has Windows integrity, audit or other SACL policy
- **THEN** the writer preserves that policy through retained handles and native readback, or refuses before payload publication when complete preservation cannot be proved
- **AND** ordinary source and stage objects with proven absent SACLs remain replaceable without additional privilege; create-only outputs retain normal inheritance
- **AND** a label-only query does not mistake filtered audit ACEs for policy absence, source-policy changes refuse replacement, and unsupported defaulted SACL state retains valid output
- **AND** no process-wide privilege enablement or mutable-path security setter is introduced
- **AND** complete retained queries preserve current supported SDK system-policy classes, including resource attributes and scoped policy alongside audit and integrity policy; unsupported serialization refuses before staging, while independent native readback preserves exact source policy and outside hardlinks

#### Scenario: Windows file encryption remains protected
- **WHEN** an existing managed output is EFS encrypted initially or becomes encrypted before publication
- **THEN** retained-file admission refuses replacement before staging or final publication and preserves its bytes, encryption policy, outside hardlinks and owned cleanup
- **AND** a successful permissions setter or matching encryption flag alone does not establish complete EFS key-policy preservation
- **AND** genuinely new files retain encrypted-directory inheritance, while ordinary replacement, indexing, database selection and explicit custom/external output capabilities remain available
- **AND** create-only collisions with existing or racing encrypted direct regular files retain the established not-created result without replacement-policy admission

#### Scenario: Windows replacement stages do not expose lasting inherited capabilities
- **WHEN** a principal can create entries and inherit access to new children but cannot modify or delete the existing protected output
- **THEN** replacement staging prevents that principal from acquiring data, delete or write-DACL capabilities before destination policy is applied, including implicit owner write-DACL rights
- **AND** retained creator data and policy operations, ordinary replacement and owned cleanup remain supported; genuinely new outputs retain normal parent inheritance
- **AND** later policy tightening, a final identity check or handle-based rename alone is not treated as revocation of already granted handles

#### Scenario: Legacy configuration admission precedes identity repair
- **WHEN** initialization selects a missing managed legacy configuration and a current database needs native root identity repair
- **THEN** an unsafe legacy destination is refused before database or sidecar mutation, preserving the prior incomplete binding for a supported retry

### Requirement: Supported explicit paths remain available
Explicit operator-selected database/config paths and canonical selected-root aliases
SHALL retain their supported behavior and existing identity/configuration safeguards.
Automatic output admission SHALL NOT become a blanket ban on external paths.

#### Scenario: Explicit external database and configuration
- **WHEN** a caller intentionally selects a supported external database or config path with a valid project root
- **THEN** CLI/MCP initialization and subsequent navigation/refresh operate normally and generated project files retain their admitted owner

#### Scenario: Custom names inside the private directory
- **WHEN** initialization explicitly selects a custom config filename directly inside `.projectatlas`, including a dangling alias to an operator-selected missing target
- **THEN** fixed managed names use their guarded writer while custom names use the explicit path route, preserving custom creation and repeated-init behavior
- **AND** CLI and configured MCP custom config/map writes preserve operator-selected behavior regardless of their parent directory

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
