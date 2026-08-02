## ADDED Requirements

### Requirement: Source privacy covers every Git-visible text file
ProjectAtlas SHALL reject machine-local absolute paths in tracked and non-ignored untracked text regardless of source extension and SHALL keep diagnostics privacy-safe.

#### Scenario: Supported text encodings contain a private path
- **WHEN** UTF-8, UTF-8 BOM, UTF-16 little-endian BOM, or UTF-16 big-endian BOM source begins with any forbidden private-path family
- **THEN** the shared decoder and path policy reject it with only repository-relative file, line, column, rule, and portable guidance

#### Scenario: Git-visible non-Rust source contains a private path
- **WHEN** a script, workflow, configuration file, document, symlink target, or extensionless text file contains a forbidden machine-local path
- **THEN** the same repository policy rejects it without an extension allowlist

#### Scenario: A root-owned Unix home is committed
- **WHEN** Git-visible text contains a root home path or its file URI form
- **THEN** the Unix home policy rejects it like every named-user home path

#### Scenario: Binary or malformed source is inspected
- **WHEN** a Git-visible file is binary
- **THEN** it remains outside the text policy
- **AND WHEN** a non-binary source encoding is malformed
- **THEN** the gate fails closed without echoing its private bytes

#### Scenario: Hostile source contains many private paths
- **WHEN** a complete current-tree or history scan finds more private paths than the diagnostic limit
- **THEN** the gate reports the complete count and emits only a bounded redacted sample while exact published-base comparison remains independent of that diagnostic limit

### Requirement: Newly reachable history cannot hide private paths
ProjectAtlas SHALL scan every newly reachable revision and path identity before accepting a push, pull request, package, parser pack, documentation deployment, or release.

#### Scenario: A clean tip follows a private intermediate revision
- **WHEN** an outgoing or hosted range introduces a forbidden path and removes it before the tip
- **THEN** the history gate rejects the range even though the current tree is clean

#### Scenario: One blob appears through several paths or object formats
- **WHEN** identical private bytes appear at several paths in SHA-1 or SHA-256 Git history
- **THEN** every path identity is inspected and no object-only deduplication hides a violation

#### Scenario: Revision input is missing or malformed
- **WHEN** the event range, revision, object response, or update row cannot be proven
- **THEN** the gate fails closed without treating unknown history as clean

### Requirement: Source privacy precedes product builds
Every local or hosted ProjectAtlas product build, package, parser-pack construction, documentation deployment, and release SHALL depend on successful current-tree and applicable history privacy gates.

#### Scenario: CI or release compiles the workspace
- **WHEN** a CI, pre-push, or release job reaches workspace check, Clippy, test, documentation, package, or publish commands
- **THEN** the exact source revision has already passed current-tree and newly reachable history policy

#### Scenario: An independent artifact workflow starts
- **WHEN** documentation, optional-parser, or release workflows run independently of the main CI run
- **THEN** they fetch complete history and enforce their own exact range before product compilation, artifact upload, deployment, or publication

#### Scenario: Auto-release dispatches publication
- **WHEN** main triggers automatic release dispatch
- **THEN** the release workflow blocks every package and publish job behind its own source-policy verification
