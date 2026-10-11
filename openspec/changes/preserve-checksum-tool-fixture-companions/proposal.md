## Why

The installed macOS ARM regression stops because the relocated generic `shasum` script cannot dispatch to its real versioned implementation. Apple's Perl versioner rewrites script arguments only beneath the interpreter's original directory prefix; adding versioned siblings beside the relocated script does not restore that dispatch boundary. The fixture must execute the real host checksum implementation while retaining its curated PATH.

## What Changes

- Resolve the host's real checksum implementation and expose it under the existing curated command name.
- On affected macOS hosts, bind the curated `shasum` name to the real implementation selected by the active host Perl version, without guessing a version suffix.
- Preserve standalone native-tool behavior, every curated-path, checksum, readiness, stale-state, and non-mutation assertion, and production hook routing.
- Prove the affected `shasum` path on both native macOS architectures before handing fresh complete candidate acceptance to #492.

## Capabilities

### New Capabilities

- `curated-checksum-fixtures`: executable real-host checksum tools in isolated release regression fixtures without weakening integrity proof.

### Modified Capabilities

None. Production checksum routing and installed hook behavior remain unchanged.

## Impact

#669 owns checksum-tool staging in `crates/projectatlas-cli/tests/e2e_lifecycle.rs`, the two existing frozen lifecycle source-hash owners, causal native macOS proof, and the existing Unix installed-candidate step budget needed to complete that proof. Correct the existing issue and OpenSpec contract in the owning change; retain its slug and release mapping. Implementation follows acceptance of the already-started #668 boundary.

## Non-Goals

No new checksum implementation, product hook feature, dependency, workflow, environment selector, database migration, weakened assertion, ambient PATH fallback, real Nix-host acceptance claim, release publication, or stable promotion.
