## Why

The Windows RC3 installed-predecessor upgrade diagnostic fails before migration fault injection because its test helper decodes SQLite's integer `registration_id` as text. This prevents the existing rollback, repair, and retry contract from executing; it does not demonstrate product data loss.

Correcting that read exposes a second fixture mismatch: the helper places its schema-19 backup at an alternate filename, while explicit legacy-root adoption correctly uses the selected root's canonical database. The injected migration fault remains unexercised until the fixture supplies that canonical state.

## What Changes

- Read and bind the native integer registration identity in the existing migration-repair helper.
- Match the typed root-identity error actually emitted while migration decodes the injected relative retired root; retain exact rollback and repair/retry assertions.
- Exercise the disposable schema-19 state from the retained compatible backup at the selected test root's canonical database path before candidate initialization, preserving ordinary product admission.
- Prove the regression through the existing real-SQLite upgrade contract and preserve complete fault, rollback, repair, retry, and authored-state checks.
- Prove the corrected Windows installed upgrade and required source gates, then restart fresh candidate acceptance under #492.

## Capabilities

### New Capabilities

- `released-upgrade-fixture-proof`: native registration identity, canonical fixture placement, and causal migration refusal, rollback, repair, and retry in the released-upgrade fixture.

### Modified Capabilities

None. The fixture-proof contract complements the unchanged installed-release acceptance contract from `prove-installed-release-upgrades-and-routes`; no product behavior or release acceptance requirement is weakened.

## Impact

Issue #666 owns the migration-repair helper in `crates/projectatlas-cli/tests/e2e_delivery.rs`, focused proof, and its IssueOps mapping. It is a direct child and blocker of release owner #492. The scope is ready for implementation after synchronized planning passes. Existing architecture, runtime, database schema, and dependencies remain unchanged.

## Non-Goals

No SQL identity coercion, runtime feature, migration, dependency, additional harness, relaxed assertion, publication, stable promotion, or unrelated state mutation.
