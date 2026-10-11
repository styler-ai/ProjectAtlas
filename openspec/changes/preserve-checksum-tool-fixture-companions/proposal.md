## Why

The installed macOS ARM regression stops in the curated checksum-tool fixture because the relocated `shasum` wrapper cannot find an alternative implementation. The release regression must preserve the real host tool's executable dependencies so its readiness and integrity assertions can complete.

## What Changes

- Preserve required checksum-tool companions in the existing temporary curated-path fixture.
- Retain the real checksum implementation, curated PATH assertion, readiness/integrity checks, and ordinary native-tool behavior.
- Confirm the fixture diagnosis and correction with the existing focused native macOS regression and required affected checks.
- Hand fresh complete candidate acceptance to #492 after the owning fix merges.

## Capabilities

### New Capabilities

- `curated-checksum-fixtures`: executable real-host checksum tools in isolated release regression fixtures without weakening integrity proof.

### Modified Capabilities

None. Production checksum routing and installed hook behavior remain unchanged.

## Impact

#669 owns checksum-tool staging in `crates/projectatlas-cli/tests/e2e_lifecycle.rs`, affected frozen-source metadata, and causal native macOS proof. The fixture currently links the generic tool but omits any required versioned siblings. Planning remains backlog until published specification and release admission; implementation follows the already-started #668 boundary.

## Non-Goals

No new checksum implementation, product hook feature, dependency, database migration, weakened assertion, real Nix-host acceptance claim, release publication, or stable promotion.
