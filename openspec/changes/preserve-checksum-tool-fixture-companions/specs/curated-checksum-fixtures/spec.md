# Spec Delta

## Purpose

Preserve real host checksum execution inside isolated release regression fixtures so curated-path readiness and integrity proof remains portable and meaningful.

## ADDED Requirements

### Requirement: Curated fixtures retain real checksum execution

The curated release fixture SHALL preserve the selected host checksum tool's required executable companions and SHALL execute the real tool under the fixture's curated PATH.

#### Scenario: Version-dispatching macOS tool

- **WHEN** the fixture stages a real checksum dispatcher that requires versioned companion tools
- **THEN** the staged tool can execute its required companion and the existing installed readiness and integrity regression completes without a missing-alternative failure

#### Scenario: Standalone native checksum tool

- **WHEN** the selected native checksum tool requires no companion files
- **THEN** the existing staged invocation and curated-path behavior remain compatible

### Requirement: Integrity and isolation assertions remain authoritative

The regression SHALL retain its existing checksum, ready/stale state, curated-path, and non-mutation assertions without substituting a fake tool or ambient PATH fallback.

#### Scenario: The fixture exercises the intended boundary

- **WHEN** the installed hook is tested with the staged real checksum tool
- **THEN** all existing integrity and readiness assertions execute against isolated fixtures and no unrelated project state is mutated
