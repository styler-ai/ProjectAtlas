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

#### Scenario: Selected subroot inherits ancestor policy
- **WHEN** an explicitly selected repository subdirectory inherits ancestor .ignore, .gitignore or common info/exclude rules
- **THEN** targeted admission agrees with full traversal under the same selected-root identity and does not silently discard inherited rules

#### Scenario: Negation below an ignored parent
- **WHEN** a leaf negation occurs below a parent directory pruned by the configured ignore engine
- **THEN** targeted selection does not reopen that leaf when full traversal excludes the parent

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

#### Scenario: Continuous watch outside the source root
- **WHEN** a selected external policy file, absent parent or supported symlink ancestor is created, edited, removed or replaced during continuous CLI/MCP watch
- **THEN** the existing watcher observes and routes the event to reconciliation without requiring a source-root edit, preserves continuity and cleanup, and never admits the outside path as project source

#### Scenario: Selection changes during a watch session
- **WHEN** a supported selected config changes the effective excludes pathname
- **THEN** accepted reconciliation updates external watch coverage, including absent paths, and discovers later edits to the newly selected input without per-event-path reconstruction of global policy

#### Scenario: A selected policy parent is a regular file
- **WHEN** a selected policy pathname requires traversing an existing regular file as a directory, directly or through a symlink target
- **THEN** watch admission refuses the impossible parent and releases partial native coverage on every supported platform, even when the operating system reports the child as missing
- **AND** regular-file leaves, genuinely absent parent chains and native ordinary Win32 parent normalization retain their supported behavior

#### Scenario: Selected source directory is replaced
- **WHEN** a supported selected source directory is replaced at the same canonical pathname during CLI/MCP observation while its explicit external database and configuration remain available
- **THEN** structural uncertainty invalidates the prior warm epoch, recursive coverage is restored before cold source sampling, and accepted refresh/read results reflect the replacement source while preserving database/project identity and all approved purposes
- **AND** later replacement-directory child edits remain observable, no conventional database is implicitly created, and existing root/config/registered-lifecycle admission remains enforced
- **AND** the rebind marker and event/sequence publication share the source-observation ingress fence; ordinary access without rescan does not request a rebind, and cancellation or failed rebinding retains uncertainty under the original work budget

#### Scenario: Policy-only document outcome
- **WHEN** an exclusion changes the ignored-versus-missing classification of an absent document target without changing the indexed source inventory
- **THEN** accepted refresh and subsequent CLI/MCP graph queries agree with full reconciliation and do not preserve a stale document reason merely because source nodes are unchanged

#### Scenario: Incremental watch limits require complete reconciliation
- **WHEN** a continuous notify batch exceeds the incremental entry/source-byte or dependency-closure bound
- **THEN** the controlled production owner performs complete reconciliation with the original operation control, discovering current sources outside the retained event set and publishing one complete generation while preserving identity and approved purposes
- **AND** source and document-only batches share this behavior, cancellation/deadline and unrelated failures propagate without a new polling budget, and stopped work preserves the last valid publication

#### Scenario: Database aliases distinguish runtime state from source events
- **WHEN** an explicit operator DB symlink or supported managed Windows short name uses a different spelling from SQLite's actual runtime cohort
- **THEN** main/WAL/SHM/journal self-events in the selected and resolved target namespaces do not fabricate source drift, and real CLI/MCP reads and purpose updates remain usable without retargeting the selected SQLite path
- **AND** distinct source symlink/hardlink names pointing at a DB object, aliases to the metadata directory, main-file replacements, alias retargets and selected config/ignore changes remain observable
- **AND** the observer captures the target in its binding identity, does not acknowledge using a changed or unknown target, and uses bounded native filename evidence rather than assuming that an 8.3 alias proves case insensitivity

#### Scenario: Observed document-only target transition
- **WHEN** an ignored document target becomes an outside-root symlink or returns to an ordinary ignored file while a verified CLI/MCP epoch is warm
- **THEN** the document-only event invalidates that epoch even when the source-path set is empty, without requesting a recursive rebind for an ordinary target edit
- **AND** subsequent read-only queries return the existing typed refresh guidance without mutating last-valid publication, and explicit refresh reconciles the reason with unchanged source parses while preserving identity and every approved purpose
- **AND** local .ignore, no-Git .gitignore and selected global exclusions retain the same behavior, while database, ordinary access and unrelated outside events retain existing filtering

#### Scenario: External policy changes during document staging
- **WHEN** a selected external rule changes a retained absent-document outcome after staging while source nodes remain identical
- **THEN** final publication revalidates those outcomes against freshly discovered policy using the original configured scan options and operation control, refusing stale staged results before the transaction
- **AND** last-valid publication and authored state survive refusal, and a subsequent stable retry converges and becomes a no-op

