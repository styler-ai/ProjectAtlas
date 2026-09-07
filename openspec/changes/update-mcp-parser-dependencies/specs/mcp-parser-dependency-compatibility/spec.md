## ADDED Requirements

### Requirement: Dependency updates preserve the consumer contract

The accepted candidate SHALL lock rmcp and rmcp-macros to 3.2.0, tree-sitter-language to 0.1.8, and tree-sitter-language-pack to 1.16.1 while preserving existing dependency features, MCP behavior, and parser artifact and platform boundaries.

#### Scenario: Intended versions and features are selected

- **WHEN** the manifest and lockfile are inspected against accepted main
- **THEN** the intended dependency versions are selected without unrelated package updates
- **AND** language-pack default features remain disabled and dynamic loading remains enabled

#### Scenario: Existing MCP client uses stdio

- **WHEN** an existing client initializes ProjectAtlas and requests its supported stdio tool payloads
- **THEN** the server retains ProjectAtlas identity and the existing protocol, payload, and lifecycle behavior

#### Scenario: Wrong root or missing index is supplied

- **WHEN** an MCP request targets a mismatched project root or requires an absent index
- **THEN** the existing typed refusal or initialization guidance is preserved
- **AND** the dependency update does not introduce implicit mutation or substitute another project's state

#### Scenario: Optional parser loading is exercised

- **WHEN** the existing parser worker loads an accepted optional parser on a supported platform
- **THEN** the accepted artifact trust contract and built-in parser precedence remain enforced
- **AND** unsupported or invalid parser inputs retain their existing typed refusal behavior

### Requirement: Acceptance uses executable affected proof

The dependency update SHALL pass the existing required local, dependency-policy, and hosted platform checks that cover its consumers before closure.

#### Scenario: A consumer proof route is skipped

- **WHEN** a required MCP or optional parser compatibility boundary lacks executed proof in the selected checks
- **THEN** the update remains unaccepted until the existing owning proof route is executed successfully
- **AND** compilation or help output alone does not substitute for runtime compatibility evidence
