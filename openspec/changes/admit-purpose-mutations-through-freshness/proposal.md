# Change: Admit Purpose Mutations Through Source Freshness

## Why

Purpose queue tokens are generation-bound, but CLI and MCP mutation paths can open the purpose writer without first reconciling current saved source. A source edit after queue issuance can therefore leave the database on the old generation while the conditional write succeeds. Later refreshes correctly preserve the newly authored purpose, allowing an outdated purpose and a falsely clean queue to persist.

## What Changes

- Reuse the existing source-freshness admission once per purpose mutation batch and retain its witness through commit.
- Publish current source changes before conditional work is evaluated, then revalidate the witness immediately before commit so stale work and post-admission edits change no authored state.
- Apply the same admission to explicit purpose set and review operations for deleted, renamed, ignored, or unrefreshed paths.
- Preserve approved authored purpose across unchanged scans and keep the existing conditional transaction and token schema.
- Add real CLI and persistent MCP stale-mutation tests plus a same-binding queue/watch no-op regression to mandatory platform and packaged release verification.

## Capabilities

### New Capabilities

- `fresh-purpose-mutation-admission`: Purpose mutations consult current saved-source authority before the existing SQLite write boundary.

### Modified Capabilities

- `purpose-curation`: Purpose mutation rejects stale source-derived work, unchanged approved purposes remain durable, and the existing same-binding queue/watch no-op path remains regression-protected.

## Impact

The change is limited to the existing CLI and MCP freshness orchestration, the caller-owned purpose transaction guard, mutation adapters, and their tests and release proof. It adds no schema, token, table, index, crate, or dependency.
