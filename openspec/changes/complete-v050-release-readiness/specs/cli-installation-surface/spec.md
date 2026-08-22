## ADDED Requirements

### Requirement: npm is a verified adapter for the existing native runtime
The v0.5 npm package SHALL declare one package identity, supported npm/Node floor, supported OS/architecture tuples, exact package-to-release-asset/version/SHA-256 mapping, and installer/cache ownership. It SHALL stage, verify, and atomically activate the existing native runtime and SHALL preserve arguments, stdout, stderr, exit status, signals, formats, selected root/config/database, and MCP behavior.

#### Scenario: Supported tuple
- **WHEN** a supported tuple installs or explicitly materializes the package
- **THEN** the matching asset is fetched or reused, version and digest are verified, activation is atomic, and CLI plus MCP smoke succeeds through the native runtime

#### Scenario: Lifecycle scripts are disabled
- **WHEN** the package manager does not run lifecycle scripts
- **THEN** the package exposes the documented explicit materialization route or a typed actionable refusal and never launches or registers an unverified runtime

#### Scenario: Unsupported or invalid artifact
- **WHEN** the tuple, asset, version, digest, proxy/offline state, or cache cannot satisfy the contract
- **THEN** installation/materialization fails deterministically without changing host registration or project state

#### Scenario: Concurrent or interrupted materialization
- **WHEN** two processes materialize the same version or one is interrupted
- **THEN** process-safe locking and staged activation leave either the prior verified cache or one complete verified new cache, never a partial executable

### Requirement: Generated host configuration is consumed by real readers
The installer SHALL verify Claude Code and OpenCode configuration through each available actual host reader in an isolated home/config root and SHALL establish a real MCP session with exact source-evidence readback.

#### Scenario: Available host
- **WHEN** a supported host is installed
- **THEN** its real reader accepts the generated native schema, absolute runtime/database/config paths, version guard, routing fields, and launches the verified runtime through MCP initialize/session/navigation

#### Scenario: Missing host
- **WHEN** the host executable is absent
- **THEN** verification reports a typed skip and does not claim host consumption

#### Scenario: Invalid or stale configuration
- **WHEN** owned configuration is invalid, stale, or points to the wrong runtime
- **THEN** the installer reports or repairs only its owned fields and re-runs the real reader without editing unrelated settings, credentials, authentication, or project data

#### Scenario: Shared registry and explicit project routing
- **WHEN** one host serves several projects
- **THEN** the default registry route and explicit per-call project root remain isolated and return evidence from the selected database only

### Requirement: One collision-safe atlas command exposes the complete CLI
The installer SHALL provide one `atlas` forwarder on Windows, Linux, and macOS to the same verified `projectatlas` runtime. It SHALL accept every present/future argument vector without per-subcommand executables and SHALL preserve `projectatlas` compatibility.

#### Scenario: Canonical command or nested command
- **WHEN** a user invokes any supported command, global flag, nested command, or format through `atlas`
- **THEN** stdout/stderr bytes, exit status, signals, JSON/TOON, help, errors, root isolation, and behavior match `projectatlas`

#### Scenario: Unmanaged collision
- **WHEN** another executable already owns the intended `atlas` location or effective PATH
- **THEN** installation reports the exact collision and does not shadow or overwrite it without explicit collision resolution

### Requirement: Health reporting and administration remain unambiguous
`atlas health [report flags]` SHALL run the read-only health report, `atlas health resolve ...` SHALL retain the existing administrative route, and `health-check` SHALL remain a compatibility alias.

#### Scenario: Report flags versus resolve subcommand
- **WHEN** the user supplies zero arguments, report flags, or the `resolve` subcommand
- **THEN** parsing selects the documented read-only or administrative owner without ambiguity and help/completion describe the same split

#### Scenario: Legacy automation
- **WHEN** automation invokes `projectatlas health-check`
- **THEN** the read-only report remains byte/schema/error compatible through the v0.5 contract

### Requirement: Installer lifecycle removes only owned state
Install, update, repair, and uninstall SHALL manage the verified runtime, npm cache/registration, `atlas` shim, completions, and generated host configuration without deleting unrelated executables, credentials, databases, caches, or configuration.

#### Scenario: Update or repair
- **WHEN** an owned shim/config/cache is stale or missing
- **THEN** it is repaired atomically to the exact verified runtime and revalidated through its real boundary

#### Scenario: Uninstall
- **WHEN** ProjectAtlas is removed
- **THEN** only proven-owned artifacts are removed and selected project databases/configuration plus unrelated host state remain according to the documented retention contract
