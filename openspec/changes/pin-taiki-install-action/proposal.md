## Why

The two workflow uses of `taiki-e/install-action` still carry the `v2.86.6` commit pin, leaving CI and release cargo-deny setup behind the current `v2.87.5` maintenance update. This small release-scoped change keeps the action immutable while preserving the existing cargo-deny tool policy.

## What Changes

- Replace the `taiki-e/install-action` SHA in `.github/workflows/ci.yml` and `.github/workflows/release.yml` with the `v2.87.5` commit `5bf6ce016fd2e72eefc647cbca1e4213f65955b8`.
- Preserve `cargo-deny@0.20.2`, workflow conditions, permissions, jobs, and all other workflow behavior.
- Verify the exact two-use source diff and the normal affected CI/release proof.

## Capabilities

### New Capabilities

- `ci-action-pinning`: CI and release workflow actions remain pinned to the reviewed immutable commit.

### Modified Capabilities

None.

## Impact

Ready for implementation under the v0.5.0-00 release scope and native release owner #492. The change is limited to two existing workflow lines plus this OpenSpec and its issue-map ownership record. It adds no runtime code, dependency, test framework, database migration, or product behavior.

## Non-Goals

- No workflow redesign, job or permission change, or cargo-deny version update.
- No action auto-merge policy, Dependabot configuration, or unrelated dependency work.
- No product, CLI, MCP, database, installer, or release-owner implementation.
