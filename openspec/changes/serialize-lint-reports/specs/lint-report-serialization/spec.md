## ADDED Requirements

### Requirement: Lint exposes one typed result
The system SHALL expose one deterministic lint result containing `ok`, the CLI-compatible `exit_code`, structured map lint facts, optional structured SQLite health facts, and a human-readable compatibility report.

#### Scenario: Clean repository
- **WHEN** all selected lint checks pass
- **THEN** the result has `ok: true`, `exit_code: 0`, typed empty or zero-valued sections, and deterministic output

#### Scenario: Blocking findings
- **WHEN** any selected map or SQLite lint check blocks
- **THEN** the result has `ok: false`, preserves the highest applicable exit code, and identifies each blocking fact in its owning structured section

#### Scenario: Missing index
- **WHEN** lint runs for a project with no SQLite index and map lint can run
- **THEN** the result contains no index section, performs no initialization or scan, and reports the map lint outcome

### Requirement: CLI lint honors the global output format and stream contract
The CLI SHALL serialize the typed lint result through the standard output adapter on stdout and SHALL reserve stderr for execution errors and diagnostics.

#### Scenario: JSON output
- **WHEN** `projectatlas --format json lint` completes its checks
- **THEN** stdout contains a parseable JSON `lint` payload, stderr contains no lint report, and the process exits with the payload's `exit_code`

#### Scenario: TOON output
- **WHEN** `projectatlas --format toon lint` completes its checks
- **THEN** stdout contains a TOON `lint` payload distinct from JSON, stderr contains no lint report, and the process exits with the payload's `exit_code`

#### Scenario: Output write failure
- **WHEN** stdout cannot accept or flush the serialized lint payload
- **THEN** the CLI returns an output error instead of silently reporting the lint exit code

### Requirement: MCP lint shares the CLI report owner
`atlas_lint` SHALL return the same typed lint report and blocking semantics as CLI lint without terminating the MCP transport.

#### Scenario: CLI and MCP parity
- **WHEN** CLI lint and `atlas_lint` run with equivalent options against the same current project
- **THEN** their typed lint facts, `ok`, and `exit_code` agree

#### Scenario: Explicit project root
- **WHEN** `atlas_lint` receives a valid explicit `project_path`
- **THEN** it lints only that addressed project and returns its typed result without changing the process default root

#### Scenario: Wrong root
- **WHEN** `atlas_lint` receives a root or index identity mismatch
- **THEN** it returns the existing typed routing or preflight error and does not substitute another project's result

#### Scenario: No implicit mutation
- **WHEN** CLI or MCP lint reads a current project
- **THEN** it performs no scan, purpose write, configuration edit, or database mutation

### Requirement: Lint behavior is platform and language neutral
The system SHALL apply the same serialization and stream contract on supported Windows, Linux, and macOS runtimes and SHALL not branch on indexed source language.

#### Scenario: Packaged runtimes
- **WHEN** the installed candidate runs the lint format contract on each supported platform
- **THEN** JSON and TOON payload shapes, stdout/stderr ownership, and exit-code semantics match