#### Scenario: Failure and operator compatibility
- **WHEN** configured external policy inputs are unreadable/malformed or controlled work is cancelled or exhausts a bound
- **THEN** existing typed failure/recovery semantics preserve the last valid publication, while valid explicit operator root/DB/config/state and external-policy paths remain usable

#### Scenario: Uncached document probes retain source admission
- **WHEN** the policy-only freshness probe or ordinary projection inspects an absent derived document target with an escaping native prefix, including an escape followed by reentry
- **THEN** both consumers return the existing typed source-boundary reason before target-specific ignore contents are read, sharing the same admission owner and original cooperative control
- **AND** cached checks and staged revalidation honor cancellation, while normal local targets and explicit operator global/shared-worktree policy retain their supported behavior

#### Scenario: Non-regular input and regular symlink controls
- **WHEN** a selected external input is unsupported non-regular state or a valid regular file reached through a supported symlink
- **THEN** unsupported input fails promptly without publishing partial state, while the regular-file and regular-symlink workflows remain usable under the same supported selection semantics

#### Scenario: Non-regular native ignore inputs
- **WHEN** a root, inherited or admitted nested .ignore, .gitignore or common Git info/exclude input is an unsupported stream or device
- **THEN** full and targeted admission refuse before the canonical parser opens that input, preserve the last valid publication, and retain ordinary regular-file and regular-symlink behavior

#### Scenario: Pruned subtrees do not load child policy
- **WHEN** a parent is excluded by enabled repository or configured policy and contains a non-regular child ignore input
- **THEN** full and targeted selection preserve parent pruning without opening or inspecting policy inside the excluded subtree, and a Git-only compatibility helper does not inspect a disabled ignore family

### Requirement: Document policy verification has bounded reusable work
Document resolution and the cold unresolved-document probe SHALL reuse native hierarchical policy within one pass, bound cumulative local policy parsing and retained directory contexts, and preserve fresh matching in later passes and publication revalidation. Pagination alone SHALL NOT be treated as a total policy-work bound.

#### Scenario: Multiple pages reuse loaded policy
- **WHEN** a cold probe checks distinct absent targets across multiple keyset pages under the same local policy
- **THEN** one pass reuses its loaded native policy rather than rebuilding it for each row, without retaining a repository-wide target-reason cache or imposing a document-row cutoff
- **AND** the original cooperative control applies across all pages and later operations discover fresh policy

#### Scenario: Policy work and retained contexts exhaust their bounds
- **WHEN** local hierarchical policy parsing or directory-context loads exceed the pass limits, including reloads after bounded-cache eviction
- **THEN** work refuses before the next disallowed policy parse with typed resource, limit and observed work, preserving last-valid publication and authored state
- **AND** the CLI uses existing full-scope PolicyUnavailable verification guidance rather than an incremental-limit fallback that restarts the quota
- **AND** defaults admit at most 128 MiB of local policy input and 1,000,000 context loads per matcher/pass; staging and fresh publication revalidation have separate pass accounting under one operation deadline

#### Scenario: Reuse preserves pruning and positive workflows
- **WHEN** a pass traverses nested or inherited policy, ignored parents, regular symlinks, selected-root aliases or legitimate external Git inputs
- **THEN** each parent is matched before its child policy is inspected, admitted inputs are validated and charged before parsing, existing precedence and source-prefix admission remain effective, and cancellation/deadline retain their original typed failures
- **AND** retained child context input/key estimates use soft 256-context / 4 MiB limits with between-target eviction, allowing one current ancestor chain under the cumulative pass limits, while an already warmed root seed is shared without reparsing it
- **AND** the existing single-path Rust API, closed filesystem/core error enums, CLI commands, MCP tools and graph schema remain compatible

### Requirement: Existing compatible indexes repair without authored-state loss
The existing derivation fingerprint and atomic publication owner SHALL repair both previously admitted excluded files and omitted legitimate files when source bytes are unchanged. A second current-contract refresh SHALL avoid redundant parsing/publication.

#### Scenario: Upgrade, convergence and rollback
- **WHEN** a prior compatible installed/development index refreshes under the corrected policy
- **THEN** full/targeted/CLI/MCP outcomes converge, identity and all approved purposes survive, no-op is stable, and failure/cancellation/compatible rollback and re-upgrade preserve authored state

#### Scenario: Exact MCP root and read-only boundary
- **WHEN** CLI/MCP navigation selects another root, lacks initialized state or only queries a stale index
- **THEN** exact root/database isolation and existing typed recovery apply without implicitly mutating, initializing or substituting a sibling database
