## ADDED Requirements

### Requirement: Existing Dependabot updates enter RC3 under the existing aggregate owner
The existing PRs #623, #635, and #636 SHALL each reference the existing aggregate issue #499 with an explicit ownership line; intermediate PRs MUST use non-closing references and #499 MUST remain open until complete aggregate acceptance and matching v0.5.0-00 milestone. The RC3 release owner SHALL remain the sole direct parent and final blocker consumer. The grouped update from #636 MUST refresh after the accepted #623 Cargo.lock baseline and be delivered through its same-issue successor with compliant commit subjects; #635 SHALL remain independent of the Cargo updates. No bot PR SHALL merge until its current head has required affected local and hosted proof plus resolved review feedback.

#### Scenario: Independent action update
- **WHEN** #639 is accepted and PR #635 is refreshed with #499 as its sole owner
- **THEN** its pinned-action installation and affected hosted workflow proof can be accepted independently of the Cargo PRs

#### Scenario: Cargo lockfile ordering
- **WHEN** PR #636 is prepared while PR #623 is not yet accepted on main
- **THEN** the grouped update remains blocked; after #623 merges, its issue-owned successor uses that accepted baseline and reruns affected proof

#### Scenario: Original bot history cannot satisfy the commit-owner gate
- **WHEN** the grouped bot branch contains commit subjects that the mandatory pre-push ownership gate refuses
- **THEN** preserve that branch and deliver its exact reviewed update through one same-issue successor PR based on accepted main with compliant commit subjects, without rewriting bot history or weakening the gate
- **AND** require complete current-head local, hosted, parser-pack, and independent acceptance before superseding the bot PR and closing #499 through the successor

### Requirement: Dependency updates preserve active product boundaries
The jsonc-parser update SHALL preserve supported JSONC configuration parsing and typed invalid-input behavior. The install-action update SHALL preserve pinned toolchain installation on supported CI/release runners. The grouped Cargo update SHALL use supported RMCP server/client configuration APIs without changing MCP initialize, tool, selected-root, or missing-index semantics, and SHALL validate optional parser-pack construction and native loading on supported Windows/Linux hosts plus typed unavailability and unchanged built-in parsing on macOS. All updates MUST retain warnings-as-errors and existing security, release, and installed-product gates.

#### Scenario: JSONC compatibility
- **WHEN** valid and invalid JSONC configuration fixtures run against the refreshed #623 dependency
- **THEN** accepted values and typed refusals match the supported contract

#### Scenario: MCP selected-root and missing-index behavior
- **WHEN** the refreshed grouped-update successor runtime receives MCP initialize and tool calls for the selected root, a wrong root, and a root lacking an index
- **THEN** supported calls preserve their response contracts, while wrong-root and missing-index requests fail with the existing typed state and no implicit project mutation

#### Scenario: Optional parser pack on supported hosts
- **WHEN** the updated tree-sitter-language-pack is built, installed, enabled, and exercised on supported Windows and Linux hosts
- **THEN** declared grammars load with the expected identities and failures remain contained without corrupting the active index

#### Scenario: Optional parser pack on macOS
- **WHEN** optional-pack activation is requested on macOS x64 or arm64
- **THEN** it returns typed `unsupported_containment` before worker launch or source transfer and preserves the built-in parser surface

#### Scenario: Incompatible dependency candidate
- **WHEN** any affected platform, protocol, parser, or installer proof fails
- **THEN** the owning PR stays unmerged and RC3 release acceptance remains blocked until the owning fix and current-head proof pass

### Requirement: Residual Mermaid dependency alerts use the aggregate owner
A residual dependency security alert found before RC3 publication SHALL remain under #499 and SHALL block release acceptance until its compatible patch and affected proof are accepted. The Mermaid lockfile patch MUST preserve unrelated dependency versions and its manifest range, prove valid and invalid parsing after locked installation, and inspect all low-severity audit findings, remediate compatible patches or explicitly disposition a remaining low finding against the actual parser boundary and upstream range, while passing the existing repository audit gate and required current-head checks.

#### Scenario: Patched sanitizer is already compatible
- **WHEN** the locked DOMPurify and source-map-js versions are affected by GHSA-p98j-92pf-mc4p and GHSA-68fv-2mgg-jv7q, and the existing dependency ranges admit patched DOMPurify 3.4.16 and source-map-js 1.2.2
- **THEN** update only those lockfile packages under reopened #499, preserve completed aggregate tasks, and close the aggregate after accepted merge and default-branch alert readback
