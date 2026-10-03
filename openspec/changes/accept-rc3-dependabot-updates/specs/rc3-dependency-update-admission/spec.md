## ADDED Requirements

### Requirement: Existing Dependabot updates enter RC3 under distinct owners
The existing PRs #623, #635, and #636 SHALL each reference exactly one open ProjectAtlas issue with a closing line and matching v0.5.0-00 milestone. The RC3 release owner SHALL remain the sole direct parent and final blocker consumer. #636 MUST refresh after the accepted #623 Cargo.lock baseline; #635 SHALL remain independent of the Cargo updates. No bot PR SHALL merge until its current head has required affected local and hosted proof plus resolved review feedback.

#### Scenario: Independent action update
- **WHEN** #639 is accepted and PR #635 is refreshed with #641 as its sole owner
- **THEN** its pinned-action installation and affected hosted workflow proof can be accepted independently of the Cargo PRs

#### Scenario: Cargo lockfile ordering
- **WHEN** PR #636 is prepared while PR #623 is not yet accepted on main
- **THEN** #636 remains blocked; after #623 merges, #636 refreshes onto that baseline and reruns affected proof

### Requirement: Dependency updates preserve active product boundaries
The jsonc-parser update SHALL preserve supported JSONC configuration parsing and typed invalid-input behavior. The install-action update SHALL preserve pinned toolchain installation on supported CI/release runners. The grouped Cargo update SHALL use supported RMCP server/client configuration APIs without changing MCP initialize, tool, selected-root, or missing-index semantics, and SHALL validate optional parser-pack construction and native loading on supported platforms. All updates MUST retain warnings-as-errors and existing security, release, and installed-product gates.

#### Scenario: JSONC compatibility
- **WHEN** valid and invalid JSONC configuration fixtures run against the refreshed #623 dependency
- **THEN** accepted values and typed refusals match the supported contract

#### Scenario: MCP selected-root and missing-index behavior
- **WHEN** a refreshed #636 runtime receives MCP initialize and tool calls for the selected root, a wrong root, and a root lacking an index
- **THEN** supported calls preserve their response contracts, while wrong-root and missing-index requests fail with the existing typed state and no implicit project mutation

#### Scenario: Optional parser pack on supported hosts
- **WHEN** the updated tree-sitter-language-pack is built, installed, enabled, and exercised on each supported release platform
- **THEN** declared grammars load with the expected identities and failures remain contained without corrupting the active index

#### Scenario: Incompatible dependency candidate
- **WHEN** any affected platform, protocol, parser, or installer proof fails
- **THEN** the owning PR stays unmerged and RC3 release acceptance remains blocked until the owning fix and current-head proof pass
