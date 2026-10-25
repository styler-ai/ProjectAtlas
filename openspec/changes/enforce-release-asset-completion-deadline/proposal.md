## Why

The release-asset test server can consume completion after its absolute deadline and report missing requests or success instead of timeout. The existing macOS ARM lifecycle gate reproduced this ordering gap, blocking release acceptance.

## What Changes

- Enforce expiry in the existing shared completion decision across all receive paths.
- Bound idle waiting by remaining time and retain operation-specific timeout diagnostics.
- Extend the existing lifecycle regression with deterministic decision cases and preserve real socket/process proof.

## Capabilities

### New Capabilities
- `release-asset-completion-deadline`: Completion cannot override an expired test-server deadline.

### Modified Capabilities
None.

## Impact

Ready for implementation under #564 in v0.5.0-00, release owner #492. Changes are confined to the existing delivery E2E owner and required frozen source identities. Publish the accepted shared baseline before refreshing preserved #477 and #390 work. No open implementation prerequisite.

## Non-Goals

No timeout inflation, scheduler hooks, new framework, dependencies, database migration, installed-product change, or CI routing redesign.
