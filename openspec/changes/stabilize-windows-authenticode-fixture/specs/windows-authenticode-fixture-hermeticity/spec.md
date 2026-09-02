## ADDED Requirements

### Requirement: Hermetic Windows Authenticode fixture
The Windows installer Authenticode E2E fixture SHALL control the child PowerShell inputs needed to exercise the production module-qualified trusted-cmdlet path, independent of the parent shell, profile, and inherited module state.

#### Scenario: Supported parent shells agree
- **WHEN** the fixture runs from PowerShell, `cmd.exe`, its serialized installer subgroup, or the normal parallel Windows E2E suite
- **THEN** it produces the same trust-boundary result without retries or a global suite lock

#### Scenario: Product behavior remains unchanged
- **WHEN** the fixture establishes its controlled child environment
- **THEN** it exercises the existing installer identity functions without changing product installer, obsolete-MCP handoff, CLI, or MCP behavior

### Requirement: Complete fail-closed trust proof
The fixture SHALL prove the trusted module-qualified lookup, signer, signature status and type, stable pre/post image digest, unsigned-image rejection, and malicious unqualified command-shadow rejection as separate causal invariants.

#### Scenario: Trusted signature facts are accepted
- **WHEN** the synthetic signature has the required signer, valid status, Authenticode type, and matching pre/post image digest
- **THEN** the existing trust predicates accept those facts

#### Scenario: Invalid trust facts fail closed
- **WHEN** the input is unsigned, a malicious unqualified lookup or signature command is present, the signer/status/type is invalid, or the image digest changes
- **THEN** the relevant trust predicate rejects it and the failure identifies the violated invariant
