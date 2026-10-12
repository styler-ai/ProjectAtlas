## Purpose

Keep released-upgrade fixtures faithful to native database identities and shipped plugin assets so native acquisition, migration failure, preservation, repair, and retry evidence remains causal.

## ADDED Requirements

### Requirement: Native plugin proof respects release-specific shipped assets
The existing native plugin assertion SHALL compare predecessor manifest and skill bytes against the exact published predecessor source. It SHALL require all current-candidate references, instructions, and readiness assets in its existing current-version branch, retaining mandatory reads and exact source/cache comparison. Missing historical current-only files SHALL NOT prevent predecessor acquisition proof; missing or mismatched required current files SHALL fail acceptance.

#### Scenario: Published predecessor lacks newer plugin assets
- **WHEN** native Codex acquires the exact v0.4.5 plugin, which ships its manifest and skill but not the newer reference and instruction files
- **THEN** its shipped assets are compared strictly and the upgrade contract reaches candidate installation

#### Scenario: Current candidate lacks or changes a required cached asset
- **WHEN** a required current-candidate plugin asset is missing or differs from the candidate source
- **THEN** the existing native plugin assertion refuses acceptance

#### Scenario: Diagnostic mode skips native acquisition
- **WHEN** prepublish-only mode skips Codex plugin assertions
- **THEN** a passing diagnostic is not counted as full native predecessor or candidate plugin proof

### Requirement: Upgrade repair fixtures preserve native registration identity
The released-upgrade migration-repair fixture SHALL decode and bind worktree registration identities using the integer type declared by the database schema. It SHALL retain parameterized queries and select the intended registration without changing production schema or coercing identity to text.

#### Scenario: Released registration has an integer key
- **WHEN** the migration-repair helper reads an exercised predecessor registration with an integer primary key
- **THEN** it reads and binds that key successfully and reaches the existing migration-fault injection

### Requirement: Registration repair retains causal upgrade proof
The fixture SHALL preserve its existing injected migration refusal, atomic rollback, authored-state comparison, row repair, successful retry, and compatible recovery assertions. Focused proof SHALL exercise the owning read against real SQLite state; complete installed acceptance remains mandatory under the release owner.

#### Scenario: Invalid registration is repaired and retried
- **WHEN** a disposable predecessor backup receives the existing invalid registration state
- **THEN** candidate migration refuses without losing authority, the fixture repairs the same integer-keyed row, and valid retry preserves the expected authored state

#### Scenario: Focused proof succeeds
- **WHEN** the focused registration regression passes
- **THEN** that result does not substitute for the installed predecessor upgrade contract or full supported-platform release acceptance

### Requirement: Legacy adoption fixtures use canonical selected-root state
The fixture SHALL preserve the earlier MCP adoption proof, restore the retained compatible schema-19 snapshot and WAL profile at the isolated root's canonical database, then exercise injected CLI adoption refusal, rollback, repair, and retry before normal candidate init and scan. It SHALL retain the compatible backup and leave product root-adoption admission unchanged.

#### Scenario: Alternate database filename cannot supply adoption state
- **WHEN** a schema-19 copy exists at an alternate filename while the selected root lacks eligible canonical state
- **THEN** legacy adoption refuses and that refusal is not counted as proof of the intended injected migration fault

#### Scenario: Canonical fixture reaches migration fault injection
- **WHEN** the retained compatible predecessor backup supplies schema-19 state at the selected test root's canonical path before the injected migration attempt and normal candidate init
- **THEN** the existing injected invalid registration exercises migration refusal, rollback, same-row repair, and successful retry without unrelated mutation
