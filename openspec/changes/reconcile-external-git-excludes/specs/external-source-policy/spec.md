## Purpose

Apply existing external Git exclusion inputs consistently while preserving trusted operator capabilities and bounded source selection.

## ADDED Requirements

### Requirement: External exclusions have one effective selection contract
Full traversal, targeted refresh and absent-document selection SHALL use the same supported global and shared-worktree exclusion policy and established local-rule precedence. Git control-file discovery SHALL NOT disable no-Git ignore support.

#### Scenario: Quoted configured excludes path
- **WHEN** a supported selected Git config sets core.excludesFile to a valid quoted or escaped pathname containing spaces
- **THEN** matching files are excluded in both full and targeted selection, exact path contents and section/key identity are preserved, and normal whitespace-free/default controls still work

#### Scenario: Linked worktree shared excludes
- **WHEN** a selected linked worktree refers to a shared Git common directory with info/exclude rules
- **THEN** those rules apply consistently in full and targeted scans and document classification, without indexing shared Git control files as project source

#### Scenario: Local rules and no-Git compatibility
- **WHEN** local .ignore/.gitignore rules or negations override lower-precedence global/common rules, including a root without a Git repository
- **THEN** established precedence and ordinary nested-source navigation remain correct and explicit root aliases retain their selected identity

#### Scenario: Conflicting external whitelist precedence
- **WHEN** common info/exclude excludes a file and the global excludes file contains a conflicting whitelist for it
- **THEN** the common exclusion wins in full and targeted selection, while a higher-priority local whitelist still admits the file

### Requirement: Supported direct configuration selection is explicit
The external policy selector SHALL load only its supported direct system/global configuration sources with canonical value syntax and declared precedence, and SHALL NOT imply general Git repository configuration loading.

#### Scenario: Effective configured value and defaults
- **WHEN** supported direct system, XDG and home sources contain repeated core.excludesFile values
- **THEN** the last value in the highest-priority selected source wins, other sections/subsections do not authorize the key, quoting/escapes/comments/continuation preserve exact path values, empty values disable global excludes, and an absent value selects the existing XDG/home default

#### Scenario: Environment selection and supported path context
- **WHEN** GIT_CONFIG_GLOBAL or GIT_CONFIG_SYSTEM selects or explicitly disables its source, GIT_CONFIG_NOSYSTEM disables system configuration, or XDG_CONFIG_HOME selects global/default paths
- **THEN** the selected direct inputs and absent paths are observed, global override replaces ordinary XDG/home global files, absolute/selected-root-relative and leading-home paths retain their supported native meaning without changing the operator's environment

#### Scenario: Explicit unsupported config-loading boundary
- **WHEN** configuration contains include/includeIf, repository-local or Git runtime overrides, user-name tilde or installation-prefix features outside the existing direct global-policy boundary
- **THEN** no files are implicitly followed, no values are inferred from unread inputs, and documentation and proof state this support limit rather than claiming full Git configuration loading

### Requirement: Selected policy inputs remain observable and bounded
Policy construction and observation SHALL retain the supported selected config/exclude inputs, including absent paths and supported environment selection, within existing cancellation/resource controls. Configuration parsing SHALL reuse a canonical parser rather than a regex or custom syntax implementation.

#### Scenario: External policy changes
- **WHEN** a selected global config/exclude or common-worktree policy input is created, edited, removed or selected through a supported environment change
- **THEN** normal refresh reconciles affected admission and document outcomes without importing outside content as project source or repeatedly parsing config per entry

#### Scenario: Failure and operator compatibility
- **WHEN** configured external policy inputs are unreadable/malformed or controlled work is cancelled or exhausts a bound
- **THEN** existing typed failure/recovery semantics preserve the last valid publication, while valid explicit operator root/DB/config/state and external-policy paths remain usable

### Requirement: Existing compatible indexes repair without authored-state loss
The existing derivation fingerprint and atomic publication owner SHALL repair both previously admitted excluded files and omitted legitimate files when source bytes are unchanged. A second current-contract refresh SHALL avoid redundant parsing/publication.

#### Scenario: Upgrade, convergence and rollback
- **WHEN** a prior compatible installed/development index refreshes under the corrected policy
- **THEN** full/targeted/CLI/MCP outcomes converge, identity and all approved purposes survive, no-op is stable, and failure/cancellation/compatible rollback and re-upgrade preserve authored state

#### Scenario: Exact MCP root and read-only boundary
- **WHEN** CLI/MCP navigation selects another root, lacks initialized state or only queries a stale index
- **THEN** exact root/database isolation and existing typed recovery apply without implicitly mutating, initializing or substituting a sibling database
