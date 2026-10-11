# Spec Delta

## Purpose

Preserve real host checksum execution inside isolated release regression fixtures so curated-path readiness and integrity proof remains portable and meaningful.

## ADDED Requirements

### Requirement: Curated fixtures retain real checksum execution

The curated release fixture SHALL expose the selected real host checksum implementation under the existing command name and SHALL execute it under the fixture's curated PATH.

#### Scenario: Version-dispatching macOS tool

- **WHEN** the selected generic macOS `shasum` launcher depends on interpreter-prefix dispatch to a versioned implementation
- **THEN** the curated `shasum` name resolves directly to the real implementation selected by the active host, without guessing a version suffix, and the installed readiness and integrity regression completes without a missing-alternative failure

#### Scenario: Standalone native checksum tool

- **WHEN** the selected native checksum tool needs no version dispatch
- **THEN** its existing staged invocation and curated-path behavior remain compatible

### Requirement: Integrity and isolation assertions remain authoritative

The regression SHALL retain its existing checksum, ready/stale state, curated-path, and non-mutation assertions without substituting a fake tool or ambient PATH fallback.

#### Scenario: The fixture exercises the intended boundary

- **WHEN** the installed hook is tested with the staged real checksum implementation
- **THEN** all existing integrity and readiness assertions execute against isolated fixtures and no unrelated project state is mutated

### Requirement: Native proof exercises the affected checksum path

Acceptance SHALL prove the corrected standalone installed regression on both native macOS architectures with the real `shasum` path selected.

#### Scenario: Native installed execution

- **WHEN** the curated fixture has no `sha256sum`, records its resolved real `shasum` implementation, and computes a known SHA-256 vector through `shasum -a 256`
- **THEN** the standalone regression runs against the checksum-verified installed package with Cargo's binary variable absent and every existing assertion remains enabled
