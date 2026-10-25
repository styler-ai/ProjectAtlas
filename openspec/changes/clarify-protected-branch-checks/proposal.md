# Proposal

## Why

A manual CI run can pass on the exact candidate commit while GitHub still reports required protected-branch checks as missing. Our workflow guide mentions manual fallback runs without explaining this event restriction.

## What Changes

- Distinguish diagnostic manual runs from the existing PR checks that GitHub accepts for protected branches.
- Link the official restriction and retain the current main-based integration policy.
- Keep short-lived branches and reviewed PRs into main as the integration workflow.

## Capabilities

### New Capabilities

- `protected-branch-guidance`: maintainer instructions that distinguish eligible checks from diagnostics and preserve the existing integration and history rules.

### Modified Capabilities

None. Runtime behavior, workflow definitions and branch protection do not change.

## Impact

Only workflow documentation, this OpenSpec packet and its native issue/release ownership. Runtime and CI definitions remain unchanged.

## Release Scope

Ready for documentation work in 0.5.1. Its release owner remains feature-free. This work does not implement the indexing performance improvements or publish a version.

## Non-Goals

No new CI mechanism, check forgery, skipped gate, force push, protection change, runtime change, automatic dev mirror or migration to dev-first integration.
