## Purpose

Admit reviewed, compatible dependency updates and prevent affected dependency advisories from silently entering release candidates.

## ADDED Requirements

### Requirement: Reported affected dependencies are replaced
The RC1 dependency graph SHALL exclude versions affected by the reported KaTeX advisory, preserve supported diagram validation and use normally generated locked package identities. Advisory suppression SHALL NOT substitute for the required update.

#### Scenario: Existing affected graph
- **WHEN** the existing Mermaid dependency graph selects KaTeX 0.16.47
- **THEN** the low-severity advisory is visible in actual audit evidence and fails the updated admission threshold

#### Scenario: Patched compatible graph
- **WHEN** the replacement graph is installed from its committed lockfile
- **THEN** its selected KaTeX version is outside the advisory range, the advisory no longer reproduces in the audit and valid/invalid Mermaid validation retains its contract on supported Node runners

### Requirement: Complete advisory inventory has an explicit disposition
All open Dependabot security alerts SHALL be inventoried with provenance at admission and rechecked at closure. Each SHALL have an accepted fix, evidence-backed not-applicable disposition, or explicit unresolved RC-blocker status. The complete set SHALL NOT be inferred from the update-PR list or a single known alert.

#### Scenario: Additional security alert
- **WHEN** admission or closure readback returns an alert beyond the initially reported KaTeX advisory
- **THEN** its actual affected graph and supported boundary are evaluated, with required remediation or a documented evidence-backed disposition before RC1 readiness

### Requirement: Updates preserve owning safety and compatibility contracts
Dependency and Action updates SHALL be reviewed against upstream provenance and affected callers, retain central version ownership and SHA pins, and preserve filesystem, parser, MCP, resource, platform and artifact-integrity contracts. Required behavior changes SHALL use the existing versioned invalidation and compatibility mechanisms.

#### Scenario: Updated parsing or identifier behavior
- **WHEN** a dependency changes extracted or resolved facts for unchanged source
- **THEN** the existing owning fingerprint causes compatible indexes to repair without resetting identity or approved purposes, with subsequent no-op and compatible rollback behavior verified

#### Scenario: Identifier table version advances
- **WHEN** an accepted dependency update advances the Python normalizer's XID property tables
- **THEN** the current profile and semantic fingerprint identify that table version, while exact source spelling, underscore support, hard-keyword rejection, join-control rejection and ordinary Unicode/ASCII aliases remain preserved

#### Scenario: Malformed or repeated Markdown links
- **WHEN** the Markdown parser receives the supported malformed-link or repeated-wikilink regression inputs
- **THEN** extraction returns bounded facts without a process panic or duplicated exponential source occurrences, while ordinary Markdown navigation remains available

#### Scenario: Updated filesystem or workflow dependency
- **WHEN** an accepted update affects filesystem classification or artifact/package handoff
- **THEN** the owning safety and platform checks still pass, uncertain state fails closed and prior unrelated proof is not misreported as current evidence

### Requirement: Dependency monitoring covers the affected ecosystems
The repository SHALL retain Cargo and Actions update monitoring, add npm monitoring for its actual diagram-validator manifest, and fail the existing npm audit gates for low-or-higher advisories without reducing other security checks.

#### Scenario: Low-severity npm advisory
- **WHEN** a selected dependency has a low-severity advisory
- **THEN** the normal audit gate refuses admission instead of passing solely because the former threshold was moderate

#### Scenario: Manifest update monitoring
- **WHEN** scheduled dependency monitoring evaluates the repository
- **THEN** its configuration includes Cargo, GitHub Actions and the diagram-validator npm directory without enabling unreviewed automatic merges
