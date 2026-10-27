## Purpose

Maintain automatic repository security analysis with verified coverage, truthful finding assessment and preserved intended product functionality.

## ADDED Requirements

### Requirement: Source and workflow analysis runs automatically
The repository SHALL enable supported managed source/workflow security scanning on its ordinary PR/push triggers and a weekly schedule. Configuration readback and completed successful analyses SHALL establish coverage; unsupported or failed language analysis SHALL be recorded separately.

#### Scenario: Initial scanner setup
- **WHEN** automatic source scanning is enabled
- **THEN** initial jobs and retained language configuration are verified, including authored Rust and repository scripting/workflow owners, without claiming coverage from failed analyses

#### Scenario: Parser fixtures lack a language build
- **WHEN** an analyzed language appears only in parser fixtures without a suitable application build
- **THEN** the failed or unsupported analysis is documented without inventing an application build, disabling successful authored-source analysis or calling the fixture language covered

### Requirement: PR security review has explicit repository policy
Online Codex automatic security review SHALL cover all repository PRs on every push and report low-or-higher findings. Stored repository settings SHALL be verified after reload, and actual review activity SHALL be checked on a subsequent applicable PR revision.

#### Scenario: Repository settings persist
- **WHEN** repository automatic security review is configured
- **THEN** reloading its authenticated settings retains enabled review, all-PR scope, every-push trigger and low minimum automatic severity

#### Scenario: New PR revision
- **WHEN** a subsequent applicable PR revision is pushed
- **THEN** its online security-review activity is checked independently from ordinary Codex code review and local independent review

### Requirement: Findings require supported-boundary evidence
Every imported initial finding SHALL retain provenance and receive an evidence-based disposition against current source, controls and supported trust boundaries. An alert or vulnerable package alone SHALL NOT be called an exploitable product vulnerability. Confirmed RC1 failures SHALL be fixed through their owning boundary and independently reviewed.

#### Scenario: Trusted operator or test-only path
- **WHEN** a scanner reports an intentional trusted-operator capability or a test helper
- **THEN** its reachability, policy and counterevidence determine disposition without automatically removing the capability or dismissing unrelated findings

#### Scenario: Confirmed supported-boundary failure
- **WHEN** source evidence confirms a reachable failure across a supported security boundary
- **THEN** the owning fix and broader implications are tracked and repaired for RC1 with causal and legitimate-behavior coverage

### Requirement: Security remediation preserves intended functionality
Security fixes SHALL retain documented CLI/MCP, indexing, navigation, plugin, filesystem-selection and release capabilities. Legitimate positive workflows SHALL be verified alongside exploit/failure controls. Blanket restrictions or undocumented compatibility narrowing SHALL NOT substitute for a targeted repair.

#### Scenario: Legitimate use of an affected boundary
- **WHEN** a security repair changes admission or processing at a supported boundary
- **THEN** ordinary documented workflows continue to succeed while the demonstrated unsafe case is refused or safely handled

### Requirement: Dependency and secret protections remain active
Existing Dependabot alerts, automatic security-update proposals, secret scanning and secret push protection SHALL remain enabled. Verification SHALL distinguish enabled protections, proposed remediation and actual accepted fixes without enabling unreviewed automatic merges.

#### Scenario: Protection readback
- **WHEN** repository security configuration is accepted
- **THEN** each available existing protection is read back successfully and pending advisories or updates remain visible until their reviewed remediation is accepted
