## ADDED Requirements

### Requirement: Rust 1.98.0 is the sole deterministic toolchain authority
ProjectAtlas v0.5.0 SHALL declare exact Rust 1.98.0 in one repository toolchain source. Local validation, CI, optional-parser construction, packaging, installer-developer validation, and release execution SHALL select and verify that declaration before expensive or mutating work. Rust 1.93.1 is historical reproduction evidence only; floating stable and duplicated numeric workflow/test pins are forbidden.

#### Scenario: Exact toolchain is selected
- **WHEN** rustc, cargo, clippy, and rustfmt match the repository declaration
- **THEN** the full locked target/feature/workspace/package matrix proceeds

#### Scenario: Toolchain selection drifts
- **WHEN** Rustup is missing, PATH selects Homebrew/system Cargo, an override is active, or any component differs from Rust 1.98.0
- **THEN** preflight reports expected and actual identities and fails before database, parser-pack, package, installer, or hosted mutation

#### Scenario: Future stable compiler is evaluated
- **WHEN** another stable compiler is proposed
- **THEN** a reviewed issue/PR runs the same complete matrix before the sole numeric declaration changes

### Requirement: Optional-parser capability has one closed typed authority
Containment, platform tuple, parser pack, installer, lifecycle, supervisor, runtime/MCP report, built-in fallback, feature gating, and tests SHALL derive from one exhaustive `PackPlatform`/capability authority.

#### Scenario: Accepted Linux or Windows tuple
- **WHEN** the tuple has the accepted pack and containment backend
- **THEN** install/update/verify/startup/parsing use the bounded worker with consistent limits, cancellation, crash, cleanup, and capability reporting

#### Scenario: Unsupported tuple
- **WHEN** no accepted pack/containment pair exists
- **THEN** every surface returns the same typed unavailability before mutation and never implies contained support

### Requirement: macOS Apple Silicon optional parsing is unavailable in v0.5
On macOS arm64, ProjectAtlas SHALL reject optional-parser install, update, verify, selection, and worker startup before any pack or persistent-state mutation. Built-in parsing SHALL remain available and SHALL be reported separately.

#### Scenario: Agent parses a supported built-in language on macOS arm64
- **WHEN** optional parsing is unavailable
- **THEN** built-in parsing/navigation succeeds while CLI, MCP, installer, and settings report optional parsing unavailable

#### Scenario: Stale or wrong pack is present
- **WHEN** owned stale/wrong optional-parser artifacts exist on macOS arm64
- **THEN** they are never selected or executed; only proven-owned cleanup may occur without representing capability as available

### Requirement: Release-owned target and feature builds are warning-clean
Every release-owned Rust 1.98.0 target/feature combination SHALL pass locked Cargo check and pedantic Clippy with warnings denied. Platform-specific optional-parser implementation SHALL compile only when reachable under the canonical capability; shared typed-unavailability and built-in fallback SHALL remain reachable.

#### Scenario: Current macOS matrix is already clean
- **WHEN** exact x64/arm64 reproduction produces no relevant diagnostics
- **THEN** #486 may close with reproducible no-product-change evidence

#### Scenario: A capability-owned warning reproduces
- **WHEN** impossible backend/lifecycle code is compiled for a tuple
- **THEN** only the smallest owning item/module cfg is tightened and no broad dead-code suppression, warning downgrade, or second platform matrix is added

### Requirement: Clean macOS arm64 proof consumes the packaged candidate
The release gate SHALL install the exact packaged macOS arm64 candidate into isolated HOME/config/cache/project state with no pre-existing database or checkout binary and SHALL verify path/version/digest before executing the lifecycle.

#### Scenario: Complete installed lifecycle
- **WHEN** the candidate is installed on actual macOS arm64 after #481/#482/#483/#484/#486
- **THEN** init/schema, scan, overview, files/summary/slice, MCP session/host config, `/var` alias, worktree lifecycle, watch, telemetry, symlink documents, built-in parsing, and typed optional-parser unavailability all pass

#### Scenario: Wrong root, failure, or cancellation
- **WHEN** the lifecycle addresses missing/unrelated state or an injected command/database failure occurs
- **THEN** it returns typed failure, preserves prior complete state, cleans proven-owned processes/files, and leaves no ambient or partial project state
